//! Grids with levels.
//!
//! A node is a [`Cell`]: x, y and a level. Levels are whatever small numbers
//! the game chooses (a floor number, or a height). Whether a move is possible,
//! and on which level it arrives, is asked from the game through [`CellMap`],
//! so stairs, ramps and bridges over each other need no special support.

use crate::search::{Cost, Goal, Graph};

pub use sagax_core::Cell;

/// The game's map as the grid search sees it, on square and hex grids alike.
///
/// A move is always asked as the offset `dx`, `dy` (each in -1..=1) from a
/// cell to its neighbour. On hex grids in offset coordinates the six
/// neighbours of a cell are six of these eight offsets (which six depends on
/// the row or column).
pub trait CellMap {
    /// Can one walk from `from` into the neighbour `from.x + dx, from.y + dy`
    /// (`dx`, `dy` in -1..=1)? Returns the level one arrives on there.
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32>;

    /// As [`step`](CellMap::step), but for moves off the start cell, where the
    /// walker may not stand in the cell's middle. Defaults to `step`.
    fn start_step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        self.step(from, dx, dy)
    }

    /// As [`step`](CellMap::step), but as if the cell entered were free
    /// even when it is blocked (occupied, by the one walked up to): walls,
    /// one-way walls, links and the map's edge still apply. Asked only to
    /// find the cells next to a destination nobody can enter.
    ///
    /// The default asks the move back, from the neighbour (on `from`'s
    /// level) toward `from`, and answers `from`'s level when that move
    /// arrives on it: exact where walls and links work both ways; override
    /// it when a wall can forbid entering a cell that may be left that way.
    fn step_unblocked(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let there = Cell::new(from.x + dx, from.y + dy, from.level);
        (self.step(there, -dx, -dy) == Some(from.level)).then_some(from.level)
    }

    /// Extra cost of the move from `from` into `to` on top of the base cost
    /// (danger, rough ground, leaving an enemy's reach); `None` forbids the
    /// move. Defaults to 0.
    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        let _ = (from, to);
        Some(0)
    }

    /// The answers of [`step`](CellMap::step) (or, with `from_start`,
    /// [`start_step`](CellMap::start_step)) for several neighbours at once:
    /// `out[i]` for the offset `offsets[i]`. `offsets.len() == out.len()`,
    /// at most 8. Square grids pass the first 4 or 8 entries of
    /// [`DenseGrid::DIRECTIONS`], hex grids the six neighbour offsets in hex
    /// direction order 0..6. The default asks one offset after the other;
    /// override it when the map answers all neighbours more cheaply together.
    fn steps(
        &mut self,
        from: Cell,
        from_start: bool,
        offsets: &[(i32, i32)],
        out: &mut [Option<i32>],
    ) {
        for (o, &(dx, dy)) in out.iter_mut().zip(offsets) {
            *o = if from_start {
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
///
/// There is no default on purpose: every game states its costs once, with
/// [`GridRules::preset`] or by filling the fields.
///
/// ```compile_fail,E0599
/// use sagax_nav::grid::GridRules;
///
/// let rules = GridRules::default();
/// ```
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

/// Ready-made move costs, for [`GridRules::preset`] and
/// [`Topology::hex`](crate::topology::Topology::hex).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    /// Straight 10, diagonal 14 (close to the real length, 1.4).
    Octile,
    /// Straight 1, diagonal 1: every step counts one square, as on many
    /// tabletop maps.
    Tabletop,
}

impl Preset {
    /// Cost of a straight move (on hex grids: of every move).
    pub const fn straight(self) -> Cost {
        match self {
            Self::Octile => 10,
            Self::Tabletop => 1,
        }
    }

    /// Cost of a diagonal move.
    pub const fn diagonal(self) -> Cost {
        match self {
            Self::Octile => 14,
            Self::Tabletop => 1,
        }
    }
}

impl GridRules {
    /// Eight directions, no cut corners, the costs of `preset`.
    pub const fn preset(preset: Preset) -> Self {
        Self {
            moves: Moves::Eight,
            cut_corners: false,
            straight: preset.straight(),
            diagonal: preset.diagonal(),
        }
    }

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
        self.map.steps(
            node,
            from_start,
            &DIRECTIONS[..count],
            &mut answers[..count],
        );
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

/// A simple grid owned by the library: compact, cache friendly, for games
/// without their own map structure (or that copy theirs once per turn), for
/// tests and benchmarks. It has one or more levels of the same size.
///
/// Per cell (x, y and level) it holds:
/// * an extra cost of entering it, or [`BLOCKED`](Self::BLOCKED);
/// * walls: 8 bits, bit `i` forbids the move by the offset in entry `i` of
///   [`DenseGrid::DIRECTIONS`] out of this cell (walls between cells). This
///   holds on hex grids too: a wall to a hex neighbour is the bit of the
///   neighbour's offset (neighbour minus cell), set with
///   [`add_wall`](Self::add_wall);
/// * zones: 32 bits, one per zone (e.g. an enemy's reach). A move that leaves
///   zones costs [`leave_cost`](Self::set_leave_cost) for each zone left.
///
/// A move keeps its level, unless a [`link`](Self::link) (a stair, a ramp)
/// sends it to another one. The accessors that take `x`, `y` mean level 0;
/// the `_at` ones take a [`Cell`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DenseGrid {
    width: i32,
    height: i32,
    levels: i32,
    cells: Vec<Cost>,
    walls: Vec<u8>,
    zones: Vec<u32>,
    /// `(slot * 8 + direction, level arrived on)`, sorted; usually empty.
    links: Vec<(usize, i32)>,
    leave_cost: Cost,
}

impl DenseGrid {
    /// A cell value that cannot be entered.
    pub const BLOCKED: Cost = Cost::MAX;

    /// The 8 neighbour offsets; bit `i` of a cell's walls belongs to entry `i`
    /// (on square and hex grids).
    pub const DIRECTIONS: [(i32, i32); 8] = DIRECTIONS;

    /// A `width` x `height` grid with one level of open cells with extra cost
    /// 0, no walls and no zones. Negative sizes count as 0.
    pub fn new(width: i32, height: i32) -> Self {
        Self::with_levels(width, height, 1)
    }

    /// As [`new`](Self::new), with `levels` levels (0 to `levels - 1`) of
    /// `width` x `height` cells each.
    ///
    /// # Panics
    /// If the number of cells does not fit `usize`.
    pub fn with_levels(width: i32, height: i32, levels: i32) -> Self {
        let width = width.max(0);
        let height = height.max(0);
        let levels = levels.max(0);
        let n = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(levels as usize))
            .expect("DenseGrid: more cells than memory can address");
        Self {
            width,
            height,
            levels,
            cells: vec![0; n],
            walls: vec![0; n],
            zones: vec![0; n],
            links: Vec::new(),
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

    /// Number of levels.
    pub fn levels(&self) -> i32 {
        self.levels
    }

    /// `true` if `x`, `y` lies on the grid (on level 0).
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.slot(Cell::new(x, y, 0)).is_some()
    }

    fn slot(&self, c: Cell) -> Option<usize> {
        let inside = c.x >= 0
            && c.y >= 0
            && c.level >= 0
            && c.x < self.width
            && c.y < self.height
            && c.level < self.levels;
        inside.then(|| {
            (c.level as usize * self.height as usize + c.y as usize) * self.width as usize
                + c.x as usize
        })
    }

    /// The extra cost of `x`, `y` on level 0, [`BLOCKED`](Self::BLOCKED), or
    /// `None` off the grid.
    pub fn get(&self, x: i32, y: i32) -> Option<Cost> {
        self.get_at(Cell::new(x, y, 0))
    }

    /// Sets the extra cost of `x`, `y` on level 0 ([`BLOCKED`](Self::BLOCKED)
    /// blocks it). Ignored off the grid.
    pub fn set(&mut self, x: i32, y: i32, cost: Cost) {
        self.set_at(Cell::new(x, y, 0), cost);
    }

    /// The extra cost of cell `c`, [`BLOCKED`](Self::BLOCKED), or `None` off
    /// the grid (also on a level it does not have).
    pub fn get_at(&self, c: Cell) -> Option<Cost> {
        self.slot(c).map(|i| self.cells[i])
    }

    /// Sets the extra cost of cell `c` ([`BLOCKED`](Self::BLOCKED) blocks
    /// it). Ignored off the grid.
    pub fn set_at(&mut self, c: Cell, cost: Cost) {
        if let Some(i) = self.slot(c) {
            self.cells[i] = cost;
        }
    }

    /// `true` if `x`, `y` on level 0 is on the grid and not blocked.
    pub fn open(&self, x: i32, y: i32) -> bool {
        self.open_at(Cell::new(x, y, 0))
    }

    fn open_at(&self, c: Cell) -> bool {
        self.get_at(c).is_some_and(|cost| cost != Self::BLOCKED)
    }

    /// The wall bits of `x`, `y` on level 0 (0 off the grid): bit `i` blocks
    /// the offset in entry `i` of [`DIRECTIONS`](Self::DIRECTIONS).
    pub fn wall_bits(&self, x: i32, y: i32) -> u8 {
        self.wall_bits_at(Cell::new(x, y, 0))
    }

    fn wall_bits_at(&self, c: Cell) -> u8 {
        self.slot(c).map_or(0, |i| self.walls[i])
    }

    /// Sets all wall bits of `x`, `y` on level 0 (see
    /// [`wall_bits`](Self::wall_bits)). Ignored off the grid.
    pub fn set_wall_bits(&mut self, x: i32, y: i32, walls: u8) {
        if let Some(i) = self.slot(Cell::new(x, y, 0)) {
            self.walls[i] = walls;
        }
    }

    /// Puts a wall between `x`, `y` on level 0 and its neighbour `dx`, `dy`
    /// away, in both directions; on hex grids `dx`, `dy` is the hex
    /// neighbour's offset. Not a neighbour or off the grid: ignored.
    pub fn add_wall(&mut self, x: i32, y: i32, dx: i32, dy: i32) {
        self.add_wall_at(Cell::new(x, y, 0), dx, dy);
    }

    /// As [`add_wall`](Self::add_wall), on the level of `c`. Returns `false`
    /// (nothing changed) if `dx`, `dy` is not a neighbour offset or `c` is off
    /// the grid; a neighbour off the grid gets no wall bit.
    pub fn add_wall_at(&mut self, c: Cell, dx: i32, dy: i32) -> bool {
        let (Some(there), Some(back)) = (direction_index(dx, dy), direction_index(-dx, -dy)) else {
            return false;
        };
        let Some(i) = self.slot(c) else {
            return false;
        };
        self.walls[i] |= 1 << there;
        if let Some(i) = self.slot(Cell::new(c.x + dx, c.y + dy, c.level)) {
            self.walls[i] |= 1 << back;
        }
        true
    }

    /// Links two neighbouring cells, usually on different levels (a stair, a
    /// ramp): a move from `a` toward `b`'s x, y arrives on `b.level`, and a
    /// move from `b` toward `a`'s x, y on `a.level`. Walls and blocked cells
    /// still apply. Returns `false` (nothing changed) unless `b` is `a`'s
    /// neighbour (`dx`, `dy` each in -1..=1, not both 0) and both are on the
    /// grid.
    pub fn link(&mut self, a: Cell, b: Cell) -> bool {
        let (Some(dx), Some(dy)) = (b.x.checked_sub(a.x), b.y.checked_sub(a.y)) else {
            return false;
        };
        let (Some(there), Some(back)) = (direction_index(dx, dy), direction_index(-dx, -dy)) else {
            return false;
        };
        let (Some(sa), Some(sb)) = (self.slot(a), self.slot(b)) else {
            return false;
        };
        self.put_link(sa * 8 + there, b.level);
        self.put_link(sb * 8 + back, a.level);
        true
    }

    fn put_link(&mut self, key: usize, level: i32) {
        match self.links.binary_search_by_key(&key, |&(k, _)| k) {
            Ok(i) => self.links[i].1 = level,
            Err(i) => self.links.insert(i, (key, level)),
        }
    }

    /// The level a move from `from` by direction `dir` arrives on, if linked.
    fn link_of(&self, from: Cell, dir: usize) -> Option<i32> {
        if self.links.is_empty() {
            return None;
        }
        let key = self.slot(from)? * 8 + dir;
        let i = self.links.binary_search_by_key(&key, |&(k, _)| k).ok()?;
        Some(self.links[i].1)
    }

    /// The zone bits of `x`, `y` on level 0 (0 off the grid).
    pub fn zones(&self, x: i32, y: i32) -> u32 {
        self.zones_at(Cell::new(x, y, 0))
    }

    fn zones_at(&self, c: Cell) -> u32 {
        self.slot(c).map_or(0, |i| self.zones[i])
    }

    /// Sets the zone bits of `x`, `y` on level 0. Ignored off the grid.
    pub fn set_zones(&mut self, x: i32, y: i32, zones: u32) {
        if let Some(i) = self.slot(Cell::new(x, y, 0)) {
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
            .map(|w| (self.zones_at(w[0]) & !self.zones_at(w[1])).count_ones())
            .sum()
    }

    /// Removes all zones.
    pub fn clear_zones(&mut self) {
        self.zones.fill(0);
    }

    /// All extra costs, level by level and row by row
    /// (`(level * height + y) * width + x`), for bulk filling.
    pub fn costs_mut(&mut self) -> &mut [Cost] {
        &mut self.cells
    }

    /// All wall bits (see [`wall_bits`](Self::wall_bits)), in the order of
    /// [`costs_mut`](Self::costs_mut), for bulk filling.
    pub fn wall_bits_mut(&mut self) -> &mut [u8] {
        &mut self.walls
    }

    /// All zone bits, in the order of [`costs_mut`](Self::costs_mut), for
    /// bulk filling.
    pub fn zones_mut(&mut self) -> &mut [u32] {
        &mut self.zones
    }
}

fn direction_index(dx: i32, dy: i32) -> Option<usize> {
    DIRECTIONS.iter().position(|&d| d == (dx, dy))
}

impl CellMap for DenseGrid {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let dir = direction_index(dx, dy);
        if dir.is_some_and(|i| self.wall_bits_at(from) & (1 << i) != 0) {
            return None;
        }
        let level = dir
            .and_then(|i| self.link_of(from, i))
            .unwrap_or(from.level);
        self.open_at(Cell::new(from.x + dx, from.y + dy, level))
            .then_some(level)
    }

    fn step_unblocked(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let dir = direction_index(dx, dy);
        if dir.is_some_and(|i| self.wall_bits_at(from) & (1 << i) != 0) {
            return None;
        }
        let level = dir
            .and_then(|i| self.link_of(from, i))
            .unwrap_or(from.level);
        self.get_at(Cell::new(from.x + dx, from.y + dy, level))
            .map(|_| level)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        let enter = self.get_at(to).filter(|&c| c != Self::BLOCKED)?;
        let left = self.zones_at(from) & !self.zones_at(to);
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

    fn step_unblocked(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        (**self).step_unblocked(from, dx, dy)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        (**self).move_cost(from, to)
    }

    fn steps(
        &mut self,
        from: Cell,
        from_start: bool,
        offsets: &[(i32, i32)],
        out: &mut [Option<i32>],
    ) {
        (**self).steps(from, from_start, offsets, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{Limits, PathResult, find_path};

    #[test]
    fn distance_is_octile() {
        let r = GridRules::preset(Preset::Octile);
        assert_eq!(r.distance(0, 0), 0);
        assert_eq!(r.distance(3, 0), 30);
        assert_eq!(r.distance(3, -3), 42);
        assert_eq!(r.distance(-5, 2), 2 * 14 + 3 * 10);
    }

    #[test]
    fn distance_with_four_moves_is_manhattan() {
        let r = GridRules {
            moves: Moves::Four,
            ..GridRules::preset(Preset::Octile)
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
    fn presets_pin_costs() {
        assert_eq!(
            GridRules::preset(Preset::Octile),
            GridRules {
                moves: Moves::Eight,
                cut_corners: false,
                straight: 10,
                diagonal: 14,
            }
        );
        assert_eq!(
            GridRules::preset(Preset::Tabletop),
            GridRules {
                moves: Moves::Eight,
                cut_corners: false,
                straight: 1,
                diagonal: 1,
            }
        );
        assert_eq!(
            (Preset::Octile.straight(), Preset::Octile.diagonal()),
            (10, 14)
        );
        assert_eq!(
            (Preset::Tabletop.straight(), Preset::Tabletop.diagonal()),
            (1, 1)
        );
    }

    /// 5 x 1 x 2: level 0 blocked at x = 2, level 1 open only at x = 2, a
    /// ramp up from (1, 0, 0) and one down to (3, 0, 0).
    fn bridge() -> DenseGrid {
        let mut g = DenseGrid::with_levels(5, 1, 2);
        g.set_at(Cell::new(2, 0, 0), DenseGrid::BLOCKED);
        for x in [0, 1, 3, 4] {
            g.set_at(Cell::new(x, 0, 1), DenseGrid::BLOCKED);
        }
        g
    }

    fn way(g: &mut DenseGrid) -> PathResult<Cell> {
        let rules = GridRules::preset(Preset::Tabletop);
        find_path(
            &mut GridGraph::new(g, rules, None),
            Cell::new(0, 0, 0),
            &CellGoal::new(4, 0, Some(0), rules),
            Limits::default(),
            usize::MAX,
        )
    }

    #[test]
    fn link_changes_level_both_ways() {
        let mut g = bridge();
        assert_eq!(g.levels(), 2);
        assert!(!way(&mut g).reached(), "no way without the ramps");
        assert!(g.link(Cell::new(1, 0, 0), Cell::new(2, 0, 1)));
        assert!(g.link(Cell::new(2, 0, 1), Cell::new(3, 0, 0)));
        let r = way(&mut g);
        assert!(r.reached());
        assert_eq!(
            r.path.iter().map(|c| c.level).collect::<Vec<_>>(),
            [0, 0, 1, 0, 0]
        );
        assert_eq!(r.cost, 4);
        // and back
        assert_eq!(g.step(Cell::new(3, 0, 0), -1, 0), Some(1));
        assert_eq!(g.step(Cell::new(2, 0, 1), -1, 0), Some(0));
        assert_eq!(g.get_at(Cell::new(2, 0, 1)), Some(0));
        assert_eq!(g.get_at(Cell::new(2, 0, 2)), None);
    }

    #[test]
    fn link_with_extreme_coordinates_returns_false() {
        let mut g = bridge();
        let before = way(&mut g);
        for (a, b) in [
            (Cell::new(i32::MIN, 0, 0), Cell::new(i32::MAX, 0, 1)),
            (Cell::new(i32::MAX, 0, 0), Cell::new(i32::MIN, 0, 1)),
            (Cell::new(0, i32::MIN, 0), Cell::new(0, i32::MAX, 1)),
            (Cell::new(1, 0, 0), Cell::new(i32::MIN, 0, 1)),
        ] {
            assert!(!g.link(a, b));
        }
        assert!(!g.add_wall_at(Cell::new(i32::MAX, 0, 0), 1, 0));
        assert!(!g.add_wall_at(Cell::new(i32::MIN, 0, 0), -1, 0));
        assert_eq!(way(&mut g), before);
    }

    #[test]
    fn link_refuses_non_neighbours() {
        let mut g = bridge();
        let before = way(&mut g);
        assert!(!g.link(Cell::new(0, 0, 0), Cell::new(2, 0, 1)), "two apart");
        assert!(!g.link(Cell::new(1, 0, 0), Cell::new(1, 0, 1)), "same x, y");
        assert!(
            !g.link(Cell::new(4, 0, 0), Cell::new(5, 0, 1)),
            "off the grid"
        );
        assert!(
            !g.link(Cell::new(1, 0, 0), Cell::new(2, 0, 2)),
            "no level 2"
        );
        assert!(
            !g.link(Cell::new(1, 0, -1), Cell::new(2, 0, 0)),
            "no level -1"
        );
        assert_eq!(way(&mut g), before, "the search is unchanged");
    }

    #[test]
    fn bulk_views_are_level_major() {
        let mut g = DenseGrid::with_levels(3, 2, 2);
        assert_eq!(g.costs_mut().len(), 12);
        g.costs_mut()[(2 + 1) * 3 + 1] = 7; // level 1, y 1, x 1
        assert_eq!(g.get_at(Cell::new(1, 1, 1)), Some(7));
        assert_eq!(g.get(1, 1), Some(0), "x, y accessors mean level 0");
        assert_eq!(DenseGrid::new(3, 2).levels(), 1);
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
