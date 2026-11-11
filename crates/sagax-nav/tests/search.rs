//! Correctness tests of the search core and the grid adapter.

use sagax_core::{NoopSink, Sink};
use sagax_debug::{Collector, Overflow};
use sagax_nav::grid::{Cell, CellGoal, CellMap, DenseGrid, GridGraph, GridRules, Moves};
use sagax_nav::{Cost, Goal, Graph, Limits, NoGoal, Search, SearchEvent, SearchStatus, find_path};

/// Small deterministic random numbers for test maps.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

fn random_grid(rng: &mut Rng, w: i32, h: i32, wall_percent: u32, weighted: bool) -> DenseGrid {
    let mut g = DenseGrid::new(w, h);
    for y in 0..h {
        for x in 0..w {
            if rng.below(100) < wall_percent {
                g.set(x, y, DenseGrid::BLOCKED);
            } else if weighted {
                g.set(x, y, rng.below(4) * 7);
            }
        }
    }
    g
}

/// Plain Dijkstra over all nodes, written independently of the library: the
/// cheapest cost from start to every reachable node.
fn reference_costs<G: Graph<Node = Cell>>(
    graph: &mut G,
    start: Cell,
) -> std::collections::HashMap<Cell, Cost> {
    let mut best = std::collections::HashMap::new();
    let mut done = std::collections::HashSet::new();
    best.insert(start, 0u32);
    let mut buf = Vec::new();
    loop {
        let next = best
            .iter()
            .filter(|(n, _)| !done.contains(*n))
            .min_by_key(|(n, c)| (**c, **n))
            .map(|(n, c)| (*n, *c));
        let Some((node, cost)) = next else { break };
        done.insert(node);
        buf.clear();
        graph.neighbors(node, &mut buf);
        for &(n, step) in &buf {
            let c = cost + step;
            if best.get(&n).is_none_or(|&old| c < old) {
                best.insert(n, c);
            }
        }
    }
    best
}

/// Checks that `path` starts at `start`, each step is a move the graph offers,
/// and the step costs add up to `cost`.
fn assert_valid_path<G: Graph<Node = Cell>>(graph: &mut G, path: &[Cell], start: Cell, cost: Cost) {
    assert_eq!(path.first(), Some(&start));
    let mut sum = 0;
    let mut buf = Vec::new();
    for pair in path.windows(2) {
        buf.clear();
        graph.neighbors(pair[0], &mut buf);
        let step = buf
            .iter()
            .filter(|(n, _)| *n == pair[1])
            .map(|(_, c)| *c)
            .min()
            .unwrap_or_else(|| panic!("illegal step {:?} -> {:?}", pair[0], pair[1]));
        sum += step;
    }
    assert_eq!(sum, cost, "step costs do not add up");
}

fn rules8() -> GridRules {
    GridRules::default()
}

