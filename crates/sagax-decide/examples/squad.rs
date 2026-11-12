//! A squad flank on a small square grid: four members, two enemies, an
//! elite leader. The leader sees one enemy, a member reports the other;
//! the squad thinks, sends the gather orders by radio, waits until all
//! stand at their side of the target and then attacks from three sides at
//! the same tick.
//! `cargo run -p sagax-decide --example squad`
//!
//! The scene runs at 17 ticks per second on Chebyshev cells. The members'
//! own behaviour is scripted: each walks one cell per `STEP` ticks toward
//! its order's spot and tells the squad when it reached its gather spot.
//! Orders reach the leader at once and the others by radio (2 ticks plus 1
//! per cell from the leader).

use sagax_decide::group::{
    Channel, GoalKind, Member, MemberState, Order, REPORT_ENEMY, Shape, Spot, Squad, Start, Status,
    TAG_COVER, TAG_HIGH, tactics,
};
use sagax_decide::memory::{Fact, Message, Metric, Pos};

/// Fact kind of an enemy in the leader's knowledge.
const ENEMY: u32 = 1;
/// Ticks per cell of the scripted walk.
const STEP: u32 = 2;
const RADIO: Channel = Channel::Radio {
    base: 2,
    per_unit: 1,
};
const SEED: u64 = 7;
const NAMES: [&str; 3] = ["rush", "formation", "flank"];

/// What happened, for the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    /// The leader made a plan (pending while it thinks).
    Planned {
        /// Tick.
        t: u32,
        /// Index of the tactic.
        tactic: usize,
        /// The target.
        target: Option<u32>,
    },
    /// The plan became active and its orders went out.
    Active {
        /// Tick.
        t: u32,
    },
    /// An order reached a member.
    Order {
        /// Tick.
        t: u32,
        /// Member id.
        member: u32,
        /// Where to go.
        pos: Pos,
        /// Tick the member starts.
        start_at: u32,
    },
    /// A member stands on its gather spot.
    Arrived {
        /// Tick.
        t: u32,
        /// Member id.
        member: u32,
    },
    /// The leader gave the signal.
    Signal {
        /// Tick.
        t: u32,
    },
    /// A member starts its attack.
    Attack {
        /// Tick.
        t: u32,
        /// Member id.
        member: u32,
    },
}

struct Walker {
    id: u32,
    order: Option<Order>,
    start_at: u32,
    gathered: bool,
}

/// One cell from `from` toward `to` (each axis by at most 1).
fn step_toward(from: Pos, to: Pos) -> Pos {
    let s = |a: i32, b: i32| a + (b - a).signum();
    [s(from[0], to[0]), s(from[1], to[1]), from[2]]
}

/// Runs the scene for ticks `0 ..= end` and returns what happened.
fn run_scene(end: u32) -> Vec<Event> {
    let mut ev = Vec::new();
    let mut sq = Squad::new(4, 8, 4, 8, Metric::Chebyshev).unwrap();
    let members = [(1, 0, 0), (2, 0, 2), (3, 0, -2), (4, -3, 0)];
    for (id, x, y) in members {
        sq.group_mut().add(Member {
            id,
            pos: [x, y, 0],
            roles: 0,
            skill: 192,
            state: MemberState::Active,
        });
        if id != 1 {
            sq.set_channel(id, RADIO);
        }
    }
    sq.set_leader(1);
    sq.set_skill(192);
    sq.set_target_kind(ENEMY);
    sq.add_tactic(tactics::rush());
    sq.add_tactic(tactics::formation(Shape::Line, 2));
    sq.add_tactic(tactics::flank(3, 4));
    let spots = [
        Spot {
            pos: [16, 0, 0],
            tags: TAG_COVER,
        },
        Spot {
            pos: [16, 2, 0],
            tags: 0,
        },
        Spot {
            pos: [20, 4, 0],
            tags: TAG_HIGH,
        },
        Spot {
            pos: [20, -4, 0],
            tags: 0,
        },
    ];
    // The leader sees enemy 10; member 2 reports enemy 11.
    sq.observe(
        Fact {
            subject: 10,
            kind: ENEMY,
            pos: [20, 0, 0],
            ..Fact::default()
        },
        0,
    );
    sq.report(
        Message {
            kind: REPORT_ENEMY,
            subject: 11,
            pos: [24, 10, 0],
            sender: 2,
            ..Message::default()
        },
        0,
    );

    let mut walkers: Vec<Walker> = members
        .iter()
        .map(|&(id, ..)| Walker {
            id,
            order: None,
            start_at: 0,
            gathered: false,
        })
        .collect();
    let mut out = Vec::new();
    let mut active = None;
    for t in 0..=end {
        for w in &mut walkers {
            let Some(o) = w.order else { continue };
            let i = sq.group().index(w.id).unwrap();
            let mut at = sq.group().get(i).unwrap().pos;
            let to = o.goal.pos.unwrap();
            if t > w.start_at && (t - w.start_at).is_multiple_of(STEP) {
                at = step_toward(at, to);
                sq.group_mut().set_pos(w.id, at);
            }
            if o.goal.start == Start::OnSignal && !w.gathered && at == to {
                w.gathered = true;
                sq.arrived(w.id, t);
                ev.push(Event::Arrived { t, member: w.id });
            }
        }
        match sq.check(t, &[], &spots, SEED).unwrap() {
            Status::Replan(_) => {
                let p = sq.pending_plan().unwrap();
                ev.push(Event::Planned {
                    t,
                    tactic: p.tactic,
                    target: p.target,
                });
            }
            Status::Signal => ev.push(Event::Signal { t }),
            Status::Keep | Status::NoPlan => {}
        }
        sq.orders_due(t, &mut out);
        let now_active = sq.active_plan().map(|p| p.id);
        if now_active != active {
            active = now_active;
            ev.push(Event::Active { t });
        }
        for o in out.drain(..) {
            let w = walkers.iter_mut().find(|w| w.id == o.member).unwrap();
            let after = match o.goal.start {
                Start::AfterTicks(n) => n,
                Start::Now | Start::OnSignal => 0,
            };
            w.order = Some(o);
            w.start_at = t + o.goal.start_delay + after;
            w.gathered = false;
            ev.push(Event::Order {
                t,
                member: o.member,
                pos: o.goal.pos.unwrap(),
                start_at: w.start_at,
            });
        }
        for w in &walkers {
            if let Some(o) = w.order
                && o.goal.kind == GoalKind::Attack
                && o.goal.start != Start::OnSignal
                && w.start_at == t
            {
                ev.push(Event::Attack { t, member: w.id });
            }
        }
    }
    ev
}

fn main() {
    for e in run_scene(60) {
        match e {
            Event::Planned { t, tactic, target } => match target {
                Some(id) => println!("t={t:>3}  plan {} on enemy {id} (thinking)", NAMES[tactic]),
                None => println!("t={t:>3}  plan {} (thinking)", NAMES[tactic]),
            },
            Event::Active { t } => println!("t={t:>3}  plan active, orders out"),
            Event::Order {
                t,
                member,
                pos,
                start_at,
            } => println!(
                "t={t:>3}  member {member}: order to ({}, {}), starts at {start_at}",
                pos[0], pos[1]
            ),
            Event::Arrived { t, member } => println!("t={t:>3}  member {member}: at gather spot"),
            Event::Signal { t } => println!("t={t:>3}  signal: all in place"),
            Event::Attack { t, member } => println!("t={t:>3}  member {member}: attacks"),
        }
    }
}
