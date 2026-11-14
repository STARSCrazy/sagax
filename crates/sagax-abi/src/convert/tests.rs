use super::*;
use crate::codes::*;
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
        plan_error(PlanError::TooManySlots { tactic: 1 }),
        (ERROR_PLAN, 1, u32::MAX)
    );
}
