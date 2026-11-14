//! WebAssembly interface of Sagax, for JavaScript games (browser, Node, Foundry VTT).
//!
//! Same shape as the C interface (`sagax.h`), but the map is not asked
//! through callbacks: a call from WebAssembly into JavaScript is slow, and JS
//! maps are small. JavaScript fills a [`DenseGrid`] through typed-array views
//! on the WebAssembly memory (costs, walls between cells, zones) and then asks
//! as often as it likes.
//!
//! * Lists go in through the input buffer ([`sx_input`]) and come out of the
//!   output buffer ([`sx_out`]), both `Int32Array`s of `x, y, level` triples
//!   (areas and flow answers: `x, y, level, cost`).
//! * Every search also fills the result ([`sx_result`]): status, cells, cost,
//!   expanded, node limit hit, as in `sx_result` of the C interface.
//! * Functions that write a list return its length, others 0; -1 means bad
//!   arguments. Ways always begin with the start.
//!
//! The JS wrapper (`js/sagax.mjs`) hides the raw calls.

use sagax_abi::codes::{CUT_CORNERS, ERROR_ARGUMENT, FOUND, FOUR_WAYS, RUNNING, UNREACHABLE};
use sagax_abi::convert::{self, nav_status, to_i32, u32_of, whole_i64};
use sagax_nav::Cost;
use sagax_nav::Limits;
use sagax_nav::dstar::{DStarLite, DStarStatus};
use sagax_nav::flow::FlowField;
use sagax_nav::formation::Formation;
use sagax_nav::grid::{Cell, DenseGrid, GridRules, Moves};
use sagax_nav::nav::{Navigator, Output, Report, WayOptions, write_way};
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
        kind => Some(convert::hex_layout(u32_of(kind)?)?),
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

impl WasmGrid {
    fn new(mut grid: DenseGrid, topology: Topology) -> Self {
        let capacity = grid.costs_mut().len();
        let mut dstar = DStarLite::with_capacity(capacity);
        dstar.set_max_nodes(capacity);
        Self {
            grid,
            topology,
            nav: Navigator::new(topology, capacity),
            flow: FlowField::with_capacity(capacity),
            dstar,
            begun: Begun::Nothing,
            capacity,
            smooth_max_extra: 0,
            result: [0; 5],
            input: Vec::new(),
            out: Vec::with_capacity(capacity * 4),
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
            nav_status(report.status),
            to_i32(report.cells),
            to_i32(report.cost),
            to_i32(report.expanded),
            i32::from(report.node_limit_hit),
        ];
    }

    fn write_cells(out: &mut Vec<i32>, cells: &[Cell]) -> i32 {
        out.clear();
        for c in cells {
            out.extend_from_slice(&[c.x, c.y, c.level]);
        }
        to_i32(cells.len())
    }

    fn write_way(&mut self, output: Output) -> i32 {
        Self::write_cells(&mut self.out, self.nav.output(&mut self.grid, output))
    }

    fn write_area(&mut self) -> i32 {
        self.out.clear();
        for (c, cost) in self.nav.area() {
            self.out
                .extend_from_slice(&[c.x, c.y, c.level, to_i32(cost)]);
        }
        to_i32(self.out.len() / 4)
    }

    /// The first `count` cells (`x, y, level`) of the input buffer into
    /// `self.cells`.
    fn input_cells(&mut self, count: usize) -> bool {
        let Some(numbers) = count.checked_mul(3).and_then(|n| self.input.get(..n)) else {
            return false;
        };
        self.cells.clear();
        self.cells
            .extend(numbers.as_chunks::<3>().0.iter().map(|&c| Cell::from(c)));
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

/// A grid of `width` x `height` open cells on each of `levels` levels with
/// fixed rules: `kind` 0 square (`straight`, `diagonal`, `flags` 1 four
/// ways, 2 cut corners) or 1..4 hex (`straight` per step). Null on bad
/// arguments.
#[unsafe(no_mangle)]
pub extern "C" fn sx_grid_new(
    width: i32,
    height: i32,
    levels: i32,
    kind: i32,
    straight: i32,
    diagonal: i32,
    flags: u32,
) -> *mut WasmGrid {
    let Some(topology) = topology_of(kind, straight, diagonal, flags) else {
        return std::ptr::null_mut();
    };
    if width <= 0 || height <= 0 || levels <= 0 {
        return std::ptr::null_mut();
    }
    let grid = DenseGrid::with_levels(width, height, levels);
    Box::into_raw(Box::new(WasmGrid::new(grid, topology)))
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

/// Extra cost per cell, `width * height * levels` `u32`s, level by level and
/// row by row (`(level * height + y) * width + x`); `0xFFFFFFFF` blocks a
/// cell.
///
/// # Safety
/// `g` from [`sx_grid_new`]. The pointer is valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_costs(g: *mut WasmGrid) -> *mut u32 {
    grid!(g, std::ptr::null_mut()).grid.costs_mut().as_mut_ptr()
}

/// Wall bits per cell, `width * height * levels` bytes in the order of
/// [`sx_grid_costs`]; bit `i` blocks the move by
/// the neighbour offset `i` (0 +x, 1 -x, 2 +y, 3 -y, 4 +x+y, 5 +x-y, 6 -x+y,
/// 7 -x-y), on square and hex grids alike. On hex grids the offset is the
/// neighbour (see [`sx_hex_neighbors`]) minus the cell; since 0.9 the bits no
/// longer follow the hex direction number.
///
/// # Safety
/// As [`sx_grid_costs`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_walls(g: *mut WasmGrid) -> *mut u8 {
    grid!(g, std::ptr::null_mut())
        .grid
        .wall_bits_mut()
        .as_mut_ptr()
}

/// Zone bits per cell, `width * height * levels` `u32`s in the order of
/// [`sx_grid_costs`]; leaving a zone costs
/// [`sx_grid_set_leave_cost`] per zone left.
///
/// # Safety
/// As [`sx_grid_costs`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_zones(g: *mut WasmGrid) -> *mut u32 {
    grid!(g, std::ptr::null_mut()).grid.zones_mut().as_mut_ptr()
}

/// Links the neighbours `ax, ay, al` and `bx, by, bl`: a move from one
/// toward the other arrives on the level of the other. Returns 1, or 0
/// (nothing changed) unless they are neighbours on the grid.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_grid_link(
    g: *mut WasmGrid,
    ax: i32,
    ay: i32,
    al: i32,
    bx: i32,
    by: i32,
    bl: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    i32::from(w.grid.link(Cell::new(ax, ay, al), Cell::new(bx, by, bl)))
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

/// The six hex neighbours of `x`, `y` in direction order as `x, y` pairs
/// (the wall bit of a neighbour is that of its offset, see
/// [`sx_grid_walls`]). Returns 6, or -1 on a square grid.
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

/// Cheapest way from `sx, sy, sl` to the cheapest of `count` goals (`x, y,
/// level` triples in the input buffer; on failure to the reachable cell
/// closest to one), written as `output` (0 cells, 1 turning points, 2
/// smoothed).
/// `budget` 0: no limit.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_find_path(
    g: *mut WasmGrid,
    sx: i32,
    sy: i32,
    sl: i32,
    count: i32,
    budget: i32,
    output: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let (Some(budget), Some(output), Ok(count)) = (
        convert::budget(budget),
        u32_of(output).and_then(convert::output),
        usize::try_from(count),
    ) else {
        return ERROR_ARGUMENT;
    };
    if !w.input_cells(count) {
        return ERROR_ARGUMENT;
    }
    let report = w
        .nav
        .find_path(&mut w.grid, Cell::new(sx, sy, sl), &w.cells, budget);
    w.report(report);
    w.begun = Begun::Path(output);
    w.write_way(output)
}

/// A way from `sx, sy, sl` away from `count` threats (`x, y, level` triples
/// in the input buffer), within `max_cost`, written as `output`. See
/// `Navigator::flee`.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flee(
    g: *mut WasmGrid,
    sx: i32,
    sy: i32,
    sl: i32,
    count: i32,
    max_cost: i32,
    output: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let (Some(output), Ok(count), Ok(max_cost)) = (
        u32_of(output).and_then(convert::output),
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
        .flee(&mut w.grid, Cell::new(sx, sy, sl), &w.cells, max_cost);
    w.report(report);
    w.begun = Begun::Path(output);
    w.write_way(output)
}

