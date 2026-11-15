//! The rules object: built-in rules change nothing, and each plan-side slot
//! does what it says, in the documented order.

use super::*;
use crate::memory::{Area, Fact, Message, Metric, Pos, ZeroCapacity};
use crate::{Consideration, Curve, DecideTrace, ONE, mix};
use sagax_debug::{Collector, Overflow};

/// A collector for squad traces.
fn log() -> Collector<SquadTrace> {
    Collector::new(4096, Overflow::DropNewest)
}

/// The decider records of a trace: (tactic, target, record).
fn decides(c: &Collector<SquadTrace>) -> Vec<(usize, Option<u32>, DecideTrace)> {
    c.records()
        .filter_map(|r| match r.event {
            SquadTrace::Decide {
                tactic,
                target,
                event,
            } => Some((tactic, target, event)),
            _ => None,
        })
        .collect()
}

/// Fact kind of the enemies in these tests.
const ENEMY: u32 = 1;

fn squad_with(config: SquadConfig, members: &[(u32, i32, i32, u32)]) -> Squad {
    let mut sq = Squad::new(config).unwrap();
    for &(id, x, y, roles) in members {
        sq.group_mut().add(Member {
            id,
            pos: [x, y, 0],
            roles,
            skill: 0,
            state: MemberState::Active,
        });
    }
    sq.set_target_kind(ENEMY);
    if let Some(&(id, ..)) = members.first() {
        assert!(sq.set_leader(id));
    }
    sq
}

fn squad_of(metric: Metric, members: &[(u32, i32, i32, u32)]) -> Squad {
    squad_with(
        SquadConfig {
            members: 8,
            memory: 8,
            areas: 4,
            inbox: 8,
            metric,
            ..SquadConfig::default()
        },
        members,
    )
}

fn see(sq: &mut Squad, id: u32, x: i32, y: i32, now: u32) {
    sq.knowledge_mut().learn(
        now,
        Fact {
            subject: id,
            kind: ENEMY,
            pos: [x, y, 0],
            seen_at: now,
            ..Fact::default()
        },
    );
}

fn four() -> [(u32, i32, i32, u32); 4] {
    [(1, 0, 0, 0), (2, 1, 0, 0), (3, 0, 1, 0), (4, 1, 1, 0)]
}

fn spot(x: i32, y: i32, tags: u32) -> Spot {
    Spot {
        pos: [x, y, 0],
        tags,
    }
}

/// Counts every slot call and returns every default.
#[derive(Default)]
struct Counting {
    keep: u32,
    columns: u32,
    place: u32,
    cost: u32,
    spot: u32,
    later: u32,
}

impl SquadRules for Counting {
    fn keep_target(&mut self, _: &SquadView<'_>, _: &Fact) -> bool {
        self.keep += 1;
        true
    }
    fn columns(&mut self, _: &SquadView<'_>, _: &RowCtx<'_>, _: &mut [i32]) {
        self.columns += 1;
    }
    fn place(&mut self, _: u32, ctx: &PlaceCtx) -> Pos {
        self.place += 1;
        ctx.leader
    }
    fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
        self.cost += 1;
        Some(ctx.built_in)
    }
    fn spot_score(&mut self, ctx: &SpotCtx) -> Option<i128> {
        self.spot += 1;
        Some(ctx.built_in)
    }
    fn trigger(&mut self, _: u32, _: &SquadView<'_>, _: u32) -> bool {
        self.later += 1;
        false
    }
    fn abort(&mut self, _: u32, _: &SquadView<'_>, _: &RowCtx<'_>) -> bool {
        self.later += 1;
        false
    }
    fn channel_delay(&mut self, _: u32, _: &ChannelCtx) -> Option<u32> {
        self.later += 1;
        None
    }
    fn report(&mut self, _: &SquadView<'_>, _: &Message, _: u32, _: &mut ReportActions<'_>) {
        self.later += 1;
    }
}

/// The scenes of the squad tests: `(squad, extra, spots)`, with rush as a
/// fallback so that every skill finds a plan.
fn scene(k: usize, skill: u8) -> (Squad, Vec<i32>, Vec<Spot>) {
    let mut sq = squad_of(Metric::Euclid, &four());
    let mut extra = Vec::new();
    let mut spots = vec![
        spot(10, 3, TAG_COVER),
        spot(-6, 0, TAG_COVER),
        spot(2, 2, TAG_HIDDEN),
        spot(3, -1, TAG_HIDDEN),
        spot(20, 14, TAG_CHOKE),
        spot(16, 4, 0),
        spot(24, -4, 0),
    ];
    match k {
        0 => {
            sq.add_tactic(tactics::formation(Shape::Wedge, 2));
            see(&mut sq, 10, 20, 0, 0);
        }
        1 => {
            sq.add_tactic(tactics::flank(3, 4));
            see(&mut sq, 10, 20, 0, 0);
            see(&mut sq, 11, 22, 6, 0);
        }
        2 => {
            let mut iso = tactics::isolate();
            iso.considerations.push(Consideration::new(
                sq.extra_start() as u32,
                Curve::Linear,
                0,
                50,
            ));
            sq.add_tactic(iso);
            assert!(sq.set_extra_columns(1));
            see(&mut sq, 10, 20, 0, 0);
            see(&mut sq, 11, 22, 0, 0);
            see(&mut sq, 12, 20, 30, 0);
            extra = vec![0, 2, 2, 30];
        }
        3 => {
            sq.add_tactic(tactics::fall_back());
            sq.group_mut().set_gone(4);
            sq.areas_mut().add(
                0,
                Area {
                    pos: [20, 0, 0],
                    radius: 3,
                    lifetime: 100,
                    ..Area::default()
                },
            );
        }
        _ => {
            sq.add_tactic(tactics::ambush());
            see(&mut sq, 10, 60, 0, 0);
            spots.push(spot(1, 1, TAG_HIDDEN));
        }
    }
    sq.add_tactic(tactics::rush());
    sq.set_skill(skill);
    (sq, extra, spots)
}

