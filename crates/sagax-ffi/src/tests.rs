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
fn version_is_8() {
    assert_eq!(sx_api_version(), 8);
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
            assert_eq!(unsafe { sx_areas_add(areas, &a, 0) }, 0);
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
        let nav = unsafe { sx_nav_create(&rules(0)) };
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
        unsafe { sx_nav_destroy(nav) };
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
        assert_eq!(unsafe { sx_areas_add(areas, &a, 0) }, 0);
        a.kind = 2;
        a.pos = [60, 50, 0];
        a.blocking = 1;
        assert_eq!(unsafe { sx_areas_add(areas, &a, 0) }, 1);
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
    use crate::{ERROR_ARGUMENT, ERROR_PLAN};
    use sagax_decide::group::{
        IN_AREA, IN_EXTRA, IN_LOSSES, IN_OFF_PLAN, IN_PLAN_AGE, IN_RANGED, IN_TARGET_AGE,
        IN_TARGET_DIST, IN_TARGET_FRIEND_DIST, MAX_EXTRA_COLUMNS, REPORT_ARRIVED, REPORT_DANGER,
        REPORT_ENEMY, REPORT_GOAL_BROKEN, ROLE_AREA, ROLE_RANGED, TAG_CHOKE, TAG_COVER, TAG_HIDDEN,
        TAG_HIGH,
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
    const EUCLID: i32 = 0;
    const CHEBYSHEV: i32 = 2;

    /// The value of `#define name value` in `sagax.h`.
    fn define(name: &str) -> i64 {
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
    fn squad_of(metric: i32, members: &[(u32, i32, i32)]) -> *mut SxSquad {
        let sq = sx_squad_new(8, 8, 4, 8, metric);
        assert!(!sq.is_null());
        unsafe {
            for (i, &(id, x, y)) in members.iter().enumerate() {
                assert_eq!(sx_squad_add_member(sq, &member(id, x, y, 0)), i as i32);
            }
            assert_eq!(sx_squad_set_leader(sq, members[0].0), 1);
            for &(id, ..) in &members[1..] {
                assert_eq!(sx_squad_set_channel(sq, id, CHANNEL_RADIO, 2, 1), 1);
            }
            assert_eq!(sx_squad_set_target_kind(sq, ENEMY), 0);
        }
        sq
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
        assert_eq!(size_of::<SxSkillTable>(), 100);
        assert_eq!(define("SX_API_VERSION"), 8);
        assert_eq!(define("SX_ERROR_PLAN"), i64::from(ERROR_PLAN));
        let core = [
            ("SX_IN_LOSSES", IN_LOSSES as i64),
            ("SX_IN_OFF_PLAN", IN_OFF_PLAN as i64),
            ("SX_IN_TARGET_AGE", IN_TARGET_AGE as i64),
            ("SX_IN_TARGET_DIST", IN_TARGET_DIST as i64),
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
            ("SX_REPORT_DANGER", i64::from(REPORT_DANGER)),
            ("SX_REPORT_ARRIVED", i64::from(REPORT_ARRIVED)),
        ];
        let ffi = [
            ("SX_MEMBER_ACTIVE", 0),
            ("SX_MEMBER_SUSPENDED", 1),
            ("SX_MEMBER_GONE", 2),
            ("SX_CHANNEL_NONE", i64::from(CHANNEL_NONE)),
            ("SX_CHANNEL_SHOUT", i64::from(CHANNEL_SHOUT)),
            ("SX_CHANNEL_RADIO", i64::from(CHANNEL_RADIO)),
            ("SX_PLACE_NONE", i64::from(PLACE_NONE)),
            ("SX_PLACE_OFFSET", i64::from(PLACE_OFFSET)),
            ("SX_PLACE_SIDE", i64::from(PLACE_SIDE)),
            ("SX_PLACE_BETWEEN", i64::from(PLACE_BETWEEN)),
            ("SX_PLACE_TAGGED", i64::from(PLACE_TAGGED)),
            ("SX_PLACE_HERE", i64::from(PLACE_HERE)),
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
            ("SX_TRIGGER_NONE", i64::from(TRIGGER_NONE)),
            ("SX_TRIGGER_ALL_ARRIVED", i64::from(TRIGGER_ALL_ARRIVED)),
            (
                "SX_TRIGGER_TARGET_IN_RADIUS",
                i64::from(TRIGGER_TARGET_IN_RADIUS),
            ),
            ("SX_TRIGGER_MANUAL", i64::from(TRIGGER_MANUAL)),
            ("SX_TACTIC_RUSH", i64::from(TACTIC_RUSH)),
            ("SX_TACTIC_LINE", i64::from(TACTIC_LINE)),
            ("SX_TACTIC_WEDGE", i64::from(TACTIC_WEDGE)),
            ("SX_TACTIC_COLUMN", i64::from(TACTIC_COLUMN)),
            ("SX_TACTIC_FLANK", i64::from(TACTIC_FLANK)),
            ("SX_TACTIC_FLANK_FRONT", i64::from(TACTIC_FLANK_FRONT)),
            ("SX_TACTIC_AMBUSH", i64::from(TACTIC_AMBUSH)),
            ("SX_TACTIC_FOCUS", i64::from(TACTIC_FOCUS)),
            ("SX_TACTIC_ISOLATE", i64::from(TACTIC_ISOLATE)),
            ("SX_TACTIC_FALL_BACK", i64::from(TACTIC_FALL_BACK)),
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
            offset_of!(SxSkillTable, pick),
            offset_of!(SxSkillTable, pick_top),
            offset_of!(SxSkillTable, think_ticks),
            offset_of!(SxSkillTable, timing_spread),
            offset_of!(SxSkillTable, hold_bonus),
            offset_of!(SxSkillTable, give_up_ticks),
            offset_of!(SxSkillTable, spot_rank_error),
        ];
        assert_eq!(table, [0, 16, 32, 48, 64, 80, 96]);
    }

    #[test]
    fn members_through_c() {
        unsafe {
            assert!(sx_squad_new(0, 8, 4, 8, EUCLID).is_null());
            assert!(sx_squad_new(8, 0, 4, 8, EUCLID).is_null());
            assert!(sx_squad_new(8, 8, 0, 8, EUCLID).is_null());
            assert!(sx_squad_new(8, 8, 4, 0, EUCLID).is_null());
            assert!(sx_squad_new(8, 8, 4, 8, 4).is_null(), "no such metric");
            let sq = sx_squad_new(2, 4, 4, 4, CHEBYSHEV);
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
            assert_eq!(sx_squad_resume(sq, 2, 0), 1);
            assert_eq!(sx_squad_resume(sq, 9, 0), 0);
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
            for which in 0..10 {
                assert_eq!(sx_squad_add_shipped(sq, which, 2), which as i32);
            }
            assert_eq!(sx_squad_add_shipped(sq, 10, 2), -1);
            assert_eq!(sx_squad_add_shipped(null_mut(), 0, 2), -1);

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
            assert_eq!(sx_tactic_set_trigger(t, TRIGGER_ALL_ARRIVED, 9), 0);
            assert_eq!(
                sx_tactic_set_trigger(t, TRIGGER_ALL_ARRIVED, -1),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_tactic_set_trigger(t, 4, 0), ERROR_ARGUMENT);
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
                assert_eq!(sx_squad_set_channel(sq, id, CHANNEL_RADIO, 0, 0), 1);
            }
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_RUSH, 0), 0);
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_FLANK, 4), 1);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            let mut id = 0;
            let made = sx_squad_plan(sq, 0, null(), 0, null(), 0, 0, &mut id);
            assert_eq!(made, 0, "nothing known yet");
            assert_eq!(sx_squad_observe(sq, &enemy(E1, 20, 0), 0, null_mut()), 0);
            let mut targets = [SxFact::default(); 2];
            assert_eq!(sx_squad_targets(sq, targets.as_mut_ptr(), 2), 1);
            assert_eq!((targets[0].subject, targets[0].source), (E1, 0));
            assert_eq!(sx_squad_targets(sq, null_mut(), 0), 1);
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
                assert_eq!(sx_squad_set_channel(sq, m, CHANNEL_RADIO, 0, 0), 1);
            }
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_RUSH, 0), 0);
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_FLANK, 4), 1);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            assert_eq!(sx_squad_observe(sq, &enemy(E1, 20, 0), 0, null_mut()), 0);
            assert_eq!(check(sq, 0, &mut id), SQUAD_REPLAN);
            assert_eq!(check(sq, 5, &mut id), SQUAD_KEEP);
        }
        (sq, id)
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
            assert_eq!(sx_squad_resume(sq, 3, 30), 1);
            assert_eq!(sx_squad_orders_due(sq, 30, out.as_mut_ptr(), 8), 1);
            let g = out[0].goal;
            assert_eq!((out[0].member, g.kind, g.plan), (3, GOAL_ATTACK, id));
            sx_squad_free(sq);

            // Rush's wish is (19, 0); a spot 5 away counts only with a
            // larger radius.
            let sq = squad_of(EUCLID, &[(1, 0, 0)]);
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_RUSH, 0), 0);
            assert_eq!(sx_squad_observe(sq, &enemy(E1, 20, 0), 0, null_mut()), 0);
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
    fn channels_and_reports_through_c() {
        unsafe {
            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0), (2, 3, 0), (3, 9, 0)]);
            assert_eq!(sx_squad_set_channel(sq, 2, CHANNEL_SHOUT, 5, 0), 1);
            assert_eq!(sx_squad_set_channel(sq, 3, CHANNEL_SHOUT, 5, 0), 1);
            assert_eq!(sx_squad_set_channel(sq, 9, CHANNEL_SHOUT, 5, 0), 0);
            assert_eq!(sx_squad_set_channel(sq, 2, 3, 5, 0), ERROR_ARGUMENT);
            let too_far = sx_squad_set_channel(sq, 2, CHANNEL_RADIO, 1 << 32, 0);
            assert_eq!(too_far, ERROR_ARGUMENT);
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_RUSH, 0), 0);

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
            let shot = SxMessage {
                kind: REPORT_DANGER,
                subject: E2,
                pos: [3, 0, 1],
                strength: 30,
                data: 4,
                sender: 2,
                ..SxMessage::default()
            };
            assert_eq!(sx_squad_report(sq, &shot, 0), 1);
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
            assert_eq!(sx_squad_targets(sq, &mut fact, 1), 1);
            assert_eq!((fact.subject, fact.pos, fact.source), (E1, [20, 0, 0], 2));
            let areas = sx_squad_areas(sq);
            assert_eq!(sx_areas_count(areas), 1);
            let mut area = SxArea::default();
            assert_eq!(sx_areas_at(areas, 0, &mut area), 1);
            assert_eq!(
                (area.pos, area.source, area.has_source),
                ([3, 0, 1], [30, 4, 1], 1)
            );
            assert_eq!((area.radius, area.data), (3, E2));

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
            assert_eq!(table.think_ticks, [34, 20, 10, 5]);
            assert_eq!(table.pick, [1, 1, 1, 0]);
            assert_eq!(table.pick_top, [3, 2, 2, 0]);
            assert_eq!(table.spot_rank_error, [2, 1, 1, 0]);
            assert_eq!(sx_discipline(0, &table), i32::from(u16::MAX) / 8);
            assert_eq!(sx_discipline(255, null()), i32::from(u16::MAX) / 2);
            assert_eq!(sx_discipline(256, null()), ERROR_ARGUMENT);

            let sq = squad_of(CHEBYSHEV, &[(1, 0, 0), (2, 1, 0)]);
            table.think_ticks = [1, 2, 3, 4];
            assert_eq!(sx_squad_set_skill_table(sq, &table), 0);
            let mut back = SxSkillTable::default();
            assert_eq!(sx_squad_skill_table(sq, &mut back), 0);
            assert_eq!(back.think_ticks, [1, 2, 3, 4]);
            table.pick[2] = 3;
            assert_eq!(sx_squad_set_skill_table(sq, &table), ERROR_ARGUMENT);
            assert_eq!(sx_squad_skill_table(sq, &mut back), 0);
            assert_eq!(back.pick[2], 1, "a refused table changes nothing");
            sx_squad_free(sq);
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
            assert_eq!(sx_keeper_set(k, &goal, 10), 0);
            let bad = SxGoal { start: 7, ..goal };
            assert_eq!(sx_keeper_set(k, &bad, 10), ERROR_ARGUMENT);
            assert_eq!(sx_keeper_goal(k, &mut g), 1);
            assert_eq!(g, goal);
            assert_eq!(sx_keeper_inputs(k, 15, 1, 0, 0, CHEBYSHEV, 1, 9, i), 0);
            assert_eq!(inputs, [1, 3, 5, 1, 9]);
            assert_eq!(sx_keeper_inputs(k, 15, 1, 0, 0, 7, 1, 9, i), ERROR_ARGUMENT);
            assert_eq!(sx_keeper_postpone(k, 20), 0);
            assert_eq!(sx_keeper_broken(k, 29, 10), 0);
            assert_eq!(sx_keeper_broken(k, 30, 10), 1);
            assert_eq!(sx_keeper_suspend(k), 0);
            assert_eq!(sx_keeper_broken(k, 40, 10), 0);
            assert_eq!(sx_keeper_unsuspend(k, 40), 0);
            assert_eq!(sx_keeper_broken(k, 49, 10), 0);
            assert_eq!(sx_keeper_broken(k, 50, 10), 1);
            assert_eq!(sx_keeper_resume(k, 50), 0);
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
            assert_eq!(sx_squad_resume(n, 1, 0), e);
            assert_eq!(sx_squad_set_spot_radius(n, 1), e);
            assert_eq!(sx_squad_set_gone(n, 1), e);
            assert_eq!(sx_squad_set_leader(n, 1), e);
            assert_eq!(sx_squad_leader(n, &mut id), e);
            assert_eq!(sx_squad_set_skill(n, 1), e);
            assert_eq!(sx_squad_skill(n), e);
            assert_eq!(sx_squad_set_skill_table(sq, null()), e);
            assert_eq!(sx_squad_skill_table(sq, null_mut()), e);
            assert_eq!(sx_skill_table_default(null_mut()), e);
            assert_eq!(sx_squad_set_channel(n, 1, 0, 0, 0), e);
            assert_eq!(sx_squad_set_danger_area(sq, null()), e);
            assert_eq!(sx_squad_danger_area(sq, null_mut()), e);
            assert_eq!(sx_squad_set_target_kind(n, 1), e);
            assert_eq!(sx_squad_set_extra_columns(n, 1), e);
            assert!(sx_squad_knowledge(n).is_null());
            assert!(sx_squad_areas(n).is_null());
            assert_eq!(sx_squad_observe(sq, null(), 0, null_mut()), e);
            assert_eq!(sx_squad_report(sq, null(), 0), e);
            assert_eq!(sx_squad_targets(sq, null_mut(), 1), e);
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
            assert_eq!(sx_tactic_set_trigger(t, 0, 0), e);
            sx_tactic_free(real);
            assert_eq!(sx_keeper_set(k, &SxGoal::default(), 0), e);
            let real = sx_keeper_new();
            assert_eq!(sx_keeper_set(real, null(), 0), e);
            assert_eq!(sx_keeper_goal(real, null_mut()), e);
            assert_eq!(sx_keeper_inputs(real, 0, 0, 0, 0, 0, 0, 0, null_mut()), e);
            assert_eq!(sx_keeper_clear(k), e);
            assert_eq!(sx_keeper_postpone(k, 0), e);
            assert_eq!(sx_keeper_resume(k, 0), e);
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
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_RUSH, 0), 0);
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_LINE, 2), 1);
            assert_eq!(sx_squad_add_shipped(sq, TACTIC_FLANK_FRONT, 4), 2);
            assert_eq!(sx_squad_observe(sq, &enemy(E1, 20, 0), 0, null_mut()), 0);
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
