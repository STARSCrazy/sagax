/* Sagax from C: one guard watches a street: smoke, a crowd and a shot.
 *
 * A square world, 256 fine units per cell, 30 ticks. The guard (id 1, the
 * guard preset with check interval 3 plus a cue rule "a drawn weapon is
 * recognised at once in the focus", reaction 2 ticks) stands at cell (2, 6)
 * and looks around east with sx_sweep_heading. The game's sense map has a
 * smoke cell at (6, 6), a short wall at x = 8 (y 8 to 10, opaque and damping
 * sound) and dark ground from x = 14 on.
 *
 *  - A walker (100, running: conspicuity 512) goes south along x = 10 from
 *    cell (10, 3), a quarter cell per tick, and passes behind the smoke (its
 *    clarity drops).
 *  - A crowd of six (200 to 205, standing) is at x = 13, 14; sx_count_crowds
 *    gives each its crowd. The guard checks at most 3 targets per tick (one
 *    kept), so the crowd stays unnoticed. From tick 20, 203 draws a weapon:
 *    an instant cue, checked outside the budget.
 *  - At tick 12 a shot (stimulus 1 from 300, channel 0, a straight line with
 *    the map's damping, fuzzed) is heard without an awareness.
 *
 * One line per percept:
 * <tick> <observer> <subject> <sense> <event> <stage> <x>,<y> <clarity>
 * then per tick "<tick> trace" and the kind of every trace record. The
 * formats are documented in crates/sagax/examples/perception_scene.rs, the
 * same scene in Rust; both print the same lines. Build and run:
 * sh examples/c/run.sh */

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

#define CELL      256
#define END       30u
#define WEAPON    1u
#define WALKER    100u
#define DRAWS     203u
#define DRAWS_AT  20u
#define SHOT_AT   12u
#define TARGETS   7
#define MAX_TRACE 256

/* The short wall at x = 8, y 8 to 10. */
static int wall(int32_t x, int32_t y)
{
	return x == 8 && y >= 8 && y <= 10;
}

static int32_t street_sight(void *ctx, int32_t x, int32_t y, int32_t level, sx_medium *out)
{
	(void)ctx;
	(void)level;
	out->opaque = (uint8_t)wall(x, y);
	if (x == 6 && y == 6)
		out->density = 384;
	return 0;
}

static int32_t street_light(void *ctx, int32_t x, int32_t y, int32_t level)
{
	(void)ctx;
	(void)y;
	(void)level;
	return x >= 14 ? 40 : 255;
}

static int32_t street_sound(void *ctx, int32_t x, int32_t y, int32_t level)
{
	(void)ctx;
	(void)level;
	return wall(x, y) ? 512 : 0;
}

/* The trace kinds of one tick. */
typedef struct trace_log {
	uint32_t kinds[MAX_TRACE];
	size_t count;
} trace_log;

static void on_trace(void *ctx, const sx_perception_trace *record)
{
	trace_log *log = (trace_log *)ctx;
	if (log->count < MAX_TRACE)
		log->kinds[log->count++] = record->kind;
}

static const char *sense_name(uint32_t sense)
{
	switch (sense) {
	case SX_SENSE_SIGHT: return "sight";
	case SX_SENSE_THERMAL: return "thermal";
	case SX_SENSE_SCANNER: return "scanner";
	default: return "hearing";
	}
}

static const char *event_name(uint32_t event)
{
	static const char *names[] = {
		"noticed", "recognized", "seen", "cues", "lost", "forgotten", "seen_through", "heard",
	};
	return event < 8 ? names[event] : "?";
}

static const char *trace_name(uint32_t kind)
{
	static const char *names[] = {
		"?", "picked", "skipped", "line", "gain", "instant", "stage", "doubt", "heard", "unheard",
	};
	return kind < 10 ? names[kind] : "?";
}

static void print_percepts(uint32_t t, const sx_percept *p, int32_t n)
{
	int32_t i;
	for (i = 0; i < n; i++)
		printf("%u %u %u %s %s %u %d,%d %u\n", (unsigned)t, (unsigned)p[i].observer,
			(unsigned)p[i].subject, sense_name(p[i].sense), event_name(p[i].event),
			(unsigned)p[i].stage, (int)p[i].cell[0], (int)p[i].cell[1], (unsigned)p[i].clarity);
}

static int centre(sx_perception *p, int32_t x, int32_t y, int32_t *out)
{
	sx_point cell;
	cell.x = x;
	cell.y = y;
	cell.level = 0;
	return sx_perception_cell_centre(p, cell, out);
}

