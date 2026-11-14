use super::*;

#[test]
fn presets_match_spec() {
    let s = Leeway::STRICT;
    assert_eq!(s.leash.distance, Some(3));
    assert_eq!(s.leash.ticks, None);
    assert_eq!(s.leash.on_lost, OnLost::Return);
    assert!(!s.replan && !s.widen);

    let f = Leeway::FREE;
    assert_eq!(f.leash.distance, None);
    assert_eq!(f.leash.ticks, None);
    assert_eq!(f.leash.on_lost, OnLost::Keep);
    assert!(f.replan && f.widen);

    let x = Leeway::FLEXIBLE;
    assert_eq!(x.leash.distance, Some(12));
    assert_eq!(x.leash.ticks, None);
    assert_eq!(
        x.leash.on_lost,
        OnLost::Search {
            ticks: 60,
            radius: 6
        }
    );
    assert!(x.replan && !x.widen);
    assert_eq!(Leeway::default(), Leeway::FLEXIBLE);
}

#[test]
fn plan_builders() {
    let steps = [Step::pause(), Step::wait(3), Step::wander()];
    let l = Plan::looping(&steps);
    assert!(l.repeat);
    assert_eq!(l.steps().len(), 3);
    let o = Plan::once(&steps);
    assert!(!o.repeat);
    assert_eq!(o.steps(), &steps);
}

#[test]
fn fails_when_fills_slots_in_order() {
    let s = Step::attack(7)
        .fails_when(Fail::Unknown { subject: 7 })
        .fails_when(Fail::Custom(2));
    assert_eq!(s.fails[0], Some(Fail::Unknown { subject: 7 }));
    assert_eq!(s.fails[1], Some(Fail::Custom(2)));
    assert_eq!(s.fails[2], None);
    let p = Plan::once(&[Step::pause()])
        .fails_when(Fail::Custom(1))
        .fails_when(Fail::Flags { mask: 1, value: 1 });
    assert_eq!(p.fails()[0], Some(Fail::Custom(1)));
    assert_eq!(p.fails()[1], Some(Fail::Flags { mask: 1, value: 1 }));
    assert_eq!(p.fails()[2], None);
}

#[test]
fn config_defaults_and_motives() {
    let c = MissionConfig::default();
    assert_eq!((c.max_steps, c.max_playbook), (16, 8));
    assert_eq!((c.enemy_bits, c.defeated_bits), (1, 2));
    assert_eq!((c.lost_after, c.pause_ticks, c.safe_distance), (30, 50, 12));
    assert_eq!(c.extra_columns, 0);
    assert_eq!(c.pick, crate::Pick::Best);
    assert_eq!(c.hex_layout, None);
    assert_eq!(Motive::defeat_enemies().subject, None);
    assert_eq!(Motive::guard_subject(4).subject, Some(4));
    assert_eq!(Motive::reach([1, 2, 3]).place, Some([1, 2, 3]));
}

#[test]
fn errors_display() {
    assert!(!MissionError::EmptyPlan.to_string().is_empty());
    let e: &dyn std::error::Error = &MissionError::HexNeedsHexMetric;
    assert!(!e.to_string().is_empty());
}

// ---- Running a plan ----

use crate::memory::{Evict, Fact, Knowledge, Metric, PointPattern, Pos, points_around};
use sagax_core::HexLayout;

const KIND: u32 = 0;

fn knowledge() -> Knowledge {
    Knowledge::new(8, Evict::Oldest).unwrap()
}

fn fact(subject: u32, pos: Pos) -> Fact {
    Fact {
        subject,
        kind: KIND,
        pos,
        ..Fact::default()
    }
}

/// The target cell of an action that walks somewhere.
fn target(a: Action) -> Option<Pos> {
    match a {
        Action::GoTo(p) | Action::Hold(p) | Action::Search(p) | Action::Flee(p) => Some(p),
        Action::Follow { pos, .. } | Action::Attack { pos, .. } => Some(pos),
        Action::Custom { pos, .. } => pos,
        Action::None | Action::Wait | Action::Wander => None,
    }
}

/// One cell (Chebyshev step) from `pos` toward the action's target.
fn step_toward(pos: Pos, a: Action) -> Pos {
    match target(a) {
        Some(t) => [
            pos[0] + (t[0] - pos[0]).signum(),
            pos[1] + (t[1] - pos[1]).signum(),
            pos[2],
        ],
        None => pos,
    }
}

fn mission(config: MissionConfig) -> Mission {
    Mission::new(config, Motive::guard_place([0, 0, 0])).unwrap()
}

