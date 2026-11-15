//! Replanning: the playbook, the default and widening plans, and how the
//! mission scores them when a plan breaks or ends.

use super::{Activity, Mission, View};
use crate::memory::{Fact, Metric, Pos, distance};
use crate::mission::rules::{MissionRules, PlanCtx};
use crate::mission::types::{
    Action, Event, MissionConfig, MissionError, MotiveKind, Plan, Step, StepKind, Where,
};
use crate::{Apply, Candidate, Consideration, Curve, Decider, ONE, choose, mix};

/// Row column: 1 when the candidate's conditions hold now and its first
/// position can be filled, else 0.
pub const IN_FEASIBLE: usize = 0;
/// Row column: distance from the character to the candidate's first
/// position in the mission's metric (Euclid: the rounded-down root), at
/// most [`DIST_CAP`]; 0 when no step names a position.
pub const IN_DIST: usize = 1;
/// Row column: the candidate's preference, 0..=[`ONE`].
pub const IN_PREF: usize = 2;
/// Row column: 1 for a widening plan, else 0.
pub const IN_WIDEN: usize = 3;
/// Row column: age in ticks of the freshest fact the candidate relies on
/// (the subjects its steps name), at most [`KNOWN_CAP`]; 0 when it names
/// none, [`KNOWN_CAP`] when none of them is known.
pub const IN_KNOWN: usize = 4;
/// The first of the game's columns
/// ([`MissionConfig::extra_columns`] of them).
pub const IN_EXTRA: usize = 5;
/// Largest value of [`IN_DIST`].
pub const DIST_CAP: i32 = 1024;
/// Largest value of [`IN_KNOWN`].
pub const KNOWN_CAP: i32 = 65535;
/// The preference ([`IN_PREF`]) of the default plan: a playbook plan of a
/// higher preference wins over it by the built-in considerations.
pub const DEFAULT_PREFERENCE: u32 = ONE / 2;
/// The preference ([`IN_PREF`]) of the built-in widening plan: above the
/// default plan, so a character that may widen calls for help first.
pub const WIDEN_PREFERENCE: u32 = ONE / 4 * 3;

/// Most steps of a default plan.
const MAX_DEFAULT_STEPS: usize = 3;
/// Most steps of a built-in plan (the widening plan: a call, then the
/// default plan).
pub(super) const MAX_BUILT_IN_STEPS: usize = MAX_DEFAULT_STEPS + 1;

/// One playbook slot.
#[derive(Clone, Debug)]
struct Slot {
    /// The plan (capacity `max_steps`); valid while `used`.
    plan: Plan,
    preference: u32,
    widening: bool,
    used: bool,
}

/// The playbook, the built-in plans, the decider and the scratch rows of a
/// replan; all reserved at creation.
#[derive(Clone, Debug)]
pub(super) struct Replan {
    playbook: Vec<Slot>,
    /// The default plan, built at each replan from what the character
    /// knows; valid while `has_default`.
    default_plan: Plan,
    has_default: bool,
    /// The built-in widening plan; valid while `has_widen`.
    widen_plan: Plan,
    has_widen: bool,
    decider: Decider,
    /// One row per candidate.
    rows: Vec<i32>,
    scores: Vec<u32>,
    /// The option of each candidate.
    options: Vec<u32>,
    /// Scratch space of the pick.
    order: Vec<usize>,
}

impl Replan {
    pub(super) fn new(config: &MissionConfig) -> Self {
        let slots = config.max_playbook;
        let options = slots + 2;
        let mut decider = Decider::new();
        for _ in 0..options {
            let o = decider.add_option(ONE);
            let feasible = Consideration::new(IN_FEASIBLE as u32, Curve::Step, 1, 1);
            let preference = Consideration::new(IN_PREF as u32, Curve::Linear, 0, ONE as i32);
            let _ = decider.add_weighted(o, feasible, ONE, Apply::Multiply);
            let _ = decider.add_weighted(o, preference, ONE, Apply::Multiply);
        }
        // Not `vec![slot; n]`: a cloned plan would lose its capacity.
        let slot = || Slot {
            plan: Plan::with_capacity(config.max_steps),
            preference: 0,
            widening: false,
            used: false,
        };
        Self {
            playbook: (0..slots).map(|_| slot()).collect(),
            default_plan: Plan::with_capacity(MAX_DEFAULT_STEPS),
            has_default: false,
            widen_plan: Plan::with_capacity(MAX_BUILT_IN_STEPS),
            has_widen: false,
            decider,
            rows: Vec::with_capacity(options * (IN_EXTRA + config.extra_columns)),
            scores: Vec::with_capacity(options),
            options: Vec::with_capacity(options),
            order: Vec::with_capacity(options),
        }
    }

