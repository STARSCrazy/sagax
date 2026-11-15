# Perception (0.13.0, C API 13) — design

Status: draft for the owner. The owner asked to "just start": every point
the owner did not settle is a call made here and listed under "Calls made
without the owner". Target version 0.13.0, C API 13. JavaScript is not part
of this round.

## Intent

Sagax remembers (`Knowledge`, `Alertness`), decides, plans missions and acts
as a group, but it does not perceive. Every game writes its own "can this
guard see that intruder?" (the `ambush_rules` example uses a Bresenham line,
`stealth_guard` fakes sight by distance), and the answer is a plain yes/no.
Real perception has stages and time:

- **What is in view** depends on the facing direction (a focus cone and a
  wider, weaker corner-of-the-eye band), on distance, on light and on what
  the line passes through (walls block, smoke and fog thin it out).
- **What is noticed** takes time and attention: a gunshot is noticed at
  once, a figure at the edge of a crowd perhaps never.
- **What is recognised** takes longer than noticing: "something moved
  there" comes a few ticks before "that is a drawn weapon", and how many
  ticks depends on the observer (a game stat that a game may boost).
- **What is heard** spreads around the listener without a cone, is damped
  by walls, and gives only a rough position.

The Perception block computes this, integer-exact and deterministic, from
state the game holds, and hands the result over as facts for `Knowledge`
and stimuli for `Alertness`.

### The one hard rule: no cheating

A character knows only what it perceived itself. The block never reads a
global "where is the player": the game passes in the targets that *could*
be perceived (its own candidate list), and the block answers what this
observer actually notices of them. Everything downstream (squad, mission,
decider) keeps reading only `Knowledge`. Perception is the honest source of
`SOURCE_OWN` and `SOURCE_HEARD` facts.

## Goals

1. **Sight**: view cone with focus and peripheral bands, range scaled by a
   game stat, light, fog/smoke along the ray, walls; exact sight lines on
   square and hex grids.
2. **Noticing and recognising over time**: a per observer–target value
   (the game stores it) that climbs through *noticed* and *recognised*,
   with an observer reaction delay and decay when lost.
3. **Limited attention**: a fixed number of checks per observer per call,
   deterministic order; crowds dilute attention; some cues are noticed
   instantly.
4. **Hearing**, built generic: stimuli on *channels*. Smell or smoke is a
   second channel of the same mechanism (slower, farther, vaguer), not a
   sense of its own.
5. **Optional special senses**: night vision, thermal vision, a short-range
   scanner through walls, better hearing. All optional, none required by
   any game.
6. **Disguise**: a disguised target is reported as disguised. By default
   a disguise always holds; seeing through it is an optional feature that is
   off unless the game turns it on.
7. **Outputs** that feed `Knowledge` and `Alertness` in one call, and a
   geometric `in_sight` helper for squad and mission rule slots.
8. **Rust and C**, square and hex, traced from the start.
9. **Leave room for indirect coordination** (later): characters perceive
   what allies *do* (attack, flee) as cues of a target, so joining in can
   later be built only on what was actually perceived.

## Non-goals

- No rendering or debug drawing of cones or rays (the trace has the data).
- No path finding and no movement; perception never moves anyone.
- No own map: the game answers per cell through a hook.
- No own randomness: position fuzz and tie-breaks come from the caller's
  seed.
- No 3D ray casting. Sight between height levels is the hook's answer.
- No sound simulation (no echoes, no wave fronts): a sound is heard or not,
  with a damped strength and a fuzzed position.
- No smell as a sense type: a game adds it as a hearing channel.
- No teams, factions or "who is an enemy": targets and cue bits are the
  game's own meaning.
- No prediction of where a target is going (a separate idea, not this
  block).
- No JavaScript binding in this round.

## Anti-goals (a failure even if it works)

- A character learns something it could not have perceived: through a wall
  without a scanner, behind itself outside every band, from a target the
  game did not pass.
- A disguised target is ever reported undisguised while see-through is off
  (default). No sequence of calls may clear the flag, and no cue that the
  disguise hides may appear.
- A game's classic mode changes in any way. Everything here is opt-in; a game
  that never calls the block is bit-identical to before, and the block keeps
  no hidden state that could leak between calls.
- Float, global state, allocation inside the library, or randomness of its
  own.
- A line from A to B and the line from B to A disagree (sight must be
  symmetric when both stand in the same conditions).
- A special sense that a game does not use costs it anything: no field is
  required, no flag is on by default, no call is slower.
- A game must learn the whole block to replace a yes/no sight test: one free
  function (`sight_line`) must do that alone.

## Constraints

- Integer only, deterministic on every machine, no allocation, no global
  state, fixed per-call work (the attention budget bounds it).
- State lives in plain value structs the game owns (as `Alertness`,
  `Progress`): one `Awareness` per observer–target pair, one `Attention`
  per observer.
- Map data comes through a hook (Rust trait, C callback struct), separate
  from movement, with defaults and ready adapters.
- Public repo: generic needs only, no names of games or projects.
- Work on branch `feat/perception` in `.worktrees/perception`, one commit
  per finished chunk. CHANGELOG 0.13.0, C API 13.

## 1. Placement

