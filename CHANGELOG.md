# Changelog

## 0.12.0 (unreleased)

The mission's open small items. A call for help now names the caller's
motive, a game can see why a replan took a plan, and the C interface reads
back the motive, the leeway and the running plan. **C API version 12.**
`SX_API_VERSION` is 12, so a game built with the new header refuses an
older library; `sx_mission_event` and `sx_mission_out` grew (see below).

### New

- **Traced missions:** `Mission::tick_traced(now, pos, knowledge, flags,
  rules, sink)` takes a `Sink<MissionTrace>`, like
  `Decider::decide_traced`; `tick_with` is `tick_traced` with `NoopSink`, so
  an untraced tick costs nothing extra. Each replan scoring sends, per
  candidate in option order, its row (`MissionTrace::Column`), the
  decider's own records of it unchanged (`Decide { option, event:
  DecideTrace }`: its factors and its score) and its score after the
  `score` slot (`Rescored { option, built_in, score }`), then the winner
  (`Picked`, `None` when every candidate scored 0). Each step that begins
  sends `Begin { index, at }` with the place its `Where` stands for (game
  coordinates; `None` for a step without one or an unknown place). A trace
  never changes an action or an event.
- **C:** `sx_mission_set_trace(mission, fn, context)` with
  `sx_mission_trace` (the decider's record embedded as `sx_decide_trace
  decide`) and the codes `SX_MISSION_TRACE_COLUMN`, `_DECIDE`, `_RESCORED`,
  `_PICKED`, `_BEGIN` (1 to 5).
- **C getters:** `sx_mission_motive`, `sx_mission_leeway` and
  `sx_mission_plan` (the summary in `sx_mission_plan_info`: step count,
  `repeat`, the plan's conditions; the first `cap` steps into the caller's
  buffer, as `sx_mission_set_plan` takes them). They read what
  `Mission::motive`, `leeway` and `plan` give in Rust.
- `sagax-abi`: `where_code` and `step_kind_code` (the inverses of `where_of`
  and `step_kind`).
- **One trace pattern** (navigation first): a trace is a `Sink` parameter.
  Every operation that decides something is `op_traced(.., sink)` with the
  sink last; the plain `op(..)` is `op_traced(.., &mut NoopSink)` and costs
  nothing extra. No Rust type stores a trace. Debug records end in `*Trace`,
  game output in `*Event`.
- `sagax_core::MapSink`: the one adapter between trace types; it maps each
  record and hands it on (`enabled` is the inner sink's). A facade passes
  `&mut MapSink::new(sink, Outer::Inner)` to the module it drives.
- **`sagax::debug` is a module**, the one place for tracing:
  `Sink`, `NoopSink`, `CallbackSink`, `MapSink`, `Collector`, `Overflow`,
  `Record`, `SearchTrace`, `NavTrace`, `SmoothTrace`, `FormationTrace`
  (`use sagax::debug::*;`).
- **`Navigator` traces:** `find_path_traced`, `continue_search_traced`,
  `reachable_traced`, `flee_traced`, `output_traced`, `formation_traced`
  report `NavTrace` records: `Search(SearchTrace)` (paths, areas, both
  searches of a flee), `Smooth(SmoothTrace)` (each line the smoothing
  tested), `Formation(FormationTrace)` (each member placement) and
  `FleeBest { cell, distance, cost }` (the cell a flee chose).
- `smooth::SmoothTrace` with `smooth_path_traced`; `formation::FormationTrace`
  (`Placed { member, step, cell, slot }`, `Unplaced { member, step }`) with
  `place_members_traced`, `place_members_hex_traced`;
  `Topology::smooth_traced`, `Topology::place_members_traced`;
  `nav::write_way_traced`. `sagax_nav::search` is a public module.
- **Decision traces:** `DecideTrace` (was `DecideEvent`) gains
  `Scored::cooled` (a cooldown forced the score to 0, so a 0 is no longer
  ambiguous) and `Ranked { rank }` (mixed ranks: only `rank` takes part;
  after the last `Scored`, before `Chosen`).
- **State machine trace:** `StateMachine::update_traced` reports
  `StateTrace` records: `Changed(Change)` and `Waiting { transition,
  elapsed, after }`, each transition of the current state whose conditions
  hold but whose `after` has not passed yet (checked before the one that
  fires): why the machine does not switch.
- `sagax::debug` also exports `DecideTrace`, `StateTrace` and
  `MissionTrace`.
- `sagax-abi`: `search_trace` / `SearchTraceCode`, `nav_trace` /
  `NavTraceCode`, `decide_trace` / `DecideTraceCode`, `state_trace` /
  `StateTraceCode`, `change` / `ChangeCode`, `mission_trace` /
  `MissionTraceCode` and the codes `SEARCH_TRACE_*`, `NAV_TRACE_*`,
  `DECIDE_TRACE_*`, `MACHINE_TRACE_*`, `MISSION_TRACE_*`, `REASON_*`,
  shared by the C and the WebAssembly layer.
- **C:** `sx_machine_set_trace` with `sx_machine_trace`
  (`sx_machine_trace_fn`; `SX_MACHINE_TRACE_CHANGED` with the `sx_change`,
  `SX_MACHINE_TRACE_WAITING` with `transition`, `elapsed`, `after`).
- **C:** `sx_search_trace` (`sx_search_trace_fn`) for flow fields and D*
  Lite, `sx_nav_trace` (`sx_nav_trace_fn`) for the navigator with the codes
  `SX_SEARCH_TRACE_STARTED` .. `_FINISHED` and `SX_NAV_TRACE_SEARCH`,
  `_SHORTCUT`, `_PLACED`, `_UNPLACED`, `_FLEE_BEST`. The nav trace covers
  every `sx_nav_*` call and `sx_path_cost`. A null handle to a
  `sx_*_set_trace` answers `SX_ERROR_ARGUMENT`.
- **Memory traces:** each call of the world memory that stores, drops or
  hands out something has a `_traced` form, and it reports what the return
  value leaves out (a refused merge, an evicted area, a timer that gave
  way). `Knowledge::learn_traced`, `merge_traced`, `learn_message_traced`,
  `merge_message_traced`, `expire_traced`, `expire_kind_traced`,
  `expire_with_traced` report `KnowledgeTrace` (`Added`, `Updated`,
  `Stale`, `Evicted`, `Expired`, each with the fact; a full table sends
  `Evicted` before `Added`; the `removed` callback of `expire*` stays).
  `Areas::add_traced`, `expire_traced` report `AreasTrace` (`Added { index,
  area }`, `Renewed { index, area }`, `Evicted`, `Expired`).
  `Timers::set_traced` reports one `TimersTrace` per call (`Started`,
  `Renewed`, `Replaced { id, duration, old, old_left }`).
  `Outbox::post_traced`, `due_traced`, `due_max_traced`,
  `sender_gone_traced` and `Inbox::push_traced`, `take_due_traced`,
  `take_due_max_traced` report `MessageTrace` (`Posted { msg, due_at }`,
  `Refused { msg, why }`, `Queued { msg, arrive_at }`, `Dropped { msg }`,
  `Delivered { msg }`). All four are in `sagax::debug`. `Alertness`,
  `Progress`, `Interval` and the pure functions stay untraced: their
  results already say everything.
- `sagax-abi`: `knowledge_trace`, `areas_trace`, `timers_trace`,
  `message_trace` (with `KnowledgeTraceCode`, `AreasTraceCode`,
  `TimersTraceCode`, `MessageTraceCode`), `refused`, and the codes
  `KNOWLEDGE_TRACE_*`, `AREAS_TRACE_*`, `TIMERS_TRACE_*`,
  `MESSAGE_TRACE_*`, `POSTED`, `REFUSED_FULL`, `REFUSED_COOLDOWN`.
- **C:** `sx_knowledge_set_trace` (`sx_knowledge_trace`: `kind`, the
  `sx_fact`), `sx_areas_set_trace` (`sx_areas_trace`: `kind`, `index`, the
  `sx_area`), `sx_timers_set_trace` (`sx_timers_trace`: `kind`, `id`,
  `duration`, `old`, `old_left`), `sx_outbox_set_trace` and
  `sx_inbox_set_trace` (both `sx_message_trace`: `kind`, the `sx_message`,
  `at`, `refused`), with the codes `SX_KNOWLEDGE_TRACE_*`,
  `SX_AREAS_TRACE_*`, `SX_TIMERS_TRACE_*`, `SX_MESSAGE_TRACE_*` (each from
  1). The handles of `sx_squad_knowledge` and `sx_squad_areas` take a trace
  too; it sees only the calls made through them.

### Changed

- **`mission::Event` is renamed `MissionEvent`** (also in the prelude; a
  bare `Event` was too generic). The variants stay.
- **`MissionEvent::Call` carries the caller's motive** (`motive: Motive`,
  the motive at the time of the call); the C `sx_mission_event` gains
  `sx_motive motive` (valid for `SX_MISSION_EVENT_CALL`). The struct grows
  from 72 to 104 bytes and `sx_mission_out` from 616 to 872.
  *Migration:* a Rust `match` that names every field of `Call` adds
  `motive` (or `..`); code that builds a `Call` sets it. C code recompiles
  against the new header; nothing else changes.
- **`DecideEvent` is renamed `DecideTrace`**; `Scored` gains `cooled`, and
  `decide_traced` sends `Ranked` when the ranks are mixed.
- **`StateMachine::update_traced` takes a `Sink<StateTrace>`** instead of a
  `Sink<Change>`; the change arrives as `StateTrace::Changed`.
- **C:** `sx_decide_event` is replaced by `sx_decide_trace` (the callback
  `sx_decide_trace_fn` keeps its name). `kind` is `uint32_t` with the codes
  `SX_DECIDE_TRACE_FACTOR`, `_SCORED`, `_CHOSEN` (the same numbers 1 to 3)
  and the new `_RANKED` (4); the product of `SCORED` moves from `value` to
  its own `product`; new `rank` and `cooled`; fields that do not apply are
  0 (`consideration` was -1). The struct grows from 24 to 36 bytes, and
  `sx_score_event`, which embeds it, from 36 to 48.
- **The navigator stores no tracer any more**: `Navigator::set_tracer`, the
  module `sagax_nav::trace` (`TraceEvent`, `TraceKind`, `Tracer`,
  `TraceSink`) are removed; pass a sink to the `_traced` calls instead.
  `SearchEvent` is renamed `SearchTrace` (same variants). The search,
  flow and D* Lite traced calls accept unsized sinks (`&mut dyn Sink<..>`).
- **The `sagax` root no longer re-exports `Sink`, `NoopSink`,
  `CallbackSink`** (they are in `sagax::debug`). The prelude exports no
  trace items.
- **C:** `sx_event` and `sx_trace_fn` are replaced: `sx_nav_set_trace` takes
  an `sx_nav_trace_fn` (search steps arrive as `SX_NAV_TRACE_SEARCH` with the
  step in `search`), `sx_flow_set_trace` and `sx_dstar_set_trace` take an
  `sx_search_trace_fn`. The search record gains `search` (the search's
  number) and `first`, `kind` is `uint32_t`, `found` a `uint8_t`; the kind
  numbers stay 1 to 5. The nav trace now also reports the smoothing, the
  formation and the flee's choice and the threats' search of a flee.
- **C:** `sx_squad_knowledge` and `sx_squad_areas` lend one handle each
  per squad (made on the first call, freed with the squad) instead of a
  pointer into the squad; `sx_knowledge_free` and `sx_areas_free` do
  nothing to them (before, freeing them was not allowed). Every call on
  them works as before.
- WebAssembly / JavaScript: unchanged (`grid.setTrace`, `grid.takeTrace`
  give the search steps as before).

### Migration table, 0.11 to 0.12

| 0.11 | 0.12 |
|---|---|
| `sagax::{Sink, NoopSink, CallbackSink}` | `sagax::debug::{Sink, NoopSink, CallbackSink}` |
| `sagax::debug` (the `sagax-debug` crate) | `sagax::debug` module: the same `Collector`, `Overflow`, `Record` plus the sinks and trace types |
| `SearchEvent<N>` | `SearchTrace<N>` (also `sagax_nav::search::SearchTrace`) |
| `sagax_nav::trace::{TraceEvent, TraceKind, Tracer, TraceSink}` | removed; `NavTrace` and the `_traced` calls |
| `nav.set_tracer(Some(..))`, then `find_path` | `nav.find_path_traced(.., &mut sink)`; likewise `continue_search_traced`, `reachable_traced`, `flee_traced`, `output_traced`, `formation_traced` |
| C `sx_event`, `sx_trace_fn` | `sx_search_trace`, `sx_search_trace_fn` (flow, D* Lite); `sx_nav_trace`, `sx_nav_trace_fn` (nav: the search step in `.search`) |
| C `SX_TRACE_STARTED` .. `SX_TRACE_FINISHED` | `SX_SEARCH_TRACE_STARTED` .. `SX_SEARCH_TRACE_FINISHED` (same numbers) |
| C `sx_nav_set_trace(nav, sx_trace_fn, ..)` | `sx_nav_set_trace(nav, sx_nav_trace_fn, ..)` |
| C `sx_flow_set_trace`, `sx_dstar_set_trace` with `sx_trace_fn` | the same names with `sx_search_trace_fn` |
| `DecideEvent` | `DecideTrace` (`Scored` gains `cooled`; new `Ranked`) |
| `StateMachine::update_traced(.., &mut impl Sink<Change>)` | `update_traced(.., &mut impl Sink<StateTrace>)`; the change is `StateTrace::Changed(change)` |
| `mission::Event` (prelude `Event`) | `mission::MissionEvent` (prelude `MissionEvent`) |
| C `sx_decide_event` | `sx_decide_trace` (`product` separate from `value`, unused fields 0) |
| C `SX_DECIDE_FACTOR`, `_SCORED`, `_CHOSEN` | `SX_DECIDE_TRACE_FACTOR`, `_SCORED`, `_CHOSEN` (same numbers; new `_RANKED`) |
| — | C `sx_machine_set_trace`, `sx_machine_trace` |
| — | `Knowledge`, `Areas`, `Timers`, `Outbox`, `Inbox`: `*_traced` with `KnowledgeTrace`, `AreasTrace`, `TimersTrace`, `MessageTrace` |
| — | C `sx_knowledge_`, `sx_areas_`, `sx_timers_`, `sx_outbox_`, `sx_inbox_set_trace` with `sx_knowledge_trace`, `sx_areas_trace`, `sx_timers_trace`, `sx_message_trace` |
| C `sx_squad_knowledge`, `sx_squad_areas`: a pointer into the squad, never freed | a lent handle per squad; `sx_knowledge_free` / `sx_areas_free` do nothing to it |

## 0.11.0

The mission planner. A character can now run a mission: a motive (what it
wants), a plan of steps, short detours (engage, flee) on a leash, and
replanning when the plan breaks. The mission sees only the `Knowledge` the
game passes in and moves nobody; it answers each tick with one action and
a few events. It is the step above the squad: a squad hands goals to its
members, a mission is what one member does with its day. Purely additive:
nothing of 0.10 changed, and the tests and golden traces of 0.10 pass
unchanged. **C API version 11.** `SX_API_VERSION` is 11, so a game built
with the new header refuses an older library.

### New

- **`sagax_decide::mission`** (re-exported in the `sagax` prelude):
  `Mission` (`new`, `set_plan`, `add_playbook`, `clear_playbook`,
  `set_leeway`, `set_motive`, `tick`, `tick_with`, `detour`, `end_detour`,
  `finish_step`, `step_index`, `in_detour`, `returning`, `decider`,
  `decider_mut`), `MissionConfig`, `Motive` (guard, defeat, reach, custom),
  `Plan` (`once`, `looping`, `fails_when`), `Step` with one constructor per
  kind (`go_to`, `go_to_where`, `wait`, `pause`, `follow`, `wait_for`,
  `search`, `hold`, `attack`, `wander`, `signal`, `custom`, `call`,
  `squad`; `fails_when`, `skip_if_impossible`), `Where`, `Fail` (unknown,
  defeated, fact, flags, custom), `Leeway` (`STRICT`, `FLEXIBLE`, `FREE`),
  `Leash`, `OnLost`, `Detour` (engage, flee, custom), `Action`, `Event`,
  `BreakReason`, `DetourEnd`, `MissionError` (`TooManyPlans` for
  `max_playbook` above 65535).
- **The rules object** `MissionRules` (with `BuiltInRules`, `FailCtx`,
  `PlanCtx`; in the prelude too, `BuiltInRules` as `MissionBuiltInRules`):
  three fixed points, lent per tick and never stored. `fail` (a condition
  of the game's own; `FailCtx::candidate` names the candidate plan while a
  replan checks its conditions), `columns` (the game's columns in each
  candidate plan's row) and `score` (replace or veto a candidate's score;
  veto what cannot run). `score` is the place for a trial run: see
  "Missions" in the guide.
- **Replanning:** on a break the mission scores the playbook plans, the
  default plan of the motive and, for a character that may widen, a
  built-in widening plan with a `Decider` and picks one (`MissionConfig::pick`).
  `Mission::default_option`, `widen_option`, the input columns `IN_*` and
  `DEFAULT_PREFERENCE`, `WIDEN_PREFERENCE` let a game add its own
  considerations. A playbook plan that just ended or broke is no candidate
  on that tick. A character that may replan and holds where it stands after
  a fallback looks again each tick and takes a plan as soon as one wins
  (one `FellBack`, then `Replanned`).
- **Flee** from the character's own cell (`from` is where it stands) runs
  `safe_distance + 1` cells along +x (or -x at the map bound) instead of
  standing still.
- **Leeway:** a strict character never takes another plan and walks back to
  its anchor after every detour; a flexible one replans; a free one also
  widens. An `Engage` is refused when its subject is outside the leash
  distance, so a guard does not run back and forth at the edge.
- **The default search pattern of `MissionConfig` is in cells** (first
  distance 1, distances 2, 4, 6), not in the memory blocks' fixed-point
  units; set `min` and `max` for a bounded map.
- **Hex:** with `MissionConfig::hex_layout` every position the game gives or
  gets, also the fact positions in the knowledge, is in offset coordinates;
  the mission converts.
- **C interface (`sx_mission_*`):** `sx_mission_config_default`,
  `sx_leeway_preset`, `sx_mission_new`, `sx_mission_free`,
  `sx_mission_set_leeway`, `sx_mission_set_motive`, `sx_mission_set_plan`,
  `sx_mission_add_playbook`, `sx_mission_clear_playbook`,
  `sx_mission_tick`, `sx_mission_tick_with` (with `sx_mission_rules`),
  `sx_mission_detour`, `sx_mission_end_detour`, `sx_mission_finish_step`,
  `sx_mission_step_index`, `sx_mission_in_detour`, `sx_mission_returning`,
  `sx_mission_decider`; the value types `sx_where`, `sx_fail`, `sx_step`,
  `sx_motive`, `sx_leeway`, `sx_mission_config`, `sx_detour`,
  `sx_mission_action`, `sx_mission_event`, `sx_mission_out`, the rule
  contexts `sx_mission_fail_ctx` (with `has_candidate`, `candidate`) and
  `sx_mission_plan_ctx`, and the
  `SX_STEP_*`, `SX_WHERE_*`, `SX_FAIL_*`, `SX_MOTIVE_*`, `SX_ON_LOST_*`,
  `SX_LEEWAY_*`, `SX_DETOUR_*`, `SX_ACTION_*`, `SX_MISSION_EVENT_*`,
  `SX_BREAK_*`, `SX_DETOUR_END_*` codes. Layouts are checked by the
  header's `_Static_assert`s.
- **Examples:** `cargo run -p sagax --example mission_guard` and
  `examples/c/mission.c`: two guards on one patrol, a strict and a flexible
  one, meet an attacker and a theft; the checks diff the Rust and C output.
- **JavaScript:** no binding yet; it follows on demand.

## 0.10.0

The slim round. A squad can now run a game's own logic at nine fixed points
(the rules object), the group code is split into a small core and a kit
that is built only from the public API, and the C and WebAssembly layers
share one crate of number codes. Code that sets none of the new things keeps
identical ticks, events and golden traces: every scene test of 0.9 still
passes unchanged. **C API version 10.** `SX_API_VERSION` is 10, so a game
built with the new header refuses an older library. The deprecated aliases
that 0.9 announced are gone (see the tables).

### New

- **The rules object** (Rust `SquadRules`, `BuiltInRules`; C
  `sx_squad_rules`): nine slots (targets, columns, place, member cost, spot
  score, trigger, abort, channel delay, report), lent per call (`plan_with`,
  `check_with`, `signal_with`, `reinstate_with`, `report_arrives_at_with`,
  `Crew::tick_with`, `Crew::reinstate_with`, `Crew::report_with`; C
  `sx_squad_plan_with`, `sx_squad_check_with`, `sx_squad_signal_with`,
  `sx_squad_reinstate_with`, `sx_squad_report_arrives_at_with`,
  `sx_crew_tick_with`, `sx_crew_reinstate_with`, `sx_crew_report_with`). The
  squad never stores the rules, so `Squad` and `Crew` stay `Clone` and
  `Debug`. A read-only `SquadView` (C `sx_view_*`, `sx_view_active_plan`)
  shows the squad to a slot. See "Your own pieces" in the guide.
- **Custom pieces:** `Place::Custom(id)`, `Trigger::Custom(id)`,
  `Abort::Custom(id)`, `Channel::Custom(id)` (C `SX_PLACE_CUSTOM`,
  `sx_tactic_trigger_custom`, `sx_tactic_add_abort_custom`,
  `sx_squad_set_channel_custom`); rule columns
  (`SquadConfig::rule_columns`); `REPORT_GAME_FIRST` (16) for a game's own
  report kinds, which go to the report slot together with every kind the core
  does not know; report actions (`ReportActions`, at most 4 per report: add a
  danger area, signal, forget a target; C `sx_report_*`) applied at one fixed
  point of a call.
- **`Squad::needs_rules`** and `PlanError::NeedsRules` (C
  `SX_ERROR_NEEDS_RULES` = -4): a squad with custom parts refuses the plain
  `plan`, `check` and `tick` instead of losing orders.
- **Helpers:** `spot_score_builtin` (C `sx_spot_score_builtin`),
  `sx_builtin_member_cost` and `sx_builtin_spot_score` (the two defaults as C
  callbacks to wrap, saturated to `int64_t`; `NULL` is the exact default),
  `sagax::rules::path_cost` and `PathCost` (C `sx_path_cost`): the member cost
  by the way walked.
- **Roles and columns:** `SquadConfig::roles` (a role list of up to 8 masks,
  `RoleCounts`; C `role_count`, `role_masks`; JavaScript `roles`),
  `IN_ROLE_0`, `IN_ROLE_1`, `Squad::role_column`, `Squad::extra_start`
  (C `sx_squad_extra_start`, JavaScript `extraStart()`), `Squad::rule_start`.
- **Skill table with 1 to 8 rows** (`SkillTable::rows`, `MAX_SKILL_ROWS`): a
  skill takes row `skill * rows / 256`. The default has 4 rows, so it reads as
  before. `SquadConfig::skill_table`.
- **Abort comparisons:** `Abort::When { input, cmp, value }` with `Compare`
  (`AtLeast`, `AtMost`, `Equal`, `NotEqual`), `Abort::at_least`; C
  `sx_tactic_add_abort_when` and `SX_CMP_*`; JavaScript `addAbortWhen`.
  `Tactic::abort_if_target_lost` (C `sx_tactic_set_abort_if_target_lost`,
  JavaScript `abortIfTargetLost`) switches off the hidden abort for a lost
  target; a target-age abort of the tactic still fires on a lost target.
- **The squad's decider and score log:** `Squad::decider`, `decider_mut`,
  `set_score_log`, `score_log` (C `sx_squad_decider`, `sx_squad_set_score_log`,
  `sx_squad_score_log`; JavaScript `decider()`, `setScoreLog`, `scoreLog`).
- **Core and kit:** the shipped tactics, tag and role bits and defaults moved to
  `group::kit`; the core never imports it (a test checks it). Public paths
  (`group::tactics`, `group::TAG_COVER`, ...) are unchanged.
- **`sagax-abi`**, an internal crate with every number code and checked
  conversion of the C and WebAssembly layers (no public API).
- **Example:** `cargo run -p sagax --example ambush_rules` and
  `examples/c/ambush_rules.c`: four guards ambush an intruder with the game's
  own sight test, hand signs, kill zone and "seen" abort; the checks diff the
  Rust and C output.

### Migration table, 0.9 to 0.10

Rust:

| 0.9 | 0.10 |
|---|---|
| `Abort { input, at_least }` | `Abort::at_least(input, at_least)`, or `Abort::When { input, cmp, value }`, or `Abort::Custom(id)` (an enum now; match arms must name the variants) |
| `SkillTable::level(s)` (associated function) | `table.level(s)` (a method; `skill * rows / 256`) |
| `SkillTable` arrays of 4 | arrays of `MAX_SKILL_ROWS` (8) and `rows: u8`; entries beyond `rows` are unused |
| `Squad::set_skill_table(table)` | returns `bool` (`false` for 0 or more than 8 rows, nothing changes); `#[must_use]` |
| `IN_EXTRA` as the first game column | `Squad::extra_start()` (`IN_EXTRA` is deprecated and right only with at most two roles) |
| `IN_RANGED`, `IN_AREA` | the same numbers; `IN_ROLE_0` and `IN_ROLE_1` are the names of the columns, `IN_RANGED` and `IN_AREA` stay for the default role list |
| `SquadConfig { members, memory, areas, inbox, dangers, metric }` | new fields `roles`, `skill_table`, `rule_columns`; write `..SquadConfig::default()` |
| a lost target always ended a plan | `Tactic::abort_if_target_lost` (default `true`: the same) |
| a report of any kind but 1, 2 and 4 was dropped | goes to `SquadRules::report` (dropped without rules, as before) |
| `Place`, `Trigger`, `Channel` with their old variants | each has a new `Custom(u32)` variant; an exhaustive `match` needs an arm |
| the shipped tactics, tags and roles inside the group module | in `group::kit`; the public re-exports (`group::tactics`, `Shape`, `TAG_*`, `ROLE_*`) are unchanged |
| `GoalKeeper::postpone(now)`, `resume(now)` | removed: `goal_postponed(now)`, `goal_won(now)` |
| `keeper::discipline(skill, &table)`, `group::discipline` | removed: `table.hold_bonus_for(skill)` |
| `Squad::resume(id, now)` | removed: `Squad::reinstate(id, now)` |
| `Areas::move_cost(from, to, start, goal)` | removed: `Areas::cell_cost(to, start, goal)` |
| `Navigator::resume(map, budget)` | removed: `Navigator::continue_search(map, budget)` |

