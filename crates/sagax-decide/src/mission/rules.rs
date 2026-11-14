//! The rules object of a mission and the contexts its slots see.

use super::types::Motive;
use crate::memory::Pos;

/// What [`MissionRules::fail`] sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FailCtx {
    /// The time of the tick.
    pub now: u32,
    /// Where the character stands.
    pub pos: Pos,
    /// The index of the step the condition belongs to; `None` for a
    /// condition of the whole plan.
    pub step: Option<usize>,
    /// The mission's motive.
    pub motive: Motive,
}

/// What [`MissionRules::columns`] and [`MissionRules::score`] see: one
/// candidate plan of a replan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlanCtx {
    /// The time of the tick.
    pub now: u32,
    /// Where the character stands.
    pub pos: Pos,
    /// The candidate's decider option (playbook slot, or
    /// [`Mission::default_option`](super::Mission::default_option) or
    /// [`Mission::widen_option`](super::Mission::widen_option)).
    pub option: u32,
    /// The candidate is a widening plan.
    pub widening: bool,
    /// Its conditions hold now and its first position can be filled.
    pub feasible: bool,
    /// Its first position, if it has one that can be filled.
    pub first: Option<Pos>,
    /// The mission's motive.
    pub motive: Motive,
}

/// Game logic a mission calls at fixed points ("slots").
///
/// A [`MissionRules`] has one method per slot. Every method has a default
/// that does what the mission does without rules, so a game overrides only
/// what it needs; [`BuiltInRules`] is all defaults. The game owns the rules
/// object and lends it per tick ([`Mission::tick_with`]); the mission never
/// stores it.
///
/// **Call order.** Slots are called in this fixed order and no slot gets a
/// random number:
///
/// 1. Each tick while a plan runs (also while a plan that ended in a
///    `Wander` step wanders, then for the plan's conditions only):
///    [`MissionRules::fail`] for the running step's [`Fail::Custom`]s, then
///    for the plan's, in list order. The checks stop at the first condition
///    that fires (built-in or custom).
/// 2. On a replan, per candidate in option order (playbook slots, then the
///    default plan, then the widening plan): [`MissionRules::fail`] for the
///    `Fail::Custom`s of its first step, then of the plan (stopping at the
///    first that fires), then [`MissionRules::columns`], then
///    [`MissionRules::score`].
///
/// With a hex layout every position in a context is in offset coordinates.
///
/// **Defaults** (what [`BuiltInRules`] does):
///
/// | Slot | Default |
/// |------|---------|
/// | `fail` | never fires |
/// | `columns` | write nothing (the game's columns stay 0) |
/// | `score` | keep the built-in score |
///
/// [`Mission::tick_with`]: super::Mission::tick_with
/// [`Fail::Custom`]: super::Fail::Custom
pub trait MissionRules {
    /// Whether the game's condition `id` ([`Fail::Custom`](super::Fail::Custom))
    /// holds now, so the plan breaks. Default: never.
    fn fail(&mut self, ctx: &FailCtx, id: u32) -> bool {
        let _ = (ctx, id);
        false
    }

    /// Fills the game's columns of a candidate's row (`out` has
    /// [`MissionConfig::extra_columns`](super::MissionConfig::extra_columns)
    /// entries, all 0 before the call). Default: writes nothing.
    fn columns(&mut self, ctx: &PlanCtx, out: &mut [i32]) {
        let _ = (ctx, out);
    }

    /// Replaces a candidate's score (`built_in` is the decider's score,
    /// [`ONE`](crate::ONE) = 1.0); `Some(0)` vetoes it. The answer is final,
    /// also for a candidate that is not feasible. Default: `None`, keep the
    /// built-in score.
    fn score(&mut self, ctx: &PlanCtx, built_in: u32) -> Option<u32> {
        let _ = (ctx, built_in);
        None
    }
}

/// The rules without any game logic: every slot keeps its default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BuiltInRules;

impl MissionRules for BuiltInRules {}
