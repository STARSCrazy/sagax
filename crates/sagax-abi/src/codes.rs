//! The numbers of the C interface (`SX_*` in `sagax.h`) that the C and the
//! WebAssembly layer both use. The names are the header's without `SX_`.

/// `SX_ERROR_ARGUMENT`: a pointer is 0 or a number is out of range.
pub const ERROR_ARGUMENT: i32 = -1;
/// `SX_ERROR_INTERNAL`: a bug in the library.
pub const ERROR_INTERNAL: i32 = -2;
/// `SX_ERROR_PLAN`: a tactic reads a column beyond the scoring row.
pub const ERROR_PLAN: i32 = -3;
/// `SX_ERROR_NEEDS_RULES`: a plain call on a squad with custom parts (see
/// `Squad::needs_rules`); use the `_with` call.
pub const ERROR_NEEDS_RULES: i32 = -4;

/// `SX_FOUR_WAYS`: square grid, only straight moves.
pub const FOUR_WAYS: u32 = 1;
/// `SX_CUT_CORNERS`: square grid, a diagonal past a blocked side neighbour.
pub const CUT_CORNERS: u32 = 2;

/// `SX_IDLE`: no search to go on with.
pub const IDLE: i32 = 0;
/// `SX_RUNNING`: the budget is used up.
pub const RUNNING: i32 = 1;
/// `SX_FOUND`: the goal was reached.
pub const FOUND: i32 = 2;
/// `SX_PARTIAL`: the goal is unreachable; a way to the closest cell.
pub const PARTIAL: i32 = 3;
/// `SX_UNREACHABLE`: nothing closer than the start.
pub const UNREACHABLE: i32 = 4;
/// `SX_DONE`: an area or flow field is complete.
pub const DONE: i32 = 5;

/// `SX_SEARCH_TRACE_STARTED`: a search begins.
pub const SEARCH_TRACE_STARTED: u32 = 1;
/// `SX_SEARCH_TRACE_EXPANDED`: a cell was expanded.
pub const SEARCH_TRACE_EXPANDED: u32 = 2;
/// `SX_SEARCH_TRACE_IMPROVED`: a cell was reached more cheaply.
pub const SEARCH_TRACE_IMPROVED: u32 = 3;
/// `SX_SEARCH_TRACE_PAUSED`: the budget is used up.
pub const SEARCH_TRACE_PAUSED: u32 = 4;
/// `SX_SEARCH_TRACE_FINISHED`: the search ended.
pub const SEARCH_TRACE_FINISHED: u32 = 5;

/// `SX_NAV_TRACE_SEARCH`: a step of a search (`search` is valid).
pub const NAV_TRACE_SEARCH: u32 = 1;
/// `SX_NAV_TRACE_SHORTCUT`: the smoothing tested a straight line.
pub const NAV_TRACE_SHORTCUT: u32 = 2;
/// `SX_NAV_TRACE_PLACED`: a formation member got its target.
pub const NAV_TRACE_PLACED: u32 = 3;
/// `SX_NAV_TRACE_UNPLACED`: a formation member found no free cell.
pub const NAV_TRACE_UNPLACED: u32 = 4;
/// `SX_NAV_TRACE_FLEE_BEST`: the cell a flee chose.
pub const NAV_TRACE_FLEE_BEST: u32 = 5;

/// `SX_GRID_SQUARE`.
pub const GRID_SQUARE: u32 = 0;
/// `SX_GRID_HEX_ODD_R`.
pub const GRID_HEX_ODD_R: u32 = 1;
/// `SX_GRID_HEX_EVEN_R`.
pub const GRID_HEX_EVEN_R: u32 = 2;
/// `SX_GRID_HEX_ODD_Q`.
pub const GRID_HEX_ODD_Q: u32 = 3;
/// `SX_GRID_HEX_EVEN_Q`.
pub const GRID_HEX_EVEN_Q: u32 = 4;

/// `SX_METRIC_EUCLID`.
pub const METRIC_EUCLID: u32 = 0;
/// `SX_METRIC_MANHATTAN`.
pub const METRIC_MANHATTAN: u32 = 1;
/// `SX_METRIC_CHEBYSHEV`.
pub const METRIC_CHEBYSHEV: u32 = 2;
/// `SX_METRIC_HEX`.
pub const METRIC_HEX: u32 = 3;

