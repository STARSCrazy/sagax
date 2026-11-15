//! `sx_squad_*`, `sx_tactic_*`, `sx_keeper_*`, `sx_gather`: group tactics,
//! see `sagax.h`.

use sagax_abi::convert::{KeeperTraceCode, SquadTraceCode};
use sagax_decide::Decider;
use sagax_decide::group::{
    Abort, BuiltInRules, Channel, Goal, GoalKeeper, KeeperTrace, Member, MemberState, Nearby,
    Order, Plan, PlanError, SkillTable, Slot, Spot, Squad, SquadConfig, SquadRules, SquadTrace,
    Tactic, Trigger, gather, tactics,
};
use std::ffi::c_void;

use super::crew::SxCrew;
use super::decide::{SxConsideration, SxDecideTrace, SxDecider};
use super::memory::{
    SxArea, SxAreas, SxAreasTrace, SxFact, SxKnowledge, SxKnowledgeTrace, SxMessage,
    SxMessageTrace, pos3, put,
};
use super::rules::{CRules, SxSquadRules};
use super::{ERROR_ARGUMENT, TraceSlot, create, destroy, guarded, set_trace, slice_of, to_i32};
use sagax_abi::convert;
use sagax_decide::memory::{Areas, Knowledge};

// ---- value structs ------------------------------------------------------------

/// `sx_member`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMember {
    /// The game's id.
    pub id: u32,
    /// Where the member is.
    pub pos: [i32; 3],
    /// Role bits.
    pub roles: u32,
    /// How good the member is.
    pub skill: u8,
    /// `SX_MEMBER_*`.
    pub state: u8,
}

impl SxMember {
    pub(crate) fn of(m: &Member) -> Self {
        Self {
            id: m.id,
            pos: m.pos,
            roles: m.roles,
            skill: m.skill,
            state: m.state as u8,
        }
    }

    pub(crate) fn member(&self) -> Option<Member> {
        Some(Member {
            id: self.id,
            pos: self.pos,
            roles: self.roles,
            skill: self.skill,
            state: convert::member_state(u32::from(self.state))?,
        })
    }
}

/// `sx_spot`.
pub type SxSpot = Spot;

/// `sx_gather_candidate`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxGatherCandidate {
    /// The game's id.
    pub id: u32,
    /// Its faction.
    pub faction: u32,
    /// Where it is.
    pub pos: [i32; 3],
    /// 1 if not in a group yet.
    pub free: u8,
}

/// `sx_place`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxPlace {
    /// `SX_PLACE_*`.
    pub kind: u32,
    /// Offset, x.
    pub x: i32,
    /// Offset, y.
    pub y: i32,
    /// Offset, z.
    pub z: i32,
    /// `SX_SIDE_*` or tag bits.
    pub value: u32,
}

impl SxPlace {
    fn codes(&self) -> convert::PlaceCodes {
        convert::PlaceCodes {
            kind: self.kind,
            value: self.value,
            offset: [self.x, self.y, self.z],
        }
    }
}

/// `sx_slot`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxSlot {
    /// Role bits a member must all have.
    pub roles: u32,
    /// Fewest members.
    pub min: u8,
    /// Most members.
    pub max: u8,
    /// Where the members go for their goal.
    pub place: SxPlace,
    /// Where they gather first, or `SX_PLACE_NONE`.
    pub gather: SxPlace,
    /// `SX_GOAL_*`.
    pub kind: u32,
    /// The game's kind for `SX_GOAL_CUSTOM`.
    pub custom: u32,
    /// `SX_START_*`.
    pub start: u32,
    /// Ticks for `SX_START_AFTER_TICKS`.
    pub start_ticks: u32,
    /// Tag bits a spot should have.
    pub want_tags: u32,
    /// Distance from the target for sides and between (also sets the spot
    /// radius).
    pub dist: i32,
    /// Like `dist`, for the gather place.
    pub gather_dist: i32,
}

impl SxSlot {
    fn slot(&self) -> Option<Slot> {
        convert::slot(&convert::SlotCodes {
            roles: self.roles,
            min: u32::from(self.min),
            max: u32::from(self.max),
            place: self.place.codes(),
            gather: self.gather.codes(),
            kind: self.kind,
            custom: self.custom,
            start: self.start,
            start_ticks: self.start_ticks,
            want_tags: self.want_tags,
            dist: self.dist,
            gather_dist: self.gather_dist,
        })
    }
}

/// `sx_goal`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxGoal {
    /// `SX_GOAL_*`.
    pub kind: u32,
    /// The game's kind for `SX_GOAL_CUSTOM`.
    pub custom: u32,
    /// Where; valid if `has_pos`.
    pub pos: [i32; 3],
    /// Where after the gather spot; valid if `has_then_pos`.
    pub then_pos: [i32; 3],
    /// The target's id; valid if `has_target`.
    pub target: u32,
    /// `SX_START_*`.
    pub start: u32,
    /// Ticks for `SX_START_AFTER_TICKS`.
    pub start_ticks: u32,
    /// Index of the slot.
    pub slot: u32,
    /// The plan's id.
    pub plan: u32,
    /// When the goal was made.
    pub issued_at: u32,
    /// Ticks the member starts late.
    pub start_delay: u32,
    /// 1 if `pos` is set.
    pub has_pos: u8,
    /// 1 if `pos` is a wish point no spot was found for.
    pub unchecked: u8,
    /// 1 if `then_pos` is set.
    pub has_then_pos: u8,
    /// Like `unchecked`, for `then_pos`.
    pub then_unchecked: u8,
    /// 1 if `target` is set.
    pub has_target: u8,
}

impl SxGoal {
    pub(crate) fn of(g: &Goal) -> Self {
        let (kind, custom) = convert::goal_kind_code(g.kind);
        let (start, start_ticks) = convert::start_code(g.start);
        Self {
            kind,
            custom,
            pos: g.pos.unwrap_or_default(),
            then_pos: g.then_pos.unwrap_or_default(),
            target: g.target.unwrap_or(0),
            start,
            start_ticks,
            slot: u32::from(g.slot),
            plan: g.plan,
            issued_at: g.issued_at,
            start_delay: g.start_delay,
            has_pos: u8::from(g.pos.is_some()),
            unchecked: u8::from(g.unchecked),
            has_then_pos: u8::from(g.then_pos.is_some()),
            then_unchecked: u8::from(g.then_unchecked),
            has_target: u8::from(g.target.is_some()),
        }
    }

    fn goal(&self) -> Option<Goal> {
        Some(Goal {
            kind: convert::goal_kind(self.kind, self.custom)?,
            pos: (self.has_pos != 0).then_some(self.pos),
            unchecked: self.unchecked != 0,
            then_pos: (self.has_then_pos != 0).then_some(self.then_pos),
            then_unchecked: self.then_unchecked != 0,
            target: (self.has_target != 0).then_some(self.target),
            start: convert::start(self.start, self.start_ticks)?,
            slot: u8::try_from(self.slot).ok()?,
            plan: self.plan,
            issued_at: self.issued_at,
            start_delay: self.start_delay,
        })
    }
}

/// `sx_order`: a goal for one member (also a goal of a plan).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxOrder {
    /// The member's id.
    pub member: u32,
    /// What it is told to do.
    pub goal: SxGoal,
}

impl SxOrder {
    fn of(member: u32, goal: &Goal) -> Self {
        Self {
            member,
            goal: SxGoal::of(goal),
        }
    }
}

/// `sx_plan`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxPlan {
    /// Its id.
    pub id: u32,
    /// Index of the tactic.
    pub tactic: u32,
    /// The target's id; valid if `has_target`.
    pub target: u32,
    /// When it was made.
    pub made_at: u32,
    /// When it becomes active.
    pub active_at: u32,
    /// Number of goals in the plan.
    pub goal_count: u32,
    /// 1 if `target` is set.
    pub has_target: u8,
}

