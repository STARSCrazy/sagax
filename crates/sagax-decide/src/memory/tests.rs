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

#[test]
fn zone_renews_first_within_radius() {
    let mut z = Zones::new(4, 0x200, 255).unwrap();
    assert_eq!(z.add([0, 0, 0], 0, 1, None), 0);
    assert_eq!(z.add([0x1ff, 0, 0], 10, 2, Some([5, 6, 7])), 0);
    assert_eq!(z.len(), 1);
    let zone = z.iter().next().unwrap();
    assert_eq!(zone.pos, [0x1ff, 0, 0]);
    assert_eq!(zone.data, 2);
    assert_eq!(zone.since, 10);
    assert_eq!(zone.source, [5, 6, 7]);
    assert_eq!(zone.has_source, 1);
}

#[test]
fn zone_at_exact_radius_is_new() {
    let mut z = Zones::new(4, 0x200, 255).unwrap();
    z.add([0, 0, 0], 0, 0, None);
    assert_eq!(z.add([0x200, 0, 0], 1, 0, None), 1);
    assert_eq!(z.len(), 2);
}

#[test]
fn zone_full_drops_oldest() {
    let mut z = Zones::new(2, 10, 255).unwrap();
    z.add([0, 0, 0], 5, 1, None);
    z.add([100, 0, 0], 2, 2, None);
    assert_eq!(z.add([200, 0, 0], 6, 3, None), 1);
    let d: Vec<u32> = z.iter().map(|x| x.data).collect();
    assert_eq!(d, vec![1, 3]);
}

#[test]
fn zone_expire_at_lifetime() {
    let mut z = Zones::new(4, 10, 255).unwrap();
    z.add([0, 0, 0], 0, 1, None);
    z.add([100, 0, 0], 1, 2, None);
    assert_eq!(z.expire(255), 1);
    assert_eq!(z.len(), 1);
    assert_eq!(z.iter().next().unwrap().data, 2);
    assert_eq!(z.expire(255), 0);
}

#[test]
fn zone_blocks_rule() {
    let mut z = Zones::new(4, 100, 255).unwrap();
    z.add([0, 0, 0], 0, 0, None);
    let out = [1000, 0, 0];
    assert!(z.blocks([10, 0, 0], out, [2000, 0, 0]));
    assert!(!z.blocks([10, 0, 0], [20, 0, 0], out));
    assert!(!z.blocks([10, 0, 0], out, [20, 0, 0]));
    assert!(!z.blocks([500, 0, 0], out, [2000, 0, 0]));
    // Overlapping zones: blocked if any zone qualifies.
    z.add([150, 0, 0], 0, 0, None);
    assert_eq!(z.len(), 2);
    // cell in both; start in first only -> second still blocks.
    assert!(z.blocks([90, 0, 0], [-50, 0, 0], out));
    assert_eq!(z.contains([90, 0, 0]), Some(0));
    // radius <= 0: nothing inside.
    let mut n = Zones::new(1, 0, 10).unwrap();
    n.add([0, 0, 0], 0, 0, None);
    assert_eq!(n.contains([0, 0, 0]), None);
}

#[test]
fn stalled_resets_on_gain() {
    let mut p = Pursuit::default();
    p.begin(7, [0; 3], 1000, 0);
    assert!(!p.stalled(900, 50, 0x80, 85));
    assert!(!p.stalled(872, 60, 0x80, 85));
    assert_eq!(p.best, 872);
    assert_eq!(p.since, 60);
    assert!(!p.stalled(872, 144, 0x80, 85));
    assert!(p.stalled(872, 145, 0x80, 85));
}

#[test]
fn lost_at_limit() {
    let mut p = Pursuit::default();
    p.begin(1, [1, 2, 3], 0, 0);
    for _ in 0..8 {
        p.missed(4);
    }
    assert_eq!(p.unseen, 32);
    assert!(!p.lost(34));
    p.missed(4);
    assert!(p.lost(34));
    p.seen([4, 5, 6]);
    assert_eq!(p.unseen, 0);
    assert_eq!(p.last_pos, [4, 5, 6]);
    p.missed(u32::MAX);
    p.missed(5);
    assert_eq!(p.unseen, u32::MAX);
    p.reset_progress(50, 9);
    assert_eq!((p.best, p.since), (50, 9));
    p.clear();
    assert_eq!(p.target, 0);
}

