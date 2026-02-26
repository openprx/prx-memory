#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

START_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "[multi-client] start: $START_TS"

cargo test --all-targets --all-features memory_evolve_stdio_flow_works
cargo test --all-targets --all-features http_health_and_mcp_call_work
cargo test --all-targets --all-features http_stream_ack_and_sse_work

END_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "[multi-client] completed: $END_TS"
echo "[multi-client] validated clients: stdio-harness, http-harness"
