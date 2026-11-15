//! One guard watches a street: smoke, a crowd and a shot.
//! `cargo run -p sagax --example perception_scene`
//!
//! A square world, 256 fine units per cell, 30 ticks. The guard (id 1,
//! the guard preset with check interval 3 plus a cue rule "a drawn weapon
//! is recognised at once in the focus", reaction 2 ticks) stands at cell
//! (2, 6) and looks around east with `sweep_heading`. The game's sense map
//! has a smoke cell at (6, 6), a short wall at x = 8 (y 8 to 10, opaque
//! and damping sound) and dark ground from x = 14 on.
//!
//! - A walker (100, running: conspicuity 512) goes south along x = 10 from
//!   cell (10, 3), a quarter cell per tick, and passes behind the smoke
//!   (its clarity drops).
//! - A crowd of six (200 to 205, standing) is at x = 13, 14; `count_crowds`
//!   gives each its crowd. The guard checks at most 3 targets per tick
//!   (one kept), so the crowd stays unnoticed. From tick 20, 203 draws a
//!   weapon: an instant cue, checked outside the budget.
//! - At tick 12 a shot (stimulus 1 from 300, channel 0, a straight line
//!   with the map's damping, fuzzed) is heard without an awareness.
//!
//! One line per percept:
//! `<tick> <observer> <subject> <sense> <event> <stage> <x>,<y> <clarity>`
//! (sense `sight`, `thermal`, `scanner`, `hearing`; event `noticed`,
//! `recognized`, `seen`, `cues`, `lost`, `forgotten`, `seen_through`,
//! `heard`; the whole cell), then per tick `<tick> trace` and the kind of
//! every trace record (`picked`, `skipped`, `line`, `gain`, `instant`,
//! `stage`, `doubt`, `heard`, `unheard`).
//!
//! The same scene is in `examples/c/perception.c`; both print the same
//! lines.

use sagax::debug::CallbackSink;
use sagax::decide::memory::{LOOK_OFFSETS, sweep_heading};
use sagax::perception::{
    Attention, Awareness, Budget, CueRule, Eye, Medium, Percept, PerceptKind, PerceptionTrace,
    Sense, SenseMap, SenseProfile, Spread, Stats, Stimulus, Target, World, cell_centre,
    count_crowds, hear_traced, look_traced,
};
use sagax::{Cell, Pos};

/// Fine units per cell.
const CELL: i32 = 256;
/// Ticks of the scene.
const END: u32 = 30;
/// The cue bit of a drawn weapon.
const WEAPON: u32 = 1;
/// The walker.
const WALKER: u32 = 100;
/// The crowd member that draws a weapon, and when.
const DRAWS: u32 = 203;
const DRAWS_AT: u32 = 20;
/// The tick of the shot.
const SHOT_AT: u32 = 12;

/// The game's sense map.
struct Street;

impl SenseMap for Street {
    fn sight(&mut self, cell: Cell) -> Medium {
        Medium {
            opaque: wall(cell),
            density: if (cell.x, cell.y) == (6, 6) { 384 } else { 0 },
            heat_density: 0,
        }
    }

    fn light(&mut self, cell: Cell) -> u8 {
        if cell.x >= 14 { 40 } else { 255 }
    }

    fn sound(&mut self, cell: Cell) -> u32 {
        if wall(cell) { 512 } else { 0 }
    }
}

/// The short wall at x = 8, y 8 to 10.
fn wall(cell: Cell) -> bool {
    cell.x == 8 && (8..=10).contains(&cell.y)
}

fn centre(world: &World, x: i32, y: i32) -> Pos {
    cell_centre(world, Cell::new(x, y, 0))
}

fn sense_name(s: Sense) -> &'static str {
    match s {
        Sense::Sight => "sight",
        Sense::Thermal => "thermal",
        Sense::Scanner => "scanner",
        Sense::Hearing(_) => "hearing",
    }
}

fn event_name(e: PerceptKind) -> &'static str {
    match e {
        PerceptKind::Noticed => "noticed",
        PerceptKind::Recognized => "recognized",
        PerceptKind::Seen => "seen",
        PerceptKind::Cues { .. } => "cues",
        PerceptKind::Lost => "lost",
        PerceptKind::Forgotten => "forgotten",
        PerceptKind::SeenThrough => "seen_through",
        PerceptKind::Heard => "heard",
    }
}

