//! Units, cells, headings, the cone test and the light wrapper.

use crate::*;
use sagax_core::Cell;

#[test]
fn cell_round_trip_square() {
    for s in [2, 3, 256] {
        let world = World::square(s);
        for level in [0, 3] {
            for x in -20..=20 {
                for y in -20..=20 {
                    let c = Cell::new(x, y, level);
                    assert_eq!(cell_of(&world, cell_centre(&world, c)), c, "S {s}");
                }
            }
        }
    }
    let world = World::square(256);
    assert_eq!(cell_centre(&world, Cell::new(1, 2, 0)), [384, 640, 0]);
    // Floor division: the lower border belongs to the cell, negatives too.
    assert_eq!(cell_of(&world, [256, 255, 1]), Cell::new(1, 0, 1));
    assert_eq!(cell_of(&world, [-1, -256, 0]), Cell::new(-1, -1, 0));
    assert_eq!(cell_of(&world, [-257, 0, 0]), Cell::new(-2, 0, 0));
}

#[test]
fn cell_round_trip_hex() {
    for s in [2, 3, 256] {
        let world = World::hex(s);
        for level in [0, 3] {
            for q in -20..=20 {
                for r in -20..=20 {
                    let c = Cell::new(q, r, level);
                    assert_eq!(cell_of(&world, cell_centre(&world, c)), c, "S {s}");
                }
            }
        }
    }
    let world = World::hex(256);
    assert_eq!(cell_centre(&world, Cell::new(1, 2, 0)), [256, 512, 0]);
}

#[test]
fn hex_border_points() {
    let world = World::hex(6);
    // Midpoint of the centres (0,0) and (1,0): q/S = 1/2, r = 0, s = -1/2.
    // Rounded (halves up): q 1, r 0, s 0; errors 1/2, 0, 1/2. q and s tie,
    // equal errors fix s: (1, 0).
    assert_eq!(cell_of(&world, [3, 0, 0]), Cell::new(1, 0, 0));
    // The opposite midpoint, between (0,0) and (-1,0): q -1/2 -> 0, s 1/2
    // -> 1, errors 1/2, 0, 1/2: fix s, (0, 0).
    assert_eq!(cell_of(&world, [-3, 0, 0]), Cell::new(0, 0, 0));
    // The vertex of (0,0), (1,0), (0,1) is the cube point (1/3, 1/3, -2/3):
    // rounded 0, 0, -1, all errors 1/3, fix s: (0, 0).
    assert_eq!(cell_of(&world, [2, 2, 0]), Cell::new(0, 0, 0));
    // The vertex of (0,0), (1,0), (1,-1) is (2/3, -1/3, -1/3): rounded
    // 1, 0, 0, all errors 1/3, fix s: (1, 0).
    assert_eq!(cell_of(&world, [4, -2, 0]), Cell::new(1, 0, 0));
    // Just inside a cell: plain nearest centre.
    assert_eq!(cell_of(&world, [2, 0, 0]), Cell::new(0, 0, 0));
    assert_eq!(cell_of(&world, [4, 0, 0]), Cell::new(1, 0, 0));
}

#[test]
fn far_coordinates() {
    let far = 1 << 22;
    for world in [World::square(256), World::hex(256)] {
        for (x, y) in [
            (far - 1, far - 1),
            (-far, -far),
            (far - 1, -far),
            (-far, far - 1),
        ] {
            let c = Cell::new(x, y, 2);
            assert_eq!(cell_of(&world, cell_centre(&world, c)), c, "{world:?}");
        }
    }
}

#[test]
fn hex_heading_values() {
    let all: Vec<u8> = (0..6).map(hex_heading).collect();
    assert_eq!(all, [0, 43, 85, 128, 171, 213]);
    assert_eq!(hex_heading(6), 0);
}

#[test]
fn cone_bands() {
    assert!(in_band(0, (10, 0), 43));
    // atan(17/10) is about 59.5 degrees, half-angle 43 about 60.5.
    assert!(in_band(0, (10, 17), 43));
    assert!(!in_band(0, (10, 18), 43));
    // Left and right alike.
    assert!(in_band(0, (10, -17), 43));
    assert!(!in_band(0, (10, -18), 43));
    // Half a turn on each side holds everything, behind too.
    assert!(in_band(0, (-10, 0), 128));
    assert!(in_band(0, (-10, 1), 128));
    // Half-angle 0: only straight ahead.
    assert!(in_band(0, (5, 0), 0));
    assert!(!in_band(0, (5, 1), 0));
    assert!(!in_band(0, (-5, 0), 0));
    // The observer's own place is in every band.
    assert!(in_band(0, (0, 0), 0));
    assert!(in_band(100, (0, 0), 43));
    // Facing +y (64) and a wide cone that is wider than 90 degrees.
    assert!(in_band(64, (0, 10), 0));
    assert!(in_band(64, (-10, -1), 80));
    assert!(!in_band(64, (-10, -10), 80));
    for h in [0u8, 10, 43, 64, 100, 127] {
        for v in [(7, 3), (3, 7), (-2, 9), (-9, 1), (1, 0)] {
            let mirror = (v.0, -v.1);
            assert_eq!(in_band(0, v, h), in_band(0, mirror, h), "h {h} {v:?}");
        }
    }
}

#[test]
fn view_vectors() {
    let square = World::square(256);
    assert_eq!(view_vector(&square, [0, 0, 0], [3, -4, 0]), (3, -4));
    let hex = World::hex(256);
    // +q points along heading 0, +r along heading 43 (60 degrees).
    let q = view_vector(&hex, [0, 0, 0], [256, 0, 0]);
    let r = view_vector(&hex, [0, 0, 0], [0, 256, 0]);
    assert!(in_band(0, q, 0));
    assert!(in_band(hex_heading(1), r, 1));
    assert!(!in_band(0, r, 42));
    assert!(in_band(0, r, 43));
}

#[test]
fn lit_wrapper() {
    let c = Cell::new(1, 2, 0);
    let mut lit = Lit {
        inner: OpenAir,
        ambient: 40,
    };
    assert_eq!(lit.light(c), 40);
    assert_eq!(lit.sight(c), Medium::default());
    assert!(lit.sight_edge(c, Cell::new(2, 2, 0)));
    assert_eq!(lit.cross_level([0, 0, 0], [0, 0, 1]), None);
    assert_eq!(lit.sound(c), 0);
    assert_eq!(lit.sound_edge(c, Cell::new(2, 2, 0)), Some(0));
    assert_eq!(OpenAir.light(c), 255);
}

#[test]
fn channel_defaults() {
    let world = World::square(256);
    assert_eq!(world.grid, Grid::Square);
    assert_eq!(world.cell_size, 256);
    assert_eq!(world.channels.len(), MAX_CHANNELS);
    let c = Channel::default();
    assert_eq!((c.damping, c.fuzz, c.speed), (256, 0, 0));
    assert_eq!(c.spread, Spread::Circle);
    assert!(world.channels.iter().all(|&ch| ch == c));
    assert_eq!(World::hex(3).grid, Grid::Hex);
}
