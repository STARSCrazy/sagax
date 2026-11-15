//! Hearing: the heard test per spread, damping, channels, fuzz in whole
//! cells, the awareness raise (at most noticed, with the reaction delay)
//! and the stateless use.

use super::*;
use sagax_core::{Cell, Pos};
use sagax_debug::{Collector, Overflow};

/// A sound made at `point` by subject 9, `loudness` fine units, at tick 0.
fn sound(point: Pos, loudness: i32) -> Stimulus {
    Stimulus {
        id: 3,
        source: 9,
        kind: 1,
        point,
        loudness,
        channel: 0,
        disguise: 0,
        at: 0,
    }
}

/// One `hear` on `world` and `map`; whether heard, and the percepts.
#[allow(clippy::too_many_arguments)]
fn listen_on(
    world: &World,
    map: &mut impl SenseMap,
    now: u32,
    ear: &Eye,
    s: &Stimulus,
    cost: Option<i32>,
    aware: Option<&mut Awareness>,
    seed: u64,
) -> (bool, Vec<Percept>) {
    let mut out = Vec::new();
    let heard = hear(world, map, now, ear, s, cost, aware, &mut out, seed);
    (heard, out)
}

/// One `hear` in open air on the square world, no awareness, seed 0.
fn listen(world: &World, ear: &Eye, s: &Stimulus) -> (bool, Vec<Percept>) {
    listen_on(world, &mut OpenAir, 1, ear, s, None, None, 0)
}

/// A square world whose channel 0 goes straight with full damping.
fn straight() -> World {
    let mut w = square();
    w.channels[0].spread = Spread::Straight;
    w
}

/// Damping 512 per cell length in one cell; `edge` says whether sound
/// crosses into that cell at all.
struct Wall {
    cell: Cell,
    edge: bool,
}

impl SenseMap for Wall {
    fn sound(&mut self, cell: Cell) -> u32 {
        if cell == self.cell { 512 } else { 0 }
    }

    fn sound_edge(&mut self, _from: Cell, to: Cell) -> Option<u32> {
        (self.edge || to != self.cell).then_some(0)
    }
}

#[test]
fn straight_damped_by_wall() {
    let w = straight();
    let p = SenseProfile::human(S);
    let e = eye(&p);
    let loud = 2300;
    let mut wall = Wall {
        cell: Cell::new(2, 0, 0),
        edge: true,
    };
    // 4 cells: 1024 + 512 through the wall.
    let (heard, out) = listen_on(&w, &mut wall, 1, &e, &sound(at(4, 0), loud), None, None, 0);
    assert!(heard);
    assert_eq!(out[0].margin, loud - 1536);
    // 8 cells: 2048 + 512 > 2300; without the wall 2048 would be heard.
    let s8 = sound(at(8, 0), loud);
    assert!(!listen_on(&w, &mut wall, 1, &e, &s8, None, None, 0).0);
    assert!(listen(&w, &e, &s8).0);
    // No damping on the channel: the wall does not count.
    let mut quiet = straight();
    quiet.channels[0].damping = 0;
    assert!(listen_on(&quiet, &mut wall, 1, &e, &s8, None, None, 0).0);
    // An edge that lets nothing through: never, however loud.
    let mut shut = Wall {
        cell: Cell::new(2, 0, 0),
        edge: false,
    };
    let s = sound(at(4, 0), i32::MAX);
    let (heard, out) = listen_on(&w, &mut shut, 1, &e, &s, None, None, 0);
    assert!(!heard && out.is_empty());
    // A Circle channel ignores walls.
    assert!(listen_on(&square(), &mut shut, 1, &e, &s, None, None, 0).0);
}

#[test]
fn straight_between_levels_is_unheard() {
    let w = straight();
    let p = SenseProfile::human(S);
    let mut s = sound(at(1, 0), 10 * S);
    s.point[2] = 1;
    assert!(!listen(&w, &eye(&p), &s).0);
    // A cost the game passes counts.
    assert!(listen_on(&w, &mut OpenAir, 1, &eye(&p), &s, Some(S), None, 0).0);
}

