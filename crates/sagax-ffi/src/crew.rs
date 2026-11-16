//! `sx_crew_*`: a squad with one goal keeper per member, run as one loop per
//! tick, see `sagax.h`.

use sagax_decide::group::{
    BuiltInRules, Crew, CrewConfig, CrewEvent, CrewTrace, GoalKeeper, KeeperInputs, Squad,
    SquadRules,
};
use std::ffi::c_void;

use super::group::{
    Held, Lend, SxGoal, SxKeeper, SxKeeperTrace, SxMember, SxSpot, SxSquad, SxSquadConfig,
    SxSquadTrace, lists, plan_error, sx_squad_config_default,
};
use super::memory::{SxFact, SxMessage, pos3, put};
use super::rules::{CRules, SxSquadRules};
use super::walk::{CDistance, DistanceFn, with_walk};
use super::{ERROR_ARGUMENT, TraceSlot, create, destroy, guarded, set_trace, to_i32};
use sagax_abi::convert;

/// `sx_crew_config`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxCrewConfig {
    /// The squad's capacities and metric.
    pub squad: SxSquadConfig,
    /// How far from its first spot a member counts as arrived.
    pub arrive_radius: u32,
    /// 0: positions pass unchanged; else `SX_GRID_HEX_*` (offset in and out).
    pub hex_grid: u32,
}

/// `sx_crew_event`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxCrewEvent {
    /// `SX_CREW_*`.
    pub kind: u32,
    /// The member's id (order, start, arrived, broken), else 0.
    pub member: u32,
    /// The plan's id (planned, activated, order), else 0.
    pub plan: u32,
    /// The tick the order starts; valid if `has_start_at`.
    pub start_at: u32,
    /// The order's goal (order only).
    pub goal: SxGoal,
    /// 1 if `start_at` is set (an order that does not wait for the signal).
    pub has_start_at: u8,
}

impl SxCrewEvent {
    fn of(e: &CrewEvent) -> Self {
        let c = convert::crew_event(e);
        Self {
            kind: c.kind,
            member: c.member,
            plan: c.plan,
            start_at: c.start_at.unwrap_or(0),
            goal: c.goal.as_ref().map(SxGoal::of).unwrap_or_default(),
            has_start_at: u8::from(c.start_at.is_some()),
        }
    }
}

/// `sx_member_inputs`: the five keeper columns by name.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMemberInputs {
    /// 1 with a goal, else 0.
    pub has_goal: i32,
    /// Distance to the goal's position, or -1.
    pub dist: i32,
    /// Ticks since the goal was issued (saturated).
    pub age: i32,
    /// 1 once the signal was given.
    pub signal: i32,
    /// The member's discipline.
    pub discipline: i32,
}

impl SxMemberInputs {
    const fn of(i: KeeperInputs) -> Self {
        Self {
            has_goal: i.has_goal,
            dist: i.dist,
            age: i.age,
            signal: i.signal,
            discipline: i.discipline,
        }
    }
}

/// `sx_crew`: the crew, its event buffer and the handles it lends.
pub struct SxCrew {
    crew: Crew,
    events: Vec<CrewEvent>,
    /// Counts the calls that changed the crew; a lent keeper made before
    /// the last one is stale.
    epoch: u64,
    /// The handle `sx_crew_squad` lends (its own allocation, freed with the
    /// crew).
    squad: *mut SxSquad,
    /// The handles `sx_crew_keeper` lent, one per member id asked for (each
    /// its own allocation, freed with the crew).
    keepers: Vec<(u32, *mut SxKeeper)>,
    /// Where `sx_crew_set_trace` sends the records of the crew's calls.
    trace: TraceSlot<SxCrewTrace>,
    /// The walking distance `sx_crew_set_distance` set.
    walk: Option<CDistance>,
}

/// `sx_crew_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxCrewTrace {
    /// `SX_CREW_TRACE_*`.
    pub kind: u32,
    /// `KEEPER`: the member; `SENT`: the sender.
    pub member: u32,
    /// `SENT`: the ticks until it reaches the leader, if `has_delay`.
    pub delay: u32,
    /// `SENT`: 1 if `delay` is set (0: it never arrives, nothing was sent).
    pub has_delay: u8,
    /// `SENT`: 1 when the inbox took it.
    pub queued: u8,
    /// `SENT`: the report.
    pub msg: SxMessage,
    /// `KEEPER`: the keeper's record.
    pub keeper: SxKeeperTrace,
    /// `SQUAD`: the squad's record.
    pub squad: SxSquadTrace,
}

