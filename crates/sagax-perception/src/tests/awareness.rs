//! One check: presets, the gain chain, special senses, instant cues,
//! reaction delay, decay, percepts, credit and the trace.

use super::*;
use sagax_core::{Cell, NoopSink, Pos};
use sagax_debug::{Collector, Overflow};

/// Fog of `density` in one cell only; clear elsewhere.
struct CellFog {
    cell: Cell,
    density: u16,
}

impl SenseMap for CellFog {
    fn sight(&mut self, cell: Cell) -> Medium {
        let density = if cell == self.cell { self.density } else { 0 };
        Medium {
            density,
            ..Medium::default()
        }
    }
}

/// Opaque cells from a list.
struct Walls(Vec<Cell>);

impl SenseMap for Walls {
    fn sight(&mut self, cell: Cell) -> Medium {
        Medium {
            opaque: self.0.contains(&cell),
            ..Medium::default()
        }
    }
}

/// Smoke in the cells `(1, 0)` and `(2, 0)`: thick for the eye, clear for
/// thermal sight.
struct Smoke;

impl SenseMap for Smoke {
    fn sight(&mut self, cell: Cell) -> Medium {
        let smoke = cell.y == 0 && (1..=2).contains(&cell.x);
        Medium {
            density: if smoke { 2048 } else { 0 },
            ..Medium::default()
        }
    }
}

/// The human preset with a larger ceiling and the given base gain.
fn strong(base_gain: u16) -> SenseProfile {
    SenseProfile {
        base_gain,
        max: 65535,
        ..SenseProfile::human(S)
    }
}

#[test]
fn presets_are_as_documented() {
    let h = SenseProfile::human(S);
    assert_eq!(h.senses, SIGHT);
    assert_eq!((h.range, h.near, h.far_gain), (12 * S, 4 * S, 64));
    assert_eq!(
        h.bands,
        [
            Band {
                half_angle: 43,
                gain: 256,
                instant: true
            },
            Band {
                half_angle: 71,
                gain: 64,
                instant: false
            },
            Band::default(),
        ]
    );
    assert_eq!((h.notice_at, h.recognize_at, h.max), (256, 768, 1024));
    assert_eq!((h.decay_per_tick, h.hold, h.hysteresis), (16, 8, 32));
    assert_eq!(h.check_interval, 1);
    assert_eq!(h.crowd_gain, [256, 192, 144, 108, 81, 61, 46, 34]);
    assert_eq!((h.hearing, h.hear_gain), ([256, 0, 0, 0], 256));
    assert!(h.cues.iter().all(|c| c.bits == 0));
    let g = SenseProfile::guard(S);
    assert_eq!((g.range, g.base_gain), (14 * S, 80));
    let w = SenseProfile::watcher(S);
    assert_eq!((w.bands[1].half_angle, w.recognize_at), (100, 1024));
    let b = h
        .with_night()
        .with_thermal()
        .with_scanner(5 * S)
        .with_hearing(1, 128);
    assert_eq!(b.senses, SIGHT | NIGHT | THERMAL | SCANNER);
    assert_eq!((b.scanner_range, b.hearing[1]), (5 * S, 128));
    assert_eq!(h.with_hearing(9, 1).hearing, h.hearing, "no such channel");
    let rule = CueRule {
        bits: 4,
        ..CueRule::default()
    };
    let c = h.with_cue(rule).with_cue(rule);
    assert_eq!((c.cues[0], c.cues[1]), (rule, rule));
    let mut full = h;
    for _ in 0..MAX_CUES + 1 {
        full = full.with_cue(rule);
    }
    assert!(full.cues.iter().all(|&c| c == rule), "full: unchanged");
}

#[test]
fn human_preset_timing() {
    let p = SenseProfile::human(S);
    let e = eye(&p);
    let t = person(at(3, 0));
    let mut a = Awareness::default();
    for k in 1..=12u32 {
        let out = check(k, &e, &t, &mut a);
        let want = match k {
            ..4 => 0,
            4..12 => 1,
            _ => 2,
        };
        assert_eq!(a.stage(), want, "check {k}");
        assert_eq!(has(&out, PerceptKind::Noticed), k == 4, "check {k}");
        assert_eq!(has(&out, PerceptKind::Recognized), k == 12, "check {k}");
    }
}

/// One baseline row: where the target stands, ambient light, fog in the
/// eye's cell, crowd, notice scale, and the counter after one check.
struct Row {
    target: Pos,
    ambient: u8,
    fog: u16,
    crowd: u8,
    notice: u16,
    counter: u16,
}

