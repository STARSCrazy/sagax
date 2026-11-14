//! Two guards on one patrol meet an attacker and a theft, on a mission each.
//! `cargo run -p sagax --example mission_guard`
//!
//! A 16 x 8 map without walls. Both guards have the same looping patrol
//! (go to (4,3), wait 5 ticks, go to (12,3)) and the motive "guard the
//! item". Guard A is [`Leeway::STRICT`] (leash 3, never replans), guard B a
//! flexible one (leash 12, searches 6 ticks when it loses its subject,
//! replans). The game moves a character one cell per tick toward the target
//! of its action (Chebyshev); nothing else is done for the mission.
//!
//! - **Ticks 0 to 29:** nothing happens. The trace is the plain script.
//! - **Phase 2 (from tick 30):** attacker 50 appears and runs east, two
//!   cells per tick, and leaves the map. A guard "sees" it within 5 cells:
//!   only then the game puts the attacker's fact into that guard's own
//!   `Knowledge` and calls `detour(Engage)`. A follows three cells and
//!   walks back; B chases, loses sight, searches, returns and resumes the
//!   step it left.
//! - **Phase 3 (tick 70):** only B is told that the item (20) is stolen
//!   and where the thief (30) is. The patrol breaks on that fact; B
//!   replans to its playbook plan "attack the thief". A is never told and
//!   keeps patrolling.
//!
//! One line per tick and guard:
//! `t=<tick> <A|B> step=<index or -> action=<action> events=<list or ->`.
//! `step` is the running plan step (`-` during a detour and the walk back).
//!
//! | Action | Printed |
//! |--------|---------|
//! | `None` / `Wait` / `Wander` | `none` / `wait` / `wander` |
//! | `GoTo` / `Hold` / `Search` / `Flee` at x,y | `go x,y` / `hold x,y` / `search x,y` / `flee x,y` |
//! | `Follow` / `Attack` subject s at x,y | `follow s@x,y` / `attack s@x,y` |
//! | `Custom` kind k | `custom k` |
//!
//! | Event | Printed |
//! |-------|---------|
//! | `StepDone(i)` | `done:i` |
//! | `Looped` / `PlanDone` | `looped` / `plandone` |
//! | `Broken(Fail)` / `Broken(Impossible)` / `Broken(PlanEnded)` | `broken:fail` / `broken:impossible` / `broken:ended` |
//! | `Replanned { option }` | `replanned:<option>` |
//! | `FellBack` | `fellback` |
//! | `DetourStarted` | `detour` |
//! | `DetourEnded(why)` | `detour_end:<defeated, forgotten, lost, leash, safe, arrived or ended>` |
//! | `Returned` | `returned` |
//! | `Signal(n)` | `signal:n` |
//! | `Call { channel, kind, .. }` | `call:channel,kind` |
//! | `WantsSquad { tactic, .. }` | `squad:tactic` |
//!
//! Events of one tick are joined with `,`; no event prints `-`.
//!
//! The same scene is in `examples/c/mission.c`; both print the same lines.

use sagax::prelude::*;

/// Ticks the scene runs: `0 .. END`.
pub const END: u32 = 90;
/// First tick of phase 2: the attacker appears.
pub const ATTACKER_FROM: u32 = 30;
/// Tick at which guard B is told about the theft.
pub const THEFT_AT: u32 = 70;
/// Subject id of the attacker.
pub const ATTACKER: u32 = 50;
/// Subject id of the item that gets stolen.
const ITEM: u32 = 20;
/// Subject id of the thief.
pub const THIEF: u32 = 30;
/// Fact kinds: where a subject is (the mission's `subject_kind`), and
/// "the item is stolen" (data bit 1).
const SEEN: u32 = 0;
const STOLEN_KIND: u32 = 1;
/// A guard sees a character within this many cells.
const SIGHT: i32 = 5;
/// The cells the patrol goes to.
pub const WEST: Pos = [4, 3, 0];
/// The other end of the patrol.
pub const EAST: Pos = [12, 3, 0];
/// Where the thief is said to be.
pub const THIEF_AT: Pos = [13, 6, 0];
/// The attacker at tick `ATTACKER_FROM + i` is at `ATTACKER_START + i *
/// ATTACKER_SPEED` cells east, until it leaves the 16 x 8 map.
const ATTACKER_START: Pos = [7, 3, 0];
const ATTACKER_SPEED: i32 = 2;
const MAP_WIDTH: i32 = 16;

