//! Perception, plus the adapters that join it with the nav maps.
//!
//! Everything of [`sagax_perception`] is re-exported here. The adapters
//! derive a [`SenseMap`] from a nav map for games that have no own sight
//! data. Perception cells are axial on hex, nav maps are keyed by offset
//! cells: the `hex` constructors take the map's [`HexLayout`] and convert
//! every cell before asking the map.

use sagax_core::{Cell, HexLayout};
use sagax_nav::grid::{CellMap, DenseGrid};

pub use sagax_perception::*;

/// An axial cell as the offset cell of `layout`; unchanged without one.
fn to_map(layout: Option<HexLayout>, cell: Cell) -> Cell {
    match layout {
        Some(layout) => {
            let (x, y) = layout.from_axial(cell.x, cell.y);
            Cell::new(x, y, cell.level)
        }
        None => cell,
    }
}

/// Sight over a [`DenseGrid`]: [`BLOCKED`](DenseGrid::BLOCKED) cells and
/// cells off the grid are opaque, nothing else (no fog, no thin walls).
#[derive(Clone, Copy, Debug)]
pub struct GridSight<'a> {
    /// The map.
    pub grid: &'a DenseGrid,
    /// On hex, the map's offset layout (cells are converted from axial);
    /// `None` on square.
    pub layout: Option<HexLayout>,
}

impl<'a> GridSight<'a> {
    /// Over a square grid.
    pub fn square(grid: &'a DenseGrid) -> Self {
        Self { grid, layout: None }
    }

    /// Over a hex grid stored in offset cells of `layout`.
    pub fn hex(grid: &'a DenseGrid, layout: HexLayout) -> Self {
        Self {
            grid,
            layout: Some(layout),
        }
    }
}

impl SenseMap for GridSight<'_> {
    fn sight(&mut self, cell: Cell) -> Medium {
        let cost = self.grid.get_at(to_map(self.layout, cell));
        Medium {
            opaque: cost.is_none_or(|c| c == DenseGrid::BLOCKED),
            ..Medium::default()
        }
    }
}

/// Sight blocked where movement is: crossing from a cell into its
/// neighbour is open exactly when [`CellMap::step`] allows the step and
/// stays on the same level. For games that share one blocker for walking
/// and seeing.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveBlocks<M: CellMap> {
    /// The movement map.
    pub map: M,
    /// On hex, the map's offset layout (cells are converted from axial and
    /// `step` gets the offset delta); `None` on square.
    pub layout: Option<HexLayout>,
}

impl<M: CellMap> MoveBlocks<M> {
    /// Over a square map.
    pub fn square(map: M) -> Self {
        Self { map, layout: None }
    }

    /// Over a hex map keyed by offset cells of `layout`.
    pub fn hex(map: M, layout: HexLayout) -> Self {
        Self {
            map,
            layout: Some(layout),
        }
    }
}

impl<M: CellMap> SenseMap for MoveBlocks<M> {
    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool {
        let (from, to) = (to_map(self.layout, from), to_map(self.layout, to));
        self.map.step(from, to.x - from.x, to.y - from.y) == Some(from.level)
    }
}
