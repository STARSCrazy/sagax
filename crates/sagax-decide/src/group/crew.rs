//! The [`Crew`]: a [`Squad`] with one [`GoalKeeper`] per member, run as one
//! loop per tick that reports what happened as [`CrewEvent`]s.

use super::keeper::{GoalKeeper, KeeperInputs, KeeperTrace};
use super::members::{Member, MemberState};
use super::orders::{Order, Status};
use super::rules::{
    BuiltInRules, ChannelCtx, CostCtx, PlaceCtx, ReportAction, ReportActions, RowCtx, SpotCtx,
    SquadRules, SquadView,
};
use super::spots::Spot;
use super::squad::{Goal, PlanError, PlanId, Squad, SquadConfig, SquadTrace};
use crate::memory::{Area, Fact, Message, Metric, Pos, distance};
use sagax_core::{HexLayout, MapSink, NoopSink, Sink};

/// The trace record of a [`Crew`]: what its `*_traced` calls decided
/// ([`Crew::tick_traced`], [`Crew::observe_traced`],
/// [`Crew::report_traced`], [`Crew::reinstate_traced`]). The
/// [`CrewEvent`]s of a tick are not repeated here. With a hex layout the
/// positions are offset coordinates, as the crew's other output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CrewTrace {
    /// All squad work of the call, unchanged.
    Squad(SquadTrace),
    /// A keeper was checked for giving its goal up (tick step 7).
    Keeper {
        /// The member's id.
        member: u32,
        /// The keeper's record.
        event: KeeperTrace,
    },
    /// The crew sent a member report to the leader over the member's
    /// channel (tick steps 6 and 7, [`Crew::report_traced`]).
    Sent {
        /// The report.
        msg: Message,
        /// Ticks until it reaches the leader; `None`: it never arrives and
        /// nothing was sent.
        delay: Option<u32>,
        /// True when the inbox took it.
        queued: bool,
    },
}

/// Wraps squad records for a crew: positions to offset coordinates with a
/// hex layout.
fn wrap(hex: Option<HexLayout>) -> impl Fn(SquadTrace) -> CrewTrace {
    move |e| {
        CrewTrace::Squad(match hex {
            Some(l) => e.map_pos(|p| l.from_axial_pos(p)),
            None => e,
        })
    }
}

/// Capacities and options of a [`Crew`] (see [`Crew::new`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CrewConfig {
    /// The squad's capacities and metric.
    pub squad: SquadConfig,
    /// How far from its first spot a member counts as arrived, in the
    /// squad's metric (for [`Metric::Euclid`] the squared distance is
    /// compared with the radius squared). Default 0: on the spot.
    pub arrive_radius: u32,
    /// The map's hex layout: the crew takes and returns offset coordinates
    /// in this layout and keeps the squad in axial ones (needs
    /// [`Metric::Hex`]). Default `None`: positions pass unchanged.
    pub hex_layout: Option<HexLayout>,
}

/// Why [`Crew::new`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CrewError {
    /// A capacity of the squad config is 0.
    ZeroCapacity,
    /// A hex layout is set but the squad's metric is not [`Metric::Hex`].
    HexNeedsHexMetric,
}

impl std::fmt::Display for CrewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroCapacity => write!(f, "a crew capacity is 0"),
            Self::HexNeedsHexMetric => write!(f, "a hex layout needs the hex metric"),
        }
    }
}

impl std::error::Error for CrewError {}

/// What happened in one [`Crew::tick`], in the order of the tick's steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CrewEvent {
    /// A new plan is pending (the first one or a replacement).
    Planned(PlanId),
    /// That plan became active (its think time is over).
    Activated(PlanId),
    /// The leader gave the signal.
    Signal,
    /// An order reached a member; its keeper holds the goal now.
    Order {
        /// The member's id.
        member: u32,
        /// The goal (offset coordinates with a hex layout).
        goal: Goal,
        /// The tick it starts ([`Goal::start_tick`] of the arrival tick);
        /// `None` when it starts on the signal.
        start_at: Option<u32>,
    },
    /// A member's start tick has come (once per order).
    Start {
        /// The member's id.
        member: u32,
    },
    /// A member stands on its first spot and sent the leader a
    /// [`Message::arrived`] (once per goal); the leader learns it later.
    Arrived {
        /// The member's id.
        member: u32,
    },
    /// A member gave its goal up and sent the leader a
    /// [`Message::goal_broken`] (once per goal).
    Broken {
        /// The member's id.
        member: u32,
    },
    /// No plan is active and none could be made.
    NoPlan,
}

