# Mission planner (0.11.0, C API 11) — design

Status: approved for building (the owner asked to write it and build it in
one go). Target version 0.11.0, C API 11.

## Intent

Sagax can already decide (utility), keep state (state machine), remember
(world memory) and act as a group (squad, crew). What is missing is the
layer above: **why** a character does something and **how it sticks to that
over time**.

Many tactics games give their enemies a short script: walk here, wait,
walk there, start again. Such a script is fine as long as nothing happens.
When something does happen, scripted enemies either ignore it or forget
the script entirely. Two failures annoy players:

- an enemy whose job is already ruined (the target is dead, the thing it
  guards is gone) keeps standing around or walks its old loop;
- an enemy that was pulled away by a fight never returns to its job, or
  returns only because the script engine happens to resume.

The mission planner gives every character a **motive** (the goal it serves)
and a **plan** (the steps toward it). The game can hand over a ready plan,
for example its own scripted behaviour, and that plan comes first. When
something gets in the way, the character first solves that problem (a
**detour**) and then returns to the plan where it left. When the plan can
no longer work, the character drops it and picks a new plan for the same
motive, if its **leeway** allows that.

### The one hard rule: no cheating

The planner decides only from what the character knows: what it saw or
heard itself, what others told it, and what the mission told it up front.
It never reads the true state of the game. Every check of "is my plan
still possible?" reads the character's [`Knowledge`] and the condition
bits the game builds **from that character's knowledge**. If the target is
dead but the character does not know it, it carries on. That is a fair
mistake, not a bug.

Sagax has no sight test yet (that comes with the perception module); the
game decides what a character perceives and puts it into its knowledge.

## Goals

1. **Take over scripted plans.** A plan is a list of steps: go to, wait,
   follow, wait for, search, hold, attack, wander, signal, and the game's
   own kinds. A plan can loop (start again after the last step), like the
   patrol scripts of classic tactics games. A game can translate its
   script table into plans one to one.
2. **Motive.** Guard (a place or a subject), defeat (a subject or the known
   enemies), reach (a place), or a kind of the game. The motive stays when
   the plan changes. It also gives the default plan when the game hands
   none over.
3. **Detours.** The game (or its decider) says "deal with this first":
   engage a subject, flee, or a kind of the game. The mission puts the plan
   on hold, runs the detour and returns to the step it left, walking back
   first.
4. **Leash.** How far and how long a detour may pull a character away:
   distance from the anchor, time, and what to do on losing sight (return,
   search then return, keep going). Without a leash the detour runs until
   the subject is defeated.
5. **Leeway.** Per character: the leash, whether it may replan when its
   plan breaks, and whether it may widen the plan (call for help, ask its
   squad for a tactic). Presets for strict, flexible and free.
6. **Replanning.** When a plan breaks and leeway allows it, the mission
   scores the alternative plans the game gave for the motive (its
   playbook) and the built-in default plan, and picks one. Scoring runs
   through the library's own [`Decider`], so curves, weights ("personality"),
   ranks, ties and tracing work as everywhere else. A slot lets the game
   score plans itself, for example by a short trial run of its own
   simulation; a score of 0 is a veto.
7. **Never idle by accident.** If no plan fits, the character falls back to
   a sensible built-in plan for its motive (guard: hold the place; defeat:
   search the last known position; reach: go there), and only idles when
   it knows nothing to act on.
8. **Rust and C**, square and hex. JavaScript only when a game asks for it.

## Non-goals

- No perception, no sight lines. The game fills the knowledge.
- No planning search over world states (no GOAP, no HTN decomposition).
  Plans are lists the game writes or the built-in defaults; "planning" is
  choosing and filling one. A search planner can come later behind the
  same plan format.
- No built-in trial run. The game can do one through the score slot.
- No change to the squad or crew. A plan step can ask for a squad tactic;
  the game passes that to its squad or crew.
- No JavaScript binding in this round.

## Anti-goals (a failure even if it works)

- The planner uses anything the character cannot know.
- A character with a ready plan and nothing happening behaves differently
  from the plain script (same steps, same order, same arrival rule).
- A strict character starts doing things it was never told: patrolling,
  widening, chasing beyond its leash.
- A character whose plan broke stands still while it still knows something
  useful to do.
- Allocation per tick, or hidden randomness: all memory is reserved at
  creation; the seed comes from the caller; on equal scores the lower
  index wins; slots are called in a fixed, documented order.
- A game must learn many new concepts to run one scripted guard. Taking
  over a script must be: create mission, add plan, tick.

## Constraints

- Deterministic, integer only, fixed capacity, no global RNG.
- Public repo: no names of private projects in code, docs or tests.
- Breaking is allowed elsewhere, but this round only adds.
- Work on branch `feat/mission` in `.worktrees/mission`, one commit per
  finished step. CHANGELOG 0.11.0, C API 11.
