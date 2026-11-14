//! The rules object: game logic a squad calls at fixed points ("slots").
//!
//! A [`SquadRules`] has one method per slot. Every method has a default that
//! does what the core does without rules, so a game overrides only what it
//! needs; [`BuiltInRules`] is all defaults. The game owns the rules object
//! and lends it per call ([`Squad::plan_with`], [`Squad::check_with`]); the
//! squad never stores it ([`Squad::signal_with`],
//! [`Squad::reinstate_with`] and [`Squad::report_arrives_at_with`] lend it
//! too, for sending over a [`Channel::Custom`]). While a slot runs it sees
//! the squad only through a read-only [`SquadView`].
//!
//! **Call order.** Slots are called in this fixed order, never in hash
//! order, and no slot gets a random number:
//!
//! - In a plan ([`Squad::plan_with`]):
//!   1. [`SquadRules::keep_target`] for every known target, in knowledge
//!      order ([`Squad::targets`]).
//!   2. Per candidate, in candidate order (tactic order, then target
//!      order): [`SquadRules::columns`], then [`SquadRules::abort`] for the
//!      tactic's [`Abort::Custom`]s in list order (the aborts stop at the
//!      first that fires; a candidate whose abort fires scores 0).
//!   3. [`SquadRules::place`] for every [`Place::Custom`] of the chosen
//!      tactic: slots in order, a slot's `gather` before its `place`.
//!   4. [`SquadRules::member_cost`]: slot-major, then the active members
//!      that have the slot's role bits in group order; computed once per
//!      plan, before members are assigned.
//!   5. [`SquadRules::spot_score`]: members in assignment order; per member
//!      its gather spots, then its place spots; the free spots within the
//!      spot radius in index order.
//! - In a check ([`Squad::check_with`]): [`SquadRules::report`] for every
//!   report of a non-core kind, in arrival order; then the report actions
//!   of all of them are applied, in order (after the danger reports of the
//!   call, before any abort); then [`SquadRules::columns`] and
//!   [`SquadRules::abort`] of the active row; then (unless the signal was
//!   given or asked for) the candidates as in a plan (steps 1 and 2); a
//!   replan then runs a whole plan (steps 1 to 5); last
//!   [`SquadRules::trigger`] of a [`Trigger::Custom`], while the plan
//!   holds and the signal was not given. [`Squad::plan_with`] reads the
//!   reports the same way first (and gives an asked-for signal).
//! - [`SquadRules::channel_delay`] whenever an order goes over a
//!   [`Channel::Custom`]: on activation and on the signal per goal in the
//!   plan's goal order, on [`Squad::reinstate_with`] for that member. In
//!   the squad, channels time orders only; reports keep the `arrive_at`
//!   the caller gives ([`Squad::report`]), and
//!   [`Squad::report_arrives_at_with`] tells when one would arrive.
//! - A [`Crew`](super::Crew) lends the rules the same way
//!   ([`Crew::tick_with`](super::Crew::tick_with),
//!   [`Crew::reinstate_with`](super::Crew::reinstate_with),
//!   [`Crew::report_with`](super::Crew::report_with)) and times its
//!   members' reports over the same channel, built-in or custom. With a hex
//!   layout its slots see offset coordinates (contexts, view, report
//!   `pos`), and the positions they return are taken as offset.
//!
//! **Without rules.** A squad that needs rules ([`Squad::needs_rules`])
//! fails loudly instead of losing orders: [`Squad::plan`] and
//! [`Squad::check`] return [`PlanError::NeedsRules`];
//! [`Squad::orders_due`], [`Squad::arrived`] and [`Squad::reinstate`] do
//! not activate a due plan (the next call with rules does, its times still
//! counted from [`Plan::active_at`](super::Plan::active_at));
//! [`Squad::signal`] returns false; and no plain call sends over a
//! [`Channel::Custom`].
//!
//! **Defaults** (what [`BuiltInRules`] does):
//!
//! | Slot | Default |
//! |------|---------|
//! | `keep_target` | keep every target |
//! | `columns` | write nothing (the rule columns stay 0) |
//! | `place` | the leader's position, as [`Place::Here`] |
//! | `member_cost` | [`CostCtx::built_in`]: the metric distance from the member to the wish point |
//! | `spot_score` | [`SpotCtx::built_in`]: [`spot_score_builtin`] |
//! | `trigger` | never fires |
//! | `abort` | never fires |
//! | `channel_delay` | never arrives |
//! | `report` | drops the report (no action) |
//!
//! Costs and scores are `i128`, lower is better; ties go to the lower
//! member index or spot index.