/// Row 1 has every factor away from 256, chosen so that swapping any two
/// neighbours in the chain changes the result: base gain 30001, periphery
/// (64), 5 cells (232), light 68 (95), clarity 112 (fog 640 over half a
/// cell), size 200, conspicuity 512, crowd 2 (144), notice 230. Each
/// further row sets one of band, distance, light, clarity, crowd, notice
/// back to full. By hand, `g = g * f >> 8` in the fixed order.
const ROWS: [Row; 7] = [
    // 1: 30001 → 7500 → 6796 → 2521 → 1102 → 860 → 1720 → 967 → 868.
    Row {
        target: [128, 128 + 5 * 256, 0],
        ambient: 68,
        fog: 640,
        crowd: 2,
        notice: 230,
        counter: 868,
    },
    // 2: focus (256): 30001 → 30001 → 27188 → 10089 → 4413 → 3447 → 6894 → 3877 → 3483.
    Row {
        target: [128 + 5 * 256, 128, 0],
        ambient: 68,
        fog: 640,
        crowd: 2,
        notice: 230,
        counter: 3483,
    },
    // 3: 3 cells, within `near` (256): 7500 → 7500 → 2783 → 1217 → 950 → 1900 → 1068 → 959.
    Row {
        target: [128, 128 + 3 * 256, 0],
        ambient: 68,
        fog: 640,
        crowd: 2,
        notice: 230,
        counter: 959,
    },
    // 4: daylight (256): 6796 → 6796 → 2973 → 2322 → 4644 → 2612 → 2346.
    Row {
        target: [128, 128 + 5 * 256, 0],
        ambient: 255,
        fog: 640,
        crowd: 2,
        notice: 230,
        counter: 2346,
    },
    // 5: no fog (256): 2521 → 2521 → 1969 → 3938 → 2215 → 1990.
    Row {
        target: [128, 128 + 5 * 256, 0],
        ambient: 68,
        fog: 0,
        crowd: 2,
        notice: 230,
        counter: 1990,
    },
    // 6: alone (256): 1720 → 1720 → 1545.
    Row {
        target: [128, 128 + 5 * 256, 0],
        ambient: 68,
        fog: 640,
        crowd: 0,
        notice: 230,
        counter: 1545,
    },
    // 7: notice 256: 967 → 967.
    Row {
        target: [128, 128 + 5 * 256, 0],
        ambient: 68,
        fog: 640,
        crowd: 2,
        notice: 256,
        counter: 967,
    },
];

/// The target of a baseline row.
fn row_target(row: &Row) -> Target {
    Target {
        size: 200,
        conspicuity: 512,
        crowd: row.crowd,
        ..person(row.target)
    }
}

fn row_map(row: &Row) -> Lit<CellFog> {
    Lit {
        inner: CellFog {
            cell: Cell::new(0, 0, 0),
            density: row.fog,
        },
        ambient: row.ambient,
    }
}

#[test]
fn gain_chain_baseline() {
    let p = strong(30001);
    for (i, row) in ROWS.iter().enumerate() {
        let mut e = eye(&p);
        e.stats.notice_scale = row.notice;
        let t = row_target(row);
        let mut map = row_map(row);
        let line = sight_line(&square(), &mut map, e.point, t.point);
        assert_eq!(line.clarity, if row.fog == 0 { 256 } else { 112 });
        let mut a = Awareness::default();
        check_on(&mut map, 1, &e, &t, &mut a);
        assert_eq!(a.counter(), row.counter, "row {}", i + 1);
    }
}

#[test]
fn outside_cone_and_range_gain_nothing() {
    let p = SenseProfile::human(S);
    let e = eye(&p);
    let range = e.point[0] + 12 * S;
    for point in [at(-3, 0), [range + 1, e.point[1], 0]] {
        let mut a = Awareness::default();
        for now in 1..=20 {
            assert!(check(now, &e, &person(point), &mut a).is_empty());
        }
        assert_eq!(a.counter(), 0);
        assert!(!a.in_view());
    }
    // Exactly at the range: seen.
    let mut a = Awareness::default();
    check(1, &e, &person([range, e.point[1], 0]), &mut a);
    assert!(a.counter() > 0);
}

