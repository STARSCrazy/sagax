//! Exact sight lines: symmetry, a brute-force reference, corners, end
//! cells, depth by length, levels, and the sound walk.

use crate::line::{sight_line_with, sound_cost};
use crate::*;
use sagax_core::{Cell, Pos, mix};

/// Seeded numbers from `mix`.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(1);
        mix(self.0) % n
    }
}

/// A `size` x `size` map of cells `0..size` on both axes (square columns
/// and rows, or axial q and r); everything outside is opaque. Edges can be
/// closed in pairs (the same both ways).
struct TestMap {
    size: i32,
    cells: Vec<Medium>,
    closed: Vec<(Cell, Cell)>,
    edge_seed: Option<u64>,
}

impl TestMap {
    fn open(size: i32) -> Self {
        Self {
            size,
            cells: vec![Medium::default(); (size * size) as usize],
            closed: Vec::new(),
            edge_seed: None,
        }
    }

    /// 30 % opaque, densities `0..=max_density`.
    fn random(size: i32, rng: &mut Rng, max_density: u64, opaque_percent: u64) -> Self {
        let mut map = Self::open(size);
        for m in &mut map.cells {
            m.opaque = rng.below(100) < opaque_percent;
            m.density = rng.below(max_density + 1) as u16;
            m.heat_density = rng.below(max_density + 1) as u16;
        }
        map
    }

    fn at(&mut self, x: i32, y: i32) -> &mut Medium {
        &mut self.cells[(y * self.size + x) as usize]
    }

    fn close(&mut self, a: Cell, b: Cell) {
        self.closed.push((a, b));
    }
}

impl SenseMap for TestMap {
    fn sight(&mut self, c: Cell) -> Medium {
        if c.x < 0 || c.y < 0 || c.x >= self.size || c.y >= self.size {
            return Medium {
                opaque: true,
                ..Medium::default()
            };
        }
        self.cells[(c.y * self.size + c.x) as usize]
    }

    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool {
        if self.closed.contains(&(from, to)) || self.closed.contains(&(to, from)) {
            return false;
        }
        match self.edge_seed {
            // A symmetric pseudo-random 10 % of all edges are closed.
            Some(seed) => {
                let (a, b) = if from < to { (from, to) } else { (to, from) };
                let h = mix(seed ^ mix((a.x as u64) << 40 ^ (a.y as u64) << 20 ^ b.x as u64))
                    ^ mix(b.y as u64);
                !mix(h).is_multiple_of(10)
            }
            None => true,
        }
    }

    fn sound(&mut self, c: Cell) -> u32 {
        u32::from(self.sight(c).density)
    }

    fn sound_edge(&mut self, from: Cell, to: Cell) -> Option<u32> {
        self.sight_edge(from, to).then_some(100)
    }
}

fn point(rng: &mut Rng, max: i32, step: i32) -> Pos {
    let n = (max / step + 1) as u64;
    [rng.below(n) as i32 * step, rng.below(n) as i32 * step, 0]
}

/// Runs both directions over `maps` random maps with `per_map` segments
/// each and asserts they agree in `clear` and `clarity`. Returns how many
/// lines were clear and how many not.
fn check_symmetry(
    world: &World,
    maps: u64,
    per_map: usize,
    max: i32,
    step: i32,
    make: impl Fn(&mut Rng) -> TestMap,
    seed: u64,
) -> (usize, usize) {
    let mut rng = Rng(seed);
    let (mut clear, mut blocked) = (0, 0);
    for _ in 0..maps {
        let mut map = make(&mut rng);
        for _ in 0..per_map {
            let a = point(&mut rng, max, step);
            let b = point(&mut rng, max, step);
            let there = sight_line(world, &mut map, a, b);
            let back = sight_line(world, &mut map, b, a);
            assert_eq!(
                (there.clear, there.clarity),
                (back.clear, back.clarity),
                "{a:?} -> {b:?}"
            );
            let thermal = sight_line_with(world, &mut map, a, b, true);
            let thermal_back = sight_line_with(world, &mut map, b, a, true);
            assert_eq!(
                (thermal.clear, thermal.clarity),
                (thermal_back.clear, thermal_back.clarity)
            );
            if there.clear {
                clear += 1;
            } else {
                blocked += 1;
            }
        }
    }
    (clear, blocked)
}