#[test]
fn patrol_loops_like_a_script() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::looping(&[
        Step::go_to([2, 0, 0]),
        Step::wait(3),
        Step::go_to([0, 0, 0]),
    ]))
    .unwrap();
    let k = knowledge();
    let mut pos = [0, 0, 0];
    let mut trace = Vec::new();
    for now in 0..20 {
        let out = m.tick(now, pos, &k, 0);
        let action = out.action;
        let events = out.events().to_vec();
        trace.push((now, m.step_index(), action, events));
        pos = step_toward(pos, action);
    }
    use Action::{GoTo, Wait};
    use Event::{Looped, StepDone};
    let a = GoTo([2, 0, 0]);
    let b = GoTo([0, 0, 0]);
    let expected: Vec<(u32, Option<usize>, Action, Vec<Event>)> = vec![
        (0, Some(0), a, vec![]),
        (1, Some(0), a, vec![]),
        (2, Some(1), Wait, vec![StepDone(0)]),
        (3, Some(1), Wait, vec![]),
        (4, Some(1), Wait, vec![]),
        (5, Some(2), b, vec![StepDone(1)]),
        (6, Some(2), b, vec![]),
        (7, Some(0), a, vec![StepDone(2), Looped]),
        (8, Some(0), a, vec![]),
        (9, Some(1), Wait, vec![StepDone(0)]),
        (10, Some(1), Wait, vec![]),
        (11, Some(1), Wait, vec![]),
        (12, Some(2), b, vec![StepDone(1)]),
        (13, Some(2), b, vec![]),
        (14, Some(0), a, vec![StepDone(2), Looped]),
        (15, Some(0), a, vec![]),
        (16, Some(1), Wait, vec![StepDone(0)]),
        (17, Some(1), Wait, vec![]),
        (18, Some(1), Wait, vec![]),
        (19, Some(2), b, vec![StepDone(1)]),
    ];
    assert_eq!(trace, expected);
}

#[test]
fn no_plan_does_nothing() {
    let mut m = mission(MissionConfig::default());
    let out = m.tick(0, [0, 0, 0], &knowledge(), 0);
    assert_eq!(out.action, Action::None);
    assert!(out.events().is_empty());
    assert_eq!(m.plan(), None);
    assert_eq!(m.step_index(), None);
}

#[test]
fn follow_reads_knowledge_only() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::follow(5)])).unwrap();
    let mut k = knowledge();
    k.learn(0, fact(5, [4, 0, 0]));
    let follow = |pos| Action::Follow { subject: 5, pos };
    assert_eq!(m.tick(0, [0, 0, 0], &k, 0).action, follow([4, 0, 0]));
    // In the game's truth the subject walks on to (6, 0), but this
    // character is not told: its knowledge keeps (4, 0).
    assert_eq!(m.tick(1, [1, 0, 0], &k, 0).action, follow([4, 0, 0]));
    // Now it learns it.
    k.set_pos(5, KIND, [6, 0, 0]);
    assert_eq!(m.tick(2, [2, 0, 0], &k, 0).action, follow([6, 0, 0]));
    assert_eq!(m.step_index(), Some(0));
}

#[test]
fn attack_done_when_known_defeated() {
    let config = MissionConfig::default();
    let defeated = config.defeated_bits;
    let mut m = mission(config);
    m.set_plan(&Plan::once(&[Step::attack(5), Step::signal(9)]))
        .unwrap();
    let mut k = knowledge();
    k.learn(0, fact(5, [3, 0, 0]));
    let out = m.tick(0, [0, 0, 0], &k, 0);
    assert_eq!(
        out.action,
        Action::Attack {
            subject: 5,
            pos: [3, 0, 0]
        }
    );
    assert!(out.events().is_empty());
    k.set_data(5, KIND, defeated);
    let out = m.tick(1, [1, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
    assert_eq!(out.action, Action::None);
    let out = m.tick(2, [1, 0, 0], &k, 0);
    assert_eq!(
        out.events(),
        &[Event::Signal(9), Event::StepDone(1), Event::PlanDone]
    );
    assert_eq!(out.action, Action::None);
    assert_eq!(m.step_index(), None);
}

#[test]
fn attack_done_when_forgotten() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::attack(5), Step::wait(10)]))
        .unwrap();
    let mut k = knowledge();
    k.learn(0, fact(5, [3, 0, 0]));
    m.tick(0, [0, 0, 0], &k, 0);
    k.remove(5, KIND);
    let out = m.tick(1, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
    assert_eq!(out.action, Action::Wait);
}

#[test]
fn wait_survives_time_wrap() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::wait(3), Step::signal(9)]))
        .unwrap();
    let k = knowledge();
    let mut now = u32::MAX - 1;
    for _ in 0..3 {
        let out = m.tick(now, [0, 0, 0], &k, 0);
        assert_eq!(out.action, Action::Wait);
        assert!(out.events().is_empty());
        now = now.wrapping_add(1);
    }
    assert_eq!(now, 1);
    let out = m.tick(now, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
    assert_eq!(m.step_index(), Some(1));
}