/// `sx_skill_table`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxSkillTable {
    /// How many rows are used (1 ..= 8).
    pub rows: u32,
    /// `SX_PICK_*` per level.
    pub pick: [u32; 8],
    /// `top` of the pick per level.
    pub pick_top: [u32; 8],
    /// Ticks from making a plan until it is active.
    pub think_ticks: [u32; 8],
    /// Most ticks a member starts late.
    pub timing_spread: [u32; 8],
    /// Bonus of the active plan, and the members' discipline.
    pub hold_bonus: [u32; 8],
    /// Ticks a member may postpone its goal.
    pub give_up_ticks: [u32; 8],
    /// Most ranks a member's spot may be off the best one.
    pub spot_rank_error: [u8; 8],
}

impl SxSkillTable {
    fn of(t: &SkillTable) -> Self {
        let c = convert::skill_codes(t);
        Self {
            rows: c.rows,
            pick: c.pick,
            pick_top: c.pick_top,
            think_ticks: c.think_ticks,
            timing_spread: c.timing_spread,
            hold_bonus: c.hold_bonus,
            give_up_ticks: c.give_up_ticks,
            spot_rank_error: c.spot_rank_error,
        }
    }

    fn table(&self) -> Option<SkillTable> {
        convert::skill_table(&convert::SkillCodes {
            rows: self.rows,
            pick: self.pick,
            pick_top: self.pick_top,
            think_ticks: self.think_ticks,
            timing_spread: self.timing_spread,
            hold_bonus: self.hold_bonus,
            give_up_ticks: self.give_up_ticks,
            spot_rank_error: self.spot_rank_error,
        })
    }
}

// ---- gather -------------------------------------------------------------------

/// `sx_gather`.
///
/// # Safety
/// `candidates` is null or points to `n`; `out` is null or has room for `max`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_gather(
    x: i32,
    y: i32,
    faction: u32,
    radius: i64,
    metric: u32,
    free_only: u32,
    candidates: *const SxGatherCandidate,
    n: u32,
    out: *mut u32,
    max: u32,
) -> i32 {
    if (candidates.is_null() && n > 0) || (out.is_null() && max > 0) {
        return ERROR_ARGUMENT;
    }
    let Some(metric) = convert::metric(metric) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: see above.
    let list: Vec<Nearby> = unsafe { slice_of(candidates, n as usize) }
        .iter()
        .map(|c| Nearby {
            id: c.id,
            faction: c.faction,
            pos: c.pos,
            free: c.free != 0,
        })
        .collect();
    let mut ids = Vec::new();
    gather(
        [x, y, 0],
        faction,
        radius,
        metric,
        free_only != 0,
        &list,
        &mut ids,
        max as usize,
    );
    for (i, id) in ids.iter().enumerate() {
        // SAFETY: at most `max` ids (see above).
        unsafe { out.add(i).write(*id) };
    }
    to_i32(ids.len())
}

// ---- tactics ------------------------------------------------------------------

/// `sx_tactic`.
pub struct SxTactic(Tactic);

/// `sx_tactic_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_tactic_new(weight: u32, min_skill: u32, needs_target: u32) -> *mut SxTactic {
    create(|| convert::tactic(weight, min_skill, needs_target).map(SxTactic))
}

/// `sx_tactic_free`.
///
/// # Safety
/// `tactic` is null or from [`sx_tactic_new`], not added and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_free(tactic: *mut SxTactic) {
    // SAFETY: see above.
    unsafe { destroy(tactic) }
}

/// `sx_tactic_add_slot`.
///
/// # Safety
/// `tactic` is null or valid; `slot` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_slot(tactic: *mut SxTactic, slot: *const SxSlot) -> i32 {
    let t = get!(tactic);
    let Some(slot) = input!(slot).slot() else {
        return ERROR_ARGUMENT;
    };
    t.0.slots.push(slot);
    to_i32(t.0.slots.len() - 1)
}

/// `sx_tactic_add_consideration`.
///
/// # Safety
/// `tactic` is null or valid; `consideration` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_consideration(
    tactic: *mut SxTactic,
    consideration: *const SxConsideration,
) -> i32 {
    let t = get!(tactic);
    let c = input!(consideration);
    let Some(c) = convert::consideration(c.input, c.curve, c.min, c.max) else {
        return ERROR_ARGUMENT;
    };
    t.0.considerations.push(c);
    0
}

/// `sx_tactic_add_abort`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_abort(
    tactic: *mut SxTactic,
    input: u32,
    at_least: i32,
) -> i32 {
    get!(tactic)
        .0
        .aborts
        .push(convert::abort_at_least(input, at_least));
    0
}

/// `sx_tactic_add_abort_when`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_abort_when(
    tactic: *mut SxTactic,
    input: u32,
    cmp: u32,
    value: i32,
) -> i32 {
    let Some(abort) = convert::abort_when(input, cmp, value) else {
        return ERROR_ARGUMENT;
    };
    get!(tactic).0.aborts.push(abort);
    0
}

/// `sx_tactic_add_abort_custom`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_add_abort_custom(tactic: *mut SxTactic, id: u32) -> i32 {
    get!(tactic).0.aborts.push(Abort::Custom(id));
    0
}

/// `sx_tactic_set_abort_if_target_lost`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_set_abort_if_target_lost(tactic: *mut SxTactic, on: u32) -> i32 {
    get!(tactic).0.abort_if_target_lost = on != 0;
    0
}

/// `sx_tactic_trigger_none`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_none(tactic: *mut SxTactic) -> i32 {
    get!(tactic).0.trigger = Trigger::None;
    0
}

/// `sx_tactic_trigger_all_arrived`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_all_arrived(
    tactic: *mut SxTactic,
    max_wait: u32,
) -> i32 {
    get!(tactic).0.trigger = Trigger::AllArrived { max_wait };
    0
}

/// `sx_tactic_trigger_target_in_radius`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_target_in_radius(
    tactic: *mut SxTactic,
    radius: i64,
) -> i32 {
    get!(tactic).0.trigger = Trigger::TargetInRadius(radius);
    0
}

/// `sx_tactic_trigger_manual`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_manual(tactic: *mut SxTactic) -> i32 {
    get!(tactic).0.trigger = Trigger::Manual;
    0
}

/// `sx_tactic_trigger_custom`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_trigger_custom(tactic: *mut SxTactic, id: u32) -> i32 {
    get!(tactic).0.trigger = Trigger::Custom(id);
    0
}

// ---- squad --------------------------------------------------------------------

/// Where a handle keeps its value: its own, or one lent by a crew.
pub(crate) enum Held<T> {
    /// The handle owns the value and frees it with itself.
    Own(T),
    /// Lent by a crew (see `sx_crew_squad`, `sx_crew_keeper`), which frees
    /// the handle with itself. Freeing the handle does nothing.
    Lent(Lend<T>),
    /// Lent by another handle (see `sx_squad_decider`,
    /// `sx_mission_decider`), which frees the handle with itself. Freeing
    /// the handle does nothing.
    Inside(Inside<T>),
}

/// A value inside another handle (a squad, a mission), reached through it
/// anew on every call.
pub(crate) struct Inside<T> {
    /// The owning handle, which frees this handle with itself.
    pub(crate) root: *mut c_void,
    /// The value in the owner, or `None` (a stale squad handle).
    ///
    /// # Safety
    /// Called only with `root`, while no other reference into it is alive.
    pub(crate) get: unsafe fn(*mut c_void) -> Option<*mut T>,
}

impl<T> Inside<T> {
    /// The value, or `None` for a stale owner.
    fn reach<'a>(&self) -> Option<&'a mut T> {
        // SAFETY: `root` is the handle that made this one and frees it with
        // itself, so it is alive while this handle is. This handle is its
        // own allocation, and the C interface is single-threaded per handle,
        // so no other reference into the owner is alive during this call;
        // the reference is used only within it.
        unsafe { (self.get)(self.root).map(|p| &mut *p) }
    }
}

/// The decider of the squad handle `root` (an [`SxSquad`]).
///
/// # Safety
/// `root` is a live squad handle with no other reference into it alive.
unsafe fn squad_decider(root: *mut c_void) -> Option<*mut Decider> {
    // SAFETY: see above.
    let squad = unsafe { &mut *root.cast::<SxSquad>() };
    squad
        .squad
        .get_mut()
        .map(|s| std::ptr::from_mut(s.decider_mut()))
}

