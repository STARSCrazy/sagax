//! Decisions for game AI: score every pair of option and target, then choose.
//!
//! This is "utility AI". A [`Decider`] holds the options a character has
//! (attack, flee, heal, ...). Each option has a weight and a list of
//! [`Consideration`]s. A consideration reads one raw number the game measured
//! for a candidate (distance, own health, enemies near, ...) and turns it
//! into a factor between 0 and 1 through a [`Curve`]. A [`pull`] sets how
//! much the factor counts. The factors of an option are put together by a
//! [`Combine`] (product by default); factors marked [`Apply::Multiply`] are
//! multiplied onto the result afterwards.
//!
//! A [`Candidate`] is one pair of option and target. The game fills one row
//! of raw inputs per candidate; [`Decider::decide`] scores all rows and
//! chooses one by [`Pick`]. Options can have a rank (emergency before
//! combat before idle), and the action going on now can get a bonus so a
//! character does not flip between two nearly equal actions.
//!
//! Everything is integer arithmetic: 0..1 is 0..[`ONE`] (65535). The same
//! inputs, options and seed give the same choice on every machine and in
//! every interface (Rust, C, JavaScript). There is no global random number
//! generator; the seed comes from the caller. On equal scores the candidate
//! with the smaller index wins.
//!
//! What happened can be followed factor by factor through a
//! [`sagax_core::Sink`] of [`DecideEvent`]s.

pub mod state;

use std::cell::RefCell;

use sagax_core::{NoopSink, Sink};

/// 1.0 in the fixed point of factors and weights.
pub const ONE: u32 = 65535;

/// Input number of a consideration that reads the time since this option
/// was last done (any target), from the decider's memory instead of the
/// row. Never done: `i32::MAX`.
pub const INPUT_SINCE_OPTION: u32 = 0xFFFF_FFF0;
/// Time since this option was last done on this target.
pub const INPUT_SINCE_PAIR: u32 = 0xFFFF_FFF1;
/// How often this option was done within the window (any target).
pub const INPUT_COUNT_OPTION: u32 = 0xFFFF_FFF2;
/// How often this option was done on this target within the window.
pub const INPUT_COUNT_PAIR: u32 = 0xFFFF_FFF3;

/// The decider remembers at most this many recorded actions; older ones
/// are forgotten first.
pub const MEMORY_LIMIT: usize = 256;

/// How a raw input becomes a factor 0..[`ONE`].
///
/// All curves first place the input between `min` and `max`:
/// `t = (input - min) / (max - min)`, clamped to 0..1. If `max <= min`, `t`
/// is 1 for `input >= max` and 0 below.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Curve {
    /// `t`: more is better.
    Linear = 0,
    /// `1 - t`: less is better.
    Inverse = 1,
    /// `t * t`: only high values count much.
    Quadratic = 2,
    /// `(1 - t) * (1 - t)`: only low values count much.
    InverseQuadratic = 3,
    /// 1 if `input >= min`, else 0 (`max` unused).
    Step = 4,
    /// 1 if `input < min`, else 0 (`max` unused).
    InverseStep = 5,
    /// S-curve `3t² - 2t³`: flat near both ends, steep in the middle; a
    /// threshold that switches softly.
    SCurve = 6,
    /// `1 - SCurve`.
    InverseSCurve = 7,
    /// `√t`: rises fast, then flattens; early values already count.
    Root = 8,
    /// `t³`: only values near the top count.
    Cubic = 9,
}

impl Curve {
    /// The curve for its number (as in C and JavaScript), or `None`.
    pub const fn from_u32(n: u32) -> Option<Self> {
        Some(match n {
            0 => Self::Linear,
            1 => Self::Inverse,
            2 => Self::Quadratic,
            3 => Self::InverseQuadratic,
            4 => Self::Step,
            5 => Self::InverseStep,
            6 => Self::SCurve,
            7 => Self::InverseSCurve,
            8 => Self::Root,
            9 => Self::Cubic,
            _ => return None,
        })
    }

    /// The factor for `input`, 0..[`ONE`].
    pub fn apply(self, input: i32, min: i32, max: i32) -> u32 {
        match self {
            Self::Step => return if input >= min { ONE } else { 0 },
            Self::InverseStep => return if input < min { ONE } else { 0 },
            _ => {}
        }
        let t = place(input, min, max);
        match self {
            Self::Linear => t,
            Self::Inverse => ONE - t,
            Self::Quadratic => mul(t, t),
            Self::InverseQuadratic => mul(ONE - t, ONE - t),
            Self::SCurve => smooth(t),
            Self::InverseSCurve => ONE - smooth(t),
            Self::Root => sqrt_fixed(t),
            Self::Cubic => mul(mul(t, t), t),
            Self::Step | Self::InverseStep => unreachable!(),
        }
    }
}