/// Every cell reachable from `sx, sy, sl` at a cost of at most `max_cost`,
/// as `x, y, level, cost` quadruples.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_reachable(
    g: *mut WasmGrid,
    sx: i32,
    sy: i32,
    sl: i32,
    max_cost: i32,
    budget: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let (Some(budget), Ok(max_cost)) = (convert::budget(budget), Cost::try_from(max_cost)) else {
        return ERROR_ARGUMENT;
    };
    let report = w
        .nav
        .reachable(&mut w.grid, Cell::new(sx, sy, sl), max_cost, budget);
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
    let Some(budget) = convert::budget(budget) else {
        return ERROR_ARGUMENT;
    };
    let report = w.nav.continue_search(&mut w.grid, budget);
    w.report(report);
    match w.begun {
        Begun::Nothing => 0,
        Begun::Path(output) => w.write_way(output),
        Begun::Area => w.write_area(),
    }
}

/// After a search: the way from its start to `x, y, level` becomes the
/// current way and is written. Status found, or unreachable (nothing written).
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_path_to(
    g: *mut WasmGrid,
    x: i32,
    y: i32,
    level: i32,
    output: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    let Some(output) = u32_of(output).and_then(convert::output) else {
        return ERROR_ARGUMENT;
    };
    match w.nav.path_to(Cell::new(x, y, level)) {
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
    let Some(output) = u32_of(output).and_then(convert::output) else {
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
            w.out.extend_from_slice(&[c.x, c.y, c.level]);
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

/// For the `count` walkers in the input buffer: `x, y, level, cost` of the
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
            .extend_from_slice(&[n.x, n.y, n.level, cost.map_or(-1, to_i32)]);
    }
    moved
}

/// D* Lite: plans from `sx, sy, sl` to `gx, gy, gl`; nothing is searched
/// yet.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_begin(
    g: *mut WasmGrid,
    sx: i32,
    sy: i32,
    sl: i32,
    gx: i32,
    gy: i32,
    gl: i32,
) -> i32 {
    let w = grid!(g, ERROR_ARGUMENT);
    w.dstar
        .begin(Cell::new(sx, sy, sl), Cell::new(gx, gy, gl), w.topology);
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
    let (Some(budget), Some(output)) = (
        convert::budget_cells(budget),
        u32_of(output).and_then(convert::output),
    ) else {
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

/// D* Lite: the walker now stands on `x, y, level`.
///
/// # Safety
/// `g` from [`sx_grid_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_dstar_move_start(g: *mut WasmGrid, x: i32, y: i32, level: i32) -> i32 {
    grid!(g, ERROR_ARGUMENT)
        .dstar
        .move_start(Cell::new(x, y, level));
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
use sagax_decide::{Apply, Candidate, Consideration, Curve, DecideEvent, Decider, Pick};

/// Numbers per decision trace event: kind (1 factor, 2 scored, 3 chosen),
/// candidate, consideration (-1 if none), input, value, score.
const DECIDE_STRIDE: usize = 6;

/// A decider with its buffers.
pub struct WasmDecider {
    decider: Held<Decider>,
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
        decider: Held::Own(Decider::new()),
        input: Vec::new(),
        candidates: Vec::new(),
        rows: Vec::new(),
        scores: Vec::new(),
        trace_on: false,
        trace: Vec::new(),
        out: Vec::new(),
    }))
}

