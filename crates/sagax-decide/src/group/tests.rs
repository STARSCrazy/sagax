use super::*;
use crate::memory::{Metric, Pos, ZeroCapacity};

fn member(id: u32) -> Member {
    Member {
        id,
        pos: [0, 0, 0],
        roles: 0,
        skill: 1,
        state: MemberState::Active,
    }
}

fn near(id: u32, faction: u32, x: i32, y: i32, free: bool) -> Nearby {
    Nearby {
        id,
        faction,
        pos: [x, y, 0],
        free,
    }
}

#[test]
fn group_zero_capacity() {
    assert_eq!(Group::new(0).err(), Some(ZeroCapacity));
}

#[test]
fn group_add_full_and_duplicate() {
    let mut g = Group::new(2).unwrap();
    assert_eq!(g.add(member(1)), Some(0));
    assert_eq!(g.add(member(1)), None);
    assert_eq!(g.add(member(2)), Some(1));
    assert_eq!(g.add(member(3)), None);
    assert_eq!(g.len(), 2);
}

#[test]
fn group_remove_keeps_order() {
    let mut g = Group::new(4).unwrap();
    for id in [5, 6, 7] {
        g.add(member(id));
    }
    assert!(g.remove(6));
    assert!(!g.remove(6));
    let ids: Vec<u32> = g.iter().map(|m| m.id).collect();
    assert_eq!(ids, [5, 7]);
    assert_eq!(g.index(7), Some(1));
    assert_eq!(g.get(1).map(|m| m.id), Some(7));
}

#[test]
fn suspend_resume_roundtrip() {
    let mut g = Group::new(3).unwrap();
    g.add(member(1));
    g.add(member(2));
    assert!(g.suspend(1));
    let active: Vec<usize> = g.active().map(|(i, _)| i).collect();
    assert_eq!(active, [1]);
    assert!(g.resume(1));
    assert_eq!(g.active().count(), 2);
    assert!(g.set_gone(2));
    assert_eq!(g.active().count(), 1);
    assert!(g.set_pos(1, [3, 4, 5]));
    assert_eq!(g.get(0).unwrap().pos, [3, 4, 5]);
    assert!(!g.suspend(99) && !g.resume(99) && !g.set_gone(99) && !g.set_pos(99, [0; 3]));
}

#[test]
fn gather_faction_radius_nearest_first() {
    let c = [
        near(1, 7, 5, 0, true),
        near(2, 7, 2, 0, true),
        near(3, 8, 1, 0, true),
        near(4, 7, 11, 0, true),
        near(5, 7, 10, 0, true),
    ];
    let mut out = Vec::new();
    gather([0; 3], 7, 10, Metric::Euclid, false, &c, &mut out, 10);
    assert_eq!(out, [2, 1, 5]);
    gather([0; 3], 7, 10, Metric::Euclid, false, &c, &mut out, 2);
    assert_eq!(out, [2, 1]);
}

#[test]
fn gather_free_only() {
    let c = [near(1, 7, 1, 0, false), near(2, 7, 2, 0, true)];
    let mut out = Vec::new();
    gather([0; 3], 7, 10, Metric::Euclid, true, &c, &mut out, 10);
    assert_eq!(out, [2]);
    gather([0; 3], 7, 10, Metric::Euclid, false, &c, &mut out, 10);
    assert_eq!(out, [1, 2]);
}

#[test]
fn gather_tie_lower_id() {
    let c = [near(9, 7, 3, 0, true), near(4, 7, 0, 3, true)];
    let mut out = Vec::new();
    gather([0; 3], 7, 5, Metric::Euclid, false, &c, &mut out, 10);
    assert_eq!(out, [4, 9]);
}

#[test]
fn gather_hex_metric() {
    // (2, -2) is 2 hex steps away but 8 squared in a straight line.
    let c = [near(1, 7, 2, -2, true), near(2, 7, 2, 2, true)];
    let mut out = Vec::new();
    gather([0; 3], 7, 2, Metric::Hex, false, &c, &mut out, 10);
    assert_eq!(out, [1]);
    gather([0; 3], 7, 2, Metric::Euclid, false, &c, &mut out, 10);
    assert!(out.is_empty());
}

// --- Tactics as data, wish points, spot pick ---

/// A spot radius that takes every spot of the tests.
const FAR: i64 = 1000;

fn spot(x: i32, y: i32, tags: u32) -> Spot {
    Spot {
        pos: [x, y, 0],
        tags,
    }
}

#[test]
fn heading_square_8_steps() {
    let o = [0, 0, 0];
    let cases = [
        ([5, 0, 0], 0),
        ([5, 5, 0], 1),
        ([0, 5, 0], 2),
        ([-5, 5, 0], 3),
        ([-5, 0, 0], 4),
        ([-5, -5, 0], 5),
        ([0, -5, 0], 6),
        ([5, -5, 0], 7),
        // 2/5 = 0.4 is below tan 22.5 deg: stays on the axis; 1/2 is diagonal.
        ([5, 2, 0], 0),
        ([4, 2, 0], 1),
        ([1, -3, 0], 6),
    ];
    for metric in [Metric::Euclid, Metric::Manhattan, Metric::Chebyshev] {
        for (to, want) in cases {
            assert_eq!(heading(metric, o, to), want, "{metric:?} {to:?}");
        }
    }
    assert_eq!(heading(Metric::Euclid, o, o), 0);
}

#[test]
fn heading_hex_6_steps() {
    let o = [0, 0, 0];
    let dirs = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];
    for (k, &(q, r)) in dirs.iter().enumerate() {
        assert_eq!(heading(Metric::Hex, o, [3 * q, 3 * r, 0]), k as u8);
    }
    // Exactly between step 0 and step 1: the lower step.
    assert_eq!(heading(Metric::Hex, o, [1, 1, 0]), 0);
    // Nearer step 1 than step 0.
    assert_eq!(heading(Metric::Hex, o, [1, 3, 0]), 1);
    // Relative to `from`.
    assert_eq!(heading(Metric::Hex, [5, 5, 0], [5, 2, 0]), 4);
}

#[test]
fn wish_side_left_is_left_of_front() {
    let c = [0, 0, 0];
    let target = Some([10, 0, 0]);
    let wish = |s| wish_point(&Place::Side(s), c, c, target, None, Metric::Euclid, 3);
    assert_eq!(wish(Side::Front), [7, 0, 0]);
    assert_eq!(wish(Side::Rear), [13, 0, 0]);
    assert_eq!(wish(Side::Left), [10, 3, 0]);
    assert_eq!(wish(Side::Right), [10, -3, 0]);
    // Turned with the group: target straight at +y, left is now -x.
    let up = Some([0, 10, 0]);
    let left = wish_point(&Place::Side(Side::Left), c, c, up, None, Metric::Euclid, 3);
    assert_eq!(left, [-3, 10, 0]);
    // Hex: left is 60 deg from the rear, right mirrored.
    let hex = |s| wish_point(&Place::Side(s), c, c, target, None, Metric::Hex, 3);
    assert_eq!(hex(Side::Left), [10, 3, 0]);
    assert_eq!(hex(Side::Right), [13, -3, 0]);
    assert_eq!(hex(Side::Front), [7, 0, 0]);
}

#[test]
fn wish_offset_turns_with_heading() {
    let leader = [2, 2, 1];
    let c = [0, 0, 0];
    let off = Place::Offset([4, 0, 1]);
    let wish = |t: Option<Pos>, m| wish_point(&off, leader, c, t, None, m, 0);
    // No target: heading 0, offset as is (z added too).
    assert_eq!(wish(None, Metric::Euclid), [6, 2, 2]);
    // Target at +y: a quarter turn.
    assert_eq!(wish(Some([0, 9, 0]), Metric::Euclid), [2, 6, 2]);
    // Target at -x: half turn.
    assert_eq!(wish(Some([-9, 0, 0]), Metric::Euclid), [-2, 2, 2]);
    // Diagonal: 4 * 0.7071 = 2.83, rounded to 3.
    assert_eq!(wish(Some([9, 9, 0]), Metric::Euclid), [5, 5, 2]);
    // Hex step 1 turns (4, 0) into (0, 4); step 2 into (-4, 4).
    assert_eq!(wish(Some([0, 9, 0]), Metric::Hex), [2, 6, 2]);
    assert_eq!(wish(Some([-9, 9, 0]), Metric::Hex), [-2, 6, 2]);
    // Here is the leader.
    let here = wish_point(
        &Place::Here,
        leader,
        c,
        Some([9, 0, 0]),
        None,
        Metric::Euclid,
        5,
    );
    assert_eq!(here, leader);
}

#[test]
fn wish_between_target_and_friend() {
    let c = [0, 0, 0];
    let t = Some([10, 0, 0]);
    let f = Some([10, 7, 0]);
    let p = wish_point(&Place::Between, c, c, t, f, Metric::Euclid, 2);
    assert_eq!(p, [10, 3, 0]);
    // Rounds down on negatives too.
    let p = wish_point(
        &Place::Between,
        c,
        c,
        t,
        Some([10, -7, 0]),
        Metric::Euclid,
        2,
    );
    assert_eq!(p, [10, -4, 0]);
    // No friend: the target's front.
    let p = wish_point(&Place::Between, c, c, t, None, Metric::Euclid, 2);
    assert_eq!(p, [8, 0, 0]);
    // Tagged looks near the group centre.
    let p = wish_point(&Place::Tagged(4), [5, 5, 0], c, t, f, Metric::Euclid, 2);
    assert_eq!(p, c);
}

#[test]
fn pick_spot_prefers_tags_then_nearest() {
    let spots = [
        spot(1, 0, 0),
        spot(5, 0, 2),
        spot(3, 0, 2 | 4),
        spot(3, 0, 6),
    ];
    let o = [0, 0, 0];
    // Index 2 and 3 have tag 2 at equal distance: lower index wins.
    assert_eq!(
        pick_spot(o, 2, &spots, &[], 0, Metric::Euclid, FAR),
        SpotPick::Spot(2)
    );
    // Both bits wanted: only 2 and 3 have 2|4.
    assert_eq!(
        pick_spot([6, 0, 0], 6, &spots, &[], 0, Metric::Euclid, FAR),
        SpotPick::Spot(2)
    );
    // No spot fits: nearest free spot.
    assert_eq!(
        pick_spot(o, 8, &spots, &[], 0, Metric::Euclid, FAR),
        SpotPick::Spot(0)
    );
}

#[test]
fn pick_spot_tags_zero_falls_back_nearest() {
    // A game with no tags at all: every shipped tactic still gets a spot.
    let spots = [spot(9, 9, 0), spot(2, 1, 0), spot(-4, 0, 0)];
    let all = [
        tactics::rush(),
        tactics::formation(Shape::Wedge, 2),
        tactics::flank(3, 4),
        tactics::ambush(),
        tactics::focus(),
        tactics::isolate(),
        tactics::fall_back(),
    ];
    for t in &all {
        for s in &t.slots {
            let want = s.want_tags | s.place.tags();
            let pick = pick_spot([1, 1, 0], want, &spots, &[], 0, Metric::Chebyshev, FAR);
            assert_eq!(pick, SpotPick::Spot(1));
        }
    }
}

#[test]
fn pick_spot_no_spots_unchecked() {
    let wish = [4, 5, 6];
    assert_eq!(
        pick_spot(wish, 1, &[], &[], 0, Metric::Hex, FAR),
        SpotPick::Unchecked(wish)
    );
    // All taken counts as none.
    let spots = [spot(0, 0, 1)];
    assert_eq!(
        pick_spot(wish, 0, &spots, &[true], 0, Metric::Hex, FAR),
        SpotPick::Unchecked(wish)
    );
}

#[test]
fn pick_spot_skips_taken() {
    let spots = [spot(1, 0, 1), spot(2, 0, 1), spot(3, 0, 0)];
    let w = [0, 0, 0];
    assert_eq!(
        pick_spot(w, 1, &spots, &[true], 0, Metric::Manhattan, FAR),
        SpotPick::Spot(1)
    );
    assert_eq!(
        pick_spot(w, 1, &spots, &[true, true], 0, Metric::Manhattan, FAR),
        SpotPick::Spot(2)
    );
}

