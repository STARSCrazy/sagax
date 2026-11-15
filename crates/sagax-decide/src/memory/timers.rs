use sagax_core::{NoopSink, Sink};

use super::ZeroCapacity;

/// What [`Timers::set_traced`] did, for debugging.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimersTrace {
    /// A new id was added.
    Started {
        /// The id.
        id: u32,
        /// Its ticks.
        duration: u32,
    },
    /// A present id was started again.
    Renewed {
        /// The id.
        id: u32,
        /// Its new ticks.
        duration: u32,
    },
    /// The set was full: the timer with the least time left gave way to the
    /// new id.
    Replaced {
        /// The new id.
        id: u32,
        /// Its ticks.
        duration: u32,
        /// The id that gave way.
        old: u32,
        /// The ticks it had left.
        old_left: u32,
    },
}

#[derive(Clone, Copy, Debug)]
struct Timer {
    id: u32,
    since: u32,
    duration: u32,
}

/// A bounded set of timers: each id is "on" for `duration` ticks after it
/// was set. Used for "ignore this target", "no alarm again for 30 s" or
/// "skip this round".
///
/// When full, a new id replaces the entry with the least time left (on a tie
/// the earlier slot).
#[derive(Clone, Debug)]
pub struct Timers {
    items: Vec<Timer>,
    capacity: usize,
}

impl Timers {
    /// An empty set for at most `capacity` ids; the whole capacity is
    /// reserved at once.
    pub fn new(capacity: usize) -> Result<Self, ZeroCapacity> {
        if capacity == 0 {
            return Err(ZeroCapacity);
        }
        Ok(Self {
            items: Vec::with_capacity(capacity),
            capacity,
        })
    }

    /// Starts `id` at `now` for `duration` ticks. A present id is renewed
    /// (new start and duration, same slot); when full the entry with the
    /// least time left is replaced.
    pub fn set(&mut self, id: u32, now: u32, duration: u32) {
        self.set_traced(id, now, duration, &mut NoopSink);
    }

    /// [`Timers::set`] with a trace: one [`TimersTrace`] record, `Renewed`,
    /// `Started` or (when full) `Replaced`.
    pub fn set_traced<S: Sink<TimersTrace>>(
        &mut self,
        id: u32,
        now: u32,
        duration: u32,
        sink: &mut S,
    ) {
        let timer = Timer {
            id,
            since: now,
            duration,
        };
        if let Some(t) = self.items.iter_mut().find(|t| t.id == id) {
            *t = timer;
            if sink.enabled() {
                sink.emit(TimersTrace::Renewed { id, duration });
            }
            return;
        }
        if self.items.len() < self.capacity {
            self.items.push(timer);
            if sink.enabled() {
                sink.emit(TimersTrace::Started { id, duration });
            }
            return;
        }
        let mut weakest = 0;
        let mut least = u32::MAX;
        for (i, t) in self.items.iter().enumerate() {
            let left = t.duration.saturating_sub(now.wrapping_sub(t.since));
            if i == 0 || left < least {
                weakest = i;
                least = left;
            }
        }
        let old = self.items[weakest].id;
        self.items[weakest] = timer;
        if sink.enabled() {
            sink.emit(TimersTrace::Replaced {
                id,
                duration,
                old,
                old_left: least,
            });
        }
    }

    /// True if `id` was set less than its duration ago.
    pub fn active(&self, id: u32, now: u32) -> bool {
        self.left(id, now) > 0
    }

    /// Ticks left for `id`; 0 if unknown or run out.
    pub fn left(&self, id: u32, now: u32) -> u32 {
        self.items
            .iter()
            .find(|t| t.id == id)
            .map_or(0, |t| t.duration.saturating_sub(now.wrapping_sub(t.since)))
    }

    /// Removes `id`. False if unknown.
    pub fn remove(&mut self, id: u32) -> bool {
        match self.items.iter().position(|t| t.id == id) {
            Some(i) => {
                self.items.remove(i);
                true
            }
            None => false,
        }
    }

    /// Forgets all timers.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Number of timers (running or run out).
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if there are no timers.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
