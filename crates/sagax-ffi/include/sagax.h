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
 * and, for games without their own map structure, a ready map:
 *   sx_grid   cells with costs, walls and levels; sx_grid_map gives its sx_map
 *
 * Conventions (the same in every part of this header):
 *   - Names: every handle has sx_<name>_new and sx_<name>_free.
 *   - Return values: a yes/no question returns 1 or 0; a count or an index
 *     returns it; a function that has nothing to report returns 0; a scalar
 *     answer is returned directly when it fits (int64_t for values that do
 *     not fit int32_t), else it goes to an out parameter; structs go to out
 *     parameters; a function that writes a list returns how many entries it
 *     wrote, and an optional uint32_t *total gives how many exist.
 *   - Errors are negative and only the SX_ERROR_* constants (a null pointer,
 *     a number out of range, ...). How a search ended is in
 *     sx_result.status, never in the return value.
 *   - Enum parameters (SX_GRID_*, SX_OUT_*, SX_METRIC_*, ...) are uint32_t.
 *   - Time: now comes right after the handle and the ids that name what the
 *     call addresses; payloads come after now; seed is the last input.
 *     Exceptions (see the guide): sx_alertness_stimulus, sx_progress_start and
 *     sx_progress_update keep their order, because moving now would swap two
 *     numbers of one type; the arrive_at calls sx_inbox_push, sx_squad_report,
 *     sx_squad_report_danger and sx_crew_report_danger take the payload first
 *     and then the tick it arrives (that is not the time of the call).
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

#define SX_API_VERSION 13

typedef struct sx_point {
	int32_t x, y, level;
} sx_point;

/* A position as three integers [x, y, z] (the z of a position is the level of
 * a cell). Fixed-size arrays of positions are int32_t[3]. These convert
 * between that and sx_point. Both return 0, or SX_ERROR_ARGUMENT for a null
 * pointer. out: 3 integers. */
int32_t sx_point_to_pos(sx_point point, int32_t *out);
int32_t sx_pos_to_point(const int32_t *pos, sx_point *out);

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

/* Hex converters: an offset position [x, y, z] of a hex grid (grid is one of
 * SX_GRID_HEX_*) to axial [q, r, z] and back; z is kept. pos and out: 3
 * integers each. Return 0, or SX_ERROR_ARGUMENT for a null pointer or a grid
 * that is not hex. */
int32_t sx_hex_to_axial(uint32_t grid, const int32_t *pos, int32_t *out);
int32_t sx_hex_from_axial(uint32_t grid, const int32_t *pos, int32_t *out);

#define SX_FOUR_WAYS    1u  /* square: only straight moves */
#define SX_CUT_CORNERS  2u  /* square: diagonal past a blocked side neighbour */

typedef struct sx_rules {
	uint32_t grid;              /* SX_GRID_* */
	uint32_t flags;             /* SX_FOUR_WAYS, SX_CUT_CORNERS (square only) */
	int32_t cost_straight;      /* > 0, e.g. 10 or 256; hex: every step */
	int32_t cost_diagonal;      /* square: > 0, e.g. 14 or 362; hex: ignored */
	int32_t node_capacity;      /* > 0: cells one search may touch, e.g. 32768 */
	int32_t smooth_max_extra;   /* smoothing may cross cells whose move_cost is
	                             * at most this (0: only cells without extra) */
} sx_rules;

/* Ready-made costs (since 9). There is no default: every game picks one. */
#define SX_PRESET_OCTILE    1u  /* straight 10, diagonal 14 (hex: every step 10) */
#define SX_PRESET_TABLETOP  2u  /* straight 1, diagonal 1 (hex: every step 1) */

/* Fills out with rules for grid (SX_GRID_*) and the costs of preset
 * (SX_PRESET_*; 0 is no preset): flags 0, node_capacity 32768,
 * smooth_max_extra 0. Returns 0, or SX_ERROR_ARGUMENT for an unknown grid or
 * preset or a null out. */
int32_t sx_rules_default(uint32_t grid, uint32_t preset, sx_rules *out);

/* ---- sx_grid: a ready map with levels (since 9) -------------------------- */

/* For games without their own map structure: width x height cells on each
 * of levels levels, all open (extra cost 0, no walls). sx_grid_map gives the
 * sx_map for every search; it is valid while the grid lives. A move keeps
 * its level unless a link (a stair, a ramp) sends it to another one. Square
 * and hex grids alike: on hex grids dx, dy is the hex neighbour's offset.
 * One search at a time per sx_grid: a search reads the grid through its map
 * and takes it as unchanged; do not share one sx_grid between threads while
 * searches run, and do not change it from a callback. */
typedef struct sx_grid sx_grid;

sx_grid *sx_grid_new(int32_t width, int32_t height, int32_t levels); /* 0 if a size is <= 0 */
void sx_grid_free(sx_grid *grid);
/* Extra cost of entering cell; cost < 0 blocks it. Returns 0, or
 * SX_ERROR_ARGUMENT for a null grid or a cell off the grid. */
int32_t sx_grid_set(sx_grid *grid, sx_point cell, int32_t cost);
/* A wall between cell and its neighbour dx, dy away on the same level, both
 * ways. Returns 0, or SX_ERROR_ARGUMENT for a null grid, a cell off the grid
 * or an offset that is no neighbour (none of the 8 square offsets). On a hex
 * grid pass only the six real neighbours of that layout (which offsets they
 * are depends on the row or column); a square diagonal that is no hex
 * neighbour is accepted but never walked, so it does nothing useful. */
int32_t sx_grid_add_wall(sx_grid *grid, sx_point cell, int32_t dx, int32_t dy);
/* A move from a toward b arrives on b's level, and back. Returns 1, or 0
 * (nothing changed) unless a and b are neighbours on the grid;
 * SX_ERROR_ARGUMENT for a null grid. On a hex grid a and b must be real hex
 * neighbours of that layout, as for sx_grid_add_wall. */
int32_t sx_grid_link(sx_grid *grid, sx_point a, sx_point b);
/* Writes the grid's map to out. Returns 0, or SX_ERROR_ARGUMENT for a null
 * pointer. */
int32_t sx_grid_map(sx_grid *grid, sx_map *out);

/* ---- results ------------------------------------------------------------- */

#define SX_IDLE         0   /* no search to go on with (continue before any search) */
#define SX_RUNNING      1   /* budget used up; call the _continue function */
#define SX_FOUND        2   /* the goal was reached */
#define SX_PARTIAL      3   /* goal unreachable; way to the closest cell */
#define SX_UNREACHABLE  4   /* goal unreachable, nothing closer than the start */
#define SX_DONE         5   /* area or flow field complete */

typedef struct sx_result {
	int32_t status;             /* SX_IDLE ... SX_DONE */
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
#define SX_ERROR_PLAN      (-3) /* a tactic reads a column beyond the row or has more
                                 * than 255 slots (since 8) */
#define SX_ERROR_NEEDS_RULES (-4) /* a plain call (sx_squad_plan, sx_squad_check,
                                 * sx_crew_tick) on a squad with custom parts: use the
                                 * _with call and its rules (since 10) */

/* budget everywhere: cells to expand at most in this call; 0 = unlimited. */

uint32_t sx_api_version(void);

/* ---- sx_nav: paths and areas --------------------------------------------- */

typedef struct sx_nav sx_nav;

/* 0 if the rules are invalid or memory is short. */
sx_nav *sx_nav_new(const sx_rules *rules);
void sx_nav_free(sx_nav *nav);

/* Searches a way from start to the cheapest of goal_count goals (a goal
 * level may be SX_ANY_LEVEL) and writes it as output into points. Several
 * goals, e.g. every exit of a map, cost one search. If no goal can be
 * reached the way leads to the reachable cell closest to one (SX_PARTIAL;
 * with more than 16 goals that guess is lost). On SX_RUNNING the best way so
 * far is written; sx_nav_continue goes on. result may be 0. */
int32_t sx_nav_find_path(sx_nav *nav, const sx_map *map, sx_point start,
	const sx_point *goals, int32_t goal_count,
	int32_t budget, uint32_t output, sx_point *points, int32_t max_points, sx_result *result);

/* A way away from threat_count threats: to the cell within max_cost of
 * start that the nearest threat needs the longest walk to reach (cells no
 * threat reaches count as farthest; ties: cheaper for the walker). SX_FOUND,
 * or SX_UNREACHABLE (only start written) if nothing is farther than start.
 * To flee off the map or to a refuge, use sx_nav_find_path with the exits
 * as goals. The first call reserves a second search of node_capacity. */
int32_t sx_nav_flee(sx_nav *nav, const sx_map *map, sx_point start,
	const sx_point *threats, int32_t threat_count, int32_t max_cost,
	uint32_t output, sx_point *points, int32_t max_points, sx_result *result);

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
int32_t sx_nav_path_to(sx_nav *nav, const sx_map *map, sx_point cell, uint32_t output,
	sx_point *points, int32_t max_points, sx_result *result);

/* The current way once more, in another output (e.g. cells and smoothed). */
int32_t sx_nav_way(sx_nav *nav, const sx_map *map, uint32_t output,
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

sx_flow *sx_flow_new(const sx_rules *rules);
void sx_flow_free(sx_flow *flow);

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

sx_dstar *sx_dstar_new(const sx_rules *rules);
void sx_dstar_free(sx_dstar *dstar);

/* Plans from start to goal (goal.level >= 0) and forgets earlier work.
 * Nothing is searched yet. Returns 0. */
int32_t sx_dstar_begin(sx_dstar *dstar, sx_point start, sx_point goal);

/* Works on the plan; on SX_FOUND writes the way from the walker to the goal
 * as output. SX_RUNNING: call again. SX_UNREACHABLE: no way. */
int32_t sx_dstar_compute(sx_dstar *dstar, const sx_map *map, int32_t budget, uint32_t output,
	sx_point *points, int32_t max_points, sx_result *result);

/* The walker now stands on cell. Returns 0. */
int32_t sx_dstar_move_start(sx_dstar *dstar, sx_point cell);

/* count cells changed (blocked, opened, other cost). Returns how many known
 * cells were updated (the cells and their neighbours on every level). */
int32_t sx_dstar_cells_changed(sx_dstar *dstar, const sx_map *map, const sx_point *cells,
	int32_t count);

/* ---- tracing (since version 2; this form since version 12) -------------- */

/* Why did a call do that? Every handle that decides something can carry
 * one trace callback (sx_<handle>_set_trace). It is off by default; off it
 * costs one check per call. On, the callback gets every record of the
 * following calls on that handle:
 *
 *  - It runs inside the call, synchronously, in record order. The record is
 *    valid only during the callback: copy what you keep.
 *  - The callback must not call any function on the same handle, on the
 *    handle that lent it (its parent) or on any handle it lent: they share
 *    the state the running call is changing. Lent handles are
 *    sx_squad_knowledge, sx_squad_areas and sx_squad_decider (parent: the
 *    squad), sx_crew_squad and sx_crew_keeper (parent: the crew) and
 *    sx_mission_decider (parent: the mission); a crew also counts its
 *    squad's lent handles as its own.
 *  - Records are flat structs: a kind first, then named fields; fields that
 *    do not apply to the kind are 0. Sub-records are embedded by value.
 *  - A trace never changes a result: every call answers the same with
 *    tracing on or off.
 *
 * set_trace with trace NULL turns tracing off. Returns 0, SX_ERROR_ARGUMENT
 * for a null handle. */

/* One step of a search: paths, areas, flow fields, D* Lite. */
#define SX_SEARCH_TRACE_STARTED   1   /* a search begins at cell */
#define SX_SEARCH_TRACE_EXPANDED  2   /* cell expanded: cost is final (D* Lite: for now) */
#define SX_SEARCH_TRACE_IMPROVED  3   /* cell reached more cheaply from parent */
#define SX_SEARCH_TRACE_PAUSED    4   /* budget used up */
#define SX_SEARCH_TRACE_FINISHED  5   /* search ended; found 1 if a goal was reached */

typedef struct sx_search_trace {
	uint32_t kind;              /* SX_SEARCH_TRACE_* */
	uint32_t search;            /* number of the search within its handle, from 1:
	                             * nav: every search (a flee runs two); flow: every
	                             * build; D* Lite: every compute */
	sx_point cell;              /* STARTED: the start; EXPANDED, IMPROVED: the cell */
	sx_point parent;            /* IMPROVED */
	int32_t cost;               /* EXPANDED, IMPROVED. nav: from the start; flow,
	                             * D* Lite: to the goal */
	int32_t estimate;           /* STARTED, EXPANDED: estimated cost still to go */
	int32_t expanded;           /* PAUSED, FINISHED: cells expanded so far */
	uint8_t found;              /* FINISHED */
	uint8_t first;              /* IMPROVED: 1 if the cell was new */
} sx_search_trace;

typedef void (*sx_search_trace_fn)(void *context, const sx_search_trace *trace);

/* What a navigator did: its searches (a flee runs two: the threats' first,
 * then the walker's, each with its own search number), the line tests of
 * SX_OUT_SMOOTH, the member placements of sx_nav_formation and the cell a
 * flee chose. Covers sx_nav_find_path, sx_nav_continue, sx_nav_reachable,
 * sx_nav_flee, sx_nav_path_to, sx_nav_way, sx_nav_formation and
 * sx_path_cost. */
#define SX_NAV_TRACE_SEARCH     1  /* search: one search step */
#define SX_NAV_TRACE_SHORTCUT   2  /* the straight line from -> to was tested; clear 1 if free */
#define SX_NAV_TRACE_PLACED     3  /* member stands on cell while the leader is on its
                                    * way's point step; slot 0: fell back to the trail */
#define SX_NAV_TRACE_UNPLACED   4  /* member found no free cell at step (waits on the leader) */
#define SX_NAV_TRACE_FLEE_BEST  5  /* the cell a flee chose: distance is the threats' walk
                                    * (INT32_MAX: none reaches it), cost the walker's */

typedef struct sx_nav_trace {
	uint32_t kind;              /* SX_NAV_TRACE_* */
	sx_search_trace search;     /* SEARCH */
	sx_point from;              /* SHORTCUT */
	sx_point to;                /* SHORTCUT */
	sx_point cell;              /* PLACED, FLEE_BEST */
	int32_t member;             /* PLACED, UNPLACED */
	int32_t step;               /* PLACED, UNPLACED */
	int32_t distance;           /* FLEE_BEST */
	int32_t cost;               /* FLEE_BEST */
	uint8_t clear;              /* SHORTCUT */
	uint8_t slot;               /* PLACED */
} sx_nav_trace;

typedef void (*sx_nav_trace_fn)(void *context, const sx_nav_trace *trace);

/* trace must not call any function on the same handle. */
int32_t sx_nav_set_trace(sx_nav *nav, sx_nav_trace_fn trace, void *context);
int32_t sx_flow_set_trace(sx_flow *flow, sx_search_trace_fn trace, void *context);
int32_t sx_dstar_set_trace(sx_dstar *dstar, sx_search_trace_fn trace, void *context);

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
 * every score. Returns 1 if chosen, 0 if every score is 0,
 * SX_ERROR_ARGUMENT for bad arguments (also an option or input that does
 * not exist). */
int32_t sx_decider_decide(sx_decider *decider, const sx_candidate *candidates,
                          int32_t count, const int32_t *inputs, int32_t stride,
                          uint32_t pick, uint32_t top, uint64_t seed,
                          uint32_t *scores, sx_choice *choice);

/* What a decision did (sx_decider_set_trace): per candidate its factors and
 * its score, then RANKED when the ranks are mixed, then the winner. Fields
 * that do not apply to the kind are 0. */
#define SX_DECIDE_TRACE_FACTOR 1   /* consideration of candidate read input, gave value */
#define SX_DECIDE_TRACE_SCORED 2   /* candidate scored score; product before the weight;
                                    * cooled 1 if a cooldown forced the score to 0 */
#define SX_DECIDE_TRACE_CHOSEN 3   /* candidate won with score */
#define SX_DECIDE_TRACE_RANKED 4   /* mixed ranks: only candidates of rank take part */

typedef struct sx_decide_trace {
	uint32_t kind;              /* SX_DECIDE_TRACE_* */
	int32_t candidate;          /* FACTOR, SCORED, CHOSEN */
	int32_t consideration;      /* FACTOR */
	int32_t input;              /* FACTOR: the raw input */
	uint32_t value;             /* FACTOR: the factor after its pull */
	uint32_t score;             /* SCORED, CHOSEN */
	uint32_t product;           /* SCORED */
	uint32_t rank;              /* RANKED */
	uint8_t cooled;             /* SCORED */
} sx_decide_trace;

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
/* option SX_ALL_OPTIONS: for every option (default SX_COMBINE_PRODUCT);
 * otherwise that option only, combine SX_COMBINE_FOLLOW to follow the decider
 * again. Both are the all-ones number, what -1 was before 0.9. */
#define SX_ALL_OPTIONS 0xFFFFFFFFu
#define SX_COMBINE_FOLLOW 0xFFFFFFFFu
int32_t sx_decider_set_combine(sx_decider *decider, uint32_t option, uint32_t combine);
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
int32_t sx_decider_record(sx_decider *decider, uint32_t now, const sx_candidate *done);
int32_t sx_decider_forget(sx_decider *decider);
/* Counts look back less than time (0: everything remembered). */
int32_t sx_decider_set_window(sx_decider *decider, uint32_t time);
/* After the option was done it scores 0 until time has passed (0: none). */
int32_t sx_decider_set_cooldown(sx_decider *decider, uint32_t option, uint32_t time);

/* trace gets every record of the following sx_decider_decide calls, with
 * context; NULL turns it off (the default). On a lent decider it covers the
 * calls made through that handle only. trace must not call any function on
 * the same decider or, for a lent one, on the squad, crew or mission that
 * lent it or their other lent handles. Returns 0. */
typedef void (*sx_decide_trace_fn)(void *context, const sx_decide_trace *trace);
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
/* Returns 1 if the state changed (change filled), 0 if not, SX_ERROR_ARGUMENT
 * for bad arguments. */
int32_t sx_machine_update(sx_machine *machine, uint32_t now, uint64_t flags, sx_change *change);
int32_t sx_machine_state(sx_machine *machine);
int32_t sx_machine_entered(sx_machine *machine, uint32_t *entered);
int32_t sx_machine_set_state(sx_machine *machine, uint32_t state, uint32_t now);

/* What an update did (since version 12): each transition of the current
 * state whose conditions hold but whose after has not passed yet (WAITING:
 * why the machine does not switch), checked before the one that fires, then
 * the change (CHANGED). Fields that do not apply to the kind are 0. */
#define SX_MACHINE_TRACE_CHANGED 1   /* change */
#define SX_MACHINE_TRACE_WAITING 2   /* transition: the state has run elapsed of after */

typedef struct sx_machine_trace {
	uint32_t kind;              /* SX_MACHINE_TRACE_* */
	sx_change change;           /* CHANGED */
	uint32_t transition;        /* WAITING: the transition's index */
	uint32_t elapsed;           /* WAITING */
	uint32_t after;             /* WAITING */
} sx_machine_trace;

/* trace gets every record of the following sx_machine_update calls, with
 * context; NULL turns it off (the default). trace must not call any function
 * on the same machine. Returns 0. */
typedef void (*sx_machine_trace_fn)(void *context, const sx_machine_trace *trace);
int32_t sx_machine_set_trace(sx_machine *machine, sx_machine_trace_fn trace, void *context);

/* ---- world memory ---------------------------------------------------------- */

/* What a character knows, for how long, and who it tells. Time is the game's
 * own count (uint32_t); ages use wrapping subtraction, so a wrapped clock
 * still works. Places are x, y, z in the game's own units. Everything is
 * integer only and deterministic.
 *
 * Return values: functions that ask a question return 1 (yes) or 0 (no); the
 * others return 0 or a count (>= 0). Errors are negative (SX_ERROR_*): a
 * null handle, struct or required pointer, or a number out of range.
 *
 * Handles (sx_knowledge, sx_outbox, sx_inbox, sx_areas, sx_timers) are made
 * with a capacity and reserve it at once; capacity 0 gives null. The value
 * structs below (sx_fact, sx_message, sx_area, sx_progress, sx_interval,
 * sx_alertness, ...) are plain data owned by the game: start them zeroed
 * (= {0}), copy or save them as you like, and pass a pointer. */

typedef struct sx_knowledge sx_knowledge;
typedef struct sx_outbox sx_outbox;
typedef struct sx_inbox sx_inbox;
typedef struct sx_areas sx_areas;
typedef struct sx_timers sx_timers;

/* Where a fact came from (sx_fact.source). The game may use other values. */
#define SX_SOURCE_OWN   0   /* seen by the character itself */
#define SX_SOURCE_HEARD 1
#define SX_SOURCE_TOLD  2

/* What to drop when a full knowledge table learns something new. */
#define SX_EVICT_OLDEST  0  /* the fact seen longest ago */
#define SX_EVICT_WEAKEST 1  /* the smallest strength; ties: the older one */

/* Returned by sx_outbox_post: SX_POSTED when accepted, else why not. */
#define SX_POSTED           0
#define SX_REFUSED_FULL     1
#define SX_REFUSED_COOLDOWN 2

/* Rules of sx_route. */
#define SX_ROUTE_NEAREST  0 /* the arg nearest candidates */
#define SX_ROUTE_IN_RANGE 1 /* all within distance arg */
#define SX_ROUTE_ALL      2 /* every candidate (arg is ignored) */
#define SX_ROUTE_TO       3 /* the candidate with id arg, if present */

/* Distance measures of sx_route (dx, dy are per axis, after the divisor). */
#define SX_METRIC_EUCLID    0 /* dx*dx + dy*dy (a squared distance) */
#define SX_METRIC_MANHATTAN 1 /* |dx| + |dy| */
#define SX_METRIC_CHEBYSHEV 2 /* max(|dx|, |dy|) */
#define SX_METRIC_HEX       3 /* (|dx| + |dy| + |dx + dy|) / 2, truncating */

/* Sine tables (256 = full turn of a heading). */
#define SX_SINE_FINE   0    /* 65536 = 1.0; 64 steps per quarter turn */
#define SX_SINE_COARSE 1    /* 256 = 1.0; 16 steps per quarter turn */

/* One thing a character knows: subject, kind (both the game's numbers) and
 * where and when it was seen. At most one fact per (subject, kind). */
typedef struct sx_fact {
	uint32_t subject;
	uint32_t kind;
	int32_t pos[3];
	uint32_t seen_at;
	uint32_t strength;          /* how sure or how important; see SX_EVICT_WEAKEST */
	uint32_t data;              /* a number the game may attach */
	uint8_t source;             /* SX_SOURCE_* */
} sx_fact;

/* A fact on its way from one character to another. */
typedef struct sx_message {
	uint32_t kind;
	uint32_t subject;
	int32_t pos[3];
	uint32_t seen_at;
	uint32_t strength;
	uint32_t data;
	uint32_t sender;            /* the game's id of the sender */
	uint8_t keep_if_sender_gone; /* 1: sx_outbox_sender_gone keeps it */
} sx_message;

/* A circle (centre pos, in the x and y plane) to stay out of or to pay extra
 * for. Set the fields, then sx_areas_add (it sets since). */
typedef struct sx_area {
	uint32_t kind;
	int32_t pos[3];
	int32_t radius;             /* <= 0: nothing is ever inside */
	uint32_t strength;          /* extra path cost (blocking == 0) */
	uint32_t since;             /* set by sx_areas_add */
	uint32_t lifetime;          /* ticks after since; see sx_areas_expire */
	uint32_t data;
	int32_t source[3];          /* what caused it; valid if has_source != 0 */
	uint8_t blocking;           /* 1: paths may not enter; 0: only cost */
	uint8_t has_source;
} sx_area;

/* Whether a chase still gets closer. Start zeroed, then sx_progress_start. */
typedef struct sx_progress {
	int32_t best;               /* smallest value reached so far */
	uint32_t since;             /* time that value was reached */
	uint8_t started;
} sx_progress;

/* A timer for "every N ticks". Start zeroed (counts as due). */
typedef struct sx_interval {
	uint32_t last;
	uint8_t started;
} sx_interval;

/* A value with levels (alarm, suspicion). Start zeroed; read it with
 * sx_alertness_value and sx_alertness_level, change it only through the
 * functions. */
typedef struct sx_alertness {
	uint32_t value;
	uint32_t last;              /* time of the last sx_alertness_update */
	uint32_t last_stimulus;
	uint8_t level;
} sx_alertness;

/* How an alertness behaves. Level n is reached when the value is at least
 * thresholds[n - 1]; it is left again when the value falls below that minus
 * hysteresis. The value goes up with stimuli (capped at max) and falls by
 * decay_per_tick per tick, starting hold ticks after the last stimulus.
 * count is the number of used thresholds (at most 8; more count as 8). */
typedef struct sx_alert_config {
	uint32_t max;
	uint32_t decay_per_tick;
	uint32_t hold;
	uint32_t hysteresis;
	uint32_t count;
	uint32_t thresholds[8];
} sx_alert_config;

/* A candidate for sx_route. */
typedef struct sx_nearby {
	uint32_t id;
	int32_t x, y;
} sx_nearby;

/* Where an agent looks around a centre (see sx_points_around). count is the
 * number of used entries of offsets and distances, at most 8. */
typedef struct sx_point_pattern {
	int32_t heading_step;       /* heading between neighbouring agents (256 = full turn) */
	int32_t first_distance;     /* distance of step 0 from the centre */
	uint32_t count;
	int32_t offsets[8];         /* turn of each later step */
	int32_t distances[8];       /* distance of each later step */
	uint32_t sine;              /* SX_SINE_*; the distances are in its units */
	int32_t min_x, min_y, max_x, max_y;  /* points are clamped to this box */
} sx_point_pattern;

/* -- knowledge: a bounded table of facts, in insertion order -- */

/* Null if capacity is 0 or evict is not SX_EVICT_*. The whole capacity is
 * reserved at once. */
sx_knowledge *sx_knowledge_new(uint32_t capacity, uint32_t evict);
void sx_knowledge_free(sx_knowledge *k);
/* Store fact (a known subject and kind is replaced, whatever its time). A
 * new one is added; if the table is full, the one chosen by the eviction rule
 * is dropped, measured at now. Returns how many facts were dropped (0 or 1;
 * a dropped one goes to *evicted, which may be null). */
int32_t sx_knowledge_learn(sx_knowledge *k, uint32_t now, const sx_fact *fact,
                           sx_fact *evicted);
/* Like learn, but a known fact is only replaced if fact is at least as fresh
 * (age at now no larger). Returns as learn. */
int32_t sx_knowledge_merge(sx_knowledge *k, uint32_t now, const sx_fact *fact,
                           sx_fact *evicted);
/* learn and merge for the fact a message tells, with the given source. */
int32_t sx_knowledge_learn_message(sx_knowledge *k, uint32_t now, const sx_message *msg,
                                   uint8_t source, sx_fact *evicted);
int32_t sx_knowledge_merge_message(sx_knowledge *k, uint32_t now, const sx_message *msg,
                                   uint8_t source, sx_fact *evicted);
/* Renew the time of a fact. Each of these returns 1, or 0 if unknown. */
int32_t sx_knowledge_touch(sx_knowledge *k, uint32_t subject, uint32_t kind, uint32_t now);
int32_t sx_knowledge_set_pos(sx_knowledge *k, uint32_t subject, uint32_t kind, int32_t x,
                             int32_t y, int32_t z);
int32_t sx_knowledge_set_data(sx_knowledge *k, uint32_t subject, uint32_t kind, uint32_t data);
int32_t sx_knowledge_set_strength(sx_knowledge *k, uint32_t subject, uint32_t kind,
                                  uint32_t strength);
/* Return 1 if it was there. */
int32_t sx_knowledge_remove(sx_knowledge *k, uint32_t subject, uint32_t kind);
/* Remove every fact about subject. Returns how many. */
int32_t sx_knowledge_remove_subject(sx_knowledge *k, uint32_t subject);
/* Fill *out. Return 1, or 0 if there is no such fact. */
int32_t sx_knowledge_get(sx_knowledge *k, uint32_t subject, uint32_t kind, sx_fact *out);
int32_t sx_knowledge_count(sx_knowledge *k);
/* Fill *out with the fact in slot index (0 .. count - 1). Return 1, or 0 if
 * there is none. */
int32_t sx_knowledge_at(sx_knowledge *k, uint32_t index, sx_fact *out);
/* Forget every fact at least max_age old. Returns how many were forgotten;
 * the facts, in slot order, go to removed (may be null), at most cap of them
 * (the rest are forgotten all the same). */
int32_t sx_knowledge_expire(sx_knowledge *k, uint32_t now, uint32_t max_age,
                            sx_fact *removed, uint32_t cap);
/* The same, but only for facts of this kind; the others stay. */
int32_t sx_knowledge_expire_kind(sx_knowledge *k, uint32_t now, uint32_t kind,
                                 uint32_t max_age, sx_fact *removed, uint32_t cap);
/* Forget everything. Returns 0. */
int32_t sx_knowledge_clear(sx_knowledge *k);

/* What learn, merge, their _message forms, the expire calls and the remove
 * calls did (since version 12): one record per fact concerned. A full table
 * sends EVICTED before the ADDED of the new fact. */
#define SX_KNOWLEDGE_TRACE_ADDED   1   /* a new subject and kind was stored */
#define SX_KNOWLEDGE_TRACE_UPDATED 2   /* a known one was replaced by fact */
#define SX_KNOWLEDGE_TRACE_STALE   3   /* merge kept the stored one; fact is the refused one */
#define SX_KNOWLEDGE_TRACE_EVICTED 4   /* dropped by the eviction rule to make room */
#define SX_KNOWLEDGE_TRACE_EXPIRED 5   /* dropped by an expire call */
#define SX_KNOWLEDGE_TRACE_REMOVED 6   /* forgotten by a remove call (or a squad's forget
                                        * report action) */

