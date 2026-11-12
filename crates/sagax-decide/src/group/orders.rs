use super::squad::{Goal, PlanId, dist, plain_dist};
use crate::memory::{Metric, Pos, arrival};

/// Report kind ([`crate::memory::Message::kind`] for [`super::Squad::report`]):
/// the sender's goal broke. `sender` is the member's id, `data` the plan id
/// the goal belonged to.
pub const REPORT_GOAL_BROKEN: u32 = 1;
/// Report kind: an enemy was seen. `subject`, `pos`, `seen_at`, `strength`
/// and `data` become a fact of the squad's target kind in the leader's
/// knowledge (merged, source [`crate::memory::SOURCE_TOLD`]).
pub const REPORT_ENEMY: u32 = 2;
/// Report kind: the sender was shot at. `pos` is the centre of the danger
/// area; the shooter stood at x = `strength`, y = `data` (both read as
/// `i32`), z = `pos[2]`, which becomes the area's `source`; `subject` (who
/// shot, if known) becomes the area's `data`. Radius, lifetime, kind,
/// strength and blocking come from [`super::Squad::set_danger_area`].
pub const REPORT_DANGER: u32 = 3;

/// A goal on its way to one member (see [`super::Squad::orders_due`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Order {
    /// The member's id.
    pub member: u32,
    /// What it is told to do.
    pub goal: Goal,
}

/// How orders reach a member from the leader. Distances run from the
/// leader's position when the order is sent, in the squad's metric (for
/// [`Metric::Euclid`] the rounded-down root).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Arrives after one tick per unit of distance, only within `range`
    /// (inclusive; for Euclid the squared distance is compared with
    /// `range²`; a negative range reaches nobody).
    Shout {
        /// Farthest distance a shout carries.
        range: i64,
    },
    /// Arrives after `base + distance * per_unit` ticks, at any distance.
    Radio {
        /// Fixed delay.
        base: u32,
        /// Delay per unit of distance.
        per_unit: u32,
    },
    /// Never arrives.
    None,
}

impl Default for Channel {
    /// At once, at any distance (a radio without delay).
    fn default() -> Self {
        Self::Radio {
            base: 0,
            per_unit: 0,
        }
    }
}

impl Channel {
    /// When an order sent at `now` from `from` reaches `to`; `None` when it
    /// does not arrive at all.
    pub fn arrival(self, metric: Metric, from: Pos, to: Pos, now: u32) -> Option<u32> {
        let d = u32::try_from(plain_dist(metric, from, to)).unwrap_or(u32::MAX);
        match self {
            Self::Shout { range } => {
                if range < 0 {
                    return None;
                }
                let limit = if metric == Metric::Euclid {
                    i128::from(range) * i128::from(range)
                } else {
                    i128::from(range)
                };
                (dist(metric, from, to) <= limit).then(|| arrival(now, 0, d, 1))
            }
            Self::Radio { base, per_unit } => Some(arrival(now, base, d, per_unit)),
            Self::None => None,
        }
    }
}

/// What [`super::Squad::check`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Go on as before (also while a new plan is being thought out).
    Keep,
    /// A new plan was made (pending, see [`super::Squad::pending_plan`]).
    Replan(PlanId),
    /// The active plan's trigger fired the signal just now.
    Signal,
    /// No plan is active and none could be made.
    NoPlan,
}