#[test]
fn pick_spot_skip_takes_next_best() {
    let spots = [spot(3, 0, 0), spot(1, 0, 1), spot(2, 0, 1), spot(1, 0, 0)];
    let w = [0, 0, 0];
    let pick = |skip| pick_spot(w, 1, &spots, &[], skip, Metric::Euclid, FAR);
    // Ranked: tagged 1, 2, then untagged 3 (distance 1), 0 (distance 3).
    assert_eq!(pick(0), SpotPick::Spot(1));
    assert_eq!(pick(1), SpotPick::Spot(2));
    assert_eq!(pick(2), SpotPick::Spot(3));
    assert_eq!(pick(3), SpotPick::Spot(0));
    // Clamped to the last ranked spot.
    assert_eq!(pick(200), SpotPick::Spot(0));
    // Taken spots are not ranked.
    let taken = [false, false, true];
    assert_eq!(
        pick_spot(w, 1, &spots, &taken, 1, Metric::Euclid, FAR),
        SpotPick::Spot(3)
    );
}

#[test]
fn shipped_tactics_valid() {
    let all = [
        tactics::rush(),
        tactics::formation(Shape::Line, 2),
        tactics::formation(Shape::Wedge, 2),
        tactics::formation(Shape::Column, 2),
        tactics::flank(2, 4),
        tactics::flank(3, 4),
        tactics::ambush(),
        tactics::focus(),
        tactics::isolate(),
        tactics::fall_back(),
    ];
    for t in &all {
        assert!(!t.slots.is_empty());
        assert!(t.weight > 0);
        for s in &t.slots {
            assert!(s.min <= s.max, "{t:?}");
            assert!(s.max > 0);
        }
    }
    for sides in [0, 2, 3, 9] {
        let f = tactics::flank(sides, 4);
        assert_eq!(f.trigger, Trigger::AllArrived { max_wait: 170 });
        assert!(
            f.slots
                .iter()
                .all(|s| s.gather.is_some() && s.start == Start::OnSignal)
        );
    }
    assert_eq!(tactics::flank(2, 4).slots.len(), 2);
    assert_eq!(tactics::flank(3, 4).slots.len(), 3);
    // The three squad levels: rush, formation, flank.
    assert_eq!(tactics::rush().min_skill, 0);
    assert_eq!(tactics::formation(Shape::Line, 2).min_skill, 64);
    assert_eq!(tactics::flank(3, 4).min_skill, 128);
    // Formation offsets scale with the spacing.
    let col = tactics::formation(Shape::Column, 3);
    assert_eq!(col.slots[2].place, Place::Offset([-6, 0, 0]));
}

// ---- Squad planning ----

use crate::memory::{Area, Fact};
use crate::{Consideration, Curve, ONE};

/// Fact kind the squad tests use for enemies.
const ENEMY: u32 = 1;

fn squad_of(metric: Metric, members: &[(u32, i32, i32, u32)]) -> Squad {
    let mut sq = Squad::new(8, 8, 4, 8, metric).unwrap();
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

fn see(sq: &mut Squad, id: u32, x: i32, y: i32, now: u32) {
    sq.knowledge_mut().learn(
        Fact {
            subject: id,
            kind: ENEMY,
            pos: [x, y, 0],
            seen_at: now,
            ..Fact::default()
        },
        now,
    );
}

fn four() -> [(u32, i32, i32, u32); 4] {
    [(1, 0, 0, 0), (2, 1, 0, 0), (3, 0, 1, 0), (4, 1, 1, 0)]
}

/// `(member, slot, kind)` of every goal of the pending plan, in order.
fn slots_of(sq: &Squad) -> Vec<(u32, u8, GoalKind)> {
    sq.pending_plan()
        .unwrap()
        .goals
        .iter()
        .map(|(id, g)| (*id, g.slot, g.kind))
        .collect()
}

#[test]
fn squad_zero_capacity() {
    for caps in [(0, 1, 1, 1), (1, 0, 1, 1), (1, 1, 0, 1), (1, 1, 1, 0)] {
        let r = Squad::new(caps.0, caps.1, caps.2, caps.3, Metric::Euclid);
        assert_eq!(r.err(), Some(ZeroCapacity));
    }
}

#[test]
fn weak_squad_does_not_know_flank() {
    let mut sq = squad_of(Metric::Euclid, &four());
    let rush = sq.add_tactic(tactics::rush());
    let flank = sq.add_tactic(tactics::flank(2, 4));
    see(&mut sq, 10, 20, 0, 0);
    sq.set_skill(0);
    for seed in 0..32 {
        sq.plan(0, &[], &[], seed).unwrap().unwrap();
        assert_eq!(sq.pending_plan().unwrap().tactic, rush);
    }
    sq.set_skill(255);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().tactic, flank);
}

#[test]
fn elite_picks_best() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    sq.add_tactic(tactics::formation(Shape::Line, 2));
    sq.add_tactic(tactics::focus());
    let flank = sq.add_tactic(tactics::flank(3, 4));
    see(&mut sq, 10, 20, 0, 0);
    sq.set_skill(255);
    for seed in 0..32 {
        sq.plan(0, &[], &[], seed).unwrap().unwrap();
        let p = sq.pending_plan().unwrap();
        assert_eq!((p.tactic, p.target), (flank, Some(10)));
    }
}

#[test]
fn weak_pick_is_seeded_and_repeatable() {
    let build = || {
        let mut sq = squad_of(Metric::Euclid, &four());
        // Weights 1/8 (never in the top 3), 1/4, 1/2, 3/4.
        for w in [ONE / 8, ONE / 4, ONE / 2, ONE * 3 / 4] {
            sq.add_tactic(Tactic {
                weight: w,
                ..tactics::rush()
            });
        }
        see(&mut sq, 10, 20, 0, 0);
        sq.set_skill(0);
        sq
    };
    let mut a = build();
    let mut b = build();
    let mut seen = [false; 4];
    for seed in 0..64 {
        a.plan(7, &[], &[], seed).unwrap().unwrap();
        b.plan(7, &[], &[], seed).unwrap().unwrap();
        let (pa, pb) = (a.pending_plan().unwrap(), b.pending_plan().unwrap());
        assert_eq!((pa.tactic, &pa.goals), (pb.tactic, &pb.goals));
        seen[pa.tactic] = true;
    }
    assert_eq!(seen, [false, true, true, true]);
}

#[test]
fn assign_roles_first_then_distance() {
    let slot = |roles| Slot {
        roles,
        min: 1,
        max: 1,
        gather: None,
        place: Place::Here,
        kind: GoalKind::Hold,
        start: Start::Now,
        want_tags: 0,
        dist: 1,
        gather_dist: 2,
    };
    let t = Tactic {
        slots: vec![slot(ROLE_RANGED), slot(0), slot(0)],
        considerations: Vec::new(),
        weight: ONE,
        aborts: Vec::new(),
        trigger: Trigger::None,
        min_skill: 0,
        needs_target: false,
    };
    // Slot 0 wants ranged: member 3, though farthest. Slot 1: the leader,
    // at the wish point. Slot 2: 2 and 4 tie at distance 2, the lower
    // member index (2) wins.
    let mut sq = squad_of(
        Metric::Chebyshev,
        &[
            (1, 0, 0, 0),
            (2, 2, 0, 0),
            (3, 9, 0, ROLE_RANGED),
            (4, -2, 0, 0),
        ],
    );
    sq.add_tactic(t);
    sq.set_skill(255);
    let id = sq.plan(0, &[], &[], 0).unwrap().unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!(p.id, id);
    let got: Vec<(u32, u8)> = p.goals.iter().map(|(m, g)| (*m, g.slot)).collect();
    assert_eq!(got, [(3, 0), (1, 1), (2, 2)]);
    for (_, g) in &p.goals {
        assert_eq!(g.pos, Some([0, 0, 0]));
        assert!(g.unchecked);
        assert_eq!(g.plan, id);
        assert_eq!(g.target, None);
    }
}

#[test]
fn assign_skips_suspended() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    see(&mut sq, 10, 20, 0, 0);
    sq.group_mut().suspend(2);
    sq.group_mut().set_gone(3);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    // Nearest the wish point (the target's front) first.
    let ids: Vec<u32> = sq
        .pending_plan()
        .unwrap()
        .goals
        .iter()
        .map(|g| g.0)
        .collect();
    assert_eq!(ids, [4, 1]);
}

#[test]
fn target_only_from_leader_memory() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    sq.add_tactic(tactics::focus());
    sq.set_skill(255);
    // Nothing known: no plan with a target is possible.
    assert_eq!(sq.plan(0, &[], &[], 0), Ok(None));
    assert!(sq.pending_plan().is_none());
    // Enemy 11 stands right next to the group in the caller's world, but
    // the leader knows only enemy 10 (and a noise of 11, another kind).
    let _caller_world = [(10u32, [30, 0, 0]), (11u32, [2, 0, 0])];
    see(&mut sq, 10, 30, 0, 0);
    sq.knowledge_mut().learn(
        Fact {
            subject: 11,
            kind: ENEMY + 1,
            pos: [2, 0, 0],
            ..Fact::default()
        },
        0,
    );
    for seed in 0..16 {
        sq.plan(0, &[], &[], seed).unwrap().unwrap();
        assert_eq!(sq.pending_plan().unwrap().target, Some(10));
    }
}

#[test]
fn per_target_scoring() {
    let mut sq = squad_of(Metric::Euclid, &four());
    // Focus, plus a caller column: the target's strength, weaker is better.
    let mut focus = tactics::focus();
    focus
        .considerations
        .push(Consideration::new(IN_EXTRA as u32, Curve::Inverse, 0, 10));
    let f = sq.add_tactic(focus);
    sq.set_skill(255);
    sq.set_extra_columns(1);
    see(&mut sq, 10, 60, 0, 0);
    see(&mut sq, 11, 20, 0, 0);
    // Rows: no target, then the targets in memory order (10, 11).
    sq.plan(0, &[0, 5, 2], &[], 0).unwrap().unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!((p.tactic, p.target), (f, Some(11)));
    // The nearer one is now too strong.
    sq.plan(0, &[0, 5, 9], &[], 0).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().target, Some(10));
    // Plain distance: the nearer target wins on equal strength.
    sq.plan(0, &[0, 5, 5], &[], 0).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().target, Some(11));
}

#[test]
fn isolate_picks_farthest_from_friends() {
    let mut sq = squad_of(Metric::Euclid, &four());
    // Which target is most isolated is the caller's column: the distance
    // from each target to its nearest friend.
    let mut iso = tactics::isolate();
    iso.considerations
        .push(Consideration::new(IN_EXTRA as u32, Curve::Linear, 0, 50));
    sq.add_tactic(iso);
    sq.set_skill(255);
    sq.set_extra_columns(1);
    see(&mut sq, 10, 20, 0, 0);
    see(&mut sq, 11, 22, 0, 0);
    see(&mut sq, 12, 20, 30, 0);
    sq.plan(0, &[0, 2, 2, 30], &[], 0).unwrap().unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!(p.target, Some(12));
    // Hold between 12 and its nearest friend 10; the rest attack.
    let holds: Vec<_> = p
        .goals
        .iter()
        .filter(|(_, g)| g.kind == GoalKind::Hold)
        .map(|(_, g)| g.pos)
        .collect();
    assert_eq!(holds, [Some([20, 15, 0]); 2]);
    let attacks = p
        .goals
        .iter()
        .filter(|(_, g)| g.kind == GoalKind::Attack && g.target == Some(12))
        .count();
    assert_eq!(attacks, 2);
}

#[test]
fn hex_assign_same_as_square_shape() {
    // Formation offsets stay in the square frame on hex; the same members
    // get the same slots in the same order.
    let line = [
        (1, 0, 0, 0),
        (2, 0, 2, 0),
        (3, 0, -2, 0),
        (4, 0, 4, 0),
        (5, 0, -4, 0),
    ];
    let mut got = Vec::new();
    for metric in [Metric::Chebyshev, Metric::Hex] {
        let mut sq = squad_of(metric, &line);
        sq.add_tactic(tactics::formation(Shape::Line, 2));
        sq.add_tactic(tactics::rush());
        sq.set_skill(255);
        see(&mut sq, 10, 20, 0, 0);
        sq.plan(0, &[], &[], 0).unwrap().unwrap();
        got.push(slots_of(&sq));
    }
    assert_eq!(got[0], got[1]);
    let ids: Vec<u32> = got[0].iter().map(|g| g.0).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5]);
    let slots: Vec<u8> = got[0].iter().map(|g| g.1).collect();
    assert_eq!(slots, [0, 1, 2, 3, 4]);
}

