use crate::Pick;
use crate::memory::{Metric, PointPattern, Pos};
use sagax_core::HexLayout;

/// A place a step or detour refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Where {
    /// A fixed position.
    Fixed(Pos),
    /// Where this subject was last known.
    Subject(u32),
    /// The place of the mission's motive.
    MotivePlace,
    /// Where the motive's subject was last known.
    MotiveSubject,
    /// Where the character stands at that moment.
    Here,
}

/// What a step does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepKind {
    /// Walk to the place.
    GoTo(Where),
    /// Wait this many ticks.
    Wait(u32),
    /// Wait for the configured pause (`pause_ticks`).
    Pause,
    /// Follow this subject.
    Follow(u32),
    /// Wait until this subject is within `radius` of the character.
    WaitFor {
        /// The subject to wait for.
        subject: u32,
        /// How close it must come.
        radius: u32,
    },
    /// Search around `centre` for this many ticks: go to the points of the
    /// config's search pattern in turn, again and again.
    Search {
        /// The middle of the search.
        centre: Where,
        /// How far from the middle to search, in the mission's metric
        /// (Euclid compares squares): a pattern point farther away is
        /// skipped and the next one taken. 0 is no limit. When every point
        /// is skipped, the character searches at the centre.
        radius: u32,
        /// How long to search.
        ticks: u32,
    },
    /// Hold the place.
    Hold(Where),
    /// Attack this subject.
    Attack(u32),
    /// Wander about.
    Wander,
    /// Give this signal (the step is done at once).
    Signal(u32),
    /// A step the game defines.
    Custom {
        /// The game's own kind.
        kind: u32,
        /// Where it happens.
        at: Where,
        /// A free parameter.
        param: u32,
    },
    /// Call for help on a channel.
    Call {
        /// The channel to call on.
        channel: u32,
        /// The kind of call.
        kind: u32,
    },
    /// Ask for a squad tactic against a target.
    Squad {
        /// The tactic to ask for.
        tactic: u32,
        /// What the tactic is aimed at.
        target: Where,
    },
}

/// A condition that breaks a step or a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fail {
    /// The subject is not known (no fact of the subject kind).
    Unknown {
        /// The subject.
        subject: u32,
    },
    /// The subject is defeated.
    Defeated {
        /// The subject.
        subject: u32,
    },
    /// A fact of this kind about the subject has `data & mask == value`.
    Fact {
        /// The subject.
        subject: u32,
        /// The fact kind.
        kind: u32,
        /// Bits of the fact's `data` to look at.
        mask: u32,
        /// The value those bits must have.
        value: u32,
    },
    /// The game's flags have `flags & mask == value`.
    Flags {
        /// Bits to look at.
        mask: u64,
        /// The value those bits must have.
        value: u64,
    },
    /// A condition the game decides (by its own number).
    Custom(u32),
}

/// How many fail conditions a step and a plan can hold each.
pub const MAX_FAILS: usize = 4;

/// Puts `fail` in the first free slot. A fifth one is dropped.
fn add_fail(slots: &mut [Option<Fail>; MAX_FAILS], fail: Fail) {
    match slots.iter_mut().find(|s| s.is_none()) {
        Some(slot) => *slot = Some(fail),
        None => debug_assert!(false, "more than MAX_FAILS fail conditions"),
    }
}

/// One step of a [`Plan`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Step {
    /// What the step does.
    pub kind: StepKind,
    /// Conditions that break the step; filled from the front.
    pub fails: [Option<Fail>; MAX_FAILS],
    /// Skip the step instead of breaking the plan when it cannot be done.
    pub skip_if_impossible: bool,
}

impl Step {
    const fn of(kind: StepKind) -> Self {
        Self {
            kind,
            fails: [None; MAX_FAILS],
            skip_if_impossible: false,
        }
    }

    /// Walk to a fixed position.
    pub const fn go_to(pos: Pos) -> Self {
        Self::of(StepKind::GoTo(Where::Fixed(pos)))
    }

    /// Walk to a place.
    pub const fn go_to_where(at: Where) -> Self {
        Self::of(StepKind::GoTo(at))
    }

    /// Wait this many ticks.
    pub const fn wait(ticks: u32) -> Self {
        Self::of(StepKind::Wait(ticks))
    }

    /// Wait for the configured pause.
    pub const fn pause() -> Self {
        Self::of(StepKind::Pause)
    }

    /// Follow a subject.
    pub const fn follow(subject: u32) -> Self {
        Self::of(StepKind::Follow(subject))
    }

