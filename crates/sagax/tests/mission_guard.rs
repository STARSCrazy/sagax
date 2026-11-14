//! The scene of `examples/mission_guard.rs`: two guards on one patrol, an
//! attacker, a theft told to one of them. The trace is the golden text the
//! C example is diffed against.

#[allow(dead_code)]
#[path = "../examples/mission_guard.rs"]
mod mission_guard;

use mission_guard::*;
use sagax::prelude::*;

/// Every line `cargo run -p sagax --example mission_guard` prints.
const GOLDEN: &str = "\
t=0 A step=0 action=go 4,3 events=-
t=0 B step=0 action=go 4,3 events=-
t=1 A step=1 action=wait events=done:0
t=1 B step=1 action=wait events=done:0
t=2 A step=1 action=wait events=-
t=2 B step=1 action=wait events=-
t=3 A step=1 action=wait events=-
t=3 B step=1 action=wait events=-
t=4 A step=1 action=wait events=-
t=4 B step=1 action=wait events=-
t=5 A step=1 action=wait events=-
t=5 B step=1 action=wait events=-
t=6 A step=2 action=go 12,3 events=done:1
t=6 B step=2 action=go 12,3 events=done:1
t=7 A step=2 action=go 12,3 events=-
t=7 B step=2 action=go 12,3 events=-
t=8 A step=2 action=go 12,3 events=-
t=8 B step=2 action=go 12,3 events=-
t=9 A step=2 action=go 12,3 events=-
t=9 B step=2 action=go 12,3 events=-
t=10 A step=2 action=go 12,3 events=-
t=10 B step=2 action=go 12,3 events=-
t=11 A step=2 action=go 12,3 events=-
t=11 B step=2 action=go 12,3 events=-
t=12 A step=2 action=go 12,3 events=-
t=12 B step=2 action=go 12,3 events=-
t=13 A step=2 action=go 12,3 events=-
t=13 B step=2 action=go 12,3 events=-
t=14 A step=0 action=go 4,3 events=done:2,looped
t=14 B step=0 action=go 4,3 events=done:2,looped
t=15 A step=0 action=go 4,3 events=-
t=15 B step=0 action=go 4,3 events=-
t=16 A step=0 action=go 4,3 events=-
t=16 B step=0 action=go 4,3 events=-
t=17 A step=0 action=go 4,3 events=-
t=17 B step=0 action=go 4,3 events=-
t=18 A step=0 action=go 4,3 events=-
t=18 B step=0 action=go 4,3 events=-
t=19 A step=0 action=go 4,3 events=-
t=19 B step=0 action=go 4,3 events=-
t=20 A step=0 action=go 4,3 events=-
t=20 B step=0 action=go 4,3 events=-
t=21 A step=0 action=go 4,3 events=-
t=21 B step=0 action=go 4,3 events=-
t=22 A step=1 action=wait events=done:0
t=22 B step=1 action=wait events=done:0
t=23 A step=1 action=wait events=-
t=23 B step=1 action=wait events=-
t=24 A step=1 action=wait events=-
t=24 B step=1 action=wait events=-
t=25 A step=1 action=wait events=-
t=25 B step=1 action=wait events=-
t=26 A step=1 action=wait events=-
t=26 B step=1 action=wait events=-
t=27 A step=2 action=go 12,3 events=done:1
t=27 B step=2 action=go 12,3 events=done:1
t=28 A step=2 action=go 12,3 events=-
t=28 B step=2 action=go 12,3 events=-
t=29 A step=2 action=go 12,3 events=-
t=29 B step=2 action=go 12,3 events=-
t=30 A step=- action=attack 50@7,3 events=detour
t=30 B step=- action=attack 50@7,3 events=detour
t=31 A step=- action=attack 50@9,3 events=-
t=31 B step=- action=attack 50@9,3 events=-
t=32 A step=- action=attack 50@11,3 events=-
t=32 B step=- action=attack 50@11,3 events=-
t=33 A step=- action=attack 50@13,3 events=-
t=33 B step=- action=attack 50@13,3 events=-
t=34 A step=- action=go 6,3 events=detour_end:leash
t=34 B step=- action=attack 50@15,3 events=-
t=35 A step=- action=go 6,3 events=-
t=35 B step=- action=attack 50@15,3 events=-
t=36 A step=- action=go 6,3 events=-
t=36 B step=- action=attack 50@15,3 events=-
t=37 A step=- action=go 6,3 events=-
t=37 B step=- action=attack 50@15,3 events=-
t=38 A step=2 action=go 12,3 events=returned
t=38 B step=- action=search 15,3 events=-
t=39 A step=2 action=go 12,3 events=-
t=39 B step=- action=search 15,3 events=-
t=40 A step=2 action=go 12,3 events=-
t=40 B step=- action=search 15,3 events=-
t=41 A step=2 action=go 12,3 events=-
t=41 B step=- action=search 15,3 events=-
t=42 A step=2 action=go 12,3 events=-
t=42 B step=- action=search 15,3 events=-
t=43 A step=2 action=go 12,3 events=-
t=43 B step=- action=search 15,3 events=-
t=44 A step=0 action=go 4,3 events=done:2,looped
t=44 B step=2 action=go 12,3 events=detour_end:lost,returned
t=45 A step=0 action=go 4,3 events=-
t=45 B step=2 action=go 12,3 events=-
t=46 A step=0 action=go 4,3 events=-
t=46 B step=2 action=go 12,3 events=-
t=47 A step=0 action=go 4,3 events=-
t=47 B step=0 action=go 4,3 events=done:2,looped
t=48 A step=0 action=go 4,3 events=-
t=48 B step=0 action=go 4,3 events=-
t=49 A step=0 action=go 4,3 events=-
t=49 B step=0 action=go 4,3 events=-
t=50 A step=0 action=go 4,3 events=-
t=50 B step=0 action=go 4,3 events=-
t=51 A step=0 action=go 4,3 events=-
t=51 B step=0 action=go 4,3 events=-
t=52 A step=1 action=wait events=done:0
t=52 B step=0 action=go 4,3 events=-
t=53 A step=1 action=wait events=-
t=53 B step=0 action=go 4,3 events=-
t=54 A step=1 action=wait events=-
t=54 B step=0 action=go 4,3 events=-
t=55 A step=1 action=wait events=-
t=55 B step=1 action=wait events=done:0
t=56 A step=1 action=wait events=-
t=56 B step=1 action=wait events=-
t=57 A step=2 action=go 12,3 events=done:1
t=57 B step=1 action=wait events=-
t=58 A step=2 action=go 12,3 events=-
t=58 B step=1 action=wait events=-
t=59 A step=2 action=go 12,3 events=-
t=59 B step=1 action=wait events=-
t=60 A step=2 action=go 12,3 events=-
t=60 B step=2 action=go 12,3 events=done:1
t=61 A step=2 action=go 12,3 events=-
t=61 B step=2 action=go 12,3 events=-
t=62 A step=2 action=go 12,3 events=-
t=62 B step=2 action=go 12,3 events=-
t=63 A step=2 action=go 12,3 events=-
t=63 B step=2 action=go 12,3 events=-
t=64 A step=2 action=go 12,3 events=-
t=64 B step=2 action=go 12,3 events=-
t=65 A step=0 action=go 4,3 events=done:2,looped
t=65 B step=2 action=go 12,3 events=-
t=66 A step=0 action=go 4,3 events=-
t=66 B step=2 action=go 12,3 events=-
t=67 A step=0 action=go 4,3 events=-
t=67 B step=2 action=go 12,3 events=-
t=68 A step=0 action=go 4,3 events=-
t=68 B step=0 action=go 4,3 events=done:2,looped
t=69 A step=0 action=go 4,3 events=-
t=69 B step=0 action=go 4,3 events=-
t=70 A step=0 action=go 4,3 events=-
t=70 B step=0 action=attack 30@13,6 events=broken:fail,replanned:0
t=71 A step=0 action=go 4,3 events=-
t=71 B step=0 action=attack 30@13,6 events=-
t=72 A step=0 action=go 4,3 events=-
t=72 B step=0 action=attack 30@13,6 events=-
t=73 A step=1 action=wait events=done:0
t=73 B step=0 action=attack 30@13,6 events=-
t=74 A step=1 action=wait events=-
t=74 B step=0 action=attack 30@13,6 events=-
t=75 A step=1 action=wait events=-
t=75 B step=0 action=attack 30@13,6 events=-
t=76 A step=1 action=wait events=-
t=76 B step=0 action=attack 30@13,6 events=-
t=77 A step=1 action=wait events=-
t=77 B step=0 action=attack 30@13,6 events=-
t=78 A step=2 action=go 12,3 events=done:1
t=78 B step=0 action=attack 30@13,6 events=-
t=79 A step=2 action=go 12,3 events=-
t=79 B step=0 action=attack 30@13,6 events=-
t=80 A step=2 action=go 12,3 events=-
t=80 B step=0 action=attack 30@13,6 events=-
t=81 A step=2 action=go 12,3 events=-
t=81 B step=0 action=attack 30@13,6 events=-
t=82 A step=2 action=go 12,3 events=-
t=82 B step=0 action=attack 30@13,6 events=-
t=83 A step=2 action=go 12,3 events=-
t=83 B step=0 action=attack 30@13,6 events=-
t=84 A step=2 action=go 12,3 events=-
t=84 B step=0 action=attack 30@13,6 events=-
t=85 A step=2 action=go 12,3 events=-
t=85 B step=0 action=attack 30@13,6 events=-
t=86 A step=0 action=go 4,3 events=done:2,looped
t=86 B step=0 action=attack 30@13,6 events=-
t=87 A step=0 action=go 4,3 events=-
t=87 B step=0 action=attack 30@13,6 events=-
t=88 A step=0 action=go 4,3 events=-
t=88 B step=0 action=attack 30@13,6 events=-
t=89 A step=0 action=go 4,3 events=-
t=89 B step=0 action=attack 30@13,6 events=-
";

