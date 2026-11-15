//! Perception, plus the helpers that join it with nav and memory.
//!
//! Everything of [`sagax_perception`] is re-exported here. On top:
//!
//! * [`GridSight`] and [`MoveBlocks`] derive a [`SenseMap`] from a nav
//!   map for games that have no own sight data.
//! * [`flood_sound`] spreads a [`Stimulus`] around corners with one
//!   [`Navigator::reachable`] over [`SoundCost`]; each listener then calls
//!   [`hear`] with the cost of its cell.
//! * [`percept_fact`], [`alert_amount`] and [`learn_percepts`] turn
//!   [`Percept`]s into [`Knowledge`] facts (whole cells, never fine points)
//!   and [`Alertness`] stimuli.
//!
//! Perception cells are axial on hex, nav maps are keyed by offset cells:
//! the `hex` constructors take the map's [`HexLayout`] and convert every
//! cell before asking the map, and the flood converts with the
//! navigator's layout.
//!
//! `Budget` here is the attention budget of [`look`]; the navigator's is
//! [`sagax::nav::Budget`](sagax_nav::Budget) (also in the prelude).

use sagax_core::{Cell, HexLayout, NoopSink, Sink};
use sagax_decide::memory::{
    AlertConfig, Alertness, Fact, Knowledge, KnowledgeTrace, SOURCE_HEARD, SOURCE_OWN,
};
use sagax_nav::grid::{CellMap, DenseGrid};
use sagax_nav::nav::Navigator;
use sagax_nav::search::Cost;
use sagax_nav::topology::Topology;

pub use sagax_perception::*;

/// An axial cell as the offset cell of `layout`; unchanged without one.
fn to_map(layout: Option<HexLayout>, cell: Cell) -> Cell {
    match layout {
        Some(layout) => {
            let (x, y) = layout.from_axial(cell.x, cell.y);
            Cell::new(x, y, cell.level)
        }
        None => cell,
    }
}

/// An offset cell of `layout` as the axial cell; unchanged without one.
fn to_axial(layout: Option<HexLayout>, cell: Cell) -> Cell {
    match layout {
        Some(layout) => {
            let (q, r) = layout.to_axial(cell.x, cell.y);
            Cell::new(q, r, cell.level)
        }
        None => cell,
    }
}

/// Sight over a [`DenseGrid`]: [`BLOCKED`](DenseGrid::BLOCKED) cells and
/// cells off the grid are opaque, nothing else (no fog, no thin walls).
#[derive(Clone, Copy, Debug)]
pub struct GridSight<'a> {
    /// The map.
    pub grid: &'a DenseGrid,
    /// On hex, the map's offset layout (cells are converted from axial);
    /// `None` on square.
    pub layout: Option<HexLayout>,
}

impl<'a> GridSight<'a> {
    /// Over a square grid.
    pub fn square(grid: &'a DenseGrid) -> Self {
        Self { grid, layout: None }
    }

    /// Over a hex grid stored in offset cells of `layout`.
    pub fn hex(grid: &'a DenseGrid, layout: HexLayout) -> Self {
        Self {
            grid,
            layout: Some(layout),
        }
    }
}

impl SenseMap for GridSight<'_> {
    fn sight(&mut self, cell: Cell) -> Medium {
        let cost = self.grid.get_at(to_map(self.layout, cell));
        Medium {
            opaque: cost.is_none_or(|c| c == DenseGrid::BLOCKED),
            ..Medium::default()
        }
    }
}

/// Sight blocked where movement is: crossing from a cell into its
/// neighbour is open exactly when [`CellMap::step`] allows the step and
/// stays on the same level. For games that share one blocker for walking
/// and seeing.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveBlocks<M: CellMap> {
    /// The movement map.
    pub map: M,
    /// On hex, the map's offset layout (cells are converted from axial and
    /// `step` gets the offset delta); `None` on square.
    pub layout: Option<HexLayout>,
}

impl<M: CellMap> MoveBlocks<M> {
    /// Over a square map.
    pub fn square(map: M) -> Self {
        Self { map, layout: None }
    }

    /// Over a hex map keyed by offset cells of `layout`.
    pub fn hex(map: M, layout: HexLayout) -> Self {
        Self {
            map,
            layout: Some(layout),
        }
    }
}

impl<M: CellMap> SenseMap for MoveBlocks<M> {
    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool {
        let (from, to) = (to_map(self.layout, from), to_map(self.layout, to));
        self.map.step(from, to.x - from.x, to.y - from.y) == Some(from.level)
    }
}

/// Sound as a walking cost, for [`Navigator::reachable`]: every step stays
/// on its level unless [`SenseMap::sound_edge`] lets nothing through, and
/// costs the navigator's step cost plus
/// `damping * (sound(to) + sound_edge(from, to)) / 256`.
///
/// The navigator's cells are its own (offset on hex); with a `layout` they
/// are converted to axial before the map is asked.
pub struct SoundCost<'a, H: SenseMap> {
    /// The game's hook.
    pub map: &'a mut H,
    /// The channel's damping, per 256.
    pub damping: u16,
    /// On hex, the navigator's offset layout; `None` on square.
    pub layout: Option<HexLayout>,
}

