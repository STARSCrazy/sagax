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
