//! The `LOCALGPT_GEN_*` environment variables Gen's external services read.
//!
//! These name servers Gen talks to for a world's own sake — an image model
//! for previews, a mesh model for assets, a small language model for NPC
//! brains — and nothing else: Gen holds no model of its own and no API key.

/// ComfyUI base URL for `gen_preview_world` (default `http://127.0.0.1:8188`).
pub const LOCALGPT_GEN_COMFYUI_URL: &str = "LOCALGPT_GEN_COMFYUI_URL";

/// Checkpoint file name for the built-in ComfyUI preview workflow.
pub const LOCALGPT_GEN_COMFYUI_CHECKPOINT: &str = "LOCALGPT_GEN_COMFYUI_CHECKPOINT";

/// Depth ControlNet file name for the built-in ComfyUI preview workflow.
pub const LOCALGPT_GEN_COMFYUI_CONTROLNET: &str = "LOCALGPT_GEN_COMFYUI_CONTROLNET";

/// Path to a custom ComfyUI workflow (API format) for previews.
pub const LOCALGPT_GEN_COMFYUI_WORKFLOW: &str = "LOCALGPT_GEN_COMFYUI_WORKFLOW";

/// Asset model server base URL for `gen_generate_asset` and
/// `gen_generate_texture` (default `http://127.0.0.1:8741`).
pub const LOCALGPT_GEN_MODEL_SERVER: &str = "LOCALGPT_GEN_MODEL_SERVER";

/// Ollama base URL for NPC brains (default `http://localhost:11434`).
pub const LOCALGPT_GEN_OLLAMA_URL: &str = "LOCALGPT_GEN_OLLAMA_URL";
