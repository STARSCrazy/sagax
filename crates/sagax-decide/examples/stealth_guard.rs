//! A turn-based stealth scene on a hex grid, built from the memory blocks
//! and the state machine: a guard hears a noise, grows suspicious and goes
//! to look, sees the player, shouts to the guards in range, loses the
//! player, searches around the last seen place and calms down again.
//! `cargo run -p sagax-decide --example stealth_guard`
//!
//! Cells are axial hex coordinates `(q, r)`, one round is one tick, a guard
//! walks one cell per round and the player is scripted. Blocks used:
//! [`Knowledge`] (heard, seen and told facts), [`route`] with
//! [`Route::InRange`] and [`Metric::Hex`] (who hears the noise and the
//! shout), [`Inbox`] with [`arrival`] (a shout takes one round per cell),
//! [`Alertness`] (unaware, suspicious, searching, alert), [`Areas`] (a lit
//! cell costs extra to walk through), [`Timers`] (no second shout for a
//! while) and [`points_around`] with [`Sine::Fine`] (where to search).

use sagax_decide::memory::{
    AlertConfig, Alertness, Area, Areas, Evict, Fact, Inbox, Knowledge, Message, Metric,
    PointPattern, Pos, Route, SOURCE_HEARD, SOURCE_OWN, SOURCE_TOLD, Sine, Timers, arrival,
    distance, points_around, route,
};
use sagax_decide::state::{ANY, StateMachine, Transition};

/// State: at the post (or walking back to it).
pub const IDLE: u32 = 0;
/// State: walking to a clue (a noise or a told place).
pub const INVESTIGATE: u32 = 1;
/// State: after the player.
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
/// Alertness level: the player was seen.
pub const ALERT: u8 = 3;

const F_SEES: u64 = 1;
const F_CALM: u64 = 2;
const F_CLUE: u64 = 4;
const F_ARRIVED: u64 = 8;
const F_LOST: u64 = 16;

const PLAYER: u32 = 99;
/// Fact kinds.
const NOISE: u32 = 1;
const SEEN: u32 = 2;
/// Area kind: a lit cell.
const LIT: u32 = 1;
const LIT_COST: u32 = 20;
/// Timer id: shouted lately.
const SHOUTED: u32 = 1;

const SIGHT: i32 = 2;
const NOISE_RANGE: i64 = 5;
const SHOUT_RANGE: i64 = 6;
const LOST_ROUNDS: u32 = 2;
const SHOUT_AGAIN: u32 = 10;
const NOISE_ROUND: u32 = 2;
const ALERT_CFG: AlertConfig = AlertConfig {
    max: 100,
    decay_per_tick: 10,
    hold: 3,
    hysteresis: 5,
    count: 3,
    thresholds: [20, 50, 80, 0, 0, 0, 0, 0],
};
const HEAR: u32 = 30;
const SEE: u32 = 100;
const TOLD: u32 = 60;
/// Neighbours of a hex cell, in the order a step tries them.
const DIRS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

/// What happened, for the trace (`t` is the round).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Event {
    /// A guard heard the noise.
    Heard { t: u32, guard: u32 },
    /// A guard saw the player.
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

struct Guard {
    post: Pos,
    pos: Pos,
    sm: StateMachine,
    known: Knowledge,
    alert: Alertness,
    inbox: Inbox,
    timers: Timers,
    /// Search centre and current step.
    search: (Pos, usize),
}

fn hex(a: Pos, b: Pos) -> i32 {
    distance(Metric::Hex, i64::from(b[0] - a[0]), i64::from(b[1] - a[1])) as i32
}

/// One cell toward `goal`: the neighbour with the least distance (times 10)
/// plus the cost of the lit areas; the first one on a tie.
fn step(from: Pos, goal: Pos, lit: &Areas) -> Pos {
    if from == goal {
        return from;
    }
    let score = |n: Pos| 10 * hex(n, goal) as u32 + lit.cost(n, from, goal);
    let cells = DIRS.map(|(dq, dr)| [from[0] + dq, from[1] + dr, 0]);
    let mut best = cells[0];
    for &n in &cells[1..] {
        if score(n) < score(best) {
            best = n;
        }
    }
    best
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

/// The player waits at (5, 0), sprints along +q in rounds 7 and 8, then hides.
fn player_at(t: u32) -> (Pos, bool) {
    match t {
        0..=6 => ([5, 0, 0], true),
        7 => ([7, 0, 0], true),
        8 => ([9, 0, 0], true),
        _ => ([9, 0, 0], false),
    }
}

/// The place the guard should go to: what it saw (or was told), else what it heard.
fn clue(g: &Guard) -> Option<&Fact> {
    g.known.get(PLAYER, SEEN).or(g.known.get(PLAYER, NOISE))
}

fn fact(kind: u32, pos: Pos, t: u32, source: u8) -> Fact {
    Fact {
        subject: PLAYER,
        kind,
        pos,
        seen_at: t,
        source,
        ..Fact::default()
    }
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
        sm,
        known: Knowledge::new(4, Evict::Oldest).unwrap(),
        alert: Alertness::default(),
        inbox: Inbox::new(4).unwrap(),
        timers: Timers::new(1).unwrap(),
        search: ([0; 3], 0),
    }
}

