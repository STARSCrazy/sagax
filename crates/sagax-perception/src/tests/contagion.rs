//! Contagion: deeds seen on allies, only through the observer's own
//! percepts.

use super::*;
use sagax_debug::{Collector, Overflow};

/// The cue bit "attacking".
const ATTACK: u32 = 1;
/// The cue bit "fleeing".
const FLEE: u32 = 2;
/// The observer.
const ME: u32 = 1;

/// Deed 0: attacking, window 5; deed 1: fleeing, window 10.
fn config(grid: Grid) -> ContagionConfig {
    ContagionConfig::new(grid)
        .with_deed(DeedRule {
            bits: ATTACK,
            window: 5,
        })
        .with_deed(DeedRule {
            bits: FLEE,
            window: 10,
        })
}

/// The human preset that recognises a near person at the first check.
fn quick() -> SenseProfile {
    SenseProfile {
        base_gain: 1024,
        ..SenseProfile::human(S)
    }
}

/// An ally `id` at `point` showing `cues`.
fn ally(id: u32, point: Pos, cues: u32) -> Target {
    Target {
        id,
        cues,
        ..person(point)
    }
}

/// Everyone is an ally.
fn everyone(_: u32, _: bool) -> bool {
    true
}

/// One `look` at every target on `map` in `world`; its percepts.
fn look_on(
    world: &World,
    map: &mut impl SenseMap,
    now: u32,
    eye: &Eye,
    targets: &[Target],
    aware: &mut [Awareness],
) -> Vec<Percept> {
    let mut out = Vec::new();
    let mut attention = Attention::default();
    let budget = Budget {
        checks: targets.len() as u16,
        keep: targets.len() as u16,
        instant_cap: 0,
    };
    look(
        world,
        map,
        now,
        eye,
        targets,
        aware,
        &mut attention,
        budget,
        &mut out,
    );
    out
}

/// Looks on `map` from tick `from` to `to` (inclusive), learning every
/// tick.
#[allow(clippy::too_many_arguments)]
fn watch(
    world: &World,
    map: &mut impl SenseMap,
    eye: &Eye,
    targets: &[Target],
    aware: &mut [Awareness],
    memory: &mut Contagion,
    from: u32,
    to: u32,
) -> Vec<Percept> {
    let mut all = Vec::new();
    for now in from..=to {
        let out = look_on(world, map, now, eye, targets, aware);
        memory.learn(&out, now, everyone);
        all.extend(out);
    }
    all
}

/// A hand-made recognised sighting of `subject` at `point` by sight.
fn sighting(subject: u32, point: Pos, cues: u32, at: u32) -> Percept {
    Percept {
        observer: ME,
        subject,
        sense: Sense::Sight,
        event: PerceptKind::Seen,
        stage: 2,
        point,
        cell: cell_of(&square(), point).pos(),
        placed: true,
        cues,
        disguised: false,
        clarity: 256,
        margin: 0,
        arrive_at: 0,
        at,
    }
}

/// Opaque cells from a list.
struct Walls(Vec<Cell>);

impl SenseMap for Walls {
    fn sight(&mut self, cell: Cell) -> Medium {
        Medium {
            opaque: self.0.contains(&cell),
            ..Medium::default()
        }
    }
}

/// No light anywhere.
struct Dark;

impl SenseMap for Dark {
    fn light(&mut self, _: Cell) -> u8 {
        0
    }
}

