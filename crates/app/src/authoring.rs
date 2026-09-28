//! Authoring: the app's model writes each section's place.
//!
//! The draft gives every section a place the instant it is typed; this is the
//! tier above it, where a model reads the section and builds a place that fits
//! what it says, through world-agent's protocol (the same tools, clamps and
//! asset resolution MD's local-model worker uses). Results land in MD's
//! sidecar keyed by the section's hash, so a section is authored once and kept
//! until its text changes, and every rebuild picks them up.
//!
//! **One request per section, with a JSON plan.** The app runs on whatever
//! model it is configured for, and the zero-config default is often a
//! signed-in CLI backend — which is a subprocess that ignores in-process tool
//! schemas. So instead of a tool loop, the model is shown the tools and asked
//! to reply with the calls as JSON, which every backend can do; the calls are
//! then applied in order through `Session::call`. This is the shape MD's
//! recipe tier already uses (instructed JSON, lenient parse). It trades the
//! loop's look-and-refine for one planned pass, which a capable model handles
//! well; a native tool loop for providers that support one is a later
//! refinement, not a different design.
//!
//! **Isolated from the chat.** Each request is a fresh provider made
//! ephemeral, so a CLI backend neither resumes the chat's stored session nor
//! writes this one over it; its built-in tools and MCP servers are switched
//! off, because planning a place needs neither and a CLI wandering into the
//! workspace, or spawning a second window through an MCP server, would be a
//! side effect nobody asked for.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use localgpt_core::agent::providers::{LLMResponseContent, Message, Role, create_provider};
use localgpt_core::config::Config;
use localgpt_world_agent::BuildOutput;
use localgpt_world_agent::assets::AssetManifest;
use localgpt_world_agent::interpreter::{MAX_AGENT_STEPS, Session, section_prompt, tool_specs};
use serde::Deserialize;

/// How long one section may take before it is given up on. A CLI backend
/// starts a process per request, so this is generous.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
/// The most calls applied from one plan — the interpreter's entity budget
/// clamps anyway; this bounds a runaway reply before it gets there.
const MAX_PLAN_CALLS: usize = MAX_AGENT_STEPS * 4;
/// The most section text shown to the model.
const MAX_EXCERPT_CHARS: usize = 1500;
/// How long a section must sit unchanged before it is sent. A pause
/// mid-sentence rebuilds the draft, which is free; a model call is not, and
/// a request for text the writer is still changing is wasted.
const AUTHOR_SETTLE: Duration = Duration::from_secs(3);

/// One section to author.
pub struct Job {
    pub hash: blake3::Hash,
    pub heading: String,
    pub body: String,
    pub genre: String,
}

/// What came back for one section.
pub struct Outcome {
    pub hash: blake3::Hash,
    pub heading: String,
    pub result: Authored,
}

pub enum Authored {
    /// A place, for the sidecar.
    Built(BuildOutput),
    /// The model replied, but nothing it planned survived the protocol.
    Empty,
    /// The request failed: no model, a timeout, an unreadable reply.
    Failed(String),
}

/// Sections waiting for the model, oldest first, each with when it was
/// queued. Shared with the worker thread, so the app can reorder and prune
/// what has not started yet.
#[derive(Default)]
struct Queue {
    jobs: VecDeque<(Instant, Job)>,
    closed: bool,
}

