//! The cone test: is a direction within a half-angle of the facing?

use sagax_core::{Pos, Sine, sine};

use crate::world::{Grid, World};

/// `√3` as `113512 / 65536` (error below 1e-5), for hex angles only.
const SQRT3_FIX: i64 = 113_512;

/// Whether the vector `v` (square plane, any length) lies within
/// `half_angle` headings of `facing` (headings 0..=255, 0 = +x, 64 = +y).
///
/// With the facing vector `f = (cos, sin)` of the fine table (amplitude
/// 65536), `dot = f·v` and `cross = |f × v|`, `v` is in exactly when
/// `sin(h) * dot >= cos(h) * cross` (in `i128`): exact for the table
/// values, the same left and right, and fine for cones wider than 90
/// degrees. Half-angles above 128 count as 128 (all around). Half-angle 0
/// holds only the straight-ahead ray. `v == (0, 0)` is always in.
pub fn in_band(facing: u8, v: (i64, i64), half_angle: u8) -> bool {
    let h = i32::from(half_angle.min(128));
    let facing = i32::from(facing);
    let (fx, fy) = (
        i128::from(sine(facing + 64, Sine::Fine)),
        i128::from(sine(facing, Sine::Fine)),
    );
    let (vx, vy) = (i128::from(v.0), i128::from(v.1));
    let dot = fx * vx + fy * vy;
    let cross = (fx * vy - fy * vx).abs();
    if h == 0 {
        return cross == 0 && dot >= 0;
    }
    let (sin, cos) = (
        i128::from(sine(h, Sine::Fine)),
        i128::from(sine(h + 64, Sine::Fine)),
    );
    sin * dot >= cos * cross
}

/// The direction from `from` to `to` as a square-plane vector for
/// [`in_band`] (levels ignored). Square: `(dx, dy)`. Hex: the axial
/// difference in the embedding `X = 2dq + dr`, `Y = dr·√3`, kept exact as
/// `(X · 65536, dr · 113512)`.
pub fn view_vector(world: &World, from: Pos, to: Pos) -> (i64, i64) {
    let dx = i64::from(to[0]) - i64::from(from[0]);
    let dy = i64::from(to[1]) - i64::from(from[1]);
    match world.grid {
        Grid::Square => (dx, dy),
        Grid::Hex => ((2 * dx + dy) * 65536, dy * SQRT3_FIX),
    }
}