#[test]
fn line_symmetric_square() {
    let world = World::square(4);
    // Any fine points; then points on the half grid (corners, borders).
    let plain = |rng: &mut Rng| TestMap::random(12, rng, 100, 30);
    let (c, b) = check_symmetry(&world, 200, 5, 47, 1, plain, 1);
    assert!(c > 100 && b > 100, "{c} clear, {b} blocked");
    check_symmetry(&world, 200, 5, 46, 2, plain, 2);
    // Closed edges, the same both ways.
    let edged = |rng: &mut Rng| {
        let mut map = TestMap::random(12, rng, 100, 10);
        map.edge_seed = Some(rng.below(1 << 30));
        map
    };
    check_symmetry(&world, 200, 5, 46, 2, edged, 3);
    // Thick fog and no walls: the walk stops at clarity 0 on both sides.
    let foggy = |rng: &mut Rng| TestMap::random(12, rng, 600, 0);
    let (c, b) = check_symmetry(&world, 100, 5, 47, 1, foggy, 4);
    assert!(c > 50 && b > 50, "{c} clear, {b} fogged out");
}

#[test]
fn line_symmetric_hex() {
    let world = World::hex(3);
    let plain = |rng: &mut Rng| TestMap::random(12, rng, 100, 30);
    let (c, b) = check_symmetry(&world, 200, 5, 33, 1, plain, 11);
    assert!(c > 100 && b > 100, "{c} clear, {b} blocked");
    // S 6 on even points: centres, border midpoints and vertices
    // (multiples of S/3) come up often.
    let world6 = World::hex(6);
    check_symmetry(&world6, 200, 5, 66, 2, plain, 12);
    let edged = |rng: &mut Rng| {
        let mut map = TestMap::random(12, rng, 100, 10);
        map.edge_seed = Some(rng.below(1 << 30));
        map
    };
    check_symmetry(&world6, 200, 5, 66, 2, edged, 13);
    let foggy = |rng: &mut Rng| TestMap::random(12, rng, 600, 0);
    let (c, b) = check_symmetry(&world, 100, 5, 33, 1, foggy, 14);
    assert!(c > 50 && b > 50, "{c} clear, {b} fogged out");
}

/// The reference: samples the open segment at `4096 * S` exact rational
/// points `a + (b - a) * k / N` (as the point `N * a + (b - a) * k` in a
/// world with cell size `N * S`) and calls the line blocked when a sampled
/// cell other than the eye's and the target's is opaque.
fn reference_clear(world: &World, map: &mut TestMap, a: Pos, b: Pos) -> bool {
    let n = 4096 * world.cell_size;
    let big = World {
        cell_size: world.cell_size * n,
        ..*world
    };
    let (eye, target) = (cell_of(world, a), cell_of(world, b));
    (1..n).all(|k| {
        let p = [
            a[0] * n + (b[0] - a[0]) * k,
            a[1] * n + (b[1] - a[1]) * k,
            a[2],
        ];
        let c = cell_of(&big, p);
        c == eye || c == target || !map.sight(c).opaque
    })
}

/// Whether the closed segment `a`..`b` holds the point `p / 3` (all three
/// scaled by 3 to keep hex vertices whole).
fn on_segment3(a: Pos, b: Pos, p: (i64, i64)) -> bool {
    let (ax, ay) = (3 * i64::from(a[0]), 3 * i64::from(a[1]));
    let (ex, ey) = (i64::from(b[0] - a[0]), i64::from(b[1] - a[1]));
    let (px, py) = (p.0 - ax, p.1 - ay);
    let dot = px * ex + py * ey;
    px * ey - py * ex == 0 && dot >= 0 && dot <= 3 * (ex * ex + ey * ey)
}

/// Segments through an exact corner (square) or vertex (hex), or running
/// along a hex border direction, which the reference cannot judge.
fn degenerate(world: &World, a: Pos, b: Pos, size: i32) -> bool {
    let s = i64::from(world.cell_size);
    match world.grid {
        Grid::Square => (0..=size).any(|x| {
            (0..=size).any(|y| on_segment3(a, b, (3 * s * i64::from(x), 3 * s * i64::from(y))))
        }),
        Grid::Hex => {
            let (eq, er) = (i64::from(b[0] - a[0]), i64::from(b[1] - a[1]));
            let es = -eq - er;
            if eq == er || er == es || es == eq {
                return true;
            }
            const THIRDS: [(i64, i64); 6] = [(2, -1), (1, 1), (-1, 2), (-2, 1), (-1, -1), (1, -2)];
            (-1..=size).any(|q| {
                (-1..=size).any(|r| {
                    THIRDS.iter().any(|&(dq, dr)| {
                        let v = (s * (3 * i64::from(q) + dq), s * (3 * i64::from(r) + dr));
                        on_segment3(a, b, v)
                    })
                })
            })
        }
    }
}

