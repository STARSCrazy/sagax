//! Walking distance ([`SquadRules::walk`]): where the squad and the crew
//! use it, unreachable as infinitely far, and no change without it.

use super::*;
use crate::memory::{Distance, Fact, Metric, Pos, distance};
use sagax_core::{HexLayout, NoopSink};

const ENEMY: u32 = 1;
const SEED: u64 = 7;

/// A walking distance from a closure, counting its questions.
struct Walk<F> {
    f: F,
    asked: usize,
}

fn walk<F: FnMut(Pos, Pos) -> Option<u32>>(f: F) -> Walk<F> {
    Walk { f, asked: 0 }
}

impl<F: FnMut(Pos, Pos) -> Option<u32>> Distance for Walk<F> {
    fn distance(&mut self, from: Pos, to: Pos) -> Option<u32> {
        self.asked += 1;
        (self.f)(from, to)
    }
}

fn cheb(a: Pos, b: Pos) -> u32 {
    let d = distance(
        Metric::Chebyshev,
        i64::from(b[0]) - i64::from(a[0]),
        i64::from(b[1]) - i64::from(a[1]),
    );
    u32::try_from(d).unwrap()
}

/// A wall at x = 5 for every y up to 8, open from y = 9 on: one side to
/// the other goes through the gap at (5, 9). A cell on the wall is not
/// reachable.
fn walled(a: Pos, b: Pos) -> Option<u32> {
    if a[0] == 5 && a[1] <= 8 || b[0] == 5 && b[1] <= 8 {
        return None;
    }
    let left = |p: Pos| p[0] < 5 && p[1] <= 8;
    let right = |p: Pos| p[0] > 5 && p[1] <= 8;
    let across = left(a) && right(b) || right(a) && left(b);
    Some(if across {
        let gap = [5, 9, 0];
        cheb(a, gap) + cheb(gap, b)
    } else {
        cheb(a, b)
    })
}

fn member(id: u32, x: i32, y: i32, roles: u32) -> Member {
    Member {
        id,
        pos: [x, y, 0],
        roles,
        skill: 255,
        state: MemberState::Active,
    }
}

fn enemy(id: u32, x: i32, y: i32, now: u32) -> Fact {
    Fact {
        subject: id,
        kind: ENEMY,
        pos: [x, y, 0],
        seen_at: now,
        ..Fact::default()
    }
}

fn config(metric: Metric) -> SquadConfig {
    SquadConfig {
        members: 8,
        memory: 8,
        areas: 4,
        inbox: 8,
        metric,
        ..SquadConfig::default()
    }
}

fn squad_of(metric: Metric, members: &[Member]) -> Squad {
    let mut sq = Squad::new(config(metric)).unwrap();
    for &m in members {
        assert!(sq.group_mut().add(m).is_some());
    }
    sq.set_target_kind(ENEMY);
    assert!(sq.set_leader(members[0].id));
    sq.set_skill(255);
    sq
}

/// The path-cost scene: leader 1 (no role) at (6, 0), member 2 at (4, 0)
/// behind the wall, member 3 at (6, 6) on the open side; one slot for one
/// member with role 1 at the leader's place; the target at (9, 0).
fn wall_squad() -> Squad {
    let mut sq = squad_of(
        Metric::Chebyshev,
        &[member(1, 6, 0, 0), member(2, 4, 0, 1), member(3, 6, 6, 1)],
    );
    let mut t = tactics::rush();
    t.slots[0].roles = 1;
    t.slots[0].min = 1;
    t.slots[0].max = 1;
    t.slots[0].place = Place::Here;
    sq.add_tactic(t);
    sq.knowledge_mut().learn(0, enemy(10, 9, 0, 0));
    sq
}

fn goal_members(sq: &Squad) -> Vec<u32> {
    sq.pending_plan()
        .unwrap()
        .goals
        .iter()
        .map(|(id, _)| *id)
        .collect()
}

