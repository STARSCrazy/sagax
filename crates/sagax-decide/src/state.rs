//! A state machine with timers and abort conditions.
//!
//! The game names its states by number (patrol 0, chase 1, search 2, ...)
//! and its conditions by bit (bit 0 "enemy seen", bit 1 "hit", ...). Every
//! tick it calls [`StateMachine::update`] with the time and the conditions
//! that hold now. The machine changes state when
//!
//! * a [`Transition`] matches: it starts in the current state (or in any
//!   state), its conditions hold and the state has run long enough, or
//! * the current state has run out of time (its timeout).
//!
//! Transitions are checked in the order they were added; the first that
//! matches wins, so put aborts ("dead", "under fire") first. A timeout is
//! checked after all transitions. At most one change per update.
//!
//! Time is the game's own count (frames, turns, seconds); it may wrap around.
//! Everything is deterministic: same ticks, same states.

use sagax_core::{NoopSink, Sink};

/// `from` of a transition that leaves any state (an abort). It does not
/// fire in its own target state, so an abort does not restart that state.
pub const ANY: u32 = u32::MAX;

/// A way from one state to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Transition {
    /// The state it leaves, or [`ANY`].
    pub from: u32,
    /// The state it enters.
    pub to: u32,
    /// Which condition bits are looked at.
    pub mask: u64,
    /// The values they must have: `flags & mask == value`.
    pub value: u64,
    /// Time the state must have run before this transition may fire (0:
    /// at once, e.g. aborts).
    pub after: u32,
}

impl Transition {
    /// From `from` to `to` when every bit of `when` is set.
    pub const fn new(from: u32, to: u32, when: u64) -> Self {
        Self {
            from,
            to,
            mask: when,
            value: when,
            after: 0,
        }
    }

    /// Also requires every bit of `unless` to be clear.
    pub const fn unless(mut self, unless: u64) -> Self {
        self.mask |= unless;
        self.value &= !unless;
        self
    }

    /// Only after the state has run `time`.
    pub const fn after(mut self, time: u32) -> Self {
        self.after = time;
        self
    }
}

/// Why the state changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    /// This transition (index in the order added) matched.
    Transition(usize),
    /// The state ran out of time.
    Timeout,
    /// The game set the state ([`StateMachine::set_state`]).
    Set,
}

/// A change of state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Change {
    /// The state left.
    pub from: u32,
    /// The state entered.
    pub to: u32,
    /// Why.
    pub reason: Reason,
}

#[derive(Clone, Copy, Debug, Default)]
struct StateDef {
    /// 0: none.
    timeout: u32,
    next: u32,
}

/// Why [`StateMachine`] refused a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnknownState(pub u32);

impl std::fmt::Display for UnknownState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no state {}", self.0)
    }
}

impl std::error::Error for UnknownState {}

/// States with timers and transitions; see the module docs.
#[derive(Clone, Debug)]
pub struct StateMachine {
    states: Vec<StateDef>,
    transitions: Vec<Transition>,
    current: u32,
    entered: u32,
}

impl StateMachine {
    /// `states` states (0..states), starting in `initial` at time 0.
    pub fn new(states: u32, initial: u32) -> Result<Self, UnknownState> {
        if initial >= states {
            return Err(UnknownState(initial));
        }
        Ok(Self {
            states: vec![StateDef::default(); states as usize],
            transitions: Vec::new(),
            current: initial,
            entered: 0,
        })
    }

    fn check(&self, state: u32) -> Result<(), UnknownState> {
        if (state as usize) < self.states.len() {
            Ok(())
        } else {
            Err(UnknownState(state))
        }
    }

    /// After `time` in `state` the machine goes to `next` (time 0: never).
    pub fn set_timeout(&mut self, state: u32, time: u32, next: u32) -> Result<(), UnknownState> {
        self.check(state)?;
        self.check(next)?;
        self.states[state as usize] = StateDef {
            timeout: time,
            next,
        };
        Ok(())
    }

    /// Adds a transition and returns its index.
    pub fn add_transition(&mut self, t: Transition) -> Result<usize, UnknownState> {
        if t.from != ANY {
            self.check(t.from)?;
        }
        self.check(t.to)?;
        self.transitions.push(t);
        Ok(self.transitions.len() - 1)
    }

    /// The current state.
    pub fn state(&self) -> u32 {
        self.current
    }

    /// When the current state was entered.
    pub fn entered(&self) -> u32 {
        self.entered
    }

