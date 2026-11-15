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

/// `SX_DECIDE_TRACE_FACTOR`: one consideration of one candidate.
pub const DECIDE_TRACE_FACTOR: u32 = 1;
/// `SX_DECIDE_TRACE_SCORED`: the score of one candidate.
pub const DECIDE_TRACE_SCORED: u32 = 2;
/// `SX_DECIDE_TRACE_CHOSEN`: the chosen candidate.
pub const DECIDE_TRACE_CHOSEN: u32 = 3;
/// `SX_DECIDE_TRACE_RANKED`: mixed ranks, only the highest takes part.
pub const DECIDE_TRACE_RANKED: u32 = 4;

/// `SX_REASON_TRANSITION`: a transition matched.
pub const REASON_TRANSITION: i32 = 1;
/// `SX_REASON_TIMEOUT`: the state ran out of time.
pub const REASON_TIMEOUT: i32 = 2;
/// `SX_REASON_SET`: the game set the state.
pub const REASON_SET: i32 = 3;

/// `SX_MACHINE_TRACE_CHANGED`: a transition or the timeout fired.
pub const MACHINE_TRACE_CHANGED: u32 = 1;
/// `SX_MACHINE_TRACE_WAITING`: a transition holds but its time has not come.
pub const MACHINE_TRACE_WAITING: u32 = 2;

/// `SX_POSTED`: the outbox took the message.
pub const POSTED: u32 = 0;
/// `SX_REFUSED_FULL`: the outbox is full.
pub const REFUSED_FULL: u32 = 1;
/// `SX_REFUSED_COOLDOWN`: the last accepted post is younger than the cooldown.
pub const REFUSED_COOLDOWN: u32 = 2;

/// `SX_KNOWLEDGE_TRACE_ADDED`: a new key was stored.
pub const KNOWLEDGE_TRACE_ADDED: u32 = 1;
/// `SX_KNOWLEDGE_TRACE_UPDATED`: a known key was overwritten.
pub const KNOWLEDGE_TRACE_UPDATED: u32 = 2;
/// `SX_KNOWLEDGE_TRACE_STALE`: a merge kept the stored fact.
pub const KNOWLEDGE_TRACE_STALE: u32 = 3;
/// `SX_KNOWLEDGE_TRACE_EVICTED`: dropped to make room.
pub const KNOWLEDGE_TRACE_EVICTED: u32 = 4;
/// `SX_KNOWLEDGE_TRACE_EXPIRED`: dropped by an expire call.
pub const KNOWLEDGE_TRACE_EXPIRED: u32 = 5;
/// `SX_KNOWLEDGE_TRACE_REMOVED`: forgotten by a remove call.
pub const KNOWLEDGE_TRACE_REMOVED: u32 = 6;

/// `SX_AREAS_TRACE_ADDED`: an area was appended.
pub const AREAS_TRACE_ADDED: u32 = 1;
/// `SX_AREAS_TRACE_RENEWED`: an area near the new centre was replaced.
pub const AREAS_TRACE_RENEWED: u32 = 2;
/// `SX_AREAS_TRACE_EVICTED`: the oldest area, removed to make room.
pub const AREAS_TRACE_EVICTED: u32 = 3;
/// `SX_AREAS_TRACE_EXPIRED`: an area whose lifetime ran out.
pub const AREAS_TRACE_EXPIRED: u32 = 4;

/// `SX_TIMERS_TRACE_STARTED`: a new id was added.
pub const TIMERS_TRACE_STARTED: u32 = 1;
/// `SX_TIMERS_TRACE_RENEWED`: a present id was started again.
pub const TIMERS_TRACE_RENEWED: u32 = 2;
/// `SX_TIMERS_TRACE_REPLACED`: full, the timer with the least time left gave way.
pub const TIMERS_TRACE_REPLACED: u32 = 3;

/// `SX_MESSAGE_TRACE_POSTED`: the outbox took the message.
pub const MESSAGE_TRACE_POSTED: u32 = 1;
/// `SX_MESSAGE_TRACE_REFUSED`: the outbox refused the message.
pub const MESSAGE_TRACE_REFUSED: u32 = 2;
/// `SX_MESSAGE_TRACE_QUEUED`: the inbox took the message.
pub const MESSAGE_TRACE_QUEUED: u32 = 3;
/// `SX_MESSAGE_TRACE_DROPPED`: a full inbox or a gone sender lost the message.
pub const MESSAGE_TRACE_DROPPED: u32 = 4;
/// `SX_MESSAGE_TRACE_DELIVERED`: the message was handed out.
pub const MESSAGE_TRACE_DELIVERED: u32 = 5;

