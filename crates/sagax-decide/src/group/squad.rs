use super::members::{Group, Member, MemberState};
use super::orders::{Channel, Order};
use super::skill::SkillTable;
use super::spots::{Spot, SpotPick, pick_spot, wish_point};
use super::tactic::{GoalKind, Slot, Start, Tactic};
use crate::memory::{
    Area, Areas, Evict, Fact, Inbox, Knowledge, Message, Metric, Pos, ZeroCapacity, distance,
};
use crate::{Candidate, Decider, INPUT_SINCE_OPTION, choose, mix};

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
/// `i32::MAX`).
pub const IN_TARGET_DIST: usize = 3;
/// Built-in row column: active members with [`ROLE_RANGED`].
pub const IN_RANGED: usize = 4;
/// Built-in row column: active members with [`ROLE_AREA`].
pub const IN_AREA: usize = 5;
/// Built-in row column: ticks since the active plan became active, in the
/// row of the active (tactic, target); 0 in the row of any other candidate.
pub const IN_PLAN_AGE: usize = 6;
/// Built-in row column: distance from the target to its nearest other known
/// target, measured as [`IN_TARGET_DIST`]; -1 without a target or without
/// another known target (a curve starting at 0 reads it as 0).
pub const IN_TARGET_FRIEND_DIST: usize = 7;
/// First caller column (see [`Squad::set_extra_columns`]).
pub const IN_EXTRA: usize = 8;
/// Most caller columns a row may have ([`Squad::set_extra_columns`]).
pub const MAX_EXTRA_COLUMNS: usize = 1024;

