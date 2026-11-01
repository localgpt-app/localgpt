//! Gen's whole configuration, built in memory: where its worlds and its
//! settings live.
//!
//! Gen reads no config file, holds no model and asks for no key: an agent
//! outside the window brings its own. What Gen remembers is in
//! [`crate::settings`]; everything else is a directory, resolved by the rule
//! every LocalGPT app shares (`localgpt_world_agent::paths::Dirs`), so the
//! `LOCALGPT_*` overrides and the profile keep working.

use std::path::{Path, PathBuf};

use localgpt_world_agent::paths::Dirs;

/// Where Gen keeps things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenConfig {
    /// Gen's workspace: `skills/` (saved worlds), `sessions/`, `exports/`,
    /// `screenshots/`, `documents/` (the app's), `generated/`.
    pub workspace: PathBuf,
    /// Where Gen remembers things: its settings, experiments once, the app's
    /// paired devices and its log.
    pub state_dir: PathBuf,
}

/// Gen's configuration, resolved from the environment. The first run with
/// worlds saved by the assistant copies them into Gen's own workspace.
pub fn gen_config() -> GenConfig {
    let dirs = Dirs::resolve().unwrap_or_else(|| Dirs {
        data: PathBuf::from("."),
        state: PathBuf::from("."),
    });
    let config = config_in(&dirs);
    migrate_legacy_worlds(&dirs.data.join("workspace"), &config.workspace);
    config
}

/// Gen's directories under `dirs`. Writes nothing.
pub fn config_in(dirs: &Dirs) -> GenConfig {
    // Gen's own workspace: its worlds, gallery and sessions are its own, not
    // the assistant's `workspace/` the earlier LocalGPT kept beside it.
    GenConfig {
        workspace: dirs.data.join("gen-workspace"),
        state_dir: dirs.state.clone(),
    }
}

/// Copy worlds saved before Gen had its own workspace into it, once.
///
/// Worlds used to land in the assistant's `workspace/skills/`, where they also
/// became skills in its system prompt. Gen reads its own workspace now, so
/// without this a user's saved worlds would look like they had vanished.
/// Copy, never move: the originals stay where they are, and the user decides
/// whether to remove them.
pub fn migrate_legacy_worlds(legacy_workspace: &Path, gen_workspace: &Path) {
    let (from, to) = (
        legacy_workspace.join("skills"),
        gen_workspace.join("skills"),
    );
    // Only on a first run — once Gen has its own skills dir, leave it alone.
    if to.exists() || !from.is_dir() {
        return;
    }

    let Ok(entries) = std::fs::read_dir(&from) else {
        return;
    };
    let mut copied = Vec::new();
    for entry in entries.flatten() {
        let source = entry.path();
        // A world, not one of the assistant's own skills.
        if !source.join("world.ron").is_file() {
            continue;
        }
        let Some(name) = source.file_name() else {
            continue;
        };
        match copy_dir(&source, &to.join(name)) {
            Ok(()) => copied.push(name.to_string_lossy().into_owned()),
            Err(e) => tracing::warn!("couldn't copy world {}: {e}", source.display()),
        }
    }

    if !copied.is_empty() {
        copied.sort();
        eprintln!(
            "Copied {} world{} into Gen's own workspace ({}): {}.\nThe originals are untouched in \
             {} — delete them there if you no longer want them.",
            copied.len(),
            if copied.len() == 1 { "" } else { "s" },
            gen_workspace.display(),
            copied.join(", "),
            from.display()
        );
    }
}

/// Recursively copy a directory. Files only — no symlink following, so a link
/// in a world folder can't copy something from outside it.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if kind.is_dir() {
            copy_dir(&source, &target)?;
        } else if kind.is_file() {
            std::fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gen's directories sit under the app directories, in its own
    /// workspace rather than the assistant's.
    #[test]
    fn gen_keeps_its_own_workspace_under_the_app_dirs() {
        let dirs = Dirs {
            data: PathBuf::from("/data/localgpt"),
            state: PathBuf::from("/state/localgpt"),
        };
        let config = config_in(&dirs);
        assert_eq!(
            config.workspace,
            PathBuf::from("/data/localgpt/gen-workspace")
        );
        assert_eq!(config.state_dir, PathBuf::from("/state/localgpt"));
    }

    /// The migration copies worlds and leaves the originals alone.
    #[test]
    fn legacy_worlds_are_copied_not_moved() {
        let tmp = tempfile::tempdir().unwrap();
        let (legacy, gen_ws) = (tmp.path().join("legacy"), tmp.path().join("gen"));
        let world = legacy.join("skills").join("medieval-village");
        std::fs::create_dir_all(world.join("assets")).unwrap();
        std::fs::write(world.join("world.ron"), "(name:\"village\")").unwrap();
        std::fs::write(world.join("assets").join("a.txt"), "x").unwrap();
        // An assistant skill, which is not a world and must not be copied.
        let skill = legacy.join("skills").join("research");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "# research").unwrap();

        migrate_legacy_worlds(&legacy, &gen_ws);

        assert!(gen_ws.join("skills/medieval-village/world.ron").is_file());
        assert!(
            gen_ws
                .join("skills/medieval-village/assets/a.txt")
                .is_file(),
            "nested files should come too"
        );
        assert!(
            !gen_ws.join("skills/research").exists(),
            "only worlds migrate"
        );
        assert!(world.join("world.ron").is_file(), "originals stay put");

        // Running again must not touch an existing Gen workspace.
        std::fs::remove_dir_all(gen_ws.join("skills/medieval-village")).unwrap();
        migrate_legacy_worlds(&legacy, &gen_ws);
        assert!(!gen_ws.join("skills/medieval-village").exists());
    }
}
