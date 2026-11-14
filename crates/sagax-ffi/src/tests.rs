#![allow(deprecated)]

use super::*;
use sagax_abi::codes::{IDLE, PARTIAL};
use sagax_nav::nav::ANY_LEVEL;

/// A 20 x 20 map, one level; context points to a list of blocked cells.
struct Walls {
    blocked: Vec<(i32, i32)>,
    calls: usize,
}

fn open(w: &Walls, x: i32, y: i32) -> bool {
    (0..20).contains(&x) && (0..20).contains(&y) && !w.blocked.contains(&(x, y))
}

unsafe extern "C" fn step(ctx: *mut c_void, x: i32, y: i32, level: i32, dx: i32, dy: i32) -> i32 {
    let w = unsafe { &mut *ctx.cast::<Walls>() };
    w.calls += 1;
    if open(w, x + dx, y + dy) { level } else { -1 }
}

unsafe extern "C" fn steps(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    level: i32,
    _from_start: i32,
    count: i32,
    offsets: *const i32,
    levels: *mut i32,
) {
    let w = unsafe { &mut *ctx.cast::<Walls>() };
    w.calls += 1;
    for i in 0..count as usize {
        let (dx, dy) = unsafe { (*offsets.add(2 * i), *offsets.add(2 * i + 1)) };
        let level = if open(w, x + dx, y + dy) { level } else { -1 };
        unsafe { levels.add(i).write(level) };
    }
}

fn walls(blocked: &[(i32, i32)]) -> Walls {
    Walls {
        blocked: blocked.to_vec(),
        calls: 0,
    }
}

fn map_of(w: &mut Walls) -> SxMap {
    SxMap {
        step: Some(step),
        start_step: None,
        move_cost: None,
        steps: None,
        context: (w as *mut Walls).cast(),
    }
}

fn rules(grid: u32) -> SxRules {
    SxRules {
        grid,
        flags: 0,
        cost_straight: 10,
        cost_diagonal: 14,
        node_capacity: 4096,
        smooth_max_extra: 0,
    }
}

fn p(x: i32, y: i32) -> Point {
    Point::new(x, y, 0)
}

/// A wall at x = 10 from y = 0 to 14.
fn wall_line() -> Walls {
    walls(&(0..15).map(|y| (10, y)).collect::<Vec<_>>())
}

#[test]
fn version_is_10() {
    assert_eq!(sx_api_version(), 10);
}

#[test]
fn bad_rules_give_null() {
    let mut r = rules(0);
    r.cost_straight = 0;
    assert!(unsafe { sx_nav_new(&r) }.is_null());
    let mut r = rules(9);
    assert!(unsafe { sx_nav_new(&r) }.is_null());
    r.grid = 0;
    r.node_capacity = 0;
    assert!(unsafe { sx_flow_new(&r) }.is_null());
    assert!(unsafe { sx_dstar_new(std::ptr::null()) }.is_null());
}

#[test]
fn rules_default_presets() {
    let header = include_str!("../include/sagax.h");
    for (name, value) in [("SX_PRESET_OCTILE", "1u"), ("SX_PRESET_TABLETOP", "2u")] {
        assert!(
            header
                .lines()
                .any(|l| l.split_whitespace().take(3).eq(["#define", name, value])),
            "{name} is {value} in sagax.h"
        );
    }
    let mut r = SxRules::default();
    assert_eq!(unsafe { sx_rules_default(0, 1, &mut r) }, 0);
    assert_eq!(
        (r.grid, r.flags, r.cost_straight, r.cost_diagonal),
        (0, 0, 10, 14)
    );
    assert_eq!((r.node_capacity, r.smooth_max_extra), (32768, 0));
    assert_eq!(unsafe { sx_rules_default(1, 2, &mut r) }, 0);
    assert_eq!((r.grid, r.cost_straight), (1, 1));
    let nav = unsafe { sx_nav_new(&r) };
    assert!(!nav.is_null(), "preset rules are valid rules");
    unsafe { sx_nav_free(nav) };
    assert_eq!(unsafe { sx_rules_default(0, 0, &mut r) }, ERROR_ARGUMENT);
    assert_eq!(unsafe { sx_rules_default(0, 3, &mut r) }, ERROR_ARGUMENT);
    assert_eq!(unsafe { sx_rules_default(5, 1, &mut r) }, ERROR_ARGUMENT);
    assert_eq!(
        unsafe { sx_rules_default(0, 1, std::ptr::null_mut()) },
        ERROR_ARGUMENT
    );
}

#[test]
fn continue_on_fresh_nav_is_idle() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut points = [Point::default(); 4];
    let mut result = SxResult {
        status: -1,
        ..SxResult::default()
    };
    let n = unsafe {
        sx_nav_continue(
            nav,
            &map,
            0,
            points.as_mut_ptr(),
            std::ptr::null_mut(),
            4,
            &mut result,
        )
    };
    assert_eq!(n, 0);
    assert_eq!(result.status, IDLE);
    unsafe { sx_nav_free(nav) };
}

#[test]
fn find_path_writes_every_output_from_the_start() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut points = [Point::default(); 32];
    let mut result = SxResult::default();
    let n = unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(0, 0),
            &Point::new(5, 2, ANY_LEVEL),
            1,
            0,
            1,
            points.as_mut_ptr(),
            32,
            &mut result,
        )
    };
    assert_eq!(&points[..n as usize], [p(0, 0), p(2, 2), p(5, 2)]);
    assert_eq!(result.status, FOUND);
    assert_eq!(result.cost, 2 * 14 + 3 * 10);
    assert_eq!(result.cells, 6);
    let n = unsafe { sx_nav_way(nav, &map, 0, points.as_mut_ptr(), 32) };
    assert_eq!(n, 6);
    assert_eq!(points[0], p(0, 0));
    let n = unsafe { sx_nav_way(nav, &map, 2, points.as_mut_ptr(), 32) };
    assert_eq!(&points[..n as usize], [p(0, 0), p(5, 2)]);
    // cut lists still count the whole way
    let n = unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(0, 0),
            &p(5, 2),
            1,
            0,
            0,
            points.as_mut_ptr(),
            2,
            &mut result,
        )
    };
    assert_eq!((n, result.cells), (2, 6));
    unsafe { sx_nav_free(nav) };
}

#[test]
fn start_is_goal_is_found_with_one_cell() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut points = [Point::default(); 4];
    let mut result = SxResult::default();
    let n = unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(3, 3),
            &p(3, 3),
            1,
            0,
            1,
            points.as_mut_ptr(),
            4,
            &mut result,
        )
    };
    assert_eq!((n, result.status, result.cells), (1, FOUND, 1));
    unsafe { sx_nav_free(nav) };
}

#[test]
fn small_budgets_continue_to_the_same_way() {
    let mut w = wall_line();
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut whole = [Point::default(); 64];
    let mut expected = SxResult::default();
    let n_whole = unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(2, 2),
            &p(18, 2),
            1,
            0,
            1,
            whole.as_mut_ptr(),
            64,
            &mut expected,
        )
    };
    let mut points = [Point::default(); 64];
    let mut result = SxResult::default();
    let mut n = unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(2, 2),
            &p(18, 2),
            1,
            5,
            1,
            points.as_mut_ptr(),
            64,
            &mut result,
        )
    };
    let mut calls = 1;
    while result.status == RUNNING {
        n = unsafe {
            sx_nav_continue(
                nav,
                &map,
                5,
                points.as_mut_ptr(),
                std::ptr::null_mut(),
                64,
                &mut result,
            )
        };
        calls += 1;
    }
    assert!(calls > 5);
    assert_eq!(result, expected);
    assert_eq!(points[..n as usize], whole[..n_whole as usize]);
    unsafe { sx_nav_free(nav) };
}

#[test]
fn closed_off_goal_gives_partial_way() {
    let mut w = walls(&(0..20).map(|y| (10, y)).collect::<Vec<_>>());
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut points = [Point::default(); 32];
    let mut result = SxResult::default();
    let n = unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(2, 5),
            &p(18, 5),
            1,
            0,
            1,
            points.as_mut_ptr(),
            32,
            &mut result,
        )
    };
    assert_eq!(result.status, PARTIAL);
    assert_eq!(points[n as usize - 1], p(9, 5));
    unsafe { sx_nav_free(nav) };
}

#[test]
fn reachable_then_path_to() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut cells = [Point::default(); 64];
    let mut costs = [0i32; 64];
    let mut result = SxResult::default();
    let n = unsafe {
        sx_nav_reachable(
            nav,
            &map,
            p(5, 5),
            10,
            0,
            cells.as_mut_ptr(),
            costs.as_mut_ptr(),
            64,
            &mut result,
        )
    };
    assert_eq!((n, result.status, result.cells), (5, DONE, 5));
    assert_eq!((cells[0], costs[0]), (p(5, 5), 0));
    let mut points = [Point::default(); 8];
    let n = unsafe { sx_nav_path_to(nav, &map, p(6, 5), 0, points.as_mut_ptr(), 8, &mut result) };
    assert_eq!(&points[..n as usize], [p(5, 5), p(6, 5)]);
    assert_eq!((result.status, result.cost), (FOUND, 10));
    let n = unsafe { sx_nav_path_to(nav, &map, p(9, 9), 0, points.as_mut_ptr(), 8, &mut result) };
    assert_eq!((n, result.status), (0, UNREACHABLE));
    unsafe { sx_nav_free(nav) };
}

#[test]
fn batch_callback_gives_the_same_way_with_fewer_calls() {
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut ways = Vec::new();
    for batch in [false, true] {
        let mut w = wall_line();
        let mut map = map_of(&mut w);
        if batch {
            map.steps = Some(steps);
        }
        let mut points = [Point::default(); 64];
        let n = unsafe {
            sx_nav_find_path(
                nav,
                &map,
                p(2, 2),
                &p(18, 2),
                1,
                0,
                0,
                points.as_mut_ptr(),
                64,
                std::ptr::null_mut(),
            )
        };
        ways.push((points[..n as usize].to_vec(), w.calls));
    }
    assert_eq!(ways[0].0, ways[1].0);
    assert!(ways[1].1 * 4 < ways[0].1, "{} vs {}", ways[1].1, ways[0].1);
    unsafe { sx_nav_free(nav) };
}

#[test]
fn formation_writes_leader_and_members() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut points = [Point::default(); 16];
    unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(2, 5),
            &p(8, 5),
            1,
            0,
            0,
            points.as_mut_ptr(),
            16,
            std::ptr::null_mut(),
        )
    };
    let mut table = [Point::default(); 4 * 16];
    let n = unsafe { sx_nav_formation(nav, &map, std::ptr::null(), 3, table.as_mut_ptr(), 16) };
    assert_eq!(n, 7);
    assert_eq!(table[0], p(2, 5));
    assert_eq!(table[6], p(8, 5));
    let end: Vec<Point> = (0..4).map(|row| table[row * 16 + 6]).collect();
    for (i, a) in end.iter().enumerate() {
        assert!(end[i + 1..].iter().all(|b| b != a), "{end:?}");
    }
    assert_eq!(
        unsafe { sx_nav_formation(nav, &map, std::ptr::null(), 2, table.as_mut_ptr(), 16) },
        ERROR_ARGUMENT
    );
    unsafe { sx_nav_free(nav) };
}

#[test]
fn flow_serves_many_walkers() {
    let mut w = wall_line();
    let map = map_of(&mut w);
    let flow = unsafe { sx_flow_new(&rules(0)) };
    let goals = [p(18, 2)];
    let mut result = SxResult::default();
    assert_eq!(
        unsafe { sx_flow_build(flow, &map, goals.as_ptr(), 1, 100_000, &mut result) },
        0
    );
    assert_eq!(result.status, DONE);
    assert!(result.cells > 300);
    let mut walkers = [p(2, 2), p(18, 2), p(5, 10)];
    let mut costs = [0i32; 3];
    let mut next = [Point::default(); 3];
    let moved = unsafe {
        sx_flow_next(
            flow,
            &map,
            walkers.as_ptr(),
            3,
            next.as_mut_ptr(),
            costs.as_mut_ptr(),
        )
    };
    assert_eq!(moved, 2);
    assert_eq!(next[1], p(18, 2));
    assert_eq!(costs[1], 0);
    // walking down the field always gets cheaper and ends at the goal
    for _ in 0..100 {
        let mut before = [0i32; 3];
        unsafe {
            sx_flow_next(
                flow,
                &map,
                walkers.as_ptr(),
                3,
                next.as_mut_ptr(),
                before.as_mut_ptr(),
            )
        };
        walkers = next;
        unsafe {
            sx_flow_next(
                flow,
                &map,
                walkers.as_ptr(),
                3,
                std::ptr::null_mut(),
                costs.as_mut_ptr(),
            )
        };
        assert!((0..3).all(|i| costs[i] <= before[i]));
    }
    assert!(walkers.iter().all(|&w| w == p(18, 2)));
    unsafe { sx_flow_free(flow) };
}

#[test]
fn dstar_repairs_after_a_change() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let d = unsafe { sx_dstar_new(&rules(0)) };
    assert_eq!(
        unsafe { sx_dstar_begin(d, p(2, 5), Point::new(18, 5, ANY_LEVEL)) },
        ERROR_ARGUMENT
    );
    assert_eq!(unsafe { sx_dstar_begin(d, p(2, 5), p(18, 5)) }, 0);
    let mut points = [Point::default(); 32];
    let mut result = SxResult::default();
    let n = unsafe { sx_dstar_compute(d, &map, 0, 1, points.as_mut_ptr(), 32, &mut result) };
    assert_eq!(&points[..n as usize], [p(2, 5), p(18, 5)]);
    assert_eq!((result.status, result.cost), (FOUND, 160));

    w.blocked = (0..15).map(|y| (10, y)).collect();
    let changed: Vec<Point> = w.blocked.iter().map(|&(x, y)| p(x, y)).collect();
    let map = map_of(&mut w);
    assert!(unsafe { sx_dstar_cells_changed(d, &map, changed.as_ptr(), changed.len() as i32) } > 0);
    let n = unsafe { sx_dstar_compute(d, &map, 0, 0, points.as_mut_ptr(), 32, &mut result) };
    assert_eq!(result.status, FOUND);
    assert!(
        points[..n as usize]
            .iter()
            .all(|q| !(q.x == 10 && q.y < 15))
    );
    assert_eq!(points[n as usize - 1], p(18, 5));
    unsafe { sx_dstar_free(d) };
}

#[test]
fn batched_steps_match_single_steps_hex() {
    for grid in 1..=4 {
        let nav = unsafe { sx_nav_new(&rules(grid)) };
        let mut ways = Vec::new();
        for batch in [false, true] {
            let mut w = wall_line();
            let mut map = map_of(&mut w);
            if batch {
                map.steps = Some(steps);
            }
            let mut points = [Point::default(); 64];
            let mut result = SxResult::default();
            let n = unsafe {
                sx_nav_find_path(
                    nav,
                    &map,
                    p(2, 2),
                    &p(18, 2),
                    1,
                    0,
                    0,
                    points.as_mut_ptr(),
                    64,
                    &mut result,
                )
            };
            assert_eq!(result.status, FOUND, "grid {grid}");
            ways.push((points[..n as usize].to_vec(), result.cost, w.calls));
        }
        assert_eq!(ways[0].0, ways[1].0, "grid {grid}");
        assert_eq!(ways[0].1, ways[1].1, "grid {grid}");
        assert!(ways[1].2 * 4 < ways[0].2, "grid {grid}");
        unsafe { sx_nav_free(nav) };
    }
}

#[test]
fn hex_grids_work_everywhere() {
    for grid in 1..=4 {
        let mut w = wall_line();
        let map = map_of(&mut w);
        let nav = unsafe { sx_nav_new(&rules(grid)) };
        let mut points = [Point::default(); 64];
        let mut result = SxResult::default();
        let n = unsafe {
            sx_nav_find_path(
                nav,
                &map,
                p(2, 2),
                &p(18, 2),
                1,
                0,
                2,
                points.as_mut_ptr(),
                64,
                &mut result,
            )
        };
        assert_eq!(result.status, FOUND, "grid {grid}");
        assert_eq!((points[0], points[n as usize - 1]), (p(2, 2), p(18, 2)));
        let mut table = [Point::default(); 4 * 64];
        let wedge = [0, -1, -1, 1, 0, -2];
        assert!(
            unsafe { sx_nav_formation(nav, &map, wedge.as_ptr(), 3, table.as_mut_ptr(), 64) } > 0
        );
        unsafe { sx_nav_free(nav) };

        let d = unsafe { sx_dstar_new(&rules(grid)) };
        unsafe { sx_dstar_begin(d, p(2, 2), p(18, 2)) };
        unsafe { sx_dstar_compute(d, &map, 0, 0, points.as_mut_ptr(), 64, &mut result) };
        assert_eq!(result.status, FOUND, "grid {grid}");
        unsafe { sx_dstar_free(d) };
    }
}

unsafe extern "C" fn record(ctx: *mut c_void, event: *const SxEvent) {
    let events = unsafe { &mut *ctx.cast::<Vec<SxEvent>>() };
    events.push(unsafe { *event });
}

#[test]
fn tracing_reports_steps_and_turns_off() {
    let mut w = wall_line();
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut events: Vec<SxEvent> = Vec::new();
    let ctx = (&mut events as *mut Vec<SxEvent>).cast();
    assert_eq!(unsafe { sx_nav_set_trace(nav, Some(record), ctx) }, 0);
    let mut points = [Point::default(); 64];
    let mut result = SxResult::default();
    unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(2, 2),
            &p(18, 2),
            1,
            0,
            1,
            points.as_mut_ptr(),
            64,
            &mut result,
        )
    };
    let expanded = events.iter().filter(|e| e.kind == 2).count();
    assert_eq!(expanded as i32, result.expanded);
    assert_eq!((events[0].kind, events[0].cell), (1, p(2, 2)));
    let last = *events.last().unwrap();
    assert_eq!((last.kind, last.found), (5, 1));

    unsafe { sx_nav_set_trace(nav, None, std::ptr::null_mut()) };
    let before = events.len();
    unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(2, 2),
            &p(18, 2),
            1,
            0,
            1,
            points.as_mut_ptr(),
            64,
            &mut result,
        )
    };
    assert_eq!(events.len(), before);
    unsafe { sx_nav_free(nav) };

    let d = unsafe { sx_dstar_new(&rules(0)) };
    let mut events: Vec<SxEvent> = Vec::new();
    unsafe { sx_dstar_set_trace(d, Some(record), (&mut events as *mut Vec<SxEvent>).cast()) };
    unsafe { sx_dstar_begin(d, p(2, 2), p(18, 2)) };
    unsafe { sx_dstar_compute(d, &map, 0, 0, points.as_mut_ptr(), 64, &mut result) };
    assert_eq!(result.status, FOUND);
    assert!(events.iter().filter(|e| e.kind == 2).count() > 10);
    assert_eq!(events.last().map(|e| (e.kind, e.found)), Some((5, 1)));
    unsafe { sx_dstar_free(d) };

    let flow = unsafe { sx_flow_new(&rules(0)) };
    let mut events: Vec<SxEvent> = Vec::new();
    unsafe {
        sx_flow_set_trace(
            flow,
            Some(record),
            (&mut events as *mut Vec<SxEvent>).cast(),
        )
    };
    unsafe { sx_flow_build(flow, &map, [p(18, 2)].as_ptr(), 1, 100_000, &mut result) };
    assert_eq!(
        events.iter().filter(|e| e.kind == 2).count() as i32,
        result.cells
    );
    unsafe { sx_flow_free(flow) };
}

