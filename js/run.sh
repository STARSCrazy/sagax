#!/bin/sh
# Builds the WebAssembly module and runs the Node tests.
set -e
cd "$(dirname "$0")/.."
NODE=${NODE:-node}
cargo build --release -p sagax-wasm --target wasm32-unknown-unknown
"$NODE" js/test.mjs
