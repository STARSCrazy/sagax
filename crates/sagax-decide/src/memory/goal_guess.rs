//! Goal recognition: where is the one I watch heading? ([`GoalGuess`])
//!
//! A classic Bayes recogniser over up to [`MAX_GOALS`] candidate goals.
//! It assumes the walker is "mostly sensible": a step that brings him
//! closer to a goal is more likely if that goal is his. For every observed
//! step the game gives, per goal, the change of walking distance of the
//! step taken and of every step he could have taken; the step's likelihood
//! for goal `k` is `w(moved_k) / Σ_j w(options_k[j])` with
//! `w(x) = exp(-beta * x / unit)` (Boltzmann). The chances are updated by
//! Bayes' rule and then a share of uniform is mixed back in ("forgetting"),
//! so a feint is recovered from quickly.
//!
//! No cheating: the block knows only the steps the game feeds it. Feed only
//! what the observer saw (its own percepts), never the true goal or the
//! true position.
//!
//! Integer only: chances are kept in 1/65536, `w` is an integer
//! exponential (see [`GoalGuessConfig`]). The same steps give the same
//! chances on every machine and in every interface. No heap: the whole
//! guess is a plain value.

use sagax_core::{NoopSink, Sink};

/// Most candidate goals one [`GoalGuess`] holds.
pub const MAX_GOALS: usize = 8;

/// Most moves of one observed step (per goal) [`GoalGuess::observe`] takes.
pub const MAX_MOVES: usize = 32;

/// Number of Decider columns [`GoalGuess::write_columns`] writes.
pub const GOAL_COLUMNS: usize = 2;

/// The internal precision of a chance: all chances sum to this.
const SCALE: u32 = 1 << 16;

/// Chances as read: all chances sum to this (Sagax fractions).
const SHARE: u32 = 256;

/// How the walker is modelled.
///
/// `w(x) = exp(-t)` with `t = beta * x / (256 * unit)` is computed as
/// `2^(-t / ln 2)`: `t / ln 2` in 1/65536 (truncated), the fraction by 16
/// fixed factors in 1/2^32, the whole part by a shift. The relative error
/// of `w` is below 2^-15; weights below 2^-63 of the best option count as
/// 0 (and a step's likelihood never drops below 1/65536).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct GoalGuessConfig {
    /// How sensible the walker is, in 1/256 per `unit` of distance: 512
    /// (= 2 per straight step) is the measured default; 0 makes every step
    /// teach nothing.
    pub beta: u32,
    /// The cost of one straight step (10 with the octile preset; 0 counts
    /// as 1).
    pub unit: u32,
    /// Share of uniform (0..256, more counts as 256) mixed back in after
    /// every step; 13 (about 5 %) is the measured default, 0 never forgets.
    pub forget: u32,
}

impl Default for GoalGuessConfig {
    /// beta 512 (2 per step), unit 10, forget 13 (about 5 %).
    fn default() -> Self {
        Self {
            beta: 512,
            unit: 10,
            forget: 13,
        }
    }
}

/// Why an observed step was refused (nothing changed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepError {
    /// The numbers do not fit the guess: `per_goal` is 0 or above
    /// [`MAX_MOVES`], a slice is too short for the goals, or (flow fields)
    /// the number of fields is not the number of goals.
    Shape,
    /// The new place is neither the old one nor one of its moves (a jump,
    /// for example after the observer lost sight): start a new chain at the
    /// new place instead.
    Jump,
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape => write!(f, "the step does not fit the goals"),
            Self::Jump => write!(f, "the new place is not a move from the old one"),
        }
    }
}

impl std::error::Error for StepError {}

/// One record of a traced goal-guess call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GoalGuessTrace {
    /// One goal after an observed step (one record per goal, in order).
    Step {
        /// The goal.
        goal: u8,
        /// The change of walking distance of the step taken.
        moved: i32,
        /// The step's likelihood for this goal, in 1/65536 (1..65536).
        likelihood: u32,
        /// The new chance, 0..256.
        chance: u32,
    },
    /// A new prior: one record per goal with its chance, 0..256.
    Prior {
        /// The goal.
        goal: u8,
        /// Its chance now.
        chance: u32,
    },
    /// Back to the prior.
    Reset,
    /// An observed step was refused; nothing changed.
    Refused {
        /// Why.
        error: StepError,
    },
}

/// `2^(-2^-(i+1))` in 1/2^32, for the bits of a fraction (bit 15 first).
const EXP2_BITS: [u64; 16] = [
    3037000500, 3611622603, 3938502376, 4112874773, 4202935003, 4248701965, 4271771996, 4283353945,
    4289156690, 4292061010, 4293513907, 4294240540, 4294603903, 4294785595, 4294876445, 4294921870,
];