/// Changes during the 30 checks: the target moves, members are lost.
fn step(sq: &mut Squad, now: u32) {
    if now == 8 {
        see(sq, 10, 12, 4, now);
    }
    if now == 15 {
        sq.group_mut().set_gone(3);
    }
    if now == 18 {
        sq.group_mut().set_gone(2);
    }
    if now == 22 {
        see(sq, 10, 4, 0, now);
    }
}

#[test]
fn builtin_rules_change_nothing() {
    let mut total = Counting::default();
    for k in 0..5 {
        for skill in [0, 255] {
            let (mut a, extra, spots) = scene(k, skill);
            let (mut b, mut c) = (a.clone(), a.clone());
            let mut counting = Counting::default();
            let pa = a.plan(0, &extra, &spots, 7);
            let pb = b.plan_with(0, &extra, &spots, 7, &mut BuiltInRules);
            let pc = c.plan_with(0, &extra, &spots, 7, &mut counting);
            assert_eq!(pa, pb, "scene {k} skill {skill}");
            assert_eq!(pa, pc, "scene {k} skill {skill}");
            assert!(pa.unwrap().is_some(), "scene {k} skill {skill}");
            assert_eq!(a.pending_plan(), b.pending_plan());
            assert_eq!(a.pending_plan(), c.pending_plan());
            for now in 1..=30 {
                for sq in [&mut a, &mut b, &mut c] {
                    step(sq, now);
                }
                let sa = a.check(now, &extra, &spots, 7 + u64::from(now));
                let sb = b.check_with(now, &extra, &spots, 7 + u64::from(now), &mut BuiltInRules);
                let sc = c.check_with(now, &extra, &spots, 7 + u64::from(now), &mut counting);
                assert_eq!(sa, sb, "scene {k} skill {skill} tick {now}");
                assert_eq!(sa, sc, "scene {k} skill {skill} tick {now}");
                assert_eq!(a.pending_plan(), b.pending_plan());
                assert_eq!(a.pending_plan(), c.pending_plan());
                assert_eq!(a.active_plan(), b.active_plan());
                assert_eq!(a.active_plan(), c.active_plan());
            }
            total.keep += counting.keep;
            total.columns += counting.columns;
            total.place += counting.place;
            total.cost += counting.cost;
            total.spot += counting.spot;
            total.later += counting.later;
        }
    }
    // The slots were really called (no custom place, nothing wired later).
    assert!(total.keep > 0 && total.columns > 0 && total.cost > 0 && total.spot > 0);
    assert_eq!((total.place, total.later), (0, 0));
}

#[test]
fn spot_score_order_matches_pick_spot() {
    let metrics = [
        Metric::Euclid,
        Metric::Manhattan,
        Metric::Chebyshev,
        Metric::Hex,
    ];
    let mut keys = Vec::new();
    let mut found = 0;
    for seed in 1..=2000u64 {
        let mut r = mix(seed);
        let mut next = |n: u64| {
            r = mix(r);
            r % n
        };
        let wish = [next(41) as i32 - 20, next(41) as i32 - 20, 0];
        let n = next(13) as usize;
        let mut spots = Vec::new();
        let mut taken = Vec::new();
        for _ in 0..n {
            let (x, y) = (next(41) as i32 - 20, next(41) as i32 - 20);
            spots.push(spot(x, y, next(16) as u32));
            taken.push(next(4) == 0);
        }
        let want = next(16) as u32;
        let skip = next(4) as u8;
        let metric = metrics[next(4) as usize];
        let radius = next(31) as i64;
        let mut plain = Collector::new(64, Overflow::DropNewest);
        let expected =
            pick_spot_traced(wish, want, &spots, &taken, skip, metric, radius, &mut plain);
        assert_eq!(
            expected,
            pick_spot(wish, want, &spots, &taken, skip, metric, radius)
        );
        let ctx = SpotCtx {
            tactic: 0,
            slot: 0,
            gather: false,
            member: Member {
                id: 1,
                pos: [0; 3],
                roles: 0,
                skill: 0,
                state: MemberState::Active,
            },
            wish,
            want_tags: want,
            index: 0,
            spot: spot(0, 0, 0),
            target: None,
            built_in: 0,
        };
        let mut ranked = Collector::new(64, Overflow::DropNewest);
        let got = rules::rank_spots(
            &mut BuiltInRules,
            &mut keys,
            &ctx,
            &spots,
            &taken,
            (skip, metric, radius),
            &mut ranked,
        );
        assert_eq!(got, expected, "seed {seed}");
        // The same ranking, spot by spot, ending in the pick.
        let trace = |c: &Collector<SpotTrace>| c.records().map(|r| r.event).collect::<Vec<_>>();
        assert_eq!(trace(&ranked), trace(&plain), "seed {seed}");
        assert_eq!(trace(&plain).last(), Some(&SpotTrace::Picked(expected)));
        found += usize::from(matches!(got, SpotPick::Spot(_)));
    }
    // Both outcomes are common.
    assert!(found > 500 && found < 1900, "{found}");
}

