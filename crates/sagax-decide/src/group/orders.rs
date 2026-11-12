use super::members::MemberState;
use super::skill::SkillTable;
use super::spots::Spot;
use super::squad::{Active, Goal, PlanError, PlanId, Scratch, Squad, dist, fires, plain_dist};
use super::tactic::{Start, Trigger};
use crate::ONE;
use crate::memory::{Area, Fact, Message, Metric, Pos, SOURCE_OWN, SOURCE_TOLD, arrival};

/// Report kind ([`crate::memory::Message::kind`] for [`super::Squad::report`]):
/// the sender's goal broke. `sender` is the member's id, `data` the plan id
/// the goal belonged to.
pub const REPORT_GOAL_BROKEN: u32 = 1;
/// Report kind: an enemy was seen. `subject`, `pos`, `seen_at`, `strength`
/// and `data` become a fact of the squad's target kind in the leader's
/// knowledge (merged, source [`crate::memory::SOURCE_TOLD`]).
pub const REPORT_ENEMY: u32 = 2;
/// Report kind: the sender was shot at. `pos` is the centre of the danger
/// area; the shooter stood at x = `strength`, y = `data` (both read as
/// `i32`), z = `pos[2]`, which becomes the area's `source`; `subject` (who
/// shot, if known) becomes the area's `data`. Radius, lifetime, kind,
/// strength and blocking come from [`super::Squad::set_danger_area`].
pub const REPORT_DANGER: u32 = 3;

/// A goal on its way to one member (see [`super::Squad::orders_due`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Order {
    /// The member's id.
    pub member: u32,
    /// What it is told to do.
    pub goal: Goal,
}

/// How orders reach a member from the leader. Distances run from the
/// leader's position when the order is sent, in the squad's metric (for
/// [`Metric::Euclid`] the rounded-down root).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Arrives after one tick per unit of distance, only within `range`
    /// (inclusive; for Euclid the squared distance is compared with
    /// `range²`; a negative range reaches nobody).
    Shout {
        /// Farthest distance a shout carries.
        range: i64,
    },
    /// Arrives after `base + distance * per_unit` ticks, at any distance.
    Radio {
        /// Fixed delay.
        base: u32,
        /// Delay per unit of distance.
        per_unit: u32,
    },
    /// Never arrives.
    None,
}

impl Default for Channel {
    /// At once, at any distance (a radio without delay).
    fn default() -> Self {
        Self::Radio {
            base: 0,
            per_unit: 0,
        }
    }
}

impl Channel {
    /// When an order sent at `now` from `from` reaches `to`; `None` when it
    /// does not arrive at all.
    pub fn arrival(self, metric: Metric, from: Pos, to: Pos, now: u32) -> Option<u32> {
        let d = u32::try_from(plain_dist(metric, from, to)).unwrap_or(u32::MAX);
        match self {
            Self::Shout { range } => {
                if range < 0 {
                    return None;
                }
                let limit = if metric == Metric::Euclid {
                    i128::from(range) * i128::from(range)
                } else {
                    i128::from(range)
                };
                (dist(metric, from, to) <= limit).then(|| arrival(now, 0, d, 1))
            }
            Self::Radio { base, per_unit } => Some(arrival(now, base, d, per_unit)),
            Self::None => None,
        }
    }
}

/// What [`super::Squad::check`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Go on as before (also while a new plan is being thought out).
    Keep,
    /// A new plan was made (pending, see [`super::Squad::pending_plan`]).
    Replan(PlanId),
    /// The active plan's trigger fired the signal just now.
    Signal,
    /// No plan is active and none could be made.
    NoPlan,
}

impl Squad {
    /// A member report on its way to the leader, read from `arrive_at` on by
    /// [`Squad::check`] and [`Squad::plan`]. Kinds: [`REPORT_GOAL_BROKEN`]
    /// (counts in [`IN_OFF_PLAN`](super::IN_OFF_PLAN) while the plan it names is active),
    /// [`REPORT_ENEMY`] (a target in the leader's knowledge) and
    /// [`REPORT_DANGER`] (a danger area); other kinds are dropped when read.
    /// False when the inbox refused it (see [`Inbox::push`](crate::memory::Inbox::push)).
    pub fn report(&mut self, msg: Message, arrive_at: u32) -> bool {
        self.inbox.push(msg, arrive_at)
    }