#[test]
fn pursuit_time_wraps() {
    let mut p = Pursuit::default();
    p.begin(1, [0; 3], 1000, u32::MAX - 10);
    assert!(!p.stalled(1000, 73, 0x80, 85));
    assert!(p.stalled(1000, 74, 0x80, 85));
}

#[test]
fn radio_cooldown_from_start() {
    let mut r = Radio::default();
    assert!(r.start([1, 2, 3], 0, 170));
    assert_eq!(r.take(), Some([1, 2, 3]));
    assert!(!r.start([0; 3], 169, 170));
    assert!(r.start([0; 3], 170, 170));
}

#[test]
fn radio_refuses_while_pending() {
    let mut r = Radio::default();
    assert!(r.start([0; 3], 0, 0));
    assert!(r.pending());
    assert!(!r.start([1; 3], 1000, 0));
    r.cancel();
    assert!(!r.pending());
    assert!(r.start([1; 3], 1000, 0));
}

#[test]
fn radio_due_after_delay() {
    let mut r = Radio::default();
    assert!(!r.due(1000, 0));
    r.start([0; 3], 10, 170);
    assert!(!r.due(111, 102));
    assert!(r.due(112, 102));
}

#[test]
fn radio_take_without_pending_is_none() {
    let mut r = Radio::default();
    assert_eq!(r.take(), None);
}

#[test]
fn radio_first_start_ignores_cooldown() {
    let mut r = Radio::default();
    assert!(r.start([0; 3], 5, 170));
}

#[test]
fn nearest_truncates_negative() {
    let mut out = Vec::new();
    // divisor 16: dx -15 -> 0 (distance 0), dx -17 -> -1 (distance 1).
    nearest(&[(1, -17, 0), (2, -15, 0)], (0, 0), 2, 16, &mut out);
    assert_eq!(out, vec![2, 1]);
}

#[test]
fn nearest_tie_earlier_wins() {
    let mut out = vec![99];
    nearest(&[(1, 5, 0), (2, 0, 5), (3, 1, 0)], (0, 0), 2, 1, &mut out);
    assert_eq!(out, vec![3, 1]);
}

#[test]
fn nearest_fewer_than_k() {
    let mut out = Vec::new();
    nearest(&[(1, 5, 0)], (0, 0), 3, 0, &mut out);
    assert_eq!(out, vec![1]);
    nearest(&[], (0, 0), 3, 1, &mut out);
    assert!(out.is_empty());
    nearest(&[(1, 5, 0)], (0, 0), 0, 1, &mut out);
    assert!(out.is_empty());
}

#[test]
fn zone_full_evicts_slot0_then_appends() {
    let mut z = Zones::new(3, 10, 255).unwrap();
    z.add([0, 0, 0], 0, 1, None);
    z.add([100, 0, 0], 5, 2, None);
    z.add([200, 0, 0], 6, 3, None);
    assert_eq!(z.add([300, 0, 0], 7, 4, None), 2);
    let d: Vec<u32> = z.iter().map(|x| x.data).collect();
    assert_eq!(d, vec![2, 3, 4]);
}

#[test]
fn zone_full_age_tie_evicts_earlier_slot() {
    let mut z = Zones::new(3, 10, 255).unwrap();
    z.add([0, 0, 0], 5, 1, None);
    z.add([100, 0, 0], 5, 2, None);
    z.add([200, 0, 0], 6, 3, None);
    assert_eq!(z.add([300, 0, 0], 7, 4, None), 2);
    let d: Vec<u32> = z.iter().map(|x| x.data).collect();
    assert_eq!(d, vec![2, 3, 4]);
}

#[test]
fn zone_expire_wraps() {
    let mut z = Zones::new(2, 10, 255).unwrap();
    z.add([0, 0, 0], u32::MAX - 10, 1, None);
    assert_eq!(z.expire(243), 0);
    assert_eq!(z.expire(245), 1);
}

#[test]
fn radio_wraps() {
    let mut r = Radio::default();
    assert!(r.start([0; 3], u32::MAX - 10, 170));
    assert!(!r.due(90, 102));
    assert!(r.due(91, 102));
    r.take();
    assert!(!r.start([0; 3], 158, 170));
    assert!(r.start([0; 3], 159, 170));
}
