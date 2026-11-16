//! A guard on a tower guesses which exit a runner heads for.
//! `cargo run -p sagax --example goal_guess_scene`
//!
//! A yard of 21 x 15 cells (square, octile costs 10 / 14) with a wall down
//! the middle (x 10, open at y 4 and y 10) and four exits: 0 top left
//! (0,0), 1 top right (20,0), 2 bottom left (0,14), 3 bottom right
//! (20,14). The game keeps one flow field per exit. A runner walks along
//! the field of his exit; the guard sees him everywhere except under a
//! roof (x 13..15, y 10..13). For every tick the guard feeds only what it
//! saw to its [`GoalGuess`] through [`observe_flow`]: a seen step from the
//! last seen place; when the runner comes out from under the roof, the
//! step from the last seen place is a jump, refused, and the chain simply
//! goes on from the new place. Three hunters are split over the exits by
//! chance ([`GoalGuess::split`]).
//!
//! Three runs:
//!
//! - `direct forget 13`: from (5,7) straight to exit 3, under the roof.
//! - `feint forget 0` and `feint forget 13`: from (5,7) five steps towards
//!   exit 1, then he turns to exit 2; once without forgetting, once with
//!   the default (about 5 %), which recognises the turn sooner.
//!
//! Lines: the run's name, then per tick
//!
//! - `t=<tick> <tag> <x>,<y> chance <c0> <c1> <c2> <c3> best <goal>
//!   hunters <h0> <h1> <h2> <h3>` for a seen place; `tag` is `start` (the
//!   first sighting), `step` (learned) or `jump` (refused, new chain);
//!   chances are 0..256 and sum to 256;
//! - `t=<tick> unseen` while he is under the roof;
//!
//! and at the end `arrived at exit <k>, recognised at t=<tick>` (the first
//! tick from which the best guess stays the true exit).
//!
//! The same scene is in `examples/c/goal_guess.c`; both print the same
//! lines.

use sagax::nav::Limits;
use sagax::nav::flow::FlowField;
use sagax::prelude::*;

/// Width of the yard.
pub const WIDTH: i32 = 21;
/// Height of the yard.
pub const HEIGHT: i32 = 15;
/// The exits, in goal order.
pub const EXITS: [(i32, i32); 4] = [(0, 0), (20, 0), (0, 14), (20, 14)];
/// Where every run starts.
pub const START: (i32, i32) = (5, 7);
/// Hunters to split.
pub const HUNTERS: u32 = 3;
/// Steps of the feint towards exit 1.
pub const FEINT: u32 = 5;

/// Whether the yard's wall is at `x`, `y`.
pub fn wall(x: i32, y: i32) -> bool {
    x == 10 && (2..=12).contains(&y) && y != 4 && y != 10
}

/// Whether `x`, `y` is under the roof (the guard cannot see it).
pub fn roofed(x: i32, y: i32) -> bool {
    (13..=15).contains(&x) && (10..=13).contains(&y)
}

/// What happened in a run, for the checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A seen step was learned (`from` and `to` both seen).
    Learned {
        /// Last seen place.
        from: (i32, i32),
        /// Place now.
        to: (i32, i32),
    },
    /// A seen place was not a move from the last seen place.
    Jump,
    /// The runner was not seen.
    Unseen,
}

/// One run.
pub struct Run {
    /// Its name line.
    pub name: &'static str,
    /// The printed lines (name first).
    pub lines: Vec<String>,
    /// Per tick: what happened (the start tick has none).
    pub events: Vec<(u32, Event)>,
    /// Per tick: the best guess (seen ticks only).
    pub best: Vec<(u32, usize)>,
    /// The exit he really took.
    pub exit: usize,
    /// The first tick from which the best guess stays the true exit.
    pub recognised: u32,
}

/// The yard.
pub fn yard() -> DenseGrid {
    let mut map = DenseGrid::new(WIDTH, HEIGHT);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if wall(x, y) {
                map.set(x, y, DenseGrid::BLOCKED);
            }
        }
    }
    map
}

/// One run: the runner follows exit `first` for `turn` steps, then exit
/// `exit`; the guard learns with `forget`.
pub fn run(name: &'static str, first: usize, turn: u32, exit: usize, forget: u32) -> Run {
    let mut map = yard();
    let mut graph = Topology::Square(GridRules::preset(Preset::Octile)).graph(&mut map, None);
    let mut fields: Vec<FlowField<Cell>> = EXITS
        .iter()
        .map(|&(x, y)| {
            let mut f = FlowField::default();
            f.build(&mut graph, [Cell::new(x, y, 0)], Limits::default());
            f
        })
        .collect();
    let config = GoalGuessConfig {
        forget,
        ..GoalGuessConfig::default()
    };
    let mut guess = GoalGuess::new(config, EXITS.len());
    let mut moves = Vec::new();
    let mut out = Run {
        name,
        lines: vec![name.to_string()],
        events: Vec::new(),
        best: Vec::new(),
        exit,
        recognised: 0,
    };
    let mut at = Cell::new(START.0, START.1, 0);
    let mut seen: Option<Cell> = None;
    let mut t = 0;
    loop {
        if roofed(at.x, at.y) {
            out.lines.push(format!("t={t:2} unseen"));
            out.events.push((t, Event::Unseen));
        } else {
            let tag = match seen {
                None => "start",
                Some(prev) => {
                    match observe_flow(&mut guess, &mut graph, &fields, prev, at, &mut moves) {
                        Ok(()) => {
                            let ev = Event::Learned {
                                from: (prev.x, prev.y),
                                to: (at.x, at.y),
                            };
                            out.events.push((t, ev));
                            "step"
                        }
                        Err(StepError::Jump) => {
                            out.events.push((t, Event::Jump));
                            "jump"
                        }
                        Err(e) => panic!("{e}"),
                    }
                }
            };
            seen = Some(at);
            let (best, _) = guess.best().expect("four goals");
            let mut hunters = [0u32; 4];
            guess.split(HUNTERS, &mut hunters);
            let c = guess.shares();
            out.lines.push(format!(
                "t={t:2} {tag} {},{} chance {} {} {} {} best {best} hunters {} {} {} {}",
                at.x, at.y, c[0], c[1], c[2], c[3], hunters[0], hunters[1], hunters[2], hunters[3]
            ));
            out.best.push((t, best));
        }
        let goal = if t < turn { first } else { exit };
        if fields[exit].cost(at) == Some(0) {
            break;
        }
        at = fields[goal].next(&mut graph, at).expect("a way");
        t += 1;
    }
    out.recognised = out
        .best
        .iter()
        .rev()
        .take_while(|&&(_, b)| b == exit)
        .last()
        .map_or(t, |&(s, _)| s);
    out.lines.push(format!(
        "arrived at exit {exit}, recognised at t={}",
        out.recognised
    ));
    out
}

/// The three runs.
pub fn run_scene() -> Vec<Run> {
    let forget = GoalGuessConfig::default().forget;
    vec![
        run("direct forget 13", 3, 0, 3, forget),
        run("feint forget 0", 1, FEINT, 2, 0),
        run("feint forget 13", 1, FEINT, 2, forget),
    ]
}

fn main() {
    for run in run_scene() {
        for line in run.lines {
            println!("{line}");
        }
    }
}
