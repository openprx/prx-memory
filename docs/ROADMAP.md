# prx-memory Public Roadmap

## Goal

Build a production-grade, local-first MCP memory component that is reusable across multiple coding-agent clients.

## Scope

- MCP transport compatibility (`stdio`, `HTTP`)
- Memory governance and quality controls
- Retrieval quality and provider portability
- Evolution support with holdout-validated selection
- Operational readiness and observability

## Milestone Snapshot

- M1: MCP baseline and core tooling - Completed
- M2: Governance and retrieval hardening - Completed
- M3: Observability and multi-client validation - Completed
- M4: Go-live gate evidence (long soak) - In progress

## Current Open Item

- Run and publish 24h reliability soak evidence:
  - `./scripts/run_soak_http.sh 86400 2`
  - Record results in `docs/engineering/GO_LIVE_VALIDATION.md`

## Release Readiness Criteria

1. Stable behavior across at least two MCP client harnesses.
2. Full regression pass for memory tool surface.
3. No critical alerts during soak run.
4. Documentation set is complete and navigable.

