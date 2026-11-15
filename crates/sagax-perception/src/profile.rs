//! The observer's side: sense profiles with presets and builders, the
//! per-call stats, the disguise rule and the [`Eye`].

use sagax_core::Pos;

use crate::world::MAX_CHANNELS;

/// Sense bit: normal sight (light, `density`, bands).
pub const SIGHT: u8 = 1;
/// Sense bit: light counts as at least the profile's `bright`.
pub const NIGHT: u8 = 2;
/// Sense bit: a second sight pass that ignores light, reads
/// `heat_density` and scales by the target's `heat`.
pub const THERMAL: u8 = 4;
/// Sense bit: within `scanner_range`, walls, bands and light do not
/// matter, but it only reaches *noticed*.
pub const SCANNER: u8 = 8;

/// Number of cue rules in a [`SenseProfile`].
pub const MAX_CUES: usize = 8;

/// One view band: a half-angle around the facing and what it gives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Band {
    /// Half-angle in headings (0..=128; 43 is a 120 degree cone).
    pub half_angle: u8,
    /// Gain per 256 inside this band; 0: the band is unused.
    pub gain: u16,
    /// Whether instant cues fire inside this band.
    pub instant: bool,
}

/// What a cue bit of a target does for this observer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct CueRule {
    /// Which cue bits (any of them matches); 0: an unused rule.
    pub bits: u32,
    /// Conspicuity factor per 256 while a bit is shown; 0 leaves the gain
    /// as it is (a rule that is only instant).
    pub gain: u16,
    /// 0: no, 1: raise to *noticed*, 2: raise to *recognised* at once, in a
    /// band that allows instant cues.
    pub instant: u8,
    /// Instant only with at least this clarity (per 256).
    pub min_clarity: u16,
    /// The rule acts only within this distance, fine units (0: the whole
    /// sight range).
    pub range: i32,
}

/// What an observer can sense and how fast it notices: a plain value the
/// game may keep per role or change per observer and tick.
///
/// Distances are fine units. Factors are per 256. See the presets
/// [`human`](Self::human), [`guard`](Self::guard) and
/// [`watcher`](Self::watcher) and the `with_*` builders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct SenseProfile {
    /// [`SIGHT`], [`NIGHT`], [`THERMAL`], [`SCANNER`] bits.
    pub senses: u8,
    /// Sight range before [`Stats::sight_scale`].
    pub range: i32,
    /// Full gain up to this distance.
    pub near: i32,
    /// Gain at `range`, falling linearly from 256 at `near`.
    pub far_gain: u16,
    /// Up to three bands in ascending half-angles; the first that holds
    /// the target counts. Outside all: unseen.
    pub bands: [Band; 3],
    /// Counter gain per tick at best conditions.
    pub base_gain: u16,
    /// Light at or below this gives `dark_gain`.
    pub dark: u8,
    /// Light at or above this gives full gain; linear in between.
    pub bright: u8,
    /// Gain per 256 in the dark.
    pub dark_gain: u16,
    /// Counter at which a target is *noticed* (stage 1).
    pub notice_at: u16,
    /// Counter at which a target is *recognised* (stage 2).
    pub recognize_at: u16,
    /// Highest counter.
    pub max: u16,
    /// Counter lost per credited tick once the hold is over.
    pub decay_per_tick: u16,
    /// Ticks after the last sighting during which nothing decays.
    pub hold: u16,
    /// How far below a threshold the counter must fall before the stage
    /// falls.
    pub hysteresis: u16,
    /// The observer's check interval: the most ticks one check credits
    /// (0 counts as 1).
    pub check_interval: u16,
    /// Gain per 256 by the target's crowd, index `min(crowd, 7)`.
    pub crowd_gain: [u16; 8],
    /// Scanner range (with [`SCANNER`]); 0: off.
    pub scanner_range: i32,
    /// Counter gain per tick of the scanner.
    pub scanner_gain: u16,
    /// Hearing range scale per channel, per 256 (0: deaf on it).
    pub hearing: [u16; MAX_CHANNELS],
    /// Counter gain per heard stimulus at full margin.
    pub hear_gain: u16,
    /// The cue rules, in index order; unused ones have `bits` 0.
    pub cues: [CueRule; MAX_CUES],
}

