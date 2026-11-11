//! C interface of Sagax. See `include/sagax.h` for the contract.
//!
//! Everything unsafe in Sagax lives here: pointer arguments and calls into
//! the game's callbacks. The library itself forbids `unsafe`. The functions
//! only translate; the work is done by [`Navigator`], [`FlowField`] and
//! [`DStarLite`].

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use sagax_nav::Cost;
use sagax_nav::Limits;
use sagax_nav::dstar::{DStarLite, DStarStatus};
use sagax_nav::flow::FlowField;
use sagax_nav::formation::Formation;
use sagax_nav::grid::{Cell, CellMap, DenseGrid, GridRules, Moves};
use sagax_nav::hex::{HexLayout, HexMap};
use sagax_nav::nav::{Navigator, Output, Report, Status, WayOptions, write_way};
use sagax_nav::topology::Topology;
use sagax_nav::trace::{TraceEvent, TraceSink, Tracer};

/// Version of the C interface (`SX_API_VERSION`).
pub const API_VERSION: u32 = 4;

const ERROR_ARGUMENT: i32 = -1;
const ERROR_INTERNAL: i32 = -2;

const FOUR_WAYS: u32 = 1;
const CUT_CORNERS: u32 = 2;

const RUNNING: i32 = 1;
const FOUND: i32 = 2;
const PARTIAL: i32 = 3;
const UNREACHABLE: i32 = 4;
const DONE: i32 = 5;

/// `sx_step_fn`.
pub type StepFn = unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32, i32) -> i32;
/// `sx_move_cost_fn`.
pub type MoveCostFn = unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32, i32, i32) -> i32;
/// `sx_steps_fn`.
pub type StepsFn = unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32, i32, *const i32, *mut i32);

/// `sx_trace_fn`.
pub type TraceFn = unsafe extern "C" fn(*mut c_void, *const SxEvent);

/// `sx_event`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxEvent {
    /// `SX_TRACE_*`.
    pub kind: i32,
    /// The cell.
    pub cell: Point,
    /// Where the cell was reached from.
    pub parent: Point,
    /// Cost.
    pub cost: i32,
    /// Estimate still to go.
    pub estimate: i32,
    /// Cells expanded so far.
    pub expanded: i32,
    /// 1 if a goal was reached.
    pub found: i32,
}

/// A tracer that calls the game's trace callback, or `None` for null.
fn tracer_of(trace: Option<TraceFn>, context: *mut c_void) -> Option<Tracer> {
    let f = trace?;
    Some(Box::new(move |e: &TraceEvent| {
        let event = SxEvent {
            kind: e.kind as i32,
            cell: Point::of(e.cell),
            parent: Point::of(e.parent),
            cost: to_i32(e.cost),
            estimate: to_i32(e.estimate),
            expanded: to_i32(e.expanded),
            found: i32::from(e.found),
        };
        // SAFETY: the game passed a valid callback and context for as long
        // as tracing is on (sagax.h).
        unsafe { f(context, &event) };
    }))
}

/// `sx_point`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    /// Column.
    pub x: i32,
    /// Row.
    pub y: i32,
    /// Level.
    pub level: i32,
}

impl Point {
    /// A point.
    pub const fn new(x: i32, y: i32, level: i32) -> Self {
        Self { x, y, level }
    }

    fn cell(self) -> Cell {
        Cell::new(self.x, self.y, self.level)
    }

    fn of(cell: Cell) -> Self {
        Self::new(cell.x, cell.y, cell.level)
    }
}

/// `sx_map`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SxMap {
    /// Required.
    pub step: Option<StepFn>,
    /// Moves off the start cell; null: `step`.
    pub start_step: Option<StepFn>,
    /// Extra costs; null: none.
    pub move_cost: Option<MoveCostFn>,
    /// All neighbours at once; null: `step` per neighbour.
    pub steps: Option<StepsFn>,
    /// Passed to all callbacks.
    pub context: *mut c_void,
}

