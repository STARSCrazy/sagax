//! Running a plan: [`Mission`] and its tick.

use super::types::{
    Action, BreakReason, Event, Fail, Leeway, MAX_EVENTS, MissionConfig, MissionError, Motive,
    Plan, Step, StepKind, Where,
};
use crate::memory::{Fact, Knowledge, Metric, Pos, distance, points_around};

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
    /// No plan to run.
    Idle,
    /// Running the plan at [`StepState::index`].
    Plan,
    /// The plan ended in a `Wander` step: wander until given a new plan.
    Wander,
    /// The plan ended or broke: nothing to do until given a new plan.
    Stopped,
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
    /// The plan storage (capacity `max_steps`); valid while `has_plan`.
    plan: Plan,
    has_plan: bool,
    /// The plan was just set: its first step begins at the next tick.
    pending_start: bool,
    activity: Activity,
    step: StepState,
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
            plan: Plan::with_capacity(config.max_steps),
            config,
            motive,
            leeway: Leeway::default(),
            has_plan: false,
            pending_start: false,
            activity: Activity::Idle,
            step: StepState::default(),
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
    /// tick, whatever ran before. Errors on an empty plan or one with more
    /// than `max_steps` steps (the mission keeps what it had).
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
        self.pending_start = true;
        self.activity = Activity::Plan;
        self.step = StepState::default();
        Ok(())
    }

    /// The plan given last, if any (also after it ended or broke).
    pub fn plan(&self) -> Option<&Plan> {
        self.has_plan.then_some(&self.plan)
    }

    /// Index of the step that runs, while a plan runs.
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
    /// (`flags`, built from that knowledge).
    ///
    /// Each tick: the fail conditions of the running step, then those of
    /// the plan; a fail breaks the plan. Then the step: impossible (a
    /// position it needs is not known) breaks the plan or, with
    /// `skip_if_impossible`, counts as done; done emits
    /// [`Event::StepDone`] and the next step begins and gives the action.
    /// After the last step a looping plan emits [`Event::Looped`] and starts
    /// again; otherwise [`Event::PlanDone`] and the action is
    /// [`Action::None`].
    pub fn tick(&mut self, now: u32, pos: Pos, knowledge: &Knowledge, flags: u64) -> Output<'_> {
        self.events.clear();
        let view = View {
            now,
            pos: self.pos_in(pos),
            knowledge,
            flags,
        };
        let action = match self.activity {
            Activity::Idle | Activity::Stopped => Action::None,
            Activity::Wander => Action::Wander,
            Activity::Plan => self.run_plan(view),
        };
        Output {
            action: self.action_out(action),
            events: &self.events,
        }
    }

    fn run_plan(&mut self, view: View<'_>) -> Action {
        if self.pending_start {
            self.pending_start = false;
            self.begin(0, view);
        }
        let step = self.plan.steps()[self.step.index];
        if let Some(fail) = self.first_fail(&step, view) {
            return self.break_plan(BreakReason::Fail(fail));
        }
        let Some(action) = self.step_action(step.kind, view) else {
            return if step.skip_if_impossible {
                self.advance(view)
            } else {
                self.break_plan(BreakReason::Impossible)
            };
        };
        if !self.step_done(step.kind, action, view) {
            return action;
        }
        match step.kind {
            StepKind::Wander => {
                self.emit(Event::StepDone(self.step.index as u16));
                self.emit(Event::PlanDone);
                self.activity = Activity::Wander;
                Action::Wander
            }
            StepKind::Signal(n) => {
                self.emit(Event::Signal(n));
                self.advance(view)
            }
            _ => self.advance(view),
        }
    }

    /// Ends the running step and begins the next; returns its action.
    fn advance(&mut self, view: View<'_>) -> Action {
        self.emit(Event::StepDone(self.step.index as u16));
        let mut next = self.step.index + 1;
        if next == self.plan.steps().len() {
            if !self.plan.repeat {
                self.emit(Event::PlanDone);
                self.activity = Activity::Stopped;
                return Action::None;
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

    fn break_plan(&mut self, reason: BreakReason) -> Action {
        self.emit(Event::Broken(reason));
        self.activity = Activity::Stopped;
        Action::None
    }

    /// The first fail condition that holds: the step's, then the plan's.
    fn first_fail(&self, step: &Step, view: View<'_>) -> Option<Fail> {
        step.fails
            .iter()
            .chain(self.plan.fails())
            .flatten()
            .copied()
            .find(|&f| self.fails(f, view))
    }

    fn fails(&self, fail: Fail, view: View<'_>) -> bool {
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
            // The game's rules answer this; without rules it never fails.
            Fail::Custom(_) => false,
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
                let mut point = self.search_point(centre, radius);
                if self.arrived(view.pos, point) {
                    self.step.search_point = (self.step.search_point + 1) % self.search_points();
                    point = self.search_point(centre, radius);
                }
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
            // Call and Squad give their events only with widening (later);
            // without, they count as done.
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

    /// The search point the step goes to now, around `centre` (internal
    /// coordinates). Points farther than `radius` from the centre are
    /// skipped (the step's counter moves past them); `radius` 0 is no
    /// limit. When every point is skipped, the centre.
    fn search_point(&mut self, centre: Pos, radius: u32) -> Pos {
        let points = self.search_points();
        for j in 0..points {
            let i = (self.step.search_point + j) % points;
            let p = self.pattern_point(i, centre);
            if radius == 0 || self.within(centre, p, radius) {
                self.step.search_point = i;
                return p;
            }
        }
        centre
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