/// `SX_PICK_BEST`.
pub const PICK_BEST: u32 = 0;
/// `SX_PICK_WEIGHTED`.
pub const PICK_WEIGHTED: u32 = 1;
/// `SX_PICK_UNIFORM`.
pub const PICK_UNIFORM: u32 = 2;

/// `SX_PLACE_NONE`.
pub const PLACE_NONE: u32 = 0;
/// `SX_PLACE_OFFSET`.
pub const PLACE_OFFSET: u32 = 1;
/// `SX_PLACE_SIDE`.
pub const PLACE_SIDE: u32 = 2;
/// `SX_PLACE_BETWEEN`.
pub const PLACE_BETWEEN: u32 = 3;
/// `SX_PLACE_TAGGED`.
pub const PLACE_TAGGED: u32 = 4;
/// `SX_PLACE_HERE`.
pub const PLACE_HERE: u32 = 5;
/// `SX_PLACE_CUSTOM`: the game's own place; `value` is its id.
pub const PLACE_CUSTOM: u32 = 6;

/// `SX_SIDE_FRONT`.
pub const SIDE_FRONT: u32 = 0;
/// `SX_SIDE_LEFT`.
pub const SIDE_LEFT: u32 = 1;
/// `SX_SIDE_RIGHT`.
pub const SIDE_RIGHT: u32 = 2;
/// `SX_SIDE_REAR`.
pub const SIDE_REAR: u32 = 3;

/// `SX_GOAL_MOVE`.
pub const GOAL_MOVE: u32 = 0;
/// `SX_GOAL_HOLD`.
pub const GOAL_HOLD: u32 = 1;
/// `SX_GOAL_ATTACK`.
pub const GOAL_ATTACK: u32 = 2;
/// `SX_GOAL_WAIT`.
pub const GOAL_WAIT: u32 = 3;
/// `SX_GOAL_RETREAT`.
pub const GOAL_RETREAT: u32 = 4;
/// `SX_GOAL_CUSTOM`.
pub const GOAL_CUSTOM: u32 = 5;

/// `SX_START_NOW`.
pub const START_NOW: u32 = 0;
/// `SX_START_AFTER_TICKS`.
pub const START_AFTER_TICKS: u32 = 1;
/// `SX_START_ON_SIGNAL`.
pub const START_ON_SIGNAL: u32 = 2;

/// `SX_CMP_AT_LEAST`.
pub const CMP_AT_LEAST: u32 = 0;
/// `SX_CMP_AT_MOST`.
pub const CMP_AT_MOST: u32 = 1;
/// `SX_CMP_EQUAL`.
pub const CMP_EQUAL: u32 = 2;
/// `SX_CMP_NOT_EQUAL`.
pub const CMP_NOT_EQUAL: u32 = 3;
/// `SX_SHAPE_LINE`.
pub const SHAPE_LINE: u32 = 0;
/// `SX_SHAPE_WEDGE`.
pub const SHAPE_WEDGE: u32 = 1;
/// `SX_SHAPE_COLUMN`.
pub const SHAPE_COLUMN: u32 = 2;

/// `SX_MEMBER_ACTIVE`.
pub const MEMBER_ACTIVE: u32 = 0;
/// `SX_MEMBER_SUSPENDED`.
pub const MEMBER_SUSPENDED: u32 = 1;
/// `SX_MEMBER_GONE`.
pub const MEMBER_GONE: u32 = 2;

/// `SX_SQUAD_KEEP`: `sx_squad_check` found nothing to do.
pub const SQUAD_KEEP: i32 = 0;
/// `SX_SQUAD_REPLAN`: a new plan is pending.
pub const SQUAD_REPLAN: i32 = 1;
/// `SX_SQUAD_SIGNAL`: the signal fired.
pub const SQUAD_SIGNAL: i32 = 2;
/// `SX_SQUAD_NO_PLAN`: no plan is active and none could be made.
pub const SQUAD_NO_PLAN: i32 = 3;

/// `SX_CREW_PLANNED`.
pub const CREW_PLANNED: u32 = 1;
/// `SX_CREW_ACTIVATED`.
pub const CREW_ACTIVATED: u32 = 2;
/// `SX_CREW_SIGNAL`.
pub const CREW_SIGNAL: u32 = 3;
/// `SX_CREW_ORDER`.
pub const CREW_ORDER: u32 = 4;
/// `SX_CREW_START`.
pub const CREW_START: u32 = 5;
/// `SX_CREW_ARRIVED`.
pub const CREW_ARRIVED: u32 = 6;
/// `SX_CREW_BROKEN`.
pub const CREW_BROKEN: u32 = 7;
/// `SX_CREW_NO_PLAN`.
pub const CREW_NO_PLAN: u32 = 8;

