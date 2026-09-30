//! The session package on disk: `world.ron` (base), `ops.jsonl` (the one
//! log), `session.json` (metadata), `snapshots/rev-<N>.ron` (keyframes).
//!
//! One artifact holds a world, how it was built (edits and tool calls) and
//! what happened in it (inputs, state, clocks); state at any revision is a
//! fold of the log over the base. Reads are tolerant — a torn last line is
//! dropped, unreadable lines skipped and counted — and integrity is
//! checkable through the SHA-256s in `session.json`.
//! See `docs/rfcs/multiplayer/session-package-format.md`.
//!
//! This module owns the package's files; the shapes are world-sync's
//! (`OpLogEntry`, `SessionOp`, `SessionMeta`) and the fold is
//! [`fold_log`]. It lives in world-agent because Gen, MD and Verse all
//! depend on this crate and none can depend on `localgpt-core`.

use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use localgpt_world_sync as sync;
use localgpt_world_types as wt;
use sync::{OpLogEntry, SessionMeta, WorldDoc, fold_log};

/// The base world, at `base_revision`.
pub const BASE_FILE: &str = "world.ron";
/// The one log, one JSON entry per line.
pub const LOG_FILE: &str = "ops.jsonl";
/// Package metadata and integrity hashes.
pub const META_FILE: &str = "session.json";
/// Derived keyframes, `rev-<N>.ron`.
pub const SNAPSHOTS_DIR: &str = "snapshots";

/// Why a package couldn't be read.
#[derive(Debug)]
pub enum PackageError {
    /// The filesystem said no.
    Io(std::io::Error),
    /// A file isn't the format it should be.
    Parse { file: &'static str, detail: String },
    /// No `session.json` — a legacy session directory, not a package.
    MissingMeta,
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse { file, detail } => write!(f, "{file} doesn't parse: {detail}"),
            Self::MissingMeta => write!(f, "no {META_FILE} — not a session package"),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<std::io::Error> for PackageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// A package read from disk: the base it folded from, the entries after
/// it, and the document at the requested revision.
#[derive(Debug)]
pub struct SessionPackage {
    /// The metadata as written.
    pub meta: SessionMeta,
    /// The base the fold started from — `world.ron`, or a newer snapshot
    /// when reading at a revision.
    pub base: wt::WorldManifest,
    /// The base's revision.
    pub base_revision: u64,
    /// The log entries after the base, in order (all kinds, not just edits).
    pub entries: Vec<OpLogEntry>,
    /// Log lines that couldn't be read (skipped, not fatal).
    pub skipped_lines: usize,
    /// The document at the requested revision.
    pub doc: WorldDoc,
}

/// SHA-256 as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Where a snapshot for `revision` lives.
pub fn snapshot_path(dir: &Path, revision: u64) -> PathBuf {
    dir.join(SNAPSHOTS_DIR).join(format!("rev-{revision}.ron"))
}

/// Write a fresh package: the base world at revision 0 plus its metadata.
///
/// Only for a session directory with no log yet — a base written over an
/// existing log would fold its history twice. Returns the metadata written.
pub fn write_base(
    dir: &Path,
    manifest: &wt::WorldManifest,
    app: &str,
    seed: Option<u64>,
    base_revision: u64,
) -> std::io::Result<SessionMeta> {
    std::fs::create_dir_all(dir)?;
    let ron = ron::ser::to_string_pretty(manifest, ron::ser::PrettyConfig::default())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(dir.join(BASE_FILE), &ron)?;
    let mut meta = SessionMeta::new(manifest.meta.name.clone());
    meta.app = Some(app.to_string());
    meta.base_revision = base_revision;
    meta.head_revision = base_revision;
    meta.seed = seed;
    meta.world_sha256 = Some(sha256_hex(ron.as_bytes()));
    meta.updated_ms = now_ms();
    write_meta(dir, &meta)?;
    Ok(meta)
}

/// Rewrite `session.json`.
pub fn write_meta(dir: &Path, meta: &SessionMeta) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(dir.join(META_FILE), json)
}

/// Read `session.json`, if the directory is a package.
pub fn read_meta(dir: &Path) -> Result<SessionMeta, PackageError> {
    let bytes = std::fs::read(dir.join(META_FILE)).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => PackageError::MissingMeta,
        _ => PackageError::Io(e),
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|e| PackageError::Parse {
            file: META_FILE,
            detail: e.to_string(),
        })
        .map(|mut m: SessionMeta| {
            // Never trust a stale head over the log we're about to read.
            m.head_revision = m.head_revision.max(m.base_revision);
            m
        })
}