/// `sx_rules`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SxRules {
    /// `SX_GRID_*`.
    pub grid: i32,
    /// `SX_FOUR_WAYS`, `SX_CUT_CORNERS`.
    pub flags: u32,
    /// Cost of a straight move (hex: of every move).
    pub cost_straight: i32,
    /// Cost of a diagonal move (square only).
    pub cost_diagonal: i32,
    /// Cells one search may touch.
    pub node_capacity: i32,
    /// Largest extra move cost smoothing may cross.
    pub smooth_max_extra: i32,
}

/// `sx_result`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxResult {
    /// `SX_RUNNING` ... `SX_DONE`, 0 before any search.
    pub status: i32,
    /// Cells of the whole way, area or field.
    pub cells: i32,
    /// Cost of the way.
    pub cost: i32,
    /// Cells expanded since the search began.
    pub expanded: i32,
    /// 1 if the node capacity was too small.
    pub node_limit_hit: i32,
}

/// The game's map, seen through the callbacks of an `sx_map`.
struct CallbackMap {
    map: SxMap,
    step: StepFn,
}

impl CallbackMap {
    fn new(map: *const SxMap) -> Option<Self> {
        // SAFETY: the caller of the sx_* function passes null or a valid map.
        let map = *unsafe { map.as_ref() }?;
        Some(Self {
            step: map.step?,
            map,
        })
    }

    fn call(&self, f: StepFn, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        // SAFETY: the caller of the sx_* function promised a valid callback
        // and context for the duration of the call (sagax.h).
        let level = unsafe { f(self.map.context, from.x, from.y, from.level, dx, dy) };
        (level >= 0).then_some(level)
    }

    fn step_fn(&self, from_start: bool) -> StepFn {
        match self.map.start_step {
            Some(f) if from_start => f,
            _ => self.step,
        }
    }

    /// Asks `steps` (or `step` per pair) for `count` offsets; `false` if
    /// there is no batch callback.
    fn batch(&self, from: Cell, from_start: bool, offsets: &[i32], levels: &mut [i32]) -> bool {
        let Some(f) = self.map.steps else {
            return false;
        };
        let count = levels.len();
        debug_assert_eq!(offsets.len(), 2 * count);
        levels.fill(-1);
        // SAFETY: as in `call`; `offsets` holds `count` pairs, `levels` has
        // room for `count` answers.
        unsafe {
            f(
                self.map.context,
                from.x,
                from.y,
                from.level,
                i32::from(from_start),
                count as i32,
                offsets.as_ptr(),
                levels.as_mut_ptr(),
            );
        }
        true
    }

    fn extra(&self, from: Cell, to: Cell) -> Option<Cost> {
        let Some(f) = self.map.move_cost else {
            return Some(0);
        };
        // SAFETY: as in `call`.
        let extra = unsafe {
            f(
                self.map.context,
                from.x,
                from.y,
                from.level,
                to.x,
                to.y,
                to.level,
            )
        };
        Cost::try_from(extra).ok()
    }
}

impl CellMap for CallbackMap {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        self.call(self.step, from, dx, dy)
    }

    fn start_step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        self.call(self.step_fn(true), from, dx, dy)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        self.extra(from, to)
    }

    fn steps(&mut self, from: Cell, from_start: bool, count: usize, out: &mut [Option<i32>; 8]) {
        let mut offsets = [0i32; 16];
        for (i, &(dx, dy)) in DenseGrid::DIRECTIONS[..count].iter().enumerate() {
            offsets[2 * i] = dx;
            offsets[2 * i + 1] = dy;
        }
        let mut levels = [-1i32; 8];
        if self.batch(
            from,
            from_start,
            &offsets[..2 * count],
            &mut levels[..count],
        ) {
            for i in 0..count {
                out[i] = (levels[i] >= 0).then_some(levels[i]);
            }
            return;
        }
        let f = self.step_fn(from_start);
        for (i, &(dx, dy)) in DenseGrid::DIRECTIONS[..count].iter().enumerate() {
            out[i] = self.call(f, from, dx, dy);
        }
    }
}

