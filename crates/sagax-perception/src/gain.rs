//! The gain chain of one check: geometry, sight lines, light and the
//! factors in their fixed order, the thermal pass, the scanner and the
//! instant cues.

use sagax_core::{Pos, Sink};

use crate::cone::{in_band, view_vector};
use crate::line::{Line, sight_line_with};
use crate::map::SenseMap;
use crate::percept::{Sense, Target};
use crate::profile::{
    Band, CueRule, Eye, MAX_CUES, NIGHT, SCANNER, SIGHT, SenseProfile, Stats, THERMAL,
};
use crate::trace::PerceptionTrace;
use crate::world::{Grid, World, cell_of, distance};

/// The sight range with the stats: `range * sight_scale / 256`.
pub(crate) fn sight_range(p: &SenseProfile, stats: Stats) -> i64 {
    i64::from(p.range) * i64::from(stats.sight_scale) / 256
}

/// Whether `b` is within `r` fine units of `a` (levels ignored): squared
/// Euclidean on square, exact; the hex distance on hex.
pub(crate) fn within(world: &World, a: Pos, b: Pos, r: i64) -> bool {
    if r < 0 {
        return false;
    }
    match world.grid {
        Grid::Square => {
            let dx = i128::from(b[0]) - i128::from(a[0]);
            let dy = i128::from(b[1]) - i128::from(a[1]);
            dx * dx + dy * dy <= i128::from(r) * i128::from(r)
        }
        Grid::Hex => distance(world, a, b) <= r,
    }
}

/// The first band (index and band) with a gain that holds `point`.
pub(crate) fn band_of(world: &World, eye: &Eye, point: Pos) -> Option<(u8, Band)> {
    let v = view_vector(world, eye.point, point);
    (0u8..)
        .zip(eye.profile.bands)
        .find(|&(_, b)| b.gain > 0 && in_band(eye.facing, v, b.half_angle))
}

/// 256 up to `near`, then linear to `far_gain` at `range` (truncating).
fn distance_factor(p: &SenseProfile, d: i64, range: i64) -> u16 {
    let near = i64::from(p.near);
    if d <= near || range <= near {
        return 256;
    }
    let far = i64::from(p.far_gain);
    let f = 256 - (256 - far) * (d.min(range) - near) / (range - near);
    f.clamp(0, i64::from(u16::MAX)) as u16
}

/// The light factor: `max(light, glow)`, lifted to `bright` with
/// [`NIGHT`]; `dark_gain` at or below `dark`, 256 at or above `bright`,
/// linear in between.
fn light_factor(world: &World, map: &mut impl SenseMap, p: &SenseProfile, t: &Target) -> u16 {
    let mut l = map.light(cell_of(world, t.point)).max(t.glow);
    if p.senses & NIGHT != 0 {
        l = l.max(p.bright);
    }
    if l >= p.bright {
        return 256;
    }
    let dark_gain = i64::from(p.dark_gain);
    if l <= p.dark {
        return p.dark_gain;
    }
    let (l, dark, bright) = (i64::from(l), i64::from(p.dark), i64::from(p.bright));
    let f = dark_gain + (256 - dark_gain) * (l - dark) / (bright - dark);
    f.clamp(0, i64::from(u16::MAX)) as u16
}

/// One factor step of the gain chain: `g * f >> 8`, saturating.
fn step(g: u64, f: u16) -> u64 {
    g.saturating_mul(u64::from(f)) >> 8
}

/// What one pass of the gain chain needs besides the light and clarity.
struct Chain<'a> {
    eye: &'a Eye<'a>,
    target: &'a Target,
    band: u16,
    distance: u16,
    crowd: u16,
    /// The cue rules that act (matching visible bits, within their range).
    rules: [bool; MAX_CUES],
}

impl Chain<'_> {
    /// The gain per credited tick: base gain, then band, distance, light,
    /// clarity, size, conspicuity, each acting cue rule's gain (0 skipped),
    /// crowd and notice scale, each as `g * f >> 8`.
    fn gain(&self, light: u16, clarity: u16) -> u64 {
        let t = self.target;
        let mut g = u64::from(self.eye.profile.base_gain);
        for f in [
            self.band,
            self.distance,
            light,
            clarity,
            t.size,
            t.conspicuity,
        ] {
            g = step(g, f);
        }
        for (rule, &acts) in self.eye.profile.cues.iter().zip(&self.rules) {
            if acts && rule.gain != 0 {
                g = step(g, rule.gain);
            }
        }
        g = step(g, self.crowd);
        step(g, self.eye.stats.notice_scale)
    }
}

/// What the senses found in one check (gains per credited tick).
#[derive(Default)]
pub(crate) struct Found {
    /// The better of the sight and thermal passes.
    pub(crate) vision: u64,
    /// Which of the two, and its clarity.
    pub(crate) sense: Sense,
    pub(crate) clarity: u16,
    /// The scanner's gain.
    pub(crate) scan: u64,
    /// The highest instant level that fired (0, 1, 2).
    pub(crate) instant: u8,
    /// The distance and the sight range (for see-through).
    pub(crate) distance: i64,
    pub(crate) range: i64,
}

