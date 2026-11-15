//! `sx_decider_*`: decisions (utility AI), see `sagax.h`.

use std::ffi::c_void;

use sagax_abi::codes::ALL_OPTIONS;
use sagax_abi::convert::{self, CombineTarget, DecideTraceCode};
use sagax_decide::{Apply, Candidate, Consideration, Curve, DecideTrace, Decider, Pick};

use super::group::Held;
use super::{ERROR_ARGUMENT, TraceSlot, create, destroy, guarded, set_trace, slice_of, to_i32};

/// `sx_consideration`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxConsideration {
    /// Index into the candidate's row of inputs.
    pub input: u32,
    /// `SX_CURVE_*`.
    pub curve: u32,
    /// Lower end.
    pub min: i32,
    /// Upper end.
    pub max: i32,
}

/// `sx_candidate`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxCandidate {
    /// Option number.
    pub option: u32,
    /// The game's target number.
    pub target: u32,
}

/// `sx_choice`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxChoice {
    /// Index of the chosen candidate, -1 for none.
    pub candidate: i32,
    /// Its score.
    pub score: u32,
}

/// `sx_decide_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxDecideTrace {
    /// `SX_DECIDE_TRACE_*`.
    pub kind: u32,
    /// `FACTOR`, `SCORED`, `CHOSEN`: index of the candidate.
    pub candidate: i32,
    /// `FACTOR`: index of the consideration.
    pub consideration: i32,
    /// `FACTOR`: the raw input.
    pub input: i32,
    /// `FACTOR`: the factor.
    pub value: u32,
    /// `SCORED`, `CHOSEN`: the score.
    pub score: u32,
    /// `SCORED`: the product before the weight.
    pub product: u32,
    /// `RANKED`: the rank that takes part.
    pub rank: u32,
    /// `SCORED`: 1 if a cooldown forced the score to 0.
    pub cooled: u8,
}

impl SxDecideTrace {
    pub(crate) fn of_code(c: DecideTraceCode) -> Self {
        Self {
            kind: c.kind,
            candidate: c.candidate,
            consideration: c.consideration,
            input: c.input,
            value: c.value,
            score: c.score,
            product: c.product,
            rank: c.rank,
            cooled: u8::from(c.cooled),
        }
    }

    /// The record of a decision trace.
    pub fn of(event: DecideTrace) -> Self {
        Self::of_code(convert::decide_trace(event))
    }
}

/// `sx_decide_trace_fn`.
pub type DecideTraceFn = unsafe extern "C" fn(*mut c_void, *const SxDecideTrace);

/// `sx_decider`.
pub struct SxDecider {
    decider: Held<Decider>,
    candidates: Vec<Candidate>,
    scores: Vec<u32>,
    trace: TraceSlot<SxDecideTrace>,
}

impl SxDecider {
    /// The decider behind the handle.
    pub(crate) fn decider_mut(&mut self) -> &mut Decider {
        &mut self.decider
    }

    /// A handle on a decider held elsewhere (the squad's), with its own
    /// scratch buffers.
    pub(crate) fn held(decider: Held<Decider>) -> Self {
        Self {
            decider,
            candidates: Vec::new(),
            scores: Vec::new(),
            trace: None,
        }
    }
}

/// `sx_decider_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_decider_new() -> *mut SxDecider {
    create(|| {
        Some(SxDecider {
            decider: Held::Own(Decider::new()),
            candidates: Vec::new(),
            scores: Vec::new(),
            trace: None,
        })
    })
}

/// `sx_decider_free`: does nothing for a decider lent by `sx_squad_decider`
/// or `sx_mission_decider`.
///
/// # Safety
/// `decider` is null, from [`sx_decider_new`] and not used again, or lent
/// by `sx_squad_decider` or `sx_mission_decider`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_free(decider: *mut SxDecider) {
    // SAFETY: null or valid (see above).
    if unsafe { decider.as_ref() }.is_some_and(|d| d.decider.is_lent()) {
        return;
    }
    // SAFETY: see above.
    unsafe { destroy(decider) }
}

/// `sx_decider_add_option`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_add_option(decider: *mut SxDecider, weight: u32) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| to_i32(d.decider.add_option(weight)))
}

