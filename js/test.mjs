// Node test of the WebAssembly build on tabletop-like situations.
// Run: js/run.sh  (builds the .wasm first)

import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { MEMORY, ONE, loadSagax, HEX, FORMATION, Progress, Interval, Alertness } from './sagax.mjs';

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


// ---- world memory ----
// Golden numbers are the ones of crates/sagax-decide/src/memory/tests.rs.

const fact = (subject, kind, seenAt, more = {}) => ({ subject, kind, pos: [0, 0, 0], seenAt, source: 0, strength: 0, data: 0, ...more });
const msg = (subject, kind = 1, more = {}) => ({ kind, subject, pos: [1, 2, 3], seenAt: 7, strength: 8, data: 9, sender: 10, keepIfSenderGone: false, ...more });
const keys = (k) => k.list().map((f) => [f.subject, f.kind]);

test('knowledge: key is (subject, kind), learn overwrites, setters', () => {
  assert.throws(() => lib.knowledge(0), RangeError);
  assert.throws(() => lib.knowledge(4, 'newest'), RangeError);
  const k = lib.knowledge(4);
  assert.equal(k.learn(fact(1, 1, 0), 0), null);
  assert.equal(k.learn(fact(1, 2, 0), 0), null);
  assert.equal(k.learn(fact(2, 1, 0), 0), null);
  assert.equal(k.count(), 3);
  assert.equal(k.learn(fact(1, 2, 5, { pos: [1, -2, 3] }), 5), null); // keeps its slot
  assert.deepEqual(keys(k), [[1, 1], [1, 2], [2, 1]]);
  assert.deepEqual(k.get(1, 2), { subject: 1, kind: 2, pos: [1, -2, 3], seenAt: 5, source: 0, strength: 0, data: 0 });
  assert.equal(k.get(2, 2), null);
  const old = fact(1, 2, 3, { source: 2, strength: 4, data: 5, pos: [7, 8, 9] });
  assert.equal(k.learn(old, 5), null); // learn overwrites even an older fact
  assert.deepEqual(k.get(1, 2), old);
  assert.equal(k.touch(1, 2, 20) && k.setPos(1, 2, [1, 1, 1]) && k.setData(1, 2, 6) && k.setStrength(1, 2, 8), true);
  assert.deepEqual(k.get(1, 2), fact(1, 2, 20, { source: 2, strength: 8, data: 6, pos: [1, 1, 1] }));
  assert.equal(k.touch(9, 9, 0) || k.setPos(9, 9, [0, 0, 0]) || k.setData(9, 9, 0) || k.setStrength(9, 9, 0) || k.remove(9, 9), false);
  assert.equal(k.remove(1, 2), true);
  assert.equal(k.removeSubject(1), 1);
  assert.equal(k.removeSubject(1), 0);
  k.learn(fact(2, 5, 0), 0);
  k.learn(fact(2, 6, 0), 0);
  assert.equal(k.removeSubject(2), 3); // all kinds
  assert.deepEqual(k.list(), []);
  k.learn(fact(3, 0, 0), 0);
  k.clear();
  assert.equal(k.count(), 0);
  // extreme values survive the round trip
  k.learn({ subject: 4294967295, kind: 4294967295, pos: [-2147483648, 2147483647, -1], seenAt: 4294967295, source: 255, strength: 4294967295, data: 4294967295 }, 0);
  assert.deepEqual(k.get(4294967295, 4294967295), { subject: 4294967295, kind: 4294967295, pos: [-2147483648, 2147483647, -1], seenAt: 4294967295, source: 255, strength: 4294967295, data: 4294967295 });
  k.free();
  assert.throws(() => k.count(), /free/);
});

