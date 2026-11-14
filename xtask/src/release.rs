//! Release tooling: what crates.io still needs at this version, and making
//! each published crate trust the release workflow (they were
//! `scripts/release-plan.py` and `scripts/trust-release-workflow.py`).
//!
//! HTTPS goes through `curl`, a tool of the system as `codesign` is: the
//! xtask carries no TLS stack of its own (the C under one is what the
//! all-Rust plan's last mile takes out).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// What `cargo xtask release-plan` does, for the usage text.
pub const PLAN_USAGE: &str =
    "  release-plan     which workspace crates crates.io lacks at this version
                   (release.yml's plan; writes $GITHUB_OUTPUT under Actions)";
pub const TRUST_USAGE: &str = "  trust-release [--dry-run]
                   make every published crate trust release.yml
                   (CRATES_IO_TOKEN with the trusted-publishing scope)";

/// A publishable member: its name and version.
#[derive(Debug, Clone, PartialEq)]
struct Member {
    name: String,
    version: String,
}

/// The members cargo would publish: `publish = false` ones are skipped, as
/// cargo skips them.
fn members(root: &Path) -> Result<Vec<Member>, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    Ok(publishable(&meta))
}

fn publishable(meta: &serde_json::Value) -> Vec<Member> {
    let mut members: Vec<Member> = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            p["publish"]
                .as_array()
                .is_none_or(|registries| !registries.is_empty())
        })
        .filter_map(|p| {
            Some(Member {
                name: p["name"].as_str()?.to_string(),
                version: p["version"].as_str()?.to_string(),
            })
        })
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));
    members
}

/// A crate's file in crates.io's sparse index.
fn index_url(name: &str) -> String {
    let n = name.to_lowercase();
    match n.len() {
        0..=2 => format!("https://index.crates.io/{}/{n}", n.len()),
        3 => format!("https://index.crates.io/3/{}/{n}", &n[..1]),
        _ => format!("https://index.crates.io/{}/{}/{n}", &n[..2], &n[2..4]),
    }
}

