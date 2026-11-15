//! Limited attention: [`look`] checks a bounded number of targets per
//! call in a fixed order, and [`count_crowds`] for games without their own
//! crowd count.

use sagax_core::{NoopSink, Sink};

use crate::awareness::{Awareness, CHECKED, observe_traced, shows_instant};
use crate::map::SenseMap;
use crate::percept::{Percept, Target};
use crate::profile::Eye;
use crate::trace::PerceptionTrace;
use crate::world::World;

/// Where one observer's round robin goes on. A plain value the game
/// stores per observer (start it zeroed).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Attention {
    cursor: u32,
}

impl Attention {
    /// The index the next round robin starts from (taken modulo the
    /// number of targets).
    pub fn cursor(&self) -> u32 {
        self.cursor
    }
}

/// How many targets one [`look`] checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Budget {
    /// Checks for kept and round-robin targets together.
    pub checks: u16,
    /// Of `checks`, at most this many go to targets already noticed.
    pub keep: u16,
    /// Targets with an instant cue checked on top of `checks`.
    pub instant_cap: u16,
}

/// Why [`look`] checked a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Pick {
    /// It shows a cue with an instant rule; outside the budget.
    Instant = 0,
    /// It is already noticed or recognised.
    Kept = 1,
    /// Its turn in the round robin.
    RoundRobin = 2,
}

/// One observer looks at its candidate targets: [`look_traced`] with
/// [`NoopSink`].
#[allow(clippy::too_many_arguments)]
pub fn look(
    world: &World,
    map: &mut impl SenseMap,
    now: u32,
    eye: &Eye,
    targets: &[Target],
    aware: &mut [Awareness],
    attention: &mut Attention,
    budget: Budget,
    out: &mut Vec<Percept>,
) {
    look_traced(
        world,
        map,
        now,
        eye,
        targets,
        aware,
        attention,
        budget,
        out,
        &mut NoopSink,
    );
}

/// One observer looks at its candidate targets (`aware` parallel to
/// `targets` and of the same length: a debug build asserts it, a release
/// build uses only the pairs present in both) and checks each chosen
/// one with [`observe_traced`], in this order:
///
/// 1. **Instant**: targets showing a visible cue bit of an instant rule,
///    in index order, up to `instant_cap`, outside the budget.
/// 2. **Kept**: targets already noticed or recognised, highest stage
///    first, then lower index, up to `keep` (out of `checks`).
/// 3. **Round robin**: the rest from the cursor, wrapping, with the
///    checks left; the cursor moves past the last one checked.
///
/// Unchecked targets are not touched, so a target checked again gets only
/// one interval's credit: in a crowd, attention is really limited. At most
/// [`MAX_PERCEPTS_PER_CHECK`](crate::MAX_PERCEPTS_PER_CHECK) percepts per
/// checked target go to `out`. The trace has a `Picked` per check and,
/// when the sink is enabled, a `Skipped` per target left out.
#[allow(clippy::too_many_arguments)]
pub fn look_traced<S: Sink<PerceptionTrace>>(
    world: &World,
    map: &mut impl SenseMap,
    now: u32,
    eye: &Eye,
    targets: &[Target],
    aware: &mut [Awareness],
    attention: &mut Attention,
    budget: Budget,
    out: &mut Vec<Percept>,
    sink: &mut S,
) {
    debug_assert_eq!(
        targets.len(),
        aware.len(),
        "look: aware must be parallel to targets"
    );
    let n = targets.len().min(aware.len());
    let (targets, aware) = (&targets[..n], &mut aware[..n]);
    for a in aware.iter_mut() {
        a.flags &= !CHECKED;
    }
    let mut check = |i: usize, why: Pick, aware: &mut [Awareness], sink: &mut S| {
        sink.emit(PerceptionTrace::Picked {
            target: targets[i].id,
            why,
        });
        observe_traced(world, map, now, eye, &targets[i], &mut aware[i], out, sink);
        aware[i].flags |= CHECKED;
    };

    let mut instant = 0;
    for i in 0..n {
        if instant >= budget.instant_cap {
            break;
        }
        if shows_instant(eye, &targets[i], &aware[i]) {
            instant += 1;
            check(i, Pick::Instant, aware, &mut *sink);
        }
    }

    let mut left = budget.checks;
    let mut keep = budget.keep.min(left);
    for stage in [2, 1] {
        for i in 0..n {
            if keep == 0 {
                break;
            }
            if aware[i].flags & CHECKED == 0 && aware[i].stage == stage {
                (keep, left) = (keep - 1, left - 1);
                check(i, Pick::Kept, aware, &mut *sink);
            }
        }
    }

    if n > 0 {
        let start = attention.cursor as usize % n;
        let mut last = None;
        for k in 0..n {
            let i = (start + k) % n;
            if aware[i].flags & CHECKED != 0 {
                continue;
            }
            if left > 0 {
                left -= 1;
                check(i, Pick::RoundRobin, aware, &mut *sink);
                last = Some(i);
            } else if sink.enabled() {
                sink.emit(PerceptionTrace::Skipped {
                    target: targets[i].id,
                });
            }
        }
        if let Some(i) = last {
            attention.cursor = ((i + 1) % n) as u32;
        }
    }
    for a in aware.iter_mut() {
        a.flags &= !CHECKED;
    }
}

/// For each target, how many others of the same group (`group_of`) stand
/// within `radius` fine units on the same level (Euclidean on the point's
/// `x`, `y`: on hex that is axial Euclid, approximate; saturating at 255), written to `out[i]`. Only the first
/// `min(targets.len(), out.len())` are written. O(n²): for a game without
/// its own crowd count.
pub fn count_crowds(
    targets: &[Target],
    radius: i32,
    group_of: impl Fn(&Target) -> u32,
    out: &mut [u8],
) {
    let r = i128::from(radius.max(0));
    for (i, (slot, a)) in out.iter_mut().zip(targets).enumerate() {
        let group = group_of(a);
        let count = targets
            .iter()
            .enumerate()
            .filter(|&(j, b)| {
                let dx = i128::from(b.point[0]) - i128::from(a.point[0]);
                let dy = i128::from(b.point[1]) - i128::from(a.point[1]);
                j != i
                    && b.point[2] == a.point[2]
                    && dx * dx + dy * dy <= r * r
                    && group_of(b) == group
            })
            .count();
        *slot = count.min(255) as u8;
    }
}
