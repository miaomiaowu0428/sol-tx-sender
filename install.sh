#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
exec cargo install --path . --locked --force \
  --config 'build.rustflags=["-C","target-cpu=native"]'
