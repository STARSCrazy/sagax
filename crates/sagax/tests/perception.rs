//! `sagax::perception`: the adapters from nav maps to sight.

use sagax::perception::*;
use sagax::prelude::*;

/// 6 x 3, a wall at x = 3 for y 0..=1 (open at y = 2).
fn walled() -> DenseGrid {
    let mut map = DenseGrid::new(6, 3);
    map.set(3, 0, DenseGrid::BLOCKED);
    map.set(3, 1, DenseGrid::BLOCKED);
    map
}

#[test]
fn grid_sight_blocks_blocked() {
    let map = walled();
    let world = World::square(256);
    let at = |x, y| cell_centre(&world, Cell::new(x, y, 0));
    let line = sight_line(&world, &mut GridSight::square(&map), at(0, 0), at(5, 0));
    assert!(!line.clear);
    assert_eq!(line.blocked_at, Some(Cell::new(3, 0, 0)));
    assert!(sight_line(&world, &mut GridSight::square(&map), at(0, 2), at(5, 2)).clear);
    // Off the grid counts as opaque; costs other than BLOCKED do not.
    let mut map = walled();
    map.set(1, 2, 50);
    assert!(sight_line(&world, &mut GridSight::square(&map), at(0, 2), at(5, 2)).clear);
    assert!(GridSight::square(&map).sight(Cell::new(-1, 0, 0)).opaque);
}

/// Steps allowed except from (1, 0) to (2, 0), in both directions.
struct OneWall;

impl CellMap for OneWall {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let to = (from.x + dx, from.y + dy);
        let wall = [(from.x, from.y), to] == [(1, 0), (2, 0)]
            || [(from.x, from.y), to] == [(2, 0), (1, 0)];
        (!wall).then_some(from.level)
    }
}

#[test]
fn move_blocks_follows_step() {
    let world = World::square(256);
    let at = |x, y| cell_centre(&world, Cell::new(x, y, 0));
    let mut sight = MoveBlocks::square(OneWall);
    assert!(!sight.sight_edge(Cell::new(1, 0, 0), Cell::new(2, 0, 0)));
    assert!(sight.sight_edge(Cell::new(1, 1, 0), Cell::new(2, 1, 0)));
    let line = sight_line(&world, &mut sight, at(0, 0), at(4, 0));
    assert_eq!(
        (line.clear, line.blocked_at),
        (false, Some(Cell::new(2, 0, 0)))
    );
    assert!(sight_line(&world, &mut sight, at(0, 1), at(4, 1)).clear);
    // A DenseGrid wall is a blocked step too.
    let mut grid = MoveBlocks::square(walled());
    assert!(!sight_line(&world, &mut grid, at(0, 0), at(5, 0)).clear);
    assert!(sight_line(&world, &mut grid, at(0, 2), at(5, 2)).clear);
    // A step that changes the level (stairs) does not carry sight.
    struct Stairs;
    impl CellMap for Stairs {
        fn step(&mut self, from: Cell, _: i32, _: i32) -> Option<i32> {
            Some(from.level + 1)
        }
    }
    assert!(!MoveBlocks::square(Stairs).sight_edge(Cell::new(0, 0, 0), Cell::new(1, 0, 0)));
}

/// A hex world over offset maps (odd rows shifted): axial cells are
/// converted before the map is asked.
const LAYOUT: HexLayout = HexLayout::OddR;

fn hex_at(world: &World, q: i32, r: i32) -> Pos {
    cell_centre(world, Cell::new(q, r, 0))
}

#[test]
fn grid_sight_hex_layout() {
    let world = World::hex(256);
    // Axial (3, 2) is offset (4, 2) in OddR.
    assert_eq!(LAYOUT.from_axial(3, 2), (4, 2));
    let mut map = DenseGrid::new(8, 8);
    map.set(4, 2, DenseGrid::BLOCKED);
    let mut sight = GridSight::hex(&map, LAYOUT);
    let line = sight_line(
        &world,
        &mut sight,
        hex_at(&world, 1, 2),
        hex_at(&world, 5, 2),
    );
    assert!(!line.clear);
    assert_eq!(line.blocked_at, Some(Cell::new(3, 2, 0)));
    // The row below misses the wall.
    let line = sight_line(
        &world,
        &mut sight,
        hex_at(&world, 1, 3),
        hex_at(&world, 5, 3),
    );
    assert!(line.clear);
    // Read without the layout, the same map names the wrong cell: the
    // wall would sit at axial (4, 2).
    let unconverted = sight_line(
        &world,
        &mut GridSight::square(&map),
        hex_at(&world, 1, 2),
        hex_at(&world, 5, 2),
    );
    assert_eq!(unconverted.blocked_at, Some(Cell::new(4, 2, 0)));
}