/// `2^32 / ln 2`, rounded.
const LOG2E_Q32: u128 = 6196328019;

/// `exp(-beta * a / (256 * unit))` in 1/2^32 (`a >= 0`).
fn weight(beta: u32, unit: u32, a: u64) -> u64 {
    let unit = u128::from(unit.max(1));
    // t / ln 2 in 1/65536.
    let s = u128::from(beta) * u128::from(a) * LOG2E_Q32 / ((256 * unit) << 16);
    let whole = s >> 16;
    if whole >= 64 {
        return 0;
    }
    let frac = (s & 0xFFFF) as u32;
    let mut r: u64 = 1 << 32;
    for (i, &c) in EXP2_BITS.iter().enumerate() {
        if frac & (1 << (15 - i)) != 0 {
            r = ((u128::from(r) * u128::from(c)) >> 32) as u64;
        }
    }
    r >> whole
}

/// Shares `total` over `weights` in proportion (largest remainder; ties of
/// the remainder go to the lower index) into `out`. All weights 0: all 0.
fn apportion(weights: &[u64], total: u32, out: &mut [u32]) {
    let sum: u128 = weights.iter().map(|&w| u128::from(w)).sum();
    let n = weights.len().min(out.len());
    if sum == 0 {
        out[..n].fill(0);
        return;
    }
    let mut rest = [0u128; MAX_GOALS];
    let mut given = 0u32;
    for i in 0..n {
        let x = u128::from(weights[i]) * u128::from(total);
        out[i] = (x / sum) as u32;
        rest[i] = x % sum;
        given += out[i];
    }
    for _ in given..total {
        let mut best = 0;
        for i in 1..n {
            if rest[i] > rest[best] {
                best = i;
            }
        }
        out[best] += 1;
        rest[best] = 0;
    }
}

/// Per-observer goal recognition over up to [`MAX_GOALS`] candidate goals:
/// a chance for each, updated from observed steps (see the module docs).
///
/// A plain value of fixed size: no heap, `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GoalGuess {
    config: GoalGuessConfig,
    goals: u8,
    /// The prior, in 1/65536 (sums to 65536 when `goals > 0`).
    prior: [u32; MAX_GOALS],
    /// The chances now, in 1/65536 (sums to 65536 when `goals > 0`).
    chance: [u32; MAX_GOALS],
}

impl GoalGuess {
    /// A guess over `goals` candidate goals (more than [`MAX_GOALS`] count
    /// as [`MAX_GOALS`]) with a uniform prior.
    pub fn new(config: GoalGuessConfig, goals: usize) -> Self {
        let mut g = Self {
            config,
            goals: goals.min(MAX_GOALS) as u8,
            prior: [0; MAX_GOALS],
            chance: [0; MAX_GOALS],
        };
        g.set_prior(&[]);
        g
    }

    /// The config.
    pub fn config(&self) -> &GoalGuessConfig {
        &self.config
    }

    /// The number of candidate goals.
    pub fn goals(&self) -> usize {
        usize::from(self.goals)
    }

    /// [`set_prior_traced`](Self::set_prior_traced) with [`NoopSink`].
    pub fn set_prior(&mut self, weights: &[u32]) {
        self.set_prior_traced(weights, &mut NoopSink);
    }

    /// Sets the prior from `weights` (one per goal, in any unit; a missing
    /// weight is 0, extra ones are ignored) and starts over from it. A goal
    /// with weight 0 is ruled out: it stays at 0 (forgetting skips it)
    /// until a new prior. All weights 0 (or none) is the uniform prior. One
    /// `Prior` record per goal.
    pub fn set_prior_traced(&mut self, weights: &[u32], sink: &mut impl Sink<GoalGuessTrace>) {
        let n = self.goals();
        let mut w = [0u64; MAX_GOALS];
        for (i, slot) in w[..n].iter_mut().enumerate() {
            *slot = u64::from(weights.get(i).copied().unwrap_or(0));
        }
        if w[..n].iter().all(|&x| x == 0) {
            w[..n].fill(1);
        }
        self.prior = [0; MAX_GOALS];
        apportion(&w[..n], SCALE, &mut self.prior[..n]);
        self.chance = self.prior;
        if sink.enabled() {
            let shares = self.shares();
            for (goal, &chance) in shares[..n].iter().enumerate() {
                sink.emit(GoalGuessTrace::Prior {
                    goal: goal as u8,
                    chance,
                });
            }
        }
    }

    /// [`reset_traced`](Self::reset_traced) with [`NoopSink`].
    pub fn reset(&mut self) {
        self.reset_traced(&mut NoopSink);
    }

