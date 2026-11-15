//! `sx_mission_*`: a character's mission (motive, plan, detours,
//! replanning), see `sagax.h`.

use std::ffi::c_void;

use sagax_abi::convert::{self, FailCode};
use sagax_decide::mission::{
    BuiltInRules, Fail, FailCtx, Leash, Leeway, MAX_EVENTS, MAX_FAILS, Mission, MissionConfig,
    MissionRules, MissionTrace, Motive, Plan, PlanCtx, Step, Where,
};
use sagax_decide::{Decider, Pick};

use super::decide::{SxDecideTrace, SxDecider};
use super::group::{Held, Inside};
use super::memory::{SxKnowledge, SxPointPattern, pos3, put};
use super::{ERROR_ARGUMENT, TraceSlot, create, destroy, guarded, set_trace, slice_of, to_i32};

/// `sx_where`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxWhere {
    /// `SX_WHERE_*`.
    pub kind: u32,
    /// `SX_WHERE_FIXED`: the position.
    pub pos: [i32; 3],
    /// `SX_WHERE_SUBJECT`: the subject.
    pub subject: u32,
}

impl SxWhere {
    fn place(&self) -> Option<Where> {
        convert::where_of(self.kind, self.pos, self.subject)
    }

    fn of(at: Where) -> Self {
        let (kind, pos, subject) = convert::where_code(at);
        Self { kind, pos, subject }
    }
}

/// `sx_fail`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxFail {
    /// `SX_FAIL_*`.
    pub kind: u32,
    /// `UNKNOWN`, `DEFEATED`, `FACT`: the subject.
    pub subject: u32,
    /// `FACT`: the fact kind.
    pub fact_kind: u32,
    /// `FACT`: bits of the fact's data.
    pub mask: u32,
    /// `FACT`: the value of those bits.
    pub value: u32,
    /// `CUSTOM`: the game's condition.
    pub id: u32,
    /// `FLAGS`: bits of the flags.
    pub flags_mask: u64,
    /// `FLAGS`: the value of those bits.
    pub flags_value: u64,
}

impl SxFail {
    fn fail(&self) -> Option<Fail> {
        convert::fail(&FailCode {
            kind: self.kind,
            subject: self.subject,
            fact_kind: self.fact_kind,
            mask: self.mask,
            value: self.value,
            id: self.id,
            flags_mask: self.flags_mask,
            flags_value: self.flags_value,
        })
    }

    fn of(fail: Fail) -> Self {
        let c = convert::fail_code(fail);
        Self {
            kind: c.kind,
            subject: c.subject,
            fact_kind: c.fact_kind,
            mask: c.mask,
            value: c.value,
            id: c.id,
            flags_mask: c.flags_mask,
            flags_value: c.flags_value,
        }
    }
}

/// `sx_step`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxStep {
    /// `SX_STEP_*`.
    pub kind: u32,
    /// The place of the step (see the header).
    pub at: SxWhere,
    /// First number of the step (see the header).
    pub a: u32,
    /// Second number of the step (see the header).
    pub b: u32,
    /// Conditions that break the step; `fail_count` are used.
    pub fails: [SxFail; MAX_FAILS],
    /// Used entries of `fails` (at most 4).
    pub fail_count: u32,
    /// 1: skip the step when it cannot be done.
    pub skip_if_impossible: u8,
}

impl SxStep {
    pub(crate) fn step(&self) -> Option<Step> {
        let kind = convert::step_kind(self.kind, self.at.place(), self.a, self.b)?;
        let mut step = Step::go_to([0; 3]);
        step.kind = kind;
        step.fails = fails_of(&self.fails, self.fail_count)?;
        step.skip_if_impossible = self.skip_if_impossible != 0;
        Some(step)
    }

    pub(crate) fn of(step: &Step) -> Self {
        let (kind, at, a, b) = convert::step_kind_code(step.kind);
        let (fails, fail_count) = fails_out(&step.fails);
        Self {
            kind,
            at: at.map(SxWhere::of).unwrap_or_default(),
            a,
            b,
            fails,
            fail_count,
            skip_if_impossible: u8::from(step.skip_if_impossible),
        }
    }
}

