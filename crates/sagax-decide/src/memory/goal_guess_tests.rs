//! Tests of [`GoalGuess`]: the research direction on a small map, the
//! range of the chances, ruled-out and unreachable goals, forgetting.

use super::*;
use sagax_debug::{Collector, Overflow};

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A small octile map (costs 10 / 14) with walls, built in the test.
struct Map {
    w: i32,
    h: i32,
    blocked: Vec<bool>,
}

const MOVES: [(i32, i32, i32); 8] = [
    (1, 0, 10),
    (-1, 0, 10),
    (0, 1, 10),
    (0, -1, 10),
    (1, 1, 14),
    (1, -1, 14),
    (-1, 1, 14),
    (-1, -1, 14),
];

impl Map {
    /// 21 x 15, a wall in the middle with gaps, a block near the top.
    fn new() -> Self {
        let (w, h) = (21, 15);
        let mut blocked = vec![false; (w * h) as usize];
        for y in 2..13 {
            if y != 4 && y != 10 {
                blocked[(y * w + 10) as usize] = true;
            }
        }
        for x in 4..8 {
            blocked[(3 * w + x) as usize] = true;
        }
        Self { w, h, blocked }
    }

    fn free(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h && !self.blocked[(y * self.w + x) as usize]
    }

    /// The moves from `(x, y)`: `(x, y, cost)`.
    fn moves(&self, x: i32, y: i32) -> Vec<(i32, i32, i32)> {
        MOVES
            .iter()
            .map(|&(dx, dy, c)| (x + dx, y + dy, c))
            .filter(|&(nx, ny, _)| self.free(nx, ny))
            .collect()
    }

    /// Walking distance to `goal` from every cell (`None`: unreachable).
    fn field(&self, goal: (i32, i32)) -> Vec<Option<i32>> {
        let mut d = vec![None; (self.w * self.h) as usize];
        let mut open = BinaryHeap::new();
        d[(goal.1 * self.w + goal.0) as usize] = Some(0);
        open.push(Reverse((0, goal.0, goal.1)));
        while let Some(Reverse((c, x, y))) = open.pop() {
            if d[(y * self.w + x) as usize] != Some(c) {
                continue;
            }
            for (nx, ny, step) in self.moves(x, y) {
                let i = (ny * self.w + nx) as usize;
                if d[i].is_none_or(|old| c + step < old) {
                    d[i] = Some(c + step);
                    open.push(Reverse((c + step, nx, ny)));
                }
            }
        }
        d
    }
}

/// Four exits of [`Map`].
const EXITS: [(i32, i32); 4] = [(0, 0), (20, 0), (0, 14), (20, 14)];

struct Scene {
    map: Map,
    fields: Vec<Vec<Option<i32>>>,
}

impl Scene {
    fn new() -> Self {
        let map = Map::new();
        let fields = EXITS.iter().map(|&e| map.field(e)).collect();
        Self { map, fields }
    }

    fn d(&self, k: usize, x: i32, y: i32) -> i32 {
        self.fields[k][(y * self.map.w + x) as usize].expect("reachable")
    }

    /// The steepest step towards exit `k` (ties: the first move).
    fn next(&self, k: usize, x: i32, y: i32) -> (i32, i32) {
        let mut best = (x, y);
        let mut best_d = self.d(k, x, y);
        for (nx, ny, step) in self.map.moves(x, y) {
            let nd = self.d(k, nx, ny);
            if nd < best_d && step + nd == self.d(k, x, y) {
                best = (nx, ny);
                best_d = nd;
            }
        }
        best
    }

    /// Feeds the step `prev -> cur` (a move) to `guess`.
    fn observe(&self, guess: &mut GoalGuess, prev: (i32, i32), cur: (i32, i32)) {
        let moves = self.map.moves(prev.0, prev.1);
        let n = guess.goals();
        let mut moved = vec![0; n];
        let mut options = vec![0; n * moves.len()];
        for k in 0..n {
            let here = self.d(k, prev.0, prev.1);
            moved[k] = self.d(k, cur.0, cur.1) - here;
            for (j, &(nx, ny, _)) in moves.iter().enumerate() {
                options[k * moves.len() + j] = self.d(k, nx, ny) - here;
            }
        }
        guess
            .observe(&moved, &options, moves.len())
            .expect("a fitting step");
    }

