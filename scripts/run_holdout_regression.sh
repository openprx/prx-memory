#!/usr/bin/env bash
set -euo pipefail

cargo test -p prx-memory-ai --test holdout_regression -- --nocapture
