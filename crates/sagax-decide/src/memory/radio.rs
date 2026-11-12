use super::Pos;

/// A call for help: started at a place, delivered after a delay, and not
/// repeated until a cooldown has passed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Radio {
    /// Where the call was started.
    pub pos: Pos,
    /// Time the pending call was started.
    pub started: u32,
    /// Time of the last accepted start (valid if `has_sent` is not 0).
    pub last_start: u32,
    /// 1 while a call waits to be taken.
    pub pending: u8,
    /// 0 until the first accepted start.
    pub has_sent: u8,
}

impl Radio {
    /// Starts a call at `pos`. Refused (false) while one is pending, or if
    /// a call was started less than `cooldown` ago. The first call never
    /// waits for the cooldown.
    pub fn start(&mut self, pos: Pos, now: u32, cooldown: u32) -> bool {
        if self.pending != 0 {
            return false;
        }
        if self.has_sent != 0 && now.wrapping_sub(self.last_start) < cooldown {
            return false;
        }
        self.pos = pos;
        self.started = now;
        self.last_start = now;
        self.pending = 1;
        self.has_sent = 1;
        true
    }

    /// True if a call is pending and at least `delay` has passed since it
    /// was started.
    pub fn due(&self, now: u32, delay: u32) -> bool {
        self.pending != 0 && now.wrapping_sub(self.started) >= delay
    }

    /// Takes the pending call's place, ending the call.
    pub fn take(&mut self) -> Option<Pos> {
        if self.pending == 0 {
            return None;
        }
        self.pending = 0;
        Some(self.pos)
    }

    /// Drops a pending call. The cooldown keeps running.
    pub fn cancel(&mut self) {
        self.pending = 0;
    }

    /// True if a call is pending.
    pub fn pending(&self) -> bool {
        self.pending != 0
    }
}