#[test]
fn flee_and_several_goals() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut points = [Point::default(); 64];
    let mut result = SxResult::default();
    let threats = [p(3, 5)];
    let n = unsafe {
        sx_nav_flee(
            nav,
            &map,
            p(5, 5),
            threats.as_ptr(),
            1,
            50,
            0,
            points.as_mut_ptr(),
            64,
            &mut result,
        )
    };
    assert_eq!(result.status, FOUND);
    assert_eq!(points[0], p(5, 5));
    assert!(points[n as usize - 1].x >= 9);

    let exits = [Point::new(0, 5, ANY_LEVEL), Point::new(19, 5, ANY_LEVEL)];
    let n = unsafe {
        sx_nav_find_path(
            nav,
            &map,
            p(15, 5),
            exits.as_ptr(),
            2,
            0,
            0,
            points.as_mut_ptr(),
            64,
            &mut result,
        )
    };
    assert_eq!(result.status, FOUND);
    assert_eq!(points[n as usize - 1], p(19, 5));
    unsafe { sx_nav_free(nav) };
}

mod areas_cost {
    use super::*;
    use crate::memory::{
        SxArea, SxAreasCostCtx, sx_areas_add, sx_areas_free, sx_areas_move_cost, sx_areas_new,
    };

    fn circle(kind: u32, x: i32, y: i32, radius: i32, strength: u32, blocking: u8) -> SxArea {
        SxArea {
            kind,
            pos: [x, y, 0],
            radius,
            strength,
            lifetime: 255,
            blocking,
            ..SxArea::default()
        }
    }

    /// The way from (0, 5) to (19, 5) on an open map with `area` in the way:
    /// (cells, cost).
    fn way_with(area: Option<SxArea>) -> (Vec<Point>, i32) {
        let areas = sx_areas_new(4);
        if let Some(a) = area {
            assert_eq!(unsafe { sx_areas_add(areas, 0, &a) }, 0);
        }
        let mut ctx = SxAreasCostCtx {
            areas,
            start: [0, 5, 0],
            goal: [19, 5, 0],
        };
        let mut w = walls(&[]);
        let mut map = map_of(&mut w);
        // Both callbacks share one context: the walls and the adapter's.
        let mut shim = Shim {
            walls: &mut w,
            ctx: &mut ctx,
        };
        map.step = Some(shim_step);
        map.context = (&mut shim as *mut Shim).cast();
        map.move_cost = Some(shim_cost);
        let nav = unsafe { sx_nav_new(&rules(0)) };
        let mut points = [Point::default(); 64];
        let mut result = SxResult::default();
        let n = unsafe {
            sx_nav_find_path(
                nav,
                &map,
                p(0, 5),
                &p(19, 5),
                1,
                0,
                0,
                points.as_mut_ptr(),
                64,
                &mut result,
            )
        };
        assert_eq!(result.status, FOUND);
        unsafe { sx_nav_free(nav) };
        unsafe { sx_areas_free(areas) };
        (points[..n as usize].to_vec(), result.cost)
    }

    /// One context for both callbacks: the walls and the areas adapter.
    struct Shim<'a> {
        walls: &'a mut Walls,
        ctx: &'a mut SxAreasCostCtx,
    }

    unsafe extern "C" fn shim_step(
        c: *mut c_void,
        x: i32,
        y: i32,
        level: i32,
        dx: i32,
        dy: i32,
    ) -> i32 {
        let s = unsafe { &mut *c.cast::<Shim>() };
        unsafe { step((s.walls as *mut Walls).cast(), x, y, level, dx, dy) }
    }

    unsafe extern "C" fn shim_cost(
        c: *mut c_void,
        x: i32,
        y: i32,
        level: i32,
        tx: i32,
        ty: i32,
        tl: i32,
    ) -> i32 {
        let s = unsafe { &mut *c.cast::<Shim>() };
        unsafe {
            sx_areas_move_cost(
                (s.ctx as *mut SxAreasCostCtx).cast(),
                x,
                y,
                level,
                tx,
                ty,
                tl,
            )
        }
    }

    fn inside(cells: &[Point], x: i32, y: i32, r: i32) -> usize {
        cells
            .iter()
            .filter(|c| (c.x - x).pow(2) + (c.y - y).pow(2) < r * r)
            .count()
    }

    #[test]
    fn blocking_area_is_walked_around() {
        let (open, open_cost) = way_with(None);
        assert_eq!(inside(&open, 10, 5, 3), 5);
        let (cells, cost) = way_with(Some(circle(1, 10, 5, 3, 0, 1)));
        assert_eq!(cells.first(), Some(&p(0, 5)));
        assert_eq!(cells.last(), Some(&p(19, 5)));
        assert_eq!(inside(&cells, 10, 5, 3), 0);
        assert!(cost > open_cost);
    }

    #[test]
    fn cost_area_is_paid_or_avoided() {
        let (_, open_cost) = way_with(None);
        // Dear: cheaper to go around.
        let (cells, cost) = way_with(Some(circle(1, 10, 5, 3, 1000, 0)));
        assert_eq!(inside(&cells, 10, 5, 3), 0);
        assert!(cost > open_cost && cost < open_cost + 1000);
        // Cheap: the path goes straight through and its cost includes 2 per cell.
        let (cells, cost) = way_with(Some(circle(1, 10, 5, 3, 2, 0)));
        assert_eq!(inside(&cells, 10, 5, 3), 5);
        assert_eq!(cost, open_cost + 5 * 2);
    }

    #[test]
    fn adapter_edge_cases() {
        let mut ctx = SxAreasCostCtx {
            areas: std::ptr::null_mut(),
            start: [0; 3],
            goal: [0; 3],
        };
        let c = (&mut ctx as *mut SxAreasCostCtx).cast();
        assert_eq!(unsafe { sx_areas_move_cost(c, 0, 0, 0, 1, 0, 0) }, 0);
        assert_eq!(
            unsafe { sx_areas_move_cost(std::ptr::null_mut(), 0, 0, 0, 1, 0, 0) },
            0
        );
        let areas = sx_areas_new(2);
        let mut a = circle(1, 50, 50, 3, u32::MAX, 0);
        assert_eq!(unsafe { sx_areas_add(areas, 0, &a) }, 0);
        a.kind = 2;
        a.pos = [60, 50, 0];
        a.blocking = 1;
        assert_eq!(unsafe { sx_areas_add(areas, 0, &a) }, 1);
        ctx.areas = areas;
        ctx.goal = [0, 0, 0];
        let c = (&mut ctx as *mut SxAreasCostCtx).cast();
        assert_eq!(
            unsafe { sx_areas_move_cost(c, 49, 50, 0, 50, 50, 0) },
            i32::MAX - 1
        );
        assert_eq!(unsafe { sx_areas_move_cost(c, 59, 50, 0, 60, 50, 0) }, -1);
        assert_eq!(unsafe { sx_areas_move_cost(c, 9, 9, 0, 10, 10, 0) }, 0);
        unsafe { sx_areas_free(areas) };
    }
}

mod squad {
    //! `sx_squad_*`, `sx_tactic_*`, `sx_keeper_*`, `sx_gather` (C API 8).

    use crate::decide::SxConsideration;
    use crate::group::*;
    use crate::memory::{
        SxArea, SxFact, SxMessage, sx_areas_at, sx_areas_count, sx_knowledge_count,
    };
    use sagax_abi::codes::*;
    use sagax_decide::group::{
        IN_AREA, IN_EXTRA, IN_LOSSES, IN_OFF_PLAN, IN_PLAN_AGE, IN_RANGED, IN_ROLE_0, IN_ROLE_1,
        IN_TARGET_AGE, IN_TARGET_DIST, IN_TARGET_FRIEND_DIST, MAX_EXTRA_COLUMNS, REPORT_ARRIVED,
        REPORT_ENEMY, REPORT_GAME_FIRST, REPORT_GOAL_BROKEN, ROLE_AREA, ROLE_RANGED, TAG_CHOKE,
        TAG_COVER, TAG_HIDDEN, TAG_HIGH,
    };
    use std::mem::{offset_of, size_of};
    use std::ptr::{null, null_mut};

    const HEADER: &str = include_str!("../include/sagax.h");
    const ENEMY: u32 = 1;
    const E1: u32 = 10;
    const E2: u32 = 11;
    const SEED: u64 = 7;
    /// Ticks per cell the scripted members walk.
    const STEP: u32 = 2;
    /// 1.0 as a tactic weight.
    const ONE_WEIGHT: u32 = 65535;
    const EUCLID: u32 = 0;
    const CHEBYSHEV: u32 = 2;
    /// The default role list as a mask array.
    const ROLES: [u32; 8] = [ROLE_RANGED, ROLE_AREA, 0, 0, 0, 0, 0, 0];

    /// The value of `#define name value` in `sagax.h`.
    pub(super) fn define(name: &str) -> i64 {
        let line = HEADER
            .lines()
            .find(|l| l.split_whitespace().take(2).eq(["#define", name]))
            .unwrap_or_else(|| panic!("{name} missing in sagax.h"));
        let value = line.split_whitespace().nth(2).unwrap();
        let value = value
            .trim_matches(|c| c == '(' || c == ')')
            .trim_end_matches('u');
        match value.strip_prefix("0x") {
            Some(hex) => i64::from_str_radix(hex, 16).unwrap(),
            None => value.parse().unwrap(),
        }
    }

    fn member(id: u32, x: i32, y: i32, roles: u32) -> SxMember {
        SxMember {
            id,
            pos: [x, y, 0],
            roles,
            ..SxMember::default()
        }
    }

    fn spot(x: i32, y: i32, tags: u32) -> SxSpot {
        SxSpot {
            pos: [x, y, 0],
            tags,
        }
    }

    fn enemy(subject: u32, x: i32, y: i32) -> SxFact {
        SxFact {
            subject,
            kind: ENEMY,
            pos: [x, y, 0],
            ..SxFact::default()
        }
    }

    fn candidate(id: u32, faction: u32, x: i32, y: i32, free: u8) -> SxGatherCandidate {
        SxGatherCandidate {
            id,
            faction,
            pos: [x, y, 0],
            free,
        }
    }

    /// A squad of `members` (the first leads) that sees enemies of kind
    /// [`ENEMY`]; the others hear by radio (2 ticks plus 1 per cell).
    fn squad_of(metric: u32, members: &[(u32, i32, i32)]) -> *mut SxSquad {
        let sq = new_squad(SxSquadConfig {
            members: 8,
            memory: 8,
            areas: 4,
            inbox: 8,
            dangers: 8,
            metric,
            role_count: 2,
            role_masks: ROLES,
            rule_columns: 0,
        });
        assert!(!sq.is_null());
        unsafe {
            for (i, &(id, x, y)) in members.iter().enumerate() {
                assert_eq!(sx_squad_add_member(sq, &member(id, x, y, 0)), i as i32);
            }
            assert_eq!(sx_squad_set_leader(sq, members[0].0), 1);
            for &(id, ..) in &members[1..] {
                assert_eq!(sx_squad_set_radio(sq, id, 2, 1), 1);
            }
            assert_eq!(sx_squad_set_target_kind(sq, ENEMY), 0);
        }
        sq
    }

    fn new_squad(config: SxSquadConfig) -> *mut SxSquad {
        unsafe { sx_squad_new(&config) }
    }

    /// `sx_squad_check` without extra columns or spots.
    unsafe fn check(sq: *mut SxSquad, now: u32, id: &mut u32) -> i32 {
        unsafe { sx_squad_check(sq, now, null(), 0, null(), 0, 0, id) }
    }

    #[test]
    fn layouts_and_numbers_match_header() {
        assert_eq!(size_of::<SxMember>(), 24);
        assert_eq!(size_of::<SxSpot>(), 16);
        assert_eq!(size_of::<SxGatherCandidate>(), 24);
        assert_eq!(size_of::<SxPlace>(), 20);
        assert_eq!(size_of::<SxSlot>(), 76);
        assert_eq!(size_of::<SxGoal>(), 68);
        assert_eq!(size_of::<SxOrder>(), 72);
        assert_eq!(size_of::<SxPlan>(), 28);
        assert_eq!(size_of::<SxSkillTable>(), 204);
        assert_eq!(size_of::<SxScoreEvent>(), 36);
        assert_eq!(define("SX_API_VERSION"), 10);
        assert_eq!(define("SX_ERROR_PLAN"), i64::from(ERROR_PLAN));
        assert_eq!(define("SX_IDLE"), i64::from(IDLE));
        let core = [
            ("SX_IN_LOSSES", IN_LOSSES as i64),
            ("SX_IN_OFF_PLAN", IN_OFF_PLAN as i64),
            ("SX_IN_TARGET_AGE", IN_TARGET_AGE as i64),
            ("SX_IN_TARGET_DIST", IN_TARGET_DIST as i64),
            ("SX_IN_ROLE_0", IN_ROLE_0 as i64),
            ("SX_IN_ROLE_1", IN_ROLE_1 as i64),
            ("SX_IN_RANGED", IN_RANGED as i64),
            ("SX_IN_AREA", IN_AREA as i64),
            ("SX_IN_PLAN_AGE", IN_PLAN_AGE as i64),
            ("SX_IN_TARGET_FRIEND_DIST", IN_TARGET_FRIEND_DIST as i64),
            ("SX_IN_EXTRA", IN_EXTRA as i64),
            ("SX_ROLE_RANGED", i64::from(ROLE_RANGED)),
            ("SX_ROLE_AREA", i64::from(ROLE_AREA)),
            ("SX_TAG_COVER", i64::from(TAG_COVER)),
            ("SX_TAG_HIGH", i64::from(TAG_HIGH)),
            ("SX_TAG_HIDDEN", i64::from(TAG_HIDDEN)),
            ("SX_TAG_CHOKE", i64::from(TAG_CHOKE)),
            ("SX_REPORT_GOAL_BROKEN", i64::from(REPORT_GOAL_BROKEN)),
            ("SX_REPORT_ENEMY", i64::from(REPORT_ENEMY)),
            ("SX_REPORT_ARRIVED", i64::from(REPORT_ARRIVED)),
            ("SX_REPORT_GAME_FIRST", i64::from(REPORT_GAME_FIRST)),
        ];
        let ffi = [
            ("SX_MEMBER_ACTIVE", 0),
            ("SX_MEMBER_SUSPENDED", 1),
            ("SX_MEMBER_GONE", 2),
            ("SX_PLACE_NONE", i64::from(PLACE_NONE)),
            ("SX_PLACE_OFFSET", i64::from(PLACE_OFFSET)),
            ("SX_PLACE_SIDE", i64::from(PLACE_SIDE)),
            ("SX_PLACE_BETWEEN", i64::from(PLACE_BETWEEN)),
            ("SX_PLACE_TAGGED", i64::from(PLACE_TAGGED)),
            ("SX_PLACE_HERE", i64::from(PLACE_HERE)),
            ("SX_PLACE_CUSTOM", i64::from(PLACE_CUSTOM)),
            ("SX_SIDE_FRONT", 0),
            ("SX_SIDE_LEFT", 1),
            ("SX_SIDE_RIGHT", 2),
            ("SX_SIDE_REAR", 3),
            ("SX_GOAL_MOVE", i64::from(GOAL_MOVE)),
            ("SX_GOAL_HOLD", i64::from(GOAL_HOLD)),
            ("SX_GOAL_ATTACK", i64::from(GOAL_ATTACK)),
            ("SX_GOAL_WAIT", i64::from(GOAL_WAIT)),
            ("SX_GOAL_RETREAT", i64::from(GOAL_RETREAT)),
            ("SX_GOAL_CUSTOM", i64::from(GOAL_CUSTOM)),
            ("SX_START_NOW", i64::from(START_NOW)),
            ("SX_START_AFTER_TICKS", i64::from(START_AFTER_TICKS)),
            ("SX_START_ON_SIGNAL", i64::from(START_ON_SIGNAL)),
            ("SX_SHAPE_LINE", i64::from(SHAPE_LINE)),
            ("SX_SHAPE_WEDGE", i64::from(SHAPE_WEDGE)),
            ("SX_SHAPE_COLUMN", i64::from(SHAPE_COLUMN)),
            ("SX_SQUAD_KEEP", i64::from(SQUAD_KEEP)),
            ("SX_SQUAD_REPLAN", i64::from(SQUAD_REPLAN)),
            ("SX_SQUAD_SIGNAL", i64::from(SQUAD_SIGNAL)),
            ("SX_SQUAD_NO_PLAN", i64::from(SQUAD_NO_PLAN)),
        ];
        for (name, value) in core.iter().chain(ffi.iter()) {
            assert_eq!(define(name), *value, "{name}");
        }
    }

    #[test]
    fn abi_codes_match_header() {
        let numbers = [
            ("SX_ERROR_ARGUMENT", i64::from(ERROR_ARGUMENT)),
            ("SX_ERROR_INTERNAL", i64::from(ERROR_INTERNAL)),
            ("SX_ERROR_PLAN", i64::from(ERROR_PLAN)),
            ("SX_ERROR_NEEDS_RULES", i64::from(ERROR_NEEDS_RULES)),
            ("SX_FOUR_WAYS", i64::from(FOUR_WAYS)),
            ("SX_CUT_CORNERS", i64::from(CUT_CORNERS)),
            ("SX_IDLE", i64::from(IDLE)),
            ("SX_RUNNING", i64::from(RUNNING)),
            ("SX_FOUND", i64::from(FOUND)),
            ("SX_PARTIAL", i64::from(PARTIAL)),
            ("SX_UNREACHABLE", i64::from(UNREACHABLE)),
            ("SX_DONE", i64::from(DONE)),
            ("SX_GRID_SQUARE", i64::from(GRID_SQUARE)),
            ("SX_GRID_HEX_ODD_R", i64::from(GRID_HEX_ODD_R)),
            ("SX_GRID_HEX_EVEN_R", i64::from(GRID_HEX_EVEN_R)),
            ("SX_GRID_HEX_ODD_Q", i64::from(GRID_HEX_ODD_Q)),
            ("SX_GRID_HEX_EVEN_Q", i64::from(GRID_HEX_EVEN_Q)),
            ("SX_METRIC_EUCLID", i64::from(METRIC_EUCLID)),
            ("SX_METRIC_MANHATTAN", i64::from(METRIC_MANHATTAN)),
            ("SX_METRIC_CHEBYSHEV", i64::from(METRIC_CHEBYSHEV)),
            ("SX_METRIC_HEX", i64::from(METRIC_HEX)),
            ("SX_PICK_BEST", i64::from(PICK_BEST)),
            ("SX_PICK_WEIGHTED", i64::from(PICK_WEIGHTED)),
            ("SX_PICK_UNIFORM", i64::from(PICK_UNIFORM)),
            ("SX_SIDE_FRONT", i64::from(SIDE_FRONT)),
            ("SX_SIDE_LEFT", i64::from(SIDE_LEFT)),
            ("SX_SIDE_RIGHT", i64::from(SIDE_RIGHT)),
            ("SX_SIDE_REAR", i64::from(SIDE_REAR)),
            ("SX_MEMBER_ACTIVE", i64::from(MEMBER_ACTIVE)),
            ("SX_MEMBER_SUSPENDED", i64::from(MEMBER_SUSPENDED)),
            ("SX_MEMBER_GONE", i64::from(MEMBER_GONE)),
            ("SX_CREW_PLANNED", i64::from(CREW_PLANNED)),
            ("SX_CREW_ACTIVATED", i64::from(CREW_ACTIVATED)),
            ("SX_CREW_SIGNAL", i64::from(CREW_SIGNAL)),
            ("SX_CREW_ORDER", i64::from(CREW_ORDER)),
            ("SX_CREW_START", i64::from(CREW_START)),
            ("SX_CREW_ARRIVED", i64::from(CREW_ARRIVED)),
            ("SX_CREW_BROKEN", i64::from(CREW_BROKEN)),
            ("SX_CREW_NO_PLAN", i64::from(CREW_NO_PLAN)),
        ];
        for (name, value) in numbers {
            assert_eq!(define(name), value, "{name}");
        }
        assert_eq!(define("SX_ALL_OPTIONS"), i64::from(ALL_OPTIONS));
        assert_eq!(define("SX_COMBINE_FOLLOW"), i64::from(COMBINE_FOLLOW));
    }

