use sagax_core::{NoopSink, Sink};

use super::{Pos, ZeroCapacity};

/// Source of a fact: observed by the character itself.
pub const SOURCE_OWN: u8 = 0;
/// Source of a fact: heard (a noise, a shout).
pub const SOURCE_HEARD: u8 = 1;
/// Source of a fact: told by someone else (a message).
pub const SOURCE_TOLD: u8 = 2;

/// What to drop when a full [`Knowledge`] gets a new fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Evict {
    /// The fact with the largest age of `seen_at`; a tie drops the earlier slot.
    Oldest = 0,
    /// The fact with the smallest `strength`; a tie drops the older one
    /// (largest age), then the earlier slot.
    Weakest = 1,
}

/// One known fact: something of a kind about a subject, where and when.
///
/// The key is `(subject, kind)`; the kind is the game's own number (seen
/// enemy, found body, heard noise, ...). Games may use more `source` values
/// than the three defined here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Fact {
    /// What the fact is about (an id of the game).
    pub subject: u32,
    /// What kind of fact it is (the game's own number).
    pub kind: u32,
    /// Where it was observed.
    pub pos: Pos,
    /// When it was observed (not when it was received).
    pub seen_at: u32,
    /// Free for the game (certainty, loudness, ...); used by [`Evict::Weakest`].
    pub strength: u32,
    /// Free for the game (flags, ...).
    pub data: u32,
    /// Where it comes from: [`SOURCE_OWN`], [`SOURCE_HEARD`], [`SOURCE_TOLD`]
    /// or a number of the game.
    pub source: u8,
}

/// What a [`Knowledge`] call did, for debugging: the record of the
/// `*_traced` calls. Every record carries the fact concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KnowledgeTrace {
    /// A new key was stored (after an [`Evicted`](Self::Evicted) when the
    /// table was full).
    Added(Fact),
    /// A known key was overwritten with this fact.
    Updated(Fact),
    /// [`Knowledge::merge`] kept the stored fact: this is the refused one,
    /// seen earlier than the stored one.
    Stale(Fact),
    /// Dropped by the [`Evict`] rule to make room for a new key.
    Evicted(Fact),
    /// Dropped by an `expire*` call.
    Expired(Fact),
}

/// A bounded table of [`Fact`]s in insertion order, keyed by
/// `(subject, kind)`.
///
/// When full, a new key drops one fact (see [`Evict`]); the rest keep their
/// order and the new fact goes last. Changing a known key keeps its slot.
#[derive(Clone, Debug)]
pub struct Knowledge {
    items: Vec<Fact>,
    capacity: usize,
    evict: Evict,
}

impl Knowledge {
    /// An empty table for at most `capacity` facts; the whole capacity is
    /// reserved at once.
    pub fn new(capacity: usize, evict: Evict) -> Result<Self, ZeroCapacity> {
        if capacity == 0 {
            return Err(ZeroCapacity);
        }
        Ok(Self {
            items: Vec::with_capacity(capacity),
            capacity,
            evict,
        })
    }

    fn find(&self, subject: u32, kind: u32) -> Option<usize> {
        self.items
            .iter()
            .position(|f| f.subject == subject && f.kind == kind)
    }

    /// The slot to drop at time `now` (the table is not empty).
    fn victim(&self, now: u32) -> usize {
        let mut at = 0;
        for (i, f) in self.items.iter().enumerate().skip(1) {
            let best = &self.items[at];
            let (age, best_age) = (now.wrapping_sub(f.seen_at), now.wrapping_sub(best.seen_at));
            let better = match self.evict {
                Evict::Oldest => age > best_age,
                Evict::Weakest => {
                    f.strength < best.strength || (f.strength == best.strength && age > best_age)
                }
            };
            if better {
                at = i;
            }
        }
        at
    }

    fn insert<S: Sink<KnowledgeTrace>>(
        &mut self,
        fact: Fact,
        now: u32,
        sink: &mut S,
    ) -> Option<Fact> {
        let mut evicted = None;
        if self.items.len() >= self.capacity {
            let v = self.victim(now);
            let gone = self.items.remove(v);
            if sink.enabled() {
                sink.emit(KnowledgeTrace::Evicted(gone));
            }
            evicted = Some(gone);
        }
        self.items.push(fact);
        if sink.enabled() {
            sink.emit(KnowledgeTrace::Added(fact));
        }
        evicted
    }

