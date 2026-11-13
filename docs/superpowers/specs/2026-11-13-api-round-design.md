# API round (0.9.0, C API 9) — design

Status: draft for approval. Target version 0.9.0, C API 9.

## Intent

Sagax has strong building blocks, but the places where they meet are weak.
Path finding and the AI blocks use different position types and different
hex coordinates. The member side of group tactics is hand work: start
ticks, arrival detection and reports are all built by the game. Some names
and conventions differ between modules and between Rust, C and JS.

This round makes Sagax look like one library from the outside:

- easy for a newcomer (three typical jobs in a few lines each);
- still ideal for professionals (no hidden allocation, full control);
- not cryptic (no packed values, no catch-all parameters).

Nobody uses Sagax in a shipped game yet, so this is the cheapest moment to
break things. The round breaks on purpose, but keeps the old names as
deprecated aliases for one minor version wherever that is cheap.

## Goals

1. **Bridges** between navigation and the AI blocks, in all three
   interfaces.
2. **Helpers** that every game needs anyway: report constructors, goal
   start tick, named keeper inputs, typed setters.
3. **One convenience layer, the `Crew`.** It ties a squad, one goal keeper
   per member and the goal option glue together. One `tick` per frame
   returns what happened.
4. **One set of conventions** for names, return values, positions and
   errors, written down and applied to every interface.
5. **A beginner path in the docs:** "three jobs in five minutes" plus one
   end-to-end example per language where agents really walk with the
   `Navigator`.

## Non-goals

- No "world" or "agent" facade that owns the map, path finding or
  perception. "Your map, your rules" stays: the game keeps its map and its
  callbacks.
- No new AI features. Mission planner, perception, trace sink and "a member
  ignores its slot" stay on the roadmap.
- The `Crew` does not move anyone. It says what should happen; the game
  moves its figures.

## Anti-goals (a failure even if it works)

- Losing determinism, fixed capacities or "no allocation while searching".
- A convenience layer that hides what the blocks do, so a pro has to fight
  it. Every `Crew` step must be doable by hand with the blocks, as today.
- Silent meaning changes. A break must fail to compile, or fail loudly,
  never change behaviour quietly.
- Rust, C and JS drifting apart again.

## Conventions (the rules every interface follows)

Written once in USAGE ("Conventions") and in the header top comment.

- **Positions:** one position type for AI blocks, `Pos = [i32; 3]` (x, y,
  z). `z` is the level for map cells. Navigation keeps `Cell` / `sx_point`,
  with bridges to and from `Pos`. JS accepts both `{x, y, z}` and
  `[x, y, z]` everywhere (`z`/`level` optional, default 0) and returns
  `[x, y, z]` from AI blocks and `{x, y, level}` from navigation, as now.
- **Hex:** AI blocks measure hex distance in axial coordinates. A new
  metric `Metric::HexOffset(layout)` (C: `SX_METRIC_HEX_ODD_R` …, one
  value per offset layout) measures directly in the map's offset
  coordinates. Converters `to_axial` / `from_axial` exist in all three
  interfaces. USAGE warns at the first mention of hex.
- **Time and seed:** `now` is always the argument right after the handle /
  `self`; `seed` is always last. (Today this already holds in most calls;
  the outliers move.)
- **C return values:**
  - yes/no questions return 1/0;
  - a count or index returns it directly, negative = `SX_ERROR_*`;
  - scalar answers return directly when they fit, otherwise an out-param;
  - structs come through out-params;
  - lists: caller buffer + capacity, return the number written; an
    optional `uint32_t *total` out-param gives the number that exist;
  - errors are only `SX_ERROR_*` constants, never a bare "-1" in docs.
  - enum parameters are `uint32_t` everywhere.
- **C lifecycle:** `_new` / `_free` for every handle. `sx_nav_create` /
  `_destroy` (and flow, dstar) become `sx_nav_new` / `sx_nav_free`; the old
  names stay as macros for 0.9.
- **JS:** every wrapper validates and throws (`RangeError`, `TypeError`)
  instead of coercing. Every handle is guarded against use after `free()`.
  Handles also support `Symbol.dispose` where the runtime has it.
