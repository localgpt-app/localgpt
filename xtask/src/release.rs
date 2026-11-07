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

  zap trash: [
    "~/.local/state/localgpt",
  ]
end
"##
    )
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
}