/// One member's keeper and what the crew already told about its goal.
#[derive(Clone, Copy, Debug)]
struct Seat {
    id: u32,
    keeper: GoalKeeper,
    /// The keeper is suspended (its member is not active).
    paused: bool,
    start_at: Option<u32>,
    started: bool,
    arrived: bool,
    broken: bool,
}

impl Seat {
    const fn new(id: u32) -> Self {
        Self {
            id,
            keeper: GoalKeeper::new(),
            paused: false,
            start_at: None,
            started: false,
            arrived: false,
            broken: false,
        }
    }

    /// Suspends or resumes the keeper when that changes.
    fn set_paused(&mut self, paused: bool, now: u32) {
        if paused == self.paused {
            return;
        }
        self.paused = paused;
        if paused {
            self.keeper.suspend();
        } else {
            self.keeper.unsuspend(now);
        }
    }
}

/// A [`Squad`] with one [`GoalKeeper`] per member: the whole group loop in
/// one call per tick.
///
/// Per tick the game sets where its members are ([`Crew::set_pos`]), calls
/// [`Crew::tick`], reads the events, and for each member asks
/// [`Crew::member_inputs`] for its own decider row and, once the member
/// decided, calls [`Crew::member_chose`]. The crew moves nobody.
///
/// Every step is a public call of [`Squad`] or [`GoalKeeper`] (see
/// [`Crew::tick`]); both stay reachable ([`Crew::squad_mut`],
/// [`Crew::keeper_mut`]) for whatever the crew does not wrap: channels,
/// tactics, skill, leader, member reports.
///
/// **Hex:** with [`CrewConfig::hex_layout`] the crew's own calls take and
/// return offset coordinates; the squad and the keepers hold axial ones.
/// Calls on [`Crew::squad_mut`] or [`Crew::keeper`] go around the
/// conversion and use axial coordinates.
///
/// Capacities are fixed at [`Crew::new`]; after the first ticks (the spot
/// buffer for hex grows to the largest spot list once) the crew allocates
/// nothing beyond what the squad does.
#[derive(Clone, Debug)]
pub struct Crew {
    squad: Squad,
    seats: Vec<Seat>,
    metric: Metric,
    arrive_radius: u32,
    hex: Option<HexLayout>,
    orders: Vec<Order>,
    spots: Vec<Spot>,
}

impl Crew {
    /// A crew with an empty squad of `config.squad`. Errors on a capacity
    /// of 0 or on a hex layout without [`Metric::Hex`].
    pub fn new(config: CrewConfig) -> Result<Self, CrewError> {
        if config.hex_layout.is_some() && config.squad.metric != Metric::Hex {
            return Err(CrewError::HexNeedsHexMetric);
        }
        let squad = Squad::new(config.squad).map_err(|_| CrewError::ZeroCapacity)?;
        Ok(Self {
            squad,
            seats: Vec::with_capacity(config.squad.members),
            metric: config.squad.metric,
            arrive_radius: config.arrive_radius,
            hex: config.hex_layout,
            orders: Vec::with_capacity(config.squad.members),
            spots: Vec::new(),
        })
    }

    /// The squad.
    pub fn squad(&self) -> &Squad {
        &self.squad
    }

    /// The squad, for what the crew does not wrap (channels, tactics,
    /// skill, leader, member reports). Positions here are axial with a hex
    /// layout. Members added or removed here get or lose their keeper at
    /// the next [`Crew::tick`].
    ///
    /// Do not call [`Squad::orders_due`] on a crew's squad: the crew takes
    /// the orders in [`Crew::tick`], and orders taken elsewhere never reach
    /// the keepers.
    pub fn squad_mut(&mut self) -> &mut Squad {
        &mut self.squad
    }

    /// The keeper of the member with this id (its goal in axial
    /// coordinates with a hex layout; [`Crew::goal`] converts).
    pub fn keeper(&self, id: u32) -> Option<&GoalKeeper> {
        self.seat(id).map(|s| &s.keeper)
    }

    /// The keeper of the member with this id, to change it by hand.
    pub fn keeper_mut(&mut self, id: u32) -> Option<&mut GoalKeeper> {
        self.seats
            .iter_mut()
            .find(|s| s.id == id)
            .map(|s| &mut s.keeper)
    }

