//! `sx_squad_*`, `sx_tactic_*`, `sx_keeper_*`, `sx_gather`: group tactics,
//! see `sagax.h`.

use sagax_decide::group::{
    Abort, Channel, Goal, GoalKeeper, GoalKind, Member, MemberState, Nearby, Order, Place, Plan,
    PlanError, Shape, Side, SkillTable, Slot, Spot, Squad, Start, Status, Tactic, Trigger,
    discipline, gather, tactics,
};
use sagax_decide::{Consideration, Curve, Pick};

use super::decide::SxConsideration;
use super::memory::{SxArea, SxAreas, SxFact, SxKnowledge, SxMessage, metric_of, put};
use super::{ERROR_ARGUMENT, ERROR_PLAN, create, destroy, guarded, slice_of, to_i32};

// Numbers of the C interface (`SX_*` in `sagax.h`).

pub(crate) const CHANNEL_NONE: u32 = 0;
pub(crate) const CHANNEL_SHOUT: u32 = 1;
pub(crate) const CHANNEL_RADIO: u32 = 2;

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

pub(crate) const TRIGGER_NONE: u32 = 0;
pub(crate) const TRIGGER_ALL_ARRIVED: u32 = 1;
pub(crate) const TRIGGER_TARGET_IN_RADIUS: u32 = 2;
pub(crate) const TRIGGER_MANUAL: u32 = 3;

pub(crate) const TACTIC_RUSH: u32 = 0;
pub(crate) const TACTIC_LINE: u32 = 1;
pub(crate) const TACTIC_WEDGE: u32 = 2;
pub(crate) const TACTIC_COLUMN: u32 = 3;
pub(crate) const TACTIC_FLANK: u32 = 4;
pub(crate) const TACTIC_FLANK_FRONT: u32 = 5;
pub(crate) const TACTIC_AMBUSH: u32 = 6;
pub(crate) const TACTIC_FOCUS: u32 = 7;
pub(crate) const TACTIC_ISOLATE: u32 = 8;
pub(crate) const TACTIC_FALL_BACK: u32 = 9;

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

    fn member(&self) -> Option<Member> {
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
    /// Distance from the target for sides and between.
    pub dist: i32,
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
    fn of(g: &Goal) -> Self {
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
    metric: i32,
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

/// `sx_tactic_set_trigger`.
///
/// # Safety
/// `tactic` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_tactic_set_trigger(
    tactic: *mut SxTactic,
    kind: u32,
    value: i64,
) -> i32 {
    let t = get!(tactic);
    t.0.trigger = match kind {
        TRIGGER_NONE => Trigger::None,
        TRIGGER_ALL_ARRIVED => match u32::try_from(value) {
            Ok(max_wait) => Trigger::AllArrived { max_wait },
            Err(_) => return ERROR_ARGUMENT,
        },
        TRIGGER_TARGET_IN_RADIUS => Trigger::TargetInRadius(value),
        TRIGGER_MANUAL => Trigger::Manual,
        _ => return ERROR_ARGUMENT,
    };
    0
}

fn shipped(which: u32, param: i32) -> Option<Tactic> {
    Some(match which {
        TACTIC_RUSH => tactics::rush(),
        TACTIC_LINE => tactics::formation(Shape::Line, param),
        TACTIC_WEDGE => tactics::formation(Shape::Wedge, param),
        TACTIC_COLUMN => tactics::formation(Shape::Column, param),
        TACTIC_FLANK => tactics::flank(2, param),
        TACTIC_FLANK_FRONT => tactics::flank(3, param),
        TACTIC_AMBUSH => tactics::ambush(),
        TACTIC_FOCUS => tactics::focus(),
        TACTIC_ISOLATE => tactics::isolate(),
        TACTIC_FALL_BACK => tactics::fall_back(),
        _ => return None,
    })
}

// ---- squad --------------------------------------------------------------------

/// `sx_squad`.
pub struct SxSquad {
    squad: Squad,
    /// Orders taken from the squad and not yet handed out (beyond a cap).
    due: Vec<Order>,
}

/// `sx_squad_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_squad_new(
    group_cap: u32,
    memory_cap: u32,
    area_cap: u32,
    inbox_cap: u32,
    metric: i32,
) -> *mut SxSquad {
    create(|| {
        let squad = Squad::new(
            group_cap as usize,
            memory_cap as usize,
            area_cap as usize,
            inbox_cap as usize,
            metric_of(metric)?,
        )
        .ok()?;
        Some(SxSquad {
            squad,
            due: Vec::new(),
        })
    })
}

/// `sx_squad_free`.
///
/// # Safety
/// `squad` is null or from [`sx_squad_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_free(squad: *mut SxSquad) {
    // SAFETY: see above.
    unsafe { destroy(squad) }
}

