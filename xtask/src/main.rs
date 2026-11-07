//! The repository's own tooling, in Rust (`docs/rfcs/authoring/all-rust.md`,
//! "Tooling and checks"): run as `cargo xtask <command>` from anywhere in the
//! checkout (the alias is in `.cargo/config.toml`).
//!
//! ```text
//! cargo xtask rule            # the all-Rust rule's ratchet, as CI runs it
//! cargo xtask rule --write    # drop the listed paths that are gone
//! cargo xtask verify          # the CI checks on this machine (--rust --bevy …)
//! ```

mod rule;
mod verify;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root();
    let result = match args.first().map(String::as_str) {
        Some("rule") => rule::run(&root, args.iter().skip(1).any(|a| a == "--write")),
        Some("verify") => verify::run(&root, &args[1..]),
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
        None => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = concat!(
    "usage: cargo xtask <command>

  rule [--write]   the all-Rust rule: no new file in another language, no new
                   crate on wry (xtask/rule.txt is what may only shrink)
",
    // `cargo xtask verify --help` says more.
    "  verify [--rust] [--bevy] [--features] [--web] [--deps]
                   the CI checks on this machine, in CI's order (default: all)"
);

/// The workspace root: the xtask crate's parent folder. `cargo run` names
/// the crate's folder as it runs it, which holds where the build was made
/// (a checkout sharing another's target directory); the folder it was
/// built in is the fallback.
fn repo_root() -> PathBuf {
    let here = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    here.parent().map(PathBuf::from).unwrap_or(here)
}
