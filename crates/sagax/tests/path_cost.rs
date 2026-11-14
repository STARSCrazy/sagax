//! `path_cost` and `PathCost`: member costs from the walking distance.

use std::num::NonZeroUsize;

use sagax::prelude::*;
use sagax::rules::{PathCost, path_cost};

const SEED: u64 = 7;

/// 10 x 10, a wall at x = 5 for y 0..=8 (open at y = 9).
fn walled() -> DenseGrid {
    let mut map = DenseGrid::new(10, 10);
    for y in 0..=8 {
        map.set(5, y, DenseGrid::BLOCKED);
    }
    map
}

fn octile() -> Navigator {
    Navigator::new(Topology::Square(GridRules::preset(Preset::Octile)), 100)
}

#[test]
fn path_cost_matches_find_path() {
    let mut map = walled();
    let mut nav = octile();
    let (from, to) = ([0, 0, 0], [9, 0, 0]);
    let report = nav.find_path(
        &mut map,
        Cell::new(0, 0, 0),
        &[Cell::new(9, 0, 0)],
        Budget::UNLIMITED,
    );
    assert_eq!(report.status, NavStatus::Found);
    assert_eq!(
        path_cost(&mut nav, &mut map, from, to, Budget::UNLIMITED),
        Some(i128::from(report.cost))
    );

    // A blocked goal is not reachable.
    map.set(9, 0, DenseGrid::BLOCKED);
    assert_eq!(
        path_cost(&mut nav, &mut map, from, to, Budget::UNLIMITED),
        None
    );
    map.set(9, 0, 0);

    // One cell of work does not get across.
    let one = Budget::from(NonZeroUsize::new(1).unwrap());
    assert_eq!(path_cost(&mut nav, &mut map, from, to, one), None);
}

/// A crew whose one slot (max 1, for roles 1) goes to the leader's place
/// (6, 0): the leader 1 has no role, member 2 stands at (4, 0) behind the
/// wall, member 3 at (6, 6) on the open side.
fn crew() -> Crew {
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
    .unwrap();
    for (id, x, y, roles) in [(1, 6, 0, 0), (2, 4, 0, 1), (3, 6, 6, 1)] {
        let m = Member {
            id,
            pos: [x, y, 0],
            roles,
            skill: 255,
            state: MemberState::Active,
        };
        assert!(crew.add_member(m).is_some());
    }
    let sq = crew.squad_mut();
    assert!(sq.set_leader(1));
    sq.set_skill(255);
    sq.set_target_kind(1);
    let mut t = tactics::rush();
    t.slots[0].roles = 1;
    t.slots[0].min = 1;
    t.slots[0].max = 1;
    t.slots[0].place = sagax::decide::group::Place::Here;
    sq.add_tactic(t);
    crew.observe(
        0,
        Fact {
            subject: 10,
            kind: 1,
            pos: [9, 0, 0],
            seen_at: 0,
            ..Fact::default()
        },
    );
    crew
}

/// The members that got an order, over 13 ticks.
fn ordered(rules: Option<&mut dyn SquadRules>) -> Vec<u32> {
    let mut crew = crew();
    let mut events = Vec::new();
    let mut ids = Vec::new();
    let mut rules = rules;
    for t in 0..=12 {
        match rules.as_deref_mut() {
            Some(r) => crew.tick_with(t, &[], &[], SEED, r, &mut events).unwrap(),
            None => crew.tick(t, &[], &[], SEED, &mut events).unwrap(),
        }
        for e in &events {
            if let CrewEvent::Order { member, .. } = e {
                ids.push(*member);
            }
        }
    }
    ids
}

#[test]
fn path_cost_picks_walking_distance() {
    // As the crow flies member 2 is nearer (2 against 6).
    assert_eq!(ordered(None), [2]);

    let mut map = walled();
    let mut nav = octile();
    let mut rules = PathCost {
        nav: &mut nav,
        map: &mut map,
        budget: Budget::UNLIMITED,
    };
    // Walking, member 2 has to go round the wall.
    assert_eq!(ordered(Some(&mut rules)), [3]);
}