use super::members::Member;
use super::orders::{Channel, Status};
use super::spots::{Spot, SpotPick};
use super::squad::{Goal, PlanError, PlanId, Squad};
use super::tactic::{Abort, Place, Tactic, Trigger};
use crate::memory::{Area, Fact, Message, Metric, Pos, distance};
use sagax_core::HexLayout;

/// Game logic a squad calls at fixed points; see the [module](self) for the
/// call order and the defaults.
///
/// Every method takes `&mut self`, so the rules may keep state between
/// calls. A slot must not call into the squad it is called from; it gets a
/// [`SquadView`] instead.
pub trait SquadRules {
    /// Slot 1, **targets**: whether the known target `fact` is a target for
    /// new plans. A dropped fact is no candidate, but still counts as a
    /// friend of other targets ([`IN_TARGET_FRIEND_DIST`](super::IN_TARGET_FRIEND_DIST),
    /// [`Place::Between`]) and as a threat for a tactic without a target;
    /// the extra blocks keep their [`Squad::targets`] index. It never ends
    /// the active plan: in a check the active (tactic, target) stays a
    /// candidate. Default: keep.
    fn keep_target(&mut self, view: &SquadView<'_>, fact: &Fact) -> bool {
        let _ = (view, fact);
        true
    }

    /// Slot 2, **columns**: fills the rule columns
    /// ([`SquadConfig::rule_columns`](super::SquadConfig::rule_columns), from
    /// [`Squad::rule_start`] on) of one candidate's row. `ctx.row` is the row
    /// before the rule columns, `out` exactly the rule columns (all 0 when
    /// called). Default: writes nothing.
    fn columns(&mut self, view: &SquadView<'_>, ctx: &RowCtx<'_>, out: &mut [i32]) {
        let _ = (view, ctx, out);
    }

    /// Slot 3, **place**: the wish point of [`Place::Custom`]`(id)`.
    /// Default: the leader's position.
    fn place(&mut self, id: u32, ctx: &PlaceCtx) -> Pos {
        let _ = id;
        ctx.leader
    }

    /// Slot 4, **member cost**: the cost of `ctx.member` taking slot
    /// `ctx.slot` (lower is better), or `None` when it cannot take it.
    /// Default: [`CostCtx::built_in`].
    fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
        Some(ctx.built_in)
    }

    /// Slot 5, **spot score**: the score of spot `ctx.index` for one member
    /// (lower is better), or `None` when it may not use it. The core still
    /// applies the spot radius, the taken spots and the skill's rank error.
    /// Default: [`SpotCtx::built_in`].
    fn spot_score(&mut self, ctx: &SpotCtx) -> Option<i128> {
        Some(ctx.built_in)
    }

    /// Slot 6, **trigger**: whether the [`Trigger::Custom`]`(id)` of the
    /// active plan fires at `now` (asked in [`Squad::check_with`] while the
    /// plan holds and its signal was not given). Default: never.
    fn trigger(&mut self, id: u32, view: &SquadView<'_>, now: u32) -> bool {
        let _ = (id, view, now);
        false
    }

    /// Slot 7, **abort**: whether the [`Abort::Custom`]`(id)` of tactic
    /// `ctx.tactic` fires on its row: the active plan's
    /// ([`RowCtx::active`] true) in a check, or a candidate's (true only for
    /// the active (tactic, target)), which then scores 0. Default: never.
    fn abort(&mut self, id: u32, view: &SquadView<'_>, ctx: &RowCtx<'_>) -> bool {
        let _ = (id, view, ctx);
        false
    }

    /// Slot 8, **channel delay**: the delay in ticks of the
    /// [`Channel::Custom`]`(id)`, or `None` when the message does not
    /// arrive; it arrives at `ctx.now + delay` (wrapping). Default: never
    /// arrives.
    fn channel_delay(&mut self, id: u32, ctx: &ChannelCtx) -> Option<u32> {
        let _ = (id, ctx);
        None
    }

    /// Slot 9, **report**: the leader reads a report of a kind the core
    /// does not know (every kind but 1, 2 and 4; a game's own start at
    /// [`REPORT_GAME_FIRST`](super::REPORT_GAME_FIRST)), in arrival order;
    /// `actions` takes what the squad should do about it. The core applies
    /// the actions of all reports of the call at one point, after the last
    /// report (see the [module](self)): a later report's `view` does not
    /// show an earlier one's actions yet. A [`ReportAction::Signal`] is
    /// given only if no abort fires and the target is not lost in the
    /// same call. Default: drops it.
    fn report(
        &mut self,
        view: &SquadView<'_>,
        msg: &Message,
        now: u32,
        actions: &mut ReportActions<'_>,
    ) {
        let _ = (view, msg, now, actions);
    }
}