    /// Wait until the subject is within `radius`.
    pub const fn wait_for(subject: u32, radius: u32) -> Self {
        Self::of(StepKind::WaitFor { subject, radius })
    }

    /// Search around `centre` within `radius` for `ticks`.
    pub const fn search(centre: Where, radius: u32, ticks: u32) -> Self {
        Self::of(StepKind::Search {
            centre,
            radius,
            ticks,
        })
    }

    /// Hold a place.
    pub const fn hold(at: Where) -> Self {
        Self::of(StepKind::Hold(at))
    }

    /// Attack a subject.
    pub const fn attack(subject: u32) -> Self {
        Self::of(StepKind::Attack(subject))
    }

    /// Wander about.
    pub const fn wander() -> Self {
        Self::of(StepKind::Wander)
    }

    /// Give a signal.
    pub const fn signal(signal: u32) -> Self {
        Self::of(StepKind::Signal(signal))
    }

    /// A step the game defines.
    pub const fn custom(kind: u32, at: Where, param: u32) -> Self {
        Self::of(StepKind::Custom { kind, at, param })
    }

    /// Call for help.
    pub const fn call(channel: u32, kind: u32) -> Self {
        Self::of(StepKind::Call { channel, kind })
    }

    /// Ask for a squad tactic.
    pub const fn squad(tactic: u32, target: Where) -> Self {
        Self::of(StepKind::Squad { tactic, target })
    }

    /// Adds a condition that breaks this step, in the first free slot. At
    /// most [`MAX_FAILS`]; a further one is ignored (a debug assertion
    /// fires).
    pub fn fails_when(mut self, fail: Fail) -> Self {
        add_fail(&mut self.fails, fail);
        self
    }

    /// Skip this step when it cannot be done.
    pub const fn skip_if_impossible(mut self) -> Self {
        self.skip_if_impossible = true;
        self
    }
}

/// A list of steps, once or in a loop, with fail conditions for the whole.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Plan {
    steps: Vec<Step>,
    /// Start again after the last step.
    pub repeat: bool,
    fails: [Option<Fail>; MAX_FAILS],
}

impl Plan {
    fn new(steps: &[Step], repeat: bool) -> Self {
        Self {
            steps: steps.to_vec(),
            repeat,
            fails: [None; MAX_FAILS],
        }
    }

    /// An empty plan with room for `steps` steps (storage of a mission).
    pub(super) fn with_capacity(steps: usize) -> Self {
        Self {
            steps: Vec::with_capacity(steps),
            repeat: false,
            fails: [None; MAX_FAILS],
        }
    }

    /// Makes this plan a copy of `other` without allocating, as long as
    /// `other` has no more steps than this plan's capacity.
    pub(super) fn copy_from(&mut self, other: &Plan) {
        self.steps.clear();
        self.steps.extend_from_slice(&other.steps);
        self.repeat = other.repeat;
        self.fails = other.fails;
    }

    /// A plan that runs the steps once.
    pub fn once(steps: &[Step]) -> Self {
        Self::new(steps, false)
    }

    /// A plan that runs the steps again and again.
    pub fn looping(steps: &[Step]) -> Self {
        Self::new(steps, true)
    }

    /// Adds a condition that breaks the whole plan, in the first free slot.
    /// At most [`MAX_FAILS`]; a further one is ignored (a debug assertion
    /// fires).
    pub fn fails_when(mut self, fail: Fail) -> Self {
        add_fail(&mut self.fails, fail);
        self
    }

    /// The steps.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The conditions that break the plan; filled from the front.
    pub fn fails(&self) -> &[Option<Fail>; MAX_FAILS] {
        &self.fails
    }
}

/// What the character wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotiveKind {
    /// Keep a place or a subject safe.
    Guard,
    /// Defeat a subject or the enemies.
    Defeat,
    /// Get to a place.
    Reach,
    /// A motive the game defines.
    Custom(u32),
}

/// The motive of a mission: what the character wants and where or whom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Motive {
    /// What the character wants.
    pub kind: MotiveKind,
    /// The place it is about, if any.
    pub place: Option<Pos>,
    /// The subject it is about, if any.
    pub subject: Option<u32>,
    /// How close counts as there; 0 uses the config's `arrive_radius`.
    pub radius: u32,
}

impl Motive {
    const fn of(kind: MotiveKind, place: Option<Pos>, subject: Option<u32>) -> Self {
        Self {
            kind,
            place,
            subject,
            radius: 0,
        }
    }

