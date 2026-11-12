//! WebAssembly interface of Sagax, for JavaScript games (browser, Node, Foundry VTT).
//!
//! Same shape as the C interface (`sagax.h`), but the map is not asked
//! through callbacks: a call from WebAssembly into JavaScript is slow, and JS
//! maps are small. JavaScript fills a [`DenseGrid`] through typed-array views
//! on the WebAssembly memory (costs, walls between cells, zones) and then asks
//! as often as it likes.
//!
//! * Lists go in through the input buffer ([`sx_input`]) and come out of the
//!   output buffer ([`sx_out`]), both `Int32Array`s of `x, y` pairs (areas and
//!   flow answers: `x, y, cost` triples).
//! * Every search also fills the result ([`sx_result`]): status, cells, cost,
//!   expanded, node limit hit, as in `sx_result` of the C interface.
//! * Functions that write a list return its length, others 0; -1 means bad
//!   arguments. Ways always begin with the start.
//!
//! The JS wrapper (`js/sagax.mjs`) hides the raw calls.

use sagax_nav::Cost;
use sagax_nav::Limits;
use sagax_nav::dstar::{DStarLite, DStarStatus};
use sagax_nav::flow::FlowField;
use sagax_nav::formation::Formation;
use sagax_nav::grid::{Cell, DenseGrid, GridRules, Moves};
use sagax_nav::hex::HexLayout;
use sagax_nav::nav::{Navigator, Output, Report, Status, WayOptions, write_way};
use sagax_nav::topology::Topology;
use sagax_nav::trace::{TraceEvent, TraceSink, Tracer};

use std::cell::RefCell;
use std::rc::Rc;

/// Numbers per trace event: kind, x, y, parent x, parent y, cost, estimate,
/// expanded, found.
const TRACE_STRIDE: usize = 9;
/// At most this many events wait to be taken; later ones are dropped.
const TRACE_LIMIT: usize = 200_000;

#[derive(Default)]
struct TraceBuffer {
    events: Vec<i32>,
    dropped: usize,
}

fn tracer_into(buffer: &Rc<RefCell<TraceBuffer>>) -> Tracer {
    let buffer = Rc::clone(buffer);
    Box::new(move |e: &TraceEvent| {
        let mut b = buffer.borrow_mut();
        if b.events.len() >= TRACE_LIMIT * TRACE_STRIDE {
            b.dropped += 1;
            return;
        }
        b.events.extend_from_slice(&[
            e.kind as i32,
            e.cell.x,
            e.cell.y,
            e.parent.x,
            e.parent.y,
            to_i32(e.cost),
            to_i32(e.estimate),
            to_i32(e.expanded),
            i32::from(e.found),
        ]);
    })
}

const ERROR_ARGUMENT: i32 = -1;

const FOUR_WAYS: u32 = 1;
const CUT_CORNERS: u32 = 2;

const RUNNING: i32 = 1;
const FOUND: i32 = 2;
const PARTIAL: i32 = 3;
const UNREACHABLE: i32 = 4;
const DONE: i32 = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Begun {
    Nothing,
    Path(Output),
    Area,
}

/// A grid with its rules, searches and buffers.
pub struct WasmGrid {
    grid: DenseGrid,
    topology: Topology,
    nav: Navigator,
    flow: FlowField<Cell>,
    dstar: DStarLite<Cell, Topology>,
    begun: Begun,
    capacity: usize,
    smooth_max_extra: Cost,
    /// status, cells, cost, expanded, node limit hit
    result: [i32; 5],
    input: Vec<i32>,
    out: Vec<i32>,
    cells: Vec<Cell>,
    path: Vec<Cell>,
    list: Vec<Cell>,
    scratch: Vec<Cell>,
    trace: Rc<RefCell<TraceBuffer>>,
    tracer: Option<Tracer>,
}

/// Grid kinds as in `sx_rules.grid`: 0 square, 1..4 hex odd rows, even rows,
/// odd columns, even columns (Foundry grid types 2..5 minus 1).
fn topology_of(kind: i32, straight: i32, diagonal: i32, flags: u32) -> Option<Topology> {
    let positive = |v: i32| Cost::try_from(v).ok().filter(|&c| c > 0);
    let straight = positive(straight)?;
    let layout = match kind {
        0 => None,
        1 => Some(HexLayout::OddR),
        2 => Some(HexLayout::EvenR),
        3 => Some(HexLayout::OddQ),
        4 => Some(HexLayout::EvenQ),
        _ => return None,
    };
    Some(match layout {
        Some(layout) => Topology::Hex {
            layout,
            step_cost: straight,
        },
        None => Topology::Square(GridRules {
            moves: if flags & FOUR_WAYS != 0 {
                Moves::Four
            } else {
                Moves::Eight
            },
            cut_corners: flags & CUT_CORNERS != 0,
            straight,
            diagonal: positive(diagonal)?,
        }),
    })
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

impl WasmGrid {
    fn new(width: i32, height: i32, topology: Topology) -> Self {
        let capacity = width as usize * height as usize;
        let mut dstar = DStarLite::with_capacity(capacity);
        dstar.set_max_nodes(capacity);
        Self {
            grid: DenseGrid::new(width, height),
            topology,
            nav: Navigator::new(topology, capacity),
            flow: FlowField::with_capacity(capacity),
            dstar,
            begun: Begun::Nothing,
            capacity,
            smooth_max_extra: 0,
            result: [0; 5],
            input: Vec::new(),
            out: Vec::with_capacity(capacity * 3),
            cells: Vec::new(),
            path: Vec::new(),
            list: Vec::new(),
            scratch: Vec::new(),
            trace: Rc::default(),
            tracer: None,
        }
    }

    fn report(&mut self, report: Report) {
        self.result = [
            status_code(report.status),
            to_i32(report.cells),
            to_i32(report.cost),
            to_i32(report.expanded),
            i32::from(report.node_limit_hit),
        ];
    }

    fn write_cells(out: &mut Vec<i32>, cells: &[Cell]) -> i32 {
        out.clear();
        for c in cells {
            out.extend_from_slice(&[c.x, c.y]);
        }
        to_i32(cells.len())
    }

    fn write_way(&mut self, output: Output) -> i32 {
        Self::write_cells(&mut self.out, self.nav.output(&mut self.grid, output))
    }

    fn write_area(&mut self) -> i32 {
        self.out.clear();
        for (c, cost) in self.nav.area() {
            self.out.extend_from_slice(&[c.x, c.y, to_i32(cost)]);
        }
        to_i32(self.out.len() / 3)
    }

    /// The first `count` cells of the input buffer into `self.cells`.
    fn input_cells(&mut self, count: usize) -> bool {
        if self.input.len() < 2 * count {
            return false;
        }
        self.cells.clear();
        self.cells.extend(
            self.input[..2 * count]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&[x, y]| Cell::new(x, y, 0)),
        );
        true
    }
}

macro_rules! grid {
    ($g:expr, $err:expr) => {
        // SAFETY: JS passes a handle from sx_grid_new (wrapper guarantees it).
        match unsafe { $g.as_mut() } {
            Some(g) => g,
            None => return $err,
        }
    };
}

/// A grid of `width` x `height` open cells with fixed rules: `kind` 0
/// square (`straight`, `diagonal`, `flags` 1 four ways, 2 cut corners) or
/// 1..4 hex (`straight` per step). Null on bad arguments.
#[unsafe(no_mangle)]
pub extern "C" fn sx_grid_new(
    width: i32,
    height: i32,
    kind: i32,
    straight: i32,
    diagonal: i32,
    flags: u32,
) -> *mut WasmGrid {
    let Some(topology) = topology_of(kind, straight, diagonal, flags) else {
        return std::ptr::null_mut();
    };
    if width <= 0 || height <= 0 {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(WasmGrid::new(width, height, topology)))
}

/// Frees a grid.
///
/// # Safety
/// `g` is null or from [`sx_grid_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_free(g: *mut WasmGrid) {
    if !g.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(g) });
    }
}

