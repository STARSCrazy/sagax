# Changelog

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