#[test]
fn dark_and_night() {
    let p = strong(1000);
    let dark = |p: &SenseProfile, glow: u8| {
        let e = eye(p);
        let t = Target {
            glow,
            ..person(at(3, 0))
        };
        let mut a = Awareness::default();
        let mut map = Lit {
            inner: OpenAir,
            ambient: 0,
        };
        check_on(&mut map, 1, &e, &t, &mut a);
        a.counter()
    };
    assert_eq!(p.dark_gain, 32);
    assert_eq!(dark(&p, 0), 1000 * 32 / 256);
    assert_eq!(dark(&p.with_night(), 0), 1000);
    assert_eq!(dark(&p, 255), 1000);
}

#[test]
fn thermal_through_smoke() {
    let world = square();
    let line = sight_line(&world, &mut Smoke, at(0, 0), at(3, 0));
    assert_eq!(line.clarity, 0);
    let warm = Target {
        heat: 255,
        ..person(at(3, 0))
    };
    // Plain sight: nothing.
    let p = SenseProfile::human(S);
    let mut a = Awareness::default();
    for now in 1..=10 {
        assert!(check_on(&mut Smoke, now, &eye(&p), &warm, &mut a).is_empty());
    }
    // Thermal: noticed in 4 checks, as sight would in clear air.
    let p = p.with_thermal();
    let mut a = Awareness::default();
    let mut noticed = None;
    for now in 1..=10 {
        let out = check_on(&mut Smoke, now, &eye(&p), &warm, &mut a);
        if let Some(n) = out.iter().find(|p| p.event == PerceptKind::Noticed) {
            assert_eq!(n.sense, Sense::Thermal);
            noticed.get_or_insert(now);
        }
    }
    assert_eq!(noticed, Some(4));
    // A cold target stays unseen to thermal sight.
    let mut a = Awareness::default();
    check_on(&mut Smoke, 1, &eye(&p), &person(at(3, 0)), &mut a);
    assert_eq!(a.counter(), 0);
}

#[test]
fn scanner_through_wall_only_noticed() {
    let p = SenseProfile::human(S).with_scanner(5 * S);
    let mut wall = Walls(vec![Cell::new(1, 0, 0)]);
    let mut a = Awareness::default();
    for now in 1..=100 {
        let out = check_on(&mut wall, now, &eye(&p), &person(at(3, 0)), &mut a);
        assert!(!has(&out, PerceptKind::Recognized));
        for pc in &out {
            assert_eq!(pc.sense, Sense::Scanner);
        }
    }
    assert_eq!(a.stage(), 1);
    assert_eq!(a.counter(), p.notice_at);
    let mut a = Awareness::default();
    for now in 1..=100 {
        assert!(check_on(&mut wall, now, &eye(&p), &person(at(6, 0)), &mut a).is_empty());
    }
    assert_eq!(a.counter(), 0);
}

fn instant_profile() -> SenseProfile {
    SenseProfile::human(S).with_cue(CueRule {
        bits: 1,
        instant: 2,
        min_clarity: 128,
        ..CueRule::default()
    })
}

#[test]
fn instant_cue_recognises() {
    let p = instant_profile();
    let armed = |point| Target {
        cues: 1,
        ..person(point)
    };
    let mut a = Awareness::default();
    let out = check(1, &eye(&p), &armed(at(3, 0)), &mut a);
    assert!(has(&out, PerceptKind::Noticed) && has(&out, PerceptKind::Recognized));
    assert_eq!(a.stage(), 2);
    // Periphery: no instant.
    let mut a = Awareness::default();
    check(1, &eye(&p), &armed(at(0, 3)), &mut a);
    assert_eq!(a.stage(), 0);
    // Fog: clarity 100 (depth 368 over half a cell) is below 128.
    let mut fog = CellFog {
        cell: Cell::new(0, 0, 0),
        density: 736,
    };
    assert_eq!(
        sight_line(&square(), &mut fog, at(0, 0), at(3, 0)).clarity,
        100
    );
    let mut a = Awareness::default();
    check_on(&mut fog, 1, &eye(&p), &armed(at(3, 0)), &mut a);
    assert_eq!(a.stage(), 0);
    assert!(a.counter() > 0);
}

#[test]
fn cue_range_limits() {
    let p = SenseProfile::human(S).with_cue(CueRule {
        bits: 1,
        instant: 2,
        range: 10 * S,
        ..CueRule::default()
    });
    for (x, stage) in [(11, 0), (10, 2), (9, 2)] {
        let t = Target {
            cues: 1,
            ..person(at(x, 0))
        };
        let mut a = Awareness::default();
        check(1, &eye(&p), &t, &mut a);
        assert_eq!(a.stage(), stage, "{x} cells");
    }
}

