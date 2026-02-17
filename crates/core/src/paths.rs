//! XDG Base Directory Specification compliant path resolution.
//!
//! Every directory is resolved through a three-level fallback:
//! 1. LocalGPT-specific env var (LOCALGPT_CONFIG_DIR, etc.)
//! 2. XDG env var (XDG_CONFIG_HOME, etc.) via `etcetera`
//! 3. Platform default (~/.config, etc.)
//!
//! All paths are absolute. Relative paths from env vars are ignored per XDG spec.

use anyhow::{Context, Result};
#[cfg(unix)]
use libc::getuid;
use std::path::{Path, PathBuf};

/// Resolved directory paths for the entire application.
///
/// Created once at startup, threaded through Config.
/// All paths are absolute.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Config directory: config.toml lives here
    pub config_dir: PathBuf,

    /// Data directory root: contains workspace/ and localgpt.device.key
    pub data_dir: PathBuf,

    /// Workspace: markdown files, knowledge, skills.
    /// May be overridden independently via LOCALGPT_WORKSPACE.
    pub workspace: PathBuf,

    /// State directory: sessions, audit log, logs
    pub state_dir: PathBuf,

    /// Cache directory: search index, embedding models
    pub cache_dir: PathBuf,

    /// Runtime directory: PID file, sockets.
    /// None if no suitable runtime directory is available.
    pub runtime_dir: Option<PathBuf>,
}

impl Paths {
    /// Resolve all paths using real environment variables.
    pub fn resolve() -> Result<Self> {
        Self::resolve_with_env(|key| std::env::var(key))
    }

    /// Resolve paths with a custom env var lookup (for testing).
    pub fn resolve_with_env<F>(env_fn: F) -> Result<Self>
    where
        F: Fn(&str) -> std::result::Result<String, std::env::VarError>,
    {
        use etcetera::BaseStrategy;

        let strategy = etcetera::choose_base_strategy()
            .map_err(|e| anyhow::anyhow!("Failed to determine base directories: {}", e))?;

        let config_dir = env_or(&env_fn, "LOCALGPT_CONFIG_DIR", || {
            strategy.config_dir().join("localgpt")
        });

        let data_dir = env_or(&env_fn, "LOCALGPT_DATA_DIR", || {
            strategy.data_dir().join("localgpt")
        });

        let state_dir = env_or(&env_fn, "LOCALGPT_STATE_DIR", || {
            // etcetera's choose_base_strategy gives XDG paths on all platforms.
            // state_dir() returns data_dir() as fallback on platforms without XDG_STATE_HOME.
            let base_state = strategy.state_dir().unwrap_or_else(|| strategy.data_dir());
            base_state.join("localgpt")
        });

        let cache_dir = env_or(&env_fn, "LOCALGPT_CACHE_DIR", || {
            strategy.cache_dir().join("localgpt")
        });

        // Workspace: independent override, then LOCALGPT_PROFILE, then default under data_dir
        let workspace = resolve_workspace(&env_fn, &data_dir);

        // Runtime: XDG_RUNTIME_DIR or platform fallback
        let runtime_dir = resolve_runtime_dir(&env_fn);

        Ok(Self {
            config_dir,
            data_dir,
            workspace,
            state_dir,
            cache_dir,
            runtime_dir,
        })
    }

    // ── Convenience accessors for specific files ──

    /// Config file: config_dir/config.toml
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Device key: data_dir/localgpt.device.key
    pub fn device_key(&self) -> PathBuf {
        self.data_dir.join("localgpt.device.key")
    }

    /// Audit log: state_dir/localgpt.audit.jsonl
    pub fn audit_log(&self) -> PathBuf {
        self.state_dir.join("localgpt.audit.jsonl")
    }

    pub fn last_heartbeat(&self) -> PathBuf {
        self.state_dir.join("last_heartbeat")
    }

    /// Search index for a specific agent: cache_dir/memory/{agent_id}.sqlite
    pub fn search_index(&self, agent_id: &str) -> PathBuf {
        self.cache_dir
            .join("memory")
            .join(format!("{}.sqlite", agent_id))
    }

    /// Sessions directory for a specific agent
    pub fn sessions_dir(&self, agent_id: &str) -> PathBuf {
        self.state_dir
            .join("agents")
            .join(agent_id)
            .join("sessions")
    }

    /// Logs directory
    pub fn logs_dir(&self) -> PathBuf {
        self.state_dir.join("logs")
    }

    /// PID file (in runtime_dir, falls back to state_dir)
    pub fn pid_file(&self) -> PathBuf {
        self.runtime_dir
            .as_ref()
            .unwrap_or(&self.state_dir)
            .join("daemon.pid")
    }

