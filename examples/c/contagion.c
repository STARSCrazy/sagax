/* Sagax from C: a guard post without orders: guards join in on what they
 * see their comrades do.
 *
 * Four guards (1 to 4) hold a post; an intruder (9) sneaks towards a doorway
 * in the east wall. Nobody gives an order. Every guard looks with its own
 * eye (sx_perception_look, the guard preset, a cue rule "a firing or fleeing
 * comrade is recognised at once in the focus"), keeps its own sx_contagion
 * fed only from its own percepts, and decides with one sx_decider whose
 * inputs are its own sight of the intruder, a grenade next to it and the
 * sx_contagion_write_columns of the deeds attack and flee: hold, attack (it
 * sees the intruder itself), join the attack (share of seen comrades
 * attacking), flee (a grenade next to it), join the flight (share of seen
 * comrades fleeing).
 *
 * Guard 1 by the doorway sees the intruder and attacks; guards 2 and 3 see
 * guard 1 but not the intruder (the wall is between) and join; guard 4
 * behind the inner wall sees none of them. The grenade next to guard 2
 * makes it flee, guard 3 sees it and flees too, guard 1 faces the doorway
 * and keeps firing. The scene runs on a square grid and on a hex grid
 * (pointy top, odd rows shifted; the scene's cells are axial).
 *
 * The line formats are documented in
 * crates/sagax/examples/contagion_scene.rs, the same scene in Rust; both
 * print the same lines. Build and run: sh examples/c/run.sh */

#include <stddef.h>
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

#define CELL        256
#define END         44u
#define INTRUDER    9u
#define GUARDS      4
#define ATTACKING   1u
#define FLEEING     2u
#define DEED_ATTACK 0u
#define DEED_FLEE   1u
#define WINDOW      6u
#define GRENADE_AT  30u

/* The acts, as Decider options. */
#define HOLD        0u
#define ATTACK      1u
#define JOIN_ATTACK 2u
#define FLEE        3u
#define JOIN_FLIGHT 4u
#define ACTS        5

/* Decider row: own sight of the intruder, grenade next to it, then the
 * columns of both deeds. */
#define COL_SEES    0
#define COL_GRENADE 1
#define ATTACK_COLS 2
#define FLEE_COLS   (ATTACK_COLS + SX_DEED_COLUMNS)
#define ROW         (FLEE_COLS + SX_DEED_COLUMNS)

/* Each guard's cell and facing (heading, 0 = +x, 64 = +y). */
static const int32_t posts[GUARDS][3] = {{8, 4, 0}, {5, 1, 32}, {5, 7, 224}, {6, 11, 192}};
static const int32_t start[2] = {18, 4};
static const int32_t stop[2] = {13, 4};
static const int32_t grenade[2] = {5, 1};
static const char *act_names[ACTS] = {"hold", "attack", "join_attack", "flee", "join_flight"};
static const char *deed_names[2] = {"attack", "flee"};

/* The walls (axial cells on hex): the east wall at x = 11 (y 0 to 8) with a
 * doorway at y = 4, the inner wall at y = 9 (x 0 to 12). */
static int wall(int32_t x, int32_t y)
{
	return (x == 11 && y >= 0 && y <= 8 && y != 4) || (y == 9 && x >= 0 && x <= 12);
}

/* Walls are opaque, daylight everywhere (no light callback). The sense map
 * gets offset cells on hex: back to axial for the walls. */
static int32_t post_sight(void *ctx, int32_t x, int32_t y, int32_t level, sx_medium *out)
{
	uint32_t grid = *(const uint32_t *)ctx;
	int32_t cell[3], axial[3];
	cell[0] = x;
	cell[1] = y;
	cell[2] = level;
	axial[0] = x;
	axial[1] = y;
	axial[2] = level;
	if (grid != SX_GRID_SQUARE && sx_hex_to_axial(grid, cell, axial) != 0)
		return -1;
	out->opaque = (uint8_t)wall(axial[0], axial[1]);
	return 0;
}

/* The fine point at the centre of cell (x, y): axial on hex. */
static void centre(uint32_t grid, int32_t x, int32_t y, int32_t *out)
{
	int32_t half = grid == SX_GRID_SQUARE ? CELL / 2 : 0;
	out[0] = x * CELL + half;
	out[1] = y * CELL + half;
	out[2] = 0;
}

static int32_t is_ally(void *ctx, uint32_t subject, int32_t disguised)
{
	(void)ctx;
	(void)disguised;
	return subject != INTRUDER;
}

/* The onsets one learn call reports. */
typedef struct onsets {
	uint32_t ally[16];
	uint32_t deed[16];
	size_t count;
} onsets;

static void on_trace(void *ctx, const sx_contagion_trace *record)
{
	onsets *o = (onsets *)ctx;
	if (record->kind == SX_CONTAGION_TRACE_ONSET && o->count < 16) {
		o->ally[o->count] = record->ally;
		o->deed[o->count] = record->deed;
		o->count++;
	}
}