/// Checks every tick from 1 to `last` with the reaction time `reaction(now)`
/// and the target `target(now)`; the ticks with a `Noticed` percept.
fn noticed_at(
    p: &SenseProfile,
    last: u32,
    reaction: impl Fn(u32) -> u16,
    target: impl Fn(u32) -> Target,
    aware: &mut Awareness,
) -> Vec<u32> {
    let mut ticks = Vec::new();
    for now in 1..=last {
        let mut e = eye(p);
        e.stats.reaction_ticks = reaction(now);
        if has(&check(now, &e, &target(now), aware), PerceptKind::Noticed) {
            ticks.push(now);
        }
    }
    ticks
}

#[test]
fn reaction_delay() {
    let p = SenseProfile::human(S);
    let walk = |_| person(at(3, 0));
    // The counter reaches notice_at at tick 4.
    let mut a = Awareness::default();
    assert_eq!(noticed_at(&p, 10, |_| 5, walk, &mut a), [9]);
    assert_eq!((a.stage(), a.pending, a.pending_since), (1, 1, 4));
    // Shortened to 2 at tick 7: 7 - 4 >= 2, due at once.
    let mut a = Awareness::default();
    let shorter = |now| if now >= 7 { 2 } else { 5 };
    assert_eq!(noticed_at(&p, 20, shorter, walk, &mut a), [7]);
    // Lengthened to 9 at tick 8: due at 4 + 9.
    let mut a = Awareness::default();
    let longer = |now| if now >= 8 { 9 } else { 5 };
    assert_eq!(noticed_at(&p, 20, longer, walk, &mut a), [13]);
    // 0: in the call that reached it.
    let mut a = Awareness::default();
    assert_eq!(noticed_at(&p, 20, |_| 0, walk, &mut a), [4]);
    // A further rise while waiting keeps pending_since: the instant cue at
    // tick 6 lifts the pending stage to 2, both count at 4 + 5.
    let q = instant_profile();
    let mut a = Awareness::default();
    let mut seen = Vec::new();
    for now in 1..=12 {
        let mut e = eye(&q);
        e.stats.reaction_ticks = 5;
        let t = Target {
            cues: u32::from(now >= 6),
            ..person(at(3, 0))
        };
        let out = check(now, &e, &t, &mut a);
        for pc in out {
            if matches!(pc.event, PerceptKind::Noticed | PerceptKind::Recognized) {
                seen.push((now, pc.event));
            }
        }
        if (6..9).contains(&now) {
            assert_eq!((a.stage(), a.pending, a.pending_since), (0, 2, 4));
        }
    }
    assert_eq!(
        seen,
        [(9, PerceptKind::Noticed), (9, PerceptKind::Recognized)]
    );
    // The counter falls below before the rise is due: nothing, cleared.
    let fast = SenseProfile {
        hold: 0,
        decay_per_tick: 300,
        ..p
    };
    let mut a = Awareness::default();
    let hide = |now| person(if now <= 4 { at(3, 0) } else { at(-3, 0) });
    assert!(noticed_at(&fast, 60, |_| 50, hide, &mut a).is_empty());
    assert_eq!((a.counter(), a.stage(), a.pending), (0, 0, 0));
}

#[test]
fn decay_and_hysteresis() {
    let p = SenseProfile::human(S);
    let mut a = Awareness::default();
    for now in 1..=5 {
        check(now, &eye(&p), &person(at(3, 0)), &mut a);
    }
    assert_eq!((a.counter(), a.stage(), a.last_seen()), (320, 1, 5));
    let mut lost = Vec::new();
    let mut forgotten = Vec::new();
    for now in 6..=30 {
        let out = check(now, &eye(&p), &person(at(-3, 0)), &mut a);
        if has(&out, PerceptKind::Lost) {
            lost.push(now);
        }
        if has(&out, PerceptKind::Forgotten) {
            forgotten.push(now);
        }
        // Hold: nothing before tick 5 + 8.
        let decayed = now.saturating_sub(12).min(20) as u16;
        assert_eq!(a.counter(), 320u16.saturating_sub(16 * decayed), "{now}");
        // Below 256 the stage holds until below 256 - 32.
        let want = u8::from(a.counter() >= 224);
        assert_eq!(a.stage(), want, "{now}");
    }
    assert_eq!(lost, [6]);
    assert_eq!(forgotten, [19]);
}

