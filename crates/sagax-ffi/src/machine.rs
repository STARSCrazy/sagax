//! `sx_machine_*`: state machine with timers, see `sagax.h`.

use std::ffi::c_void;

use sagax_abi::convert::{self, ChangeCode};
use sagax_decide::state::{ANY, Change, StateMachine, StateTrace, Transition};

use super::{ERROR_ARGUMENT, TraceSlot, create, destroy, set_trace, to_i32};

/// `sx_change`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxChange {
    /// The state left.
    pub from: i32,
    /// The state entered.
    pub to: i32,
    /// `SX_REASON_*`.
    pub reason: i32,
    /// The transition's index, or -1.
    pub transition: i32,
}

impl SxChange {
    fn of_code(c: ChangeCode) -> Self {
        Self {
            from: c.from,
            to: c.to,
            reason: c.reason,
            transition: c.transition,
        }
    }

    /// The C form of a change.
    pub fn of(c: Change) -> Self {
        Self::of_code(convert::change(c))
    }
}

/// `sx_machine_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMachineTrace {
    /// `SX_MACHINE_TRACE_*`.
    pub kind: u32,
    /// `CHANGED`: the change.
    pub change: SxChange,
    /// `WAITING`: the transition's index.
    pub transition: u32,
    /// `WAITING`: how long the state has run.
    pub elapsed: u32,
    /// `WAITING`: how long it must run first.
    pub after: u32,
}

impl SxMachineTrace {
    /// The record of a state machine trace.
    pub fn of(event: StateTrace) -> Self {
        let c = convert::state_trace(event);
        Self {
            kind: c.kind,
            change: SxChange::of_code(c.change),
            transition: c.transition,
            elapsed: c.elapsed,
            after: c.after,
        }
    }
}

/// `sx_machine_trace_fn`.
pub type MachineTraceFn = unsafe extern "C" fn(*mut c_void, *const SxMachineTrace);

/// `sx_machine`.
pub struct SxMachine {
    machine: StateMachine,
    trace: TraceSlot<SxMachineTrace>,
}

/// `sx_machine_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_machine_new(states: u32, initial: u32) -> *mut SxMachine {
    create(|| {
        StateMachine::new(states, initial)
            .ok()
            .map(|machine| SxMachine {
                machine,
                trace: None,
            })
    })
}

/// `sx_machine_free`.
///
/// # Safety
/// `machine` is null or from [`sx_machine_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_free(machine: *mut SxMachine) {
    // SAFETY: see above.
    unsafe { destroy(machine) }
}

macro_rules! machine {
    ($m:expr) => {
        // SAFETY: the caller passes null or a valid machine (sagax.h).
        match unsafe { $m.as_mut() } {
            Some(m) => &mut m.machine,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// `sx_machine_set_timeout`.
///
/// # Safety
/// `machine` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_set_timeout(
    machine: *mut SxMachine,
    state: u32,
    time: u32,
    next: u32,
) -> i32 {
    match machine!(machine).set_timeout(state, time, next) {
        Ok(()) => 0,
        Err(_) => ERROR_ARGUMENT,
    }
}

/// `sx_machine_add_transition`.
///
/// # Safety
/// `machine` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_add_transition(
    machine: *mut SxMachine,
    from: i32,
    to: u32,
    when: u64,
    unless: u64,
    after: u32,
) -> i32 {
    let m = machine!(machine);
    let from = if from < 0 { ANY } else { from as u32 };
    let t = Transition::new(from, to, when).unless(unless).after(after);
    match m.add_transition(t) {
        Ok(i) => to_i32(i),
        Err(_) => ERROR_ARGUMENT,
    }
}

/// `sx_machine_update`.
///
/// # Safety
/// `machine` is null or valid; `change` is null or points to one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_update(
    machine: *mut SxMachine,
    now: u32,
    flags: u64,
    change: *mut SxChange,
) -> i32 {
    // SAFETY: the caller passes null or a valid machine (sagax.h).
    let Some(SxMachine { machine: m, trace }) = (unsafe { machine.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    let c = traced!(*trace, SxMachineTrace::of, |sink| m
        .update_traced(now, flags, sink));
    if !change.is_null() {
        let value = c.map_or(
            SxChange {
                from: to_i32(m.state()),
                to: to_i32(m.state()),
                reason: 0,
                transition: -1,
            },
            SxChange::of,
        );
        // SAFETY: see above.
        unsafe { change.write(value) };
    }
    i32::from(c.is_some())
}

/// `sx_machine_state`.
///
/// # Safety
/// `machine` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_state(machine: *mut SxMachine) -> i32 {
    to_i32(machine!(machine).state())
}

/// `sx_machine_entered`.
///
/// # Safety
/// `machine` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_entered(machine: *mut SxMachine, entered: *mut u32) -> i32 {
    let m = machine!(machine);
    if entered.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { entered.write(m.entered()) };
    0
}