#[test]
fn pause_waits_pause_ticks() {
    let mut m = mission(MissionConfig {
        pause_ticks: 2,
        ..MissionConfig::default()
    });
    m.set_plan(&Plan::once(&[Step::pause(), Step::wait(5)]))
        .unwrap();
    let k = knowledge();
    assert_eq!(m.tick(0, [0, 0, 0], &k, 0).action, Action::Wait);
    assert!(m.tick(1, [0, 0, 0], &k, 0).events().is_empty());
    assert_eq!(m.tick(2, [0, 0, 0], &k, 0).events(), &[Event::StepDone(0)]);
}

#[test]
fn empty_knowledge_subject_step_breaks() {
    let k = knowledge();
    for step in [
        Step::follow(5),
        Step::go_to_where(Where::Subject(5)),
        Step::hold(Where::MotiveSubject),
    ] {
        let mut m = mission(MissionConfig::default());
        m.set_plan(&Plan::once(&[step])).unwrap();
        let out = m.tick(0, [0, 0, 0], &k, 0);
        assert_eq!(out.action, Action::None);
        assert_eq!(out.events(), &[Event::Broken(BreakReason::Impossible)]);
        let out = m.tick(1, [0, 0, 0], &k, 0);
        assert_eq!(out.action, Action::None);
        assert!(out.events().is_empty());
        assert_eq!(m.step_index(), None);
    }
}

#[test]
fn impossible_step_skipped_when_asked() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[
        Step::follow(5).skip_if_impossible(),
        Step::go_to([3, 0, 0]),
    ]))
    .unwrap();
    let out = m.tick(0, [0, 0, 0], &knowledge(), 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
    assert_eq!(out.action, Action::GoTo([3, 0, 0]));
}

#[test]
fn fails_break_step_then_plan() {
    let mut k = knowledge();
    k.learn(0, fact(5, [3, 0, 0]));

    // A step fail on flags.
    let mut m = mission(MissionConfig::default());
    let flag = Fail::Flags { mask: 4, value: 4 };
    m.set_plan(&Plan::looping(&[Step::wait(9).fails_when(flag)]))
        .unwrap();
    assert_eq!(m.tick(0, [0, 0, 0], &k, 0).action, Action::Wait);
    let out = m.tick(1, [0, 0, 0], &k, 4);
    assert_eq!(out.events(), &[Event::Broken(BreakReason::Fail(flag))]);
    assert_eq!(out.action, Action::None);

    // The step's fail is checked before the plan's.
    let step_fail = Fail::Unknown { subject: 6 };
    let plan_fail = Fail::Flags { mask: 0, value: 0 };
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::wait(9).fails_when(step_fail)]).fails_when(plan_fail))
        .unwrap();
    let out = m.tick(0, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::Broken(BreakReason::Fail(step_fail))]);

    // A fact condition on an unknown fact, a custom one and "defeated" on a
    // subject that is not are all false.
    let mut m = mission(MissionConfig::default());
    m.set_plan(
        &Plan::once(&[Step::wait(9)])
            .fails_when(Fail::Fact {
                subject: 6,
                kind: KIND,
                mask: 0,
                value: 0,
            })
            .fails_when(Fail::Custom(1))
            .fails_when(Fail::Defeated { subject: 5 }),
    )
    .unwrap();
    let out = m.tick(0, [0, 0, 0], &k, 0);
    assert!(out.events().is_empty());
    assert_eq!(out.action, Action::Wait);

    // A known fact matching mask and value breaks the plan.
    let stolen = Fail::Fact {
        subject: 5,
        kind: KIND,
        mask: 8,
        value: 8,
    };
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::wait(9)]).fails_when(stolen))
        .unwrap();
    assert!(m.tick(0, [0, 0, 0], &k, 0).events().is_empty());
    k.set_data(5, KIND, 8);
    let out = m.tick(1, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::Broken(BreakReason::Fail(stolen))]);
}

#[test]
fn custom_step_waits_for_finish_step() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[
        Step::custom(3, Where::Here, 7),
        Step::wait(5),
    ]))
    .unwrap();
    let k = knowledge();
    let custom = Action::Custom {
        kind: 3,
        pos: Some([1, 1, 0]),
        param: 7,
    };
    assert_eq!(m.tick(0, [1, 1, 0], &k, 0).action, custom);
    // `Here` was fixed when the step began.
    for now in 1..5 {
        let out = m.tick(now, [2, 2, 0], &k, 0);
        assert_eq!(out.action, custom);
        assert!(out.events().is_empty());
    }
    m.finish_step();
    let out = m.tick(5, [2, 2, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
    assert_eq!(out.action, Action::Wait);
}

#[test]
fn wander_ends_the_plan() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::looping(&[
        Step::wait(1),
        Step::wander(),
        Step::wait(1),
    ]))
    .unwrap();
    let k = knowledge();
    assert_eq!(m.tick(0, [0, 0, 0], &k, 0).action, Action::Wait);
    // The wander step begins and wanders ...
    let out = m.tick(1, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
    assert_eq!(out.action, Action::Wander);
    // ... and ends the plan at the next tick, without looping.
    let out = m.tick(2, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(1), Event::PlanDone]);
    assert_eq!(out.action, Action::Wander);
    assert_eq!(m.step_index(), None);
    for now in 3..8 {
        let out = m.tick(now, [0, 0, 0], &k, 0);
        assert_eq!(out.action, Action::Wander);
        assert!(out.events().is_empty());
    }
}

