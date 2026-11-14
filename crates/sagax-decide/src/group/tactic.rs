use crate::Consideration;
use crate::memory::Pos;

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
    /// A point the game's rules choose
    /// ([`SquadRules::place`](super::SquadRules::place) gets the id; the
    /// core never reads it). Needs a rules object (see
    /// [`Squad::needs_rules`](super::Squad::needs_rules)); without one it is
    /// the leader's position, as [`Place::Here`].
    Custom(u32),
}

impl Place {
    /// The tag bits this place itself asks for ([`Place::Tagged`]), else 0
    /// (also for [`Place::Custom`]).
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
    /// (without a friend) of `place`. Also sets how far a spot may be from
    /// the wish point, in both phases (see
    /// [`Squad::set_spot_radius`](super::Squad::set_spot_radius)).
    pub dist: i32,
    /// Like `dist`, for the `gather` place: the shipped [`tactics::flank`](super::tactics::flank)
    /// gathers at twice the attack distance, out of the target's reach.
    pub gather_dist: i32,
}

/// When the group signal fires by itself. Once the signal was given, the
/// plan is no longer replaced by a better one; only its [`Abort`] rules end
/// it (see [`Squad::check`](super::Squad::check)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// Never by itself.
    None,
    /// When every assigned active member has arrived at its gather spot
    /// ([`Squad::arrived`](super::Squad::arrived) or a
    /// [`REPORT_ARRIVED`](super::REPORT_ARRIVED) that has come in), or
    /// `max_wait` ticks after the gather orders went out, whichever comes
    /// first; never while no assigned member is active.
    AllArrived {
        /// Longest wait for stragglers.
        max_wait: u32,
    },
    /// When the target comes within this distance (in the metric; for
    /// Euclid the square is compared with the radius squared).
    TargetInRadius(i64),
    /// Only when the caller gives it.
    Manual,
    /// When the game's rules say so
    /// ([`SquadRules::trigger`](super::SquadRules::trigger) gets the id; the
    /// core never reads it). Needs a rules object (see
    /// [`Squad::needs_rules`](super::Squad::needs_rules)); without one it
    /// never fires.
    Custom(u32),
}

/// How a scoring-row value is compared with a limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Compare {
    /// `value >= limit`.
    AtLeast,
    /// `value <= limit`.
    AtMost,
    /// `value == limit`.
    Equal,
    /// `value != limit`.
    NotEqual,
}

impl Compare {
    /// Whether `value` meets `limit` under this comparison.
    pub fn holds(self, value: i32, limit: i32) -> bool {
        match self {
            Compare::AtLeast => value >= limit,
            Compare::AtMost => value <= limit,
            Compare::Equal => value == limit,
            Compare::NotEqual => value != limit,
        }
    }
}

/// A rule that ends a tactic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Abort {
    /// Fires when `cmp` holds between `row[input]` and `value`; a column
    /// beyond the row never fires.
    When {
        /// Column of the scoring row.
        input: usize,
        /// The comparison.
        cmp: Compare,
        /// The limit, inclusive for `AtLeast` and `AtMost`.
        value: i32,
    },
    /// A rule decided by the game's rules
    /// ([`SquadRules::abort`](super::SquadRules::abort) gets the id and the
    /// row; the core never reads the id). Needs a rules object (see
    /// [`Squad::needs_rules`](super::Squad::needs_rules)); without one it
    /// never fires.
    Custom(u32),
}

impl Abort {
    /// Fires when `row[input] >= value`.
    pub const fn at_least(input: usize, value: i32) -> Abort {
        Abort::When {
            input,
            cmp: Compare::AtLeast,
            value,
        }
    }
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
    /// Whether the plan ends when its target is gone from the leader's
    /// knowledge. When false the active row is scored with the target's
    /// facts missing, like a plan without a target.
    pub abort_if_target_lost: bool,
}
