# Privacy Policy — LocalGPT Gen MCP Server

**Last updated:** November 1, 2026

## Overview

LocalGPT Gen is a local-first 3D world builder that runs entirely on your machine. This privacy policy covers the `localgpt-gen` MCP server.

## Data Collection

LocalGPT Gen **does not collect, transmit, or store any personal data**. All processing happens locally on your device.

Specifically:

- **No telemetry.** No usage data, crash reports, or analytics are sent anywhere.
- **No model, no key.** Gen runs no language or image model and holds no API key. The agent that drives it runs under its own account, in its own process; what that agent sends to its provider is governed by the agent's privacy policy, not this one.
- **No network calls by default.** The MCP server communicates only with the connected MCP client, over stdio or a localhost HTTP port. No tool calls a service.
- **No accounts required.** No sign-up, login, or authentication with external services.

## Data Storage

All data stays on your local filesystem:

- **Worlds, sessions and exports** are stored under `~/.local/share/localgpt/gen-workspace/` (`LOCALGPT_DATA_DIR` moves it).
- **Settings** (what was opened last) and **logs** are stored under `~/.local/state/localgpt/`.
- **A live `.world` package** keeps its history in the folder you opened.

No data is synced to cloud services.

## Connections You Start

Three things make a network connection, and only when you ask for them:

- `localgpt-gen --world <URL>` fetches that URL to import a world.
- `localgpt-gen --host --relay <URL>` connects to the relay you name, and `--join <host>` to the host you name, for a collaborative session; the relay forwards frames and never holds world state.
- An MCP client connecting over `--mcp-http` does so from your machine to a localhost port.

## Third-Party Services

LocalGPT Gen does not embed any third-party SDKs, trackers, or advertising frameworks.

## Contact

For questions about this privacy policy, open an issue at https://github.com/localgpt-app/localgpt/issues.
