//! Path smoothing: straight lines instead of zigzag along grid cells.
//!
//! A grid path only moves in 8 (or 6) directions, so a walk at an odd angle
//! becomes a staircase. Smoothing keeps a cell only where the walker cannot
//! see past it: from each kept cell it jumps to the farthest later cell that
//! a straight line can reach. The result is a short list of corner points.
//!
//! This is post-smoothing of an A* path. It never makes a path longer in
//! straight-line terms, keeps every corner a wall forces, and costs one line
//! test per skipped cell. (Theta*, which smooths during the search, finds
//! slightly shorter any-angle paths at a higher search cost; not needed yet.)
//!
//! Square grids: [`line_clear`]. Hex grids: [`hex_line_clear`]. Both plug
//! into [`smooth_path`]. [`smooth_path_traced`] reports every line it tested
//! as a [`SmoothTrace`].

use sagax_core::{NoopSink, Sink};

use crate::grid::{Cell, CellMap};
use crate::hex::HexLayout;
use crate::search::Cost;

/// Keeps from `path` only the points a walker needs: the start, every point
/// where the straight line to the next kept point would be blocked, and the
/// end. `clear(a, b)` says whether one can walk straight from `a` to `b`.
///
/// From each kept point it looks for the farthest later point in sight by
/// doubling the distance, then halving back (a few line tests per corner
/// instead of one per cell). Every line it keeps was tested, so the result is
/// always walkable; where sight comes and goes along the path it may keep a
/// corner more than strictly needed.
pub fn smooth_path<N: Copy>(path: &[N], out: &mut Vec<N>, clear: impl FnMut(N, N) -> bool) {
    smooth_path_traced(path, out, clear, &mut NoopSink);
}

/// What [`smooth_path_traced`] reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmoothTrace<N> {
    /// The straight line from `from` to `to` was tested; `clear` is the
    /// answer of the line test.
    Shortcut {
        /// Where the line starts (a kept point).
        from: N,
        /// The later point it was tested to.
        to: N,
        /// `true` if the line was free.
        clear: bool,
    },
}

/// [`smooth_path`] that reports each line test to `sink`, in the order they
/// are made. The result is the same as without a sink.
pub fn smooth_path_traced<N: Copy, S: Sink<SmoothTrace<N>> + ?Sized>(
    path: &[N],
    out: &mut Vec<N>,
    mut clear: impl FnMut(N, N) -> bool,
    sink: &mut S,
) {
    let mut clear = |from: N, to: N| {
        let free = clear(from, to);
        if sink.enabled() {
            sink.emit(SmoothTrace::Shortcut {
                from,
                to,
                clear: free,
            });
        }
        free
    };
    out.clear();
    let Some(&first) = path.first() else {
        return;
    };
    out.push(first);
    let last = path.len() - 1;
    let mut i = 0;
    while i < last {
        // path[i + 1] is always reachable: it is the next step of the path
        let mut good = i + 1;
        let mut step = 2;
        let mut bad = None;
        while good < last {
            let probe = (i + step).min(last);
            if clear(path[i], path[probe]) {
                good = probe;
                step *= 2;
            } else {
                bad = Some(probe);
                break;
            }
        }
        if let Some(mut bad) = bad {
            while bad - good > 1 {
                let mid = good + (bad - good) / 2;
                if clear(path[i], path[mid]) {
                    good = mid;
                } else {
                    bad = mid;
                }
            }
        }
        out.push(path[good]);
        i = good;
    }
}

/// `true` if a walker can go in a straight line from the middle of `a` to the
/// middle of `b` on one level: every cell the line touches can be entered by
/// a step the map allows, without a level change and without extra cost
/// (smoothing must not cut through danger the search chose to avoid).
/// Where the line passes exactly through a cell corner, the diagonal step
/// and both side cells must be open.
pub fn line_clear<M: CellMap>(map: &mut M, a: Cell, b: Cell) -> bool {
    line_clear_within(map, a, b, 0)
}

