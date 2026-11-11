//! Correctness tests of D* Lite against fresh A* searches.

use sagax_nav::dstar::{DStarLite, DStarStatus, HexHeuristic};
use sagax_nav::grid::{Cell, CellGoal, DenseGrid, GridGraph, GridRules, Moves};
use sagax_nav::hex::{HexGoal, HexGraph, HexLayout};
use sagax_nav::{Cost, Graph, Limits, find_path};

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

fn random_open_cell(rng: &mut Rng, g: &DenseGrid) -> Cell {
    loop {
        let x = rng.below(g.width() as u32) as i32;
        let y = rng.below(g.height() as u32) as i32;
        if g.open(x, y) {
            return Cell::new(x, y, 0);
        }
    }
}

fn rules(four: bool) -> GridRules {
    GridRules {
        moves: if four { Moves::Four } else { Moves::Eight },
        ..GridRules::default()
    }
}

/// Fresh A* cost from `start` to `goal`, `None` if unreachable.
fn astar<G: Graph<Node = Cell>>(
    graph: &mut G,
    start: Cell,
    goal: Cell,
    r: GridRules,
) -> Option<u64> {
    let goal = CellGoal::new(goal.x, goal.y, None, r);
    let result = find_path(graph, start, &goal, Limits::default(), usize::MAX);
    result.reached().then_some(u64::from(result.cost))
}

/// Checks that `path` goes from `start` to `goal` by moves the graph offers
/// and that the step costs add up to `cost`.
fn assert_valid_path<G: Graph<Node = Cell>>(
    graph: &mut G,
    path: &[Cell],
    start: Cell,
    goal: Cell,
    cost: u64,
) {
    assert_eq!(path.first(), Some(&start));
    assert_eq!(path.last(), Some(&goal));
    let mut sum = 0u64;
    let mut buf = Vec::new();
    for pair in path.windows(2) {
        buf.clear();
        graph.neighbors(pair[0], &mut buf);
        let step: Cost = buf
            .iter()
            .filter(|(n, _)| *n == pair[1])
            .map(|(_, c)| *c)
            .min()
            .unwrap_or_else(|| panic!("illegal step {:?} -> {:?}", pair[0], pair[1]));
        sum += u64::from(step);
    }
    assert_eq!(sum, cost, "step costs do not add up");
}

/// The cell and its 8 neighbours: every node whose moves change when the
/// cell is blocked or unblocked.
fn around(x: i32, y: i32, out: &mut Vec<Cell>) {
    out.clear();
    for dy in -1..=1 {
        for dx in -1..=1 {
            out.push(Cell::new(x + dx, y + dy, 0));
        }
    }
}

#[test]
fn static_maps_match_astar() {
    let mut rng = Rng(7);
    let mut d = DStarLite::new();
    let mut path = Vec::new();
    let mut found = 0;
    for case in 0..200 {
        let four = case % 2 == 0;
        let weighted = case % 4 >= 2;
        let r = rules(four);
        let w = 5 + rng.below(30) as i32;
        let h = 5 + rng.below(30) as i32;
        let grid = random_grid(&mut rng, w, h, 25, weighted);
        let start = random_open_cell(&mut rng, &grid);
        let goal = random_open_cell(&mut rng, &grid);
        let mut g = GridGraph::new(grid, r, None);

        let expected = astar(&mut g, start, goal, r);
        d.begin(start, goal, r);
        let status = d.compute(&mut g, usize::MAX);
        assert_eq!(d.cost(), expected, "case {case}");
        match expected {
            Some(cost) => {
                found += 1;
                assert_eq!(status, DStarStatus::Found);
                assert!(d.path(&mut g, &mut path));
                assert_valid_path(&mut g, &path, start, goal, cost);
            }
            None => {
                assert_eq!(status, DStarStatus::Unreachable);
                assert!(!d.path(&mut g, &mut path));
                assert!(path.is_empty());
            }
        }
    }
    assert!(found > 100, "too few reachable cases: {found}");
}

#[test]
fn small_budgets_end_like_one_big_compute() {
    let mut rng = Rng(11);
    for _ in 0..20 {
        let r = rules(false);
        let grid = random_grid(&mut rng, 25, 25, 20, true);
        let start = random_open_cell(&mut rng, &grid);
        let goal = random_open_cell(&mut rng, &grid);
        let mut g = GridGraph::new(grid, r, None);
        let mut d = DStarLite::new();
        d.begin(start, goal, r);
        let mut rounds = 0;
        while d.compute(&mut g, 3) == DStarStatus::Running {
            rounds += 1;
            assert!(rounds < 100_000);
        }
        assert_eq!(d.cost(), astar(&mut g, start, goal, r));
    }
}

