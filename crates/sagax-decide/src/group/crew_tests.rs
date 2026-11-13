use super::*;
use crate::memory::{Fact, Message, Metric, Pos};
use sagax_core::HexLayout;

/// Fact kind of an enemy.
const ENEMY: u32 = 1;
/// Ticks per cell of the scripted walk.
const STEP: u32 = 2;
const RADIO: Channel = Channel::Radio {
    base: 2,
    per_unit: 1,
};
const SEED: u64 = 7;
/// The members of the flank scene (`examples/squad.rs`): id, x, y.
const MEMBERS: [(u32, i32, i32); 4] = [(1, 0, 0), (2, 0, 2), (3, 0, -2), (4, -3, 0)];
const SKILL: u8 = 192;

fn flank_spots() -> [Spot; 4] {
    [
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
    ]
}

fn scene_config(inbox: usize) -> SquadConfig {
    SquadConfig {
        members: 4,
        memory: 8,
        areas: 4,
        inbox,
        metric: Metric::Chebyshev,
        ..SquadConfig::default()
    }
}

fn scene_member(id: u32, x: i32, y: i32) -> Member {
    Member {
        id,
        pos: [x, y, 0],
        roles: 0,
        skill: SKILL,
        state: MemberState::Active,
    }
}

/// Everything of the flank scene after the members are in: channels,
/// leader, skill, tactics, the leader's sighting and member 2's report.
fn setup_scene(sq: &mut Squad, t0: u32) {
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
    let told = Fact {
        subject: 11,
        kind: ENEMY,
        pos: [24, 10, 0],
        seen_at: t0,
        ..Fact::default()
    };
    assert!(sq.report(Message::enemy(2, told), t0));
}

fn leader_sees(t0: u32) -> Fact {
    Fact {
        subject: 10,
        kind: ENEMY,
        pos: [20, 0, 0],
        seen_at: t0,
        ..Fact::default()
    }
}

fn flank_crew(inbox: usize, t0: u32) -> Crew {
    let mut crew = Crew::new(CrewConfig {
        squad: scene_config(inbox),
        ..CrewConfig::default()
    })
    .unwrap();
    for (id, x, y) in MEMBERS {
        assert!(crew.add_member(scene_member(id, x, y)).is_some());
    }
    setup_scene(crew.squad_mut(), t0);
    crew.observe(t0, leader_sees(t0));
    crew
}

/// One cell from `from` toward `to` (each axis by at most 1).
fn step_toward(from: Pos, to: Pos) -> Pos {
    let s = |a: i32, b: i32| a + (b - a).signum();
    [s(from[0], to[0]), s(from[1], to[1]), from[2]]
}

/// The scripted walk of one member: where to and from which tick.
#[derive(Clone, Copy, Default)]
struct Walk {
    to: Option<Pos>,
    from: u32,
}

impl Walk {
    /// Notes an order received at `now`.
    fn order(&mut self, now: u32, goal: &Goal, start_at: Option<u32>) {
        self.to = goal.pos;
        self.from = start_at.unwrap_or(now);
    }

    /// The next position at `now`: one cell per `STEP` ticks after `from`.
    fn next(&self, now: u32, at: Pos) -> Option<Pos> {
        let to = self.to?;
        let d = now.wrapping_sub(self.from);
        (d > 0 && d < 1 << 31 && d.is_multiple_of(STEP) && at != to).then(|| step_toward(at, to))
    }
}

fn walk_index(id: u32) -> usize {
    MEMBERS.iter().position(|m| m.0 == id).unwrap()
}

fn note_orders(walks: &mut [Walk], now: u32, events: &[CrewEvent]) {
    for e in events {
        if let CrewEvent::Order {
            member,
            goal,
            start_at,
        } = e
        {
            walks[walk_index(*member)].order(now, goal, *start_at);
        }
    }
}

/// Runs the flank scene with a `Crew` for ticks `t0 ..= t0 + len`; returns
/// `(tick, event)` in order.
fn run_crew(crew: &mut Crew, t0: u32, len: u32) -> Vec<(u32, CrewEvent)> {
    let spots = flank_spots();
    let mut walks = [Walk::default(); 4];
    let mut all = Vec::new();
    let mut events = Vec::new();
    for k in 0..=len {
        let t = t0.wrapping_add(k);
        for (i, (id, ..)) in MEMBERS.iter().enumerate() {
            let g = crew.squad().group();
            let at = g.get(g.index(*id).unwrap()).unwrap().pos;
            if let Some(p) = walks[i].next(t, at) {
                assert!(crew.set_pos(*id, p));
            }
        }
        crew.tick(t, &[], &spots, SEED, &mut events).unwrap();
        note_orders(&mut walks, t, &events);
        all.extend(events.iter().map(|e| (t, *e)));
    }
    all
}

