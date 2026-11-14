// Node test of the WebAssembly build on tabletop-like situations.
// Run: js/run.sh  (builds the .wasm first)

import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { ALL_OPTIONS, KEEPER, MEMORY, ONE, Report, SHAPE, SQUAD, loadSagax, HEX, FORMATION, Progress, Interval, Alertness, PlanError } from './sagax.mjs';

const wasm = await readFile(new URL('../target/wasm32-unknown-unknown/release/sagax_wasm.wasm', import.meta.url));
console.log(`wasm size ${(wasm.length / 1024).toFixed(0)} KB`);
const lib = await loadSagax(wasm);
let passed = 0;
function test(name, fn) {
  fn();
  passed++;
  console.log(`ok  ${name}`);
}

test('open ground: diagonal counts as one step', () => {
  const g = lib.grid(20, 20, { preset: 'tabletop' });
  const way = g.findPath({ x: 0, y: 0 }, { x: 5, y: 3 });
  assert.equal(way.reached, true);
  assert.equal(way.cost, 5);
  assert.deepEqual(way.cells[0], { x: 0, y: 0, level: 0 });
  assert.deepEqual(way.cells.at(-1), { x: 5, y: 3, level: 0 });
  g.free();
});

test('walls between cells force a way through the door', () => {
  // a wall along x = 5|6 from y = 0 to 9, door at y = 9
  const g = lib.grid(12, 10, { preset: 'tabletop' });
  for (let y = 0; y < 9; y++) {
    g.addWall(5, y, 1, 0);
    g.addWall(5, y, 1, 1);
    g.addWall(5, y, 1, -1);
    g.addWall(5, y + 1, 1, -1);
  }
  const way = g.findPath({ x: 2, y: 0 }, { x: 8, y: 0 });
  assert.equal(way.reached, true);
  assert.ok(way.cells.some((c) => c.y === 9), 'goes through the door');
  for (let i = 1; i < way.cells.length; i++) {
    const a = way.cells[i - 1], b = way.cells[i];
    assert.ok(!(a.x <= 5 && b.x >= 6 && a.y < 9 && b.y < 9 && !(a.y === 8 && b.y === 9)), 'never through the wall');
  }
  g.free();
});

test('leaving an enemy reach costs extra: the way goes around', () => {
  // enemy at (5, 5) with reach 1; walker starts outside and must not cross the reach
  const g = lib.grid(11, 11, { preset: 'tabletop' });
  g.setLeaveCost(10);
  for (let dx = -1; dx <= 1; dx++) for (let dy = -1; dy <= 1; dy++) g.addZone(5 + dx, 5 + dy, 0);
  g.block(5, 5); // the enemy itself
  const way = g.findPath({ x: 5, y: 1 }, { x: 5, y: 9 });
  assert.equal(way.reached, true);
  assert.ok(way.cells.every((c) => Math.abs(c.x - 5) > 1 || Math.abs(c.y - 5) > 1), 'avoids the reach');
  g.free();
});

test('in a corridor past an enemy the leave cost is paid once', () => {
  // corridor 1 cell wide (y = 1), enemy reach covers x = 4..6
  const g = lib.grid(12, 3, { preset: 'tabletop' });
  for (let x = 0; x < 12; x++) {
    g.block(x, 0);
    g.block(x, 2);
  }
  for (let x = 4; x <= 6; x++) g.addZone(x, 1, 0);
  g.setLeaveCost(10);
  const way = g.findPath({ x: 0, y: 1 }, { x: 11, y: 1 });
  assert.equal(way.reached, true);
  assert.equal(way.cost, 11 + 10);
  g.free();
});

test('reachable cells within the movement budget (GS + 2)', () => {
  const g = lib.grid(30, 30, { preset: 'tabletop' });
  const area = g.reachable({ x: 15, y: 15 }, 6).cells;
  assert.equal(area.length, 13 * 13);
  assert.ok(area.every((c) => c.cost <= 6));
  const back = g.pathTo(21, 9);
  assert.equal(back.cost, 6);
  assert.equal(g.pathTo(22, 15), null);
  g.free();
});

test('unreachable goal gives the closest cell', () => {
  const g = lib.grid(10, 10, { preset: 'tabletop' });
  for (let y = 0; y < 10; y++) g.block(5, y);
  const way = g.findPath({ x: 0, y: 5 }, { x: 9, y: 5 });
  assert.equal(way.status, 'partial');
  assert.deepEqual(way.cells.at(-1), { x: 4, y: 5, level: 0 });
  g.free();
});

test('hex grid: distance, walls between hexes, reachable', () => {
  const g = lib.grid(20, 20, { preset: 'tabletop', hex: HEX.oddR });
  const open = g.findPath({ x: 2, y: 2 }, { x: 10, y: 2 });
  assert.equal(open.reached, true);
  assert.equal(open.cost, 8);
  // walls between column 5 and 6, open only at row 19
  for (let y = 0; y < 19; y++) {
    g.hexNeighbors(5, y).forEach((c, dir) => { if (c.x > 5) g.addHexWall(5, y, dir); });
    g.hexNeighbors(6, y).forEach((c, dir) => { if (c.x < 6) g.addHexWall(6, y, dir); });
  }
  const around = g.findPath({ x: 2, y: 2 }, { x: 10, y: 2 });
  assert.equal(around.reached, true);
  assert.ok(around.cells.some((c) => c.y >= 18), 'walks round the end of the wall');
  const area = g.reachable({ x: 10, y: 10 }, 2).cells;
  assert.equal(area.length, 1 + 6 + 12);
  g.free();
});

test('hex walls: raw bits throw', () => {
  const g = lib.grid(5, 5, { preset: 'tabletop', hex: HEX.oddR });
  const hexMsg = /hex wall bits changed in 0\.9/;
  assert.throws(() => g.walls(), (e) => e instanceof TypeError && hexMsg.test(e.message));
  assert.throws(() => g.setWalls(new Uint8Array(25)), (e) => e instanceof TypeError && hexMsg.test(e.message));
  // addHexWall still blocks exactly the one pair
  const n = g.hexNeighbors(2, 2)[3];
  g.addHexWall(2, 2, 3);
  const area = g.reachable({ x: 2, y: 2 }, 1).cells;
  assert.equal(area.length, 1 + 5);
  assert.ok(!area.some((c) => c.x === n.x && c.y === n.y));
  g.free();
  const square = lib.grid(3, 3, { preset: 'tabletop' });
  assert.equal(square.walls().length, 9);
  square.free();
});

test('speed on a 60 x 60 battle map', () => {
  const g = lib.grid(60, 60, { preset: 'tabletop' });
  for (let i = 0; i < 600; i++) g.block((i * 37) % 60, (i * 91) % 60);
  const t = performance.now();
  for (let i = 0; i < 200; i++) g.findPath({ x: 0, y: 0 }, { x: 59, y: 59 });
  const findMs = (performance.now() - t) / 200;
  const t2 = performance.now();
  for (let i = 0; i < 200; i++) g.reachable({ x: 30, y: 30 }, 10);
  const reachMs = (performance.now() - t2) / 200;
  console.log(`    findPath ${findMs.toFixed(3)} ms, reachable(10) ${reachMs.toFixed(3)} ms`);
  assert.ok(findMs < 5);
  g.free();
});

test('flow field: every walker finds the nearest goal', () => {
  const g = lib.grid(20, 10, { preset: 'tabletop' });
  const n = g.buildFlow([{ x: 0, y: 5 }, { x: 19, y: 5 }]);
  assert.equal(n, 200);
  assert.equal(g.flowCost(4, 5), 4);
  assert.equal(g.flowCost(16, 5), 3);
  const [a, b, c] = g.flowNext([{ x: 16, y: 5 }, { x: 0, y: 5 }, { x: 3, y: 5 }]);
  assert.deepEqual(a, { x: 17, y: 5, level: 0, cost: 3 });
  assert.deepEqual(b, { x: 0, y: 5, level: 0, cost: 0 });
  assert.deepEqual(c, { x: 2, y: 5, level: 0, cost: 3 });
  assert.equal(g.findPath({ x: 1, y: 1 }, { x: 3, y: 1 }).cells.length, 3, 'flow keeps paths apart');
  g.free();
});

test('smoothing and formation after findPath', () => {
  const g = lib.grid(30, 10, { preset: 'tabletop' });
  g.findPath({ x: 0, y: 0 }, { x: 20, y: 4 });
  assert.deepEqual(g.way('smooth'), [{ x: 0, y: 0, level: 0 }, { x: 20, y: 4, level: 0 }]);
  g.findPath({ x: 2, y: 4 }, { x: 20, y: 4 });
  const f = g.formation();
  assert.equal(f.members.length, 3);
  assert.deepEqual(f.leader.at(-1), { x: 20, y: 4, level: 0 });
  assert.deepEqual(f.members[0].at(-1), { x: 20, y: 5, level: 0 });
  assert.deepEqual(f.members[1].at(-1), { x: 19, y: 4, level: 0 });
  g.free();
});

test('D* Lite repairs the way when a cell gets blocked (square and hex)', () => {
  const g = lib.grid(20, 10, { preset: 'tabletop' });
  g.dstarBegin({ x: 0, y: 5 }, { x: 10, y: 5 });
  assert.equal(g.dstarCompute().status, 'found');
  assert.equal(g.dstarCompute().cost, 10);
  g.dstarMoveStart({ x: 2, y: 5 });
  for (let y = 0; y < 10; y++) if (y !== 9) g.block(5, y);
  g.dstarCellsChanged([...Array(9).keys()].map((y) => ({ x: 5, y })));
  const way = g.dstarCompute();
  assert.equal(way.status, 'found');
  assert.ok(way.cells.some((c) => c.x === 5 && c.y === 9), 'through the gap');
  assert.equal(way.cost, g.findPath({ x: 2, y: 5 }, { x: 10, y: 5 }).cost);
  g.free();

  const h = lib.grid(15, 15, { preset: 'tabletop', hex: HEX.oddR });
  h.dstarBegin({ x: 2, y: 7 }, { x: 12, y: 7 });
  assert.equal(h.dstarCompute().status, 'found');
  h.block(7, 7);
  h.dstarCellsChanged([{ x: 7, y: 7 }]);
  const repaired = h.dstarCompute();
  assert.equal(repaired.status, 'found');
  assert.equal(repaired.cost, h.findPath({ x: 2, y: 7 }, { x: 12, y: 7 }).cost);
  h.free();
});

test('hex: smoothing and formation work on every layout', () => {
  for (const hex of Object.values(HEX)) {
    const g = lib.grid(30, 20, { preset: 'tabletop', hex });
    g.findPath({ x: 2, y: 12 }, { x: 20, y: 4 });
    assert.deepEqual(g.way('smooth'), [{ x: 2, y: 12, level: 0 }, { x: 20, y: 4, level: 0 }], `layout ${hex}`);
    for (let y = 0; y < 20; y++) if (y !== 10) g.block(15, y);
    const way = g.findPath({ x: 5, y: 10 }, { x: 25, y: 10 });
    assert.equal(way.reached, true);
    const f = g.formation(FORMATION.block);
    const n = f.leader.length;
    for (let i = 0; i < n; i++) {
      const here = [f.leader[i], ...f.members.map((m) => m[i])].map((c) => `${c.x},${c.y}`);
      assert.equal(new Set(here).size, 4, `layout ${hex} step ${i}: nobody shares a cell`);
      assert.ok(f.members.every((m) => m[i].x !== 15 || m[i].y === 10), `layout ${hex}: nobody in the wall`);
    }
    g.free();
  }
});

test('continue before any search is idle', () => {
  const g = lib.grid(10, 10, { preset: 'tabletop' });
  assert.equal(g.continue().status, 'idle');
  g.free();
});

test('small budgets, continue, outputs, bulk filling', () => {
  const g = lib.grid(30, 30, { preset: 'tabletop' });
  const costs = new Uint32Array(30 * 30);
  for (let y = 0; y < 25; y++) costs[y * 30 + 15] = 0xffffffff;
  g.setCosts(costs);
  const whole = g.findPath({ x: 1, y: 1 }, { x: 28, y: 1 });
  let part = g.findPath({ x: 1, y: 1 }, { x: 28, y: 1 }, { budget: 10 });
  let calls = 1;
  while (part.status === 'running') { part = g.continue(10); calls++; }
  assert.ok(calls > 10);
  assert.deepEqual(part, whole);
  const turns = g.way('turns');
  assert.deepEqual(turns[0], { x: 1, y: 1, level: 0 });
  assert.deepEqual(turns.at(-1), { x: 28, y: 1, level: 0 });
  assert.ok(g.way('smooth').length <= turns.length);
  g.free();
});

test('tracing: every step, same result, off again', () => {
  const g = lib.grid(20, 20, { preset: 'tabletop' });
  for (let y = 0; y < 15; y++) g.block(10, y);
  const plain = g.findPath({ x: 2, y: 2 }, { x: 18, y: 2 });
  g.setTrace(true);
  const traced = g.findPath({ x: 2, y: 2 }, { x: 18, y: 2 });
  assert.deepEqual(traced, plain);
  const { events, dropped } = g.takeTrace();
  assert.equal(dropped, 0);
  assert.equal(events[0].kind, 'started');
  assert.deepEqual([events[0].x, events[0].y], [2, 2]);
  assert.equal(events.filter((e) => e.kind === 'expanded').length, traced.expanded);
  assert.equal(events.at(-1).kind, 'finished');
  assert.equal(events.at(-1).found, true);
  g.dstarBegin({ x: 2, y: 2 }, { x: 18, y: 2 });
  g.dstarCompute();
  assert.ok(g.takeTrace().events.some((e) => e.kind === 'expanded'));
  g.setTrace(false);
  g.findPath({ x: 2, y: 2 }, { x: 18, y: 2 });
  assert.equal(g.takeTrace().events.length, 0);
  g.free();
});

test('flee from threats, flee to the nearest exit', () => {
  const g = lib.grid(20, 5, { preset: 'tabletop' });
  const away = g.flee({ x: 5, y: 2 }, [{ x: 3, y: 2 }], 8);
  assert.equal(away.status, 'found');
  assert.deepEqual(away.cells[0], { x: 5, y: 2, level: 0 });
  assert.ok(away.cells.at(-1).x >= 9);
  const exit = g.findPath({ x: 15, y: 2 }, [{ x: 0, y: 2 }, { x: 19, y: 2 }]);
  assert.deepEqual(exit.cells.at(-1), { x: 19, y: 2, level: 0 });
  g.free();
});

test('ways split walking cost and zone penalty', () => {
  const g = lib.grid(10, 1, { preset: 'tabletop' });
  for (let x = 0; x <= 3; x++) g.addZone(x, 0, 0);
  g.setLeaveCost(5);
  const way = g.findPath({ x: 2, y: 0 }, { x: 7, y: 0 });
  assert.equal(way.cost, 10);
  assert.equal(way.steps, 5);
  assert.equal(way.zonesLeft, 1);
  g.reachable({ x: 2, y: 0 }, 20);
  const back = g.pathTo(6, 0);
  assert.deepEqual([back.steps, back.zonesLeft], [4, 1]);
  g.setLeaveCost(0);
  assert.deepEqual([g.findPath({ x: 2, y: 0 }, { x: 7, y: 0 }).steps, g.findPath({ x: 2, y: 0 }, { x: 7, y: 0 }).zonesLeft], [5, 0]);
  g.free();
});

test('grid needs rules or a preset', () => {
  assert.throws(() => lib.grid(5, 5), TypeError);
  assert.throws(() => lib.grid(5, 5, { hex: HEX.oddR }), TypeError);
  assert.throws(() => lib.grid(5, 5, { diagonal: 14 }), TypeError);
  assert.throws(() => lib.grid(5, 5, { preset: 'chess' }), RangeError);
  const explicit = lib.grid(5, 5, { straight: 2, diagonal: 3 });
  assert.equal(explicit.findPath({ x: 0, y: 0 }, { x: 3, y: 1 }).cost, 3 + 2 + 2);
  explicit.free();
  const hex = lib.grid(5, 5, { hex: HEX.oddR, step: 4 });
  assert.equal(hex.findPath({ x: 0, y: 0 }, { x: 3, y: 0 }).cost, 12);
  hex.free();
});

test('presets', () => {
  const octile = lib.grid(10, 10, { preset: 'octile' });
  assert.equal(octile.findPath({ x: 0, y: 0 }, { x: 3, y: 1 }).cost, 34);
  octile.free();
  const tabletop = lib.grid(10, 10, { preset: 'tabletop' });
  assert.equal(tabletop.findPath({ x: 0, y: 0 }, { x: 3, y: 1 }).cost, 3);
  tabletop.free();
  const hex = lib.grid(10, 10, { preset: 'octile', hex: HEX.oddR });
  assert.equal(hex.findPath({ x: 0, y: 0 }, { x: 3, y: 0 }).cost, 30);
  hex.free();
});

test('levels and links', () => {
  // 5 x 1 x 2: level 0 blocked at x = 2, level 1 open only at x = 2
  const g = lib.grid(5, 1, { preset: 'tabletop', levels: 2 });
  g.block({ x: 2, y: 0, level: 0 });
  for (const x of [0, 1, 3, 4]) g.block({ x, y: 0, level: 1 });
  assert.equal(g.findPath({ x: 0, y: 0 }, { x: 4, y: 0 }).reached, false);
  assert.equal(g.link({ x: 1, y: 0, level: 0 }, { x: 2, y: 0, level: 1 }), true);
  assert.equal(g.link([2, 0, 1], [3, 0, 0]), true);
  assert.equal(g.link({ x: 0, y: 0, level: 0 }, { x: 2, y: 0, level: 1 }), false);
  assert.equal(g.link({ x: 1, y: 0, level: 0 }, { x: 2, y: 0, level: 2 }), false);
  const way = g.findPath({ x: 0, y: 0 }, { x: 4, y: 0 });
  assert.equal(way.reached, true);
  assert.equal(way.cost, 4);
  assert.deepEqual(way.cells.map((c) => c.level), [0, 0, 1, 0, 0]);
  assert.deepEqual(way.cells[2], { x: 2, y: 0, level: 1 });
  // a start on level 1, an area over both levels
  const back = g.findPath({ x: 2, y: 0, level: 1 }, { x: 0, y: 0 });
  assert.deepEqual(back.cells.map((c) => c.level), [1, 0, 0]);
  const area = g.reachable({ x: 0, y: 0 }, 10).cells;
  assert.equal(area.length, 5);
  assert.ok(area.some((c) => c.x === 2 && c.level === 1 && c.cost === 2));
  assert.equal(g.costs().length, 10);
  g.setCost({ x: 3, y: 0, level: 0 }, 5);
  assert.equal(g.costs()[3], 5);
  g.free();
});