/// `SX_ALL_OPTIONS`: `option` of `sx_decider_set_combine` for every option.
pub const ALL_OPTIONS: u32 = u32::MAX;
/// `SX_COMBINE_FOLLOW`: `combine` of `sx_decider_set_combine` for "follow
/// the decider".
pub const COMBINE_FOLLOW: u32 = u32::MAX;

/// `SX_WHERE_FIXED`: a fixed position.
pub const WHERE_FIXED: u32 = 0;
/// `SX_WHERE_SUBJECT`: where a subject was last known.
pub const WHERE_SUBJECT: u32 = 1;
/// `SX_WHERE_MOTIVE_PLACE`: the place of the motive.
pub const WHERE_MOTIVE_PLACE: u32 = 2;
/// `SX_WHERE_MOTIVE_SUBJECT`: where the motive's subject was last known.
pub const WHERE_MOTIVE_SUBJECT: u32 = 3;
/// `SX_WHERE_HERE`: where the character stands.
pub const WHERE_HERE: u32 = 4;

/// `SX_STEP_GO_TO`.
pub const STEP_GO_TO: u32 = 0;
/// `SX_STEP_WAIT`.
pub const STEP_WAIT: u32 = 1;
/// `SX_STEP_PAUSE`.
pub const STEP_PAUSE: u32 = 2;
/// `SX_STEP_FOLLOW`.
pub const STEP_FOLLOW: u32 = 3;
/// `SX_STEP_WAIT_FOR`.
pub const STEP_WAIT_FOR: u32 = 4;
/// `SX_STEP_SEARCH`.
pub const STEP_SEARCH: u32 = 5;
/// `SX_STEP_HOLD`.
pub const STEP_HOLD: u32 = 6;
/// `SX_STEP_ATTACK`.
pub const STEP_ATTACK: u32 = 7;
/// `SX_STEP_WANDER`.
pub const STEP_WANDER: u32 = 8;
/// `SX_STEP_SIGNAL`.
pub const STEP_SIGNAL: u32 = 9;
/// `SX_STEP_CUSTOM`.
pub const STEP_CUSTOM: u32 = 10;
/// `SX_STEP_CALL`.
pub const STEP_CALL: u32 = 11;
/// `SX_STEP_SQUAD`.
pub const STEP_SQUAD: u32 = 12;

/// `SX_FAIL_UNKNOWN`.
pub const FAIL_UNKNOWN: u32 = 0;
/// `SX_FAIL_DEFEATED`.
pub const FAIL_DEFEATED: u32 = 1;
/// `SX_FAIL_FACT`.
pub const FAIL_FACT: u32 = 2;
/// `SX_FAIL_FLAGS`.
pub const FAIL_FLAGS: u32 = 3;
/// `SX_FAIL_CUSTOM`.
pub const FAIL_CUSTOM: u32 = 4;

/// `SX_MOTIVE_GUARD`.
pub const MOTIVE_GUARD: u32 = 0;
/// `SX_MOTIVE_DEFEAT`.
pub const MOTIVE_DEFEAT: u32 = 1;
/// `SX_MOTIVE_REACH`.
pub const MOTIVE_REACH: u32 = 2;
/// `SX_MOTIVE_CUSTOM`.
pub const MOTIVE_CUSTOM: u32 = 3;

/// `SX_ON_LOST_RETURN`.
pub const ON_LOST_RETURN: u32 = 0;
/// `SX_ON_LOST_SEARCH`.
pub const ON_LOST_SEARCH: u32 = 1;
/// `SX_ON_LOST_KEEP`.
pub const ON_LOST_KEEP: u32 = 2;

/// `SX_LEEWAY_STRICT`.
pub const LEEWAY_STRICT: u32 = 0;
/// `SX_LEEWAY_FLEXIBLE`.
pub const LEEWAY_FLEXIBLE: u32 = 1;
/// `SX_LEEWAY_FREE`.
pub const LEEWAY_FREE: u32 = 2;

/// `SX_DETOUR_ENGAGE`.
pub const DETOUR_ENGAGE: u32 = 0;
/// `SX_DETOUR_FLEE`.
pub const DETOUR_FLEE: u32 = 1;
/// `SX_DETOUR_CUSTOM`.
pub const DETOUR_CUSTOM: u32 = 2;