/// Hex grids: the step callback is asked with the offset (dx, dy) of the
/// neighbour cell, each in -1..1.
impl HexMap for CallbackMap {
    fn step(&mut self, from: Cell, _dir: usize, to: (i32, i32)) -> Option<i32> {
        self.call(self.step, from, to.0 - from.x, to.1 - from.y)
    }

    fn start_step(&mut self, from: Cell, _dir: usize, to: (i32, i32)) -> Option<i32> {
        self.call(self.step_fn(true), from, to.0 - from.x, to.1 - from.y)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        self.extra(from, to)
    }

    fn steps(
        &mut self,
        from: Cell,
        from_start: bool,
        to: &[(i32, i32); 6],
        out: &mut [Option<i32>; 6],
    ) {
        let mut offsets = [0i32; 12];
        for (i, &(x, y)) in to.iter().enumerate() {
            offsets[2 * i] = x - from.x;
            offsets[2 * i + 1] = y - from.y;
        }
        let mut levels = [-1i32; 6];
        if self.batch(from, from_start, &offsets, &mut levels) {
            for i in 0..6 {
                out[i] = (levels[i] >= 0).then_some(levels[i]);
            }
            return;
        }
        let f = self.step_fn(from_start);
        for i in 0..6 {
            out[i] = self.call(f, from, offsets[2 * i], offsets[2 * i + 1]);
        }
    }
}

/// Valid rules as topology, capacity and smoothing tolerance.
fn rules_of(rules: *const SxRules) -> Option<(Topology, usize, Cost)> {
    // SAFETY: the caller passes null or valid rules.
    let rules = *unsafe { rules.as_ref() }?;
    let positive = |v: i32| Cost::try_from(v).ok().filter(|&c| c > 0);
    let straight = positive(rules.cost_straight)?;
    let capacity = usize::try_from(rules.node_capacity)
        .ok()
        .filter(|&c| c > 0)?;
    let tolerance = Cost::try_from(rules.smooth_max_extra).ok()?;
    let layout = match rules.grid {
        0 => None,
        1 => Some(HexLayout::OddR),
        2 => Some(HexLayout::EvenR),
        3 => Some(HexLayout::OddQ),
        4 => Some(HexLayout::EvenQ),
        _ => return None,
    };
    let topology = match layout {
        Some(layout) => Topology::Hex {
            layout,
            step_cost: straight,
        },
        None => Topology::Square(GridRules {
            moves: if rules.flags & FOUR_WAYS != 0 {
                Moves::Four
            } else {
                Moves::Eight
            },
            cut_corners: rules.flags & CUT_CORNERS != 0,
            straight,
            diagonal: positive(rules.cost_diagonal)?,
        }),
    };
    Some((topology, capacity, tolerance))
}

fn budget_of(budget: i32) -> Option<usize> {
    match budget {
        0 => Some(usize::MAX),
        b => usize::try_from(b).ok(),
    }
}

fn output_of(output: i32) -> Option<Output> {
    match output {
        0 => Some(Output::Cells),
        1 => Some(Output::Turns),
        2 => Some(Output::Smooth),
        _ => None,
    }
}

/// `max` entries at `out` must be writable (or `max` 0).
fn room<T>(out: *mut T, max: i32) -> Option<usize> {
    let max = usize::try_from(max).ok()?;
    (max == 0 || !out.is_null()).then_some(max)
}

fn to_i32<T: TryInto<i32>>(value: T) -> i32 {
    value.try_into().unwrap_or(i32::MAX)
}

fn status_code(status: Option<Status>) -> i32 {
    match status {
        None => 0,
        Some(Status::Running) => RUNNING,
        Some(Status::Found) => FOUND,
        Some(Status::Partial) => PARTIAL,
        Some(Status::Unreachable) => UNREACHABLE,
        Some(Status::Done) => DONE,
    }
}