/// Allows exactly the six offset neighbour steps of `LAYOUT`, except
/// between offset (3, 2) and (4, 2) (axial (2, 2) and (3, 2)).
struct OffsetWall;

impl CellMap for OffsetWall {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let to = (from.x + dx, from.y + dy);
        LAYOUT.direction(from.x, from.y, to.0, to.1)?;
        let pair = [(from.x, from.y), to];
        let wall = pair == [(3, 2), (4, 2)] || pair == [(4, 2), (3, 2)];
        (!wall).then_some(from.level)
    }
}

#[test]
fn move_blocks_hex_layout() {
    let world = World::hex(256);
    let mut sight = MoveBlocks::hex(OffsetWall, LAYOUT);
    assert!(!sight.sight_edge(Cell::new(2, 2, 0), Cell::new(3, 2, 0)));
    assert!(!sight.sight_edge(Cell::new(3, 2, 0), Cell::new(2, 2, 0)));
    let line = sight_line(
        &world,
        &mut sight,
        hex_at(&world, 0, 2),
        hex_at(&world, 5, 2),
    );
    assert_eq!(
        (line.clear, line.blocked_at),
        (false, Some(Cell::new(3, 2, 0)))
    );
    // Every axial neighbour step of a line across rows is a real offset
    // neighbour step: the lines stay clear.
    for (a, b) in [
        ((2, 0), (2, 6)),
        ((5, 0), (0, 5)),
        ((0, 4), (6, 1)),
        ((0, 5), (6, 3)),
    ] {
        let (a, b) = (hex_at(&world, a.0, a.1), hex_at(&world, b.0, b.1));
        assert!(sight_line(&world, &mut sight, a, b).clear, "{a:?} -> {b:?}");
    }
}

// ---------------------------------------------------------------------
// Hearing and the Knowledge helpers.

use sagax::decide::memory::{AlertConfig, Alertness, Route, SOURCE_HEARD, SOURCE_OWN, route};
use sagax::nav::Budget as NavBudget;
use sagax::nav::grid::Moves;

/// Fine units per cell in these tests.
const S: i32 = 256;

/// Seeded numbers.
struct Rng(u64);

impl Rng {
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        self.0 = self.0.wrapping_add(1);
        lo + (sagax::decide::mix(self.0) % (hi - lo + 1) as u64) as i32
    }
}

/// An ear (or eye) with id 1 at `point`, facing +x.
fn ear(profile: &SenseProfile, point: Pos) -> Eye<'_> {
    Eye {
        id: 1,
        point,
        facing: 0,
        profile,
        stats: Stats::NORMAL,
        disguise: DisguiseRule::default(),
    }
}

/// A stimulus of subject 9 at `point`.
fn stimulus(point: Pos, loudness: i32) -> Stimulus {
    Stimulus {
        id: 4,
        source: 9,
        kind: 1,
        point,
        loudness,
        channel: 0,
        disguise: 0,
        at: 0,
    }
}

#[test]
fn circle_equals_route_in_range() {
    let p = SenseProfile::human(S);
    for (world, metric) in [
        (World::square(S), Metric::Euclid),
        (World::hex(S), Metric::Hex),
    ] {
        let mut rng = Rng(if metric == Metric::Hex { 500 } else { 0 });
        let (mut yes, mut no) = (0, 0);
        for _ in 0..50 {
            let (sx, sy) = (rng.range(-10, 10), rng.range(-10, 10));
            let (lx, ly) = (rng.range(-10, 10), rng.range(-10, 10));
            let loud = rng.range(1, 14);
            let e = ear(&p, cell_centre(&world, Cell::new(lx, ly, 0)));
            let s = stimulus(cell_centre(&world, Cell::new(sx, sy, 0)), loud * S);
            let mut out = Vec::new();
            let heard = hear(&world, &mut OpenAir, 1, &e, &s, None, None, &mut out, 0);
            let mut ids = Vec::new();
            route(
                Route::InRange(i64::from(loud)),
                (sx, sy),
                &[(1, lx, ly)],
                metric,
                1,
                &mut ids,
            );
            assert_eq!(
                heard,
                !ids.is_empty(),
                "{metric:?} {sx},{sy} -> {lx},{ly} at {loud}"
            );
            if heard {
                yes += 1;
            } else {
                no += 1;
            }
        }
        assert!(yes > 5 && no > 5, "{metric:?}: {yes} heard, {no} not");
    }
}

