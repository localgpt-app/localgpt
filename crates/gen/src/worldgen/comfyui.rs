//! ComfyUI client for `gen_preview_world` (WG7.2): depth map + prompt →
//! styled preview image, through a depth ControlNet.
//!
//! Uses ComfyUI's HTTP API: upload the depth map (`POST /upload/image`),
//! queue a workflow (`POST /prompt`), poll `GET /history/{id}` until it has
//! outputs, and fetch the image (`GET /view`). The built-in workflow is
//! SD 1.5 + a depth ControlNet; the model file names, the server URL and a
//! whole custom workflow can be overridden through `LOCALGPT_GEN_COMFYUI*`
//! environment variables.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use localgpt_core::env::{
    LOCALGPT_GEN_COMFYUI_CHECKPOINT as COMFYUI_CHECKPOINT_ENV,
    LOCALGPT_GEN_COMFYUI_CONTROLNET as COMFYUI_CONTROLNET_ENV,
    LOCALGPT_GEN_COMFYUI_URL as COMFYUI_URL_ENV,
    LOCALGPT_GEN_COMFYUI_WORKFLOW as COMFYUI_WORKFLOW_ENV,
};

// A custom workflow (COMFYUI_WORKFLOW_ENV) is in ComfyUI API format; string
// values `{{prompt}}`, `{{negative}}`, `{{depth_image}}`, `{{width}}`,
// `{{height}}` and `{{seed}}` are substituted (the numeric ones become
// numbers).

const DEFAULT_URL: &str = "http://127.0.0.1:8188";
const DEFAULT_CHECKPOINT: &str = "v1-5-pruned-emaonly.safetensors";
const DEFAULT_CONTROLNET: &str = "control_v11f1p_sd15_depth.safetensors";
const DEFAULT_NEGATIVE: &str = "blurry, low quality, distorted, text, watermark";

/// One preview request.
pub struct PreviewRequest<'a> {
    pub depth_png: Vec<u8>,
    pub prompt: &'a str,
    pub negative: Option<&'a str>,
    pub width: u32,
    pub height: u32,
    pub seed: u64,
}

pub struct ComfyClient {
    base: String,
    checkpoint: String,
    controlnet: String,
    workflow: Option<Value>,
    http: reqwest::Client,
    poll_interval: Duration,
    timeout: Duration,
}

