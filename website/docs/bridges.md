---
sidebar_position: 13
---

# Messaging Bridges

LocalGPT supports connecting to messaging platforms through **bridge daemons** — lightweight binaries that relay messages between a chat platform and LocalGPT's agent.

Three official bridges are available:

| Bridge | Platform | Library |
|--------|----------|---------|
| `localgpt-bridge-telegram` | Telegram | teloxide |
| `localgpt-bridge-discord` | Discord | serenity |
| `localgpt-bridge-whatsapp` | WhatsApp | whatsapp-web.js (Node.js adapter) |

## How Bridges Work

```
┌──────────┐       ┌──────────────┐       ┌──────────────────┐
│ Telegram │◀─────▶│   Bridge     │◀─────▶│ LocalGPT Daemon  │
│ / Discord│  API  │   Binary     │  IPC   │ (agent + memory) │
│ / WhatsApp│      │              │ socket │                  │
└──────────┘       └──────────────┘       └──────────────────┘
```

1. The **LocalGPT daemon** starts an IPC socket and holds encrypted credentials.
2. The **bridge binary** connects to the daemon, authenticates, and retrieves its API token.
3. The bridge logs in to the messaging platform and relays messages to/from the LocalGPT agent.

All credentials are encrypted at rest with your device master key — bridge binaries never store secrets directly.

## Prerequisites

Before setting up any bridge, ensure you have:

1. **LocalGPT installed and initialized:**
   ```bash
   localgpt init
   ```

2. **The daemon running** (bridges connect to the daemon via IPC):
   ```bash
   localgpt daemon start
   ```

## Telegram Bridge

### 1. Create a Telegram Bot

1. Open Telegram and message [@BotFather](https://t.me/BotFather).
2. Send `/newbot` and follow the prompts to choose a name and username.
3. Copy the **API token** BotFather gives you (e.g., `123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11`).

### 2. Register the Token

```bash
localgpt bridge register --id telegram --secret "YOUR_TELEGRAM_BOT_TOKEN"
```

This encrypts the token with your device master key and stores it in `~/.local/share/localgpt/bridges/telegram.enc`.

### 3. Build and Run the Bridge

```bash
# From the repository root
cargo build -p localgpt-bridge-telegram --release

# Run the bridge (daemon must be running)
./target/release/localgpt-bridge-telegram
```

The bridge connects to the daemon, retrieves its token, and starts listening for Telegram messages.

### 4. Pair Your Account

1. Send any message to your bot in Telegram.
2. The bot replies with a **6-digit pairing code**.
3. Enter that code in the bot chat.
4. Once paired, the bot responds as LocalGPT with full chat, tool use, and memory support.

### Bot Commands

| Command | Description |
|---------|-------------|
| `/start` | Show welcome message |
| `/help` | List available commands |
| `/new` | Start a new session |
| `/status` | Show session stats (tokens, idle time) |
| `/compact` | Compress context window |
| `/clear` | Clear session history |
| `/memory <query>` | Search persistent memory |
| `/model [name]` | View or switch models |
| `/skills` | List installed skills |
| `/unpair` | Reset pairing |

## Discord Bridge

### 1. Create a Discord Bot

1. Go to the [Discord Developer Portal](https://discord.com/developers/applications).
2. Click **New Application**, give it a name, then go to the **Bot** tab.
3. Click **Reset Token** and copy the bot token.
4. Under **Privileged Gateway Intents**, enable **Message Content Intent**.
5. Go to **OAuth2 → URL Generator**, select the `bot` scope with **Send Messages** and **Read Message History** permissions.
6. Open the generated URL to invite the bot to your server.

### 2. Register the Token

```bash
localgpt bridge register --id discord --secret "YOUR_DISCORD_BOT_TOKEN"
```

### 3. Build and Run the Bridge

```bash
cargo build -p localgpt-bridge-discord --release
./target/release/localgpt-bridge-discord
```

### 4. Pair Your Account

1. Send a direct message to the bot (or mention it in a server channel).
2. The bot replies with a **6-digit pairing code**.
3. Enter the code to complete pairing.

Discord bridge supports both DMs and server channels. Each channel/DM gets its own independent session.

### Bot Commands

Same commands as the Telegram bridge — use `/help` in any conversation to see them.

## WhatsApp Bridge

The WhatsApp bridge uses a **two-component architecture** because of library compatibility constraints:

```
WhatsApp ↔ Node.js Adapter ↔ Rust Bridge (HTTP) ↔ LocalGPT Daemon (IPC)
```

### 1. Register the Bridge

WhatsApp authentication is QR-based (no API token), but the bridge still needs a registered ID:

```bash
localgpt bridge register --id whatsapp --secret "placeholder"
```

### 2. Build and Run the Rust Bridge

```bash
cargo build -p localgpt-bridge-whatsapp --release
./target/release/localgpt-bridge-whatsapp
```

This starts an HTTP relay server on `http://127.0.0.1:3000`.

### 3. Run the Node.js Adapter

In a separate terminal:

```bash
cd bridges/whatsapp
npm install
npm start
```

A **QR code** appears in the terminal. Scan it with your WhatsApp mobile app (**Linked Devices → Link a Device**).

### 4. Pair Your Account

Once WhatsApp Web is connected:

1. Send a message to yourself or have someone message you.
2. The bot replies with a **6-digit pairing code**.
3. Enter the code to complete pairing.

### Architecture Notes

- The Node.js adapter (`adapter.js`) uses [`whatsapp-web.js`](https://github.com/nicholasantonio/whatsapp-web.js) for WhatsApp Web protocol support.
- Messages are relayed via `POST /webhook` to the Rust bridge on `localhost:3000`.
- A `/health` endpoint is available for monitoring.

## Common Features

All three bridges share these features:

- **6-digit pairing** — First-time users must enter a one-time code to link their account.
- **Session management** — Each user/channel gets an independent conversation session.
- **Streaming responses** — Replies stream in with debounced edits (every ~2 seconds) to avoid rate limits.
- **Memory integration** — Full access to LocalGPT's persistent memory system.
- **Turn gating** — Only one message is processed at a time per session to prevent race conditions.
- **Model selection** — Switch models mid-conversation with `/model`.
- **Message chunking** — Long responses are automatically split to respect platform limits (4096 chars for Telegram, 2000 for Discord).

## Troubleshooting

### Bridge can't connect to daemon

Make sure the daemon is running:
```bash
localgpt daemon status
```

If the daemon is running but the bridge can't find the socket, check the daemon logs for the socket path:
```
INFO BridgeManager listening on /run/user/1000/localgpt/bridge.sock
```

### "NotRegistered" error

The bridge ID hasn't been registered. Run:
```bash
localgpt bridge register --id <bridge-name> --secret "YOUR_TOKEN"
```

### Telegram bot not responding

1. Verify the bot token is correct — message [@BotFather](https://t.me/BotFather) and use `/mybots` to check.
2. Ensure no other process is polling the same bot token.
3. Check bridge logs for connection errors.

### WhatsApp QR code expired

Restart the Node.js adapter (`npm start`) to generate a fresh QR code. The adapter stores session data locally, so subsequent starts may reconnect automatically.

## Developing Custom Bridges

See the [Bridge Development Guide](https://github.com/localgpt-app/localgpt/blob/main/docs/bridge-development.md) for details on building your own bridge using the `localgpt-bridge` IPC library.
