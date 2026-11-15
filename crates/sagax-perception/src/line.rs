//! Exact sight lines: integer walkers over every cell a segment passes,
//! with sub-cell endpoints and per-cell lengths for fog.
//!
//! Square: Amanatides–Woo with the crossing times as fractions, compared by
//! cross-multiplication in `i128`. Hex: the Voronoi walk in cube
//! coordinates; the border to the neighbour in direction `d` is
//! `(p - c)·d = S`, the exit time `(S - (A - c)·d) / ((B - A)·d)`.
//!
//! The walk visits the cells of `t` in `[0, 1]` in order. Crossings at
//! `t < 1` are walked; a target point on a border is entered at the end.
//! Where the line passes exactly through a corner (square) or vertex (hex),
//! the cells it only grazes have zero length: on square the line crosses
//! diagonally and passes if one of the two grazed cells lets it through
//! (an opaque pair blocks it); on hex the line crosses straight into the
//! next hex and the one grazed hex never blocks. A line running exactly
//! along a hex border belongs to the smaller of the two cells (by
//! [`Cell`] order), the same from both ends.

use std::cmp::Ordering;

use sagax_core::{Cell, Pos, axial_step};

use crate::map::{Medium, SenseMap};
use crate::world::{Grid, World, cell_of, distance, length_fix};

/// Depth at which nothing gets through: nine halvings of 256.
const DEPTH_LIMIT: u128 = 9 * 256;

/// The answer of a sight line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Line {
    /// Sight gets through: no wall, closed edge, closed corner or level in
    /// the way, and the clarity is above 0.
    pub clear: bool,
    /// How much gets through, 0 ..= 256 (256: clear air). 0 when not clear.
    pub clarity: u16,
    /// Where sight stopped: the opaque cell, or the cell behind a closed
    /// edge or corner. `None` when clear, or stopped by fog or by levels.
    pub blocked_at: Option<Cell>,
}

impl Line {
    const fn closed(at: Option<Cell>) -> Self {
        Self {
            clear: false,
            clarity: 0,
            blocked_at: at,
        }
    }

    fn through(depth: u128) -> Self {
        let clarity = clarity(depth);
        Self {
            clear: clarity > 0,
            clarity,
            blocked_at: None,
        }
    }
}

/// Clarity of an optical depth: `256 >> (depth / 256)`, then linear toward
/// the next halving by `depth % 256`.
fn clarity(depth: u128) -> u16 {
    if depth >= DEPTH_LIMIT {
        return 0;
    }
    let c0 = 256u32 >> (depth / 256);
    let c1 = c0 >> 1;
    let rest = (depth % 256) as u32;
    (c0 - (c0 - c1) * rest / 256) as u16
}

/// Whether sight gets from `from` to `to` (fine points) and how clearly.
///
/// Walks every cell the segment passes, exactly. The eye's cell never
/// blocks; the target's cell blocks only through its
/// [`sight_edge`](SenseMap::sight_edge); every cell in between blocks when
/// opaque. Fog adds up by length in every cell, both end cells included:
/// depth `Σ density · length / S`, clarity 256 halving per 256 depth. A line
/// between two levels is the map's
/// [`cross_level`](SenseMap::cross_level) answer, applied once over the
/// whole length. The walk stops at the first block or once nothing gets
/// through. `sight_line(a, b)` and `sight_line(b, a)` agree in `clear` and
/// `clarity` when the map's edges answer the same both ways.
pub fn sight_line(world: &World, map: &mut impl SenseMap, from: Pos, to: Pos) -> Line {
    sight_line_with(world, map, from, to, false)
}

