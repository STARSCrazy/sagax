/* Sagax from C: two guards on one patrol meet an attacker and a theft, on a
 * mission each.
 *
 * A 16 x 8 map without walls. Both guards have the same looping patrol (go
 * to (4,3), wait 5 ticks, go to (12,3)) and the motive "guard the item".
 * Guard A is SX_LEEWAY_STRICT (leash 3, never replans), guard B a flexible
 * one (leash 12, searches 6 ticks when it loses its subject, replans). The
 * game moves a character one cell per tick toward the target of its action
 * (Chebyshev); nothing else is done for the mission.
 *
 *  - Ticks 0 to 29: nothing happens. The trace is the plain script.
 *  - Phase 2 (from tick 30): attacker 50 appears and runs east, two cells
 *    per tick, and leaves the map. A guard "sees" it within 5 cells: only
 *    then the game puts the attacker's fact into that guard's own knowledge
 *    and starts an engage detour.
 *  - Phase 3 (tick 70): only B is told that the item (20) is stolen and
 *    where the thief (30) is. The patrol breaks on that fact; B replans to
 *    its playbook plan "attack the thief". A is never told.
 *
 * One line per tick and guard:
 * t=<tick> <A|B> step=<index or -> action=<action> events=<list or ->
 * The formats are documented in crates/sagax/examples/mission_guard.rs, the
 * same scene in Rust; both print the same lines. Build and run:
 * sh examples/c/run.sh */

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "sagax.h"

