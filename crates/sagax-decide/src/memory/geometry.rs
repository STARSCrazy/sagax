use super::Pos;

/// Sine of a quarter wave in 17 steps of 4, times 256.
const COARSE_QUARTER: [i32; 17] = [
    0, 25, 50, 74, 98, 121, 142, 162, 181, 198, 213, 226, 237, 245, 251, 255, 256,
];

/// Sine of a quarter wave in 65 steps of 1 (a full turn is 256), times
/// 65536, rounded to the nearest integer.
const FINE_QUARTER: [i32; 65] = [
    0, 1608, 3216, 4821, 6424, 8022, 9616, 11204, 12785, 14359, 15924, 17479, 19024, 20557, 22078,
    23586, 25080, 26558, 28020, 29466, 30893, 32303, 33692, 35062, 36410, 37736, 39040, 40320,
    41576, 42806, 44011, 45190, 46341, 47464, 48559, 49624, 50660, 51665, 52639, 53581, 54491,
    55368, 56212, 57022, 57798, 58538, 59244, 59914, 60547, 61145, 61705, 62228, 62714, 63162,
    63572, 63944, 64277, 64571, 64827, 65043, 65220, 65358, 65457, 65516, 65536,
];

/// Which sine table [`sine`] and [`points_around`] use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Sine {
    /// 65-entry quarter table, one entry per heading, amplitude 65536.
    #[default]
    Fine = 0,
    /// 17-entry quarter table in steps of 4 headings, amplitude 256; the
    /// reference table of the original officer behaviour.
    Coarse = 1,
}

impl Sine {
    /// The value of a quarter turn.
    fn amplitude(self) -> i64 {
        match self {
            Sine::Fine => 65536,
            Sine::Coarse => 256,
        }
    }
}

/// Sine of `h` in the amplitude of `s` (65536 for [`Sine::Fine`], 256 for
/// [`Sine::Coarse`]), where a full turn is 256 (a quarter turn is 64). Only
/// the low 8 bits of `h` count. Coarse moves in steps of 4, rounded to the
/// nearest step.
pub fn sine(h: i32, s: Sine) -> i32 {
    match s {
        Sine::Fine => {
            let i = (h & 0xff) as usize;
            match i {
                0..=64 => FINE_QUARTER[i],
                65..=128 => FINE_QUARTER[128 - i],
                129..=192 => -FINE_QUARTER[i - 128],
                _ => -FINE_QUARTER[256 - i],
            }
        }
        Sine::Coarse => {
            let i = ((((h & 0xff) + 2) / 4) & 63) as usize;
            match i {
                0..16 => COARSE_QUARTER[i],
                16..32 => COARSE_QUARTER[32 - i],
                32..48 => -COARSE_QUARTER[i - 32],
                _ => -COARSE_QUARTER[64 - i],
            }
        }
    }
}

/// Where an agent searches around a centre, without random numbers: each
/// agent gets its own heading (`agent * heading_step`), step 0 stands
/// `first_distance` away along it, and step `n` (1-based) turns by
/// `offsets[n-1]` and goes `distances[n-1]` out. Points are clamped to
/// `min ..= max` per axis.
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
    /// clamped to `0 ..= 32767`, with [`Sine::Fine`].
    fn default() -> Self {
        Self {
            heading_step: 0x62,
            first_distance: 0x100,
            offsets: vec![0, 0x20, -0x20],
            distances: vec![5 * 256, 9 * 256, 12 * 256],
            sine: Sine::Fine,
            min: (0, 0),
            max: (32767, 32767),
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
