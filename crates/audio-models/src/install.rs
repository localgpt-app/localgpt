//! Installing and removing models.
//!
//! A model is a directory under [`models_dir`] named by its id. Each file is
//! downloaded beside its final name, hashed as it arrives, and renamed into
//! place only when its SHA-256 is the pinned one, so a directory never holds
//! a file that was not checked. Installed means every file is there at its
//! pinned size.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::catalog::{Model, ModelFile};

/// Moves the models elsewhere (tests, a shared machine).
pub const LOCALGPT_AUDIO_MODELS: &str = "LOCALGPT_AUDIO_MODELS";

/// Where the models are installed: `$LOCALGPT_AUDIO_MODELS`, else
/// `models/audio/` under the data directory every LocalGPT app shares
/// (`~/.local/share/localgpt/models/audio` by default).
pub fn models_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(LOCALGPT_AUDIO_MODELS) {
        return Some(PathBuf::from(dir));
    }
    let dirs = localgpt_world_agent::paths::Dirs::resolve()?;
    Some(dirs.data.join("models").join("audio"))
}

/// Whether `model` is installed under `dir`.
pub fn installed(dir: &Path, model: &Model) -> bool {
    let home = dir.join(model.id);
    model.files.iter().all(|file| {
        std::fs::metadata(home.join(file.name)).is_ok_and(|m| m.is_file() && m.len() == file.bytes)
    })
}

/// How far an install has got.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub file: &'static str,
    /// Bytes of this model received so far, across its files.
    pub done: u64,
    /// The model's download, in bytes.
    pub total: u64,
}

/// Install `model` under `dir`, reporting progress as it downloads. Files
/// already in place and checked are kept.
pub async fn install(dir: &Path, model: &Model, mut progress: impl FnMut(Progress)) -> Result<()> {
    let home = dir.join(model.id);
    tokio::fs::create_dir_all(&home)
        .await
        .with_context(|| format!("creating {}", home.display()))?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("localgpt/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;
    let total = model.bytes();
    let mut before = 0;
    for file in model.files {
        let dest = home.join(file.name);
        if !matches_pin(&dest, file).await? {
            fetch(&client, file, &dest, |got| {
                progress(Progress {
                    file: file.name,
                    done: before + got,
                    total,
                })
            })
            .await?;
        }
        before += file.bytes;
        progress(Progress {
            file: file.name,
            done: before,
            total,
        });
    }
    Ok(())
}

/// Remove `model` from `dir`. Removing what is not installed is not an error.
pub fn remove(dir: &Path, model: &Model) -> Result<()> {
    let home = dir.join(model.id);
    match std::fs::remove_dir_all(&home) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", home.display())),
    }
}

/// Whether `path` holds `file` already: its size, then its hash.
async fn matches_pin(path: &Path, file: &ModelFile) -> Result<bool> {
    match tokio::fs::metadata(path).await {
        Ok(m) if m.len() == file.bytes => {}
        _ => return Ok(false),
    }
    let path = path.to_path_buf();
    let digest = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
        use std::io::Read as _;
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(hex(&hasher.finalize()))
    })
    .await??;
    Ok(digest == file.sha256)
}

/// Download `file` to `dest`, through a `.part` file renamed into place once
/// its hash is the pinned one.
async fn fetch(
    client: &reqwest::Client,
    file: &ModelFile,
    dest: &Path,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let part = dest.with_extension("part");
    let response = client
        .get(file.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("downloading {}", file.url))?;
    let mut out = tokio::fs::File::create(&part)
        .await
        .with_context(|| format!("creating {}", part.display()))?;
    let mut hasher = Sha256::new();
    let mut got = 0u64;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.with_context(|| format!("downloading {}", file.url))?;
        got += chunk.len() as u64;
        if got > file.bytes {
            drop(out);
            let _ = tokio::fs::remove_file(&part).await;
            bail!(
                "{} is larger than its pinned {} bytes",
                file.url,
                file.bytes
            );
        }
        hasher.update(&chunk);
        out.write_all(&chunk).await?;
        progress(got);
    }
    out.flush().await?;
    drop(out);
    let digest = hex(&hasher.finalize());
    if digest != file.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        bail!(
            "{} does not match its pin (sha256 {digest}, expected {})",
            file.url,
            file.sha256
        );
    }
    tokio::fs::rename(&part, dest)
        .await
        .with_context(|| format!("moving {} into place", dest.display()))?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    static FILE: ModelFile = ModelFile {
        name: "weights.bin",
        // Never fetched: the tests write the file themselves.
        url: "https://example.invalid/weights.bin",
        sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        bytes: 7,
    };
    static MODEL: Model = Model {
        id: "test",
        name: "Test",
        does: "nothing",
        licence: "none",
        source: "https://example.invalid",
        files: std::slice::from_ref(&FILE),
    };

    #[test]
    fn a_model_is_installed_when_every_file_has_its_size() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!installed(dir.path(), &MODEL));
        std::fs::create_dir_all(dir.path().join("test")).unwrap();
        std::fs::write(dir.path().join("test/weights.bin"), b"short").unwrap();
        assert!(!installed(dir.path(), &MODEL), "a file of the wrong size");
        std::fs::write(dir.path().join("test/weights.bin"), b"weights").unwrap();
        assert!(installed(dir.path(), &MODEL));
        remove(dir.path(), &MODEL).unwrap();
        assert!(!installed(dir.path(), &MODEL));
        remove(dir.path(), &MODEL).unwrap();
    }

    #[tokio::test]
    async fn a_file_matches_its_pin_by_hash_not_only_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weights.bin");
        let digest = hex(&Sha256::digest(b"weights"));
        let pinned = ModelFile {
            sha256: Box::leak(digest.into_boxed_str()),
            ..FILE
        };
        std::fs::write(&path, b"weights").unwrap();
        assert!(matches_pin(&path, &pinned).await.unwrap());
        std::fs::write(&path, b"weighTS").unwrap();
        assert!(!matches_pin(&path, &pinned).await.unwrap());
    }
}