typedef struct sx_knowledge_trace {
	uint32_t kind;              /* SX_KNOWLEDGE_TRACE_* */
	sx_fact fact;               /* the fact concerned */
} sx_knowledge_trace;

/* trace gets every record of the following calls on k, with context; NULL
 * turns it off (the default). For sx_squad_knowledge it covers the calls
 * made through that handle only. trace must not call any function on k or,
 * for sx_squad_knowledge, on the squad (or crew) that lent it or their other
 * lent handles. Returns 0. */
typedef void (*sx_knowledge_trace_fn)(void *context, const sx_knowledge_trace *trace);
int32_t sx_knowledge_set_trace(sx_knowledge *k, sx_knowledge_trace_fn trace, void *context);

/* -- outbox and inbox: messages in flight -- */

/* Null if capacity is 0. The whole capacity is reserved at once. */
sx_outbox *sx_outbox_new(uint32_t capacity);
void sx_outbox_free(sx_outbox *o);
/* Post msg at now; it is due delay ticks later. Refused (nothing stored) if
 * the outbox is full or the last accepted post was less than cooldown ago.
 * Returns SX_POSTED, SX_REFUSED_FULL or SX_REFUSED_COOLDOWN. */
int32_t sx_outbox_post(sx_outbox *o, uint32_t now, const sx_message *msg, uint32_t delay,
                       uint32_t cooldown);
/* Move the messages whose delay has passed into out, oldest post first, at
 * most cap of them (out may be null if cap is 0); the rest stay pending.
 * Returns how many were moved. The messages are plain data: change them (a
 * fresh pos, say) before routing. */
int32_t sx_outbox_due(sx_outbox *o, uint32_t now, sx_message *out, uint32_t cap);
/* The sender is gone: drop every pending message without keep_if_sender_gone.
 * Returns 0. */
int32_t sx_outbox_sender_gone(sx_outbox *o);
int32_t sx_outbox_pending(sx_outbox *o);
int32_t sx_outbox_clear(sx_outbox *o);

/* Null if capacity is 0. The whole capacity is reserved at once. */
sx_inbox *sx_inbox_new(uint32_t capacity);
void sx_inbox_free(sx_inbox *b);
/* msg arrives at arrive_at. When full, the message that arrives last goes if
 * the new one arrives strictly earlier; otherwise the new one is refused.
 * Returns 1 if msg was stored, 0 if refused (a yes/no answer). */
int32_t sx_inbox_push(sx_inbox *b, const sx_message *msg, uint32_t arrive_at);
/* Move the messages whose arrival time has come into out, in push order, at
 * most cap of them; the rest stay. Returns how many were moved. */
int32_t sx_inbox_take_due(sx_inbox *b, uint32_t now, sx_message *out, uint32_t cap);
int32_t sx_inbox_count(sx_inbox *b);
int32_t sx_inbox_clear(sx_inbox *b);

/* What post, due, sender_gone, push and take_due did (since version 12): one
 * record per message concerned. A full inbox that makes room sends DROPPED
 * (the message arriving last) before the QUEUED of the new one; one that
 * refuses sends DROPPED of the new one. */
#define SX_MESSAGE_TRACE_POSTED    1   /* at: when it becomes due */
#define SX_MESSAGE_TRACE_REFUSED   2   /* refused: SX_REFUSED_* */
#define SX_MESSAGE_TRACE_QUEUED    3   /* at: when it arrives */
#define SX_MESSAGE_TRACE_DROPPED   4   /* lost: a full inbox or a gone sender */
#define SX_MESSAGE_TRACE_DELIVERED 5   /* handed out by due or take_due */

typedef struct sx_message_trace {
	uint32_t kind;              /* SX_MESSAGE_TRACE_* */
	sx_message msg;             /* the message concerned */
	uint32_t at;                /* POSTED, QUEUED */
	uint32_t refused;           /* REFUSED */
} sx_message_trace;

/* trace gets every record of the following calls on the outbox or inbox,
 * with context; NULL turns it off (the default). trace must not call any
 * function on the same outbox or inbox. Returns 0. */
typedef void (*sx_message_trace_fn)(void *context, const sx_message_trace *trace);
int32_t sx_outbox_set_trace(sx_outbox *o, sx_message_trace_fn trace, void *context);
int32_t sx_inbox_set_trace(sx_inbox *b, sx_message_trace_fn trace, void *context);
/* Arrival time of a message sent at now: now + base + distance * per_unit
 * (wrapping). */
uint32_t sx_arrival(uint32_t now, uint32_t base, uint32_t distance, uint32_t per_unit);

/* -- route: who gets a message -- */

/* Pick recipients among n candidates by the rule (SX_ROUTE_*), seen from
 * ox, oy: the ids go to out in the order given below and the count is
 * returned. out must have room for n ids. arg is k for SX_ROUTE_NEAREST (the
 * k nearest, nearest first, ties keep the earlier candidate first), the
 * distance r for SX_ROUTE_IN_RANGE (candidate order; for SX_METRIC_EUCLID
 * the squared distance is compared with r * r), the id for SX_ROUTE_TO, and
 * ignored for SX_ROUTE_ALL. Distances are measured per axis as
 * (dx / divisor, dy / divisor) with truncating division (SX_METRIC_HEX then
 * halves its sum, truncating, so an odd sum after a divisor loses a half); a
 * divisor <= 0 counts as 1. With n == 0 nothing is written and 0 is returned
 * (candidates and out may be null). Returns SX_ERROR_ARGUMENT for an unknown
 * rule or metric, a negative arg for NEAREST, or an arg outside 0 .. 2^32 - 1
 * for TO. */
int32_t sx_route(uint32_t rule, int64_t arg, int32_t ox, int32_t oy,
                 const sx_nearby *candidates, uint32_t n, uint32_t metric, int32_t divisor,
                 uint32_t *out);

/* -- areas: places to stay out of or pay extra for -- */

/* Null if capacity is 0. The whole capacity is reserved at once. */
sx_areas *sx_areas_new(uint32_t capacity);
void sx_areas_free(sx_areas *a);
/* Add area at now (its since is set to now). If its centre is already inside
 * an area of the same kind (the first in slot order), that area is replaced.
 * When full, the oldest area goes and the new one is last. Returns the index
 * of the area. */
int32_t sx_areas_add(sx_areas *a, uint32_t now, const sx_area *area);
/* Remove areas whose age is at least lifetime. Returns how many. */
int32_t sx_areas_expire(sx_areas *a, uint32_t now);
/* Return 1 if x, y, z is inside an area (index of the first one goes to
 * *index, which may be null), else 0. The z coordinate is ignored. */
int32_t sx_areas_contains(sx_areas *a, int32_t x, int32_t y, int32_t z, uint32_t *index);
/* The same, only for areas of this kind. */
int32_t sx_areas_contains_kind(sx_areas *a, int32_t x, int32_t y, int32_t z, uint32_t kind,
                               uint32_t *index);
/* Return 1 if cell is inside a blocking area that holds neither start nor
 * goal. cell, start and goal are 3 integers each. Use it as the map's
 * "closed". */
int32_t sx_areas_blocks(sx_areas *a, const int32_t *cell, const int32_t *start,
                        const int32_t *goal);
/* The extra cost of cell: the sum (saturating) of the strength of every
 * non-blocking area that holds cell but neither start nor goal. Returns it
 * (0 if there is none; a negative value is SX_ERROR_*). */
int64_t sx_areas_cost(sx_areas *a, const int32_t *cell, const int32_t *start,
                      const int32_t *goal);
/* The one way to feed danger areas to the path search: an sx_move_cost_fn
 * over areas: set map.move_cost = sx_areas_move_cost and
 * map.context = a pointer to this. Only the cell moved to is judged: a move
 * into a blocking area (one holding neither start nor goal) returns -1,
 * else the extra cost of sx_areas_cost, at most INT32_MAX - 1. A null
 * context or null areas give 0. The level is the z coordinate. If the map's
 * own callbacks need a context too, call this from your move_cost.
 * Units: the areas, start and goal must be in the map's cell coordinates
 * (x, y, level), as the cells passed here. Areas kept in finer world units
 * need a move_cost of your own that converts each cell first (for example
 * cell x to x * 256 + 128 for 256 units per cell) and calls sx_areas_cost. */
typedef struct sx_areas_cost_ctx {
	sx_areas *areas;
	int32_t start[3];
	int32_t goal[3];
} sx_areas_cost_ctx;
int32_t sx_areas_move_cost(void *context, int32_t x, int32_t y, int32_t level,
                           int32_t to_x, int32_t to_y, int32_t to_level);
int32_t sx_areas_count(sx_areas *a);
/* Fill *out with the area in slot index. Return 1, or 0 if there is none. */
int32_t sx_areas_at(sx_areas *a, uint32_t index, sx_area *out);
/* Remove all areas. Returns 0. */
int32_t sx_areas_clear(sx_areas *a);

/* What add and expire did (since version 12), with the area as stored. A
 * full set sends EVICTED before the ADDED of the new area. Fields that do
 * not apply to the kind are 0. */
#define SX_AREAS_TRACE_ADDED   1   /* appended at index */
#define SX_AREAS_TRACE_RENEWED 2   /* the area at index was replaced */
#define SX_AREAS_TRACE_EVICTED 3   /* the oldest, removed to make room */
#define SX_AREAS_TRACE_EXPIRED 4   /* its lifetime ran out */

typedef struct sx_areas_trace {
	uint32_t kind;              /* SX_AREAS_TRACE_* */
	uint32_t index;             /* ADDED, RENEWED */
	sx_area area;               /* the area concerned */
} sx_areas_trace;

/* trace gets every record of the following calls on a, with context; NULL
 * turns it off (the default). For sx_squad_areas it covers the calls made
 * through that handle only. trace must not call any function on a or, for
 * sx_squad_areas, on the squad (or crew) that lent it or their other lent
 * handles. Returns 0. */
typedef void (*sx_areas_trace_fn)(void *context, const sx_areas_trace *trace);
int32_t sx_areas_set_trace(sx_areas *a, sx_areas_trace_fn trace, void *context);

/* -- timers: ids that count as "on" for a while -- */

/* Null if capacity is 0. The whole capacity is reserved at once. */
sx_timers *sx_timers_new(uint32_t capacity);
void sx_timers_free(sx_timers *t);
/* Switch id on from now for duration ticks (a present id restarts). When
 * full, the timer with the least time left is replaced. Returns 0. */
int32_t sx_timers_set(sx_timers *t, uint32_t id, uint32_t now, uint32_t duration);
/* Return 1 if id has time left. */
int32_t sx_timers_active(sx_timers *t, uint32_t id, uint32_t now);
/* Returns the time left of id (0 if none; a negative value is SX_ERROR_*). */
int64_t sx_timers_left(sx_timers *t, uint32_t id, uint32_t now);
/* Return 1 if id was there. */
int32_t sx_timers_remove(sx_timers *t, uint32_t id);
int32_t sx_timers_count(sx_timers *t);
int32_t sx_timers_clear(sx_timers *t);

/* What set did (since version 12): one record per call. Fields that do not
 * apply to the kind are 0. */
#define SX_TIMERS_TRACE_STARTED  1   /* id was new */
#define SX_TIMERS_TRACE_RENEWED  2   /* id was present and starts again */
#define SX_TIMERS_TRACE_REPLACED 3   /* full: old, with old_left ticks left, gave way to id */

typedef struct sx_timers_trace {
	uint32_t kind;              /* SX_TIMERS_TRACE_* */
	uint32_t id;
	uint32_t duration;
	uint32_t old;               /* REPLACED */
	uint32_t old_left;          /* REPLACED */
} sx_timers_trace;

/* trace gets every record of the following sx_timers_set calls on t, with
 * context; NULL turns it off (the default). trace must not call any function
 * on t. Returns 0. */
typedef void (*sx_timers_trace_fn)(void *context, const sx_timers_trace *trace);
int32_t sx_timers_set_trace(sx_timers *t, sx_timers_trace_fn trace, void *context);

/* -- progress, interval, alertness: value structs -- */

/* Start measuring from value at now (lower is better, e.g. a distance). */
int32_t sx_progress_start(sx_progress *p, int32_t value, uint32_t now);
/* Call with the current value, once per check. Getting at least min_gain
 * below the best so far counts as progress: the mark is renewed and 1 is
 * returned, else 0 (also if not started). */
int32_t sx_progress_update(sx_progress *p, int32_t value, uint32_t now, int32_t min_gain);
/* Return 1 if started and there has been no progress for at least limit. */
int32_t sx_progress_stalled(sx_progress *p, uint32_t now, uint32_t limit);
/* Back to not started. Returns 0. */
int32_t sx_progress_reset(sx_progress *p);

/* Return 1 if never marked, or at least every has passed since the mark. */
int32_t sx_interval_due(sx_interval *i, uint32_t now, uint32_t every);
int32_t sx_interval_mark(sx_interval *i, uint32_t now);
/* Back to never marked. */
int32_t sx_interval_reset(sx_interval *i);

/* Let the value decay up to now (as sx_alertness_update does), add amount
 * (capped at cfg->max) and note the time of the stimulus. The level does not
 * change here; call sx_alertness_update for it. Returns 0. */
int32_t sx_alertness_stimulus(sx_alertness *a, uint32_t amount, uint32_t now,
                              const sx_alert_config *cfg);
/* Let the value decay up to now and find the level. It jumps straight to
 * the level the value reaches (no steps between). Returns 1 if the level
 * changed (the old and the new level go to levels[0] and levels[1]; levels
 * may be null, else it has room for 2), else 0. A level above cfg->count
 * is first cut back to cfg->count. Call it once per tick or
 * whenever the level is needed; the decay is by elapsed time. */
int32_t sx_alertness_update(sx_alertness *a, uint32_t now, const sx_alert_config *cfg,
                            int32_t *levels);
/* Returns the value (a negative value is SX_ERROR_*). */
int64_t sx_alertness_value(const sx_alertness *a);
/* The level as of the last update (0 .. cfg->count). */
int32_t sx_alertness_level(sx_alertness *a);

/* -- geometry: sine, points around a place, looking around -- */

/* Sine of a heading (256 = full turn; only the low 8 bits count): the fine
 * table scales to -65536 .. 65536, the coarse one to -256 .. 256. kind is
 * SX_SINE_*; any other value counts as SX_SINE_FINE. */
int32_t sx_sine(int32_t heading, uint32_t kind);
/* Point `step` of `agent` around cx, cy, cz, without random numbers: each
 * agent has its own heading (agent * heading_step); step 0 is first_distance
 * away along it, step n (1-based) turns by offsets[n-1] and goes
 * distances[n-1] out. z is kept; x and y are clamped to the pattern's box.
 * pattern null: the default (fine sine, three points at 5, 9 and 12 times
 * 256, box INT32_MIN .. INT32_MAX on both axes, so no clamping). The ring is
 * Euclidean: on an axial hex grid it is skewed, so convert to the game's
 * world coordinates first or snap the result to the nearest hex. out: 3
 * integers. Returns 1, or 0 if step is past the last point;
 * SX_ERROR_ARGUMENT if pattern->count > 8 or pattern->sine is not
 * SX_SINE_*. */
int32_t sx_points_around(uint32_t agent, uint32_t step, int32_t cx, int32_t cy, int32_t cz,
                         const sx_point_pattern *pattern, int32_t *out);
/* Heading elapsed ticks into looking around from base: plus
 * offsets[elapsed / step_ticks], or plus nothing once past the end; result
 * 0 .. 255. offsets null (count is then ignored): look to one side, back, to
 * the other side and back (0, 32, 64, 32, 0, -32, -64, -32). A step_ticks of
 * 0 counts as 1. */
int32_t sx_sweep_heading(int32_t base, uint32_t elapsed, uint32_t step_ticks,
                         const int32_t *offsets, uint32_t count);

/* ---- group tactics (since version 8) ------------------------------------ */

/* A squad leader with its own memory plans for a group: tactics as data
 * (slots, scoring, abort rules, a trigger) are scored per known target and
 * picked by skill; members get goals as orders that arrive over their
 * channel; the leader watches the plan, gives the signal and reads the
 * members' reports. Targets come only from the leader's own knowledge (what
 * it saw itself and what members reported), never from the game directly.
 * Integer only and deterministic; seeds come from the game. Ties: the lower
 * index or id wins. Time is the game's own count; ages wrap safely.
 *
 * Return values as for world memory: questions return 1 or 0, the others 0
 * or a count; errors are negative (SX_ERROR_*). Handles (sx_squad,
 * sx_tactic, sx_keeper) are made once and reused; a capacity of 0 gives
 * null. The value structs (sx_member, sx_spot, sx_slot, sx_goal, sx_order,
 * sx_plan, sx_skill_table, ...) are plain data owned by the game: start
 * them zeroed (= {0}). Flags in them are 0 or 1 (any non-zero counts as 1
 * when read). */

typedef struct sx_squad sx_squad;
typedef struct sx_tactic sx_tactic;
typedef struct sx_keeper sx_keeper;

/* sx_member.state. */
#define SX_MEMBER_ACTIVE    0   /* takes part */
#define SX_MEMBER_SUSPENDED 1   /* out for now (the game's orders first); stays in the group */
#define SX_MEMBER_GONE      2   /* dead or lost; counts in SX_IN_LOSSES while listed */

/* Columns of the scoring row every tactic is scored on (put them in
 * sx_consideration.input or sx_tactic_add_abort). The game's own columns
 * follow from sx_squad_extra_start (SX_IN_EXTRA with at most two roles; see
 * sx_squad_set_extra_columns). */
#define SX_IN_LOSSES      0   /* members marked gone and still listed (never reset by a new
                               * plan; sx_squad_remove_member stops counting one) */
#define SX_IN_OFF_PLAN    1   /* active members whose goal of the active plan broke */
#define SX_IN_TARGET_AGE  2   /* ticks since the target was seen; no target: INT32_MAX */
#define SX_IN_TARGET_DIST 3   /* group centre to target (Euclid: the root); no target: INT32_MAX */
#define SX_IN_ROLE_0      4   /* active members with the first mask of the role list */
#define SX_IN_ROLE_1      5   /* active members with the second mask of the role list */
#define SX_IN_RANGED      4   /* = SX_IN_ROLE_0: with the default list, SX_ROLE_RANGED */
#define SX_IN_AREA        5   /* = SX_IN_ROLE_1: with the default list, SX_ROLE_AREA */
#define SX_IN_PLAN_AGE    6   /* ticks since the active plan became active (its own row only) */
#define SX_IN_TARGET_FRIEND_DIST 7 /* target to the nearest other known target (Euclid: the
                                    * root); no target or no other target: -1 */
#define SX_IN_EXTRA       8   /* deprecated: the first column of the game's own is 8 only
                               * with at most two roles; use sx_squad_extra_start */
#define SX_MAX_ROLE_COUNTS 8  /* most entries of the role list */

/* Role bits the default role list counts; the others are the game's. */
#define SX_ROLE_RANGED 1
#define SX_ROLE_AREA   2

/* Spot tag bits the shipped tactics ask for; the others are the game's. */
#define SX_TAG_COVER  1
#define SX_TAG_HIGH   2
#define SX_TAG_HIDDEN 4
#define SX_TAG_CHOKE  8

/* Reports from members (sx_message.kind for sx_squad_report). Danger has its
 * own call, sx_squad_report_danger; a message of kind 3 is dropped. */
#define SX_REPORT_GOAL_BROKEN 1   /* sender: the member; data: the plan id of its goal */
#define SX_REPORT_ENEMY       2   /* subject, pos, seen_at, strength, data: a target
                                   * (merged into the leader's knowledge as SX_SOURCE_TOLD) */
#define SX_REPORT_ARRIVED     4   /* sender: the member; data: the plan id: it reached its
                                   * gather spot (as sx_squad_arrived, once it is read) */
#define SX_REPORT_GAME_FIRST 16   /* the first kind a game should use for its own reports (they
                                   * go to the report callback of sx_squad_rules); kinds below
                                   * may gain a meaning of their own later */

/* A member of a group. */
typedef struct sx_member {
	uint32_t id;                /* the game's id */
	int32_t pos[3];
	uint32_t roles;             /* role bits (the game's, plus SX_ROLE_*) */
	uint8_t skill;              /* the member's own skill, for sx_skill_table_hold_bonus_for */
	uint8_t state;              /* SX_MEMBER_* */
} sx_member;

/* A candidate place with tag bits (SX_TAG_* and the game's own). Spots are
 * always picked from the game's list, never invented. */
typedef struct sx_spot {
	int32_t pos[3];
	uint32_t tags;
} sx_spot;

/* A character that could join a group (see sx_gather). */
typedef struct sx_gather_candidate {
	uint32_t id;
	uint32_t faction;
	int32_t pos[3];
	uint8_t free;               /* 1: not in a group yet */
} sx_gather_candidate;

/* Pick up to max candidates of faction within radius of x, y (inclusive, in
 * metric SX_METRIC_*; for Euclid the squared distance is compared with
 * radius * radius), nearest first, ties to the lower id. free_only 1 skips
 * the candidates that are not free; a negative radius reaches nobody. Only
 * x and y count. The ids go to out (room for max). Returns how many;
 * SX_ERROR_ARGUMENT for an unknown metric or a null list with a count. */
int32_t sx_gather(int32_t x, int32_t y, uint32_t faction, int64_t radius, uint32_t metric,
                  uint32_t free_only, const sx_gather_candidate *candidates, uint32_t n,
                  uint32_t *out, uint32_t max);

/* -- tactics: a plan as data -- */

/* Shapes of a formation (sx_squad_add_formation). */
#define SX_SHAPE_LINE   0   /* side by side */
#define SX_SHAPE_WEDGE  1   /* as a wedge */
#define SX_SHAPE_COLUMN 2   /* one behind the other */

/* Where a slot's members go (sx_place.kind). Wish points face the target:
 * the heading runs from the group centre to the target. */
#define SX_PLACE_NONE    0   /* no place (only for sx_slot.gather: no gather phase) */
#define SX_PLACE_OFFSET  1   /* x, y, z from the leader in the group's frame (+x toward the
                              * target, +y to its left), turned by the heading; z unturned */
#define SX_PLACE_SIDE    2   /* value SX_SIDE_* of the target, sx_slot.dist away from it */
#define SX_PLACE_BETWEEN 3   /* halfway between the target and its nearest other target;
                              * alone: like SX_SIDE_FRONT */