- A game that must keep an original mode bit-identical runs the planner
  only in its improved mode; the planner never touches the game's own
  random numbers (it has none of its own).

## Concepts

### Where (a position that may be filled later)

Steps and motives name positions as `Where`:

- `Fixed(Pos)`: this cell.
- `Subject(u32)`: the last known position of that subject (its fact of the
  mission's subject kind in the character's knowledge).
- `MotivePlace`, `MotiveSubject`: the motive's place or subject.
- `Here`: where the character stood when the step began.

A `Where` that cannot be filled (subject not known) makes the step
impossible; see "When a plan breaks".

### Steps

| Step | Done when | Action the game gets |
|------|-----------|----------------------|
| `GoTo(where)` | within the arrive radius | go to the position |
| `Wait(ticks)` | the time has passed | wait |
| `Pause` | a fixed short wait (config) | wait |
| `Follow(subject)` | never by itself (until the plan moves on by a failure or the game finishes it) | follow, with the last known position |
| `WaitFor { subject, radius }` | the subject is known within `radius` of the character | wait |
| `Search { centre, radius, ticks }` | the time has passed (the points come from the world memory geometry, `points_around`) | go to the current search point |
| `Hold(where)` | never by itself | stand there |
| `Attack(subject)` | the subject is known as defeated or no longer known | attack, with the last known position |
| `Wander` | at once; the plan then ends without loop, the character wanders until replanned or given a new plan | wander |
| `Signal(u32)` | at once; reports an event the game reads (mission won, mission lost, ...) | none |
| `Custom { kind, at: where, param }` | when the game calls `finish_step` | the game's own action |

"Defeated" means the fact's `data` has the mission's defeated bits set
(config). The game sets them in the knowledge of a character when that
character learns of it.

Step options (all off by default, so a translated script behaves as the
script):

- `fails`: conditions that break the plan (see below), per step and per
  plan.
- `skip_if_impossible`: move on instead of breaking when the step's
  position cannot be filled.

A plan is: steps (up to the configured maximum), `repeat` (after the last
step start again at the first), and plan-wide `fails`. Plans are copied into
the mission; the game may reuse one plan for many characters.

### Motive

`Motive { kind, place: Option<Pos>, subject: Option<u32>, radius }` with
kind `Guard`, `Defeat`, `Reach`, `Custom(u32)`. `radius` is the area the
motive is about (guard area, target area). The default plan per kind:

- `Guard`: go to the place (or the subject's last known position) and hold
  there; with a subject, follow it instead.
- `Defeat`: attack the subject when known; else search its last known
  position; with no subject, attack the nearest known enemy (facts of the
  enemy kind, config) or search where one was last known.
- `Reach`: go to the place, then signal "reached" (a signal number from the
  config), then hold the place (so the signal is not sent again
  and again).
- `Custom`: none; the game must give a plan or a playbook.

### Leash and leeway

```text
Leash   { distance: Option<u32>, ticks: Option<u32>, on_lost: OnLost }
OnLost  = Return | Search { ticks, radius } | Keep
Leeway  { leash, replan: bool, widen: bool }
```

Presets (my call, the game can set every field):

- `STRICT`: distance 3, ticks none (ticks depend on the game's clock, so
  presets leave them to the game), on_lost Return, replan false, widen
  false. A strict guard defends itself, steps a little
  after an attacker and goes back; when its plan breaks it holds its last
  plan position (fallback), it never makes up a new plan.
- `FLEXIBLE` (default): distance 12, ticks none, on_lost Search { short },
  replan true, widen false.
- `FREE`: no distance, no ticks, on_lost Keep (that is "no leash": the
  detour ends when the subject is defeated), replan true, widen true.

The leash is measured from the **anchor**: where the character was when
the detour began (for `Hold` steps: the hold position). Distance uses the
mission's metric (the same `Metric` as the rest of the library; Euclid
compares squares).

### Detours

`Mission::detour(now, Detour)` with `Engage(subject)`, `Flee { from:
where, to: Option<Pos> }`, `Custom { kind, at: where, param }`. Every
character may take a detour, whatever its leeway: defending is always
allowed. A detour may replace a running detour (the new one wins, the
anchor stays the first one). The game decides **when** to start one, from
its own perception or decider; Sagax provides the "after" part.

A detour ends when:

- `Engage`: the subject is defeated or forgotten; or the leash pulls (too
  far from the anchor, or too long); or the subject is lost (not seen for
  the config's "lost after" ticks, read from the fact's `seen_at`) and
  `on_lost` says return. With `on_lost = Search` a search around the last
  known position runs first.
- `Flee`: arrived at `to`, or the subject is further than the config's
  "safe distance".
