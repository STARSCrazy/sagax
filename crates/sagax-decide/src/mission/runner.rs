//! Running a plan: [`Mission`] and its tick.

use super::rules::{BuiltInRules, FailCtx, MissionRules};
use super::types::{
    Action, BreakReason, Event, Fail, Leeway, MAX_EVENTS, MissionConfig, MissionError, Motive,
    Plan, StepKind, Where,
};
use crate::memory::{Fact, Knowledge, Metric, Pos, distance, points_around};

#[path = "detour.rs"]
mod detour;
#[path = "replan.rs"]
mod replan;

use detour::DetourState;
pub use replan::{
    DEFAULT_PREFERENCE, DIST_CAP, IN_DIST, IN_EXTRA, IN_FEASIBLE, IN_KNOWN, IN_PREF, IN_WIDEN,
    KNOWN_CAP, WIDEN_PREFERENCE,
};
use replan::{MAX_BUILT_IN_STEPS, Replan};

/// What one [`Mission::tick`] gives the game: the action to take now and
/// what happened on the way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output<'a> {
    /// What the character should do now.
    pub action: Action,
    events: &'a [Event],
}

impl<'a> Output<'a> {
    /// What happened during the tick, in order; at most [`MAX_EVENTS`].
    pub fn events(&self) -> &'a [Event] {
        self.events
    }
}

/// What the mission is doing at the moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Activity {
    /// No plan yet: the next tick replans (or holds where it stands).
    Idle,
    /// Running the plan at [`StepState::index`].
    Plan,
    /// The plan ended in a `Wander` step: wander until a condition of the
    /// plan fires or the game gives a new plan.
    Wander,
    /// A detour runs; what ran before waits in [`DetourState::resume`].
    Detour,
    /// The detour ended: walking back to the anchor before resuming.
    Returning,
}

/// The running step and what it keeps while it runs.
#[derive(Clone, Copy, Debug, Default)]
struct StepState {
    /// Index of the step in the plan.
    index: usize,
    /// When the step began.
    begun_at: u32,
    /// Where the character stood when the step began (internal coordinates).
    here: Pos,
    /// The search point the step is going to (a counter, cycled).
    search_point: usize,
    /// The game called [`Mission::finish_step`].
    finished: bool,
}

/// What the tick sees: time, the character and what it knows.
#[derive(Clone, Copy)]
struct View<'k> {
    now: u32,
    /// The character's position (internal coordinates).
    pos: Pos,
    knowledge: &'k Knowledge,
    flags: u64,
}

/// A character's mission: a motive and the plan it runs toward it.
///
/// Create it, give it a plan, tick it. Each [`Mission::tick`] runs the
/// current step of the plan from what the character knows and returns the
/// action to take. A step that is done hands over to the next step within
/// the same tick (at most one step change per tick); a looping plan starts
/// again after its last step.
///
/// The mission reads only what it is given: the character's position, its
/// [`Knowledge`] and the condition bits the game built from that knowledge.
///
/// **Hex:** with [`MissionConfig::hex_layout`] every position the game gives
/// or gets is in offset coordinates, including the fact positions in the
/// knowledge; the mission works in axial coordinates inside.
///
/// All storage is reserved in [`Mission::new`]; ticking allocates nothing.
#[derive(Clone, Debug)]
pub struct Mission {
    config: MissionConfig,
    motive: Motive,
    leeway: Leeway,
    /// The plan storage (capacity `max_steps`, at least the longest
    /// built-in plan); valid while `has_plan`.
    plan: Plan,
    has_plan: bool,
    /// The decider option the running plan came from; `None` for a plan
    /// the game set and for holding after a fallback.
    running_option: Option<u32>,
    /// The plan was just set: its first step begins at the next tick.
    pending_start: bool,
    activity: Activity,
    step: StepState,
    /// Where the character stood at the last tick (internal coordinates).
    last_pos: Option<Pos>,
    detour: DetourState,
    replan: Replan,
    events: Vec<Event>,
}

