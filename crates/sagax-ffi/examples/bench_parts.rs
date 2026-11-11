//! Where does the search time go? Random 128 x 96 maps (two levels, ramps,
//! blocked cells), searched three ways:
//!   1. core search, map as plain Rust (callbacks inlined)
//!   2. core search, map behind an `extern "C"` function pointer (as from C)
//!   3. the same through sx_nav_find_path (step per neighbour, and batched)
//!
//! Run: cargo run --release -p sagax-ffi --example bench_parts

use std::ffi::c_void;
use std::time::Instant;

use sagax::{Point, SxMap, SxResult, SxRules, sx_nav_create, sx_nav_find_path};
use sagax_core::NoopSink;
use sagax_nav::grid::{Cell, CellGoal, CellMap, GridGraph, GridRules};
use sagax_nav::{Limits, Search};

const W: i32 = 128;
const H: i32 = 96;

struct Map {
    open: Vec<bool>, // [level][y][x]
    ramp: Vec<bool>,
    danger: Vec<bool>,
}

static mut MAP: Option<Map> = None;

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1103515245).wrapping_add(12345);
        (self.0 >> 16) & 0x7fff
    }
}

fn make_map(rng: &mut Rng, walls: u32) -> Map {
    let n = (W * H) as usize;
    let mut open = vec![false; 2 * n];
    for l in 0..2 {
        for i in 0..n {
            open[l * n + i] = rng.next() % 100 >= if l == 0 { walls } else { 60 };
        }
    }
    let mut ramp = vec![false; n];
    let mut danger = vec![false; n];
    for i in 0..n {
        ramp[i] = rng.next().is_multiple_of(40);
        danger[i] = rng.next().is_multiple_of(200);
    }
    Map { open, ramp, danger }
}

#[inline]
fn step_on(map: &Map, cx: i32, cy: i32, level: i32, dx: i32, dy: i32) -> i32 {
    let (x, y) = (cx + dx, cy + dy);
    if x < 0 || y < 0 || x >= W || y >= H {
        return -1;
    }
    let i = (y * W + x) as usize;
    let n = (W * H) as usize;
    if map.danger[i] {
        return -1;
    }
    if map.open[level as usize * n + i] {
        return level;
    }
    if map.ramp[i] && map.open[(1 - level) as usize * n + i] {
        return 1 - level;
    }
    -1
}

impl CellMap for &Map {
    #[inline]
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let l = step_on(self, from.x, from.y, from.level, dx, dy);
        (l >= 0).then_some(l)
    }
}

#[allow(static_mut_refs)]
unsafe extern "C" fn c_step(_: *mut c_void, cx: i32, cy: i32, level: i32, dx: i32, dy: i32) -> i32 {
    let map = unsafe { MAP.as_ref().unwrap() };
    step_on(map, cx, cy, level, dx, dy)
}

struct FnMap(unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32, i32) -> i32);
impl CellMap for FnMap {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let l = unsafe { (self.0)(std::ptr::null_mut(), from.x, from.y, from.level, dx, dy) };
        (l >= 0).then_some(l)
    }
}

#[allow(static_mut_refs)]
unsafe extern "C" fn c_steps(
    _: *mut c_void,
    cx: i32,
    cy: i32,
    level: i32,
    _from_start: i32,
    count: i32,
    offsets: *const i32,
    levels: *mut i32,
) {
    let map = unsafe { MAP.as_ref().unwrap() };
    for i in 0..count as usize {
        let (dx, dy) = unsafe { (*offsets.add(2 * i), *offsets.add(2 * i + 1)) };
        unsafe { levels.add(i).write(step_on(map, cx, cy, level, dx, dy)) };
    }
}