#define SX_PLACE_TAGGED  4   /* a spot with the tag bits value, near the group centre */
#define SX_PLACE_HERE    5   /* the leader's position */
#define SX_PLACE_CUSTOM  6   /* the game's own: value is its id, the place callback of
                              * sx_squad_rules decides (without it: the leader's position) */

/* Sides of the target, seen from the group (left with +y pointing up). */
#define SX_SIDE_FRONT 0
#define SX_SIDE_LEFT  1
#define SX_SIDE_RIGHT 2
#define SX_SIDE_REAR  3

/* What a member is told to do (sx_slot.kind, sx_goal.kind). */
#define SX_GOAL_MOVE    0
#define SX_GOAL_HOLD    1   /* go there and stay */
#define SX_GOAL_ATTACK  2   /* attack the target, from the spot if any */
#define SX_GOAL_WAIT    3
#define SX_GOAL_RETREAT 4
#define SX_GOAL_CUSTOM  5   /* the game's own kind, in custom */

/* When a goal starts (sx_slot.start, sx_goal.start). */
#define SX_START_NOW         0
#define SX_START_AFTER_TICKS 1   /* start_ticks after the order arrives */
#define SX_START_ON_SIGNAL   2   /* when the group signal is given */

typedef struct sx_place {
	uint32_t kind;              /* SX_PLACE_* */
	int32_t x, y, z;            /* SX_PLACE_OFFSET */
	uint32_t value;             /* SX_SIDE_* for SX_PLACE_SIDE, tag bits for SX_PLACE_TAGGED */
} sx_place;

/* One role in a tactic, filled by min .. max members. */
typedef struct sx_slot {
	uint32_t roles;             /* bits a member must all have; 0 takes anyone */
	uint8_t min, max;
	sx_place place;             /* where they go for their goal */
	sx_place gather;            /* where they gather first, or SX_PLACE_NONE */
	uint32_t kind;              /* SX_GOAL_* */
	uint32_t custom;            /* SX_GOAL_CUSTOM: the game's kind */
	uint32_t start;             /* SX_START_* */
	uint32_t start_ticks;       /* SX_START_AFTER_TICKS */
	uint32_t want_tags;         /* tag bits a spot should have; 0 takes any */
	int32_t dist;               /* from the target, for SX_PLACE_SIDE and SX_PLACE_BETWEEN of
	                             * place; spots count within 3 * dist (at least 1) of the wish
	                             * point unless sx_squad_set_spot_radius says otherwise */
	int32_t gather_dist;        /* like dist, for gather (the shipped flank tactic: 2 * its dist) */
} sx_slot;

/* A new, empty tactic: weight SX_ONE = 1.0; squads below min_skill (0 ..
 * 255) do not know it; needs_target 1: scored once per known target, 0:
 * once without one. No trigger (sx_tactic_trigger_none). Null if min_skill > 255. */
sx_tactic *sx_tactic_new(uint32_t weight, uint32_t min_skill, uint32_t needs_target);
/* Only for a tactic not given to sx_squad_add_tactic. */
void sx_tactic_free(sx_tactic *tactic);
/* Slots are filled in the order added. Returns the slot's index;
 * SX_ERROR_ARGUMENT for an unknown place, side, kind or start, or a place of
 * SX_PLACE_NONE. */
int32_t sx_tactic_add_slot(sx_tactic *tactic, const sx_slot *slot);
/* A factor over the scoring row (see SX_IN_*); the factors are multiplied.
 * Returns 0; SX_ERROR_ARGUMENT for an unknown curve. */
int32_t sx_tactic_add_consideration(sx_tactic *tactic, const sx_consideration *consideration);
/* The plan ends (a replan) when row[input] >= at_least; a column beyond the
 * row never fires. Returns 0. */
int32_t sx_tactic_add_abort(sx_tactic *tactic, uint32_t input, int32_t at_least);
/* Comparisons of an abort rule (sx_tactic_add_abort_when). */
#define SX_CMP_AT_LEAST  0   /* value >= limit */
#define SX_CMP_AT_MOST   1   /* value <= limit */
#define SX_CMP_EQUAL     2   /* value == limit */
#define SX_CMP_NOT_EQUAL 3   /* value != limit */
/* The plan ends when row[input] compares to value as cmp (SX_CMP_*); a column
 * beyond the row never fires. Returns 0; SX_ERROR_ARGUMENT for an unknown
 * cmp. */
int32_t sx_tactic_add_abort_when(sx_tactic *tactic, uint32_t input, uint32_t cmp, int32_t value);
/* The plan ends when the abort callback of sx_squad_rules says so for id (the
 * game's own number); without rules it never fires. A squad with such a
 * tactic needs rules (SX_ERROR_NEEDS_RULES). Returns 0. */
int32_t sx_tactic_add_abort_custom(sx_tactic *tactic, uint32_t id);
/* Whether the plan ends when its target is gone from the squad's knowledge
 * (on != 0, the default of sx_tactic_new and the shipped tactics). Off: the
 * plan is judged with the target's facts missing, like a plan without a
 * target. Returns 0. */
int32_t sx_tactic_set_abort_if_target_lost(sx_tactic *tactic, uint32_t on);
/* When the signal fires by itself. Once the signal was given, only the
 * tactic's abort rules end the plan. Each returns 0. */
/* Never (the default). */
int32_t sx_tactic_trigger_none(sx_tactic *tactic);
/* Every assigned active member arrived at its gather spot (sx_squad_arrived
 * or SX_REPORT_ARRIVED), or max_wait ticks after activation. */
int32_t sx_tactic_trigger_all_arrived(sx_tactic *tactic, uint32_t max_wait);
/* The target is within radius of the group centre (Euclid: squared distance
 * against radius * radius; a negative radius never fires). */
int32_t sx_tactic_trigger_target_in_radius(sx_tactic *tactic, int64_t radius);
/* Only by sx_squad_signal. */
int32_t sx_tactic_trigger_manual(sx_tactic *tactic);
/* When the trigger callback of sx_squad_rules says so for id (the game's own
 * number); without rules never. A squad with such a tactic needs rules
 * (SX_ERROR_NEEDS_RULES). Returns 0. */
int32_t sx_tactic_trigger_custom(sx_tactic *tactic, uint32_t id);

/* -- skill: how well a squad plans and acts -- */

/* What a squad's skill changes, in rows 0 .. rows - 1 (1 to 8 rows): a
 * skill (0 .. 255) takes row skill * rows / 256 (0 weak, the last row
 * elite). Entries beyond rows are unused. Times in the game's ticks. */
typedef struct sx_skill_table {
	uint32_t rows;              /* 1 .. 8 */
	uint32_t pick[8];           /* SX_PICK_*: how the tactic is chosen */
	uint32_t pick_top[8];       /* how many of the best take part (0 all) */
	uint32_t think_ticks[8];    /* from making a plan until it is active */
	uint32_t timing_spread[8];  /* most ticks a member starts late (by seed) */
	uint32_t hold_bonus[8];     /* SX_ONE = 1.0: a new plan must score more than the active
	                             * one times 1 + bonus; also the members' discipline */
	uint32_t give_up_ticks[8];  /* how long a member may postpone its goal */
	uint8_t spot_rank_error[8]; /* most ranks a spot may be off the best (by seed) */
} sx_skill_table;

/* The default table: 4 rows (the other four copy the last). Weak squads
 * pick by chance among the top 3, think 34 ticks, miss spots by up to 2
 * ranks and start up to 17 ticks apart; elite squads pick the best, think 5
 * ticks and start together. Returns 0. */
int32_t sx_skill_table_default(sx_skill_table *out);
/* One entry of the table at the level of a member's own skill (0 .. 255,
 * row skill * rows / 256); table null: the default. Return it, or
 * SX_ERROR_ARGUMENT (for a skill above 255, rows 0 or above 8, or a table
 * with a bad pick). hold_bonus is also the
 * member's discipline (SX_ONE = 1.0). */
int64_t sx_skill_table_think_ticks_for(const sx_skill_table *table, uint32_t skill);
int64_t sx_skill_table_timing_spread_for(const sx_skill_table *table, uint32_t skill);
int64_t sx_skill_table_hold_bonus_for(const sx_skill_table *table, uint32_t skill);
int64_t sx_skill_table_give_up_ticks_for(const sx_skill_table *table, uint32_t skill);
int32_t sx_skill_table_spot_rank_error_for(const sx_skill_table *table, uint32_t skill);
/* The pick at that level: SX_PICK_* to *pick, its top to *top. Returns 0, or
 * SX_ERROR_ARGUMENT. */
int32_t sx_skill_table_pick_for(const sx_skill_table *table, uint32_t skill,
                                uint32_t *pick, uint32_t *top);

/* -- squad: the leader, its group, memory and plans -- */

/* What sx_squad_check found. */
#define SX_SQUAD_KEEP    0   /* go on (also while a new plan is being thought out) */
#define SX_SQUAD_REPLAN  1   /* a new plan was made (pending); its id goes to *plan */
#define SX_SQUAD_SIGNAL  2   /* the active plan's trigger gave the signal just now */
#define SX_SQUAD_NO_PLAN 3   /* no plan is active and none could be made */

/* A goal for one member: in sx_squad_orders_due, and the goals of a plan.
 * pos: go there now; kind and start: what to do there and when. A slot
 * with a gather place first gets SX_GOAL_MOVE, SX_START_NOW to its gather
 * spot (with then_pos), and the slot's own kind on the signal. */
typedef struct sx_goal {
	uint32_t kind;              /* SX_GOAL_* */
	uint32_t custom;            /* SX_GOAL_CUSTOM: the game's kind */
	int32_t pos[3];             /* where; valid if has_pos (plans always set it) */
	int32_t then_pos[3];        /* after the gather spot pos, on the signal; valid if has_then_pos */
	uint32_t target;            /* the target's id; valid if has_target */
	uint32_t start;             /* SX_START_* */
	uint32_t start_ticks;       /* SX_START_AFTER_TICKS */
	uint32_t slot;              /* index of the slot in the tactic */
	uint32_t plan;              /* the plan's id */
	uint32_t issued_at;
	uint32_t start_delay;       /* ticks the member starts late (skill and seed) */
	uint8_t has_pos;
	uint8_t unchecked;          /* 1: no free spot, pos is the plain wish point */
	uint8_t has_then_pos;
	uint8_t then_unchecked;     /* like unchecked, for then_pos */
	uint8_t has_target;
} sx_goal;

typedef struct sx_order {
	uint32_t member;            /* the member's id */
	sx_goal goal;
} sx_order;

typedef struct sx_plan {
	uint32_t id;                /* each new plan gets a higher one (wrapping) */
	uint32_t tactic;            /* index from sx_squad_add_* / sx_squad_add_tactic */
	uint32_t target;            /* valid if has_target */
	uint32_t made_at;
	uint32_t active_at;         /* made_at + think_ticks of the skill */
	uint32_t goal_count;        /* goals in the plan, also when fewer were written */
	uint8_t has_target;
} sx_plan;

/* Capacities and metric of a squad. */
typedef struct sx_squad_config {
	uint32_t members;           /* most members */
	uint32_t memory;            /* most facts (the oldest goes when full) */
	uint32_t areas;             /* most danger areas */
	uint32_t inbox;             /* most waiting member reports */
	uint32_t dangers;           /* most waiting danger reports */
	uint32_t metric;            /* SX_METRIC_* */
	uint32_t role_count;        /* entries of role_masks in use, at most SX_MAX_ROLE_COUNTS */
	uint32_t role_masks[8];     /* the role list: one scoring column counts the active
	                             * members that have all bits of a mask. Entries 0 and 1
	                             * are columns SX_IN_ROLE_0 and SX_IN_ROLE_1, entry i >= 2
	                             * is column 8 + i - 2. Fewer than two: the free columns
	                             * read 0. */
	uint32_t rule_columns;      /* columns the columns callback of sx_squad_rules fills, after
	                             * the extra columns (sx_squad_set_extra_columns): column
	                             * sx_squad_extra_start + extra columns on; at most 1024 (with
	                             * the extra columns). Above 0 the squad needs rules. */
} sx_squad_config;

/* Fill *out with the defaults: 8 members, 32 facts, 8 areas, 16 inbox
 * reports, 8 danger reports, SX_METRIC_EUCLID, two roles {SX_ROLE_RANGED,
 * SX_ROLE_AREA}, no rule columns. Returns 0; SX_ERROR_ARGUMENT
 * for a null out. */
int32_t sx_squad_config_default(sx_squad_config *out);
/* A squad as configured. Skill 0, the default skill table, target kind 0,
 * no tactics. Null if config is null, a capacity is 0, the metric is
 * unknown, role_count is above 8 or rule_columns (with the extra columns)
 * is above 1024. */
sx_squad *sx_squad_new(const sx_squad_config *config);
void sx_squad_free(sx_squad *squad);

/* Add a member at the end. Returns its index; SX_ERROR_ARGUMENT if the group
 * is full, the id is already there or the state is unknown. (For every
 * sx_squad_* function: SX_ERROR_ARGUMENT for a null squad or a null required
 * pointer.) */
int32_t sx_squad_add_member(sx_squad *squad, const sx_member *member);
/* Each of these returns 1, or 0 if the id is unknown. remove keeps the
 * order of the others; suspend takes a member out of play for now (no
 * goal, not off plan; slot assignment skips it) and reinstate puts it back
 * (also a gone one) and, if it was suspended, sends it its goal of the
 * active plan at now over its channel: before the signal the plan's goal,
 * after it the signal order (its start not lined up any more); set_gone marks it
 * dead or lost (when it led, the next active member in slot order takes
 * over; the plan stays and the loss counts in SX_IN_LOSSES). */
int32_t sx_squad_remove_member(sx_squad *squad, uint32_t id);
int32_t sx_squad_set_pos(sx_squad *squad, uint32_t id, int32_t x, int32_t y, int32_t z);
int32_t sx_squad_suspend(sx_squad *squad, uint32_t id);
int32_t sx_squad_reinstate(sx_squad *squad, uint32_t id, uint32_t now);
int32_t sx_squad_set_gone(sx_squad *squad, uint32_t id);
/* Returns how many members are listed (gone ones too). */
int32_t sx_squad_member_count(sx_squad *squad);
/* Fill *out with the member at index. Return 1, or 0 if there is none. */
int32_t sx_squad_member_at(sx_squad *squad, uint32_t index, sx_member *out);
/* Returns 1, or 0 if the id is not in the group. */
int32_t sx_squad_set_leader(sx_squad *squad, uint32_t id);
/* The acting leader goes to *id (may be null): the set one while active,
 * else the first active member of the active plan in slot order, else the
 * first active member. Returns 1, or 0 if there is none. Orders are timed
 * from its position. */
int32_t sx_squad_leader(sx_squad *squad, uint32_t *id);
/* 0 weak .. 255 elite (default 0). Returns 0; SX_ERROR_ARGUMENT if skill > 255. */
int32_t sx_squad_set_skill(sx_squad *squad, uint32_t skill);
/* Returns the skill (0 .. 255). */
int32_t sx_squad_skill(sx_squad *squad);
/* Returns 0; SX_ERROR_ARGUMENT for an unknown pick or rows 0 or above 8 (the
 * table is then not changed). */
int32_t sx_squad_set_skill_table(sx_squad *squad, const sx_skill_table *table);
/* Fill *out with the squad's table. Returns 0; SX_ERROR_ARGUMENT for a null
 * out. */
int32_t sx_squad_skill_table(sx_squad *squad, sx_skill_table *out);

/* The decider that scores the squad's tactics: option i is tactic i. Lent:
 * sx_decider_free does nothing to it, it is valid until sx_squad_free (a
 * crew's squad: until sx_crew_free), and the same handle comes back each
 * time. Its combine, rank, cooldown and current action are free to change
 * (its own sx_decider_* calls); adding options or considerations is not:
 * sx_squad_validate, plan and check then return SX_ERROR_PLAN with tactic
 * and input UINT32_MAX when the option count no longer matches the tactics.
 * Null for a null squad. */
sx_decider *sx_squad_decider(sx_squad *squad);

/* What the squad's calls did (since version 12): sx_squad_observe,
 * sx_squad_report (INBOX: what the leader's inbox did, as sx_inbox_push),
 * sx_squad_plan, sx_squad_check, sx_squad_orders_due, sx_squad_signal,
 * sx_squad_reinstate and their _with forms. A plan or check sends, in order:
 * the orders that go out (DELAY), the reports it reads (REPORT, then what
 * they did to knowledge and areas), the expired danger areas (AREAS), the
 * targets slot's answer per known target (TARGET), the abort rules of the
 * active row (check) and of each candidate (ABORT), the decider records of each
 * candidate (DECIDE: FACTOR and SCORED; candidate is the index in the
 * candidate list, tactic order, then target order), then PICKED or NO_PICK,
 * the slot assignment (ASSIGNED), the spot ranking of each member that
 * wants a spot (SPOT_RANKED best first, then SPOT_PICKED) and THINKING.
 * Fields that do not apply to kind are 0. */
#define SX_SQUAD_TRACE_TARGET      1   /* fact: a known target; kept 1 if the targets slot kept it */
#define SX_SQUAD_TRACE_DECIDE      2   /* decide: a decider record of tactic (and target) */
#define SX_SQUAD_TRACE_ABORT       3   /* abort rule abort of tactic (and target): fired 1 or 0 */
#define SX_SQUAD_TRACE_PICKED      4   /* the new plan's tactic (and target) with score */
#define SX_SQUAD_TRACE_NO_PICK     5   /* every candidate scored 0 */
#define SX_SQUAD_TRACE_THINKING    6   /* plan becomes active at until */
#define SX_SQUAD_TRACE_ASSIGNED    7   /* member takes slot at cost (saturated) */
#define SX_SQUAD_TRACE_SPOT_RANKED 8   /* member's spot: fits 1 if it has every wanted tag;
                                        * distance from the wish point (saturated) */
#define SX_SQUAD_TRACE_SPOT_PICKED 9   /* member goes to spot (has_spot 1) or to the
                                        * unchecked wish point pos (has_spot 0) */
#define SX_SQUAD_TRACE_TRIGGER     10  /* the active plan's trigger: fired 1 or 0 */
#define SX_SQUAD_TRACE_REPORT      11  /* msg: a report the leader reads */
#define SX_SQUAD_TRACE_DELAY       12  /* an order or report to or from member arrives in
                                        * delay ticks (has_delay 0: never) */
#define SX_SQUAD_TRACE_KNOWLEDGE   13  /* knowledge: what it did to the leader's knowledge */
#define SX_SQUAD_TRACE_AREAS       14  /* areas: what it did to the danger areas */
#define SX_SQUAD_TRACE_INBOX       15  /* inbox: the leader's inbox took (QUEUED) or lost
                                        * (DROPPED) a report */

typedef struct sx_squad_trace {
	uint32_t kind;              /* SX_SQUAD_TRACE_* */
	uint32_t tactic;            /* DECIDE, ABORT, PICKED */
	uint32_t target;            /* DECIDE, ABORT, PICKED: if has_target */
	uint32_t member;            /* ASSIGNED, SPOT_*, DELAY */
	uint32_t slot;              /* ASSIGNED */
	uint32_t abort;             /* ABORT: the rule's index */
	uint32_t plan;              /* THINKING */
	uint32_t until;             /* THINKING */
	uint32_t score;             /* PICKED */
	uint32_t delay;             /* DELAY: if has_delay */
	uint32_t spot;              /* SPOT_*: the spot's index, if has_spot */
	int32_t pos[3];             /* SPOT_PICKED without a spot: the wish point */
	uint8_t has_target;
	uint8_t has_delay;
	uint8_t has_spot;           /* 1 for every SPOT_RANKED */
	uint8_t fired;              /* ABORT, TRIGGER */
	uint8_t kept;               /* TARGET */
	uint8_t fits;               /* SPOT_RANKED */
	int64_t cost;               /* ASSIGNED */
	int64_t distance;           /* SPOT_RANKED */
	sx_fact fact;               /* TARGET */
	sx_message msg;             /* REPORT */
	sx_decide_trace decide;     /* DECIDE */
	sx_knowledge_trace knowledge; /* KNOWLEDGE */
	sx_areas_trace areas;       /* AREAS */
	sx_message_trace inbox;     /* INBOX */
} sx_squad_trace;

/* trace gets every record of the calls above on squad, with context; NULL
 * turns it off (the default). For sx_crew_squad it covers the calls made
 * through that handle only; the crew's own work goes to sx_crew_set_trace.
 * trace must not call any function on the same squad, on the crew that lent
 * it (sx_crew_squad) or on a handle the squad or that crew lent
 * (sx_squad_knowledge, sx_squad_areas, sx_squad_decider, sx_crew_keeper).
 * Without rules a squad that needs them refuses plan and check (nothing is
 * traced). Returns 0. */
typedef void (*sx_squad_trace_fn)(void *context, const sx_squad_trace *trace);
int32_t sx_squad_set_trace(sx_squad *squad, sx_squad_trace_fn trace, void *context);
/* How orders (and reports) reach the member; the default is radio 0, 0: at
 * once. The leader always has its own orders at once. Distances run from the
 * leader in the squad's metric (Euclid: the root). Each returns 1, or 0 if
 * the id is not in the group. */
/* 1 tick per unit of distance, only within range (inclusive). */
int32_t sx_squad_set_shout(sx_squad *squad, uint32_t id, int64_t range);
/* base + distance * per_unit ticks, at any distance. */
int32_t sx_squad_set_radio(sx_squad *squad, uint32_t id, uint32_t base, uint32_t per_unit);
/* Never arrives. */
int32_t sx_squad_set_silent(sx_squad *squad, uint32_t id);
/* The game's own channel: the channel_delay callback of sx_squad_rules gives
 * the delay of every order (and of reports, see sx_squad_report_arrives_at_with
 * and sx_crew_report) for id (the game's own number); without rules it never
 * arrives. A squad with a member on such a channel needs rules
 * (SX_ERROR_NEEDS_RULES). */
int32_t sx_squad_set_channel_custom(sx_squad *squad, uint32_t member, uint32_t id);
/* The area a danger report (sx_squad_report_danger) adds: its kind, radius, strength, lifetime
 * and blocking are used; centre, source and data come from the report.
 * Default: kind 0, radius 2, strength 1, lifetime 255, not blocking.
 * Return 0 (danger_area fills *out); SX_ERROR_ARGUMENT for a null pointer. */
int32_t sx_squad_set_danger_area(sx_squad *squad, const sx_area *area);
int32_t sx_squad_danger_area(sx_squad *squad, sx_area *out);
/* The fact kind in the leader's knowledge that counts as a target (default
 * 0). Returns 0. */
int32_t sx_squad_set_target_kind(sx_squad *squad, uint32_t kind);
/* How many of the game's own columns each scoring row has from
 * sx_squad_extra_start
 * on (default 0, at most 1024). Returns 0; SX_ERROR_ARGUMENT above 1024
 * (nothing changes). */
int32_t sx_squad_set_extra_columns(sx_squad *squad, uint32_t count);
/* The first column of the game's own: 8, plus one for every role beyond two
 * (8 with the default list). SX_ERROR_ARGUMENT for a null
 * squad. */
int32_t sx_squad_extra_start(sx_squad *squad);
/* How far (inclusive, in the squad's metric, x and y; Euclid compares
 * squares) a spot may be from a slot's wish point to be taken, in both
 * phases; beyond it a spot does not count, and with none within the wish
 * point is used unchecked. Negative: the default, 3 * the slot's dist (at
 * least 1). Returns 0; SX_ERROR_ARGUMENT above 2^32 - 1. */
int32_t sx_squad_set_spot_radius(sx_squad *squad, int64_t radius);

/* The leader's own knowledge and danger areas, for every sx_knowledge_* and
 * sx_areas_* call (sx_areas_move_cost and set_trace too). One handle each
 * per squad, lent until sx_squad_free; sx_knowledge_free and sx_areas_free
 * do nothing to them. Null for a null squad. They reach into the squad:
 * use them only between squad calls, never while another call on the same
 * squad runs (on another thread, or from a trace or rules callback of that
 * call). Their traces see only the calls made
 * through them, not the squad's own work.
 * sx_squad_plan and sx_squad_check expire the areas (sx_areas_expire); the
 * game ages the knowledge itself (sx_knowledge_expire). */
sx_knowledge *sx_squad_knowledge(sx_squad *squad);
sx_areas *sx_squad_areas(sx_squad *squad);
/* The leader's own sighting: merged into its knowledge with source
 * SX_SOURCE_OWN. Returns how many facts were dropped to make room (0 or 1;
 * a dropped one goes to *evicted, which may be null). */
int32_t sx_squad_observe(sx_squad *squad, uint32_t now, const sx_fact *fact, sx_fact *evicted);
/* A member's report (SX_REPORT_*) on its way to the leader, read from
 * arrive_at on by sx_squad_check and sx_squad_plan; other kinds are dropped
 * when read. Returns 1, or 0 if the inbox refused it (as sx_inbox_push). */
int32_t sx_squad_report(sx_squad *squad, const sx_message *msg, uint32_t arrive_at);
/* A danger report on its way to the leader: member sender was shot at near
 * centre (3 values), from shooter (3 values; null: unknown). From arrive_at
 * on, sx_squad_check and sx_squad_plan add a danger area (the template of
 * sx_squad_set_danger_area with pos = centre, source = shooter, has_source 1
 * if given, data = sender), right after the reports of sx_squad_report.
 * Returns 1 if stored, 0 if the danger queue (config dangers) is full. */
int32_t sx_squad_report_danger(sx_squad *squad, uint32_t sender, const int32_t *centre,
                               const int32_t *shooter, uint32_t arrive_at);
/* When a report sent by the member at now reaches the acting leader: at
 * once for the leader, else over the member's channel from its position to
 * the leader's. Returns 1 and the tick in *at (may be null), or 0 if it
 * never arrives (also for an unknown or inactive member, and over a custom
 * channel: see sx_squad_report_arrives_at_with). */
int32_t sx_squad_report_arrives_at(sx_squad *squad, uint32_t id, uint32_t now, uint32_t *at);
/* The plan that became active since the last call, once: returns 1 and the
 * plan id in *plan (may be null), or 0. */
int32_t sx_squad_take_activated(sx_squad *squad, uint32_t *plan);
/* The known targets (facts of the target kind, in knowledge order; the
 * game's extra blocks follow this order): the first cap go to out (may be
 * null if cap is 0). Returns how many were written; total (may be null) gets
 * how many targets there are. */
int32_t sx_squad_targets(sx_squad *squad, sx_fact *out, uint32_t cap, uint32_t *total);

/* Add a shipped tactic; each returns its index (0, 1, ... in the order
 * added). Their numbers (weights, thresholds, skills) are defaults to tune;
 * the rows they read are the built-in columns. Skill: what a squad needs to
 * know the tactic. All but rush and fall back end (replan) on 2 losses, 2
 * members off plan, or a target not seen for 255 ticks. */
/* All attack the target's front; skill 0, low weight. */
int32_t sx_squad_add_rush(sx_squad *squad);
/* Advance in a shape (SX_SHAPE_*), spacing apart (2 suits cells); skill 64.
 * SX_ERROR_ARGUMENT for an unknown shape. */
int32_t sx_squad_add_formation(sx_squad *squad, uint32_t shape, int32_t spacing);
/* Gather on 2 sides (left and right) or 3 (and the front), 2 * dist from
 * the target, then all attack from dist on the signal (when all arrived, at
 * most 170 ticks); dist 4 suits cells; skill 128. SX_ERROR_ARGUMENT for
 * sides other than 2 or 3. */
int32_t sx_squad_add_flank(sx_squad *squad, uint32_t sides, int32_t dist);
/* Hold hidden spots within 12 of the group centre, attack when the target
 * comes within 6; skill 128. */
int32_t sx_squad_add_ambush(sx_squad *squad);
/* All attack one target, nearer ones score higher; skill 64. */
int32_t sx_squad_add_focus(sx_squad *squad);
/* 1-2 hold between the target and its nearest friend, the rest attack it;
 * the farther that friend (SX_IN_TARGET_FRIEND_DIST, full at 32), the
 * better; skill 192. */
int32_t sx_squad_add_isolate(sx_squad *squad);
/* All retreat 8 behind the leader, preferring cover, away from the nearest
 * target or danger area; scores by losses (0 none, full at 4); needs no
 * target; skill 0. */
int32_t sx_squad_add_fall_back(sx_squad *squad);
/* Add a tactic from sx_tactic_new. The squad takes it over in any case
 * (also on an error): do not use or free it again. Returns its index;
 * SX_ERROR_ARGUMENT for a null squad or tactic (a non-null tactic is freed
 * then). */
int32_t sx_squad_add_tactic(sx_squad *squad, sx_tactic *tactic);
/* Check that every consideration reads a column the rows have (sx_squad_extra_start
 * + extra columns, or a decider memory input), no tactic has more than
 * 255 slots and the squad's decider (sx_squad_decider) still has one option
 * per tactic. Returns 0, or SX_ERROR_PLAN with the tactic's index and the
 * column (UINT32_MAX for too many slots) to *tactic and *input (each may be
 * null); both UINT32_MAX for a changed option count. plan and check run this first. */
int32_t sx_squad_validate(sx_squad *squad, uint32_t *tactic, uint32_t *input);

/* Make a new plan and keep it pending; it becomes active think_ticks (from
 * skill) after now, until then the old one stays active. First a pending
 * plan whose time has come becomes active, and the reports that have
 * arrived are read.
 * Candidates: every tactic the skill knows whose slots can all reach their
 * minimum at once with the active members (role bits are hard), once per
 * known target (or once without one). Ties: the earlier tactic, then
 * target. A candidate whose own abort rule fires on its row scores 0.
 * extra: extra_count numbers, one block of sx_squad_set_extra_columns
 * values for the candidates without a target, then one block per known
 * target in sx_squad_targets order; missing values read 0. spots:
 * spot_count candidate places (each taken once per phase, only within the
 * spot radius, see sx_squad_set_spot_radius). seed: the game's, mixed with
 * now, for the skill's pick, spot errors and timing spread. Danger areas
 * past their lifetime are dropped first.
 * Returns 1 (the id goes to *plan, which may be null), 0 if nothing scores
 * above 0 or no member is active (the pending plan stays), SX_ERROR_PLAN
 * (see sx_squad_validate), SX_ERROR_NEEDS_RULES (a squad with custom parts:
 * use sx_squad_plan_with), or SX_ERROR_ARGUMENT. */
int32_t sx_squad_plan(sx_squad *squad, uint32_t now, const int32_t *extra,
                      uint32_t extra_count, const sx_spot *spots, uint32_t spot_count,
                      uint64_t seed, uint32_t *plan);
/* Watch the plans at now; call it regularly with the same extra and spots
 * as for sx_squad_plan (which it calls to replan, with seed). After the
 * first steps of sx_squad_plan, in this order: a plan is pending: KEEP. No
 * plan is active: it plans (REPLAN or NO_PLAN). An abort rule of the active
 * tactic fires, or its target is no longer known: it plans anew (REPLAN;
 * if nothing scores the active plan and its orders are dropped: NO_PLAN).
 * Another candidate scores more than the active one times 1 + hold_bonus
 * (not after the signal): REPLAN among the candidates scoring above that
 * bar only. The trigger fires: the signal is given (SIGNAL). Else KEEP.
 * Returns SX_SQUAD_* (a new plan's id to *plan, which may be null),
 * SX_ERROR_PLAN, SX_ERROR_NEEDS_RULES (see sx_squad_plan) or
 * SX_ERROR_ARGUMENT. */
int32_t sx_squad_check(sx_squad *squad, uint32_t now, const int32_t *extra,
                       uint32_t extra_count, const sx_spot *spots, uint32_t spot_count,
                       uint64_t seed, uint32_t *plan);
/* Move the orders whose arrival time has come into out, in the order they
 * were sent, at most cap of them (out may be null if cap is 0); the rest
 * wait for the next call. Orders for members that are no longer active
 * when they arrive are dropped. When a plan becomes active (in this call or
 * any other squad call), each of its active members gets its goal: the
 * leader at once, the others by their channel, measured from where everyone
 * is then; orders of the old plan still on their way are dropped, and so
 * are orders held back beyond cap that belong to another plan or to a
 * member no longer active by the next call. A member's later order never
 * arrives before its earlier one and replaces it if that is still on its
 * way. Returns how many were moved. */
int32_t sx_squad_orders_due(sx_squad *squad, uint32_t now, sx_order *out, uint32_t cap);
/* Give the signal of the active plan at now, once per plan: every active
 * member whose goal has a then_pos or starts on signal gets a second order
 * (pos = then_pos if any; kind: the slot's). Their start is lined up:
 * SX_START_AFTER_TICKS of the time until the slowest of these orders
 * arrives, plus the slot's own start_ticks, so all start at the same tick
 * (start_delay still comes on top). Returns 1, or 0 if no plan is active
 * or the signal was given. */
int32_t sx_squad_signal(sx_squad *squad, uint32_t now);
/* The member reached its first spot (for sx_tactic_trigger_all_arrived), seen by
 * the leader at once; a member that has to tell the leader sends an
 * SX_REPORT_ARRIVED instead. Returns 1, or 0 if it has no goal in the
 * active plan. */
int32_t sx_squad_arrived(sx_squad *squad, uint32_t id, uint32_t now);
/* Return 1 if the signal of the active plan was given. */
int32_t sx_squad_signal_given(sx_squad *squad);
/* The plan made last while it waits, and the plan being carried out: the
 * plan goes to *out (may be null), its first cap goals as (member, goal)
 * to goals (may be null if cap is 0), in assignment order. Return 1, or 0
 * if there is none. */
int32_t sx_squad_pending_plan(sx_squad *squad, sx_plan *out, sx_order *goals, uint32_t cap);
int32_t sx_squad_active_plan(sx_squad *squad, sx_plan *out, sx_order *goals, uint32_t cap);

/* -- rules: the game's logic at fixed points of a plan or check -- */

/* A squad calls the game's own code ("slots") through a sx_squad_rules that
 * the game lends to one call (the _with functions; the plain functions use
 * no rules). The squad never keeps the table or its pointers. Each callback
 * pointer may be NULL: then that slot does what the squad does without rules
 * (a zeroed sx_squad_rules is all built-in). A callback must not call any
 * function on the same squad or crew or on its decider (sx_squad_decider); it reads the squad through the view
 * (sx_view_*), valid only inside that callback. Costs and scores are
 * int64_t, lower is better; ties go to the lower member or spot index. The
 * built-in value a context carries (built_in) saturates to int64_t.
 *
 * Order of calls, always the same, never in hash order, no random numbers:
 *   plan:  keep_target for every known target (knowledge order); per candidate
 *          (tactic order, then target order) columns, then the custom aborts
 *          of its tactic in the order added (the first that fires stops the
 *          rest and the candidate scores 0); place for every custom place of
 *          the chosen tactic (slots in order, a slot's gather before its
 *          place); member_cost per slot and active member that has the slot's
 *          role bits, slot by slot, members in group order, before members
 *          are assigned; spot_score per member in assignment order and its
 *          gather spots, then its place spots, free spots within the spot
 *          radius in index order.
 *   check: first report for every game report that has arrived (arrival order),
 *          then all report actions are applied (after the danger reports of the
 *          call, before any abort); then columns and abort of the active row;
 *          then the candidates as in a plan (a replan runs a whole plan);
 *          last trigger of a custom trigger, while the plan holds and the
 *          signal was not given. sx_squad_plan_with reads the reports the
 *          same way first.
 *   channel_delay whenever an order goes over a custom channel: when a plan
 *          becomes active and on the signal per goal in the plan's goal order,
 *          on reinstate for that member; reports of the crew over the
 *          member's channel (sx_crew_tick_with, sx_crew_report_with).
 *          A squad times orders only: sx_squad_report keeps the arrive_at
 *          the game gives; sx_squad_report_arrives_at_with tells it.
 * Hex crews: every position a callback sees is in the game's offset
 * coordinates (contexts, view, report msg pos), and a position a callback
 * writes (place, an area added by a report) is taken as offset. The
 * contexts' built_in values, distances and heading stay as computed.
 * A squad that needs rules (a custom place, trigger, abort or channel, or
 * rule columns) is refused by the plain plan, check and tick with
 * SX_ERROR_NEEDS_RULES; the plain sx_squad_orders_due, sx_squad_arrived and
 * sx_squad_reinstate never activate a due plan (the next call with rules
 * does, its times counted from when it was due), sx_squad_signal returns 0,
 * and no plain call sends over a custom channel. */

/* Read-only look at a squad inside a callback (never made by the game). */
typedef struct sx_squad_view sx_squad_view;
/* What a report callback hands back, at most 4 actions per report (never made
 * by the game). The squad applies the actions of all reports of a call at one
 * point, after the last report: a later report's view does not show the
 * actions of an earlier one yet. A signal action is given only if no abort
 * fires and the target is not lost in the same call. */
typedef struct sx_report_actions sx_report_actions;

/* Slot 3, place: one candidate's tactic and slot, the group, the target. The
 * wish point goes to out (3 values), preset to the leader's position. */
typedef struct sx_place_ctx {
	uint32_t tactic;            /* index of the tactic */
	uint32_t slot;              /* index of the slot */
	uint32_t target;            /* the target's id; valid if has_target */
	int32_t dist;               /* the slot's gather_dist for a gather place, else dist */
	int32_t leader[3];          /* the acting leader */
	int32_t centre[3];          /* the group centre (mean of the active members) */
	int32_t face[3];            /* what the slots face (the target, else the nearest threat);
	                             * valid if has_face */
	int32_t friend_pos[3];      /* the target's nearest other target; valid if has_friend */
	uint8_t heading;            /* from centre to face (256 = full turn); 0 without a face */
	uint8_t gather;             /* 1: the slot's gather place, 0: its place */
	uint8_t has_face;
	uint8_t has_friend;
	uint8_t has_target;
} sx_place_ctx;

/* Slot 4, member cost. built_in: the metric distance from the member to the
 * slot's first wish point (Euclid: squared). */
typedef struct sx_cost_ctx {
	int64_t built_in;
	uint32_t tactic;
	uint32_t slot;
	sx_member member;
	int32_t wish[3];
} sx_cost_ctx;

/* Slot 5, spot score. built_in: 2^100 added to the metric
 * distance from wish to the spot when the spot lacks a bit of want_tags, saturated
 * to int64_t (see sx_spot_score_builtin), so every missing spot is INT64_MAX.
 * miss and distance are its two parts: a callback that keeps the exact
 * built-in order scores e.g. (miss ? 1 << 62 : 0) + distance + its own
 * change, as long as the distances stay below 2^62. */
typedef struct sx_spot_ctx {
	int64_t built_in;
	int64_t distance;           /* wish to spot in the squad's metric (Euclid:
	                             * squared), saturated to int64_t */
	uint32_t tactic;
	uint32_t slot;
	uint32_t index;             /* of the spot in the game's list */
	uint32_t want_tags;         /* slot want_tags | the place's tags */
	uint32_t target;            /* the plan's target id; valid if has_target */
	sx_spot spot;
	sx_member member;           /* the member that gets the spot */
	int32_t wish[3];
	int32_t target_pos[3];      /* valid if has_target */
	uint8_t gather;             /* 1: ranking gather spots */
	uint8_t has_target;
	uint8_t miss;               /* 1: the spot lacks a bit of want_tags */
} sx_spot_ctx;

/* Slot 8, channel delay. distance: the plain distance in the squad's metric
 * (Euclid: the rounded-down root). */
typedef struct sx_channel_ctx {
	uint32_t sender;
	uint32_t receiver;
	uint32_t distance;
	uint32_t now;               /* when it is sent */
	int32_t from[3];
	int32_t to[3];
	uint8_t report;             /* 1: a report, 0: an order */
} sx_channel_ctx;

/* Slots 2 and 7, one scoring row. For columns: row is the row before the rule
 * columns (row_len values) and out gets exactly the rule columns (all 0 when
 * called). For abort: the active plan's row (active 1) in a check, or a
 * candidate's (active 1 only for the active (tactic, target)), which then
 * scores 0. row is valid during the callback only. */
typedef struct sx_row_ctx {
	const int32_t *row;
	uint32_t row_len;
	uint32_t tactic;
	uint32_t target;            /* the candidate's target id; valid if has_target */
	uint32_t now;
	uint8_t has_target;
	uint8_t active;
} sx_row_ctx;

/* The game's callbacks, NULL = built-in. Return values of 1 or 0 mean: keep /
 * fire / usable / arrives, or not. user is passed to every callback.
 *   keep_target   slot 1: 1 keeps the known target for new plans, 0 drops it
 *                 (it still counts as a friend of other targets and as a threat for a
 *                 tactic without a target; it never ends the active plan).
 *   columns       slot 2: fill the count rule columns of a candidate's row.
 *   place         slot 3: the wish point of SX_PLACE_CUSTOM id.
 *   member_cost   slot 4: 1 and the cost in *cost, or 0: the member cannot
 *                 take the slot.
 *   spot_score    slot 5: 1 and the score in *score, or 0: the member may not
 *                 use the spot (the spot radius, the taken spots and the
 *                 skill's rank error still apply).
 *   trigger       slot 6: 1 fires the signal of sx_tactic_trigger_custom id.
 *   abort         slot 7: 1 ends the plan of sx_tactic_add_abort_custom id.
 *   channel_delay slot 8: 1 and the delay in ticks in *delay (arrives at
 *                 now + delay, wrapping), or 0: it does not arrive.
 *   report        slot 9: the leader reads a report of a kind the squad does not
 *                 know (every kind but 1, 2 and 4; a game's own start at
 *                 SX_REPORT_GAME_FIRST); put what the squad should do into actions. */
typedef struct sx_squad_rules {
	int32_t (*keep_target)(void *user, const sx_squad_view *view, const sx_fact *fact);
	void (*columns)(void *user, const sx_squad_view *view, const sx_row_ctx *ctx, int32_t *out,
	                uint32_t count);
	void (*place)(void *user, uint32_t id, const sx_place_ctx *ctx, int32_t *out);
	int32_t (*member_cost)(void *user, const sx_cost_ctx *ctx, int64_t *cost);
	int32_t (*spot_score)(void *user, const sx_spot_ctx *ctx, int64_t *score);
	int32_t (*trigger)(void *user, uint32_t id, const sx_squad_view *view, uint32_t now);
	int32_t (*abort)(void *user, uint32_t id, const sx_squad_view *view, const sx_row_ctx *ctx);
	int32_t (*channel_delay)(void *user, uint32_t id, const sx_channel_ctx *ctx, uint32_t *delay);
	void (*report)(void *user, const sx_squad_view *view, const sx_message *msg, uint32_t now,
	               sx_report_actions *actions);
	void *user;
} sx_squad_rules;

/* The two built-ins as callbacks, for wrapping: they return ctx->built_in,
 * saturated to int64_t (1 = usable). For exact built-in behaviour leave the
 * pointer NULL. */
int32_t sx_builtin_member_cost(void *user, const sx_cost_ctx *ctx, int64_t *cost);
int32_t sx_builtin_spot_score(void *user, const sx_spot_ctx *ctx, int64_t *score);

/* For a member_cost callback: the cost of the way from from to to (3 ints
 * each: x, y, level; on a hex grid offset coordinates, as the callbacks get
 * them) to *cost. Returns 1 if a way was found, 0 if the goal cannot be
 * reached or budget cells (0 = unlimited) ran out (*cost is not written),
 * SX_ERROR_ARGUMENT for a null pointer or a negative budget. Runs a search on
 * nav and replaces its last one (sx_nav_continue then has nothing to go on
 * with). Pass the nav and map the game already has. */
int32_t sx_path_cost(sx_nav *nav, const sx_map *map, const int32_t *from, const int32_t *to,
	int32_t budget, int64_t *cost);

/* Every callback NULL and user NULL (all built-in, exactly as a zeroed table):
 * a table to start from. Returns 0; SX_ERROR_ARGUMENT for a null out. */
int32_t sx_squad_rules_default(sx_squad_rules *out);

/* The built-in spot score to *out: (miss << 100) | distance, where miss is 1
 * when the spot lacks a bit of want_tags and distance the metric distance
 * (SX_METRIC_*) from wish (3 values) to the spot in x and y (Euclid: squared);
 * saturated to int64_t (a spot that misses a tag always reads INT64_MAX).
 * Returns 0; SX_ERROR_ARGUMENT for a null pointer or an unknown metric. */
int32_t sx_spot_score_builtin(const int32_t *wish, uint32_t want_tags, const sx_spot *spot,
                              uint32_t metric, int64_t *out);

/* The view, read-only, valid inside the callback it came with. Counts and
 * indexes run in group order (also suspended and gone members) and knowledge
 * order (every known target, also the ones keep_target drops). The at calls
 * return 1, or 0 past the end; the count calls return a count;
 * SX_ERROR_ARGUMENT for a null pointer. */
int32_t sx_view_member_count(const sx_squad_view *view);
int32_t sx_view_member_at(const sx_squad_view *view, uint32_t index, sx_member *out);
int32_t sx_view_target_count(const sx_squad_view *view);
int32_t sx_view_target_at(const sx_squad_view *view, uint32_t index, sx_fact *out);
/* The plan being carried out. */
typedef struct sx_view_plan {
	uint32_t plan;              /* its id */
	uint32_t tactic;
	uint32_t target;            /* valid if has_target */
	uint32_t since;             /* when it became active */
	uint8_t has_plan;           /* 0: no plan is active (the other fields are 0) */
	uint8_t has_target;
	uint8_t signal_given;
} sx_view_plan;
/* Always writes the active plan to *out (all 0, has_plan 0, without one);
 * returns 1 with a plan, else 0. */
int32_t sx_view_active_plan(const sx_squad_view *view, sx_view_plan *out);
/* The group centre (mean of the active members) to out (3 values); 1, or 0
 * without an active member. */
int32_t sx_view_centre(const sx_squad_view *view, int32_t *out);

/* Actions of a report callback. Each returns 1 if stored, 0 when the report
 * has its 4 actions already. */
/* Add a danger area (its since becomes the tick of the call). */
int32_t sx_report_add_area(sx_report_actions *actions, const sx_area *area);
/* Give the signal of the active plan, if it was not given yet. */
int32_t sx_report_signal(sx_report_actions *actions);
/* Forget the target with this subject. */
int32_t sx_report_forget(sx_report_actions *actions, uint32_t subject);

/* The calls with rules (NULL = built-in, the same as the plain call; the table
 * is read during the call only). As sx_squad_plan and sx_squad_check. */
int32_t sx_squad_plan_with(sx_squad *squad, uint32_t now, const int32_t *extra,
                           uint32_t extra_count, const sx_spot *spots, uint32_t spot_count,
                           uint64_t seed, const sx_squad_rules *rules, uint32_t *plan);
int32_t sx_squad_check_with(sx_squad *squad, uint32_t now, const int32_t *extra,
                            uint32_t extra_count, const sx_spot *spots, uint32_t spot_count,
                            uint64_t seed, const sx_squad_rules *rules, uint32_t *plan);
/* As sx_squad_signal; the second order of each member goes over a custom channel too. */
int32_t sx_squad_signal_with(sx_squad *squad, uint32_t now, const sx_squad_rules *rules);
/* As sx_squad_reinstate; the goal goes over a custom channel too. */
int32_t sx_squad_reinstate_with(sx_squad *squad, uint32_t id, uint32_t now,
                                const sx_squad_rules *rules);
/* As sx_squad_report_arrives_at; over a custom channel the channel_delay
 * callback says (the plain call: never). */
int32_t sx_squad_report_arrives_at_with(sx_squad *squad, uint32_t id, uint32_t now,
                                        const sx_squad_rules *rules, uint32_t *at);

/* -- keeper: one member's goal -- */

/* A member's current goal with its postpone state: once the goal stays
 * postponed for give_up_ticks it counts as broken (report it with
 * SX_REPORT_GOAL_BROKEN). While suspended the clock does not run. The
 * functions without their own note return 0, or SX_ERROR_ARGUMENT for a null
 * keeper. */
sx_keeper *sx_keeper_new(void);
void sx_keeper_free(sx_keeper *keeper);
/* Take goal (from an order), replacing the old one and the postpone state.
 * Returns 0; SX_ERROR_ARGUMENT for an unknown kind or start or a slot > 255.
 * now is not used by the keeper itself; it is there for the common order. */
int32_t sx_keeper_set(sx_keeper *keeper, uint32_t now, const sx_goal *goal);
/* Fill *out. Return 1, or 0 without a goal. */
int32_t sx_keeper_goal(sx_keeper *keeper, sx_goal *out);
/* Drop the goal and the postpone state. */
int32_t sx_keeper_clear(sx_keeper *keeper);
/* The member turns to something else for now (the first time stays). */
int32_t sx_keeper_goal_postponed(sx_keeper *keeper, uint32_t now);
/* Back on the goal: the postpone clock is reset. */
int32_t sx_keeper_goal_won(sx_keeper *keeper, uint32_t now);
/* Out of play: never broken. unsuspend restarts a postponed goal's clock at
 * now. */
int32_t sx_keeper_suspend(sx_keeper *keeper);
int32_t sx_keeper_unsuspend(sx_keeper *keeper, uint32_t now);
/* Return 1 once the goal has been postponed for at least give_up_ticks
 * (give_up_ticks of the member's skill level), never while suspended. */
int32_t sx_keeper_broken(sx_keeper *keeper, uint32_t now, uint32_t give_up_ticks);

/* What sx_keeper_broken did (since version 12): one record per call. */
#define SX_KEEPER_TRACE_CHECKED 1  /* waited ticks since the goal was postponed (0 when
                                    * it is not) against limit; broken: the answer */

