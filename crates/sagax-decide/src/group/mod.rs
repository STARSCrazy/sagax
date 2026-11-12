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
//!
//! Everything is integer only and deterministic. On equal distance the
//! lower id (or index) wins.

mod members;
mod spots;
mod tactic;

#[cfg(test)]
mod tests;

pub use members::{Group, Member, MemberState, Nearby, gather};
pub use spots::{Spot, SpotPick, heading, pick_spot, wish_point};
pub use tactic::{
    Abort, GoalKind, Place, Shape, Side, Slot, Start, TAG_CHOKE, TAG_COVER, TAG_HIDDEN, TAG_HIGH,
    Tactic, Trigger, tactics,
};
