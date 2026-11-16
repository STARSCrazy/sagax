# Goal recognition (0.15.0, C API 15) — design

Status: written by the controller; the owner said "just start" and skipped
the questions. Every point the owner did not say is a call made here and
listed under "Calls made without the owner". Target version 0.15.0, C API
15. JavaScript is not part of this round.

## Intent

A guard who watches the player walk should guess **where the player is
heading** — which exit, which terminal, which objective — and act on it:
cut him off, send most hunters to the likely goal and one to the second
one. The owner's research (2026-11-12, `experiment_g.py`, class `Bayes`)
measured how: a classic Bayes recogniser that assumes the player is
"mostly sensible" (Boltzmann over the change of walking distance to each
goal, beta = 2 per cell) was right 84 % of the time in the second half of
the way, well calibrated (0.82), and beat a small trained network (0.67
calibration). It needs only the walking distance to each candidate goal,
which Sagax already has (flow fields), no training, a fixed number of
values. A variant that "forgets" (mixes 5 % uniform back in each step)
recovered faster after a feint.

### The one hard rule: no cheating

The recogniser knows only the steps the game feeds it. The game feeds only
what the observer saw (for instance positions from the observer's own
percepts, 0.13.0). The block itself has no access to the true goal or the
true position. Docs say so; the example feeds only seen steps.

## Goals

1. A per-observer memory `GoalGuess` over up to `MAX_GOALS` (8) candidate
   goals, holding a probability for each (integer, fixed point).
2. **Update from one observed step.** For each goal `k` the game gives the
   change of walking distance of the step actually taken (`moved_k = d_k(cur)
   - d_k(prev)`) and the changes of all moves that were possible from `prev`
   (`options_k[j]`). Likelihood `w(moved_k) / Σ_j w(options_k[j])` with
   `w(x) = exp(-beta * x / unit)`. Posterior ∝ prior × likelihood, then
   normalised. A step where `cur == prev` (standing still) changes nothing.
3. **Forgetting.** After each update, mix `forget` (0..256, share of uniform)
   back in. Default 13 (≈ 5 %), the measured value.
