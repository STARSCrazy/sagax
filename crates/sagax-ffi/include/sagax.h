#ifndef SAGAX_H
#define SAGAX_H

/* Sagax C interface, version SX_API_VERSION.
 *
 * Path search on square and hex grids with levels (stairs, bridges). The game
 * keeps its map; the library asks it through callbacks (sx_map). All numbers
 * are integers.
 *
 * Three handles, each made once from sx_rules and reused:
 *   sx_nav    paths, areas, smoothing, formations
 *   sx_flow   one search for many walkers with the same goals
 *   sx_dstar  one walker's way, repaired while the map changes
 *
 * Return values: functions that write a list return how many entries they
 * wrote (>= 0); the others return 0. Errors are negative (SX_ERROR_*). How a
 * search ended is in sx_result.status, never in the return value.
 * Lists of a way always begin with the start and end at the last cell.
 *
 * Memory: a handle reserves everything for rules.node_capacity cells when it
 * is made. Calls within that capacity allocate nothing.
 * Threads: one handle per thread; callbacks run on the calling thread.
 * Determinism: the same calls with the same callback answers give the same
 * results on every machine. The library uses no random numbers. */

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SX_API_VERSION 6

typedef struct sx_point {
	int32_t x, y, level;
} sx_point;

/* ---- the game's map ------------------------------------------------------ */

/* Can a walker go from cell x, y on level into the neighbour x + dx, y + dy
 * (dx, dy in -1..1)? Return the level he arrives on (>= 0), or -1.
 * Hex grids: dx, dy is the offset of one of the six neighbour cells (which
 * pairs occur depends on the row or column, see SX_GRID_HEX_*). */
typedef int32_t (*sx_step_fn)(void *context, int32_t x, int32_t y, int32_t level, int32_t dx, int32_t dy);

/* Extra cost of the move from (x, y, level) to (to_x, to_y, to_level), on top
 * of the rules' step cost (danger, rough ground). Return -1 to forbid it. */
typedef int32_t (*sx_move_cost_fn)(void *context, int32_t x, int32_t y, int32_t level,
	int32_t to_x, int32_t to_y, int32_t to_level);

/* Optional: every neighbour of one cell in one call, as step (or start_step
 * if from_start is 1) would answer. offsets holds count pairs dx, dy; write
 * levels[i] for pair i (-1: no). Saves calls only; answers must match step. */
typedef void (*sx_steps_fn)(void *context, int32_t x, int32_t y, int32_t level, int32_t from_start,
	int32_t count, const int32_t *offsets, int32_t *levels);

typedef struct sx_map {
	sx_step_fn step;            /* required */
	sx_step_fn start_step;      /* moves off the start cell (walker not in the
	                             * middle of it); 0: step. With it, turning and
	                             * smoothed lists keep the first cell after the
	                             * start. */
	sx_move_cost_fn move_cost;  /* 0: no extra costs */
	sx_steps_fn steps;          /* 0: ask step for each neighbour */
	void *context;              /* passed to all callbacks */
} sx_map;

/* ---- rules --------------------------------------------------------------- */

#define SX_GRID_SQUARE      0
#define SX_GRID_HEX_ODD_R   1   /* pointy top, odd rows shifted right */
#define SX_GRID_HEX_EVEN_R  2   /* pointy top, even rows shifted right */
#define SX_GRID_HEX_ODD_Q   3   /* flat top, odd columns shifted down */
#define SX_GRID_HEX_EVEN_Q  4   /* flat top, even columns shifted down */

#define SX_FOUR_WAYS    1u  /* square: only straight moves */
#define SX_CUT_CORNERS  2u  /* square: diagonal past a blocked side neighbour */

typedef struct sx_rules {
	int32_t grid;               /* SX_GRID_* */
	uint32_t flags;             /* SX_FOUR_WAYS, SX_CUT_CORNERS (square only) */
	int32_t cost_straight;      /* > 0, e.g. 10 or 256; hex: every step */
	int32_t cost_diagonal;      /* square: > 0, e.g. 14 or 362; hex: ignored */
	int32_t node_capacity;      /* > 0: cells one search may touch, e.g. 32768 */
	int32_t smooth_max_extra;   /* smoothing may cross cells whose move_cost is
	                             * at most this (0: only cells without extra) */
} sx_rules;

/* ---- results ------------------------------------------------------------- */

#define SX_RUNNING      1   /* budget used up; call the _continue function */
#define SX_FOUND        2   /* the goal was reached */
#define SX_PARTIAL      3   /* goal unreachable; way to the closest cell */
#define SX_UNREACHABLE  4   /* goal unreachable, nothing closer than the start */
#define SX_DONE         5   /* area or flow field complete */