C:

| 0.9 | 0.10 |
|---|---|
| `sx_skill_table` with arrays of 4 | `rows` and arrays of 8 (`pick`, `pick_top`, `think_ticks`, `timing_spread`, `hold_bonus`, `give_up_ticks`, `spot_rank_error`); the layout changed, the header's static asserts catch an old build; `sx_skill_table_default` has `rows` 4 |
| `sx_squad_config` | grew by `role_count`, `role_masks[8]` and `rule_columns`; `sx_squad_config_default` fills them (two roles, no rule columns); the crew config grew with it |
| `SX_IN_EXTRA` as the first game column | `sx_squad_extra_start(squad)` (the macro stays, deprecated, right only with at most two roles) |
| `SX_IN_RANGED`, `SX_IN_AREA` | the same numbers; `SX_IN_ROLE_0`, `SX_IN_ROLE_1` are the new names |
| `sx_tactic_add_abort(t, input, at_least)` | unchanged; `sx_tactic_add_abort_when(t, input, SX_CMP_*, value)` and `sx_tactic_add_abort_custom(t, id)` are new |
| a lost target always ended a plan | `sx_tactic_set_abort_if_target_lost(t, on)`, default on: the same |
| a report of any kind but 1, 2 and 4 was dropped | goes to the `report` callback of `sx_squad_rules` (dropped with `NULL`) |
| `sx_squad_plan`, `sx_squad_check`, `sx_crew_tick` on a squad with custom parts | `SX_ERROR_NEEDS_RULES` (-4); use the `_with` calls |
| `sx_squad_rules` | new; `sx_squad_rules_default` sets every pointer `NULL`, a zeroed table is the same |
| `sx_nav_create`, `sx_nav_destroy`, `sx_flow_create`, `sx_flow_destroy`, `sx_dstar_create`, `sx_dstar_destroy` | removed: `_new` and `_free` |
| `sx_squad_resume` | removed: `sx_squad_reinstate` |
| `sx_keeper_postpone`, `sx_keeper_resume` | removed: `sx_keeper_goal_postponed`, `sx_keeper_goal_won` |
| `sx_discipline(skill, table)` | removed (symbol and prototype): `sx_skill_table_hold_bonus_for(table, skill)` |