/// The rules without any game logic: every slot does its default.
/// [`Squad::plan`] and [`Squad::check`] use it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BuiltInRules;

impl SquadRules for BuiltInRules {}

/// One scoring row, for [`SquadRules::columns`] and [`SquadRules::abort`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowCtx<'a> {
    /// Index of the tactic.
    pub tactic: usize,
    /// The candidate's target, as the leader knows it (`None` for a tactic
    /// without one, or a lost target).
    pub target: Option<Fact>,
    /// The row (for `columns`: up to [`Squad::rule_start`]).
    pub row: &'a [i32],
    /// The tick of the call.
    pub now: u32,
    /// True for the row of the active (tactic, target).
    pub active: bool,
}

/// What [`SquadRules::place`] knows of the slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlaceCtx {
    /// Index of the tactic.
    pub tactic: usize,
    /// Index of the slot.
    pub slot: usize,
    /// True for the slot's `gather` place, false for its `place`.
    pub gather: bool,
    /// The acting leader's position.
    pub leader: Pos,
    /// The group centre (mean of the active members).
    pub centre: Pos,
    /// What the slots face: the target, else the nearest threat (see
    /// [`Squad::plan`]); `None` with neither.
    pub face: Option<Pos>,
    /// The target's nearest other known target, if any.
    pub friend: Option<Pos>,
    /// The target's id, if the tactic has one.
    pub target: Option<u32>,
    /// The heading from `centre` to `face` ([`super::heading`]); 0 without
    /// a face.
    pub heading: u8,
    /// [`Slot::gather_dist`](super::Slot::gather_dist) for the gather place,
    /// else [`Slot::dist`](super::Slot::dist).
    pub dist: i32,
}

/// What [`SquadRules::member_cost`] knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostCtx {
    /// Index of the tactic.
    pub tactic: usize,
    /// Index of the slot.
    pub slot: usize,
    /// The member.
    pub member: Member,
    /// The slot's first wish point (its gather point, if it gathers).
    pub wish: Pos,
    /// The built-in cost: the metric distance from the member to `wish`
    /// (squared for [`Metric::Euclid`]).
    pub built_in: i128,
}

/// What [`SquadRules::spot_score`] knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpotCtx {
    /// Index of the tactic.
    pub tactic: usize,
    /// Index of the slot.
    pub slot: usize,
    /// True when ranking gather spots, false for place spots.
    pub gather: bool,
    /// The member that gets the spot.
    pub member: Member,
    /// The wish point.
    pub wish: Pos,
    /// The tag bits wanted (`slot.want_tags | place.tags()`).
    pub want_tags: u32,
    /// Index of the spot in the caller's list.
    pub index: usize,
    /// The spot.
    pub spot: Spot,
    /// The plan's target, if any.
    pub target: Option<Fact>,
    /// The built-in score ([`spot_score_builtin`]).
    pub built_in: i128,
}

/// What [`SquadRules::channel_delay`] knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChannelCtx {
    /// Who sends.
    pub sender: u32,
    /// Who receives.
    pub receiver: u32,
    /// The sender's position.
    pub from: Pos,
    /// The receiver's position.
    pub to: Pos,
    /// The plain distance between them in the metric (for
    /// [`Metric::Euclid`] the rounded-down root).
    pub distance: u32,
    /// When it is sent.
    pub now: u32,
    /// True for a report, false for an order.
    pub report: bool,
}

/// Most actions one report may hand back ([`ReportActions`]).
pub const MAX_REPORT_ACTIONS: usize = 4;

/// What the squad should do about a report (see [`SquadRules::report`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReportAction {
    /// Add this danger area.
    AddArea(Area),
    /// Give the signal of the active plan.
    Signal,
    /// Forget the target with this subject.
    Forget {
        /// The target's subject id.
        subject: u32,
    },
}

/// The actions of one report, at most [`MAX_REPORT_ACTIONS`]. The squad's
/// buffer is kept between calls, so handing actions back allocates nothing
/// once it has grown.
#[derive(Debug)]
pub struct ReportActions<'a> {
    out: &'a mut Vec<ReportAction>,
    start: usize,
}

impl<'a> ReportActions<'a> {
    /// Actions appended to `out` (for example to test a rules object on
    /// its own).
    pub fn new(out: &'a mut Vec<ReportAction>) -> Self {
        let start = out.len();
        Self { out, start }
    }

