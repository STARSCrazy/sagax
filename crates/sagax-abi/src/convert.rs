//! Checked conversions between the numbers of [`crate::codes`] and Rust
//! values. A function that reads a code returns `None` for a code it does
//! not know; the layers turn that into `SX_ERROR_ARGUMENT`.

use std::num::NonZeroUsize;

use sagax_core::{HexLayout, Pos};
use sagax_decide::group::{
    Compare, CrewEvent, CrewTrace, Goal, GoalKind, KeeperTrace, MemberState, Place, PlanError,
    PlanId, RoleCounts, Shape, Side, SkillTable, SpotPick, SpotTrace, SquadTrace, Start,
    Status as SquadStatus,
};
use sagax_decide::memory::{
    Area, AreasTrace, Fact, KnowledgeTrace, Message, MessageTrace, Metric, Refused, TimersTrace,
};
use sagax_decide::mission::{
    Action, BreakReason, Detour, DetourEnd, Fail, Leeway, MissionEvent, MissionTrace, Motive,
    MotiveKind, OnLost, StepKind, Where,
};
use sagax_decide::state::{Change, Reason, StateTrace};
use sagax_decide::{Combine, DecideTrace, Decider, Pick};
use sagax_nav::Cost;
use sagax_nav::formation::FormationTrace;
use sagax_nav::grid::Cell;
use sagax_nav::nav::{Budget, NavTrace, Output, Status};
use sagax_nav::search::{SearchStatus, SearchTrace};
use sagax_nav::smooth::SmoothTrace;

use crate::codes::{
    ACTION_ATTACK, ACTION_CUSTOM, ACTION_FLEE, ACTION_FOLLOW, ACTION_GO_TO, ACTION_HOLD,
    ACTION_NONE, ACTION_SEARCH, ACTION_WAIT, ACTION_WANDER, BREAK_FAIL, BREAK_IMPOSSIBLE,
    BREAK_PLAN_ENDED, DETOUR_CUSTOM, DETOUR_END_ARRIVED, DETOUR_END_DEFEATED, DETOUR_END_ENDED,
    DETOUR_END_FORGOTTEN, DETOUR_END_LEASH, DETOUR_END_LOST, DETOUR_END_SAFE, DETOUR_ENGAGE,
    DETOUR_FLEE, FAIL_CUSTOM, FAIL_DEFEATED, FAIL_FACT, FAIL_FLAGS, FAIL_UNKNOWN, GRID_SQUARE,
    LEEWAY_FLEXIBLE, LEEWAY_FREE, LEEWAY_STRICT, MISSION_EVENT_BROKEN, MISSION_EVENT_CALL,
    MISSION_EVENT_DETOUR_ENDED, MISSION_EVENT_DETOUR_STARTED, MISSION_EVENT_FELL_BACK,
    MISSION_EVENT_LOOPED, MISSION_EVENT_PLAN_DONE, MISSION_EVENT_REPLANNED, MISSION_EVENT_RETURNED,
    MISSION_EVENT_SIGNAL, MISSION_EVENT_STEP_DONE, MISSION_EVENT_WANTS_SQUAD, MOTIVE_CUSTOM,
    MOTIVE_DEFEAT, MOTIVE_GUARD, MOTIVE_REACH, ON_LOST_KEEP, ON_LOST_RETURN, ON_LOST_SEARCH,
    STEP_ATTACK, STEP_CALL, STEP_CUSTOM, STEP_FOLLOW, STEP_GO_TO, STEP_HOLD, STEP_PAUSE,
    STEP_SEARCH, STEP_SIGNAL, STEP_SQUAD, STEP_WAIT, STEP_WAIT_FOR, STEP_WANDER, WHERE_FIXED,
    WHERE_HERE, WHERE_MOTIVE_PLACE, WHERE_MOTIVE_SUBJECT, WHERE_SUBJECT,
};
use crate::codes::{
    AREAS_TRACE_ADDED, AREAS_TRACE_EVICTED, AREAS_TRACE_EXPIRED, AREAS_TRACE_RENEWED,
    KNOWLEDGE_TRACE_ADDED, KNOWLEDGE_TRACE_EVICTED, KNOWLEDGE_TRACE_EXPIRED,
    KNOWLEDGE_TRACE_REMOVED, KNOWLEDGE_TRACE_STALE, KNOWLEDGE_TRACE_UPDATED,
    MESSAGE_TRACE_DELIVERED, MESSAGE_TRACE_DROPPED, MESSAGE_TRACE_POSTED, MESSAGE_TRACE_QUEUED,
    MESSAGE_TRACE_REFUSED, REFUSED_COOLDOWN, REFUSED_FULL, TIMERS_TRACE_RENEWED,
    TIMERS_TRACE_REPLACED, TIMERS_TRACE_STARTED,
};
use crate::codes::{
    CMP_AT_LEAST, CMP_AT_MOST, CMP_EQUAL, CMP_NOT_EQUAL, COMBINE_FOLLOW, CREW_ACTIVATED,
    CREW_ARRIVED, CREW_BROKEN, CREW_NO_PLAN, CREW_ORDER, CREW_PLANNED, CREW_SIGNAL, CREW_START,
    DONE, ERROR_NEEDS_RULES, ERROR_PLAN, FOUND, GOAL_ATTACK, GOAL_CUSTOM, GOAL_HOLD, GOAL_MOVE,
    GOAL_RETREAT, GOAL_WAIT, GRID_HEX_EVEN_Q, GRID_HEX_EVEN_R, GRID_HEX_ODD_Q, GRID_HEX_ODD_R,
    IDLE, MEMBER_ACTIVE, MEMBER_GONE, MEMBER_SUSPENDED, METRIC_CHEBYSHEV, METRIC_EUCLID,
    METRIC_HEX, METRIC_MANHATTAN, PARTIAL, PICK_BEST, PICK_UNIFORM, PICK_WEIGHTED, PLACE_BETWEEN,
    PLACE_CUSTOM, PLACE_HERE, PLACE_NONE, PLACE_OFFSET, PLACE_SIDE, PLACE_TAGGED, RUNNING,
    SHAPE_COLUMN, SHAPE_LINE, SHAPE_WEDGE, SIDE_FRONT, SIDE_LEFT, SIDE_REAR, SIDE_RIGHT,
    SQUAD_KEEP, SQUAD_NO_PLAN, SQUAD_REPLAN, SQUAD_SIGNAL, START_AFTER_TICKS, START_NOW,
    START_ON_SIGNAL, UNREACHABLE,
};
use crate::codes::{
    CREW_TRACE_KEEPER, CREW_TRACE_SENT, CREW_TRACE_SQUAD, KEEPER_TRACE_CHECKED, SQUAD_TRACE_ABORT,
    SQUAD_TRACE_AREAS, SQUAD_TRACE_ASSIGNED, SQUAD_TRACE_DECIDE, SQUAD_TRACE_DELAY,
    SQUAD_TRACE_INBOX, SQUAD_TRACE_KNOWLEDGE, SQUAD_TRACE_NO_PICK, SQUAD_TRACE_PICKED,
    SQUAD_TRACE_REPORT, SQUAD_TRACE_SPOT_PICKED, SQUAD_TRACE_SPOT_RANKED, SQUAD_TRACE_TARGET,
    SQUAD_TRACE_THINKING, SQUAD_TRACE_TRIGGER,
};
use crate::codes::{
    DECIDE_TRACE_CHOSEN, DECIDE_TRACE_FACTOR, DECIDE_TRACE_RANKED, DECIDE_TRACE_SCORED,
    MACHINE_TRACE_CHANGED, MACHINE_TRACE_WAITING, MISSION_TRACE_BEGIN, MISSION_TRACE_COLUMN,
    MISSION_TRACE_DECIDE, MISSION_TRACE_PICKED, MISSION_TRACE_RESCORED, REASON_SET, REASON_TIMEOUT,
    REASON_TRANSITION,
};
use crate::codes::{
    NAV_TRACE_FLEE_BEST, NAV_TRACE_PLACED, NAV_TRACE_SEARCH, NAV_TRACE_SHORTCUT,
    NAV_TRACE_UNPLACED, SEARCH_TRACE_EXPANDED, SEARCH_TRACE_FINISHED, SEARCH_TRACE_IMPROVED,
    SEARCH_TRACE_PAUSED, SEARCH_TRACE_STARTED,
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

// ---- traces of the navigation -------------------------------------------------

/// The numbers of a search trace record (`sx_search_trace`). Fields that do
/// not apply to `kind` are 0 (cells `(0, 0, 0)`); costs saturate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchTraceCode {
    /// `SX_SEARCH_TRACE_*`.
    pub kind: u32,
    /// Number of the search within its search object.
    pub search: u32,
    /// `STARTED`: the start; `EXPANDED`, `IMPROVED`: the cell.
    pub cell: Cell,
    /// `IMPROVED`: where the cell is reached from.
    pub parent: Cell,
    /// `EXPANDED`, `IMPROVED`: the cost.
    pub cost: i32,
    /// `STARTED`, `EXPANDED`: the estimate still to go.
    pub estimate: i32,
    /// `PAUSED`, `FINISHED`: cells expanded so far.
    pub expanded: i32,
    /// `FINISHED`: a goal was reached.
    pub found: bool,
    /// `IMPROVED`: the cell was new.
    pub first: bool,
}

