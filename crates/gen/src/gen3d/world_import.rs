//! Open a world from outside the workspace: `localgpt-gen --world <PATH|URL>`.
//!
//! A world folder (with `world.ron`) or a world skill name opens in place. A
//! loose manifest — `.json` as the web viewer reads it, or `.ron` — and an
//! http(s) URL to one are imported first: the manifest is written to
//! `{workspace}/skills/<name>/world.ron` and every asset it references is
//! copied or downloaded into that folder's `assets/`, so the world saves,
//! grows regions and exports like any world Gen made. An existing folder of
//! that name opens as it is: an import never overwrites your edits. Delete the
//! folder to import again.
//!
//! Asset paths follow the format (`MeshAssetRef::path`, `SoundtrackDef::path`,
//! `AudioSource::File`): relative to the world's `assets/` directory, so a
//! world published at `<base>/<name>.json` keeps them under `<base>/assets/`.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use localgpt_world_types as wt;

/// Most assets one import copies or downloads.
const MAX_ASSETS: usize = 512;
/// Most bytes one import downloads.
const MAX_DOWNLOAD_BYTES: u64 = 1 << 30;

/// What `--world` resolved to.
#[derive(Debug)]
pub struct StartupWorld {
    /// The world folder to load (holds `world.ron`).
    pub dir: PathBuf,
    pub outcome: Outcome,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// A world folder or skill name, opened in place.
    Opened,
    /// Imported into the workspace just now.
    Imported { assets: usize, missing: Vec<String> },
    /// Imported on an earlier run; opened as it is.
    AlreadyImported,
}

/// Resolve `--world`: open a world folder or skill name, or import a manifest
/// file or URL into `{workspace}/skills/` first.
pub async fn prepare(arg: &str, workspace: &Path) -> Result<StartupWorld> {
    if is_remote(arg) {
        return import_remote(arg, workspace).await;
    }
    let path = PathBuf::from(shellexpand::tilde(arg).into_owned());
    for dir in [path.clone(), workspace.join("skills").join(&path)] {
        if dir.is_dir() && dir.join("world.ron").is_file() {
            let dir = dir.canonicalize().unwrap_or(dir);
            return Ok(StartupWorld {
                dir,
                outcome: Outcome::Opened,
            });
        }
    }
    if path.is_file() {
        return import_local(&path, workspace);
    }
    bail!(
        "no world at {arg}: pass a world folder (with world.ron), a world name from \
         {}, a .json or .ron manifest, or an http(s) URL to one",
        workspace.join("skills").display()
    )
}

