use super::*;
use sagax_core::CallbackSink;

#[test]
fn curves() {
    assert_eq!(Curve::Linear.apply(5, 0, 10), ONE / 2);
    assert_eq!(Curve::Linear.apply(-3, 0, 10), 0);
    assert_eq!(Curve::Linear.apply(99, 0, 10), ONE);
    assert_eq!(Curve::Inverse.apply(0, 0, 10), ONE);
    assert_eq!(Curve::Inverse.apply(10, 0, 10), 0);
    assert_eq!(Curve::Quadratic.apply(5, 0, 10), mul(ONE / 2, ONE / 2));
    assert_eq!(Curve::InverseQuadratic.apply(0, 0, 10), ONE);
    assert_eq!(Curve::Step.apply(3, 3, 0), ONE);
    assert_eq!(Curve::Step.apply(2, 3, 0), 0);
    assert_eq!(Curve::InverseStep.apply(2, 3, 0), ONE);
    // max <= min: a step at max.
    assert_eq!(Curve::Linear.apply(4, 4, 4), ONE);
    assert_eq!(Curve::Linear.apply(3, 4, 4), 0);
    // Extreme ranges do not overflow.
    assert_eq!(Curve::Linear.apply(0, i32::MIN, i32::MAX), ONE / 2);
    for n in 0..6 {
        assert_eq!(Curve::from_u32(n).map(|c| c as u32), Some(n));
    }
}

#[test]
fn compensation() {
    assert_eq!(compensate(0, 3), 0);
    assert_eq!(compensate(ONE, 3), ONE);
    assert_eq!(compensate(ONE / 2, 1), ONE / 2);
    // Two factors of 0.5 = 0.25, made up to 0.25 + 0.75 * 0.5 * 0.25 = 0.34375.
    let p = compensate(mul(ONE / 2, ONE / 2), 2);
    assert!((22_500..22_600).contains(&p), "{p}");
}

fn two_options() -> Decider {
    let mut d = Decider::new();
    let attack = d.add_option(ONE);
    let flee = d.add_option(ONE);
    // input 0: distance to the target (near is better), 1: own health 0..100.
    assert!(d.add_consideration(attack, Consideration::new(0, Curve::Inverse, 1, 10)));
    assert!(d.add_consideration(attack, Consideration::new(1, Curve::Linear, 0, 100)));
    assert!(d.add_consideration(flee, Consideration::new(1, Curve::InverseQuadratic, 0, 100)));
    assert!(!d.add_consideration(9, Consideration::new(0, Curve::Linear, 0, 1)));
    d
}

#[test]
fn best_choice_follows_the_inputs() {
    let mut d = two_options();
    let candidates = [
        Candidate::new(0, 7),
        Candidate::new(0, 8),
        Candidate::new(1, 0),
    ];
    let mut scores = Vec::new();
    // Healthy: attack the near target 8.
    let inputs = [5, 90, 2, 90, 0, 90];
    let c = d
        .decide(&candidates, &inputs, 2, Pick::Best, 0, &mut scores)
        .unwrap()
        .unwrap();
    assert_eq!(c.candidate, 1);
    assert_eq!(scores.len(), 3);
    // Nearly dead: flee.
    let inputs = [5, 5, 2, 5, 0, 5];
    let c = d
        .decide(&candidates, &inputs, 2, Pick::Best, 0, &mut scores)
        .unwrap()
        .unwrap();
    assert_eq!(c.candidate, 2);
}

#[test]
fn zero_rules_out_and_ties_take_the_smaller_index() {
    let mut d = two_options();
    let candidates = [Candidate::new(0, 1), Candidate::new(0, 2)];
    let mut scores = Vec::new();
    // Both out of reach: nothing.
    let none = d
        .decide(
            &candidates,
            &[10, 50, 12, 50],
            2,
            Pick::Best,
            0,
            &mut scores,
        )
        .unwrap();
    assert_eq!(none, None);
    assert_eq!(scores, [0, 0]);
    // Equal: the first one.
    let c = d
        .decide(&candidates, &[3, 50, 3, 50], 2, Pick::Best, 0, &mut scores)
        .unwrap()
        .unwrap();
    assert_eq!(c.candidate, 0);
    // Weighted with top 1 is the best, too.
    let c = d
        .decide(
            &candidates,
            &[3, 50, 3, 50],
            2,
            Pick::Weighted { top: 1 },
            12345,
            &mut scores,
        )
        .unwrap()
        .unwrap();
    assert_eq!(c.candidate, 0);
}

