//! Perception, plus the adapters that join it with the nav maps.
//!
//! Everything of [`sagax_perception`] is re-exported here. The adapters
//! derive a [`SenseMap`] from a nav map for games that have no own sight
//! data. Both address the map with the cells they are given, so on hex the
//! map must be keyed by axial cells.

use sagax_core::Cell;
use sagax_nav::grid::{CellMap, DenseGrid};

pub use sagax_perception::*;

/// Sight over a [`DenseGrid`]: [`BLOCKED`](DenseGrid::BLOCKED) cells and
/// cells off the grid are opaque, nothing else (no fog, no thin walls).
#[derive(Clone, Copy, Debug)]
pub struct GridSight<'a>(pub &'a DenseGrid);

impl SenseMap for GridSight<'_> {
    fn sight(&mut self, cell: Cell) -> Medium {
        Medium {
            opaque: self.0.get_at(cell).is_none_or(|c| c == DenseGrid::BLOCKED),
            ..Medium::default()
        }
    }
}

/// Sight blocked where movement is: crossing from a cell into its
/// neighbour is open exactly when [`CellMap::step`] allows the step and
/// stays on the same level. For games that share one blocker for walking
/// and seeing.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveBlocks<M: CellMap>(pub M);

impl<M: CellMap> SenseMap for MoveBlocks<M> {
    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool {
        self.0.step(from, to.x - from.x, to.y - from.y) == Some(from.level)
    }
}
