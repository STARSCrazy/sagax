# Group tactics (decide, stage 4) — design

Status: draft for approval. Target version 0.8.0, C API 8.

## Intent

Game agents should act together: hold a formation, rush one target, come
from several sides, set an ambush, combine weapons (one keeps distance and
height, one throws into crowds), agree on a shared target, cut one enemy
off from his friends.

Today a game writes all of this by hand. Sagax should give it generic
blocks so a game can get squad tactics without writing a squad AI — and
still keep full control over who is a group and what the roles mean.

Fun matters more than strength. Weak enemies must look weak.

## Two levels

1. **The rough plan (group).** A *squad leader* (a piece of code, not
   necessarily a figure) looks at what the group knows, picks a tactic and
   gives every member a **goal**. The plan holds as long as it works.
2. **Acting in the moment (member).** Each member decides with the existing
   `Decider`. The group goal is *one* option among others (shoot, take
   cover, flee). Normally the goal wins; a surprise (an enemy suddenly in
   front of him) makes another option win. The member postpones the goal
   and picks it up again when things calm down, or reports that the plan
   broke.

This is the pattern of shipped squad AIs: the squad hands out goals, the
individual fulfils them in its own way.

## Goals

- New module `group` in `sagax-decide`, next to `state` and `memory`.
- Same shape in Rust, C (`sx_*`) and WebAssembly/JS. Square and hex maps
  work the same way (positions are plain integers; distances via the
  existing `Metric`).
- Deterministic: caller-given `u32` tick, seed from the caller, no global
  RNG, no floats in decisions, documented tie-breaks (lower index wins).
- Fixed size: capacities at creation, nothing allocates while running.
- Every time, radius and threshold is a parameter.

## Non-goals

- **Who is a group is decided by the game.** Sagax gets a finished member
  list. It only offers a helper "all agents of faction F within radius R of
  an initiator" that the game may use.
- No perception inside: no line of sight, no cover search. The game passes
  candidate spots (see below). Sight lines and cover come with the later
  perception module.
- No path finding inside. The game (or `sagax-nav`) supplies distances.
- No mission planner. That is the next stage; it will hand goals to groups
  the way groups hand goals to members.

## Anti-goals (a failure even if it works)

- **Cheating.** The leader plans only with what members *know* (their
  `Knowledge`), never with the true world state. Knowledge reaches the
  leader and other members only through messages (`Outbox`/`Inbox`, shout
  near, radio far, both with delay).
- **Instant coordination.** Making and spreading a plan costs ticks.
  A plan can be stale when it arrives.
- **Weak enemies playing perfectly.** Skill must visibly change what a group
  can do and how well.
- Game names or game-only roles in the API (`police`, `sniper`, `grenade`).
  Roles are neutral numbers with caller meaning; docs say "ranged", "area",
  "guard".
- Plan flicker: switching tactic every tick.

## The building blocks

All positions `Pos` (`i32` x, y, z) in caller units; age = `now.wrapping_sub(since)`.

### 1. `Group` — the member list

Fixed capacity. Per member: `id: u32`, `pos`, `role_mask: u32` (what this
member *can* do, caller bits, e.g. bit 0 = ranged, bit 1 = area),
`skill: u8` (own skill), `state` (active / busy / gone), `goal` (see 3).

- `add`, `remove`, `set_pos`, `set_state`, `iter` in slot order.
- `gather(initiator_pos, faction, radius, candidates, metric)` — the
  optional helper: from a caller list `(id, faction, pos)` take those with
  same faction within radius, nearest first, ties → lower id, up to
  capacity.

### 2. `Tactic` — a plan as data

A tactic is data, not code: a list of **slots** plus scoring and abort
rules.

- **Slot:** required role bits, count (min, max), how its spot is chosen
  (offset from leader for formations; side relative to the target —
  front / left / right / rear; "candidate spot with tag T"), what the goal
  is (move, hold, attack, wait for signal), timing (now / on signal /
  after N ticks).
- **Score:** `Consideration`s over a caller row (as in `Decider`), e.g.
  "target unaware", "spots with tag hidden near", "members with ranged".
- **Abort rules:** threshold checks over the same row ("own losses ≥ k",
  "target not where expected for > T ticks", "members off plan ≥ k").
- `min_skill: u8` — weaker groups do not know this tactic.

Sagax ships a few ready tactics built only from these parts: **rush**,
**flank** (two or three sides), **formation** (line, wedge, column),
**ambush** (hold hidden spots, attack on signal), **focus** (all on one
target), **isolate** (pick the target farthest from its friends, place
members between it and them), **fall back**. A game can change them or add
its own.

### 3. `Goal` — what a member gets

`kind` (move, hold, attack, wait, retreat — plus caller kinds), `pos`,
`target: Option<u32>`, `start: Signal | Tick`, `slot`, `issued_at`.

The member feeds the goal into its own `Decider` as inputs: "has goal",
"distance to goal spot", "goal target visible", "signal given". The goal
option should carry the stage-1 *current bonus* so it is not dropped
lightly. How strongly a member clings to it is a parameter scaled by its
skill (disciplined members hold longer).

### 4. `Squad` — the leader logic