/// `(id, q, r)` of every guard but `skip`, for [`route`].
fn candidates(guards: &[Guard], skip: Option<usize>) -> Vec<(u32, i32, i32)> {
    let others = guards.iter().enumerate().filter(|&(j, _)| Some(j) != skip);
    others
        .map(|(j, g)| (j as u32, g.pos[0], g.pos[1]))
        .collect()
}

/// Runs rounds `1 ..= end` and returns what happened.
pub(crate) fn run_scene(end: u32) -> Vec<Event> {
    let mut ev = Vec::new();
    let mut guards = [guard([0, 0, 0]), guard([4, -6, 0]), guard([-2, 0, 0])];
    let mut lit = Areas::new(4).unwrap();
    let torch = Area {
        kind: LIT,
        pos: [2, 0, 0],
        radius: 1,
        strength: LIT_COST,
        lifetime: u32::MAX,
        ..Area::default()
    };
    lit.add(torch, 0);
    let (mut picked, mut msgs) = (Vec::new(), Vec::new());
    for t in 1..=end {
        let (player, visible) = player_at(t);
        if t == NOISE_ROUND {
            // A noise reaches every guard in range, whatever it is doing.
            let all = candidates(&guards, None);
            route(
                Route::InRange(NOISE_RANGE),
                (player[0], player[1]),
                &all,
                Metric::Hex,
                1,
                &mut picked,
            );
            for &i in &picked {
                let g = &mut guards[i as usize];
                g.known.learn(fact(NOISE, player, t, SOURCE_HEARD), t);
                g.alert.stimulus(HEAR, t, &ALERT_CFG);
                ev.push(Event::Heard { t, guard: i });
            }
        }
        for i in 0..guards.len() {
            let me = i as u32;
            // Shouts that have arrived.
            msgs.clear();
            guards[i].inbox.take_due(t, &mut msgs);
            for m in &msgs {
                guards[i].known.learn_message(m, SOURCE_TOLD, t);
                guards[i].alert.stimulus(TOLD, t, &ALERT_CFG);
                ev.push(Event::Told { t, guard: me });
            }
            // Looking.
            let sees = visible && hex(guards[i].pos, player) <= SIGHT;
            if sees {
                let g = &mut guards[i];
                g.known.learn(fact(SEEN, player, t, SOURCE_OWN), t);
                g.alert.stimulus(SEE, t, &ALERT_CFG);
                ev.push(Event::Saw { t, guard: me });
                if !g.timers.active(SHOUTED, t) {
                    g.timers.set(SHOUTED, t, SHOUT_AGAIN);
                    ev.push(Event::Shout { t, guard: me });
                    let at = g.pos;
                    let shout = Message {
                        kind: SEEN,
                        subject: PLAYER,
                        pos: player,
                        seen_at: t,
                        sender: me,
                        ..Message::default()
                    };
                    let others = candidates(&guards, Some(i));
                    route(
                        Route::InRange(SHOUT_RANGE),
                        (at[0], at[1]),
                        &others,
                        Metric::Hex,
                        1,
                        &mut picked,
                    );
                    for &j in &picked {
                        let o = &mut guards[j as usize];
                        // One round per cell.
                        let when = arrival(t, 0, hex(at, o.pos) as u32, 1);
                        o.inbox.push(shout, when);
                    }
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
            if g.known
                .get(PLAYER, SEEN)
                .is_some_and(|f| t.wrapping_sub(f.seen_at) >= LOST_ROUNDS)
            {
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
                    // Calm again: forget the player.
                    IDLE => {
                        g.known.remove_subject(PLAYER);
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
            g.pos = step(g.pos, goal, &lit);
        }
    }
    ev
}

fn main() {
    for e in run_scene(30) {
        match e {
            Event::Heard { t, guard } => println!("round {t:>2}  guard {guard}: hears a noise"),
            Event::Saw { t, guard } => println!("round {t:>2}  guard {guard}: sees the player"),
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
}
