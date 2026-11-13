//! CLI subcommand: `localgpt gen …` — forward to `localgpt-app gen …` (or
//! standalone `localgpt-gen` when installed on its own).
//!
//! Keeping the heavy Bevy + GPU engine in `localgpt-app` (and `localgpt-gen`)
//! lets `localgpt` stay fast and tiny across crates.io, npm and CI while giving
//! users and MCP clients one command name (`localgpt gen mcp-server`,
//! `localgpt gen --live`, …).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

fn exe_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// Hand off to `localgpt-app gen …`.
    App(PathBuf),
    /// Fall back to standalone `localgpt-gen …` (crates.io / dev without app).
    Standalone(PathBuf),
}

impl Target {
    fn command(&self, verbose: bool, args: &[OsString]) -> Command {
        let mut cmd = match self {
            Self::App(path) => {
                let mut cmd = Command::new(path);
                cmd.arg("gen");
                cmd
            }
            Self::Standalone(path) => Command::new(path),
        };
        if verbose && !args.iter().any(|a| a == "-v" || a == "--verbose") {
            cmd.arg("--verbose");
        }
        cmd.args(args);
        cmd
    }
}

fn exe_dirs(exe: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Some(exe) = exe else {
        return dirs;
    };
    if let Ok(canon) = std::fs::canonicalize(exe)
        && let Some(dir) = canon.parent()
    {
        dirs.push(dir.to_path_buf());
    }
    if let Some(dir) = exe.parent()
        && !dirs.iter().any(|seen| seen == dir)
    {
        dirs.push(dir.to_path_buf());
    }
    dirs
}

fn find_in_dirs(dirs: impl IntoIterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    dirs.into_iter()
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn macos_app_candidates(home: Option<&OsStr>) -> Vec<PathBuf> {
    let mut out = vec![PathBuf::from(
        "/Applications/LocalGPT.app/Contents/MacOS/localgpt-app",
    )];
    if let Some(home) = home {
        out.push(PathBuf::from(home).join("Applications/LocalGPT.app/Contents/MacOS/localgpt-app"));
    }
    out
}

fn resolve_target_with(
    exe_dirs: &[PathBuf],
    path_env: Option<&OsStr>,
    app_bundles: &[PathBuf],
    app_name: &str,
    gen_name: &str,
) -> Option<Target> {
    if let Some(path) = find_in_dirs(exe_dirs.iter().cloned(), app_name) {
        return Some(Target::App(path));
    }
    let path_dirs: Vec<PathBuf> = path_env
        .map(|raw| std::env::split_paths(raw).collect())
        .unwrap_or_default();
    if let Some(path) = find_in_dirs(path_dirs.iter().cloned(), app_name) {
        return Some(Target::App(path));
    }
    if let Some(path) = app_bundles.iter().find(|p| p.is_file()) {
        return Some(Target::App(path.clone()));
    }
    if let Some(path) = find_in_dirs(exe_dirs.iter().cloned(), gen_name) {
        return Some(Target::Standalone(path));
    }
    if let Some(path) = find_in_dirs(path_dirs, gen_name) {
        return Some(Target::Standalone(path));
    }
    None
}

fn resolve_target() -> Option<Target> {
    let exe = std::env::current_exe().ok();
    let dirs = exe_dirs(exe.as_deref());
    let path_env = std::env::var_os("PATH");
    let bundles = if cfg!(target_os = "macos") {
        macos_app_candidates(std::env::var_os("HOME").as_deref())
    } else {
        Vec::new()
    };
    resolve_target_with(
        &dirs,
        path_env.as_deref(),
        &bundles,
        &exe_name("localgpt-app"),
        &exe_name("localgpt-gen"),
    )
}

/// Replace or spawn the Gen front end with `args` attached verbatim (including
/// stdin/stdout/stderr for `mcp-server`).
pub fn run(verbose: bool, args: Vec<OsString>) -> Result<()> {
    let Some(target) = resolve_target() else {
        if cfg!(target_os = "macos") {
            bail!(
                "could not find `localgpt-app` (LocalGPT.app) or `localgpt-gen`.\n\
                 Install the desktop app (`brew install --cask localgpt-app/tap/localgpt`), \
                 or install Gen directly (`cargo install localgpt-gen`)."
            );
        } else {
            bail!(
                "could not find `localgpt-app` or `localgpt-gen`.\n\
                 Install Gen (`cargo install localgpt-gen`) or build `localgpt-app` from source."
            );
        }
    };

    let mut cmd = target.command(verbose, &args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(err)
            .with_context(|| format!("failed to execute {}", cmd.get_program().to_string_lossy()))
    }

    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .with_context(|| format!("failed to launch {}", cmd.get_program().to_string_lossy()))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
        path.to_path_buf()
    }

    #[test]
    fn prefers_sibling_app_then_path_then_bundle_then_standalone_gen() {
        let root = TempDir::new().unwrap();
        let sib = root.path().join("sib");
        let path_dir = root.path().join("path");
        let bundle = root.path().join("LocalGPT.app/Contents/MacOS/localgpt-app");

        let sib_gen = touch(&sib.join("localgpt-gen"));
        let path_app = touch(&path_dir.join("localgpt-app"));
        touch(&bundle);
        let path_env = std::env::join_paths([&path_dir]).unwrap();

        // Precedence: sibling app > PATH app > bundle > sibling gen > PATH gen.
        assert_eq!(
            resolve_target_with(
                std::slice::from_ref(&sib),
                Some(path_env.as_os_str()),
                std::slice::from_ref(&bundle),
                "localgpt-app",
                "localgpt-gen",
            ),
            Some(Target::App(path_app))
        );

        // Without PATH app, the macOS app bundle wins over standalone gen.
        assert_eq!(
            resolve_target_with(
                std::slice::from_ref(&sib),
                None,
                std::slice::from_ref(&bundle),
                "localgpt-app",
                "localgpt-gen",
            ),
            Some(Target::App(bundle.clone()))
        );

        // With sibling app present, sibling app wins over everything.
        let sib_app = touch(&sib.join("localgpt-app"));
        assert_eq!(
            resolve_target_with(
                std::slice::from_ref(&sib),
                Some(path_env.as_os_str()),
                std::slice::from_ref(&bundle),
                "localgpt-app",
                "localgpt-gen",
            ),
            Some(Target::App(sib_app.clone()))
        );

        // With neither app anywhere, sibling standalone gen is used.
        std::fs::remove_file(&sib_app).unwrap();
        assert_eq!(
            resolve_target_with(
                std::slice::from_ref(&sib),
                None,
                &[],
                "localgpt-app",
                "localgpt-gen",
            ),
            Some(Target::Standalone(sib_gen))
        );
    }

    #[test]
    fn command_prepends_gen_for_app_and_keeps_standalone_args() {
        let os = |items: &[&str]| items.iter().map(OsString::from).collect::<Vec<_>>();

        let app_cmd = Target::App(PathBuf::from(
            "/Applications/LocalGPT.app/Contents/MacOS/localgpt-app",
        ))
        .command(true, &os(&["mcp-server", "--headless"]));
        let app_args: Vec<_> = app_cmd.get_args().collect();
        assert_eq!(
            app_args,
            os(&["gen", "--verbose", "mcp-server", "--headless"])
        );

        let gen_cmd = Target::Standalone(PathBuf::from("/usr/local/bin/localgpt-gen"))
            .command(false, &os(&["--live", "harbour.world"]));
        let gen_args: Vec<_> = gen_cmd.get_args().collect();
        assert_eq!(gen_args, os(&["--live", "harbour.world"]));
    }
}