/// The leader's knowledge of the squad handle `root` (an [`SxSquad`]).
///
/// # Safety
/// As [`squad_decider`].
unsafe fn squad_knowledge_of(root: *mut c_void) -> Option<*mut Knowledge> {
    // SAFETY: see above.
    let squad = unsafe { &mut *root.cast::<SxSquad>() };
    squad
        .squad
        .get_mut()
        .map(|s| std::ptr::from_mut(s.knowledge_mut()))
}

/// The leader's danger areas of the squad handle `root` (an [`SxSquad`]).
///
/// # Safety
/// As [`squad_decider`].
unsafe fn squad_areas_of(root: *mut c_void) -> Option<*mut Areas> {
    // SAFETY: see above.
    let squad = unsafe { &mut *root.cast::<SxSquad>() };
    squad
        .squad
        .get_mut()
        .map(|s| std::ptr::from_mut(s.areas_mut()))
}

/// A value lent by a crew. Only the crew's root pointer is kept: the value
/// is reached through it anew on every call, so no pointer into the crew is
/// held across calls that borrow the crew itself.
pub(crate) struct Lend<T> {
    /// The crew, as `sx_crew_new` made it (`Box::into_raw`).
    pub(crate) root: *mut SxCrew,
    /// The member the value belongs to (unused for the squad).
    pub(crate) id: u32,
    /// The crew's epoch when lent: once a call changed the crew the handle
    /// is stale. `None`: never stale (the squad).
    pub(crate) epoch: Option<u64>,
    /// The value in the crew, or `None` (the member is gone).
    pub(crate) get: fn(&mut SxCrew, u32) -> Option<&mut T>,
}

impl<T> Lend<T> {
    /// The value, or `None` for a stale handle.
    fn reach<'a>(&self) -> Option<&'a mut T> {
        // SAFETY: `root` came from `Box::into_raw` in `sx_crew_new` and the
        // crew frees its lent handles with itself, so it is alive while
        // this handle is. The handle lives in its own allocation (not inside
        // the crew), and the C interface is single-threaded per crew, so no
        // other reference into the crew is alive during this call; the
        // reference is used only within it.
        let crew = unsafe { &mut *self.root };
        if self.epoch.is_some_and(|e| e != crew.epoch()) {
            return None;
        }
        (self.get)(crew, self.id)
    }
}

impl<T> Held<T> {
    /// The value, or `None` for a stale lent handle.
    pub(crate) fn get(&self) -> Option<&T> {
        match self {
            Self::Own(t) => Some(t),
            Self::Lent(l) => l.reach().map(|t| &*t),
            Self::Inside(i) => i.reach().map(|t| &*t),
        }
    }

    /// The value, or `None` for a stale lent handle.
    pub(crate) fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Own(t) => Some(t),
            Self::Lent(l) => l.reach(),
            Self::Inside(i) => i.reach(),
        }
    }

    pub(crate) const fn is_lent(&self) -> bool {
        matches!(self, Self::Lent(_) | Self::Inside(_))
    }
}

impl<T> std::ops::Deref for Held<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.get().expect("a stale lent handle")
    }
}

impl<T> std::ops::DerefMut for Held<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.get_mut().expect("a stale lent handle")
    }
}

/// `sx_squad`.
pub struct SxSquad {
    pub(crate) squad: Held<Squad>,
    /// Orders taken from the squad and not yet handed out (beyond a cap).
    pub(crate) due: Vec<Order>,
    /// The decider handle lent by `sx_squad_decider` (null until asked),
    /// freed with the squad.
    pub(crate) decider: *mut SxDecider,
    /// The knowledge handle lent by `sx_squad_knowledge` (null until
    /// asked), freed with the squad.
    pub(crate) knowledge: *mut SxKnowledge,
    /// The areas handle lent by `sx_squad_areas` (null until asked), freed
    /// with the squad.
    pub(crate) areas: *mut SxAreas,
    /// Where `sx_squad_set_trace` sends the records of the calls through
    /// this handle.
    pub(crate) trace: TraceSlot<SxSquadTrace>,
}

impl Drop for SxSquad {
    fn drop(&mut self) {
        // SAFETY: each was made by its `sx_squad_*` function with
        // `Box::into_raw` for this squad only and is freed here once; the
        // `sx_*_free` functions leave lent handles alone.
        unsafe {
            destroy(self.decider);
            destroy(self.knowledge);
            destroy(self.areas);
        }
    }
}

/// `sx_squad_config`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxSquadConfig {
    /// Most members.
    pub members: u32,
    /// Most facts.
    pub memory: u32,
    /// Most danger areas.
    pub areas: u32,
    /// Most waiting member reports.
    pub inbox: u32,
    /// Most waiting danger reports.
    pub dangers: u32,
    /// `SX_METRIC_*`.
    pub metric: u32,
    /// How many of `role_masks` are used (at most 8).
    pub role_count: u32,
    /// The role list: one scoring column per mask (see `sx_squad_extra_start`).
    pub role_masks: [u32; 8],
    /// Columns the rules fill (`sx_squad_rules.columns`), after the extra
    /// columns.
    pub rule_columns: u32,
}

impl SxSquadConfig {
    /// The config; `None` for an unknown metric or more than 8 roles.
    pub(crate) fn config(&self) -> Option<SquadConfig> {
        convert::squad_config(&convert::SquadCodes {
            members: self.members,
            memory: self.memory,
            areas: self.areas,
            inbox: self.inbox,
            dangers: self.dangers,
            metric: self.metric,
            role_count: self.role_count,
            role_masks: self.role_masks,
            rule_columns: self.rule_columns,
        })
    }

    fn of(c: &SquadConfig) -> Self {
        let c = convert::squad_codes(c);
        Self {
            members: c.members,
            memory: c.memory,
            areas: c.areas,
            inbox: c.inbox,
            dangers: c.dangers,
            metric: c.metric,
            role_count: c.role_count,
            role_masks: c.role_masks,
            rule_columns: c.rule_columns,
        }
    }
}

/// `sx_squad_config_default`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_config_default(out: *mut SxSquadConfig) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { put(out, SxSquadConfig::of(&SquadConfig::default())) };
    0
}

/// `sx_squad_new`: null if `config` is null, a capacity is 0, the metric is
/// unknown, `role_count` is above 8 or `rule_columns` is too large.
///
/// # Safety
/// `config` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_new(config: *const SxSquadConfig) -> *mut SxSquad {
    // SAFETY: null or valid (see above).
    let Some(c) = (unsafe { config.as_ref() }).copied() else {
        return std::ptr::null_mut();
    };
    create(|| {
        let squad = Squad::new(c.config()?).ok()?;
        Some(SxSquad {
            squad: Held::Own(squad),
            due: Vec::new(),
            decider: std::ptr::null_mut(),
            knowledge: std::ptr::null_mut(),
            areas: std::ptr::null_mut(),
            trace: None,
        })
    })
}

/// `sx_squad_free`: does nothing for a squad lent by a crew.
///
/// # Safety
/// `squad` is null, from [`sx_squad_new`] and not used again, or lent by
/// `sx_crew_squad`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_free(squad: *mut SxSquad) {
    // SAFETY: null or valid (see above).
    if unsafe { squad.as_ref() }.is_some_and(|s| s.squad.is_lent()) {
        return;
    }
    // SAFETY: see above.
    unsafe { destroy(squad) }
}

/// The squad behind a handle (a place), or `ERROR_ARGUMENT`.
macro_rules! squad {
    ($p:expr) => {
        (*get!($p).squad)
    };
}

/// `sx_squad_add_member`.
///
/// # Safety
/// `squad` is null or valid; `member` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_member(squad: *mut SxSquad, member: *const SxMember) -> i32 {
    let s = &mut squad!(squad);
    let Some(m) = input!(member).member() else {
        return ERROR_ARGUMENT;
    };
    s.group_mut().add(m).map_or(ERROR_ARGUMENT, to_i32)
}

/// `sx_squad_remove_member`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_remove_member(squad: *mut SxSquad, id: u32) -> i32 {
    i32::from(squad!(squad).group_mut().remove(id))
}

/// `sx_squad_set_pos`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_pos(
    squad: *mut SxSquad,
    id: u32,
    x: i32,
    y: i32,
    z: i32,
) -> i32 {
    i32::from(squad!(squad).group_mut().set_pos(id, [x, y, z]))
}