#[test]
fn member_cost_walks_round_the_wall() {
    let mut sq = wall_squad();
    sq.plan(0, &[], &[], SEED).unwrap();
    // As the crow flies member 2 is nearer (2 against 6).
    assert_eq!(goal_members(&sq), [2]);

    let mut sq = wall_squad();
    let mut w = walk(walled);
    sq.plan_with(0, &[], &[], SEED, &mut Walking::new(&mut w))
        .unwrap();
    // Walking, member 2 has to go round the wall (9 + 9 = 18 against 6).
    assert_eq!(goal_members(&sq), [3]);
    assert!(w.asked > 0);
}

/// Member costs as the member cost slot sees them.
#[derive(Default)]
struct Costs(Vec<(u32, i128)>);

impl SquadRules for Costs {
    fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
        self.0.push((ctx.member.id, ctx.built_in));
        Some(ctx.built_in)
    }
}

#[test]
fn unreachable_member_comes_last_but_still_serves() {
    let shut_in = |a: Pos, b: Pos| {
        if a == [4, 0, 0] {
            None
        } else {
            Some(cheb(a, b))
        }
    };

    // Member 2 is nearer but cannot get there: member 3 goes.
    let mut sq = wall_squad();
    let mut costs = Costs::default();
    let mut w = walk(shut_in);
    sq.plan_with(0, &[], &[], SEED, &mut Walking::with(&mut costs, &mut w))
        .unwrap();
    assert_eq!(goal_members(&sq), [3]);
    assert_eq!(costs.0, [(2, UNREACHABLE), (3, 6)]);

    // Alone with the role, it still takes the slot: infinitely far is
    // last, not out.
    let mut sq = wall_squad();
    assert!(sq.group_mut().remove(3));
    let mut w = walk(shut_in);
    sq.plan_with(0, &[], &[], SEED, &mut Walking::new(&mut w))
        .unwrap();
    assert_eq!(goal_members(&sq), [2]);
}

#[test]
fn euclid_squares_the_walking_distance() {
    let mut sq = wall_squad();
    sq.metric = Metric::Euclid;
    let mut costs = Costs::default();
    let mut w = walk(|a, b| Some(cheb(a, b) + 1));
    sq.plan_with(0, &[], &[], SEED, &mut Walking::with(&mut costs, &mut w))
        .unwrap();
    // (2 + 1)² and (6 + 1)².
    assert_eq!(costs.0, [(2, 9), (3, 49)]);
}

#[test]
fn target_dist_is_the_mean_walk_of_the_members() {
    let sq = squad_of(
        Metric::Euclid,
        &[member(1, 0, 0, 0), member(2, 2, 0, 0), member(3, 4, 0, 0)],
    );
    let target = enemy(10, 9, 0, 0);
    let row_with = |rules: &mut dyn SquadRules| {
        let mut row = Vec::new();
        sq.row_into(
            rules,
            &mut row,
            0,
            [2, 0, 0],
            Some(&target),
            None,
            &[],
            false,
        );
        row
    };
    // Without walking: the centre's straight line.
    assert_eq!(row_with(&mut BuiltInRules)[IN_TARGET_DIST], 7);
    // Walking 10, 11 and (member 3) no way: the mean of those that can.
    let mut w = walk(|a: Pos, _| match a[0] {
        0 => Some(10),
        2 => Some(11),
        _ => None,
    });
    assert_eq!(row_with(&mut Walking::new(&mut w))[IN_TARGET_DIST], 10);
    // Nobody can get there: infinitely far.
    let mut w = walk(|_, _| None);
    let row = row_with(&mut Walking::new(&mut w));
    assert_eq!(row[IN_TARGET_DIST], i32::MAX);
    // Without another target the friend column stays -1.
    assert_eq!(row[IN_TARGET_FRIEND_DIST], -1);
}

