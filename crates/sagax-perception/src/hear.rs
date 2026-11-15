//! Hearing: a [`Stimulus`] and one check per listener, [`hear`].

use sagax_core::{Cell, NoopSink, Pos, Sink, mix};

use crate::awareness::Awareness;
use crate::line::sound_cost;
use crate::map::SenseMap;
use crate::percept::{Percept, PerceptKind, Sense};
use crate::profile::Eye;
use crate::trace::PerceptionTrace;
use crate::world::{Grid, Spread, World, cell_centre, cell_of, distance, fit};

/// Something that can be heard (or smelt): a sound, a shout, a shot, smoke.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Stimulus {
    /// The game's number for this stimulus (a seed input of the fuzz).
    pub id: u32,
    /// Who or what made it (becomes `Percept::subject`, then
    /// `Fact::subject`).
    pub source: u32,
    /// The game's kind (gunshot, step, voice, smoke).
    pub kind: u32,
    /// Where, fine point.
    pub point: Pos,
    /// The audible range in fine units for a normal listener.
    pub loudness: i32,
    /// The channel, `0 .. MAX_CHANNELS` (an index into
    /// [`World::channels`]).
    pub channel: u8,
    /// When it was made.
    pub at: u32,
}

/// One hearing check of one listener; [`hear_traced`] with [`NoopSink`].
#[allow(clippy::too_many_arguments)]
pub fn hear(
    world: &World,
    map: &mut impl SenseMap,
    now: u32,
    ear: &Eye,
    stimulus: &Stimulus,
    cost: Option<i32>,
    aware: Option<&mut Awareness>,
    out: &mut Vec<Percept>,
    seed: u64,
) -> bool {
    hear_traced(
        world,
        map,
        now,
        ear,
        stimulus,
        cost,
        aware,
        out,
        seed,
        &mut NoopSink,
    )
}

/// One hearing check of one listener. Returns whether it was heard.
///
/// * **Cost**: `cost` if the game passes one (a flood around corners,
///   negative counts as 0); else by the channel's [`Spread`]:
///   [`Circle`](Spread::Circle) is the straight distance (rounded up on
///   square, so the test is exact; levels ignored),
///   [`Straight`](Spread::Straight) adds the map's damping along one walk
///   and is not heard when an edge lets nothing through or the levels
///   differ.
/// * **Heard** when `range > 0` and `cost <= range`, with
///   `range = loudness * hearing[channel] * hearing_scale / 65536`. A
///   channel past [`MAX_CHANNELS`](crate::MAX_CHANNELS) is never heard.
/// * Then a `Heard` percept is always pushed: `subject = source`,
///   `sense = Hearing(channel)`, `margin = range - cost`,
///   `arrive_at = at + cost / speed` (`at` with speed 0), the fuzzed
///   cell and its centre as the point (`placed`).
/// * **Fuzz** in whole cells: the source's cell moves by at most
///   `r = cost * fuzz / (256 * S)` cells. With
///   `h = mix(seed ^ ear.id << 32 ^ stimulus.id)` and `m = 2r + 1`:
///   `dx = h % m - r`, `dy = (h >> 16) % m - r` (on hex `dq`, `dr`; a pair
///   farther than `r` hexes is redrawn from `h = mix(h)` up to 4 times,
///   else no offset). Same inputs, same cell; `r = 0` is the true cell.
/// * **With an `Awareness`** (this listener and `stimulus.source`): the
///   counter gains `hear_gain * (margin * 256 / range) >> 8`, at most up
///   to `notice_at` (a counter at or above it stays); `last` and
///   `last_seen` stay. The stage rule then runs with `ear.stats`, so a
///   sound makes a target *noticed* after the reaction time, never
///   *recognised*. The stage percepts follow the `Heard` one, placed at
///   the heard cell: `Noticed` when the stage rose; also `Recognized` when
///   a recognition by sight, still waiting for the reaction time, becomes
///   due in this call, and `Forgotten` on a fall to unseen (unplaced).
///
/// Hearing has no cone. Trace: `Heard` or `Unheard`, then the `Stage`
/// records.
#[allow(clippy::too_many_arguments)]
pub fn hear_traced(
    world: &World,
    map: &mut impl SenseMap,
    now: u32,
    ear: &Eye,
    stimulus: &Stimulus,
    cost: Option<i32>,
    aware: Option<&mut Awareness>,
    out: &mut Vec<Percept>,
    seed: u64,
    sink: &mut impl Sink<PerceptionTrace>,
) -> bool {
    let ch = stimulus.channel;
    let scale = ear.profile.hearing.get(usize::from(ch)).copied();
    let range = i128::from(stimulus.loudness.max(0))
        * i128::from(scale.unwrap_or(0))
        * i128::from(ear.stats.hearing_scale)
        / 65536;
    let range = range.min(i128::from(i32::MAX)) as i32;
    let channel = world.channels.get(usize::from(ch));
    let cost = match (channel, cost) {
        (None, _) => None,
        (Some(_), Some(c)) => Some(c.max(0)),
        (Some(c), None) => match c.spread {
            Spread::Circle => Some(circle_cost(world, stimulus.point, ear.point)),
            Spread::Straight => sound_cost(world, map, stimulus.point, ear.point, c.damping),
        },
    };
    let (Some(channel), Some(cost)) = (channel, cost) else {
        sink.emit(PerceptionTrace::Unheard {
            stimulus: stimulus.id,
            cost,
            range,
        });
        return false;
    };
    if range <= 0 || cost > range {
        sink.emit(PerceptionTrace::Unheard {
            stimulus: stimulus.id,
            cost: Some(cost),
            range,
        });
        return false;
    }

    let cell = fuzzed(world, ear.id, stimulus, cost, channel.fuzz, seed);
    sink.emit(PerceptionTrace::Heard {
        stimulus: stimulus.id,
        cost,
        range,
        cell: cell.pos(),
    });
    let margin = range - cost;
    let arrive_at = if channel.speed > 0 {
        stimulus.at.wrapping_add((cost / channel.speed) as u32)
    } else {
        stimulus.at
    };
    let (from, to) = match aware {
        Some(a) => {
            let stages = a.raise_heard(
                now,
                ear.profile,
                ear.stats,
                i64::from(margin),
                i64::from(range),
                stimulus.source,
                sink,
            );
            if stages.0 > 0 && stages.1 == 0 {
                a.cues = 0;
            }
            stages
        }
        None => (0, 0),
    };
    let point = cell_centre(world, cell);
    let mut push = |event, placed: bool| {
        out.push(Percept {
            observer: ear.id,
            subject: stimulus.source,
            sense: Sense::Hearing(ch),
            event,
            stage: to,
            point: if placed { point } else { [0; 3] },
            cell: if placed { cell.pos() } else { [0; 3] },
            placed,
            cues: 0,
            disguised: false,
            clarity: 0,
            margin,
            arrive_at,
            at: now,
        });
    };
    push(PerceptKind::Heard, true);
    if from == 0 && to >= 1 {
        push(PerceptKind::Noticed, true);
    }
    if from < 2 && to == 2 {
        push(PerceptKind::Recognized, true);
    }
    if from > 0 && to == 0 {
        push(PerceptKind::Forgotten, false);
    }
    true
}

