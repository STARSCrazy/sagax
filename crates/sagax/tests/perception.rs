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
