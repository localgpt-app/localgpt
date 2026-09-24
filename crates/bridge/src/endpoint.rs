//! Endpoint ownership: who may mutate the bridge socket's directory entry.
//!
//! Two invariants govern the canonical socket path:
//!
//! > Only a daemon publishing itself onto the canonical endpoint may mutate that
//! > directory entry, and only by replacing an entry it has itself just proven dead.
//!
//! > No actor removes a name it did not create.
//!
//! The naive shape — `remove_file(path)` then bind — deletes whichever socket
//! happens to sit at the path, including a live daemon's. That daemon stays alive
//! serving sessions no client can reach, which reads to the user as a bridge that
//! accepts connections and never answers. The same hazard exists on the way out:
//! a listener that unlinks its path on drop deletes its *replacement's* entry.
//!
//! The protocol here is: bind a private `.p<hex>` name -> try an exclusive `link`
//! -> on `EEXIST` prove the incumbent dead by connecting -> re-check the entry has
//! not changed hands -> probe once more -> `rename` in one syscall -> verify we
//! kept it.
//!
//! Traps this design deliberately avoids:
//!
//! - **"Can't tell" is never "dead."** Only a refused connection or a missing
//!   entry proves death. A timeout or `EPERM` proves nothing and must decline.
//! - **`link` first, never an unconditional `rename`.** `rename` replaces whatever
//!   it finds, so it would let a starting daemon destroy a healthy one.
//! - **`rename`, never `unlink`-then-`link`.** The latter leaves the name absent
//!   between two calls, which is a window where clients see no endpoint at all.
//! - **Entries are identified by `(dev, ino)`, never by creation time.** Birth time
//!   is unavailable or coarse on many filesystems.
//! - **No sweeper.** Deciding whether someone else's leftover is safe to delete is
//!   the question this design retires. Every actor removes its own scratch name.
//! - **Never unlink the endpoint on shutdown.** A departing daemon leaves a dead
//!   entry; the next publisher replaces it in one rename.
//!
//! Windows named pipes have no directory entry, so none of this applies: the OS
//! refuses a duplicate instance and that refusal *is* the occupancy check.

use std::io;

/// What a liveness probe of an existing endpoint established.
///
/// There are exactly three verdicts and no synonyms. In particular there is no
/// value meaning "probably dead" — anything short of proof is [`Unverifiable`].
///
/// [`Unverifiable`]: IncumbentProbe::Unverifiable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncumbentProbe {
    /// Something accepted our connection. The endpoint is occupied.
    Live,
    /// Nothing is listening: the connection was refused, or the entry is gone.
    /// This is the only verdict that permits replacing the entry.
    Exited,
    /// We could not establish either. Timeout, permission error, or an
    /// unrecognized failure. Declines.
    Unverifiable,
}

/// How a successful publish acquired the canonical name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    /// No entry existed; we created it with an exclusive `link`.
    Created,
    /// An entry existed, we proved it dead, and replaced it with one `rename`.
    Replaced,
    /// The platform provides exclusive creation itself (Windows named pipes).
    ExclusiveCreate,
}

/// Why a publish declined to take the canonical name.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    /// A live listener holds the endpoint. This is not an error condition for
    /// the system — it means another daemon is already serving.
    #[error("bridge endpoint is already served by a live listener")]
    Occupied,

    /// An entry exists and we could not prove it dead. Declining is mandatory:
    /// replacing it might displace a live daemon.
    #[error("bridge endpoint exists but its liveness could not be determined")]
    Unverifiable,

    /// The entry kept changing hands while we inspected it, meaning other
    /// publishers are actively contending. Declining leaves the winner in place.
    #[error("bridge endpoint changed hands during publication; another publisher won")]
    Contended,

    #[error("bridge endpoint I/O error: {0}")]
    Io(#[from] io::Error),
}

/// How long a single liveness probe may take before it is [`Unverifiable`].
///
/// [`Unverifiable`]: IncumbentProbe::Unverifiable
pub(crate) const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// Attempts before a contended entry is abandoned to whoever else is publishing.
pub(crate) const MAX_PUBLISH_ATTEMPTS: usize = 3;

