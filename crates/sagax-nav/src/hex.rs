//! Hex grids.
//!
//! Cells are addressed by "offset" coordinates (column `x`, row `y`), as games
//! and Foundry VTT store them. Every second row (or column) is shifted by half
//! a cell; [`HexLayout`] says which. Internally distances use axial
//! coordinates. Each cell has six neighbours, numbered 0..6 so that `i` and
//! `(i + 3) % 6` are opposite. The map is the same [`CellMap`] as on square
//! grids: a move is asked as the offset of the neighbour (each part in
//! -1..=1), and a [`DenseGrid`](crate::grid::DenseGrid) wall bit belongs to
//! that offset, as on square grids.

use crate::grid::{Cell, CellMap};
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

/// A [`CellMap`] seen as a hex [`Graph`]; every step costs `step_cost` plus
/// extras. The map is asked with the offset of each neighbour (neighbour
/// minus cell), in hex direction order 0..6.
pub struct HexGraph<M> {
    /// The game's map.
    pub map: M,
    /// Row or column shift.
    pub layout: HexLayout,
    /// Cost of one step.
    pub step_cost: Cost,
    /// Moves off this cell use [`CellMap::start_step`].
    pub start: Option<Cell>,
}

impl<M: CellMap> HexGraph<M> {
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

impl<M: CellMap> Graph for HexGraph<M> {
    type Node = Cell;

    fn neighbors(&mut self, node: Cell, out: &mut Vec<(Cell, Cost)>) {
        let offsets: [(i32, i32); 6] = std::array::from_fn(|dir| {
            let (x, y) = self.layout.neighbor(node.x, node.y, dir);
            (x - node.x, y - node.y)
        });
        let mut answers = [None; 6];
        self.map
            .steps(node, self.start == Some(node), &offsets, &mut answers);
        for (&(dx, dy), answer) in offsets.iter().zip(answers) {
            let Some(level) = answer else {
                continue;
            };
            let next = Cell::new(node.x + dx, node.y + dy, level);
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
    use crate::grid::DenseGrid;

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
        grid.add_wall(2, 2, nx - 2, ny - 2);
        let (bx, by) = (2 - nx, 2 - ny);
        assert_eq!(grid.step(Cell::new(2, 2, 0), nx - 2, ny - 2), None);
        assert_eq!(grid.step(Cell::new(nx, ny, 0), bx, by), None);
        let (ox, oy) = layout.neighbor(2, 2, 1);
        assert_eq!(grid.step(Cell::new(2, 2, 0), ox - 2, oy - 2), Some(0));
    }

    #[test]
    fn hex_wall_bits_follow_offsets() {
        let layout = HexLayout::OddR;
        let mut grid = DenseGrid::new(5, 5);
        let (nx, ny) = layout.neighbor(2, 2, 0);
        let (dx, dy) = (nx - 2, ny - 2);
        grid.add_wall(2, 2, dx, dy);
        assert_eq!(CellMap::step(&mut grid, Cell::new(2, 2, 0), dx, dy), None);
        assert_eq!(
            CellMap::step(&mut grid, Cell::new(nx, ny, 0), -dx, -dy),
            None
        );
        let (ox, oy) = layout.neighbor(2, 2, 1);
        assert_eq!(
            CellMap::step(&mut grid, Cell::new(2, 2, 0), ox - 2, oy - 2),
            Some(0)
        );
        // the bit is the offset's entry in DenseGrid::DIRECTIONS
        let bit = DenseGrid::DIRECTIONS
            .iter()
            .position(|&d| d == (dx, dy))
            .unwrap();
        assert_eq!(grid.wall_bits(2, 2), 1 << bit);

        // the hex search honours a wall set by offset in every direction,
        // on even and odd rows and columns of every layout
        for layout in ALL {
            for (x, y) in [(2, 2), (3, 3)] {
                for dir in 0..6 {
                    let mut grid = DenseGrid::new(7, 7);
                    let (wx, wy) = layout.neighbor(x, y, dir);
                    grid.add_wall(x, y, wx - x, wy - y);
                    let mut g = HexGraph::new(&mut grid, layout, 1, None);
                    let mut out = Vec::new();
                    g.neighbors(Cell::new(x, y, 0), &mut out);
                    assert_eq!(out.len(), 5, "{layout:?} ({x}, {y}) dir {dir}");
                    assert!(out.iter().all(|&(c, _)| (c.x, c.y) != (wx, wy)));
                    out.clear();
                    g.neighbors(Cell::new(wx, wy, 0), &mut out);
                    assert_eq!(out.len(), 5, "{layout:?} back to ({x}, {y}) dir {dir}");
                    assert!(out.iter().all(|&(c, _)| (c.x, c.y) != (x, y)));
                }
            }
        }
    }
}
