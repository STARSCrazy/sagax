# Perception Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new crate `sagax-perception` (sight with cone, light, fog and exact lines; noticing and recognising over time; attention budget; hearing on channels; disguise) plus facade helpers in `sagax::perception` and a C layer, in Rust and C, square and hex, traced from the start.

**Architecture:** `sagax-perception` depends on `sagax-core` only: geometry (`world.rs`, `line.rs`, `cone.rs`), the game hook (`map.rs`), values and the per-pair check (`profile.rs`, `awareness.rs`, `look.rs`), hearing (`hear.rs`), outputs and trace (`percept.rs`, `trace.rs`). Everything that joins perception with `Knowledge`, `Alertness` or `Navigator` lives in `crates/sagax/src/perception.rs`. C bindings in `crates/sagax-ffi/src/perception.rs`, codes and trace conversion in `sagax-abi`.

**Tech Stack:** Rust 2024 (workspace `sagax`), C99/C11 header `crates/sagax-ffi/include/sagax.h`, mingw64 gcc for `examples/c/run.sh`.

**Spec:** `docs/superpowers/specs/2026-11-15-perception-design.md` — read it first and keep it open. Its type definitions (sections 2–7), the gain chain (5, step 5), the stage rule (5, step 8), the hearing rules (6), the `learn_percepts` table (7), the trace records (8) and the C sketch (9) are binding and are not repeated here. "Decisions (owner)" and section 12 override anything that reads otherwise.

## Global Constraints

