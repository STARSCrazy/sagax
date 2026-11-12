//! World memory: what a character knows, and for how long.
//!
//! [`Knowledge`] is a bounded table of [`Fact`]s keyed by subject and kind.
//! [`Timers`] are ids that count as "on" for a while (ignore a target, no
//! alarm again), [`Progress`] tells whether a chase still gets closer, and
//! [`Interval`] is a small timer for "do this every N ticks". [`Zones`] are
//! areas to stay out of, [`Outbox`] and [`Inbox`] carry messages (recipients are picked
//! with [`route`]),
//! [`search_point`] spreads search points around a place and [`look_heading`]
//! turns the view while looking around.
//!
//! Time is the game's own count (u32); ages are computed with wrapping
//! subtraction, so a wrapped clock still works. Everything is integer only
//! and deterministic.

mod interval;
mod knowledge;
mod messages;
mod progress;
mod route;
mod search;
mod timers;
mod zones;

#[cfg(test)]
mod tests;

pub use interval::Interval;
pub use knowledge::{Evict, Fact, Knowledge, SOURCE_HEARD, SOURCE_OWN, SOURCE_TOLD};
pub use messages::{Inbox, Message, Outbox, Refused, arrival};
pub use progress::Progress;
pub use route::{Metric, Route, distance, route};
pub use search::{LOOK_OFFSETS, SearchPattern, look_heading, search_point, sin256};
pub use timers::Timers;
pub use zones::{Zone, Zones};

/// A place: three integer coordinates in the game's own units.
pub type Pos = [i32; 3];

/// A memory was asked for with capacity 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ZeroCapacity;

impl std::fmt::Display for ZeroCapacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "capacity must be at least 1")
    }
}

impl std::error::Error for ZeroCapacity {}
