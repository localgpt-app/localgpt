//! What a live world's folder tells any agent started in it.
//!
//! The world is a folder and an agent is a process that changes it from
//! outside. **What connects the two is the working directory.** A live
//! world's folder holds `AGENTS.md`,
//! which says how to change the world, and `.live/endpoint.json`, which holds
//! the API's URL and token — so an agent started *in* the folder discovers
//! everything it needs by reading files it already reads. Nothing is
//! injected into the agent, nothing here knows one agent from another, and
//! the app that opens a terminal in the folder sets only the environment
//! below for *our* side of the bridge: `localgpt world`, the MCP shim, a
//! person's script.

use std::path::Path;

use serde::Deserialize;

/// `$LOCALGPT_WORLD_DIR` — the live world's folder.
pub const WORLD_DIR_ENV: &str = "LOCALGPT_WORLD_DIR";
/// `$LOCALGPT_WORLD_API` — the live API's base URL.
pub const API_ENV: &str = "LOCALGPT_WORLD_API";
/// `$LOCALGPT_WORLD_TOKEN` — the bearer token for that API.
pub const TOKEN_ENV: &str = "LOCALGPT_WORLD_TOKEN";
/// `$LOCALGPT_WORLD_AUTHOR` — who to attribute a session's batches to.
pub const AUTHOR_ENV: &str = "LOCALGPT_WORLD_AUTHOR";

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
    /// a world on disk, so a caller should treat it as "open the world
    /// first", not as a fault.
    pub fn read(world_dir: &Path) -> Result<Self, String> {
        let path = world_dir.join(crate::live::LIVE).join("endpoint.json");
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_nothing_has_open_is_a_state_not_a_fault() {
        // The normal state of a world on disk. The caller's answer is "open
        // it first", so this must be an ordinary Err rather than a panic.
        let err = Endpoint::read(Path::new("/definitely/not/a/world")).unwrap_err();
        assert!(err.contains("endpoint.json"), "{err}");
    }

    #[test]
    fn an_endpoint_without_a_pid_still_reads() {
        // `pid` is advisory; a writer that omits it must not break a reader.
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
