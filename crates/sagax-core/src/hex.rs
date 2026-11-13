//! Hex layouts and axial coordinates.
//!
//! Cells are addressed by "offset" coordinates (column `x`, row `y`), as games
//! and Foundry VTT store them. Every second row (or column) is shifted by half
//! a cell; [`HexLayout`] says which. Internally distances use axial
//! coordinates. Each cell has six neighbours, numbered 0..6 so that `i` and
//! `(i + 3) % 6` are opposite.

use crate::Pos;

/// How the rows or columns of a hex grid are shifted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HexLayout {
    /// Pointy top, odd rows shifted right.
    OddR,
    /// Pointy top, even rows shifted right.
    EvenR,
    /// Flat top, odd columns shifted down.
    OddQ,
    /// Flat top, even columns shifted down.
    EvenQ,
}

/// Axial directions (q, r); `i` and `(i + 3) % 6` are opposite.
const AXIAL: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

/// The axial step (q, r) of direction `dir` (taken modulo 6). Directions go
/// round counter-clockwise on screen (y down), so `dir + 5` is 60 degrees
/// clockwise ("right") of `dir`.
pub fn axial_step(dir: usize) -> (i32, i32) {
    AXIAL[dir % 6]
}

impl HexLayout {
    /// Offset (column, row) to axial (q, r).
    pub fn to_axial(self, x: i32, y: i32) -> (i32, i32) {
        match self {
            Self::OddR => (x - (y - (y & 1)) / 2, y),
            Self::EvenR => (x - (y + (y & 1)) / 2, y),
            Self::OddQ => (x, y - (x - (x & 1)) / 2),
            Self::EvenQ => (x, y - (x + (x & 1)) / 2),
        }
    }

    /// Axial (q, r) to offset (column, row).
    pub fn from_axial(self, q: i32, r: i32) -> (i32, i32) {
        match self {
            Self::OddR => (q + (r - (r & 1)) / 2, r),
            Self::EvenR => (q + (r + (r & 1)) / 2, r),
            Self::OddQ => (q, r + (q - (q & 1)) / 2),
            Self::EvenQ => (q, r + (q + (q & 1)) / 2),
        }
    }

    /// [`to_axial`](HexLayout::to_axial) for a [`Pos`]: `[q, r, z]`, `z` kept.
    pub fn to_axial_pos(self, p: Pos) -> Pos {
        let (q, r) = self.to_axial(p[0], p[1]);
        [q, r, p[2]]
    }

    /// [`from_axial`](HexLayout::from_axial) for a `[q, r, z]` [`Pos`]:
    /// `[x, y, z]`, `z` kept.
    pub fn from_axial_pos(self, p: Pos) -> Pos {
        let (x, y) = self.from_axial(p[0], p[1]);
        [x, y, p[2]]
    }

    /// The neighbour of `x`, `y` in direction `dir` (0..6).
    pub fn neighbor(self, x: i32, y: i32, dir: usize) -> (i32, i32) {
        let (q, r) = self.to_axial(x, y);
        let (dq, dr) = AXIAL[dir % 6];
        self.from_axial(q + dq, r + dr)
    }

    /// The direction from `x`, `y` to its neighbour `nx`, `ny`, if they are neighbours.
    pub fn direction(self, x: i32, y: i32, nx: i32, ny: i32) -> Option<usize> {
        (0..6).find(|&d| self.neighbor(x, y, d) == (nx, ny))
    }

    /// Number of steps between two cells on open ground.
    pub fn distance(self, ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
        let (aq, ar) = self.to_axial(ax, ay);
        let (bq, br) = self.to_axial(bx, by);
        let (dq, dr) = (aq - bq, ar - br);
        (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
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
    fn axial_round_trip() {
        for layout in ALL {
            for x in -5..5 {
                for y in -5..5 {
                    let (q, r) = layout.to_axial(x, y);
                    assert_eq!(layout.from_axial(q, r), (x, y), "{layout:?}");
                }
            }
        }
    }

    #[test]
    fn six_distinct_neighbours_at_distance_one_and_opposites_return() {
        for layout in ALL {
            for (x, y) in [(0, 0), (1, 0), (0, 1), (3, 4), (-1, -3)] {
                let mut seen = Vec::new();
                for d in 0..6 {
                    let (nx, ny) = layout.neighbor(x, y, d);
                    assert_eq!(layout.distance(x, y, nx, ny), 1);
                    assert_eq!(layout.neighbor(nx, ny, (d + 3) % 6), (x, y));
                    assert_eq!(layout.direction(x, y, nx, ny), Some(d));
                    seen.push((nx, ny));
                }
                seen.sort();
                seen.dedup();
                assert_eq!(seen.len(), 6);
            }
        }
    }

    #[test]
    fn odd_r_neighbours_match_the_textbook() {
        // odd-r: row 1 is shifted right, so (0,1) touches (0,0) and (1,0)
        let mut n: Vec<_> = (0..6).map(|d| HexLayout::OddR.neighbor(0, 1, d)).collect();
        n.sort();
        assert_eq!(n, [(-1, 1), (0, 0), (0, 2), (1, 0), (1, 1), (1, 2)]);
    }

    #[test]
    fn axial_pos_keeps_z() {
        assert_eq!(HexLayout::OddR.to_axial_pos([3, 3, 2]), [2, 3, 2]);
        assert_eq!(HexLayout::OddR.from_axial_pos([2, 3, 2]), [3, 3, 2]);
        assert_eq!(HexLayout::EvenR.to_axial_pos([3, 1, 0]), [2, 1, 0]);
        assert_eq!(HexLayout::OddQ.to_axial_pos([3, 3, 0]), [3, 2, 0]);
    }
}