/// Whether `rule` acts on `visible` bits of a target at `point`.
fn acts(world: &World, eye: &Eye, point: Pos, rule: &CueRule, visible: u32) -> bool {
    rule.bits & visible != 0
        && (rule.range == 0 || within(world, eye.point, point, i64::from(rule.range)))
}

/// Steps 2 to 6: geometry, lines, light and the gain chain for sight and
/// thermal, the scanner, and the instant cues.
pub(crate) fn sense(
    world: &World,
    map: &mut impl SenseMap,
    eye: &Eye,
    target: &Target,
    visible: u32,
    credit: u64,
    sink: &mut impl Sink<PerceptionTrace>,
) -> Found {
    let p = eye.profile;
    let id = target.id;
    let range = sight_range(p, eye.stats);
    let d = distance(world, eye.point, target.point);
    let mut found = Found {
        distance: d,
        range,
        ..Found::default()
    };
    let band = if within(world, eye.point, target.point, range) {
        band_of(world, eye, target.point)
    } else {
        None
    };
    if let Some((index, band)) = band {
        let mut chain = Chain {
            eye,
            target,
            band: band.gain,
            distance: distance_factor(p, d, range),
            crowd: p.crowd_gain[usize::from(target.crowd.min(7))],
            rules: [false; MAX_CUES],
        };
        for (slot, rule) in chain.rules.iter_mut().zip(&p.cues) {
            *slot = acts(world, eye, target.point, rule, visible);
        }
        let mut sight = 0;
        if p.senses & SIGHT != 0 {
            let line = sight_line_with(world, map, eye.point, target.point, false);
            emit_line(sink, id, &line);
            let light = if line.clear {
                light_factor(world, map, p, target)
            } else {
                0
            };
            if line.clear {
                sight = chain.gain(light, line.clarity);
            }
            emit_gain(
                sink,
                id,
                index,
                &chain,
                light,
                line.clarity,
                sight.saturating_mul(credit),
                Sense::Sight,
            );
            (found.vision, found.sense, found.clarity) = (sight, Sense::Sight, line.clarity);
            if band.instant && line.clear {
                for (i, (rule, &on)) in (0u8..).zip(p.cues.iter().zip(&chain.rules)) {
                    if on && rule.instant > 0 && line.clarity >= rule.min_clarity {
                        found.instant = found.instant.max(rule.instant.min(2));
                        sink.emit(PerceptionTrace::Instant {
                            target: id,
                            rule: i,
                        });
                    }
                }
            }
        }
        if p.senses & THERMAL != 0 {
            let line = sight_line_with(world, map, eye.point, target.point, true);
            emit_line(sink, id, &line);
            let heat = (u32::from(target.heat) * 256 / 255) as u16;
            let thermal = if line.clear {
                chain.gain(heat, line.clarity)
            } else {
                0
            };
            emit_gain(
                sink,
                id,
                index,
                &chain,
                heat,
                line.clarity,
                thermal.saturating_mul(credit),
                Sense::Thermal,
            );
            if thermal > sight {
                (found.vision, found.sense, found.clarity) =
                    (thermal, Sense::Thermal, line.clarity);
            }
        }
    }
    if p.senses & SCANNER != 0
        && p.scanner_range > 0
        && eye.point[2] == target.point[2]
        && within(world, eye.point, target.point, i64::from(p.scanner_range))
    {
        found.scan = u64::from(p.scanner_gain);
        sink.emit(PerceptionTrace::Gain {
            target: id,
            band: u8::MAX,
            distance: 256,
            light: 256,
            clarity: 256,
            crowd: 256,
            gain: fit_u32(found.scan.saturating_mul(credit)),
            sense: Sense::Scanner,
        });
    }
    found
}

fn fit_u32(v: u64) -> u32 {
    v.min(u64::from(u32::MAX)) as u32
}

fn emit_line(sink: &mut impl Sink<PerceptionTrace>, target: u32, line: &Line) {
    sink.emit(PerceptionTrace::Line {
        target,
        clear: line.clear,
        clarity: line.clarity,
        blocked_at: line.blocked_at,
    });
}

#[allow(clippy::too_many_arguments)]
fn emit_gain(
    sink: &mut impl Sink<PerceptionTrace>,
    target: u32,
    band: u8,
    chain: &Chain,
    light: u16,
    clarity: u16,
    gain: u64,
    sense: Sense,
) {
    sink.emit(PerceptionTrace::Gain {
        target,
        band,
        distance: chain.distance,
        light,
        clarity,
        crowd: chain.crowd,
        gain: fit_u32(gain),
        sense,
    });
}
