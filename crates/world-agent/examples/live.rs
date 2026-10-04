//! The live-world authority without an app.
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
//! The commands are [`localgpt_world_agent::headless`] — the same ones
//! `localgpt world` runs — so this is argument parsing and printing only.
//! While an app has the world open, writes go to its API instead: one
//! authority at a time.

use std::io::Read as _;
use std::path::Path;

use localgpt_world_agent::headless::{self, Command};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(command), Some(dir)) = (args.first(), args.get(1)) else {
        eprintln!("usage: live <init|submit|undo|log|verify|history> <dir> ...");
        std::process::exit(2);
    };
    match parse(command, &args[2..]).and_then(|cmd| {
        headless::run(Path::new(dir), cmd).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }) {
        Ok(report) => {
            for line in report.lines {
                println!("{line}");
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn parse(command: &str, rest: &[String]) -> Result<Command, Box<dyn std::error::Error>> {
    Ok(match command {
        "init" => Command::Init {
            git: rest.iter().any(|a| a == "--git"),
        },
        "submit" => {
            let text = match rest.first().map(String::as_str) {
                None | Some("-") => {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
                Some(path) => std::fs::read_to_string(path)?,
            };
            Command::Submit {
                batch: serde_json::from_str(&text)?,
                author: rest.get(1).map_or("cli", String::as_str).to_string(),
                message: None,
            }
        }
        "undo" => Command::Undo {
            author: "cli".into(),
        },
        "log" => Command::Log,
        "verify" => Command::Verify,
        "history" => Command::History {
            rev: rest.first().cloned(),
        },
        other => return Err(format!("unknown command {other}").into()),
    })
}
