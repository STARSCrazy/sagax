//! `examples/c/ambush_rules.c` replayed through the exported `sx_*` functions.
//!
//! There is no C compiler on every machine, so this file is the local proof
//! of the C example: it follows `ambush_rules.c` line by line (the same
//! calls in the same order, the same callbacks as `extern "C"` functions
//! behind one `user` pointer) and must print the trace of the Rust example
//! `crates/sagax/examples/ambush_rules.rs` (its golden text is copied
//! from `crates/sagax/tests/ambush_rules.rs`). The CI diff of the real C
//! program against the Rust one is `examples/c/run.sh`.

use std::ffi::c_void;
use std::fmt::Write as _;
use std::ptr::{null, null_mut};

use sagax_abi::codes::*;
use sagax_decide::group::{
    IN_LOSSES, IN_OFF_PLAN, IN_TARGET_AGE, IN_TARGET_DIST, REPORT_GAME_FIRST, TAG_HIDDEN,
};

use crate::Point;
use crate::crew::*;
use crate::decide::SxConsideration;
use crate::grid::{SxGrid, sx_grid_free, sx_grid_new, sx_grid_set};
use crate::group::*;
use crate::memory::{SxFact, SxMessage};
use crate::perception::*;
use crate::rules::*;

const END: u32 = 80;
const WIDTH: i32 = 20;
const HEIGHT: i32 = 12;
const WALL_X: i32 = 10;
const WALL_TOP: i32 = 8;
const SHIELD_X: i32 = 17;
const SHIELD_TOP: i32 = 4;
const INTRUDER: u32 = 99;
const ENEMY: u32 = 1;
const SKILL: u32 = 192;
const SEED: u64 = 7;
const GUARDS: usize = 4;
const EVENTS: u32 = 16;
const STEP: u32 = 2;
const HAND_SIGNS: u32 = 1;
const KILL_ZONE: u32 = 1;
const SEEN: u32 = 1;
const REPORT_SEEN: u32 = REPORT_GAME_FIRST;
const SEEN_PENALTY: i64 = 10000;
/// `SX_ONE`.
const SX_ONE: u32 = 65535;
const CELL: i32 = 256;
const LOOKOUT: u32 = 5;
const LOOKOUT_X: i32 = 6;
const LOOKOUT_Y: i32 = 1;
const SOUTH: i32 = 64;
const WEST: i32 = 128;
const TORCH: u32 = 1;
const WATCHERS: usize = 5;
const PERCEPTS: u32 = 8;
/// `SX_GRID_SQUARE`.
const GRID_SQUARE: u32 = 0;

/// The game's pieces, behind the user pointer of the rules table.
struct AmbushRules {
    sight: *mut SxPerception,
    signs: *const SxSenseProfile,
    intruder: [i32; 3],
    seen: bool,
}

/// One guard's walk.
#[derive(Clone, Copy, Default)]
struct Walk {
    has_goal: bool,
    goal: [i32; 3],
    from: u32,
}

fn sign(v: i32) -> i32 {
    v.signum()
}

/// `centre` of the C file: the fine point at the centre of cell `c`.
unsafe fn centre(p: *mut SxPerception, c: [i32; 3]) -> [i32; 3] {
    let mut out = [0i32; 3];
    let cell = Point {
        x: c[0],
        y: c[1],
        level: c[2],
    };
    unsafe { sx_perception_cell_centre(p, cell, out.as_mut_ptr()) };
    out
}

/// `sees` of the C file: the exact sight line between two cell centres.
unsafe fn sees(p: *mut SxPerception, from: [i32; 3], to: [i32; 3]) -> bool {
    unsafe {
        let (a, b) = (centre(p, from), centre(p, to));
        let mut line = SxLine::default();
        sx_perception_line(p, a.as_ptr(), b.as_ptr(), &mut line) == 0 && line.clear != 0
    }
}

/// `make_eye` of the C file.
unsafe fn make_eye(
    p: *mut SxPerception,
    id: u32,
    at: [i32; 3],
    facing: i32,
    profile: *const SxSenseProfile,
) -> SxEye {
    SxEye {
        id,
        point: unsafe { centre(p, at) },
        facing,
        profile,
        stats: SxStats {
            sight_scale: 256,
            notice_scale: 256,
            hearing_scale: 256,
            reaction_ticks: 0,
        },
        disguise: SxDisguiseRule::default(),
    }
}