typedef struct sx_result {
	int32_t status;             /* SX_RUNNING ... SX_DONE */
	int32_t cells;              /* cells of the whole way (start included), of
	                             * the area or of the field, also when fewer
	                             * were written */
	int32_t cost;               /* cost of the way */
	int32_t expanded;           /* cells expanded since the search began */
	int32_t node_limit_hit;     /* 1: node_capacity was too small */
} sx_result;

/* How a way is written. */
#define SX_OUT_CELLS   0   /* every cell */
#define SX_OUT_TURNS   1   /* start, each cell where direction or level
                            * changes, last cell: straight between them */
#define SX_OUT_SMOOTH  2   /* start, corners of straight lines at any angle,
                            * last cell; lines only cross cells the walker can
                            * step into on the same level (see
                            * smooth_max_extra), so they never cut walls */

#define SX_ANY_LEVEL  (-1)  /* goal level: any */

#define SX_ERROR_ARGUMENT  (-1) /* a pointer is 0 or a number is out of range */
#define SX_ERROR_INTERNAL  (-2) /* a bug in the library; please report */

/* budget everywhere: cells to expand at most in this call; 0: no limit. */

uint32_t sx_api_version(void);

/* ---- sx_nav: paths and areas --------------------------------------------- */

typedef struct sx_nav sx_nav;

/* 0 if the rules are invalid or memory is short. */
sx_nav *sx_nav_create(const sx_rules *rules);
void sx_nav_destroy(sx_nav *nav);

/* Searches a way from start to the cheapest of goal_count goals (a goal
 * level may be SX_ANY_LEVEL) and writes it as output into points. Several
 * goals, e.g. every exit of a map, cost one search. If no goal can be
 * reached the way leads to the reachable cell closest to one (SX_PARTIAL;
 * with more than 16 goals that guess is lost). On SX_RUNNING the best way so
 * far is written; sx_nav_continue goes on. result may be 0. */
int32_t sx_nav_find_path(sx_nav *nav, const sx_map *map, sx_point start,
	const sx_point *goals, int32_t goal_count,
	int32_t budget, int32_t output, sx_point *points, int32_t max_points, sx_result *result);

/* A way away from threat_count threats: to the cell within max_cost of
 * start that the nearest threat needs the longest walk to reach (cells no
 * threat reaches count as farthest; ties: cheaper for the walker). SX_FOUND,
 * or SX_UNREACHABLE (only start written) if nothing is farther than start.
 * To flee off the map or to a refuge, use sx_nav_find_path with the exits
 * as goals. The first call reserves a second search of node_capacity. */
int32_t sx_nav_flee(sx_nav *nav, const sx_map *map, sx_point start,
	const sx_point *threats, int32_t threat_count, int32_t max_cost,
	int32_t output, sx_point *points, int32_t max_points, sx_result *result);

/* Every cell reachable from start at a cost of at most max_cost, with its
 * cost (costs may be 0). Afterwards sx_nav_path_to gives the way to each. */
int32_t sx_nav_reachable(sx_nav *nav, const sx_map *map, sx_point start, int32_t max_cost,
	int32_t budget, sx_point *cells, int32_t *costs, int32_t max_cells, sx_result *result);

/* Goes on with the last sx_nav_find_path or sx_nav_reachable after
 * SX_RUNNING and writes as that call did (costs only for areas, may be 0).
 * The map must not have changed since. */
int32_t sx_nav_continue(sx_nav *nav, const sx_map *map, int32_t budget,
	sx_point *points, int32_t *costs, int32_t max_points, sx_result *result);

/* Makes the way from the last search's start to cell the current way and
 * writes it. status SX_FOUND, or SX_UNREACHABLE (nothing written, current
 * way kept) if the search did not reach cell. */
int32_t sx_nav_path_to(sx_nav *nav, const sx_map *map, sx_point cell, int32_t output,
	sx_point *points, int32_t max_points, sx_result *result);

/* The current way once more, in another output (e.g. cells and smoothed). */
int32_t sx_nav_way(sx_nav *nav, const sx_map *map, int32_t output,
	sx_point *points, int32_t max_points);

