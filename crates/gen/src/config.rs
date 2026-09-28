//! Gen's whole configuration, built in memory.
//!
//! Lifted from `main.rs` so every Gen-shaped binary — `localgpt-gen` and the
//! one-window `localgpt-app` — builds its `Config` through the same path.
//! A second copy in the app would have reintroduced exactly the coupling the
//! settings commit removed: `Config::load()` writing the assistant's
//! `config.toml` on first run and reading the assistant's workspace.

use std::path::Path;

/// Gen's whole configuration, built in memory.
///
/// Gen is a desktop app, so it must open on a machine with nothing on disk and
/// must not write into the assistant's `~/.config/localgpt/config.toml` — the
/// old `Config::load()` created that file on first run. Everything Gen
/// remembers is in [`crate::settings`]; everything else is a default,
/// with the handful of fields below set explicitly because a defaulted
/// `Config` gets them wrong for Gen.
///
/// Paths still come from `Paths::resolve()` via `Config::default()`, so every
/// `LOCALGPT_*` override and the XDG rules keep working.
pub fn gen_config() -> localgpt_core::config::Config {
    let mut config = localgpt_core::config::Config::default();
    let legacy_workspace = config.paths.workspace.clone();

    // First run with a config.toml already on disk: adopt what the user chose
    // there once, so upgrading doesn't silently reset their model.
    let settings = load_or_import_settings(&config);
    apply_gen_defaults(&mut config, &settings);
    migrate_legacy_worlds(&legacy_workspace, &config.paths.workspace);
    config
}

/// Gen's settings, importing from a pre-existing config file the first time.
fn load_or_import_settings(config: &localgpt_core::config::Config) -> crate::settings::GenSettings {
    use crate::settings;

    if let Some(settings) = settings::load() {
        return settings;
    }
    let imported = import_legacy_settings(config);
    if imported != settings::GenSettings::default()
        && let Err(e) = settings::save(&imported)
    {
        tracing::warn!("couldn't save the settings imported from config.toml: {e}");
    }
    imported
}

/// The fields a defaulted core `Config` gets wrong for Gen. Writes nothing.
pub fn apply_gen_defaults(
    config: &mut localgpt_core::config::Config,
    settings: &crate::settings::GenSettings,
) {
    // Gen's own workspace: its worlds, gallery and style memory are its own,
    // not the assistant's. Sharing one meant every saved world showed up as a
    // skill in `localgpt chat`'s system prompt, and Gen's memory writes landed
    // in the assistant's MEMORY.md. Same move `build_scoped_remote_agent`
    // already makes for guests.
    config.paths.workspace = config.paths.data_dir.join("gen-workspace");

    // Scene building repeats tools by design (spawn 40 primitives, check
    // scene_info between steps); core's default of 3 aborts mid-scene.
    config.agent.max_tool_repeats = config.agent.max_tool_repeats.max(20);

    // Local embeddings would block the first launch on an ~80 MB model
    // download. Keyword search still works, and nothing in Gen needs vectors
    // to build a world.
    config.memory.embedding_provider = "none".to_string();

    config.agent.default_model = choose_model(settings.default_model.as_deref(), config);

    // Subagents otherwise default to claude-cli/sonnet, which would quietly
    // reach for a different backend than the one Gen is running on.
    config.agent.subagent_model = Some(config.agent.default_model.clone());

    config.r#gen.tool_profile = settings.tool_profile.clone();
}

/// Whether Gen can actually run this model. Gen asks for no credentials, so a
/// model needing an API key can't be started from a bare install — a signed-in
/// CLI backend or a model on disk can.
pub fn is_credential_free(model: &str) -> bool {
    const PREFIXES: &[&str] = &["claude-cli/", "gemini-cli/", "codex-cli/", "gguf/"];
    PREFIXES.iter().any(|prefix| model.starts_with(prefix))
}

/// Copy worlds saved before Gen had its own workspace into it, once.
///
/// Worlds used to land in the assistant's `workspace/skills/`, where they also
/// became skills in its system prompt. Gen reads its own workspace now, so
/// without this a user's saved worlds would look like they had vanished.
/// Copy, never move: the originals stay where they are, and the user decides
/// whether to remove them from the assistant.
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
             {} — you can delete them there if you don't want them in the assistant's skills.",
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

/// Decide what to run on: the remembered choice, else whatever this machine
/// already has. Every candidate here is credential-free — a CLI backend the
/// user has already signed into, or a local GGUF — because a desktop app can't
/// ask for an API key before it will open.
pub fn choose_model(remembered: Option<&str>, config: &localgpt_core::config::Config) -> String {
    use crate::desktop::{hardware, models};

    if let Some(choice) = remembered.map(str::trim).filter(|c| !c.is_empty()) {
        if choice.eq_ignore_ascii_case("auto") {
            return match models::auto_local_model() {
                Some(model) => {
                    eprintln!("Model: {model} (auto — {})", hardware::summary());
                    model
                }
                None => {
                    eprintln!(
                        "Model: {} (\"auto\" found no local model that fits this machine — {})",
                        config.agent.default_model,
                        hardware::summary()
                    );
                    config.agent.default_model.clone()
                }
            };
        }
        return choice.to_string();
    }

    // Nothing remembered: first run. Prefer a local model if one is already
    // downloaded, else a CLI backend that's installed.
    if let Some(model) = models::auto_local_model() {
        eprintln!(
            "Model: {model} (found on this machine — {})",
            hardware::summary()
        );
        return model;
    }
    for (program, candidates) in models::CLI_BACKENDS {
        if models::find_on_path(program).is_some()
            && let Some(model) = candidates.first()
        {
            eprintln!("Model: {model} ({program} is installed)");
            return (*model).to_string();
        }
    }
    // Nothing found. Keep the default so the panel can explain what to install
    // (missing_cli_backend_hint) rather than refusing to start.
    config.agent.default_model.clone()
}

