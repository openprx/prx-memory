# Repository Guidelines

## Project Overview
- `prx-memory` is a local-first MCP memory component for coding agents.
- It is designed to run without a centralized memory service and to support multiple MCP clients.
- Primary goals:
  - durable, searchable memory storage
  - governance-enforced memory quality
  - measurable evolution support (train+holdout acceptance)

## MCP Surface (Quick Reference)
- Transports:
  - `stdio` (default integration path)
  - `HTTP` (health, metrics, stream/session endpoints)
- Core tools:
  - `memory_store`, `memory_recall`, `memory_update`, `memory_forget`
  - `memory_list`, `memory_stats`
  - `memory_store_dual` (governed dual-layer write path)
- Maintenance tools:
  - `memory_export`, `memory_import`, `memory_migrate`
  - `memory_reembed`, `memory_compact`
- Evolution and skill tools:
  - `memory_evolve`
  - `memory_skill_manifest`
- MCP resources:
  - governance skill files under `prx://skills/...`
  - templates under `prx://templates/...` via `resources/templates/list`
- For public user-facing details, use:
  - root `README.md`
  - `docs/README.md`
  - `docs/ROADMAP.md`

## Project Structure & Module Organization
- Root is the `prx-memory` workspace scaffold.
- Planning notes currently live in `need.md` (ignored by Git).
- Historical planning artifacts are archived under `docs/archive/`.
- Rust implementation should be added in standard locations:
  - `src/` for runtime/library code
  - `tests/` for integration tests
  - `examples/` for runnable usage samples
- Keep documentation in `docs/`; keep production Rust code outside `docs/`.

## Build, Test, and Development Commands
- `cargo fmt` — format Rust code.
- `cargo check` — fast compile/type validation without producing binaries.
- `cargo test` — run unit and integration tests.
- `cargo clippy --all-targets --all-features -D warnings` — enforce lint cleanliness before PR.
- If `Cargo.toml` is not present yet, add project scaffolding first (`cargo init` or `cargo new`).

## Coding Style & Naming Conventions
- Use Rust 2021 idioms and 4-space indentation.
- File/module names: `snake_case` (example: `memory_store.rs`).
- Types/traits: `PascalCase`; functions/variables: `snake_case`; constants: `SCREAMING_SNAKE_CASE`.
- Prefer small modules with explicit ownership boundaries (storage, retrieval, protocol, config).
- Run `cargo fmt` and clippy before committing.

## Testing Guidelines
- Place unit tests next to code with `#[cfg(test)]`.
- Put cross-module and protocol tests in `tests/`.
- Name tests by behavior, e.g. `recall_returns_ranked_results`.
- Cover error paths (invalid config, empty index, embedding failures), not only happy paths.

## Commit & Pull Request Guidelines
- No usable commit history exists yet on `master`; adopt Conventional Commits now:
  - `feat: add mcp memory recall tool`
  - `fix: handle empty lancedb result set`
- PRs should include:
  - concise problem/solution summary
  - linked issue or task
  - test evidence (`cargo test` / `cargo check` output)
  - config or behavior examples when interfaces change

## Security & Configuration Tips
- Never commit API keys, tokens, or local DB artifacts.
- Use environment variables for secrets and provide `.env.example` when adding new config.
- Keep large generated data in ignored paths (for example `target/`, local index/cache directories).

## Task Planning Standard
- For substantial work, create or update a task document under `docs/task/`.
- Task docs must use exactly three top-level sections: `Plan`, `Pending`, `Completed`.
- Move completed items from `Pending` to `Completed` with date and result evidence.
- Keep a repo-wide changelog in `docs/CHANGELOG.md` and update it after each execution batch.
