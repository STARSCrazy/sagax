//! [`Navigator`]: the one object games and interfaces use for paths.
//!
//! It owns the rules (square or hex, costs), the search and all buffers. The
//! map is passed to each call, so a game can keep its own map and change it
//! between calls. Every call answers with a [`Report`]; ways are lists of
//! cells that always begin with the start.
//!
//! ```
//! use sagax_nav::grid::{Cell, DenseGrid, GridRules};
//! use sagax_nav::nav::{Navigator, Output, Status};
//! use sagax_nav::topology::Topology;
//!
//! let mut map = DenseGrid::new(20, 10);
//! let mut nav = Navigator::new(Topology::Square(GridRules::default()), 1000);
//! let report = nav.find_path(&mut map, Cell::new(0, 0, 0), Cell::new(15, 3, 0), true, usize::MAX);
//! assert_eq!(report.status, Some(Status::Found));
//! let corners = nav.output(&mut map, Output::Smooth);
//! assert_eq!(corners, [Cell::new(0, 0, 0), Cell::new(15, 3, 0)]);
//! ```

use sagax_core::NoopSink;

use crate::formation::Formation;
use crate::grid::{Cell, CellMap};
use crate::hex::HexMap;
use crate::search::{Cost, Limits, NoGoal, Search, SearchStatus};
use crate::topology::{AnyGoal, Topology};

/// How a call ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// The budget ran out; [`Navigator::resume`] goes on. A path call has
    /// already written the best way so far.
    Running,
    /// The goal was reached.
    Found,
    /// The goal cannot be reached; the way leads to the reachable cell
    /// closest to it.
    Partial,
    /// The goal cannot be reached and no cell is closer to it than the start.
    Unreachable,
    /// An area or field is complete.
    Done,
}

/// What a call did. `cells` counts the whole result (way or area), also
/// where an interface writes fewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Report {
    /// How the call ended.
    pub status: Option<Status>,
    /// Cost of the way (0 for areas).
    pub cost: Cost,
    /// Cells of the way (start included) or of the area.
    pub cells: usize,
    /// Cells expanded since the search began.
    pub expanded: usize,
    /// The node capacity was too small; the result may miss ways.
    pub node_limit_hit: bool,
}

/// What a way is given as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Output {
    /// Every cell.
    Cells,
    /// The start and every cell where the direction or level changes (the
    /// way is straight between them), ending at the last cell.
    Turns,
    /// The start and the corners of straight lines at any angle
    /// ([`Topology::smooth`]), ending at the last cell.
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Last {
    Nothing,
    Path,
    Area,
}

/// Path search, areas, smoothing and formations for one kind of grid.
pub struct Navigator {
    topology: Topology,
    capacity: usize,
    search: Search<Cell>,
    start: Cell,
    goal: Option<AnyGoal>,
    last: Last,
    keep_first: bool,
    smooth_max_extra: Cost,
    path: Vec<Cell>,
    list: Vec<Cell>,
    scratch: Vec<Cell>,
    members: Vec<Vec<Cell>>,
}

impl Navigator {
    /// A navigator for `topology` that searches at most `capacity` cells per
    /// search (all memory for them is reserved now).
    pub fn new(topology: Topology, capacity: usize) -> Self {
        Self {
            topology,
            capacity,
            search: Search::with_capacity(capacity),
            start: Cell::new(0, 0, 0),
            goal: None,
            last: Last::Nothing,
            keep_first: false,
            smooth_max_extra: 0,
            path: Vec::with_capacity(capacity),
            list: Vec::with_capacity(capacity),
            scratch: Vec::with_capacity(capacity),
            members: Vec::new(),
        }
    }

    /// The rules.
    pub fn topology(&self) -> Topology {
        self.topology
    }

    /// With `true` the first cell after the start is always kept in
    /// [`Output::Turns`] and [`Output::Smooth`] (for walkers who do not stand
    /// in the middle of their start cell, see `start_step` of the maps).
    pub fn set_keep_first(&mut self, keep: bool) {
        self.keep_first = keep;
    }

