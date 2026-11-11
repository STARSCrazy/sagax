//! The search core shared by A* and Dijkstra.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::hash::Hash;

use sagax_core::{NoopSink, Sink};

use crate::table::NodeTable;

/// Cost of moving through a graph. Non-negative by type.
pub type Cost = u32;

/// How a search sees a map.
///
/// The game implements this over its own data; the search only asks for the
/// neighbours of one node at a time.
pub trait Graph {
    /// A node, e.g. a grid cell. Small and cheap to copy.
    type Node: Copy + Eq + Hash;

    /// Appends every node reachable in one move from `node`, with the cost of
    /// that move. `out` is empty when called; the search reuses its memory.
    fn neighbors(&mut self, node: Self::Node, out: &mut Vec<(Self::Node, Cost)>);
}

/// Where a search wants to go, and how far away that is.
pub trait Goal<N> {
    /// `true` for a node the search may end at.
    fn is_goal(&self, node: N) -> bool;

    /// A guess of the cost still needed from `node` to a goal.
    ///
    /// The search finds a cheapest path when the guess never exceeds the real
    /// cost and never drops by more than the cost of a move ("consistent").
    /// The built-in grid estimates have this property; for your own estimate
    /// that is your responsibility. Return 0 everywhere for Dijkstra.
    ///
    /// When the search cannot reach a goal, it reports the node with the
    /// smallest estimate as the best partial result.
    fn estimate(&self, node: N) -> Cost;
}

/// A goal that is never reached: the search settles every node it can reach
/// (within [`Limits::max_cost`]). Use it to ask "where can I get to?".
#[derive(Clone, Copy, Debug, Default)]
pub struct NoGoal;

impl<N> Goal<N> for NoGoal {
    #[inline]
    fn is_goal(&self, _node: N) -> bool {
        false
    }

    #[inline]
    fn estimate(&self, _node: N) -> Cost {
        0
    }
}

/// Bounds of one search, fixed when it begins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Nodes the search may get to know at most (memory bound). New nodes
    /// beyond it are ignored and [`Search::node_limit_hit`] is set.
    pub max_nodes: usize,
    /// Paths costing more than this are not followed.
    pub max_cost: Cost,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_nodes: usize::MAX,
            max_cost: Cost::MAX,
        }
    }
}

/// Where a search stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SearchStatus {
    /// Not begun yet.
    Idle,
    /// Can go on: call [`Search::advance`] again.
    Running,
    /// A goal was reached; the path to it is a cheapest one.
    Found,
    /// Every reachable node was expanded and none is a goal.
    Exhausted,
}

/// What a search reports to a diagnostic sink, in this order: `Started`, then
/// for each expanded node `Expanded` followed by its `Improved` neighbours,
/// `Paused` when an advance ends early, and `Finished` once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchEvent<N> {
    /// The search begins at `start`.
    Started {
        /// Number of the search within its [`Search`] (counts up from 1).
        search: u32,
        /// Start node.
        start: N,
        /// Estimate of the start.
        estimate: Cost,
    },
    /// `node` was taken from the open list; its cost is final.
    Expanded {
        /// Number of the search.
        search: u32,
        /// The node.
        node: N,
        /// Its cost from the start.
        cost: Cost,
        /// Its estimate to the goal.
        estimate: Cost,
    },
    /// `node` was reached more cheaply than before (`first` when it was new).
    Improved {
        /// Number of the search.
        search: u32,
        /// The node.
        node: N,
        /// The node it is reached from.
        parent: N,
        /// Its new cost from the start.
        cost: Cost,
        /// `true` when the node was seen for the first time.
        first: bool,
    },
    /// An advance used up its budget.
    Paused {
        /// Number of the search.
        search: u32,
        /// Nodes expanded so far.
        expanded: usize,
    },
    /// The search ended.
    Finished {
        /// Number of the search.
        search: u32,
        /// [`SearchStatus::Found`] or [`SearchStatus::Exhausted`].
        status: SearchStatus,
        /// Nodes expanded in total.
        expanded: usize,
    },
}

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Record<N> {
    node: N,
    cost: Cost,
    estimate: Cost,
    parent: u32,
    closed: bool,
}

