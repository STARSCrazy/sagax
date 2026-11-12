# Changelog

## 0.7.0

- Memory of the world, in `sagax-decide` (module `memory`): `Sightings`
  (table of known ids with place and time, oldest dropped when full),
  `Zones` (remembered areas, merge radius, lifetime, `blocks` for a map's
  `move_cost`), `Pursuit` (lost / stalled), `Ignore`, `Radio` with cooldown,
  `Interval`, `nearest`, and a deterministic search pattern (`search_point`,
  `look_heading`, `sin256`). Integer only, wrap-safe time, fixed sizes; every
  time and limit is a parameter. Rust `sagax_decide::memory`, C `sx_sightings_*`,
  `sx_zones_*`, `sx_ignore_*`, `sx_pursuit_*`, `sx_radio_*`, `sx_interval_*`,
  `sx_nearest`, `sx_search_point`, `sx_look_heading`, JavaScript
  `lib.sightings()`, `lib.zones()`, `lib.ignore()`, `Pursuit`, `Radio`,
  `Interval`, `lib.nearest()`, `lib.searchPoint()`.
- Example `officer` (Rust, with a reference test) and `examples/c/memory.c`;
  recipes in the guide (section 7).
- C API version 7. `SX_API_VERSION` is now 7, so the version check of a game
  built with the new header refuses an older library; the functions are
  additive and nothing was changed in the older ones.

## 0.6.0

- State machine with timers and aborts: states, transitions on condition
  bits (when / unless), a minimum time per transition, a timeout per state,
  transitions from any state. Rust `state::StateMachine`, C `sx_machine_*`,
  JavaScript `lib.machine()` with named states and conditions.
- Decisions remember actions: `record` what was done, hard cooldowns per
  option, and inputs for time since and count within a window (per option or
  per option and target) for soft dislike and penalties for repeating.
- C API version 6 (additive).

## 0.5.0

- Decisions, finer control: how factors are put together is chosen per
  decider and per option (product, geometric mean, minimum, average,
  median); a factor can stay outside as a true multiplier; a pull per
  factor makes it gentle or strict; options get ranks (emergency before
  combat); the current action can get a bonus against flipping. New curves:
  s-curve, inverse s-curve, root, cubic. Example `compare` and recipes in
  the guide. C API version 5 (additive: `sx_decider_add_weighted`,
  `sx_decider_set_combine`, `sx_decider_set_rank`, `sx_decider_set_current`).
  Rust: new `Curve` variants (breaking for exhaustive matches).

## 0.4.1

- Ways split their cost: JavaScript results carry `steps` (walking and
  terrain) and `zonesLeft` (zones left, e.g. attacks of opportunity) beside
  `cost`. Rust `DenseGrid::zones_left`.
- Foundry adapter: walker and enemies may be tokens or token documents.

## 0.4.0

- Decisions (utility AI), new crate `sagax-decide`: options with weights and
  considerations (raw input, curve, range), scored per pair of option and
  target in integer fixed point, compensated for the number of factors.
  Choice: best, or weighted / uniform among the top N with a seed from the
  caller. Trace per factor. Rust `Decider`, C `sx_decider_*` (API version
  4, additive), JavaScript `lib.decider()`.

## 0.3.0

- Several goals in one search: a way to the cheapest of them, e.g. to the
  nearest exit of a map. Breaking: Rust `find_path` takes a slice of goals
  (level `ANY_LEVEL` for any level, the `any_level` flag is gone), C
  `sx_nav_find_path` takes `goals, goal_count` (API version 3), JavaScript
  `findPath` takes a goal or an array.
- Fleeing: a way away from threats, to the cell the nearest threat needs the
  longest walk to reach. Rust `Navigator::flee`, C `sx_nav_flee`,
  JavaScript `grid.flee`.

## 0.2.0

- Tracing for debugging and logging: every step of a search (started,
  expanded, improved, paused, finished) for paths, areas, flow fields and
  D* Lite. Off by default and then free. Rust `Navigator::set_tracer`,
  C `sx_*_set_trace` (API version 2), JavaScript `grid.setTrace` /
  `grid.takeTrace`.

## 0.1.0

First public version: path search on square and hex grids with levels
(A*/Dijkstra with budgets, reachable areas, smoothing, formations, flow
fields, D* Lite), C interface (`sagax.h`, API version 1) and WebAssembly
with a JavaScript wrapper.
