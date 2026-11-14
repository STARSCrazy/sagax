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
- **Decisions and memory**: utility AI, state machines, a memory of the
  world, and **group tactics**: a squad leader plans from what it knows,
  hands out goals with shout or radio delay, and strikes at the same moment.
  The `Crew` runs that loop in one call per tick. A rules object lets a game put its own logic (sight, costs, triggers) into nine fixed points.
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

Three jobs: a path, a decision, a squad. Each is shown in Rust, C and
JavaScript; [QUICKSTART.md](docs/QUICKSTART.md) has them in full, and every
snippet comes from code that is built and run in this repository; some are
shortened.

**A path.** A grid has no default cost: pick a preset (`Octile`: straight 10,
diagonal 14; `Tabletop`: every step 1).

```rust
use sagax::prelude::*;

let mut map = DenseGrid::new(10, 10);
let mut nav = Navigator::new(Topology::Square(GridRules::preset(Preset::Octile)), 1000);
let report = nav.find_path(
    &mut map,
    Cell::new(0, 0, 0),
    &[Cell::new(9, 0, ANY_LEVEL)],
    Budget::UNLIMITED,
);
assert_eq!(report.status, NavStatus::Found);
assert_eq!(report.cost, 90);
```

```c
#include "sagax.h"

sx_rules rules;
sx_rules_default(SX_GRID_SQUARE, SX_PRESET_OCTILE, &rules);
sx_nav *nav = sx_nav_new(&rules);
sx_map map = { .step = my_step, .context = my_game };   /* or sx_grid_map(grid, &map) */
sx_point start = {2, 4, 0}, goal = {12, 4, 0}, way[64];
sx_result result;
int n = sx_nav_find_path(nav, &map, start, &goal, 1, 0, SX_OUT_CELLS, way, 64, &result);
sx_nav_free(nav);
```

```js
import { loadSagax } from './js/sagax.mjs';

const lib = await loadSagax(wasmUrlOrBytes);
const grid = lib.grid(10, 10, { preset: 'octile' });
const way = grid.findPath({ x: 0, y: 0 }, { x: 9, y: 0 });
console.log(way.status, way.cost);   // found 90
grid.free();
```

**A decision.** Options with considerations; the best score wins.

```rust
let mut d = Decider::new();
let a = d.add_option(ONE);
let b = d.add_option(ONE);
assert!(d.add_consideration(a, Consideration::new(0, Curve::Linear, 0, 10)));
assert!(d.add_consideration(b, Consideration::new(0, Curve::Linear, 0, 10)));
let candidates = [Candidate::new(a, 0), Candidate::new(b, 0)];
let rows = [[2], [8]];
let choice = d.choose(&candidates, &rows, Pick::Best, 0).unwrap();
assert!(matches!(choice, Some(Choice { candidate: 1, .. })));
```

```c
sx_decider *d = sx_decider_new();
sx_consideration near = {0, SX_CURVE_INVERSE, 1, 10};      /* input 0: distance */
sx_candidate candidates[2] = {{0, 7}, {0, 8}};
int32_t inputs[2] = {6, 2};                                /* one row per candidate */
sx_choice choice;
int32_t attack = sx_decider_add_option(d, SX_ONE);
sx_decider_add_consideration(d, (uint32_t)attack, &near);
sx_decider_decide(d, candidates, 2, inputs, 1, SX_PICK_BEST, 0, 0, NULL, &choice);
sx_decider_free(d);
```

```js
const d = lib.decider();
const a = d.addOption(1, [{ input: 0, curve: 'linear', min: 0, max: 10 }]);
const b = d.addOption(1, [{ input: 0, curve: 'linear', min: 0, max: 10 }]);
const choice = d.decide([{ option: a, inputs: [2] }, { option: b, inputs: [8] }]);
console.log('candidate', choice.index);   // candidate 1
d.free();
```

**A squad.** A `Crew` is a squad leader plus the members' goal keepers, run by
one call per tick. It reports what happened (`Planned`, `Activated`,
`Signal`, `Order`, `Start`, `Arrived`, `Broken`, `NoPlan`); walking and
shooting stay yours.

