use super::{Pos, ZeroCapacity};

/// A remembered area: a centre, when it was last renewed, and a little data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Zone {
    /// Centre of the zone.
    pub pos: Pos,
    /// Time the zone was added or last renewed.
    pub since: u32,
    /// Caller data attached to the zone.
    pub data: u32,
    /// Where the cause of the zone was, valid only if `has_source` is not 0.
    pub source: Pos,
    /// 1 if `source` is set, else 0.
    pub has_source: u8,
}

/// Areas to stay out of, with a merge radius and a lifetime.
///
/// Distances are 2D (the third coordinate is ignored). A cell is inside a
/// zone if `dx*dx + dy*dy < radius*radius`.
#[derive(Clone, Debug)]
pub struct Zones {
    items: Vec<Zone>,
    capacity: usize,
    radius: i32,
    lifetime: u32,
}

impl Zones {
    /// An empty set for at most `capacity` zones. A `radius` of 0 or less is
    /// allowed; then nothing is ever inside a zone.
    pub fn new(capacity: usize, radius: i32, lifetime: u32) -> Result<Self, ZeroCapacity> {
        if capacity == 0 {
            return Err(ZeroCapacity);
        }
        Ok(Self {
            items: Vec::with_capacity(capacity),
            capacity,
            radius,
            lifetime,
        })
    }

    fn inside(&self, zone: &Zone, p: Pos) -> bool {
        let dx = i64::from(zone.pos[0]) - i64::from(p[0]);
        let dy = i64::from(zone.pos[1]) - i64::from(p[1]);
        let r = i64::from(self.radius);
        r > 0 && dx * dx + dy * dy < r * r
    }

    /// Adds a zone at `pos`. If `pos` is inside an existing zone (the first
    /// in slot order), that zone is renewed: centre, time, data and source
    /// are replaced. When full, the oldest zone is replaced (on a tie the
    /// earlier slot). Returns the index of the zone.
    pub fn add(&mut self, pos: Pos, now: u32, data: u32, source: Option<Pos>) -> usize {
        let zone = Zone {
            pos,
            since: now,
            data,
            source: source.unwrap_or([0; 3]),
            has_source: u8::from(source.is_some()),
        };
        if let Some(i) = self.contains(pos) {
            self.items[i] = zone;
            return i;
        }
        if self.items.len() < self.capacity {
            self.items.push(zone);
            return self.items.len() - 1;
        }
        let mut oldest = 0;
        let mut oldest_age = 0;
        for (i, z) in self.items.iter().enumerate() {
            let age = now.wrapping_sub(z.since);
            if i == 0 || age > oldest_age {
                oldest = i;
                oldest_age = age;
            }
        }
        self.items[oldest] = zone;
        oldest
    }

    /// Removes zones whose age is at least the lifetime, keeping the order
    /// of the rest. Returns how many were removed.
    pub fn expire(&mut self, now: u32) -> usize {
        let before = self.items.len();
        let lifetime = self.lifetime;
        self.items.retain(|z| now.wrapping_sub(z.since) < lifetime);
        before - self.items.len()
    }

    /// Index of the first zone containing `pos`, if any.
    pub fn contains(&self, pos: Pos) -> Option<usize> {
        self.items.iter().position(|z| self.inside(z, pos))
    }

    /// True if `cell` is in some zone that holds neither `start` nor `goal`.
    /// All zones are checked.
    pub fn blocks(&self, cell: Pos, start: Pos, goal: Pos) -> bool {
        self.items
            .iter()
            .any(|z| self.inside(z, cell) && !self.inside(z, start) && !self.inside(z, goal))
    }

    /// The zones in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Zone> {
        self.items.iter()
    }

    /// Number of zones.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if there are no zones.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Removes all zones.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}
