use super::*;
use sagax_decide::group::Goal;

fn goal(plan: u32) -> Goal {
    Goal {
        kind: GoalKind::Move,
        pos: Some([1, 2, 0]),
        unchecked: false,
        then_pos: None,
        then_unchecked: false,
        target: None,
        start: Start::Now,
        slot: 0,
        plan,
        issued_at: 0,
        start_delay: 0,
    }
}

#[test]
fn codes_round_trip() {
    for kind in 0..=7 {
        let value = place(kind, 3, [4, 5, 6]);
        let Some(value) = value else {
            assert!(kind > PLACE_HERE, "kind {kind}");
            continue;
        };
        let (code, extra, offset) = place_code(value);
        assert_eq!(code, kind);
        // The same record again gives the same place.
        assert_eq!(place(code, extra, offset), Some(value));
    }
    assert_eq!(place(PLACE_NONE, 9, [1, 1, 1]), Some(None));
    // A custom place keeps its id in both directions (it is no lossy
    // "here" any more).
    assert_eq!(
        place(PLACE_CUSTOM, 41, [0; 3]),
        Some(Some(Place::Custom(41)))
    );
    assert_eq!(
        place_code(Some(Place::Custom(41))),
        (PLACE_CUSTOM, 41, [0; 3])
    );
    assert_eq!(place(PLACE_SIDE, 4, [0; 3]), None, "unknown side");

    for code in 0..=3 {
        assert_eq!(side(code).map(|s| s as u32), Some(code));
    }
    assert_eq!(side(4), None);

    for code in 0..=5 {
        let kind = goal_kind(code, 77).unwrap();
        let (back, custom) = goal_kind_code(kind);
        assert_eq!(back, code);
        assert_eq!(custom, if code == GOAL_CUSTOM { 77 } else { 0 });
    }
    assert_eq!(goal_kind(6, 0), None);

    for code in 0..=2 {
        let s = start(code, 9).unwrap();
        let (back, ticks) = start_code(s);
        assert_eq!(back, code);
        assert_eq!(ticks, if code == START_AFTER_TICKS { 9 } else { 0 });
    }
    assert_eq!(start(3, 0), None);

    for code in 0..=2 {
        assert_eq!(shape(code).map(|s| s as u32), Some(code));
        assert_eq!(member_state(code).map(|s| s as u32), Some(code));
    }
    assert_eq!(shape(3), None);
    assert_eq!(member_state(3), None);

    for code in 0..=3 {
        assert_eq!(metric(code).map(metric_code), Some(code));
    }
    assert_eq!(metric(4), None);
}

#[test]
fn set_combine_rule() {
    let mut d = Decider::new();
    d.add_option(1);
    d.add_option(1);
    assert!(set_combine(&mut d, CombineTarget::All, 2));
    assert!(format!("{d:?}").contains("combine: Minimum"));
    assert!(!set_combine(&mut d, CombineTarget::All, COMBINE_FOLLOW));
    assert!(!set_combine(&mut d, CombineTarget::All, 9));
    assert!(
        !set_combine(&mut d, CombineTarget::Option(9), 1),
        "no option 9"
    );
    assert!(set_combine(&mut d, CombineTarget::Option(1), 1));
    assert!(set_combine(
        &mut d,
        CombineTarget::Option(1),
        COMBINE_FOLLOW
    ));
    assert!(!set_combine(&mut d, CombineTarget::Option(1), 9));
    assert!(format!("{d:?}").contains("combine: None"));
}

#[test]
fn whole_i64_rejects() {
    assert_eq!(whole_i64(f64::NAN), None);
    assert_eq!(whole_i64(1.5), None);
    assert_eq!(whole_i64(9.3e18), None);
    assert_eq!(whole_i64(-9.3e18), None);
    assert_eq!(whole_i64(f64::INFINITY), None);
    assert_eq!(whole_i64(-3.0), Some(-3));
    assert_eq!(whole_i64(0.0), Some(0));
    assert_eq!(whole_i64(-9_223_372_036_854_775_808.0), Some(i64::MIN));
}

#[test]
fn flank_sides_only_2_or_3() {
    assert_eq!(flank_sides(1), None);
    assert_eq!(flank_sides(4), None);
    assert_eq!(flank_sides(2), Some(2));
    assert_eq!(flank_sides(3), Some(3));
}

