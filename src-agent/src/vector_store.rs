//! Vector store for semantic memory search.
//!
//! This module provides a flat, in-memory embedding store backed by:
//! - An `IndexMap<String, VectorMemoryDoc>` for O(1) document lookup with
//!   stable iteration order (ensuring the embedding flat array stays
//!   in sync with document indices).
//! - A `Vec<f32>` flattened embedding array, where document `i` occupies
//!   indices `[i * embedding_dim .. (i+1) * embedding_dim)`.
//!
//! # Search Algorithm
//!
//! `semantic_search()` performs **O(n) brute-force cosine similarity** over
//! every stored embedding. This is acceptable for workloads under ~10,000
//! entries. Beyond that threshold, you should integrate an ANN index
//! (e.g., HNSW via the `hnswx` crate, which is reserved as a dependency
//! but not yet wired in).
//!
//! # Current Limitations
//!
//! - No ANN index (search is O(n) linear scan).
//! - Embeddings must be computed externally and supplied to `insert()`;
//!   no automatic embedding generation is performed by this module.
//! - No hybrid scoring with keyword FTS5 is implemented.
//!
//! # Performance Guidance
//!
//! | Dataset Size    | Recommended Approach         |
//! |-----------------|------------------------------|
//! | < 10,000 docs   | O(n) brute force (current)   |
//! | > 10,000 docs   | Switch to HNSW / ANN index   |

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::AgentResult;
use crate::task::MemoryEntry;

/// Memory document for vector storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMemoryDoc {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub source: String,
    pub created_at: String,
}

impl VectorMemoryDoc {
    /// Create a VectorMemoryDoc from a MemoryEntry
    pub fn from_memory_entry(entry: &MemoryEntry) -> Self {
        Self {
            id: entry.id.clone(),
            content: entry.content.clone(),
            tags: entry.tags.clone(),
            source: entry.source.clone(),
            created_at: entry.created_at.clone(),
        }
    }
}

/// Search result with relevance score
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
}

impl SearchResult {
    pub fn new(id: String, score: f32) -> Self {
        Self { id, score }
    }
}

/// Hybrid vector store combining keyword and semantic search.
///
/// Stores documents in an `IndexMap` for O(1) lookup with stable iteration
/// order, paired with a flat `Vec<f32>` embedding array. The embedding for
/// document at position `i` lives at indices
/// `[i * embedding_dim .. (i+1) * embedding_dim)` so that both structures
/// stay synchronised.
///
/// **Search is O(n) brute-force cosine similarity** — see the module-level
/// documentation for performance guidance and when to switch to ANN.
///
/// # Usage
///
/// ```rust
/// # use rupoo::vector_store::{VectorStore, VectorMemoryDoc};
/// let mut store = VectorStore::new(384); // 384-dim (all-MiniLM-L6-v2)
/// assert_eq!(store.len(), 0);
/// assert!(store.is_empty());
/// ```
pub struct VectorStore {
    /// Store documents and their embeddings
    documents: IndexMap<String, VectorMemoryDoc>,
    /// Store embeddings as flat Vec<f32> arrays
    /// Format: [id1_emb_0, id1_emb_1, ..., id2_emb_0, id2_emb_1, ...]
    embeddings: Vec<f32>,
    /// Embedding dimension
    embedding_dim: usize,
}

impl VectorStore {
    /// Create a new vector store with specified embedding dimension.
    ///
    /// Common dimensions:
    /// - 384: Lightweight models (e.g., all-MiniLM-L6-v2)
    /// - 768: Standard models (e.g., text-embedding-ada-002)
    /// - 1536: High-quality models (e.g., text-embedding-3-large)
    pub fn new(embedding_dim: usize) -> Self {
        Self {
            documents: IndexMap::new(),
            embeddings: Vec::new(),
            embedding_dim,
        }
    }

    /// Default vector store with 384 dimensions (lightweight, fast)
    pub fn with_default_dim() -> Self {
        Self::new(384)
    }

    /// Insert a memory entry into the vector store.
    ///
    /// Note: This stores the document but doesn't compute embeddings yet.
    /// Embeddings should be computed externally using an embedding model.
    ///
    /// # TODO
    ///
    /// - Add automatic embedding generation using LLM provider
    /// - Integrate with MemoryStore for automatic vector indexing
    pub async fn insert(&mut self, doc: VectorMemoryDoc, embedding: Vec<f32>) -> AgentResult<()> {
        if embedding.len() != self.embedding_dim {
            tracing::warn!(
                expected = self.embedding_dim,
                actual = embedding.len(),
                "embedding dimension mismatch, skipping"
            );
            return Ok(());
        }

        let id = doc.id.clone();
        self.documents.insert(id.clone(), doc);
        self.embeddings.extend(embedding);

        debug!(id, "memory inserted into vector store");
        Ok(())
    }

