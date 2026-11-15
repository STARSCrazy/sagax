//! The stealth guard example in `examples/stealth_guard.rs`: hearing gives
//! a heard fact, the light decides how soon the sneaker is noticed, and
//! the daylight scene's rounds are pinned.

#[allow(dead_code)]
#[path = "../examples/stealth_guard.rs"]
mod stealth_guard;

use sagax::decide::memory::SOURCE_HEARD;
use stealth_guard::Event::*;
use stealth_guard::*;

/// The round guard `guard` first noticed someone, if ever.
fn noticed(events: &[Event], guard: u32) -> Option<u32> {
    events.iter().find_map(|e| match *e {
        Noticed { t, guard: g } if g == guard => Some(t),
        _ => None,
    })
}

#[test]
fn the_noise_gives_a_heard_fact() {
    // Noise range 5 from (5, 0): guard 0 at (0, 0) is 5 away and hears it,
    // guards 1 at (4, -6) and 2 at (-2, 0) are 7 away.
    let scene = run_scene(NOISE_ROUND, DAYLIGHT);
    let heard: Vec<u32> = scene
        .events
        .iter()
        .filter_map(|e| match *e {
            Heard { t, guard } => Some((t, guard)),
            _ => None,
        })
        .map(|(t, guard)| {
            assert_eq!(t, NOISE_ROUND);
            guard
        })
        .collect();
    assert_eq!(heard, vec![0]);
    let fact = scene.guards[0].known.get(SNEAKER, NOISE).copied().unwrap();
    assert_eq!(fact.source, SOURCE_HEARD);
    assert_eq!(fact.pos, sneaker_at(NOISE_ROUND).0);
    assert_eq!(fact.seen_at, NOISE_ROUND);
    for g in &scene.guards[1..] {
        assert!(g.known.get(SNEAKER, NOISE).is_none());
    }
}

#[test]
fn the_sneaker_is_noticed_later_in_the_dark() {
    let day = run_scene(30, DAYLIGHT).events;
    let night = run_scene(30, NIGHT).events;
    let (day_at, night_at) = (noticed(&day, 0).unwrap(), noticed(&night, 0).unwrap());
    assert!(day_at < night_at, "day {day_at}, night {night_at}");
    // In daylight guard 0 also recognises the sneaker and shouts; at night
    // it never gets that far before the sneaker is gone.
    assert!(day.iter().any(|e| matches!(e, Saw { guard: 0, .. })));
    assert!(!night.iter().any(|e| matches!(e, Saw { .. } | Shout { .. })));
}

#[test]
fn daylight_rounds() {
    let state = |t, guard, from, to| State { t, guard, from, to };
    let level = |t, guard, from, to| Level { t, guard, from, to };
    let want = vec![
        // The noise: guard 0 hears it (+70: searching) and goes to look.
        Heard { t: 2, guard: 0 },
        level(2, 0, UNAWARE, SEARCHING),
        state(2, 0, IDLE, INVESTIGATE),
        // Walking toward (5, 0) it faces the sneaker and notices it after
        // a few checks in full light (+40: alert); it chases.
        Noticed { t: 6, guard: 0 },
        level(6, 0, SEARCHING, ALERT),
        state(6, 0, INVESTIGATE, CHASE),
        // Recognised as the sneaker sprints off: a shout, 8 hexes to the
        // others, one round per hex.
        Saw { t: 12, guard: 0 },
        Shout { t: 12, guard: 0 },
        // The sneaker is in the cellar: lost after 2 rounds, search.
        state(14, 0, CHASE, SEARCH),
        Searching {
            t: 14,
            guard: 0,
            step: 0,
            at: [9, 2, 0],
        },
        Searching {
            t: 17,
            guard: 0,
            step: 1,
            at: [9, 3, 0],
        },
        level(18, 0, ALERT, SEARCHING),
        Searching {
            t: 18,
            guard: 0,
            step: 2,
            at: [11, 2, 0],
        },
        Searching {
            t: 20,
            guard: 0,
            step: 3,
            at: [7, 2, 0],
        },
        Told { t: 20, guard: 1 },
        level(20, 1, UNAWARE, SEARCHING),
        state(20, 1, IDLE, INVESTIGATE),
        Told { t: 20, guard: 2 },
        level(20, 2, UNAWARE, SEARCHING),
        state(20, 2, IDLE, INVESTIGATE),
        // Everyone calms down again.
        level(21, 0, SEARCHING, SUSPICIOUS),
        level(24, 0, SUSPICIOUS, UNAWARE),
        state(24, 0, SEARCH, IDLE),
        level(25, 1, SEARCHING, SUSPICIOUS),
        level(25, 2, SEARCHING, SUSPICIOUS),
        level(28, 1, SUSPICIOUS, UNAWARE),
        state(28, 1, INVESTIGATE, IDLE),
        level(28, 2, SUSPICIOUS, UNAWARE),
        state(28, 2, INVESTIGATE, IDLE),
    ];
    assert_eq!(run_scene(30, DAYLIGHT).events, want);
}
