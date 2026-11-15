//! Group movement in formation: one path for the group, a place for each
//! member.
//!
//! Only the leader searches a path. Each member has a slot, an offset from
//! the leader measured along the leader's walking direction (forward, right).
//! For every cell of the leader's path the slots are turned to the current
//! direction and placed next to the leader. A slot that is blocked, already
//! taken, or not in straight sight of the leader cannot be used there; that
//! member then falls in line behind the leader on the leader's own trail, and
//! returns to its slot as soon as there is room again. So the group walks
//! side by side in the open and in single file through doors and corridors.
//!
//! This is the classic "formation slots + follow the leader" approach. It
//! costs one search plus a few line tests per member and cell. Members get
//! target cells, not full paths: consecutive targets of one member lie at
//! most a few cells apart (more after sharp turns), so the game walks each
//! member there directly or with a short search.
//!
//! Square grids: [`place_members`], slots turn in 8 directions ([`rotate`]).
//! Hex grids: [`place_members_hex`], slots turn in 6 directions
//! ([`rotate_hex`]). The `_traced` forms report each placement as a
//! [`FormationTrace`].

use sagax_core::{NoopSink, Sink};

use crate::grid::{Cell, CellMap, DenseGrid};
use crate::hex::{HexLayout, axial_step};
use crate::smooth::{hex_line_clear, line_clear};

/// Slot offsets relative to the leader: `(forward, right)` in cells, for a
/// leader walking towards +x (right is +y, as y grows downwards).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formation {
    /// One offset per member (the leader itself is not listed).
    pub slots: Vec<(i32, i32)>,
}

impl Formation {
    /// Four in a 2 x 2 block, leader front left (a squad walking side by side):
    /// one beside the leader, two behind.
    pub fn block_of_four() -> Self {
        Self {
            slots: vec![(0, 1), (-1, 0), (-1, 1)],
        }
    }

    /// Everyone in a line behind the leader.
    pub fn column(members: usize) -> Self {
        Self {
            slots: (1..=members as i32).map(|i| (-i, 0)).collect(),
        }
    }

    /// A wedge: members alternately behind left and behind right.
    pub fn wedge(members: usize) -> Self {
        let slots = (0..members as i32)
            .map(|i| {
                let row = i / 2 + 1;
                (-row, if i % 2 == 0 { -row } else { row })
            })
            .collect();
        Self { slots }
    }

    /// A wedge on a hex grid: members alternately on the two diagonals
    /// behind the leader (back left and back right, see [`rotate_hex`]).
    pub fn wedge_hex(members: usize) -> Self {
        let slots = (0..members as i32)
            .map(|i| {
                let row = i / 2 + 1;
                if i % 2 == 0 { (0, -row) } else { (-row, row) }
            })
            .collect();
        Self { slots }
    }
}

/// What [`place_members_traced`] and [`place_members_hex_traced`] report,
/// one record per member and leader step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormationTrace {
    /// Member `member` stands on `cell` while the leader stands on step
    /// `step` of its path. `slot` is `false` when the slot cell could not be
    /// used and the member fell back to the leader's trail (or a free cell
    /// next to the leader).
    Placed {
        /// Index of the member (its slot).
        member: usize,
        /// Index into the leader's path.
        step: usize,
        /// The member's target.
        cell: Cell,
        /// `true` if the target is the member's slot.
        slot: bool,
    },
    /// No free cell was found: member `member` waits on the leader's cell
    /// at step `step`.
    Unplaced {
        /// Index of the member.
        member: usize,
        /// Index into the leader's path.
        step: usize,
    },
}

/// Turns a slot offset to walking direction `(dx, dy)` (one of the 8 grid
/// directions). Diagonal directions turn by 45 degrees on the grid.
pub fn rotate((forward, right): (i32, i32), (dx, dy): (i32, i32)) -> (i32, i32) {
    let (rx, ry) = (-dy, dx);
    (forward * dx + right * rx, forward * dy + right * ry)
}