/// One HTTPS call: the status and the body. Headers go to curl on its
/// standard input, never in its arguments, where a token would show.
fn curl(
    method: &str,
    url: &str,
    headers: &[String],
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let mut args = vec![
        "-sS".to_string(),
        "--max-time".into(),
        "30".into(),
        "-X".into(),
        method.into(),
        "-H".into(),
        "@-".into(),
        "-w".into(),
        "\n%{http_code}".into(),
    ];
    let dir = std::env::temp_dir().join(format!("xtask-curl-{}", std::process::id()));
    if let Some(body) = body {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::fs::write(dir.join("body.json"), body).map_err(|e| format!("a request body: {e}"))?;
        args.push("--data-binary".into());
        args.push(format!("@{}", dir.join("body.json").display()));
    }
    args.push(url.into());
    let mut child = Command::new("curl")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl would not start: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(headers.join("\n").as_bytes())
            .map_err(|e| format!("curl: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    if !out.status.success() {
        return Err(format!(
            "{url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, status) = text.rsplit_once('\n').unwrap_or(("", &text));
    let status = status
        .trim()
        .parse()
        .map_err(|_| format!("{url}: no status"))?;
    Ok((status, body.to_string()))
}

/// The versions crates.io has of a crate; none for one never published.
fn published_versions(name: &str) -> Result<Vec<String>, String> {
    let (status, body) = curl(
        "GET",
        &index_url(name),
        &["User-Agent: localgpt-release-plan".into()],
        None,
    )?;
    match status {
        404 => Ok(Vec::new()),
        200 => Ok(body
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|entry| entry["vers"].as_str().map(str::to_string))
            .collect()),
        other => Err(format!("{}: HTTP {other}", index_url(name))),
    }
}

/// `cargo xtask release-plan`: which crates still need publishing at
/// their current version. The release workflow runs it before
/// `cargo publish --workspace`, so a rerun after a partial publish (a rate
/// limit, a network blip) picks up where the last run stopped. Under
/// GitHub Actions it writes `excludes` (the `--exclude` flags for crates
/// already up) and `remaining` to `$GITHUB_OUTPUT`, and a summary.
pub fn plan(root: &Path) -> Result<(), String> {
    let members = members(root)?;
    if members.is_empty() {
        return Err("no publishable members".into());
    }
    let (mut done, mut left) = (Vec::new(), Vec::new());
    for member in members {
        if published_versions(&member.name)?.contains(&member.version) {
            done.push(member);
        } else {
            left.push(member);
        }
    }
    for m in &done {
        println!("up already  {} {}", m.name, m.version);
    }
    for m in &left {
        println!("to publish  {} {}", m.name, m.version);
    }
    let excludes: Vec<String> = done
        .iter()
        .map(|m| format!("--exclude {}", m.name))
        .collect();
    if let Some(out) = std::env::var_os("GITHUB_OUTPUT") {
        append(
            Path::new(&out),
            &format!(
                "excludes={}\nremaining={}\n",
                excludes.join(" "),
                left.len()
            ),
        )?;
    }
    if let Some(summary) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text = format!(
            "### crates.io\n\n{} to publish, {} up already.\n\n",
            left.len(),
            done.len()
        );
        for m in &left {
            text.push_str(&format!("- `{}` {}\n", m.name, m.version));
        }
        append(Path::new(&summary), &text)?;
    }
    Ok(())
}

fn append(path: &Path, text: &str) -> Result<(), String> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub const CASK_USAGE: &str = "  cask <version> <arm64.dmg> <x86_64.dmg>
                   the Homebrew cask for one desktop release, to stdout";

/// `cargo xtask cask <version> <arm64.dmg> <x86_64.dmg>`: the Homebrew
/// cask for LocalGPT.app, rendered for one release (it was
/// `packaging/homebrew/cask.sh`). It belongs in the tap
/// (github.com/localgpt-app/homebrew-tap) as `Casks/localgpt.rb` once the
/// `desktop-v<version>` release is published, since its URLs are that
/// release's; desktop.yml renders it with the release's files and attaches
/// it to the draft.
pub fn cask(args: &[String]) -> Result<(), String> {
    let [version, arm, intel] = args else {
        return Err(format!(
            "usage: cargo xtask cask <version> <arm64.dmg> <x86_64.dmg>\n{CASK_USAGE}"
        ));
    };
    let (arm, intel) = (sha256(Path::new(arm))?, sha256(Path::new(intel))?);
    print!("{}", cask_text(version, &arm, &intel));
    Ok(())
}

fn sha256(file: &Path) -> Result<String, String> {
    if !file.is_file() {
        return Err(format!("no such file: {}", file.display()));
    }
    let out = Command::new("shasum")
        .args(["-a", "256"])
        .arg(file)
        .output()
        .map_err(|e| format!("shasum would not start: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .filter(|sum| out.status.success() && sum.len() == 64)
        .map(str::to_string)
        .ok_or_else(|| format!("shasum {}: no sum", file.display()))
}

fn cask_text(version: &str, arm: &str, intel: &str) -> String {
    format!(
        r##"# LocalGPT.app for macOS, from the desktop-v{version} GitHub release.
# Rendered by `cargo xtask cask` in the localgpt repository.
cask "localgpt" do
  arch arm: "arm64", intel: "x86_64"

  version "{version}"
  sha256 arm:   "{arm}",
         intel: "{intel}"

  url "https://github.com/localgpt-app/localgpt/releases/download/desktop-v#{{version}}/LocalGPT-#{{version}}-#{{arch}}.dmg"
  name "LocalGPT"
  desc "Desktop app where a prompt, a document, a song or a screenplay becomes a .world"
  homepage "https://localgpt.app/"

  livecheck do
    url :url
    strategy :github_releases
    regex(/^desktop-v?(\d+(?:\.\d+)+)$/i)
  end

  depends_on macos: ">= :big_sur"

  app "LocalGPT.app"
  binary "#{{appdir}}/LocalGPT.app/Contents/MacOS/localgpt"

  zap trash: [
    "~/.local/state/localgpt",
  ]
end
"##
    )
}

pub const NPM_USAGE: &str = "  npm-package <binaries-dir> <out-dir>
                   the `localgpt` npm package, ready for `npm pack`: the launcher and
                   every platform's `localgpt` in vendor/ (npm.yml runs it)";

/// A platform the npm package carries a `localgpt` for.
struct NpmTarget {
    /// Rust's name for it: what the binary was built as.
    rust_target: &'static str,
    /// Node's, `process.platform`-`process.arch`: its folder in `vendor/`.
    node: &'static str,
    bin_name: &'static str,
}

const NPM_TARGETS: [NpmTarget; 5] = [
    NpmTarget {
        rust_target: "aarch64-apple-darwin",
        node: "darwin-arm64",
        bin_name: "localgpt",
    },
    NpmTarget {
        rust_target: "x86_64-apple-darwin",
        node: "darwin-x64",
        bin_name: "localgpt",
    },
    NpmTarget {
        rust_target: "aarch64-unknown-linux-gnu",
        node: "linux-arm64",
        bin_name: "localgpt",
    },
    NpmTarget {
        rust_target: "x86_64-unknown-linux-gnu",
        node: "linux-x64",
        bin_name: "localgpt",
    },
    NpmTarget {
        rust_target: "x86_64-pc-windows-msvc",
        node: "win32-x64",
        bin_name: "localgpt.exe",
    },
];

/// `cargo xtask npm-package <binaries-dir> <out-dir>`: writes the `localgpt`
/// npm package into `<out-dir>`, at the workspace's version — `package.json`,
/// the launcher `bin/localgpt.js`, and each platform's binary from
/// `<binaries-dir>` as `vendor/<platform>-<arch>/localgpt`. It is one
/// package holding all five, so it needs one name on npm and nothing from
/// the registry at install time but itself.
///
/// It is written at release time, so no JavaScript or `package.json` of it
/// is tracked (`xtask/rule.txt`). A platform with no binary stops it: a
/// package missing one would install everywhere and fail there.
pub fn npm_package(root: &Path, args: &[String]) -> Result<(), String> {
    let [binaries_dir, out_dir] = args else {
        return Err(format!(
            "usage: cargo xtask npm-package <binaries-dir> <out-dir>\n{NPM_USAGE}"
        ));
    };
    let version = crate::bundle::version(root)?;
    tag_names(
        std::env::var("GITHUB_REF_TYPE").ok().as_deref(),
        std::env::var("GITHUB_REF_NAME").ok().as_deref(),
        &version,
    )?;
    npm_package_at(root, &version, Path::new(binaries_dir), Path::new(out_dir))
}

/// On a release tag's run, the tag is the version being packed: `v0.7.0`
/// packs 0.7.0 or nothing. Any other run packs what the tree says.
fn tag_names(ref_type: Option<&str>, ref_name: Option<&str>, version: &str) -> Result<(), String> {
    match (ref_type, ref_name) {
        (Some("tag"), Some(tag)) if tag.strip_prefix('v') != Some(version) => Err(format!(
            "the tag is {tag}, and Cargo.toml says {version}: nothing packed"
        )),
        _ => Ok(()),
    }
}

fn npm_package_at(
    root: &Path,
    version: &str,
    binaries_dir: &Path,
    out_dir: &Path,
) -> Result<(), String> {
    let found: Vec<_> = NPM_TARGETS
        .iter()
        .map(|target| (target, find_npm_binary(binaries_dir, target)))
        .collect();
    let missing: Vec<&str> = found
        .iter()
        .filter(|(_, binary)| binary.is_none())
        .map(|(target, _)| target.node)
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "no localgpt for {} under {} (looked for <platform>/, localgpt-<platform>/ \
             and <rust target>/release/)",
            missing.join(", "),
            binaries_dir.display()
        ));
    }

    let write = |path: &Path, text: &str| {
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    };
    let folder =
        |path: &Path| std::fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()));
    for (target, binary) in &found {
        let Some(binary) = binary else { continue };
        let dir = out_dir.join("vendor").join(target.node);
        folder(&dir)?;
        let placed = dir.join(target.bin_name);
        std::fs::copy(binary, &placed).map_err(|e| format!("{}: {e}", binary.display()))?;
        // An artifact comes back from GitHub without its mode.
        make_executable(&placed)?;
        println!("vendor/{}/{}", target.node, target.bin_name);
    }
    let bin = out_dir.join("bin");
    folder(&bin)?;
    let launcher = bin.join("localgpt.js");
    write(&launcher, &npm_launcher_js())?;
    make_executable(&launcher)?;
    write(&out_dir.join("package.json"), &npm_package_json(version))?;
    for doc in ["README.md", "LICENSE"] {
        std::fs::copy(root.join(doc), out_dir.join(doc)).map_err(|e| format!("{doc}: {e}"))?;
    }
    println!("localgpt {version} for npm, in {}", out_dir.display());
    Ok(())
}

