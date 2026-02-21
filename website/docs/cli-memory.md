---
sidebar_position: 8
---

# Memory Command

The `memory` command provides operations for managing and searching the memory system.

## Usage

```bash
localgpt memory <SUBCOMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| `search <QUERY>` | Search memory for matching content |
| `reindex` | Rebuild the search index |
| `stats` | Display memory statistics |
| `recent` | Show recent memory entries |

## Searching Memory

```bash
localgpt memory search "rust async"
```

Output:
```
Search results for "rust async" (3 matches):

[2024-01-15] memory/2024-01-15.md (score: 0.95)
  ...discussed async/await patterns in Rust, specifically
  using tokio for concurrent HTTP requests...

[2024-01-10] memory/2024-01-10.md (score: 0.72)
  ...Rust async runtime comparison: tokio vs async-std...

[2024-01-08] MEMORY.md (score: 0.68)
  ## Rust Async Notes
  - Use `#[tokio::main]` for async main functions...
```

### Search Options

| Option | Description |
|--------|-------------|
| `-n, --limit <N>` | Maximum results (default: 10) |
| `--score` | Show relevance scores |
| `--context <N>` | Lines of context around matches |

## Reindexing

Rebuild the search index from all markdown files:

```bash
# Normal reindex (skips unchanged files)
localgpt memory reindex

# Force full reindex
localgpt memory reindex --force
```

Output:
```
Reindexing memory...
  Scanning: ~/.local/share/localgpt/workspace
  Found: 45 markdown files
  Indexed: 42 files (3 unchanged)
  Chunks: 156 total
Done in 0.8s
```

## Memory Statistics

```bash
localgpt memory stats
```

Output:
```
Memory Statistics
─────────────────
Location: ~/.local/share/localgpt/workspace

Files:
  MEMORY.md:     2.4 KB (42 lines)
  HEARTBEAT.md:  0.3 KB (8 lines)
  Daily logs:    45 files (128 KB total)

Index:
  Chunks: 156
  Last indexed: 2024-01-15 10:30:00
  Database size: 245 KB

Oldest entry: 2024-01-01
Newest entry: 2024-01-15
```

## Recent Entries

View recent memory entries:

```bash
# Show last 5 entries (default)
localgpt memory recent

# Show last 20 entries
localgpt memory recent --count 20
```

Output:
```
Recent Memory Entries
─────────────────────
[2024-01-15 14:30] Discussed Rust error handling patterns
[2024-01-15 10:15] Created new project structure for web API
[2024-01-14 16:45] Reviewed database migration scripts
[2024-01-14 11:00] Implemented user authentication
[2024-01-13 15:30] Set up CI/CD pipeline with GitHub Actions
```

## Memory File Structure

The memory system uses three types of files:

```
~/.local/share/localgpt/workspace/
├── MEMORY.md          # Curated long-term knowledge
├── HEARTBEAT.md       # Pending autonomous tasks
└── memory/
    ├── 2024-01-15.md  # Today's log
    ├── 2024-01-14.md  # Yesterday's log
    └── ...            # Historical logs
```

### MEMORY.md

Curated, long-term knowledge that you want the AI to always remember:

```markdown
# Memory

## Project Context
- Working on LocalGPT, a Rust-based AI assistant
- Using SQLite for the search index

## Preferences
- Prefer concise code examples
- Use markdown for documentation
```

### Daily Logs

Automatic conversation logs organized by date:

```markdown
# 2024-01-15

## 14:30 - Rust Error Handling
Discussed using `thiserror` for custom error types...

## 10:15 - Project Structure
Created new API project with the following layout...
```