/// Per member, kept by hand: the keeper and what was already told.
#[derive(Clone, Copy, Default)]
struct HandMember {
    keeper: GoalKeeper,
    start_at: Option<u32>,
    started: bool,
    arrived: bool,
    broken: bool,
}

/// The flank scene with the blocks by hand, in the crew's tick order.
fn run_by_hand(len: u32) -> Vec<(u32, CrewEvent)> {
    let mut sq = Squad::new(scene_config(8)).unwrap();
    for (id, x, y) in MEMBERS {
        sq.group_mut().add(scene_member(id, x, y));
    }
    setup_scene(&mut sq, 0);
    sq.observe(0, leader_sees(0));
    let spots = flank_spots();
    let mut walks = [Walk::default(); 4];
    let mut hand = [HandMember::default(); 4];
    let mut orders = Vec::new();
    let mut all = Vec::new();
    let mut events = Vec::new();
    for t in 0..=len {
        for (i, (id, ..)) in MEMBERS.iter().enumerate() {
            let g = sq.group();
            let at = g.get(g.index(*id).unwrap()).unwrap().pos;
            if let Some(p) = walks[i].next(t, at) {
                sq.group_mut().set_pos(*id, p);
            }
        }
        events.clear();
        match sq.check(t, &[], &spots, SEED).unwrap() {
            Status::Replan(id) => events.push(CrewEvent::Planned(id)),
            Status::Signal => events.push(CrewEvent::Signal),
            Status::NoPlan => events.push(CrewEvent::NoPlan),
            Status::Keep => {}
        }
        if let Some(id) = sq.take_activated() {
            events.push(CrewEvent::Activated(id));
        }
        sq.orders_due(t, &mut orders);
        for o in orders.drain(..) {
            let h = &mut hand[walk_index(o.member)];
            h.keeper.set(t, o.goal);
            h.start_at = o.goal.start_tick(t);
            h.started = false;
            h.arrived = false;
            h.broken = false;
            events.push(CrewEvent::Order {
                member: o.member,
                goal: o.goal,
                start_at: h.start_at,
            });
        }
        for (i, (id, ..)) in MEMBERS.iter().enumerate() {
            let h = &mut hand[i];
            if let Some(s) = h.start_at
                && !h.started
                && t.wrapping_sub(s) < 1 << 31
            {
                h.started = true;
                events.push(CrewEvent::Start { member: *id });
            }
        }
        for (i, (id, ..)) in MEMBERS.iter().enumerate() {
            let h = &mut hand[i];
            let g = sq.group();
            let at = g.get(g.index(*id).unwrap()).unwrap().pos;
            let Some(goal) = h.keeper.goal().copied() else {
                continue;
            };
            if h.arrived || !goal.awaits_arrival() || goal.pos != Some(at) {
                continue;
            }
            match sq.report_arrives_at(*id, t) {
                Some(when) => {
                    if sq.report(Message::arrived(*id, goal.plan), when) {
                        h.arrived = true;
                        events.push(CrewEvent::Arrived { member: *id });
                    }
                }
                None => {
                    h.arrived = true;
                    events.push(CrewEvent::Arrived { member: *id });
                }
            }
        }
        // Nobody postpones in this scene: no keeper breaks.
        for h in &hand {
            assert!(
                !h.keeper
                    .broken(t, sq.skill_table().give_up_ticks_for(SKILL))
            );
        }
        note_orders(&mut walks, t, &events);
        all.extend(events.iter().map(|e| (t, *e)));
    }
    all
}

fn ticks_of(run: &[(u32, CrewEvent)], want: impl Fn(&CrewEvent) -> bool) -> Vec<u32> {
    run.iter()
        .filter(|(_, e)| want(e))
        .map(|(t, _)| *t)
        .collect()
}

fn orders_of(run: &[(u32, CrewEvent)]) -> Vec<(u32, u32, Pos, Option<u32>)> {
    run.iter()
        .filter_map(|(t, e)| match e {
            CrewEvent::Order {
                member,
                goal,
                start_at,
            } => Some((*t, *member, goal.pos.unwrap(), *start_at)),
            _ => None,
        })
        .collect()
}