fn trace_name(r: &PerceptionTrace) -> &'static str {
    match r {
        PerceptionTrace::Picked { .. } => "picked",
        PerceptionTrace::Skipped { .. } => "skipped",
        PerceptionTrace::Line { .. } => "line",
        PerceptionTrace::Gain { .. } => "gain",
        PerceptionTrace::Instant { .. } => "instant",
        PerceptionTrace::Stage { .. } => "stage",
        PerceptionTrace::Doubt { .. } => "doubt",
        PerceptionTrace::Heard { .. } => "heard",
        PerceptionTrace::Unheard { .. } => "unheard",
    }
}

fn print(t: u32, p: &Percept) {
    println!(
        "{t} {} {} {} {} {} {},{} {}",
        p.observer,
        p.subject,
        sense_name(p.sense),
        event_name(p.event),
        p.stage,
        p.cell[0],
        p.cell[1],
        p.clarity
    );
}

fn main() {
    let mut world = World::square(CELL);
    world.channels[0].spread = Spread::Straight;
    world.channels[0].fuzz = 64;
    world.channels[0].speed = 4 * CELL;
    let mut profile = SenseProfile::guard(CELL).with_cue(CueRule {
        bits: WEAPON,
        gain: 512,
        instant: 2,
        min_clarity: 0,
        range: 0,
    });
    profile.check_interval = 3;
    let stats = Stats {
        reaction_ticks: 2,
        ..Stats::NORMAL
    };
    let budget = Budget {
        checks: 3,
        keep: 1,
        instant_cap: 1,
    };
    let crowd_at = [(13, 3), (13, 4), (14, 3), (14, 4), (13, 5), (14, 5)];
    let mut targets = vec![Target {
        id: WALKER,
        size: 256,
        conspicuity: 512,
        ..Target::default()
    }];
    for (i, &(x, y)) in crowd_at.iter().enumerate() {
        targets.push(Target {
            id: 200 + i as u32,
            point: centre(&world, x, y),
            size: 256,
            conspicuity: 128,
            ..Target::default()
        });
    }
    let mut crowds = [0u8; 7];
    count_crowds(
        &targets,
        2 * CELL,
        |t| u32::from(t.id != WALKER),
        &mut crowds,
    );
    for (t, &c) in targets.iter_mut().zip(&crowds) {
        t.crowd = c;
    }
    let mut aware = [Awareness::default(); 7];
    let mut attention = Attention::default();
    let mut map = Street;
    let mut percepts = Vec::new();
    let mut kinds: Vec<&'static str> = Vec::new();

    for t in 0..END {
        targets[0].point = [10 * CELL + 128, 3 * CELL + t as i32 * CELL / 4, 0];
        if t >= DRAWS_AT {
            targets[1 + (DRAWS - 200) as usize].cues = WEAPON;
        }
        let eye = Eye {
            id: 1,
            point: centre(&world, 2, 6),
            facing: sweep_heading(0, t % 32, 4, &LOOK_OFFSETS) as u8,
            profile: &profile,
            stats,
            disguise: Default::default(),
        };
        kinds.clear();
        percepts.clear();
        let mut sink = CallbackSink(|r: PerceptionTrace| kinds.push(trace_name(&r)));
        look_traced(
            &world,
            &mut map,
            t,
            &eye,
            &targets,
            &mut aware,
            &mut attention,
            budget,
            &mut percepts,
            &mut sink,
        );
        if t == SHOT_AT {
            let shot = Stimulus {
                id: 1,
                source: 300,
                kind: 1,
                point: centre(&world, 16, 9),
                loudness: 16 * CELL,
                channel: 0,
                disguise: 0,
                at: t,
            };
            hear_traced(
                &world,
                &mut map,
                t,
                &eye,
                &shot,
                None,
                None,
                &mut percepts,
                7,
                &mut sink,
            );
        }
        for p in &percepts {
            print(t, p);
        }
        println!("{t} trace {}", kinds.join(" "));
    }
}
