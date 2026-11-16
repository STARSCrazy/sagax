use super::members::{Group, Member, MemberState};
use super::orders::{Channel, Order};
use super::rules::{
    BuiltInRules, CostCtx, PlaceCtx, ReportAction, RowCtx, SpotCtx, SquadRules, SquadView,
    UNREACHABLE, rank_spots, walk_dist, walk_plain,
};
use super::skill::{MAX_SKILL_ROWS, SkillTable};
use super::spots::{Spot, SpotPick, SpotTrace, heading, wish_point};
use super::tactic::{Abort, GoalKind, Place, Slot, Start, Tactic};
use crate::memory::{
    Area, Areas, AreasTrace, Evict, Fact, Inbox, Knowledge, KnowledgeTrace, Message, MessageTrace,
    Metric, Pos, ZeroCapacity, distance,
};
use crate::{Candidate, DecideTrace, Decider, INPUT_SINCE_OPTION, choose, mix};
use sagax_core::{MapSink, Sink};

/// Built-in row column: own losses, every member marked gone (also the
/// leader) that is still listed, however long ago; the count does not reset
/// with a new plan. Remove a gone member ([`Group::remove`]) to stop
/// counting it, for example when the group regroups.
pub const IN_LOSSES: usize = 0;
/// Built-in row column: active members that reported their goal of the
/// active plan broken ([`REPORT_GOAL_BROKEN`](super::REPORT_GOAL_BROKEN)); 0 in the row of any other
/// candidate (a new plan starts with nobody off it).
pub const IN_OFF_PLAN: usize = 1;
/// Built-in row column: ticks since the target was last seen (no target:
/// `i32::MAX`).
pub const IN_TARGET_AGE: usize = 2;
/// Built-in row column: distance from the group centre to the target, in
/// the metric (for [`Metric::Euclid`] the rounded-down root; no target:
/// `i32::MAX`). With a walking distance
/// ([`SquadRules::walk`](super::SquadRules::walk)) the mean walking
/// distance of the active members to the target (rounded down; members
/// that cannot get there do not count; none can: `i32::MAX`).
pub const IN_TARGET_DIST: usize = 3;
/// Built-in row column: active members with the first role of
/// [`SquadConfig::roles`].
pub const IN_ROLE_0: usize = 4;
/// Built-in row column: active members with the second role of
/// [`SquadConfig::roles`].
pub const IN_ROLE_1: usize = 5;
/// Built-in row column: ticks since the active plan became active, in the
/// row of the active (tactic, target); 0 in the row of any other candidate.
pub const IN_PLAN_AGE: usize = 6;
/// Built-in row column: distance from the target to its nearest other known
/// target, in the metric as [`IN_TARGET_DIST`]; -1 without a target or
/// without another known target (a curve starting at 0 reads it as 0).
/// With a walking distance the way from the other target to this one
/// (`i32::MAX` when none of them can get there).
pub const IN_TARGET_FRIEND_DIST: usize = 7;
/// First caller column, with at most two roles in [`SquadConfig::roles`];
/// with more roles use [`Squad::extra_start`].
#[deprecated(note = "use Squad::extra_start")]
pub const IN_EXTRA: usize = 8;
/// Most caller columns a row may have ([`Squad::set_extra_columns`]).
pub const MAX_EXTRA_COLUMNS: usize = 1024;
/// Most entries of the role list ([`SquadConfig::roles`]).
pub const MAX_ROLE_COUNTS: usize = 8;

/// The role list of a squad: up to [`MAX_ROLE_COUNTS`] role masks. Each
/// mask gets one row column that counts the active members having all its
/// bits (see [`Squad::role_column`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RoleCounts {
    masks: [u32; MAX_ROLE_COUNTS],
    len: u8,
}

impl RoleCounts {
    /// No role column.
    pub const NONE: RoleCounts = RoleCounts {
        masks: [0; MAX_ROLE_COUNTS],
        len: 0,
    };

    /// A role list of `masks`; `None` above [`MAX_ROLE_COUNTS`].
    pub fn new(masks: &[u32]) -> Option<RoleCounts> {
        if masks.len() > MAX_ROLE_COUNTS {
            return None;
        }
        let mut list = Self::NONE;
        list.masks[..masks.len()].copy_from_slice(masks);
        list.len = masks.len() as u8;
        Some(list)
    }

    /// The masks, in column order.
    pub fn as_slice(&self) -> &[u32] {
        &self.masks[..usize::from(self.len)]
    }
}

/// The number of a plan; each new plan gets a higher one (wrapping).
pub type PlanId = u32;

/// What one member is told to do: `pos` is where to go now; `kind` and
/// `start` are what to do there and when.
///
/// A slot that gathers first gets two orders: the plan's goal (the first
/// order) is [`GoalKind::Move`], [`Start::Now`] to the gather spot, with
/// `then_pos` set; on the signal the second order brings `then_pos` as
/// `pos`, the slot's own kind and a lined-up start (see [`Squad::signal`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Goal {
    /// What to do at `pos`.
    pub kind: GoalKind,
    /// Where to go now: a candidate spot, or the wish point itself when
    /// `unchecked`. [`Squad::plan`] always sets it.
    pub pos: Option<Pos>,
    /// True when no free spot was there and `pos` is the plain wish point,
    /// not checked by the game.
    pub unchecked: bool,
    /// Where to go after `pos` when the slot gathers first: `pos` is then
    /// the gather spot and this the spot of the slot's place (picked the
    /// same way; possibly an unchecked wish point). `None` without a gather
    /// place. On the signal the member gets a new order with this as `pos`.
    pub then_pos: Option<Pos>,
    /// Like `unchecked`, for `then_pos`.
    pub then_unchecked: bool,
    /// The target's id, if the tactic has one.
    pub target: Option<u32>,
    /// When to start what `kind` says, once at `pos`.
    pub start: Start,
    /// Index of the slot in the tactic.
    pub slot: u8,
    /// The plan this goal belongs to.
    pub plan: PlanId,
    /// When the goal was made.
    pub issued_at: u32,
    /// Ticks the member starts late (timing spread from skill and seed).
    pub start_delay: u32,
}

impl Goal {
    /// The tick the goal starts, for an order that arrived at `received`:
    /// `received + start_delay`, plus `n` for [`Start::AfterTicks`]`(n)`
    /// (wrapping). `None` for [`Start::OnSignal`]: it starts on the signal.
    pub fn start_tick(&self, received: u32) -> Option<u32> {
        let wait = match self.start {
            Start::Now => 0,
            Start::AfterTicks(n) => n,
            Start::OnSignal => return None,
        };
        Some(received.wrapping_add(self.start_delay).wrapping_add(wait))
    }

    /// True when the member must report its arrival at the first spot:
    /// the goal gathers first (`then_pos`) or waits for the signal.
    pub fn awaits_arrival(&self) -> bool {
        self.then_pos.is_some() || self.start == Start::OnSignal
    }
}

/// A plan made by [`Squad::plan`]: pending until `active_at`, then active
/// (see [`Squad::active_plan`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Plan {
    /// Its number.
    pub id: PlanId,
    /// Index of the tactic (from [`Squad::add_tactic`]).
    pub tactic: usize,
    /// The target's id, if the tactic has one.
    pub target: Option<u32>,
    /// `(member id, goal)` in assignment order.
    pub goals: Vec<(u32, Goal)>,
    /// When it was made.
    pub made_at: u32,
    /// When it becomes active: `made_at + think_ticks` of the skill
    /// (wrapping).
    pub active_at: u32,
}