/// The numbers of a search trace record.
pub fn search_trace(event: SearchTrace<Cell>) -> SearchTraceCode {
    let of = |kind, search| SearchTraceCode {
        kind,
        search,
        ..SearchTraceCode::default()
    };
    match event {
        SearchTrace::Started {
            search,
            start,
            estimate,
        } => SearchTraceCode {
            cell: start,
            estimate: to_i32(estimate),
            ..of(SEARCH_TRACE_STARTED, search)
        },
        SearchTrace::Expanded {
            search,
            node,
            cost,
            estimate,
        } => SearchTraceCode {
            cell: node,
            cost: to_i32(cost),
            estimate: to_i32(estimate),
            ..of(SEARCH_TRACE_EXPANDED, search)
        },
        SearchTrace::Improved {
            search,
            node,
            parent,
            cost,
            first,
        } => SearchTraceCode {
            cell: node,
            parent,
            cost: to_i32(cost),
            first,
            ..of(SEARCH_TRACE_IMPROVED, search)
        },
        SearchTrace::Paused { search, expanded } => SearchTraceCode {
            expanded: to_i32(expanded),
            ..of(SEARCH_TRACE_PAUSED, search)
        },
        SearchTrace::Finished {
            search,
            status,
            expanded,
        } => SearchTraceCode {
            expanded: to_i32(expanded),
            found: status == SearchStatus::Found,
            ..of(SEARCH_TRACE_FINISHED, search)
        },
    }
}

/// The numbers of a navigator trace record (`sx_nav_trace`). Fields that do
/// not apply to `kind` are 0; costs saturate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NavTraceCode {
    /// `SX_NAV_TRACE_*`.
    pub kind: u32,
    /// `SEARCH`: the search record.
    pub search: SearchTraceCode,
    /// `SHORTCUT`: where the tested line starts.
    pub from: Cell,
    /// `SHORTCUT`: where it ends.
    pub to: Cell,
    /// `PLACED`: the member's target; `FLEE_BEST`: the chosen cell.
    pub cell: Cell,
    /// `PLACED`, `UNPLACED`: the member.
    pub member: i32,
    /// `PLACED`, `UNPLACED`: the step of the leader's way.
    pub step: i32,
    /// `FLEE_BEST`: the threats' walk to the cell (`i32::MAX`: none
    /// reaches it).
    pub distance: i32,
    /// `FLEE_BEST`: the walker's cost to the cell.
    pub cost: i32,
    /// `SHORTCUT`: the line was free.
    pub clear: bool,
    /// `PLACED`: the target is the member's slot.
    pub slot: bool,
}

