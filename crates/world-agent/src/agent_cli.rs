//! Launching a coding agent into a live world's folder.
//!
//! The world is a folder and the agent is a process that changes it from
//! outside (`docs/rfcs/authoring/live-editing-poc.md`). This module says how
//! to start that process: which agents are installed, and the exact command,
//! working directory and environment to run one against a given world.
//!
//! **What actually connects the agent to the world is the working directory.**
//! `claude` and `codex` do not read LocalGPT's environment; they read the
//! folder they are started in. A live world's folder holds `AGENTS.md`, which
//! says how to change the world, and `.live/endpoint.json`, which holds the
//! API's URL and token — so an agent started *in* the folder can discover
//! everything it needs by reading files it already reads. Nothing has to be
//! injected into the agent, which is what keeps this working with whatever
//! those tools ship next.
//!
//! The environment this adds is for *our* side of the bridge — the MCP shim,
//! a user's script, a wrapper — and for attribution: a batch's author comes
//! from [`AUTHOR_ENV`] rather than from a string the agent chooses for
//! itself.
//!
//! This crate is Bevy-free and deliberately does not depend on
//! `localgpt-core`, so a [`Launch`] is plain data rather than core's
//! `PtySpawnSpec`. Whoever owns a PTY host converts it; see
//! [`Launch::into_parts`].

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// `$LOCALGPT_WORLD_DIR` — the live world's folder.
pub const WORLD_DIR_ENV: &str = "LOCALGPT_WORLD_DIR";
/// `$LOCALGPT_WORLD_API` — the live API's base URL.
pub const API_ENV: &str = "LOCALGPT_WORLD_API";
/// `$LOCALGPT_WORLD_TOKEN` — the bearer token for that API.
pub const TOKEN_ENV: &str = "LOCALGPT_WORLD_TOKEN";
/// `$LOCALGPT_WORLD_AUTHOR` — who to attribute this session's batches to.
pub const AUTHOR_ENV: &str = "LOCALGPT_WORLD_AUTHOR";

/// A coding agent this machine might have.
///
/// Interactive invocations only: the point of a terminal pane is the agent's
/// own full surface — permission prompts, slash commands, login flows,
/// whatever it ships next — so nothing here passes a headless or
/// print-and-exit flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCli {
    /// Stable id, for settings and for a pane's label.
    pub id: &'static str,
    /// The executable to look for on `PATH`.
    pub exe: &'static str,
    /// What to call it in a menu.
    pub label: &'static str,
    /// Arguments for an interactive session. Usually none.
    pub args: &'static [&'static str],
}

/// The agents a pane offers, in the order a menu should list them.
pub const KNOWN: &[AgentCli] = &[
    AgentCli {
        id: "claude",
        exe: "claude",
        label: "Claude Code",
        args: &[],
    },
    AgentCli {
        id: "codex",
        exe: "codex",
        label: "Codex",
        args: &[],
    },
    AgentCli {
        id: "gemini",
        exe: "gemini",
        label: "Gemini CLI",
        args: &[],
    },
];

/// The agent with this id, if it is one we know.
pub fn by_id(id: &str) -> Option<&'static AgentCli> {
    KNOWN.iter().find(|a| a.id == id)
}

/// The known agents whose executable is on `PATH`, in [`KNOWN`] order.
pub fn installed() -> Vec<&'static AgentCli> {
    KNOWN
        .iter()
        .filter(|a| find_on_path(a.exe).is_some())
        .collect()
}

/// Find `program` on `PATH`, trying `.exe` and `.cmd` on Windows.
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    find_on_path_in(program, &path, is_executable)
}

