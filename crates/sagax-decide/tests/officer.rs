//! Reference numbers of the officer recipe in `examples/officer.rs`.

#[allow(dead_code)]
#[path = "../examples/officer.rs"]
mod officer;

#[test]
fn officer_recipe_reference_numbers() {
    use officer::Event::*;
    use officer::*;
    let ev = run_scene(2100);
    let state = |t, officer, from, to| State {
        t,
        officer,
        from,
        to,
    };
    let want = vec![
        // t=1: officer 0 witnesses the crime: reports at once (reported was
        // 0) and starts the chase.
        RadioSent { t: 1, officer: 0 },
        state(1, 0, IDLE, CHASE),
        // Sight ticks of officer 0: (t+0)%4==0 -> 4, 8, ..., 40 seen. From
        // 44 on each sight tick adds 4: 44,48,...,76 are 9 misses = 36 >= 34
        // -> lost at 76 (8 misses = 32 is not enough). No stall: the
        // officer gains 8 per tick (> 0x80 in 85 ticks) or is closer than
        // 0x100.
        state(76, 0, CHASE, WALK),
        // Search centre = last seen place [4320, 4000] (actor at t=40).
        // Officer at t=76: x = 3000 + 16*75 = 4200, y = 4000. Step 0 of
        // agent 0 (heading 0) = [4320, 4256]; after one step (77) it is
        // 256 <= 0x180 away -> Look 51 (after a chase) -> 77+51 = 128.
        state(77, 0, WALK, LOOK),
        // t=103 = 1 + 102: call delivered. Officer 0 is busy; free are
        // 1, 2, 3. Distance (dx/16)^2+(dy/16)^2 to the call [4008, 4000]:
        // 1: 125^2, 2: 62^2, 3: 125^2 -> 2, then the tie 1 before 3.
        Helpers {
            t: 103,
            ids: [2, 1, 3],
        },
        state(103, 2, IDLE, WALK),
        state(103, 1, IDLE, WALK),
        state(103, 3, IDLE, WALK),
        state(128, 0, LOOK, WALK),
        // Step 1: [4320, 5280]; from y 4016 to 4896 (within 0x180) is
        // 880/16 = 55 ticks -> 128+55 = 183; look 51 -> 234.
        state(183, 0, WALK, LOOK),
        state(234, 0, LOOK, WALK),
        // Step 2: heading 0x20, sin = cos = 181, 9*256*181/256 = 1629 ->
        // [5949, 5629]; x from 4320 to 5565 needs ceil(1245/16) = 78 ->
        // 312; look 51 -> 363.
        state(312, 0, WALK, LOOK),
        state(363, 0, LOOK, WALK),
        // Step 3: heading -0x20 -> [4320-2172, 4000+2172] = [2148, 6172];
        // x from 5568 to 2532 needs ceil(3036/16) = 190 -> 553 (walk 190 <=
        // 340, search 425 <= 1020); look 51 -> 604, last step -> Idle.
        state(553, 0, WALK, LOOK),
        state(604, 0, LOOK, WALK),
        state(604, 0, WALK, IDLE),
        // Last refresh of the table at t=40 (helpers learning by radio do
        // not refresh it): forgotten at 40 + 2040 = 2080.
        Forgotten { t: 2080, id: ACTOR },
    ];
    let mine: Vec<_> = ev
        .iter()
        .copied()
        .filter(|e| !matches!(e, State { officer: 1..=3, t, .. } if *t != 103))
        .collect();
    assert_eq!(mine, want);
    // Only one call: the report timer (255) is due again at 256, but the
    // actor is out of sight by then.
    assert_eq!(
        ev.iter().filter(|e| matches!(e, RadioSent { .. })).count(),
        1
    );
    // Helper 2 (first picked): heading 2*0x62 = 0xc4, sin = -255, cos = 25,
    // so step 0 is [4320-255, 4000+25] = [4065, 4025] around the sender's
    // last seen place. From [4008, 3000] it needs dy <= 0x180: (1025-384)/16
    // -> 41 walking ticks, the first one in its own turn at 103 -> Look at
    // 143. It was sent, so it looks 170 -> Walk at 313.
    let helper2: Vec<_> = ev
        .iter()
        .copied()
        .filter(|e| matches!(e, State { officer: 2, .. }))
        .take(3)
        .collect();
    assert_eq!(
        helper2,
        vec![
            state(103, 2, IDLE, WALK),
            state(143, 2, WALK, LOOK),
            state(313, 2, LOOK, WALK),
        ]
    );
}
