use crate::{ONE, Pick};

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
    /// ([`ONE`] = 1.0).
    pub hold_bonus: [u32; 4],
    /// Ticks a member may postpone its goal before it reports it broken.
    pub give_up_ticks: [u32; 4],
}

impl SkillTable {
    /// The level (row index) for `skill`: `skill / 64`.
    pub const fn level(skill: u8) -> usize {
        (skill / 64) as usize
    }
}

impl Default for SkillTable {
    /// Weak squads pick by chance among the top 3, think 2 s, miss spots by
    /// up to 2 ranks and start up to 1 s apart; elite squads pick the best,
    /// think 5 ticks, take the best spot and start together.
    fn default() -> Self {
        Self {
            pick: [
                Pick::Weighted { top: 3 },
                Pick::Weighted { top: 2 },
                Pick::Weighted { top: 2 },
                Pick::Best,
            ],
            think_ticks: [34, 20, 10, 5],
            spot_rank_error: [2, 1, 1, 0],
            timing_spread: [17, 10, 5, 0],
            hold_bonus: [ONE / 8, ONE / 4, ONE * 3 / 8, ONE / 2],
            give_up_ticks: [17, 34, 51, 68],
        }
    }
}
