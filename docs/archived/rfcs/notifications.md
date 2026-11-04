# RFC: Notification Subsystem

**Status:** Draft, 2026-02-24; archived 2026-11-04. The assistant this was for — its heartbeat, its autonomous tasks, its channels — went on 2026-11-01 with the harness ([../../rfcs/authoring/the-harness-goes.md](../../rfcs/authoring/the-harness-goes.md)); nothing in the tree runs unattended, so there is nothing to notify from. Kept for the channel survey.
**Author:** Claude
**Date:** 2026-02-24

## Summary

Add a unified notification subsystem to LocalGPT that supports multiple notification channels (push, email, messaging) with a simple configuration-based approach. This enables users to receive alerts when autonomous tasks complete, when the agent needs attention, or for error conditions.

## Motivation

LocalGPT operates autonomously via heartbeat, but users have no way to know when:
- A heartbeat task completes
- The agent encounters an error
- User attention is required (e.g., clarification needed)
- Long-running operations finish

Currently, Telegram is the only notification channel via a separate bridge daemon. Users should be able to choose their preferred notification method without running multiple bridge processes.

## References

- **[UnifiedPush](https://unifiedpush.org/)** - Decentralized push notification standard
- **[Apprise](https://github.com/caronc/apprise)** - Unified notification library (80+ services)

## Architecture

### Design Principles

1. **HTTP-first** - Most notification services are HTTP-based; implement natively in Rust
2. **Apprise optional** - Shell out to Apprise CLI for exotic providers
3. **Event-driven** - Notifications triggered by agent lifecycle events
4. **Tag-based routing** - Route different events to different channels

### Component Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      localgpt-core                          │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │   Agent     │───▶│  Notifier   │───▶│  Providers  │     │
│  │ (lifecycle) │    │  (router)   │    │ (senders)   │     │
│  └─────────────┘    └─────────────┘    └─────────────┘     │
│                            │                                │
│                     ┌──────┴──────┐                         │
│                     │ Event Queue │                         │
│                     └─────────────┘                         │
└─────────────────────────────────────────────────────────────┘
                              │
                    ┌────────┴────────┐
                    │                 │
              ┌─────▼─────┐    ┌──────▼──────┐
              │  Native   │    │   Apprise   │
              │ Providers │    │   Bridge    │
              └───────────┘    └─────────────┘
                    │
        ┌───────────┼───────────┬───────────┐
        ▼           ▼           ▼           ▼
     ┌─────┐   ┌───────┐   ┌───────┐   ┌───────┐
     │ntfy │   │Gotify │   │Pushover│  │Matrix │
     └─────┘   └───────┘   └───────┘   └───────┘
```

### Core Types

```rust
// crates/core/src/notification/mod.rs

pub mod provider;
pub mod router;
pub mod event;

/// Notification event types that can be emitted by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NotificationEvent {
    /// Heartbeat cycle completed
    HeartbeatComplete {
        tasks_run: usize,
        summary: String,
    },
    /// Agent needs user input
    AttentionRequired {
        reason: String,
        context: String,
    },
    /// Tool execution error
    ToolError {
        tool_name: String,
        error: String,
    },
    /// Long operation completed
    OperationComplete {
        operation: String,
        result_summary: String,
    },
    /// System health issue
    SystemAlert {
        level: AlertLevel,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertLevel {
    Info,
    Warning,
    Error,
    Critical,
}

/// Notification priority for providers that support it
#[derive(Debug, Clone, Copy)]
pub enum Priority {
    Low,
    Normal,
    High,
    Urgent,
}

/// A notification to be sent
#[derive(Debug, Clone)]
pub struct Notification {
    pub event: NotificationEvent,
    pub title: String,
    pub body: String,
    pub priority: Priority,
    pub tags: Vec<String>,  // For routing to specific providers
    pub timestamp: DateTime<Utc>,
}
```

### Provider Trait

```rust
// crates/core/src/notification/provider.rs

use async_trait::async_trait;

/// Trait for notification providers
#[async_trait]
pub trait NotificationProvider: Send + Sync {
    /// Unique identifier for this provider
    fn id(&self) -> &str;

    /// Human-readable name
    fn name(&self) -> &str;

    /// Send a notification
    async fn send(&self, notification: &Notification) -> Result<(), NotificationError>;

    /// Check if provider is healthy/configured correctly
    async fn health_check(&self) -> Result<(), NotificationError>;

    /// Supported features (priority, attachments, etc.)
    fn capabilities(&self) -> ProviderCapabilities;
}

#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub supports_priority: bool,
    pub supports_attachments: bool,
    pub supports_html: bool,
    pub supports_markdown: bool,
    pub max_body_length: Option<usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum NotificationError {
    #[error("Provider configuration error: {0}")]
    Config(String),

    #[error("Failed to send notification: {0}")]
    SendFailed(String),

    #[error("Rate limited")]
    RateLimited { retry_after: Option<Duration> },

    #[error("Provider unavailable: {0}")]
    Unavailable(String),
}
```

### Native Providers (Phase 1)

Implement these directly in Rust with `reqwest`:

| Provider | URL Scheme | Features |
|----------|------------|----------|
| **ntfy** | `ntfy://topic` or `ntfys://topic` | Priority, tags, attach, markdown |
| **Gotify** | `gotify://host/token` | Priority |
| **Pushover** | `pover://user@token` | Priority, HTML, devices |
| **Matrix** | `matrix://user:pass@host/room` | Markdown |
| **Discord Webhook** | `discord://webhook_id/token` | Embeds, files |
| **Slack Webhook** | `slack://webhook_url` | Blocks |
| **Email (SMTP)** | `mailto://user:pass@host` | HTML, attachments |

### Router

```rust
// crates/core/src/notification/router.rs

use std::collections::HashMap;

/// Routes notifications to appropriate providers based on tags
pub struct NotificationRouter {
    providers: HashMap<String, Box<dyn NotificationProvider>>,
    routes: Vec<Route>,
}

#[derive(Debug, Clone)]
pub struct Route {
    /// Match events by type
    pub event_types: Option<Vec<String>>,
    /// Match events by tags (AND logic)
    pub tags: Vec<String>,
    /// Providers to notify (by ID)
    pub providers: Vec<String>,
}

impl NotificationRouter {
    pub async fn dispatch(&self, notification: Notification) -> Vec<Result<(), NotificationError>> {
        let matched_providers = self.match_providers(&notification);
        let mut results = Vec::new();

        for provider_id in matched_providers {
            if let Some(provider) = self.providers.get(&provider_id) {
                let result = provider.send(&notification).await;
                results.push(result);
            }
        }

        results
    }

    fn match_providers(&self, notification: &Notification) -> Vec<String> {
        self.routes
            .iter()
            .filter(|route| self.route_matches(route, notification))
            .flat_map(|route| route.providers.clone())
            .collect()
    }
}
```

### Configuration

```toml
# ~/.localgpt/config.toml

[notifications]
# Enable notifications globally
enabled = true

# Queue notifications if offline, retry later
queue_size = 100

# Default tags applied to all events
default_tags = ["localgpt"]

[[notifications.providers]]
id = "ntfy-main"
type = "ntfy"
url = "ntfys://ntfy.example.com/localgpt-alerts"
priority = "high"

[[notifications.providers]]
id = "email-work"
type = "smtp"
host = "smtp.example.com"
port = 587
user = "alerts@example.com"
password = "${EMAIL_PASSWORD}"  # Env var expansion
from = "LocalGPT <alerts@example.com>"
to = ["user@example.com"]

[[notifications.providers]]
id = "discord-dev"
type = "discord"
webhook_id = "123456789"
webhook_token = "${DISCORD_WEBHOOK_TOKEN}"

# Apprise bridge for exotic providers
[[notifications.providers]]
id = "apprise-custom"
type = "apprise"
urls = [
    "whatsapp://token@from/to",
    "signal://host/from/to"
]

# Routing rules
[[notifications.routes]]
# All events go to ntfy
tags = []
providers = ["ntfy-main"]

[[notifications.routes]]
# Errors also go to email
event_types = ["ToolError", "SystemAlert"]
tags = []
providers = ["email-work"]

[[notifications.routes]]
# Dev team gets attention requests via Discord
event_types = ["AttentionRequired"]
tags = []
providers = ["discord-dev"]
```

### Lifecycle Integration

```rust
// Hook into existing lifecycle system (RFC 0027)

use crate::lifecycle::{LifecycleHook, LifecycleEvent};

pub struct NotificationHook {
    router: Arc<NotificationRouter>,
}

impl LifecycleHook for NotificationHook {
    async fn on_event(&self, event: &LifecycleEvent) {
        let notification = match event {
            LifecycleEvent::HeartbeatComplete { tasks, summary } => {
                Some(Notification {
                    event: NotificationEvent::HeartbeatComplete {
                        tasks_run: *tasks,
                        summary: summary.clone(),
                    },
                    title: "Heartbeat Complete".into(),
                    body: summary.clone(),
                    priority: Priority::Low,
                    tags: vec!["heartbeat".into()],
                    timestamp: Utc::now(),
                })
            }
            LifecycleEvent::ToolError { tool, error } => {
                Some(Notification {
                    event: NotificationEvent::ToolError {
                        tool_name: tool.clone(),
                        error: error.clone(),
                    },
                    title: format!("Tool Error: {}", tool),
                    body: error.clone(),
                    priority: Priority::High,
                    tags: vec!["error".into()],
                    timestamp: Utc::now(),
                })
            }
            _ => None,
        };

        if let Some(notification) = notification {
            // Fire and forget - don't block agent on notification
            let router = self.router.clone();
            tokio::spawn(async move {
                let _ = router.dispatch(notification).await;
            });
        }
    }
}
```

## Mobile Integration (UnifiedPush)

### Android App

The Android app registers as a UnifiedPush client:

```kotlin
// apps/android/app/src/main/java/app/localgpt/UnifiedPushManager.kt

import org.unifiedpush.android.connector.UnifiedPush

class UnifiedPushManager(private val context: Context) {
    fun register() {
        UnifiedPush.registerApp(
            context,
            "localgpt",
            features = listOf(UnifiedPush.FEATURE_BYTES_MESSAGE)
        )
    }

    fun getEndpoint(): String? {
        return UnifiedPush.getEndpoint(context, "localgpt")
    }
}
```

### Daemon to Mobile Push

When daemon starts, mobile apps register their UnifiedPush endpoint:

```toml
# ~/.localgpt/config.toml

[notifications.mobile]
# Enable pushing to registered mobile devices
enabled = true

# Devices auto-register via HTTP API
auto_register = true

# Events to push to mobile
events = ["AttentionRequired", "HeartbeatComplete", "SystemAlert"]
```

```rust
// HTTP endpoint for mobile registration
// POST /api/devices/register
// { "endpoint": "https://ntfy.example.com/device-abc123" }

async fn register_device(
    endpoint: String,
    device_name: Option<String>,
) -> Result<(), Error> {
    // Store endpoint for later push
    device_store::register(endpoint, device_name).await
}

// Push to all registered devices
async fn push_to_devices(notification: &Notification) {
    for device in device_store::all().await {
        // Use WebPush or ntfy protocol
        webpush::send(&device.endpoint, notification).await;
    }
}
```

## Apprise Bridge

For providers without native Rust implementations:

```rust
// crates/core/src/notification/provider/apprise.rs

pub struct AppriseProvider {
    cli_path: PathBuf,
    urls: Vec<String>,
}

impl AppriseProvider {
    pub fn new(cli_path: PathBuf, urls: Vec<String>) -> Self {
        Self { cli_path, urls }
    }
}

#[async_trait]
impl NotificationProvider for AppriseProvider {
    fn id(&self) -> &str { "apprise" }
    fn name(&self) -> &str { "Apprise Bridge" }

    async fn send(&self, notification: &Notification) -> Result<(), NotificationError> {
        let mut cmd = Command::new(&self.cli_path);
        cmd.arg("--title").arg(&notification.title)
           .arg("--body").arg(&notification.body);

        for url in &self.urls {
            cmd.arg(url);
        }

        let output = cmd.output()
            .map_err(|e| NotificationError::SendFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(NotificationError::SendFailed(
                String::from_utf8_lossy(&output.stderr).into()
            ));
        }

        Ok(())
    }
}
```

## Implementation Phases

### Phase 1: Core Infrastructure
- [ ] Notification types and traits in `localgpt-core`
- [ ] `NotificationRouter` implementation
- [ ] Configuration parsing
- [ ] Lifecycle hook integration

### Phase 2: Native Providers
- [ ] ntfy provider
- [ ] Gotify provider
- [ ] Pushover provider
- [ ] Discord webhook provider
- [ ] Matrix provider

### Phase 3: Apprise Bridge
- [ ] Apprise CLI detection
- [ ] Subprocess provider wrapper
- [ ] Fallback for unknown URL schemes

### Phase 4: Mobile Push
- [ ] Device registration API
- [ ] UnifiedPush endpoint storage
- [ ] WebPush implementation
- [ ] Android client integration

### Phase 5: HTTP API
- [ ] `POST /api/notify` endpoint
- [ ] `GET /api/notifications/providers`
- [ ] Provider health status

## Security Considerations

1. **Credential Storage** - Passwords stored in config should use environment variable expansion, not plaintext
2. **Rate Limiting** - Built-in rate limiting per provider to prevent abuse
3. **Message Sanitization** - Strip sensitive data from notification bodies
4. **Endpoint Validation** - Validate UnifiedPush endpoints before storing

## Testing Strategy

1. **Mock Provider** - In-memory provider for testing routing logic
2. **Integration Tests** - Test against ntfy.dev public server
3. **Config Validation** - Early validation of provider configurations

## Open Questions

1. **Attachment Support** - Should we support file attachments in Phase 1?
2. **Notification History** - Store sent notifications for debugging?
3. **Bi-directional** - Allow users to respond to notifications (e.g., approve/deny actions)?

## Alternatives Considered

1. **Python Apprise Only** - Simpler but requires Python dependency
2. **Single Provider (ntfy)** - Too limiting for user choice
3. **Bridge-based Only** - Current approach, but requires running separate daemons

## References

- [UnifiedPush Specification](https://unifiedpush.org/)
- [Apprise Documentation](https://github.com/caronc/apprise)
- [ntfy Documentation](https://ntfy.sh/docs/)
- [WebPush RFC 8030](https://datatracker.ietf.org/doc/html/rfc8030)