/// Why [`Squad::plan`] refused: a caller error in the tactics or columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanError {
    /// A consideration of tactic `tactic` reads column `input`, but a row
    /// has only [`Squad::rule_start`] + [`SquadConfig::rule_columns`]
    /// columns.
    InputOutOfRange {
        /// Index of the tactic.
        tactic: usize,
        /// The column it reads.
        input: u32,
    },
    /// Tactic `tactic` has more than 255 slots (a goal keeps the slot index
    /// as a `u8`).
    TooManySlots {
        /// Index of the tactic.
        tactic: usize,
    },
    /// The decider has a different number of options than the squad has
    /// tactics (see [`Squad::decider_mut`]).
    OptionsChanged,
    /// The squad has parts only a rules object decides
    /// ([`Squad::needs_rules`]): call [`Squad::plan_with`] or
    /// [`Squad::check_with`].
    NeedsRules,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputOutOfRange { tactic, input } => {
                write!(f, "tactic {tactic} reads column {input}, beyond the row")
            }
            Self::TooManySlots { tactic } => {
                write!(f, "tactic {tactic} has more than 255 slots")
            }
            Self::OptionsChanged => write!(f, "the decider has options the tactics do not"),
            Self::NeedsRules => write!(f, "the squad has custom parts and needs a rules object"),
        }
    }
}

impl std::error::Error for PlanError {}

/// Salts that keep the seeded draws apart.
const SALT_SPOT: u64 = 0x5350_4f54_0000_0001;
const SALT_TIME: u64 = 0x5449_4d45_0000_0002;
const SALT_PICK: u64 = 0x5049_434b_0000_0003;

/// A seeded number for one member at one tick.
fn draw(seed: u64, salt: u64, member: usize, now: u32) -> u64 {
    mix(mix(seed ^ salt) ^ member as u64 ^ (u64::from(now) << 32))
}

fn clamp_i32(v: i128) -> i32 {
    v.clamp(0, i128::from(i32::MAX)) as i32
}

/// Distance in the metric between two points (x, y); for Euclid the
/// squared value, as [`distance`].
pub(super) fn dist(metric: Metric, a: Pos, b: Pos) -> i128 {
    distance(
        metric,
        i64::from(b[0]) - i64::from(a[0]),
        i64::from(b[1]) - i64::from(a[1]),
    )
}

/// Plain distance for a row: the root for Euclid.
pub(super) fn plain_dist(metric: Metric, a: Pos, b: Pos) -> i32 {
    let d = dist(metric, a, b);
    let d = if metric == Metric::Euclid {
        (d as u128).isqrt() as i128
    } else {
        d
    };
    clamp_i32(d)
}

/// The trace record of a [`Squad`]: what its `*_traced` calls decided, in
/// the order they decided it ([`Squad::plan_traced`],
/// [`Squad::check_traced`], [`Squad::observe_traced`],
/// [`Squad::report_traced`], [`Squad::signal_traced`],
/// [`Squad::reinstate_traced`], [`Squad::orders_due_traced`]). The rule slots are not traced themselves:
/// what a slot answered shows in the decision it fed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SquadTrace {
    /// A known target as the targets slot judged it (before each scoring,
    /// in knowledge order); `kept` false: no candidate for new plans.
    Target {
        /// The target, as the leader knows it.
        fact: Fact,
        /// The slot's answer.
        kept: bool,
    },
    /// A decider record of one candidate: its factors, then its score. The
    /// candidate index is the index in the candidate list of that scoring
    /// (tactic order, then target order).
    Decide {
        /// Index of the tactic.
        tactic: usize,
        /// The candidate's target; `None` for a tactic without one.
        target: Option<u32>,
        /// The decider's record.
        event: DecideTrace,
    },
    /// One abort rule checked, in list order (the check stops at the first
    /// that fires): on a candidate's row before it is scored (it scores 0
    /// when one fires), and on the active plan's row in a check.
    Abort {
        /// Index of the tactic.
        tactic: usize,
        /// The row's target; `None` for a tactic without one or a lost
        /// target.
        target: Option<u32>,
        /// Index of the rule in the tactic's list.
        abort: usize,
        /// True when it fired.
        fired: bool,
    },
    /// The candidate a scoring picked for a new plan (by the skill's pick;
    /// in a replan for a better plan only among the candidates above the
    /// bar).
    Picked {
        /// Index of the tactic.
        tactic: usize,
        /// Its target, if any.
        target: Option<u32>,
        /// Its score.
        score: u32,
    },
    /// A scoring for a new plan found no candidate to pick: no new plan.
    NoPick,
    /// The new plan is pending until `until`.
    Thinking {
        /// Its number.
        plan: PlanId,
        /// When it becomes active.
        until: u32,
    },
    /// A member was assigned to a slot of the new plan, in assignment
    /// order.
    Assigned {
        /// Index of the slot.
        slot: usize,
        /// The member's id.
        member: u32,
        /// Its member cost for the slot (the member cost slot's answer).
        cost: i128,
    },
    /// A spot pick for a member of the new plan: per member its first spot
    /// (the gather spot when it gathers), then its place spot.
    Spot {
        /// The member's id.
        member: u32,
        /// The ranking and the pick.
        event: SpotTrace,
    },
    /// The active plan's trigger was checked (while the plan holds and its
    /// signal was not given).
    Trigger {
        /// True when it fired.
        fired: bool,
    },
    /// A member report was read from the inbox, before it is handled.
    Report {
        /// The report.
        msg: Message,
    },
    /// The channel delay of an order to a member: on activation and on the
    /// signal per goal in goal order, on reinstating for that member.
    Delay {
        /// The member's id.
        member: u32,
        /// Ticks until it arrives; `None`: it does not reach the member.
        delay: Option<u32>,
    },
    /// The leader's knowledge changed (an own sighting, an enemy report, a
    /// [`ReportAction::Forget`](super::ReportAction::Forget)).
    Knowledge(KnowledgeTrace),
    /// The leader's danger areas changed (danger reports, report actions,
    /// expiry).
    Areas(AreasTrace),
    /// The leader's inbox took a report ([`MessageTrace::Queued`]) or a
    /// full one lost one ([`MessageTrace::Dropped`]: the report arriving
    /// last, or the new one), from [`Squad::report_traced`].
    Inbox(MessageTrace),
}

impl SquadTrace {
    /// The same record with every position passed through `f` (a hex crew
    /// turns axial positions into offset ones).
    pub(super) fn map_pos(self, f: impl Fn(Pos) -> Pos) -> Self {
        let fact = |x: Fact| Fact { pos: f(x.pos), ..x };
        let area = |a: Area| Area {
            pos: f(a.pos),
            source: f(a.source),
            ..a
        };
        match self {
            Self::Target { fact: x, kept } => Self::Target {
                fact: fact(x),
                kept,
            },
            Self::Spot {
                member,
                event: SpotTrace::Picked(SpotPick::Unchecked(p)),
            } => Self::Spot {
                member,
                event: SpotTrace::Picked(SpotPick::Unchecked(f(p))),
            },
            Self::Report { msg } => Self::Report {
                msg: Message {
                    pos: f(msg.pos),
                    ..msg
                },
            },
            Self::Inbox(m) => {
                let msg = |x: Message| Message { pos: f(x.pos), ..x };
                Self::Inbox(match m {
                    MessageTrace::Posted { msg: x, due_at } => MessageTrace::Posted {
                        msg: msg(x),
                        due_at,
                    },
                    MessageTrace::Refused { msg: x, why } => {
                        MessageTrace::Refused { msg: msg(x), why }
                    }
                    MessageTrace::Queued { msg: x, arrive_at } => MessageTrace::Queued {
                        msg: msg(x),
                        arrive_at,
                    },
                    MessageTrace::Dropped { msg: x } => MessageTrace::Dropped { msg: msg(x) },
                    MessageTrace::Delivered { msg: x } => MessageTrace::Delivered { msg: msg(x) },
                })
            }
            Self::Knowledge(k) => Self::Knowledge(match k {
                KnowledgeTrace::Added(x) => KnowledgeTrace::Added(fact(x)),
                KnowledgeTrace::Updated(x) => KnowledgeTrace::Updated(fact(x)),
                KnowledgeTrace::Stale(x) => KnowledgeTrace::Stale(fact(x)),
                KnowledgeTrace::Evicted(x) => KnowledgeTrace::Evicted(fact(x)),
                KnowledgeTrace::Expired(x) => KnowledgeTrace::Expired(fact(x)),
                KnowledgeTrace::Removed(x) => KnowledgeTrace::Removed(fact(x)),
            }),
            Self::Areas(a) => Self::Areas(match a {
                AreasTrace::Added { index, area: x } => AreasTrace::Added {
                    index,
                    area: area(x),
                },
                AreasTrace::Renewed { index, area: x } => AreasTrace::Renewed {
                    index,
                    area: area(x),
                },
                AreasTrace::Evicted(x) => AreasTrace::Evicted(area(x)),
                AreasTrace::Expired(x) => AreasTrace::Expired(area(x)),
            }),
            other => other,
        }
    }
}

