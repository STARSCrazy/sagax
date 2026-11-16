//! `FlowDistance`: walking distances from flow fields, the cache by
//! destination, square and hex, and a crew that walks.

use sagax::prelude::*;

const SEED: u64 = 7;

/// 10 x 10, a wall at x = 5 for y 0..=8 (open at y = 9).
fn walled() -> DenseGrid {
    let mut map = DenseGrid::new(10, 10);
    for y in 0..=8 {
        map.set(5, y, DenseGrid::BLOCKED);
    }
    map
}

fn octile() -> Topology {
    Topology::Square(GridRules::preset(Preset::Octile))
}

/// The cost of the way from `from` to `to` as a path search finds it, in
/// cells (straight cost 10).
fn path_cells(map: &mut DenseGrid, topology: Topology, from: Pos, to: Pos) -> Option<u32> {
    let mut nav = Navigator::new(topology, 1000);
    let report = nav.find_path(map, Cell::from(from), &[Cell::from(to)], Budget::UNLIMITED);
    (report.status == NavStatus::Found).then_some(report.cost / 10)
}

#[test]
fn distance_is_the_walking_cost_in_cells() {
    let mut map = walled();
    let mut fd = FlowDistance::new(octile(), 4, 100);
    for from in [[4, 0, 0], [0, 0, 0], [9, 9, 0], [6, 1, 0], [3, 7, 0]] {
        let to = [6, 0, 0];
        assert_eq!(
            fd.distance(&mut map, from, to),
            path_cells(&mut map, octile(), from, to),
            "{from:?}"
        );
    }
    // Ten straight steps are ten cells; a diagonal adds 1.4 to the sum.
    let mut fd = FlowDistance::new(octile(), 4, 400);
    let mut open = DenseGrid::new(20, 20);
    assert_eq!(fd.distance(&mut open, [0, 0, 0], [10, 0, 0]), Some(10));
    assert_eq!(fd.distance(&mut open, [0, 5, 0], [5, 0, 0]), Some(7));
    assert_eq!(fd.distance(&mut open, [3, 3, 0], [3, 3, 0]), Some(0));
}

#[test]
fn unreachable_is_none() {
    let mut map = walled();
    // Close the gap: the two sides are cut off.
    map.set(5, 9, DenseGrid::BLOCKED);
    let mut fd = FlowDistance::new(octile(), 4, 100);
    assert_eq!(fd.distance(&mut map, [4, 0, 0], [6, 0, 0]), None);
    assert_eq!(fd.distance(&mut map, [7, 0, 0], [6, 0, 0]), Some(1));
    // A figure on a blocked cell (its own, occupied) still walks off it.
    assert_eq!(fd.distance(&mut map, [5, 0, 0], [6, 0, 0]), Some(1));
    // Beyond the capacity of a field counts as no way.
    let mut tiny = FlowDistance::new(octile(), 1, 3);
    let mut open = DenseGrid::new(20, 20);
    assert_eq!(tiny.distance(&mut open, [19, 19, 0], [0, 0, 0]), None);
}

#[test]
fn repeat_destination_runs_no_new_search() {
    let mut map = walled();
    let mut fd = FlowDistance::new(octile(), 4, 100);
    let to = [6, 0, 0];
    for x in 0..5 {
        fd.distance(&mut map, [x, 0, 0], to);
    }
    assert_eq!(fd.searches(), 1);
    assert!(fd.cached(to));
    assert!(!fd.cached([7, 0, 0]));
}

#[test]
fn least_recently_used_is_replaced() {
    let mut map = DenseGrid::new(10, 10);
    let mut fd = FlowDistance::new(octile(), 2, 100);
    let (a, b, c) = ([1, 1, 0], [2, 2, 0], [3, 3, 0]);
    let from = [0, 0, 0];
    fd.distance(&mut map, from, a);
    fd.distance(&mut map, from, b);
    // `a` was used again, so `b` is the oldest when `c` comes.
    fd.distance(&mut map, from, a);
    fd.distance(&mut map, from, c);
    assert_eq!(fd.searches(), 3);
    assert!(fd.cached(a) && fd.cached(c) && !fd.cached(b));
    // Back to `b`: a search again, and `a` goes.
    fd.distance(&mut map, from, b);
    assert_eq!(fd.searches(), 4);
    assert!(!fd.cached(a));
}

