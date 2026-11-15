//! A guard post without orders: guards join in on what they see their
//! comrades do. `cargo run -p sagax --example contagion_scene`
//!
//! Four guards (1 to 4) hold a post; an intruder (9) sneaks towards a
//! doorway in the east wall. Nobody gives an order. Every guard looks with
//! its own eye ([`look`], the guard preset, a cue rule "a firing or
//! fleeing comrade is recognised at once in the focus"), keeps its own
//! [`Contagion`] memory fed only from its own percepts, and decides with
//! one [`Decider`] whose inputs are its own sight of the intruder, a
//! grenade next to it and the [`Contagion::write_columns`] of the deeds
//! `attack` and `flee`:
//!
//! - hold (weight 1/8, always possible);
//! - attack: it recognises the intruder itself (weight 1/2);
//! - join the attack: the share of seen comrades attacking (weight 1/2);
//! - flee: a grenade went off next to it (weight 1);
//! - join the flight: the share of seen comrades fleeing (weight 1).
//!
//! A guard that attacks shows the cue `ATTACKING` (and fires: conspicuity
//! 2048); one that flees shows `FLEEING`, runs west a quarter cell per
//! tick (conspicuity 512) and keeps fleeing. Guard 1 stands by the doorway
//! and sees the intruder; guards 2 and 3 stand back, see guard 1 but not
//! the intruder (the wall is between); guard 4 stands behind the inner
//! wall and sees none of them. Then the intruder throws a grenade next to
//! guard 2: it flees, guard 3 sees it and flees too, guard 1 faces the
//! doorway, does not see it and keeps firing.
//!
//! The scene runs on a square grid and on a hex grid (the same cells,
//! axial on hex; the distances differ). Lines:
//!
//! - `<grid>`: `square` or `hex`, then per event
//! - `t=<tick> guard <id> sees <ally> start <deed>`: a contagion `Onset`
//!   record (`attack`, `flee`);
//! - `t=<tick> grenade at <x>,<y>`;
//! - `t=<tick> guard <id> <act> <doing>/<seen> share <share> nearest
//!   <cells> since <ticks>` when its choice changes (`hold`, `attack`,
//!   `join_attack`, `flee`, `join_flight`); the numbers are the inputs of
//!   the deed behind the act (`attack` for hold, attack and join_attack,
//!   `flee` for the others), `nearest` in whole cells (`-` for none) and
//!   `since` `-` for none.
//!
//! The same scene is in `examples/c/contagion.c`; both print the same
//! lines.

use sagax::debug::CallbackSink;
use sagax::perception::{
    Attention, Budget, ContagionTrace, CueRule, DEED_COLUMNS, Grid, Medium, SenseMap, World,
    cell_centre, look,
};
use sagax::prelude::*;

/// Fine units per cell.
pub const CELL: i32 = 256;
/// Ticks of a scene.
pub const END: u32 = 44;
/// The intruder.
pub const INTRUDER: u32 = 9;
/// The guards' ids are `1..=GUARDS`.
pub const GUARDS: usize = 4;
/// Cue bit: firing at someone.
pub const ATTACKING: u32 = 1;
/// Cue bit: running away.
pub const FLEEING: u32 = 2;
/// Deed index of the contagion config: attacking.
pub const DEED_ATTACK: usize = 0;
/// Deed index of the contagion config: fleeing.
pub const DEED_FLEE: usize = 1;
/// How long a sighting counts, ticks.
pub const WINDOW: u32 = 6;
/// The tick of the grenade.
pub const GRENADE_AT: u32 = 30;

/// Act (the Decider option index): hold the post.
pub const HOLD: u32 = 0;
/// Act: attack the intruder it sees itself.
pub const ATTACK: u32 = 1;
/// Act: join the attack it sees comrades make.
pub const JOIN_ATTACK: u32 = 2;
/// Act: flee from a grenade next to it.
pub const FLEE: u32 = 3;
/// Act: join the flight it sees comrades take.
pub const JOIN_FLIGHT: u32 = 4;
const ACTS: [&str; 5] = ["hold", "attack", "join_attack", "flee", "join_flight"];

/// Decider row: own sight of the intruder, grenade next to it, then the
/// columns of both deeds.
const SEES: usize = 0;
const GRENADE: usize = 1;
const ATTACK_COLS: usize = 2;
const FLEE_COLS: usize = ATTACK_COLS + DEED_COLUMNS;
const ROW: usize = FLEE_COLS + DEED_COLUMNS;

/// Each guard's cell and facing (heading, 0 = +x, 64 = +y).
pub const POSTS: [(i32, i32, u8); GUARDS] = [(8, 4, 0), (5, 1, 32), (5, 7, 224), (6, 11, 192)];
/// The intruder walks west a quarter cell per tick from this cell ...
pub const START: (i32, i32) = (18, 4);
/// ... and stops at this one.
pub const STOP: (i32, i32) = (13, 4);
/// The grenade's cell (guard 2's).
pub const GRENADE_CELL: (i32, i32) = (5, 1);