    /// Adds a member (position in offset coordinates with a hex layout)
    /// with an empty keeper; its index in the group, or `None` when the
    /// group is full or the id is taken.
    pub fn add_member(&mut self, member: Member) -> Option<usize> {
        let pos = self.pos_in(member.pos);
        let i = self.squad.group_mut().add(Member { pos, ..member })?;
        self.sync_seats();
        Some(i)
    }

    /// Removes a member and its keeper; false when the id is unknown.
    pub fn remove_member(&mut self, id: u32) -> bool {
        let known = self.squad.group_mut().remove(id);
        self.sync_seats();
        known
    }

    /// Moves a member (offset coordinates with a hex layout); false when
    /// the id is unknown.
    pub fn set_pos(&mut self, id: u32, pos: Pos) -> bool {
        let pos = self.pos_in(pos);
        self.squad.group_mut().set_pos(id, pos)
    }

    /// Takes a member out of play: suspended in the group, its keeper
    /// paused (never broken). False when the id is unknown.
    pub fn suspend(&mut self, id: u32) -> bool {
        if !self.squad.group_mut().suspend(id) {
            return false;
        }
        self.pause(id, true, 0);
        true
    }

    /// Puts a member back into play ([`Squad::reinstate`]: it gets its
    /// current goal again) and restarts its keeper's clock at `now`. When
    /// the active plan has no goal for it (none is active, or the plan was
    /// made without it), its keeper is cleared: it has no goal until an
    /// order comes. False when the id is unknown.
    pub fn reinstate(&mut self, id: u32, now: u32) -> bool {
        let known = self.squad.reinstate(id, now);
        self.reinstated(id, now, known)
    }

    /// [`Crew::reinstate`] with the game's rules ([`Squad::reinstate_with`]:
    /// the goal goes over a [`Channel::Custom`](super::Channel::Custom) too;
    /// the slots see offset coordinates with a hex layout).
    pub fn reinstate_with(&mut self, id: u32, now: u32, rules: &mut dyn SquadRules) -> bool {
        self.reinstate_traced(id, now, rules, &mut NoopSink)
    }

    /// [`Crew::reinstate_with`] with a trace ([`Squad::reinstate_traced`],
    /// wrapped in [`CrewTrace::Squad`]).
    pub fn reinstate_traced<S: Sink<CrewTrace>>(
        &mut self,
        id: u32,
        now: u32,
        rules: &mut dyn SquadRules,
        sink: &mut S,
    ) -> bool {
        let mut squad = MapSink::new(sink, wrap(self.hex));
        let known = match self.hex {
            Some(layout) => {
                self.squad
                    .reinstate_traced(id, now, &mut HexRules::new(rules, layout), &mut squad)
            }
            None => self.squad.reinstate_traced(id, now, rules, &mut squad),
        };
        self.reinstated(id, now, known)
    }

    /// What the crew does after the squad reinstated `id` (`known`).
    fn reinstated(&mut self, id: u32, now: u32, known: bool) -> bool {
        if !known {
            return false;
        }
        self.pause(id, false, now);
        if !self.in_active_plan(id) {
            self.drop_goal(id);
        }
        true
    }

    /// Marks a member gone ([`Squad::set_gone`]); its keeper stays paused.
    /// False when the id is unknown.
    pub fn set_gone(&mut self, id: u32) -> bool {
        if !self.squad.set_gone(id) {
            return false;
        }
        self.pause(id, true, 0);
        true
    }

    /// The leader's own sighting ([`Squad::observe`]; offset coordinates
    /// with a hex layout). Returns the fact dropped to make room, if any.
    pub fn observe(&mut self, now: u32, fact: Fact) -> Option<Fact> {
        self.observe_traced(now, fact, &mut NoopSink)
    }

    /// [`Crew::observe`] with a trace ([`Squad::observe_traced`], wrapped
    /// in [`CrewTrace::Squad`]).
    pub fn observe_traced<S: Sink<CrewTrace>>(
        &mut self,
        now: u32,
        fact: Fact,
        sink: &mut S,
    ) -> Option<Fact> {
        let fact = Fact {
            pos: self.pos_in(fact.pos),
            ..fact
        };
        let dropped =
            self.squad
                .observe_traced(now, fact, &mut MapSink::new(sink, wrap(self.hex)))?;
        Some(Fact {
            pos: self.pos_out(dropped.pos),
            ..dropped
        })
    }

