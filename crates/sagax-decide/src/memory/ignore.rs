use super::ZeroCapacity;

/// Ids to leave alone for a while, each with the time it was added.
///
/// When full, a new id replaces the oldest entry (on a tie the earlier slot).
#[derive(Clone, Debug)]
pub struct Ignore {
    items: Vec<(u32, u32)>,
    capacity: usize,
}

impl Ignore {
    /// An empty list for at most `capacity` ids; the whole capacity is
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

    /// Ignore `id` from `now`. A present id is renewed; when full the
    /// oldest entry is replaced.
    pub fn add(&mut self, id: u32, now: u32) {
        if let Some(e) = self.items.iter_mut().find(|e| e.0 == id) {
            e.1 = now;
            return;
        }
        if self.items.len() < self.capacity {
            self.items.push((id, now));
            return;
        }
        let mut oldest = 0;
        let mut oldest_age = 0;
        for (i, e) in self.items.iter().enumerate() {
            let age = now.wrapping_sub(e.1);
            if i == 0 || age > oldest_age {
                oldest = i;
                oldest_age = age;
            }
        }
        self.items[oldest] = (id, now);
    }

    /// True if `id` was added less than `duration` ago.
    pub fn ignores(&self, id: u32, now: u32, duration: u32) -> bool {
        self.items
            .iter()
            .any(|e| e.0 == id && now.wrapping_sub(e.1) < duration)
    }

    /// Number of ids in the list.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Forgets all ids.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

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
