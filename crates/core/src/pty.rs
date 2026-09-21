//! The PTY host interface: what the daemon may ask of terminal sessions.
//!
//! The trait and its data live here, in the portable crate, so that the daemon
//! and the bridge can talk about PTY sessions without depending on a PTY
//! implementation. The implementation — which needs real platform APIs — lives
//! in `localgpt-cli-tools`, and the daemon injects it, the same way it injects
//! the dangerous tool set.
//!
//! Keeping the seam here is what makes it possible to move sessions into a
//! separately supervised process later without changing any caller: every method
//! below is one the daemon could equally issue over IPC.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Bytes of recent output retained per session for reattaching clients.
///
/// Sized for a few screens of a wide terminal. This is a replay buffer, not a
/// log: anything that needs to survive the session belongs on disk.
pub const DEFAULT_SCROLLBACK_BYTES: usize = 256 * 1024;

/// Identifier for a PTY session, unique within one registry.
pub type SessionId = String;

/// Whether a session's child process is still running.
///
/// Deliberately three-valued. A local PTY can usually give a definite answer,
/// but a host that has been moved out-of-process cannot always be reached, and
/// "we could not ask" must never be reported as "it exited" — that is what makes
/// a client discard a pane whose process is alive and still producing output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Liveness {
    /// The child is running.
    Live,
    /// The child exited, with this status.
    Exited { code: i32 },
    /// Contact with the host was lost. Says nothing about the child.
    Unverifiable,
}

/// A session's shape as reported to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtySessionInfo {
    pub id: SessionId,
    /// Argv as spawned, for display.
    pub command: Vec<String>,
    pub cwd: Option<String>,
    pub rows: u16,
    pub cols: u16,
    pub liveness: Liveness,
    /// Clients currently subscribed to the live stream.
    pub attached_clients: usize,
    /// Unix seconds at spawn.
    pub started_at: i64,
}

/// What a client gets when it attaches: the replay, then the live stream.
pub struct PtyAttachment {
    /// Output produced before this client attached, oldest first.
    pub scrollback: Vec<u8>,
    /// Output produced from the attach point onward.
    pub live: broadcast::Receiver<Arc<[u8]>>,
    pub info: PtySessionInfo,
}

/// Parameters for spawning a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtySpawnSpec {
    /// Program and arguments. Must be non-empty.
    pub command: Vec<String>,
    pub cwd: Option<String>,
    /// Extra environment entries layered over the daemon's own.
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default = "default_rows")]
    pub rows: u16,
    #[serde(default = "default_cols")]
    pub cols: u16,
}

fn default_rows() -> u16 {
    24
}
fn default_cols() -> u16 {
    80
}

/// The surface the daemon uses to reach PTY sessions.
///
/// Every method is one the daemon could equally issue over IPC to a separately
/// supervised host process, which is the point: nothing here assumes the
/// sessions live in this address space.
#[async_trait]
pub trait PtyHost: Send + Sync {
    async fn spawn(&self, spec: PtySpawnSpec) -> Result<PtySessionInfo>;
    async fn attach(&self, id: &str) -> Result<PtyAttachment>;
    /// Read output from `offset` onward without holding a live subscription.
    ///
    /// This is the form a remote client uses: request/response transports cannot
    /// carry [`PtyAttachment`]'s broadcast handle, and a cursor lets a client
    /// that reconnects resume exactly where it stopped.
    async fn read_from(&self, id: &str, offset: u64) -> Result<PtyReadSlice>;
    async fn write(&self, id: &str, data: &[u8]) -> Result<()>;
    async fn resize(&self, id: &str, rows: u16, cols: u16) -> Result<()>;
    async fn list(&self) -> Result<Vec<PtySessionInfo>>;
    async fn kill(&self, id: &str) -> Result<()>;
    /// Drop exited sessions from the registry. Returns the ids removed.
    async fn reap(&self) -> Result<Vec<SessionId>>;
}

/// Output a client missed, plus where to resume from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyReadSlice {
    /// Bytes from the requested offset onward.
    pub data: Vec<u8>,
    /// Offset to pass on the next read.
    pub next_offset: u64,
    /// True when the requested offset had already been evicted from the
    /// scrollback, so `data` starts later than the caller asked. Clients must
    /// repaint rather than append, and the flag is what tells them to.
    pub gap: bool,
}
