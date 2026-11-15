//! A turn-based stealth scene on a hex grid, built from the perception
//! block, the memory blocks and the state machine: a guard hears a noise,
//! goes to look, notices and then recognises a sneaker, shouts to the
//! guards in range, loses the sneaker, searches around the last seen place
//! and calms down again. The scene runs once in daylight and once at night.
//! `cargo run -p sagax --example stealth_guard`
//!
//! Cells are axial hex coordinates `(q, r)`, one round is one tick, a guard
//! walks one cell per round and faces the way it walked (all face -q at
//! the start), and the sneaker is scripted. Blocks used:
//!
//! - sight: [`look`] on a hex [`World`] with [`SenseProfile::guard`]; the
//!   light is [`Lit`] (the time of day) plus the torch's lit cell, so at
//!   night the sneaker is noticed later;
//! - hearing: the noise is a [`Stimulus`] on channel 0 with
//!   [`Spread::Straight`], checked by [`hear`] for every guard;
//! - memory: [`learn_percepts`] turns the percepts into [`Knowledge`]
//!   facts (noticed, recognised, heard) and [`Alertness`] stimuli
//!   (unaware, suspicious, searching, alert);
//! - [`route`] with [`Route::InRange`] and [`Metric::Hex`] (who hears the
//!   shout), [`Inbox`] with [`arrival`] (a shout takes one round per cell),
//!   [`Areas`] (the torch's cell costs extra to walk through), [`Timers`]
//!   (no second shout for a while) and [`points_around`] with
//!   [`Sine::Fine`] (where to search).

use sagax::decide::memory::{
    AlertConfig, Alertness, Area, Areas, Inbox, Route, SOURCE_TOLD, Sine, Timers, arrival,
    distance, points_around, route,
};
use sagax::decide::state::{ANY, StateMachine, Transition};
use sagax::perception::{
    AlertAmounts, Attention, Budget as SightBudget, FactKinds, Lit, OpenAir, PerceptKind, Sense,
    SenseMap, Spread, World, cell_centre, hex_heading,
};
use sagax::prelude::*;

/// State: at the post (or walking back to it).
pub const IDLE: u32 = 0;
/// State: walking to a clue (a noise or a told place).
pub const INVESTIGATE: u32 = 1;
/// State: after the sneaker.
pub const CHASE: u32 = 2;
/// State: walking the search points around the last known place.
pub const SEARCH: u32 = 3;
const NAMES: [&str; 4] = ["Idle", "Investigate", "Chase", "Search"];

/// Alertness level: nothing noticed.
pub const UNAWARE: u8 = 0;
/// Alertness level: something is odd.
pub const SUSPICIOUS: u8 = 1;
/// Alertness level: someone is here.
pub const SEARCHING: u8 = 2;
/// Alertness level: the sneaker was recognised.
pub const ALERT: u8 = 3;

/// Light of the day ([`Lit::ambient`]).
pub const DAYLIGHT: u8 = 255;
/// Light of the night: a moonlit yard.
pub const NIGHT: u8 = 112;

const F_SEES: u64 = 1;
const F_CALM: u64 = 2;
const F_CLUE: u64 = 4;
const F_ARRIVED: u64 = 8;
const F_LOST: u64 = 16;

/// The sneaker's id.
pub const SNEAKER: u32 = 99;
/// Fact kinds.
pub const NOISE: u32 = 1;
/// Fact kind: the sneaker recognised (or told).
pub const SEEN: u32 = 2;
/// Fact kind: something noticed, not yet recognised.
pub const NOTICED: u32 = 3;
const KINDS: FactKinds = FactKinds {
    noticed_kind: NOTICED,
    seen_kind: SEEN,
    heard_kind: NOISE,
    disguised_kind: None,
};
/// Area kind: the torch's lit cell.
const LIT: u32 = 1;
const LIT_COST: u32 = 20;
/// Timer id: shouted lately.
const SHOUTED: u32 = 1;