/// Keeps only the targets with these subjects.
struct KeepOnly(&'static [u32]);

impl SquadRules for KeepOnly {
    fn keep_target(&mut self, _: &SquadView<'_>, fact: &Fact) -> bool {
        self.0.contains(&fact.subject)
    }
}

#[test]
fn targets_slot_drops_candidates() {
    let mut sq = squad_of(Metric::Chebyshev, &four());
    let x = sq.extra_start() as u32;
    let mut rush = tactics::rush();
    rush.considerations = vec![
        Consideration::new(x, Curve::Linear, 0, 9),
        Consideration::new(IN_TARGET_FRIEND_DIST as u32, Curve::Linear, 0, 100),
    ];
    let t = sq.add_tactic(rush);
    assert!(sq.set_extra_columns(1));
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    see(&mut sq, 11, 23, 0, 0);
    let mut trace = log();
    sq.plan_traced(0, &[0, 5, 9], &[], 0, &mut KeepOnly(&[11]), &mut trace)
        .unwrap()
        .unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!((p.tactic, p.target), (t, Some(11)));
    // Only 11 was scored; it read its own block (2) and its dropped friend.
    let inputs: Vec<(Option<u32>, i32)> = decides(&trace)
        .into_iter()
        .filter_map(|(_, target, e)| match e {
            DecideTrace::Factor { input, .. } => Some((target, input)),
            _ => None,
        })
        .collect();
    assert_eq!(inputs, [(Some(11), 9), (Some(11), 3)]);
}

#[test]
fn targets_slot_keeps_active_pair() {
    let mut sq = squad_of(Metric::Chebyshev, &four());
    sq.add_tactic(tactics::rush());
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    see(&mut sq, 11, 20, 2, 0);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().target, Some(10));
    assert_eq!(sq.check(10, &[], &[], 0), Ok(Status::Keep));
    assert_eq!(sq.active_plan().unwrap().target, Some(10));
    // Dropping the active target does not end the plan: the pair stays a
    // candidate, and 11 is not clearly better.
    let status = sq.check_with(11, &[], &[], 0, &mut KeepOnly(&[11]));
    assert_eq!(status, Ok(Status::Keep));
    assert!(sq.pending_plan().is_none());
}

/// Writes fixed rule columns and records the row lengths it saw.
struct Columns {
    values: [i32; 2],
    seen: Vec<(usize, usize)>,
}

impl SquadRules for Columns {
    fn columns(&mut self, _: &SquadView<'_>, ctx: &RowCtx<'_>, out: &mut [i32]) {
        self.seen.push((ctx.row.len(), out.len()));
        assert!(out.iter().all(|&v| v == 0));
        out.copy_from_slice(&self.values);
    }
}

#[test]
fn columns_slot_fills_after_extra() {
    let mut sq = squad_with(
        SquadConfig {
            rule_columns: 2,
            ..SquadConfig::default()
        },
        &four(),
    );
    sq.set_target_kind(ENEMY);
    assert!(sq.set_extra_columns(1));
    let start = sq.rule_start();
    assert_eq!(start, sq.extra_start() + 1);
    let mut rush = tactics::rush();
    rush.considerations = vec![Consideration::new(start as u32 + 1, Curve::Linear, 0, 9)];
    sq.add_tactic(rush);
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    let mut rules = Columns {
        values: [7, 9],
        seen: Vec::new(),
    };
    let mut trace = log();
    sq.plan_traced(0, &[0, 3], &[], 0, &mut rules, &mut trace)
        .unwrap()
        .unwrap();
    assert_eq!(rules.seen, [(start, 2)]);
    let factors: Vec<(i32, u32)> = decides(&trace)
        .into_iter()
        .filter_map(|(_, _, e)| match e {
            DecideTrace::Factor { input, value, .. } => Some((input, value)),
            _ => None,
        })
        .collect();
    assert_eq!(factors, [(9, ONE)]);
}

/// Records the place calls and answers with a fixed point.
struct PlaceAt {
    at: Pos,
    seen: Vec<(u32, PlaceCtx)>,
}

impl SquadRules for PlaceAt {
    fn place(&mut self, id: u32, ctx: &PlaceCtx) -> Pos {
        self.seen.push((id, *ctx));
        self.at
    }
}

fn custom_tactic(place: Place) -> Tactic {
    let mut t = tactics::rush();
    t.slots[0].place = place;
    t.slots[0].max = 1;
    t
}

#[test]
fn place_custom_gets_ctx() {
    let mut sq = squad_of(Metric::Chebyshev, &[(1, 2, 0, 0), (2, 0, 0, 0)]);
    sq.add_tactic(custom_tactic(Place::Custom(5)));
    sq.set_skill(255);
    see(&mut sq, 10, 10, 0, 0);
    let mut rules = PlaceAt {
        at: [3, 4, 0],
        seen: Vec::new(),
    };
    sq.plan_with(0, &[], &[], 0, &mut rules).unwrap().unwrap();
    assert_eq!(rules.seen.len(), 1);
    let (id, ctx) = rules.seen[0];
    assert_eq!(id, 5);
    assert_eq!(ctx.leader, [2, 0, 0]);
    assert_eq!(ctx.centre, [1, 0, 0]);
    assert_eq!(ctx.face, Some([10, 0, 0]));
    assert_eq!(ctx.target, Some(10));
    assert_eq!((ctx.tactic, ctx.slot, ctx.gather), (0, 0, false));
    let g = sq.pending_plan().unwrap().goals[0].1;
    assert_eq!(g.pos, Some([3, 4, 0]));
    assert!(g.unchecked);
}

/// Member costs from a table `(slot, member id) -> cost`.
struct Costs(fn(usize, u32, i128) -> Option<i128>);

impl SquadRules for Costs {
    fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
        (self.0)(ctx.slot, ctx.member.id, ctx.built_in)
    }
}

#[test]
fn member_cost_swaps_assignment() {
    let mut sq = squad_of(Metric::Chebyshev, &[(1, 0, 0, 0), (2, 5, 0, 0)]);
    let mut t = custom_tactic(Place::Here);
    t.slots[0].min = 1;
    sq.add_tactic(t);
    sq.set_skill(255);
    see(&mut sq, 10, 10, 0, 0);
    let members = |sq: &Squad| -> Vec<u32> {
        sq.pending_plan()
            .unwrap()
            .goals
            .iter()
            .map(|(m, _)| *m)
            .collect()
    };
    sq.plan_with(0, &[], &[], 0, &mut BuiltInRules)
        .unwrap()
        .unwrap();
    assert_eq!(members(&sq), [1]);
    sq.plan_with(0, &[], &[], 0, &mut Costs(|_, _, d| Some(-d)))
        .unwrap()
        .unwrap();
    assert_eq!(members(&sq), [2]);
    // Nobody may take the slot: the plan is still made, without goals.
    sq.plan_with(0, &[], &[], 0, &mut Costs(|_, _, _| None))
        .unwrap()
        .unwrap();
    assert_eq!(members(&sq), Vec::<u32>::new());
}

