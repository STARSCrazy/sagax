#![allow(deprecated)]

use super::*;
use sagax_abi::codes::{IDLE, PARTIAL, RUNNING};
use sagax_nav::grid::GridRules;
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
fn version_is_15() {
    assert_eq!(sx_api_version(), 16);
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

unsafe extern "C" fn record(ctx: *mut c_void, event: *const SxSearchTrace) {
    let events = unsafe { &mut *ctx.cast::<Vec<SxSearchTrace>>() };
    events.push(unsafe { *event });
}

unsafe extern "C" fn record_nav(ctx: *mut c_void, event: *const SxNavTrace) {
    let events = unsafe { &mut *ctx.cast::<Vec<SxNavTrace>>() };
    events.push(unsafe { *event });
}

/// `sx_nav_find_path` from (2, 2) to (18, 2) as `output`.
fn find(nav: *mut SxNav, map: &SxMap, output: u32) -> (Vec<Point>, SxResult) {
    let mut points = [Point::default(); 64];
    let mut result = SxResult::default();
    let n = unsafe {
        sx_nav_find_path(
            nav,
            map,
            p(2, 2),
            &p(18, 2),
            1,
            0,
            output,
            points.as_mut_ptr(),
            64,
            &mut result,
        )
    };
    (points[..n as usize].to_vec(), result)
}

#[test]
fn tracing_reports_steps_and_turns_off() {
    use sagax_abi::codes::{NAV_TRACE_SEARCH, NAV_TRACE_SHORTCUT, SEARCH_TRACE_EXPANDED};
    let mut w = wall_line();
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut events: Vec<SxNavTrace> = Vec::new();
    let ctx = (&mut events as *mut Vec<SxNavTrace>).cast();
    assert_eq!(unsafe { sx_nav_set_trace(nav, Some(record_nav), ctx) }, 0);
    assert_eq!(
        unsafe { sx_nav_set_trace(std::ptr::null_mut(), Some(record_nav), ctx) },
        ERROR_ARGUMENT
    );
    let (way, result) = find(nav, &map, 2);
    let search: Vec<SxSearchTrace> = events
        .iter()
        .filter(|e| e.kind == NAV_TRACE_SEARCH)
        .map(|e| e.search)
        .collect();
    let expanded = search
        .iter()
        .filter(|e| e.kind == SEARCH_TRACE_EXPANDED)
        .count();
    assert_eq!(expanded as i32, result.expanded);
    assert_eq!((search[0].kind, search[0].cell), (1, p(2, 2)));
    let last = *search.last().unwrap();
    assert_eq!((last.kind, last.found), (5, 1));
    // the smoothed way: its line tests follow the search
    let shortcuts = events
        .iter()
        .skip_while(|e| e.kind == NAV_TRACE_SEARCH)
        .inspect(|e| assert_eq!(e.kind, NAV_TRACE_SHORTCUT))
        .count();
    assert!(shortcuts > 0);

    unsafe { sx_nav_set_trace(nav, None, std::ptr::null_mut()) };
    let before = events.len();
    assert_eq!(find(nav, &map, 2), (way, result));
    assert_eq!(events.len(), before);
    unsafe { sx_nav_free(nav) };

    let d = unsafe { sx_dstar_new(&rules(0)) };
    let mut points = [Point::default(); 64];
    let mut result = SxResult::default();
    let mut events: Vec<SxSearchTrace> = Vec::new();
    unsafe {
        sx_dstar_set_trace(
            d,
            Some(record),
            (&mut events as *mut Vec<SxSearchTrace>).cast(),
        )
    };
    unsafe { sx_dstar_begin(d, p(2, 2), p(18, 2)) };
    unsafe { sx_dstar_compute(d, &map, 0, 0, points.as_mut_ptr(), 64, &mut result) };
    assert_eq!(result.status, FOUND);
    assert!(events.iter().filter(|e| e.kind == 2).count() > 10);
    assert_eq!(events.last().map(|e| (e.kind, e.found)), Some((5, 1)));
    let before = events.len();
    unsafe { sx_dstar_set_trace(d, None, std::ptr::null_mut()) };
    unsafe { sx_dstar_begin(d, p(2, 2), p(18, 2)) };
    unsafe { sx_dstar_compute(d, &map, 0, 0, points.as_mut_ptr(), 64, &mut result) };
    assert_eq!(events.len(), before);
    unsafe { sx_dstar_free(d) };

    let flow = unsafe { sx_flow_new(&rules(0)) };
    let mut events: Vec<SxSearchTrace> = Vec::new();
    unsafe {
        sx_flow_set_trace(
            flow,
            Some(record),
            (&mut events as *mut Vec<SxSearchTrace>).cast(),
        )
    };
    unsafe { sx_flow_build(flow, &map, [p(18, 2)].as_ptr(), 1, 100_000, &mut result) };
    assert_eq!(
        events.iter().filter(|e| e.kind == 2).count() as i32,
        result.cells
    );
    let before = events.len();
    unsafe { sx_flow_set_trace(flow, None, std::ptr::null_mut()) };
    unsafe { sx_flow_build(flow, &map, [p(18, 2)].as_ptr(), 1, 100_000, &mut result) };
    assert_eq!(events.len(), before);
    assert_eq!(
        unsafe { sx_flow_set_trace(std::ptr::null_mut(), None, std::ptr::null_mut()) },
        ERROR_ARGUMENT
    );
    assert_eq!(
        unsafe { sx_dstar_set_trace(std::ptr::null_mut(), None, std::ptr::null_mut()) },
        ERROR_ARGUMENT
    );
    unsafe { sx_flow_free(flow) };
}

/// Every C nav call with a trace gives the records the Rust `_traced` call
/// gives, converted.
#[test]
fn nav_trace_matches_rust() {
    use sagax_abi::codes::{
        NAV_TRACE_FLEE_BEST, NAV_TRACE_PLACED, NAV_TRACE_SEARCH, NAV_TRACE_SHORTCUT,
    };
    use sagax_nav::nav::Output;
    let mut w = wall_line();
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_new(&rules(0)) };
    let mut events: Vec<SxNavTrace> = Vec::new();
    let ctx = (&mut events as *mut Vec<SxNavTrace>).cast();
    assert_eq!(unsafe { sx_nav_set_trace(nav, Some(record_nav), ctx) }, 0);
    let mut points = [Point::default(); 64];
    let mut result = SxResult::default();
    let threats = [p(4, 2)];
    let mut table = [Point::default(); 4 * 64];
    unsafe {
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
        );
        sx_nav_formation(nav, &map, std::ptr::null(), 3, table.as_mut_ptr(), 64);
        sx_nav_flee(
            nav,
            &map,
            p(2, 2),
            threats.as_ptr(),
            1,
            60,
            0,
            points.as_mut_ptr(),
            64,
            &mut result,
        );
    }
    unsafe { sx_nav_free(nav) };

    let mut rust: Vec<SxNavTrace> = Vec::new();
    let (topology, capacity, tolerance) = rules_of(&rules(0)).unwrap();
    let mut navigator = Navigator::new(topology, capacity);
    navigator.set_smooth_max_extra(tolerance);
    let mut map = CallbackMap::new(&map).unwrap();
    let mut sink = CallbackSink(|e| rust.push(SxNavTrace::of(e)));
    navigator.find_path_traced(
        &mut map,
        Cell::new(2, 2, 0),
        &[Cell::new(18, 2, 0)],
        sagax_nav::Budget::UNLIMITED,
        &mut sink,
    );
    navigator.output_traced(&mut map, Output::Smooth, &mut sink);
    navigator.formation_traced(&mut map, &Formation::block_of_four(), &mut sink);
    navigator.flee_traced(
        &mut map,
        Cell::new(2, 2, 0),
        &[Cell::new(4, 2, 0)],
        60,
        &mut sink,
    );
    navigator.output_traced(&mut map, Output::Cells, &mut sink);
    assert_eq!(events, rust);
    for kind in [
        NAV_TRACE_SEARCH,
        NAV_TRACE_SHORTCUT,
        NAV_TRACE_PLACED,
        NAV_TRACE_FLEE_BEST,
    ] {
        assert!(events.iter().any(|e| e.kind == kind), "kind {kind}");
    }
    assert_eq!(events.last().map(|e| e.kind), Some(NAV_TRACE_FLEE_BEST));
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
        assert_eq!(define("SX_API_VERSION"), 16);
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
            ("SX_REASON_TRANSITION", i64::from(REASON_TRANSITION)),
            ("SX_REASON_TIMEOUT", i64::from(REASON_TIMEOUT)),
            ("SX_REASON_SET", i64::from(REASON_SET)),
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
            ("SX_ROUTE_NEAREST", i64::from(ROUTE_NEAREST)),
            ("SX_ROUTE_IN_RANGE", i64::from(ROUTE_IN_RANGE)),
            ("SX_ROUTE_ALL", i64::from(ROUTE_ALL)),
            ("SX_ROUTE_TO", i64::from(ROUTE_TO)),
            ("SX_SINE_FINE", i64::from(SINE_FINE)),
            ("SX_SINE_COARSE", i64::from(SINE_COARSE)),
            ("SX_EVICT_OLDEST", i64::from(EVICT_OLDEST)),
            ("SX_EVICT_WEAKEST", i64::from(EVICT_WEAKEST)),
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
        let mission = [
            ("SX_WHERE_FIXED", WHERE_FIXED),
            ("SX_WHERE_SUBJECT", WHERE_SUBJECT),
            ("SX_WHERE_MOTIVE_PLACE", WHERE_MOTIVE_PLACE),
            ("SX_WHERE_MOTIVE_SUBJECT", WHERE_MOTIVE_SUBJECT),
            ("SX_WHERE_HERE", WHERE_HERE),
            ("SX_STEP_GO_TO", STEP_GO_TO),
            ("SX_STEP_WAIT", STEP_WAIT),
            ("SX_STEP_PAUSE", STEP_PAUSE),
            ("SX_STEP_FOLLOW", STEP_FOLLOW),
            ("SX_STEP_WAIT_FOR", STEP_WAIT_FOR),
            ("SX_STEP_SEARCH", STEP_SEARCH),
            ("SX_STEP_HOLD", STEP_HOLD),
            ("SX_STEP_ATTACK", STEP_ATTACK),
            ("SX_STEP_WANDER", STEP_WANDER),
            ("SX_STEP_SIGNAL", STEP_SIGNAL),
            ("SX_STEP_CUSTOM", STEP_CUSTOM),
            ("SX_STEP_CALL", STEP_CALL),
            ("SX_STEP_SQUAD", STEP_SQUAD),
            ("SX_FAIL_UNKNOWN", FAIL_UNKNOWN),
            ("SX_FAIL_DEFEATED", FAIL_DEFEATED),
            ("SX_FAIL_FACT", FAIL_FACT),
            ("SX_FAIL_FLAGS", FAIL_FLAGS),
            ("SX_FAIL_CUSTOM", FAIL_CUSTOM),
            ("SX_MOTIVE_GUARD", MOTIVE_GUARD),
            ("SX_MOTIVE_DEFEAT", MOTIVE_DEFEAT),
            ("SX_MOTIVE_REACH", MOTIVE_REACH),
            ("SX_MOTIVE_CUSTOM", MOTIVE_CUSTOM),
            ("SX_ON_LOST_RETURN", ON_LOST_RETURN),
            ("SX_ON_LOST_SEARCH", ON_LOST_SEARCH),
            ("SX_ON_LOST_KEEP", ON_LOST_KEEP),
            ("SX_LEEWAY_STRICT", LEEWAY_STRICT),
            ("SX_LEEWAY_FLEXIBLE", LEEWAY_FLEXIBLE),
            ("SX_LEEWAY_FREE", LEEWAY_FREE),
            ("SX_DETOUR_ENGAGE", DETOUR_ENGAGE),
            ("SX_DETOUR_FLEE", DETOUR_FLEE),
            ("SX_DETOUR_CUSTOM", DETOUR_CUSTOM),
            ("SX_ACTION_NONE", ACTION_NONE),
            ("SX_ACTION_GO_TO", ACTION_GO_TO),
            ("SX_ACTION_WAIT", ACTION_WAIT),
            ("SX_ACTION_FOLLOW", ACTION_FOLLOW),
            ("SX_ACTION_HOLD", ACTION_HOLD),
            ("SX_ACTION_ATTACK", ACTION_ATTACK),
            ("SX_ACTION_SEARCH", ACTION_SEARCH),
            ("SX_ACTION_WANDER", ACTION_WANDER),
            ("SX_ACTION_FLEE", ACTION_FLEE),
            ("SX_ACTION_CUSTOM", ACTION_CUSTOM),
            ("SX_MISSION_EVENT_STEP_DONE", MISSION_EVENT_STEP_DONE),
            ("SX_MISSION_EVENT_LOOPED", MISSION_EVENT_LOOPED),
            ("SX_MISSION_EVENT_PLAN_DONE", MISSION_EVENT_PLAN_DONE),
            ("SX_MISSION_EVENT_BROKEN", MISSION_EVENT_BROKEN),
            ("SX_MISSION_EVENT_REPLANNED", MISSION_EVENT_REPLANNED),
            ("SX_MISSION_EVENT_FELL_BACK", MISSION_EVENT_FELL_BACK),
            (
                "SX_MISSION_EVENT_DETOUR_STARTED",
                MISSION_EVENT_DETOUR_STARTED,
            ),
            ("SX_MISSION_EVENT_DETOUR_ENDED", MISSION_EVENT_DETOUR_ENDED),
            ("SX_MISSION_EVENT_RETURNED", MISSION_EVENT_RETURNED),
            ("SX_MISSION_EVENT_SIGNAL", MISSION_EVENT_SIGNAL),
            ("SX_MISSION_EVENT_CALL", MISSION_EVENT_CALL),
            ("SX_MISSION_EVENT_WANTS_SQUAD", MISSION_EVENT_WANTS_SQUAD),
            ("SX_SEARCH_TRACE_STARTED", SEARCH_TRACE_STARTED),
            ("SX_SEARCH_TRACE_EXPANDED", SEARCH_TRACE_EXPANDED),
            ("SX_SEARCH_TRACE_IMPROVED", SEARCH_TRACE_IMPROVED),
            ("SX_SEARCH_TRACE_PAUSED", SEARCH_TRACE_PAUSED),
            ("SX_SEARCH_TRACE_FINISHED", SEARCH_TRACE_FINISHED),
            ("SX_NAV_TRACE_SEARCH", NAV_TRACE_SEARCH),
            ("SX_NAV_TRACE_SHORTCUT", NAV_TRACE_SHORTCUT),
            ("SX_NAV_TRACE_PLACED", NAV_TRACE_PLACED),
            ("SX_NAV_TRACE_UNPLACED", NAV_TRACE_UNPLACED),
            ("SX_NAV_TRACE_FLEE_BEST", NAV_TRACE_FLEE_BEST),
            ("SX_MISSION_TRACE_COLUMN", MISSION_TRACE_COLUMN),
            ("SX_MISSION_TRACE_DECIDE", MISSION_TRACE_DECIDE),
            ("SX_MISSION_TRACE_RESCORED", MISSION_TRACE_RESCORED),
            ("SX_MISSION_TRACE_PICKED", MISSION_TRACE_PICKED),
            ("SX_MISSION_TRACE_BEGIN", MISSION_TRACE_BEGIN),
            ("SX_DECIDE_TRACE_FACTOR", DECIDE_TRACE_FACTOR),
            ("SX_DECIDE_TRACE_SCORED", DECIDE_TRACE_SCORED),
            ("SX_DECIDE_TRACE_CHOSEN", DECIDE_TRACE_CHOSEN),
            ("SX_DECIDE_TRACE_RANKED", DECIDE_TRACE_RANKED),
            ("SX_MACHINE_TRACE_CHANGED", MACHINE_TRACE_CHANGED),
            ("SX_MACHINE_TRACE_WAITING", MACHINE_TRACE_WAITING),
            ("SX_POSTED", POSTED),
            ("SX_REFUSED_FULL", REFUSED_FULL),
            ("SX_REFUSED_COOLDOWN", REFUSED_COOLDOWN),
            ("SX_KNOWLEDGE_TRACE_ADDED", KNOWLEDGE_TRACE_ADDED),
            ("SX_KNOWLEDGE_TRACE_UPDATED", KNOWLEDGE_TRACE_UPDATED),
            ("SX_KNOWLEDGE_TRACE_STALE", KNOWLEDGE_TRACE_STALE),
            ("SX_KNOWLEDGE_TRACE_EVICTED", KNOWLEDGE_TRACE_EVICTED),
            ("SX_KNOWLEDGE_TRACE_EXPIRED", KNOWLEDGE_TRACE_EXPIRED),
            ("SX_KNOWLEDGE_TRACE_REMOVED", KNOWLEDGE_TRACE_REMOVED),
            ("SX_AREAS_TRACE_ADDED", AREAS_TRACE_ADDED),
            ("SX_AREAS_TRACE_RENEWED", AREAS_TRACE_RENEWED),
            ("SX_AREAS_TRACE_EVICTED", AREAS_TRACE_EVICTED),
            ("SX_AREAS_TRACE_EXPIRED", AREAS_TRACE_EXPIRED),
            ("SX_TIMERS_TRACE_STARTED", TIMERS_TRACE_STARTED),
            ("SX_TIMERS_TRACE_RENEWED", TIMERS_TRACE_RENEWED),
            ("SX_TIMERS_TRACE_REPLACED", TIMERS_TRACE_REPLACED),
            ("SX_MESSAGE_TRACE_POSTED", MESSAGE_TRACE_POSTED),
            ("SX_MESSAGE_TRACE_REFUSED", MESSAGE_TRACE_REFUSED),
            ("SX_MESSAGE_TRACE_QUEUED", MESSAGE_TRACE_QUEUED),
            ("SX_MESSAGE_TRACE_DROPPED", MESSAGE_TRACE_DROPPED),
            ("SX_SQUAD_TRACE_TARGET", SQUAD_TRACE_TARGET),
            ("SX_SQUAD_TRACE_DECIDE", SQUAD_TRACE_DECIDE),
            ("SX_SQUAD_TRACE_ABORT", SQUAD_TRACE_ABORT),
            ("SX_SQUAD_TRACE_PICKED", SQUAD_TRACE_PICKED),
            ("SX_SQUAD_TRACE_NO_PICK", SQUAD_TRACE_NO_PICK),
            ("SX_SQUAD_TRACE_THINKING", SQUAD_TRACE_THINKING),
            ("SX_SQUAD_TRACE_ASSIGNED", SQUAD_TRACE_ASSIGNED),
            ("SX_SQUAD_TRACE_SPOT_RANKED", SQUAD_TRACE_SPOT_RANKED),
            ("SX_SQUAD_TRACE_SPOT_PICKED", SQUAD_TRACE_SPOT_PICKED),
            ("SX_SQUAD_TRACE_TRIGGER", SQUAD_TRACE_TRIGGER),
            ("SX_SQUAD_TRACE_REPORT", SQUAD_TRACE_REPORT),
            ("SX_SQUAD_TRACE_DELAY", SQUAD_TRACE_DELAY),
            ("SX_SQUAD_TRACE_KNOWLEDGE", SQUAD_TRACE_KNOWLEDGE),
            ("SX_SQUAD_TRACE_AREAS", SQUAD_TRACE_AREAS),
            ("SX_SQUAD_TRACE_INBOX", SQUAD_TRACE_INBOX),
            ("SX_KEEPER_TRACE_CHECKED", KEEPER_TRACE_CHECKED),
            ("SX_CREW_TRACE_SQUAD", CREW_TRACE_SQUAD),
            ("SX_CREW_TRACE_KEEPER", CREW_TRACE_KEEPER),
            ("SX_CREW_TRACE_SENT", CREW_TRACE_SENT),
            ("SX_MESSAGE_TRACE_DELIVERED", MESSAGE_TRACE_DELIVERED),
            ("SX_BREAK_FAIL", BREAK_FAIL),
            ("SX_BREAK_IMPOSSIBLE", BREAK_IMPOSSIBLE),
            ("SX_BREAK_PLAN_ENDED", BREAK_PLAN_ENDED),
            ("SX_DETOUR_END_DEFEATED", DETOUR_END_DEFEATED),
            ("SX_DETOUR_END_FORGOTTEN", DETOUR_END_FORGOTTEN),
            ("SX_DETOUR_END_LOST", DETOUR_END_LOST),
            ("SX_DETOUR_END_LEASH", DETOUR_END_LEASH),
            ("SX_DETOUR_END_SAFE", DETOUR_END_SAFE),
            ("SX_DETOUR_END_ARRIVED", DETOUR_END_ARRIVED),
            ("SX_DETOUR_END_ENDED", DETOUR_END_ENDED),
        ];
        for (name, value) in mission {
            assert_eq!(define(name), i64::from(value), "{name}");
        }
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
            include_str!("mission.rs"),
            include_str!("perception.rs"),
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
            "fn sine_of",
            "fn topology_of",
            "fn role_list",
            "fn place_from",
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

    unsafe extern "C" fn keep<R: Copy>(ctx: *mut std::ffi::c_void, t: *const R) {
        let records = unsafe { &mut *ctx.cast::<Vec<R>>() };
        records.push(unsafe { *t });
    }

    #[test]
    fn lent_memory_handles_trace_only_their_own_calls() {
        use crate::memory::{
            SxAreasTrace, SxKnowledgeTrace, sx_areas_add, sx_areas_free, sx_areas_set_trace,
            sx_knowledge_free, sx_knowledge_learn, sx_knowledge_set_trace,
        };
        unsafe {
            let sq = squad_of(EUCLID, &[(1, 0, 0), (2, 1, 0)]);
            let k = sx_squad_knowledge(sq);
            let a = sx_squad_areas(sq);
            // one handle each per squad
            assert_eq!(k, sx_squad_knowledge(sq));
            assert_eq!(a, sx_squad_areas(sq));
            let mut facts: Vec<SxKnowledgeTrace> = Vec::new();
            let mut areas: Vec<SxAreasTrace> = Vec::new();
            assert_eq!(
                sx_knowledge_set_trace(k, Some(keep), (&raw mut facts).cast()),
                0
            );
            assert_eq!(
                sx_areas_set_trace(a, Some(keep), (&raw mut areas).cast()),
                0
            );
            // The squad's own work is not the lent handle's.
            let seen = SxFact {
                subject: E1,
                kind: ENEMY,
                pos: [5, 0, 0],
                seen_at: 1,
                ..SxFact::default()
            };
            assert_eq!(sx_squad_observe(sq, 1, &seen, null_mut()), 0);
            assert!(facts.is_empty());
            assert_eq!(sx_knowledge_count(k), 1);
            // A call through the lent handle is.
            let told = SxFact {
                subject: E2,
                ..seen
            };
            assert_eq!(sx_knowledge_learn(k, 2, &told, null_mut()), 0);
            assert_eq!(
                facts,
                [SxKnowledgeTrace {
                    kind: KNOWLEDGE_TRACE_ADDED,
                    fact: told
                }]
            );
            let area = SxArea {
                kind: 1,
                radius: 2,
                lifetime: 50,
                ..SxArea::default()
            };
            assert_eq!(sx_areas_add(a, 2, &area), 0);
            assert_eq!(areas.len(), 1);
            assert_eq!(areas[0].kind, AREAS_TRACE_ADDED);
            // Freeing a lent handle does nothing; the squad frees it.
            sx_knowledge_free(k);
            sx_areas_free(a);
            assert_eq!(sx_knowledge_count(k), 2);
            assert_eq!(sx_areas_count(a), 1);
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
    fn reinstate_without_rules_is_traced_too() {
        unsafe {
            let (sq, _) = active_flank();
            // Member 4 on a custom channel: the squad now needs rules.
            assert_eq!(sx_squad_set_channel_custom(sq, 4, 7), 1);
            let mut log: Vec<SxSquadTrace> = Vec::new();
            assert_eq!(sx_squad_set_trace(sq, Some(keep), (&raw mut log).cast()), 0);
            assert_eq!(sx_squad_suspend(sq, 3), 1);
            assert_eq!(sx_squad_reinstate(sq, 3, 6), 1);
            assert_eq!(log.len(), 1);
            assert_eq!(
                (log[0].kind, log[0].member, log[0].has_delay, log[0].delay),
                (SQUAD_TRACE_DELAY, 3, 1, 0)
            );
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
    fn squad_trace_set_call_off() {
        unsafe {
            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0), (2, 1, 0), (3, 0, 1), (4, 1, 1)]);
            assert_eq!(sx_squad_add_rush(sq), 0);
            assert_eq!(sx_squad_add_focus(sq), 1);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            let mut log: Vec<SxSquadTrace> = Vec::new();
            let ctx = (&raw mut log).cast();
            assert_eq!(sx_squad_set_trace(sq, Some(keep), ctx), 0);
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E1, 20, 0), null_mut()), 0);
            assert_eq!(sx_squad_observe(sq, 0, &enemy(E2, 30, 0), null_mut()), 0);
            let learned = log.iter().filter(|e| e.kind == SQUAD_TRACE_KNOWLEDGE);
            assert_eq!(learned.count(), 2);
            log.clear();
            // A report: what the inbox did.
            let report = SxMessage {
                kind: 99,
                sender: 2,
                ..SxMessage::default()
            };
            assert_eq!(sx_squad_report(sq, &report, 1), 1);
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].kind, SQUAD_TRACE_INBOX);
            assert_eq!(
                (log[0].inbox.kind, log[0].inbox.msg, log[0].inbox.at),
                (sagax_abi::codes::MESSAGE_TRACE_QUEUED, report, 1)
            );
            assert_eq!((log[0].member, log[0].msg), (0, SxMessage::default()));
            log.clear();
            let mut id = 0;
            assert_eq!(sx_squad_plan(sq, 1, null(), 0, null(), 0, 0, &mut id), 1);
            // Every pair of tactic and target is scored.
            let scored: Vec<(u32, u32, u8)> = log
                .iter()
                .filter(|e| e.kind == SQUAD_TRACE_DECIDE && e.decide.kind == DECIDE_TRACE_SCORED)
                .map(|e| (e.tactic, e.target, e.has_target))
                .collect();
            assert_eq!(scored, [(0, E1, 1), (0, E2, 1), (1, E1, 1), (1, E2, 1)]);
            let picked: Vec<_> = log
                .iter()
                .filter(|e| e.kind == SQUAD_TRACE_PICKED)
                .collect();
            assert_eq!(picked.len(), 1);
            assert_eq!(
                log.iter()
                    .filter(|e| e.kind == SQUAD_TRACE_ASSIGNED)
                    .count(),
                4
            );
            // Unused fields are 0.
            assert_eq!(
                (picked[0].member, picked[0].cost, picked[0].fact.subject),
                (0, 0, 0)
            );
            assert_eq!(sx_squad_set_trace(sq, None, null_mut()), 0);
            log.clear();
            assert_eq!(sx_squad_plan(sq, 2, null(), 0, null(), 0, 0, &mut id), 1);
            assert!(log.is_empty(), "off is off");
            assert_eq!(
                sx_squad_set_trace(null_mut(), None, null_mut()),
                ERROR_ARGUMENT
            );
            sx_squad_free(sq);
        }
    }

    #[test]
    fn keeper_trace_set_call_off() {
        unsafe {
            let k = sx_keeper_new();
            let goal = SxGoal {
                kind: GOAL_MOVE,
                has_pos: 1,
                ..SxGoal::default()
            };
            assert_eq!(sx_keeper_set(k, 0, &goal), 0);
            let mut log: Vec<SxKeeperTrace> = Vec::new();
            assert_eq!(sx_keeper_set_trace(k, Some(keep), (&raw mut log).cast()), 0);
            assert_eq!(sx_keeper_broken(k, 3, 10), 0);
            assert_eq!(sx_keeper_goal_postponed(k, 4), 0);
            assert_eq!(sx_keeper_broken(k, 20, 10), 1);
            let checked = |waited, broken| SxKeeperTrace {
                kind: KEEPER_TRACE_CHECKED,
                waited,
                limit: 10,
                broken,
            };
            assert_eq!(log, [checked(0, 0), checked(16, 1)]);
            assert_eq!(sx_keeper_set_trace(k, None, null_mut()), 0);
            assert_eq!(sx_keeper_broken(k, 21, 10), 1);
            assert_eq!(log.len(), 2, "off is off");
            assert_eq!(
                sx_keeper_set_trace(null_mut(), None, null_mut()),
                ERROR_ARGUMENT
            );
            sx_keeper_free(k);
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
    const SOURCES: [&str; 13] = [
        include_str!("lib.rs"),
        include_str!("perception.rs"),
        include_str!("contagion.rs"),
        include_str!("goal_guess.rs"),
        include_str!("grid.rs"),
        include_str!("memory.rs"),
        include_str!("mission.rs"),
        include_str!("decide.rs"),
        include_str!("machine.rs"),
        include_str!("group.rs"),
        include_str!("crew.rs"),
        include_str!("rules.rs"),
        include_str!("walk.rs"),
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
        use crate::contagion::{SxContagionConfig, SxContagionTrace, SxDeedInputs, SxDeedRule};
        use crate::crew::{SxCrewConfig, SxCrewEvent, SxCrewTrace, SxMemberInputs};
        use crate::decide::{SxCandidate, SxChoice, SxConsideration, SxDecideTrace};
        use crate::goal_guess::{SxGoalGuessConfig, SxGoalGuessTrace};
        use crate::group::{
            SxGatherCandidate, SxGoal, SxKeeperTrace, SxMember, SxOrder, SxPlace, SxPlan,
            SxSkillTable, SxSlot, SxSpot, SxSquadConfig, SxSquadTrace,
        };
        use crate::machine::{SxChange, SxMachineTrace};
        use crate::memory::{
            SxAlertConfig, SxAlertness, SxArea, SxAreasTrace, SxFact, SxInterval, SxKnowledgeTrace,
            SxMessage, SxMessageTrace, SxNearby, SxPointPattern, SxProgress, SxTimersTrace,
        };
        use crate::mission::{
            SxDetour, SxFail, SxLeeway, SxMissionAction, SxMissionConfig, SxMissionEvent,
            SxMissionFailCtx, SxMissionOut, SxMissionPlanCtx, SxMissionPlanInfo, SxMissionTrace,
            SxMotive, SxStep, SxWhere,
        };
        use crate::perception::{
            SxAlertAmounts, SxAttention, SxAwareness, SxBand, SxBudget, SxChannel, SxCueRule,
            SxDisguiseRule, SxFactKinds, SxLine, SxMedium, SxPercept, SxPerceptionConfig,
            SxPerceptionTrace, SxSeeThrough, SxSenseProfile, SxStats, SxStimulus, SxTarget,
        };
        use crate::rules::{SxChannelCtx, SxCostCtx, SxPlaceCtx, SxSpotCtx, SxViewPlan};
        use crate::{Point, SxNavTrace, SxResult, SxRules, SxSearchTrace};
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
            layout!("sx_deed_rule", SxDeedRule, window),
            layout!("sx_goal_guess_config", SxGoalGuessConfig, unit, forget),
            layout!(
                "sx_goal_guess_trace",
                SxGoalGuessTrace,
                goal,
                moved,
                likelihood,
                chance,
                error
            ),
            layout!("sx_contagion_config", SxContagionConfig, count, deeds),
            layout!("sx_deed_inputs", SxDeedInputs, seen, share, nearest, since),
            layout!(
                "sx_contagion_trace",
                SxContagionTrace,
                ally,
                old,
                bits,
                deed,
                inputs
            ),
            layout!("sx_point", Point, level),
            layout!("sx_rules", SxRules, cost_straight, smooth_max_extra),
            layout!("sx_result", SxResult, node_limit_hit),
            layout!(
                "sx_search_trace",
                SxSearchTrace,
                search,
                cell,
                parent,
                cost,
                estimate,
                expanded,
                found,
                first
            ),
            layout!(
                "sx_nav_trace",
                SxNavTrace,
                search,
                from,
                to,
                cell,
                member,
                step,
                distance,
                cost,
                clear,
                slot
            ),
            layout!("sx_consideration", SxConsideration, max),
            layout!("sx_candidate", SxCandidate, target),
            layout!("sx_choice", SxChoice, score),
            layout!(
                "sx_decide_trace",
                SxDecideTrace,
                candidate,
                consideration,
                input,
                value,
                score,
                product,
                rank,
                cooled
            ),
            layout!("sx_change", SxChange, transition),
            layout!(
                "sx_machine_trace",
                SxMachineTrace,
                change,
                transition,
                elapsed,
                after
            ),
            layout!("sx_fact", SxFact, pos, seen_at, data, source),
            layout!("sx_message", SxMessage, pos, sender, keep_if_sender_gone),
            layout!("sx_area", SxArea, radius, source, blocking, has_source),
            layout!("sx_knowledge_trace", SxKnowledgeTrace, fact),
            layout!("sx_message_trace", SxMessageTrace, msg, at, refused),
            layout!("sx_areas_trace", SxAreasTrace, index, area),
            layout!("sx_timers_trace", SxTimersTrace, duration, old, old_left),
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
            layout!(
                "sx_squad_trace",
                SxSquadTrace,
                tactic,
                target,
                member,
                slot,
                abort,
                plan,
                until,
                score,
                delay,
                spot,
                pos,
                has_target,
                has_delay,
                has_spot,
                fired,
                kept,
                fits,
                cost,
                distance,
                fact,
                msg,
                decide,
                knowledge,
                areas,
                inbox
            ),
            layout!("sx_keeper_trace", SxKeeperTrace, waited, limit, broken),
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
                "sx_crew_trace",
                SxCrewTrace,
                member,
                delay,
                has_delay,
                queued,
                msg,
                keeper,
                squad
            ),
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
                distance,
                tactic,
                want_tags,
                target,
                spot,
                member,
                wish,
                target_pos,
                gather,
                has_target,
                miss
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
            layout!("sx_where", SxWhere, pos, subject),
            layout!("sx_fail", SxFail, id, flags_mask, flags_value),
            layout!(
                "sx_step",
                SxStep,
                at,
                a,
                b,
                fails,
                fail_count,
                skip_if_impossible
            ),
            layout!(
                "sx_motive",
                SxMotive,
                place,
                subject,
                radius,
                has_place,
                has_subject
            ),
            layout!(
                "sx_leeway",
                SxLeeway,
                on_lost,
                lost_radius,
                has_distance,
                widen
            ),
            layout!(
                "sx_mission_config",
                SxMissionConfig,
                metric,
                grid,
                pick_top,
                search
            ),
            layout!(
                "sx_mission_trace",
                SxMissionTrace,
                option,
                column,
                built_in,
                score,
                index,
                value,
                at,
                has_option,
                has_at,
                decide
            ),
            layout!(
                "sx_mission_plan_info",
                SxMissionPlanInfo,
                fail_count,
                fails,
                repeat
            ),
            layout!("sx_detour", SxDetour, at, to, custom, param, has_to),
            layout!(
                "sx_mission_action",
                SxMissionAction,
                pos,
                subject,
                param,
                has_pos
            ),
            layout!(
                "sx_mission_event",
                SxMissionEvent,
                subject,
                pos,
                has_pos,
                has_subject,
                fail,
                motive
            ),
            layout!("sx_mission_out", SxMissionOut, event_count, events),
            layout!(
                "sx_mission_fail_ctx",
                SxMissionFailCtx,
                step,
                candidate,
                motive,
                has_step,
                has_candidate
            ),
            layout!(
                "sx_mission_plan_ctx",
                SxMissionPlanCtx,
                option,
                first,
                motive,
                widening,
                has_first
            ),
            layout!("sx_medium", SxMedium, density, heat_density),
            layout!("sx_channel", SxChannel, fuzz, speed, spread),
            layout!(
                "sx_perception_config",
                SxPerceptionConfig,
                cell_size,
                channels
            ),
            layout!("sx_band", SxBand, gain, instant),
            layout!("sx_cue_rule", SxCueRule, gain, instant, min_clarity, range),
            layout!(
                "sx_sense_profile",
                SxSenseProfile,
                range,
                near,
                far_gain,
                bands,
                base_gain,
                dark,
                bright,
                dark_gain,
                notice_at,
                recognize_at,
                max,
                decay_per_tick,
                hold,
                hysteresis,
                check_interval,
                crowd_gain,
                scanner_range,
                scanner_gain,
                hearing,
                hear_gain,
                cues
            ),
            layout!(
                "sx_stats",
                SxStats,
                notice_scale,
                hearing_scale,
                reaction_ticks
            ),
            layout!("sx_see_through", SxSeeThrough, near, at),
            layout!(
                "sx_disguise_rule",
                SxDisguiseRule,
                see_through,
                has_see_through
            ),
            layout!(
                "sx_target",
                SxTarget,
                point,
                size,
                conspicuity,
                cues,
                disguise,
                heat,
                glow,
                crowd
            ),
            (
                "sx_awareness",
                size_of::<SxAwareness>(),
                vec![
                    ("doubt", 2),
                    ("stage", 4),
                    ("pending", 5),
                    ("flags", 6),
                    ("last", 8),
                    ("pending_since", 12),
                    ("last_seen", 16),
                    ("cues", 20),
                ],
            ),
            ("sx_attention", size_of::<SxAttention>(), vec![]),
            layout!("sx_budget", SxBudget, keep, instant_cap),
            layout!(
                "sx_stimulus",
                SxStimulus,
                source,
                kind,
                point,
                loudness,
                channel,
                disguise,
                at
            ),
            layout!("sx_line", SxLine, has_blocked_at, clarity, blocked_at),
            layout!(
                "sx_percept",
                SxPercept,
                subject,
                sense,
                channel,
                event,
                new_cues,
                point,
                cell,
                cues,
                margin,
                arrive_at,
                at,
                clarity,
                stage,
                placed,
                disguised
            ),
            layout!(
                "sx_fact_kinds",
                SxFactKinds,
                seen_kind,
                heard_kind,
                disguised_kind,
                has_disguised_kind
            ),
            layout!(
                "sx_alert_amounts",
                SxAlertAmounts,
                recognized,
                seen,
                heard,
                cue_bits,
                cue_amount
            ),
            layout!(
                "sx_perception_trace",
                SxPerceptionTrace,
                target,
                why,
                sense,
                channel,
                gain,
                cost,
                range,
                cell,
                clarity,
                distance,
                light,
                crowd,
                doubt,
                band,
                rule,
                from,
                to,
                clear,
                pending,
                has_cell,
                has_cost
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
    const POINTER_STRUCTS: [&str; 7] = [
        "sx_map",
        "sx_sense_map",
        "sx_eye",
        "sx_areas_cost_ctx",
        "sx_row_ctx",
        "sx_squad_rules",
        "sx_mission_rules",
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

    unsafe extern "C" fn keep<R: Copy>(ctx: *mut std::ffi::c_void, t: *const R) {
        let records = unsafe { &mut *ctx.cast::<Vec<R>>() };
        records.push(unsafe { *t });
    }

    #[test]
    fn crew_trace_same_as_rust_and_lent_handles_trace_their_own_calls() {
        use sagax_decide::group::BuiltInRules;
        // Rust: the traced ticks, as C records.
        let mut crew = rust_crew();
        let spots = spots();
        let mut walks = [Walk::default(); 4];
        let mut events = Vec::new();
        let mut rust_keys = Vec::new();
        let mut rust = Vec::new();
        for t in 0..=80 {
            for (i, (id, ..)) in MEMBERS.iter().enumerate() {
                let g = crew.squad().group();
                let at = g.get(g.index(*id).unwrap()).unwrap().pos;
                if let Some(p) = walks[i].next(t, at) {
                    assert!(crew.set_pos(*id, p));
                }
            }
            let mut sink = sagax_core::CallbackSink(|e| rust.push(SxCrewTrace::of(e)));
            crew.tick_traced(
                t,
                &[],
                &spots,
                SEED,
                &mut BuiltInRules,
                &mut events,
                &mut sink,
            )
            .unwrap();
            let keys: Vec<Key> = events.iter().map(rust_key).collect();
            note(&mut walks, t, &keys);
            rust_keys.push(keys);
        }
        assert_eq!(rust_keys, run_rust(80), "tracing changes nothing");

        let c = c_crew();
        let mut traces: Vec<SxCrewTrace> = Vec::new();
        let mut squad: Vec<SxSquadTrace> = Vec::new();
        let mut keeper: Vec<SxKeeperTrace> = Vec::new();
        unsafe {
            assert_eq!(
                sx_crew_set_trace(c, Some(keep), (&raw mut traces).cast()),
                0
            );
            let sq = sx_crew_squad(c);
            let k = sx_crew_keeper(c, MEMBERS[1].0);
            assert_eq!(
                sx_squad_set_trace(sq, Some(keep), (&raw mut squad).cast()),
                0
            );
            assert_eq!(
                sx_keeper_set_trace(k, Some(keep), (&raw mut keeper).cast()),
                0
            );
            let mut walks = [Walk::default(); 4];
            let mut c_keys = Vec::new();
            for t in 0..=80 {
                walk_c(c, &walks, t);
                let (_, _, keys) = tick_c(c, t, 16);
                note(&mut walks, t, &keys);
                c_keys.push(keys);
            }
            assert_eq!(c_keys, rust_keys);
            assert_eq!(traces, rust);
            // The crew's work goes to the crew trace only.
            assert!(squad.is_empty() && keeper.is_empty());
            let count = |kind| traces.iter().filter(|e| e.kind == kind).count();
            assert!(count(CREW_TRACE_SQUAD) > 0);
            assert!(count(CREW_TRACE_KEEPER) > 0);
            assert!(
                traces
                    .iter()
                    .any(|e| e.kind == CREW_TRACE_SQUAD && e.squad.kind == SQUAD_TRACE_DECIDE)
            );
            // The lent handles trace the calls made through them.
            let n = traces.len();
            let seen = sighting(12, 30, 0);
            assert_eq!(sx_squad_observe(sq, 81, &seen, null_mut()), 0);
            assert_eq!(squad.len(), 1);
            assert_eq!(squad[0].kind, SQUAD_TRACE_KNOWLEDGE);
            // Crew calls make lent keepers stale; asked again, the same
            // handle comes back with its trace still set.
            assert_eq!(sx_crew_keeper(c, MEMBERS[1].0), k);
            assert!(sx_keeper_broken(k, 81, 10) >= 0);
            assert_eq!(keeper.len(), 1);
            assert_eq!(keeper[0].kind, KEEPER_TRACE_CHECKED);
            assert_eq!(traces.len(), n);
            // A report through the crew: what the squad's inbox did, then
            // one SENT record; the squad's own trace sees nothing.
            let msg = Message::enemy(MEMBERS[2].0, sighting(13, 31, 0));
            assert_eq!(sx_crew_report(c, 82, &msg), 1);
            let sent = &traces[n..];
            assert_eq!(sent.len(), 2);
            assert_eq!(
                (sent[0].kind, sent[0].squad.kind, sent[0].squad.inbox.kind),
                (CREW_TRACE_SQUAD, SQUAD_TRACE_INBOX, MESSAGE_TRACE_QUEUED)
            );
            assert_eq!(sent[0].squad.inbox.msg, sent[1].msg);
            assert_eq!(
                (
                    sent[1].kind,
                    sent[1].member,
                    sent[1].has_delay,
                    sent[1].queued
                ),
                (CREW_TRACE_SENT, MEMBERS[2].0, 1, 1)
            );
            assert_eq!(squad.len(), 1);
            assert_eq!(sx_crew_set_trace(c, None, null_mut()), 0);
            assert_eq!(sx_crew_report(c, 83, &msg), 1);
            assert_eq!(traces.len(), n + 2, "off is off");
            assert_eq!(
                sx_crew_set_trace(null_mut(), None, null_mut()),
                ERROR_ARGUMENT
            );
            sx_crew_free(c);
        }
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

mod mission {
    //! `sx_mission_*` (C API 11, getters and trace 12): the Rust `Mission` through C. Each scene
    //! runs a C mission and the same Rust mission side by side and checks
    //! that every tick gives the same action and events.

    use super::squad::define;
    use crate::decide::{
        SxConsideration, SxDecideTrace, sx_decider_add_consideration, sx_decider_free,
        sx_decider_set_trace,
    };
    use crate::memory::{SxKnowledge, sx_knowledge_free, sx_knowledge_learn, sx_knowledge_new};
    use crate::mission::*;
    use sagax_abi::codes::*;
    use sagax_decide::memory::{Fact, PointPattern, Pos};
    use sagax_decide::mission::{
        DEFAULT_PREFERENCE, DIST_CAP, Detour, FailCtx, IN_DIST, IN_EXTRA, IN_FEASIBLE, IN_KNOWN,
        IN_PREF, IN_WIDEN, KNOWN_CAP, Leeway, Mission, MissionConfig, MissionRules, Motive,
        PlanCtx, Step, WIDEN_PREFERENCE,
    };
    use sagax_decide::{ONE, mission::Fail, mission::Plan};
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    const SEEN: u32 = 0;
    const STOLEN: u32 = 1;
    const ITEM: u32 = 20;
    const THIEF: u32 = 30;
    const ATTACKER: u32 = 50;
    const WEST: Pos = [4, 3, 0];
    const EAST: Pos = [12, 3, 0];

    fn at(pos: Pos) -> SxWhere {
        SxWhere {
            kind: WHERE_FIXED,
            pos,
            subject: 0,
        }
    }

    fn c_step(kind: u32, a: u32, b: u32, place: SxWhere) -> SxStep {
        SxStep {
            kind,
            at: place,
            a,
            b,
            ..SxStep::default()
        }
    }

    fn c_go(pos: Pos) -> SxStep {
        c_step(STEP_GO_TO, 0, 0, at(pos))
    }

    fn guard_motive() -> SxMotive {
        SxMotive {
            kind: MOTIVE_GUARD,
            place: WEST,
            has_place: 1,
            ..SxMotive::default()
        }
    }

    fn stolen_fail() -> SxFail {
        SxFail {
            kind: FAIL_FACT,
            subject: ITEM,
            fact_kind: STOLEN,
            mask: 1,
            value: 1,
            ..SxFail::default()
        }
    }

    fn c_config() -> SxMissionConfig {
        let mut c = SxMissionConfig::default();
        assert_eq!(unsafe { sx_mission_config_default(&mut c) }, 0);
        c.id = 7;
        c.lost_after = 3;
        c
    }

    fn rust_config() -> MissionConfig {
        MissionConfig {
            id: 7,
            lost_after: 3,
            ..MissionConfig::default()
        }
    }

    fn fact(subject: u32, kind: u32, pos: Pos, now: u32) -> Fact {
        Fact {
            subject,
            kind,
            pos,
            seen_at: now,
            data: 1,
            ..Fact::default()
        }
    }

    /// One cell from `pos` toward the action's position, if it has one.
    fn toward(pos: &mut Pos, action: &SxMissionAction) {
        if action.has_pos != 0 {
            for (p, t) in pos.iter_mut().zip(action.pos).take(2) {
                *p += (t - *p).signum();
            }
        }
    }

    /// A C mission and a Rust mission that should act the same, at one
    /// position and with one knowledge.
    struct Twin {
        c: *mut SxMission,
        r: Mission,
        k: *mut SxKnowledge,
        pos: Pos,
        trace: Vec<SxMissionOut>,
    }

    impl Twin {
        fn new(c: *mut SxMission, r: Mission, pos: Pos) -> Self {
            assert!(!c.is_null());
            Self {
                c,
                r,
                k: sx_knowledge_new(8, 0),
                pos,
                trace: Vec::new(),
            }
        }

        fn learn(&mut self, now: u32, f: Fact) {
            assert!(unsafe { sx_knowledge_learn(self.k, now, &f, null_mut()) } >= 0);
        }

        fn detour(&mut self, now: u32, c: SxDetour, r: Detour) -> i32 {
            let taken = unsafe { sx_mission_detour(self.c, now, &c, self.k) };
            let k = unsafe { &*(*self.k).value };
            assert_eq!(taken, i32::from(self.r.detour(now, r, k)));
            taken
        }

        /// One tick of both, with `rules` for C and `r_rules` for Rust;
        /// then one cell toward the action's target.
        fn tick_with(
            &mut self,
            now: u32,
            rules: *const SxMissionRules,
            r_rules: &mut dyn MissionRules,
        ) -> SxMissionOut {
            let mut out = SxMissionOut::default();
            let n = unsafe {
                sx_mission_tick_with(self.c, now, self.pos.as_ptr(), self.k, 0, rules, &mut out)
            };
            let k = unsafe { &*(*self.k).value };
            let r = self.r.tick_with(now, self.pos, k, 0, r_rules);
            assert_eq!(out.action, action_of(r.action), "action at {now}");
            let events: Vec<_> = r.events().iter().map(|e| event_of(*e)).collect();
            assert_eq!(n, events.len() as i32, "count at {now}");
            assert_eq!(out.event_count, events.len() as u32);
            assert_eq!(&out.events[..events.len()], &events[..], "events at {now}");
            let mut index = 0;
            let has = unsafe { sx_mission_step_index(self.c, &mut index) };
            assert_eq!(has == 1, self.r.step_index().is_some(), "step at {now}");
            if has == 1 {
                assert_eq!(Some(index as usize), self.r.step_index());
            }
            assert_eq!(
                unsafe { sx_mission_in_detour(self.c) },
                i32::from(self.r.in_detour())
            );
            assert_eq!(
                unsafe { sx_mission_returning(self.c) },
                i32::from(self.r.returning())
            );
            toward(&mut self.pos, &out.action);
            self.trace.push(out);
            out
        }

        fn tick(&mut self, now: u32) -> SxMissionOut {
            self.tick_with(now, null(), &mut sagax_decide::mission::BuiltInRules)
        }

        /// Every event of the trace with this kind.
        fn events(&self, kind: u32) -> Vec<SxMissionEvent> {
            self.trace
                .iter()
                .flat_map(|o| o.events[..o.event_count as usize].to_vec())
                .filter(|e| e.kind == kind)
                .collect()
        }
    }

    impl Drop for Twin {
        fn drop(&mut self) {
            unsafe {
                sx_mission_free(self.c);
                sx_knowledge_free(self.k);
            }
        }
    }

    /// The patrol in both forms: go west, wait 5, go east, again; it
    /// breaks when the item is stolen.
    fn patrol(c: *mut SxMission, r: &mut Mission) {
        let steps = [
            c_go(WEST),
            c_step(STEP_WAIT, 5, 0, SxWhere::default()),
            c_go(EAST),
        ];
        let fails = [stolen_fail()];
        assert_eq!(
            unsafe { sx_mission_set_plan(c, steps.as_ptr(), 3, 1, fails.as_ptr(), 1) },
            0
        );
        let plan = Plan::looping(&[Step::go_to(WEST), Step::wait(5), Step::go_to(EAST)])
            .fails_when(Fail::Fact {
                subject: ITEM,
                kind: STOLEN,
                mask: 1,
                value: 1,
            });
        r.set_plan(&plan).unwrap();
    }

    fn twin(leeway: u32) -> Twin {
        let c = unsafe { sx_mission_new(&c_config(), &guard_motive()) };
        let mut r = Mission::new(rust_config(), Motive::guard_place(WEST)).unwrap();
        let mut l = SxLeeway::default();
        unsafe {
            assert_eq!(sx_leeway_preset(leeway, &mut l), 0);
            assert_eq!(sx_mission_set_leeway(c, &l), 0);
        }
        r.set_leeway(convert_leeway(leeway));
        patrol(c, &mut r);
        Twin::new(c, r, [3, 2, 0])
    }

    fn convert_leeway(which: u32) -> Leeway {
        [Leeway::STRICT, Leeway::FLEXIBLE, Leeway::FREE][which as usize]
    }

    #[test]
    fn defaults_match_rust() {
        let mut c = SxMissionConfig::default();
        unsafe { assert_eq!(sx_mission_config_default(&mut c), 0) };
        let d = MissionConfig::default();
        assert_eq!(
            (c.id, c.max_steps, c.max_playbook, c.subject_kind),
            (0, 16, 8, 0)
        );
        assert_eq!((c.enemy_bits, c.defeated_bits, c.arrive_radius), (1, 2, 0));
        assert_eq!((c.lost_after, c.pause_ticks, c.safe_distance), (30, 50, 12));
        assert_eq!(c.reached_signal, d.reached_signal);
        assert_eq!(
            (c.metric, c.grid, c.extra_columns),
            (METRIC_CHEBYSHEV, 0, 0)
        );
        assert_eq!((c.pick, c.pick_top), (PICK_BEST, 0));
        let s = c.search;
        assert_eq!((s.heading_step, s.first_distance, s.count), (0x62, 1, 3));
        assert_eq!(s.distances[..3], [2, 4, 6]);
        assert_eq!(s.offsets[..3], [0, 0x20, -0x20]);
        assert_eq!(s.sine, 0);
        assert_eq!((s.min_x, s.min_y), (i32::MIN, i32::MIN));
        assert_eq!((s.max_x, s.max_y), (i32::MAX, i32::MAX));
        assert_eq!(
            d.search,
            PointPattern {
                first_distance: 1,
                distances: vec![2, 4, 6],
                ..PointPattern::default()
            }
        );
        for (which, want) in [
            (LEEWAY_STRICT, Leeway::STRICT),
            (LEEWAY_FLEXIBLE, Leeway::FLEXIBLE),
            (LEEWAY_FREE, Leeway::FREE),
        ] {
            let mut l = SxLeeway::default();
            unsafe { assert_eq!(sx_leeway_preset(which, &mut l), 0) };
            let back = Leeway {
                leash: sagax_decide::mission::Leash {
                    distance: (l.has_distance != 0).then_some(l.distance),
                    ticks: (l.has_ticks != 0).then_some(l.ticks),
                    on_lost: sagax_abi::convert::on_lost(l.on_lost, l.lost_ticks, l.lost_radius)
                        .unwrap(),
                },
                replan: l.replan != 0,
                widen: l.widen != 0,
            };
            assert_eq!(back, want);
        }
        let mut l = SxLeeway::default();
        unsafe { assert_eq!(sx_leeway_preset(LEEWAY_STRICT, &mut l), 0) };
        assert_eq!(
            (l.distance, l.has_distance, l.on_lost),
            (3, 1, ON_LOST_RETURN)
        );
        unsafe { assert_eq!(sx_leeway_preset(LEEWAY_FLEXIBLE, &mut l), 0) };
        assert_eq!(
            (l.on_lost, l.lost_ticks, l.lost_radius),
            (ON_LOST_SEARCH, 60, 6)
        );
        let core = [
            ("SX_MISSION_IN_FEASIBLE", IN_FEASIBLE as i64),
            ("SX_MISSION_IN_DIST", IN_DIST as i64),
            ("SX_MISSION_IN_PREF", IN_PREF as i64),
            ("SX_MISSION_IN_WIDEN", IN_WIDEN as i64),
            ("SX_MISSION_IN_KNOWN", IN_KNOWN as i64),
            ("SX_MISSION_IN_EXTRA", IN_EXTRA as i64),
            ("SX_MISSION_DIST_CAP", i64::from(DIST_CAP)),
            ("SX_MISSION_KNOWN_CAP", i64::from(KNOWN_CAP)),
            (
                "SX_MISSION_DEFAULT_PREFERENCE",
                i64::from(DEFAULT_PREFERENCE),
            ),
            ("SX_MISSION_WIDEN_PREFERENCE", i64::from(WIDEN_PREFERENCE)),
            (
                "SX_MISSION_MAX_FAILS",
                sagax_decide::mission::MAX_FAILS as i64,
            ),
            (
                "SX_MISSION_MAX_EVENTS",
                sagax_decide::mission::MAX_EVENTS as i64,
            ),
        ];
        for (name, value) in core {
            assert_eq!(define(name), value, "{name}");
        }
    }

    #[test]
    fn patrol_loops() {
        let mut t = twin(LEEWAY_STRICT);
        for now in 0..40 {
            t.tick(now);
        }
        assert!(t.events(MISSION_EVENT_LOOPED).len() >= 2, "it loops");
        let done: Vec<u32> = t
            .events(MISSION_EVENT_STEP_DONE)
            .iter()
            .map(|e| e.a)
            .collect();
        assert_eq!(done[..3], [0, 1, 2]);
        assert_eq!(t.trace[0].action.kind, ACTION_GO_TO);
        assert_eq!(t.trace[0].action.pos, WEST);
    }

    #[test]
    fn strict_returns() {
        let mut t = twin(LEEWAY_STRICT);
        for now in 0..40 {
            // An attacker near the west end, running east two cells a tick.
            if now >= 8 {
                let x = 6 + 2 * (now as i32 - 8);
                if x < 16 {
                    t.learn(now, fact(ATTACKER, SEEN, [x, 3, 0], now));
                    if now == 8 {
                        let d = SxDetour {
                            kind: DETOUR_ENGAGE,
                            subject: ATTACKER,
                            ..SxDetour::default()
                        };
                        assert_eq!(t.detour(now, d, Detour::Engage(ATTACKER)), 1);
                    }
                }
            }
            t.tick(now);
        }
        assert_eq!(t.events(MISSION_EVENT_DETOUR_STARTED).len(), 1);
        let ended = t.events(MISSION_EVENT_DETOUR_ENDED);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].a, DETOUR_END_LEASH);
        assert_eq!(t.events(MISSION_EVENT_RETURNED).len(), 1);
        assert!(t.events(MISSION_EVENT_REPLANNED).is_empty());
        let attacks = t
            .trace
            .iter()
            .filter(|o| o.action.kind == ACTION_ATTACK)
            .count();
        assert!(attacks > 0);
    }

    #[test]
    fn replan_on_fact() {
        let mut t = twin(LEEWAY_FLEXIBLE);
        let attack = [c_step(STEP_ATTACK, THIEF, 0, SxWhere::default())];
        let option =
            unsafe { sx_mission_add_playbook(t.c, attack.as_ptr(), 1, 0, null(), 0, ONE, 0) };
        assert_eq!(option, 0);
        let r_option =
            t.r.add_playbook(&Plan::once(&[Step::attack(THIEF)]), ONE, false)
                .unwrap();
        assert_eq!(r_option, 0);
        for now in 0..20 {
            if now == 10 {
                t.learn(now, fact(ITEM, STOLEN, [0; 3], now));
                t.learn(now, fact(THIEF, SEEN, [13, 6, 0], now));
            }
            t.tick(now);
        }
        let broken = t.events(MISSION_EVENT_BROKEN);
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].a, BREAK_FAIL);
        assert_eq!(broken[0].fail, stolen_fail());
        let replanned = t.events(MISSION_EVENT_REPLANNED);
        assert_eq!(replanned.len(), 1);
        assert_eq!(replanned[0].a, 0);
        let last = t.trace.last().unwrap().action;
        assert_eq!((last.kind, last.subject), (ACTION_ATTACK, THIEF));
        assert_eq!((last.pos, last.has_pos), ([13, 6, 0], 1));
    }

    #[test]
    fn getters_give_back_what_was_set() {
        unsafe {
            let c = sx_mission_new(&c_config(), &guard_motive());
            let mut motive = SxMotive::default();
            assert_eq!(sx_mission_motive(c, &mut motive), 0);
            assert_eq!(motive, guard_motive());
            let hunt = SxMotive {
                kind: MOTIVE_CUSTOM,
                custom: 9,
                subject: THIEF,
                has_subject: 1,
                radius: 4,
                ..SxMotive::default()
            };
            assert_eq!(sx_mission_set_motive(c, &hunt), 0);
            assert_eq!(sx_mission_motive(c, &mut motive), 0);
            assert_eq!(motive, hunt);

            let mut leeway = SxLeeway::default();
            assert_eq!(sx_mission_leeway(c, &mut leeway), 0);
            let mut flexible = SxLeeway::default();
            assert_eq!(sx_leeway_preset(LEEWAY_FLEXIBLE, &mut flexible), 0);
            assert_eq!(leeway, flexible, "the default leeway");
            let mine = SxLeeway {
                distance: 0,
                ticks: 40,
                on_lost: ON_LOST_SEARCH,
                lost_ticks: 7,
                lost_radius: 2,
                has_distance: 0,
                has_ticks: 1,
                replan: 1,
                widen: 0,
            };
            assert_eq!(sx_mission_set_leeway(c, &mine), 0);
            assert_eq!(sx_mission_leeway(c, &mut leeway), 0);
            assert_eq!(leeway, mine);

            // No plan yet: 0 and nothing written.
            let mut info = SxMissionPlanInfo {
                step_count: 99,
                ..SxMissionPlanInfo::default()
            };
            let mut back = [SxStep::default(); 16];
            assert_eq!(sx_mission_plan(c, &mut info, back.as_mut_ptr(), 16), 0);
            assert_eq!(info.step_count, 99);

            // Every step kind, with conditions, through set and get.
            let subject = SxWhere {
                kind: WHERE_SUBJECT,
                subject: 4,
                ..SxWhere::default()
            };
            let mut steps = Vec::new();
            for kind in STEP_GO_TO..=STEP_SQUAD {
                let has_place = matches!(
                    kind,
                    STEP_GO_TO | STEP_SEARCH | STEP_HOLD | STEP_CUSTOM | STEP_SQUAD
                );
                let uses_a = !matches!(kind, STEP_GO_TO | STEP_HOLD | STEP_PAUSE | STEP_WANDER);
                let uses_b = matches!(kind, STEP_WAIT_FOR | STEP_SEARCH | STEP_CUSTOM | STEP_CALL);
                let place = if has_place {
                    subject
                } else {
                    SxWhere::default()
                };
                let mut s = c_step(kind, u32::from(uses_a) * 3, u32::from(uses_b) * 5, place);
                if kind % 2 == 0 {
                    s.skip_if_impossible = 1;
                    s.fails[0] = stolen_fail();
                    s.fails[1] = SxFail {
                        kind: FAIL_CUSTOM,
                        id: 8,
                        ..SxFail::default()
                    };
                    s.fail_count = 2;
                }
                steps.push(s);
            }
            steps[0] = c_go(EAST);
            let fails = [
                stolen_fail(),
                SxFail {
                    kind: FAIL_FLAGS,
                    flags_mask: 1 << 40,
                    flags_value: 1 << 40,
                    ..SxFail::default()
                },
            ];
            let n = steps.len() as u32;
            assert_eq!(
                sx_mission_set_plan(c, steps.as_ptr(), n, 1, fails.as_ptr(), 2),
                0
            );
            assert_eq!(sx_mission_plan(c, &mut info, back.as_mut_ptr(), 16), 1);
            assert_eq!((info.step_count, info.repeat), (n, 1));
            assert_eq!(info.fail_count, 2);
            assert_eq!(info.fails[..2], fails);
            assert_eq!(info.fails[2], SxFail::default());
            assert_eq!(&back[..n as usize], &steps[..]);
            assert_eq!(back[n as usize], SxStep::default(), "beyond the plan");

            // A short buffer gets the first steps; null with cap 0 is fine.
            let mut two = [SxStep::default(); 2];
            assert_eq!(sx_mission_plan(c, null_mut(), two.as_mut_ptr(), 2), 1);
            assert_eq!(two, [steps[0], steps[1]]);
            assert_eq!(sx_mission_plan(c, &mut info, null_mut(), 0), 1);
            assert_eq!(info.step_count, n);

            // Bad arguments.
            assert_eq!(sx_mission_plan(c, &mut info, null_mut(), 1), -1);
            assert_eq!(sx_mission_plan(null(), &mut info, null_mut(), 0), -1);
            assert_eq!(sx_mission_motive(c, null_mut()), -1);
            assert_eq!(sx_mission_motive(null(), &mut motive), -1);
            assert_eq!(sx_mission_leeway(c, null_mut()), -1);
            assert_eq!(sx_mission_leeway(null(), &mut leeway), -1);
            sx_mission_free(c);
        }
    }

    #[test]
    fn plan_getter_reads_a_replanned_plan() {
        let mut t = twin(LEEWAY_FLEXIBLE);
        t.r.set_plan(&Plan::once(&[Step::wait(1)])).unwrap();
        let wait = [c_step(STEP_WAIT, 1, 0, SxWhere::default())];
        assert_eq!(
            unsafe { sx_mission_set_plan(t.c, wait.as_ptr(), 1, 0, null(), 0) },
            0
        );
        for now in 0..4 {
            t.tick(now);
        }
        assert_eq!(t.events(MISSION_EVENT_REPLANNED).len(), 1);
        let plan = t.r.plan().unwrap();
        let mut info = SxMissionPlanInfo::default();
        let mut back = [SxStep::default(); 4];
        assert_eq!(
            unsafe { sx_mission_plan(t.c, &mut info, back.as_mut_ptr(), 4) },
            1
        );
        assert_eq!(info.step_count as usize, plan.steps().len());
        assert_eq!(info.repeat, u8::from(plan.repeat));
        for (c, r) in back.iter().zip(plan.steps()) {
            assert_eq!(c.step(), Some(*r));
        }
    }

    unsafe extern "C" fn collect_trace(ctx: *mut c_void, e: *const SxMissionTrace) {
        let events = unsafe { &mut *ctx.cast::<Vec<SxMissionTrace>>() };
        events.push(unsafe { *e });
    }

    #[test]
    fn trace_matches_rust() {
        let mut t = twin(LEEWAY_FLEXIBLE);
        let attack = [c_step(STEP_ATTACK, THIEF, 0, SxWhere::default())];
        unsafe { sx_mission_add_playbook(t.c, attack.as_ptr(), 1, 0, null(), 0, ONE, 0) };
        t.r.add_playbook(&Plan::once(&[Step::attack(THIEF)]), ONE, false)
            .unwrap();
        let mut c_events: Vec<SxMissionTrace> = Vec::new();
        let ctx = (&raw mut c_events).cast();
        assert_eq!(
            unsafe { sx_mission_set_trace(t.c, Some(collect_trace), ctx) },
            0
        );
        assert_eq!(
            unsafe { sx_mission_set_trace(null_mut(), None, null_mut()) },
            -1
        );
        // The lent decider's trace covers only calls made through it: the
        // mission's own scorings go to the mission's trace.
        let mut lent: Vec<SxDecideTrace> = Vec::new();
        unsafe extern "C" fn collect_lent(ctx: *mut c_void, e: *const SxDecideTrace) {
            let records = unsafe { &mut *ctx.cast::<Vec<SxDecideTrace>>() };
            records.push(unsafe { *e });
        }
        let decider = unsafe { sx_mission_decider(t.c) };
        let lent_ctx = (&raw mut lent).cast();
        assert_eq!(
            unsafe { sx_decider_set_trace(decider, Some(collect_lent), lent_ctx) },
            0
        );
        let k = t.k;
        let mut r_events = Vec::new();
        let mut pos = t.pos;
        for now in 0..14 {
            if now == 10 {
                t.learn(now, fact(ITEM, STOLEN, [0; 3], now));
                t.learn(now, fact(THIEF, SEEN, [13, 6, 0], now));
            }
            let mut out = SxMissionOut::default();
            unsafe { sx_mission_tick(t.c, now, pos.as_ptr(), k, 0, &mut out) };
            let know = unsafe { &*(*k).value };
            let mut sink = sagax_core::CallbackSink(|e| r_events.push(SxMissionTrace::of(e)));
            let r = t.r.tick_traced(
                now,
                pos,
                know,
                0,
                &mut sagax_decide::mission::BuiltInRules,
                &mut sink,
            );
            assert_eq!(out.action, action_of(r.action));
            toward(&mut pos, &out.action);
        }
        assert_eq!(c_events, r_events);
        // One scoring: the playbook slot and the default plan, then the pick.
        let picked: Vec<_> = c_events
            .iter()
            .filter(|e| e.kind == MISSION_TRACE_PICKED)
            .collect();
        assert_eq!(picked.len(), 1);
        assert_eq!((picked[0].option, picked[0].has_option), (0, 1));
        let scored: Vec<u32> = c_events
            .iter()
            .filter(|e| e.kind == MISSION_TRACE_RESCORED)
            .map(|e| e.option)
            .collect();
        assert_eq!(scored, [0, 8]);
        // The decider's records arrive whole, embedded.
        let decided: Vec<(u32, u32)> = c_events
            .iter()
            .filter(|e| e.kind == MISSION_TRACE_DECIDE && e.decide.kind == DECIDE_TRACE_SCORED)
            .map(|e| (e.option, e.decide.score))
            .collect();
        let rescored: Vec<(u32, u32)> = c_events
            .iter()
            .filter(|e| e.kind == MISSION_TRACE_RESCORED)
            .map(|e| (e.option, e.built_in))
            .collect();
        assert_eq!(decided, rescored);
        // Each step that begins, with its place: the attack has none.
        let begins: Vec<_> = c_events
            .iter()
            .filter(|e| e.kind == MISSION_TRACE_BEGIN)
            .map(|e| (e.index, e.has_at))
            .collect();
        assert!(begins.contains(&(0, 0)), "{begins:?}");
        assert!(lent.is_empty());
        let columns = c_events
            .iter()
            .filter(|e| e.kind == MISSION_TRACE_COLUMN)
            .count();
        assert_eq!(columns, 2 * IN_EXTRA);

        // Off again: nothing more arrives.
        let before = c_events.len();
        unsafe {
            assert_eq!(sx_mission_set_trace(t.c, None, null_mut()), 0);
            let mut out = SxMissionOut::default();
            let empty = sx_knowledge_new(4, 0);
            // A fresh replan: the knowledge is empty, the thief unknown.
            let steps = [c_step(STEP_WAIT, 1, 0, SxWhere::default())];
            sx_mission_set_plan(t.c, steps.as_ptr(), 1, 0, null(), 0);
            for now in 14..18 {
                sx_mission_tick(t.c, now, pos.as_ptr(), empty, 0, &mut out);
            }
            sx_knowledge_free(empty);
        }
        assert_eq!(c_events.len(), before);
    }

    #[test]
    fn call_carries_the_motive() {
        let mut t = twin(LEEWAY_FREE);
        let steps = [c_step(STEP_CALL, 2, 3, SxWhere::default()), c_go(EAST)];
        assert_eq!(
            unsafe { sx_mission_set_plan(t.c, steps.as_ptr(), 2, 0, null(), 0) },
            0
        );
        t.r.set_plan(&Plan::once(&[Step::call(2, 3), Step::go_to(EAST)]))
            .unwrap();
        t.tick(0);
        let calls = t.events(MISSION_EVENT_CALL);
        assert_eq!(calls.len(), 1);
        assert_eq!((calls[0].a, calls[0].b), (2, 3));
        assert_eq!((calls[0].pos, calls[0].has_pos), ([3, 2, 0], 1));
        assert_eq!(calls[0].motive, guard_motive());
    }

    /// What the C callbacks saw, and what they answer.
    #[derive(Default)]
    struct Seen {
        fails: Vec<(u32, u8, u32)>,
        columns: Vec<(u32, u32)>,
        scores: Vec<(u32, u32)>,
    }

    unsafe extern "C" fn c_fail(user: *mut c_void, ctx: *const SxMissionFailCtx, id: u32) -> i32 {
        let seen = unsafe { &mut *user.cast::<Seen>() };
        let ctx = unsafe { &*ctx };
        seen.fails.push((ctx.now, ctx.has_step, id));
        i32::from(id == 9 && ctx.now >= 6)
    }

    unsafe extern "C" fn c_columns(
        user: *mut c_void,
        ctx: *const SxMissionPlanCtx,
        out: *mut i32,
        count: u32,
    ) {
        let seen = unsafe { &mut *user.cast::<Seen>() };
        let option = unsafe { (*ctx).option };
        seen.columns.push((option, count));
        for i in 0..count as usize {
            unsafe { out.add(i).write(option as i32 + 1) };
        }
    }

    unsafe extern "C" fn c_score(
        user: *mut c_void,
        ctx: *const SxMissionPlanCtx,
        built_in: u32,
        score: *mut u32,
    ) -> i32 {
        let seen = unsafe { &mut *user.cast::<Seen>() };
        let option = unsafe { (*ctx).option };
        seen.scores.push((option, built_in));
        // Veto the playbook slot: the default plan wins.
        if option == 0 {
            unsafe { score.write(0) };
            1
        } else {
            0
        }
    }

    /// The same rules in Rust.
    struct Same;

    impl MissionRules for Same {
        fn fail(&mut self, ctx: &FailCtx, id: u32) -> bool {
            id == 9 && ctx.now >= 6
        }
        fn columns(&mut self, ctx: &PlanCtx, out: &mut [i32]) {
            out.fill(ctx.option as i32 + 1);
        }
        fn score(&mut self, ctx: &PlanCtx, _built_in: u32) -> Option<u32> {
            (ctx.option == 0).then_some(0)
        }
    }

    #[test]
    fn rules_through_c() {
        let mut cfg = c_config();
        cfg.extra_columns = 2;
        let c = unsafe { sx_mission_new(&cfg, &guard_motive()) };
        let r = Mission::new(
            MissionConfig {
                extra_columns: 2,
                ..rust_config()
            },
            Motive::guard_place(WEST),
        )
        .unwrap();
        let mut t = Twin::new(c, r, [3, 2, 0]);
        let custom = SxFail {
            kind: FAIL_CUSTOM,
            id: 9,
            ..SxFail::default()
        };
        let steps = [c_go(WEST), c_go(EAST)];
        let attack = [c_step(STEP_ATTACK, THIEF, 0, SxWhere::default())];
        unsafe {
            assert_eq!(sx_mission_set_plan(c, steps.as_ptr(), 2, 1, &custom, 1), 0);
            assert_eq!(
                sx_mission_add_playbook(c, attack.as_ptr(), 1, 0, null(), 0, ONE, 0),
                0
            );
        }
        let plan =
            Plan::looping(&[Step::go_to(WEST), Step::go_to(EAST)]).fails_when(Fail::Custom(9));
        t.r.set_plan(&plan).unwrap();
        t.r.add_playbook(&Plan::once(&[Step::attack(THIEF)]), ONE, false)
            .unwrap();
        t.learn(0, fact(THIEF, SEEN, [13, 6, 0], 0));
        let mut seen = Seen::default();
        let rules = SxMissionRules {
            fail: Some(c_fail),
            columns: Some(c_columns),
            score: Some(c_score),
            user: (&mut seen as *mut Seen).cast(),
        };
        for now in 0..10 {
            t.tick_with(now, &rules, &mut Same);
        }
        let broken = t.events(MISSION_EVENT_BROKEN);
        assert_eq!(broken.len(), 1);
        assert_eq!((broken[0].a, broken[0].fail), (BREAK_FAIL, custom));
        assert!(seen.fails.contains(&(6, 0, 9)), "the plan's condition at 6");
        // Playbook slot 0 and the default plan (option 8) were scored.
        assert!(seen.columns.contains(&(0, 2)) && seen.columns.contains(&(8, 2)));
        assert!(seen.scores.iter().any(|&(o, _)| o == 0));
        // The vetoed slot loses; the default plan (option 8) wins.
        let replanned = t.events(MISSION_EVENT_REPLANNED);
        assert_eq!(replanned.iter().map(|e| e.a).collect::<Vec<_>>(), [8]);
        assert!(t.events(MISSION_EVENT_FELL_BACK).is_empty());
    }

    #[test]
    fn null_rules_is_built_in() {
        // Plain tick, NULL rules and a zeroed table act the same, also
        // with a custom condition (it never fires without a callback).
        let custom = SxFail {
            kind: FAIL_CUSTOM,
            id: 1,
            ..SxFail::default()
        };
        let steps = [c_go(WEST), c_step(STEP_WAIT, 3, 0, SxWhere::default())];
        let zeroed = SxMissionRules {
            fail: None,
            columns: None,
            score: None,
            user: null_mut(),
        };
        let mut traces = Vec::new();
        for way in 0..3 {
            let c = unsafe { sx_mission_new(&c_config(), &guard_motive()) };
            let k = sx_knowledge_new(4, 0);
            unsafe { assert_eq!(sx_mission_set_plan(c, steps.as_ptr(), 2, 1, &custom, 1), 0) };
            let mut pos = [3, 2, 0];
            let mut trace = Vec::new();
            for now in 0..20 {
                let mut out = SxMissionOut::default();
                let n = unsafe {
                    match way {
                        0 => sx_mission_tick(c, now, pos.as_ptr(), k, 0, &mut out),
                        1 => sx_mission_tick_with(c, now, pos.as_ptr(), k, 0, null(), &mut out),
                        _ => sx_mission_tick_with(c, now, pos.as_ptr(), k, 0, &zeroed, &mut out),
                    }
                };
                assert!(n >= 0);
                toward(&mut pos, &out.action);
                trace.push(out);
            }
            assert!(trace.iter().all(|o| {
                o.events[..o.event_count as usize]
                    .iter()
                    .all(|e| e.kind != MISSION_EVENT_BROKEN)
            }));
            traces.push(trace);
            unsafe {
                sx_mission_free(c);
                sx_knowledge_free(k);
            }
        }
        assert_eq!(traces[0], traces[1]);
        assert_eq!(traces[0], traces[2]);
    }

    unsafe extern "C" fn c_fail_log(
        user: *mut c_void,
        ctx: *const SxMissionFailCtx,
        id: u32,
    ) -> i32 {
        let log = unsafe { &mut *user.cast::<Vec<(u8, u32, u8, u32)>>() };
        let ctx = unsafe { &*ctx };
        log.push((ctx.has_candidate, ctx.candidate, ctx.has_step, id));
        i32::from(id == 1)
    }

    #[test]
    fn fail_ctx_names_the_candidate() {
        let c = unsafe { sx_mission_new(&c_config(), &guard_motive()) };
        let k = sx_knowledge_new(4, 0);
        let custom = |id| SxFail {
            kind: FAIL_CUSTOM,
            id,
            ..SxFail::default()
        };
        let steps = [c_step(STEP_WAIT, 9, 0, SxWhere::default())];
        let mut wait = c_step(STEP_WAIT, 2, 0, SxWhere::default());
        wait.fails[0] = custom(2);
        wait.fail_count = 1;
        unsafe {
            assert_eq!(
                sx_mission_set_plan(c, steps.as_ptr(), 1, 1, &custom(1), 1),
                0
            );
            assert_eq!(
                sx_mission_add_playbook(c, &wait, 1, 0, &custom(3), 1, ONE, 0),
                0
            );
        }
        let mut log: Vec<(u8, u32, u8, u32)> = Vec::new();
        let rules = SxMissionRules {
            fail: Some(c_fail_log),
            columns: None,
            score: None,
            user: (&mut log as *mut Vec<(u8, u32, u8, u32)>).cast(),
        };
        let mut out = SxMissionOut::default();
        let pos = [0, 0, 0];
        unsafe {
            assert!(sx_mission_tick_with(c, 0, pos.as_ptr(), k, 0, &rules, &mut out) >= 0);
            sx_mission_free(c);
            sx_knowledge_free(k);
        }
        // The running plan's condition, then slot 0's step and plan.
        assert_eq!(log, [(0, 0, 0, 1), (1, 0, 1, 2), (1, 0, 0, 3)]);
        let kinds: Vec<u32> = out.events[..out.event_count as usize]
            .iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, [MISSION_EVENT_BROKEN, MISSION_EVENT_REPLANNED]);
    }

    #[test]
    fn new_rejects_more_plans_than_u16_numbers() {
        let motive = guard_motive();
        unsafe {
            let at_most = SxMissionConfig {
                max_playbook: 65535,
                max_steps: 1,
                ..c_config()
            };
            let m = sx_mission_new(&at_most, &motive);
            assert!(!m.is_null());
            sx_mission_free(m);
            let over = SxMissionConfig {
                max_playbook: 65536,
                ..c_config()
            };
            assert!(sx_mission_new(&over, &motive).is_null());
        }
    }

    #[test]
    fn every_step_kind_converts() {
        let c = unsafe { sx_mission_new(&c_config(), &guard_motive()) };
        let subject = SxWhere {
            kind: WHERE_SUBJECT,
            subject: 4,
            ..SxWhere::default()
        };
        let mut all = Vec::new();
        for kind in STEP_GO_TO..=STEP_SQUAD {
            let mut s = c_step(kind, 3, 4, subject);
            s.skip_if_impossible = 1;
            s.fails[0] = SxFail {
                kind: FAIL_FLAGS,
                flags_mask: 1 << 40,
                flags_value: 1 << 40,
                ..SxFail::default()
            };
            s.fail_count = 1;
            all.push(s);
        }
        unsafe {
            assert_eq!(
                sx_mission_set_plan(c, all.as_ptr(), all.len() as u32, 0, null(), 0),
                0
            );
            sx_mission_free(c);
        }
        let s = all[STEP_SEARCH as usize].step().unwrap();
        assert_eq!(
            s.kind,
            sagax_decide::mission::StepKind::Search {
                centre: sagax_decide::mission::Where::Subject(4),
                radius: 3,
                ticks: 4,
            }
        );
        assert!(s.skip_if_impossible);
        assert_eq!(
            s.fails[0],
            Some(Fail::Flags {
                mask: 1 << 40,
                value: 1 << 40
            })
        );
    }

    #[test]
    fn decider_is_lent() {
        unsafe {
            let c = sx_mission_new(&c_config(), &guard_motive());
            let d = sx_mission_decider(c);
            assert!(!d.is_null());
            assert_eq!(sx_mission_decider(c), d, "one handle per mission");
            let cons = SxConsideration {
                input: IN_DIST as u32,
                curve: 1,
                min: 0,
                max: 20,
            };
            // Option 8 is the default plan; 10 does not exist.
            assert_eq!(sx_decider_add_consideration(d, 8, &cons), 0);
            assert_eq!(sx_decider_add_consideration(d, 10, &cons), -1);
            sx_decider_free(d);
            assert_eq!(sx_decider_add_consideration(d, 8, &cons), 0, "still lent");
            assert!(sx_mission_decider(null_mut()).is_null());
            sx_mission_free(c);

            // The lent handle reaches the mission's own decider: without a
            // plan the first tick replans; the default plan (option 8)
            // beats a playbook plan of lower preference, unless a
            // consideration through the handle rules the default out.
            let never = SxConsideration {
                input: IN_FEASIBLE as u32,
                curve: 4, // SX_CURVE_STEP: 1 if input >= 2, never
                min: 2,
                max: 2,
            };
            let east = [c_go(EAST)];
            let mut winners = Vec::new();
            for veto in [false, true] {
                let c = sx_mission_new(&c_config(), &guard_motive());
                let k = sx_knowledge_new(4, 0);
                let low = ONE / 4;
                assert_eq!(
                    sx_mission_add_playbook(c, east.as_ptr(), 1, 0, null(), 0, low, 0),
                    0
                );
                if veto {
                    let d = sx_mission_decider(c);
                    assert_eq!(sx_decider_add_consideration(d, 8, &never), 0);
                }
                let mut out = SxMissionOut::default();
                let pos = [3, 2, 0];
                assert!(sx_mission_tick(c, 0, pos.as_ptr(), k, 0, &mut out) > 0);
                let replanned: Vec<u32> = out.events[..out.event_count as usize]
                    .iter()
                    .filter(|e| e.kind == MISSION_EVENT_REPLANNED)
                    .map(|e| e.a)
                    .collect();
                winners.push(replanned);
                sx_mission_free(c);
                sx_knowledge_free(k);
            }
            assert_eq!(winners, [vec![8], vec![0]]);
        }
    }

    #[test]
    fn bad_arguments() {
        unsafe {
            let cfg = c_config();
            let motive = guard_motive();
            assert!(sx_mission_new(null(), &motive).is_null());
            assert!(sx_mission_new(&cfg, null()).is_null());
            for bad in [
                SxMissionConfig {
                    max_steps: 0,
                    ..cfg
                },
                SxMissionConfig {
                    max_steps: 65537,
                    ..cfg
                },
                SxMissionConfig { metric: 9, ..cfg },
                SxMissionConfig { grid: 9, ..cfg },
                SxMissionConfig { grid: 1, ..cfg },
                SxMissionConfig { pick: 3, ..cfg },
            ] {
                assert!(sx_mission_new(&bad, &motive).is_null(), "{bad:?}");
            }
            let mut wide = cfg;
            wide.search.count = 9;
            assert!(sx_mission_new(&wide, &motive).is_null());
            let mut sine = cfg;
            sine.search.sine = 2;
            assert!(sx_mission_new(&sine, &motive).is_null());
            let hex = SxMissionConfig {
                grid: GRID_HEX_ODD_R,
                metric: METRIC_HEX,
                ..cfg
            };
            let h = sx_mission_new(&hex, &motive);
            assert!(!h.is_null());
            sx_mission_free(h);
            let odd = SxMotive { kind: 4, ..motive };
            assert!(sx_mission_new(&cfg, &odd).is_null());
            assert_eq!(sx_mission_config_default(null_mut()), ERROR_ARGUMENT);
            let mut l = SxLeeway::default();
            assert_eq!(sx_leeway_preset(3, &mut l), ERROR_ARGUMENT);
            assert_eq!(sx_leeway_preset(0, null_mut()), ERROR_ARGUMENT);

            let m = sx_mission_new(&cfg, &motive);
            let k = sx_knowledge_new(4, 0);
            let pos = [0, 0, 0];
            let go = [c_go(WEST)];
            let five = [stolen_fail(); 5];
            assert_eq!(
                sx_mission_set_plan(m, null(), 0, 0, null(), 0),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_mission_set_plan(m, null(), 1, 0, null(), 0),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_mission_set_plan(m, go.as_ptr(), 1, 0, null(), 1),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_mission_set_plan(m, go.as_ptr(), 1, 0, five.as_ptr(), 5),
                ERROR_ARGUMENT
            );
            let mut many = go[0];
            many.fail_count = 5;
            assert_eq!(
                sx_mission_set_plan(m, &many, 1, 0, null(), 0),
                ERROR_ARGUMENT
            );
            let mut odd_step = go[0];
            odd_step.kind = 13;
            assert_eq!(
                sx_mission_set_plan(m, &odd_step, 1, 0, null(), 0),
                ERROR_ARGUMENT
            );
            let mut odd_where = go[0];
            odd_where.at.kind = 5;
            assert_eq!(
                sx_mission_set_plan(m, &odd_where, 1, 0, null(), 0),
                ERROR_ARGUMENT
            );
            let mut odd_fail = go[0];
            odd_fail.fails[0].kind = 5;
            odd_fail.fail_count = 1;
            assert_eq!(
                sx_mission_set_plan(m, &odd_fail, 1, 0, null(), 0),
                ERROR_ARGUMENT
            );
            // A wait ignores its place, so a bad one does not matter there.
            let mut wait = c_step(STEP_WAIT, 1, 0, SxWhere::default());
            wait.at.kind = 77;
            assert_eq!(sx_mission_set_plan(m, &wait, 1, 0, null(), 0), 0);
            let long = vec![c_go(WEST); 17];
            assert_eq!(
                sx_mission_set_plan(m, long.as_ptr(), 17, 0, null(), 0),
                ERROR_ARGUMENT
            );
            for slot in 0..8 {
                assert_eq!(
                    sx_mission_add_playbook(m, go.as_ptr(), 1, 0, null(), 0, ONE, 0),
                    slot
                );
            }
            assert_eq!(
                sx_mission_add_playbook(m, go.as_ptr(), 1, 0, null(), 0, ONE, 0),
                ERROR_ARGUMENT,
                "full"
            );
            assert_eq!(sx_mission_clear_playbook(m), 0);
            assert_eq!(
                sx_mission_add_playbook(m, go.as_ptr(), 1, 0, null(), 0, ONE, 1),
                0
            );
            let mut out = SxMissionOut::default();
            assert_eq!(
                sx_mission_tick(m, 0, null(), k, 0, &mut out),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_mission_tick(m, 0, pos.as_ptr(), null(), 0, &mut out),
                ERROR_ARGUMENT
            );
            assert!(sx_mission_tick(m, 0, pos.as_ptr(), k, 0, null_mut()) >= 0);
            let odd_leeway = SxLeeway {
                on_lost: 3,
                ..SxLeeway::default()
            };
            assert_eq!(sx_mission_set_leeway(m, &odd_leeway), ERROR_ARGUMENT);
            assert_eq!(sx_mission_set_leeway(m, null()), ERROR_ARGUMENT);
            assert_eq!(sx_mission_set_motive(m, &odd), ERROR_ARGUMENT);
            assert_eq!(sx_mission_set_motive(m, null()), ERROR_ARGUMENT);
            assert_eq!(sx_mission_set_motive(m, &motive), 0);
            let flee_bad = SxDetour {
                kind: DETOUR_FLEE,
                at: SxWhere {
                    kind: 9,
                    ..SxWhere::default()
                },
                ..SxDetour::default()
            };
            assert_eq!(sx_mission_detour(m, 1, &flee_bad, k), ERROR_ARGUMENT);
            let odd_detour = SxDetour {
                kind: 3,
                ..SxDetour::default()
            };
            assert_eq!(sx_mission_detour(m, 1, &odd_detour, k), ERROR_ARGUMENT);
            assert_eq!(sx_mission_detour(m, 1, null(), k), ERROR_ARGUMENT);
            let engage = SxDetour::default();
            assert_eq!(sx_mission_detour(m, 1, &engage, null()), ERROR_ARGUMENT);
            assert_eq!(sx_mission_detour(m, 1, &engage, k), 0, "unknown subject");
            let flee = SxDetour {
                kind: DETOUR_FLEE,
                at: at([1, 0, 0]),
                ..SxDetour::default()
            };
            assert_eq!(sx_mission_detour(m, 1, &flee, k), 1);
            assert_eq!(sx_mission_end_detour(m), 0);
            assert_eq!(sx_mission_finish_step(m), 0);
            assert_eq!(sx_mission_step_index(m, null_mut()), ERROR_ARGUMENT);
            sx_mission_free(m);
            sx_knowledge_free(k);

            let n = null_mut::<SxMission>();
            let mut index = 0;
            assert_eq!(sx_mission_set_leeway(n, &l), ERROR_ARGUMENT);
            assert_eq!(sx_mission_set_motive(n, &motive), ERROR_ARGUMENT);
            assert_eq!(
                sx_mission_set_plan(n, go.as_ptr(), 1, 0, null(), 0),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_mission_add_playbook(n, go.as_ptr(), 1, 0, null(), 0, 0, 0),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_mission_clear_playbook(n), ERROR_ARGUMENT);
            assert_eq!(
                sx_mission_tick(n, 0, pos.as_ptr(), null(), 0, &mut out),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_mission_detour(n, 0, &engage, null()), ERROR_ARGUMENT);
            assert_eq!(sx_mission_end_detour(n), ERROR_ARGUMENT);
            assert_eq!(sx_mission_finish_step(n), ERROR_ARGUMENT);
            assert_eq!(sx_mission_step_index(n, &mut index), ERROR_ARGUMENT);
            assert_eq!(sx_mission_in_detour(n), ERROR_ARGUMENT);
            assert_eq!(sx_mission_returning(n), ERROR_ARGUMENT);
            sx_mission_free(n);
        }
    }
}

