//! Vector store for semantic memory search with HNSW indexing.
//!
//! This module provides efficient vector storage and approximate nearest neighbor
//! search using the Hierarchical Navigable Small World (HNSW) algorithm.
//!
//! # Features
//!
//! - **Fast Semantic Search**: O(log n) search complexity using HNSW graph
//! - **Thread-Safe Operations**: Uses `RwLock` for concurrent access
//! - **Euclidean Distance**: Fast similarity calculation
//! - **Scalable**: Supports up to 10,000+ vectors by default
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
//!     +---> Vector Search (semantic) using HNSW
//!               |
//!               +---> Similar meanings, intent understanding (O(log n))
//!
//! Combined Results (relevance ranking)
//! ```
//!
//! # Usage Example
//!
//! ```rust
//! use rupoo::vector_store::{VectorStore, VectorMemoryDoc, VectorStoreConfig};
//!
//! // Create a vector store with default configuration (384 dimensions)
//! let store = VectorStore::with_default_dim();
//!
//! // Create a document
//! let doc = VectorMemoryDoc {
//!     id: "doc-1".to_string(),
//!     content: "Rust programming language".to_string(),
//!     tags: vec!["rust".to_string(), "programming".to_string()],
//!     source: "user".to_string(),
//!     created_at: "2025-01-01T00:00:00Z".to_string(),
//! };
//!
//! // Insert with embedding (384-dimensional)
//! let embedding: Vec<f32> = vec![0.5; 384];
//! store.insert(doc, embedding).await.unwrap();
//!
//! // Search for similar documents
//! let query_embedding: Vec<f32> = vec![0.6; 384];
//! let results = store.semantic_search(query_embedding, 5).await.unwrap();
//!
//! for result in results {
//!     println!("Document ID: {}, Similarity: {}", result.id, result.score);
//! }
//! ```
//!
//! # HNSW Configuration Parameters
//!
//! | Parameter | Description | Default | Range |
//! |-----------|-------------|---------|-------|
//! | `m` | Maximum number of connections per node | 32 | 4-64 |
//! | `ef_construction` | Size of dynamic candidate list during construction | 32 | 10-200 |
//! | `ef_search` | Size of dynamic candidate list during search | 100 | 10-500 |
//! | `max_elements` | Maximum number of vectors in index | 10000 | 100-1,000,000 |
//!
//! # Performance Characteristics
//!
//! - **Insertion**: O(log n) with HNSW construction overhead
//! - **Search**: O(log n) approximate nearest neighbor search
//! - **Memory**: ~4 * dim * n bytes for vectors + graph overhead
//!
//! # Implementation Status
//!
//! - ✅ VectorStore struct with HNSW indexing
//! - ✅ Semantic search using Euclidean distance
//! - ✅ Thread-safe operations
//! - ✅ O(log n) search performance with HNSW
//! - ✅ Hybrid search ready

use std::sync::RwLock;

use hnswx::{EuclideanDistance, HNSW};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::AgentResult;
use crate::task::MemoryEntry;

/// Memory document for vector storage.
///
/// Represents a document stored in the vector store, containing both the
/// original content and metadata for semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorMemoryDoc {
    /// Unique identifier for the document
    pub id: String,
    /// The actual content of the document
    pub content: String,
    /// Tags/categories associated with the document
    pub tags: Vec<String>,
    /// Source/origin of the document (e.g., "user", "agent", "file")
    pub source: String,
    /// Timestamp when the document was created (RFC3339 format)
    pub created_at: String,
}

impl VectorMemoryDoc {
    /// Create a VectorMemoryDoc from a MemoryEntry.
    ///
    /// # Arguments
    ///
    /// * `entry` - The MemoryEntry to convert from
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::task::MemoryEntry;
    /// use rupoo::vector_store::VectorMemoryDoc;
    ///
    /// let entry = MemoryEntry {
    ///     id: "mem-1".to_string(),
    ///     content: "Hello world".to_string(),
    ///     tags: vec!["greeting".to_string()],
    ///     source: "user".to_string(),
    ///     created_at: "2025-01-01T00:00:00Z".to_string(),
    /// };
    ///
    /// let doc = VectorMemoryDoc::from_memory_entry(&entry);
    /// assert_eq!(doc.id, "mem-1");
    /// ```
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

/// Search result containing document ID and relevance score.
///
/// Represents a single result from a semantic search, with the document ID
/// and a similarity score ranging from 0.0 (no similarity) to 1.0 (identical).
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Unique identifier of the matching document
    pub id: String,
    /// Similarity score (0.0 to 1.0), higher means more relevant
    pub score: f32,
}

