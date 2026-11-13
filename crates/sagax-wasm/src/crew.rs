//! `sx_crew_*`: a squad with one goal keeper per member, run as one loop per
//! tick (see `sx_crew_*` in `sagax.h`). Records go in through the scratch
//! buffer and come out of the crew's out buffer, as for the squad.

use sagax_decide::group::{Crew, CrewConfig, CrewEvent, GoalKeeper, Squad, SquadConfig};

use super::{
    ERROR_ARGUMENT, FACT_SIZE, GOAL_SIZE, Held, Lend, MEMBER_SIZE, WasmKeeper, WasmSquad,
    boxed_squad, drop_squad, fact_of, fact_record, goal_record, hex_layout_of, member_of,
    metric_of, plan_error_to, plan_input, scratch_copy, to_i32,
};

/// Numbers per event record: kind (`SX_CREW_*`), member, plan, start at,
/// has start at, then a goal record (for an order; else zeros).
const EVENT_SIZE: usize = 5 + GOAL_SIZE;

/// A crew, its events and out buffer, and the handles it lends.
pub struct WasmCrew {
    crew: Crew,
    events: Vec<CrewEvent>,
    out: Vec<i32>,
    /// Counts the calls that changed the crew; a lent keeper made before
    /// the last one is stale.
    epoch: u64,
    /// The squad handle [`sx_crew_squad`] lends, with its own knowledge and
    /// areas handles (its own allocation, freed with the crew).
    squad: *mut WasmSquad,
    /// The keeper handles [`sx_crew_keeper`] lent, one per member id asked
    /// for (each its own allocation, freed with the crew).
    keepers: Vec<(u32, *mut WasmKeeper)>,
}

impl WasmCrew {
    pub(super) const fn epoch(&self) -> u64 {
        self.epoch
    }
}

impl Drop for WasmCrew {
    fn drop(&mut self) {
        // SAFETY: the handles were made for this crew only and are freed
        // here once; sx_squad_free and sx_keeper_free leave lent ones alone.
        unsafe {
            drop_squad(self.squad);
            for &(_, k) in &self.keepers {
                drop(Box::from_raw(k));
            }
        }
    }
}

/// The crew's squad, for the lent squad handle.
fn squad_of(c: &mut WasmCrew, _: u32) -> Option<&mut Squad> {
    Some(c.crew.squad_mut())
}

/// The keeper of member `id`, for a lent keeper handle: `None` once the
/// member left the group.
fn keeper_of(c: &mut WasmCrew, id: u32) -> Option<&mut GoalKeeper> {
    c.crew.squad().group().index(id)?;
    c.crew.keeper_mut(id)
}

/// The crew behind a handle for a call that changes it (its lent keepers
/// go stale), or -1.
macro_rules! crew {
    ($c:expr) => {{
        // SAFETY: JS passes a handle from sx_crew_new (wrapper guarantees it).
        let Some(c) = (unsafe { $c.as_mut() }) else {
            return ERROR_ARGUMENT;
        };
        c.epoch = c.epoch.wrapping_add(1);
        c
    }};
}

/// The crew behind a handle for a call that only reads it, or -1.
macro_rules! crew_ref {
    ($c:expr) => {{
        // SAFETY: as in `crew!`.
        let Some(c) = (unsafe { $c.as_mut() }) else {
            return ERROR_ARGUMENT;
        };
        c
    }};
}

/// A crew (see `sx_crew_new` in `sagax.h`); `hex` 0 for none, else the
/// layout 1 .. 4 (`SX_GRID_HEX_*`). Null if a capacity is 0, the metric or
/// layout is unknown, or a layout comes without the hex metric.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn sx_crew_new(
    members: u32,
    memory: u32,
    areas: u32,
    inbox: u32,
    dangers: u32,
    metric: i32,
    arrive_radius: u32,
    hex: u32,
) -> *mut WasmCrew {
    let Some(metric) = metric_of(metric) else {
        return std::ptr::null_mut();
    };
    let hex_layout = match hex {
        0 => None,
        layout => match hex_layout_of(layout) {
            Some(l) => Some(l),
            None => return std::ptr::null_mut(),
        },
    };
    let Ok(crew) = Crew::new(CrewConfig {
        squad: SquadConfig {
            members: members as usize,
            memory: memory as usize,
            areas: areas as usize,
            inbox: inbox as usize,
            dangers: dangers as usize,
            metric,
        },
        arrive_radius,
        hex_layout,
    }) else {
        return std::ptr::null_mut();
    };
    let root = Box::into_raw(Box::new(WasmCrew {
        crew,
        events: Vec::new(),
        out: Vec::new(),
        epoch: 0,
        squad: std::ptr::null_mut(),
        keepers: Vec::new(),
    }));
    let squad = boxed_squad(Held::Crew(Lend {
        root,
        id: 0,
        epoch: None,
        get: squad_of,
    }));
    // SAFETY: `root` was just made and nothing else refers to it yet.
    unsafe { (*root).squad = squad };
    root
}

