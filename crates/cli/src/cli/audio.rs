//! `localgpt audio` — the optional audio-analysis models.
//!
//! Nothing is installed by default. `install` downloads a model's pinned
//! files into the shared models directory (the desktop app installs into
//! the same one), and from then on `world mcp` and `audio mcp` offer its tool
//! to an agent.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};
use clap::Subcommand;
use localgpt_audio_models::tools::ToolContext;
use localgpt_audio_models::{self as models, MODELS, Model};
use localgpt_world_agent::mcp::run_mcp_stdio_server;

#[derive(Subcommand)]
pub enum AudioCommands {
    /// Every model: what it does, its download and licence, and whether it is installed
    List,

    /// Download models into the shared models directory, each file checked against its pinned SHA-256
    Install {
        /// Model ids, from `localgpt audio list`
        #[arg(required_unless_present = "all")]
        ids: Vec<String>,
        /// Every model
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },

    /// Delete installed models
    Remove {
        /// Model ids, from `localgpt audio list`
        #[arg(required = true)]
        ids: Vec<String>,
    },

    /// Serve the installed models' tools over stdio MCP; what they make is written into the working directory
    Mcp,

    /// What a sound or a recording contains, from AudioSet's 527 labels (the `tags` model)
    Tags {
        /// The recording (WAV, FLAC, Ogg, MP3, AAC/M4A, ALAC or AIFF)
        file: PathBuf,
        /// Print the answer as JSON, as the `audio_tags` tool gives it
        #[arg(long)]
        json: bool,
    },

    /// The notes played in a recording (the `notes` model): pitch, start, end and velocity
    Notes {
        /// The recording (WAV, FLAC, MP3, Ogg, AAC/M4A, ALAC or AIFF)
        file: PathBuf,
        /// Print the notes as JSON
        #[arg(long)]
        json: bool,
        /// Hear from here, in seconds
        #[arg(long)]
        start: Option<f32>,
        /// Hear up to here, in seconds
        #[arg(long)]
        end: Option<f32>,
        /// How sure the model must be that a note starts, 0 to 1 (lower finds more)
        #[arg(long, default_value_t = 0.5)]
        onset_threshold: f32,
        /// How sure it must be that a note still sounds, 0 to 1 (lower makes longer notes)
        #[arg(long, default_value_t = 0.3)]
        frame_threshold: f32,
        /// The shortest note kept, in milliseconds
        #[arg(long, default_value_t = 127.7)]
        min_note_ms: f32,
    },