fn check_reference(world: &World, max: i32, seed: u64) {
    let mut rng = Rng(seed);
    let (mut agree_clear, mut agree_blocked) = (0, 0);
    for _ in 0..200 {
        let mut map = TestMap::random(12, &mut rng, 100, 30);
        for _ in 0..4 {
            let a = point(&mut rng, max, 1);
            let b = point(&mut rng, max, 1);
            if degenerate(world, a, b, 12) {
                continue;
            }
            let line = sight_line(world, &mut map, a, b);
            assert_eq!(
                line.clear,
                reference_clear(world, &mut map, a, b),
                "{a:?} -> {b:?}"
            );
            // Densities stay below 2304 / 17 per cell: fog alone never
            // closes a line inside this map.
            assert_eq!(line.clear, line.blocked_at.is_none());
            if line.clear {
                agree_clear += 1;
            } else {
                agree_blocked += 1;
            }
        }
    }
    assert!(
        agree_clear > 100 && agree_blocked > 100,
        "{agree_clear} clear, {agree_blocked} blocked"
    );
}

#[test]
fn line_matches_brute_force_square() {
    // S 4: crossing times differ by at least 1 / (47 * 47), more than the
    // sampling step 1 / 16384, so every crossed cell is sampled.
    check_reference(&World::square(4), 47, 21);
}

#[test]
fn line_matches_brute_force_hex() {
    // S 3: crossing time denominators are at most 3 * 33, their products
    // stay below the 12288 samples.
    check_reference(&World::hex(3), 33, 22);
}

fn opaque() -> Medium {
    Medium {
        opaque: true,
        ..Medium::default()
    }
}

#[test]
fn corner_rules() {
    let world = World::square(256);
    let (eye, target) = (
        cell_centre(&world, Cell::new(0, 0, 0)),
        cell_centre(&world, Cell::new(2, 2, 0)),
    );
    let mut map = TestMap::open(4);
    *map.at(0, 1) = opaque();
    assert!(sight_line(&world, &mut map, eye, target).clear);
    assert!(sight_line(&world, &mut map, target, eye).clear);
    *map.at(1, 0) = opaque();
    let line = sight_line(&world, &mut map, eye, target);
    assert!(!line.clear);
    assert_eq!(line.clarity, 0);
    assert_eq!(line.blocked_at, Some(Cell::new(1, 1, 0)));
    assert!(!sight_line(&world, &mut map, target, eye).clear);
    // One side open but its edges closed: the corner is closed too.
    let mut map = TestMap::open(4);
    *map.at(0, 1) = opaque();
    map.close(Cell::new(0, 0, 0), Cell::new(1, 0, 0));
    assert!(!sight_line(&world, &mut map, eye, target).clear);
    assert!(!sight_line(&world, &mut map, target, eye).clear);

    // Hex, S 6: the line r = 2 from (5, 2) in cell (1, 0) to (-1, 2) in
    // cell (0, 0) passes the vertex (2, 2) of (0, 0), (1, 0) and (0, 1)
    // and only grazes (0, 1).
    let world = World::hex(6);
    let (a, b) = ([5, 2, 0], [-1, 2, 0]);
    assert_eq!(cell_of(&world, a), Cell::new(1, 0, 0));
    assert_eq!(cell_of(&world, b), Cell::new(0, 0, 0));
    // TestMap cells start at 0: the scene is shifted by one cell in q and
    // r, so (0, 1) is the map's (1, 2).
    let shift = |p: Pos| [p[0] + 6, p[1] + 6, 0];
    let mut map = TestMap::open(4);
    *map.at(1, 2) = opaque();
    assert!(sight_line(&world, &mut map, shift(a), shift(b)).clear);
    assert!(sight_line(&world, &mut map, shift(b), shift(a)).clear);
    // A line through the middle of that hex, on to (-1, 2), is blocked.
    let far = shift([-5, 10, 0]);
    assert_eq!(cell_of(&world, far), Cell::new(0, 3, 0));
    let line = sight_line(&world, &mut map, shift(a), far);
    assert!(!line.clear);
    assert_eq!(line.blocked_at, Some(Cell::new(1, 2, 0)));
}

