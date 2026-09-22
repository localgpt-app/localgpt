//! Asynchronous AI inference queue for the cloud tier (§2 of
//! `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md`).
//!
//! SpacetimeDB is the queue: clients insert prompt jobs, a fleet of
//! registered GPU worker identities claims them, and workers commit the
//! resulting world mutations through the ordinary entity reducers before
//! marking the job complete. The `prompt_job` table is public, so every
//! subscribed client sees queued/running jobs at their anchor position —
//! that row *is* the translucent scaffold, and it disappears from the
//! scaffold set when the job reaches a terminal state.
//!
//! Semantics mirror the listen-server queue in `localgpt-gen`
//! (`crates/gen/src/net/jobs.rs`): FIFO, bounded backlog, per-requester
//! limit, and the same state names.
//!
//! Trust: only identities registered by the module admin (the publisher,
//! seeded by the `init` reducer) may claim and complete jobs, and a job can
//! only be completed by the worker that claimed it. Claims whose worker
//! stops heart-beating are requeued so a crashed node doesn't strand work.

use spacetimedb::{reducer, table, Identity, ReducerContext, Table, Timestamp};

use crate::CHUNK_SIZE;

/// Total queued jobs accepted at once.
pub const MAX_QUEUED: usize = 256;
/// Queued jobs per requester.
pub const MAX_QUEUED_PER_REQUESTER: usize = 4;
/// Longest prompt accepted (characters).
pub const MAX_PROMPT_CHARS: usize = 2000;
/// A running job whose worker has been silent this long is requeued.
pub const STALE_CLAIM_MICROS: i64 = 120 * 1_000_000;
/// Terminal jobs are pruned after this long.
pub const TERMINAL_RETENTION_MICROS: i64 = 10 * 60 * 1_000_000;

pub const STATE_QUEUED: &str = "queued";
pub const STATE_RUNNING: &str = "running";
pub const STATE_DONE: &str = "done";
pub const STATE_FAILED: &str = "failed";
pub const STATE_CANCELLED: &str = "cancelled";

/// Module administrators (seeded with the publisher at `init`).
#[table(accessor = admin)]
pub struct Admin {
    #[primary_key]
    pub identity: Identity,
}

/// Registered inference worker nodes.
#[table(accessor = inference_worker, public)]
pub struct InferenceWorker {
    #[primary_key]
    pub identity: Identity,
    pub name: String,
    pub registered_at: Timestamp,
    pub last_heartbeat: Timestamp,
    /// Job currently being processed, if any.
    pub current_job: Option<u64>,
    pub completed: u64,
}

/// A prompt waiting for / being processed by a worker. Public: clients
/// render non-terminal rows as scaffolds at the anchor.
#[table(accessor = prompt_job, public)]
pub struct PromptJob {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub requester: Identity,
    /// Client-chosen correlation id (hand-off from the client's predicted
    /// scaffold to this row).
    pub request_id: u64,
    pub prompt: String,
    pub anchor_x: f32,
    pub anchor_y: f32,
    pub anchor_z: f32,
    pub chunk_x: i32,
    pub chunk_y: i32,
    pub state: String,
    pub worker: Option<Identity>,
    pub error: Option<String>,
    pub created_at: Timestamp,
    pub claimed_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
}

// ---------------------------------------------------------------------------
// Pure policy (unit-tested natively)
// ---------------------------------------------------------------------------

/// Validate and normalise a prompt submission.
pub fn validate_submission(
    prompt: &str,
    anchor: [f32; 3],
    queued_total: usize,
    queued_by_requester: usize,
) -> Result<String, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("empty prompt".into());
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(format!("prompt longer than {MAX_PROMPT_CHARS} characters"));
    }
    if !anchor.iter().all(|v| v.is_finite()) {
        return Err("anchor must be finite".into());
    }
    if queued_total >= MAX_QUEUED {
        return Err(format!("queue is full ({MAX_QUEUED} jobs)"));
    }
    if queued_by_requester >= MAX_QUEUED_PER_REQUESTER {
        return Err(format!(
            "you already have {MAX_QUEUED_PER_REQUESTER} prompts waiting"
        ));
    }
    Ok(prompt.to_string())
}

/// Whether a running claim should be returned to the queue.
pub fn claim_is_stale(worker_last_heartbeat_micros: i64, now_micros: i64) -> bool {
    now_micros - worker_last_heartbeat_micros > STALE_CLAIM_MICROS
}

pub fn is_terminal(state: &str) -> bool {
    matches!(state, STATE_DONE | STATE_FAILED | STATE_CANCELLED)
}

/// Pick the oldest queued job (lowest id — ids are monotonically assigned).
pub fn next_queued<'a>(jobs: impl Iterator<Item = (u64, &'a str)>) -> Option<u64> {
    jobs.filter(|(_, state)| *state == STATE_QUEUED)
        .map(|(id, _)| id)
        .min()
}

// ---------------------------------------------------------------------------
// Reducers
// ---------------------------------------------------------------------------

