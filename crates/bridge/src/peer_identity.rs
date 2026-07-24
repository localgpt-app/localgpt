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
/// Windows under the hood.
pub fn get_peer_identity(stream: &LocalSocketStream) -> io::Result<PeerIdentity> {
    let creds = stream.peer_creds()?;
    Ok(PeerIdentity {
        uid: creds.euid(),
        gid: creds.egid(),
        pid: creds.pid(),
    })
}
