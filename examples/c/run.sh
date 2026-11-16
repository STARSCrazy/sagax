#!/bin/sh
# Builds the Sagax C library and runs the C examples (quickstart.c, memory.c, squad.c, ambush_rules.c, mission.c, perception.c, contagion.c, goal_guess.c, trace.c) against it.
# Every example is built twice: as C99 (no layout checks) and as C11 (the
# _Static_assert layout checks at the end of sagax.h are on). The output of
# squad.c, ambush_rules.c, mission.c, perception.c, contagion.c and
# goal_guess.c must equal the Rust examples' (cargo run -p sagax --example
# squad_walk, ambush_rules, mission_guard, perception_scene, contagion_scene
# and goal_guess_scene).
set -e
cd "$(dirname "$0")/../.."
CC=${CC:-cc}
cargo build --release -p sagax-ffi
mkdir -p examples/c/out
cargo run -q -p sagax --example squad_walk > examples/c/out/squad-rust.txt
cargo run -q -p sagax --example ambush_rules > examples/c/out/ambush-rust.txt
cargo run -q -p sagax --example mission_guard > examples/c/out/mission-rust.txt
cargo run -q -p sagax --example perception_scene > examples/c/out/perception-rust.txt
cargo run -q -p sagax --example contagion_scene > examples/c/out/contagion-rust.txt
cargo run -q -p sagax --example goal_guess_scene > examples/c/out/goal_guess-rust.txt
for std in c99 c11; do
	for name in quickstart memory squad ambush_rules mission perception contagion goal_guess trace; do
		"$CC" -std=$std -O2 -Wall -Wextra -Werror -I crates/sagax-ffi/include \
			examples/c/$name.c -L target/release -lsagax -o examples/c/out/$name-$std
		PATH="$PWD/target/release:$PATH" LD_LIBRARY_PATH="$PWD/target/release" DYLD_LIBRARY_PATH="$PWD/target/release" \
			examples/c/out/$name-$std > examples/c/out/$name-$std.raw
		# Windows C runtimes write CRLF; compare plain lines.
		tr -d '\r' < examples/c/out/$name-$std.raw > examples/c/out/$name-$std.txt
		cat examples/c/out/$name-$std.txt
	done
	diff examples/c/out/squad-rust.txt examples/c/out/squad-$std.txt
	diff examples/c/out/ambush-rust.txt examples/c/out/ambush_rules-$std.txt
	diff examples/c/out/mission-rust.txt examples/c/out/mission-$std.txt
	diff examples/c/out/perception-rust.txt examples/c/out/perception-$std.txt
	diff examples/c/out/contagion-rust.txt examples/c/out/contagion-$std.txt
	diff examples/c/out/goal_guess-rust.txt examples/c/out/goal_guess-$std.txt
done
echo "squad, ambush_rules, mission, perception, contagion and goal_guess examples: C output equals Rust"
