//! Goal recognition from flow fields: square and hex, jumps, standing
//! still, a walled-off goal, splitting hunters (no heap per step is
//! checked in the C API tests, which may count allocations).

use sagax::debug::{Collector, GoalGuessTrace, Overflow};
use sagax::goal_guess::observe_flow_traced;
use sagax::nav::Limits;
use sagax::nav::flow::FlowField;
use sagax::nav::search::Graph;
use sagax::prelude::*;

/// 21 x 15 with a wall in the middle (gaps at y 4 and 10).
fn map() -> DenseGrid {
    let mut map = DenseGrid::new(21, 15);
    for y in 2..13 {
        if y != 4 && y != 10 {
            map.set(10, y, DenseGrid::BLOCKED);
        }
    }
    map
}

const EXITS: [(i32, i32); 4] = [(0, 0), (20, 0), (0, 14), (20, 14)];

fn fields<G: Graph<Node = Cell>>(graph: &mut G, exits: &[(i32, i32)]) -> Vec<FlowField<Cell>> {
    exits
        .iter()
        .map(|&(x, y)| {
            let mut f = FlowField::default();
            f.build(graph, [Cell::new(x, y, 0)], Limits::default());
            f
        })
        .collect()
}

/// A walker from `start` to exit `k` along its field: the true goal is
/// the best guess in the second half of the way.
fn walk_and_check(topology: Topology, start: Cell) {
    let mut map = map();
    let mut graph = topology.graph(&mut map, None);
    let mut fields = fields(&mut graph, &EXITS);
    for k in 0..EXITS.len() {
        let mut way = Vec::new();
        assert!(fields[k].path(&mut graph, start, &mut way));
        let mut guess = GoalGuess::new(GoalGuessConfig::default(), EXITS.len());
        let mut moves = Vec::new();
        let steps = way.len() - 1;
        for (i, w) in way.windows(2).enumerate() {
            observe_flow(&mut guess, &mut graph, &fields, w[0], w[1], &mut moves).unwrap();
            let sum: u32 = (0..EXITS.len()).map(|g| guess.chance(g)).sum();
            assert_eq!(sum, 256);
            if 2 * (i + 1) >= steps {
                assert_eq!(
                    guess.best().map(|b| b.0),
                    Some(k),
                    "{topology:?} exit {k} step {} of {steps}",
                    i + 1
                );
            }
        }
    }
}

#[test]
fn a_direct_walker_is_recognised_on_square_and_hex() {
    let square = Topology::Square(GridRules::preset(Preset::Octile));
    for start in [Cell::new(9, 7, 0), Cell::new(13, 6, 0), Cell::new(4, 9, 0)] {
        walk_and_check(square, start);
        walk_and_check(Topology::hex(HexLayout::OddR, Preset::Octile), start);
    }
}

#[test]
fn jumps_and_wrong_fields_are_refused_standing_still_teaches_nothing() {
    let mut map = map();
    let mut graph = Topology::Square(GridRules::preset(Preset::Octile)).graph(&mut map, None);
    let fields = fields(&mut graph, &EXITS);
    let mut guess = GoalGuess::new(GoalGuessConfig::default(), EXITS.len());
    let mut moves = Vec::new();
    let mut log = Collector::new(16, Overflow::DropOldest);
    let a = Cell::new(5, 5, 0);
    let before = guess;
    assert_eq!(
        observe_flow_traced(
            &mut guess,
            &mut graph,
            &fields,
            a,
            Cell::new(7, 5, 0),
            &mut moves,
            &mut log
        ),
        Err(StepError::Jump)
    );
    assert_eq!(
        observe_flow_traced(
            &mut guess,
            &mut graph,
            &fields[..3],
            a,
            a,
            &mut moves,
            &mut log
        ),
        Err(StepError::Shape)
    );
    assert_eq!(
        observe_flow_traced(&mut guess, &mut graph, &fields, a, a, &mut moves, &mut log),
        Ok(())
    );
    assert_eq!(guess, before);
    let events: Vec<_> = log.records().map(|r| r.event).collect();
    assert_eq!(
        events,
        [
            GoalGuessTrace::Refused {
                error: StepError::Jump
            },
            GoalGuessTrace::Refused {
                error: StepError::Shape
            }
        ]
    );
    // A real step after the refusals: one record per goal.
    observe_flow_traced(
        &mut guess,
        &mut graph,
        &fields,
        a,
        Cell::new(4, 4, 0),
        &mut moves,
        &mut log,
    )
    .unwrap();
    assert_eq!(log.records().len(), 2 + EXITS.len());
    assert_eq!(guess.best().map(|b| b.0), Some(0));
}

#[test]
fn a_walled_off_goal_zeros_nobody() {
    let mut map = map();
    // Exit 4 sits in a closed box.
    for (x, y) in [
        (15, 6),
        (16, 6),
        (17, 6),
        (15, 7),
        (17, 7),
        (15, 8),
        (16, 8),
        (17, 8),
    ] {
        map.set(x, y, DenseGrid::BLOCKED);
    }
    let mut graph = Topology::Square(GridRules::preset(Preset::Octile)).graph(&mut map, None);
    let mut exits = EXITS.to_vec();
    exits.push((16, 7));
    let mut fields = fields(&mut graph, &exits);
    let mut guess = GoalGuess::new(GoalGuessConfig::default(), exits.len());
    let mut moves = Vec::new();
    let mut way = Vec::new();
    assert!(fields[3].path(&mut graph, Cell::new(4, 9, 0), &mut way));
    for w in way.windows(2) {
        observe_flow(&mut guess, &mut graph, &fields, w[0], w[1], &mut moves).unwrap();
        let sum: u32 = (0..exits.len()).map(|g| guess.chance(g)).sum();
        assert_eq!(sum, 256);
        for g in 0..4 {
            assert!(guess.chance_fine(g) > 0, "goal {g} never zero");
        }
    }
    assert_eq!(guess.best().map(|b| b.0), Some(3));
}

#[test]
fn hunters_follow_the_guess() {
    let mut map = map();
    let mut graph = Topology::Square(GridRules::preset(Preset::Octile)).graph(&mut map, None);
    let mut fields = fields(&mut graph, &EXITS);
    let mut way = Vec::new();
    assert!(fields[1].path(&mut graph, Cell::new(4, 9, 0), &mut way));
    let mut guess = GoalGuess::new(GoalGuessConfig::default(), EXITS.len());
    let mut moves = Vec::with_capacity(16);
    let mut hunters = [0u32; 4];
    for w in way.windows(2) {
        observe_flow(&mut guess, &mut graph, &fields, w[0], w[1], &mut moves).unwrap();
        guess.split(3, &mut hunters);
    }
    assert_eq!(hunters, [0, 3, 0, 0]);
}
