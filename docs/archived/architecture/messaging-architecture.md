# Messaging and Rendering Architecture

**Date:** 2026-02-23
**Purpose:** Architecture decisions for mobile connectivity, 3D rendering, and messaging integrations

## Overview

This document covers architectural decisions for:
1. Server-side 3D rendering for mobile clients
2. Local client connectivity via the bridge IPC socket (CLI bridge)
3. IPC vs HTTP/WebSocket protocol choices

---

## 1. Process Architecture

### Single Process vs Multi-Process

Moltis (reference implementation) runs everything in a single process with tokio for concurrency. This works well but has implications for LocalGPT's use cases.

```
┌─────────────────────────────────────────────────────────────┐
│                 Single Process (Moltis-style)               │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              tokio::main (async runtime)             │   │
│  │                                                      │   │
│  │   ┌─────────────┐  ┌──────────────┐  ┌───────────┐  │   │
│  │   │ HTTP Server │  │  WebSocket   │  │  Agent    │  │   │
│  │   │   (Axum)    │  │  Connections │  │  Runner   │  │   │
│  │   └─────────────┘  └──────────────┘  └───────────┘  │   │
│  │         │                │                 │        │   │
│  │         └────────────────┴─────────────────┘        │   │
│  │                    Shared State                     │   │
│  │         (Arc<GatewayState>, ProviderRegistry,       │   │
│  │          SessionStore, ToolRegistry, etc.)          │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

**Pros:**
- Simpler state sharing
- No IPC overhead
- Easier debugging

**Cons:**
- No crash isolation
- All components share memory

### Recommended: Hybrid Architecture

For LocalGPT with Gen mode and the CLI bridge:

```
┌─────────────────────────────────────────────────────────────────┐
│                      LocalGPT System                            │
│                                                                 │
│  ┌─────────────────┐                    ┌─────────────────┐    │
│  │ localgpt daemon │                    │ localgpt-gen    │    │
│  │                 │    Unix Socket     │                 │    │
│  │ HTTP/WS Server ─┼───────────────────┼─► Bevy Renderer │    │
│  │ Agent           │   SceneRequest    │   (main thread) │    │
│  │ Memory          │◄──────────────────┼─ SceneResult    │    │
│  └─────────────────┘   RenderedImage   └─────────────────┘    │
│         │                                                      │
│         │ IPC                                                  │
│         ▼                                                      │
│  ┌─────────────────┐                                          │
│  │ Bridge Daemon   │                                          │
│  │ - cli           │                                          │
│  └─────────────────┘                                          │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. Headless Rendering

### Can Bevy Run Headless?

**Short answer: Yes, but with caveats.**

| Mode | Description | Main Thread Required? |
|------|-------------|----------------------|
| **Headless (no GPU)** | No rendering at all, just simulation | No |
| **Headless (offscreen GPU)** | Render to texture, no window | **Yes on macOS** |
| **Headless (software)** | CPU-only rendering (OSMesa) | No |

### The macOS Constraint

The constraint is NOT Bevy itself, but wgpu/Metal on macOS:

```rust
// This IS possible on Linux/Windows with Vulkan:
app.add_plugins(DefaultPlugins.set(WindowPlugin {
    primary_window: None,  // No window
    ..default()
}));

// But on macOS, Metal still needs a proper graphics context
// Even offscreen rendering needs to be on main thread
```

### Solution Options for Server-Side Rendering

#### Option A: Virtual Display

```
┌─────────────┐
│  Xvfb /     │     Bevy thinks there's a display
│  Quartz     │     Can render normally
│  Debug      │     Works on Linux (Xvfb), tricky on macOS
└─────────────┘
```

#### Option B: Separate Gen Process (Recommended)

```
┌─────────────┐      IPC       ┌─────────────┐
│   Gateway   │◄──────────────►│  Gen Process│
│ (any thread)│                 │ (main thread)│
│             │  SceneRequest  │              │
│ HTTP/WS     │◄───────────────│  Bevy + wgpu │
│ Mobile API  │  RenderedImage │              │
└─────────────┘                 └─────────────┘

✅ Works today on macOS
✅ Crash isolation
✅ Can run on different machine
```

#### Option C: Client-Side Rendering

