//! `sx_*_set_distance` and `sx_flow_distance_*` (C API 16): the Rust
//! walking distance through C.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use crate::crew::*;
use crate::grid::{SxGrid, sx_grid_free, sx_grid_map, sx_grid_new, sx_grid_set};
use crate::group::*;
use crate::walk::*;
use crate::{ERROR_ARGUMENT, Point, SxMap, SxRules, sx_rules_default};
use sagax_abi::codes::*;
use sagax_decide::ONE;
use sagax_decide::group::{
    Crew, CrewConfig, CrewEvent, GoalKind, Member, MemberState, Place, Slot, SquadConfig, Start,
    Tactic, Trigger, Walking,
};
use sagax_decide::memory::{Distance, Fact, Metric, Pos};
use sagax_facade::walk::FlowDistance;
use sagax_nav::grid::{DenseGrid, GridRules, Preset};
use sagax_nav::topology::Topology;

const ENEMY: u32 = 1;
const SEED: u64 = 7;
/// The path-cost scene: leader 1 (no role) at (6, 0), member 2 at (4, 0)
/// behind the wall, member 3 at (6, 6) on the open side.
const MEMBERS: [(u32, i32, i32, u32); 3] = [(1, 6, 0, 0), (2, 4, 0, 1), (3, 6, 6, 1)];

/// A 10 x 10 grid with a wall at x = 5 for y 0..=8 (open at y = 9).
unsafe fn walled() -> (*mut SxGrid, SxMap) {
    unsafe {
        let grid = sx_grid_new(10, 10, 1);
        assert!(!grid.is_null());
        for y in 0..=8 {
            assert_eq!(sx_grid_set(grid, Point::new(5, y, 0), -1), 0);
        }
        let mut map = SxMap {
            step: None,
            start_step: None,
            move_cost: None,
            steps: None,
            context: null_mut(),
        };
        assert_eq!(sx_grid_map(grid, &mut map), 0);
        (grid, map)
    }
}

fn dense_walled() -> DenseGrid {
    let mut d = DenseGrid::new(10, 10);
    for y in 0..=8 {
        d.set(5, y, DenseGrid::BLOCKED);
    }
    d
}

fn octile_rules() -> SxRules {
    let mut r = SxRules::default();
    // SAFETY: a local, writable.
    unsafe { assert_eq!(sx_rules_default(0, 1, &mut r), 0) };
    r
}

fn rust_flow(slots: usize) -> FlowDistance {
    let topology = Topology::Square(GridRules::preset(Preset::Octile));
    FlowDistance::new(topology, slots, octile_rules().node_capacity as usize)
}

fn enemy() -> Fact {
    Fact {
        subject: 10,
        kind: ENEMY,
        pos: [9, 0, 0],
        ..Fact::default()
    }
}

/// The scene in Rust: one slot for one member with role 1 at the leader's
/// place.
fn rust_crew() -> Crew {
    let mut crew = Crew::new(CrewConfig {
        squad: SquadConfig {
            members: 4,
            memory: 4,
            areas: 1,
            inbox: 4,
            metric: Metric::Chebyshev,
            ..SquadConfig::default()
        },
        arrive_radius: 1,
        ..CrewConfig::default()
    })
    .unwrap();
    for (id, x, y, roles) in MEMBERS {
        let m = Member {
            id,
            pos: [x, y, 0],
            roles,
            skill: 255,
            state: MemberState::Active,
        };
        assert!(crew.add_member(m).is_some());
    }
    let sq = crew.squad_mut();
    assert!(sq.set_leader(1));
    sq.set_skill(255);
    sq.set_target_kind(ENEMY);
    sq.add_tactic(Tactic {
        slots: vec![Slot {
            roles: 1,
            min: 1,
            max: 1,
            gather: None,
            place: Place::Here,
            kind: GoalKind::Attack,
            start: Start::Now,
            want_tags: 0,
            dist: 1,
            gather_dist: 2,
        }],
        considerations: Vec::new(),
        weight: ONE / 4,
        aborts: Vec::new(),
        trigger: Trigger::None,
        min_skill: 0,
        needs_target: true,
        abort_if_target_lost: true,
    });
    crew.observe(0, enemy());
    crew
}

/// Adds the scene's tactic (`start`: `SX_START_*`) to a squad handle.
unsafe fn add_tactic(sq: *mut SxSquad, start: u32) {
    unsafe {
        let t = sx_tactic_new(ONE / 4, 0, 1);
        assert!(!t.is_null());
        let slot = SxSlot {
            roles: 1,
            min: 1,
            max: 1,
            place: SxPlace {
                kind: PLACE_HERE,
                ..SxPlace::default()
            },
            kind: GOAL_ATTACK,
            start,
            dist: 1,
            gather_dist: 2,
            ..SxSlot::default()
        };
        assert_eq!(sx_tactic_add_slot(t, &slot), 0);
        assert_eq!(sx_squad_add_tactic(sq, t), 0);
    }
}

