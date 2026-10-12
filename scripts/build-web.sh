#!/usr/bin/env bash
# Builds the engine to WASM and writes the JS bindings to web/pkg.
# Requires: rustup target add wasm32-unknown-unknown
#           cargo install wasm-bindgen-cli --version 0.2.129
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/engine"
cargo build --release --lib --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir "$root/web/pkg" \
  target/wasm32-unknown-unknown/release/hive_engine.wasm
ls -l "$root/web/pkg"
