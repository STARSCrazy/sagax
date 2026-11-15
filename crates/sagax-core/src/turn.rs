//! Turns and seeds: the sine tables of the 0..=255 heading and the seed
//! mix, shared by decisions and perception.

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

/// Which sine table [`sine`] (and the turn helpers built on it) use.
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
    /// The value of a quarter turn: 65536 for [`Sine::Fine`], 256 for
    /// [`Sine::Coarse`].
    pub fn amplitude(self) -> i64 {
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

/// A well-mixed number from `seed` (SplitMix64 finaliser).
pub fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