/// Seed the publisher as the module admin.
#[reducer(init)]
pub fn init(ctx: &ReducerContext) {
    ctx.db.admin().insert(Admin {
        identity: ctx.sender(),
    });
}

fn is_admin(ctx: &ReducerContext) -> bool {
    ctx.db.admin().identity().find(&ctx.sender()).is_some()
}

/// Admin: allow `worker` to claim jobs.
#[reducer]
pub fn register_worker(ctx: &ReducerContext, worker: Identity, name: String) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("only an admin can register workers".into());
    }
    let name: String = name.chars().take(64).collect();
    match ctx.db.inference_worker().identity().find(&worker) {
        Some(mut existing) => {
            existing.name = name;
            ctx.db.inference_worker().identity().update(existing);
        }
        None => {
            ctx.db.inference_worker().insert(InferenceWorker {
                identity: worker,
                name,
                registered_at: ctx.timestamp,
                last_heartbeat: ctx.timestamp,
                current_job: None,
                completed: 0,
            });
        }
    }
    Ok(())
}

/// Admin: revoke a worker; its running job (if any) is requeued.
#[reducer]
pub fn unregister_worker(ctx: &ReducerContext, worker: Identity) -> Result<(), String> {
    if !is_admin(ctx) {
        return Err("only an admin can unregister workers".into());
    }
    if let Some(w) = ctx.db.inference_worker().identity().find(&worker) {
        if let Some(job_id) = w.current_job {
            requeue(ctx, job_id);
        }
        ctx.db.inference_worker().identity().delete(worker);
    }
    Ok(())
}

/// Client: queue a prompt anchored at a world position.
#[reducer]
pub fn submit_prompt(
    ctx: &ReducerContext,
    request_id: u64,
    prompt: String,
    anchor_x: f32,
    anchor_y: f32,
    anchor_z: f32,
) -> Result<(), String> {
    prune_terminal(ctx);
    let sender = ctx.sender();
    let (mut total, mut mine) = (0usize, 0usize);
    for job in ctx.db.prompt_job().iter() {
        if job.state == STATE_QUEUED {
            total += 1;
            if job.requester == sender {
                mine += 1;
            }
        }
    }
    let prompt = validate_submission(&prompt, [anchor_x, anchor_y, anchor_z], total, mine)?;
    ctx.db.prompt_job().insert(PromptJob {
        id: 0,
        requester: sender,
        request_id,
        prompt,
        anchor_x,
        anchor_y,
        anchor_z,
        chunk_x: (anchor_x / CHUNK_SIZE).floor() as i32,
        chunk_y: (anchor_z / CHUNK_SIZE).floor() as i32,
        state: STATE_QUEUED.to_string(),
        worker: None,
        error: None,
        created_at: ctx.timestamp,
        claimed_at: None,
        finished_at: None,
    });
    Ok(())
}

/// Client: withdraw one of your own jobs while it is still queued.
#[reducer]
pub fn cancel_prompt(ctx: &ReducerContext, job_id: u64) -> Result<(), String> {
    let Some(mut job) = ctx.db.prompt_job().id().find(&job_id) else {
        return Err("no such job".into());
    };
    if job.requester != ctx.sender() {
        return Err("not your job".into());
    }
    if job.state != STATE_QUEUED {
        return Err(format!("job is {}", job.state));
    }
    job.state = STATE_CANCELLED.to_string();
    job.finished_at = Some(ctx.timestamp);
    ctx.db.prompt_job().id().update(job);
    Ok(())
}

/// Worker: liveness ping (keeps running claims from being requeued).
#[reducer]
pub fn worker_heartbeat(ctx: &ReducerContext) -> Result<(), String> {
    let Some(mut worker) = ctx.db.inference_worker().identity().find(&ctx.sender()) else {
        return Err("not a registered worker".into());
    };
    worker.last_heartbeat = ctx.timestamp;
    ctx.db.inference_worker().identity().update(worker);
    Ok(())
}

/// Worker: claim the oldest queued job. Also requeues stale claims first.
///
/// The claimed row (with `worker == sender`, `state == "running"`) is how
/// the worker learns which job it got — it subscribes to its own claims.
#[reducer]
pub fn claim_job(ctx: &ReducerContext) -> Result<(), String> {
    let sender = ctx.sender();
    let Some(mut worker) = ctx.db.inference_worker().identity().find(&sender) else {
        return Err("not a registered worker".into());
    };
    if worker.current_job.is_some() {
        return Err("finish your current job first".into());
    }
    requeue_stale(ctx);

    let jobs: Vec<(u64, String)> = ctx
        .db
        .prompt_job()
        .iter()
        .map(|j| (j.id, j.state))
        .collect();
    let Some(job_id) = next_queued(jobs.iter().map(|(id, s)| (*id, s.as_str()))) else {
        return Err("queue is empty".into());
    };
    let Some(mut job) = ctx.db.prompt_job().id().find(&job_id) else {
        return Err("job vanished".into());
    };
    job.state = STATE_RUNNING.to_string();
    job.worker = Some(sender);
    job.claimed_at = Some(ctx.timestamp);
    ctx.db.prompt_job().id().update(job);

    worker.current_job = Some(job_id);
    worker.last_heartbeat = ctx.timestamp;
    ctx.db.inference_worker().identity().update(worker);
    Ok(())
}

