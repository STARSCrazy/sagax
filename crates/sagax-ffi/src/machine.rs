//! `sx_machine_*`: state machine with timers, see `sagax.h`.

use sagax_decide::state::{ANY, Change, Reason, StateMachine, Transition};

use super::{ERROR_ARGUMENT, create, destroy, to_i32};

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
    /// The C form of a change.
    pub fn of(c: Change) -> Self {
        let (reason, transition) = match c.reason {
            Reason::Transition(i) => (1, to_i32(i)),
            Reason::Timeout => (2, -1),
            Reason::Set => (3, -1),
        };
        Self {
            from: to_i32(c.from),
            to: to_i32(c.to),
            reason,
            transition,
        }
    }
}

/// `sx_machine`.
pub struct SxMachine(StateMachine);

/// `sx_machine_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_machine_new(states: u32, initial: u32) -> *mut SxMachine {
    create(|| StateMachine::new(states, initial).ok().map(SxMachine))
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
            Some(m) => &mut m.0,
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
    let m = machine!(machine);
    let c = m.update(now, flags);
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
}