/// An L-shaped corridor: (0..=5, 0) and (5, 0..=5); sound does not cross
/// into anything else.
struct Corridor;

impl Corridor {
    fn open(c: Cell) -> bool {
        (c.y == 0 && (0..=5).contains(&c.x)) || (c.x == 5 && (0..=5).contains(&c.y))
    }
}

impl SenseMap for Corridor {
    fn sound_edge(&mut self, from: Cell, to: Cell) -> Option<u32> {
        (Self::open(from) && Self::open(to)).then_some(0)
    }
}

/// Square rules with four directions and a straight step of `S`.
fn four() -> GridRules {
    GridRules {
        moves: Moves::Four,
        cut_corners: false,
        straight: S as u32,
        diagonal: 362,
    }
}

#[test]
fn flood_around_corner() {
    let mut world = World::square(S);
    world.channels[0].spread = Spread::Straight;
    let p = SenseProfile::human(S);
    let at = |x, y| cell_centre(&World::square(S), Cell::new(x, y, 0));
    let s = stimulus(at(0, 0), 12 * S);
    let e = ear(&p, at(5, 5));
    let mut out = Vec::new();
    // Straight through the walls: nothing.
    assert!(!hear(
        &world,
        &mut Corridor,
        1,
        &e,
        &s,
        None,
        None,
        &mut out,
        0
    ));
    // The flood goes round the corner: ten steps.
    let mut nav = Navigator::new(Topology::Square(four()), 200);
    let mut costs = Vec::new();
    flood_sound(
        &mut nav,
        &world,
        &mut Corridor,
        &s,
        NavBudget::UNLIMITED,
        &mut costs,
    );
    let cost = costs
        .iter()
        .find(|(c, _)| *c == Cell::new(5, 5, 0))
        .map(|&(_, c)| c);
    assert_eq!(cost, Some(10 * S));
    assert!(costs.iter().all(|&(c, _)| Corridor::open(c)));
    assert!(hear(
        &world,
        &mut Corridor,
        1,
        &e,
        &s,
        cost,
        None,
        &mut out,
        0
    ));
    // Bounded by the loudness.
    let quiet = stimulus(at(0, 0), 9 * S);
    flood_sound(
        &mut nav,
        &world,
        &mut Corridor,
        &quiet,
        NavBudget::UNLIMITED,
        &mut costs,
    );
    assert!(costs.iter().all(|&(_, c)| c <= 9 * S));
    assert!(!costs.iter().any(|(c, _)| *c == Cell::new(5, 5, 0)));
}

/// Damping in one cell: 128 per cell length.
struct Mud;

impl SenseMap for Mud {
    fn sound(&mut self, cell: Cell) -> u32 {
        if cell == Cell::new(1, 0, 0) { 128 } else { 0 }
    }
}

#[test]
fn flood_adds_damping() {
    let mut world = World::square(S);
    world.channels[0].damping = 512;
    let mut nav = Navigator::new(Topology::Square(four()), 200);
    let mut costs = Vec::new();
    let s = stimulus(cell_centre(&world, Cell::new(0, 0, 0)), 3 * S);
    flood_sound(
        &mut nav,
        &world,
        &mut Mud,
        &s,
        NavBudget::UNLIMITED,
        &mut costs,
    );
    let cost_of = |cell: Cell| costs.iter().find(|(c, _)| *c == cell).map(|&(_, c)| c);
    // Into the mud: 256 + 512 * 128 / 256.
    assert_eq!(cost_of(Cell::new(1, 0, 0)), Some(S + 256));
    assert_eq!(cost_of(Cell::new(0, 1, 0)), Some(S));
}

#[test]
fn flood_on_hex_gives_axial_cells() {
    let world = World::hex(S);
    let layout = HexLayout::OddR;
    let topology = Topology::Hex {
        layout,
        step_cost: S as u32,
    };
    let mut nav = Navigator::new(topology, 400);
    let start = Cell::new(2, 3, 0);
    let s = stimulus(cell_centre(&world, start), 3 * S);
    let mut costs = Vec::new();
    flood_sound(
        &mut nav,
        &world,
        &mut OpenAir,
        &s,
        NavBudget::UNLIMITED,
        &mut costs,
    );
    // Every hex within 3 rings: 1 + 6 + 12 + 18.
    assert_eq!(costs.len(), 37);
    for &(c, cost) in &costs {
        let (dq, dr) = (c.x - start.x, c.y - start.y);
        let rings = (dq.abs() + dr.abs() + (dq + dr).abs()) / 2;
        assert_eq!(cost, rings * S, "{c:?}");
    }
}

