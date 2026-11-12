# Guide

What to use for which case, how to call it, and where the pitfalls are. The
exact contracts are in `crates/sagax-ffi/include/sagax.h` (C),
`js/sagax.mjs` (JavaScript) and the Rust docs (`cargo doc -p sagax-nav --open`).

## 1. Which building block?

| The game asks | Building block | C | JavaScript |
|---|---|---|---|
| One walker, one goal (or the nearest of several) | A* | `sx_nav_find_path` | `grid.findPath` |
| Get away: off the map, or away from threats | Exits as goals, or flee | `sx_nav_find_path` / `sx_nav_flee` | `grid.findPath` / `grid.flee` |
| Where can I get with X movement? | Reachable area | `sx_nav_reachable` + `sx_nav_path_to` | `grid.reachable` + `grid.pathTo` |
| A way without staircase steps | Smoothing | output `SX_OUT_SMOOTH` | `output: 'smooth'` |
| A group follows its leader, one cell each | Formation | `sx_nav_formation` | `grid.formation` |
| Many walkers, same goals | Flow field | `sx_flow_*` | `grid.buildFlow` + `grid.flowNext` |
| The map changes while a walker is on its way | D* Lite | `sx_dstar_*` | `grid.dstar*` |
| What should I do, and to whom? | Decisions (utility AI) | `sx_decider_*` | `lib.decider()` |
| Patrol, chase, search: behaviour over time | State machine | `sx_machine_*` | `lib.machine()` |
| Who did I see, where, how long ago; where is it dangerous | Memory of the world | `sx_sightings_*`, `sx_zones_*`, ... | `lib.sightings()`, `lib.zones()` |

Everything works on square and hex grids and in every interface.

**The same shape everywhere:**

- **Rules** (square or hex, costs, memory) are given once when the handle is
  made: C `sx_rules`, JS `lib.grid(w, h, rules)`, Rust
  `Navigator::new(topology, capacity)`.
- Every search answers with a **result**: status, cells of the whole way,
  cost, cells expanded, whether the node capacity was too small.
- **Status**: running (budget used up, continue later), found, partial (goal
  unreachable, way to the closest reachable cell), unreachable, done (area
  or field complete).
- **Ways always begin with the start** and end at the last cell, as every
  cell, as turning points or smoothed.
- **Budget 0 means no limit.**

**Rules of thumb:**

- **When in doubt, A\*.** On a 128 x 96 map a search takes well under a
  millisecond. After a map change, searching again is almost always fine.
- **Flow fields** pay off from roughly 5–10 walkers with the same goal.
- **D\* Lite** pays off only when the map changes *often* and *near the
  walker*. It needs its own memory per walker.
- **Formations** work on whole cells: every member gets its own cell.

**Hex grids:** C `rules.grid = SX_GRID_HEX_*`, JS `{ hex: HEX.oddR }`, Rust
`Topology::Hex`. The step callback is asked with `dx, dy` = offset to the
neighbour cell. Formation slots: "forward" is the walking direction,
"right" the direction 60° clockwise of it; `(0, 1)` front right, `(-1, 0)`
behind, `(-1, 1)` back right. Wedge: `Formation::wedge_hex` /
`FORMATION.wedgeHex`.

## 2. C

Build `cargo build --release -p sagax-ffi`; link `target/release/sagax.dll`
(Windows) or `libsagax.so` / `libsagax.dylib`. Header:
`crates/sagax-ffi/include/sagax.h`. A complete program:
`examples/c/quickstart.c`.

### Basics

```c
sx_rules rules = {0};
rules.grid = SX_GRID_SQUARE;            /* or SX_GRID_HEX_ODD_R ... */
rules.cost_straight = 10;  rules.cost_diagonal = 14;
rules.node_capacity = 32768;
sx_nav *nav = sx_nav_create(&rules);    /* once, reuse; 0: invalid rules */

sx_map map = {0};
map.step = my_step;              /* (ctx, x, y, level, dx, dy) -> level or -1 */
map.start_step = my_start_step;  /* optional: moves off the start cell */
map.move_cost = my_extra_cost;   /* optional: extra cost, -1 forbids */
map.context = &walker;

sx_point start = { x, y, z }, goal = { gx, gy, SX_ANY_LEVEL };
sx_point pts[64]; sx_result r;
int n = sx_nav_find_path(nav, &map, start, &goal, 1, 4000, SX_OUT_TURNS, pts, 64, &r);
/* n < 0: error. pts[0] is the start, pts[n-1] the goal or (r.status ==
 * SX_PARTIAL) the closest reachable cell. r.cells counts the whole way,
 * even if pts was too small. */
```

- Return values: entries written (≥ 0) or an error (< 0). How a search
  ended is only in `r.status`.
- Outputs: `SX_OUT_CELLS` (every cell), `SX_OUT_TURNS` (turning points),
  `SX_OUT_SMOOTH` (smoothed). With `start_step` the first cell after the
  start is always kept (the walker may not stand in the middle of its cell).