/// `sx_squad_suspend`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_suspend(squad: *mut SxSquad, id: u32) -> i32 {
    i32::from(squad!(squad).group_mut().suspend(id))
}

/// `sx_squad_reinstate`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_reinstate(squad: *mut SxSquad, id: u32, now: u32) -> i32 {
    // SAFETY: see above; no rules.
    unsafe { sx_squad_reinstate_with(squad, id, now, std::ptr::null()) }
}

/// `sx_squad_reinstate_with`: the goal goes over a custom channel too.
///
/// # Safety
/// `squad` is null or valid; `rules` is null or valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_reinstate_with(
    squad: *mut SxSquad,
    id: u32,
    now: u32,
    rules: *const SxSquadRules,
) -> i32 {
    let h = get!(squad);
    let trace = h.trace;
    let s = &mut *h.squad;
    // SAFETY: null or valid (see above).
    let mut rules = unsafe { CRules::of(rules) };
    guarded(|| {
        // Without rules a squad that needs them does not activate a due plan.
        let rules = rules.as_mut().map(|r| r as &mut dyn SquadRules);
        let known = traced!(trace, SxSquadTrace::of, |sink| s
            .reinstate_traced(id, now, rules, sink));
        i32::from(known)
    })
}

/// `sx_squad_set_gone`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_gone(squad: *mut SxSquad, id: u32) -> i32 {
    i32::from(squad!(squad).set_gone(id))
}

/// `sx_squad_member_count`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_member_count(squad: *mut SxSquad) -> i32 {
    to_i32(squad!(squad).group().len())
}

/// `sx_squad_member_at`.
///
/// # Safety
/// `squad` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_member_at(
    squad: *mut SxSquad,
    index: u32,
    out: *mut SxMember,
) -> i32 {
    let s = &mut squad!(squad);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match s.group().get(index as usize) {
        Some(m) => {
            // SAFETY: non-null and writable (see above).
            unsafe { put(out, SxMember::of(m)) };
            1
        }
        None => 0,
    }
}

/// `sx_squad_set_leader`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_leader(squad: *mut SxSquad, id: u32) -> i32 {
    i32::from(squad!(squad).set_leader(id))
}

/// `sx_squad_leader`.
///
/// # Safety
/// `squad` is null or valid; `id` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_leader(squad: *mut SxSquad, id: *mut u32) -> i32 {
    match squad!(squad).leader() {
        Some(leader) => {
            // SAFETY: see above.
            unsafe { put(id, leader) };
            1
        }
        None => 0,
    }
}

/// `sx_squad_set_skill`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_skill(squad: *mut SxSquad, skill: u32) -> i32 {
    let s = &mut squad!(squad);
    let Ok(skill) = u8::try_from(skill) else {
        return ERROR_ARGUMENT;
    };
    s.set_skill(skill);
    0
}

/// `sx_squad_skill`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_skill(squad: *mut SxSquad) -> i32 {
    i32::from(squad!(squad).skill())
}

/// `sx_skill_table_default`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_skill_table_default(out: *mut SxSkillTable) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { put(out, SxSkillTable::of(&SkillTable::default())) };
    0
}

/// `sx_squad_set_skill_table`.
///
/// # Safety
/// `squad` is null or valid; `table` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_skill_table(
    squad: *mut SxSquad,
    table: *const SxSkillTable,
) -> i32 {
    let s = &mut squad!(squad);
    let Some(table) = input!(table).table() else {
        return ERROR_ARGUMENT;
    };
    if s.set_skill_table(table) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// `sx_squad_skill_table`.
///
/// # Safety
/// `squad` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_skill_table(squad: *mut SxSquad, out: *mut SxSkillTable) -> i32 {
    let s = &mut squad!(squad);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { put(out, SxSkillTable::of(s.skill_table())) };
    0
}

/// The table behind a C pointer (null: the default) and the skill, or
/// `None` for a skill above 255 or a table with a bad pick.
///
/// # Safety
/// `table` is null or valid.
unsafe fn table_and_skill(table: *const SxSkillTable, skill: u32) -> Option<(SkillTable, u8)> {
    let skill = u8::try_from(skill).ok()?;
    // SAFETY: see above.
    let table = match unsafe { table.as_ref() } {
        None => SkillTable::default(),
        Some(t) => t.table()?,
    };
    Some((table, skill))
}

/// One number of the table at `skill`'s level, or `ERROR_ARGUMENT` (as
/// `i64`).
///
/// # Safety
/// `table` is null or valid.
unsafe fn skill_number(
    table: *const SxSkillTable,
    skill: u32,
    read: fn(&SkillTable, u8) -> u32,
) -> i64 {
    // SAFETY: see above.
    match unsafe { table_and_skill(table, skill) } {
        Some((t, s)) => i64::from(read(&t, s)),
        None => i64::from(ERROR_ARGUMENT),
    }
}

/// `sx_skill_table_think_ticks_for`.
///
/// # Safety
/// `table` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_skill_table_think_ticks_for(
    table: *const SxSkillTable,
    skill: u32,
) -> i64 {
    // SAFETY: see above.
    unsafe { skill_number(table, skill, SkillTable::think_ticks_for) }
}

/// `sx_skill_table_timing_spread_for`.
///
/// # Safety
/// `table` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_skill_table_timing_spread_for(
    table: *const SxSkillTable,
    skill: u32,
) -> i64 {
    // SAFETY: see above.
    unsafe { skill_number(table, skill, SkillTable::timing_spread_for) }
}

/// `sx_skill_table_hold_bonus_for`.
///
/// # Safety
/// `table` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_skill_table_hold_bonus_for(
    table: *const SxSkillTable,
    skill: u32,
) -> i64 {
    // SAFETY: see above.
    unsafe { skill_number(table, skill, SkillTable::hold_bonus_for) }
}

/// `sx_skill_table_give_up_ticks_for`.
///
/// # Safety
/// `table` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_skill_table_give_up_ticks_for(
    table: *const SxSkillTable,
    skill: u32,
) -> i64 {
    // SAFETY: see above.
    unsafe { skill_number(table, skill, SkillTable::give_up_ticks_for) }
}

/// `sx_skill_table_spot_rank_error_for`.
///
/// # Safety
/// `table` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_skill_table_spot_rank_error_for(
    table: *const SxSkillTable,
    skill: u32,
) -> i32 {
    // SAFETY: see above.
    match unsafe { table_and_skill(table, skill) } {
        Some((t, s)) => i32::from(t.spot_rank_error_for(s)),
        None => ERROR_ARGUMENT,
    }
}

/// `sx_skill_table_pick_for`.
///
/// # Safety
/// `table` is null or valid; `pick` and `top` are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_skill_table_pick_for(
    table: *const SxSkillTable,
    skill: u32,
    pick: *mut u32,
    top: *mut u32,
) -> i32 {
    if pick.is_null() || top.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let Some((t, s)) = (unsafe { table_and_skill(table, skill) }) else {
        return ERROR_ARGUMENT;
    };
    let level = t.level(s);
    let coded = SxSkillTable::of(&t);
    // SAFETY: non-null and writable (see above).
    unsafe {
        pick.write(coded.pick[level]);
        top.write(coded.pick_top[level]);
    }
    0
}

/// `sx_squad_decider`: the squad's decider, lent until `sx_squad_free`;
/// `sx_decider_free` does nothing to it. One handle per squad. Null for a
/// null squad.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_decider(squad: *mut SxSquad) -> *mut SxDecider {
    // SAFETY: null or valid (see above).
    let Some(s) = (unsafe { squad.as_mut() }) else {
        return std::ptr::null_mut();
    };
    if s.decider.is_null() {
        s.decider = create(|| {
            Some(SxDecider::held(Held::Inside(Inside {
                root: squad.cast(),
                get: squad_decider,
            })))
        });
    }
    s.decider
}