/// The C form of conditions filled from the front, and how many there are.
fn fails_out(fails: &[Option<Fail>; MAX_FAILS]) -> ([SxFail; MAX_FAILS], u32) {
    let mut out = [SxFail::default(); MAX_FAILS];
    let mut count = 0;
    for (slot, f) in out.iter_mut().zip(fails.iter().flatten()) {
        *slot = SxFail::of(*f);
        count += 1;
    }
    (out, count)
}

/// The first `count` of `fails`, filled from the front; `None` for more
/// than [`MAX_FAILS`] or an unknown kind.
fn fails_of(fails: &[SxFail], count: u32) -> Option<[Option<Fail>; MAX_FAILS]> {
    let count = count as usize;
    if count > MAX_FAILS || count > fails.len() {
        return None;
    }
    let mut out = [None; MAX_FAILS];
    for (slot, f) in out.iter_mut().zip(&fails[..count]) {
        *slot = Some(f.fail()?);
    }
    Some(out)
}

/// `sx_motive`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMotive {
    /// `SX_MOTIVE_*`.
    pub kind: u32,
    /// `SX_MOTIVE_CUSTOM`: the game's kind.
    pub custom: u32,
    /// The place; valid if `has_place`.
    pub place: [i32; 3],
    /// The subject; valid if `has_subject`.
    pub subject: u32,
    /// How close counts as there; 0: the config's `arrive_radius`.
    pub radius: u32,
    /// 1 if `place` is set.
    pub has_place: u8,
    /// 1 if `subject` is set.
    pub has_subject: u8,
}

impl SxMotive {
    fn motive(&self) -> Option<Motive> {
        Some(Motive {
            kind: convert::motive_kind(self.kind, self.custom)?,
            place: (self.has_place != 0).then_some(self.place),
            subject: (self.has_subject != 0).then_some(self.subject),
            radius: self.radius,
        })
    }

    fn of(m: Motive) -> Self {
        let (kind, custom) = convert::motive_kind_code(m.kind);
        Self {
            kind,
            custom,
            place: m.place.unwrap_or_default(),
            subject: m.subject.unwrap_or_default(),
            radius: m.radius,
            has_place: u8::from(m.place.is_some()),
            has_subject: u8::from(m.subject.is_some()),
        }
    }
}

/// `sx_leeway`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxLeeway {
    /// Largest distance from the anchor; valid if `has_distance`.
    pub distance: u32,
    /// Longest detour in ticks; valid if `has_ticks`.
    pub ticks: u32,
    /// `SX_ON_LOST_*`.
    pub on_lost: u32,
    /// `SX_ON_LOST_SEARCH`: how long to search.
    pub lost_ticks: u32,
    /// `SX_ON_LOST_SEARCH`: how far around the last known place.
    pub lost_radius: u32,
    /// 1 if `distance` is set (0: no limit).
    pub has_distance: u8,
    /// 1 if `ticks` is set (0: no limit).
    pub has_ticks: u8,
    /// 1: may take another plan when its plan breaks or ends.
    pub replan: u8,
    /// 1: may fall back to the widening plans (only with `replan`).
    pub widen: u8,
}

impl SxLeeway {
    fn leeway(&self) -> Option<Leeway> {
        Some(Leeway {
            leash: Leash {
                distance: (self.has_distance != 0).then_some(self.distance),
                ticks: (self.has_ticks != 0).then_some(self.ticks),
                on_lost: convert::on_lost(self.on_lost, self.lost_ticks, self.lost_radius)?,
            },
            replan: self.replan != 0,
            widen: self.widen != 0,
        })
    }

    fn of(l: Leeway) -> Self {
        let (on_lost, lost_ticks, lost_radius) = convert::on_lost_code(l.leash.on_lost);
        Self {
            distance: l.leash.distance.unwrap_or_default(),
            ticks: l.leash.ticks.unwrap_or_default(),
            on_lost,
            lost_ticks,
            lost_radius,
            has_distance: u8::from(l.leash.distance.is_some()),
            has_ticks: u8::from(l.leash.ticks.is_some()),
            replan: u8::from(l.replan),
            widen: u8::from(l.widen),
        }
    }
}

/// `sx_mission_plan_info`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMissionPlanInfo {
    /// Number of steps of the plan.
    pub step_count: u32,
    /// Used entries of `fails`.
    pub fail_count: u32,
    /// Conditions that break the whole plan.
    pub fails: [SxFail; MAX_FAILS],
    /// 1: the plan starts again after its last step.
    pub repeat: u8,
}