impl SxCrewTrace {
    /// The record of a crew trace.
    pub fn of(event: CrewTrace) -> Self {
        let c = convert::crew_trace(event);
        Self {
            kind: c.kind,
            member: c.member,
            delay: c.delay.unwrap_or_default(),
            has_delay: u8::from(c.delay.is_some()),
            queued: u8::from(c.queued),
            msg: c.msg,
            keeper: SxKeeperTrace::of_code(c.keeper),
            squad: SxSquadTrace::of_code(c.squad),
        }
    }
}

/// `sx_crew_trace_fn`.
pub type CrewTraceFn = unsafe extern "C" fn(*mut c_void, *const SxCrewTrace);

/// `sx_crew_set_trace`.
///
/// # Safety
/// `crew` is null or valid; `trace` and `context` stay valid while tracing
/// is on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_set_trace(
    crew: *mut SxCrew,
    trace: Option<CrewTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(
        unsafe { crew.as_mut() }.map(|c| &mut c.trace),
        trace,
        context,
    )
}

impl SxCrew {
    pub(crate) const fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Drop for SxCrew {
    fn drop(&mut self) {
        // SAFETY: the handles came from `Box::into_raw` for this crew only
        // and are freed here once; sx_squad_free and sx_keeper_free leave
        // lent handles alone.
        unsafe {
            destroy(self.squad);
            for &(_, k) in &self.keepers {
                destroy(k);
            }
        }
    }
}

/// The crew's squad, for a lent squad handle.
fn squad_of(c: &mut SxCrew, _: u32) -> Option<&mut Squad> {
    Some(c.crew.squad_mut())
}

/// The keeper of member `id`, for a lent keeper handle: `None` once the
/// member left the group.
fn keeper_of(c: &mut SxCrew, id: u32) -> Option<&mut GoalKeeper> {
    c.crew.squad().group().index(id)?;
    c.crew.keeper_mut(id)
}

/// The crew behind a handle for a call that changes it (its lent keepers
/// go stale), or `ERROR_ARGUMENT`.
macro_rules! crew {
    ($p:expr) => {{
        let c = get!($p);
        c.epoch = c.epoch.wrapping_add(1);
        c
    }};
}

/// `sx_crew_config_default`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_config_default(out: *mut SxCrewConfig) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    let mut squad = SxSquadConfig::default();
    // SAFETY: a local, writable.
    unsafe { sx_squad_config_default(&mut squad) };
    // SAFETY: non-null and writable (see above).
    unsafe {
        put(
            out,
            SxCrewConfig {
                squad,
                arrive_radius: 0,
                hex_grid: 0,
            },
        );
    }
    0
}

/// `sx_crew_new`: null if `config` is null, a capacity is 0, the metric or
/// grid is unknown, `role_count` is above 8, or a hex grid comes without `SX_METRIC_HEX`.
///
/// # Safety
/// `config` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_new(config: *const SxCrewConfig) -> *mut SxCrew {
    // SAFETY: null or valid (see above).
    let Some(c) = (unsafe { config.as_ref() }).copied() else {
        return std::ptr::null_mut();
    };
    let root = create(|| {
        let s = c.squad;
        let crew = Crew::new(CrewConfig {
            squad: s.config()?,
            arrive_radius: c.arrive_radius,
            hex_layout: convert::grid_layout(c.hex_grid)?,
        })
        .ok()?;
        Some(SxCrew {
            crew,
            events: Vec::new(),
            epoch: 0,
            squad: std::ptr::null_mut(),
            keepers: Vec::new(),
            trace: None,
            walk: None,
        })
    });
    if root.is_null() {
        return root;
    }
    let squad = Box::into_raw(Box::new(SxSquad {
        squad: Held::Lent(Lend {
            root,
            id: 0,
            epoch: None,
            get: squad_of,
        }),
        due: Vec::new(),
        decider: std::ptr::null_mut(),
        knowledge: std::ptr::null_mut(),
        areas: std::ptr::null_mut(),
        trace: None,
        walk: None,
    }));
    // SAFETY: `root` was just made by `create` (Box::into_raw) and nothing
    // else refers to it yet.
    unsafe { (*root).squad = squad };
    root
}