/// The numbers of a navigator trace record.
pub fn nav_trace(event: NavTrace) -> NavTraceCode {
    let of = |kind| NavTraceCode {
        kind,
        ..NavTraceCode::default()
    };
    match event {
        NavTrace::Search(e) => NavTraceCode {
            search: search_trace(e),
            ..of(NAV_TRACE_SEARCH)
        },
        NavTrace::Smooth(SmoothTrace::Shortcut { from, to, clear }) => NavTraceCode {
            from,
            to,
            clear,
            ..of(NAV_TRACE_SHORTCUT)
        },
        NavTrace::Formation(FormationTrace::Placed {
            member,
            step,
            cell,
            slot,
        }) => NavTraceCode {
            cell,
            member: to_i32(member),
            step: to_i32(step),
            slot,
            ..of(NAV_TRACE_PLACED)
        },
        NavTrace::Formation(FormationTrace::Unplaced { member, step }) => NavTraceCode {
            member: to_i32(member),
            step: to_i32(step),
            ..of(NAV_TRACE_UNPLACED)
        },
        NavTrace::FleeBest {
            cell,
            distance,
            cost,
        } => NavTraceCode {
            cell,
            distance: to_i32::<Cost>(distance),
            cost: to_i32(cost),
            ..of(NAV_TRACE_FLEE_BEST)
        },
    }
}

// ---- traces of decisions and the state machine --------------------------------

/// The numbers of a decision trace record (`sx_decide_trace`). Fields that
/// do not apply to `kind` are 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecideTraceCode {
    /// `SX_DECIDE_TRACE_*`.
    pub kind: u32,
    /// `FACTOR`, `SCORED`, `CHOSEN`: index of the candidate.
    pub candidate: i32,
    /// `FACTOR`: index of the consideration within its option.
    pub consideration: i32,
    /// `FACTOR`: the raw input.
    pub input: i32,
    /// `FACTOR`: the factor after its pull.
    pub value: u32,
    /// `SCORED`, `CHOSEN`: the score.
    pub score: u32,
    /// `SCORED`: the factors put together, before the weight.
    pub product: u32,
    /// `RANKED`: the rank that takes part.
    pub rank: u32,
    /// `SCORED`: a cooldown forced the score to 0.
    pub cooled: bool,
}

/// The numbers of a decision trace record.
pub fn decide_trace(event: DecideTrace) -> DecideTraceCode {
    let of = |kind, candidate| DecideTraceCode {
        kind,
        candidate: to_i32(candidate),
        ..DecideTraceCode::default()
    };
    match event {
        DecideTrace::Factor {
            candidate,
            consideration,
            input,
            value,
        } => DecideTraceCode {
            consideration: to_i32(consideration),
            input,
            value,
            ..of(DECIDE_TRACE_FACTOR, candidate)
        },
        DecideTrace::Scored {
            candidate,
            product,
            score,
            cooled,
        } => DecideTraceCode {
            product,
            score,
            cooled,
            ..of(DECIDE_TRACE_SCORED, candidate)
        },
        DecideTrace::Ranked { rank } => DecideTraceCode {
            rank,
            ..of(DECIDE_TRACE_RANKED, 0)
        },
        DecideTrace::Chosen { candidate, score } => DecideTraceCode {
            score,
            ..of(DECIDE_TRACE_CHOSEN, candidate)
        },
    }
}

/// The numbers of a state change (`sx_change`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChangeCode {
    /// The state left.
    pub from: i32,
    /// The state entered.
    pub to: i32,
    /// `SX_REASON_*`.
    pub reason: i32,
    /// The transition's index, or -1.
    pub transition: i32,
}

/// The numbers of a state change.
pub fn change(c: Change) -> ChangeCode {
    let (reason, transition) = match c.reason {
        Reason::Transition(i) => (REASON_TRANSITION, to_i32(i)),
        Reason::Timeout => (REASON_TIMEOUT, -1),
        Reason::Set => (REASON_SET, -1),
    };
    ChangeCode {
        from: to_i32(c.from),
        to: to_i32(c.to),
        reason,
        transition,
    }
}

/// The numbers of a state machine trace record (`sx_machine_trace`).
/// Fields that do not apply to `kind` are 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateTraceCode {
    /// `SX_MACHINE_TRACE_*`.
    pub kind: u32,
    /// `CHANGED`: the change.
    pub change: ChangeCode,
    /// `WAITING`: the transition's index.
    pub transition: u32,
    /// `WAITING`: how long the state has run.
    pub elapsed: u32,
    /// `WAITING`: how long it must run first.
    pub after: u32,
}

/// The numbers of a state machine trace record.
pub fn state_trace(event: StateTrace) -> StateTraceCode {
    match event {
        StateTrace::Changed(c) => StateTraceCode {
            kind: MACHINE_TRACE_CHANGED,
            change: change(c),
            ..StateTraceCode::default()
        },
        StateTrace::Waiting {
            transition,
            elapsed,
            after,
        } => StateTraceCode {
            kind: MACHINE_TRACE_WAITING,
            transition: u32::try_from(transition).unwrap_or(u32::MAX),
            elapsed,
            after,
            ..StateTraceCode::default()
        },
    }
}

// ---- traces of the world memory -----------------------------------------------

/// Why an outbox refused a post (`SX_REFUSED_*`).
pub fn refused(why: Refused) -> u32 {
    match why {
        Refused::Full => REFUSED_FULL,
        Refused::Cooldown => REFUSED_COOLDOWN,
    }
}

/// The numbers of a knowledge trace record (`sx_knowledge_trace`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KnowledgeTraceCode {
    /// `SX_KNOWLEDGE_TRACE_*`.
    pub kind: u32,
    /// The fact concerned.
    pub fact: Fact,
}

/// The numbers of a knowledge trace record.
pub fn knowledge_trace(event: KnowledgeTrace) -> KnowledgeTraceCode {
    let (kind, fact) = match event {
        KnowledgeTrace::Added(f) => (KNOWLEDGE_TRACE_ADDED, f),
        KnowledgeTrace::Updated(f) => (KNOWLEDGE_TRACE_UPDATED, f),
        KnowledgeTrace::Stale(f) => (KNOWLEDGE_TRACE_STALE, f),
        KnowledgeTrace::Evicted(f) => (KNOWLEDGE_TRACE_EVICTED, f),
        KnowledgeTrace::Expired(f) => (KNOWLEDGE_TRACE_EXPIRED, f),
        KnowledgeTrace::Removed(f) => (KNOWLEDGE_TRACE_REMOVED, f),
    };
    KnowledgeTraceCode { kind, fact }
}