    #[test]
    fn one_codec_per_layer() {
        // The conversions live in `sagax-abi`; neither layer has its own.
        let sources = [
            include_str!("lib.rs"),
            include_str!("bench.rs"),
            include_str!("crew.rs"),
            include_str!("decide.rs"),
            include_str!("grid.rs"),
            include_str!("group.rs"),
            include_str!("machine.rs"),
            include_str!("memory.rs"),
            include_str!("../../sagax-wasm/src/lib.rs"),
            include_str!("../../sagax-wasm/src/crew.rs"),
            include_str!("../../sagax-wasm/src/values.rs"),
        ];
        let gone = [
            "fn metric_of",
            "fn hex_layout",
            "fn kind_of",
            "fn start_of",
            "fn place_of",
            "fn skill_of",
            "fn set_combine",
            "fn status_code",
            "fn to_i32",
        ];
        for source in sources {
            for name in gone {
                assert!(!source.contains(name), "{name}");
            }
        }
    }

    #[test]
    fn field_offsets_match_header() {
        // Mirrors the C layout of sagax.h (and the checks in squad.c).
        let member = [
            offset_of!(SxMember, id),
            offset_of!(SxMember, pos),
            offset_of!(SxMember, roles),
            offset_of!(SxMember, skill),
            offset_of!(SxMember, state),
        ];
        assert_eq!(member, [0, 4, 16, 20, 21]);
        assert_eq!([offset_of!(SxSpot, pos), offset_of!(SxSpot, tags)], [0, 12]);
        let candidate = [
            offset_of!(SxGatherCandidate, id),
            offset_of!(SxGatherCandidate, faction),
            offset_of!(SxGatherCandidate, pos),
            offset_of!(SxGatherCandidate, free),
        ];
        assert_eq!(candidate, [0, 4, 8, 20]);
        let place = [
            offset_of!(SxPlace, kind),
            offset_of!(SxPlace, x),
            offset_of!(SxPlace, y),
            offset_of!(SxPlace, z),
            offset_of!(SxPlace, value),
        ];
        assert_eq!(place, [0, 4, 8, 12, 16]);
        let slot = [
            offset_of!(SxSlot, roles),
            offset_of!(SxSlot, min),
            offset_of!(SxSlot, max),
            offset_of!(SxSlot, place),
            offset_of!(SxSlot, gather),
            offset_of!(SxSlot, kind),
            offset_of!(SxSlot, custom),
            offset_of!(SxSlot, start),
            offset_of!(SxSlot, start_ticks),
            offset_of!(SxSlot, want_tags),
            offset_of!(SxSlot, dist),
            offset_of!(SxSlot, gather_dist),
        ];
        assert_eq!(slot, [0, 4, 5, 8, 28, 48, 52, 56, 60, 64, 68, 72]);
        let goal = [
            offset_of!(SxGoal, kind),
            offset_of!(SxGoal, custom),
            offset_of!(SxGoal, pos),
            offset_of!(SxGoal, then_pos),
            offset_of!(SxGoal, target),
            offset_of!(SxGoal, start),
            offset_of!(SxGoal, start_ticks),
            offset_of!(SxGoal, slot),
            offset_of!(SxGoal, plan),
            offset_of!(SxGoal, issued_at),
            offset_of!(SxGoal, start_delay),
            offset_of!(SxGoal, has_pos),
            offset_of!(SxGoal, unchecked),
            offset_of!(SxGoal, has_then_pos),
            offset_of!(SxGoal, then_unchecked),
            offset_of!(SxGoal, has_target),
        ];
        assert_eq!(
            goal,
            [0, 4, 8, 20, 32, 36, 40, 44, 48, 52, 56, 60, 61, 62, 63, 64]
        );
        assert_eq!(
            [offset_of!(SxOrder, member), offset_of!(SxOrder, goal)],
            [0, 4]
        );
        let plan = [
            offset_of!(SxPlan, id),
            offset_of!(SxPlan, tactic),
            offset_of!(SxPlan, target),
            offset_of!(SxPlan, made_at),
            offset_of!(SxPlan, active_at),
            offset_of!(SxPlan, goal_count),
            offset_of!(SxPlan, has_target),
        ];
        assert_eq!(plan, [0, 4, 8, 12, 16, 20, 24]);
        let table = [
            offset_of!(SxSkillTable, rows),
            offset_of!(SxSkillTable, pick),
            offset_of!(SxSkillTable, pick_top),
            offset_of!(SxSkillTable, think_ticks),
            offset_of!(SxSkillTable, timing_spread),
            offset_of!(SxSkillTable, hold_bonus),
            offset_of!(SxSkillTable, give_up_ticks),
            offset_of!(SxSkillTable, spot_rank_error),
        ];
        assert_eq!(table, [0, 4, 36, 68, 100, 132, 164, 196]);
        assert_eq!(offset_of!(SxScoreEvent, event), 12);
    }

    #[test]
    fn squad_config_default() {
        unsafe {
            let mut c = SxSquadConfig::default();
            assert_eq!(sx_squad_config_default(&mut c), 0);
            assert_eq!(
                (c.members, c.memory, c.areas, c.inbox, c.dangers, c.metric),
                (8, 32, 8, 16, 8, EUCLID)
            );
            assert_eq!(size_of::<SxSquadConfig>(), 64);
            assert_eq!((c.role_count, c.role_masks), (2, ROLES));
            assert_eq!(sx_squad_config_default(null_mut()), ERROR_ARGUMENT);
            let sq = sx_squad_new(&c);
            assert!(!sq.is_null());
            sx_squad_free(sq);
        }
    }

    #[test]
    fn squad_config_roles() {
        unsafe {
            let mut c = SxSquadConfig::default();
            assert_eq!(sx_squad_config_default(&mut c), 0);
            assert_eq!((c.role_count, c.role_masks[..2].to_vec()), (2, vec![1, 2]));
            let sq = sx_squad_new(&c);
            assert_eq!(sx_squad_extra_start(sq), 8);
            sx_squad_free(sq);
            let three = SxSquadConfig {
                role_count: 3,
                role_masks: [1, 2, 4, 0, 0, 0, 0, 0],
                ..c
            };
            let sq = sx_squad_new(&three);
            assert_eq!(sx_squad_extra_start(sq), 9);
            sx_squad_free(sq);
            let none = SxSquadConfig { role_count: 0, ..c };
            let sq = sx_squad_new(&none);
            assert_eq!(sx_squad_extra_start(sq), 8);
            sx_squad_free(sq);
            let eight = SxSquadConfig { role_count: 8, ..c };
            let sq = sx_squad_new(&eight);
            assert_eq!(sx_squad_extra_start(sq), 14);
            sx_squad_free(sq);
            let nine = SxSquadConfig { role_count: 9, ..c };
            assert!(sx_squad_new(&nine).is_null());
            assert_eq!(sx_squad_extra_start(null_mut()), ERROR_ARGUMENT);
        }
    }

    #[test]
    fn typed_setters() {
        unsafe {
            let sq = squad_of(EUCLID, &[(1, 0, 0), (2, 3, 0), (3, 40, 0)]);
            assert_eq!(sx_squad_add_flank(sq, 4, 4), ERROR_ARGUMENT);
            assert_eq!(sx_squad_add_flank(sq, 3, 4), 0);
            assert_eq!(sx_squad_set_shout(sq, 2, 5), 1);
            assert_eq!(sx_squad_set_shout(sq, 99, 3), 0);
            assert_eq!(sx_squad_set_radio(sq, 2, 2, 1), 1);
            assert_eq!(sx_squad_set_silent(sq, 3), 1);
            let mut at = 0;
            assert_eq!(sx_squad_report_arrives_at(sq, 2, 10, &mut at), 1);
            assert_eq!(at, 15, "radio 2 + 1 per cell, 3 cells");
            assert_eq!(sx_squad_report_arrives_at(sq, 1, 10, &mut at), 1);
            assert_eq!(at, 10, "the leader at once");
            assert_eq!(sx_squad_report_arrives_at(sq, 3, 10, &mut at), 0);
            assert_eq!(sx_squad_report_arrives_at(sq, 99, 10, &mut at), 0);
            sx_squad_free(sq);

            let t = sx_tactic_new(ONE_WEIGHT, 0, 1);
            assert_eq!(sx_tactic_trigger_all_arrived(t, 100), 0);
            assert_eq!(sx_tactic_trigger_target_in_radius(t, -1), 0);
            sx_tactic_free(t);
        }
    }

    #[test]
    fn danger_queue_through_c() {
        unsafe {
            let sq = new_squad(SxSquadConfig {
                members: 4,
                memory: 4,
                areas: 4,
                inbox: 4,
                dangers: 1,
                metric: EUCLID,
                role_count: 2,
                role_masks: ROLES,
                rule_columns: 0,
            });
            assert_eq!(sx_squad_add_member(sq, &member(1, 0, 0, 0)), 0);
            assert_eq!(sx_squad_add_member(sq, &member(2, 1, 0, 0)), 1);
            assert_eq!(sx_squad_set_gone(sq, 2), 1);
            assert_eq!(sx_squad_add_fall_back(sq), 0);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            assert_eq!(
                sx_squad_report_danger(sq, 1, [9, 0, 0].as_ptr(), null(), 5),
                1
            );
            assert_eq!(
                sx_squad_report_danger(sq, 1, [8, 0, 0].as_ptr(), null(), 5),
                0
            );
            let mut id = 0;
            assert_eq!(sx_squad_plan(sq, 4, null(), 0, null(), 0, 0, &mut id), 1);
            assert_eq!(sx_areas_count(sx_squad_areas(sq)), 0);
            assert_eq!(sx_squad_plan(sq, 5, null(), 0, null(), 0, 0, &mut id), 1);
            let mut area = SxArea::default();
            assert_eq!(sx_areas_at(sx_squad_areas(sq), 0, &mut area), 1);
            assert_eq!((area.pos, area.has_source, area.data), ([9, 0, 0], 0, 1));
            // Activation shows once.
            let mut plan = 0;
            assert_eq!(sx_squad_take_activated(sq, &mut plan), 0);
            let mut out = [SxOrder::default(); 2];
            assert_eq!(sx_squad_orders_due(sq, 10, out.as_mut_ptr(), 2), 1);
            assert_eq!(sx_squad_take_activated(sq, &mut plan), 1);
            assert_eq!(plan, id);
            assert_eq!(sx_squad_take_activated(sq, &mut plan), 0);
            // A kind 3 message is dropped like any unknown kind.
            let old = SxMessage {
                kind: 3,
                pos: [1, 0, 0],
                sender: 1,
                ..SxMessage::default()
            };
            assert_eq!(sx_squad_report(sq, &old, 0), 1);
            assert_eq!(sx_squad_plan(sq, 11, null(), 0, null(), 0, 0, &mut id), 1);
            assert_eq!(sx_areas_count(sx_squad_areas(sq)), 1);
            sx_squad_free(sq);
        }
    }

    #[test]
    fn removed_names_are_gone() {
        for name in [
            "SX_REPORT_DANGER",
            "SX_TACTIC_",
            "SX_CHANNEL_",
            "SX_TRIGGER_",
            "sx_squad_set_channel(",
            "sx_squad_add_shipped(",
            "sx_tactic_set_trigger(",
        ] {
            assert!(!HEADER.contains(name), "{name} is still in sagax.h");
        }
        assert!(!HEADER.contains("sx_squad_resume"));
    }

    #[test]
    fn members_through_c() {
        unsafe {
            let base = SxSquadConfig {
                members: 8,
                memory: 8,
                areas: 4,
                inbox: 8,
                dangers: 8,
                metric: EUCLID,
                role_count: 2,
                role_masks: ROLES,
                rule_columns: 0,
            };
            for broken in [
                SxSquadConfig { members: 0, ..base },
                SxSquadConfig { memory: 0, ..base },
                SxSquadConfig { areas: 0, ..base },
                SxSquadConfig { inbox: 0, ..base },
                SxSquadConfig { dangers: 0, ..base },
                SxSquadConfig { metric: 4, ..base },
            ] {
                assert!(sx_squad_new(&broken).is_null(), "{broken:?}");
            }
            assert!(sx_squad_new(null()).is_null());
            let sq = new_squad(SxSquadConfig {
                members: 2,
                memory: 4,
                metric: CHEBYSHEV,
                ..base
            });
            assert_eq!(sx_squad_add_member(sq, &member(1, 0, 0, 0)), 0);
            assert_eq!(sx_squad_add_member(sq, &member(1, 5, 5, 0)), -1, "same id");
            let bad = SxMember {
                state: 3,
                ..member(2, 0, 0, 0)
            };
            assert_eq!(sx_squad_add_member(sq, &bad), -1, "no such state");
            assert_eq!(sx_squad_add_member(sq, &member(2, 1, 0, 4)), 1);
            assert_eq!(sx_squad_add_member(sq, &member(3, 0, 0, 0)), -1, "full");
            assert_eq!(sx_squad_member_count(sq), 2);
            assert_eq!(sx_squad_set_pos(sq, 2, 7, 8, 9), 1);
            assert_eq!(sx_squad_set_pos(sq, 9, 7, 8, 9), 0);
            let mut m = SxMember::default();
            assert_eq!(sx_squad_member_at(sq, 1, &mut m), 1);
            assert_eq!((m.id, m.pos, m.roles, m.state), (2, [7, 8, 9], 4, 0));
            assert_eq!(sx_squad_member_at(sq, 2, &mut m), 0);
            assert_eq!(sx_squad_suspend(sq, 2), 1);
            assert_eq!(sx_squad_member_at(sq, 1, &mut m), 1);
            assert_eq!(m.state, 1);
            assert_eq!(sx_squad_reinstate(sq, 2, 0), 1);
            assert_eq!(sx_squad_reinstate(sq, 9, 0), 0);
            assert_eq!(sx_squad_suspend(sq, 9), 0);
            // Leader: the set one while active, else the next active member.
            let mut id = 0;
            assert_eq!(sx_squad_set_leader(sq, 9), 0);
            assert_eq!(sx_squad_set_leader(sq, 1), 1);
            assert_eq!(sx_squad_leader(sq, &mut id), 1);
            assert_eq!(id, 1);
            assert_eq!(sx_squad_set_gone(sq, 1), 1);
            assert_eq!(sx_squad_set_gone(sq, 9), 0);
            assert_eq!(sx_squad_member_at(sq, 0, &mut m), 1);
            assert_eq!(m.state, 2);
            assert_eq!(sx_squad_leader(sq, &mut id), 1);
            assert_eq!(id, 2);
            assert_eq!(sx_squad_remove_member(sq, 2), 1);
            assert_eq!(sx_squad_remove_member(sq, 2), 0);
            assert_eq!(sx_squad_leader(sq, null_mut()), 0);
            assert_eq!(sx_squad_set_skill(sq, 256), ERROR_ARGUMENT);
            assert_eq!(sx_squad_set_skill(sq, 200), 0);
            assert_eq!(sx_squad_skill(sq), 200);
            sx_squad_free(sq);
        }
    }

    #[test]
    fn gather_through_c() {
        let list = [
            candidate(7, 1, 30, 0, 1),
            candidate(4, 1, -3, 0, 1),
            candidate(3, 1, 0, -2, 1),
            candidate(6, 1, 1, 1, 0),
            candidate(5, 2, 1, 0, 1),
            candidate(2, 1, 0, 2, 1),
        ];
        let c = list.as_ptr();
        let mut out = [0u32; 6];
        let o = out.as_mut_ptr();
        unsafe {
            // Nearest first, ties to the lower id; the other faction, the
            // busy and the far candidate are left out.
            assert_eq!(sx_gather(0, 0, 1, 10, CHEBYSHEV, 1, c, 6, o, 3), 3);
            assert_eq!(out[..3], [2, 3, 4]);
            let n = sx_gather(0, 0, 1, 10, CHEBYSHEV, 0, c, 6, o, 6);
            assert_eq!(out[..n as usize], [6, 2, 3, 4]);
            assert_eq!(sx_gather(0, 0, 1, 10, 9, 0, c, 6, o, 6), ERROR_ARGUMENT);
            assert_eq!(
                sx_gather(0, 0, 1, 10, 2, 0, null(), 6, o, 6),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_gather(0, 0, 1, 10, 2, 0, c, 6, null_mut(), 6),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_gather(0, 0, 1, 10, 2, 0, null(), 0, null_mut(), 0), 0);
        }
    }