/// Frees a decider; does nothing for the squad's (see [`sx_squad_decider`]),
/// which is freed with the squad.
///
/// # Safety
/// `d` is null, from [`sx_decider_new`] and not used again, or from
/// [`sx_squad_decider`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_free(d: *mut WasmDecider) {
    // SAFETY: null or valid (see above).
    match unsafe { d.as_ref() } {
        None
        | Some(WasmDecider {
            decider: Held::Via { .. },
            ..
        }) => {}
        // SAFETY: see above.
        Some(_) => drop(unsafe { Box::from_raw(d) }),
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

/// Combine 0..4 for every option (`option` -1, JS `ALL_OPTIONS`) or one
/// option (`combine` -1: follow the decider). Any other negative `option`
/// is an argument error.
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
    let target = match option {
        -1 => CombineTarget::All,
        option => match u32_of(option) {
            Some(option) => CombineTarget::Option(option),
            None => return ERROR_ARGUMENT,
        },
    };
    // -1 is the C `SX_COMBINE_FOLLOW` (all bits set); other negatives wrap
    // to numbers that are no combine.
    if convert::set_combine(&mut w.decider, target, combine as u32) {
        0
    } else {
        ERROR_ARGUMENT
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
        .record(now, Candidate::new(option, target));
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
/// see `DECIDE_STRIDE`) and returns how many.
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
    Area, Areas, Evict, Fact, Inbox, Knowledge, LOOK_OFFSETS, Message, Outbox, PointPattern,
    Refused, Route, Sine, Timers, points_around, route, sine, sweep_heading,
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

/// Where a handle keeps its value: its own, one lent by a squad (the
/// squad's own memory, see [`sx_squad_knowledge`]), or one lent by a crew
/// (its squad and keepers, see [`sx_crew_squad`] and [`sx_crew_keeper`]).
enum Held<T> {
    Own(T),
    /// Lent by a [`WasmSquad`], which outlives the handle (it frees it).
    /// Only the squad's root pointer is kept: the table is reached through
    /// it anew on every call, so no pointer into the squad is held across
    /// calls that borrow the squad itself.
    Via {
        root: *mut WasmSquad,
        get: fn(&mut Squad) -> &mut T,
    },
    /// Lent by a [`WasmCrew`], reached through its root the same way.
    Crew(Lend<T>),
}

/// A value lent by a crew: the crew's root pointer, the member it belongs
/// to and the crew's epoch when lent.
struct Lend<T> {
    /// The crew, as `sx_crew_new` made it (`Box::into_raw`).
    root: *mut WasmCrew,
    /// The member the value belongs to (unused for the squad).
    id: u32,
    /// Once a call changed the crew the handle is stale; `None`: never
    /// stale (the squad).
    epoch: Option<u64>,
    /// The value in the crew, or `None` (the member left the group).
    get: fn(&mut WasmCrew, u32) -> Option<&mut T>,
}

impl<T> Lend<T> {
    /// The value, or `None` for a stale handle.
    fn reach<'a>(&self) -> Option<&'a mut T> {
        // SAFETY: `root` came from `Box::into_raw` in sx_crew_new and the
        // crew frees its lent handles with itself, so it is alive while this
        // handle is. The handle lives in its own allocation (not inside the
        // crew), and JS is single threaded, so no other reference into the
        // crew is alive during a call; the reference is used only within it.
        let crew = unsafe { &mut *self.root };
        if self.epoch.is_some_and(|e| e != crew.epoch()) {
            return None;
        }
        (self.get)(crew, self.id)
    }
}

impl<T> Held<T> {
    /// False for a lent keeper that went stale (the crew changed, or its
    /// member left the group).
    fn is_live(&self) -> bool {
        match self {
            Self::Crew(l) => l.reach().is_some(),
            _ => true,
        }
    }

    /// True for a value lent by a crew (freeing the handle does nothing).
    const fn is_lent(&self) -> bool {
        matches!(self, Self::Crew(_))
    }
}

impl<T> std::ops::Deref for Held<T> {
    type Target = T;
    fn deref(&self) -> &T {
        match self {
            Self::Own(t) => t,
            // SAFETY: the root is valid while its squad lives, and the squad
            // frees the handle with itself; JS is single threaded, so no
            // other reference into the squad is alive during a call.
            Self::Via { root, get } => get(unsafe { &mut (**root).squad }),
            // Callers check `is_live` first (keepers; the squad never goes
            // stale).
            Self::Crew(l) => l.reach().expect("a stale lent handle"),
        }
    }
}

impl<T> std::ops::DerefMut for Held<T> {
    fn deref_mut(&mut self) -> &mut T {
        match self {
            Self::Own(t) => t,
            // SAFETY: as in `deref`.
            Self::Via { root, get } => get(unsafe { &mut (**root).squad }),
            Self::Crew(l) => l.reach().expect("a stale lent handle"),
        }
    }
}

/// Knowledge with a fact record (in and out) and the facts handed out by the
/// last call that removed or evicted some.
pub struct WasmKnowledge {
    knowledge: Held<Knowledge>,
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
            knowledge: Held::Own(knowledge),
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
    // SAFETY: see above.
    match unsafe { k.as_ref() } {
        // A squad's own memory is freed with the squad.
        None
        | Some(WasmKnowledge {
            knowledge: Held::Via { .. },
            ..
        }) => {}
        // SAFETY: see above.
        Some(_) => drop(unsafe { Box::from_raw(k) }),
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
    let evicted = w.knowledge.learn(now, fact_of(&w.record));
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
    let evicted = w.knowledge.merge(now, fact_of(&w.record));
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
    match w.outbox.post(now, message_of(&w.record), delay, cooldown) {
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
    areas: Held<Areas>,
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
            areas: Held::Own(areas),
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
    // SAFETY: see above.
    match unsafe { a.as_ref() } {
        // A squad's own memory is freed with the squad.
        None
        | Some(WasmAreas {
            areas: Held::Via { .. },
            ..
        }) => {}
        // SAFETY: see above.
        Some(_) => drop(unsafe { Box::from_raw(a) }),
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
    to_i32(w.areas.add(now, area_of(&w.record)))
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
        1 => match whole_i64(arg) {
            Some(range) => Route::InRange(range),
            None => return ERROR_ARGUMENT,
        },
        2 => Route::All,
        3 if arg >= 0.0 => Route::To(arg as u32),
        _ => return ERROR_ARGUMENT,
    };
    let Some(metric) = u32_of(metric).and_then(convert::metric) else {
        return ERROR_ARGUMENT;
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

// ---- group tactics -----------------------------------------------------------
//
// Same blocks as the C interface (`sx_squad_*`, `sx_tactic_*`, `sx_keeper_*`,
// `sx_gather`, `sx_areas_move_cost`), but without structs: records go in
// through the scratch buffer ([`sx_scratch`]) and come out of the handle's
// out buffer (`i32`s; `u32` values as their bit pattern). Numbers as in
// `sagax.h` (`SX_PLACE_*`, `SX_GOAL_*`, `SX_START_*`, ...). Values beyond
// 32 bits (radii, ranges) come as `f64`, like the route argument.

use sagax_abi::convert::{CombineTarget, SkillCodes};
use sagax_decide::group::{
    Abort, Channel, Goal, GoalKeeper, MAX_ROLE_COUNTS, Member, MemberState, Nearby, Order, Place,
    Plan, PlanError, RoleCounts, SkillTable, Slot, Spot, Squad, SquadConfig, Tactic, Trigger,
    gather, tactics,
};

/// Numbers per member record: id, x, y, z, roles, skill, state.
const MEMBER_SIZE: usize = 7;
/// Numbers per spot record: x, y, z, tags.
const SPOT_SIZE: usize = 4;
/// Numbers per gather candidate: id, faction, x, y, z, free.
const CANDIDATE_SIZE: usize = 6;
/// Numbers per slot record: roles, min, max, place (kind, x, y, z, value),
/// gather (the same five), kind, custom, start, start ticks, want tags, dist,
/// gather dist (20 numbers).
const SLOT_SIZE: usize = 20;
/// Numbers per goal record: kind, custom, x, y, z, then x, y, z, target,
/// start, start ticks, slot, plan, issued at, start delay, has pos,
/// unchecked, has then pos, then unchecked, has target.
const GOAL_SIZE: usize = 20;
// An order record is the member's id and a goal record (21 numbers); a
// plan record is id, tactic, target, made at, active at, goal count, has
// target (7 numbers), then one order record per goal.
/// Numbers per skill table: the rows, then pick, pick top, think ticks,
/// timing spread, hold bonus, give up ticks, spot rank error; eight levels
/// each.
const SKILL_SIZE: usize = 1 + 7 * 8;
/// Numbers per score log event: tactic, target, has target, then the
/// decision event (kind 1 factor or 2 scored, candidate, consideration,
/// input, value, score).
const SCORE_STRIDE: usize = 9;

/// The first `count` numbers of the scratch buffer, copied.
fn scratch_copy(count: usize) -> Option<Vec<i32>> {
    SCRATCH.with(|s| s.borrow().get(..count).map(<[i32]>::to_vec))
}

/// The role list of `count` masks in the scratch buffer; `None` above 8.
fn role_list(count: u32) -> Option<RoleCounts> {
    if count as usize > MAX_ROLE_COUNTS {
        return None;
    }
    let masks = scratch_copy(count as usize)?;
    let masks: Vec<u32> = masks.iter().map(|&m| m as u32).collect();
    convert::roles(count, &masks)
}

/// Writes `values` to the start of the scratch buffer (it has room).
fn scratch_put(values: &[i32]) {
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() < values.len() {
            s.resize(values.len(), 0);
        }
        s[..values.len()].copy_from_slice(values);
    });
}

fn member_of(r: &[i32]) -> Option<Member> {
    Some(Member {
        id: r[0] as u32,
        pos: [r[1], r[2], r[3]],
        roles: r[4] as u32,
        skill: u8::try_from(r[5]).ok()?,
        state: convert::member_state(u32_of(r[6])?)?,
    })
}

fn member_record(m: &Member) -> [i32; MEMBER_SIZE] {
    let p = m.pos;
    let state = m.state as i32;
    [
        m.id as i32,
        p[0],
        p[1],
        p[2],
        m.roles as i32,
        i32::from(m.skill),
        state,
    ]
}

/// The place of five numbers: kind, x, y, z, value. `Some(None)` for no
/// place (kind 0), `None` if invalid.
fn place_from(r: &[i32]) -> Option<Option<Place>> {
    convert::place(u32_of(r[0])?, r[4] as u32, [r[1], r[2], r[3]])
}

fn slot_of(r: &[i32]) -> Option<Slot> {
    Some(Slot {
        roles: r[0] as u32,
        min: u8::try_from(r[1]).ok()?,
        max: u8::try_from(r[2]).ok()?,
        place: place_from(&r[3..8])??,
        gather: place_from(&r[8..13])?,
        kind: convert::goal_kind(u32_of(r[13])?, r[14] as u32)?,
        start: convert::start(u32_of(r[15])?, r[16] as u32)?,
        want_tags: r[17] as u32,
        dist: r[18],
        gather_dist: r[19],
    })
}

fn goal_record(g: &Goal) -> [i32; GOAL_SIZE] {
    let (kind, custom) = convert::goal_kind_code(g.kind);
    let (start, ticks) = convert::start_code(g.start);
    let p = g.pos.unwrap_or_default();
    let t = g.then_pos.unwrap_or_default();
    [
        kind as i32,
        custom as i32,
        p[0],
        p[1],
        p[2],
        t[0],
        t[1],
        t[2],
        g.target.unwrap_or(0) as i32,
        start as i32,
        ticks as i32,
        i32::from(g.slot),
        g.plan as i32,
        g.issued_at as i32,
        g.start_delay as i32,
        i32::from(g.pos.is_some()),
        i32::from(g.unchecked),
        i32::from(g.then_pos.is_some()),
        i32::from(g.then_unchecked),
        i32::from(g.target.is_some()),
    ]
}

fn goal_of(r: &[i32]) -> Option<Goal> {
    Some(Goal {
        kind: convert::goal_kind(u32_of(r[0])?, r[1] as u32)?,
        pos: (r[15] != 0).then_some([r[2], r[3], r[4]]),
        unchecked: r[16] != 0,
        then_pos: (r[17] != 0).then_some([r[5], r[6], r[7]]),
        then_unchecked: r[18] != 0,
        target: (r[19] != 0).then_some(r[8] as u32),
        start: convert::start(u32_of(r[9])?, r[10] as u32)?,
        slot: u8::try_from(r[11]).ok()?,
        plan: r[12] as u32,
        issued_at: r[13] as u32,
        start_delay: r[14] as u32,
    })
}

fn put_order(out: &mut Vec<i32>, member: u32, goal: &Goal) {
    out.push(member as i32);
    out.extend_from_slice(&goal_record(goal));
}

fn skill_record(t: &SkillTable) -> [i32; SKILL_SIZE] {
    let c = convert::skill_codes(t);
    let mut r = [0; SKILL_SIZE];
    r[0] = c.rows as i32;
    for i in 0..8 {
        r[1 + i] = c.pick[i] as i32;
        r[9 + i] = c.pick_top[i] as i32;
        r[17 + i] = c.think_ticks[i] as i32;
        r[25 + i] = c.timing_spread[i] as i32;
        r[33 + i] = c.hold_bonus[i] as i32;
        r[41 + i] = c.give_up_ticks[i] as i32;
        r[49 + i] = i32::from(c.spot_rank_error[i]);
    }
    r
}

/// The table of a record; `None` for rows 0 or above 8, an unknown pick or a
/// negative number (a skill table has no negative entry).
fn skill_from(r: &[i32]) -> Option<SkillTable> {
    let mut c = SkillCodes {
        rows: u32_of(r[0])?,
        ..SkillCodes::default()
    };
    for i in 0..8 {
        c.pick[i] = u32_of(r[1 + i])?;
        c.pick_top[i] = u32_of(r[9 + i])?;
        c.think_ticks[i] = u32_of(r[17 + i])?;
        c.timing_spread[i] = u32_of(r[25 + i])?;
        c.hold_bonus[i] = u32_of(r[33 + i])?;
        c.give_up_ticks[i] = u32_of(r[41 + i])?;
        c.spot_rank_error[i] = u8::try_from(r[49 + i]).ok()?;
    }
    convert::skill_table(&c)
}

/// Picks up to `max` of the `n` candidates in the scratch buffer (6 numbers
/// each: id, faction, x, y, z, free) of `faction` within `radius` of `x, y`,
/// nearest first (see `sx_gather`). The ids go to the scratch buffer;
/// returns how many, or -1 on bad arguments.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn sx_gather(
    x: i32,
    y: i32,
    faction: u32,
    radius: f64,
    metric: i32,
    free_only: u32,
    n: u32,
    max: u32,
) -> i32 {
    let (Some(metric), Some(radius)) =
        (u32_of(metric).and_then(convert::metric), whole_i64(radius))
    else {
        return ERROR_ARGUMENT;
    };
    let Some(r) = (n as usize)
        .checked_mul(CANDIDATE_SIZE)
        .and_then(scratch_copy)
    else {
        return ERROR_ARGUMENT;
    };
    let list: Vec<Nearby> = r
        .as_chunks::<CANDIDATE_SIZE>()
        .0
        .iter()
        .map(|c| Nearby {
            id: c[0] as u32,
            faction: c[1] as u32,
            pos: [c[2], c[3], c[4]],
            free: c[5] != 0,
        })
        .collect();
    let mut ids = Vec::new();
    gather(
        [x, y, 0],
        faction,
        radius,
        metric,
        free_only != 0,
        &list,
        &mut ids,
        max as usize,
    );
    let ids: Vec<i32> = ids.iter().map(|&id| id as i32).collect();
    scratch_put(&ids);
    to_i32(ids.len())
}

/// The extra cost of the move from cell to cell (see `Areas::move_cost`);
/// reads 12 integers from the scratch buffer (from, to, start, goal).
/// Returns 0 if blocked, else 1 with the cost (at most `i32::MAX - 1`, as
/// in C) in the first number of the scratch buffer; -1 on bad arguments.
/// The areas and cells are in the same (cell) coordinates.
///
/// # Safety
/// `a` from [`sx_areas_new`] or [`sx_squad_areas`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_move_cost(a: *mut WasmAreas) -> i32 {
    let w = areas!(a);
    let Some(p) = scratch_copy(12) else {
        return ERROR_ARGUMENT;
    };
    let cell = |i: usize| Cell::new(p[i], p[i + 1], p[i + 2]);
    match w.areas.cell_cost(cell(3), cell(6), cell(9)) {
        Some(cost) => {
            scratch_put(&[cost.min(i32::MAX as u32 - 1) as i32]);
            1
        }
        None => 0,
    }
}

/// Offset `x`, `y` of a hex grid to axial `q`, `r`, written as two integers
/// to the scratch buffer. `layout` is 1..4 as in `sx_rules.grid` (odd rows,
/// even rows, odd columns, even columns). Returns 0, or -1 for another layout.
#[unsafe(no_mangle)]
pub extern "C" fn sx_hex_to_axial(layout: u32, x: i32, y: i32) -> i32 {
    let Some([q, r, _]) = convert::hex_to_axial(layout, [x, y, 0]) else {
        return ERROR_ARGUMENT;
    };
    scratch_put(&[q, r]);
    0
}

/// Axial `q`, `r` to offset `x`, `y` (two integers in the scratch buffer);
/// see [`sx_hex_to_axial`].
#[unsafe(no_mangle)]
pub extern "C" fn sx_hex_from_axial(layout: u32, q: i32, r: i32) -> i32 {
    let Some([x, y, _]) = convert::hex_from_axial(layout, [q, r, 0]) else {
        return ERROR_ARGUMENT;
    };
    scratch_put(&[x, y]);
    0
}

/// A tactic being built (`sx_tactic_*`).
pub struct WasmTactic(Tactic);

macro_rules! tactic {
    ($t:expr) => {
        // SAFETY: JS passes a handle from sx_tactic_new (wrapper guarantees it).
        match unsafe { $t.as_mut() } {
            Some(t) => &mut t.0,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// A new, empty tactic (see `sx_tactic_new`). Null if `min_skill > 255`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_new(weight: u32, min_skill: u32, needs_target: u32) -> *mut WasmTactic {
    let Ok(min_skill) = u8::try_from(min_skill) else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(WasmTactic(Tactic {
        slots: Vec::new(),
        considerations: Vec::new(),
        weight,
        aborts: Vec::new(),
        trigger: Trigger::None,
        min_skill,
        needs_target: needs_target != 0,
        abort_if_target_lost: true,
    })))
}

/// A tactic as a handle for [`sx_squad_add_tactic`].
fn shipped(t: Tactic) -> *mut WasmTactic {
    Box::into_raw(Box::new(WasmTactic(t)))
}

/// The shipped rush tactic (`sx_squad_add_rush`).
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_rush() -> *mut WasmTactic {
    shipped(tactics::rush())
}

/// The shipped formation tactic: `shape` 0 line, 1 wedge, 2 column
/// (`SX_SHAPE_*`), members `spacing` apart. Null for another shape.
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_formation(shape: u32, spacing: i32) -> *mut WasmTactic {
    convert::shape(shape).map_or(std::ptr::null_mut(), |shape| {
        shipped(tactics::formation(shape, spacing))
    })
}

/// The shipped flank tactic from `sides` 2 or 3, at `dist`. Null for another
/// number of sides.
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_flank(sides: u32, dist: i32) -> *mut WasmTactic {
    convert::flank_sides(sides).map_or(std::ptr::null_mut(), |sides| {
        shipped(tactics::flank(sides, dist))
    })
}

/// The shipped ambush tactic.
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_ambush() -> *mut WasmTactic {
    shipped(tactics::ambush())
}

/// The shipped focus tactic.
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_focus() -> *mut WasmTactic {
    shipped(tactics::focus())
}

/// The shipped isolate tactic.
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_isolate() -> *mut WasmTactic {
    shipped(tactics::isolate())
}

/// The shipped fall-back tactic.
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_fall_back() -> *mut WasmTactic {
    shipped(tactics::fall_back())
}

/// Frees a tactic not given to a squad.
///
/// # Safety
/// `t` is null or from [`sx_tactic_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_free(t: *mut WasmTactic) {
    if !t.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(t) });
    }
}