/// The squad behind a handle (a place), or `ERROR_ARGUMENT`.
macro_rules! squad {
    ($p:expr) => {
        get!($p).squad
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

/// `sx_squad_resume`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_resume(squad: *mut SxSquad, id: u32) -> i32 {
    i32::from(squad!(squad).group_mut().resume(id))
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

/// `sx_discipline`.
///
/// # Safety
/// `table` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_discipline(skill: u32, table: *const SxSkillTable) -> i32 {
    let Ok(skill) = u8::try_from(skill) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: see above.
    let table = match unsafe { table.as_ref() } {
        None => SkillTable::default(),
        Some(t) => match t.table() {
            Some(t) => t,
            None => return ERROR_ARGUMENT,
        },
    };
    to_i32(discipline(skill, &table))
}

/// `sx_squad_set_channel`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_set_channel(
    squad: *mut SxSquad,
    id: u32,
    kind: u32,
    a: i64,
    b: u32,
) -> i32 {
    let s = &mut squad!(squad);
    let channel = match kind {
        CHANNEL_NONE => Channel::None,
        CHANNEL_SHOUT => Channel::Shout { range: a },
        CHANNEL_RADIO => match u32::try_from(a) {
            Ok(base) => Channel::Radio { base, per_unit: b },
            Err(_) => return ERROR_ARGUMENT,
        },
        _ => return ERROR_ARGUMENT,
    };
    i32::from(s.set_channel(id, channel))
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
    squad!(squad).set_extra_columns(count as usize);
    0
}

/// `sx_squad_knowledge`: the leader's knowledge, owned by the squad.
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
    fact: *const SxFact,
    now: u32,
    evicted: *mut SxFact,
) -> i32 {
    let s = &mut squad!(squad);
    match s.observe(*input!(fact), now) {
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

/// `sx_squad_targets`.
///
/// # Safety
/// `squad` is null or valid; `out` is null or has room for `cap` facts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_targets(squad: *mut SxSquad, out: *mut SxFact, cap: u32) -> i32 {
    let s = &mut squad!(squad);
    if out.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    let mut total = 0;
    for (i, f) in s.targets().enumerate() {
        if i < cap as usize {
            // SAFETY: within `cap` (see above).
            unsafe { out.add(i).write(*f) };
        }
        total += 1;
    }
    to_i32(total)
}

/// `sx_squad_add_shipped`.
///
/// # Safety
/// `squad` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_squad_add_shipped(squad: *mut SxSquad, which: u32, param: i32) -> i32 {
    let s = &mut squad!(squad);
    match shipped(which, param) {
        Some(t) => to_i32(s.add_tactic(t)),
        None => ERROR_ARGUMENT,
    }
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
unsafe fn plan_error(e: PlanError, tactic: *mut u32, input: *mut u32) -> i32 {
    let PlanError::InputOutOfRange {
        tactic: t,
        input: i,
    } = e;
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
unsafe fn lists<'a>(
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
    if out.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    s.squad.orders_due(now, &mut s.due);
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
pub struct SxKeeper(GoalKeeper);

/// `sx_keeper_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_keeper_new() -> *mut SxKeeper {
    create(|| Some(SxKeeper(GoalKeeper::new())))
}

/// `sx_keeper_free`.
///
/// # Safety
/// `keeper` is null or from [`sx_keeper_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_free(keeper: *mut SxKeeper) {
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
    goal: *const SxGoal,
    now: u32,
) -> i32 {
    let k = get!(keeper);
    let Some(goal) = input!(goal).goal() else {
        return ERROR_ARGUMENT;
    };
    k.0.set(goal, now);
    0
}

/// `sx_keeper_goal`.
///
/// # Safety
/// `keeper` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_goal(keeper: *mut SxKeeper, out: *mut SxGoal) -> i32 {
    let k = get!(keeper);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match k.0.goal() {
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
    get!(keeper).0.clear();
    0
}

/// `sx_keeper_postpone`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_postpone(keeper: *mut SxKeeper, now: u32) -> i32 {
    get!(keeper).0.postpone(now);
    0
}

/// `sx_keeper_resume`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_resume(keeper: *mut SxKeeper, now: u32) -> i32 {
    get!(keeper).0.resume(now);
    0
}

/// `sx_keeper_suspend`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_suspend(keeper: *mut SxKeeper) -> i32 {
    get!(keeper).0.suspend();
    0
}

/// `sx_keeper_unsuspend`.
///
/// # Safety
/// `keeper` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_keeper_unsuspend(keeper: *mut SxKeeper, now: u32) -> i32 {
    get!(keeper).0.unsuspend(now);
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
    i32::from(get!(keeper).0.broken(now, give_up_ticks))
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
    metric: i32,
    signal_given: u32,
    discipline: u32,
    out: *mut i32,
) -> i32 {
    let k = get!(keeper);
    let Some(metric) = metric_of(metric) else {
        return ERROR_ARGUMENT;
    };
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    let inputs =
        k.0.inputs(now, [x, y, z], metric, signal_given != 0, discipline);
    for (i, v) in inputs.iter().enumerate() {
        // SAFETY: room for 5 (see above).
        unsafe { out.add(i).write(*v) };
    }
    0
}
