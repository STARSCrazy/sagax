// Sagax for JavaScript: grid path search compiled to WebAssembly.
// Works in the browser (Foundry) and in Node. No dependencies.
//
//   const lib = await loadSagax(url or bytes);
//   const grid = lib.grid(30, 20);                       // square, a diagonal step costs 1
//   const hexGrid = lib.grid(30, 20, { hex: HEX.oddR });  // hex
//   grid.block(5, 5); grid.addWall(3, 3, 1, 0); grid.addZone(4, 4, 0); grid.setLeaveCost(100);
//   const way = grid.findPath({x: 0, y: 0}, {x: 10, y: 4});  // {cells, status, reached, cost, ...}
//   const area = grid.reachable({x: 0, y: 0}, 6);            // {cells: [{x, y, cost}], ...}
//   grid.free();
//
// Every search answers with the same object:
//   {cells, status, reached, cost, steps, zonesLeft, total, expanded, nodeLimitHit}
//   cost = steps (walking and terrain) + zonesLeft * leave cost
//   status: 'running' (budget used up, call grid.continue()), 'found',
//   'partial' (goal unreachable, way to the closest cell), 'unreachable',
//   'done' (area complete). Ways always begin with the start.

const BLOCKED = 0xffffffff;
const DIRECTIONS = [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [1, -1], [-1, 1], [-1, -1]];
const STATUS = ['idle', 'running', 'found', 'partial', 'unreachable', 'done'];
const OUTPUT = { cells: 0, turns: 1, smooth: 2 };
const TRACE = [null, 'started', 'expanded', 'improved', 'paused', 'finished'];

/** Default rules: tabletop style, a diagonal step costs as much as a straight one. */
const DEFAULT_RULES = { straight: 1, diagonal: 1, fourWays: false, cutCorners: false, hex: null, step: 1 };

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
  return new Sagax(instance.exports);
}

class Sagax {
  constructor(exports) {
    this.x = exports;
  }