/// `sx_decider_add_consideration`.
///
/// # Safety
/// `decider` is null or valid; `consideration` is null or points to one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_add_consideration(
    decider: *mut SxDecider,
    option: u32,
    consideration: *const SxConsideration,
) -> i32 {
    // SAFETY: see above.
    let (Some(d), Some(c)) = (unsafe { decider.as_mut() }, unsafe {
        consideration.as_ref()
    }) else {
        return ERROR_ARGUMENT;
    };
    let Some(curve) = Curve::from_u32(c.curve) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        let added = d
            .decider
            .add_consideration(option, Consideration::new(c.input, curve, c.min, c.max));
        if added { 0 } else { ERROR_ARGUMENT }
    })
}

/// `sx_decider_add_weighted`.
///
/// # Safety
/// As [`sx_decider_add_consideration`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_add_weighted(
    decider: *mut SxDecider,
    option: u32,
    consideration: *const SxConsideration,
    pull: u32,
    apply: u32,
) -> i32 {
    // SAFETY: see above.
    let (Some(d), Some(c)) = (unsafe { decider.as_mut() }, unsafe {
        consideration.as_ref()
    }) else {
        return ERROR_ARGUMENT;
    };
    let (Some(curve), Some(apply)) = (Curve::from_u32(c.curve), Apply::from_u32(apply)) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        let c = Consideration::new(c.input, curve, c.min, c.max);
        if d.decider.add_weighted(option, c, pull, apply) {
            0
        } else {
            ERROR_ARGUMENT
        }
    })
}

/// `sx_decider_set_combine`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_combine(
    decider: *mut SxDecider,
    option: u32,
    combine: u32,
) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    let target = if option == ALL_OPTIONS {
        CombineTarget::All
    } else {
        CombineTarget::Option(option)
    };
    if convert::set_combine(&mut d.decider, target, combine) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// `sx_decider_set_rank`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_rank(
    decider: *mut SxDecider,
    option: u32,
    rank: u32,
) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    if d.decider.set_rank(option, rank) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// `sx_decider_set_current`.
///
/// # Safety
/// `decider` is null or valid; `current` is null or points to one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_current(
    decider: *mut SxDecider,
    current: *const SxCandidate,
    bonus: u32,
) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: see above.
    let current = unsafe { current.as_ref() }.map(|c| Candidate::new(c.option, c.target));
    d.decider.set_current(current, bonus);
    0
}

/// `sx_decider_set_now`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_now(decider: *mut SxDecider, now: u32) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    d.decider.set_now(now);
    0
}

/// `sx_decider_record`.
///
/// # Safety
/// `decider` is null or valid; `done` is null or points to one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_record(
    decider: *mut SxDecider,
    now: u32,
    done: *const SxCandidate,
) -> i32 {
    // SAFETY: see above.
    let (Some(d), Some(c)) = (unsafe { decider.as_mut() }, unsafe { done.as_ref() }) else {
        return ERROR_ARGUMENT;
    };
    d.decider.record(now, Candidate::new(c.option, c.target));
    0
}

/// `sx_decider_forget`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_forget(decider: *mut SxDecider) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    d.decider.forget();
    0
}

/// `sx_decider_set_window`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_window(decider: *mut SxDecider, time: u32) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    d.decider.set_window(time);
    0
}

/// `sx_decider_set_cooldown`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_cooldown(
    decider: *mut SxDecider,
    option: u32,
    time: u32,
) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    if d.decider.set_cooldown(option, time) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// `sx_decider_decide`.
///
/// # Safety
/// `decider` is null or valid; `candidates` holds `count` candidates,
/// `inputs` `count * stride` numbers, `scores` is null or has room for
/// `count`, `choice` is null or points to one.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_decider_decide(
    decider: *mut SxDecider,
    candidates: *const SxCandidate,
    count: i32,
    inputs: *const i32,
    stride: i32,
    pick: u32,
    top: u32,
    seed: u64,
    scores: *mut u32,
    choice: *mut SxChoice,
) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    let (Ok(count), Ok(stride), Some(pick)) = (
        usize::try_from(count),
        usize::try_from(stride),
        Pick::from_u32(pick, top),
    ) else {
        return ERROR_ARGUMENT;
    };
    let Some(numbers) = count.checked_mul(stride) else {
        return ERROR_ARGUMENT;
    };
    if (count > 0 && candidates.is_null()) || (numbers > 0 && inputs.is_null()) {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let (given, inputs) = unsafe { (slice_of(candidates, count), slice_of(inputs, numbers)) };
    guarded(|| {
        d.candidates.clear();
        d.candidates
            .extend(given.iter().map(|c| Candidate::new(c.option, c.target)));
        let result = traced!(d.trace, SxDecideTrace::of, |sink| d.decider.decide_traced(
            &d.candidates,
            inputs,
            stride,
            pick,
            seed,
            &mut d.scores,
            sink,
        ));
        let Ok(chosen) = result else {
            return ERROR_ARGUMENT;
        };
        if !scores.is_null() {
            for (i, &s) in d.scores.iter().enumerate() {
                // SAFETY: room for `count` scores (see above).
                unsafe { scores.add(i).write(s) };
            }
        }
        if !choice.is_null() {
            let value = chosen.map_or(
                SxChoice {
                    candidate: -1,
                    score: 0,
                },
                |c| SxChoice {
                    candidate: to_i32(c.candidate),
                    score: c.score,
                },
            );
            // SAFETY: see above.
            unsafe { choice.write(value) };
        }
        i32::from(chosen.is_some())
    })
}