fn write_result(result: *mut SxResult, value: SxResult) {
    // SAFETY: the caller passes null or a valid result.
    if let Some(result) = unsafe { result.as_mut() } {
        *result = value;
    }
}

fn result_of(report: Report) -> SxResult {
    SxResult {
        status: status_code(report.status),
        cells: to_i32(report.cells),
        cost: to_i32(report.cost),
        expanded: to_i32(report.expanded),
        node_limit_hit: i32::from(report.node_limit_hit),
    }
}

/// Copies at most `max` cells into `out`; returns how many.
fn write_cells(cells: &[Cell], out: *mut Point, max: usize) -> i32 {
    let n = cells.len().min(max);
    for (i, &cell) in cells[..n].iter().enumerate() {
        // SAFETY: the caller promised room for `max` points at `out`.
        unsafe { out.add(i).write(Point::of(cell)) };
    }
    to_i32(n)
}

/// A slice of `count` items at `ptr` (null allowed for 0).
///
/// # Safety
/// `ptr` holds `count` items for the duration of the call.
unsafe fn slice_of<'a, T>(ptr: *const T, count: usize) -> &'a [T] {
    if count == 0 {
        &[]
    } else {
        // SAFETY: see above.
        unsafe { std::slice::from_raw_parts(ptr, count) }
    }
}

fn guarded(f: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(ERROR_INTERNAL)
}

fn create<T>(f: impl FnOnce() -> Option<T>) -> *mut T {
    catch_unwind(AssertUnwindSafe(f))
        .ok()
        .flatten()
        .map_or(std::ptr::null_mut(), |v| Box::into_raw(Box::new(v)))
}

/// # Safety
/// `ptr` is null or came from `Box::into_raw` and is not used again.
unsafe fn destroy<T>(ptr: *mut T) {
    if !ptr.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(ptr) });
    }
}

/// `sx_api_version`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_api_version() -> u32 {
    API_VERSION
}

// ---- sx_nav ---------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Begun {
    Nothing,
    Path(Output),
    Area,
}

/// `sx_nav`.
pub struct SxNav {
    nav: Navigator,
    begun: Begun,
    cells: Vec<Cell>,
}

impl SxNav {
    fn write_way(
        &mut self,
        map: &mut CallbackMap,
        output: Output,
        out: *mut Point,
        max: usize,
    ) -> i32 {
        self.nav.set_keep_first(map.map.start_step.is_some());
        write_cells(self.nav.output(map, output), out, max)
    }

    fn take_cells(&mut self, points: &[Point]) {
        self.cells.clear();
        self.cells.extend(points.iter().map(|p| p.cell()));
    }

    fn write_area(&self, cells: *mut Point, costs: *mut i32, max: usize) -> i32 {
        let mut n = 0;
        for (cell, cost) in self.nav.area().take(max) {
            // SAFETY: the caller promised room for `max` entries.
            unsafe {
                cells.add(n).write(Point::of(cell));
                if !costs.is_null() {
                    costs.add(n).write(to_i32(cost));
                }
            }
            n += 1;
        }
        to_i32(n)
    }
}

/// `sx_nav_create`.
///
/// # Safety
/// `rules` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_nav_create(rules: *const SxRules) -> *mut SxNav {
    create(|| {
        let (topology, capacity, tolerance) = rules_of(rules)?;
        let mut nav = Navigator::new(topology, capacity);
        nav.set_smooth_max_extra(tolerance);
        Some(SxNav {
            nav,
            begun: Begun::Nothing,
            cells: Vec::new(),
        })
    })
}

/// `sx_nav_destroy`.
///
/// # Safety
/// `nav` is null or came from `sx_nav_create` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_nav_destroy(nav: *mut SxNav) {
    // SAFETY: see above.
    unsafe { destroy(nav) }
}

