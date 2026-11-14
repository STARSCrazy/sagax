//! The kit: the shipped tactics, tag and role bits and the default numbers.
//!
//! Everything here is built from the public parts of the core and could be
//! written by a game; the core never imports it (see the layout test).

mod defaults;
mod roles;
pub mod tactics;
mod tags;

pub use roles::{IN_AREA, IN_RANGED, ROLE_AREA, ROLE_RANGED};
pub use tactics::Shape;
pub use tags::{TAG_CHOKE, TAG_COVER, TAG_HIDDEN, TAG_HIGH};