    /// Capacities of all storage, to check that nothing grows.
    #[cfg(test)]
    pub(super) fn capacities(&self) -> Vec<usize> {
        let mut c = vec![
            self.playbook.capacity(),
            self.default_plan.capacity(),
            self.widen_plan.capacity(),
            self.rows.capacity(),
            self.scores.capacity(),
            self.options.capacity(),
            self.order.capacity(),
        ];
        c.extend(self.playbook.iter().map(|s| s.plan.capacity()));
        c
    }
}

impl Mission {
    /// Copies `plan` into the first free playbook slot and returns the slot,
    /// which is its decider option. `preference` (0..=[`ONE`], more is cut)
    /// is its [`IN_PREF`] column; a `widening` plan is a candidate only for
    /// a character that may widen. Errors on an empty plan, one with more
    /// than `max_steps` steps, or a full playbook.
    pub fn add_playbook(
        &mut self,
        plan: &Plan,
        preference: u32,
        widening: bool,
    ) -> Result<u32, MissionError> {
        let len = plan.steps().len();
        if len == 0 {
            return Err(MissionError::EmptyPlan);
        }
        if len > self.config.max_steps {
            return Err(MissionError::TooManySteps);
        }
        let (i, slot) = self
            .replan
            .playbook
            .iter_mut()
            .enumerate()
            .find(|(_, s)| !s.used)
            .ok_or(MissionError::PlaybookFull)?;
        slot.plan.copy_from(plan);
        slot.preference = preference.min(ONE);
        slot.widening = widening;
        slot.used = true;
        Ok(i as u32)
    }

    /// Empties every playbook slot; the decider and the considerations on
    /// its options stay. A running playbook plan runs on (when it ends, a
    /// plan added to its slot meanwhile is still a candidate).
    pub fn clear_playbook(&mut self) {
        for slot in &mut self.replan.playbook {
            slot.used = false;
        }
        if self.running_option < Some(self.default_option()) {
            self.running_option = None;
        }
    }

    /// The decider option of the default plan (`max_playbook`).
    pub fn default_option(&self) -> u32 {
        self.config.max_playbook as u32
    }

    /// The decider option of the built-in widening plan (`max_playbook + 1`).
    pub fn widen_option(&self) -> u32 {
        self.config.max_playbook as u32 + 1
    }

    /// The decider that scores the candidate plans of a replan. Option `i`
    /// below `max_playbook` is playbook slot `i`, then come
    /// [`Mission::default_option`] and [`Mission::widen_option`]. Each
    /// option has two built-in considerations, both [`Apply::Multiply`]: a
    /// step on [`IN_FEASIBLE`] and [`IN_PREF`] linear over 0..=[`ONE`].
    /// A row has [`IN_EXTRA`] built-in columns, then the game's.
    pub fn decider(&self) -> &Decider {
        &self.replan.decider
    }

    /// The decider to add considerations, change the combine and so on (see
    /// [`Mission::decider`]). A consideration on an input beyond the row
    /// makes its candidate score 0; options added beyond the built-in ones
    /// are never scored.
    pub fn decider_mut(&mut self) -> &mut Decider {
        &mut self.replan.decider
    }