/// `sx_mission_config`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMissionConfig {
    /// The mission's id (for the seed and for search points).
    pub id: u32,
    /// Most steps a plan may have.
    pub max_steps: u32,
    /// Number of playbook slots.
    pub max_playbook: u32,
    /// The fact kind positions are read from.
    pub subject_kind: u32,
    /// Bits of a fact's data that mark an enemy.
    pub enemy_bits: u32,
    /// Bits of a fact's data that mark a defeated subject.
    pub defeated_bits: u32,
    /// How close counts as arrived.
    pub arrive_radius: u32,
    /// Ticks after which a subject not seen counts as lost.
    pub lost_after: u32,
    /// Ticks of `SX_STEP_PAUSE`.
    pub pause_ticks: u32,
    /// Distance at which a fleeing character counts as safe.
    pub safe_distance: u32,
    /// The signal given when the motive's place is reached.
    pub reached_signal: u32,
    /// `SX_METRIC_*`.
    pub metric: u32,
    /// `SX_GRID_SQUARE` or `SX_GRID_HEX_*` (needs `SX_METRIC_HEX`).
    pub grid: u32,
    /// Extra input columns the game adds to each decider row.
    pub extra_columns: u32,
    /// `SX_PICK_*` among playbook plans.
    pub pick: u32,
    /// `top` of the pick.
    pub pick_top: u32,
    /// The pattern of search points, in cells.
    pub search: SxPointPattern,
}

impl SxMissionConfig {
    fn config(&self) -> Option<MissionConfig> {
        Some(MissionConfig {
            id: self.id,
            max_steps: self.max_steps as usize,
            max_playbook: self.max_playbook as usize,
            subject_kind: self.subject_kind,
            enemy_bits: self.enemy_bits,
            defeated_bits: self.defeated_bits,
            arrive_radius: self.arrive_radius,
            lost_after: self.lost_after,
            pause_ticks: self.pause_ticks,
            safe_distance: self.safe_distance,
            reached_signal: self.reached_signal,
            metric: convert::metric(self.metric)?,
            search: self.search.pattern()?,
            hex_layout: convert::grid_layout(self.grid)?,
            extra_columns: self.extra_columns as usize,
            pick: Pick::from_u32(self.pick, self.pick_top)?,
        })
    }

    fn of(c: &MissionConfig) -> Option<Self> {
        let (pick, pick_top) = convert::pick_code(c.pick);
        Some(Self {
            id: c.id,
            max_steps: u32::try_from(c.max_steps).ok()?,
            max_playbook: u32::try_from(c.max_playbook).ok()?,
            subject_kind: c.subject_kind,
            enemy_bits: c.enemy_bits,
            defeated_bits: c.defeated_bits,
            arrive_radius: c.arrive_radius,
            lost_after: c.lost_after,
            pause_ticks: c.pause_ticks,
            safe_distance: c.safe_distance,
            reached_signal: c.reached_signal,
            metric: convert::metric_code(c.metric),
            grid: convert::grid_code(c.hex_layout),
            extra_columns: u32::try_from(c.extra_columns).ok()?,
            pick,
            pick_top,
            search: SxPointPattern::of(&c.search)?,
        })
    }
}

/// `sx_detour`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxDetour {
    /// `SX_DETOUR_*`.
    pub kind: u32,
    /// `ENGAGE`: the subject.
    pub subject: u32,
    /// `FLEE`: what to get away from; `CUSTOM`: where it happens.
    pub at: SxWhere,
    /// `FLEE`: where to run to; valid if `has_to`.
    pub to: [i32; 3],
    /// `CUSTOM`: the game's kind.
    pub custom: u32,
    /// `CUSTOM`: a free parameter.
    pub param: u32,
    /// 1 if `to` is set.
    pub has_to: u8,
}

/// `sx_mission_action`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMissionAction {
    /// `SX_ACTION_*`.
    pub kind: u32,
    /// Where; valid if `has_pos`.
    pub pos: [i32; 3],
    /// `FOLLOW`, `ATTACK`: the subject.
    pub subject: u32,
    /// `CUSTOM`: the game's kind.
    pub custom: u32,
    /// `CUSTOM`: the parameter.
    pub param: u32,
    /// 1 if `pos` is set.
    pub has_pos: u8,
}

