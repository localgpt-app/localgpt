pub mod peer_identity;
pub mod protocol;

pub use interprocess::local_socket::tokio::Stream as LocalSocketStream;

// Re-export protocol
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
pub struct BridgeListener(TokioListener);

impl BridgeListener {
    /// Accept the next incoming connection.
    pub async fn accept(&self) -> std::io::Result<LocalSocketStream> {
        use interprocess::local_socket::traits::tokio::Listener as _;
        self.0.accept().await
    }
}

impl BridgeServer {
    /// Bind to the socket path, cleaning up if necessary.
    pub fn bind(socket_name: &str) -> std::io::Result<BridgeListener> {
        use interprocess::local_socket::{GenericFilePath, ListenerOptions, ToFsName};

        // Try to cleanup old socket on Unix
        #[cfg(unix)]
        let _ = std::fs::remove_file(socket_name);

        let name = socket_name.to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new().name(name).create_tokio()?;
        tracing::info!("BridgeServer bound to {}", socket_name);
        Ok(BridgeListener(listener))
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
