// Sagax for JavaScript: grid path search compiled to WebAssembly.
// Works in the browser (Foundry) and in Node. No dependencies.
//
//   const lib = await loadSagax(url or bytes);
//   const grid = lib.grid(30, 20, { preset: 'tabletop' });             // square, a diagonal step costs 1
//   const hexGrid = lib.grid(30, 20, { preset: 'tabletop', hex: HEX.oddR });
//   grid.block(5, 5); grid.addWall(3, 3, 1, 0); grid.addZone(4, 4, 0); grid.setLeaveCost(100);
//   const way = grid.findPath({x: 0, y: 0}, {x: 10, y: 4});  // {cells: [{x, y, level}], status, reached, cost, ...}
//   const area = grid.reachable({x: 0, y: 0}, 6);            // {cells: [{x, y, level, cost}], ...}
//   grid.free();
//
// Positions are {x, y}, {x, y, level} (or {x, y, z}) or [x, y, level] (level 0
// if left out); the loose calls (setCost, contains, pathTo, ...) take x, y or
// one position. Choices are names ('ranged', 'cover', 'told', ...) or numbers.
// Numbers are checked: a bad number throws a RangeError, a wrong type a
// TypeError. Handles (Grid, Decider, ...) throw after free(), and free in
// `using` blocks where the runtime has Symbol.dispose.
//
// Every search answers with the same object:
//   {cells, status, reached, cost, steps, zonesLeft, total, expanded, nodeLimitHit}
//   cost = steps (walking and terrain) + zonesLeft * leave cost
//   status: 'idle' (continue before any search), 'running' (budget used up, call grid.continue()), 'found',
//   'partial' (goal unreachable, way to the closest cell), 'unreachable',
//   'done' (area complete). Ways always begin with the start.

const BLOCKED = 0xffffffff;
const DIRECTIONS = [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [1, -1], [-1, 1], [-1, -1]];
const HEX_WALLS_CHANGED = 'Sagax: hex wall bits changed in 0.9 (offset bits); use addHexWall';
const STATUS = ['idle', 'running', 'found', 'partial', 'unreachable', 'done'];
const OUTPUT = { cells: 0, turns: 1, smooth: 2 };
const TRACE = [null, 'started', 'expanded', 'improved', 'paused', 'finished'];

/** Ready-made costs for rules.preset. There is no default: every game picks one. */
const PRESETS = {
  octile: { straight: 10, diagonal: 14, step: 10 }, // close to the real length (1.4)
  tabletop: { straight: 1, diagonal: 1, step: 1 }, // every step counts one square
};

/** The full rules of lib.grid: a preset, explicit costs, or both (explicit wins). */
function rulesOf(rules) {
  const preset = rules.preset === undefined ? {} : PRESETS[rules.preset];
  if (preset === undefined) throw new RangeError(`Sagax: preset must be octile or tabletop, not ${rules.preset}`);
  const r = { fourWays: false, cutCorners: false, levels: 1, ...preset, ...rules };
  r.hex = r.hex === undefined || r.hex === null ? null : layoutOf(r.hex);
  const own = r.hex === null ? rules.straight : rules.step;
  if (rules.preset === undefined && own === undefined) {
    throw new TypeError(
      "Sagax: lib.grid needs rules: { preset: 'octile' | 'tabletop' } or explicit costs (straight and diagonal, on hex step)",
    );
  }
  return r;
}

/** Hex layouts for rules.hex (Foundry grid types 2..5 in this order). */
export const HEX = { oddR: 1, evenR: 2, oddQ: 3, evenQ: 4 };

/** Formation slots [forward, right]. On hex grids 'right' is 60 degrees clockwise. */
export const FORMATION = {
  block: [[0, 1], [-1, 0], [-1, 1]],
  column: (n) => Array.from({ length: n }, (_, i) => [-(i + 1), 0]),
  wedge: (n) => Array.from({ length: n }, (_, i) => { const r = (i >> 1) + 1; return [-r, i % 2 ? r : -r]; }),
  wedgeHex: (n) => Array.from({ length: n }, (_, i) => { const r = (i >> 1) + 1; return i % 2 ? [-r, r] : [0, -r]; }),
};

/**
 * Loads the WebAssembly module.
 * @param {string|URL|ArrayBuffer|Uint8Array|Response} source  URL (browser) or bytes
 */
export async function loadSagax(source) {
  let instance;
  if (source instanceof ArrayBuffer || ArrayBuffer.isView(source)) {
    ({ instance } = await WebAssembly.instantiate(source, {}));
  } else {
    const response = source instanceof Response ? source : await fetch(source);
    if (WebAssembly.instantiateStreaming && response.headers.get('content-type')?.includes('wasm')) {
      ({ instance } = await WebAssembly.instantiateStreaming(response, {}));
    } else {
      ({ instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {}));
    }
  }
  loaded = instance.exports;
  return new Sagax(instance.exports);
}

/** The module the value classes (Progress, Interval, Alertness) call into: the one loaded last. */
let loaded = null;

/** The exports of the loaded module, for the value classes. */
function wasm() {
  if (loaded === null) throw new Error('Sagax: call loadSagax before using Progress, Interval or Alertness');
  return loaded;
}

/**
 * Every handle class: the module, the handle and one guard. A freed handle
 * would write to address 0, so every call goes through `h`, which throws.
 */
class Handle {
  get h() {
    if (!this.handle) throw new Error('Sagax: used after free()');
    return this.handle;
  }
}

if (Symbol.dispose) {
  Object.defineProperty(Handle.prototype, Symbol.dispose, {
    value() { this.free(); },
    writable: true,
    configurable: true,
  });
}

class Sagax {
  constructor(exports) {
    this.x = exports;
  }

  /**
   * A grid of open cells on `levels` levels (1) with fixed rules, given as
   * a preset {preset: 'octile' | 'tabletop'} and/or explicit costs: square
   * {straight, diagonal, fourWays, cutCorners} or hex {hex: HEX.*, step}.
   * Without either it throws a TypeError.
   * smoothMaxExtra: smoothing may cross cells with at most this extra cost (0).
   */
  grid(width, height, rules = {}) {
    return new Grid(this.x, width, height, rulesOf(rules));
  }

  /** A decider (utility AI) without options; see Decider. */
  decider() {
    return new Decider(this.x);
  }

  /**
   * A state machine: machine(['patrol', 'chase', 'search'], ['seen', 'hit'], 'patrol').
   * States and conditions by name (at most 64 conditions); see Machine.
   */
  machine(states, conditions = [], initial = states[0]) {
    return new Machine(this.x, states, conditions, initial);
  }
}

function outputOf(output) {
  const o = OUTPUT[output ?? 'cells'];
  if (o === undefined) throw new RangeError(`Sagax: output must be cells, turns or smooth, not ${output}`);
  return o;
}

/** Positions as one flat list of x, y, level triples (the WebAssembly input). */
function triples(list, what) {
  if (!Array.isArray(list)) throw new TypeError(`Sagax: ${what}s must be an array of positions`);
  return list.flatMap((p) => posOf(p, what));
}

/** Loose (x, y, ...rest) or (position, ...rest) as [[x, y, level], ...rest]. */
function cellArgs(args) {
  if (args[0] !== null && typeof args[0] === 'object') return [posOf(args[0], 'cell'), ...args.slice(1)];
  return [[i32Of(args[0], 'x'), i32Of(args[1], 'y'), 0], ...args.slice(2)];
}

export class Grid extends Handle {
  constructor(x, width, height, rules) {
    super();
    this.width = intIn(width, 1, I32_MAX, 'width');
    this.height = intIn(height, 1, I32_MAX, 'height');
    this.levels = intIn(rules.levels, 1, I32_MAX, 'levels');
    this.x = x;
    this.rules = rules;
    this.hex = rules.hex;
    const flags = (rules.fourWays ? 1 : 0) | (rules.cutCorners ? 2 : 0);
    const handle = rules.hex === null
      ? x.sx_grid_new(this.width, this.height, this.levels, 0, i32Of(rules.straight, 'straight'), i32Of(rules.diagonal ?? 0, 'diagonal'), flags)
      : x.sx_grid_new(this.width, this.height, this.levels, rules.hex, i32Of(rules.step, 'step'), 0, 0);
    if (!handle) throw new RangeError('Sagax: bad grid size or rules');
    this.handle = handle;
    if (rules.smoothMaxExtra !== undefined) x.sx_grid_set_smooth_max_extra(handle, i32Of(rules.smoothMaxExtra, 'smoothMaxExtra'));
  }

  // Views are made fresh for every use: they die when the WebAssembly memory grows.
  // Each has width * height * levels entries, level by level and row by row:
  // (level * height + y) * width + x.
  costs() {
    return new Uint32Array(this.x.memory.buffer, this.x.sx_grid_costs(this.h), this.#cells());
  }
  /** Raw wall bits, square grids only: bit i blocks the move by DIRECTIONS[i]. */
  walls() {
    if (this.hex !== null) throw new TypeError(HEX_WALLS_CHANGED);
    return this.#wallBits();
  }
  #wallBits() {
    return new Uint8Array(this.x.memory.buffer, this.x.sx_grid_walls(this.h), this.#cells());
  }
  zones() {
    return new Uint32Array(this.x.memory.buffer, this.x.sx_grid_zones(this.h), this.#cells());
  }
  #cells() {
    return this.width * this.height * this.levels;
  }
  /** Index of [x, y, level] in the views, or -1 off the grid. */
  #index([x, y, level]) {
    const inside = x >= 0 && y >= 0 && x < this.width && y < this.height && level >= 0 && level < this.levels;
    return inside ? (level * this.height + y) * this.width + x : -1;
  }

  /** Bulk filling, in the order of the views (width * height * levels entries each). */
  setCosts(array) { this.costs().set(array); }
  setWalls(array) { this.walls().set(array); } // square grids only
  setAllZones(array) { this.zones().set(array); }

  /** Whether a cell is on the grid: contains(x, y) (level 0) or contains({x, y, level}). */
  contains(...args) {
    return this.#index(cellArgs(args)[0]) >= 0;
  }

  /**
   * Extra cost of entering a cell (on top of the step cost):
   * setCost(x, y, cost) on level 0, or setCost({x, y, level}, cost).
   */
  setCost(...args) {
    const [cell, cost] = cellArgs(args);
    const c = u32Of(cost, 'cost');
    const i = this.#index(cell);
    if (i >= 0) this.costs()[i] = c;
  }
  /** block(x, y) on level 0, or block({x, y, level}). */
  block(...args) {
    const [cell] = cellArgs(args);
    this.setCost(cell, BLOCKED);
  }

  /**
   * Links two neighbouring cells, usually on different levels (a stair, a
   * ramp): a move from a toward b arrives on b's level, and back. Walls and
   * blocked cells still apply. false (nothing changed) unless they are
   * neighbours on the grid.
   */
  link(a, b) {
    const [ax, ay, al] = posOf(a, 'a');
    const [bx, by, bl] = posOf(b, 'b');
    return this.x.sx_grid_link(this.h, ax, ay, al, bx, by, bl) === 1;
  }

  /**
   * A wall between a cell and its neighbour dx, dy away (both ways), on the
   * cell's level: addWall(x, y, dx, dy) or addWall({x, y, level}, dx, dy).
   * dx, dy is a step of one in each direction (on hex grids the neighbour's
   * offset), else RangeError.
   */
  addWall(...args) {
    const [[x, y, level], dx, dy] = cellArgs(args);
    const there = DIRECTIONS.findIndex(([a, b]) => a === dx && b === dy);
    if (there < 0) throw new RangeError(`Sagax: a wall goes to a neighbour (dx, dy from -1 to 1, not both 0), not ${String(dx)}, ${String(dy)}`);
    const back = DIRECTIONS.findIndex(([a, b]) => a === -dx && b === -dy);
    const walls = this.#wallBits();
    const here = this.#index([x, y, level]);
    const next = this.#index([x + dx, y + dy, level]);
    if (here >= 0) walls[here] |= 1 << there;
    if (next >= 0) walls[next] |= 1 << back;
  }

  /** Hex grids: the six neighbours of a cell, hexNeighbors(x, y) or hexNeighbors({x, y}); the index is the direction (as for addHexWall). */
  hexNeighbors(...args) {
    const [[x, y]] = cellArgs(args);
    if (this.x.sx_hex_neighbors(this.h, x, y) !== 6) throw new RangeError('Sagax: not a hex grid');
    return this.readCells(6, 2);
  }

  /** Hex grids: a wall between a cell and its neighbour in direction dir 0..5 (both ways): addHexWall(x, y, dir) or addHexWall({x, y, level}, dir). */
  addHexWall(...args) {
    const [cell, dir] = cellArgs(args);
    intIn(dir, 0, 5, 'dir');
    const n = this.hexNeighbors(cell)[dir];
    this.addWall(cell, n.x - cell[0], n.y - cell[1]);
  }

  /** Zone bits of a cell (bit i: inside zone i, e.g. the reach of enemy i): setZones(x, y, bits) or setZones({x, y, level}, bits). */
  setZones(...args) {
    const [cell, bits] = cellArgs(args);
    const b = u32Of(bits, 'bits');
    const i = this.#index(cell);
    if (i >= 0) this.zones()[i] = b;
  }
  /** Puts a cell into zone 0..31: addZone(x, y, zone) or addZone({x, y, level}, zone). */
  addZone(...args) {
    const [cell, zone] = cellArgs(args);
    const z = intIn(zone, 0, 31, 'zone');
    const i = this.#index(cell);
    if (i >= 0) this.zones()[i] |= (1 << z) >>> 0;
  }
  clearZones() {
    this.zones().fill(0);
  }

  /** Cost added for each zone a move leaves (e.g. an attack of opportunity). */
  setLeaveCost(cost) {
    this.leaveCost = u32Of(cost, 'cost');
    this.x.sx_grid_set_leave_cost(this.h, this.leaveCost);
  }