/// Update the metadata's head and log hash from the log on disk.
///
/// Returns the metadata as written, or `None` when the directory has no
/// `session.json` (a legacy session directory — leave it as it is).
pub fn refresh_meta(dir: &Path, head_revision: u64) -> std::io::Result<Option<SessionMeta>> {
    let Ok(mut meta) = read_meta(dir) else {
        return Ok(None);
    };
    meta.head_revision = head_revision;
    if let Ok(bytes) = std::fs::read(dir.join(LOG_FILE)) {
        meta.log_sha256 = Some(sha256_hex(&bytes));
    }
    meta.updated_ms = now_ms();
    write_meta(dir, &meta)?;
    Ok(Some(meta))
}

/// Append one entry to the log, one JSON line.
pub fn append_entry(dir: &Path, entry: &OpLogEntry) -> std::io::Result<()> {
    let mut line = sync::encode_line(entry)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(LOG_FILE))?;
    file.write_all(line.as_bytes())
}

/// Write a snapshot: the document at `revision`, a derived keyframe.
pub fn write_snapshot(
    dir: &Path,
    revision: u64,
    manifest: &wt::WorldManifest,
) -> std::io::Result<()> {
    let path = snapshot_path(dir, revision);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ron = ron::ser::to_string_pretty(manifest, ron::ser::PrettyConfig::default())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, ron)
}

/// The newest snapshot at or below `revision`, as (revision, manifest).
fn newest_snapshot_at_or_below(
    dir: &Path,
    revision: u64,
) -> Result<Option<(u64, wt::WorldManifest)>, PackageError> {
    let entries = match std::fs::read_dir(dir.join(SNAPSHOTS_DIR)) {
        Ok(entries) => entries,
        Err(_) => return Ok(None),
    };
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix("rev-") else {
            continue;
        };
        let Some(num) = rest.strip_suffix(".ron") else {
            continue;
        };
        let Ok(rev) = num.parse::<u64>() else {
            continue;
        };
        if rev <= revision && best.as_ref().is_none_or(|(b, _)| rev > *b) {
            best = Some((rev, entry.path()));
        }
    }
    match best {
        None => Ok(None),
        Some((rev, path)) => {
            let text = std::fs::read_to_string(path)?;
            let manifest = ron::from_str(&text).map_err(|e| PackageError::Parse {
                file: "a snapshot",
                detail: e.to_string(),
            })?;
            Ok(Some((rev, manifest)))
        }
    }
}

/// Read a package: the base (or the nearest snapshot at or below `at`,
/// when given), the log entries after it, and the document folded to the
/// requested revision.
///
/// Unreadable log lines are skipped and counted — a torn last line, the
/// crash case, loses at most itself. The fold stops at the first entry
/// that no longer applies, which surfaces as an error.
pub fn read_package(dir: &Path, at: Option<u64>) -> Result<SessionPackage, PackageError> {
    let meta = read_meta(dir)?;
    let ceiling = at.unwrap_or(u64::MAX);

    let (base_revision, base) = match newest_snapshot_at_or_below(dir, ceiling)? {
        Some(snapshot @ (rev, _)) if rev > meta.base_revision => snapshot,
        _ => {
            let text = std::fs::read_to_string(dir.join(BASE_FILE)).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    PackageError::Parse {
                        file: BASE_FILE,
                        detail: "missing".into(),
                    }
                } else {
                    PackageError::Io(e)
                }
            })?;
            let manifest = ron::from_str(&text).map_err(|e| PackageError::Parse {
                file: BASE_FILE,
                detail: e.to_string(),
            })?;
            (meta.base_revision, manifest)
        }
    };

    let mut entries = Vec::new();
    let mut skipped = 0usize;
    if let Ok(log) = std::fs::read_to_string(dir.join(LOG_FILE)) {
        for line in log.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match sync::decode_line(line) {
                Ok(entry) => {
                    if entry.revision > base_revision && entry.revision <= ceiling {
                        entries.push(entry);
                    }
                }
                Err(_) => skipped += 1,
            }
        }
    }

    let mut doc = WorldDoc::from_manifest(&base).map_err(|e| PackageError::Parse {
        file: BASE_FILE,
        detail: format!("base doesn't load: {e}"),
    })?;
    let applied = entries.len();
    doc = fold_log(&doc, &entries).map_err(|e| PackageError::Parse {
        file: LOG_FILE,
        detail: format!("entry {applied} no longer applies: {e}"),
    })?;

    Ok(SessionPackage {
        meta,
        base,
        base_revision,
        entries,
        skipped_lines: skipped,
        doc,
    })
}

