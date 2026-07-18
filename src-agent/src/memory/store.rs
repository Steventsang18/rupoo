//! High-level memory operations for the long-term memory system.
//!
//! Wraps TaskRepo's FTS5-backed memory methods with hybrid search support.
//!
//! # Architecture
//!
//! - **FTS5 Search**: Full-text search using SQLite FTS5 (keyword matching)
//! - **Vector Search**: Semantic search using embeddings (new)
//! - **Hybrid Search**: Combines both for optimal relevance
//!
//! # Hybrid Search Design
//!
//! ```text
//! User Query
//!     |
//!     +---> FTS5 Search (keyword matching)
//!     |         |
//!     |         +---> Exact matches, keyword hits
//!     |
//!     +---> Vector Search (semantic)
//!               |
//!               +---> Similar meanings, intent understanding
//!
//! Combined Results (RRF ranking)
//! ```
//!

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::db::TaskRepo;
use crate::embedding::EmbeddingService;
use crate::error::AgentResult;
use crate::task::MemoryEntry;
use crate::vector_store::{VectorMemoryDoc, VectorStore};

/// Configuration for hybrid search.
#[derive(Debug, Clone)]
pub struct HybridSearchConfig {
    /// Enable vector search (requires embedding service).
    pub enable_vector_search: bool,
    /// Weight for FTS5 results (0.0 to 1.0).
    pub fts_weight: f32,
    /// Weight for vector search results (0.0 to 1.0).
    pub vector_weight: f32,
    /// Minimum similarity threshold for vector search.
    pub min_similarity: f32,
    /// Use Reciprocal Rank Fusion for combining results.
    pub use_rrf: bool,
    /// RRF constant (typically 60).
    pub rrf_k: u32,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            enable_vector_search: true,
            fts_weight: 0.5,
            vector_weight: 0.5,
            min_similarity: 0.3,
            use_rrf: true,
            rrf_k: 60,
        }
    }
}

impl HybridSearchConfig {
    /// Create config with vector search disabled.
    pub fn fts_only() -> Self {
        Self {
            enable_vector_search: false,
            ..Default::default()
        }
    }

    /// Create config optimized for semantic search.
    pub fn semantic_focused() -> Self {
        Self {
            enable_vector_search: true,
            fts_weight: 0.3,
            vector_weight: 0.7,
            min_similarity: 0.2,
            ..Default::default()
        }
    }

    /// Create config optimized for keyword matching.
    pub fn keyword_focused() -> Self {
        Self {
            enable_vector_search: true,
            fts_weight: 0.7,
            vector_weight: 0.3,
            min_similarity: 0.4,
            ..Default::default()
        }
    }
}

/// High-level memory operations for the long-term memory system.
///
/// Supports hybrid search combining:
/// - FTS5 full-text search (keyword matching)
/// - Vector semantic search (intent understanding)
pub struct MemoryStore {
    repo: Arc<TaskRepo>,
    /// Default source tag applied when none is provided.
    default_source: String,
    /// Optional vector store for semantic search.
    vector_store: Option<Arc<RwLock<VectorStore>>>,
    /// Optional embedding service for generating embeddings.
    embedding_service: Option<Arc<EmbeddingService>>,
    /// Hybrid search configuration.
    config: HybridSearchConfig,
}

impl MemoryStore {
    /// Create a basic memory store with FTS5 search only.
    pub fn new(repo: Arc<TaskRepo>) -> Self {
        Self {
            repo,
            default_source: "agent".to_string(),
            vector_store: None,
            embedding_service: None,
            config: HybridSearchConfig::fts_only(),
        }
    }

    /// Create a memory store with hybrid search support.
    ///
    /// # Arguments
    ///
    /// * `repo` - Database repository
    /// * `embedding_service` - Optional embedding service for vector search
    /// * `config` - Hybrid search configuration
    pub fn with_hybrid_search(
        repo: Arc<TaskRepo>,
        embedding_service: Option<Arc<EmbeddingService>>,
        config: HybridSearchConfig,
    ) -> Self {
        let vector_store = if config.enable_vector_search {
            if let Some(ref svc) = embedding_service {
                let dim = svc.dimension();
                Some(Arc::new(RwLock::new(VectorStore::new(dim))))
            } else {
                None
            }
        } else {
            None
        };

        Self {
            repo,
            default_source: "agent".to_string(),
            vector_store,
            embedding_service,
            config,
        }
    }