impl SearchResult {
    /// Create a new SearchResult.
    ///
    /// # Arguments
    ///
    /// * `id` - Document identifier
    /// * `score` - Similarity score (0.0 to 1.0)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::SearchResult;
    ///
    /// let result = SearchResult::new("doc-1".to_string(), 0.95);
    /// assert_eq!(result.id, "doc-1");
    /// assert!((result.score - 0.95).abs() < 0.001);
    /// ```
    pub fn new(id: String, score: f32) -> Self {
        Self { id, score }
    }
}

/// Configuration parameters for the HNSW vector store.
///
/// These parameters control the behavior and performance of the HNSW index.
/// Adjusting these can significantly impact search speed and recall quality.
#[derive(Debug, Clone)]
pub struct VectorStoreConfig {
    /// Maximum number of connections per node in the HNSW graph.
    ///
    /// Higher values increase recall but use more memory and slow down
    /// both insertion and search. Typical range: 4-64.
    pub m: usize,
    /// Size of the dynamic candidate list during index construction.
    ///
    /// Higher values improve index quality but slow down insertion.
    /// Typical range: 10-200.
    pub ef_construction: usize,
    /// Size of the dynamic candidate list during search.
    ///
    /// Higher values improve recall but slow down search.
    /// Typical range: 10-500.
    pub ef_search: usize,
    /// Maximum number of elements that can be stored in the index.
    ///
    /// The index cannot grow beyond this limit.
    pub max_elements: usize,
}

impl Default for VectorStoreConfig {
    /// Returns the default configuration for VectorStore.
    ///
    /// Default values are tuned for a balance between performance and recall:
    /// - m: 32
    /// - ef_construction: 32
    /// - ef_search: 100
    /// - max_elements: 10000
    fn default() -> Self {
        Self {
            m: 32,
            ef_construction: 32,
            ef_search: 100,
            max_elements: 10000,
        }
    }
}

/// Optimized vector store with HNSW indexing for O(log n) semantic search.
///
/// This implementation provides:
/// - **Fast Semantic Search**: Uses HNSW (Hierarchical Navigable Small World)
///   graph for approximate nearest neighbor search with O(log n) complexity
/// - **Separate Document Store**: Maintains document metadata separately from
///   the vector index to avoid embedding fragmentation
/// - **Euclidean Distance**: Uses Euclidean distance metric for similarity
///   calculation
/// - **Thread-Safe Operations**: Uses `RwLock` for concurrent read/write access
///
/// # Thread Safety
///
/// All operations are thread-safe. Multiple readers can access the store
/// concurrently, and writers are serialized.
pub struct VectorStore {
    /// Store documents by ID
    documents: RwLock<std::collections::HashMap<String, VectorMemoryDoc>>,
    /// HNSW index for fast approximate nearest neighbor search
    hnsw: RwLock<HNSW<EuclideanDistance>>,
    /// Mapping from HNSW node ID to document ID
    node_id_to_doc_id: RwLock<std::collections::HashMap<usize, String>>,
    /// Embedding dimension (e.g., 384, 768, 1536)
    embedding_dim: usize,
}

impl VectorStore {
    /// Create a new vector store with specified embedding dimension and HNSW config.
    ///
    /// # Arguments
    ///
    /// * `embedding_dim` - Dimension of the embedding vectors (e.g., 384, 768, 1536)
    /// * `config` - HNSW configuration parameters
    ///
    /// # Common Embedding Dimensions
    ///
    /// | Model | Dimension | Description |
    /// |-------|-----------|-------------|
    /// | all-MiniLM-L6-v2 | 384 | Lightweight, fast |
    /// | text-embedding-ada-002 | 1536 | Standard OpenAI model |
    /// | text-embedding-3-small | 1536 | OpenAI small model |
    /// | text-embedding-3-large | 3072 | OpenAI large model |
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::{VectorStore, VectorStoreConfig};
    ///
    /// let config = VectorStoreConfig {
    ///     m: 16,
    ///     ef_construction: 64,
    ///     ef_search: 200,
    ///     max_elements: 1000,
    /// };
    ///
    /// let store = VectorStore::new(384, config);
    /// assert_eq!(store.embedding_dim(), 384);
    /// ```
    pub fn new(embedding_dim: usize, config: VectorStoreConfig) -> Self {
        let hnsw_config = hnswx::HnswConfig {
            max_elements: config.max_elements,
            m: config.m,
            m_max: config.m * 2,
            m_max_0: config.m * 4,
            ef_construction: config.ef_construction,
            ef_search: config.ef_search,
            level_multiplier: 1.0 / (config.m as f64).ln(),
            allow_replace_deleted: true,
            num_threads: 0,
            batch_size: 64,
        };

        let hnsw = HNSW::new(hnsw_config, EuclideanDistance::new());

        Self {
            documents: RwLock::new(std::collections::HashMap::new()),
            hnsw: RwLock::new(hnsw),
            node_id_to_doc_id: RwLock::new(std::collections::HashMap::new()),
            embedding_dim,
        }
    }

