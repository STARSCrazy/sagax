//! Walking distance for the squad: [`FlowDistance`] answers "how far does
//! a figure have to walk" from flow fields, cached by destination.
//!
//! The squad asks a [`Distance`] through its rules
//! ([`SquadRules::walk`](crate::decide::group::SquadRules::walk));
//! [`Walking`](crate::decide::group::Walking) lends one to the built-in
//! rules or the game's own.
//!
//! ```
//! use sagax::prelude::*;
//!
//! // A wall at x = 5, open at y = 9.
//! let mut map = DenseGrid::new(10, 10);
//! for y in 0..=8 {
//!     map.set(5, y, DenseGrid::BLOCKED);
//! }
//! let topology = Topology::Square(GridRules::preset(Preset::Octile));
//! let mut fd = FlowDistance::new(topology, FlowDistance::DEFAULT_SLOTS, 100);
//! // Two cells as the crow flies, round the wall it is 20.
//! assert_eq!(fd.distance(&mut map, [4, 0, 0], [6, 0, 0]), Some(20));
//! // The same destination again: no new search.
//! assert_eq!(fd.distance(&mut map, [4, 1, 0], [6, 0, 0]), Some(19));
//! assert_eq!(fd.searches(), 1);
//!
//! // For a squad or crew: the provider on the map, lent through the rules.
//! let mut walk = fd.on(&mut map);
//! let _rules = Walking::new(&mut walk);
//! ```

use std::ops::RangeInclusive;

use sagax_decide::memory::Distance;
use sagax_nav::flow::FlowField;
use sagax_nav::grid::Moves;
use sagax_nav::search::{Cost, Limits};
use sagax_nav::{ReverseBuffers, Reversed};

use crate::prelude::{Cell, CellMap, Pos, Topology};

/// One cached flow field and the destination it is rooted at.
struct Slot {
    to: Option<Cell>,
    /// The destination cannot be entered: the field is rooted at its
    /// neighbours and every distance is one cell more.
    beside: bool,
    used: u64,
    field: FlowField<Cell>,
}

/// Walking distances from flow fields, one field per destination, a fixed
/// number of them cached (least recently used replaced).
///
/// [`FlowDistance::distance`] builds (or reuses) the field rooted at `to`
/// and reads the cost at `from`, so many members asking about the same
/// few destinations (the target, a wish point, a goal) cost one search per
/// new destination. The field is built over the map's moves turned round
/// ([`Reversed`]), so the cost at `from` is the cost of walking from
/// `from` to `to`: what a path search from `from` finds (`find_path` on the
/// topology's graph without a start cell; `start_step` is not used), also
/// where moves cost differently in the two directions (the cell entered,
/// leave costs of zones, one-way walls). The cell `from` stands on is only
/// left, never entered, so a figure on a cell the map blocks (occupied by
/// itself) still walks. A destination that cannot be entered (occupied by
/// the enemy, blocked) is walked up to: the field is rooted at the cells a
/// real move would enter it from, were it free
/// ([`CellMap::step_unblocked`]: walls, one-way walls, links between
/// levels and corners still count), and the distance is one cell more than
/// to the nearest of them; a path search would call it unreachable.
/// `from == to` is 0.
///
/// The cost is turned into cells by dividing the sum by the straight step
/// cost (rounded down: on an octile grid a diagonal counts 1.4 cells in
/// the sum). A cell the field did not reach (no way, or beyond its node
/// capacity: a field of `capacity` cells reaches about as far as a search
/// of that many cells) is unreachable, so size the capacity for the
/// farthest walk that should count. Moves into a cell are looked for from
/// its neighbours on its own level and the levels next to it (one up, one
/// down), so ordinary stairs and ramps need no setup; a map whose moves
/// jump further between levels names them ([`FlowDistance::with_levels`]).
///
/// **Cost.** The map only knows moves forward, so a field asks every
/// neighbour of a cell for its moves: a field costs some 25 to 70 times the
/// map questions of a plain flow field of the same size. Give it the
/// capacity it needs and few destinations (the target, a wish point), not
/// one per member.
///
/// Positions are map cells `[x, y, level]`: on a hex map the game's offset
/// coordinates, which is what a hex [`Crew`](crate::prelude::Crew) hands
/// its rules (a plain [`Squad`](crate::prelude::Squad) on hex keeps axial
/// ones and the game converts). Call [`FlowDistance::invalidate`] when
/// the map changes. Nothing is allocated after [`FlowDistance::new`]
/// beyond what the flow fields need for their capacity.
pub struct FlowDistance {
    topology: Topology,
    capacity: usize,
    step: Cost,
    levels: Option<RangeInclusive<i32>>,
    buffers: ReverseBuffers<Cell>,
    moves: Vec<(Cell, Cost)>,
    roots: Vec<Cell>,
    slots: Vec<Slot>,
    clock: u64,
    searches: u64,
}

