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

pub const NPM_USAGE: &str = "  npm-package <version> <binaries-dir> <out-dir>
                   the npm packages for `localgpt` (`@localgpt/cli-<os>-<arch>` + `localgpt`)";

struct NpmTarget {
    rust_target: &'static str,
    dir_name: &'static str,
    pkg_name: &'static str,
    os: &'static str,
    cpu: &'static str,
    bin_name: &'static str,
}

const NPM_TARGETS: &[NpmTarget] = &[
    NpmTarget {
        rust_target: "aarch64-apple-darwin",
        dir_name: "cli-darwin-arm64",
        pkg_name: "@localgpt/cli-darwin-arm64",
        os: "darwin",
        cpu: "arm64",
        bin_name: "localgpt",
    },
    NpmTarget {
        rust_target: "x86_64-apple-darwin",
        dir_name: "cli-darwin-x64",
        pkg_name: "@localgpt/cli-darwin-x64",
        os: "darwin",
        cpu: "x64",
        bin_name: "localgpt",
    },
    NpmTarget {
        rust_target: "x86_64-unknown-linux-gnu",
        dir_name: "cli-linux-x64",
        pkg_name: "@localgpt/cli-linux-x64",
        os: "linux",
        cpu: "x64",
        bin_name: "localgpt",
    },
    NpmTarget {
        rust_target: "aarch64-unknown-linux-gnu",
        dir_name: "cli-linux-arm64",
        pkg_name: "@localgpt/cli-linux-arm64",
        os: "linux",
        cpu: "arm64",
        bin_name: "localgpt",
    },
    NpmTarget {
        rust_target: "x86_64-pc-windows-msvc",
        dir_name: "cli-win32-x64",
        pkg_name: "@localgpt/cli-win32-x64",
        os: "win32",
        cpu: "x64",
        bin_name: "localgpt.exe",
    },
];

/// `cargo xtask npm-package <version> <binaries-dir> <out-dir>`: writes the
/// platform packages (`@localgpt/cli-<os>-<arch>`) for every target binary in
/// `<binaries-dir>` and the root `localgpt` launcher package into `<out-dir>`.
/// Generated at release time so no JS or package.json lives in git outside
/// `xtask/rule.txt`.
pub fn npm_package(root: &Path, args: &[String]) -> Result<(), String> {
    let [version, binaries_dir, out_dir] = args else {
        return Err(format!(
            "usage: cargo xtask npm-package <version> <binaries-dir> <out-dir>\n{NPM_USAGE}"
        ));
    };
    let binaries_dir = Path::new(binaries_dir);
    let out_dir = Path::new(out_dir);
    let mut packaged = Vec::new();
    for target in NPM_TARGETS {
        let Some(src) = find_npm_binary(binaries_dir, target) else {
            continue;
        };
        let pkg_dir = out_dir.join(target.dir_name);
        let bin_dir = pkg_dir.join("bin");
        std::fs::create_dir_all(&bin_dir).map_err(|e| format!("{}: {e}", bin_dir.display()))?;
        let dst = bin_dir.join(target.bin_name);
        std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        make_executable(&dst)?;
        std::fs::write(
            pkg_dir.join("package.json"),
            npm_platform_package_json(target, version),
        )
        .map_err(|e| format!("{}/package.json: {e}", pkg_dir.display()))?;
        std::fs::write(
            pkg_dir.join("README.md"),
            format!(
                "# `{}`\n\nPrebuilt `{}-{}` binary for [`localgpt`](https://www.npmjs.com/package/localgpt).\n",
                target.pkg_name, target.os, target.cpu
            ),
        )
        .map_err(|e| format!("{}/README.md: {e}", pkg_dir.display()))?;
        let license = root.join("LICENSE");
        if license.is_file() {
            let _ = std::fs::copy(&license, pkg_dir.join("LICENSE"));
        }
        println!("packaged  {} ({version})", target.pkg_name);
        packaged.push(target);
    }
    if packaged.is_empty() {
        return Err(format!(
            "no target binaries found under {}",
            binaries_dir.display()
        ));
    }

    let main_dir = out_dir.join("localgpt");
    let main_bin = main_dir.join("bin");
    std::fs::create_dir_all(&main_bin).map_err(|e| format!("{}: {e}", main_bin.display()))?;
    let launcher = main_bin.join("localgpt.js");
    std::fs::write(&launcher, npm_launcher_js())
        .map_err(|e| format!("{}: {e}", launcher.display()))?;
    make_executable(&launcher)?;
    std::fs::write(
        main_dir.join("package.json"),
        npm_root_package_json(version, &packaged),
    )
    .map_err(|e| format!("{}/package.json: {e}", main_dir.display()))?;
    for doc in ["README.md", "LICENSE"] {
        let src = root.join(doc);
        if src.is_file() {
            let _ = std::fs::copy(&src, main_dir.join(doc));
        }
    }
    println!("packaged  localgpt ({version})");
    Ok(())
}