/// [`line_clear`] that also crosses cells whose extra move cost is at most
/// `max_extra` (for maps that charge a little on every cell).
pub fn line_clear_within<M: CellMap>(map: &mut M, a: Cell, b: Cell, max_extra: Cost) -> bool {
    if a.level != b.level {
        return false;
    }
    let (dx, dy) = ((b.x - a.x).abs(), (b.y - a.y).abs());
    let (sx, sy) = ((b.x - a.x).signum(), (b.y - a.y).signum());
    let mut cur = a;
    // walk the cells the line crosses: compare when it leaves the cell in x
    // and in y (both scaled by 2 * dx * dy to stay in integers)
    let (mut ix, mut iy) = (0i64, 0i64);
    let (dx64, dy64) = (i64::from(dx), i64::from(dy));
    while (ix, iy) != (dx64, dy64) {
        let next_x = (2 * ix + 1) * dy64; // when the line crosses the next x border
        let next_y = (2 * iy + 1) * dx64; // ... the next y border
        let (mx, my) = if ix == dx64 {
            (0, sy)
        } else if iy == dy64 || next_x < next_y {
            (sx, 0)
        } else if next_y < next_x {
            (0, sy)
        } else {
            (sx, sy) // exactly through a corner
        };
        if !step_ok(map, cur, mx, my, max_extra) {
            return false;
        }
        if mx != 0
            && my != 0
            && !(step_ok(map, cur, mx, 0, max_extra) && step_ok(map, cur, 0, my, max_extra))
        {
            return false;
        }
        cur = Cell::new(cur.x + mx, cur.y + my, cur.level);
        ix += i64::from(mx != 0);
        iy += i64::from(my != 0);
    }
    true
}

/// Hex version of [`line_clear`]: `true` if a walker can go straight from
/// the middle of hex `a` to the middle of hex `b` on one level. Every hex the
/// line touches must be entered by an allowed step without extra cost. Where
/// the line runs exactly along the edge between two hexes, both sides must
/// be clear.
pub fn hex_line_clear<M: CellMap>(map: &mut M, layout: HexLayout, a: Cell, b: Cell) -> bool {
    hex_line_clear_within(map, layout, a, b, 0)
}

/// [`hex_line_clear`] that also crosses hexes whose extra move cost is at
/// most `max_extra`.
pub fn hex_line_clear_within<M: CellMap>(
    map: &mut M,
    layout: HexLayout,
    a: Cell,
    b: Cell,
    max_extra: Cost,
) -> bool {
    if a.level != b.level {
        return false;
    }
    let n = layout.distance(a.x, a.y, b.x, b.y);
    if n == 0 {
        return true;
    }
    let (aq, ar) = layout.to_axial(a.x, a.y);
    let (bq, br) = layout.to_axial(b.x, b.y);
    // cube coordinates (q, r, s = -q - r), nudged to both sides of an edge
    let (aq, ar, bq, br) = (f64::from(aq), f64::from(ar), f64::from(bq), f64::from(br));
    for side in [1.0, -1.0] {
        let e = 1e-6 * side;
        let mut cur = a;
        for k in 1..=n {
            let t = f64::from(k) / f64::from(n);
            let q = aq + (bq - aq) * t + e;
            let r = ar + (br - ar) * t + 2.0 * e;
            let (q, r) = cube_round(q, r, -q - r);
            let (x, y) = layout.from_axial(q, r);
            if (x, y) == (cur.x, cur.y) {
                continue;
            }
            if layout.direction(cur.x, cur.y, x, y).is_none() {
                return false; // cannot happen for a line, but never jump
            }
            let next = Cell::new(x, y, cur.level);
            if map.step(cur, x - cur.x, y - cur.y) != Some(cur.level)
                || map.move_cost(cur, next).is_none_or(|c| c > max_extra)
            {
                return false;
            }
            cur = next;
        }
    }
    true
}

/// Rounds fractional cube coordinates to the hex they lie in.
fn cube_round(q: f64, r: f64, s: f64) -> (i32, i32) {
    let (mut rq, mut rr, rs) = (q.round(), r.round(), s.round());
    let (dq, dr, ds) = ((rq - q).abs(), (rr - r).abs(), (rs - s).abs());
    if dq > dr && dq > ds {
        rq = -rr - rs;
    } else if dr > ds {
        rr = -rq - rs;
    }
    (rq as i32, rr as i32)
}