/// The scene through `sx_crew_*`.
unsafe fn c_crew() -> *mut SxCrew {
    unsafe {
        let mut config = SxCrewConfig::default();
        assert_eq!(sx_crew_config_default(&mut config), 0);
        config.squad.members = 4;
        config.squad.memory = 4;
        config.squad.areas = 1;
        config.squad.inbox = 4;
        config.squad.metric = METRIC_CHEBYSHEV;
        config.arrive_radius = 1;
        let c = sx_crew_new(&config);
        assert!(!c.is_null());
        for (i, (id, x, y, roles)) in MEMBERS.into_iter().enumerate() {
            let m = SxMember {
                id,
                pos: [x, y, 0],
                roles,
                skill: 255,
                ..SxMember::default()
            };
            assert_eq!(sx_crew_add_member(c, &m), i as i32);
        }
        let sq = sx_crew_squad(c);
        assert_eq!(sx_squad_set_leader(sq, 1), 1);
        assert_eq!(sx_squad_set_skill(sq, 255), 0);
        assert_eq!(sx_squad_set_target_kind(sq, ENEMY), 0);
        add_tactic(sq, START_NOW);
        assert_eq!(sx_crew_observe(c, 0, &enemy(), null_mut()), 0);
        c
    }
}

/// `(tick, member)` of every order of ticks 0..=12, and member 2's
/// distance to its goal at 12.
fn rust_run(walk: Option<&mut dyn Distance>) -> (Vec<(u32, u32)>, i32) {
    let mut crew = rust_crew();
    let mut events = Vec::new();
    let mut out = Vec::new();
    let mut walk = walk;
    for t in 0..=12 {
        match walk.as_deref_mut() {
            Some(w) => crew
                .tick_with(t, &[], &[], SEED, &mut Walking::new(w), &mut events)
                .unwrap(),
            None => crew.tick(t, &[], &[], SEED, &mut events).unwrap(),
        }
        for e in &events {
            if let CrewEvent::Order { member, .. } = e {
                out.push((t, *member));
            }
        }
    }
    let inputs = match walk {
        Some(w) => crew.member_inputs_with(2, 12, &mut Walking::new(w)),
        None => crew.member_inputs(2, 12),
    };
    (out, inputs.unwrap().dist)
}

unsafe fn c_run(c: *mut SxCrew) -> (Vec<(u32, u32)>, i32) {
    unsafe {
        let mut events = [SxCrewEvent::default(); 16];
        let mut out = Vec::new();
        for t in 0..=12 {
            let ev = events.as_mut_ptr();
            let n = sx_crew_tick(c, t, null(), 0, null(), 0, SEED, ev, 16, null_mut());
            assert!(n >= 0, "{n}");
            for e in &events[..n as usize] {
                if e.kind == CREW_ORDER {
                    out.push((t, e.member));
                }
            }
        }
        let mut inputs = SxMemberInputs::default();
        assert_eq!(sx_crew_member_inputs(c, 2, 12, &mut inputs), 1);
        (out, inputs.dist)
    }
}

fn members_of(orders: &[(u32, u32)]) -> Vec<u32> {
    orders.iter().map(|o| o.1).collect()
}

#[test]
fn c_crew_walks_as_rust() {
    unsafe {
        let (grid, map) = walled();
        // Without a distance: the straight line sends member 2.
        let c = c_crew();
        let plain = c_run(c);
        assert_eq!(plain, rust_run(None));
        assert_eq!(members_of(&plain.0), [2]);
        sx_crew_free(c);

        // With the ready flow distance on the same grid: member 3.
        let fd = sx_flow_distance_new(&octile_rules(), 0);
        assert!(!fd.is_null());
        assert_eq!(sx_flow_distance_set_map(fd, &map), 0);
        let c = c_crew();
        let walk = Some(sx_flow_distance_fn as DistanceFn);
        assert_eq!(sx_crew_set_distance(c, walk, fd.cast()), 0);
        let walked = c_run(c);
        let mut dense = dense_walled();
        let mut rust_fd = rust_flow(FlowDistance::DEFAULT_SLOTS);
        let mut on = rust_fd.on(&mut dense);
        assert_eq!(walked, rust_run(Some(&mut on)));
        assert_eq!(members_of(&walked.0), [3]);
        assert_eq!(sx_flow_distance_searches(fd), rust_fd.searches() as i64);

        // Member 2 has no goal now; member 3 walks 6 to it, as the crow
        // flies too. Off again: the straight line.
        assert_eq!(walked.1, -1);
        let mut inputs = SxMemberInputs::default();
        assert_eq!(sx_crew_member_inputs(c, 3, 12, &mut inputs), 1);
        assert_eq!(inputs.dist, 6);
        assert_eq!(sx_crew_set_distance(c, None, null_mut()), 0);
        assert_eq!(sx_crew_member_inputs(c, 3, 12, &mut inputs), 1);
        assert_eq!(inputs.dist, 6);
        assert_eq!(
            sx_crew_set_distance(null_mut(), None, null_mut()),
            ERROR_ARGUMENT
        );
        sx_crew_free(c);
        sx_flow_distance_free(fd);
        sx_grid_free(grid);
    }
}

