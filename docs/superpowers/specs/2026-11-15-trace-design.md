# Trace round — one debugging pattern for every module — design

Status: draft for approval. Ships with the round in progress (0.12.0, C API
12); if it misses that release, it takes the next C API version.

## Intent

Every building block of Sagax should answer "why did you do that?" the same
way, in Rust and in C, on square and hex grids. Today three patterns live
side by side and most modules have none:

| Module | Today | Problem |
|---|---|---|
| search, flow, D* Lite | `*_traced(.., &mut impl Sink<SearchEvent<N>>)` | fine; the model to follow |
| `Navigator` | stored `Tracer` (boxed closure of a flat `TraceEvent`) | second event type for the same thing, adapter `TraceSink`, the only Rust object that stores a trace |
| `Decider` | `decide_traced` / `score_traced` with `DecideEvent` | fine |
| `StateMachine` | `update_traced(Sink<Change>)` | sink of a return type; no C trace |
| `Squad` | built-in `set_score_log` / `score_log() -> &[ScoreEntry]` | a log, not a sink; only scores, nothing about picks, slots, aborts |
| `Mission` | `tick_traced(.., Sink<MissionTraceEvent>)` (being added) | decider events re-flattened by hand |
| area, smoothing, formation, memory, keeper, spots, crew | nothing | |
| C | `sx_event` + `sx_trace_fn` (nav), `sx_decide_event` + `sx_decide_trace_fn`, `sx_score_event` log, `sx_mission_trace_event` | one naming scheme per module |
| WASM | own pull buffers built on `Navigator::set_tracer` | depends on the stored tracer |

This round makes it **one pattern**, applied everywhere, and removes the
duplicates. Interfaces may break (feature phase; nothing in production).

### Hard requirements

1. **Off costs nothing.** Rust: `NoopSink` is monomorphised away. C: one
   null check per call on the handle.
2. **Deterministic.** A trace never changes a result. Every `_traced` call
   returns exactly what the plain call returns (tested per module).
3. **Bounded.** The library never allocates for a trace. Keeping events is
   the receiver's job: `sagax_debug::Collector` in Rust, the callback in C.
4. **Every module covered**: each building block either has a trace or is
   listed as *transparent* (a pure function or plain value whose return value
   is already the whole answer; see "Transparent blocks").

## The rule

> **A trace is a `Sink` parameter. Every operation that decides something
> is written once as `op_traced(.., sink)`; the plain `op(..)` is
> `op_traced(.., &mut NoopSink)`. No Rust type stores a trace. C and WASM
> handles store one callback each and call the `_traced` operation when it
> is set.**

Details of the rule:

- **Sink last.** `sink: &mut S` with `S: Sink<XTrace>` is always the last
  parameter.
- **`_traced` extends the most general variant.** Where an operation has a
  `_with(.., rules)` form, `op_traced` takes the rules too
  (`check_traced(now, extra, spots, seed, rules, sink)`); a caller without
  own rules passes `&mut BuiltInRules`. So an operation has at most three
  forms: `op`, `op_with`, `op_traced`. Never `op_with_traced`.
- **Only decisions.** Setters, getters and constructors are never traced.
  The traced operations per module are listed below.
- **One trace type per traced object or free function**, named
  `<Module>Trace`, defined next to the code it traces.
- **Nesting by wrapping.** A facade that drives sub-modules has one variant
  per sub-module that carries the sub-record unchanged, plus the context
  the sub-record lacks (which tactic, which member, which option):
  `SquadTrace::Decide { tactic, target, event: DecideTrace }`,
  `CrewTrace::Squad(SquadTrace)`. The facade passes a `MapSink` to the
  sub-module. No facade re-declares a sub-module's events.
- **Complete on its own.** A trace records each decision of the call even
  when the return value also reports it (an evicted fact, a refused post),
  so a nested trace read alone tells the whole story.
- **`enabled()` guards work.** Code that must compute something only for
  the trace (an iteration, a distance) checks `sink.enabled()` first.

### Why parameters, not stored traces

| | Sink parameter (chosen) | Stored `Box<dyn Sink>` on objects |
|---|---|---|
| cost off | zero (monomorphised) | a branch and a vtable per event site |
| reading a `Collector` back | `&mut collector`, read after the call | needs `Rc<RefCell<..>>` or downcasting |
| nesting | `MapSink` over a borrow, no allocation | facade must box an adapter per sub-object |
| `Copy`/`Clone` value types (`Areas`, `Knowledge` inside `Squad`) | unaffected | lose `Clone`/`Debug` derives or need special impls |
| matches C | C handle = object + callback; same as `sx_decider` today | — |
| method count | one extra per decision point | one setter per object |