impl<H: SenseMap> CellMap for SoundCost<'_, H> {
    fn step(&mut self, from: Cell, dx: i32, dy: i32) -> Option<i32> {
        let to = Cell::new(from.x + dx, from.y + dy, from.level);
        let (a, b) = (to_axial(self.layout, from), to_axial(self.layout, to));
        self.map.sound_edge(a, b).map(|_| from.level)
    }

    fn move_cost(&mut self, from: Cell, to: Cell) -> Option<Cost> {
        let (from, to) = (to_axial(self.layout, from), to_axial(self.layout, to));
        let edge = self.map.sound_edge(from, to)?;
        let damp = u64::from(self.map.sound(to)) + u64::from(edge);
        let extra = damp * u64::from(self.damping) / 256;
        Some(extra.min(u64::from(Cost::MAX)) as Cost)
    }
}

/// Every cell `stimulus` reaches around corners, with its cost in fine
/// units, into `out` (cleared first): one [`Navigator::reachable`] from
/// `cell_of(stimulus.point)` over [`SoundCost`] with the channel's
/// damping, bounded by the loudness, by `budget` and by the navigator's
/// capacity. Each listener then calls [`hear`] with `Some(cost)` of its
/// cell (no entry: not reached).
///
/// The navigator's [`Topology`] decides square or hex; its straight step
/// cost (on hex: every step) must be `world.cell_size`, so that a cost is
/// in fine units. On hex the navigator works in offset cells of its
/// layout; `out` holds axial cells, as [`cell_of`] gives them. A channel
/// past [`MAX_CHANNELS`] reaches nothing. Replaces the navigator's last
/// search.
pub fn flood_sound<H: SenseMap>(
    nav: &mut Navigator,
    world: &World,
    map: &mut H,
    stimulus: &Stimulus,
    budget: sagax_nav::Budget,
    out: &mut Vec<(Cell, i32)>,
) {
    out.clear();
    let Some(channel) = world.channels.get(usize::from(stimulus.channel)) else {
        return;
    };
    let layout = match nav.topology() {
        Topology::Hex { layout, .. } => Some(layout),
        Topology::Square(_) => None,
    };
    let start = to_map(layout, cell_of(world, stimulus.point));
    let max = Cost::try_from(stimulus.loudness).unwrap_or(0);
    let mut cost = SoundCost {
        map,
        damping: channel.damping,
        layout,
    };
    nav.reachable(&mut cost, start, max, budget);
    out.extend(nav.area().map(|(cell, cost)| {
        (
            to_axial(layout, cell),
            i32::try_from(cost).unwrap_or(i32::MAX),
        )
    }));
}

/// The game's fact kinds for [`percept_fact`] and [`learn_percepts`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FactKinds {
    /// A target noticed (stage 1).
    pub noticed_kind: u32,
    /// A target recognised (stage 2).
    pub seen_kind: u32,
    /// A heard stimulus.
    pub heard_kind: u32,
    /// A disguised target, in place of the noticed and seen kinds; `None`:
    /// disguised percepts are skipped (not a threat at all).
    pub disguised_kind: Option<u32>,
}

/// The [`Alertness`] stimulus per percept kind (0: none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AlertAmounts {
    /// For `Noticed`.
    pub noticed: u32,
    /// For `Recognized`.
    pub recognized: u32,
    /// For every `Seen`.
    pub seen: u32,
    /// For `Heard`.
    pub heard: u32,
    /// Cue bits: entry `i` matches a `Cues { new }` that shares a bit with
    /// it.
    pub cue_bits: [u32; 8],
    /// The amount of entry `i` of `cue_bits`; the matching ones add up.
    pub cue_amount: [u32; 8],
}

/// The fact kind a percept stands for, `None` for none.
fn fact_kind(p: &Percept, kinds: &FactKinds) -> Option<u32> {
    if p.event == PerceptKind::Heard {
        return Some(kinds.heard_kind);
    }
    let (noticed, seen) = noticed_and_seen(p, kinds)?;
    match p.event {
        PerceptKind::Noticed => Some(noticed),
        PerceptKind::Seen if p.stage < 2 => Some(noticed),
        PerceptKind::Recognized | PerceptKind::Seen | PerceptKind::Cues { .. } => Some(seen),
        _ => None,
    }
}

/// The noticed and seen kinds for this percept: `disguised_kind` for both
/// when it is disguised (`None` without one).
fn noticed_and_seen(p: &Percept, kinds: &FactKinds) -> Option<(u32, u32)> {
    if p.disguised {
        kinds.disguised_kind.map(|k| (k, k))
    } else {
        Some((kinds.noticed_kind, kinds.seen_kind))
    }
}