```
┌─────────────┐  SceneGraph   ┌─────────────┐
│   Server    │──────────────►│ Mobile App  │
│             │  (JSON/gltf)  │ (SceneKit/  │
│             │               │  Metal)     │
└─────────────┘                └─────────────┘

✅ No server GPU needed
✅ Lower latency (render where display is)
❌ Limited by mobile GPU capability
❌ Can't do server-side AI vision
```

---

## 3. IPC vs HTTP/WebSocket

### Integration Patterns

#### Pattern 1: Local CLI Bridge

```
┌──────────┐    IPC        ┌──────────┐
│   CLI    │◄─────────────►│ LocalGPT │
│  Bridge  │  Unix Socket  │  Daemon  │
└──────────┘               └──────────┘

✅ IPC works - same machine
✅ Peer identity verified via socket credentials
```

#### Pattern 2: Bridge Daemon (Same Machine)

```
┌──────────────────────────────────────────────────────────┐
│                      Your Machine                         │
│                                                          │
│  ┌──────────┐    IPC          ┌──────────┐              │
│  │ LocalGPT │◄───────────────►│   CLI    │──► Terminal  │
│  │  Daemon  │   Unix Socket   │  Bridge  │     Chat     │
│  └──────────┘                 └──────────┘              │
└──────────────────────────────────────────────────────────┘

✅ IPC works (same machine)
✅ Peer identity verified via socket credentials
```

#### Pattern 3: Distributed Setup

```
┌─────────────┐              ┌─────────────┐
│  Mac Studio │   HTTP/WS    │  VPS/Cloud  │
│  (Gen +     │◄────────────►│  (Daemon +  │
│   Storage)  │   Network    │  HTTP/WS)   │
└─────────────┘              └─────────────┘
      │                              │
      ▼                              ▼
┌─────────────┐              ┌─────────────┐
│ Bevy Gen    │              │ Web UI /    │
│ (main thread)│             │ Mobile Apps │
└─────────────┘              └─────────────┘

✅ HTTP/WS required for cross-machine
❌ IPC only works on same machine
```

### Protocol Comparison

| Aspect | IPC (Unix Socket) | HTTP/WebSocket |
|--------|-------------------|----------------|
| **Same machine** | ✅ Fast, low latency | ✅ Works |
| **Different machine** | ❌ No | ✅ Works |
| **Language agnostic** | ❌ Need IPC library | ✅ Any HTTP client |
| **Debugging** | Harder | Easy (curl, browser) |
| **Firewall/NAT** | N/A | May need config |
| **Authentication** | File permissions | Need tokens |
| **Latency** | ~0.1ms | ~1-10ms (localhost) |
| **Implementation** | serde + socket | Any HTTP framework |

---

## 4. Recommended Architecture

### Protocol Selection by Use Case

| Use Case | Protocol | Reason |
|----------|----------|--------|
| Mobile app → Server | HTTP/WS | Network required |
| Web UI → Server | HTTP/WS | Network required |
| Server → Gen Process | IPC | Same machine, low latency |
| Server → Bridge (local) | IPC or HTTP | Flexible |
| Server → Bridge (remote) | HTTP/WS | Network required |

### Bridge Protocol Design

Bridges should support BOTH IPC and HTTP, using the same message protocol:

```rust
/// Bridge message protocol - works over IPC or HTTP
#[derive(Serialize, Deserialize)]
enum BridgeMessage {
    // Incoming from bridge
    Message {
        channel: String,
        sender: String,
        text: String,
        attachments: Vec<Attachment>,
    },

    // Outgoing to bridge
    Reply {
        channel: String,
        text: String,
        /// For gen results - rendered images
        image: Option<ImageData>,
    },

    // Control messages
    Ping,
    Pong,
}

/// Image data for gen results
#[derive(Serialize, Deserialize)]
struct ImageData {
    format: ImageFormat,  // PNG, JPEG, WebP
    data: Vec<u8>,        // Raw bytes
    width: u32,
    height: u32,
}

/// Connection options
enum BridgeConnection {
    /// Unix domain socket (local only)
    UnixSocket { path: PathBuf },
    /// HTTP REST API
    Http { base_url: String },
    /// WebSocket connection
    WebSocket { url: String },
}
```

### Gen Process Protocol