#[test]
fn heard_test_uses_profile_and_stats() {
    let p = SenseProfile::human(S);
    let s = sound(at(4, 0), 4 * S);
    // Exactly at the range.
    let (heard, out) = listen(&square(), &eye(&p), &s);
    assert!(heard);
    assert_eq!(out[0].margin, 0);
    // Half hearing: range 2 cells.
    let mut e = eye(&p);
    e.stats.hearing_scale = 128;
    assert!(!listen(&square(), &e, &s).0);
    // Double: margin 4 cells.
    e.stats.hearing_scale = 512;
    assert_eq!(listen(&square(), &e, &s).1[0].margin, 4 * S);
    // Deaf on the channel: not even at its own place.
    let deaf = SenseProfile::human(S).with_hearing(0, 0);
    assert!(!listen(&square(), &eye(&deaf), &sound(at(0, 0), 4 * S)).0);
    // A channel past MAX_CHANNELS: never.
    let mut odd = sound(at(1, 0), 4 * S);
    odd.channel = MAX_CHANNELS as u8;
    assert!(!listen(&square(), &eye(&p), &odd).0);
}

#[test]
fn second_channel_slower_and_vaguer() {
    let mut w = square();
    w.channels[0] = Channel {
        damping: 256,
        fuzz: 0,
        speed: 0,
        spread: Spread::Circle,
    };
    w.channels[1] = Channel {
        damping: 256,
        fuzz: 1024,
        speed: 32,
        spread: Spread::Circle,
    };
    let p = SenseProfile::human(S).with_hearing(1, 256);
    let e = eye(&p);
    let mut s = sound(at(6, 0), 12 * S);
    s.at = 100;
    let truth = Cell::new(6, 0, 0).pos();
    let mut moved = false;
    for seed in 0..50 {
        let (_, fast) = listen_on(&w, &mut OpenAir, 100, &e, &s, None, None, seed);
        assert_eq!((fast[0].arrive_at, fast[0].cell), (100, truth));
        let mut slow_s = s;
        slow_s.channel = 1;
        let (heard, slow) = listen_on(&w, &mut OpenAir, 100, &e, &slow_s, None, None, seed);
        assert!(heard);
        let pc = slow[0];
        assert_eq!(pc.sense, Sense::Hearing(1));
        // 1536 fine units at 32 per tick.
        assert_eq!(pc.arrive_at, 100 + 48);
        // Radius 1536 * 1024 / (256 * 256) = 24 cells.
        let (dx, dy) = (pc.cell[0] - truth[0], pc.cell[1] - truth[1]);
        assert!(dx.abs() <= 24 && dy.abs() <= 24, "{dx}, {dy}");
        assert_eq!(pc.cell[2], 0);
        assert_eq!(pc.point, cell_centre(&w, Cell::from(pc.cell)));
        moved |= pc.cell != truth;
    }
    assert!(moved);
}