    /// Workspace lock file (in runtime_dir, falls back to state_dir)
    pub fn workspace_lock(&self) -> PathBuf {
        self.runtime_dir
            .as_ref()
            .unwrap_or(&self.state_dir)
            .join("workspace.lock")
    }

    /// Telegram pairing file
    pub fn pairing_file(&self) -> PathBuf {
        self.state_dir.join("telegram_paired_user.json")
    }

    /// Managed skills directory: data_dir/skills
    pub fn managed_skills_dir(&self) -> PathBuf {
        self.data_dir.join("skills")
    }

    /// Embedding cache directory: cache_dir/embeddings
    pub fn embedding_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("embeddings")
    }

    /// Create Paths with all directories rooted under a single base path.
    ///
    /// Mobile apps use this to point everything at their app-specific
    /// document or library directory.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            workspace: root.join("data").join("workspace"),
            state_dir: root.join("state"),
            cache_dir: root.join("cache"),
            runtime_dir: None,
        }
    }

    /// Create all directories with appropriate permissions.
    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            &self.config_dir,
            &self.data_dir,
            &self.state_dir,
            &self.cache_dir,
        ];

        for dir in &dirs {
            create_dir_with_mode(dir)?;
        }

        if let Some(ref runtime) = self.runtime_dir {
            create_dir_with_mode(runtime)?;
        }

        Ok(())
    }
}

impl Default for Paths {
    fn default() -> Self {
        Self::resolve().unwrap_or_else(|_| {
            // Emergency fallback — should never happen in practice
            let home = etcetera::home_dir().unwrap_or_else(|_| PathBuf::from("."));
            Self {
                config_dir: home.join(".config").join("localgpt"),
                data_dir: home.join(".local").join("share").join("localgpt"),
                workspace: home
                    .join(".local")
                    .join("share")
                    .join("localgpt")
                    .join("workspace"),
                state_dir: home.join(".local").join("state").join("localgpt"),
                cache_dir: home.join(".cache").join("localgpt"),
                runtime_dir: None,
            }
        })
    }
}

/// Resolve an env var with fallback. Ignores empty and relative paths per XDG spec.
fn env_or<F>(env_fn: &F, var: &str, default: impl FnOnce() -> PathBuf) -> PathBuf
where
    F: Fn(&str) -> std::result::Result<String, std::env::VarError>,
{
    env_fn(var)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute()) // XDG spec: ignore relative paths
        .unwrap_or_else(default)
}

/// Resolve workspace path with LOCALGPT_WORKSPACE, LOCALGPT_PROFILE, or default.
fn resolve_workspace<F>(env_fn: &F, data_dir: &Path) -> PathBuf
where
    F: Fn(&str) -> std::result::Result<String, std::env::VarError>,
{
    // Direct workspace override
    if let Ok(ws) = env_fn("LOCALGPT_WORKSPACE") {
        let trimmed = ws.trim();
        if !trimmed.is_empty() {
            let expanded = shellexpand::tilde(trimmed);
            let path = PathBuf::from(expanded.to_string());
            if path.is_absolute() {
                return path;
            }
        }
    }

    // Profile-based workspace
    if let Ok(profile) = env_fn("LOCALGPT_PROFILE") {
        let trimmed = profile.trim().to_lowercase();
        if !trimmed.is_empty() && trimmed != "default" {
            return data_dir.join(format!("workspace-{}", trimmed));
        }
    }

    data_dir.join("workspace")
}