- Integer only (`i32`/`i64`/`i128`/`u64`), no `f32`/`f64` anywhere in `sagax-perception`, no global state, no own randomness (seeds from the caller), no allocation inside the library (outputs go to the caller's `Vec`/array; `push` only).
- Deterministic: same inputs → same outputs on every machine; ties by lower index.
- `sagax-perception` depends only on `sagax-core` (dev-dependency `sagax-debug` allowed).
- Every public item documented (`missing_docs`); every chunk ends green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`, `CC=gcc sh examples/c/run.sh` (gcc from `D:\Dev-Tools` mingw64 on `PATH`).
- Public repo: no names of private games or projects in code, docs, tests, commits. Commit author is the configured git user.
- Square and hex everywhere; fine points inside, whole cells at every edge; hex offset conversion only on whole cells (C).
- Everything opt-in: no existing test, example output or golden trace may change except the `ambush_rules` re-baseline allowed in Task 5.
- Version 0.13.0, C API 13 (`SX_API_VERSION 13`, `API_VERSION: u32 = 13`) — bumped in Task 4 (API) and Task 5 (crate version).
- Branch `feat/perception` in `.worktrees/perception`; one commit per task.
- `MAX_CHANNELS = 4`, `MAX_CUES = 8`, 3 bands, `MAX_PERCEPTS_PER_CHECK = 4`.

## Review Focus

1. **Eye and target in the same cell, or at the same point** → line is clear with clarity 256, cone test with `v = 0` counts as inside every band (no division by zero, no "behind me"). Test in Task 1 (`same_point_line`) and Task 2 (`zero_vector_is_in_view`).
2. **Time wrap** (`now` passing `u32::MAX` between checks, `pending_since` before the wrap) → `wrapping_sub` everywhere; credit stays ≤ `check_interval`, reaction delay still due. Test in Task 2 (`credit_and_reaction_survive_wrap`).
3. **Empty target list, `checks = 0`, `count` mismatch** → `look` with 0 targets or budget 0 writes no percepts, leaves `cursor` valid (`< len` or 0), no panic; mismatched `targets.len() != aware.len()` uses the shorter (Rust `debug_assert`, C returns error). Test in Task 2 (`look_empty_and_zero_budget`) and Task 4 (C error code).
4. **Saturation**: `conspicuity 2048` × several cue gains × `credit` on a `max` near `u16::MAX` → counter clamps at `max`, no overflow panic in debug builds. Test in Task 2 (`gain_saturates_at_max`).
5. **Hex border points and far coordinates**: `cell_of` on exact hex borders and vertices and on points near `±2^30` → stays deterministic and `cell_of(cell_centre(c)) == c`; line walks with endpoints on borders terminate. Test in Task 1 (`hex_border_points`, `far_coordinates`).

---

### Task 1: Geometry — sine/mix move, world, hook, cone, exact sight lines

**Files:**
- Modify: `crates/sagax-core/src/lib.rs` (add `mod turn; pub use turn::{Sine, sine, mix};`), create `crates/sagax-core/src/turn.rs` (the two tables, `Sine`, `sine` moved verbatim from `crates/sagax-decide/src/memory/geometry.rs`; `mix` moved verbatim from `crates/sagax-decide/src/lib.rs:858`).
- Modify: `crates/sagax-decide/src/memory/geometry.rs`, `crates/sagax-decide/src/lib.rs` (delete the moved code, `pub use sagax_core::{Sine, sine}` / `pub use sagax_core::mix` at the old paths — `sagax_decide::memory::{Sine, sine}` and `sagax_decide::mix` must keep compiling unchanged in decide, abi, ffi, wasm).
- Modify: root `Cargo.toml` (add member `crates/sagax-perception`).
- Create: `crates/sagax-perception/Cargo.toml` (workspace fields as the other crates; dep `sagax-core`; dev-dep `sagax-debug`), `src/lib.rs`, `src/world.rs`, `src/map.rs`, `src/cone.rs`, `src/line.rs`, `src/tests/mod.rs` (+ `tests/line.rs`, `tests/world.rs`).

**Interfaces (Produces):**
```rust
// sagax-core
pub enum Sine { Fine, Coarse }  pub fn sine(h: i32, s: Sine) -> i32;  pub fn mix(seed: u64) -> u64;
// sagax-perception
pub enum Grid { Square, Hex }                       // hex: axial fine points, layout only in C
pub struct World { pub grid: Grid, pub cell_size: i32, pub channels: [Channel; MAX_CHANNELS] }
impl World { pub fn square(cell_size: i32) -> Self; pub fn hex(cell_size: i32) -> Self; }  // channels default (Task 3 fills Channel; in Task 1 define `Channel` with its spec fields and `Default` = damping 256, fuzz 0, speed 0, Circle)
pub fn cell_centre(world: &World, cell: Cell) -> Pos;
pub fn cell_of(world: &World, point: Pos) -> Cell;
pub fn hex_heading(dir: u8) -> u8;                  // 0, 43, 85, 128, 171, 213
pub struct Medium { pub opaque: bool, pub density: u16, pub heat_density: u16 }   // #[repr(C)], spec 2
pub trait SenseMap { sight, sight_edge, cross_level, light, sound, sound_edge }   // signatures and defaults exactly as spec 2
pub struct OpenAir;                                  // all defaults
pub struct Lit<H: SenseMap> { pub inner: H, pub ambient: u8 }
```
`GridSight` and `MoveBlocks` need `DenseGrid`/`CellMap` from `sagax-nav`, which this crate must not depend on. **Decision:** `OpenAir` and `Lit<H>` live in `sagax-perception`; `GridSight<'a>` (over `&'a DenseGrid`, `BLOCKED` = opaque) and `MoveBlocks<M: CellMap>` (sight_edge = `step(from, dx, dy) == Some(from.level)`) live in `crates/sagax/src/perception.rs` (created in this task with just these two plus `pub use sagax_perception::*;`). `Lit<H: SenseMap> { pub inner: H, pub ambient: u8 }`: `light = min(inner.light, ambient)`, all other methods forward.
```rust
pub fn in_band(facing: u8, v: (i64, i64), half_angle: u8) -> bool;   // square vector; spec 1 "Headings and the cone test"; v == (0,0) → true
pub fn view_vector(world: &World, from: Pos, to: Pos) -> (i64, i64); // square: (dx, dy); hex: X = 2dq + dr, Y = dr·113512/65536 scaled — use (X·65536, dr·113512) to stay exact
pub struct Line { pub clear: bool, pub clarity: u16, pub blocked_at: Option<Cell> }   // clarity 0..=256
pub fn sight_line(world: &World, map: &mut impl SenseMap, from: Pos, to: Pos) -> Line;
pub(crate) fn sight_line_with(world, map, from, to, thermal: bool) -> Line;  // thermal uses heat_density; used by Task 2
pub(crate) fn sound_cost(world, map, from, to, damping: u16) -> Option<i32>;  // Straight spread for Task 3: same walker, sums sound()·len/S and sound_edge per border, ×damping/256, plus straight distance
```
Rules: walkers are the spec's exact rational ones (square Amanatides–Woo with fractions over `cell_size·|d|`, `i128` cross-multiplication; hex six-direction cube walk with `t_d = (S − (A−c)·d)/((B−A)·d)`). Grazed corner/vertex cells have zero length; a grazed pair blocks only if **both** are opaque (owner decision 2). Eye cell never blocks, target cell only through `sight_edge`. Clarity from summed optical depth `D = Σ density·len/S`: `clarity = 256 >> (D/256)`, then linear between that halving and the next by `D % 256`. Different levels → `cross_level(from, to)`; `None` blocks, `Some(m)` applies `m` once over the whole length. Walk stops at the first block or when clarity reaches 0. Hex range is `hex_distance(fine axial)`; square range Euclidean squared.

- [ ] **Step 1: Move `sine`/`Sine`/`mix`.** Move the code, add re-exports, run `cargo test --workspace` → all existing tests pass unchanged (the move is checked by the old tests). No new test.
- [ ] **Step 2: Write failing tests** in `sagax-perception/src/tests/`:
  - `cell_round_trip_square` and `cell_round_trip_hex`: for all cells in `-20..=20 × -20..=20 × {0, 3}`, `S ∈ {2, 3, 256}`: `cell_of(cell_centre(c)) == c`; square `cell_centre(Cell(1,2,0))` with S 256 = `[384, 640, 0]`; hex `cell_centre(Cell(1,2,0))` = `[256, 512, 0]`.
  - `hex_border_points`: the midpoint between two hex centres and a vertex point map to the cell the rounding rule names (compute by hand for `(0,0)`/`(1,0)` and the vertex of `(0,0),(1,0),(0,1)`); `far_coordinates`: centres near `±2^22` cells with S 256 round-trip.
  - `hex_heading_values`: `[0,43,85,128,171,213]`.
  - `cone_bands`: facing 0, half-angle 43: `(10, 0)` in, `(10, 17)` in (≈59.5°), `(10, 18)` out; half-angle 128 contains `(-10, 0)`; half-angle 0 contains only `(x>0, 0)`; left/right symmetric; `(0,0)` in.
  - `line_symmetric_square` / `line_symmetric_hex`: 200 random small maps (seeded with `mix`, 12×12, 30 % opaque, random densities), random fine endpoints: `sight_line(a,b) == sight_line(b,a)` in `clear` and `clarity`.
  - `line_matches_brute_force_square` / `_hex`: reference walker samples the segment at 4096·S exact rational points (`a + (b−a)·k/N`, cell via `cell_of` on the scaled point) to list cells with positive length; the exact walker's opaque/clear answer equals the reference on the same random maps (exclude segments through exact corners from the reference; corners have their own test).
  - `corner_rules`: square — eye `(0,0)`, target `(2,2)` through the vertex at cell corner `(1,1)`: one of `(0,1)`/`(1,0)` opaque → clear; both opaque → blocked. Hex — a line through a vertex of three hexes with one opaque grazed hex → clear.
  - `end_cells`: eye in an opaque cell → clear; target in opaque cell → clear; `sight_edge` false into the target cell → blocked.
  - `depth_by_length`: one fog cell `density 256` crossed fully → clarity 128; crossed for half its length (endpoint at its middle) → 192 (linear rule); two cells → 64.
  - `cross_level_default_blocks`: from level 0 to level 1 over `OpenAir` → `clear == false`; a hook answering `Some(Medium::default())` → clear 256.
  - `same_point_line`: from == to → clear, 256.
  - `lit_wrapper`: `Lit { inner: OpenAir, ambient: 40 }.light(c) == 40`.
- [ ] **Step 3:** `cargo test -p sagax-perception` → fails to compile.
- [ ] **Step 4: Implement** `world.rs`, `map.rs`, `cone.rs`, `line.rs`, and `crates/sagax/src/perception.rs` (`GridSight`, `MoveBlocks`, re-export; add `sagax-perception` dep to `crates/sagax/Cargo.toml`, `pub mod perception;` in `crates/sagax/src/lib.rs`). Tests in `crates/sagax/tests/perception.rs`: `grid_sight_blocks_blocked` (a `DenseGrid` wall blocks), `move_blocks_follows_step`.
- [ ] **Step 5:** All four green commands pass.
- [ ] **Step 6: Commit** "Perception: geometry and exact sight lines" (mention the `sine`/`mix` move to core).

### Task 2: Awareness — profiles, observe, look, disguise

**Files:**
- Create: `crates/sagax-perception/src/profile.rs`, `awareness.rs`, `look.rs`, `percept.rs`, `trace.rs`, `src/tests/awareness.rs`, `src/tests/look.rs`, `src/tests/disguise.rs`.

**Interfaces:**
- Consumes: Task 1 `World`, `SenseMap`, `in_band`, `view_vector`, `sight_line_with`, `cell_of`.
- Produces (fields exactly as the spec; all `#[repr(C)]`, `Clone, Copy, Debug, PartialEq, Eq, Hash`, `Default` where the spec has it):
```rust
pub const SIGHT: u8 = 1; pub const NIGHT: u8 = 2; pub const THERMAL: u8 = 4; pub const SCANNER: u8 = 8;
pub struct Band { pub half_angle: u8, pub gain: u16, pub instant: bool }
pub struct CueRule { pub bits: u32, pub gain: u16, pub instant: u8, pub min_clarity: u16, pub range: i32 }
pub struct SenseProfile { /* spec 3 */ }
impl SenseProfile { pub fn human(cell: i32) -> Self; pub fn guard(cell: i32) -> Self; pub fn watcher(cell: i32) -> Self;
    pub fn with_night(self) -> Self; pub fn with_thermal(self) -> Self; pub fn with_scanner(self, range: i32) -> Self;
    pub fn with_hearing(self, channel: u8, scale: u16) -> Self; pub fn with_cue(self, rule: CueRule) -> Self /* first free slot */ }
pub struct Stats { pub sight_scale: u16, pub notice_scale: u16, pub hearing_scale: u16, pub reaction_ticks: u16 }  // Stats::NORMAL
pub struct SeeThrough { pub rate: u16, pub near: i32, pub at: u16 }
pub struct DisguiseRule { pub hides: u32, pub see_through: Option<SeeThrough> }   // Default: hides 0, None
pub struct Eye<'a> { pub id: u32, pub point: Pos, pub facing: u8, pub profile: &'a SenseProfile, pub stats: Stats, pub disguise: DisguiseRule }
pub struct Target { /* spec 4 */ }
pub struct Awareness { /* spec 5, private fields */ }
impl Awareness { pub fn stage(&self) -> u8; pub fn counter(&self) -> u16; pub fn in_view(&self) -> bool;
    pub fn last_seen(&self) -> u32; pub fn seen_through(&self) -> bool; pub fn reset(&mut self);
    pub(crate) fn raise_heard(&mut self, now: u32, gain: u32, profile: &SenseProfile, stats: Stats) -> Option<(u8, u8)> /* Task 3 */ }
pub enum Sense { Sight, Thermal, Scanner, Hearing(u8) }
pub enum PerceptKind { Noticed, Recognized, Seen, Cues { new: u32 }, Lost, Forgotten, SeenThrough, Heard }
pub struct Percept { /* spec 7 */ }
pub const MAX_PERCEPTS_PER_CHECK: usize = 4;
pub fn observe(world: &World, map: &mut impl SenseMap, now: u32, eye: &Eye, target: &Target, aware: &mut Awareness, out: &mut Vec<Percept>);
pub fn observe_traced(.., sink: &mut impl Sink<PerceptionTrace>);
pub fn in_sight(world: &World, map: &mut impl SenseMap, eye: &Eye, point: Pos) -> bool;
pub struct Attention { cursor: u32 }   pub struct Budget { pub checks: u16, pub keep: u16, pub instant_cap: u16 }
pub enum Pick { Instant, Kept, RoundRobin }
pub fn look(world, map, now, eye: &Eye, targets: &[Target], aware: &mut [Awareness], attention: &mut Attention, budget: Budget, out: &mut Vec<Percept>);
pub fn look_traced(.., sink);
pub fn count_crowds(targets: &[Target], radius: i32, group_of: impl Fn(&Target) -> u32, out: &mut [u8]);
pub enum PerceptionTrace { /* spec 8, all records; Heard/Unheard used in Task 3 */ }
```
`observe` follows spec 5 "One check" steps 1–9 literally; the gain chain is spec 5 step 5 with its factor table in that order; `THERMAL` is a second pass, the larger `g` counts and decides `sense`; `SCANNER` within `scanner_range` ignores walls, bands, light and caps at `notice_at`. Preset numbers: `human` range 12 cells, near 4 cells, far_gain 64, bands `[43/256/instant, 71/64/no, 0/0]`, base_gain such that a clear lit person walking in focus at near range reaches `notice_at` in 4 checks and `recognize_at` in 12 (pick `notice_at 256, recognize_at 768, max 1024, base_gain 64`, check that with the test below), decay 16, hold 8, hysteresis 32, check_interval 1, crowd `[256,192,144,108,81,61,46,34]`, hearing `[256,0,0,0]`, hear_gain 256. `guard`: range 14 cells, base_gain 80. `watcher`: periphery half-angle 100, recognize_at 1024.

- [ ] **Step 1: Write failing tests** (all on `OpenAir` unless said; S = 256; `Stats::NORMAL`):
  - `human_preset_timing`: walking person (size 256, conspicuity 256) 3 cells ahead in focus, checked every tick: stage 1 at check 4, stage 2 at check 12.
  - `gain_chain_baseline`: a table of 6 rows (vary band, distance, light, clarity, crowd, notice_scale one at a time) with the exact counter after one check — compute the expected values by hand from the chain and write them into the test; this pins the factor order.
  - `outside_cone_and_range_gain_nothing`: target behind (facing 0, target at `(-3 cells, 0)`) and target at range + 1 → counter 0, no percept.
  - `dark_and_night`: light 0 at target cell → gain × `dark_gain`; same with `with_night()` → full gain; `glow 255` in a dark cell → full gain.
  - `thermal_through_smoke`: smoke cell `density 2048, heat_density 0` between: sight clarity ~0, thermal profile with target `heat 255` reaches noticed, percept `sense == Thermal`.
  - `scanner_through_wall_only_noticed`: wall between, `with_scanner(5 cells)`, 100 checks → stage 1, never 2; at 6 cells → nothing.
  - `instant_cue_recognises`: cue rule `bits 1, instant 2, min_clarity 128`; target `cues 1` in focus → `Noticed` and `Recognized` in the first check (reaction 0); in periphery → no instant; behind fog clarity 100 → no instant.
  - `cue_range_limits`: rule `range 10 cells`, target at 11 cells → rule ignored.
  - `reaction_delay`: `reaction_ticks 5`: counter reaches noticed at tick t, `Noticed` percept first at tick t+5; shortened to 2 at t+3 → reported at t+3; lengthened to 9 at t+4 → reported at t+9; `0` → same call; a further rise while pending keeps `pending_since`; counter falls below before due → no percept, pending cleared.
  - `decay_and_hysteresis`: after loss, nothing for `hold` ticks, then `decay_per_tick × credit` per check; stage falls only below `threshold − hysteresis`; `Lost` once when leaving view, `Forgotten` when stage reaches 0.
  - `seen_every_check`: at stage ≥ 1 in view, every check ends with exactly one `Seen` carrying current `point` and `cell_of(point)`; target walks 5 cells → 5 different cells; never `Lost` and `Seen` in one check; never more than 4 percepts per check.
  - `cues_reported_once`: `Cues { new: 2 }` when bit 2 appears after recognition, not again next check.
  - `every_4_equals_every_1`: `check_interval 4`; pair A checked every tick, pair B every 4th tick → equal counter at ticks 4, 8, …, 40.
  - `skipped_time_not_credited`: `check_interval 4`, checked at 0 and 40 → second check credit 4.
  - `credit_and_reaction_survive_wrap`: start at `u32::MAX − 2`.
  - `gain_saturates_at_max`: `max 65535`, conspicuity 2048, all 8 cue rules gain 1024 → counter == max, no panic (debug build).
  - `zero_vector_is_in_view`: target point == eye point → in view.
  - `observe_does_not_touch_attention` (look with an `Attention`, then observe directly → cursor unchanged).
  - `in_sight_matches_observe_geometry`: `in_sight` true exactly when one `observe` with huge base_gain gives gain > 0 (20 random cases).
  - look: `look_order` (one instant target, two kept at stage 2 and 1, the rest round robin: trace `Picked` order equals Instant, Kept(stage 2), Kept(stage 1), RoundRobin from cursor), `instant_ignores_budget` (`checks 0`, instant target still checked, capped by `instant_cap`), `cursor_wraps`, `crowd_slows` (20 targets, budget 4: a target with `crowd 5` reaches noticed later than the same target alone with budget ≥ 20), `look_empty_and_zero_budget`, `skipped_emitted_only_when_enabled` (`NoopSink` vs `Collector`).
  - disguise: `disguise_holds` (disguise 200, see_through None, hides bit 1: percepts all `disguised: true`, `Cues` never contain bit 1, `seen_through()` false after 500 checks, `doubt` 0); `disguise_holds_random_sequences` (500 seeded random sequences of observe/look with random targets, positions, cues and `disguise > 0` → never `disguised: false`, never `SeenThrough`, never hidden bits); `see_through_when_on` (rate 64, near 2 cells: `SeenThrough` exactly once, then hidden cues appear, until `reset()`); `disguise_zero_breaks_at_once`.
  - `trace_equals_plain`: the same call sequence through `observe`/`look` and `_traced` with a `Collector` gives identical percepts and awareness; the `Gain` record of one check carries the factors of `gain_chain_baseline` row 1.
- [ ] **Step 2:** `cargo test -p sagax-perception` → fails.
- [ ] **Step 3: Implement** in the listed files; `observe` = `observe_traced(.., &mut NoopSink)`, same for `look`.
- [ ] **Step 4:** All four green commands pass.
- [ ] **Step 5: Commit** "Perception: awareness, attention and disguise".

### Task 3: Hearing and the facade helpers

**Files:**
- Create: `crates/sagax-perception/src/hear.rs`, `src/tests/hear.rs`.
- Modify: `crates/sagax/src/perception.rs`, `crates/sagax/src/prelude.rs`, `crates/sagax/src/lib.rs` (`debug` gains `pub use sagax_perception::PerceptionTrace;`), `crates/sagax/tests/perception.rs`, `crates/sagax/tests/prelude.rs`.

**Interfaces:**
- Consumes: Task 1 `sound_cost`, `cell_of`, `cell_centre`, `mix`; Task 2 `Eye`, `Awareness::raise_heard`, `Percept`, `PerceptionTrace`.
- Produces:
```rust
pub struct Stimulus { pub id: u32, pub source: u32, pub kind: u32, pub point: Pos, pub loudness: i32, pub channel: u8, pub at: u32 }
pub enum Spread { Circle, Straight }   // #[repr(u8)]
pub struct Channel { pub damping: u16, pub fuzz: u16, pub speed: i32, pub spread: Spread }   // already declared in Task 1
pub fn hear(world: &World, map: &mut impl SenseMap, now: u32, ear: &Eye, stimulus: &Stimulus, cost: Option<i32>, aware: Option<&mut Awareness>, out: &mut Vec<Percept>, seed: u64) -> bool;
pub fn hear_traced(.., seed: u64, sink: &mut impl Sink<PerceptionTrace>) -> bool;
// crates/sagax/src/perception.rs
pub struct SoundCost<'a, H: SenseMap> { pub world: &'a World, pub map: &'a mut H, pub damping: u16 }   // impl CellMap: step = always same level unless sound_edge None; move_cost = damping × (sound(to) + sound_edge) / 256
pub fn flood_sound<H: SenseMap>(nav: &mut Navigator, world: &World, map: &mut H, stimulus: &Stimulus, budget: Budget, out: &mut Vec<(Cell, i32)>);   // reachable from cell_of(point), max_cost = loudness; out from nav.area(); cost in fine units (step cost = cell_size)
pub struct FactKinds { pub noticed_kind: u32, pub seen_kind: u32, pub heard_kind: u32, pub disguised_kind: Option<u32> }
pub struct AlertAmounts { pub noticed: u32, pub recognized: u32, pub seen: u32, pub heard: u32, pub cue_bits: [u32; 8], pub cue_amount: [u32; 8] }  // cue table: amount for a `Cues{new}` with those bits
pub fn percept_fact(p: &Percept, kinds: &FactKinds) -> Option<Fact>;
pub fn alert_amount(p: &Percept, amounts: &AlertAmounts) -> u32;
pub fn learn_percepts(known: &mut Knowledge, alert: &mut Alertness, alert_cfg: &AlertConfig, now: u32, percepts: &[Percept], kinds: &FactKinds, amounts: &AlertAmounts);
```
`hear` follows spec 6 literally (heard test, `Heard` percept always, `Awareness` raised at most to `notice_at` without touching `last`/`last_seen`, stage rule with the call's `reaction_ticks`, `Noticed` after `Heard` when the stage rose, fuzz in whole cells with the exact `mix` formula, hex redraw at most 4 times). `percept_fact` / `learn_percepts`: spec 7 table; disguised percepts use `disguised_kind` or are skipped. The navigator's `Topology` decides square vs hex for the flood; the test sets one up per grid.

- [ ] **Step 1: Write failing tests:**
  - `circle_equals_route_in_range`: on square and hex, for 50 seeded stimulus/listener pairs, `hear(.., Circle)` is heard exactly when the old `route(Metric::Euclid / Hex, InRange)` check of the same cells at `loudness` is (use `sagax_decide::memory::route` in a `crates/sagax/tests/perception.rs` test, since perception has no decide dep).
  - `straight_damped_by_wall`: wall cell `sound 512` between → heard at 4 cells, not at 8; `sound_edge None` → never.
  - `second_channel_slower_and_vaguer`: channel 1 `speed 32, fuzz 1024` vs channel 0 `speed 0, fuzz 0`: `arrive_at` later, fuzz radius > 0, cell differs from the true cell for some seed and stays within the radius.
  - `same_seed_same_fuzz`: same inputs twice → identical percept; different `ear.id` → may differ; radius 0 → true cell; hex fuzz within hex distance r.
  - `hearing_never_past_noticed`: 1000 loud stimuli with an `Awareness` → stage 1 at most, counter ≤ `notice_at`; a counter already above `notice_at` unchanged; `last_seen` unchanged.
  - `hearing_respects_reaction_delay`: `reaction_ticks 3` → `Noticed` percept on the call ≥ 3 ticks later.
  - `stateless_hear`: `aware: None` → one `Heard` percept, returns true.
  - in `crates/sagax/tests/perception.rs`: `flood_around_corner` (L-shaped corridor: `Straight` blocked, flood cost reaches the listener round the corner, `hear(Some(cost))` heard), `seen_keeps_fact_pos_current` (target walks 5 cells under a watching guard → after each tick `known.get(subject, seen_kind).pos` is the target's current cell), `recognition_replaces_noticed`, `lost_removes_nothing`, `facts_are_whole_cells` (fine point `[300, 700, 0]`, S 256 → `Fact.pos == [1, 2, 0]`), `disguised_fact_kind_or_skip`, `alert_from_percepts` (amounts reach `Alertness`).
  - `prelude` test: the new prelude names compile (`SenseProfile`, `Target`, `Awareness`, `observe`, `look`, `hear`, `sight_line`, `learn_percepts`).
- [ ] **Step 2:** run, fail. **Step 3:** implement. **Step 4:** all four green commands pass.
- [ ] **Step 5: Commit** "Perception: hearing and Knowledge helpers".

### Task 4: C layer

**Files:**
- Modify: `crates/sagax-abi/src/codes.rs` (`SENSE_*`, `PERCEPT_*`, `SPREAD_*`, `PERCEPTION_TRACE_*` from 1, `SENSE_PRESET_HUMAN/GUARD/WATCHER`, `API_VERSION = 13`), `crates/sagax-abi/src/convert.rs` + `convert/tests.rs` (`perception_trace`, enum↔code both ways, round-trip tests), `crates/sagax-abi/Cargo.toml` (dep `sagax-perception`).
- Create: `crates/sagax-ffi/src/perception.rs`; modify `crates/sagax-ffi/src/lib.rs`, `crates/sagax-ffi/Cargo.toml` (deps `sagax-perception`, `sagax`), `crates/sagax-ffi/src/tests.rs`, `crates/sagax-ffi/include/sagax.h` (`SX_API_VERSION 13`, all structs and functions of spec 9 with `_Static_assert` sizes and offsets, the position comment above the block).
- Create: `examples/c/perception.c`, `crates/sagax/examples/perception_scene.rs` (its Rust twin); modify `examples/c/run.sh` (build/run `perception`, diff against `perception-rust.txt`, update the header comment and final echo).

**Interfaces:**
- Consumes: everything from Tasks 1–3.
- Produces: the C API exactly as spec 9 (function names, argument order, `cap`/`*total`, `seed` last, errors negative, `sx_sense_map` with nullable fn pointers, adapters `sx_sense_map_from_grid`, `sx_sense_map_from_map`). Hex: `sx_perception_config.grid` carries `SX_GRID_HEX_*`; the handle stores the `HexLayout` and converts whole cells only. `sx_learn_percepts` / `sx_percept_fact` write offset cells on hex. `sx_perception_look` with `count > 0` and a null `targets` or `aware` → negative error; `count == 0` → returns 0, writes nothing. (A length mismatch cannot be seen from C: one `count` covers both arrays.) A callback must not re-enter the handle (document; the handle holds the map while calling).

- [ ] **Step 1: Write failing tests:** abi `perception_codes_round_trip`, `perception_trace_all_records`; ffi `ffi_line_equals_rust`, `ffi_observe_equals_rust` (same scene, same percepts), `ffi_hex_cells_are_offset` (percept cell and fact pos in offset, fine point axial), `ffi_null_and_small_cap` (cap 0 → returns 0, `*total` 4; null pointers → negative), `ffi_trace_callback` (records arrive with the right `kind`).
- [ ] **Step 2:** run, fail. **Step 3:** implement abi, ffi, header.
- [ ] **Step 4: Write** `perception.c` and `perception_scene.rs`: one square scene, 30 ticks — a guard (guard preset, facing sweeping with `sweep_heading`), a smoke cell between it and a walker, a crowd of 6 with budget 3, a shot (`Stimulus` channel 0, `Straight`) at tick 12; print one line per percept (`t observer subject sense event stage cell clarity`) plus the trace kind of every record. C output must equal the Rust twin byte for byte.
- [ ] **Step 5:** all four green commands pass, including `run.sh` with the new diff.
- [ ] **Step 6: Commit** "Perception: C interface (C API 13)".

### Task 5: Examples rewritten, docs, version

**Files:**
- Modify: `crates/sagax/examples/ambush_rules.rs`, `crates/sagax/tests/ambush_rules.rs`, `examples/c/ambush_rules.c`.
- Move (`git mv`): `crates/sagax-decide/examples/stealth_guard.rs` → `crates/sagax/examples/stealth_guard.rs`, `crates/sagax-decide/tests/stealth_guard.rs` → `crates/sagax/tests/stealth_guard.rs`; fix imports to `sagax::…`.
- Modify: `docs/USAGE.md`, `docs/ROADMAP.md`, `CHANGELOG.md`, root `Cargo.toml` (`version = "0.13.0"`), `README.md` if it lists crates.

**Interfaces:**
- Consumes: `sight_line`, `in_sight`, `look`, `hear`, `GridSight`, `learn_percepts`, `FactKinds`, C equivalents.

- [ ] **Step 1: `ambush_rules`** (Rust and C): replace the own Bresenham `sees` by `sight_line` over `GridSight` (C: `sx_perception_line` over `sx_sense_map_from_grid`); hand signs use `in_sight` with the receiver's facing; the intruder becomes a `Target` the guards `look` at, and its percepts feed the crew's `observe` through `percept_fact`. Run `cargo test -p sagax --test ambush_rules` and `run.sh`: Rust and C output still equal. If the outcome differs from the old baseline, update the expected output once and put the reason ("exact sight line instead of Bresenham: <what changed>") in the commit message.
- [ ] **Step 2: `stealth_guard`**: move, then rewrite on hex `look` with `SenseProfile::guard`, `Lit` as the lit area, noise as a `Stimulus` on channel 0 with `Straight`, facts through `learn_percepts`. Its test asserts the guard notices the sneaker later in the dark than in the light, and the noise gives a `SOURCE_HEARD` fact. `cargo test -p sagax --test stealth_guard` passes.
- [ ] **Step 3: Docs.** USAGE: new section "Perception" between memory and group tactics (sight line as drop-in, a guard with `look`, hearing with a second channel, disguise default, `learn_percepts`, the C names, fine points vs whole cells); update the "game decides what is perceived" sentence in section 7; add the row to the section 10 table. ROADMAP: Perception done; "indirect coordination" stays next. CHANGELOG 0.13.0 (new crate, `sine`/`mix` moved to core with re-exports, C API 13, `stealth_guard` moved, `ambush_rules` rewritten and re-baselined if so). Version 0.13.0.
- [ ] **Step 4:** all four green commands pass; `cargo doc --workspace --no-deps` without warnings.
- [ ] **Step 5: Commit** "Version 0.13.0 (C API 13): perception".
