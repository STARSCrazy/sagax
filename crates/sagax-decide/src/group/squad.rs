use super::members::{Group, Member};
use super::skill::SkillTable;
use super::spots::{Spot, SpotPick, pick_spot, wish_point};
use super::tactic::{GoalKind, Start, Tactic};
use crate::memory::{Areas, Evict, Fact, Inbox, Knowledge, Metric, Pos, ZeroCapacity, distance};
use crate::{Candidate, Decider, INPUT_SINCE_OPTION, choose, mix};

/// Built-in row column: own losses (members marked gone).
pub const IN_LOSSES: usize = 0;
/// Built-in row column: members off plan.
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
/// Built-in row column: ticks since the pending plan was made (none:
/// `i32::MAX`).
pub const IN_PLAN_AGE: usize = 6;
/// First caller column (see [`Squad::set_extra_columns`]).
pub const IN_EXTRA: usize = 7;

/// Role bit counted by [`IN_RANGED`]: fights from a distance.
pub const ROLE_RANGED: u32 = 1;
/// Role bit counted by [`IN_AREA`]: hits an area (throws into crowds).
pub const ROLE_AREA: u32 = 2;

/// The number of a plan; each new plan gets a higher one (wrapping).
pub type PlanId = u32;

/// What one member is told to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Goal {
    /// What to do.
    pub kind: GoalKind,
    /// Where: a candidate spot, or the wish point itself when `unchecked`.
    /// [`Squad::plan`] always sets it.
    pub pos: Option<Pos>,
    /// True when no free spot was there and `pos` is the plain wish point,
    /// not checked by the game.
    pub unchecked: bool,
    /// Where to go after `pos` when the slot gathers first: `pos` is then
    /// the gather spot and this the spot of the slot's place (picked the
    /// same way; possibly an unchecked wish point). `None` without a gather
    /// place.
    pub then_pos: Option<Pos>,
    /// The target's id, if the tactic has one.
    pub target: Option<u32>,
    /// When to start.
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

/// A plan made by [`Squad::plan`], waiting to become active.
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
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputOutOfRange { tactic, input } => {
                write!(f, "tactic {tactic} reads column {input}, beyond the row")
            }
        }
    }
}

impl std::error::Error for PlanError {}

/// Salts that keep the seeded draws apart.
const SALT_SPOT: u64 = 0x5350_4f54_0000_0001;
const SALT_TIME: u64 = 0x5449_4d45_0000_0002;

/// A seeded number for one member at one tick.
fn draw(seed: u64, salt: u64, member: usize, now: u32) -> u64 {
    mix(mix(seed ^ salt) ^ member as u64 ^ (u64::from(now) << 32))
}

fn clamp_i32(v: i128) -> i32 {
    v.clamp(0, i128::from(i32::MAX)) as i32
}

/// Distance in the metric between two points (x, y); for Euclid the
/// squared value, as [`distance`].
fn dist(metric: Metric, a: Pos, b: Pos) -> i128 {
    distance(
        metric,
        i64::from(b[0]) - i64::from(a[0]),
        i64::from(b[1]) - i64::from(a[1]),
    )
}