impl FlowDistance {
    /// The number of cached fields of the spec's default.
    pub const DEFAULT_SLOTS: usize = 4;

    /// A provider for `topology` keeping `slots` fields (0: 1), each of at
    /// most `capacity` cells.
    pub fn new(topology: Topology, slots: usize, capacity: usize) -> Self {
        let step = match topology {
            Topology::Square(rules) => rules.straight,
            Topology::Hex { step_cost, .. } => step_cost,
        }
        .max(1);
        Self {
            topology,
            capacity,
            step,
            levels: None,
            buffers: ReverseBuffers::with_capacity(48, 8),
            moves: Vec::with_capacity(8),
            roots: Vec::with_capacity(48),
            slots: (0..slots.max(1))
                .map(|_| Slot {
                    to: None,
                    beside: false,
                    used: 0,
                    field: FlowField::with_capacity(capacity),
                })
                .collect(),
            clock: 0,
            searches: 0,
        }
    }

    /// The most levels [`with_levels`](Self::with_levels) looks at.
    pub const MAX_LEVELS: i32 = 64;

    /// The levels a move may arrive from, instead of the default (the
    /// cell's own level and one up and one down): a field looks for moves
    /// into a cell from its neighbours on the cell's own level and on
    /// these. At most [`MAX_LEVELS`](Self::MAX_LEVELS): a wider range is cut
    /// to its first 64 levels. Forgets every field.
    pub fn with_levels(mut self, levels: RangeInclusive<i32>) -> Self {
        self.set_levels(levels);
        self
    }

    /// Sets the levels as [`with_levels`](Self::with_levels).
    pub fn set_levels(&mut self, levels: RangeInclusive<i32>) {
        let (lo, hi) = levels.into_inner();
        let hi = hi.min(lo.saturating_add(Self::MAX_LEVELS - 1));
        self.levels = Some(lo..=hi);
        self.invalidate();
    }

    /// Back to the default levels: the cell's own and one up and one down.
    /// Forgets every field.
    pub fn levels_around(&mut self) {
        self.levels = None;
        self.invalidate();
    }

    /// Forgets every field (call it when the map changed).
    pub fn invalidate(&mut self) {
        for s in &mut self.slots {
            s.to = None;
            s.used = 0;
        }
    }

    /// The searches run so far (one per field built).
    pub fn searches(&self) -> u64 {
        self.searches
    }

    /// True when a field rooted at `to` is cached.
    pub fn cached(&self, to: Pos) -> bool {
        let to = Cell::from(to);
        self.slots.iter().any(|s| s.to == Some(to))
    }