impl Mission {
    /// A mission for `motive`, without a plan. Errors on `max_steps` of 0
    /// ([`MissionError::ZeroCapacity`]), above 65536 (step numbers are
    /// `u16`; [`MissionError::TooManySteps`]) or on a hex layout without
    /// [`Metric::Hex`].
    pub fn new(config: MissionConfig, motive: Motive) -> Result<Self, MissionError> {
        if config.max_steps == 0 {
            return Err(MissionError::ZeroCapacity);
        }
        if config.max_steps > usize::from(u16::MAX) + 1 {
            return Err(MissionError::TooManySteps);
        }
        if config.hex_layout.is_some() && config.metric != Metric::Hex {
            return Err(MissionError::HexNeedsHexMetric);
        }
        Ok(Self {
            plan: Plan::with_capacity(config.max_steps.max(MAX_BUILT_IN_STEPS)),
            replan: Replan::new(&config),
            config,
            motive,
            leeway: Leeway::default(),
            has_plan: false,
            running_option: None,
            pending_start: false,
            activity: Activity::Idle,
            step: StepState::default(),
            last_pos: None,
            detour: DetourState::new(),
            events: Vec::with_capacity(MAX_EVENTS),
        })
    }

    /// The settings.
    pub fn config(&self) -> &MissionConfig {
        &self.config
    }

    /// Sets how freely the character acts.
    pub fn set_leeway(&mut self, leeway: Leeway) {
        self.leeway = leeway;
    }

    /// How freely the character acts.
    pub fn leeway(&self) -> Leeway {
        self.leeway
    }

    /// Sets the motive; the running plan stays.
    pub fn set_motive(&mut self, motive: Motive) {
        self.motive = motive;
    }

    /// The motive.
    pub fn motive(&self) -> Motive {
        self.motive
    }

    /// Copies `plan` into the mission; its first step begins at the next
    /// tick, whatever ran before. During a detour (or the walk back) the
    /// detour goes on and the new plan begins when the character resumes.
    /// Errors on an empty plan or one with more than `max_steps` steps (the
    /// mission keeps what it had).
    pub fn set_plan(&mut self, plan: &Plan) -> Result<(), MissionError> {
        let len = plan.steps().len();
        if len == 0 {
            return Err(MissionError::EmptyPlan);
        }
        if len > self.config.max_steps {
            return Err(MissionError::TooManySteps);
        }
        self.plan.copy_from(plan);
        self.has_plan = true;
        self.running_option = None;
        self.pending_start = true;
        match self.activity {
            Activity::Detour | Activity::Returning => self.detour.resume = Activity::Plan,
            _ => self.activity = Activity::Plan,
        }
        self.step = StepState::default();
        Ok(())
    }

    /// The plan that runs, or ran last (also after it ended or broke): the
    /// one the game set, a playbook plan, a built-in plan, or the
    /// `Hold(Here)` of a fallback. `None` before the first.
    pub fn plan(&self) -> Option<&Plan> {
        self.has_plan.then_some(&self.plan)
    }

    /// Index of the step that runs, while a plan runs (not during a detour
    /// or the walk back).
    pub fn step_index(&self) -> Option<usize> {
        (self.activity == Activity::Plan).then_some(self.step.index)
    }

    /// Marks the running [`StepKind::Custom`] step as done; the plan moves
    /// on at the next tick. Other steps ignore it.
    pub fn finish_step(&mut self) {
        self.step.finished = true;
    }

