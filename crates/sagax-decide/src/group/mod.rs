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
//!   members [`Squad::report`] back through the leader's inbox (and
//!   [`Squad::report_danger`] for shots).
//! - [`rules`]: the game's own logic at fixed points of a plan or check
//!   ([`SquadRules`], lent per call to [`Squad::plan_with`] and
//!   [`Squad::check_with`]).
//! - Walking distance: a [`SquadRules::walk`] (for example [`Walking`])
//!   makes every "how far does a figure have to walk" question ask the
//!   game's [`crate::memory::Distance`] instead of the straight line; see
//!   the [`rules`].
//! - [`Crew`]: a squad with one [`GoalKeeper`] per member, run as one call
//!   per tick ([`Crew::tick`]) that reports [`CrewEvent`]s; every step is
//!   also a public squad or keeper call.
//!
//! Everything is integer only and deterministic. On equal distance the
//! lower id (or index) wins.

mod crew;
pub mod keeper;
mod kit;
mod members;
mod orders;
pub mod rules;
mod skill;
mod spots;
mod squad;
mod tactic;

#[cfg(test)]
mod crew_tests;
#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod rules_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod walk_tests;

pub use crew::{Crew, CrewConfig, CrewError, CrewEvent, CrewTrace};
pub use keeper::{GOAL_OPTION_RANGE, GoalKeeper, KeeperInputs, KeeperTrace};
pub use kit::{
    IN_AREA, IN_RANGED, ROLE_AREA, ROLE_RANGED, Shape, TAG_CHOKE, TAG_COVER, TAG_HIDDEN, TAG_HIGH,
    tactics,
};
pub use members::{Group, Member, MemberState, Nearby, gather};
pub use orders::{
    Channel, Order, REPORT_ARRIVED, REPORT_ENEMY, REPORT_GAME_FIRST, REPORT_GOAL_BROKEN, Status,
};
pub use rules::{
    BuiltInRules, ChannelCtx, CostCtx, MAX_REPORT_ACTIONS, PlaceCtx, PlanInfo, ReportAction,
    ReportActions, RowCtx, SpotCtx, SquadRules, SquadView, UNREACHABLE, Walking,
    spot_score_builtin,
};
pub use skill::{MAX_SKILL_ROWS, SkillTable};
pub use spots::{Spot, SpotPick, SpotTrace, heading, pick_spot, pick_spot_traced, wish_point};
#[allow(deprecated)]
pub use squad::IN_EXTRA;
pub use squad::{
    Goal, IN_LOSSES, IN_OFF_PLAN, IN_PLAN_AGE, IN_ROLE_0, IN_ROLE_1, IN_TARGET_AGE, IN_TARGET_DIST,
    IN_TARGET_FRIEND_DIST, MAX_EXTRA_COLUMNS, MAX_ROLE_COUNTS, Plan, PlanError, PlanId, RoleCounts,
    Squad, SquadConfig, SquadTrace,
};
pub use tactic::{Abort, Compare, GoalKind, Place, Side, Slot, Start, Tactic, Trigger};