#[test]
fn flank_spreads_over_sides() {
    // Minimums first, then one more per slot in turn: 4 members on 3 sides.
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::flank(3, 4));
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    let mut per_slot = [0; 3];
    for (_, slot, _) in slots_of(&sq) {
        per_slot[usize::from(slot)] += 1;
    }
    assert_eq!(per_slot, [2, 1, 1]);
}

#[test]
fn fall_back_without_target_moves_away_from_danger() {
    let mut sq = squad_of(Metric::Euclid, &four());
    let fb = sq.add_tactic(tactics::fall_back());
    sq.set_skill(255);
    // No losses: fall back scores 0, nothing else known.
    assert_eq!(sq.plan(0, &[], &[], 0), Ok(None));
    sq.group_mut().set_gone(3);
    sq.group_mut().set_gone(4);
    // No target, no danger: stay at the leader.
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!((p.tactic, p.target), (fb, None));
    assert!(p.goals.iter().all(|(_, g)| g.pos == Some([0, 0, 0])));
    // A danger area to +x: retreat to -x behind the leader.
    sq.areas_mut().add(
        Area {
            pos: [20, 0, 0],
            radius: 3,
            lifetime: 100,
            ..Area::default()
        },
        0,
    );
    sq.plan(1, &[], &[], 0).unwrap().unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!(p.goals[0].1.pos, Some([-tactics::FALL_BACK_DIST, 0, 0]));
    assert_eq!(p.goals[0].1.kind, GoalKind::Retreat);
    // A known enemy to +y beats the area as the nearest threat.
    see(&mut sq, 10, 0, 5, 1);
    sq.plan(2, &[], &[], 0).unwrap().unwrap();
    let p = sq.pending_plan().unwrap();
    assert_eq!(p.target, None);
    assert_eq!(p.goals[0].1.pos, Some([0, -tactics::FALL_BACK_DIST, 0]));
}

#[test]
fn weak_spot_error_seeded() {
    // Spots along x; the wish point (the target's front, 19) is nearest
    // the spot at 19, then 18, 17, ...; rush's spot radius (3) reaches 16.
    let spots: Vec<Spot> = (0..6)
        .map(|i| Spot {
            pos: [19 - i, 0, 0],
            tags: 0,
        })
        .collect();
    let first_spot = |skill: u8, seed: u64| {
        let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0)]);
        sq.add_tactic(tactics::rush());
        sq.set_skill(skill);
        see(&mut sq, 10, 20, 0, 0);
        sq.plan(3, &[], &spots, seed).unwrap().unwrap();
        sq.pending_plan().unwrap().goals[0].1.pos.unwrap()
    };
    let mut ranks = [false; 6];
    for seed in 0..64 {
        assert_eq!(first_spot(255, seed), [19, 0, 0]);
        let p = first_spot(0, seed);
        assert_eq!(p, first_spot(0, seed));
        ranks[(19 - p[0]) as usize] = true;
    }
    // Weak: up to 2 ranks off, never more.
    assert_eq!(ranks, [true, true, true, false, false, false]);
}

#[test]
fn plan_ids_increase_and_stamp_time() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    see(&mut sq, 10, 20, 0, 0);
    let a = sq.plan(0, &[], &[], 0).unwrap().unwrap();
    let b = sq.plan(5, &[], &[], 0).unwrap().unwrap();
    assert!(b > a);
    let p = sq.pending_plan().unwrap();
    assert_eq!(p.made_at, 5);
    assert!(p.goals.iter().all(|(_, g)| g.issued_at == 5 && g.plan == b));
    assert_eq!(
        [
            IN_LOSSES,
            IN_OFF_PLAN,
            IN_TARGET_AGE,
            IN_TARGET_DIST,
            IN_RANGED,
            IN_AREA,
            IN_PLAN_AGE,
            IN_TARGET_FRIEND_DIST,
            IN_EXTRA
        ],
        [0, 1, 2, 3, 4, 5, 6, 7, 8]
    );
}

#[test]
fn plan_rejects_column_beyond_row() {
    let mut sq = squad_of(Metric::Euclid, &four());
    let mut focus = tactics::focus();
    focus
        .considerations
        .push(Consideration::new(IN_EXTRA as u32, Curve::Linear, 0, 10));
    sq.add_tactic(tactics::rush());
    let f = sq.add_tactic(focus);
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    // No caller columns: a caller error, not a silent "no plan".
    let err = PlanError::InputOutOfRange {
        tactic: f,
        input: IN_EXTRA as u32,
    };
    assert_eq!(sq.validate(), Err(err));
    assert_eq!(sq.plan(0, &[], &[], 0), Err(err));
    assert!(sq.pending_plan().is_none());
    // Also with nothing to plan for.
    sq.knowledge_mut().clear();
    assert_eq!(sq.plan(0, &[], &[], 0), Err(err));
    sq.set_extra_columns(1);
    assert_eq!(sq.validate(), Ok(()));
    see(&mut sq, 10, 20, 0, 0);
    assert!(sq.plan(0, &[0, 10], &[], 0).unwrap().is_some());
}

#[test]
fn required_roles_are_hard() {
    // A flank whose left side needs a ranged member.
    let mut ranged_flank = tactics::flank(2, 4);
    ranged_flank.slots[0].roles = ROLE_RANGED;
    let build = |members: &[(u32, i32, i32, u32)]| {
        let mut sq = squad_of(Metric::Euclid, members);
        sq.add_tactic(tactics::rush());
        sq.add_tactic(ranged_flank.clone());
        sq.set_skill(255);
        see(&mut sq, 10, 20, 0, 0);
        sq
    };
    // Nobody ranged: the flank is no candidate, rush is chosen.
    let mut sq = build(&four());
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().tactic, 0);
    // Member 4 is ranged: the flank wins, and only 4 fills the left side.
    let mut members = four();
    members[3].3 = ROLE_RANGED;
    let mut sq = build(&members);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().tactic, 1);
    let left: Vec<u32> = slots_of(&sq)
        .iter()
        .filter(|g| g.1 == 0)
        .map(|g| g.0)
        .collect();
    assert_eq!(left, [4]);
    assert_eq!(slots_of(&sq).len(), 4);
}

#[test]
fn open_slot_does_not_use_up_specialist() {
    let slot = |roles| Slot {
        roles,
        min: 1,
        max: 1,
        gather: None,
        place: Place::Here,
        kind: GoalKind::Hold,
        start: Start::Now,
        want_tags: 0,
        dist: 1,
        gather_dist: 2,
    };
    let t = Tactic {
        slots: vec![slot(0), slot(ROLE_RANGED)],
        considerations: Vec::new(),
        weight: ONE,
        aborts: Vec::new(),
        trigger: Trigger::None,
        min_skill: 0,
        needs_target: false,
    };
    // The only ranged member is nearest the open slot's wish point.
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, ROLE_RANGED), (2, 5, 0, 0)]);
    sq.add_tactic(t);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    let got: Vec<(u32, u8)> = slots_of(&sq).iter().map(|g| (g.0, g.1)).collect();
    assert_eq!(got, [(1, 1), (2, 0)]);
}

#[test]
fn gather_slot_has_then_pos() {
    let mut t = tactics::rush();
    t.slots[0].gather = Some(Place::Here);
    t.slots[0].start = Start::OnSignal;
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0)]);
    sq.add_tactic(t);
    sq.add_tactic(tactics::rush());
    see(&mut sq, 10, 20, 0, 0);
    sq.set_skill(255);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    // Gather at the leader first, then the target's front.
    let g = sq.pending_plan().unwrap().goals[0].1;
    assert_eq!((g.pos, g.then_pos), (Some([0, 0, 0]), Some([19, 0, 0])));
    // Flank: with spots only near the target both phases take the same
    // spot (each phase picks from all spots).
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0), (2, 1, 0, 0)]);
    sq.add_tactic(tactics::flank(2, 4));
    see(&mut sq, 10, 20, 0, 0);
    sq.set_skill(255);
    let spots = [
        Spot {
            pos: [20, 4, 0],
            tags: 0,
        },
        Spot {
            pos: [20, -4, 0],
            tags: 0,
        },
    ];
    sq.plan(0, &[], &spots, 0).unwrap().unwrap();
    for (_, g) in &sq.pending_plan().unwrap().goals {
        assert!(g.pos.is_some());
        assert_eq!(g.then_pos, g.pos);
    }
    // Without a gather place there is no second position.
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0)]);
    sq.add_tactic(tactics::rush());
    see(&mut sq, 10, 20, 0, 0);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().goals[0].1.then_pos, None);
}

// --- Squad running: orders, check, leader loss, signal ---

use crate::memory::{Message, SOURCE_OWN, SOURCE_TOLD};

/// The member ids of `orders`, sorted.
fn members_of(orders: &[Order]) -> Vec<u32> {
    let mut ids: Vec<u32> = orders.iter().map(|o| o.member).collect();
    ids.sort_unstable();
    ids
}

/// An elite squad of [`four`] with rush and a 2-side flank, enemy 10 at
/// (20, 0); `check(0)` makes the flank, `check(5)` activates it.
fn active_flank() -> (Squad, PlanId) {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    sq.add_tactic(tactics::flank(2, 4));
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    let Ok(Status::Replan(id)) = sq.check(0, &[], &[], 0) else {
        panic!("first check makes a plan");
    };
    assert_eq!(sq.pending_plan().unwrap().tactic, 1);
    assert_eq!(sq.check(5, &[], &[], 0), Ok(Status::Keep));
    assert_eq!(sq.active_plan().unwrap().id, id);
    (sq, id)
}

fn broken(member: u32, plan: PlanId) -> Message {
    Message {
        kind: REPORT_GOAL_BROKEN,
        sender: member,
        data: plan,
        ..Message::default()
    }
}

#[test]
fn plan_waits_think_ticks() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    let id = sq.plan(100, &[], &[], 0).unwrap().unwrap();
    let think = sq.skill_table().think_ticks[3];
    assert_eq!(sq.pending_plan().unwrap().active_at, 100 + think);
    let mut out = Vec::new();
    sq.orders_due(100 + think - 1, &mut out);
    assert!(out.is_empty());
    assert!(sq.active_plan().is_none());
    assert_eq!(sq.pending_plan().unwrap().id, id);
    sq.orders_due(100 + think, &mut out);
    assert!(sq.pending_plan().is_none());
    assert_eq!(sq.active_plan().unwrap().id, id);
    assert_eq!(members_of(&out), [1, 2, 3, 4]);
    assert!(out.iter().all(|o| o.goal.plan == id));
    // A weak squad thinks longer.
    sq.set_skill(0);
    sq.plan(200, &[], &[], 0).unwrap().unwrap();
    assert_eq!(
        sq.pending_plan().unwrap().active_at,
        200 + sq.skill_table().think_ticks[0]
    );
    assert!(sq.skill_table().think_ticks[0] > think);
}

#[test]
fn think_time_wraps() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, u32::MAX - 1);
    let think = sq.skill_table().think_ticks[3];
    sq.plan(u32::MAX - 1, &[], &[], 0).unwrap().unwrap();
    let at = (u32::MAX - 1).wrapping_add(think);
    assert_eq!(sq.pending_plan().unwrap().active_at, at);
    let mut out = Vec::new();
    sq.orders_due(u32::MAX, &mut out);
    sq.orders_due(at.wrapping_sub(1), &mut out);
    assert!(out.is_empty() && sq.active_plan().is_none());
    sq.orders_due(at, &mut out);
    assert_eq!(out.len(), 4);
    assert!(sq.active_plan().is_some());
}

#[test]
fn radio_delay_by_distance() {
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0), (2, 6, 8, 0)]);
    sq.add_tactic(tactics::rush());
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    assert!(sq.set_channel(
        2,
        Channel::Radio {
            base: 3,
            per_unit: 2
        }
    ));
    assert!(!sq.set_channel(99, Channel::None));
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    let at = sq.pending_plan().unwrap().active_at;
    let mut out = Vec::new();
    // The leader has its own order at once.
    sq.orders_due(at, &mut out);
    assert_eq!(members_of(&out), [1]);
    // Distance 10 from the leader: 3 + 10 * 2 ticks.
    out.clear();
    sq.orders_due(at + 22, &mut out);
    assert!(out.is_empty());
    sq.orders_due(at + 23, &mut out);
    assert_eq!(members_of(&out), [2]);
}

