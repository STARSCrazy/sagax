//! `sx_sightings_*`, `sx_zones_*`, `sx_ignore_*`, value structs and search
//! helpers: world memory, see `sagax.h`.

use sagax_decide::memory::{
    Ignore, Interval, LOOK_OFFSETS, Pursuit, Radio, SearchPattern, Sighting, Sightings, Zone,
    Zones, look_heading, nearest, search_point, sin256,
};

use super::{ERROR_ARGUMENT, create, destroy, slice_of, to_i32};

/// `sx_sighting`.
pub type SxSighting = Sighting;
/// `sx_zone`.
pub type SxZone = Zone;
/// `sx_pursuit`.
pub type SxPursuit = Pursuit;
/// `sx_radio`.
pub type SxRadio = Radio;
/// `sx_interval`.
pub type SxInterval = Interval;

/// `sx_nearby` (candidate of [`sx_nearest`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxNearby {
    /// The game's id.
    pub id: u32,
    /// Place, x.
    pub x: i32,
    /// Place, y.
    pub y: i32,
}

/// Most entries of a search pattern.
const PATTERN_MAX: usize = 8;

/// `sx_search_pattern`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxSearchPattern {
    /// Heading between neighbouring agents.
    pub heading_step: i32,
    /// Distance of step 0.
    pub first_distance: i32,
    /// Number of used entries in `offsets` and `distances` (at most 8).
    pub count: u32,
    /// Turn of each later step.
    pub offsets: [i32; PATTERN_MAX],
    /// Distance of each later step.
    pub distances: [i32; PATTERN_MAX],
    /// Smallest allowed x.
    pub min_x: i32,
    /// Smallest allowed y.
    pub min_y: i32,
    /// Largest allowed x.
    pub max_x: i32,
    /// Largest allowed y.
    pub max_y: i32,
}

fn flag(b: bool) -> i32 {
    i32::from(b)
}

/// A reference to a handle or value struct, or `ERROR_ARGUMENT`.
macro_rules! get {
    ($p:expr) => {
        // SAFETY: the caller passes null or a valid pointer (sagax.h).
        match unsafe { $p.as_mut() } {
            Some(v) => v,
            None => return ERROR_ARGUMENT,
        }
    };
}

// ---- sightings --------------------------------------------------------------

/// `sx_sightings`.
pub struct SxSightings(Sightings);

/// `sx_sightings_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_sightings_new(capacity: u32) -> *mut SxSightings {
    create(|| Sightings::new(capacity as usize).ok().map(SxSightings))
}

/// `sx_sightings_free`.
///
/// # Safety
/// `s` is null or from [`sx_sightings_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_free(s: *mut SxSightings) {
    // SAFETY: see above.
    unsafe { destroy(s) }
}

/// `sx_sightings_seen`.
///
/// # Safety
/// `s` is null or valid; `evicted` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_seen(
    s: *mut SxSightings,
    id: u32,
    x: i32,
    y: i32,
    z: i32,
    now: u32,
    evicted: *mut u32,
) -> i32 {
    let s = get!(s);
    match s.0.seen(id, [x, y, z], now) {
        Some(gone) => {
            if !evicted.is_null() {
                // SAFETY: see above.
                unsafe { evicted.write(gone) };
            }
            1
        }
        None => 0,
    }
}

/// `sx_sightings_touch`.
///
/// # Safety
/// `s` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_touch(s: *mut SxSightings, id: u32, now: u32) -> i32 {
    flag(get!(s).0.touch(id, now))
}

/// `sx_sightings_set_pos`.
///
/// # Safety
/// `s` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_set_pos(
    s: *mut SxSightings,
    id: u32,
    x: i32,
    y: i32,
    z: i32,
) -> i32 {
    flag(get!(s).0.set_pos(id, [x, y, z]))
}

/// `sx_sightings_set_data`.
///
/// # Safety
/// `s` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_set_data(s: *mut SxSightings, id: u32, data: u32) -> i32 {
    flag(get!(s).0.set_data(id, data))
}

/// `sx_sightings_remove`.
///
/// # Safety
/// `s` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_remove(s: *mut SxSightings, id: u32) -> i32 {
    flag(get!(s).0.remove(id))
}