/// Adds the slot record (20 numbers) in the scratch buffer; returns its
/// index, or -1 for an unknown place, side, kind or start, a goal place of
/// none, or a min or max above 255.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_slot(t: *mut WasmTactic) -> i32 {
    let t = tactic!(t);
    let Some(slot) = scratch_copy(SLOT_SIZE).as_deref().and_then(slot_of) else {
        return ERROR_ARGUMENT;
    };
    t.slots.push(slot);
    to_i32(t.slots.len() - 1)
}

/// Adds a factor over the scoring row: input column, curve 0..9, min, max.
/// Returns 0; -1 for an unknown curve.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_consideration(
    t: *mut WasmTactic,
    input: u32,
    curve: u32,
    min: i32,
    max: i32,
) -> i32 {
    let t = tactic!(t);
    let Some(curve) = Curve::from_u32(curve) else {
        return ERROR_ARGUMENT;
    };
    t.considerations
        .push(Consideration::new(input, curve, min, max));
    0
}

/// The plan ends when `row[input] >= at_least`. Returns 0.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_abort(t: *mut WasmTactic, input: u32, at_least: i32) -> i32 {
    tactic!(t)
        .aborts
        .push(Abort::at_least(input as usize, at_least));
    0
}

/// The plan ends when `row[input]` compares to `value` as `cmp`
/// (`SX_CMP_*`). Returns 0, or `SX_ERROR_ARGUMENT` for an unknown `cmp`.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_abort_when(
    t: *mut WasmTactic,
    input: u32,
    cmp: u32,
    value: i32,
) -> i32 {
    let Some(cmp) = convert::compare(cmp) else {
        return ERROR_ARGUMENT;
    };
    tactic!(t).aborts.push(Abort::When {
        input: input as usize,
        cmp,
        value,
    });
    0
}