/// `sx_squad_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxSquadTrace {
    /// `SX_SQUAD_TRACE_*`.
    pub kind: u32,
    /// `DECIDE`, `ABORT`, `PICKED`: the tactic.
    pub tactic: u32,
    /// `DECIDE`, `ABORT`, `PICKED`: the target, if `has_target`.
    pub target: u32,
    /// `ASSIGNED`, `SPOT_*`, `DELAY`: the member.
    pub member: u32,
    /// `ASSIGNED`: the slot.
    pub slot: u32,
    /// `ABORT`: the rule's index.
    pub abort: u32,
    /// `THINKING`: the plan.
    pub plan: u32,
    /// `THINKING`: when it becomes active.
    pub until: u32,
    /// `PICKED`: the score.
    pub score: u32,
    /// `DELAY`: the ticks until it arrives, if `has_delay`.
    pub delay: u32,
    /// `SPOT_*`: the spot's index, if `has_spot`.
    pub spot: u32,
    /// `SPOT_PICKED` without a spot: the unchecked wish point.
    pub pos: [i32; 3],
    /// 1 if `target` is set.
    pub has_target: u8,
    /// 1 if `delay` is set (0: the order does not reach the member).
    pub has_delay: u8,
    /// 1 if `spot` is set.
    pub has_spot: u8,
    /// `ABORT`, `TRIGGER`: 1 when it fired.
    pub fired: u8,
    /// `TARGET`: 1 when the targets slot kept it.
    pub kept: u8,
    /// `SPOT_RANKED`: 1 when the spot has every wanted tag.
    pub fits: u8,
    /// `ASSIGNED`: the member cost (saturated).
    pub cost: i64,
    /// `SPOT_RANKED`: the distance from the wish point (saturated).
    pub distance: i64,
    /// `TARGET`: the target.
    pub fact: SxFact,
    /// `REPORT`: the report.
    pub msg: SxMessage,
    /// `DECIDE`: the decider's record.
    pub decide: SxDecideTrace,
    /// `KNOWLEDGE`: the knowledge record.
    pub knowledge: SxKnowledgeTrace,
    /// `AREAS`: the areas record.
    pub areas: SxAreasTrace,
    /// `INBOX`: the inbox record.
    pub inbox: SxMessageTrace,
}

impl SxSquadTrace {
    pub(crate) fn of_code(c: SquadTraceCode) -> Self {
        Self {
            kind: c.kind,
            tactic: c.tactic,
            target: c.target.unwrap_or_default(),
            member: c.member,
            slot: c.slot,
            abort: c.abort,
            plan: c.plan,
            until: c.until,
            score: c.score,
            delay: c.delay.unwrap_or_default(),
            spot: c.spot.unwrap_or_default(),
            pos: c.pos,
            has_target: u8::from(c.target.is_some()),
            has_delay: u8::from(c.delay.is_some()),
            has_spot: u8::from(c.spot.is_some()),
            fired: u8::from(c.fired),
            kept: u8::from(c.kept),
            fits: u8::from(c.fits),
            cost: c.cost,
            distance: c.distance,
            fact: c.fact,
            msg: c.msg,
            decide: SxDecideTrace::of_code(c.decide),
            knowledge: SxKnowledgeTrace {
                kind: c.knowledge.kind,
                fact: c.knowledge.fact,
            },
            areas: SxAreasTrace {
                kind: c.areas.kind,
                index: c.areas.index,
                area: c.areas.area,
            },
            inbox: SxMessageTrace::of_code(c.inbox),
        }
    }

    /// The record of a squad trace.
    pub fn of(event: SquadTrace) -> Self {
        Self::of_code(convert::squad_trace(event))
    }
}

/// `sx_squad_trace_fn`.
pub type SquadTraceFn = unsafe extern "C" fn(*mut c_void, *const SxSquadTrace);

/// `sx_squad_set_trace`.
///
/// # Safety
/// `squad` is null or valid; `trace` and `context` stay valid while tracing
/// is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_trace(
    squad: *mut SxSquad,
    trace: Option<SquadTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(
        unsafe { squad.as_mut() }.map(|s| &mut s.trace),
        trace,
        context,
    )
}

/// `sx_squad_set_shout`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_shout(squad: *mut SxSquad, id: u32, range: i64) -> i32 {
    i32::from(squad!(squad).set_channel(id, Channel::Shout { range }))
}

/// `sx_squad_set_radio`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_radio(
    squad: *mut SxSquad,
    id: u32,
    base: u32,
    per_unit: u32,
) -> i32 {
    i32::from(squad!(squad).set_channel(id, Channel::Radio { base, per_unit }))
}

/// `sx_squad_set_silent`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_silent(squad: *mut SxSquad, id: u32) -> i32 {
    i32::from(squad!(squad).set_channel(id, Channel::None))
}

/// `sx_squad_set_channel_custom`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_channel_custom(
    squad: *mut SxSquad,
    member: u32,
    id: u32,
) -> i32 {
    i32::from(squad!(squad).set_channel(member, Channel::Custom(id)))
}

/// `sx_squad_set_danger_area`.
///
/// # Safety
/// `squad` is null or valid; `area` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_danger_area(squad: *mut SxSquad, area: *const SxArea) -> i32 {
    let s = &mut squad!(squad);
    s.set_danger_area(*input!(area));
    0
}

/// `sx_squad_danger_area`.
///
/// # Safety
/// `squad` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_danger_area(squad: *mut SxSquad, out: *mut SxArea) -> i32 {
    let s = &mut squad!(squad);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { put(out, s.danger_area()) };
    0
}

/// `sx_squad_set_target_kind`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_target_kind(squad: *mut SxSquad, kind: u32) -> i32 {
    squad!(squad).set_target_kind(kind);
    0
}

/// `sx_squad_set_extra_columns`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_extra_columns(squad: *mut SxSquad, count: u32) -> i32 {
    if squad!(squad).set_extra_columns(count as usize) {
        0
    } else {
        ERROR_ARGUMENT
    }
}

/// `sx_squad_extra_start`: the first column of the game's own (8, plus one
/// for every role beyond two).
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_extra_start(squad: *mut SxSquad) -> i32 {
    to_i32(squad!(squad).extra_start())
}

/// `sx_squad_set_spot_radius`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_spot_radius(squad: *mut SxSquad, radius: i64) -> i32 {
    let s = &mut squad!(squad);
    let Some(radius) = convert::spot_radius(radius) else {
        return ERROR_ARGUMENT;
    };
    s.set_spot_radius(radius);
    0
}

/// `sx_squad_knowledge`: the leader's knowledge, lent until `sx_squad_free`;
/// `sx_knowledge_free` does nothing to it. One handle per squad, reaching
/// into the squad anew on every call. Null for a null squad.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_knowledge(squad: *mut SxSquad) -> *mut SxKnowledge {
    // SAFETY: null or valid (see above).
    let Some(s) = (unsafe { squad.as_mut() }) else {
        return std::ptr::null_mut();
    };
    if s.knowledge.is_null() {
        s.knowledge = create(|| {
            Some(SxKnowledge::inside(Inside {
                root: squad.cast(),
                get: squad_knowledge_of,
            }))
        });
    }
    s.knowledge
}

/// `sx_squad_areas`: the leader's danger areas, lent as
/// [`sx_squad_knowledge`] is; `sx_areas_free` does nothing to it.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_areas(squad: *mut SxSquad) -> *mut SxAreas {
    // SAFETY: null or valid (see above).
    let Some(s) = (unsafe { squad.as_mut() }) else {
        return std::ptr::null_mut();
    };
    if s.areas.is_null() {
        s.areas = create(|| {
            Some(SxAreas::inside(Inside {
                root: squad.cast(),
                get: squad_areas_of,
            }))
        });
    }
    s.areas
}

/// `sx_squad_observe`.
///
/// # Safety
/// `squad` is null or valid; `fact` is null or valid; `evicted` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_observe(
    squad: *mut SxSquad,
    now: u32,
    fact: *const SxFact,
    evicted: *mut SxFact,
) -> i32 {
    let h = get!(squad);
    let trace = h.trace;
    let s = &mut *h.squad;
    let fact = *input!(fact);
    match traced!(trace, SxSquadTrace::of, |sink| s
        .observe_traced(now, fact, sink))
    {
        Some(gone) => {
            // SAFETY: see above.
            unsafe { put(evicted, gone) };
            1
        }
        None => 0,
    }
}