    #[test]
    fn tactics_through_c() {
        unsafe {
            let sq = squad_of(EUCLID, &[(1, 0, 0), (2, 1, 0)]);
            assert_eq!(sx_squad_add_rush(sq), 0);
            assert_eq!(sx_squad_add_formation(sq, SHAPE_LINE, 2), 1);
            assert_eq!(sx_squad_add_formation(sq, SHAPE_WEDGE, 2), 2);
            assert_eq!(sx_squad_add_formation(sq, SHAPE_COLUMN, 2), 3);
            assert_eq!(sx_squad_add_flank(sq, 2, 4), 4);
            assert_eq!(sx_squad_add_flank(sq, 3, 4), 5);
            assert_eq!(sx_squad_add_ambush(sq), 6);
            assert_eq!(sx_squad_add_focus(sq), 7);
            assert_eq!(sx_squad_add_isolate(sq), 8);
            assert_eq!(sx_squad_add_fall_back(sq), 9);
            assert_eq!(sx_squad_add_formation(sq, 3, 2), ERROR_ARGUMENT);
            assert_eq!(sx_squad_add_flank(sq, 4, 4), ERROR_ARGUMENT);
            assert_eq!(sx_squad_add_flank(sq, 1, 4), ERROR_ARGUMENT);
            assert_eq!(sx_squad_add_rush(null_mut()), ERROR_ARGUMENT);

            // A custom tactic: a slot that gathers at the leader, then holds
            // a choke spot; scored on the first extra column.
            assert!(sx_tactic_new(ONE_WEIGHT, 256, 1).is_null());
            let t = sx_tactic_new(ONE_WEIGHT, 0, 1);
            assert!(!t.is_null());
            let mut slot = SxSlot {
                min: 1,
                max: 2,
                place: SxPlace {
                    kind: PLACE_TAGGED,
                    value: TAG_CHOKE,
                    ..SxPlace::default()
                },
                gather: SxPlace {
                    kind: PLACE_HERE,
                    ..SxPlace::default()
                },
                kind: GOAL_HOLD,
                start: START_AFTER_TICKS,
                start_ticks: 3,
                dist: 1,
                ..SxSlot::default()
            };
            assert_eq!(sx_tactic_add_slot(t, &slot), 0);
            slot.place.kind = PLACE_NONE;
            assert_eq!(sx_tactic_add_slot(t, &slot), -1, "a place is needed");
            slot.place = SxPlace {
                kind: PLACE_SIDE,
                value: 4,
                ..SxPlace::default()
            };
            assert_eq!(sx_tactic_add_slot(t, &slot), -1, "no such side");
            slot.place.value = 3;
            slot.kind = 6;
            assert_eq!(sx_tactic_add_slot(t, &slot), -1, "no such goal kind");
            slot.kind = GOAL_CUSTOM;
            slot.custom = 77;
            slot.start = 3;
            assert_eq!(sx_tactic_add_slot(t, &slot), -1, "no such start");
            slot.start = START_ON_SIGNAL;
            assert_eq!(sx_tactic_add_slot(t, &slot), 1);
            let c = SxConsideration {
                input: IN_EXTRA as u32,
                curve: 0,
                min: 0,
                max: 10,
            };
            assert_eq!(sx_tactic_add_consideration(t, &c), 0);
            let bad = SxConsideration { curve: 99, ..c };
            assert_eq!(sx_tactic_add_consideration(t, &bad), ERROR_ARGUMENT);
            assert_eq!(sx_tactic_add_abort(t, IN_LOSSES as u32, 1), 0);
            assert_eq!(sx_tactic_trigger_none(t), 0);
            assert_eq!(sx_tactic_trigger_target_in_radius(t, 6), 0);
            assert_eq!(sx_tactic_trigger_manual(t), 0);
            assert_eq!(sx_tactic_trigger_all_arrived(t, 9), 0);
            assert_eq!(sx_squad_add_tactic(sq, t), 10);
            assert_eq!(sx_squad_add_tactic(sq, null_mut()), -1);
            // Null squad: the tactic is freed all the same.
            assert_eq!(sx_squad_add_tactic(null_mut(), sx_tactic_new(1, 0, 0)), -1);
            sx_tactic_free(sx_tactic_new(1, 0, 0));
            sx_tactic_free(null_mut());

            // The custom tactic reads column IN_EXTRA, beyond a row without
            // extra columns.
            let (mut tactic, mut input) = (0, 0);
            assert_eq!(sx_squad_validate(sq, &mut tactic, &mut input), ERROR_PLAN);
            assert_eq!((tactic, input), (10, IN_EXTRA as u32));
            let mut id = 0;
            let made = sx_squad_plan(sq, 0, null(), 0, null(), 0, 0, &mut id);
            assert_eq!(made, ERROR_PLAN);
            assert_eq!(check(sq, 0, &mut id), ERROR_PLAN);
            assert_eq!(sx_squad_set_extra_columns(sq, 1), 0);
            assert_eq!(sx_squad_validate(sq, null_mut(), null_mut()), 0);
            let most = MAX_EXTRA_COLUMNS as u32;
            assert_eq!(sx_squad_set_extra_columns(sq, most + 1), ERROR_ARGUMENT);
            assert_eq!(sx_squad_set_extra_columns(sq, u32::MAX), ERROR_ARGUMENT);
            assert_eq!(sx_squad_validate(sq, null_mut(), null_mut()), 0);
            assert_eq!(sx_squad_set_extra_columns(sq, most), 0);
            // More than 255 slots.
            let big = sx_tactic_new(ONE_WEIGHT, 0, 0);
            let mut s = SxSlot {
                place: SxPlace {
                    kind: PLACE_HERE,
                    ..SxPlace::default()
                },
                ..SxSlot::default()
            };
            s.max = 1;
            for i in 0..256 {
                assert_eq!(sx_tactic_add_slot(big, &s), i);
            }
            assert_eq!(sx_squad_add_tactic(sq, big), 11);
            assert_eq!(sx_squad_validate(sq, &mut tactic, &mut input), ERROR_PLAN);
            assert_eq!((tactic, input), (11, u32::MAX));
            sx_squad_free(sq);
        }
    }