/// Spot scores: the nearer spot (index 0) gets a penalty, or no spot is
/// usable.
struct Scores(bool);

impl SquadRules for Scores {
    fn spot_score(&mut self, ctx: &SpotCtx) -> Option<i128> {
        match (self.0, ctx.index) {
            (true, 0) => Some(ctx.built_in + 1000),
            (true, _) => Some(ctx.built_in),
            (false, _) => None,
        }
    }
}

#[test]
fn spot_score_prefers_hidden() {
    let mut sq = squad_of(Metric::Chebyshev, &[(1, 0, 0, 0)]);
    sq.add_tactic(custom_tactic(Place::Here));
    sq.set_skill(255);
    sq.set_spot_radius(Some(10));
    see(&mut sq, 10, 10, 0, 0);
    let spots = [spot(1, 0, 0), spot(4, 0, 0)];
    let goal = |sq: &Squad| sq.pending_plan().unwrap().goals[0].1;
    sq.plan_with(0, &[], &spots, 0, &mut BuiltInRules)
        .unwrap()
        .unwrap();
    assert_eq!(goal(&sq).pos, Some([1, 0, 0]));
    sq.plan_with(0, &[], &spots, 0, &mut Scores(true))
        .unwrap()
        .unwrap();
    assert_eq!(goal(&sq).pos, Some([4, 0, 0]));
    assert!(!goal(&sq).unchecked);
    sq.plan_with(0, &[], &spots, 0, &mut Scores(false))
        .unwrap()
        .unwrap();
    assert_eq!(goal(&sq).pos, Some([0, 0, 0]));
    assert!(goal(&sq).unchecked);
}

/// One slot call, as [`Recorder`] logs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Call {
    Keep(u32),
    Columns(usize, Option<u32>),
    Place(u32, usize, bool),
    Cost(usize, u32),
    Spot(u32, bool, usize),
}

#[derive(Default)]
struct Recorder(Vec<Call>);

impl SquadRules for Recorder {
    fn keep_target(&mut self, _: &SquadView<'_>, fact: &Fact) -> bool {
        self.0.push(Call::Keep(fact.subject));
        true
    }
    fn columns(&mut self, _: &SquadView<'_>, ctx: &RowCtx<'_>, _: &mut [i32]) {
        self.0
            .push(Call::Columns(ctx.tactic, ctx.target.map(|f| f.subject)));
    }
    fn place(&mut self, id: u32, ctx: &PlaceCtx) -> Pos {
        self.0.push(Call::Place(id, ctx.slot, ctx.gather));
        match id {
            1 => [0, 5, 0],
            2 => [0, 6, 0],
            _ => [1, -5, 0],
        }
    }
    fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
        self.0.push(Call::Cost(ctx.slot, ctx.member.id));
        Some(ctx.built_in)
    }
    fn spot_score(&mut self, ctx: &SpotCtx) -> Option<i128> {
        self.0
            .push(Call::Spot(ctx.member.id, ctx.gather, ctx.index));
        Some(ctx.built_in)
    }
}

#[test]
fn slot_call_order_is_fixed() {
    use Call as C;
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0), (2, 1, 0, 0)]);
    let mut two = tactics::rush();
    two.weight = ONE;
    two.slots = vec![
        Slot {
            gather: Some(Place::Custom(1)),
            place: Place::Custom(2),
            max: 1,
            ..two.slots[0]
        },
        Slot {
            place: Place::Custom(3),
            max: 1,
            ..two.slots[0]
        },
    ];
    sq.add_tactic(two);
    sq.add_tactic(tactics::rush());
    sq.set_skill(255);
    sq.set_spot_radius(Some(100));
    see(&mut sq, 10, 20, 0, 0);
    see(&mut sq, 11, 20, 4, 0);
    let spots = [spot(0, 5, 0), spot(1, -5, 0)];
    let mut rec = Recorder::default();
    sq.plan_with(0, &[], &spots, 0, &mut rec).unwrap().unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!((p.tactic, p.target), (0, Some(10)));
    assert_eq!(
        rec.0,
        [
            C::Keep(10),
            C::Keep(11),
            C::Columns(0, Some(10)),
            C::Columns(0, Some(11)),
            C::Columns(1, Some(10)),
            C::Columns(1, Some(11)),
            C::Place(1, 0, true),
            C::Place(2, 0, false),
            C::Place(3, 1, false),
            C::Cost(0, 1),
            C::Cost(0, 2),
            C::Cost(1, 1),
            C::Cost(1, 2),
            C::Spot(1, true, 0),
            C::Spot(1, true, 1),
            C::Spot(1, false, 0),
            C::Spot(1, false, 1),
            C::Spot(2, false, 1),
        ]
    );
}

#[test]
fn rule_columns_validate() {
    let config = SquadConfig {
        rule_columns: 2,
        ..SquadConfig::default()
    };
    let mut sq = squad_with(config, &four());
    let start = sq.rule_start() as u32;
    let mut t = tactics::rush();
    t.considerations = vec![Consideration::new(start + 1, Curve::Linear, 0, 9)];
    sq.add_tactic(t.clone());
    assert_eq!(sq.validate(), Ok(()));
    t.considerations = vec![Consideration::new(start + 2, Curve::Linear, 0, 9)];
    sq.add_tactic(t);
    assert_eq!(
        sq.validate(),
        Err(PlanError::InputOutOfRange {
            tactic: 1,
            input: start + 2
        })
    );
    let too_many = SquadConfig {
        rule_columns: MAX_EXTRA_COLUMNS + 1,
        ..SquadConfig::default()
    };
    assert_eq!(Squad::new(too_many).err(), Some(ZeroCapacity));
}