#[test]
fn seen_every_check() {
    let p = SenseProfile::human(S);
    let mut a = Awareness::default();
    let mut cells = Vec::new();
    for now in 1..=9 {
        let x = if now <= 4 { 3 } else { now as i32 - 2 };
        let t = person([at(x, 0)[0], at(x, 0)[1] + 10, 0]);
        let out = check(now, &eye(&p), &t, &mut a);
        if a.stage() >= 1 {
            let last = out.last().unwrap();
            assert_eq!(last.event, PerceptKind::Seen);
            assert_eq!(
                out.iter().filter(|p| p.event == PerceptKind::Seen).count(),
                1
            );
            assert_eq!(last.point, t.point);
            assert_eq!(last.cell, cell_of(&square(), t.point).pos());
            if now >= 5 {
                cells.push(last.cell);
            }
        }
    }
    cells.dedup();
    assert_eq!(cells.len(), 5);
    // Random walks in and out of view: the per-check rules hold.
    let p = instant_profile();
    let mut rng = Rng(11);
    let mut a = Awareness::default();
    for now in 1..=2000 {
        let t = Target {
            cues: rng.below(4) as u32,
            ..person(at(rng.range(-6, 14), rng.range(-6, 6)))
        };
        let mut e = eye(&p);
        e.stats.reaction_ticks = rng.below(4) as u16;
        let out = check(now, &e, &t, &mut a);
        assert!(out.len() <= MAX_PERCEPTS_PER_CHECK);
        assert!(!(has(&out, PerceptKind::Lost) && has(&out, PerceptKind::Seen)));
        let seen = has(&out, PerceptKind::Seen);
        assert_eq!(seen, a.stage() >= 1 && a.in_view(), "{now}");
        if seen {
            assert_eq!(out.last().unwrap().event, PerceptKind::Seen);
        }
        for pc in &out {
            assert_eq!(pc.stage, a.stage());
            assert_eq!((pc.observer, pc.subject, pc.at), (1, 7, now));
        }
    }
}

#[test]
fn cues_reported_once() {
    let p = SenseProfile::human(S);
    let mut a = Awareness::default();
    for now in 1..=12 {
        let out = check(now, &eye(&p), &person(at(3, 0)), &mut a);
        assert!(
            !out.iter()
                .any(|p| matches!(p.event, PerceptKind::Cues { .. }))
        );
    }
    assert_eq!(a.stage(), 2);
    let armed = Target {
        cues: 2,
        ..person(at(3, 0))
    };
    let out = check(13, &eye(&p), &armed, &mut a);
    assert!(has(&out, PerceptKind::Cues { new: 2 }));
    assert_eq!(out.last().unwrap().cues, 2);
    let out = check(14, &eye(&p), &armed, &mut a);
    assert!(
        !out.iter()
            .any(|p| matches!(p.event, PerceptKind::Cues { .. }))
    );
}

#[test]
fn every_4_equals_every_1() {
    let p = SenseProfile {
        check_interval: 4,
        ..SenseProfile::human(S)
    };
    let t = person(at(0, 8));
    let (mut a, mut b) = (Awareness::default(), Awareness::default());
    for now in 1..=40 {
        check(now, &eye(&p), &t, &mut a);
        if now % 4 == 0 {
            check(now, &eye(&p), &t, &mut b);
            assert_eq!(a.counter(), b.counter(), "{now}");
        }
    }
    assert!(a.counter() > 0 && a.counter() < p.max);
}

#[test]
fn skipped_time_not_credited() {
    let p = SenseProfile {
        check_interval: 4,
        ..SenseProfile::human(S)
    };
    let t = person(at(3, 0));
    let mut a = Awareness::default();
    check(0, &eye(&p), &t, &mut a);
    assert_eq!(a.counter(), 0, "no time passed yet");
    check(40, &eye(&p), &t, &mut a);
    assert_eq!(a.counter(), 4 * 64);
}

#[test]
fn credit_and_reaction_survive_wrap() {
    let p = SenseProfile::human(S);
    let t = person(at(3, 0));
    let mut a = Awareness::default();
    let start = u32::MAX - 2;
    let mut noticed = None;
    for k in 0..8u32 {
        let now = start.wrapping_add(k);
        let mut e = eye(&p);
        e.stats.reaction_ticks = 2;
        if has(&check(now, &e, &t, &mut a), PerceptKind::Noticed) {
            noticed = Some(k);
        }
        assert_eq!(a.counter(), 64 * (k as u16 + 1));
    }
    // Reached at k = 3 (now 0), reported two ticks later.
    assert_eq!(noticed, Some(5));
}

