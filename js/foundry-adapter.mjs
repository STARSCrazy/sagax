// Builds a Sagax grid from a Foundry VTT (v13/v14) scene, e.g. for a
// combat AI that moves tokens.
//
//   * cells are Foundry grid offsets (x = column j, y = row i), so square and
//     hex grids use Foundry's own conversions; cells outside the scene
//     rectangle (padding) are blocked
//   * square: straight and diagonal step 1 (Foundry counts grid spaces);
//     hex: step 1
//   * walls: moves between neighbouring cell centres blocked for movement
//     ('move' collision) become wall bits, so walls between cells work
//   * other tokens block their cells
//   * enemy reach (zone of control): one zone bit per enemy; leaving it costs
//     `leaveCost`, so the walker goes around if it can
//
// Example:
//   const { grid, walkerCell } = buildCombatGrid(lib, { walker, enemies });
//   const area = grid.reachable(walkerCell, movementLeft).cells;
//   const way = grid.pathTo(target.x, target.y, 'turns');

import { HEX } from './sagax.mjs';

const SQUARE_DIRECTIONS = [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [1, -1], [-1, 1], [-1, -1]];
const HEX_LAYOUT_BY_GRID_TYPE = { 2: HEX.oddR, 3: HEX.evenR, 4: HEX.oddQ, 5: HEX.evenQ };

/**
 * @param {object} lib        result of loadSagax()
 * @param {object} options
 * @param {{document: TokenDocument}} options.walker  the token that moves
 * @param {{document: TokenDocument}[]} options.enemies  tokens whose reach costs extra to leave (at most 32)
 * @param {(t) => number} options.reachOf  reach radius in cells
 * @param {number} [options.leaveCost=4]  extra cost for leaving one enemy's reach
 * @returns {{grid, toCell(point), toPoint(cell), cellOf(tokenDocument), walkerCell}}
 *   the grid already carries the rules (square or hex)
 */
export function buildCombatGrid(lib, { walker, enemies = [], reachOf = () => 1, leaveCost = 4 }) {
  const fgrid = canvas.grid;
  const hex = fgrid.isHexagonal ? HEX_LAYOUT_BY_GRID_TYPE[fgrid.type] : null;
  if (!fgrid.isSquare && hex === undefined) throw new Error(`Sagax: grid type ${fgrid.type} not supported`);
  const rules = hex === null ? { straight: 1, diagonal: 1 } : { hex, step: 1 };

  const rect = canvas.dimensions.sceneRect;
  const corner = fgrid.getOffset({ x: rect.x + rect.width - 1, y: rect.y + rect.height - 1 });
  const width = corner.j + 2;
  const height = corner.i + 2;
  const grid = lib.grid(width, height, rules);

  const toCell = (p) => {
    const o = fgrid.getOffset(p);
    return { x: o.j, y: o.i };
  };
  const toPoint = (c) => fgrid.getCenterPoint({ i: c.y, j: c.x });
  const cellOf = (doc) => toCell({
    x: doc.x + (doc.width * fgrid.sizeX) / 2,
    y: doc.y + (doc.height * fgrid.sizeY) / 2,
  });

  // cells outside the scene rectangle are padding
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const p = toPoint({ x, y });
      if (p.x < rect.x || p.y < rect.y || p.x >= rect.x + rect.width || p.y >= rect.y + rect.height) grid.block(x, y);
    }
  }

  // walls between cells; each pair is tested once
  const backend = CONFIG.Canvas.polygonBackends.move;
  const blocked = (a, b) => backend.testCollision(toPoint(a), toPoint(b), { type: 'move', mode: 'any' });
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (hex === null) {
        for (const i of [0, 2, 4, 5]) {
          const [dx, dy] = SQUARE_DIRECTIONS[i];
          if (grid.contains(x + dx, y + dy) && blocked({ x, y }, { x: x + dx, y: y + dy })) grid.addWall(x, y, dx, dy);
        }
      } else {
        const neighbors = grid.hexNeighbors(x, y);
        for (let dir = 0; dir < 3; dir++) {
          const n = neighbors[dir];
          if (grid.contains(n.x, n.y) && blocked({ x, y }, n)) grid.addHexWall(x, y, dir);
        }
      }
    }
  }

  // other tokens block their cells. Uses the scene's token documents:
  // placeables are missing for tokens the viewer does not see.
  for (const doc of canvas.scene.tokens) {
    if (doc === walker.document) continue;
    if (hex === null) {
      const c = toCell({ x: doc.x, y: doc.y });
      for (let dx = 0; dx < (doc.width ?? 1); dx++) {
        for (let dy = 0; dy < (doc.height ?? 1); dy++) grid.block(c.x + dx, c.y + dy);
      }
    } else {
      grid.block(cellOf(doc).x, cellOf(doc).y); // large hex tokens: only the centre cell (simplification)
    }
  }

  // enemy reach as zones: every cell within the reach radius (grid distance).
  // All reaches are measured first with leave cost 0, so one enemy's zone
  // cannot shrink the next one's reach; then the leave cost is switched on.
  grid.setLeaveCost(0);
  const reaches = enemies.slice(0, 32).map((enemy) => {
    const r = reachOf(enemy);
    return r > 0 ? grid.reachable(cellOf(enemy.document), r).cells : [];
  });
  reaches.forEach((cells, zone) => cells.forEach((c) => grid.addZone(c.x, c.y, zone)));
  grid.setLeaveCost(leaveCost);

  return { grid, rules, toCell, toPoint, cellOf, walkerCell: cellOf(walker.document) };
}