/// Extra cost per cell, `width * height` `u32`s row by row; `0xFFFFFFFF`
/// blocks a cell.
///
/// # Safety
/// `g` from [`sx_grid_new`]. The pointer is valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_costs(g: *mut WasmGrid) -> *mut u32 {
    grid!(g, std::ptr::null_mut()).grid.costs_mut().as_mut_ptr()
}

/// Wall bits per cell, `width * height` bytes; bit `i` blocks the move in
/// direction `i` (square: 0 +x, 1 -x, 2 +y, 3 -y, 4 +x+y, 5 +x-y, 6 -x+y,
/// 7 -x-y; hex: see [`sx_hex_neighbors`]).
///
/// # Safety
/// As [`sx_grid_costs`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_walls(g: *mut WasmGrid) -> *mut u8 {
    grid!(g, std::ptr::null_mut()).grid.walls_mut().as_mut_ptr()
}

/// Zone bits per cell, `width * height` `u32`s; leaving a zone costs
/// [`sx_grid_set_leave_cost`] per zone left.
///
/// # Safety
/// As [`sx_grid_costs`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_zones(g: *mut WasmGrid) -> *mut u32 {
    grid!(g, std::ptr::null_mut()).grid.zones_mut().as_mut_ptr()
}

/// Sets the cost of leaving one zone.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_set_leave_cost(g: *mut WasmGrid, cost: u32) -> i32 {
    grid!(g, ERROR_ARGUMENT).grid.set_leave_cost(cost);
    0
}

/// Smoothing may cross cells whose extra cost is at most `max_extra`.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_set_smooth_max_extra(g: *mut WasmGrid, max_extra: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Ok(max_extra) = Cost::try_from(max_extra) else {
        return ERROR_ARGUMENT;
    };
    w.nav.set_smooth_max_extra(max_extra);
    w.smooth_max_extra = max_extra;
    0
}

/// The input buffer with room for `count` `i32`s. Valid until memory grows.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_input(g: *mut WasmGrid, count: i32) -> *mut i32 {
    let w = grid!(g, std::ptr::null_mut());
    w.input.clear();
    w.input.resize(usize::try_from(count).unwrap_or(0), 0);
    w.input.as_mut_ptr()
}

/// The output buffer (`Int32Array`) of the last call that wrote a list.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_out(g: *mut WasmGrid) -> *const i32 {
    grid!(g, std::ptr::null()).out.as_ptr()
}

/// Five `i32`s about the last search: status (1 running, 2 found, 3 partial,
/// 4 unreachable, 5 done), cells of the whole way or area, cost, cells
/// expanded, 1 if the node limit was hit.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_result(g: *mut WasmGrid) -> *const i32 {
    grid!(g, std::ptr::null()).result.as_ptr()
}

/// The six hex neighbours of `x`, `y` in direction order (the wall bit
/// numbers) as `x, y` pairs. Returns 6, or -1 on a square grid.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_hex_neighbors(g: *mut WasmGrid, x: i32, y: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Topology::Hex { layout, .. } = w.topology else {
        return ERROR_ARGUMENT;
    };
    w.out.clear();
    for dir in 0..6 {
        let (nx, ny) = layout.neighbor(x, y, dir);
        w.out.extend_from_slice(&[nx, ny]);
    }
    6
}

/// Cheapest way from `sx, sy` to the cheapest of `count` goals (`x, y`
/// pairs in the input buffer; on failure to the reachable cell closest to
/// one), written as `output` (0 cells, 1 turning points, 2 smoothed).
/// `budget` 0: no limit.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_find_path(
    g: *mut WasmGrid,
    sx: i32,
    sy: i32,
    count: i32,
    budget: i32,
    output: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let (Some(budget), Some(output), Ok(count)) =
        (budget_of(budget), output_of(output), usize::try_from(count))
    else {
        return ERROR_ARGUMENT;
    };
    if !w.input_cells(count) {
        return ERROR_ARGUMENT;
    }
    let report = w
        .nav
        .find_path(&mut w.grid, Cell::new(sx, sy, 0), &w.cells, budget);
    w.report(report);
    w.begun = Begun::Path(output);
    w.write_way(output)
}

/// A way from `sx, sy` away from `count` threats (`x, y` pairs in the input
/// buffer), within `max_cost`, written as `output`. See
/// `Navigator::flee`.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flee(
    g: *mut WasmGrid,
    sx: i32,
    sy: i32,
    count: i32,
    max_cost: i32,
    output: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let (Some(output), Ok(count), Ok(max_cost)) = (
        output_of(output),
        usize::try_from(count),
        Cost::try_from(max_cost),
    ) else {
        return ERROR_ARGUMENT;
    };
    if !w.input_cells(count) {
        return ERROR_ARGUMENT;
    }
    let report = w
        .nav
        .flee(&mut w.grid, Cell::new(sx, sy, 0), &w.cells, max_cost);
    w.report(report);
    w.begun = Begun::Path(output);
    w.write_way(output)
}

/// Every cell reachable from `sx, sy` at a cost of at most `max_cost`, as
/// `x, y, cost` triples.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_reachable(
    g: *mut WasmGrid,
    sx: i32,
    sy: i32,
    max_cost: i32,
    budget: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let (Some(budget), Ok(max_cost)) = (budget_of(budget), Cost::try_from(max_cost)) else {
        return ERROR_ARGUMENT;
    };
    let report = w
        .nav
        .reachable(&mut w.grid, Cell::new(sx, sy, 0), max_cost, budget);
    w.report(report);
    w.begun = Begun::Area;
    w.write_area()
}

/// Goes on with the last [`sx_find_path`] or [`sx_reachable`] after status
/// running and writes as that call did.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_continue(g: *mut WasmGrid, budget: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Some(budget) = budget_of(budget) else {
        return ERROR_ARGUMENT;
    };
    let report = w.nav.resume(&mut w.grid, budget);
    w.report(report);
    match w.begun {
        Begun::Nothing => 0,
        Begun::Path(output) => w.write_way(output),
        Begun::Area => w.write_area(),
    }
}

/// After a search: the way from its start to `x, y` becomes the current way
/// and is written. Status found, or unreachable (nothing written).
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_path_to(g: *mut WasmGrid, x: i32, y: i32, output: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Some(output) = output_of(output) else {
        return ERROR_ARGUMENT;
    };
    match w.nav.path_to(Cell::new(x, y, 0)) {
        None => {
            w.result = [UNREACHABLE, 0, 0, 0, 0];
            w.out.clear();
            0
        }
        Some(cost) => {
            w.result = [FOUND, to_i32(w.nav.path().len()), to_i32(cost), 0, 0];
            w.write_way(output)
        }
    }
}

/// The current way once more, as `output`.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_way(g: *mut WasmGrid, output: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Some(output) = output_of(output) else {
        return ERROR_ARGUMENT;
    };
    w.write_way(output)
}

/// Formation along the current way for `members` members whose slots
/// (`forward, right` pairs) are in the input buffer. Writes `members + 1`
/// rows of `n` cells (`n` the return value): row 0 the leader's way, row k
/// member k's target while the leader stands on that column's cell.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_formation(g: *mut WasmGrid, members: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Ok(members) = usize::try_from(members) else {
        return ERROR_ARGUMENT;
    };
    if w.input.len() < 2 * members {
        return ERROR_ARGUMENT;
    }
    let formation = Formation {
        slots: w.input[..2 * members]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[f, r]| (f, r))
            .collect(),
    };
    w.nav.formation(&mut w.grid, &formation);
    w.out.clear();
    for row in std::iter::once(w.nav.path()).chain(w.nav.members().iter().map(Vec::as_slice)) {
        for c in row {
            w.out.extend_from_slice(&[c.x, c.y]);
        }
    }
    to_i32(w.nav.path().len())
}

