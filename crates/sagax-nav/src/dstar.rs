//! D* Lite: a path that is kept up to date while the map changes.
//!
//! A walker goes from a start to a goal; on the way doors close, cells get
//! blocked or cheaper. After a change, [`DStarLite`] repairs its earlier work
//! instead of searching again from scratch. This is the optimised D* Lite of
//! Koenig & Likhachev (2002) with the key modifier `km`.
//!
//! # How it works
//!
//! The search runs *backward*, from the goal towards the walker. Every node
//! keeps `g`, its known cost to the goal, and `rhs`, the one-step lookahead
//! `min over successors s' of c(node, s') + g(s')`. A node with `g != rhs` is
//! inconsistent and waits in the open list. [`DStarLite::compute`] works the
//! open list off until the walker's node is consistent and nothing cheaper is
//! waiting; then `g(start)` is the cheapest cost to the goal.
//!
//! # Graph requirement: symmetric neighbour relation
//!
//! The backward search needs the *predecessors* of a node (the nodes that can
//! move into it). [`Graph`] only lists successors, so D* Lite assumes:
//!
//! > if `b` is in `neighbors(a)`, then `a` is in `neighbors(b)`.
//!
//! Neighbours therefore double as predecessors. The move *costs* may differ
//! between the two directions (a [`DenseGrid`](crate::grid::DenseGrid) charges
//! the cost of the cell entered), because every `rhs` is recomputed from the
//! node's own successor list, never from a predecessor's cost.
//!
//! A [`DenseGrid`](crate::grid::DenseGrid) without zones and with walls set in
//! pairs ([`add_wall`](crate::grid::DenseGrid::add_wall)) fulfils this, with
//! one tolerated exception: a blocked cell still lists moves *out* of it,
//! while nobody can move in. That is harmless as long as the walker does not
//! stand on such a cell: nothing routes through it. (A plan *begun* on a
//! blocked cell reports [`DStarStatus::Unreachable`], whereas A* would leave
//! it; a walker whose cell gets blocked later keeps a correct plan, because
//! its node is re-evaluated at every [`compute`](DStarLite::compute).)
//! One-way edges (one-way doors, zone costs
//! that only appear on leaving) break the requirement and give wrong results.
//!
//! # Map changes
//!
//! The graph passed to the calls must always show the current map. After
//! changing it, call [`nodes_changed`](DStarLite::nodes_changed) with every
//! node whose outgoing moves changed (cost, added or removed); with the
//! symmetric relation that includes both ends of a changed move. For a grid
//! cell that gets blocked or unblocked: the cell and its 8 neighbours (the
//! diagonal between two neighbours may pass the cell's corner).
//!
//! # Estimates
//!
//! The keys need an estimate between two arbitrary nodes ([`Heuristic`]). It
//! must never exceed the real cost (admissible) and satisfy the triangle
//! inequality (consistent). Costs are added in `u64`; [`INFINITY`] stands for
//! "no path".

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::hash::Hash;

use crate::grid::{Cell, GridRules};
use crate::hex::HexLayout;
use sagax_core::{NoopSink, Sink};

use crate::search::{Cost, Graph, SearchEvent, SearchStatus};
use crate::table::NodeTable;

/// The cost of "no path" in `g`, `rhs` and the keys.
pub const INFINITY: u64 = u64::MAX;

/// An estimate of the cost between two arbitrary nodes.
///
/// It must never exceed the cheapest real cost from `from` to `to`, and
/// `estimate(a, c) <= cost(a, b) + estimate(b, c)` for every move `a -> b`
/// (consistent). Returning 0 everywhere is allowed (D* Lite then works like
/// an incremental Dijkstra).
pub trait Heuristic<N> {
    /// Estimated cost from `from` to `to`.
    fn estimate(&self, from: N, to: N) -> Cost;
}

/// Grid distance on open ground ([`GridRules::distance`]); levels are ignored,
/// which keeps the estimate a lower bound.
impl Heuristic<Cell> for GridRules {
    fn estimate(&self, from: Cell, to: Cell) -> Cost {
        self.distance(to.x.saturating_sub(from.x), to.y.saturating_sub(from.y))
    }
}

/// Hex distance times the cost of one step, for a
/// [`HexGraph`](crate::hex::HexGraph) with the same layout and step cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HexHeuristic {
    /// Row or column shift.
    pub layout: HexLayout,
    /// Cost of one step.
    pub step_cost: Cost,
}

impl Heuristic<Cell> for HexHeuristic {
    fn estimate(&self, from: Cell, to: Cell) -> Cost {
        let steps = self.layout.distance(from.x, from.y, to.x, to.y);
        Cost::try_from(steps)
            .unwrap_or(Cost::MAX)
            .saturating_mul(self.step_cost)
    }
}

