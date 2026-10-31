//! # LocalGPT Security Module
//!
//! Central security module for LocalGPT. This file is the **front door**
//! for security auditing — all security-critical types, constants, and
//! functions are re-exported here.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │                  localgpt.rs (you are here)                      │
//! │                  Public API facade & documentation               │
//! ├──────────────────────────────────────────────────────────────────┤
//! │  policy.rs — load + sanitize the POLICY.md policy pipeline       │
//! ├──────────────────────────────────────────────────────────────────┤
//! │  suffix.rs — Hardcoded security suffix (always last in context) │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! The protected-files deny list and the device key went with the shell and
//! file tools and the bridges they guarded: the agent has no tool that writes
//! a file outside its memory.
//!
//! ## Security Model
//!
//! 1. **Additive only**: The user's `POLICY.md` policy can tighten
//!    restrictions on top of the built-in safety rules. It cannot
//!    weaken or override the hardcoded security suffix.
//!
//! 2. **Sanitized before injection**: Policy content goes through the
//!    same injection defense pipeline as tool output. Suspicious
//!    content is rejected — the agent then runs with the hardcoded
//!    suffix only. The system never fails open.
//!
//! 3. **Recency-reinforced**: The hardcoded security suffix is always
//!    the last content in the context window, exploiting transformer
//!    recency bias to keep security rules in the model's high-attention
//!    zone even in long sessions.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use localgpt::security::{load_policy, build_ending_security_block};
//!
//! // At session start — load the policy (if any):
//! let user_policy = load_policy(&workspace_path);
//!
//! // At context assembly — always append the security block last:
//! let block = build_ending_security_block(user_policy.as_deref(), true);
//! ```
//!
//! ## File Hierarchy
//!
//! LocalGPT follows the XDG Base Directory Specification. Paths can be customized
//! via environment variables (see `paths.rs`).
//!
//! ```text
//! ~/.config/localgpt/                       # Config directory (XDG_CONFIG_HOME)
//! └── config.toml                           # User configuration
//!
//! ~/.local/share/localgpt/                  # Data directory (XDG_DATA_HOME)
//! └── workspace/                            # Memory workspace
//!     ├── POLICY.md                         # User security policy
//!     └── MEMORY.md                         # Long-term memory
//!
//! ~/.local/state/localgpt/                  # State directory (XDG_STATE_HOME)
//! ├── agents/{agent_id}/sessions/           # Session transcripts
//! └── logs/                                 # Application logs
//!
//! ~/.cache/localgpt/                        # Cache directory (XDG_CACHE_HOME)
//! └── memory/{agent_id}.sqlite              # Search index & embeddings
//! ```
//!
//! ## Threat Model
//!
//! | Threat | Defense Layer |
//! |--------|--------------|
//! | Injected content in policy file | Sanitization pipeline (blocking) |
//! | Policy weakens hardcoded rules | Hardcoded suffix always last in context |
//! | Policy floods context window | 4096 char limit |

// ── Policy Loading ──────────────────────────────────────────────────

pub use super::policy::{MAX_POLICY_CHARS, find_policy_file, load_policy, sanitize_policy_content};

// ── Context Window Suffix ───────────────────────────────────────────

pub use super::suffix::{HARDCODED_SECURITY_SUFFIX, build_ending_security_block};

// ── Constants ───────────────────────────────────────────────────────

/// The filename for the user-editable security policy.
///
/// This file lives in the workspace alongside `MEMORY.md` and the other
/// markdown files. It follows the same plain-markdown convention.
pub const POLICY_FILENAME: &str = "POLICY.md";

/// The pre-rename filename for the user security policy.
///
/// Workspaces created before the rename still load their `LocalGPT.md`.
pub const LEGACY_POLICY_FILENAME: &str = "LocalGPT.md";