    /// Guard a place.
    pub const fn guard_place(place: Pos) -> Self {
        Self::of(MotiveKind::Guard, Some(place), None)
    }

    /// Guard a subject.
    pub const fn guard_subject(subject: u32) -> Self {
        Self::of(MotiveKind::Guard, None, Some(subject))
    }

    /// Defeat a subject.
    pub const fn defeat(subject: u32) -> Self {
        Self::of(MotiveKind::Defeat, None, Some(subject))
    }

    /// Defeat the enemies (no particular subject).
    pub const fn defeat_enemies() -> Self {
        Self::of(MotiveKind::Defeat, None, None)
    }

    /// Reach a place.
    pub const fn reach(place: Pos) -> Self {
        Self::of(MotiveKind::Reach, Some(place), None)
    }

    /// A motive the game defines.
    pub const fn custom(kind: u32) -> Self {
        Self::of(MotiveKind::Custom(kind), None, None)
    }
}

/// What the character does when it loses the subject of a detour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OnLost {
    /// Go back to the anchor.
    Return,
    /// Search for a while first.
    Search {
        /// How long to search.
        ticks: u32,
        /// How far around the last known place.
        radius: u32,
    },
    /// Keep going until the detour ends by itself.
    Keep,
}

/// How far and how long a detour may take the character from its plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Leash {
    /// Largest distance from the anchor, in the mission's metric; `None`
    /// for no limit.
    pub distance: Option<u32>,
    /// Longest detour in ticks; `None` for no limit.
    pub ticks: Option<u32>,
    /// What to do when the subject is lost.
    pub on_lost: OnLost,
}

/// How freely a character acts: its leash and whether it may replan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Leeway {
    /// The leash for detours.
    pub leash: Leash,
    /// May take another plan when its plan breaks or ends.
    pub replan: bool,
    /// May fall back to the built-in widening plan (only with `replan`).
    pub widen: bool,
}

impl Leeway {
    /// Distance 3, returns when the subject is lost, never replans.
    pub const STRICT: Self = Self {
        leash: Leash {
            distance: Some(3),
            ticks: None,
            on_lost: OnLost::Return,
        },
        replan: false,
        widen: false,
    };
    /// Distance 12, searches when the subject is lost, replans.
    pub const FLEXIBLE: Self = Self {
        leash: Leash {
            distance: Some(12),
            ticks: None,
            on_lost: OnLost::Search {
                ticks: 60,
                radius: 6,
            },
        },
        replan: true,
        widen: false,
    };
    /// No leash, replans and widens.
    pub const FREE: Self = Self {
        leash: Leash {
            distance: None,
            ticks: None,
            on_lost: OnLost::Keep,
        },
        replan: true,
        widen: true,
    };
}

impl Default for Leeway {
    /// [`Leeway::FLEXIBLE`].
    fn default() -> Self {
        Self::FLEXIBLE
    }
}

/// Settings of a mission.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MissionConfig {
    /// The mission's id (for the seed and for search points).
    pub id: u32,
    /// Most steps a plan may have.
    pub max_steps: usize,
    /// Number of playbook slots.
    pub max_playbook: usize,
    /// The fact kind positions are read from.
    pub subject_kind: u32,
    /// Bits of a fact's `data` that mark an enemy.
    pub enemy_bits: u32,
    /// Bits of a fact's `data` that mark a defeated subject.
    pub defeated_bits: u32,
    /// How close counts as arrived.
    pub arrive_radius: u32,
    /// Ticks after which a subject not seen counts as lost.
    pub lost_after: u32,
    /// Ticks of the [`StepKind::Pause`] step.
    pub pause_ticks: u32,
    /// Distance at which a fleeing character counts as safe.
    pub safe_distance: u32,
    /// The signal given when the motive's place is reached.
    pub reached_signal: u32,
    /// How distances are measured.
    pub metric: Metric,
    /// The pattern of search points.
    pub search: PointPattern,
    /// Hex layout: positions of the game are then offset coordinates.
    pub hex_layout: Option<HexLayout>,
    /// Extra input columns the game adds to each decider row.
    pub extra_columns: usize,
    /// How the decider picks among playbook plans.
    pub pick: Pick,
}