/// Frees a crew and every handle it lent.
///
/// # Safety
/// `c` is null or from [`sx_crew_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_free(c: *mut WasmCrew) {
    if !c.is_null() {
        // SAFETY: see above.
        drop(unsafe { Box::from_raw(c) });
    }
}

/// The out buffer of the last call that wrote records.
///
/// # Safety
/// `c` from [`sx_crew_new`]. Valid until memory grows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_out(c: *mut WasmCrew) -> *const i32 {
    // SAFETY: see above.
    unsafe { c.as_ref() }.map_or(std::ptr::null(), |w| w.out.as_ptr())
}

/// Adds the member record (7 numbers) in the scratch buffer at the end.
/// Returns its index; -2 if the group is full or the id is there; -1 for a
/// bad state or skill.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_add_member(c: *mut WasmCrew) -> i32 {
    let w = crew!(c);
    let Some(m) = scratch_copy(MEMBER_SIZE).as_deref().and_then(member_of) else {
        return ERROR_ARGUMENT;
    };
    w.crew.add_member(m).map_or(-2, to_i32)
}

/// Removes a member and its keeper; 1, or 0 if unknown.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_remove_member(c: *mut WasmCrew, id: u32) -> i32 {
    i32::from(crew!(c).crew.remove_member(id))
}

/// Moves a member; 1, or 0 if unknown.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_set_pos(c: *mut WasmCrew, id: u32, x: i32, y: i32, z: i32) -> i32 {
    i32::from(crew!(c).crew.set_pos(id, [x, y, z]))
}

/// Takes a member out of play, its keeper paused; 1, or 0 if unknown.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_suspend(c: *mut WasmCrew, id: u32) -> i32 {
    i32::from(crew!(c).crew.suspend(id))
}

/// Puts a member back into play at `now`; 1, or 0 if unknown.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_reinstate(c: *mut WasmCrew, id: u32, now: u32) -> i32 {
    i32::from(crew!(c).crew.reinstate(id, now))
}

/// Marks a member gone; 1, or 0 if unknown.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_set_gone(c: *mut WasmCrew, id: u32) -> i32 {
    i32::from(crew!(c).crew.set_gone(id))
}

/// The leader's own sighting: the fact record in the scratch buffer.
/// Returns 1 with the evicted fact in the out buffer, or 0.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_observe(c: *mut WasmCrew, now: u32) -> i32 {
    let w = crew!(c);
    let Some(r) = scratch_copy(FACT_SIZE) else {
        return ERROR_ARGUMENT;
    };
    let r: [u32; FACT_SIZE] = std::array::from_fn(|i| r[i] as u32);
    w.out.clear();
    match w.crew.observe(now, fact_of(&r)) {
        Some(gone) => {
            w.out.extend(fact_record(&gone).map(|v| v as i32));
            1
        }
        None => 0,
    }
}

/// A danger report on its way to the leader (as `sx_squad_report_danger`);
/// 1, or 0 when the danger queue is full.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_crew_report_danger(
    c: *mut WasmCrew,
    sender: u32,
    cx: i32,
    cy: i32,
    cz: i32,
    has_shooter: u32,
    sx: i32,
    sy: i32,
    sz: i32,
    arrive_at: u32,
) -> i32 {
    let shooter = (has_shooter != 0).then_some([sx, sy, sz]);
    let w = crew!(c);
    i32::from(
        w.crew
            .report_danger(sender, [cx, cy, cz], shooter, arrive_at),
    )
}