#[test]
fn call_and_squad_without_widen_are_done() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[
        Step::call(1, 2),
        Step::squad(3, Where::Here),
        Step::wait(1),
    ]))
    .unwrap();
    let k = knowledge();
    // One step change per tick: the call is done on the first tick, the
    // squad step begins and is done on the next.
    let out = m.tick(0, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
    assert_eq!(out.action, Action::None);
    let out = m.tick(1, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(1)]);
    assert_eq!(out.action, Action::Wait);
}

#[test]
fn wait_for_subject_within_radius() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::wait_for(5, 2), Step::wait(1)]))
        .unwrap();
    let mut k = knowledge();
    assert_eq!(m.tick(0, [0, 0, 0], &k, 0).action, Action::Wait);
    k.learn(1, fact(5, [3, 0, 0]));
    assert!(m.tick(1, [0, 0, 0], &k, 0).events().is_empty());
    k.set_pos(5, KIND, [2, 1, 0]);
    assert_eq!(m.tick(2, [0, 0, 0], &k, 0).events(), &[Event::StepDone(0)]);
}

#[test]
fn euclid_arrival_compares_squares() {
    let mut m = mission(MissionConfig {
        metric: Metric::Euclid,
        arrive_radius: 2,
        ..MissionConfig::default()
    });
    m.set_plan(&Plan::once(&[Step::go_to([0, 0, 0]), Step::wait(1)]))
        .unwrap();
    let k = knowledge();
    // 2² + 1² = 5 > 4: not there.
    assert!(m.tick(0, [2, 1, 0], &k, 0).events().is_empty());
    // On another level: not there.
    assert!(m.tick(1, [0, 0, 1], &k, 0).events().is_empty());
    // 1² + 1² = 2 <= 4: there.
    assert_eq!(m.tick(2, [1, 1, 0], &k, 0).events(), &[Event::StepDone(0)]);
}

