//! Path search for games.
//!
//! * [`nav::Navigator`] is the entry point: one object per kind of grid
//!   (square or hex) for paths, areas, smoothing and formations. The C and
//!   WebAssembly interfaces only translate to it.
//! * [`trace`] shows what a search did, step by step (off: no cost).
//! * [`Search`] is the one search core. With a [`Goal`] that estimates the
//!   remaining cost it is A*; with an estimate of 0 it is Dijkstra. It can be
//!   stopped after any number of expanded nodes and continued later, and it
//!   reports what it does through an [`sagax_core::Sink`].
//! * [`Graph`] is how the search sees a map. The game keeps its own map and
//!   answers neighbour questions; nothing is copied.
//! * [`dstar`] keeps a path up to date while the map changes (D* Lite).
//! * [`flow`] serves many walkers with one search (flow fields).
//! * [`smooth`] straightens grid paths; [`formation`] moves groups.
//! * [`topology`] switches between square and hex at run time.
//! * [`grid`] adapts 2D grids with levels (stairs, bridges) to [`Graph`].
//!
//! Costs are non-negative integers ([`Cost`]). Sums that would overflow are
//! not followed and are reported ([`Search::cost_overflow`]).

mod hash;
mod search;
mod table;

pub mod dstar;
pub mod flow;
pub mod formation;
pub mod grid;
pub mod hex;
pub mod nav;
pub mod smooth;
pub mod topology;
pub mod trace;

pub use search::{
    Cost, Goal, Graph, Limits, NoGoal, PathResult, Search, SearchEvent, SearchStatus, find_path,
};
