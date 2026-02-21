---
sidebar_position: 12
---

# HTTP API

LocalGPT provides a RESTful HTTP API when running in daemon mode.

## Starting the Server

```bash
localgpt daemon start
```

The server listens on `http://127.0.0.1:31327` by default.

## Endpoints

### Health Check

Check if the server is running.

```
GET /health
```

**Response:**
```json
{
  "status": "ok"
}
```

### Server Status

Get detailed server status.

```
GET /api/status
```

**Response:**
```json
{
  "version": "0.1.3",
  "uptime_seconds": 3600,
  "model": "claude-cli/opus",
  "memory": {
    "files_indexed": 42,
    "chunks": 156
  },
  "heartbeat": {
    "enabled": true,
    "last_run": "2026-02-14T10:30:00Z",
    "next_run": "2026-02-14T11:00:00Z"
  }
}
```

### Chat

Send a message and get a response.

```
POST /api/chat
Content-Type: application/json
```

**Request Body:**
```json
{
  "message": "What is the capital of France?",
  "model": "claude-cli/opus",
  "include_memory": true
}
```

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `message` | string | Yes | The user message |
| `model` | string | No | Override default model |
| `include_memory` | boolean | No | Load memory context (default: true) |

**Response:**
```json
{
  "response": "The capital of France is Paris.",
  "model": "claude-cli/opus",
  "tokens": {
    "prompt": 45,
    "completion": 12,
    "total": 57
  },
  "tools_used": []
}
```

**With Tool Usage:**
```json
{
  "response": "I found 3 files in your project...",
  "model": "claude-cli/opus",
  "tokens": {
    "prompt": 120,
    "completion": 45,
    "total": 165
  },
  "tools_used": [
    {
      "name": "bash",
      "arguments": {"command": "ls ~/project"}
    }
  ]
}
```

### Memory Search

Search the memory index.

```
GET /api/memory/search?q=<query>&limit=<n>
```

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `q` | string | Yes | Search query |
| `limit` | integer | No | Max results (default: 10) |

**Example:**
```
GET /api/memory/search?q=rust%20async&limit=5
```

**Response:**
```json
{
  "query": "rust async",
  "results": [
    {
      "file": "memory/2026-02-14.md",
      "content": "...discussed async/await patterns in Rust...",
      "score": 0.95,
      "line_start": 45,
      "line_end": 52
    },
    {
      "file": "MEMORY.md",
      "content": "## Rust Async Notes...",
      "score": 0.72,
      "line_start": 10,
      "line_end": 18
    }
  ],
  "total": 2
}
```

### Memory Statistics

Get memory system statistics.

```
GET /api/memory/stats
```

**Response:**
```json
{
  "workspace": "~/.local/share/localgpt/workspace",
  "files": {
    "total": 47,
    "memory_md": {
      "size_bytes": 2456,
      "lines": 42
    },
    "heartbeat_md": {
      "size_bytes": 312,
      "lines": 8
    },
    "daily_logs": {
      "count": 45,
      "total_size_bytes": 131072
    }
  },
  "index": {
    "chunks": 156,
    "last_indexed": "2026-02-14T10:30:00Z",
    "database_size_bytes": 250880
  }
}
```

## Error Responses

All endpoints return errors in a consistent format:

```json
{
  "error": {
    "code": "invalid_request",
    "message": "Missing required parameter: message"
  }
}
```

**Error Codes:**
| Code | HTTP Status | Description |
|------|-------------|-------------|
| `invalid_request` | 400 | Bad request parameters |
| `not_found` | 404 | Resource not found |
| `provider_error` | 502 | LLM provider error |
| `internal_error` | 500 | Internal server error |

## Configuration

Configure the HTTP server in `config.toml`:

```toml
[server]
enabled = true
port = 31327
bind = "127.0.0.1"
```

**Security Note:** The default bind address `127.0.0.1` only accepts local connections. To expose the API over the network, change to `0.0.0.0`, but be aware this has security implications.

## Using with curl

**Health check:**
```bash
curl http://localhost:31327/health
```

**Send a chat message:**
```bash
curl -X POST http://localhost:31327/api/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "Hello, how are you?"}'
```

**Search memory:**
```bash
curl "http://localhost:31327/api/memory/search?q=rust"
```

## Sessions

The API supports multi-turn conversations with session persistence:

```bash
# Create a session
curl -X POST http://localhost:31327/api/sessions

# Stream a chat message in a session
curl -X POST http://localhost:31327/api/chat/stream \
  -H "Content-Type: application/json" \
  -d '{"session_id": "...", "message": "Hello!"}'

# List sessions
curl http://localhost:31327/api/sessions

# Delete a session
curl -X DELETE http://localhost:31327/api/sessions/<id>
```

## Streaming & WebSocket

- **SSE Streaming** — `POST /api/chat/stream` returns Server-Sent Events for real-time responses
- **WebSocket** — `GET /api/ws` for bidirectional real-time chat

## Security Notes

- Default bind `127.0.0.1` only accepts local connections
- No API key authentication — rely on network security (localhost binding)
- Tool approval mode available for dangerous operations
