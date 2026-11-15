//! `sx_knowledge_*`, `sx_outbox_*`, `sx_inbox_*`, `sx_areas_*`, `sx_timers_*`,
//! value structs and geometry helpers: world memory, see `sagax.h`.

use std::ffi::c_void;

use sagax_decide::memory::{
    AlertConfig, Alertness, Area, Areas, AreasTrace, Fact, Inbox, Interval, Knowledge,
    KnowledgeTrace, LOOK_OFFSETS, Message, MessageTrace, Outbox, PointPattern, Progress, Timers,
    TimersTrace, arrival, points_around, route, sine, sweep_heading,
};

use super::group::{Held, Inside};
use super::{ERROR_ARGUMENT, TraceSlot, create, destroy, set_trace, slice_of, to_i32};
use sagax_abi::codes::POSTED;
use sagax_abi::convert::{self, PATTERN_MAX, PatternCodes};
use sagax_nav::grid::Cell;

/// `sx_fact`.
pub type SxFact = Fact;
/// `sx_message`.
pub type SxMessage = Message;
/// `sx_area`.
pub type SxArea = Area;
/// `sx_progress`.
pub type SxProgress = Progress;
/// `sx_interval`.
pub type SxInterval = Interval;
/// `sx_alertness`.
pub type SxAlertness = Alertness;
/// `sx_alert_config`.
pub type SxAlertConfig = AlertConfig;

/// `sx_nearby` (candidate of [`sx_route`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxNearby {
    /// The game's id.
    pub id: u32,
    /// Place, x.
    pub x: i32,
    /// Place, y.
    pub y: i32,
}

/// `sx_point_pattern`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxPointPattern {
    /// Heading between neighbouring agents.
    pub heading_step: i32,
    /// Distance of step 0.
    pub first_distance: i32,
    /// Number of used entries in `offsets` and `distances` (at most 8).
    pub count: u32,
    /// Turn of each later step.
    pub offsets: [i32; PATTERN_MAX],
    /// Distance of each later step.
    pub distances: [i32; PATTERN_MAX],
    /// `SX_SINE_*`.
    pub sine: u32,
    /// Smallest allowed x.
    pub min_x: i32,
    /// Smallest allowed y.
    pub min_y: i32,
    /// Largest allowed x.
    pub max_x: i32,
    /// Largest allowed y.
    pub max_y: i32,
}

impl SxPointPattern {
    /// The pattern; `None` for more than 8 entries or an unknown sine.
    pub(crate) fn pattern(&self) -> Option<PointPattern> {
        convert::point_pattern(&PatternCodes {
            heading_step: self.heading_step,
            first_distance: self.first_distance,
            count: self.count,
            offsets: self.offsets,
            distances: self.distances,
            sine: self.sine,
            min: (self.min_x, self.min_y),
            max: (self.max_x, self.max_y),
        })
    }

    /// The C form of a pattern; `None` for more than 8 entries or
    /// `offsets` and `distances` of different lengths.
    pub(crate) fn of(p: &PointPattern) -> Option<Self> {
        let c = convert::pattern_codes(p)?;
        Some(Self {
            heading_step: c.heading_step,
            first_distance: c.first_distance,
            count: c.count,
            offsets: c.offsets,
            distances: c.distances,
            sine: c.sine,
            min_x: c.min.0,
            min_y: c.min.1,
            max_x: c.max.0,
            max_y: c.max.1,
        })
    }
}

fn flag(b: bool) -> i32 {
    i32::from(b)
}

/// A mutable reference to a handle or value struct, or `ERROR_ARGUMENT`.
macro_rules! get {
    ($p:expr) => {
        // SAFETY: the caller passes null or a valid pointer (sagax.h).
        match unsafe { $p.as_mut() } {
            Some(v) => v,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// A shared reference to an input struct, or `ERROR_ARGUMENT`.
macro_rules! input {
    ($p:expr) => {
        // SAFETY: the caller passes null or a valid pointer (sagax.h).
        match unsafe { $p.as_ref() } {
            Some(v) => v,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// Writes `value` to `out` unless `out` is null.
///
/// # Safety
/// `out` is null or writable.
pub(crate) unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        // SAFETY: non-null and writable (see above); `write` does not read
        // or drop the old, possibly uninitialised, value.
        unsafe { out.write(value) };
    }
}

/// Reads 3 integers.
///
/// # Safety
/// `p` points to 3 integers.
pub(crate) unsafe fn pos3(p: *const i32) -> [i32; 3] {
    // SAFETY: see above.
    unsafe { [*p, *p.add(1), *p.add(2)] }
}

/// Fills `out` from `moved` (at most `out_cap` entries); returns how many.
///
/// # Safety
/// `out` has room for `out_cap` messages (null allowed for 0).
unsafe fn write_messages(moved: &[Message], out: *mut Message, out_cap: u32) -> i32 {
    for (i, m) in moved.iter().take(out_cap as usize).enumerate() {
        // SAFETY: within `out_cap` (see above).
        unsafe { out.add(i).write(*m) };
    }
    to_i32(moved.len().min(out_cap as usize))
}

/// The value behind a knowledge or areas handle and its trace slot, or
/// `ERROR_ARGUMENT` (a null handle, or a lent one whose owner is stale).
macro_rules! held {
    ($p:expr) => {{
        let h = get!($p);
        match h.value.get_mut() {
            Some(v) => (v, h.trace),
            None => return ERROR_ARGUMENT,
        }
    }};
}

// ---- knowledge --------------------------------------------------------------

/// `sx_knowledge_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxKnowledgeTrace {
    /// `SX_KNOWLEDGE_TRACE_*`.
    pub kind: u32,
    /// The fact concerned.
    pub fact: SxFact,
}

impl SxKnowledgeTrace {
    /// The record of a knowledge trace.
    pub fn of(event: KnowledgeTrace) -> Self {
        let c = convert::knowledge_trace(event);
        Self {
            kind: c.kind,
            fact: c.fact,
        }
    }
}

/// `sx_knowledge_trace_fn`.
pub type KnowledgeTraceFn = unsafe extern "C" fn(*mut c_void, *const SxKnowledgeTrace);

/// `sx_knowledge`: its own table, or the one of a squad (`sx_squad_knowledge`).
pub struct SxKnowledge {
    pub(crate) value: Held<Knowledge>,
    pub(crate) trace: TraceSlot<SxKnowledgeTrace>,
}

impl SxKnowledge {
    /// A handle on the table inside another handle.
    pub(crate) fn inside(inside: Inside<Knowledge>) -> Self {
        Self {
            value: Held::Inside(inside),
            trace: None,
        }
    }
}

/// `sx_knowledge_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_knowledge_new(capacity: u32, evict: u32) -> *mut SxKnowledge {
    create(|| {
        let evict = convert::evict(evict)?;
        Knowledge::new(capacity as usize, evict)
            .ok()
            .map(|k| SxKnowledge {
                value: Held::Own(k),
                trace: None,
            })
    })
}

/// `sx_knowledge_free`: does nothing for the table lent by
/// `sx_squad_knowledge`.
///
/// # Safety
/// `k` is null, from [`sx_knowledge_new`] and not used again, or lent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_free(k: *mut SxKnowledge) {
    // SAFETY: null or valid (see above).
    if unsafe { k.as_ref() }.is_some_and(|k| k.value.is_lent()) {
        return;
    }
    // SAFETY: see above.
    unsafe { destroy(k) }
}

/// `sx_knowledge_set_trace`.
///
/// # Safety
/// `k` is null or valid; `trace` and `context` stay valid while tracing is
/// on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_set_trace(
    k: *mut SxKnowledge,
    trace: Option<KnowledgeTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { k.as_mut() }.map(|k| &mut k.trace), trace, context)
}

/// `sx_knowledge_learn`.
///
/// # Safety
/// `k` is null or valid; `fact` is null or valid; `evicted` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_learn(
    k: *mut SxKnowledge,
    now: u32,
    fact: *const SxFact,
    evicted: *mut SxFact,
) -> i32 {
    let (k, trace) = held!(k);
    let fact = input!(fact);
    let gone = traced!(trace, SxKnowledgeTrace::of, |sink| k
        .learn_traced(now, *fact, sink));
    match gone {
        Some(gone) => {
            // SAFETY: see above.
            unsafe { put(evicted, gone) };
            1
        }
        None => 0,
    }
}

/// `sx_knowledge_merge`.
///
/// # Safety
/// `k` is null or valid; `fact` is null or valid; `evicted` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_merge(
    k: *mut SxKnowledge,
    now: u32,
    fact: *const SxFact,
    evicted: *mut SxFact,
) -> i32 {
    let (k, trace) = held!(k);
    let fact = input!(fact);
    let gone = traced!(trace, SxKnowledgeTrace::of, |sink| k
        .merge_traced(now, *fact, sink));
    match gone {
        Some(gone) => {
            // SAFETY: see above.
            unsafe { put(evicted, gone) };
            1
        }
        None => 0,
    }
}