/// Flow field towards the `count` goal cells in the input buffer, up to
/// `max_cost`. Returns the cells in the field. Does not touch the current
/// way.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_build(g: *mut WasmGrid, count: i32, max_cost: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let (Ok(count), Ok(max_cost)) = (usize::try_from(count), Cost::try_from(max_cost)) else {
        return ERROR_ARGUMENT;
    };
    if !w.input_cells(count) {
        return ERROR_ARGUMENT;
    }
    let limits = Limits {
        max_nodes: w.capacity,
        max_cost,
    };
    let mut graph = w.topology.graph(&mut w.grid, None);
    let goals = w.cells.iter().copied();
    match w.tracer.as_deref_mut() {
        None => w.flow.build(&mut graph, goals, limits),
        Some(t) => w
            .flow
            .build_traced(&mut graph, goals, limits, &mut TraceSink(t)),
    }
    to_i32(w.flow.len())
}

/// For the `count` walkers in the input buffer: `x, y, cost` triples of the
/// next cell towards the nearest goal (the walker's own cell at a goal,
/// outside the field or stuck) and the walker's cost to that goal (-1
/// outside). Returns how many walkers got a next cell.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_next(g: *mut WasmGrid, count: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Ok(count) = usize::try_from(count) else {
        return ERROR_ARGUMENT;
    };
    if !w.input_cells(count) {
        return ERROR_ARGUMENT;
    }
    let mut graph = w.topology.graph(&mut w.grid, None);
    w.out.clear();
    let mut moved = 0;
    for &cell in &w.cells {
        let cost = w.flow.cost(cell);
        let next = cost.and_then(|_| w.flow.next(&mut graph, cell));
        moved += i32::from(next.is_some());
        let n = next.unwrap_or(cell);
        w.out
            .extend_from_slice(&[n.x, n.y, cost.map_or(-1, to_i32)]);
    }
    moved
}

/// D* Lite: plans from `sx, sy` to `gx, gy`; nothing is searched yet.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_begin(
    g: *mut WasmGrid,
    sx: i32,
    sy: i32,
    gx: i32,
    gy: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    w.dstar
        .begin(Cell::new(sx, sy, 0), Cell::new(gx, gy, 0), w.topology);
    w.path.clear();
    0
}

/// D* Lite: works on the plan; when found writes the way from the walker to
/// the goal as `output`. The status is in [`sx_result`].
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_compute(g: *mut WasmGrid, budget: i32, output: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let (Some(budget), Some(output)) = (budget_of(budget), output_of(output)) else {
        return ERROR_ARGUMENT;
    };
    let status = {
        let mut graph = w.topology.graph(&mut w.grid, None);
        let status = match w.tracer.as_deref_mut() {
            None => w.dstar.compute(&mut graph, budget),
            Some(t) => w
                .dstar
                .compute_traced(&mut graph, budget, &mut TraceSink(t)),
        };
        if !(status == DStarStatus::Found && w.dstar.path(&mut graph, &mut w.path)) {
            w.path.clear();
        }
        status
    };
    let code = match status {
        DStarStatus::Idle => 0,
        DStarStatus::Running => RUNNING,
        DStarStatus::Found if w.path.is_empty() => UNREACHABLE,
        DStarStatus::Found => FOUND,
        DStarStatus::Unreachable => UNREACHABLE,
    };
    let cost = if code == FOUND {
        w.dstar.cost().map_or(0, to_i32)
    } else {
        0
    };
    w.result = [
        code,
        to_i32(w.path.len()),
        cost,
        to_i32(w.dstar.expanded()),
        i32::from(w.dstar.node_limit_hit()),
    ];
    let options = WayOptions {
        keep_first: false,
        smooth_max_extra: w.smooth_max_extra,
    };
    write_way(
        w.topology,
        &mut w.grid,
        &w.path,
        output,
        options,
        &mut w.scratch,
        &mut w.list,
    );
    WasmGrid::write_cells(&mut w.out, &w.list)
}

/// D* Lite: the walker now stands on `x, y`.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_move_start(g: *mut WasmGrid, x: i32, y: i32) -> i32 {
    grid!(g, ERROR_ARGUMENT)
        .dstar
        .move_start(Cell::new(x, y, 0));
    0
}

/// D* Lite: the `count` cells in the input buffer changed; the grid already
/// shows the new state. Returns how many known cells were updated.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_cells_changed(g: *mut WasmGrid, count: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Ok(count) = usize::try_from(count) else {
        return ERROR_ARGUMENT;
    };
    if !w.input_cells(count) {
        return ERROR_ARGUMENT;
    }
    let mut graph = w.topology.graph(&mut w.grid, None);
    to_i32(
        w.topology
            .cells_changed(&mut w.dstar, &mut graph, &w.cells, &mut w.scratch),
    )
}

/// Tracing on (`on` 1) or off (0) for every search of this grid. Events wait
/// in a buffer until [`sx_trace_take`]; off it costs nothing.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_trace(g: *mut WasmGrid, on: i32) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    if on != 0 {
        w.nav.set_tracer(Some(tracer_into(&w.trace)));
        w.tracer = Some(tracer_into(&w.trace));
    } else {
        w.nav.set_tracer(None);
        w.tracer = None;
    }
    0
}

/// Moves the waiting trace events to the output buffer (9 numbers each:
/// kind, x, y, parent x, parent y, cost, estimate, expanded, found) and
/// returns how many. Dropped events: [`sx_trace_dropped`].
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_trace_take(g: *mut WasmGrid) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let mut b = w.trace.borrow_mut();
    std::mem::swap(&mut w.out, &mut b.events);
    b.events.clear();
    to_i32(w.out.len() / TRACE_STRIDE)
}

/// Events dropped since the last call because the buffer was full; resets
/// the count.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_trace_dropped(g: *mut WasmGrid) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    to_i32(std::mem::take(&mut w.trace.borrow_mut().dropped))
}

// ---- decisions -------------------------------------------------------------

use sagax_core::CallbackSink;
use sagax_decide::{Apply, Candidate, Combine, Consideration, Curve, DecideEvent, Decider, Pick};

/// Numbers per decision trace event: kind (1 factor, 2 scored, 3 chosen),
/// candidate, consideration (-1 if none), input, value, score.
const DECIDE_STRIDE: usize = 6;

/// A decider with its buffers.
pub struct WasmDecider {
    decider: Decider,
    input: Vec<i32>,
    candidates: Vec<Candidate>,
    rows: Vec<i32>,
    scores: Vec<u32>,
    trace_on: bool,
    trace: Vec<i32>,
    out: Vec<i32>,
}

macro_rules! decider {
    ($d:expr, $err:expr) => {
        // SAFETY: JS passes a handle from sx_decider_new (wrapper guarantees it).
        match unsafe { $d.as_mut() } {
            Some(d) => d,
            None => return $err,
        }
    };
}

/// A decider without options.
#[unsafe(no_mangle)]
pub extern "C" fn sx_decider_new() -> *mut WasmDecider {
    Box::into_raw(Box::new(WasmDecider {
        decider: Decider::new(),
        input: Vec::new(),
        candidates: Vec::new(),
        rows: Vec::new(),
        scores: Vec::new(),
        trace_on: false,
        trace: Vec::new(),
        out: Vec::new(),
    }))
}

/// Frees a decider.
///
/// # Safety
/// `d` is null or from [`sx_decider_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_free(d: *mut WasmDecider) {
    if !d.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(d) });
    }
}

/// Adds an option (`weight` 65535 = 1.0) and returns its number.
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_add_option(d: *mut WasmDecider, weight: u32) -> i32 {
    to_i32(decider!(d, ERROR_ARGUMENT).decider.add_option(weight))
}

