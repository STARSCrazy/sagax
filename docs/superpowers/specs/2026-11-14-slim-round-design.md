# Slim round (0.10.0, C API 10) — design

Status: draft for approval. Target version 0.10.0, C API 10.

## Intent

Group tactics work, but the core still knows too much about the games it
was first built for. Places, triggers, aborts, channels, report kinds and
spot ranking are closed sets. A game that wants something the shipped
tactics do not cover has no place to plug it in: today it can only add
extra score columns, use `GoalKind::Custom` and fire `Trigger::Manual`
from outside.

This round turns group tactics into **a small general core plus optional
building blocks**, with **slots for the game's own pieces**. The shipped
tactics, roles, tags and the skill table become building blocks that use
the same public interface a game uses. A game can then take the library,
keep what fits and add one special piece of its own without touching or
forking Sagax.

The same slots are where the planned perception module (sight lines,
cover, "allies attack, so I join") will plug in later. Building the slots
now means perception becomes one more building block, not another rework.

The round also removes the helper code that the C and WebAssembly layers
implement twice and that has already started to drift apart.

### Why now

Sagax is in its feature phase. Interfaces may break; the games that use it
will adapt. What matters is that the library ends up as a clean, useful
toolset that takes work off the game without burying it.

### Test cases for "general enough"

Three things a stealth or tactics game would want and that are **not** in
the core today. They are the yardstick of this round, not features of it:

1. **Sneaking:** members prefer spots where the target cannot see them and
   pass orders by a silent channel that only works in sight.
2. **Planning an ambush:** wait hidden until the target walks into a kill
   zone the game defines, then strike together.
3. **Seeking cover:** under fire, pick spots that give cover *against this
   enemy*, not just spots with a "cover" label.

Each must be buildable by a game through the new slots, with no change to
the core. The round proves this with an example (see "Proof").

## Goals

1. **Generic core.** Squad, plan, goal, order, report, signal and time
   handling. The core does not name any concrete tactic, role or tag.
2. **Building blocks ("kit").** Shipped tactics (rush, formation, flank,
   ambush, focus, isolate, fall_back), the two role bits and their count
   columns, the four spot tags, formation shapes, the default skill table.
   They use only public interface and stay as short to use as today.
3. **Slots for the game's own pieces** at the points listed below, in Rust
   and C. One object per squad holds them (see "The rules object").
4. **Helpers** that make the slots easy to fill: ready-made rule
   implementations a game can wrap or reuse.
5. **One implementation of every binding helper**, shared by the C and
   WebAssembly layers, with the known drifts fixed.

## Non-goals

- No perception, no sight lines, no cover computation in this round. The
  proof example computes its own simple sight test as game code.
- No callbacks from JavaScript. JS keeps working with the building blocks
  and gets the renamed/moved API, nothing more ("JS only on demand").
- No new shipped tactics beyond what exists.
- No mission planner.
- C and WebAssembly stay separate layers; only their shared helpers move.

## Anti-goals (would be a failure even if it works)

- Using a shipped tactic becomes longer or harder than today
  (`squad.add_tactic(tactics::flank(3, 4))` must stay one line, in C one
  call).
- A framework of many registries, ids and plug-in objects a newcomer has
  to understand before planning anything. A game that uses no slots must
  never see them.
- Any change in behaviour for code that uses no slots: every existing
  scene (squad, crew, officer, stealth_guard examples and the golden
  traces of C/JS) keeps exactly the same ticks and events.
- New allocation per tick in the hot path where today there is none.
- Hidden non-determinism: the core calls slots in a fixed, documented
  order and never in hash order.

## Constraints

- Rust, C and JS stay in step for everything that is not a callback.
  Callback slots exist in Rust and C. (Project rule: every building block
  in Rust and C, square and hex; JS on demand.)
- Square and hex grids both work through every slot; positions in slot
  contexts are the same `Pos` the rest of group tactics uses.
- Deterministic: same inputs and seed → same plan. Slots get the seed or
  tick when they need one; they never get a global RNG.
- Public repo: no names of private projects in code, docs or tests.
- Breaking is allowed. Old names become deprecated aliases where that is
  cheap; the CHANGELOG gets a migration table "0.9 to 0.10" like 0.9 had.
- Work happens on a branch in `.worktrees/`, one commit per finished
  step.

## Part 1: Shared binding helpers

Today the C layer (`sagax-ffi`) and the WebAssembly layer (`sagax-wasm`)
each implement their own conversion code. Found duplicates:

- hex layout code ↔ `HexLayout`, hex conversion (C works on 3D positions,
  WASM on `(x, y)` only), budget conversion, metric code ↔ `Metric`
  (duplicated once more inside WASM `sx_route`);
- Place / Side / GoalKind / Start / Slot / Goal / MemberState /
  SkillTable codecs, the tactic builder, crew event codes, `to_i32`,
  status and plan-error codes.

Known drifts to fix:

