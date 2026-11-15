//! The per-pair value [`Awareness`] and one check: [`observe`], plus the
//! geometric [`in_sight`].

use sagax_core::{NoopSink, Pos, Sink};

use crate::gain::{band_of, sense, sight_range, within};
use crate::line::sight_line;
use crate::map::SenseMap;
use crate::percept::{Percept, PerceptKind, Sense, Target};
use crate::profile::{Eye, SenseProfile, Stats};
use crate::trace::PerceptionTrace;
use crate::world::{World, cell_of};

/// Flag: the target was in view at the last check.
pub(crate) const IN_VIEW: u8 = 1;
/// Flag: this observer saw through the target's disguise.
pub(crate) const SEEN_THROUGH: u8 = 2;
/// Flag: checked in the running `look` call (cleared before it returns).
pub(crate) const CHECKED: u8 = 4;

/// What one observer knows of one target: the evidence counter, the
/// counted stage and the timing. A plain value the game stores per pair
/// (start it zeroed); there is no other state.
///
/// Stages: 0 unseen, 1 noticed, 2 recognised.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Awareness {
    /// Accumulated evidence, `0 ..= profile.max`.
    pub(crate) counter: u16,
    /// See-through progress (only with see-through on).
    pub(crate) doubt: u16,
    /// The counted stage.
    pub(crate) stage: u8,
    /// The stage the counter reached, waiting for the reaction time
    /// (equal to `stage` when nothing waits).
    pub(crate) pending: u8,
    /// [`IN_VIEW`], [`SEEN_THROUGH`].
    pub(crate) flags: u8,
    pub(crate) _pad: u8,
    /// Time of the last check.
    pub(crate) last: u32,
    /// When the counter reached the waiting stage.
    pub(crate) pending_since: u32,
    /// Time of the last check with the target in view.
    pub(crate) last_seen: u32,
    /// Cue bits reported so far.
    pub(crate) cues: u32,
}

impl Awareness {
    /// The counted stage: 0 unseen, 1 noticed, 2 recognised.
    pub fn stage(&self) -> u8 {
        self.stage
    }

    /// The evidence counter.
    pub fn counter(&self) -> u16 {
        self.counter
    }

    /// Whether the target was in view at the last check.
    pub fn in_view(&self) -> bool {
        self.flags & IN_VIEW != 0
    }

    /// The time of the last check that saw the target by sight or thermal
    /// (0 if never; a scanner contact does not count).
    pub fn last_seen(&self) -> u32 {
        self.last_seen
    }

    /// Whether this observer saw through the target's disguise (only
    /// possible with see-through on; holds until [`reset`](Self::reset)).
    pub fn seen_through(&self) -> bool {
        self.flags & SEEN_THROUGH != 0
    }

    /// Back to zero: unseen, nothing pending, disguise holds again.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The stage rule: `pending` follows the counter (rising straight to
    /// the highest threshold reached, falling with hysteresis); a fall
    /// counts at once, a rise once `now - pending_since` reaches the
    /// current `reaction_ticks`. `pending_since` is set only when a rise
    /// starts waiting. Returns the counted stage before and after.
    pub(crate) fn settle(
        &mut self,
        now: u32,
        p: &SenseProfile,
        stats: Stats,
        target: u32,
        sink: &mut impl Sink<PerceptionTrace>,
    ) -> (u8, u8) {
        let c = self.counter;
        let reached = if c < p.notice_at {
            0
        } else if c < p.recognize_at {
            1
        } else {
            2
        };
        let from = self.stage;
        let was = self.pending.max(from);
        let mut pending = was;
        if reached > pending {
            pending = reached;
        } else {
            let th = [p.notice_at, p.recognize_at];
            while pending > 0 && c < th[usize::from(pending) - 1].saturating_sub(p.hysteresis) {
                pending -= 1;
            }
        }
        if pending <= from {
            self.stage = pending;
        } else {
            if was <= from {
                self.pending_since = now;
            }
            if now.wrapping_sub(self.pending_since) >= u32::from(stats.reaction_ticks) {
                self.stage = pending;
            }
        }
        self.pending = pending;
        if pending != was && pending > self.stage {
            sink.emit(PerceptionTrace::Stage {
                target,
                from: self.stage,
                to: pending,
                pending: true,
            });
        }
        if self.stage != from {
            sink.emit(PerceptionTrace::Stage {
                target,
                from,
                to: self.stage,
                pending: false,
            });
        }
        (from, self.stage)
    }