test('decide: utility scores, ties, seeded chance, trace', () => {
  const d = lib.decider();
  const attack = d.addOption(1, [
    { input: 0, curve: 'inverse', min: 1, max: 10 },
    { input: 1, curve: 'linear', min: 0, max: 100 },
  ]);
  const flee = d.addOption(1, [{ input: 1, curve: 'inverseQuadratic', min: 0, max: 100 }]);
  const c = (option, target, inputs) => ({ option, target, inputs });
  const healthy = d.decide([c(attack, 7, [5, 90]), c(attack, 8, [2, 90]), c(flee, 0, [0, 90])]);
  assert.equal(healthy.index, 1);
  assert.equal(healthy.target, 8);
  const hurt = d.decide([c(attack, 7, [5, 5]), c(attack, 8, [2, 5]), c(flee, 0, [0, 5])]);
  assert.equal(hurt.option, flee);
  assert.equal(d.decide([c(attack, 1, [3, 50]), c(attack, 2, [3, 50])]).index, 0);
  assert.equal(d.decide([c(attack, 1, [20, 50])]), null);
  assert.throws(() => d.decide([c(9, 0, [0, 0])]));

  // Same function as Rust: SplitMix64 of the seed, modulo the sum of the top scores.
  const w = lib.decider();
  w.addOption(3);
  w.addOption(1);
  const mix = (seed) => {
    const m = (1n << 64n) - 1n;
    let z = (BigInt(seed) + 0x9e3779b97f4a7c15n) & m;
    z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & m;
    z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & m;
    return z ^ (z >> 31n);
  };
  for (const seed of [0, 1, 42, 2 ** 40 + 7]) {
    const got = w.decide([c(0, 0, []), c(1, 0, [])], { pick: 'weighted', seed });
    const expect = mix(seed) % BigInt(4 * ONE) < BigInt(3 * ONE) ? 0 : 1;
    assert.equal(got.index, expect, `seed ${seed}`);
    assert.deepEqual(got.scores, [3 * ONE, ONE]);
  }
  w.free();

  d.setTrace(true);
  d.decide([c(attack, 0, [1, 100])]);
  const events = d.takeTrace();
  assert.deepEqual(events.map((e) => e.kind), ['factor', 'factor', 'scored', 'chosen']);
  assert.equal(events[0].value, ONE);
  d.free();
});

test('decide: combine, pull, multiply, rank, current', () => {
  const d = lib.decider();
  // three factors of 0.95 (input 95 of 0..100)
  const f = (input) => ({ input, curve: 'linear', min: 0, max: 100 });
  const many = d.addOption(1, [f(0), f(1), f(2)], { combine: 'geometricMean' });
  const one = d.addOption(1, [f(0)]);
  const c = d.decide([{ option: many, inputs: [95, 95, 95] }, { option: one, inputs: [94, 0, 0] }]);
  assert.equal(c.index, 0);
  assert.ok(Math.abs(c.score - 0.95 * ONE) < 3);
  // a true factor: average would let it through, multiply does not
  const shoot = d.addOption(1, [f(0), { input: 3, curve: 'step', min: 1, apply: 'multiply' }], { combine: 'average' });
  assert.equal(d.decide([{ option: shoot, inputs: [100, 0, 0, 0] }]), null);
  // gentle pull: a factor of 0 only halves
  const soft = d.addOption(1, [{ ...f(0), pull: 0.5 }]);
  assert.equal(d.decide([{ option: soft, inputs: [0] }]).score, ONE - Math.round(0.5 * ONE));
  // rank: emergency first
  const flee = d.addOption(0.1, [{ input: 0, curve: 'step', min: 1 }], { rank: 1 });
  assert.equal(d.decide([{ option: one, inputs: [100] }, { option: flee, inputs: [5] }]).option, flee);
  // current action keeps the tie
  d.setCurrent({ option: one, target: 2 }, 0.1);
  assert.equal(d.decide([{ option: one, target: 1, inputs: [50] }, { option: one, target: 2, inputs: [50] }]).target, 2);
  d.setCurrent(null);
  d.setCombine('minimum');
  d.setCombine(null, many);
  assert.throws(() => d.setCombine('nope'));
  assert.throws(() => d.addConsideration(one, { input: 0, apply: 'nope' }));
  d.free();
});

test('decide: memory, cooldown, dislike', () => {
  const d = lib.decider();
  const strike = d.addOption(1, [{ input: MEMORY.sincePair, curve: 'linear', min: 0, max: 10 }]);
  const wait = d.addOption(0.5);
  const cands = [{ option: strike, target: 1 }, { option: strike, target: 2 }, { option: wait }];
  d.setNow(100);
  assert.equal(d.decide(cands).index, 0);
  d.record(100, cands[0]);
  d.setNow(102);
  assert.equal(d.decide(cands).index, 1); // target 1 was struck just now
  d.setCooldown(strike, 3);
  assert.equal(d.decide(cands).option, wait);
  d.setNow(103);
  assert.equal(d.decide(cands).index, 1);
  d.setWindow(2);
  d.forget();
  assert.equal(d.decide(cands).index, 0);
  d.free();
});

test('state machine: patrol, chase, search, abort', () => {
  const m = lib.machine(['patrol', 'chase', 'search', 'dead'], ['seen', 'killed']);
  m.transition({ from: '*', to: 'dead', when: ['killed'] });
  m.transition({ from: 'patrol', to: 'chase', when: ['seen'] });
  m.transition({ from: 'chase', to: 'search', unless: ['seen'], after: 10 });
  m.transition({ from: 'search', to: 'chase', when: ['seen'] });
  m.timeout('search', 100, 'patrol');
  assert.equal(m.update(1), null);
  assert.deepEqual(m.update(2, { seen: true }), { from: 'patrol', to: 'chase', reason: 'transition', transition: 1 });
  assert.equal(m.update(5), null);
  assert.equal(m.update(12).to, 'search');
  assert.equal(m.update(111), null);
  assert.equal(m.update(112).reason, 'timeout');
  assert.equal(m.state, 'patrol');
  assert.equal(m.entered, 112);
  assert.equal(m.update(113, { seen: true, killed: true }).to, 'dead');
  assert.equal(m.update(114, { killed: true }), null);
  m.setState('patrol', 200);
  assert.equal(m.state, 'patrol');
  assert.throws(() => m.update(201, { nope: true }));
  assert.throws(() => m.transition({ to: 'nowhere' }));
  m.free();
});


// ---- world memory ----
// Golden numbers are the ones of crates/sagax-decide/src/memory/tests.rs.

const fact = (subject, kind, seenAt, more = {}) => ({ subject, kind, pos: [0, 0, 0], seenAt, source: 0, strength: 0, data: 0, ...more });
const msg = (subject, kind = 1, more = {}) => ({ kind, subject, pos: [1, 2, 3], seenAt: 7, strength: 8, data: 9, sender: 10, keepIfSenderGone: false, ...more });
const keys = (k) => k.list().map((f) => [f.subject, f.kind]);

test('memory: now comes first', () => {
  const k = lib.knowledge(4);
  assert.equal(k.learn(5, { subject: 1, kind: 2, pos: [0, 0, 0], seenAt: 5 }), null);
  assert.equal(k.get(1, 2).seenAt, 5);
  // The old order (fact, now) must fail loudly, not store something odd.
  assert.throws(() => k.learn({ subject: 1, kind: 2, pos: [0, 0, 0], seenAt: 5 }, 5), TypeError);
  assert.throws(() => k.merge({ subject: 1, kind: 2, pos: [0, 0, 0], seenAt: 5 }, 5), TypeError);
  assert.equal(k.count(), 1);
  k.free();
  // The same for every call that takes now first: the old order throws a TypeError.
  const o = lib.outbox(2);
  assert.throws(() => o.post(msg(1), 5), TypeError);
  assert.equal(o.pending(), 0);
  o.free();
  const a = lib.areas(2);
  assert.throws(() => a.add({ kind: 1, pos: [0, 0, 0], radius: 5, lifetime: 9 }, 5), TypeError);
  assert.equal(a.count(), 0);
  a.free();
  const sq = lib.squad({});
  assert.throws(() => sq.observe({ subject: 1, kind: 2, pos: [0, 0, 0], seenAt: 5 }, 5), TypeError);
  assert.equal(sq.knowledge().count(), 0);
  sq.free();
  const kp = lib.keeper();
  assert.throws(() => kp.set({ kind: 'hold', pos: [0, 0, 0] }, 5), TypeError);
  assert.equal(kp.goal(), null);
  kp.free();
  const d = lib.decider();
  d.addOption(1);
  assert.throws(() => d.record({ option: 0, target: 1 }, 5), TypeError);
  d.free();
});

test('knowledge: key is (subject, kind), learn overwrites, setters', () => {
  assert.throws(() => lib.knowledge(0), RangeError);
  assert.throws(() => lib.knowledge(4, 'newest'), RangeError);
  const k = lib.knowledge(4);
  assert.equal(k.learn(0, fact(1, 1, 0)), null);
  assert.equal(k.learn(0, fact(1, 2, 0)), null);
  assert.equal(k.learn(0, fact(2, 1, 0)), null);
  assert.equal(k.count(), 3);
  assert.equal(k.learn(5, fact(1, 2, 5, { pos: [1, -2, 3] })), null); // keeps its slot
  assert.deepEqual(keys(k), [[1, 1], [1, 2], [2, 1]]);
  assert.deepEqual(k.get(1, 2), { subject: 1, kind: 2, pos: [1, -2, 3], seenAt: 5, source: 0, strength: 0, data: 0 });
  assert.equal(k.get(2, 2), null);
  const old = fact(1, 2, 3, { source: 2, strength: 4, data: 5, pos: [7, 8, 9] });
  assert.equal(k.learn(5, old), null); // learn overwrites even an older fact
  assert.deepEqual(k.get(1, 2), old);
  assert.equal(k.touch(1, 2, 20) && k.setPos(1, 2, [1, 1, 1]) && k.setData(1, 2, 6) && k.setStrength(1, 2, 8), true);
  assert.deepEqual(k.get(1, 2), fact(1, 2, 20, { source: 2, strength: 8, data: 6, pos: [1, 1, 1] }));
  assert.equal(k.touch(9, 9, 0) || k.setPos(9, 9, [0, 0, 0]) || k.setData(9, 9, 0) || k.setStrength(9, 9, 0) || k.remove(9, 9), false);
  assert.equal(k.remove(1, 2), true);
  assert.equal(k.removeSubject(1), 1);
  assert.equal(k.removeSubject(1), 0);
  k.learn(0, fact(2, 5, 0));
  k.learn(0, fact(2, 6, 0));
  assert.equal(k.removeSubject(2), 3); // all kinds
  assert.deepEqual(k.list(), []);
  k.learn(0, fact(3, 0, 0));
  k.clear();
  assert.equal(k.count(), 0);
  // extreme values survive the round trip
  k.learn(0, { subject: 4294967295, kind: 4294967295, pos: [-2147483648, 2147483647, -1], seenAt: 4294967295, source: 255, strength: 4294967295, data: 4294967295 });
  assert.deepEqual(k.get(4294967295, 4294967295), { subject: 4294967295, kind: 4294967295, pos: [-2147483648, 2147483647, -1], seenAt: 4294967295, source: 255, strength: 4294967295, data: 4294967295 });
  k.free();
  assert.throws(() => k.count(), /free/);
});

test('knowledge: evict oldest (tie earlier slot) and weakest then oldest', () => {
  const k = lib.knowledge(3, 'oldest');
  k.learn(0, fact(1, 0, 0));
  k.learn(1, fact(2, 0, 1));
  k.learn(2, fact(3, 0, 2));
  assert.equal(k.touch(1, 0, 3), true);
  assert.equal(k.learn(4, fact(4, 0, 4)).subject, 2); // returns the evicted fact
  assert.deepEqual(keys(k), [[1, 0], [3, 0], [4, 0]]);
  const t = lib.knowledge(3, 'oldest');
  for (let s = 1; s <= 3; s++) t.learn(5, fact(s, 0, 5));
  assert.equal(t.learn(5, fact(9, 0, 5)).subject, 1);
  assert.deepEqual(keys(t), [[2, 0], [3, 0], [9, 0]]);
  assert.equal(t.learn(6, fact(2, 0, 6)), null); // present key does not evict
  const w = lib.knowledge(4, 'weakest');
  const with_ = (s, at, st) => fact(s, 0, at, { strength: st });
  w.learn(10, with_(1, 10, 5));
  w.learn(10, with_(2, 0, 9));
  w.learn(10, with_(3, 4, 5));
  w.learn(10, with_(4, 4, 5));
  assert.equal(w.learn(10, with_(5, 10, 7)).subject, 3); // weakest 5, oldest of those, earlier slot
  assert.deepEqual(keys(w), [[1, 0], [2, 0], [4, 0], [5, 0]]);
  assert.equal(w.learn(10, with_(6, 10, 9)).subject, 4);
  const s = lib.knowledge(2, 'weakest');
  s.learn(100, with_(1, 0, 8));
  s.learn(100, with_(2, 99, 2));
  assert.equal(s.learn(100, with_(3, 100, 5)).subject, 2); // strength beats age
});

test('knowledge: merge keeps the newer, also across the wrap', () => {
  const k = lib.knowledge(2);
  k.merge(110, fact(1, 0, 100, { data: 1 }));
  assert.equal(k.merge(110, fact(1, 0, 90, { data: 2 })), null);
  assert.equal(k.get(1, 0).data, 1);
  k.merge(110, fact(1, 0, 100, { data: 3 })); // equal: not older
  assert.equal(k.get(1, 0).data, 3);
  k.merge(110, fact(1, 0, 105, { data: 4 }));
  assert.deepEqual([k.get(1, 0).seenAt, k.get(1, 0).data], [105, 4]);
  const w = lib.knowledge(1);
  w.merge(5, fact(1, 0, 4294967293)); // age 8
  w.merge(5, fact(1, 0, 2, { data: 9 })); // age 3 is newer
  assert.equal(w.get(1, 0).data, 9);
  w.merge(5, fact(1, 0, 4294967287, { data: 7 })); // age 15 is older
  assert.equal(w.get(1, 0).data, 9);
  const f = lib.knowledge(1);
  f.merge(1, fact(1, 0, 0));
  assert.equal(f.merge(1, fact(2, 0, 1)).subject, 1);
});

test('knowledge: learnMessage and mergeMessage turn a message into a fact', () => {
  const k = lib.knowledge(4);
  const m = msg(5, 3);
  k.learnMessage(10, m, 2);
  assert.deepEqual(k.get(5, 3), { subject: 5, kind: 3, pos: [1, 2, 3], seenAt: 7, source: 2, strength: 8, data: 9 });
  k.mergeMessage(10, { ...m, seenAt: 3, data: 1 }, 1); // older: kept
  assert.equal(k.get(5, 3).data, 9);
  k.mergeMessage(10, { ...m, seenAt: 8, data: 1 }, 1);
  assert.deepEqual([k.get(5, 3).data, k.get(5, 3).source], [1, 1]);
});

test('knowledge: expire, expire with per-kind ages, expireKind', () => {
  const k = lib.knowledge(5);
  k.learn(0, fact(1, 1, 0));
  k.learn(0, fact(2, 2, 0));
  k.learn(10, fact(3, 1, 10));
  k.learn(10, fact(4, 2, 10));
  assert.deepEqual(k.expire(20, 20).map((f) => f.subject), [1, 2]);
  // kind 1 lives 5, kind 2 lives 15: only subject 3 (age 10, kind 1) goes
  assert.deepEqual(k.expire(20, 0, (f) => (f.kind === 1 ? 5 : 15)).map((f) => f.subject), [3]);
  assert.deepEqual(keys(k), [[4, 2]]);
  assert.throws(() => k.expire(20, 0, () => undefined), TypeError);
  // Infinity and limits past 2^32 - 1 never expire; bad limits are refused
  assert.deepEqual(k.expire(1e9, 0, () => Infinity), []);
  assert.deepEqual(k.expire(1e9, 0, () => 2 ** 32), []);
  for (const bad of [-1, NaN, 1.5, -Infinity]) assert.throws(() => k.expire(20, 0, () => bad), RangeError);
  assert.equal(k.count(), 1); // a bad callback removes nothing
  const e = lib.knowledge(5);
  e.learn(0, fact(1, 1, 0));
  e.learn(0, fact(2, 2, 0));
  e.learn(0, fact(3, 1, 0));
  e.learn(8, fact(4, 1, 8));
  const gone = e.expireKind(10, 1, 10);
  assert.deepEqual(gone.map((f) => f.subject), [1, 3]); // slot order, kind 1 only
  assert.deepEqual(gone[0], fact(1, 1, 0));
  assert.deepEqual(keys(e), [[2, 2], [4, 1]]);
  assert.deepEqual(e.expireKind(10, 9, 0), []);
  // wrapped clock: age 15 at now = 10
  const w = lib.knowledge(1);
  w.learn(0, fact(1, 0, 4294967291));
  assert.deepEqual(w.expire(10, 16), []);
  assert.equal(w.expire(10, 15).length, 1);
  w.learn(0, fact(1, 0, 4294967291));
  assert.equal(w.expireKind(10, 0, 15).length, 1);
});

