//! The attention budget: order, instant cues, the cursor, crowds and
//! the `Skipped` record.

use super::*;
use sagax_core::Sink;
use sagax_debug::{Collector, Overflow};

/// `n` people in focus, ids `100 + i`.
fn crowd(n: usize) -> Vec<Target> {
    (0..n)
        .map(|i| Target {
            id: 100 + i as u32,
            ..person(at(3, i as i32 % 3 - 1))
        })
        .collect()
}

/// An awareness already at `stage` (counter at its threshold).
fn at_stage(p: &SenseProfile, stage: u8) -> Awareness {
    let counter = [0, p.notice_at, p.recognize_at][usize::from(stage)];
    Awareness {
        counter,
        stage,
        pending: stage,
        ..Awareness::default()
    }
}

/// One traced `look`; the `Picked` records as `(id, why)`.
#[allow(clippy::too_many_arguments)]
fn picked(
    now: u32,
    eye: &Eye,
    targets: &[Target],
    aware: &mut [Awareness],
    attention: &mut Attention,
    budget: Budget,
    out: &mut Vec<Percept>,
) -> Vec<(u32, Pick)> {
    let mut sink = Collector::new(10_000, Overflow::DropNewest);
    look_traced(
        &square(),
        &mut OpenAir,
        now,
        eye,
        targets,
        aware,
        attention,
        budget,
        out,
        &mut sink,
    );
    sink.records()
        .filter_map(|r| match r.event {
            PerceptionTrace::Picked { target, why } => Some((target, why)),
            _ => None,
        })
        .collect()
}

fn instant_profile() -> SenseProfile {
    SenseProfile::human(S).with_cue(CueRule {
        bits: 1,
        instant: 1,
        ..CueRule::default()
    })
}

#[test]
fn look_order() {
    let p = instant_profile();
    let mut targets = crowd(6);
    targets[3].cues = 1;
    let budget = Budget {
        checks: 4,
        keep: 2,
        instant_cap: 1,
    };
    // Move the cursor to 2 with a first call that only checks 0 and 1.
    let mut attention = Attention::default();
    let mut out = Vec::new();
    let first = picked(
        0,
        &eye(&p),
        &crowd(6),
        &mut [Awareness::default(); 6],
        &mut attention,
        Budget {
            checks: 2,
            keep: 0,
            instant_cap: 0,
        },
        &mut out,
    );
    assert_eq!(first, [(100, Pick::RoundRobin), (101, Pick::RoundRobin)]);
    assert_eq!(attention.cursor(), 2);
    // Instant 3, kept 4 (stage 2) and 1 (stage 1), then round robin from
    // the cursor: 2, then 3 and 4 are taken, then 5.
    let mut aware2 = [Awareness::default(); 6];
    aware2[1] = at_stage(&p, 1);
    aware2[4] = at_stage(&p, 2);
    let order = picked(
        1,
        &eye(&p),
        &targets,
        &mut aware2,
        &mut attention,
        budget,
        &mut out,
    );
    assert_eq!(
        order,
        [
            (103, Pick::Instant),
            (104, Pick::Kept),
            (101, Pick::Kept),
            (102, Pick::RoundRobin),
            (105, Pick::RoundRobin),
        ]
    );
    assert_eq!(attention.cursor(), 0);
    assert_eq!(aware2[0], Awareness::default(), "unchecked: untouched");
}

#[test]
fn instant_ignores_budget() {
    let p = instant_profile();
    let mut targets = crowd(4);
    targets[1].cues = 1;
    targets[2].cues = 1;
    let mut aware = [Awareness::default(); 4];
    let mut attention = Attention::default();
    let budget = Budget {
        checks: 0,
        keep: 4,
        instant_cap: 1,
    };
    let mut out = Vec::new();
    let order = picked(
        1,
        &eye(&p),
        &targets,
        &mut aware,
        &mut attention,
        budget,
        &mut out,
    );
    assert_eq!(order, [(101, Pick::Instant)]);
    assert_eq!(aware[1].stage(), 1);
    for i in [0, 2, 3] {
        assert_eq!(aware[i], Awareness::default());
    }
    assert_eq!(attention.cursor(), 0);
}