// ---------------------------------------------------------------------------
// The transport form
// ---------------------------------------------------------------------------

/// Zip a session directory into the transport form: one `.world` file.
///
/// Entries are stored (uncompressed): the assets are already compressed
/// media (GLB, PNG, MP3), and the log stays readable inside the archive.
/// Walk order is sorted, so the same package zips to the same archive.
pub fn export_zip(dir: &Path, dest: &Path) -> Result<(), PackageError> {
    use std::fs::File;
    use std::io::copy;
    use zip::CompressionMethod;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(dir, &mut files)?;
    files.sort();

    let file = File::create(dest)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for path in &files {
        let name = path
            .strip_prefix(dir)
            .map_err(|e| PackageError::Parse {
                file: "the package",
                detail: e.to_string(),
            })?
            .to_string_lossy()
            .replace('\\', "/");
        zip.start_file(name.clone(), options)
            .map_err(|e| PackageError::Parse {
                file: "the package",
                detail: format!("can't start {name}: {e}"),
            })?;
        let mut source = File::open(path)?;
        copy(&mut source, &mut zip)?;
    }
    zip.finish().map_err(|e| PackageError::Parse {
        file: "the package",
        detail: e.to_string(),
    })?;
    Ok(())
}

/// A temporary directory removed when dropped — what a `.world` archive
/// extracts into for reading.
pub struct TempSessionDir(PathBuf);

impl TempSessionDir {
    /// Where the archive extracted.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempSessionDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Fork a package: a new package whose base is the source folded to
/// `at` (a revision; `None` is head), with the source recorded as
/// provenance and an empty log of its own.
///
/// The copy form of a fork (spec/rfcs/branching-histories.md): the new
/// package folds independently and shares nothing but its history's
/// meaning.
pub fn fork_package(
    from: &Path,
    to: &Path,
    at: Option<u64>,
    app: &str,
) -> Result<SessionMeta, PackageError> {
    let source = read_package(from, at)?;
    let revision = at.unwrap_or_else(|| {
        source.entries.last().map_or(source.base_revision, |e| {
            e.revision.max(source.base_revision)
        })
    });
    let mut meta = write_base(to, &source.doc.to_manifest(), app, None, revision)?;
    meta.forked_from = Some(format!("{}@{}", source.meta.name, revision));
    write_meta(to, &meta)?;
    Ok(meta)
}

/// Extract a `.world` archive (the transport form) into a fresh temporary
/// directory. The package reads from there; the directory goes away when
/// the guard does.
pub fn extract_zip(zip_path: &Path) -> Result<TempSessionDir, PackageError> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "localgpt-world-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| PackageError::Parse {
        file: "the package archive",
        detail: e.to_string(),
    })?;
    archive.extract(&dir).map_err(|e| PackageError::Parse {
        file: "the package archive",
        detail: e.to_string(),
    })?;
    Ok(TempSessionDir(dir))
}

