//! Places: [`Pos`] (three integers) and [`Cell`] (column, row, level).

/// A place: three integer coordinates `[x, y, z]` in the game's own units.
pub type Pos = [i32; 3];

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

    /// The cell as a [`Pos`] (`[x, y, level]`).
    pub const fn pos(self) -> Pos {
        [self.x, self.y, self.level]
    }
}

impl From<Cell> for Pos {
    fn from(cell: Cell) -> Self {
        cell.pos()
    }
}

impl From<Pos> for Cell {
    fn from(pos: Pos) -> Self {
        Self::new(pos[0], pos[1], pos[2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_pos_round_trip() {
        assert_eq!(Pos::from(Cell::new(1, 2, 3)), [1, 2, 3]);
        assert_eq!(Cell::from([1, 2, 3]), Cell::new(1, 2, 3));
        assert_eq!(Cell::new(1, 2, 3).pos(), [1, 2, 3]);
    }
}