/// Whether the plan ends when its target is gone (the default). Returns 0.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_set_abort_if_target_lost(t: *mut WasmTactic, on: u32) -> i32 {
    tactic!(t).abort_if_target_lost = on != 0;
    0
}

/// The signal never fires by itself (the default). Returns 0.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_none(t: *mut WasmTactic) -> i32 {
    tactic!(t).trigger = Trigger::None;
    0
}

/// The signal fires when every active member arrived, or `max_wait` ticks
/// after activation. Returns 0.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_all_arrived(t: *mut WasmTactic, max_wait: u32) -> i32 {
    tactic!(t).trigger = Trigger::AllArrived { max_wait };
    0
}

/// The signal fires when the target is within `radius` of the group centre.
/// Returns 0; -1 if `radius` is not a whole number in range.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_target_in_radius(
    t: *mut WasmTactic,
    radius: f64,
) -> i32 {
    let t = tactic!(t);
    let Some(radius) = whole_i64(radius) else {
        return ERROR_ARGUMENT;
    };
    t.trigger = Trigger::TargetInRadius(radius);
    0
}

/// The signal is only given by hand. Returns 0.
///
/// # Safety
/// `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_manual(t: *mut WasmTactic) -> i32 {
    tactic!(t).trigger = Trigger::Manual;
    0
}

/// The default skill table (57 numbers) in the scratch buffer. Returns 0.
#[unsafe(no_mangle)]
pub extern "C" fn sx_skill_table_default() -> i32 {
    scratch_put(&skill_record(&SkillTable::default()));
    0
}

/// The skill table to read (the default, or with `custom` not 0 the one in
/// the scratch buffer) and the skill (0 .. 255).
fn table_and_skill(skill: u32, custom: u32) -> Option<(SkillTable, u8)> {
    let skill = u8::try_from(skill).ok()?;
    let table = if custom == 0 {
        SkillTable::default()
    } else {
        scratch_copy(SKILL_SIZE).as_deref().and_then(skill_from)?
    };
    Some((table, skill))
}

/// One number of the skill table at `skill`'s level: `which` 0 think ticks,
/// 1 timing spread, 2 hold bonus, 3 give-up ticks, 4 spot rank error. With
/// `custom` not 0 the table is read from the scratch buffer, else the
/// default. Writes the number to `scratch[0]` (as `u32` bits) and returns 0;
/// -1 for a bad argument or table.
#[unsafe(no_mangle)]
pub extern "C" fn sx_skill_for(which: u32, skill: u32, custom: u32) -> i32 {
    let Some((t, s)) = table_and_skill(skill, custom) else {
        return ERROR_ARGUMENT;
    };
    let value = match which {
        0 => t.think_ticks_for(s),
        1 => t.timing_spread_for(s),
        2 => t.hold_bonus_for(s),
        3 => t.give_up_ticks_for(s),
        4 => u32::from(t.spot_rank_error_for(s)),
        _ => return ERROR_ARGUMENT,
    };
    scratch_put(&[value as i32]);
    0
}

/// The pick at `skill`'s level to scratch[0..2] (pick code, top), read as
/// for [`sx_skill_for`]. Returns 0; -1 for a bad argument or table.
#[unsafe(no_mangle)]
pub extern "C" fn sx_skill_pick_for(skill: u32, custom: u32) -> i32 {
    let Some((t, s)) = table_and_skill(skill, custom) else {
        return ERROR_ARGUMENT;
    };
    let record = skill_record(&t);
    let level = t.level(s);
    scratch_put(&[record[1 + level], record[9 + level]]);
    0
}

/// A member's discipline at `skill` (0 .. 255), the old name of
/// [`sx_skill_for`] with `which` 2 (`custom` as there). Returns it, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn sx_discipline(skill: u32, custom: u32) -> i32 {
    if sx_skill_for(2, skill, custom) != 0 {
        return ERROR_ARGUMENT;
    }
    scratch_copy(1).map_or(ERROR_ARGUMENT, |v| to_i32(v[0] as u32))
}

/// A squad with its own memory, an out buffer and the orders that are due
/// but not taken yet.
pub struct WasmSquad {
    squad: Held<Squad>,
    out: Vec<i32>,
    due: Vec<Order>,
    /// Handles onto the squad's own knowledge and areas, freed with it.
    knowledge: *mut WasmKnowledge,
    areas: *mut WasmAreas,
    /// The handle on the squad's decider, freed with it.
    decider: *mut WasmDecider,
}

macro_rules! squad {
    ($s:expr) => {
        // SAFETY: JS passes a handle from sx_squad_new (wrapper guarantees it).
        match unsafe { $s.as_mut() } {
            Some(s) => s,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// A squad (see `sx_squad_new` in `sagax.h`). The `role_count` role masks
/// are read from the scratch buffer. Null if a capacity is 0, the metric is
/// unknown or `role_count` is above 8.
#[unsafe(no_mangle)]
pub extern "C" fn sx_squad_new(
    members: u32,
    memory: u32,
    areas: u32,
    inbox: u32,
    dangers: u32,
    metric: i32,
    role_count: u32,
) -> *mut WasmSquad {
    let Some(metric) = u32_of(metric).and_then(convert::metric) else {
        return std::ptr::null_mut();
    };
    let Some(roles) = role_list(role_count) else {
        return std::ptr::null_mut();
    };
    let Ok(squad) = Squad::new(SquadConfig {
        members: members as usize,
        memory: memory as usize,
        areas: areas as usize,
        inbox: inbox as usize,
        dangers: dangers as usize,
        metric,
        roles,
        ..SquadConfig::default()
    }) else {
        return std::ptr::null_mut();
    };
    boxed_squad(Held::Own(squad))
}

/// A squad handle with its knowledge and areas handles (freed by
/// [`drop_squad`]).
fn boxed_squad(squad: Held<Squad>) -> *mut WasmSquad {
    let s = Box::into_raw(Box::new(WasmSquad {
        squad,
        out: Vec::new(),
        due: Vec::new(),
        knowledge: std::ptr::null_mut(),
        areas: std::ptr::null_mut(),
        decider: std::ptr::null_mut(),
    }));
    // SAFETY: `s` was just made; the squad stays at this address until
    // drop_squad, which frees the two handles first.
    unsafe {
        (*s).knowledge = Box::into_raw(Box::new(WasmKnowledge {
            knowledge: Held::Via {
                root: s,
                get: Squad::knowledge_mut,
            },
            record: [0; FACT_SIZE],
            out: Vec::new(),
        }));
        (*s).areas = Box::into_raw(Box::new(WasmAreas {
            areas: Held::Via {
                root: s,
                get: Squad::areas_mut,
            },
            record: [0; AREA_SIZE],
        }));
        (*s).decider = Box::into_raw(Box::new(WasmDecider {
            decider: Held::Via {
                root: s,
                get: Squad::decider_mut,
            },
            input: Vec::new(),
            candidates: Vec::new(),
            rows: Vec::new(),
            scores: Vec::new(),
            trace_on: false,
            trace: Vec::new(),
            out: Vec::new(),
        }));
    }
    s
}

/// Frees a squad handle from [`boxed_squad`] with its three handles.
///
/// # Safety
/// `s` is from [`boxed_squad`] and not used again.
unsafe fn drop_squad(s: *mut WasmSquad) {
    // SAFETY: see above; the two handles were made by boxed_squad.
    unsafe {
        let w = Box::from_raw(s);
        drop(Box::from_raw(w.knowledge));
        drop(Box::from_raw(w.areas));
        drop(Box::from_raw(w.decider));
    }
}

/// Frees a squad with its own knowledge and areas; does nothing for a
/// squad lent by a crew (the crew frees it).
///
/// # Safety
/// `s` is null, from [`sx_squad_new`] and not used again, or from
/// [`sx_crew_squad`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_free(s: *mut WasmSquad) {
    // SAFETY: null or valid (see above).
    match unsafe { s.as_ref() } {
        None => {}
        Some(w) if w.squad.is_lent() => {}
        // SAFETY: from sx_squad_new (see above).
        Some(_) => unsafe { drop_squad(s) },
    }
}

/// The out buffer of the last call that wrote records.
///
/// # Safety
/// `s` from [`sx_squad_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_out(s: *mut WasmSquad) -> *const i32 {
    // SAFETY: see above.
    unsafe { s.as_ref() }.map_or(std::ptr::null(), |w| w.out.as_ptr())
}

/// The squad's own knowledge, for every `sx_knowledge_*` call. It belongs
/// to the squad (sx_knowledge_free does nothing to it).
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_knowledge(s: *mut WasmSquad) -> *mut WasmKnowledge {
    // SAFETY: see above.
    unsafe { s.as_ref() }.map_or(std::ptr::null_mut(), |w| w.knowledge)
}

/// The squad's own danger areas, for every `sx_areas_*` call. They belong
/// to the squad.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_areas(s: *mut WasmSquad) -> *mut WasmAreas {
    // SAFETY: see above.
    unsafe { s.as_ref() }.map_or(std::ptr::null_mut(), |w| w.areas)
}

