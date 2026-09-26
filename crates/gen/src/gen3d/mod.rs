//! LocalGPT Gen — AI-driven 3D scene generation via Bevy
//!
//! This module provides in-process 3D rendering and scene composition
//! controlled by the LLM agent through intent-level tools.
//!
//! Architecture: Agent loop (tokio) ←mpsc channels→ Bevy App (main thread)

pub mod asset_gen;
pub mod audio;
pub mod audio_graphs;
pub mod avatar;
pub mod behaviors;
pub mod commands;
pub mod compat;
pub mod gallery;
pub mod gallery_ui;
pub mod generation_log;
pub mod gltf_export;
pub mod headless;
pub mod mcp_relay;
pub mod model_server;
pub mod offscreen;
pub mod ops_apply;
pub mod pending_writes;
pub mod plugin;
pub mod region_dirty;
pub mod registry;
pub mod replay;
pub mod sync;
pub mod system_prompt;
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
pub struct GenBridge {
    cmd_tx: mpsc::UnboundedSender<GenCommand>,
    resp_rx: Mutex<mpsc::UnboundedReceiver<GenResponse>>,
}

impl GenBridge {
    /// Send a command to Bevy and await the response.
    pub async fn send(&self, cmd: GenCommand) -> anyhow::Result<GenResponse> {
        self.cmd_tx
            .send(cmd)
            .map_err(|_| anyhow::anyhow!("Gen bridge closed — Bevy app may have exited"))?;
        let mut rx = self.resp_rx.lock().await;
        rx.recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Gen bridge closed — no response from Bevy"))
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
        resp_rx: Mutex::new(resp_rx),
    });

    let channels = GenChannels {
        cmd_rx,
        resp_tx,
        cmd_tx,
    };

    (bridge, channels)
}
