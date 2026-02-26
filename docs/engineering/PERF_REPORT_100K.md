# 100k Recall Performance Report

## Scope
- Component: `prx-memory-storage`
- Scenario: lexical+importance+recency recall on 100,000 entries
- Tool: `cargo test -p prx-memory-storage --test perf_100k -- --ignored --nocapture`

## Latest Result
- Date: 2026-02-26
- Status: pass
- p95(ms): 122.683
- Threshold: < 300ms

## Notes
- This benchmark isolates retrieval ranking path.
- It does not include network embedding/rerank calls.
- For release, pair with end-to-end MCP latency benchmarks.
