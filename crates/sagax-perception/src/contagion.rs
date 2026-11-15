//! Indirect coordination: a character joins in on what it saw its allies
//! do ([`Contagion`]), read as Decider inputs ([`DeedInputs`]).
//!
//! The memory reads only the character's own [`Percept`]s, so every gate
//! of the perception block (cone, light, walls, attention, recognition,
//! disguise) applies unchanged: an ally it could not see never counts.

use sagax_core::{NoopSink, Pos, Sink};

use crate::percept::{Percept, Sense};
use crate::world::{Grid, fit, grid_distance};

/// Most deeds one [`ContagionConfig`] holds.
pub const MAX_DEEDS: usize = 8;

/// Number of Decider columns [`Contagion::write_columns`] writes.
pub const DEED_COLUMNS: usize = 5;

/// One deed: the cue bits that show it and how long a sighting counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct DeedRule {
    /// The cue bits (any of them shows the deed); 0: an unused deed.
    pub bits: u32,
    /// Ticks a sighting counts: an ally seen at `t` counts up to and
    /// including `t + window`.
    pub window: u32,
}

/// What one character's [`Contagion`] watches for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ContagionConfig {
    /// The grid of the fine points (for the distances: Euclidean on
    /// square, the hex distance on hex; levels are ignored).
    pub grid: Grid,
    /// The deeds; only the first `count` are used.
    pub deeds: [DeedRule; MAX_DEEDS],
    /// How many deeds are in use (at most [`MAX_DEEDS`]).
    pub count: u8,
}

impl ContagionConfig {
    /// No deeds yet, on `grid`.
    pub fn new(grid: Grid) -> Self {
        Self {
            grid,
            ..Self::default()
        }
    }

    /// Adds `rule` as the next deed (its index is the old `count`);
    /// unchanged when all [`MAX_DEEDS`] are in use.
    pub fn with_deed(mut self, rule: DeedRule) -> Self {
        let n = usize::from(self.count);
        if n < MAX_DEEDS {
            self.deeds[n] = rule;
            self.count += 1;
        }
        self
    }

    /// Deed `deed` if it is in use (within `count`, `bits` not 0).
    fn rule(&self, deed: usize) -> Option<DeedRule> {
        let n = usize::from(self.count).min(MAX_DEEDS);
        self.deeds[..n].get(deed).copied().filter(|r| r.bits != 0)
    }

    /// The deeds in use, with their index.
    fn rules(&self) -> impl Iterator<Item = (usize, DeedRule)> + '_ {
        let n = usize::from(self.count).min(MAX_DEEDS);
        self.deeds[..n]
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, r)| r.bits != 0)
    }
}

/// The Decider inputs of one deed, as [`Contagion::inputs`] gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct DeedInputs {
    /// Distinct allies seen showing the deed within its window.
    pub doing: u32,
    /// Distinct allies seen at all within the window.
    pub seen: u32,
    /// `doing * 256 / seen` (0 when `seen` is 0).
    pub share: u32,
    /// Fine distance from the observer to the nearest ally doing it
    /// (`i32::MAX` when none).
    pub nearest: i32,
    /// Ticks since the most recent onset among those allies (`u32::MAX`
    /// when none).
    pub since: u32,
}

impl DeedInputs {
    /// Nobody seen: `0, 0, 0, i32::MAX, u32::MAX`.
    pub const NONE: DeedInputs = DeedInputs {
        doing: 0,
        seen: 0,
        share: 0,
        nearest: i32::MAX,
        since: u32::MAX,
    };

    /// The [`DEED_COLUMNS`] Decider columns: `doing, seen, share, nearest,
    /// since`, each clamped to `i32::MAX`.
    pub fn columns(&self) -> [i32; DEED_COLUMNS] {
        let c = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        [
            c(self.doing),
            c(self.seen),
            c(self.share),
            self.nearest,
            c(self.since),
        ]
    }
}

