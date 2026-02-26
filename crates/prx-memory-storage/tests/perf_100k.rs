use std::time::Instant;

use prx_memory_storage::{recall_entries, MemoryEntry, RecallQuery};

fn make_entries(n: usize) -> Vec<MemoryEntry> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let provider = if i % 3 == 0 {
            "jina"
        } else if i % 3 == 1 {
            "gemini"
        } else {
            "openai"
        };
        out.push(MemoryEntry {
            id: format!("mem-{i}"),
            text: format!(
                "memory {i}: use {provider} embeddings for retrieval query ranking and governance"
            ),
            category: "fact".to_string(),
            scope: if i % 2 == 0 {
                "global".to_string()
            } else {
                "project:alpha".to_string()
            },
            importance: ((i % 10) as f32) / 10.0,
            tags: vec![
                provider.to_string(),
                "retrieval".to_string(),
                "mcp".to_string(),
            ],
            timestamp_ms: 1_700_000_000_000 + (i as u64 * 1000),
            embedding: None,
        });
    }
    out
}

fn percentile(sorted_ms: &[f64], p: f64) -> f64 {
    let idx = ((sorted_ms.len().saturating_sub(1)) as f64 * p).round() as usize;
    sorted_ms[idx]
}

#[test]
#[ignore]
fn recall_p95_under_threshold_on_100k() {
    let entries = make_entries(100_000);

    let mut samples_ms = Vec::new();
    for i in 0..120 {
        let q = if i % 2 == 0 {
            "jina retrieval query"
        } else {
            "gemini governance ranking"
        };

        let started = Instant::now();
        let _ = recall_entries(
            &entries,
            RecallQuery {
                query: q.to_string(),
                query_embedding: None,
                scope: None,
                category: None,
                limit: 8,
                vector_weight: None,
                lexical_weight: None,
            },
        );
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        samples_ms.push(elapsed);
    }

    samples_ms.sort_by(|a, b| a.total_cmp(b));
    let p95 = percentile(&samples_ms, 0.95);
    eprintln!("recall p95(ms) on 100k entries: {:.3}", p95);

    assert!(p95 < 300.0, "p95 too high: {:.3}ms", p95);
}