/// The walls (the same cells on both grids): the east wall at x = 11
/// (y 0 to 8) with a doorway at y = 4, the inner wall at y = 9 (x 0 to
/// 12).
pub fn wall(cell: Cell) -> bool {
    (cell.x == 11 && (0..=8).contains(&cell.y) && cell.y != 4)
        || (cell.y == 9 && (0..=12).contains(&cell.x))
}

/// The game's sense map: walls are opaque, daylight everywhere.
pub struct Post;

impl SenseMap for Post {
    fn sight(&mut self, cell: Cell) -> Medium {
        Medium {
            opaque: wall(cell),
            density: 0,
            heat_density: 0,
        }
    }

    fn light(&mut self, _cell: Cell) -> u8 {
        255
    }

    fn sound(&mut self, _cell: Cell) -> u32 {
        0
    }
}

/// What happened, for the tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// `guard` saw `ally` start deed `deed`.
    Onset {
        /// The observer.
        guard: u32,
        /// The ally it saw.
        ally: u32,
        /// The deed index.
        deed: u8,
    },
    /// The grenade went off.
    Grenade,
    /// `guard` chose `act`.
    Act {
        /// The guard.
        guard: u32,
        /// The act ([`HOLD`] to [`JOIN_FLIGHT`]).
        act: u32,
    },
}

/// One run.
pub struct Scene {
    /// `(tick, event)` in order.
    pub events: Vec<(u32, Event)>,
    /// The printed lines.
    pub lines: Vec<String>,
    /// Per tick the fine points of the guards (index 0 is guard 1) and
    /// the intruder (index `GUARDS`), as they were when the guards looked.
    pub points: Vec<[Pos; GUARDS + 1]>,
    /// Per tick the cues of the guards, as they were when they looked.
    pub cues: Vec<[u32; GUARDS]>,
}

/// The world of `grid`.
pub fn world(grid: Grid) -> World {
    match grid {
        Grid::Hex => World::hex(CELL),
        _ => World::square(CELL),
    }
}

/// The fine point at the centre of cell `(x, y)`.
pub fn centre(world: &World, (x, y): (i32, i32)) -> Pos {
    cell_centre(world, Cell::new(x, y, 0))
}

/// The guard's sense profile.
pub fn profile() -> SenseProfile {
    SenseProfile::guard(CELL).with_cue(CueRule {
        bits: ATTACKING | FLEEING,
        gain: 512,
        instant: 2,
        min_clarity: 0,
        range: 0,
    })
}

/// The guards' Decider (one for all: it keeps no state between calls).
fn decider() -> Decider {
    let mut d = Decider::new();
    let hold = d.add_option(ONE / 8);
    let attack = d.add_option(ONE / 2);
    let join_attack = d.add_option(ONE / 2);
    let flee = d.add_option(ONE);
    let join_flight = d.add_option(ONE);
    let share = |deed_cols: usize| Consideration::new(deed_cols as u32 + 2, Curve::Linear, 0, 256);
    let step = |input: usize, min| Consideration::new(input as u32, Curve::Step, min, min);
    assert!(d.add_consideration(hold, step(SEES, 0)));
    assert!(d.add_consideration(attack, step(SEES, 1)));
    assert!(d.add_consideration(join_attack, share(ATTACK_COLS)));
    assert!(d.add_consideration(flee, step(GRENADE, 1)));
    assert!(d.add_consideration(join_flight, share(FLEE_COLS)));
    d
}

/// The inputs of one deed as the log shows them.
fn show(inputs: &DeedInputs) -> String {
    let nearest = if inputs.nearest == i32::MAX {
        "-".to_string()
    } else {
        (inputs.nearest / CELL).to_string()
    };
    let since = if inputs.since == u32::MAX {
        "-".to_string()
    } else {
        inputs.since.to_string()
    };
    format!(
        "{}/{} share {} nearest {nearest} since {since}",
        inputs.doing, inputs.seen, inputs.share
    )
}