static void print_act(uint32_t t, uint32_t id, uint32_t act, const sx_deed_inputs *in)
{
	printf("t=%3u guard %u %s %u/%u share %u nearest ", (unsigned)t, (unsigned)id,
		act_names[act], (unsigned)in->doing, (unsigned)in->seen, (unsigned)in->share);
	if (in->nearest == INT32_MAX)
		printf("-");
	else
		printf("%d", (int)(in->nearest / CELL));
	printf(" since ");
	if (in->since == UINT32_MAX)
		printf("-\n");
	else
		printf("%u\n", (unsigned)in->since);
}

static sx_decider *guard_decider(void)
{
	sx_decider *d = sx_decider_new();
	sx_consideration hold = {COL_SEES, SX_CURVE_STEP, 0, 0};
	sx_consideration attack = {COL_SEES, SX_CURVE_STEP, 1, 1};
	sx_consideration join_attack = {ATTACK_COLS + 2, SX_CURVE_LINEAR, 0, 256};
	sx_consideration flee = {COL_GRENADE, SX_CURVE_STEP, 1, 1};
	sx_consideration join_flight = {FLEE_COLS + 2, SX_CURVE_LINEAR, 0, 256};
	if (d == NULL)
		return NULL;
	sx_decider_add_option(d, SX_ONE / 8);
	sx_decider_add_option(d, SX_ONE / 2);
	sx_decider_add_option(d, SX_ONE / 2);
	sx_decider_add_option(d, SX_ONE);
	sx_decider_add_option(d, SX_ONE);
	sx_decider_add_consideration(d, HOLD, &hold);
	sx_decider_add_consideration(d, ATTACK, &attack);
	sx_decider_add_consideration(d, JOIN_ATTACK, &join_attack);
	sx_decider_add_consideration(d, FLEE, &flee);
	sx_decider_add_consideration(d, JOIN_FLIGHT, &join_flight);
	return d;
}

