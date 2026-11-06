//! The all-Rust rule's ratchet (`docs/rfcs/authoring/all-rust.md`, decided
//! 2026-11-05): Rust above the operating system. What is not Rust yet is
//! listed in `xtask/rule.txt`, and the list may only shrink:
//!
//! - a tracked file in another language that is not listed fails, and so
//!   does a crate of the workspace that depends on `wry` and is not listed;
//! - a listed file that is gone, or a listed crate that no longer depends on
//!   `wry`, fails too, so the list stays the truth (`--write` drops those
//!   entries, and never adds one).
//!
//! An entry is added only by hand, so it is seen in review with its reason.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// The languages the rule counts, by extension. Markup, styles and data
/// (HTML, CSS, TOML, JSON, Markdown) are not code in another language; the
/// sites' templates stay Zola's.
const EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts", "py", "sh", "bash", "zsh", "fish", "ps1",
    "cmd", "bat", "rb", "pl", "lua", "go", "c", "cc", "cpp", "cxx", "h", "hh", "hpp", "m", "mm",
    "swift", "kt", "kts", "java", "scala", "cs", "php",
];

/// What `rule.txt` lists.
#[derive(Debug, Default, PartialEq)]
pub struct Baseline {
    pub wry: BTreeSet<String>,
    pub files: BTreeSet<String>,
}

impl Baseline {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut baseline = Self::default();
        let mut section: Option<&str> = None;
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = Some(match name {
                    "wry" => "wry",
                    "files" => "files",
                    other => return Err(format!("rule.txt:{}: unknown section [{other}]", n + 1)),
                });
                continue;
            }
            match section {
                Some("wry") => baseline.wry.insert(line.to_string()),
                Some("files") => baseline.files.insert(line.to_string()),
                _ => return Err(format!("rule.txt:{}: {line:?} is outside a section", n + 1)),
            };
        }
        Ok(baseline)
    }

    /// The list with `gone` taken out, keeping the header comments: what
    /// `--write` saves.
    pub fn without(text: &str, gone: &BTreeSet<String>) -> String {
        let mut out = String::new();
        for line in text.lines() {
            if !gone.contains(line.trim()) {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }
}

/// A file the rule counts: one of [`EXTENSIONS`], or an extensionless
/// script (its first two bytes are `#!`, as `bin/lg` is).
pub fn counts(path: &str, head: impl FnOnce() -> Option<[u8; 2]>) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
        }
        _ => head() == Some(*b"#!"),
    }
}

/// What the rule finds against what is listed.
#[derive(Debug, Default, PartialEq)]
pub struct Findings {
    /// Files in another language that are not listed.
    pub new_files: BTreeSet<String>,
    /// Listed files that are no longer tracked.
    pub gone_files: BTreeSet<String>,
    /// Crates on `wry` that are not listed.
    pub new_wry: BTreeSet<String>,
    /// Listed crates that no longer depend on `wry`.
    pub gone_wry: BTreeSet<String>,
}

impl Findings {
    pub fn compare(baseline: &Baseline, files: &BTreeSet<String>, wry: &BTreeSet<String>) -> Self {
        Self {
            new_files: files.difference(&baseline.files).cloned().collect(),
            gone_files: baseline.files.difference(files).cloned().collect(),
            new_wry: wry.difference(&baseline.wry).cloned().collect(),
            gone_wry: baseline.wry.difference(wry).cloned().collect(),
        }
    }

    fn grew(&self) -> bool {
        !self.new_files.is_empty() || !self.new_wry.is_empty()
    }

    fn shrank(&self) -> bool {
        !self.gone_files.is_empty() || !self.gone_wry.is_empty()
    }
}

pub fn run(root: &Path, write: bool) -> Result<(), String> {
    let list = root.join("xtask/rule.txt");
    let text = std::fs::read_to_string(&list).map_err(|e| format!("{}: {e}", list.display()))?;
    let baseline = Baseline::parse(&text)?;
    let files = tracked_files(root)?;
    let wry = crates_on_wry(root)?;
    let findings = Findings::compare(&baseline, &files, &wry);

    if findings.grew() {
        let mut message = String::from(
            "The all-Rust rule (docs/rfcs/authoring/all-rust.md): nothing new above the operating \
             system in another language, and no new crate on wry.\n",
        );
        for f in &findings.new_files {
            message.push_str(&format!("  new file in another language: {f}\n"));
        }
        for c in &findings.new_wry {
            message.push_str(&format!("  new crate depending on wry: {c}\n"));
        }
        message.push_str(
            "Write it in Rust: a script is an xtask command, a check a Rust test. If it must be \
             an exception, list it in xtask/rule.txt by hand, with the reason in review.",
        );
        return Err(message);
    }
    if findings.shrank() {
        if write {
            let gone: BTreeSet<String> = findings
                .gone_files
                .iter()
                .chain(&findings.gone_wry)
                .cloned()
                .collect();
            std::fs::write(&list, Baseline::without(&text, &gone))
                .map_err(|e| format!("{}: {e}", list.display()))?;
            println!("rule.txt: {} entries dropped, nothing added", gone.len());
        } else {
            let mut message = String::from(
                "Less is not Rust than xtask/rule.txt says (good): drop these with `cargo xtask rule --write`.\n",
            );
            for f in findings.gone_files.iter().chain(&findings.gone_wry) {
                message.push_str(&format!("  gone: {f}\n"));
            }
            return Err(message);
        }
    }
    report(root, &files, &wry);
    Ok(())
}