/// The straight distance in fine units, levels ignored: on square the
/// Euclidean length rounded up (so `cost <= range` is exactly
/// `d² <= range²`), on hex the hex distance of the fine points.
fn circle_cost(world: &World, a: Pos, b: Pos) -> i32 {
    let d = match world.grid {
        Grid::Square => {
            let dx = i128::from(b[0]) - i128::from(a[0]);
            let dy = i128::from(b[1]) - i128::from(a[1]);
            let sq = (dx * dx + dy * dy) as u128;
            let root = sq.isqrt();
            if root * root < sq { root + 1 } else { root }
        }
        Grid::Hex => distance(world, a, b).unsigned_abs().into(),
    };
    d.min(i32::MAX as u128) as i32
}

/// The source's cell moved by the deterministic fuzz (see [`hear_traced`]).
fn fuzzed(world: &World, ear: u32, stimulus: &Stimulus, cost: i32, fuzz: u16, seed: u64) -> Cell {
    let truth = cell_of(world, stimulus.point);
    let r = i64::from(cost) * i64::from(fuzz) / (256 * world.size());
    let m = (2 * r + 1) as u64;
    let draw = |h: u64| ((h % m) as i64 - r, ((h >> 16) % m) as i64 - r);
    let mut h = mix(seed ^ (u64::from(ear) << 32) ^ u64::from(stimulus.id));
    let mut d = draw(h);
    if world.grid == Grid::Hex {
        let rings = |(dq, dr): (i64, i64)| (dq.abs() + dr.abs() + (dq + dr).abs()) / 2;
        let mut redraws = 0;
        while rings(d) > r {
            if redraws == 4 {
                d = (0, 0);
                break;
            }
            h = mix(h);
            d = draw(h);
            redraws += 1;
        }
    }
    Cell::new(
        fit(i64::from(truth.x) + d.0),
        fit(i64::from(truth.y) + d.1),
        truth.level,
    )
}