/// An entry of the open list, 16 bytes. Ordered so that the max-heap pops the
/// smallest total first; among equal totals the one nearer the goal (on open
/// ground many ways cost the same, and this avoids trying them all), then the
/// node seen first. The order is fully defined, so a search is reproducible.
/// Totals beyond `u32::MAX` count as equal (they only arise near overflow).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Open {
    /// total (saturated to 32 bits) in the high half, estimate in the low half
    key: u64,
    index: u32,
}

impl Ord for Open {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        (other.key, other.index).cmp(&(self.key, self.index))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A search that keeps its state between calls.
///
/// Keep one `Search` and reuse it: after the first searches it needs no new
/// memory. The map must not change between [`begin`](Search::begin) and the
/// end of the search; begin again after a change.
pub struct Search<N> {
    index: NodeTable<N>,
    records: Vec<Record<N>>,
    open: BinaryHeap<Open>,
    scratch: Vec<(N, Cost)>,
    limits: Limits,
    status: SearchStatus,
    best: u32,
    goal: u32,
    expanded: usize,
    search_number: u32,
    started: bool,
    node_limit_hit: bool,
    cost_overflow: bool,
}

impl<N: Copy + Eq + Hash> Default for Search<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<N: Copy + Eq + Hash> Search<N> {
    /// An empty search.
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// An empty search with memory for `nodes` nodes reserved up front.
    pub fn with_capacity(nodes: usize) -> Self {
        Self {
            index: NodeTable::with_capacity(nodes),
            records: Vec::with_capacity(nodes),
            open: BinaryHeap::with_capacity(nodes),
            scratch: Vec::with_capacity(16),
            limits: Limits::default(),
            status: SearchStatus::Idle,
            best: NONE,
            goal: NONE,
            expanded: 0,
            search_number: 0,
            started: false,
            node_limit_hit: false,
            cost_overflow: false,
        }
    }

    /// Starts a new search from `start` and forgets the previous one.
    pub fn begin<G: Goal<N>>(&mut self, start: N, goal: &G, limits: Limits) {
        self.begin_many(std::iter::once(start), goal, limits);
    }

    /// Starts a new search from several nodes at once, all at cost 0 (e.g.
    /// every cell next to a target, or every goal of a flow field).
    /// Repeated nodes count once. Without any start the search is
    /// [`SearchStatus::Exhausted`] at once.
    pub fn begin_many<G: Goal<N>>(
        &mut self,
        starts: impl IntoIterator<Item = N>,
        goal: &G,
        limits: Limits,
    ) {
        self.index.clear();
        self.records.clear();
        self.open.clear();
        self.limits = limits;
        self.status = SearchStatus::Running;
        self.goal = NONE;
        self.expanded = 0;
        self.search_number = self.search_number.wrapping_add(1);
        self.started = false;
        self.node_limit_hit = false;
        self.cost_overflow = false;

        self.best = NONE;
        for start in starts {
            if self.index.get(&start).is_some() {
                continue;
            }
            let index = self.records.len() as u32;
            let estimate = goal.estimate(start);
            self.index.insert_new(start, index);
            self.records.push(Record {
                node: start,
                cost: 0,
                estimate,
                parent: NONE,
                closed: false,
            });
            if self.best == NONE || estimate < self.records[self.best as usize].estimate {
                self.best = index;
            }
            self.push_open(index, 0, estimate);
        }
        if self.records.is_empty() {
            self.status = SearchStatus::Exhausted;
        }
    }