/// `sx_sightings_get`.
///
/// # Safety
/// `s` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_get(
    s: *mut SxSightings,
    id: u32,
    out: *mut SxSighting,
) -> i32 {
    let s = get!(s);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match s.0.get(id) {
        Some(found) => {
            // SAFETY: `out` is non-null and writable (see above); `write`
            // does not read or drop the old, possibly uninitialised, value.
            unsafe { out.write(*found) };
            1
        }
        None => 0,
    }
}

/// `sx_sightings_count`.
///
/// # Safety
/// `s` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_count(s: *mut SxSightings) -> i32 {
    to_i32(get!(s).0.len())
}

/// `sx_sightings_at`.
///
/// # Safety
/// `s` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_at(
    s: *mut SxSightings,
    index: u32,
    out: *mut SxSighting,
) -> i32 {
    let s = get!(s);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match s.0.iter().nth(index as usize) {
        Some(found) => {
            // SAFETY: `out` is non-null and writable (see above); `write`
            // does not read or drop the old, possibly uninitialised, value.
            unsafe { out.write(*found) };
            1
        }
        None => 0,
    }
}

/// `sx_sightings_expire`.
///
/// # Safety
/// `s` is null or valid; `removed` is null or has room for `removed_cap` ids.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_expire(
    s: *mut SxSightings,
    now: u32,
    max_age: u32,
    removed: *mut u32,
    removed_cap: u32,
) -> i32 {
    let s = get!(s);
    let mut count = 0usize;
    s.0.expire(now, max_age, |id| {
        if !removed.is_null() && count < removed_cap as usize {
            // SAFETY: within `removed_cap` (see above).
            unsafe { removed.add(count).write(id) };
        }
        count += 1;
    });
    to_i32(count)
}

/// `sx_sightings_clear`.
///
/// # Safety
/// `s` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sightings_clear(s: *mut SxSightings) -> i32 {
    get!(s).0.clear();
    0
}

// ---- zones ------------------------------------------------------------------

/// `sx_zones`.
pub struct SxZones(Zones);

/// `sx_zones_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_zones_new(capacity: u32, radius: i32, lifetime: u32) -> *mut SxZones {
    create(|| {
        Zones::new(capacity as usize, radius, lifetime)
            .ok()
            .map(SxZones)
    })
}

/// `sx_zones_free`.
///
/// # Safety
/// `z` is null or from [`sx_zones_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_zones_free(z: *mut SxZones) {
    // SAFETY: see above.
    unsafe { destroy(z) }
}

/// `sx_zones_add`.
///
/// # Safety
/// `zones` is null or valid; `source` is null or points to 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_zones_add(
    zones: *mut SxZones,
    x: i32,
    y: i32,
    z: i32,
    now: u32,
    data: u32,
    source: *const i32,
) -> i32 {
    let zones = get!(zones);
    // SAFETY: see above.
    let source = (!source.is_null()).then(|| unsafe { [*source, *source.add(1), *source.add(2)] });
    to_i32(zones.0.add([x, y, z], now, data, source))
}

/// `sx_zones_expire`.
///
/// # Safety
/// `z` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_zones_expire(z: *mut SxZones, now: u32) -> i32 {
    to_i32(get!(z).0.expire(now))
}

/// `sx_zones_contains`.
///
/// # Safety
/// `zones` is null or valid; `index` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_zones_contains(
    zones: *mut SxZones,
    x: i32,
    y: i32,
    z: i32,
    index: *mut u32,
) -> i32 {
    match get!(zones).0.contains([x, y, z]) {
        Some(i) => {
            if !index.is_null() {
                // SAFETY: see above.
                unsafe { index.write(i as u32) };
            }
            1
        }
        None => 0,
    }
}

/// `sx_zones_blocks`.
///
/// # Safety
/// `z` is null or valid; `cell`, `start`, `goal` are null or point to 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_zones_blocks(
    z: *mut SxZones,
    cell: *const i32,
    start: *const i32,
    goal: *const i32,
) -> i32 {
    let z = get!(z);
    if cell.is_null() || start.is_null() || goal.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let pos = |p: *const i32| unsafe { [*p, *p.add(1), *p.add(2)] };
    flag(z.0.blocks(pos(cell), pos(start), pos(goal)))
}

