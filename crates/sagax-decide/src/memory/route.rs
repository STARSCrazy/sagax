/// How the distance between two places is measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Metric {
    /// Straight line; [`distance`] returns the square (`dx² + dy²`).
    Euclid = 0,
    /// Sum of the axis distances (`|dx| + |dy|`).
    Manhattan = 1,
    /// Largest axis distance (`max(|dx|, |dy|)`).
    Chebyshev = 2,
    /// Hex grid in axial coordinates (`q = x`, `r = y`):
    /// `(|dq| + |dr| + |dq + dr|) / 2`. The sum is even for whole hex steps;
    /// with a divisor the axis differences are truncated first, and the
    /// final `/ 2` truncates an odd sum.
    Hex = 3,
}

/// Which candidates get a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Route {
    /// The `k` nearest, nearest first; a tie keeps the earlier candidate first.
    Nearest(usize),
    /// Every candidate within this plain distance (in the metric, after the
    /// divisor), inclusive, in candidate order. Negative reaches nobody.
    InRange(i64),
    /// Every candidate, in order.
    All,
    /// This one id, if it is among the candidates.
    To(u32),
}

/// Distance for the axis differences `dx`, `dy` under `metric`, computed in
/// `i128` so nothing overflows. [`Metric::Euclid`] returns the square.
pub fn distance(metric: Metric, dx: i64, dy: i64) -> i128 {
    let (dx, dy) = (i128::from(dx), i128::from(dy));
    match metric {
        Metric::Euclid => dx * dx + dy * dy,
        Metric::Manhattan => dx.abs() + dy.abs(),
        Metric::Chebyshev => dx.abs().max(dy.abs()),
        Metric::Hex => (dx.abs() + dy.abs() + (dx + dy).abs()) / 2,
    }
}

/// Picks recipients from `candidates` `(id, x, y)` for a message sent from
/// `origin`, writing their ids to `out` (cleared first).
///
/// Each axis difference is divided (truncating toward zero) by `divisor`
/// first; a `divisor` of 0 or less counts as 1. The caller filters the
/// candidates (team, free, ...). For [`Route::InRange`] with
/// [`Metric::Euclid`] the squared distance is compared with `r²`.
pub fn route(
    rule: Route,
    origin: (i32, i32),
    candidates: &[(u32, i32, i32)],
    metric: Metric,
    divisor: i32,
    out: &mut Vec<u32>,
) {
    out.clear();
    let div = i64::from(divisor.max(1));
    let dist = |x: i32, y: i32| {
        let dx = (i64::from(x) - i64::from(origin.0)) / div;
        let dy = (i64::from(y) - i64::from(origin.1)) / div;
        distance(metric, dx, dy)
    };
    match rule {
        Route::Nearest(k) => {
            if k == 0 {
                return;
            }
            let mut best: Vec<(i128, u32)> = Vec::with_capacity(k.min(candidates.len()));
            for &(id, x, y) in candidates {
                let d = dist(x, y);
                // First slot with a strictly greater distance: ties stay ahead.
                let at = best.iter().position(|b| b.0 > d).unwrap_or(best.len());
                if at < k {
                    best.insert(at, (d, id));
                    best.truncate(k);
                }
            }
            out.extend(best.iter().map(|b| b.1));
        }
        Route::InRange(r) => {
            if r < 0 {
                return;
            }
            let limit = match metric {
                Metric::Euclid => i128::from(r) * i128::from(r),
                _ => i128::from(r),
            };
            out.extend(
                candidates
                    .iter()
                    .filter(|&&(_, x, y)| dist(x, y) <= limit)
                    .map(|c| c.0),
            );
        }
        Route::All => out.extend(candidates.iter().map(|c| c.0)),
        Route::To(id) => {
            if candidates.iter().any(|c| c.0 == id) {
                out.push(id);
            }
        }
    }
}
