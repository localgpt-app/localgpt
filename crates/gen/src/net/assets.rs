//! On-demand asset streaming (§2 "Asset Streaming & Memory Management").
//!
//! Heavy geometry does not ride the replication channel. The host publishes
//! each custom mesh as an immutable, **content-addressed** blob (SHA-256 of
//! its bytes) on a small HTTP server — a stand-in for the spec's edge CDN —
//! and replicates only the digest ([`super::protocol::NetMeshAsset`]).
//! Clients fetch blobs the first time they need them, verify the digest, and
//! keep them in a local disk cache keyed by digest, so a mesh is downloaded
//! once per machine no matter how many sessions or entities use it.
//!
//! Because URLs are content hashes, responses are cacheable forever
//! (`Cache-Control: immutable`) and the host's server can be swapped for a
//! real CDN without touching the protocol.
//!
//! This module holds the pure codec + digest functions and the I/O helpers
//! (server, fetch, disk cache); the Bevy systems live in `host.rs` /
//! `client.rs`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use sha2::{Digest, Sha256};

/// Largest blob the host publishes / a client accepts (bytes).
pub const MAX_BLOB_BYTES: usize = 64 * 1024 * 1024;

/// Largest vertex count a client decodes (guards allocations on untrusted
/// input before the size checks run).
const MAX_VERTICES: usize = 4 * 1024 * 1024;

const MAGIC: &[u8; 4] = b"LGM1";
const HAS_NORMALS: u8 = 1 << 0;
const HAS_UVS: u8 = 1 << 1;
const HAS_COLORS: u8 = 1 << 2;

/// Hex SHA-256 of a blob — its address.
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Whether `s` looks like a digest produced by [`digest`].
pub fn is_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Encode a triangle-list mesh (positions + optional normals/UVs/colors +
/// optional indices) into the `LGM1` blob format. Returns `None` for meshes
/// this format cannot carry (other topologies, missing positions, CPU data
/// already released).
pub fn encode_mesh(mesh: &Mesh) -> Option<Vec<u8>> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok()?
    else {
        return None;
    };
    let n = positions.len();
    let normals = match mesh.try_attribute_option(Mesh::ATTRIBUTE_NORMAL).ok()? {
        Some(VertexAttributeValues::Float32x3(v)) if v.len() == n => Some(v),
        _ => None,
    };
    let uvs = match mesh.try_attribute_option(Mesh::ATTRIBUTE_UV_0).ok()? {
        Some(VertexAttributeValues::Float32x2(v)) if v.len() == n => Some(v),
        _ => None,
    };
    let colors = match mesh.try_attribute_option(Mesh::ATTRIBUTE_COLOR).ok()? {
        Some(VertexAttributeValues::Float32x4(v)) if v.len() == n => Some(v),
        _ => None,
    };
    let indices: Vec<u32> = mesh
        .try_indices_option()
        .ok()?
        .map(|i| i.iter().map(|x| x as u32).collect())
        .unwrap_or_default();

    let mut flags = 0u8;
    if normals.is_some() {
        flags |= HAS_NORMALS;
    }
    if uvs.is_some() {
        flags |= HAS_UVS;
    }
    if colors.is_some() {
        flags |= HAS_COLORS;
    }

    let mut out = Vec::with_capacity(13 + n * 48 + indices.len() * 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.push(flags);
    out.extend_from_slice(&(indices.len() as u32).to_le_bytes());
    let mut put = |values: &[f32]| {
        for v in values {
            out.extend_from_slice(&v.to_le_bytes());
        }
    };
    for p in positions {
        put(p);
    }
    if let Some(normals) = normals {
        for v in normals {
            put(v);
        }
    }
    if let Some(uvs) = uvs {
        for v in uvs {
            put(v);
        }
    }
    if let Some(colors) = colors {
        for v in colors {
            put(v);
        }
    }
    for i in indices {
        out.extend_from_slice(&i.to_le_bytes());
    }
    Some(out)
}

/// Why a blob failed to decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    BadMagic,
    Truncated,
    TooLarge,
    IndexOutOfRange,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::BadMagic => "not an LGM1 mesh blob",
            Self::Truncated => "mesh blob is truncated or has trailing bytes",
            Self::TooLarge => "mesh blob exceeds size limits",
            Self::IndexOutOfRange => "mesh blob has an out-of-range index",
        };
        f.write_str(msg)
    }
}