| Option | Verdict |
|---|---|
| module in `sagax-nav` | no: nav is movement; sight must not share its blocker or its `CellMap` |
| module in `sagax-decide` | no: decide has no grid geometry (cells, hex layout, ray walking); it would pull grid code into the decision crate |
| **new crate `sagax-perception`** on `sagax-core` only | **chosen**: geometry plus awareness values, nothing else; usable without nav or decide |

Dependencies and moves:

- `sagax-perception` depends on `sagax-core` only. It uses `Cell`, `Pos`,
  `HexLayout`, `Sink`/`NoopSink`/`MapSink`.
- `sine` / `Sine` (the 65-entry and 17-entry tables, headings 0..=255) and
  the seed mix `mix` move from `sagax-decide` into `sagax-core`.
  `sagax-decide` re-exports them under the old paths (no API break, no
  duplicate table).
- The adapters to `Knowledge` / `Alertness` and the flood-fill hearing (which
  uses `Navigator::reachable`) live in the facade crate `sagax`, module
  `sagax::perception` (re-exports the crate plus these helpers), exactly
  like `sagax::rules::path_cost` does for walking cost. So the perception
  crate stays pure, and the helpers that join two crates live where both are
  visible.
- `sagax-ffi` and `sagax-abi` gain the C layer and the number codes.
- `stealth_guard` (an example of `sagax-decide`) gets `sagax-perception` as a
  dev-dependency (no cycle: perception does not depend on decide).

### Headings and the cone test

Headings are the library's 0..=255 turn (0 = +x, 64 = +y), the same as
`sine`, `sweep_heading` and `points_around`. The game holds each observer's
facing as one heading (`sweep_heading` already produces looking around).

Is the direction to a target within half-angle `h` of the facing? With the
facing vector `f = (cos h0, sin h0)` from the fine table (amplitude 65536),
the target vector `v`, `dot = f·v`, `cross = |f × v|`:

> `v` is within `h` (0 ≤ h ≤ 128) of `f` exactly when
> `sin(h) * dot >= cos(h) * cross` (all in `i128`).

This is `sin(h − θ) ≥ 0` without a square root or an angle; it is exact for
the table values, symmetric left and right, and works for cones wider than
90°. A band is a half-angle in headings; a 120° cone is half-angle 43.

### Distances and the hex embedding

Positions are `Pos = [x, y, z]` in the game's **fine units**; the world
config gives `cell_size` (units per cell or hex, any positive integer, for
example 256), and `z` is the level, as everywhere in the library.

- **Square**: range is Euclidean, compared squared (exact); the distance
  factor uses the integer square root (`u128::isqrt`, deterministic).
- **Hex**: positions are axial `(q, r)` scaled by `cell_size` (a hex game
  converts with `to_axial_pos` at the edge, as the mission and the crew do).
  Range is the hex distance (rings: "within 10 hexes" is what hex games
  mean). Angles use the embedding `X = 2q + r`, `Y = r·√3`, with `√3` as the
  fixed constant `113512 / 65536` (error below 1e-5, far below one heading).
  This is an approximation of an irrational number but integer and the same
  on every machine. Hex direction `k` (as in `heading` of the spots, 0 = +q,
  1 = (0, 1)) has the heading `round(k·256/6)`: 0, 43, 85, 128, 171, 213;
  `hex_heading(dir)` gives it.

### Exact sight lines (no `f64`)

`sight_line` walks every cell the segment from the eye point to the target
point passes through, **with sub-cell endpoints**, in exact rational
arithmetic. `smooth.rs`'s `line_clear` (cell centres only) and
`hex_line_clear` (`f64` cube rounding with a nudge) are walker tests for
movement and stay as they are; perception does not reuse them, because sight
needs sub-cell endpoints, per-cell segment lengths for fog, and no float.

- **Square**: the Amanatides–Woo traversal with the crossing times kept as
  fractions over `cell_size · |d|`. The next border is the smaller fraction,
  compared by cross-multiplication in `i128`.
- **Hex**: a hex is the Voronoi cell of its centre in cube coordinates
  (`q, r, s = −q − r`, an isometric embedding of the hex plane). The border
  to the neighbour in cube direction `d` (|d|² = 2) is `(p − c)·d = S`
  (with `S = cell_size`). For each of the six directions with `(B − A)·d > 0`
  the exit time is `t_d = (S − (A − c)·d) / ((B − A)·d)`; the smallest wins
  and the walk moves to `c + d`. Six integer dot products per hex, exact.
- **Corners and vertices** (the line passes exactly through a point where
  cells meet): the cells it only grazes have zero length. A grazed pair
  blocks only together: sight slips past one opaque corner, not between two
  (my call; the movement test `line_clear` is stricter on purpose, a walker
  has a body). Both directions of the walk meet the same corners, so
  `A → B` and `B → A` give the same answer.
- **End cells**: the eye's cell never blocks; the target's cell blocks only
  through its `sight_edge` (a target behind a door in its own cell stays
  seen through an open door). Fog and smoke count in every cell by length,
  including both end cells.
- **Levels**: a line between two different levels is the hook's
  `cross_level` answer; the default blocks (as `line_clear` does). A game
  with open floors or balconies answers there, or runs its own 3D test.
- **Cost**: one walk per check, at most range / cell cells; the walk stops at
  the first opaque cell or once the clarity reaches 0.