fn shut_in_rust(a: Pos, b: Pos) -> Option<u32> {
    (a[..2] != [4, 0]).then(|| {
        (a[0] - b[0])
            .unsigned_abs()
            .max((a[1] - b[1]).unsigned_abs())
    })
}

/// The game's own distance: none from (4, 0), else Chebyshev; counts its
/// calls in the context.
unsafe extern "C" fn shut_in(
    ctx: *mut c_void,
    from: *const i32,
    to: *const i32,
    out: *mut u32,
) -> i32 {
    unsafe {
        *ctx.cast::<u32>() += 1;
        let p = |q: *const i32| [*q, *q.add(1), *q.add(2)];
        match shut_in_rust(p(from), p(to)) {
            Some(d) => {
                *out = d;
                1
            }
            None => 0,
        }
    }
}

struct Plain(fn(Pos, Pos) -> Option<u32>);

impl Distance for Plain {
    fn distance(&mut self, from: Pos, to: Pos) -> Option<u32> {
        (self.0)(from, to)
    }
}

#[test]
fn c_squad_plan_walks_as_rust() {
    unsafe {
        let c = c_crew();
        let sq = sx_crew_squad(c);
        let mut calls = 0u32;
        let ctx = (&raw mut calls).cast();
        assert_eq!(sx_squad_set_distance(sq, Some(shut_in), ctx), 0);
        let mut id = 0;
        assert_eq!(sx_squad_plan(sq, 0, null(), 0, null(), 0, SEED, &mut id), 1);
        assert!(calls > 0);
        let mut plan = SxPlan::default();
        let mut orders = [SxOrder::default(); 4];
        assert_eq!(
            sx_squad_pending_plan(sq, &mut plan, orders.as_mut_ptr(), 4),
            1
        );

        let mut rust = rust_crew();
        let sq_r = rust.squad_mut();
        let rid = sq_r
            .plan_with(
                0,
                &[],
                &[],
                SEED,
                &mut Walking::new(&mut Plain(shut_in_rust)),
            )
            .unwrap()
            .unwrap();
        assert_eq!(id, rid);
        let p = sq_r.pending_plan().unwrap();
        assert_eq!(p.goals.len(), 1);
        // Member 2 cannot get there: member 3, as in Rust.
        assert_eq!((orders[0].member, p.goals[0].0), (3, 3));
        assert_eq!(
            sx_squad_set_distance(null_mut(), None, null_mut()),
            ERROR_ARGUMENT
        );
        sx_crew_free(c);
    }
}

