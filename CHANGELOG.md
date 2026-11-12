# Changelog

## 0.8.0

- Group tactics, in `sagax-decide` (module `group`): a squad leader plans
  for a group, the members act on their goals with their own decisions.
  Integer only, deterministic, wrap-safe time, seeds from the game, fixed
  sizes.
  - `Squad`: member list, leader (the next active member takes over when it
    is lost, the plan stays valid), its own `Knowledge`, `Areas` and inbox
    (the leader plans only with what it saw or was told), tactic scoring per
    (tactic, target), planning with think time from skill, `check` (keep,
    replan, signal), `orders_due`, `signal`, `arrived`.
  - Orders with a channel per member (shout with a range, radio, none) and
    a delay from the leader's position. The signal lines up the starts
    (`AfterTicks`), so all members attack at the same tick.
  - Tactics as data (slots, scoring, abort rules, trigger) and shipped ones:
    rush, formation (line, wedge, column), flank (gather, then attack on
    signal), ambush, focus, isolate, fall back. They work with few or no
    spot tags (fall back to the nearest spot, then to the wish point).
  - `SkillTable`: what skill changes (which plan, how fast, how well, how
    long a goal is held). `GoalKeeper` for the member side, `discipline`,
    `suspend` / `resume` (the game's orders come first), `gather` (free
    characters of a faction within a radius).
  - `Areas::move_cost`, an adapter that turns areas into path costs.
- Rust `sagax_decide::group`; C `sx_squad_*`, `sx_keeper_*`, `sx_tactic_*`,
  `sx_gather`, `sx_discipline`, `sx_areas_move_cost`; JavaScript
  `lib.squad()`, `lib.keeper()`, `lib.tactic()`, `lib.tactics`,
  `lib.skillTable()`, `lib.discipline()`, `lib.gather()`,
  `lib.areasMoveCost()`. The JavaScript group wrappers throw on a time or
  value out of range; the older wrappers still coerce.
- Examples `squad` (a flank on a small grid: sighting, plan, radio orders,
  gather, signal, simultaneous attack) and `examples/c/squad.c`; section 8
  of the guide.
- C API version 8. `SX_API_VERSION` is now 8, so the version check of a game
  built with the new header refuses an older library; the functions are
  additive.

## 0.7.0

- Memory of the world, in `sagax-decide` (module `memory`), built from generic
  blocks that serve turn-based tactics, real-time strategy, stealth and
  action games alike. Integer only, no random numbers, wrap-safe time, fixed
  sizes; every time and limit is a parameter. The game decides what is
  perceived; the blocks keep, pass on and forget it.
  - `Knowledge`: facts keyed by (subject, kind) with place, observation
    time, source (own, heard, told), strength and data; evicts the oldest or
    the weakest; `learn`, `merge` (newer wins), per-kind expiry.
  - `Outbox`, `Inbox`, `route`, `arrival`: messages with a delay and a
    cooldown, recipients picked by `Nearest(k)`, `InRange(r)`, `All` or
    `To(id)` under a metric (Euclid, Manhattan, Chebyshev, hex), arrival
    times that grow with distance. Recipes: radio, shout, messenger, noise.
  - `Areas`: remembered or announced circles with a lifetime, blocking or
    with a cost per cell, for a map's `move_cost` (`blocks`, `cost`).
  - `Progress` (does a chase still get closer), `Timers` (ids that are on for
    a while), `Interval`.
  - `Alertness`: a value with stimuli, hold, decay, levels and hysteresis;
    the levels drive a state machine.
  - `points_around` (fine or coarse sine table), `sweep_heading`, `sine`.
- Rust `sagax_decide::memory`; C `sx_knowledge_*`, `sx_outbox_*`,
  `sx_inbox_*`, `sx_areas_*`, `sx_timers_*`, `sx_progress_*`,
  `sx_interval_*`, `sx_alertness_*`, `sx_route`, `sx_arrival`,
  `sx_points_around`, `sx_sweep_heading`, `sx_sine`; JavaScript
  `lib.knowledge()`, `lib.outbox()`, `lib.inbox()`, `lib.areas()`,
  `lib.timers()`, `Progress`, `Interval`, `Alertness`, `lib.route()`,
  `lib.arrival()`, `lib.pointsAround()`, `lib.sweepHeading()`, `lib.sine()`.
- Examples `officer` (the reference officer behaviour on the generic blocks,
  with a test) and `stealth_guard` (hex grid, alertness levels, shouts, lit
  cells), plus `examples/c/memory.c`; recipes in the guide (section 7).
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
