# PRX-Memory Self-Evolving Memory System: A Local-First MCP Approach from Governance to Verifiable Evolution

## Abstract

This paper argues that a long-term memory system must do more than storing and retrieving text. It must be governable, testable, and evolvable. We present `prx-memory`, a local-first MCP memory component, as a three-layer architecture that integrates governance, retrieval, and evolution. We formalize a Minimal Self-Evolving System (MSES) with train-holdout acceptance, cost and risk penalties, and hard constraints. This prevents "train-only gains" and makes evolution behavior auditable. We provide a practical narrative, implementation mapping, reproducible commands, and feasibility analysis for production use.

Keywords: MCP, long-term memory, governance, verifiable evolution, holdout, local-first, Rust

## 1. Problem Statement

Many agent memory systems degrade over time into three failure modes:

1. Log pollution: raw chat and transient noise dominate memory.
2. Abstraction imbalance: decision-like entries grow too fast and lose actionability.
3. Evolution drift: strategy updates improve on train data but regress in practice.

`prx-memory` is not designed as "just another vector index". It is designed as an execution system that keeps memory useful under real engineering constraints across clients such as Codex, Claude Code, OpenClaw, and OpenPRX.

## 2. Research Questions and Falsifiable Hypotheses

### 2.1 Research Questions

1. Can a local-first MCP memory component preserve stable contracts across clients?
2. Can governance move from "documentation advice" to "tool-level enforcement"?
3. Can self-evolution be made measurable and falsifiable in production settings?

### 2.2 Hypotheses

- H1: If writes are constrained by governance and ratio control, retrieval pollution can be controlled long-term.
- H2: If acceptance requires train and holdout improvements, pseudo-evolution risk drops significantly.
- H3: If evolution output is coupled with compaction and regression checks, usability remains stable over time.

## 3. Key Story: From Repeated Pitfalls to Reusable Knowledge

### 3.1 Context

Plugin-like runtime environments often show "code changed but behavior did not". Root causes are typically hidden runtime state: cache, load path ordering, or reload sequencing. These issues are:

1. Highly recurrent.
2. Expensive to diagnose.
3. Often misdiagnosed as logic defects.

### 3.2 Why Traditional Handling Fails

Typical handling relies on retries plus ad hoc notes. This leads to:

1. Unstructured records with low recall quality.
2. Duplicate memory entries with semantic overlap.
3. No post-write verification of retrievability.

### 3.3 How `prx-memory` Handles It

The same issue is transformed into reusable memory artifacts:

1. Technical layer (fact): `Pitfall / Cause / Fix / Prevention`.
2. Principle layer (decision): created only when generalizable.
3. Post-write checks: recall verification and dedup paths.
4. Periodic maintenance: every 100 writes, dedup and ratio rebalancing.

This turns "experience text" into retrievable engineering assets.

## 4. System Model

## 4.1 Three-Layer Architecture

1. Governance layer
- Enforces format, tags, categories, ratio bounds, dedup, and verification.

2. Retrieval layer
- Executes lexical + vector fusion, optional remote rerank, and ACL scope filtering.

3. Evolution layer
- Evaluates candidate variants, applies acceptance logic, and guards generalization.

## 4.2 Theoretical Anchors: From Essence to Evolution

This paper explicitly follows a three-anchor reasoning path:

1. First-principles layer (essence and constraints)
- Define non-negotiable invariants and measurable objectives.
- In this system, this maps to \(G\), \(\Pi\), and constraint-aware evaluation in \(\mathcal{J}\).

2. Da Vinci style structural transfer layer (structured recomposition)
- Create candidate structures by cross-module recombination and process reordering.
- In this system, this maps to variation over \(\mathcal{H}\) using retained artifacts in \(\mathcal{M}\).

3. Darwinian evolution layer (selection and retention)
- Keep variants that survive real evaluation pressure and reject speculative gains.
- In this system, this maps to acceptance on train + holdout improvement and retention into \(\mathcal{M}\).

These three anchors can be reduced into the minimal executable triad:

\[
Constraint + Variation + Selection
\]

which is the operational definition used by the MSES loop in this paper.

## 4.3 Minimal Self-Evolving System (MSES)

System state:

\[
S=\langle G,\mathcal{H},\mathcal{M},\mathcal{E},\mathcal{J},\Pi\rangle
\]

Where:

- \(G\): objective function
- \(\mathcal{H}\): hypothesis or policy space
- \(\mathcal{M}\): retained memory
- \(\mathcal{E}\): environment or task distribution
- \(\mathcal{J}\): evaluator
- \(\Pi\): hard constraints

Acceptance criterion:

\[
accept(h)=\left(\Delta train>0\right)\land\left(\Delta holdout>0\right)\land\left(score_{eff}>score_{parent}\right)\land\Pi
\]

With:

\[
score_{eff}=score_{holdout}-\lambda\cdot cost-\mu\cdot risk
\]

This criterion maps directly to the current implementation (`select_candidate` and `EvolutionRunner`).

## 5. Implementation Mapping

### 5.1 Evolution Core

- `crates/prx-memory-core/src/mses.rs`
  - `select_candidate(...)`
  - train-holdout dual improvement checks
  - cost and risk penalties
- `crates/prx-memory-core/src/evolution.rs`
  - `EvolutionRunner::run_generation(...)`
  - best accepted candidate by effective score

### 5.2 MCP Surface

- `crates/prx-memory-mcp/src/server.rs`
  - `exec_memory_evolve(...)`
  - externalized as the `memory_evolve` MCP tool

### 5.3 Long-Term Stability Mechanism

- `crates/prx-memory-mcp/src/server.rs`
  - every 100 writes triggers `run_periodic_maintenance(...)`
  - dedup merge, decision ratio rebalance, low-value cleanup

### 5.4 Regression Evidence

- `crates/prx-memory-ai/tests/holdout_regression.rs`
  - reads `data/holdout/evolution_cases.json`
  - validates acceptance behavior against fixed holdout cases

## 6. Feasibility Argument

## 6.1 Theoretical Feasibility

If acceptance requires both train and holdout improvements under hard constraints, most overfitting-style candidates are rejected by construction. The claim is falsifiable: any candidate violating criteria is not accepted.

## 6.2 Engineering Feasibility

`prx-memory` already provides an executable baseline:

1. Dual transport: stdio and HTTP.
2. Multi-client validation harness: `./scripts/run_multi_client_validation.sh`.
3. Tool regression tests: `cargo test -p prx-memory-mcp --all-targets --all-features`.
4. Holdout regression: `./scripts/run_holdout_regression.sh`.
5. Maintenance toolchain: export, import, migrate, reembed, compact.

## 6.3 Production Feasibility

Risk is reduced through:

1. Provider abstraction and controlled fallback paths.
2. Metrics and summary alert surfaces.
3. Ratio control (decision <= 30%).
4. Periodic compaction and dedup to control memory growth.

## 7. Practical Path: From Zero-Config to Governed Mode

To balance adoption speed and rigor, `prx-memory` exposes profile-based standardization:

1. `zero-config`: low-friction onboarding with automatic defaults.
2. `governed`: stricter enforcement for production workflows.

Key env vars:

- `PRX_MEMORY_STANDARD_PROFILE=zero-config|governed`
- `PRX_MEMORY_DEFAULT_PROJECT_TAG`
- `PRX_MEMORY_DEFAULT_TOOL_TAG`
- `PRX_MEMORY_DEFAULT_DOMAIN_TAG`

## 8. How This Differs from Typical Memory Plugins

1. Not retrieval-only. It is governance + retrieval + evolution.
2. Not tied to one client runtime. It is an MCP contract component.
3. Not policy-by-doc only. Rules are encoded as executable constraints.

## 9. Limitations and Threats

1. Evolution quality depends on evaluator stability and representative holdout sets.
2. Remote embedding and rerank still depend on external key and provider health.
3. Long-horizon online A/B evidence still needs broader production accumulation.

## 10. Conclusion

The value of `prx-memory` is not "remembering more", but "remembering reusable, verifiable, evolvable knowledge". By encoding governance, retrieval, and evolution in one execution system, requirements that were previously narrative become testable and auditable behavior. This provides a practical production path for long-term agent memory.

## Appendix A: Reproducible Commands

```bash
cargo test -p prx-memory-mcp --all-targets --all-features
./scripts/run_multi_client_validation.sh
./scripts/run_holdout_regression.sh
```

## Appendix B: Suggested Companion Docs

- `README.md`
- `skills/README.md`
- `docs/engineering/INSTALL_AND_TROUBLESHOOTING.md`
- `docs/engineering/OBSERVABILITY.md`
- `docs/ROADMAP.md`