    #[test]
    fn plan_check_and_orders_through_c() {
        unsafe {
            // Elite, rush and a 2-side flank, enemy 10 at (20, 0); everyone
            // hears at once.
            let sq = squad_of(EUCLID, &[(1, 0, 0), (2, 1, 0), (3, 0, 1), (4, 1, 1)]);
            for id in 2..=4 {
                assert_eq!(sx_squad_set_radio(sq, id, 0, 0), 1);
            }
            assert_eq!(sx_squad_add_rush(sq), 0);
            assert_eq!(sx_squad_add_flank(sq, 2, 4), 1);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            let mut id = 0;
            let made = sx_squad_plan(sq, 0, null(), 0, null(), 0, 0, &mut id);
            assert_eq!(made, 0, "nothing known yet");
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E1, 20, 0), null_mut()), 0);
            let mut targets = [SxFact::default(); 2];
            assert_eq!(sx_squad_targets(sq, targets.as_mut_ptr(), 2, null_mut()), 1);
            assert_eq!((targets[0].subject, targets[0].source), (E1, 0));
            let mut total = 0u32;
            assert_eq!(sx_squad_targets(sq, null_mut(), 0, &mut total), 0);
            assert_eq!(total, 1);
            assert_eq!(sx_knowledge_count(sx_squad_knowledge(sq)), 1);

            assert_eq!(check(sq, 0, &mut id), SQUAD_REPLAN);
            let mut plan = SxPlan::default();
            let mut goals = [SxOrder::default(); 4];
            assert_eq!(
                sx_squad_pending_plan(sq, &mut plan, goals.as_mut_ptr(), 4),
                1
            );
            assert_eq!((plan.id, plan.tactic, plan.has_target), (id, 1, 1));
            assert_eq!((plan.target, plan.made_at, plan.active_at), (E1, 0, 5));
            assert_eq!(plan.goal_count, 4);
            let mut assigned: Vec<u32> = goals.iter().map(|g| g.member).collect();
            assigned.sort_unstable();
            assert_eq!(assigned, [1, 2, 3, 4]);
            assert!(goals.iter().all(|g| g.goal.plan == id));
            assert_eq!(sx_squad_active_plan(sq, &mut plan, null_mut(), 0), 0);
            assert_eq!(check(sq, 5, &mut id), SQUAD_KEEP);
            assert_eq!(sx_squad_active_plan(sq, &mut plan, null_mut(), 0), 1);
            assert_eq!(sx_squad_pending_plan(sq, &mut plan, null_mut(), 0), 0);

            // Four gather orders; a buffer of 3 leaves one for the next call.
            let mut out = [SxOrder::default(); 4];
            assert_eq!(sx_squad_orders_due(sq, 5, out.as_mut_ptr(), 3), 3);
            assert_eq!(sx_squad_orders_due(sq, 5, out[3..].as_mut_ptr(), 1), 1);
            assert_eq!(sx_squad_orders_due(sq, 5, null_mut(), 0), 0);
            let mut members: Vec<u32> = out.iter().map(|o| o.member).collect();
            members.sort_unstable();
            assert_eq!(members, [1, 2, 3, 4]);
            for o in &out {
                // Gather first: go there now, only move.
                let g = &o.goal;
                assert_eq!((g.kind, g.start, g.plan), (GOAL_MOVE, START_NOW, id));
                assert_eq!((g.has_pos, g.has_then_pos, g.has_target), (1, 1, 1));
                assert_eq!(g.target, E1);
            }

            // Arrivals fire the signal; once per plan.
            for (m, t) in [(4, 6), (2, 7), (3, 8)] {
                assert_eq!(sx_squad_arrived(sq, m, t), 1);
                assert_eq!(check(sq, t, &mut id), SQUAD_KEEP);
            }
            assert_eq!(sx_squad_arrived(sq, 99, 8), 0);
            assert_eq!(sx_squad_signal_given(sq), 0);
            assert_eq!(sx_squad_arrived(sq, 1, 9), 1);
            assert_eq!(check(sq, 9, &mut id), SQUAD_SIGNAL);
            assert_eq!(sx_squad_signal_given(sq), 1);
            assert_eq!(sx_squad_signal(sq, 10), 0);
            let mut attack = [SxOrder::default(); 8];
            assert_eq!(sx_squad_orders_due(sq, 9, attack.as_mut_ptr(), 8), 4);
            for o in &attack[..4] {
                let before = out.iter().find(|f| f.member == o.member).unwrap();
                assert_eq!(o.goal.pos, before.goal.then_pos);
                assert_eq!(
                    (o.goal.kind, o.goal.start, o.goal.issued_at),
                    (GOAL_ATTACK, START_NOW, 9)
                );
            }

            // Two members report their goal broken: the flank's abort rule
            // fires once the reports are in and rush is planned.
            let broken = |sender| SxMessage {
                kind: REPORT_GOAL_BROKEN,
                sender,
                data: id,
                ..SxMessage::default()
            };
            assert_eq!(sx_squad_report(sq, &broken(2), 11), 1);
            assert_eq!(sx_squad_report(sq, &broken(3), 11), 1);
            assert_eq!(check(sq, 10, &mut id), SQUAD_KEEP);
            let mut next = 0;
            assert_eq!(check(sq, 11, &mut next), SQUAD_REPLAN);
            assert!(next > id);
            assert_eq!(sx_squad_pending_plan(sq, &mut plan, null_mut(), 0), 1);
            assert_eq!(plan.tactic, 0);
            sx_squad_free(sq);
        }
    }

    /// The elite flank of `plan_check_and_orders_through_c`, active at 5,
    /// everyone hearing at once.
    unsafe fn active_flank() -> (*mut SxSquad, u32) {
        let sq = squad_of(EUCLID, &[(1, 0, 0), (2, 1, 0), (3, 0, 1), (4, 1, 1)]);
        let mut id = 0;
        unsafe {
            for m in 2..=4 {
                assert_eq!(sx_squad_set_radio(sq, m, 0, 0), 1);
            }
            assert_eq!(sx_squad_add_rush(sq), 0);
            assert_eq!(sx_squad_add_flank(sq, 2, 4), 1);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E1, 20, 0), null_mut()), 0);
            assert_eq!(check(sq, 0, &mut id), SQUAD_REPLAN);
            assert_eq!(check(sq, 5, &mut id), SQUAD_KEEP);
        }
        (sq, id)
    }

    /// A one-slot tactic over a squad with one enemy at distance 20.
    unsafe fn custom_squad(tactic: *mut SxTactic) -> *mut SxSquad {
        let sq = squad_of(EUCLID, &[(1, 0, 0)]);
        unsafe {
            let slot = SxSlot {
                max: 1,
                place: SxPlace {
                    kind: PLACE_HERE,
                    ..SxPlace::default()
                },
                ..SxSlot::default()
            };
            assert_eq!(sx_tactic_add_slot(tactic, &slot), 0);
            assert_eq!(sx_squad_add_tactic(sq, tactic), 0);
            assert_eq!(sx_squad_set_extra_columns(sq, 1), 0);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E1, 20, 0), null_mut()), 0);
        }
        sq
    }

    #[test]
    fn abort_when_codes() {
        unsafe {
            let t = sx_tactic_new(ONE_WEIGHT, 0, 1);
            assert_eq!(sx_tactic_add_abort_when(t, 0, 4, 1), ERROR_ARGUMENT);
            assert_eq!(sx_tactic_add_abort_when(null_mut(), 0, 0, 1), -1);
            sx_tactic_free(t);
            // Column IN_EXTRA holds 5 (a block for no target, one for E1). A
            // candidate whose rule fires scores 0 and is not planned.
            let cases = [
                (0, 5, true),
                (0, 6, false),
                (1, 5, true),
                (1, 4, false),
                (2, 5, true),
                (3, 5, false),
                (3, 4, true),
            ];
            for (cmp, value, fires) in cases {
                let t = sx_tactic_new(ONE_WEIGHT, 0, 1);
                assert_eq!(sx_tactic_add_abort_when(t, IN_EXTRA as u32, cmp, value), 0);
                let sq = custom_squad(t);
                let (mut id, extra) = (0, [5, 5]);
                let mut at =
                    |now| sx_squad_check(sq, now, extra.as_ptr(), 2, null(), 0, 0, &mut id);
                let want = if fires { SQUAD_NO_PLAN } else { SQUAD_REPLAN };
                assert_eq!(at(0), want, "cmp {cmp} value {value}");
                sx_squad_free(sq);
                // A column beyond the row never fires.
                let t = sx_tactic_new(ONE_WEIGHT, 0, 1);
                assert_eq!(sx_tactic_add_abort_when(t, 99, cmp, value), 0);
                let sq = custom_squad(t);
                let mut at =
                    |now| sx_squad_check(sq, now, extra.as_ptr(), 2, null(), 0, 0, &mut id);
                assert_eq!(at(0), SQUAD_REPLAN, "cmp {cmp} on column 99");
                sx_squad_free(sq);
            }
        }
    }

    #[test]
    fn tactic_new_aborts_on_lost_target() {
        unsafe {
            for (on, want) in [(None, SQUAD_NO_PLAN), (Some(0), SQUAD_KEEP)] {
                let t = sx_tactic_new(ONE_WEIGHT, 0, 1);
                if let Some(on) = on {
                    assert_eq!(sx_tactic_set_abort_if_target_lost(t, on), 0);
                }
                let sq = custom_squad(t);
                let mut id = 0;
                assert_eq!(check(sq, 0, &mut id), SQUAD_REPLAN);
                assert_eq!(check(sq, 5, &mut id), SQUAD_KEEP);
                assert_eq!(
                    crate::memory::sx_knowledge_remove(sx_squad_knowledge(sq), E1, ENEMY),
                    1
                );
                assert_eq!(check(sq, 6, &mut id), want);
                sx_squad_free(sq);
            }
            assert_eq!(sx_tactic_set_abort_if_target_lost(null_mut(), 1), -1);
        }
    }

    #[test]
    fn held_back_orders_go_stale() {
        unsafe {
            // Three of four orders wait beyond the cap; their members are
            // suspended before the next call.
            let (sq, _) = active_flank();
            let mut out = [SxOrder::default(); 8];
            assert_eq!(sx_squad_orders_due(sq, 5, out.as_mut_ptr(), 1), 1);
            for m in (1..=4).filter(|&m| m != out[0].member) {
                assert_eq!(sx_squad_suspend(sq, m), 1);
            }
            assert_eq!(sx_squad_orders_due(sq, 5, out.as_mut_ptr(), 8), 0);
            sx_squad_free(sq);

            // Three wait while a new plan becomes active: only its own
            // orders come out.
            let (sq, id) = active_flank();
            assert_eq!(sx_squad_orders_due(sq, 5, out.as_mut_ptr(), 1), 1);
            for sender in [2, 3] {
                let broken = SxMessage {
                    kind: REPORT_GOAL_BROKEN,
                    sender,
                    data: id,
                    ..SxMessage::default()
                };
                assert_eq!(sx_squad_report(sq, &broken, 6), 1);
            }
            let mut next = 0;
            assert_eq!(check(sq, 6, &mut next), SQUAD_REPLAN);
            // At 11 the new plan becomes active (check may plan again).
            check(sq, 11, &mut 0);
            let mut plan = SxPlan::default();
            assert_eq!(sx_squad_active_plan(sq, &mut plan, null_mut(), 0), 1);
            assert_eq!(plan.id, next);
            let n = sx_squad_orders_due(sq, 11, out.as_mut_ptr(), 8);
            assert_eq!(n, 4);
            assert!(out[..4].iter().all(|o| o.goal.plan == next));
            sx_squad_free(sq);
        }
    }

    #[test]
    fn arrival_report_resume_and_spot_radius_through_c() {
        unsafe {
            // Arrival reports count once they are in.
            let (sq, id) = active_flank();
            for sender in 1..=4 {
                let arrived = SxMessage {
                    kind: REPORT_ARRIVED,
                    sender,
                    data: id,
                    ..SxMessage::default()
                };
                assert_eq!(sx_squad_report(sq, &arrived, 20), 1);
            }
            let mut next = 0;
            assert_eq!(check(sq, 19, &mut next), SQUAD_KEEP);
            assert_eq!(check(sq, 20, &mut next), SQUAD_SIGNAL);
            // The attack orders replace the gather orders not yet taken.
            let mut out = [SxOrder::default(); 8];
            assert_eq!(sx_squad_orders_due(sq, 20, out.as_mut_ptr(), 8), 4);
            assert!(out[..4].iter().all(|o| o.goal.kind == GOAL_ATTACK));
            // A member back after the signal gets the attack order again.
            assert_eq!(sx_squad_suspend(sq, 3), 1);
            assert_eq!(sx_squad_reinstate(sq, 3, 30), 1);
            assert_eq!(sx_squad_orders_due(sq, 30, out.as_mut_ptr(), 8), 1);
            let g = out[0].goal;
            assert_eq!((out[0].member, g.kind, g.plan), (3, GOAL_ATTACK, id));
            sx_squad_free(sq);

            // Rush's wish is (19, 0); a spot 5 away counts only with a
            // larger radius.
            let sq = squad_of(EUCLID, &[(1, 0, 0)]);
            assert_eq!(sx_squad_add_rush(sq), 0);
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E1, 20, 0), null_mut()), 0);
            let spots = [spot(14, 0, 0)];
            let mut plan = [SxOrder::default(); 1];
            for (radius, pos, unchecked) in [(-1, [19, 0, 0], 1), (5, [14, 0, 0], 0)] {
                assert_eq!(sx_squad_set_spot_radius(sq, radius), 0);
                let made = sx_squad_plan(sq, 0, null(), 0, spots.as_ptr(), 1, 0, &mut next);
                assert_eq!(made, 1);
                assert_eq!(
                    sx_squad_pending_plan(sq, null_mut(), plan.as_mut_ptr(), 1),
                    1
                );
                assert_eq!((plan[0].goal.pos, plan[0].goal.unchecked), (pos, unchecked));
            }
            assert_eq!(sx_squad_set_spot_radius(sq, 1 << 32), ERROR_ARGUMENT);
            assert_eq!(sx_squad_set_spot_radius(sq, i64::from(u32::MAX)), 0);
            sx_squad_free(sq);
        }
    }

    #[test]
    fn squad_targets_writes_cap_and_total() {
        unsafe {
            let sq = squad_of(EUCLID, &[(1, 0, 0)]);
            for (i, id) in [E1, E2, 12].into_iter().enumerate() {
                let fact = enemy(id, 20 + 10 * i as i32, 0);
                assert_eq!(sx_squad_observe(sq, 0, &fact, null_mut()), 0);
            }
            let mut out = [SxFact::default(); 3];
            let mut total = 99u32;
            assert_eq!(sx_squad_targets(sq, out.as_mut_ptr(), 1, &mut total), 1);
            assert_eq!(total, 3);
            assert_eq!(sx_squad_targets(sq, out.as_mut_ptr(), 3, null_mut()), 3);
            assert_eq!(sx_squad_targets(sq, null_mut(), 0, &mut total), 0);
            assert_eq!(total, 3);
            assert_eq!(
                sx_squad_targets(sq, null_mut(), 1, &mut total),
                ERROR_ARGUMENT
            );
            sx_squad_free(sq);
        }
    }

    #[test]
    fn channels_and_reports_through_c() {
        unsafe {
            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0), (2, 3, 0), (3, 9, 0)]);
            assert_eq!(sx_squad_set_shout(sq, 2, 5), 1);
            assert_eq!(sx_squad_set_shout(sq, 3, 5), 1);
            assert_eq!(sx_squad_set_shout(sq, 9, 5), 0);
            assert_eq!(sx_squad_set_radio(sq, 9, 1, 1), 0);
            assert_eq!(sx_squad_set_silent(sq, 9), 0);
            assert_eq!(sx_squad_add_rush(sq), 0);

            // An enemy report: a target known by being told.
            let seen = SxMessage {
                kind: REPORT_ENEMY,
                subject: E1,
                pos: [20, 0, 0],
                sender: 2,
                ..SxMessage::default()
            };
            assert_eq!(sx_squad_report(sq, &seen, 0), 1);
            // A danger report: the shooter stood at (30, 4, 1).
            assert_eq!(
                sx_squad_report_danger(sq, 2, [3, 0, 1].as_ptr(), [30, 4, 1].as_ptr(), 0),
                1
            );
            let mut danger = SxArea::default();
            assert_eq!(sx_squad_danger_area(sq, &mut danger), 0);
            assert_eq!((danger.radius, danger.lifetime), (2, 255));
            danger.radius = 3;
            danger.strength = 9;
            assert_eq!(sx_squad_set_danger_area(sq, &danger), 0);
            let mut back = SxArea::default();
            assert_eq!(sx_squad_danger_area(sq, &mut back), 0);
            assert_eq!((back.radius, back.strength), (3, 9));

            let mut id = 0;
            assert_eq!(check(sq, 0, &mut id), SQUAD_REPLAN);
            let mut fact = SxFact::default();
            assert_eq!(sx_squad_targets(sq, &mut fact, 1, null_mut()), 1);
            assert_eq!((fact.subject, fact.pos, fact.source), (E1, [20, 0, 0], 2));
            let areas = sx_squad_areas(sq);
            assert_eq!(sx_areas_count(areas), 1);
            let mut area = SxArea::default();
            assert_eq!(sx_areas_at(areas, 0, &mut area), 1);
            assert_eq!(
                (area.pos, area.source, area.has_source),
                ([3, 0, 1], [30, 4, 1], 1)
            );
            assert_eq!((area.radius, area.data), (3, 2));

            // Weak: active after 34 ticks. Shout: one tick per cell within
            // range; 3 (9 away) gets nothing.
            let mut out = [SxOrder::default(); 4];
            let mut got = Vec::new();
            for t in 0..=50 {
                let n = sx_squad_orders_due(sq, t, out.as_mut_ptr(), 4);
                got.extend(out[..n as usize].iter().map(|o| (t, o.member)));
            }
            assert_eq!(got, [(34, 1), (37, 2)]);
            sx_squad_free(sq);
        }
    }

    #[test]
    fn skill_table_through_c() {
        unsafe {
            let mut table = SxSkillTable::default();
            assert_eq!(sx_skill_table_default(&mut table), 0);
            assert_eq!(table.rows, 4);
            assert_eq!(table.think_ticks, [34, 20, 10, 5, 5, 5, 5, 5]);
            assert_eq!(table.pick, [1, 1, 1, 0, 0, 0, 0, 0]);
            assert_eq!(table.pick_top, [3, 2, 2, 0, 0, 0, 0, 0]);
            assert_eq!(table.spot_rank_error, [2, 1, 1, 0, 0, 0, 0, 0]);

            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0), (2, 1, 0)]);
            table.think_ticks = [1, 2, 3, 4, 4, 4, 4, 4];
            assert_eq!(sx_squad_set_skill_table(sq, &table), 0);
            let mut back = SxSkillTable::default();
            assert_eq!(sx_squad_skill_table(sq, &mut back), 0);
            assert_eq!(back.think_ticks, [1, 2, 3, 4, 4, 4, 4, 4]);
            table.pick[2] = 3;
            assert_eq!(sx_squad_set_skill_table(sq, &table), ERROR_ARGUMENT);
            assert_eq!(sx_squad_skill_table(sq, &mut back), 0);
            assert_eq!(back.pick[2], 1, "a refused table changes nothing");
            sx_squad_free(sq);
        }
    }

    #[test]
    fn skill_table_rows() {
        unsafe {
            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0), (2, 1, 0)]);
            let mut table = SxSkillTable::default();
            assert_eq!(sx_skill_table_default(&mut table), 0);
            for rows in [0, 9] {
                table.rows = rows;
                assert_eq!(sx_squad_set_skill_table(sq, &table), ERROR_ARGUMENT);
            }
            let mut back = SxSkillTable::default();
            assert_eq!(sx_squad_skill_table(sq, &mut back), 0);
            assert_eq!(back.rows, 4, "a refused table changes nothing");
            // Three rows: skill 200 is level 2.
            table.rows = 3;
            table.think_ticks = [9, 6, 3, 3, 3, 3, 3, 3];
            assert_eq!(sx_squad_set_skill_table(sq, &table), 0);
            assert_eq!(sx_squad_skill_table(sq, &mut back), 0);
            assert_eq!(back, table);
            assert_eq!(sx_skill_table_think_ticks_for(&table, 200), 3);
            assert_eq!(sx_skill_table_think_ticks_for(&table, 85), 9);
            assert_eq!(sx_skill_table_think_ticks_for(&table, 86), 6);
            table.rows = 9;
            assert_eq!(
                sx_skill_table_think_ticks_for(&table, 0),
                i64::from(ERROR_ARGUMENT)
            );
            sx_squad_free(sq);
        }
    }

    /// A squad with one tactic scored on two extra columns (50 and 80 of 100).
    unsafe fn two_factor_squad() -> *mut SxSquad {
        unsafe {
            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0), (2, 1, 0)]);
            assert_eq!(sx_squad_set_extra_columns(sq, 2), 0);
            let first = sx_squad_extra_start(sq) as u32;
            let tactic = sx_tactic_new(ONE_WEIGHT, 0, 0);
            for input in [first, first + 1] {
                let c = SxConsideration {
                    input,
                    curve: 0,
                    min: 0,
                    max: 100,
                };
                assert_eq!(sx_tactic_add_consideration(tactic, &c), 0);
            }
            let slot = SxSlot {
                min: 1,
                max: 1,
                place: SxPlace {
                    kind: PLACE_HERE,
                    ..SxPlace::default()
                },
                kind: GOAL_HOLD,
                ..SxSlot::default()
            };
            assert_eq!(sx_tactic_add_slot(tactic, &slot), 0);
            assert_eq!(sx_squad_add_tactic(sq, tactic), 0);
            sq
        }
    }

    unsafe fn tactic_score(sq: *mut SxSquad) -> u32 {
        use crate::decide::{SxCandidate, SxChoice, sx_decider_decide};
        unsafe {
            let first = sx_squad_extra_start(sq) as usize;
            let mut row = vec![0i32; first + 2];
            row[first] = 50;
            row[first + 1] = 80;
            let cand = SxCandidate {
                option: 0,
                target: 0,
            };
            let (mut score, mut choice) = (0u32, SxChoice::default());
            let d = sx_squad_decider(sq);
            assert_eq!(
                sx_decider_decide(
                    d,
                    &cand,
                    1,
                    row.as_ptr(),
                    row.len() as i32,
                    0,
                    0,
                    0,
                    &mut score,
                    &mut choice
                ),
                1
            );
            score
        }
    }

    #[test]
    fn lent_squad_decider() {
        use crate::decide::{sx_decider_free, sx_decider_set_combine};
        unsafe {
            let sq = two_factor_squad();
            let d = sx_squad_decider(sq);
            assert!(!d.is_null());
            assert_eq!(sx_squad_decider(sq), d, "one handle per squad");
            let product = tactic_score(sq);
            assert_eq!(sx_decider_set_combine(d, 0, 2), 0);
            let min = tactic_score(sq);
            assert!(min.abs_diff(ONE_WEIGHT / 2) <= 1, "{min}");
            assert_ne!(min, product);
            // Freeing a lent decider does nothing.
            sx_decider_free(d);
            assert_eq!(sx_decider_set_combine(d, 0, 0), 0);
            assert_eq!(tactic_score(sq), product);
            // A changed option count is a plan error.
            assert_eq!(sx_squad_validate(sq, null_mut(), null_mut()), 0);
            assert_eq!(crate::decide::sx_decider_add_option(d, ONE_WEIGHT), 1);
            let (mut tactic, mut input) = (0, 0);
            assert_eq!(sx_squad_validate(sq, &mut tactic, &mut input), ERROR_PLAN);
            assert_eq!((tactic, input), (u32::MAX, u32::MAX));
            assert!(sx_squad_decider(null_mut()).is_null());
            sx_squad_free(sq);
        }
    }

    #[test]
    fn score_log_cap_and_total() {
        unsafe {
            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0), (2, 1, 0), (3, 0, 1), (4, 1, 1)]);
            assert_eq!(sx_squad_add_rush(sq), 0);
            assert_eq!(sx_squad_add_focus(sq), 1);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E1, 20, 0), null_mut()), 0);
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E2, 30, 0), null_mut()), 0);
            let mut id = 0;
            assert_eq!(sx_squad_plan(sq, 1, null(), 0, null(), 0, 0, &mut id), 1);
            let mut total = 99;
            assert_eq!(sx_squad_score_log(sq, null_mut(), 0, &mut total), 0);
            assert_eq!(total, 0, "off by default");
            assert_eq!(sx_squad_set_score_log(sq, 1), 0);
            assert_eq!(sx_squad_plan(sq, 2, null(), 0, null(), 0, 0, &mut id), 1);
            assert_eq!(sx_squad_score_log(sq, null_mut(), 0, &mut total), 0);
            // Four candidates, scored events at least; focus has a factor each.
            assert!(total >= 6, "{total}");
            let mut all = vec![SxScoreEvent::default(); total as usize];
            assert_eq!(
                sx_squad_score_log(sq, all.as_mut_ptr(), total, null_mut()),
                total as i32
            );
            let scored: Vec<(u32, u32, u8)> = all
                .iter()
                .filter(|e| e.event.kind == 2)
                .map(|e| (e.tactic, e.target, e.has_target))
                .collect();
            assert_eq!(scored, [(0, E1, 1), (0, E2, 1), (1, E1, 1), (1, E2, 1)]);
            // A smaller cap writes that many and still counts all.
            let mut two = [SxScoreEvent::default(); 2];
            let mut seen = 0;
            assert_eq!(sx_squad_score_log(sq, two.as_mut_ptr(), 2, &mut seen), 2);
            assert_eq!((seen, two[0], two[1]), (total, all[0], all[1]));
            assert_eq!(
                sx_squad_score_log(sq, null_mut(), 1, &mut seen),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_squad_set_score_log(sq, 0), 0);
            assert_eq!(sx_squad_score_log(sq, null_mut(), 0, &mut seen), 0);
            assert_eq!(seen, 0);
            assert_eq!(sx_squad_set_score_log(null_mut(), 1), ERROR_ARGUMENT);
            sx_squad_free(sq);
        }
    }

    #[test]
    fn keeper_member_helpers_through_c() {
        use crate::decide::{
            SxCandidate, SxChoice, sx_decider_decide, sx_decider_free, sx_decider_new,
        };
        unsafe {
            assert_eq!(define("SX_KEEPER_IN_HAS_GOAL"), 0);
            assert_eq!(define("SX_KEEPER_IN_DIST"), 1);
            assert_eq!(define("SX_KEEPER_IN_AGE"), 2);
            assert_eq!(define("SX_KEEPER_IN_SIGNAL"), 3);
            assert_eq!(define("SX_KEEPER_IN_DISCIPLINE"), 4);
            assert_eq!(define("SX_KEEPER_IN_EXTRA"), 5);
            assert!(!HEADER.contains("sx_keeper_resume"));
            assert!(!HEADER.contains("sx_keeper_postpone"));
            assert!(HEADER.contains("sx_member_inputs"));

            let d = sx_decider_new();
            assert_eq!(sx_keeper_add_goal_option(d, ONE_WEIGHT), 0);
            assert_eq!(
                sx_keeper_add_goal_option(null_mut(), ONE_WEIGHT),
                ERROR_ARGUMENT
            );
            let score = |row: [i32; 5]| {
                let c = [SxCandidate {
                    option: 0,
                    target: 0,
                }];
                let mut scores = [0u32; 1];
                let mut choice = SxChoice::default();
                assert!(
                    sx_decider_decide(
                        d,
                        c.as_ptr(),
                        1,
                        row.as_ptr(),
                        5,
                        0,
                        0,
                        1,
                        scores.as_mut_ptr(),
                        &mut choice
                    ) >= 0
                );
                scores[0]
            };
            assert_eq!(score([1, 0, 0, 0, 0]), ONE_WEIGHT);
            assert_eq!(score([0, 0, 0, 0, 0]), 0);
            assert_eq!(score([1, 64, 0, 0, 0]), 0);

            let k = sx_keeper_new();
            let goal = SxGoal {
                kind: GOAL_MOVE,
                has_pos: 1,
                pos: [0, 0, 0],
                ..SxGoal::default()
            };
            assert_eq!(sx_keeper_set(k, 0, &goal), 0);
            assert_eq!(sx_keeper_set_current(k, d, 0, ONE_WEIGHT / 2), 0);
            assert_eq!(score([1, 0, 0, 0, 0]), 98302);
            assert_eq!(sx_keeper_goal_postponed(k, 5), 0);
            assert_eq!(sx_keeper_set_current(k, d, 0, ONE_WEIGHT / 2), 0);
            assert_eq!(score([1, 0, 0, 0, 0]), 65535);
            assert_eq!(sx_keeper_set_current(null(), d, 0, 0), ERROR_ARGUMENT);
            assert_eq!(sx_keeper_set_current(k, null_mut(), 0, 0), ERROR_ARGUMENT);
            sx_keeper_free(k);
            sx_decider_free(d);
        }
    }

    #[test]
    fn goal_start_and_arrival_through_c() {
        unsafe {
            let goal = |start, start_ticks, has_then| SxGoal {
                kind: GOAL_MOVE,
                start,
                start_ticks,
                start_delay: 3,
                has_then_pos: has_then,
                ..SxGoal::default()
            };
            let mut out = 0u32;
            assert_eq!(
                sx_goal_start_tick(&goal(START_AFTER_TICKS, 5, 0), 100, &mut out),
                1
            );
            assert_eq!(out, 108);
            assert_eq!(
                sx_goal_start_tick(&goal(START_NOW, 0, 0), u32::MAX, &mut out),
                1
            );
            assert_eq!(out, 2);
            assert_eq!(
                sx_goal_start_tick(&goal(START_ON_SIGNAL, 0, 0), 1, &mut out),
                0
            );
            assert_eq!(
                sx_goal_start_tick(&goal(START_NOW, 0, 0), 1, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_goal_start_tick(null(), 1, &mut out), ERROR_ARGUMENT);
            assert_eq!(sx_goal_awaits_arrival(&goal(START_NOW, 0, 1)), 1);
            assert_eq!(sx_goal_awaits_arrival(&goal(START_ON_SIGNAL, 0, 0)), 1);
            assert_eq!(sx_goal_awaits_arrival(&goal(START_NOW, 0, 0)), 0);
            assert_eq!(sx_goal_awaits_arrival(null()), ERROR_ARGUMENT);
        }
    }

    #[test]
    fn skill_lookups_through_c() {
        unsafe {
            let err = i64::from(ERROR_ARGUMENT);
            assert_eq!(sx_skill_table_give_up_ticks_for(null(), 255), 68);
            assert_eq!(sx_skill_table_give_up_ticks_for(null(), 0), 17);
            assert_eq!(sx_skill_table_think_ticks_for(null(), 255), 5);
            assert_eq!(sx_skill_table_hold_bonus_for(null(), 0), 65535 / 8);
            assert_eq!(sx_skill_table_timing_spread_for(null(), 0), 17);
            assert_eq!(sx_skill_table_spot_rank_error_for(null(), 0), 2);
            assert_eq!(sx_skill_table_give_up_ticks_for(null(), 256), err);
            let (mut pick, mut top) = (9, 9);
            assert_eq!(sx_skill_table_pick_for(null(), 255, &mut pick, &mut top), 0);
            assert_eq!((pick, top), (0, 0));
            assert_eq!(sx_skill_table_pick_for(null(), 0, &mut pick, &mut top), 0);
            assert_eq!((pick, top), (1, 3));
            assert_eq!(
                sx_skill_table_pick_for(null(), 0, null_mut(), &mut top),
                ERROR_ARGUMENT
            );
            let mut table = SxSkillTable::default();
            assert_eq!(sx_skill_table_default(&mut table), 0);
            table.give_up_ticks[1] = 99;
            assert_eq!(sx_skill_table_give_up_ticks_for(&table, 64), 99);
            table.pick[0] = 7;
            assert_eq!(sx_skill_table_give_up_ticks_for(&table, 64), err);
        }
    }

    #[test]
    fn report_constructors_through_c() {
        unsafe {
            let mut m = SxMessage::default();
            assert_eq!(sx_report_arrived(3, 9, &mut m), 0);
            assert_eq!(
                m,
                SxMessage {
                    kind: REPORT_ARRIVED,
                    sender: 3,
                    data: 9,
                    ..SxMessage::default()
                }
            );
            assert_eq!(sx_report_goal_broken(4, 7, &mut m), 0);
            assert_eq!((m.kind, m.sender, m.data), (REPORT_GOAL_BROKEN, 4, 7));
            let fact = SxFact {
                subject: 8,
                pos: [5, 6, 1],
                seen_at: 40,
                strength: 3,
                data: 12,
                ..SxFact::default()
            };
            assert_eq!(sx_report_enemy(2, &fact, &mut m), 0);
            assert_eq!(
                m,
                SxMessage {
                    kind: REPORT_ENEMY,
                    subject: 8,
                    pos: [5, 6, 1],
                    seen_at: 40,
                    strength: 3,
                    data: 12,
                    sender: 2,
                    ..SxMessage::default()
                }
            );
            assert_eq!(sx_report_enemy(2, null(), &mut m), ERROR_ARGUMENT);
            assert_eq!(sx_report_arrived(1, 1, null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_report_goal_broken(1, 1, null_mut()), ERROR_ARGUMENT);
        }
    }

    #[test]
    fn keeper_through_c() {
        unsafe {
            let k = sx_keeper_new();
            assert!(!k.is_null());
            let mut g = SxGoal::default();
            let mut inputs = [0i32; 5];
            let i = inputs.as_mut_ptr();
            assert_eq!(sx_keeper_goal(k, &mut g), 0);
            assert_eq!(sx_keeper_inputs(k, 0, 0, 0, 0, CHEBYSHEV, 0, 9, i), 0);
            assert_eq!(inputs, [0, -1, 0, 0, 9]);
            let goal = SxGoal {
                kind: GOAL_MOVE,
                has_pos: 1,
                pos: [4, 0, 0],
                issued_at: 10,
                start: START_AFTER_TICKS,
                start_ticks: 2,
                ..SxGoal::default()
            };
            assert_eq!(sx_keeper_set(k, 10, &goal), 0);
            let bad = SxGoal { start: 7, ..goal };
            assert_eq!(sx_keeper_set(k, 10, &bad), ERROR_ARGUMENT);
            assert_eq!(sx_keeper_goal(k, &mut g), 1);
            assert_eq!(g, goal);
            assert_eq!(sx_keeper_inputs(k, 15, 1, 0, 0, CHEBYSHEV, 1, 9, i), 0);
            assert_eq!(inputs, [1, 3, 5, 1, 9]);
            assert_eq!(sx_keeper_inputs(k, 15, 1, 0, 0, 7, 1, 9, i), ERROR_ARGUMENT);
            assert_eq!(sx_keeper_goal_postponed(k, 20), 0);
            assert_eq!(sx_keeper_broken(k, 29, 10), 0);
            assert_eq!(sx_keeper_broken(k, 30, 10), 1);
            assert_eq!(sx_keeper_suspend(k), 0);
            assert_eq!(sx_keeper_broken(k, 40, 10), 0);
            assert_eq!(sx_keeper_unsuspend(k, 40), 0);
            assert_eq!(sx_keeper_broken(k, 49, 10), 0);
            assert_eq!(sx_keeper_broken(k, 50, 10), 1);
            assert_eq!(sx_keeper_goal_won(k, 50), 0);
            assert_eq!(sx_keeper_broken(k, 99, 10), 0);
            assert_eq!(sx_keeper_clear(k), 0);
            assert_eq!(sx_keeper_goal(k, &mut g), 0);
            sx_keeper_free(k);
        }
    }

    #[test]
    fn null_arguments_are_refused() {
        let n = null_mut::<SxSquad>();
        let k = null_mut::<SxKeeper>();
        let t = null_mut::<SxTactic>();
        let e = ERROR_ARGUMENT;
        unsafe {
            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0)]);
            let mut id = 0;
            let mut m = SxMember::default();
            assert_eq!(sx_squad_add_member(n, &member(1, 0, 0, 0)), -1);
            assert_eq!(sx_squad_add_member(sq, null()), -1);
            assert_eq!(sx_squad_member_at(sq, 0, null_mut()), e);
            assert_eq!(sx_squad_member_at(n, 0, &mut m), e);
            assert_eq!(sx_squad_member_count(n), e);
            assert_eq!(sx_squad_remove_member(n, 1), e);
            assert_eq!(sx_squad_set_pos(n, 1, 0, 0, 0), e);
            assert_eq!(sx_squad_suspend(n, 1), e);
            assert_eq!(sx_squad_reinstate(n, 1, 0), e);
            assert_eq!(sx_squad_set_spot_radius(n, 1), e);
            assert_eq!(sx_squad_set_gone(n, 1), e);
            assert_eq!(sx_squad_set_leader(n, 1), e);
            assert_eq!(sx_squad_leader(n, &mut id), e);
            assert_eq!(sx_squad_set_skill(n, 1), e);
            assert_eq!(sx_squad_skill(n), e);
            assert_eq!(sx_squad_set_skill_table(sq, null()), e);
            assert_eq!(sx_squad_skill_table(sq, null_mut()), e);
            assert_eq!(sx_skill_table_default(null_mut()), e);
            assert_eq!(sx_squad_set_shout(n, 1, 0), e);
            assert_eq!(sx_squad_set_radio(n, 1, 0, 0), e);
            assert_eq!(sx_squad_set_silent(n, 1), e);
            assert_eq!(sx_squad_report_danger(n, 1, [0; 3].as_ptr(), null(), 0), e);
            assert_eq!(sx_squad_report_danger(sq, 1, null(), null(), 0), e);
            assert_eq!(sx_squad_take_activated(n, null_mut()), e);
            assert_eq!(sx_squad_report_arrives_at(n, 1, 0, null_mut()), e);
            assert_eq!(sx_squad_add_rush(n), e);
            assert_eq!(sx_squad_add_formation(n, 0, 1), e);
            assert_eq!(sx_squad_add_flank(n, 2, 1), e);
            assert_eq!(sx_squad_set_danger_area(sq, null()), e);
            assert_eq!(sx_squad_danger_area(sq, null_mut()), e);
            assert_eq!(sx_squad_set_target_kind(n, 1), e);
            assert_eq!(sx_squad_set_extra_columns(n, 1), e);
            assert!(sx_squad_knowledge(n).is_null());
            assert!(sx_squad_areas(n).is_null());
            assert_eq!(sx_squad_observe(sq, 0, null(), null_mut()), e);
            assert_eq!(sx_squad_report(sq, null(), 0), e);
            assert_eq!(sx_squad_targets(sq, null_mut(), 1, null_mut()), e);
            assert_eq!(sx_squad_validate(n, null_mut(), null_mut()), e);
            assert_eq!(sx_squad_plan(n, 0, null(), 0, null(), 0, 0, &mut id), e);
            assert_eq!(sx_squad_plan(sq, 0, null(), 1, null(), 0, 0, &mut id), e);
            assert_eq!(sx_squad_check(sq, 0, null(), 0, null(), 1, 0, &mut id), e);
            assert_eq!(sx_squad_orders_due(sq, 0, null_mut(), 1), e);
            assert_eq!(sx_squad_signal(n, 0), e);
            assert_eq!(sx_squad_arrived(n, 1, 0), e);
            assert_eq!(sx_squad_signal_given(n), e);
            assert_eq!(sx_squad_pending_plan(sq, null_mut(), null_mut(), 1), e);
            assert_eq!(sx_squad_active_plan(n, null_mut(), null_mut(), 0), e);
            assert_eq!(sx_tactic_add_slot(t, &SxSlot::default()), -1);
            let real = sx_tactic_new(1, 0, 1);
            assert_eq!(sx_tactic_add_slot(real, null()), -1);
            assert_eq!(sx_tactic_add_consideration(real, null()), e);
            assert_eq!(sx_tactic_add_abort(t, 0, 0), e);
            assert_eq!(sx_tactic_trigger_none(t), e);
            assert_eq!(sx_tactic_trigger_all_arrived(t, 0), e);
            assert_eq!(sx_tactic_trigger_target_in_radius(t, 0), e);
            assert_eq!(sx_tactic_trigger_manual(t), e);
            assert_eq!(sx_squad_config_default(null_mut()), e);
            sx_tactic_free(real);
            assert_eq!(sx_keeper_set(k, 0, &SxGoal::default()), e);
            let real = sx_keeper_new();
            assert_eq!(sx_keeper_set(real, 0, null()), e);
            assert_eq!(sx_keeper_goal(real, null_mut()), e);
            assert_eq!(sx_keeper_inputs(real, 0, 0, 0, 0, 0, 0, 0, null_mut()), e);
            assert_eq!(sx_keeper_clear(k), e);
            assert_eq!(sx_keeper_goal_postponed(k, 0), e);
            assert_eq!(sx_keeper_goal_won(k, 0), e);
            assert_eq!(sx_keeper_suspend(k), e);
            assert_eq!(sx_keeper_unsuspend(k, 0), e);
            assert_eq!(sx_keeper_broken(k, 0, 0), e);
            sx_keeper_free(real);
            sx_keeper_free(k);
            sx_squad_free(n);
            sx_squad_free(sq);
        }
    }

    /// One cell from `from` toward `to` on Chebyshev: the neighbour nearest
    /// to `to`, ties to the first in the list (as in the Rust scene).
    fn step_toward(from: [i32; 3], to: [i32; 3]) -> [i32; 3] {
        const DIRS: [(i32, i32); 8] = [
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
            (0, -1),
            (1, -1),
        ];
        if from[..2] == to[..2] {
            return from;
        }
        let left = |p: [i32; 3]| (to[0] - p[0]).abs().max((to[1] - p[1]).abs());
        DIRS.iter()
            .map(|&(dx, dy)| [from[0] + dx, from[1] + dy, from[2]])
            .min_by_key(|&p| left(p))
            .unwrap()
    }

    /// A scripted member: its id, goal, start tick and whether it reached
    /// its gather spot.
    struct Walker {
        id: u32,
        goal: Option<SxGoal>,
        start_at: u32,
        gathered: bool,
    }

    /// The `three_levels` flank scene at skill 192, driven only through the
    /// C calls: the leader gathers its three nearest free members, the
    /// flank is planned once, gathers, fires one signal, and every attack
    /// starts at the same tick.
    #[test]
    fn three_levels_flank_in_c() {
        const FLANK: u32 = 2;
        let around = [
            candidate(7, 1, 30, 0, 1),
            candidate(4, 1, -3, 0, 1),
            candidate(3, 1, 0, -2, 1),
            candidate(6, 1, 1, 1, 0),
            candidate(2, 1, 0, 2, 1),
        ];
        let mut called = [0u32; 3];
        let n = unsafe {
            sx_gather(
                0,
                0,
                1,
                10,
                CHEBYSHEV,
                1,
                around.as_ptr(),
                5,
                called.as_mut_ptr(),
                3,
            )
        };
        assert_eq!((n, called), (3, [2, 3, 4]));
        let mut members = vec![(1, 0, 0)];
        for id in called {
            let c = around.iter().find(|c| c.id == id).unwrap();
            members.push((id, c.pos[0], c.pos[1]));
        }
        let sq = squad_of(CHEBYSHEV, &members);
        let spots = [
            spot(16, 0, TAG_COVER),
            spot(16, 2, 0),
            spot(20, 4, TAG_HIGH),
            spot(20, -4, 0),
            spot(12, 8, TAG_COVER),
        ];
        unsafe {
            assert_eq!(sx_squad_set_skill(sq, 192), 0);
            assert_eq!(sx_squad_add_rush(sq), 0);
            assert_eq!(sx_squad_add_formation(sq, SHAPE_LINE, 2), 1);
            assert_eq!(sx_squad_add_flank(sq, 3, 4), 2);
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E1, 20, 0), null_mut()), 0);
            let told = SxMessage {
                kind: REPORT_ENEMY,
                subject: E2,
                pos: [24, 10, 0],
                sender: 2,
                ..SxMessage::default()
            };
            assert_eq!(sx_squad_report(sq, &told, 0), 1);
        }

        let mut walkers: Vec<Walker> = members
            .iter()
            .map(|&(id, ..)| Walker {
                id,
                goal: None,
                start_at: 0,
                gathered: false,
            })
            .collect();
        let mut plans = Vec::new();
        let mut signals = Vec::new();
        let mut orders = Vec::new();
        let mut arrivals = Vec::new();
        let mut attacks = Vec::new();
        let mut out = [SxOrder::default(); 8];
        for t in 0..=60u32 {
            for (i, w) in walkers.iter_mut().enumerate() {
                let Some(goal) = w.goal else { continue };
                let mut m = SxMember::default();
                assert_eq!(unsafe { sx_squad_member_at(sq, i as u32, &mut m) }, 1);
                let mut at = m.pos;
                if t > w.start_at && (t - w.start_at).is_multiple_of(STEP) {
                    at = step_toward(at, goal.pos);
                    let [x, y, z] = at;
                    assert_eq!(unsafe { sx_squad_set_pos(sq, w.id, x, y, z) }, 1);
                }
                let gathers = goal.has_then_pos != 0 || goal.start == START_ON_SIGNAL;
                if gathers && !w.gathered && at[..2] == goal.pos[..2] {
                    w.gathered = true;
                    assert_eq!(unsafe { sx_squad_arrived(sq, w.id, t) }, 1);
                    arrivals.push((t, w.id));
                }
            }
            let mut id = 0;
            let status =
                unsafe { sx_squad_check(sq, t, null(), 0, spots.as_ptr(), 5, SEED, &mut id) };
            match status {
                SQUAD_REPLAN => {
                    let mut p = SxPlan::default();
                    let pending = unsafe { sx_squad_pending_plan(sq, &mut p, null_mut(), 0) };
                    assert_eq!((pending, p.id), (1, id));
                    plans.push((t, p.tactic, p.target));
                }
                SQUAD_SIGNAL => signals.push(t),
                SQUAD_KEEP | SQUAD_NO_PLAN => {}
                other => panic!("check failed: {other}"),
            }
            let n = unsafe { sx_squad_orders_due(sq, t, out.as_mut_ptr(), 8) };
            for o in &out[..n as usize] {
                let w = walkers.iter_mut().find(|w| w.id == o.member).unwrap();
                let after = if o.goal.start == START_AFTER_TICKS {
                    o.goal.start_ticks
                } else {
                    0
                };
                w.goal = Some(o.goal);
                w.start_at = t + o.goal.start_delay + after;
                w.gathered = false;
                orders.push((t, o.member));
            }
            for w in &walkers {
                if let Some(g) = w.goal
                    && g.kind == GOAL_ATTACK
                    && g.start != START_ON_SIGNAL
                    && w.start_at == t
                {
                    attacks.push((t, w.id));
                }
            }
        }
        unsafe { sx_squad_free(sq) };

        assert_eq!(plans, [(0, FLANK, E1)], "one plan, no replan");
        assert_eq!(orders[..4], [(5, 1), (9, 2), (9, 3), (10, 4)]);
        assert_eq!(arrivals, [(37, 1), (48, 4), (49, 2), (49, 3)]);
        assert_eq!(signals, [49], "one signal");
        attacks.sort_unstable_by_key(|&(_, m)| m);
        assert_eq!(attacks, [(55, 1), (55, 2), (55, 3), (55, 4)], "all aligned");
    }
}