- `sx_nav_way` writes the same way again in another output, without a new
  search.
- One handle per thread. No allocation during a search while
  `node_capacity` suffices (`r.node_limit_hit` reports it).

### Over several frames

```c
n = sx_nav_find_path(nav, &map, start, &goal, 1, 500, SX_OUT_TURNS, pts, 64, &r);
/* next frame, while r.status == SX_RUNNING and the map is unchanged: */
n = sx_nav_continue(nav, &map, 500, pts, NULL, 64, &r);
```

Many small steps end exactly like one big one. On `SX_RUNNING`, `pts`
already holds the best way so far.

Several goals: `sx_point exits[] = {...}; sx_nav_find_path(nav, &map, start,
exits, n, ...)` gives the way to the cheapest one, in one search.

### Fleeing

```c
sx_point threats[] = { { ox, oy, oz } };
n = sx_nav_flee(nav, &map, start, threats, 1, move_left, SX_OUT_TURNS, pts, 64, &r);
/* SX_UNREACHABLE: nothing within move_left is farther from the threats */
```

Picks the cell, within the walker's budget, that the nearest threat needs
the longest walk to reach. If the map has exits or a refuge, fleeing *there*
is usually more sensible: pass them as goals to `sx_nav_find_path`. Give cells
near the threats an extra `move_cost` so the way does not run past them.

### Reachable area

```c
sx_point cells[2048]; int32_t costs[2048];
n = sx_nav_reachable(nav, &map, start, 120, 0, cells, costs, 2048, &r);
n = sx_nav_path_to(nav, &map, cells[7], SX_OUT_CELLS, pts, 64, &r);
/* r.status == SX_UNREACHABLE: that cell was not in the area */
```

### Batched callback (optional)

`map.steps` answers all neighbours of a cell in one call (`offsets` holds
`dx, dy` pairs, answers go to `levels`). It saves time only when a single
`step` answer is expensive in your game. Its answers must match `step`;
`step` stays required (smoothing and formations ask single moves).

### Smoothing

Output `SX_OUT_SMOOTH`. A straight line is used only where every cell it
touches can be entered with `step` on the same level and `move_cost` is at
most `rules.smooth_max_extra` (default 0). Stairs, corners and costly cells
stay corners. `step` is asked between cell centres: if your walker does not
move through centres, check its own clearance.

### Formation

```c
const int32_t slots[] = { 0, 1,  -1, 0,  -1, 1 };  /* (forward, right) per member */
enum { MAX = 512 };
sx_point table[4 * MAX];                            /* (members + 1) rows */
int cols = sx_nav_formation(nav, &map, slots, 3, table, MAX);
/* table[0*MAX + i]: the leader's cell, table[k*MAX + i]: member k's target
 * while the leader stands on cell i */
```

Works on the last way of `nav`. `slots = NULL` with 3 members is the 2 × 2
block. At narrow places members fall back onto the leader's trail and
return afterwards.

### Flow field

```c
sx_flow *flow = sx_flow_create(&rules);           /* its own handle */
sx_point goals[] = { { gx, gy, z } };
sx_flow_build(flow, &map, goals, 1, 100000, &r);  /* once per goal or change */
sx_point at[8], next[8]; int32_t cost[8];
sx_flow_next(flow, &map, at, 8, next, cost);      /* all walkers in one call */
```

The field is built *backwards* from the goals. Costs are exact when every
move costs the same both ways. With one-way moves or zones that cost extra
to leave they are approximate; the next steps still use only legal moves.

### D* Lite

```c
sx_dstar *d = sx_dstar_create(&rules);            /* one per walker */
sx_dstar_begin(d, start, goal);                   /* goal.level >= 0 */
n = sx_dstar_compute(d, &map, 0, SX_OUT_TURNS, pts, 64, &r);

sx_dstar_move_start(d, here);                     /* after every cell walked */
sx_point changed[] = { { door_x, door_y, door_z } };
sx_dstar_cells_changed(d, &map, changed, 1);      /* step already answers anew */
n = sx_dstar_compute(d, &map, 0, SX_OUT_TURNS, pts, 64, &r);
```

Needs: if a move from A to B is possible, so is the move from B to A
(costs may differ). `start_step` is not used. If the goal is unreachable,
`compute` works through everything reachable from the goal, so set a budget
and spread it over frames.

## 3. JavaScript