impl SenseProfile {
    /// An ordinary person, for `cell` fine units per cell: range 12 cells,
    /// full gain up to 4 cells, 64 at the range; focus half-angle 43
    /// (gain 256, instant cues) and periphery 71 (gain 64). A walking
    /// person in clear daylight in focus within 4 cells is noticed after 4
    /// checks and recognised after 12 (base gain 64, notice at 256,
    /// recognise at 768, max 1024). Light at or below 32 gives 32 / 256,
    /// at or above 160 full gain. Decay 16 per tick after a hold of 8,
    /// hysteresis 32, checked every tick. Crowd table 256, 192, 144, 108,
    /// 81, 61, 46, 34. Hears channel 0 at full range. No special sense,
    /// no cue rule.
    pub fn human(cell: i32) -> Self {
        Self {
            senses: SIGHT,
            range: cell.saturating_mul(12),
            near: cell.saturating_mul(4),
            far_gain: 64,
            bands: [
                Band {
                    half_angle: 43,
                    gain: 256,
                    instant: true,
                },
                Band {
                    half_angle: 71,
                    gain: 64,
                    instant: false,
                },
                Band::default(),
            ],
            base_gain: 64,
            dark: 32,
            bright: 160,
            dark_gain: 32,
            notice_at: 256,
            recognize_at: 768,
            max: 1024,
            decay_per_tick: 16,
            hold: 8,
            hysteresis: 32,
            check_interval: 1,
            crowd_gain: [256, 192, 144, 108, 81, 61, 46, 34],
            scanner_range: 0,
            scanner_gain: 64,
            hearing: [256, 0, 0, 0],
            hear_gain: 256,
            cues: [CueRule::default(); MAX_CUES],
        }
    }

    /// A guard: [`human`](Self::human) with range 14 cells and base gain
    /// 80 (quicker).
    pub fn guard(cell: i32) -> Self {
        Self {
            range: cell.saturating_mul(14),
            base_gain: 80,
            ..Self::human(cell)
        }
    }

    /// A watcher: [`human`](Self::human) with a wide periphery
    /// (half-angle 100) and slower recognition (at 1024).
    pub fn watcher(cell: i32) -> Self {
        let mut p = Self::human(cell);
        p.bands[1].half_angle = 100;
        p.recognize_at = 1024;
        p
    }

    /// Adds [`NIGHT`]: light counts as at least `bright`.
    pub fn with_night(mut self) -> Self {
        self.senses |= NIGHT;
        self
    }

    /// Adds [`THERMAL`]: a second sight pass by heat.
    pub fn with_thermal(mut self) -> Self {
        self.senses |= THERMAL;
        self
    }

    /// Adds [`SCANNER`] with `range` fine units.
    pub fn with_scanner(mut self, range: i32) -> Self {
        self.senses |= SCANNER;
        self.scanner_range = range;
        self
    }

    /// Sets the hearing scale of `channel` (per 256); a channel past
    /// [`MAX_CHANNELS`] changes nothing.
    pub fn with_hearing(mut self, channel: u8, scale: u16) -> Self {
        if let Some(h) = self.hearing.get_mut(usize::from(channel)) {
            *h = scale;
        }
        self
    }

    /// Puts `rule` into the first unused cue slot (`bits` 0); unchanged
    /// when all [`MAX_CUES`] are in use.
    pub fn with_cue(mut self, rule: CueRule) -> Self {
        if let Some(slot) = self.cues.iter_mut().find(|c| c.bits == 0) {
            *slot = rule;
        }
        self
    }
}

/// The observer's stats, passed on every call, so a game stat or a boost
/// acts without touching the profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Stats {
    /// Sight range × / 256 (a perception stat, eye upgrades).
    pub sight_scale: u16,
    /// Gain × / 256 (attentiveness).
    pub notice_scale: u16,
    /// Hearing range × / 256.
    pub hearing_scale: u16,
    /// Ticks before a new stage counts; read on every call.
    pub reaction_ticks: u16,
}

impl Stats {
    /// Everything at 256, no reaction delay.
    pub const NORMAL: Stats = Stats {
        sight_scale: 256,
        notice_scale: 256,
        hearing_scale: 256,
        reaction_ticks: 0,
    };
}

/// How an observer sees through a disguise (only when the game turns it
/// on).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct SeeThrough {
    /// Doubt per credited tick at disguise 0 and full closeness.
    pub rate: u16,
    /// Full closeness up to this distance, falling to 0 at the sight
    /// range.
    pub near: i32,
    /// Doubt at which the disguise no longer holds for this observer.
    pub at: u16,
}

/// What a disguise does for this observer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct DisguiseRule {
    /// The cue bits a disguise masks.
    pub hides: u32,
    /// `None` (the default): a disguise always holds. `Some`: the observer
    /// may see through it.
    pub see_through: Option<SeeThrough>,
}

/// One observer for one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Eye<'a> {
    /// The game's id of the observer (becomes `Percept::observer`).
    pub id: u32,
    /// Where the eye is, fine point.
    pub point: Pos,
    /// Facing heading (0..=255, 0 = +x, 64 = +y).
    pub facing: u8,
    /// What it can sense.
    pub profile: &'a SenseProfile,
    /// Its stats for this call.
    pub stats: Stats,
    /// What a disguise does for it.
    pub disguise: DisguiseRule,
}