/// One-time import of a pre-existing `config.toml`, so a user who set
/// `[gen] default_model` (or just `agent.default_model`) keeps it. Parsed
/// loosely and read-only: a missing or malformed file just means no import,
/// and Gen never writes there again.
fn import_legacy_settings(config: &localgpt_core::config::Config) -> crate::settings::GenSettings {
    let mut imported = crate::settings::GenSettings::default();
    let Some(legacy) = localgpt_core::config::Config::peek_gen_migration(&config.paths) else {
        return imported;
    };
    if legacy.is_empty() {
        return imported;
    }

    imported.tool_profile = legacy.gen_tool_profile;

    // Only adopt a model Gen can start on its own. The assistant's model is
    // often an API one whose key lives in the config file Gen no longer reads,
    // so importing it would hand Gen a backend that fails at the first prompt;
    // leaving it unset lets auto-detection find something that works.
    let candidate = legacy.gen_default_model.or(legacy.agent_default_model);
    match candidate {
        Some(model) if model.eq_ignore_ascii_case("auto") || is_credential_free(&model) => {
            eprintln!(
                "Using {model} from {} — Gen keeps its own settings now and won't read that file \
                 again.",
                legacy.source.display()
            );
            imported.default_model = Some(model);
        }
        Some(model) => eprintln!(
            "{} sets {model}, which needs an API key Gen doesn't ask for — picking a model this \
             computer can run instead. Change it in the model menu.",
            legacy.source.display()
        ),
        None => {}
    }
    imported
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[gen] default_model` overrides the assistant's model for Gen only.
    #[test]
    fn a_remembered_model_is_used_verbatim() {
        let mut config = localgpt_core::config::Config::default();
        config.agent.default_model = "claude-cli/opus".to_string();

        assert_eq!(choose_model(Some("gguf/bonsai"), &config), "gguf/bonsai");
        // Blank or whitespace means "nothing remembered", not "no model".
        assert!(!choose_model(Some("   "), &config).is_empty());
        assert!(!choose_model(None, &config).is_empty());
    }

    /// Gen's config comes from its own settings, and the fields a defaulted
    /// core Config gets wrong for Gen are set explicitly.
    #[test]
    fn gen_config_is_built_for_gen_not_the_assistant() {
        // apply_gen_defaults, not gen_config(): the latter reads and writes
        // real files, which a test must not do.
        let tmp = tempfile::tempdir().unwrap();
        let mut config = localgpt_core::config::Config::default();
        config.paths.data_dir = tmp.path().to_path_buf();
        apply_gen_defaults(&mut config, &crate::settings::GenSettings::default());

        // Its own workspace, not the assistant's.
        assert!(
            config.paths.workspace.ends_with("gen-workspace"),
            "{:?}",
            config.paths.workspace
        );
        // Scene building needs a high repeat ceiling.
        assert!(config.agent.max_tool_repeats >= 20);
        // No ~80 MB embedding download on first launch.
        assert_eq!(config.memory.embedding_provider, "none");
        // Subagents run on the same backend as Gen, not claude-cli/sonnet.
        assert_eq!(
            config.agent.subagent_model.as_deref(),
            Some(config.agent.default_model.as_str())
        );
        assert!(!config.agent.default_model.is_empty());
    }

    /// The trap this refactor turns on: `MemoryManager::new_with_agent`
    /// discards `config.paths` and re-resolves from the environment, so Gen
    /// would keep using the assistant's workspace while looking configured
    /// otherwise. `new_with_full_config` is what actually honours it.
    #[test]
    fn memory_lands_in_gens_workspace_not_the_assistants() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = localgpt_core::config::Config::default();
        config.paths.data_dir = tmp.path().to_path_buf();
        config.paths.cache_dir = tmp.path().join("cache");
        apply_gen_defaults(&mut config, &crate::settings::GenSettings::default());
        let gen_workspace = config.paths.workspace.clone();

        let _memory = localgpt_core::memory::MemoryManager::new_with_full_config(
            &config.memory,
            Some(&config),
            "gen",
        )
        .expect("memory manager should initialise");

        // init_workspace materialises the workspace it was given.
        assert!(
            gen_workspace.join("MEMORY.md").is_file(),
            "expected MEMORY.md in {gen_workspace:?}"
        );
        assert!(
            gen_workspace.ends_with("gen-workspace"),
            "{gen_workspace:?}"
        );
    }

    /// Gen asks for no credentials, so it must not adopt a model it can't
    /// start — the assistant's model is often an API one whose key is in the
    /// config file Gen no longer reads.
    #[test]
    fn only_models_gen_can_start_are_credential_free() {
        for model in [
            "claude-cli/opus",
            "gemini-cli/gemini-3.1-pro-preview",
            "codex-cli/o4-mini",
            "gguf/prism-ml_Bonsai-8B-unpacked-Q4_K_M",
        ] {
            assert!(is_credential_free(model), "{model} needs no key");
        }
        for model in [
            "glm/glm-5.3",
            "anthropic/claude-opus-4-6",
            "openai/gpt-4o",
            "xai/grok-3-mini",
            "openai-compat/deepseek-chat",
        ] {
            assert!(!is_credential_free(model), "{model} needs a key");
        }
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