    /// Split a song into drums, bass, vocals and other (the `stems` model)
    Stems {
        /// The recording: WAV, FLAC, Ogg Vorbis, MP3, AAC/M4A, ALAC or AIFF
        file: PathBuf,
        /// Where the four WAVs go [default: stems/<file name>/]
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

pub async fn run(command: AudioCommands) -> Result<()> {
    let dir = models::models_dir().ok_or_else(|| {
        anyhow!(
            "no data directory here: set {}",
            models::install::LOCALGPT_AUDIO_MODELS
        )
    })?;
    match command {
        AudioCommands::List => {
            list(&dir);
            Ok(())
        }
        AudioCommands::Install { ids, all } => {
            for model in chosen(&ids, all)? {
                install(&dir, model).await?;
            }
            Ok(())
        }
        AudioCommands::Remove { ids } => {
            for model in chosen(&ids, false)? {
                models::remove(&dir, model)?;
                println!("removed {}", model.id);
            }
            Ok(())
        }
        AudioCommands::Mcp => {
            let here = std::env::current_dir()?;
            let tools = models::tools::installed(&ToolContext {
                models: dir,
                base: here.clone(),
                out: here,
            });
            if tools.is_empty() {
                bail!("no audio model is installed (localgpt audio list)");
            }
            run_mcp_stdio_server(tools, "localgpt-audio").await
        }
        AudioCommands::Tags { file, json } => tags(&dir, file, json).await,
        AudioCommands::Notes {
            file,
            json,
            start,
            end,
            onset_threshold,
            frame_threshold,
            min_note_ms,
        } => {
            let options = models::notes::NoteOptions {
                onset_threshold,
                frame_threshold,
                min_note_ms,
                start: start.unwrap_or(0.0),
                end,
                ..Default::default()
            };
            notes(&dir, &file, &options, json).await
        }
        AudioCommands::Stems { file, out } => stems(&dir, file, out).await,
    }
}

async fn tags(dir: &Path, file: PathBuf, json: bool) -> Result<()> {
    use models::tags::{self, TagOptions};

    let model = &models::catalog::TAGS;
    if !models::installed(dir, model) {
        bail!("the tags model is not installed (localgpt audio install tags)");
    }
    let home = home(dir, model);
    let shown = file.clone();
    let tags = tokio::task::spawn_blocking(move || tags::tag(&home, &file, &TagOptions::default()))
        .await??;
    if json {
        println!("{}", serde_json::to_string_pretty(&tags.to_json())?);
        return Ok(());
    }
    println!("{} ({:.1} s)", shown.display(), tags.seconds);
    for label in &tags.overall {
        println!("  {:<32} {:.3}", label.name, label.score);
    }
    if tags.segments.len() > 1 {
        println!("\nEvery {} s:", tags::WINDOW_SECONDS);
        for segment in &tags.segments {
            let top: Vec<String> = segment
                .top
                .iter()
                .map(|l| format!("{} {:.2}", l.name, l.score))
                .collect();
            println!(
                "  {}–{}  {}",
                clock(segment.start),
                clock(segment.end),
                top.join(" · ")
            );
        }
    }
    Ok(())
}

/// Seconds as m:ss.
fn clock(seconds: f32) -> String {
    let whole = seconds.round() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// Transcribe `file` and print its notes, a line each or as JSON.
async fn notes(
    dir: &Path,
    file: &Path,
    options: &models::notes::NoteOptions,
    json: bool,
) -> Result<()> {
    use models::notes::{Transcriber, note_name, report};
    let model = dir.join(models::catalog::NOTES.id);
    let (file, options) = (file.to_path_buf(), options.clone());
    let heard =
        tokio::task::spawn_blocking(move || Transcriber::load(&model)?.transcribe(&file, &options))
            .await??;
    if json {
        println!("{:#}", report(&heard, None));
        return Ok(());
    }
    for note in &heard.notes {
        println!(
            "{:>8.3} {:>8.3}  {:<4} {:>3}  {:.2}",
            note.start,
            note.end,
            note_name(note.pitch),
            note.pitch,
            note.velocity
        );
    }
    let (from, to) = heard.heard;
    let range = match (
        heard.notes.iter().map(|n| n.pitch).min(),
        heard.notes.iter().map(|n| n.pitch).max(),
    ) {
        (Some(low), Some(high)) => format!(", {} to {}", note_name(low), note_name(high)),
        _ => String::new(),
    };
    eprintln!(
        "{} notes{range}, heard {from:.3}–{to:.3} s (start and end in seconds, name, MIDI pitch, velocity)",
        heard.notes.len()
    );
    Ok(())
}

/// Split `file` with the installed `stems` model and say where the stems went.
async fn stems(dir: &Path, file: PathBuf, out: Option<PathBuf>) -> Result<()> {
    let model = &models::catalog::STEMS;
    if !models::installed(dir, model) {
        bail!("the stems model is not installed: localgpt audio install stems");
    }
    let out = out.unwrap_or_else(|| {
        let name = file.file_stem().unwrap_or("track".as_ref());
        Path::new("stems").join(name)
    });
    let home = home(dir, model);
    let started = std::time::Instant::now();
    let stems = tokio::task::spawn_blocking(move || {
        models::stems::separate(&home, &file, &out)
            .with_context(|| format!("splitting {}", file.display()))
    })
    .await??;
    println!(
        "{:.1} s of audio split in {:.1} s:",
        stems.seconds,
        started.elapsed().as_secs_f32()
    );
    for (name, file) in models::stems::NAMES.iter().zip(&stems.files) {
        println!("  {name:<7} {}", file.display());
    }
    Ok(())
}

/// The models `ids` name, or all of them.
fn chosen(ids: &[String], all: bool) -> Result<Vec<&'static Model>> {
    if all {
        return Ok(MODELS.to_vec());
    }
    ids.iter()
        .map(|id| {
            models::find(id).ok_or_else(|| {
                let known: Vec<_> = MODELS.iter().map(|m| m.id).collect();
                anyhow!("no model `{id}` (models: {})", known.join(", "))
            })
        })
        .collect()
}

fn list(dir: &Path) {
    if MODELS.is_empty() {
        println!("No audio models in this build.");
        return;
    }
    for model in MODELS {
        let state = if models::installed(dir, model) {
            "installed"
        } else {
            "not installed"
        };
        println!(
            "{:<7} {} — {} ({}, {})",
            model.id,
            model.name,
            model.does,
            megabytes(model.bytes()),
            state
        );
        println!("        {}", model.licence);
    }
    println!("\nModels live in {}", dir.display());
}

async fn install(dir: &Path, model: &Model) -> Result<()> {
    if models::installed(dir, model) {
        println!("{} is installed", model.id);
        return Ok(());
    }
    eprintln!(
        "installing {} ({}, {}) — {}",
        model.id,
        model.name,
        megabytes(model.bytes()),
        model.licence
    );
    let mut shown = u64::MAX;
    models::install(dir, model, |p| {
        let percent = p.done * 100 / p.total.max(1);
        if percent != shown {
            shown = percent;
            eprint!("\r  {percent:>3}%  {}", p.file);
            let _ = std::io::stderr().flush();
        }
    })
    .await?;
    eprintln!();
    println!("installed {} in {}", model.id, home(dir, model).display());
    Ok(())
}

fn home(dir: &Path, model: &Model) -> PathBuf {
    dir.join(model.id)
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}
