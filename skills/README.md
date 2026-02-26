# Skills

This directory contains skill materials used by `prx-memory`.

## Available Skill Package

- `prx-memory-governance`
  - Path: `skills/prx-memory-governance/SKILL.md`
  - Purpose: enforce memory governance practices and provide a reusable online regression flow across core, maintenance, and evolve toolchains.

## How Clients Consume Skills

### If the client supports MCP resources

1. Call `resources/list`
2. Call `resources/read`
3. Read these URIs:
   - `prx://skills/prx-memory-governance/SKILL.md`
   - `prx://skills/prx-memory-governance/references/memory-governance.md`
   - `prx://skills/prx-memory-governance/references/tag-taxonomy.md`

### If the client does not support MCP resources

- Call `tools/call` with `memory_skill_manifest`.

## Functional-Line Regression Shortcut

After loading the skill, run this ordered flow:

1. capability: `memory_stats` + `memory_skill_manifest` + `resources/list` + `resources/templates/list`
2. core CRUD: `memory_store -> memory_recall -> memory_update -> memory_list -> memory_forget`
3. governed dual-layer: `memory_store_dual` (+ required principle fields when `include_principle=true`)
4. maintenance: `memory_export/import/migrate/reembed/compact`
5. evolution: `memory_evolve`
6. cleanup: remove regression entries and re-check `memory_stats`

## Resource Templates

The server also exposes template resources to speed up client integration:

- `resources/templates/list`
- `resources/read` on:
  - `prx://templates/memory-store?...`
  - `prx://templates/memory-recall?...`
  - `prx://templates/memory-store-dual?...`

These templates return standard JSON payload skeletons for direct MCP tool calls.