#[test]
fn crew_flank_scene_matches_blocks_by_hand() {
    let mut crew = flank_crew(8, 0);
    let by_crew = run_crew(&mut crew, 0, 80);
    let by_hand = run_by_hand(80);
    assert_eq!(by_crew, by_hand);

    // Pinned from `examples/squad.rs` (before this round): plan at 0,
    // active at 5, the gather orders at 5, 9, 9, 10, the members on their
    // gather spots at 37, 48, 49, 49.
    assert_eq!(
        ticks_of(&by_crew, |e| matches!(e, CrewEvent::Planned(_))),
        [0]
    );
    assert_eq!(
        ticks_of(&by_crew, |e| matches!(e, CrewEvent::Activated(_))),
        [5]
    );
    let orders = orders_of(&by_crew);
    assert_eq!(
        orders[..4],
        [
            (5, 1, [16, 0, 0], Some(5)),
            (9, 2, [20, 4, 0], Some(9)),
            (9, 3, [20, -4, 0], Some(9)),
            (10, 4, [16, 2, 0], Some(10)),
        ]
    );
    let arrived: Vec<(u32, u32)> = by_crew
        .iter()
        .filter_map(|(t, e)| match e {
            CrewEvent::Arrived { member } => Some((*t, *member)),
            _ => None,
        })
        .collect();
    assert_eq!(arrived, [(37, 1), (48, 4), (49, 2), (49, 3)]);
    // The example tells the squad with the delay-free `Squad::arrived`
    // and signals at 49. The crew reports honestly over the radio: the
    // last reports (members 2 and 3, 4 cells off, 2 + 4 ticks) reach the
    // leader at 55, so the signal comes at 55. The second orders keep the
    // example's spacing (49, 53, 55, 55 there): sent at 55, member 4 two
    // cells off (4 ticks), members 2 and 3 four cells off (6 ticks); all
    // attack together at 61 (55 + 6).
    assert_eq!(ticks_of(&by_crew, |e| matches!(e, CrewEvent::Signal)), [55]);
    assert_eq!(
        orders[4..],
        [
            (55, 1, [16, 0, 0], Some(61)),
            (59, 4, [16, 2, 0], Some(61)),
            (61, 2, [20, 4, 0], Some(61)),
            (61, 3, [20, -4, 0], Some(61)),
        ]
    );
    let attacks: Vec<u32> = by_crew
        .iter()
        .filter(|(t, e)| matches!(e, CrewEvent::Start { .. }) && *t > 55)
        .map(|(t, _)| *t)
        .collect();
    // Three sides; the front slot holds two members (1 and 4).
    assert_eq!(attacks, [61, 61, 61, 61]);
    assert_eq!(orders.len(), 8);
}

fn rank(e: &CrewEvent) -> u8 {
    match e {
        CrewEvent::Planned(_) | CrewEvent::Signal | CrewEvent::NoPlan => 0,
        CrewEvent::Activated(_) => 1,
        CrewEvent::Order { .. } => 2,
        CrewEvent::Start { .. } => 3,
        CrewEvent::Arrived { .. } => 4,
        CrewEvent::Broken { .. } => 5,
    }
}

#[test]
fn crew_events_keep_the_tick_order() {
    let mut crew = flank_crew(8, 0);
    let spots = flank_spots();
    let mut events = Vec::new();
    let mut seen = [false; 6];
    let mut walks = [Walk::default(); 4];
    for t in 0..=160 {
        for (i, (id, ..)) in MEMBERS.iter().enumerate() {
            let g = crew.squad().group();
            let at = g.get(g.index(*id).unwrap()).unwrap().pos;
            if let Some(p) = walks[i].next(t, at) {
                assert!(crew.set_pos(*id, p));
            }
        }
        // Member 3 puts its goal off from tick 20; its second order (61)
        // resets the clock, so it breaks 68 ticks after that.
        if t >= 20 {
            assert!(crew.member_chose(3, t, false));
        }
        crew.tick(t, &[], &spots, SEED, &mut events).unwrap();
        note_orders(&mut walks, t, &events);
        let ranks: Vec<u8> = events.iter().map(rank).collect();
        assert!(
            ranks.windows(2).all(|w| w[0] <= w[1]),
            "tick {t}: {events:?}"
        );
        for r in ranks {
            seen[usize::from(r)] = true;
        }
        if t == 5 {
            assert!(matches!(
                events[..],
                [
                    CrewEvent::Activated(_),
                    CrewEvent::Order { member: 1, .. },
                    CrewEvent::Start { member: 1 }
                ]
            ));
        }
    }
    assert_eq!(seen, [true; 6], "every kind of event came up");
}

