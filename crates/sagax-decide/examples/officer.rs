//! An officer recipe built only from the generic memory blocks and the state
//! machine: a witness chases (on a stall it ignores the actor and searches
//! as if lost), calls for help, loses sight and searches;
//! the three nearest free officers come to help; the table forgets.
//! `cargo run -p sagax-decide --example officer`
//!
//! The scene runs at 17 ticks per second on a plain grid. Who sees what is
//! scripted; walking moves at most `SPEED` per axis and tick.
//!
//! Blocks used: [`Knowledge`] (each officer's own target fact, and a shared
//! table of 8 that forgets), [`Outbox`] and [`route`] (the radio: delay 102,
//! cooldown 170, the three nearest free officers), [`Progress`] (does the
//! chase still get closer), [`Timers`] (ignore a stalled target),
//! [`Interval`] (report again every 255) and [`points_around`] with
//! [`Sine::Coarse`] (where to search).

use sagax_decide::memory::{
    Evict, Fact, Interval, Knowledge, Message, Metric, Outbox, PointPattern, Pos, Progress, Route,
    SOURCE_OWN, SOURCE_TOLD, Sine, Timers, points_around, route,
};
use sagax_decide::state::{StateMachine, Transition};

/// State: nothing to do.
pub const IDLE: u32 = 0;
/// State: walking to a search point.
pub const WALK: u32 = 1;
/// State: looking around at a search point.
pub const LOOK: u32 = 2;
/// State: chasing the actor.
pub const CHASE: u32 = 3;
const NAMES: [&str; 4] = ["Idle", "Walk", "Look", "Chase"];

const F_LOST: u64 = 1;
const F_STALLED: u64 = 2;
const F_ARRIVED: u64 = 4;
const F_GIVE_UP: u64 = 8;

/// Id of the actor.
pub const ACTOR: u32 = 7;
/// Kind of fact: "the actor was seen here".
const SEEN: u32 = 1;
const CRIME_TICK: u32 = 1;
const VISIBLE_UNTIL: u32 = 40;
const SPEED: i32 = 16;
const SIGHT_EVERY: u32 = 4;
const LOST_AT: u32 = 34;
const STALL_TICKS: u32 = 85;
const STALL_GAIN: i32 = 0x80;
const IGNORE_TICKS: u32 = 170;
const RADIO_DELAY: u32 = 102;
const RADIO_COOLDOWN: u32 = 170;
const REPORT_EVERY: u32 = 255;
const FORGET_TICKS: u32 = 2040;
const TABLE_SIZE: usize = 8;
/// How many free officers a call brings.
pub const HELPERS: usize = 3;
const LOOK_SENT: u32 = 170;
const LOOK_AFTER_CHASE: u32 = 51;
const LOOK_LATER: u32 = 51;
const WALK_FIRST: u32 = 1020;
const WALK_LATER: u32 = 340;
const SEARCH_TOTAL: u32 = 1020;
const LAST_STEP: usize = 3;
const ARRIVE: i32 = 0x180;
const CLOSE: i32 = 0x100;

/// What happened, for the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// An officer changed state.
    State {
        /// Tick.
        t: u32,
        /// Officer index.
        officer: u32,
        /// State left.
        from: u32,
        /// State entered.
        to: u32,
    },
    /// An officer started a call.
    RadioSent {
        /// Tick.
        t: u32,
        /// Officer index.
        officer: u32,
    },
    /// A call was delivered.
    Helpers {
        /// Tick.
        t: u32,
        /// Helpers chosen, nearest first.
        ids: [u32; HELPERS],
    },
    /// The table forgot an id.
    Forgotten {
        /// Tick.
        t: u32,
        /// Id forgotten.
        id: u32,
    },
}

struct Search {
    centre: Pos,
    step: usize,
    target: Pos,
    walked: u32,
    searched: u32,
}

struct Officer {
    pos: Pos,
    sm: StateMachine,
    /// The one target this officer knows about (capacity 1).
    known: Knowledge,
    report: Interval,
    progress: Progress,
    ignore: Timers,
    radio: Outbox,
    search: Option<Search>,
}

/// The search pattern of the reference behaviour: default numbers, coarse sine.
fn pattern() -> PointPattern {
    PointPattern {
        sine: Sine::Coarse,
        ..PointPattern::default()
    }
}

/// "The actor was seen at `pos` at `t`", by the officer itself.
fn seen(pos: Pos, t: u32) -> Fact {
    Fact {
        subject: ACTOR,
        kind: SEEN,
        pos,
        seen_at: t,
        source: SOURCE_OWN,
        ..Fact::default()
    }
}