JavaScript and WebAssembly:

| 0.9 | 0.10 |
|---|---|
| `SHAPE` numbers 1 to 3 | 0 to 2 as C `SX_SHAPE_*`; `formation` takes the name or the number |
| `tactics.flank` with sides clamped | 2 or 3 sides, anything else throws `RangeError` |
| `decider.setCombine` with a negative option | only `ALL_OPTIONS` (-1, exported) means all; other negatives throw |
| radius, range and skill table entries given as fractions or out of range | throw `RangeError` (a skill table entry is limited to 0 .. 2^31 - 1 in JavaScript) |
| WASM `sx_tactic_shipped(kind, ..)` | `sx_tactic_rush`, `_formation`, `_flank`, `_ambush`, `_focus`, `_isolate`, `_fall_back` (internal exports; `lib.tactics.*` is unchanged) |
| the squad config keys `groupCap`, `memoryCap`, `areaCap`, `inboxCap` | removed: `members`, `memory`, `areas`, `inbox` (the old keys throw a `TypeError` that names the new key) |
| `squad.resume(id, now)` | removed: `reinstate` |
| `keeper.postpone(now)`, `keeper.resume(now)` | removed: `goalPostponed`, `goalWon` |
| `lib.discipline(skill, table)` | removed: `lib.holdBonusFor(table, skill)` |
| WASM `sx_discipline(skill, custom)` | removed (internal export; `lib.holdBonusFor` is unchanged) |
| `SQUAD.inExtra` | deprecated: `squad.extraStart()` |

