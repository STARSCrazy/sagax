/* Sagax from C: four guards ambush an intruder, built from the game's own
 * pieces.
 *
 * A 20 x 12 map with a wall at x = 10 (rows 0 to 8, a gap below) and a
 * short one at x = 17 (rows 0 to 4). Intruder 99 walks east through the
 * gap. The guards are on the east side and walk to the spot of their order
 * (a cell per 2 ticks), so what is hidden is where they really stand.
 *
 * Sight is the library's: sx_perception_line over sx_sense_map_from_grid
 * (the walls of the map) in a perception world of 256 fine units per cell,
 * from cell centre to cell centre. The intruder carries a lit torch (a cue
 * bit) and is an sx_target: every tick a lookout (5, not in the crew, on a
 * tower at (6, 1) facing south) and the four guards (facing west, toward
 * the wall) look at it (sx_perception_look), and each percept goes to
 * sx_crew_observe through sx_percept_fact. A cue rule "a torch is noticed at
 * once" (gain 0: it is only instant) lets the lookout notice it on tick 0.
 * The crew knows only what they saw. The game hands the squad a rules
 * object (sx_squad_rules) with five callbacks of its own:
 *
 *  - spot_score: a spot the intruder can see (sx_perception_line) scores
 *    far worse, so the guards hide (sneaking, cover against this one
 *    enemy);
 *  - channel_delay: hand signs arrive one tick later, only when the
 *    receiver sees the sender (sx_perception_in_sight with the receiver's
 *    facing and a wide periphery; guard 4 stands behind the short wall: the
 *    leader never reaches it);
 *  - trigger: the intruder steps into the kill zone, the signal goes;
 *  - report: a guard that was seen says so (a kind of the game);
 *  - abort: that report ends the ambush; the squad plans a rush.
 *
 * Scene A: the intruder walks into the kill zone and the guards strike
 * together. Scene B: the intruder stops at the wall, steps to the gap and
 * sees guard 3; the ambush is dropped and the squad rushes. Guard 2, at
 * (12, 1) with the leader behind it, never sees the sign and stays.
 *
 * The same scenes are in crates/sagax/examples/ambush_rules.rs; both print
 * the same lines. Build and run: sh examples/c/run.sh */

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include "sagax.h"

