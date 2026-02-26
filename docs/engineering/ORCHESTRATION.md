# prx-memory Development Orchestration

## Role Assignment
- Agent-A (Architecture): module boundaries, trait stability, backward compatibility.
- Agent-B (Implementation): feature implementation and unit tests.
- Agent-C (Quality): lint/test gates, performance, and observability checks.
- Commander: task decomposition, merge decisions, acceptance, and release readiness.

## Execution Cadence
1. Decompose requirements into independently shippable tasks (<= 1 day each).
2. Every task must include acceptance criteria and rollback points.
3. All code must pass quality gates before merge.
4. Commander performs design review, test review, and risk review per task.

## Production Rust Standards
- Toolchain: stable Rust with lockfile committed.
- Formatting: `cargo fmt --all`.
- Linting: `cargo clippy --all-targets --all-features -- -D warnings`.
- Compile gate: `cargo check --all-targets --all-features -q` (no warnings expected).
- Tests: `cargo test --all-targets --all-features`.
- Prohibited: unused dependencies, ignored errors, implicit panics (except tests).

## Task Acceptance Template
- Scope: explicit change boundaries.
- Invariants: constraints that must remain true.
- Tests: added/updated test list.
- Observability: logs/metrics coverage.
- Rollback: fast rollback approach if validation fails.

## Current Cycle
- T1: implement evolution orchestration core (MSES runner).
- T2: unify viability score and evolution acceptance policy.
- T3: run full quality gates and publish acceptance results.