    /// Enable or disable vector search at runtime.
    pub async fn set_vector_search_enabled(&mut self, enabled: bool) {
        self.config.enable_vector_search = enabled;
        if enabled && self.vector_store.is_none() {
            if let Some(ref svc) = self.embedding_service {
                let dim = svc.dimension();
                self.vector_store = Some(Arc::new(RwLock::new(VectorStore::new(dim))));
            }
        } else if !enabled {
            self.vector_store = None;
        }
    }

    /// Store a memory entry with automatic timestamp.
    ///
    /// If vector search is enabled, automatically generates and stores embedding.
    pub async fn remember(&self, content: &str, tags: &[&str]) -> AgentResult<String> {
        let id = self
            .repo
            .store_memory(content, tags, &self.default_source)
            .await?;

        // Use floor_char_boundary to safely cut at character boundary for UTF-8
        let preview_len = content.len().min(60);
        let preview_end = content.floor_char_boundary(preview_len);
        info!(
            memory_id = %id,
            content_preview = &content[..preview_end],
            "memory stored (FTS5 indexed)"
        );

        // Generate and store embedding if vector search is enabled
        if self.config.enable_vector_search {
            if let (Some(vs), Some(es)) = (&self.vector_store, &self.embedding_service) {
                match es.embed(content).await {
                    Ok(embedding) => {
                        let entry = self.repo.get_memory(&id).await?;
                        if let Some(entry) = entry {
                            let doc = VectorMemoryDoc::from_memory_entry(&entry);
                            let mut vs = vs.write().await;
                            if let Err(e) = vs.insert(doc, embedding).await {
                                warn!(error = %e, "failed to store embedding");
                            } else {
                                debug!(memory_id = %id, "embedding stored");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to generate embedding");
                    }
                }
            }
        }

        Ok(id)
    }

    /// Store a memory with an explicit source.
    pub async fn remember_from(
        &self,
        content: &str,
        tags: &[&str],
        source: &str,
    ) -> AgentResult<String> {
        let id = self.repo.store_memory(content, tags, source).await?;

        info!(
            memory_id = %id,
            source = %source,
            "memory stored from source (FTS5 indexed)"
        );

        // Generate and store embedding if vector search is enabled
        if self.config.enable_vector_search {
            if let (Some(vs), Some(es)) = (&self.vector_store, &self.embedding_service) {
                match es.embed(content).await {
                    Ok(embedding) => {
                        let entry = self.repo.get_memory(&id).await?;
                        if let Some(entry) = entry {
                            let doc = VectorMemoryDoc::from_memory_entry(&entry);
                            let mut vs = vs.write().await;
                            if let Err(e) = vs.insert(doc, embedding).await {
                                warn!(error = %e, "failed to store embedding");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to generate embedding");
                    }
                }
            }
        }

        Ok(id)
    }

    /// Search memories using hybrid search (FTS5 + Vector).
    ///
    /// Combines keyword matching with semantic understanding for
    /// optimal relevance ranking.
    ///
    /// # Algorithm
    ///
    /// 1. Perform FTS5 search for keyword matches
    /// 2. Perform vector search for semantic matches
    /// 3. Combine results using Reciprocal Rank Fusion (RRF)
    ///
    /// # RRF Formula
    ///
    /// ```text
    /// RRF_score(d) = Σ (1 / (k + rank(d)))
    /// ```
    pub async fn recall(&self, query: &str, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        // If vector search is disabled or not available, use FTS5 only
        if !self.config.enable_vector_search
            || self.vector_store.is_none()
            || self.embedding_service.is_none()
        {
            debug!("using FTS5 search only");
            return self.repo.search_memories(query, limit).await;
        }

        debug!("performing hybrid search");

        // Perform both searches in parallel
        let fts_future = self.repo.search_memories(query, limit * 2);
        let embedding_future = self
            .embedding_service
            .as_ref()
            .expect("hybrid_search requires embedding_service — call Agent::with_embedding_service() during setup")
            .embed(query);

        let (fts_results, embedding) = tokio::try_join!(fts_future, embedding_future)?;

        // Perform vector search
        let vector_results = {
            let vs = self
                .vector_store
                .as_ref()
                .expect("hybrid_search requires vector_store — call Agent::with_vector_store() during setup")
                .read().await;
            vs.semantic_search(embedding, limit * 2).await?
        };

        // Combine results using RRF
        let (mut combined, vector_only_ids) =
            self.combine_results_rrf(&fts_results, &vector_results, limit);

        // Fetch vector-only entries from DB (IDs present in vector results but not FTS)
        for id in &vector_only_ids {
            if combined.len() >= limit {
                break;
            }
            if let Some(entry) = self.repo.get_memory(id).await? {
                combined.push(entry);
            }
        }

        debug!(
            fts_count = fts_results.len(),
            vector_count = vector_results.len(),
            combined_count = combined.len(),
            vector_only_fetched = vector_only_ids.len(),
            "hybrid search completed"
        );

        Ok(combined)
    }

    /// Combine FTS and vector results using Reciprocal Rank Fusion.
    ///
    /// Returns a tuple of:
    /// - Resolved `MemoryEntry` items (those found in `fts_results`)
    /// - Vector-only IDs (present in `vector_results` but not in `fts_results`),
    ///   which the caller should fetch from the DB to complete the result set.
    fn combine_results_rrf(
        &self,
        fts_results: &[MemoryEntry],
        vector_results: &[crate::vector_store::SearchResult],
        limit: usize,
    ) -> (Vec<MemoryEntry>, Vec<String>) {
        let k = self.config.rrf_k;
        let mut scores: HashMap<String, f32> = HashMap::new();

        // Add FTS scores
        for (rank, entry) in fts_results.iter().enumerate() {
            let rrf_score = 1.0 / (k as f32 + (rank + 1) as f32);
            *scores.entry(entry.id.clone()).or_default() += rrf_score * self.config.fts_weight;
        }

        // Add vector scores
        for (rank, result) in vector_results.iter().enumerate() {
            let rrf_score = 1.0 / (k as f32 + (rank + 1) as f32);
            *scores.entry(result.id.clone()).or_default() += rrf_score * self.config.vector_weight;
        }

        // Sort by combined score
        let mut sorted_ids: Vec<_> = scores.into_iter().collect();
        sorted_ids.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Build FTS lookup map
        let fts_map: HashMap<&str, &MemoryEntry> =
            fts_results.iter().map(|e| (e.id.as_str(), e)).collect();

        // Resolve entries: FTS hits are resolved; vector-only IDs are collected for DB fetch
        let mut resolved: Vec<MemoryEntry> = Vec::new();
        let mut vector_only_ids: Vec<String> = Vec::new();

        for (id, _score) in sorted_ids.into_iter().take(limit) {
            if let Some(entry) = fts_map.get(id.as_str()) {
                resolved.push((*entry).clone());
            } else {
                // This ID exists only in vector results — caller must fetch from DB
                vector_only_ids.push(id);
            }
        }

        (resolved, vector_only_ids)
    }

    /// Get the most recent memories.
    pub async fn recent(&self, limit: usize) -> AgentResult<Vec<MemoryEntry>> {
        self.repo.recent_memories(limit).await
    }

    /// Format memories into a context string for prompt injection.
    pub fn format_context(memories: &[MemoryEntry]) -> String {
        if memories.is_empty() {
            return String::new();
        }
        let mut ctx = String::from("--- Relevant Memories ---\n");
        for mem in memories {
            ctx.push_str(&format!(
                "- [{}] {} (tags: {})\n",
                mem.created_at,
                mem.content,
                mem.tags.join(", ")
            ));
        }
        ctx.push_str("--- End Memories ---");
        ctx
    }

    /// Get the number of memories stored.
    pub async fn count(&self) -> AgentResult<usize> {
        self.repo.count_memories().await
    }

    /// Check if hybrid search is enabled.
    pub fn is_hybrid_enabled(&self) -> bool {
        self.config.enable_vector_search && self.vector_store.is_some()
    }

    /// Delete a memory entry by its content_id.
    ///
    /// # Preconditions
    /// - `id` must be a valid content_id returned by [`remember`] or [`remember_from`].
    ///
    /// # Postconditions
    /// - The memory entry is removed from the database.
    /// - If no matching entry exists, a warning is logged but no error is returned.
    pub async fn delete(&self, id: &str) -> AgentResult<()> {
        self.repo.delete_memory(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TaskRepo;

    fn setup() -> (Arc<TaskRepo>, MemoryStore) {
        let repo = Arc::new(TaskRepo::new(":memory:").unwrap());
        let store = MemoryStore::new(Arc::clone(&repo));
        (repo, store)
    }

    #[tokio::test]
    async fn test_store_and_search_memory() {
        let (_, store) = setup();
        store
            .remember(
                "The user prefers Rust over Python for system programming",
                &["language", "preference"],
            )
            .await
            .unwrap();

        let results = store.recall("Rust programming", 10).await.unwrap();
        assert!(!results.is_empty(), "should find the memory");
        assert!(results[0].content.contains("Rust"));
    }

    #[tokio::test]
    async fn test_recent_memories() {
        let (_, store) = setup();
        store.remember("memory one", &["a"]).await.unwrap();
        store.remember("memory two", &["b"]).await.unwrap();

        let recent = store.recent(5).await.unwrap();
        assert_eq!(recent.len(), 2);
        // Most recent first
        assert!(recent[0].content.contains("two"));
    }

    #[tokio::test]
    async fn test_format_context_empty() {
        let ctx = MemoryStore::format_context(&[]);
        assert!(ctx.is_empty());
    }

    #[tokio::test]
    async fn test_format_context_with_memories() {
        let mems = vec![MemoryEntry {
            id: "1".into(),
            content: "user likes Rust".into(),
            tags: vec!["lang".into()],
            source: "test".into(),
            created_at: "2025-01-01".into(),
            updated_at: "2025-01-01".into(),
        }];
        let ctx = MemoryStore::format_context(&mems);
        assert!(ctx.contains("Rust"));
        assert!(ctx.contains("Relevant Memories"));
    }

    #[tokio::test]
    async fn test_hybrid_search_config_defaults() {
        let config = HybridSearchConfig::default();
        assert!(config.enable_vector_search);
        assert!((config.fts_weight - 0.5).abs() < 0.001);
        assert!((config.vector_weight - 0.5).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_fts_only_config() {
        let config = HybridSearchConfig::fts_only();
        assert!(!config.enable_vector_search);
    }

    /// H3 regression: vector-only IDs must not be silently dropped by RRF.
    #[test]
    fn test_combine_results_rrf_includes_vector_only_ids() {
        let (_, store) = setup();

        let fts_results = vec![MemoryEntry {
            id: "fts-1".into(),
            content: "fts match".into(),
            tags: vec![],
            source: "test".into(),
            created_at: "2025-01-01".into(),
            updated_at: "2025-01-01".into(),
        }];

        let vector_results = vec![
            crate::vector_store::SearchResult::new("fts-1".into(), 0.9),
            // This ID exists only in vector results — must NOT be dropped
            crate::vector_store::SearchResult::new("vec-only-1".into(), 0.8),
        ];

        let (resolved, vector_only_ids) =
            store.combine_results_rrf(&fts_results, &vector_results, 10);

        // FTS entry should be resolved
        assert_eq!(resolved.len(), 1, "FTS entry should be resolved");
        assert_eq!(resolved[0].id, "fts-1");

        // Vector-only ID should be reported for DB fetch, not silently dropped
        assert_eq!(
            vector_only_ids.len(),
            1,
            "vector-only ID must be returned for DB fetch"
        );
        assert_eq!(vector_only_ids[0], "vec-only-1");
    }
}