#[test]
fn walking_through_a_changing_map_matches_fresh_astar() {
    let mut rng = Rng(42);
    let mut d = DStarLite::new();
    let mut path = Vec::new();
    let mut changed = Vec::new();
    let mut total_changes = 0;
    let mut lost_and_found = 0;
    for scenario in 0..120 {
        let four = scenario % 3 == 0;
        let weighted = scenario % 2 == 1;
        let r = rules(four);
        let grid = random_grid(&mut rng, 30, 30, 20, weighted);
        let mut start = random_open_cell(&mut rng, &grid);
        let goal = random_open_cell(&mut rng, &grid);
        let mut g = GridGraph::new(grid, r, None);
        d.begin(start, goal, r);
        d.compute(&mut g, usize::MAX);
        assert_eq!(d.cost(), astar(&mut g, start, goal, r));

        let mut was_unreachable = d.cost().is_none();
        for round in 0..40 {
            // walk a few steps along the current path
            if d.path(&mut g, &mut path) {
                let steps = 1 + rng.below(3) as usize;
                let next = path[steps.min(path.len() - 1)];
                if next != start {
                    start = next;
                    d.move_start(start);
                }
            }
            if start == goal {
                break;
            }

            // change the map
            let changes = 1 + rng.below(4);
            for _ in 0..changes {
                let (x, y) = match rng.below(10) {
                    // a cell on the current path
                    0..=3 if path.len() > 2 => {
                        let c = path[1 + rng.below(path.len() as u32 - 1) as usize];
                        (c.x, c.y)
                    }
                    // the goal itself
                    4 if round % 7 == 3 => (goal.x, goal.y),
                    _ => (rng.below(30) as i32, rng.below(30) as i32),
                };
                if (x, y) == (start.x, start.y) {
                    continue; // the walker stands there
                }
                let new = if g.map.open(x, y) {
                    if rng.below(3) == 0 && weighted {
                        rng.below(4) * 7 // cheaper or dearer, still open
                    } else {
                        DenseGrid::BLOCKED
                    }
                } else if weighted {
                    rng.below(4) * 7
                } else {
                    0
                };
                g.map.set(x, y, new);
                around(x, y, &mut changed);
                d.nodes_changed(&mut g, changed.iter().copied());
                total_changes += 1;
            }
            // reopen the goal now and then so it becomes reachable again
            if round % 7 == 5 && !g.map.open(goal.x, goal.y) {
                g.map.set(goal.x, goal.y, 0);
                around(goal.x, goal.y, &mut changed);
                d.nodes_changed(&mut g, changed.iter().copied());
            }

            let status = d.compute(&mut g, usize::MAX);
            let expected = astar(&mut g, start, goal, r);
            assert_eq!(d.cost(), expected, "scenario {scenario}, round {round}");
            match expected {
                Some(cost) => {
                    assert_eq!(status, DStarStatus::Found);
                    assert!(d.path(&mut g, &mut path));
                    assert_valid_path(&mut g, &path, start, goal, cost);
                    if was_unreachable {
                        lost_and_found += 1;
                    }
                    was_unreachable = false;
                }
                None => {
                    assert_eq!(status, DStarStatus::Unreachable);
                    assert!(!d.path(&mut g, &mut path));
                    was_unreachable = true;
                }
            }
        }
    }
    println!("changes: {total_changes}, unreachable -> reachable again: {lost_and_found}");
    assert!(total_changes > 2000);
    assert!(lost_and_found > 5);
}