fn rows_of(guard: char) -> Vec<Row> {
    run_scene()
        .into_iter()
        .filter(|r| r.guard == guard)
        .collect()
}

#[test]
fn nothing_happens_the_trace_is_the_plain_script() {
    for (guard, start) in [('A', [3, 2, 0]), ('B', [3, 4, 0])] {
        let mut m = Mission::new(MissionConfig::default(), Motive::guard_place(WEST)).unwrap();
        m.set_plan(&patrol()).unwrap();
        let k = Knowledge::new(8, Evict::Oldest).unwrap();
        let mut pos: Pos = start;
        for row in rows_of(guard).iter().filter(|r| r.tick < ATTACKER_FROM) {
            let out = m.tick(row.tick, pos, &k, 0);
            assert_eq!(out.action, row.action, "{guard} t={}", row.tick);
            assert_eq!(out.events(), &row.events[..], "{guard} t={}", row.tick);
            assert_eq!(m.step_index(), row.step);
            assert!(!row.events.iter().any(|e| matches!(
                e,
                Event::Broken(_) | Event::DetourStarted | Event::Replanned { .. }
            )));
            pos = row.pos;
            let t = match row.action {
                Action::GoTo(p) => p,
                _ => pos,
            };
            pos = [
                pos[0] + (t[0] - pos[0]).signum(),
                pos[1] + (t[1] - pos[1]).signum(),
                0,
            ];
        }
    }
}