impl Queue {
    fn retain(&mut self, keep: &HashSet<blake3::Hash>) -> Vec<blake3::Hash> {
        let mut dropped = Vec::new();
        self.jobs.retain(|(_, job)| {
            let kept = keep.contains(&job.hash);
            if !kept {
                dropped.push(job.hash);
            }
            kept
        });
        dropped
    }
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

/// The authoring worker: one background thread, one section at a time, so a
/// long document queues rather than launching a request per section at once.
pub struct Worker {
    pub model: String,
    shared: Arc<Shared>,
    outcomes: Mutex<mpsc::Receiver<Outcome>>,
}

impl Worker {
    pub fn spawn(config: &Config, manifest: Option<AssetManifest>) -> Self {
        let config = authoring_config(config);
        let model = config.agent.default_model.clone();
        let shared = Arc::new(Shared::default());
        let (outcome_tx, outcomes) = mpsc::channel::<Outcome>();
        let queue = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("app-author".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::warn!("authoring: no tokio runtime ({e}) — drafts only");
                        return;
                    }
                };
                while let Some(job) = next_job(&queue, AUTHOR_SETTLE) {
                    let result = rt.block_on(author(&config, &job, manifest.as_ref()));
                    let outcome = Outcome {
                        hash: job.hash,
                        heading: job.heading,
                        result,
                    };
                    if outcome_tx.send(outcome).is_err() {
                        break; // the app is gone
                    }
                }
            })
            .expect("spawning the authoring thread");
        Self {
            model,
            shared,
            outcomes: Mutex::new(outcomes),
        }
    }

    /// Queue a section. `first` puts it ahead of everything waiting — for the
    /// section just edited, which is the one the writer is looking at.
    pub fn submit(&self, job: Job, first: bool) {
        let Ok(mut queue) = self.shared.queue.lock() else {
            return;
        };
        let entry = (Instant::now(), job);
        if first {
            queue.jobs.push_front(entry);
        } else {
            queue.jobs.push_back(entry);
        }
        self.shared.wake.notify_one();
    }

    /// Drop waiting sections whose hash is not in `keep` — edited since they
    /// were queued, or deleted — and return them. A request already running
    /// finishes; its result lands under a hash the sidecar prunes on save.
    pub fn retain(&self, keep: &HashSet<blake3::Hash>) -> Vec<blake3::Hash> {
        match self.shared.queue.lock() {
            Ok(mut queue) => queue.retain(keep),
            Err(_) => Vec::new(),
        }
    }

    /// Everything that has finished since the last call.
    pub fn drain(&self) -> Vec<Outcome> {
        let Ok(outcomes) = self.outcomes.lock() else {
            return Vec::new();
        };
        std::iter::from_fn(|| outcomes.try_recv().ok()).collect()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.closed = true;
        }
        self.shared.wake.notify_all();
    }
}

/// The next section to send: the first in queue order that has sat for
/// `settle`, so the others keep moving while the writer types in one. Blocks
/// until one is ready; `None` once the worker is dropped.
fn next_job(shared: &Shared, settle: Duration) -> Option<Job> {
    let mut queue = shared.queue.lock().ok()?;
    loop {
        if queue.closed {
            return None;
        }
        let ready = queue
            .jobs
            .iter()
            .position(|(queued, _)| queued.elapsed() >= settle);
        if let Some(i) = ready {
            return queue.jobs.remove(i).map(|(_, job)| job);
        }
        let soonest = queue
            .jobs
            .iter()
            .map(|(queued, _)| settle.saturating_sub(queued.elapsed()))
            .min();
        queue = match soonest {
            Some(wait) => shared.wake.wait_timeout(queue, wait).ok()?.0,
            None => shared.wake.wait(queue).ok()?,
        };
    }
}