- `Custom`: the game calls `end_detour`.
- the game calls `end_detour` for any detour.

Then the mission returns: a `GoTo(anchor)` step runs first (unless the
resumed step goes somewhere anyway), then the plan goes on with the step
it left, keeping its state (a `Wait` keeps its remaining time; a search
restarts). If the plan broke during the detour, replanning happens now.

### When a plan breaks

Conditions (`Fail`):

- `Unknown { subject }`: the subject is no longer in the knowledge.
- `Defeated { subject }`: it is known as defeated.
- `Fact { subject, kind, mask, value }`: a fact's `data & mask == value`
  (e.g. "the item was stolen" as the game records it in knowledge).
- `Flags { mask, value }`: the game's condition bits for this character
  (`flags & mask == value`); the game must build them from what this
  character knows.
- `Custom(u32)`: the game's rules object answers.
- Implicit: a step position that cannot be filled (unless
  `skip_if_impossible`).

Checked every tick for the running step and the plan, in that order, not
while a detour runs (they are checked when it ends).

On a break:

1. With `replan`: score every playbook plan for the motive, plus the
   default plan, plus (with `widen`) the widening plans; pick one; it
   starts at its first step. A playbook plan whose own conditions already
   fail, or whose first position cannot be filled, scores 0.
2. Without `replan`, or when every candidate scores 0: the fallback. It is
   the default plan if that is possible, else `Hold(Here)`. A strict
   character always takes `Hold(Here)` (it never makes up a plan; it only
   does what it was told or nothing new).
3. Event `Broken { plan, reason }` and `Replanned { plan }` (or
   `FellBack`).

### Scoring plans

The mission owns a `Decider`. Each candidate plan is an option. Each row
has built-in columns:

- `IN_FEASIBLE` (1 or 0: its conditions hold now and its first position can
  be filled), `IN_DIST` (distance to its first position, in the metric's
  unit, capped), `IN_PREF` (the plan's preference 0..ONE, the game's
  "personality"), `IN_WIDEN` (1 for widening plans), `IN_KNOWN` (age of the
  freshest fact the plan relies on, capped), then the game's columns.

Built-in considerations on each option: step on feasible, the preference
as a multiplier. The game may add its own considerations through the
decider accessor, like the squad's. Pick: best by default (ties → lower
index), or weighted with the caller's seed (`tick` hashed with the
mission's id, the same `mix` the decider uses).

