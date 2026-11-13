//! `sx_squad_*`, `sx_tactic_*`, `sx_keeper_*`, `sx_gather`: group tactics,
//! see `sagax.h`.

use sagax_decide::group::{
    Abort, Channel, Goal, GoalKeeper, GoalKind, Member, MemberState, Nearby, Order, Place, Plan,
    PlanError, Shape, Side, SkillTable, Slot, Spot, Squad, SquadConfig, Start, Status, Tactic,
    Trigger, gather, tactics,
};
use sagax_decide::{Consideration, Curve, Pick};

use super::decide::{SxConsideration, SxDecider};
use super::memory::{SxArea, SxAreas, SxFact, SxKnowledge, SxMessage, metric_of, pos3, put};
use super::{ERROR_ARGUMENT, ERROR_PLAN, create, destroy, guarded, slice_of, to_i32};
use sagax_decide::memory::Metric;

// Numbers of the C interface (`SX_*` in `sagax.h`).

pub(crate) const PLACE_NONE: u32 = 0;
pub(crate) const PLACE_OFFSET: u32 = 1;
pub(crate) const PLACE_SIDE: u32 = 2;
pub(crate) const PLACE_BETWEEN: u32 = 3;
pub(crate) const PLACE_TAGGED: u32 = 4;
pub(crate) const PLACE_HERE: u32 = 5;

pub(crate) const GOAL_MOVE: u32 = 0;
pub(crate) const GOAL_HOLD: u32 = 1;
pub(crate) const GOAL_ATTACK: u32 = 2;
pub(crate) const GOAL_WAIT: u32 = 3;
pub(crate) const GOAL_RETREAT: u32 = 4;
pub(crate) const GOAL_CUSTOM: u32 = 5;

pub(crate) const START_NOW: u32 = 0;
pub(crate) const START_AFTER_TICKS: u32 = 1;
pub(crate) const START_ON_SIGNAL: u32 = 2;

pub(crate) const SHAPE_LINE: u32 = 0;
pub(crate) const SHAPE_WEDGE: u32 = 1;
pub(crate) const SHAPE_COLUMN: u32 = 2;