/// `sees_sign` of the C file: a guard at `receiver`, facing west, sees a
/// hand sign at `sender` (range, cone and line).
unsafe fn sees_sign(r: &AmbushRules, sender: [i32; 3], receiver: [i32; 3]) -> bool {
    unsafe {
        let eye = make_eye(r.sight, 0, receiver, WEST, r.signs);
        let at = centre(r.sight, sender);
        sx_perception_in_sight(r.sight, &eye, at.as_ptr()) == 1
    }
}

fn in_kill_zone(p: [i32; 3]) -> bool {
    p[0] >= 11 && p[0] <= 14 && p[1] >= 9 && p[1] <= 11
}

fn intruder_at(scene_b: bool, t: u32) -> [i32; 3] {
    let walked = (t / STEP) as i32;
    let x = 2 + walked;
    let px = if !scene_b {
        x.min(WIDTH - 1)
    } else if t < 24 {
        x.min(8)
    } else {
        9
    };
    [px, 10, 0]
}

unsafe extern "C" fn cb_spot_score(
    user: *mut c_void,
    ctx: *const SxSpotCtx,
    score: *mut i64,
) -> i32 {
    unsafe {
        let r = &*user.cast::<AmbushRules>();
        let ctx = &*ctx;
        let penalty: i64 = if sees(r.sight, r.intruder, ctx.spot.pos) {
            SEEN_PENALTY
        } else {
            0
        };
        // saturating add: built_in is INT64_MAX when the miss part is huge
        *score = if ctx.built_in > i64::MAX - penalty {
            i64::MAX
        } else {
            ctx.built_in + penalty
        };
        1
    }
}

unsafe extern "C" fn cb_channel_delay(
    user: *mut c_void,
    id: u32,
    ctx: *const SxChannelCtx,
    delay: *mut u32,
) -> i32 {
    unsafe {
        let r = &*user.cast::<AmbushRules>();
        let ctx = &*ctx;
        if id == HAND_SIGNS && sees_sign(r, ctx.from, ctx.to) {
            *delay = 1;
            return 1;
        }
        0
    }
}

unsafe extern "C" fn cb_trigger(
    user: *mut c_void,
    id: u32,
    _view: *const SxSquadView,
    _now: u32,
) -> i32 {
    unsafe {
        let r = &*user.cast::<AmbushRules>();
        i32::from(id == KILL_ZONE && in_kill_zone(r.intruder))
    }
}

unsafe extern "C" fn cb_report(
    user: *mut c_void,
    _view: *const SxSquadView,
    msg: *const SxMessage,
    _now: u32,
    _actions: *mut SxReportActions,
) {
    unsafe {
        let r = &mut *user.cast::<AmbushRules>();
        if (*msg).kind == REPORT_SEEN {
            r.seen = true;
        }
    }
}

unsafe extern "C" fn cb_abort(
    user: *mut c_void,
    id: u32,
    _view: *const SxSquadView,
    _ctx: *const SxRowCtx,
) -> i32 {
    unsafe {
        let r = &*user.cast::<AmbushRules>();
        i32::from(id == SEEN && r.seen)
    }
}

fn kind_name(kind: u32) -> &'static str {
    match kind {
        CREW_PLANNED => "planned",
        CREW_ACTIVATED => "activated",
        CREW_SIGNAL => "signal",
        CREW_ORDER => "order",
        CREW_START => "start",
        CREW_ARRIVED => "arrived",
        CREW_BROKEN => "broken",
        CREW_NO_PLAN => "noplan",
        _ => "?",
    }
}

fn spot(x: i32, y: i32) -> SxSpot {
    SxSpot {
        pos: [x, y, 0],
        tags: TAG_HIDDEN,
    }
}

