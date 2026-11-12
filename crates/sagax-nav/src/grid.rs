//! Grids with levels.
//!
//! A node is a [`Cell`]: x, y and a level. Levels are whatever small numbers
//! the game chooses (a floor number, or a height). Whether a move is possible,
//! and on which level it arrives, is asked from the game through [`CellMap`],
//! so stairs, ramps and bridges over each other need no special support.

use crate::search::{Cost, Goal, Graph};

/// A grid cell on a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Cell {
    /// Column.
    pub x: i32,
    /// Row.
    pub y: i32,
    /// Level (floor, height layer, …), chosen by the game.
    pub level: i32,
}

impl Cell {
    /// A cell.
    pub const fn new(x: i32, y: i32, level: i32) -> Self {
        Self { x, y, level }
    }
}

/// The game's map as the grid search sees it.
pub trait CellMap {
    /// Can one walk from `from` into the neighbour `from.x + dx, from.y + dy`
    /// (`dx`, `dy` in -1..=1)? Returns the level one arrives on there.
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32>;

    /// As [`step`](CellMap::step), but for moves off the start cell, where the
    /// walker may not stand in the cell's middle. Defaults to `step`.
    fn start_step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        self.step(from, dx, dy)
    }

    /// Extra cost of the move from `from` into `to` on top of the base cost
    /// (danger, rough ground, leaving an enemy's reach); `None` forbids the
    /// move. Defaults to 0.
    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        let _ = (from, to);
        Some(0)
    }

    /// The answers of [`step`](CellMap::step) (or, with `from_start`,
    /// [`start_step`](CellMap::start_step)) for the first `count` (4 or 8)
    /// directions of [`DenseGrid::DIRECTIONS`] at once: `out[i]` for
    /// direction `i`. The default asks one direction after the other;
    /// override it when the map answers all neighbours more cheaply together.
    fn steps(&mut self, from: Cell, from_start: bool, count: usize, out: &mut [Option<i32>; 8]) {
        for (i, &(dx, dy)) in DIRECTIONS[..count].iter().enumerate() {
            out[i] = if from_start {
                self.start_step(from, dx, dy)
            } else {
                self.step(from, dx, dy)
            };
        }
    }
}

/// Which moves a walker can make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moves {
    /// Up, down, left, right.
    Four,
    /// Also diagonally.
    Eight,
}

/// Movement rules and base costs of a grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridRules {
    /// Four or eight directions.
    pub moves: Moves,
    /// Allow a diagonal move past a blocked side neighbour.
    pub cut_corners: bool,
    /// Cost of a straight move.
    pub straight: Cost,
    /// Cost of a diagonal move.
    pub diagonal: Cost,
}

impl Default for GridRules {
    /// Eight directions, no cut corners, costs 10 and 14.
    fn default() -> Self {
        Self {
            moves: Moves::Eight,
            cut_corners: false,
            straight: 10,
            diagonal: 14,
        }
    }
}

impl GridRules {
    /// The least cost of getting from one cell to another `dx`, `dy` away on
    /// open ground. Never more than the real cost, so A* stays exact.
    pub fn distance(&self, dx: i32, dy: i32) -> Cost {
        let dx = u64::from(dx.unsigned_abs());
        let dy = u64::from(dy.unsigned_abs());
        let s = u64::from(self.straight);
        let d = u64::from(self.diagonal);
        let total = match self.moves {
            Moves::Four => s * (dx + dy),
            Moves::Eight => {
                let (long, short) = if dx > dy { (dx, dy) } else { (dy, dx) };
                // two diagonals can replace a straight move, a straight pair a diagonal
                short * d.min(2 * s) + (long - short) * s.min(d)
            }
        };
        Cost::try_from(total).unwrap_or(Cost::MAX)
    }
}

/// Order of the neighbours: straight first, then diagonal.
const DIRECTIONS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// A [`CellMap`] seen as a [`Graph`].
pub struct GridGraph<M> {
    /// The game's map.
    pub map: M,
    /// Movement rules.
    pub rules: GridRules,
    /// Moves off this cell use [`CellMap::start_step`].
    pub start: Option<Cell>,
}

impl<M: CellMap> GridGraph<M> {
    /// A grid graph; `start` selects the cell whose moves use `start_step`.
    pub fn new(map: M, rules: GridRules, start: Option<Cell>) -> Self {
        Self { map, rules, start }
    }
}