/// `sx_squad_report`.
///
/// # Safety
/// `squad` is null or valid; `msg` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_report(
    squad: *mut SxSquad,
    msg: *const SxMessage,
    arrive_at: u32,
) -> i32 {
    let h = get!(squad);
    let trace = h.trace;
    let s = &mut *h.squad;
    let msg = *input!(msg);
    i32::from(traced!(trace, SxSquadTrace::of, |sink| s
        .report_traced(msg, arrive_at, sink)))
}

/// `sx_squad_report_danger`: `centre` is 3 values; `shooter` is 3 values or
/// null (unknown). 1 stored, 0 when the danger queue is full.
///
/// # Safety
/// `squad` is null or valid; `centre` is null or 3 readable values;
/// `shooter` is null or 3 readable values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_report_danger(
    squad: *mut SxSquad,
    sender: u32,
    centre: *const i32,
    shooter: *const i32,
    arrive_at: u32,
) -> i32 {
    let s = &mut squad!(squad);
    if centre.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: 3 readable values (see above).
    let centre = unsafe { pos3(centre) };
    let shooter = if shooter.is_null() {
        None
    } else {
        // SAFETY: 3 readable values (see above).
        Some(unsafe { pos3(shooter) })
    };
    i32::from(s.report_danger(sender, centre, shooter, arrive_at))
}

/// `sx_squad_take_activated`: 1 and the plan id in `plan` (may be null), or
/// 0.
///
/// # Safety
/// `squad` is null or valid; `plan` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_take_activated(squad: *mut SxSquad, plan: *mut u32) -> i32 {
    match squad!(squad).take_activated() {
        Some(id) => {
            // SAFETY: null or writable (see above).
            unsafe { put(plan, id) };
            1
        }
        None => 0,
    }
}

/// `sx_squad_report_arrives_at`: 1 and the tick in `at` (may be null), or 0
/// when it never arrives.
///
/// # Safety
/// `squad` is null or valid; `at` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_report_arrives_at(
    squad: *mut SxSquad,
    id: u32,
    now: u32,
    at: *mut u32,
) -> i32 {
    // SAFETY: see above; no rules.
    unsafe { sx_squad_report_arrives_at_with(squad, id, now, std::ptr::null(), at) }
}

/// `sx_squad_report_arrives_at_with`: a custom channel asks `rules`.
///
/// # Safety
/// As [`sx_squad_report_arrives_at`]; `rules` is null or valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_report_arrives_at_with(
    squad: *mut SxSquad,
    id: u32,
    now: u32,
    rules: *const SxSquadRules,
    at: *mut u32,
) -> i32 {
    let s = &mut squad!(squad);
    // SAFETY: null or valid (see above).
    let mut rules = unsafe { CRules::of(rules) };
    guarded(|| {
        let arrives = match rules.as_mut() {
            Some(r) => s.report_arrives_at_with(id, now, r),
            None => s.report_arrives_at(id, now),
        };
        match arrives {
            Some(t) => {
                // SAFETY: null or writable (see above).
                unsafe { put(at, t) };
                1
            }
            None => 0,
        }
    })
}

/// `sx_squad_targets`: returns how many facts were written; `total` (may be
/// null) gets the number of all targets.
///
/// # Safety
/// `squad` is null or valid; `out` is null or has room for `cap` facts;
/// `total` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_targets(
    squad: *mut SxSquad,
    out: *mut SxFact,
    cap: u32,
    total: *mut u32,
) -> i32 {
    let s = &mut squad!(squad);
    if out.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    let mut all = 0usize;
    let mut written = 0usize;
    for (i, f) in s.targets().enumerate() {
        if i < cap as usize {
            // SAFETY: within `cap` (see above).
            unsafe { out.add(i).write(*f) };
            written += 1;
        }
        all += 1;
    }
    // SAFETY: null or writable (see above).
    unsafe { put(total, u32::try_from(all).unwrap_or(u32::MAX)) };
    to_i32(written)
}

/// `sx_squad_add_rush`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_rush(squad: *mut SxSquad) -> i32 {
    to_i32(squad!(squad).add_tactic(tactics::rush()))
}

/// `sx_squad_add_formation`: `shape` is `SX_SHAPE_*`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_formation(
    squad: *mut SxSquad,
    shape: u32,
    spacing: i32,
) -> i32 {
    let s = &mut squad!(squad);
    let Some(shape) = convert::shape(shape) else {
        return ERROR_ARGUMENT;
    };
    to_i32(s.add_tactic(tactics::formation(shape, spacing)))
}

/// `sx_squad_add_flank`: `sides` is 2 or 3.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_flank(squad: *mut SxSquad, sides: u32, dist: i32) -> i32 {
    let s = &mut squad!(squad);
    match convert::flank_sides(sides) {
        Some(sides) => to_i32(s.add_tactic(tactics::flank(sides, dist))),
        None => ERROR_ARGUMENT,
    }
}

/// `sx_squad_add_ambush`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_ambush(squad: *mut SxSquad) -> i32 {
    to_i32(squad!(squad).add_tactic(tactics::ambush()))
}

/// `sx_squad_add_focus`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_focus(squad: *mut SxSquad) -> i32 {
    to_i32(squad!(squad).add_tactic(tactics::focus()))
}

/// `sx_squad_add_isolate`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_isolate(squad: *mut SxSquad) -> i32 {
    to_i32(squad!(squad).add_tactic(tactics::isolate()))
}

/// `sx_squad_add_fall_back`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_fall_back(squad: *mut SxSquad) -> i32 {
    to_i32(squad!(squad).add_tactic(tactics::fall_back()))
}

/// `sx_squad_add_tactic`: takes `tactic` over in any case.
///
/// # Safety
/// `squad` is null or valid; `tactic` is null or from [`sx_tactic_new`] and
/// not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_tactic(squad: *mut SxSquad, tactic: *mut SxTactic) -> i32 {
    if tactic.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above; the box is taken back exactly once.
    let tactic = unsafe { Box::from_raw(tactic) };
    let s = &mut squad!(squad);
    to_i32(s.add_tactic(tactic.0))
}

/// Writes a plan error's details (each pointer may be null).
///
/// # Safety
/// `tactic` and `input` are null or writable.
pub(crate) unsafe fn plan_error(e: PlanError, tactic: *mut u32, input: *mut u32) -> i32 {
    let (code, t, i) = convert::plan_error(e);
    // SAFETY: see above.
    unsafe {
        put(tactic, t);
        put(input, i);
    }
    code
}

/// `sx_squad_validate`.
///
/// # Safety
/// `squad` is null or valid; `tactic` and `input` are null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_validate(
    squad: *mut SxSquad,
    tactic: *mut u32,
    input: *mut u32,
) -> i32 {
    match squad!(squad).validate() {
        Ok(()) => 0,
        // SAFETY: see above.
        Err(e) => unsafe { plan_error(e, tactic, input) },
    }
}

/// The extra columns and spots of `sx_squad_plan` and `sx_squad_check`, or
/// `None` for a null list with a count.
///
/// # Safety
/// Each list is null or holds its count.
pub(crate) unsafe fn lists<'a>(
    extra: *const i32,
    extra_count: u32,
    spots: *const SxSpot,
    spot_count: u32,
) -> Option<(&'a [i32], &'a [Spot])> {
    if (extra.is_null() && extra_count > 0) || (spots.is_null() && spot_count > 0) {
        return None;
    }
    // SAFETY: see above.
    unsafe {
        Some((
            slice_of(extra, extra_count as usize),
            slice_of(spots, spot_count as usize),
        ))
    }
}

/// `sx_squad_plan`: `sx_squad_plan_with` without rules.
///
/// # Safety
/// `squad` is null or valid; `extra` and `spots` are null or hold their
/// counts; `plan` is null or writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_squad_plan(
    squad: *mut SxSquad,
    now: u32,
    extra: *const i32,
    extra_count: u32,
    spots: *const SxSpot,
    spot_count: u32,
    seed: u64,
    plan: *mut u32,
) -> i32 {
    // SAFETY: see above; no rules.
    unsafe {
        sx_squad_plan_with(
            squad,
            now,
            extra,
            extra_count,
            spots,
            spot_count,
            seed,
            std::ptr::null(),
            plan,
        )
    }
}

