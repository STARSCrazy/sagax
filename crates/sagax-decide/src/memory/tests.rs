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

#[test]
fn nearest_negative_dx_matches_reference() {
    let mut out = Vec::new();
    let c = [(1, -40, 3), (2, -31, -31), (3, 20, 0), (4, -100, -17)];
    nearest(&c, (10, 5), 3, 16, &mut out);
    assert_eq!(out, vec![3, 2, 1]);
}

#[test]
fn sin256_matches_reference() {
    let want = [
        -226, -198, -162, -142, -98, -50, 0, 25, 74, 121, 162, 181, 213, 237, 251, 255, 255, 245,
        226, 213, 181, 142, 98, 74, 25, -25, -74, -98, -142, -181, -213, -226, -245, -255, -255,
        -251, -237, -213, -181, -162, -121, -74, -25, 0, 50, 98, 142, 162, 198, 226, 245, 251, 256,
        251, 237, 226, 198, 162, 121, 98, 50, 0, -50, -74, -121, -162, -198, -213, -237, -251,
        -256, -255, -245, -226, -198, -181, -142, -98, -50, -25, 25, 74, 121, 142, 181, 213,
    ];
    let got: Vec<i32> = (-300..=300).step_by(7).map(sin256).collect();
    assert_eq!(got, want);
}

#[test]
fn search_point_matches_reference() {
    /// Centre, agent, expected (x, y) for steps 0..=3.
    type Case = ((i32, i32), u32, [(i32, i32); 4]);
    #[rustfmt::skip]
    let want: [Case; 18] = [
        ((0, 0), 0, [(0, 256), (0, 1280), (1629, 1629), (0, 2172)]),
        ((0, 0), 1, [(162, 0), (810, 0), (0, 0), (3060, 0)]),
        ((0, 0), 2, [(0, 25), (0, 125), (0, 1782), (0, 0)]),
        ((0, 0), 5, [(0, 226), (0, 1130), (666, 2205), (0, 888)]),
        ((0, 0), 17, [(0, 0), (0, 0), (0, 0), (1944, 0)]),
        ((0, 0), 255, [(0, 0), (0, 0), (0, 0), (0, 0)]),
        ((5000, 5000), 0, [(5000, 5256), (5000, 6280), (6629, 6629), (2828, 7172)]),
        ((5000, 5000), 1, [(5162, 4802), (5810, 4010), (4775, 2705), (8060, 4700)]),
        ((5000, 5000), 2, [(4745, 5025), (3725, 5125), (3542, 6782), (2624, 3056)]),
        ((5000, 5000), 5, [(4879, 5226), (4395, 6130), (5666, 7205), (2060, 5888)]),
        ((5000, 5000), 17, [(4975, 4745), (4875, 3725), (3218, 3542), (6944, 2624)]),
        ((5000, 5000), 255, [(4819, 4819), (4095, 4095), (2696, 5000), (5000, 1928)]),
        ((32700, 100), 0, [(32700, 356), (32700, 1380), (32767, 1729), (30528, 2272)]),
        ((32700, 100), 1, [(32767, 0), (32767, 0), (32475, 0), (32767, 0)]),
        ((32700, 100), 2, [(32445, 125), (31425, 225), (31242, 1882), (30324, 0)]),
        ((32700, 100), 5, [(32579, 326), (32095, 1230), (32767, 2305), (29760, 988)]),
        ((32700, 100), 17, [(32675, 0), (32575, 0), (30918, 0), (32767, 0)]),
        ((32700, 100), 255, [(32519, 0), (31795, 0), (30396, 100), (32700, 0)]),
    ];
    let p = SearchPattern::default();
    for (c, agent, pts) in want {
        for (step, (x, y)) in pts.into_iter().enumerate() {
            let got = search_point(agent, step, [c.0, c.1, 7], &p);
            assert_eq!(
                got,
                Some([x, y, 7]),
                "agent {agent} step {step} centre {c:?}"
            );
        }
    }
}

