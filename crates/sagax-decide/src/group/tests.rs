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
        pick_spot(o, 2, &spots, &[], 0, Metric::Euclid),
        SpotPick::Spot(2)
    );
    // Both bits wanted: only 2 and 3 have 2|4.
    assert_eq!(
        pick_spot([6, 0, 0], 6, &spots, &[], 0, Metric::Euclid),
        SpotPick::Spot(2)
    );
    // No spot fits: nearest free spot.
    assert_eq!(
        pick_spot(o, 8, &spots, &[], 0, Metric::Euclid),
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
            let pick = pick_spot([1, 1, 0], want, &spots, &[], 0, Metric::Chebyshev);
            assert_eq!(pick, SpotPick::Spot(1));
        }
    }
}

#[test]
fn pick_spot_no_spots_unchecked() {
    let wish = [4, 5, 6];
    assert_eq!(
        pick_spot(wish, 1, &[], &[], 0, Metric::Hex),
        SpotPick::Unchecked(wish)
    );
    // All taken counts as none.
    let spots = [spot(0, 0, 1)];
    assert_eq!(
        pick_spot(wish, 0, &spots, &[true], 0, Metric::Hex),
        SpotPick::Unchecked(wish)
    );
}

#[test]
fn pick_spot_skips_taken() {
    let spots = [spot(1, 0, 1), spot(2, 0, 1), spot(3, 0, 0)];
    let w = [0, 0, 0];
    assert_eq!(
        pick_spot(w, 1, &spots, &[true], 0, Metric::Manhattan),
        SpotPick::Spot(1)
    );
    assert_eq!(
        pick_spot(w, 1, &spots, &[true, true], 0, Metric::Manhattan),
        SpotPick::Spot(2)
    );
}

#[test]
fn pick_spot_skip_takes_next_best() {
    let spots = [spot(3, 0, 0), spot(1, 0, 1), spot(2, 0, 1), spot(1, 0, 0)];
    let w = [0, 0, 0];
    let pick = |skip| pick_spot(w, 1, &spots, &[], skip, Metric::Euclid);
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
        pick_spot(w, 1, &spots, &taken, 1, Metric::Euclid),
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
    // the spot at 16, then 15, 14, ...
    let spots: Vec<Spot> = (0..6)
        .map(|i| Spot {
            pos: [16 - i, 0, 0],
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
        assert_eq!(first_spot(255, seed), [16, 0, 0]);
        let p = first_spot(0, seed);
        assert_eq!(p, first_spot(0, seed));
        ranks[(16 - p[0]) as usize] = true;
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
            IN_EXTRA
        ],
        [0, 1, 2, 3, 4, 5, 6, 7]
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
    // Flank: gather and attack point coincide; both phases may take the
    // same spot.
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