#[test]
fn invalidate_drops_every_field() {
    let mut map = walled();
    let mut fd = FlowDistance::new(octile(), 4, 100);
    let (from, to) = ([4, 0, 0], [6, 0, 0]);
    let round = fd.distance(&mut map, from, to);
    // Open a door in the wall: the cached field still knows the old map.
    map.set(5, 0, 0);
    assert_eq!(fd.distance(&mut map, from, to), round);
    fd.invalidate();
    assert!(!fd.cached(to));
    assert_eq!(fd.distance(&mut map, from, to), Some(2));
    assert_eq!(fd.searches(), 2);
}

#[test]
fn hex_distance_in_offset_cells() {
    let layout = HexLayout::OddR;
    let topology = Topology::hex(layout, Preset::Octile);
    let mut map = DenseGrid::new(12, 12);
    let mut fd = FlowDistance::new(topology, 4, 200);
    // On an open hex map the walk is the hex distance.
    for (from, to) in [
        ([0, 0, 0], [5, 0, 0]),
        ([2, 3, 0], [7, 8, 0]),
        ([1, 9, 0], [8, 2, 0]),
    ] {
        let (a, b) = (layout.to_axial_pos(from), layout.to_axial_pos(to));
        let dx = i64::from(b[0]) - i64::from(a[0]);
        let dy = i64::from(b[1]) - i64::from(a[1]);
        let hex = sagax::decide::memory::distance(Metric::Hex, dx, dy);
        assert_eq!(
            fd.distance(&mut map, from, to).map(i128::from),
            Some(hex),
            "{from:?} {to:?}"
        );
    }
    // A wall across the map (every cell of column 5 but the last row).
    for y in 0..11 {
        map.set(5, y, DenseGrid::BLOCKED);
    }
    fd.invalidate();
    let round = fd.distance(&mut map, [4, 0, 0], [6, 0, 0]).unwrap();
    assert!(round > 10, "{round}");
    assert_eq!(
        Some(round),
        path_cells(&mut map, topology, [4, 0, 0], [6, 0, 0])
    );
}

/// The path-cost scene: a crew whose one slot (max 1, for roles 1) goes
/// to the leader's place (6, 0): the leader 1 has no role, member 2 stands
/// at (4, 0) behind the wall, member 3 at (6, 6) on the open side.
fn crew() -> Crew {
    let mut crew = Crew::new(CrewConfig {
        squad: SquadConfig {
            members: 4,
            memory: 4,
            areas: 1,
            inbox: 4,
            metric: Metric::Chebyshev,
            ..SquadConfig::default()
        },
        ..CrewConfig::default()
    })
    .unwrap();
    for (id, x, y, roles) in [(1, 6, 0, 0), (2, 4, 0, 1), (3, 6, 6, 1)] {
        let m = Member {
            id,
            pos: [x, y, 0],
            roles,
            skill: 255,
            state: MemberState::Active,
        };
        assert!(crew.add_member(m).is_some());
    }
    let sq = crew.squad_mut();
    assert!(sq.set_leader(1));
    sq.set_skill(255);
    sq.set_target_kind(1);
    let mut t = tactics::rush();
    t.slots[0].roles = 1;
    t.slots[0].min = 1;
    t.slots[0].max = 1;
    t.slots[0].place = sagax::decide::group::Place::Here;
    sq.add_tactic(t);
    crew.observe(
        0,
        Fact {
            subject: 10,
            kind: 1,
            pos: [9, 0, 0],
            seen_at: 0,
            ..Fact::default()
        },
    );
    crew
}

#[test]
fn crew_with_flow_distance_sends_the_one_who_can_walk_there() {
    let ordered = |fd: Option<&mut FlowDistance>| {
        let mut crew = crew();
        let mut map = walled();
        let mut events = Vec::new();
        let mut ids = Vec::new();
        let mut fd = fd;
        for t in 0..=12 {
            match fd.as_deref_mut() {
                Some(fd) => {
                    let mut walk = fd.on(&mut map);
                    crew.tick_with(t, &[], &[], SEED, &mut Walking::new(&mut walk), &mut events)
                        .unwrap();
                }
                None => crew.tick(t, &[], &[], SEED, &mut events).unwrap(),
            }
            for e in &events {
                if let CrewEvent::Order { member, .. } = e {
                    ids.push(*member);
                }
            }
        }
        ids
    };
    assert_eq!(ordered(None), [2]);
    let mut fd = FlowDistance::new(octile(), FlowDistance::DEFAULT_SLOTS, 100);
    assert_eq!(ordered(Some(&mut fd)), [3]);
    // Thirteen ticks, a handful of destinations (the target, the wish
    // point): far fewer searches than questions.
    assert!(fd.searches() <= 4, "{}", fd.searches());
}
