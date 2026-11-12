//! What one member holds: its current goal, whether it is postponed or
//! suspended, and the inputs its own decider reads.

use super::skill::SkillTable;
use super::squad::{Goal, plain_dist};
use crate::memory::{Metric, Pos};

/// A member's current [`Goal`] with its postpone state.
///
/// A goal can be postponed (the member does something more urgent) and
/// resumed; once it stays postponed for `give_up_ticks` it counts as
/// [`broken`](Self::broken). While suspended the clock does not run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GoalKeeper {
    goal: Option<Goal>,
    postponed_at: Option<u32>,
    suspended: bool,
}

impl GoalKeeper {
    /// An empty keeper: no goal.
    pub const fn new() -> Self {
        Self {
            goal: None,
            postponed_at: None,
            suspended: false,
        }
    }

    /// The current goal, if any.
    pub const fn goal(&self) -> Option<&Goal> {
        self.goal.as_ref()
    }

    /// Takes a new goal, replacing the old one and clearing the postpone
    /// state. `now` is unused by the state itself (the goal carries its own
    /// `issued_at`) and kept for symmetry with the other calls.
    pub fn set(&mut self, goal: Goal, _now: u32) {
        self.goal = Some(goal);
        self.postponed_at = None;
    }

    /// Drops the goal and the postpone state.
    pub fn clear(&mut self) {
        self.goal = None;
        self.postponed_at = None;
    }

    /// The member turns to something else for now. Already postponed: the
    /// first postpone time stays. Without a goal: nothing happens.
    pub fn postpone(&mut self, now: u32) {
        if self.goal.is_some() && self.postponed_at.is_none() {
            self.postponed_at = Some(now);
        }
    }

    /// The member is back on its goal: the postpone clock is reset.
    pub fn resume(&mut self, _now: u32) {
        self.postponed_at = None;
    }

    /// The member is out of play: [`broken`](Self::broken) is never true.
    pub fn suspend(&mut self) {
        self.suspended = true;
    }

    /// The member is back in play; a postponed goal's clock restarts at
    /// `now`.
    pub fn unsuspend(&mut self, now: u32) {
        self.suspended = false;
        if self.postponed_at.is_some() {
            self.postponed_at = Some(now);
        }
    }

    /// True once the goal has been postponed for at least `give_up_ticks`
    /// (wrap-safe); never while suspended.
    pub fn broken(&self, now: u32, give_up_ticks: u32) -> bool {
        match self.postponed_at {
            Some(at) if !self.suspended => now.wrapping_sub(at) >= give_up_ticks,
            _ => false,
        }
    }

    /// The member's decider inputs:
    /// `[has_goal, dist_to_goal, goal_age, signal_given, discipline]`.
    ///
    /// `dist_to_goal` is the distance from `me` to the goal's position in
    /// `metric` (for [`Metric::Euclid`] the rounded-down root), or -1
    /// without a goal or a position. `goal_age` is the ticks since the goal
    /// was issued (saturated). `has_goal` and `signal_given` are 0 or 1.
    pub fn inputs(
        &self,
        now: u32,
        me: Pos,
        metric: Metric,
        signal_given: bool,
        discipline: u32,
    ) -> [i32; 5] {
        let (has, dist, age) = match &self.goal {
            Some(g) => (
                1,
                g.pos.map_or(-1, |p| plain_dist(metric, me, p)),
                i32::try_from(now.wrapping_sub(g.issued_at)).unwrap_or(i32::MAX),
            ),
            None => (0, -1, 0),
        };
        [
            has,
            dist,
            age,
            i32::from(signal_given),
            i32::try_from(discipline).unwrap_or(i32::MAX),
        ]
    }
}

/// A member's discipline: the table's `hold_bonus` at the level of the
/// member's own `skill` (`skill / 64`).
pub fn discipline(skill: u8, table: &SkillTable) -> u32 {
    table.hold_bonus[SkillTable::level(skill)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ONE;
    use crate::group::{GoalKind, Start};

    fn goal(pos: Option<Pos>, issued_at: u32) -> Goal {
        Goal {
            kind: GoalKind::Move,
            pos,
            unchecked: false,
            then_pos: None,
            then_unchecked: false,
            target: None,
            start: Start::Now,
            slot: 0,
            plan: 1,
            issued_at,
            start_delay: 0,
        }
    }

    #[test]
    fn postpone_until_give_up_reports_broken() {
        let mut k = GoalKeeper::new();
        k.set(goal(Some([0, 0, 0]), 0), 0);
        assert!(!k.broken(100, 10));
        k.postpone(100);
        assert!(!k.broken(109, 10));
        assert!(k.broken(110, 10));
        k.postpone(105); // keeps the first time
        assert!(k.broken(110, 10));
    }

    #[test]
    fn resume_resets_postpone() {
        let mut k = GoalKeeper::new();
        k.set(goal(None, 0), 0);
        k.postpone(10);
        k.resume(15);
        assert!(!k.broken(1000, 10));
        k.postpone(20);
        assert!(!k.broken(29, 10));
        assert!(k.broken(30, 10));
        k.set(goal(None, 30), 30);
        assert!(!k.broken(1000, 10), "a new goal clears the postpone");
    }

    #[test]
    fn give_up_zero_breaks_at_once() {
        let mut k = GoalKeeper::new();
        k.set(goal(None, 0), 0);
        assert!(!k.broken(5, 0), "not postponed: never broken");
        k.postpone(5);
        assert!(k.broken(5, 0));
        k.suspend();
        assert!(!k.broken(5, 0));
    }

    #[test]
    fn suspended_never_broken() {
        let mut k = GoalKeeper::new();
        k.set(goal(None, 0), 0);
        k.postpone(0);
        k.suspend();
        assert!(!k.broken(1000, 10));
        k.unsuspend(1000);
        assert!(!k.broken(1009, 10));
        assert!(k.broken(1010, 10));
        let mut free = GoalKeeper::new();
        free.set(goal(None, 0), 0);
        free.suspend();
        free.unsuspend(5);
        assert!(!free.broken(1000, 10), "not postponed stays so");
    }

    #[test]
    fn inputs_layout() {
        let mut k = GoalKeeper::new();
        assert_eq!(
            k.inputs(5, [0, 0, 0], Metric::Euclid, false, 7),
            [0, -1, 0, 0, 7]
        );
        k.set(goal(Some([3, 4, 0]), 10), 10);
        assert_eq!(
            k.inputs(25, [0, 0, 0], Metric::Euclid, true, ONE),
            [1, 5, 15, 1, ONE as i32]
        );
        assert_eq!(
            k.inputs(25, [0, 0, 0], Metric::Manhattan, false, 0),
            [1, 7, 15, 0, 0]
        );
        k.set(goal(None, 10), 10);
        assert_eq!(
            k.inputs(12, [0, 0, 0], Metric::Euclid, false, 0),
            [1, -1, 2, 0, 0]
        );
        k.clear();
        assert!(k.goal().is_none());
        let t = SkillTable::default();
        assert_eq!(discipline(0, &t), ONE / 8);
        assert_eq!(discipline(200, &t), ONE / 2);
        assert_eq!(discipline(255, &t), ONE / 2);
    }

    #[test]
    fn give_up_wraps() {
        let mut k = GoalKeeper::new();
        k.set(goal(None, u32::MAX - 5), u32::MAX - 5);
        k.postpone(u32::MAX - 2);
        assert!(!k.broken(4, 10)); // age 7
        assert!(k.broken(7, 10)); // age 10
        assert_eq!(k.inputs(4, [0, 0, 0], Metric::Euclid, false, 0)[2], 10);
    }
}
