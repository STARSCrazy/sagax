//! `sx_squad_rules`: the game's logic for a squad or crew as C callbacks,
//! the read-only views and report actions they get, and the built-in
//! helpers, see `sagax.h`.
//!
//! A [`CRules`] wraps the table of one call and implements
//! [`SquadRules`]: a null pointer forwards to the slot's default, `int64_t`
//! costs and scores are widened to `i128`. The callbacks run inside the
//! `guarded` call that lent them.

use std::ffi::c_void;

use sagax_abi::codes::ERROR_ARGUMENT;
use sagax_abi::convert::{self, to_i32};
use sagax_decide::group::{
    ChannelCtx, CostCtx, PlaceCtx, ReportActions, RowCtx, SpotCtx, SquadRules, SquadView,
    spot_score_builtin,
};
use sagax_decide::memory::{Fact, Message, Pos};

use super::group::{SxMember, SxSpot};
use super::memory::{SxArea, SxFact, SxMessage, pos3, put};
use crate::{Begun, CallbackMap, SxMap, SxNav};
use sagax_nav::grid::Cell;
use sagax_nav::nav::Status;

// ---- value structs ------------------------------------------------------------

/// A count that does not fit `u32` saturates.
fn small(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A cost or score as `int64_t`: saturated.
fn saturate(v: i128) -> i64 {
    i64::try_from(v).unwrap_or(if v < 0 { i64::MIN } else { i64::MAX })
}

/// `sx_place_ctx`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxPlaceCtx {
    /// Index of the tactic.
    pub tactic: u32,
    /// Index of the slot.
    pub slot: u32,
    /// The target's id; valid if `has_target`.
    pub target: u32,
    /// The slot's `gather_dist` for a gather place, else `dist`.
    pub dist: i32,
    /// The acting leader's position.
    pub leader: [i32; 3],
    /// The group centre.
    pub centre: [i32; 3],
    /// What the slots face; valid if `has_face`.
    pub face: [i32; 3],
    /// The target's nearest other target; valid if `has_friend`.
    pub friend_pos: [i32; 3],
    /// Heading from the centre to the face (0 without a face).
    pub heading: u8,
    /// 1 for the slot's gather place, 0 for its place.
    pub gather: u8,
    /// 1 if `face` is set.
    pub has_face: u8,
    /// 1 if `friend_pos` is set.
    pub has_friend: u8,
    /// 1 if `target` is set.
    pub has_target: u8,
}

impl SxPlaceCtx {
    fn of(c: &PlaceCtx) -> Self {
        Self {
            tactic: small(c.tactic),
            slot: small(c.slot),
            target: c.target.unwrap_or(0),
            dist: c.dist,
            leader: c.leader,
            centre: c.centre,
            face: c.face.unwrap_or_default(),
            friend_pos: c.friend.unwrap_or_default(),
            heading: c.heading,
            gather: u8::from(c.gather),
            has_face: u8::from(c.face.is_some()),
            has_friend: u8::from(c.friend.is_some()),
            has_target: u8::from(c.target.is_some()),
        }
    }
}

/// `sx_cost_ctx`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxCostCtx {
    /// The built-in cost (saturated to `int64_t`).
    pub built_in: i64,
    /// Index of the tactic.
    pub tactic: u32,
    /// Index of the slot.
    pub slot: u32,
    /// The member.
    pub member: SxMember,
    /// The slot's first wish point.
    pub wish: [i32; 3],
}

impl SxCostCtx {
    fn of(c: &CostCtx) -> Self {
        Self {
            built_in: saturate(c.built_in),
            tactic: small(c.tactic),
            slot: small(c.slot),
            member: SxMember::of(&c.member),
            wish: c.wish,
        }
    }
}

/// `sx_spot_ctx`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SxSpotCtx {
    /// The built-in score (saturated to `int64_t`).
    pub built_in: i64,
    /// Index of the tactic.
    pub tactic: u32,
    /// Index of the slot.
    pub slot: u32,
    /// Index of the spot in the caller's list.
    pub index: u32,
    /// Tag bits wanted.
    pub want_tags: u32,
    /// The plan's target id; valid if `has_target`.
    pub target: u32,
    /// The spot.
    pub spot: SxSpot,
    /// The member that gets the spot.
    pub member: SxMember,
    /// The wish point.
    pub wish: [i32; 3],
    /// The target's position; valid if `has_target`.
    pub target_pos: [i32; 3],
    /// 1 when ranking gather spots.
    pub gather: u8,
    /// 1 if `target` and `target_pos` are set.
    pub has_target: u8,
}

impl SxSpotCtx {
    fn of(c: &SpotCtx) -> Self {
        Self {
            built_in: saturate(c.built_in),
            tactic: small(c.tactic),
            slot: small(c.slot),
            index: small(c.index),
            want_tags: c.want_tags,
            target: c.target.map_or(0, |f| f.subject),
            spot: c.spot,
            member: SxMember::of(&c.member),
            wish: c.wish,
            target_pos: c.target.map_or([0; 3], |f| f.pos),
            gather: u8::from(c.gather),
            has_target: u8::from(c.target.is_some()),
        }
    }
}

/// `sx_channel_ctx`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxChannelCtx {
    /// Who sends.
    pub sender: u32,
    /// Who receives.
    pub receiver: u32,
    /// The plain distance between them.
    pub distance: u32,
    /// When it is sent.
    pub now: u32,
    /// The sender's position.
    pub from: [i32; 3],
    /// The receiver's position.
    pub to: [i32; 3],
    /// 1 for a report, 0 for an order.
    pub report: u8,
}

impl SxChannelCtx {
    fn of(c: &ChannelCtx) -> Self {
        Self {
            sender: c.sender,
            receiver: c.receiver,
            distance: c.distance,
            now: c.now,
            from: c.from,
            to: c.to,
            report: u8::from(c.report),
        }
    }
}

/// `sx_row_ctx`: one scoring row.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SxRowCtx {
    /// The row (for `columns`: up to the rule columns); valid during the
    /// callback.
    pub row: *const i32,
    /// Number of values at `row`.
    pub row_len: u32,
    /// Index of the tactic.
    pub tactic: u32,
    /// The candidate's target id; valid if `has_target`.
    pub target: u32,
    /// The tick of the call.
    pub now: u32,
    /// 1 if the candidate has a target.
    pub has_target: u8,
    /// 1 for the row of the active (tactic, target).
    pub active: u8,
}

impl SxRowCtx {
    fn of(c: &RowCtx<'_>) -> Self {
        Self {
            row: c.row.as_ptr(),
            row_len: small(c.row.len()),
            tactic: small(c.tactic),
            target: c.target.map_or(0, |f| f.subject),
            now: c.now,
            has_target: u8::from(c.target.is_some()),
            active: u8::from(c.active),
        }
    }
}

/// `sx_view_plan`: the active plan, as a view shows it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxViewPlan {
    /// Its number.
    pub plan: u32,
    /// Index of the tactic.
    pub tactic: u32,
    /// The target's id; valid if `has_target`.
    pub target: u32,
    /// When it became active.
    pub since: u32,
    /// 1 if there is an active plan (else the other fields are 0).
    pub has_plan: u8,
    /// 1 if `target` is set.
    pub has_target: u8,
    /// 1 once the plan's signal was given.
    pub signal_given: u8,
}

// ---- the rules table ------------------------------------------------------------

/// `sx_squad_view`: a read-only look at the squad, valid during one
/// callback. Never made by the game.
#[repr(C)]
pub struct SxSquadView {
    _opaque: [u8; 0],
}

/// `sx_report_actions`: what a report callback hands back, valid during
/// that callback. Never made by the game.
#[repr(C)]
pub struct SxReportActions {
    _opaque: [u8; 0],
}

/// `keep_target` of `sx_squad_rules`.
pub type KeepTargetFn = unsafe extern "C" fn(*mut c_void, *const SxSquadView, *const SxFact) -> i32;
/// `columns` of `sx_squad_rules`.
pub type ColumnsFn =
    unsafe extern "C" fn(*mut c_void, *const SxSquadView, *const SxRowCtx, *mut i32, u32);
/// `place` of `sx_squad_rules`.
pub type PlaceFn = unsafe extern "C" fn(*mut c_void, u32, *const SxPlaceCtx, *mut i32);
/// `member_cost` of `sx_squad_rules`.
pub type MemberCostFn = unsafe extern "C" fn(*mut c_void, *const SxCostCtx, *mut i64) -> i32;
/// `spot_score` of `sx_squad_rules`.
pub type SpotScoreFn = unsafe extern "C" fn(*mut c_void, *const SxSpotCtx, *mut i64) -> i32;
/// `trigger` of `sx_squad_rules`.
pub type TriggerFn = unsafe extern "C" fn(*mut c_void, u32, *const SxSquadView, u32) -> i32;
/// `abort` of `sx_squad_rules`.
pub type AbortFn =
    unsafe extern "C" fn(*mut c_void, u32, *const SxSquadView, *const SxRowCtx) -> i32;
/// `channel_delay` of `sx_squad_rules`.
pub type ChannelDelayFn =
    unsafe extern "C" fn(*mut c_void, u32, *const SxChannelCtx, *mut u32) -> i32;
/// `report` of `sx_squad_rules`.
pub type ReportFn = unsafe extern "C" fn(
    *mut c_void,
    *const SxSquadView,
    *const SxMessage,
    u32,
    *mut SxReportActions,
);

/// `sx_squad_rules`: one callback per slot, `NULL` = the built-in.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SxSquadRules {
    /// Slot 1: keep this target for new plans (1) or drop it (0).
    pub keep_target: Option<KeepTargetFn>,
    /// Slot 2: fill the rule columns of a candidate's row.
    pub columns: Option<ColumnsFn>,
    /// Slot 3: the wish point of a custom place.
    pub place: Option<PlaceFn>,
    /// Slot 4: the cost of a member for a slot.
    pub member_cost: Option<MemberCostFn>,
    /// Slot 5: the score of a spot.
    pub spot_score: Option<SpotScoreFn>,
    /// Slot 6: whether a custom trigger fires.
    pub trigger: Option<TriggerFn>,
    /// Slot 7: whether a custom abort fires.
    pub abort: Option<AbortFn>,
    /// Slot 8: the delay of a custom channel.
    pub channel_delay: Option<ChannelDelayFn>,
    /// Slot 9: the leader reads a game report.
    pub report: Option<ReportFn>,
    /// Passed to every callback.
    pub user: *mut c_void,
}

impl Default for SxSquadRules {
    /// Every callback `NULL` (all built-in), `user` null.
    fn default() -> Self {
        Self {
            keep_target: None,
            columns: None,
            place: None,
            member_cost: None,
            spot_score: None,
            trigger: None,
            abort: None,
            channel_delay: None,
            report: None,
            user: std::ptr::null_mut(),
        }
    }
}