The extra methods are the price; the "only decisions" and "extend the most
general variant" rules keep it to one per decision point. The decider and
the search already work this way, and so does the mission planner's new
`tick_traced`, so the change is mostly *removing* the two stored outliers
(`Navigator::set_tracer`, `Squad::set_score_log`).

### Game events vs trace records

Two different things, two suffixes, everywhere:

| Suffix | Meaning | Examples |
|---|---|---|
| `*Event` | Output the **game acts on**; part of the normal API, returned or written to a `Vec`. | `CrewEvent`, `MissionEvent` (renamed from `mission::Event`) |
| `*Trace` | A **debug record**; only through a `Sink`, never needed to play. | `SearchTrace`, `DecideTrace`, `SquadTrace`, `MissionTrace` |

`Change` (state machine) stays: it is a return value, not a trace record.
The state machine gets its own `StateTrace`.

C follows the same split: `sx_crew_event`, `sx_mission_event` stay game
events; trace records are `sx_<x>_trace`. This is also why the C trace
struct cannot be called `sx_<mod>_event`: crew and mission already use that
name for game events.

## Core pieces (`sagax-core`, `sagax-debug`)

Unchanged: `Sink<E>` (`enabled`, `emit`), `NoopSink`, `CallbackSink`, the
`&mut S` impl, `Collector<E>`, `Overflow`, `Record`.

New in `sagax-core`:

```rust
/// Hands each event to `inner` after `map`; `enabled` is the inner one.
pub struct MapSink<'a, S: ?Sized, F> { inner: &'a mut S, map: F }

impl<'a, S: ?Sized, F> MapSink<'a, S, F> {
    pub fn new(inner: &'a mut S, map: F) -> Self;
}

impl<E, D, S: Sink<D> + ?Sized, F: FnMut(E) -> D> Sink<E> for MapSink<'_, S, F> {
    fn enabled(&self) -> bool { self.inner.enabled() }
    fn emit(&mut self, event: E) { self.inner.emit((self.map)(event)) }
}
```

This is the only adapter. It replaces `nav::trace::TraceSink`, the squad's
private `LogSink` and the mission's hand-made forwarding.

`Collector` gains nothing. (A `drain()` can come later if someone needs it.)

## Paths and exports

- Each trace type lives next to its code: `sagax_nav::search::SearchTrace`,
  `sagax_nav::nav::NavTrace`, `sagax_decide::DecideTrace`,
  `sagax_decide::group::SquadTrace`, `sagax_decide::memory::KnowledgeTrace`,
  and so on (full list below).
- **`sagax::debug` becomes a module** (not a crate alias) and is the one
  place for everything trace related:

  ```rust
  pub mod debug {
      pub use sagax_core::{CallbackSink, MapSink, NoopSink, Sink};
      pub use sagax_debug::{Collector, Overflow, Record};
      pub use sagax_nav::{nav::NavTrace, search::SearchTrace,
                          smooth::SmoothTrace, formation::FormationTrace};
      pub use sagax_decide::{DecideTrace, state::StateTrace};
      pub use sagax_decide::memory::{AreasTrace, KnowledgeTrace, MessageTrace, TimersTrace};
      pub use sagax_decide::group::{CrewTrace, KeeperTrace, SpotTrace, SquadTrace};
      pub use sagax_decide::mission::MissionTrace;
  }
  ```

  `use sagax::debug::*;` is the debug prelude.
- The root re-exports `sagax::{Sink, NoopSink, CallbackSink}` go; they live
  in `sagax::debug` only. The game prelude (`sagax::prelude`) exports no
  trace items. It renames its mission `Event` to `MissionEvent` (a bare
  `Event` in a prelude is too generic).

## Records per module

Payload conventions: positions as the module's own type (`Cell` in nav,
`Pos` in decide); indices as `usize`; ids as `u32`; costs as the module's
cost type. "new" marks an event that does not exist yet.

### Navigation (`sagax-nav`)