Behaviour changes (no compile error, so read these):

- The skill level of a skill is `skill * rows / 256`. With the default four
  rows that is `skill / 64` as before; a table with other rows reads
  differently.
- `Tactic::abort_if_target_lost = false` removes only the hidden abort for a
  lost target. A tactic that also has a target-age abort (the shipped ones
  do) still aborts when the target is lost, because a lost target's age counts
  as the largest value: drop that abort too to keep a lost target.
- A squad with a custom piece or rule columns refuses the plain calls (see
  above). This cannot affect a squad built with the 0.9 API.
- JavaScript skill table entries are whole numbers from 0 to 2^31 - 1 (the
  WebAssembly module takes 32-bit signed numbers); C and Rust take the full
  `u32`.
- The built-in spot score: a spot that misses a wanted tag always scores worse
  than any spot that has them all, whatever the distance and whatever penalty
  you add (this is how `pick_spot` always worked). The shipped ambush scores
  the group-to-target distance on a curve of 0 to 100 and loses to rush on
  small maps; the example builds its own consideration.

## 0.9.0

The API round. 0.9 changes how the three interfaces look, not what the
blocks compute: every scene test keeps its tick numbers (the one change in
a result is listed under "Behaviour changes"). A change either fails to
compile, or fails loudly, or is listed below; old names stay as deprecated
aliases for 0.9 where that was cheap (Rust `#[deprecated]` forwarders, C
macros, JavaScript method aliases) and go in 0.10. **C API version 9.**
`SX_API_VERSION` is now 9, so a game built with the new header refuses an
older library.