- **Rust:** fallible setters return `Result` or are `#[must_use]`; no bare
  ignored `bool`.

## Changes

### A. Navigation and AI meet

1. Rust: `impl From<Cell> for Pos`, `impl From<Pos> for Cell`, `Cell::pos()`.
   C: `sx_point_to_pos`, `sx_pos_to_point`. JS: accept both shapes (above).
2. `HexLayout::to_axial` / `from_axial` in C (`sx_hex_to_axial`,
   `sx_hex_from_axial`) and JS (`lib.hex.toAxial`, `fromAxial`).
3. `Metric::HexOffset(HexLayout)` in `sagax-decide` (needs only the layout
   enum; move `HexLayout` into `sagax-core` so `sagax-decide` does not
   depend on `sagax-nav`).
4. `Areas::cell_cost(to: Cell, start: Cell, goal: Cell)` plus the existing
   C adapter, documented as the one way to feed danger areas to the path
   search.
5. Optional walking distance for the squad: `Squad::set_distance(fn)` /
   C callback / JS function, used for `IN_TARGET_DIST`, keeper distances
   and channel delays instead of the straight-line metric. Default stays
   the metric.

### B. Navigation polish

1. **Budget:** Rust `enum Budget { Unlimited, Cells(NonZeroUsize) }` with
   `impl From<usize>` that panics on 0 in debug and maps 0 → `Unlimited`
   in release is *not* wanted (silent). Instead: `find_path(..., budget:
   impl Into<Budget>)`, `Budget::UNLIMITED`, and `usize` stops being
   accepted. C and JS keep "0 = unlimited", documented as such.
2. Rust `Report.status` becomes `Status` with a new `Status::Idle` instead
   of `Option<Status>`.
3. **One map trait in Rust:** `CellMap::step(from, dx, dy)` serves square
   and hex grids, as C already does. `HexMap` goes away; `Navigator` takes
   `M: CellMap`. A square-only game implements one trait.
4. **Ready grid in C:** export the dense grid that WASM already has:
   `sx_grid_new(w, h, levels)`, `sx_grid_free`, `sx_grid_set(x, y, level,
   cost)` (cost < 0 = blocked), `sx_grid_map(grid) -> sx_map`. JS grid gets
   levels and level links to match.
5. **Rule presets:** `sx_rules_default(grid_kind)`, Rust
   `GridRules::preset(Preset::Octile | Preset::Tabletop)`, JS
   `{ preset: 'octile' | 'tabletop' }`. Octile = straight 10, diagonal 14;
   tabletop = 1 / 1. Rust and JS defaults become the same (octile).
6. **Names:** Rust `Navigator::resume` → `continue_search` (C/JS already
   say `continue`).

### C. Group tactics: the member side

1. Named keeper inputs: Rust `keeper::IN_HAS_GOAL … IN_DISCIPLINE`, C
   `SX_KEEPER_IN_*`, JS `KEEPER.hasGoal …`. `inputs()` keeps its array but
   also gets a struct form `KeeperInputs { has_goal, dist, age, signal,
   discipline }` with `as_row()`.
2. `GoalKeeper::add_goal_option(decider, weight)` adds the standard goal
   option (has goal, closer is better, discipline as current bonus) to a
   member's `Decider` and returns the option index. Documented as the
   default way; a game can still build its own.
3. `Goal::start_tick(received)` and `Goal::awaits_arrival()`; C
   `sx_goal_start_tick`, `sx_goal_awaits_arrival`; JS same names.
4. `SkillTable::give_up_ticks_for(skill)` and siblings
   (`*_for(skill)` for every per-level field); C and JS same.
5. **Report constructors:** `Message::enemy`, `Message::danger(sender,
   centre, shooter: Option<Pos>, now)`, `Message::arrived(sender, plan)`,
   `Message::goal_broken(sender, plan)`; C `sx_report_*`, JS `Report.*`.
   The shooter position stops living in `strength`/`data`: the squad
   gets `report_danger(centre, shooter, arrive_at)` as a typed call, and
   the packed form is removed.
