# Indirect coordination (0.14.0, C API 14) — design

Status: written by the controller; the owner said "just implement it" and
skipped the questions. Every point below that the owner did not say is a
call made here and listed under "Calls made without the owner". Target
version 0.14.0, C API 14. JavaScript is not part of this round.

## Intent

Groups in Sagax coordinate by orders: a leader decides, the order travels by
shout, radio or a custom channel (hand signs in `ambush_rules`). Real groups
also coordinate without any order: a guard **sees** two comrades charge and
charges too; a civilian **sees** others run and runs as well; panic spreads
from one frightened figure to the next. The owner calls this *indirect
coordination* (memory, 2026-11-12): "a figure sees allies attack and joins
the attack, or sees them flee and flees too — without an order. Condition:
it must really see the allies (no cheating)."

### The one hard rule: no cheating

A character joins in only on what it perceived itself. The new block reads
**only percepts** of that character's own `look` (sight or thermal, stage
recognised, cue bits as perceived). It never reads what an ally "really"
does, never reads another character's state and never sees through walls or
darkness — every gate of the Perception block (cone, light, walls,
attention, disguise) applies unchanged, because allies are ordinary
`Target`s whose `cues` show what they are doing (perception spec §7,
ROADMAP "Perception, next").

## Goals

1. A per-character memory `Contagion` of *deeds* seen on allies: for each
   ally, which deed bits it was last seen showing, since when, and when it
   was last seen at all.
2. Deeds are the game's cue bits (e.g. `ATTACKING`, `FLEEING`, `HIDING`),
   up to `MAX_DEEDS` (8) per memory, each with its own window.
3. Fed from percepts: `learn(percepts, now, is_ally)`. Only percepts with
   `sense` Sight or Thermal and stage recognised carry deed bits (that is
   where perception reports cues). `Cues { new }` marks the onset of a deed;
   `Seen`/`Recognized` refresh "last seen" and the current bits; `Lost`
   and `Forgotten` keep the entry until the window ends (what I saw a
   moment ago still counts). `Heard` and `Scanner` percepts never carry
   deeds.
4. Read as Decider inputs, per deed: `DeedInputs { doing, seen, share,
   nearest, since }` —
   - `doing`: distinct allies seen showing the deed within its window,
   - `seen`: distinct allies seen at all within the window,
   - `share`: `doing * 256 / seen` (0 when `seen` is 0),
   - `nearest`: fine distance from the observer to the nearest ally seen
     doing it (`i32::MAX` when none),
   - `since`: ticks since the most recent onset among those allies
     (`u32::MAX` when none).
   A helper writes these as five consecutive `i32` columns into a Decider
   row, so a game adds a consideration like "join the charge: share of
   seen allies attacking, curve rising" without new Decider code.
5. Cascades fall out by construction: when A flees, the game sets A's
   `FLEEING` cue; B sees it, flees, gets the cue; C sees B.
6. Disguise is honoured: `is_ally(subject, disguised)` is the game's belief
   ("who do I take for an ally"), given the percept's `disguised` flag, so
   an enemy disguised as a guard can be taken for an ally — and its staged
   "fleeing" can trick guards. That is fair: it is what the guard saw.
7. Trace from the start (one debug pattern for all blocks): `learn_traced`
   and `inputs_traced` with records (deed onset, refresh, expire, inputs).
8. Everything in Rust and C (`sx_contagion_*`), square and hex (only
   distances on fine points; no grid code needed), deterministic, integer,
   no heap after `new`.
9. An example `contagion_scene` (Rust + C, "C output equals Rust" in
   `examples/c/run.sh`): a guard post; one guard sees the intruder and
   charges, two comrades who see him join, one behind a wall does not; then
   the intruder's grenade makes one guard flee and panic spreads to those
   who see it, not to the one facing away. Printed as a short text log.

## Non-goals

- No squad or leader involvement. Joining in is an individual decision;
  a game that wants the leader to know can send an existing game report
  (`REPORT_GAME_FIRST` +) through `Crew::report_with`. Squad/Crew code does
  not change.