/// `sx_nav_find_path`.
///
/// # Safety
/// As documented in `sagax.h`: valid pointers, `goal_count` points at
/// `goals`, room for `max_points`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_nav_find_path(
    nav: *mut SxNav,
    map: *const SxMap,
    start: Point,
    goals: *const Point,
    goal_count: i32,
    budget: i32,
    output: i32,
    points: *mut Point,
    max_points: i32,
    result: *mut SxResult,
) -> i32 {
    // SAFETY: see above.
    let (Some(s), Some(mut map)) = (unsafe { nav.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Some(budget), Some(output), Some(max), Some(count)) = (
        budget_of(budget),
        output_of(output),
        room(points, max_points),
        room(goals.cast_mut(), goal_count),
    ) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        // SAFETY: the caller promised `goal_count` points at `goals`.
        s.take_cells(unsafe { slice_of(goals, count) });
        let report = s.nav.find_path(&mut map, start.cell(), &s.cells, budget);
        s.begun = Begun::Path(output);
        write_result(result, result_of(report));
        s.write_way(&mut map, output, points, max)
    })
}

/// `sx_nav_flee`.
///
/// # Safety
/// As documented in `sagax.h`: valid pointers, `threat_count` points at
/// `threats`, room for `max_points`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_nav_flee(
    nav: *mut SxNav,
    map: *const SxMap,
    start: Point,
    threats: *const Point,
    threat_count: i32,
    max_cost: i32,
    output: i32,
    points: *mut Point,
    max_points: i32,
    result: *mut SxResult,
) -> i32 {
    // SAFETY: see above.
    let (Some(s), Some(mut map)) = (unsafe { nav.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Ok(max_cost), Some(output), Some(max), Some(count)) = (
        Cost::try_from(max_cost),
        output_of(output),
        room(points, max_points),
        room(threats.cast_mut(), threat_count),
    ) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        // SAFETY: the caller promised `threat_count` points at `threats`.
        s.take_cells(unsafe { slice_of(threats, count) });
        let report = s.nav.flee(&mut map, start.cell(), &s.cells, max_cost);
        s.begun = Begun::Path(output);
        write_result(result, result_of(report));
        s.write_way(&mut map, output, points, max)
    })
}

/// `sx_nav_reachable`.
///
/// # Safety
/// As documented in `sagax.h`: valid pointers, room for `max_cells`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_nav_reachable(
    nav: *mut SxNav,
    map: *const SxMap,
    start: Point,
    max_cost: i32,
    budget: i32,
    cells: *mut Point,
    costs: *mut i32,
    max_cells: i32,
    result: *mut SxResult,
) -> i32 {
    // SAFETY: see above.
    let (Some(s), Some(mut map)) = (unsafe { nav.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Ok(max_cost), Some(budget), Some(max)) = (
        Cost::try_from(max_cost),
        budget_of(budget),
        room(cells, max_cells),
    ) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        let report = s.nav.reachable(&mut map, start.cell(), max_cost, budget);
        s.begun = Begun::Area;
        write_result(result, result_of(report));
        s.write_area(cells, costs, max)
    })
}

/// `sx_nav_continue`.
///
/// # Safety
/// As documented in `sagax.h`: valid pointers, room for `max_points`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_nav_continue(
    nav: *mut SxNav,
    map: *const SxMap,
    budget: i32,
    points: *mut Point,
    costs: *mut i32,
    max_points: i32,
    result: *mut SxResult,
) -> i32 {
    // SAFETY: see above.
    let (Some(s), Some(mut map)) = (unsafe { nav.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Some(budget), Some(max)) = (budget_of(budget), room(points, max_points)) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        let report = s.nav.resume(&mut map, budget);
        write_result(result, result_of(report));
        match s.begun {
            Begun::Nothing => 0,
            Begun::Path(output) => s.write_way(&mut map, output, points, max),
            Begun::Area => s.write_area(points, costs, max),
        }
    })
}

