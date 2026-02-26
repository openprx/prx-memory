# Observability Integration Guide

## Endpoints
- `GET /metrics`: Prometheus exposition text.
- `GET /metrics/summary`: JSON summary for dashboard and health probes.

## Cardinality Controls
Environment variables:
- `PRX_METRICS_MAX_RECALL_SCOPE_LABELS` (default: `32`)
- `PRX_METRICS_MAX_RECALL_CATEGORY_LABELS` (default: `32`)
- `PRX_METRICS_MAX_RERANK_PROVIDER_LABELS` (default: `16`)

When limits are exceeded, new label values are not emitted and are counted in:
- `prx_memory_metrics_label_overflow_total{dimension=...}`

## Alert Thresholds
Environment variables:
- `PRX_ALERT_TOOL_ERROR_RATIO_WARN` (default: `0.05`)
- `PRX_ALERT_TOOL_ERROR_RATIO_CRIT` (default: `0.20`)
- `PRX_ALERT_REMOTE_WARNING_RATIO_WARN` (default: `0.25`)
- `PRX_ALERT_REMOTE_WARNING_RATIO_CRIT` (default: `0.60`)

Alert state signals:
- `prx_memory_alert_state{signal="tool_error_ratio"}`
- `prx_memory_alert_state{signal="remote_warning_ratio"}`
- `prx_memory_alert_state{signal="metrics_label_overflow"}`

State encoding:
- `0`: OK
- `1`: WARN
- `2`: CRIT

## Session Telemetry
- `prx_memory_sessions_active`
- `prx_memory_sessions_created_total`
- `prx_memory_sessions_renewed_total`
- `prx_memory_sessions_expired_total`
- `prx_memory_session_access_errors_total{kind=...}`

## Files
- Grafana dashboard sample: `docs/engineering/OBSERVABILITY_DASHBOARD.json`
- Prometheus alert rules sample: `docs/engineering/ALERT_RULES_PRX_MEMORY.yml`