/// `sx_zones_count`.
///
/// # Safety
/// `z` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_zones_count(z: *mut SxZones) -> i32 {
    to_i32(get!(z).0.len())
}

/// `sx_zones_at`.
///
/// # Safety
/// `z` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_zones_at(z: *mut SxZones, index: u32, out: *mut SxZone) -> i32 {
    let z = get!(z);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match z.0.iter().nth(index as usize) {
        Some(found) => {
            // SAFETY: `out` is non-null and writable (see above); `write`
            // does not read or drop the old, possibly uninitialised, value.
            unsafe { out.write(*found) };
            1
        }
        None => 0,
    }
}

/// `sx_zones_clear`.
///
/// # Safety
/// `z` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_zones_clear(z: *mut SxZones) -> i32 {
    get!(z).0.clear();
    0
}

// ---- ignore -----------------------------------------------------------------

/// `sx_ignore`.
pub struct SxIgnore(Ignore);

/// `sx_ignore_new`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_ignore_new(capacity: u32) -> *mut SxIgnore {
    create(|| Ignore::new(capacity as usize).ok().map(SxIgnore))
}

/// `sx_ignore_free`.
///
/// # Safety
/// `i` is null or from [`sx_ignore_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_ignore_free(i: *mut SxIgnore) {
    // SAFETY: see above.
    unsafe { destroy(i) }
}

/// `sx_ignore_add`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_ignore_add(i: *mut SxIgnore, id: u32, now: u32) -> i32 {
    get!(i).0.add(id, now);
    0
}

/// `sx_ignore_ignores`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_ignore_ignores(
    i: *mut SxIgnore,
    id: u32,
    now: u32,
    duration: u32,
) -> i32 {
    flag(get!(i).0.ignores(id, now, duration))
}

/// `sx_ignore_clear`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_ignore_clear(i: *mut SxIgnore) -> i32 {
    get!(i).0.clear();
    0
}

// ---- pursuit ----------------------------------------------------------------

/// `sx_pursuit_begin`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_pursuit_begin(
    p: *mut SxPursuit,
    target: u32,
    x: i32,
    y: i32,
    z: i32,
    distance: i32,
    now: u32,
) -> i32 {
    get!(p).begin(target, [x, y, z], distance, now);
    0
}

/// `sx_pursuit_seen`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_pursuit_seen(p: *mut SxPursuit, x: i32, y: i32, z: i32) -> i32 {
    get!(p).seen([x, y, z]);
    0
}

/// `sx_pursuit_missed`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_pursuit_missed(p: *mut SxPursuit, step: u32) -> i32 {
    get!(p).missed(step);
    0
}

/// `sx_pursuit_reset_progress`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_pursuit_reset_progress(
    p: *mut SxPursuit,
    distance: i32,
    now: u32,
) -> i32 {
    get!(p).reset_progress(distance, now);
    0
}

/// `sx_pursuit_lost`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_pursuit_lost(p: *mut SxPursuit, limit: u32) -> i32 {
    flag(get!(p).lost(limit))
}

/// `sx_pursuit_stalled`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_pursuit_stalled(
    p: *mut SxPursuit,
    distance: i32,
    now: u32,
    min_gain: i32,
    limit: u32,
) -> i32 {
    flag(get!(p).stalled(distance, now, min_gain, limit))
}

/// `sx_pursuit_clear`.
///
/// # Safety
/// `p` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_pursuit_clear(p: *mut SxPursuit) -> i32 {
    get!(p).clear();
    0
}

// ---- radio ------------------------------------------------------------------

/// `sx_radio_start`.
///
/// # Safety
/// `r` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_radio_start(
    r: *mut SxRadio,
    x: i32,
    y: i32,
    z: i32,
    now: u32,
    cooldown: u32,
) -> i32 {
    flag(get!(r).start([x, y, z], now, cooldown))
}

/// `sx_radio_due`.
///
/// # Safety
/// `r` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_radio_due(r: *mut SxRadio, now: u32, delay: u32) -> i32 {
    flag(get!(r).due(now, delay))
}

