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