#[test]
fn an_ally_seen_attacking_counts() {
    let p = quick();
    let e = eye(&p);
    let targets = [ally(7, at(3, 0), ATTACK)];
    let mut aware = [Awareness::default()];
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let out = watch(
        &square(),
        &mut OpenAir,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        1,
        1,
    );
    assert!(has(&out, PerceptKind::Cues { new: ATTACK }));
    let i = memory.inputs(0, e.point, 1);
    assert_eq!(
        i,
        DeedInputs {
            doing: 1,
            seen: 1,
            share: 256,
            nearest: 3 * S,
            since: 0,
        }
    );
    assert_eq!(memory.nearest_point(0, e.point, 1), Some(at(3, 0)));
    // Fleeing: seen, but not doing it.
    assert_eq!(
        memory.inputs(1, e.point, 1),
        DeedInputs {
            seen: 1,
            ..DeedInputs::NONE
        }
    );
    assert_eq!(memory.nearest_point(1, e.point, 1), None);
    // Two ticks later, still in view and attacking.
    watch(
        &square(),
        &mut OpenAir,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        2,
        3,
    );
    assert_eq!(memory.inputs(0, e.point, 3).since, 2);
    assert_eq!(memory.len(), 1);
}

#[test]
fn an_ally_behind_a_wall_never_counts() {
    let p = quick();
    let e = eye(&p);
    let targets = [ally(7, at(3, 0), ATTACK | FLEE)];
    let mut aware = [Awareness::default()];
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let mut map = Walls(vec![Cell::new(2, 0, 0)]);
    let out = watch(
        &square(),
        &mut map,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        1,
        50,
    );
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(memory.inputs(0, e.point, 50), DeedInputs::NONE);
    assert_eq!(memory.inputs(1, e.point, 50), DeedInputs::NONE);
    assert_eq!(memory.len(), 0);
}

#[test]
fn an_ally_in_the_dark_never_counts() {
    let p = SenseProfile {
        dark_gain: 0,
        ..quick()
    };
    let e = eye(&p);
    let targets = [ally(7, at(3, 0), ATTACK)];
    let mut aware = [Awareness::default()];
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    watch(
        &square(),
        &mut Dark,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        1,
        50,
    );
    assert_eq!(memory.inputs(0, e.point, 50), DeedInputs::NONE);
    // The same ally with a torch is seen.
    let lit = [Target {
        glow: 255,
        ..targets[0]
    }];
    watch(
        &square(),
        &mut Dark,
        &e,
        &lit,
        &mut aware,
        &mut memory,
        51,
        51,
    );
    assert_eq!(memory.inputs(0, e.point, 51).doing, 1);
}

#[test]
fn an_ally_behind_the_observer_never_counts() {
    let p = quick();
    let e = eye(&p);
    let targets = [ally(7, at(-3, 0), ATTACK)];
    let mut aware = [Awareness::default()];
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    watch(
        &square(),
        &mut OpenAir,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        1,
        50,
    );
    assert_eq!(memory.inputs(0, e.point, 50), DeedInputs::NONE);
    // Turned around, it sees the ally.
    let back = Eye { facing: 128, ..e };
    watch(
        &square(),
        &mut OpenAir,
        &back,
        &targets,
        &mut aware,
        &mut memory,
        51,
        51,
    );
    assert_eq!(memory.inputs(0, e.point, 51).doing, 1);
}

#[test]
fn an_ally_not_yet_recognised_never_counts() {
    // The normal human preset: noticed after a few checks, recognised
    // later; until then the ally is neither doing nor seen.
    let p = SenseProfile::human(S);
    let e = eye(&p);
    let targets = [ally(7, at(3, 0), ATTACK)];
    let mut aware = [Awareness::default()];
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let mut recognised_at = None;
    for now in 1..=40 {
        let out = look_on(&square(), &mut OpenAir, now, &e, &targets, &mut aware);
        memory.learn(&out, now, everyone);
        if aware[0].stage() < 2 {
            assert_eq!(memory.inputs(0, e.point, now), DeedInputs::NONE, "{now}");
        } else if recognised_at.is_none() {
            recognised_at = Some(now);
        }
    }
    let at = recognised_at.expect("recognised in the end");
    assert!(at > 2, "noticed before recognised");
    let i = memory.inputs(0, e.point, 40);
    assert_eq!((i.doing, i.seen), (1, 1));
    assert_eq!(i.since, 40 - at);
}