#[test]
fn look_heading_matches_reference() {
    let want = [
        10, 10, 42, 42, 74, 74, 42, 42, 10, 10, 234, 234, 202, 202, 234, 234, 10, 10, 10,
    ];
    let got: Vec<i32> = (0..=180)
        .step_by(10)
        .map(|e| look_heading(10, e, 20, &LOOK_OFFSETS))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn search_point_clamps() {
    let p = SearchPattern {
        min: (100, 200),
        max: (1000, 300),
        ..SearchPattern::default()
    };
    // agent 0, step 1: straight along +y by 5 cells.
    assert_eq!(search_point(0, 1, [500, 250, 0], &p), Some([500, 300, 0]));
    // agent 255, step 3: towards -y by 12 cells.
    assert_eq!(search_point(255, 3, [50, 250, 0], &p), Some([100, 200, 0]));
}

#[test]
fn search_point_past_last_step_is_none() {
    let p = SearchPattern::default();
    assert!(search_point(3, 3, [0; 3], &p).is_some());
    assert_eq!(search_point(3, 4, [0; 3], &p), None);
    assert_eq!(search_point(3, usize::MAX, [0; 3], &p), None);
}

// ---- Officer recipe reference (see examples/officer.rs for the trace) ----

#[test]
fn officer_recipe_reference_numbers() {
    use officer::Event::*;
    use officer::*;
    let ev = run_scene(2100);
    let state = |t, officer, from, to| State {
        t,
        officer,
        from,
        to,
    };
    let want = vec![
        // t=1: officer 0 witnesses the crime: reports at once (reported was
        // 0) and starts the chase.
        RadioSent { t: 1, officer: 0 },
        state(1, 0, IDLE, CHASE),
        // Sight ticks of officer 0: (t+0)%4==0 -> 4, 8, ..., 40 seen. From
        // 44 on each sight tick adds 4: 44,48,...,76 are 9 misses = 36 >= 34
        // -> lost at 76 (8 misses = 32 is not enough). No stall: the
        // officer gains 8 per tick (> 0x80 in 85 ticks) or is closer than
        // 0x100.
        state(76, 0, CHASE, WALK),
        // Search centre = last seen place [4320, 4000] (actor at t=40).
        // Officer at t=76: x = 3000 + 16*75 = 4200, y = 4000. Step 0 of
        // agent 0 (heading 0) = [4320, 4256]; after one step (77) it is
        // 256 <= 0x180 away -> Look 51 (after a chase) -> 77+51 = 128.
        state(77, 0, WALK, LOOK),
        // t=103 = 1 + 102: call delivered. Officer 0 is busy; free are
        // 1, 2, 3. Distance (dx/16)^2+(dy/16)^2 to the call [4008, 4000]:
        // 1: 125^2, 2: 62^2, 3: 125^2 -> 2, then the tie 1 before 3.
        Helpers {
            t: 103,
            ids: [2, 1, 3],
        },
        state(103, 2, IDLE, WALK),
        state(103, 1, IDLE, WALK),
        state(103, 3, IDLE, WALK),
        state(128, 0, LOOK, WALK),
        // Step 1: [4320, 5280]; from y 4016 to 4896 (within 0x180) is
        // 880/16 = 55 ticks -> 128+55 = 183; look 51 -> 234.
        state(183, 0, WALK, LOOK),
        state(234, 0, LOOK, WALK),
        // Step 2: heading 0x20, sin = cos = 181, 9*256*181/256 = 1629 ->
        // [5949, 5629]; x from 4320 to 5565 needs ceil(1245/16) = 78 ->
        // 312; look 51 -> 363.
        state(312, 0, WALK, LOOK),
        state(363, 0, LOOK, WALK),
        // Step 3: heading -0x20 -> [4320-2172, 4000+2172] = [2148, 6172];
        // x from 5568 to 2532 needs ceil(3036/16) = 190 -> 553 (walk 190 <=
        // 340, search 425 <= 1020); look 51 -> 604, last step -> Idle.
        state(553, 0, WALK, LOOK),
        state(604, 0, LOOK, WALK),
        state(604, 0, WALK, IDLE),
        // Last refresh of the table at t=40 (helpers learning by radio do
        // not refresh it): forgotten at 40 + 2040 = 2080.
        Forgotten { t: 2080, id: ACTOR },
    ];
    let mine: Vec<_> = ev
        .iter()
        .copied()
        .filter(|e| !matches!(e, State { officer: 1..=3, t, .. } if *t != 103))
        .collect();
    assert_eq!(mine, want);
    // Only one call: the report timer (255) is due again at 256, but the
    // actor is out of sight by then.
    assert_eq!(
        ev.iter().filter(|e| matches!(e, RadioSent { .. })).count(),
        1
    );
}

/// Copy of the scene in examples/officer.rs (examples cannot be shared
/// with unit tests); keep both in step.
#[allow(dead_code)]
mod officer {
    use crate::memory::{
        Ignore, Interval, Pos, Pursuit, Radio, SearchPattern, Sightings, nearest, search_point,
    };
    use crate::state::{StateMachine, Transition};
    pub const IDLE: u32 = 0;
    pub const WALK: u32 = 1;
    pub const LOOK: u32 = 2;
    pub const CHASE: u32 = 3;
    pub const NAMES: [&str; 4] = ["Idle", "Walk", "Look", "Chase"];

    pub const F_LOST: u64 = 1;
    pub const F_STALLED: u64 = 2;
    pub const F_ARRIVED: u64 = 4;
    pub const F_GIVE_UP: u64 = 8;

    pub const ACTOR: u32 = 7;
    pub const CRIME_TICK: u32 = 1;
    pub const VISIBLE_UNTIL: u32 = 40;
    pub const SPEED: i32 = 16;
    pub const SIGHT_EVERY: u32 = 4;
    pub const LOST_AT: u32 = 34;
    pub const STALL_TICKS: u32 = 85;
    pub const STALL_GAIN: i32 = 0x80;
    pub const IGNORE_TICKS: u32 = 170;
    pub const RADIO_DELAY: u32 = 102;
    pub const RADIO_COOLDOWN: u32 = 170;
    pub const REPORT_EVERY: u32 = 255;
    pub const FORGET_TICKS: u32 = 2040;
    pub const TABLE_SIZE: usize = 8;
    pub const HELPERS: usize = 3;
    pub const LOOK_SENT: u32 = 170;
    pub const LOOK_AFTER_CHASE: u32 = 51;
    pub const LOOK_LATER: u32 = 51;
    pub const WALK_FIRST: u32 = 1020;
    pub const WALK_LATER: u32 = 340;
    pub const SEARCH_TOTAL: u32 = 1020;
    pub const LAST_STEP: usize = 3;
    pub const ARRIVE: i32 = 0x180;
    pub const CLOSE: i32 = 0x100;

    /// What happened, for the trace.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Event {
        State {
            t: u32,
            officer: u32,
            from: u32,
            to: u32,
        },
        RadioSent {
            t: u32,
            officer: u32,
        },
        Helpers {
            t: u32,
            ids: [u32; HELPERS],
        },
        Forgotten {
            t: u32,
            id: u32,
        },
    }

    struct Search {
        centre: Pos,
        step: usize,
        target: Pos,
        walked: u32,
        searched: u32,
    }

    struct Officer {
        pos: Pos,
        sm: StateMachine,
        known: Sightings,
        report: Interval,
        pursuit: Pursuit,
        ignore: Ignore,
        radio: Radio,
        search: Option<Search>,
    }

    fn cheb(a: Pos, b: Pos) -> i32 {
        (a[0] - b[0]).abs().max((a[1] - b[1]).abs())
    }

    fn walk(from: Pos, to: Pos) -> Pos {
        let s = |a: i32, b: i32| a + (b - a).clamp(-SPEED, SPEED);
        [s(from[0], to[0]), s(from[1], to[1]), to[2]]
    }

    /// The actor runs along +x, then hides at `VISIBLE_UNTIL`.
    fn actor_at(t: u32) -> Pos {
        [4000 + 8 * t.min(VISIBLE_UNTIL) as i32, 4000, 0]
    }

    fn officer(pos: Pos) -> Officer {
        let mut sm = StateMachine::new(4, IDLE).unwrap();
        sm.add_transition(Transition::new(CHASE, WALK, F_LOST))
            .unwrap();
        sm.add_transition(Transition::new(CHASE, IDLE, F_STALLED))
            .unwrap();
        sm.add_transition(Transition::new(WALK, IDLE, F_GIVE_UP))
            .unwrap();
        sm.add_transition(Transition::new(WALK, LOOK, F_ARRIVED))
            .unwrap();
        Officer {
            pos,
            sm,
            known: Sightings::new(1).unwrap(),
            report: Interval::default(),
            pursuit: Pursuit::default(),
            ignore: Ignore::new(1).unwrap(),
            radio: Radio::default(),
            search: None,
        }
    }

    /// Starts searching around `centre`: walk to step 0, look `look` there.
    fn begin_search(o: &mut Officer, me: u32, centre: Pos, look: u32) {
        o.sm.set_timeout(LOOK, look, WALK).unwrap();
        o.search = Some(Search {
            centre,
            step: 0,
            target: search_point(me, 0, centre, &SearchPattern::default()).unwrap(),
            walked: 0,
            searched: 0,
        });
    }

    /// Starts a call if the report is due and the radio accepts it.
    fn report(o: &mut Officer, me: u32, at: Pos, t: u32, ev: &mut Vec<Event>) {
        if o.report.due(t, REPORT_EVERY) && o.radio.start(at, t, RADIO_COOLDOWN) {
            o.report.mark(t);
            ev.push(Event::RadioSent { t, officer: me });
        }
    }

    pub fn run_scene(end: u32) -> Vec<Event> {
        let mut ev = Vec::new();
        let mut table = Sightings::new(TABLE_SIZE).unwrap();
        let mut cops = [
            officer([3000, 4000, 0]),
            officer([6008, 4000, 0]),
            officer([4008, 3000, 0]),
            officer([4008, 6000, 0]),
        ];
        let mut picked = Vec::new();
        for t in 1..=end {
            let actor = actor_at(t);
            table.expire(t, FORGET_TICKS, |id| {
                ev.push(Event::Forgotten { t, id });
                for o in cops.iter_mut() {
                    o.known.remove(id);
                }
            });
            for i in 0..cops.len() {
                let me = i as u32;
                let sees = me == 0 && t <= VISIBLE_UNTIL;
                let o = &mut cops[i];
                let mut flags = 0;
                if t == CRIME_TICK && sees && !o.ignore.ignores(ACTOR, t, IGNORE_TICKS) {
                    // Witness: remember, report at once, chase.
                    table.seen(ACTOR, actor, t);
                    o.known.seen(ACTOR, actor, t);
                    report(o, me, actor, t, &mut ev);
                    o.pursuit.begin(ACTOR, actor, cheb(o.pos, actor), t);
                    let c = o.sm.set_state(CHASE, t).unwrap();
                    ev.push(Event::State {
                        t,
                        officer: me,
                        from: c.from,
                        to: c.to,
                    });
                } else if o.sm.state() == CHASE {
                    if (t + me).is_multiple_of(SIGHT_EVERY) {
                        if sees {
                            o.pursuit.seen(actor);
                            o.known.seen(ACTOR, actor, t);
                            table.seen(ACTOR, actor, t);
                            report(o, me, actor, t, &mut ev);
                        } else {
                            o.pursuit.missed(SIGHT_EVERY);
                        }
                    }
                    let d = cheb(o.pos, o.pursuit.last_pos);
                    if o.pursuit.lost(LOST_AT) {
                        flags |= F_LOST;
                    } else if d >= CLOSE {
                        if o.pursuit.stalled(d, t, STALL_GAIN, STALL_TICKS) {
                            o.ignore.add(ACTOR, t);
                            flags |= F_STALLED;
                        }
                    } else {
                        o.pursuit.reset_progress(d, t);
                    }
                    o.pos = walk(o.pos, o.pursuit.last_pos);
                } else if let Some(s) = o.search.as_mut() {
                    if s.step > 0 {
                        s.searched += 1;
                    }
                    if o.sm.state() == WALK {
                        s.walked += 1;
                        o.pos = walk(o.pos, s.target);
                        let limit = if s.step == 0 { WALK_FIRST } else { WALK_LATER };
                        if s.walked > limit || s.searched > SEARCH_TOTAL {
                            flags |= F_GIVE_UP;
                        } else if cheb(o.pos, s.target) <= ARRIVE {
                            flags |= F_ARRIVED;
                        }
                    }
                }
                if let Some(c) = o.sm.update(t, flags) {
                    ev.push(Event::State {
                        t,
                        officer: me,
                        from: c.from,
                        to: c.to,
                    });
                    match (c.from, c.to) {
                        (CHASE, WALK) => {
                            let centre = o.pursuit.last_pos;
                            begin_search(o, me, centre, LOOK_AFTER_CHASE);
                        }
                        (LOOK, WALK) => {
                            let s = o.search.as_mut().unwrap();
                            if s.step >= LAST_STEP || s.searched >= SEARCH_TOTAL {
                                o.search = None;
                                o.sm.set_state(IDLE, t).unwrap();
                                ev.push(Event::State {
                                    t,
                                    officer: me,
                                    from: WALK,
                                    to: IDLE,
                                });
                            } else {
                                s.step += 1;
                                s.walked = 0;
                                s.target =
                                    search_point(me, s.step, s.centre, &SearchPattern::default())
                                        .unwrap();
                                o.sm.set_timeout(LOOK, LOOK_LATER, WALK).unwrap();
                            }
                        }
                        (_, IDLE) => o.search = None,
                        _ => {}
                    }
                }
                // The call goes out in the sender's own tick, to the nearest
                // free officers; they search where the sender now thinks the
                // actor is.
                if cops[i].radio.due(t, RADIO_DELAY) {
                    let call = cops[i].radio.take().unwrap();
                    let centre = cops[i].known.get(ACTOR).map_or(call, |s| s.pos);
                    let free: Vec<(u32, i32, i32)> = cops
                        .iter()
                        .enumerate()
                        .filter(|(j, c)| *j != i && c.sm.state() == IDLE)
                        .map(|(j, c)| (j as u32, c.pos[0], c.pos[1]))
                        .collect();
                    nearest(&free, (call[0], call[1]), HELPERS, 16, &mut picked);
                    let mut ids = [u32::MAX; HELPERS];
                    ids[..picked.len()].copy_from_slice(&picked);
                    ev.push(Event::Helpers { t, ids });
                    for &h in &picked {
                        let c = &mut cops[h as usize];
                        c.known.seen(ACTOR, centre, t);
                        c.report.mark(t);
                        begin_search(c, h, centre, LOOK_SENT);
                        let ch = c.sm.set_state(WALK, t).unwrap();
                        ev.push(Event::State {
                            t,
                            officer: h,
                            from: ch.from,
                            to: ch.to,
                        });
                    }
                }
            }
        }
        ev
    }
}