mod perception {
    //! C API 13: perception through C equals perception in Rust.

    use super::squad::define;
    use crate::grid::{SxGrid, sx_grid_free, sx_grid_map, sx_grid_new, sx_grid_set};
    use crate::memory::{
        SxAlertConfig, SxAlertness, SxFact, sx_knowledge_free, sx_knowledge_get, sx_knowledge_new,
    };
    use crate::perception::*;
    use crate::{ERROR_ARGUMENT, Point, SxMap, SxRules, sx_nav_free, sx_nav_new};
    use sagax_abi::codes::*;
    use sagax_core::{CallbackSink, HexLayout, Pos, mix};
    use sagax_decide::memory::{Knowledge, LOOK_OFFSETS, sweep_heading};
    use sagax_facade::perception as rs;
    use sagax_facade::perception::{
        Attention, Awareness, Budget, CueRule, Eye, Percept, PerceptionTrace, SenseMap,
        SenseProfile, Stats, Stimulus, Target, World,
    };
    use sagax_nav::grid::Cell;
    use sagax_nav::nav::Navigator;
    use std::ffi::c_void;
    use std::mem::{offset_of, size_of};
    use std::ptr::{null, null_mut};

    const CELL: i32 = 256;
    /// Cue bit: a drawn weapon (instant recognition in the focus).
    const WEAPON: u32 = 1;