/// The rules of one C call: the table, as [`SquadRules`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct CRules {
    table: SxSquadRules,
}

impl CRules {
    /// The rules behind a C pointer, or `None` for null (the built-in rules:
    /// the plain call).
    ///
    /// # Safety
    /// `table` is null or valid for the call.
    pub(crate) unsafe fn of(table: *const SxSquadRules) -> Option<Self> {
        // SAFETY: null or valid (see above).
        unsafe { table.as_ref() }.map(|t| Self { table: *t })
    }
}

/// The view as the C side gets it, for the length of one callback.
fn view_ptr(view: &SquadView<'_>) -> *const SxSquadView {
    std::ptr::from_ref(view).cast()
}

// SAFETY (all calls below): the game filled the table with valid callbacks
// and a valid `user` for the duration of the call that lent it (sagax.h);
// every pointer passed is to a value that lives through the callback.
impl SquadRules for CRules {
    fn keep_target(&mut self, view: &SquadView<'_>, fact: &Fact) -> bool {
        let Some(f) = self.table.keep_target else {
            return true;
        };
        // SAFETY: see above.
        unsafe { f(self.table.user, view_ptr(view), fact) != 0 }
    }

    fn columns(&mut self, view: &SquadView<'_>, ctx: &RowCtx<'_>, out: &mut [i32]) {
        let Some(f) = self.table.columns else {
            return;
        };
        let c = SxRowCtx::of(ctx);
        // SAFETY: see above.
        unsafe {
            f(
                self.table.user,
                view_ptr(view),
                &c,
                out.as_mut_ptr(),
                small(out.len()),
            );
        }
    }

    fn place(&mut self, id: u32, ctx: &PlaceCtx) -> Pos {
        let mut out = ctx.leader;
        if let Some(f) = self.table.place {
            let c = SxPlaceCtx::of(ctx);
            // SAFETY: see above; `out` has room for 3.
            unsafe { f(self.table.user, id, &c, out.as_mut_ptr()) };
        }
        out
    }

    fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
        let Some(f) = self.table.member_cost else {
            return Some(ctx.built_in);
        };
        let c = SxCostCtx::of(ctx);
        let mut cost = 0i64;
        // SAFETY: see above.
        let usable = unsafe { f(self.table.user, &c, &mut cost) };
        (usable != 0).then_some(i128::from(cost))
    }

    fn spot_score(&mut self, ctx: &SpotCtx) -> Option<i128> {
        let Some(f) = self.table.spot_score else {
            return Some(ctx.built_in);
        };
        let c = SxSpotCtx::of(ctx);
        let mut score = 0i64;
        // SAFETY: see above.
        let usable = unsafe { f(self.table.user, &c, &mut score) };
        (usable != 0).then_some(i128::from(score))
    }

    fn trigger(&mut self, id: u32, view: &SquadView<'_>, now: u32) -> bool {
        let Some(f) = self.table.trigger else {
            return false;
        };
        // SAFETY: see above.
        unsafe { f(self.table.user, id, view_ptr(view), now) != 0 }
    }

    fn abort(&mut self, id: u32, view: &SquadView<'_>, ctx: &RowCtx<'_>) -> bool {
        let Some(f) = self.table.abort else {
            return false;
        };
        let c = SxRowCtx::of(ctx);
        // SAFETY: see above.
        unsafe { f(self.table.user, id, view_ptr(view), &c) != 0 }
    }

    fn channel_delay(&mut self, id: u32, ctx: &ChannelCtx) -> Option<u32> {
        let f = self.table.channel_delay?;
        let c = SxChannelCtx::of(ctx);
        let mut delay = 0u32;
        // SAFETY: see above.
        let arrives = unsafe { f(self.table.user, id, &c, &mut delay) };
        (arrives != 0).then_some(delay)
    }

    fn report(
        &mut self,
        view: &SquadView<'_>,
        msg: &Message,
        now: u32,
        actions: &mut ReportActions<'_>,
    ) {
        let Some(f) = self.table.report else {
            return;
        };
        let a = std::ptr::from_mut(actions).cast::<SxReportActions>();
        // SAFETY: see above; `a` is the live actions of this report.
        unsafe { f(self.table.user, view_ptr(view), msg, now, a) };
    }
}

// ---- the view -----------------------------------------------------------------

/// The view behind a C pointer, or `ERROR_ARGUMENT`.
macro_rules! view {
    ($p:expr) => {
        // SAFETY: null, or the pointer a callback got: it points to a live
        // `SquadView` for the length of that callback (sagax.h).
        match unsafe { $p.cast::<SquadView<'_>>().as_ref() } {
            Some(v) => v,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// `sx_view_member_count`: every member (also suspended and gone ones).
///
/// # Safety
/// `view` is null or the pointer a callback got.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_view_member_count(view: *const SxSquadView) -> i32 {
    to_i32(view!(view).members().count())
}

/// `sx_view_member_at`: 1, or 0 past the end.
///
/// # Safety
/// `view` is null or the pointer a callback got; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_view_member_at(
    view: *const SxSquadView,
    index: u32,
    out: *mut SxMember,
) -> i32 {
    let v = view!(view);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match v.members().nth(index as usize) {
        Some(m) => {
            // SAFETY: non-null and writable (see above).
            unsafe { put(out, SxMember::of(&m)) };
            1
        }
        None => 0,
    }
}

/// `sx_view_target_count`: every known target, also those the targets slot
/// drops.
///
/// # Safety
/// `view` is null or the pointer a callback got.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_view_target_count(view: *const SxSquadView) -> i32 {
    to_i32(view!(view).targets().count())
}

/// `sx_view_target_at`: 1, or 0 past the end.
///
/// # Safety
/// `view` is null or the pointer a callback got; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_view_target_at(
    view: *const SxSquadView,
    index: u32,
    out: *mut SxFact,
) -> i32 {
    let v = view!(view);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match v.targets().nth(index as usize) {
        Some(f) => {
            // SAFETY: non-null and writable (see above).
            unsafe { put(out, f) };
            1
        }
        None => 0,
    }
}

/// `sx_view_active_plan`: writes the active plan to `out` (all 0 without one);
/// returns 1 with a plan, else 0.
///
/// # Safety
/// `view` is null or the pointer a callback got; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_view_active_plan(
    view: *const SxSquadView,
    out: *mut SxViewPlan,
) -> i32 {
    let v = view!(view);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    let plan = v
        .active_plan()
        .map_or_else(SxViewPlan::default, |p| SxViewPlan {
            plan: p.id,
            tactic: small(p.tactic),
            target: p.target.unwrap_or(0),
            since: p.since,
            has_plan: 1,
            has_target: u8::from(p.target.is_some()),
            signal_given: u8::from(p.signal_given),
        });
    // SAFETY: non-null and writable (see above).
    unsafe { put(out, plan) };
    i32::from(plan.has_plan)
}

/// `sx_view_centre`: the group centre to `out` (3 values); 1, or 0 without
/// an active member.
///
/// # Safety
/// `view` is null or the pointer a callback got; `out` is null or has room
/// for 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_view_centre(view: *const SxSquadView, out: *mut i32) -> i32 {
    let v = view!(view);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match v.centre() {
        Some(c) => {
            // SAFETY: room for 3 (see above).
            unsafe { out.cast::<[i32; 3]>().write_unaligned(c) };
            1
        }
        None => 0,
    }
}

// ---- report actions -------------------------------------------------------------

/// The actions behind a C pointer, or `ERROR_ARGUMENT`.
macro_rules! actions {
    ($p:expr) => {
        // SAFETY: null, or the pointer a report callback got: it points to
        // live actions for the length of that callback (sagax.h).
        match unsafe { $p.cast::<ReportActions<'_>>().as_mut() } {
            Some(a) => a,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// `sx_report_add_area`: 1 stored, 0 when the report has its 4 actions.
///
/// # Safety
/// `actions` is null or the pointer a report callback got; `area` is null
/// or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_report_add_area(
    actions: *mut SxReportActions,
    area: *const SxArea,
) -> i32 {
    let a = actions!(actions);
    // SAFETY: null or valid (see above).
    let Some(area) = (unsafe { area.as_ref() }) else {
        return ERROR_ARGUMENT;
    };
    i32::from(a.add_area(*area))
}

/// `sx_report_signal`: 1 stored, 0 when full.
///
/// # Safety
/// `actions` is null or the pointer a report callback got.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_report_signal(actions: *mut SxReportActions) -> i32 {
    i32::from(actions!(actions).signal())
}

/// `sx_report_forget`: 1 stored, 0 when full.
///
/// # Safety
/// `actions` is null or the pointer a report callback got.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_report_forget(actions: *mut SxReportActions, subject: u32) -> i32 {
    i32::from(actions!(actions).forget(subject))
}

// ---- helpers --------------------------------------------------------------------

/// `sx_builtin_member_cost`: `*cost = ctx->built_in` (saturated), usable.
///
/// # Safety
/// `ctx` and `cost` are null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_builtin_member_cost(
    _user: *mut c_void,
    ctx: *const SxCostCtx,
    cost: *mut i64,
) -> i32 {
    // SAFETY: null or valid (see above).
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        return 0;
    };
    if cost.is_null() {
        return 0;
    }
    // SAFETY: non-null and writable (see above).
    unsafe { cost.write(ctx.built_in) };
    1
}

/// `sx_path_cost`: the cost of the way from `from` to `to` (3 integers
/// each) to `*cost`; 1 found, 0 not reachable or `budget` (cells, 0 =
/// unlimited) used up (`*cost` untouched). Replaces the navigator's last
/// search.
///
/// # Safety
/// `nav` and `map` are null or valid (the callbacks too, for the call);
/// `from` and `to` are null or hold 3 integers; `cost` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_path_cost(
    nav: *mut SxNav,
    map: *const SxMap,
    from: *const i32,
    to: *const i32,
    budget: i32,
    cost: *mut i64,
) -> i32 {
    // SAFETY: null or valid (see above).
    let (Some(s), Some(mut map)) = (unsafe { nav.as_mut() }, CallbackMap::new(map)) else {
        return ERROR_ARGUMENT;
    };
    let Some(budget) = convert::budget(budget) else {
        return ERROR_ARGUMENT;
    };
    if from.is_null() || to.is_null() || cost.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: 3 integers each (see above).
    let (from, to) = unsafe { (pos3(from), pos3(to)) };
    crate::guarded(|| {
        s.begun = Begun::Nothing;
        // The same as `sagax::rules::path_cost`.
        let report = crate::traced!(s.trace, crate::SxNavTrace::of, |sink| s
            .nav
            .find_path_traced(&mut map, Cell::from(from), &[Cell::from(to)], budget, sink));
        if report.status != Status::Found {
            return 0;
        }
        // SAFETY: non-null and writable (see above).
        unsafe { cost.write(i64::from(report.cost)) };
        1
    })
}