    /// A heard stimulus with `margin` left of the listener's `range`
    /// (`0 <= margin <= range`, `range > 0`): the counter gains
    /// `hear_gain * (margin * 256 / range) >> 8`, but only up to
    /// `notice_at`; a counter already at or above it is not changed.
    /// `last` and `last_seen` stay (hearing is not seeing, credit is a
    /// sight matter). Then the stage rule with this call's stats. Returns
    /// the counted stage before and after.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn raise_heard(
        &mut self,
        now: u32,
        p: &SenseProfile,
        stats: Stats,
        margin: i64,
        range: i64,
        target: u32,
        sink: &mut impl Sink<PerceptionTrace>,
    ) -> (u8, u8) {
        let notice = p.notice_at.min(p.max);
        if self.counter < notice && range > 0 {
            let share = (margin.clamp(0, range) * 256 / range) as u64;
            let gain = (u64::from(p.hear_gain) * share) >> 8;
            self.counter = (u64::from(self.counter) + gain).min(u64::from(notice)) as u16;
        }
        self.settle(now, p, stats, target, sink)
    }
}

/// One check of one observer–target pair; pushes at most
/// [`MAX_PERCEPTS_PER_CHECK`](crate::MAX_PERCEPTS_PER_CHECK) percepts to
/// `out`. [`observe_traced`] with [`NoopSink`].
pub fn observe(
    world: &World,
    map: &mut impl SenseMap,
    now: u32,
    eye: &Eye,
    target: &Target,
    aware: &mut Awareness,
    out: &mut Vec<Percept>,
) {
    observe_traced(world, map, now, eye, target, aware, out, &mut NoopSink);
}

