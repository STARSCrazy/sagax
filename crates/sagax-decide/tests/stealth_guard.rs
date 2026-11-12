//! Exact round numbers of the stealth guard example in
//! `examples/stealth_guard.rs`, derived by hand.
//!
//! Notation: cells are axial `(q, r)`; `d(a, b)` is the hex distance
//! `(|dq| + |dr| + |dq + dr|) / 2`. A step goes to the first neighbour (in
//! the order `+q, +q-r, -r, -q, -q+r, +r`) with the least
//! `10 * d(n, goal) + cost(n)`; the torch at (2, 0) makes that one cell cost
//! 20. Each round: the player moves, noises are heard, then every guard in
//! index order takes messages, looks, updates its alertness, changes state
//! and acts (walks).
//!
//! Alertness: thresholds 20 / 50 / 80 (suspicious / searching / alert),
//! hysteresis 5, hold 3 rounds, then 10 lost per round. After a stimulus at
//! round s that leaves the value at v, the value at round t >= s + 3 is
//! `v - 10 * (t - s - 3)`. Falling: alert -> searching below 75, searching
//! -> suspicious below 45, suspicious -> unaware below 15.

#[allow(dead_code)]
#[path = "../examples/stealth_guard.rs"]
mod stealth_guard;

#[test]
fn stealth_guard_round_numbers() {
    use stealth_guard::Event::*;
    use stealth_guard::*;
    let state = |t, guard, from, to| State { t, guard, from, to };
    let level = |t, guard, from, to| Level { t, guard, from, to };
    let want = vec![
        // Round 2: the player (standing at (5, 0)) knocks something over.
        // Noise range 5 from (5, 0): guard 0 at (0, 0) is d = 5 away (hears),
        // guard 1 at (4, -6) is (1 + 6 + 7) / 2 = 7 and guard 2 at (-2, 0)
        // is 7 away.
        // Only guard 0 hears: 30 -> suspicious, so it goes to look.
        Heard { t: 2, guard: 0 },
        level(2, 0, UNAWARE, SUSPICIOUS),
        state(2, 0, IDLE, INVESTIGATE),
        // Guard 0 walks to (5, 0). Round 2: (1, 0) (d 4, score 40).
        // Round 3 from (1, 0): (2, 0) would be d 3 + torch 20 = 50, but
        // (2, -1) is d 4 = 40 and comes first -> (2, -1). The light costs a
        // round. Round 4: (3, -1) (d 3). Round 5: (4, -1) (d 2).
        // Round 6: it looks from (4, -1): d to (5, 0) = (1 + 1 + 2) / 2 = 2
        // <= sight 2 -> sees. +100 -> 100 (capped) -> alert. It shouts from
        // (4, -1): guard 1 is d = 5, guard 2 is (6 + 1 + 5) / 2 = 6, both
        // within 6 (inclusive); they hear it at 6 + 5 = 11 and 6 + 6 = 12.
        Saw { t: 6, guard: 0 },
        Shout { t: 6, guard: 0 },
        level(6, 0, SUSPICIOUS, ALERT),
        state(6, 0, INVESTIGATE, CHASE),
        // Round 6 it steps to (5, -1). Round 7: the player sprints to
        // (7, 0), d = (2 + 1 + 3) / 2 = 3, unseen; the fact is 1 round old.
        // It steps onto the last seen place (5, 0). Round 8: the player is
        // at (9, 0), unseen; the fact is 8 - 6 = 2 >= 2 rounds old -> lost.
        // It searches around (5, 0). Agent 0 has heading 0: step 0 at
        // distance 2 -> (5, 0 + 2) = (5, 2).
        state(8, 0, CHASE, SEARCH),
        Searching {
            t: 8,
            guard: 0,
            step: 0,
            at: [5, 2, 0],
        },
        // Round 8: (5, 1); 9: (5, 2); 10: arrived -> step 1, heading 0,
        // distance 3 -> (5, 3), reached in the same round.
        Searching {
            t: 10,
            guard: 0,
            step: 1,
            at: [5, 3, 0],
        },
        // Round 11: arrived -> step 2, heading 0x20: sin = cos = 46341 / 65536,
        // 4 * 46341 / 65536 = 2 (truncated) -> (7, 2).
        Searching {
            t: 11,
            guard: 0,
            step: 2,
            at: [7, 2, 0],
        },
        // Guard 1 hears the shout in round 11: told the place (5, 0), +60 ->
        // 60 -> searching; it goes to look.
        Told { t: 11, guard: 1 },
        level(11, 1, UNAWARE, SEARCHING),
        state(11, 1, IDLE, INVESTIGATE),
        // Guard 0: stimulus at 6, value 100: 100 - 10 * (12 - 9) = 70 < 75
        // -> searching at round 12.
        level(12, 0, ALERT, SEARCHING),
        // Guard 2 hears the shout in round 12.
        Told { t: 12, guard: 2 },
        level(12, 2, UNAWARE, SEARCHING),
        state(12, 2, IDLE, INVESTIGATE),
        // Guard 0 round 11: (6, 3); 12: (7, 2); 13: arrived -> step 3,
        // heading -0x20: sin = -46341, cos = 46341 -> (5 - 2, 0 + 2) = (3, 2).
        // It walks (6, 2), (5, 2), (4, 2), (3, 2) by round 16 and stays: no
        // step after 3.
        Searching {
            t: 13,
            guard: 0,
            step: 3,
            at: [3, 2, 0],
        },
        // Guard 0: 100 - 10 * (15 - 9) = 40 < 45 -> suspicious at 15.
        level(15, 0, SEARCHING, SUSPICIOUS),
        // Guard 1 (60 at 11): 60 - 10 * (16 - 14) = 40 < 45 -> 16.
        level(16, 1, SEARCHING, SUSPICIOUS),
        // Guard 2 (60 at 12): 60 - 10 * (17 - 15) = 40 < 45 -> 17.
        level(17, 2, SEARCHING, SUSPICIOUS),
        // Guard 0: 100 - 10 * (18 - 9) = 10 < 15 -> unaware at 18: calm,
        // back to idle (and it forgets the player).
        level(18, 0, SUSPICIOUS, UNAWARE),
        state(18, 0, SEARCH, IDLE),
        // Guard 1 walks from (4, -6) (d 7) to (5, 0): (5, -6) in round 11,
        // then straight along q = 5: (5, -5) ... (5, 0) in round 17. Round
        // 18: arrived (value 20, still suspicious) -> search. Agent 1 has
        // heading 0x62: sin = 44011, cos = sin(0xa2) = -48559 (of 65536);
        // distance 2 -> (5 + 1, 0 - 1) = (6, -1).
        state(18, 1, INVESTIGATE, SEARCH),
        Searching {
            t: 18,
            guard: 1,
            step: 0,
            at: [6, -1, 0],
        },
        // Guard 1: 60 - 10 * (19 - 14) = 10 < 15 -> unaware at 19, idle.
        level(19, 1, SUSPICIOUS, UNAWARE),
        state(19, 1, SEARCH, IDLE),
        // Guard 2 walks from (-2, 0): (-1, 0) at 12, (0, 0) at 13, (1, 0) at
        // 14, the torch again -> (2, -1) at 15, (3, -1) at 16, (4, -1) at 17,
        // (5, -1) at 18, (5, 0) at 19. Round 20: 60 - 10 * (20 - 15) = 10 <
        // 15 -> unaware; calm is checked before "arrived", so it gives up.
        level(20, 2, SUSPICIOUS, UNAWARE),
        state(20, 2, INVESTIGATE, IDLE),
    ];
    assert_eq!(run_scene(30), want);
}