#[test]
fn search_visits_points_until_time_is_up() {
    let pattern = PointPattern {
        first_distance: 2,
        offsets: vec![0, 64],
        distances: vec![4, 4],
        ..PointPattern::default()
    };
    let config = MissionConfig {
        search: pattern.clone(),
        ..MissionConfig::default()
    };
    let centre = [10, 10, 0];
    let id = config.id;
    let p = |i| points_around(id, i, centre, &pattern).unwrap();
    let mut m = mission(config);
    m.set_plan(&Plan::once(&[
        Step::search(Where::Fixed(centre), 6, 30),
        Step::wait(1),
    ]))
    .unwrap();
    let k = knowledge();
    let mut pos = centre;
    let mut seen = Vec::new();
    for now in 0..30 {
        let out = m.tick(now, pos, &k, 0);
        let Action::Search(at) = out.action else {
            panic!("tick {now}: {:?}", out.action)
        };
        if seen.last() != Some(&at) {
            seen.push(at);
        }
        pos = step_toward(pos, out.action);
    }
    assert_eq!(&seen[..4], &[p(0), p(1), p(2), p(0)]);
    let out = m.tick(30, pos, &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
}

#[test]
fn hex_positions_in_offset() {
    let mut m = mission(MissionConfig {
        metric: Metric::Hex,
        hex_layout: Some(HexLayout::OddR),
        arrive_radius: 1,
        ..MissionConfig::default()
    });
    m.set_plan(&Plan::once(&[
        Step::go_to([0, 0, 0]),
        Step::go_to_where(Where::Subject(5)),
        Step::wait(9),
    ]))
    .unwrap();
    let mut k = knowledge();
    // Offset (3, 2) is axial (2, 2) in odd-r; the game gives offset.
    k.learn(0, fact(5, [3, 2, 0]));
    // Offset (1, 1) is one cell away by Chebyshev but two hexes.
    let out = m.tick(0, [1, 1, 0], &k, 0);
    assert_eq!(out.action, Action::GoTo([0, 0, 0]));
    assert!(out.events().is_empty());
    // Offset (0, 1) is a neighbour hex of (0, 0).
    let out = m.tick(1, [0, 1, 0], &k, 0);
    assert_eq!(out.events(), &[Event::StepDone(0)]);
    assert_eq!(out.action, Action::GoTo([3, 2, 0]));
    // Offset (1, 2) is two hexes from (3, 2); (3, 3) is a neighbour.
    assert!(m.tick(2, [1, 2, 0], &k, 0).events().is_empty());
    assert_eq!(m.tick(3, [3, 3, 0], &k, 0).events(), &[Event::StepDone(1)]);
}

#[test]
fn new_and_set_plan_check_their_input() {
    let hex_no_metric = MissionConfig {
        hex_layout: Some(HexLayout::OddR),
        ..MissionConfig::default()
    };
    assert_eq!(
        Mission::new(hex_no_metric, Motive::reach([0, 0, 0])).err(),
        Some(MissionError::HexNeedsHexMetric)
    );
    let no_steps = MissionConfig {
        max_steps: 0,
        ..MissionConfig::default()
    };
    assert_eq!(
        Mission::new(no_steps, Motive::reach([0, 0, 0])).err(),
        Some(MissionError::ZeroCapacity)
    );
    let mut m = mission(MissionConfig {
        max_steps: 2,
        ..MissionConfig::default()
    });
    assert_eq!(m.set_plan(&Plan::once(&[])), Err(MissionError::EmptyPlan));
    assert_eq!(
        m.set_plan(&Plan::once(&[Step::pause(); 3])),
        Err(MissionError::TooManySteps)
    );
    assert_eq!(m.plan(), None);
    let plan = Plan::looping(&[Step::pause(), Step::wander()]);
    m.set_plan(&plan).unwrap();
    assert_eq!(m.plan(), Some(&plan));
    assert_eq!(m.step_index(), Some(0));
    m.set_leeway(Leeway::STRICT);
    assert_eq!(m.leeway(), Leeway::STRICT);
    m.set_motive(Motive::defeat(3));
    assert_eq!(m.motive(), Motive::defeat(3));
    assert_eq!(m.config().max_steps, 2);
}

#[test]
fn search_skips_points_outside_radius() {
    let pattern = PointPattern {
        first_distance: 2,
        offsets: vec![0, 0],
        distances: vec![9, 3],
        ..PointPattern::default()
    };
    let config = MissionConfig {
        search: pattern.clone(),
        ..MissionConfig::default()
    };
    let id = config.id;
    let centre = [10, 10, 0];
    let p = |i| points_around(id, i, centre, &pattern).unwrap();
    let plan = |radius| Plan::once(&[Step::search(Where::Fixed(centre), radius, 40)]);
    let k = knowledge();

    // Radius 4: point 1 (9 away) is skipped.
    let mut m = mission(config.clone());
    m.set_plan(&plan(4)).unwrap();
    let mut pos = centre;
    let mut seen = Vec::new();
    for now in 0..30 {
        let out = m.tick(now, pos, &k, 0);
        if let Action::Search(at) = out.action
            && seen.last() != Some(&at)
        {
            seen.push(at);
        }
        pos = step_toward(pos, out.action);
    }
    assert_eq!(&seen[..3], &[p(0), p(2), p(0)]);
    assert!(!seen.contains(&p(1)));

    // Radius 1: every point is skipped, the search stays at the centre.
    let mut m = mission(config);
    m.set_plan(&plan(1)).unwrap();
    for now in 0..3 {
        assert_eq!(m.tick(now, centre, &k, 0).action, Action::Search(centre));
    }
}

#[test]
fn hex_search_and_here_in_offset() {
    // Step 0 at the centre, step 1 a quarter turn away, inside the bounds of
    // an offset map 0..=5.
    let pattern = PointPattern {
        first_distance: 0,
        offsets: vec![64],
        distances: vec![3],
        min: (0, 0),
        max: (5, 5),
        ..PointPattern::default()
    };
    let config = MissionConfig {
        metric: Metric::Hex,
        hex_layout: Some(HexLayout::OddR),
        search: pattern.clone(),
        ..MissionConfig::default()
    };
    let centre = [4, 4, 0];
    let expected = points_around(config.id, 1, centre, &pattern).unwrap();
    assert_ne!(expected, centre);
    assert!((0..=5).contains(&expected[0]) && (0..=5).contains(&expected[1]));
    let mut m = mission(config.clone());
    m.set_plan(&Plan::once(&[Step::search(Where::Fixed(centre), 0, 9)]))
        .unwrap();
    // Standing at the centre (point 0): on to point 1, in offset and in
    // the map's bounds.
    let out = m.tick(0, centre, &knowledge(), 0);
    assert_eq!(out.action, Action::Search(expected));

    // `Here` comes back in offset too.
    let mut m = mission(config);
    m.set_plan(&Plan::once(&[Step::hold(Where::Here)])).unwrap();
    assert_eq!(
        m.tick(0, [3, 2, 0], &knowledge(), 0).action,
        Action::Hold([3, 2, 0])
    );
}

#[test]
fn new_rejects_more_steps_than_u16_numbers() {
    let config = MissionConfig {
        max_steps: usize::from(u16::MAX) + 2,
        ..MissionConfig::default()
    };
    assert_eq!(
        Mission::new(config, Motive::reach([0, 0, 0])).err(),
        Some(MissionError::TooManySteps)
    );
}

// ---- Detours ----

fn attack(pos: Pos) -> Action {
    Action::Attack { subject: 5, pos }
}

/// A guard holding the gate at (0, 0) under `leeway`, after one tick there.
fn guard_at_gate(leeway: Leeway) -> Mission {
    let mut m = mission(MissionConfig::default());
    m.set_leeway(leeway);
    m.set_plan(&Plan::looping(&[Step::hold(Where::Fixed([0, 0, 0]))]))
        .unwrap();
    assert_eq!(
        m.tick(0, [0, 0, 0], &knowledge(), 0).action,
        Action::Hold([0, 0, 0])
    );
    m
}

#[test]
fn strict_guard_steps_three_and_returns() {
    let gate = [0, 0, 0];
    let mut m = guard_at_gate(Leeway::STRICT);
    let mut k = knowledge();
    k.learn(0, fact(5, [1, 0, 0]));
    assert!(m.detour(0, Detour::Engage(5), &k));
    assert!(m.in_detour());
    let mut pos = gate;
    let mut trace = Vec::new();
    for now in 1..=10 {
        // The attacker walks away one cell per tick, and the guard sees it.
        k.set_pos(5, KIND, [now as i32 + 1, 0, 0]);
        k.touch(5, KIND, now);
        let out = m.tick(now, pos, &k, 0);
        trace.push((pos, out.action, out.events().to_vec()));
        pos = step_toward(pos, out.action);
    }
    use Event::{DetourEnded, DetourStarted, Returned};
    let home = Action::GoTo(gate);
    let expected: Vec<(Pos, Action, Vec<Event>)> = vec![
        ([0, 0, 0], attack([2, 0, 0]), vec![DetourStarted]),
        ([1, 0, 0], attack([3, 0, 0]), vec![]),
        ([2, 0, 0], attack([4, 0, 0]), vec![]),
        ([3, 0, 0], attack([5, 0, 0]), vec![]),
        // One cell past the leash: the leash pulls.
        ([4, 0, 0], home, vec![DetourEnded(DetourEnd::Leash)]),
        ([3, 0, 0], home, vec![]),
        ([2, 0, 0], home, vec![]),
        ([1, 0, 0], home, vec![]),
        ([0, 0, 0], Action::Hold(gate), vec![Returned]),
        ([0, 0, 0], Action::Hold(gate), vec![]),
    ];
    assert_eq!(trace, expected);
    assert!(!m.in_detour() && !m.returning());
    assert_eq!(m.step_index(), Some(0));
    // The attacker is far outside the leash now: no new detour.
    assert!(!m.detour(11, Detour::Engage(5), &k));
}

#[test]
fn refused_while_attacker_outside_leash() {
    let mut m = guard_at_gate(Leeway::STRICT);
    let mut k = knowledge();
    // An unknown subject is refused.
    assert!(!m.detour(0, Detour::Engage(5), &k));
    k.learn(0, fact(5, [4, 0, 0]));
    for now in 1..10 {
        k.touch(5, KIND, now);
        assert!(!m.detour(now, Detour::Engage(5), &k));
        let out = m.tick(now, [0, 0, 0], &k, 0);
        assert!(out.events().is_empty());
        assert_eq!(out.action, Action::Hold([0, 0, 0]));
    }
    // Within the leash it is taken; without a distance leash always.
    k.set_pos(5, KIND, [3, 0, 0]);
    assert!(m.detour(10, Detour::Engage(5), &k));
    let mut m = guard_at_gate(Leeway::FREE);
    k.set_pos(5, KIND, [99, 0, 0]);
    assert!(m.detour(1, Detour::Engage(5), &k));
}

#[test]
fn flexible_guard_searches_then_resumes_patrol() {
    let mut m = mission(MissionConfig::default());
    assert_eq!(m.leeway(), Leeway::FLEXIBLE);
    let OnLost::Search { ticks: search, .. } = Leeway::FLEXIBLE.leash.on_lost else {
        panic!("FLEXIBLE searches")
    };
    let lost_after = m.config().lost_after;
    m.set_plan(&Plan::looping(&[
        Step::go_to([2, 0, 0]),
        Step::wait(10),
        Step::go_to([0, 0, 0]),
    ]))
    .unwrap();
    let mut k = knowledge();
    let mut pos = [0, 0, 0];
    // Ticks 0 and 1 walk, the wait begins at tick 2.
    for now in 0..5 {
        pos = step_toward(pos, m.tick(now, pos, &k, 0).action);
    }
    assert_eq!(m.step_index(), Some(1));
    // At tick 5 the wait has run 3 of its 10 ticks. The attacker is seen
    // once at tick 4, never again.
    let mut attacker = fact(5, [4, 0, 0]);
    attacker.seen_at = 4;
    k.learn(4, attacker);
    assert!(m.detour(5, Detour::Engage(5), &k));
    let mut log = Vec::new();
    for now in 5..200 {
        let out = m.tick(now, pos, &k, 0);
        let (action, events) = (out.action, out.events().to_vec());
        log.push((now, action, events, m.step_index()));
        pos = step_toward(pos, action);
    }
    let first = |e: Event| log.iter().find(|l| l.2.contains(&e)).unwrap().0;
    assert_eq!(first(Event::DetourStarted), 5);
    let lost_at = 4 + lost_after + 1;
    for l in &log {
        if l.0 < lost_at {
            assert_eq!(l.1, attack([4, 0, 0]), "tick {}", l.0);
        } else if l.0 < lost_at + search {
            assert!(matches!(l.1, Action::Search(_)), "tick {}", l.0);
        }
    }
    assert_eq!(first(Event::DetourEnded(DetourEnd::Lost)), lost_at + search);
    let back = first(Event::Returned);
    let at_back = &log[(back - 5) as usize];
    assert_eq!(at_back.3, Some(1));
    assert_eq!(at_back.1, Action::Wait);
    // The wait goes on with the 7 ticks it had left.
    assert_eq!(first(Event::StepDone(1)), back + 7);
}

#[test]
fn free_guard_keeps_on_until_defeated() {
    let mut m = guard_at_gate(Leeway::FREE);
    let defeated = m.config().defeated_bits;
    let mut k = knowledge();
    k.learn(0, fact(5, [2, 0, 0]));
    assert!(m.detour(0, Detour::Engage(5), &k));
    let mut pos = [0, 0, 0];
    for now in 1..80 {
        // The attacker runs to (11, 0) and is not seen after tick 10.
        if now <= 10 {
            k.set_pos(5, KIND, [now as i32 + 1, 0, 0]);
            k.touch(5, KIND, now);
        }
        let out = m.tick(now, pos, &k, 0);
        assert!(
            !out.events()
                .iter()
                .any(|e| matches!(e, Event::DetourEnded(_))),
            "tick {now}"
        );
        assert!(matches!(out.action, Action::Attack { subject: 5, .. }));
        pos = step_toward(pos, out.action);
    }
    assert_eq!(pos, [11, 0, 0]);
    k.set_data(5, KIND, defeated);
    let out = m.tick(80, pos, &k, 0);
    assert_eq!(out.events(), &[Event::DetourEnded(DetourEnd::Defeated)]);
    assert_eq!(out.action, Action::GoTo([0, 0, 0]));
    assert!(m.returning());
}

#[test]
fn forgotten_target_ends_detour() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::wait(50)])).unwrap();
    let mut k = knowledge();
    k.learn(0, fact(5, [2, 0, 0]));
    m.tick(0, [0, 0, 0], &k, 0);
    assert!(m.detour(0, Detour::Engage(5), &k));
    assert_eq!(m.tick(1, [1, 0, 0], &k, 0).action, attack([2, 0, 0]));
    k.remove(5, KIND);
    let out = m.tick(2, [1, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::DetourEnded(DetourEnd::Forgotten)]);
    assert_eq!(out.action, Action::GoTo([0, 0, 0]));
    let out = m.tick(3, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::Returned]);
    assert_eq!(out.action, Action::Wait);
}