    /// Smoothing may cross cells whose extra move cost is at most `max_extra`
    /// (default 0: only cells without extra cost).
    pub fn set_smooth_max_extra(&mut self, max_extra: Cost) {
        self.smooth_max_extra = max_extra;
    }

    /// Searches a way from `start` to `goal` (on `goal.level`, or on any
    /// level with `any_level`), expanding at most `budget` cells. Afterwards
    /// [`path`](Self::path) and [`output`](Self::output) give the way: to the
    /// goal, or to the reachable cell closest to it.
    pub fn find_path<M: CellMap + HexMap>(
        &mut self,
        map: &mut M,
        start: Cell,
        goal: Cell,
        any_level: bool,
        budget: usize,
    ) -> Report {
        let goal = AnyGoal {
            x: goal.x,
            y: goal.y,
            level: (!any_level).then_some(goal.level),
            topology: self.topology,
        };
        let limits = Limits {
            max_nodes: self.capacity,
            ..Limits::default()
        };
        self.search.begin(start, &goal, limits);
        self.start = start;
        self.goal = Some(goal);
        self.last = Last::Path;
        self.run_path(map, budget)
    }

    /// Goes on with the last search for another `budget` cells (after
    /// [`Status::Running`]).
    pub fn resume<M: CellMap + HexMap>(&mut self, map: &mut M, budget: usize) -> Report {
        match self.last {
            Last::Path => self.run_path(map, budget),
            Last::Area => self.run_area(map, budget),
            Last::Nothing => Report::default(),
        }
    }

    /// Every cell reachable from `start` at a cost of at most `max_cost`,
    /// expanding at most `budget` cells. Afterwards [`area`](Self::area)
    /// lists them and [`path_to`](Self::path_to) gives the way to each.
    pub fn reachable<M: CellMap + HexMap>(
        &mut self,
        map: &mut M,
        start: Cell,
        max_cost: Cost,
        budget: usize,
    ) -> Report {
        let limits = Limits {
            max_nodes: self.capacity,
            max_cost,
        };
        self.search.begin(start, &NoGoal, limits);
        self.start = start;
        self.goal = None;
        self.last = Last::Area;
        self.path.clear();
        self.run_area(map, budget)
    }