/// Places the members of `formation` along `leader_path` on a square grid.
///
/// `out[k][i]` becomes the target of member `k` while the leader stands on
/// `leader_path[i]`. A slot is used when its cell is on the leader's level,
/// free of the other members and in straight sight of the leader
/// ([`line_clear`]); otherwise the member takes the nearest free cell on the
/// leader's trail behind it. Members never share a target with each other or
/// with the leader.
pub fn place_members<M: CellMap>(
    map: &mut M,
    leader_path: &[Cell],
    formation: &Formation,
    out: &mut Vec<Vec<Cell>>,
) {
    place_members_traced(map, leader_path, formation, out, &mut NoopSink);
}

/// [`place_members`] that reports every placement to `sink`.
pub fn place_members_traced<M: CellMap, S: Sink<FormationTrace> + ?Sized>(
    map: &mut M,
    leader_path: &[Cell],
    formation: &Formation,
    out: &mut Vec<Vec<Cell>>,
    sink: &mut S,
) {
    place(
        map,
        leader_path,
        formation,
        out,
        sink,
        (1, 0),
        |a, b| {
            let step = ((b.x - a.x).signum(), (b.y - a.y).signum());
            (step != (0, 0)).then_some(step)
        },
        |leader, dir, slot| {
            let (ox, oy) = rotate(slot, dir);
            Cell::new(leader.x + ox, leader.y + oy, leader.level)
        },
        line_clear,
        |c| {
            DenseGrid::DIRECTIONS
                .iter()
                .map(|&(dx, dy)| Cell::new(c.x + dx, c.y + dy, c.level))
                .collect()
        },
    );
}

/// Turns a slot offset `(forward, right)` to hex direction `dir` and returns
/// the axial offset: `forward` steps in direction `dir`, `right` steps in the
/// direction 60 degrees clockwise of it (`dir + 5`). So `(0, 1)` is front
/// right, `(-1, 1)` back right and `(-1, 0)` straight behind.
pub fn rotate_hex((forward, right): (i32, i32), dir: usize) -> (i32, i32) {
    let (fq, fr) = axial_step(dir);
    let (rq, rr) = axial_step(dir + 5);
    (forward * fq + right * rq, forward * fr + right * rr)
}

/// [`place_members`] on a hex grid with `layout`. Slots are read as in
/// [`rotate_hex`]; [`Formation::block_of_four`] and [`Formation::column`]
/// keep their meaning, [`Formation::wedge_hex`] is the hex wedge. Sight is
/// [`hex_line_clear`].
pub fn place_members_hex<M: CellMap>(
    map: &mut M,
    layout: HexLayout,
    leader_path: &[Cell],
    formation: &Formation,
    out: &mut Vec<Vec<Cell>>,
) {
    place_members_hex_traced(map, layout, leader_path, formation, out, &mut NoopSink);
}

/// [`place_members_hex`] that reports every placement to `sink`.
pub fn place_members_hex_traced<M: CellMap, S: Sink<FormationTrace> + ?Sized>(
    map: &mut M,
    layout: HexLayout,
    leader_path: &[Cell],
    formation: &Formation,
    out: &mut Vec<Vec<Cell>>,
    sink: &mut S,
) {
    place(
        map,
        leader_path,
        formation,
        out,
        sink,
        0usize,
        |a, b| layout.direction(a.x, a.y, b.x, b.y),
        |leader, dir, slot| {
            let (dq, dr) = rotate_hex(slot, dir);
            let (q, r) = layout.to_axial(leader.x, leader.y);
            let (x, y) = layout.from_axial(q + dq, r + dr);
            Cell::new(x, y, leader.level)
        },
        |map, a, b| hex_line_clear(map, layout, a, b),
        |c| {
            (0..6)
                .map(|d| {
                    let (x, y) = layout.neighbor(c.x, c.y, d);
                    Cell::new(x, y, c.level)
                })
                .collect()
        },
    );
}

