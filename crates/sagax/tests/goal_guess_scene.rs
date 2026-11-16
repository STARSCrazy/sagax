//! The tower guard of `examples/goal_guess_scene.rs`: it guesses the
//! runner's exit from the steps it saw, never from more. The trace is the
//! golden text the C example is diffed against.

#[allow(dead_code)]
#[path = "../examples/goal_guess_scene.rs"]
mod goal_guess_scene;

use goal_guess_scene::*;

/// Every line `cargo run -p sagax --example goal_guess_scene` prints.
const GOLDEN: &str = "\
direct forget 13
t= 0 start 5,7 chance 64 64 64 64 best 0 hunters 1 1 1 0
t= 1 step 6,7 chance 10 118 10 118 best 1 hunters 0 2 0 1
t= 2 step 7,8 chance 3 8 4 241 best 3 hunters 0 0 0 3
t= 3 step 8,9 chance 3 4 3 246 best 3 hunters 0 0 0 3
t= 4 step 9,10 chance 3 7 3 243 best 3 hunters 0 0 0 3
t= 5 step 10,10 chance 3 10 3 240 best 3 hunters 0 0 0 3
t= 6 step 11,10 chance 3 13 3 237 best 3 hunters 0 0 0 3
t= 7 step 12,10 chance 4 8 3 241 best 3 hunters 0 0 0 3
t= 8 unseen
t= 9 unseen
t=10 unseen
t=11 jump 16,10 chance 4 8 3 241 best 3 hunters 0 0 0 3
t=12 step 17,11 chance 3 4 3 246 best 3 hunters 0 0 0 3
t=13 step 18,12 chance 3 4 3 246 best 3 hunters 0 0 0 3
t=14 step 19,13 chance 3 4 3 246 best 3 hunters 0 0 0 3
t=15 step 20,14 chance 3 4 3 246 best 3 hunters 0 0 0 3
arrived at exit 3, recognised at t=2
feint forget 0
t= 0 start 5,7 chance 64 64 64 64 best 0 hunters 1 1 1 0
t= 1 step 6,7 chance 7 121 7 121 best 1 hunters 0 2 0 1
t= 2 step 7,6 chance 1 251 0 4 best 1 hunters 0 3 0 0
t= 3 step 8,5 chance 0 256 0 0 best 1 hunters 0 3 0 0
t= 4 step 9,4 chance 0 256 0 0 best 1 hunters 0 3 0 0
t= 5 step 10,4 chance 0 256 0 0 best 1 hunters 0 3 0 0
t= 6 step 9,4 chance 0 255 0 1 best 1 hunters 0 3 0 0
t= 7 step 9,5 chance 0 252 4 0 best 1 hunters 0 3 0 0
t= 8 step 8,6 chance 1 99 155 1 best 2 hunters 0 1 2 0
t= 9 step 7,7 chance 0 1 255 0 best 2 hunters 0 0 3 0
t=10 step 6,8 chance 0 0 256 0 best 2 hunters 0 0 3 0
t=11 step 5,9 chance 0 0 256 0 best 2 hunters 0 0 3 0
t=12 step 4,10 chance 0 0 256 0 best 2 hunters 0 0 3 0
t=13 step 3,11 chance 0 0 256 0 best 2 hunters 0 0 3 0
t=14 step 2,12 chance 0 0 256 0 best 2 hunters 0 0 3 0
t=15 step 1,13 chance 0 0 256 0 best 2 hunters 0 0 3 0
t=16 step 0,14 chance 0 0 256 0 best 2 hunters 0 0 3 0
arrived at exit 2, recognised at t=8
feint forget 13
t= 0 start 5,7 chance 64 64 64 64 best 0 hunters 1 1 1 0
t= 1 step 6,7 chance 10 118 10 118 best 1 hunters 0 2 0 1
t= 2 step 7,6 chance 4 241 3 8 best 1 hunters 0 3 0 0
t= 3 step 8,5 chance 3 246 3 4 best 1 hunters 0 3 0 0
t= 4 step 9,4 chance 3 243 3 7 best 1 hunters 0 3 0 0
t= 5 step 10,4 chance 3 240 3 10 best 1 hunters 0 3 0 0
t= 6 step 9,4 chance 75 99 75 7 best 1 hunters 1 1 1 0
t= 7 step 9,5 chance 15 20 216 5 best 2 hunters 0 0 3 0
t= 8 step 8,6 chance 6 3 243 4 best 2 hunters 0 0 3 0
t= 9 step 7,7 chance 4 3 245 4 best 2 hunters 0 0 3 0
t=10 step 6,8 chance 3 3 246 4 best 2 hunters 0 0 3 0
t=11 step 5,9 chance 4 3 246 3 best 2 hunters 0 0 3 0
t=12 step 4,10 chance 4 3 246 3 best 2 hunters 0 0 3 0
t=13 step 3,11 chance 4 3 246 3 best 2 hunters 0 0 3 0
t=14 step 2,12 chance 4 3 246 3 best 2 hunters 0 0 3 0
t=15 step 1,13 chance 4 3 246 3 best 2 hunters 0 0 3 0
t=16 step 0,14 chance 4 3 246 3 best 2 hunters 0 0 3 0
arrived at exit 2, recognised at t=7
";

#[test]
fn the_guard_learns_only_what_it_saw() {
    for run in run_scene() {
        for &(t, e) in &run.events {
            match e {
                // No cheating: a learned step joins two seen places.
                Event::Learned { from, to } => {
                    assert!(!roofed(from.0, from.1) && !roofed(to.0, to.1), "t={t}");
                    assert!((from.0 - to.0).abs() <= 1 && (from.1 - to.1).abs() <= 1);
                }
                Event::Jump | Event::Unseen => {}
            }
        }
        // Every printed chance line sums to 256.
        for line in run.lines.iter().filter(|l| l.contains(" chance ")) {
            let words: Vec<&str> = line.split_whitespace().collect();
            let at = words.iter().position(|&w| w == "chance").unwrap();
            let sum: u32 = words[at + 1..at + 5]
                .iter()
                .map(|w| w.parse::<u32>().unwrap())
                .sum();
            assert_eq!(sum, 256, "{line}");
        }
    }
}

#[test]
fn direct_runner_is_recognised_and_the_roof_breaks_the_chain() {
    let runs = run_scene();
    let direct = &runs[0];
    let jumps = direct
        .events
        .iter()
        .filter(|(_, e)| *e == Event::Jump)
        .count();
    let unseen = direct
        .events
        .iter()
        .filter(|(_, e)| *e == Event::Unseen)
        .count();
    assert_eq!((jumps, unseen), (1, 3));
    let half = direct.best.len() / 2;
    assert!(direct.best[half..].iter().all(|&(_, b)| b == direct.exit));
    assert!(direct.recognised <= 2);
}

#[test]
fn forgetting_sees_through_the_feint_sooner() {
    let runs = run_scene();
    let (slow, fast) = (&runs[1], &runs[2]);
    assert_eq!((slow.exit, fast.exit), (2, 2));
    // Both fall for the feint first.
    assert!(slow.best.iter().any(|&(t, b)| t <= FEINT && b == 1));
    assert!(fast.best.iter().any(|&(t, b)| t <= FEINT && b == 1));
    assert!(
        fast.recognised < slow.recognised,
        "{} vs {}",
        fast.recognised,
        slow.recognised
    );
}

#[test]
fn golden_trace() {
    let mut text = String::new();
    for run in run_scene() {
        for line in run.lines {
            text.push_str(&line);
            text.push('\n');
        }
    }
    assert_eq!(text, GOLDEN);
}