    /// A danger report on its way to the leader ([`Squad::report_danger`];
    /// offset coordinates with a hex layout). False when the danger queue
    /// is full.
    #[must_use]
    pub fn report_danger(
        &mut self,
        sender: u32,
        centre: Pos,
        shooter: Option<Pos>,
        arrive_at: u32,
    ) -> bool {
        let centre = self.pos_in(centre);
        let shooter = shooter.map(|p| self.pos_in(p));
        self.squad.report_danger(sender, centre, shooter, arrive_at)
    }

    /// A game report from member `msg.sender` to the leader (its `pos` in
    /// offset coordinates with a hex layout): sent over the member's channel
    /// at [`Squad::report_arrives_at`], as the crew's own reports. False when
    /// the inbox refused it; a report that can never arrive (no way to the
    /// leader, or the sender is not an active member) is dropped and
    /// returns true. Its kind decides who reads it (see [`Squad::report`]):
    /// a game's own kinds go to [`SquadRules::report`] in
    /// [`Crew::tick_with`].
    #[must_use]
    pub fn report(&mut self, now: u32, msg: Message) -> bool {
        self.report_with(now, msg, &mut BuiltInRules)
    }

    /// [`Crew::report`] with the game's rules: a
    /// [`Channel::Custom`](super::Channel::Custom) times it through
    /// [`SquadRules::channel_delay`] ([`Squad::report_arrives_at_with`]).
    #[must_use]
    pub fn report_with(&mut self, now: u32, msg: Message, rules: &mut dyn SquadRules) -> bool {
        self.report_traced(now, msg, rules, &mut NoopSink)
    }

    /// [`Crew::report_with`] with a trace: what the inbox did
    /// ([`SquadTrace::Inbox`] in [`CrewTrace::Squad`], when the report is on
    /// its way), then one [`CrewTrace::Sent`].
    #[must_use]
    pub fn report_traced<S: Sink<CrewTrace>>(
        &mut self,
        now: u32,
        msg: Message,
        rules: &mut dyn SquadRules,
        sink: &mut S,
    ) -> bool {
        let msg = Message {
            pos: self.pos_in(msg.pos),
            ..msg
        };
        self.send(msg, msg.sender, now, rules, sink)
    }

    /// Runs the group at `now`; `events` is cleared first and gets what
    /// happened, in this order:
    ///
    /// 1. Members added or removed through [`Crew::squad_mut`] get or lose
    ///    their keeper; a keeper is paused while its member is not active.
    /// 2. [`Squad::check`] with `extra`, `spots` (offset coordinates with a
    ///    hex layout) and `seed`: [`CrewEvent::Planned`] on
    ///    [`Status::Replan`], [`CrewEvent::Signal`],
    ///    [`CrewEvent::NoPlan`]. When the check ends the active plan (an
    ///    abort with nothing to replan: `NoPlan`), every keeper is cleared
    ///    (no goal, `has_goal` 0); a `NoPlan` while no plan was active
    ///    leaves the keepers alone.
    /// 3. [`CrewEvent::Activated`] from [`Squad::take_activated`]; the
    ///    keeper of every member without a goal in the new plan is cleared
    ///    (no goal, [`Crew::goal`] `None`, `has_goal` 0).
    /// 4. [`Squad::orders_due`]: each order goes into its member's keeper
    ///    ([`GoalKeeper::set`]); [`CrewEvent::Order`] with
    ///    [`Goal::start_tick`] of `now`.
    /// 5. [`CrewEvent::Start`] once per order when `now` reaches its start
    ///    tick (wrap-safe), for an active member.
    /// 6. An active member whose goal [`awaits_arrival`](Goal::awaits_arrival)
    ///    and that stands within the arrive radius of the goal's `pos` (on
    ///    the same level) sends [`Message::arrived`] over its channel
    ///    ([`Squad::report`] at [`Squad::report_arrives_at`]):
    ///    [`CrewEvent::Arrived`], once per goal. Without a way to the
    ///    leader nothing is sent and the event still comes; when the inbox
    ///    refuses the report, there is no event and it is tried again next
    ///    tick.
    /// 7. An active member whose keeper is [`broken`](GoalKeeper::broken)
    ///    (give-up ticks from its own skill) sends [`Message::goal_broken`]
    ///    the same way: [`CrewEvent::Broken`], once per goal.
    ///
    /// Steps 5 to 7 only look at a keeper whose goal belongs to the active
    /// plan: a goal of a dead plan (still held while the new order is on
    /// its way) gets no `Start`, no report and no event.
    ///
    /// Members are visited in group order. Errors only from
    /// [`Squad::check`]; nothing after it is done then.
    ///
    /// **Rules:** this is [`Crew::tick_with`] with [`BuiltInRules`]; a squad
    /// that needs rules ([`Squad::needs_rules`]) is refused in step 2 with
    /// [`PlanError::NeedsRules`].
    pub fn tick(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        events: &mut Vec<CrewEvent>,
    ) -> Result<(), PlanError> {
        self.run(now, extra, spots, seed, None, events, &mut NoopSink)
    }

