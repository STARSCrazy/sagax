use super::*;

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

fn area(kind: u32, x: i32, radius: i32) -> Area {
    Area {
        kind,
        pos: [x, 0, 0],
        radius,
        lifetime: 255,
        ..Area::default()
    }
}

#[test]
fn area_renews_first_same_kind_within_radius() {
    let mut z = Areas::new(4).unwrap();
    assert_eq!(z.add(area(1, 0, 0x200), 0), 0);
    let mut a = area(1, 0x1ff, 0x200);
    a.data = 2;
    a.source = [5, 6, 7];
    a.has_source = 1;
    assert_eq!(z.add(a, 10), 0);
    assert_eq!(z.len(), 1);
    let got = z.iter().next().unwrap();
    assert_eq!(got.pos, [0x1ff, 0, 0]);
    assert_eq!(got.data, 2);
    assert_eq!(got.since, 10);
    assert_eq!(got.source, [5, 6, 7]);
    assert_eq!(got.has_source, 1);
}

#[test]
fn area_at_exact_radius_is_new() {
    let mut z = Areas::new(4).unwrap();
    z.add(area(1, 0, 0x200), 0);
    assert_eq!(z.add(area(1, 0x200, 0x200), 1), 1);
    assert_eq!(z.len(), 2);
}

#[test]
fn areas_other_kind_not_merged() {
    let mut z = Areas::new(4).unwrap();
    z.add(area(1, 0, 100), 0);
    assert_eq!(z.add(area(2, 0, 100), 1), 1);
    assert_eq!(z.len(), 2);
    assert_eq!(z.contains_kind([0, 0, 0], 2), Some(1));
    assert_eq!(z.contains_kind([0, 0, 0], 3), None);
}

#[test]
fn area_renewal_uses_existing_radius() {
    let mut z = Areas::new(4).unwrap();
    z.add(area(1, 0, 10), 0);
    // New radius is big, but the existing one (10) decides.
    assert_eq!(z.add(area(1, 50, 1000), 1), 1);
    // Within the existing radius of 10: renewed, and radius replaced.
    assert_eq!(z.add(area(1, 5, 20), 2), 0);
    assert_eq!(z.iter().next().unwrap().radius, 20);
}

#[test]
fn area_full_drops_oldest() {
    let mut z = Areas::new(2).unwrap();
    let mut a = area(1, 0, 10);
    a.data = 1;
    z.add(a, 5);
    a.pos[0] = 100;
    a.data = 2;
    z.add(a, 2);
    a.pos[0] = 200;
    a.data = 3;
    assert_eq!(z.add(a, 6), 1);
    let d: Vec<u32> = z.iter().map(|x| x.data).collect();
    assert_eq!(d, vec![1, 3]);
}

#[test]
fn area_expire_per_own_lifetime() {
    let mut z = Areas::new(4).unwrap();
    let mut a = area(1, 0, 10);
    a.lifetime = 100;
    z.add(a, 0);
    a.pos[0] = 100;
    a.lifetime = 300;
    a.data = 2;
    z.add(a, 1);
    assert_eq!(z.expire(99), 0);
    assert_eq!(z.expire(100), 1);
    assert_eq!(z.len(), 1);
    assert_eq!(z.iter().next().unwrap().data, 2);
    assert_eq!(z.expire(300), 0);
    assert_eq!(z.expire(301), 1);
    assert!(z.is_empty());
}

#[test]
fn areas_blocks_any_blocking() {
    let mut z = Areas::new(4).unwrap();
    let mut a = area(1, 0, 100);
    a.blocking = 1;
    z.add(a, 0);
    let out = [1000, 0, 0];
    assert!(z.blocks([10, 0, 0], out, [2000, 0, 0]));
    assert!(!z.blocks([10, 0, 0], [20, 0, 0], out));
    assert!(!z.blocks([10, 0, 0], out, [20, 0, 0]));
    assert!(!z.blocks([500, 0, 0], out, [2000, 0, 0]));
    // Overlapping blocking area of another kind: blocked if any qualifies.
    let mut b = area(2, 150, 100);
    b.blocking = 1;
    z.add(b, 0);
    assert_eq!(z.len(), 2);
    assert!(z.blocks([90, 0, 0], [-50, 0, 0], out));
    assert_eq!(z.contains([90, 0, 0]), Some(0));
    // Non-blocking areas never block.
    let mut n = Areas::new(1).unwrap();
    n.add(area(1, 0, 100), 0);
    assert!(!n.blocks([10, 0, 0], out, [2000, 0, 0]));
    // radius <= 0: nothing inside.
    let mut e = Areas::new(1).unwrap();
    e.add(area(1, 0, 0), 0);
    assert_eq!(e.contains([0, 0, 0]), None);
}

#[test]
fn areas_cost_sums_nonblocking_saturating() {
    let mut z = Areas::new(4).unwrap();
    let mut a = area(1, 0, 100);
    a.strength = 7;
    z.add(a, 0);
    a.kind = 2;
    a.strength = 5;
    z.add(a, 0);
    a.kind = 3;
    a.strength = 1000;
    a.blocking = 1;
    z.add(a, 0);
    let out = [1000, 0, 0];
    assert_eq!(z.cost([10, 0, 0], out, [2000, 0, 0]), 12);
    assert_eq!(z.cost([500, 0, 0], out, [2000, 0, 0]), 0);
    // Start inside all areas: none counts.
    assert_eq!(z.cost([10, 0, 0], [20, 0, 0], out), 0);
    let mut s = Areas::new(2).unwrap();
    let mut b = area(1, 0, 100);
    b.strength = u32::MAX;
    s.add(b, 0);
    b.kind = 2;
    b.strength = 9;
    s.add(b, 0);
    assert_eq!(s.cost([10, 0, 0], out, [2000, 0, 0]), u32::MAX);
}