test('outbox: cooldown from the last accepted post, full, delays, sender gone', () => {
  assert.throws(() => lib.outbox(0), RangeError);
  const o = lib.outbox(4);
  assert.equal(o.post(0, msg(1), 102, 170), 'ok');
  assert.equal(o.post(169, msg(2), 102, 170), 'cooldown');
  assert.equal(o.post(170, msg(3), 102, 170), 'ok'); // a refused post does not move the start
  assert.equal(o.post(339, msg(4), 102, 170), 'cooldown');
  assert.equal(o.post(340, msg(5), 102, 170), 'ok');
  assert.equal(o.pending(), 3);
  const w = lib.outbox(2);
  assert.equal(w.post(4294967285, msg(1), 0, 170), 'ok');
  assert.equal(w.post(158, msg(1), 0, 170), 'cooldown');
  assert.equal(w.post(159, msg(1), 0, 170), 'ok');
  const f = lib.outbox(2);
  assert.equal(f.post(0, msg(1), 5), 'ok');
  assert.equal(f.post(0, msg(2), 5), 'ok');
  assert.equal(f.post(0, msg(3), 5), 'full');
  assert.equal(f.due(5).length, 2);
  assert.equal(f.post(5, msg(3), 5), 'ok');

  const d = lib.outbox(4);
  d.post(0, msg(1), 10);
  d.post(1, msg(2), 2);
  d.post(2, msg(3), 0);
  assert.deepEqual(d.due(3).map((m) => m.subject), [2, 3]);
  assert.equal(d.pending(), 1);
  const [last] = d.due(10);
  assert.deepEqual(last, msg(1));
  assert.equal(d.pending(), 0);

  const g = lib.outbox(4);
  g.post(0, msg(1), 5);
  g.post(0, msg(2, 1, { keepIfSenderGone: true }), 5);
  g.senderGone();
  assert.equal(g.pending(), 1);
  assert.deepEqual(g.due(5), [msg(2, 1, { keepIfSenderGone: true })]);
  g.post(6, msg(3), 5);
  g.clear();
  assert.equal(g.pending(), 0);
  assert.equal(g.post(7, msg(4), 5, 100), 'cooldown'); // the cooldown survives clear
  assert.equal(g.post(106, msg(4), 5, 100), 'ok');

  const m = lib.outbox(4);
  for (const s of [1, 2, 3]) m.post(0, msg(s), 0);
  assert.deepEqual(m.due(5, 2).map((x) => x.subject), [1, 2]);
  assert.equal(m.pending(), 1);
  assert.deepEqual(m.due(5, 0), []);
  assert.equal(m.due(5, 9).length, 1);
  // a message with the extremes of every field
  const big = { kind: 4294967295, subject: 4294967295, pos: [-2147483648, 2147483647, -1], seenAt: 4294967295, strength: 4294967295, data: 4294967295, sender: 4294967295, keepIfSenderGone: true };
  m.post(0, big, 0);
  assert.deepEqual(m.due(0), [big]);
});

test('inbox: push order, wrap, full replaces the latest if earlier', () => {
  assert.throws(() => lib.inbox(0), RangeError);
  const b = lib.inbox(4);
  assert.equal(b.push(msg(1), 20), true);
  assert.equal(b.push(msg(2), 5), true);
  assert.equal(b.push(msg(3), 10), true);
  assert.deepEqual(b.takeDue(10).map((m) => m.subject), [2, 3]);
  assert.equal(b.count(), 1);
  assert.equal(b.takeDue(19).length, 0);
  assert.equal(b.takeDue(20).length, 1);
  assert.equal(b.count(), 0);
  assert.equal(b.push(msg(4), 5), true); // just after the wrap
  assert.equal(b.takeDue(4294967294).length, 0);
  assert.equal(b.takeDue(5).length, 1);
  assert.equal(lib.arrival(4294967294, 2, 3, 4), 12);
  assert.equal(lib.arrival(10, 0, 0, 0), 10);
  assert.equal(lib.arrival(0, 0, 4294967295, 2), 4294967294); // wrapping product
  b.push(msg(5), 1);
  b.clear();
  assert.equal(b.count(), 0);

  const f = lib.inbox(3);
  f.push(msg(1), 10);
  f.push(msg(2), 30);
  f.push(msg(3), 30);
  assert.equal(f.push(msg(4), 30), false); // equal to the latest: refused
  assert.equal(f.push(msg(5), 20), true); // the later of the two latest goes
  assert.deepEqual(f.takeDue(100).map((m) => m.subject), [1, 2, 5]);
  const w = lib.inbox(3);
  w.push(msg(1), 4294967295 - 0);
  w.push(msg(2), 3);
  w.push(msg(3), 4294967295);
  assert.equal(w.push(msg(4), 4294967293), true); // earlier than 3 (after the wrap): replaces it
  assert.equal(w.push(msg(5), 1), false);
  assert.deepEqual(w.takeDue(4294967295).map((m) => m.subject), [1, 3, 4]);
  const m = lib.inbox(4);
  for (const s of [1, 2, 3]) m.push(msg(s), 0);
  assert.deepEqual(m.takeDue(0, 2).map((x) => x.subject), [1, 2]);
  assert.equal(m.count(), 1);
});

test('areas: renew, evict, expire, contains, blocks, cost', () => {
  assert.throws(() => lib.areas(0), RangeError);
  const area = (kind, x, radius, more = {}) => ({ kind, pos: [x, 0, 0], radius, lifetime: 255, strength: 0, data: 0, blocking: false, source: null, ...more });
  const z = lib.areas(4);
  assert.equal(z.add(0, area(1, 0, 0x200)), 0);
  assert.equal(z.add(10, area(1, 0x1ff, 0x200, { data: 2, source: [5, 6, -7] })), 0); // renews
  assert.equal(z.count(), 1);
  assert.deepEqual(z.list(), [area(1, 0x1ff, 0x200, { data: 2, since: 10, source: [5, 6, -7] })]);
  assert.equal(z.add(11, area(1, 0x200 + 0x1ff, 0x200)), 1); // exactly at the radius: new
  assert.equal(z.add(1, area(2, 0, 100)), 2); // other kind: new
  assert.equal(z.containsKind([0, 0, 0], 2), 2);
  assert.equal(z.containsKind([0, 0, 0], 3), null);
  assert.equal(z.contains([0, 0, 0]), 0);
  assert.equal(z.contains([5000, 0, 0]), null);
  z.clear();
  assert.equal(z.count(), 0);
  // renewal uses the existing radius, then replaces it
  z.add(0, area(1, 0, 10));
  assert.equal(z.add(1, area(1, 50, 1000)), 1);
  assert.equal(z.add(2, area(1, 5, 20)), 0);
  assert.equal(z.list()[0].radius, 20);
  // full: the oldest goes, the new one is last
  const f = lib.areas(2);
  f.add(5, area(1, 0, 10, { data: 1 }));
  f.add(2, area(1, 100, 10, { data: 2 }));
  assert.equal(f.add(6, area(1, 200, 10, { data: 3 })), 1);
  assert.deepEqual(f.list().map((a) => a.data), [1, 3]);
  // expire by own lifetime, also across the wrap
  const e = lib.areas(4);
  e.add(0, area(1, 0, 10, { lifetime: 100 }));
  e.add(1, area(1, 100, 10, { lifetime: 300, data: 2 }));
  assert.equal(e.expire(99), 0);
  assert.equal(e.expire(100), 1);
  assert.equal(e.expire(300), 0);
  assert.equal(e.expire(301), 1);
  const w = lib.areas(1);
  w.add(4294967285, area(1, 0, 10));
  assert.equal(w.expire(243), 0);
  assert.equal(w.expire(245), 1);
  // blocks and cost
  const b = lib.areas(4);
  b.add(0, area(1, 0, 100, { blocking: true, strength: 1000 }));
  const out = [1000, 0, 0];
  assert.equal(b.blocks([10, 0, 0], out, [2000, 0, 0]), true);
  assert.equal(b.blocks([10, 0, 0], [20, 0, 0], out), false); // start inside
  assert.equal(b.blocks([10, 0, 0], out, [20, 0, 0]), false); // goal inside
  assert.equal(b.blocks([500, 0, 0], out, [2000, 0, 0]), false);
  const c = lib.areas(4);
  c.add(0, area(1, 0, 100, { strength: 7 }));
  c.add(0, area(2, 0, 100, { strength: 5 }));
  c.add(0, area(3, 0, 100, { strength: 1000, blocking: true }));
  assert.equal(c.cost([10, 0, 0], out, [2000, 0, 0]), 12);
  assert.equal(c.cost([500, 0, 0], out, [2000, 0, 0]), 0);
  assert.equal(c.cost([10, 0, 0], [20, 0, 0], out), 0);
  const s = lib.areas(2);
  s.add(0, area(1, 0, 100, { strength: 4294967295 }));
  s.add(0, area(2, 0, 100, { strength: 9 }));
  assert.equal(s.cost([10, 0, 0], out, [2000, 0, 0]), 4294967295); // saturates, still unsigned
  // radius 0 holds nothing; extreme coordinates do not overflow
  const r = lib.areas(1);
  r.add(0, area(1, 0, 0));
  assert.equal(r.contains([0, 0, 0]), null);
  const x = lib.areas(2);
  x.add(0, { kind: 1, pos: [-2147483648, -2147483648, 0], radius: 2147483647, lifetime: 1, blocking: true });
  assert.equal(x.contains([-2147483648, -2147483648, 0]), 0);
  assert.equal(x.contains([2147483647, 2147483647, 0]), null);
  assert.equal(x.blocks([-2147483648, -2147483648, 0], [2147483647, 2147483647, 0], [2147483647, 2147483647, 0]), true);
});

test('timers: duration, renew, replace least left, wrap', () => {
  assert.throws(() => lib.timers(0), RangeError);
  const t = lib.timers(2);
  t.set(1, 0, 170);
  assert.equal(t.active(1, 169), true);
  assert.equal(t.active(1, 170), false);
  assert.equal(t.left(1, 100), 70);
  assert.equal(t.left(1, 170), 0);
  assert.equal(t.left(9, 0), 0);
  assert.equal(t.active(9, 0), false);
  t.set(1, 100, 50);
  assert.equal(t.left(1, 100), 50);
  assert.equal(t.count(), 1);
  t.set(2, 100, 10);
  t.set(3, 105, 20); // full: id 2 has least left (5)
  assert.deepEqual([t.active(1, 105), t.active(2, 105), t.active(3, 105)], [true, false, true]);
  t.set(4, 150, 5); // both ran out: tie, earlier slot (id 1)
  assert.deepEqual([t.active(1, 150), t.active(4, 150), t.active(3, 150)], [false, true, false]);
  assert.equal(t.left(4, 152), 3);
  t.set(5, 151, 0);
  assert.equal(t.active(5, 151), false);
  assert.equal(t.remove(5), true);
  assert.equal(t.remove(5), false);
  t.clear();
  assert.equal(t.count(), 0);
  const w = lib.timers(1);
  w.set(1, 4294967285, 30);
  assert.equal(w.active(1, 18), true);
  assert.equal(w.active(1, 19), false);
  assert.equal(w.left(1, 9), 10);
  w.set(2, 0, 4294967295);
  assert.equal(w.left(2, 0), 4294967295); // unsigned, not -1
});

test('progress: update, stalled, extremes, wrap', () => {
  const p = new Progress();
  assert.equal(p.update(0, 5, 0), false);
  assert.equal(p.stalled(1000000, 0), false); // not started
  p.start(1000, 0);
  assert.equal(p.update(900, 50, 0x80), false);
  assert.deepEqual([p.best, p.since], [1000, 0]);
  assert.equal(p.update(872, 60, 0x80), true);
  assert.deepEqual([p.best, p.since], [872, 60]);
  assert.equal(p.stalled(144, 85), false);
  assert.equal(p.stalled(145, 85), true);
  assert.deepEqual([p.best, p.since], [872, 60]); // stalled is pure
  p.reset();
  assert.deepEqual([p.best, p.since, p.started], [0, 0, false]);
  const a = new Progress();
  a.start(2147483647, 0);
  assert.equal(a.update(-2147483648, 5, 2147483647), true); // gain 2^32 - 1, no overflow
  assert.equal(a.best, -2147483648);
  const q = new Progress();
  q.start(-2147483648, 0);
  assert.equal(q.update(2147483647, 10, -2147483648), false); // gain -(2^32 - 1)
  assert.equal(q.best, -2147483648);
  assert.equal(q.stalled(10, 10), true);
  const w = new Progress();
  w.start(1000, 4294967285);
  assert.equal(w.stalled(73, 85), false);
  assert.equal(w.stalled(74, 85), true);
  assert.equal(w.update(0, 80, 1) && w.since === 80, true);
});

test('interval: never started is due, wrap', () => {
  const i = new Interval();
  assert.equal(i.due(0, 255) && i.due(12345, 255), true);
  i.mark(0);
  assert.equal(i.due(254, 255), false);
  assert.equal(i.due(255, 255), true);
  i.reset();
  assert.equal(i.due(5, 10), true);
  i.mark(4294967290);
  assert.equal(i.due(3, 10), false); // age 9
  assert.equal(i.due(4, 10), true); // age 10
});

const alertCfg = (decay, hold, hysteresis) => ({ max: 100, decayPerTick: decay, hold, hysteresis, thresholds: [20, 50, 80] });

test('alertness: levels, jumps, hold then decay, hysteresis, wrap', () => {
  let cfg = alertCfg(2, 5, 5);
  let a = new Alertness();
  a.stimulus(10, 0, cfg);
  assert.equal(a.update(0, cfg), null);
  a.stimulus(10, 0, cfg);
  assert.deepEqual(a.update(0, cfg), [0, 1]);
  a.stimulus(30, 0, cfg);
  assert.equal(a.value, 50);
  assert.deepEqual(a.update(0, cfg), [1, 2]);
  a.stimulus(500, 0, cfg);
  assert.equal(a.value, 100);
  assert.deepEqual(a.update(0, cfg), [2, 3]);
  assert.equal(a.level, 3);
  a = new Alertness();
  a.stimulus(90, 0, cfg);
  assert.deepEqual(a.update(0, cfg), [0, 3]); // jumps straight
  assert.deepEqual(a.update(1000, cfg), [3, 0]);
  assert.equal(a.value, 0);
  // hold lasts to tick 15, then 2 per tick
  a = new Alertness();
  a.stimulus(50, 10, cfg);
  assert.deepEqual(a.update(10, cfg), [0, 2]);
  assert.equal(a.update(15, cfg), null);
  assert.equal(a.value, 50);
  assert.deepEqual(a.update(20, cfg), [2, 1]);
  assert.equal(a.value, 40);
  const b = new Alertness();
  b.stimulus(100, 10, cfg);
  b.update(10, cfg);
  b.update(40, cfg);
  assert.equal(b.value, 100 - 2 * 25); // a late update only decays the part after the hold
  // hysteresis
  cfg = alertCfg(1, 0, 5);
  a = new Alertness();
  a.stimulus(50, 0, cfg);
  assert.deepEqual(a.update(0, cfg), [0, 2]);
  assert.equal(a.update(4, cfg), null); // 46
  assert.equal(a.update(5, cfg), null); // 45, not below 45
  assert.deepEqual(a.update(6, cfg), [2, 1]); // 44
  a.stimulus(5, 6, cfg);
  assert.equal(a.value, 49);
  assert.equal(a.update(6, cfg), null); // rising needs the plain threshold
  assert.equal(a.level, 1);
  a.stimulus(1, 6, cfg);
  assert.deepEqual(a.update(6, cfg), [1, 2]);
  // wrapped clock: 16 ticks across the wrap, 11 after the hold: 110 > 100
  cfg = alertCfg(10, 5, 5);
  a = new Alertness();
  a.stimulus(100, 4294967290, cfg);
  assert.deepEqual(a.update(4294967290, cfg), [0, 3]);
  assert.deepEqual(a.update(10, cfg), [3, 0]);
  assert.equal(a.value, 0);
  // stimulus applies the owed decay first (50 - 100 saturates to 0, then +10)
  cfg = alertCfg(1, 0, 0);
  a = new Alertness();
  a.stimulus(50, 0, cfg);
  a.update(0, cfg);
  a.stimulus(10, 100, cfg);
  assert.equal(a.level, 2); // the level is left to update
  assert.deepEqual(a.update(100, cfg), [2, 0]);
  assert.equal(a.value, 10);
  // a lowered count cannot leave the level stuck
  cfg = alertCfg(0, 0, 0);
  a = new Alertness();
  a.stimulus(90, 0, cfg);
  assert.deepEqual(a.update(0, cfg), [0, 3]);
  assert.deepEqual(a.update(1, { ...cfg, thresholds: [20] }), [3, 1]);
  assert.deepEqual(a.update(2, { ...cfg, thresholds: [] }), [1, 0]);
  // thresholds: at most 8 (Rust clamps a larger count, JS refuses it)
  const big = { max: 100, decayPerTick: 1, hold: 0, hysteresis: 0, thresholds: [1, 2, 3, 4, 5, 6, 7, 8] };
  a = new Alertness();
  a.stimulus(100, 0, big);
  assert.deepEqual(a.update(0, big), [0, 8]);
  assert.throws(() => a.update(0, { ...big, thresholds: [1, 2, 3, 4, 5, 6, 7, 8, 9] }), RangeError);
  // saturation and huge decay products stay exact
  a = new Alertness();
  a.stimulus(4294967295, 0, { ...big, max: 4294967295 });
  assert.equal(a.value, 4294967295);
  a.stimulus(5, 0, { ...big, max: 4294967295 });
  assert.equal(a.value, 4294967295);
  a.update(4294967295, { max: 4294967295, decayPerTick: 4294967295, hold: 0, hysteresis: 0, thresholds: [] });
  assert.equal(a.value, 0);
});

test('sine: fine quarter points and symmetry, coarse reference', () => {
  const f = (h) => lib.sine(h, 'fine');
  assert.deepEqual([0, 64, 128, 192, 256, -64, 32, 1].map(f), [0, 65536, 0, -65536, 0, -65536, 46341, 1608]);
  assert.equal(lib.sine(64), 65536); // fine is the default
  for (let h = 0; h < 256; h++) {
    assert.equal(f(h), 0 - f(h + 128), `h ${h}`);
    assert.equal(f(h), f(128 - h), `h ${h}`);
    assert.equal(f(h + 64), f(64 - h), `h ${h}`);
  }
  const want = [
    -226, -198, -162, -142, -98, -50, 0, 25, 74, 121, 162, 181, 213, 237, 251, 255, 255, 245,
    226, 213, 181, 142, 98, 74, 25, -25, -74, -98, -142, -181, -213, -226, -245, -255, -255,
    -251, -237, -213, -181, -162, -121, -74, -25, 0, 50, 98, 142, 162, 198, 226, 245, 251, 256,
    251, 237, 226, 198, 162, 121, 98, 50, 0, -50, -74, -121, -162, -198, -213, -237, -251,
    -256, -255, -245, -226, -198, -181, -142, -98, -50, -25, 25, 74, 121, 142, 181, 213,
  ];
  const got = [];
  for (let h = -300; h <= 300; h += 7) got.push(lib.sine(h, 'coarse'));
  assert.deepEqual(got, want);
  assert.throws(() => lib.sine(0, 'rough'), RangeError);
});

// The reference numbers were made on a 0..32767 map.
const COARSE = { sine: 'coarse', min: [0, 0], max: [32767, 32767] };

