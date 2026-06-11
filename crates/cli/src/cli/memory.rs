use anyhow::Result;
use clap::{Args, Subcommand};

use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;
use localgpt_core::text::single_line_prefix_with_ellipsis;

#[derive(Args)]
pub struct MemoryArgs {
    #[command(subcommand)]
    pub command: MemoryCommands,
}

#[derive(Subcommand)]
pub enum MemoryCommands {
    /// Search memory
    Search {
        /// Search query
        query: String,

        /// Maximum number of results
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Reindex all memory files
    Reindex {
        /// Force full reindex (ignore file hashes)
        #[arg(short, long)]
        force: bool,
    },

    /// Show memory statistics
    Stats,

    /// List recent memory entries
    Recent {
        /// Number of entries to show
        #[arg(short, long, default_value = "10")]
        count: usize,
    },
}

pub async fn run(args: MemoryArgs, agent_id: &str) -> Result<()> {
    let config = Config::load()?;
    let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), agent_id)?;

    match args.command {
        MemoryCommands::Search { query, limit } => search_memory(&memory, &query, limit).await,
        MemoryCommands::Reindex { force } => reindex_memory(&memory, force).await,
        MemoryCommands::Stats => show_stats(&memory).await,
        MemoryCommands::Recent { count } => show_recent(&memory, count).await,
    }
}

async fn search_memory(memory: &MemoryManager, query: &str, limit: usize) -> Result<()> {
    let results = memory.search(query, limit)?;

    if results.is_empty() {
        println!("No results found for '{}'", query);
        return Ok(());
    }

    println!("Found {} results for '{}':\n", results.len(), query);

    for (i, result) in results.iter().enumerate() {
        println!(
            "{}. {} (lines {}-{})",
            i + 1,
            result.file,
            result.line_start,
            result.line_end
        );
        println!("   Score: {:.3}", result.score);

        println!("   {}\n", memory_result_preview(&result.content));
    }

    Ok(())
}

fn memory_result_preview(content: &str) -> String {
    single_line_prefix_with_ellipsis(content, 200)
}

async fn reindex_memory(memory: &MemoryManager, force: bool) -> Result<()> {
    println!(
        "Reindexing memory files{}...",
        if force { " (full)" } else { "" }
    );

    let stats = memory.reindex(force)?;

    println!("Reindex complete:");
    println!("  Files processed: {}", stats.files_processed);
    println!("  Files updated: {}", stats.files_updated);
    println!("  Chunks indexed: {}", stats.chunks_indexed);
    println!("  Duration: {:?}", stats.duration);

    // Generate embeddings if provider is configured
    if memory.has_embeddings() {
        println!("\nGenerating embeddings...");
        let (processed, embedded) = memory.generate_embeddings(50).await?;
        if processed > 0 {
            println!("  Chunks processed: {}", processed);
            println!("  Embeddings generated: {}", embedded);
        } else {
            println!("  All chunks already have embeddings");
        }
    }

    Ok(())
}

async fn show_stats(memory: &MemoryManager) -> Result<()> {
    let stats = memory.stats()?;

    println!("Memory Statistics");
    println!("-----------------");
    println!("Workspace: {}", stats.workspace);
    println!("Total files: {}", stats.total_files);
    println!("Total chunks: {}", stats.total_chunks);
    println!("Index size: {} KB", stats.index_size_kb);
    println!("\nFiles:");
    for file in &stats.files {
        println!(
            "  {} ({} chunks, {} lines)",
            file.name, file.chunks, file.lines
        );
    }

    Ok(())
}

async fn show_recent(memory: &MemoryManager, count: usize) -> Result<()> {
    let entries = memory.recent_entries(count)?;

    if entries.is_empty() {
        println!("No recent memory entries found");
        return Ok(());
    }

    println!("Recent memory entries:\n");

    for entry in entries {
        println!("[{}] {}", entry.timestamp, entry.file);
        println!("  {}\n", entry.preview);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_result_preview_preserves_short_multibyte_text() {
        assert_eq!(memory_result_preview(&"✅".repeat(100)), "✅".repeat(100));
    }

    #[test]
    fn memory_result_preview_truncates_by_characters() {
        let preview = memory_result_preview(&"✅".repeat(201));

        assert_eq!(preview.chars().filter(|&c| c == '✅').count(), 200);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn memory_result_preview_flattens_newlines() {
        assert_eq!(memory_result_preview("one\ntwo"), "one two");
    }
}
