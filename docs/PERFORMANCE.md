# Performance

Measured on a desktop CPU (Intel i7-12700F, Windows, Rust stable, release
build). 400 searches on random 128 x 96 maps with two levels, ramps and
blocked cells, budget 30000, costs 10/14, about 2800 cells expanded per
search:

```sh
cargo run --release -p sagax-ffi --example bench_parts
```

| Variant | µs / search |
|---|---|
| core, map in Rust (callback inlined) | ~330 |
| core, map behind a C function pointer | ~330 |
| whole C interface, `step` per neighbour | ~375 |
| whole C interface, batched `steps` | ~375 |

That is roughly 120 ns per expanded cell. A comparable hand-written A* in C
with a binary heap took about the same time on the same maps, with the same
paths and the same number of cells expanded.

## Findings

- Calling a C callback from Rust costs the same as a C-to-C call through a
  function pointer.
- Each straight step is asked once per cell and reused for the corner check
  of diagonals: about 40 % fewer callbacks than asking every move.
- The open list key packs total and estimate into one `u64`, so one
  comparison replaces three (~20 % faster).
- The batched `steps` callback helps only when a single `step` answer is
  expensive in the game.
- Runs vary by ±15 µs; compare several runs.
