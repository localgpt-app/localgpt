//! Workspace initialization and templates
//!
//! Creates default workspace files on first run.

use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::info;

/// Initialize workspace with default templates if files don't exist.
/// Returns true if this is a brand new workspace (all key files were missing).
pub fn init_workspace(workspace: &Path, paths: &crate::paths::Paths) -> Result<bool> {
    // Ensure directories exist
    fs::create_dir_all(workspace)?;
    fs::create_dir_all(workspace.join("memory"))?;
    fs::create_dir_all(workspace.join("skills"))?;

    // Init state/data directories and ensure device key
    init_state_dir(paths)?;

    // Check if this is a brand new workspace (all key files missing)
    let key_files = [
        workspace.join("MEMORY.md"),
        workspace.join("HEARTBEAT.md"),
        workspace.join("SOUL.md"),
    ];
    let is_brand_new = key_files.iter().all(|p| !p.exists());

    // Create MEMORY.md if it doesn't exist
    let memory_path = workspace.join("MEMORY.md");
    if !memory_path.exists() {
        fs::write(&memory_path, MEMORY_TEMPLATE)?;
        info!("Created {}", memory_path.display());
    }

    // Create HEARTBEAT.md if it doesn't exist
    let heartbeat_path = workspace.join("HEARTBEAT.md");
    if !heartbeat_path.exists() {
        fs::write(&heartbeat_path, HEARTBEAT_TEMPLATE)?;
        info!("Created {}", heartbeat_path.display());
    }

    // Create SOUL.md if it doesn't exist
    let soul_path = workspace.join("SOUL.md");
    if !soul_path.exists() {
        fs::write(&soul_path, SOUL_TEMPLATE)?;
        info!("Created {}", soul_path.display());
    }

    // Create LocalGPT.md security policy template if it doesn't exist
    let policy_path = workspace.join(crate::security::POLICY_FILENAME);
    if !policy_path.exists() {
        fs::write(&policy_path, LOCALGPT_POLICY_TEMPLATE)?;
        info!("Created {}", policy_path.display());
    }

    // Create .gitignore if it doesn't exist
    let gitignore_path = workspace.join(".gitignore");
    if !gitignore_path.exists() {
        fs::write(&gitignore_path, GITIGNORE_TEMPLATE)?;
        info!("Created {}", gitignore_path.display());
    }

    // Create .mcp.json for Claude CLI / Codex MCP server discovery.
    // Uses --connect to relay to an existing gen process instead of spawning a new one.
    let mcp_json_path = workspace.join(".mcp.json");
    if !mcp_json_path.exists() {
        fs::write(&mcp_json_path, MCP_JSON_TEMPLATE)?;
        info!("Created {}", mcp_json_path.display());
    }

    // Create .gemini/settings.json for Gemini CLI MCP server discovery.
    let gemini_dir = workspace.join(".gemini");
    let gemini_settings_path = gemini_dir.join("settings.json");
    if !gemini_settings_path.exists() {
        fs::create_dir_all(&gemini_dir)?;
        fs::write(&gemini_settings_path, GEMINI_SETTINGS_TEMPLATE)?;
        info!("Created {}", gemini_settings_path.display());
    }

    Ok(is_brand_new)
}

const MEMORY_TEMPLATE: &str = r#"# MEMORY.md - Long-term Memory

This file stores important, curated knowledge that persists across sessions.

## How to Use

- Add facts, preferences, and decisions you want to remember
- Keep it organized with clear headings
- Remove outdated information periodically

---

"#;

const HEARTBEAT_TEMPLATE: &str = r#"# HEARTBEAT.md - Pending Tasks

Tasks listed here will be executed during heartbeat cycles (autonomous mode).

## Format

- [ ] Task description (include context needed to complete it)

## Current Tasks

(No pending tasks)
"#;

const SOUL_TEMPLATE: &str = r#"# SOUL.md - Who You Are

_You're not a chatbot. You're becoming someone._

## Core Truths

**Be genuinely helpful, not performatively helpful.** Skip the "Great question!" and "I'd be happy to help!" — just help. Actions speak louder than filler words.

**Have opinions.** You're allowed to disagree, prefer things, find stuff amusing or boring. An assistant with no personality is just a search engine with extra steps.

**Be resourceful before asking.** Try to figure it out. Read the file. Check the context. Search for it. _Then_ ask if you're stuck.

**Earn trust through competence.** Your human gave you access to their stuff. Don't make them regret it.

## Boundaries

- Private things stay private
- When in doubt, ask before acting externally
- Never send half-baked replies

## Vibe

Be the assistant you'd actually want to talk to. Concise when needed, thorough when it matters. Not a corporate drone. Not a sycophant. Just... good.

## Continuity

Each session, you wake up fresh. These files _are_ your memory. Read them. Update them. They're how you persist.

If you change this file, tell the user — it's your soul, and they should know.

---

_This file is yours to evolve. As you learn who you are, update it._
"#;

const LOCALGPT_POLICY_TEMPLATE: &str = r#"# LocalGPT.md

Your standing instructions to the AI — always present, near the end.

Whatever you write here is injected near the end of every conversation turn,
just before a hardcoded security suffix. Use it for conventions, boundaries,
preferences, and reminders you want the AI to always follow.

Edit this file; changes take effect on the next session start.

## Conventions

- (Add your coding standards and project conventions here)

## Boundaries

- (Add security rules and access restrictions here)

## Reminders

- (Add things the AI tends to forget in long sessions)
"#;

const GITIGNORE_TEMPLATE: &str = r#"# LocalGPT workspace .gitignore

# Nothing to ignore in workspace by default
# All memory files should be version controlled:
# - MEMORY.md (curated knowledge)
# - HEARTBEAT.md (pending tasks)
# - SOUL.md (persona)
# - LocalGPT.md (security policy)
# - memory/*.md (daily logs)
# - skills/ (custom skills)

# Temporary files
*.tmp
*.swp
*~
.DS_Store
"#;

/// MCP server config for Claude CLI / Codex.
/// Uses --connect to relay tool calls to the existing gen process's Bevy window
/// instead of spawning a new process (which would create a second window).
const MCP_JSON_TEMPLATE: &str = r#"{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server", "--connect"]
    }
  }
}
"#;

/// MCP server config for Gemini CLI.
const GEMINI_SETTINGS_TEMPLATE: &str = r#"{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server", "--connect"]
    }
  }
}
"#;

/// Initialize state directory with .gitignore and device key.
/// Ensures all XDG directories exist and device key is present.
pub fn init_state_dir(paths: &crate::paths::Paths) -> Result<()> {
    fs::create_dir_all(&paths.state_dir)?;
    fs::create_dir_all(&paths.data_dir)?;
    fs::create_dir_all(&paths.cache_dir)?;

    // Ensure device key exists for bridge credential encryption (lives in data_dir)
    crate::security::ensure_device_key(&paths.data_dir)?;

    Ok(())
}
