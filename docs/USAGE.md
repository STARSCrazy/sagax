# Guide

What to use for which case, how to call it, and where the pitfalls are. The
exact contracts are in `crates/sagax-ffi/include/sagax.h` (C),
`js/sagax.mjs` (JavaScript) and the Rust docs (`cargo doc -p sagax-nav --open`).

## 1. Which building block?

| The game asks | Building block | C | JavaScript |
|---|---|---|---|
| One walker, one goal | A* | `sx_nav_find_path` | `grid.findPath` |
| Where can I get with X movement? | Reachable area | `sx_nav_reachable` + `sx_nav_path_to` | `grid.reachable` + `grid.pathTo` |
| A way without staircase steps | Smoothing | output `SX_OUT_SMOOTH` | `output: 'smooth'` |
| A group follows its leader, one cell each | Formation | `sx_nav_formation` | `grid.formation` |
| Many walkers, same goals | Flow field | `sx_flow_*` | `grid.buildFlow` + `grid.flowNext` |
| The map changes while a walker is on its way | D* Lite | `sx_dstar_*` | `grid.dstar*` |

Everything works on square and hex grids and in every interface.

**The same shape everywhere:**

- **Rules** (square or hex, costs, memory) are given once when the handle is
  made: C `sx_rules`, JS `lib.grid(w, h, rules)`, Rust
  `Navigator::new(topology, capacity)`.
- Every search answers with a **result**: status, cells of the whole way,
  cost, cells expanded, whether the node capacity was too small.
- **Status**: running (budget used up, continue later), found, partial (goal
  unreachable, way to the closest reachable cell), unreachable, done (area
  or field complete).
- **Ways always begin with the start** and end at the last cell, as every
  cell, as turning points or smoothed.
- **Budget 0 means no limit.**

**Rules of thumb:**

- **When in doubt, A\*.** On a 128 x 96 map a search takes well under a
  millisecond. After a map change, searching again is almost always fine.
- **Flow fields** pay off from roughly 5–10 walkers with the same goal.
- **D\* Lite** pays off only when the map changes *often* and *near the
  walker*. It needs its own memory per walker.
- **Formations** work on whole cells: every member gets its own cell.

**Hex grids:** C `rules.grid = SX_GRID_HEX_*`, JS `{ hex: HEX.oddR }`, Rust
`Topology::Hex`. The step callback is asked with `dx, dy` = offset to the
neighbour cell. Formation slots: "forward" is the walking direction,
"right" the direction 60° clockwise of it; `(0, 1)` front right, `(-1, 0)`
behind, `(-1, 1)` back right. Wedge: `Formation::wedge_hex` /
`FORMATION.wedgeHex`.

## 2. C

Build `cargo build --release -p sagax-ffi`; link `target/release/sagax.dll`
(Windows) or `libsagax.so` / `libsagax.dylib`. Header:
`crates/sagax-ffi/include/sagax.h`. A complete program:
`examples/c/quickstart.c`.

### Basics

```c
sx_rules rules = {0};
rules.grid = SX_GRID_SQUARE;            /* or SX_GRID_HEX_ODD_R ... */
rules.cost_straight = 10;  rules.cost_diagonal = 14;
rules.node_capacity = 32768;
sx_nav *nav = sx_nav_create(&rules);    /* once, reuse; 0: invalid rules */

sx_map map = {0};
map.step = my_step;              /* (ctx, x, y, level, dx, dy) -> level or -1 */
map.start_step = my_start_step;  /* optional: moves off the start cell */
map.move_cost = my_extra_cost;   /* optional: extra cost, -1 forbids */
map.context = &walker;

sx_point start = { x, y, z }, goal = { gx, gy, SX_ANY_LEVEL };
sx_point pts[64]; sx_result r;
int n = sx_nav_find_path(nav, &map, start, goal, 4000, SX_OUT_TURNS, pts, 64, &r);
/* n < 0: error. pts[0] is the start, pts[n-1] the goal or (r.status ==
 * SX_PARTIAL) the closest reachable cell. r.cells counts the whole way,
 * even if pts was too small. */
```

- Return values: entries written (≥ 0) or an error (< 0). How a search
  ended is only in `r.status`.
- Outputs: `SX_OUT_CELLS` (every cell), `SX_OUT_TURNS` (turning points),
  `SX_OUT_SMOOTH` (smoothed). With `start_step` the first cell after the
  start is always kept (the walker may not stand in the middle of its cell).
- `sx_nav_way` writes the same way again in another output, without a new
  search.
- One handle per thread. No allocation during a search while
  `node_capacity` suffices (`r.node_limit_hit` reports it).

### Over several frames

```c
n = sx_nav_find_path(nav, &map, start, goal, 500, SX_OUT_TURNS, pts, 64, &r);
/* next frame, while r.status == SX_RUNNING and the map is unchanged: */
n = sx_nav_continue(nav, &map, 500, pts, NULL, 64, &r);
```

Many small steps end exactly like one big one. On `SX_RUNNING`, `pts`
already holds the best way so far.

### Reachable area

```c
sx_point cells[2048]; int32_t costs[2048];
n = sx_nav_reachable(nav, &map, start, 120, 0, cells, costs, 2048, &r);
n = sx_nav_path_to(nav, &map, cells[7], SX_OUT_CELLS, pts, 64, &r);
/* r.status == SX_UNREACHABLE: that cell was not in the area */
```

### Batched callback (optional)