int main(void)
{
	static const int32_t crowd_at[6][2] = {{13, 3}, {13, 4}, {14, 3}, {14, 4}, {13, 5}, {14, 5}};
	sx_perception_config cfg;
	sx_sense_map map;
	sx_perception *p;
	sx_sense_profile profile;
	sx_eye eye;
	sx_target targets[TARGETS];
	sx_awareness aware[TARGETS];
	sx_attention attention;
	sx_budget budget;
	uint32_t groups[TARGETS];
	uint8_t crowds[TARGETS];
	sx_percept out[4 * 4 + 3];
	trace_log log;
	uint32_t t;
	size_t i;

	if (sx_api_version() < SX_API_VERSION) {
		fprintf(stderr, "library too old\n");
		return 1;
	}

	CHECK(sx_perception_config_default(SX_GRID_SQUARE, CELL, &cfg) == 0);
	cfg.channels[0].spread = SX_SPREAD_STRAIGHT;
	cfg.channels[0].fuzz = 64;
	cfg.channels[0].speed = 4 * CELL;
	memset(&map, 0, sizeof map);
	map.sight = street_sight;
	map.light = street_light;
	map.sound = street_sound;
	p = sx_perception_new(&cfg, &map);
	CHECK(p != NULL);
	CHECK(sx_perception_set_trace(p, on_trace, &log) == 0);

	CHECK(sx_sense_profile_preset(SX_SENSE_PRESET_GUARD, CELL, &profile) == 0);
	profile.check_interval = 3;
	profile.cues[0].bits = WEAPON;
	profile.cues[0].gain = 512;
	profile.cues[0].instant = 2;

	memset(&eye, 0, sizeof eye);
	eye.id = 1;
	CHECK(centre(p, 2, 6, eye.point) == 0);
	eye.profile = &profile;
	eye.stats.sight_scale = 256;
	eye.stats.notice_scale = 256;
	eye.stats.hearing_scale = 256;
	eye.stats.reaction_ticks = 2;

	memset(targets, 0, sizeof targets);
	memset(aware, 0, sizeof aware);
	memset(&attention, 0, sizeof attention);
	targets[0].id = WALKER;
	targets[0].size = 256;
	targets[0].conspicuity = 512;
	groups[0] = 0;
	for (i = 0; i < 6; i++) {
		targets[1 + i].id = 200 + (uint32_t)i;
		CHECK(centre(p, crowd_at[i][0], crowd_at[i][1], targets[1 + i].point) == 0);
		targets[1 + i].size = 256;
		targets[1 + i].conspicuity = 128;
		groups[1 + i] = 1;
	}
	CHECK(sx_count_crowds(targets, TARGETS, 2 * CELL, groups, crowds) == 0);
	for (i = 0; i < TARGETS; i++)
		targets[i].crowd = crowds[i];
	budget.checks = 3;
	budget.keep = 1;
	budget.instant_cap = 1;

	for (t = 0; t < END; t++) {
		int32_t n;
		targets[0].point[0] = 10 * CELL + 128;
		targets[0].point[1] = 3 * CELL + (int32_t)t * CELL / 4;
		targets[0].point[2] = 0;
		if (t >= DRAWS_AT)
			targets[1 + (DRAWS - 200)].cues = WEAPON;
		eye.facing = sx_sweep_heading(0, t % 32, 4, NULL, 0);
		log.count = 0;

		n = sx_perception_look(p, t, &eye, targets, aware, TARGETS, &attention, &budget, out,
			4 * (3 + 1), NULL);
		CHECK(n >= 0);
		print_percepts(t, out, n);
		if (t == SHOT_AT) {
			sx_stimulus shot;
			memset(&shot, 0, sizeof shot);
			shot.id = 1;
			shot.source = 300;
			shot.kind = 1;
			CHECK(centre(p, 16, 9, shot.point) == 0);
			shot.loudness = 16 * CELL;
			shot.channel = 0;
			shot.disguise = 0;
			shot.at = t;
			n = sx_perception_hear(p, t, &eye, &shot, 0, 0, NULL, out, 3, NULL, 7);
			CHECK(n >= 0);
			print_percepts(t, out, n);
		}
		printf("%u trace", (unsigned)t);
		for (i = 0; i < log.count; i++)
			printf(" %s", trace_name(log.kinds[i]));
		printf("\n");
	}
	sx_perception_free(p);
	return 0;
}
