//! `sx_goal_guess_*`: goal recognition, where the one an observer watches
//! is heading, see `sagax.h`.
//!
//! One handle per observer holds the guess (a fixed-size value), its trace
//! and the reused memory for the moves of a step (`observe_flow`); nothing
//! allocates after `sx_goal_guess_new`.

use std::ffi::c_void;

use sagax_abi::convert;
use sagax_decide::memory::{
    GOAL_COLUMNS, GoalGuess, GoalGuessConfig, GoalGuessTrace, MAX_GOALS, MAX_MOVES, StepError,
};
use sagax_facade::goal_guess::observe_flow_traced;
use sagax_nav::Cost;
use sagax_nav::flow::FlowField;
use sagax_nav::grid::Cell;

use super::memory::put;
use super::{
    CallbackMap, ERROR_ARGUMENT, Point, SxFlow, SxMap, TraceSlot, create, destroy, guarded,
    set_trace, slice_of, to_i32,
};

/// `sx_goal_guess_config`.
pub type SxGoalGuessConfig = GoalGuessConfig;

/// `sx_goal_guess_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxGoalGuessTrace {
    /// `SX_GOAL_GUESS_TRACE_*`.
    pub kind: u32,
    /// `STEP`, `PRIOR`: the goal.
    pub goal: u32,
    /// `STEP`: the change of walking distance of the step taken.
    pub moved: i32,
    /// `STEP`: the step's likelihood for the goal, 1/65536.
    pub likelihood: u32,
    /// `STEP`, `PRIOR`: the goal's chance now, 0..256.
    pub chance: u32,
    /// `REFUSED`: `SX_STEP_*`.
    pub error: u32,
}

impl SxGoalGuessTrace {
    /// The record of a goal-guess trace.
    pub fn of(event: GoalGuessTrace) -> Self {
        let c = convert::goal_guess_trace(event);
        Self {
            kind: c.kind,
            goal: c.goal,
            moved: c.moved,
            likelihood: c.likelihood,
            chance: c.chance,
            error: c.error,
        }
    }
}

/// `sx_goal_guess_trace_fn`.
pub type GoalGuessTraceFn = unsafe extern "C" fn(*mut c_void, *const SxGoalGuessTrace);

/// `sx_goal_guess`: one observer's guess, its trace and the moves memory.
pub struct SxGoalGuess {
    guess: GoalGuess,
    trace: TraceSlot<SxGoalGuessTrace>,
    moves: Vec<(Cell, Cost)>,
}

/// The handle behind `g` (mutable), or `ERROR_ARGUMENT`.
macro_rules! handle {
    ($g:expr) => {
        // SAFETY: the caller passes null or a valid handle (sagax.h).
        match unsafe { $g.as_mut() } {
            Some(h) => h,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// The handle behind `g` (shared), or `ERROR_ARGUMENT`.
macro_rules! view {
    ($g:expr) => {
        // SAFETY: the caller passes null or a valid handle (sagax.h).
        match unsafe { $g.as_ref() } {
            Some(h) => h,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// `sx_goal_guess_config_default`: beta 512, unit 10, forget 13.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_config_default(out: *mut SxGoalGuessConfig) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { put(out, GoalGuessConfig::default()) };
    0
}

/// `sx_goal_guess_new`: a guess over `goals` (1..`SX_MAX_GOALS`) goals
/// with a uniform prior; null for a null config or a goal count out of
/// range.
///
/// # Safety
/// `cfg` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_new(
    cfg: *const SxGoalGuessConfig,
    goals: u32,
) -> *mut SxGoalGuess {
    create(|| {
        // SAFETY: null or valid (see above).
        let config = *unsafe { cfg.as_ref() }?;
        let goals = usize::try_from(goals)
            .ok()
            .filter(|n| (1..=MAX_GOALS).contains(n))?;
        Some(SxGoalGuess {
            guess: GoalGuess::new(config, goals),
            trace: None,
            moves: Vec::with_capacity(MAX_MOVES),
        })
    })
}

/// `sx_goal_guess_free`.
///
/// # Safety
/// `g` is null or from [`sx_goal_guess_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_free(g: *mut SxGoalGuess) {
    // SAFETY: see above.
    unsafe { destroy(g) }
}

/// `sx_goal_guess_set_trace`.
///
/// # Safety
/// `g` is null or valid; `trace` and `context` stay valid while tracing is
/// on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_set_trace(
    g: *mut SxGoalGuess,
    trace: Option<GoalGuessTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { g.as_mut() }.map(|h| &mut h.trace), trace, context)
}

/// `sx_goal_guess_goals`: the number of goals, or `SX_ERROR_ARGUMENT`.
///
/// # Safety
/// `g` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_goals(g: *const SxGoalGuess) -> i32 {
    to_i32(view!(g).guess.goals())
}

