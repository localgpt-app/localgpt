# Migrating from OpenClaw

LocalGPT is a spiritual successor to OpenClaw, built from scratch in Rust. It uses the same workspace file formats and conventions, so migrating your data is straightforward.

LocalGPT does **not** auto-migrate your OpenClaw data. Follow the steps below to bring your existing workspace, config, and sessions into LocalGPT.

## Config

OpenClaw uses `~/.openclaw/config.json5`. LocalGPT uses `~/.config/localgpt/config.toml`.

Create your LocalGPT config manually. Here is a mapping of the most common settings:

| OpenClaw (`config.json5`) | LocalGPT (`config.toml`) |
|---|---|
| `agents.defaults.model` | `agent.default_model` |
| `agents.defaults.workspace` | `memory.workspace` |
| `agents.defaults.contextWindow` | `agent.context_window` |
| `models.openai.apiKey` | `providers.openai.api_key` |
| `models.anthropic.apiKey` | `providers.anthropic.api_key` |

Example LocalGPT config:

```toml
[agent]
default_model = "claude-cli/opus"
context_window = 128000

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"

[providers.claude_cli]
command = "claude"
```

Run `localgpt config show` to verify your configuration after creating the file.

## Workspace files

OpenClaw workspace files are plain Markdown and fully compatible. Copy them directly:

```bash
cp -r ~/.openclaw/workspace/* ~/.local/share/localgpt/workspace/
```

This includes:

| File | Purpose |
|---|---|
| `MEMORY.md` | Long-term curated knowledge |
| `HEARTBEAT.md` | Pending autonomous tasks |
| `SOUL.md` | Persona and tone guidance |
| `USER.md` | User profile |
| `IDENTITY.md` | Agent identity |
| `TOOLS.md` | Tool notes |
| `AGENTS.md` | Operating instructions |
| `memory/*.md` | Daily logs |
| `knowledge/**/*.md` | Knowledge repository |
| `skills/*/SKILL.md` | Custom skills |

LocalGPT will rebuild the memory index automatically on first run.

## Session data

Session transcripts and metadata can be copied as-is:

```bash
cp -r ~/.openclaw/agents ~/.local/share/localgpt/agents
```

This preserves your conversation history, session IDs, and CLI session mappings.

## Key differences

LocalGPT takes a different approach from OpenClaw in several areas:

- **Bridge-based integrations** &mdash; Telegram, Discord, and WhatsApp are supported via standalone bridge binaries in the `bridges/` directory, connected to the daemon via secure IPC (rather than built-in channels)
- **No plugin/extension system** &mdash; LocalGPT uses a simpler skills-based approach
- **No gateway routing** &mdash; single-agent, local-first design with bridge daemons instead of a multi-channel gateway
- **Embedded web UI** &mdash; browser-based chat interface served directly from the binary, plus an optional desktop GUI (egui)
- **No subagent spawning** &mdash; single "main" agent

Everything else &mdash; memory, heartbeat, skills, session management &mdash; works the same way.

## Cleanup

Once you have verified that LocalGPT is working correctly with your migrated data, you can optionally remove the OpenClaw directory:

```bash
rm -rf ~/.openclaw
```

This will suppress the startup notice about detected OpenClaw data.