typedef struct sx_keeper_trace {
	uint32_t kind;              /* SX_KEEPER_TRACE_* */
	uint32_t waited;
	uint32_t limit;             /* give_up_ticks */
	uint8_t broken;
} sx_keeper_trace;

/* trace gets every record of sx_keeper_broken on keeper, with context; NULL
 * turns it off (the default). For sx_crew_keeper it covers the calls made
 * through that handle only (it stays set while the handle does). trace must
 * not call any function on keeper or, for sx_crew_keeper, on the crew that
 * lent it or its other lent handles. Returns 0. */
typedef void (*sx_keeper_trace_fn)(void *context, const sx_keeper_trace *trace);
int32_t sx_keeper_set_trace(sx_keeper *keeper, sx_keeper_trace_fn trace, void *context);
/* The member's decider inputs at x, y, z to out (room for 5): has_goal,
 * dist_to_goal (in metric SX_METRIC_*, Euclid the root; -1 without a goal
 * or pos), goal_age (saturated), signal_given (0 or 1), discipline (see
 * sx_skill_table_hold_bonus_for). Returns 0; SX_ERROR_ARGUMENT for an
 * unknown metric. The five numbers are the columns SX_KEEPER_IN_* and fit
 * an sx_member_inputs. */
int32_t sx_keeper_inputs(sx_keeper *keeper, uint32_t now, int32_t x, int32_t y, int32_t z,
                         uint32_t metric, uint32_t signal_given, uint32_t discipline,
                         int32_t *out);

/* The columns of sx_keeper_inputs by number. A member's decider row starts
 * with these five; its own inputs follow from SX_KEEPER_IN_EXTRA. */
#define SX_KEEPER_IN_HAS_GOAL   0
#define SX_KEEPER_IN_DIST       1
#define SX_KEEPER_IN_AGE        2
#define SX_KEEPER_IN_SIGNAL     3
#define SX_KEEPER_IN_DISCIPLINE 4
#define SX_KEEPER_IN_EXTRA      5

/* The same five numbers by name (named after the member: the function
 * sx_keeper_inputs already takes the obvious name). */
typedef struct sx_member_inputs {
	int32_t has_goal, dist, age, signal, discipline;
} sx_member_inputs;

/* Adds the ready-made "keep my goal" option to a member's decider and returns
 * its option number: weight times (has_goal is 1) times a falling curve on
 * dist (full at 0, nothing at 64). Rows start with the five keeper columns.
 * Returns SX_ERROR_ARGUMENT for a null decider. */
int32_t sx_keeper_add_goal_option(sx_decider *decider, uint32_t weight);
/* Marks the goal option (from sx_keeper_add_goal_option) as the action going
 * on while the keeper holds a goal that is not postponed, with discipline as
 * its bonus; otherwise no action is current. Returns 0, or SX_ERROR_ARGUMENT
 * for a null keeper or decider. */
int32_t sx_keeper_set_current(const sx_keeper *keeper, sx_decider *decider,
                              uint32_t option, uint32_t discipline);

/* -- crew: a squad with one keeper per member, one call per tick -- */

/* The whole group loop: per tick the game sets where its members are
 * (sx_crew_set_pos), calls sx_crew_tick, reads the events, and for each
 * member asks sx_crew_member_inputs for its decider row and, once it
 * decided, calls sx_crew_member_chose. The crew moves nobody. Channels,
 * tactics, skill and leader are set on the crew's own squad
 * (sx_crew_squad). With hex_grid the crew's own calls take and return
 * offset coordinates of that layout (the squad and keepers hold axial
 * ones: calls on sx_crew_squad and sx_crew_keeper use axial coordinates).
 * The functions without their own note return 1, or 0 for an unknown id;
 * SX_ERROR_ARGUMENT for a null crew or a null required pointer. */
typedef struct sx_crew sx_crew;

typedef struct sx_crew_config {
	sx_squad_config squad;
	uint32_t arrive_radius;     /* how far from its first spot a member counts as
	                             * arrived, in the squad's metric (Euclid: compared
	                             * squared) */
	uint32_t hex_grid;          /* 0 none, else SX_GRID_HEX_* (needs SX_METRIC_HEX) */
} sx_crew_config;

/* The squad defaults (sx_squad_config_default), arrive radius 0, no hex.
 * Returns 0; SX_ERROR_ARGUMENT for a null out. */
int32_t sx_crew_config_default(sx_crew_config *out);

/* What happened in one sx_crew_tick, in the order of its steps. */
#define SX_CREW_PLANNED   1 /* a new plan is pending (plan) */
#define SX_CREW_ACTIVATED 2 /* that plan became active (plan); members without a
                             * goal in it have none now */
#define SX_CREW_SIGNAL    3 /* the leader gave the signal */
#define SX_CREW_ORDER     4 /* an order reached member; its keeper holds goal now
                             * (plan = goal.plan); start_at: the tick it starts,
                             * unless it starts on the signal (has_start_at 0) */
#define SX_CREW_START     5 /* member's start tick has come (once per order) */
#define SX_CREW_ARRIVED   6 /* member stands on its first spot and sent the leader
                             * SX_REPORT_ARRIVED (once per goal) */
#define SX_CREW_BROKEN    7 /* member gave its goal up and sent the leader
                             * SX_REPORT_GOAL_BROKEN (once per goal) */
#define SX_CREW_NO_PLAN   8 /* no plan is active and none could be made; when the
                             * active plan ended, no member has a goal now */

typedef struct sx_crew_event {
	uint32_t kind;              /* SX_CREW_* */
	uint32_t member;            /* ORDER, START, ARRIVED, BROKEN; else 0 */
	uint32_t plan;              /* PLANNED, ACTIVATED, ORDER; else 0 */
	uint32_t start_at;          /* valid if has_start_at */
	sx_goal goal;               /* ORDER only */
	uint8_t has_start_at;
} sx_crew_event;

/* A crew with an empty squad as configured. Null if config is null, a
 * capacity is 0, the metric or hex_grid is unknown, or hex_grid is set
 * without SX_METRIC_HEX. */
sx_crew *sx_crew_new(const sx_crew_config *config);
/* Frees the crew and ends every handle it lent. */
void sx_crew_free(sx_crew *crew);
/* Add a member (with an empty keeper) at the end. Returns its index;
 * SX_ERROR_ARGUMENT if the group is full, the id is taken or the state is
 * unknown. */
int32_t sx_crew_add_member(sx_crew *crew, const sx_member *member);
/* Remove a member and its keeper. */
int32_t sx_crew_remove_member(sx_crew *crew, uint32_t id);
/* Out of play: suspended in the group, its keeper paused (never broken). */
int32_t sx_crew_suspend(sx_crew *crew, uint32_t id);
/* Back in play (as sx_squad_reinstate); its keeper's clock restarts at now.
 * Without a goal in the active plan its keeper is cleared. */
