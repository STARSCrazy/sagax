//! The shipped tactics, built only from the public parts.
//!
//! Their numbers (weights, curve ends, thresholds, skills) are defaults to
//! tune; a game can change any field or build its own.
//!
//! Skill levels cover three common squad levels: [`rush`] (skill 0),
//! [`formation`] (64) and [`flank`] (128) are "everyone rushes", "advance
//! together" and "front plus flanks that gather, then attack at once".
//!
//! The scoring rows they read are the Squad's built-in inputs
//! ([`IN_LOSSES`], [`IN_OFF_PLAN`], [`IN_TARGET_AGE`], [`IN_TARGET_DIST`],
//! [`IN_TARGET_FRIEND_DIST`]).
//!
//! Formation offsets are in the square frame (+x forward, +y left) also on
//! a hex map, where they are read as axial coordinates; the shapes then
//! look skewed, but members fill the same slots in the same order.

use super::super::squad::{
    IN_LOSSES, IN_OFF_PLAN, IN_TARGET_AGE, IN_TARGET_DIST, IN_TARGET_FRIEND_DIST,
};
use super::super::tactic::{Abort, GoalKind, Place, Side, Slot, Start, Tactic, Trigger};
use super::tags::{TAG_CHOKE, TAG_COVER, TAG_HIDDEN};
use crate::{Consideration, Curve, ONE};

/// The shape of [`formation`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Shape {
    /// Side by side, across the heading.
    Line = 0,
    /// A point at the leader, two arms trailing back.
    Wedge = 1,
    /// One behind the other.
    Column = 2,
}

/// Default `max_wait` of [`flank`]'s gather phase, in ticks.
pub const FLANK_MAX_WAIT: u32 = 170;
/// Distance of [`fall_back`]'s retreat point behind the leader.
pub const FALL_BACK_DIST: i32 = 8;
/// Radius of [`ambush`]'s trigger.
pub const AMBUSH_RADIUS: i64 = 6;
/// Ticks without a sighting after which the shipped tactics (all but
/// [`rush`] and [`fall_back`]) give their target up.
pub const TARGET_LOST_TICKS: i32 = 255;

fn slot(place: Place, kind: GoalKind) -> Slot {
    Slot {
        roles: 0,
        min: 1,
        max: u8::MAX,
        gather: None,
        place,
        kind,
        start: Start::Now,
        want_tags: 0,
        dist: 1,
        gather_dist: 2,
    }
}

/// Replan when two members were lost or are off plan, or the target was
/// not seen for [`TARGET_LOST_TICKS`].
fn usual_aborts() -> Vec<Abort> {
    vec![
        Abort {
            input: IN_LOSSES,
            at_least: 2,
        },
        Abort {
            input: IN_OFF_PLAN,
            at_least: 2,
        },
        Abort {
            input: IN_TARGET_AGE,
            at_least: TARGET_LOST_TICKS,
        },
    ]
}

/// Everyone attacks the target at once, from its front. Skill 0, no
/// abort; the plain fallback with a low weight.
pub fn rush() -> Tactic {
    Tactic {
        slots: vec![slot(Place::Side(Side::Front), GoalKind::Attack)],
        considerations: Vec::new(),
        weight: ONE / 4,
        aborts: Vec::new(),
        trigger: Trigger::None,
        min_skill: 0,
        needs_target: true,
    }
}

/// Advance together in `shape`, members `spacing` apart, facing the
/// target. Five slots of one member; the first (the leader's place,
/// offset 0) is needed, the others are filled as members allow. Skill
/// 64. A `spacing` of 2 suits a grid of cells.
pub fn formation(shape: Shape, spacing: i32) -> Tactic {
    let s = spacing;
    let offsets: [(i32, i32); 5] = match shape {
        Shape::Line => [(0, 0), (0, s), (0, -s), (0, 2 * s), (0, -2 * s)],
        Shape::Wedge => [(0, 0), (-s, s), (-s, -s), (-2 * s, 2 * s), (-2 * s, -2 * s)],
        Shape::Column => [(0, 0), (-s, 0), (-2 * s, 0), (-3 * s, 0), (-4 * s, 0)],
    };
    let slots = offsets
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| Slot {
            min: u8::from(i == 0),
            max: 1,
            ..slot(Place::Offset([x, y, 0]), GoalKind::Move)
        })
        .collect();
    Tactic {
        slots,
        considerations: Vec::new(),
        weight: ONE / 2,
        aborts: usual_aborts(),
        trigger: Trigger::None,
        min_skill: 64,
        needs_target: true,
    }
}