    /// The leader's own sighting: merged into its knowledge
    /// ([`Knowledge::merge`](crate::memory::Knowledge::merge)) with source [`SOURCE_OWN`]. Returns the fact
    /// dropped to make room, if any.
    pub fn observe(&mut self, fact: Fact, now: u32) -> Option<Fact> {
        self.knowledge.merge(
            Fact {
                source: SOURCE_OWN,
                ..fact
            },
            now,
        )
    }

    /// Moves every order whose arrival time has come to the end of `out`, in
    /// the order they were sent; orders for members that are no longer
    /// active (suspended or gone) are dropped. First a pending plan whose
    /// think time has passed becomes active.
    ///
    /// On activation each active member of the plan gets its goal: the
    /// leader at once, the others by their [`Channel`]. Times count from
    /// [`Plan::active_at`](super::Plan::active_at); distances are taken
    /// from the positions at the call that activates the plan (this one,
    /// [`Squad::check`], [`Squad::plan`], [`Squad::signal`] or
    /// [`Squad::arrived`]), so a late call measures from where everyone is
    /// then. A member with
    /// [`Channel::None`], beyond shout range or not active then gets no
    /// order. Orders of the old plan still on their way are dropped.
    pub fn orders_due(&mut self, now: u32, out: &mut Vec<Order>) {
        self.advance(now);
        let group = &self.group;
        self.orders.retain(|(order, at)| {
            let due = now.wrapping_sub(*at) < 1 << 31;
            let active = group
                .index(order.member)
                .and_then(|i| group.get(i))
                .is_some_and(|m| m.state == MemberState::Active);
            if due && active {
                out.push(*order);
            }
            !due
        });
    }

    /// Gives the signal of the active plan at `now`, once per plan. Every
    /// active member whose goal has a `then_pos` or starts
    /// [`Start::OnSignal`] gets a second order, sent now like the first:
    /// `pos` = `then_pos` (`unchecked` = `then_unchecked`), or the old `pos`
    /// without one; same plan and start delay. So that all start at the
    /// same tick, `start` is [`Start::AfterTicks`] of the time between this
    /// member's order and the slowest second order arriving
    /// ([`Start::Now`] for the slowest, and for all when they arrive
    /// together). False when no plan is active or the signal was already
    /// given.
    pub fn signal(&mut self, now: u32) -> bool {
        self.advance(now);
        let Some(a) = self.active.as_mut() else {
            return false;
        };
        if a.signal_at.is_some() {
            return false;
        }
        a.signal_at = Some(now);
        self.send(now, true);
        true
    }

    /// The member with this id reached its first spot (for
    /// [`Trigger::AllArrived`]). False when it has no goal in the active
    /// plan.
    pub fn arrived(&mut self, id: u32, now: u32) -> bool {
        self.advance(now);
        let Some(a) = self.active.as_mut() else {
            return false;
        };
        if !a.has(id) {
            return false;
        }
        if !a.arrived.contains(&id) {
            a.arrived.push(id);
        }
        true
    }