    /// Stores `fact`. A known key is overwritten in place (all fields); a
    /// new key is added, and if the table is full the fact chosen by the
    /// [`Evict`] rule is dropped and returned.
    pub fn learn(&mut self, now: u32, fact: Fact) -> Option<Fact> {
        self.learn_traced(now, fact, &mut NoopSink)
    }

    /// [`Knowledge::learn`] with a trace: [`KnowledgeTrace::Updated`] for a
    /// known key, else [`KnowledgeTrace::Evicted`] (when full) and
    /// [`KnowledgeTrace::Added`].
    pub fn learn_traced<S: Sink<KnowledgeTrace>>(
        &mut self,
        now: u32,
        fact: Fact,
        sink: &mut S,
    ) -> Option<Fact> {
        match self.find(fact.subject, fact.kind) {
            Some(i) => {
                self.items[i] = fact;
                if sink.enabled() {
                    sink.emit(KnowledgeTrace::Updated(fact));
                }
                None
            }
            None => self.insert(fact, now, sink),
        }
    }

    /// Like [`Knowledge::learn`], but a known key is overwritten only if the
    /// new `seen_at` is not older than the stored one (ages relative to
    /// `now`, so a wrapped clock works); otherwise the stored fact stays.
    pub fn merge(&mut self, now: u32, fact: Fact) -> Option<Fact> {
        self.merge_traced(now, fact, &mut NoopSink)
    }

    /// [`Knowledge::merge`] with a trace: as [`Knowledge::learn_traced`],
    /// and [`KnowledgeTrace::Stale`] with `fact` when the stored one stays.
    pub fn merge_traced<S: Sink<KnowledgeTrace>>(
        &mut self,
        now: u32,
        fact: Fact,
        sink: &mut S,
    ) -> Option<Fact> {
        match self.find(fact.subject, fact.kind) {
            Some(i) => {
                let newer =
                    now.wrapping_sub(fact.seen_at) <= now.wrapping_sub(self.items[i].seen_at);
                if newer {
                    self.items[i] = fact;
                }
                if sink.enabled() {
                    sink.emit(if newer {
                        KnowledgeTrace::Updated(fact)
                    } else {
                        KnowledgeTrace::Stale(fact)
                    });
                }
                None
            }
            None => self.insert(fact, now, sink),
        }
    }

    /// Renews the time of a fact (`seen_at = now`). False if unknown.
    pub fn touch(&mut self, subject: u32, kind: u32, now: u32) -> bool {
        match self.find(subject, kind) {
            Some(i) => {
                self.items[i].seen_at = now;
                true
            }
            None => false,
        }
    }

    /// Sets the place of a fact. False if unknown.
    pub fn set_pos(&mut self, subject: u32, kind: u32, pos: Pos) -> bool {
        match self.find(subject, kind) {
            Some(i) => {
                self.items[i].pos = pos;
                true
            }
            None => false,
        }
    }

    /// Sets the `data` of a fact. False if unknown.
    pub fn set_data(&mut self, subject: u32, kind: u32, data: u32) -> bool {
        match self.find(subject, kind) {
            Some(i) => {
                self.items[i].data = data;
                true
            }
            None => false,
        }
    }

    /// Sets the `strength` of a fact. False if unknown.
    pub fn set_strength(&mut self, subject: u32, kind: u32, strength: u32) -> bool {
        match self.find(subject, kind) {
            Some(i) => {
                self.items[i].strength = strength;
                true
            }
            None => false,
        }
    }

    /// Forgets one fact. False if unknown.
    pub fn remove(&mut self, subject: u32, kind: u32) -> bool {
        match self.find(subject, kind) {
            Some(i) => {
                self.items.remove(i);
                true
            }
            None => false,
        }
    }

    /// Forgets all facts about `subject` (every kind); returns how many.
    pub fn remove_subject(&mut self, subject: u32) -> usize {
        let before = self.items.len();
        self.items.retain(|f| f.subject != subject);
        before - self.items.len()
    }

    /// The fact for a key, if known.
    pub fn get(&self, subject: u32, kind: u32) -> Option<&Fact> {
        self.find(subject, kind).map(|i| &self.items[i])
    }

    /// All facts in slot order.
    pub fn iter(&self) -> impl Iterator<Item = &Fact> {
        self.items.iter()
    }

