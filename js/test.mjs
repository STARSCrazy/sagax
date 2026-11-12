// Node test of the WebAssembly build on tabletop-like situations.
// Run: js/run.sh  (builds the .wasm first)

import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { MEMORY, ONE, loadSagax, HEX, FORMATION, Pursuit, Radio, Interval } from './sagax.mjs';

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

test('sightings: seen, evict oldest, data kept, expire, wrap', () => {
  assert.throws(() => lib.sightings(0));
  const s = lib.sightings(2);
  assert.equal(s.seen(1, [10, 20, 0], 5), null);
  assert.equal(s.seen(2, [11, 21, 0], 6), null);
  assert.equal(s.seen(3, [12, 22, 0], 7), 1);
  assert.equal(s.list().length, 2);
  assert.equal(s.count(), 2);
  assert.equal(s.setData(3, 9), true);
  assert.equal(s.setData(99, 9), false);
  assert.equal(s.seen(3, [1, 2, 3], 8), null); // update keeps data
  assert.deepEqual(s.get(3), { id: 3, pos: [1, 2, 3], lastSeen: 8, data: 9 });
  assert.equal(s.get(1), null);
  assert.equal(s.setPos(3, [-4, 5, -6]), true);
  assert.deepEqual(s.get(3).pos, [-4, 5, -6]);
  assert.equal(s.touch(2, 20), true);
  assert.equal(s.touch(1, 20), false);
  assert.deepEqual(s.list().map((e) => e.id), [2, 3]);
  assert.deepEqual(s.expire(20, 10), [3]);
  assert.equal(s.remove(2), true);
  assert.equal(s.remove(2), false);
  s.seen(4, [0, 0, 0], 1);
  assert.equal(s.count(), 1);
  s.clear();
  assert.deepEqual(s.list(), []);
  assert.equal(s.count(), 0);
  // time wraps around 2^32
  s.seen(7, [0, 0, 0], 4294967290);
  assert.equal(s.get(7).lastSeen, 4294967290);
  assert.deepEqual(s.expire(4, 10), [7]); // age 10 across the wrap
  s.free();
});

test('zones: add, renew, contains, blocks, expire', () => {
  assert.throws(() => lib.zones(0, 10, 10));
  const z = lib.zones(2, 10, 50);
  assert.equal(z.add([100, 100, 0], 5, 7, [1, 2, 3]), 0);
  assert.equal(z.add([103, 100, 0], 8, 7), 0); // renews, drops the source
  assert.deepEqual(z.list(), [{ pos: [103, 100, 0], since: 8, data: 7, source: null }]);
  z.add([100, 100, 0], 9, 1, [1, 2, 3]);
  assert.deepEqual(z.list()[0].source, [1, 2, 3]);
  assert.equal(z.contains([100, 105, 0]), 0);
  assert.equal(z.contains([500, 0, 0]), null);
  assert.equal(z.blocks([100, 100, 0], [900, 900, 0], [900, 900, 0]), true);
  assert.equal(z.blocks([100, 100, 0], [100, 100, 0], [900, 900, 0]), false);
  assert.equal(z.add([500, 500, 0], 20, 2), 1);
  assert.equal(z.add([900, 900, 0], 30, 3), 1); // full: oldest goes, new one last
  assert.deepEqual(z.list().map((e) => e.data), [2, 3]);
  assert.equal(z.count(), 2);
  assert.equal(z.expire(100), 2);
  assert.deepEqual(z.list(), []);
  assert.equal(z.count(), 0);
  z.add([1, 1, 0], 1);
  assert.equal(z.count(), 1);
  z.clear();
  assert.equal(z.count(), 0);
  assert.deepEqual(z.list(), []);
  z.add([0, 0, 0], 4294967290, 1);
  assert.equal(z.expire(4), 0); // age 10 < 50 across the wrap
  assert.equal(z.expire(43), 0);
  assert.equal(z.expire(44), 1);
  z.free();
});

test('ignore: duration, renew, evict oldest', () => {
  assert.throws(() => lib.ignore(0));
  const i = lib.ignore(2);
  i.add(4, 10);
  assert.equal(i.ignores(4, 15, 10), true);
  assert.equal(i.ignores(4, 25, 10), false);
  assert.equal(i.ignores(5, 15, 10), false);
  i.add(5, 12);
  i.add(6, 14); // evicts 4 (oldest)
  assert.equal(i.ignores(4, 15, 10), false);
  assert.equal(i.ignores(6, 15, 10), true);
  i.clear();
  assert.equal(i.ignores(6, 15, 10), false);
  assert.equal(i.ignores(5, 15, 10), false);
  i.add(7, 20);
  assert.equal(i.ignores(7, 21, 10), true);
  i.free();
});

