pub mod peer_identity;
pub mod protocol;

pub use interprocess::local_socket::tokio::{LocalSocketListener, LocalSocketStream};

// Re-export protocol
pub use protocol::{BRIDGE_PROTOCOL_VERSION, BridgeError, BridgeService, BridgeServiceClient};

use futures::StreamExt;
use tarpc::server::{BaseChannel, Channel};

pub struct BridgeServer;

impl BridgeServer {
    /// Bind to the socket path, cleaning up if necessary.
    pub fn bind(socket_name: &str) -> std::io::Result<LocalSocketListener> {
        // Try to cleanup old socket on Unix
        #[cfg(unix)]
        let _ = std::fs::remove_file(socket_name);

        let listener = LocalSocketListener::bind(socket_name)?;
        tracing::info!("BridgeServer bound to {}", socket_name);
        Ok(listener)
    }
}

/// Handle a single connection.
/// This should be called inside a spawned task.
pub async fn handle_connection<S>(conn: LocalSocketStream, service: S) -> anyhow::Result<()>
where
    S: BridgeService + Send + Clone + 'static,
{
    // Wrap with tokio-util compat
    use tokio_util::compat::FuturesAsyncReadCompatExt;
    let conn = conn.compat();

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

pub async fn connect(socket_name: &str) -> anyhow::Result<BridgeServiceClient> {
    let conn = LocalSocketStream::connect(socket_name).await?;

    use tokio_util::compat::FuturesAsyncReadCompatExt;
    let conn = conn.compat();

    use tarpc::tokio_util::codec::{Framed, LengthDelimitedCodec};
    use tokio_serde::formats::Json;

    let transport = tarpc::serde_transport::new(
        Framed::new(conn, LengthDelimitedCodec::new()),
        Json::default(),
    );

    let client = BridgeServiceClient::new(tarpc::client::Config::default(), transport).spawn();
    Ok(client)
}