impl Default for MissionConfig {
    /// Id 0, 16 steps, 8 playbook slots, subject kind 0, enemy bit 1,
    /// defeated bit 2, arrive radius 0, lost after 30, pause 50, safe
    /// distance 12, reached signal 0, Chebyshev, the default search
    /// pattern, no hex layout, no extra columns, [`Pick::Best`].
    fn default() -> Self {
        Self {
            id: 0,
            max_steps: 16,
            max_playbook: 8,
            subject_kind: 0,
            enemy_bits: 1,
            defeated_bits: 2,
            arrive_radius: 0,
            lost_after: 30,
            pause_ticks: 50,
            safe_distance: 12,
            reached_signal: 0,
            metric: Metric::Chebyshev,
            search: PointPattern::default(),
            hex_layout: None,
            extra_columns: 0,
            pick: Pick::Best,
        }
    }
}

/// A short break from the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Detour {
    /// Go for this subject.
    Engage(u32),
    /// Get away from a place, to a place if given.
    Flee {
        /// What to get away from.
        from: Where,
        /// Where to run to, if the game says.
        to: Option<Pos>,
    },
    /// A detour the game defines.
    Custom {
        /// The game's own kind.
        kind: u32,
        /// Where it happens.
        at: Where,
        /// A free parameter.
        param: u32,
    },
}

/// What the character should do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Nothing.
    None,
    /// Walk to the position.
    GoTo(Pos),
    /// Stand and wait.
    Wait,
    /// Follow a subject, last known at `pos`.
    Follow {
        /// The subject.
        subject: u32,
        /// Where it was last known.
        pos: Pos,
    },
    /// Hold the position.
    Hold(Pos),
    /// Attack a subject, last known at `pos`.
    Attack {
        /// The subject.
        subject: u32,
        /// Where it was last known.
        pos: Pos,
    },
    /// Search at the position.
    Search(Pos),
    /// Wander about.
    Wander,
    /// Run to the position.
    Flee(Pos),
    /// An action of a game-defined step or detour.
    Custom {
        /// The game's own kind.
        kind: u32,
        /// Where, if anywhere.
        pos: Option<Pos>,
        /// A free parameter.
        param: u32,
    },
}

/// Why a plan broke.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BreakReason {
    /// A fail condition fired.
    Fail(Fail),
    /// A step could not be done.
    Impossible,
    /// The plan ended.
    PlanEnded,
}

/// Why a detour ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DetourEnd {
    /// The subject was defeated.
    Defeated,
    /// The subject is no longer known.
    Forgotten,
    /// The subject was lost.
    Lost,
    /// The leash ran out.
    Leash,
    /// The character is safe.
    Safe,
    /// The character arrived.
    Arrived,
    /// The game ended it.
    Ended,
}

/// Something that happened during a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// The step with this index is done.
    StepDone(u16),
    /// The plan started again.
    Looped,
    /// The plan is done.
    PlanDone,
    /// The plan broke.
    Broken(BreakReason),
    /// A new plan was taken; `option` is the decider option.
    Replanned {
        /// The decider option that won.
        option: u32,
    },
    /// The character fell back to holding its position.
    FellBack,
    /// A detour started.
    DetourStarted,
    /// A detour ended.
    DetourEnded(DetourEnd),
    /// The character is back at its anchor and resumes.
    Returned,
    /// A signal was given.
    Signal(u32),
    /// A call for help.
    Call {
        /// The channel.
        channel: u32,
        /// The kind of call.
        kind: u32,
        /// Where the caller is.
        pos: Pos,
    },
    /// The mission wants a squad tactic.
    WantsSquad {
        /// The tactic.
        tactic: u32,
        /// The target place, if any.
        target: Option<Pos>,
        /// The target subject, if any.
        subject: Option<u32>,
    },
}

/// Most events one tick can give.
pub const MAX_EVENTS: usize = 8;

/// Why a mission could not be made or changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MissionError {
    /// A capacity of 0 was asked for.
    ZeroCapacity,
    /// The plan has no steps.
    EmptyPlan,
    /// The plan has more steps than `max_steps`.
    TooManySteps,
    /// All playbook slots are in use.
    PlaybookFull,
    /// A hex layout needs the hex metric.
    HexNeedsHexMetric,
}

impl std::fmt::Display for MissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ZeroCapacity => "capacity must be at least 1",
            Self::EmptyPlan => "the plan has no steps",
            Self::TooManySteps => "the plan has more steps than the mission allows",
            Self::PlaybookFull => "the playbook is full",
            Self::HexNeedsHexMetric => "a hex layout needs the hex metric",
        })
    }
}

impl std::error::Error for MissionError {}