/// The squad's decider (option `i` is tactic `i`), for every `sx_decider_*`
/// call that does not add options. It belongs to the squad
/// (sx_decider_free does nothing to it); the same handle comes back each
/// time.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_decider(s: *mut WasmSquad) -> *mut WasmDecider {
    // SAFETY: see above.
    unsafe { s.as_ref() }.map_or(std::ptr::null_mut(), |w| w.decider)
}

/// Switches the score log on (1) or off (0, clears it). Returns 0.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_score_log(s: *mut WasmSquad, on: i32) -> i32 {
    squad!(s).squad.set_score_log(on != 0);
    0
}

/// Moves the events of the last scoring in plan or check to the out buffer
/// (9 numbers each, see `SCORE_STRIDE`) and returns how many.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_score_log(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    w.out.clear();
    for e in w.squad.score_log() {
        let (kind, candidate, consideration, input, value, score) = match e.event {
            DecideEvent::Factor {
                candidate,
                consideration,
                input,
                value,
            } => (1, candidate, to_i32(consideration), input, value, 0),
            DecideEvent::Scored {
                candidate,
                product,
                score,
            } => (2, candidate, -1, 0, product, score),
            DecideEvent::Chosen { candidate, score } => (3, candidate, -1, 0, 0, score),
        };
        w.out.extend([
            to_i32(e.tactic),
            e.target.unwrap_or(0) as i32,
            i32::from(e.target.is_some()),
            kind,
            to_i32(candidate),
            consideration,
            input,
            value as i32,
            score as i32,
        ]);
    }
    to_i32(w.out.len() / SCORE_STRIDE)
}

/// Adds the member record (7 numbers) in the scratch buffer at the end.
/// Returns its index; -2 if the group is full or the id is there; -1 for a
/// bad state or skill.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_member(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    let Some(m) = scratch_copy(MEMBER_SIZE).as_deref().and_then(member_of) else {
        return ERROR_ARGUMENT;
    };
    w.squad.group_mut().add(m).map_or(-2, to_i32)
}

/// Removes a member (the order of the others stays); 1, or 0 if unknown.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_remove_member(s: *mut WasmSquad, id: u32) -> i32 {
    i32::from(squad!(s).squad.group_mut().remove(id))
}

/// Sets a member's position; 1, or 0 if unknown.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_pos(
    s: *mut WasmSquad,
    id: u32,
    x: i32,
    y: i32,
    z: i32,
) -> i32 {
    i32::from(squad!(s).squad.group_mut().set_pos(id, [x, y, z]))
}

/// Takes a member out of play for now; 1, or 0 if unknown.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_suspend(s: *mut WasmSquad, id: u32) -> i32 {
    i32::from(squad!(s).squad.group_mut().suspend(id))
}

/// Puts a member back and, if it was suspended, sends it its goal of the
/// active plan (see `Squad::reinstate`); 1, or 0 if unknown.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_reinstate(s: *mut WasmSquad, id: u32, now: u32) -> i32 {
    i32::from(squad!(s).squad.reinstate(id, now))
}

/// Marks a member dead or lost; 1, or 0 if unknown.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_gone(s: *mut WasmSquad, id: u32) -> i32 {
    i32::from(squad!(s).squad.set_gone(id))
}

/// Writes every member record (7 numbers each) to the out buffer; returns
/// how many.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_members(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    w.out.clear();
    for m in w.squad.group().iter() {
        w.out.extend_from_slice(&member_record(m));
    }
    to_i32(w.squad.group().len())
}

/// Sets the leader; 1, or 0 if the id is not in the group.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_leader(s: *mut WasmSquad, id: u32) -> i32 {
    i32::from(squad!(s).squad.set_leader(id))
}

/// The acting leader's id to the first number of the out buffer; 1, or 0
/// if there is none.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_leader(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    w.out.clear();
    match w.squad.leader() {
        Some(id) => {
            w.out.push(id as i32);
            1
        }
        None => 0,
    }
}

/// Sets the skill (0 .. 255). Returns 0; -1 if above 255.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_skill(s: *mut WasmSquad, skill: u32) -> i32 {
    let w = squad!(s);
    let Ok(skill) = u8::try_from(skill) else {
        return ERROR_ARGUMENT;
    };
    w.squad.set_skill(skill);
    0
}

/// The skill.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_skill(s: *mut WasmSquad) -> i32 {
    i32::from(squad!(s).squad.skill())
}

/// Sets the skill table from the scratch buffer (57 numbers). Returns 0;
/// -1 for an unknown pick, rows 0 or above 8, or a spot rank error above
/// 255 (nothing changes).
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_skill_table(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    let Some(table) = scratch_copy(SKILL_SIZE).as_deref().and_then(skill_from) else {
        return ERROR_ARGUMENT;
    };
    if w.squad.set_skill_table(table) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// The skill table (57 numbers) to the out buffer. Returns 0.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_skill_table(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    w.out.clear();
    w.out
        .extend_from_slice(&skill_record(w.squad.skill_table()));
    0
}

/// Orders (and reports) reach the member after 1 tick per unit, within
/// `range`. Returns 1, or 0 if the id is not in the group; -1 if `range` is
/// not a whole number in range.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_shout(s: *mut WasmSquad, id: u32, range: f64) -> i32 {
    let w = squad!(s);
    let Some(range) = whole_i64(range) else {
        return ERROR_ARGUMENT;
    };
    i32::from(w.squad.set_channel(id, Channel::Shout { range }))
}

/// Orders reach the member after `base + distance * per_unit` ticks.
/// Returns 1, or 0 if the id is not in the group.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_radio(
    s: *mut WasmSquad,
    id: u32,
    base: u32,
    per_unit: u32,
) -> i32 {
    let w = squad!(s);
    i32::from(w.squad.set_channel(id, Channel::Radio { base, per_unit }))
}

/// Orders never reach the member. Returns 1, or 0 if the id is not in the
/// group.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_silent(s: *mut WasmSquad, id: u32) -> i32 {
    i32::from(squad!(s).squad.set_channel(id, Channel::None))
}

/// A danger report on its way to the leader: centre (`cx`, `cy`, `cz`), the
/// shooter's position when `has_shooter` is not 0. Returns 1, or 0 when the
/// danger queue is full.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_squad_report_danger(
    s: *mut WasmSquad,
    sender: u32,
    cx: i32,
    cy: i32,
    cz: i32,
    has_shooter: u32,
    sx: i32,
    sy: i32,
    sz: i32,
    arrive_at: u32,
) -> i32 {
    let shooter = (has_shooter != 0).then_some([sx, sy, sz]);
    i32::from(
        squad!(s)
            .squad
            .report_danger(sender, [cx, cy, cz], shooter, arrive_at),
    )
}

/// The plan that became active since the last call to the first number of
/// the out buffer; 1, or 0 if none did.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_take_activated(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    w.out.clear();
    match w.squad.take_activated() {
        Some(id) => {
            w.out.push(id as i32);
            1
        }
        None => 0,
    }
}

/// When a report sent by member `id` at `now` reaches the leader, to the
/// first number of the out buffer; 1, or 0 if it never arrives.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_report_arrives_at(s: *mut WasmSquad, id: u32, now: u32) -> i32 {
    let w = squad!(s);
    w.out.clear();
    match w.squad.report_arrives_at(id, now) {
        Some(t) => {
            w.out.push(t as i32);
            1
        }
        None => 0,
    }
}

/// Sets the area a danger report adds from the area record in the scratch
/// buffer (14 numbers, as for areas). Returns 0.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_danger_area(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    let Some(r) = scratch_copy(AREA_SIZE) else {
        return ERROR_ARGUMENT;
    };
    let r: [u32; AREA_SIZE] = std::array::from_fn(|i| r[i] as u32);
    w.squad.set_danger_area(area_of(&r));
    0
}

