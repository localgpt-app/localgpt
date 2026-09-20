# LocalGPT Server

A background daemon that provides the core LocalGPT services over HTTP and via local client bridges. This crate implements the HTTP REST API and the embedded Web UI.

## Features

- **HTTP REST API**: Unified interface for LocalGPT reasoning and system management.
- **Embedded Web UI**: Self-contained web application for interacting with the assistant from any browser.
- **Secure Bridge IPC**: Facilitates encrypted communication with standalone bridge daemons.
- **Websocket Support**: Real-time interaction and status updates.
