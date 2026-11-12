use super::{Pos, ZeroCapacity};

/// The last known place and time of one id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Sighting {
    /// What was seen.
    pub id: u32,
    /// Where it was last seen.
    pub pos: Pos,
    /// When it was last seen.
    pub last_seen: u32,
    /// A number the game may attach (kind, flags, ...).
    pub data: u32,
}

/// A bounded list of [`Sighting`]s in insertion order.
///
/// When full, a new id replaces the one seen longest ago (on a tie the
/// earlier slot). Updating an id keeps its slot.
#[derive(Clone, Debug)]
pub struct Sightings {
    items: Vec<Sighting>,
    capacity: usize,
}

impl Sightings {
    /// An empty memory for at most `capacity` ids.
    pub fn new(capacity: usize) -> Result<Self, ZeroCapacity> {
        if capacity == 0 {
            return Err(ZeroCapacity);
        }
        Ok(Self {
            items: Vec::with_capacity(capacity),
            capacity,
        })
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.items.iter().position(|s| s.id == id)
    }

    /// `id` was seen at `pos` at time `now`. A known id is updated in place
    /// (its `data` is kept). A new id is added; if the memory is full, the
    /// oldest sighting is dropped and its id returned.
    pub fn seen(&mut self, id: u32, pos: Pos, now: u32) -> Option<u32> {
        if let Some(i) = self.find(id) {
            self.items[i].pos = pos;
            self.items[i].last_seen = now;
            return None;
        }
        let mut evicted = None;
        if self.items.len() >= self.capacity {
            let mut oldest = 0;
            let mut oldest_age = 0;
            for (i, s) in self.items.iter().enumerate() {
                let age = now.wrapping_sub(s.last_seen);
                if i == 0 || age > oldest_age {
                    oldest = i;
                    oldest_age = age;
                }
            }
            evicted = Some(self.items.remove(oldest).id);
        }
        self.items.push(Sighting {
            id,
            pos,
            last_seen: now,
            data: 0,
        });
        evicted
    }

    /// Renews the time of `id` without changing its place. False if unknown.
    pub fn touch(&mut self, id: u32, now: u32) -> bool {
        match self.find(id) {
            Some(i) => {
                self.items[i].last_seen = now;
                true
            }
            None => false,
        }
    }

    /// Sets the place of `id` without changing its time. False if unknown.
    pub fn set_pos(&mut self, id: u32, pos: Pos) -> bool {
        match self.find(id) {
            Some(i) => {
                self.items[i].pos = pos;
                true
            }
            None => false,
        }
    }

    /// Sets the attached number of `id`. False if unknown.
    pub fn set_data(&mut self, id: u32, data: u32) -> bool {
        match self.find(id) {
            Some(i) => {
                self.items[i].data = data;
                true
            }
            None => false,
        }
    }

    /// Forgets `id`. False if unknown.
    pub fn remove(&mut self, id: u32) -> bool {
        match self.find(id) {
            Some(i) => {
                self.items.remove(i);
                true
            }
            None => false,
        }
    }

    /// The sighting of `id`, if known.
    pub fn get(&self, id: u32) -> Option<&Sighting> {
        self.find(id).map(|i| &self.items[i])
    }

    /// All sightings in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Sighting> {
        self.items.iter()
    }

    /// Number of sightings.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if nothing is remembered.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The most ids this memory holds.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Forgets every sighting whose age (`now - last_seen`) is at least
    /// `max_age`. `removed` is called with each id, in slot order.
    pub fn expire(&mut self, now: u32, max_age: u32, mut removed: impl FnMut(u32)) {
        self.items.retain(|s| {
            let gone = now.wrapping_sub(s.last_seen) >= max_age;
            if gone {
                removed(s.id);
            }
            !gone
        });
    }
}