/// `(input - min) / (max - min)` clamped, in 0..ONE, rounded down.
fn place(input: i32, min: i32, max: i32) -> u32 {
    if max <= min {
        return if input >= max { ONE } else { 0 };
    }
    if input <= min {
        return 0;
    }
    if input >= max {
        return ONE;
    }
    let span = i64::from(max) - i64::from(min);
    let above = i64::from(input) - i64::from(min);
    // 0 < above < span, so the quotient is below ONE.
    (above * i64::from(ONE) / span) as u32
}

/// `a * b` in fixed point, rounded down.
fn mul(a: u32, b: u32) -> u32 {
    (u64::from(a) * u64::from(b) / u64::from(ONE)) as u32
}

/// `3t² - 2t³` in fixed point.
fn smooth(t: u32) -> u32 {
    let t2 = mul(t, t);
    let t3 = mul(t2, t);
    3 * t2 - 2 * t3
}

/// `√t` in fixed point, rounded down: the largest `x` with `x² <= t * ONE`.
fn sqrt_fixed(t: u32) -> u32 {
    let target = u64::from(t) * u64::from(ONE);
    let (mut lo, mut hi) = (0u64, u64::from(ONE));
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if mid * mid <= target {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo as u32
}

/// One factor of an option: which input, which curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Consideration {
    /// Index into the candidate's row of inputs.
    pub input: u32,
    /// How the input becomes a factor.
    pub curve: Curve,
    /// Lower end of the curve.
    pub min: i32,
    /// Upper end of the curve.
    pub max: i32,
}

impl Consideration {
    /// A consideration.
    pub const fn new(input: u32, curve: Curve, min: i32, max: i32) -> Self {
        Self {
            input,
            curve,
            min,
            max,
        }
    }
}

/// How the factors of an option are put together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Combine {
    /// Multiply, compensated for the number of factors. Any 0 rules out;
    /// many good-but-not-perfect factors still pull the score down.
    #[default]
    Product = 0,
    /// Geometric mean (the n-th root of the product). Any 0 rules out, but
    /// three times 0.95 stays 0.95: the number of factors does not matter.
    GeometricMean = 1,
    /// The weakest factor decides.
    Minimum = 2,
    /// Arithmetic mean: factors make up for each other; a 0 does not rule out.
    Average = 3,
    /// The middle factor: single outliers, high or low, do not count.
    Median = 4,
}

impl Combine {
    /// The combine for its number (as in C and JavaScript), or `None`.
    pub const fn from_u32(n: u32) -> Option<Self> {
        Some(match n {
            0 => Self::Product,
            1 => Self::GeometricMean,
            2 => Self::Minimum,
            3 => Self::Average,
            4 => Self::Median,
            _ => return None,
        })
    }
}

/// Where a factor goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Apply {
    /// Into the option's [`Combine`] with the other factors.
    #[default]
    Combine = 0,
    /// Multiplied onto the result afterwards, whatever the combine is: a
    /// true factor. For hard rules ("no ammunition": a step to 0) or one
    /// thing that must always count fully.
    Multiply = 1,
}

impl Apply {
    /// The apply for its number (0 combine, 1 multiply), or `None`.
    pub const fn from_u32(n: u32) -> Option<Self> {
        Some(match n {
            0 => Self::Combine,
            1 => Self::Multiply,
            _ => return None,
        })
    }
}

/// How much a factor counts, applied after its curve: the factor `v`
/// becomes `1 - pull * (1 - v)`, at least 0. `pull` [`ONE`] keeps it; less
/// makes it gentle (with 0.5 it never drops below 0.5); more makes it strict
/// (with 2 it is 0 from `v` = 0.5 down); 0 switches it off.
pub fn pull(value: u32, pull: u32) -> u32 {
    if pull == ONE {
        return value;
    }
    let drop = u64::from(ONE - value.min(ONE)) * u64::from(pull) / u64::from(ONE);
    ONE - drop.min(u64::from(ONE)) as u32
}

