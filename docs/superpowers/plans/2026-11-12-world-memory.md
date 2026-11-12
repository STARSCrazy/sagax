# World Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generic world-memory blocks (sightings, zones, pursuit, ignore, radio, interval, nearest, search pattern) in `sagax-decide`, in Rust, C and WASM/JS.

**Architecture:** New module `crates/sagax-decide/src/memory/` with one file per block, pure integer code, caller-given `u32` time. C exposes the tables as opaque handles and the small value types as `#[repr(C)]` structs with free functions. JS wraps WASM like `lib.machine()`.

**Tech Stack:** Rust (workspace), `sagax-ffi` (C, `include/sagax.h`), `sagax-wasm` + `js/sagax.mjs`, Node 22 tests.

**Spec:** `docs/superpowers/specs/2026-11-12-world-memory-design.md`

## Global Constraints

- Time: `u32`, ages always `now.wrapping_sub(since)`.
- No random numbers, no floats in any block.
- Tie-breaks exactly as in the spec (earlier slot / earlier candidate wins).
- Capacity fixed at creation; no growth after `new`.
- Public repo: no game names, no "police"/"suspect" in API or docs (examples may say "guard"/"officer").
- Version 0.7.0, `API_VERSION = 7`. CI: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace`, `js/run.sh`.
- One commit per finished task.

## Review Focus

- Time wrap-around (`since` near `u32::MAX`, `now` small): ages must stay correct → test in Task 1 and 3.
- Negative coordinates in `nearest` and `search_point`: truncating division, not shift → test in Task 3 and 4.
- Capacity 0 passed to a table: `new` returns an error (Rust) / null (C) → test in Task 1, 2, 6.
- `Sightings.seen` with an id already present while full: must not evict → test in Task 1.
- `Radio.take` / `due` without pending message: `due` false, `take` returns `None` → test in Task 3.

---

### Task 1: Sightings, Ignore, Interval

**Files:**
- Create: `crates/sagax-decide/src/memory/mod.rs`, `memory/sightings.rs`, `memory/ignore.rs`, `memory/tests.rs`
- Modify: `crates/sagax-decide/src/lib.rs` (add `pub mod memory;`)

**Interfaces:**
- Produces: `pub type Pos = [i32; 3];` `pub struct ZeroCapacity;`
  `Sightings::new(capacity: usize) -> Result<Self, ZeroCapacity>`;
  `seen(&mut self, id: u32, pos: Pos, now: u32) -> Option<u32>`;
  `touch(&mut self, id: u32, now: u32) -> bool`; `set_pos(&mut self, id: u32, pos: Pos) -> bool`;
  `set_data(&mut self, id: u32, data: u32) -> bool`; `remove(&mut self, id: u32) -> bool`;
  `get(&self, id: u32) -> Option<&Sighting>`; `iter(&self) -> impl Iterator<Item=&Sighting>`;
  `len`, `is_empty`, `capacity`, `clear`;
  `expire(&mut self, now: u32, max_age: u32, removed: impl FnMut(u32))`.
  `pub struct Sighting { pub id: u32, pub pos: Pos, pub last_seen: u32, pub data: u32 }` (`#[repr(C)]`, `Copy`).
  `Ignore::new(capacity) -> Result<Self, ZeroCapacity>`; `add(&mut self, id, now)` (present → renew; full → replace oldest, tie earlier slot); `ignores(&self, id, now, duration) -> bool`.
  `#[repr(C)] pub struct Interval { pub last: u32, pub started: u8 }`; `Interval::due(&self, now, every) -> bool` (never started → true); `mark(&mut self, now)`; `reset(&mut self)`.

- [ ] **Step 1: Failing tests** in `memory/tests.rs`:
  `sightings_evicts_oldest_and_keeps_order` (cap 3, seen ids 1@t0,2@t1,3@t2, touch 1@t3, seen 4@t4 → returns `Some(2)`, order `[1,3,4]`);
  `sightings_tie_evicts_earlier_slot` (all same time → evicts first);
  `sightings_present_while_full_does_not_evict`;
  `sightings_expire_in_slot_order` (max_age 2040: ages 2040 and 2039 → only first removed, callback order);
  `sightings_age_wraps` (last_seen `u32::MAX-5`, now 10 → age 16, not expired at 17);
  `zero_capacity_is_error`;
  `ignore_window` (add 7@100, `ignores(7,269,170)` true, `ignores(7,270,170)` false);
  `interval_first_due_then_every` (due at any time before mark; mark@0, due@254 false, due@255 true).