/// `sx_goal_guess_set_prior`: `count` weights (missing ones 0, extra
/// ones ignored; all 0 is uniform), and start over from them. Returns 0, or
/// `SX_ERROR_ARGUMENT` for a null handle or a null `weights` with `count`
/// above 0.
///
/// # Safety
/// `g` is null or valid; `weights` holds `count` integers (null allowed
/// for 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_set_prior(
    g: *mut SxGoalGuess,
    weights: *const u32,
    count: u32,
) -> i32 {
    let h = handle!(g);
    if count > 0 && weights.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: `count` integers (see above).
    let weights = unsafe { slice_of(weights, count as usize) };
    guarded(|| {
        let guess = &mut h.guess;
        traced!(h.trace, SxGoalGuessTrace::of, |sink| guess
            .set_prior_traced(weights, sink));
        0
    })
}

/// `sx_goal_guess_reset`: back to the prior. Returns 0, or
/// `SX_ERROR_ARGUMENT` for a null handle.
///
/// # Safety
/// `g` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_reset(g: *mut SxGoalGuess) -> i32 {
    let h = handle!(g);
    let guess = &mut h.guess;
    traced!(h.trace, SxGoalGuessTrace::of, |sink| guess
        .reset_traced(sink));
    0
}

/// `sx_goal_guess_observe`: one observed step, `moved` (one per goal) and
/// `options` (goal-major, `per_goal` per goal). Returns 0, or
/// `SX_ERROR_ARGUMENT` for a null pointer or `per_goal` 0 or above
/// `SX_MAX_MOVES` (then nothing changed).
///
/// # Safety
/// `g` is null or valid; `moved` holds the goal count and `options` goal
/// count times `per_goal` integers (or are null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_observe(
    g: *mut SxGoalGuess,
    moved: *const i32,
    options: *const i32,
    per_goal: u32,
) -> i32 {
    let h = handle!(g);
    let n = h.guess.goals();
    let per = per_goal as usize;
    if moved.is_null() || options.is_null() || per == 0 || per > MAX_MOVES {
        // A refused step is still traced, as in Rust.
        let guess = &mut h.guess;
        let _ = traced!(h.trace, SxGoalGuessTrace::of, |sink| guess.observe_traced(
            &[],
            &[],
            0,
            sink
        ));
        return ERROR_ARGUMENT;
    }
    // SAFETY: the counts the caller promised (see above).
    let (moved, options) = unsafe { (slice_of(moved, n), slice_of(options, n * per)) };
    guarded(|| {
        let guess = &mut h.guess;
        match traced!(h.trace, SxGoalGuessTrace::of, |sink| guess
            .observe_traced(moved, options, per, sink))
        {
            Ok(()) => 0,
            Err(_) => ERROR_ARGUMENT,
        }
    })
}

/// `sx_goal_guess_observe_flow`: the seen step `prev -> cur` with one flow
/// field per goal (`flows[k]` for goal `k`, `count` the goal count); the
/// moves of `prev` come from `map` with the rules of `flows[0]`. Returns 0
/// (learned, or `cur == prev`: nothing to learn), 1 for a jump (`cur` is
/// not a move from `prev`; nothing changed), or `SX_ERROR_ARGUMENT` for a
/// null pointer, `count` not the goal count, or more than `SX_MAX_MOVES`
/// moves.
///
/// # Safety
/// `g` is null or valid; `flows` holds `count` pointers, each null or a
/// valid flow; `map` is null or valid with callbacks valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_observe_flow(
    g: *mut SxGoalGuess,
    flows: *const *const SxFlow,
    count: u32,
    map: *const SxMap,
    prev: Point,
    cur: Point,
) -> i32 {
    let h = handle!(g);
    let Some(mut map) = CallbackMap::new(map) else {
        return ERROR_ARGUMENT;
    };
    let n = count as usize;
    if flows.is_null() || n != h.guess.goals() || n == 0 || n > MAX_GOALS {
        return ERROR_ARGUMENT;
    }
    // SAFETY: `count` pointers (see above).
    let flows = unsafe { slice_of(flows, n) };
    let mut handles: [Option<&SxFlow>; MAX_GOALS] = [None; MAX_GOALS];
    for (slot, &f) in handles.iter_mut().zip(flows) {
        // SAFETY: null or a valid flow (see above).
        *slot = unsafe { f.as_ref() };
    }
    if handles[..n].iter().any(Option::is_none) {
        return ERROR_ARGUMENT;
    }
    let Some(first) = handles[0] else {
        return ERROR_ARGUMENT;
    };
    let refs: [&FlowField<Cell>; MAX_GOALS] =
        std::array::from_fn(|k| &handles[k].unwrap_or(first).field);
    guarded(|| {
        let mut graph = first.topology.graph(&mut map, None);
        let guess = &mut h.guess;
        let moves = &mut h.moves;
        let done = traced!(h.trace, SxGoalGuessTrace::of, |sink| observe_flow_traced(
            guess,
            &mut graph,
            &refs[..n],
            prev.cell(),
            cur.cell(),
            moves,
            sink
        ));
        match done {
            Ok(()) => 0,
            Err(StepError::Jump) => 1,
            Err(StepError::Shape) => ERROR_ARGUMENT,
        }
    })
}

