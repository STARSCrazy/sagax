// A squad walks around a wall and attacks from three sides at once.
// Run: node examples/js/squad.mjs  (after js/run.sh built the .wasm)
//
// A 24 x 12 grid with a wall at x = 12 (rows 0 to 7, open below). The
// leader sees an enemy behind the wall; the crew plans a flank, sends the
// orders by radio and gives the signal once all stand at their side. Each
// member has its own decider: keep the goal, or idle. A member that chose
// its goal walks its grid way, one cell per STEP ticks from its start tick.
//
// The same scene is in crates/sagax/examples/squad_walk.rs and
// examples/c/squad.c; all three print the same lines.

import { readFile } from 'node:fs/promises';
import { ONE, loadSagax } from '../../js/sagax.mjs';

const wasm = await readFile(new URL('../../target/wasm32-unknown-unknown/release/sagax_wasm.wasm', import.meta.url));
const lib = await loadSagax(wasm);

const END = 120; // ticks 0 ..= END
const ENEMY = 1; // fact kind of an enemy
const STEP = 2; // ticks per cell of the walk
const SEED = 7;
const WIDTH = 24;
const HEIGHT = 12;
const MEMBERS = [[1, 2, 5], [2, 2, 7], [3, 2, 3], [4, 0, 5]]; // id, x, y; member 1 leads
const SKILL = 192;
const SPOTS = [
  { pos: [16, 5, 0], tags: ['cover'] },
  { pos: [16, 7, 0], tags: [] },
  { pos: [20, 9, 0], tags: ['high'] },
  { pos: [20, 1, 0], tags: [] },
];

// The map: open cells and the wall at x = 12, rows 0 to 7. A JS grid
// carries its own navigator, so each member gets one.
function sceneMap() {
  const map = lib.grid(WIDTH, HEIGHT, { preset: 'octile' });
  for (let y = 0; y <= 7; y++) map.block({ x: 12, y, level: 0 });
  return map;
}

const crew = lib.crew({ squad: { members: 4, memory: 8, areas: 4, inbox: 8, dangers: 4, metric: 'chebyshev' } });
for (const [id, x, y] of MEMBERS) crew.addMember({ id, pos: { x, y, level: 0 }, skill: SKILL });
const sq = crew.squad();
// Orders reach the leader at once, the others by radio: 2 ticks plus 1 per cell.
for (const [id] of MEMBERS) if (id !== 1) sq.setRadio(id, 2, 1);
sq.setLeader(1);
sq.setSkill(SKILL);
sq.setTargetKind(ENEMY);
sq.addTactic(lib.tactics.rush());
sq.addTactic(lib.tactics.formation('line', 2));
sq.addTactic(lib.tactics.flank(3, 4));
crew.observe(0, { subject: 10, kind: ENEMY, pos: { x: 20, y: 5, level: 0 }, seenAt: 0 });

// One member's own side: its decider, its way and where it is on it.
const walkers = MEMBERS.map(([id]) => {
  const decider = lib.decider();
  const goalOption = crew.keeper(id).addGoalOption(decider, 1);
  // The idle weight is ONE / 4 rounded down, as in Rust and C (0.25 would round up).
  const idleOption = decider.addOption(Math.floor(ONE / 4) / ONE);
  // way: the cells of the way; next: the next one to take; from: the walk
  // starts after this tick; onGoal: the member chose its goal last time.
  return { id, decider, goalOption, idleOption, nav: sceneMap(), way: [], next: 0, from: 0, onGoal: false };
});

const posOf = (id) => crew.squad().members().find((m) => m.id === id).pos;
const lines = [];
const pad = (t) => String(t).padStart(3, ' ');

for (let t = 0; t <= END; t++) {
  // 1. Walk: one cell per STEP ticks after the start, for a member on its goal.
  for (const w of walkers) {
    const d = t - w.from;
    if (w.onGoal && d > 0 && d % STEP === 0 && w.next < w.way.length) {
      const cell = w.way[w.next++];
      crew.setPos(w.id, cell);
    }
  }
  // 2. The group.
  for (const e of crew.tick(t, { spots: SPOTS, seed: SEED })) {
    if (e.kind === 'order') {
      const w = walkers.find((w) => w.id === e.member);
      w.way = w.nav.findPath(posOf(e.member), e.goal.pos).cells;
      w.next = 1;
      w.from = e.startAt ?? t;
    }
    switch (e.kind) {
      case 'planned':
      case 'activated':
        lines.push(`t=${pad(t)} ${e.kind} plan ${e.plan}`);
        break;
      case 'order':
      case 'start':
      case 'arrived':
      case 'broken':
        lines.push(`t=${pad(t)} ${e.kind} member ${e.member}`);
        break;
      default:
        lines.push(`t=${pad(t)} ${e.kind.toLowerCase()}`);
    }
  }
  // 3. Each member decides: its goal or idle.
  for (const w of walkers) {
    const inputs = crew.memberInputs(w.id, t);
    crew.keeper(w.id).setCurrent(w.decider, w.goalOption, inputs.discipline);
    const choice = w.decider.decide([
      { option: w.goalOption, inputs: inputs.row },
      { option: w.idleOption, inputs: inputs.row },
    ]);
    w.onGoal = choice !== null && choice.index === 0;
    crew.memberChose(w.id, t, w.onGoal);
  }
}
for (const line of lines) console.log(line);

for (const w of walkers) {
  w.decider.free();
  w.nav.free();
}
crew.free();
