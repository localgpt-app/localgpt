//! CLI subcommand: `localgpt gen …` — forward to `localgpt-app gen …` (or
//! standalone `localgpt-gen` when installed on its own).
//!
//! Keeping the heavy Bevy + GPU engine in `localgpt-app` (and `localgpt-gen`)
//! lets `localgpt` stay fast and tiny across crates.io, npm and CI while giving
//! users and MCP clients one command name (`localgpt gen mcp-server`,
//! `localgpt gen --live`, …).
//!
//! What needs no renderer runs here instead, with nothing else installed
//! ([`native`]): `mcp-server --connect`, the stdio end of a window's relay,
//! and `--export-session`, a session's package zipped. Both behave as Gen's
//! own; anything else, or anything they do not parse, goes to Gen as typed.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use localgpt_world_agent::mcp::relay;
use localgpt_world_agent::paths::Dirs;
use localgpt_world_agent::session;

/// The part of Gen's command line `localgpt` runs itself. Its flags are
/// Gen's, spelled as Gen spells them; it takes no help or version, so those
/// reach Gen.
#[derive(Parser, Debug, PartialEq)]
#[command(
    no_binary_name = true,
    disable_help_flag = true,
    disable_version_flag = true
)]
struct Native {
    #[arg(short, long, global = true)]
    verbose: bool,
    #[arg(long)]
    export_session: Option<String>,
    #[arg(long, requires = "export_session")]
    out: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<NativeCommand>,
}

#[derive(Subcommand, Debug, PartialEq)]
enum NativeCommand {
    #[command(disable_help_flag = true)]
    McpServer {
        #[arg(long)]
        connect: Option<Option<u16>>,
    },
}

/// What `localgpt gen` does without Gen.
#[derive(Debug, PartialEq)]
enum Job {
    /// `mcp-server --connect [PORT]`: carry stdio to a window's relay.
    Connect(Option<u16>),
    /// `--export-session <NAME|DIR> [--out FILE]`.
    Export {
        session: String,
        out: Option<PathBuf>,
    },
}

/// The job, when `args` ask for one of them and nothing else; with it,
/// whether `-v` was among them.
fn native(args: &[OsString]) -> Option<(bool, Job)> {
    match Native::try_parse_from(args).ok()? {
        Native {
            verbose,
            export_session: None,
            out: None,
            command:
                Some(NativeCommand::McpServer {
                    connect: Some(port),
                }),
        } => Some((verbose, Job::Connect(port))),
        Native {
            verbose,
            export_session: Some(session),
            out,
            command: None,
        } => Some((verbose, Job::Export { session, out })),
        _ => None,
    }
}

fn run_native(verbose: bool, job: Job) -> Result<()> {
    // Logs go to stderr: stdout is the transport while the bridge runs.
    let level = if verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .with_writer(std::io::stderr)
        .init();
    match job {
        Job::Connect(asked) => {
            let port = relay::port_or_discovered(asked)?;
            tracing::info!("connecting to the MCP relay on port {port}");
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(relay::run_stdio_bridge(port))
        }
        Job::Export { session, out } => {
            // Gen's workspace, by the rule Gen resolves it with.
            let workspace = Dirs::resolve()
                .unwrap_or_else(|| Dirs {
                    data: PathBuf::from("."),
                    state: PathBuf::from("."),
                })
                .gen_workspace();
            let (dir, dest) = session::export(&workspace, &session, out.as_deref())?;
            let kb = std::fs::metadata(&dest).map_or(0, |m| m.len() / 1024);
            eprintln!("exported {} → {} ({kb} KB)", dir.display(), dest.display());
            Ok(())
        }
    }
}

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

/// Run what needs no renderer here; replace or spawn the Gen front end with
/// anything else, `args` attached verbatim (including stdin/stdout/stderr for
/// `mcp-server`).
pub fn run(verbose: bool, args: Vec<OsString>) -> Result<()> {
    if let Some((asked_verbose, job)) = native(&args) {
        return run_native(verbose || asked_verbose, job);
    }
    let Some(target) = resolve_target() else {
        if cfg!(target_os = "macos") {
            bail!(
                "could not find `localgpt-app` (LocalGPT.app) or `localgpt-gen`.\n\
                 Install Gen (`cargo install localgpt-gen`), or build LocalGPT.app from source \
                 (https://localgpt.app/docs/installation/)."
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

    fn os(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// The relay's stdio end and a session's export need no renderer, so
    /// they run here, as Gen's flags spell them.
    #[test]
    fn connect_and_export_run_here() {
        let job = |args: &[&str]| native(&os(args));
        assert_eq!(
            job(&["mcp-server", "--connect"]),
            Some((false, Job::Connect(None)))
        );
        assert_eq!(
            job(&["mcp-server", "--connect", "9878"]),
            Some((false, Job::Connect(Some(9878))))
        );
        assert_eq!(
            job(&["-v", "mcp-server", "--connect=4242"]),
            Some((true, Job::Connect(Some(4242))))
        );
        assert_eq!(
            job(&["mcp-server", "--connect", "--verbose"]),
            Some((true, Job::Connect(None)))
        );
        assert_eq!(
            job(&["--export-session", "castle"]),
            Some((
                false,
                Job::Export {
                    session: "castle".into(),
                    out: None
                }
            ))
        );
        assert_eq!(
            job(&["--export-session", "castle", "--out", "c.world"]),
            Some((
                false,
                Job::Export {
                    session: "castle".into(),
                    out: Some(PathBuf::from("c.world"))
                }
            ))
        );
    }

    /// Everything else is Gen's, help included, and so is anything these
    /// jobs come with that they do not take.
    #[test]
    fn everything_else_goes_to_gen() {
        for args in [
            &[][..],
            &["--help"],
            &["--version"],
            &["mcp-server"],
            &["mcp-server", "--headless"],
            &["mcp-server", "--help"],
            &["mcp-server", "--connect", "--headless"],
            &["mcp-server", "--connect", "--world", "castle"],
            &["--mcp-relay"],
            &["--live", "scene.world"],
            &["--export-session", "castle", "--live", "scene.world"],
            &["--replay", "castle"],
            &["--host", "--web"],
        ] {
            assert_eq!(native(&os(args)), None, "{args:?}");
        }
    }

    #[test]
    fn command_prepends_gen_for_app_and_keeps_standalone_args() {
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