/// `sx_mission_event`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMissionEvent {
    /// `SX_MISSION_EVENT_*`.
    pub kind: u32,
    /// First number (see the header).
    pub a: u32,
    /// `CALL`: the kind of call.
    pub b: u32,
    /// `WANTS_SQUAD`: the target subject; valid if `has_subject`.
    pub subject: u32,
    /// `CALL`: the caller's position; `WANTS_SQUAD`: the target place;
    /// valid if `has_pos`.
    pub pos: [i32; 3],
    /// 1 if `pos` is set.
    pub has_pos: u8,
    /// 1 if `subject` is set.
    pub has_subject: u8,
    /// `BROKEN` with `SX_BREAK_FAIL`: the condition that fired.
    pub fail: SxFail,
    /// `CALL`: the caller's motive.
    pub motive: SxMotive,
}

/// `sx_mission_out`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMissionOut {
    /// What the character should do now.
    pub action: SxMissionAction,
    /// Used entries of `events`.
    pub event_count: u32,
    /// What happened during the tick, in order.
    pub events: [SxMissionEvent; MAX_EVENTS],
}

/// `sx_mission_fail_ctx`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMissionFailCtx {
    /// The time of the tick.
    pub now: u32,
    /// Where the character stands.
    pub pos: [i32; 3],
    /// The step the condition belongs to; valid if `has_step`.
    pub step: u32,
    /// The candidate's decider option while a replan scores it; valid if
    /// `has_candidate`.
    pub candidate: u32,
    /// The mission's motive.
    pub motive: SxMotive,
    /// 1: a step's condition, 0: the plan's.
    pub has_step: u8,
    /// 1: a condition of a candidate during a replan, 0: of the running
    /// plan.
    pub has_candidate: u8,
}

/// `sx_mission_plan_ctx`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMissionPlanCtx {
    /// The time of the tick.
    pub now: u32,
    /// Where the character stands.
    pub pos: [i32; 3],
    /// The candidate's decider option.
    pub option: u32,
    /// Its first position; valid if `has_first`.
    pub first: [i32; 3],
    /// The mission's motive.
    pub motive: SxMotive,
    /// 1: a widening plan.
    pub widening: u8,
    /// 1: its conditions hold now and its first position can be filled.
    pub feasible: u8,
    /// 1 if `first` is set.
    pub has_first: u8,
}

impl SxMissionPlanCtx {
    fn of(c: &PlanCtx) -> Self {
        Self {
            now: c.now,
            pos: c.pos,
            option: c.option,
            first: c.first.unwrap_or_default(),
            motive: SxMotive::of(c.motive),
            widening: u8::from(c.widening),
            feasible: u8::from(c.feasible),
            has_first: u8::from(c.first.is_some()),
        }
    }
}

/// `sx_mission_rules.fail`.
pub type MissionFailFn = unsafe extern "C" fn(*mut c_void, *const SxMissionFailCtx, u32) -> i32;
/// `sx_mission_rules.columns`.
pub type MissionColumnsFn =
    unsafe extern "C" fn(*mut c_void, *const SxMissionPlanCtx, *mut i32, u32);
/// `sx_mission_rules.score`.
pub type MissionScoreFn =
    unsafe extern "C" fn(*mut c_void, *const SxMissionPlanCtx, u32, *mut u32) -> i32;

/// `sx_mission_rules`: one callback per slot, `NULL` = the built-in.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SxMissionRules {
    /// Whether the game's condition holds.
    pub fail: Option<MissionFailFn>,
    /// Fill the game's columns of a candidate's row.
    pub columns: Option<MissionColumnsFn>,
    /// Replace a candidate's score.
    pub score: Option<MissionScoreFn>,
    /// Passed to every callback.
    pub user: *mut c_void,
}

impl MissionRules for SxMissionRules {
    fn fail(&mut self, ctx: &FailCtx, id: u32) -> bool {
        let Some(f) = self.fail else {
            return false;
        };
        let c = SxMissionFailCtx {
            now: ctx.now,
            pos: ctx.pos,
            step: ctx.step.map_or(0, |s| s as u32),
            candidate: ctx.candidate.unwrap_or(0),
            motive: SxMotive::of(ctx.motive),
            has_step: u8::from(ctx.step.is_some()),
            has_candidate: u8::from(ctx.candidate.is_some()),
        };
        // SAFETY: the game passed a valid callback and user for this call
        // (sagax.h).
        unsafe { f(self.user, &c, id) != 0 }
    }

