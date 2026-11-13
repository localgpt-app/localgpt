//! What authors get wrong, and whether the authority's answer helps: a
//! corpus of batches replayed through `LiveWorld::submit`, and every example
//! in the package guide sent as written.
//!
//! The log of a world shows what its authors got right. This is the other
//! half: batches as they were sent — a typo, a field in the wrong place, a
//! name nothing has — each with the answer it must get, down to the words
//! of the refusal that let an author fix it. A refusal that stops saying
//! which field was wrong fails here, and so does a mistake that starts to
//! be taken silently.
//!
//! No model runs: a case is a file. `tests/corpus/README.md` says where
//! cases come from and how one an agent really sent is added.

use std::path::{Path, PathBuf};

use localgpt_world_agent::lint;
use localgpt_world_agent::live::{self, LiveWorld};
use serde_json::Value;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A fresh package holding the named starting world.
fn package(world: &str, case: &str) -> (PathBuf, LiveWorld) {
    let manifest = corpus().join("worlds").join(format!("{world}.json"));
    assert!(manifest.is_file(), "{case}: no starting world {world}.json");
    let dir = std::env::temp_dir().join(format!(
        "localgpt-corpus-{}-{}",
        std::process::id(),
        case.replace(['/', '.'], "-")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&manifest, dir.join(live::MANIFEST)).unwrap();
    let world = LiveWorld::open(&dir).unwrap_or_else(|e| panic!("{case}: {e}"));
    (dir, world)
}

fn cases() -> Vec<PathBuf> {
    let mut cases = Vec::new();
    for source in ["seed", "captured"] {
        let Ok(entries) = std::fs::read_dir(corpus().join(source)) else {
            continue;
        };
        cases.extend(
            entries
                .filter_map(|entry| Some(entry.ok()?.path()))
                .filter(|path| path.extension().is_some_and(|ext| ext == "json")),
        );
    }
    cases.sort();
    cases
}

/// What went wrong with one case, if anything did.
fn replay(path: &Path) -> Result<&'static str, String> {
    let case = json(path);
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    for key in ["what", "source", "world", "batch", "expect"] {
        if case.get(key).is_none() {
            return Err(format!("the case has no `{key}`"));
        }
    }
    let source = path
        .parent()
        .and_then(Path::file_name)
        .unwrap()
        .to_string_lossy();
    if case["source"] != *source {
        return Err(format!(
            "it sits in {source}/ and says it is {}",
            case["source"]
        ));
    }
    let (dir, mut world) = package(case["world"].as_str().unwrap_or_default(), &name);
    let result = world.submit(&case["batch"], "corpus", None);
    let strings = |list: &Value| -> Vec<String> {
        list.as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let outcome = match (
        &result,
        case["expect"].get("rejected"),
        case["expect"].get("committed"),
    ) {
        (Err(refused), Some(expected), None) => {
            let expected = strings(expected);
            if refused.errors.len() != expected.len() {
                Err(format!(
                    "expected {} reason(s), got {}: {:#?}",
                    expected.len(),
                    refused.errors.len(),
                    refused.errors
                ))
            } else if let Some((said, wanted)) = refused
                .errors
                .iter()
                .zip(&expected)
                .find(|(said, wanted)| !said.contains(wanted.as_str()))
            {
                Err(format!(
                    "the refusal should say {wanted:?}, and says: {said}"
                ))
            } else if live::rejected(&dir).len() != 1 {
                Err("a refused batch is kept in .live/rejected.jsonl".into())
            } else {
                Ok("refused")
            }
        }
        (Ok(done), None, Some(expected)) => {
            let warnings = strings(&expected["warnings"]);
            let noted = strings(&expected["lint"]);
            let report = lint::lint(world.head_manifest());
            let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
            if done.warnings.len() != warnings.len() {
                Err(format!(
                    "expected {} warning(s), got: {:#?}",
                    warnings.len(),
                    done.warnings
                ))
            } else if let Some(wanted) = warnings
                .iter()
                .find(|wanted| !done.warnings.iter().any(|w| w.contains(wanted.as_str())))
            {
                Err(format!("no warning says {wanted:?}: {:#?}", done.warnings))
            } else if let Some(code) = noted.iter().find(|code| !codes.contains(&code.as_str())) {
                Err(format!("the lint should find {code}, and finds {codes:?}"))
            } else if !world.verify().is_empty() {
                Err(format!(
                    "the package no longer verifies: {:?}",
                    world.verify()
                ))
            } else {
                Ok("committed")
            }
        }
        (Ok(done), Some(_), None) => Err(format!(
            "it should be refused, and it committed: {:?}",
            live::describe_ops(&done.ops)
        )),
        (Err(refused), None, Some(_)) => Err(format!(
            "it should commit, and it was refused: {:#?}",
            refused.errors
        )),
        _ => Err("`expect` holds exactly one of `rejected` and `committed`".into()),
    };
    std::fs::remove_dir_all(&dir).ok();
    outcome
}

#[test]
fn every_batch_in_the_corpus_gets_the_answer_it_should() {
    let cases = cases();
    assert!(cases.len() >= 16, "the corpus is {} cases", cases.len());
    let mut failures = Vec::new();
    let (mut committed, mut refused) = (0, 0);
    for path in &cases {
        match replay(path) {
            Ok("committed") => committed += 1,
            Ok(_) => refused += 1,
            Err(why) => failures.push(format!(
                "{}: {why}",
                path.strip_prefix(corpus()).unwrap().display()
            )),
        }
    }
    println!(
        "corpus: {} cases, {committed} committed, {refused} refused with the reason an author needs",
        cases.len()
    );
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

/// The fenced `json` blocks of the guide, and the batch in its heredoc.
fn guide_examples() -> Vec<(String, Value)> {
    let guide = live::AGENTS_GUIDE;
    let mut examples = Vec::new();
    let heredoc = guide
        .split_once("<<'EOF'\n")
        .and_then(|(_, rest)| rest.split_once("\nEOF"))
        .map(|(batch, _)| batch)
        .expect("the guide's first example is a heredoc");
    examples.push((
        "the submit heredoc".to_string(),
        serde_json::from_str(heredoc).expect("the heredoc is a JSON batch"),
    ));
    for (n, block) in guide.split("```json\n").skip(1).enumerate() {
        let text = block.split("```").next().unwrap_or_default();
        let op: Value = serde_json::from_str(text).unwrap_or_else(|e| {
            panic!(
                "the guide's json example {} is not JSON ({e}):\n{text}",
                n + 1
            )
        });
        examples.push((format!("json example {}", n + 1), Value::Array(vec![op])));
    }
    examples
}

/// The guide is written into every live world, and an agent copies from it.
/// An example the authority refuses teaches the refusal.
#[test]
fn every_example_in_the_package_guide_commits_as_written() {
    let examples = guide_examples();
    assert!(examples.len() >= 5, "found {} examples", examples.len());
    for (what, batch) in examples {
        let (dir, mut world) = package("guide", &what.replace(' ', "-"));
        let done = world
            .submit(&batch, "guide", None)
            .unwrap_or_else(|refused| panic!("{what} is refused: {:#?}", refused.errors));
        assert_eq!(done.warnings, Vec::<String>::new(), "{what}");
        assert!(world.verify().is_empty(), "{what}: {:?}", world.verify());
        // What the guide shows is also what the lint is content with.
        let report = lint::lint(world.head_manifest());
        assert_eq!(report.warnings(), 0, "{what}: {:#?}", report.diagnostics);
        std::fs::remove_dir_all(&dir).ok();
    }
}