4. **Prior.** `set_prior(weights)` (for example from the mission: "he
   probably wants the building with the objective"). `reset()` returns to
   the prior. Goals with weight 0 stay at 0 until reset with other weights
   (they are ruled out) — but forgetting mixes into all goals with a non-zero
   prior only, so a ruled-out goal stays out.
5. **Read.** `chance(k)` as share 0..256 (Sagax fractions), `best()` →
   `(goal, chance)`, `rank(k)` (0 = most likely, ties by lower index).
6. **Decider columns.** `write_columns(k, row)` writes `chance, rank` as two
   `i32` — a consideration "intercept at goal k: chance rising" needs no new
   Decider code.
7. **Split hunters.** `split(hunters, out)`: share `n` units over the goals
   by chance (largest remainder, ties by lower index, deterministic), so
   "3 cops: 2 to the likely exit, 1 to the second" falls out.
8. **Flow-field helper.** Most games have one flow field per goal. A helper
   in the facade (`sagax`, which sees nav and decide) computes `moved_k` and
   `options_k` from the fields and the graph for a step `prev → cur` and
   calls the update: `observe_flow(&mut guess, graph, fields, prev, cur)`.
   `cur` must be `prev` or one of its moves; anything else (a jump after the
   observer lost sight) returns an error and changes nothing — the game then
   simply starts a new chain at the new position (the forgetting already
   handles the uncertainty). A goal unreachable from `prev` or `cur` gets
   likelihood of the least likely option, never 0 for the whole field, so
   one blocked goal cannot zero every other.
9. **Trace from the start** (one debug pattern for all blocks):
   `observe_traced` with records (step with per-goal likelihood and new
   chance, prior set, reset, jump refused in the helper).
10. **Rust and C** (`sx_goal_guess_*`), square and hex (the core is
    grid-free; the helper uses any nav graph, C uses `sx_flow` + `sx_map`),
    deterministic, integer, no heap after `new`.
11. **Example `goal_guess_scene`** (Rust + C, "C output equals Rust" in
    `examples/c/run.sh`, golden test): a small map with 4 exits; a runner
    walks to one; a watching guard prints the chances per step and the split
    of 3 hunters. Second run: a runner who feints towards one exit first and
    then turns; shown once with forgetting 0 and once with the default, to
    show the faster recovery. Short text log.

## Non-goals

- No neural network, no learning between games.
- No hidden-movement tracking (where is he now, unseen) — that is the world
  memory's job (`Knowledge`, last seen point).
- No automatic goal discovery: the game names the candidate goals.
- No changes to squad, crew or mission code. A game wires `split` or the
  columns into its own orders.
- No JavaScript binding.

## Anti-goals (failure even if it works)

1. Probabilities that drift out of their range or do not sum to 256
   (within rounding, see calls) — tested.
2. Any result that depends on float rounding or platform: integer only, the
   C example output must equal Rust byte for byte.
3. A single unreachable or zero-prior goal turning every chance into 0 or
   panicking.
4. Heap per update; goldens of existing examples changing.

## Shape (builder may refine names, keep the meaning)

```rust
// sagax-decide, module memory::goal_guess (world memory)
pub const MAX_GOALS: usize = 8;
pub struct GoalGuessConfig { pub beta: u32 /* 1/256 per unit, default 512 = 2 */,
                             pub unit: u32 /* cost of one straight step, e.g. 10 */,
                             pub forget: u32 /* 0..256, default 13 */ }
pub struct GoalGuess { /* chances in high precision, prior, count */ }
impl GoalGuess {
    pub fn new(config: GoalGuessConfig, goals: usize) -> Self;
    pub fn set_prior(&mut self, weights: &[u32]);
    pub fn reset(&mut self);
    pub fn observe(&mut self, moved: &[i32], options: &[i32], per_goal: usize);
    pub fn chance(&self, goal: usize) -> u32;   // 0..256
    pub fn best(&self) -> Option<(usize, u32)>;
    pub fn rank(&self, goal: usize) -> usize;
    pub fn write_columns(&self, goal: usize, row: &mut [i32]);
    pub fn split(&self, hunters: u32, out: &mut [u32]);
}
// sagax facade
pub fn observe_flow<G: Graph>(guess: &mut GoalGuess, graph: &mut G,
    fields: &mut [FlowField<G::Node>], prev: G::Node, cur: G::Node) -> Result<(), StepError>;
```

`options` is laid out goal-major: `options[k * per_goal + j]`, the same `per_goal`
moves for every goal (the moves from `prev` are the same; only their
distance changes differ). C: `sx_goal_guess_new/free/set_prior/reset/
observe/chance/best/rank/write_columns/split/set_trace` and
`sx_goal_guess_observe_flow(guess, flows, count, map, prev, cur)`, with
`_Static_assert` layouts for the config.

## Calls made without the owner

- Name "goal guess" (`GoalGuess`), lives in the world memory of
  `sagax-decide`; the flow-field helper lives in the facade (decide does
  not depend on nav, and should not start to).
- Defaults from the research: beta 2 per straight step, forgetting 5 %
  (13/256).
- Chances read as 0..256 like other Sagax fractions; internally a higher
  precision (builder's call, for example 1/65536 or log-domain fixed point).
- `w(x)` from an integer exponential (table or exp2 by shifts), not floats.
  The builder documents its precision.
- Standing still teaches nothing; jumps are refused by the helper.
- A goal unreachable from the step gets the least-likely option's weight.
- New example instead of changing an existing one.
- 0.15.0 / C API 15.

Builder calls (made while building, from the spec's open points):

- **Precision.** Chances are kept in 1/65536 and always sum to exactly
  65536 (every normalisation is a largest-remainder share, ties to the
  lower goal). Read as 0..256 (`chance`, `shares`) they are shared the
  same way, so they sum to exactly 256, not only within rounding.
  `chance_fine` (C `sx_goal_guess_chance_fine`) reads the internal value;
  `best`, `rank` and `split` compare in the internal precision.
- **The exponential.** `w(x) = exp(-t)`, `t = beta * a / (256 * unit)`
  with `a` the change minus the smallest change of that goal's options (so
  the best option has weight 1), computed as `2^(-t / ln 2)`: `t / ln 2`
  in 1/65536 (truncated, from `2^32 / ln 2`), the 16 fraction bits by 16
  fixed factors in 1/2^32, the whole part by a shift. The error is at most
  `w * 2^-15 + 2^-32` (the shift truncates to 1/2^32), so the relative
  error is near 2^-15 only for weights well above 2^-32; weights below
  2^-32 count as 0. Tested against `f64`. A step's likelihood is clamped
  to 1/65536..1.
- **Floor.** After the Bayes update a goal not ruled out that fell to 0 is
  lifted to 1/65536 (taken from the most likely goal). Without it a goal
  that had dropped below the precision could never come back when
  `forget` is 0 (a long feint did exactly that). So "Bayes alone" recovers
  too, only later than with forgetting.
- **Forgetting** mixes uniform over the goals with a non-zero prior (not
  the prior itself), as the spec says; `forget` above 256 counts as 256.
- **Prior.** `set_prior` replaces the prior and starts over from it
  (chances = prior). A missing weight is 0, extra ones are ignored, all 0
  (or none) is uniform; the start is uniform. Only weight 0 rules a goal
  out: a non-zero weight that would round to 0 keeps 1/65536 (taken from
  the largest).
- **`observe` returns `Result<(), StepError>`**: `per_goal` 0 or above
  `MAX_MOVES` (32) or a slice too short for the goals is
  `StepError::Shape`, nothing changes, a `Refused` record. `StepError`
  (`Shape`, `Jump`) lives in `sagax-decide` next to the trace, so the one
  trace type carries the helper's refusals too.
- **Goal count.** Rust `new` clamps to `MAX_GOALS` and allows 0 (then
  `best` is `None`); C `sx_goal_guess_new` refuses 0 and more than
  `SX_MAX_GOALS` (null), so `sx_goal_guess_best` always has an answer.
  `rank` of a goal out of range is the goal count, `chance` 0,
  `write_columns` writes `0, goals`.
- **`observe_flow` shape.** `observe_flow(guess, graph, fields, prev, cur,
  moves)`: `fields: &[F]` with `F: Borrow<FlowField<N>>` (a slice of
  fields or of references; the C layer passes references to the fields of
  separate `sx_flow` handles without copying), and `moves`, reused memory
  for the moves of `prev`, so the helper allocates nothing per step (the
  spec's "no heap"; the graph trait needs a `Vec`). The moves are
  `graph.neighbors(prev)`, standing still is not one of them. More than
  `MAX_MOVES` moves or a field count other than the goal count is
  `StepError::Shape`.
- **Unreachable goals in the helper** (replaces goal 8's "likelihood of
  the least likely option" for a goal unreachable from the step): a goal
  whose field does not hold `prev`, or holds none of the moves, learns
  nothing from the step (every change 0, a neutral likelihood of
  1 / moves). Only a single move outside the field of an otherwise
  reachable goal gets that goal's largest known change (the least likely
  option), also when it is the step taken.
- **Standing still** in the helper is `Ok` with no change and no record;
  the core `observe` has no notion of it (the game does not call it).
- **C `observe_flow`** takes `sx_flow *const *flows` (one per goal,
  `count` the goal count) and `sx_point` by value; the moves come from the
  map with the rules of `flows[0]`. Returns 0 (learned or standing still),
  1 for a jump (nothing changed, a `REFUSED` record), `SX_ERROR_ARGUMENT`
  for a null pointer, a wrong count or too many moves. The handle holds
  the `moves` buffer (capacity `SX_MAX_MOVES`); the FFI test counts zero
  allocations per step. `sx_goal_guess_config_default` fills the defaults.
- **Trace records**: `Step { goal, moved, likelihood (1/65536), chance
  (0..256) }` per goal after a step, `Prior { goal, chance }` per goal,
  `Reset`, `Refused { error }`; C `sx_goal_guess_trace` (24 bytes) with
  `SX_GOAL_GUESS_TRACE_*` and `SX_STEP_SHAPE` / `SX_STEP_JUMP`.
- **Tests** live in `memory/goal_guess_tests.rs` (not the long
  `memory/tests.rs`); the direct-walker test builds a 21 x 15 octile map
  with a wall and four exits and its own Dijkstra, the facade repeats it
  with flow fields on square and hex.
- **Example.** Square only (hex is covered by the facade and FFI tests): a
  tower guard, 21 x 15 yard, four exits, a roof where the runner is not
  seen (his reappearance is a jump, the chain goes on from there), three
  hunters split per tick; the feint is five steps towards exit 1, then
  exit 2. With the floor above the recovery difference is modest (t=8
  without forgetting, t=7 with it); the tests over many feints show
  forgetting never slower and a third faster overall.

## Done means

`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo fmt --check`, `sh js/run.sh` (unchanged), `CC=gcc sh
examples/c/run.sh` green; a test reproducing the research's direction (a
direct walker on a small map: the true goal is the best guess in the second
half of the way); tests for every goal and anti-goals 1–3; existing example
outputs byte-equal to 9fbb98b; USAGE gets a section; CHANGELOG 0.15.0;
ROADMAP entry; README block list.