## 2. The game hook

The game answers per cell (or per edge between two neighbours). Sight is
separate from movement; a ready adapter derives it from movement for games
that share one blocker today.

```rust
/// What a sight line meets inside one cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Medium {
    /// Stops sight (a wall, a closed door).
    pub opaque: bool,
    /// Fog or smoke: optical depth per full cell length. 256 halves the
    /// clarity per cell; it adds up along the ray by length.
    pub density: u16,
    /// The same for thermal sight (smoke is usually 0 here, a wall stays
    /// opaque).
    pub heat_density: u16,
}

pub trait SenseMap {
    /// Inside `cell`. Default: clear air.
    fn sight(&mut self, cell: Cell) -> Medium { Medium::default() }
    /// Crossing from `from` into the neighbour `to` (thin walls, windows,
    /// doors). `false` blocks. Default: open.
    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool { true }
    /// A line between two levels (`from.level != to.level`). Default: blocked.
    fn cross_level(&mut self, from: Pos, to: Pos) -> Option<Medium> { None }
    /// Light at a cell, 0 (dark) ..= 255 (daylight). Default: 255.
    fn light(&mut self, cell: Cell) -> u8 { 255 }
    /// Sound damping inside a cell, in fine units per cell length. Default: 0.
    fn sound(&mut self, cell: Cell) -> u32 { 0 }
    /// Sound damping crossing into a neighbour (a wall that a sound passes
    /// muffled), fine units. `None`: does not pass at all. Default: `Some(0)`.
    fn sound_edge(&mut self, from: Cell, to: Cell) -> Option<u32> { Some(0) }
}
```

Adapters and defaults:

| Adapter | Use |
|---|---|
| `OpenAir` (unit struct) | no map at all: clear, lit, silent; cone, range and light from the target only |
| `MoveBlocks<M: CellMap>` | sight blocked where movement is (`sight_edge` = `step` allowed on the same level): the shared-blocker case |
| `GridSight<'a>` over `DenseGrid` | `BLOCKED` cells are opaque, nothing else (the examples) |
| `Lit<H>` wrapper | the game's hook plus one global light level (time of day): `light = min(cell light, ambient)`, or `ambient` where the hook has none |

Light per place, time of day, rain and haze are the game's: time of day and
rain can be the ambient light of `Lit`, haze a `density` everywhere (or the
profile's range), lamps the per-cell light. The block reads only the light at
the **target's** cell (and the target's own `glow`); an observer standing in
light is not penalised (my call; can be a hook answer later).

C: `sx_sense_map` with one optional function pointer per method (null = the
default) and one `context`, the same shape as `sx_map`. Adapters:
`sx_sense_map_from_grid(const sx_grid *)` and `sx_sense_map_from_map(const
sx_map *)` (valid as long as the given struct lives).

## 3. Sense profiles

A profile is a plain `Copy` value. The game may keep one per role or copy and
change it per observer or per tick (a weapon that sets the range, a drug).

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct SenseProfile {
    pub senses: u8,                 // SIGHT | NIGHT | THERMAL | SCANNER bits
    pub range: i32,                 // sight range, fine units (before stats)
    pub near: i32,                  // full gain up to here
    pub far_gain: u16,              // gain at `range` (per 256), linear from `near`
    pub bands: [Band; 3],           // ascending half-angles; outside all: unseen
    pub base_gain: u16,             // counter gain per tick at best conditions
    pub dark: u8, pub bright: u8,   // light below `dark` gives `dark_gain`, above `bright` full
    pub dark_gain: u16,             // per 256
    pub notice_at: u16, pub recognize_at: u16, pub max: u16,
    pub decay_per_tick: u16, pub hold: u16, pub hysteresis: u16,
    pub max_step: u16,              // most ticks credited per check
    pub scanner_range: i32, pub scanner_gain: u16,
    pub hearing: [u16; MAX_CHANNELS], // range scale per channel, per 256 (0: deaf)
    pub cues: [CueRule; MAX_CUES],
}
pub struct Band { pub half_angle: u8, pub gain: u16, pub instant: bool }
```

`MAX_CHANNELS` = 4, `MAX_CUES` = 8 (my calls; fixed so the C struct is flat).

**Bands** replace a smooth falloff: focus (e.g. half-angle 43 = 120°, gain
256, instant allowed), periphery (e.g. 71 ≈ 200°, gain 64, no instant), and
optionally a third (a "feel behind me" of short range is a scanner-free use
of a 128 band with a tiny gain and the `near` cut). Exact integer test per
band, no angle needed.

**Stats** come from the game per call, so a game stat or a boost acts
without touching the profile:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Stats {
    pub sight_scale: u16,    // range × /256 (perception stat, eye upgrades)
    pub notice_scale: u16,   // gain × /256 (attentiveness)
    pub hearing_scale: u16,  // hearing range × /256
    pub reaction_ticks: u16, // delay before a new stage counts (see 5.)
}
impl Stats { pub const NORMAL: Stats = Stats { sight_scale: 256, notice_scale: 256, hearing_scale: 256, reaction_ticks: 0 }; }
```

