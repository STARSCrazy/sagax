//! Square or hex: one switch for code that serves both grid kinds.
//!
//! The interfaces (C, WebAssembly) get the grid kind at run time. With
//! [`Topology`] and [`AnyGraph`] they call every building block the same way
//! and the right square or hex version runs: search, turning points,
//! smoothing, formation, D* Lite estimates.

use crate::dstar::{DStarLite, Heuristic, HexHeuristic};
use crate::formation::{Formation, place_members, place_members_hex};
use crate::grid::{Cell, CellMap, GridGraph, GridRules, turning_points};
use crate::hex::{HexGraph, HexLayout, HexMap, hex_turning_points};
use crate::search::{Cost, Graph};
use crate::smooth::{hex_line_clear_within, line_clear_within, smooth_path};

/// The kind of grid and its move costs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topology {
    /// Square cells, 4 or 8 directions.
    Square(GridRules),
    /// Hex cells, 6 directions, every step `step_cost`.
    Hex {
        /// Row or column shift.
        layout: HexLayout,
        /// Cost of one step.
        step_cost: Cost,
    },
}

impl Topology {
    /// The turning points of `path` (see [`turning_points`] and
    /// [`hex_turning_points`]).
    pub fn turning_points(&self, path: &[Cell], first_always: bool, out: &mut Vec<Cell>) {
        match *self {
            Self::Square(_) => turning_points(path, first_always, out),
            Self::Hex { layout, .. } => hex_turning_points(layout, path, first_always, out),
        }
    }

    /// `true` if one can walk straight from `a` to `b` through cells whose
    /// extra move cost is at most `max_extra` (see [`line_clear_within`] and
    /// [`hex_line_clear_within`]).
    pub fn line_clear<M: CellMap + HexMap>(
        &self,
        map: &mut M,
        a: Cell,
        b: Cell,
        max_extra: Cost,
    ) -> bool {
        match *self {
            Self::Square(_) => line_clear_within(map, a, b, max_extra),
            Self::Hex { layout, .. } => hex_line_clear_within(map, layout, a, b, max_extra),
        }
    }

    /// Smooths `path` into `out` (see [`smooth_path`]); lines cross cells
    /// with an extra cost of at most `max_extra`.
    pub fn smooth<M: CellMap + HexMap>(
        &self,
        map: &mut M,
        path: &[Cell],
        max_extra: Cost,
        out: &mut Vec<Cell>,
    ) {
        smooth_path(path, out, |a, b| self.line_clear(map, a, b, max_extra));
    }

    /// Tells `planner` that `cells` changed: every node it knows on or next
    /// to one of them, on any level, is checked again. Returns how many.
    pub fn cells_changed<G: Graph<Node = Cell>>(
        &self,
        planner: &mut DStarLite<Cell, Topology>,
        graph: &mut G,
        cells: &[Cell],
        scratch: &mut Vec<Cell>,
    ) -> usize {
        // the columns round the changed cells, once; then each known node once
        let mut around: Vec<(i32, i32)> = Vec::with_capacity(cells.len() * 9);
        for c in cells {
            match *self {
                Self::Square(_) => {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            around.push((c.x + dx, c.y + dy));
                        }
                    }
                }
                Self::Hex { layout, .. } => {
                    around.push((c.x, c.y));
                    around.extend((0..6).map(|d| layout.neighbor(c.x, c.y, d)));
                }
            }
        }
        around.sort_unstable();
        around.dedup();
        scratch.clear();
        scratch.extend(
            planner
                .known_nodes()
                .filter(|n| around.binary_search(&(n.x, n.y)).is_ok()),
        );
        planner.nodes_changed(graph, scratch.iter().copied());
        scratch.len()
    }

    /// Places a formation along the leader's path (see [`place_members`] and
    /// [`place_members_hex`]).
    pub fn place_members<M: CellMap + HexMap>(
        &self,
        map: &mut M,
        leader_path: &[Cell],
        formation: &Formation,
        out: &mut Vec<Vec<Cell>>,
    ) {
        match *self {
            Self::Square(_) => place_members(map, leader_path, formation, out),
            Self::Hex { layout, .. } => place_members_hex(map, layout, leader_path, formation, out),
        }
    }

    /// A graph of `map` for this topology. `start` is the cell whose moves
    /// use the map's `start_step`.
    pub fn graph<M: CellMap + HexMap>(&self, map: M, start: Option<Cell>) -> AnyGraph<M> {
        match *self {
            Self::Square(rules) => AnyGraph::Square(GridGraph::new(map, rules, start)),
            Self::Hex { layout, step_cost } => {
                AnyGraph::Hex(HexGraph::new(map, layout, step_cost, start))
            }
        }
    }

    /// Estimated cost between two cells ([`Heuristic`] for D* Lite and goals).
    pub fn estimate(&self, from: Cell, to: Cell) -> Cost {
        match *self {
            Self::Square(rules) => rules.estimate(from, to),
            Self::Hex { layout, step_cost } => {
                HexHeuristic { layout, step_cost }.estimate(from, to)
            }
        }
    }
}

impl Heuristic<Cell> for Topology {
    fn estimate(&self, from: Cell, to: Cell) -> Cost {
        Topology::estimate(self, from, to)
    }
}

/// A square or hex graph over the same map type.
pub enum AnyGraph<M> {
    /// Square grid.
    Square(GridGraph<M>),
    /// Hex grid.
    Hex(HexGraph<M>),
}

