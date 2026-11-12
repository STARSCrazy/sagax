//! World memory: what a character has seen and what it chooses to ignore.
//!
//! [`Sightings`] remembers the last known place and time of things (by id),
//! with a fixed capacity. [`Ignore`] remembers ids that are to be left alone
//! for a while. [`Interval`] is a small timer for "do this every N ticks".
//! [`Zones`] are areas to stay out of, [`Pursuit`] tracks a chase,
//! [`Radio`] is a call for help with a cooldown, and [`nearest`] picks the
//! closest candidates. [`search_point`] spreads search points around a place
//! and [`look_heading`] turns the view while looking around.
//!
//! Time is the game's own count (u32); ages are computed with wrapping
//! subtraction, so a wrapped clock still works. Everything is integer only
//! and deterministic.

mod ignore;
mod pursuit;
mod radio;
mod search;
mod sightings;
mod zones;

#[cfg(test)]
mod tests;

pub use ignore::{Ignore, Interval};
pub use pursuit::{Pursuit, nearest};
pub use radio::Radio;
pub use search::{LOOK_OFFSETS, SearchPattern, look_heading, search_point, sin256};
pub use sightings::{Sighting, Sightings};
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