    /// [`Crew::tick`] with the game's rules, in the same steps: step 2 is
    /// [`Squad::check_with`], and steps 6 and 7 send at
    /// [`Squad::report_arrives_at_with`], so a
    /// [`Channel::Custom`](super::Channel::Custom) times the members'
    /// reports too. With a hex layout every slot sees offset coordinates:
    /// its contexts, its [`SquadView`] and the report's `pos`; what it
    /// returns (a [`SquadRules::place`] point, an area of a
    /// [`ReportActions::add_area`]) is taken as offset and kept axial. The
    /// contexts' `built_in` values, distances and headings stay as the
    /// squad computes them (in axial coordinates; hex distances are the
    /// same in both).
    pub fn tick_with(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        rules: &mut dyn SquadRules,
        events: &mut Vec<CrewEvent>,
    ) -> Result<(), PlanError> {
        self.run(now, extra, spots, seed, Some(rules), events, &mut NoopSink)
    }

    /// [`Crew::tick_with`] with a trace: the squad's check and orders
    /// ([`CrewTrace::Squad`]), the keeper checks of step 7
    /// ([`CrewTrace::Keeper`]) and the reports of steps 6 and 7
    /// ([`CrewTrace::Sent`]), in the order of the steps.
    #[allow(clippy::too_many_arguments)]
    pub fn tick_traced<S: Sink<CrewTrace>>(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        rules: &mut dyn SquadRules,
        events: &mut Vec<CrewEvent>,
        sink: &mut S,
    ) -> Result<(), PlanError> {
        self.run(now, extra, spots, seed, Some(rules), events, sink)
    }

