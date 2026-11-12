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

#define SX_API_VERSION 7

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

/* ---- world memory ---------------------------------------------------------- */

/* What a character has seen, what it avoids and what it ignores. Time is the
 * game's own count (uint32_t); ages use wrapping subtraction, so a wrapped
 * clock still works. Places are x, y, z in the game's own units.
 *
 * Return values: functions that ask a question return 1 (yes) or 0 (no); the
 * others return 0 or a count (>= 0). Errors are negative (SX_ERROR_*): a
 * null handle, struct or required pointer.
 *
 * Handles (sx_sightings, sx_zones, sx_ignore) are made with a capacity and
 * reserve it at once; capacity 0 gives null. The value structs below
 * (sx_pursuit, sx_radio, sx_interval) are plain data owned by the game: start
 * them zeroed (= {0}), copy or save them as you like, and pass a pointer. */

typedef struct sx_sightings sx_sightings;
typedef struct sx_zones sx_zones;
typedef struct sx_ignore sx_ignore;

/* The last known place and time of one id. */
typedef struct sx_sighting {
	uint32_t id;
	int32_t pos[3];
	uint32_t last_seen;
	uint32_t data;              /* a number the game may attach (kind, flags, ...) */
} sx_sighting;

/* An area to stay out of, centred on pos. */
typedef struct sx_zone {
	int32_t pos[3];
	uint32_t since;             /* when it was added or renewed */
	uint32_t data;
	int32_t source[3];          /* what caused it; valid if has_source != 0 */
	uint8_t has_source;
} sx_zone;

/* Chasing one target. Start zeroed, then sx_pursuit_begin. */
typedef struct sx_pursuit {
	uint32_t target;
	int32_t last_pos[3];        /* last known place of the target */
	uint32_t unseen;            /* time spent without seeing it */
	int32_t best;               /* smallest distance reached so far */
	uint32_t since;             /* time that distance was reached */
} sx_pursuit;

/* A call for help. Start zeroed. */
typedef struct sx_radio {
	int32_t pos[3];             /* where the pending call was started */
	uint32_t started;
	uint32_t last_start;        /* valid if has_sent != 0 */
	uint8_t pending;            /* 1 while a call waits to be taken */
	uint8_t has_sent;
} sx_radio;

/* A timer for "every N ticks". Start zeroed (counts as due). */
typedef struct sx_interval {
	uint32_t last;
	uint8_t started;
} sx_interval;

/* A candidate for sx_nearest. */
typedef struct sx_nearby {
	uint32_t id;
	int32_t x, y;
} sx_nearby;

/* Where an agent searches around a centre (see sx_search_point). count is the
 * number of used entries of offsets and distances, at most 8. */
typedef struct sx_search_pattern {
	int32_t heading_step;       /* heading between neighbouring agents (256 = full turn) */
	int32_t first_distance;     /* distance of step 0 from the centre */
	uint32_t count;
	int32_t offsets[8];         /* turn of each later step */
	int32_t distances[8];       /* distance of each later step */
	int32_t min_x, min_y, max_x, max_y;  /* points are clamped to this box */
} sx_search_pattern;

/* -- sightings: a bounded list of last known places, in insertion order -- */

/* Null if capacity is 0. The whole capacity is reserved at once. */
sx_sightings *sx_sightings_new(uint32_t capacity);
void sx_sightings_free(sx_sightings *s);
/* id was seen at x, y, z at now. A known id is updated in place (its data is
 * kept). A new id is added; if the list is full, the one seen longest ago is
 * dropped. Returns 1 if one was dropped (its id goes to *evicted, which may
 * be null), else 0. */
int32_t sx_sightings_seen(sx_sightings *s, uint32_t id, int32_t x, int32_t y, int32_t z,
                          uint32_t now, uint32_t *evicted);