  /**
   * Zones the way leaves, cell by cell (each one e.g. an attack of
   * opportunity). cells: the whole way (output 'cells').
   */
  zonesLeft(cells) {
    const zones = this.zones();
    let left = 0;
    const at = (c) => zones[this.#index([c.x, c.y, c.level ?? 0])] ?? 0;
    for (let i = 1; i < cells.length; i++) {
      const a = at(cells[i - 1]);
      const b = at(cells[i]);
      let bits = (a & ~b) >>> 0;
      while (bits) { bits = (bits & (bits - 1)) >>> 0; left++; }
    }
    return left;
  }

  /** stride 3: cells {x, y, level}; 4: {x, y, level, cost}; 2: {x, y} (hex neighbours). */
  readCells(count, stride) {
    const out = new Int32Array(this.x.memory.buffer, this.x.sx_out(this.h), count * stride);
    const cells = new Array(count);
    for (let i = 0; i < count; i++) {
      const o = i * stride;
      if (stride === 4) cells[i] = { x: out[o], y: out[o + 1], level: out[o + 2], cost: out[o + 3] };
      else if (stride === 3) cells[i] = { x: out[o], y: out[o + 1], level: out[o + 2] };
      else cells[i] = { x: out[o], y: out[o + 1] };
    }
    return cells;
  }

  writeInput(numbers) {
    const ptr = this.x.sx_input(this.h, numbers.length);
    new Int32Array(this.x.memory.buffer, ptr, numbers.length).set(numbers);
  }

  answer(n, stride, split = false) {
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    const r = new Int32Array(this.x.memory.buffer, this.x.sx_result(this.h), 5);
    const status = STATUS[r[0]];
    const cells = this.readCells(n, stride);
    // Ways: split the cost into walking (steps and terrain) and zone penalty.
    let zonesLeft = 0;
    if (split && this.leaveCost && n > 1) {
      const whole = this.x.sx_way(this.h, 0);
      if (whole > 1) zonesLeft = this.zonesLeft(this.readCells(whole, 3));
    }
    return {
      cells,
      status,
      reached: status === 'found',
      cost: r[2],
      steps: r[2] - zonesLeft * (this.leaveCost ?? 0),
      zonesLeft,
      total: r[1],
      expanded: r[3],
      nodeLimitHit: r[4] === 1,
    };
  }

  /**
   * Cheapest way from start to goal, or to the cheapest of several goals
   * (an array, e.g. every exit of the map). If no goal can be reached, the
   * way ends at the reachable cell closest to one (status 'partial').
   * @param {{budget?: number, output?: 'cells'|'turns'|'smooth'}} options  budget 0: no limit
   */
  findPath(start, goal, { budget = 0, output = 'cells' } = {}) {
    // one goal may itself be an array position [x, y, level]
    const goals = Array.isArray(goal) && typeof goal[0] !== 'number' ? goal : [goal];
    const [sx, sy, sl] = posOf(start, 'start');
    const b = intIn(budget, 0, I32_MAX, 'budget');
    const o = outputOf(output);
    this.writeInput(triples(goals, 'goal'));
    this.last = 3;
    return this.answer(this.x.sx_find_path(this.h, sx, sy, sl, goals.length, b, o), 3, true);
  }

  /**
   * A way away from threats ([{x, y}]): to the cell within maxCost that the
   * nearest threat needs the longest walk to reach. Status 'found', or
   * 'unreachable' (only the start) if nothing is farther than here. To flee
   * off the map, use findPath with the exits as goals.
   */
  flee(start, threats, maxCost, { output = 'cells' } = {}) {
    const [sx, sy, sl] = posOf(start, 'start');
    const m = intIn(maxCost, 0, I32_MAX, 'maxCost');
    const o = outputOf(output);
    this.writeInput(triples(threats, 'threat'));
    this.last = 3;
    return this.answer(this.x.sx_flee(this.h, sx, sy, sl, threats.length, m, o), 3, true);
  }

  /** Every cell reachable from start at a cost of at most maxCost: cells [{x, y, level, cost}]. */
  reachable(start, maxCost, { budget = 0 } = {}) {
    const [sx, sy, sl] = posOf(start, 'start');
    const m = intIn(maxCost, 0, I32_MAX, 'maxCost');
    const b = intIn(budget, 0, I32_MAX, 'budget');
    this.last = 4;
    return this.answer(this.x.sx_reachable(this.h, sx, sy, sl, m, b), 4);
  }

  /** After status 'running': goes on with the last findPath / reachable. */
  continue(budget = 0) {
    const stride = this.last ?? 3;
    return this.answer(this.x.sx_continue(this.h, intIn(budget, 0, I32_MAX, 'budget')), stride, stride === 3);
  }

  /** After reachable() or findPath(): the way from that start to a cell, pathTo(x, y, output) or pathTo({x, y, level}, output), or null. */
  pathTo(...args) {
    const [[x, y, level], output = 'cells'] = cellArgs(args);
    const a = this.answer(this.x.sx_path_to(this.h, x, y, level, outputOf(output)), 3, true);
    return a.reached ? a : null;
  }

  /** The current way once more, as 'cells', 'turns' or 'smooth'. */
  way(output = 'cells') {
    const n = this.x.sx_way(this.h, outputOf(output));
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    return this.readCells(n, 3);
  }

  /**
   * Along the current way (the leader's): a target per member for every cell.
   * slots: [[forward, right], ...] relative to the walking direction
   * (default: 2 x 2 block; on hex 'right' is 60 degrees clockwise, so the
   * default is a compact group of four; FORMATION.wedgeHex is the hex wedge).
   * @returns {{leader: {x,y,level}[], members: {x,y,level}[][]}}
   */
  formation(slots = FORMATION.block) {
    if (!Array.isArray(slots)) throw new TypeError('Sagax: slots must be an array of [forward, right]');
    this.writeInput(slots.flatMap((slot) => {
      if (!Array.isArray(slot) || slot.length !== 2) throw new RangeError('Sagax: a formation slot is [forward, right]');
      return slot.map((v) => i32Of(v, 'slot'));
    }));
    const n = this.x.sx_formation(this.h, slots.length);
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    const rows = this.readCells(n * (slots.length + 1), 3);
    const members = slots.map((_, k) => rows.slice((k + 1) * n, (k + 2) * n));
    return { leader: rows.slice(0, n), members };
  }

  /**
   * Flow field: one search for many walkers heading to the nearest of `goals`.
   * Afterwards flowNext answers for any cells. Exact when steps cost the same
   * both ways; zones (leave cost) make the costs approximate. Keeps the
   * current way. @returns {number} cells in the field
   */
  buildFlow(goals, maxCost = 0x7fffffff) {
    const m = intIn(maxCost, 0, I32_MAX, 'maxCost');
    this.writeInput(triples(goals, 'goal'));
    const n = this.x.sx_flow_build(this.h, goals.length, m);
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    return n;
  }

  /**
   * For each walker the next cell towards the nearest goal and the walker's
   * cost to it: [{x, y, level, cost}]. At a goal, outside the field (cost -1)
   * or stuck the walker's own cell.
   */
  flowNext(walkers) {
    this.writeInput(triples(walkers, 'walker'));
    if (this.x.sx_flow_next(this.h, walkers.length) < 0) throw new RangeError('Sagax: bad arguments');
    return this.readCells(walkers.length, 4);
  }

  /** Cost from a cell, flowCost(x, y) or flowCost({x, y, level}), to the nearest goal of the flow field, or null. */
  flowCost(...args) {
    const c = this.flowNext([cellArgs(args)[0]])[0].cost;
    return c < 0 ? null : c;
  }

  /**
   * D* Lite: a way that is repaired when the grid changes.
   *   grid.dstarBegin(start, goal); grid.dstarCompute();   // {status: 'found', cells, ...}
   *   ... grid.block(5, 5); grid.dstarCellsChanged([{x: 5, y: 5}]);
   *   grid.dstarMoveStart(cell); grid.dstarCompute();
   * Needs steps that are possible both ways (no zones).
   */
  dstarBegin(start, goal) {
    this.x.sx_dstar_begin(this.h, ...posOf(start, 'start'), ...posOf(goal, 'goal'));
  }

  /** Works on the plan; on 'found' the way from the walker to the goal. */
  dstarCompute({ budget = 0, output = 'cells' } = {}) {
    return this.answer(this.x.sx_dstar_compute(this.h, intIn(budget, 0, I32_MAX, 'budget'), outputOf(output)), 3);
  }

  dstarMoveStart(cell) {
    this.x.sx_dstar_move_start(this.h, ...posOf(cell, 'cell'));
  }

  /** Tell D* Lite which cells changed (after setCost / block / addWall). */
  dstarCellsChanged(cells) {
    this.writeInput(triples(cells, 'cell'));
    return this.x.sx_dstar_cells_changed(this.h, cells.length);
  }

  /**
   * Tracing: every step of the following searches (findPath, reachable,
   * buildFlow, dstarCompute) is kept until takeTrace(). Off by default; off
   * it costs nothing.
   */
  setTrace(on) {
    this.x.sx_trace(this.h, on ? 1 : 0);
  }

  /**
   * The steps since the last call: [{kind, x, y, parent, cost, estimate, expanded, found}],
   * kind 'started' | 'expanded' | 'improved' | 'paused' | 'finished'.
   * dropped counts steps lost because more than 200000 were waiting.
   */
  takeTrace() {
    const n = this.x.sx_trace_take(this.h);
    const dropped = this.x.sx_trace_dropped(this.h);
    const raw = new Int32Array(this.x.memory.buffer, this.x.sx_out(this.h), n * 9);
    const events = new Array(n);
    for (let i = 0; i < n; i++) {
      const o = i * 9;
      events[i] = {
        kind: TRACE[raw[o]],
        x: raw[o + 1],
        y: raw[o + 2],
        parent: { x: raw[o + 3], y: raw[o + 4] },
        cost: raw[o + 5],
        estimate: raw[o + 6],
        expanded: raw[o + 7],
        found: raw[o + 8] === 1,
      };
    }
    return { events, dropped };
  }

  free() {
    if (this.handle) this.x.sx_grid_free(this.handle);
    this.handle = 0;
  }
}

/** 1.0 in the fixed point of scores (0..1 = 0..ONE). */
export const ONE = 65535;
const CURVE = {
  linear: 0, inverse: 1, quadratic: 2, inverseQuadratic: 3, step: 4, inverseStep: 5,
  sCurve: 6, inverseSCurve: 7, root: 8, cubic: 9,
};
/** Inputs read from the decider's memory instead of the candidate's row. */
export const MEMORY = {
  sinceOption: 0xfffffff0, // time since this option was done (never: 2^31 - 1)
  sincePair: 0xfffffff1, // ... on this target
  countOption: 0xfffffff2, // how often within the window
  countPair: 0xfffffff3, // ... on this target
};
const COMBINE = { product: 0, geometricMean: 1, minimum: 2, average: 3, median: 4 };
/** `option` of Decider.setCombine for every option (the same as leaving it out). */
export const ALL_OPTIONS = -1;
const APPLY = { combine: 0, multiply: 1 };
const fixed = (v, what) => {
  const n = Math.round(v * ONE);
  if (!(n >= 0 && n <= 0xffffffff)) throw new RangeError(`Sagax: bad ${what} ${v}`);
  return n;
};
const combineOf = (name) => {
  const c = COMBINE[name];
  if (c === undefined) throw new RangeError(`Sagax: unknown combine ${name}`);
  return c;
};
const PICK = { best: 0, weighted: 1, uniform: 2 };
const DECIDE_TRACE = [null, 'factor', 'scored', 'chosen'];

/**
 * Utility AI: options with considerations, scored per candidate (option x target).
 * All integer, deterministic; the seed comes from the caller.
 *
 *   const d = lib.decider();
 *   const attack = d.addOption(1, [{ input: 0, curve: 'inverse', min: 1, max: 10 }]);
 *   const c = d.decide([{ option: attack, target: 7, inputs: [3] }], { pick: 'best' });
 *   // c: {index, option, target, score, scores} or null if every score is 0
 */
export class Decider extends Handle {
  constructor(x) {
    super();
    this.x = x;
    this.handle = x.sx_decider_new();
    this.inputCount = 0;
  }

  /**
   * Adds an option and returns its number. weight: 1 = normal (more allowed).
   * considerations: [{input, curve, min, max}], curve one of linear, inverse,
   * quadratic, inverseQuadratic, step, inverseStep. t = (input - min) / (max - min)
   * clamped; the factors are multiplied (0 rules out), compensated for their number.
   */
  addOption(weight = 1, considerations = [], { combine, rank } = {}) {
    const option = this.x.sx_decider_add_option(this.h, fixed(weight, 'weight'));
    for (const c of considerations) this.addConsideration(option, c);
    if (combine !== undefined) this.setCombine(combine, option);
    if (rank !== undefined) this.setRank(option, rank);
    return option;
  }

  /**
   * Adds one consideration {input, curve, min, max, pull, apply} to an option.
   * pull (default 1): how much it counts; the factor v becomes 1 - pull * (1 - v),
   * so 0.5 is gentle (never below 0.5), 2 strict, 0 off. apply: 'combine'
   * (default, goes into the option's combine) or 'multiply' (multiplied onto the
   * result afterwards: a true factor, e.g. a hard rule).
   */
  addConsideration(option, { input, curve = 'linear', min = 0, max = ONE, pull = 1, apply = 'combine' }) {
    const k = CURVE[curve];
    if (k === undefined) throw new RangeError(`Sagax: unknown curve ${curve}`);
    const a = APPLY[apply];
    if (a === undefined) throw new RangeError(`Sagax: apply must be combine or multiply, not ${apply}`);
    const o = u32Of(option, 'option');
    const column = u32Of(input, 'input');
    const r = this.x.sx_decider_add_weighted(this.h, o, column, k, i32Of(min, 'min'), i32Of(max, 'max'), fixed(pull, 'pull'), a);
    if (r !== 0) throw new RangeError(`Sagax: no option ${option}`);
    if (column < MEMORY.sinceOption) this.inputCount = Math.max(this.inputCount, column + 1);
  }

  /**
   * Memory of actions for cooldowns and dislike. Time is the game's own count
   * (frames, turns); setNow before deciding, record what really happened.
   */
  setNow(now) {
    this.x.sx_decider_set_now(this.h, timeOf(now));
  }
  record(now, { option, target = 0 }) {
    const t = timeOf(now);
    this.x.sx_decider_record(this.h, u32Of(option, 'option'), u32Of(target, 'target'), t);
  }
  forget() {
    this.x.sx_decider_forget(this.h);
  }
  /** Counts look back less than time (0: all of the last 256 actions). */
  setWindow(time) {
    this.x.sx_decider_set_window(this.h, u32Of(time, 'window'));
  }
  /** After the option was done it scores 0 until time has passed (0: none). */
  setCooldown(option, time) {
    if (this.x.sx_decider_set_cooldown(this.h, u32Of(option, 'option'), u32Of(time, 'cooldown')) !== 0) {
      throw new RangeError(`Sagax: no option ${option}`);
    }
  }

  /**
   * How factors are put together: 'product' (default), 'geometricMean',
   * 'minimum', 'average' or 'median'. Without option: for every option;
   * with option: that one only (null: follow the decider again). The option
   * is ALL_OPTIONS or a valid option number, nothing else.
   */
  setCombine(combine, option) {
    const c = combine === null ? -1 : combineOf(combine);
    const o = option === undefined || option === null || option === ALL_OPTIONS ? -1 : intIn(option, 0, I32_MAX, 'option');
    if (this.x.sx_decider_set_combine(this.h, o, c) !== 0) {
      throw new RangeError('Sagax: bad combine or option');
    }
  }

  /** Rank of an option: only the highest rank that still scores is chosen from. */
  setRank(option, rank) {
    if (this.x.sx_decider_set_rank(this.h, u32Of(option, 'option'), u32Of(rank, 'rank')) !== 0) {
      throw new RangeError(`Sagax: no option ${option}`);
    }
  }

  /** The action going on now ({option, target} or null) scores times (1 + bonus). */
  setCurrent(current, bonus = 0.1) {
    const has = current ? 1 : 0;
    const option = has ? u32Of(current.option, 'option') : 0;
    const target = has ? u32Of(current.target ?? 0, 'target') : 0;
    this.x.sx_decider_set_current(this.h, has, option, target, fixed(bonus, 'bonus'));
  }

  /**
   * Scores every candidate {option, target, inputs: [raw integers]} and chooses one.
   * pick: 'best' (equal scores: the earlier one), 'weighted' or 'uniform' among the
   * top best (0: all), driven by seed (a safe integer, or a BigInt for 64 bits). Returns
   * {index, option, target, score, scores} or null if every score is 0.
   */
  decide(candidates, { pick = 'best', top = 0, seed = 0 } = {}) {
    const p = PICK[pick];
    if (p === undefined) throw new RangeError(`Sagax: pick must be best, weighted or uniform, not ${pick}`);
    if (!Array.isArray(candidates)) throw new TypeError('Sagax: candidates must be an array');
    const stride = this.inputCount;
    const row = stride + 2;
    const n = candidates.length;
    const topN = u32Of(top, 'top');
    const [low, high] = seedOf(seed);
    // Checked first: the buffer is asked for only when every number is good.
    const values = candidates.flatMap((c) => {
      const inputs = c.inputs ?? [];
      return [u32Of(c.option, 'option'), u32Of(c.target ?? 0, 'target'), ...Array.from({ length: stride }, (_, k) => i32Of(inputs[k] ?? 0, 'input'))];
    });
    const ptr = this.x.sx_decider_input(this.h, n * row);
    new Int32Array(this.x.memory.buffer, ptr, n * row).set(values);
    const r = this.x.sx_decider_decide(this.h, n, stride, p, topN, low, high);
    if (r === -1) throw new RangeError('Sagax: bad candidates (unknown option?)');
    const scores = Array.from(new Uint32Array(this.x.memory.buffer, this.x.sx_decider_out(this.h), n));
    if (r < 0) return null;
    return { index: r, option: candidates[r].option, target: candidates[r].target ?? 0, score: scores[r], scores };
  }