impl ComfyClient {
    /// Client configured from the environment.
    pub fn from_env() -> Result<Self> {
        let workflow = match std::env::var(COMFYUI_WORKFLOW_ENV) {
            Ok(path) => {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {COMFYUI_WORKFLOW_ENV}={path}"))?;
                Some(serde_json::from_str(&text).with_context(|| format!("parsing {path}"))?)
            }
            Err(_) => None,
        };
        let mut client =
            Self::new(&std::env::var(COMFYUI_URL_ENV).unwrap_or_else(|_| DEFAULT_URL.into()));
        if let Ok(c) = std::env::var(COMFYUI_CHECKPOINT_ENV) {
            client.checkpoint = c;
        }
        if let Ok(c) = std::env::var(COMFYUI_CONTROLNET_ENV) {
            client.controlnet = c;
        }
        client.workflow = workflow;
        Ok(client)
    }

    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            checkpoint: DEFAULT_CHECKPOINT.into(),
            controlnet: DEFAULT_CONTROLNET.into(),
            workflow: None,
            http: reqwest::Client::new(),
            poll_interval: Duration::from_millis(500),
            timeout: Duration::from_secs(300),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// Fail fast, with a useful message, when ComfyUI isn't running.
    pub async fn check_reachable(&self) -> Result<()> {
        let url = format!("{}/system_stats", self.base);
        match self
            .http
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => Ok(()),
            Ok(r) => bail!("ComfyUI at {} answered {}", self.base, r.status()),
            Err(e) => bail!(
                "ComfyUI is not reachable at {} ({e}). Start ComfyUI with a depth \
                 ControlNet installed, or set {COMFYUI_URL_ENV}.",
                self.base
            ),
        }
    }

    /// Generate a preview and return the PNG bytes.
    pub async fn generate(&self, req: &PreviewRequest<'_>) -> Result<Vec<u8>> {
        let uploaded = self.upload(&req.depth_png).await?;
        let workflow = self.workflow_for(req, &uploaded);
        let queued: Value = self
            .http
            .post(format!("{}/prompt", self.base))
            .json(&json!({ "prompt": workflow, "client_id": "localgpt-gen" }))
            .send()
            .await?
            .error_for_status()
            .context("ComfyUI rejected the workflow (are the model files installed?)")?
            .json()
            .await?;
        let prompt_id = queued["prompt_id"]
            .as_str()
            .context("ComfyUI returned no prompt_id")?
            .to_string();

        let started = std::time::Instant::now();
        let image = loop {
            if started.elapsed() > self.timeout {
                bail!("ComfyUI did not finish within {:?}", self.timeout);
            }
            let history: Value = self
                .http
                .get(format!("{}/history/{prompt_id}", self.base))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let entry = &history[&prompt_id];
            if entry["status"]["status_str"] == "error" {
                bail!("ComfyUI failed: {}", entry["status"]["messages"]);
            }
            let images = entry["outputs"]
                .as_object()
                .into_iter()
                .flat_map(|o| o.values())
                .filter_map(|out| out["images"].as_array())
                .flatten()
                .find(|img| img["type"] == "output")
                .cloned();
            if let Some(image) = images {
                break image;
            }
            tokio::time::sleep(self.poll_interval).await;
        };

        let mut view = reqwest::Url::parse(&format!("{}/view", self.base))?;
        view.query_pairs_mut()
            .append_pair("filename", image["filename"].as_str().unwrap_or_default())
            .append_pair("subfolder", image["subfolder"].as_str().unwrap_or_default())
            .append_pair("type", "output");
        let bytes = self
            .http
            .get(view)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        if !bytes.starts_with(b"\x89PNG") {
            bail!("ComfyUI returned something that is not a PNG");
        }
        Ok(bytes.to_vec())
    }

    async fn upload(&self, png: &[u8]) -> Result<String> {
        let part = reqwest::multipart::Part::bytes(png.to_vec())
            .file_name("localgpt_depth.png")
            .mime_str("image/png")?;
        let form = reqwest::multipart::Form::new()
            .part("image", part)
            .text("overwrite", "true");
        let resp: Value = self
            .http
            .post(format!("{}/upload/image", self.base))
            .multipart(form)
            .send()
            .await?
            .error_for_status()
            .context("uploading the depth map to ComfyUI")?
            .json()
            .await?;
        let name = resp["name"]
            .as_str()
            .context("ComfyUI upload has no name")?;
        Ok(match resp["subfolder"].as_str() {
            Some(sub) if !sub.is_empty() => format!("{sub}/{name}"),
            _ => name.to_string(),
        })
    }

    fn workflow_for(&self, req: &PreviewRequest<'_>, depth_image: &str) -> Value {
        let negative = req.negative.unwrap_or(DEFAULT_NEGATIVE);
        match &self.workflow {
            Some(template) => substitute(template, req, negative, depth_image),
            None => json!({
                "1": { "class_type": "CheckpointLoaderSimple",
                       "inputs": { "ckpt_name": self.checkpoint } },
                "2": { "class_type": "CLIPTextEncode",
                       "inputs": { "text": req.prompt, "clip": ["1", 1] } },
                "3": { "class_type": "CLIPTextEncode",
                       "inputs": { "text": negative, "clip": ["1", 1] } },
                "4": { "class_type": "LoadImage", "inputs": { "image": depth_image } },
                "5": { "class_type": "ControlNetLoader",
                       "inputs": { "control_net_name": self.controlnet } },
                "6": { "class_type": "ControlNetApplyAdvanced",
                       "inputs": { "positive": ["2", 0], "negative": ["3", 0],
                                   "control_net": ["5", 0], "image": ["4", 0],
                                   "strength": 1.0, "start_percent": 0.0, "end_percent": 1.0 } },
                "7": { "class_type": "EmptyLatentImage",
                       "inputs": { "width": req.width, "height": req.height, "batch_size": 1 } },
                "8": { "class_type": "KSampler",
                       "inputs": { "model": ["1", 0], "seed": req.seed, "steps": 20, "cfg": 7.0,
                                   "sampler_name": "euler", "scheduler": "normal",
                                   "positive": ["6", 0], "negative": ["6", 1],
                                   "latent_image": ["7", 0], "denoise": 1.0 } },
                "9": { "class_type": "VAEDecode",
                       "inputs": { "samples": ["8", 0], "vae": ["1", 2] } },
                "10": { "class_type": "SaveImage",
                        "inputs": { "images": ["9", 0], "filename_prefix": "localgpt_preview" } }
            }),
        }
    }
}