A game whose eye upgrades (levels 1–3) only raise its perception stat maps
that stat to `sight_scale` (and, if it likes, `notice_scale`) and needs
nothing else. A reaction time of "30 minus something" frames maps to
`reaction_ticks` directly. A game's own sight-range table per role is the
profile's `range`.

**Special senses** are flags, all off in the plain presets:

| Flag | Effect |
|---|---|
| `SIGHT` | normal sight (light, `density`, bands) |
| `NIGHT` | light counts as at least `bright` |
| `THERMAL` | a second sight pass: ignores light, uses `heat_density`, gain × target `heat`/255; the better of the two passes counts |
| `SCANNER` | within `scanner_range`, ignores walls, bands and light; reaches only *noticed* (a location, never what it is) |

**Presets** (numbers to tune, given `cell_size`): `SenseProfile::human(cell)`
(range 12 cells, focus 43 / periphery 71, notice in about 4 good ticks,
recognise in about 12), `guard(cell)` (range 14, quicker), `watcher(cell)`
(wide periphery, slower recognition), and builders `.with_night()`,
`.with_thermal()`, `.with_scanner(range)`, `.with_hearing(channel, scale)`.
There is no "augmented" preset that bundles them; a game combines what it
uses.

## 4. Targets

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Target {
    pub id: u32,          // the game's subject id (becomes Fact.subject)
    pub pos: Pos,         // eye-to-target point, fine units
    pub size: u16,        // per 256 (a person 256, a crouching one 128, a crate 384)
    pub conspicuity: u16, // per 256: still 128, walking 256, running 512, firing 2048
    pub cues: u32,        // what it shows now (the game's bits)
    pub disguise: u8,     // 0 none, 1..=255 strength
    pub heat: u8,         // for THERMAL (0: cold)
    pub glow: u8,         // own light (a torch, a lit screen): light = max(cell, glow)
    pub crowd: u8,        // how many others it blends with (the game counts)
}
```

- **Cues** are the game's own bits: drawn weapon, attacking, fleeing,
  wounded, a body, an alarm light. The profile's `CueRule`s say what a cue
  does for this observer:

  ```rust
  pub struct CueRule {
      pub bits: u32,        // which cue bits (0: unused rule)
      pub gain: u16,        // extra conspicuity × /256 while shown
      pub instant: u8,      // 0 no, 1 to noticed, 2 to recognised, in an instant band
      pub min_clarity: u16, // instant only with at least this clarity (per 256)
      pub range: i32,       // the rule acts only within this range (0: sight range)
  }
  ```

  "Finding bodies within 10 cells" is a cue rule with `range` 10 cells; "a
  drawn weapon is recognised at once in the focus" is `instant: 2`.
- **Disguise**: see 5. `disguise_hides: u32` in the observer's
  `DisguiseRule` lists the cue bits a disguise masks.
- **Crowd**: `crowd` is the number of other people the target blends with;
  the profile's crowd table (`crowd_gain: [u16; 8]`, index `min(crowd, 7)`)
  divides the gain. Crowds do not block sight (they are not in the map).
  `count_crowds(targets, radius, group_of, out)` is an optional helper for
  games without their own count.
- **Allies and actions**: nothing in `Target` says "enemy". An ally whose
  `cues` show *attacking* or *fleeing* is perceived like anything else;
  this is the hook for indirect coordination later (see 7).

## 5. Awareness model

### The value the game stores

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Awareness {
    counter: u16,       // accumulated evidence, 0 ..= profile.max
    doubt: u16,         // see-through progress (only with see-through on)
    stage: u8,          // 0 unseen, 1 noticed, 2 recognised (what counts now)
    pending: u8,        // the stage the counter reached, waiting for reaction
    flags: u8,          // IN_VIEW, SEEN_THROUGH
    _pad: u8,
    last: u32,          // time of the last check
    pending_since: u32,
    last_seen: u32,
    cues: u32,          // cues reported so far (to report only changes)
}
```

Start it zeroed. The game keeps one per observer–target pair it cares about
(for example the eight entries of a threat list, parallel to its target
array). Getters: `stage()`, `counter()`, `in_view()`, `last_seen()`,
`seen_through()`; `reset()`.

### One check

`observe(world, map, eye, target, aware, now)` with
`eye = Eye { id, pos, facing, profile: &SenseProfile, stats: Stats,
disguise: DisguiseRule }`:

1. `credit = min(now − aware.last, profile.max_step)` (wrapping time).
2. **Geometry**: effective range `range × sight_scale / 256`; outside it, or
   outside every band, the gain is 0. The band gives its `gain` and whether
   `instant` is allowed.
3. **Line**: `sight_line` gives `clear` and `clarity` (0..=256, from the summed
   optical depth: 256 halves per 256 depth, between halvings linear).
   Opaque: gain 0.
4. **Light**: `max(light(target cell), glow)`, `NIGHT` lifts it to `bright`;
   below `dark` → `dark_gain`, above `bright` → 256, linear between.
5. **Gain** (all factors per 256, product in `u64`, divided once at the end,
   truncating):
   `base_gain × band × distance × light × clarity × size × conspicuity ×
   cue gains × crowd × notice_scale × credit`.
   `THERMAL` computes a second gain with `heat_density` and `heat` instead of
   `density` and light; the larger counts. `SCANNER` (within its range)
   gives `scanner_gain × credit`, capped at `notice_at`.
