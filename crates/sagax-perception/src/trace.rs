//! What [`observe_traced`](crate::observe_traced),
//! [`look_traced`](crate::look_traced) and the hearing check report.

use sagax_core::{Cell, Pos};

use crate::look::Pick;
use crate::percept::Sense;

/// One record of a traced perception call. `target` is the target's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PerceptionTrace {
    /// `look` chose to check this target.
    Picked {
        /// The target.
        target: u32,
        /// Why.
        why: Pick,
    },
    /// The budget ran out before this target (only when the sink is
    /// enabled).
    Skipped {
        /// The target.
        target: u32,
    },
    /// A sight line was walked (the normal one, then the thermal one).
    Line {
        /// The target.
        target: u32,
        /// It got through.
        clear: bool,
        /// How much got through, 0..=256.
        clarity: u16,
        /// Where it stopped, if at a cell.
        blocked_at: Option<Cell>,
    },
    /// The factors of one pass of a check (the "why not noticed?").
    Gain {
        /// The target.
        target: u32,
        /// The band index (`u8::MAX` for the scanner).
        band: u8,
        /// The distance factor, per 256.
        distance: u16,
        /// The light factor (thermal: the heat factor), per 256.
        light: u16,
        /// The clarity factor, per 256.
        clarity: u16,
        /// The crowd factor, per 256.
        crowd: u16,
        /// The gain of this check (credit included).
        gain: u32,
        /// Which pass.
        sense: Sense,
    },
    /// An instant cue fired.
    Instant {
        /// The target.
        target: u32,
        /// The cue rule index.
        rule: u8,
    },
    /// A stage change (`pending: false`), or a rise now waiting for the
    /// reaction time (`pending: true`, `to` the waiting stage).
    Stage {
        /// The target.
        target: u32,
        /// The counted stage before.
        from: u8,
        /// The new (or waiting) stage.
        to: u8,
        /// Waiting for the reaction time.
        pending: bool,
    },
    /// See-through progress (only with see-through on).
    Doubt {
        /// The target.
        target: u32,
        /// The doubt now.
        doubt: u16,
    },
    /// A stimulus was heard.
    Heard {
        /// The stimulus id.
        stimulus: u32,
        /// Its cost to this listener.
        cost: i32,
        /// The listener's range for it.
        range: i32,
        /// The (fuzzed) cell reported.
        cell: Pos,
    },
    /// A stimulus was not heard.
    Unheard {
        /// The stimulus id.
        stimulus: u32,
        /// Its cost, `None` when it does not get through at all.
        cost: Option<i32>,
        /// The listener's range for it.
        range: i32,
    },
}