test('pointsAround: coarse matches the reference, fine defaults', () => {
  const want = [
    [[0, 0], 0, [[0, 256], [0, 1280], [1629, 1629], [0, 2172]]],
    [[0, 0], 1, [[162, 0], [810, 0], [0, 0], [3060, 0]]],
    [[0, 0], 2, [[0, 25], [0, 125], [0, 1782], [0, 0]]],
    [[0, 0], 5, [[0, 226], [0, 1130], [666, 2205], [0, 888]]],
    [[0, 0], 17, [[0, 0], [0, 0], [0, 0], [1944, 0]]],
    [[0, 0], 255, [[0, 0], [0, 0], [0, 0], [0, 0]]],
    [[5000, 5000], 0, [[5000, 5256], [5000, 6280], [6629, 6629], [2828, 7172]]],
    [[5000, 5000], 1, [[5162, 4802], [5810, 4010], [4775, 2705], [8060, 4700]]],
    [[5000, 5000], 2, [[4745, 5025], [3725, 5125], [3542, 6782], [2624, 3056]]],
    [[5000, 5000], 5, [[4879, 5226], [4395, 6130], [5666, 7205], [2060, 5888]]],
    [[5000, 5000], 17, [[4975, 4745], [4875, 3725], [3218, 3542], [6944, 2624]]],
    [[5000, 5000], 255, [[4819, 4819], [4095, 4095], [2696, 5000], [5000, 1928]]],
    [[32700, 100], 0, [[32700, 356], [32700, 1380], [32767, 1729], [30528, 2272]]],
    [[32700, 100], 1, [[32767, 0], [32767, 0], [32475, 0], [32767, 0]]],
    [[32700, 100], 2, [[32445, 125], [31425, 225], [31242, 1882], [30324, 0]]],
    [[32700, 100], 5, [[32579, 326], [32095, 1230], [32767, 2305], [29760, 988]]],
    [[32700, 100], 17, [[32675, 0], [32575, 0], [30918, 0], [32767, 0]]],
    [[32700, 100], 255, [[32519, 0], [31795, 0], [30396, 100], [32700, 0]]],
  ];
  for (const [c, agent, pts] of want) {
    pts.forEach(([x, y], step) => {
      assert.deepEqual(lib.pointsAround(agent, step, [c[0], c[1], 7], COARSE), [x, y, 7], `agent ${agent} step ${step} centre ${c}`);
    });
  }
  assert.equal(lib.pointsAround(0, 4, [0, 0, 0], COARSE), null);
  assert.equal(lib.pointsAround(3, 4, [0, 0, 0]), null);
  // the default pattern is not clamped (generic i32 box), with or without a pattern object
  assert.deepEqual(lib.pointsAround(0, 1, [-5000, -5000, 0]), [-5000, -3720, 0]);
  assert.deepEqual(lib.pointsAround(0, 1, [-5000, -5000, 0], {}), [-5000, -3720, 0]);
  assert.equal(lib.pointsAround(3, 4294967295, [0, 0, 0]), null);
  // fine is the default, with or without a pattern
  assert.deepEqual(lib.pointsAround(0, 0, [0, 0, 7]), [0, 256, 7]);
  const c = [5000, 5000, 7];
  assert.deepEqual(lib.pointsAround(1, 0, c), [5171, 4811, 7]);
  assert.deepEqual(lib.pointsAround(1, 1, c), [5859, 4052, 7]);
  assert.deepEqual(lib.pointsAround(1, 3, c), [8068, 4850, 7]);
  assert.equal(lib.pointsAround(1, 4, c), null);
  assert.deepEqual(lib.pointsAround(1, 3, c, {}), [8068, 4850, 7]);
  // clamps, own pattern, missing distance counts as 0, too many steps
  const p = { min: [100, 200], max: [1000, 300], sine: 'coarse' };
  assert.deepEqual(lib.pointsAround(0, 1, [500, 250, 0], p), [500, 300, 0]);
  assert.deepEqual(lib.pointsAround(255, 3, [50, 250, 0], p), [100, 200, 0]);
  const own = { headingStep: 0, firstDistance: 256, offsets: [0], distances: [512], min: [0, 0], max: [32767, 32767] };
  assert.deepEqual(lib.pointsAround(0, 0, [1000, 1000, 0], own), [1000, 1256, 0]);
  assert.deepEqual(lib.pointsAround(0, 1, [1000, 1000, 0], own), [1000, 1512, 0]);
  assert.equal(lib.pointsAround(0, 2, [1000, 1000, 0], own), null);
  const short = { ...own, offsets: [0, 0], distances: [256] };
  assert.deepEqual(lib.pointsAround(0, 2, [500, 600, 7], short), [500, 600, 7]);
  assert.deepEqual(lib.pointsAround(0, 1, [1000, 1000, 0], { ...own, offsets: [0, 0], distances: [] }), [1000, 1000, 0]);
  assert.throws(() => lib.pointsAround(0, 0, [0, 0, 0], { offsets: new Array(9).fill(0), distances: new Array(9).fill(0) }), RangeError);
  assert.throws(() => lib.pointsAround(0, 0, [0, 0, 0], { sine: 'rough' }), RangeError);
});

test('sweepHeading matches the reference', () => {
  const want = [10, 10, 42, 42, 74, 74, 42, 42, 10, 10, 234, 234, 202, 202, 234, 234, 10, 10, 10];
  const got = [];
  for (let e = 0; e <= 180; e += 10) got.push(lib.sweepHeading(10, e, 20));
  assert.deepEqual(got, want);
  assert.equal(lib.sweepHeading(10, 25, 10, [0, 5, 7]), 17);
  assert.equal(lib.sweepHeading(10, 35, 10, [0, 5, 7]), 10);
  assert.equal(lib.sweepHeading(250, 0, 10, [10]), 4);
  assert.equal(lib.sweepHeading(10, 1, 0, [1, 2]), 12); // step 0 counts as 1
  assert.equal(lib.sweepHeading(10, 0, 10, []), 10);
});

const cand = (list) => list.map(([id, x, y]) => ({ id, x, y }));

test('route: nearest with truncating division, ties, extremes', () => {
  const e = 'euclid';
  // divisor 16: dx -15 -> 0, dx -17 -> -1
  assert.deepEqual(lib.route({ nearest: 2 }, [0, 0], cand([[1, -17, 0], [2, -15, 0]]), e, 16), [2, 1]);
  assert.deepEqual(lib.route({ nearest: 2 }, [0, 0], cand([[1, 5, 0], [2, 0, 5], [3, 1, 0]]), e, 1), [3, 1]);
  assert.deepEqual(lib.route({ nearest: 3 }, [0, 0], cand([[1, 5, 0]]), e, 0), [1]); // divisor 0 counts as 1
  assert.deepEqual(lib.route({ nearest: 3 }, [0, 0], [], e, 1), []);
  assert.deepEqual(lib.route({ nearest: 0 }, [0, 0], cand([[1, 5, 0]]), e, 1), []);
  const c = cand([[1, -40, 3], [2, -31, -31], [3, 20, 0], [4, -100, -17]]);
  assert.deepEqual(lib.route({ nearest: 3 }, [10, 5], c, e, 16), [3, 2, 1]);
  const x = cand([[1, -2147483648, -2147483648], [2, 2147483647, 2147483647], [3, 0, 0]]);
  assert.deepEqual(lib.route({ nearest: 3 }, [2147483647, 2147483647], x, e, 1), [2, 3, 1]);
  assert.deepEqual(lib.route({ nearest: 3 }, [-2147483648, -2147483648], x, e, 1), [1, 3, 2]);
  // other metrics and a three-way tie
  const m = cand([[1, 4, 4], [2, 0, 5], [3, 3, 0], [4, -2, -2]]);
  assert.deepEqual(lib.route({ nearest: 3 }, [0, 0], m, 'manhattan'), [3, 4, 2]);
  assert.deepEqual(lib.route({ nearest: 3 }, [0, 0], m, 'chebyshev'), [4, 3, 1]);
  assert.deepEqual(lib.route({ nearest: 3 }, [0, 0], m, 'hex'), [3, 4, 2]);
  assert.deepEqual(lib.route({ nearest: 2 }, [0, 0], cand([[7, 5, 0], [8, 0, 5], [9, -5, 0]])), [7, 8]); // metric defaults to euclid
  // ids above 2^31 come back unsigned
  assert.deepEqual(lib.route({ nearest: 1 }, [0, 0], cand([[4294967295, 1, 1]])), [4294967295]);
});

test('route: in range is inclusive per metric, in candidate order', () => {
  const c = cand([[1, 3, 4], [2, 5, 0], [3, 4, 4], [4, -3, -3], [5, 0, 6]]);
  assert.deepEqual(lib.route({ inRange: 5 }, [0, 0], c, 'euclid'), [1, 2, 4]);
  assert.deepEqual(lib.route({ inRange: 7 }, [0, 0], c, 'manhattan'), [1, 2, 4, 5]);
  assert.deepEqual(lib.route({ inRange: 4 }, [0, 0], c, 'chebyshev'), [1, 3, 4]);
  assert.deepEqual(lib.route({ inRange: 4 }, [0, 0], c, 'hex'), []);
  assert.deepEqual(lib.route({ inRange: 5 }, [0, 0], c, 'hex'), [2]);
  assert.deepEqual(lib.route({ inRange: -1 }, [0, 0], cand([[1, 0, 0]])), []);
  assert.deepEqual(lib.route({ inRange: 0 }, [0, 0], cand([[1, 0, 0], [2, 1, 0]])), [1]);
  assert.deepEqual(lib.route({ inRange: 1 }, [0, 0], cand([[1, 15, 0], [2, 16, 0]]), 'manhattan', 16), [1, 2]);
  assert.deepEqual(lib.route({ inRange: 1 }, [0, 0], cand([[1, 31, 0], [2, 32, 0]]), 'manhattan', 16), [1]);
  // a radius beyond 2^32 and an euclid square beyond 2^64 still compare exactly
  const far = cand([[1, 2147483647, 2147483647], [2, -2147483648, 0]]);
  assert.deepEqual(lib.route({ inRange: 4294967295 }, [-2147483648, -2147483648], far, 'euclid'), [2]);
  assert.deepEqual(lib.route({ inRange: 9007199254740991 }, [0, 0], far, 'euclid'), [1, 2]);
});

test('route: all, to, bad rules, hex distance', () => {
  const c = cand([[7, 0, 0], [3, 9, 9], [5, -2, 1]]);
  assert.deepEqual(lib.route('all', [0, 0], c), [7, 3, 5]);
  assert.deepEqual(lib.route({ to: 3 }, [0, 0], c, 'hex'), [3]);
  assert.deepEqual(lib.route({ to: 4 }, [0, 0], c, 'hex'), []);
  for (const bad of ['everyone', {}, null, { nearest: -1 }, { nearest: 1.5 }, { nearest: 4294967296 }, { inRange: 1.5 }, { inRange: 2 ** 60 }, { to: -1 }]) {
    assert.throws(() => lib.route(bad, [0, 0], c), RangeError, JSON.stringify(bad));
  }
  assert.throws(() => lib.route('all', [0, 0], c, 'taxicab'), RangeError);
  // hex axial: (1,-1) is a neighbour, (1,1) is two away, (3,-5) is five away
  const one = (q, r) => lib.route({ inRange: 1 }, [0, 0], cand([[1, q, r]]), 'hex').length === 1;
  assert.deepEqual([one(1, 0), one(0, -1), one(1, -1), one(-1, 1), one(1, 1), one(3, -5)], [true, true, true, true, false, false]);
  assert.deepEqual(lib.route({ inRange: 5 }, [0, 0], cand([[1, 3, -5]]), 'hex'), [1]);
  assert.deepEqual(lib.route({ inRange: 4 }, [0, 0], cand([[1, 3, -5]]), 'hex'), []);
});

test('stealth guard slice: noise, alertness, shout with delay, told', () => {
  // The first rounds of crates/sagax-decide/tests/stealth_guard.rs. Guards on
  // hex cells (q, r): 0 at (0, 0), 1 at (4, -6), 2 at (-2, 0). Hex distance
  // (|dq| + |dr| + |dq + dr|) / 2. Alertness: thresholds 20/50/80,
  // hysteresis 5, hold 3, then 10 per tick.
  const cfg = { max: 100, decayPerTick: 10, hold: 3, hysteresis: 5, thresholds: [20, 50, 80] };
  const [NOISE, SEEN, PLAYER] = [1, 2, 99];
  const at = [[0, 0], [4, -6], [-2, 0]];
  const all = at.map(([x, y], id) => ({ id, x, y }));
  const known = [lib.knowledge(4), lib.knowledge(4), lib.knowledge(4)];
  const alert = [new Alertness(), new Alertness(), new Alertness()];
  const inbox = [lib.inbox(4), lib.inbox(4), lib.inbox(4)];

  // Round 2: noise at (5, 0), range 5. Guard 0 is 5 away; guards 1 and 2 are 7 away.
  const heard = lib.route({ inRange: 5 }, [5, 0], all, 'hex');
  assert.deepEqual(heard, [0]);
  for (const g of heard) {
    known[g].learn(2, { subject: PLAYER, kind: NOISE, pos: [5, 0, 0], seenAt: 2, source: 1 });
    alert[g].stimulus(30, 2, cfg);
  }
  assert.deepEqual(alert[0].update(2, cfg), [0, 1]); // suspicious
  assert.equal(known[0].get(PLAYER, NOISE).source, 1);
  assert.equal(known[1].count(), 0);
  // Rounds 3..5: nothing changes (hold ends after round 5).
  for (let t = 3; t <= 5; t++) assert.equal(alert[0].update(t, cfg), null);
  assert.equal(alert[0].value, 30);

  // Round 6: guard 0 at (4, -1) sees the player: +100 (capped) -> alert.
  alert[0].stimulus(100, 6, cfg);
  assert.deepEqual(alert[0].update(6, cfg), [1, 3]);
  assert.equal(alert[0].value, 100);
  // It shouts: guard 1 is 5 away, guard 2 is 6 away, both within 6; one round per cell.
  const shout = { kind: SEEN, subject: PLAYER, pos: [5, 0, 0], seenAt: 6, strength: 0, data: 0, sender: 0, keepIfSenderGone: false };
  const others = all.filter((c) => c.id !== 0);
  const ids = lib.route({ inRange: 6 }, [4, -1], others, 'hex');
  assert.deepEqual(ids, [1, 2]);
  const hex = (g) => {
    const [dq, dr] = [4 - at[g][0], -1 - at[g][1]];
    return (Math.abs(dq) + Math.abs(dr) + Math.abs(dq + dr)) / 2;
  };
  const arrivals = ids.map((g) => lib.arrival(6, 0, hex(g), 1));
  assert.deepEqual(arrivals, [11, 12]);
  ids.forEach((g, i) => inbox[g].push(shout, arrivals[i]));

  // Round 11: guard 1 hears it: told the place, +60 -> searching.
  assert.deepEqual(inbox[1].takeDue(10), []);
  const [told] = inbox[1].takeDue(11);
  known[1].learnMessage(11, told, 2);
  alert[1].stimulus(60, 11, cfg);
  assert.deepEqual(known[1].get(PLAYER, SEEN), { subject: PLAYER, kind: SEEN, pos: [5, 0, 0], seenAt: 6, source: 2, strength: 0, data: 0 });
  assert.deepEqual(alert[1].update(11, cfg), [0, 2]);
  // Value at round t >= 11 + 3 is 60 - 10 * (t - 14): 50 at 15 (still >= 45), 40 at 16 -> suspicious.
  for (let t = 12; t <= 14; t++) assert.equal(alert[1].update(t, cfg), null);
  assert.equal(alert[1].update(15, cfg), null);
  assert.equal(alert[1].value, 50);
  assert.deepEqual(alert[1].update(16, cfg), [2, 1]);
  assert.equal(alert[1].value, 40);
  // Round 12: guard 2 hears it.
  assert.equal(inbox[2].takeDue(11).length, 0);
  assert.equal(inbox[2].takeDue(12).length, 1);
});

test('route and points survive memory growth', () => {
  // 200000 candidates need a 2.4 MB scratch buffer: the module grows its
  // memory while the buffer is asked for and while the call allocates.
  const c = [];
  for (let i = 0; i < 200000; i++) c.push({ id: i, x: i, y: 0 });
  assert.deepEqual(lib.route({ nearest: 3 }, [150000, 0], c), [150000, 149999, 150001]);
  assert.deepEqual(lib.route({ nearest: 1000000 }, [0, 0], c.slice(0, 2)), [0, 1]);
  assert.equal(lib.route({ inRange: 100 }, [0, 0], c, 'chebyshev').length, 101);
  assert.equal(lib.route('all', [0, 0], c).length, 200000);
  // tables keep working after the memory moved
  const k = lib.knowledge(2);
  k.learn(5, fact(1, 1, 5, { pos: [1, 2, 3] }));
  assert.deepEqual(k.get(1, 1).pos, [1, 2, 3]);
  const big = lib.knowledge(20000);
  for (let i = 0; i < 20000; i++) big.learn(i, fact(i, 0, i));
  assert.equal(big.expire(20000, 10000).length, 10001);
  big.free();
});

test('capacity is validated', () => {
  for (const bad of [0, -1, 1.5, 2 ** 31, NaN, '3', undefined, Infinity]) {
    for (const make of ['knowledge', 'outbox', 'inbox', 'areas', 'timers']) {
      assert.throws(() => lib[make](bad), RangeError, `${make}(${String(bad)})`);
    }
  }
});

test('records and lists are copies; a freed table cannot be used', () => {
  const o = lib.outbox(2);
  const m = msg(1);
  o.post(0, m, 0);
  const [out] = o.due(0);
  out.pos[0] = 99;
  m.pos[1] = 77; // the caller's own message is not kept either
  o.post(1, msg(1), 0);
  assert.deepEqual(o.due(1)[0].pos, [1, 2, 3]);
  const k = lib.knowledge(2);
  k.learn(0, fact(1, 1, 0, { pos: [4, 5, 6] }));
  const f = k.get(1, 1);
  f.pos[0] = 99;
  k.list()[0].pos[1] = 99;
  assert.deepEqual(k.get(1, 1).pos, [4, 5, 6]);
  for (const t of [o, k, lib.inbox(1), lib.areas(1), lib.timers(1)]) {
    t.free();
    t.free(); // twice is fine
  }
  assert.throws(() => o.pending(), /free/);
  assert.throws(() => k.learn(0, fact(1, 1, 0)), /free/);
});

// ---- group tactics ------------------------------------------------------------

const ENEMY = 1;
const enemy = (subject, x, y, seenAt = 0) => ({ subject, kind: ENEMY, pos: [x, y, 0], seenAt });

/** A squad of members [id, x, y, roles?] (the first leads) that sees enemies of kind ENEMY; the others hear by radio (2 ticks plus 1 per unit). */
function squadOf(metric, members, { radio = true } = {}) {
  const sq = lib.squad({ members: 8, memory: 8, areas: 4, inbox: 8, dangers: 8, metric });
  members.forEach(([id, x, y, roles = 0], i) => assert.equal(sq.addMember({ id, pos: [x, y, 0], roles }), i));
  assert.equal(sq.setLeader(members[0][0]), true);
  if (radio) for (const [id] of members.slice(1)) assert.equal(sq.setRadio(id, 2, 1), true);
  sq.setTargetKind(ENEMY);
  return sq;
}