```js
import { loadSagax, HEX, FORMATION } from './sagax.mjs';
const lib = await loadSagax(wasmUrlOrBytes);     // URL in the browser, bytes in Node

const grid = lib.grid(30, 20);                    // square, diagonal step costs 1
const hexGrid = lib.grid(30, 20, { hex: HEX.oddR, step: 1 });
grid.block(5, 5); grid.addWall(3, 3, 1, 0);
grid.addZone(4, 4, 0); grid.setLeaveCost(4);      // leaving zone 0 costs 4
grid.setCosts(uint32Array);                       // whole map at once

const way = grid.findPath(a, b, { output: 'turns', budget: 0 });
// way.status: 'found' | 'partial' | 'running' (then grid.continue()) | 'unreachable'
const corners = grid.way('smooth');               // the same way, smoothed
const exit = grid.findPath(a, [e1, e2, e3]);      // to the nearest of several goals
const away = grid.flee(a, [enemy], 6);            // away from threats, within 6
const area = grid.reachable(a, 6).cells;          // [{x, y, cost}]
const back = grid.pathTo(9, 4);                   // way to one of them, or null
const { leader, members } = grid.formation(FORMATION.wedge(3));
grid.buildFlow([{ x: 3, y: 4 }]);
const steps = grid.flowNext(walkers);             // [{x, y, cost}] per walker
grid.dstarBegin(a, b); grid.dstarCompute();       // {status: 'found', cells, ...}
grid.free();
```

`js/foundry-adapter.mjs` builds a grid from a Foundry VTT scene (walls,
tokens, enemy zones of control). Flow fields and D* Lite do not suit zones
with a leave cost (one-way costs); use `findPath` / `reachable` there.

## 4. Rust

```rust
use sagax_nav::grid::{Cell, DenseGrid, GridRules};
use sagax_nav::nav::{ANY_LEVEL, Navigator, Output, Status};
use sagax_nav::topology::Topology;

let mut map = DenseGrid::new(64, 64);
let mut nav = Navigator::new(Topology::Square(GridRules::default()), 4096);
let r = nav.find_path(&mut map, Cell::new(0, 0, 0), &[Cell::new(40, 9, ANY_LEVEL)], usize::MAX);
assert_eq!(r.status, Some(Status::Found));
let corners = nav.output(&mut map, Output::Smooth);
```

`Navigator` bundles search (to one or several goals), `resume`, reachable
areas (`reachable`, `area`, `path_to`), `flee`, outputs and formations. Underneath: `Search` (A*/Dijkstra with a
budget), `flow::FlowField`, `dstar::DStarLite`, `smooth`, `formation` and
`topology::Topology` (square or hex at run time). Your own maps implement
`grid::CellMap` (square), `hex::HexMap` (hex) or `Graph` (anything); a
`&mut` map is a map too.

## 5. Ways as input for decisions

Game AI often weighs options by ways, not by straight-line distance:

- **Walking distance to a target:** `find_path` (budget 0) and `r.cost`.
  For many targets at once: one `reachable` from the walker, then the cost
  of each target cell from the area (no entry: out of reach).
- **Reachable this turn:** `reachable` with the movement left as `max_cost`;
  a cell in the area can be reached now, its cost is what it takes.
- **Attack positions:** the goals of `find_path` can be every cell next to
  the target; the way ends at the cheapest.
- **Escape:** exits as goals, or `flee`; `r.status` tells whether there is a
  way out at all.
- **Danger:** zones to avoid as extra `move_cost`; then the cost already
  counts the risk.
- **Movement budget with zones:** with a leave cost, `cost` mixes walking
  and penalty. JavaScript ways also give `steps` (walking and terrain only)
  and `zonesLeft` (zones left, e.g. attacks of opportunity provoked), so
  `cost = steps + zonesLeft * leaveCost`. Check `steps` against the movement
  left; Rust: `DenseGrid::zones_left(path)`. In C the penalty is part of your
  own `move_cost`, so you already know it. For "where can I get this turn"
  without zones, run `reachable` with leave cost 0; then check single targets
  with `findPath` and zones on.

## 6. Decisions (utility AI)

`sagax-decide` chooses what a character does. You list **options** (attack,
flee, heal ...). Each has a **weight** and **considerations**: one raw number
the game measured (distance, own health, enemies near ...) turned into a
factor 0..1 by a **curve**. A **candidate** is one pair of option and target;
the game fills one row of raw inputs per candidate. The score is the product
of the factors (one 0 rules the candidate out), compensated for the number of
factors so options with many considerations are not punished, times the
weight.

- **Curves:** `t = (input - min) / (max - min)`, clamped to 0..1. `linear` t,
  `inverse` 1 - t, `quadratic` t², `inverseQuadratic` (1 - t)², `cubic` t³
  (only the top counts), `root` √t (early values already count), `sCurve`
  3t² - 2t³ (a soft threshold), `inverseSCurve`, `step` 1 if input >= min,
  `inverseStep` 1 if input < min.
- **Choice:** `best` (equal scores: the smaller index wins), `weighted`
  (by chance in proportion to the score) or `uniform` (each equally likely),
  both among the `top` best (0: all).
- **Deterministic:** all integer, 1.0 = 65535. No global random numbers; the
  seed comes from you (e.g. character index + turn). Same inputs and seed,
  same choice, in Rust, C and JavaScript.
- **Inputs from ways:** see section 5 (walking cost, reachable this turn,
  flee). The target number is yours (token, cell index, weapon); Sagax only
  carries it along.

JavaScript:

```js
const d = lib.decider();
const attack = d.addOption(1, [
  { input: 0, curve: 'inverse', min: 1, max: 10 },   // distance: near is better
  { input: 1, curve: 'linear', min: 0, max: 100 },   // own health
]);
const flee = d.addOption(1, [{ input: 1, curve: 'inverseQuadratic', min: 0, max: 100 }]);
const choice = d.decide([
  { option: attack, target: goblin, inputs: [3, hp] },
  { option: attack, target: orc, inputs: [6, hp] },
  { option: flee, target: 0, inputs: [0, hp] },
], { pick: 'weighted', top: 2, seed: actorIndex * 1000 + round });
// choice: {index, option, target, score, scores} or null if every score is 0
d.setTrace(true); /* ... */ d.takeTrace();   // [{kind: 'factor'|'scored'|'chosen', ...}]
d.free();
```

C:

```c
sx_decider *d = sx_decider_new();
int32_t attack = sx_decider_add_option(d, SX_ONE);
sx_consideration near = {0, SX_CURVE_INVERSE, 1, 10};
sx_decider_add_consideration(d, attack, &near);
sx_candidate cands[2] = {{attack, 7}, {attack, 8}};
int32_t inputs[2] = {6, 2};                 /* one row of stride 1 per candidate */
sx_choice choice;
if (sx_decider_decide(d, cands, 2, inputs, 1, SX_PICK_BEST, 0, seed, NULL, &choice) == 1)
	/* cands[choice.candidate] */;
sx_decider_set_trace(d, on_event, ctx);     /* SX_DECIDE_FACTOR / SCORED / CHOSEN */
sx_decider_free(d);
```

Rust: `sagax_decide::Decider` with `add_option`, `add_consideration`,
`decide` / `decide_traced` (any `sagax_core::Sink<DecideEvent>`).

### Putting factors together