/// `sx_knowledge_learn_message`.
///
/// # Safety
/// `k` is null or valid; `msg` is null or valid; `evicted` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_learn_message(
    k: *mut SxKnowledge,
    now: u32,
    msg: *const SxMessage,
    source: u8,
    evicted: *mut SxFact,
) -> i32 {
    let (k, trace) = held!(k);
    let msg = input!(msg);
    let gone = traced!(trace, SxKnowledgeTrace::of, |sink| k
        .learn_message_traced(now, msg, source, sink));
    match gone {
        Some(gone) => {
            // SAFETY: see above.
            unsafe { put(evicted, gone) };
            1
        }
        None => 0,
    }
}

/// `sx_knowledge_merge_message`.
///
/// # Safety
/// `k` is null or valid; `msg` is null or valid; `evicted` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_merge_message(
    k: *mut SxKnowledge,
    now: u32,
    msg: *const SxMessage,
    source: u8,
    evicted: *mut SxFact,
) -> i32 {
    let (k, trace) = held!(k);
    let msg = input!(msg);
    let gone = traced!(trace, SxKnowledgeTrace::of, |sink| k
        .merge_message_traced(now, msg, source, sink));
    match gone {
        Some(gone) => {
            // SAFETY: see above.
            unsafe { put(evicted, gone) };
            1
        }
        None => 0,
    }
}

/// `sx_knowledge_touch`.
///
/// # Safety
/// `k` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_touch(
    k: *mut SxKnowledge,
    subject: u32,
    kind: u32,
    now: u32,
) -> i32 {
    flag(held!(k).0.touch(subject, kind, now))
}

/// `sx_knowledge_set_pos`.
///
/// # Safety
/// `k` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_set_pos(
    k: *mut SxKnowledge,
    subject: u32,
    kind: u32,
    x: i32,
    y: i32,
    z: i32,
) -> i32 {
    flag(held!(k).0.set_pos(subject, kind, [x, y, z]))
}

/// `sx_knowledge_set_data`.
///
/// # Safety
/// `k` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_set_data(
    k: *mut SxKnowledge,
    subject: u32,
    kind: u32,
    data: u32,
) -> i32 {
    flag(held!(k).0.set_data(subject, kind, data))
}

/// `sx_knowledge_set_strength`.
///
/// # Safety
/// `k` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_set_strength(
    k: *mut SxKnowledge,
    subject: u32,
    kind: u32,
    strength: u32,
) -> i32 {
    flag(held!(k).0.set_strength(subject, kind, strength))
}

/// `sx_knowledge_remove`.
///
/// # Safety
/// `k` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_remove(k: *mut SxKnowledge, subject: u32, kind: u32) -> i32 {
    let (k, trace) = held!(k);
    flag(traced!(trace, SxKnowledgeTrace::of, |sink| k
        .remove_traced(subject, kind, sink)))
}

/// `sx_knowledge_remove_subject`.
///
/// # Safety
/// `k` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_remove_subject(k: *mut SxKnowledge, subject: u32) -> i32 {
    let (k, trace) = held!(k);
    to_i32(traced!(trace, SxKnowledgeTrace::of, |sink| k
        .remove_subject_traced(subject, sink)))
}

/// `sx_knowledge_get`.
///
/// # Safety
/// `k` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_get(
    k: *mut SxKnowledge,
    subject: u32,
    kind: u32,
    out: *mut SxFact,
) -> i32 {
    let (k, _) = held!(k);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match k.get(subject, kind) {
        Some(found) => {
            // SAFETY: `out` is non-null and writable (see above).
            unsafe { put(out, *found) };
            1
        }
        None => 0,
    }
}

/// `sx_knowledge_count`.
///
/// # Safety
/// `k` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_count(k: *mut SxKnowledge) -> i32 {
    to_i32(held!(k).0.len())
}

/// `sx_knowledge_at`.
///
/// # Safety
/// `k` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_at(k: *mut SxKnowledge, index: u32, out: *mut SxFact) -> i32 {
    let (k, _) = held!(k);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match k.iter().nth(index as usize) {
        Some(found) => {
            // SAFETY: `out` is non-null and writable (see above).
            unsafe { put(out, *found) };
            1
        }
        None => 0,
    }
}

/// Writes the first `cap` of `gone` to `removed` (null allowed).
///
/// # Safety
/// `removed` is null or has room for `cap` facts.
unsafe fn write_facts(gone: &[Fact], removed: *mut SxFact, cap: u32) {
    if removed.is_null() {
        return;
    }
    for (i, f) in gone.iter().take(cap as usize).enumerate() {
        // SAFETY: within `cap` (see above).
        unsafe { removed.add(i).write(*f) };
    }
}

/// `sx_knowledge_expire`.
///
/// # Safety
/// `k` is null or valid; `removed` is null or has room for `cap` facts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_expire(
    k: *mut SxKnowledge,
    now: u32,
    max_age: u32,
    removed: *mut SxFact,
    cap: u32,
) -> i32 {
    let (k, trace) = held!(k);
    let mut gone = Vec::new();
    traced!(trace, SxKnowledgeTrace::of, |sink| k.expire_traced(
        now,
        max_age,
        |f| gone.push(f),
        sink
    ));
    // SAFETY: see above.
    unsafe { write_facts(&gone, removed, cap) };
    to_i32(gone.len())
}

/// `sx_knowledge_expire_kind`.
///
/// # Safety
/// `k` is null or valid; `removed` is null or has room for `cap` facts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_expire_kind(
    k: *mut SxKnowledge,
    now: u32,
    kind: u32,
    max_age: u32,
    removed: *mut SxFact,
    cap: u32,
) -> i32 {
    let (k, trace) = held!(k);
    let mut gone = Vec::new();
    traced!(trace, SxKnowledgeTrace::of, |sink| k.expire_kind_traced(
        now,
        kind,
        max_age,
        |f| gone.push(f),
        sink
    ));
    // SAFETY: see above.
    unsafe { write_facts(&gone, removed, cap) };
    to_i32(gone.len())
}

/// `sx_knowledge_clear`.
///
/// # Safety
/// `k` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_knowledge_clear(k: *mut SxKnowledge) -> i32 {
    held!(k).0.clear();
    0
}

// ---- outbox -----------------------------------------------------------------

/// `sx_message_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMessageTrace {
    /// `SX_MESSAGE_TRACE_*`.
    pub kind: u32,
    /// The message concerned.
    pub msg: SxMessage,
    /// `POSTED`: when it becomes due; `QUEUED`: when it arrives.
    pub at: u32,
    /// `REFUSED`: `SX_REFUSED_*`.
    pub refused: u32,
}

impl SxMessageTrace {
    pub(crate) fn of_code(c: convert::MessageTraceCode) -> Self {
        Self {
            kind: c.kind,
            msg: c.msg,
            at: c.at,
            refused: c.refused,
        }
    }

    /// The record of a message trace.
    pub fn of(event: MessageTrace) -> Self {
        Self::of_code(convert::message_trace(event))
    }
}

/// `sx_message_trace_fn`.
pub type MessageTraceFn = unsafe extern "C" fn(*mut c_void, *const SxMessageTrace);

/// `sx_outbox`.
pub struct SxOutbox {
    outbox: Outbox,
    trace: TraceSlot<SxMessageTrace>,
}

/// `sx_outbox_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_outbox_new(capacity: u32) -> *mut SxOutbox {
    create(|| {
        Outbox::new(capacity as usize).ok().map(|outbox| SxOutbox {
            outbox,
            trace: None,
        })
    })
}

/// `sx_outbox_set_trace`.
///
/// # Safety
/// `o` is null or valid; `trace` and `context` stay valid while tracing is
/// on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_set_trace(
    o: *mut SxOutbox,
    trace: Option<MessageTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { o.as_mut() }.map(|o| &mut o.trace), trace, context)
}

/// `sx_outbox_free`.
///
/// # Safety
/// `o` is null or from [`sx_outbox_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_free(o: *mut SxOutbox) {
    // SAFETY: see above.
    unsafe { destroy(o) }
}

/// `sx_outbox_post`.
///
/// # Safety
/// `o` is null or valid; `msg` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_post(
    o: *mut SxOutbox,
    now: u32,
    msg: *const SxMessage,
    delay: u32,
    cooldown: u32,
) -> i32 {
    let o = get!(o);
    let msg = input!(msg);
    let posted = traced!(o.trace, SxMessageTrace::of, |sink| o
        .outbox
        .post_traced(now, *msg, delay, cooldown, sink));
    match posted {
        Ok(()) => POSTED as i32,
        Err(why) => convert::refused(why) as i32,
    }
}

/// `sx_outbox_due`.
///
/// # Safety
/// `o` is null or valid; `out` is null or has room for `cap` messages.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_due(
    o: *mut SxOutbox,
    now: u32,
    out: *mut SxMessage,
    cap: u32,
) -> i32 {
    let o = get!(o);
    if out.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    let mut moved = Vec::new();
    traced!(o.trace, SxMessageTrace::of, |sink| o.outbox.due_max_traced(
        now,
        cap as usize,
        &mut moved,
        sink
    ));
    // SAFETY: at most `cap` messages (see above).
    unsafe { write_messages(&moved, out, cap) }
}

