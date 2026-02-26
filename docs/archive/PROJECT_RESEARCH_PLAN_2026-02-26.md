# prx-memory Research and Production Plan (V1)

## 1. Goal and Scope
- Goal: build a Rust local-first MCP memory component inspired by `memory-lancedb-pro`, usable by Codex, Claude Code, OpenClaw, and OpenPRX over stdio/HTTP.
- Non-goals:
  - not a raw conversation log store
  - not tied to a single embedding or rerank vendor

## 2. Reference Architecture Decomposition
The reference implementation can be decomposed into six layers:
1. `store`: vector/FTS/BM25 storage and dedup
2. `embedder`: OpenAI-compatible embedding abstraction + cache
3. `retriever`: hybrid retrieval + rerank + scoring pipeline
4. `scopes`: multi-scope isolation
5. `tools`: MCP tool surface (`memory_recall/store/forget`)
6. `governance`: noise filter, dual-layer memory, ratio controls

## 3. Generic MCP Architecture
### 3.1 Transport and Protocol
- Primary transport: `stdio`
- Secondary transport: streamable HTTP
- Maintain compatibility for recent MCP revisions.

### 3.2 Module Layout
- `crates/prx-memory-core`: domain rules and scoring
- `crates/prx-memory-storage`: storage adapters and schema evolution
- `crates/prx-memory-embed`: embedding provider trait + adapters
- `crates/prx-memory-rerank`: rerank provider trait + adapters
- `crates/prx-memory-mcp`: MCP server tools/resources/prompts
- `crates/prx-memory-skill`: governance skill generation and validation
- `bin/prx-memoryd`: runtime entry point

### 3.3 Initial MCP Tools
- `memory_recall`
- `memory_store`
- `memory_forget`
- `memory_stats`
- `memory_list`
- `memory_compact`

## 4. Third-Party Vector Strategy
### 4.1 Embeddings
- Interface: `EmbeddingProvider::embed_query/embed_passage/embed_batch`
- Support OpenAI-compatible, Jina, Gemini, and compatible gateways.
- Production requirements: batching, retry/backoff, rate limiting, dimension checks, LRU+TTL cache.

### 4.2 Rerank
- Optional adapters for Jina/Cohere/Pinecone-compatible services.
- Graceful fallback when rerank is unavailable.

### 4.3 Scoring Pipeline
- vector + BM25 retrieval
- fusion + rerank + recency/importance/length/time-decay + hard threshold
- stage-level observability for diagnostics

## 5. Skill Production Plan
- Include installable governance skill package with:
  - recall-before-store
  - dual-layer memory format
  - dedup/update policy
  - decision ratio cap
  - periodic compaction rules

## 6. Production Engineering Baseline
- Configuration: default config + env overrides
- Security: secrets in env/secret manager only
- Observability: structured logs and metrics
- Reliability: graceful shutdown, migration rollback, recovery path
- Testing: unit/integration/contract/performance tests

## 7. Milestones
- M1: MCP stdio + core tools + baseline storage
- M2: hybrid retrieval, rerank, scope isolation, governance enforcement
- M3: skill package, compaction, observability hardening
- M4: multi-client integration and release

## 8. Go-Live Criteria
- functional completeness for core tools
- stable performance under 100k entries
- 24h reliability run without critical failures
- at least two MCP clients validated
- complete install/config/troubleshooting documentation

## 9. Alignment Status (as of 2026-02-26)
### 9.1 Completed
- Local MCP stdio server is implemented and test-covered.
- Secondary HTTP transport baseline is implemented:
  - `POST /mcp` for JSON-RPC over HTTP
  - `GET /health` for liveness check
- Runtime entrypoint `prx-memoryd` is available with `PRX_MEMORYD_TRANSPORT=stdio|http`.
- Stream/session semantics baseline is implemented for HTTP:
  - `POST /mcp/session/start`
  - `POST /mcp/stream?session=...` (enqueue framed RPC response)
  - `GET /mcp/stream?session=...&from=...&limit=...` (ordered event polling)
- Tool surface has exceeded initial scope:
  - `memory_recall/store/store_dual/forget/stats/list/update`
  - `memory_export/import/migrate/reembed/compact`
  - `memory_evolve` and `memory_skill_manifest`
- Storage layer:
  - JSON backend available by default
  - LanceDB backend available via feature flag
  - lexical + vector fusion retrieval with recency/importance/length weighting
- Governance baseline is implemented:
  - dedup before store
  - critical post-store recall verification
  - decision ratio cap (<=30%)
  - periodic compaction every 100 writes
  - governed mode now enforces dual-layer workflow (`memory_store_dual`)
- Scope isolation and ACL are implemented:
  - `global / agent:* / custom:* / project:* / user:*`
  - wildcard and per-agent access rules
- Skill distribution path is implemented:
  - MCP `resources/list` + `resources/read`
  - governance skill/readme can be fetched by client
- Observability baseline is implemented:
  - `GET /metrics` in Prometheus text format
  - tool-level call/error/latency counters
  - recall stage latency metrics (`local/remote/total`)
- Tag policy is unified:
  - canonical prefixed tags (`project:*`, `tool:*`, `domain:*`)
  - structured inputs (`project_tag/tool_tag/domain_tag`) are normalized
- Test and quality evidence:
  - `cargo test --all-targets --all-features` passed
  - 100k performance test passed (`p95=201.573ms`, threshold `< 300ms`)
- Embedding runtime hardening baseline is implemented:
  - built-in LRU+TTL embedding cache
  - built-in token-bucket rate limiting
- Planned crate split baseline is completed:
  - independent crates added: `prx-memory-core / prx-memory-embed / prx-memory-rerank / prx-memory-skill`
  - `prx-memory-mcp` migrated to depend on split crates directly
  - `prx-memory-ai` retained as compatibility re-export layer
- Streamable HTTP advanced baseline is completed:
  - SSE mode is available on `GET /mcp/stream?mode=sse` with heartbeat and wait window
  - session lease/expiry policy is enforced (`PRX_MEMORY_STREAM_SESSION_TTL_MS`)
  - resumable ack protocol is available via `ack` cursor and `next_from/effective_from`
  - lease renewal endpoint is available: `POST /mcp/session/renew?session=...`
- Rerank provider coverage baseline is completed:
  - implemented: `jina`, `cohere`, `pinecone-compatible` (plus `none`)
  - environment-driven selection remains via `PRX_RERANK_PROVIDER`
- Observability hardening baseline is completed:
  - metrics cardinality controls for recall dimensions with overflow counters
  - `GET /metrics/summary` JSON summary endpoint for dashboard/health probes
  - built-in alert signals (`tool_error_ratio`, `remote_warning_ratio`, `metrics_label_overflow`)
  - dashboard and alert rule templates published under `docs/engineering/`

### 9.2 Pending
- Go-live validation gaps:
  - 24h reliability soak evidence not completed (soak harness + short-run evidence ready)

### 9.3 Milestone Snapshot
- M1: Completed
- M2: Completed (core target)
- M3: Completed
- M4: Not completed
