//! `cargo xtask verify`: the CI checks on this machine, in the order
//! `.github/workflows/ci.yml` runs them, stopping at the first that fails.
//! Run it before a release or a deploy. It was `bin/ops/verify.sh`.
//!
//! It needs what CI installs: the Rust toolchain, cargo-deny, node 22,
//! wasm-pack with the wasm32-unknown-unknown target, and Chromium for
//! Playwright. A missing `node_modules` is installed with `npm ci`, as CI
//! does.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub const USAGE: &str = "  verify [--rust] [--bevy] [--features] [--web] [--deps]
                   the CI checks on this machine, in CI's order (default: all)
                     --rust      fmt, the rule, clippy and tests for the workspace
                                 minus the Bevy crates, the world.schema.json snapshot
                     --bevy      clippy and tests for app, gen, doc, verse,
                                 world-bevy, world-audio
                     --features  cargo check for the opt-in tier (Verse's ml)
                     --web       the phone's page, the vendored viewer against npm,
                                 localgpt.world's render check, localgpt.md's page
                     --deps      cargo-deny: licenses, bans, sources, advisories";

/// The crates the `bevy` job owns: those that need native audio and
/// windowing headers.
const BEVY: [&str; 6] = [
    "localgpt-gen",
    "localgpt-world-bevy",
    "localgpt-doc",
    "localgpt-verse",
    "localgpt-world-audio",
    "localgpt-app",
];

#[derive(Debug, Default, PartialEq)]
struct Want {
    rust: bool,
    bevy: bool,
    features: bool,
    web: bool,
    deps: bool,
}

/// What the arguments ask for: every group when none is named.
fn wanted(args: &[String]) -> Result<Option<Want>, String> {
    let mut want = Want::default();
    for arg in args {
        match arg.as_str() {
            "--rust" => want.rust = true,
            "--bevy" => want.bevy = true,
            "--features" => want.features = true,
            "--web" => want.web = true,
            "--deps" => want.deps = true,
            "-h" | "--help" => return Ok(None),
            other => {
                return Err(format!(
                    "unexpected argument {other:?} (--rust|--bevy|--features|--web|--deps)"
                ));
            }
        }
    }
    if want == Want::default() {
        want = Want {
            rust: true,
            bevy: true,
            features: true,
            web: true,
            deps: true,
        };
    }
    Ok(Some(want))
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let Some(want) = wanted(args)? else {
        println!(
            "usage: cargo xtask verify [--rust] [--bevy] [--features] [--web] [--deps]\n\n{USAGE}"
        );
        return Ok(());
    };
    let started = Instant::now();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let packages = |flag: &str| -> Vec<String> {
        BEVY.iter()
            .flat_map(|krate| [flag.to_string(), krate.to_string()])
            .collect()
    };

    if want.rust {
        stage(
            root,
            "cargo fmt --check",
            &cargo,
            &["fmt", "--all", "--", "--check"],
        )?;
        println!("\n──────── the all-Rust rule (cargo xtask rule) ────────");
        crate::rule::run(root, false).map_err(|e| {
            format!("{e}\n\n'the all-Rust rule' FAILED — nothing after it was run.")
        })?;
        let mut clippy = vec!["clippy", "--workspace"];
        let not_bevy = packages("--exclude");
        clippy.extend(not_bevy.iter().map(String::as_str));
        clippy.extend(["--", "-D", "warnings"]);
        stage(
            root,
            "cargo clippy --workspace (minus Bevy, -D warnings)",
            &cargo,
            &clippy,
        )?;
        let mut test = vec!["test", "--workspace"];
        test.extend(not_bevy.iter().map(String::as_str));
        stage(root, "cargo test --workspace (minus Bevy)", &cargo, &test)?;
        stage(
            root,
            "world.schema.json snapshot",
            &cargo,
            &["test", "-p", "localgpt-world-types", "--features", "schema"],
        )?;
    }

    if want.bevy {
        let bevy = packages("-p");
        let mut clippy = vec!["clippy"];
        clippy.extend(bevy.iter().map(String::as_str));
        clippy.extend(["--", "-D", "warnings"]);
        stage(
            root,
            "cargo clippy (app, gen, doc, verse, world-bevy, world-audio)",
            &cargo,
            &clippy,
        )?;
        let mut test = vec!["test"];
        test.extend(bevy.iter().map(String::as_str));
        stage(
            root,
            "cargo test (app, gen, doc, verse, world-bevy, world-audio)",
            &cargo,
            &test,
        )?;
    }

    if want.features {
        stage(
            root,
            "cargo check -p localgpt-verse --features ml",
            &cargo,
            &["check", "-p", "localgpt-verse", "--features", "ml"],
        )?;
    }

    if want.web {
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
        need(npm, &["--version"], "install node 22")?;
        need(
            "wasm-pack",
            &["--version"],
            "cargo install wasm-pack (and rustup target add wasm32-unknown-unknown)",
        )?;
        let page = root.join("crates/app/ui");
        modules(&page, npm)?;
        stage(
            &page,
            "the phone's page: typecheck and build",
            npm,
            &["run", "build"],
        )?;
        stage(
            &page,
            "the phone's page: its design sheet",
            npm,
            &["run", "design:build"],
        )?;
        println!("\n──────── viewer is the published openworldformat renderer ────────");
        crate::viewer::run(root, &["--check".to_string()])
            .map_err(|e| format!("{e}\n\n'the viewer check' FAILED — nothing after it was run."))?;
        let world = root.join("website-world");
        modules(&world, npm)?;
        stage(
            &world,
            "localgpt.world: conformance render",
            npm,
            &["run", "check"],
        )?;
        let md = root.join("website-md");
        modules(&md, npm)?;
        stage(&md, "localgpt.md: assemble", npm, &["run", "assemble"])?;
        stage(&md, "localgpt.md: page check", npm, &["run", "check"])?;
    }

    if want.deps {
        need("cargo-deny", &["--version"], "cargo install cargo-deny")?;
        stage(
            root,
            "cargo deny check licenses bans sources",
            &cargo,
            &["deny", "check", "licenses", "bans", "sources"],
        )?;
        stage(
            root,
            "cargo deny check advisories",
            &cargo,
            &["deny", "check", "advisories"],
        )?;
    }

    println!(
        "\nverify: all selected checks passed in {}s.",
        started.elapsed().as_secs()
    );
    Ok(())
}

/// One check, its output as it comes; the first that fails stops the rest.
fn stage(dir: &Path, name: &str, program: &str, args: &[&str]) -> Result<(), String> {
    println!("\n──────── {name} ────────");
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| format!("'{name}': {program} would not start: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("\n'{name}' FAILED — nothing after it was run."))
    }
}

/// A tool the checks need, or what to install.
fn need(program: &str, probe: &[&str], hint: &str) -> Result<(), String> {
    match Command::new(program).args(probe).output() {
        Ok(_) => Ok(()),
        Err(_) => Err(format!("{program} is not on PATH — {hint}")),
    }
}

/// A page's packages, installed as CI installs them when they are missing.
fn modules(dir: &Path, npm: &str) -> Result<(), String> {
    if dir.join("node_modules").is_dir() {
        return Ok(());
    }
    println!("==> {}: packages missing — npm ci", dir.display());
    stage(dir, "npm ci", npm, &["ci"])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn no_group_named_is_every_group() {
        let all = wanted(&[]).unwrap().unwrap();
        assert!(all.rust && all.bevy && all.features && all.web && all.deps);
        let some = wanted(&args(&["--rust", "--deps"])).unwrap().unwrap();
        assert_eq!(
            some,
            Want {
                rust: true,
                deps: true,
                ..Want::default()
            }
        );
        assert_eq!(wanted(&args(&["--help"])).unwrap(), None);
        assert!(wanted(&args(&["--everything"])).is_err());
    }
}
