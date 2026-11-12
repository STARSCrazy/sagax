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

/** A table a squad lends out (its own knowledge or areas): it lives and dies with the squad. */
class Lent {
  constructor(handle, owner) {
    this.handle = handle;
    this.owner = owner;
  }
}

/**
 * A handle to a table in the module; every call checks that it is not freed
 * (a freed handle would write to address 0). A lent table (owner set) is
 * freed by its owner.
 */
class Table {
  constructor(x, prefix, handle, owner = null) {
    if (!handle) throw new RangeError('Sagax: capacity must be at least 1');
    this.x = x;
    this.prefix = prefix;
    this.handle = handle;
    this.owner = owner;
  }

  get h() {
    if (!this.handle || (this.owner && !this.owner.handle)) throw new Error('Sagax: used after free()');
    return this.handle;
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
 *   k.learn({subject: 42, kind: 1, pos: [x, y, 0], seenAt: now}, now);  // null, or the evicted fact
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
    if (capacity instanceof Lent) {
      super(x, 'sx_areas', capacity.handle, capacity.owner);
      return;
    }
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

/** Columns of the scoring row, role bits, spot tags and report kinds. */
export const SQUAD = {
  inLosses: 0, // members marked gone
  inOffPlan: 1, // active members whose goal of the active plan broke
  inTargetAge: 2, // ticks since the target was seen
  inTargetDist: 3, // group centre to target
  inRanged: 4, // active members with roleRanged
  inArea: 5, // active members with roleArea
  inPlanAge: 6, // ticks since the active plan became active (its own row only)
  inExtra: 7, // first column of the game's own (setExtraColumns)
  roleRanged: 1,
  roleArea: 2,
  tagCover: 1,
  tagHigh: 2,
  tagHidden: 4,
  tagChoke: 8,
  reportGoalBroken: 1, // sender: the member; data: the plan id of its goal
  reportEnemy: 2, // subject, pos, seenAt, strength, data: a target (told)
  reportDanger: 3, // pos: the danger's centre; the shooter at x = strength, y = data
};

const ERROR_PLAN = -3;
const MEMBER_STATE = { active: 0, suspended: 1, gone: 2 };
const MEMBER_STATES = Object.keys(MEMBER_STATE);
const GOAL_KIND = { move: 0, hold: 1, attack: 2, wait: 3, retreat: 4, custom: 5 };
const GOAL_KINDS = Object.keys(GOAL_KIND);
const START = { now: 0, after: 1, signal: 2 };
const STARTS = Object.keys(START);
const PLACE = { none: 0, offset: 1, side: 2, between: 3, tagged: 4, here: 5 };
const SIDE = { front: 0, left: 1, right: 2, rear: 3 };
const TRIGGER = { none: 0, allArrived: 1, targetInRadius: 2, manual: 3 };
const CHANNEL = { none: 0, shout: 1, radio: 2 };
const SHAPE = { line: 1, wedge: 2, column: 3 };
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

/** An integer from lo to hi, else RangeError. */
function intIn(v, lo, hi, what) {
  if (!Number.isInteger(v) || v < lo || v > hi) throw new RangeError(`Sagax: ${what} must be an integer from ${lo} to ${hi}, not ${String(v)}`);
  return v;
}
const u32Of = (v, what) => intIn(v, 0, ALL, what);
const i32Of = (v, what) => intIn(v, I32_MIN, I32_MAX, what);
const u8Of = (v, what) => intIn(v, 0, 255, what);
const timeOf = (v) => u32Of(v, 'time');
const safeOf = (v, what) => intIn(v, Number.MIN_SAFE_INTEGER, Number.MAX_SAFE_INTEGER, what);

/** [x, y, z] as checked i32s. */
function posOf(p, what = 'pos') {
  if (!Array.isArray(p) || p.length !== 3) throw new RangeError(`Sagax: ${what} must be [x, y, z]`);
  return p.map((c) => i32Of(c, what));
}

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
  return [u32Of(m.id, 'id'), ...posOf(m.pos), u32Of(m.roles ?? 0, 'roles'), u8Of(m.skill ?? 0, 'skill'), choice(MEMBER_STATE, m.state ?? 'active', 'state')];
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
    ...levels(t.pickTop, u32Of, 'pickTop'),
    ...levels(t.thinkTicks, u32Of, 'thinkTicks'),
    ...levels(t.timingSpread, u32Of, 'timingSpread'),
    ...levels(t.holdBonus, u32Of, 'holdBonus'),
    ...levels(t.giveUpTicks, u32Of, 'giveUpTicks'),
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
  if (kind === PLACE.tagged) value = u32Of(p.tags, 'tags');
  return [kind, ...pos, value];
}

function slotValues(s) {
  const kind = choice(GOAL_KIND, s.kind ?? 'move', 'goal kind');
  return [
    u32Of(s.roles ?? 0, 'roles'), u8Of(s.min ?? 1, 'min'), u8Of(s.max ?? 1, 'max'),
    ...placeValues(s.place, false), ...placeValues(s.gather ?? { kind: 'none' }, true),
    kind, kind === GOAL_KIND.custom ? u32Of(s.custom ?? 0, 'custom') : 0,
    ...startValues(s.start), u32Of(s.wantTags ?? 0, 'wantTags'), i32Of(s.dist ?? 0, 'dist'),
  ].map(i32);
}

/** The seed as two u32 halves. */
function seedOf(seed) {
  if (!Number.isSafeInteger(seed) || seed < 0) throw new RangeError('Sagax: seed must be a safe integer, 0 or more');
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
 *   t.setTrigger({ kind: 'allArrived', maxWait: 170 });
 */
export class Tactic extends Table {
  constructor(x, handle) {
    super(x, 'sx_tactic', handle);
  }

  /**
   * Adds a slot and returns its index. A slot: {roles = 0, min = 1, max = 1,
   * place, gather = {kind: 'none'}, kind = 'move', custom = 0,
   * start = {kind: 'now'}, wantTags = 0, dist = 0}. A place: {kind: 'offset',
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

  /**
   * When the signal fires by itself: {kind: 'none'} (default), {kind:
   * 'allArrived', maxWait} (every assigned active member at its gather spot,
   * or maxWait ticks after activation), {kind: 'targetInRadius', radius} or
   * {kind: 'manual'} (only by Squad.signal).
   */
  setTrigger(trigger) {
    const kind = choice(TRIGGER, trigger?.kind, 'trigger');
    let value = 0;
    if (kind === TRIGGER.allArrived) value = u32Of(trigger.maxWait, 'maxWait');
    if (kind === TRIGGER.targetInRadius) value = safeOf(trigger.radius, 'radius');
    this.x.sx_tactic_set_trigger(this.h, kind, value);
  }
}

/**
 * A squad leader with its own memory, planning for a group.
 *
 *   const sq = lib.squad({ groupCap: 8, memoryCap: 16, areaCap: 4, inboxCap: 8, metric: 'chebyshev' });
 *   sq.addMember({ id: 1, pos: [0, 0, 0] }); sq.setLeader(1); sq.setTargetKind(ENEMY);
 *   sq.addTactic(lib.tactics.flank(3, 4)); sq.setSkill(192);
 *   sq.observe({ subject: 9, kind: ENEMY, pos: [20, 0, 0], seenAt: now }, now);
 *   const { status, plan } = sq.check(now, { spots, seed });   // every tick
 *   for (const order of sq.ordersDue(now)) give(order);        // goals, see above
 *
 * Targets come only from the squad's own knowledge (observe, enemy reports).
 */
export class Squad extends Table {
  #knowledge = null;
  #areas = null;

  constructor(x, { groupCap, memoryCap, areaCap, inboxCap, metric = 'euclid' }) {
    const caps = [groupCap, memoryCap, areaCap, inboxCap].map(capacityOf);
    const m = choice(METRIC, metric, 'metric');
    super(x, 'sx_squad', x.sx_squad_new(...caps, m));
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

  /** Puts a suspended member back. False if unknown. */
  resume(id) {
    return this.x.sx_squad_resume(this.h, u32Of(id, 'id')) === 1;
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
   * How orders reach a member: {kind: 'none'}, {kind: 'shout', range} (1 tick
   * per unit, within range) or {kind: 'radio', base = 0, perUnit = 0}
   * (default: radio at once). False if the id is not in the group.
   */
  setChannel(id, channel) {
    const kind = choice(CHANNEL, channel?.kind, 'channel');
    let a = 0;
    let b = 0;
    if (kind === CHANNEL.shout) a = safeOf(channel.range, 'range');
    if (kind === CHANNEL.radio) {
      a = u32Of(channel.base ?? 0, 'base');
      b = u32Of(channel.perUnit ?? 0, 'perUnit');
    }
    return this.#checked(this.x.sx_squad_set_channel(this.h, u32Of(id, 'id'), kind, a, b)) === 1;
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

  /** How many of the game's own columns each scoring row has from SQUAD.inExtra on (default 0). */
  setExtraColumns(count) {
    this.x.sx_squad_set_extra_columns(this.h, u32Of(count, 'count'));
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
  observe(fact, now) {
    const t = timeOf(now);
    const h = this.h;
    putScratch(this.x, factValues(fact));
    return this.x.sx_squad_observe(h, t) === 1 ? factOf(this.#out(FACT_SIZE).map(u32)) : null;
  }

  /** A member's report (a message, kind SQUAD.report*), read from arriveAt on. False if the inbox refused it. */
  report(message, arriveAt) {
    const t = timeOf(arriveAt);
    const h = this.h;
    putScratch(this.x, messageValues(message));
    return this.x.sx_squad_report(h, t) === 1;
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
    const t = tactic.h;
    const r = this.x.sx_squad_add_tactic(this.h, t);
    tactic.handle = 0;
    return this.#checked(r);
  }

  /** null if every consideration reads a column the rows have, else {tactic, input}. */
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
      ...spots.flatMap((s) => [...posOf(s.pos, 'spot'), i32(u32Of(s.tags ?? 0, 'tags'))]),
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

  /** Takes (at most max of) the orders whose arrival time has come, in the order they were sent. */
  ordersDue(now, max = ALL) {
    const n = this.x.sx_squad_orders_due(this.h, timeOf(now), u32Of(max, 'max'));
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

  /** The member reached its first spot (for the 'allArrived' trigger). False if it has no goal in the active plan. */
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
  constructor(x) {
    super(x, 'sx_keeper', x.sx_keeper_new());
  }

  #out(count) {
    return takeInts(this.x, this.x.sx_keeper_out(this.h), count);
  }

  /** Takes a goal (from an order), replacing the old one and the postpone state. */
  set(goal, now) {
    const values = goalValues(goal);
    const t = timeOf(now);
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
  postpone(now) {
    this.x.sx_keeper_postpone(this.h, timeOf(now));
  }

  /** Back on the goal: the postpone clock is reset. */
  resume(now) {
    this.x.sx_keeper_resume(this.h, timeOf(now));
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

/** A shipped tactic as a Tactic. */
function shipped(x, which, param = 0) {
  return new Tactic(x, x.sx_tactic_shipped(which, i32Of(param, 'param')));
}

Object.assign(Sagax.prototype, {
  /** A squad {groupCap, memoryCap, areaCap, inboxCap, metric = 'euclid'}; see Squad. */
  squad(options) {
    return new Squad(this.x, options);
  },
  /** A member's goal keeper; see Keeper. */
  keeper() {
    return new Keeper(this.x);
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
  /** A member's discipline: holdBonus at the level of its own skill (table: the default). */
  discipline(skill, table) {
    const s = u8Of(skill, 'skill');
    if (!table) return this.x.sx_discipline(s, 0);
    putScratch(this.x, skillValues(table));
    const r = this.x.sx_discipline(s, 1);
    if (r < 0) throw new RangeError('Sagax: bad skill table');
    return u32(r);
  },
  /**
   * Picks who joins: gather([x, y], faction, radius, candidates, {metric = 'euclid', freeOnly = false, max}).
   * candidates: [{id, faction, pos, free}]. Within radius (inclusive; Euclid: squared
   * compare), nearest first, ties to the lower id; a negative radius reaches nobody.
   * Only x and y count. Returns the ids.
   */
  gather(origin, faction, radius, candidates, { metric = 'euclid', freeOnly = false, max = candidates.length } = {}) {
    const [ox, oy] = posOf([...origin.slice(0, 2), 0], 'origin');
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
   * goal, else the cost as for Areas.cost.
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

Object.defineProperty(Sagax.prototype, 'tactics', {
  /**
   * The shipped tactics as Tactic objects (numbers are defaults to tune):
   * rush(), formation('line'|'wedge'|'column', spacing), flank(sides 2|3,
   * dist), ambush(), focus(), isolate(), fallBack().
   */
  get() {
    const x = this.x;
    return {
      rush: () => shipped(x, 0),
      formation: (shape, spacing) => shipped(x, choice(SHAPE, shape, 'shape'), spacing),
      flank: (sides, dist) => shipped(x, u8Of(sides, 'sides') >= 3 ? 5 : 4, dist),
      ambush: () => shipped(x, 6),
      focus: () => shipped(x, 7),
      isolate: () => shipped(x, 8),
      fallBack: () => shipped(x, 9),
    };
  },
});