#[test]
fn chance_is_seeded_and_proportional() {
    let mut d = Decider::new();
    let a = d.add_option(3 * ONE);
    let b = d.add_option(ONE);
    let _ = (a, b);
    let candidates = [Candidate::new(0, 0), Candidate::new(1, 0)];
    let mut scores = Vec::new();
    let mut count = [0u32; 2];
    for seed in 0..4000u64 {
        let c = d
            .decide(
                &candidates,
                &[],
                0,
                Pick::Weighted { top: 0 },
                seed,
                &mut scores,
            )
            .unwrap()
            .unwrap();
        count[c.candidate] += 1;
        // Same seed, same choice.
        let again = d
            .decide(
                &candidates,
                &[],
                0,
                Pick::Weighted { top: 0 },
                seed,
                &mut scores,
            )
            .unwrap()
            .unwrap();
        assert_eq!(c, again);
    }
    // About 3:1.
    assert!((2800..3200).contains(&count[0]), "{count:?}");
    let mut uniform = [0u32; 2];
    for seed in 0..4000u64 {
        let c = d
            .decide(
                &candidates,
                &[],
                0,
                Pick::Uniform { top: 2 },
                seed,
                &mut scores,
            )
            .unwrap()
            .unwrap();
        uniform[c.candidate] += 1;
    }
    assert!((1800..2200).contains(&uniform[0]), "{uniform:?}");
}

#[test]
fn fixed_seed_gives_a_fixed_answer() {
    // Pins the random function: C and JavaScript must give the same.
    assert_eq!(mix(0), 0xe220_a839_7b1d_cdaf);
    assert_eq!(mix(42), 0xbdd7_3226_2feb_6e95);
}

#[test]
fn errors() {
    let mut d = two_options();
    let mut s = Vec::new();
    assert_eq!(
        d.decide(&[Candidate::new(5, 0)], &[0, 0], 2, Pick::Best, 0, &mut s),
        Err(DecideError::UnknownOption { candidate: 0 })
    );
    assert_eq!(
        d.decide(&[Candidate::new(0, 0)], &[0], 1, Pick::Best, 0, &mut s),
        Err(DecideError::InputOutOfRange {
            candidate: 0,
            input: 1
        })
    );
    assert_eq!(
        d.decide(&[Candidate::new(0, 0)], &[0], 2, Pick::Best, 0, &mut s),
        Err(DecideError::InputsTooShort)
    );
}

#[test]
fn trace_shows_every_factor() {
    let mut d = two_options();
    let mut events = Vec::new();
    let mut scores = Vec::new();
    d.decide_traced(
        &[Candidate::new(0, 0), Candidate::new(1, 0)],
        &[1, 100, 1, 100],
        2,
        Pick::Best,
        0,
        &mut scores,
        &mut CallbackSink(|e| events.push(e)),
    )
    .unwrap();
    assert_eq!(
        events,
        [
            DecideEvent::Factor {
                candidate: 0,
                consideration: 0,
                input: 1,
                value: ONE
            },
            DecideEvent::Factor {
                candidate: 0,
                consideration: 1,
                input: 100,
                value: ONE
            },
            DecideEvent::Scored {
                candidate: 0,
                product: ONE,
                score: ONE
            },
            DecideEvent::Factor {
                candidate: 1,
                consideration: 0,
                input: 100,
                value: 0
            },
            DecideEvent::Scored {
                candidate: 1,
                product: 0,
                score: 0
            },
            DecideEvent::Chosen {
                candidate: 0,
                score: ONE
            },
        ]
    );
}

const H: u32 = ONE / 2;
const P95: u32 = 62258; // 0.95

#[test]
fn new_curves() {
    assert_eq!(Curve::SCurve.apply(0, 0, 10), 0);
    assert_eq!(Curve::SCurve.apply(10, 0, 10), ONE);
    let mid = Curve::SCurve.apply(5, 0, 10);
    assert!((32_700..32_800).contains(&mid), "{mid}");
    // flat near the ends: at 0.1 far below linear
    assert!(Curve::SCurve.apply(1, 0, 10) < ONE / 20);
    assert_eq!(Curve::InverseSCurve.apply(0, 0, 10), ONE);
    // sqrt(0.25) = 0.5
    let root = Curve::Root.apply(25, 0, 100);
    assert!((32_760..=32_768).contains(&root), "{root}");
    assert_eq!(Curve::Root.apply(100, 0, 100), ONE);
    assert!(Curve::Cubic.apply(5, 0, 10) < ONE / 8 + 2);
    for n in 6..10 {
        assert_eq!(Curve::from_u32(n).map(|c| c as u32), Some(n));
    }
    assert_eq!(Curve::from_u32(10), None);
}