**`SearchTrace<N>`** — renamed from `SearchEvent<N>`, content unchanged:
`Started { search, start, estimate }`, `Expanded { search, node, cost,
estimate }`, `Improved { search, node, parent, cost, first }`,
`Paused { search, expanded }`, `Finished { search, status, expanded }`.
Traced by `Search::advance` (already takes the sink), `Flow::build_traced`,
`DStar::compute_traced`. Area searches (`reachable`) are plain searches
without a goal and need nothing more.

**`SmoothTrace<N>`** (new) — `smooth_path_traced(path, out, clear, sink)`:
- `Shortcut { from: N, to: N, clear: bool }` — each straight line the
  smoother tested, and whether it was free.

`line_clear*` stay transparent (one bool); the smoother reports each test.

**`FormationTrace`** (new) — `place_members_traced`, `place_members_hex_traced`:
- `Placed { member: usize, step: usize, cell: Cell, slot: bool }` — the
  target of member `member` at leader step `step`; `slot` false when the
  slot cell was unusable and the member fell back to the leader's trail.
- `Unplaced { member, step }` — no free cell was found.

**`NavTrace`** (new) — the `Navigator`'s record:
- `Search(SearchTrace<Cell>)` — paths, areas, both searches of `flee`
  (they differ in `search`).
- `Smooth(SmoothTrace<Cell>)` — from `output` with a smoothed output.
- `Formation(FormationTrace)` — from `formation`.
- `FleeBest { cell: Cell, distance: Cost, cost: Cost }` — the cell `flee`
  chose (distance from the threats, cost to get there).

Traced: `find_path_traced`, `continue_search_traced`, `reachable_traced`,
`flee_traced`, `output_traced`, `formation_traced`. `set_tracer`, `Tracer`,
`TraceEvent`, `TraceKind`, `TraceSink` and the module `sagax_nav::trace` go.

### Decisions (`sagax-decide`)

**`DecideTrace`** — renamed from `DecideEvent`:
- `Factor { candidate, consideration, input, value }` — unchanged.
- `Scored { candidate, product, score, cooled: bool }` — `cooled` (new
  field) is true when a cooldown forced the score to 0, so a 0 is no longer
  ambiguous.
- `Ranked { rank }` (new) — candidates of mixed rank: only `rank` takes
  part.
- `Chosen { candidate, score }` — unchanged.

Traced: `score_traced`, `decide_traced` (exist).

**`StateTrace`** (new; replaces `Sink<Change>`):
- `Changed(Change)` — a transition or timeout fired.
- `Waiting { transition: usize, elapsed: u32, after: u32 }` — a transition
  whose state and flags match but whose `after` has not passed yet: the
  answer to "why does it not switch?".

Traced: `update_traced`.

### Memory (`sagax-decide::memory`)

**`KnowledgeTrace`** — every variant carries the `Fact` concerned:
- `Added(Fact)`, `Updated(Fact)` — a new key, or a known key overwritten.
- `Stale(Fact)` — `merge` kept the stored fact; the record is the refused
  newer-but-older one.
- `Evicted(Fact)` — dropped by the `Evict` rule to make room.
- `Expired(Fact)` — dropped by an `expire*` call.

Traced: `learn_traced`, `merge_traced`, `learn_message_traced`,
`merge_message_traced`, `expire_traced`, `expire_kind_traced`,
`expire_with_traced`. The game-facing `removed` callback of `expire*` stays
(it is how the game reacts, not debugging).

**`AreasTrace`**:
- `Added { index, area }`, `Renewed { index, area }`.
- `Evicted(Area)` — the oldest, removed because the table was full
  (silent today).
- `Expired(Area)`.

Traced: `add_traced`, `expire_traced`.

**`TimersTrace`**:
- `Started { id, duration }`, `Renewed { id, duration }`.
- `Replaced { id, old: u32, old_left: u32 }` — full: the timer with the
  least time left gave way (silent today).

Traced: `set_traced`.

**`MessageTrace`** — one type for both mailboxes:
- `Posted { msg, due_at }`, `Refused { msg, why: Refused }` (outbox).
- `Queued { msg, arrive_at }`, `Dropped { msg }` (inbox full, or outbox
  `sender_gone`).
- `Delivered { msg }` — handed out by `due*` / `take_due*`.

Traced: `Outbox::post_traced`, `due_traced`, `due_max_traced`,
`sender_gone_traced`; `Inbox::push_traced`, `take_due_traced`,
`take_due_max_traced`.