pub(crate) const SQUAD_KEEP: i32 = 0;
pub(crate) const SQUAD_REPLAN: i32 = 1;
pub(crate) const SQUAD_SIGNAL: i32 = 2;
pub(crate) const SQUAD_NO_PLAN: i32 = 3;

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
    fn of(m: &Member) -> Self {
        Self {
            id: m.id,
            pos: m.pos,
            roles: m.roles,
            skill: m.skill,
            state: m.state as u8,
        }
    }

    pub(crate) fn member(&self) -> Option<Member> {
        let state = match self.state {
            0 => MemberState::Active,
            1 => MemberState::Suspended,
            2 => MemberState::Gone,
            _ => return None,
        };
        Some(Member {
            id: self.id,
            pos: self.pos,
            roles: self.roles,
            skill: self.skill,
            state,
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
    /// The place, `Some(None)` for `SX_PLACE_NONE`, `None` if invalid.
    fn place(&self) -> Option<Option<Place>> {
        Some(Some(match self.kind {
            PLACE_NONE => return Some(None),
            PLACE_OFFSET => Place::Offset([self.x, self.y, self.z]),
            PLACE_SIDE => Place::Side(match self.value {
                0 => Side::Front,
                1 => Side::Left,
                2 => Side::Right,
                3 => Side::Rear,
                _ => return None,
            }),
            PLACE_BETWEEN => Place::Between,
            PLACE_TAGGED => Place::Tagged(self.value),
            PLACE_HERE => Place::Here,
            _ => return None,
        }))
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
        Some(Slot {
            roles: self.roles,
            min: self.min,
            max: self.max,
            gather: self.gather.place()?,
            place: self.place.place()??,
            kind: kind_of(self.kind, self.custom)?,
            start: start_of(self.start, self.start_ticks)?,
            want_tags: self.want_tags,
            dist: self.dist,
            gather_dist: self.gather_dist,
        })
    }
}

fn kind_of(code: u32, custom: u32) -> Option<GoalKind> {
    Some(match code {
        GOAL_MOVE => GoalKind::Move,
        GOAL_HOLD => GoalKind::Hold,
        GOAL_ATTACK => GoalKind::Attack,
        GOAL_WAIT => GoalKind::Wait,
        GOAL_RETREAT => GoalKind::Retreat,
        GOAL_CUSTOM => GoalKind::Custom(custom),
        _ => return None,
    })
}

fn kind_code(kind: GoalKind) -> (u32, u32) {
    match kind {
        GoalKind::Move => (GOAL_MOVE, 0),
        GoalKind::Hold => (GOAL_HOLD, 0),
        GoalKind::Attack => (GOAL_ATTACK, 0),
        GoalKind::Wait => (GOAL_WAIT, 0),
        GoalKind::Retreat => (GOAL_RETREAT, 0),
        GoalKind::Custom(c) => (GOAL_CUSTOM, c),
    }
}

fn start_of(code: u32, ticks: u32) -> Option<Start> {
    Some(match code {
        START_NOW => Start::Now,
        START_AFTER_TICKS => Start::AfterTicks(ticks),
        START_ON_SIGNAL => Start::OnSignal,
        _ => return None,
    })
}

fn start_code(start: Start) -> (u32, u32) {
    match start {
        Start::Now => (START_NOW, 0),
        Start::AfterTicks(n) => (START_AFTER_TICKS, n),
        Start::OnSignal => (START_ON_SIGNAL, 0),
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
        let (kind, custom) = kind_code(g.kind);
        let (start, start_ticks) = start_code(g.start);
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
            kind: kind_of(self.kind, self.custom)?,
            pos: (self.has_pos != 0).then_some(self.pos),
            unchecked: self.unchecked != 0,
            then_pos: (self.has_then_pos != 0).then_some(self.then_pos),
            then_unchecked: self.then_unchecked != 0,
            target: (self.has_target != 0).then_some(self.target),
            start: start_of(self.start, self.start_ticks)?,
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
    /// `SX_PICK_*` per level.
    pub pick: [u32; 4],
    /// `top` of the pick per level.
    pub pick_top: [u32; 4],
    /// Ticks from making a plan until it is active.
    pub think_ticks: [u32; 4],
    /// Most ticks a member starts late.
    pub timing_spread: [u32; 4],
    /// Bonus of the active plan, and the members' discipline.
    pub hold_bonus: [u32; 4],
    /// Ticks a member may postpone its goal.
    pub give_up_ticks: [u32; 4],
    /// Most ranks a member's spot may be off the best one.
    pub spot_rank_error: [u8; 4],
}

impl SxSkillTable {
    fn of(t: &SkillTable) -> Self {
        let pick = t.pick.map(|p| match p {
            Pick::Best => (0, 0),
            Pick::Weighted { top } => (1, top),
            Pick::Uniform { top } => (2, top),
        });
        Self {
            pick: pick.map(|p| p.0),
            pick_top: pick.map(|p| p.1),
            think_ticks: t.think_ticks,
            timing_spread: t.timing_spread,
            hold_bonus: t.hold_bonus,
            give_up_ticks: t.give_up_ticks,
            spot_rank_error: t.spot_rank_error,
        }
    }

    fn table(&self) -> Option<SkillTable> {
        let mut pick = [Pick::Best; 4];
        for (i, p) in pick.iter_mut().enumerate() {
            *p = Pick::from_u32(self.pick[i], self.pick_top[i])?;
        }
        Some(SkillTable {
            pick,
            think_ticks: self.think_ticks,
            spot_rank_error: self.spot_rank_error,
            timing_spread: self.timing_spread,
            hold_bonus: self.hold_bonus,
            give_up_ticks: self.give_up_ticks,
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
    let Some(metric) = metric_of(metric) else {
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
    create(|| {
        let min_skill = u8::try_from(min_skill).ok()?;
        Some(SxTactic(Tactic {
            slots: Vec::new(),
            considerations: Vec::new(),
            weight,
            aborts: Vec::new(),
            trigger: Trigger::None,
            min_skill,
            needs_target: needs_target != 0,
        }))
    })
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
    let Some(curve) = Curve::from_u32(c.curve) else {
        return ERROR_ARGUMENT;
    };
    t.0.considerations
        .push(Consideration::new(c.input, curve, c.min, c.max));
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
    get!(tactic).0.aborts.push(Abort {
        input: input as usize,
        at_least,
    });
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

// ---- squad --------------------------------------------------------------------

/// Where a handle keeps its value: its own, or one lent by a crew.
pub(crate) enum Held<T> {
    /// The handle owns the value and frees it with itself.
    Own(T),
    /// A pointer into a crew, which owns the value and outlives the handle
    /// (see `sx_crew_squad`, `sx_crew_keeper`); null once the crew has made
    /// it stale. Freeing the handle does nothing.
    Lent(*mut T),
}

impl<T> Held<T> {
    /// The value, or `None` for a stale lent handle.
    pub(crate) fn get(&self) -> Option<&T> {
        match self {
            Self::Own(t) => Some(t),
            // SAFETY: a lent pointer is null or points into its live crew
            // (the crew nulls it before the value may move, and the C
            // interface is single-threaded per handle, so no other reference
            // into the crew is alive during this call).
            Self::Lent(p) => unsafe { p.as_ref() },
        }
    }

    /// The value, or `None` for a stale lent handle.
    pub(crate) fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Own(t) => Some(t),
            // SAFETY: as in `get`.
            Self::Lent(p) => unsafe { p.as_mut() },
        }
    }

    pub(crate) const fn is_lent(&self) -> bool {
        matches!(self, Self::Lent(_))
    }
}

impl<T> std::ops::Deref for Held<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.get()
            .expect("a lent handle used after its crew changed")
    }
}

impl<T> std::ops::DerefMut for Held<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.get_mut()
            .expect("a lent handle used after its crew changed")
    }
}

/// `sx_squad`.
pub struct SxSquad {
    pub(crate) squad: Held<Squad>,
    /// Orders taken from the squad and not yet handed out (beyond a cap).
    pub(crate) due: Vec<Order>,
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
    let c = SquadConfig::default();
    let metric = match c.metric {
        Metric::Euclid => 0,
        Metric::Manhattan => 1,
        Metric::Chebyshev => 2,
        Metric::Hex => 3,
    };
    // SAFETY: see above.
    unsafe {
        put(
            out,
            SxSquadConfig {
                members: c.members as u32,
                memory: c.memory as u32,
                areas: c.areas as u32,
                inbox: c.inbox as u32,
                dangers: c.dangers as u32,
                metric,
            },
        );
    }
    0
}

/// `sx_squad_new`: null if `config` is null, a capacity is 0 or the metric
/// is unknown.
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
        let squad = Squad::new(SquadConfig {
            members: c.members as usize,
            memory: c.memory as usize,
            areas: c.areas as usize,
            inbox: c.inbox as usize,
            dangers: c.dangers as usize,
            metric: metric_of(c.metric)?,
        })
        .ok()?;
        Some(SxSquad {
            squad: Held::Own(squad),
            due: Vec::new(),
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
    i32::from(squad!(squad).reinstate(id, now))
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
    s.set_skill_table(table);
    0
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
    let level = SkillTable::level(s);
    let coded = SxSkillTable::of(&t);
    // SAFETY: non-null and writable (see above).
    unsafe {
        pick.write(coded.pick[level]);
        top.write(coded.pick_top[level]);
    }
    0
}

/// `sx_discipline`: the old name of `sx_skill_table_hold_bonus_for`.
///
/// # Safety
/// `table` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_discipline(skill: u32, table: *const SxSkillTable) -> i32 {
    // SAFETY: see above.
    let bonus = unsafe { sx_skill_table_hold_bonus_for(table, skill) };
    if bonus < 0 {
        ERROR_ARGUMENT
    } else {
        to_i32(bonus)
    }
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

/// `sx_squad_set_spot_radius`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_spot_radius(squad: *mut SxSquad, radius: i64) -> i32 {
    let s = &mut squad!(squad);
    if radius < 0 {
        s.set_spot_radius(None);
        return 0;
    }
    match u32::try_from(radius) {
        Ok(r) => {
            s.set_spot_radius(Some(r));
            0
        }
        Err(_) => ERROR_ARGUMENT,
    }
}

/// `sx_squad_knowledge`: the leader's knowledge, owned by the squad.
///
/// The pointer aliases the squad: it is valid until the squad is freed, and
/// is only used while no other call on this squad runs (the C interface is
/// single-threaded per handle). It must not be passed to
/// `sx_knowledge_free`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_knowledge(squad: *mut SxSquad) -> *mut SxKnowledge {
    // SAFETY: see above.
    match unsafe { squad.as_mut() } {
        // `SxKnowledge` is a transparent wrapper of `Knowledge`.
        Some(s) => std::ptr::from_mut(s.squad.knowledge_mut()).cast(),
        None => std::ptr::null_mut(),
    }
}

/// `sx_squad_areas`: the leader's danger areas, owned by the squad.
///
/// Aliases the squad as [`sx_squad_knowledge`] does; never pass it to
/// `sx_areas_free`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_areas(squad: *mut SxSquad) -> *mut SxAreas {
    // SAFETY: see above.
    match unsafe { squad.as_mut() } {
        // `SxAreas` is a transparent wrapper of `Areas`.
        Some(s) => std::ptr::from_mut(s.squad.areas_mut()).cast(),
        None => std::ptr::null_mut(),
    }
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
    let s = &mut squad!(squad);
    match s.observe(now, *input!(fact)) {
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
    let s = &mut squad!(squad);
    i32::from(s.report(*input!(msg), arrive_at))
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
    match squad!(squad).report_arrives_at(id, now) {
        Some(t) => {
            // SAFETY: null or writable (see above).
            unsafe { put(at, t) };
            1
        }
        None => 0,
    }
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
    let shape = match shape {
        SHAPE_LINE => Shape::Line,
        SHAPE_WEDGE => Shape::Wedge,
        SHAPE_COLUMN => Shape::Column,
        _ => return ERROR_ARGUMENT,
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
    match sides {
        2 | 3 => to_i32(s.add_tactic(tactics::flank(sides as u8, dist))),
        _ => ERROR_ARGUMENT,
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
    let (t, i) = match e {
        PlanError::InputOutOfRange { tactic, input } => (tactic, input),
        PlanError::TooManySlots { tactic } => (tactic, u32::MAX),
    };
    // SAFETY: see above.
    unsafe {
        put(tactic, u32::try_from(t).unwrap_or(u32::MAX));
        put(input, i);
    }
    ERROR_PLAN
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

/// `sx_squad_plan`.
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
    let s = &mut squad!(squad);
    // SAFETY: see above.
    let Some((extra, spots)) = (unsafe { lists(extra, extra_count, spots, spot_count) }) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| match s.plan(now, extra, spots, seed) {
        Ok(Some(id)) => {
            // SAFETY: see above.
            unsafe { put(plan, id) };
            1
        }
        Ok(None) => 0,
        // SAFETY: null details.
        Err(e) => unsafe { plan_error(e, std::ptr::null_mut(), std::ptr::null_mut()) },
    })
}

/// `sx_squad_check`.
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
    let s = &mut squad!(squad);
    // SAFETY: see above.
    let Some((extra, spots)) = (unsafe { lists(extra, extra_count, spots, spot_count) }) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| match s.check(now, extra, spots, seed) {
        Ok(Status::Keep) => SQUAD_KEEP,
        Ok(Status::Replan(id)) => {
            // SAFETY: see above.
            unsafe { put(plan, id) };
            SQUAD_REPLAN
        }
        Ok(Status::Signal) => SQUAD_SIGNAL,
        Ok(Status::NoPlan) => SQUAD_NO_PLAN,
        // SAFETY: null details.
        Err(e) => unsafe { plan_error(e, std::ptr::null_mut(), std::ptr::null_mut()) },
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
    s.squad.orders_due(now, &mut s.due);
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
    i32::from(squad!(squad).signal(now))
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

/// `sx_keeper`.
pub struct SxKeeper(pub(crate) Held<GoalKeeper>);

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
    create(|| Some(SxKeeper(Held::Own(GoalKeeper::new()))))
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

/// `sx_keeper_goal_postponed` (old name `sx_keeper_postpone`).
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_goal_postponed(keeper: *mut SxKeeper, now: u32) -> i32 {
    keeper!(keeper).goal_postponed(now);
    0
}

/// `sx_keeper_goal_won` (old name `sx_keeper_resume`).
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
    i32::from(keeper!(keeper).broken(now, give_up_ticks))
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
    let Some(metric) = metric_of(metric) else {
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