    /// Create a new vector store with default HNSW configuration.
    ///
    /// # Arguments
    ///
    /// * `embedding_dim` - Dimension of the embedding vectors
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::VectorStore;
    ///
    /// let store = VectorStore::with_default_config(768);
    /// assert_eq!(store.embedding_dim(), 768);
    /// ```
    pub fn with_default_config(embedding_dim: usize) -> Self {
        Self::new(embedding_dim, VectorStoreConfig::default())
    }

    /// Create a vector store with default configuration and 384 dimensions.
    ///
    /// This is the most lightweight option, suitable for lightweight embedding
    /// models like all-MiniLM-L6-v2.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::VectorStore;
    ///
    /// let store = VectorStore::with_default_dim();
    /// assert_eq!(store.embedding_dim(), 384);
    /// assert!(store.is_empty());
    /// ```
    pub fn with_default_dim() -> Self {
        Self::with_default_config(384)
    }

    /// Insert a memory entry into the vector store with its embedding.
    ///
    /// # Arguments
    ///
    /// * `doc` - The document to store
    /// * `embedding` - The vector embedding of the document content
    ///
    /// # Errors
    ///
    /// Returns `AgentError` if there is an issue during insertion.
    /// Note: If the embedding dimension doesn't match, the document is silently
    /// skipped (logged as warning).
    ///
    /// # Performance
    ///
    /// Insertion time: O(log n) with HNSW construction overhead
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::{VectorStore, VectorMemoryDoc};
    ///
    /// let store = VectorStore::with_default_dim();
    ///
    /// let doc = VectorMemoryDoc {
    ///     id: "doc-1".to_string(),
    ///     content: "Hello world".to_string(),
    ///     tags: vec![],
    ///     source: "user".to_string(),
    ///     created_at: "2025-01-01T00:00:00Z".to_string(),
    /// };
    ///
    /// let embedding: Vec<f32> = vec![0.5; 384];
    /// store.insert(doc, embedding).await.unwrap();
    ///
    /// assert_eq!(store.len(), 1);
    /// ```
    pub async fn insert(&self, doc: VectorMemoryDoc, embedding: Vec<f32>) -> AgentResult<()> {
        if embedding.len() != self.embedding_dim {
            tracing::warn!(
                expected = self.embedding_dim,
                actual = embedding.len(),
                "embedding dimension mismatch, skipping"
            );
            return Ok(());
        }

        let id = doc.id.clone();

        // Insert into HNSW index and get the actual node ID
        let node_id = {
            let mut hnsw = self.hnsw.write().unwrap();
            hnsw.insert(embedding)
        };

        // Update mappings
        self.documents.write().unwrap().insert(id.clone(), doc);
        self.node_id_to_doc_id
            .write()
            .unwrap()
            .insert(node_id, id.clone());

        debug!(id, node_id, "memory inserted into vector store with HNSW");
        Ok(())
    }