const NOTICED: u32 = 1;
const SEEN: u32 = 2;
const HEARD: u32 = 3;
const DISGUISED: u32 = 4;

fn kinds() -> FactKinds {
    FactKinds {
        noticed_kind: NOTICED,
        seen_kind: SEEN,
        heard_kind: HEARD,
        disguised_kind: None,
    }
}

/// No alert amounts.
fn quiet() -> AlertAmounts {
    AlertAmounts::default()
}

/// A guard that recognises a target with cue bit 1 at once.
fn sharp() -> SenseProfile {
    SenseProfile::guard(S).with_cue(CueRule {
        bits: 1,
        instant: 2,
        ..CueRule::default()
    })
}

/// A walking person (id 7) at `point` with the cue bits `cues`.
fn person(point: Pos, cues: u32) -> Target {
    Target {
        id: 7,
        point,
        size: 256,
        conspicuity: 256,
        cues,
        ..Target::default()
    }
}

/// Memory, alertness and its config for the helper tests.
fn memory() -> (Knowledge, Alertness, AlertConfig) {
    let cfg = AlertConfig {
        max: 1000,
        count: 2,
        thresholds: [38, 39, 0, 0, 0, 0, 0, 0],
        hold: 1000,
        ..AlertConfig::default()
    };
    (
        Knowledge::new(8, Evict::Oldest).unwrap(),
        Alertness::default(),
        cfg,
    )
}

#[test]
fn seen_keeps_fact_pos_current() {
    let world = World::square(S);
    let at = |x, y| cell_centre(&world, Cell::new(x, y, 0));
    let p = sharp();
    let e = ear(&p, at(0, 0));
    let (mut known, mut alert, cfg) = memory();
    let mut aware = Awareness::default();
    for (i, x) in (2..=6).enumerate() {
        let now = i as u32 + 1;
        let mut out = Vec::new();
        let t = person(at(x, 0), 1);
        observe(&world, &mut OpenAir, now, &e, &t, &mut aware, &mut out);
        learn_percepts(&mut known, &mut alert, &cfg, now, &out, &kinds(), &quiet());
        let fact = known.get(7, SEEN).unwrap();
        assert_eq!((fact.pos, fact.seen_at), ([x, 0, 0], now));
        assert_eq!(
            (fact.source, fact.data, fact.strength),
            (SOURCE_OWN, 1, 256)
        );
    }
}

#[test]
fn recognition_replaces_noticed() {
    let world = World::square(S);
    let at = |x, y| cell_centre(&world, Cell::new(x, y, 0));
    let p = SenseProfile::human(S);
    let e = ear(&p, at(0, 0));
    let (mut known, mut alert, cfg) = memory();
    let mut aware = Awareness::default();
    let mut had_noticed = false;
    for now in 1..=20 {
        let mut out = Vec::new();
        let t = person(at(3, 0), 0);
        observe(&world, &mut OpenAir, now, &e, &t, &mut aware, &mut out);
        learn_percepts(&mut known, &mut alert, &cfg, now, &out, &kinds(), &quiet());
        match aware.stage() {
            0 => assert!(known.get(7, NOTICED).is_none() && known.get(7, SEEN).is_none()),
            1 => {
                assert!(known.get(7, NOTICED).is_some() && known.get(7, SEEN).is_none());
                had_noticed = true;
            }
            _ => {
                assert!(known.get(7, NOTICED).is_none());
                assert_eq!(known.get(7, SEEN).unwrap().pos, [3, 0, 0]);
            }
        }
    }
    assert!(had_noticed && aware.stage() == 2);
}

