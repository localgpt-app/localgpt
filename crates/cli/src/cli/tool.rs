use anyhow::{Result, bail};
use clap::{Args, Subcommand};

use localgpt_core::config::Config;
use localgpt_core::paths::Paths;

#[derive(Args)]
pub struct ToolArgs {
    #[command(subcommand)]
    pub command: ToolCommand,
}

#[derive(Subcommand)]
pub enum ToolCommand {
    /// List all configured MCP tool servers
    List,
    /// Add a new MCP tool server to config
    Add {
        /// Server name (used for tool namespacing)
        name: String,
        /// Transport type
        #[arg(short, long, default_value = "stdio")]
        transport: String,
        /// Command to run (for stdio transport)
        #[arg(long)]
        command: Option<String>,
        /// URL (for sse transport)
        #[arg(long)]
        url: Option<String>,
        /// Arguments for the command
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Remove a tool server by name
    Remove {
        /// Server name to remove
        name: String,
    },
    /// Enable a disabled MCP tool server
    Enable {
        /// Server name to enable
        name: String,
    },
    /// Disable an MCP tool server without removing it
    Disable {
        /// Server name to disable
        name: String,
    },
}

pub async fn run(args: ToolArgs) -> Result<()> {
    match args.command {
        ToolCommand::List => cmd_list().await,
        ToolCommand::Add {
            name,
            transport,
            command,
            url,
            args,
        } => cmd_add(&name, &transport, command.as_deref(), url.as_deref(), &args),
        ToolCommand::Remove { name } => cmd_remove(&name),
        ToolCommand::Enable { name } => cmd_enable(&name),
        ToolCommand::Disable { name } => cmd_disable(&name),
    }
}

async fn cmd_list() -> Result<()> {
    let config = Config::load()?;

    if config.mcp.servers.is_empty() {
        println!("No MCP tool servers configured.");
        println!("\nAdd one with: localgpt plugin add <name> -c <command>");
        return Ok(());
    }

    println!(
        "{:<20} {:<10} {:<10} COMMAND/URL",
        "NAME", "TRANSPORT", "STATUS"
    );
    println!("{}", "-".repeat(70));

    for server in &config.mcp.servers {
        let detail = if server.transport == "sse" {
            server.url.as_deref().unwrap_or("-")
        } else {
            server.command.as_deref().unwrap_or("-")
        };

        let args_str = if !server.args.is_empty() {
            format!(" {}", server.args.join(" "))
        } else {
            String::new()
        };

        let status = if server.enabled {
            "enabled"
        } else {
            "disabled"
        };

        println!(
            "{:<20} {:<10} {:<10} {}{}",
            server.name, server.transport, status, detail, args_str
        );
    }

    let enabled = config.mcp.servers.iter().filter(|s| s.enabled).count();
    let total = config.mcp.servers.len();
    println!("\n{} server(s) configured ({} enabled)", total, enabled);

    Ok(())
}

fn cmd_add(
    name: &str,
    transport: &str,
    command: Option<&str>,
    url: Option<&str>,
    args: &[String],
) -> Result<()> {
    // Validate
    if transport != "stdio" && transport != "sse" {
        bail!("Transport must be 'stdio' or 'sse', got '{}'", transport);
    }
    if transport == "stdio" && command.is_none() {
        bail!("Stdio transport requires --command (-c)");
    }
    if transport == "sse" && url.is_none() {
        bail!("SSE transport requires --url (-u)");
    }

    // Check for duplicates
    let config = Config::load()?;
    if config.mcp.servers.iter().any(|s| s.name == name) {
        bail!("Tool server '{}' already exists. Remove it first.", name);
    }

    // Build TOML section
    let paths = Paths::resolve()?;
    let config_path = paths.config_file();

    let mut toml_section = format!("\n[[mcp.servers]]\nname = \"{}\"\n", name);
    toml_section.push_str(&format!("transport = \"{}\"\n", transport));

    if let Some(cmd) = command {
        toml_section.push_str(&format!("command = \"{}\"\n", cmd));
    }
    if let Some(u) = url {
        toml_section.push_str(&format!("url = \"{}\"\n", u));
    }
    if !args.is_empty() {
        let args_toml: Vec<String> = args.iter().map(|a| format!("\"{}\"", a)).collect();
        toml_section.push_str(&format!("args = [{}]\n", args_toml.join(", ")));
    }

    // Append to config file
    let mut content = if config_path.exists() {
        std::fs::read_to_string(&config_path)?
    } else {
        String::new()
    };
    content.push_str(&toml_section);
    std::fs::write(&config_path, content)?;

    println!("Added MCP tool server '{}'", name);
    if transport == "stdio" {
        println!("  Transport: stdio");
        println!("  Command: {}", command.unwrap_or("-"));
    } else {
        println!("  Transport: sse");
        println!("  URL: {}", url.unwrap_or("-"));
    }
    println!("\nRestart the daemon to connect to the new server.");

    Ok(())
}

fn cmd_remove(name: &str) -> Result<()> {
    let paths = Paths::resolve()?;
    let config_path = paths.config_file();

    if !config_path.exists() {
        bail!("No config file found at {}", config_path.display());
    }

    let content = std::fs::read_to_string(&config_path)?;

    // Parse and check if it exists
    let config = Config::load()?;
    if !config.mcp.servers.iter().any(|s| s.name == name) {
        bail!("Tool server '{}' not found in config", name);
    }

    // Remove the [[mcp.servers]] block matching this name
    let new_content = remove_mcp_server_block(&content, name);
    std::fs::write(&config_path, new_content)?;

    println!("Removed MCP tool server '{}'", name);
    println!("Restart the daemon to apply changes.");

    Ok(())
}

fn cmd_enable(name: &str) -> Result<()> {
    let mut config = Config::load()?;

    let server = config
        .mcp
        .servers
        .iter_mut()
        .find(|s| s.name == name)
        .ok_or_else(|| anyhow::anyhow!("Tool server '{}' not found", name))?;

    if server.enabled {
        println!("Tool server '{}' is already enabled.", name);
        return Ok(());
    }

    server.enabled = true;
    config.save()?;

    println!("Enabled tool server '{}'", name);
    println!("Restart the daemon to apply changes.");

    Ok(())
}

fn cmd_disable(name: &str) -> Result<()> {
    let mut config = Config::load()?;

    let server = config
        .mcp
        .servers
        .iter_mut()
        .find(|s| s.name == name)
        .ok_or_else(|| anyhow::anyhow!("Tool server '{}' not found", name))?;

    if !server.enabled {
        println!("Tool server '{}' is already disabled.", name);
        return Ok(());
    }

    server.enabled = false;
    config.save()?;

    println!("Disabled tool server '{}'", name);
    println!("Restart the daemon to apply changes.");

    Ok(())
}

/// Remove a [[mcp.servers]] block from TOML content by server name.
/// Operates on raw text to preserve comments and formatting.
fn remove_mcp_server_block(content: &str, name: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();
    let mut skip_block = false;
    let name_pattern = format!("name = \"{}\"", name);

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed == "[[mcp.servers]]" {
            // Look ahead to check if this block matches the target name
            let mut found_name = false;
            let mut block_end = i + 1;

            for (j, next_line) in lines.iter().enumerate().skip(i + 1) {
                let next_trimmed = next_line.trim();
                if next_trimmed.starts_with("[[")
                    || (next_trimmed.starts_with('[') && !next_trimmed.starts_with("[["))
                {
                    block_end = j;
                    break;
                }
                if next_trimmed.contains(&name_pattern) {
                    found_name = true;
                }
                block_end = j + 1;
            }

            if found_name {
                // Skip this entire block (including trailing blank lines)
                i = block_end;
                while i < lines.len() && lines[i].trim().is_empty() {
                    i += 1;
                }
                skip_block = true;
                continue;
            }
        }

        if !skip_block {
            result.push(line);
        }
        skip_block = false;
        i += 1;
    }

    let mut output = result.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_mcp_server_block() {
        let content = r#"[agent]
default_model = "claude-cli/opus"

[[mcp.servers]]
name = "filesystem"
transport = "stdio"
command = "npx"
args = ["@mcp/server-fs"]

[[mcp.servers]]
name = "web"
transport = "sse"
url = "http://localhost:8080/sse"

[server]
port = 31327
"#;
        let result = remove_mcp_server_block(content, "filesystem");
        assert!(!result.contains("filesystem"));
        assert!(result.contains("web")); // Other server preserved
        assert!(result.contains("[agent]")); // Other sections preserved
        assert!(result.contains("[server]"));
    }

    #[test]
    fn test_remove_mcp_server_block_last() {
        let content = r#"[[mcp.servers]]
name = "only-server"
transport = "stdio"
command = "test"
"#;
        let result = remove_mcp_server_block(content, "only-server");
        assert!(!result.contains("only-server"));
    }

    #[test]
    fn test_remove_mcp_server_block_not_found() {
        let content = r#"[[mcp.servers]]
name = "keep-this"
transport = "stdio"
command = "test"
"#;
        let result = remove_mcp_server_block(content, "nonexistent");
        assert!(result.contains("keep-this")); // Unchanged
    }
}
