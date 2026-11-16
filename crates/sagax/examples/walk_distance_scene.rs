//! A wall between a squad and an enemy: who should run?
//! `cargo run -p sagax --example walk_distance_scene`
//!
//! A yard of 16 x 10 cells (square, octile costs 10 / 14, the squad
//! measures in Chebyshev cells) with a wall down x 8, open only at the
//! bottom (y 8 and 9). The leader 1 stands at (2,8); two runners (role 1):
//! guard 2 at (7,2), right behind the wall, and guard 3 at (13,8) on the
//! enemy's side. The leader sees an enemy at (12,2). The crew's one tactic
//! sends one runner to the enemy's front.
//!
//! As the crow flies guard 2 is the nearer one, so the plain crew sends it,
//! and it has to walk all the way round the wall. With a [`FlowDistance`]
//! lent through [`Walking`] the crew asks how far each one has to walk and
//! sends guard 3, who gets there much sooner. The runner walks one cell per
//! tick along a flow field to its goal.
//!
//! Lines: per guard `guard <id> at <x>,<y>: line <cells>, walk <cells>`
//! (to the enemy), then per run its name (`metric` or `walking`) and
//!
//! - `t=<tick> planned <plan>`, `t=<tick> activated <plan>`,
//!   `t=<tick> order <member> to <x>,<y>`, `t=<tick> start <member>`: the
//!   crew's events;
//! - `t=<tick> member <id> there` when the runner reaches its goal;
//!
//! and for the walking run `searches <n>`: the flow fields the provider
//! built in the whole run.
//!
//! The same scene is in `examples/c/walk_distance.c`; both print the same
//! lines.

use sagax::nav::Limits;
use sagax::nav::flow::FlowField;
use sagax::prelude::*;

/// Width of the yard.
pub const WIDTH: i32 = 16;
/// Height of the yard.
pub const HEIGHT: i32 = 10;
/// The leader and the two runners: id, x, y, roles.
pub const MEMBERS: [(u32, i32, i32, u32); 3] = [(1, 2, 8, 0), (2, 7, 2, 1), (3, 13, 8, 1)];
/// Where the enemy is.
pub const ENEMY_AT: (i32, i32) = (12, 2);
/// Fact kind of an enemy.
pub const ENEMY: u32 = 1;
/// The ticks a run lasts at most.
pub const TICKS: u32 = 40;
/// Seed of the crew's ticks.
pub const SEED: u64 = 7;