    fn columns(&mut self, ctx: &PlanCtx, out: &mut [i32]) {
        if let Some(f) = self.columns {
            let c = SxMissionPlanCtx::of(ctx);
            // SAFETY: as above; `out` has `out.len()` entries.
            unsafe { f(self.user, &c, out.as_mut_ptr(), out.len() as u32) };
        }
    }

    fn score(&mut self, ctx: &PlanCtx, built_in: u32) -> Option<u32> {
        let f = self.score?;
        let c = SxMissionPlanCtx::of(ctx);
        let mut score = built_in;
        // SAFETY: as above.
        let replaced = unsafe { f(self.user, &c, built_in, &mut score) } != 0;
        replaced.then_some(score)
    }
}

/// `sx_mission_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMissionTrace {
    /// `SX_MISSION_TRACE_*`.
    pub kind: u32,
    /// The candidate's option; `PICKED`: the winner. Valid if `has_option`.
    pub option: u32,
    /// `COLUMN`: the column.
    pub column: u32,
    /// `RESCORED`: the decider's score.
    pub built_in: u32,
    /// `RESCORED`: the score after the game's slot; `PICKED`: the winner's.
    pub score: u32,
    /// `BEGIN`: the step.
    pub index: u32,
    /// `COLUMN`: its value.
    pub value: i32,
    /// `BEGIN`: the position the step goes for, if `has_at`.
    pub at: [i32; 3],
    /// 1 if `option` is set.
    pub has_option: u8,
    /// 1 if `at` is set.
    pub has_at: u8,
    /// `DECIDE`: the decider's record.
    pub decide: SxDecideTrace,
}

impl SxMissionTrace {
    pub(crate) fn of(e: MissionTrace) -> Self {
        let c = convert::mission_trace(e);
        Self {
            kind: c.kind,
            option: c.option.unwrap_or_default(),
            column: c.column,
            built_in: c.built_in,
            score: c.score,
            index: c.index,
            value: c.value,
            at: c.at.unwrap_or_default(),
            has_option: u8::from(c.option.is_some()),
            has_at: u8::from(c.at.is_some()),
            decide: SxDecideTrace::of_code(c.decide),
        }
    }
}

/// `sx_mission_trace_fn`.
pub type MissionTraceFn = unsafe extern "C" fn(*mut c_void, *const SxMissionTrace);

/// `sx_mission`.
pub struct SxMission {
    mission: Mission,
    /// The decider handle lent by `sx_mission_decider` (null until asked),
    /// freed with the mission.
    decider: *mut SxDecider,
    /// Where `sx_mission_set_trace` sends the trace records.
    trace: TraceSlot<SxMissionTrace>,
}

impl Drop for SxMission {
    fn drop(&mut self) {
        // SAFETY: made by `sx_mission_decider` with `Box::into_raw` for this
        // mission only and freed here once; `sx_decider_free` leaves it alone.
        unsafe { destroy(self.decider) };
    }
}

/// The mission's decider for its lent handle.
///
/// # Safety
/// `root` is a live mission handle with no other reference into it alive.
unsafe fn mission_decider(root: *mut c_void) -> Option<*mut Decider> {
    // SAFETY: see above.
    let m = unsafe { &mut *root.cast::<SxMission>() };
    Some(std::ptr::from_mut(m.mission.decider_mut()))
}

macro_rules! mission {
    ($m:expr) => {
        // SAFETY: the caller passes null or a valid mission (sagax.h).
        match unsafe { $m.as_mut() } {
            Some(m) => &mut m.mission,
            None => return ERROR_ARGUMENT,
        }
    };
}