test('pursuit: lost, stalled, progress, wrap', () => {
  const p = new Pursuit();
  p.begin(7, [1, 2, 3], 50, 100);
  p.missed(5);
  assert.equal(p.lost(5), true);
  p.seen([4, 5, 6]);
  assert.deepEqual(p.lastPos, [4, 5, 6]);
  assert.equal(p.lost(5), false);
  assert.equal(p.stalled(50, 110, 5, 10), true);
  assert.equal(p.stalled(44, 111, 5, 10), false); // gain 6 restarts
  assert.equal(p.stalled(44, 120, 5, 10), false);
  assert.equal(p.stalled(44, 121, 5, 10), true);
  p.resetProgress(40, 130);
  assert.equal(p.stalled(40, 139, 5, 10), false);
  p.missed(4294967295);
  p.missed(5); // saturates
  assert.equal(p.unseen, 4294967295);
  p.clear();
  assert.equal(p.target, 0);
  const w = new Pursuit();
  w.begin(1, [0, 0, 0], 10, 4294967290);
  assert.equal(w.stalled(10, 4, 5, 10), true); // age 10 across the wrap
  assert.equal(w.stalled(10, 3, 5, 10), false);
});

test('radio: start, delay, take, cooldown, wrap', () => {
  const r = new Radio();
  assert.equal(r.start([1, 2, 3], 10, 100), true);
  assert.equal(r.start([1, 2, 3], 11, 100), false); // pending
  assert.equal(r.due(12, 5), false);
  assert.equal(r.due(15, 5), true);
  assert.deepEqual(r.take(), [1, 2, 3]);
  assert.equal(r.take(), null);
  assert.equal(r.start([1, 2, 3], 50, 100), false); // cooldown
  assert.equal(r.start([4, 5, 6], 110, 100), true);
  r.cancel();
  assert.equal(r.due(500, 0), false);
  assert.equal(r.start([4, 5, 6], 150, 100), false); // cooldown keeps running
  const w = new Radio();
  w.start([0, 0, 0], 4294967290, 10);
  w.take();
  assert.equal(w.start([0, 0, 0], 3, 10), false); // age 9
  assert.equal(w.start([0, 0, 0], 4, 10), true); // age 10
  assert.equal(w.due(8, 5), false);
  assert.equal(w.due(9, 5), true);
});

test('interval: never started is due, wrap', () => {
  const i = new Interval();
  assert.equal(i.due(0, 10), true);
  i.mark(5);
  assert.equal(i.due(10, 10), false);
  assert.equal(i.due(15, 10), true);
  i.reset();
  assert.equal(i.due(5, 10), true);
  i.mark(4294967290);
  assert.equal(i.due(3, 10), false); // age 9
  assert.equal(i.due(4, 10), true); // age 10
});

test('nearest: truncating division toward zero', () => {
  const c = [{ id: 1, x: -40, y: 3 }, { id: 2, x: -31, y: -31 }, { id: 3, x: 20, y: 0 }, { id: 4, x: -100, y: -17 }];
  assert.deepEqual(lib.nearest(c, [10, 5], 3, 16), [3, 2, 1]);
  assert.deepEqual(lib.nearest([], [0, 0], 3, 1), []);
  assert.deepEqual(lib.nearest(c, [0, 0], 0, 1), []);
  assert.deepEqual(lib.nearest([{ id: 5, x: 1, y: 1 }, { id: 6, x: -1, y: -1 }], [0, 0], 2, 1), [5, 6]); // ties keep order
});

test('sin256 matches the reference', () => {
  const want = [
    -226, -198, -162, -142, -98, -50, 0, 25, 74, 121, 162, 181, 213, 237, 251, 255, 255, 245,
    226, 213, 181, 142, 98, 74, 25, -25, -74, -98, -142, -181, -213, -226, -245, -255, -255,
    -251, -237, -213, -181, -162, -121, -74, -25, 0, 50, 98, 142, 162, 198, 226, 245, 251, 256,
    251, 237, 226, 198, 162, 121, 98, 50, 0, -50, -74, -121, -162, -198, -213, -237, -251,
    -256, -255, -245, -226, -198, -181, -142, -98, -50, -25, 25, 74, 121, 142, 181, 213,
  ];
  const got = [];
  for (let h = -300; h <= 300; h += 7) got.push(lib.sin256(h));
  assert.deepEqual(got, want);
});

