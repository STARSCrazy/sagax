use crate::Pick;

/// What a squad's skill changes, in four levels: index `skill / 64`
/// (0 = weak 0..=63, 1 = 64..=127, 2 = 128..=191, 3 = elite 192..=255).
///
/// The defaults are numbers to tune; a game can change any entry. Times are
/// in the caller's ticks (the defaults assume about 17 ticks per second).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SkillTable {
    /// How the tactic is chosen among the scored candidates.
    pub pick: [Pick; 4],
    /// Ticks from making a plan until it becomes active.
    pub think_ticks: [u32; 4],
    /// Most ranks a member's spot may be off the best one (picked by seed).
    pub spot_rank_error: [u8; 4],
    /// Most ticks a member starts late (picked by seed per member).
    pub timing_spread: [u32; 4],
    /// Bonus of the active plan over a new one, and the members' discipline
    /// ([`crate::ONE`] = 1.0): [`Squad::check`](super::Squad::check) replans only
    /// for a candidate scoring more than the active one times
    /// `1 + hold_bonus`.
    pub hold_bonus: [u32; 4],
    /// Ticks a member may postpone its goal before it reports it broken.
    pub give_up_ticks: [u32; 4],
}

impl SkillTable {
    /// The level (row index) for `skill`: `skill / 64`.
    pub const fn level(skill: u8) -> usize {
        (skill / 64) as usize
    }

    /// How the tactic is chosen at `skill`'s level.
    pub const fn pick_for(&self, skill: u8) -> Pick {
        self.pick[Self::level(skill)]
    }

    /// Ticks from making a plan until it is active, at `skill`'s level.
    pub const fn think_ticks_for(&self, skill: u8) -> u32 {
        self.think_ticks[Self::level(skill)]
    }

    /// Most ranks a spot may be off the best one, at `skill`'s level.
    pub const fn spot_rank_error_for(&self, skill: u8) -> u8 {
        self.spot_rank_error[Self::level(skill)]
    }

    /// Most ticks a member starts late, at `skill`'s level.
    pub const fn timing_spread_for(&self, skill: u8) -> u32 {
        self.timing_spread[Self::level(skill)]
    }

    /// Bonus of the active plan over a new one, and the member's discipline
    /// at `skill`'s level.
    pub const fn hold_bonus_for(&self, skill: u8) -> u32 {
        self.hold_bonus[Self::level(skill)]
    }

    /// Ticks a member may postpone its goal before it reports it broken,
    /// at `skill`'s level.
    pub const fn give_up_ticks_for(&self, skill: u8) -> u32 {
        self.give_up_ticks[Self::level(skill)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ONE;

    #[test]
    fn skill_for_reads_the_level() {
        let t = SkillTable::default();
        assert_eq!(
            [0, 64, 191, 255].map(|s| t.give_up_ticks_for(s)),
            [17, 34, 51, 68]
        );
        assert_eq!(t.think_ticks_for(255), 5);
        assert_eq!(t.hold_bonus_for(0), ONE / 8);
        assert_eq!(t.spot_rank_error_for(0), 2);
        assert_eq!(t.timing_spread_for(0), 17);
        assert_eq!(t.pick_for(255), Pick::Best);
        assert_eq!(t.pick_for(0), Pick::Weighted { top: 3 });
    }
}