/// One check of one observer–target pair, in the fixed steps:
///
/// 1. **Credit** `min(now - aware.last, check_interval)` (wrapping time);
///    skipped time is never paid back.
/// 2. **Geometry**: within `range * sight_scale / 256` and inside a band.
/// 3. **Line**: [`sight_line`]; opaque gives nothing.
/// 4. **Light** at the target's cell (or its glow; [`NIGHT`](crate::NIGHT) lifts it).
/// 5. **Gain**: `g = base_gain`, then `g = g * f >> 8` (saturating `u64`)
///    for band, distance, light, clarity, size, conspicuity, each acting
///    cue rule, crowd and `notice_scale`, then `g * credit`, added up to
///    `max`. [`THERMAL`](crate::THERMAL) runs the chain again with heat for light and
///    `heat_density` for clarity; the larger counts. [`SCANNER`](crate::SCANNER) adds
///    `scanner_gain * credit`, but only up to `notice_at`.
/// 6. **Instant** cues in a band that allows them, with enough clarity,
///    raise the counter to `notice_at` or `recognize_at`.
/// 7. **Decay** when sight and thermal saw nothing, once `hold` ticks
///    passed since the last sighting: `decay_per_tick * credit`; with a
///    scanner contact never below `notice_at`, so the scanner holds a
///    target at *noticed*.
/// 8. **Stages**: a fall counts at once, a rise after the current
///    `reaction_ticks` since it was reached.
/// 9. **Percepts**, in this order: `Noticed`, `Recognized`, `Lost` (only
///    once noticed), `Forgotten`, `SeenThrough`, `Cues`, and `Seen` last on
///    every check at stage noticed or higher with the target in view.
///    Cue bits and see-through need sight or thermal; a scanner contact
///    gives only the place.
///
/// A disguised target (`disguise > 0`, not seen through) is reported with
/// `disguised: true`, and the cue bits of the observer's
/// [`DisguiseRule::hides`](crate::DisguiseRule::hides) act nowhere and are
/// never reported. Without [`see_through`](crate::DisguiseRule) that never
/// changes. A direct `observe` never touches an `Attention`.
#[allow(clippy::too_many_arguments)]
pub fn observe_traced(
    world: &World,
    map: &mut impl SenseMap,
    now: u32,
    eye: &Eye,
    target: &Target,
    aware: &mut Awareness,
    out: &mut Vec<Percept>,
    sink: &mut impl Sink<PerceptionTrace>,
) {
    let p = eye.profile;
    let id = target.id;
    let credit = u64::from(
        now.wrapping_sub(aware.last)
            .min(u32::from(p.check_interval.max(1))),
    );
    aware.last = now;
    let mut disguised = target.disguise > 0 && !aware.seen_through();
    let mask = |disguised: bool| if disguised { eye.disguise.hides } else { 0 };
    let found = sense(
        world,
        map,
        eye,
        target,
        target.cues & !mask(disguised),
        credit,
        sink,
    );

    // Steps 5 to 7: the counter.
    let max = u64::from(p.max);
    let notice = u64::from(p.notice_at).min(max);
    let c = u64::from(aware.counter);
    let by_vision = c
        .saturating_add(found.vision.saturating_mul(credit))
        .min(max);
    let by_scan = if found.scan > 0 && c < notice {
        c.saturating_add(found.scan.saturating_mul(credit))
            .min(notice)
    } else {
        c.min(max)
    };
    let mut next = by_vision.max(by_scan);
    let instant = [0, notice, u64::from(p.recognize_at).min(max)][usize::from(found.instant)];
    next = next.max(instant);
    // Seen: by sight or thermal (or an instant cue). In view: seen, or a
    // scanner contact (a place, never what it is).
    let seen = found.vision > 0 || found.instant > 0;
    let in_view = seen || found.scan > 0;
    let (sense, clarity) = if by_scan > by_vision || !seen {
        (Sense::Scanner, 256)
    } else {
        (found.sense, found.clarity)
    };
    let (sense, clarity) = if in_view {
        (sense, clarity)
    } else {
        (Sense::Sight, 0)
    };
    if seen {
        aware.last_seen = now;
    } else if now.wrapping_sub(aware.last_seen) >= u32::from(p.hold) {
        // A scanner contact keeps it noticed, never more.
        let floor = if found.scan > 0 { next.min(notice) } else { 0 };
        next = next
            .saturating_sub(u64::from(p.decay_per_tick).saturating_mul(credit))
            .max(floor);
    }
    aware.counter = next as u16;

    // Step 8: the stages.
    let (from, to) = aware.settle(now, p, eye.stats, id, sink);

    // See-through: only with it on, while recognised and seen.
    let mut through = false;
    if let Some(st) = eye.disguise.see_through
        && disguised
        && from == 2
        && seen
    {
        let (d, range, near) = (found.distance, found.range, i64::from(st.near));
        let close = if d <= near {
            256
        } else if d >= range {
            0
        } else {
            (256 * (range - d) / (range - near)) as u64
        };
        let rate = (u64::from(st.rate) * u64::from(256 - u16::from(target.disguise))) >> 8;
        let grow = ((rate * close) >> 8).saturating_mul(credit);
        aware.doubt = u64::from(aware.doubt)
            .saturating_add(grow)
            .min(u64::from(u16::MAX)) as u16;
        sink.emit(PerceptionTrace::Doubt {
            target: id,
            doubt: aware.doubt,
        });
        if aware.doubt >= st.at {
            aware.flags |= SEEN_THROUGH;
            (through, disguised) = (true, false);
        }
    }

    // Step 9: the percepts.
    let was_in_view = aware.in_view();
    aware.flags = (aware.flags & !IN_VIEW) | if in_view { IN_VIEW } else { 0 };
    let shown = target.cues & !mask(disguised);
    let recognised = to == 2 && seen;
    let (point, cell) = if in_view {
        (target.point, cell_of(world, target.point).pos())
    } else {
        ([0; 3], [0; 3])
    };
    let mut push = |event| {
        out.push(Percept {
            observer: eye.id,
            subject: id,
            sense,
            event,
            stage: to,
            point,
            cell,
            placed: in_view,
            cues: if recognised { shown } else { 0 },
            disguised,
            clarity,
            margin: 0,
            arrive_at: 0,
            at: now,
        });
    };
    if from == 0 && to >= 1 {
        push(PerceptKind::Noticed);
    }
    if from < 2 && to == 2 {
        push(PerceptKind::Recognized);
    }
    if from >= 1 && was_in_view && !in_view {
        push(PerceptKind::Lost);
    }
    if from > 0 && to == 0 {
        push(PerceptKind::Forgotten);
        aware.cues = 0;
    }
    if through {
        push(PerceptKind::SeenThrough);
    }
    if recognised {
        let new = shown & !aware.cues;
        aware.cues |= shown;
        if new != 0 {
            push(PerceptKind::Cues { new });
        }
    }
    if to >= 1 && in_view {
        push(PerceptKind::Seen);
    }
}

/// Whether `eye` could see `point` now: within the sight range (with the
/// stats), inside a band, and a clear [`sight_line`]. No awareness, no
/// time, no light; for rule slots that ask "can this one see that spot?".
pub fn in_sight(world: &World, map: &mut impl SenseMap, eye: &Eye, point: Pos) -> bool {
    within(world, eye.point, point, sight_range(eye.profile, eye.stats))
        && band_of(world, eye, point).is_some()
        && sight_line(world, map, eye.point, point).clear
}

/// The cheap instant test of `look`: does the target show a visible cue
/// bit that one of the observer's instant rules names?
pub(crate) fn shows_instant(eye: &Eye, target: &Target, aware: &Awareness) -> bool {
    let hidden = if target.disguise > 0 && !aware.seen_through() {
        eye.disguise.hides
    } else {
        0
    };
    let visible = target.cues & !hidden;
    eye.profile
        .cues
        .iter()
        .any(|r| r.instant > 0 && r.bits & visible != 0)
}