/// Where a [`DStarLite`] stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DStarStatus {
    /// Not begun yet.
    Idle,
    /// The expansion budget ran out: call [`DStarLite::compute`] again.
    Running,
    /// The cheapest path from the current start to the goal is known.
    Found,
    /// The goal cannot be reached from the current start.
    Unreachable,
}

/// Priority of an open node: lexicographic `(k1, k2)`.
type Key = (u64, u64);

#[derive(Clone, Copy)]
struct Record<N> {
    node: N,
    g: u64,
    rhs: u64,
    /// The key of the node's valid open-list entry; `None` if not open.
    queued: Option<Key>,
}

/// An open-list entry; the max-heap pops the smallest key first, among equal
/// keys the node seen first. Entries whose key differs from the node's
/// `queued` are outdated and skipped (lazy deletion).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Entry {
    key: Key,
    index: u32,
}

impl Ord for Entry {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        (other.key, other.index).cmp(&(self.key, self.index))
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Incremental path planning with D* Lite (see the [module docs](self)).
///
/// Keep one and reuse it: [`begin`](Self::begin) forgets the previous goal
/// but keeps the memory, so after warm-up only the node list and the open
/// list grow when a bigger area is searched.
pub struct DStarLite<N, H> {
    index: NodeTable<N>,
    records: Vec<Record<N>>,
    open: BinaryHeap<Entry>,
    /// Successors of the node whose `rhs` is recomputed.
    succ: Vec<(N, Cost)>,
    /// Predecessors of the expanded node.
    pred: Vec<(N, Cost)>,
    heuristic: Option<H>,
    start: Option<N>,
    goal: Option<N>,
    km: u64,
    computes: u64,
    status: DStarStatus,
    expanded: usize,
    max_nodes: usize,
    node_limit_hit: bool,
}

impl<N: Copy + Eq + Hash, H: Heuristic<N>> Default for DStarLite<N, H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<N: Copy + Eq + Hash, H: Heuristic<N>> DStarLite<N, H> {
    /// An empty planner.
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    /// An empty planner with memory for `nodes` nodes reserved up front.
    pub fn with_capacity(nodes: usize) -> Self {
        Self {
            index: NodeTable::with_capacity(nodes),
            records: Vec::with_capacity(nodes),
            open: BinaryHeap::with_capacity(nodes),
            succ: Vec::with_capacity(16),
            pred: Vec::with_capacity(16),
            heuristic: None,
            start: None,
            goal: None,
            km: 0,
            computes: 0,
            status: DStarStatus::Idle,
            expanded: 0,
            max_nodes: usize::MAX,
            node_limit_hit: false,
        }
    }

    /// Plans anew from `start` to `goal` and forgets all earlier work.
    /// Nothing is searched yet; call [`compute`](Self::compute).
    pub fn begin(&mut self, start: N, goal: N, heuristic: H) {
        self.index.clear();
        self.records.clear();
        self.open.clear();
        self.heuristic = Some(heuristic);
        self.start = Some(start);
        self.goal = Some(goal);
        self.km = 0;
        self.status = DStarStatus::Running;
        self.expanded = 0;
        self.node_limit_hit = false;

        self.index.insert_new(goal, 0);
        self.records.push(Record {
            node: goal,
            g: INFINITY,
            rhs: 0,
            queued: None,
        });
        let key = self.key(&self.records[0]);
        self.records[0].queued = Some(key);
        self.open.push(Entry { key, index: 0 });
    }

    /// Works on the plan, expanding at most `max_expansions` nodes. Returns
    /// [`DStarStatus::Running`] if the budget ran out before the cost of the
    /// current start was settled.
    pub fn compute<G: Graph<Node = N>>(
        &mut self,
        graph: &mut G,
        max_expansions: usize,
    ) -> DStarStatus {
        self.compute_traced(graph, max_expansions, &mut NoopSink)
    }