Multiplying is the default, and it has a known weakness: many good but not
perfect factors drag a score down. Five factors of 0.9 give 0.59 plainly,
0.78 with the compensation; a single factor of 0.8 then wins. So the way
factors are put together can be chosen, for the whole decider
(`setCombine('geometricMean')`) and per option (`addOption(w, list,
{ combine: 'minimum' })`, which wins over the decider's).

What each one does with typical situations
(`cargo run -p sagax-decide --example compare`):

| Situation | product | geometricMean | minimum | average | median |
|---|---|---|---|---|---|
| five times 0.9 | 0.784 | 0.900 | 0.900 | 0.900 | 0.900 |
| one 0.8 | 0.800 | 0.800 | 0.800 | 0.800 | 0.800 |
| 1, 1, 1, 0.1 | 0.167 | 0.562 | 0.100 | 0.775 | 1.000 |
| 1, 1, 0 (no line of sight) | 0 | 0 | 0 | 0.667 | 1.000 |
| 0.9, 0.9, 0.9, 0.05 | 0.063 | 0.437 | 0.050 | 0.687 | 0.900 |

Which to take:

- **geometricMean as the everyday choice.** The number of factors no longer
  matters (three times 0.95 stays 0.95), a 0 still rules out, a weak factor
  still hurts clearly. Good for most actions.
- **product** when every extra doubt should cost: risky actions (a charge,
  a spell with many conditions), where several "almost fine" mean "rather
  not".
- **minimum** for "everything has to fit": the weakest point decides, e.g.
  a position that must be covered *and* reachable *and* in range.
- **average** for preferences that make up for each other: choosing a cell
  to stand on, a weapon by taste. A 0 does **not** rule out.
- **median** against noisy inputs: a single outlier is ignored. A 0 does not
  rule out either, so never use it alone for hard rules.

Two tools work with every combine:

- **`apply: 'multiply'`** puts a factor outside the combine: it is
  multiplied onto the result afterwards, a true factor. Use it for hard
  rules ("no ammunition", "target not visible": a step to 0) and for one
  thing that must always count fully, even if the option averages the rest.
- **`pull`** sets how much a factor counts. The factor v becomes
  1 - pull · (1 - v). For a factor of 0.4: pull 0 → 1 (off), 0.25 → 0.85,
  0.5 → 0.7 (gentle, never below 0.5), 1 → 0.4 (as it is), 1.5 → 0.1,
  2 → 0 (strict). Gentle for side matters ("prefers cover a bit"), strict
  for what should bite early.

Two more for steady behaviour:

- **Rank** (`{ rank: 2 }` or `setRank`): only the highest rank that still
  scores above 0 is chosen from. 2 emergency (flee, heal), 1 combat, 0
  idle. Easier to tune than ever larger weights.
- **Current action** (`setCurrent({ option, target }, 0.1)`): the action
  going on now scores 10 % more, so a character does not flip between two
  nearly equal actions every turn.

A recipe for a melee fighter:

```js
const d = lib.decider();
d.setCombine('geometricMean');
const attack = d.addOption(1, [
  { input: 0, curve: 'inverse', min: 1, max: 8 },            // walking cost to the target
  { input: 1, curve: 'root', min: 0, max: 100 },             // own health: only low health hurts much
  { input: 2, curve: 'linear', min: 0, max: 100, pull: 0.3 },// target already hurt: a gentle bonus
  { input: 3, curve: 'step', min: 1, apply: 'multiply' },    // reachable this turn: a hard rule
]);
const flee = d.addOption(1, [
  { input: 1, curve: 'inverseSCurve', min: 15, max: 40 },    // health below about 30 %: switches softly
], { rank: 1 });
```

C: `sx_decider_add_weighted` (pull, `SX_APPLY_*`), `sx_decider_set_combine`
(`SX_COMBINE_*`, option -1 for all), `sx_decider_set_rank`,
`sx_decider_set_current`. Rust: `add_weighted`, `set_combine`,
`set_option_combine`, `set_rank`, `set_current`.

### Memory: cooldowns and "not again so soon"

The opposite of the bonus for the current action: after doing something, a
character often may not, or does not want to, do it again for a while. The
decider remembers what the game says it did (`record`, the last 256
actions). Time is the game's own count: frames, turns, hours; set it with
`setNow` before deciding.

- **Hard cooldown** (a rule: "once every 3 turns"): `setCooldown(option, 3)`.
  The option scores 0 until the time has passed.
- **Soft dislike** (allowed, but not wanted): a consideration that reads
  `MEMORY.sinceOption` (or `sincePair` for this target) instead of a row
  value. With `curve: 'linear', min: 0, max: 10` the score is low right
  after and full again after 10. Never done counts as very long ago.
- **Penalty for repeating:** `MEMORY.countOption` / `countPair` counts how
  often within the window (`setWindow(time)`); e.g. `curve: 'inverse',
  min: 0, max: 3` for "the third time in a row is unattractive".

```js
d.setNow(turn);
const taunt = d.addOption(1, [{ input: MEMORY.sincePair, curve: 'root', min: 0, max: 5 }]);
d.setCooldown(heal, 3);
const c = d.decide(candidates);
if (c) { act(c); d.record(c, turn); }
```

C: `sx_decider_set_now`, `sx_decider_record`, `sx_decider_set_cooldown`,
`sx_decider_set_window`, `sx_decider_forget`, inputs `SX_INPUT_SINCE_OPTION`
and friends.

### State machine

For behaviour that runs over time: patrol, chase, search, give up. States
and conditions are named (JavaScript) or numbered (C, Rust); the game says
every tick which conditions hold. Transitions are checked in the order
added, the first that matches wins; then the state's timeout. So put aborts
("dead", "under fire") first, from any state. At most one change per tick.

```js
const m = lib.machine(['patrol', 'chase', 'search', 'dead'], ['seen', 'killed']);
m.transition({ from: '*', to: 'dead', when: ['killed'] });          // abort, from any state
m.transition({ from: 'patrol', to: 'chase', when: ['seen'] });
m.transition({ from: 'chase', to: 'search', unless: ['seen'], after: 10 }); // chase at least 10
m.transition({ from: 'search', to: 'chase', when: ['seen'] });
m.timeout('search', 300, 'patrol');                                  // give up after 300
const change = m.update(frame, { seen: canSee, killed: hp <= 0 });   // {from, to, reason} or null
```

`after` keeps a state for a while (no flicker); `timeout` ends it. Inside a
state the decider can choose the details (which target, which weapon).
Time may wrap around. C: `sx_machine_*` with condition bits in a
`uint64_t`; Rust: `sagax_decide::state::StateMachine`.

## 7. Memory of the world

Characters should remember things for a while: what they saw and heard,
where and when, what a colleague told them, which places are dangerous or
lit, how alert they are. The blocks of `sagax_decide::memory` do that, each
usable alone, and with the state machine and the decider they make up a
whole behaviour for turn-based tactics, real-time strategy, stealth or
action games. **The game decides what is perceived** (no sight lines or
hearing in here): it says "seen", "heard", and gives positions. The blocks
keep it, pass it on and let it fade. Positions are integers `[x, y, z]` (on
a hex grid use axial `(q, r)` in `x`, `y`), time is the game's own `u32`
count (frames or rounds; ages use wrapping subtraction), no random numbers,
and sizes are fixed at creation. **Every time and limit is a parameter:** try
a short memory against a long one, a slow message against a fast one, and
keep what plays best.

The snippets assume `use sagax_decide::memory::*;` and, in Rust, a function
that may use `?` (a capacity of 0 is an error). Kinds, ids and areas of the
game (`SEEN`, `NOISE`, `LIT`, ...) are your own numbers.

- **Knowledge** (what a character knows): a table of facts keyed by
  `(subject, kind)`. A `Fact` has `pos`, `seen_at` (when it was observed, not
  when it arrived), `source` (`SOURCE_OWN`, `SOURCE_HEARD`, `SOURCE_TOLD`,
  more of your own are fine), and free `strength` and `data`. When full,
  `Evict::Oldest` or `Evict::Weakest` makes room. `learn` always
  overwrites; `merge` only if the new fact is not older.

  ```rust
  let mut known = Knowledge::new(8, Evict::Oldest)?;
  let fact = Fact { subject: 7, kind: SEEN, pos: [4000, 4000, 0], seen_at: now,
                    strength: 100, data: 0, source: SOURCE_OWN };
  let dropped = known.learn(fact, now);                  // Option<Fact>: the evicted one
  known.merge(heard_about_7, now);                       // keeps a newer own fact
  known.touch(7, SEEN, now);                             // seen again, same place
  known.set_pos(7, SEEN, [4100, 4000, 0]);               // false if unknown
  known.expire(now, 2040, |f| forget(f));                // age >= 2040 goes
  if let Some(f) = known.get(7, SEEN) { search_near(f.pos, now.wrapping_sub(f.seen_at)); }
  ```

  Also `remove`, `remove_subject`, `set_data`, `set_strength`, `expire_kind`,
  `expire_with` (an age per fact), `iter`, `len`, `clear`,
  `learn_message` / `merge_message` (turn a message into a fact). C
  `sx_knowledge_*` (handle, `sx_fact`, `SX_SOURCE_*`, `SX_EVICT_*`, `at` for
  walking the table, `sx_knowledge_expire_kind` for one age per kind), JS
  `lib.knowledge(8, 'oldest')` with `list()`, `expire(now, age, perKind)`.
- **Messages** (who tells whom, and when it arrives): three parts.
  The `Outbox` of a sender holds messages that become due after a delay,
  with a cooldown between posts. `route` picks the recipients among
  candidates you give (filter team and free state yourself). The `Inbox` of
  a receiver holds messages with their arrival time; `arrival` computes
  `now + base + distance * per_unit`, so a shout can take one round per cell.

  ```rust
  let mut out = Outbox::new(4)?;
  let msg = Message { kind: SEEN, subject: 7, pos: [4000, 4000, 0], seen_at: now,
                      strength: 0, data: 0, sender: me, keep_if_sender_gone: 0 };
  match out.post(msg, now, 102, 170) {                   // delay, cooldown
      Ok(()) => {}
      Err(Refused::Cooldown) | Err(Refused::Full) => {}
  }
  let mut due = Vec::new();
  out.due(now, &mut due);                                // delay over, in post order
  // `free`: &[(id, x, y)]; the ids go to `ids`
  let mut ids = Vec::new();
  route(Route::Nearest(3), (x, y), &free, Metric::Euclid, 16, &mut ids);
  let mut inbox = Inbox::new(4)?;                        // one per receiver
  inbox.push(msg, arrival(now, 0, dist, 2));             // false if refused
  let mut got = Vec::new();
  inbox.take_due(now, &mut got);                         // arrived, in push order
  for m in &got { known.merge_message(m, SOURCE_TOLD, now); }
  ```

  Rules of `route`: `Route::Nearest(k)` (nearest first, a tie keeps the
  earlier candidate), `InRange(r)` (all with distance `<= r`, candidate
  order; with `Metric::Euclid` the square is compared with `r * r`), `All`,
  `To(id)`. `Metric` is `Euclid`, `Manhattan`, `Chebyshev` or `Hex` (axial);
  each axis is divided by the divisor first (truncating; the hex `/ 2` then
  truncates an odd sum). A message is a
  plain value: after `due` you may refresh `pos` and `seen_at` before
  routing. `sender_gone` drops the messages of a sender that died, except
  those with `keep_if_sender_gone` (a noise stays, a radio call is lost).
  `due_max` and `take_due_max` take at most some. An `Inbox` compares
  arrival times by their wrapping difference, so all stored arrivals must
  lie within a window narrower than 2^31 ticks.

  C `sx_outbox_*` (`sx_outbox_post` returns 0, `SX_REFUSED_FULL` or
  `SX_REFUSED_COOLDOWN`), `sx_inbox_*`, `sx_arrival`, `sx_route` (with
  `SX_ROUTE_*`, `SX_METRIC_*`, `sx_nearby`), `sx_message`. JS `lib.outbox(4)`
  (`post` gives `'ok'`, `'full'` or `'cooldown'`), `lib.inbox(4)`,
  `lib.arrival`, `lib.route({nearest: 3}, [x, y], candidates, 'euclid', 16)`
  with candidates `[{id, x, y}]`.
- **Areas** (places to stay out of or pay extra for): a circle with a kind,
  centre, radius, `lifetime`, `strength` (cost per cell) and `blocking`.
  Adding an area of the same kind whose centre lies inside an existing one
  renews that one; when full the oldest goes.

  ```rust
  let mut areas = Areas::new(8)?;
  let lit = Area { kind: LIT, pos: [x, y, 0], radius: 600, strength: 20,
                   lifetime: 3000, blocking: 0, ..Area::default() };
  let index = areas.add(lit, now);                       // since = now
  areas.expire(now);                                     // age >= lifetime goes
  let inside = areas.contains(pos);                      // Option<usize>
  let blocked = areas.blocks(cell, start, goal);         // for move_cost, see below
  let extra = areas.cost(cell, start, goal);             // u32, for move_cost
  ```

  Start and goal are exempt in both `blocks` and `cost`, so a way can always
  leave an area and reach a goal inside one. Also `contains_kind`, `iter`,
  `len`, `clear`. C `sx_areas_*` with `sx_area`, JS `lib.areas(8)`.
- **Progress** (does the chase still get closer): a lower value is better,
  usually a distance. "Lost" is not here: it is the age of the fact in
  `Knowledge`.

  ```rust
  let mut p = Progress::default();
  p.start(distance, now);
  p.update(distance, now, 0x80);                         // true if it improved by 0x80
  if p.stalled(now, 85) { give_up(); }                   // no gain for 85 ticks
  ```

  C `sx_progress` plus `sx_progress_*`, JS `new Progress()`.
- **Timers** (ids that are "on" for a while: ignore a target, no alarm
  again): `set` renews, when full the entry with the least time left is
  replaced.

  ```rust
  let mut skip = Timers::new(4)?;
  skip.set(7, now, 170);
  if !skip.active(7, now) { chase(7); }
  let left = skip.left(7, now);                          // ticks still to go
  ```

  Also `remove`, `len`, `clear`. C `sx_timers_*`, JS `lib.timers(4)`.
- **Interval** ("again every N ticks"; never marked counts as due):

  ```rust
  let mut report = Interval::default();
  if report.due(now, 255) { report.mark(now); }
  ```

  C `sx_interval` plus `sx_interval_*`, JS `new Interval()`.
- **Alertness** (a value with levels): stimuli raise it, time lowers it. The
  level jumps straight to the highest threshold reached and falls when the
  value is below its threshold minus `hysteresis`. Decay only counts ticks
  after `hold` ticks since the last stimulus. The levels mean what you say
  (unaware, suspicious, searching, alert) and are good conditions for a
  state machine.

  ```rust
  let cfg = AlertConfig { max: 100, decay_per_tick: 2, hold: 5, hysteresis: 5,
                          count: 3, thresholds: [20, 50, 80, 0, 0, 0, 0, 0] };
  let mut alert = Alertness::default();
  alert.stimulus(30, now, &cfg);                         // a noise
  if let Some((from, to)) = alert.update(now, &cfg) { on_level(from, to); }
  let level = alert.level();                             // 0 ..= cfg.count
  ```

  C `sx_alertness` and `sx_alert_config` plus `sx_alertness_*` (`update`
  returns 1 and fills the two levels), JS `new Alertness()` with a config
  `{max, decayPerTick, hold, hysteresis, thresholds}`.
- **Points around a place** (agents spread out without random numbers) and
  **sweep** (turning the view while looking around). `PointPattern::default()`
  holds all numbers; change any of them. `Sine::Fine` is accurate;
  `Sine::Coarse` is the small table that reproduces older numbers exactly.
  The default box is not clamped (`min` and `max` span all of `i32`): set
  `min` and `max` for a bounded map. The ring is Euclidean, so on an axial
  hex grid it is skewed: convert to the game's world coordinates first (and
  back), or snap the result to the nearest hex.

  ```rust
  let pattern = PointPattern::default();
  let first = points_around(me, 0, last_pos, &pattern).unwrap();   // step 0, then 1..=3
  let heading = sweep_heading(base, elapsed, 51, &LOOK_OFFSETS);   // 0..=255
  let s = sine(heading, Sine::Fine);                               // 65536 = 1.0
  ```

  C `sx_points_around` (a `sx_point_pattern` or null), `sx_sweep_heading`,
  `sx_sine`; JS `lib.pointsAround`, `lib.sweepHeading`, `lib.sine`.

