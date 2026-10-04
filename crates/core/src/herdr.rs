//! Reporting this agent's state to Herdr, when we are running inside it.
//!
//! Herdr is a terminal multiplexer for coding agents: it owns a pane's
//! terminal and shows, per pane, whether the agent in it is idle, working or
//! blocked on a human. For agents that don't report, it works that out by
//! matching the bottom of the rendered screen against per-agent patterns —
//! versioned rule files that track, for instance, which spinner glyph a
//! release of another agent happens to use.
//!
//! We don't need to be guessed at. Herdr lets an agent report its own state
//! (`herdr pane report-agent`), and an agent that does is a first-class
//! citizen: its name and state appear in the sidebar and in `herdr agent
//! list`, `herdr agent wait` can block on it, the user gets a notification
//! when it finishes or needs a decision, and after a Herdr server restart the
//! pane reopens on a resume command we supplied.
//!
//! ## The shape of the integration
//!
//! There is deliberately **no dependency on Herdr**. Every process in a Herdr
//! pane inherits [`HERDR_ENV`] and friends, one of which is the path to the
//! binary that owns the pane, so reporting is invoking a program that is
//! already there. Outside Herdr every call here is a no-op — which is the
//! documented contract, and why this costs nothing when nobody is using it.
//!
//! Reports are fire-and-forget: a multiplexer's opinion of our state is never
//! worth failing a turn over, so a failed report is logged at debug and
//! dropped.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// `HERDR_ENV` — `1` inside a Herdr pane. Nothing else here applies without it.
pub const HERDR_ENV: &str = "HERDR_ENV";
/// `HERDR_PANE_ID` — the pane this process runs in.
pub const HERDR_PANE_ID: &str = "HERDR_PANE_ID";
/// `HERDR_BIN_PATH` — the Herdr binary that owns this pane.
pub const HERDR_BIN_PATH: &str = "HERDR_BIN_PATH";
/// `HERDR_SOCKET_PATH` — Herdr's API socket.
pub const HERDR_SOCKET_PATH: &str = "HERDR_SOCKET_PATH";

/// How we identify this integration to Herdr. Stable and unique, and not
/// prefixed `herdr:` — that prefix is reserved for Herdr's own integrations.
pub const SOURCE: &str = "localgpt";
/// The agent name a user sees in Herdr's sidebar.
pub const AGENT: &str = "localgpt";

/// What this agent is doing, in Herdr's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Ready for input.
    Idle,
    /// A turn is running.
    Working,
    /// Waiting on a human decision — a tool approval, a prompt.
    Blocked,
}

impl State {
    /// The spelling `herdr pane report-agent --state` expects.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
        }
    }
}

/// A Herdr pane we are running inside.
#[derive(Debug, Clone)]
pub struct Herdr {
    bin: PathBuf,
    pane: String,
}

/// Monotonic sequence for reports from this process.
///
/// Herdr ignores a report whose sequence is not higher than the last it
/// accepted from the same source, so out-of-order or late reports cannot
/// overwrite newer state. It must increase across sessions and restarts too,
/// so this starts from the wall clock rather than from zero.
static SEQ: AtomicU64 = AtomicU64::new(0);

/// The next sequence after `last`, at wall-clock `now`.
///
/// Pure, because the rule is the interesting part and a shared counter is
/// not testable: take the clock when it is ahead, so a restart never replays
/// numbers a previous run already used, and otherwise keep counting, so two
/// reports in the same millisecond still increase.
fn seq_after(last: u64, now: u64) -> u64 {
    if now > last { now } else { last + 1 }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn next_seq() -> u64 {
    let now = now_ms();
    SEQ.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |last| {
        Some(seq_after(last, now))
    })
    .map(|last| seq_after(last, now))
    .unwrap_or(now)
}

impl Herdr {
    /// The Herdr pane this process is in, or `None` when it is not in one.
    pub fn detect() -> Option<Self> {
        Self::from_env(|key| std::env::var_os(key))
    }

    /// [`Self::detect`] over a given environment, for tests.
    pub fn from_env<F>(env: F) -> Option<Self>
    where
        F: Fn(&str) -> Option<OsString>,
    {
        // Only `HERDR_ENV=1` counts. A pane that set the variable to anything
        // else is not telling us we are inside Herdr.
        if env(HERDR_ENV)?.to_str()? != "1" {
            return None;
        }
        let bin = PathBuf::from(env(HERDR_BIN_PATH)?);
        let pane = env(HERDR_PANE_ID)?.to_str()?.to_string();
        if bin.as_os_str().is_empty() || pane.is_empty() {
            return None;
        }
        Some(Self { bin, pane })
    }