#[test]
fn crew_tick_clears_events() {
    let mut crew = crew_with_orders(0);
    let mut events = vec![CrewEvent::NoPlan; 3];
    crew.tick(11, &[], &flank_spots(), SEED, &mut events)
        .unwrap();
    assert!(events.is_empty());
}

#[test]
fn crew_arrival_needs_a_channel() {
    let mut crew = flank_crew(8, 0);
    let spots = flank_spots();
    let mut walks = [Walk::default(); 4];
    let mut events = Vec::new();
    let mut all = Vec::new();
    for t in 0..=200 {
        for (i, (id, ..)) in MEMBERS.iter().enumerate() {
            let g = crew.squad().group();
            let at = g.get(g.index(*id).unwrap()).unwrap().pos;
            if let Some(p) = walks[i].next(t, at) {
                assert!(crew.set_pos(*id, p));
            }
        }
        crew.tick(t, &[], &spots, SEED, &mut events).unwrap();
        note_orders(&mut walks, t, &events);
        // Member 4 has its order (tick 10), then loses its radio.
        if t == 10 {
            assert!(crew.squad_mut().set_channel(4, Channel::None));
        }
        all.extend(events.iter().map(|e| (t, *e)));
    }
    assert_eq!(
        ticks_of(&all, |e| matches!(e, CrewEvent::Arrived { member: 4 })),
        [48],
        "the crew sees it arrive"
    );
    // The squad never counts it: the flank waits its full time.
    let active = ticks_of(&all, |e| matches!(e, CrewEvent::Activated(_)))[0];
    assert_eq!(
        ticks_of(&all, |e| matches!(e, CrewEvent::Signal)),
        [active + tactics::FLANK_MAX_WAIT]
    );
}

#[test]
fn crew_arrival_waits_for_room_in_the_inbox() {
    let mut crew = flank_crew(1, 0);
    let run = run_crew(&mut crew, 0, 90);
    let arrived: Vec<(u32, u32)> = run
        .iter()
        .filter_map(|(t, e)| match e {
            CrewEvent::Arrived { member } => Some((*t, *member)),
            _ => None,
        })
        .collect();
    // One report at a time: member 4's (sent 48, arrives 52) holds the
    // inbox, so members 2 and 3 (on their spots at 49) wait; member 2's
    // goes out at 52 (arrives 58), member 3's at 58 (arrives 64). No event
    // while a report is refused.
    assert_eq!(arrived, [(37, 1), (48, 4), (52, 2), (58, 3)]);
    assert_eq!(ticks_of(&run, |e| matches!(e, CrewEvent::Signal)), [64]);
}

/// A crew with the flank scene, run until every member has its gather
/// order (tick 10), nobody walking.
fn crew_with_orders(arrive_radius: u32) -> Crew {
    let mut crew = Crew::new(CrewConfig {
        squad: scene_config(8),
        arrive_radius,
        ..CrewConfig::default()
    })
    .unwrap();
    for (id, x, y) in MEMBERS {
        assert!(crew.add_member(scene_member(id, x, y)).is_some());
    }
    setup_scene(crew.squad_mut(), 0);
    crew.observe(0, leader_sees(0));
    let mut events = Vec::new();
    for t in 0..=10 {
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
    }
    crew
}

#[test]
fn crew_arrive_radius() {
    for (radius, want) in [(0, false), (1, true)] {
        let mut crew = crew_with_orders(radius);
        assert_eq!(crew.goal(1).unwrap().pos, Some([16, 0, 0]));
        assert!(crew.set_pos(1, [15, 1, 0]));
        let mut events = Vec::new();
        let mut arrived = false;
        for t in 11..=30 {
            crew.tick(t, &[], &flank_spots(), SEED, &mut events)
                .unwrap();
            arrived |= events.contains(&CrewEvent::Arrived { member: 1 });
        }
        assert_eq!(arrived, want, "radius {radius}");
    }
    // Another level is not the spot, however close.
    let mut crew = crew_with_orders(1);
    assert!(crew.set_pos(1, [16, 0, 1]));
    let mut events = Vec::new();
    crew.tick(11, &[], &flank_spots(), SEED, &mut events)
        .unwrap();
    assert!(!events.contains(&CrewEvent::Arrived { member: 1 }));
}