### Group (`sagax-decide::group`)

**`SpotTrace`** (new) — `pick_spot_traced`:
- `Ranked { spot: usize, fits: bool, distance: i128 }` — each spot in the
  ranking, in rank order (`fits`: has all wanted tags).
- `Picked(SpotPick)` — the result, after the skill's `skip`.

**`KeeperTrace`** (new) — `GoalKeeper::broken_traced`:
- `Checked { waited: u32, limit: u32, broken: bool }`.

**`SquadTrace`** (new; replaces the score log and closes the ROADMAP item
"squad trace sink"):
- `Target { fact: Fact, kept: bool }` — each target the targets slot judged.
- `Decide { tactic: usize, target: Option<u32>, event: DecideTrace }` —
  every factor and score of every candidate (what `ScoreEntry` was), now
  also `Ranked` and `Chosen`.
- `Picked { tactic: usize, target: Option<u32>, score: u32 }`,
  `NoPick` — the outcome of a scoring.
- `Thinking { plan: PlanId, until: u32 }` — the new plan is pending.
- `Assigned { slot: usize, member: u32, cost: Option<i128> }` — each slot
  assignment (`None`: unreachable).
- `Spot { member: u32, event: SpotTrace }` — the spot pick of each member.
- `Abort { tactic: usize, abort: usize, fired: bool }` — each abort check of
  the active plan.
- `Trigger { fired: bool }` — the trigger check.
- `Report { msg: Message }` — each report read, before it is handled.
- `Delay { member: u32, delay: Option<u32> }` — the channel delay of an
  order (`None`: does not reach).
- `Knowledge(KnowledgeTrace)`, `Areas(AreasTrace)` — the squad's own
  memory, changed by `observe`, reports and `check`.

Traced: `plan_traced`, `check_traced`, `observe_traced`, `report_traced`,
`signal_traced`, `reinstate_traced`, `orders_due_traced`. Each takes rules
(the most general variant). The rule slots are not traced themselves: what
a slot answered appears as the decision it fed (`Target.kept`,
`Abort.fired`, `Trigger.fired`, `Delay.delay`, `Assigned.cost`, the spot
score in `Spot`).

**`CrewTrace`** (new):
- `Squad(SquadTrace)` — all squad work of the crew call.
- `Keeper { member: u32, event: KeeperTrace }` — each keeper check.

Traced: `tick_traced`, `observe_traced`, `report_traced`,
`reinstate_traced`. `CrewEvent` stays the game-facing output and is not
repeated in the trace.

### Missions (`sagax-decide::mission`)

**`MissionTrace`** — renamed from `MissionTraceEvent` (being added by the
mission work), reshaped to the wrapping rule:
- `Column { option, column, value }` — unchanged.
- `Decide { option: u32, event: DecideTrace }` — replaces the hand-copied
  `Factor` and the decider half of `Scored`.
- `Rescored { option, built_in: u32, score: u32 }` — the score after the
  game's `score` slot (the other half of today's `Scored`).
- `Picked { option: Option<u32>, score }` — unchanged.
- `Begin { index: usize, at: Option<Pos> }` (new) — a step begins, with the
  position its `Where` resolved to.

Traced: `tick_traced` (exists, takes rules and sink last: already follows
the rule). The game-facing `mission::Event` becomes `MissionEvent`.

### Transparent blocks

These are pure functions or plain values. Their return value is the whole
answer; a trace would only repeat it. They stay untraced in Rust and C:

| Block | Why |
|---|---|
| `route`, `distance`, `arrival` | pure; the recipients / time are the result |
| `points_around`, `sweep_heading`, `sine` | pure geometry |
| `Interval`, `Progress` | two-field values; `due` / `stalled` / `update` return the answer |
| `Alertness` | `update` already returns the level change; plain C struct, no handle |
| `line_clear*`, `heading`, `wish_point`, `rotate*` | pure; reported where used (`SmoothTrace`, `FormationTrace`) |
| `SkillTable`, `kit` builders, `Tactic` builders | data and constructors, no decision at run time |
| `SquadRules` / `MissionRules` slots | the game's code; their answers appear in the facade trace |

If one of them later grows a hidden decision, it gets a trace by the rule.

## C interface

### Pattern

For every traced object type `sx_x` with trace record `sx_t_trace`:

```c
typedef void (*sx_t_trace_fn)(void *ctx, const sx_t_trace *t);
/* fn NULL turns tracing off (the default). Returns 0; SX_ERROR_ARGUMENT
 * for a null handle. */
int32_t sx_x_set_trace(sx_x *x, sx_t_trace_fn fn, void *ctx);
```

Usually `t` is `x` (`sx_squad` → `sx_squad_trace`). The search handles use
the search record: `sx_flow_set_trace(flow, sx_search_trace_fn, ctx)`,
`sx_dstar_set_trace(dstar, sx_search_trace_fn, ctx)`. Outbox and inbox share
`sx_message_trace`.

Rules for every `set_trace`:

- The callback runs inside the call, synchronously, in event order. `t` is
  valid only during the callback; copy what you keep. The callback must not
  call into the same handle (or its parent).
- The handle stores `(fn, ctx)` next to its value; the call uses the
  `_traced` Rust operation only while `fn` is set (one null check when off).
- **Lent and inside handles** (`sx_crew_squad`, `sx_crew_keeper`,
  `sx_squad_decider`, `sx_squad_knowledge`, `sx_mission_decider`, ...)
  accept `set_trace` too. It covers the calls made *through that handle*.
  Work the parent does with its parts goes to the parent's trace, wrapped.
  One place per call, never two.

### Record layout

Follows what the header already does most: a flat struct with a `kind`
first, named fields after it, unused fields 0, options as a value plus an
`uint8_t has_x`, and sub-records **embedded by value** (as `sx_score_event`
embeds `sx_decide_event` today). No unions. Every struct gets its
`_Static_assert` size and offset lines.

- `uint32_t kind` first, codes `SX_<T>_TRACE_<KIND>`, numbered from 1 per
  type (0 never used), e.g. `SX_SEARCH_TRACE_STARTED 1`,
  `SX_SQUAD_TRACE_DECIDE 2`.