/// One record of a traced contagion call. `ally` is the subject id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContagionTrace {
    /// An ally was seen starting a deed (or showing it again after it had
    /// stopped or run out of its window).
    Onset {
        /// The ally.
        ally: u32,
        /// The deed index.
        deed: u8,
    },
    /// A recognised sighting of an ally changed its entry (time, bits or
    /// place); the several percepts of one check give one record.
    Refresh {
        /// The ally.
        ally: u32,
        /// The cue bits seen.
        bits: u32,
    },
    /// A deed of an ally ran out of its window without a new sighting.
    Expired {
        /// The ally.
        ally: u32,
        /// The deed index.
        deed: u8,
    },
    /// The memory was full: the ally seen longest ago made room.
    Replaced {
        /// The ally that went.
        old: u32,
        /// The ally that came.
        ally: u32,
    },
    /// A recognised subject is no longer taken for an ally (`is_ally`
    /// answered `false`, for example a disguise seen through): its entry
    /// is gone.
    Dropped {
        /// The former ally.
        ally: u32,
    },
    /// The inputs of one deed were read.
    Inputs {
        /// The deed index.
        deed: u8,
        /// What was read.
        inputs: DeedInputs,
    },
}

/// One remembered ally.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Slot {
    used: bool,
    ally: u32,
    /// The latest recognised sighting.
    last_seen: u32,
    /// Where it was then, fine point.
    point: Pos,
    /// The cue bits it showed then.
    bits: u32,
    /// Deeds shown then whose `Expired` record is still due.
    live: u8,
    /// When each deed it shows started.
    onset: [u32; MAX_DEEDS],
}

/// Which percepts are recognised sightings that may carry deeds: sight or
/// thermal, stage recognised, with a place.
fn is_sighting(p: &Percept) -> bool {
    matches!(p.sense, Sense::Sight | Sense::Thermal) && p.stage == 2 && p.placed
}

/// One character's memory of the deeds it saw its allies do: for each
/// ally the cue bits of the latest recognised sighting, when and where it
/// was, and when each deed started.
///
/// Fed only from this character's own percepts ([`learn`](Self::learn)),
/// so it never knows more than the character saw. A fixed number of
/// slots, allocated in [`new`](Self::new); nothing allocates afterwards.
/// Integer, deterministic, square and hex alike.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Contagion {
    config: ContagionConfig,
    slots: Box<[Slot]>,
}

impl Contagion {
    /// A memory for `capacity` allies (0 remembers nothing).
    pub fn new(config: ContagionConfig, capacity: usize) -> Self {
        Self {
            config,
            slots: vec![Slot::default(); capacity].into_boxed_slice(),
        }
    }

    /// The config.
    pub fn config(&self) -> &ContagionConfig {
        &self.config
    }