/// The step index a tick's events say the plan is resuming at, for the
/// rows after `from`: the first row with a step again.
fn resumed_step(rows: &[Row], from: u32) -> Option<usize> {
    rows.iter()
        .find(|r| r.tick > from && r.step.is_some())?
        .step
}

#[test]
fn strict_follows_three_cells_and_returns() {
    let rows = rows_of('A');
    let started = rows
        .iter()
        .find(|r| r.events.contains(&Event::DetourStarted))
        .unwrap();
    assert_eq!(started.tick, ATTACKER_FROM);
    let left_at = rows
        .iter()
        .find(|r| r.events.contains(&Event::DetourEnded(DetourEnd::Leash)))
        .unwrap();
    // The anchor is where A stood at the last tick before the detour.
    let Action::GoTo(anchor) = left_at.action else {
        panic!("walks back");
    };
    assert_eq!(chebyshev(anchor, started.pos), 1);
    // The leash ended it as soon as A stood beyond 3 cells from the anchor.
    assert_eq!(chebyshev(anchor, left_at.pos), 4);
    let back = rows
        .iter()
        .find(|r| r.events.contains(&Event::Returned))
        .unwrap();
    assert_eq!(back.pos, anchor);
    // It resumes the step it left (the walk east) and never breaks.
    assert_eq!(back.step, Some(2));
    assert_eq!(left_at.tick, back.tick - 4);
    assert!(!rows.iter().any(|r| {
        r.events
            .iter()
            .any(|e| matches!(e, Event::Broken(_) | Event::Replanned { .. }))
    }));
    // Told of nothing, it patrols to the end.
    assert!(
        rows.iter()
            .all(|r| !matches!(r.action, Action::Attack { subject: THIEF, .. }))
    );
    assert!(resumed_step(&rows, left_at.tick).is_some());
}