/// Where a platform's binary is under `<binaries-dir>`: in a folder named
/// for the platform as Node names it, in the artifact npm.yml's build
/// uploads (`localgpt-<platform>`), or where `cargo build --target` left it,
/// so a target directory serves as it is.
fn find_npm_binary(binaries_dir: &Path, target: &NpmTarget) -> Option<std::path::PathBuf> {
    [
        binaries_dir.join(target.node).join(target.bin_name),
        binaries_dir
            .join(format!("localgpt-{}", target.node))
            .join(target.bin_name),
        binaries_dir
            .join(target.rust_target)
            .join("release")
            .join(target.bin_name),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// The package's `package.json`. `repository` is this repository exactly:
/// npm checks a provenance statement against it and refuses a publish whose
/// package names another. No `os` or `cpu`: where there is no binary the
/// launcher says so, which is kinder than an install that fails for a
/// project that merely depends on this.
fn npm_package_json(version: &str) -> String {
    let value = serde_json::json!({
        "name": "localgpt",
        "version": version,
        "description": "LocalGPT's command line, prebuilt: a .world package's authority, the MCP shim that gives any agent tools against an open world, and deterministic screenplay-to-previs",
        "license": "Apache-2.0",
        "homepage": "https://localgpt.app",
        "repository": {
            "type": "git",
            "url": "git+https://github.com/localgpt-app/localgpt.git"
        },
        "bugs": {
            "url": "https://github.com/localgpt-app/localgpt/issues"
        },
        "keywords": [
            "localgpt",
            "world",
            "mcp",
            "agents",
            "3d",
            "previs",
            "openworldformat"
        ],
        "bin": {
            "localgpt": "bin/localgpt.js"
        },
        "files": ["bin", "vendor"],
        "engines": {
            "node": ">=18"
        }
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}

/// `bin/localgpt.js`: what npm puts on `PATH`. It starts the binary for
/// this computer with the same arguments, streams and exit status, so
/// `localgpt world mcp` through it is the stdio server it is without it.
fn npm_launcher_js() -> String {
    let platforms: Vec<&str> = NPM_TARGETS.iter().map(|target| target.node).collect();
    NPM_LAUNCHER.replace(
        "__PLATFORMS__",
        &serde_json::to_string(&platforms).unwrap_or_default(),
    )
}

const NPM_LAUNCHER: &str = r#"#!/usr/bin/env node
"use strict";

// Starts the localgpt built for this computer, from vendor/<platform>-<arch>/.
// Written by `cargo xtask npm-package` in github.com/localgpt-app/localgpt.

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const PLATFORMS = __PLATFORMS__;
const FROM_SOURCE =
  "With Rust installed, `cargo install localgpt` builds it here: https://localgpt.app/docs/installation/";

let platform = `${process.platform}-${process.arch}`;
// Windows on Arm runs x64 programs.
if (platform === "win32-arm64") platform = "win32-x64";

if (!PLATFORMS.includes(platform)) {
  console.error(
    `localgpt: no prebuilt binary for ${platform} (there is one for ${PLATFORMS.join(", ")}).\n${FROM_SOURCE}`
  );
  process.exit(1);
}

const bin = path.join(
  __dirname,
  "..",
  "vendor",
  platform,
  process.platform === "win32" ? "localgpt.exe" : "localgpt"
);
const start = () =>
  spawnSync(bin, process.argv.slice(2), { stdio: "inherit", windowsHide: true });

let result = start();
if (result.error && result.error.code === "EACCES") {
  // Unpacked by something that dropped the executable bit.
  try {
    fs.chmodSync(bin, 0o755);
    result = start();
  } catch {
    // The first error is the one to report.
  }
}
if (result.error) {
  // A file that is there and "not found" is its loader missing: a Linux
  // without glibc, such as Alpine.
  const noLoader =
    process.platform === "linux" && result.error.code === "ENOENT" && fs.existsSync(bin);
  console.error(
    noLoader
      ? `localgpt: the prebuilt binary needs glibc, which this system does not have.\n${FROM_SOURCE}`
      : `localgpt: could not start ${bin}: ${result.error.message}`
  );
  process.exit(1);
}
if (result.signal) {
  // Ended by a signal: end the same way, so whoever started this sees it.
  process.kill(process.pid, result.signal);
}
process.exit(result.status === null ? 1 : result.status);
"#;

const API: &str = "https://crates.io/api/v1";
/// The configuration every crate is to trust: what release.yml runs as.
const WANT: [(&str, &str); 4] = [
    ("repository_owner", "localgpt-app"),
    ("repository_name", "localgpt"),
    ("workflow_filename", "release.yml"),
    ("environment", "crates-io"),
];

fn api(
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> Result<(u16, serde_json::Value), String> {
    let mut headers = vec![
        "User-Agent: localgpt-trust-release-workflow".to_string(),
        "Accept: application/json".into(),
    ];
    if let Some(token) = token {
        headers.push(format!("Authorization: {token}"));
    }
    if body.is_some() {
        headers.push("Content-Type: application/json".into());
    }
    let (status, text) = curl(method, &format!("{API}{path}"), &headers, body)?;
    let value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    Ok((status, value))
}

/// Whether a crate's GitHub configurations hold the one release.yml needs.
fn trusts(listing: &serde_json::Value) -> bool {
    listing["github_configs"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|config| WANT.iter().all(|(key, value)| config[*key] == *value))
}

/// `cargo xtask trust-release [--dry-run]`: crates.io's Trusted Publishing
/// is configured per crate; this gives each publishable member that exists
/// on crates.io the configuration release.yml runs as. Crates that have it
/// are left alone; crates not on crates.io yet are listed (crates.io can
/// trust a workflow only for a crate that exists: publish it once, then run
/// this again). The token, `CRATES_IO_TOKEN`, needs the
/// `trusted-publishing` scope, best limited to `localgpt*`; delete it once
/// this has run, since the workflow itself needs none.
pub fn trust(root: &Path, args: &[String]) -> Result<(), String> {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let token = std::env::var("CRATES_IO_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    if token.is_none() && !dry_run {
        return Err(format!("CRATES_IO_TOKEN is not set\n\n{TRUST_USAGE}"));
    }
    let mut failed = false;
    for member in members(root)? {
        let name = member.name;
        let (status, _) = api("GET", &format!("/crates/{name}"), None, None)?;
        if status == 404 {
            println!("not yet on crates.io  {name}: publish it once, then run this again");
            continue;
        }
        let Some(token) = token.as_deref() else {
            // A dry run without a token cannot list configurations.
            println!("would trust           {name}");
            continue;
        };
        let (status, listing) = api(
            "GET",
            &format!("/trusted_publishing/github_configs?crate={name}"),
            Some(token),
            None,
        )?;
        if status != 200 {
            println!("cannot list           {name}: {status} {listing}");
            failed = true;
            continue;
        }
        if trusts(&listing) {
            println!("trusts already        {name}");
            continue;
        }
        let others = listing["github_configs"].as_array().map_or(0, Vec::len);
        if dry_run {
            println!("would trust           {name} ({others} other configuration(s) kept)");
            continue;
        }
        let mut config = serde_json::Map::new();
        config.insert("crate".into(), name.clone().into());
        for (key, value) in WANT {
            config.insert(key.into(), value.into());
        }
        let body = serde_json::json!({ "github_config": config }).to_string();
        let (status, created) = api(
            "POST",
            "/trusted_publishing/github_configs",
            Some(token),
            Some(&body),
        )?;
        if status == 200 {
            println!("trusts now            {name}");
        } else {
            println!("refused               {name}: {status} {created}");
            failed = true;
        }
    }
    if failed {
        Err("some crates were not trusted (above)".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sparse_index_is_laid_out_by_name_length() {
        assert_eq!(index_url("a"), "https://index.crates.io/1/a");
        assert_eq!(index_url("ab"), "https://index.crates.io/2/ab");
        assert_eq!(index_url("abc"), "https://index.crates.io/3/a/abc");
        assert_eq!(index_url("Serde"), "https://index.crates.io/se/rd/serde");
        assert_eq!(
            index_url("localgpt-world-types"),
            "https://index.crates.io/lo/ca/localgpt-world-types"
        );
    }

    #[test]
    fn publish_false_members_are_skipped() {
        let meta = serde_json::json!({"packages": [
            {"name": "zeta", "version": "0.5.0", "publish": null},
            {"name": "localgpt-app", "version": "0.5.0", "publish": []},
            {"name": "alpha", "version": "0.5.0", "publish": ["crates-io"]},
        ]});
        let names: Vec<String> = publishable(&meta).into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["alpha", "zeta"]);
    }

    #[test]
    fn a_crate_trusts_the_workflow_only_with_every_field() {
        let mut config = serde_json::Map::new();
        for (key, value) in WANT {
            config.insert(key.into(), value.into());
        }
        let good = serde_json::json!({ "github_configs": [config.clone()] });
        assert!(trusts(&good));
        config.insert("environment".into(), "staging".into());
        let other = serde_json::json!({ "github_configs": [config] });
        assert!(!trusts(&other));
        assert!(!trusts(&serde_json::json!({})));
    }

    #[test]
    fn the_cask_links_the_cli_from_the_app_bundle() {
        let rendered = cask_text("0.5.0", "aaa", "bbb");
        assert!(rendered.contains("app \"LocalGPT.app\""));
        assert!(rendered.contains("binary \"#{appdir}/LocalGPT.app/Contents/MacOS/localgpt\""));
        // Ruby reads it either way; `brew style` reads the indentation: a
        // stanza at two spaces, `end` at the margin, nothing trailing.
        for stanza in ["version", "depends_on", "app", "binary", "zap"] {
            assert!(
                rendered.contains(&format!("\n  {stanza} ")),
                "{stanza} is not a stanza at two spaces"
            );
        }
        assert!(rendered.ends_with("\nend\n"));
        assert!(rendered.lines().all(|line| line.trim_end() == line));
    }

    /// A scratch folder holding a stand-in binary for every platform, each
    /// in one of the three places the command looks. A stand-in is a shell
    /// script that says what it was given and leaves with status 7.
    fn stand_ins(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let scratch = std::env::temp_dir().join(format!("xtask-npm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let bins = scratch.join("bins");
        for (index, target) in NPM_TARGETS.iter().enumerate() {
            let dir = match index % 3 {
                0 => bins.join(target.node),
                1 => bins.join(format!("localgpt-{}", target.node)),
                _ => bins.join(target.rust_target).join("release"),
            };
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(target.bin_name),
                "#!/bin/sh\necho \"ran: $*\"\nexit 7\n",
            )
            .unwrap();
        }
        (scratch.clone(), bins)
    }

    fn repo() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
    }

    #[test]
    fn the_npm_package_is_one_package_with_every_platform() {
        let (scratch, bins) = stand_ins("whole");
        let out = scratch.join("out");
        npm_package_at(repo(), "1.2.3", &bins, &out).unwrap();

        let package: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(out.join("package.json")).unwrap())
                .unwrap();
        assert_eq!(package["name"], "localgpt");
        assert_eq!(package["version"], "1.2.3");
        assert_eq!(package["bin"]["localgpt"], "bin/localgpt.js");
        assert_eq!(package["files"], serde_json::json!(["bin", "vendor"]));
        // npm holds a provenance statement to this, to the letter.
        assert_eq!(
            package["repository"]["url"],
            "git+https://github.com/localgpt-app/localgpt.git"
        );
        // One package: nothing else to fetch, nothing run at install, and
        // no platform turned away by npm before the launcher can speak.
        for absent in [
            "dependencies",
            "optionalDependencies",
            "scripts",
            "os",
            "cpu",
        ] {
            assert!(package.get(absent).is_none(), "package.json has {absent}");
        }

        let launcher = std::fs::read_to_string(out.join("bin/localgpt.js")).unwrap();
        assert!(launcher.starts_with("#!/usr/bin/env node\n"));
        assert!(!launcher.contains("__PLATFORMS__"));
        for target in &NPM_TARGETS {
            let binary = out.join("vendor").join(target.node).join(target.bin_name);
            assert!(binary.is_file(), "{} is not packed", binary.display());
            assert!(
                launcher.contains(&format!("\"{}\"", target.node)),
                "the launcher does not know {}",
                target.node
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&binary).unwrap().permissions().mode();
                assert_eq!(mode & 0o111, 0o111, "{} cannot be run", binary.display());
            }
        }
        assert!(out.join("README.md").is_file());
        assert!(out.join("LICENSE").is_file());
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_platform_with_no_binary_stops_the_package() {
        let (scratch, bins) = stand_ins("short");
        std::fs::remove_dir_all(bins.join("localgpt-darwin-x64")).unwrap();
        let out = scratch.join("out");
        let refused = npm_package_at(repo(), "1.2.3", &bins, &out).unwrap_err();
        assert!(refused.contains("darwin-x64"), "{refused}");
        assert!(!refused.contains("linux-x64"), "{refused}");
        assert!(
            !out.join("package.json").exists(),
            "a package was started without it"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_release_tag_names_the_version_it_packs() {
        assert!(tag_names(Some("tag"), Some("v0.7.0"), "0.7.0").is_ok());
        assert!(tag_names(Some("tag"), Some("v0.7.0-rc.1"), "0.7.0-rc.1").is_ok());
        let refused = tag_names(Some("tag"), Some("v0.7.1"), "0.7.0").unwrap_err();
        assert!(
            refused.contains("v0.7.1") && refused.contains("0.7.0"),
            "{refused}"
        );
        assert!(tag_names(Some("tag"), Some("0.7.0"), "0.7.0").is_err());
        // A branch's run, and a run outside GitHub, pack what the tree says.
        assert!(tag_names(Some("branch"), Some("main"), "0.7.0").is_ok());
        assert!(tag_names(None, None, "0.7.0").is_ok());
    }

    /// This computer as Node names it, when the package has a binary for it.
    #[cfg(unix)]
    fn this_platform() -> Option<&'static str> {
        let node = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "darwin-arm64",
            ("macos", "x86_64") => "darwin-x64",
            ("linux", "aarch64") => "linux-arm64",
            ("linux", "x86_64") => "linux-x64",
            _ => return None,
        };
        Some(node)
    }

    /// The launcher itself, run by node over the stand-in for this
    /// computer: it is the one piece of the package that is not Rust, and
    /// what every `localgpt` from npm starts through.
    #[cfg(unix)]
    #[test]
    fn the_launcher_hands_over_arguments_streams_and_the_exit_status() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let Some(platform) = this_platform() else {
            return;
        };
        if Command::new("node").arg("--version").output().is_err() {
            assert!(
                std::env::var_os("CI").is_none(),
                "node is not installed, and CI is where the launcher must be run"
            );
            eprintln!("node is not installed: the launcher was not run");
            return;
        }
        let (scratch, bins) = stand_ins("launcher");
        let out = scratch.join("out");
        npm_package_at(repo(), "1.2.3", &bins, &out).unwrap();
        let launch = || {
            Command::new("node")
                .arg(out.join("bin/localgpt.js"))
                .args(["world", "mcp", "--flag"])
                .output()
                .unwrap()
        };

        let ran = launch();
        assert_eq!(
            String::from_utf8_lossy(&ran.stdout),
            "ran: world mcp --flag\n"
        );
        assert_eq!(ran.status.code(), Some(7));

        // Unpacked without its mode, it is made runnable and run.
        let binary = out.join("vendor").join(platform).join("localgpt");
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o644)).unwrap();
        let ran = launch();
        assert_eq!(
            String::from_utf8_lossy(&ran.stdout),
            "ran: world mcp --flag\n"
        );
        assert_eq!(ran.status.code(), Some(7));

        // Gone, it says which file would not start.
        std::fs::remove_file(&binary).unwrap();
        let failed = launch();
        assert_eq!(failed.status.code(), Some(1));
        let said = String::from_utf8_lossy(&failed.stderr);
        assert!(said.contains("localgpt: could not start"), "{said}");
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
