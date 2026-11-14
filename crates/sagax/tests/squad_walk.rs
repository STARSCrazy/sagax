//! The squad walk of `examples/squad_walk.rs`: the attack starts together,
//! nobody walks through the wall, and the trace is the golden text the C
//! and JavaScript examples are diffed against.

#[allow(dead_code)]
#[path = "../examples/squad_walk.rs"]
mod squad_walk;

use sagax::decide::group::GoalKind;
use sagax::prelude::*;
use squad_walk::*;

/// Every line `cargo run -p sagax --example squad_walk` prints.
const GOLDEN: &str = "\
t=  0 planned plan 1
t=  5 activated plan 1
t=  5 order member 1
t=  5 start member 1
t=  9 order member 2
t=  9 order member 3
t=  9 order member 4
t=  9 start member 2
t=  9 start member 3
t=  9 start member 4
t= 33 arrived member 1
t= 41 arrived member 4
t= 45 arrived member 2
t= 45 arrived member 3
t= 51 signal
t= 51 order member 1
t= 55 order member 4
t= 57 order member 2
t= 57 order member 3
t= 57 start member 1
t= 57 start member 2
t= 57 start member 3
t= 57 start member 4
";

#[test]
fn squad_walk_attacks_together() {
    let scene = run_scene(END);
    let events = &scene.events;

    let signals: Vec<usize> = (0..events.len())
        .filter(|&i| events[i].1 == CrewEvent::Signal)
        .collect();
    assert_eq!(signals.len(), 1, "exactly one signal");

    // The kind of each member's latest order, to tell attack starts apart.
    let mut kind = [None; 5];
    let mut attack_ticks = Vec::new();
    for (i, (t, e)) in events.iter().enumerate() {
        match e {
            CrewEvent::Order { member, goal, .. } => kind[*member as usize] = Some(goal.kind),
            CrewEvent::Start { member }
                if i > signals[0] && kind[*member as usize] == Some(GoalKind::Attack) =>
            {
                attack_ticks.push(*t);
            }
            _ => {}
        }
    }
    assert!(attack_ticks.len() >= 3, "attack starts: {attack_ticks:?}");
    assert!(
        attack_ticks.iter().all(|&t| t == attack_ticks[0]),
        "attack starts share one tick: {attack_ticks:?}"
    );

    let map = scene_map();
    for p in &scene.stood {
        assert!(map.open(p[0], p[1]), "a member stood on {p:?}");
    }
    assert!(scene.stood.len() > 20, "the members walked");

    assert_eq!(trace(events).join("\n") + "\n", GOLDEN);
}