/// `sx_builtin_spot_score`: `*score = ctx->built_in` (saturated), usable.
///
/// # Safety
/// `ctx` and `score` are null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_builtin_spot_score(
    _user: *mut c_void,
    ctx: *const SxSpotCtx,
    score: *mut i64,
) -> i32 {
    // SAFETY: null or valid (see above).
    let Some(ctx) = (unsafe { ctx.as_ref() }) else {
        return 0;
    };
    if score.is_null() {
        return 0;
    }
    // SAFETY: non-null and writable (see above).
    unsafe { score.write(ctx.built_in) };
    1
}

/// `sx_squad_rules_default`: every callback and `user` `NULL`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_rules_default(out: *mut SxSquadRules) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: non-null and writable (see above).
    unsafe {
        put(out, SxSquadRules::default());
    }
    0
}

/// `sx_spot_score_builtin`: the built-in spot score to `out`, saturated.
///
/// # Safety
/// `wish` is null or 3 readable values; `spot` is null or valid; `out` is
/// null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_spot_score_builtin(
    wish: *const i32,
    want_tags: u32,
    spot: *const SxSpot,
    metric: u32,
    out: *mut i64,
) -> i32 {
    // SAFETY: null or valid (see above).
    let Some(spot) = (unsafe { spot.as_ref() }) else {
        return ERROR_ARGUMENT;
    };
    if wish.is_null() || out.is_null() {
        return ERROR_ARGUMENT;
    }
    let Some(metric) = convert::metric(metric) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: 3 readable values (see above).
    let wish = unsafe { pos3(wish) };
    // SAFETY: non-null and writable (see above).
    unsafe { out.write(saturate(spot_score_builtin(wish, want_tags, spot, metric))) };
    0
}

#[cfg(test)]
mod tests {
    //! The rules object through C: the Rust scenes of the squad and crew
    //! tests (`group/rules_tests.rs`, `group/crew_tests.rs`) with C callbacks
    //! give the same plans, statuses and ticks.

    use super::*;
    use crate::crew::*;
    use crate::decide::SxConsideration;
    use crate::group::*;
    use crate::memory::{sx_areas_at, sx_areas_count};
    use sagax_abi::codes::*;
    use sagax_decide::group::{
        Channel, Crew, CrewConfig, CrewEvent, IN_TARGET_DIST, IN_TARGET_FRIEND_DIST, Member,
        MemberState, REPORT_GAME_FIRST, TAG_COVER, TAG_HIDDEN, TAG_HIGH, tactics,
    };
    use sagax_decide::memory::{Message, Metric};
    use std::ptr::{null, null_mut};

    const ENEMY: u32 = 1;
    const ONE: u32 = 65535;
    const ROLES: [u32; 8] = [1, 2, 0, 0, 0, 0, 0, 0];
    const SEED: u64 = 7;

    fn user<T>(state: &mut T) -> *mut c_void {
        std::ptr::from_mut(state).cast()
    }

    fn config(metric: u32) -> SxSquadConfig {
        SxSquadConfig {
            members: 8,
            memory: 8,
            areas: 4,
            inbox: 8,
            dangers: 8,
            metric,
            role_count: 2,
            role_masks: ROLES,
            rule_columns: 0,
        }
    }

    fn member(id: u32, x: i32, y: i32) -> SxMember {
        SxMember {
            id,
            pos: [x, y, 0],
            ..SxMember::default()
        }
    }

    fn fact(subject: u32, x: i32, y: i32, now: u32) -> SxFact {
        SxFact {
            subject,
            kind: ENEMY,
            pos: [x, y, 0],
            seen_at: now,
            ..SxFact::default()
        }
    }

    /// A squad of `members` (the first leads), skill 255, target kind
    /// [`ENEMY`], no tactics.
    unsafe fn squad(metric: u32, members: &[(u32, i32, i32)]) -> *mut SxSquad {
        unsafe {
            let sq = sx_squad_new(&config(metric));
            assert!(!sq.is_null());
            for &(id, x, y) in members {
                assert!(sx_squad_add_member(sq, &member(id, x, y)) >= 0);
            }
            assert_eq!(sx_squad_set_leader(sq, members[0].0), 1);
            assert_eq!(sx_squad_set_target_kind(sq, ENEMY), 0);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            sq
        }
    }

    unsafe fn see(sq: *mut SxSquad, id: u32, x: i32, y: i32, now: u32) {
        unsafe { assert!(sx_squad_observe(sq, now, &fact(id, x, y, now), null_mut()) >= 0) };
    }

    fn place(kind: u32, value: u32) -> SxPlace {
        SxPlace {
            kind,
            value,
            ..SxPlace::default()
        }
    }

    /// The slot of the shipped tactics: anyone, 1 to `max` members.
    fn slot(place: SxPlace, max: u8) -> SxSlot {
        SxSlot {
            min: 1,
            max,
            place,
            gather: place_none(),
            kind: GOAL_ATTACK,
            dist: 1,
            gather_dist: 2,
            ..SxSlot::default()
        }
    }

    const fn place_none() -> SxPlace {
        SxPlace {
            kind: PLACE_NONE,
            x: 0,
            y: 0,
            z: 0,
            value: 0,
        }
    }

    /// Rush with one slot at `place` (the shipped rush has the front of the
    /// target and up to 255 members).
    unsafe fn tactic(place: SxPlace, max: u8) -> *mut SxTactic {
        unsafe {
            let t = sx_tactic_new(ONE / 4, 0, 1);
            assert!(!t.is_null());
            assert_eq!(sx_tactic_add_slot(t, &slot(place, max)), 0);
            t
        }
    }

    unsafe fn add(sq: *mut SxSquad, t: *mut SxTactic) {
        unsafe { assert!(sx_squad_add_tactic(sq, t) >= 0) };
    }

    /// The pending plan with its goals.
    unsafe fn pending(sq: *mut SxSquad) -> (SxPlan, Vec<SxOrder>) {
        unsafe {
            let mut plan = SxPlan::default();
            let mut goals = [SxOrder::default(); 16];
            assert_eq!(
                sx_squad_pending_plan(sq, &mut plan, goals.as_mut_ptr(), 16),
                1
            );
            let n = (plan.goal_count as usize).min(16);
            (plan, goals[..n].to_vec())
        }
    }

    unsafe fn active(sq: *mut SxSquad) -> SxPlan {
        unsafe {
            let mut plan = SxPlan::default();
            assert_eq!(sx_squad_active_plan(sq, &mut plan, null_mut(), 0), 1);
            plan
        }
    }

    unsafe fn plan_with(sq: *mut SxSquad, now: u32, rules: *const SxSquadRules) -> i32 {
        unsafe { sx_squad_plan_with(sq, now, null(), 0, null(), 0, 0, rules, null_mut()) }
    }

    unsafe fn check_with(sq: *mut SxSquad, now: u32, rules: *const SxSquadRules) -> i32 {
        unsafe { sx_squad_check_with(sq, now, null(), 0, null(), 0, 0, rules, null_mut()) }
    }

    /// The orders due at `now`, as `(member, now)`.
    unsafe fn due(sq: *mut SxSquad, now: u32) -> Vec<(u32, u32)> {
        unsafe {
            let mut out = [SxOrder::default(); 16];
            let n = sx_squad_orders_due(sq, now, out.as_mut_ptr(), 16);
            assert!(n >= 0);
            out[..n as usize].iter().map(|o| (o.member, now)).collect()
        }
    }

    // ---- null, zeroed and default rules are the built-in ones ----------------

    fn flank_spots() -> Vec<SxSpot> {
        let spot = |x, y, tags| SxSpot {
            pos: [x, y, 0],
            tags,
        };
        vec![
            spot(10, 3, TAG_COVER),
            spot(-6, 0, TAG_COVER),
            spot(2, 2, TAG_HIDDEN),
            spot(3, -1, TAG_HIDDEN),
            spot(20, 14, 8),
            spot(16, 4, 0),
            spot(24, -4, TAG_HIGH),
        ]
    }

    /// What one run of the flank scene did: per tick the check result, the
    /// new plan and its goals, the orders that came due.
    type Run = Vec<(i32, Option<(SxPlan, Vec<SxOrder>)>, Vec<(u32, u32)>)>;

    /// The flank scene (see `scene` in the Rust rules tests), 120 checks, each
    /// through `call`.
    unsafe fn flank_run(skill: u32, call: unsafe fn(*mut SxSquad, u32, &[SxSpot]) -> i32) -> Run {
        unsafe {
            let sq = squad(EUCLID, &[(1, 0, 0), (2, 1, 0), (3, 0, 1), (4, 1, 1)]);
            assert_eq!(sx_squad_add_flank(sq, 3, 4), 0);
            see(sq, 10, 20, 0, 0);
            see(sq, 11, 22, 6, 0);
            assert!(sx_squad_add_rush(sq) >= 0);
            assert_eq!(sx_squad_set_skill(sq, skill), 0);
            let spots = flank_spots();
            let mut run = Vec::new();
            for now in 0..120 {
                match now {
                    8 => see(sq, 10, 12, 4, now),
                    15 => assert_eq!(sx_squad_set_gone(sq, 3), 1),
                    18 => assert_eq!(sx_squad_set_gone(sq, 2), 1),
                    22 => see(sq, 10, 4, 0, now),
                    _ => {}
                }
                let code = call(sq, now, &spots);
                assert!(code >= 0, "{code}");
                let made = (code == SQUAD_REPLAN).then(|| pending(sq));
                run.push((code, made, due(sq, now)));
            }
            sx_squad_free(sq);
            run
        }
    }

    const EUCLID: u32 = METRIC_EUCLID;
    const CHEBYSHEV: u32 = METRIC_CHEBYSHEV;

    unsafe fn call_plain(sq: *mut SxSquad, now: u32, spots: &[SxSpot]) -> i32 {
        unsafe {
            sx_squad_check(
                sq,
                now,
                null(),
                0,
                spots.as_ptr(),
                spots.len() as u32,
                SEED,
                null_mut(),
            )
        }
    }

    unsafe fn call_null(sq: *mut SxSquad, now: u32, spots: &[SxSpot]) -> i32 {
        unsafe {
            sx_squad_check_with(
                sq,
                now,
                null(),
                0,
                spots.as_ptr(),
                spots.len() as u32,
                SEED,
                null(),
                null_mut(),
            )
        }
    }

