//! Embedding service for vector search.
//!
//! Provides text embeddings using LLM providers (OpenAI, etc.)
//! for semantic search in the memory system.

use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::error::{AgentError, AgentResult};
use crate::llm::LlmConfig;
use crate::llm::LlmProvider;

/// Embedding service for generating text embeddings.
///
/// Supports multiple providers:
/// - OpenAI: text-embedding-3-small (default), text-embedding-3-large, text-embedding-ada-002
/// - Ollama: nomic-embed-text, all-minilm (local models)
///
/// # Example
///
/// ```rust,no_run
/// # use rupoo::embedding::EmbeddingService;
/// # use rupoo::llm::{LlmConfig, LlmProvider};
/// # use std::sync::Arc;
/// # let config = LlmConfig::new(LlmProvider::OpenAI, Some("api-key".to_string()));
/// # let http_client = Arc::new(reqwest::Client::new());
/// let service = EmbeddingService::new(&config, &http_client)?;
/// let embedding = service.embed("Hello world").await?;
/// ```
pub struct EmbeddingService {
    /// Dimension of embeddings
    dimension: usize,
    /// Provider name for logging
    provider: String,
    /// HTTP client for API calls
    http_client: Arc<reqwest::Client>,
    /// API key (if required)
    api_key: Option<String>,
    /// Base URL (for Ollama or custom endpoints)
    base_url: Option<String>,
    /// Model name
    model: String,
}

impl EmbeddingService {
    /// Create a new embedding service from LLM config.
    ///
    /// # Arguments
    ///
    /// * `config` - LLM configuration
    /// * `http_client` - Shared HTTP client for connection pooling
    ///
    /// # Default Models
    ///
    /// - OpenAI: text-embedding-3-small (1536 dimensions)
    /// - Ollama: nomic-embed-text (768 dimensions)
    pub fn new(config: &LlmConfig, http_client: &Arc<reqwest::Client>) -> AgentResult<Self> {
        match &config.provider {
            LlmProvider::OpenAI => {
                let api_key = config.api_key.clone()
                    .ok_or_else(|| AgentError::Config(
                        "OpenAI embedding requires an API key. Set it via: rupoo config set api_key.openai <key>".into()
                    ))?;

                // Use text-embedding-3-small by default (good balance of quality and cost)
                let model = config
                    .embedding_model
                    .clone()
                    .unwrap_or_else(|| "text-embedding-3-small".to_string());

                let dimension = Self::get_openai_dimension(&model);

                info!(
                    provider = "openai",
                    model = %model,
                    dimension = dimension,
                    "embedding service initialized"
                );

                Ok(Self {
                    dimension,
                    provider: "openai".to_string(),
                    http_client: Arc::clone(http_client),
                    api_key: Some(api_key),
                    base_url: config.base_url.clone(),
                    model,
                })
            }

            LlmProvider::Ollama => {
                let base_url = config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434".to_string());

                // Use nomic-embed-text by default (good quality, small footprint)
                let model = config
                    .embedding_model
                    .clone()
                    .unwrap_or_else(|| "nomic-embed-text".to_string());

                let dimension = Self::get_ollama_dimension(&model);

                info!(
                    provider = "ollama",
                    model = %model,
                    dimension = dimension,
                    "embedding service initialized"
                );

                Ok(Self {
                    dimension,
                    provider: "ollama".to_string(),
                    http_client: Arc::clone(http_client),
                    api_key: None,
                    base_url: Some(base_url),
                    model,
                })
            }

            LlmProvider::Anthropic => {
                warn!(
                    "Anthropic does not support embeddings. Using fallback: hash-based embedding."
                );

                // For Anthropic, we'll use a simple hash-based embedding
                // This is not ideal but provides a fallback
                Ok(Self {
                    dimension: 384,
                    provider: "anthropic-fallback".to_string(),
                    http_client: Arc::clone(http_client),
                    api_key: None,
                    base_url: None,
                    model: "fallback".to_string(),
                })
            }
        }
    }

    /// Get embedding dimension for OpenAI models
    fn get_openai_dimension(model: &str) -> usize {
        match model {
            "text-embedding-3-small" => 1536,
            "text-embedding-3-large" => 3072,
            "text-embedding-ada-002" => 1536,
            _ => 1536, // Default to 1536
        }
    }

    /// Get embedding dimension for Ollama models
    fn get_ollama_dimension(model: &str) -> usize {
        match model {
            "nomic-embed-text" => 768,
            "all-minilm" => 384,
            "mxbai-embed-large" => 1024,
            _ => 768, // Default to 768
        }
    }

