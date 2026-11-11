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

#define SX_API_VERSION 2

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

/* Searches a way from start to goal (goal.level may be SX_ANY_LEVEL) and
 * writes it as output into points. If the goal cannot be reached the way
 * leads to the reachable cell closest to it (SX_PARTIAL). On SX_RUNNING the
 * best way so far is written; sx_nav_continue goes on. result may be 0. */
int32_t sx_nav_find_path(sx_nav *nav, const sx_map *map, sx_point start, sx_point goal,
	int32_t budget, int32_t output, sx_point *points, int32_t max_points, sx_result *result);

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

#ifdef __cplusplus
}
#endif

#endif
