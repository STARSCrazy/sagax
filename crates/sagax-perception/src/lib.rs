//! Perception for game AI: what a character can see, integer-exact and
//! deterministic.
//!
//! This first part is the geometry:
//!
//! * [`World`]: square or hex, the cell size `S` in fine units, and the
//!   hearing [`Channel`]s. Positions inside the block are fine points
//!   ([`Pos`](sagax_core::Pos) `[x, y, level]`); [`cell_centre`] and
//!   [`cell_of`] convert to and from whole cells, which every edge to the
//!   rest of the library uses.
//! * [`SenseMap`]: the game's answers per cell and edge (walls, fog, light,
//!   sound), separate from movement, with [`OpenAir`] and the [`Lit`]
//!   wrapper.
//! * [`in_band`] and [`view_vector`]: the view cone test on the 0..=255
//!   heading, without angles or square roots.
//! * [`sight_line`]: an exact walk over every cell between two fine points,
//!   with fog by length, symmetric from both ends. On its own it replaces a
//!   game's yes/no sight test.
//!
//! No floats, no allocation, no global state.

mod cone;
mod line;
mod map;
mod world;

pub use cone::{in_band, view_vector};
pub use line::{Line, sight_line};
pub use map::{Lit, Medium, OpenAir, SenseMap};
pub use world::{Channel, Grid, MAX_CHANNELS, Spread, World, cell_centre, cell_of, hex_heading};

#[cfg(test)]
mod tests;
