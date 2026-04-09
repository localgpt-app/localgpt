# Privacy Policy — LocalGPT Gen MCP Server

**Last updated:** April 9, 2026

## Overview

LocalGPT Gen is a local-first 3D world builder that runs entirely on your machine. This privacy policy covers the `localgpt-gen` MCP server.

## Data Collection

LocalGPT Gen **does not collect, transmit, or store any personal data**. All processing happens locally on your device.

Specifically:

- **No telemetry.** No usage data, crash reports, or analytics are sent anywhere.
- **No network calls by default.** The MCP server communicates only with the connected MCP client via stdio or local HTTP.
- **No accounts required.** No sign-up, login, or authentication with external services.

## Data Storage

All data stays on your local filesystem:

- **Workspace files** (scenes, memory, skills) are stored in `~/.local/share/localgpt/workspace/` or a directory you configure.
- **Cache** (embedding models, search indexes) is stored in `~/.cache/localgpt/`.
- **Session logs** are stored in `~/.local/state/localgpt/`.

No data is synced to cloud services.

## Optional External Connections

If **you** configure an external LLM provider (OpenAI, Anthropic, Ollama, etc.), your prompts and scene descriptions are sent to that provider. This is initiated by your configuration and governed by the provider's own privacy policy. LocalGPT Gen itself does not require any external API.

If you use the `web_fetch` or `web_search` tools, those requests go to the URLs or search provider you specify.

## Third-Party Services

LocalGPT Gen does not embed any third-party SDKs, trackers, or advertising frameworks.

## Contact

For questions about this privacy policy, open an issue at https://github.com/localgpt-app/localgpt/issues.