/// `sx_radio_take`.
///
/// # Safety
/// `r` is null or valid; `xyz` is null or has room for 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_radio_take(r: *mut SxRadio, xyz: *mut i32) -> i32 {
    let r = get!(r);
    if xyz.is_null() {
        return ERROR_ARGUMENT;
    }
    match r.take() {
        Some(pos) => {
            // SAFETY: see above.
            unsafe { std::ptr::copy_nonoverlapping(pos.as_ptr(), xyz, 3) };
            1
        }
        None => 0,
    }
}

/// `sx_radio_cancel`.
///
/// # Safety
/// `r` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_radio_cancel(r: *mut SxRadio) -> i32 {
    get!(r).cancel();
    0
}

// ---- interval ---------------------------------------------------------------

/// `sx_interval_due`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_interval_due(i: *mut SxInterval, now: u32, every: u32) -> i32 {
    flag(get!(i).due(now, every))
}

/// `sx_interval_mark`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_interval_mark(i: *mut SxInterval, now: u32) -> i32 {
    get!(i).mark(now);
    0
}

/// `sx_interval_reset`.
///
/// # Safety
/// `i` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_interval_reset(i: *mut SxInterval) -> i32 {
    get!(i).reset();
    0
}

// ---- nearest and search -----------------------------------------------------

/// `sx_nearest`.
///
/// # Safety
/// `c` is null or points to `n` candidates; `out` is null or has room for `k`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_nearest(
    c: *const SxNearby,
    n: u32,
    px: i32,
    py: i32,
    k: u32,
    divisor: i32,
    out: *mut u32,
) -> i32 {
    if (c.is_null() && n > 0) || (out.is_null() && k > 0) {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let list: Vec<(u32, i32, i32)> = unsafe { slice_of(c, n as usize) }
        .iter()
        .map(|c| (c.id, c.x, c.y))
        .collect();
    let mut ids = Vec::new();
    nearest(&list, (px, py), k as usize, divisor, &mut ids);
    for (i, id) in ids.iter().enumerate() {
        // SAFETY: at most `k` ids (see above).
        unsafe { out.add(i).write(*id) };
    }
    to_i32(ids.len())
}

/// `sx_sin256`.
#[unsafe(no_mangle)]
pub extern "C" fn sx_sin256(heading: i32) -> i32 {
    sin256(heading)
}

/// `sx_search_point`.
///
/// # Safety
/// `pattern` is null or valid; `out` is null or has room for 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_search_point(
    agent: u32,
    step: u32,
    cx: i32,
    cy: i32,
    cz: i32,
    pattern: *const SxSearchPattern,
    out: *mut i32,
) -> i32 {
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: see above.
    let pattern = match unsafe { pattern.as_ref() } {
        None => SearchPattern::default(),
        Some(p) => {
            let n = p.count as usize;
            if n > PATTERN_MAX {
                return ERROR_ARGUMENT;
            }
            SearchPattern {
                heading_step: p.heading_step,
                first_distance: p.first_distance,
                offsets: p.offsets[..n].to_vec(),
                distances: p.distances[..n].to_vec(),
                min: (p.min_x, p.min_y),
                max: (p.max_x, p.max_y),
            }
        }
    };
    match search_point(agent, step as usize, [cx, cy, cz], &pattern) {
        Some(pos) => {
            // SAFETY: see above.
            unsafe { std::ptr::copy_nonoverlapping(pos.as_ptr(), out, 3) };
            1
        }
        None => 0,
    }
}