/// Decode an `LGM1` blob into a mesh. Validates every length against the
/// buffer before allocating, and every index against the vertex count.
pub fn decode_mesh(bytes: &[u8]) -> Result<Mesh, DecodeError> {
    if bytes.len() < 13 || &bytes[..4] != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let n = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let flags = bytes[8];
    let m = u32::from_le_bytes(bytes[9..13].try_into().unwrap()) as usize;
    if n > MAX_VERTICES || m > MAX_VERTICES * 6 {
        return Err(DecodeError::TooLarge);
    }
    let floats_per_vertex = 3
        + if flags & HAS_NORMALS != 0 { 3 } else { 0 }
        + if flags & HAS_UVS != 0 { 2 } else { 0 }
        + if flags & HAS_COLORS != 0 { 4 } else { 0 };
    let expected = 13 + n * floats_per_vertex * 4 + m * 4;
    if bytes.len() != expected {
        return Err(DecodeError::Truncated);
    }

    let mut cursor = 13;
    let mut take_f32 = |count: usize| -> Vec<f32> {
        let (words, _) = bytes[cursor..cursor + count * 4].as_chunks::<4>();
        cursor += count * 4;
        words.iter().map(|w| f32::from_le_bytes(*w)).collect()
    };
    let positions: Vec<[f32; 3]> = take_f32(n * 3).as_chunks::<3>().0.to_vec();
    let normals = (flags & HAS_NORMALS != 0)
        .then(|| take_f32(n * 3).as_chunks::<3>().0.to_vec() as Vec<[f32; 3]>);
    let uvs = (flags & HAS_UVS != 0)
        .then(|| take_f32(n * 2).as_chunks::<2>().0.to_vec() as Vec<[f32; 2]>);
    let colors = (flags & HAS_COLORS != 0)
        .then(|| take_f32(n * 4).as_chunks::<4>().0.to_vec() as Vec<[f32; 4]>);
    let indices: Vec<u32> = bytes[cursor..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    if indices.iter().any(|&i| i as usize >= n) {
        return Err(DecodeError::IndexOutOfRange);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    if let Some(normals) = normals {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    }
    if let Some(uvs) = uvs {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
    if let Some(colors) = colors {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    }
    if !indices.is_empty() {
        mesh.insert_indices(Indices::U32(indices));
    }
    if normals_missing(&mesh) {
        mesh.compute_normals();
    }
    Ok(mesh)
}

fn normals_missing(mesh: &Mesh) -> bool {
    mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_none()
}

// ---------------------------------------------------------------------------
// Host: content-addressed blob store + HTTP server
// ---------------------------------------------------------------------------

/// Shared, content-addressed blob store (digest → bytes).
#[derive(Clone, Default)]
pub struct AssetStore {
    blobs: Arc<RwLock<HashMap<String, Arc<Vec<u8>>>>>,
}

impl AssetStore {
    /// Publish a blob, returning its digest. Idempotent.
    pub fn publish(&self, bytes: Vec<u8>) -> String {
        let key = digest(&bytes);
        if let Ok(mut blobs) = self.blobs.write() {
            blobs.entry(key.clone()).or_insert_with(|| Arc::new(bytes));
        }
        key
    }

    pub fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        self.blobs.read().ok()?.get(key).cloned()
    }

    pub fn len(&self) -> usize {
        self.blobs.read().map(|b| b.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Serve `GET /assets/{digest}` on `addr` from a background thread.
///
/// Only published blobs are reachable — there is no path mapping onto the
/// host filesystem.
pub fn spawn_asset_server(store: AssetStore, addr: SocketAddr) -> std::io::Result<()> {
    use axum::Router;
    use axum::extract::{Path as AxPath, State};
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    use axum::routing::get;

    async fn serve_blob(
        State(store): State<AssetStore>,
        AxPath(key): AxPath<String>,
    ) -> axum::response::Response {
        if !is_digest(&key) {
            return StatusCode::BAD_REQUEST.into_response();
        }
        match store.get(&key) {
            Some(bytes) => (
                [
                    (header::CONTENT_TYPE, "application/octet-stream"),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes.as_ref().clone(),
            )
                .into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }

    // Bind synchronously so the caller learns about port conflicts.
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    std::thread::Builder::new()
        .name("gen-asset-server".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Asset server runtime failed: {e}");
                    return;
                }
            };
            rt.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                let app = Router::new()
                    .route("/assets/{digest}", get(serve_blob))
                    .with_state(store);
                if let Err(e) = axum::serve(listener, app).await {
                    eprintln!("Asset server stopped: {e}");
                }
            });
        })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Client: fetch + disk cache
// ---------------------------------------------------------------------------

/// Default on-disk cache directory for streamed assets.
pub fn default_cache_dir() -> PathBuf {
    localgpt_core::paths::Paths::resolve()
        .map(|p| p.cache_dir)
        .unwrap_or_else(|_| std::env::temp_dir().join("localgpt"))
        .join("gen-assets")
}

/// Path of a cached blob.
pub fn cache_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.lgm"))
}

/// Read a cached blob if present and intact (digest re-verified).
pub fn read_cached(dir: &Path, key: &str) -> Option<Vec<u8>> {
    let bytes = std::fs::read(cache_path(dir, key)).ok()?;
    (digest(&bytes) == key).then_some(bytes)
}

/// Write a blob to the cache (best effort, atomic rename).
pub fn write_cached(dir: &Path, key: &str, bytes: &[u8]) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!("{key}.tmp"));
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, cache_path(dir, key));
    }
}