6. **Typed setters** instead of catch-all parameters:
   `set_shout(id, range)`, `set_radio(id, base, per_unit)`,
   `set_silent(id)`; `add_rush()`, `add_formation(shape, spacing)`,
   `add_flank(sides, dist)`, `add_ambush()`, … in C
   (`sx_squad_add_flank(sq, sides, dist)`); tactic triggers
   `trigger_all_arrived(max_wait)`, `trigger_target_in_radius(r)`,
   `trigger_manual()`. The generic `sx_squad_set_channel`,
   `sx_squad_add_shipped` and `sx_tactic_set_trigger` are removed (C API 9
   is a break anyway; keeping two ways is the confusing part).
7. **Names:** `Squad::resume` → `reinstate(id, now)` (puts a suspended
   member back and resends its goal); `GoalKeeper::resume` →
   `goal_won(now)`; `GoalKeeper::postpone` → `goal_postponed(now)`.
   `Group::resume` stays (it only flips state) but is documented as the
   low-level call. Nav and group `Status` keep their names in their own
   modules; the prelude exports them as `NavStatus` / `SquadStatus`.
8. `SquadConfig { members, memory, areas, inbox, metric }` with
   `Default`; `Squad::new(config)`. C: `sx_squad_config` struct +
   `sx_squad_config_default()`. JS already takes an object.

### D. The Crew (layer 2)

A `Crew` owns a `Squad`, one `GoalKeeper` per member and the delivered
goals. Fixed capacities; caller time and seed; no movement.

Per tick the game:
1. tells the crew where its members are (`set_pos` or a positions slice),
2. calls `tick(now, spots, seed, &mut events)`,
3. reads the events,
4. for each member, asks `member_inputs(id, now)` for its own `Decider`
   row and, after the member decided, calls `member_chose(id, goal_won,
   now)`.

`tick` does, in this order: squad `check`; deliver due orders into the
keepers; compute each order's start tick; detect arrival at a gather spot
(`awaits_arrival` + position equals goal pos, or within `arrive_radius`,
default 0) and report it over the member's channel (honest, with delay);
turn keepers that gave up into `goal_broken` reports.

Events: `Planned(PlanId)`, `Activated(PlanId)`, `Signal`, `Order { member,
goal, start_at }`, `Start { member }` (start tick reached), `Arrived {
member }`, `Broken { member }`, `Replan(PlanId)`, `NoPlan`.

Interfaces: Rust `Crew`, C `sx_crew_*` (events into a caller buffer, return
the number written), JS `lib.crew({...})` with `tick()` returning an array.

Every step stays available by hand (anti-goal "hides what blocks do"). The
squad and keepers are reachable from the crew (`crew.squad()`,
`crew.keeper(id)`).

### E. Rust ergonomics

1. A `sagax` umbrella crate re-exporting the others, with `sagax::prelude`.
2. `Decider::choose(&candidates, &rows, pick, seed) -> Result<Option<Choice>>`
   with an internal scores buffer; `decide` with the caller buffer stays
   for pros.
3. `#[must_use]` on every remaining `bool` return that means "refused".

### F. C details

1. Make the outliers follow the return rules (examples from the review:
   `sx_inbox_push`, `sx_knowledge_learn`, `sx_timers_left`,
   `sx_alertness_value`, `sx_areas_cost`, `sx_squad_targets`).
2. All enum parameters `uint32_t`.
3. Layout checks (`_Static_assert` on sizes and key offsets) inside
   `sagax.h` behind `#if __STDC_VERSION__ >= 201112L`, so users get them
   for free.

### G. JS details

1. Accept both position shapes; loose-number calls (`pathTo(x, y)`)
   also accept one position.
2. String constants everywhere a choice is made (`'ranged'`, `'cover'`,
   `'radio'`), numbers still accepted.
3. Every wrapper throws on bad input; every handle guarded; `Symbol.dispose`.
4. The pure-JS copies of `Progress`, `Interval`, `Alertness` and `arrival`
   call the WASM versions instead, so there is one implementation.

