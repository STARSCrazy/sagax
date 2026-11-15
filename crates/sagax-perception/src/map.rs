//! The game hook: what sight and sound meet per cell and per edge.

use sagax_core::{Cell, Pos};

/// What a sight line meets inside one cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Medium {
    /// Stops sight (a wall, a closed door).
    pub opaque: bool,
    /// Fog or smoke: optical depth per full cell length. 256 halves the
    /// clarity per cell; it adds up along the ray by length.
    pub density: u16,
    /// The same for thermal sight (smoke is usually 0 here, a wall stays
    /// opaque).
    pub heat_density: u16,
}

/// The game's answers per cell (or per edge between two neighbours), for
/// sight and sound. Separate from movement; every method has a default, so
/// a game answers only what it has.
pub trait SenseMap {
    /// Inside `cell`. Default: clear air.
    fn sight(&mut self, cell: Cell) -> Medium {
        let _ = cell;
        Medium::default()
    }

    /// Crossing from `from` into the neighbour `to` (thin walls, windows,
    /// doors). `false` blocks. Default: open.
    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool {
        let _ = (from, to);
        true
    }

    /// A line between two levels (`from[2] != to[2]`). `None` blocks; a
    /// medium applies once over the whole length. Default: blocked.
    fn cross_level(&mut self, from: Pos, to: Pos) -> Option<Medium> {
        let _ = (from, to);
        None
    }

    /// Light at a cell, 0 (dark) ..= 255 (daylight). Default: 255.
    fn light(&mut self, cell: Cell) -> u8 {
        let _ = cell;
        255
    }

    /// Sound damping inside a cell, in fine units per cell length.
    /// Default: 0.
    fn sound(&mut self, cell: Cell) -> u32 {
        let _ = cell;
        0
    }

    /// Sound damping crossing into a neighbour (a wall that a sound passes
    /// muffled), fine units. `None`: does not pass at all. Default:
    /// `Some(0)`.
    fn sound_edge(&mut self, from: Cell, to: Cell) -> Option<u32> {
        let _ = (from, to);
        Some(0)
    }
}

impl<H: SenseMap + ?Sized> SenseMap for &mut H {
    fn sight(&mut self, cell: Cell) -> Medium {
        (**self).sight(cell)
    }

    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool {
        (**self).sight_edge(from, to)
    }

    fn cross_level(&mut self, from: Pos, to: Pos) -> Option<Medium> {
        (**self).cross_level(from, to)
    }

    fn light(&mut self, cell: Cell) -> u8 {
        (**self).light(cell)
    }

    fn sound(&mut self, cell: Cell) -> u32 {
        (**self).sound(cell)
    }

    fn sound_edge(&mut self, from: Cell, to: Cell) -> Option<u32> {
        (**self).sound_edge(from, to)
    }
}

/// No map at all: clear, lit and silent everywhere; levels do not see each
/// other. Cone, range and light then come from the target only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct OpenAir;

impl SenseMap for OpenAir {}

/// The game's hook plus one global light level (time of day, rain):
/// `light = min(inner light, ambient)`, so `ambient` wherever the hook
/// keeps the default. Every other answer is the inner one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lit<H: SenseMap> {
    /// The game's hook.
    pub inner: H,
    /// The global light level, 0 (dark) ..= 255 (daylight).
    pub ambient: u8,
}

impl<H: SenseMap> SenseMap for Lit<H> {
    fn sight(&mut self, cell: Cell) -> Medium {
        self.inner.sight(cell)
    }

    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool {
        self.inner.sight_edge(from, to)
    }

    fn cross_level(&mut self, from: Pos, to: Pos) -> Option<Medium> {
        self.inner.cross_level(from, to)
    }

    fn light(&mut self, cell: Cell) -> u8 {
        self.inner.light(cell).min(self.ambient)
    }

    fn sound(&mut self, cell: Cell) -> u32 {
        self.inner.sound(cell)
    }

    fn sound_edge(&mut self, from: Cell, to: Cell) -> Option<u32> {
        self.inner.sound_edge(from, to)
    }
}
