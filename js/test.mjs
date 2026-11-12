// Node test of the WebAssembly build on tabletop-like situations.
// Run: js/run.sh  (builds the .wasm first)

import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { MEMORY, ONE, SQUAD, loadSagax, HEX, FORMATION, Progress, Interval, Alertness } from './sagax.mjs';

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
  // Infinity and limits past 2^32 - 1 never expire; bad limits are refused
  assert.deepEqual(k.expire(1e9, 0, () => Infinity), []);
  assert.deepEqual(k.expire(1e9, 0, () => 2 ** 32), []);
  for (const bad of [-1, NaN, 1.5, -Infinity]) assert.throws(() => k.expire(20, 0, () => bad), RangeError);
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

// ---- group tactics ------------------------------------------------------------

const ENEMY = 1;
const enemy = (subject, x, y, seenAt = 0) => ({ subject, kind: ENEMY, pos: [x, y, 0], seenAt });

/** A squad of members [id, x, y, roles?] (the first leads) that sees enemies of kind ENEMY; the others hear by radio (2 ticks plus 1 per unit). */
function squadOf(metric, members, { radio = true } = {}) {
  const sq = lib.squad({ groupCap: 8, memoryCap: 8, areaCap: 4, inboxCap: 8, metric });
  members.forEach(([id, x, y, roles = 0], i) => assert.equal(sq.addMember({ id, pos: [x, y, 0], roles }), i));
  assert.equal(sq.setLeader(members[0][0]), true);
  if (radio) for (const [id] of members.slice(1)) assert.equal(sq.setChannel(id, { kind: 'radio', base: 2, perUnit: 1 }), true);
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
  c.setTrigger({ kind: 'targetInRadius', radius: 6 });
  c.setTrigger({ kind: 'allArrived', maxWait: 100 });
  c.setTrigger({ kind: 'manual' });
  assert.throws(() => c.addSlot({ place: { kind: 'none' } }), RangeError);
  assert.throws(() => c.addSlot({ place: { kind: 'side', side: 'up' } }), RangeError);
  assert.throws(() => c.addSlot({ place: { kind: 'here' }, kind: 'dance' }), RangeError);
  assert.throws(() => c.addSlot({ place: { kind: 'here' }, start: { kind: 'later' } }), RangeError);
  assert.throws(() => c.addSlot({ place: { kind: 'here' }, max: 256 }), RangeError);
  assert.throws(() => c.addConsideration({ input: 0, curve: 'wobbly' }), RangeError);
  assert.throws(() => c.setTrigger({ kind: 'allArrived', maxWait: -1 }), RangeError);
  assert.throws(() => c.setTrigger({ kind: 'sometimes' }), RangeError);
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
  for (const caps of [{ groupCap: 0 }, { memoryCap: 0 }, { areaCap: 0 }, { inboxCap: 0 }]) {
    assert.throws(() => lib.squad({ groupCap: 2, memoryCap: 2, areaCap: 2, inboxCap: 2, ...caps }), RangeError);
  }
  assert.throws(() => lib.squad({ groupCap: 2, memoryCap: 2, areaCap: 2, inboxCap: 2, metric: 'taxi' }), RangeError);
  const sq = lib.squad({ groupCap: 3, memoryCap: 4, areaCap: 4, inboxCap: 4, metric: 'chebyshev' });
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
  assert.equal(sq.resume(2, 0), true);
  assert.equal(sq.resume(9, 0), false);
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
  assert.equal(sq.setChannel(2, { kind: 'shout', range: 5 }), true);
  assert.equal(sq.setChannel(2, { kind: 'none' }), true);
  assert.equal(sq.setChannel(9, { kind: 'none' }), false);
  assert.throws(() => sq.setChannel(2, { kind: 'radio', base: -1, perUnit: 0 }), RangeError);
  assert.throws(() => sq.setChannel(2, { kind: 'pigeon' }), RangeError);
  sq.setExtraColumns(2);
  assert.throws(() => sq.setExtraColumns(-1), RangeError);
  sq.free();
});

test('squad memory: observe, report, targets, own knowledge and areas', () => {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 3, 0]]);
  const k = sq.knowledge();
  assert.equal(k, sq.knowledge(), 'the same wrapper every time');
  assert.equal(sq.observe(enemy(10, 20, 0, 5), 5), null);
  assert.deepEqual(k.get(10, ENEMY), { subject: 10, kind: ENEMY, pos: [20, 0, 0], seenAt: 5, source: 0, strength: 0, data: 0 });
  k.learn({ subject: 50, kind: 9, pos: [0, 0, 0], seenAt: 5 }, 5); // not a target
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
  sq.report({ kind: SQUAD.reportDanger, subject: 11, pos: [5, 5, 0], seenAt: 9, strength: 30, data: 1, sender: 2 }, 9);
  sq.check(9);
  assert.equal(a.count(), 1);
  assert.deepEqual(a.list()[0].pos, [5, 5, 0]);
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
  z.add({ kind: 1, pos: [0, 0, 0], radius: 100, lifetime: 255, blocking: true }, 0);
  z.add({ kind: 2, pos: [500, 0, 0], radius: 100, lifetime: 255, strength: 7 }, 0);
  const start = [1000, 0, 0];
  const goal = [2000, 0, 0];
  assert.equal(lib.areasMoveCost(z, [900, 0, 0], [10, 0, 0], start, goal), null);
  assert.equal(lib.areasMoveCost(z, [10, 0, 0], [20, 0, 0], start, goal), null);
  assert.equal(lib.areasMoveCost(z, [900, 0, 0], [500, 0, 0], start, goal), 7);
  assert.equal(lib.areasMoveCost(z, [10, 0, 0], [800, 0, 0], start, goal), 0, 'leaving is free');
  assert.equal(lib.areasMoveCost(z, [900, 0, 0], [10, 0, 0], [20, 0, 0], goal), 0, 'start inside');
  const s = lib.areas(2);
  s.add({ kind: 1, pos: [0, 0, 0], radius: 100, lifetime: 255, strength: 4294967295 }, 0);
  s.add({ kind: 2, pos: [0, 0, 0], radius: 100, lifetime: 255, strength: 9 }, 0);
  assert.equal(lib.areasMoveCost(s, [0, 0, 0], [10, 0, 0], start, goal), 2 ** 31 - 2, 'saturates as in C');
  z.free();
  assert.throws(() => lib.areasMoveCost(z, [0, 0, 0], [0, 0, 0], start, goal), /free/);
});

