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
use sagax_decide::{Candidate, Consideration, Curve, DecideEvent, Decider, Pick};

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
