//! Embedding providers for semantic search
//!
//! Supports OpenAI embeddings API, local embeddings via fastembed (ONNX),
//! and optional GGUF embeddings via llama.cpp (requires `gguf` feature).

use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::debug;

/// Embedding provider trait
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Get the provider ID (e.g., "openai", "local")
    fn id(&self) -> &str;

    /// Get the model name
    fn model(&self) -> &str;

    /// Get embedding dimensions
    fn dimensions(&self) -> usize;

    /// Embed a single text
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Embed multiple texts (batch)
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// OpenAI embedding provider
pub struct OpenAIEmbeddingProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    dimensions: usize,
}

impl OpenAIEmbeddingProvider {
    pub fn new(api_key: &str, base_url: &str, model: &str) -> Result<Self> {
        // text-embedding-3-small has 1536 dimensions by default
        // text-embedding-3-large has 3072 dimensions by default
        let dimensions = match model {
            "text-embedding-3-small" => 1536,
            "text-embedding-3-large" => 3072,
            "text-embedding-ada-002" => 1536,
            _ => 1536, // default
        };

        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            dimensions,
        })
    }
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OpenAIEmbeddingProvider {
    fn id(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_batch(&[text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = EmbeddingRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
        };

        debug!("Embedding {} texts with {}", texts.len(), self.model);

        let response = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API error {}: {}", status, body);
        }

        let response: EmbeddingResponse = response.json().await?;

        // Normalize embeddings to unit vectors
        let embeddings: Vec<Vec<f32>> = response
            .data
            .into_iter()
            .map(|d| normalize_embedding(d.embedding))
            .collect();

        Ok(embeddings)
    }
}

/// Normalize embedding to unit vector
pub fn normalize_embedding(mut vec: Vec<f32>) -> Vec<f32> {
    let magnitude: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if magnitude > 1e-10 {
        for x in &mut vec {
            *x /= magnitude;
        }
    }
    vec
}

/// Hash text for embedding cache lookup
pub fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

// ============================================================================
// Local Embedding Provider (fastembed) - Default provider, no API key needed
// ============================================================================

#[cfg(feature = "embeddings-local")]
use std::sync::{Arc, Mutex as StdMutex};

#[cfg(feature = "embeddings-local")]
pub struct FastEmbedProvider {
    model: Arc<StdMutex<fastembed::TextEmbedding>>,
    model_name: String,
    dimensions: usize,
}

#[cfg(feature = "embeddings-local")]
impl FastEmbedProvider {
    pub fn new(model_name: Option<&str>) -> Result<Self> {
        Self::new_with_cache_dir(model_name, None)
    }