- [ ] **Step 2:** `cargo test -p sagax-decide memory` → FAIL (module missing).
- [ ] **Step 3:** Implement. Storage `Vec` with `with_capacity(capacity)`, never pushed beyond capacity; removal by `Vec::remove` (keeps order).
- [ ] **Step 4:** `cargo test -p sagax-decide` → PASS; clippy clean.
- [ ] **Step 5:** Commit `Memory: sightings, ignore, interval (Rust core)`.

### Task 2: Zones

**Files:** Create `memory/zones.rs`; tests in `memory/tests.rs`.

**Interfaces:**
- Consumes: `Pos`, `ZeroCapacity`.
- Produces: `#[repr(C)] pub struct Zone { pub pos: Pos, pub since: u32, pub data: u32, pub source: Pos, pub has_source: u8 }`;
  `Zones::new(capacity: usize, radius: i32, lifetime: u32) -> Result<Self, ZeroCapacity>`;
  `add(&mut self, pos: Pos, now: u32, data: u32, source: Option<Pos>) -> usize` (index of zone);
  `expire(&mut self, now: u32) -> usize` (count removed); `contains(&self, pos: Pos) -> Option<usize>`;
  `blocks(&self, cell: Pos, start: Pos, goal: Pos) -> bool`; `iter`, `len`, `clear`.
  Distance test 2D, `dx*dx + dy*dy < r*r` in `i64`.

- [ ] **Step 1: Failing tests:** `zone_renews_first_within_radius` (radius 0x200: add (0,0), add (0x1ff,0) → same index, centre moved, data/source replaced, since renewed); `zone_at_exact_radius_is_new` (0x200 away → new zone); `zone_full_drops_oldest`; `zone_expire_at_lifetime` (255: age 255 removed, 254 kept); `zone_blocks_rule` (cell in Z, start out, goal out → true; start in Z → false; goal in Z → false; overlapping zones: blocked if ANY such Z).
- [ ] **Step 2:** run → FAIL. **Step 3:** implement. **Step 4:** run → PASS.
- [ ] **Step 5:** Commit `Memory: zones with merge radius and expiry (Rust core)`.

### Task 3: Pursuit, Radio, nearest

**Files:** Create `memory/pursuit.rs`, `memory/radio.rs`; tests in `memory/tests.rs`.

**Interfaces:**
- Produces: `#[repr(C)] #[derive(Default)] pub struct Pursuit { pub target: u32, pub last_pos: Pos, pub unseen: u32, pub best: i32, pub since: u32 }` with
  `begin(&mut self, target, pos, distance: i32, now)`, `seen(&mut self, pos)`, `missed(&mut self, step: u32)` (saturating add), `reset_progress(&mut self, distance, now)`, `lost(&self, limit) -> bool`, `stalled(&mut self, distance, now, min_gain: i32, limit: u32) -> bool`, `clear(&mut self)`.
  `#[repr(C)] #[derive(Default)] pub struct Radio { pub pos: Pos, pub started: u32, pub last_start: u32, pub pending: u8, pub has_sent: u8 }` with
  `start(&mut self, pos, now, cooldown) -> bool`, `due(&self, now, delay) -> bool`, `take(&mut self) -> Option<Pos>`, `cancel(&mut self)`, `pending(&self) -> bool`.
  `pub fn nearest(candidates: &[(u32, i32, i32)], point: (i32, i32), k: usize, divisor: i32, out: &mut Vec<u32>)` — distance `(dx/divisor)² + (dy/divisor)²` in `i64`, Rust `/` (truncates toward zero), stable insertion: an equal later candidate goes behind. `divisor` ≥ 1 (0 → treat as 1).

