//! `sx_grid`: a dense grid with levels owned by the library, for games
//! without their own map structure. [`sx_grid_map`] gives the `sx_map` that
//! every search takes.

use std::ffi::c_void;

use sagax_nav::grid::{Cell, CellMap, DenseGrid};

use crate::{ERROR_ARGUMENT, Point, SxMap, create, destroy, to_i32};

/// `sx_grid`.
pub struct SxGrid(DenseGrid);

impl SxGrid {
    /// The grid itself.
    pub(crate) fn dense(&self) -> &DenseGrid {
        &self.0
    }
}

/// The grid behind a handle, or `None` for null.
///
/// # Safety
/// `grid` is null or from [`sx_grid_new`] and not freed.
unsafe fn grid_of<'a>(grid: *mut SxGrid) -> Option<&'a mut DenseGrid> {
    // SAFETY: see above.
    unsafe { grid.as_mut() }.map(|g| &mut g.0)
}

/// `sx_grid_new`: `width` x `height` open cells on each of `levels` levels
/// (extra cost 0, no walls). Null if any size is 0 or less.
#[unsafe(no_mangle)]
pub extern "C" fn sx_grid_new(width: i32, height: i32, levels: i32) -> *mut SxGrid {
    create(|| {
        (width > 0 && height > 0 && levels > 0)
            .then(|| SxGrid(DenseGrid::with_levels(width, height, levels)))
    })
}

/// `sx_grid_free`.
///
/// # Safety
/// `grid` is null or from [`sx_grid_new`] and not used again (also not
/// through a map from [`sx_grid_map`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_free(grid: *mut SxGrid) {
    // SAFETY: see above.
    unsafe { destroy(grid) }
}

/// `sx_grid_set`: the extra cost of entering `cell`; a negative cost blocks
/// it. Returns 0, or `SX_ERROR_ARGUMENT` for a null grid or a cell off the
/// grid.
///
/// # Safety
/// `grid` is null or from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_set(grid: *mut SxGrid, cell: Point, cost: i32) -> i32 {
    // SAFETY: see above.
    let Some(g) = (unsafe { grid_of(grid) }) else {
        return ERROR_ARGUMENT;
    };
    let cell = cell.cell();
    if g.get_at(cell).is_none() {
        return ERROR_ARGUMENT;
    }
    g.set_at(cell, u32::try_from(cost).unwrap_or(DenseGrid::BLOCKED));
    0
}

/// `sx_grid_add_wall`: a wall between `cell` and its neighbour `dx`, `dy`
/// away on the same level, both ways. Returns 0, or `SX_ERROR_ARGUMENT` for
/// a null grid, a cell off the grid or an offset that is no neighbour.
///
/// # Safety
/// `grid` is null or from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_add_wall(grid: *mut SxGrid, cell: Point, dx: i32, dy: i32) -> i32 {
    // SAFETY: see above.
    match unsafe { grid_of(grid) }.map(|g| g.add_wall_at(cell.cell(), dx, dy)) {
        Some(true) => 0,
        _ => ERROR_ARGUMENT,
    }
}

/// `sx_grid_link`: a move from `a` toward `b` arrives on `b`'s level, and
/// back. Returns 1, or 0 (nothing changed) unless `a` and `b` are
/// neighbours on the grid; `SX_ERROR_ARGUMENT` for a null grid.
///
/// # Safety
/// `grid` is null or from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_link(grid: *mut SxGrid, a: Point, b: Point) -> i32 {
    // SAFETY: see above.
    match unsafe { grid_of(grid) } {
        Some(g) => i32::from(g.link(a.cell(), b.cell())),
        None => ERROR_ARGUMENT,
    }
}