mod conventions {
    //! C API 9: exports match the header, handle names, return rules.

    use crate::decide::{
        sx_decider_add_option, sx_decider_free, sx_decider_new, sx_decider_set_combine,
    };
    use crate::memory::{
        SxArea, sx_alertness_stimulus, sx_alertness_value, sx_areas_add, sx_areas_cost,
        sx_areas_free, sx_areas_new, sx_timers_free, sx_timers_left, sx_timers_new, sx_timers_set,
    };
    use sagax_decide::memory::{AlertConfig, Alertness};
    use std::collections::BTreeSet;
    use std::ptr::null_mut;

    const HEADER: &str = include_str!("../include/sagax.h");
    const SOURCES: [&str; 8] = [
        include_str!("lib.rs"),
        include_str!("grid.rs"),
        include_str!("memory.rs"),
        include_str!("decide.rs"),
        include_str!("machine.rs"),
        include_str!("group.rs"),
        include_str!("crew.rs"),
        include_str!("rules.rs"),
    ];

    /// Every `#[unsafe(no_mangle)]` function of the sources.
    fn exports() -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for source in SOURCES {
            let mut lines = source.lines();
            while let Some(line) = lines.next() {
                if line.trim() != "#[unsafe(no_mangle)]" {
                    continue;
                }
                for next in lines.by_ref() {
                    if let Some(at) = next.find("fn sx_") {
                        let rest = &next[at + 3..];
                        let end = rest.find('(').expect("a function name ends at (");
                        names.insert(rest[..end].trim().to_string());
                        break;
                    }
                }
            }
        }
        names
    }

    /// The header without its comments.
    fn header_code() -> String {
        let mut code = String::new();
        let mut rest = HEADER;
        while let Some(start) = rest.find("/*") {
            code.push_str(&rest[..start]);
            let end = rest[start..].find("*/").expect("comment ends");
            rest = &rest[start + end + 2..];
        }
        code.push_str(rest);
        code
    }

    /// Names that are followed by `(`: the prototypes.
    fn prototypes() -> BTreeSet<String> {
        let code = header_code();
        let bytes = code.as_bytes();
        let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut names = BTreeSet::new();
        let mut i = 0;
        while let Some(at) = code[i..].find("sx_") {
            let start = i + at;
            let mut end = start;
            while end < bytes.len() && is_word(bytes[end]) {
                end += 1;
            }
            let word_start = start == 0 || !is_word(bytes[start - 1]);
            if word_start && bytes.get(end) == Some(&b'(') {
                names.insert(code[start..end].to_string());
            }
            i = end.max(start + 3);
        }
        names
    }

    /// Size and chosen field offsets of every value struct, from the Rust
    /// `#[repr(C)]` types: `(c name, size, [(field, offset)])`.
    type Layout = (&'static str, usize, Vec<(&'static str, usize)>);

    fn rust_layouts() -> Vec<Layout> {
        use crate::crew::{SxCrewConfig, SxCrewEvent, SxMemberInputs};
        use crate::decide::{SxCandidate, SxChoice, SxConsideration, SxDecideEvent};
        use crate::group::{
            SxGatherCandidate, SxGoal, SxMember, SxOrder, SxPlace, SxPlan, SxScoreEvent,
            SxSkillTable, SxSlot, SxSpot, SxSquadConfig,
        };
        use crate::machine::SxChange;
        use crate::memory::{
            SxAlertConfig, SxAlertness, SxArea, SxFact, SxInterval, SxMessage, SxNearby,
            SxPointPattern, SxProgress,
        };
        use crate::rules::{SxChannelCtx, SxCostCtx, SxPlaceCtx, SxSpotCtx, SxViewPlan};
        use crate::{Point, SxEvent, SxResult, SxRules};
        use std::mem::{offset_of, size_of};

        macro_rules! layout {
            ($c:literal, $t:ty $(, $f:ident)*) => {
                (
                    $c,
                    size_of::<$t>(),
                    vec![$((stringify!($f), offset_of!($t, $f))),*],
                )
            };
        }
        vec![
            layout!("sx_point", Point, level),
            layout!("sx_rules", SxRules, cost_straight, smooth_max_extra),
            layout!("sx_result", SxResult, node_limit_hit),
            layout!("sx_event", SxEvent, cell, parent, cost, found),
            layout!("sx_consideration", SxConsideration, max),
            layout!("sx_candidate", SxCandidate, target),
            layout!("sx_choice", SxChoice, score),
            layout!("sx_decide_event", SxDecideEvent, score),
            layout!("sx_change", SxChange, transition),
            layout!("sx_fact", SxFact, pos, seen_at, data, source),
            layout!("sx_message", SxMessage, pos, sender, keep_if_sender_gone),
            layout!("sx_area", SxArea, radius, source, blocking, has_source),
            layout!("sx_progress", SxProgress, since, started),
            layout!("sx_interval", SxInterval, started),
            layout!("sx_alertness", SxAlertness),
            layout!("sx_alert_config", SxAlertConfig, thresholds),
            layout!("sx_nearby", SxNearby, y),
            layout!(
                "sx_point_pattern",
                SxPointPattern,
                offsets,
                distances,
                sine,
                max_y
            ),
            layout!("sx_member", SxMember, pos, roles, skill, state),
            layout!("sx_spot", SxSpot, tags),
            layout!("sx_gather_candidate", SxGatherCandidate, pos, free),
            layout!("sx_place", SxPlace, z, value),
            layout!(
                "sx_slot",
                SxSlot,
                min,
                max,
                place,
                gather,
                dist,
                gather_dist
            ),
            layout!("sx_skill_table", SxSkillTable, hold_bonus, spot_rank_error),
            layout!("sx_score_event", SxScoreEvent, event),
            layout!(
                "sx_goal",
                SxGoal,
                pos,
                then_pos,
                target,
                start_delay,
                has_pos,
                unchecked,
                has_then_pos,
                then_unchecked,
                has_target
            ),
            layout!("sx_order", SxOrder, goal),
            layout!("sx_plan", SxPlan, goal_count, has_target),
            layout!(
                "sx_squad_config",
                SxSquadConfig,
                metric,
                role_count,
                role_masks,
                rule_columns
            ),
            layout!("sx_member_inputs", SxMemberInputs, discipline),
            layout!(
                "sx_crew_config",
                SxCrewConfig,
                squad,
                arrive_radius,
                hex_grid
            ),
            layout!("sx_crew_event", SxCrewEvent, start_at, goal, has_start_at),
            layout!(
                "sx_place_ctx",
                SxPlaceCtx,
                dist,
                leader,
                centre,
                face,
                friend_pos,
                heading,
                has_target
            ),
            layout!("sx_cost_ctx", SxCostCtx, tactic, member, wish),
            layout!(
                "sx_spot_ctx",
                SxSpotCtx,
                tactic,
                want_tags,
                target,
                spot,
                member,
                wish,
                target_pos,
                gather,
                has_target
            ),
            layout!(
                "sx_channel_ctx",
                SxChannelCtx,
                distance,
                now,
                from,
                to,
                report
            ),
            layout!(
                "sx_view_plan",
                SxViewPlan,
                since,
                has_plan,
                has_target,
                signal_given
            ),
        ]
    }

    /// The `_Static_assert` lines of the header: ("sx_x", None or field, value).
    fn header_asserts() -> BTreeSet<(String, Option<String>, usize)> {
        let mut found = BTreeSet::new();
        for line in HEADER.lines() {
            let Some(rest) = line.trim().strip_prefix("_Static_assert(") else {
                continue;
            };
            if let Some(rest) = rest.strip_prefix("sizeof(") {
                let (name, rest) = rest.split_once(") == ").expect("sizeof assert");
                let (n, _) = rest.split_once(',').expect("value ends at ,");
                found.insert((name.to_string(), None, n.trim().parse().expect("a number")));
            } else if let Some(rest) = rest.strip_prefix("offsetof(") {
                let (name, rest) = rest.split_once(", ").expect("offsetof struct");
                let (field, rest) = rest.split_once(") == ").expect("offsetof field");
                let (n, _) = rest.split_once(',').expect("value ends at ,");
                found.insert((
                    name.to_string(),
                    Some(field.to_string()),
                    n.trim().parse().expect("a number"),
                ));
            }
        }
        found
    }

    #[test]
    fn header_static_asserts_match_rust() {
        let mut expected = BTreeSet::new();
        for (name, size, fields) in rust_layouts() {
            expected.insert((name.to_string(), None, size));
            for (field, offset) in fields {
                expected.insert((name.to_string(), Some(field.to_string()), offset));
            }
        }
        assert_eq!(header_asserts(), expected);
        assert!(HEADER.contains(
            "#if !defined(__cplusplus) && defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L"
        ));
        assert!(HEADER.contains("#include <stddef.h>"));
    }

    /// Structs holding pointers: no size assert (they differ between 32 and 64 bit).
    const POINTER_STRUCTS: [&str; 4] = [
        "sx_map",
        "sx_areas_cost_ctx",
        "sx_row_ctx",
        "sx_squad_rules",
    ];

    #[test]
    fn every_value_struct_has_a_size_assert() {
        let code = header_code();
        let sized: BTreeSet<String> = header_asserts()
            .into_iter()
            .filter(|(_, field, _)| field.is_none())
            .map(|(name, _, _)| name)
            .collect();
        let mut bodies = 0;
        for chunk in code.split("typedef struct ").skip(1) {
            let Some((head, rest)) = chunk.split_once('{') else {
                continue;
            };
            let name = head.trim();
            if name.contains(';') || name.contains(char::is_whitespace) {
                continue;
            }
            let (body, _) = rest.split_once('}').expect("a struct body ends");
            bodies += 1;
            if POINTER_STRUCTS.contains(&name) {
                assert!(body.contains('*'), "{name} holds no pointer");
                assert!(!sized.contains(name), "{name} must have no size assert");
            } else {
                assert!(
                    !body.contains('*'),
                    "{name} holds a pointer: list it as an exception"
                );
                assert!(sized.contains(name), "{name} has no sizeof assert");
            }
        }
        assert!(bodies >= 30, "found only {bodies} struct bodies");
    }

    #[test]
    fn every_export_is_declared_in_the_header() {
        let exports = exports();
        assert!(exports.len() > 150, "found {} exports", exports.len());
        for name in &exports {
            assert!(
                HEADER.contains(&format!("{name}(")),
                "{name} is not declared in sagax.h"
            );
        }
        for name in prototypes() {
            assert!(
                exports.contains(&name),
                "{name} is declared in sagax.h but not exported"
            );
        }
    }

    #[test]
    fn old_handle_names_are_gone() {
        for kind in ["nav", "flow", "dstar"] {
            for old in ["create", "destroy"] {
                let name = format!("sx_{kind}_{old}");
                assert!(!HEADER.contains(&name), "{name} is still in sagax.h");
            }
        }
    }

    #[test]
    fn timers_left_returns_the_time() {
        unsafe {
            let t = sx_timers_new(4);
            assert_eq!(sx_timers_set(t, 3, 10, 50), 0);
            assert_eq!(sx_timers_left(t, 3, 20), 40);
            assert_eq!(sx_timers_left(t, 9, 20), 0, "unknown id");
            sx_timers_free(t);
        }
    }

    #[test]
    fn alertness_value_returns_it() {
        let cfg = AlertConfig {
            max: 100,
            decay_per_tick: 1,
            hold: 0,
            hysteresis: 0,
            count: 2,
            thresholds: [30, 60, 0, 0, 0, 0, 0, 0],
        };
        let mut a = Alertness::default();
        unsafe {
            assert_eq!(sx_alertness_stimulus(&mut a, 30, 0, &cfg), 0);
            assert_eq!(sx_alertness_value(&a), 30);
        }
    }

    #[test]
    fn areas_cost_returns_it() {
        unsafe {
            let a = sx_areas_new(4);
            let base = SxArea {
                kind: 1,
                pos: [100, 100, 0],
                radius: 10,
                lifetime: 50,
                ..SxArea::default()
            };
            let (cell, far) = ([100, 100, 0], [900, 900, 0]);
            let blocking = SxArea {
                blocking: 1,
                strength: 99,
                ..base
            };
            assert_eq!(sx_areas_add(a, 0, &blocking), 0);
            assert_eq!(
                sx_areas_cost(a, cell.as_ptr(), far.as_ptr(), far.as_ptr()),
                0
            );
            let five = SxArea {
                kind: 2,
                strength: 5,
                ..base
            };
            let seven = SxArea {
                kind: 3,
                strength: 7,
                ..base
            };
            assert!(sx_areas_add(a, 0, &five) >= 0);
            assert!(sx_areas_add(a, 0, &seven) >= 0);
            assert_eq!(
                sx_areas_cost(a, cell.as_ptr(), far.as_ptr(), far.as_ptr()),
                12
            );
            sx_areas_free(a);
        }
    }

    #[test]
    fn set_combine_follow_bits() {
        assert!(HEADER.contains("#define SX_COMBINE_FOLLOW 0xFFFFFFFFu"));
        assert!(HEADER.contains("#define SX_ALL_OPTIONS 0xFFFFFFFFu"));
        unsafe {
            let d = sx_decider_new();
            assert_eq!(sx_decider_add_option(d, 65535), 0);
            assert_eq!(sx_decider_set_combine(d, 0, u32::MAX), 0);
            assert_eq!(sx_decider_set_combine(d, u32::MAX, 1), 0);
            sx_decider_free(d);
            assert_eq!(sx_decider_set_combine(null_mut(), 0, 0), -1);
        }
    }
}

