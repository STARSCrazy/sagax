use super::tactic::{Place, Side};
use crate::memory::{Metric, Pos, distance};

/// A candidate position with tag bits (cover, high, hidden, choke, ... as
/// the game defines them). Laid out as C's `sx_spot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Spot {
    /// Where the spot is.
    pub pos: Pos,
    /// Tag bits of the spot.
    pub tags: u32,
}

/// The result of [`pick_spot`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpotPick {
    /// The index of the chosen spot.
    Spot(usize),
    /// No free spot: the wish point itself, not checked by the game.
    Unchecked(Pos),
}

/// sin 45° times 65536, rounded (as `sine(32, Sine::Fine)`).
const HALF_SQRT2: i64 = 46341;

/// The 6 hex directions in axial coordinates (`q = x`, `r = y`), step 0 =
/// +q, each next step turning toward +r.
const HEX_DIRS: [(i64, i64); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

fn is_hex(metric: Metric) -> bool {
    metric == Metric::Hex
}

/// Number of heading steps in a full turn: 6 on [`Metric::Hex`], else 8.
fn steps(metric: Metric) -> u8 {
    if is_hex(metric) { 6 } else { 8 }
}

/// The heading from `from` to `to` (x and y only).
///
/// On square metrics there are 8 steps of 45°: step 0 = +x, step 2 = +y,
/// step 4 = -x, step 6 = -y (each step turns from +x toward +y). On
/// [`Metric::Hex`] (axial, `q = x`, `r = y`) there are 6 steps of 60°:
/// step 0 = +q `(1, 0)`, then `(0, 1)`, `(-1, 1)`, `(-1, 0)`, `(0, -1)`,
/// `(1, -1)`. The nearest step wins, compared exactly in integers; a hex
/// vector exactly between two steps takes the lower step. `from == to`
/// gives step 0.
pub fn heading(metric: Metric, from: Pos, to: Pos) -> u8 {
    let dx = i128::from(to[0]) - i128::from(from[0]);
    let dy = i128::from(to[1]) - i128::from(from[1]);
    if dx == 0 && dy == 0 {
        return 0;
    }
    if is_hex(metric) {
        // Cube coordinates are an even (isometric) embedding of the hex
        // plane, so the largest dot product is the nearest direction.
        let cube = |q: i128, r: i128| [q, -q - r, r];
        let v = cube(dx, dy);
        let mut best = (0u8, i128::MIN);
        for (k, &(q, r)) in HEX_DIRS.iter().enumerate() {
            let d = cube(i128::from(q), i128::from(r));
            let dot = v[0] * d[0] + v[1] * d[1] + v[2] * d[2];
            if dot > best.1 {
                best = (k as u8, dot);
            }
        }
        return best.0;
    }
    // A vector is within 22.5° of an axis when the other axis is below
    // tan 22.5° = √2 - 1 of it, i.e. (a + b)² < 2a² (exact; never equal for
    // integers other than 0).
    let (ax, ay) = (dx.abs(), dy.abs());
    let sum = (ax + ay) * (ax + ay);
    if sum < 2 * ax * ax {
        if dx > 0 { 0 } else { 4 }
    } else if sum < 2 * ay * ay {
        if dy > 0 { 2 } else { 6 }
    } else {
        match (dx > 0, dy > 0) {
            (true, true) => 1,
            (false, true) => 3,
            (false, false) => 5,
            (true, false) => 7,
        }
    }
}

/// `n / d` rounded to nearest, halves away from zero (`d > 0`).
fn round_div(n: i64, d: i64) -> i64 {
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

fn sat(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Turns `(x, y)` by `step` heading steps (see [`heading`]).
///
/// Square: quarter turns are exact; an odd step adds 45° in fixed point,
/// rounded to nearest. Hex: each step is an exact 60° turn in axial
/// coordinates, `(q, r) -> (-r, q + r)`.
fn turn(metric: Metric, x: i64, y: i64, step: u8) -> (i64, i64) {
    let step = step % steps(metric);
    let (mut x, mut y) = (x, y);
    if is_hex(metric) {
        for _ in 0..step {
            (x, y) = (-y, x + y);
        }
        return (x, y);
    }
    for _ in 0..step / 2 {
        (x, y) = (-y, x);
    }
    if step % 2 == 1 {
        (x, y) = (
            round_div((x - y) * HALF_SQRT2, 65536),
            round_div((x + y) * HALF_SQRT2, 65536),
        );
    }
    (x, y)
}

fn add(p: Pos, dx: i64, dy: i64, dz: i64) -> Pos {
    [
        sat(i64::from(p[0]) + dx),
        sat(i64::from(p[1]) + dy),
        sat(i64::from(p[2]) + dz),
    ]
}

/// The point `dist` away from `target` on `side`, with `h` the heading from
/// the group centre to the target (step table in [`wish_point`]).
fn side_point(metric: Metric, target: Pos, side: Side, h: u8, dist: i32) -> Pos {
    let n = steps(metric);
    let quarter = if is_hex(metric) { 1 } else { 2 };
    let turn_by = match side {
        Side::Rear => 0,
        Side::Left => quarter,
        Side::Front => n / 2,
        Side::Right => n - quarter,
    };
    let (dx, dy) = turn(metric, i64::from(dist), 0, (h + turn_by) % n);
    add(target, dx, dy, 0)
}

/// The point a slot would like to stand on.
///
/// The heading `h` runs from `centre` (the group centre) to `target`; with
/// no target it is 0 and the target counts as at `centre`.
///
/// - [`Place::Offset`] gives `leader` plus the offset turned by `h` (z added
///   unturned). On hex the offset is in axial coordinates.
/// - [`Place::Side`] gives the point `dist` from the target on that side.
///   Square: rear =
///   step `h`, left = `h + 2`, front = `h + 4`, right = `h + 6`. Hex has no
///   90° direction: rear = `h`, left = `h + 1`, front = `h + 3`, right =
///   `h + 5`, so front, left and right are evenly 120° apart. Left means
///   left with +y pointing up (on a y-down screen it shows on the right).
/// - [`Place::Between`] gives halfway between target and `friend` (rounded
///   down); without a friend, like [`Side::Front`].
/// - [`Place::Tagged`] gives `centre`; the tag is matched by [`pick_spot`].
/// - [`Place::Here`] gives `leader`.
pub fn wish_point(
    place: &Place,
    leader: Pos,
    centre: Pos,
    target: Option<Pos>,
    friend: Option<Pos>,
    metric: Metric,
    dist: i32,
) -> Pos {
    let h = target.map_or(0, |t| heading(metric, centre, t));
    let at = target.unwrap_or(centre);
    match *place {
        Place::Offset(o) => {
            let (dx, dy) = turn(metric, i64::from(o[0]), i64::from(o[1]), h);
            add(leader, dx, dy, i64::from(o[2]))
        }
        Place::Side(side) => side_point(metric, at, side, h, dist),
        Place::Between => match (target, friend) {
            (Some(t), Some(f)) => {
                let mid = |a: i32, b: i32| sat((i64::from(a) + i64::from(b)).div_euclid(2));
                [mid(t[0], f[0]), mid(t[1], f[1]), mid(t[2], f[2])]
            }
            _ => side_point(metric, at, Side::Front, h, dist),
        },
        Place::Tagged(_) => centre,
        Place::Here => leader,
    }
}

/// Chooses a free spot for `wish`.
///
/// Free spots (not `taken[i]`; missing entries count as free) are ranked:
/// first those with all `want_tags` bits (0 makes every spot fit), then the
/// rest; within each, nearest to `wish` first (x and y in `metric`), ties
/// to the lower index. The result is rank `skip` (0 = best), clamped to the
/// last ranked spot. With no free spot: [`SpotPick::Unchecked`] of `wish`.
pub fn pick_spot(
    wish: Pos,
    want_tags: u32,
    spots: &[Spot],
    taken: &[bool],
    skip: u8,
    metric: Metric,
) -> SpotPick {
    let key = |i: usize, s: &Spot| {
        let miss = s.tags & want_tags != want_tags;
        let dx = i64::from(s.pos[0]) - i64::from(wish[0]);
        let dy = i64::from(s.pos[1]) - i64::from(wish[1]);
        (miss, distance(metric, dx, dy), i)
    };
    // Select rank by rank, each the smallest key above the previous one;
    // no allocation, and `skip` is small.
    let mut picked = None;
    for _ in 0..=skip {
        let next = spots
            .iter()
            .enumerate()
            .filter(|&(i, _)| !taken.get(i).copied().unwrap_or(false))
            .map(|(i, s)| key(i, s))
            .filter(|k| picked.is_none_or(|p| *k > p))
            .min();
        match next {
            Some(k) => picked = Some(k),
            None => break,
        }
    }
    match picked {
        Some((_, _, i)) => SpotPick::Spot(i),
        None => SpotPick::Unchecked(wish),
    }
}