/// `sx_machine_set_state`.
///
/// # Safety
/// `machine` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_set_state(
    machine: *mut SxMachine,
    state: u32,
    now: u32,
) -> i32 {
    match machine!(machine).set_state(state, now) {
        Ok(_) => 0,
        Err(_) => ERROR_ARGUMENT,
    }
}

/// `sx_machine_set_trace`.
///
/// # Safety
/// `machine` is null or valid; `trace` and `context` stay valid while
/// tracing is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_machine_set_trace(
    machine: *mut SxMachine,
    trace: Option<MachineTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(
        unsafe { machine.as_mut() }.map(|m| &mut m.trace),
        trace,
        context,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_through_c() {
        unsafe {
            assert!(sx_machine_new(2, 2).is_null());
            let m = sx_machine_new(3, 0);
            assert_eq!(sx_machine_add_transition(m, -1, 2, 2, 0, 0), 0);
            assert_eq!(sx_machine_add_transition(m, 0, 1, 1, 0, 0), 1);
            assert_eq!(sx_machine_add_transition(m, 0, 9, 1, 0, 0), ERROR_ARGUMENT);
            assert_eq!(sx_machine_set_timeout(m, 1, 5, 0), 0);
            let mut c = SxChange::default();
            assert_eq!(sx_machine_update(m, 1, 0, &mut c), 0);
            assert_eq!(sx_machine_update(m, 2, 1, &mut c), 1);
            assert_eq!((c.from, c.to, c.reason, c.transition), (0, 1, 1, 1));
            assert_eq!(sx_machine_update(m, 7, 0, &mut c), 1);
            assert_eq!((c.to, c.reason), (0, 2));
            let mut at = 0;
            assert_eq!(sx_machine_entered(m, &mut at), 0);
            assert_eq!(at, 7);
            assert_eq!(sx_machine_update(m, 8, 3, std::ptr::null_mut()), 1);
            assert_eq!(sx_machine_state(m), 2);
            assert_eq!(sx_machine_set_state(m, 1, 9), 0);
            assert_eq!(sx_machine_set_state(m, 5, 9), ERROR_ARGUMENT);
            assert_eq!(sx_machine_state(std::ptr::null_mut()), ERROR_ARGUMENT);
            sx_machine_free(m);
        }
    }

    unsafe extern "C" fn collect(ctx: *mut c_void, t: *const SxMachineTrace) {
        let records = unsafe { &mut *ctx.cast::<Vec<SxMachineTrace>>() };
        records.push(unsafe { *t });
    }

    #[test]
    fn machine_trace_through_c() {
        use sagax_abi::codes::{MACHINE_TRACE_CHANGED, MACHINE_TRACE_WAITING, REASON_TRANSITION};

        unsafe {
            let m = sx_machine_new(2, 0);
            assert_eq!(sx_machine_add_transition(m, 0, 1, 1, 0, 5), 0);
            let mut records: Vec<SxMachineTrace> = Vec::new();
            let ctx = (&raw mut records).cast();
            assert_eq!(sx_machine_set_trace(m, Some(collect), ctx), 0);
            assert_eq!(
                sx_machine_set_trace(std::ptr::null_mut(), Some(collect), ctx),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_machine_update(m, 3, 1, std::ptr::null_mut()), 0);
            let mut c = SxChange::default();
            assert_eq!(sx_machine_update(m, 5, 1, &mut c), 1);
            let waiting = SxMachineTrace {
                kind: MACHINE_TRACE_WAITING,
                transition: 0,
                elapsed: 3,
                after: 5,
                ..SxMachineTrace::default()
            };
            let changed = SxMachineTrace {
                kind: MACHINE_TRACE_CHANGED,
                change: SxChange {
                    from: 0,
                    to: 1,
                    reason: REASON_TRANSITION,
                    transition: 0,
                },
                ..SxMachineTrace::default()
            };
            assert_eq!(records, [waiting, changed]);
            assert_eq!(c, changed.change);
            // Off: nothing more arrives.
            assert_eq!(sx_machine_set_trace(m, None, std::ptr::null_mut()), 0);
            assert_eq!(sx_machine_set_state(m, 0, 10), 0);
            assert_eq!(sx_machine_update(m, 20, 1, std::ptr::null_mut()), 1);
            assert_eq!(records.len(), 2);
            sx_machine_free(m);
        }
    }
}
