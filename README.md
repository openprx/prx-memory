# prx-memory

`prx-memory` is a local-first MCP memory component for coding agents.
It is designed to work across Codex, Claude Code, OpenClaw, OpenPRX, and other MCP-compatible clients without requiring a centralized memory service.
Maintainer: `Andy.z`

## What It Provides

- Local MCP server (`stdio` and `HTTP` transport).
- Full memory toolchain: store, recall, update, forget, export/import, migrate, reembed, compact.
- Governance controls: structured memory format, tag normalization, ratio bounds, periodic maintenance.
- Evolution support: `memory_evolve` with train+holdout acceptance and constraint gating.
- Skill distribution through MCP resources and skill manifest tools.

## Core Concept

`prx-memory` focuses on **reusable engineering knowledge**, not raw logs.
The system combines:

1. Governance layer: quality and safety constraints.
2. Retrieval layer: lexical/vector recall and optional rerank.
3. Evolution layer: measurable candidate selection with holdout safeguards.

## Quick Start

### Build

```bash
cargo build -p prx-memory-mcp --bin prx-memoryd
```

### Run (stdio)

```bash
PRX_MEMORYD_TRANSPORT=stdio \
PRX_MEMORY_DB=./data/memory-db.json \
./target/debug/prx-memoryd
```

### Run (http)

```bash
PRX_MEMORYD_TRANSPORT=http \
PRX_MEMORY_HTTP_ADDR=127.0.0.1:8787 \
PRX_MEMORY_DB=./data/memory-db.json \
./target/debug/prx-memoryd
```

## MCP Client Configuration Example

```json
{
  "mcpServers": {
    "prx_memory": {
      "command": "/opt/worker/code/prx-memory/target/release/prx-memoryd",
      "env": {
        "PRX_MEMORYD_TRANSPORT": "stdio",
        "PRX_MEMORY_BACKEND": "json",
        "PRX_MEMORY_DB": "/opt/worker/code/prx-memory/data/memory-db.json"
      }
    }
  }
}
```

## Skills and Templates

- Governance skill package: `skills/prx-memory-governance/SKILL.md`
- Client discovery path:
  - `resources/list`
  - `resources/read`
  - `tools/call` -> `memory_skill_manifest`
- Payload templates:
  - `resources/templates/list`
  - `resources/read` with `prx://templates/...`

## Standardization Profile

- `PRX_MEMORY_STANDARD_PROFILE=zero-config|governed`
- `PRX_MEMORY_DEFAULT_PROJECT_TAG` (default: `prx-memory`)
- `PRX_MEMORY_DEFAULT_TOOL_TAG` (default: `mcp`)
- `PRX_MEMORY_DEFAULT_DOMAIN_TAG` (default: `general`)

## Documentation Map

See [docs/README.md](docs/README.md) for a categorized documentation index.

## Evolution Papers

- Chinese paper: `PRX_MEMORY_EVOLUTION_PAPER_CN.md`
- English paper: `PRX_MEMORY_EVOLUTION_PAPER_EN.md`

## Development and Regression

```bash
cargo fmt
cargo check --all-targets --all-features
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
./scripts/run_multi_client_validation.sh
```