/// `sx_outbox_sender_gone`.
///
/// # Safety
/// `o` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_sender_gone(o: *mut SxOutbox) -> i32 {
    let o = get!(o);
    traced!(o.trace, SxMessageTrace::of, |sink| o
        .outbox
        .sender_gone_traced(sink));
    0
}

/// `sx_outbox_pending`.
///
/// # Safety
/// `o` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_pending(o: *mut SxOutbox) -> i32 {
    to_i32(get!(o).outbox.pending())
}

/// `sx_outbox_clear`.
///
/// # Safety
/// `o` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_outbox_clear(o: *mut SxOutbox) -> i32 {
    get!(o).outbox.clear();
    0
}

// ---- inbox ------------------------------------------------------------------

/// `sx_inbox`.
pub struct SxInbox {
    inbox: Inbox,
    trace: TraceSlot<SxMessageTrace>,
}

/// `sx_inbox_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_inbox_new(capacity: u32) -> *mut SxInbox {
    create(|| {
        Inbox::new(capacity as usize)
            .ok()
            .map(|inbox| SxInbox { inbox, trace: None })
    })
}

/// `sx_inbox_set_trace`.
///
/// # Safety
/// `b` is null or valid; `trace` and `context` stay valid while tracing is
/// on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_set_trace(
    b: *mut SxInbox,
    trace: Option<MessageTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { b.as_mut() }.map(|b| &mut b.trace), trace, context)
}

/// `sx_inbox_free`.
///
/// # Safety
/// `b` is null or from [`sx_inbox_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_free(b: *mut SxInbox) {
    // SAFETY: see above.
    unsafe { destroy(b) }
}

/// `sx_inbox_push`.
///
/// # Safety
/// `b` is null or valid; `msg` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_push(
    b: *mut SxInbox,
    msg: *const SxMessage,
    arrive_at: u32,
) -> i32 {
    let b = get!(b);
    let msg = input!(msg);
    flag(traced!(b.trace, SxMessageTrace::of, |sink| b
        .inbox
        .push_traced(*msg, arrive_at, sink)))
}

/// `sx_inbox_take_due`.
///
/// # Safety
/// `b` is null or valid; `out` is null or has room for `cap` messages.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_take_due(
    b: *mut SxInbox,
    now: u32,
    out: *mut SxMessage,
    cap: u32,
) -> i32 {
    let b = get!(b);
    if out.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    let mut moved = Vec::new();
    traced!(b.trace, SxMessageTrace::of, |sink| b
        .inbox
        .take_due_max_traced(now, cap as usize, &mut moved, sink));
    // SAFETY: at most `cap` messages (see above).
    unsafe { write_messages(&moved, out, cap) }
}

/// `sx_inbox_count`.
///
/// # Safety
/// `b` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_count(b: *mut SxInbox) -> i32 {
    to_i32(get!(b).inbox.len())
}

/// `sx_inbox_clear`.
///
/// # Safety
/// `b` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_inbox_clear(b: *mut SxInbox) -> i32 {
    get!(b).inbox.clear();
    0
}

/// `sx_arrival`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_arrival(now: u32, base: u32, distance: u32, per_unit: u32) -> u32 {
    arrival(now, base, distance, per_unit)
}

// ---- route ------------------------------------------------------------------

/// `sx_route`.
///
/// # Safety
/// `c` is null or points to `n` candidates; `out` is null or has room for `n`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_route(
    rule: u32,
    arg: i64,
    ox: i32,
    oy: i32,
    c: *const SxNearby,
    n: u32,
    metric: u32,
    divisor: i32,
    out: *mut u32,
) -> i32 {
    if (c.is_null() && n > 0) || (out.is_null() && n > 0) {
        return ERROR_ARGUMENT;
    }
    let Some(metric) = convert::metric(metric) else {
        return ERROR_ARGUMENT;
    };
    let Some(rule) = convert::route_rule(rule, arg) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: see above.
    let list: Vec<(u32, i32, i32)> = unsafe { slice_of(c, n as usize) }
        .iter()
        .map(|c| (c.id, c.x, c.y))
        .collect();
    let mut ids = Vec::new();
    route(rule, (ox, oy), &list, metric, divisor, &mut ids);
    for (i, id) in ids.iter().enumerate() {
        // SAFETY: a route never has more ids than candidates (see above).
        unsafe { out.add(i).write(*id) };
    }
    to_i32(ids.len())
}

// ---- areas ------------------------------------------------------------------

/// `sx_areas_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxAreasTrace {
    /// `SX_AREAS_TRACE_*`.
    pub kind: u32,
    /// `ADDED`, `RENEWED`: the slot.
    pub index: u32,
    /// The area concerned.
    pub area: SxArea,
}

impl SxAreasTrace {
    /// The record of an areas trace.
    pub fn of(event: AreasTrace) -> Self {
        let c = convert::areas_trace(event);
        Self {
            kind: c.kind,
            index: c.index,
            area: c.area,
        }
    }
}

/// `sx_areas_trace_fn`.
pub type AreasTraceFn = unsafe extern "C" fn(*mut c_void, *const SxAreasTrace);

/// `sx_areas`: its own set, or the one of a squad (`sx_squad_areas`).
pub struct SxAreas {
    pub(crate) value: Held<Areas>,
    trace: TraceSlot<SxAreasTrace>,
}

impl SxAreas {
    /// A handle on the set inside another handle.
    pub(crate) fn inside(inside: Inside<Areas>) -> Self {
        Self {
            value: Held::Inside(inside),
            trace: None,
        }
    }
}

/// `sx_areas_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_areas_new(capacity: u32) -> *mut SxAreas {
    create(|| {
        Areas::new(capacity as usize).ok().map(|a| SxAreas {
            value: Held::Own(a),
            trace: None,
        })
    })
}

/// `sx_areas_free`: does nothing for the set lent by `sx_squad_areas`.
///
/// # Safety
/// `a` is null, from [`sx_areas_new`] and not used again, or lent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_free(a: *mut SxAreas) {
    // SAFETY: null or valid (see above).
    if unsafe { a.as_ref() }.is_some_and(|a| a.value.is_lent()) {
        return;
    }
    // SAFETY: see above.
    unsafe { destroy(a) }
}

/// `sx_areas_set_trace`.
///
/// # Safety
/// `a` is null or valid; `trace` and `context` stay valid while tracing is
/// on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_set_trace(
    a: *mut SxAreas,
    trace: Option<AreasTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { a.as_mut() }.map(|a| &mut a.trace), trace, context)
}

/// `sx_areas_add`.
///
/// # Safety
/// `a` is null or valid; `area` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_add(a: *mut SxAreas, now: u32, area: *const SxArea) -> i32 {
    let (a, trace) = held!(a);
    let area = input!(area);
    to_i32(traced!(trace, SxAreasTrace::of, |sink| a
        .add_traced(now, *area, sink)))
}

/// `sx_areas_expire`.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_expire(a: *mut SxAreas, now: u32) -> i32 {
    let (a, trace) = held!(a);
    to_i32(traced!(trace, SxAreasTrace::of, |sink| a.expire_traced(now, sink)))
}

/// `sx_areas_contains`.
///
/// # Safety
/// `a` is null or valid; `index` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_contains(
    a: *mut SxAreas,
    x: i32,
    y: i32,
    z: i32,
    index: *mut u32,
) -> i32 {
    match held!(a).0.contains([x, y, z]) {
        Some(i) => {
            // SAFETY: see above.
            unsafe { put(index, i as u32) };
            1
        }
        None => 0,
    }
}

/// `sx_areas_contains_kind`.
///
/// # Safety
/// `a` is null or valid; `index` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_contains_kind(
    a: *mut SxAreas,
    x: i32,
    y: i32,
    z: i32,
    kind: u32,
    index: *mut u32,
) -> i32 {
    match held!(a).0.contains_kind([x, y, z], kind) {
        Some(i) => {
            // SAFETY: see above.
            unsafe { put(index, i as u32) };
            1
        }
        None => 0,
    }
}

/// `sx_areas_blocks`.
///
/// # Safety
/// `a` is null or valid; `cell`, `start`, `goal` are null or point to 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_blocks(
    a: *mut SxAreas,
    cell: *const i32,
    start: *const i32,
    goal: *const i32,
) -> i32 {
    let (a, _) = held!(a);
    if cell.is_null() || start.is_null() || goal.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { flag(a.blocks(pos3(cell), pos3(start), pos3(goal))) }
}