/// `scene_crew` of the C file.
unsafe fn scene_crew(guards: &[[i32; 3]; GUARDS]) -> *mut SxCrew {
    unsafe {
        let mut config = SxCrewConfig::default();
        if sx_crew_config_default(&mut config) != 0 {
            return null_mut();
        }
        config.squad.members = GUARDS as u32;
        config.squad.metric = METRIC_CHEBYSHEV;
        let crew = sx_crew_new(&config);
        if crew.is_null() {
            return null_mut();
        }
        for (i, g) in guards.iter().enumerate() {
            let m = SxMember {
                id: g[0] as u32,
                pos: [g[1], g[2], 0],
                skill: SKILL as u8,
                state: MEMBER_ACTIVE as u8,
                ..SxMember::default()
            };
            if sx_crew_add_member(crew, &m) != i as i32 {
                return null_mut();
            }
        }
        let squad = sx_crew_squad(crew);
        for g in guards {
            if sx_squad_set_channel_custom(squad, g[0] as u32, HAND_SIGNS) != 1 {
                return null_mut();
            }
        }
        if sx_squad_set_leader(squad, 1) != 1
            || sx_squad_set_skill(squad, SKILL) != 0
            || sx_squad_set_target_kind(squad, ENEMY) != 0
        {
            return null_mut();
        }

        let ambush = sx_tactic_new(SX_ONE * 3 / 4, 128, 1);
        if ambush.is_null() {
            return null_mut();
        }
        let slot = SxSlot {
            min: 1,
            max: 255,
            place: SxPlace {
                kind: PLACE_TAGGED,
                value: TAG_HIDDEN,
                ..SxPlace::default()
            },
            gather: SxPlace {
                kind: PLACE_NONE,
                ..SxPlace::default()
            },
            kind: GOAL_ATTACK,
            start: START_ON_SIGNAL,
            dist: 4,
            gather_dist: 2,
            ..SxSlot::default()
        };
        let linear = SxConsideration {
            input: IN_TARGET_DIST as u32,
            curve: 0, // SX_CURVE_LINEAR
            min: 0,
            max: 16,
        };
        if sx_tactic_add_slot(ambush, &slot) != 0
            || sx_tactic_add_consideration(ambush, &linear) != 0
            || sx_tactic_add_abort(ambush, IN_LOSSES as u32, 2) != 0
            || sx_tactic_add_abort(ambush, IN_OFF_PLAN as u32, 2) != 0
            || sx_tactic_add_abort(ambush, IN_TARGET_AGE as u32, 255) != 0
            || sx_tactic_add_abort_custom(ambush, SEEN) != 0
            || sx_tactic_trigger_custom(ambush, KILL_ZONE) != 0
        {
            sx_tactic_free(ambush);
            return null_mut();
        }
        if sx_squad_add_tactic(squad, ambush) != 0 || sx_squad_add_rush(squad) != 1 {
            return null_mut();
        }
        crew
    }
}