    pub fn new_with_cache_dir(model_name: Option<&str>, cache_dir: Option<&str>) -> Result<Self> {
        use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

        // Set cache directory via environment variable if provided
        // This must be done before TextEmbedding::try_new
        if let Some(dir) = cache_dir {
            let expanded = shellexpand::tilde(dir).to_string();
            let path = std::path::Path::new(&expanded);
            if let Err(e) = std::fs::create_dir_all(path) {
                debug!("Failed to create cache directory {}: {}", expanded, e);
            }
            // SAFETY: called during single-threaded init before any threads are spawned
            unsafe { std::env::set_var("FASTEMBED_CACHE_DIR", &expanded) };
            debug!("Set FASTEMBED_CACHE_DIR to {}", expanded);
        }

        // Supported models with disk sizes:
        // - all-MiniLM-L6-v2:      384 dims, ~80 MB  (default, English, fastest)
        // - bge-base-en-v1.5:      768 dims, ~430 MB (English, quality)
        // - bge-small-zh-v1.5:     512 dims, ~95 MB  (Chinese only)
        // - multilingual-e5-small: 384 dims, ~470 MB (multilingual, compact)
        // - multilingual-e5-base:  768 dims, ~1.1 GB (multilingual, recommended for Chinese)
        // - bge-m3:               1024 dims, ~2.2 GB (best multilingual quality)
        let (model_enum, name, dims) = match model_name {
            // English models
            Some("all-MiniLM-L6-v2") | None => {
                (EmbeddingModel::AllMiniLML6V2, "all-MiniLM-L6-v2", 384)
            }
            Some("bge-base-en-v1.5") => (EmbeddingModel::BGEBaseENV15, "bge-base-en-v1.5", 768),
            // Chinese-specific model
            Some("bge-small-zh-v1.5") => (EmbeddingModel::BGESmallZHV15, "bge-small-zh-v1.5", 512),
            // Multilingual models (Chinese, Japanese, Korean, 100+ languages)
            Some("multilingual-e5-small") => (
                EmbeddingModel::MultilingualE5Small,
                "multilingual-e5-small",
                384,
            ),
            Some("multilingual-e5-base") => (
                EmbeddingModel::MultilingualE5Base,
                "multilingual-e5-base",
                768,
            ),
            Some("bge-m3") => (EmbeddingModel::BGEM3, "bge-m3", 1024),
            Some(other) => {
                anyhow::bail!(
                    "Unknown embedding model: '{}'. Supported models:\n\
                     English:\n\
                       - all-MiniLM-L6-v2 (default, ~80MB)\n\
                       - bge-base-en-v1.5 (~430MB)\n\
                     Chinese:\n\
                       - bge-small-zh-v1.5 (~95MB)\n\
                     Multilingual:\n\
                       - multilingual-e5-small (~470MB)\n\
                       - multilingual-e5-base (~1.1GB, recommended for Chinese)\n\
                       - bge-m3 (~2.2GB, best quality)",
                    other
                );
            }
        };

        debug!("Loading local embedding model: {}", name);
        let model = TextEmbedding::try_new(InitOptions::new(model_enum))?;

        Ok(Self {
            model: Arc::new(StdMutex::new(model)),
            model_name: name.to_string(),
            dimensions: dims,
        })
    }
}

#[cfg(feature = "embeddings-local")]
#[async_trait]
impl EmbeddingProvider for FastEmbedProvider {
    fn id(&self) -> &str {
        "local"
    }

