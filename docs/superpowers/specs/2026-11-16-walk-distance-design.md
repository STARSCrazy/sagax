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