int32_t sx_crew_reinstate(sx_crew *crew, uint32_t id, uint32_t now);
/* Dead or lost (as sx_squad_set_gone); its keeper stays paused. */
int32_t sx_crew_set_gone(sx_crew *crew, uint32_t id);
/* Where the member is now. pos: 3 values. */
int32_t sx_crew_set_pos(sx_crew *crew, uint32_t id, const int32_t *pos);
/* As sx_squad_observe. */
int32_t sx_crew_observe(sx_crew *crew, uint32_t now, const sx_fact *fact, sx_fact *evicted);
/* As sx_squad_report_danger: returns 1 if stored, 0 if the danger queue is
 * full (nothing stored). */
int32_t sx_crew_report_danger(sx_crew *crew, uint32_t sender, const int32_t *centre,
                              const int32_t *shooter, uint32_t arrive_at);
/* Runs the group at now with extra, spots and seed as for sx_squad_check:
 * syncs the keepers with the group (a keeper is paused while its member is
 * not active), checks the plans, takes the activated plan, hands the due
 * orders to the keepers, then notes starts, arrivals (an active member
 * within arrive_radius of its goal's pos on the same level reports
 * SX_REPORT_ARRIVED over its channel; a report the inbox refuses is tried
 * again next tick) and broken goals (give_up_ticks of the member's skill).
 * Only goals of the active plan count. Members in group order.
 * The tick's events go to events, at most cap of them (events may be null
 * if cap is 0); the rest are dropped, and nothing else depends on cap.
 * *total (total may be null) gets the number of all events. Returns how
 * many were written, SX_ERROR_PLAN (nothing after the check is done then)
 * or SX_ERROR_ARGUMENT. */
int32_t sx_crew_tick(sx_crew *crew, uint32_t now, const int32_t *extra, uint32_t extra_count,
                     const sx_spot *spots, uint32_t spot_count, uint64_t seed,
                     sx_crew_event *events, uint32_t cap, uint32_t *total);
/* As sx_crew_tick with the game's rules (NULL = the plain call): the squad's
 * check is sx_squad_check_with, the orders and the members' reports go over a
 * custom channel through its channel_delay callback, and a game report that
 * arrives (see sx_crew_report) goes to the report callback. With hex_grid
 * every callback sees offset coordinates (see "rules" above). Returns as
 * sx_crew_tick; a crew whose squad needs rules is refused by the plain call
 * with SX_ERROR_NEEDS_RULES. */
int32_t sx_crew_tick_with(sx_crew *crew, uint32_t now, const int32_t *extra, uint32_t extra_count,
                          const sx_spot *spots, uint32_t spot_count, uint64_t seed,
                          const sx_squad_rules *rules, sx_crew_event *events, uint32_t cap,
                          uint32_t *total);
/* As sx_crew_reinstate with rules: the goal goes over a custom channel too. */
int32_t sx_crew_reinstate_with(sx_crew *crew, uint32_t id, uint32_t now,
                               const sx_squad_rules *rules);
/* A report of the game, sent by member msg->sender at now over the member's
 * channel to the leader (as the crew's own reports; msg->pos is in offset
 * coordinates with hex_grid). Its kind decides who reads it (see
 * sx_squad_report): a game's own kind (SX_REPORT_GAME_FIRST on) goes to the
 * report callback in sx_crew_tick_with. Returns 1 if stored or if it can never
 * arrive (no way to the leader, the sender is not an active member, a custom
 * channel without rules: it is dropped), 0 if the inbox refused it,
 * SX_ERROR_ARGUMENT for a null pointer. */
int32_t sx_crew_report(sx_crew *crew, uint32_t now, const sx_message *msg);
/* As sx_crew_report; a custom channel times it through the rules. */
int32_t sx_crew_report_with(sx_crew *crew, uint32_t now, const sx_message *msg,
                            const sx_squad_rules *rules);
/* The member's decider inputs at now (its position, the squad's metric, the
 * signal and the discipline of its own skill) to *out. */
int32_t sx_crew_member_inputs(sx_crew *crew, uint32_t id, uint32_t now, sx_member_inputs *out);
/* What the member chose at now: its goal (goal_won 1: the postpone clock is
 * reset) or something else (goal_won 0: the goal is postponed). */
int32_t sx_crew_member_chose(sx_crew *crew, uint32_t id, uint32_t now, uint32_t goal_won);
/* The member's current goal to *out. Returns 1, or 0 for an unknown id or
 * no goal. */
int32_t sx_crew_goal(sx_crew *crew, uint32_t id, sx_goal *out);
/* Handles lent by the crew: its own squad (valid until sx_crew_free) and
 * the keeper of member id (valid until the next sx_crew_* call that changes
 * the crew, i.e. any but sx_crew_member_inputs, sx_crew_goal, sx_crew_squad
 * and sx_crew_keeper, and while member id is in the group; after that
 * every sx_keeper_* call on it returns SX_ERROR_ARGUMENT, it never reaches
 * another member's keeper). One keeper handle per member id, the same each
 * time (one small handle per member id ever asked for, kept until the
 * crew is freed, so asking for many ids over a long game grows the list
 * a little). sx_squad_free and sx_keeper_free do nothing to them; the crew
 * frees them with itself.
 * The crew takes its squad's orders itself: sx_squad_orders_due on a crew's
 * squad returns SX_ERROR_ARGUMENT. Null for a null crew or an unknown id. */
sx_squad *sx_crew_squad(sx_crew *crew);
sx_keeper *sx_crew_keeper(sx_crew *crew, uint32_t id);

/* What the crew's calls did (since version 12): sx_crew_observe,
 * sx_crew_tick, sx_crew_reinstate, sx_crew_report and their _with forms.
 * The squad's records come wrapped in SQUAD (with hex_grid in offset
 * coordinates), each keeper check of a tick in KEEPER, each report the crew
 * sends (its own and the game's) in SENT, after the squad's INBOX records of
 * it (none when it never arrives). Fields that do not apply to kind are 0. */
#define SX_CREW_TRACE_SQUAD  1     /* squad: a record of the crew's squad */
#define SX_CREW_TRACE_KEEPER 2     /* keeper: the check of member's keeper */
#define SX_CREW_TRACE_SENT   3     /* member (msg.sender) sent msg; it arrives in delay
                                    * ticks (has_delay 0: never, it was dropped); queued
                                    * 1 when the inbox took it */

typedef struct sx_crew_trace {
	uint32_t kind;              /* SX_CREW_TRACE_* */
	uint32_t member;            /* KEEPER, SENT */
	uint32_t delay;             /* SENT: if has_delay */
	uint8_t has_delay;
	uint8_t queued;             /* SENT */
	sx_message msg;             /* SENT */
	sx_keeper_trace keeper;     /* KEEPER */
	sx_squad_trace squad;       /* SQUAD */
} sx_crew_trace;

/* trace gets every record of the calls above on crew, with context; NULL
 * turns it off (the default). Calls through sx_crew_squad and sx_crew_keeper
 * go to their own traces, not to this one. trace must not call any function
 * on the same crew or on a handle it lent (sx_crew_squad, sx_crew_keeper and
 * the squad's sx_squad_knowledge, sx_squad_areas and sx_squad_decider).
 * Without rules a crew whose squad
 * needs them refuses the tick (nothing is traced). Returns 0. */
typedef void (*sx_crew_trace_fn)(void *context, const sx_crew_trace *trace);
int32_t sx_crew_set_trace(sx_crew *crew, sx_crew_trace_fn trace, void *context);

/* -- goal and report helpers -- */

/* The tick a goal starts when its order arrived at received: received +
 * start_delay (+ start_ticks for SX_START_AFTER_TICKS), wrapping. Returns 1
 * and writes *out, 0 for SX_START_ON_SIGNAL (it starts on the signal), or
 * SX_ERROR_ARGUMENT. */
int32_t sx_goal_start_tick(const sx_goal *goal, uint32_t received, uint32_t *out);
/* 1 if the member must report its arrival (the goal gathers first or waits
 * for the signal), else 0; SX_ERROR_ARGUMENT for a null or bad goal. */
int32_t sx_goal_awaits_arrival(const sx_goal *goal);
/* Fill *out with a report for sx_squad_report. Return 0, or
 * SX_ERROR_ARGUMENT for a null argument. enemy copies the fact's subject,
 * pos, seen_at, strength and data. arrived and goal_broken name the plan. */
int32_t sx_report_enemy(uint32_t sender, const sx_fact *fact, sx_message *out);
int32_t sx_report_arrived(uint32_t sender, uint32_t plan, sx_message *out);
int32_t sx_report_goal_broken(uint32_t sender, uint32_t plan, sx_message *out);

/* ---- missions (since version 11) ----------------------------------------- */

/* A character's mission: a motive (what it wants), a plan of steps toward it
 * (patrol, wait, search, attack, ...), short detours from the plan (engage,
 * flee) and replanning when the plan breaks. The game ticks the mission with
 * the character's position, its sx_knowledge and the game's condition bits
 * (flags); the mission returns one action and what happened (events). The
 * mission moves nobody; the game walks or attacks as the action says.
 *
 * Positions are 3 values in cells. With config.grid SX_GRID_HEX_* every
 * position the game gives or gets is in offset coordinates of that layout,
 * also the fact positions in the knowledge and in the rules' contexts.
 * Everything is reserved in sx_mission_new; a tick allocates nothing
 * (sx_mission_set_plan and sx_mission_add_playbook may). Integer only,
 * deterministic, no random numbers but the seed of the decider's pick (now
 * with the mission's id).
 *
 * The functions without their own note return 0, or SX_ERROR_ARGUMENT for a
 * null mission, a null required pointer or an unknown code. */
typedef struct sx_mission sx_mission;

/* Most fail conditions of a step and of a plan; most events of a tick. */
#define SX_MISSION_MAX_FAILS  4
#define SX_MISSION_MAX_EVENTS 8

/* Places a step or detour refers to (sx_where.kind). */
#define SX_WHERE_FIXED          0 /* pos */
#define SX_WHERE_SUBJECT        1 /* where subject was last known */
#define SX_WHERE_MOTIVE_PLACE   2 /* the motive's place */
#define SX_WHERE_MOTIVE_SUBJECT 3 /* where the motive's subject was last known */
#define SX_WHERE_HERE           4 /* where the character stands at that moment */

typedef struct sx_where {
	uint32_t kind;              /* SX_WHERE_* */
	int32_t pos[3];             /* SX_WHERE_FIXED */
	uint32_t subject;           /* SX_WHERE_SUBJECT */
} sx_where;

/* Conditions that break a step or a plan (sx_fail.kind). A subject is known
 * when a fact of the config's subject_kind exists for it. */
#define SX_FAIL_UNKNOWN  0 /* subject is not known */
#define SX_FAIL_DEFEATED 1 /* subject is known as defeated (config.defeated_bits) */
#define SX_FAIL_FACT     2 /* a fact (subject, fact_kind) has data & mask == value */
#define SX_FAIL_FLAGS    3 /* the tick's flags have flags & flags_mask == flags_value */
#define SX_FAIL_CUSTOM   4 /* the fail callback of sx_mission_rules says so for id */

typedef struct sx_fail {
	uint32_t kind;              /* SX_FAIL_* */
	uint32_t subject;           /* UNKNOWN, DEFEATED, FACT */
	uint32_t fact_kind;         /* FACT */
	uint32_t mask;              /* FACT */
	uint32_t value;             /* FACT */
	uint32_t id;                /* CUSTOM */
	uint64_t flags_mask;        /* FLAGS */
	uint64_t flags_value;       /* FLAGS */
} sx_fail;

/* What a step does (sx_step.kind), with its place at and numbers a and b:
 *   GO_TO     walk to at
 *   WAIT      wait a ticks
 *   PAUSE     wait config.pause_ticks
 *   FOLLOW    follow subject a
 *   WAIT_FOR  wait until subject a is within radius b
 *   SEARCH    search around at within radius a (0: no limit; Euclid
 *             compares squares) for b ticks, going to the points of
 *             config.search in turn
 *   HOLD      hold at
 *   ATTACK    attack subject a
 *   WANDER    wander about
 *   SIGNAL    give signal a (done at once)
 *   CUSTOM    the game's step a at at with param b; done on
 *             sx_mission_finish_step
 *   CALL      call for help on channel a, kind b
 *   SQUAD     ask for squad tactic a against at */
#define SX_STEP_GO_TO    0
#define SX_STEP_WAIT     1
#define SX_STEP_PAUSE    2
#define SX_STEP_FOLLOW   3
#define SX_STEP_WAIT_FOR 4
#define SX_STEP_SEARCH   5
#define SX_STEP_HOLD     6
#define SX_STEP_ATTACK   7
#define SX_STEP_WANDER   8
#define SX_STEP_SIGNAL   9
#define SX_STEP_CUSTOM   10
#define SX_STEP_CALL     11
#define SX_STEP_SQUAD    12

/* One step of a plan. A zeroed sx_step is GO_TO (0, 0, 0). at is read only
 * by the kinds with a place. */
typedef struct sx_step {
	uint32_t kind;              /* SX_STEP_* */
	sx_where at;
	uint32_t a, b;
	sx_fail fails[4];           /* conditions that break the step */
	uint32_t fail_count;        /* used entries of fails, at most 4 */
	uint8_t skip_if_impossible; /* 1: skip the step instead of breaking the plan
	                             * when a position it needs is not known */
} sx_step;

/* What the character wants (sx_motive.kind). */
#define SX_MOTIVE_GUARD  0 /* keep a place or a subject safe */
#define SX_MOTIVE_DEFEAT 1 /* defeat a subject or (without one) the enemies */
#define SX_MOTIVE_REACH  2 /* get to a place */
#define SX_MOTIVE_CUSTOM 3 /* the game's motive custom */

typedef struct sx_motive {
	uint32_t kind;              /* SX_MOTIVE_* */
	uint32_t custom;            /* SX_MOTIVE_CUSTOM: the game's kind */
	int32_t place[3];           /* valid if has_place */
	uint32_t subject;           /* valid if has_subject */
	uint32_t radius;            /* how close counts as there; 0: config.arrive_radius */
	uint8_t has_place;
	uint8_t has_subject;
} sx_motive;

/* What a character does when it loses the subject of an engage detour
 * (sx_leeway.on_lost). */
#define SX_ON_LOST_RETURN 0 /* go back to the anchor */
#define SX_ON_LOST_SEARCH 1 /* search lost_ticks within lost_radius of the last
                             * known place first */
#define SX_ON_LOST_KEEP   2 /* go on to the last known place */

/* The presets of sx_leeway_preset. */
#define SX_LEEWAY_STRICT   0 /* distance 3, returns when the subject is lost, never replans */
#define SX_LEEWAY_FLEXIBLE 1 /* distance 12, searches 60 ticks within 6, replans (the default) */
#define SX_LEEWAY_FREE     2 /* no leash, keeps going, replans and widens */

/* How freely a character acts: its leash for detours (distance from the
 * anchor in the mission's metric, Euclid compares squares, and duration)
 * and whether it may replan and widen. */
typedef struct sx_leeway {
	uint32_t distance;          /* valid if has_distance (else no limit) */
	uint32_t ticks;             /* valid if has_ticks (else no limit) */
	uint32_t on_lost;           /* SX_ON_LOST_* */
	uint32_t lost_ticks;        /* SX_ON_LOST_SEARCH */
	uint32_t lost_radius;       /* SX_ON_LOST_SEARCH */
	uint8_t has_distance;
	uint8_t has_ticks;
	uint8_t replan;             /* 1: may take another plan when its plan breaks or ends */
	uint8_t widen;              /* 1: may take the widening plans (only with replan) */
} sx_leeway;

/* Settings of a mission. Start from sx_mission_config_default. */
typedef struct sx_mission_config {
	uint32_t id;                /* for the seed and the search points */
	uint32_t max_steps;         /* most steps of a plan, 1 .. 65536 */
	uint32_t max_playbook;      /* playbook slots */
	uint32_t subject_kind;      /* the fact kind positions are read from */
	uint32_t enemy_bits;        /* bits of a fact's data that mark an enemy */
	uint32_t defeated_bits;     /* bits of a fact's data that mark a defeated subject */
	uint32_t arrive_radius;     /* how close counts as arrived */
	uint32_t lost_after;        /* ticks after which a subject not seen counts as lost */
	uint32_t pause_ticks;       /* ticks of SX_STEP_PAUSE */
	uint32_t safe_distance;     /* distance at which a fleeing character is safe */
	uint32_t reached_signal;    /* the signal given when the motive's place is reached */
	uint32_t metric;            /* SX_METRIC_* */
	uint32_t grid;              /* SX_GRID_SQUARE or SX_GRID_HEX_* (needs SX_METRIC_HEX) */
	uint32_t extra_columns;     /* the game's columns of each decider row (columns callback) */
	uint32_t pick;              /* SX_PICK_* among the candidate plans of a replan */
	uint32_t pick_top;          /* top of the pick */
	sx_point_pattern search;    /* the search points, in cells */
} sx_mission_config;

/* Detours (sx_detour.kind): a short break from the plan. */
#define SX_DETOUR_ENGAGE 0 /* go for subject */
#define SX_DETOUR_FLEE   1 /* get away from at, to to if has_to */
#define SX_DETOUR_CUSTOM 2 /* the game's detour custom at at with param; ends on
                            * sx_mission_end_detour */

typedef struct sx_detour {
	uint32_t kind;              /* SX_DETOUR_* */
	uint32_t subject;           /* ENGAGE */
	sx_where at;                /* FLEE: what to get away from; CUSTOM: where */
	int32_t to[3];              /* FLEE: where to run to; valid if has_to */
	uint32_t custom;            /* CUSTOM: the game's kind */
	uint32_t param;             /* CUSTOM */
	uint8_t has_to;
} sx_detour;

/* What the character should do now (sx_mission_action.kind). */
#define SX_ACTION_NONE   0
#define SX_ACTION_GO_TO  1 /* walk to pos */
#define SX_ACTION_WAIT   2 /* stand and wait */
#define SX_ACTION_FOLLOW 3 /* follow subject, last known at pos */
#define SX_ACTION_HOLD   4 /* hold pos */
#define SX_ACTION_ATTACK 5 /* attack subject, last known at pos */
#define SX_ACTION_SEARCH 6 /* search at pos */
#define SX_ACTION_WANDER 7 /* wander about */
#define SX_ACTION_FLEE   8 /* run to pos */
#define SX_ACTION_CUSTOM 9 /* the game's step or detour custom, at pos if has_pos */

typedef struct sx_mission_action {
	uint32_t kind;              /* SX_ACTION_* */
	int32_t pos[3];             /* valid if has_pos */
	uint32_t subject;           /* FOLLOW, ATTACK */
	uint32_t custom;            /* CUSTOM: the game's kind */
	uint32_t param;             /* CUSTOM */
	uint8_t has_pos;
} sx_mission_action;

/* What happened during a tick (sx_mission_event.kind), with a and b: */
#define SX_MISSION_EVENT_STEP_DONE      0  /* step a is done */
#define SX_MISSION_EVENT_LOOPED         1  /* the plan started again */
#define SX_MISSION_EVENT_PLAN_DONE      2  /* the plan is done */
#define SX_MISSION_EVENT_BROKEN         3  /* the plan broke, a = SX_BREAK_* (fail: the
                                            * condition for SX_BREAK_FAIL) */
#define SX_MISSION_EVENT_REPLANNED      4  /* the plan of decider option a was taken */
#define SX_MISSION_EVENT_FELL_BACK      5  /* fell back to the default plan or to
                                            * holding where it stands */
#define SX_MISSION_EVENT_DETOUR_STARTED 6
#define SX_MISSION_EVENT_DETOUR_ENDED   7  /* a = SX_DETOUR_END_* */
#define SX_MISSION_EVENT_RETURNED       8  /* back at the anchor, the plan resumes */
#define SX_MISSION_EVENT_SIGNAL         9  /* signal a was given */
#define SX_MISSION_EVENT_CALL           10 /* call on channel a, kind b, from pos, with
                                            * the caller's motive */
#define SX_MISSION_EVENT_WANTS_SQUAD    11 /* squad tactic a, against pos (has_pos) or
                                            * subject (has_subject) */

/* Why a plan broke (a of SX_MISSION_EVENT_BROKEN). */
#define SX_BREAK_FAIL       0 /* a fail condition fired */
#define SX_BREAK_IMPOSSIBLE 1 /* a step could not be done */
#define SX_BREAK_PLAN_ENDED 2 /* the plan ended */

/* Why a detour ended (a of SX_MISSION_EVENT_DETOUR_ENDED). */
#define SX_DETOUR_END_DEFEATED  0 /* the subject was defeated */
#define SX_DETOUR_END_FORGOTTEN 1 /* the subject is no longer known */
#define SX_DETOUR_END_LOST      2 /* the subject was lost */
#define SX_DETOUR_END_LEASH     3 /* the leash ran out */
#define SX_DETOUR_END_SAFE      4 /* the fleeing character is safe */
#define SX_DETOUR_END_ARRIVED   5 /* the fleeing character arrived */
#define SX_DETOUR_END_ENDED     6 /* sx_mission_end_detour */

typedef struct sx_mission_event {
	uint32_t kind;              /* SX_MISSION_EVENT_* */
	uint32_t a, b;
	uint32_t subject;           /* WANTS_SQUAD; valid if has_subject */
	int32_t pos[3];             /* CALL, WANTS_SQUAD; valid if has_pos */
	uint8_t has_pos;
	uint8_t has_subject;
	sx_fail fail;               /* BROKEN with SX_BREAK_FAIL */
	sx_motive motive;           /* CALL: the caller's motive (since version 12) */
} sx_mission_event;

/* What one tick gives: the action and the events in order. */
typedef struct sx_mission_out {
	sx_mission_action action;
	uint32_t event_count;
	sx_mission_event events[8];
} sx_mission_out;

/* Id 0, 16 steps, 8 playbook slots, subject kind 0, enemy bits 1, defeated
 * bits 2, arrive radius 0, lost after 30, pause 50, safe distance 12, reached
 * signal 0, SX_METRIC_CHEBYSHEV, square, no extra columns, SX_PICK_BEST, and a
 * search pattern in cells: heading step 0x62, first distance 1, offsets 0,
 * 0x20, -0x20 at distances 2, 4, 6, SX_SINE_FINE, unclamped (set min and max
 * for a bounded map). */
int32_t sx_mission_config_default(sx_mission_config *out);
/* The leeway of SX_LEEWAY_* to *out. */
int32_t sx_leeway_preset(uint32_t which, sx_leeway *out);

/* A mission for motive, without a plan, with SX_LEEWAY_FLEXIBLE. Null for a
 * null pointer, an unknown code, max_steps 0 or above 65536, max_playbook
 * above 65535, a search pattern with more than 8 entries, or a hex grid
 * without SX_METRIC_HEX. */
sx_mission *sx_mission_new(const sx_mission_config *config, const sx_motive *motive);
/* Frees the mission and the decider handle it lent. */
void sx_mission_free(sx_mission *mission);
int32_t sx_mission_set_leeway(sx_mission *mission, const sx_leeway *leeway);
/* The running plan stays. */
int32_t sx_mission_set_motive(sx_mission *mission, const sx_motive *motive);
/* The motive and the leeway to *out (since version 12). SX_ERROR_ARGUMENT
 * for a null pointer. */
int32_t sx_mission_motive(const sx_mission *mission, sx_motive *out);
int32_t sx_mission_leeway(const sx_mission *mission, sx_leeway *out);

/* A plan: count steps (repeat 1: start again after the last) and fail_count
 * conditions for the whole plan (at most 4; fails may be null if 0). The
 * mission copies it; its first step begins at the next tick, whatever ran
 * before (during a detour, when the character resumes). SX_ERROR_ARGUMENT
 * for no steps, more than max_steps, more than 4 conditions or an unknown
 * code (the mission keeps what it had). */
int32_t sx_mission_set_plan(sx_mission *mission, const sx_step *steps, uint32_t count,
                            uint32_t repeat, const sx_fail *fails, uint32_t fail_count);
/* A plan for the playbook, as for sx_mission_set_plan, in the first free
 * slot. preference (0 .. SX_ONE, more is cut) is its SX_MISSION_IN_PREF
 * column; widening 1: a candidate only for a character that may widen, and
 * only on a real break. Returns the slot, which is its decider option;
 * SX_ERROR_ARGUMENT also when the playbook is full. */
int32_t sx_mission_add_playbook(sx_mission *mission, const sx_step *steps, uint32_t count,
                                uint32_t repeat, const sx_fail *fails, uint32_t fail_count,
                                uint32_t preference, uint32_t widening);
/* Empties every slot; the decider keeps its considerations. A running
 * playbook plan runs on. */
int32_t sx_mission_clear_playbook(sx_mission *mission);

/* A plan as sx_mission_plan reads it back (since version 12). */
typedef struct sx_mission_plan_info {
	uint32_t step_count;        /* steps of the plan */
	uint32_t fail_count;        /* used entries of fails */
	sx_fail fails[4];           /* conditions that break the whole plan */
	uint8_t repeat;             /* 1: starts again after the last step */
} sx_mission_plan_info;

/* The plan that runs, or ran last (also after it ended or broke): the one
 * set by sx_mission_set_plan, a playbook plan, the default or the widening
 * plan, or the hold of a fallback. Its summary goes to *out (may be null),
 * its first cap steps to steps (may be null if cap is 0), as
 * sx_mission_set_plan takes them (unused fields 0). Returns 1, or 0 if
 * there is none yet (nothing written). */
int32_t sx_mission_plan(const sx_mission *mission, sx_mission_plan_info *out, sx_step *steps,
                        uint32_t cap);

/* Runs one tick at now for a character standing at pos (3 values), from what
 * it knows and the game's condition bits flags. The action and the events go
 * to *out (out may be null). Returns the number of events (at most
 * SX_MISSION_MAX_EVENTS), or SX_ERROR_ARGUMENT for a null mission, pos or
 * knowledge.
 *
 * Each tick checks the fail conditions of the running step, then of the plan;
 * a fail breaks the plan. Then the step runs: one that needs a position that
 * is not known breaks the plan (or is skipped with skip_if_impossible); a
 * step that is done hands over to the next within the tick. After the last
 * step a looping plan starts again; else the plan is done, which counts as a
 * break for replanning (a plan ending in SX_STEP_WANDER wanders instead).
 * CALL and SQUAD steps give their event only to a character that may widen.
 *
 * On a break: with leeway.replan the candidates (the filled playbook slots,
 * the motive's default plan, and for a character that may widen and only on
 * a real break the widening slots and the built-in widening plan) are scored
 * by the mission's decider and the winner starts at once
 * (SX_MISSION_EVENT_REPLANNED); when every candidate scores 0, the default
 * plan if it is feasible, else holding where it stands
 * (SX_MISSION_EVENT_FELL_BACK). A playbook plan that just ended or broke is
 * no candidate on that tick. While it holds after a fallback the character
 * looks again each tick (no widening plans) and takes a winner with
 * SX_MISSION_EVENT_REPLANNED; while none wins it holds on without an event.
 * Without replan: holding where it stands. The
 * default plan: GUARD follows its subject or holds its place, DEFEAT attacks
 * its subject (or the nearest known enemy) when seen within lost_after, else
 * searches its last known place, REACH goes there, gives reached_signal and
 * holds, CUSTOM has none. Without a plan yet the tick does what a break does.
 * During a detour the detour runs and no fail condition is checked. */
