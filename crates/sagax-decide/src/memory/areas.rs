use super::{Pos, ZeroCapacity};

/// A remembered or announced area: a centre and radius, how long it lasts,
/// whether it can be walked through, and a little data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Area {
    /// The game's own kind of area (lit, noisy, searched, forbidden, ...).
    pub kind: u32,
    /// Centre of the area.
    pub pos: Pos,
    /// Radius; 0 or less means nothing is inside.
    pub radius: i32,
    /// Cost per cell for [`Areas::cost`] (non-blocking areas).
    pub strength: u32,
    /// Time the area was added or last renewed (set by [`Areas::add`]).
    pub since: u32,
    /// Ticks the area lasts; it expires at age `>= lifetime`.
    pub lifetime: u32,
    /// Caller data attached to the area.
    pub data: u32,
    /// Where the cause of the area was, valid only if `has_source` is not 0.
    pub source: Pos,
    /// 1 if the area blocks movement, else 0.
    pub blocking: u8,
    /// 1 if `source` is set, else 0.
    pub has_source: u8,
}

/// Bounded set of [`Area`]s.
///
/// Distances are 2D (the third coordinate is ignored). A cell is inside an
/// area if `dx*dx + dy*dy < radius*radius`, with the area's own radius.
#[derive(Clone, Debug)]
pub struct Areas {
    items: Vec<Area>,
    capacity: usize,
}

/// True if `p` is strictly inside the circle of `centre` and `radius`.
fn inside(centre: Pos, radius: i32, p: Pos) -> bool {
    // i128: the square of a difference of two i32 does not fit in i64.
    let dx = i128::from(centre[0]) - i128::from(p[0]);
    let dy = i128::from(centre[1]) - i128::from(p[1]);
    let r = i128::from(radius);
    r > 0 && dx * dx + dy * dy < r * r
}

impl Areas {
    /// An empty set for at most `capacity` areas; the whole capacity is
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

    /// Adds `area` (its `since` becomes `now`). The first existing area of
    /// the same kind whose centre is closer than that area's own radius to
    /// the new centre is renewed: all fields are replaced. Otherwise the
    /// area is appended; when full, the oldest is removed first (on a tie
    /// the earlier slot), the rest keep their order. Returns the index.
    pub fn add(&mut self, mut area: Area, now: u32) -> usize {
        area.since = now;
        if let Some(i) = self
            .items
            .iter()
            .position(|a| a.kind == area.kind && inside(a.pos, a.radius, area.pos))
        {
            self.items[i] = area;
            return i;
        }
        if self.items.len() >= self.capacity {
            let mut oldest = 0;
            let mut oldest_age = 0;
            for (i, a) in self.items.iter().enumerate() {
                let age = now.wrapping_sub(a.since);
                if i == 0 || age > oldest_age {
                    oldest = i;
                    oldest_age = age;
                }
            }
            self.items.remove(oldest);
        }
        self.items.push(area);
        self.items.len() - 1
    }

    /// Removes areas whose age is at least their own lifetime, keeping the
    /// order of the rest. Returns how many were removed.
    pub fn expire(&mut self, now: u32) -> usize {
        let before = self.items.len();
        self.items
            .retain(|a| now.wrapping_sub(a.since) < a.lifetime);
        before - self.items.len()
    }

    /// Index of the first area containing `pos`, if any.
    pub fn contains(&self, pos: Pos) -> Option<usize> {
        self.items.iter().position(|a| inside(a.pos, a.radius, pos))
    }

    /// Index of the first area of `kind` containing `pos`, if any.
    pub fn contains_kind(&self, pos: Pos, kind: u32) -> Option<usize> {
        self.items
            .iter()
            .position(|a| a.kind == kind && inside(a.pos, a.radius, pos))
    }

    /// True if `cell` is in some blocking area that holds neither `start`
    /// nor `goal`. All areas are checked.
    pub fn blocks(&self, cell: Pos, start: Pos, goal: Pos) -> bool {
        self.items.iter().any(|a| {
            a.blocking != 0
                && inside(a.pos, a.radius, cell)
                && !inside(a.pos, a.radius, start)
                && !inside(a.pos, a.radius, goal)
        })
    }

    /// Saturating sum of `strength` over non-blocking areas that contain
    /// `cell` but neither `start` nor `goal`.
    pub fn cost(&self, cell: Pos, start: Pos, goal: Pos) -> u32 {
        self.items
            .iter()
            .filter(|a| {
                a.blocking == 0
                    && inside(a.pos, a.radius, cell)
                    && !inside(a.pos, a.radius, start)
                    && !inside(a.pos, a.radius, goal)
            })
            .fold(0u32, |sum, a| sum.saturating_add(a.strength))
    }

    /// Extra cost of the move `from` -> `to` for a path search: `None` if
    /// `to` is blocked (see [`Areas::blocks`]), else `Some` of the cost of
    /// `to` (see [`Areas::cost`]). Only the destination cell is judged;
    /// `from` is accepted for symmetry with the search's callback and is
    /// not used yet. The cells are compared with the areas as they are: keep
    /// both in the same (cell) coordinates, or convert the cell first and
    /// call [`Areas::blocks`] and [`Areas::cost`] yourself.
    pub fn move_cost(&self, _from: Pos, to: Pos, start: Pos, goal: Pos) -> Option<u32> {
        if self.blocks(to, start, goal) {
            None
        } else {
            Some(self.cost(to, start, goal))
        }
    }

    /// The areas in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Area> {
        self.items.iter()
    }

    /// Number of areas.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if there are no areas.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Removes all areas.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}