/// Adds a consideration to `option`: input index, curve 0..5, min, max.
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_add_consideration(
    d: *mut WasmDecider,
    option: u32,
    input: u32,
    curve: u32,
    min: i32,
    max: i32,
) -> i32 {
    let w = decider!(d, ERROR_ARGUMENT);
    let Some(curve) = Curve::from_u32(curve) else {
        return ERROR_ARGUMENT;
    };
    if w.decider
        .add_consideration(option, Consideration::new(input, curve, min, max))
    {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// Adds a consideration with `pull` (65535 as it is) and `apply` (0 into
/// the combine, 1 multiplied afterwards).
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_decider_add_weighted(
    d: *mut WasmDecider,
    option: u32,
    input: u32,
    curve: u32,
    min: i32,
    max: i32,
    pull: u32,
    apply: u32,
) -> i32 {
    let w = decider!(d, ERROR_ARGUMENT);
    let (Some(curve), Some(apply)) = (Curve::from_u32(curve), Apply::from_u32(apply)) else {
        return ERROR_ARGUMENT;
    };
    let c = Consideration::new(input, curve, min, max);
    if w.decider.add_weighted(option, c, pull, apply) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// Combine 0..4 for every option (`option` -1) or one option (`combine` -1:
/// follow the decider).
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_combine(
    d: *mut WasmDecider,
    option: i32,
    combine: i32,
) -> i32 {
    let w = decider!(d, ERROR_ARGUMENT);
    let how = u32::try_from(combine).ok().and_then(Combine::from_u32);
    match (u32::try_from(option), how) {
        (Err(_), Some(how)) => {
            w.decider.set_combine(how);
            0
        }
        (Ok(option), how)
            if (how.is_some() || combine == -1) && w.decider.set_option_combine(option, how) =>
        {
            0
        }
        _ => ERROR_ARGUMENT,
    }
}

/// Rank of an option (the highest rank that still scores is chosen from).
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_rank(d: *mut WasmDecider, option: u32, rank: u32) -> i32 {
    if decider!(d, ERROR_ARGUMENT).decider.set_rank(option, rank) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// The action going on now (`has` 0: none) gets `bonus` (65535 = +100 %).
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_current(
    d: *mut WasmDecider,
    has: i32,
    option: u32,
    target: u32,
    bonus: u32,
) -> i32 {
    let current = (has != 0).then(|| Candidate::new(option, target));
    decider!(d, ERROR_ARGUMENT)
        .decider
        .set_current(current, bonus);
    0
}

/// The time for the next decisions.
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_now(d: *mut WasmDecider, now: u32) -> i32 {
    decider!(d, ERROR_ARGUMENT).decider.set_now(now);
    0
}

/// Remembers that option, target was done at `now`.
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_record(
    d: *mut WasmDecider,
    option: u32,
    target: u32,
    now: u32,
) -> i32 {
    decider!(d, ERROR_ARGUMENT)
        .decider
        .record(Candidate::new(option, target), now);
    0
}

/// Forgets every recorded action.
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_forget(d: *mut WasmDecider) -> i32 {
    decider!(d, ERROR_ARGUMENT).decider.forget();
    0
}

/// Window of the count inputs (0: all remembered).
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_window(d: *mut WasmDecider, time: u32) -> i32 {
    decider!(d, ERROR_ARGUMENT).decider.set_window(time);
    0
}

/// Hard cooldown of an option (0: none).
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_cooldown(
    d: *mut WasmDecider,
    option: u32,
    time: u32,
) -> i32 {
    if decider!(d, ERROR_ARGUMENT)
        .decider
        .set_cooldown(option, time)
    {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// The input buffer, resized to `count` `i32`s: per candidate option,
/// target and `stride` raw inputs.
///
/// # Safety
/// `d` from [`sx_decider_new`]. The pointer is valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_input(d: *mut WasmDecider, count: i32) -> *mut i32 {
    let w = decider!(d, std::ptr::null_mut());
    w.input.clear();
    w.input.resize(usize::try_from(count).unwrap_or(0), 0);
    w.input.as_mut_ptr()
}

/// The output buffer: the scores of the last decision (`Uint32Array`, one
/// per candidate) or the trace events after [`sx_decider_trace_take`].
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_out(d: *mut WasmDecider) -> *const i32 {
    decider!(d, std::ptr::null()).out.as_ptr()
}

/// Scores the `count` candidates in the input buffer and chooses one
/// (`pick` 0 best, 1 weighted, 2 uniform among `top` best, 0 all; seed in
/// two halves). Returns the chosen index, -2 if every score is 0, -1 on bad
/// arguments. The scores go to the output buffer.
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_decider_decide(
    d: *mut WasmDecider,
    count: i32,
    stride: i32,
    pick: u32,
    top: u32,
    seed_low: u32,
    seed_high: u32,
) -> i32 {
    let w = decider!(d, ERROR_ARGUMENT);
    let (Ok(count), Ok(stride), Some(pick)) = (
        usize::try_from(count),
        usize::try_from(stride),
        Pick::from_u32(pick, top),
    ) else {
        return ERROR_ARGUMENT;
    };
    let row = stride + 2;
    if count.checked_mul(row) != Some(w.input.len()) {
        return ERROR_ARGUMENT;
    }
    w.candidates.clear();
    w.rows.clear();
    for c in w.input.chunks_exact(row) {
        let (Ok(option), Ok(target)) = (u32::try_from(c[0]), u32::try_from(c[1])) else {
            return ERROR_ARGUMENT;
        };
        w.candidates.push(Candidate::new(option, target));
        w.rows.extend_from_slice(&c[2..]);
    }
    let seed = u64::from(seed_low) | (u64::from(seed_high) << 32);
    let result = if w.trace_on {
        let trace = &mut w.trace;
        w.decider.decide_traced(
            &w.candidates,
            &w.rows,
            stride,
            pick,
            seed,
            &mut w.scores,
            &mut CallbackSink(|e| {
                if trace.len() >= TRACE_LIMIT * DECIDE_STRIDE {
                    return;
                }
                trace.extend_from_slice(&match e {
                    DecideEvent::Factor {
                        candidate,
                        consideration,
                        input,
                        value,
                    } => [
                        1,
                        to_i32(candidate),
                        to_i32(consideration),
                        input,
                        value as i32,
                        0,
                    ],
                    DecideEvent::Scored {
                        candidate,
                        product,
                        score,
                    } => [2, to_i32(candidate), -1, 0, product as i32, score as i32],
                    DecideEvent::Chosen { candidate, score } => {
                        [3, to_i32(candidate), -1, 0, 0, score as i32]
                    }
                });
            }),
        )
    } else {
        w.decider
            .decide(&w.candidates, &w.rows, stride, pick, seed, &mut w.scores)
    };
    w.out.clear();
    // Scores are u32; JS reads them through a Uint32Array.
    w.out.extend(w.scores.iter().map(|&s| s as i32));
    match result {
        Ok(Some(c)) => to_i32(c.candidate),
        Ok(None) => -2,
        Err(_) => ERROR_ARGUMENT,
    }
}

/// Tracing of decisions on (1) or off (0).
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_trace(d: *mut WasmDecider, on: i32) -> i32 {
    let w = decider!(d, ERROR_ARGUMENT);
    w.trace_on = on != 0;
    if !w.trace_on {
        w.trace.clear();
    }
    0
}

/// Moves the waiting decision events to the output buffer (6 numbers each,
/// see [`DECIDE_STRIDE`]) and returns how many.
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_trace_take(d: *mut WasmDecider) -> i32 {
    let w = decider!(d, ERROR_ARGUMENT);
    std::mem::swap(&mut w.out, &mut w.trace);
    w.trace.clear();
    to_i32(w.out.len() / DECIDE_STRIDE)
}

// ---- state machine ---------------------------------------------------------

use sagax_decide::state::{ANY, Reason, StateMachine, Transition};

/// A state machine and its last change.
pub struct WasmMachine {
    machine: StateMachine,
    /// from, to, reason (0 none, 1 transition, 2 timeout, 3 set), transition index
    change: [i32; 4],
}

macro_rules! machine {
    ($m:expr) => {
        // SAFETY: JS passes a handle from sx_machine_new (wrapper guarantees it).
        match unsafe { $m.as_mut() } {
            Some(m) => m,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// A machine with `states` states starting in `initial`; null if invalid.
#[unsafe(no_mangle)]
pub extern "C" fn sx_machine_new(states: u32, initial: u32) -> *mut WasmMachine {
    match StateMachine::new(states, initial) {
        Ok(machine) => Box::into_raw(Box::new(WasmMachine {
            machine,
            change: [0; 4],
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees a machine.
///
/// # Safety
/// `m` is null or from [`sx_machine_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_free(m: *mut WasmMachine) {
    if !m.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(m) });
    }
}

/// After `time` in `state`, go to `next`.
///
/// # Safety
/// `m` from [`sx_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_set_timeout(
    m: *mut WasmMachine,
    state: u32,
    time: u32,
    next: u32,
) -> i32 {
    match machine!(m).machine.set_timeout(state, time, next) {
        Ok(()) => 0,
        Err(_) => ERROR_ARGUMENT,
    }
}

/// A transition (`from` -1: any); condition masks in 32-bit halves.
///
/// # Safety
/// `m` from [`sx_machine_new`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_machine_add_transition(
    m: *mut WasmMachine,
    from: i32,
    to: u32,
    when_low: u32,
    when_high: u32,
    unless_low: u32,
    unless_high: u32,
    after: u32,
) -> i32 {
    let w = machine!(m);
    let from = if from < 0 { ANY } else { from as u32 };
    let when = u64::from(when_low) | (u64::from(when_high) << 32);
    let unless = u64::from(unless_low) | (u64::from(unless_high) << 32);
    match w
        .machine
        .add_transition(Transition::new(from, to, when).unless(unless).after(after))
    {
        Ok(i) => to_i32(i),
        Err(_) => ERROR_ARGUMENT,
    }
}

/// One tick. Returns 1 if the state changed (see [`sx_machine_change`]).
///
/// # Safety
/// `m` from [`sx_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_update(
    m: *mut WasmMachine,
    now: u32,
    flags_low: u32,
    flags_high: u32,
) -> i32 {
    let w = machine!(m);
    let flags = u64::from(flags_low) | (u64::from(flags_high) << 32);
    let state = to_i32(w.machine.state());
    w.change = match w.machine.update(now, flags) {
        None => [state, state, 0, -1],
        Some(c) => {
            let (reason, index) = match c.reason {
                Reason::Transition(i) => (1, to_i32(i)),
                Reason::Timeout => (2, -1),
                Reason::Set => (3, -1),
            };
            [to_i32(c.from), to_i32(c.to), reason, index]
        }
    };
    i32::from(w.change[2] != 0)
}

/// The last change: from, to, reason, transition index (4 `i32`s).
///
/// # Safety
/// `m` from [`sx_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_change(m: *mut WasmMachine) -> *const i32 {
    // SAFETY: see above.
    match unsafe { m.as_ref() } {
        Some(w) => w.change.as_ptr(),
        None => std::ptr::null(),
    }
}

/// The current state.
///
/// # Safety
/// `m` from [`sx_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_state(m: *mut WasmMachine) -> i32 {
    to_i32(machine!(m).machine.state())
}

/// When the current state was entered (as `u32`).
///
/// # Safety
/// `m` from [`sx_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_entered(m: *mut WasmMachine) -> u32 {
    // SAFETY: see above.
    unsafe { m.as_ref() }.map_or(0, |w| w.machine.entered())
}

/// Puts the machine into `state` at `now`.
///
/// # Safety
/// `m` from [`sx_machine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_set_state(m: *mut WasmMachine, state: u32, now: u32) -> i32 {
    match machine!(m).machine.set_state(state, now) {
        Ok(_) => 0,
        Err(_) => ERROR_ARGUMENT,
    }
}

// ---- world memory ----------------------------------------------------------
//
// Same blocks as the C interface (`sx_knowledge_*`, `sx_outbox_*`, ...), but
// without structs: every handle has a record buffer (`u32`s; signed values
// as their bit pattern) that JS fills before and reads after a call, and
// the tables that hand out several records have an out buffer too.

use sagax_decide::memory::{
    Area, Areas, Evict, Fact, Inbox, Knowledge, LOOK_OFFSETS, Message, Metric, Outbox,
    PointPattern, Refused, Route, Sine, Timers, points_around, route, sine, sweep_heading,
};

/// Numbers per fact record: subject, kind, x, y, z, seen at, strength, data,
/// source.
const FACT_SIZE: usize = 9;
/// Numbers per message record: kind, subject, x, y, z, seen at, strength,
/// data, sender, keep if sender gone.
const MESSAGE_SIZE: usize = 10;
/// Numbers per area record: kind, x, y, z, radius, strength, since, lifetime,
/// data, source x, y, z, blocking, has source.
const AREA_SIZE: usize = 14;

fn fact_record(f: &Fact) -> [u32; FACT_SIZE] {
    [
        f.subject,
        f.kind,
        f.pos[0] as u32,
        f.pos[1] as u32,
        f.pos[2] as u32,
        f.seen_at,
        f.strength,
        f.data,
        u32::from(f.source),
    ]
}

fn fact_of(r: &[u32; FACT_SIZE]) -> Fact {
    Fact {
        subject: r[0],
        kind: r[1],
        pos: [r[2] as i32, r[3] as i32, r[4] as i32],
        seen_at: r[5],
        strength: r[6],
        data: r[7],
        source: r[8] as u8,
    }
}

fn message_record(m: &Message) -> [u32; MESSAGE_SIZE] {
    [
        m.kind,
        m.subject,
        m.pos[0] as u32,
        m.pos[1] as u32,
        m.pos[2] as u32,
        m.seen_at,
        m.strength,
        m.data,
        m.sender,
        u32::from(m.keep_if_sender_gone),
    ]
}

fn message_of(r: &[u32; MESSAGE_SIZE]) -> Message {
    Message {
        kind: r[0],
        subject: r[1],
        pos: [r[2] as i32, r[3] as i32, r[4] as i32],
        seen_at: r[5],
        strength: r[6],
        data: r[7],
        sender: r[8],
        keep_if_sender_gone: r[9] as u8,
    }
}

fn area_record(a: &Area) -> [u32; AREA_SIZE] {
    [
        a.kind,
        a.pos[0] as u32,
        a.pos[1] as u32,
        a.pos[2] as u32,
        a.radius as u32,
        a.strength,
        a.since,
        a.lifetime,
        a.data,
        a.source[0] as u32,
        a.source[1] as u32,
        a.source[2] as u32,
        u32::from(a.blocking),
        u32::from(a.has_source),
    ]
}

fn area_of(r: &[u32; AREA_SIZE]) -> Area {
    Area {
        kind: r[0],
        pos: [r[1] as i32, r[2] as i32, r[3] as i32],
        radius: r[4] as i32,
        strength: r[5],
        since: r[6],
        lifetime: r[7],
        data: r[8],
        source: [r[9] as i32, r[10] as i32, r[11] as i32],
        blocking: r[12] as u8,
        has_source: r[13] as u8,
    }
}

/// Knowledge with a fact record (in and out) and the facts handed out by the
/// last call that removed or evicted some.
pub struct WasmKnowledge {
    knowledge: Knowledge,
    record: [u32; FACT_SIZE],
    out: Vec<u32>,
}

macro_rules! knowledge {
    ($k:expr) => {
        // SAFETY: JS passes a handle from sx_knowledge_new (wrapper guarantees it).
        match unsafe { $k.as_mut() } {
            Some(k) => k,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// Knowledge for at most `capacity` facts; `evict` 0 drops the oldest when
/// full, 1 the weakest. Null if `capacity` is 0 or `evict` unknown.
#[unsafe(no_mangle)]
pub extern "C" fn sx_knowledge_new(capacity: u32, evict: u32) -> *mut WasmKnowledge {
    let evict = match evict {
        0 => Evict::Oldest,
        1 => Evict::Weakest,
        _ => return std::ptr::null_mut(),
    };
    match Knowledge::new(capacity as usize, evict) {
        Ok(knowledge) => Box::into_raw(Box::new(WasmKnowledge {
            knowledge,
            record: [0; FACT_SIZE],
            out: Vec::new(),
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees knowledge.
///
/// # Safety
/// `k` is null or from [`sx_knowledge_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_free(k: *mut WasmKnowledge) {
    if !k.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(k) });
    }
}

/// The record buffer (9 `u32`): the fact JS gives to `learn` and `merge`, and
/// the fact `get` and `at` load.
///
/// # Safety
/// `k` from [`sx_knowledge_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_record(k: *mut WasmKnowledge) -> *mut u32 {
    // SAFETY: see above.
    unsafe { k.as_mut() }.map_or(std::ptr::null_mut(), |w| w.record.as_mut_ptr())
}

/// The out buffer: the evicted fact or the removed facts (9 `u32` each).
///
/// # Safety
/// `k` from [`sx_knowledge_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_out(k: *mut WasmKnowledge) -> *const u32 {
    // SAFETY: see above.
    unsafe { k.as_ref() }.map_or(std::ptr::null(), |w| w.out.as_ptr())
}

fn evicted_out(w: &mut WasmKnowledge, evicted: Option<Fact>) -> i32 {
    w.out.clear();
    match evicted {
        Some(f) => {
            w.out.extend_from_slice(&fact_record(&f));
            1
        }
        None => 0,
    }
}

/// Learns the fact in the record buffer (overwrites a known one). Returns 1
/// if another fact was evicted (it is in the out buffer), else 0.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_learn(k: *mut WasmKnowledge, now: u32) -> i32 {
    let w = knowledge!(k);
    let evicted = w.knowledge.learn(fact_of(&w.record), now);
    evicted_out(w, evicted)
}

/// Merges the fact in the record buffer (a known fact is only overwritten by
/// a not older one). Returns 1 if another fact was evicted, else 0.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_merge(k: *mut WasmKnowledge, now: u32) -> i32 {
    let w = knowledge!(k);
    let evicted = w.knowledge.merge(fact_of(&w.record), now);
    evicted_out(w, evicted)
}

/// Renews the time of a fact; 1 if known.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_touch(
    k: *mut WasmKnowledge,
    subject: u32,
    kind: u32,
    now: u32,
) -> i32 {
    i32::from(knowledge!(k).knowledge.touch(subject, kind, now))
}

/// Sets the place of a fact; 1 if known.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_set_pos(
    k: *mut WasmKnowledge,
    subject: u32,
    kind: u32,
    x: i32,
    y: i32,
    z: i32,
) -> i32 {
    i32::from(knowledge!(k).knowledge.set_pos(subject, kind, [x, y, z]))
}

