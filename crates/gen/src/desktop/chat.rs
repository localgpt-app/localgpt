//! Channels between the agent loop (a tokio thread) and the prompt panel
//! (Bevy's main thread).
//!
//! Prompts flow from the panel into the same merged input stream the REPL
//! feeds; the agent loop reports what it's doing back as [`ChatEvent`]s.
//! Both directions are unbounded and non-blocking, so neither side can stall
//! the other: a closed panel just drops events, and a busy agent just queues
//! prompts.

use tokio::sync::mpsc;

/// What the agent loop reports to the prompt panel.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    /// The agent is ready, or switched models.
    Ready { model: String },
    /// Models this machine can switch to right now (detected CLIs and local
    /// Ollama models), for the panel's model menu.
    ModelOptions(Vec<String>),
    /// The agent started a turn. `from` is `None` for prompts typed in this
    /// panel, and names the sender otherwise (the terminal, or a
    /// collaborative client).
    Prompt { text: String, from: Option<String> },
    /// Streamed text from the model.
    Delta(String),
    /// A tool call started.
    ToolStarted {
        name: String,
        detail: Option<String>,
    },
    /// A tool call finished.
    ToolFinished { name: String, error: Option<String> },
    /// The turn is over; `error` is set when the model or stream failed.
    TurnFinished { error: Option<String> },
    /// Something worth telling the user that isn't model output.
    Notice(String),
    /// A problem the user should fix, like a missing CLI backend; shown
    /// prominently, but the agent keeps running.
    Warning(String),
    /// The agent couldn't start, or stopped with an error; no more turns
    /// will run.
    Failed(String),
}

/// Sending half of the event channel, held by the agent loop.
#[derive(Clone, Debug)]
pub struct ChatSink(mpsc::UnboundedSender<ChatEvent>);

impl ChatSink {
    /// Report an event. Never blocks; ignored once the panel is gone.
    pub fn send(&self, event: ChatEvent) {
        let _ = self.0.send(event);
    }
}

/// The panel's side: prompts out, events in.
pub struct PanelChannels {
    pub prompt_tx: mpsc::UnboundedSender<String>,
    pub events_rx: mpsc::UnboundedReceiver<ChatEvent>,
}

/// The agent loop's side: prompts in, events out.
pub struct AgentChannels {
    pub prompt_rx: mpsc::UnboundedReceiver<String>,
    pub sink: ChatSink,
}

/// Create both ends of the panel ⇄ agent link.
pub fn create_chat_channels() -> (PanelChannels, AgentChannels) {
    let (prompt_tx, prompt_rx) = mpsc::unbounded_channel();
    let (event_tx, events_rx) = mpsc::unbounded_channel();
    (
        PanelChannels {
            prompt_tx,
            events_rx,
        },
        AgentChannels {
            prompt_rx,
            sink: ChatSink(event_tx),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_and_events_cross_the_link() {
        let (mut panel, mut agent) = create_chat_channels();

        panel.prompt_tx.send("build a castle".into()).unwrap();
        assert_eq!(agent.prompt_rx.try_recv().unwrap(), "build a castle");

        agent.sink.send(ChatEvent::Delta("Raising walls".into()));
        assert_eq!(
            panel.events_rx.try_recv().unwrap(),
            ChatEvent::Delta("Raising walls".into())
        );
    }

    #[test]
    fn sending_after_the_panel_closes_is_harmless() {
        let (panel, agent) = create_chat_channels();
        drop(panel);
        agent.sink.send(ChatEvent::Notice("still fine".into()));
    }
}
