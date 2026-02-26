#!/usr/bin/env bash
set -euo pipefail

entry="${1:-}"
if [[ -z "$entry" ]]; then
  echo "usage: validate_memory_entry.sh '<entry>'" >&2
  exit 2
fi

if [[ ${#entry} -gt 500 ]]; then
  echo "invalid: entry exceeds 500 chars" >&2
  exit 1
fi

if [[ "$entry" != *"Pitfall:"* && "$entry" != *"Decision principle"* ]]; then
  echo "invalid: missing required template prefix" >&2
  exit 1
fi

echo "ok"