/// The geometric mean of `values` in fixed point, rounded down.
pub fn geometric_mean(values: &[u32]) -> u32 {
    if values.is_empty() {
        return ONE;
    }
    if values.contains(&0) {
        return 0;
    }
    // Products in Q32 (32 fraction bits).
    const Q: u64 = 1 << 32;
    let to_q = |v: u32| u64::from(v) * Q / u64::from(ONE);
    let times = |p: u64, q: u64| ((u128::from(p) * u128::from(q)) >> 32) as u64;
    let product = values.iter().fold(Q, |p, &v| times(p, to_q(v)));
    let power = |x: u32| {
        let q = to_q(x);
        values.iter().fold(Q, |p, _| times(p, q))
    };
    // The largest x with x^n <= product.
    let (mut lo, mut hi) = (0u32, ONE);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if power(mid) <= product {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// Puts `values` together by `how`; `values` may be reordered. No values: 1.
pub fn combine(how: Combine, values: &mut [u32]) -> u32 {
    let n = values.len();
    if n == 0 {
        return ONE;
    }
    match how {
        Combine::Product => compensate(values.iter().fold(ONE, |p, &v| mul(p, v)), n),
        Combine::GeometricMean => geometric_mean(values),
        Combine::Minimum => values.iter().copied().min().unwrap_or(ONE),
        Combine::Average => (values.iter().map(|&v| u64::from(v)).sum::<u64>() / n as u64) as u32,
        Combine::Median => {
            values.sort_unstable();
            if n % 2 == 1 {
                values[n / 2]
            } else {
                ((u64::from(values[n / 2 - 1]) + u64::from(values[n / 2])) / 2) as u32
            }
        }
    }
}

/// One pair to score: an option and a target. The target is the game's own
/// number (a token, a cell index, a weapon); Sagax only carries it along.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Candidate {
    /// Option from [`Decider::add_option`].
    pub option: u32,
    /// The game's target number.
    pub target: u32,
}

impl Candidate {
    /// A candidate.
    pub const fn new(option: u32, target: u32) -> Self {
        Self { option, target }
    }
}

/// How to choose among the scored candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    /// The highest score; on equal scores the smaller index.
    Best,
    /// Among the `top` best (0: all), by chance in proportion to the score.
    Weighted {
        /// How many of the best take part; 0 for all.
        top: u32,
    },
    /// Among the `top` best (0: all), each equally likely.
    Uniform {
        /// How many of the best take part; 0 for all.
        top: u32,
    },
}

impl Pick {
    /// The pick for its number (0 best, 1 weighted, 2 uniform) and `top`.
    pub const fn from_u32(mode: u32, top: u32) -> Option<Self> {
        Some(match mode {
            0 => Self::Best,
            1 => Self::Weighted { top },
            2 => Self::Uniform { top },
            _ => return None,
        })
    }
}

/// What [`Decider::decide`] chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Choice {
    /// Index into the candidates.
    pub candidate: usize,
    /// Its score (weight times factors, [`ONE`] = 1.0).
    pub score: u32,
}

/// Why [`Decider::decide`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecideError {
    /// A candidate names an option that does not exist.
    UnknownOption {
        /// Index of the candidate.
        candidate: usize,
    },
    /// A consideration reads an input beyond the row.
    InputOutOfRange {
        /// Index of the candidate.
        candidate: usize,
        /// The input it wanted.
        input: u32,
    },
    /// `inputs` is shorter than `candidates.len() * stride`.
    InputsTooShort,
}

impl std::fmt::Display for DecideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownOption { candidate } => {
                write!(f, "candidate {candidate} names an unknown option")
            }
            Self::InputOutOfRange { candidate, input } => {
                write!(f, "candidate {candidate} has no input {input}")
            }
            Self::InputsTooShort => write!(f, "fewer inputs than candidates times stride"),
        }
    }
}

impl std::error::Error for DecideError {}

/// What a decision did, for debugging and logs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecideEvent {
    /// One consideration of one candidate.
    Factor {
        /// Index of the candidate.
        candidate: usize,
        /// Index of the consideration within its option.
        consideration: usize,
        /// The raw input.
        input: i32,
        /// The factor the curve made of it, after its pull.
        value: u32,
    },
    /// The final score of one candidate.
    Scored {
        /// The factors put together (see [`Combine`] and [`Apply`]), before
        /// the weight.
        product: u32,
        /// Index of the candidate.
        candidate: usize,
        /// Score after the weight and the bonus for the current action.
        score: u32,
    },
    /// The chosen candidate.
    Chosen {
        /// Index of the candidate.
        candidate: usize,
        /// Its score.
        score: u32,
    },
}

