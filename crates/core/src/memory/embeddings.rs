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