#[test]
fn flexible_chases_loses_sight_searches_returns_and_resumes() {
    let rows = rows_of('B');
    let find = |e: Event| rows.iter().find(|r| r.events.contains(&e)).unwrap();
    let started = find(Event::DetourStarted);
    assert_eq!(started.tick, ATTACKER_FROM);
    let lost = find(Event::DetourEnded(DetourEnd::Lost));
    let back = find(Event::Returned);
    // Same tick: the walk back is skipped, the target is the same step.
    assert_eq!(lost.tick, back.tick);
    let chase: Vec<&Row> = rows
        .iter()
        .filter(|r| (started.tick..lost.tick).contains(&r.tick))
        .collect();
    assert!(
        chase
            .iter()
            .any(|r| matches!(r.action, Action::Attack { .. }))
    );
    let search = chase
        .iter()
        .filter(|r| matches!(r.action, Action::Search(_)))
        .count();
    assert_eq!(search, 6, "searches 6 ticks, then gives up");
    // It resumes at the step it left; nothing broke.
    assert_eq!(back.step, Some(2));
    assert!(
        !rows
            .iter()
            .any(|r| r.tick < THEFT_AT && r.events.iter().any(|e| matches!(e, Event::Broken(_))))
    );
}

#[test]
fn only_the_told_guard_replans_to_the_thief() {
    let a = rows_of('A');
    let b = rows_of('B');
    let at_theft = b.iter().find(|r| r.tick == THEFT_AT).unwrap();
    assert_eq!(at_theft.step, Some(0));
    assert_eq!(
        at_theft.events,
        vec![
            Event::Broken(BreakReason::Fail(Fail::Fact {
                subject: 20,
                kind: 1,
                mask: 1,
                value: 1
            })),
            Event::Replanned { option: 0 }
        ]
    );
    assert_eq!(
        at_theft.action,
        Action::Attack {
            subject: THIEF,
            pos: THIEF_AT
        }
    );
    // From then on B goes for the thief; A never breaks and keeps patrolling.
    assert!(
        b.iter()
            .filter(|r| r.tick >= THEFT_AT)
            .all(|r| matches!(r.action, Action::Attack { subject: THIEF, .. }))
    );
    assert!(
        a.iter()
            .all(|r| !r.events.iter().any(|e| matches!(e, Event::Broken(_))))
    );
    assert!(
        a.iter()
            .filter(|r| r.tick >= THEFT_AT)
            .all(|r| matches!(r.action, Action::GoTo(_) | Action::Wait))
    );
}

#[test]
fn golden() {
    assert_eq!(trace_all().join("\n") + "\n", GOLDEN);
}