#[derive(Clone, Copy, Debug)]
struct Factor {
    consideration: Consideration,
    pull: u32,
    apply: Apply,
}

#[derive(Clone, Debug)]
struct OptionDef {
    weight: u32,
    rank: u32,
    cooldown: u32,
    combine: Option<Combine>,
    factors: Vec<Factor>,
}

/// Options with their considerations; scores candidates and chooses one.
#[derive(Clone, Debug, Default)]
pub struct Decider {
    options: Vec<OptionDef>,
    combine: Combine,
    current: Option<(Candidate, u32)>,
    now: u32,
    window: u32,
    memory: std::collections::VecDeque<(Candidate, u32)>,
    order: Vec<usize>,
    values: RefCell<Vec<u32>>,
    ranked: Vec<u32>,
}

impl Decider {
    /// A decider without options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an option with `weight` ([`ONE`] = 1.0; more than `ONE` is
    /// allowed) and returns its number, counting from 0.
    pub fn add_option(&mut self, weight: u32) -> u32 {
        self.options.push(OptionDef {
            weight,
            rank: 0,
            cooldown: 0,
            combine: None,
            factors: Vec::new(),
        });
        (self.options.len() - 1) as u32
    }

    /// Adds a consideration to `option` (pull [`ONE`], [`Apply::Combine`]).
    /// `false` if the option does not exist.
    pub fn add_consideration(&mut self, option: u32, consideration: Consideration) -> bool {
        self.add_weighted(option, consideration, ONE, Apply::Combine)
    }

    /// Adds a consideration with its [`pull`] and where it goes. `false` if
    /// the option does not exist.
    pub fn add_weighted(
        &mut self,
        option: u32,
        consideration: Consideration,
        pull: u32,
        apply: Apply,
    ) -> bool {
        match self.options.get_mut(option as usize) {
            Some(o) => {
                o.factors.push(Factor {
                    consideration,
                    pull,
                    apply,
                });
                true
            }
            None => false,
        }
    }

    /// How factors are put together, for every option without its own
    /// (default [`Combine::Product`]).
    pub fn set_combine(&mut self, combine: Combine) {
        self.combine = combine;
    }

    /// `option` puts its factors together its own way; `None` follows the
    /// decider. `false` if the option does not exist.
    pub fn set_option_combine(&mut self, option: u32, combine: Option<Combine>) -> bool {
        self.options
            .get_mut(option as usize)
            .map(|o| o.combine = combine)
            .is_some()
    }

    /// Priority of `option` (default 0). Only candidates of the highest rank
    /// that still scores above 0 take part in the choice: e.g. 2 emergency,
    /// 1 combat, 0 idle. `false` if the option does not exist.
    pub fn set_rank(&mut self, option: u32, rank: u32) -> bool {
        self.options
            .get_mut(option as usize)
            .map(|o| o.rank = rank)
            .is_some()
    }

    /// The action going on now gets `bonus` on top (score times
    /// `1 + bonus`, [`ONE`] = 1.0), so a character does not flip between
    /// two nearly equal actions. `None` for none.
    pub fn set_current(&mut self, current: Option<Candidate>, bonus: u32) {
        self.current = current.map(|c| (c, bonus));
    }

    /// The game's time for the next decisions (frames, turns, seconds; it
    /// may wrap around). Needed for cooldowns and the memory inputs.
    pub fn set_now(&mut self, now: u32) {
        self.now = now;
    }

    /// Remembers that `candidate` was done at `now`. Only the game knows
    /// whether the chosen action really happened, so it records it.
    pub fn record(&mut self, candidate: Candidate, now: u32) {
        if self.memory.len() == MEMORY_LIMIT {
            self.memory.pop_front();
        }
        self.memory.push_back((candidate, now));
    }

    /// Forgets every recorded action.
    pub fn forget(&mut self) {
        self.memory.clear();
    }

    /// The window of [`INPUT_COUNT_OPTION`] and [`INPUT_COUNT_PAIR`]: actions
    /// less than `time` ago count (default: all remembered).
    pub fn set_window(&mut self, time: u32) {
        self.window = time;
    }