int32_t sx_mission_tick(sx_mission *mission, uint32_t now, const int32_t *pos,
                        const sx_knowledge *knowledge, uint64_t flags, sx_mission_out *out);

/* Rules: the game's logic at fixed points, lent to one tick
 * (sx_mission_tick_with; rules NULL is the plain tick). Each callback may be
 * NULL: that slot does what the mission does without rules (a zeroed
 * sx_mission_rules is all built-in). user is passed to every callback. A
 * callback must not call any function on the same mission or on its decider
 * (sx_mission_decider). With a hex grid
 * every position in a context is in offset coordinates.
 *   fail     1: the game's condition id (SX_FAIL_CUSTOM) holds now and breaks
 *            the step (has_step) or plan. During a replan it is asked for a
 *            candidate's conditions (has_candidate, candidate = its option).
 *            Built-in: never.
 *   columns  fill the count game columns of a candidate's row (all 0 when
 *            called; count is config.extra_columns). Built-in: nothing.
 *   score    1 and the score in *score (preset to built_in, SX_ONE = 1.0; 0
 *            vetoes) replaces the candidate's score, also for one that is not
 *            feasible (so veto one that is not, or that cannot run); 0 keeps
 *            built_in. When every candidate ends at 0 the fallback still
 *            applies. Built-in: keep.
 * Order of calls, always the same: each tick while a plan runs, fail for the
 * running step's custom conditions, then the plan's, in list order, stopping
 * at the first condition that fires. On a replan, per candidate in option
 * order: fail for the custom conditions of its first step, then of the plan,
 * then columns, then score (also each tick while holding after a fallback
 * with replan). */
typedef struct sx_mission_fail_ctx {
	uint32_t now;
	int32_t pos[3];             /* where the character stands */
	uint32_t step;              /* the step the condition belongs to; valid if has_step */
	uint32_t candidate;         /* the candidate's option during a replan; valid if has_candidate */
	sx_motive motive;
	uint8_t has_step;           /* 0: a condition of the whole plan */
	uint8_t has_candidate;      /* 1: a candidate's condition, 0: of the running plan */
} sx_mission_fail_ctx;

typedef struct sx_mission_plan_ctx {
	uint32_t now;
	int32_t pos[3];             /* where the character stands */
	uint32_t option;            /* the candidate's decider option */
	int32_t first[3];           /* its first position; valid if has_first */
	sx_motive motive;
	uint8_t widening;           /* 1: a widening plan */
	uint8_t feasible;           /* 1: its conditions hold now and first can be filled */
	uint8_t has_first;
} sx_mission_plan_ctx;

typedef struct sx_mission_rules {
	int32_t (*fail)(void *user, const sx_mission_fail_ctx *ctx, uint32_t id);
	void (*columns)(void *user, const sx_mission_plan_ctx *ctx, int32_t *out, uint32_t count);
	int32_t (*score)(void *user, const sx_mission_plan_ctx *ctx, uint32_t built_in,
	                 uint32_t *score);
	void *user;
} sx_mission_rules;

/* As sx_mission_tick with the game's rules (NULL = the plain tick). */
int32_t sx_mission_tick_with(sx_mission *mission, uint32_t now, const int32_t *pos,
                             const sx_knowledge *knowledge, uint64_t flags,
                             const sx_mission_rules *rules, sx_mission_out *out);

/* Starts a detour at now (the game decides when, from its own perception).
 * Returns 1 if taken, 0 if refused: an ENGAGE of a subject that is not known
 * or, with a leash distance, whose last known place is farther than that from
 * the anchor. The plan waits until the detour ends, then the character walks
 * back to the anchor (where it stood at the last tick, or the place it held)
 * and resumes (SX_MISSION_EVENT_RETURNED). A new detour replaces a running
 * one; the anchor stays. The next tick gives SX_MISSION_EVENT_DETOUR_STARTED.
 * ENGAGE ends when the subject is defeated or forgotten, the leash runs out,
 * or the subject was not seen for lost_after ticks (then on_lost decides);
 * FLEE when it arrives at to or is farther than safe_distance from at
 * (without to it runs away from at; standing on at, safe_distance + 1 cells
 * along +x, or -x at the bound); CUSTOM only by sx_mission_end_detour. SX_ERROR_ARGUMENT also for an unknown
 * code. */
int32_t sx_mission_detour(sx_mission *mission, uint32_t now, const sx_detour *detour,
                          const sx_knowledge *knowledge);
/* Ends the running detour at the next tick (SX_DETOUR_END_ENDED). Ignored
 * when no detour runs. */
int32_t sx_mission_end_detour(sx_mission *mission);
/* Marks the running SX_STEP_CUSTOM step as done; the plan moves on at the
 * next tick. Other steps ignore it. */
int32_t sx_mission_finish_step(sx_mission *mission);
/* 1 and the index of the running step to *index while a plan runs, 0 during
 * a detour, the walk back or without a plan. */
int32_t sx_mission_step_index(const sx_mission *mission, uint32_t *index);
/* 1 while a detour runs, else 0. */
int32_t sx_mission_in_detour(const sx_mission *mission);
/* 1 while the character walks back to its anchor after a detour, else 0. */
int32_t sx_mission_returning(const sx_mission *mission);

/* The decider that scores the candidate plans of a replan, lent until
 * sx_mission_free (sx_decider_free does nothing to it; one handle per
 * mission; null for a null mission). Option i below max_playbook is playbook
 * slot i, then come the default plan (max_playbook) and the built-in widening
 * plan (max_playbook + 1). Each option has two built-in considerations, both
 * SX_APPLY_MULTIPLY: a step on SX_MISSION_IN_FEASIBLE and SX_MISSION_IN_PREF
 * linear over 0 .. SX_ONE. A row has SX_MISSION_IN_EXTRA built-in columns,
 * then config.extra_columns of the game's. A consideration on an input beyond
 * the row makes its candidate score 0; options added beyond the built-in ones
 * are never scored. */
sx_decider *sx_mission_decider(sx_mission *mission);

/* The built-in columns of a candidate's row. */
#define SX_MISSION_IN_FEASIBLE 0 /* 1 if its conditions hold and its first position
                                  * can be filled, else 0 */
#define SX_MISSION_IN_DIST     1 /* distance to its first position in the metric
                                  * (Euclid: the rounded-down root), at most
                                  * SX_MISSION_DIST_CAP; 0 when no step names one */
#define SX_MISSION_IN_PREF     2 /* its preference (0 .. SX_ONE) */
#define SX_MISSION_IN_WIDEN    3 /* 1 for a widening plan */
#define SX_MISSION_IN_KNOWN    4 /* age of the freshest fact of the subjects its
                                  * steps name, at most SX_MISSION_KNOWN_CAP; 0 when
                                  * it names none, the cap when none is known */
#define SX_MISSION_IN_EXTRA    5 /* the first of the game's columns */
#define SX_MISSION_DIST_CAP    1024
#define SX_MISSION_KNOWN_CAP   65535
/* Preference of the default plan and of the built-in widening plan. */
#define SX_MISSION_DEFAULT_PREFERENCE 32767
#define SX_MISSION_WIDEN_PREFERENCE   49149

/* Tracing a mission (since version 12): why a plan won, and where each
 * step goes. Each replan scoring sends, per candidate in option order, its
 * row (one COLUMN per column, after the columns callback), the decider's
 * records of it (DECIDE: its factors and its score, candidate always 0) and
 * its score after the score callback (RESCORED), then the winner (PICKED).
 * A tick may score twice (holding after a fallback it looks again, then its
 * plan breaks). Each step that begins sends BEGIN. Fields that do not apply
 * to the kind are 0. The actions and events are the same with tracing on or
 * off. */
#define SX_MISSION_TRACE_COLUMN   1 /* column of the candidate's row has value */
#define SX_MISSION_TRACE_DECIDE   2 /* decide: a decider record of the candidate */
#define SX_MISSION_TRACE_RESCORED 3 /* built_in: the decider's score (0 when a
                                     * consideration reads beyond the row), score:
                                     * after the score callback */
#define SX_MISSION_TRACE_PICKED   4 /* the winner option and its score; has_option 0
                                     * when every candidate scored 0 (the fallback
                                     * applies) */
#define SX_MISSION_TRACE_BEGIN    5 /* step index begins; at: the place its where
                                     * stands for now, if has_at (0 for a step without
                                     * one or an unknown place) */

typedef struct sx_mission_trace {
	uint32_t kind;              /* SX_MISSION_TRACE_* */
	uint32_t option;            /* COLUMN, DECIDE, RESCORED, PICKED: the candidate's
	                             * decider option, if has_option */
	uint32_t column;            /* COLUMN */
	uint32_t built_in;          /* RESCORED */
	uint32_t score;             /* RESCORED, PICKED */
	uint32_t index;             /* BEGIN: the step */
	int32_t value;              /* COLUMN */
	int32_t at[3];              /* BEGIN */
	uint8_t has_option;
	uint8_t has_at;
	sx_decide_trace decide;     /* DECIDE */
} sx_mission_trace;

/* trace gets every record of the following ticks, with context; NULL turns
 * it off (the default). trace must not call any function on the same
 * mission or on its decider (sx_mission_decider). Returns 0. */
typedef void (*sx_mission_trace_fn)(void *context, const sx_mission_trace *trace);
int32_t sx_mission_set_trace(sx_mission *mission, sx_mission_trace_fn trace, void *context);

/* ---- perception (since version 13) --------------------------------------- */

/* What a character perceives: view cones, exact integer sight lines with
 * fog, smoke and light, attention, and hearing, on square and hex grids.
 * Everything is integer only and deterministic.
 *
 * One handle (sx_perception) holds the world: the grid (SX_GRID_*), the cell
 * size S in fine units (at least 2, e.g. 256), the hearing channels, the
 * game's sense map and the trace. Everything per character is a value struct
 * the game keeps (zero to start): sx_sense_profile, sx_awareness (one per
 * observer-target pair), sx_attention (one per observer).
 *
 * Positions at the C edge:
 *   - Fine points are int32_t[3] [x, y, level] in fine units: sx_eye.point,
 *     sx_target.point, sx_stimulus.point, sx_percept.point and the from / to
 *     of sx_perception_line. On a hex grid they are AXIAL (q, r scaled by S;
 *     the centre of hex (q, r) is (q * S, r * S)) and are never
 *     offset-converted. A square cell (x, y) spans [x * S, (x + 1) * S); its
 *     centre is (x * S + S / 2, y * S + S / 2).
 *   - Whole cells are [x, y, level]: sx_percept.cell, the facts that
 *     sx_percept_fact and sx_learn_percepts write, the flood cells, the cell
 *     helpers, sx_line.blocked_at, the trace cells and the cells the sense map
 *     callbacks get. On a hex grid they are OFFSET cells of the handle's
 *     layout, converted as whole cells by the handle (as the mission does).
 *   - A game that has only cells passes sx_perception_cell_centre(cell) as
 *     the fine point.
 *
 * Callbacks run on the calling thread and must not call any function on the
 * same sx_perception (the handle holds its map while it calls them). */

typedef struct sx_perception sx_perception;

#define SX_MAX_CHANNELS            4   /* hearing channels of a world */
#define SX_MAX_CUES                8   /* cue rules of a sense profile */
#define SX_MAX_PERCEPTS_PER_CHECK  4   /* percepts one check of one target makes */

/* ---- the game's sense map ---- */

/* What a sight line meets inside one cell. density: fog or smoke, optical
 * depth per full cell length (256 halves the clarity per cell; it adds up
 * along the line by length); heat_density: the same for thermal sight. */
typedef struct sx_medium {
	uint8_t opaque;             /* 1: stops sight (a wall, a closed door) */
	uint8_t _pad;
	uint16_t density;
	uint16_t heat_density;
	uint16_t _pad2;
} sx_medium;

/* Inside cell x, y, level: write the medium to *out (it holds clear air when
 * called). The return value is ignored (return 0). */
typedef int32_t (*sx_sight_fn)(void *context, int32_t x, int32_t y, int32_t level, sx_medium *out);
/* Crossing from cell x, y into its neighbour to_x, to_y on the same level
 * (thin walls, windows, doors): return 1 if sight passes, 0 if not. */
typedef int32_t (*sx_sight_edge_fn)(void *context, int32_t x, int32_t y, int32_t level,
	int32_t to_x, int32_t to_y);
/* A line between two levels, from and to the fine points (3 integers each):
 * return 1 and write the medium over the whole length to *out (it holds
 * clear air), or 0: blocked. */
typedef int32_t (*sx_cross_level_fn)(void *context, const int32_t *from, const int32_t *to,
	sx_medium *out);
/* Light at a cell, 0 (dark) .. 255 (daylight); values outside are clamped. */
typedef int32_t (*sx_light_fn)(void *context, int32_t x, int32_t y, int32_t level);
/* Sound damping inside a cell, fine units per cell length (negative: 0). */
typedef int32_t (*sx_sound_fn)(void *context, int32_t x, int32_t y, int32_t level);
/* Sound damping crossing into a neighbour, fine units; -1 (any negative):
 * the sound does not pass at all. */
typedef int32_t (*sx_sound_edge_fn)(void *context, int32_t x, int32_t y, int32_t level,
	int32_t to_x, int32_t to_y);

/* One optional callback per question (0: the default) and one context, the
 * same shape as sx_map. Sight is separate from movement. */
typedef struct sx_sense_map {
	sx_sight_fn sight;             /* 0: clear air */
	sx_sight_edge_fn sight_edge;   /* 0: open */
	sx_cross_level_fn cross_level; /* 0: blocked */
	sx_light_fn light;             /* 0: 255 */
	sx_sound_fn sound;             /* 0: no damping */
	sx_sound_edge_fn sound_edge;   /* 0: no damping */
	void *context;                 /* passed to all callbacks */
} sx_sense_map;

/* Ready sense maps. From a grid: blocked cells and cells off the grid are
 * opaque, nothing else (no fog, no thin walls). From a map: sight is blocked
 * where movement is (crossing into a neighbour is open exactly when step
 * allows it on the same level), for games that share one blocker. Both are
 * valid while the given grid or struct lives; on a hex grid they read it in
 * offset cells, like every callback. Return 0, or SX_ERROR_ARGUMENT for a
 * null pointer or a map without step. */
int32_t sx_sense_map_from_grid(const sx_grid *grid, sx_sense_map *out);
int32_t sx_sense_map_from_map(const sx_map *map, sx_sense_map *out);

/* ---- the world ---- */

#define SX_SPREAD_CIRCLE   0  /* straight distance only, walls do not matter */
#define SX_SPREAD_STRAIGHT 1  /* distance plus the map's damping along one line */

/* One hearing channel (sound, smell, smoke, ...). */
typedef struct sx_channel {
	uint16_t damping;           /* how much the map's sound damping counts, per 256 */
	uint16_t fuzz;              /* position error in cells per cell of cost, per 256 */
	int32_t speed;              /* fine units per tick (0: arrives at once) */
	uint32_t spread;            /* SX_SPREAD_* */
} sx_channel;

typedef struct sx_perception_config {
	uint32_t grid;              /* SX_GRID_SQUARE, SX_GRID_HEX_* */
	int32_t cell_size;          /* fine units per cell or hex, >= 2 */
	sx_channel channels[SX_MAX_CHANNELS];
} sx_perception_config;

/* Fills out for grid and cell_size, every channel with damping 256, fuzz 0,
 * speed 0, SX_SPREAD_CIRCLE. Returns 0, or SX_ERROR_ARGUMENT for an unknown
 * grid, a cell size below 2 or a null out. */
int32_t sx_perception_config_default(uint32_t grid, int32_t cell_size, sx_perception_config *out);

/* Null for a null or invalid config (unknown grid or spread, cell size
 * below 2). map 0: no map (clear, lit, silent). The map struct is copied;
 * its callbacks and context must stay valid while it is set. */
sx_perception *sx_perception_new(const sx_perception_config *cfg, const sx_sense_map *map);
void sx_perception_free(sx_perception *p);
/* Replaces the sense map (0: no map). Returns 0. */
int32_t sx_perception_set_map(sx_perception *p, const sx_sense_map *map);

/* ---- sense profiles, stats, targets ---- */

/* Bits of sx_sense_profile.senses (flags; not the SX_SENSE_* percept codes). */
#define SX_SENSE_FLAG_SIGHT    1  /* normal sight (light, density, bands) */
#define SX_SENSE_FLAG_NIGHT    2  /* light counts as at least bright */
#define SX_SENSE_FLAG_THERMAL  4  /* a second pass by heat (heat_density, target heat) */
#define SX_SENSE_FLAG_SCANNER  8  /* within scanner_range, no walls, bands or light; only noticed */

/* One view band: a half-angle around the facing (headings 0 .. 255 are a full
 * turn; 43 is a 120 degree cone). The first band that holds the target
 * counts; outside all it is unseen. */
typedef struct sx_band {
	uint8_t half_angle;         /* 0 .. 128 */
	uint16_t gain;              /* per 256; 0: unused */
	uint8_t instant;            /* 1: instant cues fire in this band */
} sx_band;

/* What a cue bit of a target (the game's bits: drawn weapon, attacking, a
 * body) does for this observer. An instant fires from the sight pass only
 * (never the thermal pass) and only while sight sees the target: none of
 * band, distance, light, clarity, size and conspicuity is 0 (so not in the
 * dark with dark_gain 0; SX_SENSE_FLAG_NIGHT lifts the light, then it
 * fires). A product that only rounds to 0 still fires. sx_perception_look
 * picks instant targets by cue bits alone: in the dark or for a
 * thermal-only eye such a pick uses an instant_cap slot without firing. */
typedef struct sx_cue_rule {
	uint32_t bits;              /* any of them matches; 0: unused rule */
	uint16_t gain;              /* conspicuity factor per 256 while shown (0: none) */
	uint8_t instant;            /* 0 no, 1 to noticed, 2 to recognised at once */
	uint16_t min_clarity;       /* instant only with at least this clarity (per 256) */
	int32_t range;              /* acts only within this distance (0: sight range) */
} sx_cue_rule;

/* What an observer can sense and how fast it notices (plain data; copy and
 * change it per observer or tick). Distances in fine units, factors per 256.
 * Stages: 0 unseen, 1 noticed (counter >= notice_at), 2 recognised
 * (>= recognize_at). Start from sx_sense_profile_preset. */
typedef struct sx_sense_profile {
	uint8_t senses;             /* SX_SENSE_FLAG_* */
	int32_t range;              /* sight range (before stats.sight_scale) */
	int32_t near;               /* full gain up to here */
	uint16_t far_gain;          /* gain at range, linear from near */
	sx_band bands[3];           /* ascending half-angles */
	uint16_t base_gain;         /* counter gain per tick at best conditions */
	uint8_t dark;               /* light at or below: dark_gain */
	uint8_t bright;             /* light at or above: full gain */
	uint16_t dark_gain;
	uint16_t notice_at;
	uint16_t recognize_at;
	uint16_t max;               /* highest counter */
	uint16_t decay_per_tick;    /* lost per credited tick once hold is over */
	uint16_t hold;              /* ticks after the last sighting without decay */
	uint16_t hysteresis;        /* how far below a threshold a stage falls */
	uint16_t check_interval;    /* most ticks one check credits (0 counts as 1) */
	uint16_t crowd_gain[8];     /* by target crowd, index min(crowd, 7) */
	int32_t scanner_range;      /* with SX_SENSE_FLAG_SCANNER; 0: off */
	uint16_t scanner_gain;      /* counter gain per tick of the scanner */
	uint16_t hearing[SX_MAX_CHANNELS]; /* range scale per channel (0: deaf on it) */
	uint16_t hear_gain;         /* counter gain per heard stimulus at full margin */
	sx_cue_rule cues[SX_MAX_CUES]; /* in index order; unused ones have bits 0 */
} sx_sense_profile;

#define SX_SENSE_PRESET_HUMAN   1u  /* range 12 cells, focus 43, periphery 71 */
#define SX_SENSE_PRESET_GUARD   2u  /* human with range 14 cells, quicker */
#define SX_SENSE_PRESET_WATCHER 3u  /* human with a wide periphery, slower recognition */

/* Fills out with preset (SX_SENSE_PRESET_*) for cell_size fine units per
 * cell: SIGHT only, no cue rules, hears channel 0 at full range. Returns 0,
 * or SX_ERROR_ARGUMENT for an unknown preset or a null out. Add senses, cue
 * rules and hearing by setting the fields. */
int32_t sx_sense_profile_preset(uint32_t preset, int32_t cell_size, sx_sense_profile *out);

/* The observer's stats, passed on every call (a game stat or a boost acts
 * without touching the profile). Everything at 256 and 0 is normal. */
typedef struct sx_stats {
	uint16_t sight_scale;       /* range x / 256 */
	uint16_t notice_scale;      /* gain x / 256 */
	uint16_t hearing_scale;     /* hearing range x / 256 */
	uint16_t reaction_ticks;    /* ticks before a new stage counts, read on every call */
} sx_stats;

/* How an observer sees through a disguise (only when has_see_through). */
typedef struct sx_see_through {
	uint16_t rate;              /* doubt per credited tick at disguise 0, full closeness */
	int32_t near;               /* full closeness up to here, 0 at the sight range */
	uint16_t at;                /* doubt at which the disguise no longer holds */
} sx_see_through;

/* A target with disguise > 0 is perceived, flagged disguised, and the cue
 * bits in hides are never reported. Without see-through that never changes. */
typedef struct sx_disguise_rule {
	uint32_t hides;
	sx_see_through see_through;
	uint8_t has_see_through;    /* 0: a disguise always holds */
} sx_disguise_rule;

/* One observer (or listener) for one call. */
typedef struct sx_eye {
	uint32_t id;                /* becomes sx_percept.observer */
	int32_t point[3];           /* fine point (axial on hex) */
	int32_t facing;             /* heading, taken modulo 256 (0 = +x, 64 = +y) */
	const sx_sense_profile *profile; /* required */
	sx_stats stats;
	sx_disguise_rule disguise;
} sx_eye;

/* Something that could be perceived, from the game's own candidate list. */
typedef struct sx_target {
	uint32_t id;                /* becomes sx_percept.subject */
	int32_t point[3];           /* the point the eye aims at, fine (axial on hex) */
	uint16_t size;              /* per 256: a person 256, crouching 128 */
	uint16_t conspicuity;       /* per 256: still 128, walking 256, firing 2048 */
	uint32_t cues;              /* the game's bits it shows now */
	uint8_t disguise;           /* 0 none, 1 .. 255 strength */
	uint8_t heat;               /* for thermal sight (0: cold) */
	uint8_t glow;               /* own light: light = max(cell light, glow) */
	uint8_t crowd;              /* how many others it blends with */
} sx_target;

/* What one observer knows of one target. Keep one per pair, start it zeroed
 * (= {0}); the fields are private (do not touch, read them through the
 * sx_awareness_* functions). A zeroed one was last checked at tick 0: start
 * your clock at 1, or the first check at tick 0 credits nothing. */
typedef struct sx_awareness {
	uint16_t counter;           /* private */
	uint16_t doubt;             /* private */
	uint8_t stage;              /* private */
	uint8_t pending;            /* private */
	uint8_t flags;              /* private */
	uint8_t _pad;               /* private */
	uint32_t last;              /* private */
	uint32_t pending_since;     /* private */
	uint32_t last_seen;         /* private */
	uint32_t cues;              /* private */
} sx_awareness;

/* Stage 0 unseen, 1 noticed, 2 recognised; the counter; 1 if in view at the
 * last check; the last tick seen by sight or thermal (int64_t); 1 if the
 * disguise was seen through; reset to zero. Null: SX_ERROR_ARGUMENT. */
int32_t sx_awareness_stage(const sx_awareness *a);
int32_t sx_awareness_counter(const sx_awareness *a);
int32_t sx_awareness_in_view(const sx_awareness *a);
int64_t sx_awareness_last_seen(const sx_awareness *a);
int32_t sx_awareness_seen_through(const sx_awareness *a);
int32_t sx_awareness_reset(sx_awareness *a);

/* Where one observer's round robin goes on. Start it zeroed; the field is
 * private. */
typedef struct sx_attention {
	uint32_t cursor;            /* private */
} sx_attention;

/* The index the next round robin starts from (int64_t), or SX_ERROR_ARGUMENT. */
int64_t sx_attention_cursor(const sx_attention *a);

/* How many targets one sx_perception_look checks. */
typedef struct sx_budget {
	uint16_t checks;            /* kept and round-robin targets together */
	uint16_t keep;              /* of checks, at most this many already noticed */
	uint16_t instant_cap;       /* targets with an instant cue, on top of checks */
} sx_budget;

/* Something that can be heard (or smelt): a sound, a shout, a shot, smoke. */
typedef struct sx_stimulus {
	uint32_t id;                /* the game's number (an input of the fuzz) */
	uint32_t source;            /* who made it (becomes sx_percept.subject) */
	uint32_t kind;              /* the game's kind */
	int32_t point[3];           /* fine point (axial on hex) */
	int32_t loudness;           /* audible range in fine units for a normal listener */
	uint8_t channel;            /* 0 .. SX_MAX_CHANNELS - 1 */
	uint8_t disguise;           /* the source's disguise (as sx_target.disguise); 0 none */
	uint32_t at;                /* when it was made */
} sx_stimulus;

/* ---- outputs ---- */

#define SX_SENSE_SIGHT    0  /* also for LOST and FORGOTTEN */
#define SX_SENSE_THERMAL  1
#define SX_SENSE_SCANNER  2
#define SX_SENSE_HEARING  3  /* sx_percept.channel is the channel */

#define SX_PERCEPT_NOTICED       0  /* the stage rose from unseen */
#define SX_PERCEPT_RECOGNIZED    1  /* the stage rose to recognised */
#define SX_PERCEPT_SEEN          2  /* noticed or higher and in view: its place now */
#define SX_PERCEPT_CUES          3  /* new_cues: recognised cue bits not reported before */
#define SX_PERCEPT_LOST          4  /* in view at the last check, not now */
#define SX_PERCEPT_FORGOTTEN     5  /* the stage fell to unseen */
#define SX_PERCEPT_SEEN_THROUGH  6  /* the observer saw through the disguise */
#define SX_PERCEPT_HEARD         7  /* a stimulus was heard */

/* One perceived event. point and cell are a real position from this call
 * only when placed is 1; otherwise both are [0, 0, 0] and clarity is 0. */
typedef struct sx_percept {
	uint32_t observer;
	uint32_t subject;           /* the target, or the stimulus source */
	uint32_t sense;             /* SX_SENSE_* */
	uint32_t channel;           /* SX_SENSE_HEARING: the channel */
	uint32_t event;             /* SX_PERCEPT_* */
	uint32_t new_cues;          /* SX_PERCEPT_CUES: the new bits */
	int32_t point[3];           /* fine point (axial on hex) */
	int32_t cell[3];            /* whole cell (offset on hex) */
	uint32_t cues;              /* cue bits seen (stage recognised; masked ones never) */
	int32_t margin;             /* hearing: how clearly heard */
	uint32_t arrive_at;         /* hearing: when it arrives */
	uint32_t at;                /* the time of the check */
	uint16_t clarity;           /* 0 .. 256 */
	uint8_t stage;              /* the counted stage after the check */
	uint8_t placed;
	uint8_t disguised;          /* 1: the target is disguised for this observer */
} sx_percept;

