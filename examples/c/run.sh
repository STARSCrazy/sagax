#!/bin/sh
# Builds the Sagax C library and runs the C examples (quickstart.c) against it.
set -e
cd "$(dirname "$0")/../.."
CC=${CC:-cc}
cargo build --release -p sagax-ffi
mkdir -p examples/c/out
for name in quickstart; do
	"$CC" -std=c99 -O2 -Wall -Wextra -I crates/sagax-ffi/include \
		examples/c/$name.c -L target/release -lsagax -o examples/c/out/$name
	PATH="$PWD/target/release:$PATH" LD_LIBRARY_PATH="$PWD/target/release" DYLD_LIBRARY_PATH="$PWD/target/release" \
		examples/c/out/$name
done
