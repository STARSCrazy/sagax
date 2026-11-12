use super::Pos;

/// Chasing one target: where it was last seen, how long it has been out of
/// sight, and whether the chase is making progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Pursuit {
    /// Id of the target.
    pub target: u32,
    /// Last known place of the target.
    pub last_pos: Pos,
    /// Time spent without seeing the target.
    pub unseen: u32,
    /// Smallest distance reached so far.
    pub best: i32,
    /// Time the best distance was reached (or the chase began).
    pub since: u32,
}

impl Pursuit {
    /// Starts chasing `target` seen at `pos`, `distance` away, at `now`.
    pub fn begin(&mut self, target: u32, pos: Pos, distance: i32, now: u32) {
        *self = Self {
            target,
            last_pos: pos,
            unseen: 0,
            best: distance,
            since: now,
        };
    }

    /// The target is in sight at `pos`.
    pub fn seen(&mut self, pos: Pos) {
        self.last_pos = pos;
        self.unseen = 0;
    }

    /// The target was not in sight for another `step` of time (saturating).
    pub fn missed(&mut self, step: u32) {
        self.unseen = self.unseen.saturating_add(step);
    }

    /// Restarts the progress check from `distance` at `now`.
    pub fn reset_progress(&mut self, distance: i32, now: u32) {
        self.best = distance;
        self.since = now;
    }

    /// True if the target has been out of sight for at least `limit`.
    pub fn lost(&self, limit: u32) -> bool {
        self.unseen >= limit
    }

    /// True if no progress was made for at least `limit`. Getting at least
    /// `min_gain` closer than the best so far counts as progress and
    /// restarts the check.
    pub fn stalled(&mut self, distance: i32, now: u32, min_gain: i32, limit: u32) -> bool {
        if i64::from(self.best) - i64::from(distance) >= i64::from(min_gain) {
            self.best = distance;
            self.since = now;
            return false;
        }
        now.wrapping_sub(self.since) >= limit
    }

    /// Forgets the chase.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Distance measure for [`nearest`]: both axes divided (truncating toward
/// zero) by `divisor`, then squared and added, in `i64`.
fn scaled(dx: i64, dy: i64, divisor: i64) -> i64 {
    let (x, y) = (dx / divisor, dy / divisor);
    x * x + y * y
}

/// The ids of the `k` candidates `(id, x, y)` nearest to `point`, nearest
/// first, written to `out` (cleared first). Distance is
/// `(dx/divisor)² + (dy/divisor)²` with truncating division; a `divisor` of
/// 0 or less counts as 1. Equal distances keep the earlier candidate first.
pub fn nearest(
    candidates: &[(u32, i32, i32)],
    point: (i32, i32),
    k: usize,
    divisor: i32,
    out: &mut Vec<u32>,
) {
    out.clear();
    if k == 0 {
        return;
    }
    let div = i64::from(divisor.max(1));
    let mut best: Vec<(i64, u32)> = Vec::with_capacity(k.min(candidates.len()));
    for &(id, x, y) in candidates {
        let d = scaled(
            i64::from(x) - i64::from(point.0),
            i64::from(y) - i64::from(point.1),
            div,
        );
        // First slot whose distance is strictly greater: equal ones stay ahead.
        let at = best.iter().position(|b| b.0 > d).unwrap_or(best.len());
        if at < k {
            best.insert(at, (d, id));
            best.truncate(k);
        }
    }
    out.extend(best.iter().map(|b| b.1));
}
