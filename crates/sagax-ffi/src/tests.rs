use super::*;
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

fn rules(grid: i32) -> SxRules {
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
fn version_is_5() {
    assert_eq!(sx_api_version(), 5);
}

#[test]
fn bad_rules_give_null() {
    let mut r = rules(0);
    r.cost_straight = 0;
    assert!(unsafe { sx_nav_create(&r) }.is_null());
    let mut r = rules(9);
    assert!(unsafe { sx_nav_create(&r) }.is_null());
    r.grid = 0;
    r.node_capacity = 0;
    assert!(unsafe { sx_flow_create(&r) }.is_null());
    assert!(unsafe { sx_dstar_create(std::ptr::null()) }.is_null());
}

#[test]
fn find_path_writes_every_output_from_the_start() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };
}

#[test]
fn start_is_goal_is_found_with_one_cell() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };
}

#[test]
fn small_budgets_continue_to_the_same_way() {
    let mut w = wall_line();
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };
}

#[test]
fn closed_off_goal_gives_partial_way() {
    let mut w = walls(&(0..20).map(|y| (10, y)).collect::<Vec<_>>());
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };
}

#[test]
fn reachable_then_path_to() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };
}

#[test]
fn batch_callback_gives_the_same_way_with_fewer_calls() {
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };
}

#[test]
fn formation_writes_leader_and_members() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };
}

#[test]
fn flow_serves_many_walkers() {
    let mut w = wall_line();
    let map = map_of(&mut w);
    let flow = unsafe { sx_flow_create(&rules(0)) };
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
    unsafe { sx_flow_destroy(flow) };
}

#[test]
fn dstar_repairs_after_a_change() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let d = unsafe { sx_dstar_create(&rules(0)) };
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
    unsafe { sx_dstar_destroy(d) };
}

#[test]
fn hex_grids_work_everywhere() {
    for grid in 1..=4 {
        let mut w = wall_line();
        let map = map_of(&mut w);
        let nav = unsafe { sx_nav_create(&rules(grid)) };
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
        unsafe { sx_nav_destroy(nav) };

        let d = unsafe { sx_dstar_create(&rules(grid)) };
        unsafe { sx_dstar_begin(d, p(2, 2), p(18, 2)) };
        unsafe { sx_dstar_compute(d, &map, 0, 0, points.as_mut_ptr(), 64, &mut result) };
        assert_eq!(result.status, FOUND, "grid {grid}");
        unsafe { sx_dstar_destroy(d) };
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
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };

    let d = unsafe { sx_dstar_create(&rules(0)) };
    let mut events: Vec<SxEvent> = Vec::new();
    unsafe { sx_dstar_set_trace(d, Some(record), (&mut events as *mut Vec<SxEvent>).cast()) };
    unsafe { sx_dstar_begin(d, p(2, 2), p(18, 2)) };
    unsafe { sx_dstar_compute(d, &map, 0, 0, points.as_mut_ptr(), 64, &mut result) };
    assert_eq!(result.status, FOUND);
    assert!(events.iter().filter(|e| e.kind == 2).count() > 10);
    assert_eq!(events.last().map(|e| (e.kind, e.found)), Some((5, 1)));
    unsafe { sx_dstar_destroy(d) };

    let flow = unsafe { sx_flow_create(&rules(0)) };
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
    unsafe { sx_flow_destroy(flow) };
}

#[test]
fn flee_and_several_goals() {
    let mut w = walls(&[]);
    let map = map_of(&mut w);
    let nav = unsafe { sx_nav_create(&rules(0)) };
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
    unsafe { sx_nav_destroy(nav) };
}