#[test]
fn start_is_goal() {
    let mut g = GridGraph::new(DenseGrid::new(3, 3), rules8(), None);
    let goal = CellGoal::new(1, 1, Some(0), rules8());
    let r = find_path(
        &mut g,
        Cell::new(1, 1, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert!(r.reached());
    assert_eq!(r.path, [Cell::new(1, 1, 0)]);
    assert_eq!(r.cost, 0);
}

#[test]
fn single_cell_map() {
    let mut g = GridGraph::new(DenseGrid::new(1, 1), rules8(), None);
    let goal = CellGoal::new(0, 0, None, rules8());
    let r = find_path(
        &mut g,
        Cell::new(0, 0, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert!(r.reached());
}

#[test]
fn empty_map_and_goal_off_map() {
    let mut g = GridGraph::new(DenseGrid::new(0, 0), rules8(), None);
    let goal = CellGoal::new(5, 5, None, rules8());
    let r = find_path(
        &mut g,
        Cell::new(0, 0, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert_eq!(r.status, SearchStatus::Exhausted);
    assert_eq!(r.path, [Cell::new(0, 0, 0)]);
}

#[test]
fn straight_and_diagonal_costs() {
    let mut g = GridGraph::new(DenseGrid::new(10, 10), rules8(), None);
    let goal = CellGoal::new(6, 3, Some(0), rules8());
    let r = find_path(
        &mut g,
        Cell::new(0, 0, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert!(r.reached());
    assert_eq!(r.cost, 3 * 14 + 3 * 10);
    assert_eq!(r.path.len(), 7);
}

#[test]
fn four_moves_use_manhattan() {
    let rules = GridRules {
        moves: Moves::Four,
        ..rules8()
    };
    let mut g = GridGraph::new(DenseGrid::new(10, 10), rules, None);
    let goal = CellGoal::new(3, 4, None, rules);
    let r = find_path(
        &mut g,
        Cell::new(0, 0, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert_eq!(r.cost, 70);
    for pair in r.path.windows(2) {
        assert_eq!(
            (pair[1].x - pair[0].x).abs() + (pair[1].y - pair[0].y).abs(),
            1
        );
    }
}

#[test]
fn no_cut_corners() {
    // . #
    // . .   a diagonal from (0,0) to (1,1) would cut the wall at (1,0)
    let mut map = DenseGrid::new(2, 2);
    map.set(1, 0, DenseGrid::BLOCKED);
    let mut g = GridGraph::new(map.clone(), rules8(), None);
    let goal = CellGoal::new(1, 1, None, rules8());
    let r = find_path(
        &mut g,
        Cell::new(0, 0, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert_eq!(r.cost, 20);

    let cut = GridRules {
        cut_corners: true,
        ..rules8()
    };
    let mut g = GridGraph::new(map, cut, None);
    let r = find_path(
        &mut g,
        Cell::new(0, 0, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert_eq!(r.cost, 14);
}

#[test]
fn enclosed_goal_is_unreachable_and_gives_best_partial() {
    let mut map = DenseGrid::new(9, 9);
    for (x, y) in [
        (5, 3),
        (6, 3),
        (7, 3),
        (5, 4),
        (7, 4),
        (5, 5),
        (6, 5),
        (7, 5),
    ] {
        map.set(x, y, DenseGrid::BLOCKED);
    }
    let mut g = GridGraph::new(map, rules8(), None);
    let goal = CellGoal::new(6, 4, None, rules8());
    let r = find_path(
        &mut g,
        Cell::new(0, 4, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert_eq!(r.status, SearchStatus::Exhausted);
    // the closest open cell to (6,4) is (4,4), two cells away
    assert_eq!(r.path.last(), Some(&Cell::new(4, 4, 0)));
    assert_eq!(r.cost, 40);
}

#[test]
fn budget_end_gives_best_partial() {
    let mut g = GridGraph::new(DenseGrid::new(100, 1), rules8(), None);
    let goal = CellGoal::new(99, 0, None, rules8());
    let r = find_path(&mut g, Cell::new(0, 0, 0), &goal, Limits::default(), 10);
    assert_eq!(r.status, SearchStatus::Running);
    assert_eq!(r.expanded, 10);
    assert_eq!(r.path.last(), Some(&Cell::new(9, 0, 0)));
}

#[test]
fn weighted_cells_are_avoided() {
    // a cheap detour around an expensive cell
    let mut map = DenseGrid::new(3, 3);
    map.set(1, 1, 1000);
    let mut g = GridGraph::new(map, rules8(), None);
    let goal = CellGoal::new(2, 1, None, rules8());
    let r = find_path(
        &mut g,
        Cell::new(0, 1, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert_eq!(r.cost, 28);
    assert!(!r.path.contains(&Cell::new(1, 1, 0)));
}

#[test]
fn leaving_an_enemy_reach_is_avoided_when_cheaper() {
    // the start lies in an enemy's reach; leaving it costs 100. The goal lies
    // inside the reach too, so a way inside it beats one around it.
    let mut map = DenseGrid::new(5, 3);
    map.set_leave_cost(100);
    for x in 0..4 {
        for y in 0..3 {
            map.set_zones(x, y, 1);
        }
    }
    let mut g = GridGraph::new(map, rules8(), None);
    let goal = CellGoal::new(3, 1, None, rules8());
    let r = find_path(
        &mut g,
        Cell::new(0, 1, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert_eq!(r.cost, 30);
    let goal = CellGoal::new(4, 1, None, rules8());
    let r = find_path(
        &mut g,
        Cell::new(0, 1, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert_eq!(r.cost, 140);
}

#[test]
fn cost_overflow_is_detected_not_wrapped() {
    struct Huge;
    impl Graph for Huge {
        type Node = u32;
        fn neighbors(&mut self, node: u32, out: &mut Vec<(u32, Cost)>) {
            if node < 3 {
                out.push((node + 1, Cost::MAX / 2 + 1));
            }
        }
    }
    struct To3;
    impl Goal<u32> for To3 {
        fn is_goal(&self, n: u32) -> bool {
            n == 3
        }
        fn estimate(&self, _: u32) -> Cost {
            0
        }
    }
    let mut s = Search::new();
    s.begin(0, &To3, Limits::default());
    assert_eq!(s.run(&mut Huge, &To3), SearchStatus::Exhausted);
    assert!(s.cost_overflow());
}

#[test]
fn node_limit_bounds_memory() {
    let mut g = GridGraph::new(DenseGrid::new(50, 50), rules8(), None);
    let goal = CellGoal::new(49, 49, None, rules8());
    let mut s = Search::new();
    s.begin(
        Cell::new(0, 0, 0),
        &goal,
        Limits {
            max_nodes: 100,
            ..Limits::default()
        },
    );
    s.run(&mut g, &goal);
    assert!(s.known() <= 100);
    assert!(s.node_limit_hit());
}

#[test]
fn reachable_within_cost() {
    let mut g = GridGraph::new(DenseGrid::new(20, 20), rules8(), None);
    let mut s = Search::new();
    s.begin(
        Cell::new(10, 10, 0),
        &NoGoal,
        Limits {
            max_cost: 20,
            ..Limits::default()
        },
    );
    assert_eq!(s.run(&mut g, &NoGoal), SearchStatus::Exhausted);
    let cells: Vec<_> = s.settled().collect();
    // cost <= 20: the 3x3 block (straight 10, diagonal 14) plus 2 straight in each axis direction
    assert_eq!(cells.len(), 9 + 4);
    assert!(cells.iter().all(|&(_, c)| c <= 20));
    assert_eq!(s.settled_cost(Cell::new(12, 10, 0)), Some(20));
    assert_eq!(s.settled_cost(Cell::new(12, 11, 0)), None);
    let mut path = Vec::new();
    assert!(s.path_to(Cell::new(12, 10, 0), &mut path));
    assert_eq!(path.len(), 3);
}

/// A map with two levels: level 0 everywhere, level 1 on a bridge over row 2
/// reached by stairs at both ends. Below the bridge, row 2 is a river.
struct BridgeMap;

impl CellMap for BridgeMap {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let (x, y) = (from.x + dx, from.y + dy);
        if !(0..10).contains(&x) || !(0..5).contains(&y) {
            return None;
        }
        // bridge: column 5, rows 1..=3, level 1; stairs: entering (5,1) from (5,0), (5,3) from (5,4)
        match from.level {
            0 => {
                if x == 5 && (y == 1 && from.y == 0 && dx == 0 || y == 3 && from.y == 4 && dx == 0)
                {
                    return Some(1);
                }
                if y == 2 || (x == 5 && (y == 1 || y == 3)) {
                    return None; // river, bridge piers
                }
                Some(0)
            }
            _ => {
                if x != 5 {
                    return None;
                }
                Some(if y == 0 || y == 4 { 0 } else { 1 })
            }
        }
    }
}

#[test]
fn bridge_over_river_uses_levels() {
    let mut g = GridGraph::new(BridgeMap, rules8(), None);
    let goal = CellGoal::new(5, 4, Some(0), rules8());
    let r = find_path(
        &mut g,
        Cell::new(5, 0, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert!(r.reached());
    let levels: Vec<_> = r.path.iter().map(|c| c.level).collect();
    assert_eq!(levels, [0, 1, 1, 1, 0]);
    assert_eq!(r.cost, 40);
    assert_valid_path(&mut g, &r.path, Cell::new(5, 0, 0), r.cost);
}

#[test]
fn goal_on_any_level() {
    let mut g = GridGraph::new(BridgeMap, rules8(), None);
    let goal = CellGoal::new(5, 2, None, rules8());
    let r = find_path(
        &mut g,
        Cell::new(5, 0, 0),
        &goal,
        Limits::default(),
        usize::MAX,
    );
    assert!(r.reached());
    assert_eq!(r.path.last(), Some(&Cell::new(5, 2, 1)));
}

#[test]
fn start_step_is_used_only_at_start() {
    // from the start only the move to the right is allowed
    struct OnlyRightAtStart;
    impl CellMap for OnlyRightAtStart {
        fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
            let (x, y) = (from.x + dx, from.y + dy);
            ((0..5).contains(&x) && (0..5).contains(&y)).then_some(0)
        }
        fn start_step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
            if (dx, dy) == (1, 0) {
                self.step(from, dx, dy)
            } else {
                None
            }
        }
    }
    let start = Cell::new(2, 2, 0);
    let mut g = GridGraph::new(OnlyRightAtStart, rules8(), Some(start));
    let goal = CellGoal::new(2, 0, None, rules8());
    let r = find_path(&mut g, start, &goal, Limits::default(), usize::MAX);
    assert_eq!(r.path[1], Cell::new(3, 2, 0));
    assert_eq!(r.cost, 10 + 14 + 10);
}

#[test]
fn astar_matches_reference_on_random_maps() {
    let mut rng = Rng(7);
    for round in 0..300 {
        let w = 1 + rng.below(16) as i32;
        let h = 1 + rng.below(16) as i32;
        let weighted = round % 2 == 0;
        let walls = rng.below(40);
        let map = random_grid(&mut rng, w, h, walls, weighted);
        let rules = if round % 3 == 0 {
            GridRules {
                moves: Moves::Four,
                ..rules8()
            }
        } else {
            GridRules {
                cut_corners: round % 5 == 0,
                ..rules8()
            }
        };
        let start = Cell::new(rng.below(w as u32) as i32, rng.below(h as u32) as i32, 0);
        let (gx, gy) = (rng.below(w as u32) as i32, rng.below(h as u32) as i32);
        if !map.open(start.x, start.y) {
            continue;
        }
        let mut g = GridGraph::new(map, rules, None);
        let reference = reference_costs(&mut g, start);
        let goal = CellGoal::new(gx, gy, None, rules);
        let r = find_path(&mut g, start, &goal, Limits::default(), usize::MAX);
        match reference.get(&Cell::new(gx, gy, 0)) {
            Some(&cost) => {
                assert!(r.reached(), "round {round}: goal not reached");
                assert_eq!(r.cost, cost, "round {round}: not the cheapest");
                assert_eq!(r.path.last(), Some(&Cell::new(gx, gy, 0)));
                assert_valid_path(&mut g, &r.path, start, r.cost);
            }
            None => assert_eq!(r.status, SearchStatus::Exhausted, "round {round}"),
        }

        // Dijkstra (estimate 0) through the same core gives the same cost
        struct Zero(CellGoal);
        impl Goal<Cell> for Zero {
            fn is_goal(&self, n: Cell) -> bool {
                self.0.is_goal(n)
            }
            fn estimate(&self, _: Cell) -> Cost {
                0
            }
        }
        let d = find_path(&mut g, start, &Zero(goal), Limits::default(), usize::MAX);
        assert_eq!(d.status, r.status);
        if d.reached() {
            assert_eq!(d.cost, r.cost);
        }
    }
}

#[test]
fn budget_split_does_not_change_the_result() {
    let mut rng = Rng(99);
    for _ in 0..50 {
        let map = random_grid(&mut rng, 30, 30, 25, true);
        let start = Cell::new(0, 0, 0);
        if !map.open(0, 0) {
            continue;
        }
        let mut g = GridGraph::new(map, rules8(), None);
        let goal = CellGoal::new(29, 29, None, rules8());

        let mut whole = Search::new();
        whole.begin(start, &goal, Limits::default());
        whole.run(&mut g, &goal);

        let mut pieces = Search::new();
        pieces.begin(start, &goal, Limits::default());
        let slice = 1 + rng.below(20) as usize;
        while pieces.advance(&mut g, &goal, slice, &mut NoopSink) == SearchStatus::Running {}

        assert_eq!(whole.status(), pieces.status());
        assert_eq!(whole.expanded(), pieces.expanded());
        let (mut a, mut b) = (Vec::new(), Vec::new());
        whole.best_path(&mut a);
        pieces.best_path(&mut b);
        assert_eq!(a, b);
    }
}

#[test]
fn search_is_reproducible_and_reusable() {
    let mut rng = Rng(5);
    let map = random_grid(&mut rng, 40, 40, 20, true);
    let mut map = map;
    map.set(0, 0, 0);
    let mut g = GridGraph::new(map, rules8(), None);
    let goal = CellGoal::new(39, 39, None, rules8());
    let mut s = Search::with_capacity(2000);
    let mut first = Vec::new();
    s.begin(Cell::new(0, 0, 0), &goal, Limits::default());
    s.run(&mut g, &goal);
    s.best_path(&mut first);
    for _ in 0..3 {
        let mut again = Vec::new();
        s.begin(Cell::new(0, 0, 0), &goal, Limits::default());
        s.run(&mut g, &goal);
        s.best_path(&mut again);
        assert_eq!(first, again);
    }
}

#[test]
fn diagnostics_report_in_order_and_change_nothing() {
    let mut rng = Rng(11);
    let mut map = random_grid(&mut rng, 20, 20, 20, false);
    map.set(0, 0, 0);
    let mut g = GridGraph::new(map, rules8(), None);
    let goal = CellGoal::new(19, 19, None, rules8());

    let mut plain = Search::new();
    plain.begin(Cell::new(0, 0, 0), &goal, Limits::default());
    plain.run(&mut g, &goal);

    let mut collector = Collector::new(100_000, Overflow::DropNewest);
    let mut watched = Search::new();
    watched.begin(Cell::new(0, 0, 0), &goal, Limits::default());
    while watched.advance(&mut g, &goal, 7, &mut collector) == SearchStatus::Running {}

    assert_eq!(plain.status(), watched.status());
    assert_eq!(plain.expanded(), watched.expanded());
    let (mut a, mut b) = (Vec::new(), Vec::new());
    plain.best_path(&mut a);
    watched.best_path(&mut b);
    assert_eq!(a, b);

    let events: Vec<_> = collector.records().map(|r| r.event).collect();
    assert!(matches!(
        events.first(),
        Some(SearchEvent::Started { search: 1, .. })
    ));
    assert!(matches!(
        events.last(),
        Some(SearchEvent::Finished { search: 1, .. })
    ));
    let expanded = events
        .iter()
        .filter(|e| matches!(e, SearchEvent::Expanded { .. }))
        .count();
    assert_eq!(expanded, watched.expanded());
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, SearchEvent::Finished { .. }))
            .count(),
        1
    );
    assert!(
        collector
            .records()
            .zip(1..)
            .all(|(r, i)| r.sequence + 1 == i)
    );
}

#[test]
fn bounded_collector_drops_but_search_goes_on() {
    let mut g = GridGraph::new(DenseGrid::new(30, 30), rules8(), None);
    let goal = CellGoal::new(29, 29, None, rules8());
    let mut collector = Collector::new(10, Overflow::DropOldest);
    let mut s = Search::new();
    s.begin(Cell::new(0, 0, 0), &goal, Limits::default());
    let status = s.advance(&mut g, &goal, usize::MAX, &mut collector);
    assert_eq!(status, SearchStatus::Found);
    assert_eq!(collector.records().len(), 10);
    assert!(collector.dropped() > 0);
    assert!(matches!(
        collector.records().last().unwrap().event,
        SearchEvent::Finished { .. }
    ));
}

#[test]
fn noop_sink_is_disabled() {
    assert!(!<NoopSink as Sink<SearchEvent<Cell>>>::enabled(&NoopSink));
}
