/* Sagax from C: a wall between a squad and an enemy, who should run?
 *
 * A yard of 16 x 10 cells (square, octile costs; the squad measures in
 * Chebyshev cells) with a wall down x 8, open only at the bottom (y 8 and
 * 9). The leader 1 at (2,8); two runners (role 1): guard 2 at (7,2) right
 * behind the wall, guard 3 at (13,8) on the enemy's side. The leader sees an
 * enemy at (12,2); the crew's one tactic sends one runner to its front.
 *
 * As the crow flies guard 2 is nearer, so the plain crew sends it round the
 * wall. With sx_crew_set_distance(crew, sx_flow_distance_fn, fd) the crew asks
 * how far each one has to walk and sends guard 3. The runner walks one cell
 * per tick along an sx_flow to its goal.
 *
 * The line formats are documented in
 * crates/sagax/examples/walk_distance_scene.rs, the same scene in Rust; both
 * print the same lines. Build and run: sh examples/c/run.sh */

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

#define WIDTH   16
#define HEIGHT  10
#define ENEMY   1u
#define TICKS   40u
#define SEED    7u
#define MEMBERS 3
#define EVENTS  16

/* id, x, y, roles */
static const int32_t members[MEMBERS][4] = {{1, 2, 8, 0}, {2, 7, 2, 1}, {3, 13, 8, 1}};
static const int32_t enemy_at[2] = {12, 2};

static int wall(int32_t x, int32_t y)
{
	return x == 8 && y <= 7;
}

static sx_point point(int32_t x, int32_t y)
{
	sx_point p;
	p.x = x;
	p.y = y;
	p.level = 0;
	return p;
}

static int32_t chebyshev(int32_t dx, int32_t dy)
{
	if (dx < 0)
		dx = -dx;
	if (dy < 0)
		dy = -dy;
	return dx > dy ? dx : dy;
}

/* The crew of the scene, the enemy seen at tick 0; NULL on failure. */
static sx_crew *make_crew(void)
{
	sx_crew_config config;
	sx_crew *crew;
	sx_squad *squad;
	sx_tactic *tactic;
	sx_slot slot;
	sx_fact fact;
	int i;

	if (sx_crew_config_default(&config) != 0)
		return NULL;
	config.squad.members = 4;
	config.squad.memory = 4;
	config.squad.areas = 1;
	config.squad.inbox = 4;
	config.squad.metric = SX_METRIC_CHEBYSHEV;
	crew = sx_crew_new(&config);
	if (crew == NULL)
		return NULL;
	for (i = 0; i < MEMBERS; i++) {
		sx_member m;
		memset(&m, 0, sizeof m);
		m.id = (uint32_t)members[i][0];
		m.pos[0] = members[i][1];
		m.pos[1] = members[i][2];
		m.roles = (uint32_t)members[i][3];
		m.skill = 255;
		if (sx_crew_add_member(crew, &m) != i)
			return NULL;
	}
	squad = sx_crew_squad(crew);
	if (sx_squad_set_leader(squad, 1) != 1 || sx_squad_set_skill(squad, 255) != 0
		|| sx_squad_set_target_kind(squad, ENEMY) != 0)
		return NULL;
	/* One runner (role 1) to the enemy's front. */
	tactic = sx_tactic_new(SX_ONE / 4, 0, 1);
	if (tactic == NULL)
		return NULL;
	memset(&slot, 0, sizeof slot);
	slot.roles = 1;
	slot.min = 1;
	slot.max = 1;
	slot.place.kind = SX_PLACE_SIDE;
	slot.place.value = SX_SIDE_FRONT;
	slot.gather.kind = SX_PLACE_NONE;
	slot.kind = SX_GOAL_ATTACK;
	slot.start = SX_START_NOW;
	slot.dist = 1;
	slot.gather_dist = 2;
	if (sx_tactic_add_slot(tactic, &slot) != 0 || sx_squad_add_tactic(squad, tactic) != 0)
		return NULL;
	memset(&fact, 0, sizeof fact);
	fact.subject = 10;
	fact.kind = ENEMY;
	fact.pos[0] = enemy_at[0];
	fact.pos[1] = enemy_at[1];
	if (sx_crew_observe(crew, 0, &fact, NULL) != 0)
		return NULL;
	return crew;
}

