# Global Change Log

> Repository-level change log (reverse chronological).

## 2026-02-26
- Completed comprehensive MCP regression execution batch:
  - `cargo fmt --all -- --check`
  - `cargo check --all-targets --all-features`
  - `cargo test --all-targets --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `./scripts/run_holdout_regression.sh`
  - `./scripts/run_multi_client_validation.sh`
  - `./scripts/run_soak_http.sh` (`duration_sec=120`, `total_fail=0`).
- Updated MCP maintainer metadata:
  - Added `authors = ["Andy.z"]` to `crates/prx-memory-mcp/Cargo.toml`.
  - Added `Maintainer: Andy.z` to root `README.md`.
- Updated skill documentation for direct client reuse of main functional-line regression:
  - `skills/prx-memory-governance/SKILL.md` now includes an ordered online regression flow:
    capability -> CRUD -> governed dual-layer -> maintenance -> evolve -> cleanup.
  - Expanded script references in skill docs:
    kept skill-local helper scripts and added repository-level regression scripts
    (`run_multi_client_validation.sh`, `run_holdout_regression.sh`, `run_soak_http.sh`, `run_perf_100k.sh`).
  - `skills/README.md` now exposes a one-pass regression shortcut sequence for MCP clients.
- Added task execution record:
  - `docs/task/MCP_PRX_MEMORY_REGRESSION_2026-02-26.md`.
- Expanded `AGENTS.md` with project-level overview and MCP surface quick reference (purpose, transport modes, tool groups, resources/templates, and doc entry points).
- Standardized open-source documentation layout:
  - Added root `README.md` (English-first).
  - Added `docs/README.md` (documentation index).
  - Added `docs/ROADMAP.md` (public roadmap).
  - Added `skills/README.md` (skill discovery and template usage).
- Added bilingual evolution papers at repository root:
  - `PRX_MEMORY_EVOLUTION_PAPER_CN.md`
  - `PRX_MEMORY_EVOLUTION_PAPER_EN.md`
- Strengthened both evolution papers with explicit theoretical anchors:
  - First principles + Da Vinci style structural transfer + Darwinian evolution.
  - Added direct mapping to `Constraint + Variation + Selection` and aligned it with the MSES loop.
- Standardized `.gitignore` for open-source release readiness:
  - removed accidental `docs/` ignore.
  - ignored local runtime state (`data/*`, MCP local db path), while keeping `data/holdout/**` test fixtures.
  - ignored local secret/env files and common temporary/coverage artifacts.
  - kept lockfile policy explicit for reproducible service builds.
- Archived historical planning material:
  - moved `PROJECT_RESEARCH_PLAN.md` -> `docs/archive/PROJECT_RESEARCH_PLAN_2026-02-26.md`.
- Removed non-essential / duplicate documentation artifacts:
  - removed `docs/PRX_MEMORY_ACADEMIC_PAPER.md` (superseded by evolution papers).
  - removed legacy binary attachment under deleted `docs/code` reference path.
- Added zero-config to governed profile standardization:
  - `PRX_MEMORY_STANDARD_PROFILE=zero-config|governed`
  - default tag dimensions for project/tool/domain.
- Added MCP resource template capability:
  - `resources/templates/list`
  - `resources/read` for `prx://templates/...` payload generation.
- Added and passed regression coverage for new template and zero-config behavior.
- Multi-client validation passed via `./scripts/run_multi_client_validation.sh`.
- Full MCP crate regression passed via `cargo test -p prx-memory-mcp --all-targets --all-features`.

## Historical Notes
- Earlier work in this repo established:
  - streamable HTTP MCP support,
  - observability metrics and summary endpoint,
  - governance skill resource delivery,
  - dual-layer memory policy,
  - periodic compaction and decision-ratio rebalance,
  - holdout-based evolution acceptance tooling.