#[test]
fn leash_ticks_wrap() {
    let mut m = mission(MissionConfig::default());
    m.set_leeway(Leeway {
        leash: Leash {
            distance: None,
            ticks: Some(5),
            on_lost: OnLost::Return,
        },
        ..Leeway::FREE
    });
    m.set_plan(&Plan::once(&[Step::wait(50)])).unwrap();
    let start = u32::MAX - 2;
    let mut k = knowledge();
    let mut attacker = fact(5, [2, 0, 0]);
    attacker.seen_at = start;
    k.learn(start, attacker);
    m.tick(start, [0, 0, 0], &k, 0);
    assert!(m.detour(start, Detour::Engage(5), &k));
    for i in 1..=5 {
        let now = start.wrapping_add(i);
        k.touch(5, KIND, now);
        let out = m.tick(now, [0, 0, 0], &k, 0);
        assert_eq!(out.action, attack([2, 0, 0]), "tick {now}");
    }
    let now = start.wrapping_add(6);
    k.touch(5, KIND, now);
    let out = m.tick(now, [0, 0, 0], &k, 0);
    assert_eq!(
        out.events(),
        &[Event::DetourEnded(DetourEnd::Leash), Event::Returned]
    );
}

#[test]
fn flee_until_safe_or_arrived() {
    let mut m = mission(MissionConfig {
        safe_distance: 3,
        ..MissionConfig::default()
    });
    m.set_plan(&Plan::once(&[Step::wait(50)])).unwrap();
    let mut k = knowledge();
    k.learn(0, fact(7, [-1, 0, 0]));
    m.tick(0, [0, 0, 0], &k, 0);
    let away = Detour::Flee {
        from: Where::Subject(7),
        to: None,
    };
    assert!(m.detour(0, away, &k));
    // Away from the threat: as far beyond as it is behind.
    let out = m.tick(1, [0, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::DetourStarted]);
    assert_eq!(out.action, Action::Flee([1, 0, 0]));
    assert_eq!(m.tick(2, [2, 0, 0], &k, 0).action, Action::Flee([5, 0, 0]));
    // 4 > 3 from the threat: safe.
    let out = m.tick(3, [3, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::DetourEnded(DetourEnd::Safe)]);
    assert_eq!(out.action, Action::GoTo([0, 0, 0]));

    // To a place the game gives.
    let to = Detour::Flee {
        from: Where::Fixed([-1, 0, 0]),
        to: Some([0, 2, 0]),
    };
    assert!(m.detour(4, to, &k));
    assert_eq!(m.tick(4, [0, 1, 0], &k, 0).action, Action::Flee([0, 2, 0]));
    let out = m.tick(5, [0, 2, 0], &k, 0);
    assert_eq!(out.events(), &[Event::DetourEnded(DetourEnd::Arrived)]);

    // The point away stays in the bounds of the map.
    let mut m = mission(MissionConfig {
        search: PointPattern {
            min: (0, 0),
            ..PointPattern::default()
        },
        ..MissionConfig::default()
    });
    m.tick(0, [0, 0, 0], &k, 0);
    let from_right = Detour::Flee {
        from: Where::Fixed([1, 1, 0]),
        to: None,
    };
    assert!(m.detour(0, from_right, &k));
    assert_eq!(m.tick(1, [0, 0, 0], &k, 0).action, Action::Flee([0, 0, 0]));
}

