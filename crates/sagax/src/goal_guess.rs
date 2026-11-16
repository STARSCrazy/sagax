//! Goal recognition from flow fields: [`observe_flow`] feeds a seen step to
//! a [`GoalGuess`], with one [`FlowField`] per candidate goal.
//!
//! The guess itself (in [`decide::memory`](crate::decide::memory)) knows no
//! map; this helper, which sees both nav and decide, turns a step
//! `prev -> cur` into the changes of walking distance the guess learns
//! from. Feed only steps the observer saw (for instance from its own
//! percepts): the guess never knows the true goal or the true position.
//!
//! ```
//! use sagax::prelude::*;
//! use sagax::nav::Limits;
//! use sagax::nav::flow::FlowField;
//!
//! let rules = GridRules::preset(Preset::Octile);
//! let mut map = DenseGrid::new(9, 1);
//! let mut graph = Topology::Square(rules).graph(&mut map, None);
//! let exits = [Cell::new(0, 0, 0), Cell::new(8, 0, 0)];
//! let fields: Vec<FlowField<Cell>> = exits
//!     .iter()
//!     .map(|&e| {
//!         let mut f = FlowField::default();
//!         f.build(&mut graph, [e], Limits::default());
//!         f
//!     })
//!     .collect();
//! let mut guess = GoalGuess::new(GoalGuessConfig::default(), exits.len());
//! let mut moves = Vec::new();
//! let (prev, cur) = (Cell::new(4, 0, 0), Cell::new(5, 0, 0));
//! observe_flow(&mut guess, &mut graph, &fields, prev, cur, &mut moves).unwrap();
//! assert_eq!(guess.best().map(|b| b.0), Some(1));
//! ```

use std::borrow::Borrow;

use sagax_core::{NoopSink, Sink};
use sagax_decide::memory::{GoalGuess, GoalGuessTrace, MAX_GOALS, MAX_MOVES, StepError};
use sagax_nav::flow::FlowField;
use sagax_nav::search::{Cost, Graph};

/// [`observe_flow_traced`] with [`NoopSink`].
pub fn observe_flow<G: Graph, F: Borrow<FlowField<G::Node>>>(
    guess: &mut GoalGuess,
    graph: &mut G,
    fields: &[F],
    prev: G::Node,
    cur: G::Node,
    moves: &mut Vec<(G::Node, Cost)>,
) -> Result<(), StepError> {
    observe_flow_traced(guess, graph, fields, prev, cur, moves, &mut NoopSink)
}

/// Feeds the seen step `prev -> cur` to `guess`; `fields[k]` is the flow
/// field of goal `k` (one per goal, in the guess's order). `moves` is
/// reused memory for the moves of `prev` (cleared first; keep one per
/// observer and nothing allocates once it has grown).
///
/// The moves are `graph.neighbors(prev)`; for each goal the change of
/// walking distance of every move is `cost(move) - cost(prev)` from that
/// goal's field. A move outside a field gets the largest known change of
/// that goal (the least likely option); a goal whose field does not hold
/// `prev`, or holds none of the moves, learns nothing from this step
/// (every change 0), so a blocked or unreachable goal never zeroes the
/// others.
///
/// `cur == prev` (standing still) teaches nothing: `Ok`, no change, no
/// record. Refused with a `Refused` record and nothing changed:
/// [`StepError::Jump`] when `cur` is not one of the moves (the observer
/// lost sight and found him elsewhere: just go on from the new place, the
/// forgetting covers the gap), [`StepError::Shape`] when the number of
/// fields is not the number of goals or `prev` has more than
/// [`MAX_MOVES`] moves.
pub fn observe_flow_traced<G: Graph, F: Borrow<FlowField<G::Node>>>(
    guess: &mut GoalGuess,
    graph: &mut G,
    fields: &[F],
    prev: G::Node,
    cur: G::Node,
    moves: &mut Vec<(G::Node, Cost)>,
    sink: &mut impl Sink<GoalGuessTrace>,
) -> Result<(), StepError> {
    let n = guess.goals();
    let mut refuse = |error: StepError| {
        sink.emit(GoalGuessTrace::Refused { error });
        Err(error)
    };
    if fields.len() != n {
        return refuse(StepError::Shape);
    }
    if cur == prev {
        return Ok(());
    }
    moves.clear();
    graph.neighbors(prev, moves);
    let per = moves.len();
    if per > MAX_MOVES {
        return refuse(StepError::Shape);
    }
    let Some(taken) = moves.iter().position(|&(m, _)| m == cur) else {
        return refuse(StepError::Jump);
    };
    let mut moved = [0i32; MAX_GOALS];
    let mut options = [0i32; MAX_GOALS * MAX_MOVES];
    for (k, field) in fields.iter().enumerate() {
        let field = field.borrow();
        let row = &mut options[k * per..(k + 1) * per];
        changes(field, prev, moves, row);
        moved[k] = row[taken];
    }
    guess.observe_traced(&moved[..n], &options[..n * per], per, sink)
}

/// The change of walking distance of every move in `field` (see
/// [`observe_flow_traced`]).
fn changes<N: Copy + Eq + std::hash::Hash>(
    field: &FlowField<N>,
    prev: N,
    moves: &[(N, Cost)],
    row: &mut [i32],
) {
    row.fill(0);
    let Some(here) = field.cost(prev) else {
        return;
    };
    let change = |m: N| {
        field.cost(m).map(|c| {
            (i64::from(c) - i64::from(here)).clamp(i32::MIN.into(), i32::MAX.into()) as i32
        })
    };
    let Some(worst) = moves.iter().filter_map(|&(m, _)| change(m)).max() else {
        return;
    };
    for (slot, &(m, _)) in row.iter_mut().zip(moves) {
        *slot = change(m).unwrap_or(worst);
    }
}