    /// [`Crew::tick_traced`]; `None` is [`Crew::tick`] (built-in rules, a
    /// squad that needs rules refused).
    #[allow(clippy::too_many_arguments)]
    fn run<S: Sink<CrewTrace>>(
        &mut self,
        now: u32,
        extra: &[i32],
        spots: &[Spot],
        seed: u64,
        rules: Option<&mut dyn SquadRules>,
        events: &mut Vec<CrewEvent>,
        sink: &mut S,
    ) -> Result<(), PlanError> {
        events.clear();
        self.sync_seats();
        self.pause_inactive(now);
        let had_plan = self.squad.active_plan().is_some();

        if rules.is_none() && self.squad.needs_rules() {
            return Err(PlanError::NeedsRules);
        }
        let mut built_in = BuiltInRules;
        let rules: &mut dyn SquadRules = match rules {
            Some(r) => r,
            None => &mut built_in,
        };
        let status = match self.hex {
            Some(layout) => {
                self.spots.clear();
                self.spots.extend(spots.iter().map(|s| Spot {
                    pos: layout.to_axial_pos(s.pos),
                    ..*s
                }));
                let mut hex = HexRules::new(&mut *rules, layout);
                self.squad.check_traced(
                    now,
                    extra,
                    &self.spots,
                    seed,
                    &mut hex,
                    &mut MapSink::new(&mut *sink, wrap(self.hex)),
                )?
            }
            None => self.squad.check_traced(
                now,
                extra,
                spots,
                seed,
                &mut *rules,
                &mut MapSink::new(&mut *sink, wrap(None)),
            )?,
        };
        match status {
            Status::Replan(id) => events.push(CrewEvent::Planned(id)),
            Status::Signal => events.push(CrewEvent::Signal),
            Status::NoPlan => events.push(CrewEvent::NoPlan),
            Status::Keep => {}
        }
        if had_plan && self.squad.active_plan().is_none() {
            for i in 0..self.seats.len() {
                let member = self.seats[i].id;
                self.drop_goal(member);
            }
        }

        if let Some(id) = self.squad.take_activated() {
            events.push(CrewEvent::Activated(id));
            for i in 0..self.seats.len() {
                let member = self.seats[i].id;
                if !self.in_active_plan(member) {
                    self.drop_goal(member);
                }
            }
        }
        let live = self.squad.active_plan().map(|p| p.id);

        self.squad.orders_due_traced(
            now,
            &mut self.orders,
            &mut MapSink::new(&mut *sink, wrap(self.hex)),
        );
        for i in 0..self.orders.len() {
            let order = self.orders[i];
            let start_at = order.goal.start_tick(now);
            if let Some(seat) = self.seats.iter_mut().find(|s| s.id == order.member) {
                seat.keeper.set(now, order.goal);
                seat.start_at = start_at;
                seat.started = false;
                seat.arrived = false;
                seat.broken = false;
            }
            events.push(CrewEvent::Order {
                member: order.member,
                goal: self.goal_out(order.goal),
                start_at,
            });
        }
        self.orders.clear();

        for seat in &mut self.seats {
            let Some(s) = seat.start_at else { continue };
            let goal_live = seat.keeper.goal().is_some_and(|g| Some(g.plan) == live);
            if !seat.started
                && goal_live
                && now.wrapping_sub(s) < 1 << 31
                && is_active(&self.squad, seat.id)
            {
                seat.started = true;
                events.push(CrewEvent::Start { member: seat.id });
            }
        }

        for i in 0..self.seats.len() {
            let seat = self.seats[i];
            let Some(goal) = seat.keeper.goal().copied().filter(|g| Some(g.plan) == live) else {
                continue;
            };
            let Some(member) = active_member(&self.squad, seat.id) else {
                continue;
            };
            if !seat.arrived
                && goal.awaits_arrival()
                && goal.pos.is_some_and(|p| self.within_radius(member.pos, p))
                && self.send(
                    Message::arrived(seat.id, goal.plan),
                    seat.id,
                    now,
                    rules,
                    sink,
                )
            {
                self.seats[i].arrived = true;
                events.push(CrewEvent::Arrived { member: seat.id });
            }
        }

        for i in 0..self.seats.len() {
            let seat = self.seats[i];
            let Some(goal) = seat.keeper.goal().copied().filter(|g| Some(g.plan) == live) else {
                continue;
            };
            let Some(member) = active_member(&self.squad, seat.id) else {
                continue;
            };
            let give_up = self.squad.skill_table().give_up_ticks_for(member.skill);
            if seat.broken {
                continue;
            }
            let mut keeper = MapSink::new(&mut *sink, |event| CrewTrace::Keeper {
                member: seat.id,
                event,
            });
            if seat.keeper.broken_traced(now, give_up, &mut keeper)
                && self.send(
                    Message::goal_broken(seat.id, goal.plan),
                    seat.id,
                    now,
                    rules,
                    sink,
                )
            {
                self.seats[i].broken = true;
                events.push(CrewEvent::Broken { member: seat.id });
            }
        }
        Ok(())
    }

    /// The member's decider inputs at `now` ([`GoalKeeper::inputs`] from
    /// its position, the squad's metric, [`Squad::signal_given`] and the
    /// [`hold_bonus`](super::SkillTable::hold_bonus_for) of its own skill
    /// as discipline). `None` when the id is unknown.
    pub fn member_inputs(&self, id: u32, now: u32) -> Option<KeeperInputs> {
        let seat = self.seat(id)?;
        let g = self.squad.group();
        let m = g.get(g.index(id)?)?;
        let discipline = self.squad.skill_table().hold_bonus_for(m.skill);
        let row = seat.keeper.inputs(
            now,
            m.pos,
            self.metric,
            self.squad.signal_given(),
            discipline,
        );
        Some(KeeperInputs::from(row))
    }

    /// What the member chose at `now`: its goal (`goal_won`,
    /// [`GoalKeeper::goal_won`]) or something else
    /// ([`GoalKeeper::goal_postponed`]). False when the id is unknown.
    pub fn member_chose(&mut self, id: u32, now: u32, goal_won: bool) -> bool {
        let Some(k) = self.keeper_mut(id) else {
            return false;
        };
        if goal_won {
            k.goal_won(now);
        } else {
            k.goal_postponed(now);
        }
        true
    }