/// `sx_areas_cost`.
///
/// # Safety
/// `a` is null or valid; `cell`, `start`, `goal` are null or point to 3
/// integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_cost(
    a: *mut SxAreas,
    cell: *const i32,
    start: *const i32,
    goal: *const i32,
) -> i64 {
    // SAFETY: the caller passes null or a valid pointer (sagax.h).
    let Some(a) = (unsafe { a.as_ref() }).and_then(|a| a.value.get()) else {
        return i64::from(ERROR_ARGUMENT);
    };
    if cell.is_null() || start.is_null() || goal.is_null() {
        return i64::from(ERROR_ARGUMENT);
    }
    // SAFETY: see above.
    let cost = a.cost(unsafe { pos3(cell) }, unsafe { pos3(start) }, unsafe {
        pos3(goal)
    });
    i64::from(cost)
}

/// `sx_areas_cost_ctx`: the context of [`sx_areas_move_cost`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SxAreasCostCtx {
    /// The areas, or null (no extra costs).
    pub areas: *mut SxAreas,
    /// The walker's start cell.
    pub start: [i32; 3],
    /// The goal cell.
    pub goal: [i32; 3],
}

/// `sx_areas_move_cost`: an `sx_move_cost_fn` over an [`SxAreasCostCtx`].
/// Blocked is -1, else the cost saturated to `i32::MAX - 1`; null context or
/// areas give 0. The level is the z coordinate.
///
/// # Safety
/// `context` is null or points to an [`SxAreasCostCtx`] whose `areas` is null
/// or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_move_cost(
    context: *mut std::ffi::c_void,
    x: i32,
    y: i32,
    level: i32,
    to_x: i32,
    to_y: i32,
    to_level: i32,
) -> i32 {
    // SAFETY: see above.
    let Some(ctx) = (unsafe { context.cast::<SxAreasCostCtx>().as_ref() }) else {
        return 0;
    };
    // SAFETY: see above.
    let Some(areas) = (unsafe { ctx.areas.as_ref() }).and_then(|a| a.value.get()) else {
        return 0;
    };
    let _ = (x, y, level);
    match areas.cell_cost(
        Cell::new(to_x, to_y, to_level),
        Cell::from(ctx.start),
        Cell::from(ctx.goal),
    ) {
        None => -1,
        Some(c) => c.min(i32::MAX as u32 - 1) as i32,
    }
}

/// `sx_areas_count`.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_count(a: *mut SxAreas) -> i32 {
    to_i32(held!(a).0.len())
}

/// `sx_areas_at`.
///
/// # Safety
/// `a` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_at(a: *mut SxAreas, index: u32, out: *mut SxArea) -> i32 {
    let (a, _) = held!(a);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match a.iter().nth(index as usize) {
        Some(found) => {
            // SAFETY: `out` is non-null and writable (see above).
            unsafe { put(out, *found) };
            1
        }
        None => 0,
    }
}

/// `sx_areas_clear`.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_areas_clear(a: *mut SxAreas) -> i32 {
    held!(a).0.clear();
    0
}

// ---- timers -----------------------------------------------------------------

/// `sx_timers_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxTimersTrace {
    /// `SX_TIMERS_TRACE_*`.
    pub kind: u32,
    /// The id set.
    pub id: u32,
    /// Its ticks.
    pub duration: u32,
    /// `REPLACED`: the id that gave way.
    pub old: u32,
    /// `REPLACED`: the ticks it had left.
    pub old_left: u32,
}

impl SxTimersTrace {
    /// The record of a timers trace.
    pub fn of(event: TimersTrace) -> Self {
        let c = convert::timers_trace(event);
        Self {
            kind: c.kind,
            id: c.id,
            duration: c.duration,
            old: c.old,
            old_left: c.old_left,
        }
    }
}

/// `sx_timers_trace_fn`.
pub type TimersTraceFn = unsafe extern "C" fn(*mut c_void, *const SxTimersTrace);

/// `sx_timers`.
pub struct SxTimers {
    timers: Timers,
    trace: TraceSlot<SxTimersTrace>,
}

/// `sx_timers_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_timers_new(capacity: u32) -> *mut SxTimers {
    create(|| {
        Timers::new(capacity as usize).ok().map(|timers| SxTimers {
            timers,
            trace: None,
        })
    })
}

/// `sx_timers_set_trace`.
///
/// # Safety
/// `t` is null or valid; `trace` and `context` stay valid while tracing is
/// on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_set_trace(
    t: *mut SxTimers,
    trace: Option<TimersTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { t.as_mut() }.map(|t| &mut t.trace), trace, context)
}

/// `sx_timers_free`.
///
/// # Safety
/// `t` is null or from [`sx_timers_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_free(t: *mut SxTimers) {
    // SAFETY: see above.
    unsafe { destroy(t) }
}

/// `sx_timers_set`.
///
/// # Safety
/// `t` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_set(t: *mut SxTimers, id: u32, now: u32, duration: u32) -> i32 {
    let t = get!(t);
    traced!(t.trace, SxTimersTrace::of, |sink| t
        .timers
        .set_traced(id, now, duration, sink));
    0
}

/// `sx_timers_active`.
///
/// # Safety
/// `t` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_active(t: *mut SxTimers, id: u32, now: u32) -> i32 {
    flag(get!(t).timers.active(id, now))
}

/// `sx_timers_left`.
///
/// # Safety
/// `t` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_left(t: *mut SxTimers, id: u32, now: u32) -> i64 {
    // SAFETY: the caller passes null or a valid pointer (sagax.h).
    match unsafe { t.as_mut() } {
        Some(t) => i64::from(t.timers.left(id, now)),
        None => i64::from(ERROR_ARGUMENT),
    }
}

/// `sx_timers_remove`.
///
/// # Safety
/// `t` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_remove(t: *mut SxTimers, id: u32) -> i32 {
    flag(get!(t).timers.remove(id))
}

/// `sx_timers_count`.
///
/// # Safety
/// `t` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_count(t: *mut SxTimers) -> i32 {
    to_i32(get!(t).timers.len())
}

/// `sx_timers_clear`.
///
/// # Safety
/// `t` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_timers_clear(t: *mut SxTimers) -> i32 {
    get!(t).timers.clear();
    0
}

// ---- progress, interval, alertness ------------------------------------------

/// `sx_progress_start`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_progress_start(p: *mut SxProgress, value: i32, now: u32) -> i32 {
    get!(p).start(value, now);
    0
}

/// `sx_progress_update`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_progress_update(
    p: *mut SxProgress,
    value: i32,
    now: u32,
    min_gain: i32,
) -> i32 {
    flag(get!(p).update(value, now, min_gain))
}

/// `sx_progress_stalled`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_progress_stalled(p: *mut SxProgress, now: u32, limit: u32) -> i32 {
    flag(get!(p).stalled(now, limit))
}

/// `sx_progress_reset`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_progress_reset(p: *mut SxProgress) -> i32 {
    get!(p).reset();
    0
}

/// `sx_interval_due`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_interval_due(i: *mut SxInterval, now: u32, every: u32) -> i32 {
    flag(get!(i).due(now, every))
}

/// `sx_interval_mark`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_interval_mark(i: *mut SxInterval, now: u32) -> i32 {
    get!(i).mark(now);
    0
}

/// `sx_interval_reset`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_interval_reset(i: *mut SxInterval) -> i32 {
    get!(i).reset();
    0
}

/// `sx_alertness_stimulus`.
///
/// # Safety
/// `a` is null or valid; `cfg` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_alertness_stimulus(
    a: *mut SxAlertness,
    amount: u32,
    now: u32,
    cfg: *const SxAlertConfig,
) -> i32 {
    let a = get!(a);
    let cfg = input!(cfg);
    a.stimulus(amount, now, cfg);
    0
}

/// `sx_alertness_update`.
///
/// # Safety
/// `a` is null or valid; `cfg` is null or valid; `levels` is null or has room
/// for 2 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_alertness_update(
    a: *mut SxAlertness,
    now: u32,
    cfg: *const SxAlertConfig,
    levels: *mut i32,
) -> i32 {
    let a = get!(a);
    let cfg = input!(cfg);
    match a.update(now, cfg) {
        Some((from, to)) => {
            if !levels.is_null() {
                // SAFETY: room for 2 integers (see above).
                unsafe {
                    levels.write(i32::from(from));
                    levels.add(1).write(i32::from(to));
                }
            }
            1
        }
        None => 0,
    }
}

/// `sx_alertness_value`.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_alertness_value(a: *const SxAlertness) -> i64 {
    // SAFETY: the caller passes null or a valid pointer (sagax.h).
    match unsafe { a.as_ref() } {
        Some(a) => i64::from(a.value()),
        None => i64::from(ERROR_ARGUMENT),
    }
}

/// `sx_alertness_level`.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_alertness_level(a: *mut SxAlertness) -> i32 {
    i32::from(get!(a).level())
}

// ---- geometry ---------------------------------------------------------------

/// `sx_sine`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_sine(heading: i32, kind: u32) -> i32 {
    sine(heading, convert::sine(kind).unwrap_or_default())
}

