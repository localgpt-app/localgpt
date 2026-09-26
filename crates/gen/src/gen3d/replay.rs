//! Time-lapse replay of a session's op log (`--replay <ops.jsonl>`).
//!
//! Opens the normal window with no agent and applies the log's batches on a
//! timer — the world builds itself again in front of you. The first tick
//! clears the startup scene (its default ground/sun would collide with the
//! replayed ids, same as a resume rebuild).

use std::collections::VecDeque;

use bevy::prelude::*;
use localgpt_world_sync::OpLogEntry;

use super::ops_apply::OpsApplier;

/// The remaining log plus pacing state.
#[derive(Resource)]
pub struct ReplayState {
    entries: VecDeque<OpLogEntry>,
    timer: Timer,
    cleared: bool,
    total: usize,
}

/// Read an `ops.jsonl` into replay order, skipping unreadable lines.
pub fn load_op_log(path: &str) -> anyhow::Result<VecDeque<OpLogEntry>> {
    let resolved = std::path::PathBuf::from(shellexpand::tilde(path).as_ref());
    let content = std::fs::read_to_string(&resolved)
        .map_err(|e| anyhow::anyhow!("can't read {}: {e}", resolved.display()))?;
    let mut entries = VecDeque::new();
    for (n, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match localgpt_world_sync::decode_line(line) {
            Ok(entry) => entries.push_back(entry),
            Err(e) => eprintln!("[replay] skipping unreadable line {}: {e}", n + 1),
        }
    }
    if entries.is_empty() {
        anyhow::bail!("no op batches in {}", resolved.display());
    }
    Ok(entries)
}

/// Insert the replay resource and tick system into the app.
pub fn setup_replay(app: &mut App, entries: VecDeque<OpLogEntry>, batches_per_second: f32) {
    let total = entries.len();
    app.insert_resource(ReplayState {
        entries,
        timer: Timer::from_seconds(1.0 / batches_per_second.max(0.1), TimerMode::Repeating),
        cleared: false,
        total,
    })
    .add_systems(Update, replay_tick);
}

fn replay_tick(time: Res<Time>, mut state: ResMut<ReplayState>, mut applier: OpsApplier) {
    if !state.timer.tick(time.delta()).just_finished() {
        return;
    }
    if !state.cleared {
        // Start from an empty scene so replayed ids don't fight the startup
        // world's defaults.
        state.cleared = true;
        applier.rebuild_scene(&[]);
    }
    let Some(entry) = state.entries.pop_front() else {
        return;
    };
    let left = state.entries.len();
    applier.apply_ops(&entry.ops);
    if left.is_multiple_of(20) || left == 0 {
        eprintln!(
            "[replay] rev {} · {} batches left",
            entry.revision,
            state.entries.len()
        );
    }
    if left == 0 {
        eprintln!(
            "[replay] done — {} batches, revision {}",
            state.total, entry.revision
        );
    }
}