#[test]
fn crew_broken_once_after_give_up() {
    let mut crew = crew_with_orders(0);
    let give_up = crew.squad().skill_table().give_up_ticks_for(SKILL);
    let mut events = Vec::new();
    let mut broken = Vec::new();
    let mut orders = Vec::new();
    // Member 2 got its order at 9 and puts it off from 11 on.
    for t in 11..=200 {
        assert!(crew.member_chose(2, t, false));
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        for e in &events {
            match e {
                CrewEvent::Broken { member: 2 } => broken.push(t),
                CrewEvent::Order { member: 2, .. } => orders.push(t),
                _ => {}
            }
        }
    }
    assert_eq!(broken[0], 11 + give_up);
    // A goal breaks once: another `Broken` only after a new order.
    for w in broken.windows(2) {
        assert!(orders.iter().any(|&o| w[0] < o && o <= w[1]), "{broken:?}");
    }
    assert!(!crew.member_chose(99, 11, false));
}

#[test]
fn crew_member_chose_won_keeps_the_goal() {
    let mut crew = crew_with_orders(0);
    let mut events = Vec::new();
    for t in 11..=200 {
        assert!(crew.member_chose(2, t, t % 10 != 0));
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        assert!(!events.contains(&CrewEvent::Broken { member: 2 }));
    }
}

#[test]
fn crew_suspended_member_never_broken() {
    let mut crew = crew_with_orders(0);
    let mut events = Vec::new();
    assert!(crew.suspend(2));
    assert!(crew.set_gone(3));
    assert!(!crew.suspend(99));
    // Both stand on their gather spots and put their goal off.
    assert!(crew.set_pos(2, [20, 4, 0]));
    assert!(crew.set_pos(3, [20, -4, 0]));
    for t in 11..=300 {
        crew.member_chose(2, t, false);
        crew.member_chose(3, t, false);
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        for e in &events {
            assert!(
                !matches!(
                    e,
                    CrewEvent::Broken { member: 2 | 3 }
                        | CrewEvent::Arrived { member: 2 | 3 }
                        | CrewEvent::Start { member: 2 | 3 }
                ),
                "tick {t}: {e:?}"
            );
        }
        assert!(!crew.keeper(2).unwrap().broken(t, 0));
        assert!(!crew.keeper(3).unwrap().broken(t, 0));
    }
    // The squad replanned without it (plan 2, active at 16): its keeper was
    // cleared then. Back in play it gets no order and holds no goal.
    assert!(crew.goal(2).is_none());
    assert!(crew.reinstate(2, 301));
    assert!(crew.goal(2).is_none());
    for t in 301..=500 {
        crew.member_chose(2, t, false);
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        assert!(
            !events.contains(&CrewEvent::Broken { member: 2 }),
            "tick {t}"
        );
        assert_eq!(crew.member_inputs(2, t).unwrap().has_goal, 0);
    }
}

/// True when `events` holds an order, start, arrival or break of `id`.
fn about(events: &[CrewEvent], id: u32) -> bool {
    events.iter().any(|e| {
        matches!(e,
            CrewEvent::Order { member, .. }
            | CrewEvent::Start { member }
            | CrewEvent::Arrived { member }
            | CrewEvent::Broken { member } if *member == id)
    })
}

#[test]
fn crew_drops_the_goal_of_a_member_the_new_plan_left_out() {
    let mut crew = crew_with_orders(0);
    let mut events = Vec::new();
    // Member 2 is out while the squad replans (member 3 lost), then back
    // by the low-level call (no order) before the plan becomes active.
    assert!(crew.suspend(2));
    assert!(crew.set_gone(3));
    crew.tick(11, &[], &flank_spots(), SEED, &mut events)
        .unwrap();
    assert!(matches!(events[..], [CrewEvent::Planned(_)]));
    assert!(crew.squad_mut().group_mut().resume(2));
    let old = crew.goal(2).unwrap();
    let mut activated = None;
    for t in 12..=250 {
        crew.member_chose(2, t, false);
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        if activated.is_none() {
            if !events.iter().any(|e| matches!(e, CrewEvent::Activated(_))) {
                continue;
            }
            activated = Some(t);
            assert!(crew.goal(2).is_none(), "cleared at activation");
            // It stands where the dead plan sent it.
            assert!(crew.set_pos(2, old.pos.unwrap()));
            continue;
        }
        if events
            .iter()
            .any(|e| matches!(e, CrewEvent::Order { member: 2, .. }))
        {
            // A later plan takes it again (here at 43): from then on its
            // events are real.
            assert!(t > 30, "{t}");
            break;
        }
        assert!(!about(&events, 2), "tick {t}: {events:?}");
        assert!(crew.goal(2).is_none());
        assert_eq!(crew.member_inputs(2, t).unwrap().has_goal, 0);
    }
    assert_eq!(activated, Some(16));
}