6. **Instant**: if a cue rule with `instant` matches visible, not masked cue
   bits, the band allows instant, and `clarity ≥ min_clarity`, the counter is
   raised to at least `notice_at` (1) or `recognize_at` (2).
7. **Decay** when gain is 0: after `hold` ticks since `last_seen`, lose
   `decay_per_tick × credit`.
8. **Stages**: `pending` is the highest threshold reached (falling with
   `hysteresis`, as `Alertness` does). A rise waits: the new stage counts
   once `now − pending_since ≥ reaction_ticks`. A fall counts at once.
   Sight and thermal reach *recognised*; scanner and hearing only *noticed*.
9. **Events** (returned, game-facing): `Noticed`, `Recognized`,
   `Cues { new }` (recognised cue bits that were not reported before:
   "now I see the weapon"), `Lost` (in view last check, not now),
   `Forgotten` (stage fell to unseen), `SeenThrough`.

Deterministic: same inputs, same counter; no hidden state outside the value.
A game that checks every 4 ticks gets the same evidence per second as one
that checks every tick, as long as `max_step ≥ 4`.

The game's example from its improved mode reads: profile `range = max(10
cells, weapon range · 9/10)` set per tick, focus band 43, no periphery, check
every 4 ticks with `max_step` 4, and a cue rule "drawn weapon or attacking:
instant 2" so guards react only to those cues (the game reads the `Cues`
event). Its classic mode does not call the block at all.

### Disguise

```rust
pub struct DisguiseRule {
    pub hides: u32,              // cue bits a disguise masks
    pub see_through: Option<SeeThrough>, // None (the default): a disguise always holds
}
pub struct SeeThrough { pub rate: u16, pub near: i32, pub at: u16 }
```

- **Default (`see_through: None`)**: a target with `disguise > 0` is
  perceived normally (noticed, recognised, lost), every event carries
  `disguised: true`, and the cue bits in `hides` are never reported. Nothing
  can clear this: `doubt` is never touched, `SEEN_THROUGH` never set (a test
  per call path, plus a property test over random call sequences in the test
  suite only). The Knowledge helper writes a disguised target under the
  game's `disguised_kind`, or skips it when that kind is `None`, so for the
  AI it is not a threat at all. A game that needs "disguised = invisible to
  threat lists" gets exactly that.
- **Optional (`Some(SeeThrough)`)**: while recognised and in view, `doubt`
  grows by `rate × (256 − disguise)/256 × closeness × credit`, where
  closeness is 256 within `near` and falls linearly to 0 at the range; at
  `at` the flag `SEEN_THROUGH` is set (event `SeenThrough`), the disguise no
  longer masks for this observer, and it stays so until `reset()`.
- A game breaks a disguise at once (the agent draws a weapon) by passing
  `disguise: 0`.

### Attention budget

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Attention { cursor: u32, last: u32 }
pub struct Budget { pub checks: u16, pub keep: u16, pub instant_cap: u16 }
```

`look(world, map, eye, targets, aware, attention, budget, now, out)` checks
at most `checks` targets of `targets` (parallel to `aware`), in this order:

1. **Instant**: targets with a cue that has an instant rule for this
   observer (cheap test: bits only), up to `instant_cap`; they do not use
   the budget (an explosion is never missed).
2. **Kept**: targets already noticed or recognised, highest stage first,
   then lower index, up to `keep`.
3. **Round robin**: the rest from `cursor`, wrapping; `cursor` moves past
   the last one checked.

Unchecked targets are not touched (their decay is applied at their next
check, from `last`). The credit per check is the observer's own interval
(`now − attention.last`, capped by `max_step`), not the gap since that
target's last check, so in a crowd of 20 with 4 checks a figure gains only
every fifth round: attention is really limited. `look` pushes its percepts
to the game's `Vec` (the game reserves it; the library allocates nothing of
its own). A game with its own loop uses `observe` per pair.

