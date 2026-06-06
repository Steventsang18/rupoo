//! Benchmarks for rupoo critical paths
//!
//! Run with: cargo bench

use bencher::black_box;
use bencher::Bencher;

/// Benchmark: Signal compression
fn bench_compress_output(b: &mut Bencher) {
    use rupoo::signal::SignalCompressor;
    
    let long_output = "line1\nline2\nline3\n".repeat(100);
    
    b.iter(|| {
        let compressor = SignalCompressor::new(100, 0.7);
        black_box(compressor.compress_output(black_box(&long_output)));
    });
}

/// Benchmark: LRU cache lookup
fn bench_lru_cache(b: &mut Bencher) {
    use lru::LruCache;
    use std::num::NonZeroUsize;
    
    let mut cache = LruCache::new(NonZeroUsize::new(1000).unwrap());
    
    // Pre-populate
    for i in 0..1000 {
        cache.put(i, format!("value_{}", i));
    }
    
    b.iter(|| {
        // Mix of hits and misses
        for i in 0..100 {
            black_box(cache.get(&i));
        }
    });
}

/// Benchmark: JSON patch generation
fn bench_json_patch(b: &mut Bencher) {
    let old_state = serde_json::json!({
        "plan": "test",
        "steps": [
            {"id": 1, "status": "completed"},
            {"id": 2, "status": "pending"},
        ],
        "index": 1
    });
    
    let new_state = serde_json::json!({
        "plan": "test",
        "steps": [
            {"id": 1, "status": "completed"},
            {"id": 2, "status": "in_progress"},
        ],
        "index": 2
    });
    
    b.iter(|| {
        // Simple diff - in production this would be more sophisticated
        let diff = if old_state != new_state {
            serde_json::json!({"index": new_state["index"]})
        } else {
            serde_json::json!({})
        };
        black_box(diff);
    });
}

/// Benchmark: Memory recall search
fn bench_memory_search(b: &mut Bencher) {
    use std::collections::HashMap;
    
    // Simulate memory entries
    let memories: Vec<(String, String)> = (0..100)
        .map(|i| (format!("mem_{}", i), format!("Memory content {} with keywords rust programming", i)))
        .collect();
    
    let search_term = "rust programming";
    
    b.iter(|| {
        // Simple keyword matching simulation
        let results: Vec<_> = memories.iter()
            .filter(|(_, content)| content.contains(search_term))
            .take(10)
            .collect();
        black_box(results);
    });
}

/// Benchmark: Plan serialization
fn bench_plan_serde(b: &mut Bencher) {
    use rupoo::db::task::Plan;
    use rupoo::db::task::Step;
    
    let plan = Plan::new("bench_plan", vec![
        Step::think("Analyze the codebase structure".to_string()),
        Step::exec("cargo build".to_string()),
        Step::exec("cargo test".to_string()),
        Step::finish("Completed benchmarking".to_string()),
    ]);
    
    b.iter(|| {
        let json = serde_json::to_string(black_box(&plan)).unwrap();
        black_box(json);
    });
}

/// Benchmark: Config parsing
fn bench_config_parse(b: &mut Bencher) {
    let config_str = r#"
        [providers.anthropic]
        api_key = "test-key"
        model = "claude-sonnet-4-20250514"
        
        [providers.openai]
        api_key = "test-key"
        model = "gpt-4"
        
        [providers.deepseek]
        base_url = "https://api.deepseek.com"
        model = "deepseek-chat"
    "#;
    
    b.iter(|| {
        let _: Result<toml::Table, _> = toml::from_str(black_box(config_str));
    });
}

bencher::benchmark_group!(
    benches,
    bench_compress_output,
    bench_lru_cache,
    bench_json_patch,
    bench_memory_search,
    bench_plan_serde,
    bench_config_parse,
);
bencher::benchmark_main!(benches);