fn step_ok<M: CellMap>(map: &mut M, from: Cell, dx: i32, dy: i32, max_extra: Cost) -> bool {
    let to = Cell::new(from.x + dx, from.y + dy, from.level);
    map.step(from, dx, dy) == Some(from.level)
        && map.move_cost(from, to).is_some_and(|c| c <= max_extra)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{CellGoal, DenseGrid, GridGraph, GridRules, Preset};
    use crate::search::{Limits, find_path};

    #[test]
    fn open_ground_becomes_one_straight_line() {
        let rules = GridRules::preset(Preset::Octile);
        let mut g = GridGraph::new(DenseGrid::new(20, 20), rules, None);
        let r = find_path(
            &mut g,
            Cell::new(0, 0, 0),
            &CellGoal::new(17, 5, None, rules),
            Limits::default(),
            usize::MAX,
        );
        let mut out = Vec::new();
        smooth_path(&r.path, &mut out, |a, b| line_clear(&mut g.map, a, b));
        assert_eq!(out, [Cell::new(0, 0, 0), Cell::new(17, 5, 0)]);
    }

    #[test]
    fn traced_smoothing_reports_each_line_and_changes_nothing() {
        use sagax_debug::{Collector, Overflow};
        let mut map = DenseGrid::new(20, 20);
        for y in 0..15 {
            map.set(10, y, DenseGrid::BLOCKED);
        }
        let rules = GridRules::preset(Preset::Octile);
        let mut g = GridGraph::new(map, rules, None);
        let r = find_path(
            &mut g,
            Cell::new(2, 2, 0),
            &CellGoal::new(18, 2, None, rules),
            Limits::default(),
            usize::MAX,
        );
        let (mut plain, mut traced) = (Vec::new(), Vec::new());
        smooth_path(&r.path, &mut plain, |a, b| line_clear(&mut g.map, a, b));
        let mut log = Collector::new(1000, Overflow::DropNewest);
        let mut tests = 0;
        smooth_path_traced(
            &r.path,
            &mut traced,
            |a, b| {
                tests += 1;
                line_clear(&mut g.map, a, b)
            },
            &mut log,
        );
        assert_eq!(plain, traced);
        assert_eq!(log.records().len(), tests);
        for rec in log.records() {
            let SmoothTrace::Shortcut { from, to, clear } = rec.event;
            assert_eq!(clear, line_clear(&mut g.map, from, to));
        }
        // the wall forces at least one blocked line
        assert!(
            log.records()
                .any(|r| !matches!(r.event, SmoothTrace::Shortcut { clear: true, .. }))
        );

        // a straight two-cell path needs no test at all
        let mut log = Collector::new(10, Overflow::DropNewest);
        let two = [Cell::new(0, 0, 0), Cell::new(1, 0, 0)];
        smooth_path_traced(&two, &mut traced, |_, _| true, &mut log);
        assert_eq!(traced, two);
        assert_eq!(log.records().len(), 0);
    }

    #[test]
    fn exact_shortcut_trace() {
        use sagax_debug::{Collector, Overflow};
        // four points, the line from 0 to 2 is free, 0 to 3 is not
        let path = [0, 1, 2, 3];
        let mut out = Vec::new();
        let mut log = Collector::new(10, Overflow::DropNewest);
        smooth_path_traced(&path, &mut out, |a: i32, b: i32| b - a < 3, &mut log);
        assert_eq!(out, [0, 2, 3]);
        let events: Vec<_> = log.records().map(|r| r.event).collect();
        let s = |from, to, clear| SmoothTrace::Shortcut { from, to, clear };
        assert_eq!(events, [s(0, 2, true), s(0, 3, false)]);
    }

    #[test]
    fn corners_forced_by_walls_stay() {
        // wall along x = 10 from y = 0..15, gap below
        let rules = GridRules::preset(Preset::Octile);
        let mut map = DenseGrid::new(20, 20);
        for y in 0..15 {
            map.set(10, y, DenseGrid::BLOCKED);
        }
        let mut g = GridGraph::new(map, rules, None);
        let r = find_path(
            &mut g,
            Cell::new(2, 2, 0),
            &CellGoal::new(18, 2, None, rules),
            Limits::default(),
            usize::MAX,
        );
        let mut out = Vec::new();
        smooth_path(&r.path, &mut out, |a, b| line_clear(&mut g.map, a, b));
        assert!(out.len() >= 3 && out.len() < r.path.len());
        // every straight piece is walkable
        for w in out.windows(2) {
            assert!(
                line_clear(&mut g.map, w[0], w[1]),
                "{:?} -> {:?}",
                w[0],
                w[1]
            );
        }
        assert_eq!(out.first(), Some(&Cell::new(2, 2, 0)));
        assert_eq!(out.last(), Some(&Cell::new(18, 2, 0)));
    }

    #[test]
    fn lines_do_not_slip_through_corners_or_danger() {
        let mut map = DenseGrid::new(5, 5);
        map.set(1, 0, DenseGrid::BLOCKED);
        assert!(!line_clear(
            &mut map,
            Cell::new(0, 0, 0),
            Cell::new(2, 2, 0)
        ));
        let mut map = DenseGrid::new(5, 5);
        map.set(2, 0, 30);
        assert!(!line_clear(
            &mut map,
            Cell::new(0, 0, 0),
            Cell::new(4, 0, 0)
        ));
        assert!(line_clear(&mut map, Cell::new(0, 1, 0), Cell::new(4, 1, 0)));
    }
}
