//! Sagax in one crate: paths, decisions and group tactics for game AI.
//!
//! `use sagax::prelude::*;` brings the types most games need. The parts are
//! also reachable as [`nav`], [`decide`] and [`debug`].

pub use sagax_core::{CallbackSink, Cell, HexLayout, NoopSink, Pos, Sink};
pub use sagax_debug as debug;
pub use sagax_decide as decide;
pub use sagax_nav as nav;

pub mod prelude;
pub mod rules;