/// [`sight_line`], with `thermal` reading `heat_density` instead of
/// `density`.
pub(crate) fn sight_line_with(
    world: &World,
    map: &mut impl SenseMap,
    from: Pos,
    to: Pos,
    thermal: bool,
) -> Line {
    let density = |m: Medium| u128::from(if thermal { m.heat_density } else { m.density });
    let len = length_fix(world, from, to);
    if from[2] != to[2] {
        return match map.cross_level(from, to) {
            Some(m) if !m.opaque => Line::through(depth(world, density(m) << 32, len)),
            _ => Line::closed(None),
        };
    }
    let target = cell_of(world, to);
    let mut sum = 0u128;
    let mut medium = Medium::default();
    let mut stop = None;
    walk(world, from, to, |visit| match visit {
        Visit::Enter(cell, via) => {
            let pass = match via {
                Via::Start => true,
                Via::Edge(prev) => map.sight_edge(prev, cell),
                Via::Corner(prev, sides) => sides.iter().any(|&side| {
                    !map.sight(side).opaque
                        && map.sight_edge(prev, side)
                        && map.sight_edge(side, cell)
                }),
            };
            medium = if pass {
                map.sight(cell)
            } else {
                Medium::default()
            };
            let blocks = !pass || (medium.opaque && via != Via::Start && cell != target);
            if blocks {
                stop = Some(Line::closed(Some(cell)));
            }
            !blocks
        }
        Visit::Length(w) => {
            sum = sum.saturating_add(density(medium) * u128::from(w));
            if depth(world, sum, len) >= DEPTH_LIMIT {
                stop = Some(Line::closed(None));
            }
            stop.is_none()
        }
    });
    stop.unwrap_or_else(|| Line::through(depth(world, sum, len)))
}

/// The cost of a sound going straight from `from` to `to` (the `Straight`
/// spread): the straight distance plus `damping / 256` times the map's
/// damping along the walk (`sound · length / S` per cell, `sound_edge` per
/// border; at a square corner the cheaper of the two ways round). `None`
/// when an edge lets nothing through or the levels differ. Saturates at
/// `i32::MAX`.
pub(crate) fn sound_cost(
    world: &World,
    map: &mut impl SenseMap,
    from: Pos,
    to: Pos,
    damping: u16,
) -> Option<i32> {
    if from[2] != to[2] {
        return None;
    }
    let mut sum = 0u128;
    let mut edges = 0u128;
    let mut here = 0u32;
    let mut open = true;
    walk(world, from, to, |visit| match visit {
        Visit::Enter(cell, via) => {
            let crossing = match via {
                Via::Start => Some(0),
                Via::Edge(prev) => map.sound_edge(prev, cell).map(u128::from),
                Via::Corner(prev, sides) => sides
                    .iter()
                    .filter_map(|&side| {
                        let into = map.sound_edge(prev, side)?;
                        let out = map.sound_edge(side, cell)?;
                        Some(u128::from(into) + u128::from(out))
                    })
                    .min(),
            };
            match crossing {
                Some(e) => {
                    edges = edges.saturating_add(e);
                    here = map.sound(cell);
                }
                None => open = false,
            }
            open
        }
        Visit::Length(w) => {
            sum = sum.saturating_add(u128::from(here) * u128::from(w));
            true
        }
    });
    if !open {
        return None;
    }
    let damp = depth(world, sum, length_fix(world, from, to)).saturating_add(edges);
    let damped = damp.saturating_mul(u128::from(damping)) / 256;
    let cost = u128::from(distance(world, from, to).unsigned_abs()).saturating_add(damped);
    Some(cost.min(i32::MAX as u128) as i32)
}

/// `Σ value · weight` (weights of `2^32` per whole segment) over the length
/// `len` (fine units times 65536), per cell length `S`, rounded.
fn depth(world: &World, sum: u128, len: u128) -> u128 {
    let den = (world.size() as u128) << 48;
    sum.saturating_mul(len).saturating_add(den / 2) / den
}

/// How the walk got into a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Via {
    /// The eye's cell.
    Start,
    /// Across the border from this neighbour.
    Edge(Cell),
    /// Diagonally through the corner from this cell, grazing the two side
    /// cells (square only).
    Corner(Cell, [Cell; 2]),
}

/// One step of a walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Visit {
    /// The walk enters a cell.
    Enter(Cell, Via),
    /// The share of the segment inside the current cell, times `2^32`
    /// (rounded).
    Length(u64),
}