/// Plain distance for a row: the root for Euclid.
fn plain_dist(metric: Metric, a: Pos, b: Pos) -> i32 {
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
struct Scratch {
    targets: Vec<Fact>,
    candidates: Vec<(usize, Option<usize>)>,
    rows: Vec<i32>,
    scores: Vec<u32>,
    order: Vec<usize>,
    wishes: Vec<Pos>,
    then_wishes: Vec<Option<Pos>>,
    counts: Vec<u8>,
    used: Vec<bool>,
    picks: Vec<(usize, usize)>,
    taken: Vec<bool>,
    then_taken: Vec<bool>,
}

/// The squad leader: the group, what the leader knows, the tactics it can
/// use and the plan it made.
///
/// Targets come only from the leader's own [`Knowledge`] (facts of the kind
/// set by [`Squad::set_target_kind`]), never from the caller, so a group
/// cannot plan with what it has not seen. The caller only adds extra
/// columns to the scoring rows.
#[derive(Clone, Debug)]
pub struct Squad {
    group: Group,
    knowledge: Knowledge,
    areas: Areas,
    #[allow(dead_code, reason = "member reports arrive here (next step)")]
    inbox: Inbox,
    metric: Metric,
    tactics: Vec<Tactic>,
    decider: Decider,
    skill: u8,
    table: SkillTable,
    leader: Option<u32>,
    target_kind: u32,
    extra_columns: usize,
    pending: Option<Plan>,
    next_id: PlanId,
    scratch: Scratch,
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

    /// The leader's id, as set.
    pub fn leader(&self) -> Option<u32> {
        self.leader
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

    /// Checks that every consideration of every tactic reads a column the
    /// rows have (or one of the decider's memory inputs). [`Squad::plan`]
    /// runs this first.
    pub fn validate(&self) -> Result<(), PlanError> {
        let stride = IN_EXTRA + self.extra_columns;
        for (t, tactic) in self.tactics.iter().enumerate() {
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
    /// layout of `extra`.
    pub fn set_extra_columns(&mut self, count: usize) {
        self.extra_columns = count;
    }

    /// The plan made last by [`Squad::plan`], if any.
    pub fn pending_plan(&self) -> Option<&Plan> {
        self.pending.as_ref()
    }

    /// The member that leads: the set leader while active, else the first
    /// active member.
    fn acting_leader(&self) -> Option<&Member> {
        let set = self
            .leader
            .and_then(|id| self.group.index(id))
            .and_then(|i| self.group.get(i))
            .filter(|m| m.state == super::MemberState::Active);
        set.or_else(|| self.group.active().map(|(_, m)| m).next())
    }

    /// The mean position of the active members (rounded down).
    fn centre(&self) -> Option<Pos> {
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

    /// Makes a new plan and keeps it as the pending one.
    ///
    /// **Candidates** are every tactic the skill knows ([`Tactic::min_skill`]
    /// `<= skill`) whose slots can all reach their minimum: enough active
    /// members for the sum of the minimums, and for each slot enough active
    /// members with all its role bits. Once per known target (see [`Squad::targets`]), or once
    /// without a target if it does not need one. Ties go to the earlier
    /// candidate (tactic order, then target order).
    ///
    /// **Rows:** each candidate is scored with its tactic's considerations
    /// over a row of the built-in inputs ([`IN_LOSSES`] ..= [`IN_PLAN_AGE`])
    /// followed by the caller's extra columns. `extra` holds one block of
    /// [`Squad::set_extra_columns`] values for the candidates without a
    /// target, then one block per known target in [`Squad::targets`] order;
    /// missing values read as 0. The pick is the skill's [`Pick`](crate::Pick)
    /// with `seed`.
    ///
    /// **Assignment:** first every slot gets its minimum (slots with role
    /// bits first, in slot order, then the others, so a slot open to anyone
    /// does not use up a needed specialist), then the slots in order take
    /// one more member each, round by round, up to their maximum. A slot
    /// only takes free active members (suspended and gone are skipped) with
    /// all its role bits; among those the nearest to the slot's wish point,
    /// ties to the lower member index. Members get spots in assignment order through
    /// [`pick_spot`] with `slot.want_tags | place.tags()`; with weak skill a
    /// spot may be up to `spot_rank_error` ranks off the best (from `seed`).
    ///
    /// **Wish points** ([`wish_point`]) face the target; the goal's `pos` is
    /// the slot's `gather` place when it has one (then `then_pos` is its
    /// `place`), else its `place`. Gather and place spots are taken
    /// separately, so both phases can use the same spots. Between
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
        let mut s = std::mem::take(&mut self.scratch);
        let made = self.plan_with(&mut s, now, extra, spots, seed);
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
    ) -> Result<Option<PlanId>, PlanError> {
        let (Some(leader), Some(centre)) = (self.acting_leader().map(|m| m.pos), self.centre())
        else {
            return Ok(None);
        };
        let level = SkillTable::level(self.skill);

        s.targets.clear();
        s.targets.extend(self.targets().copied());

        // Candidates: tactic x target.
        let active = self.group.active().count();
        s.candidates.clear();
        for (t, tactic) in self.tactics.iter().enumerate() {
            let need: usize = tactic.slots.iter().map(|sl| usize::from(sl.min)).sum();
            let fits = |roles: u32| {
                self.group
                    .active()
                    .filter(|(_, m)| m.roles & roles == roles)
                    .count()
            };
            let short = tactic
                .slots
                .iter()
                .any(|sl| fits(sl.roles) < usize::from(sl.min));
            if tactic.min_skill > self.skill || need > active || short {
                continue;
            }
            if tactic.needs_target {
                s.candidates
                    .extend((0..s.targets.len()).map(|i| (t, Some(i))));
            } else {
                s.candidates.push((t, None));
            }
        }

        // Rows and scores.
        let stride = IN_EXTRA + self.extra_columns;
        let losses = self
            .group
            .iter()
            .filter(|m| m.state == super::MemberState::Gone)
            .count();
        let count_role = |bit: u32| {
            self.group
                .active()
                .filter(|(_, m)| m.roles & bit == bit)
                .count()
        };
        let (ranged, area) = (count_role(ROLE_RANGED), count_role(ROLE_AREA));
        let plan_age = self.pending.as_ref().map_or(i32::MAX, |p| {
            clamp_i32(i128::from(now.wrapping_sub(p.made_at)))
        });
        let to_i32 = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
        s.scores.clear();
        for &(t, target) in &s.candidates {
            s.rows.clear();
            s.rows.resize(stride, 0);
            let row = &mut s.rows;
            row[IN_LOSSES] = to_i32(losses);
            row[IN_OFF_PLAN] = 0;
            row[IN_RANGED] = to_i32(ranged);
            row[IN_AREA] = to_i32(area);
            row[IN_PLAN_AGE] = plan_age;
            row[IN_TARGET_AGE] = i32::MAX;
            row[IN_TARGET_DIST] = i32::MAX;
            if let Some(i) = target {
                let f = &s.targets[i];
                row[IN_TARGET_AGE] = clamp_i32(i128::from(now.wrapping_sub(f.seen_at)));
                row[IN_TARGET_DIST] = plain_dist(self.metric, centre, f.pos);
            }
            let block = target.map_or(0, |i| i + 1) * self.extra_columns;
            for c in 0..self.extra_columns {
                row[IN_EXTRA + c] = extra.get(block + c).copied().unwrap_or(0);
            }
            let id = target.map_or(0, |i| s.targets[i].subject);
            // `validate` ruled out the only error a row can cause.
            let score = self
                .decider
                .score(Candidate::new(t as u32, id), row)
                .map_err(|e| PlanError::InputOutOfRange {
                    tactic: t,
                    input: match e {
                        crate::DecideError::InputOutOfRange { input, .. } => input,
                        _ => u32::MAX,
                    },
                })?;
            s.scores.push(score);
        }
        let Some(choice) = choose(&s.scores, self.table.pick[level], seed, &mut s.order) else {
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
            let first = slot.gather.unwrap_or(slot.place);
            s.wishes.push(wish_of(&first, slot.dist));
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
                    if !self.take(s, si, slot.roles) {
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
            let pick = |wish: Pos, place: &super::Place, taken: &mut Vec<bool>| {
                let want = slot.want_tags | place.tags();
                match pick_spot(wish, want, spots, taken, skip, metric) {
                    SpotPick::Spot(i) => {
                        taken[i] = true;
                        (spots[i].pos, false)
                    }
                    SpotPick::Unchecked(w) => (w, true),
                }
            };
            let (pos, unchecked) = pick(s.wishes[si], &first, &mut s.taken);
            let then_pos = s.then_wishes[si].map(|w| pick(w, &slot.place, &mut s.then_taken).0);
            let goal = Goal {
                kind: slot.kind,
                pos: Some(pos),
                unchecked,
                then_pos,
                target: target_fact.map(|f| f.subject),
                start: slot.start,
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
