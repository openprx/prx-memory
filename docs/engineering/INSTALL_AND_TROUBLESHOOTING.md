# Install, Config, and Troubleshooting

## Prerequisites
- Rust stable toolchain (`rustup`, `cargo`)
- Linux/macOS shell with `curl`

## Build
```bash
cargo build -p prx-memory-mcp --bin prx-memoryd
```

## Run (stdio)
```bash
PRX_MEMORYD_TRANSPORT=stdio \
PRX_MEMORY_DB=./data/memory-db.json \
./target/debug/prx-memoryd
```

## Run (http)
```bash
PRX_MEMORYD_TRANSPORT=http \
PRX_MEMORY_HTTP_ADDR=127.0.0.1:8787 \
PRX_MEMORY_DB=./data/memory-db.json \
./target/debug/prx-memoryd
```

## Health and Metrics
```bash
curl -sS http://127.0.0.1:8787/health
curl -sS http://127.0.0.1:8787/metrics | head -n 40
curl -sS http://127.0.0.1:8787/metrics/summary
```

## Key Environment Variables
- Storage/runtime
  - `PRX_MEMORY_DB`
  - `PRX_MEMORYD_TRANSPORT=stdio|http`
  - `PRX_MEMORY_HTTP_ADDR`
- Embedding/rerank
  - `PRX_EMBED_PROVIDER`, `PRX_EMBED_API_KEY`, `PRX_EMBED_MODEL`, `PRX_EMBED_BASE_URL`
  - `PRX_RERANK_PROVIDER`, `PRX_RERANK_API_KEY`, `PRX_RERANK_MODEL`, `PRX_RERANK_ENDPOINT`
  - `COHERE_API_KEY`, `PINECONE_API_KEY`, `JINA_API_KEY` (provider-specific fallback)
- Stream/session
  - `PRX_MEMORY_STREAM_SESSION_TTL_MS`
- Standardization (zero-config -> long-term governance)
  - `PRX_MEMORY_STANDARD_PROFILE=zero-config|governed` (default: `zero-config`)
  - `PRX_MEMORY_DEFAULT_PROJECT_TAG` (default: `prx-memory`)
  - `PRX_MEMORY_DEFAULT_TOOL_TAG` (default: `mcp`)
  - `PRX_MEMORY_DEFAULT_DOMAIN_TAG` (default: `general`)
- Observability and alert thresholds
  - `PRX_METRICS_MAX_RECALL_SCOPE_LABELS`
  - `PRX_METRICS_MAX_RECALL_CATEGORY_LABELS`
  - `PRX_METRICS_MAX_RERANK_PROVIDER_LABELS`
  - `PRX_ALERT_TOOL_ERROR_RATIO_WARN`
  - `PRX_ALERT_TOOL_ERROR_RATIO_CRIT`
  - `PRX_ALERT_REMOTE_WARNING_RATIO_WARN`
  - `PRX_ALERT_REMOTE_WARNING_RATIO_CRIT`

## Common Issues
- `PRX_EMBED_API_KEY is not configured`
  - Cause: remote semantic recall requested without embedding key.
  - Fix: set `PRX_EMBED_API_KEY` (or disable remote path).
- `Unsupported rerank provider`
  - Cause: invalid `PRX_RERANK_PROVIDER`.
  - Fix: use `jina|cohere|pinecone|pinecone-compatible|none`.
- `session_expired`
  - Cause: stream session exceeded lease TTL.
  - Fix: call `POST /mcp/session/renew?session=...` or increase `PRX_MEMORY_STREAM_SESSION_TTL_MS`.
- Metrics cardinality overflow alert
  - Cause: too many distinct labels in recall dimensions.
  - Fix: increase `PRX_METRICS_MAX_*_LABELS` or normalize scope/category/provider inputs.

## Resource Templates (for standardized payload generation)
```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"resources/templates/list","params":{}}' | ./target/debug/prx-memoryd
printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"prx://templates/memory-store?text=Pitfall:+demo&scope=global"}}' | ./target/debug/prx-memoryd
```

## Validation Commands
```bash
./scripts/run_multi_client_validation.sh
./scripts/run_soak_http.sh 300 5
```

## 24h Reliability Soak (release gate)
```bash
./scripts/run_soak_http.sh 86400 2
```
Record output and attach to release checklist.
