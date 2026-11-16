//! `FlowDistance` equals the path search from the walker to the
//! destination, also where moves cost differently in the two directions:
//! the cell entered, leave costs of zones, one-way walls, levels; square and
//! hex. And the edges: a walker on a blocked cell, a destination that cannot
//! be entered.

use sagax::nav::grid::Moves;
use sagax::prelude::*;

/// Small deterministic random numbers for the maps.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as u32) % n
    }
}

fn octile() -> Topology {
    Topology::Square(GridRules::preset(Preset::Octile))
}

/// The raw cost of the way a path search finds from `from` to `to`.
fn path_cost(map: &mut DenseGrid, topology: Topology, from: Pos, to: Pos) -> Option<u32> {
    let mut nav = Navigator::new(topology, 10_000);
    let report = nav.find_path(map, Cell::from(from), &[Cell::from(to)], Budget::UNLIMITED);
    (report.status == NavStatus::Found).then_some(report.cost)
}

/// In cells (straight cost 10), as `FlowDistance` gives it.
fn path_cells(map: &mut DenseGrid, topology: Topology, from: Pos, to: Pos) -> Option<u32> {
    path_cost(map, topology, from, to).map(|c| c / 10)
}

/// A row of ten cells, the destination at (5, 0), the walker at (0, 0).
fn row() -> DenseGrid {
    DenseGrid::new(10, 1)
}

#[test]
fn the_reviewers_row() {
    let (from, to) = ([0, 0, 0], [5, 0, 0]);
    // The destination stands in a zone (x 3..=7) that costs 50 to leave:
    // walking in costs nothing extra, 5 cells.
    let mut map = row();
    for x in 3..=7 {
        map.set_zones(x, 0, 1);
    }
    map.set_leave_cost(50);
    let mut fd = FlowDistance::new(octile(), 4, 100);
    assert_eq!(fd.distance(&mut map, from, to), Some(5));
    // Back out of the zone it is 10.
    assert_eq!(fd.distance(&mut map, to, from), Some(10));
    assert_eq!(path_cells(&mut map, octile(), to, from), Some(10));

    // The walker in a swamp (entering it costs 90): it only leaves it.
    let mut map = row();
    map.set(0, 0, 90);
    let mut fd = FlowDistance::new(octile(), 4, 100);
    assert_eq!(fd.distance(&mut map, from, to), Some(5));
    // The destination in the swamp: the last step costs 100.
    let mut map = row();
    map.set(5, 0, 90);
    let mut fd = FlowDistance::new(octile(), 4, 100);
    assert_eq!(fd.distance(&mut map, from, to), Some(14));
    assert_eq!(path_cells(&mut map, octile(), from, to), Some(14));

    // The walker stands on a blocked cell (occupied by itself).
    let mut map = row();
    map.set(0, 0, DenseGrid::BLOCKED);
    let mut fd = FlowDistance::new(octile(), 4, 100);
    assert_eq!(fd.distance(&mut map, from, to), Some(5));
    assert_eq!(path_cells(&mut map, octile(), from, to), Some(5));

    // Standing there: 0, without a search, whatever the capacity.
    let mut fd = FlowDistance::new(octile(), 4, 0);
    assert_eq!(fd.distance(&mut row(), to, to), Some(0));
    assert_eq!(fd.searches(), 0);
}

#[test]
fn one_way_walls() {
    // A one-way wall between x 4 and 5: from 4 one cannot step to 5, from
    // 5 one can step to 4. On a single row the way 0 -> 9 is cut, 9 -> 0
    // is open.
    let mut map = row();
    map.set_wall_bits(4, 0, 1); // bit 0: the +x direction
    let mut fd = FlowDistance::new(octile(), 4, 100);
    assert_eq!(fd.distance(&mut map, [0, 0, 0], [9, 0, 0]), None);
    assert_eq!(fd.distance(&mut map, [9, 0, 0], [0, 0, 0]), Some(9));
    assert_eq!(path_cells(&mut map, octile(), [0, 0, 0], [9, 0, 0]), None);
}

/// A random map: blocked cells, enter costs, a zone with a leave cost and
/// one-way walls.
fn random_map(rng: &mut Rng, w: i32, h: i32, one_way: bool) -> DenseGrid {
    let mut map = DenseGrid::new(w, h);
    for y in 0..h {
        for x in 0..w {
            match rng.below(10) {
                0 => map.set(x, y, DenseGrid::BLOCKED),
                1 | 2 => map.set(x, y, rng.below(60)),
                _ => {}
            }
            if rng.below(6) == 0 {
                map.set_zones(x, y, 1 << rng.below(2));
            }
            if one_way && rng.below(8) == 0 {
                map.set_wall_bits(x, y, 1 << rng.below(8));
            }
        }
    }
    map.set_leave_cost(35);
    map
}

