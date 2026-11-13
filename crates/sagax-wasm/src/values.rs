//! Stateless value exports: `sx_progress_*`, `sx_interval_*`, `sx_alertness_*`
//! and `sx_arrival`. JavaScript keeps `Progress`, `Interval` and `Alertness`
//! as plain objects; their methods write the record into the scratch buffer
//! ([`sx_scratch`](super::sx_scratch)), call one of these and read the record
//! back, so the rules live in one place (`sagax-decide`), as in C.
//!
//! Records, one number (`u32` bit pattern) each:
//!
//! * progress: `best`, `since`, `started` (3 numbers);
//! * interval: `last`, `started` (2 numbers);
//! * alertness: `value`, `last`, `last_stimulus`, `level`, then the config:
//!   `max`, `decay`, `hold`, `hysteresis`, `count`, `thresholds` x 8 (17).
//!
//! `ptr` must be the pointer [`sx_scratch`](super::sx_scratch) gave for the
//! record; any other pointer, or a buffer that is too short, is refused with
//! -1 (`SX_ERROR_ARGUMENT`).

use sagax_decide::memory::{AlertConfig, Alertness, Interval, Progress, arrival};
use std::mem::{align_of, size_of};

use super::{ERROR_ARGUMENT, SCRATCH};

/// Numbers in a progress record.
const PROGRESS_WORDS: usize = 3;
/// Numbers in an interval record.
const INTERVAL_WORDS: usize = 2;
/// Numbers in an alertness record: the state, then the config.
const ALERTNESS_WORDS: usize = 4 + 13;
/// Numbers of the state part of an alertness record; the config starts there.
const ALERTNESS_STATE_WORDS: usize = 4;

// The in-place casts below rely on these sizes and alignments; a change to
// the structs in `sagax-decide` breaks the build here, not the records.
const _: () = {
    assert!(size_of::<Progress>() <= PROGRESS_WORDS * 4);
    assert!(align_of::<Progress>() <= 4);
    assert!(size_of::<Interval>() <= INTERVAL_WORDS * 4);
    assert!(align_of::<Interval>() <= 4);
    assert!(size_of::<Alertness>() == ALERTNESS_STATE_WORDS * 4);
    assert!(align_of::<Alertness>() <= 4);
    assert!(size_of::<AlertConfig>() == 13 * 4);
    assert!(align_of::<AlertConfig>() <= 4);
    assert!(ALERTNESS_WORDS * 4 >= size_of::<Alertness>() + size_of::<AlertConfig>());
};

/// Runs `f` on the record at the start of the scratch buffer, in place.
/// The record is a `repr(C)` struct of 4-byte words (and a trailing `u8`)
/// laid out as the module docs say. -1 if `ptr` is not the scratch buffer or
/// it holds fewer than `words` numbers.
fn with_record(ptr: *mut i32, words: usize, f: impl FnOnce(*mut i32) -> i32) -> i32 {
    SCRATCH.with(|s| {
        let mut s = s.borrow_mut();
        if ptr.is_null() || s.as_mut_ptr() != ptr || s.len() < words {
            return ERROR_ARGUMENT;
        }
        f(s.as_mut_ptr())
    })
}

/// `sx_progress_start`: starts the record at `value`, `now`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_progress_start(ptr: *mut i32, value: i32, now: u32) -> i32 {
    with_record(ptr, PROGRESS_WORDS, |p| {
        // SAFETY: `p` is the scratch buffer, 4-aligned and at least 3 numbers
        // long; `Progress` is `repr(C)` { i32, u32, u8 } (12 bytes, align 4)
        // and every bit pattern is valid for its fields.
        let progress = unsafe { &mut *p.cast::<Progress>() };
        progress.start(value, now);
        0
    })
}

/// `sx_progress_update`: 1 if `value` counted as progress, else 0.
#[unsafe(no_mangle)]
pub extern "C" fn sx_progress_update(ptr: *mut i32, value: i32, now: u32, min_gain: i32) -> i32 {
    with_record(ptr, PROGRESS_WORDS, |p| {
        // SAFETY: as in `sx_progress_start`.
        let progress = unsafe { &mut *p.cast::<Progress>() };
        i32::from(progress.update(value, now, min_gain))
    })
}

/// `sx_progress_stalled`: 1 if started and the last progress is `limit` old.
#[unsafe(no_mangle)]
pub extern "C" fn sx_progress_stalled(ptr: *mut i32, now: u32, limit: u32) -> i32 {
    with_record(ptr, PROGRESS_WORDS, |p| {
        // SAFETY: as in `sx_progress_start`.
        let progress = unsafe { &*p.cast::<Progress>() };
        i32::from(progress.stalled(now, limit))
    })
}

/// `sx_interval_due`: 1 if never marked, or `every` has passed since.
#[unsafe(no_mangle)]
pub extern "C" fn sx_interval_due(ptr: *mut i32, now: u32, every: u32) -> i32 {
    with_record(ptr, INTERVAL_WORDS, |p| {
        // SAFETY: the scratch buffer holds 2 numbers at 4-alignment;
        // `Interval` is `repr(C)` { u32, u8 } (8 bytes, align 4).
        let interval = unsafe { &*p.cast::<Interval>() };
        i32::from(interval.due(now, every))
    })
}

/// `sx_interval_mark`: records that it ran at `now`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_interval_mark(ptr: *mut i32, now: u32) -> i32 {
    with_record(ptr, INTERVAL_WORDS, |p| {
        // SAFETY: as in `sx_interval_due`.
        let interval = unsafe { &mut *p.cast::<Interval>() };
        interval.mark(now);
        0
    })
}

/// `sx_alertness_stimulus`: adds `amount` at `now` (state, then config, in
/// the record).
#[unsafe(no_mangle)]
pub extern "C" fn sx_alertness_stimulus(ptr: *mut i32, amount: u32, now: u32) -> i32 {
    with_record(ptr, ALERTNESS_WORDS, |p| {
        // SAFETY: the buffer holds 17 numbers at 4-alignment: an `Alertness`
        // (`repr(C)`, 16 bytes) and, right after it, an `AlertConfig`
        // (`repr(C)`, 13 `u32`s); the two do not overlap and every bit
        // pattern is valid for them.
        let (alertness, config) = unsafe {
            (
                &mut *p.cast::<Alertness>(),
                &*p.add(ALERTNESS_STATE_WORDS).cast::<AlertConfig>(),
            )
        };
        alertness.stimulus(amount, now, config);
        0
    })
}

/// `sx_alertness_update`: decays to `now` and sets the level; 1 if the level
/// changed (the new one is in the record), else 0.
#[unsafe(no_mangle)]
pub extern "C" fn sx_alertness_update(ptr: *mut i32, now: u32) -> i32 {
    with_record(ptr, ALERTNESS_WORDS, |p| {
        // SAFETY: as in `sx_alertness_stimulus`.
        let (alertness, config) = unsafe {
            (
                &mut *p.cast::<Alertness>(),
                &*p.add(ALERTNESS_STATE_WORDS).cast::<AlertConfig>(),
            )
        };
        i32::from(alertness.update(now, config).is_some())
    })
}

/// `sx_arrival`: `now + base + distance * per_unit`, wrapping.
#[unsafe(no_mangle)]
pub extern "C" fn sx_arrival(now: u32, base: u32, distance: u32, per_unit: u32) -> u32 {
    arrival(now, base, distance, per_unit)
}