    fn handle(grid: u32, map: Option<&SxSenseMap>) -> *mut SxPerception {
        let mut cfg = SxPerceptionConfig::default();
        assert_eq!(
            unsafe { sx_perception_config_default(grid, CELL, &mut cfg) },
            0
        );
        let p = unsafe { sx_perception_new(&cfg, map.map_or(null(), |m| m)) };
        assert!(!p.is_null());
        p
    }

    fn profile() -> SenseProfile {
        SenseProfile::guard(CELL).with_cue(CueRule {
            bits: WEAPON,
            gain: 512,
            instant: 2,
            min_clarity: 0,
            range: 0,
        })
    }

    fn c_eye(profile: &SxSenseProfile, point: Pos, facing: i32) -> SxEye {
        SxEye {
            id: 1,
            point,
            facing,
            profile,
            stats: Stats::NORMAL,
            disguise: SxDisguiseRule::default(),
        }
    }

    fn rs_eye(profile: &SenseProfile, point: Pos, facing: i32) -> Eye<'_> {
        Eye {
            id: 1,
            point,
            facing: facing as u8,
            profile,
            stats: Stats::NORMAL,
            disguise: Default::default(),
        }
    }

    fn centre(world: &World, x: i32, y: i32) -> Pos {
        rs::cell_centre(world, Cell::new(x, y, 0))
    }

    fn target(id: u32, point: Pos, cues: u32) -> Target {
        Target {
            id,
            point,
            size: 256,
            conspicuity: 256,
            cues,
            ..Target::default()
        }
    }

    /// A 12 x 12 grid with a few walls, as an `sx_grid`.
    fn walled_grid() -> *mut SxGrid {
        let g = sx_grid_new(12, 12, 1);
        for (x, y) in [(5, 3), (5, 4), (5, 5), (7, 8), (8, 8), (2, 9), (9, 2)] {
            assert_eq!(unsafe { sx_grid_set(g, Point::new(x, y, 0), -1) }, 0);
        }
        g
    }

    fn c_line(p: *mut SxPerception, from: Pos, to: Pos) -> SxLine {
        let mut line = SxLine::default();
        let r = unsafe { sx_perception_line(p, from.as_ptr(), to.as_ptr(), &mut line) };
        assert_eq!(r, 0);
        line
    }

    fn same_line(rust: rs::Line, c: SxLine, layout: Option<HexLayout>) {
        let blocked = rust.blocked_at.map(|b| match layout {
            Some(l) => l.from_axial_pos(b.pos()),
            None => b.pos(),
        });
        assert_eq!(c.clear, u8::from(rust.clear));
        assert_eq!(c.clarity, rust.clarity);
        assert_eq!(c.has_blocked_at, u8::from(blocked.is_some()));
        assert_eq!(c.blocked_at, blocked.unwrap_or_default());
    }

    /// Fine points in on-grid cells (offset cells of `layout` on hex).
    fn points(world: &World, layout: Option<HexLayout>, n: u64) -> Vec<Pos> {
        (0..n)
            .map(|i| {
                let h = mix(i * 7 + 3);
                let (x, y) = ((h % 12) as i32, ((h >> 8) % 12) as i32);
                let cell = match layout {
                    Some(l) => Cell::from(l.to_axial_pos([x, y, 0])),
                    None => Cell::new(x, y, 0),
                };
                let mut p = rs::cell_centre(world, cell);
                p[0] += ((h >> 16) % 61) as i32 - 30;
                p[1] += ((h >> 24) % 61) as i32 - 30;
                p
            })
            .collect()
    }

    #[test]
    fn ffi_line_equals_rust() {
        let g = walled_grid();
        let dense = unsafe { (*g).dense().clone() };
        let mut grid_map = SxSenseMap::default();
        assert_eq!(unsafe { sx_sense_map_from_grid(g, &mut grid_map) }, 0);
        let mut nav_map = SxMap {
            step: None,
            start_step: None,
            move_cost: None,
            steps: None,
            context: null_mut(),
        };
        assert_eq!(unsafe { sx_grid_map(g, &mut nav_map) }, 0);
        let mut moves = SxSenseMap::default();
        assert_eq!(unsafe { sx_sense_map_from_map(&nav_map, &mut moves) }, 0);

        for (grid, layout) in [
            (GRID_SQUARE, None),
            (GRID_HEX_ODD_R, Some(HexLayout::OddR)),
            (GRID_HEX_EVEN_Q, Some(HexLayout::EvenQ)),
        ] {
            let world = match layout {
                Some(_) => World::hex(CELL),
                None => World::square(CELL),
            };
            let sight = handle(grid, Some(&grid_map));
            let walk = handle(grid, Some(&moves));
            let mut rs_sight = match layout {
                Some(l) => rs::GridSight::hex(&dense, l),
                None => rs::GridSight::square(&dense),
            };
            let mut rs_walk = match layout {
                Some(l) => rs::MoveBlocks::hex(dense.clone(), l),
                None => rs::MoveBlocks::square(dense.clone()),
            };
            let pts = points(&world, layout, 40);
            let (mut blocked, mut walk_blocked) = (0, 0);
            for &a in &pts {
                for &b in &pts {
                    let r = rs::sight_line(&world, &mut rs_sight, a, b);
                    blocked += usize::from(!r.clear);
                    same_line(r, c_line(sight, a, b), layout);
                    let r = rs::sight_line(&world, &mut rs_walk, a, b);
                    walk_blocked += usize::from(!r.clear);
                    same_line(r, c_line(walk, a, b), layout);
                }
            }
            assert!(blocked > 50, "the walls block some lines ({blocked})");
            assert!(walk_blocked > 50, "the moves block some ({walk_blocked})");
            unsafe {
                sx_perception_free(sight);
                sx_perception_free(walk);
            }
        }
        unsafe { sx_grid_free(g) };
    }

    /// Smoke at cell (5, 5) through a C callback.
    unsafe extern "C" fn smoke_sight(
        _ctx: *mut c_void,
        x: i32,
        y: i32,
        _level: i32,
        out: *mut SxMedium,
    ) -> i32 {
        if (x, y) == (5, 5) {
            unsafe { (*out).density = 300 };
        }
        0
    }

    /// The same smoke in Rust.
    struct Smoke;

    impl SenseMap for Smoke {
        fn sight(&mut self, cell: Cell) -> rs::Medium {
            rs::Medium {
                density: if (cell.x, cell.y) == (5, 5) { 300 } else { 0 },
                ..rs::Medium::default()
            }
        }
    }

    fn smoke_map() -> SxSenseMap {
        SxSenseMap {
            sight: Some(smoke_sight),
            ..SxSenseMap::default()
        }
    }

    fn c_percepts(out: &[SxPercept], n: i32) -> Vec<SxPercept> {
        out[..n as usize].to_vec()
    }

    fn of_rust(out: &[Percept], layout: Option<HexLayout>) -> Vec<SxPercept> {
        out.iter().map(|x| SxPercept::of(x, layout)).collect()
    }

    #[test]
    fn ffi_observe_equals_rust() {
        let world = World::square(CELL);
        let profile = profile();
        let c_profile = SxSenseProfile::of(&profile);
        assert_eq!(c_profile.profile(), profile);
        let map = smoke_map();
        let p = handle(GRID_SQUARE, Some(&map));
        let (mut a_rs, mut a_c) = (Awareness::default(), Awareness::default());
        let mut out = vec![];
        let mut buf = [SxPercept::default(); 8];
        let mut seen = 0;
        for t in 0..40u32 {
            let facing = sweep_heading(0, t, 4, &LOOK_OFFSETS);
            let eye_at = centre(&world, 2, 5);
            let cues = if t >= 25 { WEAPON } else { 0 };
            let tgt = target(7, [9 * CELL + 40, (t as i32) * 40 + 100, 0], cues);
            out.clear();
            let eye = rs_eye(&profile, eye_at, facing);
            rs::observe(&world, &mut Smoke, t, &eye, &tgt, &mut a_rs, &mut out);
            let mut total = 0;
            let eye = c_eye(&c_profile, eye_at, facing);
            let n = unsafe {
                sx_perception_observe(p, t, &eye, &tgt, &mut a_c, buf.as_mut_ptr(), 8, &mut total)
            };
            assert_eq!(total as usize, out.len());
            assert_eq!(c_percepts(&buf, n), of_rust(&out, None), "tick {t}");
            assert_eq!(a_c, a_rs, "tick {t}");
            seen += out.len();
        }
        assert!(seen > 10, "the scene sees something ({seen})");
        unsafe { sx_perception_free(p) };
    }

    #[test]
    fn ffi_look_and_hear_equal_rust() {
        let world = World::square(CELL);
        let profile = profile().with_hearing(1, 256);
        let c_profile = SxSenseProfile::of(&profile);
        let map = smoke_map();
        let p = handle(GRID_SQUARE, Some(&map));
        let targets: Vec<Target> = (0..6)
            .map(|i| {
                let mut t = target(10 + i, centre(&world, 8, 2 + i as i32), 0);
                t.crowd = 5;
                t
            })
            .collect();
        let budget = Budget {
            checks: 3,
            keep: 2,
            instant_cap: 1,
        };
        let mut rs_aware = [Awareness::default(); 6];
        let mut c_aware = [Awareness::default(); 6];
        let (mut rs_att, mut c_att) = (Attention::default(), Attention::default());
        let eye_at = centre(&world, 2, 5);
        let mut out = vec![];
        let mut buf = [SxPercept::default(); 16];
        for t in 0..12u32 {
            out.clear();
            rs::look(
                &world,
                &mut Smoke,
                t,
                &rs_eye(&profile, eye_at, 0),
                &targets,
                &mut rs_aware,
                &mut rs_att,
                budget,
                &mut out,
            );
            let mut total = 0;
            let eye = c_eye(&c_profile, eye_at, 0);
            let n = unsafe {
                sx_perception_look(
                    p,
                    t,
                    &eye,
                    targets.as_ptr(),
                    c_aware.as_mut_ptr(),
                    6,
                    &mut c_att,
                    &budget,
                    buf.as_mut_ptr(),
                    16,
                    &mut total,
                )
            };
            assert_eq!(c_percepts(&buf, n), of_rust(&out, None), "tick {t}");
            assert_eq!(c_aware, rs_aware);
            assert_eq!(c_att, rs_att);
        }
        let cursor = unsafe { sx_attention_cursor(&c_att) };
        assert_eq!(cursor, i64::from(c_att.cursor()));

        // A shot and a smell, by spread and by a given cost.
        let shot = Stimulus {
            id: 3,
            source: 99,
            kind: 1,
            point: centre(&world, 9, 9),
            loudness: 20 * CELL,
            channel: 0,
            disguise: 0,
            at: 12,
        };
        let cases = [
            (0u8, 0u8, 0),
            (0, 1, 3 * CELL),
            (1, 0, 0),
            (0, 1, 99 * CELL),
        ];
        for (ch, has_cost, cost) in cases {
            let s = Stimulus {
                channel: ch,
                ..shot
            };
            let (mut a_rs, mut a_c) = (Awareness::default(), Awareness::default());
            out.clear();
            let heard = rs::hear(
                &world,
                &mut Smoke,
                12,
                &rs_eye(&profile, eye_at, 0),
                &s,
                (has_cost != 0).then_some(cost),
                Some(&mut a_rs),
                &mut out,
                42,
            );
            let mut total = 0;
            let eye = c_eye(&c_profile, eye_at, 0);
            let n = unsafe {
                sx_perception_hear(
                    p,
                    12,
                    &eye,
                    &s,
                    has_cost,
                    cost,
                    &mut a_c,
                    buf.as_mut_ptr(),
                    16,
                    &mut total,
                    42,
                )
            };
            assert_eq!(total > 0, heard);
            assert_eq!(c_percepts(&buf, n), of_rust(&out, None));
            assert_eq!(a_c, a_rs);
        }
        unsafe { sx_perception_free(p) };
    }

    #[test]
    fn ffi_hex_cells_are_offset() {
        let world = World::hex(CELL);
        let layout = HexLayout::OddR;
        let p = handle(GRID_HEX_ODD_R, None);
        // Whole cells in: offset (2, 3) is axial (1, 3).
        let mut eye_at = [0; 3];
        let r = unsafe { sx_perception_cell_centre(p, Point::new(2, 3, 0), eye_at.as_mut_ptr()) };
        assert_eq!(r, 0);
        assert_eq!(eye_at, rs::cell_centre(&world, Cell::new(1, 3, 0)));
        let mut tgt_at = [0; 3];
        unsafe { sx_perception_cell_centre(p, Point::new(6, 3, 0), tgt_at.as_mut_ptr()) };
        tgt_at[0] += 30; // fine points stay axial, also off the centre
        let mut back = Point::default();
        let r = unsafe { sx_perception_cell_of(p, tgt_at.as_ptr(), &mut back) };
        assert_eq!(r, 0);
        assert_eq!(back, Point::new(6, 3, 0));

        let profile = SxSenseProfile::of(&profile());
        let eye = c_eye(&profile, eye_at, 0);
        let tgt = target(7, tgt_at, WEAPON);
        let mut aware = Awareness::default();
        let mut buf = [SxPercept::default(); 4];
        let mut total = 0;
        let n = unsafe {
            sx_perception_observe(
                p,
                1,
                &eye,
                &tgt,
                &mut aware,
                buf.as_mut_ptr(),
                4,
                &mut total,
            )
        };
        assert_eq!((n, total), (4, 4));
        let kinds: Vec<u32> = buf.iter().map(|x| x.event).collect();
        let expected = [
            PERCEPT_NOTICED,
            PERCEPT_RECOGNIZED,
            PERCEPT_CUES,
            PERCEPT_SEEN,
        ];
        assert_eq!(kinds, expected);
        for pc in &buf {
            assert_eq!(pc.placed, 1);
            assert_eq!(pc.cell, [6, 3, 0], "whole cells are offset");
            assert_eq!(pc.point, tgt_at, "fine points stay axial");
        }
        let kinds = SxFactKinds {
            noticed_kind: 1,
            seen_kind: 2,
            heard_kind: 3,
            ..SxFactKinds::default()
        };
        let mut fact = SxFact::default();
        assert_eq!(unsafe { sx_percept_fact(&buf[3], &kinds, &mut fact) }, 1);
        assert_eq!((fact.kind, fact.pos), (2, [6, 3, 0]));

        let k = sx_knowledge_new(8, 0);
        let mut alert = SxAlertness::default();
        let cfg = SxAlertConfig {
            max: 1000,
            ..SxAlertConfig::default()
        };
        let amounts = SxAlertAmounts {
            noticed: 5,
            seen: 1,
            ..SxAlertAmounts::default()
        };
        assert_eq!(unsafe { sx_percept_alert(&buf[0], &amounts) }, 5);
        let r =
            unsafe { sx_learn_percepts(k, &mut alert, &cfg, 1, buf.as_ptr(), 4, &kinds, &amounts) };
        assert_eq!(r, 0);
        let mut got = SxFact::default();
        assert_eq!(unsafe { sx_knowledge_get(k, 7, 2, &mut got) }, 1);
        assert_eq!(got.pos, [6, 3, 0]);
        let gone = unsafe { sx_knowledge_get(k, 7, 1, &mut fact) };
        assert_eq!(gone, 0, "recognition replaced the noticed fact");
        // The same as Rust with the offset cells.
        let mut known = Knowledge::new(8, sagax_decide::memory::Evict::Oldest).unwrap();
        let mut rs_alert = SxAlertness::default();
        let rs_percepts: Vec<Percept> = buf.iter().map(|x| x.percept().unwrap()).collect();
        rs::learn_percepts(
            &mut known,
            &mut rs_alert,
            &cfg,
            1,
            &rs_percepts,
            &kinds.kinds(),
            &amounts.amounts(),
        );
        assert_eq!(known.get(7, 2).copied(), Some(got));
        assert_eq!(rs_alert, alert);

        // Lost: not placed, so the cell stays [0, 0, 0].
        let far = target(7, [tgt_at[0] + 100 * CELL, tgt_at[1], 0], 0);
        let n = unsafe {
            sx_perception_observe(
                p,
                2,
                &eye,
                &far,
                &mut aware,
                buf.as_mut_ptr(),
                4,
                &mut total,
            )
        };
        assert_eq!(n, 1);
        let lost = (buf[0].event, buf[0].placed, buf[0].cell);
        assert_eq!(lost, (PERCEPT_LOST, 0, [0; 3]));

        // The flood: offset cells out, the same as Rust in axial.
        let rules = SxRules {
            grid: GRID_HEX_ODD_R,
            flags: 0,
            cost_straight: CELL,
            cost_diagonal: CELL,
            node_capacity: 4096,
            smooth_max_extra: 0,
        };
        let nav = unsafe { sx_nav_new(&rules) };
        let s = Stimulus {
            id: 1,
            source: 50,
            point: eye_at,
            loudness: 3 * CELL,
            ..Stimulus::default()
        };
        let mut cells = [Point::default(); 64];
        let mut costs = [0; 64];
        let (c, cs) = (cells.as_mut_ptr(), costs.as_mut_ptr());
        let n = unsafe { sx_perception_flood(p, nav, &s, c, cs, 64, &mut total) };
        assert_eq!(n as u32, total);
        let topology = sagax_abi::convert::topology(GRID_HEX_ODD_R, 0, CELL, CELL).unwrap();
        let mut rs_nav = Navigator::new(topology, 4096);
        let mut rs_cells = vec![];
        let unlimited = sagax_nav::Budget::UNLIMITED;
        let mut air = rs::OpenAir;
        rs::flood_sound(&mut rs_nav, &world, &mut air, &s, unlimited, &mut rs_cells);
        let expected: Vec<(Point, i32)> = rs_cells
            .iter()
            .map(|&(c, cost)| (Point::of(Cell::from(layout.from_axial_pos(c.pos()))), cost))
            .collect();
        let got: Vec<(Point, i32)> = cells[..n as usize]
            .iter()
            .copied()
            .zip(costs[..n as usize].iter().copied())
            .collect();
        assert_eq!(got, expected);
        assert!(got.len() > 7, "three hexes of sound reach far");
        assert!(got.contains(&(Point::new(2, 3, 0), 0)), "the source cell");
        // A square handle refuses a hex navigator.
        let square = handle(GRID_SQUARE, None);
        let r = unsafe { sx_perception_flood(square, nav, &s, c, null_mut(), 64, null_mut()) };
        assert_eq!(r, ERROR_ARGUMENT);
        unsafe {
            sx_nav_free(nav);
            sx_knowledge_free(k);
            sx_perception_free(square);
            sx_perception_free(p);
        }
    }

    #[test]
    fn ffi_null_and_small_cap() {
        let world = World::square(CELL);
        let p = handle(GRID_SQUARE, None);
        let profile = SxSenseProfile::of(&profile());
        let eye = c_eye(&profile, centre(&world, 2, 2), 0);
        let tgt = target(7, centre(&world, 5, 2), WEAPON);
        let mut aware = Awareness::default();
        let mut total = 99;
        // cap 0: nothing written, but the check ran and counted 4.
        let a = &mut aware as *mut Awareness;
        let n = unsafe { sx_perception_observe(p, 1, &eye, &tgt, a, null_mut(), 0, &mut total) };
        assert_eq!((n, total), (0, 4));
        unsafe {
            assert_eq!(sx_awareness_stage(a), 2);
            assert_eq!(sx_awareness_in_view(a), 1);
            assert_eq!(sx_awareness_last_seen(a), 1);
            assert_eq!(sx_awareness_counter(a), i32::from((*a).counter()));
            assert_eq!(sx_awareness_seen_through(a), 0);
            assert_eq!(sx_awareness_reset(a), 0);
            assert_eq!(*a, Awareness::default());
        }

        let mut buf = [SxPercept::default(); 4];
        let e = ERROR_ARGUMENT;
        let out = buf.as_mut_ptr();
        let none = null_mut::<u32>();
        unsafe {
            let n = null_mut::<SxPerception>();
            assert_eq!(sx_perception_observe(n, 1, &eye, &tgt, a, out, 4, none), e);
            assert_eq!(
                sx_perception_observe(p, 1, null(), &tgt, a, out, 4, none),
                e
            );
            assert_eq!(
                sx_perception_observe(p, 1, &eye, null(), a, out, 4, none),
                e
            );
            let no_aware = null_mut();
            assert_eq!(
                sx_perception_observe(p, 1, &eye, &tgt, no_aware, out, 4, none),
                e
            );
            let no_out = null_mut();
            assert_eq!(
                sx_perception_observe(p, 1, &eye, &tgt, a, no_out, 4, none),
                e
            );
            let no_profile = SxEye {
                profile: null(),
                ..eye
            };
            assert_eq!(
                sx_perception_observe(p, 1, &no_profile, &tgt, a, out, 4, none),
                e
            );

            let mut att = Attention::default();
            let att = &mut att as *mut Attention;
            let budget = Budget::default();
            let look = |targets: *const Target, aware: *mut Awareness, count, total| {
                sx_perception_look(
                    p, 1, &eye, targets, aware, count, att, &budget, out, 4, total,
                )
            };
            assert_eq!(look(null(), a, 1, none), e);
            assert_eq!(look(&tgt, null_mut(), 1, none), e);
            let mut total = 9;
            assert_eq!(look(null(), null_mut(), 0, &mut total), 0);
            assert_eq!(total, 0);
            let no_att = null_mut();
            let r = sx_perception_look(p, 1, &eye, &tgt, a, 1, no_att, &budget, out, 4, none);
            assert_eq!(r, e);

            let s = Stimulus::default();
            let hear = |ear: *const SxEye, s: *const Stimulus, total| {
                sx_perception_hear(p, 1, ear, s, 0, 0, null_mut(), out, 4, total, 0)
            };
            assert_eq!(hear(&eye, null(), none), e);
            assert_eq!(hear(null(), &s, none), e);
            // Stateless hearing: only the HEARD percept.
            let shot = Stimulus {
                point: centre(&world, 3, 3),
                loudness: 10 * CELL,
                ..s
            };
            let mut total = 0;
            assert_eq!(hear(&eye, &shot, &mut total), 1);
            let heard = (buf[0].event, buf[0].sense, total);
            assert_eq!(heard, (PERCEPT_HEARD, SENSE_HEARING, 1));

            let mut line = SxLine::default();
            let at = [0; 3];
            assert_eq!(sx_perception_line(p, null(), at.as_ptr(), &mut line), e);
            assert_eq!(
                sx_perception_line(p, at.as_ptr(), at.as_ptr(), null_mut()),
                e
            );
            assert_eq!(sx_perception_in_sight(p, &eye, null()), e);
            assert_eq!(sx_perception_in_sight(p, &eye, tgt.point.as_ptr()), 1);
            assert_eq!(sx_perception_in_sight(p, null(), tgt.point.as_ptr()), e);
            let mut cell = Point::default();
            assert_eq!(sx_perception_cell_centre(p, cell, null_mut()), e);
            assert_eq!(sx_perception_cell_of(p, null(), &mut cell), e);
            assert_eq!(sx_perception_set_map(null_mut(), null()), e);
            assert_eq!(sx_perception_set_map(p, null()), 0);
            assert_eq!(sx_perception_set_trace(null_mut(), None, null_mut()), e);
            assert_eq!(sx_awareness_stage(null()), e);
            assert_eq!(sx_awareness_last_seen(null()), i64::from(e));
            assert_eq!(sx_awareness_reset(null_mut()), e);
            assert_eq!(sx_attention_cursor(null()), i64::from(e));
            assert_eq!(sx_percept_alert(null(), null()), 0);
            let mut fact = SxFact::default();
            assert_eq!(sx_percept_fact(&buf[0], null(), &mut fact), e);
            let bad = SxPercept {
                event: 99,
                ..buf[0]
            };
            let kinds = SxFactKinds::default();
            assert_eq!(sx_percept_fact(&bad, &kinds, &mut fact), e);
            // A disguised HEARD: no fact without a disguised kind, else
            // that kind.
            let masked = SxPercept {
                disguised: 1,
                ..buf[0]
            };
            assert_eq!(sx_percept_fact(&masked, &kinds, &mut fact), 0);
            let with = SxFactKinds {
                heard_kind: 4,
                disguised_kind: 7,
                has_disguised_kind: 1,
                ..kinds
            };
            assert_eq!(sx_percept_fact(&masked, &with, &mut fact), 1);
            assert_eq!(fact.kind, 7);
            assert_eq!(sx_percept_fact(&buf[0], &with, &mut fact), 1);
            assert_eq!(fact.kind, 4);
            let mut map = SxSenseMap::default();
            assert_eq!(sx_sense_map_from_grid(null(), &mut map), e);
            assert_eq!(sx_sense_map_from_map(null(), &mut map), e);
            let mut prof = SxSenseProfile::default();
            assert_eq!(sx_sense_profile_preset(0, CELL, &mut prof), e);
            let watcher = SENSE_PRESET_WATCHER;
            assert_eq!(sx_sense_profile_preset(watcher, CELL, null_mut()), e);
            assert_eq!(sx_sense_profile_preset(watcher, CELL, &mut prof), 0);
            assert_eq!(prof.profile(), SenseProfile::watcher(CELL));

            let mut cfg = SxPerceptionConfig::default();
            assert_eq!(sx_perception_config_default(9, CELL, &mut cfg), e);
            assert_eq!(sx_perception_config_default(GRID_SQUARE, 1, &mut cfg), e);
            assert!(sx_perception_new(null(), null()).is_null());
            assert!(sx_perception_new(&cfg, null()).is_null(), "cell size 0");
            assert_eq!(sx_perception_config_default(GRID_SQUARE, CELL, &mut cfg), 0);
            cfg.channels[2].spread = 7;
            assert!(sx_perception_new(&cfg, null()).is_null(), "unknown spread");
            sx_perception_free(null_mut());

            let crowd = [tgt, tgt, target(8, [0, 0, 0], 0)];
            let mut counts = [9u8; 3];
            let (t, c) = (crowd.as_ptr(), counts.as_mut_ptr());
            let same = [1, 1, 1];
            assert_eq!(sx_count_crowds(t, 3, CELL, same.as_ptr(), c), 0);
            assert_eq!(counts, [1, 1, 0]);
            let apart = [1, 2, 1];
            let c = counts.as_mut_ptr();
            assert_eq!(sx_count_crowds(t, 3, CELL, apart.as_ptr(), c), 0);
            assert_eq!(counts, [0, 0, 0]);
            let c = counts.as_mut_ptr();
            assert_eq!(sx_count_crowds(t, 3, CELL, null(), c), 0);
            assert_eq!(counts, [1, 1, 0]);
            assert_eq!(sx_count_crowds(null(), 3, CELL, null(), c), e);
            assert_eq!(sx_count_crowds(null(), 0, CELL, null(), null_mut()), 0);
            assert_eq!(sx_hex_heading(7), 43);
            sx_perception_free(p);
        }
    }

    unsafe extern "C" fn collect(ctx: *mut c_void, record: *const SxPerceptionTrace) {
        let v = unsafe { &mut *ctx.cast::<Vec<SxPerceptionTrace>>() };
        v.push(unsafe { *record });
    }

    #[test]
    fn ffi_trace_callback() {
        let world = World::hex(CELL);
        let layout = Some(HexLayout::OddR);
        let p = handle(GRID_HEX_ODD_R, None);
        let mut records: Vec<SxPerceptionTrace> = vec![];
        let ctx = (&mut records as *mut Vec<SxPerceptionTrace>).cast();
        assert_eq!(unsafe { sx_perception_set_trace(p, Some(collect), ctx) }, 0);
        let profile = profile();
        let c_profile = SxSenseProfile::of(&profile);
        let eye_at = centre(&world, 0, 0);
        let targets = [
            target(10, centre(&world, 3, 0), 0),
            target(11, centre(&world, 4, 1), 0),
        ];
        let budget = Budget {
            checks: 1,
            keep: 0,
            instant_cap: 0,
        };
        let mut aware = [Awareness::default(); 2];
        let mut att = Attention::default();
        let eye = c_eye(&c_profile, eye_at, 0);
        let mut buf = [SxPercept::default(); 8];
        let out = buf.as_mut_ptr();
        let (t, a) = (targets.as_ptr(), aware.as_mut_ptr());
        unsafe { sx_perception_look(p, 1, &eye, t, a, 2, &mut att, &budget, out, 8, null_mut()) };
        let shot = Stimulus {
            id: 4,
            source: 99,
            point: centre(&world, 2, 3),
            loudness: 10 * CELL,
            ..Stimulus::default()
        };
        let quiet = Stimulus {
            id: 5,
            loudness: 1,
            ..shot
        };
        let hear = |s: &Stimulus| unsafe {
            sx_perception_hear(p, 1, &eye, s, 0, 0, null_mut(), out, 8, null_mut(), 3)
        };
        hear(&shot);
        hear(&quiet);
        let kinds: Vec<u32> = records.iter().map(|r| r.kind).collect();
        let expected = [
            PERCEPTION_TRACE_PICKED,
            PERCEPTION_TRACE_LINE,
            PERCEPTION_TRACE_GAIN,
            PERCEPTION_TRACE_SKIPPED,
            PERCEPTION_TRACE_HEARD,
            PERCEPTION_TRACE_UNHEARD,
        ];
        assert_eq!(kinds, expected);
        assert_eq!((records[0].target, records[0].why), (10, LOOK_ROUND_ROBIN));
        assert_eq!(records[3].target, 11);
        let heard = (records[4].target, records[4].has_cost, records[4].has_cell);
        assert_eq!(heard, (4, 1, 1));
        assert_eq!(records[5].has_cell, 0);

        // The same records as the Rust trace, cells offset.
        let mut rs_records = vec![];
        let mut sink =
            CallbackSink(|e: PerceptionTrace| rs_records.push(SxPerceptionTrace::of(e, layout)));
        let mut rs_aware = [Awareness::default(); 2];
        let mut rs_att = Attention::default();
        let mut found = vec![];
        let ear = rs_eye(&profile, eye_at, 0);
        let mut air = rs::OpenAir;
        rs::look_traced(
            &world,
            &mut air,
            1,
            &ear,
            &targets,
            &mut rs_aware,
            &mut rs_att,
            budget,
            &mut found,
            &mut sink,
        );
        for s in [shot, quiet] {
            rs::hear_traced(
                &world, &mut air, 1, &ear, &s, None, None, &mut found, 3, &mut sink,
            );
        }
        assert_eq!(records, rs_records);
        let source = rs::cell_of(&world, shot.point).pos();
        assert_eq!(
            rs_records[4].cell,
            HexLayout::OddR.from_axial_pos(source),
            "an unfuzzed heard cell is the source's, offset"
        );

        // Off: nothing more arrives.
        assert_eq!(unsafe { sx_perception_set_trace(p, None, null_mut()) }, 0);
        hear(&shot);
        assert_eq!(records.len(), 6);
        unsafe { sx_perception_free(p) };
    }

    /// The layout of `Awareness` (private fields) that `sx_awareness`
    /// mirrors.
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct AwarenessMirror {
        counter: u16,
        doubt: u16,
        stage: u8,
        pending: u8,
        flags: u8,
        _pad: u8,
        last: u32,
        pending_since: u32,
        last_seen: u32,
        cues: u32,
    }

    #[test]
    fn awareness_layout_matches_header() {
        assert_eq!(size_of::<Awareness>(), size_of::<AwarenessMirror>());
        let world = World::square(CELL);
        let profile = profile();
        let tgt = target(7, centre(&world, 5, 2), WEAPON);
        let mut a = Awareness::default();
        let eye = rs_eye(&profile, centre(&world, 2, 2), 0);
        rs::observe(&world, &mut rs::OpenAir, 9, &eye, &tgt, &mut a, &mut vec![]);
        // SAFETY: both are plain `repr(C)` integers of the same size.
        let m: AwarenessMirror = unsafe { std::mem::transmute(a) };
        assert_eq!(m.counter, a.counter());
        assert_eq!(m.stage, a.stage());
        assert_eq!(m.flags & 1, u8::from(a.in_view()));
        assert_eq!((m.last, m.last_seen, m.cues), (9, 9, WEAPON));
        assert_eq!((m.pending, m.doubt, m.pending_since, m._pad), (2, 0, 9, 0));
        let offsets = [
            offset_of!(AwarenessMirror, doubt),
            offset_of!(AwarenessMirror, stage),
            offset_of!(AwarenessMirror, pending),
            offset_of!(AwarenessMirror, flags),
            offset_of!(AwarenessMirror, last),
            offset_of!(AwarenessMirror, pending_since),
            offset_of!(AwarenessMirror, last_seen),
            offset_of!(AwarenessMirror, cues),
        ];
        assert_eq!(offsets, [2, 4, 5, 6, 8, 12, 16, 20]);
    }

    #[test]
    fn perception_numbers_match_header() {
        assert_eq!(define("SX_API_VERSION"), 16);
        let numbers = [
            ("SX_MAX_CHANNELS", rs::MAX_CHANNELS as i64),
            ("SX_MAX_CUES", rs::MAX_CUES as i64),
            (
                "SX_MAX_PERCEPTS_PER_CHECK",
                rs::MAX_PERCEPTS_PER_CHECK as i64,
            ),
            ("SX_SENSE_FLAG_SIGHT", i64::from(rs::SIGHT)),
            ("SX_SENSE_FLAG_NIGHT", i64::from(rs::NIGHT)),
            ("SX_SENSE_FLAG_THERMAL", i64::from(rs::THERMAL)),
            ("SX_SENSE_FLAG_SCANNER", i64::from(rs::SCANNER)),
            ("SX_SPREAD_CIRCLE", i64::from(SPREAD_CIRCLE)),
            ("SX_SPREAD_STRAIGHT", i64::from(SPREAD_STRAIGHT)),
            ("SX_SENSE_SIGHT", i64::from(SENSE_SIGHT)),
            ("SX_SENSE_THERMAL", i64::from(SENSE_THERMAL)),
            ("SX_SENSE_SCANNER", i64::from(SENSE_SCANNER)),
            ("SX_SENSE_HEARING", i64::from(SENSE_HEARING)),
            ("SX_PERCEPT_NOTICED", i64::from(PERCEPT_NOTICED)),
            ("SX_PERCEPT_RECOGNIZED", i64::from(PERCEPT_RECOGNIZED)),
            ("SX_PERCEPT_SEEN", i64::from(PERCEPT_SEEN)),
            ("SX_PERCEPT_CUES", i64::from(PERCEPT_CUES)),
            ("SX_PERCEPT_LOST", i64::from(PERCEPT_LOST)),
            ("SX_PERCEPT_FORGOTTEN", i64::from(PERCEPT_FORGOTTEN)),
            ("SX_PERCEPT_SEEN_THROUGH", i64::from(PERCEPT_SEEN_THROUGH)),
            ("SX_PERCEPT_HEARD", i64::from(PERCEPT_HEARD)),
            ("SX_LOOK_INSTANT", i64::from(LOOK_INSTANT)),
            ("SX_LOOK_KEPT", i64::from(LOOK_KEPT)),
            ("SX_LOOK_ROUND_ROBIN", i64::from(LOOK_ROUND_ROBIN)),
            ("SX_SENSE_PRESET_HUMAN", i64::from(SENSE_PRESET_HUMAN)),
            ("SX_SENSE_PRESET_GUARD", i64::from(SENSE_PRESET_GUARD)),
            ("SX_SENSE_PRESET_WATCHER", i64::from(SENSE_PRESET_WATCHER)),
            (
                "SX_PERCEPTION_TRACE_PICKED",
                i64::from(PERCEPTION_TRACE_PICKED),
            ),
            (
                "SX_PERCEPTION_TRACE_SKIPPED",
                i64::from(PERCEPTION_TRACE_SKIPPED),
            ),
            ("SX_PERCEPTION_TRACE_LINE", i64::from(PERCEPTION_TRACE_LINE)),
            ("SX_PERCEPTION_TRACE_GAIN", i64::from(PERCEPTION_TRACE_GAIN)),
            (
                "SX_PERCEPTION_TRACE_INSTANT",
                i64::from(PERCEPTION_TRACE_INSTANT),
            ),
            (
                "SX_PERCEPTION_TRACE_STAGE",
                i64::from(PERCEPTION_TRACE_STAGE),
            ),
            (
                "SX_PERCEPTION_TRACE_DOUBT",
                i64::from(PERCEPTION_TRACE_DOUBT),
            ),
            (
                "SX_PERCEPTION_TRACE_HEARD",
                i64::from(PERCEPTION_TRACE_HEARD),
            ),
            (
                "SX_PERCEPTION_TRACE_UNHEARD",
                i64::from(PERCEPTION_TRACE_UNHEARD),
            ),
        ];
        for (name, value) in numbers {
            assert_eq!(define(name), value, "{name}");
        }
    }
}

