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
t=  6 order member 3
t=  6 order member 2
t= 11 arrived member 1
t= 12 arrived member 2
t= 16 arrived member 3
t= 18 signal
t= 18 order member 1
t= 19 order member 3
t= 19 order member 2
t= 19 start member 1
t= 19 start member 2
t= 19 start member 3
scene B
t=  0 planned plan 1
t=  5 activated plan 1
t=  5 order member 1
t=  6 order member 3
t=  6 order member 2
t=  6 arrived member 3
t=  7 arrived member 1
t= 12 arrived member 2
t= 25 planned plan 2
t= 30 activated plan 2
t= 30 order member 1
t= 30 start member 1
t= 31 order member 3
t= 31 order member 2
t= 31 start member 2
t= 31 start member 3
";

#[test]
fn ambush_waits_hidden_then_strikes_together() {
    let scene = run_scene(false, END);
    let events = &scene.events;
    let walls = scene_walls();

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
    let signal = signals[0];

    // The first order of each guard that got one: its spot is out of the
    // intruder's sight when it is given, and the guard really stands on it
    // from its arrival until the signal, never seen by the intruder.
    let mut ordered = Vec::new();
    for &(t, e) in events {
        let CrewEvent::Order { member, goal, .. } = e else {
            continue;
        };
        if ordered.contains(&member) {
            continue;
        }
        ordered.push(member);
        let spot = goal.pos.unwrap();
        assert!(
            !sees(&walls, intruder_at(false, t), spot),
            "member {member}"
        );
        let on_spot: Vec<u32> = (0..signal)
            .filter(|&k| scene.stood[k as usize][member as usize - 1] == spot)
            .collect();
        assert_eq!(
            on_spot.last(),
            Some(&(signal - 1)),
            "member {member} is on its spot until the signal"
        );
        for &k in &on_spot {
            let guard = scene.stood[k as usize][member as usize - 1];
            assert!(
                !sees(&walls, intruder_at(false, k), guard),
                "member {member} seen on its spot at t={k}"
            );
        }
    }
    ordered.sort_unstable();
    assert_eq!(ordered, vec![1, 2, 3], "guard 4 got no order");

    // Hand signs travel only in sight: guard 4 stands behind the short wall
    // and never gets an order, a start or a signal; the others do.
    let leader = guard_start(1);
    assert!(!sees(&walls, leader, guard_start(4)));
    for id in 2..=3 {
        assert!(sees(&walls, leader, guard_start(id)));
    }
    assert!(
        !events.iter().any(|(_, e)| matches!(
            e,
            CrewEvent::Order { member: 4, .. } | CrewEvent::Start { member: 4 }
        )),
        "{events:?}"
    );
    // The signs of the others take one tick (the leader's own order none).
    let activated = events
        .iter()
        .find_map(|&(t, e)| matches!(e, CrewEvent::Activated(_)).then_some(t))
        .unwrap();
    for (id, delay) in [(1, 0), (2, 1), (3, 1)] {
        let at = events
            .iter()
            .find_map(|&(t, e)| {
                matches!(e, CrewEvent::Order { member, .. } if member == id).then_some(t)
            })
            .unwrap();
        assert_eq!(at, activated + delay, "order of member {id}");
    }

    // Everyone who got the signal starts the attack on one tick.
    let starts: Vec<u32> = events
        .iter()
        .filter(|(t, e)| matches!(e, CrewEvent::Start { .. }) && *t > signal)
        .map(|(t, _)| *t)
        .collect();
    assert_eq!(starts.len(), 3);
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