    fn model(&self) -> &str {
        &self.model_name
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_batch(&[text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        debug!(
            "Embedding {} texts locally with {}",
            texts.len(),
            self.model_name
        );

        // fastembed is synchronous, run in blocking task
        let texts = texts.to_vec();
        let model = Arc::clone(&self.model);

        let embeddings: Vec<Vec<f32>> = tokio::task::spawn_blocking(move || {
            let mut model_guard = model
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            model_guard
                .embed(texts, None)
                .map_err(|e| anyhow::anyhow!("{}", e))
        })
        .await??;

        // Normalize all embeddings
        Ok(embeddings.into_iter().map(normalize_embedding).collect())
    }
}

// ============================================================================
// GGUF Embedding Provider (llama.cpp) - Optional, requires `gguf` feature
// ============================================================================

#[cfg(feature = "gguf")]
pub struct LlamaCppProvider {
    model: Arc<StdMutex<llama_cpp_2::model::LlamaModel>>,
    backend: Arc<llama_cpp_2::llama_backend::LlamaBackend>,
    model_name: String,
    dimensions: usize,
    #[allow(dead_code)] // Reserved for future HuggingFace download support
    cache_dir: Option<String>,
}

#[cfg(feature = "gguf")]
impl LlamaCppProvider {
    /// Create a new GGUF embedding provider
    ///
    /// # Arguments
    /// * `model_path` - Path to the GGUF model file, or HuggingFace model ID
    /// * `cache_dir` - Optional directory for caching downloaded models
    ///
    /// # Supported models
    /// - embeddinggemma-300M-Q8_0.gguf (~320MB, 1024 dims)
    /// - nomic-embed-text-v1.5.Q8_0.gguf (~270MB, 768 dims)
    /// - mxbai-embed-large-v1-q8_0.gguf (~670MB, 1024 dims)
    pub fn new(model_path: &str, cache_dir: Option<&str>) -> Result<Self> {
        use llama_cpp_2::llama_backend::LlamaBackend;
        use llama_cpp_2::model::LlamaModel;
        use llama_cpp_2::model::params::LlamaModelParams;

        // Initialize backend
        let backend = LlamaBackend::init()?;

        // Resolve model path - check if it's a file or needs downloading
        let resolved_path = Self::resolve_model_path(model_path, cache_dir)?;

        // Extract model name from path
        let model_name = std::path::Path::new(&resolved_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(model_path)
            .to_string();

        debug!("Loading GGUF embedding model: {}", model_name);

        // Load model with embedding mode enabled
        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, &resolved_path, &model_params)?;

        // Determine dimensions from model (embeddinggemma uses 1024)
        let dimensions = Self::get_model_dimensions(&model_name);

        Ok(Self {
            model: Arc::new(StdMutex::new(model)),
            backend: Arc::new(backend),
            model_name,
            dimensions,
            cache_dir: cache_dir.map(|s| s.to_string()),
        })
    }

    /// Resolve model path - download from HuggingFace if needed
    fn resolve_model_path(model_path: &str, cache_dir: Option<&str>) -> Result<String> {
        // If it's already a file path, use it directly
        let path = std::path::Path::new(model_path);
        if path.exists() {
            return Ok(model_path.to_string());
        }

        // Check in cache directory
        if let Some(cache) = cache_dir {
            let expanded = shellexpand::tilde(cache).to_string();
            let cached_path = std::path::Path::new(&expanded).join(model_path);
            if cached_path.exists() {
                return Ok(cached_path.to_string_lossy().to_string());
            }
        }

        // For now, require the model file to exist
        // TODO: Add HuggingFace download support like OpenClaw's resolveModelFile
        anyhow::bail!(
            "GGUF model file not found: '{}'. \n\
             Please download the model manually:\n\
             - embeddinggemma: https://huggingface.co/ggml-org/embeddinggemma-300M-GGUF\n\
             - nomic-embed: https://huggingface.co/nomic-ai/nomic-embed-text-v1.5-GGUF",
            model_path
        )
    }

    /// Get embedding dimensions for known models
    fn get_model_dimensions(model_name: &str) -> usize {
        let name_lower = model_name.to_lowercase();
        if name_lower.contains("embeddinggemma") {
            1024
        } else if name_lower.contains("nomic-embed") {
            768
        } else if name_lower.contains("mxbai-embed-large") {
            1024
        } else if name_lower.contains("bge") {
            768
        } else {
            // Default to common dimension
            768
        }
    }
}

#[cfg(feature = "gguf")]
#[async_trait]
impl EmbeddingProvider for LlamaCppProvider {
    fn id(&self) -> &str {
        "gguf"
    }

    fn model(&self) -> &str {
        &self.model_name
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.embed_batch(&[text.to_string()]).await?;
        results
            .pop()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let texts = texts.to_vec();
        let model = Arc::clone(&self.model);
        let backend = Arc::clone(&self.backend);
        let model_name = self.model_name.clone();

        // llama.cpp is synchronous, run in blocking task
        tokio::task::spawn_blocking(move || {
            use llama_cpp_2::context::params::LlamaContextParams;
            use llama_cpp_2::llama_batch::LlamaBatch;
            use llama_cpp_2::model::AddBos;

            let model_guard = model
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

            let mut all_results = Vec::with_capacity(texts.len());

            // Process in smaller sub-batches to avoid context window limits
            // 512 is a safe default for many embedding models
            const MAX_BATCH_TOKENS: usize = 2048;

            let mut current_sub_batch = Vec::new();
            let mut current_tokens_count = 0;

            for text in texts {
                let tokens = model_guard.str_to_token(&text, AddBos::Always)?;
                if current_tokens_count + tokens.len() > MAX_BATCH_TOKENS
                    && !current_sub_batch.is_empty()
                {
                    // Process current sub-batch
                    let sub_results =
                        process_sub_batch(&model_guard, &backend, &current_sub_batch)?;
                    all_results.extend(sub_results);
                    current_sub_batch.clear();
                    current_tokens_count = 0;
                }
                current_tokens_count += tokens.len();
                current_sub_batch.push(tokens);
            }

            // Process final sub-batch
            if !current_sub_batch.is_empty() {
                let sub_results = process_sub_batch(&model_guard, &backend, &current_sub_batch)?;
                all_results.extend(sub_results);
            }

            debug!(
                "Generated {} GGUF embeddings with {}",
                all_results.len(),
                model_name
            );

            Ok(all_results)
        })
        .await?
    }
}

#[cfg(feature = "gguf")]
fn process_sub_batch(
    model: &llama_cpp_2::model::LlamaModel,
    backend: &llama_cpp_2::llama_backend::LlamaBackend,
    sub_batch_tokens: &[Vec<llama_cpp_2::token::LlamaToken>],
) -> Result<Vec<Vec<f32>>> {
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_batch::LlamaBatch;

    let total_tokens: usize = sub_batch_tokens.iter().map(|v| v.len()).sum();
    let batch_len = sub_batch_tokens.len();

    // Create context for this sub-batch
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(Some(
            std::num::NonZeroU32::new((total_tokens + batch_len) as u32).unwrap(),
        ))
        .with_embeddings(true);

    let mut ctx = model.new_context(backend, ctx_params)?;

    // Create a batch with all tokens
    let mut batch = LlamaBatch::new(total_tokens as u32, batch_len as i32);
    for (seq_id, tokens) in sub_batch_tokens.iter().enumerate() {
        for (i, token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch.add(*token, i as i32, &[seq_id as i32], is_last)?;
        }
    }

    // Decode to generate embeddings
    ctx.decode(&mut batch)?;

    // Get embeddings from context
    let mut results = Vec::with_capacity(batch_len);
    for i in 0..batch_len {
        let embeddings = ctx.embeddings_seq_ith(i as i32)?;
        results.push(normalize_embedding(embeddings.to_vec()));
    }

    Ok(results)
}

// ============================================================================
// Gemini Embedding Provider - Free tier, multimodal with v2 model
// ============================================================================

/// Gemini embedding provider using Google's Generative Language API.
///
/// Models:
/// - `text-embedding-004`: 2048 token max, 768 dimensions
/// - `gemini-embedding-2-preview`: 8192 token max, 768/1536/3072 dimensions, multimodal
pub struct GeminiEmbeddingProvider {
    client: Client,
    api_key: String,
    model: String,
    dimensions: usize,
}

impl GeminiEmbeddingProvider {
    pub fn new(api_key: &str, model: Option<&str>) -> Self {
        let model = model.unwrap_or("text-embedding-004").to_string();
        let dimensions = match model.as_str() {
            "text-embedding-004" => 768,
            "gemini-embedding-2-preview" => 768, // default; supports 768/1536/3072
            _ => 768,
        };
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            model,
            dimensions,
        }
    }

    /// Embed text + image together using multimodal Gemini model.
    ///
    /// Requires `gemini-embedding-2-preview` model for multimodal support.
    pub async fn embed_multimodal(
        &self,
        text: &str,
        image_data: &[u8],
        image_mime_type: &str,
    ) -> Result<Vec<f32>> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_data);

