//! Sagax in one crate: paths, decisions and group tactics for game AI.
//!
//! `use sagax::prelude::*;` brings the types most games need. The parts are
//! also reachable as [`nav`] and [`decide`]; [`perception`] holds sight
//! and hearing with their nav and memory helpers; [`debug`] holds
//! everything trace related.

pub use sagax_core::{Cell, HexLayout, Pos};
pub use sagax_decide as decide;
pub use sagax_nav as nav;

pub mod perception;
pub mod prelude;
pub mod rules;

pub mod debug {
    //! Everything for tracing, in one place: `use sagax::debug::*;`.
    //!
    //! A trace is a [`Sink`] parameter: every operation that decides something
    //! has an `op_traced(.., sink)` form next to the plain `op(..)`, which is
    //! the traced one with [`NoopSink`] and costs nothing. A [`Collector`] keeps
    //! the last (or first) records of a call; a facade hands the records of the
    //! module it drives on, wrapped, through a [`MapSink`].
    //!
    //! ```
    //! use sagax::debug::*;
    //! use sagax::prelude::*;
    //!
    //! let mut map = DenseGrid::new(10, 10);
    //! let mut nav = Navigator::new(Topology::Square(GridRules::preset(Preset::Octile)), 100);
    //! let mut log = Collector::new(256, Overflow::DropOldest);
    //! let goal = [Cell::new(5, 5, 0)];
    //! nav.find_path_traced(&mut map, Cell::new(0, 0, 0), &goal, Budget::UNLIMITED, &mut log);
    //! assert!(matches!(
    //!     log.records().last().unwrap().event,
    //!     NavTrace::Search(SearchTrace::Finished { .. })
    //! ));
    //! ```

    pub use sagax_core::{CallbackSink, MapSink, NoopSink, Sink};
    pub use sagax_debug::{Collector, Overflow, Record};
    pub use sagax_decide::group::{CrewTrace, KeeperTrace, SpotTrace, SquadTrace};
    pub use sagax_decide::memory::{AreasTrace, KnowledgeTrace, MessageTrace, TimersTrace};
    pub use sagax_decide::{DecideTrace, mission::MissionTrace, state::StateTrace};
    pub use sagax_nav::{
        formation::FormationTrace, nav::NavTrace, search::SearchTrace, smooth::SmoothTrace,
    };
    pub use sagax_perception::PerceptionTrace;
}
