//! `sx_crew_*`: a squad with one goal keeper per member, run as one loop per
//! tick, see `sagax.h`.

use sagax_decide::group::{
    Crew, CrewConfig, CrewEvent, GoalKeeper, KeeperInputs, Squad, SquadConfig,
};

use super::group::{
    Held, Lend, SxGoal, SxKeeper, SxMember, SxSpot, SxSquad, SxSquadConfig, lists, plan_error,
    sx_squad_config_default,
};
use super::memory::{SxFact, metric_of, pos3, put};
use super::{ERROR_ARGUMENT, create, destroy, guarded, hex_layout, to_i32};

// Event kinds (`SX_CREW_*` in `sagax.h`).
pub(crate) const CREW_PLANNED: u32 = 1;
pub(crate) const CREW_ACTIVATED: u32 = 2;
pub(crate) const CREW_SIGNAL: u32 = 3;
pub(crate) const CREW_ORDER: u32 = 4;
pub(crate) const CREW_START: u32 = 5;
pub(crate) const CREW_ARRIVED: u32 = 6;
pub(crate) const CREW_BROKEN: u32 = 7;
pub(crate) const CREW_NO_PLAN: u32 = 8;

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
        let plain = |kind, member, plan| Self {
            kind,
            member,
            plan,
            ..Self::default()
        };
        match *e {
            CrewEvent::Planned(plan) => plain(CREW_PLANNED, 0, plan),
            CrewEvent::Activated(plan) => plain(CREW_ACTIVATED, 0, plan),
            CrewEvent::Signal => plain(CREW_SIGNAL, 0, 0),
            CrewEvent::Order {
                member,
                goal,
                start_at,
            } => Self {
                kind: CREW_ORDER,
                member,
                plan: goal.plan,
                start_at: start_at.unwrap_or(0),
                goal: SxGoal::of(&goal),
                has_start_at: u8::from(start_at.is_some()),
            },
            CrewEvent::Start { member } => plain(CREW_START, member, 0),
            CrewEvent::Arrived { member } => plain(CREW_ARRIVED, member, 0),
            CrewEvent::Broken { member } => plain(CREW_BROKEN, member, 0),
            CrewEvent::NoPlan => plain(CREW_NO_PLAN, 0, 0),
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
/// grid is unknown, or a hex grid comes without `SX_METRIC_HEX`.
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
        let layout = match c.hex_grid {
            0 => None,
            grid => Some(hex_layout(grid)?),
        };
        let crew = Crew::new(CrewConfig {
            squad: SquadConfig {
                members: s.members as usize,
                memory: s.memory as usize,
                areas: s.areas as usize,
                inbox: s.inbox as usize,
                dangers: s.dangers as usize,
                metric: metric_of(s.metric)?,
            },
            arrive_radius: c.arrive_radius,
            hex_layout: layout,
        })
        .ok()?;
        Some(SxCrew {
            crew,
            events: Vec::new(),
            epoch: 0,
            squad: std::ptr::null_mut(),
            keepers: Vec::new(),
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
    i32::from(crew!(crew).crew.reinstate(id, now))
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
    match c.crew.observe(now, *input!(fact)) {
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
    let c = crew!(crew);
    if events.is_null() && cap > 0 {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let Some((extra, spots)) = (unsafe { lists(extra, extra_count, spots, spot_count) }) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| {
        let result = c.crew.tick(now, extra, spots, seed, &mut c.events);
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

/// `sx_crew_member_inputs`: 1, or 0 for an unknown id.
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
    match c.crew.member_inputs(id, now) {
        Some(i) => {
            // SAFETY: non-null and writable (see above).
            unsafe { put(out, SxMemberInputs::of(i)) };
            1
        }
        None => 0,
    }
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
    let k = Box::into_raw(Box::new(SxKeeper(Held::Lent(lend))));
    c.keepers.push((id, k));
    k
}