  /** Tracing of decisions on or off. */
  setTrace(on) {
    this.x.sx_decider_trace(this.h, on ? 1 : 0);
  }

  /**
   * Events since the last call: [{kind, candidate, consideration, input, value, score}],
   * kind 'factor' (value: factor of the raw input), 'scored' (value: product before
   * the weight) or 'chosen'.
   */
  takeTrace() {
    const n = this.x.sx_decider_trace_take(this.h);
    const raw = new Int32Array(this.x.memory.buffer, this.x.sx_decider_out(this.h), n * 6);
    const events = new Array(n);
    for (let i = 0; i < n; i++) {
      const o = i * 6;
      events[i] = {
        kind: DECIDE_TRACE[raw[o]],
        candidate: raw[o + 1],
        consideration: raw[o + 2],
        input: raw[o + 3],
        value: raw[o + 4] >>> 0,
        score: raw[o + 5] >>> 0,
      };
    }
    return events;
  }

  free() {
    if (this.handle) this.x.sx_decider_free(this.handle);
    this.handle = 0;
  }
}

const REASON = [null, 'transition', 'timeout', 'set'];

/**
 * A state machine with timers and aborts. Every tick: update(now, {seen: true}).
 * Transitions are checked in the order added (put aborts first), then the
 * state's timeout; at most one change per update.
 *
 *   const m = lib.machine(['patrol', 'chase', 'search', 'dead'], ['seen', 'killed']);
 *   m.transition({ from: '*', to: 'dead', when: ['killed'] });       // abort
 *   m.transition({ from: 'patrol', to: 'chase', when: ['seen'] });
 *   m.transition({ from: 'chase', to: 'search', unless: ['seen'], after: 10 });
 *   m.timeout('search', 300, 'patrol');
 *   const change = m.update(frame, { seen: canSee });   // {from, to, reason, transition} or null
 */
export class Machine extends Handle {
  constructor(x, states, conditions, initial) {
    super();
    if (!Array.isArray(states) || !Array.isArray(conditions)) throw new TypeError('Sagax: states and conditions are arrays of names');
    if (conditions.length > 64) throw new RangeError('Sagax: at most 64 conditions');
    this.x = x;
    this.states = states;
    this.conditions = conditions;
    this.handle = x.sx_machine_new(states.length, this.stateOf(initial));
    if (!this.handle) throw new RangeError('Sagax: bad states');
  }

  stateOf(name) {
    if (typeof name !== 'number' && typeof name !== 'string') throw new TypeError(`Sagax: a state is a name or a number, not ${typeof name}`);
    const i = typeof name === 'number' ? name : this.states.indexOf(name);
    if (!(i >= 0 && i < this.states.length) || !Number.isInteger(i)) throw new RangeError(`Sagax: unknown state ${name}`);
    return i;
  }

  bits(names = []) {
    let mask = 0n;
    for (const n of names) {
      const i = this.conditions.indexOf(n);
      if (i < 0) throw new RangeError(`Sagax: unknown condition ${n}`);
      mask |= 1n << BigInt(i);
    }
    return [Number(mask & 0xffffffffn) >>> 0, Number(mask >> 32n) >>> 0];
  }

  /** {from ('*' for any), to, when: [conditions], unless: [conditions], after: time}. Returns its index. */
  transition({ from = '*', to, when = [], unless = [], after = 0 }) {
    const f = from === '*' ? -1 : this.stateOf(from);
    const [wl, wh] = this.bits(when);
    const [ul, uh] = this.bits(unless);
    const i = this.x.sx_machine_add_transition(this.h, f, this.stateOf(to), wl, wh, ul, uh, u32Of(after, 'after'));
    if (i < 0) throw new RangeError('Sagax: bad transition');
    return i;
  }

  /** After time in state, go to next. */
  timeout(state, time, next) {
    if (this.x.sx_machine_set_timeout(this.h, this.stateOf(state), u32Of(time, 'time'), this.stateOf(next)) !== 0) {
      throw new RangeError('Sagax: bad timeout');
    }
  }

  /** One tick; flags {name: true}. Returns {from, to, reason, transition} or null. */
  update(now, flags = {}) {
    const t = timeOf(now);
    const [l, h] = this.bits(Object.keys(flags).filter((k) => flags[k]));
    if (this.x.sx_machine_update(this.h, t, l, h) !== 1) return null;
    const c = new Int32Array(this.x.memory.buffer, this.x.sx_machine_change(this.h), 4);
    return { from: this.states[c[0]], to: this.states[c[1]], reason: REASON[c[2]], transition: c[3] };
  }

  get state() {
    return this.states[this.x.sx_machine_state(this.h)];
  }

  /** When the current state was entered. */
  get entered() {
    return this.x.sx_machine_entered(this.h) >>> 0;
  }

  /** Puts the machine into state at now (also to start the clock). */
  setState(state, now) {
    this.x.sx_machine_set_state(this.h, this.stateOf(state), timeOf(now));
  }

  free() {
    if (this.handle) this.x.sx_machine_free(this.handle);
    this.handle = 0;
  }
}

// ---- world memory -----------------------------------------------------------
//
// What a character knows and for how long (Knowledge), messages on their way
// (Outbox, route, Inbox), places to avoid (Areas), id timers (Timers), and
// three small value classes (Progress, Interval, Alertness). Same rules as
// the Rust and C versions. Times are the game's own u32 count; ages wrap.

const u32 = (v) => v >>> 0;
const i32 = (v) => v | 0;

/** A capacity: an integer from 1 to 2^31 - 1, else RangeError. */
function capacityOf(c) {
  if (!Number.isInteger(c) || c < 1 || c > 0x7fffffff) throw new RangeError("Sagax: capacity must be an integer from 1 to 2147483647");
  return c;
}

/** A name from a table of choices, else RangeError. */
function choice(table, name, what) {
  if (typeof name !== 'string' || !Object.hasOwn(table, name)) {
    throw new RangeError(`Sagax: ${what} must be ${Object.keys(table).join(', ')}, not ${String(name)}`);
  }
  return table[name];
}

const EVICT = { oldest: 0, weakest: 1 };
const SOURCE = { own: 0, heard: 1, told: 2 };
const METRIC = { euclid: 0, manhattan: 1, chebyshev: 2, hex: 3 };
const SINE = { fine: 0, coarse: 1 };
const POST = ['ok', 'full', 'cooldown'];
const ALL = 0xffffffff;

/** Writes numbers into a record buffer; the pointer is asked for right before the view is made. */
function putRecord(x, ptr, values) {
  new Uint32Array(x.memory.buffer, ptr, values.length).set(values);
}

/** Reads numbers out of a buffer at once (a view dies when the memory grows). */
function takeRecord(x, ptr, count) {
  return Array.from(new Uint32Array(x.memory.buffer, ptr, count));
}

const FACT_SIZE = 9;
const MESSAGE_SIZE = 10;
const AREA_SIZE = 14;

/** A fact source: 'own', 'heard', 'told' or a number 0 .. 255 (more values are yours to use). */
function sourceOf(source) {
  return typeof source === 'string' ? choice(SOURCE, source, 'source') : u8Of(source, 'source');
}

function factValues(f) {
  const [px, py, pz] = posOf(f.pos ?? [0, 0, 0]);
  return [u32Of(f.subject, 'subject'), u32Of(f.kind, 'kind'), px, py, pz, timeOf(f.seenAt ?? 0), u32Of(f.strength ?? 0, 'strength'), u32Of(f.data ?? 0, 'data'), sourceOf(f.source ?? 0)];
}

function factOf(r) {
  return { subject: r[0], kind: r[1], pos: [i32(r[2]), i32(r[3]), i32(r[4])], seenAt: r[5], source: r[8], strength: r[6], data: r[7] };
}

function messageValues(m) {
  const [px, py, pz] = posOf(m.pos ?? [0, 0, 0]);
  return [u32Of(m.kind, 'kind'), u32Of(m.subject ?? 0, 'subject'), px, py, pz, timeOf(m.seenAt ?? 0), u32Of(m.strength ?? 0, 'strength'), u32Of(m.data ?? 0, 'data'), u32Of(m.sender ?? 0, 'sender'), m.keepIfSenderGone ? 1 : 0];
}

function messageOf(r) {
  return { kind: r[0], subject: r[1], pos: [i32(r[2]), i32(r[3]), i32(r[4])], seenAt: r[5], strength: r[6], data: r[7], sender: r[8], keepIfSenderGone: r[9] !== 0 };
}

function areaValues(a) {
  const [px, py, pz] = posOf(a.pos ?? [0, 0, 0]);
  const [sx, sy, sz] = posOf(a.source ?? [0, 0, 0], 'source');
  return [u32Of(a.kind ?? 0, 'kind'), px, py, pz, i32Of(a.radius, 'radius'), u32Of(a.strength ?? 0, 'strength'), timeOf(a.since ?? 0), u32Of(a.lifetime, 'lifetime'), u32Of(a.data ?? 0, 'data'), sx, sy, sz, a.blocking ? 1 : 0, a.source ? 1 : 0];
}

function areaOf(r) {
  return {
    kind: r[0],
    pos: [i32(r[1]), i32(r[2]), i32(r[3])],
    radius: i32(r[4]),
    strength: r[5],
    since: r[6],
    lifetime: r[7],
    data: r[8],
    source: r[13] !== 0 ? [i32(r[9]), i32(r[10]), i32(r[11])] : null,
    blocking: r[12] !== 0,
  };
}

/** Messages out of a list of records. */
function messagesOf(all, n) {
  const list = [];
  for (let i = 0; i < n; i++) list.push(messageOf(all.slice(i * MESSAGE_SIZE, (i + 1) * MESSAGE_SIZE)));
  return list;
}

/**
 * A handle an owner lends out (a squad its own knowledge or areas, a crew
 * its squad and keepers): it lives and dies with the owner. With epoch set,
 * it also goes stale once the owner's epoch moves on (a crew's keepers).
 */
class Lent {
  constructor(handle, owner, epoch = null) {
    this.handle = handle;
    this.owner = owner;
    this.epoch = epoch;
  }
}

/**
 * A handle to a table in the module; every call checks that it is not freed
 * (a freed handle would write to address 0). A lent table (owner set) is
 * freed by its owner.
 */
class Table extends Handle {
  constructor(x, prefix, handle, owner = null, epoch = null) {
    super();
    if (!handle) throw new RangeError('Sagax: capacity must be at least 1');
    this.x = x;
    this.prefix = prefix;
    this.handle = handle;
    this.owner = owner;
    this.lentAt = epoch;
  }

  get h() {
    const handle = super.h;
    if (this.owner) {
      void this.owner.h; // throws after the owner (or its owner) was freed
      if (this.lentAt !== null && this.owner.epoch !== this.lentAt) {
        throw new Error('Sagax: this lent keeper is stale: the crew changed since crew.keeper(); ask for it again');
      }
      if (this.lentAt !== null && this.x.sx_keeper_live(handle) !== 1) {
        throw new Error('Sagax: this lent keeper is stale: its member left the group');
      }
    }
    return handle;
  }

  /** Frees the table. Using it afterwards throws. A squad's own table is freed with the squad (this does nothing). */
  free() {
    if (this.owner) return;
    if (this.handle) this.x[`${this.prefix}_free`](this.handle);
    this.handle = 0;
  }
}

/**
 * What a character knows: facts keyed by (subject, kind), for at most
 * `capacity` of them. When full, the oldest (or the weakest) goes.
 *
 *   const k = lib.knowledge(8, 'oldest');   // or 'weakest'
 *   k.learn(now, {subject: 42, kind: 1, pos: [x, y, 0], seenAt: now});  // null, or the evicted fact
 *   k.get(42, 1);     // {subject, kind, pos, seenAt, source, strength, data} or null
 *
 * source: 0 own, 1 heard, 2 told (more values are yours to use).
 */
export class Knowledge extends Table {
  constructor(x, capacity, evict = 'oldest') {
    if (capacity instanceof Lent) {
      super(x, 'sx_knowledge', capacity.handle, capacity.owner);
      return;
    }
    const e = choice(EVICT, evict, 'evict');
    super(x, 'sx_knowledge', x.sx_knowledge_new(capacityOf(capacity), e));
  }

  #fact() {
    return factOf(takeRecord(this.x, this.x.sx_knowledge_record(this.h), FACT_SIZE));
  }

  #facts(n) {
    const all = takeRecord(this.x, this.x.sx_knowledge_out(this.h), n * FACT_SIZE);
    const facts = [];
    for (let i = 0; i < n; i++) facts.push(factOf(all.slice(i * FACT_SIZE, (i + 1) * FACT_SIZE)));
    return facts;
  }

  #put(now, fact, call) {
    const t = timeOf(now);
    putRecord(this.x, this.x.sx_knowledge_record(this.h), factValues(fact));
    return this.x[call](this.h, t) === 1 ? this.#facts(1)[0] : null;
  }

  /** Stores the fact, overwriting a known one. Returns the evicted fact, or null. */
  learn(now, fact) {
    return this.#put(now, fact, 'sx_knowledge_learn');
  }

  /** Like learn, but a known fact is only overwritten by one that is not older. */
  merge(now, fact) {
    return this.#put(now, fact, 'sx_knowledge_merge');
  }

  /** learn for a message {kind, subject, pos, seenAt, strength, data}, with the given source. */
  learnMessage(now, message, source) {
    return this.learn(now, { ...message, source });
  }

  /** merge for a message. */
  mergeMessage(now, message, source) {
    return this.merge(now, { ...message, source });
  }

  /** Renews the time of a fact. False if unknown. */
  touch(subject, kind, now) {
    return this.x.sx_knowledge_touch(this.h, u32Of(subject, 'subject'), u32Of(kind, 'kind'), timeOf(now)) === 1;
  }

  /** Sets the place of a fact. False if unknown. */
  setPos(subject, kind, pos) {
    const [px, py, pz] = posOf(pos);
    return this.x.sx_knowledge_set_pos(this.h, u32Of(subject, 'subject'), u32Of(kind, 'kind'), px, py, pz) === 1;
  }

  /** Sets the data of a fact. False if unknown. */
  setData(subject, kind, data) {
    return this.x.sx_knowledge_set_data(this.h, u32Of(subject, 'subject'), u32Of(kind, 'kind'), u32Of(data, 'data')) === 1;
  }

  /** Sets the strength of a fact. False if unknown. */
  setStrength(subject, kind, strength) {
    return this.x.sx_knowledge_set_strength(this.h, u32Of(subject, 'subject'), u32Of(kind, 'kind'), u32Of(strength, 'strength')) === 1;
  }

  /** Forgets one fact. False if unknown. */
  remove(subject, kind) {
    return this.x.sx_knowledge_remove(this.h, u32Of(subject, 'subject'), u32Of(kind, 'kind')) === 1;
  }

  /** Forgets all facts about a subject. Returns how many. */
  removeSubject(subject) {
    return this.x.sx_knowledge_remove_subject(this.h, u32Of(subject, 'subject'));
  }

  /** The fact (subject, kind) or null. */
  get(subject, kind) {
    return this.x.sx_knowledge_get(this.h, u32Of(subject, 'subject'), u32Of(kind, 'kind')) === 1 ? this.#fact() : null;
  }

  /** All facts in slot order. */
  list() {
    const n = this.x.sx_knowledge_count(this.h);
    const all = [];
    for (let i = 0; i < n; i++) {
      this.x.sx_knowledge_at(this.h, i);
      all.push(this.#fact());
    }
    return all;
  }

  /**
   * Forgets facts at least maxAge old; returns them in slot order. With
   * perKind (a function fact => maxAge) every fact has its own age limit and
   * maxAge is ignored. A limit of Infinity (or above 2^32 - 1) never expires;
   * a negative, fractional or NaN limit throws a RangeError.
   */
  expire(now, maxAge, perKind) {
    const t = timeOf(now);
    if (!perKind) return this.#facts(this.x.sx_knowledge_expire(this.h, t, u32Of(maxAge, 'maxAge')));
    const gone = [];
    for (const f of this.list()) {
      const limit = perKind(f);
      if (typeof limit !== 'number') throw new TypeError('Sagax: perKind must return a number');
      // Infinity (or any integer past 2^32 - 1) never expires; other bad limits are refused.
      if (limit === Infinity || limit > ALL) continue;
      if (!Number.isInteger(limit) || limit < 0) throw new RangeError('Sagax: perKind must return a whole number of ticks, 0 or more (Infinity: never)');
      if (u32(t - f.seenAt) >= limit) gone.push(f);
    }
    for (const f of gone) this.remove(f.subject, f.kind);
    return gone;
  }

  /** Like expire, for facts of one kind only (other kinds stay). */
  expireKind(now, kind, maxAge) {
    const t = timeOf(now);
    return this.#facts(this.x.sx_knowledge_expire_kind(this.h, t, u32Of(kind, 'kind'), u32Of(maxAge, 'maxAge')));
  }

  /** Number of facts. */
  count() {
    return this.x.sx_knowledge_count(this.h);
  }

  /** Forgets everything. */
  clear() {
    this.x.sx_knowledge_clear(this.h);
  }
}

