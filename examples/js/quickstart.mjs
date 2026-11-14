// Sagax from JavaScript: a way, a decision and a crew in a few lines.
// Run: node examples/js/quickstart.mjs  (after js/run.sh built the .wasm)

import { readFile } from 'node:fs/promises';
import { loadSagax } from '../../js/sagax.mjs';

const wasm = await readFile(new URL('../../target/wasm32-unknown-unknown/release/sagax_wasm.wasm', import.meta.url));
const lib = await loadSagax(wasm);

// A path.
const grid = lib.grid(10, 10, { preset: 'octile' });
const way = grid.findPath({ x: 0, y: 0 }, { x: 9, y: 0 });
console.log(way.status, way.cost);
grid.free();

// A choice.
const d = lib.decider();
const a = d.addOption(1, [{ input: 0, curve: 'linear', min: 0, max: 10 }]);
const b = d.addOption(1, [{ input: 0, curve: 'linear', min: 0, max: 10 }]);
const choice = d.decide([
  { option: a, inputs: [2] },
  { option: b, inputs: [8] },
]);
console.log('candidate', choice.index);
d.free();

// A crew.
const crew = lib.crew({ squad: { members: 1, memory: 4, areas: 2, inbox: 4, metric: 'chebyshev' } });
crew.addMember({ id: 1, pos: [0, 0, 0], skill: 128 });
crew.squad().setTargetKind(1);
crew.squad().addTactic(lib.tactics.rush());
crew.observe(0, { subject: 11, kind: 1, pos: [10, 0, 0], seenAt: 0 });
for (const event of crew.tick(0, { seed: 7 })) console.log('event', event.kind);
crew.free();