/// A time on the segment, `num / den` with `den > 0`.
#[derive(Clone, Copy, Debug)]
struct Time {
    num: i128,
    den: i128,
}

impl Time {
    const ZERO: Self = Self { num: 0, den: 1 };
    const END: Self = Self { num: 1, den: 1 };

    fn new(num: i64, den: i64) -> Self {
        Self {
            num: i128::from(num),
            den: i128::from(den),
        }
    }

    fn cmp(self, other: Self) -> Ordering {
        (self.num * other.den).cmp(&(other.num * self.den))
    }

    fn before_end(self) -> bool {
        self.num < self.den
    }

    /// `(to - self) · 2^32`, rounded; `to >= self`.
    fn weight(self, to: Self) -> u64 {
        let num = to.num * self.den - self.num * to.den;
        let den = to.den * self.den;
        ((num * (1 << 33) + den) / (2 * den)) as u64
    }
}

/// Walks the cells from `a` to `b` (fine points on `a`'s level), calling
/// `visit` until it answers `false` or the walk ends.
fn walk(world: &World, a: Pos, b: Pos, visit: impl FnMut(Visit) -> bool) {
    match world.grid {
        Grid::Square => walk_square(world, a, b, visit),
        Grid::Hex => walk_hex(world, a, b, visit),
    }
}

/// Amanatides–Woo over square cells `[x·S, (x+1)·S)`.
fn walk_square(world: &World, a: Pos, b: Pos, mut visit: impl FnMut(Visit) -> bool) {
    let s = world.size();
    let (ax, ay) = (i64::from(a[0]), i64::from(a[1]));
    let (dx, dy) = (i64::from(b[0]) - ax, i64::from(b[1]) - ay);
    let target = cell_of(world, b);
    let mut cur = cell_of(world, a);
    if !visit(Visit::Enter(cur, Via::Start)) {
        return;
    }
    // Time of the next border on one axis: moving up, the cell's upper
    // border; moving down, its own lower border (which belongs to it).
    let next = |c: i32, start: i64, d: i64| -> Option<Time> {
        let t = match d.signum() {
            1 => Time::new((i64::from(c) + 1) * s - start, d),
            -1 => Time::new(start - i64::from(c) * s, -d),
            _ => return None,
        };
        t.before_end().then_some(t)
    };
    let (sx, sy) = (dx.signum() as i32, dy.signum() as i32);
    let mut t = Time::ZERO;
    loop {
        let tx = next(cur.x, ax, dx);
        let ty = next(cur.y, ay, dy);
        let (at, order) = match (tx, ty) {
            (None, None) => break,
            (Some(tx), None) => (tx, Ordering::Less),
            (None, Some(ty)) => (ty, Ordering::Greater),
            (Some(tx), Some(ty)) => {
                let order = tx.cmp(ty);
                (if order.is_gt() { ty } else { tx }, order)
            }
        };
        let (into, via) = match order {
            Ordering::Less => (Cell::new(cur.x + sx, cur.y, cur.level), Via::Edge(cur)),
            Ordering::Greater => (Cell::new(cur.x, cur.y + sy, cur.level), Via::Edge(cur)),
            Ordering::Equal => {
                let sides = [
                    Cell::new(cur.x + sx, cur.y, cur.level),
                    Cell::new(cur.x, cur.y + sy, cur.level),
                ];
                let into = Cell::new(cur.x + sx, cur.y + sy, cur.level);
                (into, Via::Corner(cur, sides))
            }
        };
        if !visit(Visit::Length(t.weight(at))) || !visit(Visit::Enter(into, via)) {
            return;
        }
        (t, cur) = (at, into);
    }
    if !visit(Visit::Length(t.weight(Time::END))) {
        return;
    }
    if cur != target {
        // The target point lies on a border or corner of the last cell.
        let via = if cur.x != target.x && cur.y != target.y {
            let sides = [
                Cell::new(target.x, cur.y, cur.level),
                Cell::new(cur.x, target.y, cur.level),
            ];
            Via::Corner(cur, sides)
        } else {
            Via::Edge(cur)
        };
        visit(Visit::Enter(target, via));
    }
}