#[test]
fn c_flow_distance_matches_rust() {
    unsafe {
        let (grid, map) = walled();
        let fd = sx_flow_distance_new(&octile_rules(), 2);
        assert!(!fd.is_null());
        let (mut out, from, to) = (7u32, [4, 0, 0], [6, 0, 0]);
        let ask = |fd: *mut SxFlowDistance, from: &[i32; 3], to: &[i32; 3], out: &mut u32| {
            sx_flow_distance_fn(fd.cast(), from.as_ptr(), to.as_ptr(), out)
        };
        // No map yet.
        assert_eq!(ask(fd, &from, &to, &mut out), ERROR_ARGUMENT);
        assert_eq!(sx_flow_distance_set_map(fd, &map), 0);
        let mut dense = dense_walled();
        let mut rust = rust_flow(2);
        for (from, to) in [
            ([4, 0, 0], [6, 0, 0]),
            ([0, 0, 0], [6, 0, 0]),
            ([9, 9, 0], [1, 1, 0]),
            ([5, 3, 0], [6, 0, 0]),
            ([2, 2, 0], [7, 7, 0]),
            ([3, 3, 0], [6, 0, 0]),
        ] {
            let want = rust.distance(&mut dense, from, to);
            let got = ask(fd, &from, &to, &mut out);
            assert_eq!(got, i32::from(want.is_some()), "{from:?} {to:?}");
            if let Some(d) = want {
                assert_eq!(out, d);
            }
        }
        assert_eq!(sx_flow_distance_searches(fd), rust.searches() as i64);
        // Invalidate: the next question builds again.
        let before = sx_flow_distance_searches(fd);
        assert_eq!(sx_flow_distance_invalidate(fd), 0);
        assert_eq!(ask(fd, &from, &to, &mut out), 1);
        assert_eq!(sx_flow_distance_searches(fd), before + 1);
        // Bad arguments.
        let (f, t) = (from.as_ptr(), to.as_ptr());
        assert_eq!(
            sx_flow_distance_fn(null_mut(), f, t, &mut out),
            ERROR_ARGUMENT
        );
        assert_eq!(
            sx_flow_distance_fn(fd.cast(), null(), t, &mut out),
            ERROR_ARGUMENT
        );
        assert_eq!(
            sx_flow_distance_fn(fd.cast(), f, null(), &mut out),
            ERROR_ARGUMENT
        );
        assert_eq!(
            sx_flow_distance_fn(fd.cast(), f, t, null_mut()),
            ERROR_ARGUMENT
        );
        assert_eq!(sx_flow_distance_set_map(null_mut(), &map), ERROR_ARGUMENT);
        assert_eq!(sx_flow_distance_invalidate(null_mut()), ERROR_ARGUMENT);
        assert_eq!(sx_flow_distance_searches(null()), i64::from(ERROR_ARGUMENT));
        assert!(sx_flow_distance_new(null(), 4).is_null());
        // Levels: lo > hi and a null handle are refused; setting them
        // forgets the fields.
        assert_eq!(sx_flow_distance_set_levels(fd, 1, 0), ERROR_ARGUMENT);
        assert_eq!(
            sx_flow_distance_set_levels(null_mut(), 0, 0),
            ERROR_ARGUMENT
        );
        let before = sx_flow_distance_searches(fd);
        assert_eq!(sx_flow_distance_set_levels(fd, 0, 1), 0);
        assert_eq!(ask(fd, &from, &to, &mut out), 1);
        assert_eq!(sx_flow_distance_searches(fd), before + 1);
        // A map without a step callback is refused; clearing it works.
        let bad = SxMap { step: None, ..map };
        assert_eq!(sx_flow_distance_set_map(fd, &bad), ERROR_ARGUMENT);
        assert_eq!(sx_flow_distance_set_map(fd, null()), 0);
        assert_eq!(ask(fd, &from, &to, &mut out), ERROR_ARGUMENT);
        sx_flow_distance_free(fd);
        sx_flow_distance_free(null_mut());
        sx_grid_free(grid);
    }
}

type Asked = Vec<([i32; 3], [i32; 3])>;

/// Records every question in the context; everything is 3 cells away.
unsafe extern "C" fn record(
    ctx: *mut c_void,
    from: *const i32,
    to: *const i32,
    out: *mut u32,
) -> i32 {
    unsafe {
        let asked = &mut *ctx.cast::<Asked>();
        let p = |q: *const i32| [*q, *q.add(1), *q.add(2)];
        asked.push((p(from), p(to)));
        *out = 3;
        1
    }
}

#[test]
fn c_hex_crew_asks_offset_coordinates() {
    unsafe {
        let mut config = SxCrewConfig::default();
        assert_eq!(sx_crew_config_default(&mut config), 0);
        config.squad.metric = METRIC_HEX;
        config.hex_grid = GRID_HEX_ODD_R;
        let c = sx_crew_new(&config);
        assert!(!c.is_null());
        for (i, (id, x, y, roles)) in [(1, 6, 0, 0), (2, 2, 3, 1)].into_iter().enumerate() {
            let m = SxMember {
                id,
                pos: [x, y, 0],
                roles,
                skill: 255,
                ..SxMember::default()
            };
            assert_eq!(sx_crew_add_member(c, &m), i as i32);
        }
        let sq = sx_crew_squad(c);
        assert_eq!(sx_squad_set_leader(sq, 1), 1);
        assert_eq!(sx_squad_set_skill(sq, 255), 0);
        assert_eq!(sx_squad_set_target_kind(sq, ENEMY), 0);
        add_tactic(sq, START_ON_SIGNAL);
        assert_eq!(sx_crew_observe(c, 0, &enemy(), null_mut()), 0);
        let mut asked = Asked::new();
        let ctx = (&raw mut asked).cast();
        assert_eq!(sx_crew_set_distance(c, Some(record), ctx), 0);
        for t in 0..=12 {
            let n = sx_crew_tick(c, t, null(), 0, null(), 0, SEED, null_mut(), 0, null_mut());
            assert!(n >= 0, "{n}");
        }
        let mut inputs = SxMemberInputs::default();
        assert_eq!(sx_crew_member_inputs(c, 2, 12, &mut inputs), 1);
        assert_eq!(inputs.dist, 3);
        // Member 2 at offset (2, 3) to its goal, the leader's offset (6, 0).
        assert!(asked.contains(&([2, 3, 0], [6, 0, 0])), "{asked:?}");
        sx_crew_free(c);
    }
}