    /// Watches the plans at `now`; call it regularly, with the same `extra`
    /// and `spots` as for [`Squad::plan`] (which it calls to replan, with
    /// `seed`).
    ///
    /// First a pending plan whose think time has passed becomes active, and
    /// the reports that have arrived are read. Then, in this order:
    ///
    /// - A plan is pending: [`Status::Keep`] (no replan while thinking).
    /// - No plan is active: it plans; [`Status::Replan`] with the new id, or
    ///   [`Status::NoPlan`] when nothing scores.
    /// - An [`Abort`](super::Abort) of the active tactic fires on the active
    ///   plan's row: it plans anew ([`Status::Replan`]); when nothing scores
    ///   the active plan and its orders are dropped ([`Status::NoPlan`]).
    /// - Another candidate scores more than the active (tactic, target)
    ///   times `1 + hold_bonus` (from skill, [`ONE`] = 1.0): it plans anew
    ///   ([`Status::Replan`]) among the other candidates only, so a weak
    ///   squad's chance pick cannot restart the same plan. An active pair
    ///   that is no candidate any more (target forgotten, slots no longer
    ///   fillable) scores 0.
    /// - The trigger fires and the signal was not given yet: it gives the
    ///   signal ([`Squad::signal`]) and returns [`Status::Signal`].
    ///   [`Trigger::AllArrived`]: at least one member of the plan is active,
    ///   and every active one called [`Squad::arrived`] or `max_wait` ticks
    ///   passed since activation.
    ///   [`Trigger::TargetInRadius`]: the plan's target, as the leader knows
    ///   it, is within the radius of the group centre. [`Trigger::None`] and
    ///   [`Trigger::Manual`] never fire here.
    /// - Else [`Status::Keep`].
    ///
    /// A new plan is pending first; the old one stays active until then.
    /// Losing a member, also the leader, is no reason by itself: it only
    /// counts in [`IN_LOSSES`](super::IN_LOSSES). With no active member left it keeps the
    /// active plan ([`Status::Keep`]) or, without one, returns
    /// [`Status::NoPlan`].
    pub fn check(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
    ) -> Result<Status, PlanError> {
        self.validate()?;
        self.advance(now);
        self.read_reports(now);
        if self.pending.is_some() {
            return Ok(Status::Keep);
        }
        let Some(centre) = self.centre() else {
            return Ok(if self.active.is_some() {
                Status::Keep
            } else {
                Status::NoPlan
            });
        };
        let verdict = match self.active {
            None => Verdict::Abort,
            Some(_) => {
                let mut s = std::mem::take(&mut self.scratch);
                let v = self.judge(&mut s, now, centre, extra);
                self.scratch = s;
                v?
            }
        };
        match verdict {
            Verdict::Abort => match self.plan_now(now, extra, spots, seed, false)? {
                Some(id) => Ok(Status::Replan(id)),
                None => {
                    self.active = None;
                    self.orders.clear();
                    Ok(Status::NoPlan)
                }
            },
            Verdict::Better => Ok(self
                .plan_now(now, extra, spots, seed, true)?
                .map_or(Status::Keep, Status::Replan)),
            Verdict::Hold if self.trigger_fires(now, centre) => {
                self.signal(now);
                Ok(Status::Signal)
            }
            Verdict::Hold => Ok(Status::Keep),
        }
    }

    /// Makes the pending plan active once its think time has passed, and
    /// sends its orders.
    pub(super) fn advance(&mut self, now: u32) {
        let due = self
            .pending
            .as_ref()
            .is_some_and(|p| now.wrapping_sub(p.made_at) >= p.active_at.wrapping_sub(p.made_at));
        if !due {
            return;
        }
        let Some(plan) = self.pending.take() else {
            return;
        };
        let since = plan.active_at;
        // Orders of the old plan still on their way are void.
        self.orders.clear();
        self.active = Some(Active {
            plan,
            since,
            signal_at: None,
            arrived: Vec::new(),
            broken: Vec::new(),
        });
        self.send(since, false);
    }

    /// Queues one order per active member of the active plan, timed from the
    /// leader's position at `at`: the goals as planned, or (`signal`) the
    /// second orders (see [`Squad::signal`]).
    fn send(&mut self, at: u32, signal: bool) {
        let mut orders = std::mem::take(&mut self.orders);
        let sent_from = orders.len();
        if let (Some(a), Some(leader)) = (&self.active, self.acting_leader()) {
            for &(member, goal) in &a.plan.goals {
                let Some(m) = self.group.index(member).and_then(|i| self.group.get(i)) else {
                    continue;
                };
                if m.state != MemberState::Active {
                    continue;
                }
                let goal = if signal {
                    match on_signal(goal, at) {
                        Some(g) => g,
                        None => continue,
                    }
                } else {
                    goal
                };
                let arrive = if member == leader.id {
                    Some(at)
                } else {
                    self.channel(member)
                        .arrival(self.metric, leader.pos, m.pos, at)
                };
                if let Some(t) = arrive {
                    orders.push((Order { member, goal }, t));
                }
            }
        }
        if signal {
            // Everyone starts when the slowest second order is in.
            let new = &mut orders[sent_from..];
            let last = new.iter().map(|(_, t)| t.wrapping_sub(at)).max();
            let last = last.unwrap_or(0);
            for (order, t) in new {
                let wait = last - t.wrapping_sub(at);
                if wait > 0 {
                    order.goal.start = Start::AfterTicks(wait);
                }
            }
        }
        self.orders = orders;
    }

