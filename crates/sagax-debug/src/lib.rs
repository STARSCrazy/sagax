//! Bounded collectors for diagnostic events.
//!
//! Algorithms report through [`sagax_core::Sink`]. This crate keeps those events
//! for later inspection without ever growing beyond a fixed capacity.

use std::collections::VecDeque;

use sagax_core::Sink;

/// What a [`Collector`] does when it is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// Drop the oldest event to make room (keeps the end of a run).
    DropOldest,
    /// Drop the new event (keeps the start of a run).
    DropNewest,
}

/// One collected event with its sequence number (0 for the first event ever
/// offered to this collector, counting dropped ones too).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record<E> {
    /// Position in the order the events were emitted.
    pub sequence: u64,
    /// The event.
    pub event: E,
}

/// Stores at most `capacity` events; counts what it had to drop.
#[derive(Clone, Debug)]
pub struct Collector<E> {
    records: VecDeque<Record<E>>,
    capacity: usize,
    overflow: Overflow,
    next_sequence: u64,
    dropped: u64,
}

impl<E> Collector<E> {
    /// A collector for at most `capacity` events. Memory for all of them is
    /// reserved up front.
    pub fn new(capacity: usize, overflow: Overflow) -> Self {
        Self {
            records: VecDeque::with_capacity(capacity),
            capacity,
            overflow,
            next_sequence: 0,
            dropped: 0,
        }
    }

    /// The kept events, oldest first.
    pub fn records(&self) -> impl ExactSizeIterator<Item = &Record<E>> {
        self.records.iter()
    }

    /// How many events were offered and not kept.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// How many events were offered in total.
    pub fn offered(&self) -> u64 {
        self.next_sequence
    }

    /// Forgets all events and counters; keeps the reserved memory.
    pub fn clear(&mut self) {
        self.records.clear();
        self.next_sequence = 0;
        self.dropped = 0;
    }
}

impl<E> Sink<E> for Collector<E> {
    fn enabled(&self) -> bool {
        self.capacity > 0
    }

    fn emit(&mut self, event: E) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        if self.records.len() >= self.capacity {
            self.dropped += 1;
            match self.overflow {
                Overflow::DropNewest => return,
                Overflow::DropOldest => {
                    if self.records.pop_front().is_none() {
                        return; // capacity 0
                    }
                }
            }
        }
        self.records.push_back(Record { sequence, event });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(overflow: Overflow) -> Collector<u32> {
        let mut c = Collector::new(3, overflow);
        for i in 0..5 {
            c.emit(i);
        }
        c
    }

    #[test]
    fn drop_oldest_keeps_the_end() {
        let c = fill(Overflow::DropOldest);
        let kept: Vec<_> = c.records().map(|r| (r.sequence, r.event)).collect();
        assert_eq!(kept, [(2, 2), (3, 3), (4, 4)]);
        assert_eq!(c.dropped(), 2);
        assert_eq!(c.offered(), 5);
    }

    #[test]
    fn drop_newest_keeps_the_start() {
        let c = fill(Overflow::DropNewest);
        let kept: Vec<_> = c.records().map(|r| r.event).collect();
        assert_eq!(kept, [0, 1, 2]);
        assert_eq!(c.dropped(), 2);
    }

    #[test]
    fn never_grows_beyond_capacity() {
        let mut c = Collector::new(4, Overflow::DropOldest);
        let reserved = c.records.capacity();
        for i in 0..10_000u32 {
            c.emit(i);
        }
        assert_eq!(c.records().len(), 4);
        assert_eq!(c.records.capacity(), reserved);
    }

    #[test]
    fn zero_capacity_is_disabled() {
        let mut c: Collector<u32> = Collector::new(0, Overflow::DropOldest);
        assert!(!c.enabled());
        c.emit(1);
        assert_eq!(c.records().len(), 0);
        assert_eq!(c.dropped(), 1);
    }
}