/// `sx_crew_free`: also ends every handle the crew lent.
///
/// # Safety
/// `crew` is null or from [`sx_crew_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_free(crew: *mut SxCrew) {
    // SAFETY: see above.
    unsafe { destroy(crew) }
}

/// `sx_crew_add_member`.
///
/// # Safety
/// `crew` is null or valid; `member` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_add_member(crew: *mut SxCrew, member: *const SxMember) -> i32 {
    let c = crew!(crew);
    let Some(m) = input!(member).member() else {
        return ERROR_ARGUMENT;
    };
    c.crew.add_member(m).map_or(ERROR_ARGUMENT, to_i32)
}

/// `sx_crew_remove_member`.
///
/// # Safety
/// `crew` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_remove_member(crew: *mut SxCrew, id: u32) -> i32 {
    i32::from(crew!(crew).crew.remove_member(id))
}

/// `sx_crew_suspend`.
///
/// # Safety
/// `crew` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_suspend(crew: *mut SxCrew, id: u32) -> i32 {
    i32::from(crew!(crew).crew.suspend(id))
}

/// `sx_crew_set_gone`.
///
/// # Safety
/// `crew` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_set_gone(crew: *mut SxCrew, id: u32) -> i32 {
    i32::from(crew!(crew).crew.set_gone(id))
}

/// `sx_crew_reinstate`.
///
/// # Safety
/// `crew` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_reinstate(crew: *mut SxCrew, id: u32, now: u32) -> i32 {
    // SAFETY: see above; no rules.
    unsafe { sx_crew_reinstate_with(crew, id, now, std::ptr::null()) }
}

/// `sx_crew_reinstate_with`: the goal goes over a custom channel too.
///
/// # Safety
/// `crew` is null or valid; `rules` is null or valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_reinstate_with(
    crew: *mut SxCrew,
    id: u32,
    now: u32,
    rules: *const SxSquadRules,
) -> i32 {
    let c = crew!(crew);
    // SAFETY: null or valid (see above).
    let mut rules = unsafe { CRules::of(rules) };
    let trace = c.trace;
    let crew = &mut c.crew;
    guarded(|| {
        // Without rules a squad that needs them does not activate a due plan.
        let rules = rules.as_mut().map(|r| r as &mut dyn SquadRules);
        let known = traced!(trace, SxCrewTrace::of, |sink| crew
            .reinstate_traced(id, now, rules, sink));
        i32::from(known)
    })
}

/// `sx_crew_set_pos`: `pos` is 3 values.
///
/// # Safety
/// `crew` is null or valid; `pos` is null or 3 readable values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_set_pos(crew: *mut SxCrew, id: u32, pos: *const i32) -> i32 {
    let c = crew!(crew);
    if pos.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: 3 readable values (see above).
    let pos = unsafe { pos3(pos) };
    i32::from(c.crew.set_pos(id, pos))
}

/// `sx_crew_observe`.
///
/// # Safety
/// `crew` is null or valid; `fact` is null or valid; `evicted` is null or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_observe(
    crew: *mut SxCrew,
    now: u32,
    fact: *const SxFact,
    evicted: *mut SxFact,
) -> i32 {
    let c = crew!(crew);
    let fact = *input!(fact);
    match traced!(c.trace, SxCrewTrace::of, |sink| c
        .crew
        .observe_traced(now, fact, sink))
    {
        Some(gone) => {
            // SAFETY: null or writable (see above).
            unsafe { put(evicted, gone) };
            1
        }
        None => 0,
    }
}

