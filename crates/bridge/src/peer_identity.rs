use crate::LocalSocketStream;
use interprocess::local_socket::traits::StreamCommon;
use std::io;

/// Identity of a peer connected via a local (Unix-domain / named-pipe) socket.
///
/// On Linux/Android all three fields are available via `SO_PEERCRED`.
/// On macOS/iOS only `uid` and `gid` are returned (`getpeereid`).
/// On Windows only `pid` is available (`GetNamedPipeClientProcessId`).
#[derive(Debug, Clone, Copy)]
pub struct PeerIdentity {
    /// Effective user ID of the connected process (Unix only).
    pub uid: Option<u32>,
    /// Effective group ID of the connected process (Unix only).
    pub gid: Option<u32>,
    /// Process ID of the connected process.
    pub pid: Option<i32>,
}

/// Look up the credentials of the process on the other end of `stream`.
///
/// Delegates to `interprocess`' cross-platform peer-credentials API, which uses
/// `SO_PEERCRED` / `getpeereid` on Unix and `GetNamedPipeClientProcessId` on
/// Windows under the hood. Only Unix exposes uid/gid; Windows reports just
/// the pid (as `u32`, widened here).
pub fn get_peer_identity(stream: &LocalSocketStream) -> io::Result<PeerIdentity> {
    let creds = stream.peer_creds()?;

    #[cfg(unix)]
    let (uid, gid) = (creds.euid(), creds.egid());
    #[cfg(not(unix))]
    let (uid, gid) = (None, None);

    #[cfg(unix)]
    let pid = creds.pid();
    #[cfg(not(unix))]
    let pid = creds.pid().map(|p| p as i32);

    Ok(PeerIdentity { uid, gid, pid })
}
