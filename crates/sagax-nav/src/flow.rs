//! Flow fields: one search serves many walkers with the same goal.
//!
//! A flow field knows, for every node it reached, the cost of the cheapest
//! way to the nearest goal. A walker anywhere in the field asks for its next
//! step and needs no search of its own. That pays when many walkers head for
//! the same place (a crowd following its leader, guards converging on an
//! alarm, all enemies hunting the player).
//!
//! The field is built by searching outward *from* the goals along the
//! graph's moves. Costs to the goal are therefore exact when moves cost the
//! same in both directions (the usual case). Where they do not (one-way
//! drops, leave costs of zones), the next step still uses the real forward
//! moves, but the field's estimate of the remaining cost may be off.

use std::hash::Hash;

use sagax_core::{NoopSink, Sink};

use crate::search::{Cost, Graph, Limits, NoGoal, Search, SearchEvent};

/// Distances to the nearest goal for every reachable node.
pub struct FlowField<N> {
    search: Search<N>,
    scratch: Vec<(N, Cost)>,
}

impl<N: Copy + Eq + Hash> Default for FlowField<N> {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

impl<N: Copy + Eq + Hash> FlowField<N> {
    /// An empty field with room for `nodes` nodes.
    pub fn with_capacity(nodes: usize) -> Self {
        Self {
            search: Search::with_capacity(nodes),
            scratch: Vec::with_capacity(16),
        }
    }

    /// Fills the field for `goals`, forgetting the previous one. `limits`
    /// bound memory and the largest cost kept (cells farther away are not in
    /// the field).
    pub fn build<G: Graph<Node = N>>(
        &mut self,
        graph: &mut G,
        goals: impl IntoIterator<Item = N>,
        limits: Limits,
    ) {
        self.build_traced(graph, goals, limits, &mut NoopSink);
    }

    /// As [`build`](Self::build), reporting every step to `sink` (costs are
    /// costs to the nearest goal).
    pub fn build_traced<G: Graph<Node = N>, S: Sink<SearchEvent<N>>>(
        &mut self,
        graph: &mut G,
        goals: impl IntoIterator<Item = N>,
        limits: Limits,
        sink: &mut S,
    ) {
        self.search.begin_many(goals, &NoGoal, limits);
        self.search.advance(graph, &NoGoal, usize::MAX, sink);
    }

    /// Cost from `node` to the nearest goal, if the field reached it.
    pub fn cost(&self, node: N) -> Option<Cost> {
        self.search.settled_cost(node)
    }

    /// Number of nodes in the field.
    pub fn len(&self) -> usize {
        self.search.settled().count()
    }

    /// `true` if the field holds no node.
    pub fn is_empty(&self) -> bool {
        self.search.settled().next().is_none()
    }

    /// The best next node from `node` towards a goal: the neighbour closer to a
    /// goal with the smallest move cost plus remaining cost. `None` at a goal,
    /// outside the field or when stuck.
    pub fn next<G: Graph<Node = N>>(&mut self, graph: &mut G, node: N) -> Option<N> {
        next_step(&self.search, graph, node, &mut self.scratch)
    }

    /// Follows the field from `from` to a goal and writes the nodes (both
    /// ends included) into `out`. Returns `false` if `from` is outside the
    /// field or the way got stuck.
    pub fn path<G: Graph<Node = N>>(&mut self, graph: &mut G, from: N, out: &mut Vec<N>) -> bool {
        out.clear();
        let Some(mut left) = self.cost(from) else {
            return false;
        };
        out.push(from);
        let mut node = from;
        loop {
            if left == 0 {
                return true;
            }
            let Some(next) = self.next(graph, node) else {
                return false;
            };
            out.push(next);
            node = next;
            left = self.cost(node).unwrap_or(0);
        }
    }

    /// Every node of the field with its cost to the nearest goal.
    pub fn nodes(&self) -> impl Iterator<Item = (N, Cost)> + '_ {
        self.search.settled()
    }
}