    /// Reads the reports that have arrived by `now`.
    pub(super) fn read_reports(&mut self, now: u32) {
        let mut due = std::mem::take(&mut self.scratch.reports);
        due.clear();
        self.inbox.take_due(now, &mut due);
        for msg in &due {
            match msg.kind {
                REPORT_GOAL_BROKEN => {
                    if let Some(a) = self.active.as_mut()
                        && a.plan.id == msg.data
                        && a.has(msg.sender)
                        && !a.broken.contains(&msg.sender)
                    {
                        a.broken.push(msg.sender);
                    }
                }
                REPORT_ENEMY => {
                    let fact = Fact {
                        kind: self.target_kind,
                        ..msg.to_fact(SOURCE_TOLD)
                    };
                    self.knowledge.merge(fact, now);
                }
                REPORT_DANGER => {
                    let area = Area {
                        pos: msg.pos,
                        source: [msg.strength as i32, msg.data as i32, msg.pos[2]],
                        has_source: 1,
                        data: msg.subject,
                        ..self.danger
                    };
                    self.areas.add(area, now);
                }
                _ => {}
            }
        }
        self.scratch.reports = due;
    }

    /// Whether the active plan must go (an abort fires), may go (a better
    /// candidate) or stays.
    fn judge(
        &self,
        s: &mut Scratch,
        now: u32,
        centre: Pos,
        extra: &[i32],
    ) -> Result<Verdict, PlanError> {
        let Some(a) = &self.active else {
            return Ok(Verdict::Hold);
        };
        let (t, target) = (a.plan.tactic, a.plan.target);
        s.targets.clear();
        s.targets.extend(self.targets().copied());
        let found = target.and_then(|id| s.targets.iter().position(|f| f.subject == id));
        let fact = found.map(|i| s.targets[i]);
        // The extra block of the active pair; none if its target is gone.
        let block = match (target, found) {
            (None, _) => Some(0),
            (Some(_), Some(i)) => Some(i + 1),
            (Some(_), None) => None,
        };
        self.row_into(&mut s.rows, now, centre, fact.as_ref(), block, extra, true);
        if fires(&self.tactics[t], &s.rows) {
            return Ok(Verdict::Abort);
        }
        self.score_candidates(s, now, centre, extra)?;
        let (mut mine, mut other) = (0u32, 0u32);
        for (&(ct, ci), &score) in s.candidates.iter().zip(&s.scores) {
            if ct == t && ci.map(|i| s.targets[i].subject) == target {
                mine = score;
            } else {
                other = other.max(score);
            }
        }
        let bonus = u64::from(self.table.hold_bonus[SkillTable::level(self.skill)]);
        let bar = u64::from(mine) + u64::from(mine) * bonus / u64::from(ONE);
        Ok(if u64::from(other) > bar {
            Verdict::Better
        } else {
            Verdict::Hold
        })
    }

    /// Whether the active plan's trigger fires now (not after the signal).
    fn trigger_fires(&self, now: u32, centre: Pos) -> bool {
        let Some(a) = &self.active else {
            return false;
        };
        if a.signal_at.is_some() {
            return false;
        }
        match self.tactics[a.plan.tactic].trigger {
            Trigger::AllArrived { max_wait } => {
                let mut active = a
                    .plan
                    .goals
                    .iter()
                    .filter(|(m, _)| self.is_active_member(*m));
                let any = active.clone().next().is_some();
                any && (now.wrapping_sub(a.since) >= max_wait
                    || active.all(|(m, _)| a.arrived.contains(m)))
            }
            Trigger::TargetInRadius(r) => {
                let limit = if self.metric == Metric::Euclid {
                    i128::from(r) * i128::from(r)
                } else {
                    i128::from(r)
                };
                r >= 0
                    && a.plan
                        .target
                        .and_then(|id| self.knowledge.get(id, self.target_kind))
                        .is_some_and(|f| dist(self.metric, centre, f.pos) <= limit)
            }
            Trigger::None | Trigger::Manual => false,
        }
    }
}

/// What [`Squad::check`] makes of the active plan.
enum Verdict {
    /// An abort rule fired (also: no plan is active).
    Abort,
    /// Another candidate beats it by more than the hold bonus.
    Better,
    /// It stays.
    Hold,
}
/// The second order of a goal on the signal: start now at `then_pos` (or at
/// `pos` without one). `None` for a goal that neither gathers nor waits for
/// the signal.
fn on_signal(goal: Goal, at: u32) -> Option<Goal> {
    if goal.then_pos.is_none() && goal.start != Start::OnSignal {
        return None;
    }
    let (pos, unchecked) = match goal.then_pos {
        Some(p) => (Some(p), goal.then_unchecked),
        None => (goal.pos, goal.unchecked),
    };
    Some(Goal {
        pos,
        unchecked,
        then_pos: None,
        then_unchecked: false,
        start: Start::Now,
        issued_at: at,
        ..goal
    })
}
