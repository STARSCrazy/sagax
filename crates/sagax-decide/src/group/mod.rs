//! Group tactics: who is in a group, and who can be gathered to join one.
//!
//! - [`Group`]: a bounded list of [`Member`]s that can be suspended
//!   (temporarily out of play) or marked gone.
//! - [`gather`]: picks the nearest free candidates of a faction around a
//!   place.
//!
//! Everything is integer only and deterministic. On equal distance the
//! lower id (or index) wins.

mod members;

#[cfg(test)]
mod tests;

pub use members::{Group, Member, MemberState, Nearby, gather};
