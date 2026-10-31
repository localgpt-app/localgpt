# LocalGPT Core

What is left of the assistant's library: the agent loop, its memory, configuration and providers, which `localgpt-gen`'s own chat still runs on, and the stdio MCP server and `Tool` trait under `localgpt world mcp`.

0.4.2 is its last release. It goes in 0.5.0, after the MCP server and the `Tool` trait move to `localgpt-world-agent` ([the-harness-goes.md](../../docs/rfcs/authoring/the-harness-goes.md)).

## What it has

- **Agent**: the conversation loop, sessions and compaction, the safe tool set, and the providers (Anthropic, OpenAI, xAI, Gemini, Vertex AI, GLM, Ollama, OpenAI-compatible endpoints, and the Claude, Gemini and Codex CLIs).
- **Memory**: a Markdown workspace indexed with SQLite FTS5, with vectors from an embeddings API (OpenAI or Gemini) when one is configured.
- **Security**: `POLICY.md` injection with sanitization, the hardcoded suffix, and the protected-files deny list.
- **Configuration**: TOML with environment variable expansion and XDG paths.
- **MCP**: a client for external servers, and the stdio server the shim runs.