/// Fine units per hex.
const CELL: i32 = 256;
/// The noise is heard up to 5 hexes away.
const NOISE_LOUDNESS: i32 = 5 * CELL;
const SHOUT_RANGE: i64 = 10;
const LOST_ROUNDS: u32 = 2;
const SHOUT_AGAIN: u32 = 10;
/// The round the sneaker knocks something over.
pub const NOISE_ROUND: u32 = 2;
const SEED: u64 = 11;
const ALERT_CFG: AlertConfig = AlertConfig {
    max: 100,
    decay_per_tick: 10,
    hold: 3,
    hysteresis: 5,
    count: 3,
    thresholds: [20, 50, 80, 0, 0, 0, 0, 0],
};
const AMOUNTS: AlertAmounts = AlertAmounts {
    noticed: 40,
    recognized: 100,
    seen: 10,
    heard: 70,
    cue_bits: [0; 8],
    cue_amount: [0; 8],
};
const TOLD: u32 = 60;
/// Neighbours of a hex cell, in the order a step tries them, and the hex
/// direction ([`hex_heading`]) of each.
const DIRS: [(i32, i32, u8); 6] = [
    (1, 0, 0),
    (1, -1, 5),
    (0, -1, 4),
    (-1, 0, 3),
    (-1, 1, 2),
    (0, 1, 1),
];

/// What happened, for the trace (`t` is the round).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// A guard heard the noise.
    Heard { t: u32, guard: u32 },
    /// A guard noticed someone.
    Noticed { t: u32, guard: u32 },
    /// A guard recognised the sneaker.
    Saw { t: u32, guard: u32 },
    /// A guard shouted.
    Shout { t: u32, guard: u32 },
    /// A shout reached a guard.
    Told { t: u32, guard: u32 },
    /// The alertness level changed.
    Level {
        t: u32,
        guard: u32,
        from: u8,
        to: u8,
    },
    /// The state changed.
    State {
        t: u32,
        guard: u32,
        from: u32,
        to: u32,
    },
    /// A guard picked search point `step`.
    Searching {
        t: u32,
        guard: u32,
        step: usize,
        at: Pos,
    },
}

/// One guard: where it is, what it knows and how it sees.
pub(crate) struct Guard {
    post: Pos,
    /// Where it is (a cell).
    pub pos: Pos,
    /// Its heading (0..=255).
    pub facing: u8,
    sm: StateMachine,
    /// What it knows.
    pub known: Knowledge,
    alert: Alertness,
    inbox: Inbox,
    timers: Timers,
    /// What it perceived of the sneaker.
    aware: Awareness,
    attention: Attention,
    /// Search centre and current step.
    search: (Pos, usize),
}

/// The yard as the guards see it: the light of the time of day ([`Lit`]
/// over open air), and the torch's lit cell always bright.
struct Yard<'a> {
    day: Lit<OpenAir>,
    torch: &'a Areas,
}

impl SenseMap for Yard<'_> {
    fn light(&mut self, cell: Cell) -> u8 {
        let lit = self.torch.contains_kind([cell.x, cell.y, cell.level], LIT);
        if lit.is_some() {
            255
        } else {
            self.day.light(cell)
        }
    }
}

fn hex(a: Pos, b: Pos) -> i32 {
    distance(Metric::Hex, i64::from(b[0] - a[0]), i64::from(b[1] - a[1])) as i32
}

/// The fine point at the centre of hex `p`.
fn centre(world: &World, p: Pos) -> Pos {
    cell_centre(world, Cell::new(p[0], p[1], p[2]))
}

