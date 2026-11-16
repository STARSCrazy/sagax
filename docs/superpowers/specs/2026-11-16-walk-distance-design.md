# Walking distance for the squad (0.16.0, C API 16) — design

Status: written by the controller; the owner picked this item from the
ROADMAP ("Walking distance for the squad") and said "do it", without
answering the four open questions of the 0.9.0 round. Every answer below is
a call made here, listed under "Calls made without the owner". Target
version 0.16.0, C API 16. JavaScript is not part of this round. Breaking
changes are allowed in this phase, but the default must behave exactly as
today.

## Intent

The squad (and the crew on top of it) measures distances in a straight-line
metric (Euclid, Manhattan, Chebyshev, hex). Behind a wall that is wrong: an
enemy two cells away through a wall but forty cells around it counts as
"close", so the squad picks the wrong member, the keeper thinks it is near
its goal, `IN_TARGET_DIST` says "in reach" and so on. Nav already knows the
real walking cost (flow fields). This round lets the squad ask for walking
distance instead of the straight line, wherever the question is "how far
does a figure have to walk".

## The four questions, answered

1. **Shape.** A trait in `sagax-decide` (which must not depend on nav):

   ```rust
   pub trait Distance {
       /// Walking distance from `from` to `to` in metric units (cells),
       /// or `None` when `to` cannot be reached from `from`.
       fn distance(&mut self, from: Pos, to: Pos) -> Option<u32>;
   }
   ```

   The squad's decisions take an optional `&mut dyn Distance` (builder's
   call on the exact plumbing: a parameter of the tick / decide call, or a
   setter holding a borrowed provider — prefer whatever keeps the squad free
   of lifetimes and heap, e.g. passing it into `tick`). `None` = today's
   metric, unchanged behaviour, byte-equal goldens.
   C: a callback `int32_t (*)(void *ctx, sx_pos from, sx_pos to, uint32_t
   *out)` (1 = reachable and `*out` written, 0 = unreachable) set on the
   squad/crew handle with `sx_squad_set_distance(sq, fn, ctx)` /
   `sx_crew_set_distance`; null clears it.
2. **Units.** The provider returns **cells** — the same linear unit the
   metric yields — so every existing threshold (`IN_TARGET_DIST`, ranges in
   the rules, keeper limits) keeps its meaning. Where the code compares
   squared Euclid distances, it squares the walking distance (saturating).
   The ready provider below converts nav cost to cells by dividing by the
   straight step cost (rounded down; 10 → 1 cell, a diagonal 14 → 1 cell
   plus 4 tenths carried in the sum, not per step).
3. **Unreachable.** Counts as infinitely far: out of every range, never the
   nearest, last in every ordering (deterministic ties as today). Never a
   fallback to the straight line (that would be the wrong answer the round
   fixes). A Decider column that carries a distance gets `i32::MAX`.