/* Formation along the current way (the leader's). slots holds members pairs
 * (forward, right) relative to the leader's walking direction; slots 0 means
 * the 2 x 2 block and needs members == 3 (beside, behind, behind beside).
 * out is a table of (members + 1) rows of max_cells points: row 0 the
 * leader's cells, row k member k's target while the leader stands on the
 * cell in the same column. A slot that is blocked, taken or out of the
 * leader's sight falls back to the nearest free cell of the leader's trail,
 * so the group walks in a line through doors. Returns the columns written.
 * Hex: forward is the leader's hex direction, right 60 degrees clockwise of
 * it; (0, 1) front right, (-1, 0) behind, (-1, 1) back right. A hex wedge:
 * (0, -1), (-1, 1), (0, -2), (-2, 2) ... */
int32_t sx_nav_formation(sx_nav *nav, const sx_map *map, const int32_t *slots, int32_t members,
	sx_point *out, int32_t max_cells);

/* ---- sx_flow: many walkers, same goals ----------------------------------- */

typedef struct sx_flow sx_flow;

sx_flow *sx_flow_create(const sx_rules *rules);
void sx_flow_destroy(sx_flow *flow);

/* Searches outward from goal_count goals up to max_cost. result->cells
 * counts the cells in the field. Exact when moves cost the same both ways
 * (one-way moves and leave costs make costs approximate; next steps still
 * use only legal moves). Returns 0. */
int32_t sx_flow_build(sx_flow *flow, const sx_map *map, const sx_point *goals, int32_t goal_count,
	int32_t max_cost, sx_result *result);

/* For count walkers at from[i]: next[i] the next cell towards the nearest
 * goal (from[i] itself at a goal, outside the field or stuck) and costs[i]
 * the cost to that goal (-1 outside). next or costs may be 0. Returns how
 * many walkers got a next cell. */
int32_t sx_flow_next(sx_flow *flow, const sx_map *map, const sx_point *from, int32_t count,
	sx_point *next, int32_t *costs);

/* ---- sx_dstar: a way kept up to date (D* Lite) --------------------------- */

/* Worth it when the map changes often near a walker (doors, cars); else a
 * fresh sx_nav_find_path is just as good.
 * Use:  sx_dstar_begin -> sx_dstar_compute (writes the way); while walking
 *       sx_dstar_move_start after each cell; after a change
 *       sx_dstar_cells_changed (step already answers for the new map), then
 *       sx_dstar_compute again.
 * Needs: if a step from a to b is possible, so is the step from b to a
 *       (costs may differ). start_step is not used. */
typedef struct sx_dstar sx_dstar;

sx_dstar *sx_dstar_create(const sx_rules *rules);
void sx_dstar_destroy(sx_dstar *dstar);

/* Plans from start to goal (goal.level >= 0) and forgets earlier work.
 * Nothing is searched yet. Returns 0. */
int32_t sx_dstar_begin(sx_dstar *dstar, sx_point start, sx_point goal);

/* Works on the plan; on SX_FOUND writes the way from the walker to the goal
 * as output. SX_RUNNING: call again. SX_UNREACHABLE: no way. */
int32_t sx_dstar_compute(sx_dstar *dstar, const sx_map *map, int32_t budget, int32_t output,
	sx_point *points, int32_t max_points, sx_result *result);

/* The walker now stands on cell. Returns 0. */
int32_t sx_dstar_move_start(sx_dstar *dstar, sx_point cell);

/* count cells changed (blocked, opened, other cost). Returns how many known
 * cells were updated (the cells and their neighbours on every level). */
int32_t sx_dstar_cells_changed(sx_dstar *dstar, const sx_map *map, const sx_point *cells,
	int32_t count);

/* ---- tracing (since version 2) ------------------------------------------ */

/* Every step of a search, for debugging and logging. Off by default; off it
 * costs nothing. The callback runs inside the search: keep it short. */
#define SX_TRACE_STARTED   1   /* a search begins at cell */
#define SX_TRACE_EXPANDED  2   /* cell expanded: cost is final (D* Lite: for now) */
#define SX_TRACE_IMPROVED  3   /* cell reached more cheaply from parent */
#define SX_TRACE_PAUSED    4   /* budget used up */
#define SX_TRACE_FINISHED  5   /* search ended; found 1 if a goal was reached */

typedef struct sx_event {
	int32_t kind;               /* SX_TRACE_* */
	sx_point cell;
	sx_point parent;            /* SX_TRACE_IMPROVED */
	int32_t cost;               /* nav: from the start; flow, D* Lite: to the goal */
	int32_t estimate;           /* estimated cost still to go */
	int32_t expanded;           /* SX_TRACE_PAUSED, SX_TRACE_FINISHED */
	int32_t found;              /* SX_TRACE_FINISHED */
} sx_event;

typedef void (*sx_trace_fn)(void *context, const sx_event *event);

