#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
wasm-pack build --target web --out-dir web/pkg --out-name jot --release --locked --no-default-features --features browser