/**
 * Messages one sender wants to send, for at most `capacity`.
 *
 *   const o = lib.outbox(4);
 *   o.post(now, msg, 102, 170);   // 'ok', 'full' or 'cooldown'
 *   o.due(now);                   // messages whose delay is over, in post order
 *
 * A message: {kind, subject, pos, seenAt, strength, data, sender, keepIfSenderGone}.
 */
export class Outbox extends Table {
  constructor(x, capacity) {
    super(x, 'sx_outbox', x.sx_outbox_new(capacityOf(capacity)));
  }

  /**
   * Queues a message that is due after delay ticks. Refused with 'full' if no
   * room, or 'cooldown' if the last accepted post is younger than cooldown
   * (the first post is never held back).
   */
  post(now, message, delay = 0, cooldown = 0) {
    const t = timeOf(now);
    const values = messageValues(message);
    const d = u32Of(delay, 'delay');
    const c = u32Of(cooldown, 'cooldown');
    putRecord(this.x, this.x.sx_outbox_record(this.h), values);
    return POST[this.x.sx_outbox_post(this.h, t, d, c)];
  }

  /** Takes (at most max of) the messages whose own delay is over, in post order. */
  due(now, max = ALL) {
    const n = this.x.sx_outbox_due(this.h, timeOf(now), u32Of(max, 'max'));
    return messagesOf(takeRecord(this.x, this.x.sx_outbox_out(this.h), n * MESSAGE_SIZE), n);
  }

  /** The sender is gone: drops every message not marked keepIfSenderGone. */
  senderGone() {
    this.x.sx_outbox_sender_gone(this.h);
  }

  /** Number of messages waiting. */
  pending() {
    return this.x.sx_outbox_pending(this.h);
  }

  /** Drops all messages (the cooldown keeps running). */
  clear() {
    this.x.sx_outbox_clear(this.h);
  }
}

/**
 * Messages on their way to one receiver, for at most `capacity`.
 *
 *   const i = lib.inbox(4);
 *   i.push(msg, lib.arrival(now, 0, distance, perUnit));   // false if refused
 *   i.takeDue(now);                                        // arrived messages, in push order
 */
export class Inbox extends Table {
  constructor(x, capacity) {
    super(x, 'sx_inbox', x.sx_inbox_new(capacityOf(capacity)));
  }

  /** Puts a message on its way, arriving at arriveAt. When full, the one arriving latest makes room if the new one is earlier; else false. */
  push(message, arriveAt) {
    const values = messageValues(message);
    const t = timeOf(arriveAt);
    putRecord(this.x, this.x.sx_inbox_record(this.h), values);
    return this.x.sx_inbox_push(this.h, t) === 1;
  }

  /** Takes (at most max of) the messages that have arrived, in push order. */
  takeDue(now, max = ALL) {
    const n = this.x.sx_inbox_take_due(this.h, timeOf(now), u32Of(max, 'max'));
    return messagesOf(takeRecord(this.x, this.x.sx_inbox_out(this.h), n * MESSAGE_SIZE), n);
  }

  /** Number of messages on their way. */
  count() {
    return this.x.sx_inbox_count(this.h);
  }

  /** Drops all messages. */
  clear() {
    this.x.sx_inbox_clear(this.h);
  }
}

/** The 9 integers of cell, start, goal in the scratch buffer; returns the pointer. */
function cellsScratch(x, cell, start, goal) {
  const ptr = x.sx_scratch(9);
  new Int32Array(x.memory.buffer, ptr, 9).set([...posOf(cell, 'cell'), ...posOf(start, 'start'), ...posOf(goal, 'goal')]);
  return ptr;
}

/**
 * Round areas (noise, light, danger) that fade after their lifetime.
 *
 *   const a = lib.areas(8);
 *   a.add(now, {kind: 1, pos: [x, y, 0], radius: 600, strength: 5, lifetime: 3000, blocking: false});
 *   a.cost(cell, start, goal);    // extra path cost for a cell
 *   a.blocks(cell, start, goal);  // for a path finder
 *
 * Optional: data, source [x, y, z] (where it came from).
 */
export class Areas extends Table {
  constructor(x, capacity) {
    if (capacity instanceof Lent) {
      super(x, 'sx_areas', capacity.handle, capacity.owner);
      return;
    }
    super(x, 'sx_areas', x.sx_areas_new(capacityOf(capacity)));
  }

  /** Adds an area (since = now), or renews the first of the same kind whose centre is closer than its radius. Returns its index. */
  add(now, area) {
    const t = timeOf(now);
    const values = areaValues(area);
    putRecord(this.x, this.x.sx_areas_record(this.h), values);
    return this.x.sx_areas_add(this.h, t);
  }

  /** Removes areas whose age is at least their lifetime. Returns how many. */
  expire(now) {
    return this.x.sx_areas_expire(this.h, timeOf(now));
  }

  /** Index of the first area containing pos, or null. */
  contains(pos) {
    const [px, py, pz] = posOf(pos);
    const i = this.x.sx_areas_contains(this.h, px, py, pz);
    return i >= 0 ? i : null;
  }

  /** Like contains, for areas of one kind. */
  containsKind(pos, kind) {
    const [px, py, pz] = posOf(pos);
    const i = this.x.sx_areas_contains_kind(this.h, px, py, pz, u32Of(kind, 'kind'));
    return i >= 0 ? i : null;
  }

  /** True if a blocking area holds cell but neither start nor goal. */
  blocks(cell, start, goal) {
    const h = this.h;
    return this.x.sx_areas_blocks(h, cellsScratch(this.x, cell, start, goal)) === 1;
  }

  /** Extra cost of cell: the strengths (saturating) of the non-blocking areas holding it but neither start nor goal. */
  cost(cell, start, goal) {
    const h = this.h;
    return u32(this.x.sx_areas_cost(h, cellsScratch(this.x, cell, start, goal)));
  }

  /** The extra cost of entering cell on the way from start to goal: null if it is blocked, else as cost(). */
  cellCost(cell, start, goal) {
    return this.blocks(cell, start, goal) ? null : this.cost(cell, start, goal);
  }

  /** All areas in slot order. */
  list() {
    const n = this.x.sx_areas_count(this.h);
    const all = [];
    for (let i = 0; i < n; i++) {
      this.x.sx_areas_at(this.h, i);
      all.push(areaOf(takeRecord(this.x, this.x.sx_areas_record(this.h), AREA_SIZE)));
    }
    return all;
  }

  /** Number of areas. */
  count() {
    return this.x.sx_areas_count(this.h);
  }

  /** Removes all areas. */
  clear() {
    this.x.sx_areas_clear(this.h);
  }
}

/**
 * Ids that count as "on" for a while (ignore a target, no alarm again).
 *
 *   const t = lib.timers(8);
 *   t.set(id, now, 300);   // on for 300 ticks (renews; when full, the one with least time left is replaced)
 *   t.active(id, now);
 */
export class Timers extends Table {
  constructor(x, capacity) {
    super(x, 'sx_timers', x.sx_timers_new(capacityOf(capacity)));
  }

  /** Starts or renews the timer of id. */
  set(id, now, duration) {
    this.x.sx_timers_set(this.h, u32Of(id, 'id'), timeOf(now), u32Of(duration, 'duration'));
  }

  /** True while the timer of id runs (age below its duration). */
  active(id, now) {
    return this.x.sx_timers_active(this.h, u32Of(id, 'id'), timeOf(now)) === 1;
  }

  /** Time left of the timer of id (0 if none or over). */
  left(id, now) {
    return u32(this.x.sx_timers_left(this.h, u32Of(id, 'id'), timeOf(now)));
  }

  /** Stops the timer of id. False if there was none. */
  remove(id) {
    return this.x.sx_timers_remove(this.h, u32Of(id, 'id')) === 1;
  }

  /** Stops all timers. */
  clear() {
    this.x.sx_timers_clear(this.h);
  }

  /** Number of timers. */
  count() {
    return this.x.sx_timers_count(this.h);
  }
}

/** Runs `call` on a record in the module's scratch buffer: put, call, take. Returns [result, record]. */
function onRecord(record, call) {
  const x = wasm();
  const ptr = x.sx_scratch(record.length);
  new Int32Array(x.memory.buffer, ptr, record.length).set(record);
  const result = call(x, ptr);
  return [result, Array.from(new Int32Array(x.memory.buffer, ptr, record.length))];
}

/**
 * Tells whether a chase still gets closer: the best value so far (lower is
 * better, e.g. a distance) and when it last improved. A plain object; the
 * rules run in the module (as in C), so load it first (loadSagax).
 */
export class Progress {
  constructor() {
    this.reset();
  }

  #record() {
    return [this.best, this.since, this.started ? 1 : 0];
  }

  #take([best, since, started]) {
    this.best = best;
    this.since = u32(since);
    this.started = (started & 0xff) !== 0;
  }

  /** Starts with value at now. */
  start(value, now) {
    const v = i32Of(value, 'value');
    const t = timeOf(now);
    this.#take(onRecord(this.#record(), (x, ptr) => x.sx_progress_start(ptr, v, t))[1]);
  }

  /** If value beats the best by at least minGain, it becomes the best (since = now) and true is returned. False if not started. */
  update(value, now, minGain) {
    const v = i32Of(value, 'value');
    const t = timeOf(now);
    const g = i32Of(minGain, 'minGain');
    const [r, record] = onRecord(this.#record(), (x, ptr) => x.sx_progress_update(ptr, v, t, g));
    this.#take(record);
    return r === 1;
  }

  /** True if started and the last improvement is at least limit old. */
  stalled(now, limit) {
    const t = timeOf(now);
    const l = u32Of(limit, 'limit');
    return onRecord(this.#record(), (x, ptr) => x.sx_progress_stalled(ptr, t, l))[0] === 1;
  }

  /** Back to not started. */
  reset() {
    this.best = 0;
    this.since = 0;
    this.started = false;
  }
}

/** A timer for "every N ticks"; never started counts as due. */
export class Interval {
  constructor() {
    this.reset();
  }

  /** True if never marked, or at least every has passed since the mark. */
  due(now, every) {
    const t = timeOf(now);
    const e = u32Of(every, 'every');
    return onRecord([this.last, this.started ? 1 : 0], (x, ptr) => x.sx_interval_due(ptr, t, e))[0] === 1;
  }

  /** Records that it ran at now. */
  mark(now) {
    const t = timeOf(now);
    const [, [last, started]] = onRecord([this.last, this.started ? 1 : 0], (x, ptr) => x.sx_interval_mark(ptr, t));
    this.last = u32(last);
    this.started = (started & 0xff) !== 0;
  }

  /** Back to never started. */
  reset() {
    this.last = 0;
    this.started = false;
  }
}

const ALERT_LEVELS = 8;

/** An alert config {max, decayPerTick, hold, hysteresis, thresholds} as the config part of the record: max, decay, hold, hysteresis, count, 8 thresholds. */
function alertConfig(cfg) {
  const thresholds = cfg.thresholds ?? [];
  if (!Array.isArray(thresholds)) throw new TypeError('Sagax: thresholds must be an array');
  if (thresholds.length > ALERT_LEVELS) throw new RangeError(`Sagax: an alertness has at most ${ALERT_LEVELS} thresholds`);
  const fill = [...thresholds.map((t) => u32Of(t, 'threshold')), ...new Array(ALERT_LEVELS - thresholds.length).fill(0)];
  return [u32Of(cfg.max, 'max'), u32Of(cfg.decayPerTick ?? 0, 'decayPerTick'), u32Of(cfg.hold ?? 0, 'hold'), u32Of(cfg.hysteresis ?? 0, 'hysteresis'), thresholds.length, ...fill];
}

/**
 * A value that rises with stimuli and decays, with levels (unaware,
 * suspicious, searching, alert, ...) that you name yourself. A plain object;
 * the rules run in the module (as in C), so load it first (loadSagax).
 *
 *   const cfg = {max: 100, decayPerTick: 2, hold: 5, hysteresis: 5, thresholds: [20, 50, 80]};
 *   const a = new Alertness();
 *   a.stimulus(30, now, cfg);
 *   a.update(now, cfg);   // null, or [from, to] when the level changed
 *   a.value; a.level;
 *
 * Decay (decayPerTick per tick) only counts ticks after hold ticks since the
 * last stimulus. The level jumps straight to the highest threshold reached
 * and falls when the value is below its threshold minus hysteresis.
 */
export class Alertness {
  constructor() {
    this.value = 0;
    this.last = 0;
    this.lastStimulus = 0;
    this.level = 0;
  }

  #run(cfg, call) {
    const record = [this.value, this.last, this.lastStimulus, this.level, ...alertConfig(cfg)];
    const [r, [value, last, lastStimulus, level]] = onRecord(record, call);
    this.value = u32(value);
    this.last = u32(last);
    this.lastStimulus = u32(lastStimulus);
    this.level = level & 0xff;
    return r;
  }

  /** Applies the decay owed up to now, adds amount (up to max) and remembers now as the last stimulus. The level changes in update. */
  stimulus(amount, now, cfg) {
    const a = u32Of(amount, 'amount');
    const t = timeOf(now);
    this.#run(cfg, (x, ptr) => x.sx_alertness_stimulus(ptr, a, t));
  }

  /** Decays up to now and sets the level. Returns [from, to] if it changed, else null. */
  update(now, cfg) {
    const t = timeOf(now);
    const from = this.level;
    const changed = this.#run(cfg, (x, ptr) => x.sx_alertness_update(ptr, t));
    return changed === 1 ? [from, this.level] : null;
  }
}

