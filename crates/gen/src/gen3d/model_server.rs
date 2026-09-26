//! Client and background worker for the local 3D asset model server (AI1).
//!
//! `gen_generate_asset` and `gen_generate_texture` hand jobs to an
//! [`AssetGenWorker`]: a thread with its own tokio runtime that talks to the
//! model server over HTTP, so Bevy's main thread never waits on the network.
//! The worker reports back through [`AssetUpdate`]s that
//! `apply_asset_gen_updates` (plugin.rs) drains every frame, spawning the mesh
//! or applying the texture maps when a job completes.
//!
//! The server speaks a small JSON protocol (documented in the Gen docs,
//! "External Services"):
//!
//! | Request | Response |
//! |---|---|
//! | `GET /health` | `{"status": "ok", "loaded_model", "gpu_memory_used_mb", "gpu_memory_total_mb"}` |
//! | `POST /generate` `{"type": "mesh" \| "texture", "prompt", …}` | `{"task_id"}` |
//! | `GET /status/{id}` | `{"status": "queued" \| "generating" \| "complete" \| "failed", "progress"?, "error"?, "outputs"?}` |
//! | `GET /result/{id}/{output}` | file bytes (`mesh` → GLB; `base_color`, `metallic_roughness`, `normal`, `emissive` → PNG) |
//! | `POST /cancel/{id}` | — |

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use base64::Engine;
use bevy::prelude::*;
use localgpt_world_types as wt;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::asset_gen::{GenerationModel, GenerationQuality, TextureStyle};