    /// Back to the prior (a `Reset` record).
    pub fn reset_traced(&mut self, sink: &mut impl Sink<GoalGuessTrace>) {
        self.chance = self.prior;
        sink.emit(GoalGuessTrace::Reset);
    }

    /// [`observe_traced`](Self::observe_traced) with [`NoopSink`].
    pub fn observe(
        &mut self,
        moved: &[i32],
        options: &[i32],
        per_goal: usize,
    ) -> Result<(), StepError> {
        self.observe_traced(moved, options, per_goal, &mut NoopSink)
    }

    /// Learns from one observed step. `moved[k]` is the change of walking
    /// distance to goal `k` of the step taken (`d_k(cur) - d_k(prev)`);
    /// `options[k * per_goal + j]` the change of move `j` of the
    /// `per_goal` moves possible from `prev` (the same moves for every
    /// goal; standing still is not a move). The likelihood for goal `k` is
    /// `w(moved_k) / Σ_j w(options_k[j])`, at most 1 and at least 1/65536;
    /// the chances become prior × likelihood, normalised (a goal not ruled
    /// out keeps at least 1/65536, so it can always come back), then
    /// `forget` of uniform over the goals not ruled out is mixed back in. Chances
    /// keep summing to exactly 65536 internally (256 as read).
    ///
    /// A step that stands still teaches nothing: the game simply does not
    /// call this. Returns [`StepError::Shape`] (a `Refused` record, nothing
    /// changed) when `per_goal` is 0 or above [`MAX_MOVES`] or a slice is
    /// too short; else one `Step` record per goal.
    pub fn observe_traced(
        &mut self,
        moved: &[i32],
        options: &[i32],
        per_goal: usize,
        sink: &mut impl Sink<GoalGuessTrace>,
    ) -> Result<(), StepError> {
        let n = self.goals();
        if per_goal == 0 || per_goal > MAX_MOVES || moved.len() < n || options.len() < n * per_goal
        {
            let error = StepError::Shape;
            sink.emit(GoalGuessTrace::Refused { error });
            return Err(error);
        }
        let GoalGuessConfig { beta, unit, forget } = self.config;
        let mut likelihood = [0u32; MAX_GOALS];
        let mut product = [0u64; MAX_GOALS];
        for k in 0..n {
            let opts = &options[k * per_goal..(k + 1) * per_goal];
            let low = opts.iter().copied().min().unwrap_or(0).min(moved[k]);
            let w = |x: i32| weight(beta, unit, (i64::from(x) - i64::from(low)) as u64);
            let sum: u128 = opts.iter().map(|&x| u128::from(w(x))).sum();
            let l = (u128::from(w(moved[k])) * u128::from(SCALE))
                .checked_div(sum)
                .map_or(SCALE, |l| l.min(u128::from(SCALE)) as u32);
            likelihood[k] = l.max(1);
            product[k] = u64::from(self.chance[k]) * u64::from(likelihood[k]);
        }
        // Some chance is above 0 and every likelihood is at least 1, so
        // the product never sums to 0.
        apportion(&product[..n], SCALE, &mut self.chance[..n]);
        self.keep_floor();
        self.forget(forget.min(SHARE));
        if sink.enabled() {
            let shares = self.shares();
            for k in 0..n {
                sink.emit(GoalGuessTrace::Step {
                    goal: k as u8,
                    moved: moved[k],
                    likelihood: likelihood[k],
                    chance: shares[k],
                });
            }
        }
        Ok(())
    }

    /// Lifts every goal not ruled out to at least 1/65536 (taken from the
    /// most likely one), so it can come back even without forgetting.
    fn keep_floor(&mut self) {
        let n = self.goals();
        for k in 0..n {
            if self.prior[k] > 0 && self.chance[k] == 0 {
                let (top, _) = self.best().expect("goals");
                self.chance[top] -= 1;
                self.chance[k] = 1;
            }
        }
    }

    /// Mixes `forget / 256` of uniform over the goals with a non-zero prior
    /// into the chances.
    fn forget(&mut self, forget: u32) {
        let n = self.goals();
        let live = self.prior[..n].iter().filter(|&&p| p > 0).count() as u64;
        if forget == 0 || live == 0 {
            return;
        }
        // Common denominator 256 * 65536 * live.
        let mut mixed = [0u64; MAX_GOALS];
        for ((m, &p), &c) in mixed.iter_mut().zip(&self.prior[..n]).zip(&self.chance) {
            if p > 0 {
                *m = u64::from(c) * u64::from(SHARE - forget) * live
                    + u64::from(forget) * u64::from(SCALE);
            }
        }
        apportion(&mixed[..n], SCALE, &mut self.chance[..n]);
    }