#define CHECK(cond) \
	do { \
		if (!(cond)) { \
			fprintf(stderr, "failed (line %d): %s\n", __LINE__, #cond); \
			return 1; \
		} \
	} while (0)

#define END          80u   /* ticks 0 ..= END */
#define WIDTH        20
#define HEIGHT       12
#define WALL_X       10    /* the wall covers rows 0 to WALL_TOP */
#define WALL_TOP     8
#define SHIELD_X     17    /* a short second wall, rows 0 to SHIELD_TOP */
#define SHIELD_TOP   4
#define INTRUDER     99u
#define ENEMY        1u    /* the fact kind of the intruder */
#define SKILL        192u
#define SEED         7u
#define GUARDS       4
#define EVENTS       16u   /* room for one tick's events */
#define STEP         2u    /* ticks per cell of the walk */
/* Ids of the pieces in the rules object (the game's own numbers). */
#define HAND_SIGNS   1u
#define KILL_ZONE    1u
#define SEEN         1u
/* The report kind of the game: "a guard was seen". */
#define REPORT_SEEN  SX_REPORT_GAME_FIRST
/* Added to the score of a spot the intruder can see. */
#define SEEN_PENALTY 10000
#define CELL         256   /* fine units per cell of the perception world */
#define LOOKOUT      5u    /* the lookout's id; it is not in the crew */
#define LOOKOUT_X    6
#define LOOKOUT_Y    1
#define SOUTH        64    /* headings: the lookout faces south, the guards west */
#define WEST         128
#define TORCH        1u    /* the intruder's cue bit: a lit torch */
#define WATCHERS     5     /* the lookout, then the guards */
#define PERCEPTS     8     /* 4 * (checks + instant_cap) of one look */

/* The game's pieces, behind the user pointer of the rules table. */
struct ambush_rules {
	sx_perception *sight;               /* the world over the walls */
	const sx_sense_profile *signs;      /* the eye for hand signs */
	int32_t intruder[3];                /* where the intruder is now */
	int seen;                           /* a guard reported being seen */
};

/* One guard's walk: a cell per STEP ticks towards the goal of its order. */
struct walk {
	int has_goal;
	int32_t goal[3];
	uint32_t from;
};

static int32_t sign(int32_t v)
{
	return v < 0 ? -1 : (v > 0 ? 1 : 0);
}

/* The fine point at the centre of cell c. */
static void centre(sx_perception *p, const int32_t *c, int32_t *out)
{
	sx_point cell;

	cell.x = c[0];
	cell.y = c[1];
	cell.level = c[2];
	(void)sx_perception_cell_centre(p, cell, out);
}

/* Whether sight gets from cell from to cell to over the walls: the exact
 * sight line between the two centres. */
static int sees(sx_perception *p, const int32_t *from, const int32_t *to)
{
	int32_t a[3], b[3];
	sx_line line;

	centre(p, from, a);
	centre(p, to, b);
	return sx_perception_line(p, a, b, &line) == 0 && line.clear;
}

/* The eye of id at cell at. */
static void make_eye(sx_perception *p, uint32_t id, const int32_t *at, int32_t facing,
                     const sx_sense_profile *profile, sx_eye *eye)
{
	memset(eye, 0, sizeof *eye);
	eye->id = id;
	centre(p, at, eye->point);
	eye->facing = facing;
	eye->profile = profile;
	eye->stats.sight_scale = 256;
	eye->stats.notice_scale = 256;
	eye->stats.hearing_scale = 256;
}

/* Whether a guard at cell receiver, facing west, sees a hand sign made at
 * cell sender: range, cone and line, with the watcher preset, whose wide
 * periphery (half-angle 100) catches a sign from the corner of the eye. */
static int sees_sign(const struct ambush_rules *r, const int32_t *sender, const int32_t *receiver)
{
	sx_eye eye;
	int32_t at[3];

	make_eye(r->sight, 0, receiver, WEST, r->signs, &eye);
	centre(r->sight, sender, at);
	return sx_perception_in_sight(r->sight, &eye, at) == 1;
}

/* The kill zone: x 11 to 14, y 9 to 11. */
static int in_kill_zone(const int32_t *p)
{
	return p[0] >= 11 && p[0] <= 14 && p[1] >= 9 && p[1] <= 11;
}

/* Where the intruder is at t. Both scenes: east from (2, 10), a cell per
 * STEP ticks. Scene A walks on. Scene B stops at (8, 10) in front of the
 * wall and steps to (9, 10) at tick 24, where the gap lets it see guard 3. */
static void intruder_at(int scene_b, uint32_t t, int32_t *out)
{
	int32_t walked = (int32_t)(t / STEP);
	int32_t x = 2 + walked;

	if (!scene_b)
		out[0] = x < WIDTH - 1 ? x : WIDTH - 1;
	else if (t < 24)
		out[0] = x < 8 ? x : 8;
	else
		out[0] = 9;
	out[1] = 10;
	out[2] = 0;
}

/* Slot 5: a spot the intruder can see scores far worse. */
static int32_t cb_spot_score(void *user, const sx_spot_ctx *ctx, int64_t *score)
{
	const struct ambush_rules *r = user;
	int64_t penalty = sees(r->sight, r->intruder, ctx->spot.pos) ? SEEN_PENALTY : 0;

	/* saturating add: built_in is INT64_MAX when the miss part is huge */
	*score = ctx->built_in > INT64_MAX - penalty ? INT64_MAX : ctx->built_in + penalty;
	return 1;
}

/* Slot 8: a hand sign arrives one tick later, only in sight. */
static int32_t cb_channel_delay(void *user, uint32_t id, const sx_channel_ctx *ctx,
                                uint32_t *delay)
{
	const struct ambush_rules *r = user;

	if (id == HAND_SIGNS && sees_sign(r, ctx->from, ctx->to)) {
		*delay = 1;
		return 1;
	}
	return 0;
}

/* Slot 6: the intruder is in the kill zone. */
static int32_t cb_trigger(void *user, uint32_t id, const sx_squad_view *view, uint32_t now)
{
	const struct ambush_rules *r = user;

	(void)view;
	(void)now;
	return id == KILL_ZONE && in_kill_zone(r->intruder);
}

/* Slot 9: the leader reads the report of the game. */
static void cb_report(void *user, const sx_squad_view *view, const sx_message *msg,
                      uint32_t now, sx_report_actions *actions)
{
	struct ambush_rules *r = user;

	(void)view;
	(void)now;
	(void)actions;
	if (msg->kind == REPORT_SEEN)
		r->seen = 1;
}

/* Slot 7: the report ends the ambush. */
static int32_t cb_abort(void *user, uint32_t id, const sx_squad_view *view,
                        const sx_row_ctx *ctx)
{
	const struct ambush_rules *r = user;

	(void)view;
	(void)ctx;
	return id == SEEN && r->seen;
}

static const char *kind_name(uint32_t kind)
{
	switch (kind) {
	case SX_CREW_PLANNED: return "planned";
	case SX_CREW_ACTIVATED: return "activated";
	case SX_CREW_SIGNAL: return "signal";
	case SX_CREW_ORDER: return "order";
	case SX_CREW_START: return "start";
	case SX_CREW_ARRIVED: return "arrived";
	case SX_CREW_BROKEN: return "broken";
	case SX_CREW_NO_PLAN: return "noplan";
	default: return "?";
	}
}

static sx_spot spot(int32_t x, int32_t y)
{
	sx_spot s = {{0}, 0};

	s.pos[0] = x;
	s.pos[1] = y;
	s.tags = SX_TAG_HIDDEN;
	return s;
}

/* The crew of four guards, guard 1 leads. The ambush is built by hand: the
 * shipped one with the game's trigger and abort and a distance consideration
 * scaled to this small map (0 to 16, or the plain rush would always score
 * higher). */
static sx_crew *scene_crew(const int32_t guards[GUARDS][3])
{
	sx_crew_config config;
	sx_crew *crew;
	sx_squad *squad;
	sx_tactic *ambush;
	sx_slot slot = {0};
	sx_consideration linear;
	int32_t i;

	if (sx_crew_config_default(&config) != 0)
		return NULL;
	config.squad.members = GUARDS;
	config.squad.metric = SX_METRIC_CHEBYSHEV;
	crew = sx_crew_new(&config);
	if (crew == NULL)
		return NULL;
	for (i = 0; i < GUARDS; i++) {
		sx_member m = {0};

		m.id = (uint32_t)guards[i][0];
		m.pos[0] = guards[i][1];
		m.pos[1] = guards[i][2];
		m.skill = SKILL;
		m.state = SX_MEMBER_ACTIVE;
		if (sx_crew_add_member(crew, &m) != i)
			return NULL;
	}
	squad = sx_crew_squad(crew);
	for (i = 0; i < GUARDS; i++)
		if (sx_squad_set_channel_custom(squad, (uint32_t)guards[i][0], HAND_SIGNS) != 1)
			return NULL;
	if (sx_squad_set_leader(squad, 1) != 1 || sx_squad_set_skill(squad, SKILL) != 0 ||
	    sx_squad_set_target_kind(squad, ENEMY) != 0)
		return NULL;

	ambush = sx_tactic_new(SX_ONE * 3 / 4, 128, 1);
	if (ambush == NULL)
		return NULL;
	slot.min = 1;
	slot.max = 255;
	slot.place.kind = SX_PLACE_TAGGED;
	slot.place.value = SX_TAG_HIDDEN;
	slot.gather.kind = SX_PLACE_NONE;
	slot.kind = SX_GOAL_ATTACK;
	slot.start = SX_START_ON_SIGNAL;
	slot.dist = 4;
	slot.gather_dist = 2;
	linear.input = SX_IN_TARGET_DIST;
	linear.curve = SX_CURVE_LINEAR;
	linear.min = 0;
	linear.max = 16;
	if (sx_tactic_add_slot(ambush, &slot) != 0 ||
	    sx_tactic_add_consideration(ambush, &linear) != 0 ||
	    sx_tactic_add_abort(ambush, SX_IN_LOSSES, 2) != 0 ||
	    sx_tactic_add_abort(ambush, SX_IN_OFF_PLAN, 2) != 0 ||
	    sx_tactic_add_abort(ambush, SX_IN_TARGET_AGE, 255) != 0 ||
	    sx_tactic_add_abort_custom(ambush, SEEN) != 0 ||
	    sx_tactic_trigger_custom(ambush, KILL_ZONE) != 0) {
		sx_tactic_free(ambush);
		return NULL;
	}
	if (sx_squad_add_tactic(squad, ambush) != 0 || sx_squad_add_rush(squad) != 1)
		return NULL;
	return crew;
}

/* Runs scene A (0) or B (1) for ticks 0 ..= END over the perception world
 * sight and prints its events. */
static int run_scene(int scene_b, sx_perception *sight)
{
	static const int32_t guards[GUARDS][3] = {
		{1, 15, 3}, {2, 14, 4}, {3, 16, 4}, {4, 19, 2}  /* id, x, y */
	};
	/* The bait (12, 10) and (13, 11) is in the intruder's view from the
	 * start, the rest is out of sight; scene B adds two places that only
	 * the gap shows. */
	static const int32_t cells[8][2] = {
		{12, 10}, {13, 11}, {11, 3}, {12, 1}, {11, 5}, {11, 1}, {16, 3}, {16, 4}
	};
	sx_spot spots[8];
	sx_squad_rules table;
	struct ambush_rules rules;
	struct walk walks[GUARDS] = {{0}};
	int32_t at_guard[GUARDS][3];
	sx_crew_event events[EVENTS];
	sx_crew *crew;
	uint32_t spot_count = scene_b ? 8u : 6u;
	int sent = 0;
	uint32_t t;
	int32_t i;
	static const int32_t lookout[3] = {LOOKOUT_X, LOOKOUT_Y, 0};
	static const sx_fact_kinds kinds = {ENEMY, ENEMY, ENEMY, 0, 0};
	sx_sense_profile watch, signs;
	sx_awareness aware[WATCHERS];   /* the lookout first, then the guards */
	sx_attention attention[WATCHERS];
	sx_budget budget;
	sx_percept percepts[PERCEPTS];

	printf("scene %c\n", scene_b ? 'B' : 'A');
	crew = scene_crew(guards);
	CHECK(crew != NULL);
	for (i = 0; i < (int32_t)spot_count; i++)
		spots[i] = spot(cells[i][0], cells[i][1]);
	for (i = 0; i < GUARDS; i++) {
		at_guard[i][0] = guards[i][1];
		at_guard[i][1] = guards[i][2];
		at_guard[i][2] = 0;
	}

	/* The eyes of the lookout and the guards: the guard preset plus "a
	 * torch is noticed at once in the focus" (gain 0: the rule is only
	 * instant). */
	CHECK(sx_sense_profile_preset(SX_SENSE_PRESET_GUARD, CELL, &watch) == 0);
	watch.cues[0].bits = TORCH;
	watch.cues[0].gain = 0;
	watch.cues[0].instant = 1;
	CHECK(sx_sense_profile_preset(SX_SENSE_PRESET_WATCHER, CELL, &signs) == 0);
	memset(aware, 0, sizeof aware);
	memset(attention, 0, sizeof attention);
	budget.checks = 1;
	budget.keep = 1;
	budget.instant_cap = 1;
	rules.sight = sight;
	rules.signs = &signs;
	intruder_at(scene_b, 0, rules.intruder);
	rules.seen = 0;
	CHECK(sx_squad_rules_default(&table) == 0);
	table.spot_score = cb_spot_score;
	table.channel_delay = cb_channel_delay;
	table.trigger = cb_trigger;
	table.report = cb_report;
	table.abort = cb_abort;
	table.user = &rules;

	for (t = 0; t <= END; t++) {
		uint32_t total = 0;
		sx_target target;
		int32_t n, k;

		/* 1. The guards walk to the goal of their last order, a cell per
		 * STEP ticks after it arrived. */
		for (i = 0; i < GUARDS; i++) {
			uint32_t d = t - walks[i].from;

			if (walks[i].has_goal && d > 0 && d < 0x80000000u && d % STEP == 0) {
				at_guard[i][0] += sign(walks[i].goal[0] - at_guard[i][0]);
				at_guard[i][1] += sign(walks[i].goal[1] - at_guard[i][1]);
				CHECK(sx_crew_set_pos(crew, (uint32_t)guards[i][0], at_guard[i]) == 1);
			}
		}

		/* 2. The intruder moves; the lookout and the guards look at it, and
		 * what they perceive goes to the crew. */
		intruder_at(scene_b, t, rules.intruder);
		memset(&target, 0, sizeof target);
		target.id = INTRUDER;
		centre(sight, rules.intruder, target.point);
		target.size = 256;
		target.conspicuity = 256;
		target.cues = TORCH;
		for (k = 0; k < WATCHERS; k++) {
			sx_eye eye;
			int32_t m;

			if (k == 0)
				make_eye(sight, LOOKOUT, lookout, SOUTH, &watch, &eye);
			else
				make_eye(sight, (uint32_t)guards[k - 1][0], at_guard[k - 1], WEST, &watch,
				         &eye);
			n = sx_perception_look(sight, t, &eye, &target, &aware[k], 1, &attention[k],
			                       &budget, percepts, PERCEPTS, NULL);
			CHECK(n >= 0);
			for (m = 0; m < n; m++) {
				sx_fact fact;

				if (sx_percept_fact(&percepts[m], &kinds, &fact) == 1)
					CHECK(sx_crew_observe(crew, t, &fact, NULL) >= 0);
			}
		}

		/* 3. Scene B: guard 3 is seen and tells the leader. */
		if (scene_b && !sent && sees(sight, rules.intruder, at_guard[2])) {
			sx_message msg = {0};

			msg.kind = REPORT_SEEN;
			msg.sender = 3;
			msg.subject = INTRUDER;
			CHECK(sx_crew_report_with(crew, t, &msg, &table) == 1);
			sent = 1;
		}

		/* 4. The group. */
		n = sx_crew_tick_with(crew, t, NULL, 0, spots, spot_count, SEED, &table, events,
		                      EVENTS, &total);
		CHECK(n >= 0 && (uint32_t)n == total);
		for (i = 0; i < n; i++) {
			const sx_crew_event *e = &events[i];

			if (e->kind == SX_CREW_ORDER) {
				int32_t k;

				for (k = 0; k < GUARDS; k++) {
					if ((uint32_t)guards[k][0] != e->member)
						continue;
					walks[k].has_goal = e->goal.has_pos;
					walks[k].goal[0] = e->goal.pos[0];
					walks[k].goal[1] = e->goal.pos[1];
					walks[k].from = t;
				}
			}
			switch (e->kind) {
			case SX_CREW_PLANNED:
			case SX_CREW_ACTIVATED:
				printf("t=%3u %s plan %u\n", (unsigned)t, kind_name(e->kind),
				       (unsigned)e->plan);
				break;
			case SX_CREW_ORDER:
			case SX_CREW_START:
			case SX_CREW_ARRIVED:
			case SX_CREW_BROKEN:
				printf("t=%3u %s member %u\n", (unsigned)t, kind_name(e->kind),
				       (unsigned)e->member);
				break;
			default:
				printf("t=%3u %s\n", (unsigned)t, kind_name(e->kind));
			}
		}
	}
	sx_crew_free(crew);
	return 0;
}

int main(void)
{
	sx_perception_config cfg;
	sx_sense_map map;
	sx_perception *sight;
	sx_grid *walls;
	int32_t y;
	int result;

	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "sagax library too old\n");
		return 1;
	}
	/* The walls: a grid whose blocked cells stop sight. */
	walls = sx_grid_new(WIDTH, HEIGHT, 1);
	CHECK(walls != NULL);
	for (y = 0; y < HEIGHT; y++) {
		sx_point cell;

		cell.y = y;
		cell.level = 0;
		cell.x = WALL_X;
		if (y <= WALL_TOP)
			CHECK(sx_grid_set(walls, cell, -1) == 0);
		cell.x = SHIELD_X;
		if (y <= SHIELD_TOP)
			CHECK(sx_grid_set(walls, cell, -1) == 0);
	}
	CHECK(sx_sense_map_from_grid(walls, &map) == 0);
	CHECK(sx_perception_config_default(SX_GRID_SQUARE, CELL, &cfg) == 0);
	sight = sx_perception_new(&cfg, &map);
	CHECK(sight != NULL);
	result = run_scene(0, sight) != 0 || run_scene(1, sight) != 0;
	sx_perception_free(sight);
	sx_grid_free(walls);
	return result;
}