    unsafe fn call_zeroed(sq: *mut SxSquad, now: u32, spots: &[SxSpot]) -> i32 {
        unsafe {
            let rules = SxSquadRules::default();
            sx_squad_check_with(
                sq,
                now,
                null(),
                0,
                spots.as_ptr(),
                spots.len() as u32,
                SEED,
                &rules,
                null_mut(),
            )
        }
    }

    unsafe fn call_default(sq: *mut SxSquad, now: u32, spots: &[SxSpot]) -> i32 {
        unsafe {
            let mut rules = SxSquadRules::default();
            assert_eq!(sx_squad_rules_default(&mut rules), 0);
            sx_squad_check_with(
                sq,
                now,
                null(),
                0,
                spots.as_ptr(),
                spots.len() as u32,
                SEED,
                &rules,
                null_mut(),
            )
        }
    }

    #[test]
    fn c_rules_null_is_builtin() {
        unsafe {
            for skill in [0, 255] {
                let plain = flank_run(skill, call_plain);
                assert!(plain.iter().any(|r| r.1.is_some()), "the scene replans");
                assert!(plain.iter().any(|r| !r.2.is_empty()), "orders came due");
                assert_eq!(flank_run(skill, call_null), plain, "NULL, skill {skill}");
                assert_eq!(
                    flank_run(skill, call_zeroed),
                    plain,
                    "zeroed, skill {skill}"
                );
                assert_eq!(
                    flank_run(skill, call_default),
                    plain,
                    "default, skill {skill}"
                );
            }
            let mut rules = SxSquadRules::default();
            assert_eq!(sx_squad_rules_default(&mut rules), 0);
            assert!(rules.keep_target.is_none() && rules.columns.is_none());
            assert!(rules.place.is_none() && rules.trigger.is_none());
            assert!(rules.abort.is_none() && rules.channel_delay.is_none());
            assert!(rules.report.is_none() && rules.user.is_null());
            assert!(rules.member_cost.is_none() && rules.spot_score.is_none());
            assert_eq!(sx_squad_rules_default(null_mut()), ERROR_ARGUMENT);
        }
    }

    // ---- the slots, as the Rust scenes -------------------------------------------

    /// `place_custom_gets_ctx`.
    struct PlaceAt {
        at: [i32; 3],
        seen: Vec<(u32, SxPlaceCtx)>,
    }

    unsafe extern "C" fn place_cb(
        user: *mut c_void,
        id: u32,
        ctx: *const SxPlaceCtx,
        out: *mut i32,
    ) {
        unsafe {
            let st = &mut *user.cast::<PlaceAt>();
            st.seen.push((id, *ctx));
            out.cast::<[i32; 3]>().write(st.at);
        }
    }

    unsafe fn scene_place_custom() {
        unsafe {
            let sq = squad(CHEBYSHEV, &[(1, 2, 0), (2, 0, 0)]);
            add(sq, tactic(place(PLACE_CUSTOM, 5), 1));
            see(sq, 10, 10, 0, 0);
            let mut st = PlaceAt {
                at: [3, 4, 0],
                seen: Vec::new(),
            };
            let rules = SxSquadRules {
                place: Some(place_cb),
                user: user(&mut st),
                ..SxSquadRules::default()
            };
            assert_eq!(plan_with(sq, 0, &rules), 1);
            assert_eq!(st.seen.len(), 1);
            let (id, ctx) = st.seen[0];
            assert_eq!(id, 5);
            assert_eq!(ctx.leader, [2, 0, 0]);
            assert_eq!(ctx.centre, [1, 0, 0]);
            assert_eq!((ctx.has_face, ctx.face), (1, [10, 0, 0]));
            assert_eq!((ctx.has_target, ctx.target), (1, 10));
            assert_eq!((ctx.tactic, ctx.slot, ctx.gather), (0, 0, 0));
            let (_, goals) = pending(sq);
            assert_eq!(goals[0].goal.pos, [3, 4, 0]);
            assert_eq!((goals[0].goal.has_pos, goals[0].goal.unchecked), (1, 1));
            // Without the callback a custom place is the leader's position.
            let none = SxSquadRules::default();
            assert_eq!(plan_with(sq, 0, &none), 1);
            assert_eq!(pending(sq).1[0].goal.pos, [2, 0, 0]);
            sx_squad_free(sq);
        }
    }

    /// `member_cost_swaps_assignment`.
    struct Costs(fn(u32, u32, i64) -> Option<i64>);

    unsafe extern "C" fn cost_cb(user: *mut c_void, ctx: *const SxCostCtx, cost: *mut i64) -> i32 {
        unsafe {
            let f = (*user.cast::<Costs>()).0;
            match f((*ctx).slot, (*ctx).member.id, (*ctx).built_in) {
                Some(c) => {
                    cost.write(c);
                    1
                }
                None => 0,
            }
        }
    }

    unsafe fn scene_member_cost() {
        unsafe {
            let sq = squad(CHEBYSHEV, &[(1, 0, 0), (2, 5, 0)]);
            add(sq, tactic(place(PLACE_HERE, 0), 1));
            see(sq, 10, 10, 0, 0);
            let members = |sq| pending(sq).1.iter().map(|o| o.member).collect::<Vec<_>>();
            assert_eq!(plan_with(sq, 0, null()), 1);
            assert_eq!(members(sq), [1]);
            let mut negated = Costs(|_, _, d| Some(-d));
            let rules = SxSquadRules {
                member_cost: Some(cost_cb),
                user: user(&mut negated),
                ..SxSquadRules::default()
            };
            assert_eq!(plan_with(sq, 0, &rules), 1);
            assert_eq!(members(sq), [2]);
            // Nobody may take the slot: the plan is still made, without goals.
            let mut nobody = Costs(|_, _, _| None);
            let rules = SxSquadRules {
                user: user(&mut nobody),
                ..rules
            };
            assert_eq!(plan_with(sq, 0, &rules), 1);
            assert_eq!(members(sq), Vec::<u32>::new());
            sx_squad_free(sq);
        }
    }

    /// `spot_score_prefers_hidden`: spot 0 gets a penalty, or no spot is
    /// usable.
    struct Scores(bool);

    unsafe extern "C" fn score_cb(
        user: *mut c_void,
        ctx: *const SxSpotCtx,
        score: *mut i64,
    ) -> i32 {
        unsafe {
            let ctx = &*ctx;
            match ((*user.cast::<Scores>()).0, ctx.index) {
                (true, 0) => score.write(ctx.built_in + 1000),
                (true, _) => score.write(ctx.built_in),
                (false, _) => return 0,
            }
            1
        }
    }

    unsafe fn scene_spot_score() {
        unsafe {
            let sq = squad(CHEBYSHEV, &[(1, 0, 0)]);
            add(sq, tactic(place(PLACE_HERE, 0), 1));
            assert_eq!(sx_squad_set_spot_radius(sq, 10), 0);
            see(sq, 10, 10, 0, 0);
            let spots = [
                SxSpot {
                    pos: [1, 0, 0],
                    tags: 0,
                },
                SxSpot {
                    pos: [4, 0, 0],
                    tags: 0,
                },
            ];
            let plan = |rules: *const SxSquadRules| {
                assert_eq!(
                    sx_squad_plan_with(sq, 0, null(), 0, spots.as_ptr(), 2, 0, rules, null_mut()),
                    1
                );
                pending(sq).1[0].goal
            };
            let g = plan(null());
            assert_eq!((g.pos, g.unchecked), ([1, 0, 0], 0));
            let mut penalty = Scores(true);
            let rules = SxSquadRules {
                spot_score: Some(score_cb),
                user: user(&mut penalty),
                ..SxSquadRules::default()
            };
            let g = plan(&rules);
            assert_eq!((g.pos, g.unchecked), ([4, 0, 0], 0));
            let mut none = Scores(false);
            let rules = SxSquadRules {
                user: user(&mut none),
                ..rules
            };
            let g = plan(&rules);
            assert_eq!((g.pos, g.unchecked), ([0, 0, 0], 1));
            sx_squad_free(sq);
        }
    }