4. **Cost.** A ready provider in the facade, `FlowDistance`, caches flow
   fields **by destination**: a fixed number of slots (`new(slots,
   capacity)`, default 4), least recently used replaced, deterministic.
   A query `distance(from, to)` builds (or reuses) the field rooted at `to`
   and reads the cost at `from`. A squad usually asks many members about the
   same few destinations (its target, the keeper's goal), so a tick costs at
   most one search per new destination. `invalidate()` drops all fields
   (the game calls it when the map changes); no heap after `new` beyond the
   flow fields' own capacity. C: `sx_flow_distance_new(rules, slots,
   capacity)`, `_free`, `_invalidate`, and a ready callback
   `sx_flow_distance_fn` + context so `sx_squad_set_distance(sq,
   sx_flow_distance_fn, fd)` works with one line (the provider holds the
   `sx_map` it was given: `sx_flow_distance_set_map(fd, map)`).

## Where walking distance applies

The builder surveys every `distance(metric, ...)` use in `group/` (squad,
members, rules, spots, keeper, crew) and lists each with its decision in
the spec's "Builder calls". The rule: walking distance where the question
is how far a figure must **walk** (choosing the nearest member for a job,
`IN_TARGET_DIST`, keeper distance to its goal, spot/slot choice by
distance). The straight line stays where something else travels: **channel
delays** of shout/radio/signals (sound and radio are not walkers; the
ROADMAP listed channel delays, this is a deliberate change of that list),
view or formation geometry.

## Goals

1. `Distance` trait + plumbing through squad and crew; default unchanged.
2. Every walking question above uses it when set; unreachable = infinite.
3. `FlowDistance` (facade) with the cache, square and hex.
4. C: callback setters on squad and crew, `sx_flow_distance_*`, tests that C
   equals Rust.
5. Trace: the existing squad/crew traces show which distance was used
   (builder's call: a flag or a separate record when a walking distance
   decided something, or nothing new if the existing records already carry
   the distance value).
6. Example: extend nothing that has a golden; new example
   `walk_distance_scene` (Rust + C, "C output equals Rust" in
   `examples/c/run.sh`, golden test): a wall between a squad and an enemy;
   with the metric the member behind the wall is sent, with `FlowDistance`
   the one who can actually get there quickly. Prints both runs.

## Non-goals

- No JavaScript binding (on demand later).
- No walking distance inside perception, mission or contagion.
- No change of the path search itself.

## Anti-goals

1. Any change of behaviour or golden output when no provider is set.
2. Falling back to the straight line for unreachable targets.
3. A search per member per tick (the cache must serve repeated
   destinations).
4. Heap per tick in the core; panics across FFI.

## Done means

`cargo test --workspace -j 2`, `cargo clippy --workspace --all-targets --
-D warnings`, `cargo fmt --check`, `sh js/run.sh`, `CC=gcc sh
examples/c/run.sh` green; existing example outputs byte-equal to 323414e;
tests for every goal and anti-goal; USAGE section; CHANGELOG 0.16.0;
ROADMAP item moved to done; README if it lists features.

## Calls made without the owner

- All four answers above (trait / callback; cells; unreachable = infinite;
  LRU flow-field cache by destination, 4 slots).
- Channel delays keep the straight line.
- New example rather than changing an existing one.
- 0.16.0 / C API 16.

## Builder calls

- **Plumbing:** a tenth rules slot, `SquadRules::walk() -> Option<&mut dyn
  Distance>` (default `None`), and a combinator `Walking { rules, distance }`
  that lends a distance together with any rules. No extra parameter on
  `tick` / `plan` / `check`: every `_with` call of squad and crew walks, the
  squad stays free of lifetimes and heap, and the plain calls (built-in
  rules) are unchanged. For that `SquadRules` is also implemented for
  `&mut R` (so `Walking::with(&mut my_rules, d)` works).
- **`IN_TARGET_DIST`** walks as the **mean** walking distance of the active
  members to the target (rounded down), not from the group centre: the
  centre may stand in a wall or off the map's graph. Members that cannot
  get there do not count; none can: `i32::MAX`. Without a provider it stays
  the centre's metric distance (byte-equal). `Trigger::TargetInRadius`
  uses the same measure when walking.
- **`IN_TARGET_FRIEND_DIST`** and the friend of `Place::Between`: from the
  other target to the target; an unreachable friend is infinitely far
  (never picked as the friend: with only unreachable others the target is
  alone, as without a friend; the column `i32::MAX`).
- **Member cost:** member to the slot's first wish point. Unreachable is
  `UNREACHABLE` = 2^100, above every real cost but still assignable (the
  slot is filled last by such a member rather than left empty; a game that
  wants it empty uses `member_cost`). Euclid squares the walking distance.
- **Spots:** spot to wish point, for the spot radius and the built-in spot
  score; a spot that cannot reach the wish point is skipped.
- **Crew:** the arrive radius walks (the level check is kept); the keeper's
  distance through the new `GoalKeeper::inputs_with` and
  `Crew::member_inputs_with(id, now, &mut rules)`.
- **Straight line kept** for channel delays (as specified), headings, the
  wish points themselves, `nearest_threat` (what a target-less tactic
  faces: a direction, not a walk), the free function `gather` and the
  public free functions `pick_spot` and `spot_score_builtin` (no rules
  there; changing their signature had no use case).
- **Trace:** no new record and no flag. The existing records already carry
  the deciding value (`Assigned` cost, the spot's `Ranked` distance, the
  `Decide` inputs), so a trace shows the walking value when one was used.
  `FlowDistance` has `searches()` instead of its own trace.
- **C callback** takes `const int32_t *` positions (three values each):
  there is no `sx_pos` type in the header. Only a return of 1 counts as
  reachable; anything else is unreachable (a callback cannot fail loudly
  inside a decision).
- **Deviation:** `sx_flow_distance_new(rules, slots)` takes the capacity
  from `rules->node_capacity` instead of a third parameter, like
  `sx_flow_new`; 0 slots means 4. `sx_flow_distance_searches` was added
  (for tests and the example).
- **Deviation:** Rust `FlowDistance::new(topology, slots, capacity)` needs
  the topology, and the map is not stored: `distance(&mut map, from, to)`
  or `on(&mut map)` gives a borrowed `Distance`. 0 slots means 1 in Rust
  (the caller names a number) and 4 in C (0 is C's "default").
- **Hex:** the crew hands its rules offset coordinates, so it asks the
  distance in offset coordinates too (the map's cells, what `FlowDistance`
  wants); a plain squad on hex keeps axial ones and the game converts.
- **The field is rooted at `to` over the reversed graph** (review fix): a
  plain flow field from `to` measured `to -> from`, wrong on asymmetric
  maps (enter costs, zone leave costs, one-way walls). `sagax-nav` got a
  generic `Reversed` adapter (predecessors from a candidate list, the cost
  of the forward move; a candidate counts only if some node enters it, so
  off-map and blocked cells, which `DenseGrid` lets a walker leave, do not
  spread the search) with `Topology::predecessors` / `reversed`, and
  `FlowField::cost_from`. The distance equals the forward path search
  (`find_path` on the graph without a start cell; `start_step` is not
  used, the field serves every start); tested on random maps with zones,
  enter costs and one-way walls, square (8 and 4 ways) and hex.
- **Edges, decided against `find_path`:** a walker on a blocked cell
  (occupied by itself) walks off it, as `find_path` does (`cost_from`: the
  first move forward). A destination nobody can enter (the enemy's occupied
  cell) is unreachable for `find_path`; here it is walked up to: the field
  is rooted at the cells next to it and the distance is one cell more,
  because the squad asks for the enemy's own cell and an occupancy map must
  not make every enemy unreachable. `from == to` is 0 without a search.
- **Levels:** moves are looked for backwards from neighbours on the cell's
  own level and on `with_levels(lo..=hi)` (C `sx_flow_distance_set_levels`,
  default 0..=0); a stair from another level is otherwise not seen. The
  capacity bounds each field: a cell beyond it is unreachable (documented:
  size it for the farthest walk that should count).
- **Review fixes, small:** a slot is unclaimed while its field is built (a
  panic in the map leaves no half-built field behind a destination);
  `sx_crew_member_inputs` runs under the panic guard; the header says that
  `sx_cost_ctx.built_in` is the walking distance and `INT64_MAX` (never to
  add to) for an unreachable member, that `sx_spot_ctx.distance` walks, and
  that `sx_flow_distance_fn` wants map cells (a plain hex squad converts).
- **Cache thrash:** per-member destinations (keeper goals, arrival) need one
  slot each; with fewer slots than destinations per tick every question is
  a search. Documented ("give at least as many slots as destinations"),
  not auto-grown (no heap after `new`).
- **Example:** the crew runs one tactic (one runner to the enemy's front);
  the runner walks along its own flow field, one cell per tick. Both runs
  print the crew's events; the walking run also the search count.
