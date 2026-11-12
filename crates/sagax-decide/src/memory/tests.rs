use super::*;

fn ids(s: &Sightings) -> Vec<u32> {
    s.iter().map(|x| x.id).collect()
}

#[test]
fn sightings_evicts_oldest_and_keeps_order() {
    let mut s = Sightings::new(3).unwrap();
    assert_eq!(s.seen(1, [0; 3], 0), None);
    assert_eq!(s.seen(2, [0; 3], 1), None);
    assert_eq!(s.seen(3, [0; 3], 2), None);
    assert!(s.touch(1, 3));
    assert_eq!(s.seen(4, [1, 2, 3], 4), Some(2));
    assert_eq!(ids(&s), vec![1, 3, 4]);
    assert_eq!(s.get(4).unwrap().pos, [1, 2, 3]);
}

#[test]
fn sightings_tie_evicts_earlier_slot() {
    let mut s = Sightings::new(3).unwrap();
    for id in 1..=3 {
        s.seen(id, [0; 3], 5);
    }
    assert_eq!(s.seen(9, [0; 3], 5), Some(1));
    assert_eq!(ids(&s), vec![2, 3, 9]);
}

#[test]
fn sightings_present_while_full_does_not_evict() {
    let mut s = Sightings::new(2).unwrap();
    s.seen(1, [0; 3], 0);
    s.seen(2, [0; 3], 1);
    assert_eq!(s.seen(1, [7, 7, 7], 5), None);
    assert_eq!(s.len(), 2);
    assert_eq!(s.get(1).unwrap().pos, [7, 7, 7]);
    assert_eq!(s.get(1).unwrap().last_seen, 5);
}

#[test]
fn sightings_expire_in_slot_order() {
    let mut s = Sightings::new(4).unwrap();
    s.seen(1, [0; 3], 0);
    s.seen(2, [0; 3], 1);
    s.seen(3, [0; 3], 0);
    let mut gone = Vec::new();
    s.expire(2040, 2040, |id| gone.push(id));
    assert_eq!(gone, vec![1, 3]);
    assert_eq!(ids(&s), vec![2]);
}

#[test]
fn sightings_expire_boundary_ages() {
    let mut s = Sightings::new(2).unwrap();
    s.seen(1, [0; 3], 0);
    s.seen(2, [0; 3], 1);
    let mut gone = Vec::new();
    s.expire(2040, 2040, |id| gone.push(id));
    assert_eq!(gone, vec![1]);
    assert_eq!(ids(&s), vec![2]);
}

#[test]
fn sightings_age_wraps() {
    let mut s = Sightings::new(1).unwrap();
    s.seen(1, [0; 3], u32::MAX - 5);
    let mut gone = Vec::new();
    s.expire(10, 17, |id| gone.push(id));
    assert!(gone.is_empty());
    s.expire(10, 16, |id| gone.push(id));
    assert_eq!(gone, vec![1]);
}

#[test]
fn sightings_setters_and_removal() {
    let mut s = Sightings::new(2).unwrap();
    assert!(!s.touch(1, 0) && !s.set_pos(1, [0; 3]) && !s.set_data(1, 0) && !s.remove(1));
    s.seen(1, [0; 3], 0);
    assert!(s.set_pos(1, [4, 5, 6]) && s.set_data(1, 9));
    assert_eq!(s.get(1).unwrap().data, 9);
    assert!(s.remove(1));
    assert!(s.is_empty());
    assert_eq!(s.capacity(), 2);
}

#[test]
fn zero_capacity_is_error() {
    assert_eq!(Sightings::new(0).unwrap_err(), ZeroCapacity);
    assert_eq!(Ignore::new(0).unwrap_err(), ZeroCapacity);
}

#[test]
fn ignore_window() {
    let mut i = Ignore::new(2).unwrap();
    i.add(7, 100);
    assert!(i.ignores(7, 269, 170));
    assert!(!i.ignores(7, 270, 170));
    assert!(!i.ignores(8, 100, 170));
}

#[test]
fn ignore_full_replaces_oldest_and_renews() {
    let mut i = Ignore::new(2).unwrap();
    i.add(1, 0);
    i.add(2, 1);
    i.add(1, 5);
    i.add(3, 6);
    assert!(i.ignores(1, 6, 100));
    assert!(!i.ignores(2, 6, 100));
    assert!(i.ignores(3, 6, 100));
}

#[test]
fn interval_first_due_then_every() {
    let mut t = Interval::default();
    assert!(t.due(0, 255) && t.due(12345, 255));
    t.mark(0);
    assert!(!t.due(254, 255));
    assert!(t.due(255, 255));
    t.reset();
    assert!(t.due(1, 255));
}