#[test]
fn needs_rules_for_custom_place() {
    let mut sq = squad_of(Metric::Chebyshev, &four());
    sq.add_tactic(tactics::rush());
    assert!(!sq.needs_rules());
    let gather = Tactic {
        slots: vec![Slot {
            gather: Some(Place::Custom(1)),
            ..tactics::rush().slots[0]
        }],
        ..tactics::rush()
    };
    let mut g = squad_of(Metric::Chebyshev, &four());
    g.add_tactic(gather);
    assert!(g.needs_rules());
    sq.add_tactic(custom_tactic(Place::Custom(5)));
    assert!(sq.needs_rules());
    see(&mut sq, 10, 10, 0, 0);
    assert_eq!(sq.plan(0, &[], &[], 0), Err(PlanError::NeedsRules));
    assert_eq!(sq.check(0, &[], &[], 0), Err(PlanError::NeedsRules));
    assert!(sq.pending_plan().is_none());
    assert!(sq.plan_with(0, &[], &[], 0, &mut BuiltInRules).is_ok());
    // Rule columns and custom aborts need rules too.
    let cols = squad_with(
        SquadConfig {
            rule_columns: 1,
            ..SquadConfig::default()
        },
        &four(),
    );
    assert!(cols.needs_rules());
    let mut ab = squad_of(Metric::Chebyshev, &four());
    ab.add_tactic(Tactic {
        aborts: vec![Abort::Custom(3)],
        ..tactics::rush()
    });
    assert!(ab.needs_rules());
}

#[test]
fn scratch_does_not_grow_after_warm_up() {
    let (mut sq, extra, spots) = scene(1, 255);
    let mut rules = Counting::default();
    let caps = |sq: &Squad| {
        (
            sq.scratch.kept.capacity(),
            sq.scratch.costs.capacity(),
            sq.scratch.spot_keys.capacity(),
        )
    };
    let mut after_two = None;
    for n in 1..=50u32 {
        // Move the targets so that the squad keeps replanning.
        see(&mut sq, 10, 20 - (n % 7) as i32, 0, n);
        see(&mut sq, 11, 22, (n % 5) as i32, n);
        sq.plan_with(n, &extra, &spots, u64::from(n), &mut rules)
            .unwrap();
        sq.check_with(n, &extra, &spots, u64::from(n), &mut rules)
            .unwrap();
        if n == 2 {
            after_two = Some(caps(&sq));
            assert!(caps(&sq).0 > 0 && caps(&sq).1 > 0 && caps(&sq).2 > 0);
        }
    }
    assert_eq!(Some(caps(&sq)), after_two);
}

#[test]
fn view_with_hex_converts_positions() {
    use sagax_core::HexLayout;
    let mut sq = squad_of(Metric::Hex, &[(1, 2, 3, 0)]);
    see(&mut sq, 10, 4, 5, 0);
    let layout = HexLayout::OddR;
    let view = SquadView::new(&sq);
    let hex = view.with_hex(layout);
    assert_eq!(view.member(1).unwrap().pos, [2, 3, 0]);
    assert_eq!(hex.member(1).unwrap().pos, layout.from_axial_pos([2, 3, 0]));
    assert_eq!(
        hex.targets().next().unwrap().pos,
        layout.from_axial_pos([4, 5, 0])
    );
    assert_eq!(hex.centre(), Some(layout.from_axial_pos([2, 3, 0])));
    assert_eq!(hex.leader(), Some(1));
    assert_eq!(hex.metric(), Metric::Hex);
}

// --- Check-side slots: trigger, abort, channel, report ---

/// Members on the x axis (the same distances on a square and a hex grid).
fn line_squad(metric: Metric, xs: &[i32]) -> Squad {
    let members: Vec<(u32, i32, i32, u32)> = xs
        .iter()
        .enumerate()
        .map(|(i, &x)| (i as u32 + 1, x, 0, 0))
        .collect();
    let mut sq = squad_of(metric, &members);
    sq.set_skill(255);
    sq
}

/// The orders due at `now`, as `(member, now)`.
fn due(sq: &mut Squad, now: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    sq.orders_due(now, &mut out);
    out.iter().map(|o| (o.member, now)).collect()
}

/// Fires the custom trigger from tick `from` on; records what it saw.
struct FireFrom {
    from: u32,
    seen: Vec<(u32, u32, Option<PlanInfo>)>,
}

impl SquadRules for FireFrom {
    fn trigger(&mut self, id: u32, view: &SquadView<'_>, now: u32) -> bool {
        self.seen.push((id, now, view.active_plan()));
        now >= self.from
    }
}

/// The trigger scene: statuses from tick 1 to 42.
fn trigger_events(metric: Metric) -> (Vec<Status>, FireFrom, Squad) {
    let mut sq = line_squad(metric, &[0, 2, 4]);
    sq.add_tactic(Tactic {
        trigger: Trigger::Custom(3),
        ..tactics::rush()
    });
    see(&mut sq, 10, 20, 0, 0);
    let mut rules = FireFrom {
        from: 40,
        seen: Vec::new(),
    };
    sq.plan_with(0, &[], &[], 0, &mut rules).unwrap().unwrap();
    let statuses = (1..=42)
        .map(|now| sq.check_with(now, &[], &[], 0, &mut rules).unwrap())
        .collect();
    (statuses, rules, sq)
}