impl<M> AnyGraph<M> {
    /// The map inside.
    pub fn map_mut(&mut self) -> &mut M {
        match self {
            Self::Square(g) => &mut g.map,
            Self::Hex(g) => &mut g.map,
        }
    }

    /// The map, given back.
    pub fn into_map(self) -> M {
        match self {
            Self::Square(g) => g.map,
            Self::Hex(g) => g.map,
        }
    }
}

impl<M: CellMap + HexMap> Graph for AnyGraph<M> {
    type Node = Cell;

    fn neighbors(&mut self, node: Cell, out: &mut Vec<(Cell, Cost)>) {
        match self {
            Self::Square(g) => g.neighbors(node, out),
            Self::Hex(g) => g.neighbors(node, out),
        }
    }
}

/// Go to cell `x`, `y` on `level` (or any level) on either grid kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnyGoal {
    /// Column.
    pub x: i32,
    /// Row.
    pub y: i32,
    /// Required level, or any.
    pub level: Option<i32>,
    /// Grid kind and costs (for the estimate).
    pub topology: Topology,
}

impl crate::search::Goal<Cell> for AnyGoal {
    fn is_goal(&self, node: Cell) -> bool {
        node.x == self.x && node.y == self.y && self.level.is_none_or(|l| l == node.level)
    }

    fn estimate(&self, node: Cell) -> Cost {
        // the estimate ignores levels: never more than the real cost
        self.topology
            .estimate(node, Cell::new(self.x, self.y, node.level))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::DenseGrid;
    use crate::search::{Limits, find_path};

    #[test]
    fn hex_paths_smooth_and_turn_on_every_layout() {
        for layout in [
            HexLayout::OddR,
            HexLayout::EvenR,
            HexLayout::OddQ,
            HexLayout::EvenQ,
        ] {
            let topo = Topology::Hex {
                layout,
                step_cost: 1,
            };
            let mut grid = DenseGrid::new(20, 20);
            let goal = AnyGoal {
                x: 15,
                y: 4,
                level: None,
                topology: topo,
            };
            let r = find_path(
                &mut topo.graph(&mut grid, None),
                Cell::new(2, 12, 0),
                &goal,
                Limits::default(),
                usize::MAX,
            );
            assert!(r.reached());
            let mut out = Vec::new();
            topo.smooth(&mut grid, &r.path, 0, &mut out);
            assert_eq!(
                out,
                [Cell::new(2, 12, 0), Cell::new(15, 4, 0)],
                "{layout:?}: open ground is one line"
            );
            topo.turning_points(&r.path, false, &mut out);
            assert!(
                out.len() <= 3,
                "{layout:?}: a hex way bends at most twice on open ground, got {out:?}"
            );
            assert_eq!(out.last(), Some(&Cell::new(15, 4, 0)));

            // a wall of blocked hexes: the smoothed corners stay walkable
            for y in 0..16 {
                grid.set(9, y, DenseGrid::BLOCKED);
            }
            let r = find_path(
                &mut topo.graph(&mut grid, None),
                Cell::new(2, 12, 0),
                &goal,
                Limits::default(),
                usize::MAX,
            );
            topo.smooth(&mut grid, &r.path, 0, &mut out);
            assert!(out.len() >= 3 && out.len() < r.path.len(), "{layout:?}");
            for w in out.windows(2) {
                assert!(
                    topo.line_clear(&mut grid, w[0], w[1], 0),
                    "{layout:?}: {:?} -> {:?}",
                    w[0],
                    w[1]
                );
            }
            assert!(!topo.line_clear(&mut grid, Cell::new(5, 5, 0), Cell::new(13, 5, 0), 0));
        }
    }

    #[test]
    fn hex_formation_keeps_its_shape_and_queues_at_a_door() {
        for layout in [
            HexLayout::OddR,
            HexLayout::EvenR,
            HexLayout::OddQ,
            HexLayout::EvenQ,
        ] {
            let topo = Topology::Hex {
                layout,
                step_cost: 1,
            };
            let mut grid = DenseGrid::new(30, 20);
            // a wall across column 15, door at row 10
            for y in 0..20 {
                if y != 10 {
                    grid.set(15, y, DenseGrid::BLOCKED);
                }
            }
            let goal = AnyGoal {
                x: 25,
                y: 10,
                level: None,
                topology: topo,
            };
            let r = find_path(
                &mut topo.graph(&mut grid, None),
                Cell::new(5, 10, 0),
                &goal,
                Limits::default(),
                usize::MAX,
            );
            assert!(r.reached());
            let mut out = Vec::new();
            topo.place_members(&mut grid, &r.path, &Formation::block_of_four(), &mut out);
            for i in 0..r.path.len() {
                let mut cells = vec![r.path[i]];
                for targets in &out {
                    let t = targets[i];
                    assert!(
                        grid.get(t.x, t.y).is_some_and(|c| c != DenseGrid::BLOCKED),
                        "{layout:?} step {i}: {t:?}"
                    );
                    assert!(!cells.contains(&t), "{layout:?} step {i}: two on {t:?}");
                    cells.push(t);
                }
            }
            // at the end all three stand next to the leader again
            let end = *r.path.last().unwrap();
            for targets in &out {
                let t = *targets.last().unwrap();
                assert_eq!(layout.distance(t.x, t.y, end.x, end.y), 1, "{layout:?}");
            }
        }
    }
}