/// `SX_SQUAD_TRACE_TARGET`: the targets slot judged a known target.
pub const SQUAD_TRACE_TARGET: u32 = 1;
/// `SX_SQUAD_TRACE_DECIDE`: a decider record of a candidate.
pub const SQUAD_TRACE_DECIDE: u32 = 2;
/// `SX_SQUAD_TRACE_ABORT`: one abort rule was checked.
pub const SQUAD_TRACE_ABORT: u32 = 3;
/// `SX_SQUAD_TRACE_PICKED`: the candidate picked for a new plan.
pub const SQUAD_TRACE_PICKED: u32 = 4;
/// `SX_SQUAD_TRACE_NO_PICK`: a scoring picked nothing.
pub const SQUAD_TRACE_NO_PICK: u32 = 5;
/// `SX_SQUAD_TRACE_THINKING`: the new plan is pending.
pub const SQUAD_TRACE_THINKING: u32 = 6;
/// `SX_SQUAD_TRACE_ASSIGNED`: a member took a slot.
pub const SQUAD_TRACE_ASSIGNED: u32 = 7;
/// `SX_SQUAD_TRACE_SPOT_RANKED`: a spot of a member's ranking.
pub const SQUAD_TRACE_SPOT_RANKED: u32 = 8;
/// `SX_SQUAD_TRACE_SPOT_PICKED`: a member's spot pick.
pub const SQUAD_TRACE_SPOT_PICKED: u32 = 9;
/// `SX_SQUAD_TRACE_TRIGGER`: the trigger was checked.
pub const SQUAD_TRACE_TRIGGER: u32 = 10;
/// `SX_SQUAD_TRACE_REPORT`: a report was read.
pub const SQUAD_TRACE_REPORT: u32 = 11;
/// `SX_SQUAD_TRACE_DELAY`: the channel delay of an order.
pub const SQUAD_TRACE_DELAY: u32 = 12;
/// `SX_SQUAD_TRACE_KNOWLEDGE`: the leader's knowledge changed.
pub const SQUAD_TRACE_KNOWLEDGE: u32 = 13;
/// `SX_SQUAD_TRACE_AREAS`: the leader's danger areas changed.
pub const SQUAD_TRACE_AREAS: u32 = 14;
/// `SX_SQUAD_TRACE_INBOX`: the leader's inbox took or lost a report.
pub const SQUAD_TRACE_INBOX: u32 = 15;

/// `SX_KEEPER_TRACE_CHECKED`: a keeper was checked for giving up.
pub const KEEPER_TRACE_CHECKED: u32 = 1;

/// `SX_CREW_TRACE_SQUAD`: a squad record of the crew call.
pub const CREW_TRACE_SQUAD: u32 = 1;
/// `SX_CREW_TRACE_KEEPER`: a keeper check.
pub const CREW_TRACE_KEEPER: u32 = 2;
/// `SX_CREW_TRACE_SENT`: a member report sent to the leader.
pub const CREW_TRACE_SENT: u32 = 3;

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

/// `SX_ROUTE_NEAREST`: the `arg` nearest candidates.
pub const ROUTE_NEAREST: u32 = 0;
/// `SX_ROUTE_IN_RANGE`: every candidate within distance `arg`.
pub const ROUTE_IN_RANGE: u32 = 1;
/// `SX_ROUTE_ALL`: every candidate (`arg` is ignored).
pub const ROUTE_ALL: u32 = 2;
/// `SX_ROUTE_TO`: the candidate with id `arg`.
pub const ROUTE_TO: u32 = 3;

/// `SX_EVICT_OLDEST`: a full knowledge drops the fact seen longest ago.
pub const EVICT_OLDEST: u32 = 0;
/// `SX_EVICT_WEAKEST`: a full knowledge drops the weakest fact.
pub const EVICT_WEAKEST: u32 = 1;

/// `SX_SINE_FINE`: 65536 = 1.0.
pub const SINE_FINE: u32 = 0;
/// `SX_SINE_COARSE`: 256 = 1.0.
pub const SINE_COARSE: u32 = 1;

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
pub const MISSION_TRACE_COLUMN: u32 = 1;
/// `SX_MISSION_TRACE_DECIDE`: a decider record of a candidate.
pub const MISSION_TRACE_DECIDE: u32 = 2;
/// `SX_MISSION_TRACE_RESCORED`: a candidate's score after the game's slot.
pub const MISSION_TRACE_RESCORED: u32 = 3;
/// `SX_MISSION_TRACE_PICKED`: the winner of a scoring.
pub const MISSION_TRACE_PICKED: u32 = 4;
/// `SX_MISSION_TRACE_BEGIN`: a step begins.
pub const MISSION_TRACE_BEGIN: u32 = 5;

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

