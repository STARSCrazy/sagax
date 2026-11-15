//! What is perceived ([`Target`]) and what comes out ([`Percept`]).

use sagax_core::Pos;

/// Most percepts one check of one target pushes.
pub const MAX_PERCEPTS_PER_CHECK: usize = 4;

/// Something that could be perceived, as the game passes it: its own
/// candidate list, never a global truth.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Target {
    /// The game's subject id (becomes `Percept::subject`).
    pub id: u32,
    /// The point the eye aims at, fine units.
    pub point: Pos,
    /// Size per 256 (a person 256, a crouching one 128, a crate 384).
    pub size: u16,
    /// Conspicuity per 256: still 128, walking 256, running 512, firing
    /// 2048.
    pub conspicuity: u16,
    /// What it shows now (the game's bits).
    pub cues: u32,
    /// 0: none, 1..=255: disguise strength.
    pub disguise: u8,
    /// For thermal sight (0: cold).
    pub heat: u8,
    /// Its own light: the light counts as `max(cell light, glow)`.
    pub glow: u8,
    /// How many others it blends with (the game counts, or
    /// [`count_crowds`](crate::count_crowds)).
    pub crowd: u8,
}

/// The sense behind a percept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub enum Sense {
    /// Normal sight (also for `Lost` and `Forgotten`).
    #[default]
    Sight,
    /// The thermal pass.
    Thermal,
    /// The scanner.
    Scanner,
    /// Hearing on this channel.
    Hearing(u8),
}

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub enum PerceptKind {
    /// The counted stage rose from unseen.
    Noticed,
    /// The counted stage rose to recognised.
    Recognized,
    /// At stage noticed or higher and in view: its current place. The last
    /// percept of such a check.
    Seen,
    /// Recognised cue bits not reported before.
    Cues {
        /// The new bits.
        new: u32,
    },
    /// In view at the last check, not now.
    Lost,
    /// The counted stage fell to unseen.
    Forgotten,
    /// The observer saw through the disguise (only with see-through on).
    SeenThrough,
    /// A stimulus was heard.
    Heard,
}

/// One perceived event, for `Knowledge` and `Alertness`.
///
/// Sight percepts carry `point` and `cell` only when the target is in view
/// in that check; otherwise (`Lost`, `Forgotten`, or a delayed rise that
/// became due while the target is out of view) both are zero and
/// `clarity` is 0: the observer does not know where it is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Percept {
    /// The observer's id.
    pub observer: u32,
    /// The target's (or the stimulus source's) id.
    pub subject: u32,
    /// The sense behind it.
    pub sense: Sense,
    /// What happened.
    pub event: PerceptKind,
    /// The counted stage after this check (0, 1, 2).
    pub stage: u8,
    /// Where, fine point.
    pub point: Pos,
    /// The whole cell `[x, y, level]` of `point` (axial on hex).
    pub cell: Pos,
    /// The cue bits seen (only at stage recognised; masked ones never).
    pub cues: u32,
    /// The target is disguised for this observer.
    pub disguised: bool,
    /// How clearly, 0..=256 (0 when not in view, and for the scanner).
    pub clarity: u16,
    /// Hearing only: how clearly heard (0 otherwise).
    pub margin: i32,
    /// Hearing only: when it arrives (0 otherwise).
    pub arrive_at: u32,
    /// The time of the check.
    pub at: u32,
}
