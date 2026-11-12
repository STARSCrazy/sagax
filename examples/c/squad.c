/* Sagax group tactics from C: a leader gathers its nearest free members,
 * plans a flank from three sides, waits until all have gathered and gives
 * one signal, so every attack starts at the same tick.
 * Build and run: sh examples/c/run.sh */

#include <stdio.h>
#include "sagax.h"

/* The structs are shared with the library as plain memory: check the layout
 * at compile time (a negative array size is an error). */
typedef char layout_member[sizeof(sx_member) == 24 ? 1 : -1];
typedef char layout_spot[sizeof(sx_spot) == 16 ? 1 : -1];
typedef char layout_gather_candidate[sizeof(sx_gather_candidate) == 24 ? 1 : -1];
typedef char layout_place[sizeof(sx_place) == 20 ? 1 : -1];
typedef char layout_slot[sizeof(sx_slot) == 72 ? 1 : -1];
typedef char layout_goal[sizeof(sx_goal) == 68 ? 1 : -1];
typedef char layout_order[sizeof(sx_order) == 72 ? 1 : -1];
typedef char layout_plan[sizeof(sx_plan) == 28 ? 1 : -1];
typedef char layout_skill_table[sizeof(sx_skill_table) == 100 ? 1 : -1];

#define CHECK(cond) \
	do { \
		if (!(cond)) { \
			fprintf(stderr, "failed (line %d): %s\n", __LINE__, #cond); \
			return 1; \
		} \
	} while (0)

#define ENEMY  1u    /* the fact kind of enemies */
#define E1     10u   /* seen by the leader */
#define E2     11u   /* reported by member 2 */
#define FACTION 1u
#define MEMBERS 4
#define STEP   2u    /* ticks per cell a member walks */

/* One member as the game moves it. */
typedef struct walker {
	uint32_t id;
	int has_goal;
	sx_goal goal;
	uint32_t start_at;
	int gathered;
} walker;

static int32_t chebyshev(int32_t ax, int32_t ay, int32_t bx, int32_t by)
{
	int32_t dx = ax > bx ? ax - bx : bx - ax;
	int32_t dy = ay > by ? ay - by : by - ay;

	return dx > dy ? dx : dy;
}

/* One cell from pos toward to: the neighbour nearest to it, ties to the
 * first in the list. */
static void step_toward(int32_t *pos, const int32_t *to)
{
	static const int32_t dirs[8][2] = {
		{1, 0}, {1, 1}, {0, 1}, {-1, 1}, {-1, 0}, {-1, -1}, {0, -1}, {1, -1}
	};
	int32_t best_x = pos[0], best_y = pos[1];
	int32_t best = -1;
	int i;

	if (pos[0] == to[0] && pos[1] == to[1])
		return;
	for (i = 0; i < 8; i++) {
		int32_t x = pos[0] + dirs[i][0];
		int32_t y = pos[1] + dirs[i][1];
		int32_t left = chebyshev(x, y, to[0], to[1]);

		if (best < 0 || left < best) {
			best = left;
			best_x = x;
			best_y = y;
		}
	}
	pos[0] = best_x;
	pos[1] = best_y;
}

static sx_gather_candidate candidate(uint32_t id, int32_t x, int32_t y, uint8_t is_free)
{
	sx_gather_candidate c = {0};

	c.id = id;
	c.faction = FACTION;
	c.pos[0] = x;
	c.pos[1] = y;
	c.free = is_free;
	return c;
}

static sx_spot spot(int32_t x, int32_t y, uint32_t tags)
{
	sx_spot s = {{0}, 0};

	s.pos[0] = x;
	s.pos[1] = y;
	s.tags = tags;
	return s;
}

int main(void)
{
	sx_gather_candidate around[5];
	sx_spot spots[5];
	uint32_t called[3];
	walker walkers[MEMBERS];
	sx_order out[8];
	sx_squad *squad;
	sx_member m = {0};
	sx_fact seen = {0};
	sx_message told = {0};
	uint32_t signal_at = 0, attack_at = 0, plans = 0, signals = 0, attacks = 0;
	uint32_t t;
	int32_t n, i;

	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "sagax library too old\n");
		return 1;
	}

	/* The leader (1) stands at (0, 0) and calls the three nearest free
	 * members of its faction within 10 cells: 6 is busy, 7 too far. */
	around[0] = candidate(7, 30, 0, 1);
	around[1] = candidate(4, -3, 0, 1);
	around[2] = candidate(3, 0, -2, 1);
	around[3] = candidate(6, 1, 1, 0);
	around[4] = candidate(2, 0, 2, 1);
	n = sx_gather(0, 0, FACTION, 10, SX_METRIC_CHEBYSHEV, 1, around, 5, called, 3);
	CHECK(n == 3 && called[0] == 2 && called[1] == 3 && called[2] == 4);
	printf("gathered: %u %u %u\n", (unsigned)called[0], (unsigned)called[1],
	       (unsigned)called[2]);

	squad = sx_squad_new(8, 8, 4, 8, SX_METRIC_CHEBYSHEV);
	CHECK(squad != NULL);
	CHECK(sx_squad_new(0, 8, 4, 8, SX_METRIC_CHEBYSHEV) == NULL);
	m.id = 1;
	CHECK(sx_squad_add_member(squad, &m) == 0);
	walkers[0].id = 1;
	for (i = 0; i < 3; i++) {
		int32_t k;

		for (k = 0; k < 5; k++)
			if (around[k].id == called[i])
				break;
		m.id = called[i];
		m.pos[0] = around[k].pos[0];
		m.pos[1] = around[k].pos[1];
		CHECK(sx_squad_add_member(squad, &m) == i + 1);
		/* Orders reach the others by radio: 2 ticks plus 1 per cell. */
		CHECK(sx_squad_set_channel(squad, m.id, SX_CHANNEL_RADIO, 2, 1) == 1);
		walkers[i + 1].id = m.id;
	}
	for (i = 0; i < MEMBERS; i++) {
		walkers[i].has_goal = 0;
		walkers[i].start_at = 0;
		walkers[i].gathered = 0;
	}
	CHECK(sx_squad_set_leader(squad, 1) == 1);
	CHECK(sx_squad_set_target_kind(squad, ENEMY) == 0);

	/* An elite squad (skill 192) knows all three levels: rush, a line and
	 * the flank from three sides; the flank scores best. */
	CHECK(sx_squad_set_skill(squad, 192) == 0);
	CHECK(sx_squad_add_shipped(squad, SX_TACTIC_RUSH, 0) == 0);
	CHECK(sx_squad_add_shipped(squad, SX_TACTIC_LINE, 2) == 1);
	CHECK(sx_squad_add_shipped(squad, SX_TACTIC_FLANK_FRONT, 4) == 2);
	CHECK(sx_squad_add_shipped(squad, 99, 0) == -1);

	/* The leader sees E1 itself; member 2 reports E2. */
	seen.subject = E1;
	seen.kind = ENEMY;
	seen.pos[0] = 20;
	CHECK(sx_squad_observe(squad, &seen, 0, NULL) == 0);
	told.kind = SX_REPORT_ENEMY;
	told.subject = E2;
	told.pos[0] = 24;
	told.pos[1] = 10;
	told.sender = 2;
	CHECK(sx_squad_report(squad, &told, 0) == 1);

	spots[0] = spot(16, 0, SX_TAG_COVER);
	spots[1] = spot(16, 2, 0);
	spots[2] = spot(20, 4, SX_TAG_HIGH);
	spots[3] = spot(20, -4, 0);
	spots[4] = spot(12, 8, SX_TAG_COVER);

	for (t = 0; t <= 60; t++) {
		uint32_t id = 0;
		int32_t status;

		/* The game moves its members one cell per STEP ticks. */
		for (i = 0; i < MEMBERS; i++) {
			walker *w = &walkers[i];
			sx_member at;

			if (!w->has_goal)
				continue;
			CHECK(sx_squad_member_at(squad, (uint32_t)i, &at) == 1);
			if (t > w->start_at && (t - w->start_at) % STEP == 0) {
				step_toward(at.pos, w->goal.pos);
				CHECK(sx_squad_set_pos(squad, w->id, at.pos[0], at.pos[1], at.pos[2]) == 1);
			}
			if ((w->goal.has_then_pos || w->goal.start == SX_START_ON_SIGNAL) && !w->gathered &&
			    at.pos[0] == w->goal.pos[0] && at.pos[1] == w->goal.pos[1]) {
				w->gathered = 1;
				CHECK(sx_squad_arrived(squad, w->id, t) == 1);
				printf("%3u: member %u gathered\n", (unsigned)t, (unsigned)w->id);
			}
		}

		status = sx_squad_check(squad, t, NULL, 0, spots, 5, 7, &id);
		CHECK(status >= 0);
		if (status == SX_SQUAD_REPLAN) {
			sx_plan plan;

			CHECK(sx_squad_pending_plan(squad, &plan, NULL, 0) == 1);
			printf("%3u: plan %u, tactic %u on %u\n", (unsigned)t, (unsigned)id,
			       (unsigned)plan.tactic, (unsigned)plan.target);
			plans++;
		} else if (status == SX_SQUAD_SIGNAL) {
			printf("%3u: signal\n", (unsigned)t);
			signal_at = t;
			signals++;
		}

		n = sx_squad_orders_due(squad, t, out, 8);
		CHECK(n >= 0);
		for (i = 0; i < n; i++) {
			int32_t k;

			for (k = 0; k < MEMBERS; k++)
				if (walkers[k].id == out[i].member)
					break;
			CHECK(k < MEMBERS);
			walkers[k].has_goal = 1;
			walkers[k].goal = out[i].goal;
			walkers[k].start_at = t + out[i].goal.start_delay +
			                      (out[i].goal.start == SX_START_AFTER_TICKS ?
			                       out[i].goal.start_ticks : 0);
			walkers[k].gathered = 0;
			printf("%3u: order for %u to %d, %d\n", (unsigned)t, (unsigned)out[i].member,
			       (int)out[i].goal.pos[0], (int)out[i].goal.pos[1]);
		}

		for (i = 0; i < MEMBERS; i++) {
			const walker *w = &walkers[i];

			if (w->has_goal && w->goal.kind == SX_GOAL_ATTACK &&
			    w->goal.start != SX_START_ON_SIGNAL && w->start_at == t) {
				printf("%3u: member %u attacks\n", (unsigned)t, (unsigned)w->id);
				CHECK(attacks == 0 || attack_at == t);
				attack_at = t;
				attacks++;
			}
		}
	}
	CHECK(plans == 1);
	CHECK(signals == 1 && signal_at == 49);
	CHECK(attacks == MEMBERS && attack_at == 55);
	printf("flank: one plan, one signal, all attack at %u\n", (unsigned)attack_at);

	/* A member keeps its goal; postponed for longer than its skill allows,
	 * the goal counts as broken and goes back to the leader as a report. */
	{
		sx_keeper *keeper = sx_keeper_new();
		sx_skill_table table;
		sx_message broken = {0};
		int32_t inputs[5];
		uint32_t give_up;

		CHECK(keeper != NULL);
		CHECK(sx_skill_table_default(&table) == 0);
		give_up = table.give_up_ticks[192 / 64];
		CHECK(sx_keeper_set(keeper, &walkers[1].goal, 60) == 0);
		CHECK(sx_keeper_inputs(keeper, 60, 16, 2, 0, SX_METRIC_CHEBYSHEV, 1,
		                       (uint32_t)sx_discipline(192, &table), inputs) == 0);
		CHECK(inputs[0] == 1 && inputs[3] == 1);
		CHECK(sx_keeper_postpone(keeper, 61) == 0);
		CHECK(sx_keeper_broken(keeper, 61 + give_up - 1, give_up) == 0);
		CHECK(sx_keeper_broken(keeper, 61 + give_up, give_up) == 1);
		broken.kind = SX_REPORT_GOAL_BROKEN;
		broken.sender = walkers[1].id;
		broken.data = walkers[1].goal.plan;
		CHECK(sx_squad_report(squad, &broken, 61 + give_up) == 1);
		printf("member %u gave its goal up after %u ticks\n", (unsigned)walkers[1].id,
		       (unsigned)give_up);
		sx_keeper_free(keeper);
	}

	/* A tactic of the game's own: everyone holds a choke spot. */
	{
		sx_tactic *hold = sx_tactic_new(SX_ONE / 2, 0, 1);
		sx_slot slot = {0};

		CHECK(hold != NULL);
		slot.min = 1;
		slot.max = 255;
		slot.place.kind = SX_PLACE_TAGGED;
		slot.place.value = SX_TAG_CHOKE;
		slot.kind = SX_GOAL_HOLD;
		slot.start = SX_START_NOW;
		CHECK(sx_tactic_add_slot(hold, &slot) == 0);
		CHECK(sx_tactic_add_abort(hold, SX_IN_LOSSES, 2) == 0);
		CHECK(sx_tactic_set_trigger(hold, SX_TRIGGER_NONE, 0) == 0);
		CHECK(sx_squad_add_tactic(squad, hold) == 3);
		CHECK(sx_squad_validate(squad, NULL, NULL) == 0);
	}

	sx_squad_free(squad);
	return 0;
}