test('knowledge: evict oldest (tie earlier slot) and weakest then oldest', () => {
  const k = lib.knowledge(3, 'oldest');
  k.learn(fact(1, 0, 0), 0);
  k.learn(fact(2, 0, 1), 1);
  k.learn(fact(3, 0, 2), 2);
  assert.equal(k.touch(1, 0, 3), true);
  assert.equal(k.learn(fact(4, 0, 4), 4).subject, 2); // returns the evicted fact
  assert.deepEqual(keys(k), [[1, 0], [3, 0], [4, 0]]);
  const t = lib.knowledge(3, 'oldest');
  for (let s = 1; s <= 3; s++) t.learn(fact(s, 0, 5), 5);
  assert.equal(t.learn(fact(9, 0, 5), 5).subject, 1);
  assert.deepEqual(keys(t), [[2, 0], [3, 0], [9, 0]]);
  assert.equal(t.learn(fact(2, 0, 6), 6), null); // present key does not evict
  const w = lib.knowledge(4, 'weakest');
  const with_ = (s, at, st) => fact(s, 0, at, { strength: st });
  w.learn(with_(1, 10, 5), 10);
  w.learn(with_(2, 0, 9), 10);
  w.learn(with_(3, 4, 5), 10);
  w.learn(with_(4, 4, 5), 10);
  assert.equal(w.learn(with_(5, 10, 7), 10).subject, 3); // weakest 5, oldest of those, earlier slot
  assert.deepEqual(keys(w), [[1, 0], [2, 0], [4, 0], [5, 0]]);
  assert.equal(w.learn(with_(6, 10, 9), 10).subject, 4);
  const s = lib.knowledge(2, 'weakest');
  s.learn(with_(1, 0, 8), 100);
  s.learn(with_(2, 99, 2), 100);
  assert.equal(s.learn(with_(3, 100, 5), 100).subject, 2); // strength beats age
});

test('knowledge: merge keeps the newer, also across the wrap', () => {
  const k = lib.knowledge(2);
  k.merge(fact(1, 0, 100, { data: 1 }), 110);
  assert.equal(k.merge(fact(1, 0, 90, { data: 2 }), 110), null);
  assert.equal(k.get(1, 0).data, 1);
  k.merge(fact(1, 0, 100, { data: 3 }), 110); // equal: not older
  assert.equal(k.get(1, 0).data, 3);
  k.merge(fact(1, 0, 105, { data: 4 }), 110);
  assert.deepEqual([k.get(1, 0).seenAt, k.get(1, 0).data], [105, 4]);
  const w = lib.knowledge(1);
  w.merge(fact(1, 0, 4294967293), 5); // age 8
  w.merge(fact(1, 0, 2, { data: 9 }), 5); // age 3 is newer
  assert.equal(w.get(1, 0).data, 9);
  w.merge(fact(1, 0, 4294967287, { data: 7 }), 5); // age 15 is older
  assert.equal(w.get(1, 0).data, 9);
  const f = lib.knowledge(1);
  f.merge(fact(1, 0, 0), 1);
  assert.equal(f.merge(fact(2, 0, 1), 1).subject, 1);
});

test('knowledge: learnMessage and mergeMessage turn a message into a fact', () => {
  const k = lib.knowledge(4);
  const m = msg(5, 3);
  k.learnMessage(m, 2, 10);
  assert.deepEqual(k.get(5, 3), { subject: 5, kind: 3, pos: [1, 2, 3], seenAt: 7, source: 2, strength: 8, data: 9 });
  k.mergeMessage({ ...m, seenAt: 3, data: 1 }, 1, 10); // older: kept
  assert.equal(k.get(5, 3).data, 9);
  k.mergeMessage({ ...m, seenAt: 8, data: 1 }, 1, 10);
  assert.deepEqual([k.get(5, 3).data, k.get(5, 3).source], [1, 1]);
});

test('knowledge: expire, expire with per-kind ages, expireKind', () => {
  const k = lib.knowledge(5);
  k.learn(fact(1, 1, 0), 0);
  k.learn(fact(2, 2, 0), 0);
  k.learn(fact(3, 1, 10), 10);
  k.learn(fact(4, 2, 10), 10);
  assert.deepEqual(k.expire(20, 20).map((f) => f.subject), [1, 2]);
  // kind 1 lives 5, kind 2 lives 15: only subject 3 (age 10, kind 1) goes
  assert.deepEqual(k.expire(20, 0, (f) => (f.kind === 1 ? 5 : 15)).map((f) => f.subject), [3]);
  assert.deepEqual(keys(k), [[4, 2]]);
  assert.throws(() => k.expire(20, 0, () => undefined), TypeError);
  assert.equal(k.count(), 1); // a bad callback removes nothing
  const e = lib.knowledge(5);
  e.learn(fact(1, 1, 0), 0);
  e.learn(fact(2, 2, 0), 0);
  e.learn(fact(3, 1, 0), 0);
  e.learn(fact(4, 1, 8), 8);
  const gone = e.expireKind(10, 1, 10);
  assert.deepEqual(gone.map((f) => f.subject), [1, 3]); // slot order, kind 1 only
  assert.deepEqual(gone[0], fact(1, 1, 0));
  assert.deepEqual(keys(e), [[2, 2], [4, 1]]);
  assert.deepEqual(e.expireKind(10, 9, 0), []);
  // wrapped clock: age 15 at now = 10
  const w = lib.knowledge(1);
  w.learn(fact(1, 0, 4294967291), 0);
  assert.deepEqual(w.expire(10, 16), []);
  assert.equal(w.expire(10, 15).length, 1);
  w.learn(fact(1, 0, 4294967291), 0);
  assert.equal(w.expireKind(10, 0, 15).length, 1);
});

