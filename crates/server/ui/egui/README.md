# Egui Web UI - Proof of Concept

This directory contains the compiled WebAssembly artifacts for the LocalGPT egui web UI.

## Overview

This is a PoC demonstrating code reuse between desktop and web platforms using egui/eframe. The same egui UI code runs on both:
- **Desktop**: Native application (compiled with default features)
- **Web**: WASM application (compiled with `egui-web` feature)

## Building

Run the build script from the repository root:

```bash
./build-egui-web.sh
```

This will:
1. Install wasm32-unknown-unknown target (if needed)
2. Install wasm-bindgen-cli (if needed)
3. Build the server crate for WASM with `egui-web` feature
4. Generate JavaScript bindings
5. Optionally optimize with wasm-opt

## Output Files

- `localgpt_server_bg.wasm` (~1.3 MB) - The compiled WebAssembly module
- `localgpt_server.js` (~76 KB) - JavaScript bindings for the WASM module

## Usage

After building:

1. Start the LocalGPT daemon:
   ```bash
   localgpt daemon start
   ```

2. Open your browser to:
   ```
   http://localhost:31327/egui
   ```

## Architecture

The web UI (`crates/server/src/web/`) implements a simple chat interface that:
- Shows a welcome screen when no messages exist
- Provides a text input for messages
- Displays a Send button  
- Uses egui's dark theme with custom styling
- Returns mock responses (PoC - not connected to backend yet)

## Code Reuse

Key patterns for desktop/web code sharing:
- `WebApp` struct in `web/app.rs` mirrors the desktop `DesktopApp` structure
- `configure_style()` method shared between both implementations
- Same egui/eframe version (v0.33.3) for consistency
- Conditional compilation with `#[cfg(feature = "egui-web")]`

## Next Steps

To make this production-ready:
- Connect to the HTTP API backend (WebSocket or REST)
- Add session management
- Implement streaming responses
- Add error handling
- Improve styling and UX
