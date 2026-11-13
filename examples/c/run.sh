#!/bin/sh
# Builds the Sagax C library and runs the C examples (quickstart.c, memory.c, squad.c) against it.
# Every example is built twice: as C99 (no layout checks) and as C11 (the
# _Static_assert layout checks at the end of sagax.h are on).
set -e
cd "$(dirname "$0")/../.."
CC=${CC:-cc}
cargo build --release -p sagax-ffi
mkdir -p examples/c/out
for std in c99 c11; do
	for name in quickstart memory squad; do
		"$CC" -std=$std -O2 -Wall -Wextra -Werror -I crates/sagax-ffi/include \
			examples/c/$name.c -L target/release -lsagax -o examples/c/out/$name-$std
		PATH="$PWD/target/release:$PATH" LD_LIBRARY_PATH="$PWD/target/release" DYLD_LIBRARY_PATH="$PWD/target/release" \
			examples/c/out/$name-$std
	done
done