#[test]
fn friend_dist_walks_and_unreachable_friends_are_infinitely_far() {
    let mut sq = squad_of(Metric::Chebyshev, &[member(1, 0, 0, 0)]);
    let target = enemy(10, 6, 0, 0);
    sq.knowledge_mut().learn(0, target);
    sq.knowledge_mut().learn(0, enemy(11, 4, 0, 0));
    let row_with = |sq: &Squad, rules: &mut dyn SquadRules| {
        let mut row = Vec::new();
        sq.row_into(
            rules,
            &mut row,
            0,
            [0, 0, 0],
            Some(&target),
            None,
            &[],
            false,
        );
        row[IN_TARGET_FRIEND_DIST]
    };
    assert_eq!(row_with(&sq, &mut BuiltInRules), 2);
    // Friend 11 has to go round the wall to reach target 10.
    let mut w = walk(walled);
    assert_eq!(row_with(&sq, &mut Walking::new(&mut w)), 9 + 9);
    // Asked from the friend to the target.
    let mut froms = Vec::new();
    let mut w = walk(|a, b| {
        froms.push((a, b));
        None
    });
    assert_eq!(row_with(&sq, &mut Walking::new(&mut w)), i32::MAX);
    assert!(froms.contains(&([4, 0, 0], [6, 0, 0])));
}

#[test]
fn between_picks_the_friend_by_walking() {
    // Target 10 at (6, 0); friend 11 at (4, 0) behind the wall, friend 12
    // at (6, 4) in the open. Members far off on the open side.
    let mk = || {
        let mut sq = squad_of(Metric::Chebyshev, &[member(1, 20, 0, 0)]);
        sq.add_tactic(Tactic {
            slots: vec![Slot {
                roles: 0,
                min: 1,
                max: 1,
                gather: None,
                place: Place::Between,
                kind: GoalKind::Move,
                start: Start::Now,
                want_tags: 0,
                dist: 0,
                gather_dist: 0,
            }],
            ..tactics::rush()
        });
        sq.knowledge_mut().learn(0, enemy(10, 6, 0, 0));
        sq.knowledge_mut().learn(0, enemy(11, 4, 0, 0));
        sq.knowledge_mut().learn(0, enemy(12, 6, 4, 0));
        sq
    };
    let goal_pos = |sq: &Squad| sq.pending_plan().unwrap().goals[0].1.pos;
    let mut by_line = mk();
    by_line.plan(0, &[], &[], SEED).unwrap();
    let mut by_walk = mk();
    let mut w = walk(walled);
    by_walk
        .plan_with(0, &[], &[], SEED, &mut Walking::new(&mut w))
        .unwrap();
    // The plan's target is 10 in both (the only one that is not a friend
    // of a nearer one does not matter: same tactic, first target).
    assert_eq!(by_line.pending_plan().unwrap().target, Some(10));
    assert_eq!(by_walk.pending_plan().unwrap().target, Some(10));
    // Between 10 and its friend: 11 by the line, 12 by walking.
    assert_eq!(goal_pos(&by_line), Some([5, 0, 0]));
    assert_eq!(goal_pos(&by_walk), Some([6, 2, 0]));
}

fn spot_ctx(wish: Pos) -> SpotCtx {
    SpotCtx {
        tactic: 0,
        slot: 0,
        gather: false,
        member: member(1, 0, 0, 0),
        wish,
        want_tags: 0,
        index: 0,
        spot: Spot { pos: wish, tags: 0 },
        target: None,
        built_in: 0,
    }
}

#[test]
fn spots_by_walking() {
    // Wish (6, 0). Spot 0 at (4, 0) behind the wall (2 straight, 18
    // walking), spot 1 at (6, 3), spot 2 on the wall at (5, 1).
    let spots = [
        Spot {
            pos: [4, 0, 0],
            tags: 0,
        },
        Spot {
            pos: [6, 3, 0],
            tags: 0,
        },
        Spot {
            pos: [5, 1, 0],
            tags: 0,
        },
    ];
    let ctx = spot_ctx([6, 0, 0]);
    let taken = [false; 3];
    let mut keys = Vec::new();
    let pick = |rules: &mut dyn SquadRules, keys: &mut Vec<(i128, usize, i128)>| {
        rank_spots_for_test(rules, keys, &ctx, &spots, &taken, (0, Metric::Chebyshev, 5))
    };
    // As the crow flies spot 2 is the nearest (1).
    assert_eq!(pick(&mut BuiltInRules, &mut keys), SpotPick::Spot(2));
    // Walking, spot 2 cannot be reached and spot 0 is beyond the radius.
    let mut w = walk(walled);
    assert_eq!(
        pick(&mut Walking::new(&mut w), &mut keys),
        SpotPick::Spot(1)
    );
    assert_eq!(keys.iter().map(|k| k.1).collect::<Vec<_>>(), [1]);
    // Nothing reachable: the wish point, unchecked.
    let mut w = walk(|_, _| None);
    assert_eq!(
        pick(&mut Walking::new(&mut w), &mut keys),
        SpotPick::Unchecked([6, 0, 0])
    );
}

