//! Four guards ambush an intruder, built from the game's own pieces.
//! `cargo run -p sagax --example ambush_rules`
//!
//! A 20 x 12 map with a wall at x = 10 (rows 0 to 8, a gap below). Intruder
//! 99 walks east through the gap. The guards wait on the east side. Nothing
//! of the library changes: the game hands the squad a rules object with
//! five pieces of its own, all built on a small sight test (Bresenham over
//! the walls, game code):
//!
//! - **spot score:** a spot the intruder can see scores far worse, so the
//!   guards hide (sneaking, cover against this one enemy);
//! - **channel:** hand signs arrive one tick later, only in sight;
//! - **trigger:** the intruder steps into the kill zone, the signal goes;
//! - **report:** a guard that was seen says so (a kind of the game);
//! - **abort:** that report ends the ambush; the squad plans a rush.
//!
//! Scene A: the intruder walks into the kill zone and the guards strike
//! together. Scene B: the intruder stops at the wall, steps to the gap and sees
//! guard 3; the ambush is dropped.
//!
//! The same scenes are in `examples/c/ambush.c`; both print the same lines.

use sagax::decide::group::{
    Abort, Channel, ChannelCtx, IN_TARGET_DIST, REPORT_GAME_FIRST, ReportActions, RowCtx, SpotCtx,
    SquadRules, SquadView, TAG_HIDDEN, Trigger,
};
use sagax::prelude::*;

/// Ticks each scene runs: `0 ..= END`.
pub const END: u32 = 80;
const WIDTH: i32 = 20;
const HEIGHT: i32 = 12;
/// Column of the wall; it covers rows 0 to `WALL_TOP`.
const WALL_X: i32 = 10;
const WALL_TOP: i32 = 8;
const INTRUDER: u32 = 99;
/// Fact kind of the intruder.
const ENEMY: u32 = 1;
const SKILL: u8 = 192;
const SEED: u64 = 7;
/// The guards: id, x, y. Guard 1 leads.
const GUARDS: [(u32, i32, i32); 4] = [(1, 15, 3), (2, 14, 4), (3, 16, 4), (4, 15, 2)];
/// Game ids of the pieces in the rules object.
const HAND_SIGNS: u32 = 1;
const KILL_ZONE: u32 = 1;
const SEEN: u32 = 1;
/// Report kind of the game: "a guard was seen".
const REPORT_SEEN: u32 = REPORT_GAME_FIRST;
/// Added to the score of a spot the intruder can see.
const SEEN_PENALTY: i128 = 10_000;
/// Ticks per cell of the intruder's walk.
const STEP: u32 = 2;

/// The walls: a grid where `BLOCKED` stops sight.
pub fn scene_walls() -> DenseGrid {
    let mut map = DenseGrid::new(WIDTH, HEIGHT);
    for y in 0..=WALL_TOP {
        map.set(WALL_X, y, DenseGrid::BLOCKED);
    }
    map
}