impl<M: CellMap> Graph for GridGraph<M> {
    type Node = Cell;

    fn neighbors(&mut self, node: Cell, out: &mut Vec<(Cell, Cost)>) {
        let from_start = self.start == Some(node);
        let count = match self.rules.moves {
            Moves::Four => 4,
            Moves::Eight => 8,
        };
        // every direction is asked once; diagonals reuse the straight
        // answers for the corner check (8 map questions per cell, not 16)
        let mut answers = [None; 8];
        self.map.steps(node, from_start, count, &mut answers);
        for (i, &(dx, dy)) in DIRECTIONS[..count].iter().enumerate() {
            let Some(level) = answers[i] else {
                continue;
            };
            let diagonal = dx != 0 && dy != 0;
            if diagonal && !self.rules.cut_corners {
                // DIRECTIONS: 0 +x, 1 -x, 2 +y, 3 -y
                let side_x = answers[if dx > 0 { 0 } else { 1 }];
                let side_y = answers[if dy > 0 { 2 } else { 3 }];
                if side_x.is_none() || side_y.is_none() {
                    continue; // would cut the corner of a wall
                }
            }
            let next = Cell::new(node.x + dx, node.y + dy, level);
            let Some(extra) = self.map.move_cost(node, next) else {
                continue;
            };
            let base = if diagonal {
                self.rules.diagonal
            } else {
                self.rules.straight
            };
            if let Some(cost) = base.checked_add(extra) {
                out.push((next, cost));
            }
        }
    }
}

/// Go to cell `x`, `y`; on `level`, or on any level if `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellGoal {
    /// Column.
    pub x: i32,
    /// Row.
    pub y: i32,
    /// Required level, or any.
    pub level: Option<i32>,
    /// The rules the estimate is computed for.
    pub rules: GridRules,
}

impl CellGoal {
    /// A goal cell.
    pub fn new(x: i32, y: i32, level: Option<i32>, rules: GridRules) -> Self {
        Self { x, y, level, rules }
    }
}

impl Goal<Cell> for CellGoal {
    fn is_goal(&self, node: Cell) -> bool {
        node.x == self.x && node.y == self.y && self.level.is_none_or(|l| l == node.level)
    }

    /// Grid distance, plus 1 on another level than a required one (reaching
    /// it takes at least one more move, which costs at least 1 unless the
    /// rules make moves free).
    fn estimate(&self, node: Cell) -> Cost {
        let distance = self.rules.distance(self.x - node.x, self.y - node.y);
        let off_level = self.level.is_some_and(|l| l != node.level);
        let min_move = self.rules.straight.min(self.rules.diagonal);
        distance.saturating_add(Cost::from(off_level && min_move > 0))
    }
}

/// Cells of `path` where the walker turns: every cell after the start where
/// the direction or the level changes, and the last cell. With
/// `first_always` the first cell after the start is a turning point too.
pub fn turning_points(path: &[Cell], first_always: bool, out: &mut Vec<Cell>) {
    out.clear();
    let n = path.len();
    for k in 1..n {
        let cell = path[k];
        let keep = k + 1 == n || (first_always && k == 1) || {
            let prev = path[k - 1];
            let next = path[k + 1];
            next.x - cell.x != cell.x - prev.x
                || next.y - cell.y != cell.y - prev.y
                || cell.level != prev.level
                || next.level != cell.level
        };
        if keep {
            out.push(cell);
        }
    }
}

/// A simple one-level grid owned by the library: compact, cache friendly,
/// for games without their own map structure (or that copy theirs once per
/// turn), for tests and benchmarks.
///
/// Per cell it holds:
/// * an extra cost of entering it, or [`BLOCKED`](Self::BLOCKED);
/// * walls: 8 bits, bit `i` forbids the move in direction `i` of
///   [`DenseGrid::DIRECTIONS`] out of this cell (walls between cells);
/// * zones: 32 bits, one per zone (e.g. an enemy's reach). A move that leaves
///   zones costs [`leave_cost`](Self::set_leave_cost) for each zone left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenseGrid {
    width: i32,
    height: i32,
    cells: Vec<Cost>,
    walls: Vec<u8>,
    zones: Vec<u32>,
    leave_cost: Cost,
}