/// The best next node from `node` towards a goal of the field in `search`:
/// the
/// neighbour closer to a goal with the smallest move cost plus remaining
/// cost. `None` at a goal, outside the field, or when stuck. `scratch` is
/// reused memory for the neighbours.
fn next_step<N: Copy + Eq + Hash, G: Graph<Node = N>>(
    search: &Search<N>,
    graph: &mut G,
    node: N,
    scratch: &mut Vec<(N, Cost)>,
) -> Option<N> {
    let here = search.settled_cost(node)?;
    if here == 0 {
        return None;
    }
    scratch.clear();
    graph.neighbors(node, scratch);
    let mut best: Option<(u64, N)> = None;
    for &(next, step) in scratch.iter() {
        let Some(there) = search.settled_cost(next) else {
            continue;
        };
        if there >= here {
            continue; // never away from the goal: no loops
        }
        let total = u64::from(step) + u64::from(there);
        if best.is_none_or(|(b, _)| total < b) {
            best = Some((total, next));
        }
    }
    best.map(|(_, n)| n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Cell, CellGoal, DenseGrid, GridGraph, GridRules};
    use crate::search::find_path;

    #[test]
    fn every_walker_gets_a_cheapest_way() {
        let mut map = DenseGrid::new(30, 20);
        for y in 0..15 {
            map.set(15, y, DenseGrid::BLOCKED);
        }
        map.set(5, 5, 40);
        let rules = GridRules::default();
        let mut g = GridGraph::new(map, rules, None);
        let goal = Cell::new(25, 3, 0);
        let mut field = FlowField::with_capacity(600);
        field.build(&mut g, [goal], Limits::default());

        let mut way = Vec::new();
        for start in [
            Cell::new(0, 0, 0),
            Cell::new(3, 19, 0),
            Cell::new(14, 2, 0),
            Cell::new(29, 19, 0),
        ] {
            let astar = find_path(
                &mut g,
                start,
                &CellGoal::new(25, 3, None, rules),
                Limits::default(),
                usize::MAX,
            );
            assert_eq!(
                field.cost(start),
                Some(astar.cost),
                "field cost from {start:?}"
            );
            assert!(field.path(&mut g, start, &mut way));
            assert_eq!(way.last(), Some(&goal));
            let mut sum = 0;
            for w in way.windows(2) {
                let mut nb = Vec::new();
                g.neighbors(w[0], &mut nb);
                sum += nb.iter().find(|(n, _)| *n == w[1]).expect("legal step").1;
            }
            assert_eq!(sum, astar.cost, "following the field costs the optimum");
        }
    }

    #[test]
    fn several_goals_lead_to_the_nearest() {
        let rules = GridRules::default();
        let mut g = GridGraph::new(DenseGrid::new(20, 1), rules, None);
        let mut field = FlowField::default();
        field.build(
            &mut g,
            [Cell::new(0, 0, 0), Cell::new(19, 0, 0)],
            Limits::default(),
        );
        assert_eq!(field.cost(Cell::new(4, 0, 0)), Some(40));
        assert_eq!(field.cost(Cell::new(15, 0, 0)), Some(40));
        assert_eq!(
            field.next(&mut g, Cell::new(4, 0, 0)),
            Some(Cell::new(3, 0, 0))
        );
        assert_eq!(
            field.next(&mut g, Cell::new(15, 0, 0)),
            Some(Cell::new(16, 0, 0))
        );
        assert_eq!(field.next(&mut g, Cell::new(0, 0, 0)), None);
    }

    #[test]
    fn cells_outside_the_range_or_cut_off_are_not_in_the_field() {
        let rules = GridRules::default();
        let mut map = DenseGrid::new(20, 3);
        for y in 0..3 {
            map.set(10, y, DenseGrid::BLOCKED);
        }
        let mut g = GridGraph::new(map, rules, None);
        let mut field = FlowField::default();
        field.build(
            &mut g,
            [Cell::new(0, 1, 0)],
            Limits {
                max_cost: 50,
                ..Limits::default()
            },
        );
        assert_eq!(field.cost(Cell::new(5, 1, 0)), Some(50));
        assert_eq!(field.cost(Cell::new(6, 1, 0)), None);
        field.build(&mut g, [Cell::new(0, 1, 0)], Limits::default());
        assert_eq!(field.cost(Cell::new(15, 1, 0)), None);
        let mut way = Vec::new();
        assert!(!field.path(&mut g, Cell::new(15, 1, 0), &mut way));
    }
}
