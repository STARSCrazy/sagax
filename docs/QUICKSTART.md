# Quickstart

Three jobs in five minutes: find a way, make a decision, and let a squad plan
and act together. Each job is shown in Rust, C and JavaScript, in that order.
Every snippet is copied from code that is built and run in this repository
(the file is named under each job). A few lines are left out (for example
the helper `point()` and the constants `WIDTH`, `HEIGHT` and `CELLS` of the C
map are defined in the named file).

Set-up first:

- Rust: add the `sagax` crate and `use sagax::prelude::*;`. The prelude holds
  the names these three jobs need (`DenseGrid`, `Navigator`, `Decider`,
  `Crew`, ...).
- C: `cargo build --release -p sagax-ffi`, then link `target/release/sagax.dll`
  (Windows) or `libsagax.so` / `libsagax.dylib`, and include
  `crates/sagax-ffi/include/sagax.h`. Check `sx_api_version() >= SX_API_VERSION`
  once at start.
- JavaScript: `cargo build --release -p sagax-wasm --target
  wasm32-unknown-unknown`, then `const lib = await loadSagax(bytes)` from
  `js/sagax.mjs` (bytes of the `.wasm` file in Node, a URL in the browser).

Three rules hold in all three languages (the full list is the
[Conventions](USAGE.md#conventions) of the guide): time is the game's own
integer count and `now` comes right after the handle; the seed comes last;
there is no default cost for a grid, so every grid asks for a preset
(`Octile`: straight 10, diagonal 14; `Tabletop`: every step 1).

## 1. A path

A 10 x 10 open map, from the corner to the other end of the first row. Straight
steps cost 10, so nine steps cost 90.

Rust (`crates/sagax/tests/prelude.rs`):

```rust
let mut map = DenseGrid::new(10, 10);
let mut nav = Navigator::new(Topology::Square(GridRules::preset(Preset::Octile)), 1000);
let report = nav.find_path(
    &mut map,
    Cell::new(0, 0, 0),
    &[Cell::new(9, 0, ANY_LEVEL)],
    Budget::UNLIMITED,
);
assert_eq!(report.status, NavStatus::Found);
assert_eq!(report.cost, 90);
```

C (`examples/c/squad.c`, the map; `examples/c/quickstart.c`, the search). The
game's map answers through callbacks; `sx_grid` is a ready map for games that
have none. A cost below 0 blocks a cell.

```c
sx_grid *grid = sx_grid_new(WIDTH, HEIGHT, 1);          /* width, height, levels */
sx_grid_set(grid, point(12, y), -1);                     /* block a cell */
sx_map map;
sx_grid_map(grid, &map);                                 /* the map for every search */

sx_rules rules;
sx_rules_default(SX_GRID_SQUARE, SX_PRESET_OCTILE, &rules);
rules.node_capacity = CELLS;                             /* cells one search may touch */
sx_nav *nav = sx_nav_new(&rules);

sx_point way[CELLS];
int32_t n = sx_nav_find_path(nav, &map, start, &goal, 1, 0, SX_OUT_CELLS, way, CELLS, NULL);
/* n: cells written (way[0] is the start), or a negative SX_ERROR_*;
 * budget 0: no limit; the last argument may be NULL if you do not need the
 * sx_result (status, cost, ...) */
sx_nav_free(nav);
sx_grid_free(grid);
```

JavaScript (`examples/js/quickstart.mjs`):

```js
const grid = lib.grid(10, 10, { preset: 'octile' });
const way = grid.findPath({ x: 0, y: 0 }, { x: 9, y: 0 });
console.log(way.status, way.cost);   // found 90
grid.free();
```

A way is `way.cells` (`{x, y, level}` each), starting with the start. Ask for
`output: 'turns'` (corners only) or `'smooth'` (any-angle lines) in the
options of `findPath`; the same way can be asked again in another form without
a new search (`grid.way('smooth')`, C `sx_nav_way`).

## 2. A decision

Two options, each scored by one number the game measured, here a distance
turned into a factor between 0 and 1 by a curve. The option with the higher
score wins.

Rust (`crates/sagax/tests/prelude.rs`):

```rust
let mut d = Decider::new();
let a = d.add_option(ONE);
let b = d.add_option(ONE);
assert!(d.add_consideration(a, Consideration::new(0, Curve::Linear, 0, 10)));
assert!(d.add_consideration(b, Consideration::new(0, Curve::Linear, 0, 10)));
let candidates = [Candidate::new(a, 0), Candidate::new(b, 0)];
let rows = [[2], [8]];
let choice = d.choose(&candidates, &rows, Pick::Best, 0).unwrap();
assert!(matches!(choice, Some(Choice { candidate: 1, .. })));
```

A *candidate* is an option for one target (here target 0); `rows` holds one
row of raw inputs per candidate. `choose` returns an error for rows of the
wrong length and `None` if every score is 0.

C (`examples/c/quickstart.c`):

```c
sx_decider *d = sx_decider_new();
sx_consideration near = {0, SX_CURVE_INVERSE, 1, 10};      /* input 0: distance */
sx_consideration healthy = {1, SX_CURVE_LINEAR, 0, 100};   /* input 1: health */
sx_consideration hurt = {1, SX_CURVE_INVERSE, 0, 100};
sx_candidate candidates[3] = {{0, 7}, {0, 8}, {1, 0}};
int32_t inputs[6] = {6, 80, 2, 80, 0, 80};
uint32_t scores[3];
sx_choice choice;
int32_t attack = sx_decider_add_option(d, SX_ONE);
int32_t flee = sx_decider_add_option(d, SX_ONE);

sx_decider_add_consideration(d, (uint32_t)attack, &near);
sx_decider_add_consideration(d, (uint32_t)attack, &healthy);
sx_decider_add_consideration(d, (uint32_t)flee, &hurt);
if (sx_decider_decide(d, candidates, 3, inputs, 2, SX_PICK_BEST, 0, 0,
                      scores, &choice) == 1)
	/* candidates[choice.candidate] won with choice.score */;
sx_decider_free(d);
```

Here option `attack` against target 7 or 8, or `flee`; `inputs` is one row of
two numbers per candidate. The example prints `decision: option 0, target 8,
score 53334`.

JavaScript (`examples/js/quickstart.mjs`):

```js
const d = lib.decider();
const a = d.addOption(1, [{ input: 0, curve: 'linear', min: 0, max: 10 }]);
const b = d.addOption(1, [{ input: 0, curve: 'linear', min: 0, max: 10 }]);
const choice = d.decide([
  { option: a, inputs: [2] },
  { option: b, inputs: [8] },
]);
console.log('candidate', choice.index);   // candidate 1
d.free();
```

## 3. A squad

A `Crew` runs a squad leader and its members' goal keepers for you. One
member sees an enemy; one `tick` makes a plan. The leader plans only with what
it has seen or been told.

Rust (`crates/sagax/tests/prelude.rs`):

```rust
let mut crew = Crew::new(CrewConfig {
    squad: SquadConfig {
        members: 1,
        memory: 4,
        areas: 2,
        inbox: 4,
        metric: Metric::Chebyshev,
        ..SquadConfig::default()
    },
    ..CrewConfig::default()
})
.unwrap();
let member = Member {
    id: 1,
    pos: [0, 0, 0],
    roles: 0,
    skill: 128,
    state: MemberState::Active,
};
assert!(crew.add_member(member).is_some());
crew.squad_mut().set_target_kind(1);
crew.squad_mut().add_tactic(tactics::rush());
crew.observe(
    0,
    Fact {
        subject: 11,
        kind: 1,
        pos: [10, 0, 0],
        seen_at: 0,
        ..Fact::default()
    },
);
let mut events = Vec::new();
crew.tick(0, &[], &[], 7, &mut events).unwrap();
assert!(events.iter().any(|e| matches!(e, CrewEvent::Planned(_))));
```

C (`examples/c/quickstart.c`):

```c
sx_crew_config config;
sx_crew_event events[8];
sx_member member = {0};
sx_fact enemy = {0};
uint32_t total;

sx_crew_config_default(&config);
config.squad.metric = SX_METRIC_CHEBYSHEV;
sx_crew *crew = sx_crew_new(&config);
member.id = 1;
member.skill = 128;
member.state = SX_MEMBER_ACTIVE;
sx_crew_add_member(crew, &member);
sx_squad_set_target_kind(sx_crew_squad(crew), 1);
sx_squad_add_rush(sx_crew_squad(crew));
enemy.subject = 11;
enemy.kind = 1;
enemy.pos[0] = 10;
sx_crew_observe(crew, 0, &enemy, NULL);
int32_t count = sx_crew_tick(crew, 0, NULL, 0, NULL, 0, 7, events, 8, &total);
/* count events written, total events there were; events[0].kind ==
 * SX_CREW_PLANNED, events[0].plan is the plan id */
sx_crew_free(crew);
```

JavaScript (`examples/js/quickstart.mjs`):

```js
const crew = lib.crew({ squad: { members: 1, memory: 4, areas: 2, inbox: 4, metric: 'chebyshev' } });
crew.addMember({ id: 1, pos: [0, 0, 0], skill: 128 });
crew.squad().setTargetKind(1);
crew.squad().addTactic(lib.tactics.rush());
crew.observe(0, { subject: 11, kind: 1, pos: [10, 0, 0], seenAt: 0 });
for (const event of crew.tick(0, { seed: 7 })) console.log('event', event.kind);   // event planned
crew.free();
```

A `tick` is called once per game tick and returns what happened, in the order
of its steps: `Planned`, `Signal` or `NoPlan` from the leader's check, then
`Activated` (the plan is now in force), `Order` (a member has a new goal),
`Start`, `Arrived` and `Broken`. The game turns the events into
actions: walk to the goal, start the attack. What a member does with its goal
is its own decision (section 2).

## 4. The whole walk

The three jobs together are one small program in each language: a squad of
four walks around a wall, gathers on three sides, and attacks at the same
tick. Each member has its own `Decider` (keep the goal, or idle), its own
`Navigator` way, and the crew handles plan, radio delay, arrival and signal.

- Rust: `cargo run -p sagax --example squad_walk`
  (`crates/sagax/examples/squad_walk.rs`)
- C: `sh examples/c/run.sh` (`examples/c/squad.c`)
- JavaScript: `js/run.sh` (`examples/js/squad.mjs`)

All three print the same lines, and `run.sh` checks that. The signal comes at
tick 51 and all four attackers start at tick 57:

```text
t=  0 planned plan 1
t=  5 activated plan 1
t=  5 order member 1
t=  5 start member 1
t=  9 order member 2
t=  9 order member 3
t=  9 order member 4
t=  9 start member 2
t=  9 start member 3
t=  9 start member 4
t= 33 arrived member 1
t= 41 arrived member 4
t= 45 arrived member 2
t= 45 arrived member 3
t= 51 signal
t= 51 order member 1
t= 55 order member 4
t= 57 order member 2
t= 57 order member 3
t= 57 start member 1
t= 57 start member 2
t= 57 start member 3
t= 57 start member 4
```

Members 2 to 4 get their orders by radio (2 ticks plus 1 per cell from the
leader), so they arrive at different ticks; the signal lines their attacks up.

Where next: [USAGE.md](USAGE.md) says which building block fits which case,
section 8 shows the crew step by step and the blocks underneath it.