    /// The cells of the last [`reachable`](Self::reachable) (or every cell a
    /// path search settled) with their cost from the start.
    pub fn area(&self) -> impl Iterator<Item = (Cell, Cost)> + '_ {
        self.search.settled()
    }

    /// Makes the way from the last search's start to `cell` the current way.
    /// Returns its cost, or `None` (way unchanged) if `cell` was not reached.
    pub fn path_to(&mut self, cell: Cell) -> Option<Cost> {
        let cost = self.search.settled_cost(cell)?;
        self.search.path_to(cell, &mut self.path);
        Some(cost)
    }

    /// The current way, every cell, start included.
    pub fn path(&self) -> &[Cell] {
        &self.path
    }

    /// The current way as `output`.
    pub fn output<M: CellMap + HexMap>(&mut self, map: &mut M, output: Output) -> &[Cell] {
        let way = WayOptions {
            keep_first: self.keep_first,
            smooth_max_extra: self.smooth_max_extra,
        };
        if output == Output::Cells {
            return &self.path;
        }
        write_way(
            self.topology,
            map,
            &self.path,
            output,
            way,
            &mut self.scratch,
            &mut self.list,
        );
        &self.list
    }

    /// Places the members of `formation` along the current way (the
    /// leader's): `result[k][i]` is member `k`'s cell while the leader stands
    /// on `path()[i]`. See [`Topology::place_members`].
    pub fn formation<M: CellMap + HexMap>(
        &mut self,
        map: &mut M,
        formation: &Formation,
    ) -> &[Vec<Cell>] {
        self.topology
            .place_members(map, &self.path, formation, &mut self.members);
        &self.members
    }

    /// The member cells of the last [`formation`](Self::formation).
    pub fn members(&self) -> &[Vec<Cell>] {
        &self.members
    }

    fn run_path<M: CellMap + HexMap>(&mut self, map: &mut M, budget: usize) -> Report {
        let Some(goal) = self.goal else {
            return Report::default();
        };
        let mut graph = self.topology.graph(&mut *map, Some(self.start));
        let status = self
            .search
            .advance(&mut graph, &goal, budget, &mut NoopSink);
        self.search.best_path(&mut self.path);
        let status = match status {
            SearchStatus::Found => Status::Found,
            SearchStatus::Running => Status::Running,
            SearchStatus::Idle | SearchStatus::Exhausted if self.path.len() > 1 => Status::Partial,
            SearchStatus::Idle | SearchStatus::Exhausted => Status::Unreachable,
        };
        Report {
            status: Some(status),
            cost: self.search.best().map_or(0, |(_, c)| c),
            cells: self.path.len(),
            expanded: self.search.expanded(),
            node_limit_hit: self.search.node_limit_hit(),
        }
    }

    fn run_area<M: CellMap + HexMap>(&mut self, map: &mut M, budget: usize) -> Report {
        let mut graph = self.topology.graph(&mut *map, Some(self.start));
        let status = self
            .search
            .advance(&mut graph, &NoGoal, budget, &mut NoopSink);
        Report {
            status: Some(if status == SearchStatus::Running {
                Status::Running
            } else {
                Status::Done
            }),
            cost: 0,
            cells: self.search.settled().count(),
            expanded: self.search.expanded(),
            node_limit_hit: self.search.node_limit_hit(),
        }
    }
}

/// How [`write_way`] treats the start and smoothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WayOptions {
    /// Always keep the first cell after the start (see
    /// [`Navigator::set_keep_first`]).
    pub keep_first: bool,
    /// Largest extra move cost smoothing may cross.
    pub smooth_max_extra: Cost,
}