    /// What a view showed inside a callback.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Snapshot {
        members: i32,
        targets: i32,
        first_member: SxMember,
        first_target: SxFact,
        past_member: i32,
        past_target: i32,
        plan_ret: i32,
        plan: SxViewPlan,
        centre_ret: i32,
        centre: [i32; 3],
    }

    /// `trigger_custom_fires_signal`: fires from tick `from` on; records the
    /// active plan the view shows at every call.
    struct FireFrom {
        from: u32,
        seen: Vec<(u32, u32, SxViewPlan)>,
        snapshot: Option<Snapshot>,
    }

    unsafe extern "C" fn trigger_cb(
        user: *mut c_void,
        id: u32,
        view: *const SxSquadView,
        now: u32,
    ) -> i32 {
        unsafe {
            let st = &mut *user.cast::<FireFrom>();
            let mut s = Snapshot {
                members: sx_view_member_count(view),
                targets: sx_view_target_count(view),
                ..Snapshot::default()
            };
            assert_eq!(sx_view_member_at(view, 0, &mut s.first_member), 1);
            assert_eq!(sx_view_target_at(view, 0, &mut s.first_target), 1);
            s.past_member = sx_view_member_at(view, 99, &mut s.first_member.clone());
            s.past_target = sx_view_target_at(view, 99, &mut s.first_target.clone());
            s.plan_ret = sx_view_active_plan(view, &mut s.plan);
            s.centre_ret = sx_view_centre(view, s.centre.as_mut_ptr());
            st.seen.push((id, now, s.plan));
            st.snapshot.get_or_insert(s);
            i32::from(now >= st.from)
        }
    }

    /// The trigger scene: line squad 0, 2, 4, rush with `Trigger::Custom(3)`,
    /// statuses from tick 1 to 42.
    unsafe fn trigger_run(metric: u32) -> (Vec<i32>, FireFrom, *mut SxSquad) {
        unsafe {
            let sq = squad(metric, &[(1, 0, 0), (2, 2, 0), (3, 4, 0)]);
            let t = tactic(place(PLACE_SIDE, SIDE_FRONT), 255);
            assert_eq!(sx_tactic_trigger_custom(t, 3), 0);
            add(sq, t);
            see(sq, 10, 20, 0, 0);
            let mut st = FireFrom {
                from: 40,
                seen: Vec::new(),
                snapshot: None,
            };
            let rules = SxSquadRules {
                trigger: Some(trigger_cb),
                user: user(&mut st),
                ..SxSquadRules::default()
            };
            assert_eq!(plan_with(sq, 0, &rules), 1);
            let statuses = (1..=42).map(|now| check_with(sq, now, &rules)).collect();
            (statuses, st, sq)
        }
    }

    unsafe fn scene_trigger() {
        unsafe {
            let (statuses, st, sq) = trigger_run(CHEBYSHEV);
            assert_eq!(statuses[38], SQUAD_KEEP, "tick 39");
            assert_eq!(statuses[39], SQUAD_SIGNAL, "tick 40");
            assert_eq!(statuses[40], SQUAD_KEEP, "tick 41");
            assert!(statuses[..39].iter().all(|&s| s == SQUAD_KEEP));
            assert_eq!(sx_squad_signal_given(sq), 1);
            // Asked from activation (tick 5) to the signal, never after it.
            assert_eq!(st.seen.len(), 36);
            let (id, now, plan) = *st.seen.last().unwrap();
            assert_eq!((id, now), (3, 40));
            assert_eq!(plan.plan, active(sq).id);
            assert_eq!((plan.tactic, plan.target, plan.since), (0, 10, 5));
            assert_eq!(
                (plan.has_plan, plan.has_target, plan.signal_given),
                (1, 1, 0)
            );
            sx_squad_free(sq);
        }
    }

    /// `abort_custom_replans`: records `(id, tactic, target, active, now)`.
    struct AbortFrom {
        from: u32,
        seen: Vec<(u32, u32, Option<u32>, bool, u32)>,
    }

    unsafe extern "C" fn abort_cb(
        user: *mut c_void,
        id: u32,
        _view: *const SxSquadView,
        ctx: *const SxRowCtx,
    ) -> i32 {
        unsafe {
            let st = &mut *user.cast::<AbortFrom>();
            let c = &*ctx;
            assert!(!c.row.is_null() && c.row_len >= 8);
            st.seen.push((
                id,
                c.tactic,
                (c.has_target != 0).then_some(c.target),
                c.active != 0,
                c.now,
            ));
            i32::from(id == 1 && c.now >= st.from)
        }
    }

    unsafe fn abort_run(metric: u32) -> (Vec<i32>, AbortFrom, *mut SxSquad) {
        unsafe {
            let sq = squad(metric, &[(1, 0, 0), (2, 2, 0), (3, 4, 0)]);
            // The ambush with only a custom abort.
            let t = sx_tactic_new(ONE * 3 / 4, 128, 1);
            let ambush = SxSlot {
                start: START_ON_SIGNAL,
                dist: 4,
                ..slot(place(PLACE_TAGGED, TAG_HIDDEN), 255)
            };
            assert_eq!(sx_tactic_add_slot(t, &ambush), 0);
            let linear = SxConsideration {
                input: IN_TARGET_DIST as u32,
                curve: 0,
                min: 0,
                max: 100,
            };
            assert_eq!(sx_tactic_add_consideration(t, &linear), 0);
            assert_eq!(sx_tactic_add_abort_custom(t, 1), 0);
            assert_eq!(sx_tactic_trigger_target_in_radius(t, 6), 0);
            add(sq, t);
            add(sq, tactic(place(PLACE_SIDE, SIDE_FRONT), 255));
            assert_eq!(sx_squad_set_score_log(sq, 1), 0);
            see(sq, 10, 60, 0, 0);
            see(sq, 11, 50, 0, 0);
            let mut st = AbortFrom {
                from: 30,
                seen: Vec::new(),
            };
            let rules = SxSquadRules {
                abort: Some(abort_cb),
                user: user(&mut st),
                ..SxSquadRules::default()
            };
            assert_eq!(plan_with(sq, 0, &rules), 1);
            let (p, _) = pending(sq);
            assert_eq!((p.tactic, p.target), (0, 10));
            let statuses = (1..=30).map(|now| check_with(sq, now, &rules)).collect();
            (statuses, st, sq)
        }
    }

    unsafe fn scene_abort() {
        unsafe {
            let (statuses, st, sq) = abort_run(CHEBYSHEV);
            assert!(statuses[..29].iter().all(|&s| s == SQUAD_KEEP));
            assert_eq!(statuses[29], SQUAD_REPLAN, "tick 30");
            let (p, _) = pending(sq);
            assert_eq!(p.tactic, 1);
            let at_30: Vec<_> = st.seen.iter().filter(|c| c.4 == 30).copied().collect();
            // First the active row (in the check), then the candidates of the
            // replan: the active pair is still the active row, the other not.
            assert_eq!(
                at_30,
                [
                    (1, 0, Some(10), true, 30),
                    (1, 0, Some(10), true, 30),
                    (1, 0, Some(11), false, 30),
                ]
            );
            assert!(st.seen.contains(&(1, 0, Some(11), false, 0)));
            // The ambush candidates scored 0 while the abort fired: only
            // rush was scored.
            let mut log = [SxScoreEvent::default(); 32];
            let mut total = 0;
            let n = sx_squad_score_log(sq, log.as_mut_ptr(), 32, &mut total);
            assert!(n > 0);
            assert!(log[..n as usize].iter().all(|e| e.tactic == 1));
            sx_squad_free(sq);
        }
    }

    /// `channel_custom_times_orders`: `per * distance`, or never.
    struct Delay {
        per: Option<u32>,
        seen: Vec<(u32, SxChannelCtx)>,
    }

    unsafe extern "C" fn delay_cb(
        user: *mut c_void,
        id: u32,
        ctx: *const SxChannelCtx,
        delay: *mut u32,
    ) -> i32 {
        unsafe {
            let st = &mut *user.cast::<Delay>();
            st.seen.push((id, *ctx));
            match st.per {
                Some(p) => {
                    delay.write(p * (*ctx).distance);
                    1
                }
                None => 0,
            }
        }
    }

    fn delay_rules(st: &mut Delay) -> SxSquadRules {
        SxSquadRules {
            channel_delay: Some(delay_cb),
            user: user(st),
            ..SxSquadRules::default()
        }
    }

    /// Leader at 0, member 2 at 4 on `two`, member 3 at 5 on a radio,
    /// member 4 at 6 on a shout; the orders due from tick 1 to 20.
    unsafe fn channel_run(custom: bool, st: &mut Delay) -> (Vec<(u32, u32)>, *mut SxSquad) {
        unsafe {
            let sq = squad(CHEBYSHEV, &[(1, 0, 0), (2, 4, 0), (3, 5, 0), (4, 6, 0)]);
            add(sq, tactic(place(PLACE_SIDE, SIDE_FRONT), 255));
            if custom {
                assert_eq!(sx_squad_set_channel_custom(sq, 2, 2), 1);
            }
            assert_eq!(sx_squad_set_radio(sq, 3, 1, 1), 1);
            assert_eq!(sx_squad_set_shout(sq, 4, 10), 1);
            see(sq, 10, 20, 0, 0);
            let rules = delay_rules(st);
            assert_eq!(plan_with(sq, 0, &rules), 1);
            let mut orders = Vec::new();
            for now in 1..=20 {
                check_with(sq, now, &rules);
                orders.extend(due(sq, now));
            }
            (orders, sq)
        }
    }

    unsafe fn scene_channel() {
        unsafe {
            let mut rules = Delay {
                per: Some(2),
                seen: Vec::new(),
            };
            let (orders, sq) = channel_run(true, &mut rules);
            // Active at 5: the leader at once, member 2 after 2 * 4.
            assert!(orders.contains(&(1, 5)));
            assert!(orders.contains(&(2, 13)));
            assert_eq!(rules.seen.len(), 1);
            let (id, ctx) = rules.seen[0];
            assert_eq!(id, 2);
            assert_eq!(
                ctx,
                SxChannelCtx {
                    sender: 1,
                    receiver: 2,
                    distance: 4,
                    now: 5,
                    from: [0, 0, 0],
                    to: [4, 0, 0],
                    report: 0,
                }
            );
            sx_squad_free(sq);
            // Radio and shout keep the ticks they have with a plain squad.
            let mut plain = Delay {
                per: None,
                seen: Vec::new(),
            };
            let (today, sq) = channel_run(false, &mut plain);
            let others = |o: &[(u32, u32)]| -> Vec<(u32, u32)> {
                o.iter()
                    .copied()
                    .filter(|&(m, _)| m == 3 || m == 4)
                    .collect()
            };
            assert_eq!(others(&orders), others(&today));
            // Radio 1 + 5, shout 6 (in goal order).
            assert_eq!(others(&today), [(4, 11), (3, 11)]);
            sx_squad_free(sq);
            // A custom channel that does not arrive: no order for member 2.
            let mut never = Delay {
                per: None,
                seen: Vec::new(),
            };
            let (lost, sq) = channel_run(true, &mut never);
            assert!(lost.iter().all(|&(m, _)| m != 2));
            assert_eq!(never.seen.len(), 1);
            // Reports over the custom channel, from the member to the leader.
            let mut at = 0;
            assert_eq!(sx_squad_report_arrives_at(sq, 2, 30, &mut at), 0);
            rules.seen.clear();
            let table = delay_rules(&mut rules);
            assert_eq!(
                sx_squad_report_arrives_at_with(sq, 2, 30, &table, &mut at),
                1
            );
            assert_eq!(at, 38);
            assert_eq!(
                sx_squad_report_arrives_at_with(sq, 1, 30, &table, &mut at),
                1
            );
            assert_eq!(at, 30);
            assert_eq!(
                rules.seen,
                [(
                    2,
                    SxChannelCtx {
                        sender: 2,
                        receiver: 1,
                        distance: 4,
                        now: 30,
                        from: [4, 0, 0],
                        to: [0, 0, 0],
                        report: 1,
                    }
                )]
            );
            sx_squad_free(sq);
        }
    }

    /// `report_actions_apply_after_all_reports`: A adds an area (and signals
    /// if `signal`), B forgets target `forget`, FULL tries five actions.
    const KIND_A: u32 = 16;
    const KIND_B: u32 = 17;
    const KIND_FULL: u32 = 18;

    #[derive(Default)]
    struct Acts {
        signal: bool,
        forget: Option<u32>,
        pushed: Vec<i32>,
    }

    fn area_a() -> SxArea {
        SxArea {
            kind: 3,
            pos: [9, 9, 0],
            radius: 1,
            lifetime: 100,
            ..SxArea::default()
        }
    }

    unsafe extern "C" fn report_cb(
        user: *mut c_void,
        _view: *const SxSquadView,
        msg: *const SxMessage,
        _now: u32,
        actions: *mut SxReportActions,
    ) {
        unsafe {
            let st = &mut *user.cast::<Acts>();
            match (*msg).kind {
                KIND_A => {
                    assert_eq!(sx_report_add_area(actions, &area_a()), 1);
                    if st.signal {
                        assert_eq!(sx_report_signal(actions), 1);
                    }
                }
                KIND_B => {
                    if let Some(s) = st.forget {
                        assert_eq!(sx_report_forget(actions, s), 1);
                    }
                }
                KIND_FULL => {
                    for s in 0..5 {
                        st.pushed.push(sx_report_forget(actions, 900 + s));
                    }
                }
                _ => {}
            }
        }
    }

    fn msg(kind: u32, sender: u32) -> SxMessage {
        SxMessage {
            kind,
            sender,
            ..SxMessage::default()
        }
    }

    /// Rush on target 10 (target 11 farther), active at 5.
    unsafe fn acts_squad(rules: &SxSquadRules) -> *mut SxSquad {
        unsafe {
            let sq = squad(CHEBYSHEV, &[(1, 0, 0), (2, 2, 0)]);
            add(sq, tactic(place(PLACE_SIDE, SIDE_FRONT), 255));
            see(sq, 10, 20, 0, 0);
            see(sq, 11, 25, 0, 0);
            assert_eq!(plan_with(sq, 0, rules), 1);
            assert_eq!(check_with(sq, 5, rules), SQUAD_KEEP);
            assert_eq!(active(sq).target, 10);
            sq
        }
    }

    unsafe fn targets(sq: *mut SxSquad) -> Vec<u32> {
        unsafe {
            let mut out = [SxFact::default(); 8];
            let n = sx_squad_targets(sq, out.as_mut_ptr(), 8, null_mut());
            out[..n as usize].iter().map(|f| f.subject).collect()
        }
    }

    unsafe fn areas_at_a(sq: *mut SxSquad) -> bool {
        unsafe {
            let areas = sx_squad_areas(sq);
            (0..sx_areas_count(areas) as u32).any(|i| {
                let mut a = SxArea::default();
                assert_eq!(sx_areas_at(areas, i, &mut a), 1);
                a.pos == area_a().pos
            })
        }
    }

    unsafe fn scene_report_actions() {
        unsafe {
            // A's signal alone: the signal is given.
            let mut st = Acts {
                signal: true,
                ..Acts::default()
            };
            let rules = SxSquadRules {
                report: Some(report_cb),
                user: user(&mut st),
                ..SxSquadRules::default()
            };
            let sq = acts_squad(&rules);
            assert_eq!(sx_squad_report(sq, &msg(KIND_A, 2), 10), 1);
            assert_eq!(sx_squad_report(sq, &msg(KIND_B, 2), 10), 1);
            assert_eq!(check_with(sq, 10, &rules), SQUAD_SIGNAL);
            assert_eq!(sx_squad_signal_given(sq), 1);
            assert!(areas_at_a(sq));
            sx_squad_free(sq);

            // A's signal and B's forget of the active target: the lost
            // target wins, the signal is not given.
            let mut st = Acts {
                signal: true,
                forget: Some(10),
                ..Acts::default()
            };
            let rules = SxSquadRules {
                user: user(&mut st),
                ..rules
            };
            let sq = acts_squad(&rules);
            assert_eq!(sx_squad_report(sq, &msg(KIND_A, 2), 10), 1);
            assert_eq!(sx_squad_report(sq, &msg(KIND_B, 2), 10), 1);
            assert_eq!(check_with(sq, 10, &rules), SQUAD_REPLAN);
            assert_eq!(sx_squad_signal_given(sq), 0);
            assert_eq!(pending(sq).0.target, 11);
            assert_eq!(targets(sq), [11]);
            // While the new plan is pending a signal action still signals
            // the active plan.
            st.forget = None;
            assert_eq!(sx_squad_report(sq, &msg(KIND_A, 2), 11), 1);
            assert_eq!(check_with(sq, 11, &rules), SQUAD_SIGNAL);
            assert_eq!(sx_squad_signal_given(sq), 1);
            sx_squad_free(sq);

            // At most four actions per report.
            let mut st = Acts::default();
            let rules = SxSquadRules {
                user: user(&mut st),
                ..rules
            };
            let sq = acts_squad(&rules);
            assert_eq!(sx_squad_report(sq, &msg(KIND_FULL, 2), 10), 1);
            assert_eq!(check_with(sq, 10, &rules), SQUAD_KEEP);
            assert_eq!(st.pushed, [1, 1, 1, 1, 0]);
            assert_eq!(REPORT_GAME_FIRST, 16);
            sx_squad_free(sq);
        }
    }

    #[test]
    fn c_slots_mirror_rust() {
        unsafe {
            scene_place_custom();
            scene_member_cost();
            scene_spot_score();
            scene_trigger();
            scene_abort();
            scene_channel();
            scene_report_actions();
            // The same through the plain calls of a squad without custom
            // parts and the hex metric: no difference to the square.
            let (square, ..) = trigger_run(CHEBYSHEV);
            let (hex, ..) = trigger_run(METRIC_HEX);
            assert_eq!(square, hex);
            let (square, s, _) = abort_run(CHEBYSHEV);
            let (hex, h, _) = abort_run(METRIC_HEX);
            assert_eq!(square, hex);
            assert_eq!(s.seen, h.seen);
        }
    }

    /// `targets_slot_drops_candidates`: keeps only the subjects in the list.
    struct KeepOnly(Vec<u32>, Vec<u32>);

    unsafe extern "C" fn keep_cb(
        user: *mut c_void,
        view: *const SxSquadView,
        fact: *const SxFact,
    ) -> i32 {
        unsafe {
            let st = &mut *user.cast::<KeepOnly>();
            assert_eq!(sx_view_target_count(view), 2);
            st.1.push((*fact).subject);
            i32::from(st.0.contains(&(*fact).subject))
        }
    }

    /// The factors the score log shows: `(target, input)`.
    unsafe fn factors(sq: *mut SxSquad) -> Vec<(u32, i32)> {
        unsafe {
            let mut log = [SxScoreEvent::default(); 32];
            let n = sx_squad_score_log(sq, log.as_mut_ptr(), 32, null_mut());
            log[..n as usize]
                .iter()
                .filter(|e| e.event.kind == 1)
                .map(|e| (e.target, e.event.input))
                .collect()
        }
    }

    #[test]
    fn c_keep_target_drops_candidates() {
        unsafe {
            let sq = squad(CHEBYSHEV, &[(1, 0, 0), (2, 1, 0), (3, 0, 1), (4, 1, 1)]);
            let t = sx_tactic_new(ONE / 4, 0, 1);
            assert_eq!(
                sx_tactic_add_slot(t, &slot(place(PLACE_SIDE, SIDE_FRONT), 255)),
                0
            );
            let x = sx_squad_extra_start(sq) as u32;
            let linear = |input, max| SxConsideration {
                input,
                curve: 0,
                min: 0,
                max,
            };
            assert_eq!(sx_tactic_add_consideration(t, &linear(x, 9)), 0);
            let friend = linear(IN_TARGET_FRIEND_DIST as u32, 100);
            assert_eq!(sx_tactic_add_consideration(t, &friend), 0);
            add(sq, t);
            assert_eq!(sx_squad_set_extra_columns(sq, 1), 0);
            assert_eq!(sx_squad_set_score_log(sq, 1), 0);
            see(sq, 10, 20, 0, 0);
            see(sq, 11, 23, 0, 0);
            let mut st = KeepOnly(vec![11], Vec::new());
            let rules = SxSquadRules {
                keep_target: Some(keep_cb),
                user: user(&mut st),
                ..SxSquadRules::default()
            };
            let extra = [0, 5, 9];
            let mut id = 0;
            let made = sx_squad_plan_with(sq, 0, extra.as_ptr(), 3, null(), 0, 0, &rules, &mut id);
            assert_eq!(made, 1);
            // Asked for every known target, in knowledge order.
            assert_eq!(st.1, [10, 11]);
            let (p, _) = pending(sq);
            assert_eq!((p.tactic, p.has_target, p.target), (0, 1, 11));
            // Only 11 was scored; it read its own block (2) and its dropped friend.
            assert_eq!(factors(sq), [(11, 9), (11, 3)]);
            sx_squad_free(sq);
        }
    }

    /// `columns_slot_fills_after_extra`: writes fixed rule columns and
    /// records the row it saw.
    struct Columns {
        values: [i32; 2],
        seen: Vec<(u32, u32, Vec<i32>)>,
    }

    unsafe extern "C" fn columns_cb(
        user: *mut c_void,
        view: *const SxSquadView,
        ctx: *const SxRowCtx,
        out: *mut i32,
        count: u32,
    ) {
        unsafe {
            let st = &mut *user.cast::<Columns>();
            assert!(sx_view_member_count(view) > 0);
            let c = &*ctx;
            let row = std::slice::from_raw_parts(c.row, c.row_len as usize).to_vec();
            st.seen.push((c.row_len, count, row));
            let out = std::slice::from_raw_parts_mut(out, count as usize);
            assert!(out.iter().all(|&v| v == 0));
            out.copy_from_slice(&st.values);
        }
    }

    #[test]
    fn c_columns_fill_after_extra() {
        unsafe {
            let sq = sx_squad_new(&SxSquadConfig {
                rule_columns: 2,
                ..config(CHEBYSHEV)
            });
            assert!(!sq.is_null());
            for (id, x, y) in [(1, 0, 0), (2, 1, 0), (3, 0, 1), (4, 1, 1)] {
                assert!(sx_squad_add_member(sq, &member(id, x, y)) >= 0);
            }
            assert_eq!(sx_squad_set_leader(sq, 1), 1);
            assert_eq!(sx_squad_set_target_kind(sq, ENEMY), 0);
            assert_eq!(sx_squad_set_skill(sq, 255), 0);
            assert_eq!(sx_squad_set_extra_columns(sq, 1), 0);
            let start = sx_squad_extra_start(sq) as u32 + 1;
            let t = tactic(place(PLACE_SIDE, SIDE_FRONT), 255);
            let second = SxConsideration {
                input: start + 1,
                curve: 0,
                min: 0,
                max: 9,
            };
            assert_eq!(sx_tactic_add_consideration(t, &second), 0);
            add(sq, t);
            assert_eq!(sx_squad_set_score_log(sq, 1), 0);
            see(sq, 10, 20, 0, 0);
            let mut st = Columns {
                values: [7, 9],
                seen: Vec::new(),
            };
            let rules = SxSquadRules {
                columns: Some(columns_cb),
                user: user(&mut st),
                ..SxSquadRules::default()
            };
            let extra = [0, 3];
            let mut id = 0;
            let made = sx_squad_plan_with(sq, 0, extra.as_ptr(), 2, null(), 0, 0, &rules, &mut id);
            assert_eq!(made, 1);
            assert_eq!(st.seen.len(), 1);
            let (len, count, row) = &st.seen[0];
            assert_eq!((*len, *count), (start, 2));
            // The row before the rule columns: the built-in columns, then the extra one.
            assert_eq!(row.len(), start as usize);
            assert_eq!(
                row[start as usize - 1],
                3,
                "the game's extra column of target 10"
            );
            // The second rule column (9 of 0..9) is the factor 1.0.
            let mut log = [SxScoreEvent::default(); 32];
            let n = sx_squad_score_log(sq, log.as_mut_ptr(), 32, null_mut());
            let f: Vec<(i32, u32)> = log[..n as usize]
                .iter()
                .filter(|e| e.event.kind == 1)
                .map(|e| (e.event.input, e.event.value))
                .collect();
            assert_eq!(f, [(9, ONE)]);
            sx_squad_free(sq);
        }
    }

    #[test]
    fn c_view_accessors() {
        unsafe {
            let (_, st, sq) = trigger_run(CHEBYSHEV);
            let s = st.snapshot.expect("the trigger was asked");
            assert_eq!(s.members, 3);
            assert_eq!(s.first_member.id, 1);
            assert_eq!(s.targets, 1);
            assert_eq!(s.first_target.subject, 10);
            assert_eq!(s.first_target.pos, [20, 0, 0]);
            assert_eq!((s.past_member, s.past_target), (0, 0));
            assert_eq!(s.plan_ret, 1);
            assert_eq!((s.plan.has_plan, s.plan.signal_given), (1, 0));
            assert_eq!(
                (s.plan.tactic, s.plan.target, s.plan.has_target),
                (0, 10, 1)
            );
            assert_eq!((s.centre_ret, s.centre), (1, [2, 0, 0]));
            // A null view is an argument error.
            let mut m = SxMember::default();
            assert_eq!(sx_view_member_count(null()), ERROR_ARGUMENT);
            assert_eq!(sx_view_member_at(null(), 0, &mut m), ERROR_ARGUMENT);
            assert_eq!(sx_view_target_count(null()), ERROR_ARGUMENT);
            assert_eq!(sx_view_active_plan(null(), null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_view_centre(null(), null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_report_signal(null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_report_forget(null_mut(), 1), ERROR_ARGUMENT);
            sx_squad_free(sq);
        }
    }

    #[test]
    fn c_needs_rules() {
        unsafe {
            let sq = squad(CHEBYSHEV, &[(1, 0, 0), (2, 4, 0)]);
            add(sq, tactic(place(PLACE_SIDE, SIDE_FRONT), 255));
            see(sq, 10, 20, 0, 0);
            let mut id = 0;
            assert_eq!(sx_squad_plan(sq, 0, null(), 0, null(), 0, 0, &mut id), 1);
            assert_eq!(sx_squad_set_channel_custom(sq, 99, 1), 0, "unknown member");
            assert_eq!(sx_squad_set_channel_custom(sq, 2, 1), 1);
            assert_eq!(ERROR_NEEDS_RULES, -4);
            assert_eq!(
                sx_squad_plan(sq, 0, null(), 0, null(), 0, 0, &mut id),
                ERROR_NEEDS_RULES
            );
            assert_eq!(
                sx_squad_check(sq, 0, null(), 0, null(), 0, 0, &mut id),
                ERROR_NEEDS_RULES
            );
            // NULL rules are the plain call.
            assert_eq!(
                sx_squad_check_with(sq, 0, null(), 0, null(), 0, 0, null(), &mut id),
                ERROR_NEEDS_RULES
            );
            let mut delay = Delay {
                per: Some(1),
                seen: Vec::new(),
            };
            let table = delay_rules(&mut delay);
            assert_eq!(
                sx_squad_plan_with(sq, 0, null(), 0, null(), 0, 0, &table, &mut id),
                1
            );
            assert_eq!(sx_squad_signal(sq, 5), 0, "the plain signal refuses");
            assert_eq!(
                sx_squad_signal_with(sq, 2, &table),
                0,
                "no plan is active yet"
            );
            assert_eq!(check_with(sq, 6, &table), SQUAD_KEEP);
            assert_eq!(sx_squad_signal(sq, 7), 0);
            assert_eq!(sx_squad_signal_with(sq, 7, &table), 1);
            assert_eq!(sx_squad_suspend(sq, 2), 1);
            assert_eq!(sx_squad_reinstate_with(sq, 2, 20, &table), 1);
            sx_squad_free(sq);

            // A custom trigger, a custom abort and a custom place do too.
            for custom in 0..3 {
                let sq = squad(CHEBYSHEV, &[(1, 0, 0)]);
                let t = tactic(
                    if custom == 2 {
                        place(PLACE_CUSTOM, 1)
                    } else {
                        place(PLACE_HERE, 0)
                    },
                    1,
                );
                match custom {
                    0 => assert_eq!(sx_tactic_trigger_custom(t, 1), 0),
                    1 => assert_eq!(sx_tactic_add_abort_custom(t, 1), 0),
                    _ => {}
                }
                add(sq, t);
                see(sq, 10, 20, 0, 0);
                assert_eq!(
                    sx_squad_plan(sq, 0, null(), 0, null(), 0, 0, &mut id),
                    ERROR_NEEDS_RULES,
                    "custom {custom}"
                );
                sx_squad_free(sq);
            }

            // Rule columns count as custom parts, and have a limit.
            let sq = sx_squad_new(&SxSquadConfig {
                rule_columns: 1,
                ..config(CHEBYSHEV)
            });
            assert!(!sq.is_null());
            add(sq, tactic(place(PLACE_SIDE, SIDE_FRONT), 255));
            assert_eq!(
                sx_squad_plan(sq, 0, null(), 0, null(), 0, 0, &mut id),
                ERROR_NEEDS_RULES
            );
            sx_squad_free(sq);
            let too_many = SxSquadConfig {
                rule_columns: 1025,
                ..config(CHEBYSHEV)
            };
            assert!(sx_squad_new(&too_many).is_null());
            let mut crew = SxCrewConfig::default();
            assert_eq!(sx_crew_config_default(&mut crew), 0);
            assert_eq!(crew.squad.rule_columns, 0);
            crew.squad.rule_columns = 1025;
            assert!(sx_crew_new(&crew).is_null());
        }
    }

    // ---- the crew ----------------------------------------------------------------

    const MEMBERS: [(u32, i32, i32); 4] = [(1, 0, 0), (2, 0, 2), (3, 0, -2), (4, -3, 0)];
    const SKILL: u8 = 192;
    const STEP: u32 = 2;

    fn spots() -> [SxSpot; 4] {
        [
            SxSpot {
                pos: [16, 0, 0],
                tags: TAG_COVER,
            },
            SxSpot {
                pos: [16, 2, 0],
                tags: 0,
            },
            SxSpot {
                pos: [20, 4, 0],
                tags: TAG_HIGH,
            },
            SxSpot {
                pos: [20, -4, 0],
                tags: 0,
            },
        ]
    }

    #[derive(Clone, Copy, Default)]
    struct Walk {
        to: Option<[i32; 3]>,
        from: u32,
    }

    impl Walk {
        fn next(&self, now: u32, at: [i32; 3]) -> Option<[i32; 3]> {
            let to = self.to?;
            let d = now.wrapping_sub(self.from);
            let s = |a: i32, b: i32| a + (b - a).signum();
            (d > 0 && d < 1 << 31 && d.is_multiple_of(STEP) && at != to)
                .then(|| [s(at[0], to[0]), s(at[1], to[1]), at[2]])
        }
    }

    /// A custom channel with a fixed delay, as a C callback and as a Rust
    /// rules object; the game reports of [`KIND_A`] are counted.
    struct Fixed {
        delay: u32,
        seen: Vec<(u32, u32, u32, u32, bool, u32)>,
        reports: Vec<(u32, u32)>,
    }

    unsafe extern "C" fn fixed_delay_cb(
        user: *mut c_void,
        id: u32,
        ctx: *const SxChannelCtx,
        delay: *mut u32,
    ) -> i32 {
        unsafe {
            let st = &mut *user.cast::<Fixed>();
            let c = &*ctx;
            st.seen
                .push((id, c.sender, c.receiver, c.now, c.report != 0, c.distance));
            delay.write(st.delay);
            1
        }
    }

    unsafe extern "C" fn fixed_report_cb(
        user: *mut c_void,
        _view: *const SxSquadView,
        msg: *const SxMessage,
        now: u32,
        _actions: *mut SxReportActions,
    ) {
        unsafe {
            let st = &mut *user.cast::<Fixed>();
            st.reports.push(((*msg).kind, now));
        }
    }

    impl sagax_decide::group::SquadRules for Fixed {
        fn channel_delay(&mut self, id: u32, ctx: &sagax_decide::group::ChannelCtx) -> Option<u32> {
            self.seen.push((
                id,
                ctx.sender,
                ctx.receiver,
                ctx.now,
                ctx.report,
                ctx.distance,
            ));
            Some(self.delay)
        }

        fn report(
            &mut self,
            _: &sagax_decide::group::SquadView<'_>,
            msg: &Message,
            now: u32,
            _: &mut sagax_decide::group::ReportActions<'_>,
        ) {
            self.reports.push((msg.kind, now));
        }
    }

    /// An event as numbers: kind, member, plan, start tick, goal.
    type Ev = (u32, u32, u32, Option<u32>, SxGoal);

    fn c_events(buf: &[SxCrewEvent]) -> Vec<Ev> {
        buf.iter()
            .map(|e| {
                (
                    e.kind,
                    e.member,
                    e.plan,
                    (e.has_start_at != 0).then_some(e.start_at),
                    e.goal,
                )
            })
            .collect()
    }

    fn rust_events(events: &[CrewEvent]) -> Vec<Ev> {
        events
            .iter()
            .map(|e| {
                let c = convert::crew_event(e);
                (
                    c.kind,
                    c.member,
                    c.plan,
                    c.start_at,
                    c.goal.as_ref().map(SxGoal::of).unwrap_or_default(),
                )
            })
            .collect()
    }

    /// The flank scene through `sx_crew_*`.
    unsafe fn c_flank_crew() -> *mut SxCrew {
        unsafe {
            let mut c = SxCrewConfig::default();
            assert_eq!(sx_crew_config_default(&mut c), 0);
            c.squad.members = 4;
            c.squad.memory = 8;
            c.squad.areas = 4;
            c.squad.inbox = 8;
            c.squad.metric = CHEBYSHEV;
            let crew = sx_crew_new(&c);
            assert!(!crew.is_null());
            for (id, x, y) in MEMBERS {
                let m = SxMember {
                    skill: SKILL,
                    ..member(id, x, y)
                };
                assert!(sx_crew_add_member(crew, &m) >= 0);
            }
            let sq = sx_crew_squad(crew);
            for (id, ..) in MEMBERS {
                assert_eq!(sx_squad_set_channel_custom(sq, id, 1), 1);
            }
            assert_eq!(sx_squad_set_leader(sq, 1), 1);
            assert_eq!(sx_squad_set_skill(sq, u32::from(SKILL)), 0);
            assert_eq!(sx_squad_set_target_kind(sq, ENEMY), 0);
            assert!(sx_squad_add_rush(sq) >= 0);
            assert!(sx_squad_add_formation(sq, SHAPE_LINE, 2) >= 0);
            assert!(sx_squad_add_flank(sq, 3, 4) >= 0);
            let mut told = SxMessage::default();
            assert_eq!(sx_report_enemy(2, &fact(11, 24, 10, 0), &mut told), 0);
            assert_eq!(sx_squad_report(sq, &told, 0), 1);
            assert_eq!(sx_crew_observe(crew, 0, &fact(10, 20, 0, 0), null_mut()), 0);
            crew
        }
    }

    /// The same scene through the Rust crew.
    fn rust_flank_crew() -> Crew {
        use sagax_decide::group::{Shape, SquadConfig};
        use sagax_decide::memory::Fact;
        let mut crew = Crew::new(CrewConfig {
            squad: SquadConfig {
                members: 4,
                memory: 8,
                areas: 4,
                inbox: 8,
                metric: Metric::Chebyshev,
                ..SquadConfig::default()
            },
            ..CrewConfig::default()
        })
        .unwrap();
        for (id, x, y) in MEMBERS {
            assert!(
                crew.add_member(Member {
                    id,
                    pos: [x, y, 0],
                    roles: 0,
                    skill: SKILL,
                    state: MemberState::Active,
                })
                .is_some()
            );
        }
        let sq = crew.squad_mut();
        for (id, ..) in MEMBERS {
            assert!(sq.set_channel(id, Channel::Custom(1)));
        }
        assert!(sq.set_leader(1));
        sq.set_skill(SKILL);
        sq.set_target_kind(ENEMY);
        sq.add_tactic(tactics::rush());
        sq.add_tactic(tactics::formation(Shape::Line, 2));
        sq.add_tactic(tactics::flank(3, 4));
        let told = Fact {
            subject: 11,
            kind: ENEMY,
            pos: [24, 10, 0],
            seen_at: 0,
            ..Fact::default()
        };
        assert!(sq.report(Message::enemy(2, told), 0));
        crew.observe(
            0,
            Fact {
                subject: 10,
                kind: ENEMY,
                pos: [20, 0, 0],
                seen_at: 0,
                ..Fact::default()
            },
        );
        crew
    }

    #[test]
    fn c_crew_tick_with_and_report() {
        unsafe {
            let c = c_flank_crew();
            let mut rust = rust_flank_crew();
            let mut c_rules = Fixed {
                delay: 3,
                seen: Vec::new(),
                reports: Vec::new(),
            };
            let mut r_rules = Fixed {
                delay: 3,
                seen: Vec::new(),
                reports: Vec::new(),
            };
            let table = SxSquadRules {
                channel_delay: Some(fixed_delay_cb),
                report: Some(fixed_report_cb),
                user: user(&mut c_rules),
                ..SxSquadRules::default()
            };
            let spots = spots();
            // The plain tick refuses: every member is on a custom channel.
            let mut buf = [SxCrewEvent::default(); 64];
            let mut total = 0;
            assert_eq!(
                sx_crew_tick(
                    c,
                    0,
                    null(),
                    0,
                    spots.as_ptr(),
                    4,
                    SEED,
                    buf.as_mut_ptr(),
                    64,
                    &mut total
                ),
                ERROR_NEEDS_RULES
            );
            let mut events = Vec::new();
            let mut walks = [Walk::default(); 4];
            let mut pos: Vec<[i32; 3]> = MEMBERS.iter().map(|m| [m.1, m.2, 0]).collect();
            let mut all: Vec<(u32, Ev)> = Vec::new();
            let step = |walks: &mut [Walk; 4],
                        pos: &mut Vec<[i32; 3]>,
                        c: *mut SxCrew,
                        rust: &mut Crew,
                        t: u32| {
                for (i, (id, ..)) in MEMBERS.iter().enumerate() {
                    if let Some(p) = walks[i].next(t, pos[i]) {
                        pos[i] = p;
                        assert_eq!(sx_crew_set_pos(c, *id, p.as_ptr()), 1);
                        assert!(rust.set_pos(*id, p));
                    }
                }
            };
            for t in 0..=200u32 {
                step(&mut walks, &mut pos, c, &mut rust, t);
                let n = sx_crew_tick_with(
                    c,
                    t,
                    null(),
                    0,
                    spots.as_ptr(),
                    4,
                    SEED,
                    &table,
                    buf.as_mut_ptr(),
                    64,
                    &mut total,
                );
                assert!(n >= 0, "tick {t}: {n}");
                rust.tick_with(t, &[], &spots, SEED, &mut r_rules, &mut events)
                    .unwrap();
                let got = c_events(&buf[..n as usize]);
                assert_eq!(got, rust_events(&events), "tick {t}");
                for e in &got {
                    if e.0 == CREW_ORDER {
                        let i = MEMBERS.iter().position(|m| m.0 == e.1).unwrap();
                        walks[i].to = (e.4.has_pos != 0).then_some(e.4.pos);
                        walks[i].from = e.3.unwrap_or(t);
                    }
                    all.push((t, *e));
                }
            }
            let kinds = |k: u32| all.iter().filter(|e| e.1.0 == k).count();
            assert!(kinds(CREW_ACTIVATED) > 0 && kinds(CREW_SIGNAL) > 0);
            assert!(kinds(CREW_ORDER) >= 4);
            // The custom channel timed the orders and the arrival reports,
            // as in Rust.
            assert!(!c_rules.seen.is_empty());
            assert!(c_rules.seen.iter().any(|s| s.4), "a report was timed");
            assert_eq!(c_rules.seen, r_rules.seen);
            let activated = all.iter().find(|e| e.1.0 == CREW_ACTIVATED).unwrap().0;
            let first = all
                .iter()
                .find(|e| e.1.0 == CREW_ORDER && e.1.1 == 2)
                .unwrap()
                .0;
            assert_eq!(first, activated + 3);

            // A game report over the member's channel: plain (the custom
            // channel never arrives, the report is dropped), then with rules
            // (it arrives 3 ticks later and reaches the report slot).
            let m = msg(KIND_A, 2);
            let rm = Message {
                kind: KIND_A,
                sender: 2,
                ..Message::default()
            };
            assert_eq!(sx_crew_report(c, 300, &m), i32::from(rust.report(300, rm)));
            assert_eq!(
                sx_crew_report_with(c, 301, &m, &table),
                i32::from(rust.report_with(301, rm, &mut r_rules))
            );
            assert_eq!(sx_crew_report(null_mut(), 0, &m), ERROR_ARGUMENT);
            assert_eq!(sx_crew_report(c, 0, null()), ERROR_ARGUMENT);
            for t in 301..=310 {
                let n = sx_crew_tick_with(
                    c,
                    t,
                    null(),
                    0,
                    spots.as_ptr(),
                    4,
                    SEED,
                    &table,
                    buf.as_mut_ptr(),
                    64,
                    &mut total,
                );
                rust.tick_with(t, &[], &spots, SEED, &mut r_rules, &mut events)
                    .unwrap();
                assert_eq!(
                    c_events(&buf[..n as usize]),
                    rust_events(&events),
                    "tick {t}"
                );
            }
            assert_eq!(c_rules.reports, [(KIND_A, 304)]);
            assert_eq!(c_rules.reports, r_rules.reports);
            // Reinstate with rules: an order over the custom channel.
            assert_eq!(sx_crew_suspend(c, 3), 1);
            assert!(rust.suspend(3));
            assert_eq!(sx_crew_reinstate_with(c, 3, 320, &table), 1);
            assert!(rust.reinstate_with(3, 320, &mut r_rules));
            let n = sx_crew_tick_with(
                c,
                323,
                null(),
                0,
                spots.as_ptr(),
                4,
                SEED,
                &table,
                buf.as_mut_ptr(),
                64,
                &mut total,
            );
            rust.tick_with(323, &[], &spots, SEED, &mut r_rules, &mut events)
                .unwrap();
            assert_eq!(c_events(&buf[..n as usize]), rust_events(&events));
            assert_eq!(c_rules.seen, r_rules.seen);
            sx_crew_free(c);
        }
    }

    // ---- the helpers -------------------------------------------------------------

    #[test]
    fn c_spot_score_builtin() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let coord = |r: u64| i32::try_from(r % (1 << 29)).unwrap() - (1 << 28);
        let mut missed = 0;
        for i in 0..200 {
            let wish = [coord(next()), coord(next()), 0];
            let spot = SxSpot {
                pos: [coord(next()), coord(next()), 0],
                tags: (next() % 16) as u32,
            };
            let want = (next() % 16) as u32;
            let metric_code = (i % 4) as u32;
            let metric = convert::metric(metric_code).unwrap();
            let exact = spot_score_builtin(wish, want, &spot, metric);
            let mut out = 0i64;
            unsafe {
                assert_eq!(
                    sx_spot_score_builtin(wish.as_ptr(), want, &spot, metric_code, &mut out),
                    0
                );
            }
            if spot.tags & want == want {
                assert!(exact < 1 << 62, "case {i}");
                assert_eq!(i128::from(out), exact, "case {i}");
            } else {
                // A missing tag is a 2^100 offset: saturated.
                missed += 1;
                assert_eq!(out, i64::MAX, "case {i}");
            }
        }
        assert!(missed > 20, "{missed} cases miss a tag");
        let spot = SxSpot {
            pos: [0; 3],
            tags: 0,
        };
        let wish = [0; 3];
        let mut out = 0;
        unsafe {
            assert_eq!(
                sx_spot_score_builtin(null(), 0, &spot, 0, &mut out),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_spot_score_builtin(wish.as_ptr(), 0, null(), 0, &mut out),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_spot_score_builtin(wish.as_ptr(), 0, &spot, 0, null_mut()),
                ERROR_ARGUMENT
            );
            assert_eq!(
                sx_spot_score_builtin(wish.as_ptr(), 0, &spot, 4, &mut out),
                ERROR_ARGUMENT
            );
            // The exported built-in callbacks hand back the context's value.
            let cost = SxCostCtx {
                built_in: 77,
                ..SxCostCtx::default()
            };
            let mut got = 0;
            assert_eq!(sx_builtin_member_cost(null_mut(), &cost, &mut got), 1);
            assert_eq!(got, 77);
        }
    }
}