    /// The plan broke or ended, or there is none yet: replan, or fall back.
    /// `excluded` is the playbook slot of a plan that just ended or broke;
    /// it is no candidate now. Widening plans are candidates only on a real
    /// break (`broke`: a fail condition or an impossible step) of a
    /// character that may widen.
    pub(super) fn replace_plan(
        &mut self,
        view: View<'_>,
        excluded: Option<u32>,
        broke: bool,
        rules: &mut dyn MissionRules,
    ) -> Action {
        if !self.leeway.replan {
            return self.hold_here(view);
        }
        let choice = self.pick(view, excluded, broke, rules);
        match self.take(choice, view) {
            Some(action) => action,
            None => self.hold_here(view),
        }
    }

    /// Holding after a fallback, a character that may replan looks again:
    /// the action of the plan it takes, or `None` to hold on (no event).
    /// No widening plans: this is no real break.
    pub(super) fn retry_fallback(
        &mut self,
        view: View<'_>,
        rules: &mut dyn MissionRules,
    ) -> Option<Action> {
        let choice = self.pick(view, None, false, rules);
        self.take(choice, view)
    }

    /// Starts what [`Mission::pick`] chose: the winner
    /// ([`Event::Replanned`]), else the default plan if it is feasible
    /// ([`Event::FellBack`]); `None` when neither.
    fn take(&mut self, choice: (Option<u32>, bool), view: View<'_>) -> Option<Action> {
        match choice {
            (Some(option), _) => {
                self.emit(Event::Replanned { option });
                Some(self.start_option(option, view))
            }
            (None, true) => {
                self.emit(Event::FellBack);
                Some(self.start_option(self.default_option(), view))
            }
            (None, false) => None,
        }
    }

    /// Scores the candidates and picks one: the winning option (none when
    /// every candidate scores 0) and whether the default plan is feasible.
    fn pick(
        &mut self,
        view: View<'_>,
        excluded: Option<u32>,
        broke: bool,
        rules: &mut dyn MissionRules,
    ) -> (Option<u32>, bool) {
        let widen = broke && self.leeway.widen;
        self.build_defaults(view, widen);
        let mut rows = std::mem::take(&mut self.replan.rows);
        let mut scores = std::mem::take(&mut self.replan.scores);
        let mut options = std::mem::take(&mut self.replan.options);
        let mut order = std::mem::take(&mut self.replan.order);
        rows.clear();
        scores.clear();
        options.clear();
        let stride = IN_EXTRA + self.config.extra_columns;
        let mut default_feasible = false;
        for option in 0..=self.widen_option() {
            if excluded == Some(option) {
                continue;
            }
            let Some((plan, preference, widening)) = self.candidate(option, widen) else {
                continue;
            };
            let (feasible, first) = self.feasible(option, plan, view, rules);
            if option == self.default_option() {
                default_feasible = feasible;
            }
            let start = rows.len();
            rows.resize(start + stride, 0);
            let row = &mut rows[start..];
            row[IN_FEASIBLE] = i32::from(feasible);
            row[IN_DIST] = first.map_or(0, |p| self.dist_column(view.pos, p));
            row[IN_PREF] = preference as i32;
            row[IN_WIDEN] = i32::from(widening);
            row[IN_KNOWN] = self.known_column(plan, view);
            let ctx = PlanCtx {
                now: view.now,
                pos: self.pos_out(view.pos),
                option,
                widening,
                feasible,
                first: first.map(|p| self.pos_out(p)),
                motive: self.motive,
            };
            rules.columns(&ctx, &mut row[IN_EXTRA..]);
            let built_in = self
                .replan
                .decider
                .score(Candidate::new(option, 0), row)
                .unwrap_or(0);
            scores.push(rules.score(&ctx, built_in).unwrap_or(built_in));
            options.push(option);
        }
        let seed = mix((u64::from(self.config.id) << 32) | u64::from(view.now));
        let chosen =
            choose(&scores, self.config.pick, seed, &mut order).map(|c| options[c.candidate]);
        self.replan.rows = rows;
        self.replan.scores = scores;
        self.replan.options = options;
        self.replan.order = order;
        (chosen, default_feasible)
    }

    /// Falls back to holding where the character stands.
    pub(super) fn hold_here(&mut self, view: View<'_>) -> Action {
        self.emit(Event::FellBack);
        self.plan.set_steps(&[Step::hold(Where::Here)], false);
        self.running_option = None;
        let action = self.start_running(view);
        self.fallback_hold = true;
        action
    }