/// Every file under `dir`, recursively.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use localgpt_world_sync::{Author, SessionOp};
    use std::sync::atomic::{AtomicU64, Ordering};

    static DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    /// A unique temp dir, removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "localgpt-session-test-{}-{}",
                std::process::id(),
                DIR_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn manifest_with(entity_count: u64) -> wt::WorldManifest {
        let mut manifest = wt::WorldManifest::new("test-world");
        for i in 1..=entity_count {
            manifest
                .entities
                .push(wt::WorldEntity::new(i, format!("e{i}")));
        }
        manifest
    }

    fn entry(revision: u64, name: &str) -> OpLogEntry {
        OpLogEntry {
            revision,
            author: Author {
                peer: None,
                name: "maya".into(),
            },
            ops: vec![SessionOp::Edit(Box::new(wt::EditOp::spawn(
                wt::WorldEntity::new(100 + revision, name),
            )))],
            timestamp_ms: revision,
            id: None,
            parent: None,
        }
    }

    #[test]
    fn write_read_roundtrip_and_fold() {
        let dir = TempDir::new();
        let meta = write_base(&dir.0, &manifest_with(2), "gen", Some(7), 0).unwrap();
        assert_eq!(meta.format_version, sync::SESSION_FORMAT_VERSION);
        assert_eq!(meta.world_sha256.as_deref().map(len64), Some(64));

        append_entry(&dir.0, &entry(1, "added")).unwrap();
        refresh_meta(&dir.0, 1).unwrap();

        let pkg = read_package(&dir.0, None).unwrap();
        assert_eq!(pkg.base_revision, 0);
        assert_eq!(pkg.doc.len(), 3); // 2 base + 1 spawned
        assert!(pkg.doc.contains(101));
        assert_eq!(pkg.skipped_lines, 0);
        assert!(pkg.meta.log_sha256.is_some());
    }

    fn len64(s: &str) -> usize {
        s.len()
    }

    #[test]
    fn read_at_revision_uses_snapshots() {
        let dir = TempDir::new();
        write_base(&dir.0, &manifest_with(1), "gen", None, 0).unwrap();
        for rev in 1..=6 {
            append_entry(&dir.0, &entry(rev, &format!("n{rev}"))).unwrap();
        }
        // A snapshot at revision 4: 1 base + 4 spawned.
        let doc = read_package(&dir.0, Some(4)).unwrap().doc;
        write_snapshot(&dir.0, 4, &doc.to_manifest()).unwrap();

        let pkg = read_package(&dir.0, Some(4)).unwrap();
        assert_eq!(pkg.base_revision, 4);
        assert_eq!(pkg.doc.len(), 5);
        assert!(pkg.entries.is_empty()); // nothing after the snapshot

        let head = read_package(&dir.0, None).unwrap();
        assert_eq!(head.doc.len(), 7); // 1 base + 6 spawned
        assert_eq!(head.base_revision, 4);
        assert_eq!(head.entries.len(), 2); // revisions 5 and 6

        let mid = read_package(&dir.0, Some(5)).unwrap();
        assert_eq!(mid.doc.len(), 6);
    }

    #[test]
    fn torn_last_line_is_skipped() {
        let dir = TempDir::new();
        write_base(&dir.0, &manifest_with(0), "gen", None, 0).unwrap();
        append_entry(&dir.0, &entry(1, "one")).unwrap();
        // A crash mid-write: half a line.
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.0.join(LOG_FILE))
            .unwrap();
        file.write_all(b"{\"revision\":2,\"auth").unwrap();

        let pkg = read_package(&dir.0, None).unwrap();
        assert_eq!(pkg.skipped_lines, 1);
        assert_eq!(pkg.doc.len(), 1);
    }

    #[test]
    fn export_zip_round_trips_the_transport_form() {
        let dir = TempDir::new();
        write_base(&dir.0, &manifest_with(2), "gen", None, 0).unwrap();
        append_entry(&dir.0, &entry(1, "added")).unwrap();
        let folded = read_package(&dir.0, None).unwrap();

        let zip_path = dir.0.join("test.world");
        export_zip(&dir.0, &zip_path).unwrap();

        // The archive holds the package's files, stored, under relative names.
        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert!(names.contains(&"world.ron".to_string()));
        assert!(names.contains(&"ops.jsonl".to_string()));
        assert!(names.contains(&"session.json".to_string()));

        // Unzip to a fresh directory; the package folds to the same state.
        let out = TempDir::new();
        archive
            .extract(&out.0)
            .expect("extract is available with default features");
        let unzipped = read_package(&out.0, None).unwrap();
        assert_eq!(unzipped.doc.len(), folded.doc.len());
        assert!(unzipped.doc.contains(101));

        // And through extract_zip, the way --resume castle.world reads one.
        let temp = extract_zip(&zip_path).unwrap();
        let via_temp = read_package(temp.path(), None).unwrap();
        assert_eq!(via_temp.doc.len(), folded.doc.len());
        assert!(via_temp.doc.contains(101));
    }

    #[test]
    fn fork_package_folds_independently_with_provenance() {
        let src = TempDir::new();
        write_base(&src.0, &manifest_with(2), "gen", None, 0).unwrap();
        append_entry(&src.0, &entry(1, "added")).unwrap();
        append_entry(&src.0, &entry(2, "also")).unwrap();

        let dst = TempDir::new();
        let meta = fork_package(&src.0, &dst.0, Some(1), "gen").unwrap();
        assert_eq!(meta.base_revision, 1);
        assert_eq!(meta.forked_from.as_deref(), Some("test-world@1"));

        // The fork holds the base plus revision 1's spawn — not revision
        // 2's — and an empty log of its own.
        let forked = read_package(&dst.0, None).unwrap();
        assert_eq!(forked.doc.len(), 3); // 2 base + 1 spawned
        assert!(forked.doc.contains(101));
        assert!(!forked.doc.contains(102));
        assert!(forked.entries.is_empty());

        // The source is unchanged: still 2 base + 2 spawned.
        assert_eq!(read_package(&src.0, None).unwrap().doc.len(), 4);
    }

    #[test]
    fn missing_meta_is_not_a_package() {
        let dir = TempDir::new();
        assert!(matches!(
            read_package(&dir.0, None),
            Err(PackageError::MissingMeta)
        ));
    }
}