### Recipe: radio

One call, delayed and with a cooldown, to the nearest free colleagues. The
sender posts a message to its `Outbox` with delay 102 and cooldown 170; when
`due` hands it out, refresh `pos` and `seen_at` from your current fact, then
`route(Route::Nearest(3), ...)` over the colleagues who are free at that
moment, and `merge_message` with `SOURCE_TOLD` at each receiver. If the
sender dies before the delay is over, `sender_gone` drops the call.

### Recipe: shout

Everyone within reach hears at once: `Route::InRange(r)` with `Metric::Hex`
or `Euclid`, and `arrival(now, 0, 0, 0)` (base 0, per unit 0), or push
straight into the receivers' `Inbox`es. A `Timers` entry keeps the shouter
quiet for a while.

### Recipe: messenger

A message that travels: the same as a shout, but `arrival(now, base,
distance, per_unit)` with `per_unit > 0`, so far receivers hear later. Use
`Metric::Hex` and a distance in cells on a hex grid. A receiver with a full
`Inbox` drops the message that would arrive last.

### Recipe: noise

A noise is a message nobody sends on purpose: build a `Message` with
`keep_if_sender_gone: 1`, `route(Route::InRange(loudness), ...)` over all
guards whatever they do, `Inbox::push` it with `arrival` (base 0 and
`per_unit` 0 for "heard at once"), and let each receiver
`learn_message(&m, SOURCE_HEARD, now)` and raise its `Alertness`. A louder
noise reaches further because `r` is the loudness. The stealth guard example
does this in round 2.