/// Runs the group at `now` with the extra numbers and spots in the scratch
/// buffer (as for `sx_squad_check`); the seed comes in two halves. Returns
/// the number of events, with one record each (`EVENT_SIZE` numbers) in the
/// out buffer; `ERROR_PLAN` (details in the out buffer) or -1.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_tick(
    c: *mut WasmCrew,
    now: u32,
    extra_count: u32,
    spot_count: u32,
    seed_low: u32,
    seed_high: u32,
) -> i32 {
    let w = crew!(c);
    let Some((extra, spots)) = plan_input(extra_count, spot_count) else {
        return ERROR_ARGUMENT;
    };
    let seed = u64::from(seed_low) | (u64::from(seed_high) << 32);
    if let Err(e) = w.crew.tick(now, &extra, &spots, seed, &mut w.events) {
        return plan_error_to(&mut w.out, e);
    }
    w.out.clear();
    for e in &w.events {
        let mut r = [0i32; EVENT_SIZE];
        let (kind, member, plan) = match *e {
            CrewEvent::Planned(plan) => (1, 0, plan),
            CrewEvent::Activated(plan) => (2, 0, plan),
            CrewEvent::Signal => (3, 0, 0),
            CrewEvent::Order {
                member,
                goal,
                start_at,
            } => {
                r[3] = start_at.unwrap_or(0) as i32;
                r[4] = i32::from(start_at.is_some());
                r[5..].copy_from_slice(&goal_record(&goal));
                (4, member, goal.plan)
            }
            CrewEvent::Start { member } => (5, member, 0),
            CrewEvent::Arrived { member } => (6, member, 0),
            CrewEvent::Broken { member } => (7, member, 0),
            CrewEvent::NoPlan => (8, 0, 0),
        };
        r[0] = kind;
        r[1] = member as i32;
        r[2] = plan as i32;
        w.out.extend_from_slice(&r);
    }
    to_i32(w.events.len())
}

/// The member's decider inputs at `now` (5 numbers) in the out buffer; 1,
/// or 0 if unknown.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_member_inputs(c: *mut WasmCrew, id: u32, now: u32) -> i32 {
    let w = crew_ref!(c);
    w.out.clear();
    match w.crew.member_inputs(id, now) {
        Some(i) => {
            w.out.extend_from_slice(&i.as_row());
            1
        }
        None => 0,
    }
}

/// What the member chose at `now`: its goal (`goal_won` 1) or something
/// else (0). Returns 1, or 0 if unknown.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_member_chose(
    c: *mut WasmCrew,
    id: u32,
    now: u32,
    goal_won: u32,
) -> i32 {
    i32::from(crew!(c).crew.member_chose(id, now, goal_won != 0))
}

/// The member's goal record (20 numbers) in the out buffer; 1, or 0 if
/// unknown or without a goal.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_goal(c: *mut WasmCrew, id: u32) -> i32 {
    let w = crew_ref!(c);
    w.out.clear();
    match w.crew.goal(id) {
        Some(g) => {
            w.out.extend_from_slice(&goal_record(&g));
            1
        }
        None => 0,
    }
}

/// The crew's own squad, for every `sx_squad_*` call except
/// `sx_squad_orders_due` (-1: the crew takes the orders), until the crew is
/// freed. It belongs to the crew (sx_squad_free does nothing to it).
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_squad(c: *mut WasmCrew) -> *mut WasmSquad {
    // SAFETY: see above.
    unsafe { c.as_ref() }.map_or(std::ptr::null_mut(), |w| w.squad)
}

/// The keeper of member `id`, for every `sx_keeper_*` call until the next
/// call that changes the crew or until the member leaves the group (then
/// they return -1, see [`sx_keeper_live`](super::sx_keeper_live)). One
/// handle per member id, freed with the crew (sx_keeper_free does nothing
/// to it). Null if the id is unknown.
///
/// # Safety
/// `c` from [`sx_crew_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_crew_keeper(c: *mut WasmCrew, id: u32) -> *mut WasmKeeper {
    // SAFETY: see above.
    let Some(w) = (unsafe { c.as_mut() }) else {
        return std::ptr::null_mut();
    };
    if keeper_of(w, id).is_none() {
        return std::ptr::null_mut();
    }
    let lend = Lend {
        root: c,
        id,
        epoch: Some(w.epoch),
        get: keeper_of,
    };
    if let Some(&(_, k)) = w.keepers.iter().find(|(m, _)| *m == id) {
        // SAFETY: made below for this crew, freed only with it; its own
        // allocation, so writing it does not touch the crew.
        unsafe { (*k).keeper = Held::Crew(lend) };
        return k;
    }
    let k = Box::into_raw(Box::new(WasmKeeper {
        keeper: Held::Crew(lend),
        out: Vec::new(),
    }));
    w.keepers.push((id, k));
    k
}
