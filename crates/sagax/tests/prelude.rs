//! The prelude is enough for a path, a choice and a crew.

use sagax::prelude::*;

#[test]
fn three_jobs_with_the_prelude() {
    // A path.
    let mut map = DenseGrid::new(10, 10);
    let mut nav = Navigator::new(Topology::Square(GridRules::preset(Preset::Octile)), 1000);
    let report = nav.find_path(
        &mut map,
        Cell::new(0, 0, 0),
        &[Cell::new(9, 0, ANY_LEVEL)],
        Budget::UNLIMITED,
    );
    assert_eq!(report.status, NavStatus::Found);
    assert_eq!(report.cost, 90);

    // A choice.
    let mut d = Decider::new();
    let a = d.add_option(ONE);
    let b = d.add_option(ONE);
    assert!(d.add_consideration(a, Consideration::new(0, Curve::Linear, 0, 10)));
    assert!(d.add_consideration(b, Consideration::new(0, Curve::Linear, 0, 10)));
    let candidates = [Candidate::new(a, 0), Candidate::new(b, 0)];
    let rows = [[2], [8]];
    let choice = d.choose(&candidates, &rows, Pick::Best, 0).unwrap();
    assert!(matches!(choice, Some(Choice { candidate: 1, .. })));

    // A crew.
    let mut crew = Crew::new(CrewConfig {
        squad: SquadConfig {
            members: 1,
            memory: 4,
            areas: 2,
            inbox: 4,
            metric: Metric::Chebyshev,
            ..SquadConfig::default()
        },
        ..CrewConfig::default()
    })
    .unwrap();
    let member = Member {
        id: 1,
        pos: [0, 0, 0],
        roles: 0,
        skill: 128,
        state: MemberState::Active,
    };
    assert!(crew.add_member(member).is_some());
    crew.squad_mut().set_target_kind(1);
    crew.squad_mut().add_tactic(tactics::rush());
    crew.observe(
        0,
        Fact {
            subject: 11,
            kind: 1,
            pos: [10, 0, 0],
            seen_at: 0,
            ..Fact::default()
        },
    );
    let mut events = Vec::new();
    crew.tick(0, &[], &[], 7, &mut events).unwrap();
    assert!(events.iter().any(|e| matches!(e, CrewEvent::Planned(_))));
}

fn decider() -> (Decider, Vec<Candidate>) {
    let mut d = Decider::new();
    let a = d.add_option(ONE);
    let b = d.add_option(ONE);
    for o in [a, b] {
        assert!(d.add_consideration(o, Consideration::new(0, Curve::Linear, 0, 10)));
        assert!(d.add_consideration(o, Consideration::new(1, Curve::Inverse, 0, 10)));
    }
    (
        d,
        vec![
            Candidate::new(a, 0),
            Candidate::new(b, 1),
            Candidate::new(a, 2),
        ],
    )
}

#[test]
fn choose_equals_decide() {
    let (mut d, c) = decider();
    let rows = [[3, 4], [9, 1], [6, 6]];
    let flat: Vec<i32> = rows.iter().flatten().copied().collect();
    for seed in 0..20 {
        let pick = Pick::Weighted { top: 3 };
        let mut scores = Vec::new();
        let by_decide = d.decide(&c, &flat, 2, pick, seed, &mut scores);
        assert_eq!(d.choose(&c, &rows, pick, seed), by_decide);
    }
}

#[test]
fn choose_rejects_ragged_rows() {
    let (mut d, c) = decider();
    let ragged: [&[i32]; 3] = [&[1, 2], &[1], &[1, 2]];
    assert_eq!(
        d.choose(&c, &ragged, Pick::Best, 0),
        Err(DecideError::RowLengths)
    );
    let short = [[1, 2], [3, 4]];
    assert_eq!(
        d.choose(&c, &short, Pick::Best, 0),
        Err(DecideError::RowLengths)
    );
}