/// Come from `sides` sides of the target (clamped to 2 ..= 3): 2 is
/// left and right, 3 adds the front. Each slot first gathers at its
/// side, `2 * dist` from the target, then all attack at once on the
/// signal from `dist` ([`Trigger::AllArrived`] with
/// [`FLANK_MAX_WAIT`]). Slots take 1 ..= 3 members. Skill 128. A `dist`
/// of 4 suits a grid of cells.
pub fn flank(sides: u8, dist: i32) -> Tactic {
    let chosen: &[Side] = if sides >= 3 {
        &[Side::Front, Side::Left, Side::Right]
    } else {
        &[Side::Left, Side::Right]
    };
    let slots = chosen
        .iter()
        .map(|&side| Slot {
            max: 3,
            gather: Some(Place::Side(side)),
            start: Start::OnSignal,
            dist,
            gather_dist: dist.saturating_mul(2),
            ..slot(Place::Side(side), GoalKind::Attack)
        })
        .collect();
    Tactic {
        slots,
        considerations: Vec::new(),
        weight: ONE * 3 / 4,
        aborts: usual_aborts(),
        trigger: Trigger::AllArrived {
            max_wait: FLANK_MAX_WAIT,
        },
        min_skill: 128,
        needs_target: true,
    }
}

/// Hold hidden spots near the group (within 12 of its centre: `dist` 4)
/// and attack when the target comes within [`AMBUSH_RADIUS`]. Better
/// the farther the target still is (distance 0 ..= 100). Skill 128.
pub fn ambush() -> Tactic {
    Tactic {
        slots: vec![Slot {
            start: Start::OnSignal,
            dist: 4,
            ..slot(Place::Tagged(TAG_HIDDEN), GoalKind::Attack)
        }],
        considerations: vec![Consideration::new(
            IN_TARGET_DIST as u32,
            Curve::Linear,
            0,
            100,
        )],
        weight: ONE * 3 / 4,
        aborts: usual_aborts(),
        trigger: Trigger::TargetInRadius(AMBUSH_RADIUS),
        min_skill: 128,
        needs_target: true,
    }
}

/// All attack one target; nearer targets score higher (distance
/// 0 ..= 100). Skill 64.
pub fn focus() -> Tactic {
    Tactic {
        slots: vec![slot(Place::Side(Side::Front), GoalKind::Attack)],
        considerations: vec![Consideration::new(
            IN_TARGET_DIST as u32,
            Curve::Inverse,
            0,
            100,
        )],
        weight: ONE * 5 / 8,
        aborts: usual_aborts(),
        trigger: Trigger::None,
        min_skill: 64,
        needs_target: true,
    }
}

/// One or two members hold between the target and its nearest friend
/// (preferring choke spots); the rest attack it. The more isolated the
/// target (its distance to its nearest friend, 0 ..= 32), the better;
/// a target without a known friend scores 0. Skill 192.
pub fn isolate() -> Tactic {
    Tactic {
        slots: vec![
            Slot {
                max: 2,
                want_tags: TAG_CHOKE,
                dist: 2,
                ..slot(Place::Between, GoalKind::Hold)
            },
            slot(Place::Side(Side::Front), GoalKind::Attack),
        ],
        considerations: vec![Consideration::new(
            IN_TARGET_FRIEND_DIST as u32,
            Curve::Linear,
            0,
            32,
        )],
        weight: ONE * 3 / 4,
        aborts: usual_aborts(),
        trigger: Trigger::None,
        min_skill: 192,
        needs_target: true,
    }
}

/// Everyone retreats [`FALL_BACK_DIST`] behind the leader, preferring
/// cover. Needs no target: it is scored once and moves away from the
/// nearest known target, else the nearest danger area; with neither the
/// members stay at the leader. Scores 0 without losses and full at 4.
/// Skill 0.
pub fn fall_back() -> Tactic {
    Tactic {
        slots: vec![Slot {
            want_tags: TAG_COVER,
            ..slot(Place::Offset([-FALL_BACK_DIST, 0, 0]), GoalKind::Retreat)
        }],
        considerations: vec![Consideration::new(IN_LOSSES as u32, Curve::Linear, 0, 4)],
        weight: ONE,
        aborts: Vec::new(),
        trigger: Trigger::None,
        min_skill: 0,
        needs_target: false,
    }
}