    /// Search by semantic similarity using brute-force O(n) cosine similarity.
    ///
    /// Every stored embedding is compared to the query. This is a linear
    /// scan — acceptable for < 10,000 entries, but should be replaced with
    /// an ANN index (e.g. HNSW via `hnswx`) for larger datasets.
    ///
    /// Returns document IDs with similarity scores sorted by relevance.
    ///
    /// # TODO
    ///
    /// - Add approximate nearest neighbor (ANN) for sub-linear search
    pub async fn semantic_search(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
    ) -> AgentResult<Vec<SearchResult>> {
        if query_embedding.len() != self.embedding_dim {
            return Err(crate::error::AgentError::Other(format!(
                "embedding dimension mismatch: expected {}, got {}",
                self.embedding_dim,
                query_embedding.len()
            )));
        }

        // Calculate cosine similarity with all stored embeddings
        let mut results: Vec<SearchResult> = Vec::new();
        let num_docs = self.embeddings.len() / self.embedding_dim;

        for i in 0..num_docs {
            let start = i * self.embedding_dim;
            let end = start + self.embedding_dim;
            let doc_embedding = &self.embeddings[start..end];

            // Cosine similarity
            let similarity = self.cosine_similarity(&query_embedding, doc_embedding);

            // Get document ID (IndexMap guarantees index == position)
            if let Some((id, _)) = self.documents.get_index(i) {
                results.push(SearchResult::new(id.clone(), similarity));
            }
        }

        // Sort by score descending
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);

        debug!(count = results.len(), "semantic search completed");
        Ok(results)
    }

    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        let mut dot_product = 0.0;
        let mut norm_a = 0.0;
        let mut norm_b = 0.0;

        for i in 0..a.len() {
            dot_product += a[i] * b[i];
            norm_a += a[i] * a[i];
            norm_b += b[i] * b[i];
        }

        let norm_product = norm_a.sqrt() * norm_b.sqrt();
        if norm_product == 0.0 {
            0.0
        } else {
            dot_product / norm_product
        }
    }

    /// Remove a memory entry and its embedding from the vector store.
    ///
    /// Uses the stable index from `IndexMap` to locate and drain the
    /// corresponding slice from the flat embedding array, preventing
    /// memory leaks and phantom search results.
    pub async fn remove(&mut self, id: &str) -> AgentResult<()> {
        if let Some(idx) = self.documents.get_index_of(id) {
            // shift_remove preserves the order of remaining entries, keeping
            // the flat embedding array index aligned with document position
            self.documents.shift_remove(id);
            let start = idx * self.embedding_dim;
            let end = start + self.embedding_dim;
            if end <= self.embeddings.len() {
                self.embeddings.drain(start..end);
            }
            debug!(id, "memory and embedding removed from vector store");
        }
        Ok(())
    }

    /// Get the number of documents in the vector store.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Check if the vector store is empty.
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
}

impl Default for VectorStore {
    fn default() -> Self {
        Self::with_default_dim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vector_store_insert() {
        let mut store = VectorStore::with_default_dim();

        let doc = VectorMemoryDoc {
            id: "test-1".to_string(),
            content: "Rust is a great programming language".to_string(),
            tags: vec!["rust".to_string()],
            source: "test".to_string(),
            created_at: "2025-01-01".to_string(),
        };

        // Create a dummy embedding (384 dimensions)
        let embedding: Vec<f32> = (0..384).map(|i| (i as f32) * 0.01).collect();

        store.insert(doc, embedding).await.unwrap();

        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn test_vector_store_search() {
        let mut store = VectorStore::with_default_dim();

        // Insert test document
        let doc = VectorMemoryDoc {
            id: "doc-1".to_string(),
            content: "Rust programming".to_string(),
            tags: vec!["rust".to_string()],
            source: "test".to_string(),
            created_at: "2025-01-01".to_string(),
        };

        let embedding: Vec<f32> = vec![1.0; 384];
        store.insert(doc, embedding).await.unwrap();

        // Search with same embedding (should have high similarity)
        let query: Vec<f32> = vec![1.0; 384];
        let results = store.semantic_search(query, 10).await.unwrap();

        assert!(!results.is_empty());
        assert_eq!(results[0].id, "doc-1");
        assert!((results[0].score - 1.0).abs() < 0.001); // Perfect match
    }

    #[tokio::test]
    async fn test_vector_store_dimension_mismatch() {
        let mut store = VectorStore::new(384);

        let doc = VectorMemoryDoc {
            id: "test-1".to_string(),
            content: "test".to_string(),
            tags: vec![],
            source: "test".to_string(),
            created_at: "2025-01-01".to_string(),
        };

        // Wrong dimension
        let wrong_embedding: Vec<f32> = vec![1.0; 768];

        let result = store.insert(doc, wrong_embedding).await;
        assert!(result.is_ok()); // Should skip gracefully
        assert_eq!(store.len(), 0); // Not inserted
    }
}