macro_rules! mission_ref {
    ($m:expr) => {
        // SAFETY: the caller passes null or a valid mission (sagax.h).
        match unsafe { $m.as_ref() } {
            Some(m) => &m.mission,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// `sx_mission_config_default`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_config_default(out: *mut SxMissionConfig) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    let Some(c) = SxMissionConfig::of(&MissionConfig::default()) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: not null and writable (see above).
    unsafe { put(out, c) };
    0
}

/// `sx_leeway_preset`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_leeway_preset(which: u32, out: *mut SxLeeway) -> i32 {
    let Some(l) = convert::leeway_preset(which) else {
        return ERROR_ARGUMENT;
    };
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: not null and writable (see above).
    unsafe { put(out, SxLeeway::of(l)) };
    0
}

/// `sx_mission_new`.
///
/// # Safety
/// `config` and `motive` are null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_new(
    config: *const SxMissionConfig,
    motive: *const SxMotive,
) -> *mut SxMission {
    // SAFETY: null or valid (see above).
    let (Some(c), Some(m)) = (unsafe { config.as_ref() }, unsafe { motive.as_ref() }) else {
        return std::ptr::null_mut();
    };
    create(|| {
        let mission = Mission::new(c.config()?, m.motive()?).ok()?;
        Some(SxMission {
            mission,
            decider: std::ptr::null_mut(),
            trace: None,
        })
    })
}

/// `sx_mission_free`.
///
/// # Safety
/// `mission` is null or from [`sx_mission_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_free(mission: *mut SxMission) {
    // SAFETY: see above.
    unsafe { destroy(mission) }
}

/// `sx_mission_set_leeway`.
///
/// # Safety
/// `mission` and `leeway` are null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_set_leeway(
    mission: *mut SxMission,
    leeway: *const SxLeeway,
) -> i32 {
    let m = mission!(mission);
    // SAFETY: null or valid (see above).
    let Some(l) = unsafe { leeway.as_ref() }.and_then(SxLeeway::leeway) else {
        return ERROR_ARGUMENT;
    };
    m.set_leeway(l);
    0
}

/// `sx_mission_set_motive`.
///
/// # Safety
/// `mission` and `motive` are null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_set_motive(
    mission: *mut SxMission,
    motive: *const SxMotive,
) -> i32 {
    let m = mission!(mission);
    // SAFETY: null or valid (see above).
    let Some(motive) = unsafe { motive.as_ref() }.and_then(SxMotive::motive) else {
        return ERROR_ARGUMENT;
    };
    m.set_motive(motive);
    0
}

/// `sx_mission_motive`.
///
/// # Safety
/// `mission` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_motive(mission: *const SxMission, out: *mut SxMotive) -> i32 {
    let m = mission_ref!(mission);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: not null and writable (see above).
    unsafe { put(out, SxMotive::of(m.motive())) };
    0
}

/// `sx_mission_leeway`.
///
/// # Safety
/// `mission` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_leeway(mission: *const SxMission, out: *mut SxLeeway) -> i32 {
    let m = mission_ref!(mission);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: not null and writable (see above).
    unsafe { put(out, SxLeeway::of(m.leeway())) };
    0
}

/// `sx_mission_plan`: 1 and the plan, or 0 when there is none yet.
///
/// # Safety
/// `mission` is null or valid; `out` is null or writable; `steps` is null
/// or has room for `cap` steps.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_plan(
    mission: *const SxMission,
    out: *mut SxMissionPlanInfo,
    steps: *mut SxStep,
    cap: u32,
) -> i32 {
    let m = mission_ref!(mission);
    if steps.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    let Some(plan) = m.plan() else {
        return 0;
    };
    let (fails, fail_count) = fails_out(plan.fails());
    let info = SxMissionPlanInfo {
        step_count: u32::try_from(plan.steps().len()).unwrap_or(u32::MAX),
        fail_count,
        fails,
        repeat: u8::from(plan.repeat),
    };
    // SAFETY: null or writable (see above).
    unsafe { put(out, info) };
    for (i, s) in plan.steps().iter().take(cap as usize).enumerate() {
        // SAFETY: within `cap` (see above).
        unsafe { steps.add(i).write(SxStep::of(s)) };
    }
    1
}

/// The plan of `count` steps and `fail_count` plan conditions; `None` for
/// a null list with a count, an unknown code or more than 4 conditions.
///
/// # Safety
/// `steps` holds `count` steps and `fails` `fail_count` conditions (null
/// allowed for 0).
unsafe fn plan_of(
    steps: *const SxStep,
    count: u32,
    repeat: u32,
    fails: *const SxFail,
    fail_count: u32,
) -> Option<Plan> {
    if (count > 0 && steps.is_null()) || (fail_count > 0 && fails.is_null()) {
        return None;
    }
    if fail_count as usize > MAX_FAILS {
        return None;
    }
    // SAFETY: see above.
    let (steps, fails) = unsafe {
        (
            slice_of(steps, count as usize),
            slice_of(fails, fail_count as usize),
        )
    };
    let steps = steps.iter().map(SxStep::step).collect::<Option<Vec<_>>>()?;
    let mut plan = if repeat != 0 {
        Plan::looping(&steps)
    } else {
        Plan::once(&steps)
    };
    for f in fails {
        plan = plan.fails_when(f.fail()?);
    }
    Some(plan)
}

