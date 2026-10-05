---
title: "Ask Command"
---

# Ask Command

The `ask` command provides single-turn question answering without maintaining a session.

## Usage

```bash
localgpt ask [OPTIONS] <QUESTION>
```

## Options

| Option | Description |
|--------|-------------|
| `-m, --model <MODEL>` | Override the default model |
| `-f, --format <FORMAT>` | Output format: `text` (default) or `json` |
| `--no-memory` | Disable memory context loading |
| `--no-tools` | Disable tool execution |

## Examples

### Basic Question

```bash
localgpt ask "What is the capital of France?"
# Paris is the capital of France.
```

### JSON Output

```bash
localgpt ask --format json "List 3 programming languages"
```

Output:
```json
{
  "response": "1. Python\n2. JavaScript\n3. Rust",
  "model": "gpt-4",
  "tokens": {
    "prompt": 45,
    "completion": 12
  }
}
```

### Using a Specific Model

```bash
localgpt ask -m claude-3-sonnet "Explain quantum computing briefly"
```

### Piping Input

```bash
echo "Summarize this" | localgpt ask -
cat error.log | localgpt ask "What's wrong with this log?"
```

### With Memory Context

By default, `ask` loads relevant memory context:

```bash
# This will include relevant memory about your projects
localgpt ask "What was I working on last week?"
```

Disable memory loading for faster responses:

```bash
localgpt ask --no-memory "What is 2+2?"
```

## Use Cases

The `ask` command is ideal for:

- **Quick questions** - Get an answer without starting a session
- **Scripting** - Use in shell scripts with JSON output
- **Piping** - Process output from other commands
- **One-off tasks** - Translation, summarization, etc.

## Comparison with Chat

| Feature | `ask` | `chat` |
|---------|-------|--------|
| Multi-turn | No | Yes |
| Session persistence | No | Yes |
| Memory context | Optional | Always |
| Tool execution | Optional | Always |
| Interactive | No | Yes |

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Error (API, config, etc.) |