/// `sx_nav_path_to`.
///
/// # Safety
/// As documented in `sagax.h`: valid pointers, room for `max_points`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_nav_path_to(
    nav: *mut SxNav,
    map: *const SxMap,
    cell: Point,
    output: i32,
    points: *mut Point,
    max_points: i32,
    result: *mut SxResult,
) -> i32 {
    // SAFETY: see above.
    let (Some(s), Some(mut map)) = (unsafe { nav.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Some(output), Some(max)) = (output_of(output), room(points, max_points)) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        let Some(cost) = s.nav.path_to(cell.cell()) else {
            write_result(
                result,
                SxResult {
                    status: UNREACHABLE,
                    ..SxResult::default()
                },
            );
            return 0;
        };
        write_result(
            result,
            SxResult {
                status: FOUND,
                cells: to_i32(s.nav.path().len()),
                cost: to_i32(cost),
                expanded: 0,
                node_limit_hit: 0,
            },
        );
        s.write_way(&mut map, output, points, max)
    })
}

/// `sx_nav_way`.
///
/// # Safety
/// As documented in `sagax.h`: valid pointers, room for `max_points`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_nav_way(
    nav: *mut SxNav,
    map: *const SxMap,
    output: i32,
    points: *mut Point,
    max_points: i32,
) -> i32 {
    // SAFETY: see above.
    let (Some(s), Some(mut map)) = (unsafe { nav.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Some(output), Some(max)) = (output_of(output), room(points, max_points)) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| s.write_way(&mut map, output, points, max))
}

/// `sx_nav_formation`.
///
/// # Safety
/// As documented in `sagax.h`: `2 * members` numbers at `slots` (or
/// null), room for `(members + 1) * max_cells` points at `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_nav_formation(
    nav: *mut SxNav,
    map: *const SxMap,
    slots: *const i32,
    members: i32,
    out: *mut Point,
    max_cells: i32,
) -> i32 {
    // SAFETY: see above.
    let (Some(s), Some(mut map)) = (unsafe { nav.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Ok(members), Some(max)) = (usize::try_from(members), room(out, max_cells)) else {
        return ERROR_ARGUMENT;
    };
    if slots.is_null() && members != 3 {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        let formation = if slots.is_null() {
            Formation::block_of_four()
        } else {
            // SAFETY: the caller promised `2 * members` numbers at `slots`.
            let raw = unsafe { slice_of(slots, 2 * members) };
            Formation {
                slots: raw
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&[f, r]| (f, r))
                    .collect(),
            }
        };
        s.nav.formation(&mut map, &formation);
        let path = s.nav.path();
        let rows = s.nav.members();
        let n = path.len().min(max);
        for (row, cells) in std::iter::once(path)
            .chain(rows.iter().map(Vec::as_slice))
            .enumerate()
        {
            // SAFETY: the caller promised room for `(members + 1) * max_cells`.
            write_cells(
                &cells[..n.min(cells.len())],
                unsafe { out.add(row * max) },
                n,
            );
        }
        to_i32(n)
    })
}

// ---- sx_flow --------------------------------------------------------------

/// `sx_flow`.
pub struct SxFlow {
    field: FlowField<Cell>,
    tracer: Option<Tracer>,
    topology: Topology,
    capacity: usize,
    goals: Vec<Cell>,
}

/// `sx_flow_create`.
///
/// # Safety
/// `rules` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_create(rules: *const SxRules) -> *mut SxFlow {
    create(|| {
        let (topology, capacity, _) = rules_of(rules)?;
        Some(SxFlow {
            field: FlowField::with_capacity(capacity),
            tracer: None,
            topology,
            capacity,
            goals: Vec::new(),
        })
    })
}

/// `sx_flow_destroy`.
///
/// # Safety
/// `flow` is null or came from `sx_flow_create` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_destroy(flow: *mut SxFlow) {
    // SAFETY: see above.
    unsafe { destroy(flow) }
}