#[test]
fn choose_empty_is_none() {
    let (mut d, _) = decider();
    let rows: [[i32; 2]; 0] = [];
    assert_eq!(d.choose(&[], &rows, Pick::Best, 0), Ok(None));
}

/// The trial-run recipe of USAGE section 9 with the prelude only.
struct TrialRun {
    plans: Vec<(u32, u32)>, // playbook option -> cost of its dry run
}

impl MissionRules for TrialRun {
    // Only to show FailCtx: no candidate here has a custom condition.
    fn fail(&mut self, ctx: &FailCtx, _id: u32) -> bool {
        ctx.candidate.is_some() && ctx.step.is_none()
    }

    fn score(&mut self, ctx: &PlanCtx, built_in: u32) -> Option<u32> {
        if !ctx.feasible {
            return Some(0);
        }
        let &(_, cost) = self.plans.iter().find(|(o, _)| *o == ctx.option)?;
        Some(built_in / (1 + cost))
    }
}

#[test]
fn mission_rules_with_the_prelude() {
    let mut m = Mission::new(MissionConfig::default(), Motive::guard_place([0, 0, 0])).unwrap();
    let near = m
        .add_playbook(&Plan::once(&[Step::go_to([1, 0, 0])]), ONE, false)
        .unwrap();
    m.add_playbook(&Plan::once(&[Step::go_to([9, 0, 0])]), ONE, false)
        .unwrap();
    let knowledge = Knowledge::new(4, Evict::Oldest).unwrap();
    let mut rules = TrialRun {
        plans: vec![(near, 1), (1, 8)],
    };
    let out = m.tick_with(0, [0, 0, 0], &knowledge, 0, &mut rules);
    assert_eq!(out.events(), &[MissionEvent::Replanned { option: near }]);
    assert_eq!(out.action, Action::GoTo([1, 0, 0]));
    let plain = m.tick_with(1, [0, 0, 0], &knowledge, 0, &mut MissionBuiltInRules);
    assert_eq!(plain.action, Action::GoTo([1, 0, 0]));
}

#[test]
fn perception_with_the_prelude() {
    let world = sagax::perception::World::square(256);
    let profile = SenseProfile::human(256);
    let eye = Eye {
        id: 1,
        point: [128, 128, 0],
        facing: 0,
        profile: &profile,
        stats: Stats::NORMAL,
        disguise: Default::default(),
    };
    let target = Target {
        id: 2,
        point: [128 + 3 * 256, 128, 0],
        size: 256,
        conspicuity: 256,
        ..Target::default()
    };
    let mut map = sagax::perception::OpenAir;
    assert!(sight_line(&world, &mut map, eye.point, target.point).clear);
    let mut aware = Awareness::default();
    let mut out: Vec<Percept> = Vec::new();
    observe(&world, &mut map, 1, &eye, &target, &mut aware, &mut out);
    let mut attention = sagax::perception::Attention::default();
    let budget = sagax::perception::Budget {
        checks: 1,
        keep: 1,
        instant_cap: 1,
    };
    look(
        &world,
        &mut map,
        2,
        &eye,
        &[target],
        std::slice::from_mut(&mut aware),
        &mut attention,
        budget,
        &mut out,
    );
    let s = Stimulus {
        id: 1,
        source: 2,
        kind: 0,
        point: target.point,
        loudness: 8 * 256,
        channel: 0,
        at: 2,
    };
    assert!(hear(&world, &mut map, 2, &eye, &s, None, None, &mut out, 0));
    let mut known = Knowledge::new(4, Evict::Oldest).unwrap();
    let mut alert = sagax::decide::memory::Alertness::default();
    let cfg = sagax::decide::memory::AlertConfig::default();
    let kinds = sagax::perception::FactKinds {
        noticed_kind: 1,
        seen_kind: 2,
        heard_kind: 3,
        disguised_kind: None,
    };
    let amounts = sagax::perception::AlertAmounts::default();
    learn_percepts(&mut known, &mut alert, &cfg, 2, &out, &kinds, &amounts);
    assert!(known.get(2, 3).is_some());
}