- [ ] **Step 1: Failing tests:** `stalled_resets_on_gain` (min_gain 0x80, limit 85; begin dist 1000@0; stalled(900,50) false (900 > 872, age 50); stalled(872,60) false and best=872, since=60; stalled(872,144) false; stalled(872,145) true); `lost_at_limit` (missed(4) ×8 → unseen 32 not lost(34); ×9 → 36 lost); `radio_cooldown_from_start` (start@0 ok; take; start@169 refused; start@170 ok); `radio_refuses_while_pending`; `radio_due_after_delay` (start@10, due(111,102) false, due(112) true); `radio_take_without_pending_is_none`; `radio_first_start_ignores_cooldown` (start@5 with cooldown 170 → ok); `nearest_truncates_negative` (dx = -15 → 0, dx=-17 → -1); `nearest_tie_earlier_wins`; `nearest_fewer_than_k`; `pursuit_time_wraps`.
- [ ] **Step 2:** FAIL. **Step 3:** implement. **Step 4:** PASS.
- [ ] **Step 5:** Commit `Memory: pursuit, radio, nearest (Rust core)`.

### Task 4: Search pattern

**Files:** Create `memory/search.rs`; tests in `memory/tests.rs`.

**Interfaces:**
- Produces: `pub fn sin256(heading: i32) -> i32` (table `{0,25,50,74,98,121,142,162,181,198,213,226,237,245,251,255,256}`, index `((h & 0xff) + 2) / 4 & 63`, quarter-wave mirrored);
  `pub struct SearchPattern { pub heading_step: i32, pub first_distance: i32, pub offsets: Vec<i32>, pub distances: Vec<i32>, pub min: (i32,i32), pub max: (i32,i32) }` with `Default` = `0x62`, `0x100`, `{0,0x20,-0x20}`, `{5*256,9*256,12*256}`, `(0,0)`, `(32767,32767)`;
  `pub fn search_point(agent: u32, step: usize, centre: Pos, pattern: &SearchPattern) -> Option<Pos>` (step > offsets.len() → `None`; z = centre z);
  `pub fn look_heading(base: i32, elapsed: u32, step_ticks: u32, offsets: &[i32]) -> i32` (index `elapsed / step_ticks`, past the end → `base`); `pub const LOOK_OFFSETS: [i32; 8] = [0,32,64,32,0,-32,-64,-32];` with `step_ticks` 20 in the reference.

- [ ] **Step 1: Golden values.** Build a throwaway C program outside the repo from the private reference implementation (pure functions for sine, search point, look heading and nearest pick) and prints: `sin256` for h = -300..300 step 7; `search_point` for agents {0,1,2,5,17,255}, steps 0..3, centres {(0,0),(5000,5000),(32700,100)}; `look_heading` for base 10, elapsed 0..180 step 10; a `nearest` case with negative dx. Put only the numbers into the tests. If no C compiler is available, stop and report.
- [ ] **Step 2:** Failing tests `sin256_matches_reference`, `search_point_matches_reference`, `look_heading_matches_reference`, `search_point_clamps`, `search_point_past_last_step_is_none`.
- [ ] **Step 3:** FAIL. **Step 4:** implement. **Step 5:** PASS.
- [ ] **Step 6:** Commit `Memory: deterministic search pattern (Rust core)`.

### Task 5: Officer example and reference test

**Files:** Create `crates/sagax-decide/examples/officer.rs`; test `memory/tests.rs::officer_recipe_reference_numbers`.

**Interfaces:**
- Consumes: Tasks 1–4, `state::StateMachine` (states Idle, Walk, Look, Chase; timeouts for Look).

- [ ] **Step 1:** Failing test: scripted scene with 4 officers (indices 0..3) and one actor at 17 ticks/s: crime seen by officer 0 at t=1 → radio due at t=103 → `nearest` picks the 3 free officers by distance; officer 0 loses sight → `lost` after 9 missed sight-ticks; search points visited in order; table forgets the actor at last_seen+2040. Assert the tick numbers.
- [ ] **Step 2:** FAIL. **Step 3:** implement the test helper loop + example (prints a short trace). **Step 4:** `cargo test -p sagax-decide`, `cargo run -p sagax-decide --example officer` → PASS / prints.
- [ ] **Step 5:** Commit `Memory: officer recipe example and reference test`.

### Task 6: C interface