/** The route rule as [code, argument]: {nearest: k}, {inRange: r}, 'all' or {to: id}. */
function ruleOf(rule) {
  if (rule === 'all') return [2, 0];
  if (rule !== null && typeof rule === 'object') {
    if (Object.hasOwn(rule, 'nearest')) {
      const k = rule.nearest;
      if (!Number.isInteger(k) || k < 0 || k > ALL) throw new RangeError('Sagax: nearest must be an integer from 0 to 4294967295');
      return [0, k];
    }
    if (Object.hasOwn(rule, 'inRange')) {
      const r = rule.inRange;
      if (!Number.isSafeInteger(r)) throw new RangeError('Sagax: inRange must be a safe integer');
      return [1, r];
    }
    if (Object.hasOwn(rule, 'to')) {
      const id = rule.to;
      if (!Number.isInteger(id) || id < 0 || id > ALL) throw new RangeError('Sagax: to must be an integer from 0 to 4294967295');
      return [3, id];
    }
  }
  throw new RangeError("Sagax: a route rule is {nearest: k}, {inRange: r}, {to: id} or 'all'");
}

/** Ids picked from candidates [{id, x, y}] by a rule, see Sagax.route. */
function routeOf(x, rule, origin, candidates, metric, divisor) {
  const [code, arg] = ruleOf(rule);
  const [ox, oy] = originOf(origin);
  const m = choice(METRIC, metric, 'metric');
  const d = i32Of(divisor, 'divisor');
  if (!Array.isArray(candidates)) throw new TypeError('Sagax: candidates must be an array');
  const n = candidates.length;
  const ids = candidates.flatMap((c) => [u32Of(c.id, 'id'), i32Of(c.x, 'x'), i32Of(c.y, 'y')]);
  const size = Math.max(n * 3, 1);
  // The pointer first, the view right after: asking for the buffer may grow
  // the memory, and a grown memory kills older views. Only the number is kept.
  const ptr = x.sx_scratch(size);
  const view = new Int32Array(x.memory.buffer, ptr, size);
  view.set(ids);
  // The call may grow the memory again: make a new view from the number.
  const count = x.sx_route(code, arg, n, ox, oy, m, d);
  if (count < 0) throw new RangeError('Sagax: bad route arguments');
  return Array.from(new Uint32Array(x.memory.buffer, ptr, count));
}

const POINT_DEFAULT = {
  headingStep: 0x62,
  firstDistance: 0x100,
  offsets: [0, 0x20, -0x20],
  distances: [5 * 256, 9 * 256, 12 * 256],
  min: [-2147483648, -2147483648],
  max: [2147483647, 2147483647],
  sine: 'fine',
};

/** Point of an agent around a place: [x, y, z] or null past the last step. */
function pointsAroundOf(x, agent, step, centre, pattern) {
  const [cx, cy, cz] = posOf(centre, 'centre');
  const a = u32Of(agent, 'agent');
  const st = u32Of(step, 'step');
  const p = { ...POINT_DEFAULT, ...pattern };
  const sine = choice(SINE, p.sine, 'sine');
  let custom = 0;
  let ptr;
  if (pattern) {
    if (p.offsets.length > 8 || p.distances.length > 8) {
      throw new RangeError('Sagax: a point pattern has at most 8 steps, one distance each');
    }
    const fill = (list) => [...list, ...new Array(8 - list.length).fill(0)];
    const offsets = fill(p.offsets);
    // Like Rust: a missing distance counts as 0 (an extra one is ignored).
    const distances = fill(p.distances.slice(0, p.offsets.length));
    ptr = x.sx_scratch(23);
    new Int32Array(x.memory.buffer, ptr, 23).set([p.headingStep, p.firstDistance, p.offsets.length, ...offsets, ...distances, ...p.min, ...p.max].map((v) => i32Of(v, 'pattern')));
    custom = 1;
  } else {
    ptr = x.sx_scratch(3);
  }
  const r = x.sx_points_around(a, st, cx, cy, cz, custom, sine);
  if (r < 0) throw new RangeError('Sagax: bad point pattern');
  if (r === 0) return null;
  return Array.from(new Int32Array(x.memory.buffer, ptr, 3));
}

Object.assign(Sagax.prototype, {
  /** Facts keyed by (subject, kind) for at most capacity; evict 'oldest' or 'weakest'. See Knowledge. */
  knowledge(capacity, evict = 'oldest') {
    return new Knowledge(this.x, capacity, evict);
  },
  /** Messages to send, for at most capacity. See Outbox. */
  outbox(capacity) {
    return new Outbox(this.x, capacity);
  },
  /** Messages on their way to one receiver, for at most capacity. See Inbox. */
  inbox(capacity) {
    return new Inbox(this.x, capacity);
  },
  /** Areas to avoid or pay extra for, for at most capacity. See Areas. */
  areas(capacity) {
    return new Areas(this.x, capacity);
  },
  /** Id timers for at most capacity ids. See Timers. */
  timers(capacity) {
    return new Timers(this.x, capacity);
  },
  /**
   * Picks recipients: route(rule, [x, y], candidates, metric = 'euclid', divisor = 1).
   * rule: {nearest: k}, {inRange: r} (distance <= r, in candidate order), 'all' or {to: id}.
   * candidates: [{id, x, y}], already filtered by the game (team, free).
   * metric: 'euclid' (squared compare), 'manhattan', 'chebyshev' or 'hex' (axial q, r).
   * Each axis is divided by divisor (truncating) first. Ties: the earlier candidate wins.
   * Returns the ids.
   */
  route(rule, origin, candidates, metric = 'euclid', divisor = 1) {
    return routeOf(this.x, rule, origin, candidates, metric, divisor);
  },
  /** When a message sent at now arrives: now + base + distance * perUnit (u32, wrapping). */
  arrival(now, base, distance, perUnit) {
    return u32(this.x.sx_arrival(timeOf(now), u32Of(base, 'base'), u32Of(distance, 'distance'), u32Of(perUnit, 'perUnit')));
  },
  /** Sine of a heading (256 = full turn): 'fine' times 65536, 'coarse' times 256 (17-entry table). */
  sine(heading, mode = 'fine') {
    return this.x.sx_sine(i32Of(heading, 'heading'), choice(SINE, mode, 'sine'));
  },
  /**
   * Point where an agent looks around a place: pointsAround(agent, step, [x, y, z], pattern?).
   * pattern: {headingStep, firstDistance, offsets, distances, min: [x, y], max: [x, y], sine: 'fine'|'coarse'}.
   */
  pointsAround(agent, step, centre, pattern) {
    return pointsAroundOf(this.x, agent, step, centre, pattern);
  },
  /** sweepHeading(base, elapsed, stepTicks, offsets?): heading 0..255 while looking around. */
  sweepHeading(base, elapsed, stepTicks, offsets) {
    const b = i32Of(base, 'base');
    const e = u32Of(elapsed, 'elapsed');
    const t = u32Of(stepTicks, 'stepTicks');
    if (!offsets) return this.x.sx_sweep_heading(b, e, t, -1);
    const list = offsets.map((o) => i32Of(o, 'offset'));
    const ptr = this.x.sx_scratch(list.length);
    new Int32Array(this.x.memory.buffer, ptr, Math.max(list.length, 1)).set(list);
    return this.x.sx_sweep_heading(b, e, t, list.length);
  },
});

// ---- group tactics ------------------------------------------------------------
//
// A squad leader with its own memory plans for a group (Squad); a member
// keeps its goal (Keeper); gather picks who joins; tactics are data (Tactic,
// lib.tactics). Same rules as the Rust and C versions: integer only,
// deterministic, seeds from the game, ties to the lower index or id. Times
// are the game's own u32 count (0 .. 2^32 - 1, checked); ages wrap.
//
// A goal (in orders and plans; member is null in a Keeper):
//   {member, kind, custom, pos, unchecked, thenPos, thenUnchecked, target,
//    start: {kind: 'now'|'after'|'signal', ticks}, slot, plan, issuedAt, startDelay}
// kind: 'move', 'hold', 'attack', 'wait', 'retreat' or 'custom' (the game's
// kind in custom); pos, thenPos and target may be null.

/** The columns of Keeper.inputs: a member's decider row starts with these five, its own inputs follow from extra. */
export const KEEPER = { hasGoal: 0, dist: 1, age: 2, signal: 3, discipline: 4, extra: 5 };

/**
 * Member reports as messages for Squad.report: enemy(sender, fact) tells a
 * target (fact: {subject, kind, pos, seenAt, strength, data}), arrived(sender,
 * plan) that the member reached its first spot, goalBroken(sender, plan) that
 * its goal broke.
 */
export const Report = {
  enemy(sender, fact) {
    return reportMessage(SQUAD.reportEnemy, sender, { subject: fact.subject, pos: fact.pos, seenAt: fact.seenAt, strength: fact.strength, data: fact.data });
  },
  arrived(sender, plan) {
    return reportMessage(SQUAD.reportArrived, sender, { data: plan });
  },
  goalBroken(sender, plan) {
    return reportMessage(SQUAD.reportGoalBroken, sender, { data: plan });
  },
};

function reportMessage(kind, sender, { subject = 0, pos = [0, 0, 0], seenAt = 0, strength = 0, data = 0 }) {
  return {
    kind,
    subject: u32Of(subject, 'subject'),
    pos: posOf(pos),
    seenAt: timeOf(seenAt),
    strength: u32Of(strength, 'strength'),
    data: u32Of(data, 'data'),
    sender: u32Of(sender, 'sender'),
    keepIfSenderGone: false,
  };
}

/** Columns of the scoring row, role bits, spot tags and report kinds (danger has its own call, Squad.reportDanger). */
export const SQUAD = {
  inLosses: 0, // members marked gone and still listed (not reset by a new plan)
  inOffPlan: 1, // active members whose goal of the active plan broke
  inTargetAge: 2, // ticks since the target was seen
  inTargetDist: 3, // group centre to target
  inRole0: 4, // active members with the first entry of the role list
  inRole1: 5, // active members with the second entry of the role list
  inRanged: 4, // = inRole0: with the default role list, roleRanged
  inArea: 5, // = inRole1: with the default role list, roleArea
  inPlanAge: 6, // ticks since the active plan became active (its own row only)
  inTargetFriendDist: 7, // target to its nearest other known target; none: -1
  inExtra: 8, // first column of the game's own with at most two roles; else Squad.extraStart()
  roleRanged: 1,
  roleArea: 2,
  tagCover: 1,
  tagHigh: 2,
  tagHidden: 4,
  tagChoke: 8,
  reportGoalBroken: 1, // sender: the member; data: the plan id of its goal
  reportEnemy: 2, // subject, pos, seenAt, strength, data: a target (told)
  reportArrived: 4, // sender: the member; data: the plan id: at its gather spot
  maxExtraColumns: 1024, // most columns setExtraColumns takes
  maxRoles: 8, // most entries of the role list
};

const ROLE = { ranged: SQUAD.roleRanged, area: SQUAD.roleArea };
const TAG = { cover: SQUAD.tagCover, high: SQUAD.tagHigh, hidden: SQUAD.tagHidden, choke: SQUAD.tagChoke };
const REPORT = { enemy: SQUAD.reportEnemy, arrived: SQUAD.reportArrived, goalBroken: SQUAD.reportGoalBroken };
const ERROR_PLAN = -3;
const MEMBER_STATE = { active: 0, suspended: 1, gone: 2 };
const MEMBER_STATES = Object.keys(MEMBER_STATE);
const GOAL_KIND = { move: 0, hold: 1, attack: 2, wait: 3, retreat: 4, custom: 5 };
const GOAL_KINDS = Object.keys(GOAL_KIND);
const START = { now: 0, after: 1, signal: 2 };
const STARTS = Object.keys(START);
const PLACE = { none: 0, offset: 1, side: 2, between: 3, tagged: 4, here: 5 };
const SIDE = { front: 0, left: 1, right: 2, rear: 3 };
/** The shapes of lib.tactics.formation, numbered as C `SX_SHAPE_*`. */
export const SHAPE = { line: 0, wedge: 1, column: 2 };
const PICKS = Object.keys(PICK);
const SQUAD_STATUS = ['keep', 'replan', 'signal', 'noPlan'];
const MEMBER_SIZE = 7;
const GOAL_SIZE = 20;
const ORDER_SIZE = 21;
const PLAN_SIZE = 7;
const SKILL_SIZE = 28;
const I32_MIN = -0x80000000;
const I32_MAX = 0x7fffffff;

/** A tactic reads a column the scoring rows do not have (see Squad.validate). */
export class PlanError extends RangeError {
  constructor(tactic, input) {
    super(`Sagax: tactic ${tactic} reads column ${input}, beyond the row`);
    this.tactic = tactic;
    this.input = input;
  }
}

/** An integer from lo to hi: a TypeError if v is not a number at all, else a RangeError if it is not a whole number in range. */
function intIn(v, lo, hi, what) {
  if (typeof v !== 'number') throw new TypeError(`Sagax: ${what} must be a number, not ${v === null ? 'null' : typeof v}`);
  if (!Number.isInteger(v) || v < lo || v > hi) throw new RangeError(`Sagax: ${what} must be an integer from ${lo} to ${hi}, not ${String(v)}`);
  return v;
}
const u32Of = (v, what) => intIn(v, 0, ALL, what);
const i32Of = (v, what) => intIn(v, I32_MIN, I32_MAX, what);
const u8Of = (v, what) => intIn(v, 0, 255, what);
const timeOf = (v) => u32Of(v, 'time');
const safeOf = (v, what) => intIn(v, Number.MIN_SAFE_INTEGER, Number.MAX_SAFE_INTEGER, what);

/**
 * A position as checked i32s [x, y, z]. Accepts [x, y, z], {x, y, z} and
 * {x, y, level} (z / level optional, 0 by default). Anything that is not a
 * position of one of these shapes is a RangeError, and so are z and level,
 * or an array with a level property, that differ: the level is never picked
 * silently.
 */
function posOf(p, what = 'pos') {
  const coord = (v) => {
    if (typeof v !== 'number') throw new RangeError(`Sagax: ${what} must be [x, y, z] or {x, y, z} of numbers`);
    return i32Of(v, what);
  };
  if (Array.isArray(p)) {
    if (p.length !== 3) throw new RangeError(`Sagax: ${what} must be [x, y, z] or {x, y, z}`);
    if (p.level !== undefined && p.level !== p[2]) {
      throw new RangeError(`Sagax: ${what} is an array with z ${String(p[2])} and level ${String(p.level)}, which differ`);
    }
    return p.map(coord);
  }
  if (p !== null && typeof p === 'object') {
    const { x, y, z, level } = p;
    if (z !== undefined && level !== undefined && z !== level) {
      throw new RangeError(`Sagax: ${what} has z ${String(z)} and level ${String(level)}, which differ`);
    }
    return [coord(x), coord(y), coord(z ?? level ?? 0)];
  }
  throw new RangeError(`Sagax: ${what} must be [x, y, z] or {x, y, z}`);
}

/** The x and y of an origin ([x, y], [x, y, z] or an object position; only x and y count). */
function originOf(origin) {
  const [x, y] = posOf(Array.isArray(origin) && origin.length === 2 ? [...origin, 0] : origin, 'origin');
  return [x, y];
}

/** Bits from a number or a list of names out of a table ('ranged', 'cover', ...), else RangeError / TypeError. */
function bitsOf(v, table, what) {
  if (!Array.isArray(v)) return u32Of(v, what);
  return v.reduce((bits, name) => (bits | choice(table, name, what)) >>> 0, 0);
}

/** A skill table entry: a whole number from 0 to 2147483647 (the module reads the numbers as i32). */
const entryOf = (v, what) => intIn(v, 0, I32_MAX, what);

/** Four numbers of a skill table row, checked. */
function levels(list, check, what) {
  if (!Array.isArray(list) || list.length !== 4) throw new RangeError(`Sagax: ${what} must have 4 levels`);
  return list.map((v) => check(v, what));
}