fn find_npm_binary(binaries_dir: &Path, target: &NpmTarget) -> Option<std::path::PathBuf> {
    [
        binaries_dir.join(target.rust_target).join(target.bin_name),
        binaries_dir
            .join(format!("cli-{}", target.rust_target))
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

fn npm_platform_package_json(target: &NpmTarget, version: &str) -> String {
    let value = serde_json::json!({
        "name": target.pkg_name,
        "version": version,
        "description": format!("Prebuilt {}-{} binary for localgpt", target.os, target.cpu),
        "license": "Apache-2.0",
        "repository": {
            "type": "git",
            "url": "git+https://github.com/localgpt-app/localgpt.git"
        },
        "homepage": "https://localgpt.app",
        "os": [target.os],
        "cpu": [target.cpu],
        "files": ["bin"],
        "preferUnplugged": true
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}

fn npm_root_package_json(version: &str, packaged: &[&NpmTarget]) -> String {
    let mut optional = serde_json::Map::new();
    for target in packaged {
        optional.insert(target.pkg_name.into(), version.into());
    }
    let value = serde_json::json!({
        "name": "localgpt",
        "version": version,
        "description": "LocalGPT's command line: a .world package's authority, the MCP shim that gives any agent tools against an open world, and deterministic screenplay-to-previs",
        "license": "Apache-2.0",
        "repository": {
            "type": "git",
            "url": "git+https://github.com/localgpt-app/localgpt.git"
        },
        "homepage": "https://localgpt.app",
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
        "files": ["bin"],
        "engines": {
            "node": ">=16"
        },
        "optionalDependencies": optional
    });
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    text.push('\n');
    text
}

fn npm_launcher_js() -> &'static str {
    r#"#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");
const path = require("node:path");

const PLATFORMS = {
  "darwin-arm64": "@localgpt/cli-darwin-arm64/bin/localgpt",
  "darwin-x64": "@localgpt/cli-darwin-x64/bin/localgpt",
  "linux-x64": "@localgpt/cli-linux-x64/bin/localgpt",
  "linux-arm64": "@localgpt/cli-linux-arm64/bin/localgpt",
  "win32-x64": "@localgpt/cli-win32-x64/bin/localgpt.exe",
};

const key = `${process.platform}-${process.arch}`;
const target = PLATFORMS[key];
if (!target) {
  console.error(
    `localgpt: unsupported platform ${key} (available: ${Object.keys(PLATFORMS).join(", ")})`
  );
  process.exit(1);
}

let bin;
try {
  bin = require.resolve(target);
} catch {
  const pkg = target.split("/").slice(0, 2).join("/");
  console.error(
    `localgpt: could not find ${pkg} for ${key}.\n` +
      `Reinstall without --no-optional, or install with: cargo install localgpt`
  );
  process.exit(1);
}

const result = spawnSync(bin, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: true,
});

if (result.error) {
  console.error(`localgpt: failed to start ${path.basename(bin)}: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
"#
}

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
    }

    #[test]
    fn npm_package_writes_platform_and_root_packages() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let scratch = std::env::temp_dir().join(format!("xtask-npm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let bins = scratch.join("bins");
        let out = scratch.join("out");
        let arm_dir = bins.join("cli-aarch64-apple-darwin");
        let win_dir = bins.join("x86_64-pc-windows-msvc");
        std::fs::create_dir_all(&arm_dir).unwrap();
        std::fs::create_dir_all(&win_dir).unwrap();
        std::fs::write(arm_dir.join("localgpt"), b"arm64-bin").unwrap();
        std::fs::write(win_dir.join("localgpt.exe"), b"win-bin").unwrap();

        npm_package(
            root,
            &[
                "0.5.0".to_string(),
                bins.to_string_lossy().into_owned(),
                out.to_string_lossy().into_owned(),
            ],
        )
        .unwrap();

        let arm_pkg: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(out.join("cli-darwin-arm64/package.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(arm_pkg["name"], "@localgpt/cli-darwin-arm64");
        assert_eq!(arm_pkg["version"], "0.5.0");
        assert_eq!(arm_pkg["os"], serde_json::json!(["darwin"]));
        assert_eq!(arm_pkg["cpu"], serde_json::json!(["arm64"]));
        assert!(out.join("cli-darwin-arm64/bin/localgpt").is_file());
        assert!(out.join("cli-win32-x64/bin/localgpt.exe").is_file());

        let root_pkg: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(out.join("localgpt/package.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(root_pkg["name"], "localgpt");
        assert_eq!(root_pkg["bin"]["localgpt"], "bin/localgpt.js");
        assert_eq!(
            root_pkg["optionalDependencies"]["@localgpt/cli-darwin-arm64"],
            "0.5.0"
        );
        assert_eq!(
            root_pkg["optionalDependencies"]["@localgpt/cli-win32-x64"],
            "0.5.0"
        );
        assert!(
            root_pkg["optionalDependencies"]
                .get("@localgpt/cli-linux-x64")
                .is_none()
        );
        assert!(out.join("localgpt/bin/localgpt.js").is_file());

        let _ = std::fs::remove_dir_all(&scratch);
    }
}