#[test]
fn area_full_evicts_slot0_then_appends() {
    let mut z = Areas::new(3).unwrap();
    let mut a = area(1, 0, 10);
    for (i, t) in [0u32, 5, 6].into_iter().enumerate() {
        a.pos[0] = 100 * i as i32;
        a.data = i as u32 + 1;
        z.add(a, t);
    }
    a.pos[0] = 300;
    a.data = 4;
    assert_eq!(z.add(a, 7), 2);
    let d: Vec<u32> = z.iter().map(|x| x.data).collect();
    assert_eq!(d, vec![2, 3, 4]);
}

#[test]
fn area_full_age_tie_evicts_earlier_slot() {
    let mut z = Areas::new(3).unwrap();
    let mut a = area(1, 0, 10);
    for (i, t) in [5u32, 5, 6].into_iter().enumerate() {
        a.pos[0] = 100 * i as i32;
        a.data = i as u32 + 1;
        z.add(a, t);
    }
    a.pos[0] = 300;
    a.data = 4;
    assert_eq!(z.add(a, 7), 2);
    let d: Vec<u32> = z.iter().map(|x| x.data).collect();
    assert_eq!(d, vec![2, 3, 4]);
}

#[test]
fn area_expire_wraps() {
    let mut z = Areas::new(2).unwrap();
    let mut a = area(1, 0, 10);
    a.lifetime = 255;
    z.add(a, u32::MAX - 10);
    assert_eq!(z.expire(243), 0);
    assert_eq!(z.expire(245), 1);
}

#[test]
fn areas_extreme_coordinates_do_not_overflow() {
    let mut z = Areas::new(2).unwrap();
    let mut a = area(1, 0, i32::MAX);
    a.pos = [i32::MIN, i32::MIN, 0];
    a.blocking = 1;
    z.add(a, 0);
    assert_eq!(z.contains([i32::MIN, i32::MIN, 0]), Some(0));
    // dx and dy are 2^32 - 1: the squares do not fit in i64.
    assert_eq!(z.contains([i32::MAX, i32::MAX, 0]), None);
    let far = [i32::MAX, i32::MAX, 0];
    let near = [i32::MIN, i32::MIN, 0];
    assert!(z.blocks(near, far, far));
    assert!(!z.blocks(near, near, far));
    // Renewal check with extreme centres does not overflow either.
    let mut b = a;
    b.pos = far;
    assert_eq!(z.add(b, 1), 1);
}

#[test]
fn area_layout_is_stable() {
    assert_eq!(std::mem::size_of::<Area>(), 52);
    assert_eq!(std::mem::offset_of!(Area, pos), 4);
    assert_eq!(std::mem::offset_of!(Area, radius), 16);
    assert_eq!(std::mem::offset_of!(Area, source), 36);
    assert_eq!(std::mem::offset_of!(Area, blocking), 48);
    assert_eq!(std::mem::offset_of!(Area, has_source), 49);
}

fn alert_cfg(decay: u32, hold: u32, hysteresis: u32) -> AlertConfig {
    let mut thresholds = [0; 8];
    thresholds[..3].copy_from_slice(&[20, 50, 80]);
    AlertConfig {
        max: 100,
        decay_per_tick: decay,
        hold,
        hysteresis,
        count: 3,
        thresholds,
    }
}

#[test]
fn alertness_rises_levels() {
    let cfg = alert_cfg(2, 5, 5);
    let mut a = Alertness::default();
    a.stimulus(10, 0, &cfg);
    assert_eq!(a.update(0, &cfg), None);
    a.stimulus(10, 0, &cfg);
    assert_eq!(a.update(0, &cfg), Some((0, 1)));
    a.stimulus(30, 0, &cfg);
    assert_eq!(a.value(), 50);
    assert_eq!(a.update(0, &cfg), Some((1, 2)));
    a.stimulus(500, 0, &cfg);
    assert_eq!(a.value(), 100);
    assert_eq!(a.update(0, &cfg), Some((2, 3)));
    assert_eq!(a.level(), 3);
}

#[test]
fn alertness_jumps_levels() {
    let cfg = alert_cfg(2, 5, 5);
    let mut a = Alertness::default();
    a.stimulus(90, 0, &cfg);
    assert_eq!(a.update(0, &cfg), Some((0, 3)));
    // Falls straight down too: value 0 after long decay.
    assert_eq!(a.update(1000, &cfg), Some((3, 0)));
    assert_eq!(a.value(), 0);
}

#[test]
fn alertness_hold_then_decay() {
    let cfg = alert_cfg(2, 5, 5);
    let mut a = Alertness::default();
    a.stimulus(50, 10, &cfg);
    assert_eq!(a.update(10, &cfg), Some((0, 2)));
    // Hold lasts until tick 15: nothing decays yet.
    assert_eq!(a.update(15, &cfg), None);
    assert_eq!(a.value(), 50);
    // Ticks 16..=20 decay: 5 * 2 = 10; 40 < 50 - 5, so level 1.
    assert_eq!(a.update(20, &cfg), Some((2, 1)));
    assert_eq!(a.value(), 40);
    // A late update only decays the part after the hold.
    let mut b = Alertness::default();
    b.stimulus(100, 10, &cfg);
    b.update(10, &cfg);
    b.update(40, &cfg);
    assert_eq!(b.value(), 100 - 2 * 25);
}

