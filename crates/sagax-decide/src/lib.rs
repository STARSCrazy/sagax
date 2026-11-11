//! Decisions for game AI: score every pair of option and target, then choose.
//!
//! This is "utility AI". A [`Decider`] holds the options a character has
//! (attack, flee, heal, ...). Each option has a weight and a list of
//! [`Consideration`]s. A consideration reads one raw number the game measured
//! for a candidate (distance, own health, enemies near, ...) and turns it
//! into a factor between 0 and 1 through a [`Curve`]. The factors are
//! multiplied, so a single 0 rules the candidate out.
//!
//! A [`Candidate`] is one pair of option and target. The game fills one row
//! of raw inputs per candidate; [`Decider::decide`] scores all rows and
//! chooses one by [`Pick`].
//!
//! Everything is integer arithmetic: 0..1 is 0..[`ONE`] (65535). The same
//! inputs, options and seed give the same choice on every machine and in
//! every interface (Rust, C, JavaScript). There is no global random number
//! generator; the seed comes from the caller. On equal scores the candidate
//! with the smaller index wins.
//!
//! What happened can be followed factor by factor through a
//! [`sagax_core::Sink`] of [`DecideEvent`]s.

use sagax_core::{NoopSink, Sink};

/// 1.0 in the fixed point of factors and weights.
pub const ONE: u32 = 65535;

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
        /// The factor the curve made of it.
        value: u32,
    },
    /// The final score of one candidate.
    Scored {
        /// Index of the candidate.
        candidate: usize,
        /// Product of the factors with compensation, before the weight.
        product: u32,
        /// Score after the weight.
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

#[derive(Clone, Debug)]
struct OptionDef {
    weight: u32,
    considerations: Vec<Consideration>,
}

/// Options with their considerations; scores candidates and chooses one.
#[derive(Clone, Debug, Default)]
pub struct Decider {
    options: Vec<OptionDef>,
    order: Vec<usize>,
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
            considerations: Vec::new(),
        });
        (self.options.len() - 1) as u32
    }

    /// Adds a consideration to `option`. `false` if the option does not exist.
    pub fn add_consideration(&mut self, option: u32, consideration: Consideration) -> bool {
        match self.options.get_mut(option as usize) {
            Some(o) => {
                o.considerations.push(consideration);
                true
            }
            None => false,
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
        let mut product = ONE;
        for (i, c) in option.considerations.iter().enumerate() {
            let input = *row
                .get(c.input as usize)
                .ok_or(DecideError::InputOutOfRange {
                    candidate: index,
                    input: c.input,
                })?;
            let value = c.curve.apply(input, c.min, c.max);
            if sink.enabled() {
                sink.emit(DecideEvent::Factor {
                    candidate: index,
                    consideration: i,
                    input,
                    value,
                });
            }
            product = mul(product, value);
        }
        let product = compensate(product, option.considerations.len());
        let score = (u64::from(product) * u64::from(option.weight) / u64::from(ONE))
            .min(u64::from(u32::MAX)) as u32;
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
        let choice = choose(scores, pick, seed, &mut self.order);
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