- Positions as the header's existing types (`sx_point` in nav, `int32_t
  pos[3]` in decide).
- `i128` costs and distances saturate to `int64_t`.
- A wrapping record has one `uint32_t kind` code per sub-module
  (`SX_SQUAD_TRACE_KNOWLEDGE`) and the sub-record as a field
  (`sx_knowledge_trace knowledge;`), valid for that kind.

Records (fields beyond `kind`; the implementing chunk fixes the order and
the asserts):

| C record | Fields |
|---|---|
| `sx_search_trace` (was `sx_event`) | `uint32_t search; sx_point cell, parent; int32_t cost, estimate, expanded; uint8_t found, first` |
| `sx_nav_trace` | `sx_search_trace search; sx_point from, to, cell; int32_t member, step, distance, cost; uint8_t clear, slot` |
| `sx_decide_trace` (was `sx_decide_event`) | `int32_t candidate, consideration, input; uint32_t value, score, product, rank; uint8_t cooled` |
| `sx_machine_trace` | `sx_change change; uint32_t transition, elapsed, after` |
| `sx_knowledge_trace` | `sx_fact fact` |
| `sx_areas_trace` | `uint32_t index; sx_area area` |
| `sx_timers_trace` | `uint32_t id, duration, old, old_left` |
| `sx_message_trace` | `sx_message msg; uint32_t at, refused` |
| `sx_squad_trace` | `uint32_t tactic, target, member, slot, abort, plan, until, score, delay; int64_t cost, distance; uint8_t has_target, has_cost, has_delay, fired, kept, fits; uint32_t spot; int32_t pos[3]; uint8_t has_spot; sx_fact fact; sx_message msg; sx_decide_trace decide; sx_knowledge_trace knowledge; sx_areas_trace areas` |
| `sx_keeper_trace` | `uint32_t waited, limit; uint8_t broken` |
| `sx_crew_trace` | `uint32_t member; sx_squad_trace squad; sx_keeper_trace keeper` |
| `sx_mission_trace` (was `sx_mission_trace_event`) | `uint32_t option, column, built_in, score, index; int32_t value; int32_t at[3]; uint8_t has_option, has_at; sx_decide_trace decide` |

Spot traces in C arrive only inside `sx_squad_trace` (there is no C
`pick_spot`): `Ranked` maps to `spot`, `fits`, `distance`; `Picked` to `spot` + `has_spot`, or `pos` for an unchecked wish point.

### Objects and free functions

| C handle | `set_trace` | Record |
|---|---|---|
| `sx_nav` | `sx_nav_set_trace` | `sx_nav_trace` (covers find_path, continue, reachable, flee, way, formation, `sx_path_cost`) |
| `sx_flow`, `sx_dstar` | `sx_flow_set_trace`, `sx_dstar_set_trace` | `sx_search_trace` |
| `sx_decider` | `sx_decider_set_trace` | `sx_decide_trace` |
| `sx_machine` | `sx_machine_set_trace` (new) | `sx_machine_trace` |
| `sx_knowledge` | `sx_knowledge_set_trace` (new) | `sx_knowledge_trace` |
| `sx_areas` | `sx_areas_set_trace` (new) | `sx_areas_trace` |
| `sx_timers` | `sx_timers_set_trace` (new) | `sx_timers_trace` |
| `sx_outbox`, `sx_inbox` | `sx_outbox_set_trace`, `sx_inbox_set_trace` (new) | `sx_message_trace` |
| `sx_keeper` | `sx_keeper_set_trace` (new) | `sx_keeper_trace` |
| `sx_squad` | `sx_squad_set_trace` (new) | `sx_squad_trace` |
| `sx_crew` | `sx_crew_set_trace` (new) | `sx_crew_trace` |
| `sx_mission` | `sx_mission_set_trace` (being added) | `sx_mission_trace` |

**Free functions.** In C, area, smoothing and formation are not free
functions: they run through `sx_nav` (`sx_nav_reachable`, `sx_nav_way`,
`sx_nav_formation`), so the nav handle's trace covers them. The C free
functions that remain (`sx_route`, `sx_arrival`, `sx_points_around`,
`sx_sine`, `sx_sweep_heading`, `sx_alertness_*`, `sx_progress_*`,
`sx_interval_*`, the skill-table lookups) are transparent. Should a C free
function ever need a trace, it takes `sx_t_trace_fn fn, void *ctx` as its
last two parameters (NULL, NULL: off). No such function exists after this
round.

### No `sx_collector`

Decision: **no** C-side collector.

- A collector needs one handle type per record type (or `void *` copies
  and casts of function pointers, which is undefined behaviour in C).
- The callback already gives C the copy; a fixed ring buffer is a few lines
  and the game knows best what to keep (only `FINISHED`, only one member).
- It would be a second, C-only way to store traces, which is the kind of
  duplicate this round removes (the squad's built-in log goes for the same
  reason).

Instead `examples/c/trace.c` ships a 30-line bounded ring
(`DropOldest`, with a dropped counter) used with a nav and a squad trace,
and runs in `examples/c/run.sh`. See open question 1.

## Shared conversion (`sagax-abi`)

Every `XTrace -> flat codes` conversion lives in `sagax_abi::convert`, one
function per record, returning a plain Rust struct of integers
(`SearchTraceCode`, `DecideTraceCode`, ...) in the way `crew_event` /
`CrewEventCode` already works. The C layer copies the codes into its
`#[repr(C)]` record; WASM writes them into its number buffer. The kind
numbers come from `sagax_abi::codes` and are the same in C and WASM.
Wrapping records convert by calling the sub-record's function
(`squad_trace` calls `decide_trace`, `knowledge_trace`, `areas_trace`).

The C `#[repr(C)]` structs and the callback plumbing (`Option<(fn, ctx)>`
plus a `CallbackSink` that converts and calls) stay in `sagax-ffi`; one
generic helper there builds that sink for any record, so each module's
`set_trace` is three lines.

## WASM / JavaScript

Optional accessory: keep it compiling and its JS API unchanged. No new
traces in JS.

Minimal changes:

1. `WasmGrid` keeps its `TraceBuffer` but no longer installs a tracer on
   the `Navigator`; when tracing is on it calls the `*_traced` operations
   with a sink that converts through `sagax_abi::convert::search_trace`
   (and unwraps `NavTrace::Search`; other nav records are skipped). The
   9-number layout and `grid.setTrace` / `grid.takeTrace` stay.
2. `WasmDecider`: `DecideEvent` → `DecideTrace` (via
   `sagax_abi::convert::decide_trace`); `Ranked` is skipped and `cooled`
   is not exported, so the 6-number layout stays.
3. Nothing else uses traces in WASM (crew has none).

## Migration