/// The numbers of an areas trace record (`sx_areas_trace`). `index` is 0
/// for `EVICTED` and `EXPIRED`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AreasTraceCode {
    /// `SX_AREAS_TRACE_*`.
    pub kind: u32,
    /// `ADDED`, `RENEWED`: the slot.
    pub index: u32,
    /// The area concerned.
    pub area: Area,
}

/// The numbers of an areas trace record.
pub fn areas_trace(event: AreasTrace) -> AreasTraceCode {
    let slot = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    let (kind, index, area) = match event {
        AreasTrace::Added { index, area } => (AREAS_TRACE_ADDED, slot(index), area),
        AreasTrace::Renewed { index, area } => (AREAS_TRACE_RENEWED, slot(index), area),
        AreasTrace::Evicted(area) => (AREAS_TRACE_EVICTED, 0, area),
        AreasTrace::Expired(area) => (AREAS_TRACE_EXPIRED, 0, area),
    };
    AreasTraceCode { kind, index, area }
}

/// The numbers of a timers trace record (`sx_timers_trace`). Fields that
/// do not apply to `kind` are 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimersTraceCode {
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

/// The numbers of a timers trace record.
pub fn timers_trace(event: TimersTrace) -> TimersTraceCode {
    match event {
        TimersTrace::Started { id, duration } => TimersTraceCode {
            kind: TIMERS_TRACE_STARTED,
            id,
            duration,
            ..TimersTraceCode::default()
        },
        TimersTrace::Renewed { id, duration } => TimersTraceCode {
            kind: TIMERS_TRACE_RENEWED,
            id,
            duration,
            ..TimersTraceCode::default()
        },
        TimersTrace::Replaced {
            id,
            duration,
            old,
            old_left,
        } => TimersTraceCode {
            kind: TIMERS_TRACE_REPLACED,
            id,
            duration,
            old,
            old_left,
        },
    }
}

/// The numbers of a message trace record (`sx_message_trace`). Fields that
/// do not apply to `kind` are 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageTraceCode {
    /// `SX_MESSAGE_TRACE_*`.
    pub kind: u32,
    /// The message concerned.
    pub msg: Message,
    /// `POSTED`: when it becomes due; `QUEUED`: when it arrives.
    pub at: u32,
    /// `REFUSED`: `SX_REFUSED_*`.
    pub refused: u32,
}

/// The numbers of a message trace record.
pub fn message_trace(event: MessageTrace) -> MessageTraceCode {
    let code = |kind, msg| MessageTraceCode {
        kind,
        msg,
        ..MessageTraceCode::default()
    };
    match event {
        MessageTrace::Posted { msg, due_at } => MessageTraceCode {
            at: due_at,
            ..code(MESSAGE_TRACE_POSTED, msg)
        },
        MessageTrace::Refused { msg, why } => MessageTraceCode {
            refused: refused(why),
            ..code(MESSAGE_TRACE_REFUSED, msg)
        },
        MessageTrace::Queued { msg, arrive_at } => MessageTraceCode {
            at: arrive_at,
            ..code(MESSAGE_TRACE_QUEUED, msg)
        },
        MessageTrace::Dropped { msg } => code(MESSAGE_TRACE_DROPPED, msg),
        MessageTrace::Delivered { msg } => code(MESSAGE_TRACE_DELIVERED, msg),
    }
}

// ---- traces of the group ------------------------------------------------------

/// `v` as `i64`, saturated.
fn sat64(v: i128) -> i64 {
    v.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// `v` as `u32`, `u32::MAX` if it does not fit.
fn index_u32(v: usize) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// The numbers of a squad trace record (`sx_squad_trace`). Fields that do
/// not apply to `kind` are 0 (`None`, `false`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SquadTraceCode {
    /// `SX_SQUAD_TRACE_*`.
    pub kind: u32,
    /// `DECIDE`, `ABORT`, `PICKED`: the tactic.
    pub tactic: u32,
    /// `DECIDE`, `ABORT`, `PICKED`: the target, if any.
    pub target: Option<u32>,
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
    /// `DELAY`: the ticks until it arrives; `None` when it does not.
    pub delay: Option<u32>,
    /// `SPOT_RANKED`: the spot; `SPOT_PICKED`: the spot, `None` for the
    /// unchecked wish point in `pos`.
    pub spot: Option<u32>,
    /// `SPOT_PICKED` without a spot: the wish point.
    pub pos: Pos,
    /// `ABORT`, `TRIGGER`: it fired.
    pub fired: bool,
    /// `TARGET`: the targets slot kept it.
    pub kept: bool,
    /// `SPOT_RANKED`: the spot has every wanted tag.
    pub fits: bool,
    /// `ASSIGNED`: the member cost (saturated).
    pub cost: i64,
    /// `SPOT_RANKED`: the distance from the wish point (saturated).
    pub distance: i64,
    /// `TARGET`: the target.
    pub fact: Fact,
    /// `REPORT`: the report.
    pub msg: Message,
    /// `DECIDE`: the decider's record.
    pub decide: DecideTraceCode,
    /// `KNOWLEDGE`: the knowledge record.
    pub knowledge: KnowledgeTraceCode,
    /// `AREAS`: the areas record.
    pub areas: AreasTraceCode,
    /// `INBOX`: the inbox record.
    pub inbox: MessageTraceCode,
}