    /// How many allies it can remember.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// How many allies it remembers now (entries beyond every window are
    /// freed at the next [`learn`](Self::learn)).
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.used).count()
    }

    /// Whether it remembers nobody.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// [`learn_traced`](Self::learn_traced) with [`NoopSink`].
    pub fn learn(
        &mut self,
        percepts: &[Percept],
        now: u32,
        is_ally: impl FnMut(u32, bool) -> bool,
    ) {
        self.learn_traced(percepts, now, is_ally, &mut NoopSink);
    }

    /// Learns this character's percepts of tick `now` (the sighting time
    /// is `now`). First every deed past its window gives an `Expired`
    /// record and an entry past every window frees its slot. Then, per
    /// percept in order, only recognised sightings count: sense
    /// [`Sight`](Sense::Sight) or [`Thermal`](Sense::Thermal), stage 2 and
    /// `placed` (so `Noticed` at stage 1, scanner contacts, `Heard`,
    /// `Lost` and `Forgotten` change nothing; what was seen a moment ago
    /// still counts until its window ends).
    ///
    /// `is_ally(subject, disguised)` is the game's belief, given the
    /// percept's `disguised` flag: a disguised enemy may be taken for an
    /// ally, which is what the character saw. A sighting the game does not
    /// take for an ally removes that subject's entry (`Dropped`). An ally
    /// not yet remembered takes a free slot, or the slot of the ally seen
    /// longest ago (ties: the lower slot; `Replaced`). The entry then
    /// keeps the percept's cue bits, its point and `now`; a deed shown now
    /// that the entry did not show (or that had run out) is an `Onset`.
    ///
    /// Pass only this character's percepts.
    pub fn learn_traced(
        &mut self,
        percepts: &[Percept],
        now: u32,
        mut is_ally: impl FnMut(u32, bool) -> bool,
        sink: &mut impl Sink<ContagionTrace>,
    ) {
        self.expire(now, sink);
        for p in percepts.iter().filter(|p| is_sighting(p)) {
            let found = self.find(p.subject);
            if !is_ally(p.subject, p.disguised) {
                if let Some(i) = found {
                    self.slots[i].used = false;
                    sink.emit(ContagionTrace::Dropped { ally: p.subject });
                }
                continue;
            }
            let (i, fresh) = match found {
                Some(i) => (i, false),
                None => match self.take(now, p.subject, sink) {
                    Some(i) => (i, true),
                    None => continue,
                },
            };
            self.see(i, fresh, p, now, sink);
        }
    }

    /// The Decider inputs of deed `deed` for an observer at `observer`
    /// (fine point) at `now`; [`DeedInputs::NONE`] for a deed not in use.
    /// Reads filter by the window, so they are right whether or not
    /// [`learn`](Self::learn) ran this tick.
    pub fn inputs(&self, deed: usize, observer: Pos, now: u32) -> DeedInputs {
        self.scan(deed, observer, now).0
    }

    /// [`inputs`](Self::inputs) with an `Inputs` record.
    pub fn inputs_traced(
        &self,
        deed: usize,
        observer: Pos,
        now: u32,
        sink: &mut impl Sink<ContagionTrace>,
    ) -> DeedInputs {
        let inputs = self.inputs(deed, observer, now);
        sink.emit(ContagionTrace::Inputs {
            deed: u8::try_from(deed).unwrap_or(u8::MAX),
            inputs,
        });
        inputs
    }

    /// Writes [`inputs`](Self::inputs) as the [`DEED_COLUMNS`] columns
    /// `doing, seen, share, nearest, since` ([`DeedInputs::columns`]) to
    /// the start of `row` (pass `&mut row[first..]`); a shorter `row` gets
    /// only the columns that fit (a debug build asserts the room).
    pub fn write_columns(&self, deed: usize, observer: Pos, now: u32, row: &mut [i32]) {
        self.write_columns_traced(deed, observer, now, row, &mut NoopSink);
    }

    /// [`write_columns`](Self::write_columns) with an `Inputs` record.
    pub fn write_columns_traced(
        &self,
        deed: usize,
        observer: Pos,
        now: u32,
        row: &mut [i32],
        sink: &mut impl Sink<ContagionTrace>,
    ) {
        debug_assert!(
            row.len() >= DEED_COLUMNS,
            "write_columns: the row needs {DEED_COLUMNS} columns"
        );
        let cols = self.inputs_traced(deed, observer, now, sink).columns();
        let n = row.len().min(DEED_COLUMNS);
        row[..n].copy_from_slice(&cols[..n]);
    }

    /// Where the nearest ally doing deed `deed` was last seen (fine
    /// point; ties: the lower slot), `None` when none.
    pub fn nearest_point(&self, deed: usize, observer: Pos, now: u32) -> Option<Pos> {
        self.scan(deed, observer, now)
            .1
            .map(|i| self.slots[i].point)
    }

    /// Forgets `ally` (nothing if it is not remembered).
    pub fn forget(&mut self, ally: u32) {
        if let Some(i) = self.find(ally) {
            self.slots[i].used = false;
        }
    }

    /// Forgets everyone.
    pub fn clear(&mut self) {
        self.slots.fill(Slot::default());
    }

    /// The slot of `ally`.
    fn find(&self, ally: u32) -> Option<usize> {
        self.slots.iter().position(|s| s.used && s.ally == ally)
    }

    /// The longest window in use.
    fn longest(&self) -> u32 {
        self.config
            .rules()
            .map(|(_, r)| r.window)
            .max()
            .unwrap_or(0)
    }

    /// `Expired` records for deeds past their window; frees entries past
    /// every window.
    fn expire(&mut self, now: u32, sink: &mut impl Sink<ContagionTrace>) {
        let longest = self.longest();
        let config = self.config;
        for s in self.slots.iter_mut().filter(|s| s.used) {
            let age = now.wrapping_sub(s.last_seen);
            for (d, r) in config.rules() {
                let bit = 1u8 << d;
                if s.live & bit != 0 && age > r.window {
                    s.live &= !bit;
                    sink.emit(ContagionTrace::Expired {
                        ally: s.ally,
                        deed: d as u8,
                    });
                }
            }
            if age > longest {
                s.used = false;
            }
        }
    }

    /// A slot for the new `ally`: a free one, else the one seen longest
    /// ago (ties: the lower slot). `None` without slots.
    fn take(&mut self, now: u32, ally: u32, sink: &mut impl Sink<ContagionTrace>) -> Option<usize> {
        let i = match self.slots.iter().position(|s| !s.used) {
            Some(i) => i,
            None => {
                let mut best: Option<(u32, usize)> = None;
                for (i, s) in self.slots.iter().enumerate() {
                    let age = now.wrapping_sub(s.last_seen);
                    if best.is_none_or(|(b, _)| age > b) {
                        best = Some((age, i));
                    }
                }
                let (_, i) = best?;
                sink.emit(ContagionTrace::Replaced {
                    old: self.slots[i].ally,
                    ally,
                });
                i
            }
        };
        self.slots[i] = Slot {
            used: true,
            ally,
            ..Slot::default()
        };
        Some(i)
    }

    /// A recognised sighting `p` of the ally in slot `i` (just taken if
    /// `fresh`).
    fn see(
        &mut self,
        i: usize,
        fresh: bool,
        p: &Percept,
        now: u32,
        sink: &mut impl Sink<ContagionTrace>,
    ) {
        let config = self.config;
        let s = &mut self.slots[i];
        let mut live = 0;
        for (d, r) in config.rules() {
            if p.cues & r.bits == 0 {
                continue;
            }
            let bit = 1u8 << d;
            live |= bit;
            if s.live & bit == 0 {
                s.onset[d] = now;
                sink.emit(ContagionTrace::Onset {
                    ally: s.ally,
                    deed: d as u8,
                });
            }
        }
        let changed = fresh || s.last_seen != now || s.bits != p.cues || s.point != p.point;
        s.last_seen = now;
        s.bits = p.cues;
        s.point = p.point;
        s.live = live;
        if changed {
            sink.emit(ContagionTrace::Refresh {
                ally: s.ally,
                bits: p.cues,
            });
        }
    }

    /// The inputs of `deed` and the slot of the nearest ally doing it.
    fn scan(&self, deed: usize, observer: Pos, now: u32) -> (DeedInputs, Option<usize>) {
        let Some(r) = self.config.rule(deed) else {
            return (DeedInputs::NONE, None);
        };
        let mut out = DeedInputs::NONE;
        let mut best: Option<(i64, usize)> = None;
        for (i, s) in self.slots.iter().enumerate() {
            if !s.used || now.wrapping_sub(s.last_seen) > r.window {
                continue;
            }
            out.seen += 1;
            if s.bits & r.bits == 0 {
                continue;
            }
            out.doing += 1;
            let d = grid_distance(self.config.grid, observer, s.point);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
            out.since = out.since.min(now.wrapping_sub(s.onset[deed]));
        }
        out.share = (out.doing * 256).checked_div(out.seen).unwrap_or(0);
        if let Some((d, _)) = best {
            out.nearest = fit(d);
        }
        (out, best.map(|(_, i)| i))
    }
}
