---
sidebar_position: 7
---

# Daemon Command

The `daemon` command manages the LocalGPT background process, which provides the HTTP API and heartbeat functionality.

## Usage

```bash
localgpt daemon <SUBCOMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| `start` | Start the daemon |
| `stop` | Stop a running daemon |
| `restart` | Restart the daemon (stop then start) |
| `status` | Check daemon status |
| `heartbeat` | Run a single heartbeat cycle |

## Starting the Daemon

```bash
# Start in background (default, daemonizes on Unix)
localgpt daemon start

# Start in foreground (logs to stdout, useful for debugging)
localgpt daemon start --foreground

# The daemon will:
# 1. Start the HTTP server on configured port
# 2. Start the heartbeat runner (if enabled)
# 3. Initialize the memory file watcher
```

Output:
```
Starting LocalGPT daemon...
HTTP server listening on 127.0.0.1:18790
Heartbeat enabled (interval: 30m)
Memory watcher started
```

## Checking Status

```bash
localgpt daemon status
```

Output:
```
LocalGPT Daemon Status
──────────────────────
Running: Yes
PID: 12345
Uptime: 2h 15m
HTTP: http://127.0.0.1:18790
Heartbeat: Enabled (last run: 5m ago)
Memory: 42 files indexed
```

## Stopping the Daemon

```bash
localgpt daemon stop
```

## Restarting the Daemon

```bash
# Restart in background
localgpt daemon restart

# Restart in foreground
localgpt daemon restart --foreground
```

## Manual Heartbeat

Run a single heartbeat cycle without starting the full daemon:

```bash
localgpt daemon heartbeat
```

This is useful for:
- Testing heartbeat tasks
- Running heartbeat from cron
- One-off autonomous task execution

## PID File

The daemon writes its PID to:

```
~/.local/state/localgpt/daemon.pid
```

This file is used by `status` and `stop` commands.

## Configuration

Daemon behavior is controlled in `config.toml`:

```toml
[server]
enabled = true
port = 18790
bind = "127.0.0.1"

[heartbeat]
enabled = true
interval = "30m"
active_hours = { start = "09:00", end = "22:00" }
```

## Running as a Service

### macOS (launchd)

Create `~/Library/LaunchAgents/com.localgpt.daemon.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.localgpt.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/localgpt</string>
        <string>daemon</string>
        <string>start</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/localgpt.out.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/localgpt.err.log</string>
</dict>
</plist>
```

Load with:
```bash
launchctl load ~/Library/LaunchAgents/com.localgpt.daemon.plist
```

### Linux (systemd)

Create `~/.config/systemd/user/localgpt.service`:

```ini
[Unit]
Description=LocalGPT Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/localgpt daemon start
Restart=on-failure
Environment=OPENAI_API_KEY=your-key-here

[Install]
WantedBy=default.target
```

Enable with:
```bash
systemctl --user enable localgpt
systemctl --user start localgpt
```
