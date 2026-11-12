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

function factValues(f) {
  const [px, py, pz] = f.pos ?? [0, 0, 0];
  return [u32(f.subject), u32(f.kind), u32(px), u32(py), u32(pz), u32(f.seenAt), u32(f.strength ?? 0), u32(f.data ?? 0), u32(f.source ?? 0) & 0xff];
}

function factOf(r) {
  return { subject: r[0], kind: r[1], pos: [i32(r[2]), i32(r[3]), i32(r[4])], seenAt: r[5], source: r[8], strength: r[6], data: r[7] };
}

function messageValues(m) {
  const [px, py, pz] = m.pos ?? [0, 0, 0];
  return [u32(m.kind), u32(m.subject), u32(px), u32(py), u32(pz), u32(m.seenAt), u32(m.strength ?? 0), u32(m.data ?? 0), u32(m.sender ?? 0), m.keepIfSenderGone ? 1 : 0];
}

function messageOf(r) {
  return { kind: r[0], subject: r[1], pos: [i32(r[2]), i32(r[3]), i32(r[4])], seenAt: r[5], strength: r[6], data: r[7], sender: r[8], keepIfSenderGone: r[9] !== 0 };
}

function areaValues(a) {
  const [px, py, pz] = a.pos ?? [0, 0, 0];
  const [sx, sy, sz] = a.source ?? [0, 0, 0];
  return [u32(a.kind ?? 0), u32(px), u32(py), u32(pz), u32(a.radius), u32(a.strength ?? 0), u32(a.since ?? 0), u32(a.lifetime), u32(a.data ?? 0), u32(sx), u32(sy), u32(sz), a.blocking ? 1 : 0, a.source ? 1 : 0];
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

/** A handle to a table in the module; every call checks that it is not freed (a freed handle would write to address 0). */
class Table {
  constructor(x, prefix, handle) {
    if (!handle) throw new RangeError('Sagax: capacity must be at least 1');
    this.x = x;
    this.prefix = prefix;
    this.handle = handle;
  }

  get h() {
    if (!this.handle) throw new Error('Sagax: used after free()');
    return this.handle;
  }

  /** Frees the table. Using it afterwards throws. */
  free() {
    if (this.handle) this.x[`${this.prefix}_free`](this.handle);
    this.handle = 0;
  }
}

/**
 * What a character knows: facts keyed by (subject, kind), for at most
 * `capacity` of them. When full, the oldest (or the weakest) goes.
 *
 *   const k = lib.knowledge(8, 'oldest');   // or 'weakest'
 *   k.learn({subject: 42, kind: 1, pos: [x, y, 0], seenAt: now}, now);  // null, or the evicted fact
 *   k.get(42, 1);     // {subject, kind, pos, seenAt, source, strength, data} or null
 *
 * source: 0 own, 1 heard, 2 told (more values are yours to use).
 */
export class Knowledge extends Table {
  constructor(x, capacity, evict = 'oldest') {
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

  #put(fact, now, call) {
    putRecord(this.x, this.x.sx_knowledge_record(this.h), factValues(fact));
    return this.x[call](this.h, u32(now)) === 1 ? this.#facts(1)[0] : null;
  }

  /** Stores the fact, overwriting a known one. Returns the evicted fact, or null. */
  learn(fact, now) {
    return this.#put(fact, now, 'sx_knowledge_learn');
  }

  /** Like learn, but a known fact is only overwritten by one that is not older. */
  merge(fact, now) {
    return this.#put(fact, now, 'sx_knowledge_merge');
  }

  /** learn for a message {kind, subject, pos, seenAt, strength, data}, with the given source. */
  learnMessage(message, source, now) {
    return this.learn({ ...message, source }, now);
  }

  /** merge for a message. */
  mergeMessage(message, source, now) {
    return this.merge({ ...message, source }, now);
  }

  /** Renews the time of a fact. False if unknown. */
  touch(subject, kind, now) {
    return this.x.sx_knowledge_touch(this.h, u32(subject), u32(kind), u32(now)) === 1;
  }

  /** Sets the place of a fact. False if unknown. */
  setPos(subject, kind, [px, py, pz]) {
    return this.x.sx_knowledge_set_pos(this.h, u32(subject), u32(kind), i32(px), i32(py), i32(pz)) === 1;
  }

  /** Sets the data of a fact. False if unknown. */
  setData(subject, kind, data) {
    return this.x.sx_knowledge_set_data(this.h, u32(subject), u32(kind), u32(data)) === 1;
  }

  /** Sets the strength of a fact. False if unknown. */
  setStrength(subject, kind, strength) {
    return this.x.sx_knowledge_set_strength(this.h, u32(subject), u32(kind), u32(strength)) === 1;
  }

  /** Forgets one fact. False if unknown. */
  remove(subject, kind) {
    return this.x.sx_knowledge_remove(this.h, u32(subject), u32(kind)) === 1;
  }

  /** Forgets all facts about a subject. Returns how many. */
  removeSubject(subject) {
    return this.x.sx_knowledge_remove_subject(this.h, u32(subject));
  }

  /** The fact (subject, kind) or null. */
  get(subject, kind) {
    return this.x.sx_knowledge_get(this.h, u32(subject), u32(kind)) === 1 ? this.#fact() : null;
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
    if (!perKind) return this.#facts(this.x.sx_knowledge_expire(this.h, u32(now), u32(maxAge)));
    const gone = [];
    for (const f of this.list()) {
      const limit = perKind(f);
      if (typeof limit !== 'number') throw new TypeError('Sagax: perKind must return a number');
      // Infinity (or any integer past 2^32 - 1) never expires; other bad limits are refused.
      if (limit === Infinity || limit > ALL) continue;
      if (!Number.isInteger(limit) || limit < 0) throw new RangeError('Sagax: perKind must return a whole number of ticks, 0 or more (Infinity: never)');
      if (u32(u32(now) - f.seenAt) >= limit) gone.push(f);
    }
    for (const f of gone) this.remove(f.subject, f.kind);
    return gone;
  }

  /** Like expire, for facts of one kind only (other kinds stay). */
  expireKind(now, kind, maxAge) {
    return this.#facts(this.x.sx_knowledge_expire_kind(this.h, u32(now), u32(kind), u32(maxAge)));
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
 *   o.post(msg, now, 102, 170);   // 'ok', 'full' or 'cooldown'
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
  post(message, now, delay = 0, cooldown = 0) {
    putRecord(this.x, this.x.sx_outbox_record(this.h), messageValues(message));
    return POST[this.x.sx_outbox_post(this.h, u32(now), u32(delay), u32(cooldown))];
  }

  /** Takes (at most max of) the messages whose own delay is over, in post order. */
  due(now, max = ALL) {
    const n = this.x.sx_outbox_due(this.h, u32(now), u32(max));
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
    putRecord(this.x, this.x.sx_inbox_record(this.h), messageValues(message));
    return this.x.sx_inbox_push(this.h, u32(arriveAt)) === 1;
  }

  /** Takes (at most max of) the messages that have arrived, in push order. */
  takeDue(now, max = ALL) {
    const n = this.x.sx_inbox_take_due(this.h, u32(now), u32(max));
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
  new Int32Array(x.memory.buffer, ptr, 9).set([...cell, ...start, ...goal].map(i32));
  return ptr;
}

/**
 * Round areas (noise, light, danger) that fade after their lifetime.
 *
 *   const a = lib.areas(8);
 *   a.add({kind: 1, pos: [x, y, 0], radius: 600, strength: 5, lifetime: 3000, blocking: false}, now);
 *   a.cost(cell, start, goal);    // extra path cost for a cell
 *   a.blocks(cell, start, goal);  // for a path finder
 *
 * Optional: data, source [x, y, z] (where it came from).
 */
export class Areas extends Table {
  constructor(x, capacity) {
    super(x, 'sx_areas', x.sx_areas_new(capacityOf(capacity)));
  }

  /** Adds an area (since = now), or renews the first of the same kind whose centre is closer than its radius. Returns its index. */
  add(area, now) {
    putRecord(this.x, this.x.sx_areas_record(this.h), areaValues(area));
    return this.x.sx_areas_add(this.h, u32(now));
  }

  /** Removes areas whose age is at least their lifetime. Returns how many. */
  expire(now) {
    return this.x.sx_areas_expire(this.h, u32(now));
  }

  /** Index of the first area containing pos, or null. */
  contains([px, py, pz]) {
    const i = this.x.sx_areas_contains(this.h, i32(px), i32(py), i32(pz));
    return i >= 0 ? i : null;
  }

  /** Like contains, for areas of one kind. */
  containsKind([px, py, pz], kind) {
    const i = this.x.sx_areas_contains_kind(this.h, i32(px), i32(py), i32(pz), u32(kind));
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
    this.x.sx_timers_set(this.h, u32(id), u32(now), u32(duration));
  }

  /** True while the timer of id runs (age below its duration). */
  active(id, now) {
    return this.x.sx_timers_active(this.h, u32(id), u32(now)) === 1;
  }

  /** Time left of the timer of id (0 if none or over). */
  left(id, now) {
    return u32(this.x.sx_timers_left(this.h, u32(id), u32(now)));
  }

  /** Stops the timer of id. False if there was none. */
  remove(id) {
    return this.x.sx_timers_remove(this.h, u32(id)) === 1;
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

/**
 * Tells whether a chase still gets closer: the best value so far (lower is
 * better, e.g. a distance) and when it last improved.
 */
export class Progress {
  constructor() {
    this.reset();
  }

  /** Starts with value at now. */
  start(value, now) {
    this.best = i32(value);
    this.since = u32(now);
    this.started = true;
  }

  /** If value beats the best by at least minGain, it becomes the best (since = now) and true is returned. False if not started. */
  update(value, now, minGain) {
    if (!this.started) return false;
    // A difference of two i32 is exact in a double.
    if (this.best - i32(value) >= i32(minGain)) {
      this.best = i32(value);
      this.since = u32(now);
      return true;
    }
    return false;
  }

  /** True if started and the last improvement is at least limit old. */
  stalled(now, limit) {
    return this.started && u32(u32(now) - this.since) >= u32(limit);
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
    return !this.started || u32(u32(now) - this.last) >= u32(every);
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

const ALERT_LEVELS = 8;

/** An alert config {max, decayPerTick, hold, hysteresis, thresholds} as u32s; at most 8 thresholds. */
function alertConfig(cfg) {
  const thresholds = cfg.thresholds ?? [];
  if (thresholds.length > ALERT_LEVELS) throw new RangeError(`Sagax: an alertness has at most ${ALERT_LEVELS} thresholds`);
  return {
    max: u32(cfg.max),
    decay: u32(cfg.decayPerTick ?? 0),
    hold: u32(cfg.hold ?? 0),
    hysteresis: u32(cfg.hysteresis ?? 0),
    thresholds: thresholds.map(u32),
  };
}

/**
 * A value that rises with stimuli and decays, with levels (unaware,
 * suspicious, searching, alert, ...) that you name yourself.
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

  /** Applies the decay owed up to now, adds amount (up to max) and remembers now as the last stimulus. The level changes in update. */
  stimulus(amount, now, cfg) {
    const c = alertConfig(cfg);
    now = u32(now);
    this.#decay(now, c);
    this.value = Math.min(c.max, Math.min(this.value + u32(amount), ALL));
    this.lastStimulus = now;
  }

  #decay(now, c) {
    const sinceLast = u32(now - this.last);
    const afterHold = Math.max(0, u32(now - this.lastStimulus) - c.hold);
    // The product can pass 2^53, but then it is far above any value anyway.
    const lost = c.decay * Math.min(sinceLast, afterHold);
    this.value = lost >= this.value ? 0 : this.value - lost;
    this.last = now;
  }

  /** Decays up to now and sets the level. Returns [from, to] if it changed, else null. */
  update(now, cfg) {
    const c = alertConfig(cfg);
    now = u32(now);
    this.#decay(now, c);

    const th = c.thresholds;
    const from = this.level;
    const reached = th.filter((t) => this.value >= t).length;
    let to = from;
    if (reached > to) {
      to = reached;
    } else {
      to = Math.min(from, th.length);
      while (to > 0 && to - 1 < th.length && this.value < Math.max(0, th[to - 1] - c.hysteresis)) to--;
    }
    this.level = to;
    return to !== from ? [from, to] : null;
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
function routeOf(x, rule, [ox, oy], candidates, metric, divisor) {
  const [code, arg] = ruleOf(rule);
  const m = choice(METRIC, metric, 'metric');
  const n = candidates.length;
  const size = Math.max(n * 3, 1);
  // The pointer first, the view right after: asking for the buffer may grow
  // the memory, and a grown memory kills older views. Only the number is kept.
  const ptr = x.sx_scratch(size);
  const view = new Int32Array(x.memory.buffer, ptr, size);
  for (let i = 0; i < n; i++) {
    const c = candidates[i];
    view[i * 3] = i32(c.id);
    view[i * 3 + 1] = i32(c.x);
    view[i * 3 + 2] = i32(c.y);
  }
  // The call may grow the memory again: make a new view from the number.
  const count = x.sx_route(code, arg, n, i32(ox), i32(oy), m, i32(divisor));
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
function pointsAroundOf(x, agent, step, [cx, cy, cz], pattern) {
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
    new Int32Array(x.memory.buffer, ptr, 23).set([p.headingStep, p.firstDistance, p.offsets.length, ...offsets, ...distances, ...p.min, ...p.max].map(i32));
    custom = 1;
  } else {
    ptr = x.sx_scratch(3);
  }
  const r = x.sx_points_around(u32(agent), u32(step), i32(cx), i32(cy), i32(cz), custom, sine);
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
    return u32(u32(now) + u32(base) + Math.imul(u32(distance), u32(perUnit)));
  },
  /** Sine of a heading (256 = full turn): 'fine' times 65536, 'coarse' times 256 (17-entry table). */
  sine(heading, mode = 'fine') {
    return this.x.sx_sine(i32(heading), choice(SINE, mode, 'sine'));
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
    if (!offsets) return this.x.sx_sweep_heading(i32(base), u32(elapsed), u32(stepTicks), -1);
    const ptr = this.x.sx_scratch(offsets.length);
    new Int32Array(this.x.memory.buffer, ptr, Math.max(offsets.length, 1)).set(offsets.map(i32));
    return this.x.sx_sweep_heading(i32(base), u32(elapsed), u32(stepTicks), offsets.length);
  },
});
