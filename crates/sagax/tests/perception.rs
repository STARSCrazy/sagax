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
    let line = sight_line(&world, &mut GridSight(&map), at(0, 0), at(5, 0));
    assert!(!line.clear);
    assert_eq!(line.blocked_at, Some(Cell::new(3, 0, 0)));
    assert!(sight_line(&world, &mut GridSight(&map), at(0, 2), at(5, 2)).clear);
    // Off the grid counts as opaque; costs other than BLOCKED do not.
    let mut map = walled();
    map.set(1, 2, 50);
    assert!(sight_line(&world, &mut GridSight(&map), at(0, 2), at(5, 2)).clear);
    assert!(GridSight(&map).sight(Cell::new(-1, 0, 0)).opaque);
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
    let mut sight = MoveBlocks(OneWall);
    assert!(!sight.sight_edge(Cell::new(1, 0, 0), Cell::new(2, 0, 0)));
    assert!(sight.sight_edge(Cell::new(1, 1, 0), Cell::new(2, 1, 0)));
    let line = sight_line(&world, &mut sight, at(0, 0), at(4, 0));
    assert_eq!(
        (line.clear, line.blocked_at),
        (false, Some(Cell::new(2, 0, 0)))
    );
    assert!(sight_line(&world, &mut sight, at(0, 1), at(4, 1)).clear);
    // A DenseGrid wall is a blocked step too.
    let mut grid = MoveBlocks(walled());
    assert!(!sight_line(&world, &mut grid, at(0, 0), at(5, 0)).clear);
    assert!(sight_line(&world, &mut grid, at(0, 2), at(5, 2)).clear);
    // A step that changes the level (stairs) does not carry sight.
    struct Stairs;
    impl CellMap for Stairs {
        fn step(&mut self, from: Cell, _: i32, _: i32) -> Option<i32> {
            Some(from.level + 1)
        }
    }
    assert!(!MoveBlocks(Stairs).sight_edge(Cell::new(0, 0, 0), Cell::new(1, 0, 0)));
}