| Old | New |
|---|---|
| `sagax_core::{Sink, NoopSink, CallbackSink}` via `sagax::` root | `sagax::debug::{Sink, NoopSink, CallbackSink}` |
| `sagax::debug` (crate alias) | `sagax::debug` module (see "Paths and exports") |
| — | `sagax_core::MapSink` |
| `SearchEvent<N>` | `SearchTrace<N>` |
| `sagax_nav::trace::{TraceEvent, TraceKind, Tracer, TraceSink}` | removed; `NavTrace` + `*_traced` |
| `Navigator::set_tracer(Some(..))` | `find_path_traced`, `continue_search_traced`, `reachable_traced`, `flee_traced`, `output_traced`, `formation_traced` |
| `DecideEvent` | `DecideTrace` (`Scored` gains `cooled`; new `Ranked`) |
| `StateMachine::update_traced(.., Sink<Change>)` | `update_traced(.., Sink<StateTrace>)` |
| `Squad::set_score_log`, `Squad::score_log`, `ScoreEntry` | removed; `plan_traced` / `check_traced` with `SquadTrace::Decide` |
| `Squad::check_with` / `plan_with` (unchanged) | plus `check_traced` / `plan_traced` taking rules and sink |
| `MissionTraceEvent::{Column, Factor, Scored, Picked}` | `MissionTrace::{Column, Decide, Rescored, Picked, Begin}` |
| `mission::Event` (prelude `Event`) | `mission::MissionEvent` (prelude `MissionEvent`) |
| C `sx_event`, `sx_trace_fn` | `sx_search_trace`, `sx_search_trace_fn` (flow, D*), `sx_nav_trace`, `sx_nav_trace_fn` (nav) |
| C `SX_TRACE_STARTED` .. `SX_TRACE_FINISHED` | `SX_SEARCH_TRACE_STARTED` .. `SX_SEARCH_TRACE_FINISHED` (same numbers) |
| C `sx_decide_event`, `sx_decide_trace_fn` | `sx_decide_trace`, `sx_decide_trace_fn` |
| C `SX_DECIDE_FACTOR`, `_SCORED`, `_CHOSEN` | `SX_DECIDE_TRACE_FACTOR`, `_SCORED`, `_CHOSEN` (+ `_RANKED`) |
| C `sx_score_event`, `sx_squad_set_score_log`, `sx_squad_score_log` | removed; `sx_squad_set_trace` with `SX_SQUAD_TRACE_DECIDE` |
| C `sx_mission_trace_event`, `SX_MISSION_TRACE_*` (being added) | `sx_mission_trace`, `SX_MISSION_TRACE_*` reshaped as above |
| C `sx_nav_set_trace(nav, sx_trace_fn, ..)` | `sx_nav_set_trace(nav, sx_nav_trace_fn, ..)` |
| C `sx_flow_set_trace`, `sx_dstar_set_trace` with `sx_trace_fn` | same names with `sx_search_trace_fn` |
| — (new) | `sx_machine_`, `sx_knowledge_`, `sx_areas_`, `sx_timers_`, `sx_outbox_`, `sx_inbox_`, `sx_keeper_`, `sx_squad_`, `sx_crew_set_trace` |
| WASM / JS | unchanged API |

## Tests

Per traced operation, in its own crate:

- **Same result:** the `_traced` call with a `Collector` gives the same
  return value and the same object state as the plain call (a few
  scenarios per module, square and hex where the module has both).
- **Content:** a small scenario whose trace is asserted exactly (e.g. a
  full knowledge table evicts the oldest: `Evicted(f)`, then `Added(g)`).
- **Off is off:** a sink whose `enabled()` is false receives nothing.
- **Nesting:** a crew tick trace contains the squad's `Decide` records for
  every candidate, and they equal the records of `Squad::check_traced` run
  on a clone.

C (`sagax-ffi` tests): every `sx_*_set_trace` is set, called, turned off;
kind codes and layout asserted; a lent handle's trace sees only its own
calls. The golden traces of earlier rounds keep passing (they read game
events, not traces).

## Build order

Starts **after the mission work has landed** (it edits the mission module,
`ffi/mission.rs`, the mission section of `sagax.h`). Each chunk ends green:
`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo fmt --check`, and the C examples
(`CC=gcc sh examples/c/run.sh` with a C compiler on `PATH`). Each chunk
adds its own CHANGELOG lines and `sagax::debug` re-exports; the chunks run
one after another, so no two touch a file at the same time.