/// `sx_crew_report_danger`: as `sx_squad_report_danger`.
///
/// # Safety
/// `crew` is null or valid; `centre` is null or 3 readable values;
/// `shooter` is null or 3 readable values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_report_danger(
    crew: *mut SxCrew,
    sender: u32,
    centre: *const i32,
    shooter: *const i32,
    arrive_at: u32,
) -> i32 {
    let c = crew!(crew);
    if centre.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: 3 readable values (see above).
    let centre = unsafe { pos3(centre) };
    let shooter = if shooter.is_null() {
        None
    } else {
        // SAFETY: 3 readable values (see above).
        Some(unsafe { pos3(shooter) })
    };
    i32::from(c.crew.report_danger(sender, centre, shooter, arrive_at))
}

/// `sx_crew_tick`: the first `cap` events to `events`, all of them counted
/// in `total`.
///
/// # Safety
/// `crew` is null or valid; `extra` and `spots` are null or hold their
/// counts; `events` is null or has room for `cap`; `total` is null or
/// writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_crew_tick(
    crew: *mut SxCrew,
    now: u32,
    extra: *const i32,
    extra_count: u32,
    spots: *const SxSpot,
    spot_count: u32,
    seed: u64,
    events: *mut SxCrewEvent,
    cap: u32,
    total: *mut u32,
) -> i32 {
    // SAFETY: see above; no rules.
    unsafe {
        sx_crew_tick_with(
            crew,
            now,
            extra,
            extra_count,
            spots,
            spot_count,
            seed,
            std::ptr::null(),
            events,
            cap,
            total,
        )
    }
}

/// `sx_crew_tick_with`: as `sx_crew_tick`, the slots going to `rules`
/// (null: the built-in ones).
///
/// # Safety
/// As [`sx_crew_tick`]; `rules` is null or valid for the call.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_crew_tick_with(
    crew: *mut SxCrew,
    now: u32,
    extra: *const i32,
    extra_count: u32,
    spots: *const SxSpot,
    spot_count: u32,
    seed: u64,
    rules: *const SxSquadRules,
    events: *mut SxCrewEvent,
    cap: u32,
    total: *mut u32,
) -> i32 {
    let c = crew!(crew);
    if events.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let Some((extra, spots)) = (unsafe { lists(extra, extra_count, spots, spot_count) }) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: null or valid (see above).
    let mut rules = unsafe { CRules::of(rules) };
    guarded(|| {
        let (crew, out, walk) = (&mut c.crew, &mut c.events, c.walk);
        let result = match rules.as_mut() {
            Some(r) => with_walk(walk, r, |r| {
                traced!(c.trace, SxCrewTrace::of, |sink| crew
                    .tick_traced(now, extra, spots, seed, r, out, sink))
            }),
            // Refused: a squad that needs rules.
            None if crew.squad().needs_rules() => crew.tick(now, extra, spots, seed, out),
            None => with_walk(walk, &mut BuiltInRules, |r| {
                traced!(c.trace, SxCrewTrace::of, |sink| crew
                    .tick_traced(now, extra, spots, seed, r, out, sink))
            }),
        };
        // SAFETY: null or writable (see above).
        unsafe { put(total, u32::try_from(c.events.len()).unwrap_or(u32::MAX)) };
        if let Err(e) = result {
            // SAFETY: null details.
            return unsafe { plan_error(e, std::ptr::null_mut(), std::ptr::null_mut()) };
        }
        let n = c.events.len().min(cap as usize);
        for (i, e) in c.events[..n].iter().enumerate() {
            // SAFETY: within `cap` (see above).
            unsafe { events.add(i).write(SxCrewEvent::of(e)) };
        }
        to_i32(n)
    })
}

/// `sx_crew_report`: a report of the game from member `msg.sender` to the
/// leader, over the member's channel. 1, or 0 when the inbox refused it.
///
/// # Safety
/// `crew` is null or valid; `msg` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_report(crew: *mut SxCrew, now: u32, msg: *const SxMessage) -> i32 {
    // SAFETY: see above; no rules.
    unsafe { sx_crew_report_with(crew, now, msg, std::ptr::null()) }
}

