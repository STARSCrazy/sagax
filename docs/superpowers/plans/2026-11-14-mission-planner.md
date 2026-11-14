# Mission planner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `Mission` per character in `sagax_decide::mission`: motive, plan of steps (taken over from game scripts), detours with leash and return, replanning through the library's `Decider`, rules slots; in Rust and C.

**Architecture:** Plain data types (`types.rs`), one runner (`mission.rs`) that the game ticks with the character's own `Knowledge`, scoring in `replan.rs` on top of `Decider`, slots in `rules.rs`. C bindings in `sagax-ffi/src/mission.rs` with number codes in `sagax-abi`. No allocation after `Mission::new`.

**Tech Stack:** Rust 2024 (workspace `sagax`), C99/C11 header `sagax.h`, mingw gcc for the C example.

**Spec:** `docs/superpowers/specs/2026-11-14-mission-planner-design.md` (read it first; its section "Decisions after the builder check" overrides the text above it).

## Global Constraints

- Integer only, deterministic; no global RNG; seed from caller; equal score → lower index.
- All capacity reserved in `Mission::new`; `tick` must not allocate (Vec capacity fixed, `clear` + `push` within capacity only).
- Public repo: no names of private projects or games in code, docs, tests.
- Every public item documented (`missing_docs = "warn"`, CI treats warnings as errors: `cargo clippy --all-targets -- -D warnings`).
- Version 0.11.0, C API 11 (`SX_API_VERSION 11`, `API_VERSION: u32 = 11`).
- Square and hex; with `hex_layout` all game-facing positions (including fact positions read from `Knowledge`) are offset coordinates.
- One commit per task; commit trailer lines:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_011LMvhj8Rz3ewAEMLbCbqre`.
- Existing tests and golden traces must stay unchanged and green.

## Review Focus

1. A plan whose subject the character never knew (empty knowledge at the first tick) → the step is impossible → break/fallback, not a panic or endless GoTo to (0,0). Test in Task 2/4.
2. `Knowledge` evicts the attack target mid-detour → detour ends as "forgotten", return runs. Test in Task 3.
3. Plan with 0 steps given to `set_plan` → error `EmptyPlan`; more steps than `max_steps` → `TooManySteps`. Test in Task 1.
4. Time wrap: `now` wrapping past `u32::MAX` during `Wait` and leash ticks → uses `wrapping_sub`. Test in Task 2/3.
5. `detour(Engage(x))` called every tick while x stays outside the leash → refused each time, no event spam, guard stays home. Test in Task 3.

---

### Task 1: Data types

**Files:**
- Create: `crates/sagax-decide/src/mission/mod.rs`, `crates/sagax-decide/src/mission/types.rs`, `crates/sagax-decide/src/mission/tests.rs`
- Modify: `crates/sagax-decide/src/lib.rs` (add `pub mod mission;`)

**Interfaces (Produces):**
```rust
pub enum Where { Fixed(Pos), Subject(u32), MotivePlace, MotiveSubject, Here }
pub enum StepKind {
    GoTo(Where), Wait(u32), Pause, Follow(u32), WaitFor { subject: u32, radius: u32 },
    Search { centre: Where, radius: u32, ticks: u32 }, Hold(Where), Attack(u32), Wander,
    Signal(u32), Custom { kind: u32, at: Where, param: u32 },
    Call { channel: u32, kind: u32 }, Squad { tactic: u32, target: Where },
}
pub enum Fail { Unknown { subject: u32 }, Defeated { subject: u32 },
    Fact { subject: u32, kind: u32, mask: u32, value: u32 }, Flags { mask: u64, value: u64 }, Custom(u32) }
pub const MAX_FAILS: usize = 4;           // per step and per plan, fixed arrays
pub struct Step { pub kind: StepKind, pub fails: [Option<Fail>; MAX_FAILS], pub skip_if_impossible: bool }
impl Step { go_to(Pos), go_to_where(Where), wait(u32), pause(), follow(u32), wait_for(u32,u32),
    search(Where,u32,u32), hold(Where), attack(u32), wander(), signal(u32), custom(u32,Where,u32),
    call(u32,u32), squad(u32,Where), fails_when(self, Fail) -> Self /* first free entry; panics never: extra ignored? no → returns Self unchanged and debug_assert */ ,
    skip_if_impossible(self) -> Self }
