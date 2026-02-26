# Go-Live Validation Status

## Validation Date
- 2026-02-26 (UTC)

## 1) Multi-client Validation (>=2)
Command:
```bash
./scripts/run_multi_client_validation.sh
```

Result:
- status: PASS
- start: `2026-02-26T12:36:58Z`
- end: `2026-02-26T12:36:59Z`
- validated clients:
  - `stdio-harness` via `memory_evolve_stdio_flow_works`
  - `http-harness` via `http_health_and_mcp_call_work`
  - stream/http semantic path via `http_stream_ack_and_sse_work`

## 2) Reliability Soak
Command executed:
```bash
./scripts/run_soak_http.sh 60 4
```

Result:
- status: PASS
- completed_at: `2026-02-26T12:38:15Z`
- duration_sec: `60`
- qps: `4`
- iterations: `225`
- store_ok/fail: `225/0`
- recall_ok/fail: `225/0`
- total_fail: `0`
- metrics_summary:
  - `overall_alert_level=0`
  - `tool_error_ratio=0.0`
  - `remote_warning_ratio=0.0`
  - `label_overflow_total=0`

24h soak gate:
- status: PENDING
- command:
```bash
./scripts/run_soak_http.sh 86400 2
```
- note: needs dedicated long-running execution window.

## 3) Release-grade Install/Config/Troubleshooting Package
- `docs/engineering/INSTALL_AND_TROUBLESHOOTING.md`
- `docs/engineering/OBSERVABILITY.md`
- `docs/engineering/OBSERVABILITY_DASHBOARD.json`
- `docs/engineering/ALERT_RULES_PRX_MEMORY.yml`

## 4) Suggested Release Gate Command Set
```bash
cargo fmt --all --check
cargo check --all-targets --all-features -q
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
./scripts/run_multi_client_validation.sh
./scripts/run_soak_http.sh 86400 2
```