## 6. Hearing

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Stimulus {
    pub id: u32,       // the game's number for this sound (seed input)
    pub source: u32,   // who or what made it (Fact.subject)
    pub kind: u32,     // the game's kind (gunshot, step, voice, smoke)
    pub pos: Pos,
    pub loudness: i32, // audible range in fine units for a normal listener
    pub channel: u8,   // 0 ..< MAX_CHANNELS
    pub at: u32,       // when it was made
}
pub struct Channel {
    pub damping: u16,  // how much the map's sound damping counts, per 256 (0: walls do not matter)
    pub fuzz: u16,     // position error per unit of distance, per 256
    pub speed: i32,    // fine units per tick (0: arrives at once)
    pub mode: Spread,
}
pub enum Spread { Circle, Straight, Given }
```

The world config holds the `Channel` table (`[Channel; MAX_CHANNELS]`). A
game defines channel 0 as sound and may define channel 1 as smell or smoke:
`speed` low, `loudness` large, `fuzz` large (no precise direction), `damping`
small. That is the whole smell feature (my call: no own sense).

`hear(world, map, ear, stimulus, how, seed) -> Option<Heard>`:

| Mode | Cost | What counts |
|---|---|---|
| `Circle` | O(1) | straight distance only, no walls (the plain "radius 8–16 cells" model) |
| `Straight` | one ray walk | distance + `damping × (sound per cell by length + sound_edge per border)`; `None` from an edge stops it |
| `Given(cost)` | O(1) | a cost the game computed, for example by flood fill around corners |

**Flood around corners** lives in the facade: `sagax::perception::flood_sound(nav,
map, stimulus, budget, out)` runs `Navigator::reachable` once per stimulus
over a `SoundCost<H: SenseMap>` adapter (step cost = cell size, extra =
damping), bounded by the loudness, and writes `(cell, cost)` pairs; each
listener then calls `hear` with `Given(cost of its cell)`. One search per
stimulus, shared by every listener, bounded by the navigator's capacity.

Heard when `cost ≤ loudness × hearing[channel] × hearing_scale / 65536`.
`Heard { stimulus, margin, pos, arrive_at }`:

- `margin` = range − cost (how clearly; becomes `Fact.strength`).
- `pos`: the source position moved by a deterministic offset of radius
  `cost × fuzz / 256`, direction and length from `mix(seed, ear id,
  stimulus id)`, snapped to fine units; on hex in axial units. Same seed,
  same answer.
- `arrive_at = at + cost / speed` (or `at` with speed 0); the game acts on
  it at that time (or pushes it into the listener's `Inbox`).

Hearing never has a cone and reaches only *noticed*. A "heard" becomes a
`SOURCE_HEARD` fact. Radio relays stay `Outbox`/`Inbox` messages as today;
a shout can be a stimulus *and* a message.

## 7. Outputs and use in rules

```rust
pub struct Percept {
    pub observer: u32, pub subject: u32,
    pub sense: Sense,        // Sight, Thermal, Scanner, Hearing(channel)
    pub event: SightEvent,   // Noticed, Recognized, Cues{..}, Lost, ...
    pub stage: u8, pub pos: Pos, pub cues: u32,
    pub disguised: bool, pub clarity: u16, pub at: u32,
}
```

Helpers in `sagax::perception` (and C):

- `percept_fact(&Percept, &FactKinds) -> Option<Fact>`: kind by stage
  (`noticed_kind`, `seen_kind`, `heard_kind`, `disguised_kind:
  Option<u32>`), `source` `SOURCE_OWN` (sight, thermal, scanner) or
  `SOURCE_HEARD`, `strength` = clarity or margin, `data` = cues. `None` for
  a disguised percept without a disguised kind.
- `heard_fact(&Heard, &FactKinds) -> Fact`.
- `alert_amount(&Percept, &AlertAmounts) -> u32`: the stimulus for
  `Alertness::stimulus` per event and per cue (a table, 0 = none).
- `learn_percepts(known, alert, alert_cfg, percepts, kinds, amounts, now)`:
  does both for a slice, the common case in one line.

For squad and mission rule slots (which must stay blind themselves):

- `sight_line(world, map, from, to) -> Line { clear, clarity, blocked_at }`:
  pure, the drop-in replacement for a game's Bresenham `sees`.
- `in_sight(world, map, eye, pos) -> bool`: cone, range and line, no
  awareness, no time; for slot questions like "can the intruder see this
  spot?" (`spot_score`) or "can the receiver see the hand sign?"
  (`channel_delay`).

Rewrites:

| Example | Today | After |
|---|---|---|
| `ambush_rules` (Rust, C) | own Bresenham `sees` over a `DenseGrid` | `sight_line` over `GridSight`; hand signs use `in_sight` with the receiver's facing; the intruder becomes a `Target` the guards `look` at, and its sighting a percept fed to the crew's `observe` |
| `stealth_guard` | `visible && hex distance <= SIGHT`, noise by `route(InRange)` | hex `look` with a guard profile and a lit area as light, the noise a `Stimulus` on channel 0 with `Straight`; facts through `learn_percepts` |

The Rust and C ambush traces must stay equal to each other; if the exact
line changes a scene's outcome against the Bresenham baseline, the expected
output is re-baselined once, in the same chunk, with the reason in the
commit.

**Indirect coordination (later, not built):** an ally's `Cues { new }` event
with an *attacking* bit is a percept like any other. A later rule ("join an
ally I saw attack") reads only such percepts, so it is honest by
construction. What this design guarantees for it: cues are generic bits,
changes are reported as events, and allies are ordinary targets.

## 8. Traces

By the trace rule: `PerceptionTrace` in `sagax-perception`, exported from
`sagax::debug`. Traced operations: `observe_traced`, `look_traced`,
`hear_traced` (sink last). `sight_line`, `in_sight`, the cone test and the
helpers are transparent (their return value is the whole answer; `look`
reports each line it walked).

| Record | When |
|---|---|
| `Picked { target: u32, why: Pick }` | `look` chose to check it (`Instant`, `Kept`, `RoundRobin`) |
| `Skipped { target: u32 }` | the budget ran out before it (only when `enabled()`) |
| `Line { target, clear, clarity: u16, blocked_at: Option<Cell> }` | each sight line walked |
| `Gain { target, band: u8, distance: u16, light: u16, clarity: u16, crowd: u16, gain: u32, sense: Sense }` | the factors of a check (the "why not noticed?") |
| `Instant { target, rule: u8 }` | an instant cue fired |
| `Stage { target, from: u8, to: u8, pending: bool }` | a stage change, or one waiting for reaction |
| `Doubt { target, doubt: u16 }` | see-through progress (only when on) |
| `Heard { stimulus: u32, cost: i32, range: i32, pos: Pos }`, `Unheard { stimulus, cost: Option<i32>, range }` | a hearing check |

C: `sx_perception_trace` (flat, `kind` first, codes
`SX_PERCEPTION_TRACE_*` from 1), `sx_perception_trace_fn`,
`sx_perception_set_trace(sx_perception *, fn, ctx)`. Conversion in
`sagax_abi::convert::perception_trace`.

## 9. C interface sketch

One handle for the world, value structs for everything per character
(as `sx_alertness`, `sx_progress`):

```c
typedef struct sx_perception sx_perception;        /* world: grid, cell size, channels, map, trace */