#[test]
fn shout_out_of_range_not_delivered() {
    let mut sq = squad_of(
        Metric::Euclid,
        &[(1, 0, 0, 0), (2, 3, 0, 0), (3, 9, 0, 0), (4, 1, 1, 0)],
    );
    sq.add_tactic(tactics::rush());
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    sq.set_channel(2, Channel::Shout { range: 5 });
    sq.set_channel(3, Channel::Shout { range: 5 });
    sq.set_channel(4, Channel::None);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    let at = sq.pending_plan().unwrap().active_at;
    let mut out = Vec::new();
    sq.orders_due(at + 2, &mut out);
    assert_eq!(members_of(&out), [1]);
    // Shout: one tick per unit of distance.
    sq.orders_due(at + 3, &mut out);
    assert_eq!(members_of(&out), [1, 2]);
    // 9 is beyond the shout range, 4 has no channel.
    sq.orders_due(at + 1000, &mut out);
    assert_eq!(members_of(&out), [1, 2]);
}

#[test]
fn hold_bonus_prevents_flicker() {
    let build = |table: SkillTable| {
        let mut sq = squad_of(Metric::Euclid, &four());
        sq.add_tactic(Tactic {
            weight: ONE / 2,
            ..tactics::rush()
        });
        sq.add_tactic(Tactic {
            considerations: vec![Consideration::new(IN_EXTRA as u32, Curve::Linear, 0, 100)],
            weight: ONE,
            ..tactics::rush()
        });
        sq.set_extra_columns(1);
        sq.set_skill_table(table);
        sq.set_skill(255);
        see(&mut sq, 10, 20, 0, 0);
        assert!(matches!(
            sq.check(0, &[0, 40], &[], 0),
            Ok(Status::Replan(_))
        ));
        assert_eq!(sq.pending_plan().unwrap().tactic, 0);
        assert_eq!(sq.check(5, &[0, 40], &[], 0), Ok(Status::Keep));
        assert_eq!(sq.active_plan().unwrap().tactic, 0);
        sq
    };
    // 0.6 does not beat 0.5 by more than the elite bonus (half).
    let mut sq = build(SkillTable::default());
    assert_eq!(sq.check(6, &[0, 60], &[], 0), Ok(Status::Keep));
    assert_eq!(sq.check(7, &[0, 70], &[], 0), Ok(Status::Keep));
    assert!(matches!(
        sq.check(8, &[0, 90], &[], 0),
        Ok(Status::Replan(_))
    ));
    assert_eq!(sq.pending_plan().unwrap().tactic, 1);
    // While the new plan is thought out, no further replan.
    assert_eq!(sq.check(9, &[0, 100], &[], 0), Ok(Status::Keep));
    // Without a bonus, 0.6 is enough.
    let mut sq = build(SkillTable {
        hold_bonus: [0; 4],
        ..SkillTable::default()
    });
    assert!(matches!(
        sq.check(6, &[0, 60], &[], 0),
        Ok(Status::Replan(_))
    ));
}

#[test]
fn abort_rule_replans() {
    let (mut sq, id) = active_flank();
    assert!(sq.report(broken(2, id), 10));
    assert!(sq.report(broken(3, id), 12));
    // A report for another plan does not count.
    assert!(sq.report(broken(4, id + 7), 10));
    assert_eq!(sq.check(9, &[], &[], 0), Ok(Status::Keep));
    assert_eq!(sq.check(10, &[], &[], 0), Ok(Status::Keep));
    let Ok(Status::Replan(next)) = sq.check(12, &[], &[], 0) else {
        panic!("two members off plan abort the flank");
    };
    assert_ne!(next, id);
    // The aborted flank is no candidate any more: rush.
    assert_eq!(sq.pending_plan().unwrap().tactic, 0);
    // The flank stays active until the new plan is ready.
    assert_eq!(sq.active_plan().unwrap().id, id);
    assert_eq!(sq.check(13, &[], &[], 0), Ok(Status::Keep));
}

#[test]
fn leader_death_keeps_plan() {
    let (mut sq, id) = active_flank();
    let mut out = Vec::new();
    sq.orders_due(5, &mut out);
    assert_eq!(out.len(), 4);
    let goals = sq.active_plan().unwrap().goals.clone();
    assert_eq!(sq.leader(), Some(1));
    assert!(sq.set_gone(1));
    // Next active member in slot order: the first one of slot 0.
    let first_of_slot0 = goals.iter().find(|(_, g)| g.slot == 0).unwrap().0;
    assert_eq!(sq.leader(), Some(first_of_slot0));
    assert_eq!(sq.check(6, &[], &[], 0), Ok(Status::Keep));
    let p = sq.active_plan().unwrap();
    assert_eq!((p.id, &p.goals), (id, &goals));
    assert!(sq.pending_plan().is_none());
    out.clear();
    sq.orders_due(1000, &mut out);
    assert!(out.is_empty());
    // Marking the new leader gone on the group directly works the same way.
    sq.group_mut().set_gone(first_of_slot0);
    let next = goals
        .iter()
        .filter(|(m, g)| g.slot == 0 && *m != first_of_slot0)
        .map(|(m, _)| *m)
        .next()
        .unwrap();
    assert_eq!(sq.leader(), Some(next));
}

#[test]
fn report_only_after_arrival() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::fall_back());
    sq.set_skill(255);
    assert!(sq.set_gone(3) && sq.set_gone(4));
    let danger = Message {
        kind: REPORT_DANGER,
        subject: 77,
        pos: [20, 0, 0],
        // The shooter stood at (25, -2).
        strength: 25,
        data: (-2i32) as u32,
        sender: 2,
        ..Message::default()
    };
    assert!(sq.report(danger, 50));
    sq.plan(49, &[], &[], 0).unwrap().unwrap();
    assert!(sq.areas().is_empty());
    assert!(
        sq.pending_plan()
            .unwrap()
            .goals
            .iter()
            .all(|(_, g)| g.pos == Some([0, 0, 0]))
    );
    sq.plan(50, &[], &[], 0).unwrap().unwrap();
    let a = *sq.areas().iter().next().unwrap();
    assert_eq!(
        (a.pos, a.source, a.has_source, a.data),
        ([20, 0, 0], [25, -2, 0], 1, 77)
    );
    assert_eq!(a.radius, sq.danger_area().radius);
    let g = sq.pending_plan().unwrap().goals[0].1;
    assert_eq!(g.pos, Some([-tactics::FALL_BACK_DIST, 0, 0]));
    // An enemy report becomes a target only when it has arrived.
    let enemy = Message {
        kind: REPORT_ENEMY,
        subject: 10,
        pos: [0, 5, 0],
        seen_at: 55,
        sender: 2,
        ..Message::default()
    };
    sq.report(enemy, 60);
    sq.plan(59, &[], &[], 0).unwrap().unwrap();
    assert_eq!(sq.targets().count(), 0);
    sq.plan(60, &[], &[], 0).unwrap().unwrap();
    let f = *sq.targets().next().unwrap();
    assert_eq!((f.subject, f.pos, f.source), (10, [0, 5, 0], SOURCE_TOLD));
    // The leader's own sight counts as its own.
    sq.observe(
        Fact {
            subject: 11,
            kind: ENEMY,
            pos: [3, 3, 0],
            seen_at: 61,
            ..Fact::default()
        },
        61,
    );
    assert_eq!(
        sq.knowledge().get(11, ENEMY).map(|f| f.source),
        Some(SOURCE_OWN)
    );
}

#[test]
fn flank_gather_then_simultaneous_attack() {
    let (mut sq, id) = active_flank();
    let mut first = Vec::new();
    sq.orders_due(5, &mut first);
    assert_eq!(first.len(), 4);
    // Gather first: go there now, only move; the attack comes on the signal.
    assert!(
        first
            .iter()
            .all(|o| (o.goal.kind, o.goal.start) == (GoalKind::Move, Start::Now))
    );
    assert!(first.iter().all(|o| o.goal.then_pos.is_some()));
    for (m, t) in [(4, 6), (2, 7), (3, 8)] {
        assert!(sq.arrived(m, t));
        assert_eq!(sq.check(t, &[], &[], 0), Ok(Status::Keep));
    }
    assert!(!sq.arrived(99, 8));
    assert!(sq.arrived(1, 9));
    assert_eq!(sq.check(9, &[], &[], 0), Ok(Status::Signal));
    assert!(sq.signal_given());
    // Once per plan.
    assert_eq!(sq.check(10, &[], &[], 0), Ok(Status::Keep));
    assert!(!sq.signal(10));
    // Every member gets its attack order, all due at the same tick.
    let mut attack = Vec::new();
    sq.orders_due(9, &mut attack);
    assert_eq!(members_of(&attack), [1, 2, 3, 4]);
    for o in &attack {
        let before = first.iter().find(|f| f.member == o.member).unwrap();
        assert_eq!(o.goal.pos, before.goal.then_pos);
        assert_eq!(o.goal.unchecked, before.goal.then_unchecked);
        assert_eq!(
            (o.goal.kind, o.goal.start, o.goal.plan, o.goal.issued_at),
            (GoalKind::Attack, Start::Now, id, 9)
        );
        assert_eq!(o.goal.start_delay, 0);
    }
    sq.orders_due(1000, &mut attack);
    assert_eq!(attack.len(), 4);
}

#[test]
fn flank_max_wait_fires_signal() {
    let (mut sq, _) = active_flank();
    for m in [4, 2, 3] {
        sq.arrived(m, 6);
    }
    let wait = tactics::FLANK_MAX_WAIT;
    assert_eq!(sq.check(5 + wait - 1, &[], &[], 0), Ok(Status::Keep));
    assert_eq!(sq.check(5 + wait, &[], &[], 0), Ok(Status::Signal));
}

#[test]
fn suspended_member_gets_no_order() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    sq.add_tactic(tactics::flank(2, 4));
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    let Ok(Status::Replan(id)) = sq.check(0, &[], &[], 0) else {
        panic!("plan");
    };
    // Suspended while the plan is thought out.
    sq.group_mut().suspend(3);
    let mut out = Vec::new();
    sq.orders_due(5, &mut out);
    assert_eq!(members_of(&out), [1, 2, 4]);
    // Its broken report does not count as off plan.
    sq.report(broken(3, id), 6);
    sq.report(broken(2, id), 6);
    assert_eq!(sq.check(6, &[], &[], 0), Ok(Status::Keep));
    // All active members arrived: the signal, nothing for 3.
    for m in [1, 2, 4] {
        sq.arrived(m, 7);
    }
    assert_eq!(sq.check(7, &[], &[], 0), Ok(Status::Signal));
    out.clear();
    sq.orders_due(7, &mut out);
    assert_eq!(members_of(&out), [1, 2, 4]);
}

#[test]
fn weak_timing_spread_seeded() {
    let delays = |skill: u8, seed: u64| {
        let mut sq = squad_of(Metric::Euclid, &four());
        sq.add_tactic(tactics::rush());
        sq.set_skill(skill);
        see(&mut sq, 10, 20, 0, 0);
        sq.check(0, &[], &[], seed).unwrap();
        let mut out = Vec::new();
        sq.orders_due(1000, &mut out);
        out.sort_by_key(|o| o.member);
        out.iter().map(|o| o.goal.start_delay).collect::<Vec<u32>>()
    };
    let spread = SkillTable::default().timing_spread[0];
    let mut seen = vec![false; spread as usize + 1];
    for seed in 0..64 {
        assert_eq!(delays(255, seed), [0, 0, 0, 0]);
        let d = delays(0, seed);
        assert_eq!(d, delays(0, seed));
        assert_eq!(d.len(), 4);
        for &x in &d {
            seen[x as usize] = true;
        }
    }
    assert!(seen.iter().filter(|&&s| s).count() > spread as usize / 2);
}

