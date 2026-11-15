/* Sagax tracing from C: records of two modules (a search and a decision)
 * into one small fixed-size ring, then a short summary of it.
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

static int32_t step(void *context, int32_t x, int32_t y, int32_t level, int32_t dx, int32_t dy)
{
	int nx = x + dx, ny = y + dy;
	(void)context;
	if (nx < 0 || ny < 0 || nx >= W || ny >= H || MAP[ny][nx] == '#')
		return -1;
	return level;
}

/* What the ring keeps of a record: where it came from, its kind, two numbers. */
enum { FROM_SEARCH = 1, FROM_DECIDE = 2 };
typedef struct entry {
	int from;
	uint32_t kind;
	int32_t a, b;
} entry;

/* A bounded ring that drops the oldest entry and counts what it dropped. */
enum { RING = 8 };
typedef struct ring {
	entry items[RING];
	unsigned head;      /* index of the oldest entry */
	unsigned len;
	unsigned dropped;
	unsigned seen[3][8];   /* per source and kind: how many came by */
} ring;

static void ring_add(ring *r, int from, uint32_t kind, int32_t a, int32_t b)
{
	entry e;
	e.from = from;
	e.kind = kind;
	e.a = a;
	e.b = b;
	r->seen[from][kind]++;
	if (r->len == RING) {
		r->items[r->head] = e;
		r->head = (r->head + 1) % RING;
		r->dropped++;
	} else {
		r->items[(r->head + r->len) % RING] = e;
		r->len++;
	}
}

/* Valid only during the call: copy what you keep. */
static void on_search(void *context, const sx_nav_trace *t)
{
	if (t->kind == SX_NAV_TRACE_SEARCH)
		ring_add(context, FROM_SEARCH, t->search.kind, t->search.cell.x, t->search.cell.y);
}

static void on_decide(void *context, const sx_decide_trace *t)
{
	ring_add(context, FROM_DECIDE, t->kind, t->candidate, (int32_t)t->score);
}

int main(void)
{
	sx_rules rules = {0};
	sx_map map = {0};
	sx_point start = {2, 4, 0}, goal = {12, 4, 0}, way[64];
	sx_result result;
	sx_nav *nav;
	sx_decider *d;
	ring r = {{{0, 0, 0, 0}}, 0, 0, 0, {{0}}};
	unsigned i;

	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "sagax library too old\n");
		return 1;
	}
	if (sx_rules_default(SX_GRID_SQUARE, SX_PRESET_OCTILE, &rules) != 0)
		return 1;
	rules.node_capacity = W * H;
	nav = sx_nav_new(&rules);
	map.step = step;
	sx_nav_set_trace(nav, on_search, &r);
	if (sx_nav_find_path(nav, &map, start, &goal, 1, 0, SX_OUT_CELLS, way, 64, &result) < 0
	    || result.status != SX_FOUND) {
		fprintf(stderr, "no way\n");
		return 1;
	}
	sx_nav_set_trace(nav, NULL, NULL);   /* off again: this one is not recorded */
	sx_nav_find_path(nav, &map, start, &goal, 1, 0, SX_OUT_CELLS, way, 64, &result);
	sx_nav_free(nav);

	d = sx_decider_new();
	{
		sx_consideration near = {0, SX_CURVE_INVERSE, 1, 10};
		sx_candidate candidates[2] = {{0, 7}, {0, 8}};
		int32_t inputs[2] = {6, 2};
		sx_choice choice;
		int32_t attack = sx_decider_add_option(d, SX_ONE);

		sx_decider_add_consideration(d, (uint32_t)attack, &near);
		sx_decider_set_trace(d, on_decide, &r);
		if (sx_decider_decide(d, candidates, 2, inputs, 1, SX_PICK_BEST, 0, 0, NULL, &choice) != 1
		    || choice.candidate != 1) {
			fprintf(stderr, "wrong decision\n");
			return 1;
		}
	}
	sx_decider_free(d);

	printf("search: %u started, %u expanded, %u finished\n",
	       r.seen[FROM_SEARCH][SX_SEARCH_TRACE_STARTED],
	       r.seen[FROM_SEARCH][SX_SEARCH_TRACE_EXPANDED],
	       r.seen[FROM_SEARCH][SX_SEARCH_TRACE_FINISHED]);
	printf("decide: %u factors, %u scored, %u chosen\n",
	       r.seen[FROM_DECIDE][SX_DECIDE_TRACE_FACTOR],
	       r.seen[FROM_DECIDE][SX_DECIDE_TRACE_SCORED],
	       r.seen[FROM_DECIDE][SX_DECIDE_TRACE_CHOSEN]);
	printf("ring: %u kept, %u dropped, last entries:\n", r.len, r.dropped);
	for (i = r.len - 3; i < r.len; i++) {
		const entry *e = &r.items[(r.head + i) % RING];
		printf("  %s kind %u (%d, %d)\n", e->from == FROM_SEARCH ? "search" : "decide",
		       e->kind, e->a, e->b);
	}
	return 0;
}
