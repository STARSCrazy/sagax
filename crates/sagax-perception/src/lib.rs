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
//! On top of it, awareness over time:
//!
//! * [`SenseProfile`] (with presets and builders), [`Stats`] and
//!   [`DisguiseRule`] make an [`Eye`]; the game passes its candidate
//!   [`Target`]s.
//! * [`observe`] runs one check of one observer–target pair and keeps the
//!   result in the pair's [`Awareness`] (noticed, recognised, reaction
//!   delay, decay, disguise); [`look`] checks a bounded number of targets
//!   per call ([`Budget`], [`Attention`]). Both push [`Percept`]s.
//! * [`in_sight`]: cone, range and line, for rule slots.
//!
//! And hearing:
//!
//! * [`hear`] checks one [`Stimulus`] for one listener: by straight
//!   distance, along one walk with the map's damping, or by a cost the
//!   game passes (a flood around corners). It always pushes a `Heard`
//!   percept with a deterministically fuzzed whole cell, and with an
//!   [`Awareness`] raises the pair at most to *noticed*.
//!
//! [`PerceptionTrace`] holds the records of the `_traced` forms.
//!
//! No floats, no allocation, no global state.

mod awareness;
mod cone;
mod gain;
mod hear;
mod line;
mod look;
mod map;
mod percept;
mod profile;
mod trace;
mod world;

pub use awareness::{Awareness, in_sight, observe, observe_traced};
pub use cone::{in_band, view_vector};
pub use hear::{Stimulus, hear, hear_traced};
pub use line::{Line, sight_line};
pub use look::{Attention, Budget, Pick, count_crowds, look, look_traced};
pub use map::{Lit, Medium, OpenAir, SenseMap};
pub use percept::{MAX_PERCEPTS_PER_CHECK, Percept, PerceptKind, Sense, Target};
pub use profile::{
    Band, CueRule, DisguiseRule, Eye, MAX_CUES, NIGHT, SCANNER, SIGHT, SeeThrough, SenseProfile,
    Stats, THERMAL,
};
pub use trace::PerceptionTrace;
pub use world::{Channel, Grid, MAX_CHANNELS, Spread, World, cell_centre, cell_of, hex_heading};

#[cfg(test)]
mod tests;