mod contagion {
    //! C API 14: contagion through C equals contagion in Rust, and neither
    //! allocates after `new`.

    use super::squad::define;
    use crate::ERROR_ARGUMENT;
    use crate::contagion::*;
    use crate::perception::SxPercept;
    use sagax_abi::codes::*;
    use sagax_core::{CallbackSink, Cell, HexLayout, Pos};
    use sagax_facade::perception as rs;
    use sagax_facade::perception::{
        Attention, Awareness, Budget, Contagion, ContagionConfig, ContagionTrace, DeedInputs,
        DeedRule, Eye, Grid, Percept, SenseProfile, Stats, Target, World,
    };
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    /// Counts the allocations of the current thread (tests run in
    /// parallel, so a global count would mix them).
    struct Counting;

    thread_local! {
        static ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    fn count() {
        let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
    }

    // SAFETY: forwards to `System`; the counter does not allocate.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count();
            // SAFETY: the caller's contract, passed on.
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the caller's contract, passed on.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            count();
            // SAFETY: the caller's contract, passed on.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    /// Allocations of this thread while `f` runs.
    pub(super) fn allocations(f: impl FnOnce()) -> usize {
        let before = ALLOCATIONS.with(|c| c.get());
        f();
        ALLOCATIONS.with(|c| c.get()) - before
    }

    const CELL: i32 = 256;
    const ATTACK: u32 = 1;
    const FLEE: u32 = 2;

    fn rs_config(grid: Grid) -> ContagionConfig {
        ContagionConfig::new(grid)
            .with_deed(DeedRule {
                bits: ATTACK,
                window: 4,
            })
            .with_deed(DeedRule {
                bits: FLEE,
                window: 9,
            })
    }

    fn c_config(grid: u32) -> SxContagionConfig {
        let r = rs_config(Grid::Square);
        SxContagionConfig {
            grid,
            count: u32::from(r.count),
            deeds: r.deeds,
        }
    }

    /// A scene seen by one observer at the origin facing +x: per tick the
    /// percepts of a real `look` at five allies that come and go, attack
    /// and flee (ids 10 to 14, the odd ones disguised).
    fn scene(world: &World) -> Vec<(u32, Vec<Percept>)> {
        let profile = SenseProfile {
            base_gain: 400,
            ..SenseProfile::guard(CELL)
        };
        let at = |q: i32, r: i32| rs::cell_centre(world, Cell::new(q, r, 0));
        let eye = Eye {
            id: 1,
            point: at(0, 0),
            facing: 0,
            profile: &profile,
            stats: Stats::NORMAL,
            disguise: Default::default(),
        };
        let mut aware = [Awareness::default(); 5];
        let mut attention = Attention::default();
        let mut ticks = Vec::new();
        for now in 1..=40u32 {
            let targets: Vec<Target> = (0..5u32)
                .map(|k| {
                    let phase = (now / (3 + k)) % 4;
                    let cues = [0, ATTACK, FLEE, ATTACK | FLEE][phase as usize];
                    // Mostly in view, now and then behind the observer.
                    let x = if (now + k) % 7 < 5 { 2 + k as i32 } else { -3 };
                    Target {
                        id: 10 + k,
                        point: at(x, k as i32 - 2),
                        size: 256,
                        conspicuity: 256,
                        cues,
                        // The odd ones drop their disguise at tick 16.
                        disguise: if k % 2 == 1 && now < 16 { 50 } else { 0 },
                        ..Target::default()
                    }
                })
                .collect();
            let mut out = Vec::new();
            let budget = Budget {
                checks: 3,
                keep: 2,
                instant_cap: 0,
            };
            rs::look(
                world,
                &mut rs::OpenAir,
                now,
                &eye,
                &targets,
                &mut aware,
                &mut attention,
                budget,
                &mut out,
            );
            ticks.push((now, out));
        }
        ticks
    }

    /// The belief of the game: everyone but 13, and 11 only while disguised.
    fn belief(subject: u32, disguised: bool) -> bool {
        subject != 13 && (subject != 11 || disguised)
    }

    unsafe extern "C" fn c_belief(_: *mut c_void, subject: u32, disguised: i32) -> i32 {
        i32::from(belief(subject, disguised != 0))
    }

    unsafe extern "C" fn collect(ctx: *mut c_void, record: *const SxContagionTrace) {
        // SAFETY: the test passes a `Vec` and a valid record.
        unsafe { (*(ctx as *mut Vec<SxContagionTrace>)).push(*record) };
    }

    /// The scene through Rust and through C give the same inputs, columns,
    /// nearest points and trace, every tick and for every deed.
    fn same_in_c(grid: u32, world: World, layout: Option<HexLayout>) {
        let ticks = scene(&world);
        let mut rs_memory = Contagion::new(rs_config(world.grid), 3, 1);
        let c = unsafe { sx_contagion_new(&c_config(grid), 3, 1) };
        assert!(!c.is_null());
        let mut c_records: Vec<SxContagionTrace> = Vec::new();
        let ctx = (&raw mut c_records).cast::<c_void>();
        assert_eq!(unsafe { sx_contagion_set_trace(c, Some(collect), ctx) }, 0);
        let mut rs_records = Vec::new();
        let observer: Pos = [0, 0, 0];
        let mut counted = [0u32; 2];
        for (now, out) in &ticks {
            let now = *now;
            let mut sink =
                CallbackSink(|e: ContagionTrace| rs_records.push(SxContagionTrace::of(e)));
            rs_memory.learn_traced(out, now, belief, &mut sink);
            let pc: Vec<SxPercept> = out.iter().map(|p| SxPercept::of(p, layout)).collect();
            let r = unsafe {
                sx_contagion_learn(
                    c,
                    now,
                    pc.as_ptr(),
                    pc.len() as u32,
                    Some(c_belief),
                    null_mut(),
                )
            };
            assert_eq!(r, 0);
            for deed in 0..3u32 {
                let want = rs_memory.inputs_traced(deed as usize, observer, now, &mut sink);
                let mut got = DeedInputs::NONE;
                let r = unsafe { sx_contagion_inputs(c, deed, observer.as_ptr(), now, &mut got) };
                assert_eq!((r, got), (0, want), "tick {now} deed {deed}");
                let mut cols = [0; 5];
                rs_memory.write_columns_traced(deed as usize, observer, now, &mut cols, &mut sink);
                let mut c_cols = [7; 5];
                let r = unsafe {
                    sx_contagion_write_columns(c, deed, observer.as_ptr(), now, c_cols.as_mut_ptr())
                };
                assert_eq!((r, c_cols), (0, cols));
                let mut point = [-9; 3];
                let r = unsafe {
                    sx_contagion_nearest_point(c, deed, observer.as_ptr(), now, point.as_mut_ptr())
                };
                match rs_memory.nearest_point(deed as usize, observer, now) {
                    Some(p) => assert_eq!((r, point), (1, p)),
                    None => assert_eq!((r, point), (0, [-9; 3])),
                }
                if deed < 2 {
                    counted[deed as usize] += want.doing;
                }
            }
            assert_eq!(
                unsafe { sx_contagion_len(c) },
                rs_memory.len() as i32,
                "tick {now}"
            );
        }
        assert!(
            counted[0] > 0 && counted[1] > 0,
            "the scene shows both deeds"
        );
        // Rust learns the percepts of a tick at once, C one at a time: the
        // same records.
        assert_eq!(c_records, rs_records);
        let kinds: std::collections::BTreeSet<u32> = c_records.iter().map(|r| r.kind).collect();
        for kind in [
            CONTAGION_TRACE_ONSET,
            CONTAGION_TRACE_REFRESH,
            CONTAGION_TRACE_EXPIRED,
            CONTAGION_TRACE_REPLACED,
            CONTAGION_TRACE_DROPPED,
            CONTAGION_TRACE_INPUTS,
        ] {
            assert!(kinds.contains(&kind), "kind {kind} never seen: {kinds:?}");
        }
        unsafe { sx_contagion_free(c) };
    }

    #[test]
    fn square_through_c_equals_rust() {
        same_in_c(GRID_SQUARE, World::square(CELL), None);
    }

    #[test]
    fn hex_through_c_equals_rust() {
        // Percept cells become offset at the C edge; the fine points stay
        // axial and the memory never needs a cell.
        same_in_c(GRID_HEX_ODD_R, World::hex(CELL), Some(HexLayout::OddR));
    }

    #[test]
    fn no_allocation_after_new() {
        let ticks = scene(&World::square(CELL));
        let mut memory = Contagion::new(rs_config(Grid::Square), 3, 1);
        let c = unsafe { sx_contagion_new(&c_config(GRID_SQUARE), 3, 1) };
        let pcs: Vec<Vec<SxPercept>> = ticks
            .iter()
            .map(|(_, out)| out.iter().map(|p| SxPercept::of(p, None)).collect())
            .collect();
        let o = [0i32; 3];
        let n = allocations(|| {
            for ((now, out), pc) in ticks.iter().zip(&pcs) {
                memory.learn(out, *now, belief);
                let mut row = [0; 10];
                memory.write_columns(0, o, *now, &mut row[..5]);
                memory.write_columns(1, o, *now, &mut row[5..]);
                let _ = memory.nearest_point(1, o, *now);
                let mut got = DeedInputs::NONE;
                let n = pc.len() as u32;
                unsafe {
                    sx_contagion_learn(c, *now, pc.as_ptr(), n, Some(c_belief), null_mut());
                    sx_contagion_inputs(c, 1, o.as_ptr(), *now, &mut got);
                    sx_contagion_write_columns(c, 0, o.as_ptr(), *now, row.as_mut_ptr());
                }
            }
            memory.forget(10);
            memory.clear();
        });
        assert_eq!(n, 0);
        unsafe { sx_contagion_free(c) };
    }

    #[test]
    fn errors() {
        unsafe {
            assert!(sx_contagion_new(null(), 4, 1).is_null());
            let too_many = SxContagionConfig {
                count: 9,
                ..c_config(GRID_SQUARE)
            };
            assert!(sx_contagion_new(&too_many, 4, 1).is_null());
            let odd_grid = SxContagionConfig {
                grid: 99,
                ..c_config(GRID_SQUARE)
            };
            assert!(sx_contagion_new(&odd_grid, 4, 1).is_null());
            let c = sx_contagion_new(&c_config(GRID_HEX_EVEN_Q), 0, 1);
            assert!(!c.is_null());
            let o = [0i32; 3];
            let mut inputs = DeedInputs::NONE;
            let mut point = [0; 3];
            let mut row = [0; 5];
            let ally = Some(c_belief as IsAllyFn);
            assert_eq!(
                sx_contagion_learn(null_mut(), 1, null(), 0, ally, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_contagion_learn(c, 1, null(), 0, None, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_contagion_learn(c, 1, null(), 1, ally, null_mut()),
                ERROR_ARGUMENT
            );
            let bad = SxPercept {
                sense: 99,
                ..SxPercept::default()
            };
            assert_eq!(
                sx_contagion_learn(c, 1, &bad, 1, ally, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_contagion_learn(c, 1, null(), 0, ally, null_mut()), 0);
            assert_eq!(
                sx_contagion_inputs(null(), 0, o.as_ptr(), 1, &mut inputs),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_contagion_inputs(c, 0, null(), 1, &mut inputs),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_contagion_inputs(c, 0, o.as_ptr(), 1, null_mut()),
                ERROR_ARGUMENT
            );
            // A deed out of range reads empty.
            assert_eq!(sx_contagion_inputs(c, 77, o.as_ptr(), 1, &mut inputs), 0);
            assert_eq!(inputs, DeedInputs::NONE);
            assert_eq!(
                sx_contagion_write_columns(c, 0, o.as_ptr(), 1, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_contagion_write_columns(c, 0, o.as_ptr(), 1, row.as_mut_ptr()),
                0
            );
            assert_eq!(row, [0, 0, 0, i32::MAX, i32::MAX]);
            assert_eq!(
                sx_contagion_nearest_point(c, 0, o.as_ptr(), 1, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_contagion_nearest_point(c, 0, o.as_ptr(), 1, point.as_mut_ptr()),
                0
            );
            assert_eq!(sx_contagion_forget(null_mut(), 1), ERROR_ARGUMENT);
            assert_eq!(sx_contagion_clear(null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_contagion_len(null()), ERROR_ARGUMENT);
            assert_eq!(
                sx_contagion_set_trace(null_mut(), None, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_contagion_forget(c, 1), 0);
            assert_eq!(sx_contagion_clear(c), 0);
            assert_eq!(sx_contagion_len(c), 0);
            sx_contagion_free(c);
            sx_contagion_free(null_mut());
        }
    }

    /// Through C as well: a handle learns only its owner's percepts of
    /// `now`. Owner 1 fed a mixed buffer (its own, the same tick's percepts
    /// of an observer 2 that sees everyone attack and flee, and its own of
    /// the previous tick again) equals Rust fed only its own of `now`;
    /// owner 2 fed only observer 1's percepts learns nothing.
    #[test]
    fn c_learns_only_the_owners_percepts_of_now() {
        let ticks = scene(&World::square(CELL));
        let mut rs_memory = Contagion::new(rs_config(Grid::Square), 3, 1);
        let mine = unsafe { sx_contagion_new(&c_config(GRID_SQUARE), 3, 1) };
        let other = unsafe { sx_contagion_new(&c_config(GRID_SQUARE), 3, 2) };
        assert!(!mine.is_null() && !other.is_null());
        let o = [0i32; 3];
        let mut previous: Vec<SxPercept> = Vec::new();
        for (now, out) in &ticks {
            let now = *now;
            rs_memory.learn(out, now, belief);
            let own: Vec<SxPercept> = out.iter().map(|p| SxPercept::of(p, None)).collect();
            let mut mixed = previous.clone();
            for p in out {
                mixed.push(SxPercept::of(p, None));
                let theirs = Percept {
                    observer: 2,
                    cues: ATTACK | FLEE,
                    ..*p
                };
                mixed.push(SxPercept::of(&theirs, None));
            }
            unsafe {
                let n = mixed.len() as u32;
                assert_eq!(
                    sx_contagion_learn(mine, now, mixed.as_ptr(), n, Some(c_belief), null_mut()),
                    0
                );
                let n = own.len() as u32;
                assert_eq!(
                    sx_contagion_learn(other, now, own.as_ptr(), n, Some(c_belief), null_mut()),
                    0
                );
            }
            for deed in 0..2u32 {
                let want = rs_memory.inputs(deed as usize, o, now);
                let mut got = DeedInputs::NONE;
                unsafe { sx_contagion_inputs(mine, deed, o.as_ptr(), now, &mut got) };
                assert_eq!(got, want, "tick {now} deed {deed}");
            }
            assert_eq!(unsafe { sx_contagion_len(mine) }, rs_memory.len() as i32);
            assert_eq!(unsafe { sx_contagion_len(other) }, 0, "tick {now}");
            previous = own;
        }
        unsafe {
            sx_contagion_free(mine);
            sx_contagion_free(other);
        }
    }

    #[test]
    fn contagion_numbers_match_header() {
        assert_eq!(define("SX_API_VERSION"), 16);
        let numbers = [
            ("SX_MAX_DEEDS", rs::MAX_DEEDS as i64),
            ("SX_DEED_COLUMNS", rs::DEED_COLUMNS as i64),
            ("SX_CONTAGION_TRACE_ONSET", i64::from(CONTAGION_TRACE_ONSET)),
            (
                "SX_CONTAGION_TRACE_REFRESH",
                i64::from(CONTAGION_TRACE_REFRESH),
            ),
            (
                "SX_CONTAGION_TRACE_EXPIRED",
                i64::from(CONTAGION_TRACE_EXPIRED),
            ),
            (
                "SX_CONTAGION_TRACE_REPLACED",
                i64::from(CONTAGION_TRACE_REPLACED),
            ),
            (
                "SX_CONTAGION_TRACE_DROPPED",
                i64::from(CONTAGION_TRACE_DROPPED),
            ),
            (
                "SX_CONTAGION_TRACE_INPUTS",
                i64::from(CONTAGION_TRACE_INPUTS),
            ),
        ];
        for (name, value) in numbers {
            assert_eq!(define(name), value, "{name}");
        }
    }
}

mod goal_guess {
    //! C API 15: the goal guess through C equals the goal guess in Rust,
    //! and neither allocates per step.

    use super::contagion::allocations;
    use super::squad::define;
    use super::{map_of, rules, walls};
    use crate::goal_guess::*;
    use crate::{ERROR_ARGUMENT, Point, SxFlow, sx_flow_build, sx_flow_free, sx_flow_new};
    use sagax_abi::codes::*;
    use sagax_core::{CallbackSink, Cell, HexLayout};
    use sagax_decide::memory::{
        GOAL_COLUMNS, GoalGuess, GoalGuessConfig, GoalGuessTrace, MAX_GOALS, MAX_MOVES, StepError,
    };
    use sagax_facade::goal_guess::observe_flow_traced;
    use sagax_nav::Limits;
    use sagax_nav::flow::FlowField;
    use sagax_nav::grid::{DenseGrid, GridRules, Preset};
    use sagax_nav::topology::Topology;
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    const BLOCKED: [(i32, i32); 9] = [
        (9, 3),
        (9, 4),
        (9, 5),
        (9, 6),
        (9, 8),
        (9, 9),
        (9, 10),
        (9, 11),
        (9, 12),
    ];
    const EXITS: [(i32, i32); 4] = [(0, 0), (19, 0), (0, 19), (19, 19)];

    unsafe extern "C" fn collect(ctx: *mut c_void, t: *const SxGoalGuessTrace) {
        let out = unsafe { &mut *ctx.cast::<Vec<SxGoalGuessTrace>>() };
        out.push(unsafe { *t });
    }

    fn c_config() -> SxGoalGuessConfig {
        let mut c = SxGoalGuessConfig {
            beta: 0,
            unit: 0,
            forget: 0,
        };
        assert_eq!(unsafe { sx_goal_guess_config_default(&mut c) }, 0);
        c
    }

    /// One walk through C and Rust: same chances, same records, per step.
    fn same_walk(grid: u32, topology: Topology, start: Cell, exit: usize) {
        let mut w = walls(&BLOCKED);
        let map = map_of(&mut w);
        let mut dense = DenseGrid::new(20, 20);
        for &(x, y) in &BLOCKED {
            dense.set(x, y, DenseGrid::BLOCKED);
        }
        let mut graph = topology.graph(&mut dense, None);
        let mut fields: Vec<FlowField<Cell>> = Vec::new();
        let mut flows: Vec<*mut SxFlow> = Vec::new();
        let r = rules(grid);
        for &(x, y) in &EXITS {
            let mut f = FlowField::default();
            f.build(&mut graph, [Cell::new(x, y, 0)], Limits::default());
            fields.push(f);
            let flow = unsafe { sx_flow_new(&r) };
            let goal = Point::new(x, y, 0);
            assert_eq!(
                unsafe { sx_flow_build(flow, &map, &goal, 1, i32::MAX, null_mut()) },
                0
            );
            flows.push(flow);
        }
        let mut way = Vec::new();
        assert!(fields[exit].path(&mut graph, start, &mut way));

        let c = unsafe { sx_goal_guess_new(&c_config(), 4) };
        let mut c_log: Vec<SxGoalGuessTrace> = Vec::new();
        unsafe {
            sx_goal_guess_set_trace(c, Some(collect), (&mut c_log as *mut Vec<_>).cast());
        }
        let mut rs = GoalGuess::new(GoalGuessConfig::default(), 4);
        let mut rs_log = Vec::new();
        let mut moves = Vec::new();
        let ptrs: Vec<*const SxFlow> = flows.iter().map(|&f| f.cast_const()).collect();
        for pair in way.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            observe_flow_traced(
                &mut rs,
                &mut graph,
                &fields,
                a,
                b,
                &mut moves,
                &mut CallbackSink(|e| rs_log.push(SxGoalGuessTrace::of(e))),
            )
            .unwrap();
            let got = unsafe {
                sx_goal_guess_observe_flow(
                    c,
                    ptrs.as_ptr().cast(),
                    4,
                    &map,
                    Point::new(a.x, a.y, a.level),
                    Point::new(b.x, b.y, b.level),
                )
            };
            assert_eq!(got, 0);
            for k in 0..4u32 {
                assert_eq!(
                    unsafe { sx_goal_guess_chance_fine(c, k) },
                    rs.chance_fine(k as usize) as i32
                );
                assert_eq!(
                    unsafe { sx_goal_guess_chance(c, k) },
                    rs.chance(k as usize) as i32
                );
                assert_eq!(
                    unsafe { sx_goal_guess_rank(c, k) },
                    rs.rank(k as usize) as i32
                );
            }
        }
        assert_eq!(c_log, rs_log);
        assert!(c_log.len() >= 4 * (way.len() - 1));
        let mut chance = 0u32;
        assert_eq!(unsafe { sx_goal_guess_best(c, &mut chance) }, exit as i32);
        assert_eq!(Some((exit, chance)), rs.best());
        unsafe {
            sx_goal_guess_free(c);
            for f in flows {
                sx_flow_free(f);
            }
        }
    }

    #[test]
    fn observe_flow_through_c_equals_rust_on_square_and_hex() {
        let square = Topology::Square(GridRules::preset(Preset::Octile));
        let hex = Topology::hex(HexLayout::OddR, Preset::Octile);
        for exit in 0..4 {
            for start in [Cell::new(8, 7, 0), Cell::new(12, 9, 0)] {
                same_walk(GRID_SQUARE, square, start, exit);
                same_walk(GRID_HEX_ODD_R, hex, start, exit);
            }
        }
    }

    #[test]
    fn observe_and_reads_through_c_equal_rust() {
        let c = unsafe { sx_goal_guess_new(&c_config(), 3) };
        let mut rs = GoalGuess::new(GoalGuessConfig::default(), 3);
        let weights = [5u32, 3, 0];
        assert_eq!(
            unsafe { sx_goal_guess_set_prior(c, weights.as_ptr(), 3) },
            0
        );
        rs.set_prior(&weights);
        let moved = [-10, 10, 0];
        let options = [-10, 10, 14, 10, -10, -14, 0, 0, 0];
        for _ in 0..3 {
            assert_eq!(
                unsafe { sx_goal_guess_observe(c, moved.as_ptr(), options.as_ptr(), 3) },
                0
            );
            rs.observe(&moved, &options, 3).unwrap();
        }
        for k in 0..4u32 {
            assert_eq!(
                unsafe { sx_goal_guess_chance(c, k) },
                rs.chance(k as usize) as i32
            );
        }
        let mut row = [0i32; GOAL_COLUMNS];
        let mut rs_row = [0i32; GOAL_COLUMNS];
        assert_eq!(
            unsafe { sx_goal_guess_write_columns(c, 1, row.as_mut_ptr()) },
            0
        );
        rs.write_columns(1, &mut rs_row);
        assert_eq!(row, rs_row);
        let mut out = [9u32; 5];
        let mut rs_out = [0u32; 5];
        assert_eq!(unsafe { sx_goal_guess_split(c, 3, out.as_mut_ptr(), 5) }, 0);
        rs.split(3, &mut rs_out);
        assert_eq!(out, rs_out);
        assert_eq!(unsafe { sx_goal_guess_goals(c) }, 3);
        assert_eq!(unsafe { sx_goal_guess_reset(c) }, 0);
        rs.reset();
        assert_eq!(unsafe { sx_goal_guess_chance(c, 0) }, rs.chance(0) as i32);
        assert_eq!(unsafe { sx_goal_guess_chance(c, 2) }, 0, "ruled out");
        unsafe { sx_goal_guess_free(c) };
    }

    #[test]
    fn bad_arguments_are_refused() {
        unsafe {
            assert!(sx_goal_guess_new(null(), 2).is_null());
            assert!(sx_goal_guess_new(&c_config(), 0).is_null());
            assert!(sx_goal_guess_new(&c_config(), MAX_GOALS as u32 + 1).is_null());
            assert_eq!(sx_goal_guess_config_default(null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_goal_guess_goals(null()), ERROR_ARGUMENT);
            assert_eq!(sx_goal_guess_chance(null(), 0), ERROR_ARGUMENT);
            assert_eq!(sx_goal_guess_best(null(), null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_goal_guess_reset(null_mut()), ERROR_ARGUMENT);
            let g = sx_goal_guess_new(&c_config(), 2);
            let mut log: Vec<SxGoalGuessTrace> = Vec::new();
            sx_goal_guess_set_trace(g, Some(collect), (&mut log as *mut Vec<_>).cast());
            let before = sx_goal_guess_chance_fine(g, 0);
            assert_eq!(sx_goal_guess_set_prior(g, null(), 2), ERROR_ARGUMENT);
            assert_eq!(
                sx_goal_guess_set_prior(g, null(), 0),
                0,
                "0 weights: uniform"
            );
            let moved = [0i32; 2];
            let options = [0i32; 2 * (MAX_MOVES + 1)];
            assert_eq!(
                sx_goal_guess_observe(g, moved.as_ptr(), options.as_ptr(), 0),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_goal_guess_observe(g, moved.as_ptr(), options.as_ptr(), MAX_MOVES as u32 + 1),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_goal_guess_observe(g, null(), options.as_ptr(), 1),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_goal_guess_chance_fine(g, 0), before);
            assert_eq!(
                sx_goal_guess_write_columns(g, 0, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_goal_guess_split(g, 3, null_mut(), 2), ERROR_ARGUMENT);
            assert_eq!(sx_goal_guess_split(g, 3, null_mut(), 0), 0);
            let refused = log
                .iter()
                .filter(|t| t.kind == GOAL_GUESS_TRACE_REFUSED && t.error == STEP_SHAPE)
                .count();
            assert_eq!(refused, 3);

            // observe_flow: wrong count, null flows, a jump.
            let mut w = walls(&[]);
            let map = map_of(&mut w);
            let r = rules(0);
            let a = sx_flow_new(&r);
            let b = sx_flow_new(&r);
            let ga = Point::new(0, 0, 0);
            let gb = Point::new(19, 19, 0);
            sx_flow_build(a, &map, &ga, 1, i32::MAX, null_mut());
            sx_flow_build(b, &map, &gb, 1, i32::MAX, null_mut());
            let two: [*const SxFlow; 2] = [a, b];
            let one_null: [*const SxFlow; 2] = [a, null()];
            let p = Point::new(5, 5, 0);
            assert_eq!(
                sx_goal_guess_observe_flow(g, two.as_ptr().cast(), 1, &map, p, p),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_goal_guess_observe_flow(g, one_null.as_ptr().cast(), 2, &map, p, p),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_goal_guess_observe_flow(g, null(), 2, &map, p, p),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_goal_guess_observe_flow(g, two.as_ptr().cast(), 2, null(), p, p),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_goal_guess_observe_flow(g, two.as_ptr().cast(), 2, &map, p, p),
                0,
                "standing still"
            );
            let far = Point::new(9, 9, 0);
            assert_eq!(
                sx_goal_guess_observe_flow(g, two.as_ptr().cast(), 2, &map, p, far),
                1,
                "a jump"
            );
            assert_eq!(sx_goal_guess_chance_fine(g, 0), before);
            let last = log.last().copied().unwrap();
            assert_eq!(
                (last.kind, last.error),
                (GOAL_GUESS_TRACE_REFUSED, STEP_JUMP)
            );
            sx_goal_guess_free(g);
            sx_flow_free(a);
            sx_flow_free(b);
        }
    }

    #[test]
    fn no_allocation_per_step() {
        let mut w = walls(&BLOCKED);
        let map = map_of(&mut w);
        let r = rules(0);
        let flows: Vec<*mut SxFlow> = EXITS
            .iter()
            .map(|&(x, y)| {
                let f = unsafe { sx_flow_new(&r) };
                let goal = Point::new(x, y, 0);
                unsafe { sx_flow_build(f, &map, &goal, 1, i32::MAX, null_mut()) };
                f
            })
            .collect();
        let ptrs: Vec<*const SxFlow> = flows.iter().map(|&f| f.cast_const()).collect();
        let c = unsafe { sx_goal_guess_new(&c_config(), 4) };
        let mut log: Vec<SxGoalGuessTrace> = Vec::with_capacity(4096);
        unsafe { sx_goal_guess_set_trace(c, Some(collect), (&mut log as *mut Vec<_>).cast()) };
        let mut out = [0u32; 4];
        let n = allocations(|| {
            for x in 3..9 {
                let got = unsafe {
                    sx_goal_guess_observe_flow(
                        c,
                        ptrs.as_ptr().cast(),
                        4,
                        &map,
                        Point::new(x, 14, 0),
                        Point::new(x + 1, 15, 0),
                    )
                };
                assert_eq!(got, 0);
                unsafe { sx_goal_guess_split(c, 3, out.as_mut_ptr(), 4) };
            }
        });
        assert_eq!(n, 0, "no heap per step");
        assert_eq!(unsafe { sx_goal_guess_best(c, null_mut()) }, 3);

        // The Rust guess is a plain value: no heap at all.
        let mut rs = GoalGuess::new(GoalGuessConfig::default(), 4);
        let n = allocations(|| {
            for _ in 0..10 {
                rs.observe(&[-10, 10, 0, 14], &[-10, 10, 10, -10, 0, 0, 14, -14], 2)
                    .unwrap();
                rs.split(3, &mut out);
            }
        });
        assert_eq!(n, 0);
        unsafe {
            sx_goal_guess_free(c);
            for f in flows {
                sx_flow_free(f);
            }
        }
    }

    #[test]
    fn goal_guess_numbers_match_header() {
        assert_eq!(define("SX_API_VERSION"), 16);
        let numbers = [
            ("SX_MAX_GOALS", MAX_GOALS as i64),
            ("SX_MAX_MOVES", MAX_MOVES as i64),
            ("SX_GOAL_COLUMNS", GOAL_COLUMNS as i64),
            ("SX_GOAL_GUESS_TRACE_STEP", i64::from(GOAL_GUESS_TRACE_STEP)),
            (
                "SX_GOAL_GUESS_TRACE_PRIOR",
                i64::from(GOAL_GUESS_TRACE_PRIOR),
            ),
            (
                "SX_GOAL_GUESS_TRACE_RESET",
                i64::from(GOAL_GUESS_TRACE_RESET),
            ),
            (
                "SX_GOAL_GUESS_TRACE_REFUSED",
                i64::from(GOAL_GUESS_TRACE_REFUSED),
            ),
            ("SX_STEP_SHAPE", i64::from(STEP_SHAPE)),
            ("SX_STEP_JUMP", i64::from(STEP_JUMP)),
        ];
        for (name, value) in numbers {
            assert_eq!(define(name), value, "{name}");
        }
        let _ = (StepError::Jump, GoalGuessTrace::Reset);
    }
}