    fn push(&mut self, action: ReportAction) -> bool {
        let full = self.out.len() - self.start >= MAX_REPORT_ACTIONS;
        if !full {
            self.out.push(action);
        }
        !full
    }

    /// Adds a danger area (`since` becomes the tick of the call, as
    /// [`Areas::add`](crate::memory::Areas::add)); false when the report has
    /// its [`MAX_REPORT_ACTIONS`] already.
    pub fn add_area(&mut self, area: Area) -> bool {
        self.push(ReportAction::AddArea(area))
    }

    /// Gives the signal of the active plan, if it was not given yet; false
    /// when full.
    pub fn signal(&mut self) -> bool {
        self.push(ReportAction::Signal)
    }

    /// Forgets the target with this subject (of the target kind); false
    /// when full.
    pub fn forget(&mut self, subject: u32) -> bool {
        self.push(ReportAction::Forget { subject })
    }

    /// Changes every action of this report in place (a hex crew converts
    /// the positions its slot gave).
    pub(super) fn map(&mut self, f: impl Fn(ReportAction) -> ReportAction) {
        for a in &mut self.out[self.start..] {
            *a = f(*a);
        }
    }
}

/// The active plan, as a slot sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlanInfo {
    /// Its number.
    pub id: PlanId,
    /// Index of the tactic.
    pub tactic: usize,
    /// The target's id, if the tactic has one.
    pub target: Option<u32>,
    /// When it became active.
    pub since: u32,
    /// True once its signal was given.
    pub signal_given: bool,
}

/// A read-only look at a squad while a slot runs.
#[derive(Clone, Copy, Debug)]
pub struct SquadView<'a> {
    squad: &'a Squad,
    hex: Option<HexLayout>,
}

impl<'a> SquadView<'a> {
    /// A view of `squad`, positions as the squad keeps them.
    pub fn new(squad: &'a Squad) -> Self {
        Self { squad, hex: None }
    }

    /// The same view, every position it returns converted from axial to
    /// offset coordinates of `layout`.
    pub(super) fn with_hex(self, layout: HexLayout) -> Self {
        Self {
            hex: Some(layout),
            ..self
        }
    }

    fn out(&self, p: Pos) -> Pos {
        self.hex.map_or(p, |l| l.from_axial_pos(p))
    }

    /// The squad's metric.
    pub fn metric(&self) -> Metric {
        self.squad.metric
    }

    /// The squad's skill.
    pub fn skill(&self) -> u8 {
        self.squad.skill()
    }

    /// The acting leader's id (see [`Squad::leader`]).
    pub fn leader(&self) -> Option<u32> {
        self.squad.leader()
    }

    /// The group centre: the mean position of the active members.
    pub fn centre(&self) -> Option<Pos> {
        self.squad.centre().map(|p| self.out(p))
    }

    /// Every member in group order (also suspended and gone ones).
    pub fn members(&self) -> impl Iterator<Item = Member> + '_ {
        self.squad.group.iter().map(|m| Member {
            pos: self.out(m.pos),
            ..*m
        })
    }

    /// The member with this id.
    pub fn member(&self, id: u32) -> Option<Member> {
        self.members().find(|m| m.id == id)
    }

    /// Every known fact of the target kind, in knowledge order (also the
    /// ones the targets slot drops).
    pub fn targets(&self) -> impl Iterator<Item = Fact> + '_ {
        self.squad.targets().map(|f| Fact {
            pos: self.out(f.pos),
            ..*f
        })
    }

    /// The danger areas the leader remembers.
    pub fn areas(&self) -> impl Iterator<Item = Area> + '_ {
        self.squad.areas.iter().map(|a| Area {
            pos: self.out(a.pos),
            source: self.out(a.source),
            ..*a
        })
    }

    /// The tactic at `index`.
    pub fn tactic(&self, index: usize) -> Option<&'a Tactic> {
        self.squad.tactics.get(index)
    }

    /// The plan being carried out, if any.
    pub fn active_plan(&self) -> Option<PlanInfo> {
        self.squad.active.as_ref().map(|a| PlanInfo {
            id: a.plan.id,
            tactic: a.plan.tactic,
            target: a.plan.target,
            since: a.since,
            signal_given: a.signal_at.is_some(),
        })
    }

    /// The member's goal in the active plan, if it has one.
    pub fn goal(&self, member: u32) -> Option<Goal> {
        let a = self.squad.active.as_ref()?;
        let &(_, g) = a.plan.goals.iter().find(|(m, _)| *m == member)?;
        Some(Goal {
            pos: g.pos.map(|p| self.out(p)),
            then_pos: g.then_pos.map(|p| self.out(p)),
            ..g
        })
    }
}