/// The danger area record (14 numbers) to the out buffer. Returns 0.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_danger_area(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    w.out.clear();
    w.out
        .extend(area_record(&w.squad.danger_area()).map(|v| v as i32));
    0
}

/// The fact kind that counts as a target. Returns 0.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_target_kind(s: *mut WasmSquad, kind: u32) -> i32 {
    squad!(s).squad.set_target_kind(kind);
    0
}

/// How many of the game's own columns each scoring row has. Returns 0; -1
/// above `MAX_EXTRA_COLUMNS` (nothing changes).
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_extra_columns(s: *mut WasmSquad, count: u32) -> i32 {
    if squad!(s).squad.set_extra_columns(count as usize) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// The first column of the game's own (8, plus one for every role beyond
/// two); -1 for a null handle.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_extra_start(s: *mut WasmSquad) -> i32 {
    to_i32(squad!(s).squad.extra_start())
}

/// How far a spot may be from a slot's wish point; negative: the default
/// (3 * the slot's dist). Returns 0; -1 above `u32::MAX` or not a whole
/// number.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_spot_radius(s: *mut WasmSquad, radius: f64) -> i32 {
    let w = squad!(s);
    let Some(radius) = whole_i64(radius) else {
        return ERROR_ARGUMENT;
    };
    if radius < 0 {
        w.squad.set_spot_radius(None);
    } else if let Ok(radius) = u32::try_from(radius) {
        w.squad.set_spot_radius(Some(radius));
    } else {
        return ERROR_ARGUMENT;
    }
    0
}

/// The leader's own sighting: the fact record (9 numbers) in the scratch
/// buffer. Returns 1 if a fact was dropped (its record is in the out
/// buffer), else 0.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_observe(s: *mut WasmSquad, now: u32) -> i32 {
    let w = squad!(s);
    let Some(r) = scratch_copy(FACT_SIZE) else {
        return ERROR_ARGUMENT;
    };
    let r: [u32; FACT_SIZE] = std::array::from_fn(|i| r[i] as u32);
    w.out.clear();
    match w.squad.observe(now, fact_of(&r)) {
        Some(gone) => {
            w.out.extend(fact_record(&gone).map(|v| v as i32));
            1
        }
        None => 0,
    }
}

/// A member's report: the message record (10 numbers) in the scratch
/// buffer, read from `arrive_at` on. Returns 1, or 0 if the inbox refused
/// it.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_report(s: *mut WasmSquad, arrive_at: u32) -> i32 {
    let w = squad!(s);
    let Some(r) = scratch_copy(MESSAGE_SIZE) else {
        return ERROR_ARGUMENT;
    };
    let r: [u32; MESSAGE_SIZE] = std::array::from_fn(|i| r[i] as u32);
    i32::from(w.squad.report(message_of(&r), arrive_at))
}

/// The known targets as fact records (9 numbers each) to the out buffer;
/// returns how many.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_targets(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    w.out.clear();
    let mut n = 0;
    for f in w.squad.targets() {
        w.out.extend(fact_record(f).map(|v| v as i32));
        n += 1;
    }
    n
}

/// Adds a tactic from [`sx_tactic_new`] or a shipped tactic ([`sx_tactic_rush`] ...); the squad
/// takes it over (do not use or free it again). Returns its index.
///
/// # Safety
/// `s` from [`sx_squad_new`]; `t` from [`sx_tactic_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_tactic(s: *mut WasmSquad, t: *mut WasmTactic) -> i32 {
    let w = squad!(s);
    if t.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above; the box is taken back exactly once.
    let t = unsafe { Box::from_raw(t) };
    to_i32(w.squad.add_tactic(t.0))
}

/// `ERROR_PLAN` (-3) with the tactic and column in the out buffer.
fn plan_error(w: &mut WasmSquad, e: PlanError) -> i32 {
    plan_error_to(&mut w.out, e)
}

/// `ERROR_PLAN` (-3) with the tactic and column in `out`.
fn plan_error_to(out: &mut Vec<i32>, e: PlanError) -> i32 {
    // No column: -1 in the out buffer.
    let (code, tactic, input) = convert::plan_error(e);
    out.clear();
    // `u32::MAX` (no tactic or column) is -1.
    out.extend([tactic as i32, input as i32]);
    code
}

/// Checks the tactics' columns. Returns 0, or `ERROR_PLAN` with the
/// tactic's index and the column in the out buffer.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_validate(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    match w.squad.validate() {
        Ok(()) => 0,
        Err(e) => plan_error(w, e),
    }
}

/// The extra numbers and spots in the scratch buffer: `extra_count`
/// numbers, then `spot_count` spot records (4 numbers each).
fn plan_input(extra_count: u32, spot_count: u32) -> Option<(Vec<i32>, Vec<Spot>)> {
    let extra = extra_count as usize;
    let len = (spot_count as usize)
        .checked_mul(SPOT_SIZE)?
        .checked_add(extra)?;
    let r = scratch_copy(len)?;
    let spots = r[extra..]
        .as_chunks::<SPOT_SIZE>()
        .0
        .iter()
        .map(|c| Spot {
            pos: [c[0], c[1], c[2]],
            tags: c[3] as u32,
        })
        .collect();
    Some((r[..extra].to_vec(), spots))
}

/// Makes a new plan (see `sx_squad_plan`) from the extra numbers and spots
/// in the scratch buffer (see `plan_input`); the seed comes in two
/// halves. Returns 1 with the plan's id in the out buffer, 0 if nothing
/// scores, `ERROR_PLAN` (details in the out buffer) or -1.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_plan(
    s: *mut WasmSquad,
    now: u32,
    extra_count: u32,
    spot_count: u32,
    seed_low: u32,
    seed_high: u32,
) -> i32 {
    let w = squad!(s);
    let Some((extra, spots)) = plan_input(extra_count, spot_count) else {
        return ERROR_ARGUMENT;
    };
    let seed = u64::from(seed_low) | (u64::from(seed_high) << 32);
    w.out.clear();
    match w.squad.plan(now, &extra, &spots, seed) {
        Ok(Some(id)) => {
            w.out.push(id as i32);
            1
        }
        Ok(None) => 0,
        Err(e) => plan_error(w, e),
    }
}

/// Watches the plans at `now` (see `sx_squad_check`), with the same
/// inputs as [`sx_squad_plan`]. Returns 0 keep, 1 replan (the new plan's
/// id in the out buffer), 2 signal, 3 no plan, `ERROR_PLAN` or -1.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_check(
    s: *mut WasmSquad,
    now: u32,
    extra_count: u32,
    spot_count: u32,
    seed_low: u32,
    seed_high: u32,
) -> i32 {
    let w = squad!(s);
    let Some((extra, spots)) = plan_input(extra_count, spot_count) else {
        return ERROR_ARGUMENT;
    };
    let seed = u64::from(seed_low) | (u64::from(seed_high) << 32);
    w.out.clear();
    match w
        .squad
        .check(now, &extra, &spots, seed)
        .map(convert::squad_status)
    {
        Ok((code, id)) => {
            if let Some(id) = id {
                w.out.push(id as i32);
            }
            code
        }
        Err(e) => plan_error(w, e),
    }
}

/// Moves at most `max` of the orders whose time has come to the out buffer
/// as order records (21 numbers each), in the order they were sent; the
/// rest wait for the next call. Returns how many.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_orders_due(s: *mut WasmSquad, now: u32, max: u32) -> i32 {
    let w = squad!(s);
    // A crew takes the orders of its own squad (sx_crew_tick).
    if w.squad.is_lent() {
        return ERROR_ARGUMENT;
    }
    w.squad.orders_due(now, &mut w.due);
    // Orders held back from an earlier call may be stale by now: drop those
    // of another plan and of members no longer active.
    let squad = &w.squad;
    let plan = squad.active_plan().map(|p| p.id);
    w.due.retain(|o| {
        let active = squad
            .group()
            .index(o.member)
            .and_then(|i| squad.group().get(i))
            .is_some_and(|m| m.state == MemberState::Active);
        active && Some(o.goal.plan) == plan
    });
    let n = w.due.len().min(max as usize);
    w.out.clear();
    for o in w.due.drain(..n) {
        put_order(&mut w.out, o.member, &o.goal);
    }
    to_i32(n)
}

/// Gives the signal of the active plan; 1, or 0 if there is no active plan
/// or the signal was given.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_signal(s: *mut WasmSquad, now: u32) -> i32 {
    i32::from(squad!(s).squad.signal(now))
}