    /// Expands at most `max_expansions` more nodes. Returns
    /// [`SearchStatus::Running`] if the search can go on.
    ///
    /// The result does not depend on how the work is split: many small
    /// advances end exactly like one big one.
    pub fn advance<G, Go, S>(
        &mut self,
        graph: &mut G,
        goal: &Go,
        max_expansions: usize,
        sink: &mut S,
    ) -> SearchStatus
    where
        G: Graph<Node = N>,
        Go: Goal<N>,
        S: Sink<SearchEvent<N>>,
    {
        if self.status != SearchStatus::Running {
            return self.status;
        }
        let search = self.search_number;
        if !self.started {
            self.started = true;
            if let (true, Some(&start)) = (sink.enabled(), self.records.first()) {
                sink.emit(SearchEvent::Started {
                    search,
                    start: start.node,
                    estimate: start.estimate,
                });
            }
        }

        let mut left = max_expansions;
        let mut neighbors = std::mem::take(&mut self.scratch);
        while left > 0 {
            let Some(top) = self.open.pop() else {
                break;
            };
            let current = top.index;
            let record = &mut self.records[current as usize];
            if record.closed {
                continue; // an older, worse entry of a node already expanded
            }
            record.closed = true;
            let Record {
                node,
                cost,
                estimate,
                ..
            } = *record;
            self.expanded += 1;
            left -= 1;
            if sink.enabled() {
                sink.emit(SearchEvent::Expanded {
                    search,
                    node,
                    cost,
                    estimate,
                });
            }

            let best = self.records[self.best as usize];
            if estimate < best.estimate || (estimate == best.estimate && cost < best.cost) {
                self.best = current;
            }
            if goal.is_goal(node) {
                self.goal = current;
                self.best = current;
                self.status = SearchStatus::Found;
                break;
            }

            neighbors.clear();
            graph.neighbors(node, &mut neighbors);
            for &(next, step) in &neighbors {
                let Some(next_cost) = cost.checked_add(step) else {
                    self.cost_overflow = true;
                    continue;
                };
                if next_cost > self.limits.max_cost {
                    continue;
                }
                let (index, first) = match self.index.get(&next) {
                    Some(index) => (index, false),
                    None => {
                        if self.records.len() >= self.limits.max_nodes
                            || self.records.len() >= NONE as usize
                        {
                            self.node_limit_hit = true;
                            continue;
                        }
                        let index = self.records.len() as u32;
                        self.index.insert_new(next, index);
                        self.records.push(Record {
                            node: next,
                            cost: Cost::MAX,
                            estimate: goal.estimate(next),
                            parent: NONE,
                            closed: false,
                        });
                        (index, true)
                    }
                };
                let record = &mut self.records[index as usize];
                if record.closed || (!first && next_cost >= record.cost) {
                    continue;
                }
                record.cost = next_cost;
                record.parent = current;
                let next_estimate = record.estimate;
                if sink.enabled() {
                    sink.emit(SearchEvent::Improved {
                        search,
                        node: next,
                        parent: node,
                        cost: next_cost,
                        first,
                    });
                }
                self.push_open(index, next_cost, next_estimate);
            }
        }
        self.scratch = neighbors;

        if self.status == SearchStatus::Running && self.open.is_empty() {
            self.status = SearchStatus::Exhausted;
        }
        if sink.enabled() {
            match self.status {
                SearchStatus::Running => sink.emit(SearchEvent::Paused {
                    search,
                    expanded: self.expanded,
                }),
                status => sink.emit(SearchEvent::Finished {
                    search,
                    status,
                    expanded: self.expanded,
                }),
            }
        }
        self.status
    }

    /// Runs the search to its end without diagnostics.
    pub fn run<G: Graph<Node = N>, Go: Goal<N>>(
        &mut self,
        graph: &mut G,
        goal: &Go,
    ) -> SearchStatus {
        self.advance(graph, goal, usize::MAX, &mut NoopSink)
    }

    #[inline]
    fn push_open(&mut self, index: u32, cost: Cost, estimate: Cost) {
        let total = cost.saturating_add(estimate);
        self.open.push(Open {
            key: u64::from(total) << 32 | u64::from(estimate),
            index,
        });
    }

    /// Where the search stands.
    pub fn status(&self) -> SearchStatus {
        self.status
    }

