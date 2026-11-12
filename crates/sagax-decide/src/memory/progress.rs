/// Progress toward a goal: the best (smallest) value reached so far and when.
///
/// Typically the value is the distance to a target. "Lost" is not here: it
/// is the age of the fact in [`Knowledge`](super::Knowledge). Recipe note for
/// bit-exact staggered sight: check "lost" only on sight ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Progress {
    /// Smallest value reached so far.
    pub best: i32,
    /// Time the best value was reached (or the check began).
    pub since: u32,
    /// 0 until [`Progress::start`].
    pub started: u8,
}

impl Progress {
    /// Starts (or restarts) the check from `value` at `now`.
    pub fn start(&mut self, value: i32, now: u32) {
        *self = Self {
            best: value,
            since: now,
            started: 1,
        };
    }

    /// Reports the current `value`. If it is at least `min_gain` below the
    /// best so far, that counts as progress: `best` and `since` are updated
    /// and true is returned. Otherwise nothing changes (false). Before
    /// [`Progress::start`] nothing changes and false is returned.
    pub fn update(&mut self, value: i32, now: u32, min_gain: i32) -> bool {
        if self.started == 0 {
            return false;
        }
        if i64::from(self.best) - i64::from(value) >= i64::from(min_gain) {
            self.best = value;
            self.since = now;
            return true;
        }
        false
    }

    /// True if the last progress is at least `limit` old. Does not change
    /// anything. Never started: false.
    pub fn stalled(&self, now: u32, limit: u32) -> bool {
        self.started != 0 && now.wrapping_sub(self.since) >= limit
    }

    /// Back to never started.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