- No new Decider features; the inputs are plain columns.
- No "what is the ally attacking" — target sharing is the radio's job. A
  joining character uses its own `Knowledge` for a target (or moves towards
  the `nearest` ally's last point, which the memory also exposes).
- No hearing-based contagion (a shout is a direct, radio-like channel and
  exists already).
- No JavaScript binding.

## Anti-goals (failure even if it works)

1. A character reacting to an ally it could not see (wall, dark, behind it,
   not yet recognised). Tested explicitly.
2. A deed lingering forever: every deed expires after its window.
3. Ticking cost or heap per tick; size is fixed at `new(capacity)`.
4. Goldens of existing examples changing.

## Shape (builder may refine names, keep the meaning)

```rust
pub struct DeedRule { pub bits: u32, pub window: u32 }   // bits: cue mask
pub struct ContagionConfig { pub deeds: [DeedRule; MAX_DEEDS], pub count: u8 }
pub struct Contagion { /* fixed slots: ally id, last_seen_at, last point,
                          bits, onset per deed */ }
impl Contagion {
    pub fn new(config: ContagionConfig, capacity: usize) -> Self;
    pub fn learn(&mut self, percepts: &[Percept], now: u32,
                 is_ally: impl FnMut(u32, bool) -> bool);
    pub fn inputs(&self, deed: usize, observer: [i32; 3], now: u32) -> DeedInputs;
    pub fn write_columns(&self, deed: usize, observer: [i32; 3], now: u32,
                         row: &mut [i32]);
    pub fn nearest_point(&self, deed: usize, observer: [i32; 3], now: u32)
        -> Option<[i32; 3]>;
    pub fn forget(&mut self, ally: u32);
    pub fn clear(&mut self);
}
```

When full, a new ally replaces the slot seen longest ago (deterministic;
ties by lower slot). Distances use the existing fine-point metric of the
Perception block (axial on hex as everywhere in perception).

Where it lives: `sagax-perception` (module `contagion`), re-exported by the
facade and prelude. C: `sx_contagion_new/free/learn/inputs/write_columns/
nearest_point/forget/clear/set_trace`, `sx_deed_rule`, `sx_deed_inputs`
with `_Static_assert` layouts; `is_ally` as a callback `int (*)(void *ctx,
uint32_t subject, int disguised)`.

## Calls made without the owner

- Name "contagion" (behaviour contagion) for the block; deeds = cue bits.
- Per-character, not shared per crew (shared would be cheating).
- `Lost`/`Forgotten` keep the entry until the window ends.
- `share` scaled 0..256 like the other Sagax fractions.
- No squad changes; reports are optional game code.
- New example instead of changing `ambush_rules` (keeps its golden). The
  `ambush_rules` docs get one sentence that guard 2, who misses the hand
  sign, could join via contagion.
- 0.14.0 / C API 14.

Builder calls (made while building, from the spec's open points):

- **Grid in the config.** Distances need the grid (Euclid on square, hex
  distance on hex), so `ContagionConfig` carries `grid: Grid` (C: an
  `SX_GRID_*` code; every hex layout means hex, fine points are axial and
  never converted). Levels are ignored, as in the perception distance.
- **What counts as a sighting.** Only a percept with sense `Sight` or
  `Thermal`, stage 2 (recognised) and `placed` counts: `Noticed` at stage
  1, `Seen` at stage 1, `Scanner`, `Heard`, `Lost` and `Forgotten` never
  touch the memory (so `Lost`/`Forgotten` keep the entry). A merely
  noticed figure is not yet known to be an ally, so it is not "seen" either.
- **Current bits, not accumulated bits.** An entry keeps the cue bits of
  its latest recognised sighting. `doing` counts allies whose latest
  sighting showed the deed (any bit of the deed's mask) and lies within the
  deed's window (`now - last_seen <= window`, wrapping time as in
  perception). An ally seen to stop is no longer doing it at once.
- **Onset.** `Awareness` reports `Cues { new }` only once per bit until the
  target is forgotten, so the memory decides onsets itself: a recognised
  sighting showing the deed when the entry did not show it (or the deed had
  run out of its window) is an onset. That covers `Cues { new }` and every
  later re-onset.
- **Sighting time** is the `now` passed to `learn` (the game learns the
  percepts of the same tick).
- **Belief changes.** A recognised sighting for which `is_ally` answers
  `false` removes an existing entry of that subject (for example a disguise
  seen through: the staged fleeing stops counting at once).
- **Expiry** is checked at the start of every `learn` (records `Expired`
  per deed once); reads filter by the window anyway, so a game that does
  not call `learn` still gets correct inputs. An entry beyond every window
  frees its slot. Replacement when full: a free slot first, else the one
  seen longest ago, ties by lower slot (record `Replaced`).
- **Columns.** `write_columns` writes `doing, seen, share, nearest, since`
  as `i32` (a `since` of `u32::MAX` becomes `i32::MAX`); a deed index out
  of range gives the empty inputs (`0, 0, 0, i32::MAX, u32::MAX`). Ties of
  `nearest` go to the lower slot.
- **Trace records** `Onset`, `Refresh` (only when last seen, bits or point
  changed, so the several percepts of one check give one record),
  `Expired`, `Replaced`, `Dropped` (the belief change above) and `Inputs`
  (`inputs_traced`, also behind `write_columns_traced`).
- **C:** `sx_contagion_learn` takes the `sx_percept` array; an unknown
  sense or event or a null `is_ally` is `SX_ERROR_ARGUMENT` and changes
  nothing. `sx_contagion_nearest_point` returns 1 and writes the point, or
  0 when no ally shows the deed.

Review calls (after the first build):

- **Owner.** `Contagion::new(config, capacity, owner)` (C
  `sx_contagion_new(cfg, capacity, owner)`): `learn` skips every percept
  whose `observer` is not the owner, so a mixed buffer of several
  characters' percepts never teaches one what another saw.
- **Only percepts of now.** `learn` skips a percept whose `at` is not
  `now`, so a stale percept passed again neither re-arms a window nor
  starts a deed. A game learns each tick's percepts in the same tick.
- `is_ally` must not call the same handle (C header note).

## Done means

`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo fmt --check`, `CC=gcc sh examples/c/run.sh` green; tests
for every goal and anti-goal 1–3; USAGE gets a section; CHANGELOG 0.14.0;
ROADMAP moves the item to done.