/// Sets the data of a fact; 1 if known.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_set_data(
    k: *mut WasmKnowledge,
    subject: u32,
    kind: u32,
    data: u32,
) -> i32 {
    i32::from(knowledge!(k).knowledge.set_data(subject, kind, data))
}

/// Sets the strength of a fact; 1 if known.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_set_strength(
    k: *mut WasmKnowledge,
    subject: u32,
    kind: u32,
    strength: u32,
) -> i32 {
    i32::from(
        knowledge!(k)
            .knowledge
            .set_strength(subject, kind, strength),
    )
}

/// Forgets one fact; 1 if known.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_remove(
    k: *mut WasmKnowledge,
    subject: u32,
    kind: u32,
) -> i32 {
    i32::from(knowledge!(k).knowledge.remove(subject, kind))
}

/// Forgets all facts about `subject`; returns how many.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_remove_subject(k: *mut WasmKnowledge, subject: u32) -> i32 {
    to_i32(knowledge!(k).knowledge.remove_subject(subject))
}

/// Loads the fact (subject, kind) into the record buffer; 1 if known.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_get(k: *mut WasmKnowledge, subject: u32, kind: u32) -> i32 {
    let w = knowledge!(k);
    match w.knowledge.get(subject, kind) {
        Some(f) => {
            w.record = fact_record(f);
            1
        }
        None => 0,
    }
}