fn rank_spots_for_test(
    rules: &mut dyn SquadRules,
    keys: &mut Vec<(i128, usize, i128)>,
    ctx: &SpotCtx,
    spots: &[Spot],
    taken: &[bool],
    at: (u8, Metric, i64),
) -> SpotPick {
    super::rules::rank_spots(rules, keys, ctx, spots, taken, at, &mut NoopSink)
}

#[test]
fn ambush_trigger_walks() {
    let run = |far: Option<u32>| {
        let mut sq = squad_of(
            Metric::Euclid,
            &[
                member(1, 0, 0, 0),
                member(2, 1, 0, 0),
                member(3, 0, 1, 0),
                member(4, 1, 1, 0),
            ],
        );
        sq.add_tactic(tactics::ambush());
        let close = [tactics::AMBUSH_RADIUS as i32, 0, 0];
        // On the line the target at the radius is in; walking it is `far`
        // away (the line elsewhere).
        let mut w = walk(
            move |a: Pos, b: Pos| {
                if b == close { far } else { Some(cheb(a, b)) }
            },
        );
        let mut rules = Walking::new(&mut w);
        sq.knowledge_mut().learn(0, enemy(10, 30, 0, 0));
        assert!(matches!(
            sq.check_with(0, &[], &[], 0, &mut rules),
            Ok(Status::Replan(_))
        ));
        assert_eq!(sq.check_with(5, &[], &[], 0, &mut rules), Ok(Status::Keep));
        sq.observe(6, enemy(10, close[0], 0, 6));
        sq.check_with(6, &[], &[], 0, &mut rules).unwrap()
    };
    assert_eq!(run(Some(6)), Status::Signal);
    assert_eq!(run(Some(40)), Status::Keep);
    assert_eq!(run(None), Status::Keep);
}

/// A crew with one on-signal slot at the leader's place for member 2;
/// the order is in by tick 10.
fn waiting_crew(hex: Option<HexLayout>, metric: Metric) -> Crew {
    let mut crew = Crew::new(CrewConfig {
        squad: config(metric),
        arrive_radius: 1,
        hex_layout: hex,
    })
    .unwrap();
    for m in [member(1, 6, 0, 0), member(2, 2, 2, 1)] {
        assert!(crew.add_member(m).is_some());
    }
    let sq = crew.squad_mut();
    assert!(sq.set_leader(1));
    sq.set_skill(255);
    sq.set_target_kind(ENEMY);
    let mut t = tactics::rush();
    t.slots[0].roles = 1;
    t.slots[0].max = 1;
    t.slots[0].place = Place::Here;
    t.slots[0].start = Start::OnSignal;
    sq.add_tactic(t);
    crew.observe(0, enemy(10, 9, 0, 0));
    let mut events = Vec::new();
    for t in 0..=10 {
        crew.tick(t, &[], &[], SEED, &mut events).unwrap();
    }
    assert!(crew.goal(2).is_some());
    crew
}

#[test]
fn crew_arrival_and_keeper_walk() {
    let arrived = |w: &mut dyn Distance| {
        let mut crew = waiting_crew(None, Metric::Chebyshev);
        let goal = crew.goal(2).unwrap().pos.unwrap();
        assert_eq!(goal, [6, 0, 0]);
        // One cell off the goal, the wall in between.
        assert!(crew.set_pos(2, [4, 0, 0]));
        let mut events = Vec::new();
        let mut rules = Walking::new(w);
        crew.tick_with(11, &[], &[], SEED, &mut rules, &mut events)
            .unwrap();
        let inputs = crew.member_inputs_with(2, 11, &mut rules).unwrap();
        (
            events.contains(&CrewEvent::Arrived { member: 2 }),
            inputs.dist,
        )
    };
    // The straight line (2 > radius 1): not there, distance 2.
    let mut line = walk(|a, b| Some(cheb(a, b)));
    assert_eq!(arrived(&mut line), (false, 2));
    // Walking round the wall.
    assert_eq!(arrived(&mut walk(walled)), (false, 18));
    // Walking it is one step: arrived.
    assert_eq!(arrived(&mut walk(|_, _| Some(1))), (true, 1));
    // No way: never arrived, infinitely far.
    assert_eq!(arrived(&mut walk(|_, _| None)), (false, i32::MAX));
    // Without walking the plain call is the metric.
    let crew = waiting_crew(None, Metric::Chebyshev);
    assert_eq!(
        crew.member_inputs(2, 11).unwrap(),
        crew.member_inputs_with(2, 11, &mut BuiltInRules).unwrap()
    );
}