/// What this officer last knew about the actor.
fn target(o: &Officer) -> Option<&Fact> {
    o.known.get(ACTOR, SEEN)
}

fn cheb(a: Pos, b: Pos) -> i32 {
    (a[0] - b[0]).abs().max((a[1] - b[1]).abs())
}

fn walk(from: Pos, to: Pos) -> Pos {
    let s = |a: i32, b: i32| a + (b - a).clamp(-SPEED, SPEED);
    [s(from[0], to[0]), s(from[1], to[1]), to[2]]
}

/// The actor runs along +x, then hides at `VISIBLE_UNTIL`.
fn actor_at(t: u32) -> Pos {
    [4000 + 8 * t.min(VISIBLE_UNTIL) as i32, 4000, 0]
}

fn officer(pos: Pos) -> Officer {
    let mut sm = StateMachine::new(4, IDLE).unwrap();
    sm.add_transition(Transition::new(CHASE, WALK, F_LOST))
        .unwrap();
    sm.add_transition(Transition::new(CHASE, WALK, F_STALLED))
        .unwrap();
    sm.add_transition(Transition::new(WALK, IDLE, F_GIVE_UP))
        .unwrap();
    sm.add_transition(Transition::new(WALK, LOOK, F_ARRIVED))
        .unwrap();
    Officer {
        pos,
        sm,
        known: Knowledge::new(1, Evict::Oldest).unwrap(),
        report: Interval::default(),
        progress: Progress::default(),
        ignore: Timers::new(1).unwrap(),
        // One call at a time: a second post is refused while one waits.
        radio: Outbox::new(1).unwrap(),
        search: None,
    }
}

/// Starts searching around `centre`: walk to step 0, look `look` there.
fn begin_search(o: &mut Officer, me: u32, centre: Pos, look: u32) {
    o.sm.set_timeout(LOOK, look, WALK).unwrap();
    o.search = Some(Search {
        centre,
        step: 0,
        target: points_around(me, 0, centre, &pattern()).unwrap(),
        walked: 0,
        searched: 0,
    });
}

/// Starts a call if the report is due and the radio accepts it.
fn report(o: &mut Officer, me: u32, at: Pos, t: u32, ev: &mut Vec<Event>) {
    let call = Message {
        kind: SEEN,
        subject: ACTOR,
        pos: at,
        seen_at: t,
        sender: me,
        // A radio call is lost when its sender is gone (the game calls
        // `Outbox::sender_gone`); nobody leaves in this scene.
        keep_if_sender_gone: 0,
        ..Message::default()
    };
    if o.report.due(t, REPORT_EVERY) && o.radio.post(call, t, RADIO_DELAY, RADIO_COOLDOWN).is_ok() {
        o.report.mark(t);
        ev.push(Event::RadioSent { t, officer: me });
    }
}