#[test]
fn custom_detour_ends_by_game_and_keeps_first_anchor() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::wait(50)])).unwrap();
    let k = knowledge();
    m.tick(0, [0, 0, 0], &k, 0);
    let custom = Detour::Custom {
        kind: 3,
        at: Where::Fixed([4, 4, 0]),
        param: 1,
    };
    assert!(m.detour(0, custom, &k));
    for now in 1..5 {
        let out = m.tick(now, [now as i32, 0, 0], &k, 0);
        assert_eq!(
            out.action,
            Action::Custom {
                kind: 3,
                pos: Some([4, 4, 0]),
                param: 1
            }
        );
    }
    // A new detour replaces it; the anchor stays the first one.
    assert!(m.detour(5, custom, &k));
    assert_eq!(
        m.tick(5, [5, 0, 0], &k, 0).events(),
        &[Event::DetourStarted]
    );
    m.end_detour();
    let out = m.tick(6, [5, 0, 0], &k, 0);
    assert_eq!(out.events(), &[Event::DetourEnded(DetourEnd::Ended)]);
    assert_eq!(out.action, Action::GoTo([0, 0, 0]));
}

#[test]
fn resumes_at_once_when_step_goes_elsewhere() {
    let mut m = mission(MissionConfig::default());
    m.set_plan(&Plan::once(&[Step::go_to([9, 0, 0])])).unwrap();
    let k = knowledge();
    m.tick(0, [0, 0, 0], &k, 0);
    let custom = Detour::Custom {
        kind: 1,
        at: Where::Here,
        param: 0,
    };
    assert!(m.detour(0, custom, &k));
    m.tick(1, [0, 3, 0], &k, 0);
    m.end_detour();
    let out = m.tick(2, [0, 3, 0], &k, 0);
    assert_eq!(
        out.events(),
        &[Event::DetourEnded(DetourEnd::Ended), Event::Returned]
    );
    assert_eq!(out.action, Action::GoTo([9, 0, 0]));
    assert_eq!(m.step_index(), Some(0));
}