  /**
   * A grid of open cells with fixed rules:
   * square {straight, diagonal, fourWays, cutCorners} or hex {hex: HEX.*, step}.
   * smoothMaxExtra: smoothing may cross cells with at most this extra cost (0).
   */
  grid(width, height, rules = {}) {
    return new Grid(this.x, width, height, { ...DEFAULT_RULES, ...rules });
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

export class Grid {
  constructor(x, width, height, rules) {
    if (!(width > 0 && height > 0)) throw new RangeError(`bad grid size ${width} x ${height}`);
    this.x = x;
    this.width = width;
    this.height = height;
    this.rules = rules;
    this.hex = rules.hex;
    const flags = (rules.fourWays ? 1 : 0) | (rules.cutCorners ? 2 : 0);
    this.handle = rules.hex === null
      ? x.sx_grid_new(width, height, 0, rules.straight, rules.diagonal, flags)
      : x.sx_grid_new(width, height, rules.hex, rules.step, 0, 0);
    if (!this.handle) throw new RangeError('Sagax: bad grid size or rules');
    if (rules.smoothMaxExtra) this.x.sx_grid_set_smooth_max_extra(this.handle, rules.smoothMaxExtra);
  }

  // Views are made fresh for every use: they die when the WebAssembly memory grows.
  costs() {
    return new Uint32Array(this.x.memory.buffer, this.x.sx_grid_costs(this.handle), this.width * this.height);
  }
  walls() {
    return new Uint8Array(this.x.memory.buffer, this.x.sx_grid_walls(this.handle), this.width * this.height);
  }
  zones() {
    return new Uint32Array(this.x.memory.buffer, this.x.sx_grid_zones(this.handle), this.width * this.height);
  }

  /** Bulk filling, row by row (width * height entries each). */
  setCosts(array) { this.costs().set(array); }
  setWalls(array) { this.walls().set(array); }
  setAllZones(array) { this.zones().set(array); }

  contains(x, y) {
    return x >= 0 && y >= 0 && x < this.width && y < this.height;
  }

  /** Extra cost of entering x, y (on top of the step cost). */
  setCost(x, y, cost) {
    if (this.contains(x, y)) this.costs()[y * this.width + x] = cost;
  }
  block(x, y) {
    this.setCost(x, y, BLOCKED);
  }

  /** Square grids: a wall between x, y and its neighbour dx, dy away (both ways). */
  addWall(x, y, dx, dy) {
    const there = DIRECTIONS.findIndex(([a, b]) => a === dx && b === dy);
    const back = DIRECTIONS.findIndex(([a, b]) => a === -dx && b === -dy);
    if (there < 0) return;
    const walls = this.walls();
    if (this.contains(x, y)) walls[y * this.width + x] |= 1 << there;
    if (this.contains(x + dx, y + dy)) walls[(y + dy) * this.width + x + dx] |= 1 << back;
  }

  /** Hex grids: the six neighbours of x, y; the index is the direction and wall bit. */
  hexNeighbors(x, y) {
    if (this.x.sx_hex_neighbors(this.handle, x, y) !== 6) throw new RangeError('Sagax: not a hex grid');
    return this.readCells(6, 2);
  }

  /** Hex grids: a wall between x, y and its neighbour in direction dir (both ways). */
  addHexWall(x, y, dir) {
    const n = this.hexNeighbors(x, y)[dir];
    const back = this.hexNeighbors(n.x, n.y).findIndex((c) => c.x === x && c.y === y);
    const walls = this.walls();
    if (this.contains(x, y)) walls[y * this.width + x] |= 1 << dir;
    if (back >= 0 && this.contains(n.x, n.y)) walls[n.y * this.width + n.x] |= 1 << back;
  }

  /** Zone bits of a cell (bit i: inside zone i, e.g. the reach of enemy i). */
  setZones(x, y, bits) {
    if (this.contains(x, y)) this.zones()[y * this.width + x] = bits >>> 0;
  }
  addZone(x, y, zone) {
    if (this.contains(x, y)) this.zones()[y * this.width + x] |= (1 << zone) >>> 0;
  }
  clearZones() {
    this.zones().fill(0);
  }

  /** Cost added for each zone a move leaves (e.g. an attack of opportunity). */
  setLeaveCost(cost) {
    this.leaveCost = cost >>> 0;
    this.x.sx_grid_set_leave_cost(this.handle, this.leaveCost);
  }

  /**
   * Zones the way leaves, cell by cell (each one e.g. an attack of
   * opportunity). cells: the whole way (output 'cells').
   */
  zonesLeft(cells) {
    const zones = this.zones();
    let left = 0;
    for (let i = 1; i < cells.length; i++) {
      const a = zones[cells[i - 1].y * this.width + cells[i - 1].x];
      const b = zones[cells[i].y * this.width + cells[i].x];
      let bits = (a & ~b) >>> 0;
      while (bits) { bits = (bits & (bits - 1)) >>> 0; left++; }
    }
    return left;
  }

  readCells(count, stride) {
    const out = new Int32Array(this.x.memory.buffer, this.x.sx_out(this.handle), count * stride);
    const cells = new Array(count);
    for (let i = 0; i < count; i++) {
      cells[i] = stride === 3
        ? { x: out[i * 3], y: out[i * 3 + 1], cost: out[i * 3 + 2] }
        : { x: out[i * 2], y: out[i * 2 + 1] };
    }
    return cells;
  }

  writeInput(numbers) {
    const ptr = this.x.sx_input(this.handle, numbers.length);
    new Int32Array(this.x.memory.buffer, ptr, numbers.length).set(numbers);
  }

  answer(n, stride, split = false) {
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    const r = new Int32Array(this.x.memory.buffer, this.x.sx_result(this.handle), 5);
    const status = STATUS[r[0]];
    const cells = this.readCells(n, stride);
    // Ways: split the cost into walking (steps and terrain) and zone penalty.
    let zonesLeft = 0;
    if (split && this.leaveCost && n > 1) {
      const whole = this.x.sx_way(this.handle, 0);
      if (whole > 1) zonesLeft = this.zonesLeft(this.readCells(whole, 2));
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
    const goals = Array.isArray(goal) ? goal : [goal];
    this.writeInput(goals.flatMap((g) => [g.x, g.y]));
    this.last = 2;
    return this.answer(this.x.sx_find_path(this.handle, start.x, start.y, goals.length, budget, outputOf(output)), 2, true);
  }

  /**
   * A way away from threats ([{x, y}]): to the cell within maxCost that the
   * nearest threat needs the longest walk to reach. Status 'found', or
   * 'unreachable' (only the start) if nothing is farther than here. To flee
   * off the map, use findPath with the exits as goals.
   */
  flee(start, threats, maxCost, { output = 'cells' } = {}) {
    this.writeInput(threats.flatMap((t) => [t.x, t.y]));
    this.last = 2;
    return this.answer(this.x.sx_flee(this.handle, start.x, start.y, threats.length, maxCost, outputOf(output)), 2, true);
  }

  /** Every cell reachable from start at a cost of at most maxCost: cells [{x, y, cost}]. */
  reachable(start, maxCost, { budget = 0 } = {}) {
    this.last = 3;
    return this.answer(this.x.sx_reachable(this.handle, start.x, start.y, maxCost, budget), 3);
  }

  /** After status 'running': goes on with the last findPath / reachable. */
  continue(budget = 0) {
    return this.answer(this.x.sx_continue(this.handle, budget), this.last ?? 2, (this.last ?? 2) === 2);
  }

  /** After reachable() or findPath(): the way from that start to x, y, or null. */
  pathTo(x, y, output = 'cells') {
    const a = this.answer(this.x.sx_path_to(this.handle, x, y, outputOf(output)), 2, true);
    return a.reached ? a : null;
  }

  /** The current way once more, as 'cells', 'turns' or 'smooth'. */
  way(output = 'cells') {
    const n = this.x.sx_way(this.handle, outputOf(output));
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    return this.readCells(n, 2);
  }

  /**
   * Along the current way (the leader's): a target per member for every cell.
   * slots: [[forward, right], ...] relative to the walking direction
   * (default: 2 x 2 block; on hex 'right' is 60 degrees clockwise, so the
   * default is a compact group of four; FORMATION.wedgeHex is the hex wedge).
   * @returns {{leader: {x,y}[], members: {x,y}[][]}}
   */
  formation(slots = FORMATION.block) {
    this.writeInput(slots.flat());
    const n = this.x.sx_formation(this.handle, slots.length);
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    const rows = this.readCells(n * (slots.length + 1), 2);
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
    this.writeInput(goals.flatMap((g) => [g.x, g.y]));
    const n = this.x.sx_flow_build(this.handle, goals.length, maxCost);
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    return n;
  }

  /**
   * For each walker the next cell towards the nearest goal and the walker's
   * cost to it: [{x, y, cost}]. At a goal, outside the field (cost -1) or
   * stuck the walker's own cell.
   */
  flowNext(walkers) {
    this.writeInput(walkers.flatMap((w) => [w.x, w.y]));
    if (this.x.sx_flow_next(this.handle, walkers.length) < 0) throw new RangeError('Sagax: bad arguments');
    return this.readCells(walkers.length, 3);
  }

  /** Cost from x, y to the nearest goal of the flow field, or null. */
  flowCost(x, y) {
    const c = this.flowNext([{ x, y }])[0].cost;
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
    this.x.sx_dstar_begin(this.handle, start.x, start.y, goal.x, goal.y);
  }

  /** Works on the plan; on 'found' the way from the walker to the goal. */
  dstarCompute({ budget = 0, output = 'cells' } = {}) {
    return this.answer(this.x.sx_dstar_compute(this.handle, budget, outputOf(output)), 2);
  }

  dstarMoveStart(cell) {
    this.x.sx_dstar_move_start(this.handle, cell.x, cell.y);
  }

  /** Tell D* Lite which cells changed (after setCost / block / addWall). */
  dstarCellsChanged(cells) {
    this.writeInput(cells.flatMap((c) => [c.x, c.y]));
    return this.x.sx_dstar_cells_changed(this.handle, cells.length);
  }

  /**
   * Tracing: every step of the following searches (findPath, reachable,
   * buildFlow, dstarCompute) is kept until takeTrace(). Off by default; off
   * it costs nothing.
   */
  setTrace(on) {
    this.x.sx_trace(this.handle, on ? 1 : 0);
  }

  /**
   * The steps since the last call: [{kind, x, y, parent, cost, estimate, expanded, found}],
   * kind 'started' | 'expanded' | 'improved' | 'paused' | 'finished'.
   * dropped counts steps lost because more than 200000 were waiting.
   */
  takeTrace() {
    const n = this.x.sx_trace_take(this.handle);
    const dropped = this.x.sx_trace_dropped(this.handle);
    const raw = new Int32Array(this.x.memory.buffer, this.x.sx_out(this.handle), n * 9);
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
export class Decider {
  constructor(x) {
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
    const option = this.x.sx_decider_add_option(this.handle, fixed(weight, 'weight'));
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
    const r = this.x.sx_decider_add_weighted(this.handle, option, input, k, min | 0, max | 0, fixed(pull, 'pull'), a);
    if (r !== 0) throw new RangeError(`Sagax: no option ${option}`);
    if (input < MEMORY.sinceOption) this.inputCount = Math.max(this.inputCount, input + 1);
  }

  /**
   * Memory of actions for cooldowns and dislike. Time is the game's own count
   * (frames, turns); setNow before deciding, record what really happened.
   */
  setNow(now) {
    this.x.sx_decider_set_now(this.handle, now >>> 0);
  }
  record({ option, target = 0 }, now) {
    this.x.sx_decider_record(this.handle, option, target >>> 0, now >>> 0);
  }
  forget() {
    this.x.sx_decider_forget(this.handle);
  }
  /** Counts look back less than time (0: all of the last 256 actions). */
  setWindow(time) {
    this.x.sx_decider_set_window(this.handle, time >>> 0);
  }
  /** After the option was done it scores 0 until time has passed (0: none). */
  setCooldown(option, time) {
    if (this.x.sx_decider_set_cooldown(this.handle, option, time >>> 0) !== 0) {
      throw new RangeError(`Sagax: no option ${option}`);
    }
  }

  /**
   * How factors are put together: 'product' (default), 'geometricMean',
   * 'minimum', 'average' or 'median'. Without option: for every option;
   * with option: that one only (null: follow the decider again).
   */
  setCombine(combine, option) {
    const c = combine === null ? -1 : combineOf(combine);
    if (this.x.sx_decider_set_combine(this.handle, option ?? -1, c) !== 0) {
      throw new RangeError('Sagax: bad combine or option');
    }
  }

  /** Rank of an option: only the highest rank that still scores is chosen from. */
  setRank(option, rank) {
    if (this.x.sx_decider_set_rank(this.handle, option, rank >>> 0) !== 0) {
      throw new RangeError(`Sagax: no option ${option}`);
    }
  }

  /** The action going on now ({option, target} or null) scores times (1 + bonus). */
  setCurrent(current, bonus = 0.1) {
    const has = current ? 1 : 0;
    this.x.sx_decider_set_current(this.handle, has, current?.option ?? 0, current?.target ?? 0, fixed(bonus, 'bonus'));
  }

  /**
   * Scores every candidate {option, target, inputs: [raw integers]} and chooses one.
   * pick: 'best' (equal scores: the earlier one), 'weighted' or 'uniform' among the
   * top best (0: all), driven by seed (integer below 2^53). Returns
   * {index, option, target, score, scores} or null if every score is 0.
   */
  decide(candidates, { pick = 'best', top = 0, seed = 0 } = {}) {
    const p = PICK[pick];
    if (p === undefined) throw new RangeError(`Sagax: pick must be best, weighted or uniform, not ${pick}`);
    const stride = this.inputCount;
    const row = stride + 2;
    const n = candidates.length;
    const ptr = this.x.sx_decider_input(this.handle, n * row);
    const buf = new Int32Array(this.x.memory.buffer, ptr, n * row);
    candidates.forEach((c, i) => {
      buf[i * row] = c.option;
      buf[i * row + 1] = c.target ?? 0;
      for (let k = 0; k < stride; k++) buf[i * row + 2 + k] = c.inputs?.[k] ?? 0;
    });
    const low = seed % 0x100000000;
    const high = Math.floor(seed / 0x100000000);
    const r = this.x.sx_decider_decide(this.handle, n, stride, p, top, low >>> 0, high >>> 0);
    if (r === -1) throw new RangeError('Sagax: bad candidates (unknown option?)');
    const scores = Array.from(new Uint32Array(this.x.memory.buffer, this.x.sx_decider_out(this.handle), n));
    if (r < 0) return null;
    return { index: r, option: candidates[r].option, target: candidates[r].target ?? 0, score: scores[r], scores };
  }

  /** Tracing of decisions on or off. */
  setTrace(on) {
    this.x.sx_decider_trace(this.handle, on ? 1 : 0);
  }

  /**
   * Events since the last call: [{kind, candidate, consideration, input, value, score}],
   * kind 'factor' (value: factor of the raw input), 'scored' (value: product before
   * the weight) or 'chosen'.
   */
  takeTrace() {
    const n = this.x.sx_decider_trace_take(this.handle);
    const raw = new Int32Array(this.x.memory.buffer, this.x.sx_decider_out(this.handle), n * 6);
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
export class Machine {
  constructor(x, states, conditions, initial) {
    if (conditions.length > 64) throw new RangeError('Sagax: at most 64 conditions');
    this.x = x;
    this.states = states;
    this.conditions = conditions;
    this.handle = x.sx_machine_new(states.length, this.stateOf(initial));
    if (!this.handle) throw new RangeError('Sagax: bad states');
  }

  stateOf(name) {
    const i = typeof name === 'number' ? name : this.states.indexOf(name);
    if (!(i >= 0 && i < this.states.length)) throw new RangeError(`Sagax: unknown state ${name}`);
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
    const i = this.x.sx_machine_add_transition(this.handle, f, this.stateOf(to), wl, wh, ul, uh, after >>> 0);
    if (i < 0) throw new RangeError('Sagax: bad transition');
    return i;
  }

  /** After time in state, go to next. */
  timeout(state, time, next) {
    if (this.x.sx_machine_set_timeout(this.handle, this.stateOf(state), time >>> 0, this.stateOf(next)) !== 0) {
      throw new RangeError('Sagax: bad timeout');
    }
  }

  /** One tick; flags {name: true}. Returns {from, to, reason, transition} or null. */
  update(now, flags = {}) {
    const [l, h] = this.bits(Object.keys(flags).filter((k) => flags[k]));
    if (this.x.sx_machine_update(this.handle, now >>> 0, l, h) !== 1) return null;
    const c = new Int32Array(this.x.memory.buffer, this.x.sx_machine_change(this.handle), 4);
    return { from: this.states[c[0]], to: this.states[c[1]], reason: REASON[c[2]], transition: c[3] };
  }

  get state() {
    return this.states[this.x.sx_machine_state(this.handle)];
  }

  /** When the current state was entered. */
  get entered() {
    return this.x.sx_machine_entered(this.handle) >>> 0;
  }

  /** Puts the machine into state at now (also to start the clock). */
  setState(state, now) {
    this.x.sx_machine_set_state(this.handle, this.stateOf(state), now >>> 0);
  }

  free() {
    if (this.handle) this.x.sx_machine_free(this.handle);
    this.handle = 0;
  }
}

// ---- world memory -----------------------------------------------------------

const u32 = (v) => v >>> 0;
const i32 = (v) => v | 0;

/** A capacity: an integer from 1 to 2^31 - 1, else RangeError. */
function capacityOf(c) {
  if (!Number.isInteger(c) || c < 1 || c > 0x7fffffff) throw new RangeError("Sagax: capacity must be an integer from 1 to 2147483647");
  return c;
}

/**
 * What a character has seen: the last place and time of things, by id, for at
 * most `capacity` ids (the oldest is dropped when full). Times are the game's
 * own u32 count; ages wrap around.
 *
 *   const s = lib.sightings(8);
 *   s.seen(42, [x, y, z], now);   // null, or the id that was dropped
 *   s.get(42);                    // {id, pos, lastSeen, data} or null
 */
export class Sightings {
  constructor(x, capacity) {
    this.x = x;
    this.handle = x.sx_sightings_new(capacityOf(capacity));
    if (!this.handle) throw new RangeError('Sagax: capacity must be at least 1');
  }

  record() {
    const r = new Uint32Array(this.x.memory.buffer, this.x.sx_sightings_record(this.handle), 6);
    return { id: r[0], pos: [i32(r[1]), i32(r[2]), i32(r[3])], lastSeen: r[4], data: r[5] };
  }

  ids(n) {
    return Array.from(new Uint32Array(this.x.memory.buffer, this.x.sx_sightings_out(this.handle), n));
  }

  /** id seen at pos at now. Returns the evicted id, or null. */
  seen(id, [px, py, pz], now) {
    return this.x.sx_sightings_seen(this.handle, u32(id), i32(px), i32(py), i32(pz), u32(now)) === 1
      ? this.ids(1)[0]
      : null;
  }

  /** Renews the time of id. False if unknown. */
  touch(id, now) {
    return this.x.sx_sightings_touch(this.handle, u32(id), u32(now)) === 1;
  }

  /** Sets the place of id. False if unknown. */
  setPos(id, [px, py, pz]) {
    return this.x.sx_sightings_set_pos(this.handle, u32(id), i32(px), i32(py), i32(pz)) === 1;
  }

  /** Sets the number attached to id. False if unknown. */
  setData(id, data) {
    return this.x.sx_sightings_set_data(this.handle, u32(id), u32(data)) === 1;
  }

  /** Forgets id. False if unknown. */
  remove(id) {
    return this.x.sx_sightings_remove(this.handle, u32(id)) === 1;
  }

  /** {id, pos, lastSeen, data} or null. */
  get(id) {
    return this.x.sx_sightings_get(this.handle, u32(id)) === 1 ? this.record() : null;
  }

  /** All sightings in slot order. */
  list() {
    const n = this.x.sx_sightings_count(this.handle);
    const all = [];
    for (let i = 0; i < n; i++) {
      this.x.sx_sightings_at(this.handle, i);
      all.push(this.record());
    }
    return all;
  }

  /** Forgets sightings at least maxAge old. Returns their ids. */
  expire(now, maxAge) {
    return this.ids(this.x.sx_sightings_expire(this.handle, u32(now), u32(maxAge)));
  }

  clear() {
    this.x.sx_sightings_clear(this.handle);
  }

  free() {
    if (this.handle) this.x.sx_sightings_free(this.handle);
    this.handle = 0;
  }
}

/**
 * Areas to stay out of: round zones of one radius that fade after lifetime.
 *
 *   const z = lib.zones(4, 600, 3000);
 *   z.add([x, y, 0], now, data, [sx, sy, sz]);  // source is optional
 *   z.blocks(cell, start, goal);               // for a path finder
 */
export class Zones {
  constructor(x, capacity, radius, lifetime) {
    this.x = x;
    this.handle = x.sx_zones_new(capacityOf(capacity), i32(radius), u32(lifetime));
    if (!this.handle) throw new RangeError('Sagax: capacity must be at least 1');
  }

  /** Adds or renews a zone at pos; returns its index. */
  add([px, py, pz], now, data = 0, source = null) {
    const [sx, sy, sz] = source ?? [0, 0, 0];
    return this.x.sx_zones_add(this.handle, i32(px), i32(py), i32(pz), u32(now), u32(data), source ? 1 : 0, i32(sx), i32(sy), i32(sz));
  }

  /** Removes zones whose age is at least the lifetime. Returns how many. */
  expire(now) {
    return this.x.sx_zones_expire(this.handle, u32(now));
  }

  /** Index of the first zone containing pos, or null. */
  contains([px, py, pz]) {
    const i = this.x.sx_zones_contains(this.handle, i32(px), i32(py), i32(pz));
    return i >= 0 ? i : null;
  }

  /** True if cell is in a zone that holds neither start nor goal. */
  blocks(cell, start, goal) {
    const p = scratch(this.x, 9, [...cell, ...start, ...goal]);
    return this.x.sx_zones_blocks(this.handle, p.byteOffset) === 1;
  }

  /** All zones in slot order: {pos, since, data, source (or null)}. */
  list() {
    const n = this.x.sx_zones_count(this.handle);
    const all = [];
    for (let i = 0; i < n; i++) {
      this.x.sx_zones_at(this.handle, i);
      const r = new Int32Array(this.x.memory.buffer, this.x.sx_zones_record(this.handle), 9);
      all.push({
        pos: [r[0], r[1], r[2]],
        since: u32(r[3]),
        data: u32(r[4]),
        source: r[8] ? [r[5], r[6], r[7]] : null,
      });
    }
    return all;
  }

  free() {
    if (this.handle) this.x.sx_zones_free(this.handle);
    this.handle = 0;
  }
}

/** Ids to leave alone for a while; the oldest is replaced when full. */
export class Ignore {
  constructor(x, capacity) {
    this.x = x;
    this.handle = x.sx_ignore_new(capacityOf(capacity));
    if (!this.handle) throw new RangeError('Sagax: capacity must be at least 1');
  }

  /** Ignore id from now (renews a present id). */
  add(id, now) {
    this.x.sx_ignore_add(this.handle, u32(id), u32(now));
  }

  /** True if id was added less than duration ago. */
  ignores(id, now, duration) {
    return this.x.sx_ignore_ignores(this.handle, u32(id), u32(now), u32(duration)) === 1;
  }

  free() {
    if (this.handle) this.x.sx_ignore_free(this.handle);
    this.handle = 0;
  }
}

/** Chasing one target; same integer rules as the Rust and C versions. */
export class Pursuit {
  constructor() {
    this.clear();
  }

  /** Starts chasing target, seen at pos, distance away, at now. */
  begin(target, pos, distance, now) {
    this.target = u32(target);
    this.lastPos = pos.map(i32);
    this.unseen = 0;
    this.best = i32(distance);
    this.since = u32(now);
  }

  /** The target is in sight at pos. */
  seen(pos) {
    this.lastPos = pos.map(i32);
    this.unseen = 0;
  }

  /** Another step of time without seeing the target (saturating). */
  missed(step) {
    this.unseen = Math.min(this.unseen + u32(step), 0xffffffff);
  }

  /** Restarts the progress check from distance at now. */
  resetProgress(distance, now) {
    this.best = i32(distance);
    this.since = u32(now);
  }

  /** True if the target has been out of sight for at least limit. */
  lost(limit) {
    return this.unseen >= u32(limit);
  }

  /** True if no progress was made for at least limit; getting minGain closer restarts the check. */
  stalled(distance, now, minGain, limit) {
    if (this.best - i32(distance) >= i32(minGain)) {
      this.best = i32(distance);
      this.since = u32(now);
      return false;
    }
    return u32(now - this.since) >= u32(limit);
  }

  /** Forgets the chase. */
  clear() {
    this.target = 0;
    this.lastPos = [0, 0, 0];
    this.unseen = 0;
    this.best = 0;
    this.since = 0;
  }
}

/** A call for help: delivered after a delay, not repeated before a cooldown. */
export class Radio {
  constructor() {
    this.pos = [0, 0, 0];
    this.started = 0;
    this.lastStart = 0;
    this.pending = false;
    this.hasSent = false;
  }

  /** Starts a call at pos. Refused (false) while one is pending or within the cooldown. */
  start(pos, now, cooldown) {
    if (this.pending) return false;
    if (this.hasSent && u32(now - this.lastStart) < u32(cooldown)) return false;
    this.pos = pos.map(i32);
    this.started = u32(now);
    this.lastStart = u32(now);
    this.pending = true;
    this.hasSent = true;
    return true;
  }

  /** True if a call is pending and at least delay has passed. */
  due(now, delay) {
    return this.pending && u32(now - this.started) >= u32(delay);
  }

  /** Takes the place of the pending call (or null), ending the call. */
  take() {
    if (!this.pending) return null;
    this.pending = false;
    return [...this.pos];
  }

  /** Drops a pending call; the cooldown keeps running. */
  cancel() {
    this.pending = false;
  }
}

/** A timer for "every N ticks"; never started counts as due. */
export class Interval {
  constructor() {
    this.reset();
  }

  /** True if never marked, or at least every has passed since the mark. */
  due(now, every) {
    return !this.started || u32(now - this.last) >= u32(every);
  }

  /** Records that it ran at now. */
  mark(now) {
    this.last = u32(now);
    this.started = true;
  }

  /** Back to never started. */
  reset() {
    this.last = 0;
    this.started = false;
  }
}

const PATTERN_DEFAULT = {
  headingStep: 0x62,
  firstDistance: 0x100,
  offsets: [0, 0x20, -0x20],
  distances: [5 * 256, 9 * 256, 12 * 256],
  min: [0, 0],
  max: [32767, 32767],
};

/** The scratch buffer of the module, `count` integers long, filled from values. */
function scratch(x, count, values = []) {
  const ptr = x.sx_scratch(count);
  const view = new Int32Array(x.memory.buffer, ptr, Math.max(count, 1));
  view.set(values.map(i32));
  return view;
}

/** The ids of the k candidates {id, x, y} nearest to [px, py], nearest first. */
function nearestOf(x, candidates, [px, py], k, divisor = 1) {
  const n = candidates.length;
  const ptr = scratch(x, Math.max(n * 3, Math.min(k, n), 1), candidates.flatMap((c) => [c.id, c.x, c.y])).byteOffset;
  const count = x.sx_nearest(n, i32(px), i32(py), u32(k), i32(divisor));
  if (count < 0) throw new RangeError('Sagax: bad candidates');
  return Array.from(new Uint32Array(x.memory.buffer, ptr, count));
}

/** Search point of an agent: [x, y, z] or null past the last step. */
function searchPointOf(x, agent, step, [cx, cy, cz], pattern) {
  let custom = 0;
  let at;
  if (pattern) {
    const p = { ...PATTERN_DEFAULT, ...pattern };
    if (p.offsets.length > 8 || p.distances.length < p.offsets.length) {
      throw new RangeError('Sagax: a search pattern has at most 8 steps, one distance each');
    }
    const fill = (list) => [...list, ...new Array(8 - list.length).fill(0)];
    const offsets = fill(p.offsets);
    const distances = fill(p.distances.slice(0, p.offsets.length));
    at = scratch(x, 23, [p.headingStep, p.firstDistance, p.offsets.length, ...offsets, ...distances, ...p.min, ...p.max]).byteOffset;
    custom = 1;
  } else {
    at = scratch(x, 3).byteOffset;
  }
  const r = x.sx_search_point(u32(agent), u32(step), i32(cx), i32(cy), i32(cz), custom);
  if (r < 0) throw new RangeError('Sagax: bad search pattern');
  if (r === 0) return null;
  return Array.from(new Int32Array(x.memory.buffer, at, 3));
}

/** Heading while looking around; offsets default to a quarter turn each way. */
function lookHeadingOf(x, base, elapsed, stepTicks, offsets) {
  if (!offsets) return x.sx_look_heading(i32(base), u32(elapsed), u32(stepTicks), -1);
  scratch(x, offsets.length, offsets);
  return x.sx_look_heading(i32(base), u32(elapsed), u32(stepTicks), offsets.length);
}

Object.assign(Sagax.prototype, {
  /** Sightings for at most capacity ids; see Sightings. */
  sightings(capacity) {
    return new Sightings(this.x, capacity);
  },
  /** Zones to stay out of; see Zones. */
  zones(capacity, radius, lifetime) {
    return new Zones(this.x, capacity, radius, lifetime);
  },
  /** An ignore list for at most capacity ids; see Ignore. */
  ignore(capacity) {
    return new Ignore(this.x, capacity);
  },
  /** nearest([{id, x, y}], [px, py], k, divisor = 1): ids of the k nearest, nearest first. */
  nearest(candidates, point, k, divisor = 1) {
    return nearestOf(this.x, candidates, point, k, divisor);
  },
  /** Sine of a heading (256 = full turn), times 256. */
  sin256(heading) {
    return this.x.sx_sin256(i32(heading));
  },
  /**
   * Point where an agent searches: searchPoint(agent, step, [x, y, z], pattern?).
   * pattern: {headingStep, firstDistance, offsets, distances, min: [x, y], max: [x, y]}.
   */
  searchPoint(agent, step, centre, pattern) {
    return searchPointOf(this.x, agent, step, centre, pattern);
  },
  /** lookHeading(base, elapsed, stepTicks, offsets?): heading 0..255 while looking around. */
  lookHeading(base, elapsed, stepTicks, offsets) {
    return lookHeadingOf(this.x, base, elapsed, stepTicks, offsets);
  },
});
