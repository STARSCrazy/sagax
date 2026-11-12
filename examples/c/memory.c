/* Sagax world memory from C: what a character saw, avoids and ignores.
 * Build and run: sh examples/c/run.sh */

#include <stdio.h>
#include "sagax.h"

/* The structs are shared with the library as plain memory: check the layout
 * at compile time (a negative array size is an error). */
typedef char layout_sighting[sizeof(sx_sighting) == 24 ? 1 : -1];
typedef char layout_zone[sizeof(sx_zone) == 36 ? 1 : -1];
typedef char layout_pursuit[sizeof(sx_pursuit) == 28 ? 1 : -1];
typedef char layout_radio[sizeof(sx_radio) == 24 ? 1 : -1];
typedef char layout_interval[sizeof(sx_interval) == 8 ? 1 : -1];
typedef char layout_nearby[sizeof(sx_nearby) == 12 ? 1 : -1];
typedef char layout_pattern[sizeof(sx_search_pattern) == 92 ? 1 : -1];

#define CHECK(cond) \
	do { \
		if (!(cond)) { \
			fprintf(stderr, "failed (line %d): %s\n", __LINE__, #cond); \
			return 1; \
		} \
	} while (0)

int main(void)
{
	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "sagax library too old\n");
		return 1;
	}

	/* Sightings: room for two; a third drops the one seen longest ago. */
	{
		sx_sightings *s = sx_sightings_new(2);
		sx_sighting found = {0};
		uint32_t evicted = 0;
		uint32_t gone[4] = {0};

		CHECK(s != NULL);
		CHECK(sx_sightings_new(0) == NULL);
		CHECK(sx_sightings_seen(s, 1, 10, 20, 0, 5, &evicted) == 0);
		CHECK(sx_sightings_seen(s, 2, 11, 21, 0, 6, &evicted) == 0);
		CHECK(sx_sightings_seen(s, 3, 12, 22, 0, 7, &evicted) == 1);
		CHECK(evicted == 1);
		CHECK(sx_sightings_count(s) == 2);
		CHECK(sx_sightings_set_data(s, 3, 9) == 1);
		CHECK(sx_sightings_set_data(s, 99, 9) == 0);
		CHECK(sx_sightings_get(s, 3, &found) == 1);
		CHECK(found.pos[0] == 12 && found.pos[1] == 22 && found.last_seen == 7 && found.data == 9);
		CHECK(sx_sightings_get(s, 1, &found) == 0);
		CHECK(sx_sightings_at(s, 0, &found) == 1 && found.id == 2);
		CHECK(sx_sightings_at(s, 2, &found) == 0);
		CHECK(sx_sightings_expire(s, 20, 14, gone, 4) == 1 && gone[0] == 2);
		CHECK(sx_sightings_count(NULL) == SX_ERROR_ARGUMENT);
		printf("sightings: %d left\n", (int)sx_sightings_count(s));
		sx_sightings_free(s);
	}

	/* Zones: stay out of where something happened, for a while. */
	{
		sx_zones *z = sx_zones_new(4, 10, 50);
		int32_t source[3] = {1, 2, 3};
		int32_t cell[3] = {100, 100, 0};
		int32_t far_away[3] = {900, 900, 0};
		uint32_t index = 9;

		CHECK(z != NULL);
		CHECK(sx_zones_add(z, 100, 100, 0, 5, 7, source) == 0);
		CHECK(sx_zones_add(z, 103, 100, 0, 8, 7, NULL) == 0); /* renews the same zone */
		CHECK(sx_zones_count(z) == 1);
		CHECK(sx_zones_contains(z, 100, 105, 0, &index) == 1 && index == 0);
		CHECK(sx_zones_contains(z, 500, 0, 0, NULL) == 0);
		CHECK(sx_zones_blocks(z, cell, far_away, far_away) == 1);
		CHECK(sx_zones_blocks(z, cell, cell, far_away) == 0); /* the start is inside */
		CHECK(sx_zones_expire(z, 100) == 1);
		CHECK(sx_zones_count(z) == 0);
		printf("zones: blocked, then expired\n");
		sx_zones_free(z);
	}

	/* Ignore: leave an id alone for a while. */
	{
		sx_ignore *ig = sx_ignore_new(2);

		CHECK(ig != NULL);
		sx_ignore_add(ig, 4, 10);
		CHECK(sx_ignore_ignores(ig, 4, 15, 10) == 1);
		CHECK(sx_ignore_ignores(ig, 4, 25, 10) == 0);
		CHECK(sx_ignore_ignores(ig, 5, 15, 10) == 0);
		sx_ignore_free(ig);
	}

	/* Pursuit, radio and interval: plain structs owned by the game. */
	{
		sx_pursuit p = {0};
		sx_radio r = {0};
		sx_interval t = {0};
		int32_t xyz[3] = {0, 0, 0};

		sx_pursuit_begin(&p, 7, 1, 2, 3, 50, 100);
		sx_pursuit_missed(&p, 5);
		CHECK(sx_pursuit_lost(&p, 5) == 1);
		sx_pursuit_seen(&p, 4, 5, 6);
		CHECK(p.last_pos[0] == 4 && sx_pursuit_lost(&p, 5) == 0);
		CHECK(sx_pursuit_stalled(&p, 50, 110, 5, 10) == 1);
		sx_pursuit_clear(&p);
		CHECK(p.target == 0);

		CHECK(sx_radio_start(&r, 1, 2, 3, 10, 100) == 1);
		CHECK(sx_radio_start(&r, 1, 2, 3, 11, 100) == 0);
		CHECK(sx_radio_due(&r, 12, 5) == 0);
		CHECK(sx_radio_due(&r, 15, 5) == 1);
		CHECK(sx_radio_take(&r, xyz) == 1 && xyz[0] == 1 && xyz[2] == 3);
		CHECK(sx_radio_take(&r, xyz) == 0);

		CHECK(sx_interval_due(&t, 0, 10) == 1);
		sx_interval_mark(&t, 5);
		CHECK(sx_interval_due(&t, 10, 10) == 0);
		CHECK(sx_interval_due(&t, 15, 10) == 1);
		CHECK(sx_pursuit_lost(NULL, 1) == SX_ERROR_ARGUMENT);
		printf("pursuit, radio, interval: ok\n");
	}

	/* The nearest candidates, and where to search. */
	{
		sx_nearby c[3] = {{1, 100, 0}, {2, 10, 0}, {3, 50, 0}};
		uint32_t out[2] = {0, 0};
		int32_t point[3] = {0, 0, 0};
		uint32_t step;

		CHECK(sx_nearest(c, 3, 0, 0, 2, 1, out) == 2);
		CHECK(out[0] == 2 && out[1] == 3);

		CHECK(sx_sin256(64) == 256);
		/* The default pattern: step 0, then three search points. */
		for (step = 0; step < 5; step++) {
			int32_t more = sx_search_point(3, step, 1000, 2000, 4, NULL, point);
			CHECK(more == (step <= 3 ? 1 : 0));
			if (more)
				printf("search point %u: %d, %d\n", (unsigned)step, (int)point[0], (int)point[1]);
		}
		CHECK(sx_look_heading(10, 0, 5, NULL, 0) == 10);
		CHECK(sx_look_heading(10, 5, 5, NULL, 0) == 42);
	}
	return 0;
}
