use crate::Pick;

/// Most rows a [`SkillTable`] can have.
pub const MAX_SKILL_ROWS: usize = 8;

/// What a squad's skill changes, in up to eight levels: the first `rows`
/// entries of every array are used, and a skill picks the row
/// `skill * rows / 256` (see [`SkillTable::level`]). With 4 rows these are
/// the quarters 0..=63 (weak) up to 192..=255 (elite).
///
/// The defaults are numbers to tune; a game can change any entry. Times are
/// in the caller's ticks (the defaults assume about 17 ticks per second).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SkillTable {
    /// How many rows are used, 1 ..= [`MAX_SKILL_ROWS`].
    pub rows: u8,
    /// How the tactic is chosen among the scored candidates.
    pub pick: [Pick; MAX_SKILL_ROWS],
    /// Ticks from making a plan until it becomes active.
    pub think_ticks: [u32; MAX_SKILL_ROWS],
    /// Most ranks a member's spot may be off the best one (picked by seed).
    pub spot_rank_error: [u8; MAX_SKILL_ROWS],
    /// Most ticks a member starts late (picked by seed per member).
    pub timing_spread: [u32; MAX_SKILL_ROWS],
    /// Bonus of the active plan over a new one, and the members' discipline
    /// ([`crate::ONE`] = 1.0): [`Squad::check`](super::Squad::check) replans only
    /// for a candidate scoring more than the active one times
    /// `1 + hold_bonus`.
    pub hold_bonus: [u32; MAX_SKILL_ROWS],
    /// Ticks a member may postpone its goal before it reports it broken.
    pub give_up_ticks: [u32; MAX_SKILL_ROWS],
}

impl SkillTable {
    /// The level (row index) for `skill`: `skill * rows / 256`, with `rows`
    /// clamped to 1 ..= [`MAX_SKILL_ROWS`] (for a table changed by hand).
    pub const fn level(&self, skill: u8) -> usize {
        let rows = if self.rows == 0 {
            1
        } else if self.rows as usize > MAX_SKILL_ROWS {
            MAX_SKILL_ROWS
        } else {
            self.rows as usize
        };
        skill as usize * rows / 256
    }

    /// How the tactic is chosen at `skill`'s level.
    pub const fn pick_for(&self, skill: u8) -> Pick {
        self.pick[self.level(skill)]
    }

    /// Ticks from making a plan until it is active, at `skill`'s level.
    pub const fn think_ticks_for(&self, skill: u8) -> u32 {
        self.think_ticks[self.level(skill)]
    }

    /// Most ranks a spot may be off the best one, at `skill`'s level.
    pub const fn spot_rank_error_for(&self, skill: u8) -> u8 {
        self.spot_rank_error[self.level(skill)]
    }

    /// Most ticks a member starts late, at `skill`'s level.
    pub const fn timing_spread_for(&self, skill: u8) -> u32 {
        self.timing_spread[self.level(skill)]
    }

    /// Bonus of the active plan over a new one, and the member's discipline
    /// at `skill`'s level.
    pub const fn hold_bonus_for(&self, skill: u8) -> u32 {
        self.hold_bonus[self.level(skill)]
    }

    /// Ticks a member may postpone its goal before it reports it broken,
    /// at `skill`'s level.
    pub const fn give_up_ticks_for(&self, skill: u8) -> u32 {
        self.give_up_ticks[self.level(skill)]
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

    #[test]
    fn skill_rows_scale_the_level() {
        let table = |rows| SkillTable {
            rows,
            ..SkillTable::default()
        };
        let t = table(4);
        assert!((0..=255u8).all(|s| t.level(s) == usize::from(s) / 64));
        let t = table(1);
        assert!((0..=255u8).all(|s| t.level(s) == 0));
        let t = table(8);
        assert_eq!((t.level(31), t.level(32), t.level(255)), (0, 1, 7));
        let t = table(3);
        assert_eq!([85, 86, 170, 171].map(|s| t.level(s)), [0, 1, 1, 2]);
        // A table changed by hand to rows 0 or 9 still clamps.
        assert_eq!(table(0).level(255), 0);
        assert_eq!(table(9).level(255), 7);
    }
}