#[test]
fn a_deed_expires_after_its_window() {
    let p = quick();
    let e = eye(&p);
    let mut targets = [ally(7, at(3, 0), ATTACK | FLEE)];
    let mut aware = [Awareness::default()];
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let mut map = Walls(vec![Cell::new(2, 1, 0)]);
    watch(
        &square(),
        &mut map,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        1,
        1,
    );
    // It steps behind the wall: Lost, the entry stays.
    targets[0].point = at(3, 2);
    let mut lost = false;
    let mut expired = Vec::new();
    for now in 2..=20 {
        let out = look_on(&square(), &mut map, now, &e, &targets, &mut aware);
        lost |= has(&out, PerceptKind::Lost);
        let mut sink = Collector::new(100, Overflow::DropNewest);
        memory.learn_traced(&out, now, everyone, &mut sink);
        expired.extend(sink.records().filter_map(|r| match r.event {
            ContagionTrace::Expired { ally, deed } => Some((now, ally, deed)),
            _ => None,
        }));
        let (attack, flee) = (
            memory.inputs(0, e.point, now),
            memory.inputs(1, e.point, now),
        );
        // Attacking: window 5; fleeing: window 10 (each its own).
        assert_eq!(attack.doing, u32::from(now <= 6), "{now}");
        assert_eq!(attack.seen, u32::from(now <= 6), "{now}");
        assert_eq!(flee.doing, u32::from(now <= 11), "{now}");
        if now <= 6 {
            assert_eq!(attack.since, now - 1);
            assert_eq!(attack.nearest, 3 * S, "the last point seen");
        } else {
            assert_eq!(attack, DeedInputs::NONE);
        }
    }
    assert!(lost);
    assert_eq!(expired, [(7, 7, 0), (12, 7, 1)]);
    assert_eq!(memory.len(), 0, "beyond every window the slot is free");
}

#[test]
fn reads_respect_the_window_without_learn() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    memory.learn(&[sighting(7, at(2, 0), ATTACK, 1)], 1, everyone);
    let o = at(0, 0);
    assert_eq!(memory.inputs(0, o, 6).doing, 1);
    assert_eq!(memory.inputs(0, o, 7), DeedInputs::NONE);
    assert_eq!(memory.nearest_point(0, o, 7), None);
    assert_eq!(memory.inputs(1, o, 11).seen, 1);
    assert_eq!(memory.inputs(1, o, 12).seen, 0);
}

#[test]
fn an_ally_seen_to_stop_no_longer_counts() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    memory.learn(&[sighting(7, at(2, 0), ATTACK, 1)], 1, everyone);
    memory.learn(&[sighting(7, at(2, 0), 0, 2)], 2, everyone);
    let i = memory.inputs(0, o, 2);
    assert_eq!((i.doing, i.seen, i.share), (0, 1, 0));
    // Attacks again: a new onset.
    memory.learn(&[sighting(7, at(2, 0), ATTACK, 4)], 4, everyone);
    memory.learn(&[sighting(7, at(2, 0), ATTACK, 5)], 5, everyone);
    assert_eq!(memory.inputs(0, o, 6).since, 2);
}

#[test]
fn an_onset_after_expiry_is_new() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    memory.learn(&[sighting(7, at(2, 0), FLEE, 1)], 1, everyone);
    // Seen again within the window: the same deed, the same onset.
    memory.learn(&[sighting(7, at(2, 0), FLEE, 8)], 8, everyone);
    assert_eq!(memory.inputs(1, o, 8).since, 7);
    // Seen again after the window ran out: a new onset.
    memory.learn(&[sighting(7, at(2, 0), FLEE | ATTACK, 30)], 30, everyone);
    assert_eq!(memory.inputs(1, o, 30).since, 0);
    assert_eq!(memory.inputs(0, o, 31).since, 1);
}