/* One run; walking: with fd (its search count printed at the end). */
static int run(int walking, sx_flow_distance *fd, const sx_rules *rules, const sx_map *map)
{
	sx_crew *crew = make_crew();
	sx_flow *field = sx_flow_new(rules);
	sx_crew_event events[EVENTS];
	sx_point at = point(0, 0);
	uint32_t runner = 0;
	int walker = 0;
	uint32_t t;

	CHECK(crew != NULL);
	CHECK(field != NULL);
	if (walking)
		CHECK(sx_crew_set_distance(crew, sx_flow_distance_fn, fd) == 0);
	printf("%s\n", walking ? "walking" : "metric");
	for (t = 0; t <= TICKS; t++) {
		int32_t n = sx_crew_tick(crew, t, NULL, 0, NULL, 0, SEED, events, EVENTS, NULL);
		int32_t i;
		CHECK(n >= 0);
		for (i = 0; i < n; i++) {
			const sx_crew_event *e = &events[i];
			switch (e->kind) {
			case SX_CREW_PLANNED:
				printf("t=%2u planned %u\n", (unsigned)t, (unsigned)e->plan);
				break;
			case SX_CREW_ACTIVATED:
				printf("t=%2u activated %u\n", (unsigned)t, (unsigned)e->plan);
				break;
			case SX_CREW_ORDER: {
				sx_point goal = point(e->goal.pos[0], e->goal.pos[1]);
				CHECK(sx_flow_build(field, map, &goal, 1, INT32_MAX, NULL) == 0);
				runner = e->member;
				walker = 0;
				printf("t=%2u order %u to %d,%d\n", (unsigned)t, (unsigned)e->member,
					(int)goal.x, (int)goal.y);
				break;
			}
			case SX_CREW_START: {
				sx_member m;
				int32_t k, count = sx_squad_member_count(sx_crew_squad(crew));
				for (k = 0; k < count; k++) {
					CHECK(sx_squad_member_at(sx_crew_squad(crew), (uint32_t)k, &m) == 1);
					if (m.id == e->member)
						at = point(m.pos[0], m.pos[1]);
				}
				walker = 1;
				printf("t=%2u start %u\n", (unsigned)t, (unsigned)e->member);
				break;
			}
			default:
				break;
			}
		}
		/* The runner walks one cell along its field. */
		if (walker) {
			sx_point next;
			int32_t cost = -1;
			int32_t pos[3];
			if (sx_flow_next(field, map, &at, 1, &next, NULL) != 1)
				break;
			at = next;
			pos[0] = at.x;
			pos[1] = at.y;
			pos[2] = at.level;
			CHECK(sx_crew_set_pos(crew, runner, pos) == 1);
			CHECK(sx_flow_next(field, map, &at, 1, NULL, &cost) >= 0);
			if (cost == 0) {
				printf("t=%2u member %u there\n", (unsigned)t, (unsigned)runner);
				break;
			}
		}
	}
	if (walking)
		printf("searches %lld\n", (long long)sx_flow_distance_searches(fd));
	sx_flow_free(field);
	sx_crew_free(crew);
	return 0;
}

int main(void)
{
	sx_rules rules;
	sx_grid *grid = sx_grid_new(WIDTH, HEIGHT, 1);
	sx_map map;
	sx_flow_distance *fd;
	int32_t x, y;
	int i;

	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "library too old\n");
		return 1;
	}
	CHECK(grid != NULL);
	for (y = 0; y < HEIGHT; y++)
		for (x = 0; x < WIDTH; x++)
			if (wall(x, y))
				CHECK(sx_grid_set(grid, point(x, y), -1) == 0);
	CHECK(sx_grid_map(grid, &map) == 0);
	CHECK(sx_rules_default(SX_GRID_SQUARE, SX_PRESET_OCTILE, &rules) == 0);
	rules.node_capacity = 1000;

	/* The guards' distances to the enemy. */
	fd = sx_flow_distance_new(&rules, 0);
	CHECK(fd != NULL);
	CHECK(sx_flow_distance_set_map(fd, &map) == 0);
	for (i = 0; i < MEMBERS; i++) {
		int32_t from[3], to[3];
		uint32_t walk = 0;
		int32_t r;
		if (members[i][3] == 0)
			continue;
		from[0] = members[i][1];
		from[1] = members[i][2];
		from[2] = 0;
		to[0] = enemy_at[0];
		to[1] = enemy_at[1];
		to[2] = 0;
		r = sx_flow_distance_fn(fd, from, to, &walk);
		CHECK(r == 0 || r == 1);
		printf("guard %d at %d,%d: line %d, ", (int)members[i][0], (int)from[0], (int)from[1],
			(int)chebyshev(from[0] - to[0], from[1] - to[1]));
		if (r == 1)
			printf("walk %u\n", (unsigned)walk);
		else
			printf("walk none\n");
	}
	sx_flow_distance_free(fd);

	/* Both runs, each with a fresh provider. */
	fd = sx_flow_distance_new(&rules, 0);
	CHECK(fd != NULL);
	CHECK(sx_flow_distance_set_map(fd, &map) == 0);
	CHECK(run(0, fd, &rules, &map) == 0);
	CHECK(run(1, fd, &rules, &map) == 0);
	sx_flow_distance_free(fd);
	sx_grid_free(grid);
	return 0;
}