    /// As [`compute`](Self::compute), reporting every expansion to `sink`
    /// (cost: to the goal as known now).
    pub fn compute_traced<G: Graph<Node = N>, S: Sink<SearchEvent<N>>>(
        &mut self,
        graph: &mut G,
        max_expansions: usize,
        sink: &mut S,
    ) -> DStarStatus {
        let Some(start) = self.start else {
            return DStarStatus::Idle;
        };
        let search = u32::try_from(self.computes).unwrap_or(u32::MAX);
        self.computes += 1;
        if sink.enabled() {
            sink.emit(SearchEvent::Started {
                search,
                start,
                estimate: 0,
            });
        }
        let mut succ = std::mem::take(&mut self.succ);
        let mut pred = std::mem::take(&mut self.pred);

        // Cheap safety net: the walker may stand where nobody can move in
        // (a blocked grid cell), so nothing else would refresh its rhs.
        self.update_vertex(graph, start, &mut succ);

        let mut left = max_expansions;
        let settled = loop {
            let Some(&top) = self.open.peek() else {
                break true;
            };
            let i = top.index as usize;
            let record = self.records[i];
            if record.queued != Some(top.key) {
                self.open.pop(); // outdated entry
                continue;
            }
            let (start_key, start_consistent) = match self.index.get(&start) {
                Some(s) => {
                    let s = &self.records[s as usize];
                    (self.key(s), s.g == s.rhs)
                }
                None => ((INFINITY, INFINITY), true),
            };
            if top.key >= start_key && start_consistent {
                break true;
            }
            if left == 0 {
                break false;
            }

            let new_key = self.key(&record);
            if top.key < new_key {
                // computed with an older start; re-queue with the current key
                self.open.pop();
                self.records[i].queued = Some(new_key);
                self.open.push(Entry {
                    key: new_key,
                    index: top.index,
                });
                continue;
            }

            self.open.pop();
            self.records[i].queued = None;
            left -= 1;
            self.expanded += 1;
            if sink.enabled() {
                let cost = record.g.min(record.rhs);
                sink.emit(SearchEvent::Expanded {
                    search,
                    node: record.node,
                    cost: Cost::try_from(cost).unwrap_or(Cost::MAX),
                    estimate: Cost::try_from(top.key.0.saturating_sub(cost)).unwrap_or(Cost::MAX),
                });
            }
            if record.g > record.rhs {
                self.records[i].g = record.rhs;
            } else {
                self.records[i].g = INFINITY;
                self.update_vertex(graph, record.node, &mut succ);
            }
            pred.clear();
            graph.neighbors(record.node, &mut pred);
            for &(p, _) in &pred {
                self.update_vertex(graph, p, &mut succ);
            }
        };

        self.succ = succ;
        self.pred = pred;
        self.status = if !settled {
            DStarStatus::Running
        } else if self.g(start) == INFINITY {
            DStarStatus::Unreachable
        } else {
            DStarStatus::Found
        };
        if sink.enabled() {
            sink.emit(match self.status {
                DStarStatus::Running => SearchEvent::Paused {
                    search,
                    expanded: self.expanded,
                },
                status => SearchEvent::Finished {
                    search,
                    status: if status == DStarStatus::Found {
                        SearchStatus::Found
                    } else {
                        SearchStatus::Exhausted
                    },
                    expanded: self.expanded,
                },
            });
        }
        self.status
    }

    /// The walker is now at `new_start`. Earlier work stays valid; call
    /// [`compute`](Self::compute) before asking for the cost or the path.
    pub fn move_start(&mut self, new_start: N) {
        let (Some(old), Some(h)) = (self.start, self.heuristic.as_ref()) else {
            return;
        };
        if old != new_start {
            self.km = self
                .km
                .saturating_add(u64::from(h.estimate(old, new_start)));
            self.start = Some(new_start);
            if self.status != DStarStatus::Idle {
                self.status = DStarStatus::Running;
            }
        }
    }

    /// Tells the planner that the outgoing moves of `nodes` changed; `graph`
    /// already shows the new map. List every node with a changed move (see
    /// the [module docs](self)); then call [`compute`](Self::compute).
    pub fn nodes_changed<G: Graph<Node = N>>(
        &mut self,
        graph: &mut G,
        nodes: impl IntoIterator<Item = N>,
    ) {
        if self.start.is_none() {
            return;
        }
        let mut succ = std::mem::take(&mut self.succ);
        for node in nodes {
            self.update_vertex(graph, node, &mut succ);
        }
        self.succ = succ;
        self.status = DStarStatus::Running;
    }

    /// Writes a cheapest path from the current start to the goal (both
    /// included) into `out`: from the start, repeatedly the neighbour with the
    /// smallest move cost plus `g`. Returns `false`, leaving `out` empty, if
    /// there is no known path. Valid after [`compute`](Self::compute) returned
    /// [`DStarStatus::Found`].
    pub fn path<G: Graph<Node = N>>(&mut self, graph: &mut G, out: &mut Vec<N>) -> bool {
        out.clear();
        let (Some(start), Some(goal)) = (self.start, self.goal) else {
            return false;
        };
        if self.g(start) == INFINITY {
            return false;
        }
        let mut succ = std::mem::take(&mut self.succ);
        let mut current = start;
        out.push(current);
        // a path never visits a node twice, so this bounds broken graphs
        let mut steps_left = self.records.len();
        let ok = loop {
            if current == goal {
                break true;
            }
            if steps_left == 0 {
                break false;
            }
            steps_left -= 1;
            succ.clear();
            graph.neighbors(current, &mut succ);
            let mut best = (INFINITY, current);
            for &(next, step) in &succ {
                let total = self.g(next).saturating_add(u64::from(step));
                if total < best.0 {
                    best = (total, next);
                }
            }
            if best.0 == INFINITY {
                break false;
            }
            current = best.1;
            out.push(current);
        };
        self.succ = succ;
        if !ok {
            out.clear();
        }
        ok
    }

