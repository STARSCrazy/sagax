//! Detours: a short break from the plan, the leash, and the walk back.

use super::{Activity, Hooks, Mission, View};
use crate::memory::{Knowledge, Pos};
use crate::mission::types::{Action, Detour, DetourEnd, Event, OnLost, StepKind, Where};

/// The search that runs when the subject of an `Engage` is lost and the
/// leash says [`OnLost::Search`].
#[derive(Clone, Copy, Debug)]
pub(super) struct LostSearch {
    /// Where the subject was last known (internal coordinates).
    centre: Pos,
    /// When the search began.
    begun_at: u32,
    /// The search point it goes to (a counter, cycled).
    point: usize,
}

/// What a detour keeps while it runs and while the character walks back.
#[derive(Clone, Copy, Debug)]
pub(super) struct DetourState {
    /// The running (or last) detour.
    kind: Detour,
    /// Where the character goes back to (internal coordinates); `None`
    /// until the character's position is known (taken at the next tick).
    anchor: Option<Pos>,
    /// When the first detour began (a replacing detour keeps it).
    started_at: u32,
    /// What ran before the detour; it goes on after the walk back.
    pub(super) resume: Activity,
    /// How long the plan's step had run when the detour began; on resume
    /// the step goes on with the time it had left.
    step_elapsed: u32,
    /// [`Event::DetourStarted`] is due at the next tick.
    pub(super) announce: bool,
    /// The game called [`Mission::end_detour`].
    end_asked: bool,
    /// The search after losing the subject, while it runs.
    search: Option<LostSearch>,
}

impl DetourState {
    /// No detour.
    pub(super) const fn new() -> Self {
        Self {
            kind: Detour::Custom {
                kind: 0,
                at: Where::Here,
                param: 0,
            },
            anchor: None,
            started_at: 0,
            resume: Activity::Idle,
            step_elapsed: 0,
            announce: false,
            end_asked: false,
            search: None,
        }
    }
}

impl Mission {
    /// Starts a detour at time `now`; true if taken. The game decides when,
    /// from its own perception; `knowledge` is what the character knows.
    ///
    /// The plan waits (its fail conditions are not checked) until the
    /// detour ends; then the character walks back to the **anchor** and
    /// resumes. The anchor is where the character stood at the last tick
    /// (for a running [`StepKind::Hold`] step: the hold position). A detour
    /// may replace a running one: the new one wins, the anchor and the
    /// start time (for [`Leash::ticks`](crate::mission::Leash::ticks)) stay.
    /// A detour during the walk back keeps the anchor and starts its time
    /// anew.
    ///
    /// The next tick emits [`Event::DetourStarted`]. A detour ends:
    ///
    /// - [`Detour::Engage`]: the subject is known as defeated
    ///   ([`DetourEnd::Defeated`]) or no longer known
    ///   ([`DetourEnd::Forgotten`]); the character is farther from the
    ///   anchor than the leash distance (in the mission's metric; Euclid
    ///   compares squares; another level counts as too far) or the detour
    ///   took longer than the leash ticks ([`DetourEnd::Leash`]); the
    ///   subject was not seen for more than `lost_after` ticks and
    ///   [`OnLost`] says: `Return` ends at once, `Search` searches around
    ///   the last known position for its ticks first (seeing the subject
    ///   again goes back to the attack), both with [`DetourEnd::Lost`];
    ///   `Keep` goes on to the last known position. The action is
    ///   [`Action::Attack`].
    /// - [`Detour::Flee`]: arrived at `to` ([`DetourEnd::Arrived`]) or
    ///   farther than `safe_distance` from `from` ([`DetourEnd::Safe`];
    ///   also when `from` cannot be filled). The action is
    ///   [`Action::Flee`] to `to`, or without `to` to the point as far
    ///   beyond the character as `from` is behind it, kept inside the
    ///   bounds of the config's search pattern. When the character stands
    ///   on `from` (say `from` is [`Where::Here`]) there is no "away": it
    ///   flees `safe_distance + 1` cells along +x (east), or along -x when
    ///   the bounds stop it there.
    /// - [`Detour::Custom`]: only by [`Mission::end_detour`]. The action is
    ///   [`Action::Custom`].
    ///
    /// On the end the tick emits [`Event::DetourEnded`] and the character
    /// walks back ([`Action::GoTo`] the anchor), unless the step it resumes
    /// is a `GoTo`, `Hold` or `Search` with another target, then it resumes
    /// at once. Resuming emits [`Event::Returned`]; the step goes on with
    /// its state (a `Wait` keeps the time it had left, a `Search` starts
    /// again) and its fail conditions are checked again.
    ///
    /// **Refused** (false, no event): an `Engage` of a subject that is not
    /// known, or, with a leash distance, whose last known position is
    /// farther than that from the anchor (of the running detour or the walk
    /// back; else the anchor this detour would get). So a guard does not go
    /// back and forth at the edge of its leash. Without a leash distance an
    /// `Engage` of a known subject is always taken; `Flee` and `Custom`
    /// always are.
    pub fn detour(&mut self, now: u32, d: Detour, knowledge: &Knowledge) -> bool {
        let view = View {
            now,
            pos: self.last_pos.unwrap_or_default(),
            knowledge,
            flags: 0,
        };
        let ongoing = matches!(self.activity, Activity::Detour | Activity::Returning);
        let anchor = if ongoing {
            self.detour.anchor
        } else {
            self.anchor_now(view)
        };
        if let Detour::Engage(subject) = d {
            let Some(at) = self.subject_pos(subject, view) else {
                return false;
            };
            if let (Some(limit), Some(anchor)) = (self.leeway.leash.distance, anchor)
                && !self.within(anchor, at, limit)
            {
                return false;
            }
        }
        match self.activity {
            Activity::Detour => {}
            Activity::Returning => self.detour.started_at = now,
            before => {
                self.detour.resume = before;
                self.detour.anchor = anchor;
                self.detour.started_at = now;
                self.detour.step_elapsed = now.wrapping_sub(self.step.begun_at);
            }
        }
        self.detour.kind = d;
        self.detour.search = None;
        self.detour.end_asked = false;
        self.detour.announce = true;
        self.activity = Activity::Detour;
        true
    }