#[test]
fn gain_saturates_at_max() {
    let mut p = SenseProfile {
        max: 65535,
        recognize_at: 65535,
        ..SenseProfile::human(S)
    };
    for _ in 0..MAX_CUES {
        p = p.with_cue(CueRule {
            bits: 1,
            gain: 1024,
            ..CueRule::default()
        });
    }
    let t = Target {
        conspicuity: 2048,
        cues: 1,
        ..person(at(3, 0))
    };
    let mut a = Awareness::default();
    check(1, &eye(&p), &t, &mut a);
    assert_eq!(a.counter(), 65535);
    check(2, &eye(&p), &t, &mut a);
    assert_eq!(a.counter(), 65535);
}

#[test]
fn zero_vector_is_in_view() {
    let p = SenseProfile::human(S);
    let e = eye(&p);
    let mut a = Awareness::default();
    check(1, &e, &person(e.point), &mut a);
    assert!(a.in_view());
    assert_eq!(a.counter(), 64);
    assert!(in_sight(&square(), &mut OpenAir, &e, e.point));
}

#[test]
fn in_sight_matches_observe_geometry() {
    let p = strong(65535);
    let mut rng = Rng(3);
    let mut hits = [0; 2];
    for case in 0..20 {
        let walls: Vec<Cell> = (0..30)
            .map(|_| Cell::new(rng.range(-8, 8), rng.range(-8, 8), 0))
            .collect();
        let mut map = Walls(walls);
        let mut e = eye(&p);
        e.point = [rng.range(-512, 512), rng.range(-512, 512), 0];
        e.facing = rng.below(256) as u8;
        let point: Pos = [rng.range(-4000, 4000), rng.range(-4000, 4000), 0];
        let mut a = Awareness::default();
        check_on(&mut map, 1, &e, &person(point), &mut a);
        let sees = in_sight(&square(), &mut map, &e, point);
        assert_eq!(sees, a.counter() > 0, "case {case}");
        hits[usize::from(sees)] += 1;
    }
    assert!(hits[0] > 0 && hits[1] > 0, "{hits:?}");
}

#[test]
fn trace_equals_plain() {
    let p = instant_profile().with_scanner(2 * S);
    let world = square();
    let mut rng = Rng(5);
    let (mut a, mut b) = (Awareness::default(), Awareness::default());
    let mut sink = Collector::new(100_000, Overflow::DropNewest);
    for now in 1..=300 {
        let t = Target {
            cues: rng.below(3) as u32,
            ..person(at(rng.range(-4, 10), rng.range(-4, 4)))
        };
        let mut e = eye(&p);
        e.stats.reaction_ticks = rng.below(3) as u16;
        let (mut plain, mut traced) = (Vec::new(), Vec::new());
        let mut wall = Walls(vec![Cell::new(1, 1, 0)]);
        observe(&world, &mut wall, now, &e, &t, &mut a, &mut plain);
        observe_traced(
            &world,
            &mut wall,
            now,
            &e,
            &t,
            &mut b,
            &mut traced,
            &mut sink,
        );
        assert_eq!(plain, traced);
        assert_eq!(a, b);
    }
    assert_eq!(sink.dropped(), 0);
    assert!(sink.records().len() > 100);
    // The Gain record of baseline row 1.
    let row = &ROWS[0];
    let q = strong(30001);
    let mut e = eye(&q);
    e.stats.notice_scale = row.notice;
    let t = row_target(row);
    let mut sink = Collector::new(64, Overflow::DropNewest);
    let mut out = Vec::new();
    let mut aware = Awareness::default();
    let mut map = row_map(row);
    observe_traced(&world, &mut map, 1, &e, &t, &mut aware, &mut out, &mut sink);
    let gains: Vec<_> = sink
        .records()
        .filter(|r| matches!(r.event, PerceptionTrace::Gain { .. }))
        .map(|r| r.event)
        .collect();
    assert_eq!(
        gains,
        [PerceptionTrace::Gain {
            target: 7,
            band: 1,
            distance: 232,
            light: 95,
            clarity: 112,
            crowd: 144,
            gain: 868,
            sense: Sense::Sight,
        }]
    );
    let lines = sink
        .records()
        .filter(|r| matches!(r.event, PerceptionTrace::Line { .. }))
        .count();
    assert_eq!(lines, 1);
    let mut silent = Vec::new();
    observe_traced(
        &world,
        &mut row_map(row),
        1,
        &e,
        &t,
        &mut Awareness::default(),
        &mut silent,
        &mut NoopSink,
    );
    assert_eq!(silent, out);
}