test('searchPoint matches the reference', () => {
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
      assert.deepEqual(lib.searchPoint(agent, step, [c[0], c[1], 7]), [x, y, 7], `agent ${agent} step ${step} centre ${c}`);
    });
  }
  assert.equal(lib.searchPoint(0, 4, [0, 0, 0]), null);
  const p = { min: [100, 200], max: [1000, 300] };
  assert.deepEqual(lib.searchPoint(0, 1, [500, 250, 0], p), [500, 300, 0]);
  assert.deepEqual(lib.searchPoint(255, 3, [50, 250, 0], p), [100, 200, 0]);
  const own = { headingStep: 0, firstDistance: 256, offsets: [0], distances: [512], min: [0, 0], max: [32767, 32767] };
  assert.deepEqual(lib.searchPoint(0, 0, [1000, 1000, 0], own), [1000, 1256, 0]);
  assert.deepEqual(lib.searchPoint(0, 1, [1000, 1000, 0], own), [1000, 1512, 0]);
  assert.equal(lib.searchPoint(0, 2, [1000, 1000, 0], own), null);
  assert.throws(() => lib.searchPoint(0, 0, [0, 0, 0], { offsets: new Array(9).fill(0), distances: new Array(9).fill(0) }));
  // A missing distance counts as 0, like in Rust (no throw).
  const short = { headingStep: 0, firstDistance: 256, offsets: [0, 0], distances: [512], min: [0, 0], max: [32767, 32767] };
  assert.deepEqual(lib.searchPoint(0, 1, [1000, 1000, 0], short), [1000, 1512, 0]);
  assert.deepEqual(lib.searchPoint(0, 2, [1000, 1000, 0], short), [1000, 1000, 0]);
  assert.deepEqual(lib.searchPoint(0, 1, [1000, 1000, 0], { ...short, distances: [] }), [1000, 1000, 0]);
});

test('lookHeading matches the reference', () => {
  const want = [10, 10, 42, 42, 74, 74, 42, 42, 10, 10, 234, 234, 202, 202, 234, 234, 10, 10, 10];
  const got = [];
  for (let e = 0; e <= 180; e += 10) got.push(lib.lookHeading(10, e, 20));
  assert.deepEqual(got, want);
  assert.equal(lib.lookHeading(10, 25, 10, [0, 5, 7]), 17);
  assert.equal(lib.lookHeading(10, 35, 10, [0, 5, 7]), 10);
  assert.equal(lib.lookHeading(250, 0, 10, [10]), 4);
  assert.equal(lib.lookHeading(10, 1, 0, [1, 2]), 12); // step 0 counts as 1
});


test('scratch buffer survives memory growth', () => {
  const c = [];
  for (let i = 0; i < 200000; i++) c.push({ id: i, x: i, y: 0 });
  assert.deepEqual(lib.nearest(c, [150000, 0], 3, 1), [150000, 149999, 150001]);
  assert.deepEqual(lib.nearest(c.slice(0, 2), [0, 0], 1000000, 1), [0, 1]);
});

test('capacity is validated', () => {
  for (const bad of [0, -1, 1.5, 2 ** 31, NaN, '3']) {
    assert.throws(() => lib.sightings(bad), RangeError);
    assert.throws(() => lib.zones(bad, 1, 1), RangeError);
    assert.throws(() => lib.ignore(bad), RangeError);
  }
});

test('radio.take returns a copy', () => {
  const r = new Radio();
  r.start([1, 2, 3], 0, 0);
  const p = r.take();
  p[0] = 99;
  assert.deepEqual(r.pos, [1, 2, 3]);
});

test('pursuit.stalled at i32 extremes', () => {
  const p = new Pursuit();
  p.begin(1, [0, 0, 0], 2147483647, 0);
  assert.equal(p.stalled(-2147483648, 5, 2147483647, 10), false); // gain 4294967295, no overflow
  assert.equal(p.best, -2147483648);
  const q = new Pursuit();
  q.begin(1, [0, 0, 0], -2147483648, 0);
  assert.equal(q.stalled(2147483647, 10, -2147483648, 10), true); // gain -4294967295 is below minGain, so only the age counts
});

console.log(`${passed} tests passed`);