#[test]
fn end_cells() {
    let world = World::square(256);
    let c = |x| cell_centre(&world, Cell::new(x, 0, 0));
    let mut map = TestMap::open(4);
    *map.at(0, 0) = opaque();
    assert!(sight_line(&world, &mut map, c(0), c(3)).clear);
    let mut map = TestMap::open(4);
    *map.at(3, 0) = opaque();
    let line = sight_line(&world, &mut map, c(0), c(3));
    assert_eq!((line.clear, line.clarity), (true, 256));
    let mut map = TestMap::open(4);
    map.close(Cell::new(2, 0, 0), Cell::new(3, 0, 0));
    let line = sight_line(&world, &mut map, c(0), c(3));
    assert!(!line.clear);
    assert_eq!(line.blocked_at, Some(Cell::new(3, 0, 0)));
    // An opaque cell in between blocks and is named.
    let mut map = TestMap::open(4);
    *map.at(2, 0) = opaque();
    let line = sight_line(&world, &mut map, c(0), c(3));
    assert_eq!(line.blocked_at, Some(Cell::new(2, 0, 0)));
}

#[test]
fn depth_by_length() {
    let world = World::square(256);
    let mut map = TestMap::open(4);
    map.at(1, 0).density = 256;
    let eye = [128, 128, 0];
    assert_eq!(
        sight_line(&world, &mut map, eye, [640, 128, 0]).clarity,
        128
    );
    // Ends in the middle of the fog cell: depth 128, half way to 128.
    assert_eq!(
        sight_line(&world, &mut map, eye, [384, 128, 0]).clarity,
        192
    );
    map.at(2, 0).density = 256;
    assert_eq!(sight_line(&world, &mut map, eye, [896, 128, 0]).clarity, 64);
    // Thermal sight reads heat_density instead.
    assert_eq!(
        sight_line_with(&world, &mut map, eye, [896, 128, 0], true).clarity,
        256
    );
    map.at(1, 0).heat_density = 256;
    assert_eq!(
        sight_line_with(&world, &mut map, eye, [640, 128, 0], true).clarity,
        128
    );
    // Hex: one hex crossed centre to centre is one cell length.
    let hex = World::hex(256);
    let mut map = TestMap::open(4);
    map.at(1, 0).density = 256;
    assert_eq!(
        sight_line(&hex, &mut map, [0, 0, 0], [512, 0, 0]).clarity,
        128
    );
    // Depth 9 * 256 and more: nothing gets through.
    let mut map = TestMap::open(12);
    for x in 1..11 {
        map.at(x, 0).density = 256;
    }
    let line = sight_line(&world, &mut map, eye, [128 + 11 * 256, 128, 0]);
    assert_eq!(
        (line.clear, line.clarity, line.blocked_at),
        (false, 0, None)
    );
}

#[test]
fn cross_level_default_blocks() {
    let world = World::square(256);
    let line = sight_line(&world, &mut OpenAir, [128, 128, 0], [128, 128, 1]);
    assert!(!line.clear);
    assert_eq!(line.clarity, 0);

    struct Balcony(Medium);
    impl SenseMap for Balcony {
        fn cross_level(&mut self, _: Pos, _: Pos) -> Option<Medium> {
            Some(self.0)
        }
    }
    let line = sight_line(
        &world,
        &mut Balcony(Medium::default()),
        [128, 128, 0],
        [900, 128, 1],
    );
    assert_eq!((line.clear, line.clarity), (true, 256));
    let haze = Medium {
        density: 128,
        ..Medium::default()
    };
    // 512 fine units at density 128: depth 256 over the whole length.
    let line = sight_line(&world, &mut Balcony(haze), [0, 0, 0], [512, 0, 1]);
    assert_eq!(line.clarity, 128);
    let line = sight_line(&world, &mut Balcony(opaque()), [0, 0, 0], [512, 0, 1]);
    assert!(!line.clear);
}

#[test]
fn same_point_line() {
    for world in [World::square(256), World::hex(256)] {
        let line = sight_line(&world, &mut OpenAir, [300, 700, 0], [300, 700, 0]);
        assert_eq!(
            (line.clear, line.clarity, line.blocked_at),
            (true, 256, None)
        );
    }
}

#[test]
fn sound_walk() {
    let world = World::square(256);
    let mut map = TestMap::open(4);
    map.at(1, 0).density = 256; // sound 256 per cell length
    let (a, b) = ([128, 128, 0], [640, 128, 0]);
    // Distance 512, two edges of 100, one cell of 256.
    assert_eq!(
        sound_cost(&world, &mut map, a, b, 256),
        Some(512 + 200 + 256)
    );
    assert_eq!(sound_cost(&world, &mut map, a, b, 128), Some(512 + 228));
    assert_eq!(sound_cost(&world, &mut map, a, b, 0), Some(512));
    map.close(Cell::new(1, 0, 0), Cell::new(2, 0, 0));
    assert_eq!(sound_cost(&world, &mut map, a, b, 256), None);
    assert_eq!(sound_cost(&world, &mut OpenAir, a, b, 256), Some(512));
}