/// Loads the fact in slot `index` into the record buffer; 1 if present.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_at(k: *mut WasmKnowledge, index: u32) -> i32 {
    let w = knowledge!(k);
    match w.knowledge.iter().nth(index as usize) {
        Some(f) => {
            w.record = fact_record(f);
            1
        }
        None => 0,
    }
}

/// Number of facts.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_count(k: *mut WasmKnowledge) -> i32 {
    to_i32(knowledge!(k).knowledge.len())
}

/// Forgets everything.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_clear(k: *mut WasmKnowledge) -> i32 {
    knowledge!(k).knowledge.clear();
    0
}

/// Forgets facts of age at least `max_age`; returns how many. The facts are
/// in the out buffer, in slot order.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_expire(k: *mut WasmKnowledge, now: u32, max_age: u32) -> i32 {
    let w = knowledge!(k);
    w.out.clear();
    let out = &mut w.out;
    w.knowledge
        .expire(now, max_age, |f| out.extend_from_slice(&fact_record(&f)));
    to_i32(w.out.len() / FACT_SIZE)
}

/// Like [`sx_knowledge_expire`], for facts of one `kind` only.
///
/// # Safety
/// `k` from [`sx_knowledge_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_expire_kind(
    k: *mut WasmKnowledge,
    now: u32,
    kind: u32,
    max_age: u32,
) -> i32 {
    let w = knowledge!(k);
    w.out.clear();
    let out = &mut w.out;
    w.knowledge.expire_kind(now, kind, max_age, |f| {
        out.extend_from_slice(&fact_record(&f));
    });
    to_i32(w.out.len() / FACT_SIZE)
}

/// Messages waiting to be sent, with a message record (in) and an out buffer
/// for the messages that are due.
pub struct WasmOutbox {
    outbox: Outbox,
    record: [u32; MESSAGE_SIZE],
    out: Vec<u32>,
}

macro_rules! outbox {
    ($o:expr) => {
        // SAFETY: JS passes a handle from sx_outbox_new (wrapper guarantees it).
        match unsafe { $o.as_mut() } {
            Some(o) => o,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// An outbox for at most `capacity` messages; null if `capacity` is 0.
#[unsafe(no_mangle)]
pub extern "C" fn sx_outbox_new(capacity: u32) -> *mut WasmOutbox {
    match Outbox::new(capacity as usize) {
        Ok(outbox) => Box::into_raw(Box::new(WasmOutbox {
            outbox,
            record: [0; MESSAGE_SIZE],
            out: Vec::new(),
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees an outbox.
///
/// # Safety
/// `o` is null or from [`sx_outbox_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_free(o: *mut WasmOutbox) {
    if !o.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(o) });
    }
}

/// The record buffer (10 `u32`): the message JS gives to `post`.
///
/// # Safety
/// `o` from [`sx_outbox_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_record(o: *mut WasmOutbox) -> *mut u32 {
    // SAFETY: see above.
    unsafe { o.as_mut() }.map_or(std::ptr::null_mut(), |w| w.record.as_mut_ptr())
}

/// The out buffer: the messages `due` handed out (10 `u32` each).
///
/// # Safety
/// `o` from [`sx_outbox_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_out(o: *mut WasmOutbox) -> *const u32 {
    // SAFETY: see above.
    unsafe { o.as_ref() }.map_or(std::ptr::null(), |w| w.out.as_ptr())
}

/// Posts the message in the record buffer: 0 accepted, 1 refused because the
/// outbox is full, 2 refused by the cooldown.
///
/// # Safety
/// `o` from [`sx_outbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_post(
    o: *mut WasmOutbox,
    now: u32,
    delay: u32,
    cooldown: u32,
) -> i32 {
    let w = outbox!(o);
    match w.outbox.post(message_of(&w.record), now, delay, cooldown) {
        Ok(()) => 0,
        Err(Refused::Full) => 1,
        Err(Refused::Cooldown) => 2,
    }
}

/// Moves up to `max` due messages to the out buffer, in post order; returns
/// how many.
///
/// # Safety
/// `o` from [`sx_outbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_due(o: *mut WasmOutbox, now: u32, max: u32) -> i32 {
    let w = outbox!(o);
    let mut list = Vec::new();
    w.outbox.due_max(now, max as usize, &mut list);
    w.out.clear();
    for m in &list {
        w.out.extend_from_slice(&message_record(m));
    }
    to_i32(list.len())
}