#[test]
fn alertness_hysteresis() {
    let cfg = alert_cfg(1, 0, 5);
    let mut a = Alertness::default();
    a.stimulus(50, 0, &cfg);
    assert_eq!(a.update(0, &cfg), Some((0, 2)));
    assert_eq!(a.update(4, &cfg), None); // 46
    assert_eq!(a.update(5, &cfg), None); // 45, not below 45
    assert_eq!(a.update(6, &cfg), Some((2, 1))); // 44
    // Rising needs the plain threshold again: 49 stays level 1.
    a.stimulus(5, 6, &cfg);
    assert_eq!(a.value(), 49);
    assert_eq!(a.update(6, &cfg), None);
    assert_eq!(a.level(), 1);
    a.stimulus(1, 6, &cfg);
    assert_eq!(a.update(6, &cfg), Some((1, 2)));
}

#[test]
fn alertness_decay_to_zero_and_wrap() {
    let cfg = alert_cfg(10, 5, 5);
    let mut a = Alertness::default();
    let t0 = u32::MAX - 5;
    a.stimulus(100, t0, &cfg);
    assert_eq!(a.update(t0, &cfg), Some((0, 3)));
    // 16 ticks later across the wrap; 11 of them after the hold: 110 > 100.
    assert_eq!(a.update(10, &cfg), Some((3, 0)));
    assert_eq!(a.value(), 0);
    // A count above 8 is treated as 8.
    let mut big = alert_cfg(1, 0, 0);
    big.count = 100;
    big.thresholds = [1, 2, 3, 4, 5, 6, 7, 8];
    let mut b = Alertness::default();
    b.stimulus(100, 0, &big);
    assert_eq!(b.update(0, &big), Some((0, 8)));
}

#[test]
fn alert_layout_is_stable() {
    assert_eq!(std::mem::size_of::<AlertConfig>(), 52);
    assert_eq!(std::mem::size_of::<Alertness>(), 16);
}

/// The default pattern with the v1 sine, for the golden numbers.
fn coarse() -> PointPattern {
    PointPattern {
        sine: Sine::Coarse,
        ..PointPattern::default()
    }
}

#[test]
fn fine_sine_quarter_points() {
    assert_eq!(sine(0, Sine::Fine), 0);
    assert_eq!(sine(64, Sine::Fine), 65536);
    assert_eq!(sine(128, Sine::Fine), 0);
    assert_eq!(sine(192, Sine::Fine), -65536);
    assert_eq!(sine(256, Sine::Fine), 0);
    assert_eq!(sine(-64, Sine::Fine), -65536);
    assert_eq!(sine(32, Sine::Fine), 46341);
    assert_eq!(sine(1, Sine::Fine), 1608);
}

#[test]
fn fine_sine_symmetry() {
    for h in 0..256 {
        assert_eq!(sine(h, Sine::Fine), -sine(h + 128, Sine::Fine), "h {h}");
        assert_eq!(sine(h, Sine::Fine), sine(128 - h, Sine::Fine), "h {h}");
        // Cosine is even.
        assert_eq!(sine(h + 64, Sine::Fine), sine(64 - h, Sine::Fine), "h {h}");
    }
}

#[test]
fn points_around_fine_default() {
    let p = PointPattern::default();
    assert_eq!(p.sine, Sine::Fine);
    // Agent 0 looks straight along +y.
    assert_eq!(points_around(0, 0, [0, 0, 7], &p), Some([0, 256, 7]));
    // Agent 1: heading 0x62 = 98.
    let c = [5000, 5000, 7];
    assert_eq!(points_around(1, 0, c, &p), Some([5171, 4811, 7]));
    assert_eq!(points_around(1, 1, c, &p), Some([5859, 4052, 7]));
    assert_eq!(points_around(1, 3, c, &p), Some([8068, 4850, 7]));
    assert_eq!(points_around(1, 4, c, &p), None);
}

#[test]
fn points_around_missing_distance_counts_as_zero_fine() {
    let p = PointPattern {
        offsets: vec![0, 0],
        distances: vec![256],
        ..PointPattern::default()
    };
    assert_eq!(points_around(0, 2, [500, 600, 7], &p), Some([500, 600, 7]));
}

