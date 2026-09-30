//! Time-lapse replay of a session (`--replay <session | package | ops.jsonl>`).
//!
//! Opens the normal window with no agent and applies the log's batches on a
//! timer — the world builds itself again in front of you. The first tick
//! clears the startup scene (its default ground/sun would collide with the
//! replayed ids, same as a resume rebuild).

use std::collections::VecDeque;

use bevy::prelude::*;
use localgpt_world_sync as sync;
use localgpt_world_sync::OpLogEntry;
use localgpt_world_types as wt;

use super::ops_apply::OpsApplier;

/// The remaining log plus pacing state.
#[derive(Resource)]
pub struct ReplayState {
    entries: VecDeque<OpLogEntry>,
    timer: Timer,
    cleared: bool,
    total: usize,
}

/// Read an `ops.jsonl` — or a session package directory, whose base world
/// becomes the first batch — into replay order, skipping unreadable lines.
/// A legacy session directory (a log, no metadata) reads its log directly,
/// and a `.world` archive extracts to a temp directory first.
pub fn load_op_log(path: &str) -> anyhow::Result<VecDeque<OpLogEntry>> {
    let mut resolved = std::path::PathBuf::from(shellexpand::tilde(path).as_ref());
    if resolved.is_file() && resolved.extension().is_some_and(|e| e == "world") {
        let temp = localgpt_world_agent::session::extract_zip(&resolved)
            .map_err(|e| anyhow::anyhow!("can't read {}: {e}", resolved.display()))?;
        let pkg = localgpt_world_agent::session::read_package(temp.path(), None)
            .map_err(|e| anyhow::anyhow!("can't read {}: {e}", resolved.display()))?;
        return package_entries(pkg);
    }
    if resolved.is_dir() {
        match localgpt_world_agent::session::read_package(&resolved, None) {
            Ok(pkg) => return package_entries(pkg),
            Err(localgpt_world_agent::session::PackageError::MissingMeta) => {
                resolved = resolved.join(localgpt_world_agent::session::LOG_FILE);
            }
            Err(e) => anyhow::bail!("can't read package {}: {e}", resolved.display()),
        }
    }
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

/// A package's replay order: its base world as the first batch, then the log.
fn package_entries(
    pkg: localgpt_world_agent::session::SessionPackage,
) -> anyhow::Result<VecDeque<OpLogEntry>> {
    let mut entries = VecDeque::new();
    let mut base_ops: Vec<sync::SessionOp> = pkg
        .base
        .entities
        .iter()
        .cloned()
        .map(|e| sync::SessionOp::Edit(Box::new(wt::EditOp::spawn(e))))
        .collect();
    if let Some(env) = pkg.base.environment.clone() {
        base_ops.push(sync::SessionOp::Edit(Box::new(
            wt::EditOp::SetEnvironment { env },
        )));
    }
    if let Some(camera) = pkg.base.camera.clone() {
        base_ops.push(sync::SessionOp::Edit(Box::new(wt::EditOp::SetCamera {
            camera,
        })));
    }
    if !base_ops.is_empty() {
        entries.push_back(OpLogEntry {
            revision: pkg.base_revision,
            author: sync::Author {
                peer: None,
                name: "base".into(),
            },
            ops: base_ops,
            timestamp_ms: 0,
            id: None,
            parent: None,
        });
    }
    entries.extend(pkg.entries);
    if entries.is_empty() {
        anyhow::bail!("package {} holds no batches", pkg.base.meta.name);
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
    applier.apply_ops(&entry.edit_ops());
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A `.world` archive extracts and loads as a package: the base world
    /// becomes the first batch, then the log.
    #[test]
    fn a_world_archive_replays() {
        use localgpt_world_agent::session as pkg;
        use localgpt_world_sync::{Author, SessionOp};
        use std::sync::atomic::{AtomicU64, Ordering};

        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "localgpt-replay-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut manifest = wt::WorldManifest::new("archived");
        manifest.entities.push(wt::WorldEntity::new(1, "keep"));
        pkg::write_base(&dir, &manifest, "gen", None, 0).unwrap();
        pkg::append_entry(
            &dir,
            &localgpt_world_sync::OpLogEntry {
                revision: 1,
                author: Author {
                    peer: None,
                    name: "maya".into(),
                },
                ops: vec![SessionOp::Edit(Box::new(wt::EditOp::spawn(
                    wt::WorldEntity::new(2, "lighthouse"),
                )))],
                timestamp_ms: 0,
                id: None,
                parent: None,
            },
        )
        .unwrap();

        let zip = dir.parent().unwrap().join(format!(
            "localgpt-replay-{}.world",
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        pkg::export_zip(&dir, &zip).unwrap();

        let entries = load_op_log(zip.to_str().unwrap()).unwrap();
        assert!(entries.len() >= 2); // the base batch, then the spawn
        assert!(entries.iter().any(|e| e.revision == 1));

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&zip);
    }
}