        let request = GeminiEmbedRequest {
            content: GeminiContent {
                parts: vec![
                    GeminiPart::Text {
                        text: text.to_string(),
                    },
                    GeminiPart::InlineData {
                        inline_data: GeminiInlineData {
                            mime_type: image_mime_type.to_string(),
                            data: b64,
                        },
                    },
                ],
            },
            task_type: "RETRIEVAL_DOCUMENT".to_string(),
        };

        let url = format!("{}:embedContent?key={}", self.base_url(), self.api_key);
        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini multimodal API error {}: {}", status, body);
        }

        let result: GeminiEmbedResponse = response.json().await?;
        Ok(normalize_embedding(result.embedding.values))
    }

    fn base_url(&self) -> String {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}",
            self.model
        )
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbedRequest {
    content: GeminiContent,
    task_type: String,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum GeminiPart {
    Text {
        text: String,
    },
    #[serde(rename_all = "camelCase")]
    InlineData {
        inline_data: GeminiInlineData,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiInlineData {
    mime_type: String,
    data: String, // base64-encoded
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiBatchRequest {
    requests: Vec<GeminiBatchItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiBatchItem {
    model: String,
    content: GeminiContent,
    task_type: String,
}

#[derive(Deserialize)]
struct GeminiEmbedResponse {
    embedding: GeminiEmbeddingValues,
}

#[derive(Deserialize)]
struct GeminiEmbeddingValues {
    values: Vec<f32>,
}

#[derive(Deserialize)]
struct GeminiBatchResponse {
    embeddings: Vec<GeminiEmbeddingValues>,
}

#[async_trait]
impl EmbeddingProvider for GeminiEmbeddingProvider {
    fn id(&self) -> &str {
        "gemini"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let request = GeminiEmbedRequest {
            content: GeminiContent {
                parts: vec![GeminiPart::Text {
                    text: text.to_string(),
                }],
            },
            task_type: "RETRIEVAL_DOCUMENT".to_string(),
        };

        let url = format!("{}:embedContent?key={}", self.base_url(), self.api_key);

        let response = self.client.post(&url).json(&request).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error {}: {}", status, body);
        }

        let result: GeminiEmbedResponse = response.json().await?;
        Ok(normalize_embedding(result.embedding.values))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let model_path = format!("models/{}", self.model);
        let requests: Vec<GeminiBatchItem> = texts
            .iter()
            .map(|text| GeminiBatchItem {
                model: model_path.clone(),
                content: GeminiContent {
                    parts: vec![GeminiPart::Text { text: text.clone() }],
                },
                task_type: "RETRIEVAL_DOCUMENT".to_string(),
            })
            .collect();

        let batch = GeminiBatchRequest { requests };
        let url = format!(
            "{}:batchEmbedContents?key={}",
            self.base_url(),
            self.api_key
        );

        debug!("Embedding {} texts with Gemini {}", texts.len(), self.model);

        let response = self.client.post(&url).json(&batch).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini batch API error {}: {}", status, body);
        }

        let result: GeminiBatchResponse = response.json().await?;
        Ok(result
            .embeddings
            .into_iter()
            .map(|e| normalize_embedding(e.values))
            .collect())
    }
}

/// Compute cosine similarity between two normalized vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    // For normalized vectors, cosine similarity is just dot product
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Serialize embedding to JSON string for storage
pub fn serialize_embedding(embedding: &[f32]) -> String {
    serde_json::to_string(embedding).unwrap_or_else(|_| "[]".to_string())
}

/// Deserialize embedding from JSON string
pub fn deserialize_embedding(json: &str) -> Vec<f32> {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_embedding() {
        let vec = vec![3.0, 4.0];
        let normalized = normalize_embedding(vec);
        assert!((normalized[0] - 0.6).abs() < 1e-6);
        assert!((normalized[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);

        let c = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &c).abs() < 1e-6);
    }

    #[test]
    fn test_serialize_deserialize() {
        let embedding = vec![0.1, 0.2, 0.3];
        let json = serialize_embedding(&embedding);
        let deserialized = deserialize_embedding(&json);
        assert_eq!(embedding, deserialized);
    }

    #[test]
    fn test_gemini_multimodal_request_serialization() {
        // Verify the multimodal request format matches Gemini API expectations
        let request = GeminiEmbedRequest {
            content: GeminiContent {
                parts: vec![
                    GeminiPart::Text {
                        text: "a photo of a cat".to_string(),
                    },
                    GeminiPart::InlineData {
                        inline_data: GeminiInlineData {
                            mime_type: "image/jpeg".to_string(),
                            data: "base64data".to_string(),
                        },
                    },
                ],
            },
            task_type: "RETRIEVAL_DOCUMENT".to_string(),
        };

        let json = serde_json::to_value(&request).unwrap();

        // Verify structure
        assert_eq!(json["taskType"], "RETRIEVAL_DOCUMENT");
        assert_eq!(json["content"]["parts"][0]["text"], "a photo of a cat");
        assert_eq!(
            json["content"]["parts"][1]["inlineData"]["mimeType"],
            "image/jpeg"
        );
        assert_eq!(
            json["content"]["parts"][1]["inlineData"]["data"],
            "base64data"
        );
    }

    #[test]
    fn test_gemini_text_only_request_serialization() {
        let request = GeminiEmbedRequest {
            content: GeminiContent {
                parts: vec![GeminiPart::Text {
                    text: "hello world".to_string(),
                }],
            },
            task_type: "RETRIEVAL_DOCUMENT".to_string(),
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["content"]["parts"][0]["text"], "hello world");
        // No inlineData in text-only request
        assert!(json["content"]["parts"][0].get("inlineData").is_none());
    }
}
