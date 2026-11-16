//! Walking distance for squads and crews, see `sagax.h`: the game's
//! `sx_distance_fn` set on a squad or crew handle
//! (`sx_squad_set_distance`, `sx_crew_set_distance`), and the ready
//! provider `sx_flow_distance_*` with its callback `sx_flow_distance_fn`.

use std::ffi::c_void;

use sagax_decide::group::{SquadRules, Walking};
use sagax_decide::memory::{Distance, Pos};
use sagax_facade::walk::FlowDistance;

use super::memory::pos3;
use super::{CallbackMap, ERROR_ARGUMENT, SxMap, SxRules, create, destroy, guarded, rules_of};

/// `sx_distance_fn`: 1 when `to` can be reached from `from` (3 integers
/// each) and `*out` holds the walking distance in cells; anything else is
/// unreachable.
pub type DistanceFn = unsafe extern "C" fn(*mut c_void, *const i32, *const i32, *mut u32) -> i32;

/// A walking distance a handle holds: the game's callback and context.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CDistance {
    f: DistanceFn,
    context: *mut c_void,
}

impl CDistance {
    /// The distance of a setter call; `None` (off) for a null callback.
    pub(crate) fn of(f: Option<DistanceFn>, context: *mut c_void) -> Option<Self> {
        f.map(|f| Self { f, context })
    }
}

impl Distance for CDistance {
    fn distance(&mut self, from: Pos, to: Pos) -> Option<u32> {
        let mut out = 0u32;
        // SAFETY: the game set a valid callback and context that stay valid
        // while set (sagax.h); the positions and `out` live through the
        // call.
        let reached = unsafe { (self.f)(self.context, from.as_ptr(), to.as_ptr(), &mut out) };
        (reached == 1).then_some(out)
    }
}

/// Runs `f` with `rules`, walking with `walk` when one is set.
pub(crate) fn with_walk<T>(
    walk: Option<CDistance>,
    rules: &mut dyn SquadRules,
    f: impl FnOnce(&mut dyn SquadRules) -> T,
) -> T {
    match walk {
        Some(mut d) => f(&mut Walking::with(rules, &mut d)),
        None => f(rules),
    }
}

// ---- sx_flow_distance -----------------------------------------------------

/// `sx_flow_distance`.
pub struct SxFlowDistance {
    fd: FlowDistance,
    map: Option<SxMap>,
}

/// `sx_flow_distance_new`: `slots` cached fields (0: 4), each of
/// `rules->node_capacity` cells; null for invalid rules.
///
/// # Safety
/// `rules` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_distance_new(
    rules: *const SxRules,
    slots: u32,
) -> *mut SxFlowDistance {
    create(|| {
        let (topology, capacity, _) = rules_of(rules)?;
        let slots = match slots {
            0 => FlowDistance::DEFAULT_SLOTS,
            n => n as usize,
        };
        Some(SxFlowDistance {
            fd: FlowDistance::new(topology, slots, capacity),
            map: None,
        })
    })
}

/// `sx_flow_distance_free`.
///
/// # Safety
/// `fd` is null or from [`sx_flow_distance_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_distance_free(fd: *mut SxFlowDistance) {
    // SAFETY: see above.
    unsafe { destroy(fd) }
}

/// `sx_flow_distance_set_map`: the map the fields are built on (copied;
/// its callbacks and context stay valid while it is set); null clears it.
///
/// # Safety
/// `fd` is null or valid; `map` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_distance_set_map(
    fd: *mut SxFlowDistance,
    map: *const SxMap,
) -> i32 {
    // SAFETY: null or valid (see above).
    let Some(h) = (unsafe { fd.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: null or valid (see above).
    let map = unsafe { map.as_ref() }.copied();
    if map.is_some_and(|m| m.step.is_none()) {
        return ERROR_ARGUMENT;
    }
    h.map = map;
    0
}

/// `sx_flow_distance_invalidate`: forgets every field.
///
/// # Safety
/// `fd` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_distance_invalidate(fd: *mut SxFlowDistance) -> i32 {
    // SAFETY: null or valid (see above).
    let Some(h) = (unsafe { fd.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    h.fd.invalidate();
    0
}

/// `sx_flow_distance_searches`: the fields built so far.
///
/// # Safety
/// `fd` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_distance_searches(fd: *const SxFlowDistance) -> i64 {
    // SAFETY: null or valid (see above).
    let Some(h) = (unsafe { fd.as_ref() }) else {
        return i64::from(ERROR_ARGUMENT);
    };
    i64::try_from(h.fd.searches()).unwrap_or(i64::MAX)
}

/// `sx_flow_distance_fn`: the walking distance of the `sx_flow_distance`
/// in `context` on its map, as an `sx_distance_fn`. 1 and `*out`, 0 when
/// unreachable, `SX_ERROR_ARGUMENT` for a null argument or no map.
///
/// # Safety
/// `context` is null or a valid `sx_flow_distance` whose map is valid;
/// `from` and `to` are null or hold 3 integers; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_flow_distance_fn(
    context: *mut c_void,
    from: *const i32,
    to: *const i32,
    out: *mut u32,
) -> i32 {
    // SAFETY: null or a valid handle (see above).
    let Some(h) = (unsafe { context.cast::<SxFlowDistance>().as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    if from.is_null() || to.is_null() || out.is_null() {
        return ERROR_ARGUMENT;
    }
    let Some(mut map) = h.map.as_ref().and_then(|m| CallbackMap::new(m)) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: 3 integers each (see above).
    let (from, to) = unsafe { (pos3(from), pos3(to)) };
    guarded(|| match h.fd.distance(&mut map, from, to) {
        Some(d) => {
            // SAFETY: non-null and writable (see above).
            unsafe { out.write(d) };
            1
        }
        None => 0,
    })
}
