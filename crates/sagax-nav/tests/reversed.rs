//! `Reversed`: a flow field over the turned-round moves holds the real cost
//! of walking to the goal, on maps whose moves cost differently each way.

use sagax_nav::flow::FlowField;
use sagax_nav::grid::{Cell, CellGoal, DenseGrid, GridRules, Preset};
use sagax_nav::topology::Topology;
use sagax_nav::{Graph, Limits, SearchStatus, find_path};

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as u32) % n
    }
}

fn octile() -> Topology {
    Topology::Square(GridRules::preset(Preset::Octile))
}

fn map(seed: u64) -> DenseGrid {
    let mut rng = Rng(seed);
    let mut map = DenseGrid::new(11, 8);
    for y in 0..8 {
        for x in 0..11 {
            match rng.below(9) {
                0 => map.set(x, y, DenseGrid::BLOCKED),
                1 | 2 => map.set(x, y, rng.below(50)),
                _ => {}
            }
            if rng.below(5) == 0 {
                map.set_zones(x, y, 1);
            }
            if rng.below(7) == 0 {
                map.set_wall_bits(x, y, 1 << rng.below(8));
            }
        }
    }
    map.set_leave_cost(40);
    map
}

#[test]
fn a_reversed_field_holds_the_forward_cost() {
    let rules = GridRules::preset(Preset::Octile);
    for seed in 1..=5 {
        let mut map = map(seed);
        let goal = Cell::new(5, 4, 0);
        map.set_at(goal, 0);
        let mut field = FlowField::default();
        {
            let mut back = octile().reversed(&mut map, 0..=0);
            field.build(&mut back, [goal], Limits::default());
        }
        let mut moves = Vec::new();
        for y in 0..8 {
            for x in 0..11 {
                let from = Cell::new(x, y, 0);
                let mut graph = octile().graph(&mut map, None);
                let r = find_path(
                    &mut graph,
                    from,
                    &CellGoal::new(goal.x, goal.y, Some(0), rules),
                    Limits::default(),
                    usize::MAX,
                );
                let want = (r.status == SearchStatus::Found).then_some(r.cost);
                // Blocked cells are no part of the field; their walker
                // takes the first move forward.
                let got = field.cost_from(&mut graph, from, &mut moves);
                assert_eq!(got, want, "seed {seed} {from:?}");
            }
        }
    }
}

#[test]
fn cells_off_the_map_and_blocked_cells_are_no_predecessors() {
    let mut map = DenseGrid::new(3, 3);
    map.set(1, 0, DenseGrid::BLOCKED);
    let mut back = octile().reversed(&mut map, 0..=0);
    let mut out = Vec::new();
    back.neighbors(Cell::new(0, 0, 0), &mut out);
    let mut got: Vec<Cell> = out.iter().map(|&(c, _)| c).collect();
    got.sort_by_key(|c| (c.x, c.y));
    // (1, 1) would cut the corner of the blocked (1, 0); (1, 0) is blocked
    // and the cells at x or y -1 are off the map.
    assert_eq!(got, [Cell::new(0, 1, 0)]);
    assert!(back.enterable(Cell::new(2, 2, 0)));
    assert!(!back.enterable(Cell::new(1, 0, 0)));
    assert!(!back.enterable(Cell::new(-1, 0, 0)));
}