#[test]
fn lost_removes_nothing() {
    let world = World::square(S);
    let at = |x, y| cell_centre(&world, Cell::new(x, y, 0));
    let p = sharp();
    let e = ear(&p, at(0, 0));
    let (mut known, mut alert, cfg) = memory();
    let mut aware = Awareness::default();
    let mut lost = false;
    for now in 1..=3 {
        // Behind the observer from tick 2 on.
        let point = if now == 1 { at(3, 0) } else { at(-3, 0) };
        let mut out = Vec::new();
        let t = person(point, 1);
        observe(&world, &mut OpenAir, now, &e, &t, &mut aware, &mut out);
        lost |= out.iter().any(|p| p.event == PerceptKind::Lost);
        learn_percepts(&mut known, &mut alert, &cfg, now, &out, &kinds(), &quiet());
        let fact = known.get(7, SEEN).unwrap();
        assert_eq!((fact.pos, fact.seen_at), ([3, 0, 0], 1));
    }
    assert!(lost);
    assert!(known.get(7, NOTICED).is_none());
}

#[test]
fn facts_are_whole_cells() {
    let world = World::square(S);
    let p = sharp();
    let e = ear(&p, cell_centre(&world, Cell::new(-2, 2, 0)));
    let mut aware = Awareness::default();
    let mut out = Vec::new();
    let t = person([300, 700, 0], 1);
    observe(&world, &mut OpenAir, 1, &e, &t, &mut aware, &mut out);
    let seen = out.iter().find(|p| p.event == PerceptKind::Seen).unwrap();
    assert_eq!(seen.point, [300, 700, 0]);
    let fact = percept_fact(seen, &kinds()).unwrap();
    assert_eq!((fact.kind, fact.pos), (SEEN, [1, 2, 0]));
    // A heard percept: its whole cell, the margin as strength.
    let mut heard = Vec::new();
    let s = stimulus([300, 700, 0], 8 * S);
    hear(&world, &mut OpenAir, 1, &e, &s, None, None, &mut heard, 0);
    let fact = percept_fact(&heard[0], &kinds()).unwrap();
    assert_eq!(
        (fact.kind, fact.pos, fact.source),
        (HEARD, [1, 2, 0], SOURCE_HEARD)
    );
    assert_eq!(fact.strength, heard[0].margin as u32);
    // Lost, Forgotten and SeenThrough stand for no fact.
    for event in [
        PerceptKind::Lost,
        PerceptKind::Forgotten,
        PerceptKind::SeenThrough,
    ] {
        assert_eq!(percept_fact(&Percept { event, ..*seen }, &kinds()), None);
    }
}

#[test]
fn unplaced_percepts_learn_nothing() {
    let p = Percept {
        observer: 1,
        subject: 7,
        sense: Sense::Sight,
        event: PerceptKind::Noticed,
        stage: 1,
        point: [0; 3],
        cell: [0; 3],
        placed: false,
        cues: 0,
        disguised: false,
        clarity: 0,
        margin: 0,
        arrive_at: 0,
        at: 5,
    };
    assert_eq!(percept_fact(&p, &kinds()), None);
    let (mut known, mut alert, cfg) = memory();
    let old = Fact {
        subject: 7,
        kind: NOTICED,
        pos: [4, 4, 0],
        seen_at: 2,
        ..Fact::default()
    };
    known.learn(2, old);
    let recognized = Percept {
        event: PerceptKind::Recognized,
        stage: 2,
        ..p
    };
    let seen = Percept {
        event: PerceptKind::Seen,
        ..recognized
    };
    let all = [p, recognized, seen];
    learn_percepts(&mut known, &mut alert, &cfg, 5, &all, &kinds(), &quiet());
    assert_eq!(known.get(7, NOTICED), Some(&old));
    assert_eq!(known.get(7, SEEN), None);
}

#[test]
fn disguised_fact_kind_or_skip() {
    let world = World::square(S);
    let at = |x, y| cell_centre(&world, Cell::new(x, y, 0));
    let p = sharp();
    let e = ear(&p, at(0, 0));
    let spy = Target {
        disguise: 100,
        ..person(at(3, 0), 1)
    };
    let mut aware = Awareness::default();
    let mut out = Vec::new();
    observe(&world, &mut OpenAir, 1, &e, &spy, &mut aware, &mut out);
    assert!(!out.is_empty() && out.iter().all(|p| p.disguised));
    let amounts = AlertAmounts {
        seen: 50,
        ..quiet()
    };
    // No disguised kind: nothing learnt, no alert.
    let (mut known, mut alert, cfg) = memory();
    learn_percepts(&mut known, &mut alert, &cfg, 1, &out, &kinds(), &amounts);
    assert!(known.get(7, NOTICED).is_none() && known.get(7, SEEN).is_none());
    assert_eq!(alert.value(), 0);
    // With one: the facts go there.
    let with = FactKinds {
        disguised_kind: Some(DISGUISED),
        ..kinds()
    };
    learn_percepts(&mut known, &mut alert, &cfg, 1, &out, &with, &amounts);
    assert!(known.get(7, NOTICED).is_none() && known.get(7, SEEN).is_none());
    assert_eq!(known.get(7, DISGUISED).unwrap().pos, [3, 0, 0]);
    assert_eq!(alert.value(), 50);
}

