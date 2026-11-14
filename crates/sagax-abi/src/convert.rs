//! Checked conversions between the numbers of [`crate::codes`] and Rust
//! values. A function that reads a code returns `None` for a code it does
//! not know; the layers turn that into `SX_ERROR_ARGUMENT`.

use std::num::NonZeroUsize;

use sagax_core::{HexLayout, Pos};
use sagax_decide::group::{
    Compare, CrewEvent, Goal, GoalKind, MemberState, Place, PlanError, PlanId, RoleCounts, Shape,
    Side, SkillTable, Start, Status as SquadStatus,
};
use sagax_decide::memory::Metric;
use sagax_decide::{Combine, Decider, Pick};
use sagax_nav::nav::{Budget, Output, Status};

use crate::codes::{
    CMP_AT_LEAST, CMP_AT_MOST, CMP_EQUAL, CMP_NOT_EQUAL, COMBINE_FOLLOW, CREW_ACTIVATED,
    CREW_ARRIVED, CREW_BROKEN, CREW_NO_PLAN, CREW_ORDER, CREW_PLANNED, CREW_SIGNAL, CREW_START,
    DONE, ERROR_PLAN, FOUND, GOAL_ATTACK, GOAL_CUSTOM, GOAL_HOLD, GOAL_MOVE, GOAL_RETREAT,
    GOAL_WAIT, GRID_HEX_EVEN_Q, GRID_HEX_EVEN_R, GRID_HEX_ODD_Q, GRID_HEX_ODD_R, IDLE,
    MEMBER_ACTIVE, MEMBER_GONE, MEMBER_SUSPENDED, METRIC_CHEBYSHEV, METRIC_EUCLID, METRIC_HEX,
    METRIC_MANHATTAN, PARTIAL, PICK_BEST, PICK_UNIFORM, PICK_WEIGHTED, PLACE_BETWEEN, PLACE_HERE,
    PLACE_NONE, PLACE_OFFSET, PLACE_SIDE, PLACE_TAGGED, RUNNING, SHAPE_COLUMN, SHAPE_LINE,
    SHAPE_WEDGE, SIDE_FRONT, SIDE_LEFT, SIDE_REAR, SIDE_RIGHT, SQUAD_KEEP, SQUAD_NO_PLAN,
    SQUAD_REPLAN, SQUAD_SIGNAL, START_AFTER_TICKS, START_NOW, START_ON_SIGNAL, UNREACHABLE,
};

// ---- numbers ------------------------------------------------------------------

/// `value` as `i32`, `i32::MAX` if it does not fit.
pub fn to_i32<T: TryInto<i32>>(value: T) -> i32 {
    value.try_into().unwrap_or(i32::MAX)
}

/// `value` as `u32`, `None` if it is negative.
pub fn u32_of(value: i32) -> Option<u32> {
    u32::try_from(value).ok()
}

/// `value` as `i64` if it is a finite whole number in range (not NaN, no
/// fraction, not beyond `i64`), else `None`. The one conversion for every
/// number that comes from JavaScript as an `f64`.
pub fn whole_i64(value: f64) -> Option<i64> {
    // 2^63: the first value above `i64::MAX`; `-2^63` itself is exact.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    (value.is_finite() && value.fract() == 0.0 && (-LIMIT..LIMIT).contains(&value))
        .then_some(value as i64)
}

// ---- grid and search ----------------------------------------------------------

/// The hex layout of an `SX_GRID_HEX_*` value; `None` for square or unknown.
pub fn hex_layout(grid: u32) -> Option<HexLayout> {
    match grid {
        GRID_HEX_ODD_R => Some(HexLayout::OddR),
        GRID_HEX_EVEN_R => Some(HexLayout::EvenR),
        GRID_HEX_ODD_Q => Some(HexLayout::OddQ),
        GRID_HEX_EVEN_Q => Some(HexLayout::EvenQ),
        _ => None,
    }
}

/// An offset position of hex grid `grid` as axial; `None` if `grid` is not
/// hex.
pub fn hex_to_axial(grid: u32, p: Pos) -> Option<Pos> {
    hex_layout(grid).map(|layout| layout.to_axial_pos(p))
}

/// An axial position as offset on hex grid `grid`; `None` if `grid` is not
/// hex.
pub fn hex_from_axial(grid: u32, p: Pos) -> Option<Pos> {
    hex_layout(grid).map(|layout| layout.from_axial_pos(p))
}