#[test]
fn share_nearest_and_since_over_several_allies() {
    let mut memory = Contagion::new(config(Grid::Square), 8, ME);
    let o = at(0, 0);
    memory.learn(
        &[
            sighting(10, at(1, 0), 0, 1),
            sighting(11, at(5, 0), ATTACK, 1),
            sighting(12, at(0, 3), ATTACK, 1),
            sighting(13, at(4, 0), 0, 1),
        ],
        1,
        everyone,
    );
    memory.learn(&[sighting(14, at(6, 0), ATTACK, 3)], 3, everyone);
    let i = memory.inputs(0, o, 3);
    assert_eq!(i.doing, 3);
    assert_eq!(i.seen, 5);
    assert_eq!(i.share, 3 * 256 / 5);
    assert_eq!(
        i.nearest,
        3 * S,
        "the nearest attacking ally, not the calm one"
    );
    assert_eq!(i.since, 0, "the most recent onset");
    assert_eq!(memory.nearest_point(0, o, 3), Some(at(0, 3)));
    // Distinct allies: several percepts of one ally count once.
    memory.learn(
        &[
            sighting(11, at(5, 0), ATTACK, 4),
            sighting(11, at(5, 1), ATTACK, 4),
        ],
        4,
        everyone,
    );
    assert_eq!(memory.inputs(0, o, 4).doing, 3);
}

#[test]
fn nearest_ties_go_to_the_lower_slot() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    memory.learn(
        &[
            sighting(20, at(0, 2), ATTACK, 1),
            sighting(21, at(2, 0), ATTACK, 1),
        ],
        1,
        everyone,
    );
    assert_eq!(memory.nearest_point(0, o, 1), Some(at(0, 2)));
}

#[test]
fn when_full_the_one_seen_longest_ago_goes() {
    let mut memory = Contagion::new(config(Grid::Square), 2, ME);
    let o = at(0, 0);
    let mut sink = Collector::new(100, Overflow::DropNewest);
    memory.learn_traced(
        &[
            sighting(1, at(1, 0), FLEE, 1),
            sighting(2, at(2, 0), FLEE, 1),
        ],
        1,
        everyone,
        &mut sink,
    );
    memory.learn_traced(&[sighting(2, at(2, 0), FLEE, 2)], 2, everyone, &mut sink);
    // 1 was seen longest ago.
    memory.learn_traced(&[sighting(3, at(3, 0), 0, 3)], 3, everyone, &mut sink);
    assert_eq!(memory.len(), 2);
    let i = memory.inputs(1, o, 3);
    assert_eq!((i.doing, i.seen), (1, 2));
    assert_eq!(i.nearest, 2 * S);
    // Now 2 (seen at 2) is the oldest.
    memory.learn_traced(&[sighting(4, at(4, 0), 0, 4)], 4, everyone, &mut sink);
    assert_eq!(memory.inputs(1, o, 4).doing, 0);
    let replaced: Vec<(u32, u32)> = sink
        .records()
        .filter_map(|r| match r.event {
            ContagionTrace::Replaced { old, ally } => Some((old, ally)),
            _ => None,
        })
        .collect();
    assert_eq!(replaced, [(1, 3), (2, 4)]);
    // Equal ages: the lower slot goes.
    let mut memory = Contagion::new(config(Grid::Square), 2, ME);
    memory.learn(
        &[sighting(1, at(1, 0), 0, 1), sighting(2, at(2, 0), FLEE, 1)],
        1,
        everyone,
    );
    memory.learn(&[sighting(3, at(3, 0), 0, 2)], 2, everyone);
    assert_eq!(memory.inputs(1, o, 2).doing, 1, "2 stays, 1 went");
    // Capacity 0 remembers nothing.
    let mut none = Contagion::new(config(Grid::Square), 0, ME);
    none.learn(&[sighting(1, at(1, 0), FLEE, 1)], 1, everyone);
    assert_eq!(none.inputs(1, o, 1), DeedInputs::NONE);
    assert_eq!(none.capacity(), 0);
}