### Recipe: the officer

A witness chases, calls for help, loses sight and searches; the three
nearest free colleagues come; the table forgets. Walk, look, chase and idle
are states of a `StateMachine`; the loop each tick: `Outbox::due` and
`route` for the radio; report `Interval`; sight (every 4th tick) into
`Knowledge` and `Progress`; "lost" is the age of the fact, `stalled` or lost
leads to a `Timers` entry (ignore) and a search; `expire` once. The state
machine flags (`lost`, `stalled`, `arrived`, `give up`) are the conditions
you pass in. A complete, runnable version with the numbers for 17 ticks per
second: `cargo run -p sagax-decide --example officer`
(`crates/sagax-decide/examples/officer.rs`); in C `examples/c/memory.c`
(`sh examples/c/run.sh`).

### Recipe: stealth guard

Alertness levels drive the state machine on a hex grid: a noise is a
`SOURCE_HEARD` fact and raises `Alertness` to "suspicious", the guard goes to
look, sees the player (a `SOURCE_OWN` fact, level "alert"), shouts to the
guards in range (`Route::InRange` with `Metric::Hex`, `arrival` with one round
per cell), loses the player (the age of the fact), searches the points
around the last place (`Sine::Fine`) and calms down as the value decays.
Lit cells cost extra on the way (`Areas::cost`). Run it with
`cargo run -p sagax-decide --example stealth_guard`
(`crates/sagax-decide/examples/stealth_guard.rs`, with the derivation of
every round number in the test).