test('gather: nearest free of the faction, ties to the lower id, validated', () => {
  const near = (id, faction, x, y, free = true) => ({ id, faction, pos: [x, y, 0], free });
  const around = [near(7, 1, 30, 0), near(4, 1, -3, 0), near(3, 1, 0, -2), near(6, 1, 1, 1, false), near(2, 1, 0, 2), near(9, 2, 1, 0)];
  assert.deepEqual(lib.gather([0, 0], 1, 10, around, { metric: 'chebyshev', freeOnly: true, max: 3 }), [2, 3, 4]);
  assert.deepEqual(lib.gather([0, 0], 1, 10, around, { metric: 'chebyshev' }), [6, 2, 3, 4]);
  assert.deepEqual(lib.gather([0, 0], 1, 2, around, { metric: 'euclid' }), [6, 2, 3]); // squared: 2 <= 4, 4 <= 4
  assert.deepEqual(lib.gather([0, 0], 1, -1, around), []);
  assert.deepEqual(lib.gather([0, 0], 1, 10, []), []);
  assert.throws(() => lib.gather([0, 0], 1, 10, around, { metric: 'taxi' }), RangeError);
  assert.throws(() => lib.gather([0, 0], 1, 1.5, around), RangeError);
  assert.throws(() => lib.gather([0, 0], 1, 10, [{ id: -1, faction: 1, pos: [0, 0, 0], free: true }]), RangeError);
});

test('tactics: shipped ones and the custom builder, taken over by a squad', () => {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 1, 0]]);
  const t = lib.tactics;
  const all = [t.rush(), t.formation('line', 2), t.formation('wedge', 2), t.formation('column', 2), t.flank(2, 4), t.flank(3, 4), t.ambush(), t.focus(), t.isolate(), t.fallBack()];
  all.forEach((tactic, i) => assert.equal(sq.addTactic(tactic), i));
  assert.throws(() => sq.addTactic(all[0]), /already given to a squad/, 'a tactic is taken over once');
  assert.throws(() => t.formation('ring', 2), RangeError);
  // custom: one slot that holds a tagged spot, scored by distance, ends on losses
  const c = lib.tactic({ weight: 0.5, minSkill: 0, needsTarget: true });
  assert.equal(c.addSlot({ min: 1, max: 2, place: { kind: 'tagged', tags: SQUAD.tagCover }, kind: 'hold', start: { kind: 'after', ticks: 3 } }), 0);
  assert.equal(c.addSlot({ place: { kind: 'side', side: 'rear' }, gather: { kind: 'offset', pos: [1, 2, 0] }, kind: 'custom', custom: 77, start: { kind: 'signal' }, dist: 3 }), 1);
  c.addConsideration({ input: SQUAD.inTargetDist, curve: 'inverse', min: 0, max: 50 });
  c.addAbort(SQUAD.inLosses, 1);
  c.triggerTargetInRadius(6);
  c.triggerAllArrived(100);
  c.triggerManual();
  c.triggerNone();
  assert.throws(() => c.addSlot({ place: { kind: 'none' } }), RangeError);
  assert.throws(() => c.addSlot({ place: { kind: 'side', side: 'up' } }), RangeError);
  assert.throws(() => c.addSlot({ place: { kind: 'here' }, kind: 'dance' }), RangeError);
  assert.throws(() => c.addSlot({ place: { kind: 'here' }, start: { kind: 'later' } }), RangeError);
  assert.throws(() => c.addSlot({ place: { kind: 'here' }, max: 256 }), RangeError);
  assert.throws(() => c.addConsideration({ input: 0, curve: 'wobbly' }), RangeError);
  assert.throws(() => c.triggerAllArrived(-1), RangeError);
  assert.throws(() => c.triggerTargetInRadius(1.5), RangeError);
  assert.throws(() => lib.tactic({ minSkill: 256 }), RangeError);
  assert.equal(sq.addTactic(c), 10);
  c.free(); // already taken over: nothing to free
  sq.free();
  sq.free(); // twice is fine
  assert.throws(() => sq.skill(), /free/);
  const lone = lib.tactics.rush();
  lone.free(); // a tactic never given to a squad
});

test('squad: members, leader, skill, skill table, channels, validated', () => {
  for (const caps of [{ members: 0 }, { memory: 0 }, { areas: 0 }, { inbox: 0 }, { dangers: 0 }]) {
    assert.throws(() => lib.squad({ members: 2, memory: 2, areas: 2, inbox: 2, dangers: 2, ...caps }), RangeError);
  }
  assert.throws(() => lib.squad({ members: 2, memory: 2, areas: 2, inbox: 2, dangers: 2, metric: 'taxi' }), RangeError);
  const sq = lib.squad({ members: 3, memory: 4, areas: 4, inbox: 4, dangers: 4, metric: 'chebyshev' });
  assert.equal(sq.leader(), null);
  assert.equal(sq.addMember({ id: 1, pos: [0, 0, 0] }), 0);
  assert.equal(sq.addMember({ id: 1, pos: [5, 5, 0] }), null, 'same id');
  assert.equal(sq.addMember({ id: 2, pos: [1, 0, 0], roles: SQUAD.roleRanged, skill: 200, state: 'suspended' }), 1);
  assert.equal(sq.addMember({ id: 3, pos: [2, 0, 0] }), 2);
  assert.equal(sq.addMember({ id: 4, pos: [3, 0, 0] }), null, 'full');
  assert.throws(() => sq.addMember({ id: 5, pos: [0, 0, 0], state: 'sleeping' }), RangeError);
  assert.throws(() => sq.addMember({ id: 5, pos: [0, 0, 0], skill: 256 }), RangeError);
  assert.throws(() => sq.addMember({ id: 5, pos: [2 ** 31, 0, 0] }), RangeError);
  assert.deepEqual(sq.members()[1], { id: 2, pos: [1, 0, 0], roles: 1, skill: 200, state: 'suspended' });
  assert.equal(sq.leader(), 1, 'the first active member');
  assert.equal(sq.setLeader(3), true);
  assert.equal(sq.setLeader(9), false);
  assert.equal(sq.leader(), 3);
  assert.equal(sq.setPos(3, [7, -8, 1]), true);
  assert.equal(sq.setPos(9, [0, 0, 0]), false);
  assert.deepEqual(sq.members()[2].pos, [7, -8, 1]);
  assert.equal(sq.reinstate(2, 0), true);
  assert.equal(sq.reinstate(9, 0), false);
  assert.equal(sq.resume(2, 0), true, 'the old name still works');
  assert.equal(sq.suspend(3), true);
  assert.equal(sq.leader(), 1, 'a suspended leader does not act');
  assert.equal(sq.setGone(1), true);
  assert.equal(sq.members()[0].state, 'gone');
  assert.equal(sq.leader(), 2);
  assert.equal(sq.removeMember(1), true);
  assert.equal(sq.removeMember(1), false);
  assert.deepEqual(sq.members().map((m) => m.id), [2, 3]);
  assert.equal(sq.suspend(9), false);
  // skill and its table
  assert.equal(sq.skill(), 0);
  sq.setSkill(255);
  assert.equal(sq.skill(), 255);
  assert.throws(() => sq.setSkill(256), RangeError);
  const table = sq.skillTable();
  assert.deepEqual(table, lib.skillTable());
  assert.deepEqual(table.pick, ['weighted', 'weighted', 'weighted', 'best']);
  assert.deepEqual(table.pickTop, [3, 2, 2, 0]);
  assert.deepEqual(table.thinkTicks, [34, 20, 10, 5]);
  assert.equal(lib.discipline(200), table.holdBonus[3]);
  sq.setSkillTable({ ...table, thinkTicks: [1, 2, 3, 4], pick: ['uniform', 'best', 'best', 'best'] });
  assert.deepEqual(sq.skillTable().thinkTicks, [1, 2, 3, 4]);
  assert.equal(sq.skillTable().pick[0], 'uniform');
  assert.equal(lib.discipline(0, { ...table, holdBonus: [9, 9, 9, 9] }), 9);
  assert.throws(() => sq.setSkillTable({ ...table, pick: ['lucky', 'best', 'best', 'best'] }), RangeError);
  assert.throws(() => sq.setSkillTable({ ...table, spotRankError: [256, 0, 0, 0] }), RangeError);
  assert.throws(() => sq.setSkillTable({ ...table, thinkTicks: [1, 2, 3] }), RangeError);
  // channels
  assert.equal(sq.setShout(2, 5), true);
  assert.equal(sq.setSilent(2), true);
  assert.equal(sq.setSilent(9), false);
  assert.throws(() => sq.setRadio(2, -1, 0), RangeError);
  sq.setExtraColumns(2);
  assert.throws(() => sq.setExtraColumns(-1), RangeError);
  sq.free();
});

test('squad: config keys and aliases', () => {
  const d = lib.squad();
  assert.equal(d.addMember({ id: 1, pos: [0, 0, 0] }), 0);
  d.free();
  const old = lib.squad({ groupCap: 1, memoryCap: 2, areaCap: 3, inboxCap: 4 });
  assert.equal(old.addMember({ id: 1, pos: [0, 0, 0] }), 0);
  assert.equal(old.addMember({ id: 2, pos: [0, 0, 0] }), null, 'groupCap 1 is members 1');
  old.free();
  assert.throws(() => lib.squad({ members: 2, groupCap: 2 }), TypeError);
  assert.throws(() => lib.squad({ memory: 2, memoryCap: 2 }), TypeError);
  assert.throws(() => lib.squad({ areas: 2, areaCap: 2 }), TypeError);
  assert.throws(() => lib.squad({ inbox: 2, inboxCap: 2 }), TypeError);
  assert.throws(() => lib.squad({ dangers: 1.5 }), RangeError);
  const one = lib.squad({ dangers: 1 });
  assert.equal(one.reportDanger({ sender: 1, centre: [0, 0, 0] }, 0), true);
  assert.equal(one.reportDanger({ sender: 1, centre: [1, 0, 0] }, 0), false, 'a full danger queue refuses');
  one.free();
});

test('squad: typed channels and triggers', () => {
  const sq = lib.squad({ members: 4, metric: 'chebyshev' });
  sq.addMember({ id: 1, pos: [0, 0, 0] });
  sq.addMember({ id: 2, pos: [3, 0, 0] });
  sq.addMember({ id: 3, pos: [9, 0, 0] });
  sq.setLeader(1);
  assert.equal(sq.setRadio(2, 2, 1), true);
  assert.equal(sq.reportArrivesAt(2, 10), 15, 'radio 2 + 1 per cell, 3 cells');
  assert.equal(sq.reportArrivesAt(1, 10), 10, 'the leader at once');
  assert.equal(sq.setShout(3, 5), true);
  assert.equal(sq.reportArrivesAt(3, 10), null, 'beyond the shout');
  assert.equal(sq.setShout(3, 9), true);
  assert.equal(sq.reportArrivesAt(3, 10), 19);
  assert.equal(sq.setSilent(2), true);
  assert.equal(sq.reportArrivesAt(2, 10), null);
  assert.equal(sq.reportArrivesAt(99, 10), null, 'unknown id');
  assert.equal(sq.setShout(99, 5), false);
  assert.equal(sq.setRadio(99, 1, 1), false);
  assert.equal(sq.setSilent(99), false);
  assert.equal(sq.setChannel, undefined, 'setChannel is gone');
  const t = lib.tactic();
  assert.equal(t.setTrigger, undefined, 'setTrigger is gone');
  for (const f of ['triggerNone', 'triggerManual']) assert.equal(t[f](), undefined);
  t.triggerAllArrived(0);
  t.triggerTargetInRadius(-1);
  t.free();
  sq.free();
});

test('squad: danger reports', () => {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 1, 0], [3, 0, 1], [4, 1, 1]]);
  sq.addTactic(lib.tactics.fallBack());
  sq.setSkill(255);
  assert.equal(sq.setGone(3) && sq.setGone(4), true);
  assert.throws(() => sq.report({ kind: 3, subject: 0, pos: [0, 0, 0], sender: 2 }, 0), RangeError);
  assert.equal(sq.reportDanger({ sender: 2, centre: [20, 0, 0], shooter: { x: 25, y: -2, z: 0 } }, 50), true);
  assert.equal(sq.reportDanger({ sender: 3, centre: { x: 4, y: 4, level: 0 } }, 50), true);
  assert.equal(typeof sq.plan(49), 'number');
  assert.equal(sq.areas().count(), 0, 'not yet arrived');
  assert.equal(typeof sq.plan(50), 'number');
  const list = sq.areas().list();
  assert.equal(list.length, 2);
  assert.deepEqual([list[0].pos, list[0].source, list[0].data], [[20, 0, 0], [25, -2, 0], 2]);
  assert.deepEqual([list[1].pos, list[1].data], [[4, 4, 0], 3]);
  assert.throws(() => sq.reportDanger({ sender: 2, centre: [0, 0, 0], shooter: 'there' }, 0), RangeError);
  // activation is told once
  assert.equal(sq.takeActivated(), null);
  sq.ordersDue(60);
  assert.equal(sq.takeActivated(), sq.activePlan().id);
  assert.equal(sq.takeActivated(), null);
  sq.free();
});

test('squad memory: observe, report, targets, own knowledge and areas', () => {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 3, 0]]);
  const k = sq.knowledge();
  assert.equal(k, sq.knowledge(), 'the same wrapper every time');
  assert.equal(sq.observe(5, enemy(10, 20, 0, 5)), null);
  assert.deepEqual(k.get(10, ENEMY), { subject: 10, kind: ENEMY, pos: [20, 0, 0], seenAt: 5, source: 0, strength: 0, data: 0 });
  k.learn(5, { subject: 50, kind: 9, pos: [0, 0, 0], seenAt: 5 }); // not a target
  assert.equal(sq.report({ kind: SQUAD.reportEnemy, subject: 11, pos: [24, 10, 0], seenAt: 6, sender: 2 }, 8), true);
  assert.deepEqual(sq.targets().map((f) => f.subject), [10], 'the report is not read yet');
  sq.addTactic(lib.tactics.rush());
  assert.equal(typeof sq.plan(8), 'number');
  assert.deepEqual(sq.targets().map((f) => [f.subject, f.source]), [[10, 0], [11, 2]], 'told');
  // a danger report adds an area to the squad's own areas
  const a = sq.areas();
  assert.equal(a.count(), 0);
  sq.setDangerArea({ kind: 3, radius: 4, strength: 9, lifetime: 100, blocking: true });
  assert.equal(sq.dangerArea().radius, 4);
  assert.equal(sq.reportDanger({ sender: 2, centre: [5, 5, 0], shooter: [30, 1, 0] }, 9), true);
  sq.check(9);
  assert.equal(a.count(), 1);
  assert.deepEqual(a.list()[0].pos, [5, 5, 0]);
  assert.deepEqual([a.list()[0].source, a.list()[0].data], [[30, 1, 0], 2]);
  assert.equal(lib.areasMoveCost(a, [0, 0, 0], [5, 5, 0], [0, 0, 0], [20, 0, 0]), null);
  // lent memory cannot be freed on its own and dies with the squad
  k.free();
  assert.equal(k.count(), 3);
  sq.free();
  assert.throws(() => k.count(), /free/);
  assert.throws(() => a.count(), /free/);
});

test('areasMoveCost: blocked is null, else the cost of the cell moved to', () => {
  const z = lib.areas(4);
  z.add(0, { kind: 1, pos: [0, 0, 0], radius: 100, lifetime: 255, blocking: true });
  z.add(0, { kind: 2, pos: [500, 0, 0], radius: 100, lifetime: 255, strength: 7 });
  const start = [1000, 0, 0];
  const goal = [2000, 0, 0];
  assert.equal(lib.areasMoveCost(z, [900, 0, 0], [10, 0, 0], start, goal), null);
  assert.equal(lib.areasMoveCost(z, [10, 0, 0], [20, 0, 0], start, goal), null);
  assert.equal(lib.areasMoveCost(z, [900, 0, 0], [500, 0, 0], start, goal), 7);
  assert.equal(lib.areasMoveCost(z, [10, 0, 0], [800, 0, 0], start, goal), 0, 'leaving is free');
  assert.equal(lib.areasMoveCost(z, [900, 0, 0], [10, 0, 0], [20, 0, 0], goal), 0, 'start inside');
  const s = lib.areas(2);
  s.add(0, { kind: 1, pos: [0, 0, 0], radius: 100, lifetime: 255, strength: 4294967295 });
  s.add(0, { kind: 2, pos: [0, 0, 0], radius: 100, lifetime: 255, strength: 9 });
  assert.equal(lib.areasMoveCost(s, [0, 0, 0], [10, 0, 0], start, goal), 2 ** 31 - 2, 'saturates as in C');
  z.free();
  assert.throws(() => lib.areasMoveCost(z, [0, 0, 0], [0, 0, 0], start, goal), /free/);
});