    /// The walking distance from `from` to `to` on `map` in cells, or
    /// `None` when there is no way (within the field's capacity).
    pub fn distance<M: CellMap>(&mut self, map: &mut M, from: Pos, to: Pos) -> Option<u32> {
        self.clock += 1;
        let (from, to) = (Cell::from(from), Cell::from(to));
        if from == to {
            return Some(0);
        }
        let i = match self.slots.iter().position(|s| s.to == Some(to)) {
            Some(i) => i,
            None => {
                // An empty slot first (lowest index), else the least
                // recently used.
                let i = self
                    .slots
                    .iter()
                    .enumerate()
                    .min_by_key(|(i, s)| (s.to.is_some(), s.used, *i))
                    .map_or(0, |(i, _)| i);
                // Unclaimed while it is built: a panic in the map leaves no
                // half-built field behind a destination.
                self.slots[i].to = None;
                self.build(i, map, to);
                self.slots[i].to = Some(to);
                i
            }
        };
        let slot = &mut self.slots[i];
        slot.used = self.clock;
        // `from` is only left: a walker on a cell nobody enters (blocked
        // under itself) takes its first move forward.
        let mut graph = self.topology.graph(&mut *map, None);
        let cost = slot.field.cost_from(&mut graph, from, &mut self.moves)?;
        let cells = cost / self.step;
        Some(if slot.beside {
            cells.saturating_add(1)
        } else {
            cells
        })
    }

    /// Builds slot `i`'s field over the reversed graph: rooted at `to`,
    /// or, if nothing moves into `to`, at the cells a move would enter it
    /// from were it free.
    fn build<M: CellMap>(&mut self, i: usize, map: &mut M, to: Cell) {
        let (topology, levels) = (self.topology, self.levels.clone());
        let to_levels = window(&levels, to);
        let mut graph = Reversed::with_buffers(
            topology.graph(&mut *map, None),
            move |c: Cell, out: &mut Vec<Cell>| topology.predecessors(c, window(&levels, c), out),
            std::mem::take(&mut self.buffers),
        );
        let beside = !graph.enterable(to);
        self.roots.clear();
        if beside {
            // The cells a real move would enter `to` from, were it free.
            topology.predecessors(to, to_levels, &mut self.roots);
            let map = graph.graph.map_mut();
            self.roots
                .retain(|&q| moves_into(topology, &mut **map, q, to));
        } else {
            self.roots.push(to);
        }
        let limits = Limits {
            max_nodes: self.capacity,
            ..Limits::default()
        };
        let slot = &mut self.slots[i];
        slot.field
            .build(&mut graph, self.roots.iter().copied(), limits);
        slot.beside = beside;
        self.searches += 1;
        self.buffers = graph.into_parts().1;
    }

    /// This provider on `map`, as a [`Distance`] to lend a squad.
    pub fn on<'a, M: CellMap>(&'a mut self, map: &'a mut M) -> FlowDistanceOn<'a, M> {
        FlowDistanceOn { cache: self, map }
    }
}

/// The levels moves into `c` are looked for on: the set ones, else `c`'s
/// own and one up and one down.
fn window(levels: &Option<RangeInclusive<i32>>, c: Cell) -> RangeInclusive<i32> {
    match levels {
        Some(l) => l.clone(),
        None => c.level.saturating_sub(1)..=c.level.saturating_add(1),
    }
}

/// Whether a move from `q` would enter `to` if `to` were free
/// ([`CellMap::step_unblocked`]), with the topology's rules: four or eight
/// ways, no cutting corners of walls.
fn moves_into<M: CellMap>(topology: Topology, map: &mut M, q: Cell, to: Cell) -> bool {
    let (dx, dy) = (to.x - q.x, to.y - q.y);
    if map.step_unblocked(q, dx, dy) != Some(to.level) {
        return false;
    }
    match topology {
        Topology::Square(rules) if dx != 0 && dy != 0 => {
            rules.moves == Moves::Eight
                && (rules.cut_corners
                    || map.step(q, dx, 0).is_some() && map.step(q, 0, dy).is_some())
        }
        _ => true,
    }
}

/// A [`FlowDistance`] on a map ([`FlowDistance::on`]).
pub struct FlowDistanceOn<'a, M> {
    cache: &'a mut FlowDistance,
    map: &'a mut M,
}

impl<M: CellMap> Distance for FlowDistanceOn<'_, M> {
    fn distance(&mut self, from: Pos, to: Pos) -> Option<u32> {
        self.cache.distance(self.map, from, to)
    }
}