    /// Generate embedding for a text string.
    ///
    /// # Arguments
    ///
    /// * `text` - Text to embed
    ///
    /// # Returns
    ///
    /// A vector of f32 values representing the embedding
    pub async fn embed(&self, text: &str) -> AgentResult<Vec<f32>> {
        debug!(
            provider = %self.provider,
            text_len = text.len(),
            "generating embedding"
        );

        let embedding = match self.provider.as_str() {
            "openai" => self.embed_openai(text).await?,
            "ollama" => self.embed_ollama(text).await?,
            "anthropic-fallback" => self.embed_fallback(text).await?,
            _ => {
                return Err(AgentError::Other(format!(
                    "Unknown embedding provider: {}",
                    self.provider
                )))
            }
        };

        debug!(dimension = embedding.len(), "embedding generated");

        Ok(embedding)
    }

    /// Generate embedding using OpenAI API
    async fn embed_openai(&self, text: &str) -> AgentResult<Vec<f32>> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| AgentError::Config("OpenAI API key not set".into()))?;

        let base_url = self
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1");

        let url = format!("{}/embeddings", base_url);

        let request = serde_json::json!({
            "model": self.model,
            "input": text,
        });

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| AgentError::Llm(format!("OpenAI embedding request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::Llm(format!(
                "OpenAI embedding failed: {} - {}",
                status, body
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AgentError::Llm(format!("Failed to parse OpenAI response: {}", e)))?;

        // Extract embedding from response
        let embedding = json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| AgentError::Llm("Invalid OpenAI embedding response".into()))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
    }

    /// Generate embedding using Ollama API
    async fn embed_ollama(&self, text: &str) -> AgentResult<Vec<f32>> {
        let base_url = self
            .base_url
            .as_ref()
            .ok_or_else(|| AgentError::Config("Ollama base URL not set".into()))?;

        let url = format!("{}/api/embeddings", base_url);

        let request = serde_json::json!({
            "model": self.model,
            "prompt": text,
        });

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| AgentError::Llm(format!("Ollama embedding request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::Llm(format!(
                "Ollama embedding failed: {} - {}",
                status, body
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AgentError::Llm(format!("Failed to parse Ollama response: {}", e)))?;

        // Extract embedding from response
        let embedding = json["embedding"]
            .as_array()
            .ok_or_else(|| AgentError::Llm("Invalid Ollama embedding response".into()))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
    }

    /// Generate fallback embedding using hash (for providers without embedding support)
    async fn embed_fallback(&self, text: &str) -> AgentResult<Vec<f32>> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Simple hash-based embedding (for fallback only)
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();

        // Generate pseudo-random embedding from hash
        let mut embedding = Vec::with_capacity(self.dimension);
        let mut state = hash;
        for _ in 0..self.dimension {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            embedding.push((state as f32 / u64::MAX as f32 - 0.5) * 2.0);
        }

        // Normalize
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut embedding {
                *x /= norm;
            }
        }

        Ok(embedding)
    }

    /// Get the dimension of embeddings produced by this service.
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Get the provider name.
    pub fn provider(&self) -> &str {
        &self.provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fallback_embedding() {
        let http_client = Arc::new(reqwest::Client::new());
        let service = EmbeddingService {
            dimension: 384,
            provider: "anthropic-fallback".to_string(),
            http_client,
            api_key: None,
            base_url: None,
            model: "fallback".to_string(),
        };

        let result = service.embed("test text").await.unwrap();

        assert_eq!(result.len(), 384);

        // Same text should produce same embedding
        let result2 = service.embed("test text").await.unwrap();
        assert_eq!(result, result2);

        // Different text should produce different embedding
        let result3 = service.embed("different text").await.unwrap();
        assert_ne!(result, result3);
    }

    #[test]
    fn test_openai_dimensions() {
        assert_eq!(
            EmbeddingService::get_openai_dimension("text-embedding-3-small"),
            1536
        );
        assert_eq!(
            EmbeddingService::get_openai_dimension("text-embedding-3-large"),
            3072
        );
        assert_eq!(
            EmbeddingService::get_openai_dimension("text-embedding-ada-002"),
            1536
        );
    }

    #[test]
    fn test_ollama_dimensions() {
        assert_eq!(
            EmbeddingService::get_ollama_dimension("nomic-embed-text"),
            768
        );
        assert_eq!(EmbeddingService::get_ollama_dimension("all-minilm"), 384);
    }
}