/// An http(s) URL (anything else is a path).
pub fn is_remote(arg: &str) -> bool {
    let lower = arg.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// Parse a manifest as JSON (the web viewer's format) or RON (Gen's).
pub fn parse_manifest(text: &str, json: bool) -> Result<wt::WorldManifest> {
    let manifest: wt::WorldManifest = if json {
        serde_json::from_str(text).context("not a LocalGPT world (JSON)")?
    } else {
        ron::from_str(text).context("not a LocalGPT world (RON)")?
    };
    manifest.check_version().map_err(anyhow::Error::msg)?;
    if manifest.region_files.is_some() || manifest.layout_file.is_some() {
        bail!("a multi-file world can't be imported from its manifest alone; open its folder");
    }
    Ok(manifest)
}

/// Every asset file a manifest references, relative to the world's `assets/`.
pub fn manifest_assets(manifest: &wt::WorldManifest) -> Vec<String> {
    let mut paths = Vec::new();
    for entity in &manifest.entities {
        if let Some(mesh) = &entity.mesh_asset {
            paths.push(mesh.path.clone());
        }
        if let Some(wt::AudioSource::File { path, .. }) = entity.audio.as_ref().map(|a| &a.source) {
            paths.push(path.clone());
        }
    }
    if let Some(path) = manifest.soundtrack.as_ref().and_then(|s| s.path.clone()) {
        paths.push(path);
    }
    paths.sort();
    paths.dedup();
    paths
}

/// A folder name for the world: its name as a slug, else the fallback's.
pub fn world_slug(manifest: &wt::WorldManifest, fallback: &str) -> String {
    let slug = slugify(&manifest.meta.name);
    if slug.is_empty() {
        let fallback = slugify(fallback);
        if fallback.is_empty() {
            "world".to_string()
        } else {
            fallback
        }
    } else {
        slug
    }
}

fn slugify(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(64);
    slug.trim_end_matches('-').to_string()
}

/// A manifest's asset path as a safe relative path: no root, no `..`, no
/// query or fragment. `None` rejects the asset.
pub fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.contains(['\\', '?', '#']) {
        return None;
    }
    let mut out = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// The import's destination, or the folder an earlier import already wrote.
fn destination(workspace: &Path, name: &str) -> Result<(PathBuf, bool)> {
    let dir = workspace.join("skills").join(name);
    let exists = dir.join("world.ron").is_file();
    if !exists && dir.exists() {
        bail!(
            "{} exists but holds no world.ron; move it aside to import this world",
            dir.display()
        );
    }
    Ok((dir, exists))
}

/// Write the manifest as `world.ron` into a staging folder, then move it into
/// place so a failed import leaves nothing half-written behind.
fn finish(staging: &Path, dir: &Path, manifest: &wt::WorldManifest) -> Result<()> {
    let ron = ron::ser::to_string_pretty(manifest, ron::ser::PrettyConfig::default())
        .context("serializing world.ron")?;
    std::fs::write(staging.join("world.ron"), ron).context("writing world.ron")?;
    std::fs::rename(staging, dir)
        .with_context(|| format!("moving the import into {}", dir.display()))
}

fn staging_dir(workspace: &Path, name: &str) -> Result<PathBuf> {
    let staging = workspace.join("skills").join(format!(".import-{name}"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).ok();
    }
    std::fs::create_dir_all(&staging).with_context(|| format!("creating {}", staging.display()))?;
    Ok(staging)
}

fn import_local(file: &Path, workspace: &Path) -> Result<StartupWorld> {
    let text =
        std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?;
    let json = file
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    let manifest = parse_manifest(&text, json)?;
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = world_slug(&manifest, stem);
    let (dir, exists) = destination(workspace, &name)?;
    if exists {
        return Ok(StartupWorld {
            dir,
            outcome: Outcome::AlreadyImported,
        });
    }

    let assets = manifest_assets(&manifest);
    if assets.len() > MAX_ASSETS {
        bail!(
            "the world references {} assets (at most {MAX_ASSETS})",
            assets.len()
        );
    }
    let source_dir = file.parent().unwrap_or(Path::new("."));
    let staging = staging_dir(workspace, &name)?;
    let mut copied = 0;
    let mut missing = Vec::new();
    for asset in &assets {
        let Some(rel) = safe_relative(asset) else {
            missing.push(asset.clone());
            continue;
        };
        // The format's place first, then the folder itself (worlds Gen saved
        // with `assets/...` paths relative to the world folder).
        let from = [source_dir.join("assets").join(&rel), source_dir.join(&rel)]
            .into_iter()
            .find(|p| p.is_file());
        let Some(from) = from else {
            missing.push(asset.clone());
            continue;
        };
        let to = staging.join("assets").join(&rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&from, &to).with_context(|| format!("copying {}", from.display()))?;
        copied += 1;
    }
    if let Err(e) = finish(&staging, &dir, &manifest) {
        std::fs::remove_dir_all(&staging).ok();
        return Err(e);
    }
    Ok(StartupWorld {
        dir,
        outcome: Outcome::Imported {
            assets: copied,
            missing,
        },
    })
}

async fn import_remote(url: &str, workspace: &Path) -> Result<StartupWorld> {
    let url = reqwest::Url::parse(url).with_context(|| format!("not a URL: {url}"))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("localgpt-gen/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let text = client
        .get(url.clone())
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("downloading {url}"))?
        .text()
        .await?;
    let json = !url.path().to_ascii_lowercase().ends_with(".ron");
    let manifest = parse_manifest(&text, json)?;
    let stem = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .and_then(|last| last.rsplit_once('.').map(|(stem, _)| stem))
        .unwrap_or("");
    let name = world_slug(&manifest, stem);
    let (dir, exists) = destination(workspace, &name)?;
    if exists {
        return Ok(StartupWorld {
            dir,
            outcome: Outcome::AlreadyImported,
        });
    }

    let assets = manifest_assets(&manifest);
    if assets.len() > MAX_ASSETS {
        bail!(
            "the world references {} assets (at most {MAX_ASSETS})",
            assets.len()
        );
    }
    let base = url.join("assets/")?;
    let staging = staging_dir(workspace, &name)?;
    let result = download_assets(&client, &base, &assets, &staging).await;
    let (copied, missing) = match result {
        Ok(counts) => counts,
        Err(e) => {
            std::fs::remove_dir_all(&staging).ok();
            return Err(e);
        }
    };
    if let Err(e) = finish(&staging, &dir, &manifest) {
        std::fs::remove_dir_all(&staging).ok();
        return Err(e);
    }
    Ok(StartupWorld {
        dir,
        outcome: Outcome::Imported {
            assets: copied,
            missing,
        },
    })
}

async fn download_assets(
    client: &reqwest::Client,
    base: &reqwest::Url,
    assets: &[String],
    staging: &Path,
) -> Result<(usize, Vec<String>)> {
    let mut total = 0u64;
    let mut copied = 0;
    let mut missing = Vec::new();
    for asset in assets {
        let Some(rel) = safe_relative(asset) else {
            missing.push(asset.clone());
            continue;
        };
        let asset_url = base.join(asset)?;
        let response = client.get(asset_url.clone()).send().await;
        let response = match response.and_then(reqwest::Response::error_for_status) {
            Ok(r) => r,
            Err(_) => {
                missing.push(asset.clone());
                continue;
            }
        };
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("downloading {asset_url}"))?;
        total += bytes.len() as u64;
        if total > MAX_DOWNLOAD_BYTES {
            bail!("the world's assets exceed {} MiB", MAX_DOWNLOAD_BYTES >> 20);
        }
        let to = staging.join("assets").join(&rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&to, &bytes).with_context(|| format!("writing {}", to.display()))?;
        copied += 1;
    }
    Ok((copied, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str) -> wt::WorldManifest {
        let mut m = wt::WorldManifest::new(name);
        let mut rock = wt::WorldEntity::new(1, "rock");
        rock.mesh_asset = Some(wt::MeshAssetRef {
            path: "models/rock.glb".into(),
            node: None,
        });
        m.entities.push(rock);
        m.next_entity_id = 2;
        m
    }

    #[test]
    fn slugs_name_the_folder() {
        assert_eq!(slugify("Markdown, as a place"), "markdown-as-a-place");
        assert_eq!(slugify("  Tide — Gardens!  "), "tide-gardens");
        assert_eq!(world_slug(&manifest("***"), "Lighthouse 2"), "lighthouse-2");
        assert_eq!(world_slug(&manifest(""), ""), "world");
    }

    #[test]
    fn asset_paths_stay_inside_the_world() {
        assert_eq!(
            safe_relative("models/rock.glb"),
            Some(PathBuf::from("models/rock.glb"))
        );
        assert_eq!(
            safe_relative("./music/a.mp3"),
            Some(PathBuf::from("music/a.mp3"))
        );
        for bad in [
            "",
            "../secret",
            "models/../../x",
            "/etc/passwd",
            "a\\b",
            "x.glb?y",
            "x#y",
        ] {
            assert_eq!(safe_relative(bad), None, "{bad}");
        }
    }

    #[test]
    fn remote_is_http_only() {
        assert!(is_remote("https://localgpt.world/worlds/a.json"));
        assert!(is_remote("HTTP://example.com/a.ron"));
        assert!(!is_remote("file:///tmp/a.json"));
        assert!(!is_remote("worlds/a.json"));
    }

    #[test]
    fn assets_include_meshes_audio_files_and_the_soundtrack() {
        let mut m = manifest("w");
        let mut drum = wt::WorldEntity::new(2, "drum");
        drum.audio = Some(wt::AudioDef {
            kind: wt::AudioKind::Sfx,
            source: wt::AudioSource::File {
                path: "audio/drum.ogg".into(),
                looping: true,
            },
            volume: 1.0,
            radius: Some(8.0),
            rolloff: Default::default(),
        });
        m.entities.push(drum);
        let mut twin = wt::WorldEntity::new(3, "rock-2");
        twin.mesh_asset = Some(wt::MeshAssetRef {
            path: "models/rock.glb".into(),
            node: None,
        });
        m.entities.push(twin);
        m.soundtrack = Some(wt::SoundtrackDef {
            path: Some("music/song.mp3".into()),
            ..Default::default()
        });
        assert_eq!(
            manifest_assets(&m),
            ["audio/drum.ogg", "models/rock.glb", "music/song.mp3"]
        );
    }

    #[test]
    fn parses_json_and_ron() {
        let m = manifest("Round Trip");
        let json = serde_json::to_string(&m).unwrap();
        let ron = ron::to_string(&m).unwrap();
        assert_eq!(parse_manifest(&json, true).unwrap().meta.name, "Round Trip");
        assert_eq!(parse_manifest(&ron, false).unwrap().meta.name, "Round Trip");
        assert!(parse_manifest("{\"hello\": 1}", true).is_err());
    }

    #[test]
    fn imports_a_loose_manifest_with_its_assets_once() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        let site = tmp.path().join("site");
        std::fs::create_dir_all(site.join("assets/models")).unwrap();
        std::fs::write(site.join("assets/models/rock.glb"), b"glb").unwrap();
        let file = site.join("rocks.json");
        std::fs::write(
            &file,
            serde_json::to_string(&manifest("Rock Garden")).unwrap(),
        )
        .unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let first = rt
            .block_on(prepare(file.to_str().unwrap(), &workspace))
            .unwrap();
        let dir = workspace.join("skills/rock-garden");
        assert_eq!(first.dir, dir);
        assert_eq!(
            first.outcome,
            Outcome::Imported {
                assets: 1,
                missing: vec![]
            }
        );
        assert!(dir.join("world.ron").is_file());
        assert!(dir.join("assets/models/rock.glb").is_file());

        // Edits survive: a second import opens the folder as it is.
        std::fs::write(dir.join("assets/models/rock.glb"), b"edited").unwrap();
        let again = rt
            .block_on(prepare(file.to_str().unwrap(), &workspace))
            .unwrap();
        assert_eq!(again.outcome, Outcome::AlreadyImported);
        assert_eq!(
            std::fs::read(dir.join("assets/models/rock.glb")).unwrap(),
            b"edited"
        );

        // The folder and the skill name open in place.
        let by_name = rt.block_on(prepare("rock-garden", &workspace)).unwrap();
        assert_eq!(by_name.outcome, Outcome::Opened);
        let by_dir = rt
            .block_on(prepare(dir.to_str().unwrap(), &workspace))
            .unwrap();
        assert_eq!(by_dir.outcome, Outcome::Opened);
    }

    #[test]
    fn a_missing_asset_is_reported_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("bare.json");
        std::fs::write(&file, serde_json::to_string(&manifest("Bare")).unwrap()).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let world = rt
            .block_on(prepare(file.to_str().unwrap(), &tmp.path().join("ws")))
            .unwrap();
        assert_eq!(
            world.outcome,
            Outcome::Imported {
                assets: 0,
                missing: vec!["models/rock.glb".into()]
            }
        );
    }

    #[test]
    fn mesh_paths_resolve_in_assets_then_the_world_folder() {
        use crate::gen3d::plugin::resolve_mesh_asset_path;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let rock = dir.join("assets/models/rock.glb");
        std::fs::create_dir_all(rock.parent().unwrap()).unwrap();
        std::fs::write(&rock, b"glb").unwrap();
        // The format: relative to the world's assets/ folder.
        assert_eq!(resolve_mesh_asset_path(Some(dir), "models/rock.glb"), rock);
        // Gen's earlier saves: assets/... relative to the world folder.
        assert_eq!(
            resolve_mesh_asset_path(Some(dir), "assets/models/rock.glb"),
            rock
        );
        // Absolute paths stay as they are.
        assert_eq!(
            resolve_mesh_asset_path(Some(dir), rock.to_str().unwrap()),
            rock
        );
    }

    #[test]
    fn nothing_there_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt
            .block_on(prepare("no-such-world", tmp.path()))
            .unwrap_err();
        assert!(err.to_string().contains("no world at no-such-world"));
    }
}
