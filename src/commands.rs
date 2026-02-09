//! Unified slash command definitions shared across CLI and Telegram interfaces.

/// Which interfaces support a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interface {
    Cli,
    Telegram,
}

/// A slash command definition.
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub interfaces: &'static [Interface],
}

impl SlashCommand {
    pub fn supports(&self, iface: Interface) -> bool {
        self.interfaces.contains(&iface)
    }

    /// Format as a help line, e.g. "  /help, /h, /?     - Show this help"
    fn help_line(&self) -> String {
        let mut names = format!("/{}", self.name);
        for alias in self.aliases {
            names.push_str(&format!(", /{}", alias));
        }
        if !self.usage.is_empty() {
            names.push_str(&format!(" {}", self.usage));
        }
        format!("  {:<20}- {}", names, self.description)
    }
}

pub const COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "help",
        description: "Show available commands",
        aliases: &["h", "?"],
        usage: "",
        interfaces: &[Interface::Cli, Interface::Telegram],
    },
    SlashCommand {
        name: "quit",
        description: "Exit chat",
        aliases: &["exit", "q"],
        usage: "",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "new",
        description: "Start a fresh session",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli, Interface::Telegram],
    },
    SlashCommand {
        name: "skills",
        description: "List available skills",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli, Interface::Telegram],
    },
    SlashCommand {
        name: "sessions",
        description: "List saved sessions",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "search",
        description: "Search across sessions",
        aliases: &[],
        usage: "<query>",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "resume",
        description: "Resume a session",
        aliases: &[],
        usage: "<id>",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "model",
        description: "Show or switch model",
        aliases: &[],
        usage: "[name]",
        interfaces: &[Interface::Cli, Interface::Telegram],
    },
    SlashCommand {
        name: "models",
        description: "List model prefixes",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "context",
        description: "Show context window usage",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "export",
        description: "Export session as markdown",
        aliases: &[],
        usage: "[file]",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "attach",
        description: "Attach file to message",
        aliases: &[],
        usage: "<file>",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "attachments",
        description: "List pending attachments",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "compact",
        description: "Compact session history",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli, Interface::Telegram],
    },
    SlashCommand {
        name: "clear",
        description: "Clear session history",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli, Interface::Telegram],
    },
    SlashCommand {
        name: "memory",
        description: "Search memory files",
        aliases: &[],
        usage: "<query>",
        interfaces: &[Interface::Cli, Interface::Telegram],
    },
    SlashCommand {
        name: "reindex",
        description: "Rebuild memory index",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "save",
        description: "Save current session",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli],
    },
    SlashCommand {
        name: "status",
        description: "Show session info",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Cli, Interface::Telegram],
    },
    SlashCommand {
        name: "unpair",
        description: "Unpair Telegram account",
        aliases: &[],
        usage: "",
        interfaces: &[Interface::Telegram],
    },
];

/// Format help text for a given interface.
pub fn format_help_text(iface: Interface) -> String {
    let mut lines = vec!["Commands:".to_string()];
    for cmd in COMMANDS {
        if cmd.supports(iface) {
            lines.push(cmd.help_line());
        }
    }
    lines.join("\n")
}

/// Build a list of `teloxide::types::BotCommand` for Telegram's setMyCommands.
pub fn telegram_bot_commands() -> Vec<teloxide::types::BotCommand> {
    use teloxide::types::BotCommand;
    COMMANDS
        .iter()
        .filter(|cmd| cmd.supports(Interface::Telegram))
        .map(|cmd| BotCommand::new(cmd.name, cmd.description))
        .collect()
}