#[test]
fn same_seed_same_fuzz() {
    let mut w = square();
    w.channels[0].fuzz = 512;
    let p = SenseProfile::human(S);
    let s = sound(at(8, 0), 12 * S);
    let e = eye(&p);
    let mut other = eye(&p);
    other.id = 2;
    let mut differs = false;
    for seed in 0..20 {
        let a = listen_on(&w, &mut OpenAir, 1, &e, &s, None, None, seed).1;
        let b = listen_on(&w, &mut OpenAir, 1, &e, &s, None, None, seed).1;
        assert_eq!(a, b);
        let c = listen_on(&w, &mut OpenAir, 1, &other, &s, None, None, seed).1;
        differs |= a[0].cell != c[0].cell;
    }
    assert!(differs);
    // Radius 0: 1 cell of cost * 255 / (256 * 256) truncates to 0.
    let mut faint = square();
    faint.channels[0].fuzz = 255;
    let near = sound(at(1, 0), 12 * S);
    for seed in 0..20 {
        let out = listen_on(&faint, &mut OpenAir, 1, &e, &near, None, None, seed).1;
        assert_eq!(out[0].cell, [1, 0, 0]);
        assert_eq!(out[0].point, at(1, 0));
    }
    // Hex: within hex distance r of the true hex.
    let mut hex = World::hex(S);
    hex.channels[0].fuzz = 512;
    let centre = |q, r| cell_centre(&hex, Cell::new(q, r, 0));
    let mut ear = eye(&p);
    ear.point = centre(0, 0);
    let s = sound(centre(6, -2), 12 * S);
    // Cost 6 hexes: radius 6 * 256 * 512 / 65536 = 12.
    let mut moved = false;
    for seed in 0..200 {
        let pc = listen_on(&hex, &mut OpenAir, 1, &ear, &s, None, None, seed).1[0];
        let (dq, dr) = (pc.cell[0] - 6, pc.cell[1] + 2);
        assert!(
            (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 <= 12,
            "{dq}, {dr}"
        );
        assert_eq!(pc.point, cell_centre(&hex, Cell::from(pc.cell)));
        moved |= (dq, dr) != (0, 0);
    }
    assert!(moved);
}

#[test]
fn fuzz_follows_the_mix_formula() {
    let mut w = square();
    w.channels[0].fuzz = 256;
    let p = SenseProfile::human(S);
    let e = eye(&p);
    let s = sound(at(4, 0), 12 * S);
    // Cost 4 cells, radius 4: 9 values per axis.
    let seed = 77;
    let h = mix(seed ^ (u64::from(e.id) << 32) ^ u64::from(s.id));
    let dx = (h % 9) as i32 - 4;
    let dy = ((h >> 16) % 9) as i32 - 4;
    let pc = listen_on(&w, &mut OpenAir, 1, &e, &s, None, None, seed).1[0];
    assert_eq!(pc.cell, [4 + dx, dy, 0]);
}

#[test]
fn hearing_never_past_noticed() {
    let p = SenseProfile {
        hear_gain: 4096,
        ..SenseProfile::human(S)
    };
    let e = eye(&p);
    let mut a = Awareness {
        last_seen: 5,
        last: 5,
        ..Awareness::default()
    };
    for now in 1..=1000 {
        let s = sound(at(1, 0), 12 * S);
        let (heard, _) = listen_on(&square(), &mut OpenAir, now, &e, &s, None, Some(&mut a), 0);
        assert!(heard);
        assert!(a.stage() <= 1 && a.counter() <= p.notice_at);
    }
    assert_eq!((a.stage(), a.counter()), (1, p.notice_at));
    assert_eq!((a.last_seen(), a.last), (5, 5));
    // A counter already above notice_at stays.
    let mut b = Awareness {
        counter: 600,
        stage: 1,
        pending: 1,
        ..Awareness::default()
    };
    let s = sound(at(1, 0), 12 * S);
    listen_on(&square(), &mut OpenAir, 7, &e, &s, None, Some(&mut b), 0);
    assert_eq!((b.counter(), b.stage()), (600, 1));
}

#[test]
fn hearing_gain_scales_with_margin() {
    let p = SenseProfile::human(S);
    let e = eye(&p);
    let mut a = Awareness::default();
    // Range 8 cells, cost 2: margin 6/8, gain 256 * 192 >> 8 = 192.
    let s = sound(at(2, 0), 8 * S);
    listen_on(&square(), &mut OpenAir, 1, &e, &s, None, Some(&mut a), 0);
    assert_eq!((a.counter(), a.stage()), (192, 0));
    let (_, out) = listen_on(&square(), &mut OpenAir, 2, &e, &s, None, Some(&mut a), 0);
    assert_eq!((a.counter(), a.stage()), (256, 1));
    let kinds: Vec<_> = out.iter().map(|p| p.event).collect();
    assert_eq!(kinds, [PerceptKind::Heard, PerceptKind::Noticed]);
}

#[test]
fn hearing_respects_reaction_delay() {
    let p = SenseProfile {
        hear_gain: 4096,
        ..SenseProfile::human(S)
    };
    let mut e = eye(&p);
    e.stats.reaction_ticks = 3;
    let mut a = Awareness::default();
    let s = sound(at(2, 1), 12 * S);
    let mut noticed = Vec::new();
    for now in 10..=20 {
        let (_, out) = listen_on(&square(), &mut OpenAir, now, &e, &s, None, Some(&mut a), 0);
        assert_eq!(out[0].event, PerceptKind::Heard);
        for pc in &out[1..] {
            assert_eq!(pc.event, PerceptKind::Noticed);
            assert!(pc.placed);
            assert_eq!((pc.cell, pc.point), (out[0].cell, out[0].point));
            assert_eq!(pc.sense, Sense::Hearing(0));
            noticed.push(now);
        }
    }
    assert_eq!(noticed, [13]);
}

#[test]
fn stateless_hear() {
    let p = SenseProfile::human(S);
    let mut s = sound(at(3, 2), 12 * S);
    s.at = 4;
    let (heard, out) = listen_on(&square(), &mut OpenAir, 6, &eye(&p), &s, None, None, 0);
    assert!(heard);
    assert_eq!(out.len(), 1);
    let pc = out[0];
    assert_eq!(pc.event, PerceptKind::Heard);
    assert_eq!((pc.observer, pc.subject), (1, 9));
    assert_eq!(pc.sense, Sense::Hearing(0));
    assert_eq!((pc.cell, pc.point, pc.placed), ([3, 2, 0], at(3, 2), true));
    assert_eq!((pc.arrive_at, pc.at, pc.stage), (4, 6, 0));
    assert!(pc.margin > 0);
    // Too far: nothing.
    let (heard, out) = listen(&square(), &eye(&p), &sound(at(13, 0), 12 * S));
    assert!(!heard && out.is_empty());
}

#[test]
fn hear_trace() {
    let p = SenseProfile::human(S);
    let e = eye(&p);
    let mut sink = Collector::new(16, Overflow::DropNewest);
    let mut out = Vec::new();
    let s = sound(at(3, 0), 8 * S);
    let mut a = Awareness::default();
    let w = square();
    assert!(hear_traced(
        &w,
        &mut OpenAir,
        1,
        &e,
        &s,
        None,
        Some(&mut a),
        &mut out,
        0,
        &mut sink
    ));
    let events: Vec<_> = sink.records().map(|r| r.event).collect();
    assert_eq!(
        events,
        [PerceptionTrace::Heard {
            stimulus: 3,
            cost: 3 * S,
            range: 8 * S,
            cell: [3, 0, 0],
        }]
    );
    let mut plain = Vec::new();
    let mut b = Awareness::default();
    hear(
        &w,
        &mut OpenAir,
        1,
        &e,
        &s,
        None,
        Some(&mut b),
        &mut plain,
        0,
    );
    assert_eq!((plain, b), (out, a));
    let mut sink = Collector::new(16, Overflow::DropNewest);
    let far = sound(at(9, 0), 8 * S);
    assert!(!hear_traced(
        &w,
        &mut OpenAir,
        1,
        &e,
        &far,
        None,
        None,
        &mut Vec::new(),
        0,
        &mut sink
    ));
    let events: Vec<_> = sink.records().map(|r| r.event).collect();
    assert_eq!(
        events,
        [PerceptionTrace::Unheard {
            stimulus: 3,
            cost: Some(9 * S),
            range: 8 * S,
        }]
    );
}

#[test]
fn disguised_noise_stays_disguised() {
    let p = SenseProfile {
        hear_gain: 4096,
        ..SenseProfile::human(S)
    };
    let e = eye(&p);
    let s = Stimulus {
        disguise: 100,
        ..sound(at(2, 0), 12 * S)
    };
    // Stateless and with an awareness: every percept is disguised.
    let (_, out) = listen(&square(), &e, &s);
    assert_eq!(out.len(), 1);
    assert!(out[0].disguised);
    let mut a = Awareness::default();
    let (_, out) = listen_on(&square(), &mut OpenAir, 1, &e, &s, None, Some(&mut a), 0);
    let kinds: Vec<_> = out.iter().map(|p| p.event).collect();
    assert_eq!(kinds, [PerceptKind::Heard, PerceptKind::Noticed]);
    assert!(out.iter().all(|p| p.disguised));
    // Hearing never sees through.
    assert!(!a.seen_through() && a.doubt == 0);
    // Seen through already: not disguised.
    let mut b = Awareness {
        flags: crate::awareness::SEEN_THROUGH,
        ..Awareness::default()
    };
    let (_, out) = listen_on(&square(), &mut OpenAir, 1, &e, &s, None, Some(&mut b), 0);
    assert!(out.iter().all(|p| !p.disguised));
    // No disguise: never disguised.
    let (_, out) = listen(&square(), &e, &sound(at(2, 0), 12 * S));
    assert!(!out[0].disguised);
}

#[test]
fn recognition_due_while_hearing_is_unplaced() {
    let p = SenseProfile::human(S);
    let mut e = eye(&p);
    e.stats.reaction_ticks = 3;
    // Recognised by sight at tick 1, still waiting for the reaction time.
    let mut a = Awareness {
        counter: p.recognize_at,
        stage: 1,
        pending: 2,
        pending_since: 1,
        last: 1,
        last_seen: 1,
        ..Awareness::default()
    };
    let s = sound(at(4, 3), 12 * S);
    let (_, out) = listen_on(&square(), &mut OpenAir, 4, &e, &s, None, Some(&mut a), 0);
    assert_eq!(a.stage(), 2);
    let kinds: Vec<_> = out.iter().map(|p| p.event).collect();
    assert_eq!(kinds, [PerceptKind::Heard, PerceptKind::Recognized]);
    assert!(out[0].placed);
    let r = out[1];
    assert!(!r.placed);
    assert_eq!((r.point, r.cell), ([0; 3], [0; 3]));
}
