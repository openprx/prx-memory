---
name: prx-memory-governance
description: Enforce production-grade memory governance for prx-memory MCP, including recall-before-store, dual-layer memory, deduplication, and periodic compaction.
---

# PRX Memory Governance

## When to use
Use this skill when:
- Storing or updating long-term memory entries
- Diagnosing repeated tool failures
- Running memory cleanup or compaction
- Auditing memory quality across agents/scopes

## Fast start (main functional lines)
Run these lines in order so a client can use `prx_memory` immediately without relearning tool semantics.

1. Capability line (service readiness)
- `memory_stats`
- `memory_skill_manifest`
- `resources/list`
- `resources/templates/list`

2. Core CRUD line
- `memory_store` -> `memory_recall` -> `memory_update` -> `memory_list` -> `memory_forget`

3. Governed dual-layer line
- `memory_store_dual` with technical layer + principle layer.
- If `include_principle=true`, provide all required fields:
  - `principle_tag`
  - `principle_rule`
  - `trigger`
  - `action`
- Negative-path check: missing required principle fields should return `-32602`.

4. Maintenance line
- `memory_export`
- `memory_import`
- `memory_migrate`
- `memory_compact` (`dry_run=true` then `dry_run=false`)
- `memory_reembed` (if no embedding key is configured, controlled failure is expected)

5. Evolution line
- `memory_evolve` with train+holdout candidates.
- Accept only when constraints pass and dual-set improvement is met.

6. Cleanup line
- Delete test entries created by regression via `memory_forget`.
- Verify baseline via `memory_stats` after cleanup.

## Mandatory workflow
1. `memory_recall` with tool + error + symptom keywords before any new write.
2. Deduplicate against top-k similar entries.
3. In governed mode, use `memory_store_dual` and store technical + principle layers together.
4. Store principle-layer memory only if broadly reusable.
5. If importance is `critical`, run post-store recall verification.
6. If a similar entry exists, update/merge instead of inserting duplicate.

## Memory format contract
- Max 500 chars per entry
- One entry = one knowledge point
- Required fields: `category`, `importance`, `tags`, `scope`
- Tags must include prefixed dimensions: `project:*`, `tool:*`, `domain:*`
- `importance` enum only: `low | medium | high | critical`
- Decision entries must remain <= 30% of total memories

## Dual-layer templates
Technical layer:
`Pitfall: [symptom]. Cause: [root cause]. Fix: [solution]. Prevention: [how to avoid].`

Principle layer (conditional):
`Decision principle ([tag]): [rule]. Trigger: [when]. Action: [do].`

## Retrieval quality rules
- Use precise keywords: tool name + core error + symptom.
- Prefer scoped recall (`project`, `agent`, `global`) to avoid pollution.
- Reject raw chat logs, stacktrace dumps, or low-signal noise.

## Compaction policy
- Every 100 entries, run a compaction pass:
  - merge semantically equivalent entries
  - remove low-value duplicates
  - rebalance decision ratio

## References
- Governance details: `references/memory-governance.md`
- Tag taxonomy: `references/tag-taxonomy.md`

## Scripts
- Skill-local helpers:
  - `scripts/validate_memory_entry.sh` validates entry format
  - `scripts/generate_dual_layer_template.sh` prints dual-layer template text
- Repository regression scripts:
  - `../../../scripts/run_multi_client_validation.sh` validates stdio/http functional paths
  - `../../../scripts/run_holdout_regression.sh` validates holdout acceptance path
  - `../../../scripts/run_soak_http.sh` validates HTTP reliability under sustained load
  - `../../../scripts/run_perf_100k.sh` validates 100k recall performance gate
