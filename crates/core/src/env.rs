//! Environment variable constants used throughout the application.
//!
//! Centralized definition of all `LOCALGPT_*` environment variables to ensure
//! consistency and avoid hardcoded strings.

/// Config directory override (e.g. `~/.config/localgpt`)
pub const LOCALGPT_CONFIG_DIR: &str = "LOCALGPT_CONFIG_DIR";

/// Data directory override (e.g. `~/.local/share/localgpt`)
pub const LOCALGPT_DATA_DIR: &str = "LOCALGPT_DATA_DIR";

/// State directory override (e.g. `~/.local/state/localgpt`)
pub const LOCALGPT_STATE_DIR: &str = "LOCALGPT_STATE_DIR";

/// Cache directory override (e.g. `~/.cache/localgpt`)
pub const LOCALGPT_CACHE_DIR: &str = "LOCALGPT_CACHE_DIR";

/// Shared local-LLM model directory override (default
/// `~/.local/share/localgpt/models/llm`). Gen, MD and Verse all read the
/// GGUF from here so one download serves every app.
pub const LOCALGPT_LLM_DIR: &str = "LOCALGPT_LLM_DIR";

/// Workspace directory absolute override
pub const LOCALGPT_WORKSPACE: &str = "LOCALGPT_WORKSPACE";

/// Workspace profile override (creates `workspace-{profile}`)
pub const LOCALGPT_PROFILE: &str = "LOCALGPT_PROFILE";

/// Configuration file path override (CLI arg default env)
pub const LOCALGPT_CONFIG: &str = "LOCALGPT_CONFIG";

/// Agent ID override (CLI arg default env)
pub const LOCALGPT_AGENT: &str = "LOCALGPT_AGENT";

/// Gen: ComfyUI base URL for `gen_preview_world` (default `http://127.0.0.1:8188`)
pub const LOCALGPT_GEN_COMFYUI_URL: &str = "LOCALGPT_GEN_COMFYUI_URL";

/// Gen: checkpoint file name for the built-in ComfyUI preview workflow
pub const LOCALGPT_GEN_COMFYUI_CHECKPOINT: &str = "LOCALGPT_GEN_COMFYUI_CHECKPOINT";

/// Gen: depth ControlNet file name for the built-in ComfyUI preview workflow
pub const LOCALGPT_GEN_COMFYUI_CONTROLNET: &str = "LOCALGPT_GEN_COMFYUI_CONTROLNET";

/// Gen: path to a custom ComfyUI workflow (API format) for previews
pub const LOCALGPT_GEN_COMFYUI_WORKFLOW: &str = "LOCALGPT_GEN_COMFYUI_WORKFLOW";

/// Gen: asset model server base URL for `gen_generate_asset` /
/// `gen_generate_texture` (default `http://127.0.0.1:8741`)
pub const LOCALGPT_GEN_MODEL_SERVER: &str = "LOCALGPT_GEN_MODEL_SERVER";

/// Test URL for SearXNG integration tests
pub const LOCALGPT_TEST_SEARXNG_URL: &str = "LOCALGPT_TEST_SEARXNG_URL";

/// Internal: macOS sandbox profile passing (private)
pub const _LOCALGPT_SBPL_PROFILE: &str = "_LOCALGPT_SBPL_PROFILE";