/// The metric of an `SX_METRIC_*` value.
pub fn metric(code: u32) -> Option<Metric> {
    match code {
        METRIC_EUCLID => Some(Metric::Euclid),
        METRIC_MANHATTAN => Some(Metric::Manhattan),
        METRIC_CHEBYSHEV => Some(Metric::Chebyshev),
        METRIC_HEX => Some(Metric::Hex),
        _ => None,
    }
}

/// The `SX_METRIC_*` value of a metric.
pub const fn metric_code(metric: Metric) -> u32 {
    match metric {
        Metric::Euclid => METRIC_EUCLID,
        Metric::Manhattan => METRIC_MANHATTAN,
        Metric::Chebyshev => METRIC_CHEBYSHEV,
        Metric::Hex => METRIC_HEX,
    }
}

/// A search budget in cells: 0 is unlimited, a negative one is refused.
pub fn budget(cells: i32) -> Option<Budget> {
    match cells {
        0 => Some(Budget::Unlimited),
        c => usize::try_from(c)
            .ok()
            .and_then(NonZeroUsize::new)
            .map(Budget::Cells),
    }
}

/// A budget in cells as a count: 0 is `usize::MAX`, a negative one is
/// refused.
pub fn budget_cells(cells: i32) -> Option<usize> {
    match cells {
        0 => Some(usize::MAX),
        c => usize::try_from(c).ok(),
    }
}

/// The way output of an `SX_OUT_*` value.
pub fn output(code: u32) -> Option<Output> {
    match code {
        0 => Some(Output::Cells),
        1 => Some(Output::Turns),
        2 => Some(Output::Smooth),
        _ => None,
    }
}

/// The `SX_IDLE` .. `SX_DONE` value of a search status.
pub const fn nav_status(status: Status) -> i32 {
    match status {
        Status::Idle => IDLE,
        Status::Running => RUNNING,
        Status::Found => FOUND,
        Status::Partial => PARTIAL,
        Status::Unreachable => UNREACHABLE,
        Status::Done => DONE,
    }
}

// ---- places, goals, members ---------------------------------------------------

/// A side of the target.
pub fn side(code: u32) -> Option<Side> {
    match code {
        SIDE_FRONT => Some(Side::Front),
        SIDE_LEFT => Some(Side::Left),
        SIDE_RIGHT => Some(Side::Right),
        SIDE_REAR => Some(Side::Rear),
        _ => None,
    }
}

/// The place of a record: `kind` is `SX_PLACE_*`, `value` the `SX_SIDE_*`
/// or the tag bits, `offset` the offset of `SX_PLACE_OFFSET`. `Some(None)`
/// for `SX_PLACE_NONE`, `None` if the kind or the side is unknown.
pub fn place(kind: u32, value: u32, offset: Pos) -> Option<Option<Place>> {
    Some(Some(match kind {
        PLACE_NONE => return Some(None),
        PLACE_OFFSET => Place::Offset(offset),
        PLACE_SIDE => Place::Side(side(value)?),
        PLACE_BETWEEN => Place::Between,
        PLACE_TAGGED => Place::Tagged(value),
        PLACE_HERE => Place::Here,
        _ => return None,
    }))
}

/// The record of a place: `(kind, value, offset)`, the inverse of [`place`].
pub const fn place_code(place: Option<Place>) -> (u32, u32, Pos) {
    match place {
        None => (PLACE_NONE, 0, [0; 3]),
        Some(Place::Offset(p)) => (PLACE_OFFSET, 0, p),
        Some(Place::Side(s)) => (PLACE_SIDE, s as u32, [0; 3]),
        Some(Place::Between) => (PLACE_BETWEEN, 0, [0; 3]),
        Some(Place::Tagged(tags)) => (PLACE_TAGGED, tags, [0; 3]),
        Some(Place::Here) => (PLACE_HERE, 0, [0; 3]),
        // No code of its own yet: shown as what it is without rules, the
        // leader's position, with the id as the value.
        Some(Place::Custom(id)) => (PLACE_HERE, id, [0; 3]),
    }
}