#[test]
fn is_ally_gets_the_disguise_and_decides() {
    // An enemy disguised as a guard stages a flight in plain view.
    let p = quick();
    let mut e = eye(&p);
    e.disguise = DisguiseRule {
        hides: ATTACK,
        see_through: None,
    };
    let targets = [
        Target {
            disguise: 200,
            ..ally(30, at(3, 0), FLEE | ATTACK)
        },
        ally(31, at(3, 1), 0),
    ];
    let mut aware = [Awareness::default(); 2];
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let out = look_on(&square(), &mut OpenAir, 1, &e, &targets, &mut aware);
    let mut asked = Vec::new();
    // The game takes everyone in a guard's uniform for an ally: 30 shows
    // the uniform (disguised), 31 is a real guard.
    memory.learn(&out, 1, |subject, disguised| {
        asked.push((subject, disguised));
        subject == 31 || disguised
    });
    assert!(asked.contains(&(30, true)));
    assert!(asked.contains(&(31, false)));
    let flee = memory.inputs(1, e.point, 1);
    assert_eq!((flee.doing, flee.seen), (1, 2), "the staged flight counts");
    // The hidden cue (attacking) is never seen.
    assert_eq!(memory.inputs(0, e.point, 1).doing, 0);
    // A game that does not trust disguises ignores it.
    let mut wary = Contagion::new(config(Grid::Square), 4, ME);
    wary.learn(&out, 1, |_, disguised| !disguised);
    assert_eq!(wary.inputs(1, e.point, 1).doing, 0);
    assert_eq!(wary.inputs(1, e.point, 1).seen, 1);
}

#[test]
fn a_changed_belief_drops_the_entry() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    let mut sink = Collector::new(100, Overflow::DropNewest);
    let mut p = sighting(30, at(2, 0), FLEE, 1);
    p.disguised = true;
    memory.learn_traced(&[p], 1, |_, d| d, &mut sink);
    assert_eq!(memory.inputs(1, o, 1).doing, 1);
    // Seen through: no longer disguised, no longer an ally.
    let p = sighting(30, at(2, 0), FLEE, 2);
    memory.learn_traced(&[p], 2, |_, d| d, &mut sink);
    assert_eq!(memory.inputs(1, o, 2), DeedInputs::NONE);
    assert!(
        sink.records()
            .any(|r| r.event == ContagionTrace::Dropped { ally: 30 })
    );
    // A non-ally never gets an entry.
    memory.learn(&[sighting(40, at(2, 0), FLEE, 3)], 3, |_, _| false);
    assert_eq!(memory.len(), 0);
}

#[test]
fn heard_and_scanner_percepts_never_carry_deeds() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    // A real heard percept.
    let p = SenseProfile::human(S);
    let ear = eye(&p);
    let shout = Stimulus {
        id: 1,
        source: 7,
        kind: 0,
        point: at(3, 0),
        loudness: 8 * S,
        channel: 0,
        disguise: 0,
        at: 1,
    };
    let mut out = Vec::new();
    assert!(hear(
        &square(),
        &mut OpenAir,
        1,
        &ear,
        &shout,
        None,
        None,
        &mut out,
        0
    ));
    assert!(has(&out, PerceptKind::Heard));
    // A real scanner contact behind a wall.
    let scan = quick().with_scanner(6 * S);
    let e = eye(&scan);
    let mut aware = [Awareness::default()];
    let mut map = Walls(vec![Cell::new(2, 0, 0)]);
    for now in 1..=20 {
        out.extend(look_on(
            &square(),
            &mut map,
            now,
            &e,
            &[ally(7, at(3, 0), ATTACK)],
            &mut aware,
        ));
    }
    assert!(out.iter().any(|p| p.sense == Sense::Scanner));
    // Hand-made ones that claim the most: stage 2, cues, placed.
    let mut heard = sighting(8, at(1, 0), ATTACK, 1);
    heard.sense = Sense::Hearing(0);
    heard.event = PerceptKind::Heard;
    let mut scanned = sighting(9, at(1, 0), ATTACK, 1);
    scanned.sense = Sense::Scanner;
    out.extend([heard, scanned]);
    memory.learn(&out, 20, everyone);
    assert_eq!(memory.inputs(0, o, 20), DeedInputs::NONE);
    assert_eq!(memory.len(), 0);
}