#define CHECK(cond) \
	do { \
		if (!(cond)) { \
			fprintf(stderr, "failed (line %d): %s\n", __LINE__, #cond); \
			return 1; \
		} \
	} while (0)

#define END            90u  /* ticks 0 .. END - 1 */
#define ATTACKER_FROM  30u
#define THEFT_AT       70u
#define ATTACKER       50u
#define ITEM           20u
#define THIEF          30u
#define SEEN           0u   /* fact kind: where a subject is */
#define STOLEN_KIND    1u   /* fact kind: the item is stolen (data bit 1) */
#define SIGHT          5
#define MAP_WIDTH      16
#define ATTACKER_X0    7
#define ATTACKER_Y     3
#define ATTACKER_SPEED 2

static const int32_t WEST[3] = {4, 3, 0};
static const int32_t EAST[3] = {12, 3, 0};
static const int32_t THIEF_AT[3] = {13, 6, 0};

typedef struct guard {
	char name;
	sx_mission *mission;
	sx_knowledge *knowledge;
	int32_t pos[3];
} guard;

static sx_step go_to(const int32_t pos[3])
{
	sx_step s;
	memset(&s, 0, sizeof s);
	s.kind = SX_STEP_GO_TO;
	s.at.kind = SX_WHERE_FIXED;
	memcpy(s.at.pos, pos, sizeof s.at.pos);
	return s;
}

static sx_step wait_for_ticks(uint32_t ticks)
{
	sx_step s;
	memset(&s, 0, sizeof s);
	s.kind = SX_STEP_WAIT;
	s.a = ticks;
	return s;
}

static sx_step attack(uint32_t subject)
{
	sx_step s;
	memset(&s, 0, sizeof s);
	s.kind = SX_STEP_ATTACK;
	s.a = subject;
	return s;
}

/* B: the flexible preset with leash 12, searching 6 ticks within 3. */
static sx_leeway flexible(void)
{
	sx_leeway l;
	sx_leeway_preset(SX_LEEWAY_FLEXIBLE, &l);
	l.has_distance = 1;
	l.distance = 12;
	l.has_ticks = 0;
	l.ticks = 0;
	l.on_lost = SX_ON_LOST_SEARCH;
	l.lost_ticks = 6;
	l.lost_radius = 3;
	return l;
}

static int make_guard(guard *g, char name, int strict, int32_t x, int32_t y)
{
	sx_mission_config config;
	sx_motive motive;
	sx_leeway leeway;
	sx_step patrol[3];
	sx_step thief_plan[1];
	sx_fail stolen;

	memset(g, 0, sizeof *g);
	g->name = name;
	g->pos[0] = x;
	g->pos[1] = y;

	CHECK(sx_mission_config_default(&config) == 0);
	config.id = (uint32_t)name;
	config.subject_kind = SEEN;
	config.lost_after = 3;
	/* Search points stay on the 16 x 8 map. */
	config.search.min_x = 0;
	config.search.min_y = 0;
	config.search.max_x = MAP_WIDTH - 1;
	config.search.max_y = 7;

	memset(&motive, 0, sizeof motive);
	motive.kind = SX_MOTIVE_GUARD;
	memcpy(motive.place, WEST, sizeof motive.place);
	motive.has_place = 1;
	/* More playbook slots than a mission allows (65535) give no mission. */
	config.max_playbook = 65536;
	CHECK(sx_mission_new(&config, &motive) == NULL);
	config.max_playbook = 8;
	g->mission = sx_mission_new(&config, &motive);
	CHECK(g->mission != NULL);

	if (strict) {
		CHECK(sx_leeway_preset(SX_LEEWAY_STRICT, &leeway) == 0);
	} else {
		leeway = flexible();
	}
	CHECK(sx_mission_set_leeway(g->mission, &leeway) == 0);

	/* The patrol: go, wait, go, again; it breaks when the item is stolen. */
	patrol[0] = go_to(WEST);
	patrol[1] = wait_for_ticks(5);
	patrol[2] = go_to(EAST);
	memset(&stolen, 0, sizeof stolen);
	stolen.kind = SX_FAIL_FACT;
	stolen.subject = ITEM;
	stolen.fact_kind = STOLEN_KIND;
	stolen.mask = 1;
	stolen.value = 1;
	CHECK(sx_mission_set_plan(g->mission, patrol, 3, 1, &stolen, 1) == 0);

	thief_plan[0] = attack(THIEF);
	CHECK(sx_mission_add_playbook(g->mission, thief_plan, 1, 0, NULL, 0, SX_ONE, 0) >= 0);

	g->knowledge = sx_knowledge_new(8, SX_EVICT_OLDEST);
	CHECK(g->knowledge != NULL);
	return 0;
}

/* Where the attacker is at tick; 0 before it appears and after it has left
 * the map. */
static int attacker_at(uint32_t tick, int32_t pos[3])
{
	int32_t x;
	if (tick < ATTACKER_FROM)
		return 0;
	x = ATTACKER_X0 + ATTACKER_SPEED * (int32_t)(tick - ATTACKER_FROM);
	if (x >= MAP_WIDTH)
		return 0;
	pos[0] = x;
	pos[1] = ATTACKER_Y;
	pos[2] = 0;
	return 1;
}

static int32_t abs32(int32_t v)
{
	return v < 0 ? -v : v;
}

static int32_t chebyshev(const int32_t a[3], const int32_t b[3])
{
	int32_t dx = abs32(a[0] - b[0]);
	int32_t dy = abs32(a[1] - b[1]);
	return dx > dy ? dx : dy;
}

static int32_t sign(int32_t v)
{
	return (v > 0) - (v < 0);
}

/* One cell from pos toward the action's target, if it has one. */
static void step_toward(int32_t pos[3], const sx_mission_action *a)
{
	switch (a->kind) {
	case SX_ACTION_GO_TO:
	case SX_ACTION_HOLD:
	case SX_ACTION_SEARCH:
	case SX_ACTION_FLEE:
	case SX_ACTION_FOLLOW:
	case SX_ACTION_ATTACK:
		break;
	case SX_ACTION_CUSTOM:
		if (!a->has_pos)
			return;
		break;
	default:
		return;
	}
	pos[0] += sign(a->pos[0] - pos[0]);
	pos[1] += sign(a->pos[1] - pos[1]);
}

static void print_action(const sx_mission_action *a)
{
	switch (a->kind) {
	case SX_ACTION_NONE:   printf("none"); break;
	case SX_ACTION_GO_TO:  printf("go %d,%d", (int)a->pos[0], (int)a->pos[1]); break;
	case SX_ACTION_WAIT:   printf("wait"); break;
	case SX_ACTION_FOLLOW: printf("follow %u@%d,%d", (unsigned)a->subject, (int)a->pos[0], (int)a->pos[1]); break;
	case SX_ACTION_HOLD:   printf("hold %d,%d", (int)a->pos[0], (int)a->pos[1]); break;
	case SX_ACTION_ATTACK: printf("attack %u@%d,%d", (unsigned)a->subject, (int)a->pos[0], (int)a->pos[1]); break;
	case SX_ACTION_SEARCH: printf("search %d,%d", (int)a->pos[0], (int)a->pos[1]); break;
	case SX_ACTION_WANDER: printf("wander"); break;
	case SX_ACTION_FLEE:   printf("flee %d,%d", (int)a->pos[0], (int)a->pos[1]); break;
	case SX_ACTION_CUSTOM: printf("custom %u", (unsigned)a->custom); break;
	default:               printf("?"); break;
	}
}

static const char *detour_end_text(uint32_t why)
{
	switch (why) {
	case SX_DETOUR_END_DEFEATED:  return "defeated";
	case SX_DETOUR_END_FORGOTTEN: return "forgotten";
	case SX_DETOUR_END_LOST:      return "lost";
	case SX_DETOUR_END_LEASH:     return "leash";
	case SX_DETOUR_END_SAFE:      return "safe";
	case SX_DETOUR_END_ARRIVED:   return "arrived";
	default:                      return "ended";
	}
}

static const char *break_text(uint32_t why)
{
	switch (why) {
	case SX_BREAK_FAIL:       return "fail";
	case SX_BREAK_IMPOSSIBLE: return "impossible";
	default:                  return "ended";
	}
}

static void print_event(const sx_mission_event *e)
{
	switch (e->kind) {
	case SX_MISSION_EVENT_STEP_DONE:      printf("done:%u", (unsigned)e->a); break;
	case SX_MISSION_EVENT_LOOPED:         printf("looped"); break;
	case SX_MISSION_EVENT_PLAN_DONE:      printf("plandone"); break;
	case SX_MISSION_EVENT_BROKEN:         printf("broken:%s", break_text(e->a)); break;
	case SX_MISSION_EVENT_REPLANNED:      printf("replanned:%u", (unsigned)e->a); break;
	case SX_MISSION_EVENT_FELL_BACK:      printf("fellback"); break;
	case SX_MISSION_EVENT_DETOUR_STARTED: printf("detour"); break;
	case SX_MISSION_EVENT_DETOUR_ENDED:   printf("detour_end:%s", detour_end_text(e->a)); break;
	case SX_MISSION_EVENT_RETURNED:       printf("returned"); break;
	case SX_MISSION_EVENT_SIGNAL:         printf("signal:%u", (unsigned)e->a); break;
	case SX_MISSION_EVENT_CALL:           printf("call:%u,%u", (unsigned)e->a, (unsigned)e->b); break;
	case SX_MISSION_EVENT_WANTS_SQUAD:    printf("squad:%u", (unsigned)e->a); break;
	default:                              printf("?"); break;
	}
}

int main(void)
{
	guard guards[2];
	uint32_t tick;
	size_t i;
	int32_t at[3];

	if (make_guard(&guards[0], 'A', 1, 3, 2) != 0 || make_guard(&guards[1], 'B', 0, 3, 4) != 0)
		return 1;

	for (tick = 0; tick < END; tick++) {
		for (i = 0; i < 2; i++) {
			guard *g = &guards[i];
			sx_mission_out out;
			sx_fact fact;
			int32_t pos[3];
			uint32_t step;
			uint32_t e;

			/* What the guard sees goes into its own knowledge, and a
			 * guard that sees the attacker goes for it. */
			if (attacker_at(tick, at) && chebyshev(g->pos, at) <= SIGHT) {
				memset(&fact, 0, sizeof fact);
				fact.subject = ATTACKER;
				fact.kind = SEEN;
				memcpy(fact.pos, at, sizeof fact.pos);
				fact.seen_at = tick;
				fact.data = 1;
				CHECK(sx_knowledge_learn(g->knowledge, tick, &fact, NULL) >= 0);
				if (!sx_mission_in_detour(g->mission) && !sx_mission_returning(g->mission)) {
					sx_detour detour;
					memset(&detour, 0, sizeof detour);
					detour.kind = SX_DETOUR_ENGAGE;
					detour.subject = ATTACKER;
					CHECK(sx_mission_detour(g->mission, tick, &detour, g->knowledge) >= 0);
				}
			}
			/* Only B is told about the theft. */
			if (tick == THEFT_AT && g->name == 'B') {
				memset(&fact, 0, sizeof fact);
				fact.subject = ITEM;
				fact.kind = STOLEN_KIND;
				fact.data = 1;
				CHECK(sx_knowledge_learn(g->knowledge, tick, &fact, NULL) >= 0);
				memset(&fact, 0, sizeof fact);
				fact.subject = THIEF;
				fact.kind = SEEN;
				memcpy(fact.pos, THIEF_AT, sizeof fact.pos);
				fact.seen_at = tick;
				fact.data = 1;
				CHECK(sx_knowledge_learn(g->knowledge, tick, &fact, NULL) >= 0);
			}

			memcpy(pos, g->pos, sizeof pos);
			CHECK(sx_mission_tick(g->mission, tick, pos, g->knowledge, 0, &out) >= 0);

			printf("t=%u %c step=", (unsigned)tick, g->name);
			if (sx_mission_step_index(g->mission, &step) == 1)
				printf("%u", (unsigned)step);
			else
				printf("-");
			printf(" action=");
			print_action(&out.action);
			printf(" events=");
			if (out.event_count == 0)
				printf("-");
			for (e = 0; e < out.event_count; e++) {
				if (e > 0)
					printf(",");
				print_event(&out.events[e]);
			}
			printf("\n");

			step_toward(g->pos, &out.action);
		}
	}

	for (i = 0; i < 2; i++) {
		sx_mission_free(guards[i].mission);
		sx_knowledge_free(guards[i].knowledge);
	}
	return 0;
}