#[test]
fn skill_codes_round_trip() {
    let table = SkillTable::default();
    let codes = skill_codes(&table);
    assert_eq!(skill_table(&codes), Some(table));
    assert_eq!(skill_codes(&skill_table(&codes).unwrap()), codes);
    let mut bad = codes;
    bad.pick[2] = 3;
    assert_eq!(skill_table(&bad), None);
    for rows in [0, 9, 256] {
        let mut bad = codes;
        bad.rows = rows;
        assert_eq!(skill_table(&bad), None);
    }
}

#[test]
fn crew_event_codes() {
    let events = [
        CrewEvent::Planned(7),
        CrewEvent::Activated(7),
        CrewEvent::Signal,
        CrewEvent::Order {
            member: 3,
            goal: goal(7),
            start_at: Some(12),
        },
        CrewEvent::Start { member: 3 },
        CrewEvent::Arrived { member: 3 },
        CrewEvent::Broken { member: 3 },
        CrewEvent::NoPlan,
    ];
    let kinds: Vec<u32> = events.iter().map(|e| crew_event(e).kind).collect();
    assert_eq!(kinds, [1, 2, 3, 4, 5, 6, 7, 8]);
    let order = crew_event(&events[3]);
    assert_eq!(
        (order.member, order.plan, order.start_at, order.goal),
        (3, 7, Some(12), Some(goal(7)))
    );
    let planned = crew_event(&events[0]);
    assert_eq!((planned.member, planned.plan, planned.goal), (0, 7, None));
    assert_eq!(crew_event(&events[4]).member, 3);
}

#[test]
fn hex_and_budgets() {
    for grid in 1..=4 {
        let axial = hex_to_axial(grid, [3, 2, 1]).unwrap();
        assert_eq!(hex_from_axial(grid, axial), Some([3, 2, 1]));
    }
    assert_eq!(hex_layout(GRID_SQUARE), None);
    assert_eq!(hex_to_axial(0, [0; 3]), None);
    assert_eq!(hex_from_axial(5, [0; 3]), None);
    assert!(matches!(budget(0), Some(Budget::Unlimited)));
    assert!(matches!(budget(5), Some(Budget::Cells(n)) if n.get() == 5));
    assert!(budget(-1).is_none());
    assert_eq!(budget_cells(0), Some(usize::MAX));
    assert_eq!(budget_cells(-1), None);
    assert!(output(2).is_some() && output(3).is_none());
    assert_eq!(u32_of(-1), None);
    assert_eq!(u32_of(4), Some(4));
    assert_eq!(to_i32(u64::MAX), i32::MAX);
}

#[test]
fn statuses_and_errors() {
    assert_eq!(nav_status(Status::Done), DONE);
    assert_eq!(
        squad_status(SquadStatus::Replan(5)),
        (SQUAD_REPLAN, Some(5))
    );
    assert_eq!(squad_status(SquadStatus::Keep), (SQUAD_KEEP, None));
    assert_eq!(
        plan_error(PlanError::InputOutOfRange {
            tactic: 2,
            input: 9
        }),
        (ERROR_PLAN, 2, 9)
    );
    assert_eq!(
        plan_error(PlanError::OptionsChanged),
        (ERROR_PLAN, u32::MAX, u32::MAX)
    );
    // NeedsRules has a code of its own, apart from OptionsChanged.
    assert_eq!(
        plan_error(PlanError::NeedsRules),
        (ERROR_NEEDS_RULES, u32::MAX, u32::MAX)
    );
    assert_ne!(ERROR_NEEDS_RULES, ERROR_PLAN);
    assert_eq!(
        plan_error(PlanError::TooManySlots { tactic: 1 }),
        (ERROR_PLAN, 1, u32::MAX)
    );
}