/// Every cell of the map to a few open destinations: the same as the path
/// search, in cells.
fn equals_the_path_search(topology: Topology, one_way: bool, seed: u64) {
    let (w, h) = (12, 9);
    let mut rng = Rng(seed);
    let mut map = random_map(&mut rng, w, h, one_way);
    let mut fd = FlowDistance::new(topology, 2, 1000);
    let mut checked = 0;
    for to in [[6, 4, 0], [0, 0, 0], [11, 8, 0], [3, 7, 0]] {
        if map.get_at(Cell::from(to)) == Some(DenseGrid::BLOCKED) {
            continue;
        }
        for y in 0..h {
            for x in 0..w {
                let from = [x, y, 0];
                let want = path_cells(&mut map, topology, from, to);
                assert_eq!(fd.distance(&mut map, from, to), want, "{from:?} -> {to:?}");
                checked += usize::from(want.is_some());
            }
        }
    }
    assert!(checked > 100, "{checked}");
}

#[test]
fn square_maps_equal_the_path_search() {
    for seed in 1..=4 {
        equals_the_path_search(octile(), true, seed);
        let mut four = GridRules::preset(Preset::Octile);
        four.moves = Moves::Four;
        equals_the_path_search(Topology::Square(four), true, seed);
    }
}

#[test]
fn hex_maps_equal_the_path_search() {
    // Hex wall bits are per hex direction; the map's one-way walls are
    // square ones, so hex gets costs, zones and blocked cells.
    for layout in [HexLayout::OddR, HexLayout::EvenQ] {
        for seed in 1..=3 {
            equals_the_path_search(Topology::hex(layout, Preset::Octile), false, seed);
        }
    }
}

#[test]
fn a_destination_nobody_can_enter_is_walked_up_to() {
    // The enemy's cell is blocked (occupied): the path search calls it
    // unreachable; the walking distance is the way next to it plus one.
    let mut map = DenseGrid::new(10, 10);
    for y in 0..=8 {
        map.set(5, y, DenseGrid::BLOCKED);
    }
    let to = [7, 3, 0];
    map.set(7, 3, DenseGrid::BLOCKED);
    let mut fd = FlowDistance::new(octile(), 4, 1000);
    for from in [[4, 0, 0], [9, 9, 0], [7, 4, 0], [8, 2, 0]] {
        assert_eq!(path_cost(&mut map, octile(), from, to), None);
        let beside = (6..=8)
            .flat_map(|x| (2..=4).map(move |y| [x, y, 0]))
            .filter(|&n| n != to)
            .filter_map(|n| path_cost(&mut map, octile(), from, n))
            .min()
            .unwrap();
        assert_eq!(
            fd.distance(&mut map, from, to),
            Some(beside / 10 + 1),
            "{from:?}"
        );
    }
    assert_eq!(fd.searches(), 1);
    // Walled in on every side: no way up to it either.
    let mut map = DenseGrid::new(10, 10);
    for (x, y) in [
        (2, 2),
        (3, 2),
        (4, 2),
        (2, 3),
        (4, 3),
        (2, 4),
        (3, 4),
        (4, 4),
    ] {
        map.set(x, y, DenseGrid::BLOCKED);
    }
    map.set(3, 3, DenseGrid::BLOCKED);
    fd.invalidate();
    assert_eq!(fd.distance(&mut map, [8, 8, 0], [3, 3, 0]), None);
}

#[test]
fn stairs_need_their_levels() {
    // Two levels; a stair from (4, 0, 1) down to (5, 0, 0). The walker is
    // upstairs, the destination downstairs.
    let mut map = DenseGrid::with_levels(10, 1, 2);
    assert!(map.link(Cell::new(4, 0, 1), Cell::new(5, 0, 0)));
    let (from, to) = ([0, 0, 1], [9, 0, 0]);
    let want = path_cells(&mut map, octile(), from, to);
    assert_eq!(want, Some(9));
    let mut fd = FlowDistance::new(octile(), 4, 100).with_levels(0..=1);
    assert_eq!(fd.distance(&mut map, from, to), want);
    // With the default levels (0..=0) a move from level 1 is not seen
    // backwards.
    let mut flat = FlowDistance::new(octile(), 4, 100);
    assert_eq!(flat.distance(&mut map, from, to), None);
}
