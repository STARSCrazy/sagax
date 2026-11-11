//! Node -> record index lookup for the search.
//!
//! Open addressing with linear probing. Every slot carries the number of the
//! search that wrote it, so starting a new search empties the table without
//! touching memory (the table is wiped only when that number wraps).

use std::hash::{Hash, Hasher};

use crate::hash::FxHasher;

#[derive(Clone, Copy)]
struct Slot<N> {
    stamp: u32,
    value: u32,
    key: N,
}

pub(crate) struct NodeTable<N> {
    slots: Vec<Slot<N>>,
    /// Size to allocate on the first insert (slots need a key as filler).
    reserved: usize,
    shift: u32,
    stamp: u32,
    len: usize,
}

impl<N: Copy + Eq + Hash> NodeTable<N> {
    /// Room for `nodes` nodes without growing.
    pub(crate) fn with_capacity(nodes: usize) -> Self {
        Self {
            slots: Vec::new(),
            reserved: nodes.saturating_mul(2).next_power_of_two().max(16),
            shift: 64,
            stamp: 1,
            len: 0,
        }
    }

    /// Allocates `size` (a power of two) slots and moves the current entries.
    fn rebuild(&mut self, size: usize, filler: N) {
        let old = std::mem::replace(
            &mut self.slots,
            vec![
                Slot {
                    stamp: 0,
                    value: 0,
                    key: filler
                };
                size
            ],
        );
        let old_stamp = self.stamp;
        self.shift = 64 - size.trailing_zeros();
        self.stamp = 1;
        self.len = 0;
        for slot in old.iter().filter(|s| s.stamp == old_stamp) {
            self.insert_new(slot.key, slot.value);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            for slot in &mut self.slots {
                slot.stamp = 0;
            }
            self.stamp = 1;
        }
    }

    #[inline]
    fn home(&self, key: &N) -> usize {
        let mut h = FxHasher::default();
        key.hash(&mut h);
        (h.finish() >> self.shift) as usize
    }

    #[inline]
    pub(crate) fn get(&self, key: &N) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut i = self.home(key);
        loop {
            let slot = &self.slots[i];
            if slot.stamp != self.stamp {
                return None;
            }
            if slot.key == *key {
                return Some(slot.value);
            }
            i = (i + 1) & mask;
        }
    }

    /// Inserts a key that is not in the table.
    #[inline]
    pub(crate) fn insert_new(&mut self, key: N, value: u32) {
        if (self.len + 1) * 2 > self.slots.len() {
            self.grow(key);
        }
        let mask = self.slots.len() - 1;
        let mut i = self.home(&key);
        while self.slots[i].stamp == self.stamp {
            i = (i + 1) & mask;
        }
        self.slots[i] = Slot {
            stamp: self.stamp,
            value,
            key,
        };
        self.len += 1;
    }

    #[cold]
    fn grow(&mut self, key: N) {
        let size = if self.slots.is_empty() {
            self.reserved
        } else {
            self.slots.len() * 2
        };
        self.rebuild(size, key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_clear() {
        let mut t = NodeTable::with_capacity(0);
        for i in 0..1000u32 {
            assert_eq!(t.get(&i), None);
            t.insert_new(i, i * 2);
        }
        for i in 0..1000u32 {
            assert_eq!(t.get(&i), Some(i * 2));
        }
        t.clear();
        assert_eq!(t.get(&5), None);
        t.insert_new(5, 1);
        assert_eq!(t.get(&5), Some(1));
        assert_eq!(t.get(&6), None);
    }

    #[test]
    fn stamp_wrap_empties() {
        let mut t = NodeTable::with_capacity(4);
        t.insert_new(1u32, 1);
        t.stamp = u32::MAX;
        t.slots.iter_mut().for_each(|s| {
            if s.stamp != 0 {
                s.stamp = u32::MAX
            }
        });
        assert_eq!(t.get(&1), Some(1));
        t.clear();
        assert_eq!(t.get(&1), None);
    }
}