#[test]
fn target_in_radius_signals_ambush() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::ambush());
    sq.set_skill(255);
    see(&mut sq, 10, 30, 0, 0);
    assert!(matches!(sq.check(0, &[], &[], 0), Ok(Status::Replan(_))));
    assert_eq!(sq.check(5, &[], &[], 0), Ok(Status::Keep));
    let mut first = Vec::new();
    sq.orders_due(5, &mut first);
    assert_eq!(first.len(), 4);
    let at = |x: i64, t: u32| Fact {
        subject: 10,
        kind: ENEMY,
        pos: [x as i32, 0, 0],
        seen_at: t,
        ..Fact::default()
    };
    sq.observe(at(tactics::AMBUSH_RADIUS + 1, 6), 6);
    assert_eq!(sq.check(6, &[], &[], 0), Ok(Status::Keep));
    sq.observe(at(tactics::AMBUSH_RADIUS, 7), 7);
    assert_eq!(sq.check(7, &[], &[], 0), Ok(Status::Signal));
    // On-signal goals without a second place get a "go" at the same spot.
    let mut go = Vec::new();
    sq.orders_due(7, &mut go);
    assert_eq!(members_of(&go), [1, 2, 3, 4]);
    for o in &go {
        let before = first.iter().find(|f| f.member == o.member).unwrap();
        assert_eq!((o.goal.pos, o.goal.start), (before.goal.pos, Start::Now));
    }
}

#[test]
fn manual_signal_once() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    assert!(!sq.signal(0));
    sq.check(0, &[], &[], 0).unwrap();
    assert!(sq.signal(5));
    assert!(!sq.signal(6));
    // Rush goals start at once: no further orders.
    let mut out = Vec::new();
    sq.orders_due(1000, &mut out);
    assert_eq!(out.len(), 4);
}

#[test]
fn role_feasibility_is_joint() {
    let slot = |roles| Slot {
        roles,
        min: 1,
        max: 1,
        gather: None,
        place: Place::Here,
        kind: GoalKind::Hold,
        start: Start::Now,
        want_tags: 0,
        dist: 1,
        gather_dist: 2,
    };
    let t = |slots| Tactic {
        slots,
        considerations: Vec::new(),
        weight: ONE,
        aborts: Vec::new(),
        trigger: Trigger::None,
        min_skill: 0,
        needs_target: false,
    };
    // Two slots each need a ranged member; only one is ranged.
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, ROLE_RANGED), (2, 1, 0, 0)]);
    sq.add_tactic(t(vec![slot(ROLE_RANGED), slot(ROLE_RANGED)]));
    assert_eq!(sq.plan(0, &[], &[], 0), Ok(None));
    let mut sq = squad_of(
        Metric::Euclid,
        &[(1, 0, 0, ROLE_RANGED), (2, 1, 0, ROLE_RANGED)],
    );
    sq.add_tactic(t(vec![slot(ROLE_RANGED), slot(ROLE_RANGED)]));
    assert!(sq.plan(0, &[], &[], 0).unwrap().is_some());
    // Nested needs: member 1 can do both, member 2 only ranged. Slot 0
    // (ranged) would take the nearer member 1 and leave slot 1 empty;
    // the pick keeps every minimum reachable.
    let both = ROLE_RANGED | ROLE_AREA;
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, both), (2, 5, 0, ROLE_RANGED)]);
    sq.add_tactic(t(vec![slot(ROLE_RANGED), slot(both)]));
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    let mut got: Vec<(u32, u8)> = slots_of(&sq).iter().map(|g| (g.0, g.1)).collect();
    got.sort_unstable();
    assert_eq!(got, [(1, 1), (2, 0)]);
}

#[test]
fn better_replan_never_restarts_same_pair() {
    let think = SkillTable::default().think_ticks[0];
    for seed in 0..16 {
        let mut sq = squad_of(Metric::Euclid, &four());
        sq.add_tactic(Tactic {
            weight: ONE / 2,
            ..tactics::rush()
        });
        sq.add_tactic(Tactic {
            considerations: vec![Consideration::new(IN_EXTRA as u32, Curve::Linear, 0, 100)],
            weight: ONE,
            ..tactics::rush()
        });
        sq.set_extra_columns(1);
        sq.set_skill(0);
        see(&mut sq, 10, 20, 0, 0);
        // The second tactic scores 0 at first: the weak pick must take rush.
        assert!(matches!(
            sq.check(0, &[0, 0], &[], seed),
            Ok(Status::Replan(_))
        ));
        assert_eq!(sq.check(think, &[0, 0], &[], seed), Ok(Status::Keep));
        assert_eq!(sq.active_plan().unwrap().tactic, 0);
        // 0.6 beats 0.5 by more than the weak bonus; with a fixed seed the
        // chance pick must never restart rush.
        for now in think + 1..think + 200 {
            if let Ok(Status::Replan(_)) = sq.check(now, &[0, 60], &[], seed) {
                assert_eq!(sq.pending_plan().unwrap().tactic, 1, "seed {seed}");
            }
        }
        assert_eq!(sq.active_plan().unwrap().tactic, 1, "seed {seed}");
    }
}

#[test]
fn all_arrived_needs_an_active_member() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::flank(2, 4));
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    assert!(matches!(sq.check(0, &[], &[], 0), Ok(Status::Replan(_))));
    assert_eq!(sq.check(5, &[], &[], 0), Ok(Status::Keep));
    // A newcomer outside the plan keeps the group in play; every plan
    // member is suspended.
    sq.group_mut().add(Member {
        id: 5,
        pos: [0, 0, 0],
        roles: 0,
        skill: 0,
        state: MemberState::Active,
    });
    for m in 1..=4 {
        sq.group_mut().suspend(m);
    }
    let wait = tactics::FLANK_MAX_WAIT;
    assert_eq!(sq.check(5 + wait, &[], &[], 0), Ok(Status::Keep));
    assert!(!sq.signal_given());
    // One plan member back: max_wait has passed, the signal fires.
    sq.group_mut().resume(2);
    assert_eq!(sq.check(6 + wait, &[], &[], 0), Ok(Status::Signal));
}

#[test]
fn no_better_replan_after_signal() {
    let build = || {
        let mut sq = squad_of(Metric::Euclid, &four());
        sq.add_tactic(Tactic {
            weight: ONE / 2,
            aborts: vec![Abort {
                input: IN_LOSSES,
                at_least: 1,
            }],
            ..tactics::rush()
        });
        sq.add_tactic(Tactic {
            considerations: vec![Consideration::new(IN_EXTRA as u32, Curve::Linear, 0, 100)],
            weight: ONE,
            ..tactics::rush()
        });
        sq.set_extra_columns(1);
        sq.set_skill(255);
        see(&mut sq, 10, 20, 0, 0);
        sq.check(0, &[0, 40], &[], 0).unwrap();
        assert_eq!(sq.check(5, &[0, 40], &[], 0), Ok(Status::Keep));
        assert_eq!(sq.active_plan().unwrap().tactic, 0);
        sq
    };
    // Without the signal 0.9 beats 0.5 by more than the bonus ...
    let mut sq = build();
    assert!(matches!(
        sq.check(8, &[0, 90], &[], 0),
        Ok(Status::Replan(_))
    ));
    // ... after it the started attack is carried through.
    let mut sq = build();
    assert!(sq.signal(6));
    assert_eq!(sq.check(8, &[0, 90], &[], 0), Ok(Status::Keep));
    assert_eq!(sq.check(9, &[0, 100], &[], 0), Ok(Status::Keep));
    assert!(sq.pending_plan().is_none());
    // An abort rule still ends it.
    assert!(sq.set_gone(4));
    assert!(matches!(
        sq.check(10, &[0, 100], &[], 0),
        Ok(Status::Replan(_))
    ));
}

#[test]
fn signal_orders_wait_for_the_slowest() {
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0), (2, 3, 0, 0), (3, 6, 0, 0)]);
    sq.add_tactic(tactics::flank(2, 4));
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    let radio = Channel::Radio {
        base: 0,
        per_unit: 1,
    };
    assert!(sq.set_channel(2, radio) && sq.set_channel(3, radio));
    sq.check(0, &[], &[], 0).unwrap();
    sq.check(5, &[], &[], 0).unwrap();
    assert!(sq.signal(6));
    let mut out = Vec::new();
    sq.orders_due(1000, &mut out);
    let mut second: Vec<(u32, Start)> = out
        .iter()
        .filter(|o| o.goal.issued_at == 6)
        .map(|o| (o.member, o.goal.start))
        .collect();
    second.sort_unstable_by_key(|&(m, _)| m);
    // In at 6 (leader), 9 (3 away) and 12 (6 away): all start at 12.
    assert_eq!(
        second,
        [
            (1, Start::AfterTicks(6)),
            (2, Start::AfterTicks(3)),
            (3, Start::Now),
        ]
    );
}

// --- Reference scenes: scripted members over the public API only ---
//
// 17 ticks per second, 4 members, 2 enemies. Each member walks one cell
// per `STEP` ticks toward its goal's `pos` (greedy over the 8 square or 6
// hex neighbours) and calls `arrived` when it stands on its gather spot.
// Per tick: members walk, then `check`, then `orders_due`.

use crate::memory::distance;

/// Ticks per cell of the scripted walk.
const STEP: u32 = 2;
/// Channel of every member but the leader: 2 ticks plus 1 per cell.
const RADIO: Channel = Channel::Radio {
    base: 2,
    per_unit: 1,
};
const SEED: u64 = 7;
/// Enemy the leader sees at (20, 0) in the flank scenes.
const E1: u32 = 10;
/// Enemy member 2 reports in the flank scenes.
const E2: u32 = 11;
/// Members of the square scenes: 1 leads.
const SQUARE_FOUR: [(u32, i32, i32); 4] = [(1, 0, 0), (2, 0, 2), (3, 0, -2), (4, -3, 0)];

/// What happened, for the scene asserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ev {
    Planned { t: u32, tactic: usize },
    Active { t: u32 },
    Order { t: u32, member: u32 },
    Arrived { t: u32, member: u32 },
    Signal { t: u32 },
    Attack { t: u32, member: u32 },
}

/// The scripted body of one member.
struct Walker {
    id: u32,
    goal: Option<Goal>,
    start_at: u32,
    gathered: bool,
    keeper: GoalKeeper,
}

struct Scene {
    sq: Squad,
    metric: Metric,
    walkers: Vec<Walker>,
    spots: Vec<Spot>,
    events: Vec<Ev>,
    /// The target of every order received, in order.
    order_targets: Vec<Option<u32>>,
    out: Vec<Order>,
    active: Option<PlanId>,
}

/// One cell from `from` toward `to`: the neighbour nearest to `to`, ties to
/// the first in the list.
fn step_toward(metric: Metric, from: Pos, to: Pos) -> Pos {
    const SQUARE: [(i32, i32); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];
    const HEX: [(i32, i32); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];
    if from[..2] == to[..2] {
        return from;
    }
    let dirs: &[(i32, i32)] = if metric == Metric::Hex { &HEX } else { &SQUARE };
    let left = |p: Pos| distance(metric, i64::from(to[0] - p[0]), i64::from(to[1] - p[1]));
    dirs.iter()
        .map(|&(dx, dy)| [from[0] + dx, from[1] + dy, from[2]])
        .min_by_key(|&p| left(p))
        .unwrap()
}

impl Scene {
    /// Members `(id, x, y)` of `skill`; the first leads and is told at once,
    /// the others by [`RADIO`]. No tactics yet.
    fn new(metric: Metric, skill: u8, members: &[(u32, i32, i32)], spots: Vec<Spot>) -> Self {
        let mut sq = Squad::new(8, 8, 4, 8, metric).unwrap();
        for &(id, x, y) in members {
            sq.group_mut().add(Member {
                id,
                pos: [x, y, 0],
                roles: 0,
                skill,
                state: MemberState::Active,
            });
        }
        assert!(sq.set_leader(members[0].0));
        for &(id, ..) in &members[1..] {
            assert!(sq.set_channel(id, RADIO));
        }
        sq.set_target_kind(ENEMY);
        sq.set_skill(skill);
        let walkers = members
            .iter()
            .map(|&(id, ..)| Walker {
                id,
                goal: None,
                start_at: 0,
                gathered: false,
                keeper: GoalKeeper::new(),
            })
            .collect();
        Self {
            sq,
            metric,
            walkers,
            spots,
            events: Vec::new(),
            order_targets: Vec::new(),
            out: Vec::new(),
            active: None,
        }
    }

    /// Adds rush (0), formation (line, spacing 2; 1) and flank (3 sides,
    /// distance 4; 2), the game team's three levels.
    fn three_tactics(&mut self) {
        self.sq.add_tactic(tactics::rush());
        self.sq.add_tactic(tactics::formation(Shape::Line, 2));
        self.sq.add_tactic(tactics::flank(3, 4));
    }

