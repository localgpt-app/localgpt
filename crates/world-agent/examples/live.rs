//! The live-world authority without an app (proof of concept).
//!
//! ```text
//! cargo run -p localgpt-world-agent --example live -- init <dir> [--git]
//! cargo run -p localgpt-world-agent --example live -- submit <dir> <ops.json | -> [author]
//! cargo run -p localgpt-world-agent --example live -- undo <dir>
//! cargo run -p localgpt-world-agent --example live -- log <dir>
//! cargo run -p localgpt-world-agent --example live -- verify <dir>
//! cargo run -p localgpt-world-agent --example live -- history <dir> [rev]
//! ```
//!
//! While an app has the world open (`.live/endpoint.json` exists), writes
//! go to its API instead: one authority at a time.

use std::io::Read as _;
use std::path::Path;

use localgpt_world_agent::live::{self, LiveWorld};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(command), Some(dir)) = (args.first(), args.get(1)) else {
        eprintln!("usage: live <init|submit|undo|log|verify|history> <dir> ...");
        std::process::exit(2);
    };
    let dir = Path::new(dir);
    if let Err(e) = run(command, dir, &args[2..]) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn run(command: &str, dir: &Path, rest: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = dir.join(live::LIVE).join("endpoint.json");
    if matches!(command, "submit" | "undo") && endpoint.exists() {
        let open: serde_json::Value = serde_json::from_slice(&std::fs::read(&endpoint)?)?;
        let url = open["url"].as_str().unwrap_or_default();
        // An app that was killed leaves its endpoint behind: only one that
        // answers holds the world.
        let answers = url
            .trim_start_matches("http://")
            .parse::<std::net::SocketAddr>()
            .ok()
            .and_then(|addr| {
                std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300))
                    .ok()
            })
            .is_some();
        if answers {
            return Err(format!(
                "an app has this world open — send it there: POST {url}/{}",
                if command == "submit" { "ops" } else { "undo" }
            )
            .into());
        }
        std::fs::remove_file(&endpoint)?;
    }
    match command {
        "init" => {
            let world = LiveWorld::open(dir)?;
            if rest.iter().any(|a| a == "--git") {
                live::init_git(dir)?;
            }
            println!(
                "{} is live at revision {} ({} entities){}",
                dir.display(),
                world.revision(),
                world.head().len(),
                if world.is_git() { ", in git" } else { "" }
            );
        }
        "submit" => {
            let text = match rest.first().map(String::as_str) {
                None | Some("-") => {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
                Some(path) => std::fs::read_to_string(path)?,
            };
            let author = rest.get(1).map_or("cli", String::as_str);
            let mut world = LiveWorld::open(dir)?;
            match world.submit(&serde_json::from_str(&text)?, author, None) {
                Ok(done) => println!(
                    "committed revision {}: {}",
                    done.revision,
                    live::describe_ops(&done.ops).join(" · ")
                ),
                Err(refused) => {
                    for error in &refused.errors {
                        eprintln!("rejected: {error}");
                    }
                    std::process::exit(1);
                }
            }
        }
        "undo" => {
            let mut world = LiveWorld::open(dir)?;
            match world.undo("cli") {
                Ok(done) => println!(
                    "committed revision {}: {}",
                    done.revision,
                    done.warnings.join(" ")
                ),
                Err(refused) => return Err(refused.errors.join("; ").into()),
            }
        }
        "log" => {
            let world = LiveWorld::open(dir)?;
            for entry in world.entries() {
                println!(
                    "r{:<3} {:<10} {}",
                    entry.revision,
                    entry.author.name,
                    live::describe_ops(&entry.edit_ops()).join(" · ")
                );
            }
        }
        "verify" => {
            let world = LiveWorld::open(dir)?;
            let problems = world.verify();
            if problems.is_empty() {
                println!(
                    "ok: fold(base, {} entries) == manifest.json, and every asset matches its hash",
                    world.entries().len()
                );
            } else {
                for p in &problems {
                    println!("✗ {p}");
                }
                std::process::exit(1);
            }
        }
        "history" => {
            for s in live::git_history(dir, rest.first().map(String::as_str))? {
                println!(
                    "{} {:<10} {:<40} {} entities",
                    &s.commit[..7],
                    s.author,
                    s.subject,
                    s.manifest.entities.len()
                );
            }
        }
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}