/* trace 0 turns tracing off. Return 0. */
int32_t sx_nav_set_trace(sx_nav *nav, sx_trace_fn trace, void *context);
int32_t sx_flow_set_trace(sx_flow *flow, sx_trace_fn trace, void *context);
int32_t sx_dstar_set_trace(sx_dstar *dstar, sx_trace_fn trace, void *context);

/* ---- decisions (since version 4) ---------------------------------------- */

/* Utility AI: score every pair of option and target, choose one. All
 * integer: 0..1 is 0..SX_ONE. Same inputs and seed give the same choice on
 * every machine. No global random numbers; the seed comes from the game.
 * Equal scores: the smaller candidate index wins. */
#define SX_ONE 65535u

/* t = (input - min) / (max - min) clamped to 0..1 (max <= min: 1 if
 * input >= max, else 0). */
#define SX_CURVE_LINEAR            0   /* t */
#define SX_CURVE_INVERSE           1   /* 1 - t */
#define SX_CURVE_QUADRATIC         2   /* t * t */
#define SX_CURVE_INVERSE_QUADRATIC 3   /* (1 - t) * (1 - t) */
#define SX_CURVE_STEP              4   /* 1 if input >= min */
#define SX_CURVE_INVERSE_STEP      5   /* 1 if input < min */
#define SX_CURVE_S_CURVE           6   /* 3t^2 - 2t^3: soft threshold (since 5) */
#define SX_CURVE_INVERSE_S_CURVE   7   /* 1 - s-curve */
#define SX_CURVE_ROOT              8   /* sqrt(t): early values already count */
#define SX_CURVE_CUBIC             9   /* t^3: only the top counts */

#define SX_PICK_BEST     0   /* highest score */
#define SX_PICK_WEIGHTED 1   /* among the top best, by chance in proportion to score */
#define SX_PICK_UNIFORM  2   /* among the top best, each equally likely */

typedef struct sx_decider sx_decider;

typedef struct sx_consideration {
	uint32_t input;             /* index into the candidate's row of inputs */
	uint32_t curve;             /* SX_CURVE_* */
	int32_t min, max;
} sx_consideration;

typedef struct sx_candidate {
	uint32_t option;            /* from sx_decider_add_option */
	uint32_t target;            /* the game's own number, carried along */
} sx_candidate;

typedef struct sx_choice {
	int32_t candidate;          /* index, -1 for none */
	uint32_t score;
} sx_choice;

sx_decider *sx_decider_new(void);
void sx_decider_free(sx_decider *decider);
/* weight: SX_ONE = 1.0, more allowed. Returns the option number (0, 1, ...). */
int32_t sx_decider_add_option(sx_decider *decider, uint32_t weight);
/* The factors of an option are multiplied (0 rules out), compensated for
 * their number, then times the weight. Returns 0. */
int32_t sx_decider_add_consideration(sx_decider *decider, uint32_t option,
                                     const sx_consideration *consideration);
/* inputs: one row of stride raw numbers per candidate. pick: SX_PICK_*,
 * top: how many of the best take part (0 all). scores (count, or 0) gets
 * every score. Returns 1 if chosen, 0 if every score is 0, -1 bad
 * arguments (also an option or input that does not exist). */
int32_t sx_decider_decide(sx_decider *decider, const sx_candidate *candidates,
                          int32_t count, const int32_t *inputs, int32_t stride,
                          uint32_t pick, uint32_t top, uint64_t seed,
                          uint32_t *scores, sx_choice *choice);

#define SX_DECIDE_FACTOR 1   /* one consideration of one candidate */
#define SX_DECIDE_SCORED 2   /* value: product before weight; score */
#define SX_DECIDE_CHOSEN 3

typedef struct sx_decide_event {
	int32_t kind;               /* SX_DECIDE_* */
	int32_t candidate;
	int32_t consideration;      /* SX_DECIDE_FACTOR, else -1 */
	int32_t input;              /* SX_DECIDE_FACTOR: raw input */
	uint32_t value;             /* factor, or product */
	uint32_t score;
} sx_decide_event;

/* ---- decisions, finer control (since version 5) ----------------------- */

/* How the factors of an option are put together. */
#define SX_COMBINE_PRODUCT        0   /* default: multiply, compensated for their number */
#define SX_COMBINE_GEOMETRIC_MEAN 1   /* 3 x 0.95 stays 0.95; a 0 still rules out */
#define SX_COMBINE_MINIMUM        2   /* the weakest factor decides */
#define SX_COMBINE_AVERAGE        3   /* factors make up for each other; 0 does not rule out */
#define SX_COMBINE_MEDIAN         4   /* single outliers do not count */