    /// The square flank scene on Chebyshev: [`SQUARE_FOUR`], the three
    /// tactics, E1 at (20, 0) and E2 at (24, 10).
    fn square(skill: u8) -> Self {
        let spots = vec![
            spot(16, 0, TAG_COVER),
            spot(16, 2, 0),
            spot(20, 4, TAG_HIGH),
            spot(20, -4, 0),
            spot(12, 8, TAG_COVER),
        ];
        let mut s = Self::new(Metric::Chebyshev, skill, &SQUARE_FOUR, spots);
        s.three_tactics();
        s.enemies_at_start([24, 10, 0]);
        s
    }

    /// The leader's own sighting of E1 at (20, 0) and member 2's report of
    /// E2 at `e2`, both read at tick 0.
    fn enemies_at_start(&mut self, e2: Pos) {
        self.sq.observe(
            Fact {
                subject: E1,
                kind: ENEMY,
                pos: [20, 0, 0],
                ..Fact::default()
            },
            0,
        );
        assert!(self.sq.report(
            Message {
                kind: REPORT_ENEMY,
                subject: E2,
                pos: e2,
                sender: 2,
                ..Message::default()
            },
            0,
        ));
    }

    fn walker(&mut self, id: u32) -> &mut Walker {
        self.walkers.iter_mut().find(|w| w.id == id).unwrap()
    }

    fn tick(&mut self, t: u32) {
        let metric = self.metric;
        for w in &mut self.walkers {
            let i = self.sq.group().index(w.id).unwrap();
            let m = *self.sq.group().get(i).unwrap();
            let Some(goal) = w.goal.filter(|_| m.state == MemberState::Active) else {
                continue;
            };
            let to = goal.pos.unwrap();
            let mut at = m.pos;
            if t > w.start_at && (t - w.start_at).is_multiple_of(STEP) {
                at = step_toward(metric, at, to);
                self.sq.group_mut().set_pos(w.id, at);
            }
            let gathers = goal.then_pos.is_some() || goal.start == Start::OnSignal;
            if gathers && !w.gathered && at[..2] == to[..2] {
                w.gathered = true;
                assert!(self.sq.arrived(w.id, t));
                self.events.push(Ev::Arrived { t, member: w.id });
            }
        }
        match self.sq.check(t, &[], &self.spots, SEED).unwrap() {
            Status::Replan(_) => {
                let tactic = self.sq.pending_plan().unwrap().tactic;
                self.events.push(Ev::Planned { t, tactic });
            }
            Status::Signal => self.events.push(Ev::Signal { t }),
            Status::Keep | Status::NoPlan => {}
        }
        self.sq.orders_due(t, &mut self.out);
        let now_active = self.sq.active_plan().map(|p| p.id);
        if now_active != self.active {
            self.active = now_active;
            self.events.push(Ev::Active { t });
        }
        for o in self.out.drain(..) {
            let w = self.walkers.iter_mut().find(|w| w.id == o.member).unwrap();
            let after = match o.goal.start {
                Start::AfterTicks(n) => n,
                Start::Now | Start::OnSignal => 0,
            };
            w.goal = Some(o.goal);
            w.start_at = t + o.goal.start_delay + after;
            w.gathered = false;
            w.keeper.set(o.goal, t);
            self.order_targets.push(o.goal.target);
            self.events.push(Ev::Order {
                t,
                member: o.member,
            });
        }
        for w in &self.walkers {
            if let Some(g) = w.goal
                && g.kind == GoalKind::Attack
                && g.start != Start::OnSignal
                && w.start_at == t
            {
                self.events.push(Ev::Attack { t, member: w.id });
            }
        }
    }

    fn run(&mut self, ticks: std::ops::RangeInclusive<u32>) {
        for t in ticks {
            self.tick(t);
        }
    }

    /// `(tick, member)` of the events `pick` selects, in order.
    fn pairs(&self, pick: impl Fn(&Ev) -> Option<(u32, u32)>) -> Vec<(u32, u32)> {
        self.events.iter().filter_map(pick).collect()
    }

    fn orders(&self) -> Vec<(u32, u32)> {
        self.pairs(|e| match *e {
            Ev::Order { t, member } => Some((t, member)),
            _ => None,
        })
    }

    fn attacks(&self) -> Vec<(u32, u32)> {
        self.pairs(|e| match *e {
            Ev::Attack { t, member } => Some((t, member)),
            _ => None,
        })
    }

    fn arrivals(&self) -> Vec<(u32, u32)> {
        self.pairs(|e| match *e {
            Ev::Arrived { t, member } => Some((t, member)),
            _ => None,
        })
    }

    fn first(&self, want: impl Fn(&Ev) -> bool) -> Option<Ev> {
        self.events.iter().copied().find(|e| want(e))
    }

    fn plans_made(&self) -> usize {
        self.events
            .iter()
            .filter(|e| matches!(e, Ev::Planned { .. }))
            .count()
    }
}

/// Asserts a flank run of the square or hex scene: one plan, made at 0
/// and active at 5; the first four orders, the gather arrivals, the signal
/// tick and one attack tick for all four members.
fn assert_flank_run(
    s: &Scene,
    orders: &[(u32, u32)],
    arrivals: &[(u32, u32)],
    signal: u32,
    attack: u32,
) {
    const FLANK: usize = 2;
    assert_eq!(
        s.events[0],
        Ev::Planned {
            t: 0,
            tactic: FLANK
        }
    );
    assert_eq!(s.plans_made(), 1, "no replan");
    assert_eq!(
        s.first(|e| matches!(e, Ev::Active { .. })),
        Some(Ev::Active { t: 5 })
    );
    assert_eq!(&s.orders()[..4], orders);
    assert_eq!(s.arrivals(), arrivals);
    assert_eq!(
        s.first(|e| matches!(e, Ev::Signal { .. })),
        Some(Ev::Signal { t: signal })
    );
    let mut hits = s.attacks();
    hits.sort_unstable_by_key(|&(_, m)| m);
    assert_eq!(hits, [(attack, 1), (attack, 2), (attack, 3), (attack, 4)]);
}

#[test]
fn three_levels() {
    const RUSH: usize = 0;
    const FORMATION: usize = 1;
    const FLANK: usize = 2;

    // Skill 0 (level 0) knows only rush (formation needs 64, flank 128):
    // (rush, E1) and (rush, E2) are the only candidates, so rush wins for
    // any seed.
    let mut weak = Scene::square(0);
    weak.run(0..=40);
    assert_eq!(weak.events[0], Ev::Planned { t: 0, tactic: RUSH });
    // Made at 0, think_ticks[0] = 34: active at 34.
    assert_eq!(
        weak.first(|e| matches!(e, Ev::Active { .. })),
        Some(Ev::Active { t: 34 })
    );
    // The leader at once (34); 2 and 3 are 2 cells from it: 34 + 2 + 2 =
    // 38; 4 is 3 cells away: 34 + 2 + 3 = 39.
    assert_eq!(weak.orders(), [(34, 1), (38, 2), (38, 3), (39, 4)]);
    // Weak members start apart (timing spread up to 17 ticks, by seed).
    let late = weak
        .walkers
        .iter()
        .zip([34, 38, 38, 39])
        .filter(|(w, got)| w.start_at > *got)
        .count();
    assert!(late > 0);

    // Skill 64 (level 1) also knows formation: weight 1/2 against rush's
    // 1/4. Level 1 picks by chance among the best 2, and with two known
    // targets those are (formation, E1) and (formation, E2), so formation
    // wins for any seed (only the target is by chance).
    let mut mid = Scene::square(64);
    mid.run(0..=30);
    assert_eq!(
        mid.events[0],
        Ev::Planned {
            t: 0,
            tactic: FORMATION
        }
    );
    // Made at 0, think_ticks[1] = 20: active at 20; delays as above.
    assert_eq!(
        mid.first(|e| matches!(e, Ev::Active { .. })),
        Some(Ev::Active { t: 20 })
    );
    assert_eq!(mid.orders(), [(20, 1), (24, 2), (24, 3), (25, 4)]);
    assert!(mid.attacks().is_empty(), "formation only moves");

    // Skill 192 (level 3, elite) knows flank: weight 3/4 beats formation
    // and rush, and level 3 picks the best; (flank, E1) comes before
    // (flank, E2) on the tie.
    let mut elite = Scene::square(192);
    elite.run(0..=60);
    let p = elite.sq.active_plan().unwrap();
    assert_eq!((p.tactic, p.target), (FLANK, Some(E1)));
    // Centre (-1, 0), heading +x: front (16, 0), left (20, 4), right
    // (20, -4). Front takes 1 (nearest, lower index on the tie), left 2,
    // right 3, then front 4 (spot (16, 2), next to the taken (16, 0)).
    let goals: Vec<(u32, u8, Option<Pos>)> =
        p.goals.iter().map(|(m, g)| (*m, g.slot, g.pos)).collect();
    assert_eq!(
        goals,
        [
            (1, 0, Some([16, 0, 0])),
            (2, 1, Some([20, 4, 0])),
            (3, 2, Some([20, -4, 0])),
            (4, 0, Some([16, 2, 0])),
        ]
    );
    assert_flank_run(
        &elite,
        // Made at 0, think_ticks[3] = 5: active at 5; the leader at once,
        // 2 and 3 at 5 + 2 + 2 = 9, 4 at 5 + 2 + 3 = 10.
        &[(5, 1), (9, 2), (9, 3), (10, 4)],
        // One cell per 2 ticks from the order: 1 walks 16 cells to (16, 0):
        // 5 + 32 = 37; 4 walks 19 to (16, 2): 10 + 38 = 48; 2 and 3 walk 20
        // to (20, +-4): 9 + 40 = 49.
        &[(37, 1), (48, 4), (49, 2), (49, 3)],
        // The last arrival at 49 fires AllArrived in that tick's check.
        49,
        // The attack orders leave the leader at (16, 0) at 49: 4 is 2 cells
        // away (in at 53), 2 and 3 are 4 away (55). Each is told to start
        // when the slowest order is in: 55.
        55,
    );
}

#[test]
fn hex_scene() {
    // The three_levels flank on axial hex coordinates.
    let spots = vec![
        spot(16, 0, 0),
        spot(16, 1, 0),
        spot(20, 4, 0),
        spot(24, -4, 0),
    ];
    let mut s = Scene::new(
        Metric::Hex,
        192,
        &[(1, 0, 0), (2, -1, 2), (3, 1, -2), (4, -3, 0)],
        spots,
    );
    s.three_tactics();
    s.enemies_at_start([24, 10, 0]);
    s.run(0..=70);
    // Centre (-1, 0), heading step 0 (+q): front = step 3 -> (16, 0), left
    // = step 1 -> (20, 4), right = step 5 -> (24, -4), 120 deg apart.
    let p = s.sq.active_plan().unwrap();
    let goals: Vec<(u32, u8, Option<Pos>)> =
        p.goals.iter().map(|(m, g)| (*m, g.slot, g.pos)).collect();
    assert_eq!(
        goals,
        [
            (1, 0, Some([16, 0, 0])),
            (2, 1, Some([20, 4, 0])),
            (3, 2, Some([24, -4, 0])),
            (4, 0, Some([16, 1, 0])),
        ]
    );
    assert_flank_run(
        &s,
        // Active at 5; 2 and 3 are 2 hex steps from the leader (9), 4 is 3
        // (10).
        &[(5, 1), (9, 2), (9, 3), (10, 4)],
        // 1 walks 16 steps: 37; 4 walks 20 to (16, 1): 10 + 40 = 50; 2 and
        // 3 walk 23: 9 + 46 = 55.
        &[(37, 1), (50, 4), (55, 2), (55, 3)],
        55,
        // From the leader at (16, 0): 4 is 1 step (in at 58), 2 and 3 are 8
        // (65); all start at 65.
        65,
    );
}