/// Scratch-name prefix. Kept to a single `.p` because released tooling elsewhere
/// has been known to sweep other dot-prefixed patterns on age alone.
#[cfg(unix)]
pub(crate) const SCRATCH_PREFIX: &str = ".p";

#[cfg(unix)]
pub(crate) mod unix {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    /// Identifies a directory entry by the inode it points at. Survives the
    /// entry being replaced under us, which is the whole point.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct EntryId {
        dev: u64,
        ino: u64,
    }

    /// Reads the identity of `path`, or `None` if nothing is there.
    pub(crate) fn entry_id(path: &Path) -> io::Result<Option<EntryId>> {
        match fs::symlink_metadata(path) {
            Ok(meta) => Ok(Some(EntryId {
                dev: meta.dev(),
                ino: meta.ino(),
            })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Picks an unused private name beside `canonical` for the initial bind.
    ///
    /// The scratch lives in the same directory so that `link`/`rename` onto the
    /// canonical name stay within one filesystem.
    pub(crate) fn scratch_path(canonical: &Path) -> io::Result<PathBuf> {
        let dir = canonical.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "bridge socket path has no parent directory",
            )
        })?;
        // PID plus a nanosecond reading is enough entropy for a name we also
        // check for existence; a dependency on `rand` would not buy anything here.
        let pid = std::process::id();
        for attempt in 0..16u32 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let tag = (u64::from(pid) << 32) ^ u64::from(nanos) ^ u64::from(attempt);
            let candidate = dir.join(format!("{SCRATCH_PREFIX}{:010x}", tag & 0xff_ffff_ffff));
            if !candidate.exists() {
                return Ok(candidate);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not find an unused scratch name for the bridge socket",
        ))
    }

    /// Classifies a connect attempt against an existing endpoint.
    ///
    /// The mapping is deliberately conservative: only an explicit refusal or a
    /// vanished entry proves death. Everything else declines, because acting on
    /// a guess here deletes a live daemon's only reachable name.
    pub(crate) fn classify_probe(result: Result<io::Result<()>, ()>) -> IncumbentProbe {
        let Ok(io_result) = result else {
            // Elapsed timeout. A full accept backlog looks exactly like this.
            return IncumbentProbe::Unverifiable;
        };
        match io_result {
            Ok(()) => IncumbentProbe::Live,
            Err(e) => match e.kind() {
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound => {
                    IncumbentProbe::Exited
                }
                _ => IncumbentProbe::Unverifiable,
            },
        }
    }

    /// Connects to `path` purely to learn whether anyone is listening.
    ///
    /// No protocol bytes are exchanged: a completed connect is the proof, and a
    /// live peer must not be made to read a handshake it did not ask for.
    pub(crate) async fn probe(path: &Path) -> IncumbentProbe {
        // Linux answers a connect to a regular file with ECONNREFUSED, exactly
        // like a dead socket (macOS says ENOTSOCK); only the entry's type tells
        // them apart. Whatever is not a socket is not ours to prove dead.
        match fs::symlink_metadata(path) {
            Ok(meta) if !meta.file_type().is_socket() => return IncumbentProbe::Unverifiable,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return IncumbentProbe::Exited,
            Err(_) => return IncumbentProbe::Unverifiable,
        }
        let attempt = tokio::time::timeout(PROBE_TIMEOUT, async {
            tokio::net::UnixStream::connect(path).await.map(|stream| {
                drop(stream);
            })
        })
        .await
        .map_err(|_elapsed| ());
        classify_probe(attempt)
    }

    /// Removes a scratch name we created. Never called on the canonical path.
    pub(crate) fn discard_scratch(scratch: &Path) {
        if let Err(e) = fs::remove_file(scratch)
            && e.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(
                path = %scratch.display(),
                error = %e,
                "failed to remove bridge scratch socket"
            );
        }
    }

    /// Restricts the socket to its owner before it becomes reachable.
    ///
    /// Applied to the scratch, so the published entry is never briefly world-
    /// accessible. The server additionally enforces same-UID on every accept.
    pub(crate) fn restrict_to_owner(scratch: &Path) -> io::Result<()> {
        fs::set_permissions(scratch, fs::Permissions::from_mode(0o600))
    }
}
