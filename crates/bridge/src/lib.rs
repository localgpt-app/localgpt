pub mod endpoint;
pub mod peer_identity;
pub mod protocol;

pub use interprocess::local_socket::tokio::Stream as LocalSocketStream;

// Re-export protocol
pub use endpoint::{IncumbentProbe, PublishError, PublishOutcome};
pub use protocol::{BRIDGE_PROTOCOL_VERSION, BridgeError, BridgeService, BridgeServiceClient};

use futures::StreamExt;
use interprocess::local_socket::tokio::Listener as TokioListener;
use tarpc::server::{BaseChannel, Channel};

/// Manages the local-socket server that local client bridges (such as the
/// CLI bridge) connect to for secure IPC with the core agent.
pub struct BridgeServer;

/// A bound local-socket listener that accepts incoming bridge connections.
///
/// Wraps the `interprocess` Tokio listener so callers don't need the
/// `interprocess` traits in scope just to accept connections.
#[derive(Debug)]
pub struct BridgeListener(TokioListener);

impl BridgeListener {
    /// Accept the next incoming connection.
    pub async fn accept(&self) -> std::io::Result<LocalSocketStream> {
        use interprocess::local_socket::traits::tokio::Listener as _;
        self.0.accept().await
    }
}

impl BridgeServer {
    /// Publish this process as the owner of the bridge endpoint.
    ///
    /// Follows the ownership protocol in [`endpoint`]: the returned listener is
    /// reachable at `socket_name`, and no live listener was displaced to get it.
    ///
    /// Returns [`PublishError::Occupied`] when another daemon is already serving,
    /// which callers should treat as "someone else has the job", not as a fault.
    ///
    /// The listener never unlinks `socket_name` when dropped. A departing daemon
    /// leaves a dead entry behind on purpose; the next publisher replaces it in
    /// one `rename`. Removing it on the way out is what lets a shutdown delete a
    /// successor's endpoint.
    pub async fn publish(
        socket_name: &str,
    ) -> Result<(BridgeListener, PublishOutcome), PublishError> {
        #[cfg(unix)]
        {
            Self::publish_unix(socket_name).await
        }
        #[cfg(windows)]
        {
            // Named pipes have no directory entry to contend over: the OS refuses
            // a second instance, and `try_overwrite` stays off so that refusal is
            // reported rather than papered over by displacing the incumbent.
            match Self::bind_raw(socket_name) {
                Ok(listener) => Ok((listener, PublishOutcome::ExclusiveCreate)),
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Err(PublishError::Occupied),
                Err(e) => Err(PublishError::Io(e)),
            }
        }
    }

    /// Bind a listener at exactly `socket_name` with no ownership negotiation.
    ///
    /// `reclaim_name(false)` keeps the listener from unlinking the path on drop;
    /// `try_overwrite` is left off so an occupied name surfaces as `AddrInUse`
    /// instead of silently displacing whoever holds it.
    fn bind_raw(socket_name: &str) -> std::io::Result<BridgeListener> {
        use interprocess::local_socket::{GenericFilePath, ListenerOptions, ToFsName};

        let name = socket_name.to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new()
            .name(name)
            .reclaim_name(false)
            .create_tokio()?;
        Ok(BridgeListener(listener))
    }