/// The numbers of a squad trace record.
pub fn squad_trace(event: SquadTrace) -> SquadTraceCode {
    let of = |kind| SquadTraceCode {
        kind,
        ..SquadTraceCode::default()
    };
    match event {
        SquadTrace::Target { fact, kept } => SquadTraceCode {
            fact,
            kept,
            ..of(SQUAD_TRACE_TARGET)
        },
        SquadTrace::Decide {
            tactic,
            target,
            event,
        } => SquadTraceCode {
            tactic: index_u32(tactic),
            target,
            decide: decide_trace(event),
            ..of(SQUAD_TRACE_DECIDE)
        },
        SquadTrace::Abort {
            tactic,
            target,
            abort,
            fired,
        } => SquadTraceCode {
            tactic: index_u32(tactic),
            target,
            abort: index_u32(abort),
            fired,
            ..of(SQUAD_TRACE_ABORT)
        },
        SquadTrace::Picked {
            tactic,
            target,
            score,
        } => SquadTraceCode {
            tactic: index_u32(tactic),
            target,
            score,
            ..of(SQUAD_TRACE_PICKED)
        },
        SquadTrace::NoPick => of(SQUAD_TRACE_NO_PICK),
        SquadTrace::Thinking { plan, until } => SquadTraceCode {
            plan,
            until,
            ..of(SQUAD_TRACE_THINKING)
        },
        SquadTrace::Assigned { slot, member, cost } => SquadTraceCode {
            slot: index_u32(slot),
            member,
            cost: sat64(cost),
            ..of(SQUAD_TRACE_ASSIGNED)
        },
        SquadTrace::Spot {
            member,
            event:
                SpotTrace::Ranked {
                    spot,
                    fits,
                    distance,
                },
        } => SquadTraceCode {
            member,
            spot: Some(index_u32(spot)),
            fits,
            distance: sat64(distance),
            ..of(SQUAD_TRACE_SPOT_RANKED)
        },
        SquadTrace::Spot {
            member,
            event: SpotTrace::Picked(pick),
        } => {
            let (spot, pos) = match pick {
                SpotPick::Spot(i) => (Some(index_u32(i)), [0; 3]),
                SpotPick::Unchecked(p) => (None, p),
            };
            SquadTraceCode {
                member,
                spot,
                pos,
                ..of(SQUAD_TRACE_SPOT_PICKED)
            }
        }
        SquadTrace::Trigger { fired } => SquadTraceCode {
            fired,
            ..of(SQUAD_TRACE_TRIGGER)
        },
        SquadTrace::Report { msg } => SquadTraceCode {
            msg,
            ..of(SQUAD_TRACE_REPORT)
        },
        SquadTrace::Delay { member, delay } => SquadTraceCode {
            member,
            delay,
            ..of(SQUAD_TRACE_DELAY)
        },
        SquadTrace::Knowledge(k) => SquadTraceCode {
            knowledge: knowledge_trace(k),
            ..of(SQUAD_TRACE_KNOWLEDGE)
        },
        SquadTrace::Areas(a) => SquadTraceCode {
            areas: areas_trace(a),
            ..of(SQUAD_TRACE_AREAS)
        },
        SquadTrace::Inbox(m) => SquadTraceCode {
            inbox: message_trace(m),
            ..of(SQUAD_TRACE_INBOX)
        },
    }
}

/// The numbers of a keeper trace record (`sx_keeper_trace`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeeperTraceCode {
    /// `SX_KEEPER_TRACE_*`.
    pub kind: u32,
    /// Ticks since the goal was postponed; 0 when it is not.
    pub waited: u32,
    /// The give-up ticks.
    pub limit: u32,
    /// The answer.
    pub broken: bool,
}

/// The numbers of a keeper trace record.
pub fn keeper_trace(event: KeeperTrace) -> KeeperTraceCode {
    match event {
        KeeperTrace::Checked {
            waited,
            limit,
            broken,
        } => KeeperTraceCode {
            kind: KEEPER_TRACE_CHECKED,
            waited,
            limit,
            broken,
        },
    }
}

/// The numbers of a crew trace record (`sx_crew_trace`). Fields that do not
/// apply to `kind` are 0 (`None`, `false`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CrewTraceCode {
    /// `SX_CREW_TRACE_*`.
    pub kind: u32,
    /// `KEEPER`: the member; `SENT`: the sender.
    pub member: u32,
    /// `SENT`: the ticks until it reaches the leader; `None` when never.
    pub delay: Option<u32>,
    /// `SENT`: the inbox took it.
    pub queued: bool,
    /// `SENT`: the report.
    pub msg: Message,
    /// `KEEPER`: the keeper's record.
    pub keeper: KeeperTraceCode,
    /// `SQUAD`: the squad's record.
    pub squad: SquadTraceCode,
}