    /// The candidate plan of `option` with its preference and whether it
    /// widens; `None` for an empty slot, a missing built-in plan, or a
    /// widening plan when the character may not widen.
    fn candidate(&self, option: u32, widen: bool) -> Option<(&Plan, u32, bool)> {
        let r = &self.replan;
        let o = option as usize;
        if let Some(slot) = r.playbook.get(o) {
            return (slot.used && (widen || !slot.widening)).then_some((
                &slot.plan,
                slot.preference,
                slot.widening,
            ));
        }
        if option == self.default_option() {
            r.has_default
                .then_some((&r.default_plan, DEFAULT_PREFERENCE, false))
        } else {
            (widen && r.has_widen).then_some((&r.widen_plan, WIDEN_PREFERENCE, true))
        }
    }

    /// Copies the plan of `option` in and starts it.
    fn start_option(&mut self, option: u32, view: View<'_>) -> Action {
        let r = &self.replan;
        let plan = match r.playbook.get(option as usize) {
            Some(slot) => &slot.plan,
            None if option == self.default_option() => &r.default_plan,
            None => &r.widen_plan,
        };
        self.plan.copy_from(plan);
        self.running_option = Some(option);
        self.fallback_hold = false;
        self.start_running(view)
    }

    /// The plan in `self.plan` begins now; its first action.
    fn start_running(&mut self, view: View<'_>) -> Action {
        self.has_plan = true;
        self.pending_start = false;
        self.activity = Activity::Plan;
        self.begin(0, view);
        let kind = self.plan.steps()[0].kind;
        // Checked from the next tick on, like a step begun by `advance`.
        self.step_action(kind, view).unwrap_or(Action::None)
    }

    /// Whether `plan` (of `option`) can start now: the conditions of its
    /// first step and of the plan hold, and its first position can be
    /// filled. Also that position.
    fn feasible(
        &self,
        option: u32,
        plan: &Plan,
        view: View<'_>,
        rules: &mut dyn MissionRules,
    ) -> (bool, Option<Pos>) {
        let fails = plan.steps().first().map_or(&[][..], |s| &s.fails[..]);
        let failed = self
            .first_fail(fails, 0, plan.fails(), Some(option), view, rules)
            .is_some();
        match self.first_place(plan, view) {
            Some(first) => (!failed, first),
            None => (false, None),
        }
    }

    /// The position of the first step that names one (`Here` is where the
    /// character stands); `Some(None)` when no step does, `None` when it
    /// cannot be filled (steps that may be skipped are skipped).
    fn first_place(&self, plan: &Plan, view: View<'_>) -> Option<Option<Pos>> {
        for step in plan.steps() {
            let at = match step.kind {
                StepKind::GoTo(at)
                | StepKind::Hold(at)
                | StepKind::Search { centre: at, .. }
                | StepKind::Custom { at, .. } => match at {
                    Where::Here => Some(view.pos),
                    _ => self.resolve(at, view),
                },
                StepKind::Follow(subject) | StepKind::Attack(subject) => {
                    self.subject_pos(subject, view)
                }
                _ => continue,
            };
            match at {
                Some(p) => return Some(Some(p)),
                None if step.skip_if_impossible => {}
                None => return None,
            }
        }
        Some(None)
    }

    /// The [`IN_DIST`] column from `at` to `to` (internal coordinates).
    fn dist_column(&self, at: Pos, to: Pos) -> i32 {
        let metric = self.config.metric;
        let d = distance(
            metric,
            i64::from(to[0]) - i64::from(at[0]),
            i64::from(to[1]) - i64::from(at[1]),
        );
        let d = if metric == Metric::Euclid {
            (d as u128).isqrt() as i128
        } else {
            d
        };
        d.min(i128::from(DIST_CAP)) as i32
    }