/// The cube direction of hex direction `k`.
fn cube_dir(k: usize) -> [i64; 3] {
    let (q, r) = axial_step(k);
    let (q, r) = (i64::from(q), i64::from(r));
    [q, r, -q - r]
}

fn dot(a: [i64; 3], b: [i64; 3]) -> i64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The hex next to `c` in direction `k`.
fn hex_step(c: Cell, k: usize) -> Cell {
    let (q, r) = axial_step(k);
    Cell::new(c.x + q, c.y + r, c.level)
}

/// The Voronoi walk over hexes in cube coordinates.
fn walk_hex(world: &World, a: Pos, b: Pos, mut visit: impl FnMut(Visit) -> bool) {
    let s = world.size();
    let cube = |p: Pos| {
        let (q, r) = (i64::from(p[0]), i64::from(p[1]));
        [q, r, -q - r]
    };
    let (pa, pb) = (cube(a), cube(b));
    let e = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    // `(A - c)·d` for the centre of `c`.
    let offset = |c: Cell, d: [i64; 3]| {
        let (q, r) = (i64::from(c.x) * s, i64::from(c.y) * s);
        dot([pa[0] - q, pa[1] - r, pa[2] + q + r], d)
    };
    // Exit times of `c` before the end, as (direction, time).
    let exits = |c: Cell| {
        (0..6).filter_map(move |k| {
            let d = cube_dir(k);
            let toward = dot(e, d);
            let t = Time::new(s - offset(c, d), toward);
            (toward > 0 && t.before_end()).then_some((k, t))
        })
    };
    let target = cell_of(world, b);
    let mut cur = cell_of(world, a);
    if !visit(Visit::Enter(cur, Via::Start)) {
        return;
    }
    // Starting on a border and running along it: belong to the smaller
    // of the two cells, as the walk from the other end does.
    if e != [0; 3] && exits(cur).all(|(_, t)| t.num != 0) {
        let along = (0..6).find(|&k| {
            let d = cube_dir(k);
            dot(e, d) == 0 && offset(cur, d) == s
        });
        if let Some(k) = along {
            let side = hex_step(cur, k);
            if side < cur {
                if !visit(Visit::Enter(side, Via::Edge(cur))) {
                    return;
                }
                cur = side;
            }
        }
    }
    let mut t = Time::ZERO;
    loop {
        // The first exit; two at the same time mean a vertex.
        let mut first: Option<(usize, Time)> = None;
        let mut tied = None;
        for (k, tk) in exits(cur) {
            match first.map(|(_, tf)| tk.cmp(tf)) {
                None | Some(Ordering::Less) => (first, tied) = (Some((k, tk)), None),
                Some(Ordering::Equal) => tied = Some(k),
                Some(Ordering::Greater) => {}
            }
        }
        let Some((k1, at)) = first else {
            break;
        };
        let into = match tied {
            None => hex_step(cur, k1),
            Some(k2) => {
                // Past the vertex the line is on the side of the hex it
                // turns toward; exactly between, on the smaller one.
                let (d1, d2) = (cube_dir(k1), cube_dir(k2));
                let turn = dot(e, [d2[0] - d1[0], d2[1] - d1[1], d2[2] - d1[2]]);
                let (h1, h2) = (hex_step(cur, k1), hex_step(cur, k2));
                match turn.signum() {
                    1 => h2,
                    -1 => h1,
                    _ => h1.min(h2),
                }
            }
        };
        if !visit(Visit::Length(t.weight(at))) || !visit(Visit::Enter(into, Via::Edge(cur))) {
            return;
        }
        (t, cur) = (at, into);
    }
    if !visit(Visit::Length(t.weight(Time::END))) {
        return;
    }
    if cur != target {
        // The target point lies on a border or vertex of the last hex.
        visit(Visit::Enter(target, Via::Edge(cur)));
    }
}
