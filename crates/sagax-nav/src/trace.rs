//! Tracing: what a search did, step by step, for debugging and logging.
//!
//! Turned off (the default) a search runs exactly the code it runs without
//! tracing, so it costs nothing. Turned on, every step is handed to a
//! [`Tracer`] as a flat [`TraceEvent`], the same record the C and JavaScript
//! interfaces pass on.
//!
//! ```
//! use sagax_nav::grid::{Cell, DenseGrid, GridRules};
//! use sagax_nav::nav::Navigator;
//! use sagax_nav::topology::Topology;
//! use sagax_nav::trace::{TraceEvent, TraceKind};
//! use std::sync::{Arc, Mutex};
//!
//! let events = Arc::new(Mutex::new(Vec::new()));
//! let sink = Arc::clone(&events);
//! let mut nav = Navigator::new(Topology::Square(GridRules::default()), 1000);
//! nav.set_tracer(Some(Box::new(move |e: &TraceEvent| sink.lock().unwrap().push(*e))));
//! let mut map = DenseGrid::new(10, 10);
//! nav.find_path(&mut map, Cell::new(0, 0, 0), &[Cell::new(5, 0, 0)], usize::MAX);
//! let events = events.lock().unwrap();
//! assert_eq!(events.first().unwrap().kind, TraceKind::Started);
//! assert_eq!(events.last().unwrap().kind, TraceKind::Finished);
//! ```

use sagax_core::Sink;

use crate::grid::Cell;
use crate::search::{Cost, SearchEvent, SearchStatus};

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum TraceKind {
    /// A search begins at `cell`.
    Started = 1,
    /// `cell` was expanded: its cost is final (D* Lite: for now).
    Expanded = 2,
    /// `cell` was reached more cheaply, coming from `parent`.
    Improved = 3,
    /// The budget ran out; the search can go on.
    Paused = 4,
    /// The search ended; `found` tells whether a goal was reached.
    Finished = 5,
}

/// One step of a search. Fields that do not apply to a kind are zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceEvent {
    /// What happened.
    pub kind: TraceKind,
    /// The cell it happened to (the start for `Started`).
    pub cell: Cell,
    /// Where `cell` was reached from (`Improved`).
    pub parent: Cell,
    /// Cost from the start (D* Lite and flow fields: to the goal).
    pub cost: Cost,
    /// Estimated cost still to go.
    pub estimate: Cost,
    /// Cells expanded so far (`Paused`, `Finished`).
    pub expanded: usize,
    /// A goal was reached (`Finished`).
    pub found: bool,
}

impl TraceEvent {
    fn new(kind: TraceKind, cell: Cell) -> Self {
        Self {
            kind,
            cell,
            parent: Cell::new(0, 0, 0),
            cost: 0,
            estimate: 0,
            expanded: 0,
            found: false,
        }
    }
}

/// Receives the steps of a search.
pub type Tracer = Box<dyn FnMut(&TraceEvent)>;

/// Hands search events to a tracer as [`TraceEvent`]s.
pub struct TraceSink<'a>(pub &'a mut dyn FnMut(&TraceEvent));

impl Sink<SearchEvent<Cell>> for TraceSink<'_> {
    fn emit(&mut self, event: SearchEvent<Cell>) {
        let e = match event {
            SearchEvent::Started {
                start, estimate, ..
            } => TraceEvent {
                estimate,
                ..TraceEvent::new(TraceKind::Started, start)
            },
            SearchEvent::Expanded {
                node,
                cost,
                estimate,
                ..
            } => TraceEvent {
                cost,
                estimate,
                ..TraceEvent::new(TraceKind::Expanded, node)
            },
            SearchEvent::Improved {
                node, parent, cost, ..
            } => TraceEvent {
                parent,
                cost,
                ..TraceEvent::new(TraceKind::Improved, node)
            },
            SearchEvent::Paused { expanded, .. } => TraceEvent {
                expanded,
                ..TraceEvent::new(TraceKind::Paused, Cell::new(0, 0, 0))
            },
            SearchEvent::Finished {
                status, expanded, ..
            } => TraceEvent {
                expanded,
                found: status == SearchStatus::Found,
                ..TraceEvent::new(TraceKind::Finished, Cell::new(0, 0, 0))
            },
        };
        (self.0)(&e);
    }
}