/// One cell toward `goal`: the neighbour with the least distance (times 10)
/// plus the cost of the lit areas; the first one on a tie. Also the heading
/// of that step (`None` when already there).
fn step(from: Pos, goal: Pos, lit: &Areas) -> (Pos, Option<u8>) {
    if from == goal {
        return (from, None);
    }
    let score = |n: Pos| 10 * hex(n, goal) as u32 + lit.cost(n, from, goal);
    let cells = DIRS.map(|(dq, dr, dir)| ([from[0] + dq, from[1] + dr, 0], dir));
    let mut best = cells[0];
    for &n in &cells[1..] {
        if score(n.0) < score(best.0) {
            best = n;
        }
    }
    (best.0, Some(hex_heading(best.1)))
}

fn pattern() -> PointPattern {
    PointPattern {
        first_distance: 2,
        distances: vec![3, 4, 4],
        sine: Sine::Fine,
        min: (-100, -100),
        max: (100, 100),
        ..PointPattern::default()
    }
}

/// The sneaker: fumbles at a lock at (5, 0) (conspicuity 256), sprints
/// along +q in rounds 11 and 12 (512), then slips down into a cellar
/// (level -1: levels do not see each other) at (9, 0).
pub fn sneaker_at(t: u32) -> (Pos, u16) {
    match t {
        0..=10 => ([5, 0, 0], 256),
        11 => ([7, 0, 0], 512),
        12 => ([9, 0, 0], 512),
        _ => ([9, 0, -1], 128),
    }
}

/// The place the guard should go to: what it saw (or was told), else what
/// it noticed, else what it heard.
fn clue(g: &Guard) -> Option<&Fact> {
    let k = &g.known;
    k.get(SNEAKER, SEEN)
        .or(k.get(SNEAKER, NOTICED))
        .or(k.get(SNEAKER, NOISE))
}

/// The guard's last own sighting (noticed or recognised).
fn sighting(g: &Guard) -> Option<&Fact> {
    let k = &g.known;
    k.get(SNEAKER, SEEN)
        .filter(|f| f.source != SOURCE_TOLD)
        .or(k.get(SNEAKER, NOTICED))
}

fn guard(post: Pos) -> Guard {
    let mut sm = StateMachine::new(4, IDLE).unwrap();
    // Seeing comes first, calm before "arrived" (a calm guard gives up).
    for (from, to, when) in [
        (ANY, CHASE, F_SEES),
        (INVESTIGATE, IDLE, F_CALM),
        (SEARCH, IDLE, F_CALM),
        (IDLE, INVESTIGATE, F_CLUE),
        (INVESTIGATE, SEARCH, F_ARRIVED),
        (CHASE, SEARCH, F_LOST),
    ] {
        sm.add_transition(Transition::new(from, to, when)).unwrap();
    }
    Guard {
        post,
        pos: post,
        facing: hex_heading(3),
        sm,
        known: Knowledge::new(4, Evict::Oldest).unwrap(),
        alert: Alertness::default(),
        inbox: Inbox::new(4).unwrap(),
        timers: Timers::new(1).unwrap(),
        aware: Awareness::default(),
        attention: Attention::default(),
        search: ([0; 3], 0),
    }
}

/// `(id, q, r)` of every guard but `skip`, for [`route`].
fn candidates(guards: &[Guard], skip: usize) -> Vec<(u32, i32, i32)> {
    let others = guards.iter().enumerate().filter(|&(j, _)| j != skip);
    others
        .map(|(j, g)| (j as u32, g.pos[0], g.pos[1]))
        .collect()
}

/// What a scene did.
pub(crate) struct Scene {
    /// What happened, in order.
    pub events: Vec<Event>,
    /// The guards at the end (read by the test).
    #[allow(dead_code)]
    pub guards: Vec<Guard>,
}

