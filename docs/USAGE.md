# Guide

What to use for which case, how to call it, and where the pitfalls are. New
here? [QUICKSTART.md](QUICKSTART.md) does a path, a decision and a squad in
five minutes. The exact contracts are in `crates/sagax-ffi/include/sagax.h`
(C), `js/sagax.mjs` (JavaScript) and the Rust docs (`cargo doc -p sagax
--open`; the `sagax` crate and its `prelude` bring the common names, the
parts are `sagax::nav`, `sagax::decide`, `sagax::perception` and
`sagax::debug`).

## Conventions

The same rules hold in Rust, C and JavaScript, and every section of this
guide shows its calls in that order: Rust, C, JavaScript. Names that
changed in 0.9 and 0.10 are listed in the migration tables of the
[CHANGELOG](../CHANGELOG.md); the deprecated aliases of 0.9 are gone in
0.10.

### Positions

A position is `[x, y, z]` (Rust `Pos = [i32; 3]`; `z` is the level of a
cell, `Cell::from(pos)`, `cell.pos()`). C passes it as `int32_t[3]`;
`sx_point_to_pos` and `sx_pos_to_point` convert from and to `sx_point`.
JavaScript takes `[x, y, z]`, `{x, y, z}` and `{x, y, level}` wherever a
position goes into an AI block and into the grid calls (`contains`,
`setCost`, `block`, `addWall`, `setZones`, `addZone`, `pathTo`, `flowCost`
take `x, y` or one position); a position with both `z` and `level` that
disagree throws `RangeError`. Cells that JavaScript returns from a search
are `{x, y, level}` (areas and flow answers `{x, y, level, cost}`).

**Hex warning:** on a hex grid the AI blocks (memory, groups) work with
*axial* coordinates `(q, r)`, not with the columns and rows of your grid.
See Hex, next.

A level outside `0 .. levels` is off the grid: a `DenseGrid` or `sx_grid`
treats such a cell as blocked and a search through it finds nothing (before
0.9 a one-level `DenseGrid` ignored the level).

### Hex

- The **navigation** (`Navigator`, `DenseGrid`, `sx_grid`, `sx_nav_*`, the
  JavaScript `grid`) works in the offset cells of your map (`x` the column,
  `y` the row, in the layout you gave: odd or even rows shifted right for
  pointy tops, odd or even columns shifted down for flat tops).
- The **AI blocks** (`Knowledge`, `Areas`, `route`, `Squad`; `Metric::Hex`)
  work with axial `(q, r)` in `x`, `y`. The hex metric measures the axial
  distance.
- **The crew converts for you.** With `CrewConfig::hex_layout` (C
  `sx_crew_config.hex_grid`, JavaScript `lib.crew({ hex: 'oddR' })`; the
  squad's metric must then be hex) the crew takes and returns offset
  coordinates: member positions, spots, facts, danger reports, order and
  goal positions. The squad and the keepers inside hold axial ones; calls on
  `crew.squad_mut()` / `sx_crew_squad` and `crew.keeper` use axial
  coordinates.
- **Without the crew, convert at the edge:** `HexLayout::to_axial_pos` and
  `from_axial_pos` (`to_axial(x, y)` and `from_axial(q, r)` for two
  numbers), C `sx_hex_to_axial` and `sx_hex_from_axial`, JavaScript
  `lib.hex.toAxial(pos, 'oddR')` and `lib.hex.fromAxial(pos, 'oddR')`. All
  keep `z`.
- **Walls and links** on a hex grid take real neighbours of that layout:
  which `(dx, dy)` offsets they are depends on the row or column
  (`grid.hexNeighbors(x, y)`, `addHexWall(x, y, dir)`). A square diagonal
  that is not a hex neighbour is accepted by `add_wall` and `link` but is
  never walked, so it does nothing useful. The wall bits of a `DenseGrid`
  are offset bits (bit `i` is `DIRECTIONS[i]`) on square and hex alike since
  0.9; JavaScript `walls()` and `setWalls()` throw on a hex grid, use
  `addWall` / `addHexWall`.

### Time and seed

`now` is the game's own count (frames, turns, seconds; it may wrap around,
ages are wrapping differences). It comes right after the handle and after the
ids that name what the call addresses; payloads come after `now`; `seed` is
always the last input, out parameters follow it. So
`known.learn(now, fact)`, `outbox.post(now, msg, delay, cooldown)`,
`areas.add(now, area)`, `decider.record(now, candidate)`,
`keeper.set(now, goal)`, `squad.observe(now, fact)`,
`crew.reinstate(id, now)`, `timers.set(id, now, duration)`. The same order
in C: `sx_knowledge_learn(k, now, fact, evicted)`,
`sx_outbox_post(o, now, msg, delay, cooldown)`. A call written in the old
order (fact first, then time) does not compile in Rust and C, and throws in
JavaScript (`TypeError`: a fact is not a time).

There are two groups of exceptions, on purpose:

- Calls where moving the time would swap two numbers of the same type and
  compile silently: `Alertness::stimulus(amount, now, cfg)`,
  `Progress::start(value, now)` and `Progress::update(value, now, min_gain)`
  (C `sx_alertness_stimulus`, `sx_progress_start`, `sx_progress_update`).
- Calls that take the tick *a message arrives at* instead of the time of the
  call; the payload comes first, then the arrival: `Inbox::push(msg,
  arrive_at)`, `Squad::report(msg, arrive_at)`, `Squad::report_danger(sender,
  centre, shooter, arrive_at)` and `Crew::report_danger(...)` (C
  `sx_inbox_push`, `sx_squad_report`, `sx_squad_report_danger`,
  `sx_crew_report_danger`; JavaScript `inbox.push(msg, arriveAt)`,
  `squad.report(msg, arriveAt)`, `squad.reportDanger(report, arriveAt)`,
  `crew.reportDanger(report, arriveAt)`). `arrive_at` is a tick in the
  future, not "now". `Squad::report_arrives_at(id, now)` tells which one.

### C

- **Return values.** A yes/no question returns 1 or 0. A count or an index is
  returned directly; a function with nothing to report returns 0. A scalar
  answer is returned directly when it fits (`int64_t` for the times and costs
  that do not fit `int32_t`: `sx_timers_left`, `sx_alertness_value`,
  `sx_areas_cost`), structs come through out parameters. A function that
  writes a list returns how many entries it wrote and, where the full count
  matters, takes an optional `uint32_t *total` (`sx_squad_targets`,
  `sx_crew_tick`). Where a function may drop a stored item
  (`sx_knowledge_learn`, `sx_knowledge_merge`), the return value is how many
  were dropped (0 or 1). `sx_inbox_push` answers yes/no (1 stored, 0
  refused); `sx_outbox_post` returns `SX_POSTED` (0) or the reason it was
  refused; `sx_squad_report_danger` and `sx_crew_report_danger` return 1 if
  stored and 0 when the danger queue is full.
- **Errors** are negative and only the `SX_ERROR_*` constants (a null
  pointer, a number out of range). The header and this guide never say a bare
  "-1" for an error.
- **Enum parameters are `uint32_t`:** `SX_GRID_*`, `SX_OUT_*`, `SX_METRIC_*`,
  `SX_EVICT_*`, `SX_ROUTE_*`, `SX_SINE_*`, `SX_COMBINE_*`, `SX_PRESET_*`,
  `SX_SHAPE_*`. "All options" and "follow the decider" of
  `sx_decider_set_combine` are `SX_ALL_OPTIONS` and `SX_COMBINE_FOLLOW` (both
  `0xFFFFFFFFu`, what -1 was).
- **Lifecycle.** Every handle has `_new` and `_free`: `sx_nav_new` /
  `sx_nav_free`, `sx_flow_new` / `sx_flow_free`, `sx_dstar_new` /
  `sx_dstar_free`, `sx_grid_new` / `sx_grid_free`, `sx_crew_new` /
  `sx_crew_free`. The old `_create` / `_destroy` names are gone
  (removed in 0.10).
- **Layout checks.** At the end of `sagax.h`, a C11 compiler checks every
  value struct (`sizeof` and key field offsets) against the numbers the
  library was built with, so a header and a library that disagree fail to
  compile. `sx_map` and `sx_areas_cost_ctx` hold pointers and are not
  checked.
- **Threads.** One handle per thread. One search at a time per `sx_grid`: a
  search reads the grid through its map and takes it as unchanged, so do not
  share one `sx_grid` between threads while searches run, and do not change
  it from a callback.

### JavaScript

- **Errors.** A number that is not an integer, or out of its range, throws
  `RangeError`; a value of the wrong type throws `TypeError`. A position of
  the wrong shape is a `RangeError` (one validation path for all positions).
  Nothing is coerced quietly: fractions, `-1` for a time, a zone above 31, a
  budget of `2.5` all throw. `reachable(cell, r)` and `leaveCost` take whole
  cells and whole costs. Time arguments are validated (`0 .. 2^32 - 1`).