    /// The member's current goal (offset coordinates with a hex layout).
    pub fn goal(&self, id: u32) -> Option<Goal> {
        let goal = *self.seat(id)?.keeper.goal()?;
        Some(self.goal_out(goal))
    }

    /// True when the active plan has a goal for `id`.
    fn in_active_plan(&self, id: u32) -> bool {
        self.squad
            .active_plan()
            .is_some_and(|p| p.goals.iter().any(|(m, _)| *m == id))
    }

    /// Clears the keeper of `id` and what was told about its goal.
    fn drop_goal(&mut self, id: u32) {
        if let Some(seat) = self.seats.iter_mut().find(|s| s.id == id) {
            seat.keeper.clear();
            seat.start_at = None;
            seat.started = false;
            seat.arrived = false;
            seat.broken = false;
        }
    }

    /// Pauses or resumes (at `now`) the keeper of `id`, if it has one.
    fn pause(&mut self, id: u32, paused: bool, now: u32) {
        if let Some(seat) = self.seats.iter_mut().find(|s| s.id == id) {
            seat.set_paused(paused, now);
        }
    }

    fn seat(&self, id: u32) -> Option<&Seat> {
        self.seats.iter().find(|s| s.id == id)
    }

    /// One seat per group member, in group order.
    fn sync_seats(&mut self) {
        let group = self.squad.group();
        self.seats.retain(|s| group.index(s.id).is_some());
        for (i, m) in group.iter().enumerate() {
            match self.seats.iter().position(|s| s.id == m.id) {
                Some(j) if j == i => {}
                Some(j) => self.seats.swap(i, j),
                None => self.seats.insert(i, Seat::new(m.id)),
            }
        }
    }

    /// Keepers follow their member's state (also when it changed through
    /// the squad): paused while it is not active, resumed at `now`.
    fn pause_inactive(&mut self, now: u32) {
        for i in 0..self.seats.len() {
            let active = is_active(&self.squad, self.seats[i].id);
            self.seats[i].set_paused(!active, now);
        }
    }

    /// Sends a member report at the time it reaches the leader; true when
    /// it is on its way, or when it can never arrive (nothing to send).
    fn send<S: Sink<CrewTrace>>(
        &mut self,
        msg: Message,
        id: u32,
        now: u32,
        rules: &mut dyn SquadRules,
        sink: &mut S,
    ) -> bool {
        let at = match self.hex {
            Some(layout) => {
                self.squad
                    .report_arrives_at_with(id, now, &mut HexRules::new(rules, layout))
            }
            None => self.squad.report_arrives_at_with(id, now, rules),
        };
        let queued = at.is_some_and(|at| {
            let mut squad = MapSink::new(&mut *sink, wrap(self.hex));
            self.squad.report_traced(msg, at, &mut squad)
        });
        if sink.enabled() {
            sink.emit(CrewTrace::Sent {
                msg: Message {
                    pos: self.pos_out(msg.pos),
                    ..msg
                },
                delay: at.map(|t| t.wrapping_sub(now)),
                queued,
            });
        }
        at.is_none() || queued
    }

    fn within_radius(&self, at: Pos, spot: Pos) -> bool {
        if at[2] != spot[2] {
            return false;
        }
        let d = distance(
            self.metric,
            i64::from(spot[0]) - i64::from(at[0]),
            i64::from(spot[1]) - i64::from(at[1]),
        );
        let r = i128::from(self.arrive_radius);
        let limit = if self.metric == Metric::Euclid {
            r * r
        } else {
            r
        };
        d <= limit
    }

    fn pos_in(&self, p: Pos) -> Pos {
        self.hex.map_or(p, |l| l.to_axial_pos(p))
    }

    fn pos_out(&self, p: Pos) -> Pos {
        self.hex.map_or(p, |l| l.from_axial_pos(p))
    }

    fn goal_out(&self, goal: Goal) -> Goal {
        Goal {
            pos: goal.pos.map(|p| self.pos_out(p)),
            then_pos: goal.then_pos.map(|p| self.pos_out(p)),
            ..goal
        }
    }
}