#[test]
fn trigger_custom_fires_signal() {
    let (statuses, rules, sq) = trigger_events(Metric::Chebyshev);
    assert!(sq.needs_rules());
    assert_eq!(statuses[38], Status::Keep, "tick 39");
    assert_eq!(statuses[39], Status::Signal, "tick 40");
    assert_eq!(statuses[40], Status::Keep, "tick 41");
    assert!(statuses[..39].iter().all(|&s| s == Status::Keep));
    assert!(sq.signal_given());
    // Asked from activation (tick 5) to the signal, never after it.
    assert_eq!(rules.seen.len(), 36);
    let (id, now, plan) = *rules.seen.last().unwrap();
    assert_eq!((id, now), (3, 40));
    let plan = plan.unwrap();
    assert_eq!(plan.id, sq.active_plan().unwrap().id);
    assert_eq!((plan.tactic, plan.target, plan.since), (0, Some(10), 5));
    assert!(!plan.signal_given);
}

/// Fires custom abort 1 from tick `from` on; records `(id, tactic, target,
/// active, now)` of every call.
struct AbortFrom {
    from: u32,
    seen: Vec<(u32, usize, Option<u32>, bool, u32)>,
}

impl SquadRules for AbortFrom {
    fn abort(&mut self, id: u32, _: &SquadView<'_>, ctx: &RowCtx<'_>) -> bool {
        self.seen.push((
            id,
            ctx.tactic,
            ctx.target.map(|f| f.subject),
            ctx.active,
            ctx.now,
        ));
        id == 1 && ctx.now >= self.from
    }
}

/// The abort scene: ambush (with only `Abort::Custom(1)`) against rush,
/// two targets; statuses from tick 1 to 30.
fn abort_events(metric: Metric) -> (Vec<Status>, AbortFrom, Squad, Collector<SquadTrace>) {
    let mut sq = line_squad(metric, &[0, 2, 4]);
    sq.add_tactic(Tactic {
        aborts: vec![Abort::Custom(1)],
        ..tactics::ambush()
    });
    sq.add_tactic(tactics::rush());
    see(&mut sq, 10, 60, 0, 0);
    see(&mut sq, 11, 50, 0, 0);
    let mut rules = AbortFrom {
        from: 30,
        seen: Vec::new(),
    };
    sq.plan_with(0, &[], &[], 0, &mut rules).unwrap().unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!((p.tactic, p.target), (0, Some(10)));
    let mut trace = log();
    let statuses = (1..=30)
        .map(|now| {
            trace.clear();
            sq.check_traced(now, &[], &[], 0, &mut rules, &mut trace)
                .unwrap()
        })
        .collect();
    (statuses, rules, sq, trace)
}

#[test]
fn abort_custom_replans() {
    let (statuses, rules, sq, trace) = abort_events(Metric::Chebyshev);
    assert!(sq.needs_rules());
    assert!(statuses[..29].iter().all(|&s| s == Status::Keep));
    let Status::Replan(id) = statuses[29] else {
        panic!("tick 30: {:?}", statuses[29]);
    };
    let p = sq.pending_plan().unwrap();
    assert_eq!((p.id, p.tactic), (id, 1));
    let at_30: Vec<_> = rules.seen.iter().filter(|c| c.4 == 30).copied().collect();
    // First the active row (in the check), then the candidates of the
    // replan: the active pair is still the active row, the other is not.
    assert_eq!(
        at_30,
        [
            (1, 0, Some(10), true, 30),
            (1, 0, Some(10), true, 30),
            (1, 0, Some(11), false, 30),
        ]
    );
    // Before tick 30 the other candidate was asked as a candidate.
    assert!(rules.seen.contains(&(1, 0, Some(11), false, 0)));
    // The ambush candidates scored 0 while the abort fired: only rush was
    // scored, and the trace says why.
    let scored = decides(&trace);
    assert!(!scored.is_empty());
    assert!(scored.iter().all(|e| e.0 == 1));
    let ambush = |fired| SquadTrace::Abort {
        tactic: 0,
        target: Some(10),
        abort: 0,
        fired,
    };
    assert!(trace.records().any(|r| r.event == ambush(true)));
    assert!(trace.records().all(|r| r.event != ambush(false)));
}

/// Custom channel delays: `per * distance`, or never; records every call.
struct Delay {
    per: Option<u32>,
    seen: Vec<(u32, ChannelCtx)>,
}

impl SquadRules for Delay {
    fn channel_delay(&mut self, id: u32, ctx: &ChannelCtx) -> Option<u32> {
        self.seen.push((id, *ctx));
        self.per.map(|p| p * ctx.distance)
    }
}

/// The channel scene: leader at 0, member 2 at 4 on `two`, member 3 at 5
/// on a radio, member 4 at 6 on a shout; the orders due from tick 1 to 20.
fn channel_events(metric: Metric, two: Channel, rules: &mut Delay) -> (Vec<(u32, u32)>, Squad) {
    let mut sq = line_squad(metric, &[0, 4, 5, 6]);
    sq.add_tactic(tactics::rush());
    assert!(sq.set_channel(2, two));
    assert!(sq.set_channel(
        3,
        Channel::Radio {
            base: 1,
            per_unit: 1
        }
    ));
    assert!(sq.set_channel(4, Channel::Shout { range: 10 }));
    see(&mut sq, 10, 20, 0, 0);
    sq.plan_with(0, &[], &[], 0, rules).unwrap().unwrap();
    let mut orders = Vec::new();
    for now in 1..=20 {
        sq.check_with(now, &[], &[], 0, rules).unwrap();
        orders.extend(due(&mut sq, now));
    }
    (orders, sq)
}

