//! `sx_decider_*`: decisions (utility AI), see `sagax.h`.

use std::ffi::c_void;

use sagax_core::CallbackSink;
use sagax_decide::{Apply, Candidate, Combine, Consideration, Curve, DecideEvent, Decider, Pick};

use super::{ERROR_ARGUMENT, create, destroy, guarded, slice_of, to_i32};

const FACTOR: i32 = 1;
const SCORED: i32 = 2;
const CHOSEN: i32 = 3;

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

/// `sx_decide_event`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxDecideEvent {
    /// `SX_DECIDE_FACTOR`, `SX_DECIDE_SCORED`, `SX_DECIDE_CHOSEN`.
    pub kind: i32,
    /// Index of the candidate.
    pub candidate: i32,
    /// Factor: index of the consideration; otherwise -1.
    pub consideration: i32,
    /// Factor: the raw input; otherwise 0.
    pub input: i32,
    /// Factor: the factor; scored: the product before the weight.
    pub value: u32,
    /// Scored and chosen: the score.
    pub score: u32,
}

impl SxDecideEvent {
    fn of(e: DecideEvent) -> Self {
        match e {
            DecideEvent::Factor {
                candidate,
                consideration,
                input,
                value,
            } => Self {
                kind: FACTOR,
                candidate: to_i32(candidate),
                consideration: to_i32(consideration),
                input,
                value,
                score: 0,
            },
            DecideEvent::Scored {
                candidate,
                product,
                score,
            } => Self {
                kind: SCORED,
                candidate: to_i32(candidate),
                consideration: -1,
                input: 0,
                value: product,
                score,
            },
            DecideEvent::Chosen { candidate, score } => Self {
                kind: CHOSEN,
                candidate: to_i32(candidate),
                consideration: -1,
                input: 0,
                value: 0,
                score,
            },
        }
    }
}

/// `sx_decide_trace_fn`.
pub type DecideTraceFn = unsafe extern "C" fn(*mut c_void, *const SxDecideEvent);

/// `sx_decider`.
pub struct SxDecider {
    decider: Decider,
    candidates: Vec<Candidate>,
    scores: Vec<u32>,
    trace: Option<(DecideTraceFn, *mut c_void)>,
}

/// `sx_decider_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_decider_new() -> *mut SxDecider {
    create(|| {
        Some(SxDecider {
            decider: Decider::new(),
            candidates: Vec::new(),
            scores: Vec::new(),
            trace: None,
        })
    })
}

/// `sx_decider_free`.
///
/// # Safety
/// `decider` is null or from [`sx_decider_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_free(decider: *mut SxDecider) {
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

/// Shared by C and WebAssembly: option -1 for all, combine -1 to follow.
pub(crate) fn set_combine(decider: &mut Decider, option: i32, combine: i32) -> i32 {
    let how = u32::try_from(combine).ok().and_then(Combine::from_u32);
    match (u32::try_from(option), how) {
        (Err(_), Some(how)) => {
            decider.set_combine(how);
            0
        }
        (Ok(option), how)
            if (how.is_some() || combine == -1) && decider.set_option_combine(option, how) =>
        {
            0
        }
        _ => ERROR_ARGUMENT,
    }
}

/// `sx_decider_set_combine`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_decider_set_combine(
    decider: *mut SxDecider,
    option: i32,
    combine: i32,
) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    set_combine(&mut d.decider, option, combine)
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
        let result = match d.trace {
            None => d
                .decider
                .decide(&d.candidates, inputs, stride, pick, seed, &mut d.scores),
            Some((f, context)) => d.decider.decide_traced(
                &d.candidates,
                inputs,
                stride,
                pick,
                seed,
                &mut d.scores,
                &mut CallbackSink(|e| {
                    let event = SxDecideEvent::of(e);
                    // SAFETY: the game keeps callback and context valid
                    // while tracing is on (sagax.h).
                    unsafe { f(context, &event) };
                }),
            ),
        };
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
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    d.trace = trace.map(|f| (f, context));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn collect(ctx: *mut c_void, e: *const SxDecideEvent) {
        let events = unsafe { &mut *ctx.cast::<Vec<SxDecideEvent>>() };
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
            let mut events: Vec<SxDecideEvent> = Vec::new();
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
            assert_eq!(events[4].kind, CHOSEN);

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
            assert_eq!(sx_decider_set_combine(d, -1, 1), 0);
            assert_eq!(sx_decider_set_combine(d, 0, 2), 0);
            assert_eq!(sx_decider_set_combine(d, 0, -1), 0);
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
            sx_decider_free(d);
        }
    }
}