/// `sx_flow_build`.
///
/// # Safety
/// As documented in `sagax.h`: `goal_count` points at `goals`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_build(
    flow: *mut SxFlow,
    map: *const SxMap,
    goals: *const Point,
    goal_count: i32,
    max_cost: i32,
    result: *mut SxResult,
) -> i32 {
    // SAFETY: see above.
    let (Some(f), Some(mut map)) = (unsafe { flow.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Ok(max_cost), Some(count)) =
        (Cost::try_from(max_cost), room(goals.cast_mut(), goal_count))
    else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        // SAFETY: the caller promised `goal_count` points at `goals`.
        let goals = unsafe { slice_of(goals, count) };
        f.goals.clear();
        f.goals.extend(goals.iter().map(|p| p.cell()));
        let limits = Limits {
            max_nodes: f.capacity,
            max_cost,
        };
        let mut graph = f.topology.graph(&mut map, None);
        let goals = f.goals.iter().copied();
        match f.tracer.as_deref_mut() {
            None => f.field.build(&mut graph, goals, limits),
            Some(t) => f
                .field
                .build_traced(&mut graph, goals, limits, &mut TraceSink(t)),
        }
        write_result(
            result,
            SxResult {
                status: DONE,
                cells: to_i32(f.field.len()),
                ..SxResult::default()
            },
        );
        0
    })
}

/// `sx_flow_next`.
///
/// # Safety
/// As documented in `sagax.h`: `count` points at `from`, room for `count`
/// entries at `next` and `costs` (each may be null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_next(
    flow: *mut SxFlow,
    map: *const SxMap,
    from: *const Point,
    count: i32,
    next: *mut Point,
    costs: *mut i32,
) -> i32 {
    // SAFETY: see above.
    let (Some(f), Some(mut map)) = (unsafe { flow.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let Some(count) = room(from.cast_mut(), count) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        // SAFETY: the caller promised `count` points at `from`.
        let from = unsafe { slice_of(from, count) };
        let mut graph = f.topology.graph(&mut map, None);
        let mut moved = 0;
        for (i, p) in from.iter().enumerate() {
            let cell = p.cell();
            let cost = f.field.cost(cell);
            let step = cost.and_then(|_| f.field.next(&mut graph, cell));
            moved += i32::from(step.is_some());
            // SAFETY: the caller promised room for `count` entries.
            unsafe {
                if !next.is_null() {
                    next.add(i).write(Point::of(step.unwrap_or(cell)));
                }
                if !costs.is_null() {
                    costs.add(i).write(cost.map_or(-1, to_i32));
                }
            }
        }
        moved
    })
}

// ---- sx_dstar -------------------------------------------------------------

/// `sx_dstar`.
pub struct SxDStar {
    planner: DStarLite<Cell, Topology>,
    tracer: Option<Tracer>,
    topology: Topology,
    smooth_max_extra: Cost,
    path: Vec<Cell>,
    list: Vec<Cell>,
    scratch: Vec<Cell>,
}

/// `sx_dstar_create`.
///
/// # Safety
/// `rules` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_create(rules: *const SxRules) -> *mut SxDStar {
    create(|| {
        let (topology, capacity, tolerance) = rules_of(rules)?;
        let mut planner = DStarLite::with_capacity(capacity);
        planner.set_max_nodes(capacity);
        Some(SxDStar {
            planner,
            tracer: None,
            topology,
            smooth_max_extra: tolerance,
            path: Vec::with_capacity(capacity),
            list: Vec::with_capacity(capacity),
            scratch: Vec::with_capacity(capacity),
        })
    })
}

/// `sx_dstar_destroy`.
///
/// # Safety
/// `dstar` is null or came from `sx_dstar_create` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_destroy(dstar: *mut SxDStar) {
    // SAFETY: see above.
    unsafe { destroy(dstar) }
}

/// `sx_dstar_begin`.
///
/// # Safety
/// `dstar` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_begin(dstar: *mut SxDStar, start: Point, goal: Point) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { dstar.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    if goal.level < 0 {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        d.planner.begin(start.cell(), goal.cell(), d.topology);
        d.path.clear();
        0
    })
}

