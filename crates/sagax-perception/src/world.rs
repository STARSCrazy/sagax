//! The world config and its units: fine points inside a cell, whole cells
//! at every edge.

use sagax_core::{Cell, Pos};

/// Number of hearing channels in a [`World`].
pub const MAX_CHANNELS: usize = 4;

/// The kind of grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Grid {
    /// Square cells; fine points `(x, y)` on the cell grid.
    #[default]
    Square = 0,
    /// Hexes; fine points are axial `(q, r)`, hex centres sit on
    /// `(q * cell_size, r * cell_size)`. Cells are axial too (an offset
    /// layout exists only at the C edge, on whole cells).
    Hex = 1,
}

/// How a stimulus on a channel travels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Spread {
    /// Straight distance only, walls do not matter.
    #[default]
    Circle = 0,
    /// One straight walk: distance plus the map's damping along it.
    Straight = 1,
}

/// One hearing channel (sound, smell, smoke, ...).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Channel {
    /// How much the map's sound damping counts, per 256 (0: walls do not
    /// matter).
    pub damping: u16,
    /// Position error in cells per cell of cost, per 256.
    pub fuzz: u16,
    /// Fine units per tick (0: arrives at once).
    pub speed: i32,
    /// How it travels.
    pub spread: Spread,
}

impl Default for Channel {
    /// Full damping, no fuzz, arrives at once, [`Spread::Circle`].
    fn default() -> Self {
        Self {
            damping: 256,
            fuzz: 0,
            speed: 0,
            spread: Spread::Circle,
        }
    }
}

/// The world config: grid, cell size and the hearing channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct World {
    /// Square or hex.
    pub grid: Grid,
    /// Fine units per cell or hex (`S`), at least 2 (for example 256);
    /// smaller values count as 1.
    pub cell_size: i32,
    /// The hearing channels, indexed by a stimulus' channel.
    pub channels: [Channel; MAX_CHANNELS],
}

impl World {
    /// A square world with `cell_size` fine units per cell and default
    /// channels.
    pub fn square(cell_size: i32) -> Self {
        Self {
            grid: Grid::Square,
            cell_size,
            channels: [Channel::default(); MAX_CHANNELS],
        }
    }

    /// A hex world with `cell_size` fine units per hex and default
    /// channels.
    pub fn hex(cell_size: i32) -> Self {
        Self {
            grid: Grid::Hex,
            ..Self::square(cell_size)
        }
    }

    /// `S` as an `i64`, at least 1.
    pub(crate) fn size(&self) -> i64 {
        i64::from(self.cell_size.max(1))
    }
}

/// An `i64` clamped into `i32`.
fn fit(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The fine point at the middle of `cell`: `(x * S + S / 2, y * S + S / 2)`
/// on square (`S / 2` truncating), `(q * S, r * S)` on hex. The level is
/// kept. Clamped into `i32` for cells too far out.
pub fn cell_centre(world: &World, cell: Cell) -> Pos {
    let s = world.size();
    let (x, y) = (i64::from(cell.x) * s, i64::from(cell.y) * s);
    match world.grid {
        Grid::Square => [fit(x + s / 2), fit(y + s / 2), cell.level],
        Grid::Hex => [fit(x), fit(y), cell.level],
    }
}

/// The cell that holds the fine point `point` (level `point[2]`). Square:
/// floor division by `S` on each axis, so a lower border belongs to the
/// cell. Hex: integer cube rounding of `(q / S, r / S, s / S)`: each is
/// rounded to the nearest integer (halves toward +infinity), then the axis
/// with the largest error is recomputed from the other two; on equal
/// errors `s` first, then `r`. `cell_of(cell_centre(c)) == c` on both
/// grids.
pub fn cell_of(world: &World, point: Pos) -> Cell {
    let s = world.size();
    let (x, y) = (i64::from(point[0]), i64::from(point[1]));
    match world.grid {
        Grid::Square => Cell::new(fit(x.div_euclid(s)), fit(y.div_euclid(s)), point[2]),
        Grid::Hex => {
            let (q, r) = hex_round(x, y, s);
            Cell::new(fit(q), fit(r), point[2])
        }
    }
}

/// Cube rounding of the axial point `(q / den, r / den)`, exact: each of
/// `q`, `r`, `s = -q - r` is rounded to the nearest integer (halves toward
/// +infinity), then the axis with the largest error is recomputed from the
/// other two; on equal errors `s` first, then `r`. `den > 0`.
pub(crate) fn hex_round(q: i64, r: i64, den: i64) -> (i64, i64) {
    let s = -q - r;
    // round(v / den) = floor((2v + den) / (2den)); in i128 against overflow.
    let round = |v: i64| (2 * i128::from(v) + i128::from(den)).div_euclid(2 * i128::from(den));
    let (rq, rr, rs) = (round(q), round(r), round(s));
    let den = i128::from(den);
    let err = |v: i64, rounded: i128| (i128::from(v) - rounded * den).abs();
    let (eq, er, es) = (err(q, rq), err(r, rr), err(s, rs));
    let (q, r) = if es >= eq && es >= er {
        (rq, rr)
    } else if er >= eq {
        (rq, -rq - rs)
    } else {
        (-rr - rs, rr)
    };
    (q as i64, r as i64)
}

/// The heading (0..=255, 0 = +x, 64 = +y) of hex direction `dir` (taken
/// modulo 6; 0 = +q, 1 = +r): `round(dir * 256 / 6)`, so 0, 43, 85, 128,
/// 171, 213.
pub fn hex_heading(dir: u8) -> u8 {
    const HEADINGS: [u8; 6] = [0, 43, 85, 128, 171, 213];
    HEADINGS[usize::from(dir % 6)]
}

/// The straight distance in fine units from `a` to `b` (levels ignored):
/// Euclidean (integer square root, truncating) on square, the hex distance
/// of the fine axial points on hex.
pub(crate) fn distance(world: &World, a: Pos, b: Pos) -> i64 {
    let dx = i64::from(b[0]) - i64::from(a[0]);
    let dy = i64::from(b[1]) - i64::from(a[1]);
    match world.grid {
        Grid::Square => {
            let sq = (i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy)) as u128;
            sq.isqrt() as i64
        }
        Grid::Hex => (dx.abs() + dy.abs() + (dx + dy).abs()) / 2,
    }
}

/// The Euclidean length of `b - a` in fine units times 65536, truncating
/// (levels ignored). On hex the axial axes are 60 degrees apart: the
/// squared length is `dq² + dq·dr + dr²`, so one hex step of `S` is one
/// cell length.
pub(crate) fn length_fix(world: &World, a: Pos, b: Pos) -> u128 {
    let dx = i128::from(b[0]) - i128::from(a[0]);
    let dy = i128::from(b[1]) - i128::from(a[1]);
    let sq = match world.grid {
        Grid::Square => dx * dx + dy * dy,
        Grid::Hex => dx * dx + dx * dy + dy * dy,
    } as u128;
    (sq << 32).isqrt()
}