#[test]
fn channel_custom_times_orders() {
    let mut rules = Delay {
        per: Some(2),
        seen: Vec::new(),
    };
    let (orders, sq) = channel_events(Metric::Chebyshev, Channel::Custom(2), &mut rules);
    assert!(sq.needs_rules());
    // Active at 5: the leader at once, member 2 after 2 * 4.
    assert!(orders.contains(&(1, 5)));
    assert!(orders.contains(&(2, 13)));
    assert_eq!(rules.seen.len(), 1);
    let (id, ctx) = rules.seen[0];
    assert_eq!(id, 2);
    assert_eq!(
        ctx,
        ChannelCtx {
            sender: 1,
            receiver: 2,
            from: [0, 0, 0],
            to: [4, 0, 0],
            distance: 4,
            now: 5,
            report: false,
        }
    );
    // Radio and shout keep the ticks they have with a plain squad.
    let mut plain = Delay {
        per: None,
        seen: Vec::new(),
    };
    let (today, _) = channel_events(Metric::Chebyshev, Channel::default(), &mut plain);
    let others = |o: &[(u32, u32)]| -> Vec<(u32, u32)> {
        o.iter()
            .copied()
            .filter(|&(m, _)| m == 3 || m == 4)
            .collect()
    };
    assert_eq!(others(&orders), others(&today));
    // Radio 1 + 5, shout 6 (in goal order).
    assert_eq!(others(&today), [(4, 11), (3, 11)]);
    // A custom channel that does not arrive: no order for member 2.
    let mut never = Delay {
        per: None,
        seen: Vec::new(),
    };
    let (lost, sq) = channel_events(Metric::Chebyshev, Channel::Custom(2), &mut never);
    assert!(lost.iter().all(|&(m, _)| m != 2));
    assert_eq!(never.seen.len(), 1);
    assert_eq!(
        Channel::Custom(2).arrival(Metric::Chebyshev, [0; 3], [4, 0, 0], 5),
        None
    );
    // Reports over the custom channel, from the member to the leader.
    assert_eq!(sq.report_arrives_at(2, 30), None);
    rules.seen.clear();
    assert_eq!(sq.report_arrives_at_with(2, 30, &mut rules), Some(38));
    assert_eq!(sq.report_arrives_at_with(1, 30, &mut rules), Some(30));
    assert_eq!(
        rules.seen,
        [(
            2,
            ChannelCtx {
                sender: 2,
                receiver: 1,
                from: [4, 0, 0],
                to: [0, 0, 0],
                distance: 4,
                now: 30,
                report: true,
            }
        )]
    );
}

/// Records the kind and tick of every report it gets.
#[derive(Default)]
struct Reports(Vec<(u32, u32)>);

impl SquadRules for Reports {
    fn report(&mut self, _: &SquadView<'_>, msg: &Message, now: u32, _: &mut ReportActions<'_>) {
        self.0.push((msg.kind, now));
    }
}

fn kind(kind: u32, sender: u32) -> Message {
    Message {
        kind,
        sender,
        ..Message::default()
    }
}

#[test]
fn report_slot_gets_unknown_kinds() {
    assert_eq!(REPORT_GAME_FIRST, 16);
    let mut sq = line_squad(Metric::Chebyshev, &[0, 2]);
    sq.add_tactic(tactics::rush());
    see(&mut sq, 10, 20, 0, 0);
    let mut rules = Reports::default();
    sq.plan_with(0, &[], &[], 0, &mut rules).unwrap().unwrap();
    let plan = sq.pending_plan().unwrap().id;
    let enemy = Fact {
        subject: 11,
        pos: [30, 0, 0],
        seen_at: 1,
        ..Fact::default()
    };
    for (msg, at) in [
        (Message::goal_broken(2, plan), 10),
        (kind(3, 2), 10),
        (Message::enemy(2, enemy), 10),
        (kind(16, 2), 10),
        (Message::arrived(2, plan), 10),
        (kind(99, 2), 12),
    ] {
        assert!(sq.report(msg, at));
    }
    for now in 1..=13 {
        sq.check_with(now, &[], &[], 0, &mut rules).unwrap();
    }
    assert_eq!(rules.0, [(3, 10), (16, 10), (99, 12)]);
    // The core kinds still did their part.
    assert!(sq.knowledge().get(11, ENEMY).is_some());
}

/// Report kinds of [`Acts`].
const KIND_A: u32 = 16;
const KIND_B: u32 = 17;
const KIND_FULL: u32 = 18;

/// A's area.
fn area_a() -> Area {
    Area {
        kind: 3,
        pos: [9, 9, 0],
        radius: 1,
        lifetime: 100,
        ..Area::default()
    }
}

/// A adds an area (and signals if `signal`); B looks for A's area (and
/// forgets target `forget`); FULL tries five actions.
#[derive(Default)]
struct Acts {
    signal: bool,
    forget: Option<u32>,
    b_saw_area: Vec<bool>,
    pushed: Vec<bool>,
}

impl SquadRules for Acts {
    fn report(&mut self, view: &SquadView<'_>, msg: &Message, _: u32, act: &mut ReportActions<'_>) {
        match msg.kind {
            KIND_A => {
                assert!(act.add_area(area_a()));
                if self.signal {
                    assert!(act.signal());
                }
            }
            KIND_B => {
                self.b_saw_area
                    .push(view.areas().any(|a| a.pos == area_a().pos));
                if let Some(s) = self.forget {
                    assert!(act.forget(s));
                }
            }
            KIND_FULL => {
                for s in 0..5 {
                    self.pushed.push(act.forget(900 + s));
                }
            }
            _ => {}
        }
    }
}

/// Rush on target 10 (target 11 farther), active at 5.
fn acts_squad(rules: &mut Acts) -> Squad {
    let mut sq = line_squad(Metric::Chebyshev, &[0, 2]);
    sq.add_tactic(tactics::rush());
    see(&mut sq, 10, 20, 0, 0);
    see(&mut sq, 11, 25, 0, 0);
    sq.plan_with(0, &[], &[], 0, rules).unwrap().unwrap();
    assert_eq!(sq.check_with(5, &[], &[], 0, rules), Ok(Status::Keep));
    assert_eq!(sq.active_plan().unwrap().target, Some(10));
    sq
}