/// The numbers of a crew trace record.
pub fn crew_trace(event: CrewTrace) -> CrewTraceCode {
    match event {
        CrewTrace::Squad(e) => CrewTraceCode {
            kind: CREW_TRACE_SQUAD,
            squad: squad_trace(e),
            ..CrewTraceCode::default()
        },
        CrewTrace::Keeper { member, event } => CrewTraceCode {
            kind: CREW_TRACE_KEEPER,
            member,
            keeper: keeper_trace(event),
            ..CrewTraceCode::default()
        },
        CrewTrace::Sent { msg, delay, queued } => CrewTraceCode {
            kind: CREW_TRACE_SENT,
            member: msg.sender,
            delay,
            queued,
            msg,
            ..CrewTraceCode::default()
        },
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

/// The place of a record: `kind` is `SX_PLACE_*`, `value` the `SX_SIDE_*`,
/// the tag bits or the id of `SX_PLACE_CUSTOM`, `offset` the offset of `SX_PLACE_OFFSET`. `Some(None)`
/// for `SX_PLACE_NONE`, `None` if the kind or the side is unknown.
pub fn place(kind: u32, value: u32, offset: Pos) -> Option<Option<Place>> {
    Some(Some(match kind {
        PLACE_NONE => return Some(None),
        PLACE_OFFSET => Place::Offset(offset),
        PLACE_SIDE => Place::Side(side(value)?),
        PLACE_BETWEEN => Place::Between,
        PLACE_TAGGED => Place::Tagged(value),
        PLACE_HERE => Place::Here,
        PLACE_CUSTOM => Place::Custom(value),
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
        Some(Place::Custom(id)) => (PLACE_CUSTOM, id, [0; 3]),
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
    let pick = table.pick.map(pick_code);
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

/// `(code, tactic, input)` of a plan error: `SX_ERROR_PLAN`, or
/// `SX_ERROR_NEEDS_RULES` for `NeedsRules`; the input is `u32::MAX` where
/// there is no column; for `OptionsChanged` and `NeedsRules` both are
/// `u32::MAX`.
pub fn plan_error(error: PlanError) -> (i32, u32, u32) {
    let (code, tactic, input) = match error {
        PlanError::InputOutOfRange { tactic, input } => (ERROR_PLAN, tactic, input),
        PlanError::TooManySlots { tactic } => (ERROR_PLAN, tactic, u32::MAX),
        PlanError::OptionsChanged => (ERROR_PLAN, usize::MAX, u32::MAX),
        PlanError::NeedsRules => (ERROR_NEEDS_RULES, usize::MAX, u32::MAX),
    };
    (code, u32::try_from(tactic).unwrap_or(u32::MAX), input)
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

// ---- mission ------------------------------------------------------------------

/// The `SX_PICK_*` value and `top` of a pick.
pub const fn pick_code(pick: Pick) -> (u32, u32) {
    match pick {
        Pick::Best => (PICK_BEST, 0),
        Pick::Weighted { top } => (PICK_WEIGHTED, top),
        Pick::Uniform { top } => (PICK_UNIFORM, top),
    }
}

/// The `SX_GRID_*` value of a hex layout, `SX_GRID_SQUARE` for none.
pub const fn grid_code(layout: Option<HexLayout>) -> u32 {
    match layout {
        None => GRID_SQUARE,
        Some(HexLayout::OddR) => GRID_HEX_ODD_R,
        Some(HexLayout::EvenR) => GRID_HEX_EVEN_R,
        Some(HexLayout::OddQ) => GRID_HEX_ODD_Q,
        Some(HexLayout::EvenQ) => GRID_HEX_EVEN_Q,
    }
}

/// The place of an `SX_WHERE_*` value; `pos` and `subject` are used by
/// `FIXED` and `SUBJECT` only.
pub fn where_of(kind: u32, pos: Pos, subject: u32) -> Option<Where> {
    Some(match kind {
        WHERE_FIXED => Where::Fixed(pos),
        WHERE_SUBJECT => Where::Subject(subject),
        WHERE_MOTIVE_PLACE => Where::MotivePlace,
        WHERE_MOTIVE_SUBJECT => Where::MotiveSubject,
        WHERE_HERE => Where::Here,
        _ => return None,
    })
}

/// The `SX_WHERE_*` value of a place, with its position (`FIXED`) and
/// subject (`SUBJECT`); both 0 when unused. The inverse of [`where_of`].
pub const fn where_code(at: Where) -> (u32, Pos, u32) {
    match at {
        Where::Fixed(p) => (WHERE_FIXED, p, 0),
        Where::Subject(s) => (WHERE_SUBJECT, [0; 3], s),
        Where::MotivePlace => (WHERE_MOTIVE_PLACE, [0; 3], 0),
        Where::MotiveSubject => (WHERE_MOTIVE_SUBJECT, [0; 3], 0),
        Where::Here => (WHERE_HERE, [0; 3], 0),
    }
}

/// The step of an `SX_STEP_*` value with its place `at` (`None`: the place
/// was not valid, which only a step with a place refuses) and the numbers
/// `a` and `b`:
///
/// | Kind | `at` | `a` | `b` |
/// |------|------|-----|-----|
/// | `GO_TO`, `HOLD` | the place | | |
/// | `WAIT` | | ticks | |
/// | `FOLLOW`, `ATTACK` | | subject | |
/// | `WAIT_FOR` | | subject | radius |
/// | `SEARCH` | centre | radius | ticks |
/// | `SIGNAL` | | signal | |
/// | `CUSTOM` | where | the game's kind | param |
/// | `CALL` | | channel | kind |
/// | `SQUAD` | target | tactic | |
pub fn step_kind(kind: u32, at: Option<Where>, a: u32, b: u32) -> Option<StepKind> {
    Some(match kind {
        STEP_GO_TO => StepKind::GoTo(at?),
        STEP_WAIT => StepKind::Wait(a),
        STEP_PAUSE => StepKind::Pause,
        STEP_FOLLOW => StepKind::Follow(a),
        STEP_WAIT_FOR => StepKind::WaitFor {
            subject: a,
            radius: b,
        },
        STEP_SEARCH => StepKind::Search {
            centre: at?,
            radius: a,
            ticks: b,
        },
        STEP_HOLD => StepKind::Hold(at?),
        STEP_ATTACK => StepKind::Attack(a),
        STEP_WANDER => StepKind::Wander,
        STEP_SIGNAL => StepKind::Signal(a),
        STEP_CUSTOM => StepKind::Custom {
            kind: a,
            at: at?,
            param: b,
        },
        STEP_CALL => StepKind::Call {
            channel: a,
            kind: b,
        },
        STEP_SQUAD => StepKind::Squad {
            tactic: a,
            target: at?,
        },
        _ => return None,
    })
}

/// The `SX_STEP_*` value of a step with its place (`None` for a kind
/// without one) and the numbers `a` and `b` (0 when unused), as in the
/// table of [`step_kind`]; its inverse.
pub const fn step_kind_code(kind: StepKind) -> (u32, Option<Where>, u32, u32) {
    match kind {
        StepKind::GoTo(at) => (STEP_GO_TO, Some(at), 0, 0),
        StepKind::Wait(ticks) => (STEP_WAIT, None, ticks, 0),
        StepKind::Pause => (STEP_PAUSE, None, 0, 0),
        StepKind::Follow(s) => (STEP_FOLLOW, None, s, 0),
        StepKind::WaitFor { subject, radius } => (STEP_WAIT_FOR, None, subject, radius),
        StepKind::Search {
            centre,
            radius,
            ticks,
        } => (STEP_SEARCH, Some(centre), radius, ticks),
        StepKind::Hold(at) => (STEP_HOLD, Some(at), 0, 0),
        StepKind::Attack(s) => (STEP_ATTACK, None, s, 0),
        StepKind::Wander => (STEP_WANDER, None, 0, 0),
        StepKind::Signal(n) => (STEP_SIGNAL, None, n, 0),
        StepKind::Custom { kind, at, param } => (STEP_CUSTOM, Some(at), kind, param),
        StepKind::Call { channel, kind } => (STEP_CALL, None, channel, kind),
        StepKind::Squad { tactic, target } => (STEP_SQUAD, Some(target), tactic, 0),
    }
}

/// A fail condition as numbers (`SX_FAIL_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FailCode {
    /// `SX_FAIL_*`.
    pub kind: u32,
    /// The subject (`UNKNOWN`, `DEFEATED`, `FACT`).
    pub subject: u32,
    /// The fact kind (`FACT`).
    pub fact_kind: u32,
    /// Bits of the fact's data (`FACT`).
    pub mask: u32,
    /// The value of those bits (`FACT`).
    pub value: u32,
    /// The game's condition (`CUSTOM`).
    pub id: u32,
    /// Bits of the flags (`FLAGS`).
    pub flags_mask: u64,
    /// The value of those bits (`FLAGS`).
    pub flags_value: u64,
}

/// The fail condition of its numbers; `None` for an unknown kind.
pub const fn fail(code: &FailCode) -> Option<Fail> {
    Some(match code.kind {
        FAIL_UNKNOWN => Fail::Unknown {
            subject: code.subject,
        },
        FAIL_DEFEATED => Fail::Defeated {
            subject: code.subject,
        },
        FAIL_FACT => Fail::Fact {
            subject: code.subject,
            kind: code.fact_kind,
            mask: code.mask,
            value: code.value,
        },
        FAIL_FLAGS => Fail::Flags {
            mask: code.flags_mask,
            value: code.flags_value,
        },
        FAIL_CUSTOM => Fail::Custom(code.id),
        _ => return None,
    })
}

/// The numbers of a fail condition, the inverse of [`fail`] (unused fields 0).
pub const fn fail_code(fail: Fail) -> FailCode {
    let zero = FailCode {
        kind: 0,
        subject: 0,
        fact_kind: 0,
        mask: 0,
        value: 0,
        id: 0,
        flags_mask: 0,
        flags_value: 0,
    };
    match fail {
        Fail::Unknown { subject } => FailCode {
            kind: FAIL_UNKNOWN,
            subject,
            ..zero
        },
        Fail::Defeated { subject } => FailCode {
            kind: FAIL_DEFEATED,
            subject,
            ..zero
        },
        Fail::Fact {
            subject,
            kind,
            mask,
            value,
        } => FailCode {
            kind: FAIL_FACT,
            subject,
            fact_kind: kind,
            mask,
            value,
            ..zero
        },
        Fail::Flags { mask, value } => FailCode {
            kind: FAIL_FLAGS,
            flags_mask: mask,
            flags_value: value,
            ..zero
        },
        Fail::Custom(id) => FailCode {
            kind: FAIL_CUSTOM,
            id,
            ..zero
        },
    }
}

/// The motive kind of an `SX_MOTIVE_*` value (`custom`: the game's kind).
pub const fn motive_kind(kind: u32, custom: u32) -> Option<MotiveKind> {
    Some(match kind {
        MOTIVE_GUARD => MotiveKind::Guard,
        MOTIVE_DEFEAT => MotiveKind::Defeat,
        MOTIVE_REACH => MotiveKind::Reach,
        MOTIVE_CUSTOM => MotiveKind::Custom(custom),
        _ => return None,
    })
}

/// The `SX_MOTIVE_*` value and the game's kind (0 if not custom).
pub const fn motive_kind_code(kind: MotiveKind) -> (u32, u32) {
    match kind {
        MotiveKind::Guard => (MOTIVE_GUARD, 0),
        MotiveKind::Defeat => (MOTIVE_DEFEAT, 0),
        MotiveKind::Reach => (MOTIVE_REACH, 0),
        MotiveKind::Custom(k) => (MOTIVE_CUSTOM, k),
    }
}

/// The reaction of an `SX_ON_LOST_*` value; `ticks` and `radius` are the
/// search's.
pub const fn on_lost(kind: u32, ticks: u32, radius: u32) -> Option<OnLost> {
    Some(match kind {
        ON_LOST_RETURN => OnLost::Return,
        ON_LOST_SEARCH => OnLost::Search { ticks, radius },
        ON_LOST_KEEP => OnLost::Keep,
        _ => return None,
    })
}

/// The `SX_ON_LOST_*` value, ticks and radius (0 unless it searches).
pub const fn on_lost_code(on_lost: OnLost) -> (u32, u32, u32) {
    match on_lost {
        OnLost::Return => (ON_LOST_RETURN, 0, 0),
        OnLost::Search { ticks, radius } => (ON_LOST_SEARCH, ticks, radius),
        OnLost::Keep => (ON_LOST_KEEP, 0, 0),
    }
}

/// The preset of an `SX_LEEWAY_*` value.
pub const fn leeway_preset(which: u32) -> Option<Leeway> {
    match which {
        LEEWAY_STRICT => Some(Leeway::STRICT),
        LEEWAY_FLEXIBLE => Some(Leeway::FLEXIBLE),
        LEEWAY_FREE => Some(Leeway::FREE),
        _ => None,
    }
}

/// The detour of an `SX_DETOUR_*` value: `ENGAGE` takes `subject`, `FLEE`
/// the place `at` (`None`: not valid) to get away from and `to`, `CUSTOM`
/// the game's kind `custom`, `at` and `param`.
pub const fn detour(
    kind: u32,
    subject: u32,
    at: Option<Where>,
    to: Option<Pos>,
    custom: u32,
    param: u32,
) -> Option<Detour> {
    Some(match (kind, at) {
        (DETOUR_ENGAGE, _) => Detour::Engage(subject),
        (DETOUR_FLEE, Some(from)) => Detour::Flee { from, to },
        (DETOUR_CUSTOM, Some(at)) => Detour::Custom {
            kind: custom,
            at,
            param,
        },
        _ => return None,
    })
}

/// An action as numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActionCode {
    /// `SX_ACTION_*`.
    pub kind: u32,
    /// The position, if the action has one.
    pub pos: Option<Pos>,
    /// The subject (`FOLLOW`, `ATTACK`).
    pub subject: u32,
    /// The game's kind (`CUSTOM`).
    pub custom: u32,
    /// The parameter (`CUSTOM`).
    pub param: u32,
}

/// The numbers of an action.
pub const fn action_code(action: Action) -> ActionCode {
    const fn of(kind: u32, pos: Option<Pos>) -> ActionCode {
        ActionCode {
            kind,
            pos,
            subject: 0,
            custom: 0,
            param: 0,
        }
    }
    match action {
        Action::None => of(ACTION_NONE, None),
        Action::GoTo(p) => of(ACTION_GO_TO, Some(p)),
        Action::Wait => of(ACTION_WAIT, None),
        Action::Follow { subject, pos } => ActionCode {
            subject,
            ..of(ACTION_FOLLOW, Some(pos))
        },
        Action::Hold(p) => of(ACTION_HOLD, Some(p)),
        Action::Attack { subject, pos } => ActionCode {
            subject,
            ..of(ACTION_ATTACK, Some(pos))
        },
        Action::Search(p) => of(ACTION_SEARCH, Some(p)),
        Action::Wander => of(ACTION_WANDER, None),
        Action::Flee(p) => of(ACTION_FLEE, Some(p)),
        Action::Custom { kind, pos, param } => ActionCode {
            custom: kind,
            param,
            ..of(ACTION_CUSTOM, pos)
        },
    }
}

/// A mission event as numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MissionEventCode {
    /// `SX_MISSION_EVENT_*`.
    pub kind: u32,
    /// `STEP_DONE`: the step; `BROKEN`: `SX_BREAK_*`; `REPLANNED`: the
    /// option; `DETOUR_ENDED`: `SX_DETOUR_END_*`; `SIGNAL`: the signal;
    /// `CALL`: the channel; `WANTS_SQUAD`: the tactic.
    pub a: u32,
    /// `CALL`: the kind of call.
    pub b: u32,
    /// `CALL`: the caller's position; `WANTS_SQUAD`: the target place.
    pub pos: Option<Pos>,
    /// `WANTS_SQUAD`: the target subject.
    pub subject: Option<u32>,
    /// `BROKEN` with `SX_BREAK_FAIL`: the condition that fired.
    pub fail: Option<Fail>,
    /// `CALL`: the caller's motive.
    pub motive: Option<Motive>,
}

/// The numbers of a mission event.
pub const fn mission_event(event: MissionEvent) -> MissionEventCode {
    const fn of(kind: u32, a: u32) -> MissionEventCode {
        MissionEventCode {
            kind,
            a,
            b: 0,
            pos: None,
            subject: None,
            fail: None,
            motive: None,
        }
    }
    match event {
        MissionEvent::StepDone(i) => of(MISSION_EVENT_STEP_DONE, i as u32),
        MissionEvent::Looped => of(MISSION_EVENT_LOOPED, 0),
        MissionEvent::PlanDone => of(MISSION_EVENT_PLAN_DONE, 0),
        MissionEvent::Broken(BreakReason::Fail(f)) => MissionEventCode {
            fail: Some(f),
            ..of(MISSION_EVENT_BROKEN, BREAK_FAIL)
        },
        MissionEvent::Broken(BreakReason::Impossible) => of(MISSION_EVENT_BROKEN, BREAK_IMPOSSIBLE),
        MissionEvent::Broken(BreakReason::PlanEnded) => of(MISSION_EVENT_BROKEN, BREAK_PLAN_ENDED),
        MissionEvent::Replanned { option } => of(MISSION_EVENT_REPLANNED, option),
        MissionEvent::FellBack => of(MISSION_EVENT_FELL_BACK, 0),
        MissionEvent::DetourStarted => of(MISSION_EVENT_DETOUR_STARTED, 0),
        MissionEvent::DetourEnded(why) => of(
            MISSION_EVENT_DETOUR_ENDED,
            match why {
                DetourEnd::Defeated => DETOUR_END_DEFEATED,
                DetourEnd::Forgotten => DETOUR_END_FORGOTTEN,
                DetourEnd::Lost => DETOUR_END_LOST,
                DetourEnd::Leash => DETOUR_END_LEASH,
                DetourEnd::Safe => DETOUR_END_SAFE,
                DetourEnd::Arrived => DETOUR_END_ARRIVED,
                DetourEnd::Ended => DETOUR_END_ENDED,
            },
        ),
        MissionEvent::Returned => of(MISSION_EVENT_RETURNED, 0),
        MissionEvent::Signal(n) => of(MISSION_EVENT_SIGNAL, n),
        MissionEvent::Call {
            channel,
            kind,
            pos,
            motive,
        } => MissionEventCode {
            b: kind,
            pos: Some(pos),
            motive: Some(motive),
            ..of(MISSION_EVENT_CALL, channel)
        },
        MissionEvent::WantsSquad {
            tactic,
            target,
            subject,
        } => MissionEventCode {
            pos: target,
            subject,
            ..of(MISSION_EVENT_WANTS_SQUAD, tactic)
        },
    }
}

/// The numbers of a mission trace record (`sx_mission_trace`). Fields that
/// do not apply to `kind` are 0 (`None`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MissionTraceCode {
    /// `SX_MISSION_TRACE_*`.
    pub kind: u32,
    /// `COLUMN`, `DECIDE`, `RESCORED`: the candidate's option; `PICKED`:
    /// the winner, `None` for none.
    pub option: Option<u32>,
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
    /// `BEGIN`: the position the step goes for, if any.
    pub at: Option<Pos>,
    /// `DECIDE`: the decider's record.
    pub decide: DecideTraceCode,
}

/// The numbers of a mission trace record.
pub fn mission_trace(event: MissionTrace) -> MissionTraceCode {
    let of = |kind, option| MissionTraceCode {
        kind,
        option,
        ..MissionTraceCode::default()
    };
    match event {
        MissionTrace::Column {
            option,
            column,
            value,
        } => MissionTraceCode {
            column: u32::try_from(column).unwrap_or(u32::MAX),
            value,
            ..of(MISSION_TRACE_COLUMN, Some(option))
        },
        MissionTrace::Decide { option, event } => MissionTraceCode {
            decide: decide_trace(event),
            ..of(MISSION_TRACE_DECIDE, Some(option))
        },
        MissionTrace::Rescored {
            option,
            built_in,
            score,
        } => MissionTraceCode {
            built_in,
            score,
            ..of(MISSION_TRACE_RESCORED, Some(option))
        },
        MissionTrace::Picked { option, score } => MissionTraceCode {
            score,
            ..of(MISSION_TRACE_PICKED, option)
        },
        MissionTrace::Begin { index, at } => MissionTraceCode {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            at,
            ..of(MISSION_TRACE_BEGIN, None)
        },
    }
}

#[cfg(test)]
mod tests;