/// Reused buffers, so planning does not allocate once they have grown.
#[derive(Clone, Debug, Default)]
pub(super) struct Scratch {
    pub(super) targets: Vec<Fact>,
    pub(super) candidates: Vec<(usize, Option<usize>)>,
    pub(super) rows: Vec<i32>,
    pub(super) scores: Vec<u32>,
    pub(super) order: Vec<usize>,
    pub(super) wishes: Vec<Pos>,
    pub(super) then_wishes: Vec<Option<Pos>>,
    pub(super) counts: Vec<u8>,
    pub(super) used: Vec<bool>,
    pub(super) picks: Vec<(usize, usize)>,
    pub(super) taken: Vec<bool>,
    pub(super) then_taken: Vec<bool>,
    pub(super) near: Vec<(i128, usize)>,
    pub(super) reports: Vec<Message>,
    /// Per target: kept by the targets slot.
    pub(super) kept: Vec<bool>,
    /// Member costs, slot-major (`slot * group.len() + member index`).
    pub(super) costs: Vec<Option<i128>>,
    /// `(score, index)` of the spots ranked for one member.
    pub(super) spot_keys: Vec<(i128, usize, i128)>,
    /// The report actions of one read of the inbox, in report order.
    pub(super) actions: Vec<ReportAction>,
}

/// The plan being carried out, with what happened since it became active.
#[derive(Clone, Debug)]
pub(super) struct Active {
    pub(super) plan: Plan,
    /// When it became active (its `active_at`).
    pub(super) since: u32,
    /// When the signal was given.
    pub(super) signal_at: Option<u32>,
    /// Members that called [`Squad::arrived`].
    pub(super) arrived: Vec<u32>,
    /// Members that reported their goal broken.
    pub(super) broken: Vec<u32>,
}

impl Active {
    pub(super) fn has(&self, member: u32) -> bool {
        self.plan.goals.iter().any(|(m, _)| *m == member)
    }
}

/// Whether the table's row count is in range.
fn rows_ok(table: &SkillTable) -> bool {
    (1..=MAX_SKILL_ROWS).contains(&usize::from(table.rows))
}

/// Most slots still short of their minimum that [`Squad::plan`] checks
/// jointly (every subset); beyond this only each slot alone and the sum.
const JOINT_SLOTS: usize = 12;

/// The default danger area of [`Squad::set_danger_area`].
const DANGER: Area = Area {
    kind: 0,
    pos: [0; 3],
    radius: 2,
    strength: 1,
    since: 0,
    lifetime: 255,
    data: 0,
    source: [0; 3],
    blocking: 0,
    has_source: 0,
};

/// Capacities and metric of a [`Squad`] (see [`Squad::new`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SquadConfig {
    /// Most members.
    pub members: usize,
    /// Most facts the leader remembers (the oldest goes first).
    pub memory: usize,
    /// Most danger areas.
    pub areas: usize,
    /// Most member reports waiting in the inbox.
    pub inbox: usize,
    /// Most danger reports waiting to arrive (see
    /// [`Squad::report_danger`]).
    pub dangers: usize,
    /// How distances are measured.
    pub metric: Metric,
    /// The role list: one row column per mask, counting the active members
    /// that have all its bits. The first two keep the columns
    /// [`IN_ROLE_0`] and [`IN_ROLE_1`], later ones follow after column 7
    /// (see [`Squad::role_column`]).
    pub roles: RoleCounts,
    /// The skill table the squad starts with (see [`Squad::set_skill_table`]).
    pub skill_table: SkillTable,
    /// How many row columns the rules fill
    /// ([`SquadRules::columns`](super::SquadRules::columns)), after the
    /// caller's extra columns from [`Squad::rule_start`] on; at most
    /// [`MAX_EXTRA_COLUMNS`]. More than 0 needs a rules object (see
    /// [`Squad::needs_rules`]).
    pub rule_columns: usize,
}

/// A danger report on its way to the leader (see [`Squad::report_danger`]).
#[derive(Clone, Copy, Debug)]
pub(super) struct DangerReport {
    pub(super) sender: u32,
    pub(super) centre: Pos,
    pub(super) shooter: Option<Pos>,
    pub(super) arrive_at: u32,
}

/// The squad leader: the group, what the leader knows, the tactics it can
/// use and the plans it made.
///
/// Targets come only from the leader's own [`Knowledge`] (facts of the kind
/// set by [`Squad::set_target_kind`]), fed by the leader's own sight
/// ([`Squad::observe`]) and by member reports ([`Squad::report`]), never
/// from the caller, so a group cannot plan with what it has not seen. The
/// caller only adds extra columns to the scoring rows.
///
/// **Running a plan:** [`Squad::plan`] (or [`Squad::check`]) makes a
/// *pending* plan. It becomes *active* `think_ticks` (from skill) after it
/// was made; until then the old plan stays active. On activation every
/// active member of the plan gets its goal as an [`Order`], arriving by its
/// [`Channel`] (see [`Squad::orders_due`]). [`Squad::check`] then watches
/// the active plan: abort rules, a better plan, the trigger.
/// [`Squad::signal`] sends the "on signal" members their second order.
#[derive(Clone, Debug)]
pub struct Squad {
    pub(super) group: Group,
    pub(super) knowledge: Knowledge,
    pub(super) areas: Areas,
    pub(super) inbox: Inbox,
    pub(super) dangers: Vec<DangerReport>,
    pub(super) dangers_cap: usize,
    pub(super) activated: Option<PlanId>,
    pub(super) metric: Metric,
    pub(super) roles: RoleCounts,
    pub(super) tactics: Vec<Tactic>,
    pub(super) decider: Decider,
    pub(super) skill: u8,
    pub(super) table: SkillTable,
    pub(super) leader: Option<u32>,
    pub(super) target_kind: u32,
    pub(super) extra_columns: usize,
    pub(super) rule_columns: usize,
    pub(super) pending: Option<Plan>,
    pub(super) active: Option<Active>,
    pub(super) orders: Vec<(Order, u32)>,
    pub(super) channels: Vec<(u32, Channel)>,
    pub(super) danger: Area,
    pub(super) spot_radius: Option<u32>,
    pub(super) next_id: PlanId,
    pub(super) scratch: Scratch,
}