#[test]
fn crew_ignores_a_dead_plans_goal_while_the_new_order_travels() {
    let mut crew = crew_with_orders(0);
    let mut events = Vec::new();
    // Member 3 lost, member 4 out: plan 2 (active at 16) gives member 2 a
    // new goal by radio, which takes a while. Until it comes, member 2
    // still holds its plan-1 goal: standing on that spot and putting it
    // off counts for nothing.
    assert!(crew.set_gone(3));
    assert!(crew.suspend(4));
    let old = crew.goal(2).unwrap();
    let mut order_at = None;
    for t in 11..=40 {
        crew.member_chose(2, t, false);
        if t == 16 {
            assert!(crew.set_pos(2, old.pos.unwrap()));
        }
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        if events
            .iter()
            .any(|e| matches!(e, CrewEvent::Order { member: 2, .. }))
        {
            order_at = Some(t);
            break;
        }
        if t >= 16 {
            let live = crew.squad().active_plan().map(|p| p.id);
            assert!(live.is_some() && live != Some(old.plan), "tick {t}");
            assert_eq!(crew.goal(2).map(|g| g.plan), Some(old.plan));
            assert!(!about(&events, 2), "tick {t}: {events:?}");
        }
    }
    assert!(order_at.unwrap() > 16, "the new order travels");
}

#[test]
fn crew_reinstate_without_a_goal_clears_the_keeper() {
    let mut crew = crew_with_orders(0);
    let mut events = Vec::new();
    assert!(crew.suspend(2));
    assert!(crew.set_gone(3));
    crew.tick(11, &[], &flank_spots(), SEED, &mut events)
        .unwrap();
    assert!(crew.goal(2).is_some(), "still the old plan's goal");
    // Back at 16: the reinstate itself makes plan 2 (without member 2)
    // active, before any tick saw it.
    assert!(crew.reinstate(2, 16));
    assert!(crew.goal(2).is_none());
    assert_eq!(crew.member_inputs(2, 16).unwrap().has_goal, 0);
    crew.tick(16, &[], &flank_spots(), SEED, &mut events)
        .unwrap();
    assert!(events.iter().any(|e| matches!(e, CrewEvent::Activated(_))));
    assert!(!about(&events, 2));
    assert!(!crew.reinstate(99, 16));
}

#[test]
fn crew_suspend_through_the_squad_pauses_the_keeper() {
    let mut crew = crew_with_orders(0);
    let mut events = Vec::new();
    assert!(crew.squad_mut().group_mut().suspend(2));
    for t in 11..=200 {
        crew.member_chose(2, t, false);
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        assert!(!events.contains(&CrewEvent::Broken { member: 2 }));
    }
}

#[test]
fn crew_keeper_slots_follow_the_group() {
    let mut crew = crew_with_orders(0);
    assert!(crew.keeper(4).is_some());
    assert!(crew.squad_mut().group_mut().remove(4));
    assert!(
        crew.squad_mut()
            .group_mut()
            .add(scene_member(7, 0, 0))
            .is_some()
    );
    let mut events = Vec::new();
    crew.tick(11, &[], &flank_spots(), SEED, &mut events)
        .unwrap();
    assert!(crew.keeper(4).is_none());
    assert!(crew.keeper(7).unwrap().goal().is_none());
    assert!(crew.remove_member(7));
    assert!(crew.keeper(7).is_none());
    assert!(!crew.remove_member(7));
    assert!(crew.keeper_mut(1).is_some());
}

/// `(tick - t0, event)` with goal times relative to `t0` too.
fn relative(run: &[(u32, CrewEvent)], t0: u32) -> Vec<(u32, CrewEvent)> {
    run.iter()
        .map(|(t, e)| {
            let e = match *e {
                CrewEvent::Order {
                    member,
                    mut goal,
                    start_at,
                } => {
                    goal.issued_at = goal.issued_at.wrapping_sub(t0);
                    CrewEvent::Order {
                        member,
                        goal,
                        start_at: start_at.map(|s| s.wrapping_sub(t0)),
                    }
                }
                e => e,
            };
            (t.wrapping_sub(t0), e)
        })
        .collect()
}

#[test]
fn crew_start_event_wraps() {
    let mut crew = flank_crew(8, 0);
    let from_zero = run_crew(&mut crew, 0, 90);
    for t0 in [u32::MAX - 50, u32::MAX - 20, u32::MAX - 7] {
        let mut crew = flank_crew(8, t0);
        let run = run_crew(&mut crew, t0, 90);
        let rel = relative(&run, t0);
        assert_eq!(rel, from_zero, "t0 = {t0}");
        // Each start once: one `Start` per order that has a start tick.
        for (id, ..) in MEMBERS {
            let starts = rel
                .iter()
                .filter(|(_, e)| *e == CrewEvent::Start { member: id })
                .count();
            let timed = rel
                .iter()
                .filter(|(_, e)| {
                    matches!(e, CrewEvent::Order { member, start_at: Some(_), .. } if *member == id)
                })
                .count();
            assert_eq!(starts, timed, "member {id}, t0 = {t0}");
        }
    }
}

