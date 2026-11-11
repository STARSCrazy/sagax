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
//   {cells, status, reached, cost, total, expanded, nodeLimitHit}
//   status: 'running' (budget used up, call grid.continue()), 'found',
//   'partial' (goal unreachable, way to the closest cell), 'unreachable',
//   'done' (area complete). Ways always begin with the start.

const BLOCKED = 0xffffffff;
const DIRECTIONS = [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [1, -1], [-1, 1], [-1, -1]];
const STATUS = ['idle', 'running', 'found', 'partial', 'unreachable', 'done'];
const OUTPUT = { cells: 0, turns: 1, smooth: 2 };

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
    this.x.sx_grid_set_leave_cost(this.handle, cost >>> 0);
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

  answer(n, stride) {
    if (n < 0) throw new RangeError('Sagax: bad arguments');
    const r = new Int32Array(this.x.memory.buffer, this.x.sx_result(this.handle), 5);
    const status = STATUS[r[0]];
    return {
      cells: this.readCells(n, stride),
      status,
      reached: status === 'found',
      cost: r[2],
      total: r[1],
      expanded: r[3],
      nodeLimitHit: r[4] === 1,
    };
  }

  /**
   * Cheapest way from start to goal. If the goal cannot be reached, the way
   * ends at the reachable cell closest to it (status 'partial').
   * @param {{budget?: number, output?: 'cells'|'turns'|'smooth'}} options  budget 0: no limit
   */
  findPath(start, goal, { budget = 0, output = 'cells' } = {}) {
    this.last = 2;
    return this.answer(this.x.sx_find_path(this.handle, start.x, start.y, goal.x, goal.y, budget, outputOf(output)), 2);
  }

  /** Every cell reachable from start at a cost of at most maxCost: cells [{x, y, cost}]. */
  reachable(start, maxCost, { budget = 0 } = {}) {
    this.last = 3;
    return this.answer(this.x.sx_reachable(this.handle, start.x, start.y, maxCost, budget), 3);
  }

  /** After status 'running': goes on with the last findPath / reachable. */
  continue(budget = 0) {
    return this.answer(this.x.sx_continue(this.handle, budget), this.last ?? 2);
  }

  /** After reachable() or findPath(): the way from that start to x, y, or null. */
  pathTo(x, y, output = 'cells') {
    const a = this.answer(this.x.sx_path_to(this.handle, x, y, outputOf(output)), 2);
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

  free() {
    if (this.handle) this.x.sx_grid_free(this.handle);
    this.handle = 0;
  }
}
