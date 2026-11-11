// Node test of the WebAssembly build on tabletop-like situations.
// Run: js/run.sh  (builds the .wasm first)

import { readFile } from 'node:fs/promises';
import assert from 'node:assert/strict';
import { loadSagax, HEX, FORMATION } from './sagax.mjs';

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

console.log(`${passed} tests passed`);
