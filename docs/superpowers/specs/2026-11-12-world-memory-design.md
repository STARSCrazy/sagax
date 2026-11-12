# World memory (decide, stage 3) — design

Status: draft for approval. Target version 0.7.0, C API 7.

## Intent

Game agents need to remember things about the world for a while: who they
saw doing something, where and when, which places are dangerous, and what a
colleague told them over the radio. Today every game writes this by hand.

The goal: a game with a hand-written "officer" AI (see the reference
behaviour below) can move most of it onto Sagax **without changing its
behaviour** — same numbers, same order, same tie-breaks. So the building
blocks must be generic, but exact enough to rebuild that behaviour
bit for bit.

Fun matters more than strength. Every time and limit is a parameter, so a
game can try "short memory" against "long memory" and keep what plays best.

## Goals

- Small, generic building blocks in `sagax-decide` (new module `memory`,
  next to `state`), each usable alone.
- Together with the stage-2 state machine and the decider they can express
  the reference officer behaviour.
- Same shape in Rust, C (`sx_*`) and WebAssembly/JS, like every Sagax part.
- Deterministic: time is a caller-given `u32` tick (frame, round); unsigned
  wrap-safe `now - since`; no random numbers; documented tie-breaks.
- Fixed size: capacity is set at creation; nothing grows or allocates while
  running.

## Non-goals

- No line of sight, hearing, path finding or perception inside these
  blocks. The caller decides "he sees X" and tells the memory. (Sight lines
  come later with the perception module.)
- No group tactics (rally point, flanking). That is the next stage.
- No planner, no neural net.
- No "fading" state that decays on its own. Uncertainty is derived from age
  (see below).
- No identity confusion: an entry is always keyed by the caller's id.

## Anti-goals (would be a failure even if it works)

- A block that only fits one game's officers (names like `police`,
  `suspect` in the API).
- Hidden global state, a global clock or a global RNG.
- Behaviour that cannot be reproduced exactly (float maths in decisions,
  unspecified iteration order).

## The building blocks

All positions are integer `(x, y, z)` (`i32`), in the caller's units.
"Age" means `now.wrapping_sub(since)`.

### 1. `Sightings` — table of known entities

Fixed capacity N. Entry: `id: u32`, `pos`, `last_seen: u32`, `data: u32`
(free for the caller, e.g. a flag "reported").

- `seen(id, pos, now) -> Option<u32>`: present → update pos and last_seen.
  Absent and full → drop the entry with the **largest age** (equal age → the
  earlier slot), compact keeping order, return the dropped id. Then append.
- `touch(id, now)` (only time), `set_pos(id, pos)` (only position, no time —
  e.g. a radio report), `remove(id) -> bool`, `get(id)`, `iter()` in slot
  order, `len`, `clear`.
- `expire(now, max_age, out)`: in slot order remove every entry with
  age `>= max_age`; report removed ids (callback / out-list).
- Capacity 1 is allowed and is the "one current target" case.

### 2. `Zones` — remembered areas

Fixed capacity N, a merge radius R, a lifetime L. Entry: `pos`, `since`,
`data: u32`, optional second point `source` (e.g. where the shot came from).

- `add(pos, now, data, source)`: the **first** zone with squared distance
  `< R²` (2D) is renewed: centre, `data`, `source` and `since` are all
  replaced by the new values. Otherwise append; if
  full drop the oldest first (equal → earlier slot).
- `expire(now)`: remove zones with age `>= L`.
- `contains(pos) -> Option<index>`, `iter()`.
- `blocks(cell_pos, start_pos, goal_pos) -> bool`: true if there is a zone
  Z that contains `cell_pos`, does not contain `start_pos` and does not
  contain `goal_pos` (any zone, checked in slot order). Blocking only, no
  extra cost. The caller turns a nav cell into its centre position, so it
  works for square and hex grids alike; the USAGE recipe shows how to plug
  it into the existing `move_cost`/passable callback. No change to
  `sagax-nav` itself.

### 3. `Pursuit` — following one target

Plain value type, one per agent.

- Fields: `target: u32` (0 = none), `last_pos`, `unseen: u32`,
  `best: i32`, `since: u32`.
- `begin(target, pos, distance, now)`, `seen(pos)` (unseen = 0),
  `missed(step)` (unseen += step — the caller may check sight only every
  k-th tick), `reset_progress(distance, now)`.
- `lost(limit) -> bool`: `unseen >= limit`.
- `stalled(distance, now, min_gain, limit) -> bool`: if
  `distance <= best - min_gain` then `best = distance`, `since = now`,
  return false; else return `now - since >= limit`.

### 4. `Ignore` — "leave this one alone for a while"

Fixed capacity (1 is enough for the reference). `add(id, now)`,
`ignores(id, now, duration) -> bool` (age `< duration`). Pure time based,
nothing is cleared by hand.

### 5. `Radio` — delayed message with cooldown

Plain value type, one per sender.