The rules slot `score(ctx) -> Option<u32>` may replace a candidate's score
(0 = veto). `columns(ctx, out)` fills the game's columns. `fail(ctx, id) ->
bool` answers `Fail::Custom`. All three default to "no change". Order:
per candidate in candidate order (playbook order, then default, then
widening): `columns`, then `score`.

### Widening

With `widen`, two more kinds of steps are allowed, and the playbook may
hold plans marked `widening`:

- `Call { channel, kind }`: emits an event "call for help" with the
  character's position and motive; done at once. The game sends it (for
  example as a `Message` through the world memory's outbox).
- `Squad { tactic, target: where }`: emits an event "wants squad tactic";
  done at once. The game hands it to its squad or crew.

The built-in widening plan for `Defeat` and `Guard`: call for help, then
the default plan. A non-widening character skips these steps (they count
as done), so one plan can serve both.

## API sketch (Rust)

```rust
let mut m = Mission::new(MissionConfig::default(), Motive::guard_place(gate))?;
m.set_leeway(Leeway::STRICT);
m.set_plan(Plan::looping(&[Step::go_to(a), Step::wait(100), Step::go_to(b)]))?;
// every tick:
let out = m.tick(now, me_pos, &knowledge, flags);        // or tick_with(.., &mut rules)
match out.action { Action::GoTo(p) => .., Action::Wait => .., .. }
for e in out.events() { .. }                              // StepDone, Broken, ...
// when the game's perception sees an attacker:
m.detour(now, Detour::Engage(attacker));
```

`tick` returns an `Output` with the current `Action` and up to a fixed
number of events. Events: `StepDone`, `Looped`, `PlanDone`, `Broken`,
`Replanned`, `FellBack`, `DetourStarted`, `DetourEnded { why }`,
`Returned`, `Signal(n)`, `Call`, `WantsSquad`.

Other calls: `set_motive`, `add_playbook_plan(plan, preference, widening)`,
`clear_playbook`, `finish_step`, `end_detour`, `plan()`, `step_index()`,
`decider_mut()`, `config()`, `snapshot`-style read access for tracing.

`MissionConfig`: `max_steps`, `max_playbook`, `subject_kind` (fact kind for
positions), `enemy_kind`, `defeated_bits`, `arrive_radius`, `lost_after`,
`pause_ticks`, `safe_distance`, `reached_signal`, `metric`, `search`
(a `PointPattern`), `hex_layout` (like the crew: positions in and out
in offset coordinates).

## C API sketch

`sx_mission_new(const sx_mission_config*, const sx_motive*)`,
`sx_mission_free`, `sx_mission_set_leeway`, `sx_mission_set_plan(m, const
sx_step*, count, flags)`, `sx_mission_add_playbook`, `sx_mission_tick(m,
now, pos, knowledge, flags, const sx_mission_rules*, sx_mission_out*)`,
`sx_mission_detour`, `sx_mission_end_detour`, `sx_mission_finish_step`,
`sx_mission_step_index`, `sx_mission_decider` (borrowed `sx_decider*`).
`sx_step` and `sx_fail` are plain tagged structs. `sx_mission_rules` is a
struct of function pointers plus `void *user`, `NULL` = built-in, like
`sx_squad_rules`. The knowledge handle is the existing `sx_knowledge`.
Number codes live in `sagax-abi`.

## Proof

1. Rust example and test `mission_guard`: two guards with the same looping
   patrol (go, wait, go). Nothing happens: the step trace equals the plain
   script. An attacker appears: the strict guard follows three cells and
   returns, the flexible one chases, loses sight, searches, returns and
   resumes at the step it left. The thing they guard is reported stolen to
   one guard: it replans (defeat the thief from the playbook), the other
   guard, who was not told, keeps patrolling.
2. The same scene in C (`examples/c/mission.c`), its trace diffed against
   the Rust trace, as for `ambush_rules`.
3. A test per anti-goal: no knowledge read outside the given `Knowledge`
   (the API makes it impossible: the mission gets nothing else), strict
   never replans, no allocation after creation (capacity tests), same
   seed same pick.

## Left to the builder

- Exact names and field order, as long as they follow the library's
  conventions (see the 0.9 API round).
- Whether `Where` resolution and step state live in one struct or two.
- Event capacity per tick.
- Where the mission module lives: `sagax_decide::mission` (my call), with
  re-exports in `sagax` and its prelude.

## Decisions after the builder check (my calls)

These settle the open points a builder found. They override anything
above that reads differently.

1. **Strict means `replan == false`.** Such a character never takes the
   default plan or a playbook plan on its own. When its plan breaks it
   first finishes any walk back to the anchor, then holds where it stands
   (`Hold(Here)`, "here" taken at that moment). `widen` only matters with
   `replan`.
2. **End of a plan without `repeat`.** Event `PlanDone`. With `replan`:
   score the playbook and the default plan as on a break (the finished plan
   itself is not a candidate on that tick). Without `replan`: hold where it
   stands. A plan that ends with `Wander` keeps the character wandering
   (action `Wander`) and does not replan until a fail condition of the
   motive fires or the game sets a new plan.
3. **One fact kind for positions.** `subject_kind` is the only fact kind
   the mission reads positions from. Enemies are the facts of that kind
   whose `data` has the config's `enemy_bits` set; `enemy_kind` is
   dropped. "Defeated" is `defeated_bits` in the same fact.
4. **Decider options are fixed slots.** At creation the mission adds one
   option per playbook slot (`0 .. max_playbook`), then the default plan
   (`max_playbook`), then the built-in widening plan (`max_playbook + 1`).
   Option numbers never change; `clear_playbook` empties the slots but
   keeps the game's considerations on them. An empty slot is not scored.
5. **No back-and-forth at the leash.** `detour` returns whether it was
   taken. An `Engage` is refused while the subject's last known position
   is outside the leash distance from the anchor (the anchor of the
   current or of the just-ended detour, until the character has returned
   and resumed; with no detour running, against the anchor the new detour
   would get). The leash pulls once the character stands beyond the
   distance, so it may be one step past it for one tick. A refused detour emits no event. Without a distance leash
   nothing is refused.
6. **Hex.** With a hex layout every position the game gives or gets is in
   offset coordinates, including the positions in the knowledge it passes
   in; the mission converts each fact position it reads. Without a layout
   positions pass unchanged.
7. Minor: a step that is done hands over to the next step within the same
   tick (at most one step change per tick, so a plan of many `Signal`s
   needs several ticks — my call, keeps the per-tick work bounded); a
   replaced detour keeps the first detour's start time for the leash
   ticks; a `Fact` condition on an unknown fact is false; the mission's
   id (for the seed and `points_around`) is a config field; the Euclid
   distance column is the rounded root, like the squad's.
8. Built during the build: the default plan has preference ONE/2 and the
   built-in widening plan 3/4·ONE, so a playbook plan can beat both;
   candidate `Fail::Custom` conditions are asked before `columns` and
   `score`; widening plans are candidates only on a real break (a fail or
   an impossible step), never at a plan end or before the first plan; a
   strict character always walks back to the anchor after a detour.