/// Runs the scene on `grid`.
pub fn run_scene(grid: Grid) -> Scene {
    let world = world(grid);
    let profile = profile();
    let stats = Stats::NORMAL;
    let budget = Budget {
        checks: 4,
        keep: 4,
        instant_cap: 4,
    };
    let config = ContagionConfig::new(grid)
        .with_deed(DeedRule {
            bits: ATTACKING,
            window: WINDOW,
        })
        .with_deed(DeedRule {
            bits: FLEEING,
            window: WINDOW,
        });
    let mut decider = decider();
    let candidates: Vec<Candidate> = (0..5).map(|o| Candidate::new(o, 0)).collect();

    let mut point: Vec<Pos> = POSTS.iter().map(|g| centre(&world, (g.0, g.1))).collect();
    let mut cues = [0u32; GUARDS];
    let mut act = [HOLD; GUARDS];
    let mut memory: Vec<Contagion> = (0..GUARDS).map(|_| Contagion::new(config, 4)).collect();
    let mut aware = [[Awareness::default(); GUARDS]; GUARDS];
    let mut attention = [Attention::default(); GUARDS];
    let mut map = Post;
    let mut percepts = Vec::new();
    let mut scene = Scene {
        events: Vec::new(),
        lines: vec![match grid {
            Grid::Hex => "hex".to_string(),
            _ => "square".to_string(),
        }],
        points: Vec::new(),
        cues: Vec::new(),
    };
    // One cell west, as a fine step (the same on square and axial hex).
    let west = centre(&world, (0, 0))[0] - centre(&world, (1, 0))[0];
    let from = centre(&world, START);
    let stop = centre(&world, STOP);

    for t in 0..END {
        let intruder = [(from[0] + west * t as i32 / 4).max(stop[0]), from[1], 0];
        let mut snapshot = [[0; 3]; GUARDS + 1];
        snapshot[..GUARDS].copy_from_slice(&point);
        snapshot[GUARDS] = intruder;
        scene.points.push(snapshot);
        scene.cues.push(cues);

        if t == GRENADE_AT {
            scene.events.push((t, Event::Grenade));
            scene.lines.push(format!(
                "t={t:3} grenade at {},{}",
                GRENADE_CELL.0, GRENADE_CELL.1
            ));
        }
        let mut chosen = act;
        for g in 0..GUARDS {
            let id = g as u32 + 1;
            // Everyone else, as this guard can see them now.
            let mut targets = Vec::with_capacity(GUARDS);
            for o in 0..GUARDS {
                if o == g {
                    continue;
                }
                targets.push(Target {
                    id: o as u32 + 1,
                    point: point[o],
                    size: 256,
                    conspicuity: match act[o] {
                        ATTACK | JOIN_ATTACK => 2048,
                        FLEE | JOIN_FLIGHT => 512,
                        _ => 128,
                    },
                    cues: cues[o],
                    ..Target::default()
                });
            }
            targets.push(Target {
                id: INTRUDER,
                point: intruder,
                size: 256,
                conspicuity: 256,
                ..Target::default()
            });
            let eye = Eye {
                id,
                point: point[g],
                facing: POSTS[g].2,
                profile: &profile,
                stats,
                disguise: Default::default(),
            };
            percepts.clear();
            look(
                &world,
                &mut map,
                t,
                &eye,
                &targets,
                &mut aware[g],
                &mut attention[g],
                budget,
                &mut percepts,
            );
            let mut onsets = Vec::new();
            let mut sink = CallbackSink(|r: ContagionTrace| {
                if let ContagionTrace::Onset { ally, deed } = r {
                    onsets.push((ally, deed));
                }
            });
            memory[g].learn_traced(&percepts, t, |subject, _| subject != INTRUDER, &mut sink);
            for (ally, deed) in onsets {
                scene.events.push((
                    t,
                    Event::Onset {
                        guard: id,
                        ally,
                        deed,
                    },
                ));
                scene.lines.push(format!(
                    "t={t:3} guard {id} sees {ally} start {}",
                    ["attack", "flee"][usize::from(deed)]
                ));
            }
            if matches!(act[g], FLEE | JOIN_FLIGHT) {
                continue; // a fleeing guard keeps fleeing
            }
            // Its own sight of the intruder: recognised and in view now.
            let sees = percepts
                .iter()
                .any(|p| p.subject == INTRUDER && p.stage == 2 && p.placed);
            let mut row = [0i32; ROW];
            row[SEES] = i32::from(sees);
            row[GRENADE] = i32::from(t == GRENADE_AT && point[g] == centre(&world, GRENADE_CELL));
            memory[g].write_columns(DEED_ATTACK, point[g], t, &mut row[ATTACK_COLS..]);
            memory[g].write_columns(DEED_FLEE, point[g], t, &mut row[FLEE_COLS..]);
            let rows = [row; 5];
            let choice = decider
                .choose(&candidates, &rows, Pick::Best, 0)
                .expect("valid rows")
                .expect("hold is always possible");
            let new = candidates[choice.candidate].option;
            if new != act[g] {
                chosen[g] = new;
                let deed = if new >= FLEE { DEED_FLEE } else { DEED_ATTACK };
                let inputs = memory[g].inputs(deed, point[g], t);
                scene.events.push((
                    t,
                    Event::Act {
                        guard: id,
                        act: new,
                    },
                ));
                scene.lines.push(format!(
                    "t={t:3} guard {id} {} {}",
                    ACTS[new as usize],
                    show(&inputs)
                ));
            }
        }
        // The choices take effect together, after everyone looked.
        for g in 0..GUARDS {
            act[g] = chosen[g];
            cues[g] = match act[g] {
                ATTACK | JOIN_ATTACK => ATTACKING,
                FLEE | JOIN_FLIGHT => FLEEING,
                _ => 0,
            };
            if cues[g] == FLEEING {
                point[g][0] += west / 4;
            }
        }
    }
    scene
}

fn main() {
    for grid in [Grid::Square, Grid::Hex] {
        for line in run_scene(grid).lines {
            println!("{line}");
        }
    }
}