    /// Ends the running detour at the next tick ([`DetourEnd::Ended`]); the
    /// character then walks back. Ignored when no detour runs.
    pub fn end_detour(&mut self) {
        if self.activity == Activity::Detour {
            self.detour.end_asked = true;
        }
    }

    /// A detour runs.
    pub fn in_detour(&self) -> bool {
        self.activity == Activity::Detour
    }

    /// The detour ended and the character walks back to its anchor.
    pub fn returning(&self) -> bool {
        self.activity == Activity::Returning
    }

    /// The anchor a detour starting now would get: the hold position of a
    /// running `Hold` step, else where the character stood at the last tick.
    fn anchor_now(&self, view: View<'_>) -> Option<Pos> {
        if self.activity == Activity::Plan
            && !self.pending_start
            && let StepKind::Hold(at) = self.plan.steps()[self.step.index].kind
            && let Some(p) = self.resolve(at, view)
        {
            return Some(p);
        }
        self.last_pos
    }

    pub(super) fn run_detour(&mut self, view: View<'_>, rules: &mut Hooks<'_>) -> Action {
        let anchor = *self.detour.anchor.get_or_insert(view.pos);
        if self.detour.end_asked {
            return self.end_with(DetourEnd::Ended, view, rules);
        }
        match self.detour.kind {
            Detour::Engage(subject) => self.engage(subject, anchor, view, rules),
            Detour::Flee { from, to } => self.flee(from, to, view, rules),
            Detour::Custom { kind, at, param } => Action::Custom {
                kind,
                pos: self.resolve(at, view),
                param,
            },
        }
    }

    fn engage(
        &mut self,
        subject: u32,
        anchor: Pos,
        view: View<'_>,
        rules: &mut Hooks<'_>,
    ) -> Action {
        let Some(fact) = self.fact(subject, view) else {
            return self.end_with(DetourEnd::Forgotten, view, rules);
        };
        if self.defeated(fact) {
            return self.end_with(DetourEnd::Defeated, view, rules);
        }
        let leash = self.leeway.leash;
        let too_far = leash
            .distance
            .is_some_and(|d| !self.within(anchor, view.pos, d));
        let too_long = leash
            .ticks
            .is_some_and(|t| view.now.wrapping_sub(self.detour.started_at) > t);
        if too_far || too_long {
            return self.end_with(DetourEnd::Leash, view, rules);
        }
        let pos = self.pos_in(fact.pos);
        let attack = Action::Attack { subject, pos };
        if view.now.wrapping_sub(fact.seen_at) <= self.config.lost_after {
            self.detour.search = None;
            return attack;
        }
        match leash.on_lost {
            OnLost::Return => self.end_with(DetourEnd::Lost, view, rules),
            OnLost::Keep => attack,
            OnLost::Search { ticks, radius } => {
                let s = self.detour.search.get_or_insert(LostSearch {
                    centre: pos,
                    begun_at: view.now,
                    point: 0,
                });
                let s = *s;
                if view.now.wrapping_sub(s.begun_at) >= ticks {
                    return self.end_with(DetourEnd::Lost, view, rules);
                }
                let (point, at) = self.search_step(s.point, s.centre, radius, view.pos);
                self.detour.search = Some(LostSearch { point, ..s });
                Action::Search(at)
            }
        }
    }