    /// The chances of all goals as read, 0..256, summing to exactly 256
    /// (largest remainder; ties to the lower goal). Entries past `goals`
    /// are 0.
    pub fn shares(&self) -> [u32; MAX_GOALS] {
        let n = self.goals();
        let mut w = [0u64; MAX_GOALS];
        for (slot, &c) in w.iter_mut().zip(&self.chance[..n]) {
            *slot = u64::from(c);
        }
        let mut out = [0u32; MAX_GOALS];
        apportion(&w[..n], SHARE, &mut out[..n]);
        out
    }

    /// The chance of `goal`, 0..256 (0 for a goal out of range). The
    /// chances of all goals sum to exactly 256 (see [`shares`](Self::shares)).
    pub fn chance(&self, goal: usize) -> u32 {
        if goal < self.goals() {
            self.shares()[goal]
        } else {
            0
        }
    }

    /// The chance of `goal` in the internal precision, 1/65536.
    pub fn chance_fine(&self, goal: usize) -> u32 {
        if goal < self.goals() {
            self.chance[goal]
        } else {
            0
        }
    }

    /// The most likely goal with its chance (0..256); ties go to the lower
    /// goal (compared in the internal precision). `None` without goals.
    pub fn best(&self) -> Option<(usize, u32)> {
        let n = self.goals();
        let mut best = None;
        for k in 0..n {
            if best.is_none_or(|b: usize| self.chance[k] > self.chance[b]) {
                best = Some(k);
            }
        }
        best.map(|b| (b, self.chance(b)))
    }

    /// The rank of `goal`: 0 is the most likely, ties by lower goal
    /// (compared in the internal precision). A goal out of range gets
    /// [`goals`](Self::goals).
    pub fn rank(&self, goal: usize) -> usize {
        let n = self.goals();
        if goal >= n {
            return n;
        }
        let c = self.chance[goal];
        (0..n)
            .filter(|&k| self.chance[k] > c || (self.chance[k] == c && k < goal))
            .count()
    }

    /// Writes the [`GOAL_COLUMNS`] Decider columns `chance, rank` of `goal`
    /// to the start of `row` (pass `&mut row[first..]`); a shorter `row`
    /// gets only the columns that fit (a debug build asserts the room). A
    /// goal out of range writes `0, goals`.
    pub fn write_columns(&self, goal: usize, row: &mut [i32]) {
        debug_assert!(
            row.len() >= GOAL_COLUMNS,
            "write_columns: the row needs {GOAL_COLUMNS} columns"
        );
        let cols = [self.chance(goal) as i32, self.rank(goal) as i32];
        let n = row.len().min(GOAL_COLUMNS);
        row[..n].copy_from_slice(&cols[..n]);
    }

    /// Shares `hunters` over the goals by chance (largest remainder in the
    /// internal precision; ties to the lower goal) into `out[..goals]`;
    /// entries past the goals are set to 0, and a shorter `out` gets only the
    /// goals that fit (the share is still over all goals). Three hunters with chances
    /// 60/30/10 % give 2, 1, 0.
    pub fn split(&self, hunters: u32, out: &mut [u32]) {
        let n = self.goals();
        let mut w = [0u64; MAX_GOALS];
        for (slot, &c) in w.iter_mut().zip(&self.chance[..n]) {
            *slot = u64::from(c);
        }
        let mut all = [0u32; MAX_GOALS];
        apportion(&w[..n], hunters, &mut all[..n]);
        out.fill(0);
        let fit = n.min(out.len());
        out[..fit].copy_from_slice(&all[..fit]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_follows_exp_closely() {
        assert_eq!(weight(512, 10, 0), 1 << 32);
        for a in [1u64, 5, 10, 14, 20, 37, 100, 200] {
            for beta in [128u32, 512, 1000] {
                let exact = (-(f64::from(beta) * a as f64) / (256.0 * 10.0)).exp() * 4294967296.0;
                let got = weight(beta, 10, a) as f64;
                assert!(
                    (got - exact).abs() <= exact / 32768.0 + 1.0,
                    "beta {beta}, a {a}: {got} vs {exact}"
                );
            }
        }
        assert_eq!(weight(512, 10, 10_000), 0);
        assert_eq!(weight(512, 0, 1), weight(512, 1, 1), "unit 0 counts as 1");
    }

    #[test]
    fn apportion_is_exact() {
        let mut out = [0u32; 3];
        apportion(&[1, 1, 1], 256, &mut out);
        assert_eq!(out, [86, 85, 85]);
        apportion(&[0, 0, 0], 256, &mut out);
        assert_eq!(out, [0, 0, 0]);
        apportion(&[u64::MAX, u64::MAX, 1], u32::MAX, &mut out);
        assert_eq!(
            out.iter().map(|&x| u64::from(x)).sum::<u64>(),
            u64::from(u32::MAX)
        );
    }
}
