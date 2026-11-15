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
        let at = where_of(kind, [1, 2, 3], 4).unwrap();
        let (k, pos, subject) = where_code(at);
        assert_eq!((k, where_of(k, pos, subject)), (kind, Some(at)));
    }
    let places = [Where::Fixed([1, 2, 3]), Where::Subject(4), Where::Here];
    for kind in STEP_GO_TO..=STEP_SQUAD {
        for at in places {
            let step = step_kind(kind, Some(at), 5, 6).unwrap();
            let (k, place, a, b) = step_kind_code(step);
            assert_eq!(k, kind);
            assert_eq!(step_kind(k, place, a, b), Some(step), "kind {kind}");
        }
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
    let e = mission_event(MissionEvent::Broken(BreakReason::Fail(Fail::Custom(2))));
    assert_eq!(
        (e.kind, e.a, e.fail),
        (MISSION_EVENT_BROKEN, BREAK_FAIL, Some(Fail::Custom(2)))
    );
    let e = mission_event(MissionEvent::WantsSquad {
        tactic: 1,
        target: None,
        subject: Some(7),
    });
    assert_eq!(
        (e.kind, e.a, e.pos, e.subject),
        (MISSION_EVENT_WANTS_SQUAD, 1, None, Some(7))
    );
    let e = mission_event(MissionEvent::Call {
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
    let e = mission_event(MissionEvent::DetourEnded(DetourEnd::Arrived));
    assert_eq!(
        (e.kind, e.a),
        (MISSION_EVENT_DETOUR_ENDED, DETOUR_END_ARRIVED)
    );
    assert_eq!(mission_event(MissionEvent::StepDone(3)).a, 3);
}

#[test]
fn mission_trace_codes() {
    let c = mission_trace(MissionTrace::Column {
        option: 2,
        column: 5,
        value: -7,
    });
    assert_eq!(
        c,
        MissionTraceCode {
            kind: MISSION_TRACE_COLUMN,
            option: Some(2),
            column: 5,
            value: -7,
            ..MissionTraceCode::default()
        }
    );
    let factor = DecideTrace::Factor {
        candidate: 0,
        consideration: 3,
        input: 9,
        value: 100,
    };
    let d = mission_trace(MissionTrace::Decide {
        option: 1,
        event: factor,
    });
    assert_eq!(
        d,
        MissionTraceCode {
            kind: MISSION_TRACE_DECIDE,
            option: Some(1),
            decide: decide_trace(factor),
            ..MissionTraceCode::default()
        }
    );
    let r = mission_trace(MissionTrace::Rescored {
        option: 8,
        built_in: 20,
        score: 30,
    });
    assert_eq!(
        (r.kind, r.option, r.built_in, r.score),
        (MISSION_TRACE_RESCORED, Some(8), 20, 30)
    );
    let p = mission_trace(MissionTrace::Picked {
        option: None,
        score: 0,
    });
    assert_eq!(
        p,
        MissionTraceCode {
            kind: MISSION_TRACE_PICKED,
            ..MissionTraceCode::default()
        }
    );
    let b = mission_trace(MissionTrace::Begin {
        index: 2,
        at: Some([4, 5, 0]),
    });
    assert_eq!(
        (b.kind, b.option, b.index, b.at),
        (MISSION_TRACE_BEGIN, None, 2, Some([4, 5, 0]))
    );
    let none = mission_trace(MissionTrace::Begin { index: 0, at: None });
    assert_eq!(none.at, None);
}

#[test]
fn decide_trace_codes() {
    let f = decide_trace(DecideTrace::Factor {
        candidate: 4,
        consideration: 1,
        input: -3,
        value: 77,
    });
    assert_eq!(
        f,
        DecideTraceCode {
            kind: DECIDE_TRACE_FACTOR,
            candidate: 4,
            consideration: 1,
            input: -3,
            value: 77,
            ..DecideTraceCode::default()
        }
    );
    let s = decide_trace(DecideTrace::Scored {
        candidate: 2,
        product: 10,
        score: 0,
        cooled: true,
    });
    assert_eq!(
        s,
        DecideTraceCode {
            kind: DECIDE_TRACE_SCORED,
            candidate: 2,
            product: 10,
            cooled: true,
            ..DecideTraceCode::default()
        }
    );
    let r = decide_trace(DecideTrace::Ranked { rank: 3 });
    assert_eq!(
        r,
        DecideTraceCode {
            kind: DECIDE_TRACE_RANKED,
            rank: 3,
            ..DecideTraceCode::default()
        }
    );
    let c = decide_trace(DecideTrace::Chosen {
        candidate: 1,
        score: 9,
    });
    assert_eq!((c.kind, c.candidate, c.score), (DECIDE_TRACE_CHOSEN, 1, 9));
}

#[test]
fn state_trace_codes() {
    let changed = state_trace(StateTrace::Changed(Change {
        from: 1,
        to: 2,
        reason: Reason::Transition(3),
    }));
    assert_eq!(
        changed,
        StateTraceCode {
            kind: MACHINE_TRACE_CHANGED,
            change: ChangeCode {
                from: 1,
                to: 2,
                reason: REASON_TRANSITION,
                transition: 3,
            },
            ..StateTraceCode::default()
        }
    );
    let timeout = change(Change {
        from: 0,
        to: 1,
        reason: Reason::Timeout,
    });
    assert_eq!((timeout.reason, timeout.transition), (REASON_TIMEOUT, -1));
    let waiting = state_trace(StateTrace::Waiting {
        transition: 2,
        elapsed: 3,
        after: 10,
    });
    assert_eq!(
        waiting,
        StateTraceCode {
            kind: MACHINE_TRACE_WAITING,
            transition: 2,
            elapsed: 3,
            after: 10,
            ..StateTraceCode::default()
        }
    );
}

#[test]
fn search_trace_codes() {
    let c = |x, y| Cell::new(x, y, 0);
    let started = search_trace(SearchTrace::Started {
        search: 2,
        start: c(1, 2),
        estimate: 30,
    });
    assert_eq!(
        started,
        SearchTraceCode {
            kind: SEARCH_TRACE_STARTED,
            search: 2,
            cell: c(1, 2),
            estimate: 30,
            ..SearchTraceCode::default()
        }
    );
    let improved = search_trace(SearchTrace::Improved {
        search: 2,
        node: c(2, 2),
        parent: c(1, 2),
        cost: Cost::MAX,
        first: true,
    });
    assert_eq!(
        (
            improved.kind,
            improved.parent,
            improved.cost,
            improved.first
        ),
        (SEARCH_TRACE_IMPROVED, c(1, 2), i32::MAX, true)
    );
    let expanded = search_trace(SearchTrace::Expanded {
        search: 1,
        node: c(3, 3),
        cost: 7,
        estimate: 9,
    });
    assert_eq!(
        (
            expanded.kind,
            expanded.cell,
            expanded.cost,
            expanded.estimate
        ),
        (SEARCH_TRACE_EXPANDED, c(3, 3), 7, 9)
    );
    let paused = search_trace(SearchTrace::Paused {
        search: 1,
        expanded: 12,
    });
    assert_eq!((paused.kind, paused.expanded), (SEARCH_TRACE_PAUSED, 12));
    for (status, found) in [
        (SearchStatus::Found, true),
        (SearchStatus::Exhausted, false),
    ] {
        let finished = search_trace(SearchTrace::Finished {
            search: 1,
            status,
            expanded: 40,
        });
        assert_eq!(
            (finished.kind, finished.expanded, finished.found),
            (SEARCH_TRACE_FINISHED, 40, found)
        );
    }
}

#[test]
fn nav_trace_codes() {
    let c = |x, y| Cell::new(x, y, 0);
    let inner = SearchTrace::Paused {
        search: 3,
        expanded: 5,
    };
    let search = nav_trace(NavTrace::Search(inner));
    assert_eq!(
        (search.kind, search.search),
        (NAV_TRACE_SEARCH, search_trace(inner))
    );
    let shortcut = nav_trace(NavTrace::Smooth(SmoothTrace::Shortcut {
        from: c(0, 0),
        to: c(4, 1),
        clear: true,
    }));
    assert_eq!(
        shortcut,
        NavTraceCode {
            kind: NAV_TRACE_SHORTCUT,
            from: c(0, 0),
            to: c(4, 1),
            clear: true,
            ..NavTraceCode::default()
        }
    );
    let placed = nav_trace(NavTrace::Formation(FormationTrace::Placed {
        member: 2,
        step: 6,
        cell: c(5, 5),
        slot: false,
    }));
    assert_eq!(
        (
            placed.kind,
            placed.member,
            placed.step,
            placed.cell,
            placed.slot
        ),
        (NAV_TRACE_PLACED, 2, 6, c(5, 5), false)
    );
    let unplaced = nav_trace(NavTrace::Formation(FormationTrace::Unplaced {
        member: 1,
        step: 0,
    }));
    assert_eq!(
        (unplaced.kind, unplaced.member, unplaced.step),
        (NAV_TRACE_UNPLACED, 1, 0)
    );
    let best = nav_trace(NavTrace::FleeBest {
        cell: c(9, 1),
        distance: Cost::MAX,
        cost: 80,
    });
    assert_eq!(
        (best.kind, best.cell, best.distance, best.cost),
        (NAV_TRACE_FLEE_BEST, c(9, 1), i32::MAX, 80)
    );
}

#[test]
fn memory_trace_codes() {
    let f = Fact {
        subject: 3,
        kind: 1,
        seen_at: 9,
        ..Fact::default()
    };
    let kinds = [
        (KnowledgeTrace::Added(f), KNOWLEDGE_TRACE_ADDED),
        (KnowledgeTrace::Updated(f), KNOWLEDGE_TRACE_UPDATED),
        (KnowledgeTrace::Stale(f), KNOWLEDGE_TRACE_STALE),
        (KnowledgeTrace::Evicted(f), KNOWLEDGE_TRACE_EVICTED),
        (KnowledgeTrace::Expired(f), KNOWLEDGE_TRACE_EXPIRED),
        (KnowledgeTrace::Removed(f), KNOWLEDGE_TRACE_REMOVED),
    ];
    for (event, kind) in kinds {
        assert_eq!(knowledge_trace(event), KnowledgeTraceCode { kind, fact: f });
    }

    let a = Area {
        kind: 2,
        radius: 4,
        since: 7,
        ..Area::default()
    };
    let areas = [
        (
            AreasTrace::Added { index: 3, area: a },
            AREAS_TRACE_ADDED,
            3,
        ),
        (
            AreasTrace::Renewed { index: 1, area: a },
            AREAS_TRACE_RENEWED,
            1,
        ),
        (AreasTrace::Evicted(a), AREAS_TRACE_EVICTED, 0),
        (AreasTrace::Expired(a), AREAS_TRACE_EXPIRED, 0),
    ];
    for (event, kind, index) in areas {
        assert_eq!(
            areas_trace(event),
            AreasTraceCode {
                kind,
                index,
                area: a
            }
        );
    }

    assert_eq!(
        timers_trace(TimersTrace::Started { id: 4, duration: 9 }),
        TimersTraceCode {
            kind: TIMERS_TRACE_STARTED,
            id: 4,
            duration: 9,
            ..TimersTraceCode::default()
        }
    );
    assert_eq!(
        timers_trace(TimersTrace::Renewed { id: 4, duration: 2 }).kind,
        TIMERS_TRACE_RENEWED
    );
    assert_eq!(
        timers_trace(TimersTrace::Replaced {
            id: 5,
            duration: 3,
            old: 4,
            old_left: 1
        }),
        TimersTraceCode {
            kind: TIMERS_TRACE_REPLACED,
            id: 5,
            duration: 3,
            old: 4,
            old_left: 1,
        }
    );

    let m = Message {
        kind: 1,
        subject: 2,
        sender: 8,
        ..Message::default()
    };
    let code = |kind, at, refused| MessageTraceCode {
        kind,
        msg: m,
        at,
        refused,
    };
    let messages = [
        (
            MessageTrace::Posted { msg: m, due_at: 12 },
            code(MESSAGE_TRACE_POSTED, 12, 0),
        ),
        (
            MessageTrace::Refused {
                msg: m,
                why: Refused::Full,
            },
            code(MESSAGE_TRACE_REFUSED, 0, REFUSED_FULL),
        ),
        (
            MessageTrace::Refused {
                msg: m,
                why: Refused::Cooldown,
            },
            code(MESSAGE_TRACE_REFUSED, 0, REFUSED_COOLDOWN),
        ),
        (
            MessageTrace::Queued {
                msg: m,
                arrive_at: 30,
            },
            code(MESSAGE_TRACE_QUEUED, 30, 0),
        ),
        (
            MessageTrace::Dropped { msg: m },
            code(MESSAGE_TRACE_DROPPED, 0, 0),
        ),
        (
            MessageTrace::Delivered { msg: m },
            code(MESSAGE_TRACE_DELIVERED, 0, 0),
        ),
    ];
    for (event, expected) in messages {
        assert_eq!(message_trace(event), expected);
    }
}

#[test]
fn group_trace_codes() {
    use sagax_decide::group::{CrewTrace, KeeperTrace, SpotPick, SpotTrace, SquadTrace};
    let f = Fact {
        subject: 3,
        kind: 1,
        ..Fact::default()
    };
    let factor = DecideTrace::Factor {
        candidate: 2,
        consideration: 1,
        input: 5,
        value: 7,
    };
    let base = |kind| SquadTraceCode {
        kind,
        ..SquadTraceCode::default()
    };
    let cases = [
        (
            SquadTrace::Target {
                fact: f,
                kept: true,
            },
            SquadTraceCode {
                fact: f,
                kept: true,
                ..base(SQUAD_TRACE_TARGET)
            },
        ),
        (
            SquadTrace::Decide {
                tactic: 1,
                target: Some(3),
                event: factor,
            },
            SquadTraceCode {
                tactic: 1,
                target: Some(3),
                decide: decide_trace(factor),
                ..base(SQUAD_TRACE_DECIDE)
            },
        ),
        (
            SquadTrace::Abort {
                tactic: 2,
                target: None,
                abort: 1,
                fired: true,
            },
            SquadTraceCode {
                tactic: 2,
                abort: 1,
                fired: true,
                ..base(SQUAD_TRACE_ABORT)
            },
        ),
        (
            SquadTrace::Picked {
                tactic: 0,
                target: Some(9),
                score: 40,
            },
            SquadTraceCode {
                target: Some(9),
                score: 40,
                ..base(SQUAD_TRACE_PICKED)
            },
        ),
        (SquadTrace::NoPick, base(SQUAD_TRACE_NO_PICK)),
        (
            SquadTrace::Thinking { plan: 4, until: 12 },
            SquadTraceCode {
                plan: 4,
                until: 12,
                ..base(SQUAD_TRACE_THINKING)
            },
        ),
        (
            SquadTrace::Assigned {
                slot: 1,
                member: 6,
                cost: i128::MAX,
            },
            SquadTraceCode {
                slot: 1,
                member: 6,
                cost: i64::MAX,
                ..base(SQUAD_TRACE_ASSIGNED)
            },
        ),
        (
            SquadTrace::Spot {
                member: 6,
                event: SpotTrace::Ranked {
                    spot: 2,
                    fits: true,
                    distance: 9,
                },
            },
            SquadTraceCode {
                member: 6,
                spot: Some(2),
                fits: true,
                distance: 9,
                ..base(SQUAD_TRACE_SPOT_RANKED)
            },
        ),
        (
            SquadTrace::Spot {
                member: 6,
                event: SpotTrace::Picked(SpotPick::Unchecked([1, 2, 3])),
            },
            SquadTraceCode {
                member: 6,
                pos: [1, 2, 3],
                ..base(SQUAD_TRACE_SPOT_PICKED)
            },
        ),
        (
            SquadTrace::Trigger { fired: true },
            SquadTraceCode {
                fired: true,
                ..base(SQUAD_TRACE_TRIGGER)
            },
        ),
        (
            SquadTrace::Delay {
                member: 2,
                delay: Some(5),
            },
            SquadTraceCode {
                member: 2,
                delay: Some(5),
                ..base(SQUAD_TRACE_DELAY)
            },
        ),
        (
            SquadTrace::Knowledge(KnowledgeTrace::Added(f)),
            SquadTraceCode {
                knowledge: knowledge_trace(KnowledgeTrace::Added(f)),
                ..base(SQUAD_TRACE_KNOWLEDGE)
            },
        ),
    ];
    for (event, code) in cases {
        assert_eq!(squad_trace(event), code, "{event:?}");
    }
    let msg = Message {
        kind: 4,
        sender: 7,
        ..Message::default()
    };
    assert_eq!(squad_trace(SquadTrace::Report { msg }).msg, msg);
    assert_eq!(
        squad_trace(SquadTrace::Spot {
            member: 1,
            event: SpotTrace::Picked(SpotPick::Spot(4)),
        })
        .spot,
        Some(4)
    );
    assert_eq!(
        squad_trace(SquadTrace::Areas(AreasTrace::Expired(Area::default()))).kind,
        SQUAD_TRACE_AREAS
    );
    let queued = MessageTrace::Queued { msg, arrive_at: 9 };
    assert_eq!(
        squad_trace(SquadTrace::Inbox(queued)),
        SquadTraceCode {
            inbox: message_trace(queued),
            ..SquadTraceCode {
                kind: SQUAD_TRACE_INBOX,
                ..SquadTraceCode::default()
            }
        }
    );

    let checked = KeeperTrace::Checked {
        waited: 3,
        limit: 5,
        broken: false,
    };
    assert_eq!(
        keeper_trace(checked),
        KeeperTraceCode {
            kind: KEEPER_TRACE_CHECKED,
            waited: 3,
            limit: 5,
            broken: false,
        }
    );
    assert_eq!(
        crew_trace(CrewTrace::Keeper {
            member: 2,
            event: checked
        }),
        CrewTraceCode {
            kind: CREW_TRACE_KEEPER,
            member: 2,
            keeper: keeper_trace(checked),
            ..CrewTraceCode::default()
        }
    );
    assert_eq!(
        crew_trace(CrewTrace::Squad(SquadTrace::NoPick)).squad,
        base(SQUAD_TRACE_NO_PICK)
    );
    assert_eq!(
        crew_trace(CrewTrace::Sent {
            msg,
            delay: None,
            queued: false
        }),
        CrewTraceCode {
            kind: CREW_TRACE_SENT,
            member: 7,
            msg,
            ..CrewTraceCode::default()
        }
    );
}