typedef struct sx_medium { uint8_t opaque, _pad; uint16_t density, heat_density, _pad2; } sx_medium;
typedef int32_t (*sx_sight_fn)(void *ctx, int32_t x, int32_t y, int32_t level, sx_medium *out);
typedef int32_t (*sx_sight_edge_fn)(void *ctx, int32_t x, int32_t y, int32_t level, int32_t to_x, int32_t to_y);
typedef int32_t (*sx_cross_level_fn)(void *ctx, const int32_t *from, const int32_t *to, sx_medium *out);
typedef int32_t (*sx_light_fn)(void *ctx, int32_t x, int32_t y, int32_t level);
typedef int32_t (*sx_sound_fn)(void *ctx, int32_t x, int32_t y, int32_t level);
typedef int32_t (*sx_sound_edge_fn)(void *ctx, int32_t x, int32_t y, int32_t level, int32_t to_x, int32_t to_y);
typedef struct sx_sense_map {
	sx_sight_fn sight;             /* 0: clear air */
	sx_sight_edge_fn sight_edge;   /* 0: open */
	sx_cross_level_fn cross_level; /* 0: blocked */
	sx_light_fn light;             /* 0: 255 */
	sx_sound_fn sound;             /* 0: no damping */
	sx_sound_edge_fn sound_edge;   /* 0: no damping; return -1: does not pass */
	void *context;
} sx_sense_map;

typedef struct sx_perception_config {
	uint32_t grid;                 /* SX_GRID_SQUARE, SX_GRID_HEX_* */
	int32_t cell_size;
	sx_channel channels[SX_MAX_CHANNELS];
} sx_perception_config;

sx_perception *sx_perception_new(const sx_perception_config *cfg, const sx_sense_map *map);
void sx_perception_free(sx_perception *p);
int32_t sx_perception_set_map(sx_perception *p, const sx_sense_map *map);
int32_t sx_perception_set_trace(sx_perception *p, sx_perception_trace_fn fn, void *ctx);

/* value structs, zero to start: sx_sense_profile, sx_band, sx_cue_rule, sx_stats,
 * sx_disguise_rule, sx_target, sx_awareness, sx_attention, sx_budget,
 * sx_stimulus, sx_channel, sx_percept, sx_heard, sx_fact_kinds, sx_alert_amounts */
int32_t sx_sense_profile_preset(uint32_t preset, int32_t cell_size, sx_sense_profile *out);

int32_t sx_perception_line(sx_perception *p, const int32_t *from, const int32_t *to, sx_line *out);
int32_t sx_perception_in_sight(sx_perception *p, const sx_eye *eye, const int32_t *pos);
/* returns the number of percepts written (<= cap), *total how many there were */
int32_t sx_perception_observe(sx_perception *p, uint32_t now, const sx_eye *eye,
	const sx_target *target, sx_awareness *aware, sx_percept *out, uint32_t cap, uint32_t *total);
int32_t sx_perception_look(sx_perception *p, uint32_t now, const sx_eye *eye,
	const sx_target *targets, sx_awareness *aware, uint32_t count,
	sx_attention *attention, const sx_budget *budget,
	sx_percept *out, uint32_t cap, uint32_t *total);
int32_t sx_perception_hear(sx_perception *p, uint32_t now, const sx_eye *ear,
	const sx_stimulus *s, uint32_t mode, int32_t given_cost, sx_heard *out, uint64_t seed);
int32_t sx_perception_flood(sx_perception *p, sx_nav *nav, const sx_stimulus *s,
	sx_point *cells, int32_t *costs, uint32_t cap, uint32_t *total);

int32_t sx_percept_fact(const sx_percept *pc, const sx_fact_kinds *k, sx_fact *out); /* 1 written, 0 skipped */
int32_t sx_heard_fact(const sx_heard *h, const sx_fact_kinds *k, sx_fact *out);
uint32_t sx_percept_alert(const sx_percept *pc, const sx_alert_amounts *a);
int32_t sx_awareness_stage(const sx_awareness *a);  /* plus _counter, _in_view, _last_seen, _seen_through, _reset */
```

- `sx_eye` = `{ uint32_t id; int32_t pos[3]; int32_t facing; const
  sx_sense_profile *profile; sx_stats stats; sx_disguise_rule disguise; }`.
- Conventions as in `sagax.h`: `now` after the handle, payload after, `seed`
  last; lists to caller arrays with `cap` and `*total`; errors negative;
  every struct with `_Static_assert` size and offsets; hex positions in
  offset coordinates at the C edge (the handle converts with its layout),
  as the mission does.
- A callback must not call the same `sx_perception`.
- `SX_API_VERSION` 13. Codes (`SX_SENSE_*`, `SX_PERCEPT_*`, `SX_SPREAD_*`,
  `SX_PERCEPTION_TRACE_*`) in `sagax-abi`.
- JS: none this round; the `sagax-abi` codes keep it possible.

## 10. Non-goals and anti-goals, short

See the lists at the top. In one line: perception computes what one
character perceives from what the game hands it; it draws nothing, moves
nobody, rolls no dice, keeps no world, and is never on a classic path.

## 11. Build order

Every chunk ends green: `cargo test --workspace`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo fmt --check`, and the C examples with
`CC=gcc sh examples/c/run.sh` (a C compiler on `PATH`). One commit per chunk.

