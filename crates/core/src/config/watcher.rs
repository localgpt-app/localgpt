//! Config hot-reload support
//!
//! Watches `config.toml` for changes and provides updated config to services.
//! Also supports SIGHUP on Unix for manual reload triggers.

use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, watch};
use tracing::{debug, info, warn};

use super::Config;

/// Minimum time between reloads (debounce)
const DEBOUNCE_INTERVAL: Duration = Duration::from_millis(500);

/// Handle for accessing the latest config and subscribing to changes.
pub struct ConfigWatcher {
    /// Current config (updated on reload)
    config: Arc<RwLock<Config>>,
    /// Channel for broadcasting config updates
    tx: watch::Sender<Config>,
    /// File watcher handle (kept alive for the watcher to work)
    _watcher: RecommendedWatcher,
    /// Path to the config file
    config_path: PathBuf,
    /// Last reload time (for debouncing)
    last_reload: Arc<RwLock<Instant>>,
}

impl ConfigWatcher {
    /// Start watching config.toml for changes.
    ///
    /// Returns a handle that provides access to the latest config via `config()`
    /// and allows subscribing to changes via `subscribe()`.
    pub fn start(initial: Config) -> Result<Self> {
        let config_path = initial.paths.config_file();

        if !config_path.exists() {
            anyhow::bail!("Config file does not exist: {}", config_path.display());
        }

        let (tx, _) = watch::channel(initial.clone());
        let config = Arc::new(RwLock::new(initial.clone()));
        let last_reload = Arc::new(RwLock::new(Instant::now()));

        // Create the file watcher
        let config_clone = config.clone();
        let tx_clone = tx.clone();
        let config_path_clone = config_path.clone();
        let last_reload_clone = last_reload.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // Only handle modify events on the config file
                        if matches!(event.kind, EventKind::Modify(_))
                            && event.paths.iter().any(|p| p == &config_path_clone)
                        {
                            // Debounce: check if enough time has passed
                            let now = Instant::now();
                            let last = *last_reload_clone.blocking_read();

                            if now.duration_since(last) < DEBOUNCE_INTERVAL {
                                debug!("Config change debounced");
                                return;
                            }

                            // Reload config
                            match Config::load() {
                                Ok(new_config) => {
                                    info!("Config reloaded from {}", config_path_clone.display());
                                    *config_clone.blocking_write() = new_config.clone();
                                    if tx_clone.send(new_config).is_err() {
                                        debug!("No subscribers for config updates");
                                    }
                                    *last_reload_clone.blocking_write() = now;
                                }
                                Err(e) => {
                                    warn!("Failed to reload config: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Watch error: {}", e);
                    }
                }
            },
            notify::Config::default(),
        )?;

        // Watch the config file's parent directory (more reliable than watching the file itself)
        let watch_path = config_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| config_path.clone());

        watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

        debug!("Watching for config changes: {}", config_path.display());

        Ok(Self {
            config,
            tx,
            _watcher: watcher,
            config_path,
            last_reload,
        })
    }

    /// Get current config (latest after any hot-reload).
    pub async fn config(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Get current config synchronously.
    pub fn config_blocking(&self) -> Config {
        self.config.blocking_read().clone()
    }

    /// Subscribe to config change notifications.
    pub fn subscribe(&self) -> watch::Receiver<Config> {
        self.tx.subscribe()
    }

    /// Manually trigger a config reload.
    pub async fn reload(&self) -> Result<()> {
        let new_config = Config::load()?;
        *self.config.write().await = new_config.clone();
        let _ = self.tx.send(new_config);
        *self.last_reload.write().await = Instant::now();
        info!(
            "Config manually reloaded from {}",
            self.config_path.display()
        );
        Ok(())
    }

    /// Get the path to the config file being watched.
    pub fn config_path(&self) -> &std::path::Path {
        &self.config_path
    }
}

/// Spawn a task that handles SIGHUP signals to reload config.
///
/// On non-Unix platforms, this is a no-op.
#[cfg(unix)]
pub fn spawn_sighup_handler(watcher: Arc<ConfigWatcher>) -> tokio::task::JoinHandle<()> {
    use tokio::signal::unix::{SignalKind, signal};

    tokio::spawn(async move {
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to install SIGHUP handler: {}", e);
                return;
            }
        };

        loop {
            sighup.recv().await;
            info!("Received SIGHUP, reloading config...");
            if let Err(e) = watcher.reload().await {
                warn!("Failed to reload config on SIGHUP: {}", e);
            }
        }
    })
}

#[cfg(not(unix))]
pub fn spawn_sighup_handler(_watcher: Arc<ConfigWatcher>) -> tokio::task::JoinHandle<()> {
    // No-op on non-Unix platforms
    tokio::spawn(async {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_config_watcher_start() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Create a minimal config
        fs::write(
            &config_path,
            r#"[agent]
default_model = "claude-cli/opus"
"#,
        )
        .unwrap();

        // Create a config that points to the temp directory
        let config = Config {
            paths: Paths::from_root(temp_dir.path()),
            ..Default::default()
        };

        // This would require more setup to test properly
        // Just verify the types compile for now
        let _ = config;
    }
}
