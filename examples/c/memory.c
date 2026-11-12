/* Sagax world memory from C: what a character knows, who it tells, where it
 * stays out of. Build and run: sh examples/c/run.sh */

#include <stdio.h>
#include "sagax.h"

/* The structs are shared with the library as plain memory: check the layout
 * at compile time (a negative array size is an error). */
typedef char layout_fact[sizeof(sx_fact) == 36 ? 1 : -1];
typedef char layout_message[sizeof(sx_message) == 40 ? 1 : -1];
typedef char layout_area[sizeof(sx_area) == 52 ? 1 : -1];
typedef char layout_progress[sizeof(sx_progress) == 12 ? 1 : -1];
typedef char layout_interval[sizeof(sx_interval) == 8 ? 1 : -1];
typedef char layout_alertness[sizeof(sx_alertness) == 16 ? 1 : -1];
typedef char layout_alert_config[sizeof(sx_alert_config) == 52 ? 1 : -1];
typedef char layout_nearby[sizeof(sx_nearby) == 12 ? 1 : -1];
typedef char layout_pattern[sizeof(sx_point_pattern) == 96 ? 1 : -1];

#define CHECK(cond) \
	do { \
		if (!(cond)) { \
			fprintf(stderr, "failed (line %d): %s\n", __LINE__, #cond); \
			return 1; \
		} \
	} while (0)

static sx_fact make_fact(uint32_t subject, uint32_t kind, uint32_t seen_at, uint32_t strength)
{
	sx_fact f = {0};

	f.subject = subject;
	f.kind = kind;
	f.seen_at = seen_at;
	f.strength = strength;
	return f;
}

static sx_message make_message(uint32_t subject)
{
	sx_message m = {0};

	m.kind = 1;
	m.subject = subject;
	return m;
}

int main(void)
{
	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "sagax library too old\n");
		return 1;
	}

	/* Knowledge: room for two facts; a third drops the one seen longest ago. */
	{
		sx_knowledge *k = sx_knowledge_new(2, SX_EVICT_OLDEST);
		sx_fact f1 = make_fact(1, 1, 5, 1);
		sx_fact f2 = make_fact(2, 1, 6, 1);
		sx_fact f3 = make_fact(3, 1, 7, 1);
		sx_fact old = make_fact(3, 1, 1, 9);
		sx_fact fresh = make_fact(3, 1, 9, 9);
		sx_fact evicted = {0};
		sx_fact found = {0};
		sx_fact gone[4];
		sx_fact other = make_fact(8, 2, 0, 1);

		CHECK(k != NULL);
		CHECK(sx_knowledge_new(0, SX_EVICT_OLDEST) == NULL);
		CHECK(sx_knowledge_new(2, 7) == NULL);
		CHECK(sx_knowledge_learn(k, &f1, 5, &evicted) == 0);
		CHECK(sx_knowledge_learn(k, &f2, 6, &evicted) == 0);
		CHECK(sx_knowledge_learn(k, &f3, 7, &evicted) == 1);
		CHECK(evicted.subject == 1);
		CHECK(sx_knowledge_count(k) == 2);
		/* merge: an older report does not replace a fresher fact. */
		CHECK(sx_knowledge_merge(k, &old, 10, NULL) == 0);
		CHECK(sx_knowledge_get(k, 3, 1, &found) == 1 && found.strength == 1);
		CHECK(sx_knowledge_merge(k, &fresh, 10, NULL) == 0);
		CHECK(sx_knowledge_get(k, 3, 1, &found) == 1 && found.strength == 9);
		CHECK(sx_knowledge_set_data(k, 3, 1, 5) == 1);
		CHECK(sx_knowledge_set_pos(k, 3, 1, 1, 2, 3) == 1);
		CHECK(sx_knowledge_set_data(k, 99, 1, 5) == 0);
		CHECK(sx_knowledge_get(k, 3, 1, &found) == 1);
		CHECK(found.pos[0] == 1 && found.pos[2] == 3 && found.data == 5 && found.seen_at == 9);
		CHECK(sx_knowledge_get(k, 1, 1, &found) == 0);
		CHECK(sx_knowledge_at(k, 0, &found) == 1 && found.subject == 2);
		CHECK(sx_knowledge_at(k, 2, &found) == 0);
		/* A fact of another kind: expire_kind leaves it alone. */
		CHECK(sx_knowledge_learn(k, &other, 10, NULL) == 1);
		CHECK(sx_knowledge_expire_kind(k, 20, 1, 10, gone, 4) == 1 && gone[0].subject == 3);
		CHECK(sx_knowledge_count(k) == 1); /* kind 2 stays */
		CHECK(sx_knowledge_expire(k, 20, 10, gone, 4) == 1 && gone[0].subject == 8);
		CHECK(sx_knowledge_count(k) == 0);
		CHECK(sx_knowledge_count(NULL) == SX_ERROR_ARGUMENT);
		CHECK(sx_knowledge_learn(k, NULL, 1, NULL) == SX_ERROR_ARGUMENT);
		printf("knowledge: ok\n");
		sx_knowledge_free(k);
	}

	/* Outbox and inbox: a message takes time; the recipients are picked by a rule. */
	{
		sx_outbox *o = sx_outbox_new(2);
		sx_inbox *b = sx_inbox_new(2);
		sx_message m1 = make_message(1);
		sx_message m2 = make_message(2);
		sx_message m3 = make_message(3);
		sx_message out[2];
		sx_nearby c[3] = {{1, 100, 0}, {2, 10, 0}, {3, 50, 0}};
		uint32_t ids[3] = {0, 0, 0};

		CHECK(o != NULL && b != NULL);
		CHECK(sx_outbox_new(0) == NULL && sx_inbox_new(0) == NULL);
		CHECK(sx_outbox_post(o, &m1, 0, 0, 10) == 0);
		CHECK(sx_outbox_post(o, &m2, 5, 0, 10) == SX_REFUSED_COOLDOWN);
		CHECK(sx_outbox_post(o, &m2, 10, 4, 10) == 0);
		CHECK(sx_outbox_post(o, &m3, 30, 0, 10) == SX_REFUSED_FULL);
		CHECK(sx_outbox_pending(o) == 2);
		CHECK(sx_outbox_due(o, 11, out, 2) == 1 && out[0].subject == 1);
		CHECK(sx_outbox_due(o, 11, out, 2) == 0); /* the second one is due at 14 */
		CHECK(sx_outbox_due(o, 14, out, 2) == 1 && out[0].subject == 2);
		CHECK(sx_outbox_pending(o) == 0);

		/* Distance makes a message late: now + base + distance * per_unit. */
		CHECK(sx_arrival(100, 2, 10, 3) == 132);
		CHECK(sx_inbox_push(b, &m1, sx_arrival(100, 2, 10, 3)) == 1);
		CHECK(sx_inbox_push(b, &m2, 110) == 1);
		CHECK(sx_inbox_push(b, &m3, 140) == 0); /* full, and it would arrive last */
		CHECK(sx_inbox_take_due(b, 120, out, 2) == 1 && out[0].subject == 2);
		CHECK(sx_inbox_take_due(b, 132, out, 1) == 1 && out[0].subject == 1);
		CHECK(sx_inbox_count(b) == 0);

		/* Routing: the nearest two, everyone in range, one named id. */
		CHECK(sx_route(SX_ROUTE_NEAREST, 2, 0, 0, c, 3, SX_METRIC_EUCLID, 1, ids) == 2);
		CHECK(ids[0] == 2 && ids[1] == 3);
		CHECK(sx_route(SX_ROUTE_IN_RANGE, 50, 0, 0, c, 3, SX_METRIC_MANHATTAN, 1, ids) == 2);
		CHECK(ids[0] == 2 && ids[1] == 3);
		CHECK(sx_route(SX_ROUTE_ALL, 0, 0, 0, c, 3, SX_METRIC_CHEBYSHEV, 1, ids) == 3);
		CHECK(ids[0] == 1 && ids[2] == 3);
		CHECK(sx_route(SX_ROUTE_TO, 3, 0, 0, c, 3, SX_METRIC_HEX, 1, ids) == 1 && ids[0] == 3);
		CHECK(sx_route(9, 0, 0, 0, c, 3, SX_METRIC_EUCLID, 1, ids) == SX_ERROR_ARGUMENT);
		CHECK(sx_outbox_pending(NULL) == SX_ERROR_ARGUMENT);
		printf("messages: ok\n");
		sx_outbox_free(o);
		sx_inbox_free(b);
	}

	/* Areas and timers: stay out of a place, ignore an id for a while. */
	{
		sx_areas *a = sx_areas_new(4);
		sx_timers *t = sx_timers_new(2);
		sx_area wall = {0};
		sx_area mud = {0};
		sx_area found = {0};
		int32_t cell[3] = {100, 100, 0};
		int32_t far_away[3] = {900, 900, 0};
		uint32_t index = 9;
		uint32_t cost = 0;
		uint32_t left = 0;

		CHECK(a != NULL && t != NULL);
		wall.kind = 1;
		wall.pos[0] = 100;
		wall.pos[1] = 100;
		wall.radius = 10;
		wall.lifetime = 50;
		wall.blocking = 1;
		mud = wall;
		mud.kind = 2;
		mud.blocking = 0;
		mud.strength = 3;

		CHECK(sx_areas_add(a, &wall, 5) == 0);
		CHECK(sx_areas_add(a, &mud, 6) == 1);
		CHECK(sx_areas_add(a, &wall, 8) == 0); /* the same kind in the same place: renewed */
		CHECK(sx_areas_count(a) == 2);
		CHECK(sx_areas_contains(a, 100, 105, 0, &index) == 1 && index == 0);
		CHECK(sx_areas_contains_kind(a, 100, 105, 0, 2, &index) == 1 && index == 1);
		CHECK(sx_areas_contains(a, 500, 0, 0, NULL) == 0);
		CHECK(sx_areas_blocks(a, cell, far_away, far_away) == 1);
		CHECK(sx_areas_blocks(a, cell, cell, far_away) == 0); /* the start is inside */
		CHECK(sx_areas_cost(a, cell, far_away, far_away, &cost) == 0 && cost == 3);
		CHECK(sx_areas_at(a, 1, &found) == 1 && found.kind == 2 && found.since == 6);
		CHECK(sx_areas_at(a, 2, &found) == 0);
		CHECK(sx_areas_expire(a, 100) == 2);
		CHECK(sx_areas_count(a) == 0);

		sx_timers_set(t, 4, 10, 10);
		CHECK(sx_timers_active(t, 4, 15) == 1);
		CHECK(sx_timers_left(t, 4, 15, &left) == 0 && left == 5);
		CHECK(sx_timers_active(t, 4, 20) == 0);
		CHECK(sx_timers_active(t, 5, 15) == 0);
		CHECK(sx_timers_remove(t, 4) == 1);
		CHECK(sx_areas_count(NULL) == SX_ERROR_ARGUMENT);
		printf("areas, timers: ok\n");
		sx_areas_free(a);
		sx_timers_free(t);
	}

	/* Progress, interval and alertness: plain structs owned by the game. */
	{
		sx_progress p = {0};
		sx_interval iv = {0};
		sx_alertness al = {0};
		sx_alert_config cfg = {100, 1, 0, 0, 2, {30, 60}};
		int32_t levels[2] = {9, 9};
		uint32_t value = 0;

		sx_progress_start(&p, 50, 100);
		CHECK(sx_progress_stalled(&p, 110, 10) == 1);
		CHECK(sx_progress_update(&p, 40, 111, 5) == 1);
		CHECK(sx_progress_stalled(&p, 115, 10) == 0);

		CHECK(sx_interval_due(&iv, 0, 10) == 1);
		sx_interval_mark(&iv, 5);
		CHECK(sx_interval_due(&iv, 10, 10) == 0);
		CHECK(sx_interval_due(&iv, 15, 10) == 1);

		sx_alertness_stimulus(&al, 70, 0, &cfg);
		CHECK(sx_alertness_update(&al, 0, &cfg, levels) == 1);
		CHECK(levels[0] == 0 && levels[1] == 2);
		CHECK(sx_alertness_value(&al, &value) == 0 && value == 70);
		CHECK(sx_alertness_update(&al, 20, &cfg, NULL) == 1);
		CHECK(sx_alertness_level(&al) == 1);
		CHECK(sx_progress_reset(NULL) == SX_ERROR_ARGUMENT);
		printf("progress, interval, alertness: ok\n");
	}

	/* Where to look: points around a place, and the sweep of the view. */
	{
		int32_t point[3] = {0, 0, 0};
		uint32_t step;

		CHECK(sx_sine(64, SX_SINE_FINE) == 65536);
		CHECK(sx_sine(64, SX_SINE_COARSE) == 256);
		/* The default pattern: step 0, then three more points. */
		for (step = 0; step < 5; step++) {
			int32_t more = sx_points_around(3, step, 1000, 2000, 4, NULL, point);
			CHECK(more == (step <= 3 ? 1 : 0));
			if (more)
				printf("point %u: %d, %d\n", (unsigned)step, (int)point[0], (int)point[1]);
		}
		CHECK(sx_sweep_heading(10, 0, 5, NULL, 0) == 10);
		CHECK(sx_sweep_heading(10, 5, 5, NULL, 0) == 42);
	}
	return 0;
}