/// `sx_decider_set_trace`.
///
/// # Safety
/// `decider` is null or valid; `trace` and `context` stay valid while
/// tracing is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_trace(
    decider: *mut SxDecider,
    trace: Option<DecideTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(
        unsafe { decider.as_mut() }.map(|d| &mut d.trace),
        trace,
        context,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn collect(ctx: *mut c_void, e: *const SxDecideTrace) {
        let events = unsafe { &mut *ctx.cast::<Vec<SxDecideTrace>>() };
        events.push(unsafe { *e });
    }

    #[test]
    fn decide_through_c() {
        unsafe {
            let d = sx_decider_new();
            assert_eq!(sx_decider_add_option(d, 65535), 0);
            assert_eq!(sx_decider_add_option(d, 65535), 1);
            let near = SxConsideration {
                input: 0,
                curve: 1,
                min: 1,
                max: 10,
            };
            assert_eq!(sx_decider_add_consideration(d, 0, &near), 0);
            assert_eq!(sx_decider_add_consideration(d, 7, &near), ERROR_ARGUMENT);
            let bad = SxConsideration { curve: 99, ..near };
            assert_eq!(sx_decider_add_consideration(d, 0, &bad), ERROR_ARGUMENT);
            let mut events: Vec<SxDecideTrace> = Vec::new();
            sx_decider_set_trace(d, Some(collect), (&raw mut events).cast());

            let candidates = [
                SxCandidate {
                    option: 0,
                    target: 4,
                },
                SxCandidate {
                    option: 0,
                    target: 5,
                },
            ];
            let inputs = [8, 2];
            let mut scores = [0u32; 2];
            let mut choice = SxChoice::default();
            let r = sx_decider_decide(
                d,
                candidates.as_ptr(),
                2,
                inputs.as_ptr(),
                1,
                0,
                0,
                0,
                scores.as_mut_ptr(),
                &mut choice,
            );
            assert_eq!(r, 1);
            assert_eq!(choice.candidate, 1);
            assert!(scores[1] > scores[0]);
            assert_eq!(events.len(), 5);
            assert_eq!(events[4].kind, sagax_abi::codes::DECIDE_TRACE_CHOSEN);

            // Out of reach: nothing chosen.
            let far = [20, 30];
            let r = sx_decider_decide(
                d,
                candidates.as_ptr(),
                2,
                far.as_ptr(),
                1,
                1,
                0,
                9,
                std::ptr::null_mut(),
                &mut choice,
            );
            assert_eq!((r, choice.candidate), (0, -1));
            // Too short a row.
            let r = sx_decider_decide(
                d,
                candidates.as_ptr(),
                2,
                inputs.as_ptr(),
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert_eq!(r, ERROR_ARGUMENT);
            assert_eq!(
                sx_decider_add_option(std::ptr::null_mut(), 1),
                ERROR_ARGUMENT
            );

            // Stage 1b: a true factor, own combine, rank, current action.
            sx_decider_set_trace(d, None, std::ptr::null_mut());
            let gate = SxConsideration {
                input: 0,
                curve: 4,
                min: 5,
                max: 0,
            };
            assert_eq!(sx_decider_add_weighted(d, 1, &gate, 65535, 1), 0);
            assert_eq!(
                sx_decider_add_weighted(d, 1, &gate, 65535, 7),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_decider_set_combine(d, u32::MAX, 1), 0);
            assert_eq!(sx_decider_set_combine(d, 0, 2), 0);
            assert_eq!(sx_decider_set_combine(d, 0, u32::MAX), 0);
            assert_eq!(sx_decider_set_combine(d, 0, 9), ERROR_ARGUMENT);
            assert_eq!(sx_decider_set_rank(d, 1, 1), 0);
            assert_eq!(sx_decider_set_rank(d, 5, 1), ERROR_ARGUMENT);
            let both = [
                SxCandidate {
                    option: 0,
                    target: 0,
                },
                SxCandidate {
                    option: 1,
                    target: 0,
                },
            ];
            // input 9: option 1 passes its gate and wins by rank
            let r = sx_decider_decide(
                d,
                both.as_ptr(),
                2,
                [9, 9].as_ptr(),
                1,
                0,
                0,
                0,
                std::ptr::null_mut(),
                &mut choice,
            );
            assert_eq!((r, choice.candidate), (1, 1));
            assert_eq!(sx_decider_set_current(d, &both[1], 6553), 0);
            assert_eq!(sx_decider_set_current(d, std::ptr::null(), 0), 0);

            // Stage 2: memory and cooldown.
            assert_eq!(sx_decider_set_now(d, 10), 0);
            assert_eq!(sx_decider_record(d, 10, &both[1]), 0);
            assert_eq!(sx_decider_record(d, 10, std::ptr::null()), ERROR_ARGUMENT);
            assert_eq!(sx_decider_set_cooldown(d, 1, 3), 0);
            assert_eq!(sx_decider_set_cooldown(d, 8, 3), ERROR_ARGUMENT);
            assert_eq!(sx_decider_set_window(d, 5), 0);
            let r = sx_decider_decide(
                d,
                both.as_ptr(),
                2,
                [9, 9].as_ptr(),
                1,
                0,
                0,
                0,
                std::ptr::null_mut(),
                &mut choice,
            );
            // option 1 cools down, so option 0 is chosen
            assert_eq!(r, 1);
            assert_eq!(choice.candidate, 0);
            assert_eq!(sx_decider_forget(d), 0);
            sx_decider_free(d);
        }
    }

    #[test]
    fn decide_trace_through_c_matches_rust() {
        use sagax_abi::codes::{DECIDE_TRACE_RANKED, DECIDE_TRACE_SCORED};
        use sagax_decide::ONE;

        unsafe {
            let d = sx_decider_new();
            let mut r = Decider::new();
            for (weight, rank, cooldown) in [(ONE, 0, 0), (ONE, 0, 3), (ONE / 10, 1, 0)] {
                let o = sx_decider_add_option(d, weight) as u32;
                assert_eq!(sx_decider_set_rank(d, o, rank), 0);
                assert_eq!(sx_decider_set_cooldown(d, o, cooldown), 0);
                let o = r.add_option(weight);
                assert!(r.set_rank(o, rank));
                assert!(r.set_cooldown(o, cooldown));
            }
            let done = SxCandidate {
                option: 1,
                target: 0,
            };
            assert_eq!(sx_decider_set_now(d, 10), 0);
            assert_eq!(sx_decider_record(d, 9, &done), 0);
            r.set_now(10);
            r.record(9, Candidate::new(1, 0));
            let mut records: Vec<SxDecideTrace> = Vec::new();
            sx_decider_set_trace(d, Some(collect), (&raw mut records).cast());
            let cands: Vec<SxCandidate> = (0..3)
                .map(|option| SxCandidate { option, target: 0 })
                .collect();
            let mut choice = SxChoice::default();
            let n = sx_decider_decide(
                d,
                cands.as_ptr(),
                3,
                std::ptr::null(),
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                &mut choice,
            );
            assert_eq!((n, choice.candidate), (1, 2));
            let mut expected = Vec::new();
            let rc: Vec<Candidate> = (0..3).map(|o| Candidate::new(o, 0)).collect();
            r.decide_traced(
                &rc,
                &[],
                0,
                Pick::Best,
                0,
                &mut Vec::new(),
                &mut sagax_core::CallbackSink(|e| expected.push(SxDecideTrace::of(e))),
            )
            .unwrap();
            assert_eq!(records, expected);
            assert_eq!(records[1].kind, DECIDE_TRACE_SCORED);
            assert_eq!((records[1].score, records[1].cooled), (0, 1));
            assert_eq!((records[3].kind, records[3].rank), (DECIDE_TRACE_RANKED, 1));
            sx_decider_free(d);
        }
    }
}
