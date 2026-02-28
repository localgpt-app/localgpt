---
sidebar_position: 3
---

# Quick Start

This guide will get you chatting with LocalGPT in just a few minutes.

## 1. Configure a Model Provider

Choose one of the following options:

### Option A: Claude CLI (Recommended)

If you have the [Claude CLI](https://claude.ai/code) installed and authenticated:

```bash
# No additional setup needed! LocalGPT uses your Claude CLI auth.
# Just start chatting with:
localgpt chat
```

### Option B: API Keys

```bash
# For OpenAI
export OPENAI_API_KEY="sk-..."

# For Anthropic API
export ANTHROPIC_API_KEY="sk-ant-..."

# For Gemini
export GEMINI_API_KEY="..."

# For GLM (Z.AI)
export GLM_API_KEY="..."
```

### Option C: Local Models (Ollama)

```bash
# Install Ollama from https://ollama.ai, then:
ollama pull llama3
localgpt config set agent.default_model "llama3"
```

## 2. Start an Interactive Chat

```bash
localgpt chat
```

You'll see a prompt where you can type messages:

```
LocalGPT Chat (type /help for commands, /quit to exit)
> Hello! What can you help me with?

I can help you with a variety of tasks:
- Answer questions and explain concepts
- Write and review code
- Execute shell commands
- Search and manage your memory
- And much more!

>
```

## 3. Basic Chat Commands

While in chat, use these commands:

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/quit` | Exit the chat |
| `/memory <query>` | Search your memory |
| `/save` | Force save important context |
| `/compact` | Manually compact the session |
| `/status` | Show session status |
| `/clear` | Clear the screen |

## 4. Ask a Single Question

For quick one-off questions:

```bash
localgpt ask "How do I create a new git branch?"
```

## 5. Search Your Memory

As you chat, important information is automatically saved. Search it later:

```bash
# Search memory from CLI
localgpt memory search "git commands"

# Or from within chat
> /memory git commands
```

## 6. Start the Daemon

For the HTTP API and heartbeat functionality:

```bash
# Start in foreground
localgpt daemon start

# Check status
localgpt daemon status

# Stop the daemon
localgpt daemon stop
```

## Example Session

```bash
$ localgpt chat

LocalGPT Chat (type /help for commands, /quit to exit)

> Can you help me write a Python script that reads a CSV file?

Sure! Here's a simple Python script using the csv module:

```python
import csv

with open('data.csv', 'r') as file:
    reader = csv.DictReader(file)
    for row in reader:
        print(row)
```

> Save this to a file called read_csv.py

I'll create that file for you.
[Tool: write_file] Creating read_csv.py...

Done! I've created `read_csv.py` with the CSV reading code.

> /memory csv
Found 1 result for "csv":
- [2026-02-14] Discussed CSV file reading in Python

> /quit
Goodbye!
```

## Next Steps

- Learn about [CLI Commands](/docs/cli-commands)
- Understand the [Memory System](/docs/memory-system)
- Set up [Skills](/docs/skills) for specialized tasks
- Configure [Heartbeat](/docs/heartbeat) for autonomous tasks