**Chunk 1 — core and navigation.**
`MapSink`; `sagax::debug` module; `SearchEvent` → `SearchTrace`;
`SmoothTrace`, `FormationTrace`, `NavTrace`; the six `Navigator::*_traced`;
delete `sagax-nav/src/trace.rs`; `search_trace` / `nav_trace` conversions;
C `sx_search_trace`, `sx_nav_trace`, the three set_trace functions; WASM
point 1.
Files: `sagax-core/src/lib.rs`, `sagax/src/{lib,prelude}.rs`,
`sagax-nav/src/{lib,search,flow,dstar,nav,smooth,formation}.rs`,
`sagax-nav/tests/search.rs`, `sagax-abi/src/{codes,convert}.rs` (+ tests),
`sagax-ffi/src/{lib,grid}.rs`, `sagax-ffi/src/tests.rs`, `sagax.h` (nav and
tracing sections), `sagax-wasm/src/lib.rs`, `CHANGELOG.md`.

**Chunk 2 — decisions, state machine, mission.**
`DecideEvent` → `DecideTrace` (`cooled`, `Ranked`); `StateTrace`;
`MissionTraceEvent` → `MissionTrace` (wrap `DecideTrace`, `Rescored`,
`Begin`); `mission::Event` → `MissionEvent`; conversions; C
`sx_decide_trace`, `sx_machine_set_trace`, `sx_mission_trace`; WASM
point 2. The squad keeps compiling by renaming inside `ScoreEntry` only
(it goes in chunk 4).
Files: `sagax-decide/src/{lib,state,tests}.rs`, `sagax-decide/src/mission/*`,
`sagax-decide/src/group/squad.rs` (rename only), `sagax/src/prelude.rs`,
`sagax-abi/src/convert.rs` (+ tests), `sagax-ffi/src/{decide,machine,mission,group,tests}.rs`,
`sagax.h` (decide, machine, mission, squad sections), `sagax-wasm/src/lib.rs`,
`examples/c/mission.c` if it names renamed items, `CHANGELOG.md`.

**Chunk 3 — memory.**
`KnowledgeTrace`, `AreasTrace`, `TimersTrace`, `MessageTrace` and their
`_traced` operations; conversions; five C `set_trace` functions.
Files: `sagax-decide/src/memory/{knowledge,areas,timers,messages,mod,tests}.rs`,
`sagax-abi/src/convert.rs` (+ tests), `sagax-ffi/src/{memory,tests}.rs`,
`sagax.h` (memory section), `CHANGELOG.md`.

**Chunk 4 — group.**
`SpotTrace`, `KeeperTrace`, `SquadTrace`, `CrewTrace`; the squad and crew
`_traced` operations (routing through `MapSink`); remove the score log,
`ScoreEntry`, `LogSink`, `Scratch::log`; conversions; C `sx_squad_trace`,
`sx_keeper_trace`, `sx_crew_trace` and their set_trace; remove
`sx_score_event` and the score-log functions.
Files: `sagax-decide/src/group/{squad,orders,rules,spots,keeper,crew,mod}.rs`
and their tests, `sagax-abi/src/convert.rs` (+ tests),
`sagax-ffi/src/{group,crew,rules,tests}.rs`, `sagax.h` (group, crew
sections), `docs/ROADMAP.md` (close "squad trace sink"), `CHANGELOG.md`.

**Chunk 5 — docs and example.**
`docs/USAGE.md` section 10 rewritten around the rule (Rust: `_traced` +
`Collector`, nesting; C: `sx_*_set_trace`, a table of handles and records;
JS unchanged); short pointers from the module sections;
`examples/c/trace.c` with the ring buffer, added to `examples/c/run.sh`;
`docs/QUICKSTART.md` if it mentions tracing.

## Open questions for the owner

1. **C collector.** This design has none (reasons above) and ships a ring
   buffer as example code. *Recommendation: keep it so.* If C users ask for
   it, a later `sx_<t>_ring` per record type can be added without touching
   anything here.
2. **Transparent blocks.** `Alertness`, `Progress`, `Interval` and the pure
   functions stay untraced because their results already say everything.
   If "every module" should mean literally every one, they would each need
   a `_traced` form and, in C, an extra callback parameter on plain-struct
   functions. *Recommendation: keep them transparent.*
