/// A timer for "every N ticks". Never started counts as due.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Interval {
    /// When it was last marked.
    pub last: u32,
    /// 0 until the first [`Interval::mark`].
    pub started: u8,
}

impl Interval {
    /// True if never marked, or at least `every` has passed since the mark.
    pub fn due(&self, now: u32, every: u32) -> bool {
        self.started == 0 || now.wrapping_sub(self.last) >= every
    }

    /// Records that it ran at `now`.
    pub fn mark(&mut self, now: u32) {
        self.last = now;
        self.started = 1;
    }

    /// Back to never started.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
