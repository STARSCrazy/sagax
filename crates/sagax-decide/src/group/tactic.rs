use crate::Consideration;
use crate::memory::Pos;

/// Tag bit the shipped tactics ask for: the spot gives cover.
pub const TAG_COVER: u32 = 1 << 0;
/// Tag bit the shipped tactics ask for: the spot is high ground.
pub const TAG_HIGH: u32 = 1 << 1;
/// Tag bit the shipped tactics ask for: the spot is hidden from view.
pub const TAG_HIDDEN: u32 = 1 << 2;
/// Tag bit the shipped tactics ask for: the spot is a choke point.
pub const TAG_CHOKE: u32 = 1 << 3;

/// A side of the target, seen from the group: the front faces the group
/// centre, the rear is the far side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Side {
    /// Between the target and the group.
    Front = 0,
    /// To the left, looking from the group centre at the target.
    Left = 1,
    /// To the right, looking from the group centre at the target.
    Right = 2,
    /// Behind the target, away from the group.
    Rear = 3,
}

/// How a slot's wish point is found (see [`super::wish_point`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// This offset from the leader, in the group's own frame (+x forward to
    /// the target, +y to the left), turned by the heading from the group
    /// centre to the target.
    Offset(Pos),
    /// This side of the target, [`Slot::dist`] away from it.
    Side(Side),
    /// Halfway between the target and its nearest friend.
    Between,
    /// A spot with these tag bits, near the group centre.
    Tagged(u32),
    /// The leader's position as given.
    Here,
}

impl Place {
    /// The tag bits this place itself asks for ([`Place::Tagged`]), else 0.
    /// The caller combines them with [`Slot::want_tags`] for
    /// [`super::pick_spot`].
    pub const fn tags(&self) -> u32 {
        match self {
            Self::Tagged(t) => *t,
            _ => 0,
        }
    }
}

/// What a member is told to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GoalKind {
    /// Go to the spot.
    Move,
    /// Go to the spot and stay there.
    Hold,
    /// Attack the target (from the spot, if any).
    Attack,
    /// Wait at the spot.
    Wait,
    /// Get away to the spot.
    Retreat,
    /// A kind the game defines.
    Custom(u32),
}

/// When a goal starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Start {
    /// At once.
    Now,
    /// This many ticks after the order arrives.
    AfterTicks(u32),
    /// When the group signal is given.
    OnSignal,
}

/// One role in a tactic, filled by `min ..= max` members.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Slot {
    /// Role bits a member must all have to fill the slot; 0 takes anyone.
    pub roles: u32,
    /// Fewest members the slot needs.
    pub min: u8,
    /// Most members the slot takes.
    pub max: u8,
    /// Where the members gather first (before the signal), if anywhere.
    pub gather: Option<Place>,
    /// Where the members go for their goal.
    pub place: Place,
    /// What they do there.
    pub kind: GoalKind,
    /// When they start.
    pub start: Start,
    /// Tag bits a spot should have (see [`super::pick_spot`]); 0 takes any.
    pub want_tags: u32,
    /// Distance from the target for [`Place::Side`] and [`Place::Between`]
    /// (without a friend), for both `gather` and `place`.
    pub dist: i32,
}

/// When the group signal fires by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// Never by itself.
    None,
    /// When every assigned member has arrived at its gather spot, or
    /// `max_wait` ticks after the gather orders went out, whichever comes
    /// first.
    AllArrived {
        /// Longest wait for stragglers.
        max_wait: u32,
    },
    /// When the target comes within this distance (in the metric; for
    /// Euclid the square is compared with the radius squared).
    TargetInRadius(i64),
    /// Only when the caller gives it.
    Manual,
}

/// A rule that ends a tactic: fires when `row[input] >= at_least`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Abort {
    /// Column of the scoring row.
    pub input: usize,
    /// Threshold, inclusive.
    pub at_least: i32,
}

/// A group plan as data: slots, scoring, abort rules and a trigger.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tactic {
    /// The roles, filled in this order.
    pub slots: Vec<Slot>,
    /// Factors over the scoring row, put together as a product.
    pub considerations: Vec<Consideration>,
    /// Weight of the tactic ([`crate::ONE`] = 1.0).
    pub weight: u32,
    /// Any one of these ends the tactic.
    pub aborts: Vec<Abort>,
    /// When the signal fires by itself.
    pub trigger: Trigger,
    /// Groups below this skill do not know the tactic.
    pub min_skill: u8,
    /// Scored once per known target; without one it is not a candidate.
    pub needs_target: bool,
}

/// The shape of [`tactics::formation`].
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

/// The shipped tactics, built only from the public parts. Their numbers
/// (weights, curve ends, thresholds, skills) are defaults to tune; a game
/// can change any field or build its own.
///
/// Skill levels cover three common squad levels: [`tactics::rush`] (skill
/// 0), [`tactics::formation`] (64) and [`tactics::flank`] (128) are
/// "everyone rushes", "advance together" and "front plus flanks that
/// gather, then attack at once".
///
/// The scoring rows they read are the Squad's built-in inputs
/// ([`IN_LOSSES`](crate::group::IN_LOSSES),
/// [`IN_OFF_PLAN`](crate::group::IN_OFF_PLAN),
/// [`IN_TARGET_DIST`](crate::group::IN_TARGET_DIST)).
///
/// Formation offsets are in the square frame (+x forward, +y left) also on
/// a hex map, where they are read as axial coordinates; the shapes then
/// look skewed, but members fill the same slots in the same order.
pub mod tactics {
    use super::super::squad::{IN_LOSSES, IN_OFF_PLAN, IN_TARGET_DIST};
    use super::*;
    use crate::{Curve, ONE};

    /// Default `max_wait` of [`flank`]'s gather phase, in ticks.
    pub const FLANK_MAX_WAIT: u32 = 170;
    /// Distance of [`fall_back`]'s retreat point behind the leader.
    pub const FALL_BACK_DIST: i32 = 8;
    /// Radius of [`ambush`]'s trigger.
    pub const AMBUSH_RADIUS: i64 = 6;

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
        }
    }

    /// Replan when this many members were lost or are off plan.
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
    /// side, `dist` from the target, then all attack at once on the signal
    /// ([`Trigger::AllArrived`] with [`FLANK_MAX_WAIT`]). Slots take 1 ..= 3
    /// members. Skill 128. A `dist` of 4 suits a grid of cells.
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

    /// Hold hidden spots near the group and attack when the target comes
    /// within [`AMBUSH_RADIUS`]. Better the farther the target still is
    /// (distance 0 ..= 100). Skill 128.
    pub fn ambush() -> Tactic {
        Tactic {
            slots: vec![Slot {
                start: Start::OnSignal,
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
    /// (preferring choke spots); the rest attack it. Which target is the
    /// most isolated is for the caller's row to score. Skill 192.
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
            considerations: Vec::new(),
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
}