/// `sx_points_around`.
///
/// # Safety
/// `pattern` is null or valid; `out` is null or has room for 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_points_around(
    agent: u32,
    step: u32,
    cx: i32,
    cy: i32,
    cz: i32,
    pattern: *const SxPointPattern,
    out: *mut i32,
) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let pattern = match unsafe { pattern.as_ref() } {
        None => PointPattern::default(),
        Some(p) => match p.pattern() {
            Some(pattern) => pattern,
            None => return ERROR_ARGUMENT,
        },
    };
    match points_around(agent, step as usize, [cx, cy, cz], &pattern) {
        Some(pos) => {
            // SAFETY: see above.
            unsafe { std::ptr::copy_nonoverlapping(pos.as_ptr(), out, 3) };
            1
        }
        None => 0,
    }
}

/// `sx_sweep_heading`.
///
/// # Safety
/// `offsets` is null or points to `count` integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sweep_heading(
    base: i32,
    elapsed: u32,
    step_ticks: u32,
    offsets: *const i32,
    count: u32,
) -> i32 {
    if offsets.is_null() {
        return sweep_heading(base, elapsed, step_ticks, &LOOK_OFFSETS);
    }
    // SAFETY: see above.
    sweep_heading(base, elapsed, step_ticks, unsafe {
        slice_of(offsets, count as usize)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sagax_decide::memory::{SOURCE_OWN, SOURCE_TOLD};
    use std::mem::{offset_of, size_of};
    use std::ptr::{null, null_mut};

    fn fact(subject: u32, kind: u32, seen_at: u32, strength: u32) -> Fact {
        Fact {
            subject,
            kind,
            seen_at,
            strength,
            ..Fact::default()
        }
    }

    fn msg(subject: u32) -> Message {
        Message {
            subject,
            kind: 1,
            ..Message::default()
        }
    }

    #[test]
    fn layouts_match_header() {
        assert_eq!(size_of::<SxFact>(), 36);
        assert_eq!(offset_of!(SxFact, kind), 4);
        assert_eq!(offset_of!(SxFact, pos), 8);
        assert_eq!(offset_of!(SxFact, seen_at), 20);
        assert_eq!(offset_of!(SxFact, strength), 24);
        assert_eq!(offset_of!(SxFact, data), 28);
        assert_eq!(offset_of!(SxFact, source), 32);
        assert_eq!(size_of::<SxMessage>(), 40);
        assert_eq!(offset_of!(SxMessage, subject), 4);
        assert_eq!(offset_of!(SxMessage, pos), 8);
        assert_eq!(offset_of!(SxMessage, seen_at), 20);
        assert_eq!(offset_of!(SxMessage, strength), 24);
        assert_eq!(offset_of!(SxMessage, data), 28);
        assert_eq!(offset_of!(SxMessage, sender), 32);
        assert_eq!(offset_of!(SxMessage, keep_if_sender_gone), 36);
        assert_eq!(size_of::<SxArea>(), 52);
        assert_eq!(offset_of!(SxArea, pos), 4);
        assert_eq!(offset_of!(SxArea, radius), 16);
        assert_eq!(offset_of!(SxArea, strength), 20);
        assert_eq!(offset_of!(SxArea, since), 24);
        assert_eq!(offset_of!(SxArea, lifetime), 28);
        assert_eq!(offset_of!(SxArea, data), 32);
        assert_eq!(offset_of!(SxArea, source), 36);
        assert_eq!(offset_of!(SxArea, blocking), 48);
        assert_eq!(offset_of!(SxArea, has_source), 49);
        assert_eq!(size_of::<SxProgress>(), 12);
        assert_eq!(offset_of!(SxProgress, since), 4);
        assert_eq!(offset_of!(SxProgress, started), 8);
        assert_eq!(size_of::<SxInterval>(), 8);
        assert_eq!(offset_of!(SxInterval, started), 4);
        assert_eq!(size_of::<SxAlertness>(), 16);
        assert_eq!(size_of::<SxAlertConfig>(), 52);
        assert_eq!(offset_of!(SxAlertConfig, decay_per_tick), 4);
        assert_eq!(offset_of!(SxAlertConfig, hold), 8);
        assert_eq!(offset_of!(SxAlertConfig, hysteresis), 12);
        assert_eq!(offset_of!(SxAlertConfig, count), 16);
        assert_eq!(offset_of!(SxAlertConfig, thresholds), 20);
        assert_eq!(size_of::<SxNearby>(), 12);
        assert_eq!(size_of::<SxPointPattern>(), 96);
        assert_eq!(offset_of!(SxPointPattern, offsets), 12);
        assert_eq!(offset_of!(SxPointPattern, distances), 44);
        assert_eq!(offset_of!(SxPointPattern, sine), 76);
        assert_eq!(offset_of!(SxPointPattern, min_x), 80);
        assert_eq!(offset_of!(SxPointPattern, max_y), 92);
    }

    #[test]
    fn knowledge_through_c() {
        unsafe {
            assert!(sx_knowledge_new(0, 0).is_null());
            assert!(sx_knowledge_new(2, 7).is_null());
            let k = sx_knowledge_new(2, 0);
            let mut gone = Fact::default();
            assert_eq!(sx_knowledge_learn(k, 5, &fact(1, 1, 5, 1), &mut gone), 0);
            assert_eq!(sx_knowledge_learn(k, 6, &fact(2, 1, 6, 1), &mut gone), 0);
            assert_eq!(sx_knowledge_learn(k, 7, &fact(3, 1, 7, 1), &mut gone), 1);
            assert_eq!(gone.subject, 1);
            assert_eq!(sx_knowledge_learn(k, 8, &fact(4, 1, 8, 1), null_mut()), 1);
            assert_eq!(sx_knowledge_count(k), 2);
            // merge: an older report does not replace a newer fact.
            assert_eq!(sx_knowledge_merge(k, 10, &fact(3, 1, 1, 9), null_mut()), 0);
            let mut out = Fact::default();
            assert_eq!(sx_knowledge_get(k, 3, 1, &mut out), 1);
            assert_eq!(out.strength, 1);
            assert_eq!(sx_knowledge_merge(k, 10, &fact(3, 1, 9, 9), null_mut()), 0);
            assert_eq!(sx_knowledge_get(k, 3, 1, &mut out), 1);
            assert_eq!((out.strength, out.seen_at), (9, 9));
            assert_eq!(sx_knowledge_touch(k, 3, 1, 12), 1);
            assert_eq!(sx_knowledge_touch(k, 99, 1, 12), 0);
            assert_eq!(sx_knowledge_set_pos(k, 3, 1, 1, 2, 3), 1);
            assert_eq!(sx_knowledge_set_data(k, 3, 1, 5), 1);
            assert_eq!(sx_knowledge_set_strength(k, 3, 1, 6), 1);
            assert_eq!(sx_knowledge_set_strength(k, 3, 2, 6), 0);
            assert_eq!(sx_knowledge_get(k, 3, 1, &mut out), 1);
            assert_eq!(
                (out.pos, out.data, out.strength, out.seen_at),
                ([1, 2, 3], 5, 6, 12)
            );
            assert_eq!(sx_knowledge_get(k, 1, 1, &mut out), 0);
            assert_eq!(sx_knowledge_at(k, 0, &mut out), 1);
            assert_eq!(out.subject, 3);
            assert_eq!(sx_knowledge_at(k, 2, &mut out), 0);
            // messages
            let m = Message {
                subject: 8,
                kind: 2,
                seen_at: 20,
                ..Message::default()
            };
            assert_eq!(sx_knowledge_learn_message(k, 20, &m, 2, &mut gone), 1);
            assert_eq!(sx_knowledge_get(k, 8, 2, &mut out), 1);
            assert_eq!(out.source, 2);
            assert_eq!(sx_knowledge_merge_message(k, 20, &m, 1, null_mut()), 0);
            assert_eq!(sx_knowledge_get(k, 8, 2, &mut out), 1);
            assert_eq!(out.source, 1);
            assert_eq!(sx_knowledge_remove(k, 8, 2), 1);
            assert_eq!(sx_knowledge_remove(k, 8, 2), 0);
            assert_eq!(sx_knowledge_remove_subject(k, 3), 1);
            assert_eq!(sx_knowledge_clear(k), 0);
            assert_eq!(sx_knowledge_count(k), 0);
            // nulls
            assert_eq!(sx_knowledge_count(null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_knowledge_learn(k, 1, null(), null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_knowledge_get(k, 1, 1, null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_knowledge_at(k, 0, null_mut()), ERROR_ARGUMENT);
            let f = fact(1, 1, 1, 1);
            assert_eq!(
                sx_knowledge_learn(null_mut(), 1, &f, null_mut()),
                ERROR_ARGUMENT
            );
            sx_knowledge_free(k);
            sx_knowledge_free(null_mut());
        }
    }

    #[test]
    fn knowledge_evicts_weakest() {
        unsafe {
            let k = sx_knowledge_new(2, 1);
            sx_knowledge_learn(k, 5, &fact(1, 1, 5, 9), null_mut());
            sx_knowledge_learn(k, 5, &fact(2, 1, 5, 2), null_mut());
            let mut gone = Fact::default();
            assert_eq!(sx_knowledge_learn(k, 6, &fact(3, 1, 6, 5), &mut gone), 1);
            assert_eq!(gone.subject, 2);
            sx_knowledge_free(k);
        }
    }

    #[test]
    fn knowledge_expire_counts_all_but_writes_cap() {
        unsafe {
            let k = sx_knowledge_new(8, 0);
            for (s, kind) in [(1, 1), (2, 1), (3, 2), (4, 1)] {
                sx_knowledge_learn(k, 0, &fact(s, kind, 0, 1), null_mut());
            }
            let mut sentinel = [fact(77, 77, 77, 77); 3];
            // Kind 1 only: three facts expire, room for two.
            assert_eq!(
                sx_knowledge_expire_kind(k, 10, 1, 10, sentinel.as_mut_ptr(), 2),
                3
            );
            assert_eq!(sentinel[0].subject, 1);
            assert_eq!(sentinel[1].subject, 2);
            assert_eq!(sentinel[2].subject, 77);
            assert_eq!(sx_knowledge_count(k), 1);
            let mut one = [Fact::default(); 1];
            assert_eq!(sx_knowledge_expire(k, 10, 10, one.as_mut_ptr(), 1), 1);
            assert_eq!(one[0].subject, 3);
            assert_eq!(sx_knowledge_expire(k, 10, 10, null_mut(), 0), 0);
            assert_eq!(
                sx_knowledge_expire(null_mut(), 1, 1, null_mut(), 0),
                ERROR_ARGUMENT
            );
            sx_knowledge_free(k);
        }
    }

    #[test]
    fn outbox_and_inbox_through_c() {
        unsafe {
            assert!(sx_outbox_new(0).is_null());
            assert!(sx_inbox_new(0).is_null());
            let o = sx_outbox_new(2);
            assert_eq!(sx_outbox_post(o, 0, &msg(1), 0, 10), 0);
            assert_eq!(sx_outbox_post(o, 5, &msg(2), 0, 10), 2);
            assert_eq!(sx_outbox_post(o, 10, &msg(2), 0, 10), 0);
            assert_eq!(sx_outbox_post(o, 30, &msg(3), 0, 10), 1);
            assert_eq!(sx_outbox_pending(o), 2);
            let mut out = [Message::default(); 2];
            assert_eq!(sx_outbox_due(o, 10, out.as_mut_ptr(), 1), 1);
            assert_eq!(out[0].subject, 1);
            assert_eq!(sx_outbox_pending(o), 1);
            assert_eq!(sx_outbox_due(o, 10, out.as_mut_ptr(), 2), 1);
            assert_eq!(out[0].subject, 2);
            assert_eq!(sx_outbox_due(o, 10, null_mut(), 1), ERROR_ARGUMENT);
            assert_eq!(sx_outbox_due(o, 10, null_mut(), 0), 0);
            assert_eq!(sx_outbox_post(o, 1, null(), 0, 0), ERROR_ARGUMENT);
            assert_eq!(sx_outbox_sender_gone(o), 0);
            assert_eq!(sx_outbox_clear(o), 0);
            assert_eq!(sx_outbox_pending(null_mut()), ERROR_ARGUMENT);
            sx_outbox_free(o);

            let b = sx_inbox_new(2);
            assert_eq!(sx_inbox_push(b, &msg(1), 20), 1);
            assert_eq!(sx_inbox_push(b, &msg(2), 5), 1);
            assert_eq!(sx_inbox_push(b, &msg(3), 30), 0);
            assert_eq!(sx_inbox_count(b), 2);
            assert_eq!(sx_inbox_take_due(b, 25, out.as_mut_ptr(), 1), 1);
            assert_eq!(out[0].subject, 1);
            assert_eq!(sx_inbox_take_due(b, 25, out.as_mut_ptr(), 2), 1);
            assert_eq!(out[0].subject, 2);
            assert_eq!(sx_inbox_take_due(b, 25, null_mut(), 1), ERROR_ARGUMENT);
            assert_eq!(sx_inbox_push(b, null(), 1), ERROR_ARGUMENT);
            assert_eq!(sx_inbox_clear(b), 0);
            assert_eq!(sx_inbox_count(null_mut()), ERROR_ARGUMENT);
            sx_inbox_free(b);
            sx_inbox_free(null_mut());
            assert_eq!(sx_arrival(u32::MAX - 1, 2, 3, 4), 12);
        }
    }

    #[test]
    fn route_through_c() {
        unsafe {
            let c = [
                SxNearby {
                    id: 1,
                    x: 100,
                    y: 0,
                },
                SxNearby { id: 2, x: 10, y: 0 },
                SxNearby { id: 3, x: 50, y: 0 },
            ];
            let mut out = [0u32; 3];
            let run = |rule, arg, metric, out: &mut [u32; 3]| {
                sx_route(rule, arg, 0, 0, c.as_ptr(), 3, metric, 1, out.as_mut_ptr())
            };
            assert_eq!(run(0, 2, 0, &mut out), 2);
            assert_eq!(out[..2], [2, 3]);
            assert_eq!(run(1, 50, 0, &mut out), 2);
            assert_eq!(out[..2], [2, 3]);
            assert_eq!(run(1, 9, 1, &mut out), 0);
            assert_eq!(run(2, 0, 2, &mut out), 3);
            assert_eq!(out, [1, 2, 3]);
            assert_eq!(run(3, 3, 3, &mut out), 1);
            assert_eq!(out[0], 3);
            assert_eq!(run(3, 9, 0, &mut out), 0);
            assert_eq!(run(4, 0, 0, &mut out), ERROR_ARGUMENT);
            assert_eq!(run(0, 1, 4, &mut out), ERROR_ARGUMENT);
            assert_eq!(run(0, -1, 0, &mut out), ERROR_ARGUMENT);
            assert_eq!(run(3, -1, 0, &mut out), ERROR_ARGUMENT);
            assert_eq!(run(3, 1 << 40, 0, &mut out), ERROR_ARGUMENT);
            let r = sx_route(2, 0, 0, 0, null(), 3, 0, 1, out.as_mut_ptr());
            assert_eq!(r, ERROR_ARGUMENT);
            let r = sx_route(2, 0, 0, 0, c.as_ptr(), 3, 0, 1, null_mut());
            assert_eq!(r, ERROR_ARGUMENT);
            assert_eq!(sx_route(2, 0, 0, 0, null(), 0, 0, 1, null_mut()), 0);
        }
    }

    #[test]
    fn areas_and_timers_through_c() {
        unsafe {
            assert!(sx_areas_new(0).is_null());
            let a = sx_areas_new(2);
            let area = Area {
                kind: 1,
                pos: [100, 100, 0],
                radius: 10,
                strength: 7,
                lifetime: 50,
                blocking: 1,
                ..Area::default()
            };
            assert_eq!(sx_areas_add(a, 5, &area), 0);
            let soft = Area {
                kind: 2,
                blocking: 0,
                strength: 3,
                ..area
            };
            assert_eq!(sx_areas_add(a, 6, &soft), 1);
            assert_eq!(sx_areas_count(a), 2);
            let mut index = 9u32;
            assert_eq!(sx_areas_contains(a, 100, 105, 0, &mut index), 1);
            assert_eq!(index, 0);
            assert_eq!(sx_areas_contains_kind(a, 100, 105, 0, 2, &mut index), 1);
            assert_eq!(index, 1);
            assert_eq!(sx_areas_contains_kind(a, 100, 105, 0, 3, null_mut()), 0);
            assert_eq!(sx_areas_contains(a, 500, 0, 0, null_mut()), 0);
            let (cell, far) = ([100, 100, 0], [900, 900, 0]);
            let (c, f) = (cell.as_ptr(), far.as_ptr());
            assert_eq!(sx_areas_blocks(a, c, f, f), 1);
            assert_eq!(sx_areas_blocks(a, c, c, f), 0);
            assert_eq!(sx_areas_blocks(a, null(), f, f), ERROR_ARGUMENT);
            assert_eq!(sx_areas_cost(a, c, f, f), 3);
            assert_eq!(sx_areas_cost(a, null(), f, f), i64::from(ERROR_ARGUMENT));
            assert_eq!(
                sx_areas_cost(null_mut(), c, f, f),
                i64::from(ERROR_ARGUMENT)
            );
            let mut out = Area::default();
            assert_eq!(sx_areas_at(a, 1, &mut out), 1);
            assert_eq!((out.kind, out.since), (2, 6));
            assert_eq!(sx_areas_at(a, 2, &mut out), 0);
            assert_eq!(sx_areas_at(a, 0, null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_areas_add(a, 1, null()), ERROR_ARGUMENT);
            assert_eq!(sx_areas_expire(a, 100), 2);
            assert_eq!(sx_areas_clear(a), 0);
            assert_eq!(sx_areas_count(null_mut()), ERROR_ARGUMENT);
            sx_areas_free(a);

            assert!(sx_timers_new(0).is_null());
            let t = sx_timers_new(2);
            assert_eq!(sx_timers_set(t, 4, 10, 10), 0);
            assert_eq!(sx_timers_active(t, 4, 15), 1);
            assert_eq!(sx_timers_active(t, 4, 20), 0);
            assert_eq!(sx_timers_left(t, 4, 15), 5);
            assert_eq!(sx_timers_left(t, 9, 15), 0);
            assert_eq!(sx_timers_left(null_mut(), 4, 15), i64::from(ERROR_ARGUMENT));
            assert_eq!(sx_timers_count(t), 1);
            assert_eq!(sx_timers_remove(t, 4), 1);
            assert_eq!(sx_timers_remove(t, 4), 0);
            assert_eq!(sx_timers_clear(t), 0);
            assert_eq!(sx_timers_set(null_mut(), 1, 1, 1), ERROR_ARGUMENT);
            sx_timers_free(t);
        }
    }

    #[test]
    fn value_structs_through_c() {
        unsafe {
            let mut p = Progress::default();
            assert_eq!(sx_progress_update(&mut p, 5, 1, 1), 0);
            assert_eq!(sx_progress_start(&mut p, 50, 100), 0);
            assert_eq!(sx_progress_stalled(&mut p, 110, 10), 1);
            assert_eq!(sx_progress_update(&mut p, 40, 111, 5), 1);
            assert_eq!(sx_progress_stalled(&mut p, 115, 10), 0);
            assert_eq!(sx_progress_reset(&mut p), 0);
            assert_eq!(p, Progress::default());
            assert_eq!(sx_progress_reset(null_mut()), ERROR_ARGUMENT);

            let mut i = Interval::default();
            assert_eq!(sx_interval_due(&mut i, 0, 10), 1);
            assert_eq!(sx_interval_mark(&mut i, 5), 0);
            assert_eq!(sx_interval_due(&mut i, 10, 10), 0);
            assert_eq!(sx_interval_due(&mut i, 15, 10), 1);
            assert_eq!(sx_interval_reset(&mut i), 0);
            assert_eq!(sx_interval_due(&mut i, 5, 10), 1);
            assert_eq!(sx_interval_mark(null_mut(), 1), ERROR_ARGUMENT);

            let cfg = AlertConfig {
                max: 100,
                decay_per_tick: 1,
                hold: 0,
                hysteresis: 0,
                count: 2,
                thresholds: [30, 60, 0, 0, 0, 0, 0, 0],
            };
            let mut a = Alertness::default();
            let mut levels = [9, 9];
            assert_eq!(sx_alertness_stimulus(&mut a, 70, 0, &cfg), 0);
            assert_eq!(sx_alertness_update(&mut a, 0, &cfg, levels.as_mut_ptr()), 1);
            assert_eq!(levels, [0, 2]);
            assert_eq!(sx_alertness_level(&mut a), 2);
            assert_eq!(sx_alertness_value(&a), 70);
            assert_eq!(sx_alertness_update(&mut a, 20, &cfg, null_mut()), 1);
            assert_eq!(sx_alertness_level(&mut a), 1);
            assert_eq!(sx_alertness_update(&mut a, 20, &cfg, null_mut()), 0);
            assert_eq!(sx_alertness_value(null()), i64::from(ERROR_ARGUMENT));
            assert_eq!(sx_alertness_stimulus(&mut a, 1, 1, null()), ERROR_ARGUMENT);
            assert_eq!(
                sx_alertness_update(&mut a, 1, null(), null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_alertness_level(null_mut()), ERROR_ARGUMENT);
        }
    }

    #[test]
    fn geometry_through_c() {
        unsafe {
            assert_eq!(sx_sine(64, 0), 65536);
            assert_eq!(sx_sine(64, 1), 256);
            assert_eq!(sx_sine(-64, 1), -256);
            let mut out = [0; 3];
            let mut reference = [0; 3];
            // The default pattern and the same pattern spelled out agree.
            let spelled = SxPointPattern {
                heading_step: 0x62,
                first_distance: 0x100,
                count: 3,
                offsets: [0, 0x20, -0x20, 0, 0, 0, 0, 0],
                distances: [5 * 256, 9 * 256, 12 * 256, 0, 0, 0, 0, 0],
                sine: 0,
                min_x: i32::MIN,
                min_y: i32::MIN,
                max_x: i32::MAX,
                max_y: i32::MAX,
            };
            for step in 0..5 {
                let a = sx_points_around(3, step, 1000, 2000, 4, null(), out.as_mut_ptr());
                let b = sx_points_around(3, step, 1000, 2000, 4, &spelled, reference.as_mut_ptr());
                assert_eq!(a, b);
                assert_eq!(a, i32::from(step <= 3));
                if a == 1 {
                    assert_eq!(out, reference);
                    assert_eq!(out[2], 4);
                }
            }
            // The coarse sine changes the points.
            let coarse = SxPointPattern { sine: 1, ..spelled };
            let r = sx_points_around(3, 1, 1000, 2000, 4, &coarse, reference.as_mut_ptr());
            assert_eq!(r, 1);
            let r = sx_points_around(3, 1, 1000, 2000, 4, &spelled, out.as_mut_ptr());
            assert_eq!(r, 1);
            assert_ne!(out, reference);
            let bad = SxPointPattern {
                count: 9,
                ..spelled
            };
            let r = sx_points_around(3, 0, 0, 0, 0, &bad, out.as_mut_ptr());
            assert_eq!(r, ERROR_ARGUMENT);
            let bad = SxPointPattern { sine: 2, ..spelled };
            let r = sx_points_around(3, 0, 0, 0, 0, &bad, out.as_mut_ptr());
            assert_eq!(r, ERROR_ARGUMENT);
            let r = sx_points_around(3, 0, 0, 0, 0, null(), null_mut());
            assert_eq!(r, ERROR_ARGUMENT);

            assert_eq!(sx_sweep_heading(10, 0, 5, null(), 0), 10);
            assert_eq!(sx_sweep_heading(10, 5, 5, null(), 0), 42);
            let own = [1, 2];
            assert_eq!(sx_sweep_heading(10, 5, 5, own.as_ptr(), 2), 12);
            assert_eq!(sx_sweep_heading(10, 50, 5, own.as_ptr(), 2), 10);
        }
    }

    /// A trace callback that keeps a copy of each record in a `Vec<R>`.
    unsafe extern "C" fn collect<R: Copy>(ctx: *mut c_void, t: *const R) {
        let records = unsafe { &mut *ctx.cast::<Vec<R>>() };
        records.push(unsafe { *t });
    }

    fn ctx<R>(records: &mut Vec<R>) -> *mut c_void {
        std::ptr::from_mut(records).cast()
    }

    #[test]
    fn knowledge_trace_through_c() {
        use sagax_abi::codes::{
            KNOWLEDGE_TRACE_ADDED, KNOWLEDGE_TRACE_EVICTED, KNOWLEDGE_TRACE_EXPIRED,
            KNOWLEDGE_TRACE_REMOVED, KNOWLEDGE_TRACE_STALE, KNOWLEDGE_TRACE_UPDATED,
        };
        let rec = |kind, fact| SxKnowledgeTrace { kind, fact };
        unsafe {
            let k = sx_knowledge_new(2, 0);
            let mut records: Vec<SxKnowledgeTrace> = Vec::new();
            let c = ctx(&mut records);
            assert_eq!(sx_knowledge_set_trace(k, Some(collect), c), 0);
            assert_eq!(
                sx_knowledge_set_trace(null_mut(), Some(collect), c),
                ERROR_ARGUMENT
            );
            let (a, b, x) = (fact(1, 0, 1, 0), fact(2, 0, 2, 0), fact(3, 0, 3, 0));
            assert_eq!(sx_knowledge_learn(k, 3, &a, null_mut()), 0);
            assert_eq!(sx_knowledge_merge(k, 3, &b, null_mut()), 0);
            let mut gone = SxFact::default();
            assert_eq!(sx_knowledge_learn(k, 3, &x, &mut gone), 1);
            assert_eq!(gone, a);
            let old = fact(2, 0, 0, 0);
            assert_eq!(sx_knowledge_merge(k, 3, &old, null_mut()), 0);
            let m = msg(3);
            let told = m.to_fact(SOURCE_TOLD);
            assert_eq!(
                sx_knowledge_learn_message(k, 3, &m, SOURCE_TOLD, null_mut()),
                1
            );
            assert_eq!(
                sx_knowledge_merge_message(k, 3, &m, SOURCE_OWN, null_mut()),
                0
            );
            let own = m.to_fact(SOURCE_OWN);
            assert_eq!(sx_knowledge_expire_kind(k, 9, 0, 1, null_mut(), 0), 1);
            assert_eq!(sx_knowledge_expire(k, 9, 1, null_mut(), 0), 1);
            assert_eq!(
                records,
                [
                    rec(KNOWLEDGE_TRACE_ADDED, a),
                    rec(KNOWLEDGE_TRACE_ADDED, b),
                    rec(KNOWLEDGE_TRACE_EVICTED, a),
                    rec(KNOWLEDGE_TRACE_ADDED, x),
                    rec(KNOWLEDGE_TRACE_STALE, old),
                    rec(KNOWLEDGE_TRACE_EVICTED, b),
                    rec(KNOWLEDGE_TRACE_ADDED, told),
                    rec(KNOWLEDGE_TRACE_UPDATED, own),
                    rec(KNOWLEDGE_TRACE_EXPIRED, x),
                    rec(KNOWLEDGE_TRACE_EXPIRED, own),
                ]
            );
            // Remove and remove_subject: one REMOVED per fact.
            records.clear();
            let (c1, c2) = (fact(5, 0, 1, 0), fact(5, 1, 1, 0));
            assert_eq!(sx_knowledge_learn(k, 9, &c1, null_mut()), 0);
            assert_eq!(sx_knowledge_learn(k, 9, &c2, null_mut()), 0);
            assert_eq!(sx_knowledge_remove(k, 5, 0), 1);
            assert_eq!(sx_knowledge_remove(k, 5, 0), 0);
            assert_eq!(sx_knowledge_learn(k, 9, &c1, null_mut()), 0);
            assert_eq!(sx_knowledge_remove_subject(k, 5), 2);
            assert_eq!(
                records[2..],
                [
                    rec(KNOWLEDGE_TRACE_REMOVED, c1),
                    rec(KNOWLEDGE_TRACE_ADDED, c1),
                    rec(KNOWLEDGE_TRACE_REMOVED, c2),
                    rec(KNOWLEDGE_TRACE_REMOVED, c1),
                ]
            );
            // Off: nothing more arrives.
            records.clear();
            assert_eq!(sx_knowledge_set_trace(k, None, null_mut()), 0);
            assert_eq!(sx_knowledge_learn(k, 9, &a, null_mut()), 0);
            assert!(records.is_empty());
            sx_knowledge_free(k);
        }
    }

    #[test]
    fn message_traces_through_c() {
        use sagax_abi::codes::{
            MESSAGE_TRACE_DELIVERED, MESSAGE_TRACE_DROPPED, MESSAGE_TRACE_POSTED,
            MESSAGE_TRACE_QUEUED, MESSAGE_TRACE_REFUSED, REFUSED_COOLDOWN, REFUSED_FULL,
        };
        let rec = |kind, msg, at, refused| SxMessageTrace {
            kind,
            msg,
            at,
            refused,
        };
        unsafe {
            let o = sx_outbox_new(1);
            let b = sx_inbox_new(1);
            let mut records: Vec<SxMessageTrace> = Vec::new();
            let c = ctx(&mut records);
            assert_eq!(sx_outbox_set_trace(o, Some(collect), c), 0);
            assert_eq!(sx_inbox_set_trace(b, Some(collect), c), 0);
            assert_eq!(
                sx_outbox_set_trace(null_mut(), Some(collect), c),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_inbox_set_trace(null_mut(), Some(collect), c),
                ERROR_ARGUMENT
            );
            let (m1, m2) = (msg(1), msg(2));
            assert_eq!(sx_outbox_post(o, 10, &m1, 5, 0), 0);
            assert_eq!(sx_outbox_post(o, 10, &m2, 5, 0), REFUSED_FULL as i32);
            let mut out = [SxMessage::default(); 2];
            assert_eq!(sx_outbox_due(o, 15, out.as_mut_ptr(), 2), 1);
            assert_eq!(sx_outbox_post(o, 16, &m2, 0, 9), REFUSED_COOLDOWN as i32);
            assert_eq!(sx_outbox_post(o, 30, &m2, 0, 9), 0);
            assert_eq!(sx_outbox_sender_gone(o), 0);
            assert_eq!(sx_inbox_push(b, &m1, 20), 1);
            assert_eq!(sx_inbox_push(b, &m2, 10), 1);
            assert_eq!(sx_inbox_push(b, &m1, 30), 0);
            assert_eq!(sx_inbox_take_due(b, 10, out.as_mut_ptr(), 2), 1);
            assert_eq!(
                records,
                [
                    rec(MESSAGE_TRACE_POSTED, m1, 15, 0),
                    rec(MESSAGE_TRACE_REFUSED, m2, 0, REFUSED_FULL),
                    rec(MESSAGE_TRACE_DELIVERED, m1, 0, 0),
                    rec(MESSAGE_TRACE_REFUSED, m2, 0, REFUSED_COOLDOWN),
                    rec(MESSAGE_TRACE_POSTED, m2, 30, 0),
                    rec(MESSAGE_TRACE_DROPPED, m2, 0, 0),
                    rec(MESSAGE_TRACE_QUEUED, m1, 20, 0),
                    rec(MESSAGE_TRACE_DROPPED, m1, 0, 0),
                    rec(MESSAGE_TRACE_QUEUED, m2, 10, 0),
                    rec(MESSAGE_TRACE_DROPPED, m1, 0, 0),
                    rec(MESSAGE_TRACE_DELIVERED, m2, 0, 0),
                ]
            );
            // Off: nothing more arrives.
            assert_eq!(sx_outbox_set_trace(o, None, null_mut()), 0);
            assert_eq!(sx_inbox_set_trace(b, None, null_mut()), 0);
            assert_eq!(sx_outbox_post(o, 50, &m1, 0, 0), 0);
            assert_eq!(sx_inbox_push(b, &m1, 50), 1);
            assert_eq!(records.len(), 11);
            sx_outbox_free(o);
            sx_inbox_free(b);
        }
    }

    #[test]
    fn areas_trace_through_c() {
        use sagax_abi::codes::{
            AREAS_TRACE_ADDED, AREAS_TRACE_EVICTED, AREAS_TRACE_EXPIRED, AREAS_TRACE_RENEWED,
        };
        let area = |x, since| SxArea {
            kind: 1,
            pos: [x, 0, 0],
            radius: 2,
            since,
            lifetime: 10,
            ..SxArea::default()
        };
        let rec = |kind, index, area| SxAreasTrace { kind, index, area };
        unsafe {
            let a = sx_areas_new(1);
            let mut records: Vec<SxAreasTrace> = Vec::new();
            let c = ctx(&mut records);
            assert_eq!(sx_areas_set_trace(a, Some(collect), c), 0);
            assert_eq!(
                sx_areas_set_trace(null_mut(), Some(collect), c),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_areas_add(a, 1, &area(0, 0)), 0);
            assert_eq!(sx_areas_add(a, 2, &area(1, 0)), 0);
            assert_eq!(sx_areas_add(a, 3, &area(9, 0)), 0);
            assert_eq!(sx_areas_expire(a, 20), 1);
            assert_eq!(
                records,
                [
                    rec(AREAS_TRACE_ADDED, 0, area(0, 1)),
                    rec(AREAS_TRACE_RENEWED, 0, area(1, 2)),
                    rec(AREAS_TRACE_EVICTED, 0, area(1, 2)),
                    rec(AREAS_TRACE_ADDED, 0, area(9, 3)),
                    rec(AREAS_TRACE_EXPIRED, 0, area(9, 3)),
                ]
            );
            assert_eq!(sx_areas_set_trace(a, None, null_mut()), 0);
            assert_eq!(sx_areas_add(a, 30, &area(0, 0)), 0);
            assert_eq!(records.len(), 5);
            sx_areas_free(a);
        }
    }

    #[test]
    fn timers_trace_through_c() {
        use sagax_abi::codes::{TIMERS_TRACE_RENEWED, TIMERS_TRACE_REPLACED, TIMERS_TRACE_STARTED};
        unsafe {
            let t = sx_timers_new(1);
            let mut records: Vec<SxTimersTrace> = Vec::new();
            let c = ctx(&mut records);
            assert_eq!(sx_timers_set_trace(t, Some(collect), c), 0);
            assert_eq!(
                sx_timers_set_trace(null_mut(), Some(collect), c),
                ERROR_ARGUMENT
            );
            assert_eq!(sx_timers_set(t, 4, 0, 10), 0);
            assert_eq!(sx_timers_set(t, 4, 2, 6), 0);
            assert_eq!(sx_timers_set(t, 5, 5, 3), 0);
            let (started, renewed) = (
                SxTimersTrace {
                    kind: TIMERS_TRACE_STARTED,
                    id: 4,
                    duration: 10,
                    ..SxTimersTrace::default()
                },
                SxTimersTrace {
                    kind: TIMERS_TRACE_RENEWED,
                    id: 4,
                    duration: 6,
                    ..SxTimersTrace::default()
                },
            );
            let replaced = SxTimersTrace {
                kind: TIMERS_TRACE_REPLACED,
                id: 5,
                duration: 3,
                old: 4,
                old_left: 3,
            };
            assert_eq!(records, [started, renewed, replaced]);
            assert_eq!(sx_timers_set_trace(t, None, null_mut()), 0);
            assert_eq!(sx_timers_set(t, 5, 9, 3), 0);
            assert_eq!(records.len(), 3);
            sx_timers_free(t);
        }
    }
}