#[test]
fn coarse_sine_matches_reference() {
    let want = [
        -226, -198, -162, -142, -98, -50, 0, 25, 74, 121, 162, 181, 213, 237, 251, 255, 255, 245,
        226, 213, 181, 142, 98, 74, 25, -25, -74, -98, -142, -181, -213, -226, -245, -255, -255,
        -251, -237, -213, -181, -162, -121, -74, -25, 0, 50, 98, 142, 162, 198, 226, 245, 251, 256,
        251, 237, 226, 198, 162, 121, 98, 50, 0, -50, -74, -121, -162, -198, -213, -237, -251,
        -256, -255, -245, -226, -198, -181, -142, -98, -50, -25, 25, 74, 121, 142, 181, 213,
    ];
    let got: Vec<i32> = (-300..=300)
        .step_by(7)
        .map(|h| sine(h, Sine::Coarse))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn points_around_matches_reference() {
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
    let p = coarse();
    for (c, agent, pts) in want {
        for (step, (x, y)) in pts.into_iter().enumerate() {
            let got = points_around(agent, step, [c.0, c.1, 7], &p);
            assert_eq!(
                got,
                Some([x, y, 7]),
                "agent {agent} step {step} centre {c:?}"
            );
        }
    }
}

#[test]
fn sweep_heading_matches_reference() {
    let want = [
        10, 10, 42, 42, 74, 74, 42, 42, 10, 10, 234, 234, 202, 202, 234, 234, 10, 10, 10,
    ];
    let got: Vec<i32> = (0..=180)
        .step_by(10)
        .map(|e| sweep_heading(10, e, 20, &LOOK_OFFSETS))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn points_around_clamps() {
    let p = PointPattern {
        min: (100, 200),
        max: (1000, 300),
        ..coarse()
    };
    // agent 0, step 1: straight along +y by 5 cells.
    assert_eq!(points_around(0, 1, [500, 250, 0], &p), Some([500, 300, 0]));
    // agent 255, step 3: towards -y by 12 cells.
    assert_eq!(points_around(255, 3, [50, 250, 0], &p), Some([100, 200, 0]));
}

#[test]
fn points_around_past_last_step_is_none() {
    let p = coarse();
    assert!(points_around(3, 3, [0; 3], &p).is_some());
    assert_eq!(points_around(3, 4, [0; 3], &p), None);
    assert_eq!(points_around(3, usize::MAX, [0; 3], &p), None);
}

#[test]
fn points_around_missing_distance_counts_as_zero() {
    let p = PointPattern {
        offsets: vec![0, 0],
        distances: vec![256],
        ..coarse()
    };
    assert_eq!(points_around(0, 2, [500, 600, 7], &p), Some([500, 600, 7]));
}

// ---- Knowledge ----

fn fact(subject: u32, kind: u32, seen_at: u32) -> Fact {
    Fact {
        subject,
        kind,
        seen_at,
        ..Fact::default()
    }
}

fn keys(k: &Knowledge) -> Vec<(u32, u32)> {
    k.iter().map(|f| (f.subject, f.kind)).collect()
}

#[test]
fn knowledge_key_is_subject_and_kind() {
    let mut k = Knowledge::new(4, Evict::Oldest).unwrap();
    assert_eq!(k.learn(fact(1, 1, 0), 0), None);
    assert_eq!(k.learn(fact(1, 2, 0), 0), None);
    assert_eq!(k.learn(fact(2, 1, 0), 0), None);
    assert_eq!(k.len(), 3);
    // Same key keeps its slot.
    let mut f = fact(1, 2, 5);
    f.pos = [1, 2, 3];
    assert_eq!(k.learn(f, 5), None);
    assert_eq!(keys(&k), vec![(1, 1), (1, 2), (2, 1)]);
    assert_eq!(k.get(1, 2).unwrap().pos, [1, 2, 3]);
    assert_eq!(k.get(2, 2), None);
    assert_eq!(k.capacity(), 4);
    assert_eq!(Knowledge::new(0, Evict::Oldest).unwrap_err(), ZeroCapacity);
}

#[test]
fn knowledge_evict_oldest_tie_earlier() {
    let mut k = Knowledge::new(3, Evict::Oldest).unwrap();
    k.learn(fact(1, 0, 0), 0);
    k.learn(fact(2, 0, 1), 1);
    k.learn(fact(3, 0, 2), 2);
    assert!(k.touch(1, 0, 3));
    // At now=4 subject 2 is the oldest.
    let out = k.learn(fact(4, 0, 4), 4).unwrap();
    assert_eq!(out.subject, 2);
    assert_eq!(keys(&k), vec![(1, 0), (3, 0), (4, 0)]);
    // Tie: all seen at 5, the earlier slot goes.
    let mut t = Knowledge::new(3, Evict::Oldest).unwrap();
    for s in 1..=3 {
        t.learn(fact(s, 0, 5), 5);
    }
    assert_eq!(t.learn(fact(9, 0, 5), 5).unwrap().subject, 1);
    assert_eq!(keys(&t), vec![(2, 0), (3, 0), (9, 0)]);
    // A present key while full does not evict.
    assert_eq!(t.learn(fact(2, 0, 6), 6), None);
    assert_eq!(t.len(), 3);
}

#[test]
fn knowledge_evict_weakest_then_oldest() {
    let with = |s: u32, at: u32, st: u32| Fact {
        strength: st,
        ..fact(s, 0, at)
    };
    let mut k = Knowledge::new(4, Evict::Weakest).unwrap();
    k.learn(with(1, 10, 5), 10);
    k.learn(with(2, 0, 9), 10);
    k.learn(with(3, 4, 5), 10);
    k.learn(with(4, 4, 5), 10);
    // Smallest strength is 5 (slots 0, 2, 3); oldest of them: slots 2 and 3
    // (age 6), the earlier one (subject 3) goes.
    assert_eq!(k.learn(with(5, 10, 7), 10).unwrap().subject, 3);
    assert_eq!(keys(&k), vec![(1, 0), (2, 0), (4, 0), (5, 0)]);
    // Again strength 5: slot 0 (age 0) and subject 4 (age 6): subject 4 goes.
    assert_eq!(k.learn(with(6, 10, 9), 10).unwrap().subject, 4);
    // Equal strength and age: the earlier slot.
    let mut t = Knowledge::new(2, Evict::Weakest).unwrap();
    t.learn(with(1, 0, 3), 0);
    t.learn(with(2, 0, 3), 0);
    assert_eq!(t.learn(with(3, 0, 3), 0).unwrap().subject, 1);
    // Strength beats age.
    let mut w = Knowledge::new(2, Evict::Weakest).unwrap();
    w.learn(with(1, 0, 8), 100);
    w.learn(with(2, 99, 2), 100);
    assert_eq!(w.learn(with(3, 100, 5), 100).unwrap().subject, 2);
}

#[test]
fn knowledge_merge_keeps_newer() {
    let mut k = Knowledge::new(2, Evict::Oldest).unwrap();
    let mut a = fact(1, 0, 100);
    a.data = 1;
    k.merge(a, 110);
    // An older fact does not overwrite.
    let mut old = fact(1, 0, 90);
    old.data = 2;
    assert_eq!(k.merge(old, 110), None);
    assert_eq!(k.get(1, 0).unwrap().data, 1);
    // Equal time overwrites ("not older").
    let mut same = fact(1, 0, 100);
    same.data = 3;
    k.merge(same, 110);
    assert_eq!(k.get(1, 0).unwrap().data, 3);
    // A newer one overwrites.
    let mut newer = fact(1, 0, 105);
    newer.data = 4;
    k.merge(newer, 110);
    assert_eq!(
        (k.get(1, 0).unwrap().seen_at, k.get(1, 0).unwrap().data),
        (105, 4)
    );
    // Wrap: now = 5, stored seen_at = u32::MAX - 2 (age 8); seen_at = 2 (age 3) is newer.
    let mut w = Knowledge::new(1, Evict::Oldest).unwrap();
    w.merge(fact(1, 0, u32::MAX - 2), 5);
    let mut n = fact(1, 0, 2);
    n.data = 9;
    w.merge(n, 5);
    assert_eq!(w.get(1, 0).unwrap().data, 9);
    // seen_at = u32::MAX - 9 (age 15) is older and does not overwrite.
    let mut o = fact(1, 0, u32::MAX - 9);
    o.data = 7;
    w.merge(o, 5);
    assert_eq!(w.get(1, 0).unwrap().data, 9);
    // An absent key inserts and evicts like learn.
    let mut f = Knowledge::new(1, Evict::Oldest).unwrap();
    f.merge(fact(1, 0, 0), 1);
    assert_eq!(f.merge(fact(2, 0, 1), 1).unwrap().subject, 1);
}

#[test]
fn knowledge_learn_overwrites() {
    let mut k = Knowledge::new(2, Evict::Oldest).unwrap();
    k.learn(fact(1, 0, 100), 110);
    let mut older = fact(1, 0, 50);
    older.source = SOURCE_TOLD;
    older.strength = 4;
    older.data = 5;
    older.pos = [7, 8, 9];
    assert_eq!(k.learn(older, 110), None);
    assert_eq!(*k.get(1, 0).unwrap(), older);
    // Setters.
    assert!(k.touch(1, 0, 120) && k.set_pos(1, 0, [1, 1, 1]) && k.set_data(1, 0, 6));
    assert!(k.set_strength(1, 0, 8));
    let f = k.get(1, 0).unwrap();
    assert_eq!(
        (f.seen_at, f.pos, f.data, f.strength),
        (120, [1, 1, 1], 6, 8)
    );
    assert!(!k.touch(2, 0, 0) && !k.set_pos(2, 0, [0; 3]) && !k.set_data(2, 0, 0));
    assert!(!k.set_strength(2, 0, 0) && !k.remove(2, 0));
    assert!(k.remove(1, 0));
    assert!(k.is_empty());
    k.learn(fact(3, 0, 0), 0);
    k.clear();
    assert_eq!(k.len(), 0);
}

#[test]
fn knowledge_remove_subject_all_kinds() {
    let mut k = Knowledge::new(5, Evict::Oldest).unwrap();
    k.learn(fact(1, 1, 0), 0);
    k.learn(fact(2, 1, 0), 0);
    k.learn(fact(1, 2, 0), 0);
    k.learn(fact(1, 3, 0), 0);
    assert_eq!(k.remove_subject(1), 3);
    assert_eq!(keys(&k), vec![(2, 1)]);
    assert_eq!(k.remove_subject(1), 0);
}

#[test]
fn knowledge_expire_with_per_kind() {
    let mut k = Knowledge::new(5, Evict::Oldest).unwrap();
    k.learn(fact(1, 1, 0), 0);
    k.learn(fact(2, 2, 0), 0);
    k.learn(fact(3, 1, 10), 10);
    k.learn(fact(4, 2, 10), 10);
    let mut gone = Vec::new();
    // At now=20 the first two have age 20 (>= 20), the others 10.
    k.expire(20, 20, |f| gone.push(f.subject));
    assert_eq!(gone, vec![1, 2]);
    // Kind 1 lives 5, kind 2 lives 15: only subject 3 (age 10, kind 1) goes.
    gone.clear();
    k.expire_with(
        20,
        |f| if f.kind == 1 { 5 } else { 15 },
        |f| gone.push(f.subject),
    );
    assert_eq!(gone, vec![3]);
    assert_eq!(keys(&k), vec![(4, 2)]);
    // Boundary with a wrapped clock: age 15 at now = 10.
    let mut w = Knowledge::new(1, Evict::Oldest).unwrap();
    w.learn(fact(1, 0, u32::MAX - 4), 0);
    w.expire(10, 16, |_| {});
    assert_eq!(w.len(), 1);
    w.expire(10, 15, |_| {});
    assert!(w.is_empty());
}

#[test]
fn fact_layout_is_stable() {
    assert_eq!(std::mem::offset_of!(Fact, subject), 0);
    assert_eq!(std::mem::offset_of!(Fact, kind), 4);
    assert_eq!(std::mem::offset_of!(Fact, pos), 8);
    assert_eq!(std::mem::offset_of!(Fact, seen_at), 20);
    assert_eq!(std::mem::offset_of!(Fact, strength), 24);
    assert_eq!(std::mem::offset_of!(Fact, data), 28);
    assert_eq!(std::mem::offset_of!(Fact, source), 32);
    assert_eq!(std::mem::size_of::<Fact>(), 36);
    assert_eq!(std::mem::size_of::<Evict>(), 1);
}

// ---- Timers ----

#[test]
fn timers_renew_and_replace_least_left() {
    let mut t = Timers::new(2).unwrap();
    assert!(t.is_empty());
    t.set(1, 0, 170);
    assert!(t.active(1, 169));
    assert!(!t.active(1, 170));
    assert_eq!(t.left(1, 100), 70);
    assert_eq!(t.left(1, 170), 0);
    assert_eq!(t.left(9, 0), 0);
    assert!(!t.active(9, 0));
    // Renew with another duration.
    t.set(1, 100, 50);
    assert_eq!(t.left(1, 100), 50);
    assert_eq!(t.len(), 1);
    t.set(2, 100, 10);
    // Full at now=105: left(1) = 45, left(2) = 5, so id 2 is replaced.
    t.set(3, 105, 20);
    assert!(t.active(1, 105) && !t.active(2, 105) && t.active(3, 105));
    assert_eq!(t.len(), 2);
    // At 150 both have run out (left 0, a tie): the earlier slot (id 1) goes.
    t.set(4, 150, 5);
    assert!(!t.active(1, 150) && t.active(4, 150) && !t.active(3, 150));
    assert_eq!(t.left(4, 152), 3);
    // Duration 0 is never active.
    t.set(5, 151, 0);
    assert!(!t.active(5, 151));
    assert!(t.remove(5));
    assert!(!t.remove(5));
    t.clear();
    assert_eq!((t.len(), t.is_empty()), (0, true));
    assert_eq!(Timers::new(0).unwrap_err(), ZeroCapacity);
}

#[test]
fn timers_wrap() {
    let mut t = Timers::new(1).unwrap();
    t.set(1, u32::MAX - 10, 30);
    assert!(t.active(1, 18));
    assert!(!t.active(1, 19));
    assert_eq!(t.left(1, 9), 10);
}

// ---- Progress ----

#[test]
fn progress_update_and_stalled() {
    let mut p = Progress::default();
    // Before start nothing happens.
    assert!(!p.update(0, 5, 0) && !p.stalled(1_000_000, 0));
    p.start(1000, 0);
    assert!(!p.update(900, 50, 0x80));
    assert_eq!((p.best, p.since), (1000, 0));
    assert!(p.update(872, 60, 0x80));
    assert_eq!((p.best, p.since), (872, 60));
    assert!(!p.stalled(144, 85));
    assert!(p.stalled(145, 85));
    // stalled is pure.
    assert_eq!((p.best, p.since), (872, 60));
    p.reset();
    assert_eq!(p, Progress::default());
}

#[test]
fn progress_i32_extremes() {
    let mut p = Progress::default();
    p.start(i32::MAX, 0);
    // A gain of 2^32 - 1 does not overflow.
    assert!(p.update(i32::MIN, 5, i32::MAX));
    assert_eq!(p.best, i32::MIN);
    let mut q = Progress::default();
    q.start(i32::MIN, 0);
    // A gain of -(2^32 - 1) is below any min_gain: only the age counts.
    assert!(!q.update(i32::MAX, 10, i32::MIN));
    assert_eq!(q.best, i32::MIN);
    assert!(q.stalled(10, 10));
    // Wrapped clock.
    let mut w = Progress::default();
    w.start(1000, u32::MAX - 10);
    assert!(!w.stalled(73, 85));
    assert!(w.stalled(74, 85));
    assert!(w.update(0, 80, 1) && w.since == 80);
}

fn msg(subject: u32, kind: u32) -> Message {
    Message {
        kind,
        subject,
        pos: [1, 2, 3],
        seen_at: 7,
        strength: 8,
        data: 9,
        sender: 10,
        keep_if_sender_gone: 0,
    }
}

#[test]
fn outbox_cooldown_from_last_accepted_post() {
    let mut o = Outbox::new(4).unwrap();
    assert_eq!(o.post(msg(1, 1), 0, 102, 170), Ok(()));
    assert_eq!(o.post(msg(2, 1), 169, 102, 170), Err(Refused::Cooldown));
    // A refused post does not move the cooldown start.
    assert_eq!(o.post(msg(3, 1), 170, 102, 170), Ok(()));
    assert_eq!(o.post(msg(4, 1), 339, 102, 170), Err(Refused::Cooldown));
    assert_eq!(o.post(msg(5, 1), 340, 102, 170), Ok(()));
    assert_eq!(o.pending(), 3);
    // Wrapped clock.
    let mut w = Outbox::new(2).unwrap();
    assert_eq!(w.post(msg(1, 1), u32::MAX - 10, 0, 170), Ok(()));
    assert_eq!(w.post(msg(1, 1), 158, 0, 170), Err(Refused::Cooldown));
    assert_eq!(w.post(msg(1, 1), 159, 0, 170), Ok(()));
}

#[test]
fn outbox_full_refused() {
    let mut o = Outbox::new(2).unwrap();
    assert_eq!(o.post(msg(1, 1), 0, 5, 0), Ok(()));
    assert_eq!(o.post(msg(2, 1), 0, 5, 0), Ok(()));
    assert_eq!(o.post(msg(3, 1), 0, 5, 0), Err(Refused::Full));
    let mut out = Vec::new();
    o.due(5, &mut out);
    assert_eq!(o.post(msg(3, 1), 5, 5, 0), Ok(()));
    assert!(Outbox::new(0).is_err());
}

#[test]
fn outbox_due_each_own_delay_in_post_order() {
    let mut o = Outbox::new(4).unwrap();
    o.post(msg(1, 1), 0, 10, 0).unwrap();
    o.post(msg(2, 1), 1, 2, 0).unwrap();
    o.post(msg(3, 1), 2, 0, 0).unwrap();
    let mut out = vec![msg(99, 1)];
    o.due(3, &mut out);
    let ids: Vec<u32> = out.iter().map(|m| m.subject).collect();
    assert_eq!(ids, vec![99, 2, 3]);
    assert_eq!(o.pending(), 1);
    out.clear();
    o.due(10, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].subject, 1);
    assert_eq!(o.pending(), 0);
}

#[test]
fn outbox_sender_gone_keeps_marked() {
    let mut o = Outbox::new(4).unwrap();
    let mut keep = msg(2, 1);
    keep.keep_if_sender_gone = 1;
    o.post(msg(1, 1), 0, 5, 0).unwrap();
    o.post(keep, 0, 5, 0).unwrap();
    o.sender_gone();
    assert_eq!(o.pending(), 1);
    let mut out = Vec::new();
    o.due(5, &mut out);
    assert_eq!(out[0].subject, 2);
    o.post(msg(3, 1), 6, 5, 0).unwrap();
    o.clear();
    assert_eq!(o.pending(), 0);
    // The cooldown survives clear (last accepted post was at 6).
    assert_eq!(o.post(msg(4, 1), 7, 5, 100), Err(Refused::Cooldown));
    assert_eq!(o.post(msg(4, 1), 106, 5, 100), Ok(()));
}

#[test]
fn inbox_take_due_in_push_order() {
    let mut b = Inbox::new(4).unwrap();
    assert!(b.push(msg(1, 1), 20));
    assert!(b.push(msg(2, 1), 5));
    assert!(b.push(msg(3, 1), 10));
    let mut out = Vec::new();
    b.take_due(10, &mut out);
    let ids: Vec<u32> = out.iter().map(|m| m.subject).collect();
    assert_eq!(ids, vec![2, 3]);
    assert_eq!(b.len(), 1);
    b.take_due(19, &mut out);
    assert_eq!(out.len(), 2);
    b.take_due(20, &mut out);
    assert_eq!(out.len(), 3);
    assert!(b.is_empty());
    // Wrapped clock: arrival just after the wrap.
    assert!(b.push(msg(4, 1), 5));
    out.clear();
    b.take_due(u32::MAX - 1, &mut out);
    assert!(out.is_empty());
    b.take_due(5, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(arrival(u32::MAX - 1, 2, 3, 4), 12);
    assert_eq!(arrival(10, 0, 0, 0), 10);
    b.push(msg(5, 1), 1);
    b.clear();
    assert_eq!(b.len(), 0);
}

#[test]
fn inbox_full_replaces_latest_if_earlier() {
    let mut b = Inbox::new(3).unwrap();
    b.push(msg(1, 1), 10);
    b.push(msg(2, 1), 30);
    b.push(msg(3, 1), 30);
    // Equal to the latest: refused.
    assert!(!b.push(msg(4, 1), 30));
    // Earlier: the later of the two latest (slot 2) goes.
    assert!(b.push(msg(5, 1), 20));
    let mut out = Vec::new();
    b.take_due(100, &mut out);
    let ids: Vec<u32> = out.iter().map(|m| m.subject).collect();
    assert_eq!(ids, vec![1, 2, 5]);
}

#[test]
fn route_nearest_tie_earlier() {
    let mut out = vec![99];
    let e = Metric::Euclid;
    // divisor 16: dx -15 -> 0, dx -17 -> -1.
    route(
        Route::Nearest(2),
        (0, 0),
        &[(1, -17, 0), (2, -15, 0)],
        e,
        16,
        &mut out,
    );
    assert_eq!(out, vec![2, 1]);
    route(
        Route::Nearest(2),
        (0, 0),
        &[(1, 5, 0), (2, 0, 5), (3, 1, 0)],
        e,
        1,
        &mut out,
    );
    assert_eq!(out, vec![3, 1]);
    route(Route::Nearest(3), (0, 0), &[(1, 5, 0)], e, 0, &mut out);
    assert_eq!(out, vec![1]);
    route(Route::Nearest(3), (0, 0), &[], e, 1, &mut out);
    assert!(out.is_empty());
    route(Route::Nearest(0), (0, 0), &[(1, 5, 0)], e, 1, &mut out);
    assert!(out.is_empty());
    let c = [(1, -40, 3), (2, -31, -31), (3, 20, 0), (4, -100, -17)];
    route(Route::Nearest(3), (10, 5), &c, e, 16, &mut out);
    assert_eq!(out, vec![3, 2, 1]);
    let c = [(1, i32::MIN, i32::MIN), (2, i32::MAX, i32::MAX), (3, 0, 0)];
    route(Route::Nearest(3), (i32::MAX, i32::MAX), &c, e, 1, &mut out);
    assert_eq!(out, vec![2, 3, 1]);
    route(Route::Nearest(3), (i32::MIN, i32::MIN), &c, e, 1, &mut out);
    assert_eq!(out, vec![1, 3, 2]);
}

#[test]
fn route_in_range_inclusive() {
    let c = [(1, 3, 4), (2, 5, 0), (3, 4, 4), (4, -3, -3), (5, 0, 6)];
    let mut out = Vec::new();
    // Euclid: d² <= 25: (3,4)=25 in, (5,0)=25 in, (4,4)=32 out, (-3,-3)=18 in, (0,6)=36 out.
    route(Route::InRange(5), (0, 0), &c, Metric::Euclid, 1, &mut out);
    assert_eq!(out, vec![1, 2, 4]);
    // Manhattan <= 7: 7 in, 5 in, 8 out, 6 in, 6 in.
    route(
        Route::InRange(7),
        (0, 0),
        &c,
        Metric::Manhattan,
        1,
        &mut out,
    );
    assert_eq!(out, vec![1, 2, 4, 5]);
    // Chebyshev <= 4: 4 in, 5 out, 4 in, 3 in, 6 out.
    route(
        Route::InRange(4),
        (0, 0),
        &c,
        Metric::Chebyshev,
        1,
        &mut out,
    );
    assert_eq!(out, vec![1, 3, 4]);
    // Hex <= 4: (3,4)=7, (5,0)=5, (4,4)=8, (-3,-3)=6, (0,6)=6: all out.
    route(Route::InRange(4), (0, 0), &c, Metric::Hex, 1, &mut out);
    assert!(out.is_empty());
    // Hex <= 5: (5,0)=5 in.
    route(Route::InRange(5), (0, 0), &c, Metric::Hex, 1, &mut out);
    assert_eq!(out, vec![2]);
    // Negative: nobody; zero: only the same place; divisor applies per axis.
    route(
        Route::InRange(-1),
        (0, 0),
        &[(1, 0, 0)],
        Metric::Euclid,
        1,
        &mut out,
    );
    assert!(out.is_empty());
    route(
        Route::InRange(0),
        (0, 0),
        &[(1, 0, 0), (2, 1, 0)],
        Metric::Euclid,
        1,
        &mut out,
    );
    assert_eq!(out, vec![1]);
    route(
        Route::InRange(1),
        (0, 0),
        &[(1, 15, 0), (2, 16, 0)],
        Metric::Manhattan,
        16,
        &mut out,
    );
    assert_eq!(out, vec![1, 2]);
    route(
        Route::InRange(1),
        (0, 0),
        &[(1, 31, 0), (2, 32, 0)],
        Metric::Manhattan,
        16,
        &mut out,
    );
    assert_eq!(out, vec![1]);
}

#[test]
fn route_all_and_to() {
    let c = [(7, 0, 0), (3, 9, 9), (5, -2, 1)];
    let mut out = vec![1];
    route(Route::All, (0, 0), &c, Metric::Euclid, 1, &mut out);
    assert_eq!(out, vec![7, 3, 5]);
    route(Route::To(3), (0, 0), &c, Metric::Hex, 1, &mut out);
    assert_eq!(out, vec![3]);
    route(Route::To(4), (0, 0), &c, Metric::Hex, 1, &mut out);
    assert!(out.is_empty());
}

#[test]
fn hex_distance_axial() {
    assert_eq!(distance(Metric::Hex, 0, 0), 0);
    assert_eq!(distance(Metric::Hex, 1, 0), 1);
    assert_eq!(distance(Metric::Hex, 0, -1), 1);
    // (1, -1) is a neighbour in axial; (1, 1) is two steps away.
    assert_eq!(distance(Metric::Hex, 1, -1), 1);
    assert_eq!(distance(Metric::Hex, -1, 1), 1);
    assert_eq!(distance(Metric::Hex, 1, 1), 2);
    assert_eq!(distance(Metric::Hex, 3, -5), 5);
    assert_eq!(distance(Metric::Euclid, 3, 4), 25);
    assert_eq!(distance(Metric::Manhattan, -3, 4), 7);
    assert_eq!(distance(Metric::Chebyshev, -3, 4), 4);
    let m = i64::from(i32::MAX) - i64::from(i32::MIN);
    assert_eq!(distance(Metric::Hex, m, m), i128::from(m) * 2);
}

#[test]
fn message_to_knowledge() {
    let mut k = Knowledge::new(2, Evict::Oldest).unwrap();
    let m = msg(5, 3);
    assert_eq!(k.learn_message(&m, SOURCE_TOLD, 20), None);
    let f = *k.get(5, 3).unwrap();
    assert_eq!(
        f,
        Fact {
            subject: 5,
            kind: 3,
            pos: [1, 2, 3],
            seen_at: 7,
            strength: 8,
            data: 9,
            source: SOURCE_TOLD,
        }
    );
    // merge: an older message does not overwrite, a newer one does.
    let mut old = msg(5, 3);
    old.seen_at = 6;
    old.data = 1;
    k.merge_message(&old, SOURCE_HEARD, 20);
    assert_eq!(k.get(5, 3).unwrap().data, 9);
    let mut new = msg(5, 3);
    new.seen_at = 8;
    new.data = 2;
    k.merge_message(&new, SOURCE_HEARD, 20);
    assert_eq!(k.get(5, 3).unwrap().data, 2);
    assert_eq!(k.get(5, 3).unwrap().source, SOURCE_HEARD);
}

#[test]
fn inbox_full_orders_across_wrap() {
    let mut b = Inbox::new(3).unwrap();
    b.push(msg(1, 1), u32::MAX - 5);
    b.push(msg(2, 1), 3);
    b.push(msg(3, 1), u32::MAX);
    // Latest is arrival 3 (after the wrap); MAX - 2 is earlier: replaces it.
    assert!(b.push(msg(4, 1), u32::MAX - 2));
    // Latest is now MAX; 1 (after the wrap) is later: refused.
    assert!(!b.push(msg(5, 1), 1));
    let mut out = Vec::new();
    b.take_due(u32::MAX, &mut out);
    let ids: Vec<u32> = out.iter().map(|m| m.subject).collect();
    assert_eq!(ids, vec![1, 3, 4]);
}

#[test]
fn route_nearest_other_metrics_and_three_way_tie() {
    let mut out = Vec::new();
    let c = [(1, 4, 4), (2, 0, 5), (3, 3, 0), (4, -2, -2)];
    // Manhattan: 8, 5, 3, 4.
    route(
        Route::Nearest(3),
        (0, 0),
        &c,
        Metric::Manhattan,
        1,
        &mut out,
    );
    assert_eq!(out, vec![3, 4, 2]);
    // Chebyshev: 4, 5, 3, 2.
    route(
        Route::Nearest(3),
        (0, 0),
        &c,
        Metric::Chebyshev,
        1,
        &mut out,
    );
    assert_eq!(out, vec![4, 3, 1]);
    // Hex: (4,4)=8, (0,5)=5, (3,0)=3, (-2,-2)=4.
    route(Route::Nearest(3), (0, 0), &c, Metric::Hex, 1, &mut out);
    assert_eq!(out, vec![3, 4, 2]);
    // Three-way tie at distance 5, k = 2: the earlier two win.
    let t = [(7, 5, 0), (8, 0, 5), (9, -5, 0)];
    route(Route::Nearest(2), (0, 0), &t, Metric::Euclid, 1, &mut out);
    assert_eq!(out, vec![7, 8]);
}
