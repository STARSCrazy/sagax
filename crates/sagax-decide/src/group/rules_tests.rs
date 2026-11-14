//! The rules object: built-in rules change nothing, and each plan-side slot
//! does what it says, in the documented order.

use super::*;
use crate::memory::{Area, Fact, Message, Metric, Pos, ZeroCapacity};
use crate::{Consideration, Curve, DecideEvent, ONE, mix};

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
        let expected = pick_spot(wish, want, &spots, &taken, skip, metric, radius);
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
        let got = rules::rank_spots(
            &mut BuiltInRules,
            &mut keys,
            &ctx,
            &spots,
            &taken,
            skip,
            metric,
            radius,
        );
        assert_eq!(got, expected, "seed {seed}");
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
    sq.set_score_log(true);
    see(&mut sq, 10, 20, 0, 0);
    see(&mut sq, 11, 23, 0, 0);
    sq.plan_with(0, &[0, 5, 9], &[], 0, &mut KeepOnly(&[11]))
        .unwrap()
        .unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!((p.tactic, p.target), (t, Some(11)));
    // Only 11 was scored; it read its own block (2) and its dropped friend.
    let inputs: Vec<(Option<u32>, i32)> = sq
        .score_log()
        .iter()
        .filter_map(|e| match e.event {
            DecideEvent::Factor { input, .. } => Some((e.target, input)),
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
    sq.set_score_log(true);
    see(&mut sq, 10, 20, 0, 0);
    let mut rules = Columns {
        values: [7, 9],
        seen: Vec::new(),
    };
    sq.plan_with(0, &[0, 3], &[], 0, &mut rules)
        .unwrap()
        .unwrap();
    assert_eq!(rules.seen, [(start, 2)]);
    let factors: Vec<(i32, u32)> = sq
        .score_log()
        .iter()
        .filter_map(|e| match e.event {
            DecideEvent::Factor { input, value, .. } => Some((input, value)),
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