    /// After `option` was done (any target), it scores 0 until `time` has
    /// passed: a hard cooldown, e.g. a rule "once every 3 turns". 0: none.
    /// `false` if the option does not exist.
    pub fn set_cooldown(&mut self, option: u32, time: u32) -> bool {
        self.options
            .get_mut(option as usize)
            .map(|o| o.cooldown = time)
            .is_some()
    }

    /// Time since `option` (and, with `target`, on that target) was last
    /// done at the current time, or `None` if never.
    pub fn since(&self, option: u32, target: Option<u32>) -> Option<u32> {
        self.memory
            .iter()
            .rev()
            .find(|(c, _)| c.option == option && target.is_none_or(|t| t == c.target))
            .map(|&(_, at)| self.now.wrapping_sub(at))
    }

    /// How often `option` (and, with `target`, on that target) was done
    /// within the window.
    pub fn count(&self, option: u32, target: Option<u32>) -> u32 {
        let window = if self.window == 0 {
            u32::MAX
        } else {
            self.window
        };
        self.memory
            .iter()
            .filter(|(c, at)| {
                c.option == option
                    && target.is_none_or(|t| t == c.target)
                    && self.now.wrapping_sub(*at) < window
            })
            .count() as u32
    }

    fn input_of(&self, candidate: Candidate, row: &[i32], input: u32) -> Option<i32> {
        let since = |t| {
            self.since(candidate.option, t)
                .map_or(i32::MAX, |s| i32::try_from(s).unwrap_or(i32::MAX))
        };
        let count = |t| i32::try_from(self.count(candidate.option, t)).unwrap_or(i32::MAX);
        match input {
            INPUT_SINCE_OPTION => Some(since(None)),
            INPUT_SINCE_PAIR => Some(since(Some(candidate.target))),
            INPUT_COUNT_OPTION => Some(count(None)),
            INPUT_COUNT_PAIR => Some(count(Some(candidate.target))),
            _ => row.get(input as usize).copied(),
        }
    }

    /// Number of options.
    pub fn option_count(&self) -> usize {
        self.options.len()
    }

    /// Scores one candidate from its row of inputs.
    pub fn score(&self, candidate: Candidate, row: &[i32]) -> Result<u32, DecideError> {
        self.score_traced(0, candidate, row, &mut NoopSink)
    }

    fn score_traced<S: Sink<DecideEvent>>(
        &self,
        index: usize,
        candidate: Candidate,
        row: &[i32],
        sink: &mut S,
    ) -> Result<u32, DecideError> {
        let option = self
            .options
            .get(candidate.option as usize)
            .ok_or(DecideError::UnknownOption { candidate: index })?;
        let mut values = self.values.borrow_mut();
        values.clear();
        let mut multiply = ONE;
        for (i, f) in option.factors.iter().enumerate() {
            let c = &f.consideration;
            let input =
                self.input_of(candidate, row, c.input)
                    .ok_or(DecideError::InputOutOfRange {
                        candidate: index,
                        input: c.input,
                    })?;
            let value = pull(c.curve.apply(input, c.min, c.max), f.pull);
            if sink.enabled() {
                sink.emit(DecideEvent::Factor {
                    candidate: index,
                    consideration: i,
                    input,
                    value,
                });
            }
            match f.apply {
                Apply::Combine => values.push(value),
                Apply::Multiply => multiply = mul(multiply, value),
            }
        }
        if option.cooldown > 0
            && self
                .since(candidate.option, None)
                .is_some_and(|s| s < option.cooldown)
        {
            multiply = 0;
        }
        let how = option.combine.unwrap_or(self.combine);
        let product = mul(combine(how, &mut values), multiply);
        let mut score = u64::from(product) * u64::from(option.weight) / u64::from(ONE);
        if let Some((current, bonus)) = self.current
            && current == candidate
        {
            score += score * u64::from(bonus) / u64::from(ONE);
        }
        let score = score.min(u64::from(u32::MAX)) as u32;
        if sink.enabled() {
            sink.emit(DecideEvent::Scored {
                candidate: index,
                product,
                score,
            });
        }
        Ok(score)
    }

    /// Scores every candidate and chooses one.
    ///
    /// `inputs` holds one row of `stride` raw inputs per candidate, in the
    /// order of `candidates`. `scores` receives every score (cleared first).
    /// `seed` drives [`Pick::Weighted`] and [`Pick::Uniform`]; it is ignored
    /// for [`Pick::Best`]. `Ok(None)` if every score is 0.
    pub fn decide(
        &mut self,
        candidates: &[Candidate],
        inputs: &[i32],
        stride: usize,
        pick: Pick,
        seed: u64,
        scores: &mut Vec<u32>,
    ) -> Result<Option<Choice>, DecideError> {
        self.decide_traced(
            candidates,
            inputs,
            stride,
            pick,
            seed,
            scores,
            &mut NoopSink,
        )
    }

