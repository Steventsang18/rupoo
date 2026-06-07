//! Simple performance benchmark for vector search.
//!
//! This benchmark measures the performance of HNSW-based vector search.

use rand::Rng;
use rupoo::vector_store::{VectorMemoryDoc, VectorStore, VectorStoreConfig};
use std::sync::Arc;
use std::time::Instant;

fn generate_random_embedding(dim: usize) -> Vec<f32> {
    let mut rng = rand::thread_rng();
    (0..dim).map(|_| rng.gen_range(-1.0..1.0)).collect()
}

fn create_documents(count: usize) -> Vec<VectorMemoryDoc> {
    (0..count)
        .map(|i| VectorMemoryDoc {
            id: format!("doc-{}", i),
            content: format!("Test document {}", i),
            tags: vec!["test".to_string()],
            source: "benchmark".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
        })
        .collect()
}

fn run_insert_benchmark(dim: usize, count: usize) -> std::time::Duration {
    let store = Arc::new(VectorStore::new(dim, VectorStoreConfig::default()));
    let docs = create_documents(count);
    let embeddings: Vec<_> = (0..count).map(|_| generate_random_embedding(dim)).collect();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let start = Instant::now();

    rt.block_on(async {
        for (doc, embedding) in docs.into_iter().zip(embeddings.into_iter()) {
            store.insert(doc, embedding).await.unwrap();
        }
    });

    start.elapsed()
}

fn run_search_benchmark(dim: usize, count: usize, searches: usize) -> std::time::Duration {
    let store = Arc::new(VectorStore::new(
        dim,
        VectorStoreConfig {
            max_elements: count * 2,
            ..Default::default()
        },
    ));

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        for i in 0..count {
            let doc = VectorMemoryDoc {
                id: format!("doc-{}", i),
                content: format!("Test document {}", i),
                tags: vec!["test".to_string()],
                source: "benchmark".to_string(),
                created_at: "2025-01-01T00:00:00Z".to_string(),
            };
            let embedding = generate_random_embedding(dim);
            store.insert(doc, embedding).await.unwrap();
        }
    });

    let start = Instant::now();

    rt.block_on(async {
        for _ in 0..searches {
            let query = generate_random_embedding(dim);
            store.semantic_search(query, 10).await.unwrap();
        }
    });

    start.elapsed()
}

fn main() {
    println!("=== Vector Search Performance Benchmark ===\n");

    // Insert benchmarks
    println!("Insert Performance (384 dimensions):");
    let time_100 = run_insert_benchmark(384, 100);
    println!("  100 docs:  {:?}", time_100);

    let time_1000 = run_insert_benchmark(384, 1000);
    println!("  1000 docs: {:?}", time_1000);

    let time_10000 = run_insert_benchmark(384, 10000);
    println!("  10000 docs: {:?}", time_10000);

    println!("\nSearch Performance (384 dimensions):");
    let search_100 = run_search_benchmark(384, 100, 100);
    println!("  100 docs, 100 searches: {:?}", search_100);
    println!("  Average per search: {:?}", search_100 / 100);

    let search_1000 = run_search_benchmark(384, 1000, 100);
    println!("  1000 docs, 100 searches: {:?}", search_1000);
    println!("  Average per search: {:?}", search_1000 / 100);

    println!("\nSearch Performance with different dimensions:");
    let search_768 = run_search_benchmark(768, 500, 100);
    println!("  768 dim, 500 docs, 100 searches: {:?}", search_768);
    println!("  Average per search: {:?}", search_768 / 100);

    println!("\n=== Benchmark Complete ===");
}
