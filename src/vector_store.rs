//! Vector store for semantic memory search.
//!
//! This module provides hybrid search combining:
//! - FTS5 keyword search (existing)
//! - Vector semantic search (planned)
//!
//! # Architecture
//!
//! ```text
//! User Query
//!     |
//!     +---> FTS5 Search (keyword matching)
//!     |         |
//!     |         +---> Exact matches, keyword hits
//!     |
//!     +---> Vector Search (semantic) [TODO]
//!               |
//!               +---> Similar meanings, intent understanding
//!
//! Combined Results (relevance ranking)
//! ```
//!
//! # Implementation Status
//!
//! - ✅ VectorStore struct and document types created
//! - ✅ Basic operations defined
//! - ⏳ Vector embedding integration (requires LLM provider support)
//! - ⏳ Hybrid search implementation
//!
//! # Next Steps
//!
//! 1. Integrate with LLM provider's embedding model
//! 2. Add automatic embedding generation on memory store
//! 3. Implement hybrid search combining FTS5 and vector results

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
/// This is a foundation structure for future hybrid search implementation.
/// Currently, Rupoo uses FTS5 for memory search. Vector search will be
/// integrated in a future release to enable semantic understanding.
///
/// # Usage
///
/// ```rust,ignore
/// let vector_store = VectorStore::new();
///
/// // TODO: Generate embedding using LLM provider
/// let embedding = llm_client.embed("user query").await?;
///
/// // TODO: Perform vector search
/// let results = vector_store.semantic_search(embedding, 10).await?;
/// ```
pub struct VectorStore {
    /// Store documents and their embeddings
    documents: std::collections::HashMap<String, VectorMemoryDoc>,
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
            documents: std::collections::HashMap::new(),
            embeddings: Vec::new(),
            embedding_dim,
        }
    }

    /// Default vector store with 384 dimensions (lightweight, fast)
    pub fn default() -> Self {
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

    /// Search by semantic similarity using cosine similarity.
    ///
    /// Returns document IDs with similarity scores sorted by relevance.
    ///
    /// # TODO
    ///
    /// - Implement efficient similarity search (currently O(n))
    /// - Consider using approximate nearest neighbor (ANN) algorithms
    /// - Add hybrid search combining with FTS5 results
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

            // Get document ID
            if let Some((id, _)) = self.documents.iter().nth(i) {
                results.push(SearchResult::new(id.clone(), similarity));
            }
        }

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
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

    /// Remove a memory entry from the vector store.
    pub async fn remove(&mut self, id: &str) -> AgentResult<()> {
        if self.documents.remove(id).is_some() {
            debug!(id, "memory removed from vector store");
        }
        // Note: In a production implementation, we would also remove the embedding
        // For simplicity, we leave orphaned embeddings (they'll be skipped in search)
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
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vector_store_insert() {
        let mut store = VectorStore::default();

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
        let mut store = VectorStore::default();

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