    /// The way from `start` to exit `k`.
    fn way(&self, k: usize, start: (i32, i32)) -> Vec<(i32, i32)> {
        let mut way = vec![start];
        while way.last() != Some(&EXITS[k]) {
            let &(x, y) = way.last().unwrap();
            way.push(self.next(k, x, y));
        }
        way
    }
}

fn assert_sums(g: &GoalGuess) {
    let n = g.goals();
    let fine: u32 = (0..n).map(|k| g.chance_fine(k)).sum();
    let read: u32 = (0..n).map(|k| g.chance(k)).sum();
    if n > 0 {
        assert_eq!(fine, 65536, "fine chances sum to 65536");
        assert_eq!(read, 256, "chances sum to 256");
    }
    for k in 0..n {
        assert!(g.chance(k) <= 256);
    }
}

#[test]
fn direct_walker_is_recognised_in_the_second_half() {
    let scene = Scene::new();
    for start in [(9, 7), (11, 6), (5, 9), (14, 8)] {
        for k in 0..EXITS.len() {
            let way = scene.way(k, start);
            let mut guess = GoalGuess::new(GoalGuessConfig::default(), EXITS.len());
            for (i, w) in way.windows(2).enumerate() {
                scene.observe(&mut guess, w[0], w[1]);
                assert_sums(&guess);
                if 2 * (i + 1) >= way.len() - 1 {
                    assert_eq!(
                        guess.best().map(|b| b.0),
                        Some(k),
                        "start {start:?}, exit {k}, step {} of {}",
                        i + 1,
                        way.len() - 1
                    );
                }
            }
            assert!(
                guess.chance(k) > 200,
                "sure at the end: {}",
                guess.chance(k)
            );
        }
    }
}

#[test]
fn chances_stay_in_range_for_every_goal_count() {
    for n in 0..=MAX_GOALS + 2 {
        let mut g = GoalGuess::new(GoalGuessConfig::default(), n);
        assert_eq!(g.goals(), n.min(MAX_GOALS));
        assert_sums(&g);
        let n = g.goals();
        // Odd numbers: every goal gets a different story.
        for step in 0..40i32 {
            let moved: Vec<i32> = (0..n as i32).map(|k| ((k + step) % 3 - 1) * 10).collect();
            let options: Vec<i32> = (0..n as i32)
                .flat_map(|k| [-10, 0, 10, -14 + k, 14 - k])
                .collect();
            g.observe(&moved, &options, 5).unwrap();
            assert_sums(&g);
        }
    }
}