/// Writes `path` (start first) as `output` into `out`; the list always
/// begins with the start and ends at the last cell. `scratch` is reused
/// memory.
pub fn write_way<M: CellMap + HexMap>(
    topology: Topology,
    map: &mut M,
    path: &[Cell],
    output: Output,
    options: WayOptions,
    scratch: &mut Vec<Cell>,
    out: &mut Vec<Cell>,
) {
    out.clear();
    match output {
        Output::Cells => out.extend_from_slice(path),
        Output::Turns => {
            topology.turning_points(path, options.keep_first, scratch);
            out.extend(path.first());
            out.extend_from_slice(scratch);
        }
        Output::Smooth => {
            let skip = usize::from(options.keep_first && path.len() > 2);
            topology.smooth(map, &path[skip..], options.smooth_max_extra, scratch);
            out.extend(path.first().filter(|_| skip == 1));
            out.extend_from_slice(scratch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{DenseGrid, GridRules};
    use crate::hex::HexLayout;

    fn square() -> Navigator {
        Navigator::new(Topology::Square(GridRules::default()), 10_000)
    }

    fn wall(map: &mut DenseGrid, x: i32, from: i32, to: i32) {
        for y in from..to {
            map.set(x, y, DenseGrid::BLOCKED);
        }
    }

    #[test]
    fn every_output_begins_with_the_start() {
        let mut map = DenseGrid::new(20, 20);
        wall(&mut map, 10, 0, 15);
        let mut nav = square();
        let start = Cell::new(2, 2, 0);
        let report = nav.find_path(&mut map, start, Cell::new(18, 2, 0), false, usize::MAX);
        assert_eq!(report.status, Some(Status::Found));
        assert_eq!(report.cells, nav.path().len());
        for output in [Output::Cells, Output::Turns, Output::Smooth] {
            let list = nav.output(&mut map, output).to_vec();
            assert_eq!(list.first(), Some(&start), "{output:?}");
            assert_eq!(list.last(), Some(&Cell::new(18, 2, 0)), "{output:?}");
        }
        let smooth = nav.output(&mut map, Output::Smooth).len();
        let turns = nav.output(&mut map, Output::Turns).len();
        assert!(smooth <= turns && turns < report.cells);
    }

    #[test]
    fn small_budgets_end_like_one_big_one() {
        let mut map = DenseGrid::new(30, 30);
        wall(&mut map, 15, 0, 25);
        let (start, goal) = (Cell::new(1, 1, 0), Cell::new(28, 1, 0));
        let mut whole = square();
        let expected = whole.find_path(&mut map, start, goal, false, usize::MAX);
        let mut nav = square();
        let mut report = nav.find_path(&mut map, start, goal, false, 7);
        let mut calls = 1;
        while report.status == Some(Status::Running) {
            report = nav.resume(&mut map, 7);
            calls += 1;
        }
        assert!(calls > 10);
        assert_eq!(report, expected);
        assert_eq!(nav.path(), whole.path());
    }

    #[test]
    fn blocked_goal_gives_partial_way_and_closed_start_unreachable() {
        let mut map = DenseGrid::new(20, 5);
        wall(&mut map, 10, 0, 5);
        let mut nav = square();
        let report = nav.find_path(
            &mut map,
            Cell::new(1, 2, 0),
            Cell::new(18, 2, 0),
            false,
            usize::MAX,
        );
        assert_eq!(report.status, Some(Status::Partial));
        assert_eq!(nav.path().last(), Some(&Cell::new(9, 2, 0)));

        let mut boxed = DenseGrid::new(3, 3);
        for (x, y) in [
            (0, 0),
            (1, 0),
            (2, 0),
            (0, 1),
            (2, 1),
            (0, 2),
            (1, 2),
            (2, 2),
        ] {
            boxed.set(x, y, DenseGrid::BLOCKED);
        }
        let report = nav.find_path(
            &mut boxed,
            Cell::new(1, 1, 0),
            Cell::new(5, 5, 0),
            false,
            usize::MAX,
        );
        assert_eq!(report.status, Some(Status::Unreachable));
        assert_eq!(nav.path(), [Cell::new(1, 1, 0)]);
    }

    #[test]
    fn area_and_way_to_each_cell() {
        let mut map = DenseGrid::new(10, 10);
        let mut nav = square();
        let report = nav.reachable(&mut map, Cell::new(5, 5, 0), 20, usize::MAX);
        assert_eq!(report.status, Some(Status::Done));
        assert_eq!(report.cells, nav.area().count());
        assert!(nav.area().all(|(_, cost)| cost <= 20));
        assert_eq!(nav.path_to(Cell::new(7, 5, 0)), Some(20));
        assert_eq!(nav.path().len(), 3);
        assert_eq!(nav.path_to(Cell::new(9, 9, 0)), None);
        assert_eq!(nav.path().len(), 3);
    }

    #[test]
    fn hex_paths_and_formation() {
        let mut map = DenseGrid::new(20, 20);
        wall(&mut map, 10, 0, 15);
        for layout in [
            HexLayout::OddR,
            HexLayout::EvenR,
            HexLayout::OddQ,
            HexLayout::EvenQ,
        ] {
            let mut nav = Navigator::new(
                Topology::Hex {
                    layout,
                    step_cost: 10,
                },
                10_000,
            );
            let start = Cell::new(2, 2, 0);
            let report = nav.find_path(&mut map, start, Cell::new(18, 2, 0), false, usize::MAX);
            assert_eq!(report.status, Some(Status::Found), "{layout:?}");
            let smooth = nav.output(&mut map, Output::Smooth).to_vec();
            assert_eq!(smooth.first(), Some(&start));
            let cells = nav.path().len();
            let members = nav.formation(&mut map, &Formation::wedge_hex(3));
            assert_eq!(members.len(), 3);
            assert!(members.iter().all(|m| m.len() == cells));
        }
    }
}