#[allow(static_mut_refs)]
fn main() {
    let mut rng = Rng(12345);
    let rules = GridRules::default();
    let mut jobs = Vec::new();
    let mut maps = Vec::new();
    for round in 0..400 {
        if round % 20 == 0 {
            maps.push(make_map(&mut rng, 10 + (round / 20) % 4 * 10));
        }
        let map = maps.last().unwrap();
        let (sx, sy, sl) = loop {
            let sx = (rng.next() % W as u32) as i32;
            let sy = (rng.next() % H as u32) as i32;
            let sl = (rng.next() % 2) as i32;
            let i = (sy * W + sx) as usize;
            if map.open[sl as usize * (W * H) as usize + i] && !map.danger[i] {
                break (sx, sy, sl);
            }
        };
        let gx = (rng.next() % W as u32) as i32;
        let gy = (rng.next() % H as u32) as i32;
        let gl = if round % 3 == 0 {
            -1
        } else {
            (rng.next() % 2) as i32
        };
        jobs.push((maps.len() - 1, Cell::new(sx, sy, sl), gx, gy, gl));
    }
    let limits = Limits {
        max_nodes: 32768,
        ..Limits::default()
    };
    let budget = 30000;

    let rules_c = SxRules {
        grid: 0,
        flags: 0,
        cost_straight: 10,
        cost_diagonal: 14,
        node_capacity: 32768,
        smooth_max_extra: 0,
    };
    let nav = unsafe { sx_nav_create(&rules_c) };
    for pass in 0..5 {
        // 1. native
        let mut search = Search::with_capacity(32768);
        let t = Instant::now();
        let mut expanded = 0;
        for &(m, start, gx, gy, gl) in &jobs {
            let goal = CellGoal::new(gx, gy, (gl >= 0).then_some(gl), rules);
            let mut g = GridGraph::new(&maps[m], rules, Some(start));
            search.begin(start, &goal, limits);
            search.advance(&mut g, &goal, budget, &mut NoopSink);
            expanded += search.expanded();
        }
        let native = t.elapsed().as_secs_f64() * 1e6 / jobs.len() as f64;

        // 2. function pointer
        let t = Instant::now();
        for &(m, start, gx, gy, gl) in &jobs {
            unsafe { MAP = Some(maps[m].clone_shallow()) };
            let goal = CellGoal::new(gx, gy, (gl >= 0).then_some(gl), rules);
            let mut g = GridGraph::new(FnMap(std::hint::black_box(c_step)), rules, Some(start));
            search.begin(start, &goal, limits);
            search.advance(&mut g, &goal, budget, &mut NoopSink);
        }
        let fnptr = t.elapsed().as_secs_f64() * 1e6 / jobs.len() as f64;

        // 3. through the C interface, one call per neighbour and batched
        let mut ffi = [0.0; 2];
        for (k, batch) in [false, true].into_iter().enumerate() {
            let map = SxMap {
                step: Some(c_step),
                start_step: None,
                move_cost: None,
                steps: batch.then_some(c_steps as sagax::StepsFn),
                context: std::ptr::null_mut(),
            };
            let mut points = vec![Point::default(); 4096];
            let mut result = SxResult::default();
            let t = Instant::now();
            for &(m, start, gx, gy, gl) in &jobs {
                unsafe { MAP = Some(maps[m].clone_shallow()) };
                unsafe {
                    sx_nav_find_path(
                        nav,
                        &map,
                        Point::new(start.x, start.y, start.level),
                        &Point::new(gx, gy, gl),
                        1,
                        budget as i32,
                        1,
                        points.as_mut_ptr(),
                        4096,
                        &mut result,
                    )
                };
            }
            ffi[k] = t.elapsed().as_secs_f64() * 1e6 / jobs.len() as f64;
        }
        println!(
            "pass {pass}: native {native:.1} us, fn pointer {fnptr:.1} us, C step {:.1} us, C steps {:.1} us  ({} expanded/search)",
            ffi[0],
            ffi[1],
            expanded / jobs.len()
        );
    }
}
impl Map {
    // the maps are small; cloning them outside the timed search is fine
    fn clone_shallow(&self) -> Map {
        Map {
            open: self.open.clone(),
            ramp: self.ramp.clone(),
            danger: self.danger.clone(),
        }
    }
}