impl DenseGrid {
    /// A cell value that cannot be entered.
    pub const BLOCKED: Cost = Cost::MAX;

    /// The 8 move directions; bit `i` of a cell's walls belongs to entry `i`.
    pub const DIRECTIONS: [(i32, i32); 8] = DIRECTIONS;

    /// A `width` x `height` grid of open cells with extra cost 0, no walls and
    /// no zones. Negative sizes count as 0.
    pub fn new(width: i32, height: i32) -> Self {
        let width = width.max(0);
        let height = height.max(0);
        let n = width as usize * height as usize;
        Self {
            width,
            height,
            cells: vec![0; n],
            walls: vec![0; n],
            zones: vec![0; n],
            leave_cost: 0,
        }
    }

    /// Width in cells.
    pub fn width(&self) -> i32 {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> i32 {
        self.height
    }

    /// `true` if `x`, `y` lies on the grid.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }

    fn slot(&self, x: i32, y: i32) -> Option<usize> {
        self.contains(x, y)
            .then(|| y as usize * self.width as usize + x as usize)
    }

    /// The extra cost of `x`, `y`, [`BLOCKED`](Self::BLOCKED), or `None` off the grid.
    pub fn get(&self, x: i32, y: i32) -> Option<Cost> {
        self.slot(x, y).map(|i| self.cells[i])
    }

    /// Sets the extra cost of `x`, `y` ([`BLOCKED`](Self::BLOCKED) blocks it).
    /// Ignored off the grid.
    pub fn set(&mut self, x: i32, y: i32, cost: Cost) {
        if let Some(i) = self.slot(x, y) {
            self.cells[i] = cost;
        }
    }

    /// `true` if `x`, `y` is on the grid and not blocked.
    pub fn open(&self, x: i32, y: i32) -> bool {
        self.get(x, y).is_some_and(|c| c != Self::BLOCKED)
    }

    /// The wall bits of `x`, `y` (0 off the grid).
    pub fn walls(&self, x: i32, y: i32) -> u8 {
        self.slot(x, y).map_or(0, |i| self.walls[i])
    }

    /// Sets all wall bits of `x`, `y`. Ignored off the grid.
    pub fn set_walls(&mut self, x: i32, y: i32, walls: u8) {
        if let Some(i) = self.slot(x, y) {
            self.walls[i] = walls;
        }
    }

    /// Puts a wall between `x`, `y` and its neighbour `dx`, `dy` away, in both
    /// directions. Not a neighbour or off the grid: ignored.
    pub fn add_wall(&mut self, x: i32, y: i32, dx: i32, dy: i32) {
        let (Some(there), Some(back)) = (direction_index(dx, dy), direction_index(-dx, -dy)) else {
            return;
        };
        if let Some(i) = self.slot(x, y) {
            self.walls[i] |= 1 << there;
        }
        if let Some(i) = self.slot(x + dx, y + dy) {
            self.walls[i] |= 1 << back;
        }
    }

    /// The zone bits of `x`, `y` (0 off the grid).
    pub fn zones(&self, x: i32, y: i32) -> u32 {
        self.slot(x, y).map_or(0, |i| self.zones[i])
    }

    /// Sets the zone bits of `x`, `y`. Ignored off the grid.
    pub fn set_zones(&mut self, x: i32, y: i32, zones: u32) {
        if let Some(i) = self.slot(x, y) {
            self.zones[i] = zones;
        }
    }

    /// Cost added for each zone a move leaves.
    pub fn set_leave_cost(&mut self, cost: Cost) {
        self.leave_cost = cost;
    }

    /// Zones `path` leaves, cell by cell (each one e.g. an attack of
    /// opportunity). The way's cost minus this times the leave cost is what
    /// walking and terrain cost alone.
    pub fn zones_left(&self, path: &[Cell]) -> u32 {
        path.windows(2)
            .map(|w| (self.zones(w[0].x, w[0].y) & !self.zones(w[1].x, w[1].y)).count_ones())
            .sum()
    }

    /// Removes all zones.
    pub fn clear_zones(&mut self) {
        self.zones.fill(0);
    }

    /// All extra costs, row by row (`y * width + x`), for bulk filling.
    pub fn costs_mut(&mut self) -> &mut [Cost] {
        &mut self.cells
    }

    /// All wall bits, row by row, for bulk filling.
    pub fn walls_mut(&mut self) -> &mut [u8] {
        &mut self.walls
    }

    /// All zone bits, row by row, for bulk filling.
    pub fn zones_mut(&mut self) -> &mut [u32] {
        &mut self.zones
    }
}

fn direction_index(dx: i32, dy: i32) -> Option<usize> {
    DIRECTIONS.iter().position(|&d| d == (dx, dy))
}

impl CellMap for DenseGrid {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let blocked_by_wall =
            direction_index(dx, dy).is_some_and(|i| self.walls(from.x, from.y) & (1 << i) != 0);
        (!blocked_by_wall && self.open(from.x + dx, from.y + dy)).then_some(from.level)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        let enter = self.get(to.x, to.y).filter(|&c| c != Self::BLOCKED)?;
        let left = self.zones(from.x, from.y) & !self.zones(to.x, to.y);
        let leave = self.leave_cost.saturating_mul(left.count_ones());
        Some(enter.saturating_add(leave))
    }
}