test('plans: goals, orders by channel, signal lines up the start, errors', () => {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 6, 8]], { radio: false });
  assert.equal(sq.setRadio(2, 3, 2), true);
  assert.equal(sq.plan(0), null, 'no tactic');
  assert.equal(sq.addTactic(lib.tactics.rush()), 0);
  assert.equal(sq.plan(0), null, 'no target');
  assert.deepEqual(sq.check(0), { status: 'noPlan', plan: null });
  sq.setSkill(255);
  sq.observe(0, enemy(10, 20, 0));
  const id = sq.plan(0, { seed: 3 });
  assert.equal(typeof id, 'number');
  const p = sq.pendingPlan();
  assert.equal(p.id, id);
  assert.deepEqual([p.tactic, p.target, p.madeAt, p.activeAt], [0, 10, 0, 5]);
  assert.equal(p.goals.length, 2);
  const g = p.goals.find((goal) => goal.member === 1);
  assert.deepEqual(Object.keys(g).sort(), ['custom', 'issuedAt', 'kind', 'member', 'plan', 'pos', 'slot', 'start', 'startDelay', 'target', 'thenPos', 'thenUnchecked', 'unchecked'].sort());
  assert.deepEqual([g.member, g.kind, g.target, g.start.kind, g.slot, g.plan, g.thenPos], [1, 'attack', 10, 'now', 0, id, null]);
  assert.equal(sq.activePlan(), null);
  assert.deepEqual(sq.check(1), { status: 'keep', plan: null });
  assert.deepEqual(sq.ordersDue(4), []);
  assert.deepEqual(sq.ordersDue(5).map((o) => o.member), [1], 'the leader at once');
  assert.equal(sq.activePlan().id, id);
  assert.deepEqual(sq.ordersDue(27), [], 'radio: 3 + 10 * 2');
  assert.deepEqual(sq.ordersDue(28).map((o) => o.member), [2]);
  assert.equal(sq.signal(28), true);
  assert.equal(sq.signalGiven(), true);
  assert.equal(sq.signal(29), false, 'once per plan');
  assert.equal(sq.arrived(2, 29), true);
  assert.equal(sq.arrived(9, 29), false);
  // a column the rows do not have is a plan error
  const bad = lib.tactic({ weight: 1 });
  bad.addSlot({ place: { kind: 'here' } });
  bad.addConsideration({ input: SQUAD.inExtra + 1, curve: 'linear', min: 0, max: 10 });
  sq.addTactic(bad);
  assert.deepEqual(sq.validate(), { tactic: 1, input: SQUAD.inExtra + 1 });
  assert.throws(() => sq.plan(30), /tactic 1 reads column 9/);
  assert.throws(() => sq.check(30), /tactic 1 reads column 9/);
  sq.setExtraColumns(2);
  assert.equal(sq.validate(), null);
  assert.equal(typeof sq.plan(30, { extra: [0, 0, 1, 1], spots: [{ pos: [1, 1, 0], tags: 0 }], seed: 2 ** 40 }), 'number');
  // times and seeds are validated
  for (const bad of [-1, 2 ** 32, 1.5, NaN]) assert.throws(() => sq.ordersDue(bad), RangeError, String(bad));
  assert.throws(() => sq.ordersDue('5'), TypeError); // a wrong type, not a bad number (Task 11)
  assert.throws(() => sq.plan(0, { seed: -1 }), RangeError);
  assert.throws(() => sq.plan(0, { spots: [{ pos: [0, 0, 0], tags: -1 }] }), RangeError);
  assert.throws(() => sq.plan(0, { extra: [2 ** 31] }), RangeError);
  // BigInt seeds take the full 64 bits
  assert.equal(typeof sq.plan(31, { extra: [0, 0, 1, 1], seed: 0xffffffffffffffffn }), 'number');
  assert.throws(() => sq.plan(0, { seed: -1n }), RangeError);
  assert.throws(() => sq.plan(0, { seed: 2n ** 64n }), RangeError);
  // extra columns are bounded
  sq.setExtraColumns(SQUAD.maxExtraColumns);
  assert.throws(() => sq.setExtraColumns(SQUAD.maxExtraColumns + 1), RangeError);
  sq.free();
});

test('squad: role list', () => {
  const extraStart = (roles) => {
    const sq = lib.squad(roles === undefined ? {} : { roles });
    const at = sq.extraStart();
    sq.free();
    return at;
  };
  assert.equal(extraStart(), 8);
  assert.equal(extraStart(['area']), 8);
  assert.equal(extraStart([]), 8);
  assert.equal(extraStart(['ranged', 'area', 4]), 9);
  assert.equal(extraStart([1, 2, 4, 8, 16, 32, 64, 128]), 14);
  assert.throws(() => lib.squad({ roles: [1, 2, 4, 8, 16, 32, 64, 128, 256] }), RangeError);
  assert.throws(() => lib.squad({ roles: ['archer'] }), RangeError);
  assert.equal(SQUAD.inRole0, 4);
  assert.equal(SQUAD.inRole1, 5);
  // the crew takes the same list
  const crew = lib.crew({ squad: { roles: ['ranged', 'area', 4] } });
  assert.equal(crew.squad().extraStart(), 9);
  crew.free();
  assert.throws(() => lib.crew({ squad: { roles: new Array(9).fill(1) } }), RangeError);
});

/** An elite squad of four with rush and a 2-side flank, enemy 10 at (20, 0), all hearing at once; the flank is active at 5. */
function activeFlank() {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 1, 0], [3, 0, 1], [4, 1, 1]], { radio: false });
  sq.addTactic(lib.tactics.rush());
  sq.addTactic(lib.tactics.flank(2, 4));
  sq.setSkill(255);
  sq.observe(0, enemy(10, 20, 0));
  const { status, plan } = sq.check(0);
  assert.equal(status, 'replan');
  assert.deepEqual(sq.check(5), { status: 'keep', plan: null });
  return [sq, plan];
}

test('gather orders, arrival reports, resume, stale orders, spot radius', () => {
  let [sq, id] = activeFlank();
  // gather first: move now to the gather spot, 2 * dist out; attack on the signal
  const first = sq.ordersDue(5);
  assert.equal(first.length, 4);
  for (const o of first) {
    assert.deepEqual([o.kind, o.start.kind, o.plan], ['move', 'now', id]);
    assert.notEqual(o.thenPos, null);
  }
  assert.deepEqual(first.map((o) => Math.abs(o.pos[1])).sort(), [8, 8, 8, 8]);
  // arrival reports count once they are in
  for (const m of [1, 2, 3, 4]) assert.equal(sq.report({ kind: SQUAD.reportArrived, sender: m, data: id }, 20), true);
  assert.equal(sq.check(19).status, 'keep');
  assert.equal(sq.check(20).status, 'signal');
  const attack = sq.ordersDue(20);
  assert.deepEqual(attack.map((o) => o.kind), ['attack', 'attack', 'attack', 'attack']);
  // a member back after the signal gets the attack order again
  assert.equal(sq.suspend(3), true);
  assert.equal(sq.reinstate(3, 30), true);
  const again = sq.ordersDue(30);
  assert.deepEqual(again.map((o) => [o.member, o.kind, o.plan]), [[3, 'attack', id]]);
  sq.free();

  // orders held back beyond max go stale when their member leaves play
  [sq, id] = activeFlank();
  const one = sq.ordersDue(5, 1);
  assert.equal(one.length, 1);
  for (const m of [1, 2, 3, 4]) if (m !== one[0].member) sq.suspend(m);
  assert.deepEqual(sq.ordersDue(5), []);
  sq.free();

  // a spot beyond the radius does not count
  sq = squadOf('euclid', [[1, 0, 0]]);
  sq.addTactic(lib.tactics.rush());
  sq.observe(0, enemy(10, 20, 0));
  const spots = [{ pos: [14, 0, 0], tags: 0 }];
  sq.plan(0, { spots });
  assert.deepEqual([sq.pendingPlan().goals[0].pos, sq.pendingPlan().goals[0].unchecked], [[19, 0, 0], true]);
  sq.setSpotRadius(5);
  sq.plan(0, { spots });
  assert.deepEqual([sq.pendingPlan().goals[0].pos, sq.pendingPlan().goals[0].unchecked], [[14, 0, 0], false]);
  sq.setSpotRadius(null);
  assert.throws(() => sq.setSpotRadius(-1), RangeError);
  // a custom slot gathers at its dist unless gatherDist says otherwise
  const t = lib.tactic({ weight: 1 });
  assert.equal(t.addSlot({ place: { kind: 'side', side: 'left' }, gather: { kind: 'side', side: 'left' }, dist: 2, gatherDist: 6, kind: 'attack', start: { kind: 'signal' } }), 0);
  const only = squadOf('chebyshev', [[1, 0, 0]]);
  only.addTactic(t);
  only.observe(0, enemy(10, 20, 0));
  only.plan(0);
  const g = only.pendingPlan().goals[0];
  assert.deepEqual([g.pos, g.thenPos], [[20, 6, 0], [20, 2, 0]]);
  only.free();
  sq.free();
});

test('tactic: abort comparisons and lost-target switch', () => {
  // Column inExtra holds 5; a candidate whose abort rule fires is not planned.
  const run = (cmp, value, input = SQUAD.inExtra) => {
    const sq = squadOf('euclid', [[1, 0, 0]]);
    const t = lib.tactic();
    t.addSlot({ place: { kind: 'here' } });
    t.addAbortWhen(input, cmp, value);
    sq.addTactic(t);
    sq.setExtraColumns(1);
    sq.setSkill(255);
    sq.observe(0, enemy(10, 20, 0));
    const r = sq.check(0, { extra: [5, 5] }).status;
    sq.free();
    return r;
  };
  const fires = [['>=', 5], ['<=', 5], ['==', 5], ['!=', 4], [0, 5], [1, 5], [2, 5], [3, 4]];
  const quiet = [['>=', 6], ['<=', 4], ['!=', 5], [0, 6], [3, 5]];
  for (const [cmp, value] of fires) assert.equal(run(cmp, value), 'noPlan', `${cmp} ${value}`);
  for (const [cmp, value] of quiet) assert.equal(run(cmp, value), 'replan', `${cmp} ${value}`);
  for (const cmp of ['>=', '<=', '==', '!=']) assert.equal(run(cmp, 5, 99), 'replan', `${cmp} beyond the row`);
  const t = lib.tactic();
  assert.throws(() => t.addAbortWhen(0, '<', 1), RangeError);
  assert.throws(() => t.addAbortWhen(0, 4, 1), RangeError);
  t.free();
  // The lost-target switch: on (the default) the plan ends, off it is kept.
  for (const [on, want] of [[undefined, 'noPlan'], [false, 'keep']]) {
    const sq = squadOf('euclid', [[1, 0, 0]]);
    const c = lib.tactic();
    c.addSlot({ place: { kind: 'here' } });
    if (on !== undefined) c.abortIfTargetLost(on);
    sq.addTactic(c);
    sq.setSkill(255);
    sq.observe(0, enemy(10, 20, 0));
    assert.equal(sq.check(0).status, 'replan');
    assert.equal(sq.check(5).status, 'keep');
    assert.equal(sq.knowledge().remove(10, ENEMY), true);
    assert.equal(sq.check(6).status, want);
    sq.free();
  }
});

test('plan activation wraps around 2^32', () => {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 1, 0], [3, 0, 1], [4, 1, 1]], { radio: false });
  sq.addTactic(lib.tactics.rush());
  sq.setSkill(255);
  const before = 4294967294;
  sq.observe(before, enemy(10, 20, 0, before));
  const think = sq.skillTable().thinkTicks[3];
  sq.plan(before);
  const at = (before + think) >>> 0;
  assert.equal(sq.pendingPlan().activeAt, at);
  assert.deepEqual(sq.ordersDue(4294967295), []);
  assert.deepEqual(sq.ordersDue(at - 1), []);
  assert.equal(sq.activePlan(), null);
  assert.equal(sq.ordersDue(at).length, 4);
  assert.equal(sq.activePlan().activeAt, at);
  sq.free();
});

test('keeper: named columns, goal option, renames', () => {
  assert.deepEqual(KEEPER, { hasGoal: 0, dist: 1, age: 2, signal: 3, discipline: 4, extra: 5 });
  const d = lib.decider();
  const k = lib.keeper();
  assert.equal(k.addGoalOption(d, 1), 0);
  const score = (row) => d.decide([{ option: 0, target: 0, inputs: row }]);
  assert.equal(score([1, 0, 0, 0, 0]).score, ONE);
  assert.equal(score([0, 0, 0, 0, 0]), null);
  assert.equal(score([1, 64, 0, 0, 0]), null);
  k.set(0, { kind: 'move', pos: [0, 0, 0] });
  k.setCurrent(d, 0, ONE / 2 | 0);
  assert.equal(score([1, 0, 0, 0, 0]).score, 98302);
  k.goalPostponed(5);
  k.setCurrent(d, 0, ONE / 2 | 0);
  assert.equal(score([1, 0, 0, 0, 0]).score, 65535);
  assert.throws(() => k.setCurrent({}, 0, 0), TypeError);
  // the old names still work and give the same answers
  k.goalWon(6);
  k.postpone(10);
  assert.equal(k.broken(20, 10), true);
  k.resume(20);
  assert.equal(k.broken(1000, 10), false);
  assert.equal(lib.discipline(200), lib.holdBonusFor(null, 200));
  k.free();
});

test('skill lookups read the level', () => {
  assert.deepEqual([0, 64, 191, 255].map((s) => lib.giveUpTicksFor(null, s)), [17, 34, 51, 68]);
  assert.equal(lib.thinkTicksFor(null, 255), 5);
  assert.equal(lib.holdBonusFor(null, 0), Math.floor(ONE / 8));
  assert.equal(lib.spotRankErrorFor(null, 0), 2);
  assert.equal(lib.timingSpreadFor(null, 0), 17);
  assert.deepEqual(lib.pickFor(null, 255), { pick: 'best', top: 0 });
  assert.deepEqual(lib.pickFor(null, 0), { pick: 'weighted', top: 3 });
  const table = { ...lib.skillTable(), giveUpTicks: [1, 99, 3, 4] };
  assert.equal(lib.giveUpTicksFor(table, 64), 99);
  assert.throws(() => lib.giveUpTicksFor(null, 256), RangeError);
  assert.throws(() => lib.holdBonusFor({ ...table, pick: ['lucky', 'best', 'best', 'best'] }, 0), RangeError);
});

test('reports and start ticks', () => {
  assert.deepEqual(Report.arrived(3, 9), { kind: SQUAD.reportArrived, subject: 0, pos: [0, 0, 0], seenAt: 0, strength: 0, data: 9, sender: 3, keepIfSenderGone: false });
  assert.deepEqual(Report.goalBroken(4, 7), { kind: SQUAD.reportGoalBroken, subject: 0, pos: [0, 0, 0], seenAt: 0, strength: 0, data: 7, sender: 4, keepIfSenderGone: false });
  const fact = { subject: 8, kind: 1, pos: [5, 6, 1], seenAt: 40, strength: 3, data: 12 };
  assert.deepEqual(Report.enemy(2, fact), { kind: SQUAD.reportEnemy, subject: 8, pos: [5, 6, 1], seenAt: 40, strength: 3, data: 12, sender: 2, keepIfSenderGone: false });
  const goal = (start, startDelay, thenPos = null) => ({ kind: 'move', pos: [0, 0, 0], thenPos, start, startDelay });
  assert.equal(lib.startTick(goal({ kind: 'after', ticks: 5 }, 3), 100), 108);
  assert.equal(lib.startTick(goal({ kind: 'now' }, 2), 100), 102);
  assert.equal(lib.startTick(goal({ kind: 'signal' }, 2), 100), null);
  assert.equal(lib.startTick(goal({ kind: 'now' }, 2), 4294967295), 1);
  assert.equal(lib.awaitsArrival(goal({ kind: 'now' }, 0, [1, 1, 0])), true);
  assert.equal(lib.awaitsArrival(goal({ kind: 'signal' }, 0)), true);
  assert.equal(lib.awaitsArrival(goal({ kind: 'now' }, 0)), false);
  assert.throws(() => lib.startTick({ kind: 'dance' }, 0), RangeError);
});

test('keeper: postpone, broken, suspend never breaks, inputs', () => {
  const k = lib.keeper();
  assert.equal(k.goal(), null);
  k.goalPostponed(0); // without a goal: nothing
  assert.equal(k.broken(100, 10), false);
  const goal = { kind: 'hold', pos: [3, 4, 0], target: 7, start: { kind: 'after', ticks: 2 }, slot: 1, plan: 5, issuedAt: 4294967290, startDelay: 1 };
  k.set(4294967290, goal);
  assert.deepEqual(k.goal(), { member: null, kind: 'hold', custom: 0, pos: [3, 4, 0], unchecked: false, thenPos: null, thenUnchecked: false, target: 7, start: { kind: 'after', ticks: 2 }, slot: 1, plan: 5, issuedAt: 4294967290, startDelay: 1 });
  k.goalPostponed(4294967295);
  k.goalPostponed(3); // the first time stays
  assert.equal(k.broken(8, 10), false);
  assert.equal(k.broken(9, 10), true, 'wraps: 10 ticks since 2^32 - 1');
  k.suspend();
  for (const now of [9, 100, 4294967295, 0]) assert.equal(k.broken(now, 0), false, 'suspended: never broken');
  k.unsuspend(200);
  assert.equal(k.broken(209, 10), false, 'the clock restarts');
  assert.equal(k.broken(210, 10), true);
  k.goalWon(211);
  assert.equal(k.broken(500, 10), false);
  // inputs: has goal, distance, age, signal, discipline
  assert.deepEqual(k.inputs(4294967295, [0, 0, 0], 'euclid', true, 9), [1, 5, 5, 1, 9]);
  assert.deepEqual(k.inputs(0, [0, 0, 0], 'chebyshev', false, 0), [1, 4, 6, 0, 0]);
  k.clear();
  assert.deepEqual(k.inputs(0, [0, 0, 0], 'euclid', false, 0), [0, -1, 0, 0, 0]);
  assert.throws(() => k.set(0, { ...goal, kind: 'dance' }), RangeError);
  assert.throws(() => k.set(0, { ...goal, slot: 256 }), RangeError);
  assert.throws(() => k.set(0, { ...goal, start: { kind: 'after', ticks: -1 } }), RangeError);
  assert.throws(() => k.inputs(0, [0, 0, 0], 'taxi', false, 0), RangeError);
  assert.throws(() => k.broken(-1, 0), RangeError);
  k.set(0, { kind: 'custom', custom: 42, pos: null });
  assert.deepEqual([k.goal().kind, k.goal().custom, k.goal().pos, k.goal().start], ['custom', 42, null, { kind: 'now', ticks: 0 }]);
  k.free();
  k.free();
  assert.throws(() => k.goal(), /free/);
});