    fn flee(
        &mut self,
        from: Where,
        to: Option<Pos>,
        view: View<'_>,
        rules: &mut Hooks<'_>,
    ) -> Action {
        let Some(from) = self.resolve(from, view) else {
            return self.end_with(DetourEnd::Safe, view, rules);
        };
        let to = to.map(|p| self.pos_in(p));
        if to.is_some_and(|p| self.arrived(view.pos, p)) {
            return self.end_with(DetourEnd::Arrived, view, rules);
        }
        if !self.within(view.pos, from, self.config.safe_distance) {
            return self.end_with(DetourEnd::Safe, view, rules);
        }
        Action::Flee(to.unwrap_or_else(|| self.away_from(view.pos, from)))
    }

    /// The point as far beyond `pos` as `from` is behind it (internal
    /// coordinates), kept inside the bounds of the search pattern (offset
    /// coordinates with a hex layout). On `from` itself: `safe_distance + 1`
    /// cells along +x, or along -x when the bounds stop that.
    fn away_from(&self, pos: Pos, from: Pos) -> Pos {
        if pos[..2] == from[..2] {
            let step = i64::from(self.config.safe_distance) + 1;
            let east = self.bounded(pos, [i64::from(pos[0]) + step, i64::from(pos[1])]);
            if east != pos {
                return east;
            }
            return self.bounded(pos, [i64::from(pos[0]) - step, i64::from(pos[1])]);
        }
        let mirror = |a: i32, b: i32| 2 * i64::from(a) - i64::from(b);
        self.bounded(pos, [mirror(pos[0], from[0]), mirror(pos[1], from[1])])
    }

    /// `p` (internal coordinates, on the level of `pos`) kept inside the
    /// bounds of the search pattern.
    fn bounded(&self, pos: Pos, p: [i64; 2]) -> Pos {
        let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        let out = self.pos_out([clamp(p[0]), clamp(p[1]), pos[2]]);
        let (min, max) = (self.config.search.min, self.config.search.max);
        self.pos_in([
            out[0].clamp(min.0, max.0.max(min.0)),
            out[1].clamp(min.1, max.1.max(min.1)),
            out[2],
        ])
    }

    /// Ends the detour: emits the event and starts the walk back.
    fn end_with(&mut self, why: DetourEnd, view: View<'_>, rules: &mut Hooks<'_>) -> Action {
        self.emit(Event::DetourEnded(why));
        self.activity = Activity::Returning;
        self.detour.search = None;
        self.detour.end_asked = false;
        self.run_return(view, rules)
    }

    pub(super) fn run_return(&mut self, view: View<'_>, rules: &mut Hooks<'_>) -> Action {
        let anchor = self.detour.anchor.unwrap_or(view.pos);
        // A strict character always walks back first: if its plan broke
        // during the detour it holds where it stands, which must be the
        // anchor.
        let shortcut = self.leeway.replan && self.resumed_step_goes_elsewhere(anchor, view);
        if self.arrived(view.pos, anchor) || shortcut {
            return self.resume(view, rules);
        }
        Action::GoTo(anchor)
    }

    /// The step to resume walks to a target that is not the anchor anyway.
    fn resumed_step_goes_elsewhere(&self, anchor: Pos, view: View<'_>) -> bool {
        if self.detour.resume != Activity::Plan {
            return false;
        }
        let index = if self.pending_start {
            0
        } else {
            self.step.index
        };
        let at = match self.plan.steps()[index].kind {
            StepKind::GoTo(at) | StepKind::Hold(at) | StepKind::Search { centre: at, .. } => at,
            _ => return false,
        };
        self.resolve(at, view).is_some_and(|p| p != anchor)
    }

    /// Back from the detour: what ran before goes on.
    fn resume(&mut self, view: View<'_>, rules: &mut Hooks<'_>) -> Action {
        self.emit(Event::Returned);
        self.activity = self.detour.resume;
        match self.activity {
            Activity::Plan => {
                if !self.pending_start {
                    if let StepKind::Search { .. } = self.plan.steps()[self.step.index].kind {
                        self.step.begun_at = view.now;
                        self.step.search_point = 0;
                    } else {
                        self.step.begun_at = view.now.wrapping_sub(self.detour.step_elapsed);
                    }
                }
                // The fail conditions were not checked during the detour;
                // the plan checks them now. A plan that broke during the
                // detour breaks here and replans or falls back.
                self.run_plan(view, rules)
            }
            Activity::Wander => self.run_wander(view, rules),
            // No plan before the detour: take one now.
            _ => self.replace_plan(view, None, false, rules),
        }
    }
}