#[test]
fn cursor_wraps() {
    let p = SenseProfile::human(S);
    let targets = crowd(5);
    let mut aware = [Awareness::default(); 5];
    let mut attention = Attention::default();
    let budget = Budget {
        checks: 2,
        keep: 0,
        instant_cap: 0,
    };
    let mut ids = Vec::new();
    for now in 1..=3 {
        let mut out = Vec::new();
        let order = picked(
            now,
            &eye(&p),
            &targets,
            &mut aware,
            &mut attention,
            budget,
            &mut out,
        );
        ids.extend(order.iter().map(|&(id, _)| id));
    }
    assert_eq!(ids, [100, 101, 102, 103, 104, 100]);
    assert_eq!(attention.cursor(), 1);
}

/// Ticks until target 0 is noticed under `look` with `budget`.
fn ticks_to_notice(targets: &[Target], budget: Budget) -> u32 {
    let p = SenseProfile::human(S);
    let mut aware = vec![Awareness::default(); targets.len()];
    let mut attention = Attention::default();
    for now in 1..=10_000 {
        let mut out = Vec::new();
        look(
            &square(),
            &mut OpenAir,
            now,
            &eye(&p),
            targets,
            &mut aware,
            &mut attention,
            budget,
            &mut out,
        );
        if aware[0].stage() >= 1 {
            return now;
        }
    }
    panic!("never noticed");
}

#[test]
fn crowd_slows() {
    let alone = ticks_to_notice(
        &crowd(1),
        Budget {
            checks: 20,
            keep: 0,
            instant_cap: 0,
        },
    );
    let mut many = crowd(20);
    many[0].crowd = 5;
    let budget = Budget {
        checks: 4,
        keep: 0,
        instant_cap: 0,
    };
    let crowded = ticks_to_notice(&many, budget);
    assert_eq!(alone, 4);
    assert!(crowded > 5 * alone, "{crowded} vs {alone}");
}

#[test]
fn look_empty_and_zero_budget() {
    let p = instant_profile();
    let mut attention = Attention::default();
    let budget = Budget {
        checks: 4,
        keep: 4,
        instant_cap: 4,
    };
    let mut out = Vec::new();
    assert!(picked(1, &eye(&p), &[], &mut [], &mut attention, budget, &mut out).is_empty());
    assert!(out.is_empty());
    let targets = crowd(3);
    let mut aware = [Awareness::default(); 3];
    let order = picked(
        1,
        &eye(&p),
        &targets,
        &mut aware,
        &mut attention,
        Budget::default(),
        &mut out,
    );
    assert!(order.is_empty() && out.is_empty());
    assert_eq!(aware, [Awareness::default(); 3]);
    assert_eq!(attention, Attention::default());
}

/// `aware` shorter than `targets`: a debug build asserts, a release build
/// counts only the pairs.
#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "aware must be parallel"))]
fn look_aware_shorter_than_targets() {
    let p = instant_profile();
    let mut attention = Attention::default();
    let budget = Budget {
        checks: 4,
        keep: 4,
        instant_cap: 4,
    };
    let mut out = Vec::new();
    let targets = crowd(3);
    let mut aware = [Awareness::default(); 1];
    let order = picked(
        1,
        &eye(&p),
        &targets,
        &mut aware,
        &mut attention,
        budget,
        &mut out,
    );
    assert_eq!(order, [(100, Pick::RoundRobin)]);
}

/// Counts what it is offered, but says it is off.
struct Off(usize);

impl Sink<PerceptionTrace> for Off {
    fn enabled(&self) -> bool {
        false
    }

    fn emit(&mut self, event: PerceptionTrace) {
        if matches!(event, PerceptionTrace::Skipped { .. }) {
            self.0 += 1;
        }
    }
}

/// One `look` call on fresh state.
type LookCall<'a> = dyn FnMut(&mut [Awareness], &mut Attention, &mut Vec<Percept>) + 'a;