### New

- **`Crew`** (Rust `sagax_decide::group::Crew`, C `sx_crew_*`, JavaScript
  `lib.crew()`): a `Squad` with one `GoalKeeper` per member, run as one call
  per tick that reports `Planned`, `Activated`, `Signal`, `Order`, `Start`,
  `Arrived`, `Broken` and `NoPlan`. It converts hex positions
  (`CrewConfig::hex_layout`, C `hex_grid`, JavaScript `hex`), sends honest
  arrival and goal-broken reports over the member's channel, drops the goals
  of a dead plan, and lends its squad and keepers to C and JavaScript.
  `member_inputs(id, now)` and `member_chose(id, now, goal_won)` close the
  loop with the member's own decider.
- **The `sagax` crate** (`crates/sagax`) with `sagax::prelude`: one `use` for
  a path, a decision and a crew. The parts are `sagax::nav`, `sagax::decide`
  and `sagax::debug`. `Decider::choose(candidates, rows, pick, seed)` takes
  rows instead of a flat buffer and returns `Result<Option<Choice>,
  DecideError>`.
- **Rule presets:** `GridRules::preset(Preset::Octile | Preset::Tabletop)`,
  `Topology::hex(layout, preset)`, C `sx_rules_default(grid, preset, &rules)`
  with `SX_PRESET_OCTILE` and `SX_PRESET_TABLETOP`, JavaScript `{ preset:
  'octile' | 'tabletop' }`.
- **A ready map with levels:** `DenseGrid::with_levels`, `get_at`, `set_at`,
  `add_wall_at`, `link` (stairs and ramps); C `sx_grid_new`, `sx_grid_set`,
  `sx_grid_add_wall`, `sx_grid_link`, `sx_grid_map`, `sx_grid_free`;
  JavaScript `lib.grid(w, h, { preset, levels })` and `grid.link(a, b)`.
- **`Budget`** (`Unlimited` or `Cells(NonZeroUsize)`) for search budgets and
  `Status::Idle` ("nothing to continue"; C `SX_IDLE`).
- **Positions:** `Pos`, `Cell` and `HexLayout` live in `sagax-core` and are
  shared by navigation and the AI blocks (`Cell::from(pos)`, `cell.pos()`,
  `HexLayout::to_axial_pos`, `from_axial_pos`); C `sx_point_to_pos`,
  `sx_pos_to_point`, `sx_hex_to_axial`, `sx_hex_from_axial`; JavaScript
  `lib.hex.toAxial` and `lib.hex.fromAxial`, and every position as `[x, y,
  z]`, `{x, y, z}` or `{x, y, level}`.
- **Squad:** `SquadConfig` (and `sx_squad_config`, `sx_squad_config_default`),
  typed danger reports (`report_danger`, with a danger queue), `take_activated`,
  `report_arrives_at`, typed channel and trigger setters (see the table).
- **Member side:** `KeeperInputs` (`IN_*` columns, C `SX_KEEPER_IN_*`,
  `sx_member_inputs`), `GoalKeeper::add_goal_option` and `set_current`,
  `Goal::start_tick` and `awaits_arrival`, `SkillTable::*_for` (one call per
  value), `Message::enemy`, `arrived` and `goal_broken` (C `sx_report_*`,
  JavaScript `Report`).
- `Areas::cell_cost(to, start, goal)`, the one way to feed danger areas to
  the path search (C `sx_areas_move_cost` and JavaScript `areas.cellCost`).
- **C layout checks:** `sagax.h` ends with `_Static_assert`s (C11) for the
  size and key offsets of every value struct, so a header and a library that
  disagree fail to compile.
- **Examples:** one squad scene in three languages that print the same lines
  and are diffed in the checks: `cargo run -p sagax --example squad_walk`,
  `examples/c/squad.c`, `examples/js/squad.mjs`; `examples/js/quickstart.mjs`;
  `examples/c/quickstart.c` gains a crew. `docs/QUICKSTART.md` is new; the
  guide has a Conventions section, a hex section and a group section that
  starts with the `Crew`.
- In JavaScript every handle has `[Symbol.dispose]` and the same "used after
  free()" guard; `Progress`, `Interval`, `Alertness` and `lib.arrival` run in
  the WebAssembly module, so they match C and Rust exactly.

### Migration table, 0.8 to 0.9

Rust:

| 0.8 | 0.9 |
|---|---|
| `GridRules::default()` | no `Default`; `GridRules::preset(Preset::Octile)` is the same numbers (straight 10, diagonal 14), `Preset::Tabletop` is 1 and 1 |
| `find_path(.., usize::MAX)`, `reachable(.., budget: usize)` | `Budget::UNLIMITED`; `Budget::from(NonZeroUsize)` (a plain `usize` does not compile; D* Lite budgets are still `usize`) |
| `Navigator::resume` | `Navigator::continue_search` (`resume` is a deprecated forwarder) |
| `Report.status: Option<Status>` | `Report.status: Status`; `Status::Idle` (the default) means nothing to continue |
| `hex::HexMap` (trait for hex maps) | removed; a hex map implements `CellMap`, asked with offsets (`dx`, `dy`) |
| `CellMap::steps(..)` | takes the offsets as a slice, `steps(from, from_start, offsets: &[(i32, i32)], out: &mut [Option<i32>])` |
| `DenseGrid::walls`, `set_walls`, `walls_mut` | `wall_bits`, `set_wall_bits`, `wall_bits_mut` (no alias; see "Behaviour changes" for the bit meaning on hex) |
| `Areas::move_cost(from, to, start, goal)` | `Areas::cell_cost(to: Cell, start: Cell, goal: Cell)` (`move_cost` is a deprecated forwarder) |
| `sagax_nav` types `Cell`, `HexLayout`, `Pos` | defined in `sagax-core`, still re-exported by `sagax-nav` and `sagax-decide` (same paths work) |
| `Knowledge::learn(fact, now)`, `merge(fact, now)` | `learn(now, fact)`, `merge(now, fact)` |
| `Knowledge::learn_message(msg, source, now)`, `merge_message(..)` | `learn_message(now, msg, source)`, `merge_message(now, msg, source)` |
| `Outbox::post(msg, now, delay, cooldown)` | `post(now, msg, delay, cooldown)` |
| `Areas::add(area, now)` | `add(now, area)` |
| `Decider::record(candidate, now)` | `record(now, candidate)` |
| `GoalKeeper::set(goal, now)` | `set(now, goal)` |
| `Squad::observe(fact, now)` | `observe(now, fact)` |
| `Squad::new(members, memory, areas, inbox, metric)` | `Squad::new(SquadConfig { members, memory, areas, inbox, dangers, metric })`; the new `dangers` is the danger queue and must be above 0 |
| `Squad::resume(id, now)` | `Squad::reinstate(id, now)` (`resume` is a deprecated forwarder) |
| `GoalKeeper::postpone(now)` | `GoalKeeper::goal_postponed(now)` (deprecated forwarder) |
| `GoalKeeper::resume(now)` | `GoalKeeper::goal_won(now)` (deprecated forwarder) |
| `keeper::discipline(skill, &table)` | `SkillTable::hold_bonus_for(skill)` (`discipline` is a deprecated forwarder) |
| `REPORT_DANGER` message with packed `strength` and `data` | `Squad::report_danger(sender, centre, shooter, arrive_at)`; a message of kind 3 is dropped when read |
| builders that answer `false` (`add_consideration`, `add_weighted`, `set_option_combine`, `set_rank`, `set_cooldown`, `Inbox::push`, `Squad::report`, `set_extra_columns`, `set_leader`, `set_channel`) | `#[must_use]`: a thrown-away answer warns |
| the `squad` example of `sagax-decide` | `cargo run -p sagax --example squad_walk` |

C:

| 0.8 | 0.9 |
|---|---|
| `sx_nav_create`, `sx_nav_destroy` (and `flow`, `dstar`) | `sx_nav_new`, `sx_nav_free` (and `flow`, `dstar`); the old names are macros for 0.9 |
| enum parameters `int32_t` (`rules.grid`, outputs, metrics, `evict`, `sx_route` rule, `sx_sine` kind, combine) | `uint32_t`; "all options" and "follow the decider" of `sx_decider_set_combine` are `SX_ALL_OPTIONS` and `SX_COMBINE_FOLLOW` (`0xFFFFFFFFu`, what -1 was) |
| bare `-1` error documented | only `SX_ERROR_*` (same values) |
| `sx_decider_record(d, cand, now)` | `sx_decider_record(d, now, cand)` |
| `sx_knowledge_learn(k, fact, now, ..)`, `sx_knowledge_merge(..)` | `sx_knowledge_learn(k, now, fact, ..)`, `sx_knowledge_merge(k, now, fact, ..)` |
| `sx_knowledge_learn_message(k, msg, source, now, ..)`, `sx_knowledge_merge_message(..)` | `sx_knowledge_learn_message(k, now, msg, source, ..)`, `sx_knowledge_merge_message(..)` |
| `sx_outbox_post(o, msg, now, delay, cooldown)` | `sx_outbox_post(o, now, msg, delay, cooldown)`; returns `SX_POSTED` (0) or the refusal |
| `sx_areas_add(a, area, now)` | `sx_areas_add(a, now, area)` |
| `sx_keeper_set(k, goal, now)` | `sx_keeper_set(k, now, goal)` |
| `sx_squad_observe(sq, fact, now, ..)` | `sx_squad_observe(sq, now, fact, ..)` |
| `int32_t sx_timers_left(t, id, now, uint32_t *out)` | `int64_t sx_timers_left(t, id, now)` (returns the time left) |
| `int32_t sx_alertness_value(a, uint32_t *out)` | `int64_t sx_alertness_value(a)` (returns the value) |
| `int32_t sx_areas_cost(..)` | `int64_t sx_areas_cost(..)` (returns the cost; negative is `SX_ERROR_*`) |
| `sx_squad_targets(sq, out, cap)` | `sx_squad_targets(sq, out, cap, uint32_t *total)`; returns how many were written, `*total` how many exist (may be null) |
| `sx_squad_new(members, memory, areas, inbox, metric)` | `sx_squad_new(const sx_squad_config *)`; `sx_squad_config_default` fills it (and adds the `dangers` queue) |
| `sx_squad_resume(sq, id, now)` | `sx_squad_reinstate`; `sx_squad_resume` is a macro for 0.9, the exported symbol is gone |
| `sx_squad_set_channel(sq, id, SX_CHANNEL_*, a, b)` and `SX_CHANNEL_*` | `sx_squad_set_shout(sq, id, range)`, `sx_squad_set_radio(sq, id, base, per_unit)`, `sx_squad_set_silent(sq, id)` |
| `sx_squad_add_shipped(sq, SX_TACTIC_*, param)` and `SX_TACTIC_*` | `sx_squad_add_rush`, `_formation(sq, SX_SHAPE_*, spacing)`, `_flank(sq, sides, dist)` (2 or 3 sides), `_ambush`, `_focus`, `_isolate`, `_fall_back` |
| `sx_tactic_set_trigger(t, SX_TRIGGER_*, value)` and `SX_TRIGGER_*` | `sx_tactic_trigger_none`, `_all_arrived(t, max_wait)`, `_target_in_radius(t, radius)`, `_manual` |
| `SX_REPORT_DANGER` | `sx_squad_report_danger(sq, sender, centre, shooter, arrive_at)` and `sx_crew_report_danger` (return 1 if stored, 0 when the danger queue is full) |
| `sx_keeper_postpone`, `sx_keeper_resume` | `sx_keeper_goal_postponed`, `sx_keeper_goal_won` (macros for the old names in 0.9) |
| `sx_discipline(skill, table)` | `sx_skill_table_hold_bonus_for(table, skill)` (`sx_discipline` stays, forwards, deprecated in the header comment) |
| status 0 had no name | `SX_IDLE` |
| `examples/c/squad.c` (gather, custom tactic, orders_due) | the same scene as the Rust and JavaScript examples, with `sx_grid`, `sx_nav_*` and `sx_crew_*` |