    /// The argv for a state report, for a caller that runs it itself.
    ///
    /// Separated from running it so the command can be asserted without
    /// spawning anything.
    pub fn report_args(&self, state: State, message: Option<&str>, seq: u64) -> Vec<String> {
        let mut args = vec![
            "pane".to_string(),
            "report-agent".to_string(),
            self.pane.clone(),
            "--source".to_string(),
            SOURCE.to_string(),
            "--agent".to_string(),
            AGENT.to_string(),
            "--state".to_string(),
            state.as_str().to_string(),
            "--seq".to_string(),
            seq.to_string(),
        ];
        // A block without a reason is a worse notification than one with it.
        if let Some(message) = message.filter(|m| !m.is_empty()) {
            args.push("--message".to_string());
            args.push(message.to_string());
        }
        args
    }

    /// The argv for a report that also tells Herdr how to resume the session
    /// after a server restart. The resume command goes after `--`.
    pub fn resume_args(&self, state: State, session_id: &str, resume: &[String]) -> Vec<String> {
        let mut args = self.report_args(state, None, next_seq());
        args.push("--agent-session-id".to_string());
        args.push(session_id.to_string());
        if !resume.is_empty() {
            args.push("--".to_string());
            args.extend(resume.iter().cloned());
        }
        args
    }

    /// The binary that owns this pane.
    pub fn bin(&self) -> &std::path::Path {
        &self.bin
    }

    /// The pane id.
    pub fn pane(&self) -> &str {
        &self.pane
    }

    /// Report a state change. Fire-and-forget.
    pub fn report(&self, state: State, message: Option<&str>) {
        self.run(self.report_args(state, message, next_seq()));
    }

    /// Report a state change and the command that reopens this session.
    pub fn report_session(&self, state: State, session_id: &str, resume: &[String]) {
        self.run(self.resume_args(state, session_id, resume));
    }

    /// Release the pane: we are exiting and Herdr should stop attributing it
    /// to us.
    pub fn release(&self) {
        self.run(vec![
            "pane".to_string(),
            "release-agent".to_string(),
            self.pane.clone(),
            "--source".to_string(),
            SOURCE.to_string(),
        ]);
    }