- `start(pos, now, cooldown) -> bool`: refused if a message is pending, or
  if the sender has sent before and `now - last_start < cooldown`. On
  success: pending, started=now, last_start=now, pos stored.
- `due(now, delay) -> bool`, `take() -> pos` (clears pending),
  `cancel()` (sender is gone → nobody hears it).
- `pending()`. The report rhythm ("again every 15 s while I still know
  him") is a separate small `Interval` helper: `due(now, every)` with a
  "never yet" state.
- Helper `nearest(candidates, point, k, shift) -> list`: up to k candidates
  nearest to `point`. Distance = `(dx >> shift)² + (dy >> shift)²` with
  **truncating division** (not shift — must match `x / 16` for negatives),
  z ignored, equal distance → the earlier candidate. Candidates are given
  by the caller in their order (only the free ones).

### 6. `Search` — deterministic search pattern

Pure functions, integer only.

- `search_point(agent_index, step, centre, pattern) -> pos`:
  heading = `agent_index * heading_step` (256 per turn);
  step 0 = arrival point at `first_distance` with the base heading; step k
  (1..n) uses `base + offsets[k-1]` (always added to the **base** heading,
  not to the previous step) and `distances[k-1]`.
  `x = centre.x + sin(h) * d / 256`, `y = centre.y + sin(h+64) * d / 256`,
  truncating, then clamp to a caller-given box.
- `sin256(h)`: the exact 17-entry quarter-wave table
  `{0,25,50,74,98,121,142,162,181,198,213,226,237,245,251,255,256}`,
  index `((h & 0xff) + 2) / 4 & 63`.
- `look_heading(base, elapsed, step_ticks, offsets)`: the "look around"
  sweep. Default offsets `{0,+32,+64,+32,0,-32,-64,-32}`, then 0.
- `pattern` holds all numbers, with a default that equals the reference.

### How they fit together (the officer recipe)

Not a new block, but an example and a test: a `StateMachine` with states
Idle, Walk, Look, Chase; the caller keeps "home" (the task before) and
restores it. Per tick: radio due → send to `nearest(free, 3)`; report
interval → radio start; sight every 4th tick (staggered by index) →
`Sightings.seen` / `Pursuit.seen|missed`; `lost` / `stalled` → `Ignore.add`
and search; search points with walk/look timers; `expire` once per tick.
The decider reads inputs such as age, distance to `last_pos`, `unseen`,
"known", "ignored".

## Interfaces

- Rust: `sagax_decide::memory::{Sightings, Zones, Pursuit, Ignore, Radio,
  Interval, nearest, search_point, sin256, look_heading, SearchPattern}`.
- C: tables as opaque handles `sx_sightings_new(capacity)` etc. (allocated
  once at creation, like `sx_machine_new`); the small value types
  (`Pursuit`, `Radio`, `Interval`, `Ignore` with capacity 1) as plain
  `#[repr(C)]` structs the caller owns, with free functions — so a C game
  can keep them inside its own fixed arrays without any allocation.
- JS: `lib.sightings()`, `lib.zones()`, plain objects for the value types.
- Tracing: optional events (added, refreshed, evicted, expired, sent,
  cancelled) through the existing `Sink`.

## Tests and proof

Scope of the proof in this stage: unit tests and golden values inside
Sagax. A frame-by-frame comparison against the real game runs later, in the
game's own repo, when it actually migrates.

- Unit tests per block, including all tie-breaks and wrap-around of time.
- A reference test with the officer numbers (17 ticks/s): lost 34, stall 85
  with gain 0x80, ignore 170, radio delay 102, cooldown 170, report every
  255, forget 2040, max 8 entries, search 3 points at 5/9/12 cells,
  look 170/51, walk limits 1020/340, search total 1020. Golden values for
  `search_point`, `sin256`, `look_heading` and `nearest`: the builder
  computes them from the reference C code (private notes) and puts only the
  numbers into the public tests — no code, no names.
- An example `examples/officer` (Rust) and a C example.
- C API version 7, USAGE section 7, CHANGELOG 0.7.0.

## Decider inputs

No new built-in input kinds. Ages, `unseen`, distance to `last_pos`,
"known" and "ignored" are plain values the caller passes as decider inputs,
as today. The officer example shows this. The example uses the real stage-2
`StateMachine`.

## Left to the builder

File split, exact names of helpers, trace event layout, how JS exposes the
value types, benchmark (tables are tiny; a micro benchmark is enough).

## Calls I made (not discussed)

- Uncertainty is not stored; a game derives "search radius" from age.
- C tables are allocated once at creation; value types are caller-owned
  structs for zero allocation.
- Danger zones are generic; the reference officers do not use them (only
  the enemy agents do), but they are part of the same need.
- Proof in this stage = unit tests + golden values, no trace comparison
  against the game (that comes with the migration).
- Zone blocking: any zone, blocking only, cell→position by the caller.
- No new decider input kinds.
- The reference mapping (which game field maps to which block) stays out
  of this public repo.
