//! What one member holds: its current goal, whether it is postponed or
//! suspended, and the inputs its own decider reads.

use super::squad::{Goal, plain_dist};
use crate::memory::{Metric, Pos};
use crate::{Candidate, Consideration, Curve, Decider};

/// Column of [`GoalKeeper::inputs`]: 1 with a goal, else 0.
pub const IN_HAS_GOAL: usize = 0;
/// Column: distance to the goal's position, or -1 without one.
pub const IN_DIST: usize = 1;
/// Column: ticks since the goal was issued (saturated).
pub const IN_AGE: usize = 2;
/// Column: 1 once the group signal was given, else 0.
pub const IN_SIGNAL: usize = 3;
/// Column: the member's discipline.
pub const IN_DISCIPLINE: usize = 4;
/// First column a member's own inputs may use after the five above.
pub const IN_EXTRA: usize = 5;
/// Distance at which the goal option of [`GoalKeeper::add_goal_option`]
/// scores 0.
pub const GOAL_OPTION_RANGE: i32 = 64;

/// The keeper's input columns by name (see [`GoalKeeper::inputs`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct KeeperInputs {
    /// 1 with a goal, else 0.
    pub has_goal: i32,
    /// Distance to the goal's position, or -1 without one.
    pub dist: i32,
    /// Ticks since the goal was issued (saturated).
    pub age: i32,
    /// 1 once the group signal was given, else 0.
    pub signal: i32,
    /// The member's discipline.
    pub discipline: i32,
}

impl KeeperInputs {
    /// The columns in order, as a row for a [`Decider`].
    pub const fn as_row(&self) -> [i32; 5] {
        [
            self.has_goal,
            self.dist,
            self.age,
            self.signal,
            self.discipline,
        ]
    }
}