/// The app's config, adjusted for side-channel requests: the CLI backends'
/// built-in tools off (`--tools ""`), no MCP servers (an empty config with
/// `--strict-mcp-config`), and a modest effort — this runs on its own every
/// time a section settles, so it should not spend like a conversation.
fn authoring_config(config: &Config) -> Config {
    let mut config = config.clone();
    // Gen's in-memory config usually has no [providers.claude_cli] table at
    // all, and ClaudeCliConfig has no Default — its defaults are per-field
    // serde functions. An empty object takes exactly those, as config loading
    // would; without this the overrides below would silently not apply.
    let cli = config.providers.claude_cli.get_or_insert_with(|| {
        serde_json::from_value(serde_json::json!({}))
            .expect("ClaudeCliConfig from its field defaults")
    });
    cli.builtin_tools = Some(String::new());
    cli.mcp_config_override = Some(r#"{"mcpServers":{}}"#.to_string());
    cli.effort = "low".to_string();
    config
}

/// Author one section: one request, one plan, applied through the protocol.
pub async fn author(config: &Config, job: &Job, manifest: Option<&AssetManifest>) -> Authored {
    match plan_and_build(config, job, manifest).await {
        Ok(Some(build)) => Authored::Built(build),
        Ok(None) => Authored::Empty,
        Err(e) => Authored::Failed(e),
    }
}

async fn plan_and_build(
    config: &Config,
    job: &Job,
    manifest: Option<&AssetManifest>,
) -> Result<Option<BuildOutput>, String> {
    let provider = create_provider(&config.agent.default_model, config)
        .map_err(|e| format!("no model to author with: {e}"))?;
    provider.make_ephemeral();

    let messages = [
        message(Role::System, system_prompt(manifest)),
        message(
            Role::User,
            format!(
                "{}\n\nReply with the JSON plan now.",
                section_prompt(
                    &job.heading,
                    &excerpt(&job.body),
                    &job.genre,
                    manifest.is_some()
                )
            ),
        ),
    ];
    let response = tokio::time::timeout(REQUEST_TIMEOUT, provider.chat(&messages, None))
        .await
        .map_err(|_| format!("timed out after {}s", REQUEST_TIMEOUT.as_secs()))?
        .map_err(|e| format!("request failed: {e}"))?;
    let text = match response.content {
        LLMResponseContent::Text(text) => text,
        LLMResponseContent::ToolCalls { .. } => {
            return Err("the model answered with tool calls instead of a plan".into());
        }
    };
    let plan = parse_plan(&text).ok_or_else(|| {
        let head: String = text.chars().take(160).collect();
        format!("no JSON plan in the reply (it began: {head:?})")
    })?;

    let mut session = Session::new(&job.hash.to_hex(), manifest.cloned());
    for call in plan.calls.iter().take(MAX_PLAN_CALLS) {
        let (reply, _) = session.call(&call.tool, &call.args.to_string());
        if reply.starts_with("error") {
            tracing::debug!("authoring \"{}\": {} → {reply}", job.heading, call.tool);
        }
    }
    Ok(session.finish(plan.description))
}

fn message(role: Role, content: String) -> Message {
    Message {
        role,
        content,
        tool_calls: None,
        tool_call_id: None,
        images: Vec::new(),
    }
}

/// The reply format and the tools, as the system prompt.
fn system_prompt(manifest: Option<&AssetManifest>) -> String {
    let mut prompt = String::from(
        "You design 3D places by planning calls to the tools below. You cannot call them \
         yourself in this conversation: reply with ONE JSON object and nothing else, in this \
         shape:\n\
         {\"calls\": [{\"tool\": \"<tool name>\", \"args\": { ... }}], \
          \"description\": \"<one sentence about the place>\"}\n\
         The calls run in order, exactly as written. Plan the whole place in this one reply — \
         about 6 to 14 calls. You will not see the result, so scene_info is not needed; give \
         every entity a unique name so later calls can refer to it.\n\nThe tools:\n",
    );
    for spec in tool_specs(manifest) {
        prompt.push_str(&format!(
            "\n- {}: {}\n  args: {}\n",
            spec.name, spec.description, spec.parameters
        ));
    }
    prompt
}

/// The section text shown to the model, cut at a word boundary.
fn excerpt(body: &str) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_EXCERPT_CHARS {
        return flat;
    }
    let cut: String = flat.chars().take(MAX_EXCERPT_CHARS).collect();
    match cut.rsplit_once(' ') {
        Some((head, _)) => format!("{head} …"),
        None => cut,
    }
}

