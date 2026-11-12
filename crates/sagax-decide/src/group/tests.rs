use super::*;
use crate::memory::{Metric, ZeroCapacity};

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
