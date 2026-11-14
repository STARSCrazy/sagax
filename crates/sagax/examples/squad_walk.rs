//! A squad walks around a wall and attacks from three sides at once.
//! `cargo run -p sagax --example squad_walk`
//!
//! A 24 x 12 grid with a wall at x = 12 (rows 0 to 7, open below). The
//! leader sees an enemy behind the wall; the [`Crew`] plans a flank, sends
//! the orders by radio and gives the signal once all stand at their side.
//! Each member has its own [`Decider`]: keep the goal, or idle. A member
//! that chose its goal walks its [`Navigator`] way, one cell per `STEP`
//! ticks from its start tick.
//!
//! The same scene is in `examples/c/squad.c` and `examples/js/squad.mjs`;
//! all three print the same lines.

use sagax::decide::group::{Channel, Shape, TAG_COVER, TAG_HIGH};
use sagax::prelude::*;

/// Ticks the scene runs: `0 ..= END`.
pub const END: u32 = 120;
/// Fact kind of an enemy.
const ENEMY: u32 = 1;
/// Ticks per cell of the walk.
const STEP: u32 = 2;
const SEED: u64 = 7;
const WIDTH: i32 = 24;
const HEIGHT: i32 = 12;
/// The members: id, x, y. Member 1 leads.
const MEMBERS: [(u32, i32, i32); 4] = [(1, 2, 5), (2, 2, 7), (3, 2, 3), (4, 0, 5)];
const SKILL: u8 = 192;
/// Orders reach the leader at once, the others by radio: 2 ticks plus 1
/// per cell from the leader.
const RADIO: Channel = Channel::Radio {
    base: 2,
    per_unit: 1,
};

/// One member's own side: its decider, its way and where it is on it.
struct Walker {
    id: u32,
    decider: Decider,
    goal_option: u32,
    idle_option: u32,
    nav: Navigator,
    /// Next cell of the way to take.
    next: usize,
    /// The walk starts after this tick.
    from: u32,
    /// The member chose its goal at the last decision.
    on_goal: bool,
}

/// What the scene did.
pub struct Scene {
    /// `(tick, event)` in order.
    pub events: Vec<(u32, CrewEvent)>,
    /// Every position a member walked to.
    pub stood: Vec<Pos>,
}

/// The map: open cells and the wall at x = 12, rows 0 to 7.
pub fn scene_map() -> DenseGrid {
    let mut map = DenseGrid::new(WIDTH, HEIGHT);
    for y in 0..=7 {
        map.set(12, y, DenseGrid::BLOCKED);
    }
    map
}

fn scene_crew() -> Crew {
    let mut crew = Crew::new(CrewConfig {
        squad: SquadConfig {
            members: 4,
            memory: 8,
            areas: 4,
            inbox: 8,
            dangers: 4,
            metric: Metric::Chebyshev,
            ..SquadConfig::default()
        },
        ..CrewConfig::default()
    })
    .unwrap();
    for (id, x, y) in MEMBERS {
        let member = Member {
            id,
            pos: [x, y, 0],
            roles: 0,
            skill: SKILL,
            state: MemberState::Active,
        };
        assert!(crew.add_member(member).is_some());
    }
    let sq = crew.squad_mut();
    for (id, ..) in MEMBERS {
        if id != 1 {
            assert!(sq.set_channel(id, RADIO));
        }
    }
    assert!(sq.set_leader(1));
    sq.set_skill(SKILL);
    sq.set_target_kind(ENEMY);
    sq.add_tactic(tactics::rush());
    sq.add_tactic(tactics::formation(Shape::Line, 2));
    sq.add_tactic(tactics::flank(3, 4));
    crew.observe(
        0,
        Fact {
            subject: 10,
            kind: ENEMY,
            pos: [20, 5, 0],
            seen_at: 0,
            ..Fact::default()
        },
    );
    crew
}

fn scene_spots() -> [Spot; 4] {
    [
        Spot {
            pos: [16, 5, 0],
            tags: TAG_COVER,
        },
        Spot {
            pos: [16, 7, 0],
            tags: 0,
        },
        Spot {
            pos: [20, 9, 0],
            tags: TAG_HIGH,
        },
        Spot {
            pos: [20, 1, 0],
            tags: 0,
        },
    ]
}