    /// Number of facts.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if nothing is known.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The most facts this table holds.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Forgets every fact whose age (`now - seen_at`) is at least `max_age`.
    /// `removed` is called with each dropped fact, in slot order.
    pub fn expire(&mut self, now: u32, max_age: u32, removed: impl FnMut(Fact)) {
        self.expire_traced(now, max_age, removed, &mut NoopSink);
    }

    /// [`Knowledge::expire`] with a trace: [`KnowledgeTrace::Expired`] for
    /// each dropped fact, in slot order.
    pub fn expire_traced<S: Sink<KnowledgeTrace>>(
        &mut self,
        now: u32,
        max_age: u32,
        removed: impl FnMut(Fact),
        sink: &mut S,
    ) {
        self.expire_where(|f| now.wrapping_sub(f.seen_at) >= max_age, removed, sink);
    }

    /// Like [`expire`](Self::expire) for facts of one `kind` only; other
    /// kinds stay. `removed` gets the facts in slot order.
    pub fn expire_kind(&mut self, now: u32, kind: u32, max_age: u32, removed: impl FnMut(Fact)) {
        self.expire_kind_traced(now, kind, max_age, removed, &mut NoopSink);
    }

    /// [`Knowledge::expire_kind`] with a trace: [`KnowledgeTrace::Expired`]
    /// for each dropped fact, in slot order.
    pub fn expire_kind_traced<S: Sink<KnowledgeTrace>>(
        &mut self,
        now: u32,
        kind: u32,
        max_age: u32,
        removed: impl FnMut(Fact),
        sink: &mut S,
    ) {
        self.expire_where(
            |f| f.kind == kind && now.wrapping_sub(f.seen_at) >= max_age,
            removed,
            sink,
        );
    }

    /// Like [`Knowledge::expire`], but `max_age` is chosen per fact (for
    /// example per kind).
    pub fn expire_with(
        &mut self,
        now: u32,
        max_age: impl Fn(&Fact) -> u32,
        removed: impl FnMut(Fact),
    ) {
        self.expire_with_traced(now, max_age, removed, &mut NoopSink);
    }

    /// [`Knowledge::expire_with`] with a trace: [`KnowledgeTrace::Expired`]
    /// for each dropped fact, in slot order.
    pub fn expire_with_traced<S: Sink<KnowledgeTrace>>(
        &mut self,
        now: u32,
        max_age: impl Fn(&Fact) -> u32,
        removed: impl FnMut(Fact),
        sink: &mut S,
    ) {
        self.expire_where(|f| now.wrapping_sub(f.seen_at) >= max_age(f), removed, sink);
    }

    /// Drops every fact `gone` says yes to, keeping the order of the rest.
    fn expire_where<S: Sink<KnowledgeTrace>>(
        &mut self,
        gone: impl Fn(&Fact) -> bool,
        mut removed: impl FnMut(Fact),
        sink: &mut S,
    ) {
        self.items.retain(|f| {
            let gone = gone(f);
            if gone {
                removed(*f);
                if sink.enabled() {
                    sink.emit(KnowledgeTrace::Expired(*f));
                }
            }
            !gone
        });
    }

    /// [`Knowledge::learn`] for the fact a message tells (`source` as given).
    pub fn learn_message(&mut self, now: u32, msg: &super::Message, source: u8) -> Option<Fact> {
        self.learn(now, msg.to_fact(source))
    }

    /// [`Knowledge::learn_message`] with a trace (see
    /// [`Knowledge::learn_traced`]).
    pub fn learn_message_traced<S: Sink<KnowledgeTrace>>(
        &mut self,
        now: u32,
        msg: &super::Message,
        source: u8,
        sink: &mut S,
    ) -> Option<Fact> {
        self.learn_traced(now, msg.to_fact(source), sink)
    }

    /// [`Knowledge::merge`] for the fact a message tells (`source` as given).
    pub fn merge_message(&mut self, now: u32, msg: &super::Message, source: u8) -> Option<Fact> {
        self.merge(now, msg.to_fact(source))
    }

    /// [`Knowledge::merge_message`] with a trace (see
    /// [`Knowledge::merge_traced`]).
    pub fn merge_message_traced<S: Sink<KnowledgeTrace>>(
        &mut self,
        now: u32,
        msg: &super::Message,
        source: u8,
        sink: &mut S,
    ) -> Option<Fact> {
        self.merge_traced(now, msg.to_fact(source), sink)
    }
}
