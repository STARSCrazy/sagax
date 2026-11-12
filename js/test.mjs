// Node test of the WebAssembly build on tabletop-like situations.
// Run: js/run.sh  (builds the .wasm first)

import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { MEMORY, ONE, loadSagax, HEX, FORMATION } from './sagax.mjs';

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
  const g = lib.grid(20, 20);
  const way = g.findPath({ x: 0, y: 0 }, { x: 5, y: 3 });
  assert.equal(way.reached, true);
  assert.equal(way.cost, 5);
  assert.deepEqual(way.cells[0], { x: 0, y: 0 });
  assert.deepEqual(way.cells.at(-1), { x: 5, y: 3 });
  g.free();
});

test('walls between cells force a way through the door', () => {
  // a wall along x = 5|6 from y = 0 to 9, door at y = 9
  const g = lib.grid(12, 10);
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
  const g = lib.grid(11, 11);
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
  const g = lib.grid(12, 3);
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
  const g = lib.grid(30, 30);
  const area = g.reachable({ x: 15, y: 15 }, 6).cells;
  assert.equal(area.length, 13 * 13);
  assert.ok(area.every((c) => c.cost <= 6));
  const back = g.pathTo(21, 9);
  assert.equal(back.cost, 6);
  assert.equal(g.pathTo(22, 15), null);
  g.free();
});

test('unreachable goal gives the closest cell', () => {
  const g = lib.grid(10, 10);
  for (let y = 0; y < 10; y++) g.block(5, y);
  const way = g.findPath({ x: 0, y: 5 }, { x: 9, y: 5 });
  assert.equal(way.status, 'partial');
  assert.deepEqual(way.cells.at(-1), { x: 4, y: 5 });
  g.free();
});

test('hex grid: distance, walls between hexes, reachable', () => {
  const g = lib.grid(20, 20, { hex: HEX.oddR });
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

test('speed on a 60 x 60 battle map', () => {
  const g = lib.grid(60, 60);
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
  const g = lib.grid(20, 10);
  const n = g.buildFlow([{ x: 0, y: 5 }, { x: 19, y: 5 }]);
  assert.equal(n, 200);
  assert.equal(g.flowCost(4, 5), 4);
  assert.equal(g.flowCost(16, 5), 3);
  const [a, b, c] = g.flowNext([{ x: 16, y: 5 }, { x: 0, y: 5 }, { x: 3, y: 5 }]);
  assert.deepEqual(a, { x: 17, y: 5, cost: 3 });
  assert.deepEqual(b, { x: 0, y: 5, cost: 0 });
  assert.deepEqual(c, { x: 2, y: 5, cost: 3 });
  assert.equal(g.findPath({ x: 1, y: 1 }, { x: 3, y: 1 }).cells.length, 3, 'flow keeps paths apart');
  g.free();
});

test('smoothing and formation after findPath', () => {
  const g = lib.grid(30, 10);
  g.findPath({ x: 0, y: 0 }, { x: 20, y: 4 });
  assert.deepEqual(g.way('smooth'), [{ x: 0, y: 0 }, { x: 20, y: 4 }]);
  g.findPath({ x: 2, y: 4 }, { x: 20, y: 4 });
  const f = g.formation();
  assert.equal(f.members.length, 3);
  assert.deepEqual(f.leader.at(-1), { x: 20, y: 4 });
  assert.deepEqual(f.members[0].at(-1), { x: 20, y: 5 });
  assert.deepEqual(f.members[1].at(-1), { x: 19, y: 4 });
  g.free();
});

test('D* Lite repairs the way when a cell gets blocked (square and hex)', () => {
  const g = lib.grid(20, 10);
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

  const h = lib.grid(15, 15, { hex: HEX.oddR });
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
    const g = lib.grid(30, 20, { hex });
    g.findPath({ x: 2, y: 12 }, { x: 20, y: 4 });
    assert.deepEqual(g.way('smooth'), [{ x: 2, y: 12 }, { x: 20, y: 4 }], `layout ${hex}`);
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

test('small budgets, continue, outputs, bulk filling', () => {
  const g = lib.grid(30, 30);
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
  assert.deepEqual(turns[0], { x: 1, y: 1 });
  assert.deepEqual(turns.at(-1), { x: 28, y: 1 });
  assert.ok(g.way('smooth').length <= turns.length);
  g.free();
});

test('tracing: every step, same result, off again', () => {
  const g = lib.grid(20, 20);
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
  const g = lib.grid(20, 5);
  const away = g.flee({ x: 5, y: 2 }, [{ x: 3, y: 2 }], 8);
  assert.equal(away.status, 'found');
  assert.deepEqual(away.cells[0], { x: 5, y: 2 });
  assert.ok(away.cells.at(-1).x >= 9);
  const exit = g.findPath({ x: 15, y: 2 }, [{ x: 0, y: 2 }, { x: 19, y: 2 }]);
  assert.deepEqual(exit.cells.at(-1), { x: 19, y: 2 });
  g.free();
});

test('ways split walking cost and zone penalty', () => {
  const g = lib.grid(10, 1);
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
  d.record(cands[0], 100);
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

console.log(`${passed} tests passed`);