#[test]
fn game_orders_first() {
    let mut s = Scene::square(192);
    s.run(0..=19);
    // Mid-flank (3 walks toward (20, -4)) the game takes member 3 for its
    // own order: suspended in the group, its goal postponed and its
    // keeper suspended.
    assert!(s.sq.group_mut().suspend(3));
    let w = s.walker(3);
    w.keeper.postpone(20);
    w.keeper.suspend();
    let give_up = SkillTable::default().give_up_ticks[SkillTable::level(192)];
    assert!(150 > 20 + give_up);
    for t in 20..=150 {
        s.tick(t);
        for w in &s.walkers {
            // A member reports its goal broken only once its keeper says
            // so; a suspended keeper never does.
            assert!(!w.keeper.broken(t, give_up), "member {} at {t}", w.id);
        }
    }
    // Not off plan: the plan made at 0 stays, nothing replans.
    assert_eq!(s.plans_made(), 1);
    // AllArrived counts only the active members: 1 at 37, 4 at 48 and 2 at
    // 49 (as in three_levels) fire the signal at 49, long before max_wait.
    assert_eq!(s.arrivals(), [(37, 1), (48, 4), (49, 2)]);
    assert_eq!(
        s.first(|e| matches!(e, Ev::Signal { .. })),
        Some(Ev::Signal { t: 49 })
    );
    // The rest attacks together: 4 is 2 cells from the leader (53), 2 is 4
    // (55); all start at 55. Member 3 gets nothing more.
    assert_eq!(s.attacks(), [(55, 1), (55, 2), (55, 4)]);
    assert!(!s.orders().iter().any(|&(t, m)| m == 3 && t >= 20));
}

#[test]
fn ambush_on_target_in_radius() {
    // Two enemies walk in along y = 0, one cell per 2 ticks: E1 from x 40,
    // E2 4 cells behind it. The leader sees both every tick.
    let hidden = [
        spot(1, 3, TAG_HIDDEN),
        spot(1, -3, TAG_HIDDEN),
        spot(-3, 3, TAG_HIDDEN),
        spot(-3, -3, TAG_HIDDEN),
        spot(-1, 0, 0),
    ];
    let mut s = Scene::new(Metric::Chebyshev, 192, &SQUARE_FOUR, hidden.to_vec());
    let ambush = s.sq.add_tactic(tactics::ambush());
    let x_at = |start: i32, t: u32| start - (t / STEP) as i32;
    // Well past the attack: E2 walks through the group and E1 would then
    // score higher, but a started attack is carried through.
    for t in 0..=200 {
        for (id, start) in [(E1, 40), (E2, 44)] {
            s.sq.observe(
                Fact {
                    subject: id,
                    kind: ENEMY,
                    pos: [x_at(start, t), 0, 0],
                    seen_at: t,
                    ..Fact::default()
                },
                t,
            );
        }
        s.tick(t);
    }
    assert_eq!(
        s.events[0],
        Ev::Planned {
            t: 0,
            tactic: ambush
        }
    );
    assert_eq!(s.plans_made(), 1);
    // Ambush scores higher the farther the target still is: E2.
    let p = s.sq.active_plan().unwrap();
    assert_eq!(p.target, Some(E2));
    // Every member holds a hidden spot (the untagged one stays free).
    for (_, g) in &p.goals {
        let i = hidden.iter().position(|h| Some(h.pos) == g.pos).unwrap();
        assert_eq!(hidden[i].tags, TAG_HIDDEN);
    }
    // Once all sit on their spots the centre is (-1, 0). E2 is at
    // 44 - t / 2, within the radius 6 once 45 - t / 2 <= 6: t = 78.
    assert_eq!(
        s.first(|e| matches!(e, Ev::Signal { .. })),
        Some(Ev::Signal { t: 78 })
    );
    // No attack before the signal. The "go" orders leave the leader at
    // (1, 3): 3 at (-3, 3) is 4 cells away (82 + 2 = 84), 2 at (1, -3) and
    // 4 at (-3, -3) are 6 (86); all start at 86.
    assert_eq!(s.attacks(), [(86, 1), (86, 2), (86, 3), (86, 4)]);
}

#[test]
fn no_tags_scene() {
    // The game knows no tags: every spot has tags 0.
    let plain = [
        spot(16, 0, 0),
        spot(15, 1, 0),
        spot(20, 4, 0),
        spot(19, 5, 0),
        spot(20, -4, 0),
        spot(21, -5, 0),
    ];
    let mut s = Scene::square(192);
    s.spots = plain.to_vec();
    s.run(0..=5);
    let p = s.sq.active_plan().unwrap();
    let mut sides = [0usize; 3];
    for (_, g) in &p.goals {
        sides[usize::from(g.slot)] += 1;
        assert!(!g.unchecked && !g.then_unchecked);
        let pos = g.pos.unwrap();
        assert!(plain.iter().any(|sp| sp.pos == pos));
        // Front faces the group (x < 20), left is +y, right is -y.
        match g.slot {
            0 => assert!(pos[0] < 20),
            1 => assert!(pos[1] > 0),
            _ => assert!(pos[1] < 0),
        }
    }
    assert_eq!(sides, [2, 1, 1], "three sides, the front takes the fourth");

    // Without any spot the flank still plans, on unchecked wish points:
    // the gather points, twice the attack distance from the target.
    let mut bare = Scene::square(192);
    bare.spots.clear();
    bare.run(0..=5);
    let p = bare.sq.active_plan().unwrap();
    let wish: Vec<(u8, Option<Pos>, bool)> = p
        .goals
        .iter()
        .map(|(_, g)| (g.slot, g.pos, g.unchecked))
        .collect();
    assert_eq!(
        wish,
        [
            (0, Some([12, 0, 0]), true),
            (1, Some([20, 8, 0]), true),
            (2, Some([20, -8, 0]), true),
            (0, Some([12, 0, 0]), true),
        ]
    );
    let then: Vec<Option<Pos>> = p.goals.iter().map(|(_, g)| g.then_pos).collect();
    assert_eq!(
        then,
        [
            Some([16, 0, 0]),
            Some([20, 4, 0]),
            Some([20, -4, 0]),
            Some([16, 0, 0])
        ]
    );

    // Every shipped tactic with a target plans on tag-0 spots only: a grid
    // of them, so every wish point has free spots within its radius.
    let grid: Vec<Spot> = (-6..=24)
        .step_by(2)
        .flat_map(|x| (-8..=12).step_by(2).map(move |y| spot(x, y, 0)))
        .collect();
    for tactic in [
        tactics::rush(),
        tactics::formation(Shape::Wedge, 2),
        tactics::flank(2, 4),
        tactics::ambush(),
        tactics::focus(),
        tactics::isolate(),
    ] {
        let mut one = Scene::new(Metric::Chebyshev, 255, &SQUARE_FOUR, grid.clone());
        one.sq.add_tactic(tactic);
        one.enemies_at_start([24, 10, 0]);
        one.run(0..=5);
        let p = one.sq.active_plan().unwrap();
        assert_eq!(p.goals.len(), 4);
        assert!(p.goals.iter().all(|(_, g)| !g.unchecked));
    }
}

#[test]
fn no_cheat() {
    // The true world: E1 and E2 in the open, E3 behind a wall right next
    // to the group, the nearest of all and so the best focus target.
    const E3: u32 = 12;
    const SIGHT: i128 = 30;
    let world: [(u32, Pos, bool); 3] = [
        (E1, [20, 0, 0], true),
        (E2, [24, 10, 0], true),
        (E3, [2, 1, 0], false),
    ];
    let run = |see_hidden: bool| {
        let mut s = Scene::new(Metric::Chebyshev, 192, &SQUARE_FOUR, Vec::new());
        s.sq.add_tactic(tactics::focus());
        let mut targets = Vec::new();
        for t in 0..=150 {
            // Members see what is in the open and within sight; the leader
            // observes, the others report by radio once a second.
            let leader = s.sq.leader().unwrap();
            let members: Vec<Member> = s.sq.group().iter().copied().collect();
            let lpos = members.iter().find(|m| m.id == leader).unwrap().pos;
            for m in &members {
                for &(id, pos, open) in &world {
                    let d = distance(
                        Metric::Chebyshev,
                        i64::from(pos[0] - m.pos[0]),
                        i64::from(pos[1] - m.pos[1]),
                    );
                    if !(open || see_hidden) || d > SIGHT {
                        continue;
                    }
                    if m.id == leader {
                        let fact = Fact {
                            subject: id,
                            kind: ENEMY,
                            pos,
                            seen_at: t,
                            ..Fact::default()
                        };
                        s.sq.observe(fact, t);
                    } else if t % 17 == 0 {
                        let at = RADIO.arrival(Metric::Chebyshev, m.pos, lpos, t).unwrap();
                        let msg = Message {
                            kind: REPORT_ENEMY,
                            subject: id,
                            pos,
                            seen_at: t,
                            sender: m.id,
                            ..Message::default()
                        };
                        s.sq.report(msg, at);
                    }
                }
            }
            s.tick(t);
            assert!(see_hidden || s.sq.knowledge().iter().all(|f| f.subject != E3));
            if let Some(p) = s.sq.active_plan() {
                targets.push(p.target);
            }
            if let Some(p) = s.sq.pending_plan() {
                targets.push(p.target);
            }
        }
        targets.extend(s.order_targets.iter().copied());
        targets
    };
    // Control: once seen, E3 is the target.
    assert!(run(true).contains(&Some(E3)));
    // Never seen nor reported: never the target of a plan or an order.
    let targets = run(false);
    assert!(!targets.is_empty());
    assert!(targets.iter().all(|&t| t == Some(E1) || t == Some(E2)));
}

// --- Final review fixes ---

#[test]
fn lost_target_ends_the_plan() {
    // After the signal the plan holds, but not once its target is gone
    // from the leader's knowledge.
    let (mut sq, id) = active_flank();
    assert!(sq.signal(6));
    see(&mut sq, 11, 30, 0, 6);
    assert!(sq.knowledge_mut().remove(10, ENEMY));
    let Ok(Status::Replan(next)) = sq.check(7, &[], &[], 0) else {
        panic!("a lost target ends the plan");
    };
    assert_ne!(next, id);
    assert_eq!(sq.pending_plan().unwrap().target, Some(11));
    // With no target left nothing can be planned: the plan is dropped.
    let (mut sq, _) = active_flank();
    assert!(sq.signal(6));
    sq.knowledge_mut().clear();
    assert_eq!(sq.check(7, &[], &[], 0), Ok(Status::NoPlan));
    assert!(sq.active_plan().is_none());
    // A target not seen for 255 ticks ends the shipped tactics too.
    let (mut sq, _) = active_flank();
    assert_eq!(sq.check(254, &[], &[], 0), Ok(Status::Signal));
    assert!(matches!(sq.check(255, &[], &[], 0), Ok(Status::Replan(_))));
    assert_eq!(sq.pending_plan().unwrap().tactic, 0, "rush has no abort");
}

#[test]
fn better_replan_takes_only_clearly_better() {
    // A (1/2), then B (1.0) and C (0.1): the weak bonus is 1/8, so only B
    // beats A; C must never be picked, for any constant seed.
    let think = SkillTable::default().think_ticks[0];
    for seed in 0..32 {
        let mut sq = squad_of(Metric::Euclid, &four());
        let a = sq.add_tactic(Tactic {
            weight: ONE / 2,
            ..tactics::rush()
        });
        let column = |c: usize| Tactic {
            considerations: vec![Consideration::new(
                (IN_EXTRA + c) as u32,
                Curve::Linear,
                0,
                100,
            )],
            weight: ONE,
            ..tactics::rush()
        };
        let b = sq.add_tactic(column(0));
        sq.add_tactic(column(1));
        sq.set_extra_columns(2);
        sq.set_skill(0);
        see(&mut sq, 10, 20, 0, 0);
        assert!(matches!(
            sq.check(0, &[0, 0, 0, 0], &[], seed),
            Ok(Status::Replan(_))
        ));
        assert_eq!(sq.check(think, &[0, 0, 0, 0], &[], seed), Ok(Status::Keep));
        assert_eq!(sq.active_plan().unwrap().tactic, a);
        for now in think + 1..think + 300 {
            if let Ok(Status::Replan(_)) = sq.check(now, &[0, 0, 100, 10], &[], seed) {
                assert_eq!(sq.pending_plan().unwrap().tactic, b, "seed {seed} at {now}");
            }
        }
        assert_eq!(sq.active_plan().unwrap().tactic, b, "seed {seed}");
    }
}

