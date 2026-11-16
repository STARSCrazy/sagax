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

use sagax_decide::memory::Distance;
use sagax_nav::flow::FlowField;
use sagax_nav::search::{Cost, Limits};

use crate::prelude::{Cell, CellMap, Pos, Topology};

/// One cached flow field and the destination it is rooted at.
struct Slot {
    to: Option<Cell>,
    used: u64,
    field: FlowField<Cell>,
}

/// Walking distances from flow fields, one field per destination, a fixed
/// number of them cached (least recently used replaced).
///
/// [`FlowDistance::distance`] builds (or reuses) the field rooted at `to`
/// and reads the cost at `from`, so many members asking about the same
/// few destinations (the target, a wish point, a goal) cost one search per
/// new destination. The cost is turned into cells by dividing the sum by
/// the straight step cost (rounded down: on an octile grid a diagonal
/// counts 1.4 cells in the sum). A cell the field did not reach (no way,
/// or beyond its node capacity: a field of `capacity` cells reaches about
/// as far as a search of that many cells) is unreachable, so size the
/// capacity for the farthest walk that should count.
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
            slots: (0..slots.max(1))
                .map(|_| Slot {
                    to: None,
                    used: 0,
                    field: FlowField::with_capacity(capacity),
                })
                .collect(),
            clock: 0,
            searches: 0,
        }
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
                let mut graph = self.topology.graph(&mut *map, None);
                let limits = Limits {
                    max_nodes: self.capacity,
                    ..Limits::default()
                };
                self.slots[i].field.build(&mut graph, [to], limits);
                self.slots[i].to = Some(to);
                self.searches += 1;
                i
            }
        };
        let slot = &mut self.slots[i];
        slot.used = self.clock;
        slot.field.cost(from).map(|c| c / self.step)
    }

    /// This provider on `map`, as a [`Distance`] to lend a squad.
    pub fn on<'a, M: CellMap>(&'a mut self, map: &'a mut M) -> FlowDistanceOn<'a, M> {
        FlowDistanceOn { cache: self, map }
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