/// The goal kind of an `SX_GOAL_*` value; `custom` is the game's own kind
/// of `SX_GOAL_CUSTOM`.
pub fn goal_kind(code: u32, custom: u32) -> Option<GoalKind> {
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

/// `(code, custom)` of a goal kind, the inverse of [`goal_kind`].
pub const fn goal_kind_code(kind: GoalKind) -> (u32, u32) {
    match kind {
        GoalKind::Move => (GOAL_MOVE, 0),
        GoalKind::Hold => (GOAL_HOLD, 0),
        GoalKind::Attack => (GOAL_ATTACK, 0),
        GoalKind::Wait => (GOAL_WAIT, 0),
        GoalKind::Retreat => (GOAL_RETREAT, 0),
        GoalKind::Custom(c) => (GOAL_CUSTOM, c),
    }
}

/// The start of an `SX_START_*` value; `ticks` is for
/// `SX_START_AFTER_TICKS`.
pub fn start(code: u32, ticks: u32) -> Option<Start> {
    match code {
        START_NOW => Some(Start::Now),
        START_AFTER_TICKS => Some(Start::AfterTicks(ticks)),
        START_ON_SIGNAL => Some(Start::OnSignal),
        _ => None,
    }
}

/// `(code, ticks)` of a start, the inverse of [`start`].
pub const fn start_code(start: Start) -> (u32, u32) {
    match start {
        Start::Now => (START_NOW, 0),
        Start::AfterTicks(n) => (START_AFTER_TICKS, n),
        Start::OnSignal => (START_ON_SIGNAL, 0),
    }
}

/// The comparison of an `SX_CMP_*` value.
pub fn compare(code: u32) -> Option<Compare> {
    match code {
        CMP_AT_LEAST => Some(Compare::AtLeast),
        CMP_AT_MOST => Some(Compare::AtMost),
        CMP_EQUAL => Some(Compare::Equal),
        CMP_NOT_EQUAL => Some(Compare::NotEqual),
        _ => None,
    }
}

/// The shape of an `SX_SHAPE_*` value.
pub fn shape(code: u32) -> Option<Shape> {
    match code {
        SHAPE_LINE => Some(Shape::Line),
        SHAPE_WEDGE => Some(Shape::Wedge),
        SHAPE_COLUMN => Some(Shape::Column),
        _ => None,
    }
}

/// The role list of a squad config: the first `count` of `masks`. `None`
/// above 8 entries (`SX_MAX_ROLE_COUNTS`) or when `masks` holds fewer than
/// `count`.
pub fn roles(count: u32, masks: &[u32]) -> Option<RoleCounts> {
    masks
        .get(..usize::try_from(count).ok()?)
        .and_then(RoleCounts::new)
}

/// The number of sides of a flank tactic: 2 or 3, nothing else.
pub fn flank_sides(sides: u32) -> Option<u8> {
    match sides {
        2 => Some(2),
        3 => Some(3),
        _ => None,
    }
}

/// The member state of an `SX_MEMBER_*` value.
pub fn member_state(code: u32) -> Option<MemberState> {
    match code {
        MEMBER_ACTIVE => Some(MemberState::Active),
        MEMBER_SUSPENDED => Some(MemberState::Suspended),
        MEMBER_GONE => Some(MemberState::Gone),
        _ => None,
    }
}

// ---- skill table --------------------------------------------------------------

/// A skill table as numbers: up to eight rows per array, picks as
/// `SX_PICK_*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkillCodes {
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

/// The skill table of its numbers; `None` for `rows` 0 or above 8 or an
/// unknown pick (also in an unused row).
pub fn skill_table(codes: &SkillCodes) -> Option<SkillTable> {
    let rows = u8::try_from(codes.rows)
        .ok()
        .filter(|r| (1..=8).contains(r))?;
    let mut pick = [Pick::Best; 8];
    for (i, p) in pick.iter_mut().enumerate() {
        *p = Pick::from_u32(codes.pick[i], codes.pick_top[i])?;
    }
    Some(SkillTable {
        rows,
        pick,
        think_ticks: codes.think_ticks,
        spot_rank_error: codes.spot_rank_error,
        timing_spread: codes.timing_spread,
        hold_bonus: codes.hold_bonus,
        give_up_ticks: codes.give_up_ticks,
    })
}

/// The numbers of a skill table, the inverse of [`skill_table`].
pub fn skill_codes(table: &SkillTable) -> SkillCodes {
    let pick = table.pick.map(|p| match p {
        Pick::Best => (PICK_BEST, 0),
        Pick::Weighted { top } => (PICK_WEIGHTED, top),
        Pick::Uniform { top } => (PICK_UNIFORM, top),
    });
    SkillCodes {
        rows: u32::from(table.rows),
        pick: pick.map(|p| p.0),
        pick_top: pick.map(|p| p.1),
        think_ticks: table.think_ticks,
        timing_spread: table.timing_spread,
        hold_bonus: table.hold_bonus,
        give_up_ticks: table.give_up_ticks,
        spot_rank_error: table.spot_rank_error,
    }
}

// ---- decider ------------------------------------------------------------------

/// Which options [`set_combine`] changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombineTarget {
    /// Every option (the decider's own combine).
    All,
    /// This option only.
    Option(u32),
}