/// `sx_dstar_compute`.
///
/// # Safety
/// As documented in `sagax.h`: valid pointers, room for `max_points`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_dstar_compute(
    dstar: *mut SxDStar,
    map: *const SxMap,
    budget: i32,
    output: i32,
    points: *mut Point,
    max_points: i32,
    result: *mut SxResult,
) -> i32 {
    // SAFETY: see above.
    let (Some(d), Some(mut map)) = (unsafe { dstar.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let (Some(budget), Some(output), Some(max)) = (
        budget_of(budget),
        output_of(output),
        room(points, max_points),
    ) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        let status = {
            let mut graph = d.topology.graph(&mut map, None);
            let status = match d.tracer.as_deref_mut() {
                None => d.planner.compute(&mut graph, budget),
                Some(t) => d
                    .planner
                    .compute_traced(&mut graph, budget, &mut TraceSink(t)),
            };
            if !(status == DStarStatus::Found && d.planner.path(&mut graph, &mut d.path)) {
                d.path.clear();
            }
            status
        };
        let code = match status {
            DStarStatus::Idle => 0,
            DStarStatus::Running => RUNNING,
            DStarStatus::Found if d.path.is_empty() => UNREACHABLE,
            DStarStatus::Found => FOUND,
            DStarStatus::Unreachable => UNREACHABLE,
        };
        write_result(
            result,
            SxResult {
                status: code,
                cells: to_i32(d.path.len()),
                cost: if code == FOUND {
                    d.planner.cost().map_or(0, to_i32)
                } else {
                    0
                },
                expanded: to_i32(d.planner.expanded()),
                node_limit_hit: i32::from(d.planner.node_limit_hit()),
            },
        );
        let options = WayOptions {
            keep_first: false,
            smooth_max_extra: d.smooth_max_extra,
        };
        write_way(
            d.topology,
            &mut map,
            &d.path,
            output,
            options,
            &mut d.scratch,
            &mut d.list,
        );
        write_cells(&d.list, points, max)
    })
}

/// `sx_dstar_move_start`.
///
/// # Safety
/// `dstar` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_move_start(dstar: *mut SxDStar, cell: Point) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { dstar.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        d.planner.move_start(cell.cell());
        0
    })
}

/// `sx_dstar_cells_changed`.
///
/// # Safety
/// As documented in `sagax.h`: `count` points at `cells`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_cells_changed(
    dstar: *mut SxDStar,
    map: *const SxMap,
    cells: *const Point,
    count: i32,
) -> i32 {
    // SAFETY: see above.
    let (Some(d), Some(mut map)) = (unsafe { dstar.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let Some(count) = room(cells.cast_mut(), count) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        // SAFETY: the caller promised `count` points at `cells`.
        let cells = unsafe { slice_of(cells, count) };
        d.list.clear();
        d.list.extend(cells.iter().map(|p| p.cell()));
        let mut graph = d.topology.graph(&mut map, None);
        let changed = d
            .topology
            .cells_changed(&mut d.planner, &mut graph, &d.list, &mut d.scratch);
        to_i32(changed)
    })
}

// ---- tracing ---------------------------------------------------------------

/// `sx_nav_set_trace`.
///
/// # Safety
/// `nav` is null or valid; `trace` and `context` stay valid while tracing is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_nav_set_trace(
    nav: *mut SxNav,
    trace: Option<TraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    let Some(s) = (unsafe { nav.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    s.nav.set_tracer(tracer_of(trace, context));
    0
}

/// `sx_flow_set_trace`.
///
/// # Safety
/// As [`sx_nav_set_trace`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_set_trace(
    flow: *mut SxFlow,
    trace: Option<TraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    let Some(f) = (unsafe { flow.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    f.tracer = tracer_of(trace, context);
    0
}

/// `sx_dstar_set_trace`.
///
/// # Safety
/// As [`sx_nav_set_trace`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_set_trace(
    dstar: *mut SxDStar,
    trace: Option<TraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { dstar.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    d.tracer = tracer_of(trace, context);
    0
}

#[cfg(test)]
mod tests;

pub mod decide;