```rust
/// Commands sent to Gen process
#[derive(Serialize, Deserialize)]
enum GenRequest {
    /// Create a new 3D scene
    CreateScene { description: String },
    /// Modify existing scene
    UpdateScene { scene_id: Uuid, changes: SceneChanges },
    /// Render a frame to image
    RenderFrame { scene_id: Uuid, camera: CameraParams },
    /// Export scene as glTF/GLB
    ExportScene { scene_id: Uuid, format: ExportFormat },
    /// Destroy a scene
    DestroyScene { scene_id: Uuid },
}

/// Responses from Gen process
#[derive(Serialize, Deserialize)]
enum GenResponse {
    SceneCreated { scene_id: Uuid },
    FrameRendered { image: ImageData },
    SceneExported { data: Vec<u8>, format: ExportFormat },
    Error { message: String },
}

/// Camera parameters for rendering
#[derive(Serialize, Deserialize)]
struct CameraParams {
    position: [f32; 3],
    look_at: [f32; 3],
    fov_degrees: f32,
    width: u32,
    height: u32,
}
```

---

## 5. Security Considerations

### Single Process Security (Moltis Model)

Moltis achieves security through **container sandboxing for tool execution**, not process isolation:

```
┌─────────────────────────────────────────────────────────────────┐
│                      Moltis Process (untrusted)                 │
│  ┌────────────┐  ┌────────────┐  ┌────────────┐                │
│  │   HTTP     │  │  Agent     │  │   Tool     │                │
│  │  Gateway   │  │  Runner    │  │  Registry  │                │
│  └────────────┘  └────────────┘  └────────────┘                │
│                           │                                     │
│                           ▼                                     │
│              ┌────────────────────────┐                        │
│              │  Container Sandbox API │                        │
│              │  (Docker/Apple)        │                        │
│              └────────────────────────┘                        │
└──────────────────────────│──────────────────────────────────────┘
                           │
                           ▼ (isolated execution)
         ┌─────────────────────────────────────────┐
         │         Container (Docker/Apple)        │
         │  - Filesystem isolation                 │
         │  - Network isolation (optional)         │
         │  - Resource limits                      │
         └─────────────────────────────────────────┘
```

### Security Layers

| Layer | Mechanism | Purpose |
|-------|-----------|---------|
| **1. Tool Approval** | `ApprovalManager` | User approves dangerous commands |
| **2. Security Level** | `Deny/Allowlist/Full` | Whitelist safe binaries |
| **3. Container Sandbox** | Docker/Apple Container | OS-level isolation for exec |
| **4. Resource Limits** | Memory, CPU, PIDs | Prevent resource exhaustion |
| **5. Network Isolation** | `no_network: true` | Block outbound network |

### IPC Security

When using IPC between processes:

```rust
/// Secure IPC configuration
struct IpcConfig {
    /// Unix socket path (file permissions for auth)
    socket_path: PathBuf,
    /// Socket file mode (0o600 for owner only)
    socket_mode: u32,
    /// Optional token-based auth for HTTP fallback
    auth_token: Option<Secret<String>>,
}

impl IpcConfig {
    fn create_socket(&self) -> Result<UnixListener> {
        let listener = UnixListener::bind(&self.socket_path)?;

        // Set restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.socket_path,
                std::fs::Permissions::from_mode(self.socket_mode))?;
        }

        Ok(listener)
    }
}
```

---

## 6. Implementation Roadmap

### Phase 1: HTTP/WS Foundation

1. Implement HTTP REST API for bridge messages
2. Add WebSocket support for real-time updates
3. Mobile app connects via HTTP/WS

### Phase 2: Gen Process IPC

1. Extract Gen into separate binary with IPC server
2. Define `GenRequest`/`GenResponse` protocol
3. Gateway connects via Unix socket

### Phase 3: Bridge Daemon

1. Create bridge protocol (same messages, multiple transports)
2. Implement the CLI bridge over IPC
3. Add further local clients as needed

### Phase 4: Distributed Deployment

1. Add authentication tokens for HTTP bridges
2. Support remote Gen processes over network
3. Cloud deployment option

---

## References

- **Moltis security model:** `external/moltis/crates/tools/src/sandbox.rs`
- **LocalGPT Gen:** `localgpt/crates/gen/`
- **Bridge architecture:** `docs/bridge-daemon-strategy.md`
- **Cross-platform isolation:** `docs/cross-platform-isolation.md`
