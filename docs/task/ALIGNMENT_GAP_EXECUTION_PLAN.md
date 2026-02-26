# Alignment Gap Execution Plan

## Plan
- Goal: align `prx-memory` with key requirements in `need.md` and target production release quality.
- Scope: MCP tool surface, governance loop, scope ACL, retrieval quality, observability, and release validation.
- Acceptance criteria:
  - `cargo fmt --all --check`
  - `cargo check --all-targets --all-features -q`
  - `cargo test --all-targets --all-features`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - 100k recall p95 target: `< 300ms`
- Risks and rollback:
  - Retrieval quality regressions after ranking or policy changes.
  - Compatibility regressions from stricter governance defaults.

## Pending
- [ ] P5-5b: run 24h reliability soak (`./scripts/run_soak_http.sh 86400 2`) and persist evidence in `docs/engineering/GO_LIVE_VALIDATION.md`.

## Completed
- [x] 2026-02-26 Added standardized profiles (`zero-config` / `governed`) and default tag dimensions for store/update/import behavior.
- [x] 2026-02-26 Added `resources/templates/list` and template resource rendering for faster MCP client integration.
- [x] 2026-02-26 Added resource and manifest access path for governance skill delivery.
- [x] 2026-02-26 Added HTTP stream advanced behaviors (SSE mode, session lease/renew, ack cursor semantics).
- [x] 2026-02-26 Added observability baseline and summary endpoint with cardinality controls.
- [x] 2026-02-26 Added maintenance tools (`memory_export/import/migrate/reembed/compact`) and coverage tests.
- [x] 2026-02-26 Added dual-layer governed memory flow and post-store verification.
- [x] 2026-02-26 Added evolution support (`memory_evolve`) with train+holdout acceptance model.
- [x] 2026-02-26 Added root README, skills README, roadmap, and bilingual evolution papers for open-source documentation.
