//! Hex grids.
//!
//! Cells are addressed by "offset" coordinates (column `x`, row `y`), as games
//! and Foundry VTT store them. Every second row (or column) is shifted by half
//! a cell; [`HexLayout`] says which. Internally distances use axial
//! coordinates. Each cell has six neighbours, numbered 0..6 so that `i` and
//! `(i + 3) % 6` are opposite; a wall bit `i` blocks direction `i`.

use crate::grid::{Cell, DenseGrid};
use crate::search::{Cost, Goal, Graph};

pub use sagax_core::{HexLayout, axial_step};

/// Cells of a hex `path` where the walker turns: every cell after the start
/// where the hex direction or the level changes, and the last cell (as
/// [`turning_points`](crate::grid::turning_points) on square grids). With
/// `first_always` the first cell after the start is a turning point too.
pub fn hex_turning_points(
    layout: HexLayout,
    path: &[Cell],
    first_always: bool,
    out: &mut Vec<Cell>,
) {
    out.clear();
    let n = path.len();
    let axial = |c: Cell| layout.to_axial(c.x, c.y);
    for k in 1..n {
        let cell = path[k];
        let keep = k + 1 == n || (first_always && k == 1) || {
            let (p, c, nx) = (axial(path[k - 1]), axial(cell), axial(path[k + 1]));
            (nx.0 - c.0, nx.1 - c.1) != (c.0 - p.0, c.1 - p.1)
                || cell.level != path[k - 1].level
                || path[k + 1].level != cell.level
        };
        if keep {
            out.push(cell);
        }
    }
}

/// The game's hex map as the search sees it.
pub trait HexMap {
    /// Can one walk from `from` in direction `dir` into the cell `to`
    /// (its neighbour)? Returns the level one arrives on there.
    fn step(&mut self, from: Cell, dir: usize, to: (i32, i32)) -> Option<i32>;

    /// As [`step`](HexMap::step), for moves off the start cell (where the
    /// walker may not stand in the cell's middle). Defaults to `step`.
    fn start_step(&mut self, from: Cell, dir: usize, to: (i32, i32)) -> Option<i32> {
        self.step(from, dir, to)
    }

    /// Extra cost of the move on top of the step cost; `None` forbids it.
    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        let _ = (from, to);
        Some(0)
    }

    /// The answers for all six directions at once (`to[i]` is the neighbour
    /// in direction `i`): `out[i]` as [`step`](HexMap::step) or, with
    /// `from_start`, [`start_step`](HexMap::start_step) would answer. The
    /// default asks one direction after the other.
    fn steps(
        &mut self,
        from: Cell,
        from_start: bool,
        to: &[(i32, i32); 6],
        out: &mut [Option<i32>; 6],
    ) {
        for dir in 0..6 {
            out[dir] = if from_start {
                self.start_step(from, dir, to[dir])
            } else {
                self.step(from, dir, to[dir])
            };
        }
    }
}

/// [`DenseGrid`] as a hex map: wall bit `i` blocks hex direction `i`; costs
/// and zones work as on square grids.
impl HexMap for DenseGrid {
    fn step(&mut self, from: Cell, dir: usize, to: (i32, i32)) -> Option<i32> {
        let wall = self.walls(from.x, from.y) & (1 << dir) != 0;
        (!wall && self.open(to.0, to.1)).then_some(from.level)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        crate::grid::CellMap::move_cost(self, from, to)
    }
}

impl<M: HexMap + ?Sized> HexMap for &mut M {
    fn step(&mut self, from: Cell, dir: usize, to: (i32, i32)) -> Option<i32> {
        HexMap::step(&mut **self, from, dir, to)
    }

    fn start_step(&mut self, from: Cell, dir: usize, to: (i32, i32)) -> Option<i32> {
        HexMap::start_step(&mut **self, from, dir, to)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        HexMap::move_cost(&mut **self, from, to)
    }

    fn steps(
        &mut self,
        from: Cell,
        from_start: bool,
        to: &[(i32, i32); 6],
        out: &mut [Option<i32>; 6],
    ) {
        HexMap::steps(&mut **self, from, from_start, to, out);
    }
}

/// A [`HexMap`] seen as a [`Graph`]; every step costs `step_cost` plus extras.
pub struct HexGraph<M> {
    /// The game's map.
    pub map: M,
    /// Row or column shift.
    pub layout: HexLayout,
    /// Cost of one step.
    pub step_cost: Cost,
    /// Moves off this cell use [`HexMap::start_step`].
    pub start: Option<Cell>,
}