/// `sx_squad_plan_with`: as `sx_squad_plan`, the slots going to `rules`
/// (null: the built-in ones).
///
/// # Safety
/// As [`sx_squad_plan`]; `rules` is null or valid for the call.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_squad_plan_with(
    squad: *mut SxSquad,
    now: u32,
    extra: *const i32,
    extra_count: u32,
    spots: *const SxSpot,
    spot_count: u32,
    seed: u64,
    rules: *const SxSquadRules,
    plan: *mut u32,
) -> i32 {
    let h = get!(squad);
    let trace = h.trace;
    let s = &mut *h.squad;
    // SAFETY: see above.
    let Some((extra, spots)) = (unsafe { lists(extra, extra_count, spots, spot_count) }) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: null or valid (see above).
    let mut rules = unsafe { CRules::of(rules) };
    guarded(|| {
        let made = match rules.as_mut() {
            Some(r) => traced!(trace, SxSquadTrace::of, |sink| s
                .plan_traced(now, extra, spots, seed, r, sink)),
            None if s.needs_rules() => Err(PlanError::NeedsRules),
            None => traced!(trace, SxSquadTrace::of, |sink| s.plan_traced(
                now,
                extra,
                spots,
                seed,
                &mut BuiltInRules,
                sink
            )),
        };
        match made {
            Ok(Some(id)) => {
                // SAFETY: see above.
                unsafe { put(plan, id) };
                1
            }
            Ok(None) => 0,
            // SAFETY: null details.
            Err(e) => unsafe { plan_error(e, std::ptr::null_mut(), std::ptr::null_mut()) },
        }
    })
}

/// `sx_squad_check`: `sx_squad_check_with` without rules.
///
/// # Safety
/// As [`sx_squad_plan`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_squad_check(
    squad: *mut SxSquad,
    now: u32,
    extra: *const i32,
    extra_count: u32,
    spots: *const SxSpot,
    spot_count: u32,
    seed: u64,
    plan: *mut u32,
) -> i32 {
    // SAFETY: see above; no rules.
    unsafe {
        sx_squad_check_with(
            squad,
            now,
            extra,
            extra_count,
            spots,
            spot_count,
            seed,
            std::ptr::null(),
            plan,
        )
    }
}

/// `sx_squad_check_with`: as `sx_squad_check`, the slots going to `rules`
/// (null: the built-in ones).
///
/// # Safety
/// As [`sx_squad_plan_with`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_squad_check_with(
    squad: *mut SxSquad,
    now: u32,
    extra: *const i32,
    extra_count: u32,
    spots: *const SxSpot,
    spot_count: u32,
    seed: u64,
    rules: *const SxSquadRules,
    plan: *mut u32,
) -> i32 {
    let h = get!(squad);
    let trace = h.trace;
    let s = &mut *h.squad;
    // SAFETY: see above.
    let Some((extra, spots)) = (unsafe { lists(extra, extra_count, spots, spot_count) }) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: null or valid (see above).
    let mut rules = unsafe { CRules::of(rules) };
    guarded(|| {
        let checked = match rules.as_mut() {
            Some(r) => traced!(trace, SxSquadTrace::of, |sink| s
                .check_traced(now, extra, spots, seed, r, sink)),
            None if s.needs_rules() => Err(PlanError::NeedsRules),
            None => traced!(trace, SxSquadTrace::of, |sink| s.check_traced(
                now,
                extra,
                spots,
                seed,
                &mut BuiltInRules,
                sink
            )),
        };
        match checked.map(convert::squad_status) {
            Ok((code, id)) => {
                if let Some(id) = id {
                    // SAFETY: see above.
                    unsafe { put(plan, id) };
                }
                code
            }
            // SAFETY: null details.
            Err(e) => unsafe { plan_error(e, std::ptr::null_mut(), std::ptr::null_mut()) },
        }
    })
}

/// `sx_squad_orders_due`.
///
/// # Safety
/// `squad` is null or valid; `out` is null or has room for `cap` orders.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_orders_due(
    squad: *mut SxSquad,
    now: u32,
    out: *mut SxOrder,
    cap: u32,
) -> i32 {
    let s = get!(squad);
    // A crew takes the orders of its own squad (sx_crew_tick).
    if (out.is_null() && cap > 0) || s.squad.is_lent() {
        return ERROR_ARGUMENT;
    }
    traced!(s.trace, SxSquadTrace::of, |sink| s
        .squad
        .orders_due_traced(now, &mut s.due, sink));
    // Orders held back from an earlier call may be stale by now: drop those
    // of another plan and of members no longer active.
    let squad = &s.squad;
    let plan = squad.active_plan().map(|p| p.id);
    s.due.retain(|o| {
        let active = squad
            .group()
            .index(o.member)
            .and_then(|i| squad.group().get(i))
            .is_some_and(|m| m.state == MemberState::Active);
        active && Some(o.goal.plan) == plan
    });
    let n = s.due.len().min(cap as usize);
    for (i, o) in s.due.drain(..n).enumerate() {
        // SAFETY: within `cap` (see above).
        unsafe { out.add(i).write(SxOrder::of(o.member, &o.goal)) };
    }
    to_i32(n)
}

/// `sx_squad_signal`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_signal(squad: *mut SxSquad, now: u32) -> i32 {
    // SAFETY: see above; no rules.
    unsafe { sx_squad_signal_with(squad, now, std::ptr::null()) }
}

/// `sx_squad_signal_with`: the signal over custom channels too.
///
/// # Safety
/// `squad` is null or valid; `rules` is null or valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_signal_with(
    squad: *mut SxSquad,
    now: u32,
    rules: *const SxSquadRules,
) -> i32 {
    let h = get!(squad);
    let trace = h.trace;
    let s = &mut *h.squad;
    // SAFETY: null or valid (see above).
    let mut rules = unsafe { CRules::of(rules) };
    guarded(|| {
        let given = match rules.as_mut() {
            Some(r) => traced!(trace, SxSquadTrace::of, |sink| s
                .signal_traced(now, r, sink)),
            None if s.needs_rules() => false,
            None => traced!(trace, SxSquadTrace::of, |sink| s.signal_traced(
                now,
                &mut BuiltInRules,
                sink
            )),
        };
        i32::from(given)
    })
}

/// `sx_squad_arrived`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_arrived(squad: *mut SxSquad, id: u32, now: u32) -> i32 {
    i32::from(squad!(squad).arrived(id, now))
}

/// `sx_squad_signal_given`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_signal_given(squad: *mut SxSquad) -> i32 {
    i32::from(squad!(squad).signal_given())
}

/// Writes `plan` to `out` and its first `cap` goals to `goals`; 1, or 0
/// without a plan.
///
/// # Safety
/// `out` is null or writable; `goals` has room for `cap` orders.
unsafe fn write_plan(plan: Option<&Plan>, out: *mut SxPlan, goals: *mut SxOrder, cap: u32) -> i32 {
    let Some(p) = plan else {
        return 0;
    };
    let value = SxPlan {
        id: p.id,
        tactic: u32::try_from(p.tactic).unwrap_or(u32::MAX),
        target: p.target.unwrap_or(0),
        made_at: p.made_at,
        active_at: p.active_at,
        goal_count: u32::try_from(p.goals.len()).unwrap_or(u32::MAX),
        has_target: u8::from(p.target.is_some()),
    };
    // SAFETY: see above.
    unsafe { put(out, value) };
    for (i, (member, goal)) in p.goals.iter().take(cap as usize).enumerate() {
        // SAFETY: within `cap` (see above).
        unsafe { goals.add(i).write(SxOrder::of(*member, goal)) };
    }
    1
}

/// `sx_squad_pending_plan`.
///
/// # Safety
/// `squad` is null or valid; `out` is null or writable; `goals` is null or
/// has room for `cap` orders.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_pending_plan(
    squad: *mut SxSquad,
    out: *mut SxPlan,
    goals: *mut SxOrder,
    cap: u32,
) -> i32 {
    let s = &mut squad!(squad);
    if goals.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { write_plan(s.pending_plan(), out, goals, cap) }
}

