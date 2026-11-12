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