**Files:** Create `crates/sagax-ffi/src/memory.rs`; modify `crates/sagax-ffi/src/lib.rs` (`mod memory;`, `API_VERSION = 7`), `crates/sagax-ffi/include/sagax.h`, `crates/sagax-ffi/src/tests.rs`; create `examples/c/memory.c` (+ add to `examples/c/run.sh`).

**Interfaces:**
- Produces C: `sx_sightings_new(uint32_t capacity)` / `_free` / `_seen(s,id,x,y,z,now,uint32_t* evicted) -> int` (1 evicted, 0 not, <0 error) / `_touch` / `_set_pos` / `_set_data` / `_remove` / `_get(s,id,sx_sighting*)` / `_count` / `_at(s,index,sx_sighting*)` / `_expire(s,now,max_age,uint32_t* removed,uint32_t removed_cap) -> int count` / `_clear`;
  `sx_zones_new(capacity,radius,lifetime)` / `_free` / `_add` / `_expire` / `_contains` / `_blocks` / `_count` / `_at`;
  `sx_ignore_new(capacity)` / `_free` / `_add` / `_ignores`;
  value structs `sx_pursuit`, `sx_radio`, `sx_interval` with `sx_pursuit_begin/seen/missed/reset_progress/lost/stalled/clear`, `sx_radio_start/due/take(r, int32_t* xyz) -> int/cancel`, `sx_interval_due/mark/reset` taking a pointer to the caller's struct (null → `ERROR_ARGUMENT`);
  `sx_nearest(const sx_candidate* c, uint32_t n, int32_t px, int32_t py, uint32_t k, int32_t divisor, uint32_t* out) -> int count`;
  `sx_sin256`, `sx_search_point(agent, step, cx,cy,cz, const sx_search_pattern*, int32_t* out_xyz) -> int` (pattern null → default; fixed arrays of up to 8 offsets/distances + count), `sx_look_heading`.

- [ ] **Step 1:** Failing FFI tests mirroring one case per block plus null/zero-capacity → null.
- [ ] **Step 2:** FAIL. **Step 3:** implement, header with comments in the style of `sx_machine_*`. **Step 4:** `cargo test --workspace`, `examples/c/run.sh` → PASS.
- [ ] **Step 5:** Commit `Memory: C interface (C API 7)`.

### Task 7: WebAssembly and JavaScript

**Files:** Modify `crates/sagax-wasm/src/lib.rs`, `js/sagax.mjs`, `js/test.mjs`.

**Interfaces:**
- Produces JS: `lib.sightings(capacity)` → `{seen(id,[x,y,z],now) -> evicted|null, touch, setPos, setData, remove, get(id) -> {id,pos,lastSeen,data}|null, list(), expire(now,maxAge) -> ids[], clear(), free()}`; `lib.zones(capacity, radius, lifetime)` → `{add(pos,now,data,source?), expire, contains, blocks(cell,start,goal), list, free}`; `lib.ignore(capacity)`; plain JS classes `Pursuit`, `Radio`, `Interval` implemented in JS with identical integer rules (`>>> 0` for u32 ages); `lib.nearest(candidates [{id,x,y}], [px,py], k, divisor)`; `lib.sin256`, `lib.searchPoint(agent, step, centre, pattern?)`, `lib.lookHeading`.

- [ ] **Step 1:** Failing JS tests: the same golden values as Task 4, and one case per block incl. time wrap for `Interval`/`Radio`.
- [ ] **Step 2:** `js/run.sh` → FAIL. **Step 3:** implement. **Step 4:** `js/run.sh` → PASS.
- [ ] **Step 5:** Commit `Memory: WebAssembly and JavaScript`.

### Task 8: Docs and release

**Files:** Modify `docs/USAGE.md` (new section 7 "Memory of the world" with the officer recipe and the zone-blocking recipe for `move_cost`), `CHANGELOG.md` (0.7.0), `docs/ROADMAP.md`, workspace version 0.7.0 in `Cargo.toml` files.

- [ ] **Step 1:** Write docs; check that no game names or private terms appear in docs, crates or js.
- [ ] **Step 2:** Full CI locally: fmt, clippy, test, `js/run.sh`, C examples → all green.
- [ ] **Step 3:** Commit `World memory: docs, version 0.7.0`; push to origin; check CI green.