    /// How long the current state has run at `now`.
    pub fn elapsed(&self, now: u32) -> u32 {
        now.wrapping_sub(self.entered)
    }

    /// Puts the machine into `state` at `now` (also to start the clock).
    pub fn set_state(&mut self, state: u32, now: u32) -> Result<Change, UnknownState> {
        self.check(state)?;
        let change = Change {
            from: self.current,
            to: state,
            reason: Reason::Set,
        };
        self.current = state;
        self.entered = now;
        Ok(change)
    }

    /// One tick: `flags` are the conditions that hold now. Returns the
    /// change, if any.
    pub fn update(&mut self, now: u32, flags: u64) -> Option<Change> {
        self.update_traced(now, flags, &mut NoopSink)
    }

    /// [`StateMachine::update`]; a change also goes to `sink`.
    pub fn update_traced<S: Sink<Change>>(
        &mut self,
        now: u32,
        flags: u64,
        sink: &mut S,
    ) -> Option<Change> {
        let elapsed = self.elapsed(now);
        let fired = self.transitions.iter().position(|t| {
            (t.from == self.current || (t.from == ANY && t.to != self.current))
                && flags & t.mask == t.value
                && elapsed >= t.after
        });
        let (to, reason) = match fired {
            Some(i) => (self.transitions[i].to, Reason::Transition(i)),
            None => {
                let s = self.states[self.current as usize];
                if s.timeout == 0 || elapsed < s.timeout {
                    return None;
                }
                (s.next, Reason::Timeout)
            }
        };
        let change = Change {
            from: self.current,
            to,
            reason,
        };
        self.current = to;
        self.entered = now;
        if sink.enabled() {
            sink.emit(change);
        }
        Some(change)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATROL: u32 = 0;
    const CHASE: u32 = 1;
    const SEARCH: u32 = 2;
    const DEAD: u32 = 3;
    const SEEN: u64 = 1;
    const KILLED: u64 = 2;

    fn guard() -> StateMachine {
        let mut m = StateMachine::new(4, PATROL).unwrap();
        // abort first: from any state
        m.add_transition(Transition::new(ANY, DEAD, KILLED))
            .unwrap();
        m.add_transition(Transition::new(PATROL, CHASE, SEEN))
            .unwrap();
        m.add_transition(Transition::new(CHASE, SEARCH, 0).unless(SEEN).after(10))
            .unwrap();
        m.add_transition(Transition::new(SEARCH, CHASE, SEEN))
            .unwrap();
        m.set_timeout(SEARCH, 100, PATROL).unwrap();
        m
    }

    #[test]
    fn a_guard_patrols_chases_searches_and_gives_up() {
        let mut m = guard();
        assert_eq!(m.update(1, 0), None);
        let c = m.update(2, SEEN).unwrap();
        assert_eq!(
            (c.from, c.to, c.reason),
            (PATROL, CHASE, Reason::Transition(1))
        );
        // lost sight, but chases at least 10 ticks
        assert_eq!(m.update(5, 0), None);
        assert_eq!(m.update(12, 0).map(|c| c.to), Some(SEARCH));
        // searches 100 ticks, then back on patrol
        assert_eq!(m.update(111, 0), None);
        let c = m.update(112, 0).unwrap();
        assert_eq!((c.to, c.reason), (PATROL, Reason::Timeout));
        assert_eq!(m.entered(), 112);
    }

    #[test]
    fn aborts_win_and_time_wraps() {
        let mut m = guard();
        m.set_state(SEARCH, u32::MAX - 5).unwrap();
        // the abort comes before "seen again"
        let c = m.update(3, SEEN | KILLED).unwrap();
        assert_eq!((c.to, c.reason), (DEAD, Reason::Transition(0)));
        assert_eq!(m.update(4, KILLED), None); // stays dead, the clock is not restarted
        let mut m = guard();
        m.set_state(SEARCH, u32::MAX - 5).unwrap();
        assert_eq!(m.elapsed(3), 9);
        assert_eq!(m.update(94, 0).map(|c| c.reason), Some(Reason::Timeout));
    }

    #[test]
    fn unknown_states_are_refused() {
        assert_eq!(StateMachine::new(2, 2).unwrap_err(), UnknownState(2));
        let mut m = guard();
        assert!(m.add_transition(Transition::new(9, 0, 0)).is_err());
        assert!(m.add_transition(Transition::new(ANY, 9, 0)).is_err());
        assert!(m.set_timeout(0, 1, 9).is_err());
        assert!(m.set_state(4, 0).is_err());
    }
}
