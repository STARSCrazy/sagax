/// Most thresholds an [`AlertConfig`] can hold.
const MAX_LEVELS: usize = 8;

/// How alertness behaves: a ceiling, linear decay after a hold time, and
/// the thresholds of the levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct AlertConfig {
    /// Highest value.
    pub max: u32,
    /// Value lost per tick once the hold is over.
    pub decay_per_tick: u32,
    /// Ticks after a stimulus during which nothing decays.
    pub hold: u32,
    /// How far below a threshold the value must drop before the level falls.
    pub hysteresis: u32,
    /// Number of thresholds used (more than 8 counts as 8).
    pub count: u32,
    /// Ascending thresholds; level `n` means the first `n` are reached.
    pub thresholds: [u32; MAX_LEVELS],
}

/// A value that rises with stimuli, decays with time, and maps to a level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Alertness {
    value: u32,
    last: u32,
    last_stimulus: u32,
    level: u8,
}

impl Alertness {
    /// Adds `amount` (capped at `cfg.max`) and notes the time. Call
    /// [`update`](Self::update) to get the new level.
    pub fn stimulus(&mut self, amount: u32, now: u32, cfg: &AlertConfig) {
        self.value = self.value.saturating_add(amount).min(cfg.max);
        self.last_stimulus = now;
    }

    /// Decays for the ticks since the last update that lie after the hold
    /// (`last_stimulus + hold`), then finds the level: it rises straight to
    /// the highest threshold reached and falls straight to the level whose
    /// threshold minus `hysteresis` is still met. Returns `(from, to)` if
    /// the level changed.
    pub fn update(&mut self, now: u32, cfg: &AlertConfig) -> Option<(u8, u8)> {
        let since_last = now.wrapping_sub(self.last);
        let after_hold = now
            .wrapping_sub(self.last_stimulus)
            .saturating_sub(cfg.hold);
        let ticks = since_last.min(after_hold);
        let lost = u64::from(cfg.decay_per_tick) * u64::from(ticks);
        self.value = u64::from(self.value).saturating_sub(lost) as u32;
        self.last = now;

        let n = (cfg.count as usize).min(MAX_LEVELS);
        let th = &cfg.thresholds[..n];
        let from = self.level;
        let reached = th.iter().filter(|&&t| self.value >= t).count() as u8;
        let mut to = from;
        if reached > to {
            to = reached;
        } else {
            while to > 0
                && th
                    .get(usize::from(to) - 1)
                    .is_some_and(|&t| self.value < t.saturating_sub(cfg.hysteresis))
            {
                to -= 1;
            }
        }
        self.level = to;
        (to != from).then_some((from, to))
    }

    /// Current value.
    pub fn value(&self) -> u32 {
        self.value
    }

    /// Current level (0 = below every threshold).
    pub fn level(&self) -> u8 {
        self.level
    }
}