/* Renew the time of id without changing its place. Return 1, or 0 if unknown. */
int32_t sx_sightings_touch(sx_sightings *s, uint32_t id, uint32_t now);
/* Set the place of id, keeping its time and data. Return 1, or 0 if unknown. */
int32_t sx_sightings_set_pos(sx_sightings *s, uint32_t id, int32_t x, int32_t y, int32_t z);
/* Set the data of id. Return 1, or 0 if unknown. */
int32_t sx_sightings_set_data(sx_sightings *s, uint32_t id, uint32_t data);
/* Return 1 if id was there. */
int32_t sx_sightings_remove(sx_sightings *s, uint32_t id);
/* Fill *out. Return 1, or 0 if id is unknown. */
int32_t sx_sightings_get(sx_sightings *s, uint32_t id, sx_sighting *out);
/* Number of sightings. */
int32_t sx_sightings_count(sx_sightings *s);
/* Fill *out with the sighting in slot index (0 .. count - 1). Return 1, or 0
 * if there is none. */
int32_t sx_sightings_at(sx_sightings *s, uint32_t index, sx_sighting *out);
/* Forget every sighting at least max_age old. Returns how many were
 * forgotten; their ids, in slot order, go to removed (may be null), at most
 * removed_cap of them. */
int32_t sx_sightings_expire(sx_sightings *s, uint32_t now, uint32_t max_age,
                            uint32_t *removed, uint32_t removed_cap);
int32_t sx_sightings_clear(sx_sightings *s);

/* -- zones: areas to stay out of -- */

/* Zones are circles of the given radius (<= 0: nothing is ever inside) that
 * last lifetime ticks. Null if capacity is 0. The whole capacity is
 * reserved at once. */
sx_zones *sx_zones_new(uint32_t capacity, int32_t radius, uint32_t lifetime);
void sx_zones_free(sx_zones *z);
/* Add a zone at x, y, z. If that place is already inside a zone (the first in
 * slot order), the zone is renewed (centre, time, data, source replaced).
 * When full, the oldest zone goes and the new one is last. source: 3
 * integers, or null for none. Returns the index of the zone. */
int32_t sx_zones_add(sx_zones *zones, int32_t x, int32_t y, int32_t z, uint32_t now,
                     uint32_t data, const int32_t *source);
/* Remove zones whose age is at least lifetime. Returns how many. */
int32_t sx_zones_expire(sx_zones *z, uint32_t now);
/* Return 1 if x, y, z is inside a zone (index of the first one goes to
 * *index, which may be null), else 0. */
int32_t sx_zones_contains(sx_zones *zones, int32_t x, int32_t y, int32_t z, uint32_t *index);
/* Return 1 if cell is inside a zone that holds neither start nor goal. cell,
 * start and goal are 3 integers each (x, y, z). Use it as the map's "closed". */
int32_t sx_zones_blocks(sx_zones *z, const int32_t *cell, const int32_t *start,
                        const int32_t *goal);
int32_t sx_zones_count(sx_zones *z);
/* Remove all zones. Returns 0. */
int32_t sx_zones_clear(sx_zones *z);
/* Fill *out with the zone in slot index. Return 1, or 0 if there is none. */
int32_t sx_zones_at(sx_zones *z, uint32_t index, sx_zone *out);

/* -- ignore: ids to leave alone for a while -- */

/* Null if capacity is 0. The whole capacity is reserved at once. */
sx_ignore *sx_ignore_new(uint32_t capacity);
void sx_ignore_free(sx_ignore *i);
/* Ignore id from now. A present id is renewed; when full, the oldest entry
 * is replaced. Returns 0. */
int32_t sx_ignore_add(sx_ignore *i, uint32_t id, uint32_t now);
/* Return 1 if id was added less than duration ago. */
int32_t sx_ignore_ignores(sx_ignore *i, uint32_t id, uint32_t now, uint32_t duration);
/* Forget all ids. Returns 0. */
int32_t sx_ignore_clear(sx_ignore *i);

/* -- pursuit: where the target was last seen and whether the chase gets
 *    anywhere -- */

/* Start chasing target, seen at x, y, z, distance away. */
int32_t sx_pursuit_begin(sx_pursuit *p, uint32_t target, int32_t x, int32_t y, int32_t z,
                         int32_t distance, uint32_t now);
