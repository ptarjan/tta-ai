#!/bin/bash
# Build the browser advisor's engine and drop it next to the static files.
#
# The weights are baked in at COMPILE time from experiments/rust_champion_*.json,
# so re-run this after a champion promotion you actually want to ship -- or pass
# a vector in the request's `weights` field to override without rebuilding.
set -euo pipefail
cd "$(dirname "$0")/../rust"
PATH="$HOME/.cargo/bin:$PATH" cargo build --release --target wasm32-unknown-unknown -p ttawasm
cp target/wasm32-unknown-unknown/release/ttawasm.wasm ../app/tta.wasm
echo "app/tta.wasm  $(wc -c < ../app/tta.wasm) bytes"