test('three levels: flank gathers, one signal at 49, all attack at 55', () => {
  const near = (id, x, y, free = true) => ({ id, faction: 1, pos: [x, y, 0], free });
  const around = [near(7, 30, 0), near(4, -3, 0), near(3, 0, -2), near(6, 1, 1, false), near(2, 0, 2)];
  const called = lib.gather([0, 0], 1, 10, around, { metric: 'chebyshev', freeOnly: true, max: 3 });
  assert.deepEqual(called, [2, 3, 4]);
  const members = [[1, 0, 0], ...called.map((id) => { const c = around.find((a) => a.id === id); return [id, c.pos[0], c.pos[1]]; })];
  const sq = squadOf('chebyshev', members);
  const spots = [[16, 0, SQUAD.tagCover], [16, 2, 0], [20, 4, SQUAD.tagHigh], [20, -4, 0], [12, 8, SQUAD.tagCover]].map(([x, y, tags]) => ({ pos: [x, y, 0], tags }));
  sq.setSkill(192);
  assert.equal(sq.addTactic(lib.tactics.rush()), 0);
  assert.equal(sq.addTactic(lib.tactics.formation('line', 2)), 1);
  assert.equal(sq.addTactic(lib.tactics.flank(3, 4)), 2);
  assert.equal(sq.observe(0, enemy(10, 20, 0)), null);
  assert.equal(sq.report({ kind: SQUAD.reportEnemy, subject: 11, pos: [24, 10, 0], seenAt: 0, sender: 2 }, 0), true);

  // One cell toward a place on Chebyshev: the nearest neighbour, ties to the first.
  const DIRS = [[1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0], [-1, -1], [0, -1], [1, -1]];
  const stepToward = (from, to) => {
    if (from[0] === to[0] && from[1] === to[1]) return from;
    let best = null;
    let bestLeft = Infinity;
    for (const [dx, dy] of DIRS) {
      const p = [from[0] + dx, from[1] + dy, from[2]];
      const left = Math.max(Math.abs(to[0] - p[0]), Math.abs(to[1] - p[1]));
      if (left < bestLeft) [best, bestLeft] = [p, left];
    }
    return best;
  };
  const STEP = 2;
  const walkers = members.map(([id]) => ({ id, goal: null, startAt: 0, gathered: false }));
  const plans = [];
  const signals = [];
  const orders = [];
  const arrivals = [];
  const attacks = [];
  for (let t = 0; t <= 60; t++) {
    walkers.forEach((w, i) => {
      if (!w.goal) return;
      let at = sq.members()[i].pos;
      if (t > w.startAt && (t - w.startAt) % STEP === 0) {
        at = stepToward(at, w.goal.pos);
        assert.equal(sq.setPos(w.id, at), true);
      }
      const gathers = w.goal.thenPos !== null || w.goal.start.kind === 'signal';
      if (gathers && !w.gathered && at[0] === w.goal.pos[0] && at[1] === w.goal.pos[1]) {
        w.gathered = true;
        assert.equal(sq.arrived(w.id, t), true);
        arrivals.push([t, w.id]);
      }
    });
    const { status, plan } = sq.check(t, { spots, seed: 7 });
    if (status === 'replan') {
      const p = sq.pendingPlan();
      assert.equal(p.id, plan);
      plans.push([t, p.tactic, p.target]);
    } else if (status === 'signal') {
      signals.push(t);
    }
    for (const o of sq.ordersDue(t, 8)) {
      const w = walkers.find((w) => w.id === o.member);
      const after = o.start.kind === 'after' ? o.start.ticks : 0;
      w.goal = o;
      w.startAt = t + o.startDelay + after;
      w.gathered = false;
      orders.push([t, o.member]);
    }
    for (const w of walkers) {
      if (w.goal && w.goal.kind === 'attack' && w.goal.start.kind !== 'signal' && w.startAt === t) attacks.push([t, w.id]);
    }
  }
  sq.free();
  assert.deepEqual(plans, [[0, 2, 10]], 'one plan, no replan');
  assert.deepEqual(orders.slice(0, 4), [[5, 1], [9, 2], [9, 3], [10, 4]]);
  assert.deepEqual(arrivals, [[37, 1], [48, 4], [49, 2], [49, 3]]);
  assert.deepEqual(signals, [49], 'one signal');
  attacks.sort((a, b) => a[1] - b[1]);
  assert.deepEqual(attacks, [[55, 1], [55, 2], [55, 3], [55, 4]], 'all aligned');
});

test('hex converters', () => {
  assert.deepEqual(lib.hex.toAxial({ x: 3, y: 3, level: 2 }, 'oddR'), [2, 3, 2]);
  assert.deepEqual(lib.hex.fromAxial([2, 3, 2], HEX.oddR), { x: 3, y: 3, level: 2 });
  assert.deepEqual(lib.hex.toAxial([3, 1, 0], 'evenR'), [2, 1, 0]);
  assert.deepEqual(lib.hex.toAxial({ x: 3, y: 3, z: 0 }, 'oddQ'), [3, 2, 0]);
  for (const layout of ['oddR', 'evenR', 'oddQ', 'evenQ']) {
    for (const [x, y] of [[3, 3], [-2, 5], [4, -3]]) {
      assert.deepEqual(lib.hex.fromAxial(lib.hex.toAxial([x, y, 1], layout), layout), { x, y, level: 1 }, layout);
    }
  }
  assert.throws(() => lib.hex.toAxial([0, 0, 0], 'square'), RangeError);
  assert.throws(() => lib.hex.toAxial([0, 0, 0], 0), RangeError);
  assert.throws(() => lib.hex.toAxial({ x: 0, y: 0, z: 1, level: 2 }, 'oddR'), RangeError);
});

test('AI blocks take both shapes', () => {
  const k = lib.knowledge(4);
  k.learn(0, { subject: 1, kind: 1, pos: { x: 4, y: 5 } });
  assert.deepEqual(k.get(1, 1).pos, [4, 5, 0]);
  k.learn(0, { subject: 2, kind: 1, pos: { x: 1, y: 2, level: 3 } });
  assert.deepEqual(k.get(2, 1).pos, [1, 2, 3]);
  k.learn(0, { subject: 3, kind: 1, pos: { x: 1, y: 2, z: 4 } });
  assert.deepEqual(k.get(3, 1).pos, [1, 2, 4]);
  k.learn(0, { subject: 4, kind: 1, pos: [7, 8, 9] });
  assert.deepEqual(k.get(4, 1).pos, [7, 8, 9]);
  const a = lib.areas(4);
  a.add(0, { kind: 1, pos: { x: 5, y: 5 }, radius: 2, strength: 7, lifetime: 100 });
  assert.equal(a.cellCost({ x: 5, y: 5 }, { x: 0, y: 0 }, [9, 9, 0]), 7);
  assert.equal(a.cellCost([8, 8, 0], [0, 0, 0], [9, 9, 0]), 0);
  a.add(0, { kind: 2, pos: [5, 5, 0], radius: 2, lifetime: 100, blocking: true });
  assert.equal(a.cellCost([5, 5, 0], [0, 0, 0], [9, 9, 0]), null);
  assert.equal(a.contains({ x: 5, y: 5 }), 0);
  assert.throws(() => k.learn(0, { subject: 5, kind: 1, pos: { x: 1 } }), RangeError);
  assert.throws(() => k.learn(0, { subject: 5, kind: 1, pos: [1, 2] }), RangeError);
});

/** The flank scene of the crew tests (Rust and C) as a JS crew. */
function flankCrew() {
  const crew = lib.crew({ squad: { members: 4, memory: 8, areas: 4, inbox: 8, metric: 'chebyshev' } });
  const members = [[1, 0, 0], [2, 0, 2], [3, 0, -2], [4, -3, 0]];
  members.forEach(([id, x, y], i) => assert.equal(crew.addMember({ id, pos: [x, y, 0], skill: 192 }), i));
  const sq = crew.squad();
  for (const id of [2, 3, 4]) assert.equal(sq.setRadio(id, 2, 1), true);
  assert.equal(sq.setLeader(1), true);
  sq.setSkill(192);
  sq.setTargetKind(ENEMY);
  sq.addTactic(lib.tactics.rush());
  sq.addTactic(lib.tactics.formation('line', 2));
  sq.addTactic(lib.tactics.flank(3, 4));
  assert.equal(sq.report(Report.enemy(2, { subject: 11, pos: [24, 10, 0], seenAt: 0 }), 0), true);
  assert.equal(crew.observe(0, enemy(10, 20, 0)), null);
  return crew;
}

const FLANK_SPOTS = [[16, 0, SQUAD.tagCover], [16, 2, 0], [20, 4, SQUAD.tagHigh], [20, -4, 0]].map(([x, y, tags]) => ({ pos: [x, y, 0], tags }));

/** Runs the flank scene for ticks 0..len: each member walks one cell per 2 ticks toward its order from its start. */
function runFlank(crew, len, each = () => {}) {
  const walks = new Map();
  const all = [];
  for (let t = 0; t <= len; t++) {
    crew.squad().members().forEach((m) => {
      const w = walks.get(m.id);
      if (!w) return;
      const d = t - w.from;
      const [x, y, z] = m.pos;
      if (d > 0 && d % 2 === 0 && (x !== w.to[0] || y !== w.to[1])) {
        assert.equal(crew.setPos(m.id, [x + Math.sign(w.to[0] - x), y + Math.sign(w.to[1] - y), z]), true);
      }
    });
    const events = crew.tick(t, { spots: FLANK_SPOTS, seed: 7 });
    for (const e of events) {
      if (e.kind === 'order') walks.set(e.member, { to: e.goal.pos, from: e.startAt ?? t });
      all.push([t, e]);
    }
    each(t, events);
  }
  return all;
}

test('crew: flank scene', () => {
  const crew = flankCrew();
  const all = runFlank(crew, 80);
  // The list of the Rust and C crew (crew_flank_scene_same_as_rust): tick, kind, member.
  const want = [
    [0, 'planned'], [5, 'activated'], [5, 'order', 1], [5, 'start', 1],
    [9, 'order', 2], [9, 'order', 3], [9, 'start', 2], [9, 'start', 3], [10, 'order', 4], [10, 'start', 4],
    [37, 'arrived', 1], [48, 'arrived', 4], [49, 'arrived', 2], [49, 'arrived', 3],
    [55, 'signal'], [55, 'order', 1], [59, 'order', 4], [61, 'order', 2], [61, 'order', 3],
    [61, 'start', 1], [61, 'start', 2], [61, 'start', 3], [61, 'start', 4],
  ];
  assert.deepEqual(all.map(([t, e]) => (e.member === undefined ? [t, e.kind] : [t, e.kind, e.member])), want);
  const [, planned] = all[0];
  assert.deepEqual(planned, { kind: 'planned', plan: planned.plan });
  const [, order] = all[2];
  assert.deepEqual(Object.keys(order).sort(), ['goal', 'kind', 'member', 'plan', 'startAt']);
  assert.equal(order.plan, planned.plan);
  assert.equal(order.goal.plan, planned.plan);
  assert.deepEqual(order.goal.pos, [16, 0, 0]);
  assert.equal(order.startAt, 5);
  const second = all.find(([t, e]) => t === 55 && e.kind === 'order')[1];
  assert.equal(second.startAt, 61);
  assert.deepEqual(crew.goal(1), { ...second.goal, member: null });
  assert.equal(crew.goal(99), null);
  // The member's own row: the keeper's five columns.
  const inputs = crew.memberInputs(1, 81);
  assert.deepEqual(inputs.row, [inputs.hasGoal, inputs.dist, inputs.age, inputs.signal, inputs.discipline]);
  assert.equal(inputs.hasGoal, 1);
  assert.equal(inputs.signal, 1);
  assert.equal(inputs.discipline, lib.holdBonusFor(null, 192));
  assert.equal(crew.memberInputs(99, 81), null);
  assert.deepEqual(crew.squad().activePlan().goals.map((g) => g.member), [1, 2, 3, 4]);
  crew.free();
});

test('crew: members, choices, reports, errors', () => {
  const crew = flankCrew();
  runFlank(crew, 12);
  // Postponing: broken after give-up ticks of the skill, once.
  const giveUp = lib.giveUpTicksFor(null, 192);
  assert.equal(crew.memberChose(2, 13, false), true);
  assert.equal(crew.memberChose(99, 13, false), false);
  const broken = [];
  for (let t = 13; t <= 13 + giveUp + 2; t++) {
    for (const e of crew.tick(t, { spots: FLANK_SPOTS, seed: 7 })) if (e.kind === 'broken') broken.push([t, e.member]);
  }
  assert.deepEqual(broken, [[13 + giveUp, 2]]);
  assert.equal(crew.suspend(3), true);
  assert.equal(crew.reinstate(3, 40), true);
  assert.equal(crew.setGone(4), true);
  assert.equal(crew.suspend(99), false);
  assert.equal(crew.reportDanger({ sender: 2, centre: [5, 5, 0] }, 41), true);
  assert.equal(crew.removeMember(3), true);
  assert.equal(crew.removeMember(3), false);
  assert.equal(crew.addMember({ id: 1, pos: [0, 0, 0] }), null, 'id taken');
  assert.throws(() => crew.addMember({ id: 8, pos: [0, 0, 0], state: 'lost' }), RangeError);
  assert.throws(() => crew.tick(50, { extra: [1.5] }), RangeError);
  // The crew takes its squad's orders itself.
  assert.throws(() => crew.squad().ordersDue(50), TypeError);
  crew.free();
  assert.throws(() => crew.tick(51), /after free/);
  crew.free();
  // Config checks.
  assert.throws(() => lib.crew({ squad: { members: 0 } }), RangeError);
  assert.throws(() => lib.crew({ squad: { metric: 'euclid' }, hex: 'oddR' }), RangeError);
  assert.throws(() => lib.crew({ hex: 'square' }), RangeError);
  assert.throws(() => lib.crew({ arriveRadius: -1 }), RangeError);
  // A tactic reading a column beyond the row: PlanError from tick.
  const bad = lib.crew({ squad: { members: 2 } });
  bad.addMember({ id: 1, pos: [0, 0, 0] });
  const t = lib.tactic({ needsTarget: false });
  t.addConsideration({ input: 99, curve: 'linear', min: 0, max: 1 });
  bad.squad().addTactic(t);
  assert.throws(() => bad.tick(0), PlanError);
  bad.free();
});

test('crew: lent handles throw when stale', () => {
  const crew = flankCrew();
  runFlank(crew, 12);
  const k2 = crew.keeper(2);
  const k3 = crew.keeper(3);
  assert.equal(crew.keeper(99), null);
  assert.deepEqual(k2.goal(), crew.goal(2), 'two keepers at once, reads in between');
  assert.equal(crew.memberInputs(2, 12).hasGoal, 1);
  k3.goalPostponed(12);
  assert.equal(k3.broken(1012, 1000), true);
  k2.free(); // lent: does nothing
  assert.equal(k2.goal().plan, crew.goal(2).plan);
  crew.tick(13, { spots: FLANK_SPOTS, seed: 7 });
  assert.throws(() => k2.goal(), /stale/);
  assert.throws(() => k3.broken(1012, 1000), /stale/);
  assert.equal(crew.keeper(3).broken(1012, 1000), true, 'the postpone reached the crew');
  crew.memberChose(3, 14, true);
  const sq = crew.squad();
  assert.equal(sq, crew.squad());
  sq.free(); // lent: does nothing
  assert.equal(sq.members().length, 4);
  assert.equal(sq.knowledge().get(10, ENEMY).pos[0], 20, 'the lent squad lends its memory too');
  const k = crew.keeper(1);
  crew.free();
  assert.throws(() => sq.activePlan(), /after free/);
  assert.throws(() => sq.knowledge().count(), /after free/);
  assert.throws(() => k.goal(), /after free/);
});

test('crew: a lent keeper follows its member, not its place', () => {
  const crew = flankCrew();
  runFlank(crew, 12);
  // Member 2 sits at group index 1; once member 1 leaves through the squad, member 3 does.
  const k2 = crew.keeper(2);
  assert.equal(crew.squad().removeMember(1), true);
  const k3 = crew.keeper(3);
  assert.deepEqual(k2.goal(), crew.goal(2), "still member 2's keeper");
  assert.deepEqual(k3.goal(), crew.goal(3));
  assert.notDeepEqual(crew.goal(2), crew.goal(3));
  // Member 2 leaves through the squad: its keeper throws.
  assert.equal(crew.squad().removeMember(2), true);
  assert.throws(() => k2.goal(), /stale/);
  assert.throws(() => k2.goalPostponed(13), /stale/);
  assert.equal(crew.keeper(2), null);
  assert.deepEqual(k3.goal(), crew.goal(3));
  crew.tick(13, { spots: FLANK_SPOTS, seed: 7 });
  assert.throws(() => k2.goal(), /stale/);
  assert.throws(() => k3.goal(), /stale/);
  assert.deepEqual(crew.keeper(3).goal(), crew.goal(3));
  crew.free();
});

test('crew: hex positions in and out', () => {
  const crew = lib.crew({ squad: { members: 4, memory: 8, areas: 4, inbox: 8, metric: 'hex' }, hex: 'oddR' });
  for (const [id, x, y] of [[1, 0, 1], [2, 1, 3], [3, 1, -1], [4, -2, 1]]) {
    crew.addMember({ id, pos: [0, 0, 0], skill: 192 });
    assert.equal(crew.setPos(id, { x, y }), true);
  }
  const sq = crew.squad();
  for (const id of [2, 3, 4]) sq.setRadio(id, 2, 1);
  sq.setLeader(1);
  sq.setSkill(192);
  sq.setTargetKind(ENEMY);
  sq.setSpotRadius(40);
  sq.addTactic(lib.tactics.flank(3, 4));
  crew.observe(0, enemy(10, 15, 1));
  // Spots on odd rows only, three cells apart: a lost or doubled shift misses them.
  const spots = [];
  for (let y = -13; y <= 15; y += 2) for (let x = -3; x <= 33; x += 3) spots.push({ pos: [x, y, 0], tags: 0 });
  const isSpot = (p) => spots.some((s) => s.pos[0] === p[0] && s.pos[1] === p[1] && s.pos[2] === p[2]);
  let checked = 0;
  for (let t = 0; t <= 30; t++) {
    for (const e of crew.tick(t, { spots, seed: 7 })) {
      if (e.kind !== 'order') continue;
      assert.deepEqual(crew.goal(e.member), { ...e.goal, member: null });
      for (const [p, unchecked] of [[e.goal.pos, e.goal.unchecked], [e.goal.thenPos, e.goal.thenUnchecked]]) {
        if (p === null) continue;
        assert.equal(unchecked, false);
        assert.ok(isSpot(p), `${p} is a given spot`);
        checked++;
      }
    }
  }
  assert.ok(checked >= 8, `${checked} positions`);
  // The squad itself holds axial positions.
  assert.deepEqual(sq.members()[0].pos, lib.hex.toAxial([0, 1, 0], 'oddR'));
  crew.free();
});


// ---- JavaScript conventions (Task 11) -----------------------------------------

test('positions: arrays and objects are the same', () => {
  const g = lib.grid(10, 10, { preset: 'tabletop' });
  assert.deepEqual(g.findPath([0, 0, 0], [5, 3, 0]), g.findPath({ x: 0, y: 0 }, { x: 5, y: 3 }));
  g.free();
  const k = lib.knowledge(4);
  k.learn(0, { subject: 1, kind: 1, pos: { x: 1, y: 2, level: 1 }, seenAt: 0 });
  k.learn(0, { subject: 2, kind: 1, pos: { x: 1, y: 2, z: 1 }, seenAt: 0 });
  assert.deepEqual(k.get(1, 1).pos, [1, 2, 1]);
  assert.deepEqual(k.get(1, 1).pos, k.get(2, 1).pos);
  k.free();
  // a position without a level sits on level 0, in route and gather too
  const near = [{ id: 1, x: 5, y: 0 }, { id: 2, x: 1, y: 0 }];
  assert.deepEqual(lib.route({ nearest: 1 }, { x: 0, y: 0 }, near), [2]);
  assert.deepEqual(lib.route({ nearest: 1 }, [0, 0, 0], near), [2]);
  const cands = [{ id: 1, faction: 1, pos: [5, 0, 0], free: true }, { id: 2, faction: 1, pos: [1, 0, 0], free: true }];
  assert.deepEqual(lib.gather({ x: 0, y: 0 }, 1, 10, cands), [2, 1]);
  assert.deepEqual(lib.gather({ x: 0, y: 0, level: 0 }, 1, 10, cands), lib.gather([0, 0, 0], 1, 10, cands));
});

