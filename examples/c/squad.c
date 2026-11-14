/* Sagax from C: a squad walks around a wall and attacks from three sides
 * at once.
 *
 * A 24 x 12 grid with a wall at x = 12 (rows 0 to 7, open below). The
 * leader sees an enemy behind the wall; the crew plans a flank, sends the
 * orders by radio and gives the signal once all stand at their side. Each
 * member has its own decider: keep the goal, or idle. A member that chose
 * its goal walks its sx_nav way, one cell per STEP ticks from its start
 * tick.
 *
 * The same scene is in crates/sagax/examples/squad_walk.rs and
 * examples/js/squad.mjs; all three print the same lines.
 * Build and run: sh examples/c/run.sh */

#include <stddef.h>
#include <stdio.h>
#include "sagax.h"

#define CHECK(cond) \
	do { \
		if (!(cond)) { \
			fprintf(stderr, "failed (line %d): %s\n", __LINE__, #cond); \
			return 1; \
		} \
	} while (0)

#define END     120u  /* ticks 0 ..= END */
#define ENEMY   1u    /* the fact kind of an enemy */
#define STEP    2u    /* ticks per cell of the walk */
#define SEED    7u
#define WIDTH   24
#define HEIGHT  12
#define CELLS   (WIDTH * HEIGHT)
#define MEMBERS 4
#define SKILL   192u
#define EVENTS  16u   /* room for one tick's events */

/* One member's own side: its decider, its way and where it is on it. */
typedef struct walker {
	uint32_t id;
	int32_t pos[3];
	sx_decider *decider;
	uint32_t goal_option;
	uint32_t idle_option;
	sx_nav *nav;
	sx_point way[CELLS];
	int32_t way_len;
	int32_t next;     /* next cell of the way to take */
	uint32_t from;    /* the walk starts after this tick */
	int on_goal;      /* the member chose its goal at the last decision */
} walker;

static sx_spot spot(int32_t x, int32_t y, uint32_t tags)
{
	sx_spot s = {{0}, 0};

	s.pos[0] = x;
	s.pos[1] = y;
	s.tags = tags;
	return s;
}

static sx_point point(int32_t x, int32_t y)
{
	sx_point p;

	p.x = x;
	p.y = y;
	p.level = 0;
	return p;
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

int main(void)
{
	static const int32_t members[MEMBERS][3] = {
		{1, 2, 5}, {2, 2, 7}, {3, 2, 3}, {4, 0, 5}  /* id, x, y; member 1 leads */
	};
	static walker walkers[MEMBERS];
	sx_crew_event events[EVENTS];
	sx_spot spots[4];
	sx_rules rules;
	sx_map map;
	sx_crew_config config;
	sx_grid *grid;
	sx_crew *crew;
	sx_squad *squad;
	sx_fact seen = {0};
	uint32_t t;
	int32_t i, y;

	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "sagax library too old\n");
		return 1;
	}

	/* The map: open cells and the wall at x = 12, rows 0 to 7. */
	grid = sx_grid_new(WIDTH, HEIGHT, 1);
	CHECK(grid != NULL);
	for (y = 0; y <= 7; y++)
		CHECK(sx_grid_set(grid, point(12, y), -1) == 0);
	CHECK(sx_grid_map(grid, &map) == 0);
	CHECK(sx_rules_default(SX_GRID_SQUARE, SX_PRESET_OCTILE, &rules) == 0);
	rules.node_capacity = CELLS;

	CHECK(sx_crew_config_default(&config) == 0);
	config.squad.members = MEMBERS;
	config.squad.memory = 8;
	config.squad.areas = 4;
	config.squad.inbox = 8;
	config.squad.dangers = 4;
	config.squad.metric = SX_METRIC_CHEBYSHEV;
	crew = sx_crew_new(&config);
	CHECK(crew != NULL);
	for (i = 0; i < MEMBERS; i++) {
		sx_member m = {0};
		walker *w = &walkers[i];
		int32_t option;

		m.id = (uint32_t)members[i][0];
		m.pos[0] = members[i][1];
		m.pos[1] = members[i][2];
		m.skill = SKILL;
		m.state = SX_MEMBER_ACTIVE;
		CHECK(sx_crew_add_member(crew, &m) == i);

		w->id = m.id;
		w->pos[0] = m.pos[0];
		w->pos[1] = m.pos[1];
		w->pos[2] = 0;
		w->decider = sx_decider_new();
		CHECK(w->decider != NULL);
		option = sx_keeper_add_goal_option(w->decider, SX_ONE);
		CHECK(option >= 0);
		w->goal_option = (uint32_t)option;
		option = sx_decider_add_option(w->decider, SX_ONE / 4);
		CHECK(option >= 0);
		w->idle_option = (uint32_t)option;
		w->nav = sx_nav_new(&rules);
		CHECK(w->nav != NULL);
		w->way_len = 0;
		w->next = 0;
		w->from = 0;
		w->on_goal = 0;
	}

	squad = sx_crew_squad(crew);
	CHECK(squad != NULL);
	/* Orders reach the leader at once, the others by radio: 2 ticks plus 1
	 * per cell. */
	for (i = 1; i < MEMBERS; i++)
		CHECK(sx_squad_set_radio(squad, walkers[i].id, 2, 1) == 1);
	CHECK(sx_squad_set_leader(squad, 1) == 1);
	CHECK(sx_squad_set_skill(squad, SKILL) == 0);
	CHECK(sx_squad_set_target_kind(squad, ENEMY) == 0);
	CHECK(sx_squad_add_rush(squad) == 0);
	CHECK(sx_squad_add_formation(squad, SX_SHAPE_LINE, 2) == 1);
	CHECK(sx_squad_add_flank(squad, 3, 4) == 2);
	seen.subject = 10;
	seen.kind = ENEMY;
	seen.pos[0] = 20;
	seen.pos[1] = 5;
	CHECK(sx_crew_observe(crew, 0, &seen, NULL) == 0);

	spots[0] = spot(16, 5, SX_TAG_COVER);
	spots[1] = spot(16, 7, 0);
	spots[2] = spot(20, 9, SX_TAG_HIGH);
	spots[3] = spot(20, 1, 0);

	for (t = 0; t <= END; t++) {
		uint32_t total = 0;
		int32_t n;

		/* 1. Walk: one cell per STEP ticks after the start, for a member
		 * on its goal. */
		for (i = 0; i < MEMBERS; i++) {
			walker *w = &walkers[i];
			uint32_t d = t - w->from;

			if (w->on_goal && d > 0 && d < 0x80000000u && d % STEP == 0 &&
			    w->next < w->way_len) {
				CHECK(sx_point_to_pos(w->way[w->next], w->pos) == 0);
				w->next++;
				CHECK(sx_crew_set_pos(crew, w->id, w->pos) == 1);
			}
		}

		/* 2. The group. */
		n = sx_crew_tick(crew, t, NULL, 0, spots, 4, SEED, events, EVENTS, &total);
		CHECK(n >= 0 && (uint32_t)n == total);
		for (i = 0; i < n; i++) {
			const sx_crew_event *e = &events[i];

			if (e->kind == SX_CREW_ORDER) {
				walker *w = NULL;
				sx_point start, goal;
				int32_t k;

				for (k = 0; k < MEMBERS; k++)
					if (walkers[k].id == e->member)
						w = &walkers[k];
				CHECK(w != NULL && e->goal.has_pos);
				CHECK(sx_pos_to_point(w->pos, &start) == 0);
				CHECK(sx_pos_to_point(e->goal.pos, &goal) == 0);
				w->way_len = sx_nav_find_path(w->nav, &map, start, &goal, 1, 0,
				                              SX_OUT_CELLS, w->way, CELLS, NULL);
				CHECK(w->way_len >= 0);
				w->next = 1;
				w->from = e->has_start_at ? e->start_at : t;
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

		/* 3. Each member decides: its goal or idle. */
		for (i = 0; i < MEMBERS; i++) {
			walker *w = &walkers[i];
			sx_member_inputs in;
			sx_candidate candidates[2];
			int32_t rows[2][5];
			sx_choice choice;
			int32_t chosen, k;

			CHECK(sx_crew_member_inputs(crew, w->id, t, &in) == 1);
			CHECK(sx_keeper_set_current(sx_crew_keeper(crew, w->id), w->decider,
			                            w->goal_option, (uint32_t)in.discipline) == 0);
			for (k = 0; k < 2; k++) {
				rows[k][SX_KEEPER_IN_HAS_GOAL] = in.has_goal;
				rows[k][SX_KEEPER_IN_DIST] = in.dist;
				rows[k][SX_KEEPER_IN_AGE] = in.age;
				rows[k][SX_KEEPER_IN_SIGNAL] = in.signal;
				rows[k][SX_KEEPER_IN_DISCIPLINE] = in.discipline;
			}
			candidates[0].option = w->goal_option;
			candidates[0].target = 0;
			candidates[1].option = w->idle_option;
			candidates[1].target = 0;
			chosen = sx_decider_decide(w->decider, candidates, 2, &rows[0][0], 5,
			                           SX_PICK_BEST, 0, 0, NULL, &choice);
			CHECK(chosen >= 0);
			w->on_goal = chosen == 1 && choice.candidate == 0;
			CHECK(sx_crew_member_chose(crew, w->id, t, (uint32_t)w->on_goal) == 1);
		}
	}

	for (i = 0; i < MEMBERS; i++) {
		sx_nav_free(walkers[i].nav);
		sx_decider_free(walkers[i].decider);
	}
	sx_crew_free(crew);
	sx_grid_free(grid);
	return 0;
}
