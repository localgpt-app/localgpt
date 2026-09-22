//! Recover the login shell's `PATH` for launches that didn't come from a
//! terminal.
//!
//! Apps started from Finder, the Dock, or `open` inherit launchd's minimal
//! environment (`PATH=/usr/bin:/bin:/usr/sbin:/sbin`), so CLI backends
//! installed with Homebrew, npm, or into `~/.local/bin` can't be found, and
//! the default model (`claude-cli/…`) fails on first launch. This asks the
//! user's login shell for the `PATH` a terminal would have and merges it in.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Brackets the value in the shell's output, so anything an rc file prints
/// is ignored.
const MARKER: &str = "__LOCALGPT_GEN_PATH__";

/// How long a slow rc file may take before we give up and keep the current
/// `PATH`.
const TIMEOUT: Duration = Duration::from_secs(3);

/// The `PATH` an interactive login shell would have, or `None` when the
/// shell can't be run, times out, or prints nothing usable.
pub fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string());
    // -l reads profile files and -i reads rc files (e.g. ~/.zshrc), where
    // many users extend PATH.
    let script = format!("printf '%s%s%s' {MARKER} \"$PATH\" {MARKER}");
    let mut child = Command::new(&shell)
        .args(["-i", "-l", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    extract_marked(&output)
}

/// The text between the first two markers, trimmed.
fn extract_marked(output: &str) -> Option<String> {
    let start = output.find(MARKER)? + MARKER.len();
    let end = start + output[start..].find(MARKER)?;
    let path = output[start..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// `login` entries first (the order the user chose), then any `current`
/// entries it lacks; duplicates and empty entries dropped.
pub fn merge_paths(current: &str, login: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    login
        .split(':')
        .chain(current.split(':'))
        .filter(|entry| !entry.is_empty() && seen.insert(*entry))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_path_between_markers_despite_rc_noise() {
        let output = format!("Welcome back!\n{MARKER}/opt/homebrew/bin:/usr/bin{MARKER}");
        assert_eq!(
            extract_marked(&output).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
    }

    #[test]
    fn missing_or_empty_output_is_none() {
        assert_eq!(extract_marked("no markers here"), None);
        assert_eq!(extract_marked(&format!("{MARKER}{MARKER}")), None);
        assert_eq!(extract_marked(&format!("{MARKER}/usr/bin")), None);
    }

    #[test]
    fn merge_prefers_login_order_and_keeps_extra_current_entries() {
        assert_eq!(
            merge_paths(
                "/usr/bin:/bin:/usr/sbin:/sbin",
                "/Users/me/.local/bin:/opt/homebrew/bin:/usr/bin:/bin"
            ),
            "/Users/me/.local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }

    #[test]
    fn merge_drops_empty_entries() {
        assert_eq!(merge_paths("/usr/bin::", ":/bin"), "/bin:/usr/bin");
    }
}