/// [`find_on_path`] over a given `PATH` and existence check, for tests.
pub fn find_on_path_in<F>(program: &str, path: &std::ffi::OsStr, is_exe: F) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool,
{
    std::env::split_paths(path).find_map(|dir| {
        #[cfg(windows)]
        let names = [
            format!("{program}.exe"),
            format!("{program}.cmd"),
            program.to_string(),
        ];
        #[cfg(not(windows))]
        let names = [program.to_string()];
        names
            .into_iter()
            .map(|name| dir.join(name))
            .find(|candidate| is_exe(candidate))
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// What an open app advertises in `.live/endpoint.json`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Endpoint {
    /// The API's base URL, e.g. `http://127.0.0.1:51734`.
    pub url: String,
    /// The bearer token every call must carry.
    pub token: String,
    /// The process holding the world open.
    #[serde(default)]
    pub pid: Option<u32>,
}

impl Endpoint {
    /// Read the endpoint a live world is advertising.
    ///
    /// `Err` means no app has the folder open — which is the normal state of
    /// a world on disk, so a caller offering to launch an agent should treat
    /// it as "open the world first", not as a fault.
    pub fn read(world_dir: &Path) -> Result<Self, String> {
        let path = world_dir.join(crate::live::LIVE).join("endpoint.json");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Everything needed to start an agent against a world.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    /// Program and arguments, program first.
    pub command: Vec<String>,
    /// The world's folder — what makes `AGENTS.md` and the endpoint
    /// discoverable, and so the thing that actually connects agent to world.
    pub cwd: PathBuf,
    /// Environment layered over the parent's.
    pub env: Vec<(String, String)>,
}

impl Launch {
    /// `(command, cwd, env)`, for a caller mapping this onto its own spawn
    /// type — `localgpt_core::pty::PtySpawnSpec`, `std::process::Command`, or
    /// a shell line.
    pub fn into_parts(self) -> (Vec<String>, PathBuf, Vec<(String, String)>) {
        (self.command, self.cwd, self.env)
    }

    /// The launch as a shell line, for "run this yourself" — the zero-cost
    /// way to put an agent in a world, and the fallback whenever a pane is
    /// not available.
    ///
    /// Quoting is deliberately minimal: it quotes what needs quoting and is
    /// meant to be read by a person, not to round-trip arbitrary bytes.
    pub fn shell_line(&self) -> String {
        let mut out = format!("cd {} &&", shell_quote(&self.cwd.to_string_lossy()));
        for (key, value) in &self.env {
            out.push_str(&format!(" {key}={}", shell_quote(value)));
        }
        for part in &self.command {
            out.push(' ');
            out.push_str(&shell_quote(part));
        }
        out
    }
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/:=".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Build the launch for `cli` against the world in `world_dir`, attributing
/// its batches to `author`.
///
/// Reads the folder's `.live/endpoint.json`, so the world must be open in an
/// app. The endpoint's URL and token go into the environment for the MCP shim
/// and for scripts; the agent itself finds them by reading the folder it is
/// started in.
pub fn launch(world_dir: &Path, cli: &AgentCli, author: &str) -> Result<Launch, String> {
    let endpoint = Endpoint::read(world_dir)?;
    Ok(launch_with(world_dir, cli, author, &endpoint))
}

/// [`launch`] against an endpoint the caller already has.
pub fn launch_with(world_dir: &Path, cli: &AgentCli, author: &str, endpoint: &Endpoint) -> Launch {
    let mut command = vec![cli.exe.to_string()];
    command.extend(cli.args.iter().map(|a| a.to_string()));
    Launch {
        command,
        cwd: world_dir.to_path_buf(),
        env: vec![
            (WORLD_DIR_ENV.into(), world_dir.to_string_lossy().into()),
            (API_ENV.into(), endpoint.url.clone()),
            (TOKEN_ENV.into(), endpoint.token.clone()),
            (AUTHOR_ENV.into(), author.to_string()),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn endpoint() -> Endpoint {
        Endpoint {
            url: "http://127.0.0.1:51734".into(),
            token: "deadbeef".into(),
            pid: Some(42),
        }
    }

    #[test]
    fn a_launch_runs_the_agent_in_the_worlds_own_folder() {
        // The whole bridge: cwd is what lets the agent read AGENTS.md and
        // the endpoint without anything being injected into it.
        let world = Path::new("/worlds/harbour.world");
        let launch = launch_with(world, by_id("claude").unwrap(), "claude", &endpoint());
        assert_eq!(launch.command, ["claude"]);
        assert_eq!(launch.cwd, world);
    }

    #[test]
    fn the_environment_carries_the_endpoint_and_the_author() {
        let launch = launch_with(
            Path::new("/worlds/harbour.world"),
            by_id("codex").unwrap(),
            "codex",
            &endpoint(),
        );
        let env: std::collections::HashMap<_, _> = launch.env.iter().cloned().collect();
        assert_eq!(env[API_ENV], "http://127.0.0.1:51734");
        assert_eq!(env[TOKEN_ENV], "deadbeef");
        assert_eq!(env[AUTHOR_ENV], "codex");
        assert_eq!(env[WORLD_DIR_ENV], "/worlds/harbour.world");
    }

    #[test]
    fn the_author_comes_from_us_not_from_the_agent() {
        // Attribution is the reason to spawn the agent ourselves: a batch's
        // author is set here, so a history cannot be forged by an agent
        // describing itself differently.
        let launch = launch_with(
            Path::new("/w"),
            by_id("claude").unwrap(),
            "yi via claude",
            &endpoint(),
        );
        let env: std::collections::HashMap<_, _> = launch.env.iter().cloned().collect();
        assert_eq!(env[AUTHOR_ENV], "yi via claude");
    }

    #[test]
    fn no_agent_is_invoked_headlessly() {
        // A pane exists for the agent's own interactive surface. A print-and-
        // exit flag would throw away the thing it is for.
        for agent in KNOWN {
            for arg in agent.args {
                assert!(
                    !matches!(*arg, "-p" | "--print" | "exec" | "--headless"),
                    "{} is invoked non-interactively",
                    agent.id
                );
            }
        }
    }

    #[test]
    fn a_shell_line_is_the_zero_cost_way_in() {
        let launch = launch_with(
            Path::new("/worlds/my world"),
            by_id("claude").unwrap(),
            "claude",
            &endpoint(),
        );
        let line = launch.shell_line();
        assert!(line.starts_with("cd '/worlds/my world' &&"), "{line}");
        assert!(
            line.contains("LOCALGPT_WORLD_API=http://127.0.0.1:51734"),
            "{line}"
        );
        assert!(line.ends_with(" claude"), "{line}");
    }

    #[test]
    fn known_agents_are_distinct_and_addressable_by_id() {
        let ids: Vec<_> = KNOWN.iter().map(|a| a.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate agent ids");
        for id in ids {
            assert_eq!(by_id(id).map(|a| a.id), Some(id));
        }
        assert!(by_id("not-an-agent").is_none());
    }

    #[test]
    fn path_lookup_takes_the_first_directory_that_has_it() {
        let path = OsString::from("/first:/second");
        let found = find_on_path_in("claude", &path, |p| {
            p == Path::new("/second/claude") || p == Path::new("/first/other")
        });
        assert_eq!(found, Some(PathBuf::from("/second/claude")));

        assert_eq!(find_on_path_in("claude", &path, |_| false), None);
    }

    #[test]
    fn a_world_nothing_has_open_is_a_state_not_a_fault() {
        // The normal state of a world on disk. The caller's answer is "open
        // it first", so this must be an ordinary Err rather than a panic.
        let err = Endpoint::read(Path::new("/definitely/not/a/world")).unwrap_err();
        assert!(err.contains("endpoint.json"), "{err}");
    }

    #[test]
    fn an_endpoint_without_a_pid_still_reads() {
        // `pid` is advisory; a writer that omits it must not break a launch.
        let parsed: Endpoint =
            serde_json::from_str(r#"{"url": "http://x", "token": "t"}"#).unwrap();
        assert_eq!(parsed.pid, None);
        assert_eq!(parsed.url, "http://x");
    }

    #[test]
    fn extra_endpoint_fields_are_ignored_rather_than_refused() {
        // The app writes `world` and `endpoints` too; an older reader must
        // keep working, as the format's must-ignore rule requires.
        let parsed: Endpoint = serde_json::from_str(
            r#"{"url": "http://x", "token": "t", "pid": 9, "world": "/w",
                 "endpoints": ["POST /ops"]}"#,
        )
        .unwrap();
        assert_eq!(parsed.pid, Some(9));
    }
}