#[test]
fn hex_detour_in_offset() {
    let mut m = mission(MissionConfig {
        metric: Metric::Hex,
        hex_layout: Some(HexLayout::OddR),
        ..MissionConfig::default()
    });
    m.set_leeway(Leeway::STRICT);
    // The gate at offset (3, 2); the attacker at offset (3, 3), a neighbour.
    m.set_plan(&Plan::looping(&[Step::hold(Where::Fixed([3, 2, 0]))]))
        .unwrap();
    let mut k = knowledge();
    k.learn(0, fact(5, [3, 3, 0]));
    m.tick(0, [3, 2, 0], &k, 0);
    assert!(m.detour(0, Detour::Engage(5), &k));
    assert_eq!(m.tick(1, [3, 2, 0], &k, 0).action, attack([3, 3, 0]));
    m.end_detour();
    assert_eq!(m.tick(2, [3, 3, 0], &k, 0).action, Action::GoTo([3, 2, 0]));
    // On the gate's row, offset (6, 2) is three hexes away, (7, 2) four.
    k.set_pos(5, KIND, [7, 2, 0]);
    assert!(!m.detour(3, Detour::Engage(5), &k));
    k.set_pos(5, KIND, [6, 2, 0]);
    assert!(m.detour(3, Detour::Engage(5), &k));
}