### H. Docs

1. README: quick look shows the three jobs (path, decision, squad), not
   only path finding; list all C examples.
2. New `docs/QUICKSTART.md`: three jobs in five minutes, each in Rust, C
   and JS, same order in every section (Rust, C, JS).
3. USAGE: a "Conventions" section first; same language order in every
   section; group section uses the `Crew` first, the blocks second.
4. One end-to-end example per language where guards / a squad walk with
   the `Navigator` and the `Crew` (replaces the hand-written step
   functions in the existing examples).
5. A migration table 0.8 → 0.9 in CHANGELOG (old name → new name).

## Constraints

- Integer only, caller time and seed, wrap-safe, documented tie-breaks.
- Fixed capacities; no allocation in searches; "no allocation after
  warm-up except one Vec per plan" stays the claim for the squad.
- Every block in Rust, C and JS; square and hex.
- Public repo: no game names, no private paths, English, author noreply.
- Identical results before and after the round for the same inputs
  (existing scene tests keep their tick numbers unless a test was wrong).

## Left to the builder

- Exact module layout of the umbrella crate and prelude content.
- Whether deprecated aliases are kept for a given rename (keep them where
  cheap; C macros, JS getters, Rust `#[deprecated]` re-exports).
- Internal structure of `Crew`.
- Order of work, as long as each step leaves the workspace green.

## Decisions after review

The designer's calls on the builder check (none needed the user).

1. **Hex stays axial inside the AI blocks.** `Metric::HexOffset` is
   dropped (A3): offset coordinates cannot be averaged or turned, and
   distance from deltas needs row parity. Instead:
   - converters in all three interfaces (A2);
   - `CrewConfig::hex_layout: Option<HexLayout>`: the `Crew` converts
     offset → axial on every position going in and axial → offset on every
     position coming out. `Squad`, `Knowledge`, `Areas` stay axial; a pro
     who uses them directly converts at the edge. Documented in USAGE
     "Conventions / Hex".
   - `HexLayout` still moves to `sagax-core` so `sagax-decide` can use it.
2. **Danger reports get their own typed path.** `Squad::report_danger(
   sender, centre, shooter: Option<Pos>, arrive_at)` stores the report in a
   small danger queue inside the squad (capacity from `SquadConfig`,
   delivered at/after `arrive_at` like the inbox). `REPORT_DANGER` and the
   packed `strength`/`data` form are removed; `Message` and `sx_message`
   stay unchanged. There is no `Message::danger`. C:
   `sx_squad_report_danger(sq, sender, centre, shooter /*null*/,
   arrive_at)`; JS `squad.reportDanger({sender, centre, shooter},
   arriveAt)`.
3. **Walking distance (A5) is deferred** to the roadmap. Shape, units
   (path cost vs cells), unreachable handling and cost need their own
   design. This round adds nothing for it.
4. **No silent default rules.** Rust `GridRules` loses its `Default`
   impl (compile error where it was used); `GridRules::preset(Preset::
   Octile | Preset::Tabletop)` replaces it. JS `lib.grid` throws when
   neither `rules` nor `preset` is given. C has no default today and gets
   `sx_rules_default(grid_kind, preset)`. So every game states its
   choice once, loudly.
5. **Crew events:**
   - `Planned(id)`: a new pending plan exists (first plan or replacement).
   - `Activated(id)`: that plan became active (think time over) — the
     squad gets a way to report this (`Squad::take_activated()` or a new
     `Status::Activated`, builder's choice).
   - `Signal`, `Order { member, goal, start_at }`, `Start { member }`.
   - `Arrived { member }`: emitted when the crew detects the arrival and
     sends the honest report; the leader learns it later.
   - `Broken { member }`, `NoPlan`. There is no separate `Replan` event.
   - `Crew::tick(now, extra, spots, seed, &mut events)` keeps the extra
     columns.
6. **JS value classes** (`Progress`, `Interval`, `Alertness`, `arrival`)
   stay plain JS objects with the same constructors; their arithmetic
   calls stateless WASM functions. No JS API break, no handle.