/// The built-in spot score: `(miss << 100) | distance`, where `miss` is 1
/// when the spot lacks a bit of `want_tags` and `distance` the metric
/// distance from `wish` to the spot (x and y; squared for
/// [`Metric::Euclid`], always below 2^100). Ranked with the spot index as
/// the tie-break this orders exactly as [`pick_spot`](super::pick_spot):
/// spots with every wanted tag first, then the nearest, then the lower
/// index.
pub fn spot_score_builtin(wish: Pos, want_tags: u32, spot: &Spot, metric: Metric) -> i128 {
    let miss = spot.tags & want_tags != want_tags;
    let dx = i64::from(spot.pos[0]) - i64::from(wish[0]);
    let dy = i64::from(spot.pos[1]) - i64::from(wish[1]);
    (i128::from(miss) << 100) | distance(metric, dx, dy)
}

/// [`pick_spot`](super::pick_spot) through [`SquadRules::spot_score`]: the
/// free spots (not `taken[i]`) within `radius` of `ctx.wish` in index
/// order are scored (`ctx` gives everything but `index`, `spot` and
/// `built_in`), unusable ones dropped, the rest sorted by `(score, index)`
/// into `keys`; the result is rank `skip`, clamped to the last.
#[allow(clippy::too_many_arguments)]
pub(super) fn rank_spots(
    rules: &mut dyn SquadRules,
    keys: &mut Vec<(i128, usize)>,
    ctx: &SpotCtx,
    spots: &[Spot],
    taken: &[bool],
    skip: u8,
    metric: Metric,
    radius: i64,
) -> SpotPick {
    let limit = if metric == Metric::Euclid {
        i128::from(radius) * i128::from(radius)
    } else {
        i128::from(radius)
    };
    keys.clear();
    for (index, spot) in spots.iter().enumerate() {
        if radius < 0 || taken.get(index).copied().unwrap_or(false) {
            continue;
        }
        let dx = i64::from(spot.pos[0]) - i64::from(ctx.wish[0]);
        let dy = i64::from(spot.pos[1]) - i64::from(ctx.wish[1]);
        if distance(metric, dx, dy) > limit {
            continue;
        }
        let built_in = spot_score_builtin(ctx.wish, ctx.want_tags, spot, metric);
        let scored = rules.spot_score(&SpotCtx {
            index,
            spot: *spot,
            built_in,
            ..*ctx
        });
        if let Some(score) = scored {
            keys.push((score, index));
        }
    }
    keys.sort_unstable();
    match keys.len() {
        0 => SpotPick::Unchecked(ctx.wish),
        n => SpotPick::Spot(keys[usize::from(skip).min(n - 1)].1),
    }
}

impl Squad {
    /// True when the squad has parts only a rules object decides: a tactic
    /// with a [`Place::Custom`], a [`Trigger::Custom`] or an
    /// [`Abort::Custom`], a member on a [`Channel::Custom`], or rule columns
    /// ([`SquadConfig::rule_columns`](super::SquadConfig::rule_columns)).
    /// Then the plain calls fail loudly (see the [module](self)); use
    /// [`Squad::plan_with`], [`Squad::check_with`] and the other `_with`
    /// calls.
    pub fn needs_rules(&self) -> bool {
        let custom = |p: &Place| matches!(p, Place::Custom(_));
        self.rule_columns > 0
            || self.tactics.iter().any(|t| {
                matches!(t.trigger, Trigger::Custom(_))
                    || t.aborts.iter().any(|a| matches!(a, Abort::Custom(_)))
                    || t.slots
                        .iter()
                        .any(|s| custom(&s.place) || s.gather.as_ref().is_some_and(custom))
            })
            || self
                .channels
                .iter()
                .any(|(m, c)| matches!(c, Channel::Custom(_)) && self.group.index(*m).is_some())
    }

    /// [`Squad::plan`] with the game's rules; see the [rules](super::rules)
    /// for when each slot is called.
    pub fn plan_with(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        rules: &mut dyn SquadRules,
    ) -> Result<Option<PlanId>, PlanError> {
        self.validate()?;
        self.advance(now, Some(&mut *rules));
        if self.read_reports(now, rules) {
            self.give_signal(now, rules);
        }
        self.areas.expire(now);
        self.plan_now(now, extra, spots, seed, None, rules)
    }

    /// [`Squad::check`] with the game's rules; see the [rules](super::rules)
    /// for when each slot is called.
    pub fn check_with(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        rules: &mut dyn SquadRules,
    ) -> Result<Status, PlanError> {
        self.check_inner(now, extra, spots, seed, rules)
    }
}
