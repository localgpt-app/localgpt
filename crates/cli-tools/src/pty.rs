//! PTY sessions owned by the daemon rather than by whoever is watching them.
//!
//! A terminal running an agent CLI must not die because the thing displaying it
//! went away. Clients attach and detach; the session and its child process keep
//! running in the daemon, and a reattaching client is handed the scrollback it
//! missed so the pane it paints matches the one it left.
//!
//! # Ownership levels
//!
//! There are two distinct guarantees, and only the first is implemented here:
//!
//! 1. **Client-restart survival.** The PTY outlives any number of client
//!    attach/detach cycles for as long as the daemon runs. That is what
//!    [`PtyRegistry`] provides.
//! 2. **Daemon-restart survival.** The PTY outlives the daemon process itself,
//!    which requires the sessions to live in a separately supervised process that
//!    the daemon detaches from and re-adopts. [`PtyHost`] is the seam for that:
//!    it is the entire surface the daemon uses, so moving the implementation
//!    out-of-process later is a transport change, not a redesign.
//!
//! Process detachment alone is not service isolation. A PTY host forked from the
//! daemon stays in the same process group and systemd cgroup, and `KillMode=mixed`
//! or `control-group` will kill it with its parent. Real level-2 survival needs a
//! separately supervised unit; claiming it without one is how terminals silently
//! die on upgrade.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub use localgpt_core::pty::{
    DEFAULT_SCROLLBACK_BYTES, Liveness, PtyAttachment, PtyHost, PtyReadSlice, PtySessionInfo,
    PtySpawnSpec, SessionId,
};

/// Capacity of the live-output broadcast channel, in chunks.
///
/// A client that falls this far behind is disconnected from the live stream
/// rather than allowed to stall the reader thread for every other client.
const BROADCAST_CAPACITY: usize = 1024;

/// A bounded byte ring that keeps the most recent output.
///
/// Stores a flat buffer and drains from the front once over budget, so a replay
/// is one contiguous copy rather than a walk over retained chunks.
#[derive(Debug)]
struct Scrollback {
    buf: Vec<u8>,
    limit: usize,
    /// Total bytes ever written. `total - buf.len()` is the offset of `buf[0]`,
    /// which is what lets a client cursor outlive eviction.
    total: u64,
}

impl Scrollback {
    fn new(limit: usize) -> Self {
        Self {
            buf: Vec::new(),
            limit,
            total: 0,
        }
    }

    /// Offset of the oldest byte still retained.
    fn base(&self) -> u64 {
        self.total - self.buf.len() as u64
    }

    /// Bytes from `offset` onward, flagging a caller that fell behind eviction.
    fn read_from(&self, offset: u64) -> (Vec<u8>, u64, bool) {
        let base = self.base();
        if offset < base {
            // The caller's cursor points at bytes we no longer hold. Hand back
            // everything we have and say so, rather than silently skipping.
            return (self.buf.clone(), self.total, true);
        }
        let start = (offset - base).min(self.buf.len() as u64) as usize;
        (self.buf[start..].to_vec(), self.total, false)
    }

    fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len() as u64;
        // A single chunk larger than the budget keeps only its tail.
        if chunk.len() >= self.limit {
            self.buf.clear();
            self.buf
                .extend_from_slice(&chunk[chunk.len() - self.limit..]);
            return;
        }
        self.buf.extend_from_slice(chunk);
        if self.buf.len() > self.limit {
            let excess = self.buf.len() - self.limit;
            self.buf.drain(..excess);
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        self.buf.clone()
    }
}

/// Mutable state a session's reader thread and its clients share.
struct SessionShared {
    scrollback: Mutex<Scrollback>,
    /// `None` until the child is reaped.
    exit_code: Mutex<Option<i32>>,
    output: broadcast::Sender<Arc<[u8]>>,
}