/// What a guard of the scene holds.
struct Guard {
    name: char,
    mission: Mission,
    knowledge: Knowledge,
    pos: Pos,
}

/// One line of the trace, structured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The tick.
    pub tick: u32,
    /// `'A'` or `'B'`.
    pub guard: char,
    /// Where the guard stood when the mission was asked.
    pub pos: Pos,
    /// The running plan step after the tick.
    pub step: Option<usize>,
    /// The action the mission gave.
    pub action: Action,
    /// The events of the tick.
    pub events: Vec<Event>,
}

/// The patrol: go, wait, go, again; it breaks when the item is stolen.
pub fn patrol() -> Plan {
    Plan::looping(&[Step::go_to(WEST), Step::wait(5), Step::go_to(EAST)]).fails_when(Fail::Fact {
        subject: ITEM,
        kind: STOLEN_KIND,
        mask: 1,
        value: 1,
    })
}

fn guard(name: char, leeway: Leeway, pos: Pos) -> Guard {
    let config = MissionConfig {
        id: u32::from(name),
        subject_kind: SEEN,
        lost_after: 3,
        // Search points stay on the 16 x 8 map.
        search: PointPattern {
            min: (0, 0),
            max: (MAP_WIDTH - 1, 7),
            ..MissionConfig::default().search
        },
        ..MissionConfig::default()
    };
    let mut mission = Mission::new(config, Motive::guard_place(WEST)).unwrap();
    mission.set_leeway(leeway);
    mission.set_plan(&patrol()).unwrap();
    mission
        .add_playbook(&Plan::once(&[Step::attack(THIEF)]), ONE, false)
        .unwrap();
    Guard {
        name,
        mission,
        knowledge: Knowledge::new(8, Evict::Oldest).unwrap(),
        pos,
    }
}

/// B: leash 12, searches 6 ticks around the last known place, replans.
fn flexible() -> Leeway {
    Leeway {
        leash: Leash {
            distance: Some(12),
            ticks: None,
            on_lost: OnLost::Search {
                ticks: 6,
                radius: 3,
            },
        },
        ..Leeway::FLEXIBLE
    }
}

/// Where the attacker is at `tick`; `None` before it appears and after it
/// has left the map.
pub fn attacker_at(tick: u32) -> Option<Pos> {
    let i = tick.checked_sub(ATTACKER_FROM)? as i32;
    let x = ATTACKER_START[0] + ATTACKER_SPEED * i;
    (x < MAP_WIDTH).then_some([x, ATTACKER_START[1], 0])
}

/// Chebyshev distance on the map.
pub fn chebyshev(a: Pos, b: Pos) -> i32 {
    (a[0] - b[0]).abs().max((a[1] - b[1]).abs())
}

/// The cell an action walks to, if it walks.
fn target(action: Action) -> Option<Pos> {
    match action {
        Action::GoTo(p) | Action::Hold(p) | Action::Search(p) | Action::Flee(p) => Some(p),
        Action::Follow { pos, .. } | Action::Attack { pos, .. } => Some(pos),
        Action::Custom { pos, .. } => pos,
        Action::None | Action::Wait | Action::Wander => None,
    }
}

/// One cell from `pos` toward the action's target.
fn step_toward(pos: Pos, action: Action) -> Pos {
    match target(action) {
        Some(t) => [
            pos[0] + (t[0] - pos[0]).signum(),
            pos[1] + (t[1] - pos[1]).signum(),
            pos[2],
        ],
        None => pos,
    }
}

