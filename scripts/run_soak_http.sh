#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

DURATION_SEC="${1:-120}"
QPS="${2:-5}"
ADDR="${PRX_SOAK_HTTP_ADDR:-127.0.0.1:18787}"
DB_PATH="${PRX_SOAK_DB_PATH:-/tmp/prx-memory-soak-$$.json}"
BIN_PATH="${PRX_SOAK_BIN_PATH:-$ROOT_DIR/target/debug/prx-memoryd}"
LOG_PATH="${PRX_SOAK_LOG_PATH:-/tmp/prx-memory-soak-$$.log}"

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

if ! command -v awk >/dev/null 2>&1; then
  echo "awk is required" >&2
  exit 1
fi

cargo build -q -p prx-memory-mcp --bin prx-memoryd

PRX_MEMORYD_TRANSPORT=http PRX_MEMORY_HTTP_ADDR="$ADDR" PRX_MEMORY_DB="$DB_PATH" "$BIN_PATH" >"$LOG_PATH" 2>&1 &
SERVER_PID="$!"

cleanup() {
  if kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -f "$DB_PATH"
}
trap cleanup EXIT

for _ in $(seq 1 120); do
  if curl -fsS "http://$ADDR/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

if ! curl -fsS "http://$ADDR/health" >/dev/null 2>&1; then
  echo "server not ready on $ADDR" >&2
  exit 1
fi

SLEEP_SEC="$(awk -v qps="$QPS" 'BEGIN { if (qps <= 0) print 1.0; else print 1.0 / qps }')"
START_EPOCH="$(date +%s)"
END_EPOCH="$((START_EPOCH + DURATION_SEC))"

STORE_OK=0
STORE_FAIL=0
RECALL_OK=0
RECALL_FAIL=0
TOTAL=0

while [ "$(date +%s)" -lt "$END_EPOCH" ]; do
  TOTAL="$((TOTAL + 1))"
  STORE_PAYLOAD="{\"jsonrpc\":\"2.0\",\"id\":$TOTAL,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_store\",\"arguments\":{\"text\":\"soak entry $TOTAL\",\"category\":\"fact\",\"scope\":\"global\",\"importance_level\":\"low\",\"governed\":false,\"tags\":[\"project:prx-memory\",\"tool:mcp\",\"domain:soak\"]}}}"
  STORE_CODE="$(curl -s -o /tmp/prx-soak-store-$$.out -w "%{http_code}" -H 'Content-Type: application/json' -d "$STORE_PAYLOAD" "http://$ADDR/mcp")"
  if [ "$STORE_CODE" = "200" ]; then
    STORE_OK="$((STORE_OK + 1))"
  else
    STORE_FAIL="$((STORE_FAIL + 1))"
  fi

  RECALL_PAYLOAD="{\"jsonrpc\":\"2.0\",\"id\":$((100000 + TOTAL)),\"method\":\"tools/call\",\"params\":{\"name\":\"memory_recall\",\"arguments\":{\"query\":\"soak entry\",\"limit\":3}}}"
  RECALL_CODE="$(curl -s -o /tmp/prx-soak-recall-$$.out -w "%{http_code}" -H 'Content-Type: application/json' -d "$RECALL_PAYLOAD" "http://$ADDR/mcp")"
  if [ "$RECALL_CODE" = "200" ]; then
    RECALL_OK="$((RECALL_OK + 1))"
  else
    RECALL_FAIL="$((RECALL_FAIL + 1))"
  fi

  sleep "$SLEEP_SEC"
done

SUMMARY_JSON="$(curl -sS "http://$ADDR/metrics/summary")"
TOTAL_FAIL="$((STORE_FAIL + RECALL_FAIL))"

END_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
cat <<REPORT
[soak] completed_at=$END_TS
[soak] duration_sec=$DURATION_SEC qps=$QPS total_iterations=$TOTAL
[soak] store_ok=$STORE_OK store_fail=$STORE_FAIL
[soak] recall_ok=$RECALL_OK recall_fail=$RECALL_FAIL
[soak] total_fail=$TOTAL_FAIL
[soak] metrics_summary=$SUMMARY_JSON
[soak] server_log=$LOG_PATH
REPORT
