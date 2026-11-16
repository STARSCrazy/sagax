//! The wall scene of `examples/walk_distance_scene.rs`: the plain crew
//! sends the guard behind the wall, the walking one the guard who gets
//! there. The trace is the golden text the C example is diffed against.

#[allow(dead_code)]
#[path = "../examples/walk_distance_scene.rs"]
mod walk_distance_scene;

use walk_distance_scene::*;

/// Every line `cargo run -p sagax --example walk_distance_scene` prints.
const GOLDEN: &str = "\
guard 2 at 7,2: line 5, walk 15
guard 3 at 13,8: line 6, walk 6
metric
t= 0 planned 1
t= 5 activated 1
t= 5 order 2 to 11,3
t= 5 start 2
t=17 member 2 there
walking
t= 0 planned 1
t= 5 activated 1
t= 5 order 3 to 11,3
t= 5 start 3
t= 9 member 3 there
searches 2
";

#[test]
fn the_line_sends_the_guard_behind_the_wall() {
    let plain = run(false);
    assert_eq!(plain.runner, Some(2));
    let walking = run(true);
    assert_eq!(walking.runner, Some(3));
    // The one who can get there gets there sooner.
    assert!(walking.there.unwrap() < plain.there.unwrap());
}

#[test]
fn one_search_per_destination() {
    // The target and the one wish point, over the whole run.
    let walking = run(true);
    assert_eq!(walking.lines.last().map(String::as_str), Some("searches 2"));
}

#[test]
fn golden_trace() {
    let mut text = String::new();
    for line in run_scene() {
        text.push_str(&line);
        text.push('\n');
    }
    assert_eq!(text, GOLDEN);
}
