#!/usr/bin/env bash
# Build the NeoBrawl WASM engine into web/pkg.
set -euo pipefail
cd "$(dirname "$0")"
cargo build --manifest-path engine/Cargo.toml --release --target wasm32-unknown-unknown --lib
wasm-bindgen --target web --out-dir web/pkg --no-typescript engine/target/wasm32-unknown-unknown/release/neobrawl.wasm
