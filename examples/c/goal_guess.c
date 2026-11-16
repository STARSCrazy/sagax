/* Sagax from C: a guard on a tower guesses which exit a runner heads for.
 *
 * A yard of 21 x 15 cells (square, octile costs) with a wall down the middle
 * (x 10, open at y 4 and y 10) and four exits: 0 top left, 1 top right, 2
 * bottom left, 3 bottom right. One sx_flow per exit. A runner walks along
 * the field of his exit; the guard sees him everywhere except under a roof
 * (x 13..15, y 10..13) and feeds only what it saw to its sx_goal_guess with
 * sx_goal_guess_observe_flow: a seen step from the last seen place; when the
 * runner comes out from under the roof the step is a jump, refused, and the
 * chain goes on from the new place. Three hunters are split over the exits
 * by chance (sx_goal_guess_split).
 *
 * Three runs: straight to exit 3 under the roof; a feint (five steps towards
 * exit 1, then to exit 2) once without forgetting and once with the default,
 * which sees through the feint sooner.
 *
 * The line formats are documented in
 * crates/sagax/examples/goal_guess_scene.rs, the same scene in Rust; both
 * print the same lines. Build and run: sh examples/c/run.sh */

#include <stdint.h>
#include <stdio.h>
#include "sagax.h"

#define CHECK(cond) \
	do { \
		if (!(cond)) { \
			fprintf(stderr, "failed (line %d): %s\n", __LINE__, #cond); \
			return 1; \
		} \
	} while (0)

#define WIDTH   21
#define HEIGHT  15
#define GOALS   4
#define HUNTERS 3u
#define FEINT   5u

static const int32_t exits[GOALS][2] = {{0, 0}, {20, 0}, {0, 14}, {20, 14}};
static const int32_t start[2] = {5, 7};

static int wall(int32_t x, int32_t y)
{
	return x == 10 && y >= 2 && y <= 12 && y != 4 && y != 10;
}

static int roofed(int32_t x, int32_t y)
{
	return x >= 13 && x <= 15 && y >= 10 && y <= 13;
}

static sx_point point(int32_t x, int32_t y)
{
	sx_point p;
	p.x = x;
	p.y = y;
	p.level = 0;
	return p;
}

/* One run: the runner follows exit first for turn steps, then exit exit;
 * the guard learns with forget. */
static int run(const char *name, sx_flow *const *flows, const sx_map *map, int first,
	uint32_t turn, int exit, uint32_t forget)
{
	sx_goal_guess_config cfg;
	sx_goal_guess *guess;
	sx_point at = point(start[0], start[1]);
	sx_point seen = at;
	int have_seen = 0;
	uint32_t t = 0;
	uint32_t recognised = 0;
	int have_recognised = 0;

	CHECK(sx_goal_guess_config_default(&cfg) == 0);
	cfg.forget = forget;
	guess = sx_goal_guess_new(&cfg, GOALS);
	CHECK(guess != NULL);
	printf("%s\n", name);
	for (;;) {
		int goal;
		int32_t cost = -1;
		sx_point next;
		if (roofed(at.x, at.y)) {
			printf("t=%2u unseen\n", (unsigned)t);
		} else {
			const char *tag = "start";
			uint32_t hunters[GOALS];
			int32_t best;
			int k;
			if (have_seen) {
				int32_t r = sx_goal_guess_observe_flow(guess, flows, GOALS, map, seen, at);
				CHECK(r == 0 || r == 1);
				tag = r == 0 ? "step" : "jump";
			}
			seen = at;
			have_seen = 1;
			best = sx_goal_guess_best(guess, NULL);
			CHECK(best >= 0);
			CHECK(sx_goal_guess_split(guess, HUNTERS, hunters, GOALS) == 0);
			printf("t=%2u %s %d,%d chance", (unsigned)t, tag, (int)at.x, (int)at.y);
			for (k = 0; k < GOALS; k++)
				printf(" %d", (int)sx_goal_guess_chance(guess, (uint32_t)k));
			printf(" best %d hunters", (int)best);
			for (k = 0; k < GOALS; k++)
				printf(" %u", (unsigned)hunters[k]);
			printf("\n");
			if (best != exit) {
				have_recognised = 0;
			} else if (!have_recognised) {
				recognised = t;
				have_recognised = 1;
			}
		}
		goal = t < turn ? first : exit;
		CHECK(sx_flow_next(flows[exit], map, &at, 1, NULL, &cost) >= 0);
		if (cost == 0)
			break;
		CHECK(sx_flow_next(flows[goal], map, &at, 1, &next, NULL) == 1);
		at = next;
		t++;
	}
	if (!have_recognised)
		recognised = t;
	printf("arrived at exit %d, recognised at t=%u\n", exit, (unsigned)recognised);
	sx_goal_guess_free(guess);
	return 0;
}

int main(void)
{
	sx_rules rules;
	sx_grid *grid = sx_grid_new(WIDTH, HEIGHT, 1);
	sx_map map;
	sx_flow *flows[GOALS];
	sx_goal_guess_config cfg;
	int32_t x, y;
	int k;

	CHECK(sx_api_version() == SX_API_VERSION);
	CHECK(grid != NULL);
	for (y = 0; y < HEIGHT; y++)
		for (x = 0; x < WIDTH; x++)
			if (wall(x, y))
				CHECK(sx_grid_set(grid, point(x, y), -1) == 0);
	CHECK(sx_grid_map(grid, &map) == 0);
	CHECK(sx_rules_default(SX_GRID_SQUARE, SX_PRESET_OCTILE, &rules) == 0);
	for (k = 0; k < GOALS; k++) {
		sx_point goal = point(exits[k][0], exits[k][1]);
		flows[k] = sx_flow_new(&rules);
		CHECK(flows[k] != NULL);
		CHECK(sx_flow_build(flows[k], &map, &goal, 1, INT32_MAX, NULL) == 0);
	}
	CHECK(sx_goal_guess_config_default(&cfg) == 0);
	CHECK(run("direct forget 13", flows, &map, 3, 0, 3, cfg.forget) == 0);
	CHECK(run("feint forget 0", flows, &map, 1, FEINT, 2, 0) == 0);
	CHECK(run("feint forget 13", flows, &map, 1, FEINT, 2, cfg.forget) == 0);
	for (k = 0; k < GOALS; k++)
		sx_flow_free(flows[k]);
	sx_grid_free(grid);
	return 0;
}
