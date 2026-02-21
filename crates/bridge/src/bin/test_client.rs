use clap::Parser;
use localgpt_bridge::connect;
use tarpc::context;

#[derive(Parser)]
#[command(name = "test-bridge")]
struct Cli {
    #[arg(short, long)]
    socket: String,

    #[arg(short, long)]
    bridge_id: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    let cli = Cli::parse();

    tracing::info!("Connecting to bridge socket at: {}", cli.socket);

    // Connect to Bridge
    let client = connect(&cli.socket).await?;

    // Request credentials
    tracing::info!("Requesting credentials for: {}", cli.bridge_id);
    match client
        .get_credentials(context::current(), cli.bridge_id.clone())
        .await?
    {
        Ok(secret) => {
            tracing::info!("Successfully retrieved credentials!");
            tracing::info!("Secret length: {} bytes", secret.len());
            if let Ok(s) = String::from_utf8(secret) {
                tracing::info!("Secret content (utf8): {}", s);
            }
        }
        Err(e) => {
            tracing::error!("Failed to retrieve credentials: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}