/// Drops the messages that are not marked to be kept.
///
/// # Safety
/// `o` from [`sx_outbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_sender_gone(o: *mut WasmOutbox) -> i32 {
    outbox!(o).outbox.sender_gone();
    0
}

/// Number of messages waiting.
///
/// # Safety
/// `o` from [`sx_outbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_pending(o: *mut WasmOutbox) -> i32 {
    to_i32(outbox!(o).outbox.pending())
}

/// Drops all messages (the cooldown keeps running).
///
/// # Safety
/// `o` from [`sx_outbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_clear(o: *mut WasmOutbox) -> i32 {
    outbox!(o).outbox.clear();
    0
}

/// Messages on their way to one receiver, with a message record (in) and an
/// out buffer for the messages that arrived.
pub struct WasmInbox {
    inbox: Inbox,
    record: [u32; MESSAGE_SIZE],
    out: Vec<u32>,
}

macro_rules! inbox {
    ($i:expr) => {
        // SAFETY: JS passes a handle from sx_inbox_new (wrapper guarantees it).
        match unsafe { $i.as_mut() } {
            Some(i) => i,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// An inbox for at most `capacity` messages; null if `capacity` is 0.
#[unsafe(no_mangle)]
pub extern "C" fn sx_inbox_new(capacity: u32) -> *mut WasmInbox {
    match Inbox::new(capacity as usize) {
        Ok(inbox) => Box::into_raw(Box::new(WasmInbox {
            inbox,
            record: [0; MESSAGE_SIZE],
            out: Vec::new(),
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees an inbox.
///
/// # Safety
/// `i` is null or from [`sx_inbox_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_free(i: *mut WasmInbox) {
    if !i.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(i) });
    }
}

/// The record buffer (10 `u32`): the message JS gives to `push`.
///
/// # Safety
/// `i` from [`sx_inbox_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_record(i: *mut WasmInbox) -> *mut u32 {
    // SAFETY: see above.
    unsafe { i.as_mut() }.map_or(std::ptr::null_mut(), |w| w.record.as_mut_ptr())
}

/// The out buffer: the messages `take_due` handed out (10 `u32` each).
///
/// # Safety
/// `i` from [`sx_inbox_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_out(i: *mut WasmInbox) -> *const u32 {
    // SAFETY: see above.
    unsafe { i.as_ref() }.map_or(std::ptr::null(), |w| w.out.as_ptr())
}

/// Puts the message in the record buffer on its way, arriving at
/// `arrive_at`; 1 if kept, 0 if refused.
///
/// # Safety
/// `i` from [`sx_inbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_push(i: *mut WasmInbox, arrive_at: u32) -> i32 {
    let w = inbox!(i);
    i32::from(w.inbox.push(message_of(&w.record), arrive_at))
}

/// Moves up to `max` arrived messages to the out buffer, in push order;
/// returns how many.
///
/// # Safety
/// `i` from [`sx_inbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_take_due(i: *mut WasmInbox, now: u32, max: u32) -> i32 {
    let w = inbox!(i);
    let mut list = Vec::new();
    w.inbox.take_due_max(now, max as usize, &mut list);
    w.out.clear();
    for m in &list {
        w.out.extend_from_slice(&message_record(m));
    }
    to_i32(list.len())
}

/// Number of messages on their way.
///
/// # Safety
/// `i` from [`sx_inbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_count(i: *mut WasmInbox) -> i32 {
    to_i32(inbox!(i).inbox.len())
}

/// Drops all messages.
///
/// # Safety
/// `i` from [`sx_inbox_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_clear(i: *mut WasmInbox) -> i32 {
    inbox!(i).inbox.clear();
    0
}

/// Areas with an area record (in and out).
pub struct WasmAreas {
    areas: Areas,
    record: [u32; AREA_SIZE],
}

macro_rules! areas {
    ($a:expr) => {
        // SAFETY: JS passes a handle from sx_areas_new (wrapper guarantees it).
        match unsafe { $a.as_mut() } {
            Some(a) => a,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// Areas for at most `capacity`; null if `capacity` is 0.
#[unsafe(no_mangle)]
pub extern "C" fn sx_areas_new(capacity: u32) -> *mut WasmAreas {
    match Areas::new(capacity as usize) {
        Ok(areas) => Box::into_raw(Box::new(WasmAreas {
            areas,
            record: [0; AREA_SIZE],
        })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees areas.
///
/// # Safety
/// `a` is null or from [`sx_areas_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_free(a: *mut WasmAreas) {
    if !a.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(a) });
    }
}

/// The record buffer (14 `u32`): the area JS gives to `add`, and the area
/// `at` loads.
///
/// # Safety
/// `a` from [`sx_areas_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_record(a: *mut WasmAreas) -> *mut u32 {
    // SAFETY: see above.
    unsafe { a.as_mut() }.map_or(std::ptr::null_mut(), |w| w.record.as_mut_ptr())
}

/// Adds the area in the record buffer (or renews the one it falls in);
/// returns its index.
///
/// # Safety
/// `a` from [`sx_areas_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_add(a: *mut WasmAreas, now: u32) -> i32 {
    let w = areas!(a);
    to_i32(w.areas.add(area_of(&w.record), now))
}

/// Removes expired areas; returns how many.
///
/// # Safety
/// `a` from [`sx_areas_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_expire(a: *mut WasmAreas, now: u32) -> i32 {
    to_i32(areas!(a).areas.expire(now))
}

/// Index of the first area containing the place, or -2 if none.
///
/// # Safety
/// `a` from [`sx_areas_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_contains(a: *mut WasmAreas, x: i32, y: i32, z: i32) -> i32 {
    areas!(a).areas.contains([x, y, z]).map_or(-2, to_i32)
}

/// Like [`sx_areas_contains`], for areas of one kind.
///
/// # Safety
/// `a` from [`sx_areas_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_contains_kind(
    a: *mut WasmAreas,
    x: i32,
    y: i32,
    z: i32,
    kind: u32,
) -> i32 {
    areas!(a)
        .areas
        .contains_kind([x, y, z], kind)
        .map_or(-2, to_i32)
}

/// 1 if a blocking area holds `cell` but neither `start` nor `goal`; reads 9
/// integers (cell, start, goal).
///
/// # Safety
/// `a` from [`sx_areas_new`]; `cells` null or points to 9 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_blocks(a: *mut WasmAreas, cells: *const i32) -> i32 {
    let w = areas!(a);
    if cells.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let p = unsafe { std::slice::from_raw_parts(cells, 9) };
    i32::from(
        w.areas
            .blocks([p[0], p[1], p[2]], [p[3], p[4], p[5]], [p[6], p[7], p[8]]),
    )
}

/// The extra cost of `cell` (sum of the strengths of non-blocking areas
/// holding it but neither `start` nor `goal`, saturating) as a `u32`; reads
/// 9 integers (cell, start, goal). A null handle or `cells` returns 0.
///
/// # Safety
/// `a` from [`sx_areas_new`]; `cells` null or points to 9 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_cost(a: *mut WasmAreas, cells: *const i32) -> u32 {
    // SAFETY: see above.
    let (Some(w), false) = (unsafe { a.as_ref() }, cells.is_null()) else {
        return 0;
    };
    // SAFETY: see above.
    let p = unsafe { std::slice::from_raw_parts(cells, 9) };
    w.areas
        .cost([p[0], p[1], p[2]], [p[3], p[4], p[5]], [p[6], p[7], p[8]])
}