JavaScript:

| 0.8 | 0.9 |
|---|---|
| `lib.grid(w, h)` with default costs | a `preset` (or explicit costs) is required, `TypeError` without; `{ preset: 'tabletop' }` is the old JavaScript default (step 1), `'octile'` the Rust one |
| navigation cells `{x, y}` | `{x, y, level}`; area and flow cells `{x, y, level, cost}`; grid views hold `width * height * levels` entries |
| `learn(fact, now)`, `merge(fact, now)` | `learn(now, fact)`, `merge(now, fact)` |
| `learnMessage(msg, source, now)`, `mergeMessage(..)` | `learnMessage(now, msg, source)`, `mergeMessage(now, msg, source)` |
| `outbox.post(msg, now, ..)`, `areas.add(area, now)`, `decider.record(cand, now)`, `keeper.set(goal, now)`, `squad.observe(fact, now)` | `post(now, msg, ..)`, `add(now, area)`, `record(now, cand)`, `set(now, goal)`, `observe(now, fact)`; the old order throws |
| `lib.squad({ groupCap, memoryCap, areaCap, inboxCap, metric })` | `{ members, memory, areas, inbox, dangers, metric }`; the old keys work for 0.9, both keys for one field are a `TypeError` |
| `squad.setChannel(id, { ... })` | `setShout(id, range)`, `setRadio(id, base, perUnit)`, `setSilent(id)` |
| `tactic.setTrigger(..)` | `triggerNone()`, `triggerAllArrived(maxWait)`, `triggerTargetInRadius(radius)`, `triggerManual()` |
| `SQUAD.reportDanger` (report kind 3) | `squad.reportDanger({ sender, centre, shooter }, arriveAt)`; a message of kind 3 throws `RangeError` |
| `squad.resume(id, now)`, `keeper.postpone(now)`, `keeper.resume(now)` | `reinstate`, `goalPostponed`, `goalWon` (the old names are aliases for 0.9) |
| `lib.discipline(skill, table)` | `lib.holdBonusFor(table, skill)` (`discipline` forwards) |
| `grid.addHexWall(x, y, dir)` with `dir` outside 0..5, `grid.addWall` with a non-neighbour offset: ignored | `RangeError` |
| `grid.walls()`, `grid.setWalls()` on a hex grid | throw `TypeError` (the bits changed meaning); use `addWall` / `addHexWall` |
| `contains`, `addWall`, `addHexWall`, `hexNeighbors`, `setZones`, `addZone`, `pathTo`, `flowCost` on level 0 only | take `x, y` or one position and honour the level |
| roles, tags, fact `source`, report kind as numbers | strings (`'ranged'`, `'cover'`, `'own'`, `'goalBroken'`, ...); numbers still work |
| `Progress`, `Interval`, `Alertness` in JavaScript | the same constructors and fields, but they run in the WebAssembly module and need `loadSagax` first |
| `grid.reachable(cell, r)`, `setLeaveCost`, costs, budgets: coerced | whole numbers only; a fraction throws `RangeError` (the Foundry adapter now rounds its reach radius and leave cost down with `Math.floor`, so it keeps working) |

Behaviour changes (no compile error, so read these):

