#!/usr/bin/env bash
set -euo pipefail

cargo test --release -p prx-memory-storage --test perf_100k -- --ignored --nocapture
