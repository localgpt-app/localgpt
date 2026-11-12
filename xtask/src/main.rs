//! The repository's own tooling, in Rust (`docs/rfcs/authoring/all-rust.md`,
//! "Tooling and checks"): run as `cargo xtask <command>` from anywhere in the
//! checkout (the alias is in `.cargo/config.toml`). It replaced `bin/lg` and
//! the scripts it routed to; `cargo xtask` alone lists the commands.
//!
//! The commands call the system's tools where a tool is the job (cargo,
//! git, codesign, hdiutil, curl, tar, npm for the web pages' packages,
//! wrangler, zola, wasm-pack), as a script did, and do the rest themselves.

mod assets;
mod build;
mod bundle;
mod release;
mod rule;
mod sites;
mod smoke;
mod verify;
mod verse;
mod viewer;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root();
    let rest = args.get(1..).unwrap_or_default();
    let result = match args.first().map(String::as_str) {
        Some("rule") => rule::run(&root, rest.iter().any(|a| a == "--write")),
        Some("verify") => verify::run(&root, rest),
        Some("build") => build::run(&root, rest),
        Some("bundle") => bundle::run(&root, rest),
        Some("fetch-assets") => assets::run(&root),
        Some("deploy") => sites::deploy(&root, rest),
        Some("assemble") => sites::assemble(&root, rest),
        Some("add-world") => sites::add_world(&root, rest),
        Some("sync-viewer") => viewer::run(&root, rest),
        Some("release-plan") => release::plan(&root),
        Some("trust-release") => release::trust(&root, rest),
        Some("cask") => release::cask(rest),
        Some("npm-package") => release::npm_package(&root, rest),
        Some("fetch-clap") => verse::fetch_clap(&root),
        Some("verse-bundle") => verse::bundle(&root),
        Some("live-smoke") => smoke::run(&root, rest),
        Some("help" | "-h" | "--help") => {
            println!("{}", usage());
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n\n{}", usage())),
        None => Err(usage()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    [
        "usage: cargo xtask <command>\n",
        "  rule [--write]   the all-Rust rule: no new file in another language, no new
                   crate on wry (xtask/rule.txt is what may only shrink)",
        "  verify [--rust] [--bevy] [--features] [--web] [--deps]
                   the CI checks on this machine, in CI's order (default: all)",
        build::USAGE,
        "  bundle [--debug] [--dmg]
                   LocalGPT.app for macOS, and a .dmg (`cargo xtask bundle --help`)",
        "  fetch-assets     the CC0 asset pack and the starter tracks, from a
                   localgpt-world-assets checkout into the shared pack",
        sites::DEPLOY_USAGE,
        sites::ASSEMBLE_USAGE,
        sites::ADD_WORLD_USAGE,
        viewer::USAGE,
        release::PLAN_USAGE,
        release::TRUST_USAGE,
        release::CASK_USAGE,
        release::NPM_USAGE,
        verse::FETCH_CLAP_USAGE,
        verse::BUNDLE_USAGE,
        smoke::USAGE,
    ]
    .join("\n")
}

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
