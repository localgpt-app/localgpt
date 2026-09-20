# Bridge Development & Testing Guide

This guide explains how to develop and test secure bridge daemons for LocalGPT using the `localgpt-bridge` architecture.

## Overview

The bridge architecture consists of two parts:
1.  **Server (`localgpt daemon`)**: Holds the master key, verifies identities, and dispenses credentials.
2.  **Client (Bridge Binary)**: Connects to the server, proves its identity, and receives its specific secrets (e.g., API tokens).

## Versioning & Distribution

To ensure stability and decouple development cycles, adhere to the following versioning strategy:

1.  **Independent Versioning:** Each bridge binary (e.g., `localgpt-bridge-cli`) should have its own semantic version (starting at `0.1.0`). Updates to a bridge do not require a version bump of the core system.
2.  **Protocol Compatibility:** All bridges must depend on a compatible version of the `localgpt-bridge` library (the IPC layer). This library defines the wire protocol.
3.  **Distribution:** The `localgpt-bridge` library will eventually be published to crates.io to allow the community to build third-party bridges without forking the main repository.

## Prerequisites

Ensure you have built the project:
```bash
cd localgpt
cargo build
```

## Testing Workflow

You can verify the entire secure credential flow using the included `test-bridge` binary.

### 1. Initialize
Generate the device master key if you haven't already.
```bash
cargo run -- init
```

### 2. Register a Credential
Securely store a secret for your bridge. This uses the main CLI to encrypt the secret with the device master key.

```bash
# Register a dummy secret for the bridge ID "test-bridge"
# This creates ~/.local/share/localgpt/bridges/test-bridge.enc
cargo run -- bridge register --id test-bridge --secret "super-secret-token-123"
```

### 3. Start the Daemon
The daemon hosts the secure IPC socket. Run it in the foreground to monitor logs and verify the socket path.

```bash
# Start daemon in foreground
cargo run -- daemon start --foreground
```

**Note:** Look for the log line indicating the socket path:
`INFO BridgeManager listening on .../bridge.sock`

On **macOS**, this path depends on your `TMPDIR` and typically looks like:
`/var/folders/xx/xxxx/T/localgpt-501/bridge.sock`

### 4. Run the Bridge Client
In a **new terminal**, run the test client. It will connect to the daemon, authenticate, and request the secret for "test-bridge".

**macOS / Linux:**
```bash
# Replace <SOCKET_PATH> with the full path from the daemon logs
# Example: /var/folders/.../T/localgpt-501/bridge.sock
cargo run -p localgpt-bridge --bin test_client -- --socket "<SOCKET_PATH>" --bridge-id test-bridge
```

**Windows:**
```bash
# On Windows, the socket is a named pipe
cargo run -p localgpt-bridge --bin test_client -- --socket "localgpt-bridge" --bridge-id test-bridge
```

### Expected Output

**Client Terminal:**
```text
INFO Connecting to bridge socket at: ...
INFO Requesting credentials for: test-bridge
INFO Successfully retrieved credentials!
INFO Secret length: 22 bytes
INFO Secret content (utf8): super-secret-token-123
```

**Daemon Terminal:**
```text
INFO Accepted connection from: PeerIdentity { uid: Some(501), ... }
```

## Developing a New Bridge

To create a new bridge (e.g., a local client like `localgpt-bridge-cli`):

1.  **New Binary**: Create a new crate or binary target in `crates/bridge/src/bin/` or a separate repository.
2.  **Dependencies**: Depend on `localgpt-bridge` and `tarpc`.
3.  **Connect**: Use `localgpt_bridge::connect(socket_path)` to establish the secure channel.
4.  **Authenticate**: Call `client.get_credentials(context, "my-bridge-id")`.
5.  **Run**: Initialize your service (e.g., terminal chat UI) using the retrieved secret.

### Example Code

```rust
use localgpt_bridge::connect;
use tarpc::context;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Connect
    let socket = "/path/to/socket"; 
    let client = connect(socket).await?;

    // 2. Fetch Secret
    let secret_bytes = client.get_credentials(context::current(), "cli".to_string()).await??;
    let token = String::from_utf8(secret_bytes)?;

    // 3. Start Bot
    start_chat_client(&token).await?;
    
    Ok(())
}
```