    /// [`Decider::decide`] with events.
    #[allow(clippy::too_many_arguments)]
    pub fn decide_traced<S: Sink<DecideEvent>>(
        &mut self,
        candidates: &[Candidate],
        inputs: &[i32],
        stride: usize,
        pick: Pick,
        seed: u64,
        scores: &mut Vec<u32>,
        sink: &mut S,
    ) -> Result<Option<Choice>, DecideError> {
        if candidates
            .len()
            .checked_mul(stride)
            .is_none_or(|n| inputs.len() < n)
        {
            return Err(DecideError::InputsTooShort);
        }
        scores.clear();
        for (i, &c) in candidates.iter().enumerate() {
            let row = &inputs[i * stride..(i + 1) * stride];
            scores.push(self.score_traced(i, c, row, sink)?);
        }
        // Only the highest rank that still scores takes part.
        let rank_of = |c: &Candidate| self.options[c.option as usize].rank;
        let top_rank = candidates
            .iter()
            .zip(scores.iter())
            .filter(|&(_, &s)| s > 0)
            .map(|(c, _)| rank_of(c))
            .max();
        let mixed = top_rank.is_some_and(|r| candidates.iter().any(|c| rank_of(c) != r));
        let choice = if let (true, Some(rank)) = (mixed, top_rank) {
            self.ranked.clear();
            self.ranked.extend(
                candidates
                    .iter()
                    .zip(scores.iter())
                    .map(|(c, &s)| if rank_of(c) == rank { s } else { 0 }),
            );
            choose(&self.ranked, pick, seed, &mut self.order)
        } else {
            choose(scores, pick, seed, &mut self.order)
        };
        if let Some(c) = choice
            && sink.enabled()
        {
            sink.emit(DecideEvent::Chosen {
                candidate: c.candidate,
                score: c.score,
            });
        }
        Ok(choice)
    }
}

/// Makes up for the number of factors: without it, an option with many
/// considerations would always lose to one with few. Dave Mark's formula:
/// `p + (1 - p) * (1 - 1/n) * p`, in fixed point. 0 stays 0, 1 stays 1.
pub fn compensate(product: u32, factors: usize) -> u32 {
    if factors <= 1 || product == 0 {
        return product;
    }
    let n = factors as u64;
    let one = u64::from(ONE);
    let p = u64::from(product);
    let modification = one - one / n;
    let make_up = (one - p) * modification / one;
    (p + make_up * p / one) as u32
}

/// A well-mixed number from `seed` (SplitMix64 finaliser).
pub fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Chooses among `scores` (index = candidate). `order` is scratch space.
pub fn choose(scores: &[u32], pick: Pick, seed: u64, order: &mut Vec<usize>) -> Option<Choice> {
    let top = match pick {
        Pick::Best => {
            // max_by_key keeps the last maximum; reversing keeps the first.
            let (i, &s) = scores.iter().enumerate().rev().max_by_key(|&(_, &s)| s)?;
            return (s > 0).then_some(Choice {
                candidate: i,
                score: s,
            });
        }
        Pick::Weighted { top } | Pick::Uniform { top } => top,
    };
    order.clear();
    order.extend((0..scores.len()).filter(|&i| scores[i] > 0));
    // Stable sort: equal scores keep the smaller index first.
    order.sort_by(|&a, &b| scores[b].cmp(&scores[a]));
    if top > 0 {
        order.truncate(top as usize);
    }
    if order.is_empty() {
        return None;
    }
    let r = mix(seed);
    let i = match pick {
        Pick::Uniform { .. } => order[(r % order.len() as u64) as usize],
        _ => {
            let total: u64 = order.iter().map(|&i| u64::from(scores[i])).sum();
            let mut left = r % total;
            let mut chosen = order[order.len() - 1];
            for &i in order.iter() {
                let s = u64::from(scores[i]);
                if left < s {
                    chosen = i;
                    break;
                }
                left -= s;
            }
            chosen
        }
    };
    Some(Choice {
        candidate: i,
        score: scores[i],
    })
}

#[cfg(test)]
mod tests;
