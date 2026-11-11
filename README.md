# Sagax

**Fast, deterministic game AI in Rust.** Path search on square and hex grids
with levels (stairs, bridges, ramps), usable from Rust, from C (one DLL /
shared library, one header) and from JavaScript (WebAssembly, no
dependencies).

*Sagax* is Latin for "keen-scented, sharp-witted".

## Features

- **A\* / Dijkstra** with a work budget: split a search over several frames,
  every split gives the same result.
- **Several goals at once**: the way to the nearest exit is one search.
- **Fleeing**: a way away from threats, to where they need longest to follow.
- **Reachable area**: every cell within a movement budget, and the way to
  each.
- **Partial ways**: an unreachable goal gives the way to the closest
  reachable cell.
- **Smoothing**: straight lines at any angle instead of grid steps, never
  through walls.
- **Formations**: group members follow a leader in slots and fall into line
  at doors.
- **Flow fields**: one search for many walkers with the same goals.
- **D\* Lite**: one walker's way, repaired while the map changes.
- **Square and hex grids** (pointy or flat top, odd or even offset), 4 or 8
  directions, walls between cells, extra costs, zones that cost extra to
  leave (zones of control).
- **Your map, your rules**: the game answers "can I step from here to
  there?" through a callback. Nothing is copied.
- **Tracing**: every step of a search for debugging and logging; free when
  turned off.
- **Deterministic**: the same answers give the same ways on every machine.
  No allocation during a search within the reserved capacity.

## Quick look

Rust:

```rust
use sagax_nav::grid::{Cell, DenseGrid, GridRules};
use sagax_nav::nav::{ANY_LEVEL, Navigator, Output};
use sagax_nav::topology::Topology;

let mut map = DenseGrid::new(64, 64);
map.set(10, 5, DenseGrid::BLOCKED);
let mut nav = Navigator::new(Topology::Square(GridRules::default()), 4096);
let report = nav.find_path(&mut map, Cell::new(0, 0, 0), &[Cell::new(40, 9, ANY_LEVEL)], usize::MAX);
let corners = nav.output(&mut map, Output::Smooth);
```

C:

```c
#include "sagax.h"

sx_rules rules = { .grid = SX_GRID_SQUARE, .cost_straight = 10,
                   .cost_diagonal = 14, .node_capacity = 32768 };
sx_nav *nav = sx_nav_create(&rules);
sx_map map = { .step = my_step, .context = my_game };
sx_point start = {2, 4, 0}, goal = {40, 9, SX_ANY_LEVEL}, way[256];
sx_result r;
int n = sx_nav_find_path(nav, &map, start, &goal, 1, 0, SX_OUT_TURNS, way, 256, &r);
```

JavaScript:

```js
import { loadSagax, HEX } from './js/sagax.mjs';

const lib = await loadSagax(wasmUrlOrBytes);
const grid = lib.grid(30, 20, { hex: HEX.oddR });
grid.block(5, 5);
const way = grid.findPath({ x: 0, y: 0 }, { x: 10, y: 4 }, { output: 'smooth' });
// {cells, status: 'found', cost, ...}
```

## Layout

| Path | What |
|---|---|
| `crates/sagax-nav` | the library: `Navigator`, search core, flow fields, smoothing, formations, D* Lite, grids |
| `crates/sagax-decide` | decisions (utility AI): options x targets, curves, seeded choice |
| `crates/sagax-ffi` | C interface: `sagax.dll` / `libsagax.so`, header in `include/sagax.h` |
| `crates/sagax-wasm` | WebAssembly interface |
| `crates/sagax-core`, `crates/sagax-debug` | diagnostic events and a collector for them |
| `js/` | JavaScript wrapper, Node tests, an adapter for Foundry VTT scenes |
| `examples/c/` | a small C program |
| `docs/` | guide, performance, roadmap |

## Build

```sh
cargo build --release -p sagax-ffi                                # C library in target/release
cargo build --release -p sagax-wasm --target wasm32-unknown-unknown  # .wasm for JavaScript
```

Checks: `cargo test --workspace`, `sh examples/c/run.sh`, `sh js/run.sh`.

## Docs

- [Guide](docs/USAGE.md): which building block for which case, recipes, pitfalls
- [Performance](docs/PERFORMANCE.md)
- [Roadmap](docs/ROADMAP.md)

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