#[test]
fn report_actions_apply_after_all_reports() {
    // A's signal alone: the signal is given.
    let mut rules = Acts {
        signal: true,
        ..Acts::default()
    };
    let mut sq = acts_squad(&mut rules);
    assert!(sq.report(kind(KIND_A, 2), 10));
    assert!(sq.report(kind(KIND_B, 2), 10));
    assert_eq!(
        sq.check_with(10, &[], &[], 0, &mut rules),
        Ok(Status::Signal)
    );
    assert!(sq.signal_given());
    // B did not see A's area; after the call it is there.
    assert_eq!(rules.b_saw_area, [false]);
    assert!(sq.areas().iter().any(|a| a.pos == area_a().pos));

    // A's signal and B's forget of the active target: the lost target
    // wins, the signal is not given.
    let mut rules = Acts {
        signal: true,
        forget: Some(10),
        ..Acts::default()
    };
    let mut sq = acts_squad(&mut rules);
    assert!(sq.report(kind(KIND_A, 2), 10));
    assert!(sq.report(kind(KIND_B, 2), 10));
    let status = sq.check_with(10, &[], &[], 0, &mut rules);
    assert!(matches!(status, Ok(Status::Replan(_))), "{status:?}");
    assert!(!sq.signal_given());
    assert_eq!(sq.pending_plan().unwrap().target, Some(11));
    assert!(sq.knowledge().get(10, ENEMY).is_none());
    // While the new plan is pending a signal action still signals the
    // active plan.
    rules.forget = None;
    assert!(sq.report(kind(KIND_A, 2), 11));
    assert_eq!(
        sq.check_with(11, &[], &[], 0, &mut rules),
        Ok(Status::Signal)
    );
    assert!(sq.signal_given());

    // At most four actions per report.
    let mut rules = Acts::default();
    let mut sq = acts_squad(&mut rules);
    assert!(sq.report(kind(KIND_FULL, 2), 10));
    assert_eq!(sq.check_with(10, &[], &[], 0, &mut rules), Ok(Status::Keep));
    assert_eq!(rules.pushed, [true, true, true, true, false]);
}

#[test]
fn report_actions_do_not_grow_after_warm_up() {
    let mut rules = Acts::default();
    let mut sq = acts_squad(&mut rules);
    let mut after_two = None;
    for now in 6..=60u32 {
        for _ in 0..3 {
            assert!(sq.report(kind(KIND_FULL, 2), now));
        }
        sq.check_with(now, &[], &[], 0, &mut rules).unwrap();
        if now == 7 {
            after_two = Some(sq.scratch.actions.capacity());
            assert!(sq.scratch.actions.capacity() >= 3 * MAX_REPORT_ACTIONS);
        }
    }
    assert_eq!(Some(sq.scratch.actions.capacity()), after_two);
}

#[test]
fn plain_calls_refuse_custom_parts() {
    let mut sq = line_squad(Metric::Chebyshev, &[0, 4]);
    sq.add_tactic(tactics::rush());
    see(&mut sq, 10, 20, 0, 0);
    assert!(!sq.needs_rules());
    assert!(sq.set_channel(2, Channel::Custom(1)));
    assert!(sq.needs_rules());
    assert_eq!(sq.plan(0, &[], &[], 0), Err(PlanError::NeedsRules));
    assert_eq!(sq.check(0, &[], &[], 0), Err(PlanError::NeedsRules));
    let mut rules = Delay {
        per: Some(2),
        seen: Vec::new(),
    };
    let id = sq.plan_with(0, &[], &[], 0, &mut rules).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().active_at, 5);
    // Plain calls do not activate the due plan.
    assert_eq!(due(&mut sq, 5), []);
    assert_eq!(sq.active_plan(), None);
    assert_eq!(sq.take_activated(), None);
    assert!(!sq.arrived(1, 5));
    assert_eq!(sq.active_plan(), None);
    // The next call with rules does, timed from `active_at`.
    assert_eq!(sq.check_with(6, &[], &[], 0, &mut rules), Ok(Status::Keep));
    assert_eq!(sq.take_activated(), Some(id));
    assert_eq!(SquadView::new(&sq).active_plan().unwrap().since, 5);
    assert_eq!(rules.seen[0].1.now, 5);
    assert_eq!(due(&mut sq, 6), [(1, 6)]);
    assert_eq!(due(&mut sq, 12), []);
    assert_eq!(due(&mut sq, 13), [(2, 13)]);
    // The plain signal refuses; with rules it is given.
    assert!(!sq.signal(7));
    assert!(!sq.signal_given());
    assert!(sq.signal_with(7, &mut rules));
    assert!(sq.signal_given());
    // A suspended member on the custom channel: the plain reinstate puts
    // it back without an order, with rules it gets one at its delay.
    assert!(sq.group_mut().suspend(2));
    assert!(sq.reinstate(2, 20));
    let state = |sq: &Squad| sq.group().iter().find(|m| m.id == 2).unwrap().state;
    assert_eq!(state(&sq), MemberState::Active);
    assert!((20..=60).all(|now| due(&mut sq, now).is_empty()));
    assert!(sq.group_mut().suspend(2));
    assert!(sq.reinstate_with(2, 70, &mut rules));
    assert_eq!(state(&sq), MemberState::Active);
    assert_eq!(due(&mut sq, 77), []);
    assert_eq!(due(&mut sq, 78), [(2, 78)]);
}

#[test]
fn custom_parts_square_and_hex() {
    let (square, ..) = trigger_events(Metric::Chebyshev);
    let (hex, ..) = trigger_events(Metric::Hex);
    assert_eq!(square, hex);
    let (square, rs, ..) = abort_events(Metric::Chebyshev);
    let (hex, rh, ..) = abort_events(Metric::Hex);
    assert_eq!(square, hex);
    assert_eq!(rs.seen, rh.seen);
    let delay = || Delay {
        per: Some(2),
        seen: Vec::new(),
    };
    let (mut rs, mut rh) = (delay(), delay());
    let (square, _) = channel_events(Metric::Chebyshev, Channel::Custom(2), &mut rs);
    let (hex, _) = channel_events(Metric::Hex, Channel::Custom(2), &mut rh);
    assert_eq!(square, hex);
    assert_eq!(rs.seen, rh.seen);
}