    /// Search for documents by semantic similarity using HNSW for O(log n) performance.
    ///
    /// Performs an approximate nearest neighbor search using the HNSW graph
    /// to find documents semantically similar to the query embedding.
    ///
    /// # Arguments
    ///
    /// * `query_embedding` - The embedding vector of the query
    /// * `limit` - Maximum number of results to return
    ///
    /// # Errors
    ///
    /// Returns `AgentError` if the embedding dimension doesn't match.
    ///
    /// # Performance
    ///
    /// Search time: O(log n) with HNSW
    ///
    /// # Similarity Calculation
    ///
    /// Similarity is calculated from Euclidean distance:
    /// `similarity = 1.0 / (1.0 + distance)`
    ///
    /// This maps:
    /// - Distance 0 → Similarity 1.0 (identical)
    /// - Larger distances → Lower similarities
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::{VectorStore, VectorMemoryDoc};
    ///
    /// let store = VectorStore::with_default_dim();
    ///
    /// let doc = VectorMemoryDoc {
    ///     id: "doc-1".to_string(),
    ///     content: "Machine learning with Python".to_string(),
    ///     tags: vec!["ml".to_string()],
    ///     source: "user".to_string(),
    ///     created_at: "2025-01-01T00:00:00Z".to_string(),
    /// };
    ///
    /// let embedding: Vec<f32> = vec![0.9; 384];
    /// store.insert(doc, embedding).await.unwrap();
    ///
    /// let query: Vec<f32> = vec![0.85; 384];
    /// let results = store.semantic_search(query, 5).await.unwrap();
    ///
    /// assert!(!results.is_empty());
    /// assert_eq!(results[0].id, "doc-1");
    /// assert!(results[0].score > 0.5);
    /// ```
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

        let hnsw = self.hnsw.read().unwrap();
        let node_id_to_doc_id = self.node_id_to_doc_id.read().unwrap();

        // Perform HNSW search
        let results = hnsw.search_knn(&query_embedding, limit);

        // Convert HNSW results to SearchResults
        let mut search_results: Vec<SearchResult> = results
            .into_iter()
            .filter_map(|hnsw_result| {
                // Euclidean distance to similarity: similarity = 1.0 / (1.0 + distance)
                // This maps distance 0 -> similarity 1.0, and larger distances -> lower similarities
                let similarity = 1.0 / (1.0 + hnsw_result.distance);
                node_id_to_doc_id
                    .get(&hnsw_result.id)
                    .map(|doc_id| SearchResult::new(doc_id.clone(), similarity))
            })
            .collect();