/// Fetch a blob over HTTP and verify its digest. Blocking — call from a
/// worker thread.
pub fn fetch_blob(base_url: &str, key: &str) -> Result<Vec<u8>, String> {
    let url = format!("{base_url}/assets/{key}");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let response = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("{url}: HTTP {}", response.status()));
        }
        if response
            .content_length()
            .is_some_and(|len| len as usize > MAX_BLOB_BYTES)
        {
            return Err(format!("{url}: blob too large"));
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BLOB_BYTES {
            return Err(format!("{url}: blob too large"));
        }
        if digest(&bytes) != key {
            return Err(format!("{url}: digest mismatch"));
        }
        Ok(bytes.to_vec())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::primitives::{Cuboid, Sphere};
    use bevy::mesh::{MeshBuilder, Meshable};

    #[test]
    fn roundtrip_preserves_geometry() {
        let mesh = Sphere::new(1.5).mesh().uv(12, 8);
        let blob = encode_mesh(&mesh).unwrap();
        let back = decode_mesh(&blob).unwrap();
        assert_eq!(back.count_vertices(), mesh.count_vertices());
        assert_eq!(
            back.indices().unwrap().iter().collect::<Vec<_>>(),
            mesh.indices().unwrap().iter().collect::<Vec<_>>()
        );
        assert_eq!(
            back.attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3(),
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
        );
        assert!(back.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        // Re-encoding is deterministic, so digests are stable.
        assert_eq!(digest(&encode_mesh(&back).unwrap()), digest(&blob));
    }

    #[test]
    fn decode_rejects_bad_input() {
        let blob = encode_mesh(&Cuboid::new(1.0, 1.0, 1.0).mesh().build()).unwrap();
        assert_eq!(decode_mesh(b"nope").err(), Some(DecodeError::BadMagic));
        assert_eq!(
            decode_mesh(&blob[..blob.len() - 1]).err(),
            Some(DecodeError::Truncated)
        );
        // Corrupt the last index to point past the vertex buffer.
        let mut bad = blob.clone();
        let len = bad.len();
        bad[len - 4..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_mesh(&bad).err(), Some(DecodeError::IndexOutOfRange));
        // Absurd header counts are rejected before allocation.
        let mut huge = blob;
        huge[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(decode_mesh(&huge).err(), Some(DecodeError::TooLarge));
    }

    #[test]
    fn digest_shape() {
        let d = digest(b"hello");
        assert!(is_digest(&d));
        assert!(!is_digest("../etc/passwd"));
        assert!(!is_digest(&d.to_uppercase()));
    }

    #[test]
    fn store_is_content_addressed() {
        let store = AssetStore::default();
        let a = store.publish(vec![1, 2, 3]);
        let b = store.publish(vec![1, 2, 3]);
        assert_eq!(a, b);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(&a).unwrap().as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn disk_cache_verifies_digest() {
        let dir = std::env::temp_dir().join(format!("lgm-test-{}", std::process::id()));
        let bytes = b"blob".to_vec();
        let key = digest(&bytes);
        write_cached(&dir, &key, &bytes);
        assert_eq!(read_cached(&dir, &key), Some(bytes));
        // A tampered file is ignored.
        std::fs::write(cache_path(&dir, &key), b"evil").unwrap();
        assert_eq!(read_cached(&dir, &key), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