#define SX_APPLY_COMBINE  0   /* goes into the combine */
#define SX_APPLY_MULTIPLY 1   /* multiplied onto the result afterwards: a true factor */

/* pull: how much the factor counts; v becomes 1 - pull * (1 - v), at least 0.
 * SX_ONE as it is, less gentle, more strict, 0 off. */
int32_t sx_decider_add_weighted(sx_decider *decider, uint32_t option,
                                const sx_consideration *consideration,
                                uint32_t pull, uint32_t apply);
/* option -1: for every option (default SX_COMBINE_PRODUCT); otherwise that
 * option only, combine -1 to follow the decider again. */
int32_t sx_decider_set_combine(sx_decider *decider, int32_t option, int32_t combine);
/* Only candidates of the highest rank that still scores above 0 are chosen
 * from (e.g. 2 emergency, 1 combat, 0 idle). Default 0. */
int32_t sx_decider_set_rank(sx_decider *decider, uint32_t option, uint32_t rank);
/* The action going on now scores times (1 + bonus / SX_ONE); null for none. */
int32_t sx_decider_set_current(sx_decider *decider, const sx_candidate *current, uint32_t bonus);

/* ---- decisions: memory and cooldowns (since version 6) ----------------- */

/* Inputs read from the decider's memory instead of the row (put them in
 * sx_consideration.input). Never done: INT32_MAX. */
#define SX_INPUT_SINCE_OPTION 0xFFFFFFF0u   /* time since this option was done */
#define SX_INPUT_SINCE_PAIR   0xFFFFFFF1u   /* ... on this target */
#define SX_INPUT_COUNT_OPTION 0xFFFFFFF2u   /* how often within the window */
#define SX_INPUT_COUNT_PAIR   0xFFFFFFF3u   /* ... on this target */

/* Time is the game's own count (frames, turns); it may wrap around. */
int32_t sx_decider_set_now(sx_decider *decider, uint32_t now);
/* The game says what it really did; the last 256 actions are kept. */
int32_t sx_decider_record(sx_decider *decider, const sx_candidate *done, uint32_t now);
int32_t sx_decider_forget(sx_decider *decider);
/* Counts look back less than time (0: everything remembered). */
int32_t sx_decider_set_window(sx_decider *decider, uint32_t time);
/* After the option was done it scores 0 until time has passed (0: none). */
int32_t sx_decider_set_cooldown(sx_decider *decider, uint32_t option, uint32_t time);

typedef void (*sx_decide_trace_fn)(void *context, const sx_decide_event *event);
int32_t sx_decider_set_trace(sx_decider *decider, sx_decide_trace_fn trace, void *context);

/* ---- state machine (since version 6) ------------------------------------ */

/* States are numbers 0..states-1; conditions are bits the game sets every
 * tick. Transitions are checked in the order added (put aborts first); the
 * first match wins, then the state's timeout. At most one change per update.
 * A transition from any state (from -1) does not fire in its own target. */
typedef struct sx_machine sx_machine;

#define SX_REASON_TRANSITION 1
#define SX_REASON_TIMEOUT    2
#define SX_REASON_SET        3

typedef struct sx_change {
	int32_t from, to;
	int32_t reason;             /* SX_REASON_*, 0 if nothing changed */
	int32_t transition;         /* index, or -1 */
} sx_change;

/* Null if initial >= states. The clock starts at time 0 (see set_state). */
sx_machine *sx_machine_new(uint32_t states, uint32_t initial);
void sx_machine_free(sx_machine *machine);
/* After time in state, go to next (time 0: never). */
int32_t sx_machine_set_timeout(sx_machine *machine, uint32_t state, uint32_t time, uint32_t next);
/* Fires when all bits of when are set and all bits of unless are clear and
 * the state has run at least after. from -1: any state. Returns its index. */
int32_t sx_machine_add_transition(sx_machine *machine, int32_t from, uint32_t to,
                                  uint64_t when, uint64_t unless, uint32_t after);
/* Returns 1 if the state changed (change filled), 0 if not, -1 bad arguments. */
int32_t sx_machine_update(sx_machine *machine, uint32_t now, uint64_t flags, sx_change *change);
int32_t sx_machine_state(sx_machine *machine);
int32_t sx_machine_entered(sx_machine *machine, uint32_t *entered);
int32_t sx_machine_set_state(sx_machine *machine, uint32_t state, uint32_t now);

#ifdef __cplusplus
}
#endif

#endif