/// Loads the area in slot `index` into the record buffer; 1 if present.
///
/// # Safety
/// `a` from [`sx_areas_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_at(a: *mut WasmAreas, index: u32) -> i32 {
    let w = areas!(a);
    match w.areas.iter().nth(index as usize) {
        Some(f) => {
            w.record = area_record(f);
            1
        }
        None => 0,
    }
}

/// Number of areas.
///
/// # Safety
/// `a` from [`sx_areas_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_count(a: *mut WasmAreas) -> i32 {
    to_i32(areas!(a).areas.len())
}

/// Removes all areas.
///
/// # Safety
/// `a` from [`sx_areas_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_clear(a: *mut WasmAreas) -> i32 {
    areas!(a).areas.clear();
    0
}

/// Ids that count as "on" for a while.
pub struct WasmTimers(Timers);

macro_rules! timers {
    ($t:expr) => {
        // SAFETY: JS passes a handle from sx_timers_new (wrapper guarantees it).
        match unsafe { $t.as_mut() } {
            Some(t) => &mut t.0,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// Timers for at most `capacity` ids; null if `capacity` is 0.
#[unsafe(no_mangle)]
pub extern "C" fn sx_timers_new(capacity: u32) -> *mut WasmTimers {
    match Timers::new(capacity as usize) {
        Ok(t) => Box::into_raw(Box::new(WasmTimers(t))),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees timers.
///
/// # Safety
/// `t` is null or from [`sx_timers_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_free(t: *mut WasmTimers) {
    if !t.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(t) });
    }
}

/// Starts (or renews) the timer of `id`.
///
/// # Safety
/// `t` from [`sx_timers_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_set(
    t: *mut WasmTimers,
    id: u32,
    now: u32,
    duration: u32,
) -> i32 {
    timers!(t).set(id, now, duration);
    0
}

/// 1 if the timer of `id` still runs.
///
/// # Safety
/// `t` from [`sx_timers_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_active(t: *mut WasmTimers, id: u32, now: u32) -> i32 {
    i32::from(timers!(t).active(id, now))
}

/// Time left of the timer of `id` as a `u32`; a null handle returns 0.
///
/// # Safety
/// `t` from [`sx_timers_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_left(t: *mut WasmTimers, id: u32, now: u32) -> u32 {
    // SAFETY: see above.
    unsafe { t.as_ref() }.map_or(0, |w| w.0.left(id, now))
}

/// Stops the timer of `id`; 1 if there was one.
///
/// # Safety
/// `t` from [`sx_timers_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_remove(t: *mut WasmTimers, id: u32) -> i32 {
    i32::from(timers!(t).remove(id))
}

/// Stops all timers.
///
/// # Safety
/// `t` from [`sx_timers_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_clear(t: *mut WasmTimers) -> i32 {
    timers!(t).clear();
    0
}

/// Number of timers.
///
/// # Safety
/// `t` from [`sx_timers_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_count(t: *mut WasmTimers) -> i32 {
    to_i32(timers!(t).len())
}

thread_local! {
    static SCRATCH: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
}

/// A shared buffer of `count` integers for the helper functions below
/// ([`sx_route`], [`sx_points_around`], [`sx_sweep_heading`]). Valid until
/// memory grows or the buffer is asked for again.
#[unsafe(no_mangle)]
pub extern "C" fn sx_scratch(count: i32) -> *mut i32 {
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        s.clear();
        s.resize(usize::try_from(count).unwrap_or(0).max(1), 0);
        s.as_mut_ptr()
    })
}

fn sine_of(mode: i32) -> Option<Sine> {
    match mode {
        0 => Some(Sine::Fine),
        1 => Some(Sine::Coarse),
        _ => None,
    }
}

/// The sine of a heading (256 = full turn): `mode` 0 fine (times 65536), 1
/// coarse (times 256). Returns `i32::MIN` for an unknown mode.
#[unsafe(no_mangle)]
pub extern "C" fn sx_sine(heading: i32, mode: i32) -> i32 {
    sine_of(mode).map_or(i32::MIN, |s| sine(heading, s))
}

/// Picks recipients. Scratch in: `n` triples (id, x, y); scratch out: the
/// ids, in the order of the rule. `rule` 0 nearest `arg` (a count), 1 in
/// range `arg` (a distance, an integer in an `f64`), 2 all, 3 to the id
/// `arg`; `metric` 0 euclid, 1 manhattan, 2 chebyshev, 3 hex. Returns how
/// many ids, -1 on bad arguments.
#[unsafe(no_mangle)]
pub extern "C" fn sx_route(
    rule: i32,
    arg: f64,
    n: u32,
    ox: i32,
    oy: i32,
    metric: i32,
    divisor: i32,
) -> i32 {
    let rule = match rule {
        0 if arg >= 0.0 => Route::Nearest(arg as usize),
        1 => Route::InRange(arg as i64),
        2 => Route::All,
        3 if arg >= 0.0 => Route::To(arg as u32),
        _ => return ERROR_ARGUMENT,
    };
    let metric = match metric {
        0 => Metric::Euclid,
        1 => Metric::Manhattan,
        2 => Metric::Chebyshev,
        3 => Metric::Hex,
        _ => return ERROR_ARGUMENT,
    };
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        let Some(len) = (n as usize).checked_mul(3).filter(|&len| len <= s.len()) else {
            return ERROR_ARGUMENT;
        };
        let list: Vec<(u32, i32, i32)> = s[..len]
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| (c[0] as u32, c[1], c[2]))
            .collect();
        let mut ids = Vec::new();
        route(rule, (ox, oy), &list, metric, divisor, &mut ids);
        let count = ids.len();
        for (i, id) in ids.into_iter().enumerate() {
            s[i] = id as i32;
        }
        to_i32(count)
    })
}

/// Number of integers a point pattern takes in the scratch buffer.
const PATTERN_SIZE: usize = 23;

/// A point around a place. With `custom` not 0 the scratch buffer holds the
/// pattern (heading step, first distance, count at most 8, 8 offsets, 8
/// distances, min x, min y, max x, max y); otherwise the default pattern is
/// used. `sine` 0 fine, 1 coarse. Returns 1 and puts x, y, z in the scratch
/// buffer (at least 3 integers long), 0 past the last step, -1 on bad
/// arguments.
#[unsafe(no_mangle)]
pub extern "C" fn sx_points_around(
    agent: u32,
    step: u32,
    cx: i32,
    cy: i32,
    cz: i32,
    custom: i32,
    sine: i32,
) -> i32 {
    let Some(sine) = sine_of(sine) else {
        return ERROR_ARGUMENT;
    };
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        let pattern = if custom != 0 {
            if s.len() < PATTERN_SIZE || !(0..=8).contains(&s[2]) {
                return ERROR_ARGUMENT;
            }
            let n = s[2] as usize;
            PointPattern {
                heading_step: s[0],
                first_distance: s[1],
                offsets: s[3..3 + n].to_vec(),
                distances: s[11..11 + n].to_vec(),
                sine,
                min: (s[19], s[20]),
                max: (s[21], s[22]),
            }
        } else {
            PointPattern {
                sine,
                ..PointPattern::default()
            }
        };
        if s.len() < 3 {
            return ERROR_ARGUMENT;
        }
        match points_around(agent, step as usize, [cx, cy, cz], &pattern) {
            Some(p) => {
                s[..3].copy_from_slice(&p);
                1
            }
            None => 0,
        }
    })
}

/// The heading while looking around. With `count` below 0 the default
/// offsets are used, otherwise `count` offsets are read from the scratch
/// buffer.
#[unsafe(no_mangle)]
pub extern "C" fn sx_sweep_heading(base: i32, elapsed: u32, step_ticks: u32, count: i32) -> i32 {
    if count < 0 {
        return sweep_heading(base, elapsed, step_ticks, &LOOK_OFFSETS);
    }
    SCRATCH.with(|s| {
        let s = s.borrow();
        let n = (count as usize).min(s.len());
        sweep_heading(base, elapsed, step_ticks, &s[..n])
    })
}