    #[cfg(unix)]
    async fn publish_unix(
        socket_name: &str,
    ) -> Result<(BridgeListener, PublishOutcome), PublishError> {
        use endpoint::unix::{discard_scratch, entry_id, restrict_to_owner, scratch_path};
        use std::path::Path;

        let canonical = Path::new(socket_name);
        let scratch = scratch_path(canonical)?;

        let listener = match Self::bind_raw(&scratch.to_string_lossy()) {
            Ok(l) => l,
            Err(e) => return Err(PublishError::Io(e)),
        };
        restrict_to_owner(&scratch)?;

        // Our own inode, captured before the name can move. After the rename this
        // is what proves the canonical entry is ours and not a racer's.
        let ours = match entry_id(&scratch) {
            Ok(Some(id)) => id,
            Ok(None) => {
                return Err(PublishError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "bridge scratch socket vanished immediately after bind",
                )));
            }
            Err(e) => {
                discard_scratch(&scratch);
                return Err(PublishError::Io(e));
            }
        };

        let result = Self::negotiate_canonical_name(canonical, &scratch, ours).await;

        // We created the scratch, so we are the only actor entitled to remove it —
        // and we do so on every path that is not a crash. After `Created` the
        // scratch is a second hard link to the very socket we are serving, so
        // dropping that name leaves the canonical one reaching the same inode.
        // After `Replaced` the rename already consumed it.
        discard_scratch(&scratch);

        match result {
            Ok(outcome) => {
                tracing::info!(
                    path = %canonical.display(),
                    ?outcome,
                    "bridge endpoint published"
                );
                Ok((listener, outcome))
            }
            Err(e) => Err(e),
        }
    }

    /// The `link` / prove-dead / re-check / `rename` core.
    #[cfg(unix)]
    async fn negotiate_canonical_name(
        canonical: &std::path::Path,
        scratch: &std::path::Path,
        ours: endpoint::unix::EntryId,
    ) -> Result<PublishOutcome, PublishError> {
        use endpoint::unix::{entry_id, probe};
        use endpoint::{IncumbentProbe, MAX_PUBLISH_ATTEMPTS};

        for _attempt in 0..MAX_PUBLISH_ATTEMPTS {
            // `link` fails loudly on an existing name. An unconditional `rename`
            // here would silently destroy a healthy daemon's endpoint.
            match std::fs::hard_link(scratch, canonical) {
                Ok(()) => return Ok(PublishOutcome::Created),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(PublishError::Io(e)),
            }

            let before = entry_id(canonical)?;
            if before.is_none() {
                // Vanished between the link and the stat; try to claim it again.
                continue;
            }

            match probe(canonical).await {
                IncumbentProbe::Live => return Err(PublishError::Occupied),
                IncumbentProbe::Unverifiable => return Err(PublishError::Unverifiable),
                IncumbentProbe::Exited => {}
            }

            // Did the entry change hands while we were probing? If so, whatever we
            // just proved dead is no longer what sits there.
            if entry_id(canonical)? != before {
                continue;
            }

            // Second probe against the entry we just confirmed is still the same
            // one. A daemon may have published onto it since the first probe.
            match probe(canonical).await {
                IncumbentProbe::Live => return Err(PublishError::Occupied),
                IncumbentProbe::Unverifiable => return Err(PublishError::Unverifiable),
                IncumbentProbe::Exited => {}
            }

            // One syscall, no window in which the name is absent.
            std::fs::rename(scratch, canonical).map_err(PublishError::Io)?;

            // POSIX has no rename-if-target-is-inode-X, so confirm we kept it.
            // If we lost, we have already stopped owning the scratch name and must
            // not touch the canonical entry again.
            return match entry_id(canonical)? {
                Some(now) if now == ours => Ok(PublishOutcome::Replaced),
                _ => Err(PublishError::Contended),
            };
        }

        Err(PublishError::Contended)
    }
}

/// Handle a single connection.
/// This should be called inside a spawned task.
pub async fn handle_connection<S>(conn: LocalSocketStream, service: S) -> anyhow::Result<()>
where
    S: BridgeService + Send + Clone + 'static,
{
    // interprocess 2.x Tokio streams implement `tokio::io::AsyncRead`/`AsyncWrite`
    // directly, so they can be framed without a futures<->tokio compat shim.
    use tarpc::tokio_util::codec::{Framed, LengthDelimitedCodec};
    use tokio_serde::formats::Json;

    let transport = tarpc::serde_transport::new(
        Framed::new(conn, LengthDelimitedCodec::new()),
        Json::default(),
    );

    BaseChannel::with_defaults(transport)
        .execute(service.serve())
        .for_each(|span| async move {
            span.await;
        })
        .await;

    Ok(())
}

/// Connect to a running [`BridgeServer`] at the given socket path and
/// return a tarpc client that can invoke bridge service RPCs.
pub async fn connect(socket_name: &str) -> anyhow::Result<BridgeServiceClient> {
    use interprocess::local_socket::traits::tokio::Stream as _;
    use interprocess::local_socket::{GenericFilePath, ToFsName};

    let name = socket_name.to_fs_name::<GenericFilePath>()?;
    let conn = LocalSocketStream::connect(name).await?;

    use tarpc::tokio_util::codec::{Framed, LengthDelimitedCodec};
    use tokio_serde::formats::Json;

    let transport = tarpc::serde_transport::new(
        Framed::new(conn, LengthDelimitedCodec::new()),
        Json::default(),
    );

    let client = BridgeServiceClient::new(tarpc::client::Config::default(), transport).spawn();
    Ok(client)
}

