use super::*;

#[test]
fn presets_match_spec() {
    let s = Leeway::STRICT;
    assert_eq!(s.leash.distance, Some(3));
    assert_eq!(s.leash.ticks, None);
    assert_eq!(s.leash.on_lost, OnLost::Return);
    assert!(!s.replan && !s.widen);

    let f = Leeway::FREE;
    assert_eq!(f.leash.distance, None);
    assert_eq!(f.leash.ticks, None);
    assert_eq!(f.leash.on_lost, OnLost::Keep);
    assert!(f.replan && f.widen);

    let x = Leeway::FLEXIBLE;
    assert_eq!(x.leash.distance, Some(12));
    assert_eq!(x.leash.ticks, None);
    assert_eq!(
        x.leash.on_lost,
        OnLost::Search {
            ticks: 60,
            radius: 6
        }
    );
    assert!(x.replan && !x.widen);
    assert_eq!(Leeway::default(), Leeway::FLEXIBLE);
}

#[test]
fn plan_builders() {
    let steps = [Step::pause(), Step::wait(3), Step::wander()];
    let l = Plan::looping(&steps);
    assert!(l.repeat);
    assert_eq!(l.steps().len(), 3);
    let o = Plan::once(&steps);
    assert!(!o.repeat);
    assert_eq!(o.steps(), &steps);
}

#[test]
fn fails_when_fills_slots_in_order() {
    let s = Step::attack(7)
        .fails_when(Fail::Unknown { subject: 7 })
        .fails_when(Fail::Custom(2));
    assert_eq!(s.fails[0], Some(Fail::Unknown { subject: 7 }));
    assert_eq!(s.fails[1], Some(Fail::Custom(2)));
    assert_eq!(s.fails[2], None);
    let p = Plan::once(&[Step::pause()])
        .fails_when(Fail::Custom(1))
        .fails_when(Fail::Flags { mask: 1, value: 1 });
    assert_eq!(p.fails()[0], Some(Fail::Custom(1)));
    assert_eq!(p.fails()[1], Some(Fail::Flags { mask: 1, value: 1 }));
    assert_eq!(p.fails()[2], None);
}

#[test]
fn config_defaults_and_motives() {
    let c = MissionConfig::default();
    assert_eq!((c.max_steps, c.max_playbook), (16, 8));
    assert_eq!((c.enemy_bits, c.defeated_bits), (1, 2));
    assert_eq!((c.lost_after, c.pause_ticks, c.safe_distance), (30, 50, 12));
    assert_eq!(c.extra_columns, 0);
    assert_eq!(c.pick, crate::Pick::Best);
    assert_eq!(c.hex_layout, None);
    assert_eq!(Motive::defeat_enemies().subject, None);
    assert_eq!(Motive::guard_subject(4).subject, Some(4));
    assert_eq!(Motive::reach([1, 2, 3]).place, Some([1, 2, 3]));
}

#[test]
fn errors_display() {
    assert!(!MissionError::EmptyPlan.to_string().is_empty());
    let e: &dyn std::error::Error = &MissionError::HexNeedsHexMetric;
    assert!(!e.to_string().is_empty());
}