/// `sx_mission_set_plan`.
///
/// # Safety
/// `mission` is null or valid; see [`plan_of`] for the lists.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_set_plan(
    mission: *mut SxMission,
    steps: *const SxStep,
    count: u32,
    repeat: u32,
    fails: *const SxFail,
    fail_count: u32,
) -> i32 {
    let m = mission!(mission);
    guarded(|| {
        // SAFETY: see above.
        let Some(plan) = (unsafe { plan_of(steps, count, repeat, fails, fail_count) }) else {
            return ERROR_ARGUMENT;
        };
        match m.set_plan(&plan) {
            Ok(()) => 0,
            Err(_) => ERROR_ARGUMENT,
        }
    })
}

/// `sx_mission_add_playbook`.
///
/// # Safety
/// As [`sx_mission_set_plan`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_mission_add_playbook(
    mission: *mut SxMission,
    steps: *const SxStep,
    count: u32,
    repeat: u32,
    fails: *const SxFail,
    fail_count: u32,
    preference: u32,
    widening: u32,
) -> i32 {
    let m = mission!(mission);
    guarded(|| {
        // SAFETY: see above.
        let Some(plan) = (unsafe { plan_of(steps, count, repeat, fails, fail_count) }) else {
            return ERROR_ARGUMENT;
        };
        match m.add_playbook(&plan, preference, widening != 0) {
            Ok(option) => to_i32(option),
            Err(_) => ERROR_ARGUMENT,
        }
    })
}

/// `sx_mission_clear_playbook`.
///
/// # Safety
/// `mission` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_clear_playbook(mission: *mut SxMission) -> i32 {
    mission!(mission).clear_playbook();
    0
}

/// `sx_mission_tick`.
///
/// # Safety
/// As [`sx_mission_tick_with`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_tick(
    mission: *mut SxMission,
    now: u32,
    pos: *const i32,
    knowledge: *const SxKnowledge,
    flags: u64,
    out: *mut SxMissionOut,
) -> i32 {
    // SAFETY: see above.
    unsafe { sx_mission_tick_with(mission, now, pos, knowledge, flags, std::ptr::null(), out) }
}

/// `sx_mission_tick_with`.
///
/// # Safety
/// `mission`, `knowledge`, `rules` and `out` are null or valid; `pos` is
/// null or holds 3 integers; the callbacks of `rules` are valid for the
/// call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_tick_with(
    mission: *mut SxMission,
    now: u32,
    pos: *const i32,
    knowledge: *const SxKnowledge,
    flags: u64,
    rules: *const SxMissionRules,
    out: *mut SxMissionOut,
) -> i32 {
    // SAFETY: null or valid (see above).
    let Some(SxMission {
        mission: m, trace, ..
    }) = (unsafe { mission.as_mut() })
    else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: null or valid (see above).
    let Some(k) = (unsafe { knowledge.as_ref() }).and_then(|k| k.value.get()) else {
        return ERROR_ARGUMENT;
    };
    if pos.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: not null; holds 3 integers (see above).
    let pos = unsafe { pos3(pos) };
    // SAFETY: null or valid (see above).
    let rules = unsafe { rules.as_ref() }.copied();
    guarded(|| {
        let mut built_in = BuiltInRules;
        let mut given;
        let rules: &mut dyn MissionRules = match rules {
            None => &mut built_in,
            Some(r) => {
                given = r;
                &mut given
            }
        };
        let output = traced!(*trace, SxMissionTrace::of, |sink| m
            .tick_traced(now, pos, k, flags, rules, sink));
        let events = output.events();
        if !out.is_null() {
            let mut o = SxMissionOut {
                action: action_of(output.action),
                event_count: events.len() as u32,
                ..SxMissionOut::default()
            };
            for (slot, e) in o.events.iter_mut().zip(events) {
                *slot = event_of(*e);
            }
            // SAFETY: not null and writable (see above).
            unsafe { put(out, o) };
        }
        to_i32(events.len())
    })
}

