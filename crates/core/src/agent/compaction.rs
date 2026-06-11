//! Post-compaction context injection.
//!
//! After session compaction, critical startup context (from AGENTS.md or SOUL.md)
//! is re-injected as a system message so the agent retains workspace rules and
//! startup sequences across compaction boundaries.

use std::path::Path;
use tracing::debug;

/// Maximum characters for injected post-compaction context.
const MAX_CONTEXT_CHARS: usize = 3000;

/// Extract named sections from markdown content.
///
/// Matches H2 (`##`) and H3 (`###`) headings case-insensitively.
/// Returns concatenated content of all matched sections.
pub fn extract_sections(content: &str, section_names: &[String]) -> String {
    if section_names.is_empty() {
        return String::new();
    }

    let names_lower: Vec<String> = section_names.iter().map(|s| s.to_lowercase()).collect();

    let mut result = String::new();
    let mut capturing = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Check for heading (## or ###)
        if let Some(heading_text) = trimmed
            .strip_prefix("### ")
            .or_else(|| trimmed.strip_prefix("## "))
        {
            let heading_lower = heading_text.trim().to_lowercase();
            capturing = names_lower.contains(&heading_lower);

            if capturing {
                // Include the heading itself
                result.push_str(line);
                result.push('\n');
            }
            continue;
        }

        if capturing {
            result.push_str(line);
            result.push('\n');
        }
    }

    truncate_context(result)
}

/// Build the post-compaction context message from workspace files.
///
/// Tries `AGENTS.md` first, falls back to `SOUL.md`.
/// Returns `None` if no matching sections found or files missing.
pub fn build_post_compaction_context(workspace: &Path, sections: &[String]) -> Option<String> {
    if sections.is_empty() {
        return None;
    }

    // Try AGENTS.md first, then SOUL.md
    let content = try_read_file(&workspace.join("AGENTS.md"))
        .or_else(|| try_read_file(&workspace.join("SOUL.md")))?;

    let extracted = extract_sections(&content, sections);
    if extracted.trim().is_empty() {
        debug!("Post-compaction: no matching sections found in workspace files");
        return None;
    }

    let now = chrono::Local::now();
    let timestamp = now.format("%A, %B %-d, %Y — %-I:%M %p (%Z)").to_string();
    let utc = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();

    Some(format!(
        "[Post-compaction context refresh]\n\n\
         Session was compacted. Key context from your workspace:\n\n\
         {}\n\
         Current time: {} / {}",
        extracted.trim(),
        timestamp,
        utc,
    ))
}

fn try_read_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn truncate_context(content: String) -> String {
    let mut chars = content.chars();
    let truncated: String = chars.by_ref().take(MAX_CONTEXT_CHARS).collect();
    if chars.next().is_some() {
        format!("{}...\n[truncated]", truncated)
    } else {
        content
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_extract_single_section() {
        let md = "\
# Main Title

Some intro text.

## Session Startup

Read these files first.

## Other Section

Not relevant.
";
        let sections = vec!["Session Startup".to_string()];
        let result = extract_sections(md, &sections);
        assert!(result.contains("## Session Startup"));
        assert!(result.contains("Read these files first."));
        assert!(!result.contains("Not relevant."));
    }

    #[test]
    fn test_extract_multiple_sections() {
        let md = "\
## Session Startup

Boot sequence here.

## Red Lines

Never do bad things.

## Notes

Random notes.
";
        let sections = vec!["Session Startup".to_string(), "Red Lines".to_string()];
        let result = extract_sections(md, &sections);
        assert!(result.contains("Boot sequence here."));
        assert!(result.contains("Never do bad things."));
        assert!(!result.contains("Random notes."));
    }

    #[test]
    fn test_extract_case_insensitive() {
        let md = "## session startup\n\nContent here.\n\n## Other\n\nNo.\n";
        let sections = vec!["Session Startup".to_string()];
        let result = extract_sections(md, &sections);
        assert!(result.contains("Content here."));
    }

    #[test]
    fn test_extract_h3_heading() {
        let md = "### Red Lines\n\nDo not X.\n\n### Other\n\nIgnore.\n";
        let sections = vec!["Red Lines".to_string()];
        let result = extract_sections(md, &sections);
        assert!(result.contains("Do not X."));
        assert!(!result.contains("Ignore."));
    }

    #[test]
    fn test_extract_missing_section() {
        let md = "## Intro\n\nSome content.\n";
        let sections = vec!["Nonexistent Section".to_string()];
        let result = extract_sections(md, &sections);
        assert!(result.trim().is_empty());
    }

    #[test]
    fn test_extract_empty_sections_list() {
        let md = "## Session Startup\n\nContent.\n";
        let result = extract_sections(md, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_truncation() {
        let long_content =
            "## Session Startup\n\n".to_string() + &"x".repeat(4000) + "\n\n## Other\n";
        let sections = vec!["Session Startup".to_string()];
        let result = extract_sections(&long_content, &sections);
        assert!(result.chars().count() <= MAX_CONTEXT_CHARS + 15); // +15 for truncation marker
        assert!(result.contains("[truncated]"));
    }

    #[test]
    fn test_extract_truncation_counts_multibyte_chars() {
        let long_content =
            "## Session Startup\n\n".to_string() + &"✅".repeat(4000) + "\n\n## Other\n";
        let sections = vec!["Session Startup".to_string()];
        let result = extract_sections(&long_content, &sections);

        assert!(result.contains("[truncated]"));
        assert_eq!(result.chars().filter(|&c| c == '✅').count(), 2980);
    }

    #[test]
    fn test_build_context_with_agents_md() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path();

        std::fs::write(
            workspace.join("AGENTS.md"),
            "## Session Startup\n\nRead MEMORY.md first.\n\n## Red Lines\n\nNever delete files.\n",
        )
        .unwrap();

        let sections = vec!["Session Startup".to_string(), "Red Lines".to_string()];
        let result = build_post_compaction_context(workspace, &sections);
        assert!(result.is_some());
        let msg = result.unwrap();
        assert!(msg.contains("[Post-compaction context refresh]"));
        assert!(msg.contains("Read MEMORY.md first."));
        assert!(msg.contains("Never delete files."));
        assert!(msg.contains("Current time:"));
    }

    #[test]
    fn test_build_context_falls_back_to_soul_md() {
        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path();

        // No AGENTS.md, only SOUL.md
        std::fs::write(workspace.join("SOUL.md"), "## Red Lines\n\nBe kind.\n").unwrap();

        let sections = vec!["Red Lines".to_string()];
        let result = build_post_compaction_context(workspace, &sections);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Be kind."));
    }

    #[test]
    fn test_build_context_no_file() {
        let tmp = TempDir::new().unwrap();
        let sections = vec!["Session Startup".to_string()];
        let result = build_post_compaction_context(tmp.path(), &sections);
        assert!(result.is_none());
    }

    #[test]
    fn test_build_context_empty_sections() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "## Startup\nContent\n").unwrap();
        let result = build_post_compaction_context(tmp.path(), &[]);
        assert!(result.is_none());
    }
}