    /// The [`IN_KNOWN`] column of `plan`.
    fn known_column(&self, plan: &Plan, view: View<'_>) -> i32 {
        let mut named = false;
        let mut freshest: Option<u32> = None;
        for step in plan.steps() {
            let subject = match step.kind {
                StepKind::Follow(s)
                | StepKind::Attack(s)
                | StepKind::WaitFor { subject: s, .. } => Some(s),
                StepKind::GoTo(at)
                | StepKind::Hold(at)
                | StepKind::Search { centre: at, .. }
                | StepKind::Custom { at, .. }
                | StepKind::Squad { target: at, .. } => self.subject_of(at),
                _ => None,
            };
            let Some(subject) = subject else { continue };
            named = true;
            if let Some(f) = self.fact(subject, view) {
                let age = view.now.wrapping_sub(f.seen_at);
                freshest = Some(freshest.map_or(age, |a| a.min(age)));
            }
        }
        match freshest {
            Some(age) => age.min(KNOWN_CAP as u32) as i32,
            None if named => KNOWN_CAP,
            None => 0,
        }
    }

    /// The subject a place names, if any.
    pub(super) fn subject_of(&self, at: Where) -> Option<u32> {
        match at {
            Where::Subject(s) => Some(s),
            Where::MotiveSubject => self.motive.subject,
            Where::Fixed(_) | Where::MotivePlace | Where::Here => None,
        }
    }

    /// Builds the default plan (and, with `widen`, the widening plan) from
    /// what the character knows now.
    fn build_defaults(&mut self, view: View<'_>, widen: bool) {
        let mut steps = [Step::pause(); MAX_BUILT_IN_STEPS];
        steps[0] = Step::call(0, 0);
        let n = self.default_steps(view, &mut steps[1..]);
        let r = &mut self.replan;
        r.has_default = n > 0;
        r.default_plan.set_steps(&steps[1..=n], false);
        r.has_widen =
            widen && n > 0 && matches!(self.motive.kind, MotiveKind::Guard | MotiveKind::Defeat);
        if r.has_widen {
            r.widen_plan.set_steps(&steps[..=n], false);
        }
    }

    /// Writes the default plan of the motive into `out` and returns its
    /// length (0: none).
    fn default_steps(&self, view: View<'_>, out: &mut [Step]) -> usize {
        let motive = self.motive;
        let place = Where::MotivePlace;
        let mut put = |steps: &[Step]| {
            out[..steps.len()].copy_from_slice(steps);
            steps.len()
        };
        match motive.kind {
            MotiveKind::Guard => match (motive.subject, motive.place) {
                (Some(s), _) => put(&[Step::follow(s)]),
                (None, Some(_)) => put(&[Step::go_to_where(place), Step::hold(place)]),
                (None, None) => 0,
            },
            MotiveKind::Reach if motive.place.is_some() => put(&[
                Step::go_to_where(place),
                Step::signal(self.config.reached_signal),
                Step::hold(place),
            ]),
            MotiveKind::Defeat => {
                let target = match motive.subject {
                    Some(s) => self.fact(s, view).filter(|f| !self.defeated(f)),
                    None => self.nearest_enemy(view),
                };
                let lost_after = self.config.lost_after;
                match target {
                    Some(f) if view.now.wrapping_sub(f.seen_at) <= lost_after => {
                        put(&[Step::attack(f.subject)])
                    }
                    Some(f) => put(&[Step::search(
                        Where::Subject(f.subject),
                        motive.radius,
                        lost_after,
                    )]),
                    None => 0,
                }
            }
            MotiveKind::Reach | MotiveKind::Custom(_) => 0,
        }
    }

    /// The nearest known enemy that is not defeated; on equal distance the
    /// lower subject id.
    fn nearest_enemy<'k>(&self, view: View<'k>) -> Option<&'k Fact> {
        let bits = self.config.enemy_bits;
        view.knowledge
            .iter()
            .filter(|f| {
                f.kind == self.config.subject_kind && f.data & bits != 0 && !self.defeated(f)
            })
            .min_by_key(|f| {
                let p = self.pos_in(f.pos);
                let d = distance(
                    self.config.metric,
                    i64::from(p[0]) - i64::from(view.pos[0]),
                    i64::from(p[1]) - i64::from(view.pos[1]),
                );
                (d, f.subject)
            })
    }
}