/// `sx_grid_map`: the map of the grid for the searches, written to `out`.
/// Returns 0, or `SX_ERROR_ARGUMENT` for a null pointer.
///
/// # Safety
/// `grid` is null or from [`sx_grid_new`]; `out` is null or valid. The map
/// is valid while the grid lives.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_map(grid: *mut SxGrid, out: *mut SxMap) -> i32 {
    // SAFETY: see above.
    let Some(out) = (unsafe { out.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    if grid.is_null() {
        return ERROR_ARGUMENT;
    }
    *out = SxMap {
        step: Some(grid_step),
        start_step: None,
        move_cost: Some(grid_move_cost),
        steps: None,
        context: grid.cast(),
    };
    0
}

/// The `step` callback of [`sx_grid_map`].
///
/// # Safety
/// `context` is a live grid from [`sx_grid_new`].
unsafe extern "C" fn grid_step(
    context: *mut c_void,
    x: i32,
    y: i32,
    level: i32,
    dx: i32,
    dy: i32,
) -> i32 {
    // SAFETY: see above; the search holds no other reference to the grid.
    let g = unsafe { &mut (*context.cast::<SxGrid>()).0 };
    g.step(Cell::new(x, y, level), dx, dy).unwrap_or(-1)
}

/// The `move_cost` callback of [`sx_grid_map`].
///
/// # Safety
/// As [`grid_step`].
unsafe extern "C" fn grid_move_cost(
    context: *mut c_void,
    x: i32,
    y: i32,
    level: i32,
    to_x: i32,
    to_y: i32,
    to_level: i32,
) -> i32 {
    // SAFETY: see above.
    let g = unsafe { &mut (*context.cast::<SxGrid>()).0 };
    g.move_cost(Cell::new(x, y, level), Cell::new(to_x, to_y, to_level))
        .map_or(-1, to_i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SxResult, SxRules, sx_nav_find_path, sx_nav_free, sx_nav_new, sx_rules_default};

    /// The map of `examples/c/quickstart.c`.
    const MAP: [&str; 8] = [
        "....................",
        "..######............",
        ".......#....#####...",
        ".......#........#...",
        "..S....#....G...#...",
        ".......#........#...",
        ".......######...#...",
        "....................",
    ];

    unsafe extern "C" fn quickstart_step(
        _: *mut c_void,
        x: i32,
        y: i32,
        level: i32,
        dx: i32,
        dy: i32,
    ) -> i32 {
        let (nx, ny) = (x + dx, y + dy);
        let open = (0..20).contains(&nx)
            && (0..8).contains(&ny)
            && MAP[ny as usize].as_bytes()[nx as usize] != b'#';
        if open { level } else { -1 }
    }

    fn rules(preset: u32) -> SxRules {
        let mut rules = SxRules::default();
        assert_eq!(unsafe { sx_rules_default(0, preset, &mut rules) }, 0);
        rules
    }

    /// The way and its result, as `SX_OUT_CELLS`.
    fn way(map: &SxMap, rules: &SxRules, start: Point, goal: Point) -> (Vec<Point>, SxResult) {
        let nav = unsafe { sx_nav_new(rules) };
        assert!(!nav.is_null());
        let mut points = [Point::default(); 64];
        let mut result = SxResult::default();
        let n = unsafe {
            sx_nav_find_path(
                nav,
                map,
                start,
                &goal,
                1,
                0,
                0,
                points.as_mut_ptr(),
                64,
                &mut result,
            )
        };
        unsafe { sx_nav_free(nav) };
        assert!(n >= 0);
        (points[..n as usize].to_vec(), result)
    }

    #[test]
    fn c_grid_same_way_as_callbacks() {
        let callbacks = SxMap {
            step: Some(quickstart_step),
            start_step: None,
            move_cost: None,
            steps: None,
            context: std::ptr::null_mut(),
        };
        let grid = sx_grid_new(20, 8, 1);
        assert!(!grid.is_null());
        for (y, row) in MAP.iter().enumerate() {
            for (x, c) in row.bytes().enumerate() {
                if c == b'#' {
                    let cell = Point::new(x as i32, y as i32, 0);
                    assert_eq!(unsafe { sx_grid_set(grid, cell, -1) }, 0);
                }
            }
        }
        let mut map = SxMap {
            step: None,
            start_step: None,
            move_cost: None,
            steps: None,
            context: std::ptr::null_mut(),
        };
        assert_eq!(unsafe { sx_grid_map(grid, &mut map) }, 0);
        let rules = rules(1);
        let (start, goal) = (Point::new(2, 4, 0), Point::new(12, 4, 0));
        let expected = way(&callbacks, &rules, start, goal);
        assert_eq!(expected.1.status, crate::FOUND);
        assert_eq!(way(&map, &rules, start, goal), expected);
        unsafe { sx_grid_free(grid) };
    }

    #[test]
    fn c_grid_levels() {
        let grid = sx_grid_new(5, 1, 2);
        unsafe {
            assert_eq!(sx_grid_set(grid, Point::new(2, 0, 0), -1), 0);
            for x in [0, 1, 3, 4] {
                assert_eq!(sx_grid_set(grid, Point::new(x, 0, 1), -1), 0);
            }
            assert_eq!(sx_grid_set(grid, Point::new(5, 0, 0), 0), ERROR_ARGUMENT);
            assert_eq!(
                sx_grid_link(grid, Point::new(1, 0, 0), Point::new(2, 0, 1)),
                1
            );
            assert_eq!(
                sx_grid_link(grid, Point::new(2, 0, 1), Point::new(3, 0, 0)),
                1
            );
            assert_eq!(
                sx_grid_link(grid, Point::new(0, 0, 0), Point::new(2, 0, 1)),
                0
            );
            assert_eq!(
                sx_grid_link(grid, Point::new(1, 0, 0), Point::new(2, 0, 2)),
                0
            );
        }
        let mut map = SxMap {
            step: None,
            start_step: None,
            move_cost: None,
            steps: None,
            context: std::ptr::null_mut(),
        };
        assert_eq!(unsafe { sx_grid_map(grid, &mut map) }, 0);
        let (cells, result) = way(&map, &rules(2), Point::new(0, 0, 0), Point::new(4, 0, 0));
        assert_eq!(result.status, crate::FOUND);
        assert_eq!(result.cost, 4);
        assert_eq!(
            cells.iter().map(|c| c.level).collect::<Vec<_>>(),
            [0, 0, 1, 0, 0]
        );
        unsafe { sx_grid_free(grid) };
    }

    #[test]
    fn c_grid_link_extreme_coordinates() {
        let grid = sx_grid_new(5, 1, 2);
        unsafe {
            assert_eq!(
                sx_grid_link(grid, Point::new(i32::MIN, 0, 0), Point::new(1, 0, 1)),
                0
            );
            assert_eq!(
                sx_grid_link(grid, Point::new(1, 0, 0), Point::new(i32::MIN, 0, 1)),
                0
            );
            assert_eq!(
                sx_grid_link(grid, Point::new(i32::MAX, 0, 0), Point::new(i32::MIN, 0, 1)),
                0
            );
            sx_grid_free(grid);
        }
    }

    #[test]
    fn c_grid_walls_and_bad_arguments() {
        assert!(sx_grid_new(0, 1, 1).is_null());
        assert!(sx_grid_new(1, 0, 1).is_null());
        assert!(sx_grid_new(1, 1, 0).is_null());
        let null = std::ptr::null_mut();
        let mut map = SxMap {
            step: None,
            start_step: None,
            move_cost: None,
            steps: None,
            context: null,
        };
        unsafe {
            assert_eq!(
                sx_grid_set(null.cast(), Point::default(), 0),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_grid_map(null.cast(), &mut map), ERROR_ARGUMENT);
            assert_eq!(
                sx_grid_link(null.cast(), Point::default(), Point::new(1, 0, 0)),
                ERROR_ARGUMENT
            );
        }
        // a wall on level 1 between x = 0 and 1: the walker goes round on y = 1
        let grid = sx_grid_new(3, 2, 2);
        unsafe {
            assert_eq!(sx_grid_map(grid, std::ptr::null_mut()), ERROR_ARGUMENT);
            assert_eq!(
                sx_grid_add_wall(grid, Point::new(0, 0, 1), 2, 0),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_grid_add_wall(grid, Point::new(0, 0, 1), 1, 0), 0);
            assert_eq!(sx_grid_add_wall(grid, Point::new(0, 0, 1), 1, 1), 0);
            assert_eq!(sx_grid_map(grid, &mut map), 0);
        }
        let rules = rules(2);
        let (cells, _) = way(&map, &rules, Point::new(0, 0, 1), Point::new(2, 0, 1));
        assert!(cells.len() > 3, "goes round: {cells:?}");
        let (cells, _) = way(&map, &rules, Point::new(0, 0, 0), Point::new(2, 0, 0));
        assert_eq!(cells.len(), 3, "level 0 has no wall");
        unsafe { sx_grid_free(grid) };
    }
}