/* ---- geometry ---- */

/* Whole cell <-> fine point. On hex grids the cell is in OFFSET coordinates
 * here (the handle converts the whole cell with its layout); the fine point
 * is always axial and is never offset-converted. out: 3 integers. Return 0,
 * or SX_ERROR_ARGUMENT for a null pointer. */
int32_t sx_perception_cell_centre(sx_perception *p, sx_point cell, int32_t *out);
int32_t sx_perception_cell_of(sx_perception *p, const int32_t *point, sx_point *out);
/* The heading (0 .. 255) of hex direction dir (modulo 6; 0 = +q, 1 = +r):
 * 0, 43, 85, 128, 171, 213. */
int32_t sx_hex_heading(uint32_t dir);

/* The answer of a sight line. */
typedef struct sx_line {
	uint8_t clear;              /* 1: sight gets through */
	uint8_t has_blocked_at;     /* 1: blocked_at is set */
	uint16_t clarity;           /* 0 .. 256 (256: clear air; 0 when not clear) */
	int32_t blocked_at[3];      /* the opaque cell, or the cell behind a closed edge */
} sx_line;

/* Whether sight gets from the fine point from to to and how clearly: an
 * exact walk over every cell between them, fog by length, the same from both
 * ends. The eye's cell never blocks; the target's only through its edge.
 * Returns 0, or SX_ERROR_ARGUMENT for a null pointer. */
int32_t sx_perception_line(sx_perception *p, const int32_t *from, const int32_t *to, sx_line *out);
/* 1 if eye could see the fine point pos now (range with the stats, band,
 * line; no awareness, no time, no light), else 0; SX_ERROR_ARGUMENT for a
 * null pointer. For rule slots. */
int32_t sx_perception_in_sight(sx_perception *p, const sx_eye *eye, const int32_t *pos);

/* ---- checks ---- */

/* Each returns the number of percepts written (<= cap; out may be 0 when cap
 * is 0), *total (may be 0) how many there were; SX_ERROR_ARGUMENT for a null
 * pointer. One check writes at most SX_MAX_PERCEPTS_PER_CHECK (4): give
 * observe cap >= 4, look cap >= 4 * (checks + instant_cap), hear cap >= 3
 * (HEARD, NOTICED, a due RECOGNIZED or FORGOTTEN). Percepts beyond cap are
 * lost; the check has run. */

/* One check of one observer-target pair; aware is the pair's value. */
int32_t sx_perception_observe(sx_perception *p, uint32_t now, const sx_eye *eye,
	const sx_target *target, sx_awareness *aware, sx_percept *out, uint32_t cap, uint32_t *total);
/* One observer looks at count targets (aware parallel to targets): first
 * targets with an instant cue (up to instant_cap), then kept ones (noticed,
 * up to keep), then round robin from the cursor; at most checks of the last
 * two. Unchecked targets are not touched. count 0: returns 0 and writes no
 * percept; count > 0 with targets or aware 0: SX_ERROR_ARGUMENT. */
int32_t sx_perception_look(sx_perception *p, uint32_t now, const sx_eye *eye,
	const sx_target *targets, sx_awareness *aware, uint32_t count,
	sx_attention *attention, const sx_budget *budget,
	sx_percept *out, uint32_t cap, uint32_t *total);
/* One hearing check. has_cost 0: the channel's spread; 1: cost is given
 * (from sx_perception_flood). aware 0: stateless (only the HEARD percept);
 * else the pair of this listener and s->source, raised at most to noticed.
 * *total 0: not heard. The heard cell is fuzzed in whole cells by seed.
 * s->disguise > 0: every percept is disguised unless aware has seen through
 * it (hearing never sees through). A RECOGNIZED that becomes due here is
 * not placed. */
int32_t sx_perception_hear(sx_perception *p, uint32_t now, const sx_eye *ear,
	const sx_stimulus *s, uint8_t has_cost, int32_t cost, sx_awareness *aware,
	sx_percept *out, uint32_t cap, uint32_t *total, uint64_t seed);
/* Every cell s reaches around corners with its cost in fine units, by one
 * search of nav over the sense map's sound (step cost plus damping), bounded
 * by the loudness and nav's capacity; then each listener calls
 * sx_perception_hear with has_cost 1 and the cost of its cell. Writes at most
 * cap cells (offset on hex) and costs (costs may be 0). nav's topology must
 * match the handle's: square with square, hex with the same offset layout
 * (else SX_ERROR_ARGUMENT); its size and walls are not compared. nav's
 * straight step cost must be the cell size (not checked in a release
 * build). The flood is bounded by the loudness: hearing or hearing_scale
 * above 256 do not widen it. With 8 moves the sound_edge hook is also asked
 * for diagonal pairs. Replaces nav's last search. */
int32_t sx_perception_flood(sx_perception *p, sx_nav *nav, const sx_stimulus *s,
	sx_point *cells, int32_t *costs, uint32_t cap, uint32_t *total);

/* For each of count targets, how many others of the same group (groups[i];
 * groups 0: all one group) stand within radius fine units on the same level
 * (Euclid on the point's x, y: axial Euclid on hex, approximate), into
 * out[i] (at most 255): the target's crowd. Returns 0. */
int32_t sx_count_crowds(const sx_target *targets, uint32_t count, int32_t radius,
	const uint32_t *groups, uint8_t *out);

/* ---- into knowledge and alertness ---- */

/* The game's fact kinds. A disguised percept (seen or heard) uses
 * disguised_kind in place of the noticed, seen and heard kinds, or is
 * skipped without one. */
typedef struct sx_fact_kinds {
	uint32_t noticed_kind;
	uint32_t seen_kind;
	uint32_t heard_kind;
	uint32_t disguised_kind;
	uint8_t has_disguised_kind;
} sx_fact_kinds;

/* The alertness stimulus per percept kind (0: none); for CUES the sum of
 * every cue_amount[i] whose cue_bits[i] share a bit with new_cues. */
typedef struct sx_alert_amounts {
	uint32_t noticed;
	uint32_t recognized;
	uint32_t seen;
	uint32_t heard;
	uint32_t cue_bits[8];
	uint32_t cue_amount[8];
} sx_alert_amounts;

/* The fact a percept stands for: NOTICED and SEEN at stage 1 give
 * noticed_kind, RECOGNIZED, SEEN at stage 2 and CUES seen_kind, HEARD
 * heard_kind (disguised: disguised_kind); pos is the percept's cell (offset on hex), strength the
 * clarity (hearing: the margin). Returns 1 (written), 0 (none: LOST,
 * FORGOTTEN, SEEN_THROUGH, not placed, disguised without a kind), or
 * SX_ERROR_ARGUMENT for a null pointer or an unknown sense or event. */
int32_t sx_percept_fact(const sx_percept *pc, const sx_fact_kinds *k, sx_fact *out);
/* The common use of count percepts, in order: NOTICED / SEEN at stage 1 learn
 * the noticed kind; RECOGNIZED / SEEN at stage 2 remove the noticed and learn
 * the seen kind; CUES set the seen fact's data; HEARD merges the heard kind
 * (a disguised percept: as in sx_fact_kinds);
 * LOST, FORGOTTEN, SEEN_THROUGH remove nothing (facts age out). Each amount
 * above 0 goes to sx_alertness_stimulus. known's trace sees each learn,
 * remove and merge.
 * Returns 0, or SX_ERROR_ARGUMENT (then nothing is changed). */
int32_t sx_learn_percepts(sx_knowledge *known, sx_alertness *alert, const sx_alert_config *cfg,
	uint32_t now, const sx_percept *pc, uint32_t count, const sx_fact_kinds *k,
	const sx_alert_amounts *a);
/* The alertness stimulus of one percept (0 for none or a null pointer). */
uint32_t sx_percept_alert(const sx_percept *pc, const sx_alert_amounts *a);

/* ---- trace ---- */

#define SX_PERCEPTION_TRACE_PICKED   1  /* look chose to check target (why) */
#define SX_PERCEPTION_TRACE_SKIPPED  2  /* the budget ran out before target */
#define SX_PERCEPTION_TRACE_LINE     3  /* a sight line: clear, clarity, cell (if blocked at one) */
#define SX_PERCEPTION_TRACE_GAIN     4  /* the factors of one pass of a check */
#define SX_PERCEPTION_TRACE_INSTANT  5  /* an instant cue fired: rule */
#define SX_PERCEPTION_TRACE_STAGE    6  /* from, to; pending 1: a rise waits */
#define SX_PERCEPTION_TRACE_DOUBT    7  /* see-through progress: doubt */
#define SX_PERCEPTION_TRACE_HEARD    8  /* target is the stimulus: cost, range, cell */
#define SX_PERCEPTION_TRACE_UNHEARD  9  /* target is the stimulus: cost (if any), range */

/* Why look checked a target (sx_perception_trace.why). */
#define SX_LOOK_INSTANT      0
#define SX_LOOK_KEPT         1
#define SX_LOOK_ROUND_ROBIN  2

/* One record; fields that do not apply to kind are 0. */
typedef struct sx_perception_trace {
	uint32_t kind;              /* SX_PERCEPTION_TRACE_* */
	uint32_t target;            /* the target id; HEARD, UNHEARD: the stimulus id */
	uint32_t why;               /* PICKED: SX_LOOK_* */
	uint32_t sense;             /* GAIN: SX_SENSE_* of the pass */
	uint32_t channel;           /* GAIN: the channel if hearing */
	uint32_t gain;              /* GAIN: the gain of the check */
	int32_t cost;               /* HEARD, UNHEARD; valid if has_cost */
	int32_t range;              /* HEARD, UNHEARD: the listener's range */
	int32_t cell[3];            /* LINE, HEARD; valid if has_cell (offset on hex) */
	uint16_t clarity;           /* LINE, GAIN */
	uint16_t distance;          /* GAIN: per 256 */
	uint16_t light;             /* GAIN: per 256 (thermal: heat) */
	uint16_t crowd;             /* GAIN: per 256 */
	uint16_t doubt;             /* DOUBT */
	uint8_t band;               /* GAIN: the band index (255: scanner) */
	uint8_t rule;               /* INSTANT: the cue rule index */
	uint8_t from;               /* STAGE */
	uint8_t to;                 /* STAGE */
	uint8_t clear;              /* LINE */
	uint8_t pending;            /* STAGE */
	uint8_t has_cell;
	uint8_t has_cost;
} sx_perception_trace;

/* trace gets every record of the following observe, look and hear calls,
 * with context; NULL turns it off (the default). trace must not call any
 * function on the same perception handle. Returns 0. */
typedef void (*sx_perception_trace_fn)(void *context, const sx_perception_trace *trace);
int32_t sx_perception_set_trace(sx_perception *p, sx_perception_trace_fn trace, void *context);

#ifdef __cplusplus
}
#endif

/* Layout checks (C11 and later): the sizes and field offsets below are the ones
 * the Rust library uses, so a header and a library that disagree fail to compile.
 * Every value struct is listed except those that hold a pointer, whose size
 * depends on the platform: sx_map, sx_areas_cost_ctx, sx_row_ctx, sx_squad_rules,
 * sx_mission_rules, sx_sense_map and sx_eye. */