    /// Nodes expanded so far.
    pub fn expanded(&self) -> usize {
        self.expanded
    }

    /// Nodes the search got to know (expanded or waiting).
    pub fn known(&self) -> usize {
        self.records.len()
    }

    /// `true` if new nodes were ignored because of [`Limits::max_nodes`].
    pub fn node_limit_hit(&self) -> bool {
        self.node_limit_hit
    }

    /// `true` if a path was not followed because its cost would overflow.
    pub fn cost_overflow(&self) -> bool {
        self.cost_overflow
    }

    /// The reached goal and its cost, once [`SearchStatus::Found`].
    pub fn goal(&self) -> Option<(N, Cost)> {
        self.record(self.goal).map(|r| (r.node, r.cost))
    }

    /// The best node so far: the goal if found, else the expanded node with
    /// the smallest estimate (among equals the cheapest), and its cost.
    pub fn best(&self) -> Option<(N, Cost)> {
        self.record(self.best).map(|r| (r.node, r.cost))
    }

    /// The final cost of an expanded node.
    pub fn settled_cost(&self, node: N) -> Option<Cost> {
        let index = self.index.get(&node)?;
        let record = self.records[index as usize];
        record.closed.then_some(record.cost)
    }

    /// Every expanded node with its final cost, in the order they were first
    /// seen. After a search with [`NoGoal`] these are all reachable nodes.
    pub fn settled(&self) -> impl Iterator<Item = (N, Cost)> + '_ {
        self.records
            .iter()
            .filter(|r| r.closed)
            .map(|r| (r.node, r.cost))
    }

    /// Writes the path from the start to `node` (both included) into `out`.
    /// Returns `false`, leaving `out` empty, if the node is unknown.
    pub fn path_to(&self, node: N, out: &mut Vec<N>) -> bool {
        out.clear();
        let Some(index) = self.index.get(&node) else {
            return false;
        };
        self.path_from_index(index, out);
        true
    }

    /// Writes the path to [`best`](Search::best) into `out`.
    pub fn best_path(&self, out: &mut Vec<N>) -> bool {
        out.clear();
        if self.best == NONE {
            return false;
        }
        self.path_from_index(self.best, out);
        true
    }

    fn path_from_index(&self, mut index: u32, out: &mut Vec<N>) {
        while index != NONE {
            let record = &self.records[index as usize];
            out.push(record.node);
            index = record.parent;
        }
        out.reverse();
    }

    fn record(&self, index: u32) -> Option<&Record<N>> {
        if index == NONE {
            None
        } else {
            self.records.get(index as usize)
        }
    }
}

/// The outcome of [`find_path`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathResult<N> {
    /// [`SearchStatus::Found`], [`SearchStatus::Exhausted`], or
    /// [`SearchStatus::Running`] when the expansion budget ran out.
    pub status: SearchStatus,
    /// From the start (included) to the goal, or, if not reached, to the best
    /// node found.
    pub path: Vec<N>,
    /// Cost of `path`.
    pub cost: Cost,
    /// Nodes expanded.
    pub expanded: usize,
}

impl<N> PathResult<N> {
    /// `true` if `path` ends at a goal.
    pub fn reached(&self) -> bool {
        self.status == SearchStatus::Found
    }
}

/// One-shot convenience: searches from `start` to `goal`, expanding at most
/// `max_expansions` nodes. Allocates a fresh [`Search`]; keep your own for
/// repeated searches.
pub fn find_path<G, Go>(
    graph: &mut G,
    start: G::Node,
    goal: &Go,
    limits: Limits,
    max_expansions: usize,
) -> PathResult<G::Node>
where
    G: Graph,
    Go: Goal<G::Node>,
{
    let mut search = Search::new();
    search.begin(start, goal, limits);
    let status = search.advance(graph, goal, max_expansions, &mut NoopSink);
    let mut path = Vec::new();
    search.best_path(&mut path);
    let cost = search.best().map_or(0, |(_, c)| c);
    PathResult {
        status,
        path,
        cost,
        expanded: search.expanded(),
    }
}