/// `SX_SPREAD_CIRCLE`: a stimulus counts by straight distance only.
pub const SPREAD_CIRCLE: u32 = 0;
/// `SX_SPREAD_STRAIGHT`: distance plus the map's damping along one walk.
pub const SPREAD_STRAIGHT: u32 = 1;

/// `SX_SENSE_SIGHT`: a percept by normal sight (also `LOST`, `FORGOTTEN`).
pub const SENSE_SIGHT: u32 = 0;
/// `SX_SENSE_THERMAL`: a percept by the thermal pass.
pub const SENSE_THERMAL: u32 = 1;
/// `SX_SENSE_SCANNER`: a percept by the scanner.
pub const SENSE_SCANNER: u32 = 2;
/// `SX_SENSE_HEARING`: a percept by hearing (the channel is beside it).
pub const SENSE_HEARING: u32 = 3;

/// `SX_PERCEPT_NOTICED`: the counted stage rose from unseen.
pub const PERCEPT_NOTICED: u32 = 0;
/// `SX_PERCEPT_RECOGNIZED`: the counted stage rose to recognised.
pub const PERCEPT_RECOGNIZED: u32 = 1;
/// `SX_PERCEPT_SEEN`: noticed or higher and in view: its current place.
pub const PERCEPT_SEEN: u32 = 2;
/// `SX_PERCEPT_CUES`: recognised cue bits not reported before.
pub const PERCEPT_CUES: u32 = 3;
/// `SX_PERCEPT_LOST`: in view at the last check, not now.
pub const PERCEPT_LOST: u32 = 4;
/// `SX_PERCEPT_FORGOTTEN`: the counted stage fell to unseen.
pub const PERCEPT_FORGOTTEN: u32 = 5;
/// `SX_PERCEPT_SEEN_THROUGH`: the observer saw through the disguise.
pub const PERCEPT_SEEN_THROUGH: u32 = 6;
/// `SX_PERCEPT_HEARD`: a stimulus was heard.
pub const PERCEPT_HEARD: u32 = 7;

/// `SX_LOOK_INSTANT`: `look` checked a target for an instant cue.
pub const LOOK_INSTANT: u32 = 0;
/// `SX_LOOK_KEPT`: `look` checked a target already noticed.
pub const LOOK_KEPT: u32 = 1;
/// `SX_LOOK_ROUND_ROBIN`: `look` checked a target in its round-robin turn.
pub const LOOK_ROUND_ROBIN: u32 = 2;

/// `SX_SENSE_PRESET_HUMAN`: an ordinary person.
pub const SENSE_PRESET_HUMAN: u32 = 1;
/// `SX_SENSE_PRESET_GUARD`: a guard (longer range, quicker).
pub const SENSE_PRESET_GUARD: u32 = 2;
/// `SX_SENSE_PRESET_WATCHER`: a watcher (wide periphery, slower
/// recognition).
pub const SENSE_PRESET_WATCHER: u32 = 3;

/// `SX_PERCEPTION_TRACE_PICKED`: `look` chose to check a target.
pub const PERCEPTION_TRACE_PICKED: u32 = 1;
/// `SX_PERCEPTION_TRACE_SKIPPED`: the budget ran out before a target.
pub const PERCEPTION_TRACE_SKIPPED: u32 = 2;
/// `SX_PERCEPTION_TRACE_LINE`: a sight line was walked.
pub const PERCEPTION_TRACE_LINE: u32 = 3;
/// `SX_PERCEPTION_TRACE_GAIN`: the factors of one pass of a check.
pub const PERCEPTION_TRACE_GAIN: u32 = 4;
/// `SX_PERCEPTION_TRACE_INSTANT`: an instant cue fired.
pub const PERCEPTION_TRACE_INSTANT: u32 = 5;
/// `SX_PERCEPTION_TRACE_STAGE`: a stage change, or a rise waiting.
pub const PERCEPTION_TRACE_STAGE: u32 = 6;
/// `SX_PERCEPTION_TRACE_DOUBT`: see-through progress.
pub const PERCEPTION_TRACE_DOUBT: u32 = 7;
/// `SX_PERCEPTION_TRACE_HEARD`: a stimulus was heard.
pub const PERCEPTION_TRACE_HEARD: u32 = 8;
/// `SX_PERCEPTION_TRACE_UNHEARD`: a stimulus was not heard.
pub const PERCEPTION_TRACE_UNHEARD: u32 = 9;
