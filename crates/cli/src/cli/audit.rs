//! CLI subcommand: `localgpt audit`
//!
//! Inspects compaction audit entries — shows recent compaction events,
//! verifies hash chain integrity, and displays aggregate statistics.

use anyhow::Result;
use clap::{Args, Subcommand};

use localgpt_core::config::Config;
use localgpt_core::security;
use localgpt_core::text::prefix_chars;

#[derive(Args)]
pub struct AuditArgs {
    #[command(subcommand)]
    pub command: AuditCommands,
}

#[derive(Subcommand)]
pub enum AuditCommands {
    /// Show recent compaction events
    Show {
        /// Maximum number of entries to display
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Verify audit log hash chain integrity
    Verify,

    /// Show compaction statistics
    Stats,
}

fn hash_preview(hash: &str) -> String {
    prefix_chars(hash, 16)
}

pub async fn run(args: AuditArgs) -> Result<()> {
    match args.command {
        AuditCommands::Show { limit, json } => show_compaction(limit, json).await,
        AuditCommands::Verify => verify_chain().await,
        AuditCommands::Stats => show_stats().await,
    }
}

async fn show_compaction(limit: usize, json_output: bool) -> Result<()> {
    let config = Config::load()?;
    let state_dir = &config.paths.state_dir;

    let entries = security::read_compaction_entries(state_dir)?;

    if entries.is_empty() {
        println!("No compaction events recorded.");
        return Ok(());
    }

    // Take the last N entries
    let start = entries.len().saturating_sub(limit);
    let recent = &entries[start..];

    if json_output {
        let json_entries: Vec<_> = recent
            .iter()
            .map(|(entry, detail)| {
                serde_json::json!({
                    "ts": entry.ts,
                    "source": entry.source,
                    "prev_entry_sha256": hash_preview(&entry.prev_entry_sha256),
                    "detail": detail,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_entries)?);
    } else {
        println!(
            "Compaction Audit Log ({} of {} entries):",
            recent.len(),
            entries.len()
        );
        println!();

        for (entry, detail) in recent {
            println!("  {} ({})", entry.ts, entry.source);

            if let Some(d) = detail {
                let msgs_removed = d.messages_before.saturating_sub(d.messages_after);
                let tokens_saved = d.tokens_before.saturating_sub(d.tokens_after);
                println!(
                    "    messages: {} -> {} (-{})",
                    d.messages_before, d.messages_after, msgs_removed
                );
                println!(
                    "    tokens:   {} -> {} (-{})",
                    d.tokens_before, d.tokens_after, tokens_saved
                );
                println!("    strategy: {}", d.strategy);
                if !d.injected_sections.is_empty() {
                    println!("    injected: {}", d.injected_sections.join(", "));
                }
                println!(
                    "    chain:    {}...",
                    hash_preview(&entry.prev_entry_sha256)
                );
            } else {
                println!("    (detail not available)");
            }
            println!();
        }
    }

    Ok(())
}

async fn verify_chain() -> Result<()> {
    let config = Config::load()?;
    let state_dir = &config.paths.state_dir;

    let all_entries = security::read_audit_log(state_dir)?;
    let broken = security::verify_audit_chain(state_dir)?;

    let compaction_count = all_entries
        .iter()
        .filter(|e| e.action == security::AuditAction::Compaction)
        .count();

    println!("Audit Log Verification:");
    println!("  Total entries:      {}", all_entries.len());
    println!("  Compaction entries:  {}", compaction_count);
    println!();

    if broken.is_empty() {
        println!("  Chain integrity: INTACT");
        println!("  All {} entries verified.", all_entries.len());
    } else {
        println!("  Chain integrity: BROKEN at {} position(s)", broken.len());
        for &idx in &broken {
            if idx < all_entries.len() {
                let e = &all_entries[idx];
                println!(
                    "    [{}] {} {:?} (source: {})",
                    idx, e.ts, e.action, e.source
                );
            } else {
                println!("    [{}] (corrupted entry)", idx);
            }
        }
    }

    Ok(())
}

async fn show_stats() -> Result<()> {
    let config = Config::load()?;
    let state_dir = &config.paths.state_dir;

    let stats = security::compaction_stats(state_dir)?;

    println!("Compaction Statistics:");
    println!("  Total compaction events:  {}", stats.total_events);
    println!(
        "  Total messages compacted: {}",
        stats.total_messages_compacted
    );
    println!("  Total tokens saved:       {}", stats.total_tokens_saved);
    match stats.last_compaction {
        Some(ref ts) => println!("  Last compaction:          {}", ts),
        None => println!("  Last compaction:          (none)"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_preview_preserves_short_hash() {
        assert_eq!(hash_preview("abc123"), "abc123");
    }

    #[test]
    fn hash_preview_truncates_ascii_hash() {
        assert_eq!(hash_preview("0123456789abcdefextra"), "0123456789abcdef");
    }

    #[test]
    fn hash_preview_truncates_multibyte_without_panicking() {
        assert_eq!(
            hash_preview("✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅"),
            "✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅"
        );
    }
}