/// The fact a percept stands for: `Noticed` and `Seen` at stage noticed
/// give `noticed_kind`; `Recognized`, `Seen` at stage recognised and
/// `Cues` give `seen_kind`; `Heard` gives `heard_kind`. A disguised
/// percept uses `disguised_kind`. `pos` is the whole cell, `seen_at` the
/// percept's time, `data` its cues; `source` [`SOURCE_HEARD`] with the
/// margin as `strength` for hearing, else [`SOURCE_OWN`] with the clarity.
///
/// `None` for `Lost`, `Forgotten`, `SeenThrough`, for a percept that is
/// not `placed` (no real position), and for a disguised percept without a
/// disguised kind.
pub fn percept_fact(p: &Percept, kinds: &FactKinds) -> Option<Fact> {
    if !p.placed {
        return None;
    }
    let kind = fact_kind(p, kinds)?;
    let heard = matches!(p.sense, Sense::Hearing(_));
    Some(Fact {
        subject: p.subject,
        kind,
        pos: p.cell,
        seen_at: p.at,
        strength: if heard {
            p.margin.max(0) as u32
        } else {
            u32::from(p.clarity)
        },
        data: p.cues,
        source: if heard { SOURCE_HEARD } else { SOURCE_OWN },
    })
}

/// The [`Alertness::stimulus`] amount of a percept: by its kind, and for
/// `Cues { new }` the sum (saturating) of every `cue_amount` whose
/// `cue_bits` share a bit with `new`. 0 for the others.
pub fn alert_amount(p: &Percept, amounts: &AlertAmounts) -> u32 {
    match p.event {
        PerceptKind::Noticed => amounts.noticed,
        PerceptKind::Recognized => amounts.recognized,
        PerceptKind::Seen => amounts.seen,
        PerceptKind::Heard => amounts.heard,
        PerceptKind::Cues { new } => amounts
            .cue_bits
            .iter()
            .zip(&amounts.cue_amount)
            .filter(|&(&bits, _)| bits & new != 0)
            .fold(0u32, |sum, (_, &a)| sum.saturating_add(a)),
        _ => 0,
    }
}

/// The common use of percepts in one call, in order:
///
/// | Percept | Knowledge |
/// |---|---|
/// | `Noticed`, or `Seen` at stage noticed | `learn` the noticed kind at the cell |
/// | `Recognized`, or `Seen` at stage recognised | `remove` the noticed kind, `learn` the seen kind |
/// | `Cues` | `set_data` of the seen-kind fact |
/// | `Heard` | `merge` the heard kind (a newer one is not overwritten) |
/// | `Lost`, `Forgotten`, `SeenThrough` | nothing: facts age out by the game's `expire*` |
///
/// and `alert.stimulus(alert_amount(p))` for each amount above 0. A
/// disguised percept uses `disguised_kind` for both the noticed and the
/// seen kind, or is skipped entirely (no fact, no alert). A percept that
/// is not `placed` changes no fact (its alert amount still counts).
pub fn learn_percepts(
    known: &mut Knowledge,
    alert: &mut Alertness,
    alert_cfg: &AlertConfig,
    now: u32,
    percepts: &[Percept],
    kinds: &FactKinds,
    amounts: &AlertAmounts,
) {
    learn_percepts_traced(
        known,
        alert,
        alert_cfg,
        now,
        percepts,
        kinds,
        amounts,
        &mut NoopSink,
    );
}

/// [`learn_percepts`] with the [`Knowledge`] trace of every `learn`,
/// `remove` and `merge` it makes in `sink`.
#[allow(clippy::too_many_arguments)]
pub fn learn_percepts_traced(
    known: &mut Knowledge,
    alert: &mut Alertness,
    alert_cfg: &AlertConfig,
    now: u32,
    percepts: &[Percept],
    kinds: &FactKinds,
    amounts: &AlertAmounts,
    sink: &mut impl Sink<KnowledgeTrace>,
) {
    for p in percepts {
        let Some((noticed, _)) = noticed_and_seen(p, kinds) else {
            continue;
        };
        let amount = alert_amount(p, amounts);
        if amount > 0 {
            alert.stimulus(amount, now, alert_cfg);
        }
        let Some(fact) = percept_fact(p, kinds) else {
            continue;
        };
        match p.event {
            PerceptKind::Noticed | PerceptKind::Seen if fact.kind == noticed => {
                known.learn_traced(now, fact, sink);
            }
            PerceptKind::Recognized | PerceptKind::Seen => {
                if noticed != fact.kind {
                    known.remove_traced(p.subject, noticed, sink);
                }
                known.learn_traced(now, fact, sink);
            }
            PerceptKind::Cues { .. } => {
                known.set_data(p.subject, fact.kind, p.cues);
            }
            PerceptKind::Heard => {
                known.merge_traced(now, fact, sink);
            }
            _ => {}
        }
    }
}