pub struct Plan { steps: Vec<Step>, pub repeat: bool, fails: [Option<Fail>; MAX_FAILS] }
impl Plan { once(&[Step]) -> Self, looping(&[Step]) -> Self, fails_when(self, Fail) -> Self, steps(&self) -> &[Step] }
pub enum MotiveKind { Guard, Defeat, Reach, Custom(u32) }
pub struct Motive { pub kind: MotiveKind, pub place: Option<Pos>, pub subject: Option<u32>, pub radius: u32 }
impl Motive { guard_place(Pos), guard_subject(u32), defeat(u32), defeat_enemies(), reach(Pos), custom(u32) }
pub enum OnLost { Return, Search { ticks: u32, radius: u32 }, Keep }
pub struct Leash { pub distance: Option<u32>, pub ticks: Option<u32>, pub on_lost: OnLost }
pub struct Leeway { pub leash: Leash, pub replan: bool, pub widen: bool }
impl Leeway { pub const STRICT, FLEXIBLE, FREE }  // values per spec; FLEXIBLE on_lost Search{ticks:60, radius:6}
impl Default for Leeway → FLEXIBLE
pub struct MissionConfig { pub id: u32, pub max_steps: usize /*16*/, pub max_playbook: usize /*8*/,
    pub subject_kind: u32 /*0*/, pub enemy_bits: u32 /*1*/, pub defeated_bits: u32 /*2*/,
    pub arrive_radius: u32 /*0*/, pub lost_after: u32 /*30*/, pub pause_ticks: u32 /*50*/,
    pub safe_distance: u32 /*12*/, pub reached_signal: u32 /*0*/, pub metric: Metric /*Chebyshev*/,
    pub search: PointPattern /*library default pattern if one exists, else 3 points at 8 cells*/,
    pub hex_layout: Option<HexLayout> }
pub enum Detour { Engage(u32), Flee { from: Where, to: Option<Pos> }, Custom { kind: u32, at: Where, param: u32 } }
pub enum Action { None, GoTo(Pos), Wait, Follow { subject: u32, pos: Pos }, Hold(Pos),
    Attack { subject: u32, pos: Pos }, Search(Pos), Wander, Flee(Pos), Custom { kind: u32, pos: Option<Pos>, param: u32 } }
pub enum BreakReason { Fail(Fail), Impossible, PlanEnded }
pub enum DetourEnd { Defeated, Forgotten, Lost, Leash, Safe, Arrived, Ended }
pub enum Event { StepDone(u16), Looped, PlanDone, Broken(BreakReason), Replanned { option: u32 }, FellBack,
    DetourStarted, DetourEnded(DetourEnd), Returned, Signal(u32),
    Call { channel: u32, kind: u32, pos: Pos }, WantsSquad { tactic: u32, target: Option<Pos>, subject: Option<u32> } }