- `set_combine`: C treats only `u32::MAX` as "all options", WASM treats
  any negative value as "all options". → One rule: one named constant
  ("all options") in each interface, everything else must be a valid
  option index, else argument error.
- WASM skill table casts negative values with `as u32` (wraps). → Error.
- WASM target radius and shout range truncate `f64` (NaN becomes 0);
  spot radius checks for fractions. → All numeric inputs from JS go
  through one checked conversion (whole number, in range, else
  `RangeError`).
- Flank sides: C rejects anything but 2 or 3, JS clamps. → Both reject.
- WASM doc says slot record has 19 numbers, the code uses 20. → Fix doc.
- JS `SHAPE` is 1–3 (shipped-tactic index), C `SX_SHAPE_*` is 0–2. →
  Same numbers everywhere.
- Crew event codes are named constants in C, literals in WASM. → One
  table.

**Decided (my call):** the shared code lives in one new workspace crate
used only by `sagax-ffi` and `sagax-wasm` (working name `sagax-abi`). It
holds the number codes and checked conversions, not public Rust API, so
it does not belong in `sagax-core` or the prelude. If the builder finds a
cleaner home that keeps it out of the public Rust API, that is fine.

## Part 2: Core and kit

Split `sagax_decide::group` into:

- **core** — `Squad`, `SquadConfig`, `Plan`, `Goal`, `Order`, `Message`,
  `Channel`, `Start`, `Trigger`, `Abort`, `Place`, `Slot`, `Tactic`,
  `GoalKeeper`, `Crew`, `SkillTable` (the type), the rules object.
- **kit** — `tactics::*` (all shipped tactics and the constants they use:
  flank max wait, fall back distance, ambush radius, target lost ticks),
  `ROLE_RANGED`, `ROLE_AREA` and their count columns, `TAG_*`, `Shape`
  and `formation`, `SkillTable::default()` values.

The core must not import from the kit. A test (or a build check) proves
it. The kit is re-exported where users find it today, so `use
sagax::prelude::*` still gives the shipped tactics.

Things that move from fixed to configurable while doing this:

- **Role count columns.** Today exactly two row columns count
  `ROLE_RANGED` and `ROLE_AREA`. New: the squad config lists the role
  masks to count (each gets one column); the kit's default list is those
  two, so default rows are unchanged. The start of the extra columns is
  then computed (a squad accessor replaces the fixed `IN_EXTRA`); with the
  default list it is today's value.
- **The hidden abort "target no longer known".** Today it is built into
  `check`. It becomes an explicit, documented switch on **every** tactic
  (`Tactic` field, on by default, also for tactics built by hand in Rust,
  C or JS), so no existing tactic loses it. A tactic can switch it off.
  *(My call.)*
- **Abort comparisons.** Today only `row[input] >= at_least`, OR-combined.
  New: a comparator (`>=`, `<=`, `==`, `!=`); the old form is the `>=`
  case.
- **Skill levels.** Today fixed four levels (`skill / 64`). New: the table
  has 1 to 8 rows, level = `skill * rows / 256`. The default table keeps
  four rows with today's values.
- **Report kinds.** Core keeps its kinds (goal broken, enemy, arrived).
  All other kinds are passed to the rules object instead of being
  dropped silently.
- **The squad's decider** becomes reachable (combine mode, tracing), as
  every other decider already is.

## Part 3: The rules object (slots)

One rules object per squad. It has one method per slot. Every method has
a default that does exactly what the core does today, so a game overrides
only what it needs and a squad without rules behaves as before.

**Ownership (my call): the game owns it and lends it per call.** `plan`,
`check` and `Crew::tick` take the rules as an optional argument (Rust
`Option<&mut dyn SquadRules>` or an equivalent, C a pointer that may be
`NULL`). The squad never stores it. So the game keeps its own map,
`Navigator` and state, can change them between ticks, and `Squad` and
`Crew` stay `Clone`/`Debug`. Calls without rules are the built-in
behaviour.

- Rust: a trait (working name `SquadRules`) with default methods. Methods
  take `&mut self`, so the rules may keep their own state (for example a
  flag "a guard was seen" set by the report slot and read by the abort
  slot).
- C: one struct of function pointers (`NULL` = built-in behaviour) plus
  one `void *user` passed to every call, the same pattern as the
  existing navigation callbacks.

Custom variants with a small number id (`Place::Custom(id)`,
`Trigger::Custom(id)`, `Abort::Custom(id)`, `Channel::Custom(id)`) let a
tactic or member point to game logic; the core passes the id to the
matching rules method. This keeps tactics plain data (C and JS can still
build them) while the logic lives in the game.

Slots, in the order the core calls them in a plan:

| # | Slot | Called when | Gets | Returns | Built-in default |
|---|------|-------------|------|---------|------------------|
| 1 | **targets** | choosing which known facts are targets | the leader's facts of the target kind | keep / drop per fact | keep all |
| 2 | **columns** | building the score row of (tactic, target) | tactic id, target, squad view | fills the extra columns | none (game fills extra columns by hand, as today) |
| 3 | **place** | `Place::Custom(id)` needs a wish point | id, slot, leader, centre, target, friend, facing | a point | — (only for custom) |
| 4 | **member cost** | assigning members to slots | member, wish point, slot | a cost or "cannot" | metric distance |
| 5 | **spot score** | ranking spots for a slot | spot, wish point, slot, member, target, tags wanted | a score (lower is better) or "unusable" | (tag miss, distance, index), as today |
| 6 | **trigger** | `Trigger::Custom(id)` is checked | id, plan, squad view, tick | fire / not | — |
| 7 | **abort** | `Abort::Custom(id)` is checked | id, plan or candidate, row, tick | abort / not | — |
| 8 | **channel delay** | `Channel::Custom(id)` sends an order or report | id, sender, receiver, distance | delay in ticks or "does not arrive" | — |
| 9 | **report** | the leader reads a report of a non-core kind | the message, tick | nothing (the game may set areas, signal, mark targets) | drop |

"Squad view" is a read-only look at members, positions, active plan and
known targets; no slot can change the squad directly while the core is
inside a plan or check. The C struct documents that a callback must not
call functions on the same squad.

Details of the slots (my calls, after the builder check):

- **Report slot actions.** The report slot may hand back a small list of
  actions: add a danger area, fire the signal, forget a target. The core
  applies them at one fixed point: after all reports of this `check` are
  read, before the abort and trigger checks. Everything else the game
  does in its own rules state.
- **Columns slot comes on top.** The caller's `extra` columns stay as
  today. The squad config declares how many columns the rules fill; they
  come after the caller's columns, and the slot gets exactly that
  writable slice. Column counts are checked as today. JS (no callbacks)
  keeps filling extra columns by hand.
- **Coordinates.** A slot always sees the positions of the object it is
  called through: a `Squad` passes its own `Pos` (axial on hex, as
  today); a `Crew` with a hex layout converts to and from offset
  coordinates around every slot call, so a game's map and `Navigator`
  fit directly.
- **Channel slot is for orders.** In the `Squad`, channels time orders
  only; reports keep the `arrive_at` the caller gives. The `Crew`, which
  computes report arrival itself, uses the same channel (built-in or
  custom) for reports. Built-in channel ticks do not change.
- **Targets slot only filters new candidates.** It does not abort an
  active plan. A game that wants "target hidden → abort" uses
  `Abort::Custom`.

The spot score replaces the ranking but keeps the core's rules around it:
skill-based rank error, separate `taken` masks for gather and place
spots, tag wish as an input. A game that only wants "cover against this
enemy" scores spots itself; the tags stay available as a cheap filter.

**Decided (my call):** one rules object, not nine separate registries.
It is one thing to learn, one thing to pass in C, and perception can
later ship as a helper that a game's rules object calls.

## Part 4: Helpers

So the slots save work instead of adding it:

- **Path-based member cost** (in the `sagax` umbrella crate, because it
  needs navigation): uses a `Navigator` and the game's map to give the
  real way length instead of metric distance. Rust and C.
- **Spot score helpers:** the built-in ranking as a public function, so a
  game can wrap it ("built-in score, plus 50 if the target sees the
  spot") instead of rewriting it.
- **Rules combinators (Rust):** a default struct that implements every
  slot as built-in, for `..Default` style overriding; in C a function
  that fills the struct with the built-in pointers.

The builder may add small helpers of the same kind if a slot turns out
awkward in the proof; each one must be in Rust and C.

## Proof

A new example **`ambush_rules`** (Rust and C, square grid, run through
the `Crew` so reports use the channels too; the C output
diffs against the Rust golden trace like the squad example does). A
squad of guards and one intruder, with a small game-side sight test
(Bresenham over walls, written in the example, not in the library):

- spot score: hidden from the intruder's sight scores better (sneaking,
  cover against this enemy);
- channel: hand signs that only arrive in sight;
- trigger: the intruder steps into a kill zone the example defines;
- abort: the intruder saw a guard (custom report kind from the guard who
  was seen);
- the shipped `ambush` tactic adapted with these custom parts.

The example must build without any change to core files. Tests assert
the events: all guards wait hidden, one signal, the attack starts on one
tick, and when a guard is seen the plan aborts.

## Release

- Version 0.10.0, C API 10; header aliases for renamed C names where
  cheap; migration table "0.9 to 0.10" in the CHANGELOG; USAGE section
  "Your own pieces" with the rules object and the proof as recipe.
- After the push and green CI: tell the games what changed.

## Left to the builder

- Exact Rust type and method names, the C struct layout, how the squad
  view is shaped, module paths of core and kit.
- How custom ids are numbered (one id space per slot or shared).
- How the "core does not import kit" rule is checked.
- Task order and size of the plan.