#[test]
fn second_order_never_overtakes_the_first() {
    // Member 2 is far when the ambush becomes active (its order needs 42
    // ticks), then steps next to the leader; the signal right after must
    // not reach it before the first order would have.
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0), (2, 40, 0, 0)]);
    sq.add_tactic(tactics::ambush());
    sq.set_skill(255);
    see(&mut sq, 10, 60, 0, 0);
    assert!(sq.set_channel(2, RADIO));
    assert!(matches!(sq.check(0, &[], &[], 0), Ok(Status::Replan(_))));
    let mut got = Vec::new();
    let mut out = Vec::new();
    sq.orders_due(5, &mut out);
    got.extend(out.drain(..).map(|o| (5, o)));
    assert!(sq.group_mut().set_pos(2, [1, 0, 0]));
    assert!(sq.signal(6));
    for t in 6..=100 {
        sq.orders_due(t, &mut out);
        got.extend(out.drain(..).map(|o| (t, o)));
    }
    let two: Vec<(u32, u32)> = got
        .iter()
        .filter(|(_, o)| o.member == 2)
        .map(|(t, o)| (*t, o.goal.issued_at))
        .collect();
    // One order only, the signal's (issued at 6), when the first would
    // have come: 5 + 2 + 40.
    assert_eq!(two, [(47, 6)]);
    // The leader waits for it: its own second order starts then too.
    let lead = got
        .iter()
        .find(|(_, o)| o.member == 1 && o.goal.issued_at == 6)
        .unwrap();
    assert_eq!((lead.0, lead.1.goal.start), (6, Start::AfterTicks(41)));
}

#[test]
fn areas_expire_inside_the_squad() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::fall_back());
    sq.set_skill(255);
    assert!(sq.set_gone(3) && sq.set_gone(4));
    sq.areas_mut().add(
        Area {
            pos: [20, 0, 0],
            radius: 3,
            lifetime: 10,
            ..Area::default()
        },
        0,
    );
    sq.plan(9, &[], &[], 0).unwrap().unwrap();
    let g = sq.pending_plan().unwrap().goals[0].1;
    assert_eq!(g.pos, Some([-tactics::FALL_BACK_DIST, 0, 0]));
    // Its lifetime is over: no danger left, the members stay.
    sq.plan(10, &[], &[], 0).unwrap().unwrap();
    assert!(sq.areas().is_empty());
    let g = sq.pending_plan().unwrap().goals[0].1;
    assert_eq!(g.pos, Some([0, 0, 0]));
}

#[test]
fn fall_back_does_not_take_cover_near_the_enemy() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::fall_back());
    sq.set_skill(255);
    assert!(sq.set_gone(3) && sq.set_gone(4));
    see(&mut sq, 10, 10, 0, 0);
    // Cover right at the enemy, a plain spot near the retreat point.
    let spots = [spot(9, 0, TAG_COVER), spot(-7, 0, 0)];
    sq.plan(0, &[], &spots, 0).unwrap().unwrap();
    let got: Vec<Option<Pos>> = sq
        .pending_plan()
        .unwrap()
        .goals
        .iter()
        .map(|(_, g)| g.pos)
        .collect();
    assert_eq!(got, [Some([-7, 0, 0]), Some([-8, 0, 0])]);
}

#[test]
fn resumed_member_rejoins_after_signal() {
    // As game_orders_first, but member 3 comes back after the signal.
    let mut s = Scene::square(192);
    s.run(0..=19);
    assert!(s.sq.group_mut().suspend(3));
    s.run(20..=59);
    assert_eq!(
        s.first(|e| matches!(e, Ev::Signal { .. })),
        Some(Ev::Signal { t: 49 })
    );
    assert!(s.sq.resume(3, 60));
    assert!(!s.sq.resume(99, 60));
    s.run(60..=120);
    // It gets the attack order by radio and attacks.
    let late: Vec<(u32, u32)> = s
        .orders()
        .into_iter()
        .filter(|&(t, m)| m == 3 && t >= 60)
        .collect();
    assert_eq!(late.len(), 1, "one order after the resume");
    assert!(s.attacks().iter().any(|&(t, m)| m == 3 && t >= late[0].0));
}

#[test]
fn resume_before_signal_resends_the_gather_order() {
    let (mut sq, id) = active_flank();
    let mut out = Vec::new();
    sq.orders_due(5, &mut out);
    assert!(sq.group_mut().suspend(3));
    assert!(sq.resume(3, 6));
    out.clear();
    sq.orders_due(6, &mut out);
    assert_eq!(members_of(&out), [3]);
    let g = out[0].goal;
    assert_eq!((g.kind, g.start, g.plan), (GoalKind::Move, Start::Now, id));
    assert!(g.then_pos.is_some());
    // A member that was not suspended gets nothing again.
    assert!(sq.resume(2, 7));
    sq.orders_due(7, &mut out);
    assert_eq!(members_of(&out), [3]);
    // Without an active plan a resume only puts the member back.
    let mut sq = squad_of(Metric::Euclid, &four());
    assert!(sq.group_mut().suspend(2));
    assert!(sq.resume(2, 0));
    assert_eq!(sq.group().get(1).unwrap().state, MemberState::Active);
}

#[test]
fn arrival_reports_count_once_in() {
    let arrived = |member: u32, plan: PlanId| Message {
        kind: REPORT_ARRIVED,
        sender: member,
        data: plan,
        ..Message::default()
    };
    // Sent at 6, in at 20; a sender that is no member does not count.
    let (mut sq, id) = active_flank();
    for m in 1..=4 {
        assert!(sq.report(arrived(m, id), 20));
    }
    assert!(sq.report(arrived(9, id), 7));
    assert_eq!(sq.check(19, &[], &[], 0), Ok(Status::Keep));
    assert_eq!(sq.check(20, &[], &[], 0), Ok(Status::Signal));
    // A report for another plan does not count.
    let (mut sq, id) = active_flank();
    for m in 1..=4 {
        assert!(sq.report(arrived(m, id + 1), 7));
    }
    assert_eq!(sq.check(8, &[], &[], 0), Ok(Status::Keep));
    assert_eq!(REPORT_ARRIVED, 4);
}

#[test]
fn pick_spot_radius_bounds_the_search() {
    // A tagged spot beyond the radius does not count; the nearest free
    // spot within does; with none within, the wish point.
    let spots = [spot(10, 0, TAG_COVER), spot(2, 0, 0), spot(3, 3, 0)];
    let w = [0, 0, 0];
    let pick = |r| pick_spot(w, TAG_COVER, &spots, &[], 0, Metric::Euclid, r);
    assert_eq!(pick(FAR), SpotPick::Spot(0));
    assert_eq!(pick(3), SpotPick::Spot(1));
    assert_eq!(pick(1), SpotPick::Unchecked(w));
    assert_eq!(pick(-1), SpotPick::Unchecked(w));
    // Euclid compares squares: (3, 3) is 18 > 9 away.
    let taken = [false, true];
    assert_eq!(
        pick_spot(w, 0, &spots, &taken, 0, Metric::Euclid, 3),
        SpotPick::Unchecked(w)
    );
    // Chebyshev: (3, 3) is 3 away, inclusive.
    assert_eq!(
        pick_spot(w, 0, &spots, &taken, 0, Metric::Chebyshev, 3),
        SpotPick::Spot(2)
    );
    // A spot error never reaches past the radius.
    assert_eq!(
        pick_spot(w, 0, &spots, &[], 9, Metric::Chebyshev, 3),
        SpotPick::Spot(2)
    );
}

#[test]
fn squad_spot_radius_setting() {
    let mut sq = squad_of(Metric::Euclid, &[(1, 0, 0, 0)]);
    sq.add_tactic(tactics::rush());
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    assert_eq!(sq.spot_radius(), None);
    // Rush's wish is (19, 0); its default spot radius is 3.
    let spots = [spot(14, 0, 0)];
    sq.plan(0, &[], &spots, 0).unwrap().unwrap();
    let g = sq.pending_plan().unwrap().goals[0].1;
    assert_eq!((g.pos, g.unchecked), (Some([19, 0, 0]), true));
    sq.set_spot_radius(Some(5));
    assert_eq!(sq.spot_radius(), Some(5));
    sq.plan(0, &[], &spots, 0).unwrap().unwrap();
    let g = sq.pending_plan().unwrap().goals[0].1;
    assert_eq!((g.pos, g.unchecked), (Some([14, 0, 0]), false));
    sq.set_spot_radius(None);
    assert_eq!(sq.spot_radius(), None);
}

#[test]
fn flank_gathers_off_the_attack_spot() {
    let f = tactics::flank(3, 4);
    assert!(f.slots.iter().all(|s| (s.dist, s.gather_dist) == (4, 8)));
    // Gather spots near the gather points, attack spots near the target.
    let mut sq = squad_of(Metric::Chebyshev, &[(1, 0, 0, 0), (2, 0, 2, 0)]);
    sq.add_tactic(tactics::flank(2, 4));
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    let spots = [
        spot(20, 9, 0),
        spot(20, -9, 0),
        spot(20, 4, 0),
        spot(20, -4, 0),
    ];
    sq.plan(0, &[], &spots, 0).unwrap().unwrap();
    let mut got: Vec<(Option<Pos>, Option<Pos>)> = sq
        .pending_plan()
        .unwrap()
        .goals
        .iter()
        .map(|(_, g)| (g.pos, g.then_pos))
        .collect();
    got.sort_unstable();
    assert_eq!(
        got,
        [
            (Some([20, -9, 0]), Some([20, -4, 0])),
            (Some([20, 9, 0]), Some([20, 4, 0])),
        ]
    );
}

#[test]
fn isolate_scores_the_friend_distance() {
    // No caller columns: the built-in friend distance scores isolate.
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::isolate());
    sq.set_skill(255);
    see(&mut sq, 10, 20, 0, 0);
    // Alone: no friend, isolate scores 0.
    assert_eq!(sq.plan(0, &[], &[], 0), Ok(None));
    see(&mut sq, 11, 22, 0, 0);
    see(&mut sq, 12, 20, 30, 0);
    sq.plan(0, &[], &[], 0).unwrap().unwrap();
    assert_eq!(sq.pending_plan().unwrap().target, Some(12));
}

#[test]
fn extra_columns_are_bounded() {
    let mut sq = squad_of(Metric::Euclid, &four());
    assert!(sq.set_extra_columns(MAX_EXTRA_COLUMNS));
    assert!(!sq.set_extra_columns(MAX_EXTRA_COLUMNS + 1));
    assert!(!sq.set_extra_columns(usize::MAX));
    // The refused counts changed nothing: the last column still reads.
    let mut t = tactics::rush();
    t.considerations.push(Consideration::new(
        (IN_EXTRA + MAX_EXTRA_COLUMNS - 1) as u32,
        Curve::Linear,
        0,
        1,
    ));
    sq.add_tactic(t);
    sq.set_skill(255);
    assert_eq!(sq.validate(), Ok(()));
    see(&mut sq, 10, 20, 0, 0);
    // One block for no target, then the target's block.
    let mut extra = vec![0; 2 * MAX_EXTRA_COLUMNS];
    assert_eq!(sq.plan(0, &extra, &[], 0), Ok(None));
    extra[2 * MAX_EXTRA_COLUMNS - 1] = 1;
    assert!(sq.plan(0, &extra, &[], 0).unwrap().is_some());
}

#[test]
fn validate_rejects_more_than_255_slots() {
    let mut sq = squad_of(Metric::Euclid, &four());
    sq.add_tactic(tactics::rush());
    let mut big = tactics::rush();
    big.slots = vec![big.slots[0]; 256];
    big.slots.iter_mut().for_each(|s| s.min = 0);
    let t = sq.add_tactic(big.clone());
    let err = PlanError::TooManySlots { tactic: t };
    assert_eq!(sq.validate(), Err(err));
    assert_eq!(sq.plan(0, &[], &[], 0), Err(err));
    assert_eq!(err.to_string(), "tactic 1 has more than 255 slots");
    let mut ok = squad_of(Metric::Euclid, &four());
    big.slots.truncate(255);
    ok.add_tactic(big);
    assert_eq!(ok.validate(), Ok(()));
}

#[test]
fn gather_negative_radius_and_zero_max() {
    let c = [near(1, 7, 0, 0, true), near(2, 7, 1, 0, true)];
    let mut out = vec![99];
    gather([0; 3], 7, -1, Metric::Chebyshev, false, &c, &mut out, 10);
    assert!(out.is_empty(), "a negative radius reaches nobody");
    out.push(99);
    gather([0; 3], 7, 10, Metric::Chebyshev, false, &c, &mut out, 0);
    assert!(out.is_empty(), "max 0 takes nobody");
    gather([0; 3], 7, 0, Metric::Chebyshev, false, &c, &mut out, 10);
    assert_eq!(out, [1], "radius 0 is inclusive");
}