pub const MAX_EVENTS: usize = 8;
pub enum MissionError { ZeroCapacity, EmptyPlan, TooManySteps, PlaybookFull, HexNeedsHexMetric }
```
`Pos`, `Metric`, `PointPattern`, `HexLayout`: reuse from `crate::memory` / `sagax_core`. All types `Clone, Copy (where possible), Debug, PartialEq, Eq, Hash`. Errors implement `Display` + `Error`.

- [ ] Step 1: tests in `mission/tests.rs`: `presets_match_spec` (STRICT = distance Some(3), ticks None, Return, replan false, widen false; FREE = None, None, Keep, true, true; FLEXIBLE distance Some(12), replan true, widen false; `Leeway::default() == FLEXIBLE`), `plan_builders` (`Plan::looping(&[..3]).repeat == true`, `steps().len()==3`), `fails_when_fills_slots_in_order`.
- [ ] Step 2: `cargo test -p sagax-decide mission` → fails to compile.
- [ ] Step 3: implement types.
- [ ] Step 4: tests pass; `cargo clippy -p sagax-decide --all-targets -- -D warnings` clean.
- [ ] Step 5: commit "Mission: data types".

### Task 2: Running a plan (no detours, no replanning)

**Files:** Create `crates/sagax-decide/src/mission/mission.rs` (or `runner.rs`); tests in `mission/tests.rs`.

**Interfaces (Produces):**
```rust
pub struct Mission { /* config, motive, leeway, plan, step index, step state, playbook, decider, detour state, events */ }
pub struct Output<'a> { pub action: Action, events: &'a [Event] }   // events(&self) -> &[Event]
impl Mission {
  pub fn new(config: MissionConfig, motive: Motive) -> Result<Self, MissionError>;
  pub fn set_leeway(&mut self, l: Leeway); pub fn leeway(&self) -> Leeway;
  pub fn set_motive(&mut self, m: Motive); pub fn motive(&self) -> Motive;
  pub fn set_plan(&mut self, plan: &Plan) -> Result<(), MissionError>;  // copies into reserved storage, starts at step 0 next tick
  pub fn plan(&self) -> Option<&Plan>; pub fn step_index(&self) -> Option<usize>;
  pub fn finish_step(&mut self);   // completes a Custom step on next tick
  pub fn tick(&mut self, now: u32, pos: Pos, knowledge: &Knowledge, flags: u64) -> Output<'_>;
  pub fn config(&self) -> &MissionConfig;
}
```
Rules: no plan set → the motive's default plan is used (Task 4 adds it; in this task with no plan the action is `Action::None`). Step done/actions per the spec table. `Where` resolution reads `knowledge.get(subject, config.subject_kind)` (with hex: offset→axial for the fact pos; output axial→offset). `Here` fixed when the step begins. Arrival: `distance(metric, pos, target) <= arrive_radius` (Euclid: squared vs radius²). At most one step change per tick: a done step emits `StepDone(i)` and the next step is entered and its action returned in the same tick. Last step done: `repeat` → `Looped`, index 0; else `PlanDone` and action `None` (Task 4 replaces with replanning/hold). `Wander` step: action `Wander` forever, plan ends. `Signal(n)` → `Event::Signal(n)`. `Call`/`Squad` steps without `widen` count as done with no event (Task 4 adds the events). Impossible position: in this task → `Broken(Impossible)` event and action `None` (Task 4 adds fallback). Fails checked every tick: step fails then plan fails; `Fact` reads `get(subject, kind)`, unknown → false; `Flags` uses `flags`. `Fail::Custom` → false here (rules come in Task 4).

- [ ] Step 1: tests: `patrol_loops_like_a_script` — square map, `Plan::looping([go_to(2,0), wait(3), go_to(0,0)])`, character moved by the test one cell per tick toward the action's target; record (tick, step_index, action) for 20 ticks and assert the exact sequence (GoTo until arrived, StepDone(0), Wait for 3 ticks, StepDone(1), GoTo back, StepDone(2)+Looped). `follow_reads_knowledge_only` (subject moves in the test's "truth" but knowledge not updated → action pos stays the old fact pos). `attack_done_when_known_defeated` (set `defeated_bits` in fact.data → StepDone). `wait_survives_time_wrap` (start at u32::MAX-1, wait 3). `empty_knowledge_subject_step_breaks` (`Broken(Impossible)`, no panic). `custom_step_waits_for_finish_step`. `hex_positions_in_offset` (hex layout, fact pos in offset, GoTo action in offset, arrival right).
- [ ] Step 2: run, fail. Step 3: implement. Step 4: pass + clippy. Step 5: commit "Mission: run a plan".

### Task 3: Detours, leash, return

**Files:** Modify `mission/mission.rs` (or add `mission/detour.rs`); tests.

**Interfaces (Produces):**
```rust
impl Mission {
  pub fn detour(&mut self, now: u32, d: Detour, knowledge: &Knowledge) -> bool; // false = refused
  pub fn end_detour(&mut self);
  pub fn in_detour(&self) -> bool; pub fn returning(&self) -> bool;
}
```
Behaviour per spec "Detours" and decision 5: anchor = pos at first detour start (for a running `Hold` step: the hold position); replacing keeps anchor and start time. While in detour, plan fails are not checked. Engage ends: Defeated / Forgotten (no fact) / Leash (distance of own pos from anchor > leash.distance, or `now - start > ticks`) / Lost (`now - fact.seen_at > lost_after`) → `on_lost`: Return ends; Search{ticks,radius} runs a search around the last known pos then ends with `Lost`; Keep goes on to the last known pos. Flee: action `Flee(to or a point away)`; ends Arrived or Safe (subject farther than `safe_distance`). After end: `DetourEnded(why)`, then returning: action `GoTo(anchor)` until arrived (skip if the resumed step is a GoTo/Hold/Search whose target is not the anchor — then resume at once), `Returned`, resume step with its state (Wait remaining time kept: store remaining, not start). Refusal: `Engage(x)` refused when `leash.distance` is Some and x's known position is farther than it from the anchor of the current detour or of the last return until resumed. Unknown x → refused.

- [ ] Step 1: tests: `strict_guard_steps_three_and_returns` (STRICT, Hold(gate); attacker fact moves away one cell per tick; guard follows ≤3 from anchor, `DetourEnded(Leash)`, GoTo anchor, `Returned`, back on Hold). `refused_while_attacker_outside_leash` (repeat `detour` every tick → false, events empty). `flexible_guard_searches_then_resumes_patrol` (attacker fact stops updating → after `lost_after` a search runs, ends Lost, return, `step_index` equals index before detour, Wait remaining kept). `free_guard_keeps_on_until_defeated`. `forgotten_target_ends_detour` (knowledge.remove mid-detour → `DetourEnded(Forgotten)`). `leash_ticks_wrap`.
- [ ] Step 2–5: fail, implement, pass + clippy, commit "Mission: detours and leash".

### Task 4: Breaks, replanning, defaults, widening, rules

**Files:** Create `crates/sagax-decide/src/mission/replan.rs`, `crates/sagax-decide/src/mission/rules.rs`; modify `mission/mission.rs`; tests.

**Interfaces (Produces):**
```rust
pub const IN_FEASIBLE: usize = 0; pub const IN_DIST: usize = 1; pub const IN_PREF: usize = 2;
pub const IN_WIDEN: usize = 3; pub const IN_KNOWN: usize = 4; pub const IN_EXTRA: usize = 5;
pub const DIST_CAP: i32 = 1024; pub const KNOWN_CAP: i32 = 65535;
impl Mission {
  pub fn add_playbook(&mut self, plan: &Plan, preference: u32 /*0..=ONE*/, widening: bool) -> Result<u32 /*option*/, MissionError>;
  pub fn clear_playbook(&mut self);
  pub fn default_option(&self) -> u32;   // max_playbook
  pub fn widen_option(&self) -> u32;     // max_playbook + 1
  pub fn decider(&self) -> &Decider; pub fn decider_mut(&mut self) -> &mut Decider;
  pub fn set_extra_columns(&mut self, n: usize); // reserved in new? → MissionConfig::extra_columns: usize (default 0) instead
  pub fn tick_with(&mut self, now: u32, pos: Pos, knowledge: &Knowledge, flags: u64, rules: &mut dyn MissionRules) -> Output<'_>;
  pub fn detour_with(...) // not needed: detour has no slots
}
pub trait MissionRules {
  fn fail(&mut self, ctx: &FailCtx, id: u32) -> bool { false }
  fn columns(&mut self, ctx: &PlanCtx, out: &mut [i32]) {}
  fn score(&mut self, ctx: &PlanCtx, built_in: u32) -> Option<u32> { None }
}
pub struct BuiltInRules; impl MissionRules for BuiltInRules {}
pub struct FailCtx { pub now: u32, pub pos: Pos, pub step: Option<usize>, pub motive: Motive }
pub struct PlanCtx { pub now: u32, pub pos: Pos, pub option: u32, pub widening: bool, pub feasible: bool, pub first: Option<Pos>, pub motive: Motive }
```
`MissionConfig` gains `extra_columns: usize` (default 0) and `pick: Pick` (default best). `tick` = `tick_with(.., &mut BuiltInRules)`. Decider built in `new`: options 0..max_playbook+2, each with `Step` consideration on IN_FEASIBLE and IN_PREF as `Apply::Multiply` linear 0..ONE (use the existing `Consideration`/`Curve` API; check `lib.rs`). Rows: one per filled candidate; empty slots skipped; seed = `mix(now, config.id)` (existing `mix`). Default plan per motive per spec "Motive"; enemies = facts of `subject_kind` with `data & enemy_bits != 0`, not defeated, nearest by metric, tie lower subject id. Widening plan = `[Call{0,0}] + default`. On break (spec "When a plan breaks" + decisions 1–2): replan → score, pick, `Replanned{option}`, else fallback: replan true → default plan if feasible (`FellBack`), else `Hold(Here)` (`FellBack`); replan false → `Hold(Here)` after any return (`FellBack`). PlanDone without repeat → same as break with reason `PlanEnded`. With widen: `Call` → `Event::Call{pos}`, `Squad` → `Event::WantsSquad`. Slot call order: per tick, `fail` for step's then plan's `Custom` fails in list order; on replan per candidate in option order `columns` then `score`.

- [ ] Step 1: tests: `stolen_item_replans_to_thief` (FLEXIBLE patrol plan with `Fail::Fact{item, STOLEN_KIND, 1, 1}`; playbook `[attack(thief)]` pref ONE; after fact learned → `Broken`, `Replanned{option:0}`, action Attack). `strict_never_replans` (same with STRICT → `FellBack`, action Hold at current pos, playbook ignored). `uninformed_guard_keeps_patrolling` (second mission, same plan, knowledge without the fact → no Broken). `defeat_without_subject_attacks_nearest_enemy` (two enemy facts, equal distance → lower id). `veto_by_rules` (score slot returns Some(0) for option 0 → default plan chosen). `same_seed_same_pick` (Pick weighted, two runs equal). `no_alloc_after_new` (capacity of internal Vecs unchanged after 200 ticks with replans — expose `#[cfg(test)]` capacity accessor). `widening_emits_call` (FREE, break → widening plan emits `Event::Call`). `plan_end_replans_or_holds`.
- [ ] Step 2–5: fail, implement, pass + clippy, commit "Mission: replanning".

