//! LocalGPT Gen — the Bevy side of the world builder.
//!
//! In-process 3D rendering and scene composition, driven through
//! intent-level tools by an agent outside the window.
//!
//! Architecture: the tool server (tokio) ←mpsc channels→ Bevy App (main
//! thread). A tool sends a `GenCommand` through the [`GenBridge`]; the
//! plugin polls the command channel each frame and answers.

pub mod audio;
pub mod avatar;
pub mod behaviors;
pub mod board;
pub mod canvas_edit;
pub mod commands;
pub mod compat;
pub mod extras;
pub mod gallery;
pub mod generation_log;
pub mod gltf_export;
pub mod headless;
pub mod live;
pub mod mcp_relay;
pub mod offscreen;
pub mod ops_apply;
pub mod pending_writes;
pub mod plugin;
pub mod region_dirty;
pub mod registry;
pub mod replay;
pub mod shots;
pub mod sync;
pub mod tools;
pub mod world;
pub mod world_import;

use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use commands::{GenCommand, GenResponse};

/// Bridge between the async agent loop and the Bevy main thread.
///
/// Tools send commands through this bridge and await responses.
/// The Bevy GenPlugin polls the command channel each frame and
/// sends responses back.
///
/// Responses carry no request id: Bevy answers each command once, and some
/// answers wait frames (a screenshot) while later commands answer at once.
/// So one call at a time owns the channel from sending its command to
/// receiving its answer, and stdio, HTTP and the relay can call together
/// without taking each other's answers.
pub struct GenBridge {
    cmd_tx: mpsc::UnboundedSender<GenCommand>,
    replies: Mutex<Replies>,
}

struct Replies {
    rx: mpsc::UnboundedReceiver<GenResponse>,
    /// Answers still due to calls dropped while waiting (a client that hung
    /// up). They are taken and discarded before the next command goes out.
    owed: usize,
}

impl GenBridge {
    /// Send a command to Bevy and await the response.
    pub async fn send(&self, cmd: GenCommand) -> anyhow::Result<GenResponse> {
        let closed = || anyhow::anyhow!("Gen bridge closed — no response from Bevy");
        let mut replies = self.replies.lock().await;
        while replies.owed > 0 {
            replies.rx.recv().await.ok_or_else(closed)?;
            replies.owed -= 1;
        }
        self.cmd_tx
            .send(cmd)
            .map_err(|_| anyhow::anyhow!("Gen bridge closed — Bevy app may have exited"))?;
        // Owed until it arrives: if this future is dropped while waiting,
        // the next call discards the answer instead of receiving it.
        replies.owed = 1;
        let response = replies.rx.recv().await.ok_or_else(closed)?;
        replies.owed = 0;
        Ok(response)
    }
}

/// Channels held by the Bevy side (inserted as a Resource).
pub struct GenChannels {
    pub cmd_rx: mpsc::UnboundedReceiver<GenCommand>,
    pub resp_tx: mpsc::UnboundedSender<GenResponse>,
    /// Sender for injecting commands from Bevy-internal systems (e.g., gallery load).
    pub cmd_tx: mpsc::UnboundedSender<GenCommand>,
}

/// Create a matched pair of (GenBridge for agent, GenChannels for Bevy).
pub fn create_gen_channels() -> (Arc<GenBridge>, GenChannels) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (resp_tx, resp_rx) = mpsc::unbounded_channel();

    let bridge = Arc::new(GenBridge {
        cmd_tx: cmd_tx.clone(),
        replies: Mutex::new(Replies {
            rx: resp_rx,
            owed: 0,
        }),
    });

    let channels = GenChannels {
        cmd_rx,
        resp_tx,
        cmd_tx,
    };

    (bridge, channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name an `EntityInfo` command asked about, echoed in an error.
    fn echo(cmd: GenCommand) -> GenResponse {
        match cmd {
            GenCommand::EntityInfo { name } => GenResponse::Error { message: name },
            _ => GenResponse::Error {
                message: String::new(),
            },
        }
    }

    fn asked(name: &str) -> GenCommand {
        GenCommand::EntityInfo { name: name.into() }
    }

    fn answer(response: GenResponse) -> String {
        match response {
            GenResponse::Error { message } => message,
            _ => panic!("expected the echo"),
        }
    }

    #[tokio::test]
    async fn concurrent_calls_each_get_their_own_answer() {
        let (bridge, mut channels) = create_gen_channels();
        // Bevy's side: answer each command as it arrives.
        tokio::spawn(async move {
            while let Some(cmd) = channels.cmd_rx.recv().await {
                tokio::task::yield_now().await;
                let _ = channels.resp_tx.send(echo(cmd));
            }
        });
        let mut calls = tokio::task::JoinSet::new();
        for i in 0..32 {
            let bridge = bridge.clone();
            calls.spawn(async move {
                let name = format!("e{i}");
                (answer(bridge.send(asked(&name)).await.unwrap()), name)
            });
        }
        while let Some(call) = calls.join_next().await {
            let (got, want) = call.unwrap();
            assert_eq!(got, want);
        }
    }

    #[tokio::test]
    async fn an_abandoned_call_does_not_answer_the_next() {
        let (bridge, mut channels) = create_gen_channels();
        let (release_tx, mut release_rx) = mpsc::unbounded_channel::<()>();
        // Bevy's side: hold each answer until released.
        tokio::spawn(async move {
            while let Some(cmd) = channels.cmd_rx.recv().await {
                release_rx.recv().await;
                let _ = channels.resp_tx.send(echo(cmd));
            }
        });
        // The first caller gives up before its answer comes.
        let first = bridge.send(asked("first"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), first)
                .await
                .is_err()
        );
        release_tx.send(()).unwrap();
        release_tx.send(()).unwrap();
        assert_eq!(
            answer(bridge.send(asked("second")).await.unwrap()),
            "second"
        );
    }
}