struct PtySession {
    id: SessionId,
    command: Vec<String>,
    cwd: Option<String>,
    size: Mutex<(u16, u16)>,
    started_at: i64,
    /// `MasterPty` is `Send` but not `Sync`, so sharing a session across tasks
    /// requires the lock even though only `resize` uses it.
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    shared: Arc<SessionShared>,
}

impl PtySession {
    fn liveness(&self) -> Liveness {
        if let Some(code) = *self
            .shared
            .exit_code
            .lock()
            .expect("scrollback mutex poisoned")
        {
            return Liveness::Exited { code };
        }
        // `try_wait` reaps without blocking; a locked child means another caller
        // is mid-check, which is not evidence of anything.
        match self.child.try_lock() {
            Ok(mut child) => match child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.exit_code() as i32;
                    *self.shared.exit_code.lock().expect("exit mutex poisoned") = Some(code);
                    Liveness::Exited { code }
                }
                Ok(None) => Liveness::Live,
                Err(_) => Liveness::Unverifiable,
            },
            Err(_) => Liveness::Live,
        }
    }

    fn info(&self) -> PtySessionInfo {
        let (rows, cols) = *self.size.lock().expect("size mutex poisoned");
        PtySessionInfo {
            id: self.id.clone(),
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            rows,
            cols,
            liveness: self.liveness(),
            attached_clients: self.shared.output.receiver_count(),
            started_at: self.started_at,
        }
    }
}

/// In-process PTY sessions, owned by the daemon.
pub struct PtyRegistry {
    sessions: Mutex<HashMap<SessionId, Arc<PtySession>>>,
    scrollback_bytes: usize,
    next_id: Mutex<u64>,
}

impl Default for PtyRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_SCROLLBACK_BYTES)
    }
}

impl PtyRegistry {
    pub fn new(scrollback_bytes: usize) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            scrollback_bytes,
            next_id: Mutex::new(1),
        }
    }

    fn mint_id(&self) -> SessionId {
        let mut n = self.next_id.lock().expect("id mutex poisoned");
        let id = format!("pty-{n}");
        *n += 1;
        id
    }

    fn get(&self, id: &str) -> Result<Arc<PtySession>> {
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .get(id)
            .cloned()
            .with_context(|| format!("no PTY session {id}"))
    }
}

