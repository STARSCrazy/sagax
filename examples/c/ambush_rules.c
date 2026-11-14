/* Sagax from C: four guards ambush an intruder, built from the game's own
 * pieces.
 *
 * A 20 x 12 map with a wall at x = 10 (rows 0 to 8, a gap below) and a
 * short one at x = 17 (rows 0 to 4). Intruder 99 walks east through the
 * gap. The guards are on the east side and walk to the spot of their order
 * (a cell per 2 ticks), so what is hidden is where they really stand. The
 * library does not change: the game hands the squad a rules object
 * (sx_squad_rules) with five callbacks of its own, all built on a small
 * sight test (Bresenham over the walls, game code):
 *
 *  - spot_score: a spot the intruder can see scores far worse, so the
 *    guards hide (sneaking, cover against this one enemy);
 *  - channel_delay: hand signs arrive one tick later, only in sight (guard
 *    4 stands behind the short wall: the leader never reaches it);
 *  - trigger: the intruder steps into the kill zone, the signal goes;
 *  - report: a guard that was seen says so (a kind of the game);
 *  - abort: that report ends the ambush; the squad plans a rush.
 *
 * Scene A: the intruder walks into the kill zone and the guards strike
 * together. Scene B: the intruder stops at the wall, steps to the gap and
 * sees guard 3; the ambush is dropped.
 *
 * The same scenes are in crates/sagax/examples/ambush_rules.rs; both print
 * the same lines. Build and run: sh examples/c/run.sh */

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
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

/* The game's pieces, behind the user pointer of the rules table. */
struct ambush_rules {
	unsigned char wall[HEIGHT][WIDTH];  /* 1 stops sight */
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

/* Sight test: Bresenham from from to to over the walls; the two end cells
 * do not block. */
static int sees(const struct ambush_rules *r, const int32_t *from, const int32_t *to)
{
	int32_t x = from[0], y = from[1];
	int32_t dx = abs(to[0] - x), dy = -abs(to[1] - y);
	int32_t stepx = sign(to[0] - x), stepy = sign(to[1] - y);
	int32_t err = dx + dy;

	for (;;) {
		int32_t e2;

		if (x == to[0] && y == to[1])
			return 1;
		if ((x != from[0] || y != from[1]) && r->wall[y][x])
			return 0;
		e2 = 2 * err;
		if (e2 >= dy) {
			err += dy;
			x += stepx;
		}
		if (e2 <= dx) {
			err += dx;
			y += stepy;
		}
	}
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
	int64_t penalty = sees(r, r->intruder, ctx->spot.pos) ? SEEN_PENALTY : 0;

	/* saturating add: built_in is INT64_MAX when the miss part is huge */
	*score = ctx->built_in > INT64_MAX - penalty ? INT64_MAX : ctx->built_in + penalty;
	return 1;
}

/* Slot 8: a hand sign arrives one tick later, only in sight. */
static int32_t cb_channel_delay(void *user, uint32_t id, const sx_channel_ctx *ctx,
                                uint32_t *delay)
{
	const struct ambush_rules *r = user;

	if (id == HAND_SIGNS && sees(r, ctx->from, ctx->to)) {
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

/* Runs scene A (0) or B (1) for ticks 0 ..= END and prints its events. */
static int run_scene(int scene_b)
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
	int32_t i, x, y;

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

	for (y = 0; y < HEIGHT; y++)
		for (x = 0; x < WIDTH; x++)
			rules.wall[y][x] = (unsigned char)((x == WALL_X && y <= WALL_TOP) ||
			                                   (x == SHIELD_X && y <= SHIELD_TOP));
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
		sx_fact fact = {0};
		int32_t n;

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

		/* 2. The intruder moves and is seen by the leader. */
		intruder_at(scene_b, t, rules.intruder);
		fact.subject = INTRUDER;
		fact.kind = ENEMY;
		fact.pos[0] = rules.intruder[0];
		fact.pos[1] = rules.intruder[1];
		fact.seen_at = t;
		CHECK(sx_crew_observe(crew, t, &fact, NULL) >= 0);

		/* 3. Scene B: guard 3 is seen and tells the leader. */
		if (scene_b && !sent && sees(&rules, rules.intruder, at_guard[2])) {
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
	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "sagax library too old\n");
		return 1;
	}
	if (run_scene(0) != 0)
		return 1;
	return run_scene(1);
}