/// Runs the whole scene: `END` ticks, guard A then B each tick.
pub fn run_scene() -> Vec<Row> {
    let mut guards = [
        guard('A', Leeway::STRICT, [3, 2, 0]),
        guard('B', flexible(), [3, 4, 0]),
    ];
    let mut rows = Vec::new();
    for tick in 0..END {
        for g in &mut guards {
            // What the guard sees goes into its own knowledge, and a
            // guard that sees the attacker goes for it.
            if let Some(at) = attacker_at(tick)
                && chebyshev(g.pos, at) <= SIGHT
            {
                g.knowledge.learn(
                    tick,
                    Fact {
                        subject: ATTACKER,
                        kind: SEEN,
                        pos: at,
                        seen_at: tick,
                        data: 1,
                        ..Fact::default()
                    },
                );
                if !g.mission.in_detour() && !g.mission.returning() {
                    g.mission
                        .detour(tick, Detour::Engage(ATTACKER), &g.knowledge);
                }
            }
            // Only B is told about the theft.
            if tick == THEFT_AT && g.name == 'B' {
                g.knowledge.learn(
                    tick,
                    Fact {
                        subject: ITEM,
                        kind: STOLEN_KIND,
                        data: 1,
                        ..Fact::default()
                    },
                );
                g.knowledge.learn(
                    tick,
                    Fact {
                        subject: THIEF,
                        kind: SEEN,
                        pos: THIEF_AT,
                        seen_at: tick,
                        data: 1,
                        ..Fact::default()
                    },
                );
            }
            let pos = g.pos;
            let out = g.mission.tick(tick, pos, &g.knowledge, 0);
            let action = out.action;
            let events = out.events().to_vec();
            rows.push(Row {
                tick,
                guard: g.name,
                pos,
                step: g.mission.step_index(),
                action,
                events,
            });
            g.pos = step_toward(pos, action);
        }
    }
    rows
}

fn action_text(action: Action) -> String {
    match action {
        Action::None => "none".to_string(),
        Action::GoTo(p) => format!("go {},{}", p[0], p[1]),
        Action::Wait => "wait".to_string(),
        Action::Follow { subject, pos } => format!("follow {subject}@{},{}", pos[0], pos[1]),
        Action::Hold(p) => format!("hold {},{}", p[0], p[1]),
        Action::Attack { subject, pos } => format!("attack {subject}@{},{}", pos[0], pos[1]),
        Action::Search(p) => format!("search {},{}", p[0], p[1]),
        Action::Wander => "wander".to_string(),
        Action::Flee(p) => format!("flee {},{}", p[0], p[1]),
        Action::Custom { kind, .. } => format!("custom {kind}"),
    }
}

fn event_text(event: &Event) -> String {
    match event {
        Event::StepDone(i) => format!("done:{i}"),
        Event::Looped => "looped".to_string(),
        Event::PlanDone => "plandone".to_string(),
        Event::Broken(BreakReason::Fail(_)) => "broken:fail".to_string(),
        Event::Broken(BreakReason::Impossible) => "broken:impossible".to_string(),
        Event::Broken(BreakReason::PlanEnded) => "broken:ended".to_string(),
        Event::Replanned { option } => format!("replanned:{option}"),
        Event::FellBack => "fellback".to_string(),
        Event::DetourStarted => "detour".to_string(),
        Event::DetourEnded(why) => format!(
            "detour_end:{}",
            match why {
                DetourEnd::Defeated => "defeated",
                DetourEnd::Forgotten => "forgotten",
                DetourEnd::Lost => "lost",
                DetourEnd::Leash => "leash",
                DetourEnd::Safe => "safe",
                DetourEnd::Arrived => "arrived",
                DetourEnd::Ended => "ended",
            }
        ),
        Event::Returned => "returned".to_string(),
        Event::Signal(n) => format!("signal:{n}"),
        Event::Call { channel, kind, .. } => format!("call:{channel},{kind}"),
        Event::WantsSquad { tactic, .. } => format!("squad:{tactic}"),
    }
}

/// The printed line of a row.
pub fn line(row: &Row) -> String {
    let step = row.step.map_or("-".to_string(), |i| i.to_string());
    let events = if row.events.is_empty() {
        "-".to_string()
    } else {
        row.events
            .iter()
            .map(event_text)
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "t={} {} step={step} action={} events={events}",
        row.tick,
        row.guard,
        action_text(row.action)
    )
}

/// Every line the example prints.
pub fn trace_all() -> Vec<String> {
    run_scene().iter().map(line).collect()
}

fn main() {
    for line in trace_all() {
        println!("{line}");
    }
}