/// `sx_crew_report_with`: a custom channel times the report through
/// `rules`.
///
/// # Safety
/// As [`sx_crew_report`]; `rules` is null or valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_report_with(
    crew: *mut SxCrew,
    now: u32,
    msg: *const SxMessage,
    rules: *const SxSquadRules,
) -> i32 {
    let c = crew!(crew);
    let msg = *input!(msg);
    // SAFETY: null or valid (see above).
    let mut rules = unsafe { CRules::of(rules) };
    guarded(|| {
        let crew = &mut c.crew;
        let r: &mut dyn SquadRules = match rules.as_mut() {
            Some(r) => r,
            None => &mut BuiltInRules,
        };
        let stored = traced!(c.trace, SxCrewTrace::of, |sink| crew
            .report_traced(now, msg, r, sink));
        i32::from(stored)
    })
}

/// `sx_crew_member_inputs`: 1, or 0 for an unknown id (`SX_ERROR_INTERNAL`
/// if the walking distance panicked).
///
/// # Safety
/// `crew` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_member_inputs(
    crew: *mut SxCrew,
    id: u32,
    now: u32,
    out: *mut SxMemberInputs,
) -> i32 {
    let c = get!(crew);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        let crew = &c.crew;
        let inputs = with_walk(c.walk, &mut BuiltInRules, |r| {
            crew.member_inputs_with(id, now, r)
        });
        match inputs {
            Some(i) => {
                // SAFETY: non-null and writable (see above).
                unsafe { put(out, SxMemberInputs::of(i)) };
                1
            }
            None => 0,
        }
    })
}

/// `sx_crew_set_distance`: the walking distance the crew's ticks and
/// `sx_crew_member_inputs` ask, with offset coordinates on a hex grid
/// (null `distance`: the metric's straight line).
///
/// # Safety
/// `crew` is null or valid; `distance` and `context` stay valid while set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_set_distance(
    crew: *mut SxCrew,
    distance: Option<DistanceFn>,
    context: *mut c_void,
) -> i32 {
    get!(crew).walk = CDistance::of(distance, context);
    0
}

/// `sx_crew_member_chose`: 1, or 0 for an unknown id.
///
/// # Safety
/// `crew` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_member_chose(
    crew: *mut SxCrew,
    id: u32,
    now: u32,
    goal_won: u32,
) -> i32 {
    i32::from(crew!(crew).crew.member_chose(id, now, goal_won != 0))
}

/// `sx_crew_goal`: 1, or 0 for an unknown id or no goal.
///
/// # Safety
/// `crew` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_goal(crew: *mut SxCrew, id: u32, out: *mut SxGoal) -> i32 {
    let c = get!(crew);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match c.crew.goal(id) {
        Some(g) => {
            // SAFETY: non-null and writable (see above).
            unsafe { put(out, SxGoal::of(&g)) };
            1
        }
        None => 0,
    }
}

/// `sx_crew_squad`: the crew's own squad, lent (valid until
/// [`sx_crew_free`]; `sx_squad_free` does nothing to it). Null for a null
/// crew.
///
/// # Safety
/// `crew` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_squad(crew: *mut SxCrew) -> *mut SxSquad {
    // SAFETY: null or valid (see above).
    unsafe { crew.as_ref() }.map_or(std::ptr::null_mut(), |c| c.squad)
}

/// `sx_crew_keeper`: the keeper of member `id`, lent until the next call
/// that changes the crew or until the member leaves the group (then every
/// `sx_keeper_*` call on it returns `SX_ERROR_ARGUMENT`); `sx_keeper_free`
/// does nothing to it. One handle per member id, freed with the crew. Null
/// for a null crew or an unknown id.
///
/// # Safety
/// `crew` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_keeper(crew: *mut SxCrew, id: u32) -> *mut SxKeeper {
    // SAFETY: null or valid (see above).
    let Some(c) = (unsafe { crew.as_mut() }) else {
        return std::ptr::null_mut();
    };
    if keeper_of(c, id).is_none() {
        return std::ptr::null_mut();
    }
    let lend = Lend {
        root: crew,
        id,
        epoch: Some(c.epoch),
        get: keeper_of,
    };
    if let Some(&(_, k)) = c.keepers.iter().find(|(m, _)| *m == id) {
        // SAFETY: made below for this crew, freed only with it; its own
        // allocation, so writing it does not touch the crew.
        unsafe { (*k).0 = Held::Lent(lend) };
        return k;
    }
    let k = Box::into_raw(Box::new(SxKeeper(Held::Lent(lend), None)));
    c.keepers.push((id, k));
    k
}