#[test]
fn crew_hex_needs_hex_metric() {
    let hex = |metric| {
        Crew::new(CrewConfig {
            squad: SquadConfig {
                metric,
                ..SquadConfig::default()
            },
            hex_layout: Some(HexLayout::OddR),
            ..CrewConfig::default()
        })
        .err()
    };
    assert_eq!(hex(Metric::Chebyshev), Some(CrewError::HexNeedsHexMetric));
    assert_eq!(hex(Metric::Euclid), Some(CrewError::HexNeedsHexMetric));
    assert_eq!(hex(Metric::Hex), None);
    let zero = Crew::new(CrewConfig {
        squad: SquadConfig {
            members: 0,
            ..SquadConfig::default()
        },
        ..CrewConfig::default()
    });
    assert_eq!(zero.err(), Some(CrewError::ZeroCapacity));
    assert!(!CrewError::HexNeedsHexMetric.to_string().is_empty());
    assert!(!CrewError::ZeroCapacity.to_string().is_empty());
    let d = CrewConfig::default();
    assert_eq!((d.arrive_radius, d.hex_layout), (0, None));
    assert_eq!(d.squad, SquadConfig::default());
}

/// Is `p` on a shifted row (R layouts) or column (Q layouts)?
fn on_odd_line(layout: HexLayout, p: Pos) -> bool {
    match layout {
        HexLayout::OddR | HexLayout::EvenR => p[1] & 1 == 1,
        HexLayout::OddQ | HexLayout::EvenQ => p[0] & 1 == 1,
    }
}

#[test]
fn crew_hex_spots_come_back_in_offset_coords() {
    for layout in [
        HexLayout::OddR,
        HexLayout::EvenR,
        HexLayout::OddQ,
        HexLayout::EvenQ,
    ] {
        let mut crew = Crew::new(CrewConfig {
            squad: SquadConfig {
                metric: Metric::Hex,
                ..scene_config(8)
            },
            hex_layout: Some(layout),
            ..CrewConfig::default()
        })
        .unwrap();
        // Offset coordinates throughout; the spots lie on odd lines only
        // and three cells apart, so a lost or doubled shift misses them.
        let q = matches!(layout, HexLayout::OddQ | HexLayout::EvenQ);
        let flip = |x: i32, y: i32| -> Pos { if q { [y, x, 0] } else { [x, y, 0] } };
        for (id, x, y) in [(1, 0, 1), (2, 1, 3), (3, 1, -1), (4, -2, 1)] {
            assert!(crew.add_member(scene_member(id, 0, 0)).is_some());
            assert!(crew.set_pos(id, flip(x, y)));
        }
        let sq = crew.squad_mut();
        for id in [2, 3, 4] {
            assert!(sq.set_channel(id, RADIO));
        }
        assert!(sq.set_leader(1));
        sq.set_skill(SKILL);
        sq.set_target_kind(ENEMY);
        sq.set_spot_radius(Some(40));
        sq.add_tactic(tactics::flank(3, 4));
        crew.observe(
            0,
            Fact {
                subject: 10,
                kind: ENEMY,
                pos: flip(15, 1),
                ..Fact::default()
            },
        );
        let mut spots = Vec::new();
        for y in (-13..=15).step_by(2) {
            for x in (-3..=33).step_by(3) {
                spots.push(Spot {
                    pos: flip(x, y),
                    tags: 0,
                });
            }
        }
        let mut events = Vec::new();
        let mut checked = 0;
        let mut odd = 0;
        for t in 0..=30 {
            crew.tick(t, &[], &spots, SEED, &mut events).unwrap();
            for e in &events {
                let CrewEvent::Order { member, goal, .. } = e else {
                    continue;
                };
                assert_eq!(crew.goal(*member), Some(*goal));
                for (p, unchecked) in [
                    (goal.pos, goal.unchecked),
                    (goal.then_pos, goal.then_unchecked),
                ] {
                    let Some(p) = p else { continue };
                    assert!(!unchecked, "{layout:?}: {p:?} unchecked");
                    assert!(
                        spots.iter().any(|s| s.pos == p),
                        "{layout:?}: {p:?} is no given spot"
                    );
                    checked += 1;
                    if on_odd_line(layout, p) {
                        odd += 1;
                    }
                }
            }
        }
        assert!(checked >= 8, "{layout:?}: {checked} positions");
        assert!(odd >= 1, "{layout:?}");
        // The squad itself holds axial positions.
        let g = crew.squad().group();
        let leader = g.get(g.index(1).unwrap()).unwrap().pos;
        assert_eq!(leader, layout.to_axial_pos(flip(0, 1)));
    }
}