fn active_member(squad: &Squad, id: u32) -> Option<Member> {
    let g = squad.group();
    g.get(g.index(id)?)
        .filter(|m| m.state == MemberState::Active)
        .copied()
}

fn is_active(squad: &Squad, id: u32) -> bool {
    active_member(squad, id).is_some()
}

/// The game's rules as a hex crew lends them: every position a slot gets
/// goes from axial to offset coordinates of `layout`, every position it
/// returns from offset to axial. Values the core computed (`built_in`,
/// distances, headings) pass unchanged.
struct HexRules<'a> {
    inner: &'a mut dyn SquadRules,
    layout: HexLayout,
}

impl<'a> HexRules<'a> {
    fn new(inner: &'a mut dyn SquadRules, layout: HexLayout) -> Self {
        Self { inner, layout }
    }

    fn out(&self, p: Pos) -> Pos {
        self.layout.from_axial_pos(p)
    }

    fn fact(&self, f: Fact) -> Fact {
        Fact {
            pos: self.out(f.pos),
            ..f
        }
    }

    fn member(&self, m: Member) -> Member {
        Member {
            pos: self.out(m.pos),
            ..m
        }
    }

    fn row<'r>(&self, ctx: &RowCtx<'r>) -> RowCtx<'r> {
        RowCtx {
            target: ctx.target.map(|f| self.fact(f)),
            ..*ctx
        }
    }
}

impl SquadRules for HexRules<'_> {
    fn keep_target(&mut self, view: &SquadView<'_>, fact: &Fact) -> bool {
        let fact = self.fact(*fact);
        self.inner.keep_target(&view.with_hex(self.layout), &fact)
    }

    fn columns(&mut self, view: &SquadView<'_>, ctx: &RowCtx<'_>, out: &mut [i32]) {
        let ctx = self.row(ctx);
        self.inner.columns(&view.with_hex(self.layout), &ctx, out);
    }

    fn place(&mut self, id: u32, ctx: &PlaceCtx) -> Pos {
        let ctx = PlaceCtx {
            leader: self.out(ctx.leader),
            centre: self.out(ctx.centre),
            face: ctx.face.map(|p| self.out(p)),
            friend: ctx.friend.map(|p| self.out(p)),
            ..*ctx
        };
        self.layout.to_axial_pos(self.inner.place(id, &ctx))
    }

    fn member_cost(&mut self, ctx: &CostCtx) -> Option<i128> {
        let ctx = CostCtx {
            member: self.member(ctx.member),
            wish: self.out(ctx.wish),
            ..*ctx
        };
        self.inner.member_cost(&ctx)
    }

    fn spot_score(&mut self, ctx: &SpotCtx) -> Option<i128> {
        let ctx = SpotCtx {
            member: self.member(ctx.member),
            wish: self.out(ctx.wish),
            spot: Spot {
                pos: self.out(ctx.spot.pos),
                ..ctx.spot
            },
            target: ctx.target.map(|f| self.fact(f)),
            ..*ctx
        };
        self.inner.spot_score(&ctx)
    }

    fn trigger(&mut self, id: u32, view: &SquadView<'_>, now: u32) -> bool {
        self.inner.trigger(id, &view.with_hex(self.layout), now)
    }

    fn abort(&mut self, id: u32, view: &SquadView<'_>, ctx: &RowCtx<'_>) -> bool {
        let ctx = self.row(ctx);
        self.inner.abort(id, &view.with_hex(self.layout), &ctx)
    }

    fn channel_delay(&mut self, id: u32, ctx: &ChannelCtx) -> Option<u32> {
        let ctx = ChannelCtx {
            from: self.out(ctx.from),
            to: self.out(ctx.to),
            ..*ctx
        };
        self.inner.channel_delay(id, &ctx)
    }

    fn report(
        &mut self,
        view: &SquadView<'_>,
        msg: &Message,
        now: u32,
        actions: &mut ReportActions<'_>,
    ) {
        let msg = Message {
            pos: self.out(msg.pos),
            ..*msg
        };
        self.inner
            .report(&view.with_hex(self.layout), &msg, now, actions);
        let layout = self.layout;
        actions.map(|a| match a {
            ReportAction::AddArea(area) => ReportAction::AddArea(Area {
                pos: layout.to_axial_pos(area.pos),
                source: layout.to_axial_pos(area.source),
                ..area
            }),
            other => other,
        });
    }
}
