//! `sx_contagion_*`: indirect coordination, the deeds a character saw its
//! allies do, see `sagax.h`.
//!
//! One handle per character holds its memory (fixed slots from
//! `sx_contagion_new`) and its trace. It reads only the `sx_percept`s the
//! game passes from that character's own look. Fine points (percept
//! points, the observer, `nearest_point`) are axial on hex and never
//! offset-converted; the handle needs no cells.

use std::ffi::c_void;

use sagax_abi::convert;
use sagax_facade::perception::{
    Contagion, ContagionConfig, ContagionTrace, DEED_COLUMNS, DeedInputs, DeedRule, Grid, MAX_DEEDS,
};

use super::memory::{pos3, put};
use super::perception::SxPercept;
use super::{ERROR_ARGUMENT, TraceSlot, create, destroy, guarded, set_trace, slice_of};

/// `sx_deed_rule`.
pub type SxDeedRule = DeedRule;
/// `sx_deed_inputs`.
pub type SxDeedInputs = DeedInputs;

/// `sx_is_ally_fn`: the game's belief, nonzero for "an ally".
pub type IsAllyFn = unsafe extern "C" fn(*mut c_void, u32, i32) -> i32;

/// `sx_contagion_config`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxContagionConfig {
    /// `SX_GRID_*` (every hex layout means hex).
    pub grid: u32,
    /// How many deeds are in use, at most `SX_MAX_DEEDS`.
    pub count: u32,
    /// The deeds.
    pub deeds: [SxDeedRule; MAX_DEEDS],
}

impl SxContagionConfig {
    /// The Rust config, `None` for an unknown grid or too many deeds.
    fn config(&self) -> Option<ContagionConfig> {
        let grid = match convert::grid_layout(self.grid)? {
            None => Grid::Square,
            Some(_) => Grid::Hex,
        };
        let count = u8::try_from(self.count)
            .ok()
            .filter(|&n| usize::from(n) <= MAX_DEEDS)?;
        Some(ContagionConfig {
            grid,
            deeds: self.deeds,
            count,
        })
    }
}

/// `sx_contagion_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SxContagionTrace {
    /// `SX_CONTAGION_TRACE_*`.
    pub kind: u32,
    /// The ally (`REPLACED`: the one that came).
    pub ally: u32,
    /// `REPLACED`: the ally that went.
    pub old: u32,
    /// `REFRESH`: the cue bits seen.
    pub bits: u32,
    /// `ONSET`, `EXPIRED`, `INPUTS`: the deed index.
    pub deed: u32,
    /// `INPUTS`: what was read (else the empty inputs).
    pub inputs: SxDeedInputs,
}

impl SxContagionTrace {
    /// The record of a contagion trace.
    pub fn of(event: ContagionTrace) -> Self {
        let c = convert::contagion_trace(event);
        Self {
            kind: c.kind,
            ally: c.ally,
            old: c.old,
            bits: c.bits,
            deed: c.deed,
            inputs: c.inputs,
        }
    }
}

/// `sx_contagion_trace_fn`.
pub type ContagionTraceFn = unsafe extern "C" fn(*mut c_void, *const SxContagionTrace);

/// `sx_contagion`: one character's memory and its trace.
pub struct SxContagion {
    memory: Contagion,
    trace: TraceSlot<SxContagionTrace>,
}