    /// Runs one tick at time `now` for a character standing at `pos`, from
    /// what it knows (`knowledge`) and the game's condition bits for it
    /// (`flags`, built from that knowledge). [`Mission::tick_with`] with
    /// [`BuiltInRules`].
    pub fn tick(&mut self, now: u32, pos: Pos, knowledge: &Knowledge, flags: u64) -> Output<'_> {
        self.tick_with(now, pos, knowledge, flags, &mut BuiltInRules)
    }

    /// [`Mission::tick`] with the game's rules (see [`MissionRules`] for
    /// the slots and their order).
    ///
    /// Each tick: the fail conditions of the running step, then those of
    /// the plan; a fail breaks the plan. Then the step: impossible (a
    /// position it needs is not known) breaks the plan or, with
    /// `skip_if_impossible`, counts as done; done emits
    /// [`Event::StepDone`] and the next step begins and gives the action.
    /// After the last step a looping plan emits [`Event::Looped`] and starts
    /// again; otherwise [`Event::PlanDone`], and the end counts as a break
    /// ([`BreakReason::PlanEnded`]). A plan that ends in a `Wander` step
    /// wanders instead (no replan) until a condition of the plan fires.
    /// `Call` and `Squad` steps emit [`Event::Call`] (with the character's
    /// position) and [`Event::WantsSquad`] when the character may widen
    /// ([`Leeway::replan`] and [`Leeway::widen`]); else they only count as
    /// done.
    ///
    /// The plan end counts as a break for replanning, with one difference:
    /// widening plans (the built-in one and widening playbook slots) are
    /// candidates only on a real break (a fail condition or an impossible
    /// step), not at a plan end and not without a plan.
    ///
    /// **On a break** the tick emits [`Event::Broken`], then:
    ///
    /// - with [`Leeway::replan`]: every filled playbook slot (a widening one
    ///   only when the character may widen; a playbook plan that just ended
    ///   is no candidate), the default plan of the motive and, when the
    ///   character may widen, the built-in widening plan (a call for help,
    ///   then the default plan; for `Guard` and `Defeat`) are scored by the
    ///   [`Mission::decider`] and picked by [`MissionConfig::pick`] (seed:
    ///   `now` with the mission's id). The winner starts at its first step
    ///   at once, [`Event::Replanned`]. When every candidate scores 0: the
    ///   default plan if it is feasible, else `Hold(Here)`, both
    ///   [`Event::FellBack`].
    /// - without: `Hold(Here)` where the character stands, [`Event::FellBack`].
    ///   A strict character always finishes the walk back to the anchor of
    ///   a detour before it resumes, so it holds at the anchor.
    ///
    /// The default plan, built from what the character knows at that
    /// moment: `Guard` follows its subject, or goes to its place and holds
    /// it; `Defeat` attacks its subject (or with none the nearest known
    /// enemy: [`MissionConfig::enemy_bits`], not defeated, lower id on equal
    /// distance) when seen within `lost_after` ticks, else searches its last
    /// known position for `lost_after` ticks within the motive's `radius`
    /// (0: no limit); `Reach` goes to its place,
    /// gives [`MissionConfig::reached_signal`] and holds there; `Custom` has
    /// none.
    ///
    /// Without a plan (none set yet) the tick does what a break does,
    /// without [`Event::Broken`].
    ///
    /// During a detour the detour runs instead and no fail condition is
    /// checked; a plan that broke meanwhile breaks on the resume. See
    /// [`Mission::detour`].
    pub fn tick_with(
        &mut self,
        now: u32,
        pos: Pos,
        knowledge: &Knowledge,
        flags: u64,
        rules: &mut dyn MissionRules,
    ) -> Output<'_> {
        self.events.clear();
        let view = View {
            now,
            pos: self.pos_in(pos),
            knowledge,
            flags,
        };
        self.last_pos = Some(view.pos);
        if self.detour.announce {
            self.detour.announce = false;
            self.emit(Event::DetourStarted);
        }
        let action = match self.activity {
            Activity::Idle => self.replace_plan(view, None, false, rules),
            Activity::Wander => self.run_wander(view, rules),
            Activity::Plan => self.run_plan(view, rules),
            Activity::Detour => self.run_detour(view, rules),
            Activity::Returning => self.run_return(view, rules),
        };
        Output {
            action: self.action_out(action),
            events: &self.events,
        }
    }

    fn run_plan(&mut self, view: View<'_>, rules: &mut dyn MissionRules) -> Action {
        if self.pending_start {
            self.pending_start = false;
            self.begin(0, view);
        }
        let index = self.step.index;
        let step = self.plan.steps()[index];
        if let Some(fail) = self.first_fail(&step.fails, index, self.plan.fails(), view, rules) {
            return self.break_plan(BreakReason::Fail(fail), view, rules);
        }
        let Some(action) = self.step_action(step.kind, view) else {
            return if step.skip_if_impossible {
                self.advance(view, rules)
            } else {
                self.break_plan(BreakReason::Impossible, view, rules)
            };
        };
        if !self.step_done(step.kind, action, view) {
            return action;
        }
        let widen = self.leeway.replan && self.leeway.widen;
        match step.kind {
            StepKind::Wander => {
                self.emit(Event::StepDone(index as u16));
                self.emit(Event::PlanDone);
                self.activity = Activity::Wander;
                return Action::Wander;
            }
            StepKind::Signal(n) => self.emit(Event::Signal(n)),
            StepKind::Call { channel, kind } if widen => self.emit(Event::Call {
                channel,
                kind,
                pos: self.pos_out(view.pos),
            }),
            StepKind::Squad { tactic, target } if widen => {
                let pos = self.resolve(target, view).map(|p| self.pos_out(p));
                self.emit(Event::WantsSquad {
                    tactic,
                    target: pos,
                    subject: self.subject_of(target),
                });
            }
            _ => {}
        }
        self.advance(view, rules)
    }

    /// The plan ended in a `Wander` step: wander on until a condition of
    /// the plan fires.
    fn run_wander(&mut self, view: View<'_>, rules: &mut dyn MissionRules) -> Action {
        match self.first_fail(&[], 0, self.plan.fails(), view, rules) {
            Some(fail) => self.break_plan(BreakReason::Fail(fail), view, rules),
            None => Action::Wander,
        }
    }

    /// Ends the running step and begins the next; returns its action.
    fn advance(&mut self, view: View<'_>, rules: &mut dyn MissionRules) -> Action {
        self.emit(Event::StepDone(self.step.index as u16));
        let mut next = self.step.index + 1;
        if next == self.plan.steps().len() {
            if !self.plan.repeat {
                self.emit(Event::PlanDone);
                self.emit(Event::Broken(BreakReason::PlanEnded));
                let finished = self.running_option.filter(|&o| o < self.default_option());
                return self.replace_plan(view, finished, false, rules);
            }
            self.emit(Event::Looped);
            next = 0;
        }
        self.begin(next, view);
        let kind = self.plan.steps()[next].kind;
        // The new step is checked from the next tick on; one that cannot
        // be done yet does nothing now.
        self.step_action(kind, view).unwrap_or(Action::None)
    }

    fn begin(&mut self, index: usize, view: View<'_>) {
        self.step = StepState {
            index,
            begun_at: view.now,
            here: view.pos,
            search_point: 0,
            finished: false,
        };
    }

    fn break_plan(
        &mut self,
        reason: BreakReason,
        view: View<'_>,
        rules: &mut dyn MissionRules,
    ) -> Action {
        self.emit(Event::Broken(reason));
        self.replace_plan(view, None, true, rules)
    }

    /// The first fail condition that holds: those of step `index`
    /// (`step_fails`), then the plan's. [`Fail::Custom`] asks the rules.
    fn first_fail(
        &self,
        step_fails: &[Option<Fail>],
        index: usize,
        plan_fails: &[Option<Fail>],
        view: View<'_>,
        rules: &mut dyn MissionRules,
    ) -> Option<Fail> {
        let step = step_fails.iter().flatten().map(|&f| (Some(index), f));
        let plan = plan_fails.iter().flatten().map(|&f| (None, f));
        step.chain(plan)
            .find(|&(step, f)| self.fails(f, step, view, rules))
            .map(|(_, f)| f)
    }

    fn fails(
        &self,
        fail: Fail,
        step: Option<usize>,
        view: View<'_>,
        rules: &mut dyn MissionRules,
    ) -> bool {
        match fail {
            Fail::Unknown { subject } => self.fact(subject, view).is_none(),
            Fail::Defeated { subject } => {
                self.fact(subject, view).is_some_and(|f| self.defeated(f))
            }
            Fail::Fact {
                subject,
                kind,
                mask,
                value,
            } => view
                .knowledge
                .get(subject, kind)
                .is_some_and(|f| f.data & mask == value),
            Fail::Flags { mask, value } => view.flags & mask == value,
            Fail::Custom(id) => {
                let ctx = FailCtx {
                    now: view.now,
                    pos: self.pos_out(view.pos),
                    step,
                    motive: self.motive,
                };
                rules.fail(&ctx, id)
            }
        }
    }

    /// The action of a step now (internal coordinates); `None` when the
    /// step cannot be done because a position it needs is not known.
    fn step_action(&mut self, kind: StepKind, view: View<'_>) -> Option<Action> {
        Some(match kind {
            StepKind::GoTo(at) => Action::GoTo(self.resolve(at, view)?),
            StepKind::Wait(_) | StepKind::Pause | StepKind::WaitFor { .. } => Action::Wait,
            StepKind::Follow(subject) => Action::Follow {
                subject,
                pos: self.subject_pos(subject, view)?,
            },
            StepKind::Search { centre, radius, .. } => {
                let centre = self.resolve(centre, view)?;
                let (i, point) = self.search_step(self.step.search_point, centre, radius, view.pos);
                self.step.search_point = i;
                Action::Search(point)
            }
            StepKind::Hold(at) => Action::Hold(self.resolve(at, view)?),
            // An attack on an unknown subject is done, not impossible.
            StepKind::Attack(subject) => match self.subject_pos(subject, view) {
                Some(pos) => Action::Attack { subject, pos },
                None => Action::None,
            },
            StepKind::Wander => Action::Wander,
            StepKind::Custom { kind, at, param } => Action::Custom {
                kind,
                pos: Some(self.resolve(at, view)?),
                param,
            },
            StepKind::Signal(_) | StepKind::Call { .. } | StepKind::Squad { .. } => Action::None,
        })
    }

    fn step_done(&self, kind: StepKind, action: Action, view: View<'_>) -> bool {
        let elapsed = view.now.wrapping_sub(self.step.begun_at);
        match kind {
            StepKind::GoTo(_) => match action {
                Action::GoTo(p) => self.arrived(view.pos, p),
                _ => false,
            },
            StepKind::Wait(ticks) | StepKind::Search { ticks, .. } => elapsed >= ticks,
            StepKind::Pause => elapsed >= self.config.pause_ticks,
            StepKind::Follow(_) | StepKind::Hold(_) => false,
            StepKind::WaitFor { subject, radius } => self
                .subject_pos(subject, view)
                .is_some_and(|p| self.within(view.pos, p, radius)),
            StepKind::Attack(subject) => self.fact(subject, view).is_none_or(|f| self.defeated(f)),
            StepKind::Custom { .. } => self.step.finished,
            // Call and Squad are done at once (with widening they give
            // their event first).
            StepKind::Wander
            | StepKind::Signal(_)
            | StepKind::Call { .. }
            | StepKind::Squad { .. } => true,
        }
    }

    /// The position `at` stands for now (internal coordinates).
    fn resolve(&self, at: Where, view: View<'_>) -> Option<Pos> {
        match at {
            Where::Fixed(p) => Some(self.pos_in(p)),
            Where::Subject(subject) => self.subject_pos(subject, view),
            Where::MotivePlace => self.motive.place.map(|p| self.pos_in(p)),
            Where::MotiveSubject => self
                .motive
                .subject
                .and_then(|subject| self.subject_pos(subject, view)),
            Where::Here => Some(self.step.here),
        }
    }

    fn fact<'k>(&self, subject: u32, view: View<'k>) -> Option<&'k Fact> {
        view.knowledge.get(subject, self.config.subject_kind)
    }

    /// Where the subject was last known (internal coordinates).
    fn subject_pos(&self, subject: u32, view: View<'_>) -> Option<Pos> {
        self.fact(subject, view).map(|f| self.pos_in(f.pos))
    }

    fn defeated(&self, fact: &Fact) -> bool {
        let bits = self.config.defeated_bits;
        bits != 0 && fact.data & bits == bits
    }

    /// Number of points in the search pattern.
    fn search_points(&self) -> usize {
        self.config.search.offsets.len() + 1
    }

    /// The point a search with counter `point` around `centre` goes to
    /// for a character standing at `at`, moving on to the next point once
    /// it is there. Returns the new counter and the point (internal
    /// coordinates).
    fn search_step(&self, point: usize, centre: Pos, radius: u32, at: Pos) -> (usize, Pos) {
        let (i, p) = self.search_point(point, centre, radius);
        if self.arrived(at, p) {
            self.search_point((i + 1) % self.search_points(), centre, radius)
        } else {
            (i, p)
        }
    }

    /// The search point from counter `start` on, around `centre` (internal
    /// coordinates), with the counter it was found at. Points farther than
    /// `radius` from the centre are skipped (the counter moves past them);
    /// `radius` 0 is no limit. When every point is skipped, the centre and
    /// `start`.
    fn search_point(&self, start: usize, centre: Pos, radius: u32) -> (usize, Pos) {
        let points = self.search_points();
        for j in 0..points {
            let i = (start + j) % points;
            let p = self.pattern_point(i, centre);
            if radius == 0 || self.within(centre, p, radius) {
                return (i, p);
            }
        }
        (start, centre)
    }

    /// Point `i` of the search pattern around `centre` (internal
    /// coordinates). With a hex layout the pattern, and its `min`/`max`
    /// bounds, work in offset coordinates.
    fn pattern_point(&self, i: usize, centre: Pos) -> Pos {
        let around = self.pos_out(centre);
        points_around(self.config.id, i, around, &self.config.search)
            .map_or(centre, |p| self.pos_in(p))
    }

    fn arrived(&self, at: Pos, spot: Pos) -> bool {
        self.within(at, spot, self.config.arrive_radius)
    }

    /// `at` and `spot` are on the same level and at most `radius` apart in
    /// the metric (Euclid compares squares).
    fn within(&self, at: Pos, spot: Pos, radius: u32) -> bool {
        if at[2] != spot[2] {
            return false;
        }
        let metric = self.config.metric;
        let d = distance(
            metric,
            i64::from(spot[0]) - i64::from(at[0]),
            i64::from(spot[1]) - i64::from(at[1]),
        );
        let r = i128::from(radius);
        d <= if metric == Metric::Euclid { r * r } else { r }
    }

    /// Capacities of all storage, to check that ticking allocates nothing.
    #[cfg(test)]
    pub(super) fn capacities(&self) -> Vec<usize> {
        let mut c = vec![self.events.capacity(), self.plan.capacity()];
        c.extend(self.replan.capacities());
        c
    }

    fn emit(&mut self, event: Event) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(event);
        } else {
            debug_assert!(false, "more than MAX_EVENTS events in one tick");
        }
    }

    fn pos_in(&self, p: Pos) -> Pos {
        self.config.hex_layout.map_or(p, |l| l.to_axial_pos(p))
    }

    fn pos_out(&self, p: Pos) -> Pos {
        self.config.hex_layout.map_or(p, |l| l.from_axial_pos(p))
    }

    fn action_out(&self, action: Action) -> Action {
        match action {
            Action::GoTo(p) => Action::GoTo(self.pos_out(p)),
            Action::Hold(p) => Action::Hold(self.pos_out(p)),
            Action::Search(p) => Action::Search(self.pos_out(p)),
            Action::Flee(p) => Action::Flee(self.pos_out(p)),
            Action::Follow { subject, pos } => Action::Follow {
                subject,
                pos: self.pos_out(pos),
            },
            Action::Attack { subject, pos } => Action::Attack {
                subject,
                pos: self.pos_out(pos),
            },
            Action::Custom { kind, pos, param } => Action::Custom {
                kind,
                pos: pos.map(|p| self.pos_out(p)),
                param,
            },
            Action::None | Action::Wait | Action::Wander => action,
        }
    }
}