impl<M: HexMap> HexGraph<M> {
    /// A hex graph; `start` selects the cell whose moves use `start_step`.
    pub fn new(map: M, layout: HexLayout, step_cost: Cost, start: Option<Cell>) -> Self {
        Self {
            map,
            layout,
            step_cost,
            start,
        }
    }
}

impl<M: HexMap> Graph for HexGraph<M> {
    type Node = Cell;

    fn neighbors(&mut self, node: Cell, out: &mut Vec<(Cell, Cost)>) {
        let to: [(i32, i32); 6] =
            std::array::from_fn(|dir| self.layout.neighbor(node.x, node.y, dir));
        let mut answers = [None; 6];
        self.map
            .steps(node, self.start == Some(node), &to, &mut answers);
        for dir in 0..6 {
            let to = to[dir];
            let Some(level) = answers[dir] else {
                continue;
            };
            let next = Cell::new(to.0, to.1, level);
            let Some(extra) = self.map.move_cost(node, next) else {
                continue;
            };
            if let Some(cost) = self.step_cost.checked_add(extra) {
                out.push((next, cost));
            }
        }
    }
}

/// Go to hex cell `x`, `y` (any level).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HexGoal {
    /// Column.
    pub x: i32,
    /// Row.
    pub y: i32,
    /// Row or column shift.
    pub layout: HexLayout,
    /// Cost of one step (the estimate is steps times this).
    pub step_cost: Cost,
}

impl Goal<Cell> for HexGoal {
    fn is_goal(&self, node: Cell) -> bool {
        node.x == self.x && node.y == self.y
    }

    fn estimate(&self, node: Cell) -> Cost {
        let steps = self.layout.distance(node.x, node.y, self.x, self.y);
        Cost::try_from(steps)
            .unwrap_or(Cost::MAX)
            .saturating_mul(self.step_cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [HexLayout; 4] = [
        HexLayout::OddR,
        HexLayout::EvenR,
        HexLayout::OddQ,
        HexLayout::EvenQ,
    ];

    #[test]
    fn search_on_hex_matches_distance_and_goes_around_walls() {
        use crate::search::{Limits, find_path};
        for layout in ALL {
            let mut grid = DenseGrid::new(15, 15);
            let goal = HexGoal {
                x: 12,
                y: 9,
                layout,
                step_cost: 1,
            };
            let mut g = HexGraph::new(&mut grid, layout, 1, None);
            let r = find_path(
                &mut g,
                Cell::new(2, 3, 0),
                &goal,
                Limits::default(),
                usize::MAX,
            );
            assert!(r.reached());
            assert_eq!(r.cost as i32, layout.distance(2, 3, 12, 9));
            for w in r.path.windows(2) {
                assert_eq!(layout.distance(w[0].x, w[0].y, w[1].x, w[1].y), 1);
            }

            // a wall of blocked cells across column 7 with a gap at row 14
            for y in 0..14 {
                grid.set(7, y, DenseGrid::BLOCKED);
            }
            let mut g = HexGraph::new(&mut grid, layout, 1, None);
            let r = find_path(
                &mut g,
                Cell::new(2, 3, 0),
                &goal,
                Limits::default(),
                usize::MAX,
            );
            assert!(r.reached());
            assert!(r.path.iter().all(|c| !(c.x == 7 && c.y < 14)));
        }
    }

    #[test]
    fn hex_wall_bits_block_one_direction_pair() {
        let layout = HexLayout::OddR;
        let mut grid = DenseGrid::new(5, 5);
        let (nx, ny) = layout.neighbor(2, 2, 0);
        grid.set_walls(2, 2, 1 << 0);
        let back = layout.direction(nx, ny, 2, 2).unwrap();
        grid.set_walls(nx, ny, 1 << back);
        assert_eq!(
            HexMap::step(&mut grid, Cell::new(2, 2, 0), 0, (nx, ny)),
            None
        );
        assert_eq!(
            HexMap::step(&mut grid, Cell::new(nx, ny, 0), back, (2, 2)),
            None
        );
        assert_eq!(
            HexMap::step(&mut grid, Cell::new(2, 2, 0), 1, layout.neighbor(2, 2, 1)),
            Some(0)
        );
    }
}