#[test]
fn uniform_start_ties_go_to_the_lower_goal() {
    let g = GoalGuess::new(GoalGuessConfig::default(), 4);
    assert_eq!(g.best(), Some((0, 64)));
    assert_eq!((0..4).map(|k| g.rank(k)).collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert_eq!(g.rank(9), 4);
    assert_eq!(g.chance(9), 0);
    let three = GoalGuess::new(GoalGuessConfig::default(), 3);
    assert_eq!(three.shares()[..3], [86, 85, 85]);
    assert_eq!(GoalGuess::new(GoalGuessConfig::default(), 0).best(), None);
}

#[test]
fn a_ruled_out_goal_stays_out() {
    let scene = Scene::new();
    let mut g = GoalGuess::new(GoalGuessConfig::default(), 4);
    g.set_prior(&[0, 1, 1, 1]);
    assert_eq!(g.shares()[..4], [0, 86, 85, 85]);
    // He walks to exit 0, which the mission ruled out.
    let way = scene.way(0, (9, 7));
    for w in way.windows(2) {
        scene.observe(&mut g, w[0], w[1]);
        assert_eq!(g.chance_fine(0), 0);
        assert_sums(&g);
    }
    assert_eq!(g.rank(0), 3);
    g.reset();
    assert_eq!(g.chance(0), 0);
    g.set_prior(&[1, 1, 1, 1]);
    assert_eq!(g.chance(0), 64, "a new prior lets it back in");
    g.set_prior(&[0, 0, 0, 0]);
    assert_eq!(g.chance(0), 64, "all zero is uniform");
    g.set_prior(&[5]);
    assert_eq!(g.shares()[..4], [256, 0, 0, 0], "missing weights are 0");
}

#[test]
fn a_tiny_weight_is_not_ruled_out() {
    let scene = Scene::new();
    let mut g = GoalGuess::new(GoalGuessConfig::default(), 4);
    g.set_prior(&[1_000_000, 1, 0, u32::MAX]);
    assert_eq!(g.chance_fine(1), 1, "a tiny weight keeps 1/65536");
    assert_eq!(g.chance_fine(2), 0, "only weight 0 rules out");
    let sum: u32 = (0..4).map(|k| g.chance_fine(k)).sum();
    assert_eq!(sum, 65536);
    // He walks to exit 1: the tiny prior can still win.
    let way = scene.way(1, (9, 7));
    for w in way.windows(2) {
        scene.observe(&mut g, w[0], w[1]);
        assert_sums(&g);
        assert_eq!(g.chance_fine(2), 0);
    }
    assert_eq!(g.best().map(|b| b.0), Some(1));
}

#[test]
fn prior_weights_and_split() {
    let mut g = GoalGuess::new(GoalGuessConfig::default(), 3);
    g.set_prior(&[60, 30, 10]);
    assert_eq!(g.shares()[..3], [154, 77, 25]);
    let mut out = [9u32; 5];
    g.split(3, &mut out);
    assert_eq!(out, [2, 1, 0, 0, 0]);
    g.split(0, &mut out);
    assert_eq!(out, [0; 5]);
    g.split(10, &mut out);
    assert_eq!(out, [6, 3, 1, 0, 0]);
    let mut short = [0u32; 2];
    g.split(10, &mut short);
    assert_eq!(short, [6, 3], "a short out gets the goals that fit");
    let mut row = [7i32; 4];
    g.write_columns(1, &mut row[1..]);
    assert_eq!(row, [7, 77, 1, 7]);
    g.write_columns(5, &mut row);
    assert_eq!(row[..2], [0, 3]);
}

#[test]
fn an_unreachable_or_extreme_goal_zeros_nobody() {
    let mut g = GoalGuess::new(GoalGuessConfig::default(), 3);
    // Goal 2: the step taken is the worst by far; goal 0 extreme numbers.
    for _ in 0..50 {
        g.observe(
            &[i32::MIN, -10, i32::MAX],
            &[i32::MIN, 0, i32::MAX, -10, 0, 10, 0, 0, i32::MAX],
            3,
        )
        .unwrap();
        assert_sums(&g);
    }
    assert!(g.chance_fine(1) > 0 && g.chance_fine(0) > 0);
    // Forgetting keeps a floor under the unlikely one.
    assert!(g.chance_fine(2) > 0);

    let mut never = GoalGuess::new(
        GoalGuessConfig {
            forget: 0,
            ..GoalGuessConfig::default()
        },
        2,
    );
    for _ in 0..200 {
        never
            .observe(&[i32::MAX, i32::MAX], &[0, i32::MAX, 0, i32::MAX], 2)
            .unwrap();
        assert_sums(&never);
    }
    assert_eq!(never.shares()[..2], [128, 128], "same story: no change");
}

#[test]
fn bad_shapes_are_refused_and_change_nothing() {
    let mut g = GoalGuess::new(GoalGuessConfig::default(), 2);
    g.set_prior(&[3, 1]);
    let before = g;
    let mut log = Collector::new(16, Overflow::DropOldest);
    for (moved, options, per) in [
        (&[0, 0][..], &[0, 0][..], 0),
        (&[0][..], &[0, 0][..], 1),
        (&[0, 0][..], &[0, 0, 0][..], 2),
        (&[0, 0][..], &[0; 2 * (MAX_MOVES + 1)][..], MAX_MOVES + 1),
    ] {
        assert_eq!(
            g.observe_traced(moved, options, per, &mut log),
            Err(StepError::Shape)
        );
        assert_eq!(g, before);
    }
    assert!(log.records().all(|r| r.event
        == GoalGuessTrace::Refused {
            error: StepError::Shape
        }));
    assert_eq!(log.records().len(), 4);
}

#[test]
fn beta_zero_teaches_nothing() {
    let mut g = GoalGuess::new(
        GoalGuessConfig {
            beta: 0,
            unit: 0,
            forget: 0,
        },
        2,
    );
    g.observe(&[-10, 10], &[-10, 10, 10, -10], 2).unwrap();
    assert_eq!(g.shares()[..2], [128, 128]);
}

/// Steps from the turn until the true goal is the best guess again.
fn recovery(forget: u32, feint: usize, turn: usize, start: (i32, i32)) -> usize {
    let scene = Scene::new();
    // Feint: `feint` steps towards exit 1 (top right), then turn to exit
    // `turn`.
    let mut at = start;
    let mut g = GoalGuess::new(
        GoalGuessConfig {
            forget,
            ..GoalGuessConfig::default()
        },
        4,
    );
    for _ in 0..feint {
        let next = scene.next(1, at.0, at.1);
        scene.observe(&mut g, at, next);
        at = next;
    }
    assert_eq!(g.best().map(|b| b.0), Some(1), "the feint works");
    let way = scene.way(turn, at);
    for (i, w) in way.windows(2).enumerate() {
        scene.observe(&mut g, w[0], w[1]);
        assert_sums(&g);
        if g.best().map(|b| b.0) == Some(turn) {
            return i + 1;
        }
    }
    usize::MAX
}

#[test]
fn forgetting_recovers_faster_after_a_feint() {
    let forget = GoalGuessConfig::default().forget;
    let (mut fast_all, mut slow_all) = (0, 0);
    for feint in [4, 6, 8, 10] {
        for turn in [0, 2, 3] {
            for start in [(3, 7), (9, 7), (14, 8)] {
                let fast = recovery(forget, feint, turn, start);
                let slow = recovery(0, feint, turn, start);
                assert!(slow < usize::MAX, "Bayes alone recovers too");
                assert!(fast <= slow, "{feint} {turn} {start:?}: {fast} vs {slow}");
                fast_all += fast;
                slow_all += slow;
            }
        }
    }
    // Overall at least a third faster.
    assert!(3 * fast_all < 2 * slow_all, "{fast_all} vs {slow_all}");
    // A long feint towards the top right, then to the bottom right.
    assert_eq!(
        (recovery(forget, 8, 3, (3, 7)), recovery(0, 8, 3, (3, 7))),
        (3, 6)
    );
}

#[test]
fn trace_tells_every_update() {
    let mut g = GoalGuess::new(GoalGuessConfig::default(), 2);
    let mut log = Collector::new(64, Overflow::DropOldest);
    g.set_prior_traced(&[1, 3], &mut log);
    g.observe_traced(&[-10, 10], &[-10, 10, 10, -10], 2, &mut log)
        .unwrap();
    g.reset_traced(&mut log);
    let events: Vec<_> = log.records().map(|r| r.event).collect();
    assert_eq!(
        events[0],
        GoalGuessTrace::Prior {
            goal: 0,
            chance: 64
        }
    );
    assert_eq!(
        events[1],
        GoalGuessTrace::Prior {
            goal: 1,
            chance: 192
        }
    );
    let GoalGuessTrace::Step {
        goal: 0,
        moved: -10,
        likelihood: l0,
        chance: c0,
    } = events[2]
    else {
        panic!("{:?}", events[2]);
    };
    let GoalGuessTrace::Step {
        goal: 1,
        moved: 10,
        likelihood: l1,
        chance: c1,
    } = events[3]
    else {
        panic!("{:?}", events[3]);
    };
    // exp(0) / (exp(0) + exp(-4)) and exp(-4) / (exp(-4) + exp(0)).
    assert!((64300..64400).contains(&l0), "{l0}");
    assert!((1150..1250).contains(&l1), "{l1}");
    assert_eq!(c0 + c1, 256);
    assert!(c0 > 128, "the step points to goal 0: {c0}");
    assert_eq!(events[4], GoalGuessTrace::Reset);
    assert_eq!(events.len(), 5);
}