`map.steps` answers all neighbours of a cell in one call (`offsets` holds
`dx, dy` pairs, answers go to `levels`). It saves time only when a single
`step` answer is expensive in your game. Its answers must match `step`;
`step` stays required (smoothing and formations ask single moves).

### Smoothing

Output `SX_OUT_SMOOTH`. A straight line is used only where every cell it
touches can be entered with `step` on the same level and `move_cost` is at
most `rules.smooth_max_extra` (default 0). Stairs, corners and costly cells
stay corners. `step` is asked between cell centres: if your walker does not
move through centres, check its own clearance.

### Formation

```c
const int32_t slots[] = { 0, 1,  -1, 0,  -1, 1 };  /* (forward, right) per member */
enum { MAX = 512 };
sx_point table[4 * MAX];                            /* (members + 1) rows */
int cols = sx_nav_formation(nav, &map, slots, 3, table, MAX);
/* table[0*MAX + i]: the leader's cell, table[k*MAX + i]: member k's target
 * while the leader stands on cell i */
```

Works on the last way of `nav`. `slots = NULL` with 3 members is the 2 × 2
block. At narrow places members fall back onto the leader's trail and
return afterwards.

### Flow field

```c
sx_flow *flow = sx_flow_create(&rules);           /* its own handle */
sx_point goals[] = { { gx, gy, z } };
sx_flow_build(flow, &map, goals, 1, 100000, &r);  /* once per goal or change */
sx_point at[8], next[8]; int32_t cost[8];
sx_flow_next(flow, &map, at, 8, next, cost);      /* all walkers in one call */
```

The field is built *backwards* from the goals. Costs are exact when every
move costs the same both ways. With one-way moves or zones that cost extra
to leave they are approximate; the next steps still use only legal moves.

### D* Lite

```c
sx_dstar *d = sx_dstar_create(&rules);            /* one per walker */
sx_dstar_begin(d, start, goal);                   /* goal.level >= 0 */
n = sx_dstar_compute(d, &map, 0, SX_OUT_TURNS, pts, 64, &r);

sx_dstar_move_start(d, here);                     /* after every cell walked */
sx_point changed[] = { { door_x, door_y, door_z } };
sx_dstar_cells_changed(d, &map, changed, 1);      /* step already answers anew */
n = sx_dstar_compute(d, &map, 0, SX_OUT_TURNS, pts, 64, &r);
```

Needs: if a move from A to B is possible, so is the move from B to A
(costs may differ). `start_step` is not used. If the goal is unreachable,
`compute` works through everything reachable from the goal, so set a budget
and spread it over frames.

## 3. JavaScript

```js
import { loadSagax, HEX, FORMATION } from './sagax.mjs';
const lib = await loadSagax(wasmUrlOrBytes);     // URL in the browser, bytes in Node

const grid = lib.grid(30, 20);                    // square, diagonal step costs 1
const hexGrid = lib.grid(30, 20, { hex: HEX.oddR, step: 1 });
grid.block(5, 5); grid.addWall(3, 3, 1, 0);
grid.addZone(4, 4, 0); grid.setLeaveCost(4);      // leaving zone 0 costs 4
grid.setCosts(uint32Array);                       // whole map at once

const way = grid.findPath(a, b, { output: 'turns', budget: 0 });
// way.status: 'found' | 'partial' | 'running' (then grid.continue()) | 'unreachable'
const corners = grid.way('smooth');               // the same way, smoothed
const area = grid.reachable(a, 6).cells;          // [{x, y, cost}]
const back = grid.pathTo(9, 4);                   // way to one of them, or null
const { leader, members } = grid.formation(FORMATION.wedge(3));
grid.buildFlow([{ x: 3, y: 4 }]);
const steps = grid.flowNext(walkers);             // [{x, y, cost}] per walker
grid.dstarBegin(a, b); grid.dstarCompute();       // {status: 'found', cells, ...}
grid.free();
```

`js/foundry-adapter.mjs` builds a grid from a Foundry VTT scene (walls,
tokens, enemy zones of control). Flow fields and D* Lite do not suit zones
with a leave cost (one-way costs); use `findPath` / `reachable` there.

## 4. Rust

```rust
use sagax_nav::grid::{Cell, DenseGrid, GridRules};
use sagax_nav::nav::{Navigator, Output, Status};
use sagax_nav::topology::Topology;

let mut map = DenseGrid::new(64, 64);
let mut nav = Navigator::new(Topology::Square(GridRules::default()), 4096);
let r = nav.find_path(&mut map, Cell::new(0, 0, 0), Cell::new(40, 9, 0), true, usize::MAX);
assert_eq!(r.status, Some(Status::Found));
let corners = nav.output(&mut map, Output::Smooth);
```

`Navigator` bundles search, `resume`, reachable areas (`reachable`, `area`,
`path_to`), outputs and formations. Underneath: `Search` (A*/Dijkstra with a
budget), `flow::FlowField`, `dstar::DStarLite`, `smooth`, `formation` and
`topology::Topology` (square or hex at run time). Your own maps implement
`grid::CellMap` (square), `hex::HexMap` (hex) or `Graph` (anything); a
`&mut` map is a map too.

## 5. Pitfalls

- Costs are integers ≥ 0. `move_cost` is an *extra* on top of the step cost.
- `SX_ANY_LEVEL` (-1) as goal level means any level; D* Lite needs an exact
  level.
- `SX_PARTIAL` is the best partial way, not an error.
- Lists are cut when the buffer is too small; `r.cells` still counts all.
- Flow fields and D* Lite assume moves that work both ways.
- Load the library at run time, check `sx_api_version()`, and fall back to
  your own code if it is missing or too old.