```rust
let mut crew = Crew::new(CrewConfig {
    squad: SquadConfig { members: 1, memory: 4, areas: 2, inbox: 4,
                         metric: Metric::Chebyshev, ..SquadConfig::default() },
    ..CrewConfig::default()
})
.unwrap();
let member = Member { id: 1, pos: [0, 0, 0], roles: 0, skill: 128, state: MemberState::Active };
assert!(crew.add_member(member).is_some());
crew.squad_mut().set_target_kind(1);
crew.squad_mut().add_tactic(tactics::rush());
crew.observe(0, Fact { subject: 11, kind: 1, pos: [10, 0, 0], seen_at: 0, ..Fact::default() });
let mut events = Vec::new();
crew.tick(0, &[], &[], 7, &mut events).unwrap();
assert!(events.iter().any(|e| matches!(e, CrewEvent::Planned(_))));
```

```c
sx_crew_config config;
sx_crew_config_default(&config);
config.squad.metric = SX_METRIC_CHEBYSHEV;
sx_crew *crew = sx_crew_new(&config);
sx_crew_add_member(crew, &member);                 /* an sx_member: id, pos, skill, state */
sx_squad_set_target_kind(sx_crew_squad(crew), 1);
sx_squad_add_rush(sx_crew_squad(crew));
sx_crew_observe(crew, 0, &enemy, NULL);
count = sx_crew_tick(crew, 0, NULL, 0, NULL, 0, 7, events, 8, &total);
sx_crew_free(crew);
```

```js
const crew = lib.crew({ squad: { members: 1, memory: 4, areas: 2, inbox: 4, metric: 'chebyshev' } });
crew.addMember({ id: 1, pos: [0, 0, 0], skill: 128 });
crew.squad().setTargetKind(1);
crew.squad().addTactic(lib.tactics.rush());
crew.observe(0, { subject: 11, kind: 1, pos: [10, 0, 0], seenAt: 0 });
for (const event of crew.tick(0, { seed: 7 })) console.log('event', event.kind);   // event planned
crew.free();
```

**Examples.** All built and run by the checks below:

| Example | What |
|---|---|
| `examples/c/quickstart.c` | a path, a decision, a crew and a state machine in C |
| `examples/c/memory.c` | the memory blocks in C (knowledge, messages, areas, timers, alertness) |
| `examples/c/squad.c` | four members walk around a wall and attack together, with `sx_grid`, `sx_nav_*` and `sx_crew_*` |
| `examples/c/ambush_rules.c` | four guards ambush an intruder with the game's own sight, hand signs, kill zone and "seen" abort in an `sx_squad_rules` object |
| `examples/js/quickstart.mjs` | the three jobs in JavaScript |
| `examples/js/squad.mjs` | the same squad walk in JavaScript |
| `crates/sagax/examples/squad_walk.rs` | the same squad walk in Rust (`cargo run -p sagax --example squad_walk`) |
| `crates/sagax/examples/ambush_rules.rs` | the ambush in Rust, with the game's own pieces in a `SquadRules` object (`cargo run -p sagax --example ambush_rules`) |

The three squad walks print the same lines, and the checks compare them.

## Layout

| Path | What |
|---|---|
| `crates/sagax` | the umbrella crate: `use sagax::prelude::*;` brings the common names; `sagax::nav`, `sagax::decide`, `sagax::debug` |
| `crates/sagax-nav` | navigation: `Navigator`, search core, flow fields, smoothing, formations, D* Lite, grids |
| `crates/sagax-decide` | decisions (utility AI), state machines, memory of the world, group tactics (`Squad`, `Crew`) |
| `crates/sagax-ffi` | C interface: `sagax.dll` / `libsagax.so`, header in `include/sagax.h` |
| `crates/sagax-wasm` | WebAssembly interface |
| `crates/sagax-core`, `crates/sagax-debug` | shared cells and positions, diagnostic events and a collector for them |
| `js/` | JavaScript wrapper, Node tests, an adapter for Foundry VTT scenes |
| `examples/` | C and JavaScript examples (the Rust ones are in the crates) |
| `docs/` | quickstart, guide, performance, roadmap |

## Build

```sh
cargo build --release -p sagax-ffi                                # C library in target/release
cargo build --release -p sagax-wasm --target wasm32-unknown-unknown  # .wasm for JavaScript
```

Checks: `cargo test --workspace`, `sh examples/c/run.sh` (needs a C compiler;
`CC=gcc sh examples/c/run.sh`), `sh js/run.sh`.

## Docs

- [Quickstart](docs/QUICKSTART.md): a path, a decision and a squad in five minutes
- [Guide](docs/USAGE.md): which building block for which case, recipes, pitfalls
- [Performance](docs/PERFORMANCE.md)
- [Roadmap](docs/ROADMAP.md)

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