/// `sx_goal_guess_chance`: the chance of `goal`, 0..256 (0 out of range;
/// all goals sum to 256), or `SX_ERROR_ARGUMENT`.
///
/// # Safety
/// `g` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_chance(g: *const SxGoalGuess, goal: u32) -> i32 {
    to_i32(view!(g).guess.chance(goal as usize))
}

/// `sx_goal_guess_chance_fine`: the chance of `goal` in 1/65536, or
/// `SX_ERROR_ARGUMENT`.
///
/// # Safety
/// `g` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_chance_fine(g: *const SxGoalGuess, goal: u32) -> i32 {
    to_i32(view!(g).guess.chance_fine(goal as usize))
}

/// `sx_goal_guess_best`: the most likely goal (ties: the lower), its
/// chance (0..256) to `chance` unless null; or `SX_ERROR_ARGUMENT`.
///
/// # Safety
/// `g` is null or valid; `chance` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_best(g: *const SxGoalGuess, chance: *mut u32) -> i32 {
    let h = view!(g);
    let Some((goal, c)) = h.guess.best() else {
        return ERROR_ARGUMENT;
    };
    if !chance.is_null() {
        // SAFETY: not null and writable (see above).
        unsafe { put(chance, c) };
    }
    to_i32(goal)
}

/// `sx_goal_guess_rank`: the rank of `goal` (0 the most likely; the goal
/// count out of range), or `SX_ERROR_ARGUMENT`.
///
/// # Safety
/// `g` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_rank(g: *const SxGoalGuess, goal: u32) -> i32 {
    to_i32(view!(g).guess.rank(goal as usize))
}

/// `sx_goal_guess_write_columns`: `chance, rank` of `goal` as
/// `SX_GOAL_COLUMNS` integers to `row`. Returns 0, or `SX_ERROR_ARGUMENT`.
///
/// # Safety
/// `g` is null or valid; `row` is null or has room for
/// `SX_GOAL_COLUMNS` integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_write_columns(
    g: *const SxGoalGuess,
    goal: u32,
    row: *mut i32,
) -> i32 {
    let h = view!(g);
    if row.is_null() {
        return ERROR_ARGUMENT;
    }
    let mut cols = [0; GOAL_COLUMNS];
    h.guess.write_columns(goal as usize, &mut cols);
    for (i, v) in cols.into_iter().enumerate() {
        // SAFETY: room for `SX_GOAL_COLUMNS` (see above).
        unsafe { row.add(i).write(v) };
    }
    0
}

/// `sx_goal_guess_split`: `hunters` shared over the goals by chance to
/// `out[0..count]` (entries past the goals 0). Returns 0, or
/// `SX_ERROR_ARGUMENT` for a null handle or a null `out` with `count`
/// above 0.
///
/// # Safety
/// `g` is null or valid; `out` has room for `count` integers (null allowed
/// for 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_guess_split(
    g: *const SxGoalGuess,
    hunters: u32,
    out: *mut u32,
    count: u32,
) -> i32 {
    let h = view!(g);
    if count > 0 && out.is_null() {
        return ERROR_ARGUMENT;
    }
    let mut all = [0u32; MAX_GOALS];
    h.guess.split(hunters, &mut all);
    for i in 0..count as usize {
        // SAFETY: room for `count` (see above).
        unsafe { out.add(i).write(all.get(i).copied().unwrap_or(0)) };
    }
    0
}
