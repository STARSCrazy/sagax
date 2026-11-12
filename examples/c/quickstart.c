/* Sagax from C: find a way through a small map and print it.
 * Build and run: sh examples/c/run.sh */

#include <stdio.h>
#include "sagax.h"

static const char *MAP[] = {
	"....................",
	"..######............",
	".......#....#####...",
	".......#........#...",
	"..S....#....G...#...",
	".......#........#...",
	".......######...#...",
	"....................",
};
enum { W = 20, H = 8 };

/* Can a walker step from x, y into x + dx, y + dy? Return his level or -1. */
static int32_t step(void *context, int32_t x, int32_t y, int32_t level, int32_t dx, int32_t dy)
{
	int nx = x + dx, ny = y + dy;
	(void)context;
	if (nx < 0 || ny < 0 || nx >= W || ny >= H || MAP[ny][nx] == '#')
		return -1;
	return level;
}

int main(void)
{
	sx_rules rules = {0};
	sx_map map = {0};
	sx_point start = {2, 4, 0}, goal = {12, 4, 0}, way[64];
	sx_result result;
	char grid[H][W + 1];
	sx_nav *nav;
	int i, n, y;

	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "sagax library too old\n");
		return 1;
	}
	rules.grid = SX_GRID_SQUARE;
	rules.cost_straight = 10;
	rules.cost_diagonal = 14;
	rules.node_capacity = W * H;
	nav = sx_nav_create(&rules);
	map.step = step;

	n = sx_nav_find_path(nav, &map, start, &goal, 1, 0, SX_OUT_CELLS, way, 64, &result);
	if (n < 0 || result.status != SX_FOUND) {
		fprintf(stderr, "no way (%d, status %d)\n", n, result.status);
		return 1;
	}
	for (y = 0; y < H; y++)
		snprintf(grid[y], sizeof grid[y], "%s", MAP[y]);
	for (i = 1; i + 1 < n; i++)
		grid[way[i].y][way[i].x] = '*';
	for (y = 0; y < H; y++)
		puts(grid[y]);
	printf("%d cells, cost %d\n", result.cells, result.cost);

	n = sx_nav_way(nav, &map, SX_OUT_SMOOTH, way, 64);
	printf("smoothed:");
	for (i = 0; i < n; i++)
		printf(" (%d,%d)", way[i].x, way[i].y);
	printf("\n");
	sx_nav_destroy(nav);

	/* A decision: attack the nearer of two targets unless health is low. */
	{
		sx_decider *d = sx_decider_new();
		sx_consideration near = {0, SX_CURVE_INVERSE, 1, 10};      /* input 0: distance */
		sx_consideration healthy = {1, SX_CURVE_LINEAR, 0, 100};   /* input 1: health */
		sx_consideration hurt = {1, SX_CURVE_INVERSE, 0, 100};
		sx_candidate candidates[3] = {{0, 7}, {0, 8}, {1, 0}};
		int32_t inputs[6] = {6, 80, 2, 80, 0, 80};
		uint32_t scores[3];
		sx_choice choice;
		int32_t attack = sx_decider_add_option(d, SX_ONE);
		int32_t flee = sx_decider_add_option(d, SX_ONE);

		sx_decider_add_consideration(d, (uint32_t)attack, &near);
		sx_decider_add_consideration(d, (uint32_t)attack, &healthy);
		sx_decider_add_consideration(d, (uint32_t)flee, &hurt);
		if (sx_decider_decide(d, candidates, 3, inputs, 2, SX_PICK_BEST, 0, 0,
		                      scores, &choice) != 1 || choice.candidate != 1) {
			fprintf(stderr, "wrong decision\n");
			return 1;
		}
		printf("decision: option %u, target %u, score %u\n",
		       candidates[choice.candidate].option, candidates[choice.candidate].target,
		       choice.score);
		sx_decider_free(d);
	}
	return 0;
}