test('positions: conflicting z and level throw', () => {
  const k = lib.knowledge(2);
  assert.throws(() => k.learn(0, { subject: 1, kind: 1, pos: { x: 1, y: 2, z: 0, level: 1 }, seenAt: 0 }), RangeError);
  assert.throws(() => k.learn(0, { subject: 1, kind: 1, pos: Object.assign([1, 2, 0], { level: 1 }), seenAt: 0 }), RangeError);
  assert.equal(k.count(), 0);
  // equal values are not a conflict
  assert.equal(k.learn(0, { subject: 1, kind: 1, pos: { x: 1, y: 2, z: 1, level: 1 }, seenAt: 0 }), null);
  k.free();
  const g = lib.grid(4, 4, { preset: 'tabletop', levels: 2 });
  assert.throws(() => g.findPath({ x: 0, y: 0, z: 0, level: 1 }, { x: 1, y: 1 }), RangeError);
  assert.throws(() => g.contains({ x: 0, y: 0, z: 1, level: 0 }), RangeError);
  g.free();
});

test('loose calls take a position', () => {
  const g = lib.grid(8, 6, { preset: 'tabletop', levels: 2 });
  g.reachable({ x: 0, y: 0 }, 20);
  assert.deepEqual(g.pathTo({ x: 4, y: 2 }), g.pathTo(4, 2));
  assert.notEqual(g.pathTo({ x: 4, y: 2 }), null);
  assert.equal(g.pathTo({ x: 4, y: 2, level: 1 }), null); // unlinked level
  assert.equal(g.contains(3, 3), true);
  assert.equal(g.contains({ x: 3, y: 3, level: 1 }), true);
  assert.equal(g.contains([3, 3, 2]), false);
  assert.equal(g.contains({ x: 8, y: 0 }), false);
  // setCost / block on level 1 through a position; the other level is untouched
  g.setCost({ x: 2, y: 2, level: 1 }, 7);
  assert.equal(g.costs()[(1 * 6 + 2) * 8 + 2], 7);
  assert.equal(g.costs()[2 * 8 + 2], 0);
  g.block({ x: 3, y: 2 });
  assert.equal(g.costs()[2 * 8 + 3], 0xffffffff);
  g.setCost(1, 1, 3);
  assert.equal(g.costs()[1 * 8 + 1], 3);
  // walls, zones, flow cost
  g.addWall({ x: 5, y: 0, level: 1 }, 1, 0);
  assert.equal(g.walls()[(1 * 6 + 0) * 8 + 5], 1);
  assert.equal(g.walls()[(1 * 6 + 0) * 8 + 6], 2);
  assert.equal(g.walls()[5], 0);
  g.addZone({ x: 1, y: 4, level: 1 }, 3);
  g.setZones({ x: 2, y: 4 }, 5);
  assert.equal(g.zones()[(1 * 6 + 4) * 8 + 1], 8);
  assert.equal(g.zones()[4 * 8 + 2], 5);
  g.buildFlow([{ x: 7, y: 5 }]);
  assert.equal(g.flowCost({ x: 4, y: 5 }), g.flowCost(4, 5));
  assert.equal(g.flowCost({ x: 4, y: 5 }), 3);
  g.free();
  const h = lib.grid(6, 6, { preset: 'tabletop', hex: HEX.oddR });
  assert.deepEqual(h.hexNeighbors({ x: 2, y: 3 }), h.hexNeighbors(2, 3));
  const n = h.hexNeighbors({ x: 2, y: 2 })[3];
  h.addHexWall({ x: 2, y: 2 }, 3);
  assert.equal(h.findPath({ x: 2, y: 2 }, { x: n.x, y: n.y }).cost > 1, true);
  assert.throws(() => h.addHexWall({ x: 2, y: 2 }, 6), RangeError);
  assert.throws(() => h.addHexWall({ x: 2, y: 2 }, -1), RangeError);
  assert.throws(() => h.addHexWall(2, 2, 1.5), RangeError);
  h.free();
  const q = lib.grid(4, 4, { preset: 'tabletop' });
  assert.throws(() => q.addWall(1, 1, 2, 0), RangeError); // not a neighbour direction
  q.free();
});

test('string constants', () => {
  const sq = lib.squad({});
  sq.addMember({ id: 1, pos: [0, 0, 0], roles: ['ranged'] });
  sq.addMember({ id: 2, pos: [0, 0, 0], roles: ['ranged', 'area'] });
  sq.addMember({ id: 3, pos: [0, 0, 0], roles: SQUAD.roleArea });
  assert.deepEqual(sq.members().map((m) => m.roles), [1, 3, 2]);
  assert.throws(() => sq.addMember({ id: 4, pos: [0, 0, 0], roles: ['archer'] }), RangeError);
  assert.throws(() => sq.addMember({ id: 4, pos: [0, 0, 0], roles: 'ranged' }), TypeError);
  sq.free();
  // spot tags by name make the same plan as by bits
  const plan = (tags) => {
    const s = lib.squad({});
    s.addMember({ id: 1, pos: [0, 0, 0] });
    s.setLeader(1);
    s.setTargetKind(1);
    s.setSpotRadius(50);
    const t = lib.tactic();
    t.addSlot({ min: 1, max: 1, place: { kind: 'tagged', tags: ['cover', 'high'] }, kind: 'hold' });
    s.addTactic(t);
    s.observe(0, { subject: 9, kind: 1, pos: [20, 0, 0], seenAt: 0 });
    s.plan(0, { spots: [{ pos: [3, 3, 0], tags: 0 }, { pos: [6, 1, 0], tags }, { pos: [4, 4, 0], tags: 1 }], seed: 1 });
    const p = s.pendingPlan();
    s.free();
    return p;
  };
  const byBits = plan(3);
  assert.equal(byBits.goals.length, 1);
  assert.deepEqual(plan(['cover']), plan(SQUAD.tagCover));
  assert.deepEqual(plan(['cover', 'high']), byBits);
  assert.deepEqual(plan(['high', 'cover']), plan(SQUAD.tagCover | SQUAD.tagHigh));
  assert.throws(() => plan(['cosy']), RangeError);
  // fact source
  const k = lib.knowledge(4);
  k.learn(0, { subject: 1, kind: 1, pos: [0, 0, 0], seenAt: 0, source: 'told' });
  k.learn(0, { subject: 2, kind: 1, pos: [0, 0, 0], seenAt: 0, source: 'heard' });
  k.learn(0, { subject: 3, kind: 1, pos: [0, 0, 0], seenAt: 0, source: 'own' });
  k.learn(0, { subject: 4, kind: 1, pos: [0, 0, 0], seenAt: 0, source: 7 });
  assert.deepEqual([1, 2, 3, 4].map((s) => k.get(s, 1).source), [2, 1, 0, 7]);
  assert.throws(() => k.learn(0, { subject: 5, kind: 1, pos: [0, 0, 0], seenAt: 0, source: 'rumour' }), RangeError);
  assert.throws(() => k.learn(0, { subject: 5, kind: 1, pos: [0, 0, 0], seenAt: 0, source: 256 }), RangeError);
  k.free();
  // report kind by name
  const r = lib.squad({});
  r.addMember({ id: 1, pos: [0, 0, 0] });
  assert.equal(r.report({ ...Report.arrived(1, 4), kind: 'arrived' }, 0), true);
  assert.equal(r.report({ ...Report.goalBroken(1, 4), kind: 'goalBroken' }, 0), true);
  assert.equal(r.report({ ...Report.enemy(1, { subject: 9, kind: 1, pos: [0, 0, 0], seenAt: 0 }), kind: 'enemy' }, 0), true);
  assert.throws(() => r.report({ ...Report.arrived(1, 4), kind: 'lost' }, 0), RangeError);
  r.free();
});

test('bad input throws', () => {
  assert.throws(() => lib.knowledge(0), RangeError);
  const k = lib.knowledge(2);
  const f = { subject: 1, kind: 1, pos: [0, 0, 0], seenAt: 0 };
  assert.throws(() => k.learn(1.5, f), RangeError);
  assert.throws(() => k.learn(-1, f), RangeError);
  assert.throws(() => k.learn('5', f), TypeError);
  assert.throws(() => k.touch(1, 1, 2 ** 32), RangeError);
  assert.throws(() => k.get(NaN, 1), RangeError);
  assert.throws(() => k.get('1', 1), TypeError);
  k.free();
  const g = lib.grid(4, 4, { preset: 'tabletop' });
  assert.throws(() => g.setCost(0, 0, 'x'), TypeError);
  assert.throws(() => g.setCost(0, 0, -1), RangeError);
  assert.throws(() => g.setCost(0.5, 0, 1), RangeError);
  assert.throws(() => g.setLeaveCost(1.5), RangeError);
  assert.throws(() => g.findPath({ x: 0, y: 0 }, { x: 1, y: 1 }, { budget: -1 }), RangeError);
  assert.throws(() => g.findPath({ x: 'a', y: 0 }, { x: 1, y: 1 }), RangeError); // a position of the wrong shape
  assert.throws(() => g.addZone(0, 0, 32), RangeError);
  g.free();
  assert.throws(() => lib.grid(0, 4, { preset: 'tabletop' }), RangeError);
  assert.throws(() => lib.grid(4.5, 4, { preset: 'tabletop' }), RangeError);
  assert.throws(() => lib.arrival(-1, 0, 0, 0), RangeError);
  assert.throws(() => lib.arrival(0, 1.5, 0, 0), RangeError);
  assert.throws(() => lib.arrival('0', 0, 0, 0), TypeError);
  const d = lib.decider();
  assert.throws(() => d.setNow(-1), RangeError);
  assert.throws(() => d.setWindow(1.5), RangeError);
  assert.throws(() => d.setCooldown(0, 1), RangeError); // no option
  d.free();
  const m = lib.machine(['a', 'b'], ['x']);
  assert.throws(() => m.update(-1), RangeError);
  assert.throws(() => m.setState('b', 2 ** 32), RangeError);
  m.free();
  const t = lib.timers(2);
  assert.throws(() => t.set(1, 0, 1.5), RangeError);
  t.free();
  assert.throws(() => lib.sine(1.5), RangeError);
  assert.throws(() => lib.pointsAround(1, 0.5, [0, 0, 0]), RangeError);
  assert.throws(() => lib.sweepHeading(0, -1, 5), RangeError);
});

test('every handle guarded', () => {
  const handles = [
    ['grid', lib.grid(4, 4, { preset: 'tabletop' }), (h) => h.findPath({ x: 0, y: 0 }, { x: 1, y: 1 })],
    ['decider', lib.decider(), (h) => h.addOption(1)],
    ['machine', lib.machine(['a', 'b']), (h) => h.update(0)],
    ['knowledge', lib.knowledge(2), (h) => h.count()],
    ['squad', lib.squad({}), (h) => h.members()],
    ['keeper', lib.keeper(), (h) => h.goal()],
    ['crew', lib.crew({}), (h) => h.goal(1)],
  ];
  for (const [name, h, call] of handles) {
    assert.doesNotThrow(() => call(h), name);
    h.free();
    assert.throws(() => call(h), /after free/, name);
    h.free(); // twice is fine
  }
  if (Symbol.dispose) {
    const makers = [
      () => lib.grid(2, 2, { preset: 'tabletop' }), () => lib.decider(), () => lib.machine(['a']), () => lib.knowledge(1),
      () => lib.outbox(1), () => lib.squad({}), () => lib.keeper(), () => lib.crew({}),
    ];
    for (const make of makers) {
      const h = make();
      h[Symbol.dispose]();
      assert.equal(h.handle, 0);
    }
  }
});

test('value classes match C', () => {
  const p = new Progress();
  p.start(3, 4294967290);
  assert.equal(p.stalled(3, 10), false); // age 9
  assert.equal(p.stalled(4, 10), true); // age 10 across the wrap
  assert.equal(p.update(1, 6, 1) && p.since === 6 && p.best === 1, true);
  const i = new Interval();
  i.mark(4294967295);
  assert.equal(i.due(8, 10), false);
  assert.equal(i.due(9, 10), true);
  assert.throws(() => p.update(1.5, 0, 0), RangeError);
  assert.throws(() => p.stalled(-1, 0), RangeError);
  assert.throws(() => i.mark(2 ** 32), RangeError);
  assert.throws(() => new Alertness().stimulus(1, 0, { max: 10, thresholds: [1, 2, 3, 4, 5, 6, 7, 8, 9] }), RangeError);
  assert.throws(() => new Alertness().stimulus(-1, 0, { max: 10 }), RangeError);
  // plain objects: the fields are there, not hidden
  assert.deepEqual(Object.keys(new Progress()).sort(), ['best', 'since', 'started']);
  assert.deepEqual(Object.keys(new Interval()).sort(), ['last', 'started']);
  assert.deepEqual(Object.keys(new Alertness()).sort(), ['last', 'lastStimulus', 'level', 'value']);
});

test('setCombine: ALL_OPTIONS or a valid option', () => {
  const d = lib.decider();
  const one = d.addOption(1, [{ input: 0 }]);
  d.setCombine('minimum', ALL_OPTIONS);
  d.setCombine('median', one);
  d.setCombine(null, one);
  assert.throws(() => d.setCombine('minimum', -2), RangeError);
  assert.throws(() => d.setCombine('minimum', one + 5), RangeError);
  assert.throws(() => d.setCombine('minimum', 1.5), RangeError);
  d.free();
});

test('skill table: negative throws', () => {
  const sq = squadOf('euclid', [[1, 0, 0]]);
  const table = sq.skillTable();
  for (const key of ['pickTop', 'thinkTicks', 'timingSpread', 'holdBonus', 'giveUpTicks']) {
    assert.throws(() => sq.setSkillTable({ ...table, [key]: [-1, 0, 0, 0] }), RangeError, key);
  }
  assert.throws(() => sq.setSkillTable({ ...table, spotRankError: [-1, 0, 0, 0] }), RangeError);
  assert.deepEqual(sq.skillTable(), table, 'nothing changed');
  // the raw module refuses a negative entry too (it never wraps to u32)
  const x = lib.x;
  const record = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  const put = (values) => new Int32Array(x.memory.buffer, x.sx_scratch(values.length), values.length).set(values);
  put(record);
  assert.equal(x.sx_squad_set_skill_table(sq.h), 0);
  put(record.map((v, i) => (i === 9 ? -1 : v)));
  assert.equal(x.sx_squad_set_skill_table(sq.h), -1);
  sq.free();
});

test('radius and range must be whole', () => {
  const t = lib.tactic();
  const sq = squadOf('euclid', [[1, 0, 0]]);
  assert.throws(() => t.triggerTargetInRadius(NaN), RangeError);
  assert.throws(() => t.triggerTargetInRadius(2.5), RangeError);
  assert.throws(() => sq.setShout(1, 1.5), RangeError);
  assert.throws(() => sq.setShout(1, NaN), RangeError);
  assert.throws(() => sq.setSpotRadius(1.5), RangeError);
  // the raw module does not truncate either
  const x = lib.x;
  const tactic = lib.tactic();
  assert.equal(x.sx_tactic_trigger_target_in_radius(tactic.h, NaN), -1);
  assert.equal(x.sx_tactic_trigger_target_in_radius(tactic.h, 2.5), -1);
  assert.equal(x.sx_tactic_trigger_target_in_radius(tactic.h, 9.3e18), -1);
  assert.equal(x.sx_tactic_trigger_target_in_radius(tactic.h, -3), 0);
  assert.equal(x.sx_squad_set_shout(sq.h, 1, 1.5), -1);
  assert.equal(x.sx_squad_set_shout(sq.h, 1, NaN), -1);
  assert.equal(x.sx_squad_set_shout(sq.h, 1, 4), 1);
  assert.equal(x.sx_squad_set_spot_radius(sq.h, 1.5), -1);
  assert.equal(x.sx_squad_set_spot_radius(sq.h, NaN), -1);
  assert.equal(x.sx_squad_set_spot_radius(sq.h, 4294967296), -1);
  tactic.free();
  t.free();
  sq.free();
});

test('flank sides are 2 or 3', () => {
  assert.throws(() => lib.tactics.flank(4, 4), RangeError);
  assert.throws(() => lib.tactics.flank(1, 4), RangeError);
  assert.throws(() => lib.tactics.flank(2.5, 4), RangeError);
  assert.throws(() => lib.tactics.flank('3', 4), RangeError);
  const sq = squadOf('euclid', [[1, 0, 0]]);
  assert.equal(sq.addTactic(lib.tactics.flank(2, 4)), 0);
  assert.equal(sq.addTactic(lib.tactics.flank(3, 4)), 1);
  sq.free();
});

test('SHAPE matches C', () => {
  assert.equal(SHAPE.line, 0);
  assert.equal(SHAPE.wedge, 1);
  assert.equal(SHAPE.column, 2);
  assert.throws(() => lib.tactics.formation(3, 2), RangeError);
  assert.throws(() => lib.tactics.formation(-1, 2), RangeError);
  assert.throws(() => lib.tactics.formation('ring', 2), RangeError);
  // the number and the name make the same tactic: the same plan, slot for slot
  const planOf = (shape) => {
    const sq = squadOf('euclid', [[1, 0, 0], [2, 1, 0], [3, 0, 1], [4, 1, 1]], { radio: false });
    sq.addTactic(lib.tactics.formation(shape, 2));
    sq.setSkill(255);
    sq.observe(0, enemy(10, 20, 0));
    sq.plan(0);
    const plan = sq.pendingPlan();
    sq.free();
    return plan;
  };
  for (const name of ['line', 'wedge', 'column']) {
    const byName = planOf(name);
    assert.ok(byName.goals.length >= 2, name);
    assert.deepEqual(planOf(SHAPE[name]), byName, name);
  }
  assert.notDeepEqual(planOf(SHAPE.line).goals.map((g) => g.pos), planOf(SHAPE.column).goals.map((g) => g.pos));
});

console.log(`${passed} tests passed`);