impl From<[i32; 5]> for KeeperInputs {
    fn from(row: [i32; 5]) -> Self {
        Self {
            has_goal: row[IN_HAS_GOAL],
            dist: row[IN_DIST],
            age: row[IN_AGE],
            signal: row[IN_SIGNAL],
            discipline: row[IN_DISCIPLINE],
        }
    }
}

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
    pub fn set(&mut self, _now: u32, goal: Goal) {
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
    pub fn goal_postponed(&mut self, now: u32) {
        if self.goal.is_some() && self.postponed_at.is_none() {
            self.postponed_at = Some(now);
        }
    }

    /// The member is back on its goal: the postpone clock is reset.
    pub fn goal_won(&mut self, _now: u32) {
        self.postponed_at = None;
    }

    /// Adds the ready-made "keep my goal" option to a member's `decider` and
    /// returns its number. It scores `weight` times a step on
    /// [`IN_HAS_GOAL`] and a falling curve on [`IN_DIST`] (full at 0, 0 at
    /// [`GOAL_OPTION_RANGE`]). The member's rows start with the five
    /// keeper columns ([`KeeperInputs::as_row`]); its own inputs follow
    /// from [`IN_EXTRA`].
    pub fn add_goal_option(decider: &mut Decider, weight: u32) -> u32 {
        let option = decider.add_option(weight);
        // The option was just added, so both calls find it.
        let _ = decider.add_consideration(
            option,
            Consideration::new(IN_HAS_GOAL as u32, Curve::Step, 1, 1),
        );
        let _ = decider.add_consideration(
            option,
            Consideration::new(IN_DIST as u32, Curve::Inverse, 0, GOAL_OPTION_RANGE),
        );
        option
    }

    /// Tells `decider` that the goal option (`option`, from
    /// [`add_goal_option`](Self::add_goal_option)) is the action going on
    /// while a goal is held and not postponed: it gets `discipline` as its
    /// bonus. Otherwise no action is current.
    pub fn set_current(&self, decider: &mut Decider, option: u32, discipline: u32) {
        let current =
            (self.goal.is_some() && self.postponed_at.is_none()).then(|| Candidate::new(option, 0));
        decider.set_current(current, discipline);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{GoalKind, SkillTable, Start};
    use crate::{Candidate, Decider, ONE};

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
    fn goal_postponed_until_give_up() {
        let mut k = GoalKeeper::new();
        k.set(0, goal(Some([0, 0, 0]), 0));
        assert!(!k.broken(100, 10));
        k.goal_postponed(100);
        assert!(!k.broken(109, 10));
        assert!(k.broken(110, 10));
        k.goal_postponed(105); // keeps the first time
        assert!(k.broken(110, 10));
    }

    #[test]
    fn goal_won_resets_postpone() {
        let mut k = GoalKeeper::new();
        k.set(0, goal(None, 0));
        k.goal_postponed(10);
        k.goal_won(15);
        assert!(!k.broken(1000, 10));
        k.goal_postponed(20);
        assert!(!k.broken(29, 10));
        assert!(k.broken(30, 10));
        k.set(30, goal(None, 30));
        assert!(!k.broken(1000, 10), "a new goal clears the postpone");
    }

    #[test]
    fn give_up_zero_breaks_at_once() {
        let mut k = GoalKeeper::new();
        k.set(0, goal(None, 0));
        assert!(!k.broken(5, 0), "not postponed: never broken");
        k.goal_postponed(5);
        assert!(k.broken(5, 0));
        k.suspend();
        assert!(!k.broken(5, 0));
    }

    #[test]
    fn suspended_never_broken() {
        let mut k = GoalKeeper::new();
        k.set(0, goal(None, 0));
        k.goal_postponed(0);
        k.suspend();
        assert!(!k.broken(1000, 10));
        k.unsuspend(1000);
        assert!(!k.broken(1009, 10));
        assert!(k.broken(1010, 10));
        let mut free = GoalKeeper::new();
        free.set(0, goal(None, 0));
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
        k.set(10, goal(Some([3, 4, 0]), 10));
        assert_eq!(
            k.inputs(25, [0, 0, 0], Metric::Euclid, true, ONE),
            [1, 5, 15, 1, ONE as i32]
        );
        assert_eq!(
            k.inputs(25, [0, 0, 0], Metric::Manhattan, false, 0),
            [1, 7, 15, 0, 0]
        );
        k.set(10, goal(None, 10));
        assert_eq!(
            k.inputs(12, [0, 0, 0], Metric::Euclid, false, 0),
            [1, -1, 2, 0, 0]
        );
        k.clear();
        assert!(k.goal().is_none());
        let t = SkillTable::default();
        assert_eq!(t.hold_bonus_for(0), ONE / 8);
        assert_eq!(t.hold_bonus_for(200), ONE / 2);
        assert_eq!(t.hold_bonus_for(255), ONE / 2);
    }

    #[test]
    fn give_up_wraps() {
        let mut k = GoalKeeper::new();
        k.set(u32::MAX - 5, goal(None, u32::MAX - 5));
        k.goal_postponed(u32::MAX - 2);
        assert!(!k.broken(4, 10)); // age 7
        assert!(k.broken(7, 10)); // age 10
        assert_eq!(k.inputs(4, [0, 0, 0], Metric::Euclid, false, 0)[2], 10);
    }

    #[test]
    fn keeper_columns_are_named() {
        assert_eq!(
            [
                IN_HAS_GOAL,
                IN_DIST,
                IN_AGE,
                IN_SIGNAL,
                IN_DISCIPLINE,
                IN_EXTRA
            ],
            [0, 1, 2, 3, 4, 5]
        );
        let mut k = GoalKeeper::new();
        k.set(10, goal(Some([3, 4, 0]), 10));
        let row = k.inputs(25, [0, 0, 0], Metric::Euclid, true, ONE);
        let named = KeeperInputs::from(row);
        assert_eq!(named.as_row(), row);
        assert_eq!(
            (named.has_goal, named.dist, named.age, named.signal),
            (1, 5, 15, 1)
        );
        assert_eq!(named.discipline, ONE as i32);
    }

    fn goal_decider() -> Decider {
        let mut d = Decider::new();
        assert_eq!(GoalKeeper::add_goal_option(&mut d, ONE), 0);
        d
    }

    #[test]
    fn goal_option_scores_has_goal_and_closeness() {
        let d = goal_decider();
        let score = |row: [i32; 5]| d.score(Candidate::new(0, 0), &row).unwrap();
        assert_eq!(score([1, 0, 0, 0, 0]), ONE);
        assert_eq!(score([0, 0, 0, 0, 0]), 0);
        assert_eq!(score([1, GOAL_OPTION_RANGE, 0, 0, 0]), 0);
        assert!(score([1, 10, 0, 0, 0]) < ONE);
    }

    #[test]
    fn set_current_gives_discipline() {
        let mut d = goal_decider();
        let mut k = GoalKeeper::new();
        k.set(0, goal(Some([0, 0, 0]), 0));
        k.set_current(&mut d, 0, ONE / 2);
        let held = d.score(Candidate::new(0, 0), &[1, 0, 0, 0, 0]).unwrap();
        assert_eq!(held, 98302);
        k.goal_postponed(5);
        k.set_current(&mut d, 0, ONE / 2);
        let postponed = d.score(Candidate::new(0, 0), &[1, 0, 0, 0, 0]).unwrap();
        assert_eq!(postponed, 65535);
        k.clear();
        k.set_current(&mut d, 0, ONE / 2);
        assert_eq!(
            d.score(Candidate::new(0, 0), &[1, 0, 0, 0, 0]).unwrap(),
            65535
        );
    }
}
