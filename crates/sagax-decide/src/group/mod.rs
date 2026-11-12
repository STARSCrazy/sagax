//! Group tactics: who is in a group, who can be gathered to join one, and
//! the plans it can follow.
//!
//! - [`Group`]: a bounded list of [`Member`]s that can be suspended
//!   (temporarily out of play) or marked gone.
//! - [`gather`]: picks the nearest free candidates of a faction around a
//!   place.
//! - [`Tactic`]: a plan as data, a list of [`Slot`]s plus scoring, abort
//!   rules and a [`Trigger`]; [`tactics`] holds the shipped ones.
//! - [`wish_point`] and [`pick_spot`]: where a slot would like to stand, and
//!   the candidate [`Spot`] it takes. Spots are always picked, never
//!   invented; without any free spot the wish point is returned unchecked.
//! - [`Squad`]: the leader with its own memory ([`crate::memory::Knowledge`]
//!   and [`crate::memory::Areas`]); [`Squad::plan`] scores every
//!   (tactic, target) pair, picks one by skill ([`SkillTable`]) and assigns
//!   members to slots as [`Goal`]s.
//! - Running a plan: it becomes active after the skill's think time and
//!   reaches the members as [`Order`]s over their [`Channel`];
//!   [`Squad::check`] replans on aborts or a clearly better plan and fires
//!   the trigger, [`Squad::signal`] starts the "on signal" goals, and
//!   members [`Squad::report`] back through the leader's inbox.
//!
//! Everything is integer only and deterministic. On equal distance the
//! lower id (or index) wins.

mod keeper;
mod members;
mod orders;
mod skill;
mod spots;
mod squad;
mod tactic;

#[cfg(test)]
mod tests;

pub use keeper::{GoalKeeper, discipline};
pub use members::{Group, Member, MemberState, Nearby, gather};
pub use orders::{
    Channel, Order, REPORT_ARRIVED, REPORT_DANGER, REPORT_ENEMY, REPORT_GOAL_BROKEN, Status,
};
pub use skill::SkillTable;
pub use spots::{Spot, SpotPick, heading, pick_spot, wish_point};
pub use squad::{
    Goal, IN_AREA, IN_EXTRA, IN_LOSSES, IN_OFF_PLAN, IN_PLAN_AGE, IN_RANGED, IN_TARGET_AGE,
    IN_TARGET_DIST, IN_TARGET_FRIEND_DIST, MAX_EXTRA_COLUMNS, Plan, PlanError, PlanId, ROLE_AREA,
    ROLE_RANGED, Squad,
};
pub use tactic::{
    Abort, GoalKind, Place, Shape, Side, Slot, Start, TAG_CHOKE, TAG_COVER, TAG_HIDDEN, TAG_HIGH,
    Tactic, Trigger, tactics,
};