pub(crate) fn action_of(a: sagax_decide::mission::Action) -> SxMissionAction {
    let c = convert::action_code(a);
    SxMissionAction {
        kind: c.kind,
        pos: c.pos.unwrap_or_default(),
        subject: c.subject,
        custom: c.custom,
        param: c.param,
        has_pos: u8::from(c.pos.is_some()),
    }
}

pub(crate) fn event_of(e: sagax_decide::mission::MissionEvent) -> SxMissionEvent {
    let c = convert::mission_event(e);
    SxMissionEvent {
        kind: c.kind,
        a: c.a,
        b: c.b,
        subject: c.subject.unwrap_or_default(),
        pos: c.pos.unwrap_or_default(),
        has_pos: u8::from(c.pos.is_some()),
        has_subject: u8::from(c.subject.is_some()),
        fail: c.fail.map(SxFail::of).unwrap_or_default(),
        motive: c.motive.map(SxMotive::of).unwrap_or_default(),
    }
}

/// `sx_mission_detour`.
///
/// # Safety
/// `mission`, `detour` and `knowledge` are null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_detour(
    mission: *mut SxMission,
    now: u32,
    detour: *const SxDetour,
    knowledge: *const SxKnowledge,
) -> i32 {
    let m = mission!(mission);
    // SAFETY: null or valid (see above).
    let (Some(d), Some(k)) = (
        unsafe { detour.as_ref() },
        unsafe { knowledge.as_ref() }.and_then(|k| k.value.get()),
    ) else {
        return ERROR_ARGUMENT;
    };
    let Some(d) = convert::detour(
        d.kind,
        d.subject,
        d.at.place(),
        (d.has_to != 0).then_some(d.to),
        d.custom,
        d.param,
    ) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| i32::from(m.detour(now, d, k)))
}

/// `sx_mission_end_detour`.
///
/// # Safety
/// `mission` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_end_detour(mission: *mut SxMission) -> i32 {
    mission!(mission).end_detour();
    0
}

/// `sx_mission_finish_step`.
///
/// # Safety
/// `mission` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_finish_step(mission: *mut SxMission) -> i32 {
    mission!(mission).finish_step();
    0
}

/// `sx_mission_step_index`.
///
/// # Safety
/// `mission` is null or valid; `index` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_step_index(mission: *const SxMission, index: *mut u32) -> i32 {
    let m = mission_ref!(mission);
    if index.is_null() {
        return ERROR_ARGUMENT;
    }
    match m.step_index() {
        Some(i) => {
            // SAFETY: not null and writable (see above).
            unsafe { put(index, i as u32) };
            1
        }
        None => 0,
    }
}

/// `sx_mission_in_detour`.
///
/// # Safety
/// `mission` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_in_detour(mission: *const SxMission) -> i32 {
    i32::from(mission_ref!(mission).in_detour())
}

/// `sx_mission_returning`.
///
/// # Safety
/// `mission` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_returning(mission: *const SxMission) -> i32 {
    i32::from(mission_ref!(mission).returning())
}

/// `sx_mission_set_trace`: `trace` gets the records of every following
/// tick; `None` turns it off.
///
/// # Safety
/// `mission` is null or valid; `trace` and `context` stay valid while
/// tracing is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_set_trace(
    mission: *mut SxMission,
    trace: Option<MissionTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: null or valid (see above).
    set_trace(
        unsafe { mission.as_mut() }.map(|m| &mut m.trace),
        trace,
        context,
    )
}

/// `sx_mission_decider`: the mission's decider, lent until
/// `sx_mission_free`; `sx_decider_free` does nothing to it. One handle per
/// mission. Null for a null mission.
///
/// # Safety
/// `mission` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_mission_decider(mission: *mut SxMission) -> *mut SxDecider {
    // SAFETY: null or valid (see above).
    let Some(m) = (unsafe { mission.as_mut() }) else {
        return std::ptr::null_mut();
    };
    if m.decider.is_null() {
        m.decider = create(|| {
            Some(SxDecider::held(Held::Inside(Inside {
                root: mission.cast(),
                get: mission_decider,
            })))
        });
    }
    m.decider
}