### Task 5: Re-exports and Rust example

**Files:** Modify `crates/sagax/src/lib.rs`, `crates/sagax/src/prelude.rs`; Create `crates/sagax/examples/mission_guard.rs`, test in `crates/sagax/tests/` (or the crate's existing example test pattern — check how `ambush_rules` is tested) asserting the printed trace.

Scene per spec "Proof" 1: square 16×8 map without walls (the test moves characters one cell per tick toward the action pos; Chebyshev). Guards A (STRICT) and B (FLEXIBLE), same looping patrol. Phase 1 ticks 0–29 nothing happens; phase 2 an attacker appears and runs off; phase 3 B learns "item stolen" and replans to attack the thief from its playbook; A is never told and keeps patrolling. Print one line per tick and guard: `t=.. A step=.. action=.. events=..`. Deterministic output; the C example in Task 7 must print the same.

- [ ] Steps: write example, a test that runs it and compares with `tests/mission_guard.txt` (golden, written once from the first correct run after checking it by eye against the proof's story), clippy, commit "Mission: example mission_guard".

### Task 6: C interface

**Files:** Create `crates/sagax-ffi/src/mission.rs`; modify `crates/sagax-ffi/src/lib.rs` (mod, `API_VERSION = 11`), `crates/sagax-ffi/include/sagax.h` (`SX_API_VERSION 11`, new types, functions, `_Static_assert` layout checks like the others), `crates/sagax-abi/src/codes.rs` (codes for step kinds, where kinds, fail kinds, motive kinds, on_lost, detour kinds, action kinds, event kinds, detour end), tests in `crates/sagax-ffi/src/tests.rs`.

**Interfaces (Produces, C):**
```c
typedef struct sx_mission sx_mission;
typedef struct { int32_t kind; sx_pos pos; uint32_t subject; } sx_where;            /* SX_WHERE_* */
typedef struct { int32_t kind; uint32_t subject, data_kind, mask, value; uint64_t flags_mask, flags_value; uint32_t id; } sx_fail; /* SX_FAIL_* */
typedef struct { int32_t kind; sx_where at; uint32_t a, b, c; sx_fail fails[4]; uint32_t fail_count; int32_t skip_if_impossible; } sx_step; /* SX_STEP_*; a,b,c per kind documented */
typedef struct { int32_t kind; int32_t has_place; sx_pos place; int32_t has_subject; uint32_t subject; uint32_t radius; } sx_motive;
typedef struct { int32_t has_distance; uint32_t distance; int32_t has_ticks; uint32_t ticks; int32_t on_lost; uint32_t lost_ticks, lost_radius; int32_t replan, widen; } sx_leeway;
typedef struct { /* every MissionConfig field; pattern as in the existing sx_point_pattern if there is one */ } sx_mission_config;
typedef struct { int32_t kind; sx_pos pos; uint32_t subject, param; } sx_action;     /* SX_ACTION_* */
typedef struct { int32_t kind; uint32_t a, b; sx_pos pos; int32_t has_pos; } sx_event; /* SX_EVENT_* */
typedef struct { sx_action action; sx_event events[8]; uint32_t event_count; } sx_mission_out;
typedef struct { void *user; int32_t (*fail)(void*, const sx_fail_ctx*, uint32_t);
  void (*columns)(void*, const sx_plan_ctx*, int32_t*, uint32_t); int32_t (*score)(void*, const sx_plan_ctx*, uint32_t, uint32_t*); } sx_mission_rules;
void sx_mission_config_default(sx_mission_config*); void sx_leeway_preset(int32_t which, sx_leeway*);
sx_mission *sx_mission_new(const sx_mission_config*, const sx_motive*); void sx_mission_free(sx_mission*);
int32_t sx_mission_set_leeway(sx_mission*, const sx_leeway*); int32_t sx_mission_set_motive(sx_mission*, const sx_motive*);
int32_t sx_mission_set_plan(sx_mission*, const sx_step*, uint32_t count, int32_t repeat, const sx_fail*, uint32_t fail_count);
int32_t sx_mission_add_playbook(sx_mission*, const sx_step*, uint32_t, int32_t repeat, const sx_fail*, uint32_t, uint32_t preference, int32_t widening); /* returns option or error */
void sx_mission_clear_playbook(sx_mission*);
int32_t sx_mission_tick(sx_mission*, uint32_t now, sx_pos pos, const sx_knowledge*, uint64_t flags, const sx_mission_rules* /*NULL ok*/, sx_mission_out*);
int32_t sx_mission_detour(sx_mission*, uint32_t now, int32_t kind, const sx_where* at, uint32_t subject, int32_t has_to, sx_pos to, uint32_t param, const sx_knowledge*); /* 1 taken, 0 refused, <0 error */
void sx_mission_end_detour(sx_mission*); void sx_mission_finish_step(sx_mission*);
int32_t sx_mission_step_index(const sx_mission*); /* -1 none */
sx_decider *sx_mission_decider(sx_mission*);   /* borrowed, valid while the mission lives */
```
Follow `machine.rs`/`memory.rs` patterns (`create`, `destroy`, `ERROR_ARGUMENT`, null checks, `catch_unwind` if used there). Check how `sx_decider` and `sx_knowledge` handles are defined and whether a borrowed decider pointer is possible; if the decider handle type owns its value, add `sx_mission_decider_*` thin wrappers for `add_consideration` instead and note it in the header. Codes live in `sagax-abi` (one table, used by ffi).

- [ ] Steps: ffi tests mirroring Task 2–4 key tests through the C functions (`patrol_loops`, `strict_returns`, `replan_on_fact`, `null_rules_is_built_in`, `bad_arguments`); header + static asserts; `cargo test -p sagax-ffi`; clippy; commit "Mission: C interface".

### Task 7: C example

**Files:** Create `examples/c/mission.c`; modify `examples/c/run.sh` (build + run `mission`, `cargo run -q -p sagax --example mission_guard > examples/c/out/mission-rust.txt`, diff).

- [ ] Steps: write `mission.c` printing the same lines as Task 5; run `PATH=/d/Dev-Tools/mingw64/bin:$PATH CC=gcc sh examples/c/run.sh` → ends with the diff line message; commit "Mission: C example".

### Task 8: Docs and version

**Files:** `Cargo.toml` (0.11.0), `CHANGELOG.md` (## 0.11.0: new section, C API 11, list of new types/functions), `docs/USAGE.md` (new "## 9. Missions" with: concepts motive/plan/leeway/detour, taking over a script (table step ↔ script command), recipe strict vs flexible guard, recipe trial-run scoring through `score`, no-cheating note, C snippet), `docs/ROADMAP.md` (planner done; next: search planner behind same plan format, JS binding on demand, perception), README feature list, section 1 "Which building block?" row.

- [ ] Steps: write docs; `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo doc --no-deps` without warnings, `js/run.sh` still green (if node available); commit "Mission: docs, version 0.11.0 (C API 11)".