#[cfg(all(test, unix))]
mod endpoint_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn canonical_in(dir: &Path) -> String {
        dir.join("bridge.sock").to_string_lossy().to_string()
    }

    fn scratch_names(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(endpoint::SCRATCH_PREFIX))
            })
            .collect()
    }

    /// Leaves a socket file with nothing listening — what a crashed daemon leaves.
    fn stale_entry(path: &str) {
        let listener = BridgeServer::bind_raw(path).unwrap();
        drop(listener);
        assert!(
            Path::new(path).exists(),
            "reclaim_name(false) must keep the entry"
        );
    }

    #[tokio::test]
    async fn publishes_onto_a_clean_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical_in(dir.path());

        let (listener, outcome) = BridgeServer::publish(&path).await.unwrap();

        assert_eq!(outcome, PublishOutcome::Created);
        assert!(Path::new(&path).exists());
        assert!(
            scratch_names(dir.path()).is_empty(),
            "scratch must not linger"
        );
        drop(listener);
    }

    #[tokio::test]
    async fn declines_while_a_live_listener_holds_the_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical_in(dir.path());

        let (incumbent, _) = BridgeServer::publish(&path).await.unwrap();
        let before = endpoint::unix::entry_id(Path::new(&path)).unwrap();

        let second = BridgeServer::publish(&path).await;

        assert!(
            matches!(second, Err(PublishError::Occupied)),
            "got {second:?}"
        );
        assert_eq!(
            endpoint::unix::entry_id(Path::new(&path)).unwrap(),
            before,
            "a declined publish must not disturb the live endpoint"
        );
        assert!(scratch_names(dir.path()).is_empty());
        drop(incumbent);
    }

    #[tokio::test]
    async fn replaces_an_entry_it_proved_dead() {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical_in(dir.path());
        stale_entry(&path);
        let dead = endpoint::unix::entry_id(Path::new(&path)).unwrap();

        let (listener, outcome) = BridgeServer::publish(&path).await.unwrap();

        assert_eq!(outcome, PublishOutcome::Replaced);
        assert_ne!(
            endpoint::unix::entry_id(Path::new(&path)).unwrap(),
            dead,
            "the stale inode must be gone, replaced by ours"
        );
        assert!(scratch_names(dir.path()).is_empty());
        drop(listener);
    }

    #[tokio::test]
    async fn a_replaced_endpoint_actually_serves() {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical_in(dir.path());
        stale_entry(&path);

        let (listener, _) = BridgeServer::publish(&path).await.unwrap();
        let accept = tokio::spawn(async move { listener.accept().await.map(|_| ()) });
        let client = tokio::net::UnixStream::connect(&path).await;

        assert!(client.is_ok(), "published endpoint must accept connections");
        assert!(accept.await.unwrap().is_ok());
    }

    /// The rule that the whole protocol exists to enforce: a departing daemon
    /// must not delete the entry, because by then it may belong to a successor.
    #[tokio::test]
    async fn dropping_the_listener_leaves_the_entry_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical_in(dir.path());

        let (listener, _) = BridgeServer::publish(&path).await.unwrap();
        drop(listener);

        assert!(
            Path::new(&path).exists(),
            "shutdown must leave a dead entry, not unlink a name a successor may hold"
        );

        // And the successor takes it over without any sweeping step.
        let (next, outcome) = BridgeServer::publish(&path).await.unwrap();
        assert_eq!(outcome, PublishOutcome::Replaced);
        drop(next);
    }

    #[tokio::test]
    async fn declines_when_the_path_is_not_a_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical_in(dir.path());
        std::fs::write(&path, b"not a socket").unwrap();

        let result = BridgeServer::publish(&path).await;

        assert!(
            matches!(
                result,
                Err(PublishError::Unverifiable | PublishError::Occupied)
            ),
            "an unrecognized occupant is never proof of death; got {result:?}"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"not a socket",
            "a file we did not create must survive untouched"
        );
        assert!(scratch_names(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn concurrent_publishers_yield_exactly_one_winner() {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical_in(dir.path());

        let mut tasks = Vec::new();
        for _ in 0..8 {
            let p = path.clone();
            tasks.push(tokio::spawn(async move { BridgeServer::publish(&p).await }));
        }

        let mut winners = Vec::new();
        for t in tasks {
            if let Ok(Ok((listener, outcome))) = t.await {
                winners.push((listener, outcome));
            }
        }

        assert_eq!(
            winners.len(),
            1,
            "exactly one publisher may hold the endpoint"
        );
        assert!(
            scratch_names(dir.path()).is_empty(),
            "losers must clean up their own scratch"
        );
    }

    #[test]
    fn probe_classification_never_guesses() {
        use endpoint::unix::classify_probe;
        use std::io::{Error, ErrorKind};

        // Proof of death — the only two that permit replacement.
        assert_eq!(
            classify_probe(Ok(Err(Error::from(ErrorKind::ConnectionRefused)))),
            IncumbentProbe::Exited
        );
        assert_eq!(
            classify_probe(Ok(Err(Error::from(ErrorKind::NotFound)))),
            IncumbentProbe::Exited
        );

        // Proof of life.
        assert_eq!(classify_probe(Ok(Ok(()))), IncumbentProbe::Live);

        // Everything else declines. A full backlog times out; a socket owned by
        // another user gives EPERM. Neither says anything about liveness.
        assert_eq!(classify_probe(Err(())), IncumbentProbe::Unverifiable);
        assert_eq!(
            classify_probe(Ok(Err(Error::from(ErrorKind::PermissionDenied)))),
            IncumbentProbe::Unverifiable
        );
        assert_eq!(
            classify_probe(Ok(Err(Error::from(ErrorKind::ConnectionReset)))),
            IncumbentProbe::Unverifiable
        );
    }

    #[test]
    fn scratch_names_are_unique_and_local_to_the_endpoint_directory() {
        use endpoint::unix::scratch_path;
        let dir = tempfile::tempdir().unwrap();
        let canonical = dir.path().join("bridge.sock");

        let a = scratch_path(&canonical).unwrap();
        std::fs::write(&a, b"").unwrap();
        let b = scratch_path(&canonical).unwrap();

        assert_ne!(a, b);
        // Same directory keeps `link`/`rename` on one filesystem.
        assert_eq!(a.parent(), canonical.parent());
        assert_eq!(b.parent(), canonical.parent());
    }
}