/// Resolve runtime directory.
fn resolve_runtime_dir<F>(env_fn: &F) -> Option<PathBuf>
where
    F: Fn(&str) -> std::result::Result<String, std::env::VarError>,
{
    // Try XDG_RUNTIME_DIR first
    if let Ok(dir) = env_fn("XDG_RUNTIME_DIR")
        && !dir.is_empty()
    {
        let path = PathBuf::from(&dir);
        if path.is_absolute() {
            return Some(path.join("localgpt"));
        }
    }

    // Fallback: $TMPDIR/localgpt-$UID on Unix
    #[cfg(unix)]
    {
        let uid = unsafe { getuid() };
        let tmpdir = env_fn("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        Some(PathBuf::from(tmpdir).join(format!("localgpt-{}", uid)))
    }

    #[cfg(not(unix))]
    {
        env_fn("TEMP").ok().map(|t| {
            let user = env_fn("USERNAME").unwrap_or_else(|_| "user".into());
            PathBuf::from(t).join(format!("localgpt-{}", user))
        })
    }
}

/// Create a directory with mode 0700 per XDG spec.
fn create_dir_with_mode(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("Failed to create directory: {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Helper: build an env_fn from a HashMap
    fn make_env(
        map: HashMap<&str, &str>,
    ) -> impl Fn(&str) -> std::result::Result<String, std::env::VarError> {
        move |key: &str| {
            map.get(key)
                .map(|v| v.to_string())
                .ok_or(std::env::VarError::NotPresent)
        }
    }

    #[test]
    fn default_paths_are_xdg_compliant() {
        let env: HashMap<&str, &str> = HashMap::new();
        let paths = Paths::resolve_with_env(make_env(env)).unwrap();

        // Should end with the expected XDG suffixes
        assert!(
            paths.config_dir.ends_with("localgpt"),
            "config_dir: {:?}",
            paths.config_dir
        );
        assert!(
            paths.data_dir.ends_with("localgpt"),
            "data_dir: {:?}",
            paths.data_dir
        );
        assert!(
            paths.state_dir.ends_with("localgpt"),
            "state_dir: {:?}",
            paths.state_dir
        );
        assert!(
            paths.cache_dir.ends_with("localgpt"),
            "cache_dir: {:?}",
            paths.cache_dir
        );
        assert!(paths.workspace.ends_with("workspace"));
    }

    #[test]
    fn localgpt_env_vars_override_xdg() {
        let mut env: HashMap<&str, &str> = HashMap::new();
        env.insert("LOCALGPT_CONFIG_DIR", "/custom/config");
        env.insert("LOCALGPT_DATA_DIR", "/custom/data");
        env.insert("LOCALGPT_STATE_DIR", "/custom/state");
        env.insert("LOCALGPT_CACHE_DIR", "/custom/cache");

        let paths = Paths::resolve_with_env(make_env(env)).unwrap();
        assert_eq!(paths.config_dir, PathBuf::from("/custom/config"));
        assert_eq!(paths.data_dir, PathBuf::from("/custom/data"));
        assert_eq!(paths.state_dir, PathBuf::from("/custom/state"));
        assert_eq!(paths.cache_dir, PathBuf::from("/custom/cache"));
    }

    #[test]
    fn relative_paths_are_ignored() {
        let mut env: HashMap<&str, &str> = HashMap::new();
        env.insert("LOCALGPT_CONFIG_DIR", "relative/path");

        let paths = Paths::resolve_with_env(make_env(env)).unwrap();
        // Should fall back to XDG default, not use relative path
        assert!(paths.config_dir.is_absolute());
        assert_ne!(paths.config_dir, PathBuf::from("relative/path"));
    }

    #[test]
    fn workspace_override_independent_of_data_dir() {
        let mut env: HashMap<&str, &str> = HashMap::new();
        env.insert("LOCALGPT_WORKSPACE", "/projects/my-workspace");

        let paths = Paths::resolve_with_env(make_env(env)).unwrap();
        assert_eq!(paths.workspace, PathBuf::from("/projects/my-workspace"));
        // data_dir should still be at XDG default (not derived from workspace)
        assert!(paths.data_dir.ends_with("localgpt"));
        assert!(!paths.data_dir.to_string_lossy().contains("my-workspace"));
    }

    #[test]
    fn profile_creates_named_workspace() {
        let mut env: HashMap<&str, &str> = HashMap::new();
        env.insert("LOCALGPT_PROFILE", "work");

        let paths = Paths::resolve_with_env(make_env(env)).unwrap();
        assert!(
            paths.workspace.ends_with("workspace-work"),
            "workspace: {:?}",
            paths.workspace
        );
    }

    #[test]
    fn convenience_accessors() {
        let env: HashMap<&str, &str> = HashMap::new();
        let paths = Paths::resolve_with_env(make_env(env)).unwrap();

        assert!(paths.config_file().ends_with("config.toml"));
        assert!(paths.device_key().ends_with("localgpt.device.key"));
        assert!(paths.audit_log().ends_with("localgpt.audit.jsonl"));
        assert!(paths.search_index("main").ends_with("memory/main.sqlite"));
        assert!(paths.sessions_dir("main").ends_with("agents/main/sessions"));
        assert!(paths.logs_dir().ends_with("logs"));
        assert!(paths.managed_skills_dir().ends_with("skills"));
        assert!(paths.embedding_cache_dir().ends_with("embeddings"));
        assert!(paths.pairing_file().ends_with("telegram_paired_user.json"));
    }

    #[test]
    fn empty_env_vars_ignored() {
        let mut env: HashMap<&str, &str> = HashMap::new();
        env.insert("LOCALGPT_CONFIG_DIR", "");

        let paths = Paths::resolve_with_env(make_env(env)).unwrap();
        // Should use XDG default, not empty string
        assert!(paths.config_dir.is_absolute());
        assert!(paths.config_dir.ends_with("localgpt"));
    }
}
