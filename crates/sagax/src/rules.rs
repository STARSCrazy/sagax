//! Ready-made pieces for a game's [`SquadRules`].
//!
//! [`path_cost`] is the walking cost between two cells, to use inside a
//! game's own [`SquadRules::member_cost`]; [`PathCost`] is a rules object
//! that does only that.
//!
//! ```
//! use sagax::prelude::*;
//! use sagax::decide::group::CostCtx;
//! use sagax::rules::path_cost;
//!
//! /// Members that cannot walk to the place cannot take the slot; the
//! /// others are ranked by the way they have to walk.
//! struct Walkers<'a> {
//!     nav: &'a mut Navigator,
//!     map: &'a mut DenseGrid,
//! }
//!
//! impl SquadRules for Walkers<'_> {
//!     fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
//!         path_cost(self.nav, self.map, ctx.member.pos, ctx.wish, Budget::UNLIMITED)
//!     }
//! }
//!
//! let mut map = DenseGrid::new(4, 4);
//! let mut nav = Navigator::new(Topology::Square(GridRules::preset(Preset::Octile)), 16);
//! let mut rules = Walkers { nav: &mut nav, map: &mut map };
//! # let _ = &mut rules as &mut dyn SquadRules;
//! ```

use sagax_decide::group::{CostCtx, SquadRules};
use sagax_nav::nav::Status;

use crate::prelude::{Budget, Cell, CellMap, Navigator, Pos};

/// The cost of the way from `from` to `to`, or `None` when there is none
/// (not reachable, or `budget` is used up first).
///
/// Positions are map cells `[x, y, level]`: on a hex map the game's offset
/// coordinates, which is what a hex [`Crew`](crate::prelude::Crew) hands its
/// rules. It runs a path search on `nav`, so the navigator's last search is
/// replaced.
pub fn path_cost<M: CellMap>(
    nav: &mut Navigator,
    map: &mut M,
    from: Pos,
    to: Pos,
    budget: Budget,
) -> Option<i128> {
    let report = nav.find_path(map, Cell::from(from), &[Cell::from(to)], budget);
    (report.status == Status::Found).then(|| i128::from(report.cost))
}

/// Rules that cost a member by the way it walks to the slot's place
/// ([`path_cost`]); every other slot is the built-in. A member that cannot
/// get there cannot take the slot.
pub struct PathCost<'a, M: CellMap> {
    /// The navigator the searches run on.
    pub nav: &'a mut Navigator,
    /// The map to walk on.
    pub map: &'a mut M,
    /// The work one search may do.
    pub budget: Budget,
}

impl<M: CellMap> SquadRules for PathCost<'_, M> {
    fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
        path_cost(self.nav, self.map, ctx.member.pos, ctx.wish, self.budget)
    }
}