static int run_scene(uint32_t grid)
{
	sx_perception_config cfg;
	sx_sense_map map;
	sx_perception *p;
	sx_sense_profile profile;
	sx_budget budget;
	sx_contagion_config ccfg;
	sx_contagion *memory[GUARDS];
	sx_awareness aware[GUARDS][GUARDS];
	sx_attention attention[GUARDS];
	sx_decider *d;
	sx_candidate candidates[ACTS];
	int32_t point[GUARDS][3];
	uint32_t cues[GUARDS], act[GUARDS], chosen[GUARDS];
	int32_t from[3], to[3], origin[3], east[3], spot[3], west;
	uint32_t t;
	int g, o, i;

	CHECK(sx_perception_config_default(grid, CELL, &cfg) == 0);
	memset(&map, 0, sizeof map);
	map.context = &grid;
	map.sight = post_sight;
	p = sx_perception_new(&cfg, &map);
	CHECK(p != NULL);

	CHECK(sx_sense_profile_preset(SX_SENSE_PRESET_GUARD, CELL, &profile) == 0);
	profile.cues[0].bits = ATTACKING | FLEEING;
	profile.cues[0].gain = 512;
	profile.cues[0].instant = 2;
	budget.checks = 4;
	budget.keep = 4;
	budget.instant_cap = 4;

	memset(&ccfg, 0, sizeof ccfg);
	ccfg.grid = grid;
	ccfg.count = 2;
	ccfg.deeds[DEED_ATTACK].bits = ATTACKING;
	ccfg.deeds[DEED_ATTACK].window = WINDOW;
	ccfg.deeds[DEED_FLEE].bits = FLEEING;
	ccfg.deeds[DEED_FLEE].window = WINDOW;

	d = guard_decider();
	CHECK(d != NULL);
	for (i = 0; i < ACTS; i++) {
		candidates[i].option = (uint32_t)i;
		candidates[i].target = 0;
	}
	memset(aware, 0, sizeof aware);
	memset(attention, 0, sizeof attention);
	for (g = 0; g < GUARDS; g++) {
		memory[g] = sx_contagion_new(&ccfg, 4);
		CHECK(memory[g] != NULL);
		centre(grid, posts[g][0], posts[g][1], point[g]);
		cues[g] = 0;
		act[g] = HOLD;
	}
	/* One cell west, as a fine step (the same on square and axial hex). */
	centre(grid, 0, 0, origin);
	centre(grid, 1, 0, east);
	west = origin[0] - east[0];
	centre(grid, start[0], start[1], from);
	centre(grid, stop[0], stop[1], to);
	centre(grid, grenade[0], grenade[1], spot);

	printf("%s\n", grid == SX_GRID_SQUARE ? "square" : "hex");
	for (t = 0; t < END; t++) {
		int32_t intruder[3];
		intruder[0] = from[0] + west * (int32_t)t / 4;
		if (intruder[0] < to[0])
			intruder[0] = to[0];
		intruder[1] = from[1];
		intruder[2] = 0;
		if (t == GRENADE_AT)
			printf("t=%3u grenade at %d,%d\n", (unsigned)t, (int)grenade[0], (int)grenade[1]);
		for (g = 0; g < GUARDS; g++) {
			sx_target targets[GUARDS];
			sx_percept out[4 * GUARDS];
			sx_eye eye;
			onsets seen;
			int32_t row[ACTS][ROW];
			sx_choice choice;
			uint32_t id = (uint32_t)g + 1, next;
			int32_t n, k = 0, sees = 0;

			/* Everyone else, as this guard can see them now. */
			memset(targets, 0, sizeof targets);
			for (o = 0; o < GUARDS; o++) {
				if (o == g)
					continue;
				targets[k].id = (uint32_t)o + 1;
				memcpy(targets[k].point, point[o], sizeof point[o]);
				targets[k].size = 256;
				targets[k].conspicuity = act[o] == ATTACK || act[o] == JOIN_ATTACK ? 2048
					: act[o] == FLEE || act[o] == JOIN_FLIGHT ? 512 : 128;
				targets[k].cues = cues[o];
				k++;
			}
			targets[k].id = INTRUDER;
			memcpy(targets[k].point, intruder, sizeof intruder);
			targets[k].size = 256;
			targets[k].conspicuity = 256;

			memset(&eye, 0, sizeof eye);
			eye.id = id;
			memcpy(eye.point, point[g], sizeof point[g]);
			eye.facing = posts[g][2];
			eye.profile = &profile;
			eye.stats.sight_scale = 256;
			eye.stats.notice_scale = 256;
			eye.stats.hearing_scale = 256;
			n = sx_perception_look(p, t, &eye, targets, aware[g], GUARDS, &attention[g], &budget,
				out, 4 * GUARDS, NULL);
			CHECK(n >= 0);

			seen.count = 0;
			CHECK(sx_contagion_set_trace(memory[g], on_trace, &seen) == 0);
			CHECK(sx_contagion_learn(memory[g], t, out, (uint32_t)n, is_ally, NULL) == 0);
			CHECK(sx_contagion_set_trace(memory[g], NULL, NULL) == 0);
			for (i = 0; i < (int)seen.count; i++)
				printf("t=%3u guard %u sees %u start %s\n", (unsigned)t, (unsigned)id,
					(unsigned)seen.ally[i], deed_names[seen.deed[i]]);
			if (act[g] == FLEE || act[g] == JOIN_FLIGHT)
				continue; /* a fleeing guard keeps fleeing */

			/* Its own sight of the intruder: recognised and in view now. */
			for (i = 0; i < n; i++)
				if (out[i].subject == INTRUDER && out[i].stage == 2 && out[i].placed)
					sees = 1;
			memset(row, 0, sizeof row);
			row[0][COL_SEES] = sees;
			row[0][COL_GRENADE] = t == GRENADE_AT && point[g][0] == spot[0]
				&& point[g][1] == spot[1] && point[g][2] == spot[2];
			CHECK(sx_contagion_write_columns(memory[g], DEED_ATTACK, point[g], t,
				&row[0][ATTACK_COLS]) == 0);
			CHECK(sx_contagion_write_columns(memory[g], DEED_FLEE, point[g], t,
				&row[0][FLEE_COLS]) == 0);
			for (i = 1; i < ACTS; i++)
				memcpy(row[i], row[0], sizeof row[0]);
			CHECK(sx_decider_decide(d, candidates, ACTS, &row[0][0], ROW, SX_PICK_BEST, 0, 0,
				NULL, &choice) == 1);
			next = candidates[choice.candidate].option;
			chosen[g] = next;
			if (next != act[g]) {
				sx_deed_inputs in;
				CHECK(sx_contagion_inputs(memory[g], next >= FLEE ? DEED_FLEE : DEED_ATTACK,
					point[g], t, &in) == 0);
				print_act(t, id, next, &in);
			}
		}
		/* The choices take effect together, after everyone looked. */
		for (g = 0; g < GUARDS; g++) {
			if (act[g] != FLEE && act[g] != JOIN_FLIGHT)
				act[g] = chosen[g];
			cues[g] = act[g] == ATTACK || act[g] == JOIN_ATTACK ? ATTACKING
				: act[g] == FLEE || act[g] == JOIN_FLIGHT ? FLEEING : 0;
			if (cues[g] == FLEEING)
				point[g][0] += west / 4;
		}
	}
	for (g = 0; g < GUARDS; g++)
		sx_contagion_free(memory[g]);
	sx_decider_free(d);
	sx_perception_free(p);
	return 0;
}

int main(void)
{
	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "library too old\n");
		return 1;
	}
	if (run_scene(SX_GRID_SQUARE) != 0)
		return 1;
	return run_scene(SX_GRID_HEX_ODD_R);
}