/// Fill a custom workflow's `{{…}}` placeholders.
fn substitute(v: &Value, req: &PreviewRequest<'_>, negative: &str, depth_image: &str) -> Value {
    match v {
        Value::String(s) => match s.as_str() {
            "{{width}}" => json!(req.width),
            "{{height}}" => json!(req.height),
            "{{seed}}" => json!(req.seed),
            _ => Value::String(
                s.replace("{{prompt}}", req.prompt)
                    .replace("{{negative}}", negative)
                    .replace("{{depth_image}}", depth_image),
            ),
        },
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|i| substitute(i, req, negative, depth_image))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute(v, req, negative, depth_image)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Round a preview size to what SD latents accept (multiples of 64, at most
/// 1024 on the long side, keeping the aspect ratio).
pub fn latent_size(width: u32, height: u32) -> (u32, u32) {
    let scale = (1024.0 / width.max(height).max(1) as f32).min(1.0);
    let round = |v: u32| (((v as f32 * scale) / 64.0).round() as u32).max(1) * 64;
    (round(width), round(height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::{Path, Query};
    use axum::routing::{get, post};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

    /// A stand-in ComfyUI: records the queued workflow, reports it done on
    /// the second history poll, and serves a PNG from /view.
    async fn mock_comfy() -> (String, Arc<Mutex<Option<Value>>>) {
        let queued: Arc<Mutex<Option<Value>>> = Arc::default();
        let polls = Arc::new(Mutex::new(0));
        let q = queued.clone();
        let app = axum::Router::new()
            .route("/system_stats", get(|| async { axum::Json(json!({})) }))
            .route(
                "/upload/image",
                post(|| async { axum::Json(json!({"name": "depth.png", "subfolder": ""})) }),
            )
            .route(
                "/prompt",
                post(move |axum::Json(body): axum::Json<Value>| {
                    let q = q.clone();
                    async move {
                        *q.lock().unwrap() = Some(body["prompt"].clone());
                        axum::Json(json!({"prompt_id": "p1"}))
                    }
                }),
            )
            .route(
                "/history/{id}",
                get(move |Path(id): Path<String>| {
                    let polls = polls.clone();
                    async move {
                        let mut n = polls.lock().unwrap();
                        *n += 1;
                        if *n < 2 {
                            return axum::Json(json!({}));
                        }
                        axum::Json(json!({ id: { "outputs": { "10": { "images": [
                            {"filename": "localgpt_preview_0001.png", "subfolder": "", "type": "output"}
                        ]}}}}))
                    }
                }),
            )
            .route(
                "/view",
                get(|Query(q): Query<HashMap<String, String>>| async move {
                    assert_eq!(q["filename"], "localgpt_preview_0001.png");
                    PNG.to_vec()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, queued)
    }

    fn request() -> PreviewRequest<'static> {
        PreviewRequest {
            depth_png: PNG.to_vec(),
            prompt: "a castle, watercolor",
            negative: None,
            width: 512,
            height: 512,
            seed: 7,
        }
    }

    #[tokio::test]
    async fn a_preview_round_trips_through_comfyui() {
        let (url, queued) = mock_comfy().await;
        let mut client = ComfyClient::new(&url);
        client.poll_interval = Duration::from_millis(10);
        client.check_reachable().await.unwrap();
        let png = client.generate(&request()).await.unwrap();
        assert_eq!(png, PNG);
        let workflow = queued.lock().unwrap().clone().unwrap();
        assert_eq!(workflow["2"]["inputs"]["text"], "a castle, watercolor");
        assert_eq!(workflow["4"]["inputs"]["image"], "depth.png");
        assert_eq!(workflow["8"]["inputs"]["seed"], 7);
    }

    #[tokio::test]
    async fn an_absent_server_is_an_error_not_a_path() {
        let client = ComfyClient::new("http://127.0.0.1:9");
        let err = client.check_reachable().await.unwrap_err().to_string();
        assert!(err.contains("not reachable"), "{err}");
    }

    #[test]
    fn custom_workflows_get_their_placeholders_filled() {
        let template = json!({"a": {"inputs": {
            "text": "{{prompt}} -- {{negative}}", "image": "{{depth_image}}",
            "width": "{{width}}", "seed": "{{seed}}"
        }}});
        let filled = substitute(&template, &request(), "ugly", "d.png");
        assert_eq!(
            filled["a"]["inputs"]["text"],
            "a castle, watercolor -- ugly"
        );
        assert_eq!(filled["a"]["inputs"]["image"], "d.png");
        assert_eq!(filled["a"]["inputs"]["width"], 512);
        assert_eq!(filled["a"]["inputs"]["seed"], 7);
    }

    #[test]
    fn preview_sizes_fit_sd_latents() {
        assert_eq!(latent_size(1024, 1024), (1024, 1024));
        assert_eq!(latent_size(1920, 1080), (1024, 576));
        assert_eq!(latent_size(500, 300), (512, 320));
    }
}