#[test]
fn crew_hex_observe_and_danger_go_in_axial() {
    let layout = HexLayout::OddR;
    let mut crew = Crew::new(CrewConfig {
        squad: SquadConfig {
            metric: Metric::Hex,
            ..scene_config(8)
        },
        hex_layout: Some(layout),
        ..CrewConfig::default()
    })
    .unwrap();
    assert!(crew.add_member(scene_member(1, 3, 3)).is_some());
    assert_eq!(
        crew.squad().group().get(0).unwrap().pos,
        layout.to_axial_pos([3, 3, 0])
    );
    crew.observe(0, leader_sees(0));
    let known = crew.squad().knowledge().iter().next().unwrap().pos;
    assert_eq!(known, layout.to_axial_pos([20, 0, 0]));
    assert!(crew.report_danger(1, [5, 7, 0], Some([9, 3, 0]), 0));
    crew.squad_mut().add_tactic(tactics::fall_back());
    let mut events = Vec::new();
    crew.tick(0, &[], &[], SEED, &mut events).unwrap();
    let a = *crew.squad().areas().iter().next().unwrap();
    assert_eq!(a.pos, layout.to_axial_pos([5, 7, 0]));
    assert_eq!(a.source, layout.to_axial_pos([9, 3, 0]));
}

#[test]
fn crew_member_inputs_match_keeper() {
    let mut crew = crew_with_orders(0);
    let mut events = Vec::new();
    for t in 11..=70 {
        if t % 7 == 0 {
            crew.member_chose(3, t, false);
        }
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        for (id, ..) in MEMBERS {
            let g = crew.squad().group();
            let m = *g.get(g.index(id).unwrap()).unwrap();
            let want = crew.keeper(id).unwrap().inputs(
                t,
                m.pos,
                Metric::Chebyshev,
                crew.squad().signal_given(),
                crew.squad().skill_table().hold_bonus_for(m.skill),
            );
            assert_eq!(crew.member_inputs(id, t).unwrap().as_row(), want);
        }
    }
    assert_eq!(crew.member_inputs(99, 70), None);
}

#[test]
fn crew_clears_goals_when_the_plan_ends() {
    let mut crew = crew_with_orders(0);
    let mut events = Vec::new();
    for (id, ..) in MEMBERS {
        assert!(crew.goal(id).is_some());
    }
    // The leader forgets every target: the plan aborts with nothing to
    // replan.
    crew.squad_mut().knowledge_mut().clear();
    crew.tick(11, &[], &flank_spots(), SEED, &mut events)
        .unwrap();
    assert_eq!(events, [CrewEvent::NoPlan]);
    assert!(crew.squad().active_plan().is_none());
    for t in 12..=200 {
        for (id, ..) in MEMBERS {
            assert!(crew.goal(id).is_none());
            assert_eq!(crew.member_inputs(id, t).unwrap().has_goal, 0);
            crew.member_chose(id, t, false);
        }
        crew.tick(t, &[], &flank_spots(), SEED, &mut events)
            .unwrap();
        assert_eq!(events, [CrewEvent::NoPlan], "tick {t}");
    }
}

#[test]
fn crew_no_plan_without_a_plan_leaves_keepers_alone() {
    let mut crew = Crew::new(CrewConfig {
        squad: scene_config(8),
        ..CrewConfig::default()
    })
    .unwrap();
    assert!(crew.add_member(scene_member(1, 0, 0)).is_some());
    crew.squad_mut().add_tactic(tactics::rush());
    let goal = Goal {
        kind: GoalKind::Move,
        pos: Some([3, 0, 0]),
        unchecked: false,
        then_pos: None,
        then_unchecked: false,
        target: None,
        start: Start::Now,
        slot: 0,
        plan: 0,
        issued_at: 0,
        start_delay: 0,
    };
    crew.keeper_mut(1).unwrap().set(0, goal);
    let mut events = Vec::new();
    crew.tick(0, &[], &[], SEED, &mut events).unwrap();
    assert_eq!(events, [CrewEvent::NoPlan]);
    assert_eq!(crew.goal(1), Some(goal), "no plan went away");
}