/// One line of where the rule stands: what is left, by language, in files
/// and lines.
fn report(root: &Path, files: &BTreeSet<String>, wry: &BTreeSet<String>) {
    let mut by_ext: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    for f in files {
        let ext = f
            .rsplit('/')
            .next()
            .and_then(|n| n.rsplit_once('.').map(|(_, e)| e.to_string()))
            .unwrap_or_else(|| "script".into());
        let lines = std::fs::read_to_string(root.join(f)).map_or(0, |t| t.lines().count());
        let entry = by_ext.entry(ext).or_default();
        entry.0 += 1;
        entry.1 += lines;
    }
    let total: (usize, usize) = by_ext.values().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    let parts: Vec<String> = by_ext
        .iter()
        .map(|(ext, (n, lines))| format!("{n} {ext} ({lines} lines)"))
        .collect();
    let on_wry = if wry.is_empty() {
        "no crate".to_string()
    } else {
        wry.iter().cloned().collect::<Vec<_>>().join(", ")
    };
    println!(
        "all-Rust rule: holds. Not Rust yet: {} files, {} lines — {}; on wry: {on_wry}.",
        total.0,
        total.1,
        parts.join(", "),
    );
}

fn tracked_files(root: &Path) -> Result<BTreeSet<String>, String> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-files: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let listed = String::from_utf8_lossy(&out.stdout);
    Ok(listed
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| {
            counts(p, || {
                use std::io::Read;
                let mut head = [0u8; 2];
                std::fs::File::open(root.join(p))
                    .and_then(|mut f| f.read_exact(&mut head))
                    .ok()
                    .map(|()| head)
            })
        })
        .map(str::to_string)
        .collect())
}

/// The workspace's crates that name `wry` among their own dependencies.
fn crates_on_wry(root: &Path) -> Result<BTreeSet<String>, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--no-deps"])
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
    let mut on = BTreeSet::new();
    for package in meta["packages"].as_array().into_iter().flatten() {
        let depends = package["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|d| d["name"] == "wry");
        if depends && let Some(name) = package["name"].as_str() {
            on.insert(name.to_string());
        }
    }
    Ok(on)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_list_reads_its_sections_and_skips_comments() {
        let text = "# what is left\n\n[wry]\nlocalgpt-app\n\n[files]\n# the page\ncrates/app/ui/src/App.tsx\nbin/lg\n";
        let baseline = Baseline::parse(text).unwrap();
        assert_eq!(baseline.wry, set(&["localgpt-app"]));
        assert_eq!(
            baseline.files,
            set(&["crates/app/ui/src/App.tsx", "bin/lg"])
        );
        assert!(
            Baseline::parse("stray\n").is_err(),
            "a path before any section"
        );
        assert!(
            Baseline::parse("[python]\nx.py\n").is_err(),
            "an unknown section"
        );
    }

    #[test]
    fn other_languages_count_and_markup_does_not() {
        let no_head = || None;
        for code in ["a/b.ts", "a/b.tsx", "x.py", "deploy.sh", "w.mjs", "A.JS"] {
            assert!(counts(code, no_head), "{code}");
        }
        for not_code in [
            "index.html",
            "site.css",
            "Cargo.toml",
            "README.md",
            "a.rs",
            "x.json",
            ".gitignore",
        ] {
            assert!(!counts(not_code, no_head), "{not_code}");
        }
        // An extensionless file counts when it is a script.
        assert!(counts("bin/lg", || Some(*b"#!")));
        assert!(!counts("LICENSE", || Some(*b"Ap")));
    }

    #[test]
    fn a_new_file_or_crate_grows_the_list_and_a_gone_one_shrinks_it() {
        let baseline = Baseline {
            wry: set(&["localgpt-app"]),
            files: set(&["a.ts", "b.py"]),
        };
        let same = Findings::compare(&baseline, &set(&["a.ts", "b.py"]), &set(&["localgpt-app"]));
        assert!(!same.grew() && !same.shrank());

        let grown = Findings::compare(
            &baseline,
            &set(&["a.ts", "b.py", "c.sh"]),
            &set(&["localgpt-app", "localgpt-gen"]),
        );
        assert_eq!(grown.new_files, set(&["c.sh"]));
        assert_eq!(grown.new_wry, set(&["localgpt-gen"]));
        assert!(grown.grew());

        let shrunk = Findings::compare(&baseline, &set(&["a.ts"]), &set(&[]));
        assert_eq!(shrunk.gone_files, set(&["b.py"]));
        assert_eq!(shrunk.gone_wry, set(&["localgpt-app"]));
        assert!(shrunk.shrank() && !shrunk.grew());
    }

    #[test]
    fn writing_drops_only_what_is_gone() {
        let text = "# keep me\n[files]\na.ts\nb.py\n";
        let written = Baseline::without(text, &set(&["b.py"]));
        assert_eq!(written, "# keep me\n[files]\na.ts\n");
    }
}