#[test]
fn lost_and_forgotten_and_unplaced_percepts_change_nothing() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    memory.learn(&[sighting(7, at(2, 0), FLEE, 1)], 1, everyone);
    for event in [PerceptKind::Lost, PerceptKind::Forgotten] {
        let mut p = sighting(7, [0; 3], FLEE, 3);
        p.event = event;
        p.placed = false;
        memory.learn(&[p], 3, everyone);
    }
    let i = memory.inputs(1, o, 3);
    assert_eq!((i.doing, i.since, i.nearest), (1, 2, 2 * S));
    // Noticed only (stage 1): not counted as seen.
    let mut p = sighting(8, at(1, 0), 0, 3);
    p.stage = 1;
    memory.learn(&[p], 3, everyone);
    assert_eq!(memory.inputs(1, o, 3).seen, 1);
}

#[test]
fn thermal_sight_counts() {
    let p = SenseProfile {
        dark_gain: 0,
        ..quick().with_thermal()
    };
    let e = eye(&p);
    let targets = [Target {
        heat: 255,
        ..ally(7, at(3, 0), FLEE)
    }];
    let mut aware = [Awareness::default()];
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let out = watch(
        &square(),
        &mut Dark,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        1,
        3,
    );
    assert!(
        out.iter()
            .any(|p| p.sense == Sense::Thermal && p.stage == 2)
    );
    assert_eq!(memory.inputs(1, e.point, 3).doing, 1);
}

#[test]
fn hex_distances_and_walls() {
    let world = World::hex(S);
    let hex = |q: i32, r: i32| cell_centre(&world, Cell::new(q, r, 0));
    let p = quick();
    let e = Eye {
        point: hex(0, 0),
        ..eye(&p)
    };
    let targets = [ally(7, hex(3, 0), ATTACK), ally(8, hex(2, -1), ATTACK)];
    let mut aware = [Awareness::default(); 2];
    let mut memory = Contagion::new(config(Grid::Hex), 4, ME);
    // A wall hex between the observer and 7.
    let mut map = Walls(vec![Cell::new(2, 0, 0)]);
    watch(
        &world,
        &mut map,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        1,
        3,
    );
    let i = memory.inputs(0, e.point, 3);
    assert_eq!((i.doing, i.seen), (1, 1), "7 is behind the wall");
    // Hex distance of (2, -1): 2 hexes.
    assert_eq!(i.nearest, 2 * S);
    // Without the wall both count; 7 is 3 hexes away.
    watch(
        &world,
        &mut OpenAir,
        &e,
        &targets,
        &mut aware,
        &mut memory,
        4,
        4,
    );
    let i = memory.inputs(0, e.point, 4);
    assert_eq!((i.doing, i.nearest), (2, 2 * S));
    // On square the same fine points are farther (Euclid).
    let mut sq = Contagion::new(config(Grid::Square), 4, ME);
    sq.learn(&[sighting(8, hex(2, -1), ATTACK, 1)], 1, everyone);
    assert_eq!(sq.inputs(0, [0; 3], 1).nearest, 572, "isqrt(512² + 256²)");
}