/// `SX_ACTION_NONE`.
pub const ACTION_NONE: u32 = 0;
/// `SX_ACTION_GO_TO`.
pub const ACTION_GO_TO: u32 = 1;
/// `SX_ACTION_WAIT`.
pub const ACTION_WAIT: u32 = 2;
/// `SX_ACTION_FOLLOW`.
pub const ACTION_FOLLOW: u32 = 3;
/// `SX_ACTION_HOLD`.
pub const ACTION_HOLD: u32 = 4;
/// `SX_ACTION_ATTACK`.
pub const ACTION_ATTACK: u32 = 5;
/// `SX_ACTION_SEARCH`.
pub const ACTION_SEARCH: u32 = 6;
/// `SX_ACTION_WANDER`.
pub const ACTION_WANDER: u32 = 7;
/// `SX_ACTION_FLEE`.
pub const ACTION_FLEE: u32 = 8;
/// `SX_ACTION_CUSTOM`.
pub const ACTION_CUSTOM: u32 = 9;

/// `SX_MISSION_EVENT_STEP_DONE`.
pub const MISSION_EVENT_STEP_DONE: u32 = 0;
/// `SX_MISSION_EVENT_LOOPED`.
pub const MISSION_EVENT_LOOPED: u32 = 1;
/// `SX_MISSION_EVENT_PLAN_DONE`.
pub const MISSION_EVENT_PLAN_DONE: u32 = 2;
/// `SX_MISSION_EVENT_BROKEN`.
pub const MISSION_EVENT_BROKEN: u32 = 3;
/// `SX_MISSION_EVENT_REPLANNED`.
pub const MISSION_EVENT_REPLANNED: u32 = 4;
/// `SX_MISSION_EVENT_FELL_BACK`.
pub const MISSION_EVENT_FELL_BACK: u32 = 5;
/// `SX_MISSION_EVENT_DETOUR_STARTED`.
pub const MISSION_EVENT_DETOUR_STARTED: u32 = 6;
/// `SX_MISSION_EVENT_DETOUR_ENDED`.
pub const MISSION_EVENT_DETOUR_ENDED: u32 = 7;
/// `SX_MISSION_EVENT_RETURNED`.
pub const MISSION_EVENT_RETURNED: u32 = 8;
/// `SX_MISSION_EVENT_SIGNAL`.
pub const MISSION_EVENT_SIGNAL: u32 = 9;
/// `SX_MISSION_EVENT_CALL`.
pub const MISSION_EVENT_CALL: u32 = 10;
/// `SX_MISSION_EVENT_WANTS_SQUAD`.
pub const MISSION_EVENT_WANTS_SQUAD: u32 = 11;

/// `SX_MISSION_TRACE_COLUMN`: one column of a candidate's row.
pub const MISSION_TRACE_COLUMN: u32 = 0;
/// `SX_MISSION_TRACE_FACTOR`: one consideration of a candidate.
pub const MISSION_TRACE_FACTOR: u32 = 1;
/// `SX_MISSION_TRACE_SCORED`: a candidate's score.
pub const MISSION_TRACE_SCORED: u32 = 2;
/// `SX_MISSION_TRACE_PICKED`: the winner of a scoring.
pub const MISSION_TRACE_PICKED: u32 = 3;

/// `SX_BREAK_FAIL`: a fail condition fired.
pub const BREAK_FAIL: u32 = 0;
/// `SX_BREAK_IMPOSSIBLE`: a step could not be done.
pub const BREAK_IMPOSSIBLE: u32 = 1;
/// `SX_BREAK_PLAN_ENDED`: the plan ended.
pub const BREAK_PLAN_ENDED: u32 = 2;

/// `SX_DETOUR_END_DEFEATED`.
pub const DETOUR_END_DEFEATED: u32 = 0;
/// `SX_DETOUR_END_FORGOTTEN`.
pub const DETOUR_END_FORGOTTEN: u32 = 1;
/// `SX_DETOUR_END_LOST`.
pub const DETOUR_END_LOST: u32 = 2;
/// `SX_DETOUR_END_LEASH`.
pub const DETOUR_END_LEASH: u32 = 3;
/// `SX_DETOUR_END_SAFE`.
pub const DETOUR_END_SAFE: u32 = 4;
/// `SX_DETOUR_END_ARRIVED`.
pub const DETOUR_END_ARRIVED: u32 = 5;
/// `SX_DETOUR_END_ENDED`.
pub const DETOUR_END_ENDED: u32 = 6;
