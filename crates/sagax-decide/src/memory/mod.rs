//! World memory: what a character knows, and for how long.
//!
//! The blocks, each usable alone:
//!
//! - [`Knowledge`]: a bounded table of [`Fact`]s keyed by subject and kind.
//! - [`Outbox`], [`Inbox`] and [`route`]: messages with a delay and a
//!   cooldown; [`route`] picks the recipients and [`arrival`] the time a
//!   message reaches one.
//! - [`Areas`]: places to stay out of or pay extra for.
//! - [`Progress`]: whether a chase still gets closer.
//! - [`Timers`]: ids that count as "on" for a while (ignore a target, no
//!   alarm again).
//! - [`Interval`]: "do this every N ticks".
//! - [`Alertness`]: a value with levels that rises and decays.
//! - [`GoalGuess`]: where the one I watch is heading, from the steps I saw
//!   (Bayes over candidate goals).
//! - [`points_around`] and [`sweep_heading`]: spread points around a place
//!   and turn the view while looking around.
//!
//! The tables that store, drop or hand out something ([`Knowledge`],
//! [`Areas`], [`Timers`], [`Outbox`], [`Inbox`]) have a `*_traced` form of
//! each such call that tells what happened ([`KnowledgeTrace`],
//! [`AreasTrace`], [`TimersTrace`], [`MessageTrace`]), and so has
//! [`GoalGuess`] for its updates ([`GoalGuessTrace`]); the rest are plain
//! values whose results already say everything.
//!
//! Time is the game's own count (u32); ages are computed with wrapping
//! subtraction, so a wrapped clock still works. Everything is integer only
//! and deterministic.

mod alertness;
mod areas;
mod geometry;
mod goal_guess;
mod interval;
mod knowledge;
mod messages;
mod progress;
mod route;
mod timers;

#[cfg(test)]
mod goal_guess_tests;
#[cfg(test)]
mod tests;

pub use alertness::{AlertConfig, Alertness};
pub use areas::{Area, Areas, AreasTrace};
pub use geometry::{LOOK_OFFSETS, PointPattern, Sine, points_around, sine, sweep_heading};
pub use goal_guess::{
    GOAL_COLUMNS, GoalGuess, GoalGuessConfig, GoalGuessTrace, MAX_GOALS, MAX_MOVES, StepError,
};
pub use interval::Interval;
pub use knowledge::{
    Evict, Fact, Knowledge, KnowledgeTrace, SOURCE_HEARD, SOURCE_OWN, SOURCE_TOLD,
};
pub use messages::{Inbox, Message, MessageTrace, Outbox, Refused, arrival};
pub use progress::Progress;
pub use route::{Metric, Route, distance, route};
pub use timers::{Timers, TimersTrace};

pub use sagax_core::Pos;

/// A memory was asked for with capacity 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ZeroCapacity;

impl std::fmt::Display for ZeroCapacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "capacity must be at least 1")
    }
}

impl std::error::Error for ZeroCapacity {}