### Recipe: area blocking and cost in `move_cost`

`Areas::blocks` and `Areas::cost` fit the map's `move_cost` (Rust
`CellMap::move_cost`, C `sx_move_cost_fn`): turn the cell into its centre
position, forbid the move, or add the cost. Start and goal are exempt, so a
way can always leave an area and reach a goal inside one.

```rust
// `self` is your map: it holds the areas and the start and goal positions.
fn move_cost(&mut self, _from: Cell, to: Cell) -> Option<Cost> {
    let centre = [to.x * 256 + 128, to.y * 256 + 128, 0];     // your cell size
    if self.areas.blocks(centre, self.start, self.goal) {
        None                                                  // forbidden
    } else {
        Some(self.areas.cost(centre, self.start, self.goal))  // extra cost, 0 outside
    }
}
```

```c
static int32_t move_cost(void *ctx, int32_t x, int32_t y, int32_t lv,
                         int32_t tx, int32_t ty, int32_t tl) {
	struct game *g = ctx;
	int32_t cell[3] = { tx * 256 + 128, ty * 256 + 128, 0 };
	uint32_t extra = 0;
	if (sx_areas_blocks(g->areas, cell, g->start, g->goal) == 1) return -1;
	sx_areas_cost(g->areas, cell, g->start, g->goal, &extra);
	return (int32_t)extra;
}
```

The same works on hex grids, since the game converts the cell to a position.
In JavaScript, call `a.blocks(cell, start, goal)` and `a.cost(cell, start,
goal)` while building the grid's walls and costs before the search.

## 8. Debugging and logging

Tracing hands every step of a search to you: `started`, `expanded` (cost now
final), `improved` (reached more cheaply, with the parent cell), `paused`
(budget used up) and `finished` (with `found`). It works for paths, areas,
flow fields and D* Lite. Off (the default) it costs nothing.

```c
static void on_step(void *ctx, const sx_event *e) {
	if (e->kind == SX_TRACE_EXPANDED) mark_cell(ctx, e->cell.x, e->cell.y, e->cost);
}
sx_nav_set_trace(nav, on_step, my_debug_view);   /* NULL turns it off */
```

```js
grid.setTrace(true);
grid.findPath(a, b);
const { events } = grid.takeTrace();   // [{kind, x, y, parent, cost, estimate, expanded, found}]
```

Rust: `nav.set_tracer(Some(Box::new(|e: &TraceEvent| ...)))`; lower down,
every `Search::advance` takes a `sagax_core::Sink` of `SearchEvent`s, and
`sagax-debug` keeps the last N of them.

Typical uses: colour the expanded cells to see why a search is slow, log
the `finished` event with `expanded` to find costly searches, and follow
`improved` to see why a way goes where it goes.

## 9. Pitfalls

- Costs are integers ≥ 0. `move_cost` is an *extra* on top of the step cost.
- `SX_ANY_LEVEL` (-1, Rust `ANY_LEVEL`) as goal level means any level; D*
  Lite needs an exact level.
- `SX_PARTIAL` is the best partial way, not an error.
- Lists are cut when the buffer is too small; `r.cells` still counts all.
- Flow fields and D* Lite assume moves that work both ways.
- Load the library at run time, check `sx_api_version()`, and fall back to
  your own code if it is missing or too old.
