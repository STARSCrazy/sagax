//! Disguise: it holds by default, under every call sequence; seeing
//! through it only when the game turns it on.

use super::*;

/// The observer's rule: a disguise hides cue bit 1.
fn hides_one(see_through: Option<SeeThrough>) -> DisguiseRule {
    DisguiseRule {
        hides: 1,
        see_through,
    }
}

/// A disguised person showing cues 1 and 2.
fn agent(point: Pos, disguise: u8) -> Target {
    Target {
        disguise,
        cues: 3,
        ..person(point)
    }
}

/// The cue bits a percept shows, its own and the new ones of `Cues`.
fn bits(p: &Percept) -> u32 {
    match p.event {
        PerceptKind::Cues { new } => new | p.cues,
        _ => p.cues,
    }
}

#[test]
fn disguise_holds() {
    let p = SenseProfile::human(S);
    let mut e = eye(&p);
    e.disguise = hides_one(None);
    let mut a = Awareness::default();
    let mut cues = 0;
    for now in 1..=500 {
        for pc in check(now, &e, &agent(at(2, 0), 200), &mut a) {
            assert!(pc.disguised);
            assert_eq!(bits(&pc) & 1, 0);
            assert_ne!(pc.event, PerceptKind::SeenThrough);
            cues |= bits(&pc);
        }
    }
    assert_eq!(cues, 2, "the other cue is seen");
    assert_eq!(a.stage(), 2);
    assert!(!a.seen_through());
    assert_eq!(a.doubt, 0);
}

#[test]
fn disguise_holds_random_sequences() {
    let mut rng = Rng(21);
    for seq in 0..500 {
        let rule = CueRule {
            bits: 1 + rng.below(3) as u32,
            gain: rng.below(1024) as u16,
            instant: rng.below(3) as u8,
            min_clarity: rng.below(256) as u16,
            range: 0,
        };
        let p = SenseProfile::human(S).with_cue(rule).with_scanner(3 * S);
        let hides = 1 + rng.below(7) as u32;
        let n = 1 + rng.below(4) as usize;
        let mut aware = vec![Awareness::default(); n];
        let mut attention = Attention::default();
        let mut now = rng.below(1000) as u32;
        for _ in 0..40 {
            now = now.wrapping_add(rng.below(5) as u32);
            let mut e = eye(&p);
            e.facing = rng.below(256) as u8;
            e.stats.reaction_ticks = rng.below(3) as u16;
            e.disguise = DisguiseRule {
                hides,
                see_through: None,
            };
            let targets: Vec<Target> = (0..n)
                .map(|i| Target {
                    id: i as u32,
                    disguise: 1 + rng.below(255) as u8,
                    cues: rng.below(8) as u32,
                    crowd: rng.below(3) as u8,
                    ..person(at(rng.range(-6, 6), rng.range(-6, 6)))
                })
                .collect();
            let mut out = Vec::new();
            if rng.below(2) == 0 {
                let i = rng.below(n as u64) as usize;
                observe(
                    &square(),
                    &mut OpenAir,
                    now,
                    &e,
                    &targets[i],
                    &mut aware[i],
                    &mut out,
                );
            } else {
                let budget = Budget {
                    checks: rng.below(4) as u16,
                    keep: rng.below(3) as u16,
                    instant_cap: rng.below(3) as u16,
                };
                look(
                    &square(),
                    &mut OpenAir,
                    now,
                    &e,
                    &targets,
                    &mut aware,
                    &mut attention,
                    budget,
                    &mut out,
                );
            }
            for pc in &out {
                assert!(pc.disguised, "seq {seq}");
                assert_ne!(pc.event, PerceptKind::SeenThrough, "seq {seq}");
                assert_eq!(bits(pc) & hides, 0, "seq {seq}");
            }
        }
        for a in &aware {
            assert!(!a.seen_through() && a.doubt == 0, "seq {seq}");
        }
    }
}

#[test]
fn see_through_when_on() {
    let p = SenseProfile::human(S);
    let mut e = eye(&p);
    e.disguise = hides_one(Some(SeeThrough {
        rate: 64,
        near: 2 * S,
        at: 1024,
    }));
    let t = agent(at(1, 0), 128);
    let mut a = Awareness::default();
    let mut through = Vec::new();
    let mut first_one = None;
    for now in 1..=80 {
        for pc in check(now, &e, &t, &mut a) {
            if pc.event == PerceptKind::SeenThrough {
                through.push(now);
            }
            if bits(&pc) & 1 != 0 {
                first_one.get_or_insert(now);
                assert!(!pc.disguised);
            }
            if through.is_empty() {
                assert!(pc.disguised);
            }
        }
    }
    // Recognised at 12; then 64 * 128 / 256 = 32 per check, 1024 at 32 more.
    assert_eq!(through, [12 + 32]);
    assert_eq!(first_one, Some(44));
    assert!(a.seen_through());
    let out = check(81, &e, &t, &mut a);
    assert!(!out.last().unwrap().disguised);
    assert_eq!(out.last().unwrap().cues, 3);
    // Until reset: disguised again from the start.
    a.reset();
    assert_eq!(a, Awareness::default());
    for now in 82..=100 {
        for pc in check(now, &e, &t, &mut a) {
            assert!(pc.disguised);
            assert_eq!(bits(&pc) & 1, 0);
        }
    }
}

#[test]
fn disguise_zero_breaks_at_once() {
    let p = SenseProfile::human(S);
    let mut e = eye(&p);
    e.disguise = hides_one(None);
    let mut a = Awareness::default();
    for now in 1..=12 {
        check(now, &e, &agent(at(2, 0), 200), &mut a);
    }
    assert_eq!(a.stage(), 2);
    let out = check(13, &e, &agent(at(2, 0), 0), &mut a);
    assert!(out.iter().all(|p| !p.disguised));
    assert!(has(&out, PerceptKind::Cues { new: 1 }));
    assert!(!a.seen_through(), "no see-through, just no disguise");
}
