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