#[test]
fn skipped_emitted_only_when_enabled() {
    let p = SenseProfile::human(S);
    let targets = crowd(6);
    let budget = Budget {
        checks: 2,
        keep: 0,
        instant_cap: 0,
    };
    let run = |sink: &mut LookCall| {
        let mut aware = [Awareness::default(); 6];
        let mut attention = Attention::default();
        let mut out = Vec::new();
        sink(&mut aware, &mut attention, &mut out);
        (aware, attention, out)
    };
    let mut off = Off(0);
    let quiet = run(&mut |aware, attention, out| {
        look_traced(
            &square(),
            &mut OpenAir,
            1,
            &eye(&p),
            &targets,
            aware,
            attention,
            budget,
            out,
            &mut off,
        )
    });
    assert_eq!(off.0, 0);
    let mut sink = Collector::new(1000, Overflow::DropNewest);
    let loud = run(&mut |aware, attention, out| {
        look_traced(
            &square(),
            &mut OpenAir,
            1,
            &eye(&p),
            &targets,
            aware,
            attention,
            budget,
            out,
            &mut sink,
        )
    });
    let skipped: Vec<u32> = sink
        .records()
        .filter_map(|r| match r.event {
            PerceptionTrace::Skipped { target } => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(skipped, [102, 103, 104, 105]);
    assert_eq!(quiet, loud);
    let plain = run(&mut |aware, attention, out| {
        look(
            &square(),
            &mut OpenAir,
            1,
            &eye(&p),
            &targets,
            aware,
            attention,
            budget,
            out,
        )
    });
    assert_eq!(plain, loud);
}

#[test]
fn observe_does_not_touch_attention() {
    let p = SenseProfile::human(S);
    let targets = crowd(5);
    let mut aware = [Awareness::default(); 5];
    let mut attention = Attention::default();
    let budget = Budget {
        checks: 3,
        keep: 0,
        instant_cap: 0,
    };
    let mut out = Vec::new();
    look(
        &square(),
        &mut OpenAir,
        1,
        &eye(&p),
        &targets,
        &mut aware,
        &mut attention,
        budget,
        &mut out,
    );
    let before = attention;
    assert_eq!(before.cursor(), 3);
    for now in 2..10 {
        check(now, &eye(&p), &targets[4], &mut aware[4]);
    }
    assert_eq!(attention, before);
}

#[test]
fn look_trace_equals_plain() {
    let p = instant_profile();
    let mut rng = Rng(9);
    let mut a = vec![Awareness::default(); 8];
    let mut b = a.clone();
    let (mut ca, mut cb) = (Attention::default(), Attention::default());
    let mut sink = Collector::new(100_000, Overflow::DropNewest);
    for now in 1..=200 {
        let targets: Vec<Target> = (0..8)
            .map(|i| Target {
                id: i,
                cues: rng.below(5) as u32 / 4,
                crowd: rng.below(3) as u8,
                ..person(at(rng.range(-3, 10), rng.range(-5, 5)))
            })
            .collect();
        let budget = Budget {
            checks: rng.below(4) as u16,
            keep: rng.below(3) as u16,
            instant_cap: rng.below(2) as u16,
        };
        let (mut pa, mut pb) = (Vec::new(), Vec::new());
        look(
            &square(),
            &mut OpenAir,
            now,
            &eye(&p),
            &targets,
            &mut a,
            &mut ca,
            budget,
            &mut pa,
        );
        look_traced(
            &square(),
            &mut OpenAir,
            now,
            &eye(&p),
            &targets,
            &mut b,
            &mut cb,
            budget,
            &mut pb,
            &mut sink,
        );
        assert_eq!(pa, pb);
        assert_eq!((&a, ca), (&b, cb));
        let checked = usize::from(budget.checks) + usize::from(budget.instant_cap);
        assert!(pa.len() <= MAX_PERCEPTS_PER_CHECK * checked);
    }
}

#[test]
fn count_crowds_counts_the_same_group() {
    let mut targets = crowd(4);
    targets[0].point = [0, 0, 0];
    targets[1].point = [300, 400, 0]; // 500 away
    targets[2].point = [0, -501, 0];
    targets[3].point = [100, 0, 1]; // other level
    let mut out = [9u8; 5];
    count_crowds(&targets, 500, |_| 0, &mut out);
    assert_eq!(out, [1, 1, 0, 0, 9]);
    count_crowds(&targets, 1000, |t| t.id % 2, &mut out);
    assert_eq!(&out[..4], [1, 0, 1, 0]);
}