/// `sx_squad_active_plan`.
///
/// # Safety
/// As [`sx_squad_pending_plan`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_active_plan(
    squad: *mut SxSquad,
    out: *mut SxPlan,
    goals: *mut SxOrder,
    cap: u32,
) -> i32 {
    let s = &mut squad!(squad);
    if goals.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    unsafe { write_plan(s.active_plan(), out, goals, cap) }
}

// ---- keeper -------------------------------------------------------------------

/// `sx_keeper`: the keeper and where `sx_keeper_set_trace` sends its
/// records.
pub struct SxKeeper(
    pub(crate) Held<GoalKeeper>,
    pub(crate) TraceSlot<SxKeeperTrace>,
);

/// `sx_keeper_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxKeeperTrace {
    /// `SX_KEEPER_TRACE_*`.
    pub kind: u32,
    /// Ticks since the goal was postponed; 0 when it is not.
    pub waited: u32,
    /// The give-up ticks it was checked against.
    pub limit: u32,
    /// 1 when the goal counts as broken.
    pub broken: u8,
}

impl SxKeeperTrace {
    pub(crate) fn of_code(c: KeeperTraceCode) -> Self {
        Self {
            kind: c.kind,
            waited: c.waited,
            limit: c.limit,
            broken: u8::from(c.broken),
        }
    }

    /// The record of a keeper trace.
    pub fn of(event: KeeperTrace) -> Self {
        Self::of_code(convert::keeper_trace(event))
    }
}

/// `sx_keeper_trace_fn`.
pub type KeeperTraceFn = unsafe extern "C" fn(*mut c_void, *const SxKeeperTrace);

/// `sx_keeper_set_trace`.
///
/// # Safety
/// `keeper` is null or valid; `trace` and `context` stay valid while
/// tracing is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_set_trace(
    keeper: *mut SxKeeper,
    trace: Option<KeeperTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { keeper.as_mut() }.map(|k| &mut k.1), trace, context)
}

/// The keeper behind a handle, or `ERROR_ARGUMENT` (also for a stale lent
/// one).
macro_rules! keeper {
    ($p:expr) => {
        match get!($p).0.get_mut() {
            Some(k) => k,
            None => return ERROR_ARGUMENT,
        }
    };
}

/// `sx_keeper_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_keeper_new() -> *mut SxKeeper {
    create(|| Some(SxKeeper(Held::Own(GoalKeeper::new()), None)))
}

/// `sx_keeper_free`: does nothing for a keeper lent by a crew.
///
/// # Safety
/// `keeper` is null, from [`sx_keeper_new`] and not used again, or lent by
/// `sx_crew_keeper`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_free(keeper: *mut SxKeeper) {
    // SAFETY: null or valid (see above).
    if unsafe { keeper.as_ref() }.is_some_and(|k| k.0.is_lent()) {
        return;
    }
    // SAFETY: see above.
    unsafe { destroy(keeper) }
}

/// `sx_keeper_set`.
///
/// # Safety
/// `keeper` is null or valid; `goal` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_set(
    keeper: *mut SxKeeper,
    now: u32,
    goal: *const SxGoal,
) -> i32 {
    let k = keeper!(keeper);
    let Some(goal) = input!(goal).goal() else {
        return ERROR_ARGUMENT;
    };
    k.set(now, goal);
    0
}

/// `sx_keeper_goal`.
///
/// # Safety
/// `keeper` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_goal(keeper: *mut SxKeeper, out: *mut SxGoal) -> i32 {
    let k = keeper!(keeper);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match k.goal() {
        Some(g) => {
            // SAFETY: see above.
            unsafe { put(out, SxGoal::of(g)) };
            1
        }
        None => 0,
    }
}

/// `sx_keeper_clear`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_clear(keeper: *mut SxKeeper) -> i32 {
    keeper!(keeper).clear();
    0
}

/// `sx_keeper_goal_postponed`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_goal_postponed(keeper: *mut SxKeeper, now: u32) -> i32 {
    keeper!(keeper).goal_postponed(now);
    0
}

/// `sx_keeper_goal_won`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_goal_won(keeper: *mut SxKeeper, now: u32) -> i32 {
    keeper!(keeper).goal_won(now);
    0
}

/// `sx_keeper_suspend`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_suspend(keeper: *mut SxKeeper) -> i32 {
    keeper!(keeper).suspend();
    0
}

/// `sx_keeper_unsuspend`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_unsuspend(keeper: *mut SxKeeper, now: u32) -> i32 {
    keeper!(keeper).unsuspend(now);
    0
}

/// `sx_keeper_broken`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_broken(
    keeper: *mut SxKeeper,
    now: u32,
    give_up_ticks: u32,
) -> i32 {
    let trace = get!(keeper).1;
    let k = keeper!(keeper);
    i32::from(traced!(trace, SxKeeperTrace::of, |sink| k.broken_traced(
        now,
        give_up_ticks,
        sink
    )))
}

/// `sx_keeper_inputs`.
///
/// # Safety
/// `keeper` is null or valid; `out` is null or has room for 5 integers.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_keeper_inputs(
    keeper: *mut SxKeeper,
    now: u32,
    x: i32,
    y: i32,
    z: i32,
    metric: u32,
    signal_given: u32,
    discipline: u32,
    out: *mut i32,
) -> i32 {
    let k = keeper!(keeper);
    let Some(metric) = convert::metric(metric) else {
        return ERROR_ARGUMENT;
    };
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    let inputs = k.inputs(now, [x, y, z], metric, signal_given != 0, discipline);
    for (i, v) in inputs.iter().enumerate() {
        // SAFETY: room for 5 (see above).
        unsafe { out.add(i).write(*v) };
    }
    0
}

/// `sx_keeper_add_goal_option`.
///
/// # Safety
/// `decider` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_add_goal_option(decider: *mut SxDecider, weight: u32) -> i32 {
    // SAFETY: see above.
    let Some(d) = (unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| to_i32(GoalKeeper::add_goal_option(d.decider_mut(), weight)))
}

/// `sx_keeper_set_current`.
///
/// # Safety
/// `keeper` and `decider` are null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_set_current(
    keeper: *const SxKeeper,
    decider: *mut SxDecider,
    option: u32,
    discipline: u32,
) -> i32 {
    // SAFETY: see above.
    let (Some(k), Some(d)) = (unsafe { keeper.as_ref() }, unsafe { decider.as_mut() }) else {
        return ERROR_ARGUMENT;
    };
    let Some(k) = k.0.get() else {
        return ERROR_ARGUMENT;
    };
    k.set_current(d.decider_mut(), option, discipline);
    0
}

// ---- goal helpers and reports --------------------------------------------------

/// `sx_goal_start_tick`.
///
/// # Safety
/// `goal` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_start_tick(
    goal: *const SxGoal,
    received: u32,
    out: *mut u32,
) -> i32 {
    let Some(goal) = input!(goal).goal() else {
        return ERROR_ARGUMENT;
    };
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match goal.start_tick(received) {
        Some(tick) => {
            // SAFETY: non-null and writable (see above).
            unsafe { out.write(tick) };
            1
        }
        None => 0,
    }
}

/// `sx_goal_awaits_arrival`.
///
/// # Safety
/// `goal` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_goal_awaits_arrival(goal: *const SxGoal) -> i32 {
    let Some(goal) = input!(goal).goal() else {
        return ERROR_ARGUMENT;
    };
    i32::from(goal.awaits_arrival())
}

/// `sx_report_enemy`.
///
/// # Safety
/// `fact` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_report_enemy(
    sender: u32,
    fact: *const SxFact,
    out: *mut SxMessage,
) -> i32 {
    let fact = input!(fact);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: non-null and writable (see above).
    unsafe { put(out, SxMessage::enemy(sender, *fact)) };
    0
}

/// `sx_report_arrived`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_report_arrived(sender: u32, plan: u32, out: *mut SxMessage) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: non-null and writable (see above).
    unsafe { put(out, SxMessage::arrived(sender, plan)) };
    0
}

/// `sx_report_goal_broken`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_report_goal_broken(sender: u32, plan: u32, out: *mut SxMessage) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: non-null and writable (see above).
    unsafe { put(out, SxMessage::goal_broken(sender, plan)) };
    0
}
