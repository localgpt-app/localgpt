//! localgpt-bridge-cli — Interactive CLI that connects to a running LocalGPT daemon
//! via the bridge IPC socket, providing the same chat experience as `localgpt chat`.
//!
//! # Security
//!
//! - Connects via Unix domain socket with peer identity verification (same UID only).
//! - The bridge CLI does NOT get dangerous tools (bash, write_file, etc.) — the daemon
//!   controls which tools are available to bridge sessions.
//! - All agent operations run server-side within the daemon process.

use anyhow::Result;
use clap::Parser;
use localgpt_bridge::{BridgeServiceClient, connect};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::{self, Write};
use tarpc::context;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "localgpt-bridge-cli")]
#[command(version, about = "Interactive CLI bridge for LocalGPT daemon")]
struct Args {
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Model to use (overrides daemon default)
    #[arg(short, long)]
    model: Option<String>,

    /// Custom session ID (default: auto-generated)
    #[arg(short, long)]
    session: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let log_level = if args.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    info!("Starting LocalGPT CLI Bridge...");

    // 1. Connect to Bridge Manager
    let paths = localgpt_core::paths::Paths::resolve()?;
    let socket_path = paths.bridge_socket_name();

    info!("Connecting to bridge socket: {}", socket_path);
    let client = match connect(&socket_path).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Failed to connect to daemon bridge socket at '{}'.\n\
                 Make sure the daemon is running: localgpt daemon start\n\
                 Error: {}",
                socket_path, e
            );
            std::process::exit(1);
        }
    };

    // 2. Verify protocol version (require major version 1)
    match client.get_version(context::current()).await {
        Ok(v) => {
            let major = v.split('.').next().and_then(|s| s.parse::<u32>().ok());
            match major {
                Some(1) => {
                    info!("Bridge protocol version: {}", v);
                }
                Some(m) => {
                    eprintln!(
                        "Unsupported bridge protocol major version {} (got '{}', need 1.x).\n\
                         Please update localgpt-bridge-cli.",
                        m, v
                    );
                    std::process::exit(1);
                }
                None => {
                    eprintln!("Invalid bridge protocol version: '{}'", v);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("Could not retrieve bridge version: {}", e);
            std::process::exit(1);
        }
    }

    // 3. Create or use provided session ID
    let session_id = args.session.unwrap_or_else(|| Uuid::new_v4().to_string());

    // 4. Initialize session
    match client
        .new_session(context::current(), session_id.clone())
        .await
    {
        Ok(Ok(info)) => {
            println!("{}", info);
        }
        Ok(Err(e)) => {
            eprintln!("Failed to create session: {}", e);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("RPC error creating session: {}", e);
            std::process::exit(1);
        }
    }

    // 5. Set model if specified
    if let Some(ref model) = args.model {
        match client
            .set_model(context::current(), session_id.clone(), model.clone())
            .await
        {
            Ok(Ok(msg)) => println!("{}", msg),
            Ok(Err(e)) => eprintln!("Warning: Failed to set model: {}", e),
            Err(e) => eprintln!("Warning: RPC error setting model: {}", e),
        }
    }

    println!(
        "\nLocalGPT CLI Bridge | Session: {}\n",
        &session_id[..session_id.len().min(8)]
    );
    println!("Type /help for commands, /quit to exit\n");

    // 6. Interactive loop
    run_interactive_loop(&client, &session_id).await?;

    println!("Goodbye!");
    Ok(())
}

async fn run_interactive_loop(client: &BridgeServiceClient, session_id: &str) -> Result<()> {
    let mut rl = DefaultEditor::new()?;
    let mut stdout = io::stdout();

    loop {
        let readline = rl.readline("You: ");

        let input = match readline {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        };

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let _ = rl.add_history_entry(input);

        // Handle commands
        if input.starts_with('/') {
            match handle_command(input, client, session_id).await {
                CommandResult::Continue => continue,
                CommandResult::Quit => break,
            }
        }

        // Send message to agent
        print!("\nLocalGPT: ");
        stdout.flush()?;

        // Use a long-lived context for chat (chat can take a while)
        let mut ctx = context::current();
        ctx.deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);

        match client
            .chat(ctx, session_id.to_string(), input.to_string())
            .await
        {
            Ok(Ok(response)) => {
                println!("{}\n", response);
            }
            Ok(Err(e)) => {
                eprintln!("\nError: {}\n", e);
            }
            Err(e) => {
                error!("RPC error: {}", e);
                eprintln!("\nConnection error: {}\n", e);
            }
        }
    }

    Ok(())
}

enum CommandResult {
    Continue,
    Quit,
}