mod positions {
    use super::*;
    use std::ptr::{null, null_mut};

    #[test]
    fn hex_converters() {
        unsafe {
            let mut out = [0i32; 3];
            assert_eq!(sx_hex_to_axial(1, [3, 3, 2].as_ptr(), out.as_mut_ptr()), 0);
            assert_eq!(out, [2, 3, 2]);
            assert_eq!(
                sx_hex_from_axial(1, [2, 3, 2].as_ptr(), out.as_mut_ptr()),
                0
            );
            assert_eq!(out, [3, 3, 2]);
            // EvenR and OddQ differ from OddR
            assert_eq!(sx_hex_to_axial(2, [3, 1, 0].as_ptr(), out.as_mut_ptr()), 0);
            assert_eq!(out, [2, 1, 0]);
            assert_eq!(sx_hex_to_axial(3, [3, 3, 0].as_ptr(), out.as_mut_ptr()), 0);
            assert_eq!(out, [3, 2, 0]);
            // square and unknown grids, null pointers
            assert_eq!(sx_hex_to_axial(0, [3, 3, 2].as_ptr(), out.as_mut_ptr()), -1);
            assert_eq!(
                sx_hex_from_axial(5, [3, 3, 2].as_ptr(), out.as_mut_ptr()),
                -1
            );
            assert_eq!(sx_hex_to_axial(1, null(), out.as_mut_ptr()), -1);
            assert_eq!(sx_hex_to_axial(1, [0, 0, 0].as_ptr(), null_mut()), -1);
        }
    }

    #[test]
    fn point_pos_bridges() {
        unsafe {
            let mut pos = [0i32; 3];
            assert_eq!(sx_point_to_pos(Point::new(4, 5, 6), pos.as_mut_ptr()), 0);
            assert_eq!(pos, [4, 5, 6]);
            let mut point = Point::default();
            assert_eq!(sx_pos_to_point(pos.as_ptr(), &mut point), 0);
            assert_eq!(point, Point::new(4, 5, 6));
            assert_eq!(sx_point_to_pos(Point::default(), null_mut()), -1);
            assert_eq!(sx_pos_to_point(null(), &mut point), -1);
            assert_eq!(sx_pos_to_point(pos.as_ptr(), null_mut()), -1);
        }
    }
}

mod crew {
    //! `sx_crew_*` (C API 9): the Rust `Crew` through C.

    use super::squad::define;
    use crate::crew::*;
    use crate::group::*;
    use crate::memory::SxFact;
    use sagax_abi::codes::*;
    use sagax_decide::group::{
        Channel, Crew, CrewConfig, CrewEvent, Member, MemberState, Shape, Spot, SquadConfig,
        TAG_COVER, TAG_HIGH, tactics,
    };
    use sagax_decide::memory::{Fact, Message, Metric};
    use std::mem::{offset_of, size_of};
    use std::ptr::{null, null_mut};

    const ENEMY: u32 = 1;
    const STEP: u32 = 2;
    const SEED: u64 = 7;
    const SKILL: u8 = 192;
    const CHEBYSHEV: u32 = 2;
    const MEMBERS: [(u32, i32, i32); 4] = [(1, 0, 0), (2, 0, 2), (3, 0, -2), (4, -3, 0)];

    fn spots() -> [Spot; 4] {
        [
            Spot {
                pos: [16, 0, 0],
                tags: TAG_COVER,
            },
            Spot {
                pos: [16, 2, 0],
                tags: 0,
            },
            Spot {
                pos: [20, 4, 0],
                tags: TAG_HIGH,
            },
            Spot {
                pos: [20, -4, 0],
                tags: 0,
            },
        ]
    }

    fn sighting(subject: u32, x: i32, y: i32) -> Fact {
        Fact {
            subject,
            kind: ENEMY,
            pos: [x, y, 0],
            ..Fact::default()
        }
    }

    /// The flank scene of the Rust crew tests, in Rust.
    fn rust_crew() -> Crew {
        let mut crew = Crew::new(CrewConfig {
            squad: SquadConfig {
                members: 4,
                memory: 8,
                areas: 4,
                inbox: 8,
                metric: Metric::Chebyshev,
                ..SquadConfig::default()
            },
            ..CrewConfig::default()
        })
        .unwrap();
        for (id, x, y) in MEMBERS {
            let m = Member {
                id,
                pos: [x, y, 0],
                roles: 0,
                skill: SKILL,
                state: MemberState::Active,
            };
            assert!(crew.add_member(m).is_some());
        }
        let sq = crew.squad_mut();
        for (id, ..) in &MEMBERS[1..] {
            let radio = Channel::Radio {
                base: 2,
                per_unit: 1,
            };
            assert!(sq.set_channel(*id, radio));
        }
        assert!(sq.set_leader(1));
        sq.set_skill(SKILL);
        sq.set_target_kind(ENEMY);
        sq.add_tactic(tactics::rush());
        sq.add_tactic(tactics::formation(Shape::Line, 2));
        sq.add_tactic(tactics::flank(3, 4));
        assert!(sq.report(Message::enemy(2, sighting(11, 24, 10)), 0));
        crew.observe(0, sighting(10, 20, 0));
        crew
    }

    fn crew_config(members: u32, metric: u32, hex_grid: u32) -> SxCrewConfig {
        let mut c = SxCrewConfig::default();
        unsafe { assert_eq!(sx_crew_config_default(&mut c), 0) };
        c.squad.members = members;
        c.squad.memory = 8;
        c.squad.areas = 4;
        c.squad.inbox = 8;
        c.squad.metric = metric;
        c.hex_grid = hex_grid;
        c
    }

    /// The same scene through `sx_crew_*`; the squad is set up through the
    /// lent squad of the crew.
    fn c_crew() -> *mut SxCrew {
        unsafe {
            let c = sx_crew_new(&crew_config(4, CHEBYSHEV, 0));
            assert!(!c.is_null());
            for (i, (id, x, y)) in MEMBERS.into_iter().enumerate() {
                let m = SxMember {
                    id,
                    pos: [x, y, 0],
                    skill: SKILL,
                    ..SxMember::default()
                };
                assert_eq!(sx_crew_add_member(c, &m), i as i32);
            }
            let sq = sx_crew_squad(c);
            assert!(!sq.is_null());
            for (id, ..) in &MEMBERS[1..] {
                assert_eq!(sx_squad_set_radio(sq, *id, 2, 1), 1);
            }
            assert_eq!(sx_squad_set_leader(sq, 1), 1);
            assert_eq!(sx_squad_set_skill(sq, u32::from(SKILL)), 0);
            assert_eq!(sx_squad_set_target_kind(sq, ENEMY), 0);
            assert_eq!(sx_squad_add_rush(sq), 0);
            assert_eq!(sx_squad_add_formation(sq, 0, 2), 1);
            assert_eq!(sx_squad_add_flank(sq, 3, 4), 2);
            let msg = Message::enemy(2, sighting(11, 24, 10));
            assert_eq!(sx_squad_report(sq, &msg, 0), 1);
            let seen: SxFact = sighting(10, 20, 0);
            assert_eq!(sx_crew_observe(c, 0, &seen, null_mut()), 0);
            c
        }
    }

    /// One event as numbers: kind, member, plan, start tick, goal position.
    type Key = (u32, u32, u32, Option<u32>, Option<[i32; 3]>);

    fn kind(name: &str) -> u32 {
        define(name) as u32
    }

    fn rust_key(e: &CrewEvent) -> Key {
        match *e {
            CrewEvent::Planned(p) => (kind("SX_CREW_PLANNED"), 0, p, None, None),
            CrewEvent::Activated(p) => (kind("SX_CREW_ACTIVATED"), 0, p, None, None),
            CrewEvent::Signal => (kind("SX_CREW_SIGNAL"), 0, 0, None, None),
            CrewEvent::Order {
                member,
                goal,
                start_at,
            } => (kind("SX_CREW_ORDER"), member, goal.plan, start_at, goal.pos),
            CrewEvent::Start { member } => (kind("SX_CREW_START"), member, 0, None, None),
            CrewEvent::Arrived { member } => (kind("SX_CREW_ARRIVED"), member, 0, None, None),
            CrewEvent::Broken { member } => (kind("SX_CREW_BROKEN"), member, 0, None, None),
            CrewEvent::NoPlan => (kind("SX_CREW_NO_PLAN"), 0, 0, None, None),
        }
    }

    fn c_key(e: &SxCrewEvent) -> Key {
        (
            e.kind,
            e.member,
            e.plan,
            (e.has_start_at != 0).then_some(e.start_at),
            (e.goal.has_pos != 0).then_some(e.goal.pos),
        )
    }

    /// Where a member of the scripted walk goes and from which tick.
    #[derive(Clone, Copy, Default)]
    struct Walk {
        to: Option<[i32; 3]>,
        from: u32,
    }

    impl Walk {
        fn next(&self, now: u32, at: [i32; 3]) -> Option<[i32; 3]> {
            let to = self.to?;
            let d = now.wrapping_sub(self.from);
            let step = |a: i32, b: i32| a + (b - a).signum();
            (d > 0 && d < 1 << 31 && d.is_multiple_of(STEP) && at != to)
                .then(|| [step(at[0], to[0]), step(at[1], to[1]), at[2]])
        }
    }

    fn walk_of(id: u32) -> usize {
        MEMBERS.iter().position(|m| m.0 == id).unwrap()
    }

    fn note(walks: &mut [Walk; 4], now: u32, keys: &[Key]) {
        for &(k, member, _, start_at, pos) in keys {
            if k == kind("SX_CREW_ORDER") {
                walks[walk_of(member)] = Walk {
                    to: pos,
                    from: start_at.unwrap_or(now),
                };
            }
        }
    }

    fn run_rust(len: u32) -> Vec<Vec<Key>> {
        let mut crew = rust_crew();
        let spots = spots();
        let mut walks = [Walk::default(); 4];
        let mut events = Vec::new();
        let mut all = Vec::new();
        for t in 0..=len {
            for (i, (id, ..)) in MEMBERS.iter().enumerate() {
                let g = crew.squad().group();
                let at = g.get(g.index(*id).unwrap()).unwrap().pos;
                if let Some(p) = walks[i].next(t, at) {
                    assert!(crew.set_pos(*id, p));
                }
            }
            crew.tick(t, &[], &spots, SEED, &mut events).unwrap();
            let keys: Vec<Key> = events.iter().map(rust_key).collect();
            note(&mut walks, t, &keys);
            all.push(keys);
        }
        all
    }

    fn member_pos(c: *mut SxCrew, index: u32) -> [i32; 3] {
        let mut m = SxMember::default();
        unsafe { assert_eq!(sx_squad_member_at(sx_crew_squad(c), index, &mut m), 1) };
        m.pos
    }

    /// Moves the members of the C crew for tick `t` by the walks.
    fn walk_c(c: *mut SxCrew, walks: &[Walk; 4], t: u32) {
        for (i, (id, ..)) in MEMBERS.iter().enumerate() {
            if let Some(p) = walks[i].next(t, member_pos(c, i as u32)) {
                unsafe { assert_eq!(sx_crew_set_pos(c, *id, p.as_ptr()), 1) };
            }
        }
    }

    /// One tick of the C crew with room for `cap` events (null if 0):
    /// (returned, total, keys of the written events).
    fn tick_c(c: *mut SxCrew, t: u32, cap: u32) -> (i32, u32, Vec<Key>) {
        let spots = spots();
        let mut buf = vec![SxCrewEvent::default(); cap as usize];
        let out = if cap == 0 {
            null_mut()
        } else {
            buf.as_mut_ptr()
        };
        let mut total = u32::MAX;
        let n = unsafe {
            sx_crew_tick(
                c,
                t,
                null(),
                0,
                spots.as_ptr(),
                spots.len() as u32,
                SEED,
                out,
                cap,
                &mut total,
            )
        };
        let written = usize::try_from(n).unwrap_or(0);
        (n, total, buf[..written].iter().map(c_key).collect())
    }