1. **Geometry.** Move `sine`, `Sine`, `mix` to `sagax-core` (decide
   re-exports). New crate `sagax-perception`: `SenseMap` and `Medium`,
   `OpenAir`, `GridSight`, `MoveBlocks`, `Lit`; the cone test; exact square
   and hex walkers; `sight_line`. Tests: symmetry (A→B = B→A), a rational
   brute-force reference (dense sampling with exact fractions) on random
   small maps for both grids, corner and vertex cases, depth adds by length,
   `cross_level` default.
2. **Awareness.** `SenseProfile` with presets and builders, `Band`,
   `CueRule`, `Stats`, `Target`, `Eye`, `DisguiseRule`, `Awareness`,
   `observe` / `observe_traced`, `in_sight`; then `Attention`, `Budget`,
   `look` / `look_traced`. Tests per factor, instant cues, reaction delay,
   decay and hysteresis, check-every-4 equals every-1, crowd slows,
   attention order, disguise holds (unit plus randomised call sequences),
   see-through when on, trace equals plain.
3. **Hearing and outputs.** `Stimulus`, `Channel`, `Spread`, `hear` /
   `hear_traced`, fuzz; in `sagax`: `sagax::perception` (re-export),
   `flood_sound`, `percept_fact`, `heard_fact`, `alert_amount`,
   `learn_percepts`; `sagax::debug` exports `PerceptionTrace`; prelude
   entries. Tests: circle equals the old `route(InRange)` result, damping,
   second channel slower and vaguer, flood around a corner, same seed same
   fuzz.
4. **C.** `sagax-abi` codes and `perception_trace`; `sagax-ffi` handle,
   value structs, callbacks, adapters, `set_trace`; header with asserts;
   `SX_API_VERSION` 13; `examples/c/perception.c` (a guard, smoke, a crowd,
   a shot) in `run.sh`, its trace diffed against a Rust twin.
5. **Examples and docs.** Rewrite `ambush_rules` (Rust and C, traces still
   equal) and `stealth_guard` on the block; USAGE: new section "Perception"
   (between memory and group tactics), the "game decides what is
   perceived" sentence of section 7 updated, section 10 table row; ROADMAP
   (Perception done, indirect coordination stays as the next item on it);
   CHANGELOG 0.13.0; version bump.

## 12. Calls made without the owner

1. New crate `sagax-perception` on `sagax-core` only; helpers that join it
   with decide or nav live in `sagax::perception`.
2. `sine` and `mix` move to `sagax-core`, re-exported from decide.
3. Exact integer line walkers of its own (square Amanatides–Woo, hex
   Voronoi walk in cube coordinates); `√3` as a fixed integer fraction for
   hex angles only. The movement tests in `smooth.rs` stay unchanged.
4. Corner rule: sight slips past one opaque corner, not between two.
5. Light is read at the target's cell (plus its glow) only.
6. View falloff is a table of up to three bands, not a smooth curve.
7. Smell is not a sense: hearing has channels, smell is a channel.
8. The reaction delay is applied by the block (a rise in stage counts only
   after `reaction_ticks`); see open question 1.
9. Disguise always holds by default; see-through is optional and off. The
   disguised percept is flagged and its hidden cues masked; the Knowledge
   helper skips it unless the game names a kind for it.
10. Special senses (night, thermal, scanner) are optional flags; no preset
    bundles them; eye upgrades of a game go through `Stats.sight_scale`.
11. Crowd size is the game's count (with an optional helper); crowds dilute
    attention but never block sight.
12. Instant cues bypass the attention budget (bounded by `instant_cap`).
13. Fixed sizes: 3 bands, 8 cue rules, 4 hearing channels.
14. Flood hearing reuses `Navigator::reachable` instead of a second
    Dijkstra.

## Open questions for the owner

1. **Who waits the reaction time?** Proposed: the block delays the stage
   change by `reaction_ticks` (one place, traced as `pending`). Alternative:
   the block reports at once with a `ready_at`, and the game waits.
   *Recommendation: the block waits* — every game needs it and it is easy to
   get wrong by hand; a game that wants the raw stage passes 0.
2. **Corner rule for sight.** Proposed: permissive (one open side lets sight
   through an exact corner). Alternative: strict like `line_clear` (both
   sides must be open). *Recommendation: permissive*, because sight has no
   body and a strict rule makes diagonal gaps between wall cells look solid;
   a game that wants strict can mark the corner cells in its hook.