#if !defined(__cplusplus) && defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
#include <stddef.h>
_Static_assert(sizeof(sx_point) == 12, "sx_point layout");
_Static_assert(offsetof(sx_point, level) == 8, "sx_point.level layout");
_Static_assert(sizeof(sx_rules) == 24, "sx_rules layout");
_Static_assert(offsetof(sx_rules, cost_straight) == 8, "sx_rules.cost_straight layout");
_Static_assert(offsetof(sx_rules, smooth_max_extra) == 20, "sx_rules.smooth_max_extra layout");
_Static_assert(sizeof(sx_result) == 20, "sx_result layout");
_Static_assert(offsetof(sx_result, node_limit_hit) == 16, "sx_result.node_limit_hit layout");
_Static_assert(sizeof(sx_search_trace) == 48, "sx_search_trace layout");
_Static_assert(offsetof(sx_search_trace, search) == 4, "sx_search_trace.search layout");
_Static_assert(offsetof(sx_search_trace, cell) == 8, "sx_search_trace.cell layout");
_Static_assert(offsetof(sx_search_trace, parent) == 20, "sx_search_trace.parent layout");
_Static_assert(offsetof(sx_search_trace, cost) == 32, "sx_search_trace.cost layout");
_Static_assert(offsetof(sx_search_trace, estimate) == 36, "sx_search_trace.estimate layout");
_Static_assert(offsetof(sx_search_trace, expanded) == 40, "sx_search_trace.expanded layout");
_Static_assert(offsetof(sx_search_trace, found) == 44, "sx_search_trace.found layout");
_Static_assert(offsetof(sx_search_trace, first) == 45, "sx_search_trace.first layout");
_Static_assert(sizeof(sx_nav_trace) == 108, "sx_nav_trace layout");
_Static_assert(offsetof(sx_nav_trace, search) == 4, "sx_nav_trace.search layout");
_Static_assert(offsetof(sx_nav_trace, from) == 52, "sx_nav_trace.from layout");
_Static_assert(offsetof(sx_nav_trace, to) == 64, "sx_nav_trace.to layout");
_Static_assert(offsetof(sx_nav_trace, cell) == 76, "sx_nav_trace.cell layout");
_Static_assert(offsetof(sx_nav_trace, member) == 88, "sx_nav_trace.member layout");
_Static_assert(offsetof(sx_nav_trace, step) == 92, "sx_nav_trace.step layout");
_Static_assert(offsetof(sx_nav_trace, distance) == 96, "sx_nav_trace.distance layout");
_Static_assert(offsetof(sx_nav_trace, cost) == 100, "sx_nav_trace.cost layout");
_Static_assert(offsetof(sx_nav_trace, clear) == 104, "sx_nav_trace.clear layout");
_Static_assert(offsetof(sx_nav_trace, slot) == 105, "sx_nav_trace.slot layout");
_Static_assert(sizeof(sx_consideration) == 16, "sx_consideration layout");
_Static_assert(offsetof(sx_consideration, max) == 12, "sx_consideration.max layout");
_Static_assert(sizeof(sx_candidate) == 8, "sx_candidate layout");
_Static_assert(offsetof(sx_candidate, target) == 4, "sx_candidate.target layout");
_Static_assert(sizeof(sx_choice) == 8, "sx_choice layout");
_Static_assert(offsetof(sx_choice, score) == 4, "sx_choice.score layout");
_Static_assert(sizeof(sx_decide_trace) == 36, "sx_decide_trace layout");
_Static_assert(offsetof(sx_decide_trace, candidate) == 4, "sx_decide_trace.candidate layout");
_Static_assert(offsetof(sx_decide_trace, consideration) == 8, "sx_decide_trace.consideration layout");
_Static_assert(offsetof(sx_decide_trace, input) == 12, "sx_decide_trace.input layout");
_Static_assert(offsetof(sx_decide_trace, value) == 16, "sx_decide_trace.value layout");
_Static_assert(offsetof(sx_decide_trace, score) == 20, "sx_decide_trace.score layout");
_Static_assert(offsetof(sx_decide_trace, product) == 24, "sx_decide_trace.product layout");
_Static_assert(offsetof(sx_decide_trace, rank) == 28, "sx_decide_trace.rank layout");
_Static_assert(offsetof(sx_decide_trace, cooled) == 32, "sx_decide_trace.cooled layout");
_Static_assert(sizeof(sx_change) == 16, "sx_change layout");
_Static_assert(offsetof(sx_change, transition) == 12, "sx_change.transition layout");
_Static_assert(sizeof(sx_machine_trace) == 32, "sx_machine_trace layout");
_Static_assert(offsetof(sx_machine_trace, change) == 4, "sx_machine_trace.change layout");
_Static_assert(offsetof(sx_machine_trace, transition) == 20, "sx_machine_trace.transition layout");
_Static_assert(offsetof(sx_machine_trace, elapsed) == 24, "sx_machine_trace.elapsed layout");
_Static_assert(offsetof(sx_machine_trace, after) == 28, "sx_machine_trace.after layout");
_Static_assert(sizeof(sx_fact) == 36, "sx_fact layout");
_Static_assert(offsetof(sx_fact, pos) == 8, "sx_fact.pos layout");
_Static_assert(offsetof(sx_fact, seen_at) == 20, "sx_fact.seen_at layout");
_Static_assert(offsetof(sx_fact, data) == 28, "sx_fact.data layout");
_Static_assert(offsetof(sx_fact, source) == 32, "sx_fact.source layout");
_Static_assert(sizeof(sx_message) == 40, "sx_message layout");
_Static_assert(offsetof(sx_message, pos) == 8, "sx_message.pos layout");
_Static_assert(offsetof(sx_message, sender) == 32, "sx_message.sender layout");
_Static_assert(offsetof(sx_message, keep_if_sender_gone) == 36, "sx_message.keep_if_sender_gone layout");
_Static_assert(sizeof(sx_area) == 52, "sx_area layout");
_Static_assert(offsetof(sx_area, radius) == 16, "sx_area.radius layout");
_Static_assert(offsetof(sx_area, source) == 36, "sx_area.source layout");
_Static_assert(offsetof(sx_area, blocking) == 48, "sx_area.blocking layout");
_Static_assert(offsetof(sx_area, has_source) == 49, "sx_area.has_source layout");
_Static_assert(sizeof(sx_knowledge_trace) == 40, "sx_knowledge_trace layout");
_Static_assert(offsetof(sx_knowledge_trace, fact) == 4, "sx_knowledge_trace.fact layout");
_Static_assert(sizeof(sx_message_trace) == 52, "sx_message_trace layout");
_Static_assert(offsetof(sx_message_trace, msg) == 4, "sx_message_trace.msg layout");
_Static_assert(offsetof(sx_message_trace, at) == 44, "sx_message_trace.at layout");
_Static_assert(offsetof(sx_message_trace, refused) == 48, "sx_message_trace.refused layout");
_Static_assert(sizeof(sx_areas_trace) == 60, "sx_areas_trace layout");
_Static_assert(offsetof(sx_areas_trace, index) == 4, "sx_areas_trace.index layout");
_Static_assert(offsetof(sx_areas_trace, area) == 8, "sx_areas_trace.area layout");
_Static_assert(sizeof(sx_timers_trace) == 20, "sx_timers_trace layout");
_Static_assert(offsetof(sx_timers_trace, duration) == 8, "sx_timers_trace.duration layout");
_Static_assert(offsetof(sx_timers_trace, old) == 12, "sx_timers_trace.old layout");
_Static_assert(offsetof(sx_timers_trace, old_left) == 16, "sx_timers_trace.old_left layout");
_Static_assert(sizeof(sx_progress) == 12, "sx_progress layout");
_Static_assert(offsetof(sx_progress, since) == 4, "sx_progress.since layout");
_Static_assert(offsetof(sx_progress, started) == 8, "sx_progress.started layout");
_Static_assert(sizeof(sx_interval) == 8, "sx_interval layout");
_Static_assert(offsetof(sx_interval, started) == 4, "sx_interval.started layout");
_Static_assert(sizeof(sx_alertness) == 16, "sx_alertness layout");
_Static_assert(sizeof(sx_alert_config) == 52, "sx_alert_config layout");
_Static_assert(offsetof(sx_alert_config, thresholds) == 20, "sx_alert_config.thresholds layout");
_Static_assert(sizeof(sx_nearby) == 12, "sx_nearby layout");
_Static_assert(offsetof(sx_nearby, y) == 8, "sx_nearby.y layout");
_Static_assert(sizeof(sx_point_pattern) == 96, "sx_point_pattern layout");
_Static_assert(offsetof(sx_point_pattern, offsets) == 12, "sx_point_pattern.offsets layout");
_Static_assert(offsetof(sx_point_pattern, distances) == 44, "sx_point_pattern.distances layout");
_Static_assert(offsetof(sx_point_pattern, sine) == 76, "sx_point_pattern.sine layout");
_Static_assert(offsetof(sx_point_pattern, max_y) == 92, "sx_point_pattern.max_y layout");
_Static_assert(sizeof(sx_member) == 24, "sx_member layout");
_Static_assert(offsetof(sx_member, pos) == 4, "sx_member.pos layout");
_Static_assert(offsetof(sx_member, roles) == 16, "sx_member.roles layout");
_Static_assert(offsetof(sx_member, skill) == 20, "sx_member.skill layout");
_Static_assert(offsetof(sx_member, state) == 21, "sx_member.state layout");
_Static_assert(sizeof(sx_spot) == 16, "sx_spot layout");
_Static_assert(offsetof(sx_spot, tags) == 12, "sx_spot.tags layout");
_Static_assert(sizeof(sx_gather_candidate) == 24, "sx_gather_candidate layout");
_Static_assert(offsetof(sx_gather_candidate, pos) == 8, "sx_gather_candidate.pos layout");
_Static_assert(offsetof(sx_gather_candidate, free) == 20, "sx_gather_candidate.free layout");
_Static_assert(sizeof(sx_place) == 20, "sx_place layout");
_Static_assert(offsetof(sx_place, z) == 12, "sx_place.z layout");
_Static_assert(offsetof(sx_place, value) == 16, "sx_place.value layout");
_Static_assert(sizeof(sx_slot) == 76, "sx_slot layout");
_Static_assert(offsetof(sx_slot, min) == 4, "sx_slot.min layout");
_Static_assert(offsetof(sx_slot, max) == 5, "sx_slot.max layout");
_Static_assert(offsetof(sx_slot, place) == 8, "sx_slot.place layout");
_Static_assert(offsetof(sx_slot, gather) == 28, "sx_slot.gather layout");
_Static_assert(offsetof(sx_slot, dist) == 68, "sx_slot.dist layout");
_Static_assert(offsetof(sx_slot, gather_dist) == 72, "sx_slot.gather_dist layout");
_Static_assert(sizeof(sx_skill_table) == 204, "sx_skill_table layout");
_Static_assert(offsetof(sx_skill_table, hold_bonus) == 132, "sx_skill_table.hold_bonus layout");
_Static_assert(offsetof(sx_skill_table, spot_rank_error) == 196, "sx_skill_table.spot_rank_error layout");
_Static_assert(sizeof(sx_squad_trace) == 344, "sx_squad_trace layout");
_Static_assert(offsetof(sx_squad_trace, tactic) == 4, "sx_squad_trace.tactic layout");
_Static_assert(offsetof(sx_squad_trace, target) == 8, "sx_squad_trace.target layout");
_Static_assert(offsetof(sx_squad_trace, member) == 12, "sx_squad_trace.member layout");
_Static_assert(offsetof(sx_squad_trace, slot) == 16, "sx_squad_trace.slot layout");
_Static_assert(offsetof(sx_squad_trace, abort) == 20, "sx_squad_trace.abort layout");
_Static_assert(offsetof(sx_squad_trace, plan) == 24, "sx_squad_trace.plan layout");
_Static_assert(offsetof(sx_squad_trace, until) == 28, "sx_squad_trace.until layout");
_Static_assert(offsetof(sx_squad_trace, score) == 32, "sx_squad_trace.score layout");
_Static_assert(offsetof(sx_squad_trace, delay) == 36, "sx_squad_trace.delay layout");
_Static_assert(offsetof(sx_squad_trace, spot) == 40, "sx_squad_trace.spot layout");
_Static_assert(offsetof(sx_squad_trace, pos) == 44, "sx_squad_trace.pos layout");
_Static_assert(offsetof(sx_squad_trace, has_target) == 56, "sx_squad_trace.has_target layout");
_Static_assert(offsetof(sx_squad_trace, has_delay) == 57, "sx_squad_trace.has_delay layout");
_Static_assert(offsetof(sx_squad_trace, has_spot) == 58, "sx_squad_trace.has_spot layout");
_Static_assert(offsetof(sx_squad_trace, fired) == 59, "sx_squad_trace.fired layout");
_Static_assert(offsetof(sx_squad_trace, kept) == 60, "sx_squad_trace.kept layout");
_Static_assert(offsetof(sx_squad_trace, fits) == 61, "sx_squad_trace.fits layout");
_Static_assert(offsetof(sx_squad_trace, cost) == 64, "sx_squad_trace.cost layout");
_Static_assert(offsetof(sx_squad_trace, distance) == 72, "sx_squad_trace.distance layout");
_Static_assert(offsetof(sx_squad_trace, fact) == 80, "sx_squad_trace.fact layout");
_Static_assert(offsetof(sx_squad_trace, msg) == 116, "sx_squad_trace.msg layout");
_Static_assert(offsetof(sx_squad_trace, decide) == 156, "sx_squad_trace.decide layout");
_Static_assert(offsetof(sx_squad_trace, knowledge) == 192, "sx_squad_trace.knowledge layout");
_Static_assert(offsetof(sx_squad_trace, areas) == 232, "sx_squad_trace.areas layout");
_Static_assert(offsetof(sx_squad_trace, inbox) == 292, "sx_squad_trace.inbox layout");
_Static_assert(sizeof(sx_keeper_trace) == 16, "sx_keeper_trace layout");
_Static_assert(offsetof(sx_keeper_trace, waited) == 4, "sx_keeper_trace.waited layout");
_Static_assert(offsetof(sx_keeper_trace, limit) == 8, "sx_keeper_trace.limit layout");
_Static_assert(offsetof(sx_keeper_trace, broken) == 12, "sx_keeper_trace.broken layout");
_Static_assert(sizeof(sx_goal) == 68, "sx_goal layout");
_Static_assert(offsetof(sx_goal, pos) == 8, "sx_goal.pos layout");
_Static_assert(offsetof(sx_goal, then_pos) == 20, "sx_goal.then_pos layout");
_Static_assert(offsetof(sx_goal, target) == 32, "sx_goal.target layout");
_Static_assert(offsetof(sx_goal, start_delay) == 56, "sx_goal.start_delay layout");
_Static_assert(offsetof(sx_goal, has_pos) == 60, "sx_goal.has_pos layout");
_Static_assert(offsetof(sx_goal, unchecked) == 61, "sx_goal.unchecked layout");
_Static_assert(offsetof(sx_goal, has_then_pos) == 62, "sx_goal.has_then_pos layout");
_Static_assert(offsetof(sx_goal, then_unchecked) == 63, "sx_goal.then_unchecked layout");
_Static_assert(offsetof(sx_goal, has_target) == 64, "sx_goal.has_target layout");
_Static_assert(sizeof(sx_order) == 72, "sx_order layout");
_Static_assert(offsetof(sx_order, goal) == 4, "sx_order.goal layout");
_Static_assert(sizeof(sx_plan) == 28, "sx_plan layout");
_Static_assert(offsetof(sx_plan, goal_count) == 20, "sx_plan.goal_count layout");
_Static_assert(offsetof(sx_plan, has_target) == 24, "sx_plan.has_target layout");
_Static_assert(sizeof(sx_squad_config) == 64, "sx_squad_config layout");
_Static_assert(offsetof(sx_squad_config, metric) == 20, "sx_squad_config.metric layout");
_Static_assert(offsetof(sx_squad_config, role_count) == 24, "sx_squad_config.role_count layout");
_Static_assert(offsetof(sx_squad_config, role_masks) == 28, "sx_squad_config.role_masks layout");
_Static_assert(offsetof(sx_squad_config, rule_columns) == 60, "sx_squad_config.rule_columns layout");
_Static_assert(sizeof(sx_member_inputs) == 20, "sx_member_inputs layout");
_Static_assert(offsetof(sx_member_inputs, discipline) == 16, "sx_member_inputs.discipline layout");
_Static_assert(sizeof(sx_crew_config) == 72, "sx_crew_config layout");
_Static_assert(offsetof(sx_crew_config, squad) == 0, "sx_crew_config.squad layout");
_Static_assert(offsetof(sx_crew_config, arrive_radius) == 64, "sx_crew_config.arrive_radius layout");
_Static_assert(offsetof(sx_crew_config, hex_grid) == 68, "sx_crew_config.hex_grid layout");
_Static_assert(sizeof(sx_crew_event) == 88, "sx_crew_event layout");
_Static_assert(offsetof(sx_crew_event, start_at) == 12, "sx_crew_event.start_at layout");
_Static_assert(offsetof(sx_crew_event, goal) == 16, "sx_crew_event.goal layout");
_Static_assert(offsetof(sx_crew_event, has_start_at) == 84, "sx_crew_event.has_start_at layout");
_Static_assert(sizeof(sx_crew_trace) == 416, "sx_crew_trace layout");
_Static_assert(offsetof(sx_crew_trace, member) == 4, "sx_crew_trace.member layout");
_Static_assert(offsetof(sx_crew_trace, delay) == 8, "sx_crew_trace.delay layout");
_Static_assert(offsetof(sx_crew_trace, has_delay) == 12, "sx_crew_trace.has_delay layout");
_Static_assert(offsetof(sx_crew_trace, queued) == 13, "sx_crew_trace.queued layout");
_Static_assert(offsetof(sx_crew_trace, msg) == 16, "sx_crew_trace.msg layout");
_Static_assert(offsetof(sx_crew_trace, keeper) == 56, "sx_crew_trace.keeper layout");
_Static_assert(offsetof(sx_crew_trace, squad) == 72, "sx_crew_trace.squad layout");
_Static_assert(sizeof(sx_place_ctx) == 72, "sx_place_ctx layout");
_Static_assert(offsetof(sx_place_ctx, dist) == 12, "sx_place_ctx.dist layout");
_Static_assert(offsetof(sx_place_ctx, leader) == 16, "sx_place_ctx.leader layout");
_Static_assert(offsetof(sx_place_ctx, centre) == 28, "sx_place_ctx.centre layout");
_Static_assert(offsetof(sx_place_ctx, face) == 40, "sx_place_ctx.face layout");
_Static_assert(offsetof(sx_place_ctx, friend_pos) == 52, "sx_place_ctx.friend_pos layout");
_Static_assert(offsetof(sx_place_ctx, heading) == 64, "sx_place_ctx.heading layout");
_Static_assert(offsetof(sx_place_ctx, has_target) == 68, "sx_place_ctx.has_target layout");
_Static_assert(sizeof(sx_cost_ctx) == 56, "sx_cost_ctx layout");
_Static_assert(offsetof(sx_cost_ctx, tactic) == 8, "sx_cost_ctx.tactic layout");
_Static_assert(offsetof(sx_cost_ctx, member) == 16, "sx_cost_ctx.member layout");
_Static_assert(offsetof(sx_cost_ctx, wish) == 40, "sx_cost_ctx.wish layout");
_Static_assert(sizeof(sx_spot_ctx) == 104, "sx_spot_ctx layout");
_Static_assert(offsetof(sx_spot_ctx, distance) == 8, "sx_spot_ctx.distance layout");
_Static_assert(offsetof(sx_spot_ctx, tactic) == 16, "sx_spot_ctx.tactic layout");
_Static_assert(offsetof(sx_spot_ctx, want_tags) == 28, "sx_spot_ctx.want_tags layout");
_Static_assert(offsetof(sx_spot_ctx, target) == 32, "sx_spot_ctx.target layout");
_Static_assert(offsetof(sx_spot_ctx, spot) == 36, "sx_spot_ctx.spot layout");
_Static_assert(offsetof(sx_spot_ctx, member) == 52, "sx_spot_ctx.member layout");
_Static_assert(offsetof(sx_spot_ctx, wish) == 76, "sx_spot_ctx.wish layout");
_Static_assert(offsetof(sx_spot_ctx, target_pos) == 88, "sx_spot_ctx.target_pos layout");
_Static_assert(offsetof(sx_spot_ctx, gather) == 100, "sx_spot_ctx.gather layout");
_Static_assert(offsetof(sx_spot_ctx, has_target) == 101, "sx_spot_ctx.has_target layout");
_Static_assert(offsetof(sx_spot_ctx, miss) == 102, "sx_spot_ctx.miss layout");
_Static_assert(sizeof(sx_channel_ctx) == 44, "sx_channel_ctx layout");
_Static_assert(offsetof(sx_channel_ctx, distance) == 8, "sx_channel_ctx.distance layout");
_Static_assert(offsetof(sx_channel_ctx, now) == 12, "sx_channel_ctx.now layout");
_Static_assert(offsetof(sx_channel_ctx, from) == 16, "sx_channel_ctx.from layout");
_Static_assert(offsetof(sx_channel_ctx, to) == 28, "sx_channel_ctx.to layout");
_Static_assert(offsetof(sx_channel_ctx, report) == 40, "sx_channel_ctx.report layout");
_Static_assert(sizeof(sx_view_plan) == 20, "sx_view_plan layout");
_Static_assert(offsetof(sx_view_plan, since) == 12, "sx_view_plan.since layout");
_Static_assert(offsetof(sx_view_plan, has_plan) == 16, "sx_view_plan.has_plan layout");
_Static_assert(offsetof(sx_view_plan, has_target) == 17, "sx_view_plan.has_target layout");
_Static_assert(offsetof(sx_view_plan, signal_given) == 18, "sx_view_plan.signal_given layout");
_Static_assert(sizeof(sx_where) == 20, "sx_where layout");
_Static_assert(offsetof(sx_where, pos) == 4, "sx_where.pos layout");
_Static_assert(offsetof(sx_where, subject) == 16, "sx_where.subject layout");
_Static_assert(sizeof(sx_fail) == 40, "sx_fail layout");
_Static_assert(offsetof(sx_fail, id) == 20, "sx_fail.id layout");
_Static_assert(offsetof(sx_fail, flags_mask) == 24, "sx_fail.flags_mask layout");
_Static_assert(offsetof(sx_fail, flags_value) == 32, "sx_fail.flags_value layout");
_Static_assert(sizeof(sx_step) == 200, "sx_step layout");
_Static_assert(offsetof(sx_step, at) == 4, "sx_step.at layout");
_Static_assert(offsetof(sx_step, a) == 24, "sx_step.a layout");
_Static_assert(offsetof(sx_step, b) == 28, "sx_step.b layout");
_Static_assert(offsetof(sx_step, fails) == 32, "sx_step.fails layout");
_Static_assert(offsetof(sx_step, fail_count) == 192, "sx_step.fail_count layout");
_Static_assert(offsetof(sx_step, skip_if_impossible) == 196, "sx_step.skip_if_impossible layout");
_Static_assert(sizeof(sx_motive) == 32, "sx_motive layout");
_Static_assert(offsetof(sx_motive, place) == 8, "sx_motive.place layout");
_Static_assert(offsetof(sx_motive, subject) == 20, "sx_motive.subject layout");
_Static_assert(offsetof(sx_motive, radius) == 24, "sx_motive.radius layout");
_Static_assert(offsetof(sx_motive, has_place) == 28, "sx_motive.has_place layout");
_Static_assert(offsetof(sx_motive, has_subject) == 29, "sx_motive.has_subject layout");
_Static_assert(sizeof(sx_leeway) == 24, "sx_leeway layout");
_Static_assert(offsetof(sx_leeway, on_lost) == 8, "sx_leeway.on_lost layout");
_Static_assert(offsetof(sx_leeway, lost_radius) == 16, "sx_leeway.lost_radius layout");
_Static_assert(offsetof(sx_leeway, has_distance) == 20, "sx_leeway.has_distance layout");
_Static_assert(offsetof(sx_leeway, widen) == 23, "sx_leeway.widen layout");
_Static_assert(sizeof(sx_mission_config) == 160, "sx_mission_config layout");
_Static_assert(offsetof(sx_mission_config, metric) == 44, "sx_mission_config.metric layout");
_Static_assert(offsetof(sx_mission_config, grid) == 48, "sx_mission_config.grid layout");
_Static_assert(offsetof(sx_mission_config, pick_top) == 60, "sx_mission_config.pick_top layout");
_Static_assert(offsetof(sx_mission_config, search) == 64, "sx_mission_config.search layout");
_Static_assert(sizeof(sx_mission_plan_info) == 176, "sx_mission_plan_info layout");
_Static_assert(offsetof(sx_mission_plan_info, fail_count) == 4, "sx_mission_plan_info.fail_count layout");
_Static_assert(offsetof(sx_mission_plan_info, fails) == 8, "sx_mission_plan_info.fails layout");
_Static_assert(offsetof(sx_mission_plan_info, repeat) == 168, "sx_mission_plan_info.repeat layout");
_Static_assert(sizeof(sx_detour) == 52, "sx_detour layout");
_Static_assert(offsetof(sx_detour, at) == 8, "sx_detour.at layout");
_Static_assert(offsetof(sx_detour, to) == 28, "sx_detour.to layout");
_Static_assert(offsetof(sx_detour, custom) == 40, "sx_detour.custom layout");
_Static_assert(offsetof(sx_detour, param) == 44, "sx_detour.param layout");
_Static_assert(offsetof(sx_detour, has_to) == 48, "sx_detour.has_to layout");
_Static_assert(sizeof(sx_mission_action) == 32, "sx_mission_action layout");
_Static_assert(offsetof(sx_mission_action, pos) == 4, "sx_mission_action.pos layout");
_Static_assert(offsetof(sx_mission_action, subject) == 16, "sx_mission_action.subject layout");
_Static_assert(offsetof(sx_mission_action, param) == 24, "sx_mission_action.param layout");
_Static_assert(offsetof(sx_mission_action, has_pos) == 28, "sx_mission_action.has_pos layout");
_Static_assert(sizeof(sx_mission_event) == 104, "sx_mission_event layout");
_Static_assert(offsetof(sx_mission_event, subject) == 12, "sx_mission_event.subject layout");
_Static_assert(offsetof(sx_mission_event, pos) == 16, "sx_mission_event.pos layout");
_Static_assert(offsetof(sx_mission_event, has_pos) == 28, "sx_mission_event.has_pos layout");
_Static_assert(offsetof(sx_mission_event, has_subject) == 29, "sx_mission_event.has_subject layout");
_Static_assert(offsetof(sx_mission_event, fail) == 32, "sx_mission_event.fail layout");
_Static_assert(offsetof(sx_mission_event, motive) == 72, "sx_mission_event.motive layout");
_Static_assert(sizeof(sx_mission_out) == 872, "sx_mission_out layout");
_Static_assert(offsetof(sx_mission_out, event_count) == 32, "sx_mission_out.event_count layout");
_Static_assert(offsetof(sx_mission_out, events) == 40, "sx_mission_out.events layout");
_Static_assert(sizeof(sx_mission_fail_ctx) == 60, "sx_mission_fail_ctx layout");
_Static_assert(offsetof(sx_mission_fail_ctx, step) == 16, "sx_mission_fail_ctx.step layout");
_Static_assert(offsetof(sx_mission_fail_ctx, candidate) == 20, "sx_mission_fail_ctx.candidate layout");
_Static_assert(offsetof(sx_mission_fail_ctx, motive) == 24, "sx_mission_fail_ctx.motive layout");
_Static_assert(offsetof(sx_mission_fail_ctx, has_step) == 56, "sx_mission_fail_ctx.has_step layout");
_Static_assert(offsetof(sx_mission_fail_ctx, has_candidate) == 57, "sx_mission_fail_ctx.has_candidate layout");
_Static_assert(sizeof(sx_mission_trace) == 80, "sx_mission_trace layout");
_Static_assert(offsetof(sx_mission_trace, option) == 4, "sx_mission_trace.option layout");
_Static_assert(offsetof(sx_mission_trace, column) == 8, "sx_mission_trace.column layout");
_Static_assert(offsetof(sx_mission_trace, built_in) == 12, "sx_mission_trace.built_in layout");
_Static_assert(offsetof(sx_mission_trace, score) == 16, "sx_mission_trace.score layout");
_Static_assert(offsetof(sx_mission_trace, index) == 20, "sx_mission_trace.index layout");
_Static_assert(offsetof(sx_mission_trace, value) == 24, "sx_mission_trace.value layout");
_Static_assert(offsetof(sx_mission_trace, at) == 28, "sx_mission_trace.at layout");
_Static_assert(offsetof(sx_mission_trace, has_option) == 40, "sx_mission_trace.has_option layout");
_Static_assert(offsetof(sx_mission_trace, has_at) == 41, "sx_mission_trace.has_at layout");
_Static_assert(offsetof(sx_mission_trace, decide) == 44, "sx_mission_trace.decide layout");
_Static_assert(sizeof(sx_mission_plan_ctx) == 68, "sx_mission_plan_ctx layout");
_Static_assert(offsetof(sx_mission_plan_ctx, option) == 16, "sx_mission_plan_ctx.option layout");
_Static_assert(offsetof(sx_mission_plan_ctx, first) == 20, "sx_mission_plan_ctx.first layout");
_Static_assert(offsetof(sx_mission_plan_ctx, motive) == 32, "sx_mission_plan_ctx.motive layout");
_Static_assert(offsetof(sx_mission_plan_ctx, widening) == 64, "sx_mission_plan_ctx.widening layout");
_Static_assert(offsetof(sx_mission_plan_ctx, has_first) == 66, "sx_mission_plan_ctx.has_first layout");
_Static_assert(sizeof(sx_medium) == 8, "sx_medium layout");
_Static_assert(offsetof(sx_medium, density) == 2, "sx_medium.density layout");
_Static_assert(offsetof(sx_medium, heat_density) == 4, "sx_medium.heat_density layout");
_Static_assert(sizeof(sx_channel) == 12, "sx_channel layout");
_Static_assert(offsetof(sx_channel, fuzz) == 2, "sx_channel.fuzz layout");
_Static_assert(offsetof(sx_channel, speed) == 4, "sx_channel.speed layout");
_Static_assert(offsetof(sx_channel, spread) == 8, "sx_channel.spread layout");
_Static_assert(sizeof(sx_perception_config) == 56, "sx_perception_config layout");
_Static_assert(offsetof(sx_perception_config, cell_size) == 4, "sx_perception_config.cell_size layout");
_Static_assert(offsetof(sx_perception_config, channels) == 8, "sx_perception_config.channels layout");
_Static_assert(sizeof(sx_band) == 6, "sx_band layout");
_Static_assert(offsetof(sx_band, gain) == 2, "sx_band.gain layout");
_Static_assert(offsetof(sx_band, instant) == 4, "sx_band.instant layout");
_Static_assert(sizeof(sx_cue_rule) == 16, "sx_cue_rule layout");
_Static_assert(offsetof(sx_cue_rule, gain) == 4, "sx_cue_rule.gain layout");
_Static_assert(offsetof(sx_cue_rule, instant) == 6, "sx_cue_rule.instant layout");
_Static_assert(offsetof(sx_cue_rule, min_clarity) == 8, "sx_cue_rule.min_clarity layout");
_Static_assert(offsetof(sx_cue_rule, range) == 12, "sx_cue_rule.range layout");
_Static_assert(sizeof(sx_sense_profile) == 212, "sx_sense_profile layout");
_Static_assert(offsetof(sx_sense_profile, range) == 4, "sx_sense_profile.range layout");
_Static_assert(offsetof(sx_sense_profile, near) == 8, "sx_sense_profile.near layout");
_Static_assert(offsetof(sx_sense_profile, far_gain) == 12, "sx_sense_profile.far_gain layout");
_Static_assert(offsetof(sx_sense_profile, bands) == 14, "sx_sense_profile.bands layout");
_Static_assert(offsetof(sx_sense_profile, base_gain) == 32, "sx_sense_profile.base_gain layout");
_Static_assert(offsetof(sx_sense_profile, dark) == 34, "sx_sense_profile.dark layout");
_Static_assert(offsetof(sx_sense_profile, bright) == 35, "sx_sense_profile.bright layout");
_Static_assert(offsetof(sx_sense_profile, dark_gain) == 36, "sx_sense_profile.dark_gain layout");
_Static_assert(offsetof(sx_sense_profile, notice_at) == 38, "sx_sense_profile.notice_at layout");
_Static_assert(offsetof(sx_sense_profile, recognize_at) == 40, "sx_sense_profile.recognize_at layout");
_Static_assert(offsetof(sx_sense_profile, max) == 42, "sx_sense_profile.max layout");
_Static_assert(offsetof(sx_sense_profile, decay_per_tick) == 44, "sx_sense_profile.decay_per_tick layout");
_Static_assert(offsetof(sx_sense_profile, hold) == 46, "sx_sense_profile.hold layout");
_Static_assert(offsetof(sx_sense_profile, hysteresis) == 48, "sx_sense_profile.hysteresis layout");
_Static_assert(offsetof(sx_sense_profile, check_interval) == 50, "sx_sense_profile.check_interval layout");
_Static_assert(offsetof(sx_sense_profile, crowd_gain) == 52, "sx_sense_profile.crowd_gain layout");
_Static_assert(offsetof(sx_sense_profile, scanner_range) == 68, "sx_sense_profile.scanner_range layout");
_Static_assert(offsetof(sx_sense_profile, scanner_gain) == 72, "sx_sense_profile.scanner_gain layout");
_Static_assert(offsetof(sx_sense_profile, hearing) == 74, "sx_sense_profile.hearing layout");
_Static_assert(offsetof(sx_sense_profile, hear_gain) == 82, "sx_sense_profile.hear_gain layout");
_Static_assert(offsetof(sx_sense_profile, cues) == 84, "sx_sense_profile.cues layout");
_Static_assert(sizeof(sx_stats) == 8, "sx_stats layout");
_Static_assert(offsetof(sx_stats, notice_scale) == 2, "sx_stats.notice_scale layout");
_Static_assert(offsetof(sx_stats, hearing_scale) == 4, "sx_stats.hearing_scale layout");
_Static_assert(offsetof(sx_stats, reaction_ticks) == 6, "sx_stats.reaction_ticks layout");
_Static_assert(sizeof(sx_see_through) == 12, "sx_see_through layout");
_Static_assert(offsetof(sx_see_through, near) == 4, "sx_see_through.near layout");
_Static_assert(offsetof(sx_see_through, at) == 8, "sx_see_through.at layout");
_Static_assert(sizeof(sx_disguise_rule) == 20, "sx_disguise_rule layout");
_Static_assert(offsetof(sx_disguise_rule, see_through) == 4, "sx_disguise_rule.see_through layout");
_Static_assert(offsetof(sx_disguise_rule, has_see_through) == 16, "sx_disguise_rule.has_see_through layout");
_Static_assert(sizeof(sx_target) == 28, "sx_target layout");
_Static_assert(offsetof(sx_target, point) == 4, "sx_target.point layout");
_Static_assert(offsetof(sx_target, size) == 16, "sx_target.size layout");
_Static_assert(offsetof(sx_target, conspicuity) == 18, "sx_target.conspicuity layout");
_Static_assert(offsetof(sx_target, cues) == 20, "sx_target.cues layout");
_Static_assert(offsetof(sx_target, disguise) == 24, "sx_target.disguise layout");
_Static_assert(offsetof(sx_target, heat) == 25, "sx_target.heat layout");
_Static_assert(offsetof(sx_target, glow) == 26, "sx_target.glow layout");
_Static_assert(offsetof(sx_target, crowd) == 27, "sx_target.crowd layout");
_Static_assert(sizeof(sx_awareness) == 24, "sx_awareness layout");
_Static_assert(offsetof(sx_awareness, doubt) == 2, "sx_awareness.doubt layout");
_Static_assert(offsetof(sx_awareness, stage) == 4, "sx_awareness.stage layout");
_Static_assert(offsetof(sx_awareness, pending) == 5, "sx_awareness.pending layout");
_Static_assert(offsetof(sx_awareness, flags) == 6, "sx_awareness.flags layout");
_Static_assert(offsetof(sx_awareness, last) == 8, "sx_awareness.last layout");
_Static_assert(offsetof(sx_awareness, pending_since) == 12, "sx_awareness.pending_since layout");
_Static_assert(offsetof(sx_awareness, last_seen) == 16, "sx_awareness.last_seen layout");
_Static_assert(offsetof(sx_awareness, cues) == 20, "sx_awareness.cues layout");
_Static_assert(sizeof(sx_attention) == 4, "sx_attention layout");
_Static_assert(sizeof(sx_budget) == 6, "sx_budget layout");
_Static_assert(offsetof(sx_budget, keep) == 2, "sx_budget.keep layout");
_Static_assert(offsetof(sx_budget, instant_cap) == 4, "sx_budget.instant_cap layout");
_Static_assert(sizeof(sx_stimulus) == 36, "sx_stimulus layout");
_Static_assert(offsetof(sx_stimulus, source) == 4, "sx_stimulus.source layout");
_Static_assert(offsetof(sx_stimulus, kind) == 8, "sx_stimulus.kind layout");
_Static_assert(offsetof(sx_stimulus, point) == 12, "sx_stimulus.point layout");
_Static_assert(offsetof(sx_stimulus, loudness) == 24, "sx_stimulus.loudness layout");
_Static_assert(offsetof(sx_stimulus, channel) == 28, "sx_stimulus.channel layout");
_Static_assert(offsetof(sx_stimulus, disguise) == 29, "sx_stimulus.disguise layout");
_Static_assert(offsetof(sx_stimulus, at) == 32, "sx_stimulus.at layout");
_Static_assert(sizeof(sx_percept) == 72, "sx_percept layout");
_Static_assert(offsetof(sx_percept, subject) == 4, "sx_percept.subject layout");
_Static_assert(offsetof(sx_percept, sense) == 8, "sx_percept.sense layout");
_Static_assert(offsetof(sx_percept, channel) == 12, "sx_percept.channel layout");
_Static_assert(offsetof(sx_percept, event) == 16, "sx_percept.event layout");
_Static_assert(offsetof(sx_percept, new_cues) == 20, "sx_percept.new_cues layout");
_Static_assert(offsetof(sx_percept, point) == 24, "sx_percept.point layout");
_Static_assert(offsetof(sx_percept, cell) == 36, "sx_percept.cell layout");
_Static_assert(offsetof(sx_percept, cues) == 48, "sx_percept.cues layout");
_Static_assert(offsetof(sx_percept, margin) == 52, "sx_percept.margin layout");
_Static_assert(offsetof(sx_percept, arrive_at) == 56, "sx_percept.arrive_at layout");
_Static_assert(offsetof(sx_percept, at) == 60, "sx_percept.at layout");
_Static_assert(offsetof(sx_percept, clarity) == 64, "sx_percept.clarity layout");
_Static_assert(offsetof(sx_percept, stage) == 66, "sx_percept.stage layout");
_Static_assert(offsetof(sx_percept, placed) == 67, "sx_percept.placed layout");
_Static_assert(offsetof(sx_percept, disguised) == 68, "sx_percept.disguised layout");
_Static_assert(sizeof(sx_line) == 16, "sx_line layout");
_Static_assert(offsetof(sx_line, has_blocked_at) == 1, "sx_line.has_blocked_at layout");
_Static_assert(offsetof(sx_line, clarity) == 2, "sx_line.clarity layout");
_Static_assert(offsetof(sx_line, blocked_at) == 4, "sx_line.blocked_at layout");
_Static_assert(sizeof(sx_fact_kinds) == 20, "sx_fact_kinds layout");
_Static_assert(offsetof(sx_fact_kinds, seen_kind) == 4, "sx_fact_kinds.seen_kind layout");
_Static_assert(offsetof(sx_fact_kinds, heard_kind) == 8, "sx_fact_kinds.heard_kind layout");
_Static_assert(offsetof(sx_fact_kinds, disguised_kind) == 12, "sx_fact_kinds.disguised_kind layout");
_Static_assert(offsetof(sx_fact_kinds, has_disguised_kind) == 16, "sx_fact_kinds.has_disguised_kind layout");
_Static_assert(sizeof(sx_alert_amounts) == 80, "sx_alert_amounts layout");
_Static_assert(offsetof(sx_alert_amounts, recognized) == 4, "sx_alert_amounts.recognized layout");
_Static_assert(offsetof(sx_alert_amounts, seen) == 8, "sx_alert_amounts.seen layout");
_Static_assert(offsetof(sx_alert_amounts, heard) == 12, "sx_alert_amounts.heard layout");
_Static_assert(offsetof(sx_alert_amounts, cue_bits) == 16, "sx_alert_amounts.cue_bits layout");
_Static_assert(offsetof(sx_alert_amounts, cue_amount) == 48, "sx_alert_amounts.cue_amount layout");
_Static_assert(sizeof(sx_perception_trace) == 64, "sx_perception_trace layout");
_Static_assert(offsetof(sx_perception_trace, target) == 4, "sx_perception_trace.target layout");
_Static_assert(offsetof(sx_perception_trace, why) == 8, "sx_perception_trace.why layout");
_Static_assert(offsetof(sx_perception_trace, sense) == 12, "sx_perception_trace.sense layout");
_Static_assert(offsetof(sx_perception_trace, channel) == 16, "sx_perception_trace.channel layout");
_Static_assert(offsetof(sx_perception_trace, gain) == 20, "sx_perception_trace.gain layout");
_Static_assert(offsetof(sx_perception_trace, cost) == 24, "sx_perception_trace.cost layout");
_Static_assert(offsetof(sx_perception_trace, range) == 28, "sx_perception_trace.range layout");
_Static_assert(offsetof(sx_perception_trace, cell) == 32, "sx_perception_trace.cell layout");
_Static_assert(offsetof(sx_perception_trace, clarity) == 44, "sx_perception_trace.clarity layout");
_Static_assert(offsetof(sx_perception_trace, distance) == 46, "sx_perception_trace.distance layout");
_Static_assert(offsetof(sx_perception_trace, light) == 48, "sx_perception_trace.light layout");
_Static_assert(offsetof(sx_perception_trace, crowd) == 50, "sx_perception_trace.crowd layout");
_Static_assert(offsetof(sx_perception_trace, doubt) == 52, "sx_perception_trace.doubt layout");
_Static_assert(offsetof(sx_perception_trace, band) == 54, "sx_perception_trace.band layout");
_Static_assert(offsetof(sx_perception_trace, rule) == 55, "sx_perception_trace.rule layout");
_Static_assert(offsetof(sx_perception_trace, from) == 56, "sx_perception_trace.from layout");
_Static_assert(offsetof(sx_perception_trace, to) == 57, "sx_perception_trace.to layout");
_Static_assert(offsetof(sx_perception_trace, clear) == 58, "sx_perception_trace.clear layout");
_Static_assert(offsetof(sx_perception_trace, pending) == 59, "sx_perception_trace.pending layout");
_Static_assert(offsetof(sx_perception_trace, has_cell) == 60, "sx_perception_trace.has_cell layout");
_Static_assert(offsetof(sx_perception_trace, has_cost) == 61, "sx_perception_trace.has_cost layout");
#endif

#endif