    /// Where the planner stands.
    pub fn status(&self) -> DStarStatus {
        self.status
    }

    /// The cheapest cost from the current start to the goal, once
    /// [`compute`](Self::compute) returned [`DStarStatus::Found`]; `None` if
    /// unreachable (or nothing is known yet).
    pub fn cost(&self) -> Option<u64> {
        self.start.and_then(|s| self.cost_from(s))
    }

    /// The known cost (`g`) from `node` to the goal; `None` if infinite or
    /// unknown. Exact for the current start after a finished compute; for
    /// other nodes it may be outdated.
    pub fn cost_from(&self, node: N) -> Option<u64> {
        let g = self.g(node);
        (g != INFINITY).then_some(g)
    }

    /// Nodes expanded since [`begin`](Self::begin).
    pub fn expanded(&self) -> usize {
        self.expanded
    }

    /// Keeps at most `nodes` nodes (default: no limit). Beyond it new nodes
    /// are left out: the plan may then miss ways and report
    /// [`DStarStatus::Unreachable`]; [`node_limit_hit`](Self::node_limit_hit)
    /// tells.
    pub fn set_max_nodes(&mut self, nodes: usize) {
        self.max_nodes = nodes;
    }

    /// `true` if a node was left out because of [`set_max_nodes`](Self::set_max_nodes).
    pub fn node_limit_hit(&self) -> bool {
        self.node_limit_hit
    }

    /// Every node the planner got to know since [`begin`](Self::begin).
    /// Nodes it does not know need no [`nodes_changed`](Self::nodes_changed):
    /// they are read from the current map when first reached.
    pub fn known_nodes(&self) -> impl Iterator<Item = N> + '_ {
        self.records.iter().map(|r| r.node)
    }

    /// Nodes the planner got to know.
    pub fn known(&self) -> usize {
        self.records.len()
    }

    /// The current start.
    pub fn start(&self) -> Option<N> {
        self.start
    }

    /// The goal.
    pub fn goal(&self) -> Option<N> {
        self.goal
    }

    #[inline]
    fn g(&self, node: N) -> u64 {
        self.index
            .get(&node)
            .map_or(INFINITY, |i| self.records[i as usize].g)
    }

    #[inline]
    fn key(&self, record: &Record<N>) -> Key {
        let m = record.g.min(record.rhs);
        if m == INFINITY {
            return (INFINITY, INFINITY);
        }
        let h = match (self.heuristic.as_ref(), self.start) {
            (Some(h), Some(start)) => u64::from(h.estimate(start, record.node)),
            _ => 0,
        };
        (m.saturating_add(h).saturating_add(self.km), m)
    }

    /// Recomputes `rhs` of `node` from its successors and (re)queues it if it
    /// is inconsistent.
    fn update_vertex<G: Graph<Node = N>>(
        &mut self,
        graph: &mut G,
        node: N,
        succ: &mut Vec<(N, Cost)>,
    ) {
        let index = if self.goal == Some(node) {
            match self.index.get(&node) {
                Some(i) => i as usize,
                None => return,
            }
        } else {
            succ.clear();
            graph.neighbors(node, succ);
            let mut rhs = INFINITY;
            for &(next, step) in succ.iter() {
                let g = self.g(next);
                if g != INFINITY {
                    rhs = rhs.min(g.saturating_add(u64::from(step)));
                }
            }
            match self.index.get(&node) {
                Some(i) => {
                    self.records[i as usize].rhs = rhs;
                    i as usize
                }
                None => {
                    if rhs == INFINITY {
                        return; // unknown nodes already count as g = rhs = INFINITY
                    }
                    if self.records.len() >= self.max_nodes.min(u32::MAX as usize) {
                        self.node_limit_hit = true;
                        return; // treated as unreachable: the plan may miss ways
                    }
                    let i = self.records.len();
                    self.index.insert_new(node, i as u32);
                    self.records.push(Record {
                        node,
                        g: INFINITY,
                        rhs,
                        queued: None,
                    });
                    i
                }
            }
        };
        let record = self.records[index];
        if record.g != record.rhs {
            let key = self.key(&record);
            if record.queued != Some(key) {
                self.records[index].queued = Some(key);
                self.open.push(Entry {
                    key,
                    index: index as u32,
                });
            }
        } else {
            self.records[index].queued = None;
        }
    }
}