/// Role bit counted by [`IN_RANGED`]: fights from a distance.
pub const ROLE_RANGED: u32 = 1;
/// Role bit counted by [`IN_AREA`]: hits an area (throws into crowds).
pub const ROLE_AREA: u32 = 2;

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
    /// has only [`IN_EXTRA`] + [`Squad::set_extra_columns`] columns.
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
    pub(super) metric: Metric,
    pub(super) tactics: Vec<Tactic>,
    pub(super) decider: Decider,
    pub(super) skill: u8,
    pub(super) table: SkillTable,
    pub(super) leader: Option<u32>,
    pub(super) target_kind: u32,
    pub(super) extra_columns: usize,
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
    /// A squad for at most `group_cap` members, `memory_cap` facts,
    /// `area_cap` areas and `inbox_cap` waiting reports, measuring in
    /// `metric`. The knowledge drops the oldest fact when full.
    pub fn new(
        group_cap: usize,
        memory_cap: usize,
        area_cap: usize,
        inbox_cap: usize,
        metric: Metric,
    ) -> Result<Self, ZeroCapacity> {
        Ok(Self {
            group: Group::new(group_cap)?,
            knowledge: Knowledge::new(memory_cap, Evict::Oldest)?,
            areas: Areas::new(area_cap)?,
            inbox: Inbox::new(inbox_cap)?,
            metric,
            tactics: Vec::new(),
            decider: Decider::new(),
            skill: 0,
            table: SkillTable::default(),
            leader: None,
            target_kind: 0,
            extra_columns: 0,
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
            self.decider.add_consideration(option, c);
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

    /// Replaces the skill table (default [`SkillTable::default`]).
    pub fn set_skill_table(&mut self, table: SkillTable) {
        self.table = table;
    }

    /// The skill table.
    pub fn skill_table(&self) -> &SkillTable {
        &self.table
    }

    /// Makes the member with this id the leader; false if it is not in the
    /// group.
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

    /// The area a [`REPORT_DANGER`](super::REPORT_DANGER) adds: its kind, radius, strength,
    /// lifetime and blocking are used; centre, source and data come from the
    /// report. Default: kind 0, radius 2 (two cells), strength 1, lifetime
    /// 255 ticks (15 s at about 17 ticks per second), not blocking.
    pub fn set_danger_area(&mut self, template: Area) {
        self.danger = template;
    }

    /// The area a [`REPORT_DANGER`](super::REPORT_DANGER) adds (see [`Squad::set_danger_area`]).
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
    /// has more than 255 slots. [`Squad::plan`] and [`Squad::check`] run
    /// this first. (An [`Abort`](super::Abort) on a column beyond the row
    /// never fires.)
    pub fn validate(&self) -> Result<(), PlanError> {
        let stride = IN_EXTRA + self.extra_columns;
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

    /// How many caller columns each scoring row has after the built-in ones
    /// (from [`IN_EXTRA`] on; default 0). See [`Squad::plan`] for the
    /// layout of `extra`. False (nothing changes) above
    /// [`MAX_EXTRA_COLUMNS`].
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
    /// block `block` (`None`: zeros). `active` marks the row of the active
    /// (tactic, target), which reads the active plan's off-plan count and
    /// age; other rows read 0 there.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn row_into(
        &self,
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
        row.clear();
        row.resize(IN_EXTRA + self.extra_columns, 0);
        row[IN_LOSSES] = to_i32(
            self.group
                .iter()
                .filter(|m| m.state == MemberState::Gone)
                .count(),
        );
        row[IN_RANGED] = to_i32(count_role(ROLE_RANGED));
        row[IN_AREA] = to_i32(count_role(ROLE_AREA));
        row[IN_TARGET_AGE] = i32::MAX;
        row[IN_TARGET_DIST] = i32::MAX;
        row[IN_TARGET_FRIEND_DIST] = -1;
        if let Some(f) = target {
            row[IN_TARGET_AGE] = clamp_i32(i128::from(now.wrapping_sub(f.seen_at)));
            row[IN_TARGET_DIST] = plain_dist(self.metric, centre, f.pos);
            if let Some(d) = self
                .targets()
                .filter(|o| o.subject != f.subject)
                .map(|o| plain_dist(self.metric, f.pos, o.pos))
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
                row[IN_EXTRA + c] = at.and_then(|i| extra.get(i)).copied().unwrap_or(0);
            }
        }
    }

    /// Fills `s.candidates` and `s.scores` from `s.targets` (see
    /// [`Squad::plan`]).
    pub(super) fn score_candidates(
        &self,
        s: &mut Scratch,
        now: u32,
        centre: Pos,
        extra: &[i32],
    ) -> Result<(), PlanError> {
        s.candidates.clear();
        for (t, tactic) in self.tactics.iter().enumerate() {
            if tactic.min_skill > self.skill || !self.mins_reachable(&tactic.slots, &[], &[]) {
                continue;
            }
            if tactic.needs_target {
                s.candidates
                    .extend((0..s.targets.len()).map(|i| (t, Some(i))));
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
                &mut s.rows,
                now,
                centre,
                fact.as_ref(),
                block,
                extra,
                active,
            );
            let score = if fires(&self.tactics[t], &s.rows) {
                0
            } else {
                // `validate` ruled out the only error a row can cause.
                self.decider
                    .score(Candidate::new(t as u32, id.unwrap_or(0)), &s.rows)
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
    /// all its role bits; among those the nearest to the slot's wish point,
    /// ties to the lower member index. While minimums are filled, a member
    /// is skipped if taking it would leave another slot's minimum
    /// unreachable. Members get spots in assignment order through
    /// [`pick_spot`] with `slot.want_tags | place.tags()`; with weak skill a
    /// spot may be up to `spot_rank_error` ranks off the best (from `seed`).
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
    /// Returns the new plan's id, or `Ok(None)` (pending plan unchanged)
    /// when there is no active member or no candidate scores above 0. A
    /// consideration that reads beyond the row is an error
    /// ([`Squad::validate`]), checked before anything else.
    pub fn plan(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
    ) -> Result<Option<PlanId>, PlanError> {
        self.validate()?;
        self.advance(now);
        self.read_reports(now);
        self.areas.expire(now);
        self.plan_now(now, extra, spots, seed, None)
    }

    /// [`Squad::plan`] without the first steps. With a `bar` every
    /// candidate scoring at most `bar` is out, so a replan for a better plan
    /// picks only among the clearly better ones (never the active pair,
    /// which scores at most the bar).
    pub(super) fn plan_now(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        bar: Option<u64>,
    ) -> Result<Option<PlanId>, PlanError> {
        let mut s = std::mem::take(&mut self.scratch);
        let made = self.plan_with(&mut s, now, extra, spots, seed, bar);
        self.scratch = s;
        made
    }

    fn plan_with(
        &mut self,
        s: &mut Scratch,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        bar: Option<u64>,
    ) -> Result<Option<PlanId>, PlanError> {
        let (Some(leader), Some(centre)) = (self.acting_leader().map(|m| m.pos), self.centre())
        else {
            return Ok(None);
        };
        let level = SkillTable::level(self.skill);

        s.targets.clear();
        s.targets.extend(self.targets().copied());
        self.score_candidates(s, now, centre, extra)?;
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
            return Ok(None);
        };
        let (t, target) = s.candidates[choice.candidate];

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
                .min_by_key(|&(j, f)| (dist(self.metric, me, f.pos), j))
                .map(|(_, f)| f.pos)
        });
        let tactic = &self.tactics[t];
        let wish_of = |place: &super::Place, dist: i32| match face {
            Some(_) => wish_point(place, leader, centre, face, friend, self.metric, dist),
            None => leader,
        };
        s.wishes.clear();
        s.then_wishes.clear();
        for slot in &tactic.slots {
            s.wishes.push(match &slot.gather {
                Some(g) => wish_of(g, slot.gather_dist),
                None => wish_of(&slot.place, slot.dist),
            });
            s.then_wishes
                .push(slot.gather.map(|_| wish_of(&slot.place, slot.dist)));
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

        // Spots and goals.
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        s.taken.clear();
        s.taken.resize(spots.len(), false);
        s.then_taken.clear();
        s.then_taken.resize(spots.len(), false);
        let err = u64::from(self.table.spot_rank_error[level]);
        let spread = u64::from(self.table.timing_spread[level]);
        let mut goals = Vec::with_capacity(s.picks.len());
        for &(mi, si) in &s.picks {
            let slot = &tactic.slots[si];
            let first = slot.gather.unwrap_or(slot.place);
            let skip = (draw(seed, SALT_SPOT, mi, now) % (err + 1)) as u8;
            let metric = self.metric;
            let radius = self.spot_radius_of(slot);
            let pick = |wish: Pos, place: &super::Place, taken: &mut Vec<bool>| {
                let want = slot.want_tags | place.tags();
                match pick_spot(wish, want, spots, taken, skip, metric, radius) {
                    SpotPick::Spot(i) => {
                        taken[i] = true;
                        (spots[i].pos, false)
                    }
                    SpotPick::Unchecked(w) => (w, true),
                }
            };
            let (pos, unchecked) = pick(s.wishes[si], &first, &mut s.taken);
            let (then_pos, then_unchecked) = match s.then_wishes[si] {
                Some(w) => {
                    let (p, u) = pick(w, &slot.place, &mut s.then_taken);
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
            let member = self.group.get(mi).map_or(0, |m| m.id);
            goals.push((member, goal));
        }
        self.pending = Some(Plan {
            id,
            tactic: t,
            target: target_fact.map(|f| f.subject),
            goals,
            made_at: now,
            active_at: now.wrapping_add(self.table.think_ticks[level]),
        });
        Ok(Some(id))
    }

    /// Gives slot `si` its best free member; false if none is left.
    fn take(&self, s: &mut Scratch, si: usize, roles: u32) -> bool {
        let wish = s.wishes[si];
        let best = self
            .group
            .active()
            .filter(|&(i, m)| !s.used[i] && m.roles & roles == roles)
            .map(|(i, m)| (dist(self.metric, m.pos, wish), i))
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

    /// Like [`Squad::take`] while minimums are filled: the nearest fitting
    /// free member whose pick still leaves every minimum reachable.
    fn take_min(&self, s: &mut Scratch, slots: &[Slot], si: usize) -> bool {
        let (wish, roles) = (s.wishes[si], slots[si].roles);
        s.near.clear();
        let used = &s.used;
        s.near.extend(
            self.group
                .active()
                .filter(|&(i, m)| !used[i] && m.roles & roles == roles)
                .map(|(i, m)| (dist(self.metric, m.pos, wish), i)),
        );
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

/// True when one of `tactic`'s abort rules fires on `row`.
pub(super) fn fires(tactic: &Tactic, row: &[i32]) -> bool {
    tactic
        .aborts
        .iter()
        .any(|ab| row.get(ab.input).is_some_and(|&v| v >= ab.at_least))
}