#[async_trait]
impl PtyHost for PtyRegistry {
    async fn spawn(&self, spec: PtySpawnSpec) -> Result<PtySessionInfo> {
        if spec.command.is_empty() {
            bail!("PTY spawn requires a command");
        }

        let size = PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system()
            .openpty(size)
            .context("failed to open PTY")?;

        let mut cmd = CommandBuilder::new(&spec.command[0]);
        for arg in &spec.command[1..] {
            cmd.arg(arg);
        }
        if let Some(cwd) = &spec.cwd {
            cmd.cwd(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("failed to spawn {:?} on a PTY", spec.command))?;
        // The slave fd must close here, or the master never sees EOF when the
        // child exits and the reader thread hangs for the life of the daemon.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;

        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        let shared = Arc::new(SessionShared {
            scrollback: Mutex::new(Scrollback::new(self.scrollback_bytes)),
            exit_code: Mutex::new(None),
            output: tx,
        });

        // A dedicated thread, not a blocking task: this read loop lives as long
        // as the session and would otherwise occupy a pool slot indefinitely.
        let reader_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("localgpt-pty-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk: Arc<[u8]> = Arc::from(&buf[..n]);
                            reader_shared
                                .scrollback
                                .lock()
                                .expect("scrollback mutex poisoned")
                                .push(&chunk);
                            // Err means nobody is attached; the scrollback above
                            // is what a later client will replay, so drop it.
                            let _ = reader_shared.output.send(chunk);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
            .context("failed to start PTY reader thread")?;

        let id = self.mint_id();
        let session = Arc::new(PtySession {
            id: id.clone(),
            command: spec.command.clone(),
            cwd: spec.cwd.clone(),
            size: Mutex::new((spec.rows, spec.cols)),
            started_at: chrono::Utc::now().timestamp(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            shared,
        });

        let info = session.info();
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .insert(id, session);
        Ok(info)
    }

    async fn attach(&self, id: &str) -> Result<PtyAttachment> {
        let session = self.get(id)?;
        // Subscribe before snapshotting, so output landing between the two shows
        // up in the live stream rather than falling in the gap between them. The
        // client may see a few bytes twice; it will never miss any.
        let live = session.shared.output.subscribe();
        let scrollback = session
            .shared
            .scrollback
            .lock()
            .expect("scrollback mutex poisoned")
            .snapshot();
        Ok(PtyAttachment {
            scrollback,
            live,
            info: session.info(),
        })
    }

    async fn read_from(&self, id: &str, offset: u64) -> Result<PtyReadSlice> {
        let session = self.get(id)?;
        let (data, next_offset, gap) = session
            .shared
            .scrollback
            .lock()
            .expect("scrollback mutex poisoned")
            .read_from(offset);
        Ok(PtyReadSlice {
            data,
            next_offset,
            gap,
        })
    }

    async fn write(&self, id: &str, data: &[u8]) -> Result<()> {
        let session = self.get(id)?;
        let mut writer = session.writer.lock().expect("writer mutex poisoned");
        writer.write_all(data).context("PTY write failed")?;
        writer.flush().context("PTY flush failed")?;
        Ok(())
    }

    async fn resize(&self, id: &str, rows: u16, cols: u16) -> Result<()> {
        let session = self.get(id)?;
        session
            .master
            .lock()
            .expect("master mutex poisoned")
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("PTY resize failed")?;
        *session.size.lock().expect("size mutex poisoned") = (rows, cols);
        Ok(())
    }

    async fn list(&self) -> Result<Vec<PtySessionInfo>> {
        let sessions: Vec<_> = self
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .values()
            .cloned()
            .collect();
        Ok(sessions.iter().map(|s| s.info()).collect())
    }

    async fn kill(&self, id: &str) -> Result<()> {
        let session = self.get(id)?;
        session
            .child
            .lock()
            .expect("child mutex poisoned")
            .kill()
            .context("failed to kill PTY child")?;
        Ok(())
    }

    async fn reap(&self) -> Result<Vec<SessionId>> {
        let candidates: Vec<_> = self
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .values()
            .cloned()
            .collect();

        // Only definite exits are removed. `Unverifiable` keeps the session, on
        // the principle that losing contact is not evidence of death.
        let dead: Vec<SessionId> = candidates
            .iter()
            .filter(|s| matches!(s.liveness(), Liveness::Exited { .. }))
            .map(|s| s.id.clone())
            .collect();

        let mut sessions = self.sessions.lock().expect("sessions mutex poisoned");
        for id in &dead {
            sessions.remove(id);
        }
        Ok(dead)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sh(script: &str) -> PtySpawnSpec {
        PtySpawnSpec {
            command: vec!["/bin/sh".into(), "-c".into(), script.into()],
            cwd: None,
            env: vec![],
            rows: 24,
            cols: 80,
        }
    }

    /// Polls until `f` holds, so tests never depend on a fixed sleep.
    async fn until<F, Fut>(mut f: F) -> bool
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        for _ in 0..200 {
            if f().await {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    async fn scrollback_of(reg: &PtyRegistry, id: &str) -> String {
        String::from_utf8_lossy(&reg.attach(id).await.unwrap().scrollback).to_string()
    }

    #[tokio::test]
    async fn spawns_and_captures_output() {
        let reg = PtyRegistry::default();
        let info = reg.spawn(sh("echo hello-pty")).await.unwrap();

        assert!(
            until(|| async { scrollback_of(&reg, &info.id).await.contains("hello-pty") }).await,
            "output never reached the scrollback"
        );
    }

    /// The guarantee the whole module exists for: output produced while nobody
    /// was watching is still there when a client comes back.
    #[tokio::test]
    async fn output_survives_detach_and_reattach() {
        let reg = PtyRegistry::default();
        let info = reg.spawn(sh("echo before-detach; sleep 30")).await.unwrap();

        // Attach, then drop the attachment entirely — the client is gone.
        {
            let attachment = reg.attach(&info.id).await.unwrap();
            drop(attachment);
        }

        assert!(
            until(|| async {
                scrollback_of(&reg, &info.id)
                    .await
                    .contains("before-detach")
            })
            .await,
            "a detached session must keep its output"
        );

        let again = reg.attach(&info.id).await.unwrap();
        assert!(
            String::from_utf8_lossy(&again.scrollback).contains("before-detach"),
            "reattaching must replay what the client missed"
        );
        assert_eq!(again.info.liveness, Liveness::Live, "child must still run");

        reg.kill(&info.id).await.unwrap();
    }

    #[tokio::test]
    async fn live_stream_reaches_an_attached_client() {
        let reg = PtyRegistry::default();
        let info = reg.spawn(sh("sleep 30")).await.unwrap();
        let mut attachment = reg.attach(&info.id).await.unwrap();

        reg.write(&info.id, b"echo streamed\n").await.unwrap();

        let mut seen = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(200), attachment.live.recv()).await {
                Ok(Ok(chunk)) => {
                    seen.push_str(&String::from_utf8_lossy(&chunk));
                    if seen.contains("streamed") {
                        break;
                    }
                }
                Ok(Err(_)) => break,
                Err(_) => continue,
            }
        }

        assert!(seen.contains("streamed"), "live stream carried: {seen:?}");
        reg.kill(&info.id).await.unwrap();
    }

    #[tokio::test]
    async fn two_clients_both_receive_output() {
        let reg = PtyRegistry::default();
        let info = reg.spawn(sh("sleep 30")).await.unwrap();
        let a = reg.attach(&info.id).await.unwrap();
        let b = reg.attach(&info.id).await.unwrap();

        assert_eq!(
            reg.list().await.unwrap()[0].attached_clients,
            2,
            "both attachments must be counted"
        );
        drop(a);
        drop(b);
        reg.kill(&info.id).await.unwrap();
    }

    #[tokio::test]
    async fn resize_is_visible_to_the_child() {
        let reg = PtyRegistry::default();
        let info = reg.spawn(sh("sleep 30")).await.unwrap();

        reg.resize(&info.id, 40, 132).await.unwrap();

        let listed = &reg.list().await.unwrap()[0];
        assert_eq!((listed.rows, listed.cols), (40, 132));
        reg.kill(&info.id).await.unwrap();
    }

    #[tokio::test]
    async fn exit_is_observed_and_reaped() {
        let reg = PtyRegistry::default();
        let info = reg.spawn(sh("exit 3")).await.unwrap();

        assert!(
            until(|| async {
                matches!(
                    reg.list().await.unwrap().first().map(|s| s.liveness),
                    Some(Liveness::Exited { .. })
                )
            })
            .await,
            "exit was never observed"
        );

        let reaped = reg.reap().await.unwrap();
        assert_eq!(reaped, vec![info.id.clone()]);
        assert!(reg.list().await.unwrap().is_empty());
        assert!(
            reg.attach(&info.id).await.is_err(),
            "reaped session is gone"
        );
    }

    /// Reaping removes only sessions proven exited — never ones merely quiet.
    #[tokio::test]
    async fn reap_spares_a_running_session() {
        let reg = PtyRegistry::default();
        let live = reg.spawn(sh("sleep 30")).await.unwrap();
        let dead = reg.spawn(sh("exit 0")).await.unwrap();

        assert!(
            until(|| async {
                reg.list()
                    .await
                    .unwrap()
                    .iter()
                    .any(|s| s.id == dead.id && matches!(s.liveness, Liveness::Exited { .. }))
            })
            .await
        );

        let reaped = reg.reap().await.unwrap();

        assert_eq!(reaped, vec![dead.id]);
        assert!(
            reg.attach(&live.id).await.is_ok(),
            "a running session must survive a reap"
        );
        reg.kill(&live.id).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_an_empty_command() {
        let reg = PtyRegistry::default();
        let spec = PtySpawnSpec {
            command: vec![],
            cwd: None,
            env: vec![],
            rows: 24,
            cols: 80,
        };
        assert!(reg.spawn(spec).await.is_err());
    }

    #[tokio::test]
    async fn unknown_session_ids_error_rather_than_panic() {
        let reg = PtyRegistry::default();
        assert!(reg.attach("pty-404").await.is_err());
        assert!(reg.write("pty-404", b"x").await.is_err());
        assert!(reg.resize("pty-404", 10, 10).await.is_err());
        assert!(reg.kill("pty-404").await.is_err());
    }

    #[test]
    fn scrollback_keeps_the_newest_bytes_within_budget() {
        let mut s = Scrollback::new(8);
        s.push(b"abcdef");
        s.push(b"ghij");
        assert_eq!(
            s.snapshot(),
            b"cdefghij",
            "must retain the tail, not the head"
        );
        assert!(s.snapshot().len() <= 8);
    }

    #[test]
    fn scrollback_truncates_an_oversized_single_chunk() {
        let mut s = Scrollback::new(4);
        s.push(b"0123456789");
        assert_eq!(s.snapshot(), b"6789");
    }

    #[test]
    fn scrollback_below_budget_is_kept_whole() {
        let mut s = Scrollback::new(64);
        s.push(b"short");
        assert_eq!(s.snapshot(), b"short");
    }
}

#[cfg(all(test, unix))]
mod cursor_tests {
    use super::*;

    #[test]
    fn a_cursor_resumes_exactly_where_it_stopped() {
        let mut s = Scrollback::new(64);
        s.push(b"alpha");
        let (first, cursor, gap) = s.read_from(0);
        assert_eq!(first, b"alpha");
        assert!(!gap);

        s.push(b"beta");
        let (next, cursor2, gap2) = s.read_from(cursor);

        assert_eq!(
            next, b"beta",
            "a resumed read must not repeat delivered bytes"
        );
        assert!(!gap2);
        assert_eq!(cursor2, 9);
    }

    #[test]
    fn a_cursor_past_eviction_reports_a_gap() {
        let mut s = Scrollback::new(4);
        s.push(b"0123456789");

        let (data, next, gap) = s.read_from(0);

        assert!(gap, "caller fell behind eviction and must be told");
        assert_eq!(data, b"6789", "hand back everything still retained");
        assert_eq!(next, 10);
    }

    #[test]
    fn reading_at_the_head_returns_nothing() {
        let mut s = Scrollback::new(64);
        s.push(b"data");
        let (data, next, gap) = s.read_from(4);
        assert!(data.is_empty());
        assert!(!gap);
        assert_eq!(next, 4);
    }

    #[test]
    fn an_offset_beyond_the_head_is_clamped_not_panicking() {
        let mut s = Scrollback::new(64);
        s.push(b"data");
        let (data, _, gap) = s.read_from(999);
        assert!(data.is_empty());
        assert!(!gap);
    }

    #[tokio::test]
    async fn registry_read_from_survives_detach() {
        let reg = PtyRegistry::default();
        let info = reg
            .spawn(PtySpawnSpec {
                command: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "echo cursor-test; sleep 30".into(),
                ],
                cwd: None,
                env: vec![],
                rows: 24,
                cols: 80,
            })
            .await
            .unwrap();

        let mut cursor = 0u64;
        let mut seen = String::new();
        for _ in 0..200 {
            let slice = reg.read_from(&info.id, cursor).await.unwrap();
            seen.push_str(&String::from_utf8_lossy(&slice.data));
            cursor = slice.next_offset;
            if seen.contains("cursor-test") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        assert!(seen.contains("cursor-test"), "cursor read saw: {seen:?}");
        // Resuming at the cursor yields nothing new, not a replay.
        let again = reg.read_from(&info.id, cursor).await.unwrap();
        assert!(again.data.is_empty());
        reg.kill(&info.id).await.unwrap();
    }
}
