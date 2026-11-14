//! The default numbers of the core configuration structs.

use super::super::crew::CrewConfig;
use super::super::skill::SkillTable;
use super::super::squad::{RoleCounts, SquadConfig};
use super::roles::{ROLE_AREA, ROLE_RANGED};
use crate::memory::Metric;
use crate::{ONE, Pick};

impl Default for SkillTable {
    /// Four rows (the other four copy the last). Weak squads pick by chance among the top 3, think 2 s, miss spots by
    /// up to 2 ranks and start up to 1 s apart; elite squads pick the best,
    /// think 5 ticks, take the best spot and start together. A new plan must
    /// beat the active one by 12.5 % (weak) up to 50 % (elite); members
    /// give a goal up after 1 s (weak) up to 4 s (elite) of postponing it.
    fn default() -> Self {
        Self {
            rows: 4,
            pick: [
                Pick::Weighted { top: 3 },
                Pick::Weighted { top: 2 },
                Pick::Weighted { top: 2 },
                Pick::Best,
                Pick::Best,
                Pick::Best,
                Pick::Best,
                Pick::Best,
            ],
            think_ticks: [34, 20, 10, 5, 5, 5, 5, 5],
            spot_rank_error: [2, 1, 1, 0, 0, 0, 0, 0],
            timing_spread: [17, 10, 5, 0, 0, 0, 0, 0],
            hold_bonus: [
                ONE / 8,
                ONE / 4,
                ONE * 3 / 8,
                ONE / 2,
                ONE / 2,
                ONE / 2,
                ONE / 2,
                ONE / 2,
            ],
            give_up_ticks: [17, 34, 51, 68, 68, 68, 68, 68],
        }
    }
}

impl Default for SquadConfig {
    /// 8 members, 32 facts, 8 areas, 16 inbox reports, 8 danger reports,
    /// [`Metric::Euclid`], the roles `[ROLE_RANGED, ROLE_AREA]`, the
    /// default [`SkillTable`] and no rule columns.
    fn default() -> Self {
        Self {
            members: 8,
            memory: 32,
            areas: 8,
            inbox: 16,
            dangers: 8,
            metric: Metric::Euclid,
            roles: RoleCounts::new(&[ROLE_RANGED, ROLE_AREA]).unwrap_or(RoleCounts::NONE),
            skill_table: SkillTable::default(),
            rule_columns: 0,
        }
    }
}

impl Default for CrewConfig {
    /// [`SquadConfig::default`], arrive radius 0, no hex layout.
    fn default() -> Self {
        Self {
            squad: SquadConfig::default(),
            arrive_radius: 0,
            hex_layout: None,
        }
    }
}