/* The target is in sight at x, y, z: remember it, unseen time starts over. */
int32_t sx_pursuit_seen(sx_pursuit *p, int32_t x, int32_t y, int32_t z);
/* The target was out of sight for another step of time (adds to unseen). */
int32_t sx_pursuit_missed(sx_pursuit *p, uint32_t step);
/* Measure progress from distance at now. */
int32_t sx_pursuit_reset_progress(sx_pursuit *p, int32_t distance, uint32_t now);
/* Return 1 if the target has been out of sight for at least limit. */
int32_t sx_pursuit_lost(sx_pursuit *p, uint32_t limit);
/* Call with the current distance, once per check: it updates the progress
 * mark. Getting at least min_gain closer than the
 * best so far counts as progress and restarts the check (returns 0). Return 1
 * if there has been no progress for at least limit ticks. */
int32_t sx_pursuit_stalled(sx_pursuit *p, int32_t distance, uint32_t now, int32_t min_gain,
                           uint32_t limit);
/* Forget the chase. */
int32_t sx_pursuit_clear(sx_pursuit *p);

/* -- radio: a call for help, delivered after a delay, with a cooldown -- */

/* Start a call at x, y, z. Return 1, or 0 if one is pending or the last one
 * was less than cooldown ago (the first call never waits). */
int32_t sx_radio_start(sx_radio *r, int32_t x, int32_t y, int32_t z, uint32_t now,
                       uint32_t cooldown);
/* Return 1 if a call is pending and at least delay has passed. */
int32_t sx_radio_due(sx_radio *r, uint32_t now, uint32_t delay);
/* Take the pending call: its place goes to xyz (3 integers). Return 1, or 0
 * if none is pending. Does not check the delay; pair it with sx_radio_due. */
int32_t sx_radio_take(sx_radio *r, int32_t *xyz);
/* Drop the pending call (the cooldown still counts). */
int32_t sx_radio_cancel(sx_radio *r);

/* -- interval: "every N ticks" -- */

/* Return 1 if never marked, or at least every has passed since the mark. */
int32_t sx_interval_due(sx_interval *i, uint32_t now, uint32_t every);
int32_t sx_interval_mark(sx_interval *i, uint32_t now);
/* Back to never marked. */
int32_t sx_interval_reset(sx_interval *i);

/* -- nearest and searching -- */

/* The ids of the k candidates nearest to px, py, nearest first, into out
 * (room for k). Distance is (dx/divisor)^2 + (dy/divisor)^2 with truncating
 * division; a divisor <= 0 counts as 1; ties keep the earlier candidate
 * first. Returns how many were written (<= min(k, n)). */
int32_t sx_nearest(const sx_nearby *candidates, uint32_t n, int32_t px, int32_t py,
                   uint32_t k, int32_t divisor, uint32_t *out);
/* Sine of a heading (256 = full turn), scaled by 256: -256 .. 256. */
int32_t sx_sin256(int32_t heading);
/* Search point `step` of `agent` around cx, cy, cz, without random numbers:
 * each agent has its own heading (agent * heading_step); step 0 is
 * first_distance away along it, step n (1-based) turns by offsets[n-1] and
 * goes distances[n-1] out. z is kept; x and y are clamped. pattern null: the
 * default (three points at 5, 9 and 12 times 256). out: 3 integers. Returns
 * 1, or 0 if step is past the last point; -1 if pattern->count > 8. */
int32_t sx_search_point(uint32_t agent, uint32_t step, int32_t cx, int32_t cy, int32_t cz,
                        const sx_search_pattern *pattern, int32_t *out);
/* Heading elapsed ticks into looking around from base: plus
 * offsets[elapsed / step_ticks], or plus nothing once past the end; result
 * 0 .. 255. offsets null (count is then ignored): look to one side, back, to the other side and back
 * (0, 32, 64, 32, 0, -32, -64, -32). A step_ticks of 0 counts as 1. */
int32_t sx_look_heading(int32_t base, uint32_t elapsed, uint32_t step_ticks,
                        const int32_t *offsets, uint32_t count);

#ifdef __cplusplus
}
#endif

#endif