    #[test]
    fn crew_flank_scene_same_as_rust() {
        let rust = run_rust(80);
        let c = c_crew();
        let mut walks = [Walk::default(); 4];
        let mut all = Vec::new();
        for t in 0..=80 {
            walk_c(c, &walks, t);
            let (n, total, keys) = tick_c(c, t, 16);
            assert_eq!(n as u32, total, "tick {t}");
            note(&mut walks, t, &keys);
            all.push(keys);
        }
        unsafe { sx_crew_free(c) };
        assert_eq!(all, rust);
        let count = |name: &str| {
            let k = kind(name);
            rust.iter().flatten().filter(|e| e.0 == k).count()
        };
        assert_eq!(count("SX_CREW_ORDER"), 8);
        assert_eq!(count("SX_CREW_ARRIVED"), 4);
        assert_eq!(count("SX_CREW_SIGNAL"), 1);
    }

    /// Everything a game can read of the crew after a tick.
    #[derive(Debug, PartialEq)]
    struct State {
        goals: Vec<Option<SxGoal>>,
        inputs: Vec<SxMemberInputs>,
        active: (i32, SxPlan),
    }

    fn state(c: *mut SxCrew, now: u32) -> State {
        let mut goals = Vec::new();
        let mut inputs = Vec::new();
        for (id, ..) in MEMBERS {
            let mut g = SxGoal::default();
            let r = unsafe { sx_crew_goal(c, id, &mut g) };
            goals.push((r == 1).then_some(g));
            let mut row = SxMemberInputs::default();
            unsafe { assert_eq!(sx_crew_member_inputs(c, id, now, &mut row), 1) };
            inputs.push(row);
        }
        let mut plan = SxPlan::default();
        let r = unsafe { sx_squad_active_plan(sx_crew_squad(c), &mut plan, null_mut(), 0) };
        State {
            goals,
            inputs,
            active: (r, plan),
        }
    }

    #[test]
    fn crew_tick_small_buffer_reports_total() {
        // Crew `a` has room for every event and scripts the walk for both;
        // `b` gets the same inputs but a short buffer at ticks 5 (Activated,
        // Order, Start) and 9 (two orders, two starts).
        let a = c_crew();
        let b = c_crew();
        let mut walks = [Walk::default(); 4];
        let mut short = Vec::new();
        for t in 0..=80 {
            walk_c(a, &walks, t);
            walk_c(b, &walks, t);
            let (n, total, keys) = tick_c(a, t, 16);
            assert_eq!(n as u32, total);
            let (bn, btotal, bkeys) = match t {
                5 => tick_c(b, t, 1),
                9 => tick_c(b, t, 0),
                _ => tick_c(b, t, 16),
            };
            assert_eq!(btotal, total, "tick {t}: total counts all");
            assert_eq!(bkeys[..], keys[..bn as usize], "tick {t}: the first ones");
            if t == 5 || t == 9 {
                short.push((t, bn, btotal));
            } else {
                assert_eq!(bn, n, "tick {t}: the following ticks");
            }
            assert_eq!(state(b, t), state(a, t), "tick {t}: nothing else changes");
            note(&mut walks, t, &keys);
        }
        assert_eq!(short, [(5, 1, 3), (9, 0, 4)]);
        unsafe {
            // A null total is fine; cap > 0 needs a buffer.
            let n = sx_crew_tick(a, 81, null(), 0, null(), 0, SEED, null_mut(), 0, null_mut());
            assert!(n >= 0);
            let mut total = 0;
            let bad = sx_crew_tick(a, 82, null(), 0, null(), 0, SEED, null_mut(), 1, &mut total);
            assert_eq!(bad, ERROR_ARGUMENT);
            sx_crew_free(a);
            sx_crew_free(b);
        }
    }

    #[test]
    fn crew_lent_squad_and_keeper() {
        let c = c_crew();
        let mut walks = [Walk::default(); 4];
        for t in 0..=12 {
            walk_c(c, &walks, t);
            let (_, _, keys) = tick_c(c, t, 16);
            note(&mut walks, t, &keys);
        }
        unsafe {
            let sq = sx_crew_squad(c);
            assert_eq!(sq, sx_crew_squad(c), "one handle per crew");
            let mut plan = SxPlan::default();
            assert_eq!(sx_squad_active_plan(sq, &mut plan, null_mut(), 0), 1);
            assert_eq!(plan.tactic, 2, "the flank");
            // Freeing a lent squad does nothing.
            sx_squad_free(sq);
            assert_eq!(sx_squad_active_plan(sq, &mut plan, null_mut(), 0), 1);
            // The crew takes the orders; on its squad this is refused.
            let mut order = SxOrder::default();
            assert_eq!(sx_squad_orders_due(sq, 12, &mut order, 1), ERROR_ARGUMENT);

            let k = sx_crew_keeper(c, 2);
            assert!(!k.is_null());
            assert!(sx_crew_keeper(c, 99).is_null());
            let mut by_keeper = SxGoal::default();
            let mut by_crew = SxGoal::default();
            assert_eq!(sx_keeper_goal(k, &mut by_keeper), 1);
            assert_eq!(sx_crew_goal(c, 2, &mut by_crew), 1);
            assert_eq!(by_keeper, by_crew);
            // Two keepers at once; freeing a lent one does nothing.
            let k3 = sx_crew_keeper(c, 3);
            sx_keeper_free(k3);
            assert_eq!(sx_keeper_goal(k3, &mut by_keeper), 1);
            assert_eq!(by_keeper.plan, by_crew.plan);
            assert_eq!(sx_keeper_goal(k, &mut by_keeper), 1);
            assert_eq!(by_keeper, by_crew);
            // Postponing through the lent keeper reaches the crew's keeper.
            assert_eq!(sx_keeper_goal_postponed(k, 12), 0);
            assert_eq!(sx_keeper_broken(k, 12 + 1000, 1000), 1);
            // The next crew call makes lent keepers stale: refused, never
            // another member's keeper.
            assert_eq!(sx_crew_member_chose(c, 2, 13, 1), 1);
            assert_eq!(sx_keeper_goal(k, &mut by_keeper), ERROR_ARGUMENT);
            assert_eq!(sx_keeper_clear(k3), ERROR_ARGUMENT);
            let k = sx_crew_keeper(c, 2);
            assert_eq!(sx_keeper_broken(k, 12 + 1000, 1000), 0, "goal won at 13");
            // The squad stays lent until the crew is freed.
            assert_eq!(sx_squad_member_count(sq), 4);
            sx_crew_free(c);
        }
    }

    #[test]
    fn crew_lent_keeper_follows_its_member() {
        let c = c_crew();
        let mut walks = [Walk::default(); 4];
        for t in 0..=12 {
            walk_c(c, &walks, t);
            let (_, _, keys) = tick_c(c, t, 16);
            note(&mut walks, t, &keys);
        }
        unsafe {
            let goal = |id| {
                let mut g = SxGoal::default();
                assert_eq!(sx_crew_goal(c, id, &mut g), 1);
                g
            };
            let mut g = SxGoal::default();
            // Member 2 sits at group index 1; once member 1 leaves through
            // the squad, member 3 does.
            let k2 = sx_crew_keeper(c, 2);
            assert_eq!(sx_squad_remove_member(sx_crew_squad(c), 1), 1);
            let k3 = sx_crew_keeper(c, 3);
            assert_ne!(k2, k3, "one handle per member id");
            assert_eq!(sx_keeper_goal(k2, &mut g), 1);
            assert_eq!(g, goal(2), "still member 2's keeper");
            assert_eq!(sx_keeper_goal(k3, &mut g), 1);
            assert_eq!(g, goal(3));
            assert_eq!(sx_crew_keeper(c, 2), k2, "the same handle again");
            // Member 2 leaves through the squad: its handle fails loudly.
            assert_eq!(sx_squad_remove_member(sx_crew_squad(c), 2), 1);
            assert_eq!(sx_keeper_goal(k2, &mut g), ERROR_ARGUMENT);
            assert_eq!(sx_keeper_goal_postponed(k2, 13), ERROR_ARGUMENT);
            assert!(sx_crew_keeper(c, 2).is_null());
            assert_eq!(sx_keeper_goal(k3, &mut g), 1);
            // The next tick drops member 2's keeper; its handle stays refused.
            let (n, ..) = tick_c(c, 13, 16);
            assert!(n >= 0);
            assert_eq!(sx_keeper_goal(k2, &mut g), ERROR_ARGUMENT);
            assert_eq!(sx_keeper_goal(k3, &mut g), ERROR_ARGUMENT, "stale");
            assert_eq!(sx_keeper_goal(sx_crew_keeper(c, 3), &mut g), 1);
            sx_crew_free(c);
        }
    }

    #[test]
    fn crew_null_and_zero() {
        unsafe {
            assert!(sx_crew_new(null()).is_null());
            let zero = crew_config(0, CHEBYSHEV, 0);
            assert!(sx_crew_new(&zero).is_null(), "zero capacity");
            let mut zero_inbox = crew_config(4, CHEBYSHEV, 0);
            zero_inbox.squad.inbox = 0;
            assert!(sx_crew_new(&zero_inbox).is_null());
            assert!(sx_crew_new(&crew_config(4, 9, 0)).is_null(), "metric");
            let mut nine = crew_config(4, CHEBYSHEV, 0);
            nine.squad.role_count = 9;
            assert!(sx_crew_new(&nine).is_null(), "role count");
            assert!(sx_crew_new(&crew_config(4, 0, 1)).is_null(), "hex, Euclid");
            assert!(sx_crew_new(&crew_config(4, 3, 5)).is_null(), "grid");
            let hex = sx_crew_new(&crew_config(4, 3, 1));
            assert!(!hex.is_null());
            sx_crew_free(hex);
            sx_crew_free(null_mut());
            assert_eq!(sx_crew_config_default(null_mut()), ERROR_ARGUMENT);

            let n = null_mut::<SxCrew>();
            let mut g = SxGoal::default();
            let mut row = SxMemberInputs::default();
            let mut total = 0;
            let m = SxMember::default();
            let centre = [0, 0, 0];
            assert_eq!(sx_crew_add_member(n, &m), ERROR_ARGUMENT);
            assert_eq!(sx_crew_remove_member(n, 1), ERROR_ARGUMENT);
            assert_eq!(sx_crew_suspend(n, 1), ERROR_ARGUMENT);
            assert_eq!(sx_crew_set_gone(n, 1), ERROR_ARGUMENT);
            assert_eq!(sx_crew_reinstate(n, 1, 0), ERROR_ARGUMENT);
            assert_eq!(sx_crew_set_pos(n, 1, centre.as_ptr()), ERROR_ARGUMENT);
            let fact = Fact::default();
            assert_eq!(sx_crew_observe(n, 0, &fact, null_mut()), ERROR_ARGUMENT);
            let r = sx_crew_report_danger(n, 1, centre.as_ptr(), null(), 0);
            assert_eq!(r, ERROR_ARGUMENT);
            let r = sx_crew_tick(n, 0, null(), 0, null(), 0, 0, null_mut(), 0, &mut total);
            assert_eq!(r, ERROR_ARGUMENT);
            assert_eq!(sx_crew_member_inputs(n, 1, 0, &mut row), ERROR_ARGUMENT);
            assert_eq!(sx_crew_member_chose(n, 1, 0, 1), ERROR_ARGUMENT);
            assert_eq!(sx_crew_goal(n, 1, &mut g), ERROR_ARGUMENT);
            assert!(sx_crew_squad(n).is_null());
            assert!(sx_crew_keeper(n, 1).is_null());

            // Unknown ids, null required pointers, a bad member.
            let c = sx_crew_new(&crew_config(2, CHEBYSHEV, 0));
            assert_eq!(sx_crew_add_member(c, null()), ERROR_ARGUMENT);
            let bad = SxMember {
                state: 7,
                ..SxMember::default()
            };
            assert_eq!(sx_crew_add_member(c, &bad), ERROR_ARGUMENT);
            assert_eq!(sx_crew_add_member(c, &m), 0);
            assert_eq!(sx_crew_add_member(c, &m), ERROR_ARGUMENT, "id taken");
            assert_eq!(sx_crew_remove_member(c, 9), 0);
            assert_eq!(sx_crew_suspend(c, 9), 0);
            assert_eq!(sx_crew_set_gone(c, 9), 0);
            assert_eq!(sx_crew_reinstate(c, 9, 0), 0);
            assert_eq!(sx_crew_set_pos(c, 9, centre.as_ptr()), 0);
            assert_eq!(sx_crew_set_pos(c, 0, null()), ERROR_ARGUMENT);
            assert_eq!(sx_crew_member_inputs(c, 9, 0, &mut row), 0);
            assert_eq!(sx_crew_member_inputs(c, 0, 0, null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_crew_member_chose(c, 9, 0, 1), 0);
            assert_eq!(sx_crew_goal(c, 9, &mut g), 0);
            assert_eq!(sx_crew_goal(c, 0, &mut g), 0, "no goal yet");
            assert_eq!(sx_crew_goal(c, 0, null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_crew_observe(c, 0, null(), null_mut()), ERROR_ARGUMENT);
            let r = sx_crew_report_danger(c, 0, null(), null(), 0);
            assert_eq!(r, ERROR_ARGUMENT);
            let r = sx_crew_report_danger(c, 0, centre.as_ptr(), null(), 0);
            assert_eq!(r, 1);
            assert_eq!(sx_crew_member_inputs(c, 0, 0, &mut row), 1);
            assert_eq!((row.has_goal, row.dist), (0, -1));
            sx_crew_free(c);
        }
    }

    #[test]
    fn crew_plan_error_comes_through() {
        unsafe {
            let c = sx_crew_new(&crew_config(2, CHEBYSHEV, 0));
            assert_eq!(sx_crew_add_member(c, &SxMember::default()), 0);
            let sq = sx_crew_squad(c);
            let t = sx_tactic_new(65535, 0, 0);
            let cons = crate::decide::SxConsideration {
                input: 99,
                ..Default::default()
            };
            assert_eq!(sx_tactic_add_consideration(t, &cons), 0);
            assert_eq!(sx_squad_add_tactic(sq, t), 0);
            let mut total = 0;
            let r = sx_crew_tick(c, 0, null(), 0, null(), 0, 0, null_mut(), 0, &mut total);
            assert_eq!(r, ERROR_PLAN);
            sx_crew_free(c);
        }
    }

    #[test]
    fn crew_layout() {
        assert_eq!(size_of::<SxCrewEvent>(), 88);
        assert_eq!(offset_of!(SxCrewEvent, kind), 0);
        assert_eq!(offset_of!(SxCrewEvent, member), 4);
        assert_eq!(offset_of!(SxCrewEvent, plan), 8);
        assert_eq!(offset_of!(SxCrewEvent, start_at), 12);
        assert_eq!(offset_of!(SxCrewEvent, goal), 16);
        assert_eq!(offset_of!(SxCrewEvent, has_start_at), 84);
        assert_eq!(size_of::<SxCrewConfig>(), 72);
        assert_eq!(offset_of!(SxSquadConfig, role_count), 24);
        assert_eq!(offset_of!(SxSquadConfig, role_masks), 28);
        assert_eq!(offset_of!(SxSquadConfig, rule_columns), 60);
        assert_eq!(offset_of!(SxCrewConfig, squad), 0);
        assert_eq!(offset_of!(SxCrewConfig, arrive_radius), 64);
        assert_eq!(offset_of!(SxCrewConfig, hex_grid), 68);
        assert_eq!(size_of::<SxMemberInputs>(), 20);
        let names = [
            "SX_CREW_PLANNED",
            "SX_CREW_ACTIVATED",
            "SX_CREW_SIGNAL",
            "SX_CREW_ORDER",
            "SX_CREW_START",
            "SX_CREW_ARRIVED",
            "SX_CREW_BROKEN",
            "SX_CREW_NO_PLAN",
        ];
        for (i, name) in names.iter().enumerate() {
            assert_eq!(define(name), i as i64 + 1, "{name}");
        }
        let mut c = SxCrewConfig {
            arrive_radius: 9,
            hex_grid: 9,
            ..SxCrewConfig::default()
        };
        let mut squad = SxSquadConfig::default();
        unsafe {
            assert_eq!(sx_crew_config_default(&mut c), 0);
            assert_eq!(sx_squad_config_default(&mut squad), 0);
        }
        assert_eq!(c.squad, squad);
        assert_eq!((c.arrive_radius, c.hex_grid), (0, 0));
    }
}

mod path_cost {
    use super::*;
    use crate::grid::{sx_grid_free, sx_grid_map, sx_grid_new, sx_grid_set};
    use crate::rules::sx_path_cost;
    use sagax_nav::grid::DenseGrid;

    /// A 10 x 10 grid with a wall at x = 5 for y 0..=8 (open at y = 9).
    unsafe fn walled() -> *mut crate::grid::SxGrid {
        unsafe {
            let grid = sx_grid_new(10, 10, 1);
            assert!(!grid.is_null());
            for y in 0..=8 {
                assert_eq!(sx_grid_set(grid, Point::new(5, y, 0), -1), 0);
            }
            grid
        }
    }

    #[test]
    fn c_path_cost_same_as_rust() {
        unsafe {
            let grid = walled();
            let mut map = SxMap {
                step: None,
                start_step: None,
                move_cost: None,
                steps: None,
                context: std::ptr::null_mut(),
            };
            assert_eq!(sx_grid_map(grid, &mut map), 0);
            let mut r = SxRules::default();
            assert_eq!(sx_rules_default(0, 1, &mut r), 0);
            let nav = sx_nav_new(&r);
            assert!(!nav.is_null());

            // The Rust side: the same grid, the same preset.
            let mut dense = DenseGrid::new(10, 10);
            for y in 0..=8 {
                dense.set(5, y, DenseGrid::BLOCKED);
            }
            let mut rust = Navigator::new(
                Topology::Square(GridRules::preset(Preset::Octile)),
                r.node_capacity as usize,
            );
            let report = rust.find_path(
                &mut dense,
                Cell::new(0, 0, 0),
                &[Cell::new(9, 0, 0)],
                sagax_nav::Budget::UNLIMITED,
            );
            assert_eq!(report.status, sagax_nav::nav::Status::Found);

            let (from, to) = ([0, 0, 0], [9, 0, 0]);
            let mut cost = -7i64;
            assert_eq!(
                sx_path_cost(nav, &map, from.as_ptr(), to.as_ptr(), 0, &mut cost),
                1
            );
            assert_eq!(cost, i64::from(report.cost));

            // Not reachable: the cost is left alone.
            let mut cost = -7i64;
            let blocked = [5, 3, 0];
            assert_eq!(
                sx_path_cost(nav, &map, from.as_ptr(), blocked.as_ptr(), 0, &mut cost),
                0
            );
            assert_eq!(cost, -7);
            // Budget used up first.
            assert_eq!(
                sx_path_cost(nav, &map, from.as_ptr(), to.as_ptr(), 1, &mut cost),
                0
            );

            // Bad arguments.
            let null = std::ptr::null();
            assert_eq!(
                sx_path_cost(
                    std::ptr::null_mut(),
                    &map,
                    from.as_ptr(),
                    to.as_ptr(),
                    0,
                    &mut cost
                ),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_path_cost(nav, &map, null, to.as_ptr(), 0, &mut cost),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_path_cost(nav, &map, from.as_ptr(), null, 0, &mut cost),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_path_cost(nav, &map, from.as_ptr(), to.as_ptr(), -1, &mut cost),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_path_cost(
                    nav,
                    &map,
                    from.as_ptr(),
                    to.as_ptr(),
                    0,
                    std::ptr::null_mut()
                ),
                ERROR_ARGUMENT
            );
            sx_nav_free(nav);
            sx_grid_free(grid);
        }
    }
}