/// The handle behind `c` (mutable), or `ERROR_ARGUMENT`.
macro_rules! handle {
    ($c:expr) => {
        // SAFETY: the caller passes null or a valid handle (sagax.h).
        match unsafe { $c.as_mut() } {
            Some(h) => h,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// The handle behind `c` (shared), or `ERROR_ARGUMENT`.
macro_rules! view {
    ($c:expr) => {
        // SAFETY: the caller passes null or a valid handle (sagax.h).
        match unsafe { $c.as_ref() } {
            Some(h) => h,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// Reads the observer's fine point, or `ERROR_ARGUMENT` for null.
macro_rules! observer {
    ($p:expr) => {{
        if $p.is_null() {
            return ERROR_ARGUMENT;
        }
        // SAFETY: not null; 3 integers (sagax.h).
        unsafe { pos3($p) }
    }};
}

/// `sx_contagion_new`: a memory for `capacity` allies; null for a null or
/// invalid config (unknown grid, `count` above `SX_MAX_DEEDS`).
///
/// # Safety
/// `cfg` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_new(
    cfg: *const SxContagionConfig,
    capacity: u32,
) -> *mut SxContagion {
    create(|| {
        // SAFETY: null or valid (see above).
        let config = unsafe { cfg.as_ref() }?.config()?;
        Some(SxContagion {
            memory: Contagion::new(config, capacity as usize),
            trace: None,
        })
    })
}

/// `sx_contagion_free`.
///
/// # Safety
/// `c` is null or from [`sx_contagion_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_free(c: *mut SxContagion) {
    // SAFETY: see above.
    unsafe { destroy(c) }
}

/// `sx_contagion_set_trace`.
///
/// # Safety
/// `c` is null or valid; `trace` and `context` stay valid while tracing is
/// on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_set_trace(
    c: *mut SxContagion,
    trace: Option<ContagionTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { c.as_mut() }.map(|h| &mut h.trace), trace, context)
}

/// `sx_contagion_learn`: `count` percepts of this character at `now`, with
/// the game's `is_ally(context, subject, disguised)`. Returns 0, or
/// `SX_ERROR_ARGUMENT` for a null handle or `is_ally`, a null `pc` with
/// `count` above 0, or a percept with an unknown sense or event (then
/// nothing is changed).
///
/// # Safety
/// `c` is null or valid; `pc` holds `count` percepts (null allowed for 0);
/// `is_ally` and `context` are valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_learn(
    c: *mut SxContagion,
    now: u32,
    pc: *const SxPercept,
    count: u32,
    is_ally: Option<IsAllyFn>,
    context: *mut c_void,
) -> i32 {
    let h = handle!(c);
    let Some(is_ally) = is_ally else {
        return ERROR_ARGUMENT;
    };
    if count > 0 && pc.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: `count` percepts (see above).
    let pc = unsafe { slice_of(pc, count as usize) };
    if pc.iter().any(|p| p.percept().is_none()) {
        return ERROR_ARGUMENT;
    }
    // SAFETY: the game's callback and context, valid for the call.
    let ally = |subject: u32, disguised: bool| unsafe {
        is_ally(context, subject, i32::from(disguised)) != 0
    };
    guarded(|| {
        let memory = &mut h.memory;
        if pc.is_empty() {
            traced!(h.trace, SxContagionTrace::of, |sink| memory.learn_traced(
                &[],
                now,
                ally,
                sink
            ));
        }
        // One percept at a time (no buffer); expiry is the same for each.
        for p in pc.iter().filter_map(SxPercept::percept) {
            traced!(h.trace, SxContagionTrace::of, |sink| memory.learn_traced(
                &[p],
                now,
                ally,
                sink
            ));
        }
        0
    })
}

/// `sx_contagion_inputs`: the inputs of deed `deed` for an observer at
/// `observer` (a fine point, 3 integers) at `now`; the empty inputs for a
/// deed not in use. Returns 0, or `SX_ERROR_ARGUMENT` for a null pointer.
///
/// # Safety
/// `c` is null or valid; `observer` is null or 3 integers; `out` is null
/// or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_inputs(
    c: *const SxContagion,
    deed: u32,
    observer: *const i32,
    now: u32,
    out: *mut SxDeedInputs,
) -> i32 {
    let h = view!(c);
    let observer = observer!(observer);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        let inputs = traced!(h.trace, SxContagionTrace::of, |sink| h
            .memory
            .inputs_traced(deed as usize, observer, now, sink));
        // SAFETY: not null and writable (see above).
        unsafe { put(out, inputs) };
        0
    })
}

/// `sx_contagion_write_columns`: the inputs as `SX_DEED_COLUMNS` integers
/// `doing, seen, share, nearest, since` (each at most `INT32_MAX`) to
/// `row`. Returns 0, or `SX_ERROR_ARGUMENT` for a null pointer.
///
/// # Safety
/// `c` is null or valid; `observer` is null or 3 integers; `row` is null
/// or has room for `SX_DEED_COLUMNS` integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_write_columns(
    c: *const SxContagion,
    deed: u32,
    observer: *const i32,
    now: u32,
    row: *mut i32,
) -> i32 {
    let h = view!(c);
    let observer = observer!(observer);
    if row.is_null() {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        let mut cols = [0; DEED_COLUMNS];
        traced!(h.trace, SxContagionTrace::of, |sink| h
            .memory
            .write_columns_traced(
                deed as usize,
                observer,
                now,
                &mut cols,
                sink
            ));
        for (i, v) in cols.into_iter().enumerate() {
            // SAFETY: room for `SX_DEED_COLUMNS` (see above).
            unsafe { row.add(i).write(v) };
        }
        0
    })
}

/// `sx_contagion_nearest_point`: where the nearest ally doing deed `deed`
/// was last seen (a fine point, 3 integers to `out`). Returns 1, 0 when
/// none (`out` unchanged), or `SX_ERROR_ARGUMENT` for a null pointer.
///
/// # Safety
/// `c` is null or valid; `observer` is null or 3 integers; `out` is null
/// or has room for 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_nearest_point(
    c: *const SxContagion,
    deed: u32,
    observer: *const i32,
    now: u32,
    out: *mut i32,
) -> i32 {
    let h = view!(c);
    let observer = observer!(observer);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    guarded(
        || match h.memory.nearest_point(deed as usize, observer, now) {
            Some(p) => {
                for (i, v) in p.into_iter().enumerate() {
                    // SAFETY: room for 3 integers (see above).
                    unsafe { out.add(i).write(v) };
                }
                1
            }
            None => 0,
        },
    )
}

/// `sx_contagion_forget`: forgets `ally`. Returns 0, or
/// `SX_ERROR_ARGUMENT` for a null handle.
///
/// # Safety
/// `c` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_forget(c: *mut SxContagion, ally: u32) -> i32 {
    handle!(c).memory.forget(ally);
    0
}

/// `sx_contagion_clear`: forgets everyone. Returns 0, or
/// `SX_ERROR_ARGUMENT` for a null handle.
///
/// # Safety
/// `c` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_clear(c: *mut SxContagion) -> i32 {
    handle!(c).memory.clear();
    0
}

/// `sx_contagion_len`: how many allies it remembers, or
/// `SX_ERROR_ARGUMENT` for a null handle.
///
/// # Safety
/// `c` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_contagion_len(c: *const SxContagion) -> i32 {
    super::to_i32(view!(c).memory.len())
}