impl Squad {
    /// A squad with the capacities and metric of `config`. The knowledge
    /// drops the oldest fact when full. A capacity or the skill table out of
    /// range (`rows` 0 or more than [`MAX_SKILL_ROWS`]) or more than
    /// [`MAX_EXTRA_COLUMNS`] rule columns is an error.
    pub fn new(config: SquadConfig) -> Result<Self, ZeroCapacity> {
        if config.dangers == 0
            || !rows_ok(&config.skill_table)
            || config.rule_columns > MAX_EXTRA_COLUMNS
        {
            return Err(ZeroCapacity);
        }
        Ok(Self {
            group: Group::new(config.members)?,
            knowledge: Knowledge::new(config.memory, Evict::Oldest)?,
            areas: Areas::new(config.areas)?,
            inbox: Inbox::new(config.inbox)?,
            dangers: Vec::with_capacity(config.dangers),
            dangers_cap: config.dangers,
            activated: None,
            metric: config.metric,
            roles: config.roles,
            tactics: Vec::new(),
            decider: Decider::new(),
            skill: 0,
            table: config.skill_table,
            leader: None,
            target_kind: 0,
            extra_columns: 0,
            rule_columns: config.rule_columns,
            pending: None,
            active: None,
            orders: Vec::new(),
            channels: Vec::new(),
            danger: DANGER,
            spot_radius: None,
            next_id: 1,
            scratch: Scratch::default(),
        })
    }

    /// The members.
    pub fn group(&self) -> &Group {
        &self.group
    }

    /// The members, to add, move, suspend or remove them.
    pub fn group_mut(&mut self) -> &mut Group {
        &mut self.group
    }

    /// What the leader knows.
    pub fn knowledge(&self) -> &Knowledge {
        &self.knowledge
    }

    /// What the leader knows, to add the leader's own sightings.
    pub fn knowledge_mut(&mut self) -> &mut Knowledge {
        &mut self.knowledge
    }

    /// The danger areas the leader remembers.
    pub fn areas(&self) -> &Areas {
        &self.areas
    }

    /// The danger areas the leader remembers, to change them.
    pub fn areas_mut(&mut self) -> &mut Areas {
        &mut self.areas
    }

    /// Adds a tactic the squad can use (if its skill reaches
    /// [`Tactic::min_skill`]); returns its index, counting from 0.
    pub fn add_tactic(&mut self, tactic: Tactic) -> usize {
        let option = self.decider.add_option(tactic.weight);
        for &c in &tactic.considerations {
            // The option was just added, so it exists.
            let _ = self.decider.add_consideration(option, c);
        }
        self.tactics.push(tactic);
        self.tactics.len() - 1
    }

    /// The tactic at `index`.
    pub fn tactic(&self, index: usize) -> Option<&Tactic> {
        self.tactics.get(index)
    }

    /// The squad's skill, 0 (weak) ..= 255 (elite); default 0.
    pub fn set_skill(&mut self, skill: u8) {
        self.skill = skill;
    }

    /// The squad's skill.
    pub fn skill(&self) -> u8 {
        self.skill
    }

    /// Replaces the skill table (default [`SkillTable::default`]). False
    /// (nothing changes) when `rows` is 0 or more than [`MAX_SKILL_ROWS`].
    #[must_use]
    pub fn set_skill_table(&mut self, table: SkillTable) -> bool {
        if !rows_ok(&table) {
            return false;
        }
        self.table = table;
        true
    }

    /// The decider that scores the tactics: option `i` is tactic `i`, and
    /// its considerations are the tactic's. Combine, rank, cooldown and the
    /// current action are free to change through [`Squad::decider_mut`];
    /// adding or removing options is not ([`Squad::validate`] refuses it
    /// with [`PlanError::OptionsChanged`]).
    pub fn decider(&self) -> &Decider {
        &self.decider
    }

    /// The decider to change how the tactics are scored (see
    /// [`Squad::decider`]).
    pub fn decider_mut(&mut self) -> &mut Decider {
        &mut self.decider
    }

    /// The skill table.
    pub fn skill_table(&self) -> &SkillTable {
        &self.table
    }

    /// Makes the member with this id the leader; false if it is not in the
    /// group.
    #[must_use]
    pub fn set_leader(&mut self, id: u32) -> bool {
        let known = self.group.index(id).is_some();
        if known {
            self.leader = Some(id);
        }
        known
    }

    /// The acting leader's id: the set leader while it is active; else the
    /// first active member of the active plan in slot order (on an equal
    /// slot the earlier goal); else the first active member of the group.
    /// Orders are timed from its position.
    pub fn leader(&self) -> Option<u32> {
        self.acting_leader().map(|m| m.id)
    }

    /// Marks a member gone (dead or lost), as [`Group::set_gone`]; false when
    /// the id is unknown. When it led, the next active member in slot order
    /// takes over (see [`Squad::leader`]). The active plan and its goals
    /// stay: the loss only counts in [`IN_LOSSES`], so it ends the plan only
    /// through the tactic's abort rules.
    pub fn set_gone(&mut self, id: u32) -> bool {
        if !self.group.set_gone(id) {
            return false;
        }
        if self.leader == Some(id) {
            self.leader = self.acting_leader().map(|m| m.id);
        }
        true
    }

    /// How orders reach the member with this id (default
    /// [`Channel::default`]: at once). The leader always has its own orders
    /// at once. False when the id is not in the group.
    #[must_use]
    pub fn set_channel(&mut self, id: u32, channel: Channel) -> bool {
        if self.group.index(id).is_none() {
            return false;
        }
        match self.channels.iter_mut().find(|(m, _)| *m == id) {
            Some(entry) => entry.1 = channel,
            None => self.channels.push((id, channel)),
        }
        true
    }

    /// The channel of the member with this id.
    pub fn channel(&self, id: u32) -> Channel {
        self.channels
            .iter()
            .find(|(m, _)| *m == id)
            .map_or_else(Channel::default, |e| e.1)
    }

    /// The area a danger report ([`Squad::report_danger`]) adds: its kind,
    /// radius, strength, lifetime and blocking are used; centre, source and
    /// data come from the report. Default: kind 0, radius 2 (two cells), strength 1, lifetime
    /// 255 ticks (15 s at about 17 ticks per second), not blocking.
    pub fn set_danger_area(&mut self, template: Area) {
        self.danger = template;
    }

    /// The area a danger report adds (see [`Squad::set_danger_area`]).
    pub fn danger_area(&self) -> Area {
        self.danger
    }

    /// The fact kind in the leader's knowledge that counts as a target
    /// (default 0).
    pub fn set_target_kind(&mut self, kind: u32) {
        self.target_kind = kind;
    }

    /// The known targets: facts of the target kind, in knowledge order.
    /// The caller's extra rows follow this order.
    pub fn targets(&self) -> impl Iterator<Item = &Fact> {
        let kind = self.target_kind;
        self.knowledge.iter().filter(move |f| f.kind == kind)
    }

    /// How far from its wish point a spot may be to count, in the metric
    /// (for [`Metric::Euclid`] the squared distance is compared with the
    /// radius squared). `None` (the default): per slot `3 * slot.dist`, at
    /// least 1, for both phases. A spot beyond it is not ranked at all; with
    /// no free spot within, the goal is the unchecked wish point.
    pub fn set_spot_radius(&mut self, radius: Option<u32>) {
        self.spot_radius = radius;
    }

    /// The spot radius (see [`Squad::set_spot_radius`]).
    pub fn spot_radius(&self) -> Option<u32> {
        self.spot_radius
    }

    /// The spot radius for `slot` (see [`Squad::set_spot_radius`]).
    fn spot_radius_of(&self, slot: &Slot) -> i64 {
        self.spot_radius
            .map_or_else(|| (3 * i64::from(slot.dist)).max(1), i64::from)
    }