#[test]
fn a_cascade_runs_only_along_sight() {
    // A (at (3, 4)) flees. B (3, 0) faces north and sees A. C (6, 0)
    // faces west and sees B, but a wall hides A from it. D (9, 0) faces
    // east, away from all of them. Each one flees once it sees a fleeing
    // ally; the game then sets its cue.
    let world = square();
    let p = quick();
    let mut map = Walls(vec![Cell::new(4, 2, 0), Cell::new(5, 2, 0)]);
    let points = [at(3, 4), at(3, 0), at(6, 0), at(9, 0)];
    let facing = [0u8, 64, 128, 0];
    let mut cues = [FLEE, 0, 0, 0];
    let mut memories: Vec<Contagion> = (0..4)
        .map(|me| Contagion::new(config(Grid::Square), 4, 10 + me as u32))
        .collect();
    let mut aware = [[Awareness::default(); 3]; 4];
    let mut fled_at = [Some(0u32), None, None, None];
    for now in 1..=20 {
        // What everyone shows at the start of the tick.
        let shown = cues;
        for me in 0..4 {
            let e = Eye {
                id: 10 + me as u32,
                point: points[me],
                facing: facing[me],
                ..eye(&p)
            };
            let others: Vec<Target> = (0..4)
                .filter(|&k| k != me)
                .map(|k| ally(10 + k as u32, points[k], shown[k]))
                .collect();
            let out = look_on(&world, &mut map, now, &e, &others, &mut aware[me]);
            memories[me].learn(&out, now, everyone);
            if cues[me] & FLEE == 0 && memories[me].inputs(1, e.point, now).doing > 0 {
                cues[me] |= FLEE;
                fled_at[me] = Some(now);
            }
        }
    }
    assert_eq!(fled_at[1], Some(1), "B sees A");
    assert_eq!(fled_at[2], Some(2), "C sees B, one tick later");
    assert_eq!(fled_at[3], None, "D faces away");
    // C never saw A.
    assert_eq!(memories[2].inputs(1, points[2], 20).seen, 1, "only B");
}

#[test]
fn columns_and_out_of_range_deeds() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    let mut row = [-1; 7];
    memory.write_columns(0, o, 1, &mut row[1..]);
    assert_eq!(row, [-1, 0, 0, 0, i32::MAX, i32::MAX, -1]);
    memory.learn(&[sighting(7, at(2, 0), ATTACK, 1)], 1, everyone);
    memory.write_columns(0, o, 3, &mut row[1..]);
    assert_eq!(row, [-1, 1, 1, 256, 2 * S, 2, -1]);
    assert_eq!(DeedInputs::NONE.columns(), [0, 0, 0, i32::MAX, i32::MAX]);
    assert_eq!(DEED_COLUMNS, 5);
    // Deed 2 is not configured; deed 8 is out of range.
    assert_eq!(memory.inputs(2, o, 1), DeedInputs::NONE);
    assert_eq!(memory.inputs(8, o, 1), DeedInputs::NONE);
    assert_eq!(memory.nearest_point(9, o, 1), None);
}

#[test]
fn the_config_holds_at_most_max_deeds() {
    let mut c = ContagionConfig::new(Grid::Square);
    for k in 0..10 {
        c = c.with_deed(DeedRule {
            bits: 1 << k,
            window: k,
        });
    }
    assert_eq!(usize::from(c.count), MAX_DEEDS);
    assert_eq!(c.deeds[7].bits, 1 << 7);
}

#[test]
fn forget_and_clear() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    memory.learn(
        &[
            sighting(7, at(2, 0), FLEE, 1),
            sighting(8, at(3, 0), FLEE, 1),
        ],
        1,
        everyone,
    );
    memory.forget(7);
    assert_eq!(memory.inputs(1, o, 1).doing, 1);
    memory.forget(99);
    assert_eq!(memory.len(), 1);
    memory.clear();
    assert_eq!(memory.len(), 0);
    assert_eq!(memory.inputs(1, o, 1), DeedInputs::NONE);
    assert_eq!(memory.capacity(), 4);
}