Holds the group, the known tactics, the active plan and its timers.

- `plan(now, row, spots, targets, seed) -> Option<Plan>`: score known
  tactics, pick with the stage-1 `Pick` (best, or weighted from top-N; the
  seed comes from the caller), assign members to slots, choose spots and
  targets. Assignment is deterministic: slots in order, for each the
  best-fitting free member by role then distance, ties → lower index.
- **Plan time:** a new plan becomes active only after
  `think_ticks` (from skill) and reaches each member via the message blocks
  (shout or radio delay). Until then members act on their old goal.
- `check(now, row) -> Status`: keep / replan. Replan when an abort rule
  fires, or when another tactic beats the active one by more than the
  *hold bonus* (no flicker).
- `report(member, event)`: a member says "my goal broke" or "new enemy
  at X" — this too arrives only as a message.
- `signal(now)`: give the go for "on signal" goals.

**Inputs from the game:**
- `row`: the scoring inputs (`i32` values, caller-defined) — built from the
  members' knowledge, not from the true world.
- `spots`: candidate positions with tag bits (cover, high, hidden, choke)
  and distances to members/target, from the game or `sagax-nav`.
- `targets`: known enemies `(id, pos, strength)` from the members'
  `Knowledge`.

### 5. Skill

One `skill: u8` per group (0 weak … 255 elite) plus the member's own skill.
It drives, through parameter tables the caller can change:

- **Which tactics are known** (`min_skill`).
- **How well the tactic is chosen:** high skill → `Best`; low skill →
  weighted pick from a wider top-N (sometimes a mediocre plan).
- **How well it is carried out:** spot error (spot picked from a worse
  rank), timing spread (some members start a few ticks early or late,
  from the seed), chance a member ignores its slot.
- **How fast:** `think_ticks` and reaction to aborts.
- **Discipline:** goal hold bonus for members.

All random effects come from the caller seed mixed with member index and
tick (`mix`), so a replay is identical.

## Decisions after review

These settle the open points of the builder check. Each one is the
designer's call, not something the user decided.

1. **The leader is a member figure.** Its slot is set by the caller. If it
   is gone, the next active member in slot order takes over. **The active
   plan stays valid:** members keep their goals, and the new leader keeps
   checking the same plan. Losing the leader is not an abort reason by
   itself; it only counts as one loss in the normal abort rules. A new plan
   comes only if the new leader's `check()` says so, and then it costs
   `think_ticks` as usual. All message delays are measured from the
   leader's position. (User decision.)
2. **The Squad keeps its own leader memory.** It holds a `Knowledge` and an
   `Inbox`, fed only by member messages and the leader's own sightings
   (`observe`). Targets come from this memory, not from the caller. That is
   how the "no cheating" rule is enforced in code. The caller row only adds
   extra inputs.
3. **Orders are a new message type `Order`.** Fields: goal, slot, target,
   start, issued_at, plan id. They have their own outbox in the Squad. The
   caller gives each member a channel (shout / radio / none) and a delay
   rule (`arrival`). The existing `Message` stays unchanged.
4. **Spots are always picked, never invented.** Formations, sides and
   "between target and friends" compute a *wish point*. The slot then takes
   the best candidate spot near that point (tag fits, nearest, ties → lower
   index). With no candidate spot, the slot gets no position goal.
   - "Front" is the direction from the group centre to the target.
   - Formations turn in 8 steps on square maps and 6 on hex maps.
   - `spots` is a flat list `(pos, tags)` with a fixed capacity.
     Distances use the `Metric`. A game may override them with a callback
     or a precomputed list, for example path lengths from `sagax-nav`.
5. **Scoring is per (tactic, target),** like Decider candidates. Tactics
   without a target are scored once. The Squad fills these inputs itself:
   own losses, members off plan, target age (ticks since last seen),
   distance from the group to the target, and the group's ranged and area
   counts. The caller can add more columns after these.
6. **No change to the Decider.** The group gives each member ready inputs:
   has goal, distance to the goal spot, goal age, signal given, and the
   discipline value from skill. A helper `GoalKeeper` tracks how long the
   goal has been postponed. Past `give_up_ticks` (from skill) it produces a
   "goal broken" report.
7. **Signal: both ways.** A tactic can carry a trigger rule, for example
   "target inside spot area" or "a member is discovered". `check()` fires
   it. The caller can also call `signal(now)` by hand.

## Trace

Like the `Decider`: an optional sink gets every tactic score, the pick,
each slot assignment and every abort check, for debugging and tuning.

## Game side (not in the repo)

What a game decides itself and Sagax only enables: who forms a group
(shout range, radio net, travelling party), what role bits mean, which
faction knows which tactic and at what skill, radio delay and shout range,
which events count as "unexpected". Notes for a concrete game are kept
privately, outside the repository.

## Left to the builder

- Exact struct layout, C handle shape and JS wrapper, following
  `sx_machine_*` and the memory blocks.
- Parameter defaults for the shipped tactics and skill tables.
- Whether `Squad` owns the `Group` or borrows it.
- Example: a small square-map scene (4 members, 2 enemies) showing flank
  and ambush, plus a hex variant of one test.