#[test]
fn replanning_after_a_small_change_expands_less_than_a_fresh_compute() {
    let mut rng = Rng(99);
    let r = rules(false);
    let mut d = DStarLite::new();
    let mut fresh = DStarLite::new();
    let mut path = Vec::new();
    let mut changed = Vec::new();
    let (mut sum_replan, mut sum_fresh, mut runs) = (0usize, 0usize, 0usize);
    for _ in 0..20 {
        let grid = random_grid(&mut rng, 100, 100, 15, false);
        let mut start = random_open_cell(&mut rng, &grid);
        let goal = random_open_cell(&mut rng, &grid);
        let mut g = GridGraph::new(grid, r, None);
        d.begin(start, goal, r);
        if d.compute(&mut g, usize::MAX) != DStarStatus::Found {
            continue;
        }
        for _ in 0..5 {
            if !d.path(&mut g, &mut path) || path.len() < 8 {
                break;
            }
            // walk two steps, then a cell a few steps ahead gets blocked
            start = path[2];
            d.move_start(start);
            let block = path[3 + rng.below(path.len() as u32 - 4) as usize];
            if block == goal {
                continue;
            }
            g.map.set(block.x, block.y, DenseGrid::BLOCKED);
            around(block.x, block.y, &mut changed);
            let before = d.expanded();
            d.nodes_changed(&mut g, changed.iter().copied());
            d.compute(&mut g, usize::MAX);
            let replan = d.expanded() - before;

            fresh.begin(start, goal, r);
            fresh.compute(&mut g, usize::MAX);
            assert_eq!(d.cost(), fresh.cost());
            sum_replan += replan;
            sum_fresh += fresh.expanded();
            runs += 1;
        }
    }
    assert!(runs > 30, "too few runs: {runs}");
    println!(
        "100x100, {runs} replans: average expansions replan {} vs fresh {}",
        sum_replan / runs,
        sum_fresh / runs
    );
    assert!(sum_replan < sum_fresh);
}

#[test]
fn start_is_goal() {
    let r = rules(false);
    let mut g = GridGraph::new(DenseGrid::new(5, 5), r, None);
    let mut d = DStarLite::new();
    let c = Cell::new(2, 2, 0);
    d.begin(c, c, r);
    assert_eq!(d.compute(&mut g, usize::MAX), DStarStatus::Found);
    assert_eq!(d.cost(), Some(0));
    let mut path = Vec::new();
    assert!(d.path(&mut g, &mut path));
    assert_eq!(path, [c]);
}

#[test]
fn unreachable_from_the_beginning() {
    let r = rules(false);
    let mut grid = DenseGrid::new(9, 9);
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
        grid.set(x, y, DenseGrid::BLOCKED);
    }
    let mut g = GridGraph::new(grid, r, None);
    let mut d = DStarLite::new();
    let (start, goal) = (Cell::new(0, 4, 0), Cell::new(6, 4, 0));
    d.begin(start, goal, r);
    assert_eq!(d.compute(&mut g, usize::MAX), DStarStatus::Unreachable);
    assert_eq!(d.cost(), None);
    let mut path = Vec::new();
    assert!(!d.path(&mut g, &mut path));

    // the walker is inside, the goal outside
    d.begin(goal, start, r);
    assert_eq!(d.compute(&mut g, usize::MAX), DStarStatus::Unreachable);

    // a gap opens
    g.map.set(5, 4, 0);
    let mut changed = Vec::new();
    around(5, 4, &mut changed);
    d.nodes_changed(&mut g, changed.iter().copied());
    assert_eq!(d.compute(&mut g, usize::MAX), DStarStatus::Found);
    assert_eq!(d.cost(), astar(&mut g, goal, start, r));
}

#[test]
fn hex_maps_match_astar() {
    let mut rng = Rng(5);
    let layouts = [
        HexLayout::OddR,
        HexLayout::EvenR,
        HexLayout::OddQ,
        HexLayout::EvenQ,
    ];
    let mut d = DStarLite::new();
    let mut path = Vec::new();
    for case in 0..40 {
        let layout = layouts[case % 4];
        let grid = random_grid(&mut rng, 20, 20, 25, case % 2 == 1);
        let start = random_open_cell(&mut rng, &grid);
        let goal = random_open_cell(&mut rng, &grid);
        let mut g = HexGraph::new(grid, layout, 10, None);
        let hex_goal = HexGoal {
            x: goal.x,
            y: goal.y,
            layout,
            step_cost: 10,
        };
        let expected = find_path(&mut g, start, &hex_goal, Limits::default(), usize::MAX);
        d.begin(
            start,
            goal,
            HexHeuristic {
                layout,
                step_cost: 10,
            },
        );
        d.compute(&mut g, usize::MAX);
        assert_eq!(
            d.cost(),
            expected.reached().then_some(u64::from(expected.cost)),
            "case {case}"
        );
        if let Some(cost) = d.cost() {
            assert!(d.path(&mut g, &mut path));
            assert_valid_path(&mut g, &path, start, goal, cost);
        }
    }
}