/// Whether the yard's wall is at `x`, `y`.
pub fn wall(x: i32, y: i32) -> bool {
    x == 8 && y <= 7
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

/// Square grid, octile costs.
pub fn topology() -> Topology {
    Topology::Square(GridRules::preset(Preset::Octile))
}

/// The crew's one tactic: one runner (role 1) to the enemy's front.
pub fn intercept() -> Tactic {
    use sagax::decide::group::{GoalKind, Place, Side, Slot, Start, Trigger};
    Tactic {
        slots: vec![Slot {
            roles: 1,
            min: 1,
            max: 1,
            gather: None,
            place: Place::Side(Side::Front),
            kind: GoalKind::Attack,
            start: Start::Now,
            want_tags: 0,
            dist: 1,
            gather_dist: 2,
        }],
        considerations: Vec::new(),
        weight: ONE / 4,
        aborts: Vec::new(),
        trigger: Trigger::None,
        min_skill: 0,
        needs_target: true,
        abort_if_target_lost: true,
    }
}

/// The crew of the scene, the enemy seen at tick 0.
pub fn crew() -> Crew {
    let mut crew = Crew::new(CrewConfig {
        squad: SquadConfig {
            members: 4,
            memory: 4,
            areas: 1,
            inbox: 4,
            metric: Metric::Chebyshev,
            ..SquadConfig::default()
        },
        ..CrewConfig::default()
    })
    .expect("capacities");
    for (id, x, y, roles) in MEMBERS {
        let m = Member {
            id,
            pos: [x, y, 0],
            roles,
            skill: 255,
            state: MemberState::Active,
        };
        crew.add_member(m).expect("room");
    }
    let sq = crew.squad_mut();
    assert!(sq.set_leader(1));
    sq.set_skill(255);
    sq.set_target_kind(ENEMY);
    sq.add_tactic(intercept());
    crew.observe(
        0,
        Fact {
            subject: 10,
            kind: ENEMY,
            pos: [ENEMY_AT.0, ENEMY_AT.1, 0],
            seen_at: 0,
            ..Fact::default()
        },
    );
    crew
}

/// What a run did, for the checks.
pub struct Run {
    /// The printed lines (name first).
    pub lines: Vec<String>,
    /// The member that got the order.
    pub runner: Option<u32>,
    /// The tick it reached its goal.
    pub there: Option<u32>,
}

/// One run; `walking`: with a [`FlowDistance`] (its search count printed
/// at the end).
pub fn run(walking: bool) -> Run {
    let mut map = yard();
    let mut fd = FlowDistance::new(topology(), FlowDistance::DEFAULT_SLOTS, 1000);
    let mut crew = crew();
    let mut events = Vec::new();
    let mut out = Run {
        lines: vec![if walking { "walking" } else { "metric" }.to_string()],
        runner: None,
        there: None,
    };
    // The runner's way: a flow field to its goal, once it starts.
    let mut field: FlowField<Cell> = FlowField::default();
    let mut walker: Option<(u32, Cell)> = None;
    for t in 0..=TICKS {
        if walking {
            let mut walk = fd.on(&mut map);
            crew.tick_with(t, &[], &[], SEED, &mut Walking::new(&mut walk), &mut events)
                .expect("no rules needed");
        } else {
            crew.tick(t, &[], &[], SEED, &mut events)
                .expect("no rules needed");
        }
        for e in &events {
            let line = match *e {
                CrewEvent::Planned(id) => format!("planned {id}"),
                CrewEvent::Activated(id) => format!("activated {id}"),
                CrewEvent::Order { member, goal, .. } => {
                    let p = goal.pos.expect("a place");
                    let goal_cell = Cell::from(p);
                    let mut graph = topology().graph(&mut map, None);
                    field.build(&mut graph, [goal_cell], Limits::default());
                    out.runner = Some(member);
                    walker = None;
                    format!("order {member} to {},{}", p[0], p[1])
                }
                CrewEvent::Start { member } => {
                    let g = crew.squad().group();
                    let m = g.get(g.index(member).expect("a member")).expect("a member");
                    walker = Some((member, Cell::from(m.pos)));
                    format!("start {member}")
                }
                _ => continue,
            };
            out.lines.push(format!("t={t:2} {line}"));
        }
        // The runner walks one cell along its field.
        if let Some((id, at)) = walker {
            let mut graph = topology().graph(&mut map, None);
            match field.next(&mut graph, at) {
                Some(next) => {
                    assert!(crew.set_pos(id, [next.x, next.y, next.level]));
                    walker = Some((id, next));
                    if field.cost(next) == Some(0) {
                        out.lines.push(format!("t={t:2} member {id} there"));
                        out.there = Some(t);
                        break;
                    }
                }
                None => break,
            }
        }
    }
    if walking {
        out.lines.push(format!("searches {}", fd.searches()));
    }
    out
}

/// The guards' distances, then both runs.
pub fn run_scene() -> Vec<String> {
    let mut map = yard();
    let mut fd = FlowDistance::new(topology(), FlowDistance::DEFAULT_SLOTS, 1000);
    let enemy = [ENEMY_AT.0, ENEMY_AT.1, 0];
    let mut lines = Vec::new();
    for (id, x, y, roles) in MEMBERS {
        if roles == 0 {
            continue;
        }
        let line = (x - ENEMY_AT.0).abs().max((y - ENEMY_AT.1).abs());
        let walk = fd
            .distance(&mut map, [x, y, 0], enemy)
            .map_or("none".to_string(), |d| d.to_string());
        lines.push(format!("guard {id} at {x},{y}: line {line}, walk {walk}"));
    }
    for walking in [false, true] {
        lines.extend(run(walking).lines);
    }
    lines
}

fn main() {
    for line in run_scene() {
        println!("{line}");
    }
}
