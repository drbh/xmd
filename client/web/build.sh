#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
# --out-dir is relative to the crate, so this lands in client/web/pkg.
wasm-pack build lang/wasm --target web --out-dir ../../client/web/pkg --out-name xmd --release --locked
