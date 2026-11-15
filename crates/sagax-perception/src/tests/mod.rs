//! Tests of the perception block.

mod awareness;
mod disguise;
mod hear;
mod line;
mod look;
mod world;

use crate::*;
use sagax_core::{Cell, Pos, mix};

/// Fine units per cell in the awareness tests.
const S: i32 = 256;

/// A square world with `S` fine units per cell.
fn square() -> World {
    World::square(S)
}

/// The centre of square cell `(x, y)` on level 0.
fn at(x: i32, y: i32) -> Pos {
    cell_centre(&square(), Cell::new(x, y, 0))
}

/// An observer at the centre of cell `(0, 0)`, facing +x.
fn eye(profile: &SenseProfile) -> Eye<'_> {
    Eye {
        id: 1,
        point: at(0, 0),
        facing: 0,
        profile,
        stats: Stats::NORMAL,
        disguise: DisguiseRule::default(),
    }
}

/// A walking person at `point`.
fn person(point: Pos) -> Target {
    Target {
        id: 7,
        point,
        size: 256,
        conspicuity: 256,
        ..Target::default()
    }
}

/// One `observe` on the given map; its percepts.
fn check_on(
    map: &mut impl SenseMap,
    now: u32,
    eye: &Eye,
    target: &Target,
    aware: &mut Awareness,
) -> Vec<Percept> {
    let mut out = Vec::new();
    observe(&square(), map, now, eye, target, aware, &mut out);
    out
}

/// One `observe` in open air; its percepts.
fn check(now: u32, eye: &Eye, target: &Target, aware: &mut Awareness) -> Vec<Percept> {
    check_on(&mut OpenAir, now, eye, target, aware)
}

/// Whether `out` holds a percept of `kind`.
fn has(out: &[Percept], kind: PerceptKind) -> bool {
    out.iter().any(|p| p.event == kind)
}

/// Seeded numbers from `mix`.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(1);
        mix(self.0) % n
    }

    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u64) as i32
    }
}
