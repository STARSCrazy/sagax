#!/bin/sh
# Builds the WebAssembly module, runs the Node tests and the squad example
# (examples/js/squad.mjs), whose output must equal the Rust example's.
set -e
cd "$(dirname "$0")/.."
NODE=${NODE:-node}
cargo build --release -p sagax-wasm --target wasm32-unknown-unknown
"$NODE" js/test.mjs
mkdir -p target/examples
cargo run -q -p sagax --example squad_walk > target/examples/squad-rust.txt
"$NODE" examples/js/squad.mjs > target/examples/squad-js.txt
diff target/examples/squad-rust.txt target/examples/squad-js.txt
echo "squad example: JavaScript output equals Rust"
