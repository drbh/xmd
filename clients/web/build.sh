#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
# --out-dir is relative to the crate, so this lands in clients/web/pkg.
wasm-pack build hosts/wasm --target web --out-dir ../../clients/web/pkg --out-name xmd --release --locked
