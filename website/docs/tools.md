---
sidebar_position: 13
---

# Agent Tools

LocalGPT's agent has access to 7 built-in tools for interacting with your system.

## Tool Overview

| Tool | Description |
|------|-------------|
| `bash` | Execute shell commands |
| `read_file` | Read file contents |
| `write_file` | Create or overwrite files |
| `edit_file` | Make targeted edits to files |
| `memory_search` | Search the memory index |
| `memory_get` | Read specific content from memory files |
| `web_fetch` | Fetch content from URLs |

## bash

Execute shell commands and return the output. All commands run inside the [shell sandbox](/docs/sandbox) by default.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `command` | string | The command to execute |
| `working_dir` | string | Optional working directory |

**Example:**
```json
{
  "name": "bash",
  "arguments": {
    "command": "ls -la ~/projects",
    "working_dir": "~"
  }
}
```

**Notes:**
- Commands run inside a kernel-enforced sandbox (Landlock + seccomp on Linux, Seatbelt on macOS)
- Sandbox restricts writes to the workspace directory, blocks network access, and denies credential directories
- Timeout after 120 seconds by default (configurable via `sandbox.timeout_secs`)
- Output capped at 1MB (configurable via `sandbox.max_output_bytes`)
- Tilde (`~`) is expanded automatically

## read_file

Read the contents of a file.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `path` | string | Path to the file |
| `offset` | integer | Line number to start from (optional) |
| `limit` | integer | Maximum lines to read (optional) |

**Example:**
```json
{
  "name": "read_file",
  "arguments": {
    "path": "~/projects/app/src/main.rs",
    "offset": 100,
    "limit": 50
  }
}
```

**Notes:**
- Returns line numbers with content
- Handles large files with offset/limit
- Tilde expansion supported

## write_file

Create a new file or overwrite an existing one.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `path` | string | Path to the file |
| `content` | string | Content to write |

**Example:**
```json
{
  "name": "write_file",
  "arguments": {
    "path": "~/projects/app/README.md",
    "content": "# My App\n\nA description of my app."
  }
}
```

**Notes:**
- Creates parent directories if needed
- Overwrites existing files completely
- Use `edit_file` for partial changes
- Writes are restricted to the workspace directory
- [Protected files](/docs/policy#how-it-stays-trustworthy) (`POLICY.md`, `IDENTITY.md`) cannot be written

## edit_file

Make targeted string replacements in a file.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `path` | string | Path to the file |
| `old_string` | string | Text to find |
| `new_string` | string | Text to replace with |

**Example:**
```json
{
  "name": "edit_file",
  "arguments": {
    "path": "~/projects/app/config.toml",
    "old_string": "port = 8080",
    "new_string": "port = 3000"
  }
}
```

**Notes:**
- Finds and replaces exact string matches
- Only replaces first occurrence
- Returns error if string not found
- Preserves file formatting

## memory_search

Search the memory index for relevant content.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `query` | string | Search query |
| `limit` | integer | Maximum results (optional, default: 10) |

**Example:**
```json
{
  "name": "memory_search",
  "arguments": {
    "query": "rust error handling",
    "limit": 5
  }
}
```

**Returns:**
- Matching chunks with file paths
- Relevance scores
- Surrounding context

## memory_get

Read specific content from memory files. Use after `memory_search` to pull only the needed lines.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `path` | string | Path to memory file (MEMORY.md or memory/*.md) |
| `start_line` | integer | Line number to start from (optional) |
| `end_line` | integer | Line number to end at (optional) |

**Example:**
```json
{
  "name": "memory_get",
  "arguments": {
    "path": "memory/2024-01-15.md",
    "start_line": 10,
    "end_line": 25
  }
}
```

**Notes:**
- Safe snippet read from memory files
- Use line ranges to keep context small
- Works with MEMORY.md and daily logs

## web_fetch

Fetch content from a URL.

**Parameters:**
| Name | Type | Description |
|------|------|-------------|
| `url` | string | URL to fetch |

**Example:**
```json
{
  "name": "web_fetch",
  "arguments": {
    "url": "https://api.github.com/repos/owner/repo"
  }
}
```

**Notes:**
- HTTP GET request only
- Response capped at 1MB by default (configurable via `tools.web_fetch_max_bytes`)
- Respects timeouts
- Returns error for non-2xx responses

## Provider Tool Support

All LLM providers in LocalGPT support tool calling:

| Provider | Tool Calling |
|----------|-------------|
| Claude CLI | Native support |
| Anthropic API | Native support |
| OpenAI | Native support |
| Ollama | Supported (v0.1.2+) — requires Ollama models with tool calling capability |
| GLM (Z.AI) | Native support |

## Tool Execution Flow

When the AI wants to use a tool:

```
User: "What files are in my project?"
      │
      ▼
┌─────────────────┐
│  AI decides to  │
│  use bash tool  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Tool Request:  │
│  bash           │
│  "ls ~/project" │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  LocalGPT       │
│  executes cmd   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Tool Result:   │
│  file1.rs       │
│  file2.rs       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  AI formats     │
│  response       │
└────────┬────────┘
         │
         ▼
"Your project contains file1.rs and file2.rs"
```

## Safety Considerations

These measures reduce risk but do not eliminate it. LLMs are probabilistic systems — no prompt or tooling arrangement can guarantee that an AI agent will never take an unintended action.

- **Shell commands** run inside a [kernel-enforced sandbox](/docs/sandbox) — write access limited to workspace, network denied, credentials blocked
- **File tools** (`write_file`, `edit_file`, `read_file`) are path-validated and restricted to the workspace
- **Protected files** — the agent cannot write to `POLICY.md` or `IDENTITY.md` (see [POLICY.md](/docs/policy))
- **No sudo** escalation is performed automatically
- **Web requests** are outbound only with SSRF protection
- **Memory** stays entirely local

Always review agent actions, especially in sensitive environments. The sandbox and protections are a safety net, not a substitute for human oversight.