/// Sets how factors are put together: for [`CombineTarget::All`] the
/// decider's combine (`combine` must be a combine), for one option its own
/// (`combine` a combine or [`COMBINE_FOLLOW`] to follow the decider again).
/// `false` for an unknown combine or an option that does not exist, with
/// nothing changed.
pub fn set_combine(decider: &mut Decider, target: CombineTarget, combine: u32) -> bool {
    let how = Combine::from_u32(combine);
    match target {
        CombineTarget::All => how.is_some_and(|how| {
            decider.set_combine(how);
            true
        }),
        CombineTarget::Option(option) => {
            (how.is_some() || combine == COMBINE_FOLLOW) && decider.set_option_combine(option, how)
        }
    }
}

// ---- squad and crew -----------------------------------------------------------

/// The `SX_SQUAD_*` value of a check result, and the new plan's id for
/// `SX_SQUAD_REPLAN`.
pub const fn squad_status(status: SquadStatus) -> (i32, Option<PlanId>) {
    match status {
        SquadStatus::Keep => (SQUAD_KEEP, None),
        SquadStatus::Replan(id) => (SQUAD_REPLAN, Some(id)),
        SquadStatus::Signal => (SQUAD_SIGNAL, None),
        SquadStatus::NoPlan => (SQUAD_NO_PLAN, None),
    }
}

/// `(SX_ERROR_PLAN, tactic, input)` of a plan error; the input is
/// `u32::MAX` where there is no column; for `OptionsChanged` and
/// `NeedsRules` both are `u32::MAX`.
pub fn plan_error(error: PlanError) -> (i32, u32, u32) {
    let (tactic, input) = match error {
        PlanError::InputOutOfRange { tactic, input } => (tactic, input),
        PlanError::TooManySlots { tactic } => (tactic, u32::MAX),
        PlanError::OptionsChanged => (usize::MAX, u32::MAX),
        PlanError::NeedsRules => (usize::MAX, u32::MAX),
    };
    (ERROR_PLAN, u32::try_from(tactic).unwrap_or(u32::MAX), input)
}

/// A crew event as numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrewEventCode {
    /// `SX_CREW_*`.
    pub kind: u32,
    /// The member's id (order, start, arrived, broken), else 0.
    pub member: u32,
    /// The plan's id (planned, activated, order), else 0.
    pub plan: u32,
    /// The tick an order starts; `None` for an order that waits for the
    /// signal and for every other event.
    pub start_at: Option<u32>,
    /// The order's goal (order only).
    pub goal: Option<Goal>,
}

/// The numbers of a crew event.
pub fn crew_event(event: &CrewEvent) -> CrewEventCode {
    let plain = |kind, member, plan| CrewEventCode {
        kind,
        member,
        plan,
        start_at: None,
        goal: None,
    };
    match *event {
        CrewEvent::Planned(plan) => plain(CREW_PLANNED, 0, plan),
        CrewEvent::Activated(plan) => plain(CREW_ACTIVATED, 0, plan),
        CrewEvent::Signal => plain(CREW_SIGNAL, 0, 0),
        CrewEvent::Order {
            member,
            goal,
            start_at,
        } => CrewEventCode {
            kind: CREW_ORDER,
            member,
            plan: goal.plan,
            start_at,
            goal: Some(goal),
        },
        CrewEvent::Start { member } => plain(CREW_START, member, 0),
        CrewEvent::Arrived { member } => plain(CREW_ARRIVED, member, 0),
        CrewEvent::Broken { member } => plain(CREW_BROKEN, member, 0),
        CrewEvent::NoPlan => plain(CREW_NO_PLAN, 0, 0),
    }
}

#[cfg(test)]
mod tests;