#[test]
fn the_trace_reports_onset_refresh_and_inputs() {
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    let o = at(0, 0);
    let mut sink = Collector::new(100, Overflow::DropNewest);
    // Three percepts of one check: one refresh record.
    let p = sighting(7, at(2, 0), ATTACK, 1);
    let recognized = Percept {
        event: PerceptKind::Recognized,
        ..p
    };
    let cues = Percept {
        event: PerceptKind::Cues { new: ATTACK },
        ..p
    };
    memory.learn_traced(&[recognized, cues, p], 1, everyone, &mut sink);
    let inputs = memory.inputs_traced(0, o, 2, &mut sink);
    let mut row = [0; 5];
    memory.write_columns_traced(0, o, 2, &mut row, &mut sink);
    let events: Vec<ContagionTrace> = sink.records().map(|r| r.event).collect();
    assert_eq!(
        events,
        [
            ContagionTrace::Onset { ally: 7, deed: 0 },
            ContagionTrace::Refresh {
                ally: 7,
                bits: ATTACK
            },
            ContagionTrace::Inputs { deed: 0, inputs },
            ContagionTrace::Inputs { deed: 0, inputs },
        ]
    );
    assert_eq!(row, inputs.columns());
}

#[test]
fn percepts_of_another_observer_never_count() {
    // One buffer with the percepts of two guards: ME sees nothing, guard 2
    // sees 7 attack. ME's memory learns only its own.
    let mut theirs = sighting(7, at(2, 0), ATTACK, 1);
    theirs.observer = 2;
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    assert_eq!(memory.owner(), ME);
    let mut sink = Collector::new(8, Overflow::DropNewest);
    memory.learn_traced(&[theirs], 1, everyone, &mut sink);
    assert!(memory.is_empty());
    assert_eq!(sink.records().len(), 0);
    assert_eq!(memory.inputs(0, at(0, 0), 1), DeedInputs::NONE);
    // A real mixed buffer: ME (facing +x) and guard 2 behind a wall look at
    // the same attacking ally; only ME's percepts count for ME, and guard
    // 2's memory, fed the same buffer, learns nothing.
    let p = quick();
    let me = eye(&p);
    let walled = Eye {
        id: 2,
        point: at(0, 4),
        ..eye(&p)
    };
    let targets = [ally(7, at(3, 4), ATTACK)];
    let mut map = Walls(vec![Cell::new(1, 4, 0)]);
    let (mut a, mut b) = ([Awareness::default()], [Awareness::default()]);
    let mut mine = Contagion::new(config(Grid::Square), 4, ME);
    let mut blind = Contagion::new(config(Grid::Square), 4, 2);
    for now in 1..=10 {
        let mut all = look_on(&square(), &mut map, now, &me, &targets, &mut a);
        all.extend(look_on(&square(), &mut map, now, &walled, &targets, &mut b));
        mine.learn(&all, now, everyone);
        blind.learn(&all, now, everyone);
    }
    assert_eq!(mine.inputs(0, me.point, 10).doing, 1);
    assert!(blind.is_empty(), "guard 2 never saw 7");
}

#[test]
fn a_stale_percept_does_not_rearm_the_window() {
    let o = at(0, 0);
    let old = sighting(7, at(2, 0), ATTACK, 1);
    let mut memory = Contagion::new(config(Grid::Square), 4, ME);
    memory.learn(&[old], 1, everyone);
    // The same percept passed again at 4 is skipped: the sighting stays at
    // 1, so the deed (window 5) has run out at 7.
    let mut sink = Collector::new(8, Overflow::DropNewest);
    memory.learn_traced(&[old], 4, everyone, &mut sink);
    assert_eq!(sink.records().len(), 0);
    assert_eq!(memory.inputs(0, o, 6).doing, 1);
    assert_eq!(memory.inputs(0, o, 7).doing, 0);
    // A percept of an earlier check, passed first later, teaches nothing.
    let mut fresh = Contagion::new(config(Grid::Square), 4, ME);
    fresh.learn(&[sighting(8, at(2, 0), FLEE, 2)], 3, everyone);
    assert!(fresh.is_empty());
}