#[derive(Debug, Deserialize)]
struct Plan {
    #[serde(default)]
    calls: Vec<PlanCall>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlanCall {
    tool: String,
    #[serde(default)]
    args: serde_json::Value,
}

/// The plan in a reply, leniently: models wrap JSON in prose or code fences,
/// so take the outermost object and let serde judge it.
fn parse_plan(text: &str) -> Option<Plan> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str::<Plan>(&text[start..=end])
        .ok()
        .filter(|plan| !plan.calls.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_is_found_inside_prose_and_fences() {
        let reply = "Here is the harbour:\n```json\n{\"calls\": [{\"tool\": \"spawn_primitive\", \
                     \"args\": {\"name\": \"quay\", \"shape\": \"Cuboid\"}}], \
                     \"description\": \"a quay\"}\n```\nEnjoy.";
        let plan = parse_plan(reply).unwrap();
        assert_eq!(plan.calls.len(), 1);
        assert_eq!(plan.calls[0].tool, "spawn_primitive");
        assert_eq!(plan.description.as_deref(), Some("a quay"));
    }

    #[test]
    fn no_plan_is_none_not_a_panic() {
        assert!(parse_plan("I would rather describe it in words.").is_none());
        assert!(
            parse_plan("{\"calls\": []}").is_none(),
            "an empty plan builds nothing"
        );
        assert!(parse_plan("} backwards {").is_none());
    }

    #[test]
    fn a_plan_builds_through_the_protocol() {
        // The whole path after the model: parse, apply through the shared
        // Session, finish. No model needed — this is what CI can check.
        let plan = parse_plan(
            r#"{"calls": [
                {"tool": "spawn_primitive", "args": {"name": "tower", "shape": "Cylinder",
                 "dimensions": {"radius": 0.6, "height": 4}}},
                {"tool": "set_light", "args": {"name": "lamp", "position": [0, 5, 0]}},
                {"tool": "summon_dragon", "args": {}}
            ], "description": "a lone tower"}"#,
        )
        .unwrap();
        let mut session = Session::new("s01", None);
        for call in &plan.calls {
            session.call(&call.tool, &call.args.to_string());
        }
        let build = session.finish(plan.description).unwrap();
        assert!(
            build
                .entities
                .iter()
                .any(|e| e.name.as_str().ends_with("tower"))
        );
        assert_eq!(build.description.as_deref(), Some("a lone tower"));
    }

    #[test]
    fn the_system_prompt_lists_every_tool() {
        let prompt = system_prompt(None);
        for spec in tool_specs(None) {
            assert!(
                prompt.contains(&format!("- {}:", spec.name)),
                "{}",
                spec.name
            );
        }
    }

    #[test]
    fn excerpts_are_bounded_at_a_word() {
        let long = "word ".repeat(2000);
        let cut = excerpt(&long);
        assert!(cut.chars().count() <= MAX_EXCERPT_CHARS + 2);
        assert!(cut.ends_with(" …"));
        assert_eq!(excerpt("  short\n text "), "short text");
    }

    fn job(heading: &str) -> Job {
        Job {
            hash: blake3::hash(heading.as_bytes()),
            heading: heading.into(),
            body: String::new(),
            genre: String::new(),
        }
    }

    #[test]
    fn a_section_being_typed_does_not_hold_up_the_rest() {
        let shared = Shared::default();
        {
            let mut queue = shared.queue.lock().unwrap();
            queue.jobs.push_back((Instant::now(), job("typing")));
            let settled = Instant::now() - Duration::from_secs(10);
            queue.jobs.push_back((settled, job("settled")));
        }
        let next = next_job(&shared, Duration::from_secs(3)).unwrap();
        assert_eq!(next.heading, "settled");
    }

    #[test]
    fn nothing_settled_waits_for_the_soonest() {
        let shared = Shared::default();
        shared
            .queue
            .lock()
            .unwrap()
            .jobs
            .push_back((Instant::now(), job("soon")));
        let started = Instant::now();
        let next = next_job(&shared, Duration::from_millis(60)).unwrap();
        assert_eq!(next.heading, "soon");
        assert!(started.elapsed() >= Duration::from_millis(50));
    }

    #[test]
    fn edited_sections_leave_the_queue_and_a_closed_worker_stops() {
        let mut queue = Queue::default();
        queue.jobs.push_back((Instant::now(), job("kept")));
        queue.jobs.push_back((Instant::now(), job("edited away")));
        let keep = HashSet::from([blake3::hash(b"kept")]);
        assert_eq!(queue.retain(&keep), vec![blake3::hash(b"edited away")]);
        assert_eq!(queue.jobs.len(), 1);

        let shared = Shared::default();
        shared.queue.lock().unwrap().closed = true;
        assert!(next_job(&shared, Duration::ZERO).is_none());
    }

    #[test]
    fn the_authoring_config_switches_off_cli_side_effects_even_with_no_table() {
        // The real case: Gen's in-memory config has no [providers.claude_cli].
        let config = Config::default();
        assert!(config.providers.claude_cli.is_none());
        let cli = authoring_config(&config).providers.claude_cli.unwrap();
        assert_eq!(cli.builtin_tools.as_deref(), Some(""), "no Bash/Read/Write");
        assert!(
            cli.mcp_config_override
                .as_deref()
                .unwrap()
                .contains("mcpServers")
        );
        assert_eq!(cli.effort, "low");
        assert!(!cli.command.is_empty(), "the command keeps its default");
    }
}