#[test]
fn mission_codes_round_trip() {
    for p in [
        Pick::Best,
        Pick::Weighted { top: 3 },
        Pick::Uniform { top: 0 },
    ] {
        let (mode, top) = pick_code(p);
        assert_eq!(Pick::from_u32(mode, top), Some(p));
    }
    for grid in GRID_SQUARE..=GRID_HEX_EVEN_Q {
        assert_eq!(grid_code(hex_layout(grid)), grid);
    }
    let fails = [
        Fail::Unknown { subject: 1 },
        Fail::Defeated { subject: 2 },
        Fail::Fact {
            subject: 3,
            kind: 4,
            mask: 5,
            value: 6,
        },
        Fail::Flags {
            mask: 1 << 50,
            value: 7,
        },
        Fail::Custom(8),
    ];
    for f in fails {
        assert_eq!(fail(&fail_code(f)), Some(f));
    }
    assert_eq!(
        fail(&FailCode {
            kind: 5,
            ..FailCode::default()
        }),
        None
    );
    for k in [
        MotiveKind::Guard,
        MotiveKind::Defeat,
        MotiveKind::Reach,
        MotiveKind::Custom(9),
    ] {
        let (kind, custom) = motive_kind_code(k);
        assert_eq!(motive_kind(kind, custom), Some(k));
    }
    assert_eq!(motive_kind(4, 0), None);
    for o in [
        OnLost::Return,
        OnLost::Search {
            ticks: 6,
            radius: 3,
        },
        OnLost::Keep,
    ] {
        let (kind, ticks, radius) = on_lost_code(o);
        assert_eq!(on_lost(kind, ticks, radius), Some(o));
    }
    assert_eq!(on_lost(3, 0, 0), None);
    assert_eq!(leeway_preset(LEEWAY_FREE), Some(Leeway::FREE));
    assert_eq!(leeway_preset(3), None);
}

#[test]
fn mission_steps_and_detours() {
    let here = Some(Where::Here);
    for kind in STEP_GO_TO..=STEP_SQUAD {
        let with = step_kind(kind, here, 1, 2);
        assert!(with.is_some(), "kind {kind}");
        let needs_place = matches!(
            kind,
            STEP_GO_TO | STEP_SEARCH | STEP_HOLD | STEP_CUSTOM | STEP_SQUAD
        );
        assert_eq!(
            step_kind(kind, None, 1, 2).is_none(),
            needs_place,
            "kind {kind}"
        );
    }
    assert_eq!(step_kind(STEP_SQUAD + 1, here, 0, 0), None);
    assert_eq!(
        step_kind(STEP_CALL, None, 3, 4),
        Some(StepKind::Call {
            channel: 3,
            kind: 4
        })
    );
    for kind in WHERE_FIXED..=WHERE_HERE {
        assert!(where_of(kind, [1, 2, 3], 4).is_some());
    }
    assert_eq!(where_of(WHERE_HERE + 1, [0; 3], 0), None);
    assert_eq!(
        detour(DETOUR_ENGAGE, 5, None, None, 0, 0),
        Some(Detour::Engage(5))
    );
    assert_eq!(detour(DETOUR_FLEE, 0, None, None, 0, 0), None);
    assert_eq!(
        detour(DETOUR_FLEE, 0, here, Some([1, 1, 0]), 0, 0),
        Some(Detour::Flee {
            from: Where::Here,
            to: Some([1, 1, 0])
        })
    );
    assert_eq!(detour(DETOUR_CUSTOM + 1, 0, here, None, 0, 0), None);
}

#[test]
fn mission_outputs() {
    let a = action_code(Action::Custom {
        kind: 4,
        pos: None,
        param: 9,
    });
    assert_eq!(
        (a.kind, a.pos, a.custom, a.param),
        (ACTION_CUSTOM, None, 4, 9)
    );
    let a = action_code(Action::Attack {
        subject: 3,
        pos: [1, 2, 0],
    });
    assert_eq!(
        (a.kind, a.pos, a.subject),
        (ACTION_ATTACK, Some([1, 2, 0]), 3)
    );
    let e = mission_event(Event::Broken(BreakReason::Fail(Fail::Custom(2))));
    assert_eq!(
        (e.kind, e.a, e.fail),
        (MISSION_EVENT_BROKEN, BREAK_FAIL, Some(Fail::Custom(2)))
    );
    let e = mission_event(Event::WantsSquad {
        tactic: 1,
        target: None,
        subject: Some(7),
    });
    assert_eq!(
        (e.kind, e.a, e.pos, e.subject),
        (MISSION_EVENT_WANTS_SQUAD, 1, None, Some(7))
    );
    let e = mission_event(Event::Call {
        channel: 2,
        kind: 3,
        pos: [4, 5, 0],
        motive: Motive::defeat(9),
    });
    assert_eq!(
        (e.kind, e.a, e.b, e.pos, e.motive),
        (
            MISSION_EVENT_CALL,
            2,
            3,
            Some([4, 5, 0]),
            Some(Motive::defeat(9))
        )
    );
    let e = mission_event(Event::DetourEnded(DetourEnd::Arrived));
    assert_eq!(
        (e.kind, e.a),
        (MISSION_EVENT_DETOUR_ENDED, DETOUR_END_ARRIVED)
    );
    assert_eq!(mission_event(Event::StepDone(3)).a, 3);
}
