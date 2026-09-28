//! The generation worker (PLAN.md M1/M3, `llm` feature): a plain
//! `std::thread` that owns the model and authors one section at a time
//! through the tier chain — **agent build → recipe → nothing** — plus the
//! app-side glue that enqueues uncached sections and applies results as
//! they arrive.
//!
//! The split mirrors Verse's analysis worker: heavy model state lives off
//! the main thread (a multi-GB GGUF never touches the renderer's memory),
//! work and results cross on `std::sync::mpsc` channels drained once per
//! frame. The draft renders immediately — every section with a cached build
//! or recipe is authored from the sidecar on load — and each remaining
//! region upgrades in place when its result lands. The rebuild is cheap and
//! keyed to stable entity ids, so the tour stop you're looking at stays
//! put.
//!
//! Failures are sticky for the run: a section whose whole chain failed is
//! not retried until the app restarts (Verse's Tier rule — a missing model
//! does not appear mid-run). Editing the section changes its hash, which
//! does retry it.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::mpsc;

use bevy::prelude::*;

use crate::llm;
use crate::scene::CurrentWorld;
use localgpt_md::assets::AssetManifest;
use localgpt_md::recipe::RegionRecipe;
use localgpt_md::sidecar::{BuildEntry, RecipeStore};

pub struct GenerationPlugin;

impl Plugin for GenerationPlugin {
    fn build(&self, app: &mut App) {
        let (job_tx, job_rx) = mpsc::channel::<(blake3::Hash, Job)>();
        let (outcome_tx, outcome_rx) = mpsc::channel::<Outcome>();
        std::thread::spawn(move || worker(job_rx, outcome_tx));
        app.insert_resource(GenState {
            job_tx,
            // `mpsc::Receiver` is !Sync; the Mutex makes the resource shareable
            // (only the main thread ever locks it).
            outcome_rx: Mutex::new(outcome_rx),
            pending: HashSet::new(),
            failed: HashSet::new(),
        })
        .add_systems(Update, (enqueue_uncached, drain_outcomes).chain());
    }
}

/// One section to author. The body crosses as a prompt-ready excerpt so the
/// worker needs no access to app state.
pub(crate) struct Job {
    pub(crate) heading: String,
    pub(crate) excerpt: String,
    pub(crate) genre: String,
}

/// What the worker produced for one section — the first tier that
/// succeeded, or `None` (the draft stands; the failure is sticky).
struct Outcome {
    hash: blake3::Hash,
    result: Option<SectionOutput>,
}

pub(crate) enum SectionOutput {
    Build(BuildEntry),
    Recipe(RegionRecipe),
}

#[derive(Resource)]
struct GenState {
    job_tx: mpsc::Sender<(blake3::Hash, Job)>,
    outcome_rx: Mutex<mpsc::Receiver<Outcome>>,
    /// Section-hash keys with a job in flight.
    pending: HashSet<String>,
    /// Hashes that failed this run; not retried (see module docs).
    failed: HashSet<String>,
}

/// The worker thread: loads the model and the asset manifest on the first
/// job — so a document whose sidecar is complete never pays for either —
/// and keeps both resident.
fn worker(jobs: mpsc::Receiver<(blake3::Hash, Job)>, outcomes: mpsc::Sender<Outcome>) {
    let mut model: Option<llm::RecipeModel> = None;
    let mut manifest: Option<AssetManifest> = None;
    while let Ok((hash, job)) = jobs.recv() {
        if model.is_none() {
            model = llm::RecipeModel::try_load();
            manifest = localgpt_md::assets::read_manifest_from_disk();
        }
        let result = generate_for(model.as_mut(), manifest.as_ref(), &hash, &job);
        if outcomes.send(Outcome { hash, result }).is_err() {
            return; // app gone; stop
        }
    }
}

/// The tier chain for one section (PLAN.md M3): an agent build first — the
/// model composes the place through tool calls — falling back to a recipe
/// (one styled JSON) when the session yields nothing. `None` when both
/// tiers fail or no model is loaded. Shared by the worker and
/// `--generate`.
pub(crate) fn generate_for(
    model: Option<&mut llm::RecipeModel>,
    manifest: Option<&AssetManifest>,
    hash: &blake3::Hash,
    job: &Job,
) -> Option<SectionOutput> {
    let model = model?;
    let key = hash.to_hex().to_string();
    if let Some(build) = localgpt_md::agent::run_session(
        model.model_mut(),
        &key,
        &job.heading,
        &job.excerpt,
        &job.genre,
        manifest,
    ) {
        return Some(SectionOutput::Build(BuildEntry {
            model: model.name().to_string(),
            description: build.description,
            entities: build.entities,
        }));
    }
    model
        .generate(&job.heading, &job.excerpt, &job.genre)
        .map(SectionOutput::Recipe)
}

/// Send a job for every section that has neither a cached build, a job in
/// flight, nor a failure on record — and no ```` ```world ```` fence, which
/// makes the section exact without any LLM. Runs every frame (the loop is a
/// few hash-set lookups per section) so it picks up startup, hot reloads,
/// and anything the model just finished without extra bookkeeping.
fn enqueue_uncached(world: Res<CurrentWorld>, store: Res<RecipeStore>, mut jobs: ResMut<GenState>) {
    for section in &world.doc.sections {
        let key = section.hash.to_hex().to_string();
        if section.world.is_some()
            || store.get_build(&section.hash).is_some()
            || jobs.pending.contains(&key)
            || jobs.failed.contains(&key)
        {
            continue;
        }
        let job = (
            section.hash,
            Job {
                heading: section.heading.clone(),
                excerpt: llm::prompt_excerpt(&section.body),
                genre: world.doc.genre().to_string(),
            },
        );
        if jobs.job_tx.send(job).is_ok() {
            jobs.pending.insert(key);
        }
    }
}

/// Apply whatever the worker finished: cache it, persist the sidecar, and
/// recompile the manifest (which reauthors just the finished regions — the
/// rest keep the draft, cached output stays put). Writing the resource is
/// what triggers the scene rebuild and keeps the tour stop.
fn drain_outcomes(
    mut world: ResMut<CurrentWorld>,
    mut store: ResMut<RecipeStore>,
    mut jobs: ResMut<GenState>,
) {
    let outcomes: Vec<Outcome> = {
        let rx = jobs.outcome_rx.lock().unwrap();
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    };
    let mut builds = 0usize;
    let mut recipes = 0usize;
    for outcome in outcomes {
        let key = outcome.hash.to_hex().to_string();
        jobs.pending.remove(&key);
        match outcome.result {
            Some(SectionOutput::Build(entry)) => {
                store.insert_build(&outcome.hash, entry);
                builds += 1;
            }
            Some(SectionOutput::Recipe(recipe)) => {
                store.insert(&outcome.hash, recipe);
                recipes += 1;
            }
            None => {
                jobs.failed.insert(key);
            }
        }
    }
    if builds + recipes == 0 {
        return;
    }
    if let Err(err) = store.save(&world.doc) {
        warn!("llm: can't write sidecar: {err}");
    }
    info!(
        "llm: {builds} build(s) + {recipes} recipe(ies) applied ({} cached)",
        store.len()
    );
    let manifest = localgpt_md::draft::compile_with(&world.doc, &store);
    world.manifest = manifest;
}