- **A danger area's `data` is now the sender**, the member that reported the
  shot (it was the shooter's id, which is now `source`). Nothing else in the
  scenes changed: every tick number is identical.
- **Hex wall bits are offset bits.** Bit `i` of a `DenseGrid`'s wall bits is
  the `DIRECTIONS[i]` offset on square and hex alike (it was hex direction `i`
  on hex). `add_wall` and `addHexWall` are unchanged and work for hex
  neighbours; only code that wrote raw bits (`set_wall_bits`, JavaScript
  `setWalls`, raw WebAssembly memory) is affected.
- **A level outside `0 .. levels` is off the grid** for a `DenseGrid` and
  `sx_grid`. Before, a one-level `DenseGrid` ignored the level of a cell, so a
  search "on level 3" worked; now it finds nothing.
- **JavaScript fails loudly where it used to coerce:** a fraction, a negative
  time, a zone above 31, a missing `preset`, a non-neighbour wall, an unknown
  string constant, a position with both `z` and `level` that disagree
  (`RangeError`), a value of the wrong type (`TypeError`), `Area` without
  `radius` and `lifetime`. The older wrappers no longer "still coerce", and a
  `decide` seed may now be a `BigInt`. `reachable(cell, r)` and `leaveCost` take
  whole numbers.
- `sx_squad_orders_due` (and `Squad.ordersDue`) on a crew's squad returns
  `SX_ERROR_ARGUMENT` (throws `TypeError`): the crew takes the orders. In Rust,
  do not call `Squad::orders_due` on `crew.squad_mut()`.
- Crew handles lent to C and JavaScript: the squad lives until
  `sx_crew_free`; a keeper is valid until the next call that changes the
  crew, and a stale keeper fails with `SX_ERROR_ARGUMENT` (JavaScript: an
  error that says "stale").
- Time order: the calls that take the tick a message arrives at keep the
  payload first (`Inbox::push(msg, arrive_at)`, `Squad::report(msg, arrive_at)`,
  `report_danger(.., arrive_at)` and their C and JavaScript twins), and
  `Alertness::stimulus`, `Progress::start` and `Progress::update` keep
  `now` where it was. All other time arguments move to the front.
- A hex `link` or `add_wall` with a square diagonal that is not a hex
  neighbour of that layout is accepted and never walked.
- WebAssembly: the raw exports (not the JavaScript wrapper) carry levels in
  every cell, take typed squad calls, and `sx_arrival` is new. The exports are
  internal to `js/sagax.mjs`.
- Only one search at a time per `sx_grid`: do not share one between threads
  while searches run.
- `sagax-ffi` builds as `cdylib` and `staticlib` and has no rustdoc page (it
  is a C API); the benchmark is the ignored test `bench_parts`
  (`cargo test -p sagax-ffi --release -- --ignored --nocapture bench_parts`).

## 0.8.0

- Group tactics, in `sagax-decide` (module `group`): a squad leader plans
  for a group, the members act on their goals with their own decisions.
  Integer only, deterministic, wrap-safe time, seeds from the game,
  capacities set once (after warm-up nothing allocates except one `Vec` per
  new plan).
  - `Squad`: member list, leader (the next active member takes over when it
    is lost, the plan stays valid), its own `Knowledge`, `Areas` and inbox
    (the leader plans only with what it saw or was told), tactic scoring per
    (tactic, target), planning with think time from skill, `check` (keep,
    replan, signal), `orders_due`, `signal`, `arrived`, `resume` (a member
    back in play gets its current goal again). A "Better" replan picks only
    among candidates above the hold bar; the pick's seed is mixed with the
    tick. A plan ends when its target is forgotten or not seen for 255
    ticks. Danger areas expire inside the squad.
  - Orders with a channel per member (shout with a range, radio, none; the
    default radio 0, 0 is instant) and a delay from the leader's position.
    An order's position is where to go now, its kind and start what to do
    there and when; a member's later order replaces one still on its way
    and never overtakes it. The signal lines up the starts (`AfterTicks`),
    so all members attack at the same tick. Members report back with
    `REPORT_GOAL_BROKEN`, `REPORT_ENEMY`, `REPORT_DANGER` and
    `REPORT_ARRIVED` (arrival with delay; `arrived` is the instant
    shortcut).
  - Tactics as data (slots, scoring, abort rules, trigger) and shipped ones:
    rush, formation (line, wedge, column), flank (gather at twice the
    attack distance, `Slot::gather_dist`, then attack on signal), ambush,
    focus, isolate (scored by the built-in `IN_TARGET_FRIEND_DIST`), fall
    back. They work with few or no spot tags (fall back to the nearest spot
    within the spot radius, then to the wish point). Spots count only
    within the spot radius of the wish point (default 3 times the slot's
    `dist`, or `Squad::set_spot_radius`).
  - `SkillTable`: what skill changes (which plan, how fast, how well, how
    long a goal is held). `GoalKeeper` for the member side, `discipline`,
    `suspend` / `resume` (the game's orders come first), `gather` (free
    characters of a faction within a radius).
  - `Areas::move_cost`, an adapter that turns areas into path costs.
- Rust `sagax_decide::group`; C `sx_squad_*` (including
  `sx_squad_set_spot_radius`), `sx_keeper_*`, `sx_tactic_*`,
  `sx_gather`, `sx_discipline`, `sx_areas_move_cost`; JavaScript
  `lib.squad()`, `lib.keeper()`, `lib.tactic()`, `lib.tactics`,
  `lib.skillTable()`, `lib.discipline()`, `lib.gather()`,
  `lib.areasMoveCost()`. The JavaScript group wrappers throw on a time or
  value out of range; the older wrappers still coerce. Seeds may be a
  BigInt for the full 64 bits. At most 1024 extra columns and 255 slots
  per tactic.
- Examples `squad` (a flank on a small grid: sighting, plan, radio orders,
  gather, signal, simultaneous attack) and `examples/c/squad.c`; section 8
  of the guide.
- C API version 8. `SX_API_VERSION` is now 8, so the version check of a game
  built with the new header refuses an older library; the functions are
  additive.

## 0.7.0

- Memory of the world, in `sagax-decide` (module `memory`), built from generic
  blocks that serve turn-based tactics, real-time strategy, stealth and
  action games alike. Integer only, no random numbers, wrap-safe time, fixed
  sizes; every time and limit is a parameter. The game decides what is
  perceived; the blocks keep, pass on and forget it.
  - `Knowledge`: facts keyed by (subject, kind) with place, observation
    time, source (own, heard, told), strength and data; evicts the oldest or
    the weakest; `learn`, `merge` (newer wins), per-kind expiry.
  - `Outbox`, `Inbox`, `route`, `arrival`: messages with a delay and a
    cooldown, recipients picked by `Nearest(k)`, `InRange(r)`, `All` or
    `To(id)` under a metric (Euclid, Manhattan, Chebyshev, hex), arrival
    times that grow with distance. Recipes: radio, shout, messenger, noise.
  - `Areas`: remembered or announced circles with a lifetime, blocking or
    with a cost per cell, for a map's `move_cost` (`blocks`, `cost`).
  - `Progress` (does a chase still get closer), `Timers` (ids that are on for
    a while), `Interval`.
  - `Alertness`: a value with stimuli, hold, decay, levels and hysteresis;
    the levels drive a state machine.
  - `points_around` (fine or coarse sine table), `sweep_heading`, `sine`.
- Rust `sagax_decide::memory`; C `sx_knowledge_*`, `sx_outbox_*`,
  `sx_inbox_*`, `sx_areas_*`, `sx_timers_*`, `sx_progress_*`,
  `sx_interval_*`, `sx_alertness_*`, `sx_route`, `sx_arrival`,
  `sx_points_around`, `sx_sweep_heading`, `sx_sine`; JavaScript
  `lib.knowledge()`, `lib.outbox()`, `lib.inbox()`, `lib.areas()`,
  `lib.timers()`, `Progress`, `Interval`, `Alertness`, `lib.route()`,
  `lib.arrival()`, `lib.pointsAround()`, `lib.sweepHeading()`, `lib.sine()`.
- Examples `officer` (the reference officer behaviour on the generic blocks,
  with a test) and `stealth_guard` (hex grid, alertness levels, shouts, lit
  cells), plus `examples/c/memory.c`; recipes in the guide (section 7).
- C API version 7. `SX_API_VERSION` is now 7, so the version check of a game
  built with the new header refuses an older library; the functions are
  additive and nothing was changed in the older ones.

## 0.6.0

- State machine with timers and aborts: states, transitions on condition
  bits (when / unless), a minimum time per transition, a timeout per state,
  transitions from any state. Rust `state::StateMachine`, C `sx_machine_*`,
  JavaScript `lib.machine()` with named states and conditions.
- Decisions remember actions: `record` what was done, hard cooldowns per
  option, and inputs for time since and count within a window (per option or
  per option and target) for soft dislike and penalties for repeating.
- C API version 6 (additive).

## 0.5.0

- Decisions, finer control: how factors are put together is chosen per
  decider and per option (product, geometric mean, minimum, average,
  median); a factor can stay outside as a true multiplier; a pull per
  factor makes it gentle or strict; options get ranks (emergency before
  combat); the current action can get a bonus against flipping. New curves:
  s-curve, inverse s-curve, root, cubic. Example `compare` and recipes in
  the guide. C API version 5 (additive: `sx_decider_add_weighted`,
  `sx_decider_set_combine`, `sx_decider_set_rank`, `sx_decider_set_current`).
  Rust: new `Curve` variants (breaking for exhaustive matches).

## 0.4.1

- Ways split their cost: JavaScript results carry `steps` (walking and
  terrain) and `zonesLeft` (zones left, e.g. attacks of opportunity) beside
  `cost`. Rust `DenseGrid::zones_left`.
- Foundry adapter: walker and enemies may be tokens or token documents.

## 0.4.0

- Decisions (utility AI), new crate `sagax-decide`: options with weights and
  considerations (raw input, curve, range), scored per pair of option and
  target in integer fixed point, compensated for the number of factors.
  Choice: best, or weighted / uniform among the top N with a seed from the
  caller. Trace per factor. Rust `Decider`, C `sx_decider_*` (API version
  4, additive), JavaScript `lib.decider()`.

## 0.3.0

- Several goals in one search: a way to the cheapest of them, e.g. to the
  nearest exit of a map. Breaking: Rust `find_path` takes a slice of goals
  (level `ANY_LEVEL` for any level, the `any_level` flag is gone), C
  `sx_nav_find_path` takes `goals, goal_count` (API version 3), JavaScript
  `findPath` takes a goal or an array.
- Fleeing: a way away from threats, to the cell the nearest threat needs the
  longest walk to reach. Rust `Navigator::flee`, C `sx_nav_flee`,
  JavaScript `grid.flee`.

## 0.2.0

- Tracing for debugging and logging: every step of a search (started,
  expanded, improved, paused, finished) for paths, areas, flow fields and
  D* Lite. Off by default and then free. Rust `Navigator::set_tracer`,
  C `sx_*_set_trace` (API version 2), JavaScript `grid.setTrace` /
  `grid.takeTrace`.

## 0.1.0

First public version: path search on square and hex grids with levels
(A*/Dijkstra with budgets, reachable areas, smoothing, formations, flow
fields, D* Lite), C interface (`sagax.h`, API version 1) and WebAssembly
with a JavaScript wrapper.