        // Sort by score descending (though HNSW already returns in order)
        search_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        debug!(
            count = search_results.len(),
            "semantic search completed with HNSW"
        );
        Ok(search_results)
    }

    /// Remove a memory entry from the vector store.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the document to remove
    ///
    /// # Limitations
    ///
    /// HNSW index does not support efficient deletion. The node is not actually
    /// removed from the graph; only the mapping is removed. This means the HNSW
    /// graph will continue to grow even after deletions. For large-scale applications
    /// with many deletions, consider rebuilding the index periodically.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::{VectorStore, VectorMemoryDoc};
    ///
    /// let store = VectorStore::with_default_dim();
    ///
    /// let doc = VectorMemoryDoc {
    ///     id: "doc-1".to_string(),
    ///     content: "Test".to_string(),
    ///     tags: vec![],
    ///     source: "user".to_string(),
    ///     created_at: "2025-01-01T00:00:00Z".to_string(),
    /// };
    ///
    /// let embedding: Vec<f32> = vec![0.5; 384];
    /// store.insert(doc, embedding).await.unwrap();
    ///
    /// assert_eq!(store.len(), 1);
    ///
    /// store.remove("doc-1").await.unwrap();
    /// assert_eq!(store.len(), 0);
    /// ```
    pub async fn remove(&self, id: &str) -> AgentResult<()> {
        if let Some(_doc) = self.documents.write().unwrap().remove(id) {
            // Find and mark corresponding HNSW node as deleted
            let mut node_id_to_doc_id = self.node_id_to_doc_id.write().unwrap();
            if let Some((&node_id, _)) = node_id_to_doc_id.iter().find(|(_, doc_id)| **doc_id == id)
            {
                node_id_to_doc_id.remove(&node_id);
                // Note: HNSW node is not actually removed from the graph
                // This is a limitation of HNSW; we just remove the mapping
            }
            debug!(id, "memory removed from vector store");
        }
        Ok(())
    }

    /// Get the number of documents in the vector store.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::VectorStore;
    ///
    /// let store = VectorStore::with_default_dim();
    /// assert_eq!(store.len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.documents.read().unwrap().len()
    }

    /// Check if the vector store is empty.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::VectorStore;
    ///
    /// let store = VectorStore::with_default_dim();
    /// assert!(store.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.documents.read().unwrap().is_empty()
    }

    /// Get the embedding dimension used by this vector store.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rupoo::vector_store::VectorStore;
    ///
    /// let store = VectorStore::new(768, Default::default());
    /// assert_eq!(store.embedding_dim(), 768);
    /// ```
    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
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
        let store = VectorStore::with_default_dim();

        let doc = VectorMemoryDoc {
            id: "test-1".to_string(),
            content: "Rust is a great programming language".to_string(),
            tags: vec!["rust".to_string()],
            source: "test".to_string(),
            created_at: "2025-01-01".to_string(),
        };

        let embedding: Vec<f32> = (0..384).map(|i| (i as f32) * 0.01).collect();

        store.insert(doc, embedding).await.unwrap();

        assert_eq!(store.len(), 1);
    }

    #[tokio::test]
    async fn test_vector_store_search_with_hnsw() {
        let store = VectorStore::with_default_dim();

        let doc = VectorMemoryDoc {
            id: "doc-1".to_string(),
            content: "Rust programming".to_string(),
            tags: vec!["rust".to_string()],
            source: "test".to_string(),
            created_at: "2025-01-01".to_string(),
        };

        let embedding: Vec<f32> = vec![1.0; 384];
        store.insert(doc, embedding).await.unwrap();

        let query: Vec<f32> = vec![1.0; 384];
        let results = store.semantic_search(query, 10).await.unwrap();

        assert!(!results.is_empty());
        assert_eq!(results[0].id, "doc-1");
        assert!(
            results[0].score > 0.9,
            "Expected high similarity score for identical vectors, got {}",
            results[0].score
        );
    }

    #[tokio::test]
    async fn test_vector_store_search_multiple_docs() {
        let store = VectorStore::with_default_dim();

        let doc1 = VectorMemoryDoc {
            id: "doc-1".to_string(),
            content: "Machine learning with Python".to_string(),
            tags: vec!["ml".to_string(), "python".to_string()],
            source: "test".to_string(),
            created_at: "2025-01-01".to_string(),
        };

        let doc2 = VectorMemoryDoc {
            id: "doc-2".to_string(),
            content: "Deep learning with PyTorch".to_string(),
            tags: vec!["ml".to_string(), "pytorch".to_string()],
            source: "test".to_string(),
            created_at: "2025-01-02".to_string(),
        };

        let doc3 = VectorMemoryDoc {
            id: "doc-3".to_string(),
            content: "Rust programming language".to_string(),
            tags: vec!["rust".to_string()],
            source: "test".to_string(),
            created_at: "2025-01-03".to_string(),
        };

        // Similar embeddings for ML docs
        let embedding1: Vec<f32> = vec![0.9; 384];
        let embedding2: Vec<f32> = vec![0.85; 384];
        let embedding3: Vec<f32> = vec![0.1; 384];

        store.insert(doc1, embedding1).await.unwrap();
        store.insert(doc2, embedding2).await.unwrap();
        store.insert(doc3, embedding3).await.unwrap();

        let query: Vec<f32> = vec![0.88; 384];
        let results = store.semantic_search(query, 3).await.unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].id, "doc-1");
        assert_eq!(results[1].id, "doc-2");
        assert_eq!(results[2].id, "doc-3");
    }

    #[tokio::test]
    async fn test_vector_store_dimension_mismatch() {
        let store = VectorStore::new(384, VectorStoreConfig::default());

        let doc = VectorMemoryDoc {
            id: "test-1".to_string(),
            content: "Test".to_string(),
            tags: vec![],
            source: "test".to_string(),
            created_at: "2025-01-01".to_string(),
        };

        let embedding: Vec<f32> = vec![0.0; 768];

        store.insert(doc, embedding).await.unwrap();

        assert_eq!(store.len(), 0);
    }

    #[tokio::test]
    async fn test_vector_store_remove() {
        let store = VectorStore::with_default_dim();

        let doc = VectorMemoryDoc {
            id: "doc-1".to_string(),
            content: "Test document".to_string(),
            tags: vec![],
            source: "test".to_string(),
            created_at: "2025-01-01".to_string(),
        };

        let embedding: Vec<f32> = vec![0.5; 384];
        store.insert(doc, embedding).await.unwrap();

        assert_eq!(store.len(), 1);

        store.remove("doc-1").await.unwrap();
        assert_eq!(store.len(), 0);
    }

    #[tokio::test]
    async fn test_vector_store_search_empty() {
        let store = VectorStore::with_default_dim();

        let query: Vec<f32> = vec![1.0; 384];
        let results = store.semantic_search(query, 10).await.unwrap();

        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_vector_store_custom_config() {
        let config = VectorStoreConfig {
            m: 16,
            ef_construction: 64,
            ef_search: 200,
            max_elements: 1000,
        };

        let store = VectorStore::new(768, config);

        assert_eq!(store.embedding_dim(), 768);
    }
}