    /// Checks that every consideration of every tactic reads a column the
    /// rows have (or one of the decider's memory inputs), and that no tactic
    /// has more than 255 slots, and that the decider still has one option per
    /// tactic. [`Squad::plan`] and [`Squad::check`] run
    /// this first. (An [`Abort`](super::Abort) on a column beyond the row
    /// never fires.)
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.decider.option_count() != self.tactics.len() {
            return Err(PlanError::OptionsChanged);
        }
        let stride = self.row_width();
        for (t, tactic) in self.tactics.iter().enumerate() {
            if tactic.slots.len() > usize::from(u8::MAX) {
                return Err(PlanError::TooManySlots { tactic: t });
            }
            for c in &tactic.considerations {
                if c.input < INPUT_SINCE_OPTION && c.input as usize >= stride {
                    return Err(PlanError::InputOutOfRange {
                        tactic: t,
                        input: c.input,
                    });
                }
            }
        }
        Ok(())
    }

    /// The row column of role `i` of the role list: 0 and 1 are
    /// [`IN_ROLE_0`] and [`IN_ROLE_1`], role `i >= 2` is column
    /// `8 + i - 2`. `None` beyond the list.
    pub fn role_column(&self, i: usize) -> Option<usize> {
        (i < self.roles.as_slice().len()).then(|| match i {
            0 => IN_ROLE_0,
            1 => IN_ROLE_1,
            _ => 8 + i - 2,
        })
    }

    /// The first caller column: 8, plus one for every role beyond two.
    /// Caller columns ([`Squad::set_extra_columns`]) start here.
    pub fn extra_start(&self) -> usize {
        8 + self.roles.as_slice().len().saturating_sub(2)
    }

    /// The first rule column ([`SquadConfig::rule_columns`]):
    /// [`Squad::extra_start`] plus the caller columns. A row is the
    /// built-in columns, the roles beyond two, the caller columns, then the
    /// rule columns.
    pub fn rule_start(&self) -> usize {
        self.extra_start() + self.extra_columns
    }

    /// The row width: [`Squad::rule_start`] plus the rule columns.
    fn row_width(&self) -> usize {
        self.rule_start() + self.rule_columns
    }

    /// How many caller columns each scoring row has after the built-in ones
    /// (from [`Squad::extra_start`] on; default 0). See [`Squad::plan`] for the
    /// layout of `extra`. False (nothing changes) above
    /// [`MAX_EXTRA_COLUMNS`].
    #[must_use]
    pub fn set_extra_columns(&mut self, count: usize) -> bool {
        if count > MAX_EXTRA_COLUMNS {
            return false;
        }
        self.extra_columns = count;
        true
    }

    /// The plan made last, while it waits to become active.
    pub fn pending_plan(&self) -> Option<&Plan> {
        self.pending.as_ref()
    }

    /// The plan being carried out, if any.
    pub fn active_plan(&self) -> Option<&Plan> {
        self.active.as_ref().map(|a| &a.plan)
    }

    /// The plan that became active since the last call, once (`None` when
    /// none did). A plan becomes active inside [`Squad::plan`],
    /// [`Squad::check`], [`Squad::orders_due`], [`Squad::signal`],
    /// [`Squad::arrived`] or [`Squad::reinstate`]; if two plans became
    /// active in between, the later one is returned.
    pub fn take_activated(&mut self) -> Option<PlanId> {
        self.activated.take()
    }

    /// True when the signal of the active plan was given.
    pub fn signal_given(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.signal_at.is_some())
    }

    /// The member that leads (see [`Squad::leader`]).
    pub(super) fn acting_leader(&self) -> Option<&Member> {
        let active = |id: u32| {
            self.group
                .index(id)
                .and_then(|i| self.group.get(i))
                .filter(|m| m.state == MemberState::Active)
        };
        if let Some(m) = self.leader.and_then(active) {
            return Some(m);
        }
        let in_plan = self.active.as_ref().and_then(|a| {
            a.plan
                .goals
                .iter()
                .enumerate()
                .filter_map(|(k, (id, g))| active(*id).map(|m| ((g.slot, k), m)))
                .min_by_key(|&(key, _)| key)
                .map(|(_, m)| m)
        });
        in_plan.or_else(|| self.group.active().map(|(_, m)| m).next())
    }

    pub(super) fn is_active_member(&self, id: u32) -> bool {
        self.group
            .index(id)
            .and_then(|i| self.group.get(i))
            .is_some_and(|m| m.state == MemberState::Active)
    }

    /// The mean position of the active members (rounded down).
    pub(super) fn centre(&self) -> Option<Pos> {
        let mut sum = [0i64; 3];
        let mut n = 0i64;
        for (_, m) in self.group.active() {
            for (s, &p) in sum.iter_mut().zip(m.pos.iter()) {
                *s += i64::from(p);
            }
            n += 1;
        }
        (n > 0).then(|| sum.map(|s| s.div_euclid(n) as i32))
    }

    /// Fills `row` for one candidate: the built-in inputs, then the extra
    /// block `block` (`None`: zeros); the rule columns stay 0. `active`
    /// marks the row of the active (tactic, target), which reads the active
    /// plan's off-plan count and age; other rows read 0 there.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn row_into(
        &self,
        rules: &mut dyn SquadRules,
        row: &mut Vec<i32>,
        now: u32,
        centre: Pos,
        target: Option<&Fact>,
        block: Option<usize>,
        extra: &[i32],
        active: bool,
    ) {
        let to_i32 = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        let count_role = |bit: u32| {
            self.group
                .active()
                .filter(|(_, m)| m.roles & bit == bit)
                .count()
        };
        let extra_start = self.extra_start();
        row.clear();
        row.resize(self.row_width(), 0);
        row[IN_LOSSES] = to_i32(
            self.group
                .iter()
                .filter(|m| m.state == MemberState::Gone)
                .count(),
        );
        for (i, &mask) in self.roles.as_slice().iter().enumerate() {
            if let Some(col) = self.role_column(i) {
                row[col] = to_i32(count_role(mask));
            }
        }
        row[IN_TARGET_AGE] = i32::MAX;
        row[IN_TARGET_DIST] = i32::MAX;
        row[IN_TARGET_FRIEND_DIST] = -1;
        if let Some(f) = target {
            row[IN_TARGET_AGE] = clamp_i32(i128::from(now.wrapping_sub(f.seen_at)));
            row[IN_TARGET_DIST] = self.target_dist(rules, centre, f.pos);
            if let Some(d) = self
                .targets()
                .filter(|o| o.subject != f.subject)
                .map(|o| self.friend_dist(rules, f.pos, o.pos))
                .min()
            {
                row[IN_TARGET_FRIEND_DIST] = d;
            }
        }
        if let Some(a) = self.active.as_ref().filter(|_| active) {
            let off = a
                .broken
                .iter()
                .filter(|&&id| self.is_active_member(id))
                .count();
            row[IN_OFF_PLAN] = to_i32(off);
            row[IN_PLAN_AGE] = clamp_i32(i128::from(now.wrapping_sub(a.since)));
        }
        if let Some(b) = block {
            let n = self.extra_columns;
            for c in 0..n {
                let at = b.checked_mul(n).and_then(|i| i.checked_add(c));
                row[extra_start + c] = at.and_then(|i| extra.get(i)).copied().unwrap_or(0);
            }
        }
    }

    /// [`IN_TARGET_DIST`] of a target at `target`: the plain metric
    /// distance from `centre`, or with a walking distance the mean walking
    /// distance of the active members (rounded down; members that cannot
    /// get there do not count; none can: `i32::MAX`).
    pub(super) fn target_dist(&self, rules: &mut dyn SquadRules, centre: Pos, target: Pos) -> i32 {
        if rules.walk().is_none() {
            return plain_dist(self.metric, centre, target);
        }
        let (mut sum, mut n) = (0u64, 0u64);
        for (_, m) in self.group.active() {
            let d = walk_plain(self.metric, rules, m.pos, target);
            if d != i32::MAX {
                sum += d as u64;
                n += 1;
            }
        }
        sum.checked_div(n)
            .map_or(i32::MAX, |d| i32::try_from(d).unwrap_or(i32::MAX))
    }

    /// [`IN_TARGET_FRIEND_DIST`] of one other target at `other` for the
    /// target at `target`: the plain distance, with a walking distance the
    /// way from the other target to this one (`i32::MAX` when it cannot get
    /// there).
    fn friend_dist(&self, rules: &mut dyn SquadRules, target: Pos, other: Pos) -> i32 {
        match rules.walk() {
            None => plain_dist(self.metric, target, other),
            Some(_) => walk_plain(self.metric, rules, other, target),
        }
    }

    /// Fills `s.kept` from `s.targets` through the targets slot, in
    /// knowledge order.
    pub(super) fn keep_targets<S: Sink<SquadTrace>>(
        &self,
        s: &mut Scratch,
        rules: &mut dyn SquadRules,
        sink: &mut S,
    ) {
        let view = SquadView::new(self);
        s.kept.clear();
        s.kept.extend(s.targets.iter().map(|f| {
            let kept = rules.keep_target(&view, f);
            if sink.enabled() {
                sink.emit(SquadTrace::Target { fact: *f, kept });
            }
            kept
        }));
    }

    /// Fills the rule columns of `s.rows` through the columns slot; `ctx`
    /// is `(tactic, target, now, active)`.
    pub(super) fn rule_columns_into(
        &self,
        s: &mut Scratch,
        rules: &mut dyn SquadRules,
        ctx: (usize, Option<Fact>, u32, bool),
    ) {
        let (tactic, target, now, active) = ctx;
        let (row, out) = s.rows.split_at_mut(self.rule_start());
        let ctx = RowCtx {
            tactic,
            target,
            row,
            now,
            active,
        };
        rules.columns(&SquadView::new(self), &ctx, out);
    }

    /// Fills `s.candidates` and `s.scores` from `s.targets` and `s.kept`
    /// (see [`Squad::plan`]); `keep` (tactic, target index) is a candidate
    /// even when the targets slot dropped its target.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn score_candidates<S: Sink<SquadTrace>>(
        &self,
        s: &mut Scratch,
        now: u32,
        centre: Pos,
        extra: &[i32],
        keep: Option<(usize, usize)>,
        rules: &mut dyn SquadRules,
        sink: &mut S,
    ) -> Result<(), PlanError> {
        s.candidates.clear();
        for (t, tactic) in self.tactics.iter().enumerate() {
            if tactic.min_skill > self.skill || !self.mins_reachable(&tactic.slots, &[], &[]) {
                continue;
            }
            if tactic.needs_target {
                let kept = &s.kept;
                s.candidates.extend(
                    (0..s.targets.len())
                        .filter(|&i| kept[i] || keep == Some((t, i)))
                        .map(|i| (t, Some(i))),
                );
            } else {
                s.candidates.push((t, None));
            }
        }
        s.scores.clear();
        for k in 0..s.candidates.len() {
            let (t, target) = s.candidates[k];
            let fact = target.map(|i| s.targets[i]);
            let id = fact.map(|f| f.subject);
            let active = self
                .active
                .as_ref()
                .is_some_and(|a| a.plan.tactic == t && a.plan.target == id);
            let block = Some(target.map_or(0, |i| i + 1));
            self.row_into(
                rules,
                &mut s.rows,
                now,
                centre,
                fact.as_ref(),
                block,
                extra,
                active,
            );
            self.rule_columns_into(s, rules, (t, fact, now, active));
            let ctx = RowCtx {
                tactic: t,
                target: fact,
                row: &s.rows,
                now,
                active,
            };
            let score = if self.aborts_fire(rules, &ctx, sink) {
                0
            } else {
                // `validate` ruled out the only error a row can cause.
                let candidate = Candidate::new(t as u32, id.unwrap_or(0));
                let mut decide = MapSink::new(&mut *sink, |event| SquadTrace::Decide {
                    tactic: t,
                    target: id,
                    event,
                });
                self.decider
                    .score_traced(k, candidate, &s.rows, &mut decide)
                    .map_err(|e| PlanError::InputOutOfRange {
                        tactic: t,
                        input: match e {
                            crate::DecideError::InputOutOfRange { input, .. } => input,
                            _ => u32::MAX,
                        },
                    })?
            };
            s.scores.push(score);
        }
        Ok(())
    }

    /// True when the free active members (not `used[i]`) can still bring
    /// every slot up to its minimum (`min`, at most `max`) on top of
    /// `counts` (missing entries count as 0 and free), all slots at once:
    /// for every set of slots still short, enough free members fit at least
    /// one of them (Hall's condition). With more than [`JOINT_SLOTS`] short
    /// slots, each slot alone and the sum are checked.
    fn mins_reachable(&self, slots: &[Slot], counts: &[u8], used: &[bool]) -> bool {
        let free = || {
            self.group
                .active()
                .filter(|(i, _)| !used.get(*i).copied().unwrap_or(false))
                .map(|(_, m)| m.roles)
        };
        let mut short = [(0u32, 0usize); JOINT_SLOTS];
        let (mut k, mut total, mut joint) = (0, 0, true);
        for (i, sl) in slots.iter().enumerate() {
            let have = usize::from(counts.get(i).copied().unwrap_or(0));
            let need = usize::from(sl.min.min(sl.max)).saturating_sub(have);
            if need == 0 {
                continue;
            }
            if free().filter(|r| r & sl.roles == sl.roles).count() < need {
                return false;
            }
            total += need;
            if k < JOINT_SLOTS {
                short[k] = (sl.roles, need);
                k += 1;
            } else {
                joint = false;
            }
        }
        if total > free().count() {
            return false;
        }
        if !joint {
            return true;
        }
        let short = &short[..k];
        (1u32..1 << k).all(|mask| {
            let chosen = || {
                short
                    .iter()
                    .enumerate()
                    .filter(move |&(j, _)| mask & (1 << j) != 0)
                    .map(|(_, &s)| s)
            };
            let need: usize = chosen().map(|(_, n)| n).sum();
            let fit = free()
                .filter(|&r| chosen().any(|(roles, _)| r & roles == roles))
                .count();
            fit >= need
        })
    }

    /// Makes a new plan and keeps it as the pending one.
    ///
    /// First a pending plan whose think time has passed becomes active (see
    /// [`Squad::orders_due`]), the reports that have arrived are read (see
    /// [`Squad::report`]) and danger areas whose lifetime is over are
    /// dropped ([`Areas::expire`]). The leader's knowledge is not aged here:
    /// forget old facts yourself (`knowledge_mut().expire(..)`, C
    /// `sx_knowledge_expire` on `sx_squad_knowledge`). The new plan replaces
    /// a pending one and becomes active `think_ticks` (from skill) after
    /// `now`.
    ///
    /// **Candidates** are every tactic the skill knows ([`Tactic::min_skill`]
    /// `<= skill`) whose slots can all reach their minimum at once with the
    /// active members (role bits are hard: a slot only takes members with
    /// all of them; two slots that each need the only ranged member cannot
    /// both have it). Once per known target (see [`Squad::targets`]), or
    /// once without a target if it does not need one. Ties go to the earlier
    /// candidate (tactic order, then target order). A candidate whose own
    /// abort rules fire on its row scores 0.
    ///
    /// **Rows:** each candidate is scored with its tactic's considerations
    /// over a row of the built-in inputs ([`IN_LOSSES`] ..=
    /// [`IN_TARGET_FRIEND_DIST`])
    /// followed by the caller's extra columns. `extra` holds one block of
    /// [`Squad::set_extra_columns`] values for the candidates without a
    /// target, then one block per known target in [`Squad::targets`] order;
    /// missing values read as 0. [`IN_OFF_PLAN`] and [`IN_PLAN_AGE`] are
    /// the active plan's in the row of the active (tactic, target), else 0.
    /// The pick is the skill's [`Pick`](crate::Pick) with `seed` mixed with
    /// `now`.
    ///
    /// **Assignment:** first every slot gets its minimum (slots with role
    /// bits first, in slot order, then the others, so a slot open to anyone
    /// does not use up a needed specialist), then the slots in order take
    /// one more member each, round by round, up to their maximum. A slot
    /// only takes free active members (suspended and gone are skipped) with
    /// all its role bits; among those the nearest to the slot's wish point
    /// (the member cost slot), ties to the lower member index. While
    /// minimums are filled, a member is skipped if taking it would leave
    /// another slot's minimum unreachable. Members get spots in assignment
    /// order ranked as [`pick_spot`](super::pick_spot) (the spot score slot)
    /// with `slot.want_tags | place.tags()`; with weak skill a spot may be up
    /// to `spot_rank_error` ranks off the best (from `seed`).
    ///
    /// Spots farther than the spot radius from the wish point do not count
    /// (see [`Squad::set_spot_radius`]).
    ///
    /// **Wish points** ([`wish_point`]) face the target; the goal's `pos` is
    /// the slot's `gather` place when it has one (at [`Slot::gather_dist`];
    /// then `then_pos` is its `place` at [`Slot::dist`] and the goal is a
    /// [`GoalKind::Move`], [`Start::Now`]; see [`Goal`]), else its `place`.
    /// Gather and place spots are taken separately, so both phases can use
    /// the same spots. Between
    /// uses the target's nearest other known target as the friend. A tactic
    /// without a target faces the nearest known target or, without one, the
    /// nearest danger area (from the group centre) and moves away from it
    /// through its offsets; with neither, every slot wishes for the
    /// leader's position. Formation offsets are in the square frame also on
    /// [`Metric::Hex`].
    ///
    /// **Rules:** this is [`Squad::plan_with`] with [`BuiltInRules`]; a
    /// squad with custom parts ([`Squad::needs_rules`]) is refused with
    /// [`PlanError::NeedsRules`] first. The [rules](super::rules) list every
    /// slot a game can fill, in the order the core calls them.
    ///
    /// Returns the new plan's id, or `Ok(None)` (pending plan unchanged)
    /// when there is no active member or no candidate scores above 0. A
    /// consideration that reads beyond the row is an error
    /// ([`Squad::validate`]), checked before anything else but the rules.
    pub fn plan(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
    ) -> Result<Option<PlanId>, PlanError> {
        if self.needs_rules() {
            return Err(PlanError::NeedsRules);
        }
        self.plan_with(now, extra, spots, seed, &mut BuiltInRules)
    }

    /// [`Squad::plan`] without the first steps. With a `bar` every
    /// candidate scoring at most `bar` is out, so a replan for a better plan
    /// picks only among the clearly better ones (never the active pair,
    /// which scores at most the bar).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn plan_now<S: Sink<SquadTrace>>(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        bar: Option<u64>,
        rules: &mut dyn SquadRules,
        sink: &mut S,
    ) -> Result<Option<PlanId>, PlanError> {
        let mut s = std::mem::take(&mut self.scratch);
        let made = self.plan_inner(&mut s, now, extra, spots, seed, bar, rules, sink);
        self.scratch = s;
        made
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_inner<S: Sink<SquadTrace>>(
        &mut self,
        s: &mut Scratch,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        bar: Option<u64>,
        rules: &mut dyn SquadRules,
        sink: &mut S,
    ) -> Result<Option<PlanId>, PlanError> {
        let (Some(leader), Some(centre)) = (self.acting_leader().map(|m| m.pos), self.centre())
        else {
            return Ok(None);
        };
        let level = self.table.level(self.skill);

        s.targets.clear();
        s.targets.extend(self.targets().copied());
        self.keep_targets(s, rules, sink);
        self.score_candidates(s, now, centre, extra, None, rules, sink)?;
        if let Some(bar) = bar {
            for score in &mut s.scores {
                if u64::from(*score) <= bar {
                    *score = 0;
                }
            }
        }
        let pick_seed = mix(seed ^ SALT_PICK ^ u64::from(now));
        let Some(choice) = choose(&s.scores, self.table.pick[level], pick_seed, &mut s.order)
        else {
            if sink.enabled() {
                sink.emit(SquadTrace::NoPick);
            }
            return Ok(None);
        };
        let (t, target) = s.candidates[choice.candidate];
        if sink.enabled() {
            sink.emit(SquadTrace::Picked {
                tactic: t,
                target: target.map(|i| s.targets[i].subject),
                score: s.scores[choice.candidate],
            });
        }

        // Where the slots face.
        let target_fact = target.map(|i| s.targets[i]);
        let face = match target_fact {
            Some(f) => Some(f.pos),
            None => self.nearest_threat(centre, &s.targets),
        };
        let friend = target.and_then(|i| {
            let me = s.targets[i].pos;
            s.targets
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .min_by_key(|&(j, f)| (self.friend_key(&mut *rules, me, f.pos), j))
                .map(|(_, f)| f.pos)
        });
        let tactic = &self.tactics[t];
        let target_id = target_fact.map(|f| f.subject);
        let h = face.map_or(0, |f| heading(self.metric, centre, f));
        let mut wish_of = |place: &Place, dist: i32, slot: usize, gather: bool| match *place {
            Place::Custom(id) => rules.place(
                id,
                &PlaceCtx {
                    tactic: t,
                    slot,
                    gather,
                    leader,
                    centre,
                    face,
                    friend,
                    target: target_id,
                    heading: h,
                    dist,
                },
            ),
            _ if face.is_some() => {
                wish_point(place, leader, centre, face, friend, self.metric, dist)
            }
            _ => leader,
        };
        s.wishes.clear();
        s.then_wishes.clear();
        for (si, slot) in tactic.slots.iter().enumerate() {
            s.wishes.push(match &slot.gather {
                Some(g) => wish_of(g, slot.gather_dist, si, true),
                None => wish_of(&slot.place, slot.dist, si, false),
            });
            s.then_wishes.push(
                slot.gather
                    .map(|_| wish_of(&slot.place, slot.dist, si, false)),
            );
        }

        // Member costs, once: slot-major, active members with the roles.
        let n = self.group.len();
        s.costs.clear();
        s.costs.resize(tactic.slots.len() * n, None);
        for (si, slot) in tactic.slots.iter().enumerate() {
            let wish = s.wishes[si];
            for (i, m) in self.group.active() {
                if m.roles & slot.roles == slot.roles {
                    let built_in =
                        walk_dist(self.metric, &mut *rules, m.pos, wish).unwrap_or(UNREACHABLE);
                    s.costs[si * n + i] = rules.member_cost(&CostCtx {
                        tactic: t,
                        slot: si,
                        member: *m,
                        wish,
                        built_in,
                    });
                }
            }
        }

        // Members to slots: minimums first, then round by round.
        s.counts.clear();
        s.counts.resize(tactic.slots.len(), 0);
        s.used.clear();
        s.used.resize(self.group.len(), false);
        s.picks.clear();
        for with_roles in [true, false] {
            for (si, slot) in tactic.slots.iter().enumerate() {
                if (slot.roles != 0) != with_roles {
                    continue;
                }
                for _ in 0..slot.min.min(slot.max) {
                    if !self.take_min(s, &tactic.slots, si) {
                        break;
                    }
                }
            }
        }
        loop {
            let mut any = false;
            for (si, slot) in tactic.slots.iter().enumerate() {
                if s.counts[si] < slot.max && self.take(s, si, slot.roles) {
                    any = true;
                }
            }
            if !any {
                break;
            }
        }
        if sink.enabled() {
            for &(mi, si) in &s.picks {
                if let (Some(m), Some(cost)) = (self.group.get(mi), self.cost(s, si, mi)) {
                    sink.emit(SquadTrace::Assigned {
                        slot: si,
                        member: m.id,
                        cost,
                    });
                }
            }
        }

        // Spots and goals.
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        s.taken.clear();
        s.taken.resize(spots.len(), false);
        s.then_taken.clear();
        s.then_taken.resize(spots.len(), false);
        let err = u64::from(self.table.spot_rank_error[level]);
        let spread = u64::from(self.table.timing_spread[level]);
        // Spots are ranked through the spot score slot (see `rank_spots`).
        s.spot_keys.clear();
        s.spot_keys.reserve(spots.len());
        let mut goals = Vec::with_capacity(s.picks.len());
        for &(mi, si) in &s.picks {
            let slot = &tactic.slots[si];
            let first = slot.gather.unwrap_or(slot.place);
            let skip = (draw(seed, SALT_SPOT, mi, now) % (err + 1)) as u8;
            let metric = self.metric;
            let radius = self.spot_radius_of(slot);
            let Some(&member) = self.group.get(mi) else {
                continue;
            };
            let keys = &mut s.spot_keys;
            let mut pick = |wish: Pos, place: &Place, gather: bool, taken: &mut Vec<bool>| {
                let ctx = SpotCtx {
                    tactic: t,
                    slot: si,
                    gather,
                    member,
                    wish,
                    want_tags: slot.want_tags | place.tags(),
                    index: 0,
                    spot: Spot { pos: wish, tags: 0 },
                    target: target_fact,
                    built_in: 0,
                };
                let mut traced = MapSink::new(&mut *sink, |event| SquadTrace::Spot {
                    member: member.id,
                    event,
                });
                let at = (skip, metric, radius);
                match rank_spots(rules, keys, &ctx, spots, taken, at, &mut traced) {
                    SpotPick::Spot(i) => {
                        taken[i] = true;
                        (spots[i].pos, false)
                    }
                    SpotPick::Unchecked(w) => (w, true),
                }
            };
            let gathers = slot.gather.is_some();
            let (pos, unchecked) = pick(s.wishes[si], &first, gathers, &mut s.taken);
            let (then_pos, then_unchecked) = match s.then_wishes[si] {
                Some(w) => {
                    let (p, u) = pick(w, &slot.place, false, &mut s.then_taken);
                    (Some(p), u)
                }
                None => (None, false),
            };
            // A gathering slot first only moves; its own kind and start come
            // with the second order on the signal.
            let (kind, start) = match then_pos {
                Some(_) => (GoalKind::Move, Start::Now),
                None => (slot.kind, slot.start),
            };
            let goal = Goal {
                kind,
                pos: Some(pos),
                unchecked,
                then_pos,
                then_unchecked,
                target: target_fact.map(|f| f.subject),
                start,
                slot: si.min(usize::from(u8::MAX)) as u8,
                plan: id,
                issued_at: now,
                start_delay: (draw(seed, SALT_TIME, mi, now) % (spread + 1)) as u32,
            };
            goals.push((member.id, goal));
        }
        let until = now.wrapping_add(self.table.think_ticks[level]);
        self.pending = Some(Plan {
            id,
            tactic: t,
            target: target_fact.map(|f| f.subject),
            goals,
            made_at: now,
            active_at: until,
        });
        if sink.enabled() {
            sink.emit(SquadTrace::Thinking { plan: id, until });
        }
        Ok(Some(id))
    }

    /// The member cost of member index `i` for slot `si` (see
    /// [`SquadRules::member_cost`]); `None`: it cannot take the slot.
    fn cost(&self, s: &Scratch, si: usize, i: usize) -> Option<i128> {
        s.costs[si * self.group.len() + i]
    }

    /// Gives slot `si` its best free member (lowest member cost, ties to
    /// the lower index); false if none is left.
    fn take(&self, s: &mut Scratch, si: usize, roles: u32) -> bool {
        let best = self
            .group
            .active()
            .filter(|&(i, m)| !s.used[i] && m.roles & roles == roles)
            .filter_map(|(i, _)| self.cost(s, si, i).map(|c| (c, i)))
            .min();
        match best {
            Some((_, i)) => {
                s.used[i] = true;
                s.counts[si] += 1;
                s.picks.push((i, si));
                true
            }
            None => false,
        }
    }

    /// Like [`Squad::take`] while minimums are filled: the cheapest fitting
    /// free member whose pick still leaves every minimum reachable. The
    /// reachability check sees role bits only: a member the cost slot
    /// refuses still counts there, so such a slot may end below its minimum.
    fn take_min(&self, s: &mut Scratch, slots: &[Slot], si: usize) -> bool {
        let roles = slots[si].roles;
        let mut near = std::mem::take(&mut s.near);
        near.clear();
        near.extend(
            self.group
                .active()
                .filter(|&(i, m)| !s.used[i] && m.roles & roles == roles)
                .filter_map(|(i, _)| self.cost(s, si, i).map(|c| (c, i))),
        );
        s.near = near;
        s.near.sort_unstable();
        for k in 0..s.near.len() {
            let i = s.near[k].1;
            s.used[i] = true;
            s.counts[si] += 1;
            if self.mins_reachable(slots, &s.counts, &s.used) {
                s.picks.push((i, si));
                return true;
            }
            s.used[i] = false;
            s.counts[si] -= 1;
        }
        false
    }

    /// The key that picks the friend of the target at `target` (lowest
    /// first): the metric distance to it, with a walking distance the way
    /// from `other` to the target (`i128::MAX` when it cannot get there).
    fn friend_key(&self, rules: &mut dyn SquadRules, target: Pos, other: Pos) -> i128 {
        match rules.walk() {
            None => dist(self.metric, target, other),
            Some(_) => walk_dist(self.metric, rules, other, target).unwrap_or(i128::MAX),
        }
    }

    /// The nearest known target to `centre`, else the nearest danger area's
    /// centre; ties to the earlier one.
    fn nearest_threat(&self, centre: Pos, targets: &[Fact]) -> Option<Pos> {
        let near = |p: Pos| dist(self.metric, centre, p);
        let by_index = |(i, p): (usize, Pos)| (near(p), i);
        targets
            .iter()
            .map(|f| f.pos)
            .enumerate()
            .min_by_key(|&e| by_index(e))
            .or_else(|| {
                self.areas
                    .iter()
                    .map(|a| a.pos)
                    .enumerate()
                    .min_by_key(|&e| by_index(e))
            })
            .map(|(_, p)| p)
    }
}