/// Worker: finish the claimed job. World mutations must already have been
/// committed via the entity reducers; `error` marks the job failed.
#[reducer]
pub fn complete_job(ctx: &ReducerContext, job_id: u64, error: Option<String>) -> Result<(), String> {
    let sender = ctx.sender();
    let Some(mut job) = ctx.db.prompt_job().id().find(&job_id) else {
        return Err("no such job".into());
    };
    if job.worker != Some(sender) || job.state != STATE_RUNNING {
        return Err("job is not claimed by you".into());
    }
    job.state = if error.is_some() {
        STATE_FAILED
    } else {
        STATE_DONE
    }
    .to_string();
    job.error = error.map(|e| e.chars().take(500).collect());
    job.finished_at = Some(ctx.timestamp);
    ctx.db.prompt_job().id().update(job);

    if let Some(mut worker) = ctx.db.inference_worker().identity().find(&sender) {
        worker.current_job = None;
        worker.completed += 1;
        worker.last_heartbeat = ctx.timestamp;
        ctx.db.inference_worker().identity().update(worker);
    }
    Ok(())
}

/// Called from the disconnect lifecycle reducer: drop the requester's
/// queued jobs; if the identity is a worker, requeue its claim.
pub fn on_disconnect(ctx: &ReducerContext) {
    let sender = ctx.sender();
    let queued: Vec<PromptJob> = ctx
        .db
        .prompt_job()
        .iter()
        .filter(|j| j.requester == sender && j.state == STATE_QUEUED)
        .collect();
    for mut job in queued {
        job.state = STATE_CANCELLED.to_string();
        job.finished_at = Some(ctx.timestamp);
        ctx.db.prompt_job().id().update(job);
    }
    if let Some(mut worker) = ctx.db.inference_worker().identity().find(&sender) {
        if let Some(job_id) = worker.current_job.take() {
            requeue(ctx, job_id);
        }
        ctx.db.inference_worker().identity().update(worker);
    }
}

fn requeue(ctx: &ReducerContext, job_id: u64) {
    if let Some(mut job) = ctx.db.prompt_job().id().find(&job_id) {
        if job.state == STATE_RUNNING {
            job.state = STATE_QUEUED.to_string();
            job.worker = None;
            job.claimed_at = None;
            ctx.db.prompt_job().id().update(job);
        }
    }
}

fn requeue_stale(ctx: &ReducerContext) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let stale: Vec<InferenceWorker> = ctx
        .db
        .inference_worker()
        .iter()
        .filter(|w| {
            w.current_job.is_some()
                && claim_is_stale(w.last_heartbeat.to_micros_since_unix_epoch(), now)
        })
        .collect();
    for mut worker in stale {
        if let Some(job_id) = worker.current_job.take() {
            requeue(ctx, job_id);
        }
        ctx.db.inference_worker().identity().update(worker);
    }
}

fn prune_terminal(ctx: &ReducerContext) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let old: Vec<u64> = ctx
        .db
        .prompt_job()
        .iter()
        .filter(|j| {
            is_terminal(&j.state)
                && j.finished_at.is_some_and(|t| {
                    now - t.to_micros_since_unix_epoch() > TERMINAL_RETENTION_MICROS
                })
        })
        .map(|j| j.id)
        .collect();
    for id in old {
        ctx.db.prompt_job().id().delete(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_validation() {
        assert_eq!(
            validate_submission("  build a tower ", [0.0; 3], 0, 0),
            Ok("build a tower".to_string())
        );
        assert!(validate_submission("   ", [0.0; 3], 0, 0).is_err());
        assert!(validate_submission("x", [f32::NAN, 0.0, 0.0], 0, 0).is_err());
        assert!(validate_submission("x", [0.0; 3], MAX_QUEUED, 0).is_err());
        assert!(validate_submission("x", [0.0; 3], 0, MAX_QUEUED_PER_REQUESTER).is_err());
        let long = "a".repeat(MAX_PROMPT_CHARS + 1);
        assert!(validate_submission(&long, [0.0; 3], 0, 0).is_err());
    }

    #[test]
    fn next_queued_is_fifo() {
        let jobs = [(5, STATE_QUEUED), (2, STATE_RUNNING), (3, STATE_QUEUED), (1, STATE_DONE)];
        assert_eq!(next_queued(jobs.iter().copied()), Some(3));
        assert_eq!(next_queued([(1, STATE_DONE)].into_iter()), None);
    }

    #[test]
    fn stale_claims_and_terminal_states() {
        assert!(!claim_is_stale(0, STALE_CLAIM_MICROS));
        assert!(claim_is_stale(0, STALE_CLAIM_MICROS + 1));
        assert!(is_terminal(STATE_CANCELLED));
        assert!(!is_terminal(STATE_RUNNING));
    }
}