impl<M: CellMap + ?Sized> CellMap for &mut M {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        (**self).step(from, dx, dy)
    }

    fn start_step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        (**self).start_step(from, dx, dy)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        (**self).move_cost(from, to)
    }

    fn steps(&mut self, from: Cell, from_start: bool, count: usize, out: &mut [Option<i32>; 8]) {
        (**self).steps(from, from_start, count, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_is_octile() {
        let r = GridRules::default();
        assert_eq!(r.distance(0, 0), 0);
        assert_eq!(r.distance(3, 0), 30);
        assert_eq!(r.distance(3, -3), 42);
        assert_eq!(r.distance(-5, 2), 2 * 14 + 3 * 10);
    }

    #[test]
    fn distance_with_four_moves_is_manhattan() {
        let r = GridRules {
            moves: Moves::Four,
            ..GridRules::default()
        };
        assert_eq!(r.distance(3, -2), 50);
    }

    #[test]
    fn turning_points_keep_turns_and_end() {
        let p = |x, y| Cell::new(x, y, 0);
        let path = [p(0, 0), p(1, 0), p(2, 0), p(3, 1), p(4, 2), p(4, 3)];
        let mut out = Vec::new();
        turning_points(&path, false, &mut out);
        assert_eq!(out, [p(2, 0), p(4, 2), p(4, 3)]);
        turning_points(&path, true, &mut out);
        assert_eq!(out, [p(1, 0), p(2, 0), p(4, 2), p(4, 3)]);
    }

    #[test]
    fn walls_block_both_ways() {
        let mut g = DenseGrid::new(3, 1);
        g.add_wall(0, 0, 1, 0);
        assert_eq!(g.step(Cell::new(0, 0, 0), 1, 0), None);
        assert_eq!(g.step(Cell::new(1, 0, 0), -1, 0), None);
        assert_eq!(g.step(Cell::new(1, 0, 0), 1, 0), Some(0));
    }

    #[test]
    fn leaving_zones_costs() {
        let mut g = DenseGrid::new(3, 1);
        g.set_leave_cost(50);
        g.set_zones(0, 0, 0b11);
        g.set_zones(1, 0, 0b01);
        let c = |x| Cell::new(x, 0, 0);
        assert_eq!(g.move_cost(c(0), c(1)), Some(50)); // leaves zone 2
        assert_eq!(g.move_cost(c(1), c(2)), Some(50)); // leaves zone 1
        assert_eq!(g.zones_left(&[c(0), c(1), c(2)]), 2);
        assert_eq!(g.zones_left(&[c(0)]), 0);
        assert_eq!(g.move_cost(c(1), c(0)), Some(0)); // enters only
    }

    #[test]
    fn turning_points_mark_level_changes() {
        let path = [
            Cell::new(0, 0, 0),
            Cell::new(1, 0, 0),
            Cell::new(2, 0, 1),
            Cell::new(3, 0, 1),
        ];
        let mut out = Vec::new();
        turning_points(&path, false, &mut out);
        assert_eq!(
            out,
            [Cell::new(1, 0, 0), Cell::new(2, 0, 1), Cell::new(3, 0, 1)]
        );
    }
}