/** Writes integers (u32 as their bit pattern) to the scratch buffer. */
function putScratch(x, values) {
  const ptr = x.sx_scratch(values.length);
  new Int32Array(x.memory.buffer, ptr, values.length).set(values);
  return ptr;
}

/** Reads count integers at ptr (a view dies when the memory grows). */
function takeInts(x, ptr, count) {
  return Array.from(new Int32Array(x.memory.buffer, ptr, count));
}

function memberValues(m) {
  return [u32Of(m.id, 'id'), ...posOf(m.pos), bitsOf(m.roles ?? 0, ROLE, 'roles'), u8Of(m.skill ?? 0, 'skill'), choice(MEMBER_STATE, m.state ?? 'active', 'state')];
}

function memberOf(r) {
  return { id: u32(r[0]), pos: [r[1], r[2], r[3]], roles: u32(r[4]), skill: r[5], state: MEMBER_STATES[r[6]] };
}

function startValues(start = { kind: 'now' }) {
  const kind = choice(START, start?.kind, 'start');
  return [kind, kind === START.after ? u32Of(start.ticks ?? 0, 'start ticks') : 0];
}

function startOf(code, ticks) {
  return { kind: STARTS[code], ticks: u32(ticks) };
}

function goalValues(g) {
  const pos = g.pos == null ? null : posOf(g.pos);
  const then = g.thenPos == null ? null : posOf(g.thenPos, 'thenPos');
  const kind = choice(GOAL_KIND, g.kind, 'goal kind');
  return [
    kind, kind === GOAL_KIND.custom ? u32Of(g.custom ?? 0, 'custom') : 0,
    ...(pos ?? [0, 0, 0]), ...(then ?? [0, 0, 0]),
    g.target == null ? 0 : u32Of(g.target, 'target'),
    ...startValues(g.start),
    u8Of(g.slot ?? 0, 'slot'), u32Of(g.plan ?? 0, 'plan'), timeOf(g.issuedAt ?? 0), u32Of(g.startDelay ?? 0, 'startDelay'),
    pos ? 1 : 0, g.unchecked ? 1 : 0, then ? 1 : 0, g.thenUnchecked ? 1 : 0, g.target == null ? 0 : 1,
  ].map(i32);
}

function goalOf(r, member = null) {
  return {
    member,
    kind: GOAL_KINDS[r[0]],
    custom: u32(r[1]),
    pos: r[15] ? [r[2], r[3], r[4]] : null,
    unchecked: r[16] !== 0,
    thenPos: r[17] ? [r[5], r[6], r[7]] : null,
    thenUnchecked: r[18] !== 0,
    target: r[19] ? u32(r[8]) : null,
    start: startOf(r[9], r[10]),
    slot: r[11],
    plan: u32(r[12]),
    issuedAt: u32(r[13]),
    startDelay: u32(r[14]),
  };
}

/** Orders out of a list of records (member, then a goal). */
function ordersOf(all, n, from = 0) {
  const list = [];
  for (let i = 0; i < n; i++) {
    const at = from + i * ORDER_SIZE;
    list.push(goalOf(all.slice(at + 1, at + ORDER_SIZE), u32(all[at])));
  }
  return list;
}

function skillValues(t) {
  const pick = levels(t.pick, (p, what) => choice(PICK, p, what), 'pick');
  return [
    ...pick,
    ...levels(t.pickTop, entryOf, 'pickTop'),
    ...levels(t.thinkTicks, entryOf, 'thinkTicks'),
    ...levels(t.timingSpread, entryOf, 'timingSpread'),
    ...levels(t.holdBonus, entryOf, 'holdBonus'),
    ...levels(t.giveUpTicks, entryOf, 'giveUpTicks'),
    ...levels(t.spotRankError, u8Of, 'spotRankError'),
  ].map(i32);
}

function skillOf(r) {
  const row = (i) => r.slice(i * 4, i * 4 + 4).map(u32);
  return {
    pick: row(0).map((p) => PICKS[p]),
    pickTop: row(1),
    thinkTicks: row(2),
    timingSpread: row(3),
    holdBonus: row(4),
    giveUpTicks: row(5),
    spotRankError: row(6),
  };
}

/** A place {kind, pos | side | tags} as 5 numbers; 'none' only where allowed. */
function placeValues(p, allowNone) {
  const kind = choice(PLACE, p?.kind, 'place');
  if (kind === PLACE.none && !allowNone) throw new RangeError('Sagax: a slot needs a place');
  const pos = kind === PLACE.offset ? posOf(p.pos, 'offset') : [0, 0, 0];
  let value = 0;
  if (kind === PLACE.side) value = choice(SIDE, p.side, 'side');
  if (kind === PLACE.tagged) value = bitsOf(p.tags, TAG, 'tags');
  return [kind, ...pos, value];
}

function slotValues(s) {
  const kind = choice(GOAL_KIND, s.kind ?? 'move', 'goal kind');
  return [
    bitsOf(s.roles ?? 0, ROLE, 'roles'), u8Of(s.min ?? 1, 'min'), u8Of(s.max ?? 1, 'max'),
    ...placeValues(s.place, false), ...placeValues(s.gather ?? { kind: 'none' }, true),
    kind, kind === GOAL_KIND.custom ? u32Of(s.custom ?? 0, 'custom') : 0,
    ...startValues(s.start), bitsOf(s.wantTags ?? 0, TAG, 'wantTags'), i32Of(s.dist ?? 0, 'dist'),
    i32Of(s.gatherDist ?? s.dist ?? 0, 'gatherDist'),
  ].map(i32);
}

/** The seed (a safe integer, or a BigInt for the full 64 bits) as two u32 halves. */
function seedOf(seed) {
  if (typeof seed === 'bigint') {
    if (seed < 0n || seed > 0xffffffffffffffffn) throw new RangeError('Sagax: a BigInt seed must be 0 .. 2^64 - 1');
    return [Number(seed & 0xffffffffn), Number(seed >> 32n)];
  }
  if (!Number.isSafeInteger(seed) || seed < 0) throw new RangeError('Sagax: seed must be a safe integer or a BigInt, 0 or more');
  return [seed % 0x100000000, Math.floor(seed / 0x100000000)];
}

/**
 * A tactic as data: slots, considerations over the scoring row, abort rules
 * and a trigger. Made by lib.tactic(...) or lib.tactics.*(); a squad takes
 * it over in addTactic (the tactic cannot be used afterwards).
 *
 *   const t = lib.tactic({ weight: 0.5, minSkill: 64 });
 *   t.addSlot({ min: 1, max: 3, place: { kind: 'side', side: 'left' }, dist: 4, kind: 'attack' });
 *   t.addConsideration({ input: SQUAD.inTargetDist, curve: 'inverse', min: 0, max: 50 });
 *   t.addAbort(SQUAD.inLosses, 2);
 *   t.triggerAllArrived(170);
 */
export class Tactic extends Table {
  constructor(x, handle) {
    super(x, 'sx_tactic', handle);
  }

  /**
   * Adds a slot and returns its index. A slot: {roles = 0, min = 1, max = 1,
   * place, gather = {kind: 'none'}, kind = 'move', custom = 0,
   * start = {kind: 'now'}, wantTags = 0, dist = 0, gatherDist = dist}. dist:
   * from the target for 'side' and 'between' places (spots count within
   * 3 * dist of the wish point, see Squad.setSpotRadius); gatherDist: the
   * same for the gather place. A place: {kind: 'offset',
   * pos: [x, y, z]} (from the leader, +x toward the target, +y to its left),
   * {kind: 'side', side: 'front'|'left'|'right'|'rear'}, {kind: 'between'},
   * {kind: 'tagged', tags}, {kind: 'here'}; gather may also be {kind: 'none'}.
   */
  addSlot(slot) {
    const values = slotValues(slot);
    const h = this.h;
    putScratch(this.x, values);
    const r = this.x.sx_tactic_add_slot(h);
    if (r < 0) throw new RangeError('Sagax: bad slot');
    return r;
  }

  /** A factor {input (a SQUAD.in* column), curve, min, max} over the scoring row; the factors are multiplied. */
  addConsideration({ input, curve = 'linear', min = 0, max = ONE }) {
    const k = choice(CURVE, curve, 'curve');
    this.x.sx_tactic_add_consideration(this.h, u32Of(input, 'input'), k, i32Of(min, 'min'), i32Of(max, 'max'));
  }

  /** The plan ends (a replan) when row[input] >= atLeast. */
  addAbort(input, atLeast) {
    this.x.sx_tactic_add_abort(this.h, u32Of(input, 'input'), i32Of(atLeast, 'atLeast'));
  }

  /** The signal never fires by itself (the default). */
  triggerNone() {
    this.x.sx_tactic_trigger_none(this.h);
  }

  /** The signal fires when every assigned active member is at its gather spot, or maxWait ticks after activation. */
  triggerAllArrived(maxWait) {
    this.x.sx_tactic_trigger_all_arrived(this.h, u32Of(maxWait, 'maxWait'));
  }

  /** The signal fires when the target is within radius of the group centre. */
  triggerTargetInRadius(radius) {
    this.x.sx_tactic_trigger_target_in_radius(this.h, safeOf(radius, 'radius'));
  }

  /** The signal is only given by Squad.signal. */
  triggerManual() {
    this.x.sx_tactic_trigger_manual(this.h);
  }
}

/**
 * A squad leader with its own memory, planning for a group.
 *
 *   const sq = lib.squad({ members: 8, memory: 16, areas: 4, inbox: 8, dangers: 4, metric: 'chebyshev' });
 *   sq.addMember({ id: 1, pos: [0, 0, 0] }); sq.setLeader(1); sq.setTargetKind(ENEMY);
 *   sq.addTactic(lib.tactics.flank(3, 4)); sq.setSkill(192);
 *   sq.observe(now, { subject: 9, kind: ENEMY, pos: [20, 0, 0], seenAt: now });
 *   const { status, plan } = sq.check(now, { spots, seed });   // every tick
 *   for (const order of sq.ordersDue(now)) give(order);        // goals, see above
 *
 * Targets come only from the squad's own knowledge (observe, enemy reports).
 * Config keys and defaults: members 8, memory 32, areas 8, inbox 16, dangers 8,
 * metric 'euclid', roles ['ranged', 'area']. roles is the role list: one
 * scoring column per entry (a name or a bit mask, at most 8, else RangeError)
 * that counts the active members having it; entries 0 and 1 are the columns
 * SQUAD.inRole0 and inRole1, later ones follow after column 7, and
 * extraStart() says where the game's own columns begin. The old keys groupCap,
 * memoryCap, areaCap and inboxCap still work (0.9 only); an old and a new key
 * for the same field is a TypeError.
 */
/** The role masks of a squad config's roles (names or bits, at most 8). */
function rolesOf(roles) {
  if (!Array.isArray(roles)) throw new TypeError('Sagax: roles must be a list');
  if (roles.length > SQUAD.maxRoles) throw new RangeError(`Sagax: at most ${SQUAD.maxRoles} roles`);
  return roles.map((r) => (typeof r === 'string' ? choice(ROLE, r, 'roles') : bitsOf(r, ROLE, 'roles')));
}

/** The numbers sx_squad_new takes for a squad config (see Squad), and the role masks to put in the scratch buffer. */
function squadConfigOf(config) {
  const field = (name, old, fallback) => {
    if (config[name] !== undefined && config[old] !== undefined) {
      throw new TypeError(`Sagax: squad config has both ${name} and its old name ${old}`);
    }
    return capacityOf(config[name] ?? config[old] ?? fallback);
  };
  const members = field('members', 'groupCap', 8);
  const memory = field('memory', 'memoryCap', 32);
  const areas = field('areas', 'areaCap', 8);
  const inbox = field('inbox', 'inboxCap', 16);
  const dangers = capacityOf(config.dangers ?? 8);
  const m = choice(METRIC, config.metric ?? 'euclid', 'metric');
  const masks = rolesOf(config.roles ?? ['ranged', 'area']);
  return { numbers: [members, memory, areas, inbox, dangers, m, masks.length], masks };
}

export class Squad extends Table {
  #knowledge = null;
  #areas = null;

  constructor(x, config = {}) {
    if (config instanceof Lent) {
      super(x, 'sx_squad', config.handle, config.owner);
      return;
    }
    const { numbers, masks } = squadConfigOf(config);
    putScratch(x, masks);
    super(x, 'sx_squad', x.sx_squad_new(...numbers));
  }

  /** The first column of the game's own (SQUAD.inExtra, plus one for every role beyond two). */
  extraStart() {
    return this.x.sx_squad_extra_start(this.h);
  }