test('plans: goals, orders by channel, signal lines up the start, errors', () => {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 6, 8]], { radio: false });
  assert.equal(sq.setChannel(2, { kind: 'radio', base: 3, perUnit: 2 }), true);
  assert.equal(sq.plan(0), null, 'no tactic');
  assert.equal(sq.addTactic(lib.tactics.rush()), 0);
  assert.equal(sq.plan(0), null, 'no target');
  assert.deepEqual(sq.check(0), { status: 'noPlan', plan: null });
  sq.setSkill(255);
  sq.observe(enemy(10, 20, 0), 0);
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
  for (const bad of [-1, 2 ** 32, 1.5, NaN, '5']) assert.throws(() => sq.ordersDue(bad), RangeError, String(bad));
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

/** An elite squad of four with rush and a 2-side flank, enemy 10 at (20, 0), all hearing at once; the flank is active at 5. */
function activeFlank() {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 1, 0], [3, 0, 1], [4, 1, 1]], { radio: false });
  sq.addTactic(lib.tactics.rush());
  sq.addTactic(lib.tactics.flank(2, 4));
  sq.setSkill(255);
  sq.observe(enemy(10, 20, 0), 0);
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
  assert.equal(sq.resume(3, 30), true);
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
  sq.observe(enemy(10, 20, 0), 0);
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
  only.observe(enemy(10, 20, 0), 0);
  only.plan(0);
  const g = only.pendingPlan().goals[0];
  assert.deepEqual([g.pos, g.thenPos], [[20, 6, 0], [20, 2, 0]]);
  only.free();
  sq.free();
});

test('plan activation wraps around 2^32', () => {
  const sq = squadOf('euclid', [[1, 0, 0], [2, 1, 0], [3, 0, 1], [4, 1, 1]], { radio: false });
  sq.addTactic(lib.tactics.rush());
  sq.setSkill(255);
  const before = 4294967294;
  sq.observe(enemy(10, 20, 0, before), before);
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

test('keeper: postpone, broken, suspend never breaks, inputs', () => {
  const k = lib.keeper();
  assert.equal(k.goal(), null);
  k.postpone(0); // without a goal: nothing
  assert.equal(k.broken(100, 10), false);
  const goal = { kind: 'hold', pos: [3, 4, 0], target: 7, start: { kind: 'after', ticks: 2 }, slot: 1, plan: 5, issuedAt: 4294967290, startDelay: 1 };
  k.set(goal, 4294967290);
  assert.deepEqual(k.goal(), { member: null, kind: 'hold', custom: 0, pos: [3, 4, 0], unchecked: false, thenPos: null, thenUnchecked: false, target: 7, start: { kind: 'after', ticks: 2 }, slot: 1, plan: 5, issuedAt: 4294967290, startDelay: 1 });
  k.postpone(4294967295);
  k.postpone(3); // the first time stays
  assert.equal(k.broken(8, 10), false);
  assert.equal(k.broken(9, 10), true, 'wraps: 10 ticks since 2^32 - 1');
  k.suspend();
  for (const now of [9, 100, 4294967295, 0]) assert.equal(k.broken(now, 0), false, 'suspended: never broken');
  k.unsuspend(200);
  assert.equal(k.broken(209, 10), false, 'the clock restarts');
  assert.equal(k.broken(210, 10), true);
  k.resume(211);
  assert.equal(k.broken(500, 10), false);
  // inputs: has goal, distance, age, signal, discipline
  assert.deepEqual(k.inputs(4294967295, [0, 0, 0], 'euclid', true, 9), [1, 5, 5, 1, 9]);
  assert.deepEqual(k.inputs(0, [0, 0, 0], 'chebyshev', false, 0), [1, 4, 6, 0, 0]);
  k.clear();
  assert.deepEqual(k.inputs(0, [0, 0, 0], 'euclid', false, 0), [0, -1, 0, 0, 0]);
  assert.throws(() => k.set({ ...goal, kind: 'dance' }, 0), RangeError);
  assert.throws(() => k.set({ ...goal, slot: 256 }, 0), RangeError);
  assert.throws(() => k.set({ ...goal, start: { kind: 'after', ticks: -1 } }, 0), RangeError);
  assert.throws(() => k.inputs(0, [0, 0, 0], 'taxi', false, 0), RangeError);
  assert.throws(() => k.broken(-1, 0), RangeError);
  k.set({ kind: 'custom', custom: 42, pos: null }, 0);
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
  assert.equal(sq.observe(enemy(10, 20, 0), 0), null);
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

console.log(`${passed} tests passed`);