const DEFAULT_URL: &str = "http://127.0.0.1:8741";
/// Give up on a job that hasn't finished after this long.
const JOB_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The model server's base URL (`LOCALGPT_GEN_MODEL_SERVER`).
pub fn server_url() -> String {
    std::env::var(localgpt_core::env::LOCALGPT_GEN_MODEL_SERVER)
        .unwrap_or_else(|_| DEFAULT_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Fail fast, with a useful message, when no model server is running.
pub async fn check_health(base: &str) -> Result<Value> {
    let resp = reqwest::Client::new()
        .get(format!("{base}/health"))
        .timeout(Duration::from_secs(3))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => Ok(r.json().await.unwrap_or(Value::Null)),
        Ok(r) => bail!("The asset model server at {base} answered {}", r.status()),
        Err(e) => bail!(
            "No asset model server is reachable at {base} ({e}), so nothing can be generated. \
             Start one (see the Gen docs, External Services) or set {}; \
             otherwise build the object from primitives with gen_spawn_primitive.",
            localgpt_core::env::LOCALGPT_GEN_MODEL_SERVER
        ),
    }
}

/// A generation job for the worker.
#[derive(Debug, Clone)]
pub enum AssetJob {
    Mesh {
        task_id: String,
        prompt: String,
        model: GenerationModel,
        quality: GenerationQuality,
        pbr: bool,
        /// Reference image file, sent as base64.
        reference_image: Option<PathBuf>,
        /// Where to write `<task_id>.glb`.
        out_dir: PathBuf,
    },
    Texture {
        task_id: String,
        prompt: String,
        style: TextureStyle,
        resolution: u32,
        /// The target's parametric shape, when it has one (lets the server
        /// fit UVs); `None` for imported meshes.
        shape: Option<wt::Shape>,
        /// Where to write `<task_id>_<map>.png`.
        out_dir: PathBuf,
    },
}

impl AssetJob {
    fn task_id(&self) -> &str {
        match self {
            Self::Mesh { task_id, .. } | Self::Texture { task_id, .. } => task_id,
        }
    }
}

/// Progress reported by the worker.
#[derive(Debug, Clone, PartialEq)]
pub enum AssetUpdate {
    Generating {
        task_id: String,
        progress: Option<f32>,
    },
    MeshReady {
        task_id: String,
        path: PathBuf,
    },
    TexturesReady {
        task_id: String,
        maps: Vec<(wt::TextureSlot, PathBuf)>,
    },
    Failed {
        task_id: String,
        error: String,
    },
    Cancelled {
        task_id: String,
    },
}

enum WorkerMsg {
    Job(AssetJob),
    Cancel(String),
}

/// Handle to the background worker thread (a Bevy resource).
#[derive(Resource)]
pub struct AssetGenWorker {
    tx: mpsc::UnboundedSender<WorkerMsg>,
    updates: Mutex<std::sync::mpsc::Receiver<AssetUpdate>>,
}

impl AssetGenWorker {
    /// Start the worker thread against the configured server.
    pub fn spawn() -> Self {
        Self::spawn_with(server_url(), Duration::from_secs(2))
    }

    pub fn spawn_with(base: String, poll: Duration) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<WorkerMsg>();
        let (update_tx, update_rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("gen-asset-worker".into())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                rt.block_on(async move {
                    let cancelled: Arc<Mutex<HashSet<String>>> = Arc::default();
                    let client = ServerClient {
                        base,
                        http: reqwest::Client::new(),
                        poll,
                    };
                    while let Some(msg) = rx.recv().await {
                        match msg {
                            WorkerMsg::Cancel(id) => {
                                cancelled.lock().unwrap().insert(id);
                            }
                            WorkerMsg::Job(job) => {
                                let client = client.clone();
                                let updates = update_tx.clone();
                                let cancelled = cancelled.clone();
                                tokio::spawn(async move {
                                    let id = job.task_id().to_string();
                                    let update = match client.run(&job, &updates, &cancelled).await
                                    {
                                        Ok(update) => update,
                                        Err(e) => AssetUpdate::Failed {
                                            task_id: id,
                                            error: format!("{e:#}"),
                                        },
                                    };
                                    let _ = updates.send(update);
                                });
                            }
                        }
                    }
                });
            });
        if let Err(e) = spawned {
            tracing::error!("Could not start the asset generation worker: {e}");
        }
        Self {
            tx,
            updates: Mutex::new(update_rx),
        }
    }

    /// A worker with no thread, fed through the returned sender (tests).
    #[cfg(test)]
    pub fn from_updates() -> (Self, std::sync::mpsc::Sender<AssetUpdate>) {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (update_tx, update_rx) = std::sync::mpsc::channel();
        let worker = Self {
            tx,
            updates: Mutex::new(update_rx),
        };
        (worker, update_tx)
    }

    pub fn submit(&self, job: AssetJob) {
        let _ = self.tx.send(WorkerMsg::Job(job));
    }

    pub fn cancel(&self, task_id: &str) {
        let _ = self.tx.send(WorkerMsg::Cancel(task_id.to_string()));
    }

    /// Updates received since the last call.
    pub fn drain(&self) -> Vec<AssetUpdate> {
        match self.updates.lock() {
            Ok(rx) => rx.try_iter().collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[derive(Clone)]
struct ServerClient {
    base: String,
    http: reqwest::Client,
    poll: Duration,
}

impl ServerClient {
    /// Submit a job, poll until it ends, download its outputs.
    async fn run(
        &self,
        job: &AssetJob,
        updates: &std::sync::mpsc::Sender<AssetUpdate>,
        cancelled: &Mutex<HashSet<String>>,
    ) -> Result<AssetUpdate> {
        let task_id = job.task_id().to_string();
        let body = request_body(job)?;
        let submitted: Value = self
            .http
            .post(format!("{}/generate", self.base))
            .json(&body)
            .send()
            .await
            .with_context(|| format!("sending the job to the model server at {}", self.base))?
            .error_for_status()?
            .json()
            .await?;
        let remote = submitted["task_id"]
            .as_str()
            .context("the model server returned no task_id")?
            .to_string();

        let started = Instant::now();
        let status = loop {
            if cancelled.lock().unwrap().remove(&task_id) {
                let _ = self
                    .http
                    .post(format!("{}/cancel/{remote}", self.base))
                    .send()
                    .await;
                return Ok(AssetUpdate::Cancelled { task_id });
            }
            if started.elapsed() > JOB_TIMEOUT {
                bail!("the model server did not finish within {JOB_TIMEOUT:?}");
            }
            let status: Value = self
                .http
                .get(format!("{}/status/{remote}", self.base))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            match status["status"].as_str().unwrap_or_default() {
                "complete" => break status,
                "failed" | "cancelled" => bail!(
                    "the model server reported: {}",
                    status["error"].as_str().unwrap_or("failed")
                ),
                _ => {
                    let _ = updates.send(AssetUpdate::Generating {
                        task_id: task_id.clone(),
                        progress: status["progress"].as_f64().map(|p| p as f32),
                    });
                }
            }
            tokio::time::sleep(self.poll).await;
        };

        match job {
            AssetJob::Mesh { out_dir, .. } => {
                let path = out_dir.join(format!("{task_id}.glb"));
                self.download(&remote, "mesh", &path).await?;
                Ok(AssetUpdate::MeshReady { task_id, path })
            }
            AssetJob::Texture { out_dir, .. } => {
                let outputs: Vec<String> = status["outputs"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["base_color".to_string()]);
                let mut maps = Vec::new();
                for output in outputs {
                    let Some(slot) = texture_slot(&output) else {
                        continue;
                    };
                    let path = out_dir.join(format!("{task_id}_{output}.png"));
                    self.download(&remote, &output, &path).await?;
                    maps.push((slot, path));
                }
                if maps.is_empty() {
                    bail!("the model server finished without any texture maps");
                }
                Ok(AssetUpdate::TexturesReady { task_id, maps })
            }
        }
    }

    async fn download(&self, remote: &str, output: &str, path: &Path) -> Result<()> {
        let bytes = self
            .http
            .get(format!("{}/result/{remote}/{output}", self.base))
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("downloading {output}"))?
            .bytes()
            .await?;
        if bytes.is_empty() {
            bail!("the model server returned an empty {output}");
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &bytes).with_context(|| format!("writing {}", path.display()))
    }
}

fn request_body(job: &AssetJob) -> Result<Value> {
    Ok(match job {
        AssetJob::Mesh {
            prompt,
            model,
            quality,
            pbr,
            reference_image,
            ..
        } => {
            let reference = match reference_image {
                Some(path) => Some(base64::engine::general_purpose::STANDARD.encode(
                    std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
                )),
                None => None,
            };
            json!({
                "type": "mesh",
                "prompt": prompt,
                "model": model,
                "quality": quality,
                "pbr": pbr,
                "output_format": "glb",
                "reference_image": reference,
            })
        }
        AssetJob::Texture {
            prompt,
            style,
            resolution,
            shape,
            ..
        } => json!({
            "type": "texture",
            "prompt": prompt,
            "style": style,
            "resolution": resolution,
            "shape": shape,
        }),
    })
}

fn texture_slot(output: &str) -> Option<wt::TextureSlot> {
    Some(match output {
        "base_color" | "albedo" => wt::TextureSlot::BaseColor,
        "metallic_roughness" => wt::TextureSlot::MetallicRoughness,
        "normal" => wt::TextureSlot::Normal,
        "emissive" => wt::TextureSlot::Emissive,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Path as AxPath;
    use axum::routing::{get, post};

    /// A stand-in model server: jobs complete on the second status poll;
    /// "fail" in the prompt makes them fail.
    async fn mock_server() -> (String, Arc<Mutex<Vec<Value>>>) {
        let jobs: Arc<Mutex<Vec<Value>>> = Arc::default();
        let polls: Arc<Mutex<u32>> = Arc::default();
        let j = jobs.clone();
        let app = axum::Router::new()
            .route(
                "/health",
                get(|| async { axum::Json(json!({"status": "ok"})) }),
            )
            .route(
                "/generate",
                post(move |axum::Json(body): axum::Json<Value>| {
                    let j = j.clone();
                    async move {
                        let mut jobs = j.lock().unwrap();
                        jobs.push(body);
                        axum::Json(json!({"task_id": format!("r{}", jobs.len())}))
                    }
                }),
            )
            .route(
                "/status/{id}",
                get({
                    let jobs = jobs.clone();
                    move |AxPath(id): AxPath<String>| {
                        let jobs = jobs.clone();
                        let polls = polls.clone();
                        async move {
                            let n: usize = id[1..].parse().unwrap();
                            let job = jobs.lock().unwrap()[n - 1].clone();
                            if job["prompt"].as_str().unwrap().contains("fail") {
                                return axum::Json(
                                    json!({"status": "failed", "error": "out of VRAM"}),
                                );
                            }
                            let mut p = polls.lock().unwrap();
                            *p += 1;
                            if *p % 2 == 1 {
                                return axum::Json(
                                    json!({"status": "generating", "progress": 0.5}),
                                );
                            }
                            axum::Json(json!({
                                "status": "complete",
                                "outputs": if job["type"] == "texture" {
                                    json!(["base_color", "normal"])
                                } else {
                                    json!(["mesh"])
                                }
                            }))
                        }
                    }
                }),
            )
            .route(
                "/result/{id}/{output}",
                get(|AxPath((_, output)): AxPath<(String, String)>| async move {
                    format!("bytes of {output}").into_bytes()
                }),
            )
            .route("/cancel/{id}", post(|| async { "" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, jobs)
    }

    fn wait_for_end(worker: &AssetGenWorker) -> Vec<AssetUpdate> {
        let mut all = Vec::new();
        for _ in 0..500 {
            all.extend(worker.drain());
            if all
                .iter()
                .any(|u| !matches!(u, AssetUpdate::Generating { .. }))
            {
                return all;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("worker never finished: {all:?}");
    }

    fn mesh_job(prompt: &str, out_dir: &Path) -> AssetJob {
        AssetJob::Mesh {
            task_id: "gen_000001".into(),
            prompt: prompt.into(),
            model: GenerationModel::TripoSG,
            quality: GenerationQuality::Draft,
            pbr: true,
            reference_image: None,
            out_dir: out_dir.to_path_buf(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_mesh_job_runs_to_a_glb_on_disk() {
        let (url, jobs) = mock_server().await;
        check_health(&url).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let worker = AssetGenWorker::spawn_with(url, Duration::from_millis(10));
        worker.submit(mesh_job("a barrel", dir.path()));
        let updates = tokio::task::spawn_blocking(move || wait_for_end(&worker))
            .await
            .unwrap();
        let glb = dir.path().join("gen_000001.glb");
        assert_eq!(
            updates.last().unwrap(),
            &AssetUpdate::MeshReady {
                task_id: "gen_000001".into(),
                path: glb.clone()
            }
        );
        assert_eq!(std::fs::read(glb).unwrap(), b"bytes of mesh");
        let sent = jobs.lock().unwrap()[0].clone();
        assert_eq!(sent["type"], "mesh");
        assert_eq!(sent["model"], "tripo_sg");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_texture_job_downloads_every_map() {
        let (url, _) = mock_server().await;
        let dir = tempfile::tempdir().unwrap();
        let worker = AssetGenWorker::spawn_with(url, Duration::from_millis(10));
        worker.submit(AssetJob::Texture {
            task_id: "gen_000002".into(),
            prompt: "mossy stone".into(),
            style: TextureStyle::Realistic,
            resolution: 512,
            shape: Some(wt::Shape::Cuboid {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }),
            out_dir: dir.path().to_path_buf(),
        });
        let updates = tokio::task::spawn_blocking(move || wait_for_end(&worker))
            .await
            .unwrap();
        let AssetUpdate::TexturesReady { maps, .. } = updates.last().unwrap() else {
            panic!("{updates:?}");
        };
        let slots: Vec<_> = maps.iter().map(|(s, _)| *s).collect();
        assert_eq!(slots, [wt::TextureSlot::BaseColor, wt::TextureSlot::Normal]);
        assert!(maps.iter().all(|(_, p)| p.exists()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn server_failures_reach_the_task() {
        let (url, _) = mock_server().await;
        let dir = tempfile::tempdir().unwrap();
        let worker = AssetGenWorker::spawn_with(url, Duration::from_millis(10));
        worker.submit(mesh_job("please fail", dir.path()));
        let updates = tokio::task::spawn_blocking(move || wait_for_end(&worker))
            .await
            .unwrap();
        let AssetUpdate::Failed { error, .. } = updates.last().unwrap() else {
            panic!("{updates:?}");
        };
        assert!(error.contains("out of VRAM"), "{error}");
    }

    #[tokio::test]
    async fn no_server_is_an_error_up_front() {
        let err = check_health("http://127.0.0.1:9").await.unwrap_err();
        assert!(err.to_string().contains("No asset model server"), "{err}");
    }
}