/// `sx_look_heading`.
///
/// # Safety
/// `offsets` is null or points to `count` integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_look_heading(
    base: i32,
    elapsed: u32,
    step_ticks: u32,
    offsets: *const i32,
    count: u32,
) -> i32 {
    if offsets.is_null() {
        return look_heading(base, elapsed, step_ticks, &LOOK_OFFSETS);
    }
    // SAFETY: see above.
    look_heading(base, elapsed, step_ticks, unsafe {
        slice_of(offsets, count as usize)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn layouts_match_header() {
        assert_eq!(size_of::<SxSighting>(), 24);
        assert_eq!(offset_of!(SxSighting, pos), 4);
        assert_eq!(offset_of!(SxSighting, last_seen), 16);
        assert_eq!(offset_of!(SxSighting, data), 20);
        assert_eq!(size_of::<SxZone>(), 36);
        assert_eq!(offset_of!(SxZone, since), 12);
        assert_eq!(offset_of!(SxZone, data), 16);
        assert_eq!(offset_of!(SxZone, source), 20);
        assert_eq!(offset_of!(SxZone, has_source), 32);
        assert_eq!(size_of::<SxPursuit>(), 28);
        assert_eq!(offset_of!(SxPursuit, last_pos), 4);
        assert_eq!(offset_of!(SxPursuit, unseen), 16);
        assert_eq!(offset_of!(SxPursuit, best), 20);
        assert_eq!(offset_of!(SxPursuit, since), 24);
        assert_eq!(size_of::<SxRadio>(), 24);
        assert_eq!(offset_of!(SxRadio, started), 12);
        assert_eq!(offset_of!(SxRadio, last_start), 16);
        assert_eq!(offset_of!(SxRadio, pending), 20);
        assert_eq!(offset_of!(SxRadio, has_sent), 21);
        assert_eq!(size_of::<SxInterval>(), 8);
        assert_eq!(offset_of!(SxInterval, started), 4);
        assert_eq!(size_of::<SxNearby>(), 12);
        assert_eq!(size_of::<SxSearchPattern>(), 3 * 4 + 64 + 16);
        assert_eq!(offset_of!(SxSearchPattern, offsets), 12);
        assert_eq!(offset_of!(SxSearchPattern, distances), 44);
        assert_eq!(offset_of!(SxSearchPattern, min_x), 76);
    }

    #[test]
    fn sightings_through_c() {
        unsafe {
            assert!(sx_sightings_new(0).is_null());
            let s = sx_sightings_new(2);
            let mut evicted = 0u32;
            assert_eq!(sx_sightings_seen(s, 1, 10, 20, 0, 5, &mut evicted), 0);
            assert_eq!(sx_sightings_seen(s, 2, 11, 21, 0, 6, &mut evicted), 0);
            assert_eq!(sx_sightings_seen(s, 3, 12, 22, 0, 7, &mut evicted), 1);
            assert_eq!(evicted, 1);
            assert_eq!(sx_sightings_count(s), 2);
            assert_eq!(sx_sightings_set_data(s, 3, 9), 1);
            assert_eq!(sx_sightings_set_data(s, 99, 9), 0);
            assert_eq!(sx_sightings_set_pos(s, 3, 1, 2, 3), 1);
            assert_eq!(sx_sightings_touch(s, 2, 20), 1);
            let mut out = Sighting {
                id: 0,
                pos: [0; 3],
                last_seen: 0,
                data: 0,
            };
            assert_eq!(sx_sightings_get(s, 3, &mut out), 1);
            assert_eq!((out.pos, out.data, out.last_seen), ([1, 2, 3], 9, 7));
            assert_eq!(sx_sightings_get(s, 1, &mut out), 0);
            assert_eq!(sx_sightings_at(s, 0, &mut out), 1);
            assert_eq!(out.id, 2);
            assert_eq!(sx_sightings_at(s, 2, &mut out), 0);
            let mut gone = [0u32; 4];
            assert_eq!(sx_sightings_expire(s, 20, 10, gone.as_mut_ptr(), 4), 1);
            assert_eq!(gone[0], 3);
            assert_eq!(sx_sightings_remove(s, 2), 1);
            assert_eq!(sx_sightings_clear(s), 0);
            assert_eq!(sx_sightings_count(s), 0);
            assert_eq!(sx_sightings_count(std::ptr::null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_sightings_get(s, 1, std::ptr::null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_sightings_at(s, 0, std::ptr::null_mut()), ERROR_ARGUMENT);
            sx_sightings_free(s);
            sx_sightings_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn zones_through_c() {
        unsafe {
            assert!(sx_zones_new(0, 10, 10).is_null());
            let z = sx_zones_new(2, 10, 50);
            let source = [1, 2, 3];
            assert_eq!(sx_zones_add(z, 100, 100, 0, 5, 7, source.as_ptr()), 0);
            assert_eq!(sx_zones_add(z, 103, 100, 0, 8, 7, std::ptr::null()), 0);
            assert_eq!(sx_zones_count(z), 1);
            let mut index = 9u32;
            assert_eq!(sx_zones_contains(z, 100, 105, 0, &mut index), 1);
            assert_eq!(index, 0);
            assert_eq!(sx_zones_contains(z, 500, 0, 0, std::ptr::null_mut()), 0);
            let (cell, far) = ([100, 100, 0], [900, 900, 0]);
            assert_eq!(
                sx_zones_blocks(z, cell.as_ptr(), far.as_ptr(), far.as_ptr()),
                1
            );
            assert_eq!(
                sx_zones_blocks(z, cell.as_ptr(), cell.as_ptr(), far.as_ptr()),
                0
            );
            assert_eq!(
                sx_zones_blocks(z, std::ptr::null(), far.as_ptr(), far.as_ptr()),
                ERROR_ARGUMENT
            );
            let mut out = Zone {
                pos: [0; 3],
                since: 0,
                data: 0,
                source: [0; 3],
                has_source: 0,
            };
            assert_eq!(sx_zones_at(z, 0, &mut out), 1);
            assert_eq!((out.since, out.has_source), (8, 0));
            assert_eq!(sx_zones_at(z, 1, &mut out), 0);
            assert_eq!(sx_zones_expire(z, 100), 1);
            assert_eq!(sx_zones_count(z), 0);
            assert_eq!(sx_zones_expire(std::ptr::null_mut(), 1), ERROR_ARGUMENT);
            assert_eq!(sx_zones_at(z, 0, std::ptr::null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_zones_add(z, 1, 1, 0, 1, 0, std::ptr::null()), 0);
            assert_eq!(sx_zones_count(z), 1);
            assert_eq!(sx_zones_clear(z), 0);
            assert_eq!(sx_zones_count(z), 0);
            assert_eq!(sx_zones_clear(std::ptr::null_mut()), ERROR_ARGUMENT);
            sx_zones_free(z);
        }
    }

    #[test]
    fn ignore_through_c() {
        unsafe {
            assert!(sx_ignore_new(0).is_null());
            let i = sx_ignore_new(2);
            assert_eq!(sx_ignore_add(i, 4, 10), 0);
            assert_eq!(sx_ignore_ignores(i, 4, 15, 10), 1);
            assert_eq!(sx_ignore_ignores(i, 4, 25, 10), 0);
            assert_eq!(sx_ignore_ignores(i, 5, 15, 10), 0);
            assert_eq!(sx_ignore_add(std::ptr::null_mut(), 1, 1), ERROR_ARGUMENT);
            assert_eq!(sx_ignore_clear(i), 0);
            assert_eq!(sx_ignore_ignores(i, 4, 15, 10), 0);
            assert_eq!(sx_ignore_clear(std::ptr::null_mut()), ERROR_ARGUMENT);
            sx_ignore_free(i);
        }
    }

    #[test]
    fn pursuit_through_c() {
        unsafe {
            let mut p = Pursuit::default();
            assert_eq!(sx_pursuit_begin(&mut p, 7, 1, 2, 3, 50, 100), 0);
            assert_eq!(sx_pursuit_missed(&mut p, 5), 0);
            assert_eq!(sx_pursuit_lost(&mut p, 5), 1);
            assert_eq!(sx_pursuit_seen(&mut p, 4, 5, 6), 0);
            assert_eq!(p.last_pos, [4, 5, 6]);
            assert_eq!(sx_pursuit_lost(&mut p, 5), 0);
            assert_eq!(sx_pursuit_stalled(&mut p, 50, 110, 5, 10), 1);
            assert_eq!(sx_pursuit_reset_progress(&mut p, 40, 120), 0);
            assert_eq!(sx_pursuit_clear(&mut p), 0);
            assert_eq!(p, Pursuit::default());
            assert_eq!(sx_pursuit_lost(std::ptr::null_mut(), 1), ERROR_ARGUMENT);
        }
    }

    #[test]
    fn radio_and_interval_through_c() {
        unsafe {
            let mut r = Radio::default();
            assert_eq!(sx_radio_start(&mut r, 1, 2, 3, 10, 100), 1);
            assert_eq!(sx_radio_start(&mut r, 1, 2, 3, 11, 100), 0);
            assert_eq!(sx_radio_due(&mut r, 12, 5), 0);
            assert_eq!(sx_radio_due(&mut r, 15, 5), 1);
            let mut xyz = [0; 3];
            assert_eq!(sx_radio_take(&mut r, xyz.as_mut_ptr()), 1);
            assert_eq!(xyz, [1, 2, 3]);
            assert_eq!(sx_radio_take(&mut r, xyz.as_mut_ptr()), 0);
            assert_eq!(sx_radio_take(&mut r, std::ptr::null_mut()), ERROR_ARGUMENT);
            assert_eq!(sx_radio_start(&mut r, 1, 2, 3, 50, 100), 0);
            assert_eq!(sx_radio_cancel(&mut r), 0);
            assert_eq!(sx_radio_cancel(std::ptr::null_mut()), ERROR_ARGUMENT);

            let mut i = Interval::default();
            assert_eq!(sx_interval_due(&mut i, 0, 10), 1);
            assert_eq!(sx_interval_mark(&mut i, 5), 0);
            assert_eq!(sx_interval_due(&mut i, 10, 10), 0);
            assert_eq!(sx_interval_due(&mut i, 15, 10), 1);
            assert_eq!(sx_interval_reset(&mut i), 0);
            assert_eq!(sx_interval_due(&mut i, 5, 10), 1);
            assert_eq!(sx_interval_mark(std::ptr::null_mut(), 1), ERROR_ARGUMENT);
        }
    }

    #[test]
    fn nearest_through_c() {
        unsafe {
            let c = [
                SxNearby {
                    id: 1,
                    x: 100,
                    y: 0,
                },
                SxNearby { id: 2, x: 10, y: 0 },
                SxNearby { id: 3, x: 50, y: 0 },
            ];
            let mut out = [0u32; 2];
            assert_eq!(sx_nearest(c.as_ptr(), 3, 0, 0, 2, 1, out.as_mut_ptr()), 2);
            assert_eq!(out, [2, 3]);
            let none = sx_nearest(std::ptr::null(), 0, 0, 0, 0, 1, std::ptr::null_mut());
            assert_eq!(none, 0);
            let bad = sx_nearest(std::ptr::null(), 3, 0, 0, 1, 1, out.as_mut_ptr());
            assert_eq!(bad, ERROR_ARGUMENT);
            let bad = sx_nearest(c.as_ptr(), 3, 0, 0, 2, 1, std::ptr::null_mut());
            assert_eq!(bad, ERROR_ARGUMENT);
        }
    }

    #[test]
    fn search_through_c() {
        unsafe {
            assert_eq!(sx_sin256(64), 256);
            let mut out = [0; 3];
            let mut reference = [0; 3];
            // The default pattern and the same pattern spelled out agree.
            let spelled = SxSearchPattern {
                heading_step: 0x62,
                first_distance: 0x100,
                count: 3,
                offsets: [0, 0x20, -0x20, 0, 0, 0, 0, 0],
                distances: [5 * 256, 9 * 256, 12 * 256, 0, 0, 0, 0, 0],
                min_x: 0,
                min_y: 0,
                max_x: 32767,
                max_y: 32767,
            };
            for step in 0..5 {
                let a = sx_search_point(3, step, 1000, 2000, 4, std::ptr::null(), out.as_mut_ptr());
                let b = sx_search_point(3, step, 1000, 2000, 4, &spelled, reference.as_mut_ptr());
                assert_eq!(a, b);
                assert_eq!(a, i32::from(step <= 3));
                if a == 1 {
                    assert_eq!(out, reference);
                    assert_eq!(out[2], 4);
                }
            }
            let too_many = SxSearchPattern {
                count: 9,
                ..spelled
            };
            let r = sx_search_point(3, 0, 0, 0, 0, &too_many, out.as_mut_ptr());
            assert_eq!(r, ERROR_ARGUMENT);
            let r = sx_search_point(3, 0, 0, 0, 0, std::ptr::null(), std::ptr::null_mut());
            assert_eq!(r, ERROR_ARGUMENT);

            assert_eq!(sx_look_heading(10, 0, 5, std::ptr::null(), 0), 10);
            assert_eq!(sx_look_heading(10, 5, 5, std::ptr::null(), 0), 42);
            let own = [1, 2];
            assert_eq!(sx_look_heading(10, 5, 5, own.as_ptr(), 2), 12);
            assert_eq!(sx_look_heading(10, 50, 5, own.as_ptr(), 2), 10);
        }
    }
}
