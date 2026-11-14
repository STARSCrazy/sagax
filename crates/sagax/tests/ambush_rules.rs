//! The ambush of `examples/ambush_rules.rs`: a game builds sneaking, hand
//! signs, a kill zone and a "seen" abort through the rules object alone.
//! The trace is the golden text the C example is diffed against.

#[allow(dead_code)]
#[path = "../examples/ambush_rules.rs"]
mod ambush_rules;

use ambush_rules::*;
use sagax::prelude::*;

/// Every line `cargo run -p sagax --example ambush_rules` prints.
const GOLDEN: &str = "\
scene A
t=  0 planned plan 1
t=  5 activated plan 1
t=  5 order member 1
t=  6 order member 2
t=  6 order member 3
t=  6 order member 4
t= 18 signal
t= 18 order member 1
t= 19 order member 2
t= 19 order member 3
t= 19 order member 4
t= 19 start member 1
t= 19 start member 2
t= 19 start member 3
t= 19 start member 4
scene B
t=  0 planned plan 1
t=  5 activated plan 1
t=  5 order member 1
t=  6 order member 2
t=  6 order member 3
t=  6 order member 4
t= 25 planned plan 2
t= 30 activated plan 2
t= 30 order member 1
t= 30 start member 1
t= 31 order member 2
t= 31 order member 3
t= 31 order member 4
t= 31 start member 2
t= 31 start member 3
t= 31 start member 4
";

#[test]
fn ambush_waits_hidden_then_strikes_together() {
    let scene = run_scene(false, END);
    let events = &scene.events;
    let walls = scene_walls();

    // Every first order sends a guard to a spot the intruder cannot see.
    let mut ordered = Vec::new();
    for &(t, e) in events {
        if let CrewEvent::Order { member, goal, .. } = e
            && !ordered.contains(&member)
        {
            ordered.push(member);
            let spot = goal.pos.unwrap();
            assert!(
                !sees(&walls, intruder_at(false, t), spot),
                "member {member} waits in sight at t={t}"
            );
        }
    }
    assert_eq!(ordered.len(), 4, "every guard got an order");

    // One signal, on the first tick the intruder stands in the kill zone.
    let signals: Vec<u32> = events
        .iter()
        .filter(|(_, e)| *e == CrewEvent::Signal)
        .map(|(t, _)| *t)
        .collect();
    let first_in_zone = (0..=END)
        .find(|&t| in_kill_zone(intruder_at(false, t)))
        .unwrap();
    assert_eq!(signals, vec![first_in_zone]);

    // Everyone starts the attack on one tick.
    let starts: Vec<u32> = events
        .iter()
        .filter(|(t, e)| matches!(e, CrewEvent::Start { .. }) && *t > signals[0])
        .map(|(t, _)| *t)
        .collect();
    assert_eq!(starts.len(), 4);
    assert!(starts.iter().all(|&t| t == starts[0]), "{starts:?}");

    let planned = events
        .iter()
        .filter(|(_, e)| matches!(e, CrewEvent::Planned(_)))
        .count();
    assert_eq!(planned, 1);
}

#[test]
fn ambush_aborts_when_a_guard_is_seen() {
    let scene = run_scene(true, END);
    assert!(
        !scene.events.iter().any(|(_, e)| *e == CrewEvent::Signal),
        "no signal"
    );
    let sent = scene.sent_at.expect("the seen report was sent");
    assert!(
        scene
            .events
            .iter()
            .any(|&(t, e)| t == sent + 1 && e == CrewEvent::Planned(2)),
        "{:?}",
        scene.events
    );
    // The replacement is the rush (tactic 1), not another ambush: look at
    // the pending plan on the tick it was made.
    let at_replan = run_scene(true, sent + 1);
    assert_eq!(at_replan.crew.squad().pending_plan().unwrap().tactic, 1);
}

#[test]
fn golden() {
    assert_eq!(trace_all().join("\n") + "\n", GOLDEN);
}
