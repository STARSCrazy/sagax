use super::Pos;
pub use sagax_core::{Sine, sine};

/// Where an agent searches around a centre, without random numbers: each
/// agent gets its own heading (`agent * heading_step`), step 0 stands
/// `first_distance` away along it, and step `n` (1-based) turns by
/// `offsets[n-1]` and goes `distances[n-1]` out. Points are clamped to
/// `min ..= max` per axis.
///
/// The ring is Euclidean (x and y are plain axes). On an axial hex grid it is
/// therefore skewed: convert the centre to the game's world coordinates first
/// and convert the result back, or snap the result to the nearest hex.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PointPattern {
    /// Heading between neighbouring agents (a full turn is 256).
    pub heading_step: i32,
    /// Distance of step 0 from the centre.
    pub first_distance: i32,
    /// Turn of each later step from the agent heading.
    pub offsets: Vec<i32>,
    /// Distance of each later step from the centre (meant to be as long as
    /// `offsets`; a missing entry counts as 0).
    pub distances: Vec<i32>,
    /// Sine table used for the points.
    pub sine: Sine,
    /// Smallest allowed `(x, y)`.
    pub min: (i32, i32),
    /// Largest allowed `(x, y)`.
    pub max: (i32, i32),
}

impl Default for PointPattern {
    /// Heading step 0x62 (near the golden angle), step 0 at 0x100, then
    /// three points at 5, 9 and 12 times 256 with turns 0, 0x20, -0x20,
    /// not clamped (`min` and `max` span all of `i32`), with [`Sine::Fine`].
    /// Set `min` and `max` for a bounded map.
    fn default() -> Self {
        Self {
            heading_step: 0x62,
            first_distance: 0x100,
            offsets: vec![0, 0x20, -0x20],
            distances: vec![5 * 256, 9 * 256, 12 * 256],
            sine: Sine::Fine,
            min: (i32::MIN, i32::MIN),
            max: (i32::MAX, i32::MAX),
        }
    }
}

/// `distance` scaled by a sine value of the amplitude of `s`, truncating
/// toward zero.
fn along(value: i32, distance: i32, s: Sine) -> i64 {
    i64::from(value) * i64::from(distance) / s.amplitude()
}

/// Point `step` of `agent` around `centre` (z is kept). Step 0 is
/// the first stand, steps `1 ..= offsets.len()` the search points; any
/// later step gives `None`. A missing entry in `distances` counts as 0.
pub fn points_around(agent: u32, step: usize, centre: Pos, pattern: &PointPattern) -> Option<Pos> {
    // Only the low 8 bits of the heading matter, so wrapping is harmless.
    let mut heading = (agent as i32).wrapping_mul(pattern.heading_step);
    let mut distance = pattern.first_distance;
    if step > 0 {
        heading = heading.wrapping_add(*pattern.offsets.get(step - 1)?);
        distance = pattern.distances.get(step - 1).copied().unwrap_or(0);
    }
    let x = i64::from(centre[0]) + along(sine(heading, pattern.sine), distance, pattern.sine);
    let y = i64::from(centre[1])
        + along(
            sine(heading.wrapping_add(64), pattern.sine),
            distance,
            pattern.sine,
        );
    let fit = |v: i64, lo: i32, hi: i32| v.min(i64::from(hi)).max(i64::from(lo)) as i32;
    Some([
        fit(x, pattern.min.0, pattern.max.0),
        fit(y, pattern.min.1, pattern.max.1),
        centre[2],
    ])
}

/// Looking around, for [`sweep_heading`]: a quarter turn to one side, back,
/// to the other side and back.
pub const LOOK_OFFSETS: [i32; 8] = [0, 32, 64, 32, 0, -32, -64, -32];

/// Heading `elapsed` ticks into looking around from `base`: plus
/// `offsets[elapsed / step_ticks]`, or plus nothing once past the end. The
/// result is a heading `0 ..= 255`. A `step_ticks` of 0 counts as 1.
pub fn sweep_heading(base: i32, elapsed: u32, step_ticks: u32, offsets: &[i32]) -> i32 {
    let i = (elapsed / step_ticks.max(1)) as usize;
    base.wrapping_add(offsets.get(i).copied().unwrap_or(0)) & 0xff
}