  #out(count) {
    return takeInts(this.x, this.x.sx_squad_out(this.h), count);
  }

  /** Throws PlanError for ERROR_PLAN, RangeError for other errors. */
  #checked(r) {
    if (r === ERROR_PLAN) {
      const [tactic, input] = this.#out(2);
      throw new PlanError(tactic, u32(input));
    }
    if (r < 0) throw new RangeError('Sagax: bad squad arguments');
    return r;
  }

  /** Adds a member {id, pos, roles = 0, skill = 0, state = 'active'|'suspended'|'gone'} at the end. Returns its index, or null if the group is full or the id is there. */
  addMember(member) {
    const values = memberValues(member);
    const h = this.h;
    putScratch(this.x, values);
    const r = this.x.sx_squad_add_member(h);
    if (r === -1) throw new RangeError('Sagax: bad member');
    return r >= 0 ? r : null;
  }

  /** Removes a member (the order of the others stays). False if unknown. */
  removeMember(id) {
    return this.x.sx_squad_remove_member(this.h, u32Of(id, 'id')) === 1;
  }

  /** Sets a member's position. False if unknown. */
  setPos(id, pos) {
    const [px, py, pz] = posOf(pos);
    return this.x.sx_squad_set_pos(this.h, u32Of(id, 'id'), px, py, pz) === 1;
  }

  /** Takes a member out of play for now (no goal, not off plan). False if unknown. */
  suspend(id) {
    return this.x.sx_squad_suspend(this.h, u32Of(id, 'id')) === 1;
  }

  /**
   * Puts a member back into play and, if it was suspended, sends it its goal
   * of the active plan at now over its channel (after the signal: the
   * signal order). False if unknown.
   */
  reinstate(id, now) {
    return this.x.sx_squad_reinstate(this.h, u32Of(id, 'id'), timeOf(now)) === 1;
  }

  /** The old name of reinstate (0.9 only). */
  resume(id, now) {
    return this.reinstate(id, now);
  }

  /** Marks a member dead or lost (counts in SQUAD.inLosses). False if unknown. */
  setGone(id) {
    return this.x.sx_squad_set_gone(this.h, u32Of(id, 'id')) === 1;
  }

  /** The members [{id, pos, roles, skill, state}] in order. */
  members() {
    const n = this.x.sx_squad_members(this.h);
    const all = this.#out(n * MEMBER_SIZE);
    const list = [];
    for (let i = 0; i < n; i++) list.push(memberOf(all.slice(i * MEMBER_SIZE, (i + 1) * MEMBER_SIZE)));
    return list;
  }

  /** Sets the leader. False if the id is not in the group. */
  setLeader(id) {
    return this.x.sx_squad_set_leader(this.h, u32Of(id, 'id')) === 1;
  }

  /** The acting leader's id (the set one while active, else the first active member of the plan, else the first active member), or null. */
  leader() {
    return this.x.sx_squad_leader(this.h) === 1 ? u32(this.#out(1)[0]) : null;
  }

  /** 0 weak .. 255 elite (default 0). */
  setSkill(skill) {
    this.x.sx_squad_set_skill(this.h, u8Of(skill, 'skill'));
  }

  skill() {
    return this.x.sx_squad_skill(this.h);
  }

  /** Sets the skill table (see lib.skillTable for its shape). */
  setSkillTable(table) {
    const values = skillValues(table);
    const h = this.h;
    putScratch(this.x, values);
    this.#checked(this.x.sx_squad_set_skill_table(h));
  }

  skillTable() {
    this.x.sx_squad_skill_table(this.h);
    return skillOf(this.#out(SKILL_SIZE));
  }

  /**
   * Orders (and reports) reach the member after 1 tick per unit of distance
   * from the leader, within range (inclusive). False if the id is not in the
   * group. The default for a member is radio 0, 0: at once.
   */
  setShout(id, range) {
    return this.x.sx_squad_set_shout(this.h, u32Of(id, 'id'), safeOf(range, 'range')) === 1;
  }

  /** Orders reach the member after base + distance * perUnit ticks, at any distance. False if the id is not in the group. */
  setRadio(id, base = 0, perUnit = 0) {
    return this.x.sx_squad_set_radio(this.h, u32Of(id, 'id'), u32Of(base, 'base'), u32Of(perUnit, 'perUnit')) === 1;
  }

  /** Orders never reach the member. False if the id is not in the group. */
  setSilent(id) {
    return this.x.sx_squad_set_silent(this.h, u32Of(id, 'id')) === 1;
  }

  /** The area a danger report adds (kind, radius, strength, lifetime, blocking are used). */
  setDangerArea(area) {
    const h = this.h;
    putScratch(this.x, areaValues(area));
    this.x.sx_squad_set_danger_area(h);
  }

  dangerArea() {
    this.x.sx_squad_danger_area(this.h);
    return areaOf(this.#out(AREA_SIZE).map(u32));
  }

  /** The fact kind in the squad's knowledge that counts as a target (default 0). */
  setTargetKind(kind) {
    this.x.sx_squad_set_target_kind(this.h, u32Of(kind, 'kind'));
  }

  /** How many of the game's own columns each scoring row has from SQUAD.inExtra on (default 0, at most SQUAD.maxExtraColumns). */
  setExtraColumns(count) {
    if (this.x.sx_squad_set_extra_columns(this.h, u32Of(count, 'count')) !== 0) {
      throw new RangeError(`Sagax: at most ${SQUAD.maxExtraColumns} extra columns`);
    }
  }

  /**
   * How far (inclusive, x and y in the squad's metric) a spot may be from a
   * slot's wish point to be taken; beyond it a spot does not count, and with
   * none within the wish point is used unchecked. null: the default, 3 * the
   * slot's dist (at least 1).
   */
  setSpotRadius(radius) {
    const r = radius === null ? -1 : u32Of(radius, 'radius');
    this.x.sx_squad_set_spot_radius(this.h, r);
  }

  /** The squad's own knowledge (a Knowledge; it is freed with the squad). */
  knowledge() {
    this.#knowledge ??= new Knowledge(this.x, new Lent(this.x.sx_squad_knowledge(this.h), this));
    return this.#knowledge;
  }

  /** The squad's own danger areas (an Areas; freed with the squad). */
  areas() {
    this.#areas ??= new Areas(this.x, new Lent(this.x.sx_squad_areas(this.h), this));
    return this.#areas;
  }

  /** The leader's own sighting, merged with source 0 (own). Returns the evicted fact, or null. */
  observe(now, fact) {
    const t = timeOf(now);
    const h = this.h;
    putScratch(this.x, factValues(fact));
    return this.x.sx_squad_observe(h, t) === 1 ? factOf(this.#out(FACT_SIZE).map(u32)) : null;
  }

  /** A member's report (a message, kind 'enemy' | 'arrived' | 'goalBroken' or SQUAD.report*), read from arriveAt on. False if the inbox refused it. */
  report(message, arriveAt) {
    if (message?.kind === 3) throw new RangeError('Sagax: kind 3 is gone, use reportDanger');
    const t = timeOf(arriveAt);
    const named = typeof message?.kind === 'string' ? { ...message, kind: choice(REPORT, message.kind, 'report kind') } : message;
    const values = messageValues(named);
    const h = this.h;
    putScratch(this.x, values);
    return this.x.sx_squad_report(h, t) === 1;
  }

  /**
   * A danger report on its way to the leader: member sender was shot at near
   * centre, from shooter (null: unknown). From arriveAt on, check and plan add
   * a danger area (the setDangerArea template; data = sender, source =
   * shooter). False when the danger queue (config dangers) is full.
   */
  reportDanger({ sender, centre, shooter = null }, arriveAt) {
    const s = u32Of(sender, 'sender');
    const [cx, cy, cz] = posOf(centre, 'centre');
    const [sx, sy, sz] = shooter === null ? [0, 0, 0] : posOf(shooter, 'shooter');
    const t = timeOf(arriveAt);
    return this.x.sx_squad_report_danger(this.h, s, cx, cy, cz, shooter === null ? 0 : 1, sx, sy, sz, t) === 1;
  }

  /** The plan that became active since the last call (once), as its id, or null. */
  takeActivated() {
    return this.x.sx_squad_take_activated(this.h) === 1 ? u32(this.#out(1)[0]) : null;
  }

  /** When a report sent by member id at now reaches the acting leader (at once for the leader, else over its channel), or null if it never arrives. */
  reportArrivesAt(id, now) {
    const i = u32Of(id, 'id');
    const t = timeOf(now);
    return this.x.sx_squad_report_arrives_at(this.h, i, t) === 1 ? u32(this.#out(1)[0]) : null;
  }

  /** The known targets (facts of the target kind, in knowledge order). */
  targets() {
    const n = this.x.sx_squad_targets(this.h);
    const all = this.#out(n * FACT_SIZE).map(u32);
    const list = [];
    for (let i = 0; i < n; i++) list.push(factOf(all.slice(i * FACT_SIZE, (i + 1) * FACT_SIZE)));
    return list;
  }

  /** Takes a tactic over (it cannot be used afterwards). Returns its index. */
  addTactic(tactic) {
    if (!(tactic instanceof Tactic)) throw new TypeError('Sagax: addTactic takes a Tactic');
    if (!tactic.handle) throw new Error('Sagax: this tactic was already given to a squad (or freed)');
    const t = tactic.h;
    const r = this.x.sx_squad_add_tactic(this.h, t);
    tactic.handle = 0;
    return this.#checked(r);
  }

  /** null if every consideration reads a column the rows have and no tactic has more than 255 slots, else {tactic, input} (input 4294967295 for too many slots). */
  validate() {
    if (this.x.sx_squad_validate(this.h) === 0) return null;
    const [tactic, input] = this.#out(2);
    return { tactic, input: u32(input) };
  }

  #run(call, now, { extra = [], spots = [], seed = 0 } = {}) {
    const t = timeOf(now);
    const [low, high] = seedOf(seed);
    const values = [
      ...extra.map((v) => i32Of(v, 'extra')),
      ...spots.flatMap((s) => [...posOf(s.pos, 'spot'), i32(bitsOf(s.tags ?? 0, TAG, 'tags'))]),
    ];
    const h = this.h;
    putScratch(this.x, values);
    return this.#checked(this.x[call](h, t, extra.length, spots.length, low, high));
  }

  /**
   * Makes a new plan, pending until the skill's think time is over. extra:
   * the game's columns (one block for the candidates without a target, then
   * one per target in targets() order); spots: [{pos, tags}] to pick from;
   * seed: the game's. Returns the plan's id, or null if nothing scores.
   * Throws PlanError if a tactic reads a column the rows do not have.
   */
  plan(now, options) {
    return this.#run('sx_squad_plan', now, options) === 1 ? u32(this.#out(1)[0]) : null;
  }

  /**
   * Watches the plans; call it regularly with the same options as plan.
   * Returns {status: 'keep'|'replan'|'signal'|'noPlan', plan: the new id or null}.
   */
  check(now, options) {
    const r = this.#run('sx_squad_check', now, options);
    return { status: SQUAD_STATUS[r], plan: r === 1 ? u32(this.#out(1)[0]) : null };
  }

  /** Takes (at most max of) the orders whose arrival time has come, in the order they were sent. Not on a crew's squad (crew.tick takes them). */
  ordersDue(now, max = ALL) {
    if (this.owner instanceof Crew) throw new TypeError("Sagax: a crew takes its squad's orders itself (crew.tick)");
    const n = this.x.sx_squad_orders_due(this.h, timeOf(now), u32Of(max, 'max'));
    if (n < 0) throw new RangeError('Sagax: bad squad arguments');
    return ordersOf(this.#out(n * ORDER_SIZE), n);
  }

  /** Gives the signal of the active plan, once per plan. False if no plan is active or it was given. */
  signal(now) {
    return this.x.sx_squad_signal(this.h, timeOf(now)) === 1;
  }

  /** True if the signal of the active plan was given. */
  signalGiven() {
    return this.x.sx_squad_signal_given(this.h) === 1;
  }

  /** The member reached its first spot (for the 'allArrived' trigger), seen by the leader at once; a member that must tell the leader sends a SQUAD.reportArrived report instead. False if it has no goal in the active plan. */
  arrived(id, now) {
    return this.x.sx_squad_arrived(this.h, u32Of(id, 'id'), timeOf(now)) === 1;
  }

  #plan(call) {
    if (this.x[call](this.h) === 0) return null;
    const head = this.#out(PLAN_SIZE);
    const all = this.#out(PLAN_SIZE + head[5] * ORDER_SIZE);
    return {
      id: u32(head[0]),
      tactic: head[1],
      target: head[6] ? u32(head[2]) : null,
      madeAt: u32(head[3]),
      activeAt: u32(head[4]),
      goals: ordersOf(all, head[5], PLAN_SIZE),
    };
  }

  /** The plan made last while it waits: {id, tactic, target, madeAt, activeAt, goals}, or null. */
  pendingPlan() {
    return this.#plan('sx_squad_pending_plan');
  }

  /** The plan being carried out, or null. */
  activePlan() {
    return this.#plan('sx_squad_active_plan');
  }
}

/**
 * One member's goal with its postpone state: once the goal stays postponed
 * for giveUpTicks it counts as broken (report it with SQUAD.reportGoalBroken).
 * While suspended the clock does not run.
 */
export class Keeper extends Table {
  constructor(x, lent = null) {
    if (lent instanceof Lent) {
      super(x, 'sx_keeper', lent.handle, lent.owner, lent.epoch);
      return;
    }
    super(x, 'sx_keeper', x.sx_keeper_new());
  }

  #out(count) {
    return takeInts(this.x, this.x.sx_keeper_out(this.h), count);
  }

  /** Takes a goal (from an order), replacing the old one and the postpone state. */
  set(now, goal) {
    const t = timeOf(now);
    const values = goalValues(goal);
    const h = this.h;
    putScratch(this.x, values);
    if (this.x.sx_keeper_set(h, t) !== 0) throw new RangeError('Sagax: bad goal');
  }

  /** The goal (member null), or null. */
  goal() {
    return this.x.sx_keeper_goal(this.h) === 1 ? goalOf(this.#out(GOAL_SIZE)) : null;
  }

  /** Drops the goal and the postpone state. */
  clear() {
    this.x.sx_keeper_clear(this.h);
  }

  /** The member turns to something else for now (the first time stays). */
  goalPostponed(now) {
    this.x.sx_keeper_goal_postponed(this.h, timeOf(now));
  }

  /** Back on the goal: the postpone clock is reset. */
  goalWon(now) {
    this.x.sx_keeper_goal_won(this.h, timeOf(now));
  }

  /** Old name of goalPostponed (for 0.9 only). */
  postpone(now) {
    this.goalPostponed(now);
  }

  /** Old name of goalWon (for 0.9 only). */
  resume(now) {
    this.goalWon(now);
  }

  /**
   * Adds the ready-made "keep my goal" option to a member's decider and
   * returns its number: weight times (has goal) times a falling curve on the
   * distance (full at 0, nothing at 64). The member's rows start with the
   * five keeper columns (KEEPER); its own inputs follow from KEEPER.extra.
   */
  addGoalOption(decider, weight = 1) {
    if (!(decider instanceof Decider)) throw new TypeError('Sagax: addGoalOption takes a Decider');
    const w = fixed(weight, 'weight');
    const option = this.x.sx_keeper_add_goal_option(decider.h, w);
    decider.inputCount = Math.max(decider.inputCount, KEEPER.extra);
    return option;
  }

  /**
   * Marks the goal option as the action going on while the keeper holds a
   * goal that is not postponed, with discipline (a number, as in inputs) as
   * its bonus; otherwise no action is current.
   */
  setCurrent(decider, option, discipline) {
    if (!(decider instanceof Decider)) throw new TypeError('Sagax: setCurrent takes a Decider');
    this.x.sx_keeper_set_current(this.h, decider.h, u32Of(option, 'option'), u32Of(discipline, 'discipline'));
  }

  /** Out of play: never broken. */
  suspend() {
    this.x.sx_keeper_suspend(this.h);
  }

  /** Back in play; a postponed goal's clock restarts at now. */
  unsuspend(now) {
    this.x.sx_keeper_unsuspend(this.h, timeOf(now));
  }

  /** True once the goal has been postponed for at least giveUpTicks (wrap-safe), never while suspended. */
  broken(now, giveUpTicks) {
    return this.x.sx_keeper_broken(this.h, timeOf(now), u32Of(giveUpTicks, 'giveUpTicks')) === 1;
  }

  /**
   * The member's decider inputs at pos: [hasGoal, distToGoal (-1 without a
   * goal or pos; Euclid: the root), goalAge, signalGiven, discipline].
   */
  inputs(now, pos, metric, signalGiven, discipline) {
    const [px, py, pz] = posOf(pos);
    const m = choice(METRIC, metric, 'metric');
    this.x.sx_keeper_inputs(this.h, timeOf(now), px, py, pz, m, signalGiven ? 1 : 0, u32Of(discipline, 'discipline'));
    return this.#out(5);
  }
}

const CREW_KINDS = [null, 'planned', 'activated', 'signal', 'order', 'start', 'arrived', 'broken', 'noPlan'];
const CREW_EVENT_SIZE = 5 + GOAL_SIZE;

function crewEventOf(r) {
  const kind = CREW_KINDS[r[0]];
  switch (kind) {
    case 'planned':
    case 'activated':
      return { kind, plan: u32(r[2]) };
    case 'order': {
      const member = u32(r[1]);
      return { kind, member, plan: u32(r[2]), goal: goalOf(r.slice(5), member), startAt: r[4] ? u32(r[3]) : null };
    }
    case 'start':
    case 'arrived':
    case 'broken':
      return { kind, member: u32(r[1]) };
    default:
      return { kind };
  }
}

/**
 * A squad with one goal keeper per member: the whole group loop in one call
 * per tick. The crew moves nobody; it says what happened.
 *
 *   const crew = lib.crew({ squad: { members: 4, metric: 'chebyshev' } });
 *   crew.addMember({ id: 1, pos: [0, 0, 0] }); ...
 *   crew.squad().setLeader(1); crew.squad().addTactic(lib.tactics.flank(3, 4));
 *   // every tick:
 *   crew.setPos(id, pos);                       // where each member is
 *   for (const e of crew.tick(now, { spots, seed })) ...  // {kind, plan?, member?, goal?, startAt?}
 *   const { row } = crew.memberInputs(id, now);  // the member's decider row starts with it
 *   crew.memberChose(id, now, goalWon);          // what it decided
 *
 * Event kinds, in the order of the tick's steps: 'planned' (plan), 'activated'
 * (plan; members without a goal in it have none now), 'signal', 'order'
 * (member, plan, goal, startAt: the tick it starts or null on the signal),
 * 'start', 'arrived' (the member reported its arrival at its first spot),
 * 'broken' (it gave its goal up and reported it), 'noPlan'.
 *
 * Channels, tactics, skill and leader go on crew.squad() (it lives until
 * crew.free(); its ordersDue throws: the crew takes the orders). crew.keeper(id)
 * is the member's keeper until the next call that changes the crew (anything
 * but memberInputs, goal, squad and keeper) or until the member leaves the
 * group (also through crew.squad()); after that it throws, it never reaches
 * another member's keeper. With hex
 * (a layout as for lib.hex, needs squad metric 'hex') the crew's own calls
 * take and return offset positions; the squad and keepers hold axial ones.
 */
export class Crew extends Table {
  #squad = null;

  constructor(x, { squad = {}, arriveRadius = 0, hex = null } = {}) {
    const { numbers, masks } = squadConfigOf(squad);
    const layout = hex === null ? 0 : layoutOf(hex);
    if (layout !== 0 && numbers[5] !== METRIC.hex) throw new RangeError("Sagax: a hex crew needs squad metric 'hex'");
    putScratch(x, masks);
    super(x, 'sx_crew', x.sx_crew_new(...numbers, u32Of(arriveRadius, 'arriveRadius'), layout));
    /** Moves on with every call that changes the crew (lent keepers go stale). */
    this.epoch = 0;
  }

  /** The handle for a call that changes the crew. */
  get #changed() {
    const h = this.h;
    this.epoch++;
    return h;
  }

  #out(count) {
    return takeInts(this.x, this.x.sx_crew_out(this.h), count);
  }

  /** Adds a member {id, pos, roles = 0, skill = 0, state = 'active'} with an empty keeper. Returns its index, or null if the group is full or the id is there. */
  addMember(member) {
    const values = memberValues(member);
    const h = this.#changed;
    putScratch(this.x, values);
    const r = this.x.sx_crew_add_member(h);
    if (r === -1) throw new RangeError('Sagax: bad member');
    return r >= 0 ? r : null;
  }

  /** Removes a member and its keeper. False if unknown. */
  removeMember(id) {
    return this.x.sx_crew_remove_member(this.#changed, u32Of(id, 'id')) === 1;
  }

  /** Where the member is now. False if unknown. */
  setPos(id, pos) {
    const [px, py, pz] = posOf(pos);
    return this.x.sx_crew_set_pos(this.#changed, u32Of(id, 'id'), px, py, pz) === 1;
  }

  /** Out of play: suspended in the group, its keeper paused (never broken). False if unknown. */
  suspend(id) {
    return this.x.sx_crew_suspend(this.#changed, u32Of(id, 'id')) === 1;
  }

  /** Back in play (as Squad.reinstate); its keeper's clock restarts at now. False if unknown. */
  reinstate(id, now) {
    const i = u32Of(id, 'id');
    return this.x.sx_crew_reinstate(this.#changed, i, timeOf(now)) === 1;
  }

  /** Dead or lost (as Squad.setGone). False if unknown. */
  setGone(id) {
    return this.x.sx_crew_set_gone(this.#changed, u32Of(id, 'id')) === 1;
  }

  /** The leader's own sighting (as Squad.observe). Returns the evicted fact, or null. */
  observe(now, fact) {
    const t = timeOf(now);
    const values = factValues(fact);
    const h = this.#changed;
    putScratch(this.x, values);
    return this.x.sx_crew_observe(h, t) === 1 ? factOf(this.#out(FACT_SIZE).map(u32)) : null;
  }

  /** A danger report on its way to the leader (as Squad.reportDanger). False when the danger queue is full. */
  reportDanger({ sender, centre, shooter = null }, arriveAt) {
    const s = u32Of(sender, 'sender');
    const [cx, cy, cz] = posOf(centre, 'centre');
    const [sx, sy, sz] = shooter === null ? [0, 0, 0] : posOf(shooter, 'shooter');
    const t = timeOf(arriveAt);
    return this.x.sx_crew_report_danger(this.#changed, s, cx, cy, cz, shooter === null ? 0 : 1, sx, sy, sz, t) === 1;
  }

  /**
   * Runs the group at now with the options of Squad.check ({extra, spots,
   * seed}) and returns what happened (see the class). Throws PlanError if a
   * tactic reads a column the rows do not have.
   */
  tick(now, { extra = [], spots = [], seed = 0 } = {}) {
    const t = timeOf(now);
    const [low, high] = seedOf(seed);
    const values = [
      ...extra.map((v) => i32Of(v, 'extra')),
      ...spots.flatMap((s) => [...posOf(s.pos, 'spot'), i32(bitsOf(s.tags ?? 0, TAG, 'tags'))]),
    ];
    const h = this.#changed;
    putScratch(this.x, values);
    const n = this.x.sx_crew_tick(h, t, extra.length, spots.length, low, high);
    if (n === ERROR_PLAN) {
      const [tactic, input] = this.#out(2);
      throw new PlanError(tactic, u32(input));
    }
    if (n < 0) throw new RangeError('Sagax: bad crew arguments');
    const all = this.#out(n * CREW_EVENT_SIZE);
    const events = [];
    for (let i = 0; i < n; i++) events.push(crewEventOf(all.slice(i * CREW_EVENT_SIZE, (i + 1) * CREW_EVENT_SIZE)));
    return events;
  }

  /**
   * The member's decider inputs at now: {hasGoal, dist, age, signal,
   * discipline} (the KEEPER columns, numbers as in Keeper.inputs) and row,
   * the same five as an array (a member's decider row starts with it); null
   * if the id is unknown.
   */
  memberInputs(id, now) {
    const i = u32Of(id, 'id');
    if (this.x.sx_crew_member_inputs(this.h, i, timeOf(now)) !== 1) return null;
    const row = this.#out(5);
    const [hasGoal, dist, age, signal, discipline] = row;
    return { hasGoal, dist, age, signal, discipline, row };
  }

  /** What the member chose at now: its goal (goalWon true: the postpone clock is reset) or something else (the goal is postponed). False if unknown. */
  memberChose(id, now, goalWon) {
    const i = u32Of(id, 'id');
    const t = timeOf(now);
    return this.x.sx_crew_member_chose(this.#changed, i, t, goalWon ? 1 : 0) === 1;
  }

  /** The member's current goal (member null), or null. */
  goal(id) {
    return this.x.sx_crew_goal(this.h, u32Of(id, 'id')) === 1 ? goalOf(this.#out(GOAL_SIZE)) : null;
  }

  /** The crew's own squad (a Squad; freed with the crew). */
  squad() {
    this.#squad ??= new Squad(this.x, new Lent(this.x.sx_crew_squad(this.h), this));
    return this.#squad;
  }

  /** The member's keeper (a Keeper; one small handle per member id ever asked for, kept until the crew is freed) until the next call that changes the crew or the member leaves the group, or null if the id is unknown. */
  keeper(id) {
    const k = this.x.sx_crew_keeper(this.h, u32Of(id, 'id'));
    return k ? new Keeper(this.x, new Lent(k, this, this.epoch)) : null;
  }
}

/** One number of a skill table at skill's level (which: see sx_skill_for); table null: the default. */
function skillEntry(x, which, table, skill) {
  const s = u8Of(skill, 'skill');
  const ptr = table ? putScratch(x, skillValues(table)) : x.sx_scratch(1);
  if (x.sx_skill_for(which, s, table ? 1 : 0) < 0) throw new RangeError('Sagax: bad skill table');
  return u32(takeInts(x, ptr, 1)[0]);
}

/** The shape number of a name ('line' | 'wedge' | 'column') or a SHAPE number. */
function shapeOf(shape) {
  return typeof shape === 'number' ? intIn(shape, 0, 2, 'shape') : choice(SHAPE, shape, 'shape');
}

Object.assign(Sagax.prototype, {
  /** A squad {members, memory, areas, inbox, dangers, metric = 'euclid', roles = ['ranged', 'area']}; see Squad. */
  squad(options) {
    return new Squad(this.x, options);
  },
  /** A member's goal keeper; see Keeper. */
  keeper() {
    return new Keeper(this.x);
  },
  /** A squad with one keeper per member {squad: {...as for squad}, arriveRadius = 0, hex = null}; see Crew. */
  crew(options) {
    return new Crew(this.x, options);
  },
  /** An empty tactic {weight = 1 (1.0), minSkill = 0 (0 .. 255), needsTarget = true}; see Tactic. */
  tactic({ weight = 1, minSkill = 0, needsTarget = true } = {}) {
    return new Tactic(this.x, this.x.sx_tactic_new(fixed(weight, 'weight'), u8Of(minSkill, 'minSkill'), needsTarget ? 1 : 0));
  },
  /** The default skill table {pick, pickTop, thinkTicks, timingSpread, holdBonus, giveUpTicks, spotRankError}, four levels each (skill / 64). */
  skillTable() {
    const ptr = this.x.sx_scratch(SKILL_SIZE);
    this.x.sx_skill_table_default();
    return skillOf(takeInts(this.x, ptr, SKILL_SIZE));
  },
  /** A member's discipline: holdBonus at the level of its own skill (table: the default). Old name of holdBonusFor. */
  discipline(skill, table) {
    return this.holdBonusFor(table, skill);
  },
  /** Ticks from making a plan until it is active, at the level of skill (0 .. 255) in table (null: the default). */
  thinkTicksFor(table, skill) {
    return skillEntry(this.x, 0, table, skill);
  },
  /** Most ticks a member starts late, at skill's level. */
  timingSpreadFor(table, skill) {
    return skillEntry(this.x, 1, table, skill);
  },
  /** The bonus of the active plan, and the members' discipline, at skill's level. */
  holdBonusFor(table, skill) {
    return skillEntry(this.x, 2, table, skill);
  },
  /** Ticks a member may postpone its goal, at skill's level. */
  giveUpTicksFor(table, skill) {
    return skillEntry(this.x, 3, table, skill);
  },
  /** Most ranks a spot may be off the best one, at skill's level. */
  spotRankErrorFor(table, skill) {
    return skillEntry(this.x, 4, table, skill);
  },
  /** How the tactic is chosen at skill's level: {pick: 'best' | 'weighted' | 'uniform', top}. */
  pickFor(table, skill) {
    const s = u8Of(skill, 'skill');
    const ptr = table ? putScratch(this.x, skillValues(table)) : this.x.sx_scratch(2);
    if (this.x.sx_skill_pick_for(s, table ? 1 : 0) < 0) throw new RangeError('Sagax: bad skill table');
    const [pick, top] = takeInts(this.x, ptr, 2);
    return { pick: PICKS[pick], top: u32(top) };
  },
  /** The tick a goal starts when its order arrived at received (wrap-safe), or null for a goal that starts on the signal. */
  startTick(goal, received) {
    const t = timeOf(received);
    const ptr = putScratch(this.x, goalValues(goal));
    const r = this.x.sx_goal_start_tick(t);
    if (r < 0) throw new RangeError('Sagax: bad goal');
    return r === 1 ? u32(takeInts(this.x, ptr, 1)[0]) : null;
  },
  /** True when the member must report its arrival at the first spot: the goal gathers first (thenPos) or waits for the signal. */
  awaitsArrival(goal) {
    putScratch(this.x, goalValues(goal));
    const r = this.x.sx_goal_awaits_arrival();
    if (r < 0) throw new RangeError('Sagax: bad goal');
    return r === 1;
  },
  /**
   * Picks who joins: gather([x, y], faction, radius, candidates, {metric = 'euclid', freeOnly = false, max}).
   * candidates: [{id, faction, pos, free}]. Within radius (inclusive; Euclid: squared
   * compare), nearest first, ties to the lower id; a negative radius reaches nobody.
   * Only x and y count. Returns the ids.
   */
  gather(origin, faction, radius, candidates, { metric = 'euclid', freeOnly = false, max = candidates.length } = {}) {
    const [ox, oy] = originOf(origin);
    const m = choice(METRIC, metric, 'metric');
    const values = candidates.flatMap((c) => [u32Of(c.id, 'id'), u32Of(c.faction, 'faction'), ...posOf(c.pos), c.free ? 1 : 0].map(i32));
    const r = safeOf(radius, 'radius');
    const ptr = putScratch(this.x, values);
    const n = this.x.sx_gather(ox, oy, u32Of(faction, 'faction'), r, m, freeOnly ? 1 : 0, candidates.length, u32Of(max, 'max'));
    if (n < 0) throw new RangeError('Sagax: bad gather arguments');
    return takeInts(this.x, ptr, n).map(u32);
  },
  /**
   * The extra cost of the move from -> to for a path search (only the cell
   * moved to counts): null if a blocking area holds it but neither start nor
   * goal, else the cost as for Areas.cost, at most 2^31 - 2 (as in C). The
   * areas must be in the same (cell) coordinates as the cells.
   */
  areasMoveCost(areas, from, to, start, goal) {
    if (!(areas instanceof Areas)) throw new TypeError('Sagax: areasMoveCost takes an Areas');
    const cells = [from, to, start, goal].flatMap((c) => posOf(c, 'cell'));
    const h = areas.h;
    const ptr = putScratch(this.x, cells);
    const r = this.x.sx_areas_move_cost(h);
    if (r < 0) throw new RangeError('Sagax: bad areas');
    return r === 1 ? u32(takeInts(this.x, ptr, 1)[0]) : null;
  },
});

/** A hex layout, 'oddR' | 'evenR' | 'oddQ' | 'evenQ' or HEX.*, as the number the module takes. */
function layoutOf(layout) {
  const n = typeof layout === 'string' ? HEX[layout] : layout;
  if (!Object.values(HEX).includes(n)) {
    throw new RangeError(`Sagax: hex layout must be one of ${Object.keys(HEX).join(', ')} (or HEX.*), not ${String(layout)}`);
  }
  return n;
}

Object.defineProperty(Sagax.prototype, 'hex', {
  /**
   * Hex converters between offset cells and axial coordinates:
   * toAxial(pos, layout) -> [q, r, z]; fromAxial(pos, layout) -> {x, y, level}.
   * pos takes the shapes of every position; layout is 'oddR' | 'evenR' |
   * 'oddQ' | 'evenQ' or HEX.*. The level (z) is kept.
   */
  get() {
    const x = this.x;
    const convert = (call, pos, layout) => {
      const [a, b, z] = posOf(pos);
      const l = layoutOf(layout);
      const ptr = x.sx_scratch(2);
      if (call(l, a, b) !== 0) throw new RangeError('Sagax: bad hex layout');
      const [c, d] = takeInts(x, ptr, 2);
      return [c, d, z];
    };
    return {
      toAxial: (pos, layout) => convert(x.sx_hex_to_axial, pos, layout),
      fromAxial: (pos, layout) => {
        const [px, py, z] = convert(x.sx_hex_from_axial, pos, layout);
        return { x: px, y: py, level: z };
      },
    };
  },
});

Object.defineProperty(Sagax.prototype, 'tactics', {
  /**
   * The shipped tactics as Tactic objects (numbers are defaults to tune):
   * rush(), formation('line'|'wedge'|'column' or a SHAPE number, spacing), flank(sides 2|3,
   * dist), ambush(), focus(), isolate(), fallBack().
   */
  get() {
    const x = this.x;
    return {
      rush: () => new Tactic(x, x.sx_tactic_rush()),
      formation: (shape, spacing = 0) => new Tactic(x, x.sx_tactic_formation(shapeOf(shape), i32Of(spacing, 'spacing'))),
      flank: (sides, dist = 0) => {
        if (sides !== 2 && sides !== 3) throw new RangeError(`Sagax: flank sides must be 2 or 3, not ${String(sides)}`);
        return new Tactic(x, x.sx_tactic_flank(sides, i32Of(dist, 'dist')));
      },
      ambush: () => new Tactic(x, x.sx_tactic_ambush()),
      focus: () => new Tactic(x, x.sx_tactic_focus()),
      isolate: () => new Tactic(x, x.sx_tactic_isolate()),
      fallBack: () => new Tactic(x, x.sx_tactic_fall_back()),
    };
  },
});
