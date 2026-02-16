# Egui Web UI PoC

This document describes the Proof of Concept (PoC) for using Egui as the internal web UI for LocalGPT, reusing code from the desktop implementation.

## Overview

LocalGPT currently has two separate UI implementations:
- **Desktop UI**: Native Rust application using `eframe`/`egui` (in `crates/cli/src/desktop/`)
- **Web UI**: HTML/CSS/JavaScript served by HTTP server (in `crates/server/ui/`)

This PoC demonstrates that egui can be compiled to WebAssembly (WASM) and run in the browser, providing a unified UI framework for both desktop and web.

## Implementation

### Architecture

The PoC adds:
1. **Web module** (`crates/server/src/web/`) - Contains egui UI code that compiles to WASM
2. **WASM entry point** - In `crates/server/src/lib.rs`, provides `start_web_ui()` function
3. **HTML wrapper** - `crates/server/ui/egui.html` loads and runs the WASM UI
4. **Build script** - `build-egui-web.sh` compiles the WASM and generates JS bindings
5. **HTTP endpoints** - `/egui` and `/egui/*` serve the WASM UI files

### Key Files

```
crates/server/
├── Cargo.toml                    # Added egui-web feature and WASM dependencies
├── src/
│   ├── lib.rs                    # WASM entry point: start_web_ui()
│   ├── http.rs                   # Added /egui routes
│   └── web/
│       ├── mod.rs                # Module exports
│       └── app.rs                # WebApp - egui implementation
└── ui/
    └── egui.html                 # HTML wrapper for WASM UI

build-egui-web.sh                 # Build script for WASM compilation
```

### Features

The egui web UI (`WebApp`) demonstrates:
- ✅ Chat interface with message history
- ✅ Message input with Enter key support
- ✅ Session management (new session button)
- ✅ Status indicator (connected/disconnected)
- ✅ Toolbar with model display
- ✅ Styled messages (user vs assistant)
- ✅ Empty state with welcome message

**Note**: This is a static demo. Backend integration (WebSocket/HTTP API) is not implemented in the PoC.

## Building the WASM UI

### Prerequisites

```bash
# Install WASM target
rustup target add wasm32-unknown-unknown

# Install wasm-bindgen-cli
cargo install wasm-bindgen-cli
```

### Build

```bash
# From repository root
./build-egui-web.sh
```

This script:
1. Compiles `localgpt-server` to WASM with `egui-web` feature
2. Runs `wasm-bindgen` to generate JavaScript bindings
3. Outputs files to `crates/server/ui/egui/`:
   - `localgpt_server_bg.wasm` - The compiled WASM module
   - `localgpt_server.js` - JavaScript glue code
   - `localgpt_server.d.ts` - TypeScript definitions

### Testing

```bash
# Start the server (requires rebuilding with ort-sys fix or different embedding provider)
localgpt daemon start

# Open browser to:
http://localhost:31327/egui
```

## Code Reuse Analysis

### What Can Be Shared

The PoC demonstrates that egui code can be shared between desktop and web:

| Component | Shareability | Notes |
|-----------|--------------|-------|
| UI layouts | ✅ High | egui panels, widgets work identically |
| Message rendering | ✅ High | Same rendering code for both platforms |
| State management | ✅ High | Rust structs/enums work in WASM |
| Event handling | ✅ High | Button clicks, text input work the same |
| Styling | ✅ High | egui styling API is platform-independent |

### What Differs

| Aspect | Desktop | Web |
|--------|---------|-----|
| Entry point | `eframe::run_native()` | `eframe::WebRunner::new().start()` |
| Agent backend | Runs in-process | Needs WebSocket/HTTP to server |
| Threading | Native threads | Web Workers (limited) |
| File access | Direct filesystem | Browser sandbox only |
| Network | Raw sockets | Browser APIs only |

### Refactoring Strategy for Production

To maximize code reuse:

1. **Extract shared UI components** to `crates/core/ui/` or new `crates/ui/` crate:
   ```
   crates/ui/
   ├── chat.rs       # Chat view
   ├── sessions.rs   # Session list
   ├── status.rs     # Status panel
   └── state.rs      # Shared state types
   ```

2. **Platform-specific backends** using traits:
   ```rust
   trait Backend {
       async fn send_message(&self, msg: String) -> Result<()>;
       async fn get_sessions(&self) -> Result<Vec<Session>>;
   }
   
   // Desktop: calls Agent directly
   struct DesktopBackend { agent: Agent }
   
   // Web: calls HTTP API
   struct WebBackend { api_client: Client }
   ```

3. **Conditional compilation**:
   ```rust
   #[cfg(not(target_arch = "wasm32"))]
   type Platform = DesktopBackend;
   
   #[cfg(target_arch = "wasm32")]
   type Platform = WebBackend;
   ```

## Benefits of Egui for Web

### Pros
- ✅ **Code reuse**: Single UI codebase for desktop and web
- ✅ **Type safety**: Rust's type system prevents many UI bugs
- ✅ **Performance**: WASM + egui is fast
- ✅ **Consistency**: Identical look and feel across platforms
- ✅ **No JS framework**: No React/Vue/etc complexity
- ✅ **Immediate mode**: Simple, predictable UI updates

### Cons
- ❌ **WASM size**: ~2-3 MB (larger than HTML/CSS/JS)
- ❌ **SEO**: Not indexable by search engines (canvas-based)
- ❌ **Accessibility**: Limited screen reader support
- ❌ **Browser compatibility**: Requires WebGL/WebGPU
- ❌ **Mobile**: Touch interactions less polished than native web
- ❌ **Integration**: Cannot easily mix with HTML/DOM

## Comparison with Current Web UI

| Aspect | Current (HTML/JS) | Egui (WASM) |
|--------|-------------------|-------------|
| Initial load | Fast (~50 KB) | Slower (~2-3 MB) |
| Code reuse | None | High (80%+) |
| Development | Separate codebase | Shared Rust code |
| Accessibility | Good | Limited |
| Mobile | Good | Fair |
| Customization | Easy (CSS) | Moderate (egui styling) |
| Type safety | None | Full Rust types |
| Debugging | Browser DevTools | Rust + WASM tools |

## Recommendations

### For LocalGPT

Given LocalGPT's use case (local-only AI assistant), egui web could work well because:
- ✅ Users access via localhost, so download size matters less
- ✅ Accessibility is less critical than for public websites
- ✅ Code reuse reduces maintenance burden
- ✅ Rust type safety catches bugs early

However, keep the current HTML/JS UI if:
- ❌ Users need mobile-first experience
- ❌ Accessibility is a requirement
- ❌ You want to support older browsers
- ❌ Team prefers web technologies

### Hybrid Approach

Consider using egui for **desktop only** and keeping HTML for web:
- Desktop users get native experience
- Web users get lightweight, accessible UI
- Shared backend (Rust agent, HTTP API)
- Moderate code duplication (UI only)

## Next Steps

To move beyond PoC:

1. **Connect to backend**: Implement WebSocket client in WASM to communicate with server
2. **Handle streaming**: Show LLM streaming responses in real-time
3. **Tool execution**: Display tool calls and results
4. **Session persistence**: Load/save sessions via HTTP API
5. **Error handling**: Graceful failure and reconnection
6. **Optimize WASM**: Use `wasm-opt` to reduce binary size
7. **Add tests**: Test egui components with `eframe::test_*` utilities
8. **Documentation**: API docs for shared UI components

## Conclusion

This PoC demonstrates that egui can successfully run in the browser via WASM, enabling significant code reuse between desktop and web UIs. The decision to adopt it depends on LocalGPT's priorities around code reuse, development velocity, and user experience requirements.

For a local-only tool like LocalGPT, egui web is a viable option worth considering for production use.