/// `run_scene` of the C file: what it prints, in `out`.
unsafe fn run_scene(
    scene_b: bool,
    sight: *mut SxPerception,
    out: &mut String,
) -> Result<(), String> {
    unsafe {
        macro_rules! check {
            ($cond:expr) => {
                if !$cond {
                    return Err(format!("failed: {}", stringify!($cond)));
                }
            };
        }
        let guards: [[i32; 3]; GUARDS] = [[1, 15, 3], [2, 14, 4], [3, 16, 4], [4, 19, 2]];
        let cells: [[i32; 2]; 8] = [
            [12, 10],
            [13, 11],
            [11, 3],
            [12, 1],
            [11, 5],
            [11, 1],
            [16, 3],
            [16, 4],
        ];
        let mut spots = [spot(0, 0); 8];
        let mut table = SxSquadRules::default();
        let lookout = [LOOKOUT_X, LOOKOUT_Y, 0];
        let kinds = SxFactKinds {
            noticed_kind: ENEMY,
            seen_kind: ENEMY,
            heard_kind: ENEMY,
            disguised_kind: 0,
            has_disguised_kind: 0,
        };
        let mut watch = SxSenseProfile::default();
        let mut signs = SxSenseProfile::default();
        let mut aware = [SxAwareness::default(); WATCHERS];
        let mut attention = [SxAttention::default(); WATCHERS];
        let mut percepts = [SxPercept::default(); PERCEPTS as usize];
        let mut rules = AmbushRules {
            sight: null_mut(),
            signs: null(),
            intruder: [0; 3],
            seen: false,
        };
        let mut walks = [Walk::default(); GUARDS];
        let mut at_guard = [[0i32; 3]; GUARDS];
        let mut events = [SxCrewEvent::default(); EVENTS as usize];
        let spot_count: u32 = if scene_b { 8 } else { 6 };
        let mut sent = false;

        writeln!(out, "scene {}", if scene_b { 'B' } else { 'A' }).unwrap();
        let crew = scene_crew(&guards);
        check!(!crew.is_null());
        for i in 0..spot_count as usize {
            spots[i] = spot(cells[i][0], cells[i][1]);
        }
        for i in 0..GUARDS {
            at_guard[i][0] = guards[i][1];
            at_guard[i][1] = guards[i][2];
            at_guard[i][2] = 0;
        }

        // The eyes of the lookout and the guards: the guard preset plus "a
        // torch is noticed at once in the focus" (gain 0: only instant).
        check!(sx_sense_profile_preset(SENSE_PRESET_GUARD, CELL, &mut watch) == 0);
        watch.cues[0].bits = TORCH;
        watch.cues[0].gain = 0;
        watch.cues[0].instant = 1;
        check!(sx_sense_profile_preset(SENSE_PRESET_WATCHER, CELL, &mut signs) == 0);
        let budget = SxBudget {
            checks: 1,
            keep: 1,
            instant_cap: 1,
        };
        rules.sight = sight;
        rules.signs = &signs;
        rules.intruder = intruder_at(scene_b, 0);
        rules.seen = false;
        check!(sx_squad_rules_default(&mut table) == 0);
        table.spot_score = Some(cb_spot_score);
        table.channel_delay = Some(cb_channel_delay);
        table.trigger = Some(cb_trigger);
        table.report = Some(cb_report);
        table.abort = Some(cb_abort);
        table.user = std::ptr::from_mut(&mut rules).cast();

        for t in 0..=END {
            let mut total = 0u32;

            // 1. The guards walk.
            for i in 0..GUARDS {
                let d = t.wrapping_sub(walks[i].from);
                if walks[i].has_goal && d > 0 && d < 0x8000_0000 && d % STEP == 0 {
                    at_guard[i][0] += sign(walks[i].goal[0] - at_guard[i][0]);
                    at_guard[i][1] += sign(walks[i].goal[1] - at_guard[i][1]);
                    check!(sx_crew_set_pos(crew, guards[i][0] as u32, at_guard[i].as_ptr()) == 1);
                }
            }

            // 2. The intruder moves; the lookout and the guards look at it,
            // and what they perceive goes to the crew. The C file writes
            // the intruder through the user pointer; so does this.
            let rules_ptr = table.user.cast::<AmbushRules>();
            (*rules_ptr).intruder = intruder_at(scene_b, t);
            let at = (*rules_ptr).intruder;
            let target = SxTarget {
                id: INTRUDER,
                point: centre(sight, at),
                size: 256,
                conspicuity: 256,
                cues: TORCH,
                ..SxTarget::default()
            };
            for k in 0..WATCHERS {
                let eye = if k == 0 {
                    make_eye(sight, LOOKOUT, lookout, SOUTH, &watch)
                } else {
                    make_eye(
                        sight,
                        guards[k - 1][0] as u32,
                        at_guard[k - 1],
                        WEST,
                        &watch,
                    )
                };
                let n = sx_perception_look(
                    sight,
                    t,
                    &eye,
                    &target,
                    &mut aware[k],
                    1,
                    &mut attention[k],
                    &budget,
                    percepts.as_mut_ptr(),
                    PERCEPTS,
                    null_mut(),
                );
                check!(n >= 0);
                for pc in &percepts[..n as usize] {
                    let mut fact = SxFact::default();
                    if sx_percept_fact(pc, &kinds, &mut fact) == 1 {
                        check!(sx_crew_observe(crew, t, &fact, null_mut()) >= 0);
                    }
                }
            }

            // 3. Scene B: guard 3 is seen and tells the leader.
            if scene_b && !sent && sees(sight, at, at_guard[2]) {
                let msg = SxMessage {
                    kind: REPORT_SEEN,
                    sender: 3,
                    subject: INTRUDER,
                    ..SxMessage::default()
                };
                check!(sx_crew_report_with(crew, t, &msg, &table) == 1);
                sent = true;
            }

            // 4. The group.
            let n = sx_crew_tick_with(
                crew,
                t,
                null(),
                0,
                spots.as_ptr(),
                spot_count,
                SEED,
                &table,
                events.as_mut_ptr(),
                EVENTS,
                &mut total,
            );
            check!(n >= 0 && n as u32 == total);
            for e in &events[..n as usize] {
                if e.kind == CREW_ORDER {
                    for k in 0..GUARDS {
                        if guards[k][0] as u32 != e.member {
                            continue;
                        }
                        walks[k].has_goal = e.goal.has_pos != 0;
                        walks[k].goal[0] = e.goal.pos[0];
                        walks[k].goal[1] = e.goal.pos[1];
                        walks[k].from = t;
                    }
                }
                match e.kind {
                    CREW_PLANNED | CREW_ACTIVATED => {
                        writeln!(out, "t={:3} {} plan {}", t, kind_name(e.kind), e.plan).unwrap();
                    }
                    CREW_ORDER | CREW_START | CREW_ARRIVED | CREW_BROKEN => {
                        writeln!(out, "t={:3} {} member {}", t, kind_name(e.kind), e.member)
                            .unwrap();
                    }
                    _ => {
                        writeln!(out, "t={:3} {}", t, kind_name(e.kind)).unwrap();
                    }
                }
            }
        }
        sx_crew_free(crew);
        Ok(())
    }
}