#[test]
fn combines() {
    let three = [P95, P95, P95];
    let product = combine(Combine::Product, &mut three.clone());
    let geo = combine(Combine::GeometricMean, &mut three.clone());
    // plain product 0.857; compensation lifts it to 0.939; the mean keeps 0.95
    assert_eq!(product, 61528);
    assert!((P95 - 2..=P95).contains(&geo), "{geo}");
    assert_eq!(combine(Combine::Minimum, &mut [H, ONE, 100]), 100);
    assert_eq!(combine(Combine::Average, &mut [0, ONE]), H);
    assert_eq!(combine(Combine::Median, &mut [ONE, 0, H]), H);
    assert_eq!(combine(Combine::Median, &mut [0, 100, 200, ONE]), 150);
    // a 0 rules out in product, mean and minimum, not in average and median
    for how in [Combine::Product, Combine::GeometricMean, Combine::Minimum] {
        assert_eq!(combine(how, &mut [0, ONE, ONE]), 0, "{how:?}");
    }
    assert!(combine(Combine::Average, &mut [0, ONE, ONE]) > 0);
    assert_eq!(combine(Combine::Median, &mut [0, ONE, ONE]), ONE);
    assert_eq!(combine(Combine::GeometricMean, &mut []), ONE);
    // geometric mean of 0.25 and 1 is 0.5
    let g = geometric_mean(&[ONE / 4, ONE]);
    assert!((H - 2..=H + 1).contains(&g), "{g}");
}

#[test]
fn pull_softens_and_sharpens() {
    assert_eq!(pull(0, ONE), 0);
    assert_eq!(pull(0, H), ONE - H); // gentle: never below 0.5
    assert_eq!(pull(H, 2 * ONE), 0); // strict: 0 from 0.5 down
    assert_eq!(pull(0, 0), ONE); // off
    assert_eq!(pull(ONE, 5 * ONE), ONE);
}

#[test]
fn multiply_stays_a_true_factor() {
    // Average would let "no ammunition" through; Multiply does not.
    let mut d = Decider::new();
    d.set_combine(Combine::Average);
    let shoot = d.add_option(ONE);
    d.add_consideration(shoot, Consideration::new(0, Curve::Linear, 0, 10));
    d.add_weighted(
        shoot,
        Consideration::new(1, Curve::Step, 1, 0),
        ONE,
        Apply::Multiply,
    );
    let c = [Candidate::new(shoot, 0)];
    assert!(d.score(c[0], &[10, 1]).unwrap() == ONE);
    assert_eq!(d.score(c[0], &[10, 0]).unwrap(), 0);
    // an option can follow its own combine
    let other = d.add_option(ONE);
    d.add_consideration(other, Consideration::new(0, Curve::Linear, 0, 10));
    d.add_consideration(other, Consideration::new(1, Curve::Linear, 0, 10));
    assert_eq!(d.score(Candidate::new(other, 0), &[10, 0]).unwrap(), H);
    assert!(d.set_option_combine(other, Some(Combine::Minimum)));
    assert_eq!(d.score(Candidate::new(other, 0), &[10, 0]).unwrap(), 0);
    assert!(!d.set_option_combine(9, None));
}

#[test]
fn rank_and_inertia() {
    let mut d = Decider::new();
    let idle = d.add_option(ONE);
    let flee = d.add_option(ONE / 10);
    d.add_consideration(flee, Consideration::new(0, Curve::Step, 1, 0));
    assert!(d.set_rank(flee, 1));
    let cands = [Candidate::new(idle, 0), Candidate::new(flee, 0)];
    let mut s = Vec::new();
    // danger: flee wins although idle scores higher
    let c = d
        .decide(&cands, &[1, 1], 1, Pick::Best, 0, &mut s)
        .unwrap()
        .unwrap();
    assert_eq!(c.candidate, 1);
    // no danger: flee scores 0, idle is chosen
    let c = d
        .decide(&cands, &[0, 0], 1, Pick::Best, 0, &mut s)
        .unwrap()
        .unwrap();
    assert_eq!(c.candidate, 0);

    // inertia: two equal attacks, the current one stays
    let mut d = Decider::new();
    let attack = d.add_option(ONE);
    let cands = [Candidate::new(attack, 1), Candidate::new(attack, 2)];
    d.set_current(Some(cands[1]), ONE / 10);
    let c = d
        .decide(&cands, &[], 0, Pick::Best, 0, &mut s)
        .unwrap()
        .unwrap();
    assert_eq!(c.candidate, 1);
    assert_eq!(s[1], ONE + ONE / 10);
    d.set_current(None, 0);
    let c = d
        .decide(&cands, &[], 0, Pick::Best, 0, &mut s)
        .unwrap()
        .unwrap();
    assert_eq!(c.candidate, 0);
}
