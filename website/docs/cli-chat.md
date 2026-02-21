---
sidebar_position: 5
---

# Chat Command

The `chat` command starts an interactive multi-turn conversation session.

## Usage

```bash
localgpt chat [OPTIONS]
```

## Options

| Option | Description |
|--------|-------------|
| `--session <ID>` | Resume an existing session by ID |
| `--resume` | Resume the most recent session |
| `-m, --model <MODEL>` | Override the default model |
| `--no-memory` | Disable memory context loading |

## Features

### Session Persistence

Sessions are automatically saved and can be resumed later:

```bash
# Start a new session (auto-generates ID)
localgpt chat

# Resume a previous session
localgpt chat --session abc123
```

### Automatic Context Management

The chat automatically manages context window limits:

- Monitors token usage as conversation grows
- Triggers compaction when approaching limits
- Prompts the AI to save important context before compacting
- Summarizes conversation history to preserve key information

### Memory Integration

Chat sessions have full access to the memory system:

- Memory context is loaded at session start
- Use `/memory <query>` to search during chat
- Important information is automatically saved to daily logs

## Chat Commands

While in chat, these slash commands are available:

| Command | Description |
|---------|-------------|
| `/help`, `/h`, `/?` | Show available commands |
| `/quit`, `/exit`, `/q` | Exit the chat |
| `/new` | Start a fresh session |
| `/sessions` | List saved sessions |
| `/resume <id>` | Resume a saved session by ID |
| `/search <query>` | Search across sessions |
| `/model [name]` | Show or switch the current model |
| `/models` | List available model prefixes |
| `/context` | Show context window usage |
| `/status` | Show session info (tokens, turns) |
| `/memory <query>` | Search memory files |
| `/reindex` | Rebuild memory search index |
| `/save` | Force save current session |
| `/compact` | Compact session history |
| `/export [file]` | Export session as markdown |
| `/attach <file>` | Attach a file to the conversation |
| `/attachments` | List pending attachments |
| `/clear` | Clear the terminal screen |
| `/skills` | List available skills |

## Example Session

```bash
$ localgpt chat

LocalGPT Chat (type /help for commands, /quit to exit)

> I'm working on a Rust project that needs async HTTP requests

I can help with that! For async HTTP in Rust, you'll typically want to use:

1. **reqwest** - The most popular HTTP client, with excellent async support
2. **tokio** - The async runtime

Add these to your Cargo.toml:
```toml
[dependencies]
reqwest = { version = "0.11", features = ["json"] }
tokio = { version = "1", features = ["full"] }
```

> /status
Session: abc123
Turns: 2
Tokens: ~450 / 128000
Memory: Loaded (3 files)

> Can you show me an example?

Here's a simple async HTTP GET request:
```rust
use reqwest;

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let response = reqwest::get("https://api.example.com/data")
        .await?
        .json::<serde_json::Value>()
        .await?;

    println!("{:#?}", response);
    Ok(())
}
```

> /memory async rust
Found 2 results for "async rust":
- [2024-01-10] Discussed async patterns in Rust
- [2024-01-08] Tokio runtime configuration

> /quit
Goodbye!
```

## Session Files

Sessions are stored as JSONL files in:

```text
~/.local/state/localgpt/agents/{agent-id}/sessions/{session-id}.jsonl
```

Each line contains a message in the conversation.