/// The placing shared by square and hex grids: `dir_of` gives the walking
/// direction between two path cells, `slot_cell` the cell of a slot, `sight`
/// whether a member can stand there, `around` the neighbours of a cell.
#[allow(clippy::too_many_arguments)]
fn place<M, D: Copy, S: Sink<FormationTrace> + ?Sized>(
    map: &mut M,
    leader_path: &[Cell],
    formation: &Formation,
    out: &mut Vec<Vec<Cell>>,
    sink: &mut S,
    first_dir: D,
    dir_of: impl Fn(Cell, Cell) -> Option<D>,
    slot_cell: impl Fn(Cell, D, (i32, i32)) -> Cell,
    sight: impl Fn(&mut M, Cell, Cell) -> bool,
    around: impl Fn(Cell) -> Vec<Cell>,
) {
    let members = formation.slots.len();
    out.resize_with(members, Vec::new);
    out.truncate(members);
    for targets in out.iter_mut() {
        targets.clear();
    }
    if leader_path.is_empty() {
        return;
    }
    let mut taken: Vec<Cell> = Vec::with_capacity(members + 1);
    let mut dir = first_dir;
    for (i, &leader) in leader_path.iter().enumerate() {
        if let Some(d) = leader_path
            .get(i + 1)
            .and_then(|&next| dir_of(leader, next))
        {
            dir = d;
        }
        taken.clear();
        taken.push(leader);
        for (k, &slot) in formation.slots.iter().enumerate() {
            let cell = slot_cell(leader, dir, slot);
            let (target, in_slot) = if !taken.contains(&cell) && sight(map, leader, cell) {
                (Some(cell), true)
            } else {
                // in line: the nearest free cell on the trail behind the leader
                let fallback = leader_path[..i]
                    .iter()
                    .rev()
                    .copied()
                    .find(|c| !taken.contains(c))
                    .or_else(|| out[k].last().copied().filter(|c| !taken.contains(c)))
                    .or_else(|| {
                        // at the very start there is no trail: any free cell
                        // next to the leader
                        around(leader)
                            .into_iter()
                            .find(|c| !taken.contains(c) && sight(map, leader, *c))
                    });
                (fallback, false)
            };
            if sink.enabled() {
                sink.emit(match target {
                    Some(cell) => FormationTrace::Placed {
                        member: k,
                        step: i,
                        cell,
                        slot: in_slot,
                    },
                    None => FormationTrace::Unplaced { member: k, step: i },
                });
            }
            // nowhere to stand: wait on the leader's cell
            let target = target.unwrap_or(leader);
            taken.push(target);
            out[k].push(target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{CellGoal, DenseGrid, GridGraph, GridRules, Preset};
    use crate::search::{Limits, find_path};

    #[test]
    fn slots_turn_with_the_walking_direction() {
        assert_eq!(rotate((0, 1), (1, 0)), (0, 1));
        assert_eq!(rotate((-1, 0), (1, 0)), (-1, 0));
        assert_eq!(rotate((0, 1), (0, 1)), (-1, 0)); // walking down: right is -x
        assert_eq!(rotate((-1, 0), (0, -1)), (0, 1)); // walking up: behind is +y
        assert_eq!(rotate((-1, 1), (1, 1)), (-2, 0));
    }

    #[test]
    fn open_ground_keeps_the_block() {
        let mut map = DenseGrid::new(30, 10);
        let path: Vec<Cell> = (5..20).map(|x| Cell::new(x, 4, 0)).collect();
        let mut out = Vec::new();
        place_members(&mut map, &path, &Formation::block_of_four(), &mut out);
        assert_eq!(out.len(), 3);
        let last = *path.last().unwrap();
        assert_eq!(out[0].last(), Some(&Cell::new(last.x, 5, 0)));
        assert_eq!(out[1].last(), Some(&Cell::new(last.x - 1, 4, 0)));
        assert_eq!(out[2].last(), Some(&Cell::new(last.x - 1, 5, 0)));
    }

    #[test]
    fn traced_placing_reports_each_member_and_changes_nothing() {
        use sagax_debug::{Collector, Overflow};
        // wall at x = 15 with a one-cell door at y = 4
        let mut map = DenseGrid::new(30, 10);
        for y in 0..10 {
            if y != 4 {
                map.set(15, y, DenseGrid::BLOCKED);
            }
        }
        let path: Vec<Cell> = (5..26).map(|x| Cell::new(x, 4, 0)).collect();
        let formation = Formation::block_of_four();
        let (mut plain, mut traced) = (Vec::new(), Vec::new());
        place_members(&mut map, &path, &formation, &mut plain);
        let mut log = Collector::new(1000, Overflow::DropNewest);
        place_members_traced(&mut map, &path, &formation, &mut traced, &mut log);
        assert_eq!(plain, traced);
        assert_eq!(log.records().len(), path.len() * 3);
        let mut fell_back = 0;
        for (n, r) in log.records().enumerate() {
            let FormationTrace::Placed {
                member,
                step,
                cell,
                slot,
            } = r.event
            else {
                panic!("{:?}", r.event);
            };
            assert_eq!((step, member), (n / 3, n % 3));
            assert_eq!(cell, traced[member][step]);
            fell_back += usize::from(!slot);
        }
        // in the door the block becomes a line
        assert!(fell_back > 0);

        // hex: the same result with and without a sink
        let layout = HexLayout::OddR;
        let (mut plain, mut traced) = (Vec::new(), Vec::new());
        place_members_hex(&mut map, layout, &path, &formation, &mut plain);
        let mut log = Collector::new(1000, Overflow::DropNewest);
        place_members_hex_traced(&mut map, layout, &path, &formation, &mut traced, &mut log);
        assert_eq!(plain, traced);
        assert_eq!(log.records().len(), path.len() * 3);
    }

    #[test]
    fn nowhere_to_stand_is_unplaced() {
        // a single open cell: the leader stands on it, the member has no room
        let mut map = DenseGrid::new(1, 1);
        let path = [Cell::new(0, 0, 0)];
        let mut out = Vec::new();
        let mut log = sagax_debug::Collector::new(10, sagax_debug::Overflow::DropNewest);
        place_members_traced(&mut map, &path, &Formation::column(1), &mut out, &mut log);
        assert_eq!(out, [vec![Cell::new(0, 0, 0)]]);
        let events: Vec<_> = log.records().map(|r| r.event).collect();
        assert_eq!(events, [FormationTrace::Unplaced { member: 0, step: 0 }]);
    }

    #[test]
    fn a_door_turns_the_block_into_a_line_and_back() {
        // wall at x = 15 with a one-cell door at y = 4
        let rules = GridRules::preset(Preset::Octile);
        let mut map = DenseGrid::new(30, 10);
        for y in 0..10 {
            if y != 4 {
                map.set(15, y, DenseGrid::BLOCKED);
            }
        }
        let mut g = GridGraph::new(map, rules, None);
        let r = find_path(
            &mut g,
            Cell::new(5, 4, 0),
            &CellGoal::new(25, 4, None, rules),
            Limits::default(),
            usize::MAX,
        );
        let mut out = Vec::new();
        place_members(&mut g.map, &r.path, &Formation::block_of_four(), &mut out);
        let door = r.path.iter().position(|c| c.x == 15).unwrap();
        // while the leader stands in the door, nobody stands in the wall and
        // the members queue on the trail
        for targets in &out {
            let t = targets[door];
            assert!(
                g.map.get(t.x, t.y).is_some_and(|c| c != DenseGrid::BLOCKED),
                "{t:?}"
            );
            assert!(t.x < 15);
        }
        // at the end the block is complete again
        let end = *r.path.last().unwrap();
        assert_eq!(out[0].last(), Some(&Cell::new(end.x, end.y + 1, 0)));
        assert_eq!(out[2].last(), Some(&Cell::new(end.x - 1, end.y + 1, 0)));
        // no two members ever share a cell, nor with the leader
        for i in 0..r.path.len() {
            let mut cells = vec![r.path[i]];
            for targets in &out {
                assert!(!cells.contains(&targets[i]), "step {i}");
                cells.push(targets[i]);
            }
        }
    }
}
