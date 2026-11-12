use crate::memory::{Metric, Pos, ZeroCapacity, distance};

/// What a member currently does for the group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MemberState {
    /// Takes part.
    Active = 0,
    /// Temporarily out (busy elsewhere); stays in the group.
    Suspended = 1,
    /// Dead or lost; stays listed until removed.
    Gone = 2,
}

/// One member of a [`Group`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    /// The game's id of the character.
    pub id: u32,
    /// Where the member is.
    pub pos: Pos,
    /// Bit set of roles the member can fill (game-defined).
    pub roles: u32,
    /// How good the member is (game-defined).
    pub skill: u8,
    /// Whether the member takes part.
    pub state: MemberState,
}

/// A bounded list of members, in the order they were added.
#[derive(Clone, Debug)]
pub struct Group {
    members: Vec<Member>,
    capacity: usize,
}

impl Group {
    /// An empty group for at most `capacity` members.
    pub fn new(capacity: usize) -> Result<Self, ZeroCapacity> {
        if capacity == 0 {
            return Err(ZeroCapacity);
        }
        Ok(Self {
            members: Vec::with_capacity(capacity),
            capacity,
        })
    }

    /// Adds a member at the end; returns its index, or `None` when the group
    /// is full or the id is already present.
    pub fn add(&mut self, member: Member) -> Option<usize> {
        if self.members.len() >= self.capacity || self.index(member.id).is_some() {
            return None;
        }
        self.members.push(member);
        Some(self.members.len() - 1)
    }

    /// Removes the member with this id; the others keep their order.
    pub fn remove(&mut self, id: u32) -> bool {
        match self.index(id) {
            Some(i) => {
                self.members.remove(i);
                true
            }
            None => false,
        }
    }

    /// Index of the member with this id.
    pub fn index(&self, id: u32) -> Option<usize> {
        self.members.iter().position(|m| m.id == id)
    }

    /// The member at index `i`.
    pub fn get(&self, i: usize) -> Option<&Member> {
        self.members.get(i)
    }

    /// Moves a member; false when the id is unknown.
    pub fn set_pos(&mut self, id: u32, pos: Pos) -> bool {
        self.update(id, |m| m.pos = pos)
    }

    /// Takes a member out of play for now; false when the id is unknown.
    pub fn suspend(&mut self, id: u32) -> bool {
        self.update(id, |m| m.state = MemberState::Suspended)
    }

    /// Puts a member back into play; false when the id is unknown.
    pub fn resume(&mut self, id: u32) -> bool {
        self.update(id, |m| m.state = MemberState::Active)
    }

    /// Marks a member as gone; false when the id is unknown.
    pub fn set_gone(&mut self, id: u32) -> bool {
        self.update(id, |m| m.state = MemberState::Gone)
    }

    /// All members in order, whatever their state.
    pub fn iter(&self) -> impl Iterator<Item = &Member> {
        self.members.iter()
    }

    /// Number of members, whatever their state.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// True when the group has no members.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// The active members with their indexes, in order.
    pub fn active(&self) -> impl Iterator<Item = (usize, &Member)> {
        self.members
            .iter()
            .enumerate()
            .filter(|(_, m)| m.state == MemberState::Active)
    }

    fn update(&mut self, id: u32, f: impl FnOnce(&mut Member)) -> bool {
        match self.members.iter_mut().find(|m| m.id == id) {
            Some(m) => {
                f(m);
                true
            }
            None => false,
        }
    }
}

/// A character the game offers as a candidate to join a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nearby {
    /// The game's id of the character.
    pub id: u32,
    /// The faction the character belongs to.
    pub faction: u32,
    /// Where the character is.
    pub pos: Pos,
    /// Whether the character is free (not already in a group).
    pub free: bool,
}

/// Picks up to `max` candidates of `faction` within `radius` of `initiator`
/// (inclusive, in `metric`; for [`Metric::Euclid`] the squared distance is
/// compared with `radius²`), nearest first, ties to the lower id. With
/// `free_only` the candidates that are not free are skipped. Only x and y
/// count. The ids go to `out` (cleared first); a negative `radius` reaches
/// nobody.
#[allow(clippy::too_many_arguments)]
pub fn gather(
    initiator: Pos,
    faction: u32,
    radius: i64,
    metric: Metric,
    free_only: bool,
    candidates: &[Nearby],
    out: &mut Vec<u32>,
    max: usize,
) {
    out.clear();
    if radius < 0 || max == 0 {
        return;
    }
    let limit = if metric == Metric::Euclid {
        i128::from(radius) * i128::from(radius)
    } else {
        i128::from(radius)
    };
    let mut hits: Vec<(i128, u32)> = Vec::with_capacity(candidates.len());
    for c in candidates {
        if c.faction != faction || (free_only && !c.free) {
            continue;
        }
        let dx = i64::from(c.pos[0]) - i64::from(initiator[0]);
        let dy = i64::from(c.pos[1]) - i64::from(initiator[1]);
        let d = distance(metric, dx, dy);
        if d <= limit {
            hits.push((d, c.id));
        }
    }
    hits.sort_unstable();
    out.extend(hits.iter().take(max).map(|&(_, id)| id));
}