/// Runs the scene for ticks `1 ..= end` and returns what happened.
pub fn run_scene(end: u32) -> Vec<Event> {
    let mut ev = Vec::new();
    let mut table = Knowledge::new(TABLE_SIZE, Evict::Oldest).unwrap();
    let mut officers = [
        officer([3000, 4000, 0]),
        officer([6008, 4000, 0]),
        officer([4008, 3000, 0]),
        officer([4008, 6000, 0]),
    ];
    let mut picked = Vec::new();
    let mut calls = Vec::new();
    for t in 1..=end {
        let actor = actor_at(t);
        table.expire(t, FORGET_TICKS, |f| {
            ev.push(Event::Forgotten { t, id: f.subject });
            for o in officers.iter_mut() {
                o.known.remove_subject(f.subject);
            }
        });
        for i in 0..officers.len() {
            let me = i as u32;
            let sees = me == 0 && t <= VISIBLE_UNTIL;
            let o = &mut officers[i];
            let mut flags = 0;
            if t == CRIME_TICK && sees && !o.ignore.active(ACTOR, t) {
                // Witness: remember, report at once, chase.
                table.learn(seen(actor, t), t);
                o.known.learn(seen(actor, t), t);
                report(o, me, actor, t, &mut ev);
                o.progress.start(cheb(o.pos, actor), t);
                let c = o.sm.set_state(CHASE, t).unwrap();
                ev.push(Event::State {
                    t,
                    officer: me,
                    from: c.from,
                    to: c.to,
                });
            } else if o.sm.state() == CHASE {
                // "Lost" is the age of the own fact, checked only on sight
                // ticks; that equals a counter of missed sight ticks.
                let mut lost = false;
                if (t + me).is_multiple_of(SIGHT_EVERY) {
                    if sees {
                        o.known.learn(seen(actor, t), t);
                        table.learn(seen(actor, t), t);
                        report(o, me, actor, t, &mut ev);
                    }
                    lost = t.wrapping_sub(target(o).unwrap().seen_at) >= LOST_AT;
                }
                let last = target(o).unwrap().pos;
                let d = cheb(o.pos, last);
                if lost {
                    flags |= F_LOST;
                } else if d >= CLOSE {
                    o.progress.update(d, t, STALL_GAIN);
                    if o.progress.stalled(t, STALL_TICKS) {
                        o.ignore.set(ACTOR, t, IGNORE_TICKS);
                        flags |= F_STALLED;
                    }
                } else {
                    o.progress.start(d, t);
                }
                o.pos = walk(o.pos, last);
            } else if let Some(s) = o.search.as_mut() {
                if s.step > 0 {
                    s.searched += 1;
                }
                if o.sm.state() == WALK {
                    s.walked += 1;
                    o.pos = walk(o.pos, s.target);
                    let limit = if s.step == 0 { WALK_FIRST } else { WALK_LATER };
                    if s.walked > limit || s.searched > SEARCH_TOTAL {
                        flags |= F_GIVE_UP;
                    } else if cheb(o.pos, s.target) <= ARRIVE {
                        flags |= F_ARRIVED;
                    }
                }
            }
            if let Some(c) = o.sm.update(t, flags) {
                ev.push(Event::State {
                    t,
                    officer: me,
                    from: c.from,
                    to: c.to,
                });
                match (c.from, c.to) {
                    // Lost or stalled: search around the last seen place.
                    (CHASE, WALK) => {
                        let centre = target(o).unwrap().pos;
                        begin_search(o, me, centre, LOOK_AFTER_CHASE);
                    }
                    (LOOK, WALK) => {
                        let s = o.search.as_mut().unwrap();
                        if s.step >= LAST_STEP || s.searched >= SEARCH_TOTAL {
                            o.search = None;
                            let back = o.sm.set_state(IDLE, t).unwrap();
                            ev.push(Event::State {
                                t,
                                officer: me,
                                from: back.from,
                                to: back.to,
                            });
                        } else {
                            s.step += 1;
                            s.walked = 0;
                            s.target = points_around(me, s.step, s.centre, &pattern()).unwrap();
                            o.sm.set_timeout(LOOK, LOOK_LATER, WALK).unwrap();
                        }
                    }
                    (_, IDLE) => o.search = None,
                    _ => {}
                }
            }
            // The call goes out in the sender's own tick, to the nearest
            // officers free at that moment, measured from where the call was
            // made. The message is refreshed first, so they search where the
            // sender now thinks the actor is.
            officers[i].radio.due(t, &mut calls);
            for mut call in calls.drain(..) {
                let origin = (call.pos[0], call.pos[1]);
                if let Some(f) = target(&officers[i]) {
                    call.pos = f.pos;
                    call.seen_at = f.seen_at;
                }
                let free: Vec<(u32, i32, i32)> = officers
                    .iter()
                    .enumerate()
                    .filter(|(j, c)| *j != i && c.sm.state() == IDLE)
                    .map(|(j, c)| (j as u32, c.pos[0], c.pos[1]))
                    .collect();
                route(
                    Route::Nearest(HELPERS),
                    origin,
                    &free,
                    Metric::Euclid,
                    16,
                    &mut picked,
                );
                let mut ids = [u32::MAX; HELPERS];
                ids[..picked.len()].copy_from_slice(&picked);
                ev.push(Event::Helpers { t, ids });
                for &h in &picked {
                    let c = &mut officers[h as usize];
                    // Learning by radio does not refresh the shared table.
                    c.known.learn_message(&call, SOURCE_TOLD, t);
                    c.report.mark(t);
                    begin_search(c, h, call.pos, LOOK_SENT);
                    let ch = c.sm.set_state(WALK, t).unwrap();
                    ev.push(Event::State {
                        t,
                        officer: h,
                        from: ch.from,
                        to: ch.to,
                    });
                }
            }
        }
    }
    ev
}

fn main() {
    for e in run_scene(2100) {
        match e {
            Event::State {
                t,
                officer,
                from,
                to,
            } => println!(
                "t={t:>4}  officer {officer}: {} -> {}",
                NAMES[from as usize], NAMES[to as usize]
            ),
            Event::RadioSent { t, officer } => println!("t={t:>4}  officer {officer}: radio sent"),
            Event::Helpers { t, ids } => println!("t={t:>4}  radio delivered, helpers {ids:?}"),
            Event::Forgotten { t, id } => println!("t={t:>4}  table forgets {id}"),
        }
    }
}
