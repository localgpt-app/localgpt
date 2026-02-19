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
//! ├────────────────┬───────────────┬──────────┬─────────────────────┤
//! │  policy.rs     │  signing.rs   │ audit.rs │ protected_files.rs  │
//! │  Load, verify, │  Device key,  │ Append-  │ Agent write deny    │
//! │  sanitize      │  HMAC-SHA256, │ only log │ list for security-  │
//! │  pipeline      │  manifest     │ + hash   │ critical files      │
//! │                │               │ chain    │                     │
//! ├────────────────┴───────────────┴──────────┴─────────────────────┤
//! │  suffix.rs — Hardcoded security suffix (always last in context) │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Security Model
//!
//! 1. **Additive only**: The user's `LocalGPT.md` policy can tighten
//!    restrictions on top of the built-in safety rules. It cannot
//!    weaken or override the hardcoded security suffix.
//!
//! 2. **Fail closed**: Any verification failure (unsigned, tampered,
//!    corrupted manifest, suspicious content) causes the system to
//!    fall back to hardcoded-only security. The agent never operates
//!    with a compromised policy.
//!
//! 3. **Tamper-evident**: Policies are signed with HMAC-SHA256 using a
//!    device-local key stored outside the workspace. The manifest
//!    includes both a quick SHA-256 content hash and the HMAC. An
//!    append-only audit log with hash chaining records all security
//!    events.
//!
//! 4. **Agent-proof**: A protected files list blocks the agent from
//!    writing to `LocalGPT.md`, the manifest, the device key, and
//!    the audit log via `write_file`/`edit_file` tools. Bash commands
//!    get a best-effort heuristic check (true enforcement requires
//!    OS-level sandboxing).
//!
//! 5. **Recency-reinforced**: The hardcoded security suffix is always
//!    the last content in the context window, exploiting transformer
//!    recency bias to keep security rules in the model's high-attention
//!    zone even in long sessions.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use localgpt::security::{
//!     load_and_verify_policy, PolicyVerification,
//!     build_ending_security_block,
//!     append_audit_entry, AuditAction,
//! };
//!
//! // At session start — verify the policy:
//! let result = load_and_verify_policy(&workspace_path, &state_dir);
//! let user_policy = match result {
//!     PolicyVerification::Valid(content) => Some(content),
//!     PolicyVerification::TamperDetected => {
//!         eprintln!("⚠ LocalGPT.md tampered. Using defaults.");
//!         append_audit_entry(
//!             &state_dir, AuditAction::TamperDetected, "", "session_start"
//!         ).ok();
//!         None
//!     }
//!     _ => None,
//! };
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
//! ├── localgpt.device.key                   # 32-byte HMAC key (0600)
//! └── workspace/                            # Memory workspace
//!     ├── LocalGPT.md                       # User security policy
//!     ├── .localgpt_manifest.json           # HMAC signature manifest
//!     ├── MEMORY.md                         # Long-term memory
//!     └── HEARTBEAT.md                      # Autonomous tasks
//!
//! ~/.local/state/localgpt/                  # State directory (XDG_STATE_HOME)
//! ├── localgpt.audit.jsonl                  # Append-only audit log
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
//! | Agent writes to `LocalGPT.md` via tool | Protected files deny list |
//! | Agent writes via `bash` | Heuristic check + OS sandbox (separate) |
//! | Injected content in policy file | Sanitization pipeline (blocking) |
//! | Modified policy after signing | HMAC verification |
//! | Attacker modifies manifest too | HMAC requires device key (outside workspace) |
//! | Policy weakens hardcoded rules | Hardcoded suffix always last in context |
//! | Policy floods context window | 4096 char limit |
//! | Audit log tampered | Hash chain + state dir location |

// ── Policy Verification ─────────────────────────────────────────────

pub use super::policy::{
    MAX_POLICY_CHARS, PolicyVerification, load_and_verify_policy, sanitize_policy_content,
};

// ── Signing & Integrity ─────────────────────────────────────────────

pub use super::signing::{
    MANIFEST_FILENAME, Manifest, content_sha256, ensure_device_key, read_device_key, read_manifest,
    sign_policy, verify_signature,
};

// ── Audit Log ───────────────────────────────────────────────────────

pub use super::audit::{
    AuditAction, AuditEntry, append_audit_entry, append_audit_entry_with_detail, audit_file_path,
    read_audit_log, verify_audit_chain,
};

// ── Protected Files ─────────────────────────────────────────────────

pub use super::protected_files::{
    PROTECTED_EXTERNAL_PATHS, PROTECTED_FILES, check_bash_command, is_path_protected,
    is_workspace_file_protected,
};

// ── Context Window Suffix ───────────────────────────────────────────

pub use super::suffix::{HARDCODED_SECURITY_SUFFIX, build_ending_security_block};

// ── Constants ───────────────────────────────────────────────────────

/// The filename for the user-editable security policy.
///
/// This file lives in the workspace alongside `MEMORY.md`, `HEARTBEAT.md`,
/// and other markdown files. It follows the same plain-markdown convention.
pub const POLICY_FILENAME: &str = "LocalGPT.md";