/// The member reached its first spot; 1, or 0 if it has no goal in the
/// active plan.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_arrived(s: *mut WasmSquad, id: u32, now: u32) -> i32 {
    i32::from(squad!(s).squad.arrived(id, now))
}

/// 1 if the signal of the active plan was given.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_signal_given(s: *mut WasmSquad) -> i32 {
    i32::from(squad!(s).squad.signal_given())
}

fn write_plan(out: &mut Vec<i32>, plan: Option<&Plan>) -> i32 {
    out.clear();
    let Some(p) = plan else {
        return 0;
    };
    out.extend([
        p.id as i32,
        to_i32(p.tactic),
        p.target.unwrap_or(0) as i32,
        p.made_at as i32,
        p.active_at as i32,
        to_i32(p.goals.len()),
        i32::from(p.target.is_some()),
    ]);
    for (member, goal) in &p.goals {
        put_order(out, *member, goal);
    }
    1
}

/// The plan made last while it waits: a plan record (7 numbers) and its
/// goals as order records to the out buffer. Returns 1, or 0 if none.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_pending_plan(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    write_plan(&mut w.out, w.squad.pending_plan())
}

/// Like [`sx_squad_pending_plan`], for the plan being carried out.
///
/// # Safety
/// `s` from [`sx_squad_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_active_plan(s: *mut WasmSquad) -> i32 {
    let w = squad!(s);
    write_plan(&mut w.out, w.squad.active_plan())
}

/// One member's goal with its postpone state, and an out buffer.
pub struct WasmKeeper {
    keeper: Held<GoalKeeper>,
    out: Vec<i32>,
}

macro_rules! keeper {
    ($k:expr) => {
        // SAFETY: JS passes a handle from sx_keeper_new or sx_crew_keeper
        // (wrapper guarantees it); a stale lent one is refused here.
        match unsafe { $k.as_mut() } {
            Some(k) if k.keeper.is_live() => k,
            _ => return ERROR_ARGUMENT,
        }
    };
}

/// A keeper without a goal.
#[unsafe(no_mangle)]
pub extern "C" fn sx_keeper_new() -> *mut WasmKeeper {
    Box::into_raw(Box::new(WasmKeeper {
        keeper: Held::Own(GoalKeeper::new()),
        out: Vec::new(),
    }))
}

/// Frees a keeper; does nothing for a keeper lent by a crew (the crew
/// frees it).
///
/// # Safety
/// `k` is null, from [`sx_keeper_new`] and not used again, or from
/// [`sx_crew_keeper`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_free(k: *mut WasmKeeper) {
    // SAFETY: null or valid (see above).
    match unsafe { k.as_ref() } {
        None => {}
        Some(w) if w.keeper.is_lent() => {}
        // SAFETY: from sx_keeper_new (see above).
        Some(_) => drop(unsafe { Box::from_raw(k) }),
    }
}

/// 1 if the keeper can be used: its own, or lent by a crew that did not
/// change since and whose group still holds the member; else 0.
///
/// # Safety
/// `k` is null, from [`sx_keeper_new`] or from [`sx_crew_keeper`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_live(k: *mut WasmKeeper) -> i32 {
    // SAFETY: see above.
    i32::from(unsafe { k.as_ref() }.is_some_and(|w| w.keeper.is_live()))
}

/// The out buffer of the last call that wrote one.
///
/// # Safety
/// `k` from [`sx_keeper_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_out(k: *mut WasmKeeper) -> *const i32 {
    // SAFETY: see above.
    unsafe { k.as_ref() }.map_or(std::ptr::null(), |w| w.out.as_ptr())
}

/// Takes the goal record (20 numbers) in the scratch buffer. Returns 0; -1
/// for an unknown kind or start or a slot above 255.
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_set(k: *mut WasmKeeper, now: u32) -> i32 {
    let w = keeper!(k);
    let Some(goal) = scratch_copy(GOAL_SIZE).as_deref().and_then(goal_of) else {
        return ERROR_ARGUMENT;
    };
    w.keeper.set(now, goal);
    0
}

/// The goal record to the out buffer; 1, or 0 without a goal.
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_goal(k: *mut WasmKeeper) -> i32 {
    let w = keeper!(k);
    w.out.clear();
    match w.keeper.goal() {
        Some(g) => {
            w.out.extend_from_slice(&goal_record(g));
            1
        }
        None => 0,
    }
}

/// Drops the goal and the postpone state.
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_clear(k: *mut WasmKeeper) -> i32 {
    keeper!(k).keeper.clear();
    0
}

/// The member turns to something else for now (the first time stays).
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_goal_postponed(k: *mut WasmKeeper, now: u32) -> i32 {
    keeper!(k).keeper.goal_postponed(now);
    0
}

/// Back on the goal: the postpone clock is reset.
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_goal_won(k: *mut WasmKeeper, now: u32) -> i32 {
    keeper!(k).keeper.goal_won(now);
    0
}

/// Out of play: never broken.
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_suspend(k: *mut WasmKeeper) -> i32 {
    keeper!(k).keeper.suspend();
    0
}

/// Back in play; a postponed goal's clock restarts at `now`.
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_unsuspend(k: *mut WasmKeeper, now: u32) -> i32 {
    keeper!(k).keeper.unsuspend(now);
    0
}

/// 1 once the goal has been postponed for at least `give_up_ticks`, never
/// while suspended.
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_broken(k: *mut WasmKeeper, now: u32, give_up_ticks: u32) -> i32 {
    i32::from(keeper!(k).keeper.broken(now, give_up_ticks))
}

/// The member's decider inputs at `x, y, z` (has goal, distance to goal,
/// goal age, signal given, discipline) to the out buffer. Returns 0; -1 for
/// an unknown metric.
///
/// # Safety
/// `k` from [`sx_keeper_new`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_keeper_inputs(
    k: *mut WasmKeeper,
    now: u32,
    x: i32,
    y: i32,
    z: i32,
    metric: i32,
    signal_given: u32,
    discipline: u32,
) -> i32 {
    let w = keeper!(k);
    let Some(metric) = u32_of(metric).and_then(convert::metric) else {
        return ERROR_ARGUMENT;
    };
    let inputs = w
        .keeper
        .inputs(now, [x, y, z], metric, signal_given != 0, discipline);
    w.out.clear();
    w.out.extend_from_slice(&inputs);
    0
}

/// Adds the keeper's ready-made goal option to decider `d`. Returns its
/// number, or -1.
///
/// # Safety
/// `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_add_goal_option(d: *mut WasmDecider, weight: u32) -> i32 {
    to_i32(GoalKeeper::add_goal_option(
        &mut decider!(d, ERROR_ARGUMENT).decider,
        weight,
    ))
}

/// Tells decider `d` that the goal option `option` is the action going on
/// while the keeper holds a goal that is not postponed. Returns 0, or -1.
///
/// # Safety
/// `k` from [`sx_keeper_new`]; `d` from [`sx_decider_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_set_current(
    k: *mut WasmKeeper,
    d: *mut WasmDecider,
    option: u32,
    discipline: u32,
) -> i32 {
    let w = keeper!(k);
    w.keeper
        .set_current(&mut decider!(d, ERROR_ARGUMENT).decider, option, discipline);
    0
}

/// The tick the goal record (20 numbers) in the scratch buffer starts at,
/// for an order that arrived at `received`: 1 with the tick in `scratch[0]`
/// (as `u32` bits), 0 for a goal that starts on the signal, -1 for a bad
/// goal.
#[unsafe(no_mangle)]
pub extern "C" fn sx_goal_start_tick(received: u32) -> i32 {
    let Some(goal) = scratch_copy(GOAL_SIZE).as_deref().and_then(goal_of) else {
        return ERROR_ARGUMENT;
    };
    match goal.start_tick(received) {
        Some(tick) => {
            scratch_put(&[tick as i32]);
            1
        }
        None => 0,
    }
}

/// 1 if the goal record (20 numbers) in the scratch buffer awaits its
/// arrival report, else 0; -1 for a bad goal.
#[unsafe(no_mangle)]
pub extern "C" fn sx_goal_awaits_arrival() -> i32 {
    scratch_copy(GOAL_SIZE)
        .as_deref()
        .and_then(goal_of)
        .map_or(ERROR_ARGUMENT, |g| i32::from(g.awaits_arrival()))
}

mod crew;
pub use crew::*;
mod values;
pub use values::*;