async fn handle_command(
    input: &str,
    client: &BridgeServiceClient,
    session_id: &str,
) -> CommandResult {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let cmd = parts[0];

    match cmd {
        "/quit" | "/exit" | "/q" => CommandResult::Quit,

        "/help" | "/h" | "/?" => {
            println!("\nCommands:");
            println!("  /help, /h, /?       - Show this help");
            println!("  /quit, /exit, /q    - Exit");
            println!("  /new                - Start a fresh session");
            println!("  /status             - Show session info");
            println!("  /model [name]       - Show or switch model");
            println!("  /compact            - Compact session history");
            println!("  /clear              - Clear session history");
            println!("  /memory <query>     - Search memory files");
            println!("  /stats              - Show memory statistics");
            println!("  /ping               - Check daemon connectivity");
            println!();
            CommandResult::Continue
        }

        "/new" => {
            match client
                .new_session(context::current(), session_id.to_string())
                .await
            {
                Ok(Ok(msg)) => println!("\n{}\n", msg),
                Ok(Err(e)) => eprintln!("\nError: {}\n", e),
                Err(e) => eprintln!("\nRPC error: {}\n", e),
            }
            CommandResult::Continue
        }

        "/status" => {
            match client
                .session_status(context::current(), session_id.to_string())
                .await
            {
                Ok(Ok(status)) => println!("\n{}\n", status),
                Ok(Err(e)) => eprintln!("\nError: {}\n", e),
                Err(e) => eprintln!("\nRPC error: {}\n", e),
            }
            CommandResult::Continue
        }

        "/model" => {
            if parts.len() >= 2 {
                let model = parts[1];
                match client
                    .set_model(
                        context::current(),
                        session_id.to_string(),
                        model.to_string(),
                    )
                    .await
                {
                    Ok(Ok(msg)) => println!("\n{}\n", msg),
                    Ok(Err(e)) => eprintln!("\nError: {}\n", e),
                    Err(e) => eprintln!("\nRPC error: {}\n", e),
                }
            } else {
                // Show current model via status
                match client
                    .session_status(context::current(), session_id.to_string())
                    .await
                {
                    Ok(Ok(status)) => {
                        // Extract model line from status
                        for line in status.lines() {
                            if line.starts_with("Model:") {
                                println!("\nCurrent {}\n", line.to_lowercase());
                                break;
                            }
                        }
                    }
                    Ok(Err(e)) => eprintln!("\nError: {}\n", e),
                    Err(e) => eprintln!("\nRPC error: {}\n", e),
                }
            }
            CommandResult::Continue
        }

        "/compact" => {
            match client
                .compact_session(context::current(), session_id.to_string())
                .await
            {
                Ok(Ok(msg)) => println!("\n{}\n", msg),
                Ok(Err(e)) => eprintln!("\nError: {}\n", e),
                Err(e) => eprintln!("\nRPC error: {}\n", e),
            }
            CommandResult::Continue
        }

        "/clear" => {
            match client
                .clear_session(context::current(), session_id.to_string())
                .await
            {
                Ok(Ok(msg)) => println!("\n{}\n", msg),
                Ok(Err(e)) => eprintln!("\nError: {}\n", e),
                Err(e) => eprintln!("\nRPC error: {}\n", e),
            }
            CommandResult::Continue
        }

        "/memory" => {
            if parts.len() < 2 {
                eprintln!("Usage: /memory <query>");
                return CommandResult::Continue;
            }
            let query = parts[1..].join(" ");
            match client.memory_search(context::current(), query, 10).await {
                Ok(Ok(results)) => println!("\n{}\n", results),
                Ok(Err(e)) => eprintln!("\nError: {}\n", e),
                Err(e) => eprintln!("\nRPC error: {}\n", e),
            }
            CommandResult::Continue
        }

        "/stats" => {
            match client.memory_stats(context::current()).await {
                Ok(Ok(stats)) => println!("\n{}", stats),
                Ok(Err(e)) => eprintln!("\nError: {}\n", e),
                Err(e) => eprintln!("\nRPC error: {}\n", e),
            }
            CommandResult::Continue
        }

        "/ping" => {
            match client.ping(context::current()).await {
                Ok(true) => println!("\nDaemon is alive.\n"),
                Ok(false) => println!("\nDaemon returned unhealthy status.\n"),
                Err(e) => eprintln!("\nDaemon unreachable: {}\n", e),
            }
            CommandResult::Continue
        }

        _ => {
            eprintln!(
                "Unknown command: {}. Type /help for available commands.",
                cmd
            );
            CommandResult::Continue
        }
    }
}