/// Game-side sight test: Bresenham from `from` to `to` over the walls; the
/// two end cells do not block.
pub fn sees(walls: &DenseGrid, from: Pos, to: Pos) -> bool {
    let (mut x, mut y) = (from[0], from[1]);
    let (dx, dy) = ((to[0] - x).abs(), -(to[1] - y).abs());
    let (sx, sy) = ((to[0] - x).signum(), (to[1] - y).signum());
    let mut err = dx + dy;
    loop {
        if (x, y) == (to[0], to[1]) {
            return true;
        }
        if (x, y) != (from[0], from[1]) && !walls.open(x, y) {
            return false;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// The kill zone: x 11 to 14, y 9 to 11.
pub fn in_kill_zone(p: Pos) -> bool {
    (11..=14).contains(&p[0]) && (9..=11).contains(&p[1])
}

/// Where the intruder is at `t`. Both scenes: east from (2, 10), one cell
/// per `STEP` ticks. Scene A walks on. Scene B stops at (8, 10) in front of
/// the wall and steps to (9, 10) at tick 24, where the gap lets it see
/// guard 3.
pub fn intruder_at(scene_b: bool, t: u32) -> Pos {
    let walked = i32::try_from(t / STEP).unwrap();
    if !scene_b {
        return [(2 + walked).min(WIDTH - 1), 10, 0];
    }
    [if t < 24 { (2 + walked).min(8) } else { 9 }, 10, 0]
}

/// The game's pieces, as one rules object.
pub struct AmbushRules {
    walls: DenseGrid,
    /// Where the intruder is now (the game sets it each tick).
    pub intruder: Pos,
    /// A guard reported being seen.
    pub seen: bool,
}

impl SquadRules for AmbushRules {
    fn spot_score(&mut self, ctx: &SpotCtx) -> Option<i128> {
        let seen = sees(&self.walls, self.intruder, ctx.spot.pos);
        Some(ctx.built_in + if seen { SEEN_PENALTY } else { 0 })
    }

    fn channel_delay(&mut self, id: u32, ctx: &ChannelCtx) -> Option<u32> {
        (id == HAND_SIGNS && sees(&self.walls, ctx.from, ctx.to)).then_some(1)
    }

    fn trigger(&mut self, id: u32, _view: &SquadView<'_>, _now: u32) -> bool {
        id == KILL_ZONE && in_kill_zone(self.intruder)
    }

    fn report(
        &mut self,
        _view: &SquadView<'_>,
        msg: &Message,
        _now: u32,
        _actions: &mut ReportActions<'_>,
    ) {
        if msg.kind == REPORT_SEEN {
            self.seen = true;
        }
    }

    fn abort(&mut self, id: u32, _view: &SquadView<'_>, _ctx: &RowCtx<'_>) -> bool {
        id == SEEN && self.seen
    }
}

/// What a scene did.
pub struct Scene {
    /// `(tick, event)` in order.
    pub events: Vec<(u32, CrewEvent)>,
    /// The tick the "seen" report was sent (scene B).
    pub sent_at: Option<u32>,
    /// The crew at the end.
    pub crew: Crew,
}

fn scene_crew() -> Crew {
    let mut crew = Crew::new(CrewConfig {
        squad: SquadConfig {
            members: 4,
            metric: Metric::Chebyshev,
            ..SquadConfig::default()
        },
        ..CrewConfig::default()
    })
    .unwrap();
    for (id, x, y) in GUARDS {
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
    for (id, ..) in GUARDS {
        assert!(sq.set_channel(id, Channel::Custom(HAND_SIGNS)));
    }
    assert!(sq.set_leader(1));
    sq.set_skill(SKILL);
    sq.set_target_kind(ENEMY);
    let mut ambush = tactics::ambush();
    ambush.trigger = Trigger::Custom(KILL_ZONE);
    ambush.aborts.push(Abort::Custom(SEEN));
    // The shipped curve spans distances 0 to 100; on this small map it is
    // scaled to 0 to 16, or the plain rush would always score higher.
    ambush.considerations = vec![Consideration::new(
        IN_TARGET_DIST as u32,
        Curve::Linear,
        0,
        16,
    )];
    sq.add_tactic(ambush);
    sq.add_tactic(tactics::rush());
    crew
}

fn scene_spots() -> [Spot; 7] {
    [
        (13, 6),
        (13, 7),
        (16, 7),
        (17, 6),
        (12, 10),
        (13, 11),
        (12, 1),
    ]
    .map(|(x, y)| Spot {
        pos: [x, y, 0],
        tags: TAG_HIDDEN,
    })
}

/// Runs scene A (`false`) or B (`true`) for ticks `0 ..= end`.
pub fn run_scene(scene_b: bool, end: u32) -> Scene {
    let walls = scene_walls();
    let mut crew = scene_crew();
    let spots = scene_spots();
    let mut rules = AmbushRules {
        walls: scene_walls(),
        intruder: intruder_at(scene_b, 0),
        seen: false,
    };
    let guard3 = {
        let (_, x, y) = GUARDS[2];
        [x, y, 0]
    };
    let mut scene = Scene {
        events: Vec::new(),
        sent_at: None,
        crew: crew.clone(),
    };
    let mut events = Vec::new();
    for t in 0..=end {
        let at = intruder_at(scene_b, t);
        rules.intruder = at;
        crew.observe(
            t,
            Fact {
                subject: INTRUDER,
                kind: ENEMY,
                pos: at,
                seen_at: t,
                ..Fact::default()
            },
        );
        // Scene B: guard 3 is seen and tells the leader.
        if scene_b && scene.sent_at.is_none() && sees(&walls, at, guard3) {
            let msg = Message {
                kind: REPORT_SEEN,
                sender: 3,
                subject: INTRUDER,
                ..Message::default()
            };
            assert!(crew.report_with(t, msg, &mut rules));
            scene.sent_at = Some(t);
        }
        crew.tick_with(t, &[], &spots, SEED, &mut rules, &mut events)
            .unwrap();
        scene.events.extend(events.iter().map(|&e| (t, e)));
    }
    scene.crew = crew;
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

/// Both scenes, each after its header line.
pub fn trace_all() -> Vec<String> {
    let mut lines = vec!["scene A".to_string()];
    lines.extend(trace(&run_scene(false, END).events));
    lines.push("scene B".to_string());
    lines.extend(trace(&run_scene(true, END).events));
    lines
}

fn main() {
    for line in trace_all() {
        println!("{line}");
    }
}