/// Runs rounds `1 ..= end` with the light `ambient` ([`DAYLIGHT`],
/// [`NIGHT`]).
pub(crate) fn run_scene(end: u32, ambient: u8) -> Scene {
    let mut ev = Vec::new();
    let mut world = World::hex(CELL);
    world.channels[0].spread = Spread::Straight;
    let profile = SenseProfile::guard(CELL);
    let budget = SightBudget {
        checks: 1,
        keep: 1,
        instant_cap: 0,
    };
    let mut guards = vec![guard([0, 0, 0]), guard([4, -6, 0]), guard([-2, 0, 0])];
    let mut lit = Areas::new(4).unwrap();
    let torch = Area {
        kind: LIT,
        pos: [2, 0, 0],
        radius: 1,
        strength: LIT_COST,
        lifetime: u32::MAX,
        ..Area::default()
    };
    lit.add(0, torch);
    let (mut picked, mut msgs, mut percepts) = (Vec::new(), Vec::new(), Vec::new());
    for t in 1..=end {
        let mut yard = Yard {
            day: Lit {
                inner: OpenAir,
                ambient,
            },
            torch: &lit,
        };
        let (at, conspicuity) = sneaker_at(t);
        let sneaker = Target {
            id: SNEAKER,
            point: centre(&world, at),
            size: 256,
            conspicuity,
            ..Target::default()
        };
        for i in 0..guards.len() {
            let me = i as u32;
            let g = &mut guards[i];
            let eye = Eye {
                id: me,
                point: centre(&world, g.pos),
                facing: g.facing,
                profile: &profile,
                stats: Stats::NORMAL,
                disguise: Default::default(),
            };
            percepts.clear();
            // Hearing: the noise reaches every guard in range, whatever it
            // is doing (no awareness: a sound is a clue, not a sighting).
            if t == NOISE_ROUND {
                let noise = Stimulus {
                    id: 1,
                    source: SNEAKER,
                    kind: NOISE,
                    point: centre(&world, at),
                    loudness: NOISE_LOUDNESS,
                    channel: 0,
                    at: t,
                };
                if hear(
                    &world,
                    &mut yard,
                    t,
                    &eye,
                    &noise,
                    None,
                    None,
                    &mut percepts,
                    SEED,
                ) {
                    ev.push(Event::Heard { t, guard: me });
                } else {
                    percepts.clear();
                }
            }
            // Looking.
            look(
                &world,
                &mut yard,
                t,
                &eye,
                &[sneaker],
                std::slice::from_mut(&mut g.aware),
                &mut g.attention,
                budget,
                &mut percepts,
            );
            learn_percepts(
                &mut g.known,
                &mut g.alert,
                &ALERT_CFG,
                t,
                &percepts,
                &KINDS,
                &AMOUNTS,
            );
            let mut sees = false;
            let mut recognised = false;
            for p in &percepts {
                if p.sense == Sense::Sight && p.placed {
                    sees = true;
                }
                match p.event {
                    PerceptKind::Noticed => ev.push(Event::Noticed { t, guard: me }),
                    PerceptKind::Recognized => {
                        recognised = true;
                        ev.push(Event::Saw { t, guard: me });
                    }
                    _ => {}
                }
            }
            // Shouts that have arrived.
            msgs.clear();
            g.inbox.take_due(t, &mut msgs);
            for m in &msgs {
                g.known.learn_message(t, m, SOURCE_TOLD);
                g.alert.stimulus(TOLD, t, &ALERT_CFG);
                ev.push(Event::Told { t, guard: me });
            }
            // A recognised sneaker: shout, unless shouted lately.
            if recognised && !g.timers.active(SHOUTED, t) {
                g.timers.set(SHOUTED, t, SHOUT_AGAIN);
                ev.push(Event::Shout { t, guard: me });
                let from = g.pos;
                let shout = Message {
                    kind: SEEN,
                    subject: SNEAKER,
                    pos: at,
                    seen_at: t,
                    sender: me,
                    ..Message::default()
                };
                let others = candidates(&guards, i);
                route(
                    Route::InRange(SHOUT_RANGE),
                    (from[0], from[1]),
                    &others,
                    Metric::Hex,
                    1,
                    &mut picked,
                );
                for &j in &picked {
                    let o = &mut guards[j as usize];
                    // One round per cell.
                    let when = arrival(t, 0, hex(from, o.pos) as u32, 1);
                    assert!(o.inbox.push(shout, when));
                }
            }
            let g = &mut guards[i];
            if let Some((from, to)) = g.alert.update(t, &ALERT_CFG) {
                ev.push(Event::Level {
                    t,
                    guard: me,
                    from,
                    to,
                });
            }
            // Conditions.
            let level = g.alert.level();
            let mut flags = 0;
            if sees {
                flags |= F_SEES;
            }
            if level == UNAWARE {
                flags |= F_CALM;
            }
            if level >= SUSPICIOUS && clue(g).is_some() {
                flags |= F_CLUE;
            }
            if clue(g).is_some_and(|c| c.pos == g.pos) {
                flags |= F_ARRIVED;
            }
            // "Lost": the own sighting is too old (it may be refreshed above).
            if sighting(g).is_some_and(|f| t.wrapping_sub(f.seen_at) >= LOST_ROUNDS) {
                flags |= F_LOST;
            }
            if let Some(c) = g.sm.update(t, flags) {
                ev.push(Event::State {
                    t,
                    guard: me,
                    from: c.from,
                    to: c.to,
                });
                match c.to {
                    SEARCH => {
                        let centre = clue(g).unwrap().pos;
                        let at = points_around(me, 0, centre, &pattern()).unwrap();
                        g.search = (centre, 0);
                        ev.push(Event::Searching {
                            t,
                            guard: me,
                            step: 0,
                            at,
                        });
                    }
                    // Calm again: forget the sneaker.
                    IDLE => {
                        g.known.remove_subject(SNEAKER);
                    }
                    _ => {}
                }
            }
            // Acting: one cell per round.
            let goal = match g.sm.state() {
                IDLE => g.post,
                INVESTIGATE | CHASE => clue(g).unwrap().pos,
                _ => {
                    let (centre, s) = g.search;
                    let mut at = points_around(me, s, centre, &pattern()).unwrap();
                    if at == g.pos
                        && let Some(next) = points_around(me, s + 1, centre, &pattern())
                    {
                        g.search.1 = s + 1;
                        ev.push(Event::Searching {
                            t,
                            guard: me,
                            step: s + 1,
                            at: next,
                        });
                        at = next;
                    }
                    at
                }
            };
            let goal = [goal[0], goal[1], 0];
            let (pos, heading) = step(g.pos, goal, &lit);
            g.pos = pos;
            if let Some(h) = heading {
                g.facing = h;
            }
        }
    }
    Scene { events: ev, guards }
}

fn print(e: Event) {
    match e {
        Event::Heard { t, guard } => println!("round {t:>2}  guard {guard}: hears a noise"),
        Event::Noticed { t, guard } => println!("round {t:>2}  guard {guard}: notices someone"),
        Event::Saw { t, guard } => println!("round {t:>2}  guard {guard}: sees the sneaker"),
        Event::Shout { t, guard } => println!("round {t:>2}  guard {guard}: shouts"),
        Event::Told { t, guard } => println!("round {t:>2}  guard {guard}: hears the shout"),
        Event::Level { t, guard, from, to } => {
            println!("round {t:>2}  guard {guard}: alertness {from} -> {to}")
        }
        Event::State { t, guard, from, to } => println!(
            "round {t:>2}  guard {guard}: {} -> {}",
            NAMES[from as usize], NAMES[to as usize]
        ),
        Event::Searching { t, guard, step, at } => {
            println!("round {t:>2}  guard {guard}: search point {step} at {at:?}")
        }
    }
}

fn main() {
    for (name, light) in [("daylight", DAYLIGHT), ("night", NIGHT)] {
        println!("{name}");
        for e in run_scene(30, light).events {
            print(e);
        }
    }
}