/// `main` of the C file: the walls, the perception world, both scenes.
fn replay() -> Result<String, String> {
    let mut out = String::new();
    unsafe {
        // The walls: a grid whose blocked cells stop sight.
        let walls: *mut SxGrid = sx_grid_new(WIDTH, HEIGHT, 1);
        if walls.is_null() {
            return Err("sx_grid_new".into());
        }
        for y in 0..HEIGHT {
            for x in [WALL_X, SHIELD_X] {
                let top = if x == WALL_X { WALL_TOP } else { SHIELD_TOP };
                if y <= top && sx_grid_set(walls, Point { x, y, level: 0 }, -1) != 0 {
                    return Err("sx_grid_set".into());
                }
            }
        }
        let mut map = SxSenseMap {
            sight: None,
            sight_edge: None,
            cross_level: None,
            light: None,
            sound: None,
            sound_edge: None,
            context: null_mut(),
        };
        let mut cfg = SxPerceptionConfig::default();
        if sx_sense_map_from_grid(walls, &mut map) != 0
            || sx_perception_config_default(GRID_SQUARE, CELL, &mut cfg) != 0
        {
            return Err("sense map or config".into());
        }
        let sight = sx_perception_new(&cfg, &map);
        if sight.is_null() {
            return Err("sx_perception_new".into());
        }
        let result =
            run_scene(false, sight, &mut out).and_then(|()| run_scene(true, sight, &mut out));
        sx_perception_free(sight);
        sx_grid_free(walls);
        result?;
    }
    Ok(out)
}

/// The Rust example's trace (`crates/sagax/tests/ambush_rules.rs`, `GOLDEN`).
const GOLDEN: &str = "\
scene A
t=  0 planned plan 1
t=  5 activated plan 1
t=  5 order member 1
t=  6 order member 3
t=  6 order member 2
t= 11 arrived member 1
t= 12 arrived member 2
t= 16 arrived member 3
t= 18 signal
t= 18 order member 1
t= 19 order member 3
t= 19 order member 2
t= 19 start member 1
t= 19 start member 2
t= 19 start member 3
scene B
t=  0 planned plan 1
t=  5 activated plan 1
t=  5 order member 1
t=  6 order member 3
t=  6 order member 2
t=  6 arrived member 3
t=  7 arrived member 1
t= 12 arrived member 2
t= 25 planned plan 2
t= 30 activated plan 2
t= 30 order member 1
t= 30 start member 1
t= 31 order member 3
t= 31 start member 3
";

#[test]
fn c_ambush_scene_same_as_rust() {
    assert_eq!(replay().unwrap(), GOLDEN);
}