#[test]
fn alert_from_percepts() {
    let world = World::square(S);
    let at = |x, y| cell_centre(&world, Cell::new(x, y, 0));
    let p = sharp();
    let e = ear(&p, at(0, 0));
    let amounts = AlertAmounts {
        noticed: 10,
        recognized: 20,
        seen: 1,
        heard: 5,
        cue_bits: [1, 2, 0, 0, 0, 0, 0, 0],
        cue_amount: [7, 100, 0, 0, 0, 0, 0, 0],
    };
    let mut aware = Awareness::default();
    let mut out = Vec::new();
    let t = person(at(3, 0), 1);
    observe(&world, &mut OpenAir, 1, &e, &t, &mut aware, &mut out);
    let events: Vec<_> = out.iter().map(|p| p.event).collect();
    assert_eq!(
        events,
        [
            PerceptKind::Noticed,
            PerceptKind::Recognized,
            PerceptKind::Cues { new: 1 },
            PerceptKind::Seen
        ]
    );
    let each: Vec<_> = out.iter().map(|p| alert_amount(p, &amounts)).collect();
    assert_eq!(each, [10, 20, 7, 1]);
    let (mut known, mut alert, cfg) = memory();
    learn_percepts(&mut known, &mut alert, &cfg, 1, &out, &kinds(), &amounts);
    assert_eq!(alert.value(), 38);
    assert_eq!(alert.update(1, &cfg), Some((0, 1)));
    // A heard stimulus adds its amount.
    let mut heard = Vec::new();
    let s = stimulus(at(2, 0), 8 * S);
    hear(&world, &mut OpenAir, 2, &e, &s, None, None, &mut heard, 0);
    assert_eq!(alert_amount(&heard[0], &amounts), 5);
    learn_percepts(&mut known, &mut alert, &cfg, 2, &heard, &kinds(), &amounts);
    assert_eq!(alert.value(), 43);
    assert_eq!(known.get(9, HEARD).unwrap().pos, [2, 0, 0]);
}

#[test]
fn disguised_noise_learns_no_threat() {
    let world = World::square(S);
    let at = |x, y| cell_centre(&world, Cell::new(x, y, 0));
    let p = SenseProfile {
        hear_gain: 4096,
        ..SenseProfile::human(S)
    };
    let e = ear(&p, at(0, 0));
    let s = Stimulus {
        disguise: 100,
        ..stimulus(at(2, 0), 8 * S)
    };
    let mut aware = Awareness::default();
    let mut out = Vec::new();
    hear(
        &world,
        &mut OpenAir,
        1,
        &e,
        &s,
        None,
        Some(&mut aware),
        &mut out,
        0,
    );
    let events: Vec<_> = out.iter().map(|p| p.event).collect();
    assert_eq!(events, [PerceptKind::Heard, PerceptKind::Noticed]);
    assert!(out.iter().all(|p| p.disguised));
    let amounts = AlertAmounts {
        noticed: 10,
        heard: 5,
        ..quiet()
    };
    // No disguised kind: no fact at all, no alert.
    let (mut known, mut alert, cfg) = memory();
    learn_percepts(&mut known, &mut alert, &cfg, 1, &out, &kinds(), &amounts);
    for kind in [NOTICED, SEEN, HEARD, DISGUISED] {
        assert!(known.get(9, kind).is_none(), "kind {kind}");
    }
    assert_eq!(alert.value(), 0);
    // One percept at a time: no fact either.
    assert!(out.iter().all(|p| percept_fact(p, &kinds()).is_none()));
    // With one: the noticed fact and the noise both go there.
    let with = FactKinds {
        disguised_kind: Some(DISGUISED),
        ..kinds()
    };
    let heard = percept_fact(&out[0], &with).unwrap();
    assert_eq!((heard.kind, heard.source), (DISGUISED, SOURCE_HEARD));
    learn_percepts(&mut known, &mut alert, &cfg, 1, &out, &with, &amounts);
    assert!(known.get(9, NOTICED).is_none());
    assert!(known.get(9, HEARD).is_none());
    assert_eq!(known.get(9, DISGUISED).unwrap().pos, [2, 0, 0]);
}