fn walker(id: u32) -> Walker {
    let mut decider = Decider::new();
    let goal_option = GoalKeeper::add_goal_option(&mut decider, ONE);
    let idle_option = decider.add_option(ONE / 4);
    Walker {
        id,
        decider,
        goal_option,
        idle_option,
        nav: Navigator::new(
            Topology::Square(GridRules::preset(Preset::Octile)),
            (WIDTH * HEIGHT) as usize,
        ),
        next: 0,
        from: 0,
        on_goal: false,
    }
}

fn pos_of(crew: &Crew, id: u32) -> Pos {
    let g = crew.squad().group();
    g.get(g.index(id).unwrap()).unwrap().pos
}

/// Runs the scene for ticks `0 ..= end`.
pub fn run_scene(end: u32) -> Scene {
    let mut map = scene_map();
    let mut crew = scene_crew();
    let spots = scene_spots();
    let mut walkers: Vec<Walker> = MEMBERS.iter().map(|&(id, ..)| walker(id)).collect();
    let mut events = Vec::new();
    let mut scene = Scene {
        events: Vec::new(),
        stood: Vec::new(),
    };
    for t in 0..=end {
        // 1. Walk: one cell per STEP ticks after the start, for a member on
        // its goal.
        for w in &mut walkers {
            let d = t.wrapping_sub(w.from);
            if w.on_goal
                && d > 0
                && d < 1 << 31
                && d.is_multiple_of(STEP)
                && let Some(&cell) = w.nav.path().get(w.next)
            {
                w.next += 1;
                assert!(crew.set_pos(w.id, cell.into()));
                scene.stood.push(cell.into());
            }
        }
        // 2. The group.
        crew.tick(t, &[], &spots, SEED, &mut events).unwrap();
        for e in &events {
            if let CrewEvent::Order {
                member,
                goal,
                start_at,
            } = *e
            {
                let w = walkers.iter_mut().find(|w| w.id == member).unwrap();
                let at = Cell::from(pos_of(&crew, member));
                let to = Cell::from(goal.pos.unwrap());
                w.nav.find_path(&mut map, at, &[to], Budget::UNLIMITED);
                w.next = 1;
                w.from = start_at.unwrap_or(t);
            }
            scene.events.push((t, *e));
        }
        // 3. Each member decides: its goal or idle.
        for w in &mut walkers {
            let inputs = crew.member_inputs(w.id, t).unwrap();
            let discipline = u32::try_from(inputs.discipline).unwrap_or(0);
            crew.keeper(w.id)
                .unwrap()
                .set_current(&mut w.decider, w.goal_option, discipline);
            let candidates = [
                Candidate::new(w.goal_option, 0),
                Candidate::new(w.idle_option, 0),
            ];
            let row = inputs.as_row();
            let choice = w
                .decider
                .choose(&candidates, &[row, row], Pick::Best, 0)
                .unwrap();
            w.on_goal = choice.is_some_and(|c| c.candidate == 0);
            assert!(crew.member_chose(w.id, t, w.on_goal));
        }
    }
    scene
}

/// One line per event: `t=%3u <kind> member <id>` (`plan <id>` for a plan).
pub fn trace(events: &[(u32, CrewEvent)]) -> Vec<String> {
    events
        .iter()
        .map(|&(t, e)| match e {
            CrewEvent::Planned(id) => format!("t={t:>3} planned plan {id}"),
            CrewEvent::Activated(id) => format!("t={t:>3} activated plan {id}"),
            CrewEvent::Signal => format!("t={t:>3} signal"),
            CrewEvent::NoPlan => format!("t={t:>3} noplan"),
            CrewEvent::Order { member, .. } => format!("t={t:>3} order member {member}"),
            CrewEvent::Start { member } => format!("t={t:>3} start member {member}"),
            CrewEvent::Arrived { member } => format!("t={t:>3} arrived member {member}"),
            CrewEvent::Broken { member } => format!("t={t:>3} broken member {member}"),
        })
        .collect()
}

fn main() {
    for line in trace(&run_scene(END).events) {
        println!("{line}");
    }
}