impl Squad {
    /// True when one of the abort rules of tactic `ctx.tactic` fires on
    /// `ctx.row`, in list order (stops at the first that fires); an
    /// [`Abort::Custom`] asks [`SquadRules::abort`].
    pub(super) fn aborts_fire<S: Sink<SquadTrace>>(
        &self,
        rules: &mut dyn SquadRules,
        ctx: &RowCtx<'_>,
        sink: &mut S,
    ) -> bool {
        let view = SquadView::new(self);
        let target = ctx.target.map(|f| f.subject);
        fires_with(
            &self.tactics[ctx.tactic],
            ctx.row,
            |id| rules.abort(id, &view, ctx),
            |abort, fired| {
                if sink.enabled() {
                    sink.emit(SquadTrace::Abort {
                        tactic: ctx.tactic,
                        target,
                        abort,
                        fired,
                    });
                }
            },
        )
    }
}

/// True when one of `tactic`'s abort rules fires on `row`, without rules
/// (an [`Abort::Custom`] never fires).
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn fires(tactic: &Tactic, row: &[i32]) -> bool {
    fires_with(tactic, row, |_| false, |_, _| {})
}

/// True when one of `tactic`'s abort rules fires on `row`, in list order
/// (stops at the first that fires); `custom` decides an [`Abort::Custom`],
/// `seen` hears each rule's index and answer.
fn fires_with(
    tactic: &Tactic,
    row: &[i32],
    mut custom: impl FnMut(u32) -> bool,
    mut seen: impl FnMut(usize, bool),
) -> bool {
    tactic.aborts.iter().enumerate().any(|(i, ab)| {
        let fired = match *ab {
            Abort::When { input, cmp, value } => {
                row.get(input).is_some_and(|&v| cmp.holds(v, value))
            }
            Abort::Custom(id) => custom(id),
        };
        seen(i, fired);
        fired
    })
}