#[test]
fn hex_crew_walks_in_offset_coordinates() {
    let layout = HexLayout::OddR;
    let mut crew = waiting_crew(Some(layout), Metric::Hex);
    let mut asked = Vec::new();
    let mut w = walk(|a, b| {
        asked.push((a, b));
        Some(3)
    });
    let mut events = Vec::new();
    crew.tick_with(11, &[], &[], SEED, &mut Walking::new(&mut w), &mut events)
        .unwrap();
    let inputs = crew
        .member_inputs_with(2, 11, &mut Walking::new(&mut w))
        .unwrap();
    assert_eq!(inputs.dist, 3);
    // Member 2 was added at offset (2, 2) and its goal is the leader's
    // offset (6, 0): the game hears offset coordinates.
    assert!(asked.contains(&([2, 2, 0], [6, 0, 0])), "{asked:?}");
}

/// The flank scene of the crew tests, with every distance asked of a
/// provider that answers the metric's straight line: the same plan, the
/// same events. (No tactic reads [`IN_TARGET_DIST`]: walking it is the
/// members' mean, not the centre's distance.)
#[test]
fn walking_like_the_metric_changes_nothing() {
    let mk = || {
        let mut crew = Crew::new(CrewConfig {
            squad: config(Metric::Chebyshev),
            arrive_radius: 1,
            ..CrewConfig::default()
        })
        .unwrap();
        for (id, x, y) in [(1, 0, 0), (2, 0, 2), (3, 0, -2), (4, -3, 0)] {
            assert!(crew.add_member(member(id, x, y, 0)).is_some());
        }
        let sq = crew.squad_mut();
        assert!(sq.set_leader(1));
        sq.set_skill(192);
        sq.set_target_kind(ENEMY);
        sq.add_tactic(tactics::rush());
        sq.add_tactic(tactics::formation(Shape::Line, 2));
        sq.add_tactic(tactics::flank(3, 4));
        sq.add_tactic(tactics::isolate());
        crew.observe(0, enemy(10, 20, 0, 0));
        crew.observe(0, enemy(11, 24, 10, 0));
        crew
    };
    let spots = [
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
            tags: TAG_HIDDEN,
        },
    ];
    let mut plain = mk();
    let mut walking = mk();
    let mut w = walk(|a, b| Some(cheb(a, b)));
    let (mut e1, mut e2) = (Vec::new(), Vec::new());
    let mut all = 0;
    for t in 0..=80 {
        if t % 7 == 0 {
            // Members walk a step towards x = 20.
            for id in 1..=4 {
                let p = plain
                    .squad()
                    .group()
                    .iter()
                    .find(|m| m.id == id)
                    .unwrap()
                    .pos;
                let next = [p[0] + 1, p[1], 0];
                assert!(plain.set_pos(id, next));
                assert!(walking.set_pos(id, next));
            }
        }
        plain.tick(t, &[], &spots, SEED, &mut e1).unwrap();
        walking
            .tick_with(t, &[], &spots, SEED, &mut Walking::new(&mut w), &mut e2)
            .unwrap();
        assert_eq!(e1, e2, "tick {t}");
        all += e1.len();
        for id in 1..=4 {
            assert_eq!(
                plain.member_inputs(id, t),
                walking.member_inputs_with(id, t, &mut Walking::new(&mut w)),
            );
        }
    }
    assert!(all > 4, "the scene should do something");
    assert!(w.asked > 0);
}