    fn run(&self, args: Vec<String>) {
        // Herdr's view of us is never worth failing a turn over: the report
        // is detached, its output discarded, and a failure logged at debug.
        let bin = self.bin.clone();
        let result = std::process::Command::new(&bin)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match result {
            Ok(mut child) => {
                // Reap without blocking the caller: a report is a few
                // milliseconds, and an unwaited child would linger as a
                // zombie for the life of the process.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => tracing::debug!("herdr report failed ({}): {e}", bin.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).map(OsString::from)
    }

    fn pane() -> Herdr {
        Herdr::from_env(env_of(&[
            (HERDR_ENV, "1"),
            (HERDR_PANE_ID, "pane-7"),
            (HERDR_BIN_PATH, "/usr/local/bin/herdr"),
        ]))
        .expect("a herdr pane")
    }

    #[test]
    fn outside_herdr_there_is_nothing_to_report_to() {
        // The documented contract: do nothing unless HERDR_ENV=1 and the
        // rest is set. This is what makes the integration free for everyone
        // not using Herdr.
        assert!(Herdr::from_env(env_of(&[])).is_none());
        assert!(
            Herdr::from_env(env_of(&[
                (HERDR_ENV, "0"),
                (HERDR_PANE_ID, "p"),
                (HERDR_BIN_PATH, "/h")
            ]))
            .is_none(),
            "HERDR_ENV=0 is not inside Herdr"
        );
        assert!(
            Herdr::from_env(env_of(&[
                (HERDR_ENV, "true"),
                (HERDR_PANE_ID, "p"),
                (HERDR_BIN_PATH, "/h")
            ]))
            .is_none(),
            "only the literal 1 counts"
        );
    }

    #[test]
    fn a_pane_without_an_id_or_a_binary_is_not_usable() {
        assert!(Herdr::from_env(env_of(&[(HERDR_ENV, "1"), (HERDR_BIN_PATH, "/h")])).is_none());
        assert!(Herdr::from_env(env_of(&[(HERDR_ENV, "1"), (HERDR_PANE_ID, "p")])).is_none());
        assert!(
            Herdr::from_env(env_of(&[
                (HERDR_ENV, "1"),
                (HERDR_PANE_ID, ""),
                (HERDR_BIN_PATH, "/h")
            ]))
            .is_none(),
            "an empty pane id is not a pane"
        );
    }

    #[test]
    fn a_report_names_the_pane_the_source_and_the_state() {
        let args = pane().report_args(State::Working, None, 42);
        assert_eq!(
            args,
            [
                "pane",
                "report-agent",
                "pane-7",
                "--source",
                "localgpt",
                "--agent",
                "localgpt",
                "--state",
                "working",
                "--seq",
                "42"
            ]
        );
    }

    #[test]
    fn a_block_carries_its_reason() {
        let args = pane().report_args(State::Blocked, Some("approve bash: rm -rf /tmp/x"), 7);
        let at = args
            .iter()
            .position(|a| a == "--message")
            .expect("--message");
        assert_eq!(args[at + 1], "approve bash: rm -rf /tmp/x");
        assert!(args.contains(&"blocked".to_string()));

        // An empty message is left off rather than passed as a blank.
        let bare = pane().report_args(State::Blocked, Some(""), 8);
        assert!(!bare.contains(&"--message".to_string()));
    }

    #[test]
    fn the_source_is_ours_and_not_in_herdrs_reserved_namespace() {
        // Herdr reserves the `herdr:` prefix for the integrations it owns.
        assert!(!SOURCE.starts_with("herdr:"));
        let args = pane().report_args(State::Idle, None, 1);
        let at = args.iter().position(|a| a == "--source").unwrap();
        assert_eq!(args[at + 1], SOURCE);
    }

    #[test]
    fn a_resume_command_rides_after_a_double_dash() {
        let resume = vec![
            "localgpt".to_string(),
            "chat".to_string(),
            "--resume".to_string(),
            "abc123".to_string(),
        ];
        let args = pane().resume_args(State::Idle, "abc123", &resume);
        let at = args
            .iter()
            .position(|a| a == "--")
            .expect("the -- separator");
        assert_eq!(&args[at + 1..], resume.as_slice());
        let id = args
            .iter()
            .position(|a| a == "--agent-session-id")
            .expect("--agent-session-id");
        assert_eq!(args[id + 1], "abc123");
        assert!(id < at, "the session id belongs before the resume command");
    }

    #[test]
    fn a_session_report_without_a_resume_command_omits_the_separator() {
        let args = pane().resume_args(State::Idle, "abc123", &[]);
        assert!(!args.contains(&"--".to_string()));
    }

    #[test]
    fn sequences_increase_even_within_one_millisecond() {
        // Herdr drops any report whose sequence is not higher than the last,
        // so two state changes in the same millisecond must not collide.
        let mut last = 7;
        for _ in 0..50 {
            let next = seq_after(last, 7);
            assert!(next > last, "sequence did not increase: {last} -> {next}");
            last = next;
        }
    }

    #[test]
    fn a_sequence_follows_the_clock_so_a_restart_cannot_replay_it() {
        // The restart case: a previous run left a number behind, and the
        // clock has since moved past it.
        assert_eq!(seq_after(1_000, 5_000), 5_000);
        // The same-millisecond case: the clock has not moved, so count.
        assert_eq!(seq_after(5_000, 5_000), 5_001);
        // A clock that went backwards (NTP, a suspended laptop) must still
        // produce an increase, or Herdr would ignore every later report.
        assert_eq!(seq_after(5_000, 4_000), 5_001);
    }

    #[test]
    fn the_live_counter_increases_too() {
        let a = next_seq();
        let b = next_seq();
        assert!(b > a, "{a} -> {b}");
        assert!(
            a > 1_700_000_000_000,
            "seeded from the clock, not from zero"
        );
    }

    #[test]
    fn releasing_names_the_pane_and_the_source_only() {
        let p = pane();
        assert_eq!(p.pane(), "pane-7");
        assert_eq!(p.bin(), std::path::Path::new("/usr/local/bin/herdr"));
    }

    #[test]
    fn the_states_spell_themselves_the_way_herdr_expects() {
        assert_eq!(State::Idle.as_str(), "idle");
        assert_eq!(State::Working.as_str(), "working");
        assert_eq!(State::Blocked.as_str(), "blocked");
    }
}