- **Handles.** Every handle class has `free()` and, where the runtime has it,
  `[Symbol.dispose]` (so `using grid = lib.grid(...)` works). A call after
  `free()` throws `Error('Sagax: used after free()')`. A handle lent by
  another (a crew's squad and keepers) throws too once it is stale.
- **String constants** instead of numbers: roles `'ranged'` and `'area'`, tags
  `'cover'`, `'high'`, `'hidden'`, `'choke'`, a fact `source` of `'own'`,
  `'heard'` or `'told'`, a report kind of `'enemy'`, `'arrived'` or
  `'goalBroken'` (numbers still work for your own bits).
- **Value classes** (`Progress`, `Interval`, `Alertness`, `lib.arrival`) run
  in the WebAssembly module now and need `loadSagax` to have run first.
- **Seeds** may be a number or a `BigInt` (full 64 bits), also for `decide`.

### Rust

- Builders that can fail return `Result` (a capacity of 0 is an error) or
  `bool`; the ones that answer `false` for an unknown id or a full table are
  `#[must_use]`, so a thrown-away answer warns.
- `Budget` is `Unlimited` or `Cells(NonZeroUsize)`; `Budget::UNLIMITED` and
  `NonZeroUsize::into()` are the two ways to write one. A search call takes
  `impl Into<Budget>`; a plain `usize` no longer compiles.
- `GridRules` has no `Default`: pick `GridRules::preset(Preset::Octile)`
  (straight 10, diagonal 14) or `Preset::Tabletop` (every step 1).
- `use sagax::prelude::*;` is enough for a path, a decision and a crew.

## 1. Which building block?

| The game asks | Building block | C | JavaScript |
|---|---|---|---|
| One walker, one goal (or the nearest of several) | A* | `sx_nav_find_path` | `grid.findPath` |
| Get away: off the map, or away from threats | Exits as goals, or flee | `sx_nav_find_path` / `sx_nav_flee` | `grid.findPath` / `grid.flee` |
| Where can I get with X movement? | Reachable area | `sx_nav_reachable` + `sx_nav_path_to` | `grid.reachable` + `grid.pathTo` |
| A way without staircase steps | Smoothing | output `SX_OUT_SMOOTH` | `output: 'smooth'` |
| A group follows its leader, one cell each | Formation | `sx_nav_formation` | `grid.formation` |
| Many walkers, same goals | Flow field | `sx_flow_*` | `grid.buildFlow` + `grid.flowNext` |
| The map changes while a walker is on its way | D* Lite | `sx_dstar_*` | `grid.dstar*` |
| What should I do, and to whom? | Decisions (utility AI) | `sx_decider_*` | `lib.decider()` |
| Patrol, chase, search: behaviour over time | State machine | `sx_machine_*` | `lib.machine()` |
| Who did I see, where, how long ago; where is it dangerous | Memory of the world | `sx_knowledge_*`, `sx_areas_*`, ... | `lib.knowledge()`, `lib.areas()` |
| What can this character see and hear, and how soon does it notice? | Perception | `sx_perception_*` | not yet |
| Several characters act together: flank, ambush, advance in line | Group tactics | `sx_crew_*` (the loop), `sx_squad_*`, `sx_keeper_*`, `sx_gather` | `lib.crew()` (the loop), `lib.squad()`, `lib.tactics`, `lib.keeper()` |
| What one character does over time: patrol, guard, chase, and what when the plan breaks | Missions | `sx_mission_*` | not yet |

Everything works on square and hex grids and in every interface.

**The same shape everywhere:**

- **Rules** (square or hex, costs, memory) are given once when the handle is
  made: Rust `Navigator::new(topology, capacity)` with `GridRules::preset`, C
  `sx_rules` (`sx_rules_default` fills it from a preset), JS
  `lib.grid(w, h, { preset })`. There is no default cost: every game picks
  `Octile` (straight 10, diagonal 14) or `Tabletop` (every step 1).
- Every search answers with a **result**: status, cells of the whole way,
  cost, cells expanded, whether the node capacity was too small.
- **Status**: idle (nothing to continue), running (budget used up, continue
  later), found, partial (goal unreachable, way to the closest reachable
  cell), unreachable, done (area or field complete).
- **Ways always begin with the start** and end at the last cell, as every
  cell, as turning points or smoothed.
- **Budget 0 means no limit** (Rust `Budget::UNLIMITED`).

**Rules of thumb:**

- **When in doubt, A\*.** On a 128 x 96 map a search takes well under a
  millisecond. After a map change, searching again is almost always fine.
- **Flow fields** pay off from roughly 5–10 walkers with the same goal.
- **D\* Lite** pays off only when the map changes *often* and *near the
  walker*. It needs its own memory per walker.
- **Formations** work on whole cells: every member gets its own cell.

**Hex grids:** Rust `Topology::hex(layout, preset)`, C `rules.grid =
SX_GRID_HEX_*`, JS `{ hex: HEX.oddR, preset }`. The step callback is asked
with `dx, dy` = offset to the neighbour cell. Positions of the AI blocks are
axial there: see Hex in the Conventions. Formation slots: "forward" is the walking direction,
"right" the direction 60° clockwise of it; `(0, 1)` front right, `(-1, 0)`
behind, `(-1, 1)` back right. Wedge: `Formation::wedge_hex` /
`FORMATION.wedgeHex`.

## 2. Rust

```rust
use sagax::prelude::*;

let mut map = DenseGrid::new(64, 64);
map.set(10, 5, DenseGrid::BLOCKED);
map.add_wall(3, 3, 1, 0);                    // a wall between two cells, both ways
let mut nav = Navigator::new(Topology::Square(GridRules::preset(Preset::Octile)), 4096);
let start = Cell::new(0, 0, 0);
let goals = [Cell::new(40, 9, ANY_LEVEL)];   // ANY_LEVEL: any level of that cell
let report = nav.find_path(&mut map, start, &goals, Budget::UNLIMITED);
assert_eq!(report.status, NavStatus::Found);
let corners = nav.output(&mut map, Output::Smooth);
```

`Navigator` bundles search (to one or several goals), `continue_search`,
reachable areas (`reachable`, `area`, `path_to`), `flee`, outputs and
formations. `Report::status` is a `Status`: `Idle` (no search to go on with;
the default), `Running`, `Found`, `Partial`, `Unreachable`, `Done`. Underneath:
`Search` (A*/Dijkstra with a budget), `flow::FlowField`, `dstar::DStarLite`,
`smooth`, `formation` and `topology::Topology` (square or hex at run time).
Your own maps implement `grid::CellMap` (one trait for square and hex; the
step callback is asked with `dx, dy` = the offset to the neighbour cell) or
`Graph` (anything); a `&mut` map is a map too. `DenseGrid` is a ready map
with levels (`with_levels`, `get_at`, `set_at`, `add_wall_at`, `link`).

Over several frames (`Budget::from(NonZeroUsize)`; many small steps end
exactly like one big one):

```rust
use std::num::NonZeroUsize;

let slice = Budget::from(NonZeroUsize::new(500).unwrap());
let mut r = nav.find_path(&mut map, start, &goals, slice);
while r.status == NavStatus::Running {
    r = nav.continue_search(&mut map, slice);   // next frame, map unchanged
}
```

Fleeing and the reachable area:

```rust
let threats = [Cell::new(5, 5, 0)];
let r = nav.flee(&mut map, start, &threats, 60);       // within 60 cost
let r = nav.reachable(&mut map, start, 120, Budget::UNLIMITED);
let cells: Vec<_> = nav.area().collect();              // (Cell, cost)
let (cell, _cost) = cells[7];
let cost = nav.path_to(cell);                          // None: not in the area
```

Levels, stairs and hex:

```rust
let mut floors = DenseGrid::with_levels(16, 16, 2);
floors.link(Cell::new(4, 4, 0), Cell::new(5, 4, 1));   // a stair: both ways
floors.set_at(Cell::new(6, 6, 1), DenseGrid::BLOCKED);
let hex_nav = Navigator::new(Topology::hex(HexLayout::OddR, Preset::Tabletop), 400);
```

## 3. C

Build `cargo build --release -p sagax-ffi`; link `target/release/sagax.dll`
(Windows) or `libsagax.so` / `libsagax.dylib`. Header:
`crates/sagax-ffi/include/sagax.h`. A complete program:
`examples/c/quickstart.c`.

### Basics

To see why a search did what it did: `sx_nav_set_trace`, or in Rust `find_path_traced` (see
[Debugging and logging](#11-debugging-and-logging)).

```c
sx_rules rules;
sx_rules_default(SX_GRID_SQUARE, SX_PRESET_OCTILE, &rules);  /* or SX_GRID_HEX_ODD_R ... */
rules.node_capacity = 32768;           /* cells one search may touch (the default) */
sx_nav *nav = sx_nav_new(&rules);      /* once, reuse; 0: invalid rules */

sx_map map = {0};
map.step = my_step;              /* (ctx, x, y, level, dx, dy) -> level or -1 */
map.start_step = my_start_step;  /* optional: moves off the start cell */
map.move_cost = my_extra_cost;   /* optional: extra cost, -1 forbids */
map.context = &walker;

sx_point start = { x, y, z }, goal = { gx, gy, SX_ANY_LEVEL };
sx_point pts[64]; sx_result r;
int n = sx_nav_find_path(nav, &map, start, &goal, 1, 4000, SX_OUT_TURNS, pts, 64, &r);
/* n < 0: error. pts[0] is the start, pts[n-1] the goal or (r.status ==
 * SX_PARTIAL) the closest reachable cell. r.cells counts the whole way,
 * even if pts was too small. */
```

- `sx_rules_default(grid, preset, &rules)` fills the rules with the costs of
  a preset (`SX_PRESET_OCTILE`: straight 10, diagonal 14; `SX_PRESET_TABLETOP`:
  every step 1; there is no preset 0); you can still set the fields of
  `sx_rules` yourself.
- Return values: entries written (≥ 0) or an error (< 0). How a search
  ended is only in `r.status`.
- Outputs: `SX_OUT_CELLS` (every cell), `SX_OUT_TURNS` (turning points),
  `SX_OUT_SMOOTH` (smoothed). With `start_step` the first cell after the
  start is always kept (the walker may not stand in the middle of its cell).
- `sx_nav_way` writes the same way again in another output, without a new
  search.
- One handle per thread. No allocation during a search while
  `node_capacity` suffices (`r.node_limit_hit` reports it).

### A ready map: `sx_grid`

A game without a map structure of its own can use `sx_grid`: cells with an
extra cost, walls and levels, all in the library.

```c
sx_grid *grid = sx_grid_new(24, 12, 2);                 /* width, height, levels */
sx_grid_set(grid, (sx_point){12, 3, 0}, -1);            /* cost < 0 blocks a cell */
sx_grid_set(grid, (sx_point){6, 6, 0}, 5);              /* 5 extra to enter */
sx_grid_add_wall(grid, (sx_point){3, 3, 0}, 1, 0);      /* between (3,3) and (4,3), both ways */
sx_grid_link(grid, (sx_point){4, 4, 0}, (sx_point){5, 4, 1});  /* a stair: arrive on level 1 */
sx_map map;
sx_grid_map(grid, &map);                                /* valid while the grid lives */
/* ... sx_nav_find_path(nav, &map, ...) ... */
sx_grid_free(grid);
```

`examples/c/squad.c` builds its map this way. A move keeps its level unless
a link sends it to another one; positions are `sx_point` everywhere. A
cell off the grid (also a level outside `0 .. levels - 1`) gives
`SX_ERROR_ARGUMENT`. On a hex grid `link` and `sx_grid_add_wall` take only
real hex neighbours (see Hex in the Conventions). One search at a time per
`sx_grid`.

### Over several frames

```c
n = sx_nav_find_path(nav, &map, start, &goal, 1, 500, SX_OUT_TURNS, pts, 64, &r);
/* next frame, while r.status == SX_RUNNING and the map is unchanged: */
n = sx_nav_continue(nav, &map, 500, pts, NULL, 64, &r);
```

Many small steps end exactly like one big one. On `SX_RUNNING`, `pts`
already holds the best way so far. `sx_nav_continue` before any search gives
`r.status == SX_IDLE`.

Several goals: `sx_point exits[] = {...}; sx_nav_find_path(nav, &map, start,
exits, n, ...)` gives the way to the cheapest one, in one search.

### Fleeing

```c
sx_point threats[] = { { ox, oy, oz } };
n = sx_nav_flee(nav, &map, start, threats, 1, move_left, SX_OUT_TURNS, pts, 64, &r);
/* SX_UNREACHABLE: nothing within move_left is farther from the threats */
```

Picks the cell, within the walker's budget, that the nearest threat needs
the longest walk to reach. If the map has exits or a refuge, fleeing *there*
is usually more sensible: pass them as goals to `sx_nav_find_path`. Give cells
near the threats an extra `move_cost` so the way does not run past them.

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
sx_flow *flow = sx_flow_new(&rules);           /* its own handle */
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
sx_dstar *d = sx_dstar_new(&rules);            /* one per walker */
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

## 4. JavaScript

```js
import { loadSagax, HEX, FORMATION } from './sagax.mjs';
const lib = await loadSagax(wasmUrlOrBytes);     // URL in the browser, bytes in Node

const grid = lib.grid(30, 20, { preset: 'tabletop' });   // square, every step costs 1
const octile = lib.grid(30, 20, { preset: 'octile' });   // straight 10, diagonal 14
const hexGrid = lib.grid(30, 20, { hex: HEX.oddR, preset: 'tabletop' });
const floors = lib.grid(30, 20, { preset: 'tabletop', levels: 2 });
grid.block(5, 5); grid.addWall(3, 3, 1, 0);
grid.addZone(4, 4, 0); grid.setLeaveCost(4);      // leaving zone 0 costs 4
grid.setCosts(uint32Array);                       // whole map at once
floors.link({ x: 4, y: 4, level: 0 }, { x: 5, y: 4, level: 1 });   // a stair, both ways

const way = grid.findPath(a, b, { output: 'turns', budget: 0 });
// way.status: 'found' | 'partial' | 'running' (then grid.continue()) | 'unreachable' | 'idle'
const corners = grid.way('smooth');               // the same way, smoothed
const exit = grid.findPath(a, [e1, e2, e3]);      // to the nearest of several goals
const away = grid.flee(a, [enemy], 6);            // away from threats, within 6
const area = grid.reachable(a, 6).cells;          // [{x, y, level, cost}]
const back = grid.pathTo(area[3]);                // way to one of them, or null
const { leader, members } = grid.formation(FORMATION.wedge(3));
grid.buildFlow([{ x: 3, y: 4 }]);
const steps = grid.flowNext(walkers);             // [{x, y, level, cost}] per walker
grid.dstarBegin(a, b); grid.dstarCompute();       // {status: 'found', cells, ...}
grid.free();
```

`lib.grid(w, h)` without a `preset` (or explicit `straight` and `diagonal`
costs; on hex `step`) throws `TypeError`: a grid has no default cost.
Explicit values override the preset. Cells in a way are `{x, y, level}`; any
position form of the Conventions goes in (`{x, y}`, `{x, y, level}`,
`[x, y, z]`). `budget` 0 is no limit. `reachable(cell, r)`, `leaveCost`
and every cost are whole numbers: a fraction throws.

`js/foundry-adapter.mjs` builds a grid from a Foundry VTT scene (walls,
tokens, enemy zones of control; the reach radius and the leave cost are
rounded down to whole cells and costs). Flow fields and D* Lite do not suit
zones with a leave cost (one-way costs); use `findPath` / `reachable` there.

## 5. Ways as input for decisions

Game AI often weighs options by ways, not by straight-line distance:

- **Walking distance to a target:** `find_path` (budget 0) and `r.cost`.
  For many targets at once: one `reachable` from the walker, then the cost
  of each target cell from the area (no entry: out of reach).
- **Reachable this turn:** `reachable` with the movement left as `max_cost`;
  a cell in the area can be reached now, its cost is what it takes.
- **Attack positions:** the goals of `find_path` can be every cell next to
  the target; the way ends at the cheapest.
- **Escape:** exits as goals, or `flee`; `r.status` tells whether there is a
  way out at all.
- **Danger:** zones to avoid as extra `move_cost`; then the cost already
  counts the risk. Remembered danger areas feed the search with
  `Areas::cell_cost` (section 9, "Areas to path costs").
- **Movement budget with zones:** with a leave cost, `cost` mixes walking
  and penalty. JavaScript ways also give `steps` (walking and terrain only)
  and `zonesLeft` (zones left, e.g. attacks of opportunity provoked), so
  `cost = steps + zonesLeft * leaveCost`. Check `steps` against the movement
  left; Rust: `DenseGrid::zones_left(path)`. In C the penalty is part of your
  own `move_cost`, so you already know it. For "where can I get this turn"
  without zones, run `reachable` with leave cost 0; then check single targets
  with `findPath` and zones on.

## 6. Decisions (utility AI)

`sagax-decide` chooses what a character does. You list **options** (attack,
flee, heal ...). Each has a **weight** and **considerations**: one raw number
the game measured (distance, own health, enemies near ...) turned into a
factor 0..1 by a **curve**. A **candidate** is one pair of option and target;
the game fills one row of raw inputs per candidate. The score is the product
of the factors (one 0 rules the candidate out), compensated for the number of
factors so options with many considerations are not punished, times the
weight.

- **Curves:** `t = (input - min) / (max - min)`, clamped to 0..1. `linear` t,
  `inverse` 1 - t, `quadratic` t², `inverseQuadratic` (1 - t)², `cubic` t³
  (only the top counts), `root` √t (early values already count), `sCurve`
  3t² - 2t³ (a soft threshold), `inverseSCurve`, `step` 1 if input >= min,
  `inverseStep` 1 if input < min.
- **Choice:** `best` (equal scores: the smaller index wins), `weighted`
  (by chance in proportion to the score) or `uniform` (each equally likely),
  both among the `top` best (0: all).
- **Deterministic:** all integer, 1.0 = 65535. No global random numbers; the
  seed comes from you (e.g. character index + turn). Same inputs and seed,
  same choice, in Rust, C and JavaScript.
- **Inputs from ways:** see section 5 (walking cost, reachable this turn,
  flee). The target number is yours (token, cell index, weapon); Sagax only
  carries it along.

Rust:

```rust
let mut d = Decider::new();
let a = d.add_option(ONE);
let b = d.add_option(ONE);
assert!(d.add_consideration(a, Consideration::new(0, Curve::Linear, 0, 10)));
assert!(d.add_consideration(b, Consideration::new(0, Curve::Linear, 0, 10)));
let candidates = [Candidate::new(a, 0), Candidate::new(b, 0)];
let rows = [[2], [8]];                       // one row of raw inputs per candidate
let choice = d.choose(&candidates, &rows, Pick::Best, 0).unwrap();
assert!(matches!(choice, Some(Choice { candidate: 1, .. })));
```

`choose` takes the rows as they are and returns `Err(DecideError::RowLengths)`
for rows of different or wrong length, `Ok(None)` if every score is 0. Under
it, `decide` / `decide_traced` (any `Sink<DecideTrace>`, see [Debugging and logging](#11-debugging-and-logging)) take one
flat slice of inputs and a stride, and write the scores.

C:

```c
sx_decider *d = sx_decider_new();
int32_t attack = sx_decider_add_option(d, SX_ONE);
sx_consideration near = {0, SX_CURVE_INVERSE, 1, 10};
sx_decider_add_consideration(d, (uint32_t)attack, &near);
sx_candidate cands[2] = {{(uint32_t)attack, 7}, {(uint32_t)attack, 8}};
int32_t inputs[2] = {6, 2};                 /* one row of stride 1 per candidate */
sx_choice choice;
if (sx_decider_decide(d, cands, 2, inputs, 1, SX_PICK_BEST, 0, seed, NULL, &choice) == 1)
	/* cands[choice.candidate] */;
sx_decider_set_trace(d, on_trace, ctx);     /* SX_DECIDE_TRACE_FACTOR / SCORED / RANKED / CHOSEN */
sx_decider_free(d);
```

JavaScript:

```js
const d = lib.decider();
const attack = d.addOption(1, [
  { input: 0, curve: 'inverse', min: 1, max: 10 },   // distance: near is better
  { input: 1, curve: 'linear', min: 0, max: 100 },   // own health
]);
const flee = d.addOption(1, [{ input: 1, curve: 'inverseQuadratic', min: 0, max: 100 }]);
const choice = d.decide([
  { option: attack, target: goblin, inputs: [3, hp] },
  { option: attack, target: orc, inputs: [6, hp] },
  { option: flee, target: 0, inputs: [0, hp] },
], { pick: 'weighted', top: 2, seed: actorIndex * 1000 + round });
// choice: {index, option, target, score, scores} or null if every score is 0
d.setTrace(true); /* ... */ d.takeTrace();   // [{kind: 'factor'|'scored'|'chosen', ...}]
d.free();
```

### Putting factors together

Multiplying is the default, and it has a known weakness: many good but not
perfect factors drag a score down. Five factors of 0.9 give 0.59 plainly,
0.78 with the compensation; a single factor of 0.8 then wins. So the way
factors are put together can be chosen, for the whole decider
(`setCombine('geometricMean')`) and per option (`addOption(w, list,
{ combine: 'minimum' })`, which wins over the decider's).

What each one does with typical situations
(`cargo run -p sagax-decide --example compare`):

| Situation | product | geometricMean | minimum | average | median |
|---|---|---|---|---|---|
| five times 0.9 | 0.784 | 0.900 | 0.900 | 0.900 | 0.900 |
| one 0.8 | 0.800 | 0.800 | 0.800 | 0.800 | 0.800 |
| 1, 1, 1, 0.1 | 0.167 | 0.562 | 0.100 | 0.775 | 1.000 |
| 1, 1, 0 (no line of sight) | 0 | 0 | 0 | 0.667 | 1.000 |
| 0.9, 0.9, 0.9, 0.05 | 0.063 | 0.437 | 0.050 | 0.687 | 0.900 |

Which to take:

- **geometricMean as the everyday choice.** The number of factors no longer
  matters (three times 0.95 stays 0.95), a 0 still rules out, a weak factor
  still hurts clearly. Good for most actions.
- **product** when every extra doubt should cost: risky actions (a charge,
  a spell with many conditions), where several "almost fine" mean "rather
  not".
- **minimum** for "everything has to fit": the weakest point decides, e.g.
  a position that must be covered *and* reachable *and* in range.
- **average** for preferences that make up for each other: choosing a cell
  to stand on, a weapon by taste. A 0 does **not** rule out.
- **median** against noisy inputs: a single outlier is ignored. A 0 does not
  rule out either, so never use it alone for hard rules.

Two tools work with every combine:

- **`apply: 'multiply'`** puts a factor outside the combine: it is
  multiplied onto the result afterwards, a true factor. Use it for hard
  rules ("no ammunition", "target not visible": a step to 0) and for one
  thing that must always count fully, even if the option averages the rest.
- **`pull`** sets how much a factor counts. The factor v becomes
  1 - pull · (1 - v). For a factor of 0.4: pull 0 → 1 (off), 0.25 → 0.85,
  0.5 → 0.7 (gentle, never below 0.5), 1 → 0.4 (as it is), 1.5 → 0.1,
  2 → 0 (strict). Gentle for side matters ("prefers cover a bit"), strict
  for what should bite early.

Two more for steady behaviour:

- **Rank** (`{ rank: 2 }` or `setRank`): only the highest rank that still
  scores above 0 is chosen from. 2 emergency (flee, heal), 1 combat, 0
  idle. Easier to tune than ever larger weights.
- **Current action** (`setCurrent({ option, target }, 0.1)`): the action
  going on now scores 10 % more, so a character does not flip between two
  nearly equal actions every turn.

A recipe for a melee fighter:

```js
const d = lib.decider();
d.setCombine('geometricMean');
const attack = d.addOption(1, [
  { input: 0, curve: 'inverse', min: 1, max: 8 },            // walking cost to the target
  { input: 1, curve: 'root', min: 0, max: 100 },             // own health: only low health hurts much
  { input: 2, curve: 'linear', min: 0, max: 100, pull: 0.3 },// target already hurt: a gentle bonus
  { input: 3, curve: 'step', min: 1, apply: 'multiply' },    // reachable this turn: a hard rule
]);
const flee = d.addOption(1, [
  { input: 1, curve: 'inverseSCurve', min: 15, max: 40 },    // health below about 30 %: switches softly
], { rank: 1 });
```

Rust: `add_weighted`, `set_combine`, `set_option_combine`, `set_rank`,
`set_current` (the builders answer `false` for an unknown option and are
`#[must_use]`). C: `sx_decider_add_weighted` (pull, `SX_APPLY_*`),
`sx_decider_set_combine` (`SX_COMBINE_*`, option `SX_ALL_OPTIONS` for all;
`SX_COMBINE_FOLLOW` follows the decider), `sx_decider_set_rank`,
`sx_decider_set_current`.

### Memory: cooldowns and "not again so soon"

The opposite of the bonus for the current action: after doing something, a
character often may not, or does not want to, do it again for a while. The
decider remembers what the game says it did (`record`, the last 256
actions). Time is the game's own count: frames, turns, hours; set it with
`setNow` before deciding.

- **Hard cooldown** (a rule: "once every 3 turns"): `setCooldown(option, 3)`.
  The option scores 0 until the time has passed.
- **Soft dislike** (allowed, but not wanted): a consideration that reads
  `MEMORY.sinceOption` (or `sincePair` for this target) instead of a row
  value. With `curve: 'linear', min: 0, max: 10` the score is low right
  after and full again after 10. Never done counts as very long ago.
- **Penalty for repeating:** `MEMORY.countOption` / `countPair` counts how
  often within the window (`setWindow(time)`); e.g. `curve: 'inverse',
  min: 0, max: 3` for "the third time in a row is unattractive".

```js
d.setNow(turn);
const taunt = d.addOption(1, [{ input: MEMORY.sincePair, curve: 'root', min: 0, max: 5 }]);
d.setCooldown(heal, 3);
const c = d.decide(candidates);
if (c) { act(c); d.record(turn, c); }
```

C: `sx_decider_set_now`, `sx_decider_record`, `sx_decider_set_cooldown`,
`sx_decider_set_window`, `sx_decider_forget`, inputs `SX_INPUT_SINCE_OPTION`
and friends.

### State machine

For behaviour that runs over time: patrol, chase, search, give up. States
and conditions are named (JavaScript) or numbered (C, Rust); the game says
every tick which conditions hold. Transitions are checked in the order
added, the first that matches wins; then the state's timeout. So put aborts
("dead", "under fire") first, from any state. At most one change per tick.

```js
const m = lib.machine(['patrol', 'chase', 'search', 'dead'], ['seen', 'killed']);
m.transition({ from: '*', to: 'dead', when: ['killed'] });          // abort, from any state
m.transition({ from: 'patrol', to: 'chase', when: ['seen'] });
m.transition({ from: 'chase', to: 'search', unless: ['seen'], after: 10 }); // chase at least 10
m.transition({ from: 'search', to: 'chase', when: ['seen'] });
m.timeout('search', 300, 'patrol');                                  // give up after 300
const change = m.update(frame, { seen: canSee, killed: hp <= 0 });   // {from, to, reason} or null
```

`after` keeps a state for a while (no flicker); `timeout` ends it. Inside a
state the decider can choose the details (which target, which weapon).
Time may wrap around. C: `sx_machine_*` with condition bits in a
`uint64_t`; Rust: `sagax_decide::state::StateMachine`.

## 7. Memory of the world

Every block here can say what it did (a fact evicted, a message refused, a
timer replaced): `*_traced` in Rust, `sx_*_set_trace` in C, see
[Debugging and logging](#11-debugging-and-logging).

Characters should remember things for a while: what they saw and heard,
where and when, what a colleague told them, which places are dangerous or
lit, how alert they are. The blocks of `sagax_decide::memory` do that, each
usable alone, and with the state machine and the decider they make up a
whole behaviour for turn-based tactics, real-time strategy, stealth or
action games. **The game decides what reaches them**: it says "seen",
"heard", and gives positions, either from its own tests or from the
percepts of the perception block (section 8, `learn_percepts`); these blocks
have no sight lines or hearing of their own. They keep it, pass it on and
let it fade. Positions are integers `[x, y, z]` (on
a hex grid use axial `(q, r)` in `x`, `y`; see Hex in the Conventions), time is the game's own `u32`
count (frames or rounds; ages use wrapping subtraction), no random numbers,
and sizes are fixed at creation. **Every time and limit is a parameter:** try
a short memory against a long one, a slow message against a fast one, and
keep what plays best.

The snippets assume `use sagax_decide::memory::*;` and, in Rust, a function
that may use `?` (a capacity of 0 is an error). Kinds, ids and areas of the
game (`SEEN`, `NOISE`, `LIT`, ...) are your own numbers.

- **Knowledge** (what a character knows): a table of facts keyed by
  `(subject, kind)`. A `Fact` has `pos`, `seen_at` (when it was observed, not
  when it arrived), `source` (`SOURCE_OWN`, `SOURCE_HEARD`, `SOURCE_TOLD`,
  more of your own are fine), and free `strength` and `data`. When full,
  `Evict::Oldest` or `Evict::Weakest` makes room. `learn` always
  overwrites; `merge` only if the new fact is not older.

  ```rust
  let mut known = Knowledge::new(8, Evict::Oldest)?;
  let fact = Fact { subject: 7, kind: SEEN, pos: [4000, 4000, 0], seen_at: now,
                    strength: 100, data: 0, source: SOURCE_OWN };
  let dropped = known.learn(now, fact);                  // Option<Fact>: the evicted one
  known.merge(now, heard_about_7);                       // keeps a newer own fact
  known.touch(7, SEEN, now);                             // seen again, same place
  known.set_pos(7, SEEN, [4100, 4000, 0]);               // false if unknown
  known.expire(now, 2040, |f| forget(f));                // age >= 2040 goes
  if let Some(f) = known.get(7, SEEN) { search_near(f.pos, now.wrapping_sub(f.seen_at)); }
  ```

  Also `remove`, `remove_subject`, `set_data`, `set_strength`, `expire_kind`,
  `expire_with` (an age per fact), `iter`, `len`, `clear`,
  `learn_message` / `merge_message` (turn a message into a fact). C
  `sx_knowledge_*` (handle, `sx_fact`, `SX_SOURCE_*`, `SX_EVICT_*`, `at` for
  walking the table, `sx_knowledge_expire_kind` for one age per kind), JS
  `lib.knowledge(8, 'oldest')` with `list()`, `expire(now, age, perKind)`.
- **Messages** (who tells whom, and when it arrives): three parts.
  The `Outbox` of a sender holds messages that become due after a delay,
  with a cooldown between posts. `route` picks the recipients among
  candidates you give (filter team and free state yourself). The `Inbox` of
  a receiver holds messages with their arrival time; `arrival` computes
  `now + base + distance * per_unit`, so a shout can take one round per cell.

  ```rust
  let mut out = Outbox::new(4)?;
  let msg = Message { kind: SEEN, subject: 7, pos: [4000, 4000, 0], seen_at: now,
                      strength: 0, data: 0, sender: me, keep_if_sender_gone: 0 };
  match out.post(now, msg, 102, 170) {                   // delay, cooldown
      Ok(()) => {}
      Err(Refused::Cooldown) | Err(Refused::Full) => {}
  }
  let mut due = Vec::new();
  out.due(now, &mut due);                                // delay over, in post order
  // `free`: &[(id, x, y)]; the ids go to `ids`
  let mut ids = Vec::new();
  route(Route::Nearest(3), (x, y), &free, Metric::Euclid, 16, &mut ids);
  let mut inbox = Inbox::new(4)?;                        // one per receiver
  inbox.push(msg, arrival(now, 0, dist, 2));             // payload, then the arrival tick; false if refused
  let mut got = Vec::new();
  inbox.take_due(now, &mut got);                         // arrived, in push order
  for m in &got { known.merge_message(now, m, SOURCE_TOLD); }
  ```

  Rules of `route`: `Route::Nearest(k)` (nearest first, a tie keeps the
  earlier candidate), `InRange(r)` (all with distance `<= r`, candidate
  order; with `Metric::Euclid` the square is compared with `r * r`), `All`,
  `To(id)`. `Metric` is `Euclid`, `Manhattan`, `Chebyshev` or `Hex` (axial);
  each axis is divided by the divisor first (truncating; the hex `/ 2` then
  truncates an odd sum). A message is a
  plain value: after `due` you may refresh `pos` and `seen_at` before
  routing. `sender_gone` drops the messages of a sender that died, except
  those with `keep_if_sender_gone` (a noise stays, a radio call is lost).
  `due_max` and `take_due_max` take at most some. An `Inbox` compares
  arrival times by their wrapping difference, so all stored arrivals must
  lie within a window narrower than 2^31 ticks.

  C `sx_outbox_*` (`sx_outbox_post` returns `SX_POSTED`, `SX_REFUSED_FULL` or
  `SX_REFUSED_COOLDOWN`), `sx_inbox_*`, `sx_arrival`, `sx_route` (with
  `SX_ROUTE_*`, `SX_METRIC_*`, `sx_nearby`), `sx_message`. JS `lib.outbox(4)`
  (`post` gives `'ok'`, `'full'` or `'cooldown'`), `lib.inbox(4)`,
  `lib.arrival`, `lib.route({nearest: 3}, [x, y], candidates, 'euclid', 16)`
  with candidates `[{id, x, y}]`.
- **Areas** (places to stay out of or pay extra for): a circle with a kind,
  centre, radius, `lifetime`, `strength` (cost per cell) and `blocking`.
  Adding an area of the same kind whose centre lies inside an existing one
  renews that one; when full the oldest goes.

  ```rust
  let mut areas = Areas::new(8)?;
  let lit = Area { kind: LIT, pos: [x, y, 0], radius: 600, strength: 20,
                   lifetime: 3000, blocking: 0, ..Area::default() };
  let index = areas.add(now, lit);                       // since = now
  areas.expire(now);                                     // age >= lifetime goes
  let inside = areas.contains(pos);                      // Option<usize>
  let blocked = areas.blocks(cell, start, goal);         // for move_cost, see below
  let extra = areas.cost(cell, start, goal);             // u32, for move_cost
  ```

  Start and goal are exempt in both `blocks` and `cost`, so a way can always
  leave an area and reach a goal inside one. Also `contains_kind`, `iter`,
  `len`, `clear`. C `sx_areas_*` with `sx_area`, JS `lib.areas(8)`.
- **Progress** (does the chase still get closer): a lower value is better,
  usually a distance. "Lost" is not here: it is the age of the fact in
  `Knowledge`.

  ```rust
  let mut p = Progress::default();
  p.start(distance, now);
  p.update(distance, now, 0x80);                         // true if it improved by 0x80
  if p.stalled(now, 85) { give_up(); }                   // no gain for 85 ticks
  ```

  C `sx_progress` plus `sx_progress_*`, JS `new Progress()`.
- **Timers** (ids that are "on" for a while: ignore a target, no alarm
  again): `set` renews, when full the entry with the least time left is
  replaced.

  ```rust
  let mut skip = Timers::new(4)?;
  skip.set(7, now, 170);
  if !skip.active(7, now) { chase(7); }
  let left = skip.left(7, now);                          // ticks still to go
  ```

  Also `remove`, `len`, `clear`. C `sx_timers_*`, JS `lib.timers(4)`.
- **Interval** ("again every N ticks"; never marked counts as due):

  ```rust
  let mut report = Interval::default();
  if report.due(now, 255) { report.mark(now); }
  ```

  C `sx_interval` plus `sx_interval_*`, JS `new Interval()`.
- **Alertness** (a value with levels): stimuli raise it, time lowers it. The
  level jumps straight to the highest threshold reached and falls when the
  value is below its threshold minus `hysteresis`. Decay only counts ticks
  after `hold` ticks since the last stimulus. The levels mean what you say
  (unaware, suspicious, searching, alert) and are good conditions for a
  state machine.

  ```rust
  let cfg = AlertConfig { max: 100, decay_per_tick: 2, hold: 5, hysteresis: 5,
                          count: 3, thresholds: [20, 50, 80, 0, 0, 0, 0, 0] };
  let mut alert = Alertness::default();
  alert.stimulus(30, now, &cfg);                         // a noise
  if let Some((from, to)) = alert.update(now, &cfg) { on_level(from, to); }
  let level = alert.level();                             // 0 ..= cfg.count
  ```

  C `sx_alertness` and `sx_alert_config` plus `sx_alertness_*` (`update`
  returns 1 and fills the two levels), JS `new Alertness()` with a config
  `{max, decayPerTick, hold, hysteresis, thresholds}`.
- **Points around a place** (agents spread out without random numbers) and
  **sweep** (turning the view while looking around). `PointPattern::default()`
  holds all numbers; change any of them. `Sine::Fine` is accurate;
  `Sine::Coarse` is the small table that reproduces older numbers exactly.
  The default box is not clamped (`min` and `max` span all of `i32`): set
  `min` and `max` for a bounded map. The ring is Euclidean, so on an axial
  hex grid it is skewed: convert to the game's world coordinates first (and
  back), or snap the result to the nearest hex.

  ```rust
  let pattern = PointPattern::default();
  let first = points_around(me, 0, last_pos, &pattern).unwrap();   // step 0, then 1..=3
  let heading = sweep_heading(base, elapsed, 51, &LOOK_OFFSETS);   // 0..=255
  let s = sine(heading, Sine::Fine);                               // 65536 = 1.0
  ```

  C `sx_points_around` (a `sx_point_pattern` or null), `sx_sweep_heading`,
  `sx_sine`; JS `lib.pointsAround`, `lib.sweepHeading`, `lib.sine`.

### Recipe: radio

One call, delayed and with a cooldown, to the nearest free colleagues. The
sender posts a message to its `Outbox` with delay 102 and cooldown 170; when
`due` hands it out, refresh `pos` and `seen_at` from your current fact, then
`route(Route::Nearest(3), ...)` over the colleagues who are free at that
moment, and `merge_message` with `SOURCE_TOLD` at each receiver. If the
sender dies before the delay is over, `sender_gone` drops the call.

### Recipe: shout

Everyone within reach hears at once: `Route::InRange(r)` with `Metric::Hex`
or `Euclid`, and `arrival(now, 0, 0, 0)` (base 0, per unit 0), or push
straight into the receivers' `Inbox`es. A `Timers` entry keeps the shouter
quiet for a while.

### Recipe: messenger

A message that travels: the same as a shout, but `arrival(now, base,
distance, per_unit)` with `per_unit > 0`, so far receivers hear later. Use
`Metric::Hex` and a distance in cells on a hex grid. A receiver with a full
`Inbox` drops the message that would arrive last.

### Recipe: noise

A noise is a message nobody sends on purpose: build a `Message` with
`keep_if_sender_gone: 1`, `route(Route::InRange(loudness), ...)` over all
guards whatever they do, `Inbox::push` it with `arrival` (base 0 and
`per_unit` 0 for "heard at once"), and let each receiver
`learn_message(now, &m, SOURCE_HEARD)` and raise its `Alertness`. A louder
noise reaches further because `r` is the loudness. With the perception block
a noise is a `Stimulus` instead and every guard checks it with `hear`
(walls damp it, the heard place is fuzzed); the stealth guard example does
that in round 2.

### Recipe: the officer

A witness chases, calls for help, loses sight and searches; the three
nearest free colleagues come; the table forgets. Walk, look, chase and idle
are states of a `StateMachine`; the loop each tick: `Outbox::due` and
`route` for the radio; report `Interval`; sight (every 4th tick) into
`Knowledge` and `Progress`; "lost" is the age of the fact, `stalled` or lost
leads to a `Timers` entry (ignore) and a search; `expire` once. The state
machine flags (`lost`, `stalled`, `arrived`, `give up`) are the conditions
you pass in. A complete, runnable version with the numbers for 17 ticks per
second: `cargo run -p sagax-decide --example officer`
(`crates/sagax-decide/examples/officer.rs`); in C `examples/c/memory.c`
(`sh examples/c/run.sh`).

### Recipe: stealth guard

Alertness levels drive the state machine on a hex grid: a noise (a
`Stimulus`, heard with `hear`) is a `SOURCE_HEARD` fact and raises
`Alertness`, the guard goes to look, its `look` notices and then recognises
the sneaker (`SOURCE_OWN` facts through `learn_percepts`, level "alert"),
it shouts to the guards in range (`Route::InRange` with `Metric::Hex`,
`arrival` with one round per cell), loses the sneaker (the age of the fact),
searches the points around the last place (`Sine::Fine`) and calms down as
the value decays. The light is `Lit` (the time of day) plus a lit cell; the
scene runs in daylight and at night, where the sneaker is noticed later.
Lit cells cost extra on the way (`Areas::cost`). Run it with
`cargo run -p sagax --example stealth_guard`
(`crates/sagax/examples/stealth_guard.rs`; the test pins the daylight
rounds).

### Recipe: area blocking and cost in `move_cost`

`Areas::blocks` and `Areas::cost` fit the map's `move_cost` (Rust
`CellMap::move_cost`, C `sx_move_cost_fn`): turn the cell into its centre
position, forbid the move, or add the cost. Start and goal are exempt, so a
way can always leave an area and reach a goal inside one.

```rust
// `self` is your map: it holds the areas and the start and goal positions.
fn move_cost(&mut self, _from: Cell, to: Cell) -> Option<Cost> {
    let centre = [to.x * 256 + 128, to.y * 256 + 128, 0];     // your cell size
    if self.areas.blocks(centre, self.start, self.goal) {
        None                                                  // forbidden
    } else {
        Some(self.areas.cost(centre, self.start, self.goal))  // extra cost, 0 outside
    }
}
```

```c
static int32_t move_cost(void *ctx, int32_t x, int32_t y, int32_t lv,
                         int32_t tx, int32_t ty, int32_t tl) {
	struct game *g = ctx;
	int32_t cell[3] = { tx * 256 + 128, ty * 256 + 128, 0 };
	int64_t extra;
	if (sx_areas_blocks(g->areas, cell, g->start, g->goal) == 1) return -1;
	extra = sx_areas_cost(g->areas, cell, g->start, g->goal);   /* returned directly */
	if (extra < 0) return -1;      /* an SX_ERROR_* (bad pointer): a set-up bug, forbid the move */
	return extra > INT32_MAX - 1 ? INT32_MAX - 1 : (int32_t)extra;
}
```

The same works on hex grids, since the game converts the cell to a position.
In JavaScript, call `a.blocks(cell, start, goal)` and `a.cost(cell, start,
goal)` while building the grid's walls and costs before the search. When the
areas are kept in cell coordinates, `Areas::cell_cost(to, start, goal)` (C
`sx_areas_move_cost`, JavaScript `areas.cellCost`) does both in one call:
section 9, "Areas to path costs".

## 8. Perception

What a character can see and hear, and how soon it notices: view cones,
exact sight lines with fog, smoke and light, attention, disguise and
hearing, on square and hex grids. Integer only and deterministic, no
allocation, no global state. Rust: `sagax::perception` (the crate
`sagax-perception` plus the helpers that join it with nav and memory),
records in `sagax::debug::PerceptionTrace`. C: `sx_perception_*`, since API
version 13. JavaScript: not yet.

**No cheating.** The block never looks at the world on its own. The game
passes its own candidate list of `Target`s and answers the questions of a
`SenseMap` (walls, fog, light, sound); what comes out is `Percept`s, which
the game turns into `Knowledge` facts. The squad and the mission stay blind:
their rule slots ask `sight_line` or `in_sight` for one yes or no.

**Fine points inside, whole cells at every edge.** A `World` has a cell
size `S` in fine units (for example 256); eyes, targets, stimuli and the
ends of a sight line are fine points `[x, y, level]`, so a character can
stand anywhere in its cell. A square cell `x` spans `[x * S, (x + 1) * S)`
with its centre at `x * S + S / 2`; on hex the fine points are axial and the
centre of hex `(q, r)` is `(q * S, r * S)`. Everything that leaves the block
is a whole cell: `Percept.cell`, the facts, the flood cells, the trace
cells, the cells the map is asked about. `cell_centre(world, cell)` and
`cell_of(world, point)` convert; a game that has only cells passes
`cell_centre` as the point. In C on hex the whole cells are **offset** cells
of the handle's layout (the handle converts them, as the mission does) and
the fine points stay **axial**.

### Sight line as a drop-in

`sight_line(world, map, from, to)` replaces a game's own Bresenham `sees`:
an exact walk over every cell the segment passes, fog by length, the same
from both ends. It answers `Line { clear, clarity, blocked_at }`.
`GridSight::square(&grid)` (or `GridSight::hex(&grid, layout)`) makes a
sense map from a `DenseGrid` (blocked cells and cells off the grid are
opaque); `MoveBlocks` blocks sight where movement is blocked.

```rust
use sagax::perception::{GridSight, World, cell_centre};

let world = World::square(256);
let c = |x, y| cell_centre(&world, Cell::new(x, y, 0));
let sees = sight_line(&world, &mut GridSight::square(&walls), c(2, 10), c(15, 3)).clear;
```

The settled edge cases:

- The eye's cell never blocks; the target's cell only through its edge
  (`sight_edge`); every cell in between blocks when opaque.
- A line running exactly along a grid line belongs to one side by a fixed
  index rule (square: the cell by floor, the one whose lower border it is;
  hex: the smaller of the two cells), so `A -> B` and `B -> A` agree.
- Through an exact corner (square) or vertex (hex) sight passes when one
  side is open; only two opaque sides block it. The movement tests
  (`line_clear`, `hex_line_clear`) stay strict.
- Between levels, `cross_level` answers (default: blocked).

`in_sight(world, map, eye, point)` adds the eye's range and view cone, with
no awareness and no time: the question of a rule slot such as "can the
receiver see the hand sign?". The ambush example uses `sight_line` for the
intruder's view of a spot and `in_sight` with the receiver's facing for hand
signs (`crates/sagax/examples/ambush_rules.rs`, `examples/c/ambush_rules.c`).

### A guard with `look`

An `Eye` is one observer for one call: its fine point, its facing (a
heading 0..=255, 0 = +x, 64 = +y; `hex_heading(dir)` on hex), a
`SenseProfile`, its `Stats` and its `DisguiseRule`. Profiles are plain
values: `SenseProfile::human`, `guard` and `watcher` with `with_night`,
`with_thermal`, `with_scanner`, `with_hearing` and `with_cue`. `Stats` are
passed on every call (`sight_scale`, `notice_scale`, `hearing_scale`,
`reaction_ticks`), so a game stat or a boost acts without touching the
profile. The game keeps one `Awareness` per observer–target pair and one
`Attention` per observer, zeroed to start. A zeroed `Awareness` was last
checked at tick 0, so a check credits the ticks since then: start your clock
at 1, or the first check at tick 0 credits nothing.

```rust
use sagax::perception::{Attention, Budget, Lit};

let profile = SenseProfile::guard(256);
let eye = Eye { id: 1, point, facing, profile: &profile,
                stats: Stats::NORMAL, disguise: Default::default() };
let mut map = Lit { inner: GridSight::square(&walls), ambient: 96 };   // night
let budget = Budget { checks: 3, keep: 1, instant_cap: 1 };
percepts.clear();
look(&world, &mut map, now, &eye, &targets, &mut aware, &mut attention, budget,
     &mut percepts);
```

Each check adds evidence by light, distance, band, size, conspicuity,
crowd and fog; the counter makes the target *noticed* (stage 1) and then
*recognised* (stage 2), after the reaction time, and decays once it is out
of view. `look` checks targets with an instant cue first (up to
`instant_cap`), then kept ones (noticed, up to `keep`), then the rest round
robin, at most `checks` of the last two; skipped time is never paid back, so
a crowd really dilutes attention. `observe` checks one pair directly.
`aware` must be parallel to `targets`, of the same length (a debug build
asserts it).

What comes out per check (at most `MAX_PERCEPTS_PER_CHECK`, 4): `Noticed`,
`Recognized`, `Cues { new }` (cue bits seen at stage recognised), `Seen`
(its place now, on every in-view check at stage noticed or higher), `Lost`
(in view at the last check, not now), `Forgotten`, `SeenThrough`.

- `Lost` comes only for a target that was noticed. Percepts with
  `placed == false` (`Lost`, `Forgotten`, a delayed rise that became due
  out of view) carry no position: `point` and `cell` are zero.
- A cue rule with `gain` 0 is neutral (no gain factor), so a rule that is
  only instant ("a drawn weapon is recognised at once in the focus") is
  possible.
- An instant fires from the sight pass only, and only while sight sees
  the target: any of band, distance, light, clarity, size or conspicuity
  at 0 blocks it (darkness with `dark_gain` 0, size 0, conspicuity 0); a
  gain that merely rounds to 0 does not. The thermal pass never fires
  one: a `SIGHT` + `THERMAL` eye with `dark_gain` 0 sees a warm target in
  the dark by heat, but its instant stays blocked (`NIGHT` lifts the
  light, so it fires). `look` picks instant targets by cue bits alone, so
  in the dark or for a thermal-only eye such a pick uses an `instant_cap`
  slot without firing.
- The scanner (`SCANNER`) reaches only *noticed*: it gives a location,
  never cues, never recognition and never sees through a disguise.
- Light is read at the target's cell (with its `glow`); `Lit { inner,
  ambient }` caps a hook's light by one global level (time of day).

**Disguise.** A target with `disguise > 0` is perceived, flagged
`disguised`, and the cue bits in `eye.disguise.hides` are never reported.
By default that always holds; `see_through: Some(..)` lets an observer
grow doubt and see through it (a `SeenThrough` percept). A `Stimulus` has a
`disguise` too: a disguised source's noise is heard as `disguised` (unless
the pair has already seen through it); hearing never grows the doubt.

### Hearing, with a second channel

A `Stimulus` (a sound, a shout, a shot, smoke) has a loudness in fine units
and a channel of `world.channels` (`MAX_CHANNELS`, 4). Each channel has its
`damping`, `fuzz` (position error per cell of cost), `speed` and `Spread`:
`Circle` (straight distance), `Straight` (distance plus the map's damping
along one line). Smell is a channel, not a sense:

```rust
let mut world = World::square(256);
world.channels[0].spread = Spread::Straight;                      // sound
world.channels[1] = Channel { damping: 0, fuzz: 128, speed: 64,   // smell
                              spread: Spread::Circle };
let profile = SenseProfile::guard(256).with_hearing(1, 128);       // a weak nose
percepts.clear();
hear(&world, &mut map, now, &eye, &stimulus, None, Some(&mut aware), &mut percepts, seed);
```

`hear` pushes a `Heard` percept with a deterministically fuzzed whole cell
and `margin`, `arrive_at`. With an `Awareness` it raises the pair at most to
*noticed*; without one (`None`) it is stateless. A recognition by sight
that becomes due during a `hear` call is reported unplaced (the sound is
not where the target was seen). One call returns up to 3
percepts (`Heard`, `Noticed`, a due `Recognized` or `Forgotten`); in C give
`sx_perception_hear` a cap of at least 3, `sx_perception_look` at least
4 × (`checks` + `instant_cap`).

Around corners: `flood_sound(nav, world, map, stimulus, budget, out)` spreads
the stimulus with one `Navigator::reachable` over `SoundCost`, bounded by the
stimulus' loudness; each listener then calls `hear` with `Some(cost)` of its
cell. It expects the navigator's straight step cost (on hex every step) to
equal `world.cell_size`, so the costs are fine units (a debug build asserts
it). The flood is bounded by the loudness alone: `hearing` or
`hearing_scale` above 256 let `hear` accept a larger cost but do not widen
the flood. With `Moves::Eight` a diagonal step asks `sound_edge` for the
diagonal pair of cells, so the map must answer those too. `count_crowds`
measures Euclid on the point's `x`, `y`: on hex that is axial Euclid, an
approximation.

### Into knowledge: `learn_percepts`

`learn_percepts(known, alert, alert_cfg, now, percepts, kinds, amounts)` is
the common use in one call. `FactKinds` names the game's kinds
(`noticed_kind`, `seen_kind`, `heard_kind`, `disguised_kind`),
`AlertAmounts` the `Alertness` stimulus per percept kind and cue bit.
`Noticed` and `Seen` at stage noticed learn the noticed kind at the cell;
`Recognized` and `Seen` at stage recognised remove it and learn the seen
kind; `Cues` set the seen fact's `data`; `Heard` merges the heard kind
(`SOURCE_HEARD`). It learns positions only from placed percepts, and
`Lost`, `Forgotten`, `SeenThrough` remove nothing: facts age out by the
game's `expire*`. A disguised percept uses `disguised_kind`, or is skipped
without one. For one percept at a time: `percept_fact` and `alert_amount`.
The stealth guard (`cargo run -p sagax --example stealth_guard`) runs this
loop on a hex grid in daylight and at night.

### C

```c
sx_perception_config cfg;
sx_sense_map map;
sx_sense_map_from_grid(grid, &map);
sx_perception_config_default(SX_GRID_SQUARE, 256, &cfg);
sx_perception *p = sx_perception_new(&cfg, &map);
sx_sense_profile guard;
sx_sense_profile_preset(SX_SENSE_PRESET_GUARD, 256, &guard);
/* eye.profile = &guard; one sx_awareness per pair, one sx_attention per eye */
int32_t n = sx_perception_look(p, now, &eye, targets, aware, count, &attention,
                               &budget, out, 4 * (budget.checks + budget.instant_cap), NULL);
sx_learn_percepts(known, alert, &alert_cfg, now, out, (uint32_t)n, &kinds, &amounts);
```

The names follow Rust: `sx_perception_line`, `_in_sight`, `_observe`,
`_look`, `_hear`, `_flood`, `_cell_centre`, `_cell_of`, `_set_trace`;
`sx_percept_fact`, `sx_percept_alert`, `sx_learn_percepts`,
`sx_count_crowds`, `sx_hex_heading`, `sx_awareness_*`. Two families of
constants look alike: **`SX_SENSE_FLAG_*` are profile bit flags**
(`sx_sense_profile.senses`, OR them: `SIGHT` 1, `NIGHT` 2, `THERMAL` 4,
`SCANNER` 8; Rust `SIGHT`, `NIGHT`, `THERMAL`, `SCANNER`), **`SX_SENSE_*`
without `FLAG` are percept sense codes** (`sx_percept.sense`, one value:
`SIGHT` 0, `THERMAL` 1, `SCANNER` 2, `HEARING` 3), and `SX_SENSE_PRESET_*`
pick a ready profile. `sx_stimulus.disguise` follows `sx_target.disguise`. Callbacks must not call any
function on the same `sx_perception`. `examples/c/perception.c` (a guard,
smoke, a crowd and a shot) prints the same lines as
`crates/sagax/examples/perception_scene.rs`.

## 9. Group tactics

Several characters should act together: advance side by side, come from
several sides and strike at the same moment, hold an ambush. Hand-written,
that is a squad AI per game. The blocks of `sagax_decide::group` give the
parts: a **squad leader** that plans from what it knows, **goals** that
reach the members as messages, and **tactics as data**. The **`Crew`** runs
those parts as one loop per tick, so a game starts there and reaches for the
blocks only when it needs more control. The game keeps control of who is a
group and what the roles mean. Same rules as the memory blocks: integer
only, wrap-safe `u32` time (any unit, all durations the same one), seeds from
the game, ties to the lower index or id, capacities set once. After warm-up
nothing allocates except one `Vec` per new plan (its goals).

The Rust snippets assume `use sagax::prelude::*;` and, where named,
`sagax::decide::group::{Channel, Shape, ...}`. `ENEMY` is your fact kind for
enemies, `now` your tick (`u32`), `spots` your candidate places
(`Vec<Spot>`, see below) and `seed` your `u64` for this decision.

### Core and kit

The group blocks have two layers. The **core** is what every squad needs:
members, the leader's memory, scoring a (tactic, target) pair, assigning
members to slots, orders over channels, the signal, aborts, skill, and the
rules slots of "Your own pieces". The **kit** is what ships for the common
case: the shipped tactics (`tactics::rush`, `formation`, `flank`, `ambush`,
`focus`, `isolate`, `fall_back`), the tag bits (`TAG_COVER`, `TAG_HIGH`,
`TAG_HIDDEN`, `TAG_CHOKE`), the role bits (`ROLE_RANGED`, `ROLE_AREA`) with
their columns (`IN_RANGED`, `IN_AREA`) and the default numbers (the default
role list and skill table). The kit is built only from the public API of the
core, and the core never imports it (a test checks this). A game can write
every piece of the kit itself, or leave all of it out: a squad with its own
role list and its own tactics uses none of it. The Rust paths are unchanged
(`group::tactics`, `group::TAG_COVER`); the module is `group::kit`.

### The Crew

A `Crew` is a `Squad` with one `GoalKeeper` per member. Per game tick you
tell it where the members are, call `tick`, read the events, and for each
member ask for the row of its own decider and, once it decided, tell the
crew what it chose. The crew moves nobody: walking, shooting and animation
are yours. Arrival counts only on the same level (z), so a game that uses
z for height must keep members on the goal's level. Channels, tactics, skill and the leader are set on the crew's own
squad (`crew.squad_mut()`, C `sx_crew_squad`, JavaScript `crew.squad()`).

Rust (`crates/sagax/examples/squad_walk.rs`, set-up and one tick):

```rust
let mut crew = Crew::new(CrewConfig {
    squad: SquadConfig {
        members: 4,
        memory: 8,
        areas: 4,
        inbox: 8,
        dangers: 4,
        metric: Metric::Chebyshev,
        ..SquadConfig::default()
    },
    ..CrewConfig::default()
})
.unwrap();
for (id, x, y) in MEMBERS {
    let member = Member {
        id,
        pos: [x, y, 0],
        roles: 0,
        skill: SKILL,
        state: MemberState::Active,
    };
    assert!(crew.add_member(member).is_some());
}
let sq = crew.squad_mut();
for (id, ..) in MEMBERS {
    if id != 1 {
        assert!(sq.set_channel(id, RADIO));
    }
}
assert!(sq.set_leader(1));
sq.set_skill(SKILL);
sq.set_target_kind(ENEMY);
sq.add_tactic(tactics::rush());
sq.add_tactic(tactics::formation(Shape::Line, 2));
sq.add_tactic(tactics::flank(3, 4));
crew.observe(
    0,
    Fact {
        subject: 10,
        kind: ENEMY,
        pos: [20, 5, 0],
        seen_at: 0,
        ..Fact::default()
    },
);
```

```rust
// every tick t:
crew.tick(t, &[], &spots, SEED, &mut events).unwrap();
for e in &events {
    // Planned, Activated, Signal, Order, Start, Arrived, Broken, NoPlan:
    // the game turns them into walking, shooting, animation
}
// each member decides: its goal or something else
for w in &mut walkers {
    let inputs = crew.member_inputs(w.id, t).unwrap();
    let discipline = u32::try_from(inputs.discipline).unwrap_or(0);
    crew.keeper(w.id)
        .unwrap()
        .set_current(&mut w.decider, w.goal_option, discipline);
    let candidates = [
        Candidate::new(w.goal_option, 0),
        Candidate::new(w.idle_option, 0),
    ];
    let row = inputs.as_row();
    let choice = w
        .decider
        .choose(&candidates, &[row, row], Pick::Best, 0)
        .unwrap();
    w.on_goal = choice.is_some_and(|c| c.candidate == 0);
    assert!(crew.member_chose(w.id, t, w.on_goal));
}
```

The goal is one option among the member's own (`GoalKeeper::add_goal_option`
makes it and gives it a decider row of five inputs); `member_chose(id, now,
goal_won)` says whether it won: `true` keeps the goal, `false` postpones it
(see Two levels). C (`examples/c/squad.c`, the same scene):

```c
sx_crew_config config;
sx_crew_config_default(&config);
config.squad.members = MEMBERS;       /* memory, areas, inbox, dangers, metric ... */
config.squad.metric = SX_METRIC_CHEBYSHEV;
sx_crew *crew = sx_crew_new(&config);
sx_crew_add_member(crew, &m);         /* an sx_member: id, pos, skill, state */
sx_squad *squad = sx_crew_squad(crew);
sx_squad_set_radio(squad, id, 2, 1);  /* base 2 ticks, 1 per cell */
sx_squad_set_leader(squad, 1);
sx_squad_add_flank(squad, 3, 4);
sx_crew_observe(crew, 0, &seen, NULL);

/* every tick t: */
n = sx_crew_tick(crew, t, NULL, 0, spots, 4, SEED, events, EVENTS, &total);
for (i = 0; i < n; i++) { /* events[i].kind: SX_CREW_PLANNED ... SX_CREW_NO_PLAN */ }
/* each member: */
sx_crew_member_inputs(crew, id, t, &in);   /* in.has_goal, in.dist, in.age, in.signal, in.discipline */
sx_keeper_set_current(sx_crew_keeper(crew, id), decider, goal_option, (uint32_t)in.discipline);
/* ... decide ... */
sx_crew_member_chose(crew, id, t, goal_won);
```

`sx_crew_tick` writes at most `cap` events and puts the number of all of
them to `*total` (may be NULL); a buffer that is too small changes nothing
else. JavaScript (`examples/js/squad.mjs`, the same scene):

```js
const crew = lib.crew({ squad: { members: 4, memory: 8, areas: 4, inbox: 8, dangers: 4, metric: 'chebyshev' } });
for (const [id, x, y] of MEMBERS) crew.addMember({ id, pos: { x, y, level: 0 }, skill: SKILL });
const sq = crew.squad();
for (const [id] of MEMBERS) if (id !== 1) sq.setRadio(id, 2, 1);
sq.setLeader(1);
sq.addTactic(lib.tactics.flank(3, 4));
crew.observe(0, { subject: 10, kind: ENEMY, pos: { x: 20, y: 5, level: 0 }, seenAt: 0 });

// every tick t:
for (const e of crew.tick(t, { spots: SPOTS, seed: SEED })) {
  // e.kind: 'planned' | 'activated' | 'signal' | 'order' | 'start' | 'arrived' | 'broken' | 'noPlan'
}
const inputs = crew.memberInputs(id, t);   // { hasGoal, dist, age, signal, discipline, row }
crew.keeper(id).setCurrent(decider, goalOption, inputs.discipline);
const choice = decider.decide([{ option: goalOption, inputs: inputs.row }, { option: idleOption, inputs: inputs.row }]);
crew.memberChose(id, t, choice !== null && choice.index === 0);
```

What `tick` reports, in the order of its steps (the Rust `CrewEvent`, C
`SX_CREW_*` and JavaScript `kind` all carry the same set):

| Event | Meaning |
|---|---|
| `Planned(plan)` | the leader made a new plan; it is pending while the squad "thinks" |
| `Activated(plan)` | that plan is in force; members without a goal in it have none now |
| `Signal` | the leader gave the signal (the attack starts) |
| `Order { member, goal, start_at }` | an order reached a member and is in its keeper; `start_at` is the tick it starts, or none if it starts on the signal |
| `Start { member }` | a member's start tick has come (once per order) |
| `Arrived { member }` | a member stands on its first spot (within `arrive_radius`, same level) and told the leader over its channel (once per goal) |
| `Broken { member }` | a member gave its goal up (postponed longer than its skill's give-up time) and told the leader (once per goal) |
| `NoPlan` | no plan is active and none could be made; if a plan just ended, every keeper is cleared |

On a real run the signal comes at tick 51 and all four attackers start at
tick 57 (see the trace in the [quickstart](QUICKSTART.md#4-the-whole-walk));
the radio makes the members arrive at ticks 33, 41, 45 and 45, and the
signal lines their attacks up.

More about the crew:

- **Members** come and go with `add_member`, `remove_member`, `suspend`
  (out of play, e.g. a cut-scene: no goal, never "broken"), `reinstate(id,
  now)` (back in play with its current goal, its keeper clock restarts) and
  `set_gone` (dead). A suspended or gone member gets no `Start`, no arrival
  and no `Broken`. The crew reads a squad you change through `squad_mut()`
  at the next tick.
- **Reports:** `crew.report_danger(sender, centre, shooter, arrive_at)` puts
  a danger report on its way to the leader (C `sx_crew_report_danger` returns
  1 if stored and 0 when the danger queue is full; JavaScript
  `crew.reportDanger`). The area it makes has `data` = the sender. The
  leader's own sightings go in with `observe(now, fact)`.
- **Goals of a dead plan** are dropped: when a replan leaves a member out, or
  the active plan ends (`NoPlan`), the keepers are cleared, so `member_inputs`
  shows `has_goal` 0 and the member gets no stale `Start`, `Arrived` or
  `Broken`. A refused arrival report (inbox full) is tried again next tick
  and gives its event only when it was accepted.
- **Hex:** a crew with a hex layout takes and returns offset coordinates (see
  Hex in the Conventions). The tick's spots are offset coordinates too.
- **Allowed calls on the squad:** do not call `orders_due` on a crew's
  squad: the crew takes the orders itself (C `sx_squad_orders_due` returns
  `SX_ERROR_ARGUMENT`, JavaScript `ordersDue` throws `TypeError`).
- **Lent handles (C and JavaScript).** `sx_crew_squad` and `sx_crew_keeper`
  (JavaScript `crew.squad()`, `crew.keeper(id)`) lend handles that the crew
  owns. The squad is valid until `sx_crew_free`; calling `sx_squad_free` on
  it does nothing. A keeper handle is valid until the next call that
  *changes* the crew (anything but `sx_crew_member_inputs`, `sx_crew_goal`,
  `sx_crew_squad` and `sx_crew_keeper`; reads do not count) and while its
  member is in the group; after that every `sx_keeper_*` call on it returns
  `SX_ERROR_ARGUMENT` (JavaScript: throws an error that says "stale"), so
  fetch it again. A keeper handle never reaches another member's keeper.
  There is one handle per member id, the same each time.
- **Capacities** are fixed at `new`. After the first ticks (a hex crew's spot
  buffer grows once, to the largest spot list) the crew allocates nothing
  beyond what the squad does.

### Two levels

1. **The rough plan (squad).** The leader picks a tactic and gives every
   member a *goal*. The plan holds as long as it works.
2. **Acting in the moment (member).** Each member decides with its own
   `Decider` (section 6). **The group goal is one option among the others**
   (shoot, take cover, flee). It normally wins; a surprise makes another
   option win, and the member picks the goal up again when things calm down.

The member's inputs come from `GoalKeeper::inputs` (or `member_inputs` of the
crew): has goal, distance to the goal spot, goal age, signal given,
discipline. `GoalKeeper::add_goal_option(decider, weight)` adds the goal
option and `keeper.set_current(decider, option, discipline)` gives it the
*current bonus* of the decider (section 6) so it is not dropped lightly;
`SkillTable::hold_bonus_for(skill)` says how much, so elite members cling
longer. Call `goal_postponed(now)` when another option wins and
`goal_won(now)` when the goal wins again (the crew does it in
`member_chose`). A goal postponed for `give_up_ticks` is *broken*:
`keeper.broken(now, give_up_ticks)` is your cue to send a
`Message::goal_broken(id, plan)` back, and the plan may change.

**The game's orders come first.** `sq.group_mut().suspend(id)` (on the
`Group`; C `sx_squad_suspend`, JS `sq.suspend`; on a crew `crew.suspend(id)`)
takes a member out (a cut-scene, a player command): it gets no goal, does not
count as off plan, slot assignment skips it, and its keeper's clock stands
still. `sq.reinstate(id, now)` (on the `Squad`; C `sx_squad_reinstate`, JS
`sq.reinstate(id, now)`) puts it back **and sends it its current goal** of
the active plan over its channel: before the signal the plan's goal, after
it the signal order (its start is not lined up any more, the others have
started). `Group::resume` alone only flips the state and the member stays
without orders. A suspension never produces "goal broken"; only `report`
does. On the member side, `GoalKeeper::suspend` / `unsuspend` stop and
restart the keeper's clock.

### The blocks by hand

What `Crew::tick` does, step by step, is a public call of `Squad` or
`GoalKeeper` each; so a game that needs another order, another message or its
own bookkeeping builds the loop itself. Rust, one tick for a squad and its
keepers:

```rust
let mut sq = Squad::new(SquadConfig {
    members: 8,
    memory: 16,
    areas: 4,
    inbox: 8,
    dangers: 8,
    metric: Metric::Chebyshev,
    ..SquadConfig::default()
})?;
assert!(sq.group_mut().add(Member { id: 1, pos: [0, 0, 0], roles: 0, skill: 192,
                                    state: MemberState::Active }).is_some());
assert!(sq.set_leader(1));
sq.set_skill(192);
sq.set_target_kind(ENEMY);                 // which fact kind counts as a target
assert!(sq.set_channel(2, Channel::Radio { base: 2, per_unit: 1 }));
sq.add_tactic(tactics::flank(3, 4));
sq.add_tactic(tactics::formation(Shape::Line, 2));
sq.add_tactic(tactics::rush());
sq.observe(0, fact);                        // the leader sees an enemy

// every tick (`keepers`: one GoalKeeper per member, in group order):
// 1.-2. the leader checks its plan
match sq.check(now, &[], &spots, seed)? {
    SquadStatus::Replan(_id) => { /* a new plan is pending (Planned) */ }
    SquadStatus::Signal => { /* the attack starts now (Signal) */ }
    SquadStatus::Keep | SquadStatus::NoPlan => {}
}
// 3. a plan that became active (Activated)
if let Some(_plan) = sq.take_activated() {}
// 4. orders that reached their member go into its keeper (Order)
sq.orders_due(now, &mut orders);
for o in &orders {
    let i = sq.group().index(o.member).unwrap();
    keepers[i].set(now, o.goal);
}
// 6. a member that stands on its spot tells the leader over its channel (Arrived)
if goal.awaits_arrival() && at_the_spot {
    let msg = Message::arrived(id, goal.plan);
    if let Some(at) = sq.report_arrives_at(id, now) {
        let _ = sq.report(msg, at);       // false: the inbox is full, try next tick
    }
}
// 7. a goal postponed for too long (Broken)
if keeper.broken(now, table.give_up_ticks_for(skill)) {
    // send Message::goal_broken(id, plan) the same way
}
```

Step 5 (`Start`) is `now` reaching `goal.start_tick(received)`; the crew
keeps those ticks for you. C: `sx_squad_check`, `sx_squad_take_activated`,
`sx_squad_orders_due`, `sx_keeper_set`, `sx_report_arrived`,
`sx_squad_report_arrives_at`, `sx_squad_report`, `sx_keeper_broken`,
`sx_goal_start_tick`, `sx_goal_awaits_arrival`. JavaScript:
`sq.check`, `sq.takeActivated()`, `sq.ordersDue(now)`, `keeper.set(now,
goal)`, `sq.reportArrivesAt(id, now)`, `sq.report(message, arriveAt)`,
`keeper.broken(...)`, `lib.startTick(goal, received)`,
`lib.awaitsArrival(goal)`.

### The squad and its memory (no cheating)

**The leader plans only with what it knows.** The squad keeps its own
`Knowledge`, `Areas` and inbox. They are filled only by the leader's own
sightings (`sq.observe(now, fact)`) and by member reports that *arrive*
(`sq.report(message, arrive_at)`, kinds `REPORT_ENEMY`, `REPORT_ARRIVED`,
`REPORT_GOAL_BROKEN`; build them with `Message::enemy`, `Message::arrived`,
`Message::goal_broken`, C `sx_report_enemy`, `sx_report_arrived`,
`sx_report_goal_broken`, JavaScript `Report.enemy`, `Report.arrived`,
`Report.goalBroken`). A danger has its own call:
`sq.report_danger(sender, centre, shooter, arrive_at)` (a message of kind 3
is dropped in Rust and C and throws in JavaScript; `REPORT_DANGER` is gone).
Targets are read from this memory (`sq.targets()`), never from the true
world; the game's `extra` row only adds columns on top (at most
`MAX_EXTRA_COLUMNS`, 1024). A shout or radio delay on the report is your own
`route` / `arrival` from section 7, or `sq.report_arrives_at(id, now)`.

**Forgetting.** `plan` and `check` drop danger areas past their lifetime by
themselves. The leader's knowledge is aged by the game, as for any
`Knowledge`: `sq.knowledge_mut().expire(now, max_age, |_| {})` (C
`sx_knowledge_expire(sx_squad_knowledge(sq), ...)`, JS
`sq.knowledge().expire(...)`). A target that is forgotten, or not seen for
`tactics::TARGET_LOST_TICKS` (255) ticks, ends the plan built on it: every
shipped tactic but rush and fall back has that abort rule, and any plan
whose target is no longer known at all is replanned.

**If the leader is lost** (`set_gone`), the next active member in slot order
takes over. The active plan stays valid: members keep their goals and the
new leader keeps checking the same plan. A loss counts once in the loss
column of the abort rules, nothing more. `IN_LOSSES` counts every member
marked gone that is still listed, also across plans; remove a member
(`group_mut().remove(id)`) to stop counting it. A new plan comes only if the new
leader's `check` asks for it, and it costs think time as usual. Delays are
measured from the leader's position.

### Planning, orders, signal

`check` makes the first plan and replans; `plan` forces one. A new plan is
*pending* for the think time from the skill table (`think_ticks`), then it
becomes *active* and the orders go out: the leader's at once, the others by
their `Channel` (`Shout { range }` arrives after one tick per unit of
distance and only within range, `Radio { base, per_unit }` anywhere, `None`
never). **The default channel is `Radio { base: 0, per_unit: 0 }`, which
arrives at once:** set real channels for every member if orders should take
time (Rust `set_channel(id, Channel::...)`, C `sx_squad_set_shout`,
`sx_squad_set_radio`, `sx_squad_set_silent`, JavaScript `sq.setShout`,
`sq.setRadio`, `sq.setSilent`). **Making and spreading a plan costs ticks:** a
plan can be stale when it arrives, as in a real chain of command.

**What an order means.** `goal.pos` is where to go now; `goal.kind` and
`goal.start` say what to do there and when. A member gets at most one order
per plan on its way: a later one replaces an earlier one still travelling
and never arrives before it.

**Signal and the simultaneous attack.** The shipped flank tactic first
*gathers*: each slot walks to its gather spot, `gather_dist` (twice the
attack distance) from the target, out of its reach. That first order is
`GoalKind::Move`, `Start::Now`, with `then_pos` set to the attack spot. The
trigger (`Trigger::AllArrived { max_wait }`) fires when every assigned
active member has arrived, or `max_wait` ticks after the gather orders went
out, whichever comes first; then `check` returns `Status::Signal`. Arrival
reaches the leader as a `REPORT_ARRIVED` message (sender: the member, data:
the plan id) through `sq.report(msg, arrive_at)`, with whatever delay the
member's way of telling has (the crew does this for you);
`sq.arrived(id, now)` is the shortcut without delay, as if the leader saw it.
You can also call `sq.signal(now)` by hand (`Trigger::Manual`), or use
`TargetInRadius`. On the signal each member gets a second order: its place at
the target (`then_pos`) with the slot's own kind (attack). The orders line
up: `start` is `AfterTicks(n)`, the time until the slowest second order
arrives (plus the slot's own `AfterTicks`, if any), so **all members start at
the same tick** however far the radio or shout travels; the member's
`start_delay` (the timing spread of weak squads) comes on top. See
`crates/sagax/examples/squad_walk.rs` for the whole scene.

**No "Better" replan after the signal.** Before the signal `check` replans
when another tactic beats the active one by more than the hold bonus of the
skill table (no flicker); the new plan is picked only among the candidates
above that bar, so a weak squad's chance pick cannot land on a worse one.
The pick's seed is mixed with `now`, so a weak squad does not repeat the
same chance pick every tick. Once the signal is given, a started attack is
carried through; only an abort rule (own losses, members off plan, target
not seen for too long) or a forgotten target ends it.

### Skill

One `skill: u8` for the squad (0 weak, 255 elite) and one per member. The
`SkillTable` (1 to 8 rows, `rows` of them, four by default; a skill takes row
`skill * rows / 256`, so with four rows that is `skill / 64`; every number can
be changed, see `Squad::set_skill_table`, which answers `false` for 0 or more
than 8 rows) sets:

| What | Weak squad | Elite squad |
|---|---|---|
| Which tactics it knows | only those with a low `min_skill` (rush) | all |
| How it picks | weighted among the top 3 | the best |
| Think time before a plan is active | long (`think_ticks`, 34) | short (5) |
| Spot choice | up to `spot_rank_error` ranks off (2) | the best spot |
| Start of the attack | spread by `timing_spread` (17) | together |
| Plan hold bonus / member discipline | 12.5 % | 50 % |
| Goal given up after | `give_up_ticks` (17) | 68 |

The numbers in brackets are the defaults, in ticks. All random effects use
the caller's seed mixed with member index and tick, so a replay is
identical. Weak enemies therefore look weak: they plan late, take poor
spots, start unevenly and give up their goal sooner. The defaults assume
about 17 ticks per second; scale them for your time unit. Each value for one
skill is a call of its own: `SkillTable::think_ticks_for(skill)`,
`timing_spread_for`, `hold_bonus_for`, `give_up_ticks_for`,
`spot_rank_error_for`, `pick_for` (C `sx_skill_table_*_for`, JavaScript
`lib.thinkTicksFor(table, skill)` and so on).

### Spots with few tags

The game passes candidate spots, `Spot { pos, tags }` (cover, high, hidden,
choke, your own bits). A tactic computes a *wish point* (front, left, right,
rear of the target, between target and its friends, a formation offset) and
takes the best spot near it. Spots are picked, never invented, and only
within the **spot radius** of the wish point: by default `3 * slot.dist`
(at least 1; the ambush's hidden spots count within 12 of the group
centre), or one radius for all set by `sq.set_spot_radius(Some(r))` (C
`sx_squad_set_spot_radius`, JS `sq.setSpotRadius(r)`). So a fall back never
takes "cover" next to the enemy just because it is tagged. **Every shipped
tactic works when a tag is missing:** it falls back to the nearest free
spot within the radius, and without one to the wish point itself, flagged
`unchecked` on the goal so you can verify it. A game that knows only "free",
"high" and "choke" can pass tags 0 everywhere and still get plans. A custom
slot with a tagged place and `dist` 0 only looks within 1: give it a `dist`.

The isolate tactic scores the built-in column `IN_TARGET_FRIEND_DIST`: the
distance from the target to its nearest other known target (-1 without
one), so the most isolated enemy wins without any column of yours.

### Group kinds are your business

The member list is fixed and finished; Sagax does not decide who belongs.
`gather(...)` (C `sx_gather`, JS `lib.gather`) is a helper for the common
case: the free characters of a faction within a radius of an initiator,
nearest first, ties to the lower id. Use it for "call the 3 nearest free
members", "everybody of the faction within the shout range" or a travelling
party; nobody forces you to.

### Areas to path costs

Danger areas the squad remembers (`sq.areas()`, filled from `report_danger`)
and the game's own `Areas` become path costs in one way:
`Areas::cell_cost(to, start, goal)` (Rust, on `Cell`s), `sx_areas_move_cost`
(C, an `sx_move_cost_fn`) and `areas.cellCost(cell, start, goal)` (JS; the
older `lib.areasMoveCost(areas, from, to, start, goal)` does the same). Only
the cell moved to is judged: blocked by an area that holds neither start nor
goal gives `None` / `-1` / `null`, otherwise the extra cost (at most
`INT32_MAX - 1` in C and JavaScript). The level is the `z` coordinate. A "go
around the place the shooter stood" route needs nothing else.

**Units:** these adapters compare the cells directly with the areas, so the
areas (and start and goal) must be in the map's cell coordinates `(x, y,
level)`. If your areas are in finer world units (the danger reports carry
whatever positions your members report), convert each cell first and call
`blocks` / `cost` yourself, as in the recipe of section 7 (cell to
`x * 256 + 128`).

In C, `map.move_cost = sx_areas_move_cost` with `map.context` pointing to an
`sx_areas_cost_ctx` (areas, start, goal) is enough when the map has no other
context. **The context is shared with the map's other callbacks:** if your
`step` callback needs its own, do not set `sx_areas_move_cost` directly;
call it from your own `move_cost` with a prepared `sx_areas_cost_ctx`:

```c
static int32_t my_move_cost(void *ctx, int32_t x, int32_t y, int32_t lv,
                            int32_t tx, int32_t ty, int32_t tl) {
	struct game *g = ctx;                      /* g->cost: an sx_areas_cost_ctx */
	return sx_areas_move_cost(&g->cost, x, y, lv, tx, ty, tl);
}
```

### The blocks in C and JavaScript

```c
sx_squad_config config;
sx_squad_config_default(&config);          /* 8 members, 32 facts, 8 areas, 16 inbox, 8 dangers, Euclid */
config.metric = SX_METRIC_CHEBYSHEV;
sx_squad *sq = sx_squad_new(&config);
sx_member m = { .id = 1, .roles = SX_ROLE_RANGED, .skill = 192 };
sx_squad_add_member(sq, &m);
sx_squad_set_leader(sq, 1);
sx_squad_set_skill(sq, 192);
sx_squad_add_flank(sq, 3, 4);              /* also _rush, _formation, _ambush, _focus, _isolate, _fall_back */
uint32_t plan_id;
int32_t st = sx_squad_check(sq, now, NULL, 0, spots, n_spots, seed, &plan_id);
sx_order due[8];
int32_t n = sx_squad_orders_due(sq, now, due, 8);
```

```js
const sq = lib.squad({ members: 8, memory: 16, areas: 4, inbox: 8, dangers: 8, metric: 'chebyshev' });
sq.addMember({ id: 1, pos: [0, 0, 0], roles: ['ranged'], skill: 192 });
sq.setLeader(1); sq.setSkill(192); sq.setTargetKind(ENEMY);
sq.addTactic(lib.tactics.flank(3, 4));
sq.observe(now, { subject: 9, kind: ENEMY, pos: [20, 0, 0], seenAt: now });
const { status } = sq.check(now, { spots, seed });        // 'keep' | 'replan' | 'signal' | 'noPlan'
for (const order of sq.ordersDue(now)) give(order);
```

The tactics are data: `lib.tactic(...)` / `sx_tactic_new` build your own from
slots (role bits, count, place, goal kind, start), scoring considerations
over the row (`SQUAD.inLosses`, `inTargetDist`, ...), abort rules and a
trigger (C `sx_tactic_trigger_none`, `_all_arrived`, `_target_in_radius`,
`_manual`; JavaScript `tactic.triggerNone()`, `triggerAllArrived(maxWait)`,
`triggerTargetInRadius(radius)`, `triggerManual()`). `validate` finds a
tactic that reads a column the row does not have, or one with more than 255
slots. A slot's `gather_dist` (C `sx_slot.gather_dist`, JS `gatherDist`,
which defaults to `dist` there) is the distance of its gather place.

**JavaScript note:** the group wrappers (`Squad`, `Keeper`, `Crew`, tactics,
`gather`) throw on a time outside `0 .. 2^32 - 1`, an id or skill out of range
and so on, as every wrapper now does (see the Conventions).

### Your own pieces

The built-in behaviour fits many games, but some rules are yours: what a
character can see, what a hand sign costs, when an ambush strikes. Instead of
forking the library, a squad calls your code at nine fixed points, the
**slots**. All nine are methods of one trait, `SquadRules` (Rust), or fields
of one struct, `sx_squad_rules` (C). Every slot has a default that does what
the squad does without rules, so you write only the ones you need.

| # | Slot | Asked | Default |
|---|------|-------|---------|
| 1 | `keep_target(view, fact)` | per known target, in knowledge order | keep every target |
| 2 | `columns(view, ctx, out)` | per candidate row, fills the rule columns | writes nothing (the columns stay 0) |
| 3 | `place(id, ctx)` | the wish point of a `Place::Custom(id)` | the leader's position |
| 4 | `member_cost(ctx)` | per slot and member, before assigning | `ctx.built_in`: the distance to the wish point |
| 5 | `spot_score(ctx)` | per member and spot within the spot radius | `ctx.built_in`: the built-in spot score |
| 6 | `trigger(id, view, now)` | in a check, for a `Trigger::Custom(id)` | never fires |
| 7 | `abort(id, view, ctx)` | per row, for an `Abort::Custom(id)` | never fires |
| 8 | `channel_delay(id, ctx)` | when an order goes over a `Channel::Custom(id)` | never arrives |
| 9 | `report(view, msg, now, actions)` | per report of a kind the core does not know | drops it |

The order is fixed, never a hash order, and no slot gets a random number: in
a plan 1, then 2 and 7 per candidate, then 3, 4 and 5; in a check first 9
(and its actions), then 2 and 7 of the active row, the candidates (and a
replan), last 6. Slot 8 runs whenever an order is sent. The full order is in
the docs of `group::rules` (Rust) and above `sx_squad_rules` in `sagax.h`.
Costs and scores are `i128` in Rust and `int64_t` in C, lower is better, ties
go to the lower member or spot index.

**Lending.** The game owns the rules object and lends it to one call; the
squad and the `Crew` never store it, so both stay `Clone` and `Debug`. Rust:
`squad.plan_with(.., &mut rules)`, `check_with`, `signal_with`,
`reinstate_with`, `report_arrives_at_with`, and on a crew `tick_with`,
`reinstate_with` and `report_with`. C: `sx_squad_plan_with` and the other
`_with` functions of squad and crew, taking a `const sx_squad_rules *`. A
**`NULL` callback** is the default of its slot, and
`sx_squad_rules_default(&rules)` gives a table with every pointer `NULL`. A
slot must not call into the squad it came from; it reads the squad through
the **view** (`SquadView`, C `sx_view_*`): members, targets, areas, the
centre, the leader, the tactics and the active plan (C
`sx_view_active_plan`), valid only inside that call.

**Custom ids.** A game's own pieces are named by numbers you choose, per
slot: `Place::Custom(id)`, `Trigger::Custom(id)`, `Abort::Custom(id)` and
`Channel::Custom(id)` in Rust; in C `SX_PLACE_CUSTOM` with the id as the
place value, `sx_tactic_trigger_custom`, `sx_tactic_add_abort_custom` and
`sx_squad_set_channel_custom`. The core never reads an id; your slot gets it
and decides.

**Reports.** A report of a kind the core knows (1 goal broken, 2 enemy, 4
arrived) is handled by the core; every other kind goes to slot 9. Start your
own kinds at `REPORT_GAME_FIRST` (`SX_REPORT_GAME_FIRST`, 16): the kinds
below may gain a meaning later. The slot answers with **report actions**, at
most four per report (`MAX_REPORT_ACTIONS`): add a danger area, give the
signal, forget a target (`ReportActions::add_area`, `signal`, `forget`; C
`sx_report_add_area`, `sx_report_signal`, `sx_report_forget`). The squad
applies the actions of all reports of one call at **one fixed point**, after
the last report and before any abort, so a later report's view does not yet
show an earlier one's actions. A signal action is given only if no abort
fires and the target is not lost in the same call.

**Needs rules.** A squad with a custom place, trigger, abort or channel, or
with rule columns (`SquadConfig::rule_columns`), cannot run without its rules.
`Squad::needs_rules()` says so. The plain `plan` and `check` return
`PlanError::NeedsRules`, the plain C calls `SX_ERROR_NEEDS_RULES` (-4); the
plain `orders_due`, `arrived` and `reinstate` do not activate a due plan (the
next call with rules does) and `signal` answers false. Nothing is lost
silently. Rule columns follow the extra columns in a row, from
`Squad::rule_start()`; the game's own columns start at `Squad::extra_start()`
(C `sx_squad_extra_start`): 8 with the default two roles, one more for each
role beyond two. `IN_EXTRA` is right only with at most two roles; do not use
it.

**Hex.** A `Crew` with a hex layout converts for the rules too: everything a
slot sees (contexts, the view, a report's `pos`) is in your offset
coordinates, and a position a slot returns (a place, an area from a report)
is taken as offset. The `built_in` values and distances stay as the squad
computed them (hex distances are the same in both systems).

**Helpers.** `BuiltInRules` is the rules object with every default. Inside a
slot, `ctx.built_in` is the default answer to add to or replace
(`CostCtx::built_in`, `SpotCtx::built_in`), and `spot_score_builtin(wish,
want_tags, spot, metric)` computes the spot score by hand (C
`sx_spot_score_builtin`). In C, `sx_builtin_member_cost` and
`sx_builtin_spot_score` are the two defaults as callbacks to wrap; they
return `built_in` saturated to `int64_t`, so for exact built-in behaviour
leave the pointer `NULL`. Because of that saturation every spot that misses
a wanted tag has a C `built_in` of `INT64_MAX`; `sx_spot_ctx` also hands
over the two parts, `miss` and `distance`, so a C callback that changes the
score can keep the built-in order, for example with
`(miss ? INT64_C(1) << 62 : 0) + distance + penalty`. For a member cost by the way walked there is
`path_cost` (Rust `sagax::rules::path_cost`, and `PathCost`, a rules object
that does only that) and C `sx_path_cost(nav, map, from, to, budget, &cost)`:
they run a search and return the cost, or nothing when there is no way.

**Spot scores and tags.** The built-in spot score is `(miss << 100) |
distance`: a spot that misses a wanted tag always scores worse than *every*
spot that has all of them, whatever the distance. So whatever penalty you add
in `spot_score` cannot lift a spot over a missing tag; it only orders spots
within the same tag class. If your penalty is to matter (a spot the enemy can see), give the
candidate spots the tags the tactic wants, as the example does (it tags all
its spots hidden), or replace the score instead of adding to `built_in`.

**The recipe: `ambush_rules`.** Four guards lie in wait for an intruder on a
map with two walls. Nothing of the library changes: the game hands the crew
one rules object with five pieces, all built on a small sight test of its
own (Bresenham over the walls).

- `spot_score` adds a large penalty to a spot the intruder can see, so the
  guards hide;
- `channel_delay` makes hand signs arrive one tick later, and only in sight (a
  guard behind the short wall never gets one);
- `trigger` gives the signal when the intruder steps into the kill zone;
- `report` takes the game's own kind "a guard was seen" and records it in the
  rules object;
- `abort` ends the ambush once it was seen, and the squad plans a rush.

The shipped ambush scores the group-to-target distance on a curve of 0 to
100. On a small map that loses to the rush everywhere, so the ambush is never
picked. The example therefore builds its own consideration (`IN_TARGET_DIST`
on a linear curve of 0 to 16) as part of the recipe; set the end of the curve
for your map.

Rust: `cargo run -p sagax --example ambush_rules`
(`crates/sagax/examples/ambush_rules.rs`). C: `examples/c/ambush_rules.c`,
the same two scenes, the same printed lines (the checks diff them). The
integration is one rules object; the rest is the game's own sight test:

```rust
struct AmbushRules { walls: DenseGrid, intruder: Pos, seen: bool }

impl SquadRules for AmbushRules {
    fn spot_score(&mut self, ctx: &SpotCtx) -> Option<i128> {
        let seen = sees(&self.walls, self.intruder, ctx.spot.pos);
        Some(ctx.built_in + if seen { SEEN_PENALTY } else { 0 })
    }
    fn trigger(&mut self, id: u32, _view: &SquadView<'_>, _now: u32) -> bool {
        id == KILL_ZONE && in_kill_zone(self.intruder)
    }
    // channel_delay, report and abort the same way
}
// per tick: crew.tick_with(now, &[], &spots, seed, &mut rules, &mut events)
```

```c
static int32_t cb_spot_score(void *user, const sx_spot_ctx *ctx, int64_t *score)
{
	const struct ambush_rules *r = user;
	int64_t penalty = sees(r, r->intruder, ctx->spot.pos) ? SEEN_PENALTY : 0;
	/* saturating add: built_in can be INT64_MAX */
	*score = ctx->built_in > INT64_MAX - penalty ? INT64_MAX : ctx->built_in + penalty;
	return 1;
}
/* ... */
sx_squad_rules table;
sx_squad_rules_default(&table);            /* every callback NULL */
table.spot_score = cb_spot_score;          /* and channel_delay, trigger, report, abort */
table.user = &rules;
sx_crew_tick_with(crew, now, NULL, 0, spots, n_spots, seed, &table, events, cap, &total);
```

**JavaScript.** JavaScript has no callbacks into the squad: the rules object
is Rust and C only, and JS pieces will follow only on demand. A JavaScript
squad therefore never needs rules. What works there are the data parts of
this round: `roles` in the squad config, `squad.extraStart()`, a skill table
with `rows`, abort comparisons (`tactic.addAbortWhen`, `abortIfTargetLost`),
`squad.decider()` and the score log (`setScoreLog`, `scoreLog`).

### Debugging the plan

For the why of every decision, trace the crew or the squad: `Crew::tick_traced`,
`Squad::plan_traced` and `sx_crew_set_trace`, `sx_squad_set_trace` (see
[Debugging and logging](#11-debugging-and-logging)).

`validate` before the first tick, then watch the events (or `Status`) and
`active_plan()` / `pending_plan()` (tactic, target, think time, when it
became active). The example `squad_walk` prints the whole sequence:
sighting, plan, orders by radio, arrival at the gather spots, signal,
attack.

## 10. Missions

A squad (section 9) hands goals to its members. A **mission** is what one
character does between such goals, or without any squad: guard a place,
hunt a subject, get somewhere, and keep doing it sensibly when things go
wrong. It is a small planner with five ideas:

- **Motive**: what the character wants. `Motive::guard_place(pos)`,
  `guard_subject(id)`, `defeat(id)`, `defeat_enemies()`, `reach(pos)` or
  `custom(kind)`. The motive gives the mission its default plan.
- **Plan**: a list of **steps**, run once or in a loop (`Plan::once`,
  `Plan::looping`). A step is "walk to", "wait", "attack", and so on. A step
  or the whole plan can carry up to four **fail conditions**
  (`fails_when`): a subject is unknown or defeated, a fact has certain bits,
  the game's flags match, or the game's own condition says so. When one
  fires, the plan **breaks**.
- **Leeway**: how freely the character acts (`Leeway::STRICT`, `FLEXIBLE`,
  `FREE`). *Strict* never takes another plan: when its plan breaks it holds
  where it stands. *Flexible* may **replan**: it takes the best of the
  playbook plans and the motive's default plan. *Free* may also **widen**:
  fall back to a built-in plan that calls for help and then does the default.
- **Detour**: a short break from the plan that the game starts from its own
  perception: `Detour::Engage(subject)` or `Detour::Flee { .. }`. The plan
  waits, and when the detour ends the character walks back to where it left
  the plan and goes on with the step it left.
- **Leash**: how far and how long a detour may take the character from its
  anchor (`Leash { distance, ticks, on_lost }`). An engage of a subject
  farther away than the leash distance is refused (`detour` returns
  `false`), so a guard does not run back and forth at the edge of its leash.
  `on_lost` says what happens when the subject is lost: return, search for a
  while, or keep going.

**The mission moves nobody.** Each tick you give it the time, the
character's position, the character's own `Knowledge` and a `u64` of
condition bits (`flags`); it gives back one `Action` (go to, wait, follow,
hold, attack, search, wander, flee or your own) and the `Event`s of that
tick (step done, looped, broken, replanned, detour started and ended,
signal, call, squad wish). A call (`Event::Call`) names the channel, the
kind, where the caller stands and its motive, so whoever answers knows what
the caller is after. The game walks and fights, and tells the mission
when a custom step is over (`finish_step`). A step that is done hands over to
the next within the same tick, but at most one step change happens per tick,
so a plan of many `Signal` steps needs several ticks.

```rust
use sagax::prelude::*;

let mut mission = Mission::new(MissionConfig::default(), Motive::guard_place([4, 3, 0]))?;
mission.set_leeway(Leeway::FLEXIBLE);
mission.set_plan(&Plan::looping(&[
    Step::go_to([4, 3, 0]),
    Step::wait(5),
    Step::go_to([12, 3, 0]),
]))?;

// every tick, for this character:
let out = mission.tick(now, pos, &knowledge, flags);
match out.action {
    Action::GoTo(p) | Action::Hold(p) => walk_toward(p),
    Action::Attack { pos, .. } => shoot_at(pos),
    _ => {}
}
for event in out.events() {
    if let Event::Signal(n) = event {
        game_signal(*n);
    }
}
```

### No cheating

The mission sees only the `Knowledge` you pass. A subject whose fact is not
in it is unknown: its position cannot be read, a `Fail::Unknown` fires, an
`Engage` of it is refused, and a step that needs its position is
impossible. Put into a character's `Knowledge` what *that character* has
seen or been told (section 7), not what is true in the world. All positions
come from facts of one kind, `MissionConfig::subject_kind`; a fact whose
`data` has `enemy_bits` set is an enemy, `defeated_bits` a defeated
subject. A subject not seen for `lost_after` ticks counts as lost.

### Taking over a script

A game with scripted characters (a tactics game's enemy scripts, say) can
turn each script into a plan, and gain the replanning and the detours for
free. The usual commands map like this:

| Script command | Step |
|---|---|
| walk to (x, y) | `Step::go_to(pos)` |
| walk to an object | `Step::go_to_where(Where::Subject(id))` (to follow it: `Step::follow(id)`) |
| wait N ticks | `Step::wait(n)` |
| pause (the game's usual pause) | `Step::pause()`, length from `MissionConfig::pause_ticks` |
| enter a vehicle, use a switch, any act the mission does not know | `Step::custom(kind, Where::Fixed(pos), param)`; the game does it and calls `finish_step()` when done |
| wait for an object to come near | `Step::wait_for(id, radius)` |
| search an area | `Step::search(Where::Fixed(pos), radius, ticks)`: goes to the points of `MissionConfig::search` in turn |
| wander | `Step::wander()`: any `Wander` step ends the plan (later steps never run, a looping plan with one emits `PlanDone`); the character keeps wandering until a condition of the plan fires |
| attack an object | `Step::attack(id)` |
| mission won / lost, trigger something | `Step::signal(n)`: an `Event::Signal(n)` for the game to act on |
| call for help | `Step::call(channel, kind)` |
| ask for a squad tactic | `Step::squad(tactic, Where::Subject(id))` |
| loop | `Plan::looping(&steps)` instead of `Plan::once(&steps)` |
| "give up when X" | `.fails_when(Fail::..)` on the step or the plan |
| "skip it if it cannot be done" | `.skip_if_impossible()` on the step |

A step whose place is not known (a subject not in the knowledge) is
*impossible*: it breaks the plan, or with `skip_if_impossible` counts as
done. A plan holds at most `MissionConfig::max_steps` steps.

### Recipe: strict and flexible guard

Two guards walk the same patrol and have the same motive. One is on a short
leash and never changes its mind; the other chases farther and takes another
plan when the item it guards is stolen. They are the guards of the example
`mission_guard` (`cargo run -p sagax --example mission_guard`; C:
`examples/c/mission.c`).

```rust
let patrol = Plan::looping(&[Step::go_to(WEST), Step::wait(5), Step::go_to(EAST)])
    .fails_when(Fail::Fact { subject: ITEM, kind: STOLEN, mask: 1, value: 1 });

let mut a = Mission::new(config.clone(), Motive::guard_place(WEST))?;
a.set_leeway(Leeway::STRICT);           // leash 3, never replans
a.set_plan(&patrol)?;

let mut b = Mission::new(config, Motive::guard_place(WEST))?;
b.set_leeway(Leeway::FLEXIBLE);         // leash 12, searches, replans
b.set_plan(&patrol)?;
b.add_playbook(&Plan::once(&[Step::attack(THIEF)]), ONE, false)?;
```

When the attacker comes within sight, the game puts its fact into that
guard's own `Knowledge` and calls `mission.detour(now, Detour::Engage(id),
&knowledge)`. A strict guard follows three cells and walks back; the
flexible one chases, loses sight, searches, returns and resumes the step it
left. When the game tells guard B (only B) that the item is stolen, the
patrol breaks on the fact; B scores its playbook and takes "attack the
thief". The strict guard, if told, would only hold where it stands. A
flexible character that holds after a fallback (nothing could be taken)
looks again each tick and takes a plan as soon as one wins, say once it
learns of an enemy; a playbook plan that just broke is no candidate on that
same tick, so another plan gets its turn. The
second number of `add_playbook` is the plan's **preference** (0 to `ONE`);
with `widening` set, only a character that may widen takes the plan.

### Recipe: score plans by a trial run

The built-in score of a candidate plan is feasibility times preference. A
game that can simulate (a path cost, a risk estimate, a dry run of the
moves) replaces it in the `score` slot of `MissionRules`. The slot is asked
once per candidate, in option order; the option is the playbook slot
(`add_playbook` returns it), or `default_option()` and `widen_option()`.
The mission does not hand over the plan; keep a table from option to plan on
the game's side.

```rust
struct TrialRun<'a> {
    world: &'a World,
    plans: Vec<(u32, Plan)>,        // playbook option -> plan, kept by the game
}

impl MissionRules for TrialRun<'_> {
    fn score(&mut self, ctx: &PlanCtx, built_in: u32) -> Option<u32> {
        if !ctx.feasible {
            return Some(0);                      // never take what cannot run
        }
        let (_, plan) = self.plans.iter().find(|(o, _)| *o == ctx.option)?; // else: built-in
        let cost = self.world.dry_run(ctx.pos, plan)?;   // None: keep the built-in score
        Some(built_in / (1 + cost))                      // cheaper is better
    }
}

let out = mission.tick_with(now, pos, &knowledge, flags, &mut TrialRun { world: &world, plans });
```

`Some(0)` vetoes a candidate. The answer is final, also for a candidate that
is not feasible (so the example returns 0 for it). When every candidate ends
at 0 the mission falls back to its default plan if that is feasible, else
holds where it stands. Veto also a plan you know cannot run: the built-in
feasibility looks only at the first step and its position, and a plan that
breaks at once would be taken again on the next look. The slot gets no
random number and must not call the same mission. A game that only wants to *add* a factor puts it in the
`columns` slot instead (set `MissionConfig::extra_columns`) and a
consideration on the options of `mission.decider_mut()`. The `fail` slot
answers a `Fail::Custom(id)` of the game's own; while a replan checks a
candidate, `FailCtx::candidate` is its option (`None` for the running
plan). `PlanCtx` and `FailCtx` are in the prelude, the mission's
`BuiltInRules` as `MissionBuiltInRules`.

### Why did it take that plan?

`Mission::tick_traced` is `tick_with` with a `Sink` of `MissionTrace`
records, the same pattern as `Decider::decide_traced`. Each replan scoring
sends, per candidate in option order, its row (`Column { option, column,
value }`, after your `columns` slot), the decider's own records of it
(`Decide { option, event }` with a `DecideTrace`: its factors and its
score), and its score after your `score` slot (`Rescored { option,
built_in, score }`: `built_in` is the decider's), then the winner (`Picked {
option, score }`, `None` when all scored 0 and the fallback applies). Each
step that begins sends `Begin { index, at }` with the place its `Where`
stands for. A character holding after a fallback may score twice in one
tick. The actions and events are the same with or without a sink, and
`NoopSink` (what `tick` and `tick_with` use) costs nothing. All sinks and
records: [Debugging and logging](#11-debugging-and-logging).

```rust
use sagax::debug::{Collector, MissionTrace, Overflow};

let mut log = Collector::new(512, Overflow::DropOldest);
let out = mission.tick_traced(now, pos, &knowledge, flags, &mut MissionBuiltInRules, &mut log);
for r in log.records() {
    if let MissionTrace::Rescored { option, built_in, score } = r.event {
        println!("plan {option}: decider {built_in}, final {score}");
    }
}
```

### C

```c
sx_mission_config config;
sx_mission_config_default(&config);
config.subject_kind = SEEN;

sx_motive motive = { .kind = SX_MOTIVE_GUARD, .has_place = 1, .place = {4, 3, 0} };
sx_mission *m = sx_mission_new(&config, &motive);

sx_leeway leeway;
sx_leeway_preset(SX_LEEWAY_FLEXIBLE, &leeway);
sx_mission_set_leeway(m, &leeway);

sx_step steps[2] = { go_to(WEST), go_to(EAST) };   /* your helpers: a zeroed sx_step is GO_TO (0,0,0) */
sx_mission_set_plan(m, steps, 2, 1 /* repeat */, NULL, 0);

sx_mission_out out;
sx_mission_tick(m, now, pos, knowledge, flags, &out);   /* or sx_mission_tick_with */
if (out.action.kind == SX_ACTION_GO_TO)
    walk_toward(out.action.pos);
for (uint32_t i = 0; i < out.event_count; i++)
    if (out.events[i].kind == SX_MISSION_EVENT_SIGNAL)
        game_signal(out.events[i].a);
sx_mission_free(m);
```

The rules are `sx_mission_rules` (callbacks `fail`, `columns`, `score`, all
optional) lent to `sx_mission_tick_with`. `sx_mission_detour` returns 1 if
the detour was taken and 0 if it was refused. A `SX_MISSION_EVENT_CALL`
carries the caller's motive in `motive`.

`sx_mission_motive` and `sx_mission_leeway` read back what is set;
`sx_mission_plan` reads the plan that runs (or ran last, also a playbook,
default or fallback plan) in the form `sx_mission_set_plan` takes:

```c
sx_mission_plan_info info;
sx_step steps[16];
if (sx_mission_plan(m, &info, steps, 16) == 1)       /* 0: no plan yet */
    show_plan(steps, info.step_count < 16 ? info.step_count : 16, info.repeat);
```

`sx_mission_set_trace(m, on_trace, ctx)` sends every record of a tick to a
callback, one `sx_mission_trace` at a time (`kind` is
`SX_MISSION_TRACE_COLUMN`, `_DECIDE` with the decider's record in `decide`,
`_RESCORED`, `_PICKED` or `_BEGIN`; `NULL` turns it off), as
`sx_decider_set_trace` does for a decider:

```c
static void on_trace(void *ctx, const sx_mission_trace *e) {
	if (e->kind == SX_MISSION_TRACE_RESCORED)
		log_plan(ctx, e->option, e->built_in, e->score);
}
sx_mission_set_trace(m, on_trace, my_log);
```

JavaScript has no mission binding yet.

### Hex

With `MissionConfig::hex_layout` (C `config.grid = SX_GRID_HEX_*`, with
`SX_METRIC_HEX`; in Rust the metric must be `Metric::Hex`, else
`Mission::new` fails with `HexNeedsHexMetric`) every position **in and out**
is in offset coordinates of your map: the position you tick with, fixed
positions in steps, motive and detours, the actions and events you get
back, the rule contexts, and the positions of the facts in the `Knowledge`
you pass. The mission converts inside. Without a layout positions pass
unchanged. (The squad holds axial positions, see Hex in the Conventions; the
mission does not.)

## 11. Debugging and logging

One rule for every building block: **a trace is a parameter, not a setting.**

**Rust.** Every operation that decides something has an
`op_traced(.., sink)` form with the sink last; the plain `op(..)` is the same
call with a `NoopSink`, which costs nothing. No Rust object stores a trace.
Everything lives in `sagax::debug` (`use sagax::debug::*;`): the sink pieces
`Sink`, `NoopSink`, `CallbackSink` and `MapSink`, the `Collector` with
`Overflow` and `Record`, and the record types:

| Module | Record | Traced operations (next to the plain one) |
|---|---|---|
| search, flow, D* Lite | `SearchTrace` | `Search::advance`, `FlowField::build_traced`, `DStarLite::compute_traced` |
| `Navigator` | `NavTrace` | `find_path_traced`, `continue_search_traced`, `reachable_traced`, `flee_traced`, `output_traced`, `formation_traced` |
| smoothing, formation | `SmoothTrace`, `FormationTrace` | `smooth_path_traced`, `place_members_traced` (and `_hex_`) |
| `Decider` | `DecideTrace` | `score_traced`, `decide_traced` |
| `StateMachine` | `StateTrace` | `update_traced` |
| `Knowledge`, `Areas`, `Timers` | `KnowledgeTrace`, `AreasTrace`, `TimersTrace` | `learn_traced`, `merge_traced`, `expire_traced`, `remove_traced`, `add_traced`, `set_traced`, ... |
| `Outbox`, `Inbox` | `MessageTrace` | `post_traced`, `due_traced`, `push_traced`, `take_due_traced`, ... |
| spots, `GoalKeeper` | `SpotTrace`, `KeeperTrace` | `pick_spot_traced`, `broken_traced` |
| `Squad` | `SquadTrace` | `plan_traced`, `check_traced`, `observe_traced`, `report_traced`, `signal_traced`, `reinstate_traced`, `orders_due_traced` |
| `Crew` | `CrewTrace` | `tick_traced`, `observe_traced`, `report_traced`, `reinstate_traced` |
| `Mission` | `MissionTrace` | `tick_traced` |
| perception | `PerceptionTrace` | `observe_traced`, `look_traced`, `hear_traced` (and `learn_percepts_traced` with `KnowledgeTrace`) |

Where an operation has a `_with(.., rules)` form, `op_traced` takes the rules
too (pass the built-in rules for none); there is never an `op_with_traced`.
One exception: `reinstate_traced` takes `Option` rules, because the plain
`reinstate` differs from built-in rules (`None` is the plain call).
Setters, getters and constructors are not traced.

A `Collector` keeps the records of a call; read it after the call:

```rust
use sagax::debug::*;
use sagax::prelude::*;

let mut log = Collector::new(256, Overflow::DropOldest);   // or DropNewest
nav.find_path_traced(&mut map, start, &goal, Budget::UNLIMITED, &mut log);
for r in log.records() {                                    // oldest first
    if let NavTrace::Search(SearchTrace::Expanded { node, cost, .. }) = r.event {
        mark_cell(node, cost);
    }
}
println!("{} records, {} dropped", log.records().len(), log.dropped());
```

`CallbackSink` hands each record to your function as it happens instead.
Code that must compute something only for the trace asks `sink.enabled()`
first.

**Nesting.** A facade that drives other blocks wraps their records and adds
what they lack: `CrewTrace::Squad(SquadTrace)`,
`SquadTrace::Decide { tactic, target, event: DecideTrace }`,
`MissionTrace::Decide { option, event }`, `NavTrace::Search(SearchTrace)`. A
trace read alone tells the whole story of one call. `MapSink` is the adapter
(it also serves your own wrappers). A trace never changes a result: the
traced and the plain call answer the same.

**Event or trace?** `*Event` (`CrewEvent`, `MissionEvent`) is output the game
acts on and belongs to the normal API. `*Trace` is a debug record that only
goes through a sink; you never need it to play.

**C.** Every handle that decides something carries one callback:

```c
static void on_trace(void *ctx, const sx_squad_trace *t) {
	if (t->kind == SX_SQUAD_TRACE_PICKED) log_pick(ctx, t->tactic, t->score);
}
sx_squad_set_trace(squad, on_trace, my_log);   /* NULL turns it off (the default) */
```

`sx_<x>_set_trace(obj, fn, ctx)` returns 0 (`SX_ERROR_ARGUMENT` for a null
handle). Off it costs one check per call. The callback runs inside the call,
in record order. The record is valid only during the callback, so copy what
you keep. From the callback, call no function on the same handle, on the
handle that lent it (its parent) or on any handle it lent (see *Lent
handles* below): they share the state the running call is changing. A
record is a flat struct: `kind` first, then named fields (0 when they do not
apply to the kind); a wrapped sub-record is a field by value (`decide`,
`knowledge`, `areas`, `inbox`, `keeper`, `squad`, `search`).

| Handle | Set | Record | Kind codes |
|---|---|---|---|
| `sx_nav` | `sx_nav_set_trace` | `sx_nav_trace` | `SX_NAV_TRACE_*` (SEARCH, SHORTCUT, PLACED, UNPLACED, FLEE_BEST) |
| `sx_flow`, `sx_dstar` | `sx_flow_set_trace`, `sx_dstar_set_trace` | `sx_search_trace` | `SX_SEARCH_TRACE_*` |
| `sx_decider` | `sx_decider_set_trace` | `sx_decide_trace` | `SX_DECIDE_TRACE_*` |
| `sx_machine` | `sx_machine_set_trace` | `sx_machine_trace` | `SX_MACHINE_TRACE_*` |
| `sx_mission` | `sx_mission_set_trace` | `sx_mission_trace` | `SX_MISSION_TRACE_*` |
| `sx_knowledge` | `sx_knowledge_set_trace` | `sx_knowledge_trace` | `SX_KNOWLEDGE_TRACE_*` |
| `sx_areas` | `sx_areas_set_trace` | `sx_areas_trace` | `SX_AREAS_TRACE_*` |
| `sx_timers` | `sx_timers_set_trace` | `sx_timers_trace` | `SX_TIMERS_TRACE_*` |
| `sx_outbox`, `sx_inbox` | `sx_outbox_set_trace`, `sx_inbox_set_trace` | `sx_message_trace` | `SX_MESSAGE_TRACE_*` |
| `sx_squad` | `sx_squad_set_trace` | `sx_squad_trace` | `SX_SQUAD_TRACE_*` |
| `sx_keeper` | `sx_keeper_set_trace` | `sx_keeper_trace` | `SX_KEEPER_TRACE_*` |
| `sx_crew` | `sx_crew_set_trace` | `sx_crew_trace` | `SX_CREW_TRACE_*` |
| `sx_perception` | `sx_perception_set_trace` | `sx_perception_trace` | `SX_PERCEPTION_TRACE_*` |

`sx_nav` covers everything that runs through it: `sx_nav_find_path`,
`_continue`, `_reachable`, `_flee`, `_path_to`, `_way` (smoothing),
`_formation` and `sx_path_cost`. Its `search` numbers count every search of
the navigator (a flee runs two); a flow field counts its builds, a D* Lite
planner its computes.
The smoothing and spot records of the Rust table arrive inside these
records (a spot pick only inside `sx_squad_trace`). The header has the exact
fields of every record.

**Lent handles** (`sx_crew_squad`, `sx_crew_keeper`, `sx_squad_decider`,
`sx_squad_knowledge`, `sx_squad_areas`, `sx_mission_decider`) accept
`set_trace` too, and it covers only the calls made through that handle. What
the parent does with its parts goes to the parent's trace, wrapped: one place
per call, never two. A lent handle and its parent share state: a trace
callback of one must not call the other, nor another handle the same parent
lent (a squad trace callback must not read `sx_squad_knowledge`, a crew one
not `sx_crew_squad`). A crew counts its squad's lent handles as its own.

There is no C collector: the callback already gives you the copy, and the game
knows best what to keep (only `FINISHED`, only one member). The example
`examples/c/trace.c` keeps the last records of a search and a decision in a
small fixed-size ring.

**Untraced on purpose.** Pure functions and plain values whose result already
says everything: `route`, `distance`, `arrival`, `points_around`,
`sweep_heading`, `sine`, `line_clear`, `sight_line`, `in_sight`,
`percept_fact`, `alert_amount`, `flood_sound`, `Alertness`, `Progress`,
`Interval`,
the skill table, and the rule slots of the squad and the mission (what a slot
answered shows in the decision it fed, such as `Target.kept` or
`Abort.fired`).

**JavaScript.** JavaScript keeps its pull buffers: `grid.setTrace(true)` and
`grid.takeTrace()` for paths, flow fields and D* Lite (`{events, dropped}`,
each `{kind, x, y, parent, cost, estimate, expanded, found}`),
`decider.setTrace(true)` and `decider.takeTrace()` (`factor`, `scored`,
`chosen`), and `squad.setScoreLog(true)` with `squad.scoreLog()` for the last
scoring of a squad. The other records are Rust and C only.

Typical uses: colour the expanded cells to see why a search is slow, log the
`Finished` record with `expanded` to find costly searches, read the squad's
`Decide` records to see why it chose a tactic, or `Evicted` to see which fact
a full memory gave up.

## 12. Pitfalls

- Costs are integers ≥ 0. `move_cost` is an *extra* on top of the step cost.
- There is no default cost for a grid: pick a preset (Rust `GridRules::preset`,
  C `sx_rules_default`, JS `{ preset }`).
- `SX_ANY_LEVEL` (-1, Rust `ANY_LEVEL`) as goal level means any level; D*
  Lite needs an exact level.
- `SX_PARTIAL` is the best partial way, not an error.
- Lists are cut when the buffer is too small; `r.cells` still counts all.
- Flow fields and D* Lite assume moves that work both ways.
- A level outside `0 .. levels` is off the grid, not ignored.
- On hex grids the AI blocks use axial positions and the navigation uses
  offset cells; let the crew convert (`hex_layout`) or convert at the edge.
  `link` and `add_wall` take real hex neighbours only.
- One search at a time per `sx_grid`; do not share one between threads while
  searches run.
- JavaScript throws where it used to coerce: fractions, negative or huge
  times, zones above 31, a missing `preset`. `reachable(cell, r)` and
  `leaveCost` take whole numbers.
- Crew handles lent to C or JavaScript (squad, keepers) end with the crew; a
  keeper is stale after the next call that changes the crew.
- Perception: `SX_SENSE_FLAG_*` (profile bits) are not `SX_SENSE_*`
  (percept sense codes). A percept with `placed == false` has no position. Give
  `sx_perception_look` a cap of 4 × (`checks` + `instant_cap`) and
  `sx_perception_hear` at least 3, or percepts are lost.
- Load the library at run time, check `sx_api_version()`, and fall back to
  your own code if it is missing or too old.