test('outbox: cooldown from the last accepted post, full, delays, sender gone', () => {
  assert.throws(() => lib.outbox(0), RangeError);
  const o = lib.outbox(4);
  assert.equal(o.post(msg(1), 0, 102, 170), 'ok');
  assert.equal(o.post(msg(2), 169, 102, 170), 'cooldown');
  assert.equal(o.post(msg(3), 170, 102, 170), 'ok'); // a refused post does not move the start
  assert.equal(o.post(msg(4), 339, 102, 170), 'cooldown');
  assert.equal(o.post(msg(5), 340, 102, 170), 'ok');
  assert.equal(o.pending(), 3);
  const w = lib.outbox(2);
  assert.equal(w.post(msg(1), 4294967285, 0, 170), 'ok');
  assert.equal(w.post(msg(1), 158, 0, 170), 'cooldown');
  assert.equal(w.post(msg(1), 159, 0, 170), 'ok');
  const f = lib.outbox(2);
  assert.equal(f.post(msg(1), 0, 5), 'ok');
  assert.equal(f.post(msg(2), 0, 5), 'ok');
  assert.equal(f.post(msg(3), 0, 5), 'full');
  assert.equal(f.due(5).length, 2);
  assert.equal(f.post(msg(3), 5, 5), 'ok');

  const d = lib.outbox(4);
  d.post(msg(1), 0, 10);
  d.post(msg(2), 1, 2);
  d.post(msg(3), 2, 0);
  assert.deepEqual(d.due(3).map((m) => m.subject), [2, 3]);
  assert.equal(d.pending(), 1);
  const [last] = d.due(10);
  assert.deepEqual(last, msg(1));
  assert.equal(d.pending(), 0);

  const g = lib.outbox(4);
  g.post(msg(1), 0, 5);
  g.post(msg(2, 1, { keepIfSenderGone: true }), 0, 5);
  g.senderGone();
  assert.equal(g.pending(), 1);
  assert.deepEqual(g.due(5), [msg(2, 1, { keepIfSenderGone: true })]);
  g.post(msg(3), 6, 5);
  g.clear();
  assert.equal(g.pending(), 0);
  assert.equal(g.post(msg(4), 7, 5, 100), 'cooldown'); // the cooldown survives clear
  assert.equal(g.post(msg(4), 106, 5, 100), 'ok');

  const m = lib.outbox(4);
  for (const s of [1, 2, 3]) m.post(msg(s), 0, 0);
  assert.deepEqual(m.due(5, 2).map((x) => x.subject), [1, 2]);
  assert.equal(m.pending(), 1);
  assert.deepEqual(m.due(5, 0), []);
  assert.equal(m.due(5, 9).length, 1);
  // a message with the extremes of every field
  const big = { kind: 4294967295, subject: 4294967295, pos: [-2147483648, 2147483647, -1], seenAt: 4294967295, strength: 4294967295, data: 4294967295, sender: 4294967295, keepIfSenderGone: true };
  m.post(big, 0, 0);
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
  assert.equal(z.add(area(1, 0, 0x200), 0), 0);
  assert.equal(z.add(area(1, 0x1ff, 0x200, { data: 2, source: [5, 6, -7] }), 10), 0); // renews
  assert.equal(z.count(), 1);
  assert.deepEqual(z.list(), [area(1, 0x1ff, 0x200, { data: 2, since: 10, source: [5, 6, -7] })]);
  assert.equal(z.add(area(1, 0x200 + 0x1ff, 0x200), 11), 1); // exactly at the radius: new
  assert.equal(z.add(area(2, 0, 100), 1), 2); // other kind: new
  assert.equal(z.containsKind([0, 0, 0], 2), 2);
  assert.equal(z.containsKind([0, 0, 0], 3), null);
  assert.equal(z.contains([0, 0, 0]), 0);
  assert.equal(z.contains([5000, 0, 0]), null);
  z.clear();
  assert.equal(z.count(), 0);
  // renewal uses the existing radius, then replaces it
  z.add(area(1, 0, 10), 0);
  assert.equal(z.add(area(1, 50, 1000), 1), 1);
  assert.equal(z.add(area(1, 5, 20), 2), 0);
  assert.equal(z.list()[0].radius, 20);
  // full: the oldest goes, the new one is last
  const f = lib.areas(2);
  f.add(area(1, 0, 10, { data: 1 }), 5);
  f.add(area(1, 100, 10, { data: 2 }), 2);
  assert.equal(f.add(area(1, 200, 10, { data: 3 }), 6), 1);
  assert.deepEqual(f.list().map((a) => a.data), [1, 3]);
  // expire by own lifetime, also across the wrap
  const e = lib.areas(4);
  e.add(area(1, 0, 10, { lifetime: 100 }), 0);
  e.add(area(1, 100, 10, { lifetime: 300, data: 2 }), 1);
  assert.equal(e.expire(99), 0);
  assert.equal(e.expire(100), 1);
  assert.equal(e.expire(300), 0);
  assert.equal(e.expire(301), 1);
  const w = lib.areas(1);
  w.add(area(1, 0, 10), 4294967285);
  assert.equal(w.expire(243), 0);
  assert.equal(w.expire(245), 1);
  // blocks and cost
  const b = lib.areas(4);
  b.add(area(1, 0, 100, { blocking: true, strength: 1000 }), 0);
  const out = [1000, 0, 0];
  assert.equal(b.blocks([10, 0, 0], out, [2000, 0, 0]), true);
  assert.equal(b.blocks([10, 0, 0], [20, 0, 0], out), false); // start inside
  assert.equal(b.blocks([10, 0, 0], out, [20, 0, 0]), false); // goal inside
  assert.equal(b.blocks([500, 0, 0], out, [2000, 0, 0]), false);
  const c = lib.areas(4);
  c.add(area(1, 0, 100, { strength: 7 }), 0);
  c.add(area(2, 0, 100, { strength: 5 }), 0);
  c.add(area(3, 0, 100, { strength: 1000, blocking: true }), 0);
  assert.equal(c.cost([10, 0, 0], out, [2000, 0, 0]), 12);
  assert.equal(c.cost([500, 0, 0], out, [2000, 0, 0]), 0);
  assert.equal(c.cost([10, 0, 0], [20, 0, 0], out), 0);
  const s = lib.areas(2);
  s.add(area(1, 0, 100, { strength: 4294967295 }), 0);
  s.add(area(2, 0, 100, { strength: 9 }), 0);
  assert.equal(s.cost([10, 0, 0], out, [2000, 0, 0]), 4294967295); // saturates, still unsigned
  // radius 0 holds nothing; extreme coordinates do not overflow
  const r = lib.areas(1);
  r.add(area(1, 0, 0), 0);
  assert.equal(r.contains([0, 0, 0]), null);
  const x = lib.areas(2);
  x.add({ kind: 1, pos: [-2147483648, -2147483648, 0], radius: 2147483647, lifetime: 1, blocking: true }, 0);
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
      assert.deepEqual(lib.pointsAround(agent, step, [c[0], c[1], 7], { sine: 'coarse' }), [x, y, 7], `agent ${agent} step ${step} centre ${c}`);
    });
  }
  assert.equal(lib.pointsAround(0, 4, [0, 0, 0], { sine: 'coarse' }), null);
  assert.equal(lib.pointsAround(3, 4, [0, 0, 0]), null);
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
    known[g].learn({ subject: PLAYER, kind: NOISE, pos: [5, 0, 0], seenAt: 2, source: 1 }, 2);
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
  known[1].learnMessage(told, 2, 11);
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
  k.learn(fact(1, 1, 5, { pos: [1, 2, 3] }), 5);
  assert.deepEqual(k.get(1, 1).pos, [1, 2, 3]);
  const big = lib.knowledge(20000);
  for (let i = 0; i < 20000; i++) big.learn(fact(i, 0, i), i);
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
  o.post(m, 0, 0);
  const [out] = o.due(0);
  out.pos[0] = 99;
  m.pos[1] = 77; // the caller's own message is not kept either
  o.post(msg(1), 1, 0);
  assert.deepEqual(o.due(1)[0].pos, [1, 2, 3]);
  const k = lib.knowledge(2);
  k.learn(fact(1, 1, 0, { pos: [4, 5, 6] }), 0);
  const f = k.get(1, 1);
  f.pos[0] = 99;
  k.list()[0].pos[1] = 99;
  assert.deepEqual(k.get(1, 1).pos, [4, 5, 6]);
  for (const t of [o, k, lib.inbox(1), lib.areas(1), lib.timers(1)]) {
    t.free();
    t.free(); // twice is fine
  }
  assert.throws(() => o.pending(), /free/);
  assert.throws(() => k.learn(fact(1, 1, 0), 0), /free/);
});

console.log(`${passed} tests passed`);
