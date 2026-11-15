//! `sx_perception_*`, `sx_sense_map_*`, `sx_percept_*`, `sx_awareness_*`:
//! what a character perceives (sight, hearing), see `sagax.h`.
//!
//! One handle (`sx_perception`) holds the world (grid, cell size, hearing
//! channels), the game's sense map and the trace; everything per character
//! is a value struct the game keeps. Fine points are axial on hex and are
//! never offset-converted; whole cells (percept cells, facts, flood cells,
//! the cell helpers, the cells the map callbacks get) are offset cells on
//! hex, converted by the handle's layout.

use std::ffi::c_void;

use sagax_abi::convert::{self, PerceptionTraceCode};
use sagax_core::{Cell, HexLayout, Pos};
use sagax_facade::perception::{
    AlertAmounts, Attention, Awareness, Band, Budget, Channel, CueRule, DisguiseRule, Eye,
    FactKinds, Grid, MAX_CHANNELS, MAX_CUES, Medium, Percept, PerceptionTrace, SeeThrough,
    SenseMap, SenseProfile, Stats, Stimulus, Target, World, alert_amount, cell_centre, cell_of,
    count_crowds, flood_sound, hear_traced, hex_heading, in_sight, learn_percepts_traced,
    look_traced, observe_traced, percept_fact, sight_line,
};
use sagax_nav::grid::DenseGrid;
use sagax_nav::topology::Topology;

use super::grid::SxGrid;
use super::memory::{SxAlertConfig, SxAlertness, SxFact, SxKnowledge, SxKnowledgeTrace, put};
use super::{
    Begun, ERROR_ARGUMENT, Point, SxMap, SxNav, TraceSlot, create, destroy, guarded, set_trace,
    slice_of, to_i32,
};

/// `sx_target`.
pub type SxTarget = Target;
/// `sx_stats`.
pub type SxStats = Stats;
/// `sx_cue_rule`.
pub type SxCueRule = CueRule;
/// `sx_see_through`.
pub type SxSeeThrough = SeeThrough;
/// `sx_awareness` (private fields; zero to start).
pub type SxAwareness = Awareness;
/// `sx_attention` (private field; zero to start).
pub type SxAttention = Attention;
/// `sx_budget` (the attention budget of `sx_perception_look`).
pub type SxBudget = Budget;
/// `sx_stimulus`.
pub type SxStimulus = Stimulus;

// ---- the game's sense map ----------------------------------------------------

/// `sx_medium`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxMedium {
    /// 1: stops sight.
    pub opaque: u8,
    /// Unused.
    pub _pad: u8,
    /// Optical depth per full cell length.
    pub density: u16,
    /// The same for thermal sight.
    pub heat_density: u16,
    /// Unused.
    pub _pad2: u16,
}

impl SxMedium {
    fn medium(self) -> Medium {
        Medium {
            opaque: self.opaque != 0,
            density: self.density,
            heat_density: self.heat_density,
        }
    }
}

/// `sx_sight_fn`.
pub type SightFn = unsafe extern "C" fn(*mut c_void, i32, i32, i32, *mut SxMedium) -> i32;
/// `sx_sight_edge_fn` and `sx_sound_edge_fn`.
pub type EdgeFn = unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32, i32) -> i32;
/// `sx_cross_level_fn`.
pub type CrossLevelFn =
    unsafe extern "C" fn(*mut c_void, *const i32, *const i32, *mut SxMedium) -> i32;
/// `sx_light_fn` and `sx_sound_fn`.
pub type CellFn = unsafe extern "C" fn(*mut c_void, i32, i32, i32) -> i32;

/// `sx_sense_map`: one optional callback per question (null: the default)
/// and one context.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SxSenseMap {
    /// Inside a cell; null: clear air.
    pub sight: Option<SightFn>,
    /// Crossing into a neighbour; null: open.
    pub sight_edge: Option<EdgeFn>,
    /// A line between two levels; null: blocked.
    pub cross_level: Option<CrossLevelFn>,
    /// Light at a cell; null: 255.
    pub light: Option<CellFn>,
    /// Sound damping inside a cell; null: 0.
    pub sound: Option<CellFn>,
    /// Sound damping crossing into a neighbour; null: 0.
    pub sound_edge: Option<EdgeFn>,
    /// Passed to every callback.
    pub context: *mut c_void,
}

impl Default for SxSenseMap {
    fn default() -> Self {
        Self {
            sight: None,
            sight_edge: None,
            cross_level: None,
            light: None,
            sound: None,
            sound_edge: None,
            context: std::ptr::null_mut(),
        }
    }
}

/// An axial cell as the offset cell of `layout`; unchanged without one.
fn to_offset(layout: Option<HexLayout>, cell: Cell) -> Cell {
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

/// The game's sense map seen through an `sx_sense_map`: cells go to the
/// callbacks as offset cells on hex.
#[derive(Clone, Copy, Debug, Default)]
struct CallbackSenseMap {
    map: SxSenseMap,
    layout: Option<HexLayout>,
}

impl CallbackSenseMap {
    fn cell(&self, cell: Cell) -> Cell {
        to_offset(self.layout, cell)
    }

    fn edge(&self, f: EdgeFn, from: Cell, to: Cell) -> i32 {
        let (a, b) = (self.cell(from), self.cell(to));
        // SAFETY: the game passed valid callbacks and context for as long
        // as the map is set (sagax.h).
        unsafe { f(self.map.context, a.x, a.y, a.level, b.x, b.y) }
    }

    fn at(&self, f: CellFn, cell: Cell) -> i32 {
        let c = self.cell(cell);
        // SAFETY: as in `edge`.
        unsafe { f(self.map.context, c.x, c.y, c.level) }
    }
}

impl SenseMap for CallbackSenseMap {
    fn sight(&mut self, cell: Cell) -> Medium {
        let Some(f) = self.map.sight else {
            return Medium::default();
        };
        let c = self.cell(cell);
        let mut m = SxMedium::default();
        // SAFETY: as in `edge`; `m` is writable.
        unsafe { f(self.map.context, c.x, c.y, c.level, &mut m) };
        m.medium()
    }

    fn sight_edge(&mut self, from: Cell, to: Cell) -> bool {
        self.map
            .sight_edge
            .is_none_or(|f| self.edge(f, from, to) != 0)
    }

    fn cross_level(&mut self, from: Pos, to: Pos) -> Option<Medium> {
        let f = self.map.cross_level?;
        let mut m = SxMedium::default();
        // SAFETY: as in `edge`; both points hold 3 integers, `m` is writable.
        let passes = unsafe { f(self.map.context, from.as_ptr(), to.as_ptr(), &mut m) };
        (passes != 0).then(|| m.medium())
    }

    fn light(&mut self, cell: Cell) -> u8 {
        match self.map.light {
            Some(f) => self.at(f, cell).clamp(0, 255) as u8,
            None => 255,
        }
    }

    fn sound(&mut self, cell: Cell) -> u32 {
        match self.map.sound {
            Some(f) => self.at(f, cell).max(0) as u32,
            None => 0,
        }
    }

    fn sound_edge(&mut self, from: Cell, to: Cell) -> Option<u32> {
        match self.map.sound_edge {
            Some(f) => u32::try_from(self.edge(f, from, to)).ok(),
            None => Some(0),
        }
    }
}

/// The `sight` callback of [`sx_sense_map_from_grid`].
unsafe extern "C" fn grid_sight(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    level: i32,
    out: *mut SxMedium,
) -> i32 {
    // SAFETY: the context is the grid given to sx_sense_map_from_grid,
    // valid while the map is used (sagax.h).
    let Some(grid) = (unsafe { ctx.cast::<SxGrid>().as_ref() }) else {
        return 0;
    };
    let cost = grid.dense().get_at(Cell::new(x, y, level));
    let opaque = cost.is_none_or(|c| c == DenseGrid::BLOCKED);
    // SAFETY: the library passes a writable medium.
    unsafe {
        put(
            out,
            SxMedium {
                opaque: u8::from(opaque),
                ..SxMedium::default()
            },
        )
    };
    0
}

/// `sx_sense_map_from_grid`: sight over an `sx_grid`: blocked cells and
/// cells off the grid are opaque, nothing else. Valid while the grid lives.
/// Returns 0, or `SX_ERROR_ARGUMENT` for a null pointer.
///
/// # Safety
/// `grid` is null or from `sx_grid_new`; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sense_map_from_grid(grid: *const SxGrid, out: *mut SxSenseMap) -> i32 {
    if grid.is_null() || out.is_null() {
        return ERROR_ARGUMENT;
    }
    let map = SxSenseMap {
        sight: Some(grid_sight),
        context: grid.cast_mut().cast(),
        ..SxSenseMap::default()
    };
    // SAFETY: not null and writable (see above).
    unsafe { put(out, map) };
    0
}

/// The `sight_edge` callback of [`sx_sense_map_from_map`].
unsafe extern "C" fn map_sight_edge(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    level: i32,
    to_x: i32,
    to_y: i32,
) -> i32 {
    // SAFETY: the context is the map given to sx_sense_map_from_map, valid
    // while the sense map is used (sagax.h).
    let Some(map) = (unsafe { ctx.cast::<SxMap>().as_ref() }) else {
        return 0;
    };
    let Some(step) = map.step else {
        return 0;
    };
    // SAFETY: the game's step callback with its own context (sagax.h).
    let arrives = unsafe { step(map.context, x, y, level, to_x - x, to_y - y) };
    i32::from(arrives == level)
}

/// `sx_sense_map_from_map`: sight blocked where movement is (crossing into
/// a neighbour is open exactly when `step` allows it on the same level).
/// Valid while `map` (the struct itself) lives. Returns 0, or
/// `SX_ERROR_ARGUMENT` for a null pointer or a map without `step`.
///
/// # Safety
/// `map` is null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sense_map_from_map(map: *const SxMap, out: *mut SxSenseMap) -> i32 {
    // SAFETY: null or valid (see above).
    let Some(m) = (unsafe { map.as_ref() }) else {
        return ERROR_ARGUMENT;
    };
    if m.step.is_none() || out.is_null() {
        return ERROR_ARGUMENT;
    }
    let sense = SxSenseMap {
        sight_edge: Some(map_sight_edge),
        context: map.cast_mut().cast(),
        ..SxSenseMap::default()
    };
    // SAFETY: not null and writable (see above).
    unsafe { put(out, sense) };
    0
}

// ---- value structs -------------------------------------------------------------

/// `sx_channel`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxChannel {
    /// How much the map's sound damping counts, per 256.
    pub damping: u16,
    /// Position error in cells per cell of cost, per 256.
    pub fuzz: u16,
    /// Fine units per tick (0: at once).
    pub speed: i32,
    /// `SX_SPREAD_*`.
    pub spread: u32,
}

impl SxChannel {
    fn channel(&self) -> Option<Channel> {
        Some(Channel {
            damping: self.damping,
            fuzz: self.fuzz,
            speed: self.speed,
            spread: convert::spread(self.spread)?,
        })
    }

    fn of(c: &Channel) -> Self {
        Self {
            damping: c.damping,
            fuzz: c.fuzz,
            speed: c.speed,
            spread: convert::spread_code(c.spread),
        }
    }
}

/// `sx_perception_config`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxPerceptionConfig {
    /// `SX_GRID_*`.
    pub grid: u32,
    /// Fine units per cell or hex, at least 2.
    pub cell_size: i32,
    /// The hearing channels.
    pub channels: [SxChannel; MAX_CHANNELS],
}

impl SxPerceptionConfig {
    fn world(&self) -> Option<(World, Option<HexLayout>)> {
        let layout = convert::grid_layout(self.grid)?;
        if self.cell_size < 2 {
            return None;
        }
        let mut channels = [Channel::default(); MAX_CHANNELS];
        for (c, s) in channels.iter_mut().zip(&self.channels) {
            *c = s.channel()?;
        }
        let grid = if layout.is_some() {
            Grid::Hex
        } else {
            Grid::Square
        };
        let world = World {
            grid,
            cell_size: self.cell_size,
            channels,
        };
        Some((world, layout))
    }
}

/// `sx_band`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxBand {
    /// Half-angle in headings (0..=128).
    pub half_angle: u8,
    /// Gain per 256; 0: unused.
    pub gain: u16,
    /// 1: instant cues fire inside this band.
    pub instant: u8,
}

impl SxBand {
    fn band(self) -> Band {
        Band {
            half_angle: self.half_angle,
            gain: self.gain,
            instant: self.instant != 0,
        }
    }

    fn of(b: Band) -> Self {
        Self {
            half_angle: b.half_angle,
            gain: b.gain,
            instant: u8::from(b.instant),
        }
    }
}

/// `sx_sense_profile`: the fields of `SenseProfile`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct SxSenseProfile {
    pub senses: u8,
    pub range: i32,
    pub near: i32,
    pub far_gain: u16,
    pub bands: [SxBand; 3],
    pub base_gain: u16,
    pub dark: u8,
    pub bright: u8,
    pub dark_gain: u16,
    pub notice_at: u16,
    pub recognize_at: u16,
    pub max: u16,
    pub decay_per_tick: u16,
    pub hold: u16,
    pub hysteresis: u16,
    pub check_interval: u16,
    pub crowd_gain: [u16; 8],
    pub scanner_range: i32,
    pub scanner_gain: u16,
    pub hearing: [u16; MAX_CHANNELS],
    pub hear_gain: u16,
    pub cues: [SxCueRule; MAX_CUES],
}

impl SxSenseProfile {
    /// The Rust profile.
    pub fn profile(&self) -> SenseProfile {
        SenseProfile {
            senses: self.senses,
            range: self.range,
            near: self.near,
            far_gain: self.far_gain,
            bands: self.bands.map(SxBand::band),
            base_gain: self.base_gain,
            dark: self.dark,
            bright: self.bright,
            dark_gain: self.dark_gain,
            notice_at: self.notice_at,
            recognize_at: self.recognize_at,
            max: self.max,
            decay_per_tick: self.decay_per_tick,
            hold: self.hold,
            hysteresis: self.hysteresis,
            check_interval: self.check_interval,
            crowd_gain: self.crowd_gain,
            scanner_range: self.scanner_range,
            scanner_gain: self.scanner_gain,
            hearing: self.hearing,
            hear_gain: self.hear_gain,
            cues: self.cues,
        }
    }

    /// The C profile.
    pub fn of(p: &SenseProfile) -> Self {
        Self {
            senses: p.senses,
            range: p.range,
            near: p.near,
            far_gain: p.far_gain,
            bands: p.bands.map(SxBand::of),
            base_gain: p.base_gain,
            dark: p.dark,
            bright: p.bright,
            dark_gain: p.dark_gain,
            notice_at: p.notice_at,
            recognize_at: p.recognize_at,
            max: p.max,
            decay_per_tick: p.decay_per_tick,
            hold: p.hold,
            hysteresis: p.hysteresis,
            check_interval: p.check_interval,
            crowd_gain: p.crowd_gain,
            scanner_range: p.scanner_range,
            scanner_gain: p.scanner_gain,
            hearing: p.hearing,
            hear_gain: p.hear_gain,
            cues: p.cues,
        }
    }
}

/// `sx_disguise_rule`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxDisguiseRule {
    /// The cue bits a disguise masks.
    pub hides: u32,
    /// How this observer sees through a disguise; valid if
    /// `has_see_through`.
    pub see_through: SxSeeThrough,
    /// 1: see-through is on; 0: a disguise always holds.
    pub has_see_through: u8,
}

impl SxDisguiseRule {
    fn rule(&self) -> DisguiseRule {
        DisguiseRule {
            hides: self.hides,
            see_through: (self.has_see_through != 0).then_some(self.see_through),
        }
    }
}

/// `sx_eye`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SxEye {
    /// The observer's id.
    pub id: u32,
    /// Where the eye is, fine point (axial on hex).
    pub point: [i32; 3],
    /// Facing heading, taken modulo 256 (0 = +x, 64 = +y).
    pub facing: i32,
    /// What it can sense (required).
    pub profile: *const SxSenseProfile,
    /// Its stats for this call.
    pub stats: SxStats,
    /// What a disguise does for it.
    pub disguise: SxDisguiseRule,
}

/// The parts of an `sx_eye` that an [`Eye`] borrows.
struct EyeParts {
    eye: SxEye,
    profile: SenseProfile,
}

impl EyeParts {
    /// The eye at `eye`, or `None` for a null eye or profile.
    fn of(eye: *const SxEye) -> Option<Self> {
        // SAFETY: the caller of the sx_* function passes null or a valid eye.
        let eye = *unsafe { eye.as_ref() }?;
        // SAFETY: the eye's profile is null or valid (sagax.h).
        let profile = unsafe { eye.profile.as_ref() }?.profile();
        Some(Self { eye, profile })
    }

    fn eye(&self) -> Eye<'_> {
        Eye {
            id: self.eye.id,
            point: self.eye.point,
            facing: self.eye.facing.rem_euclid(256) as u8,
            profile: &self.profile,
            stats: self.eye.stats,
            disguise: self.eye.disguise.rule(),
        }
    }
}

/// `sx_line`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxLine {
    /// 1: sight gets through.
    pub clear: u8,
    /// 1: `blocked_at` is set.
    pub has_blocked_at: u8,
    /// How much gets through, 0..=256.
    pub clarity: u16,
    /// Where sight stopped (a whole cell, offset on hex).
    pub blocked_at: [i32; 3],
}

/// `sx_percept`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxPercept {
    /// The observer's id.
    pub observer: u32,
    /// The target's (or the stimulus source's) id.
    pub subject: u32,
    /// `SX_SENSE_*`.
    pub sense: u32,
    /// `SX_SENSE_HEARING`: the channel.
    pub channel: u32,
    /// `SX_PERCEPT_*`.
    pub event: u32,
    /// `SX_PERCEPT_CUES`: the new cue bits.
    pub new_cues: u32,
    /// Where, fine point (axial on hex).
    pub point: [i32; 3],
    /// The whole cell (offset on hex).
    pub cell: [i32; 3],
    /// The cue bits seen.
    pub cues: u32,
    /// Hearing: how clearly heard.
    pub margin: i32,
    /// Hearing: when it arrives.
    pub arrive_at: u32,
    /// The time of the check.
    pub at: u32,
    /// How clearly, 0..=256.
    pub clarity: u16,
    /// The counted stage after the check.
    pub stage: u8,
    /// 1: `point` and `cell` are a real position from this call.
    pub placed: u8,
    /// 1: the target is disguised for this observer.
    pub disguised: u8,
}

impl SxPercept {
    /// The C percept; with `layout` (hex) a placed cell becomes offset.
    pub fn of(p: &Percept, layout: Option<HexLayout>) -> Self {
        let (sense, channel) = convert::sense_code(p.sense);
        let (event, new_cues) = convert::percept_kind_code(p.event);
        let cell = if p.placed {
            to_offset(layout, Cell::from(p.cell)).pos()
        } else {
            p.cell
        };
        Self {
            observer: p.observer,
            subject: p.subject,
            sense,
            channel,
            event,
            new_cues,
            point: p.point,
            cell,
            cues: p.cues,
            margin: p.margin,
            arrive_at: p.arrive_at,
            at: p.at,
            clarity: p.clarity,
            stage: p.stage,
            placed: u8::from(p.placed),
            disguised: u8::from(p.disguised),
        }
    }

    /// The Rust percept, with the cell as it is (offset on hex); `None` for
    /// an unknown sense or event.
    pub(crate) fn percept(&self) -> Option<Percept> {
        Some(Percept {
            observer: self.observer,
            subject: self.subject,
            sense: convert::sense(self.sense, self.channel)?,
            event: convert::percept_kind(self.event, self.new_cues)?,
            stage: self.stage,
            point: self.point,
            cell: self.cell,
            placed: self.placed != 0,
            cues: self.cues,
            disguised: self.disguised != 0,
            clarity: self.clarity,
            margin: self.margin,
            arrive_at: self.arrive_at,
            at: self.at,
        })
    }
}

/// `sx_fact_kinds`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxFactKinds {
    /// A target noticed.
    pub noticed_kind: u32,
    /// A target recognised.
    pub seen_kind: u32,
    /// A heard stimulus.
    pub heard_kind: u32,
    /// A disguised target; valid if `has_disguised_kind`.
    pub disguised_kind: u32,
    /// 0: disguised percepts are skipped.
    pub has_disguised_kind: u8,
}

impl SxFactKinds {
    pub(crate) fn kinds(&self) -> FactKinds {
        FactKinds {
            noticed_kind: self.noticed_kind,
            seen_kind: self.seen_kind,
            heard_kind: self.heard_kind,
            disguised_kind: (self.has_disguised_kind != 0).then_some(self.disguised_kind),
        }
    }
}

/// `sx_alert_amounts`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxAlertAmounts {
    /// For `NOTICED`.
    pub noticed: u32,
    /// For `RECOGNIZED`.
    pub recognized: u32,
    /// For every `SEEN`.
    pub seen: u32,
    /// For `HEARD`.
    pub heard: u32,
    /// Cue bits per entry.
    pub cue_bits: [u32; 8],
    /// The amount of each entry.
    pub cue_amount: [u32; 8],
}

impl SxAlertAmounts {
    pub(crate) fn amounts(&self) -> AlertAmounts {
        AlertAmounts {
            noticed: self.noticed,
            recognized: self.recognized,
            seen: self.seen,
            heard: self.heard,
            cue_bits: self.cue_bits,
            cue_amount: self.cue_amount,
        }
    }
}

// ---- trace -------------------------------------------------------------------------

/// `sx_perception_trace`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SxPerceptionTrace {
    /// `SX_PERCEPTION_TRACE_*`.
    pub kind: u32,
    /// The target's id; `HEARD`, `UNHEARD`: the stimulus id.
    pub target: u32,
    /// `PICKED`: `SX_LOOK_*`.
    pub why: u32,
    /// `GAIN`: `SX_SENSE_*`.
    pub sense: u32,
    /// `GAIN`: the channel if hearing.
    pub channel: u32,
    /// `GAIN`: the gain of the check.
    pub gain: u32,
    /// `HEARD`, `UNHEARD`: the cost; valid if `has_cost`.
    pub cost: i32,
    /// `HEARD`, `UNHEARD`: the listener's range.
    pub range: i32,
    /// `LINE`: where it stopped; `HEARD`: the cell; valid if `has_cell`
    /// (offset on hex).
    pub cell: [i32; 3],
    /// `LINE`, `GAIN`: the clarity.
    pub clarity: u16,
    /// `GAIN`: the distance factor.
    pub distance: u16,
    /// `GAIN`: the light (thermal: heat) factor.
    pub light: u16,
    /// `GAIN`: the crowd factor.
    pub crowd: u16,
    /// `DOUBT`: the doubt now.
    pub doubt: u16,
    /// `GAIN`: the band index (255: scanner).
    pub band: u8,
    /// `INSTANT`: the cue rule index.
    pub rule: u8,
    /// `STAGE`: the stage before.
    pub from: u8,
    /// `STAGE`: the new (or waiting) stage.
    pub to: u8,
    /// `LINE`: 1 if sight got through.
    pub clear: u8,
    /// `STAGE`: 1 if the rise waits for the reaction time.
    pub pending: u8,
    /// 1 if `cell` is set.
    pub has_cell: u8,
    /// 1 if `cost` is set.
    pub has_cost: u8,
}

impl SxPerceptionTrace {
    /// The record of a perception trace; with `layout` (hex) its cell
    /// becomes offset.
    pub fn of(event: PerceptionTrace, layout: Option<HexLayout>) -> Self {
        let c: PerceptionTraceCode = convert::perception_trace(event);
        Self {
            kind: c.kind,
            target: c.target,
            why: c.why,
            sense: c.sense,
            channel: c.channel,
            gain: c.gain,
            cost: c.cost.unwrap_or(0),
            range: c.range,
            cell: c
                .cell
                .map(|p| to_offset(layout, Cell::from(p)).pos())
                .unwrap_or_default(),
            clarity: c.clarity,
            distance: c.distance,
            light: c.light,
            crowd: c.crowd,
            doubt: c.doubt,
            band: c.band,
            rule: c.rule,
            from: c.from,
            to: c.to,
            clear: u8::from(c.clear),
            pending: u8::from(c.pending),
            has_cell: u8::from(c.cell.is_some()),
            has_cost: u8::from(c.cost.is_some()),
        }
    }
}

/// `sx_perception_trace_fn`.
pub type PerceptionTraceFn = unsafe extern "C" fn(*mut c_void, *const SxPerceptionTrace);

// ---- the handle ----------------------------------------------------------------

/// `sx_perception`: the world, the game's sense map and the trace.
pub struct SxPerception {
    world: World,
    layout: Option<HexLayout>,
    map: CallbackSenseMap,
    trace: TraceSlot<SxPerceptionTrace>,
    percepts: Vec<Percept>,
    flood: Vec<(Cell, i32)>,
}

/// The handle behind `p`, or `ERROR_ARGUMENT`.
macro_rules! handle {
    ($p:expr) => {
        // SAFETY: the caller passes null or a valid handle (sagax.h).
        match unsafe { $p.as_mut() } {
            Some(h) => h,
            None => return ERROR_ARGUMENT,
        }
    };
}

impl SxPerception {
    /// Writes the collected percepts to `out` (at most `cap`) and their
    /// number to `total`; returns how many were written.
    ///
    /// # Safety
    /// `out` has room for `cap` percepts (null allowed for 0); `total` is
    /// null or writable.
    unsafe fn write(&self, out: *mut SxPercept, cap: u32, total: *mut u32) -> i32 {
        let n = self.percepts.len().min(cap as usize);
        for (i, p) in self.percepts[..n].iter().enumerate() {
            // SAFETY: within `cap` (see above).
            unsafe { out.add(i).write(SxPercept::of(p, self.layout)) };
        }
        // SAFETY: see above.
        unsafe {
            put(
                total,
                u32::try_from(self.percepts.len()).unwrap_or(u32::MAX),
            )
        };
        to_i32(n)
    }
}

/// `sx_perception_config_default`: a config for `grid` (`SX_GRID_*`) and
/// `cell_size`, every channel with full damping, no fuzz, at once,
/// `SX_SPREAD_CIRCLE`. Returns 0, or `SX_ERROR_ARGUMENT` for an unknown
/// grid, a cell size below 2 or a null `out`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_config_default(
    grid: u32,
    cell_size: i32,
    out: *mut SxPerceptionConfig,
) -> i32 {
    if convert::grid_layout(grid).is_none() || cell_size < 2 || out.is_null() {
        return ERROR_ARGUMENT;
    }
    let cfg = SxPerceptionConfig {
        grid,
        cell_size,
        channels: [SxChannel::of(&Channel::default()); MAX_CHANNELS],
    };
    // SAFETY: not null and writable (see above).
    unsafe { put(out, cfg) };
    0
}

/// `sx_perception_new`: null for a null or invalid config. `map` null: no
/// map (clear, lit, silent).
///
/// # Safety
/// `cfg` is null or valid; `map` is null or valid, its callbacks and
/// context for as long as it is set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_new(
    cfg: *const SxPerceptionConfig,
    map: *const SxSenseMap,
) -> *mut SxPerception {
    create(|| {
        // SAFETY: null or valid (see above).
        let (world, layout) = unsafe { cfg.as_ref() }?.world()?;
        // SAFETY: null or valid (see above).
        let map = unsafe { map.as_ref() }.copied().unwrap_or_default();
        Some(SxPerception {
            world,
            layout,
            map: CallbackSenseMap { map, layout },
            trace: None,
            percepts: Vec::with_capacity(64),
            flood: Vec::new(),
        })
    })
}

/// `sx_perception_free`.
///
/// # Safety
/// `p` is null or from [`sx_perception_new`] and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_free(p: *mut SxPerception) {
    // SAFETY: see above.
    unsafe { destroy(p) }
}

/// `sx_perception_set_map`: replaces the sense map (`map` null: no map).
///
/// # Safety
/// `p` is null or valid; `map` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_set_map(
    p: *mut SxPerception,
    map: *const SxSenseMap,
) -> i32 {
    let h = handle!(p);
    // SAFETY: null or valid (see above).
    h.map.map = unsafe { map.as_ref() }.copied().unwrap_or_default();
    0
}

/// `sx_perception_set_trace`.
///
/// # Safety
/// `p` is null or valid; `trace` and `context` stay valid while tracing is
/// on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_set_trace(
    p: *mut SxPerception,
    trace: Option<PerceptionTraceFn>,
    context: *mut c_void,
) -> i32 {
    // SAFETY: see above.
    set_trace(unsafe { p.as_mut() }.map(|h| &mut h.trace), trace, context)
}

/// `sx_sense_profile_preset`: `SX_SENSE_PRESET_*` for `cell_size` fine
/// units per cell. Returns 0, or `SX_ERROR_ARGUMENT`.
///
/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_sense_profile_preset(
    preset: u32,
    cell_size: i32,
    out: *mut SxSenseProfile,
) -> i32 {
    let Some(profile) = convert::sense_preset(preset, cell_size) else {
        return ERROR_ARGUMENT;
    };
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: not null and writable (see above).
    unsafe { put(out, SxSenseProfile::of(&profile)) };
    0
}

/// `sx_perception_cell_centre`: the fine point at the middle of `cell`
/// (offset on hex) into `out` (3 integers, axial on hex).
///
/// # Safety
/// `p` is null or valid; `out` is null or has room for 3 integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_cell_centre(
    p: *mut SxPerception,
    cell: Point,
    out: *mut i32,
) -> i32 {
    let h = handle!(p);
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    let point = cell_centre(&h.world, to_axial(h.layout, cell.cell()));
    // SAFETY: not null; room for 3 integers (see above).
    unsafe { out.cast::<[i32; 3]>().write_unaligned(point) };
    0
}

/// `sx_perception_cell_of`: the whole cell (offset on hex) holding the fine
/// point `point` (3 integers, axial on hex).
///
/// # Safety
/// `p` is null or valid; `point` is null or holds 3 integers; `out` is
/// null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_cell_of(
    p: *mut SxPerception,
    point: *const i32,
    out: *mut Point,
) -> i32 {
    let h = handle!(p);
    if point.is_null() || out.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: not null; 3 integers (see above).
    let point = unsafe { point.cast::<[i32; 3]>().read_unaligned() };
    let cell = to_offset(h.layout, cell_of(&h.world, point));
    // SAFETY: not null and writable.
    unsafe { put(out, Point::of(cell)) };
    0
}

/// `sx_hex_heading`: the heading (0..=255) of hex direction `dir` (taken
/// modulo 6; 0 = +q, 1 = +r).
#[unsafe(no_mangle)]
pub extern "C" fn sx_hex_heading(dir: u32) -> i32 {
    i32::from(hex_heading((dir % 6) as u8))
}

/// Reads a fine point of 3 integers, `None` for null.
///
/// # Safety
/// `p` is null or holds 3 integers.
unsafe fn point_at(p: *const i32) -> Option<Pos> {
    // SAFETY: see above.
    (!p.is_null()).then(|| unsafe { p.cast::<[i32; 3]>().read_unaligned() })
}

/// `sx_perception_line`: whether sight gets from `from` to `to` (fine
/// points) and how clearly.
///
/// # Safety
/// `p` is null or valid; `from`, `to` are null or hold 3 integers; `out`
/// is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_line(
    p: *mut SxPerception,
    from: *const i32,
    to: *const i32,
    out: *mut SxLine,
) -> i32 {
    let h = handle!(p);
    // SAFETY: see above.
    let (Some(from), Some(to)) = (unsafe { point_at(from) }, unsafe { point_at(to) }) else {
        return ERROR_ARGUMENT;
    };
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        let line = sight_line(&h.world, &mut h.map, from, to);
        let blocked = line.blocked_at.map(|c| to_offset(h.layout, c));
        let value = SxLine {
            clear: u8::from(line.clear),
            has_blocked_at: u8::from(blocked.is_some()),
            clarity: line.clarity,
            blocked_at: blocked.map(Cell::pos).unwrap_or_default(),
        };
        // SAFETY: not null and writable (see above).
        unsafe { put(out, value) };
        0
    })
}

/// `sx_perception_in_sight`: 1 if `eye` could see `pos` now (range, band,
/// line), else 0.
///
/// # Safety
/// `p` is null or valid; `eye` is null or valid; `pos` is null or holds 3
/// integers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_perception_in_sight(
    p: *mut SxPerception,
    eye: *const SxEye,
    pos: *const i32,
) -> i32 {
    let h = handle!(p);
    let Some(eye) = EyeParts::of(eye) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: see above.
    let Some(pos) = (unsafe { point_at(pos) }) else {
        return ERROR_ARGUMENT;
    };
    guarded(|| i32::from(in_sight(&h.world, &mut h.map, &eye.eye(), pos)))
}

/// `cap` entries at `out` are writable, or `cap` is 0.
fn room<T>(out: *mut T, cap: u32) -> bool {
    cap == 0 || !out.is_null()
}

/// `sx_perception_observe`: one check of one observer–target pair.
/// Returns the number of percepts written (at most `cap`, a check makes at
/// most `SX_MAX_PERCEPTS_PER_CHECK`), `*total` how many there were.
///
/// # Safety
/// `p`, `eye`, `target`, `aware` are null or valid; `out` has room for
/// `cap` percepts (null allowed for 0); `total` is null or writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_perception_observe(
    p: *mut SxPerception,
    now: u32,
    eye: *const SxEye,
    target: *const SxTarget,
    aware: *mut SxAwareness,
    out: *mut SxPercept,
    cap: u32,
    total: *mut u32,
) -> i32 {
    let h = handle!(p);
    let Some(eye) = EyeParts::of(eye) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: null or valid (see above).
    let (Some(target), Some(aware)) = (unsafe { target.as_ref() }, unsafe { aware.as_mut() })
    else {
        return ERROR_ARGUMENT;
    };
    if !room(out, cap) {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        h.percepts.clear();
        let layout = h.layout;
        let eye = eye.eye();
        traced!(h.trace, |e| SxPerceptionTrace::of(e, layout), |sink| {
            observe_traced(
                &h.world,
                &mut h.map,
                now,
                &eye,
                target,
                aware,
                &mut h.percepts,
                sink,
            )
        });
        // SAFETY: see above.
        unsafe { h.write(out, cap, total) }
    })
}

/// `sx_perception_look`: one observer looks at `count` targets (`aware`
/// parallel). Returns the number of percepts written, `*total` how many
/// there were.
///
/// # Safety
/// `p`, `eye`, `attention`, `budget` are null or valid; `targets` and
/// `aware` hold `count` entries (null allowed for 0); `out` has room for
/// `cap` percepts (null allowed for 0); `total` is null or writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_perception_look(
    p: *mut SxPerception,
    now: u32,
    eye: *const SxEye,
    targets: *const SxTarget,
    aware: *mut SxAwareness,
    count: u32,
    attention: *mut SxAttention,
    budget: *const SxBudget,
    out: *mut SxPercept,
    cap: u32,
    total: *mut u32,
) -> i32 {
    let h = handle!(p);
    let Some(eye) = EyeParts::of(eye) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: null or valid (see above).
    let (Some(attention), Some(budget)) =
        (unsafe { attention.as_mut() }, unsafe { budget.as_ref() })
    else {
        return ERROR_ARGUMENT;
    };
    if !room(out, cap) || (count > 0 && (targets.is_null() || aware.is_null())) {
        return ERROR_ARGUMENT;
    }
    if count == 0 {
        // SAFETY: see above.
        unsafe { put(total, 0) };
        return 0;
    }
    // SAFETY: both hold `count` entries (see above).
    let targets = unsafe { slice_of(targets, count as usize) };
    // SAFETY: as above; the game does not alias the two arrays.
    let aware = unsafe { std::slice::from_raw_parts_mut(aware, count as usize) };
    guarded(|| {
        h.percepts.clear();
        let layout = h.layout;
        let eye = eye.eye();
        traced!(h.trace, |e| SxPerceptionTrace::of(e, layout), |sink| {
            look_traced(
                &h.world,
                &mut h.map,
                now,
                &eye,
                targets,
                aware,
                attention,
                *budget,
                &mut h.percepts,
                sink,
            )
        });
        // SAFETY: see above.
        unsafe { h.write(out, cap, total) }
    })
}

/// `sx_perception_hear`: one hearing check of one listener. Returns the
/// number of percepts written (a heard stimulus gives at least the `HEARD`
/// one, at most 3), `*total` how many there were (0: not heard).
///
/// # Safety
/// `p`, `ear`, `s` are null or valid; `aware` is null or valid; `out` has
/// room for `cap` percepts (null allowed for 0); `total` is null or
/// writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_perception_hear(
    p: *mut SxPerception,
    now: u32,
    ear: *const SxEye,
    s: *const SxStimulus,
    has_cost: u8,
    cost: i32,
    aware: *mut SxAwareness,
    out: *mut SxPercept,
    cap: u32,
    total: *mut u32,
    seed: u64,
) -> i32 {
    let h = handle!(p);
    let Some(ear) = EyeParts::of(ear) else {
        return ERROR_ARGUMENT;
    };
    // SAFETY: null or valid (see above).
    let Some(s) = (unsafe { s.as_ref() }) else {
        return ERROR_ARGUMENT;
    };
    if !room(out, cap) {
        return ERROR_ARGUMENT;
    }
    // SAFETY: null or valid (see above).
    let aware = unsafe { aware.as_mut() };
    let cost = (has_cost != 0).then_some(cost);
    guarded(|| {
        h.percepts.clear();
        let layout = h.layout;
        let ear = ear.eye();
        traced!(h.trace, |e| SxPerceptionTrace::of(e, layout), |sink| {
            hear_traced(
                &h.world,
                &mut h.map,
                now,
                &ear,
                s,
                cost,
                aware,
                &mut h.percepts,
                seed,
                sink,
            )
        });
        // SAFETY: see above.
        unsafe { h.write(out, cap, total) }
    })
}

/// `sx_perception_flood`: every cell `s` reaches around corners, with its
/// cost in fine units, by one search of `nav` over the sense map's sound.
/// Writes at most `cap` cells (offset on hex) and costs (`costs` may be
/// null); returns how many, `*total` how many there were. `nav`'s grid
/// must be the handle's (else `SX_ERROR_ARGUMENT`), its straight step cost
/// the cell size. Replaces the navigator's last search.
///
/// # Safety
/// `p`, `nav`, `s` are null or valid; `cells` and `costs` have room for
/// `cap` entries (null allowed for 0; `costs` may be null); `total` is null
/// or writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_perception_flood(
    p: *mut SxPerception,
    nav: *mut SxNav,
    s: *const SxStimulus,
    cells: *mut Point,
    costs: *mut i32,
    cap: u32,
    total: *mut u32,
) -> i32 {
    let h = handle!(p);
    // SAFETY: null or valid (see above).
    let (Some(nav), Some(s)) = (unsafe { nav.as_mut() }, unsafe { s.as_ref() }) else {
        return ERROR_ARGUMENT;
    };
    if !room(cells, cap) {
        return ERROR_ARGUMENT;
    }
    let nav_layout = match nav.nav.topology() {
        Topology::Hex { layout, .. } => Some(layout),
        Topology::Square(_) => None,
    };
    if nav_layout != h.layout {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        let budget = sagax_nav::Budget::UNLIMITED;
        flood_sound(&mut nav.nav, &h.world, &mut h.map, s, budget, &mut h.flood);
        nav.begun = Begun::Nothing;
        let n = h.flood.len().min(cap as usize);
        for (i, &(cell, cost)) in h.flood[..n].iter().enumerate() {
            // SAFETY: within `cap` (see above).
            unsafe {
                cells.add(i).write(Point::of(to_offset(h.layout, cell)));
                if !costs.is_null() {
                    costs.add(i).write(cost);
                }
            }
        }
        // SAFETY: see above.
        unsafe { put(total, u32::try_from(h.flood.len()).unwrap_or(u32::MAX)) };
        to_i32(n)
    })
}

/// `sx_count_crowds`: for each of `count` targets, how many others of the
/// same group (`groups[i]`, parallel; null: all one group) stand within
/// `radius` fine units on the same level, into `out[i]` (saturating at
/// 255). Returns 0.
///
/// # Safety
/// `targets` and `out` hold `count` entries, `groups` is null or holds
/// `count` (null allowed for 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_count_crowds(
    targets: *const SxTarget,
    count: u32,
    radius: i32,
    groups: *const u32,
    out: *mut u8,
) -> i32 {
    if count > 0 && (targets.is_null() || out.is_null()) {
        return ERROR_ARGUMENT;
    }
    let n = count as usize;
    // SAFETY: `count` entries each (see above).
    let targets = unsafe { slice_of(targets, n) };
    let out = if n == 0 {
        &mut [][..]
    } else {
        // SAFETY: see above.
        unsafe { std::slice::from_raw_parts_mut(out, n) }
    };
    let groups = if groups.is_null() {
        None
    } else {
        // SAFETY: see above.
        Some(unsafe { slice_of(groups, n) })
    };
    // `count_crowds` hands out references into `targets`: the index of a
    // target is its offset from the start.
    let base = targets.as_ptr();
    count_crowds(
        targets,
        radius,
        |t| {
            let i = (std::ptr::from_ref(t) as usize - base as usize) / size_of::<SxTarget>();
            groups.map_or(0, |g| g[i])
        },
        out,
    );
    0
}

/// `sx_percept_fact`: the fact a percept stands for into `out` (its pos is
/// the percept's cell, offset on hex). Returns 1 if written, 0 if the
/// percept stands for none, `SX_ERROR_ARGUMENT` for a null pointer or an
/// unknown sense or event.
///
/// # Safety
/// `pc`, `k` are null or valid; `out` is null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_percept_fact(
    pc: *const SxPercept,
    k: *const SxFactKinds,
    out: *mut SxFact,
) -> i32 {
    // SAFETY: null or valid (see above).
    let (Some(pc), Some(k)) = (unsafe { pc.as_ref() }, unsafe { k.as_ref() }) else {
        return ERROR_ARGUMENT;
    };
    let Some(p) = pc.percept() else {
        return ERROR_ARGUMENT;
    };
    if out.is_null() {
        return ERROR_ARGUMENT;
    }
    match percept_fact(&p, &k.kinds()) {
        Some(fact) => {
            // SAFETY: not null and writable (see above).
            unsafe { put(out, fact) };
            1
        }
        None => 0,
    }
}

/// `sx_learn_percepts`: the common use of `count` percepts (the rules of
/// `learn_percepts`). Returns 0, or `SX_ERROR_ARGUMENT` for a null pointer
/// or a percept with an unknown sense or event (then nothing is changed).
///
/// # Safety
/// `known`, `alert`, `cfg`, `k`, `a` are null or valid; `pc` holds `count`
/// percepts (null allowed for 0).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn sx_learn_percepts(
    known: *mut SxKnowledge,
    alert: *mut SxAlertness,
    cfg: *const SxAlertConfig,
    now: u32,
    pc: *const SxPercept,
    count: u32,
    k: *const SxFactKinds,
    a: *const SxAlertAmounts,
) -> i32 {
    let (known, trace) = held!(known);
    let alert = get!(alert);
    let cfg = input!(cfg);
    let kinds = input!(k).kinds();
    let amounts = input!(a).amounts();
    if count > 0 && pc.is_null() {
        return ERROR_ARGUMENT;
    }
    // SAFETY: `count` percepts (see above).
    let pc = unsafe { slice_of(pc, count as usize) };
    if pc.iter().any(|p| p.percept().is_none()) {
        return ERROR_ARGUMENT;
    }
    guarded(|| {
        for p in pc.iter().filter_map(SxPercept::percept) {
            traced!(trace, SxKnowledgeTrace::of, |sink| learn_percepts_traced(
                known,
                alert,
                cfg,
                now,
                &[p],
                &kinds,
                &amounts,
                sink
            ));
        }
        0
    })
}

/// `sx_percept_alert`: the alertness stimulus of a percept (0 for none, a
/// null pointer or an unknown event).
///
/// # Safety
/// `pc` and `a` are null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_percept_alert(pc: *const SxPercept, a: *const SxAlertAmounts) -> u32 {
    // SAFETY: null or valid (see above).
    let (Some(pc), Some(a)) = (unsafe { pc.as_ref() }, unsafe { a.as_ref() }) else {
        return 0;
    };
    pc.percept().map_or(0, |p| alert_amount(&p, &a.amounts()))
}

// ---- awareness and attention --------------------------------------------------

/// `sx_awareness_stage`: 0 unseen, 1 noticed, 2 recognised.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_awareness_stage(a: *const SxAwareness) -> i32 {
    i32::from(input!(a).stage())
}

/// `sx_awareness_counter`: the evidence counter.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_awareness_counter(a: *const SxAwareness) -> i32 {
    i32::from(input!(a).counter())
}

/// `sx_awareness_in_view`: 1 if the target was in view at the last check.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_awareness_in_view(a: *const SxAwareness) -> i32 {
    i32::from(input!(a).in_view())
}

/// `sx_awareness_last_seen`: the time of the last check that saw it.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_awareness_last_seen(a: *const SxAwareness) -> i64 {
    // SAFETY: null or valid (see above).
    match unsafe { a.as_ref() } {
        Some(a) => i64::from(a.last_seen()),
        None => i64::from(ERROR_ARGUMENT),
    }
}

/// `sx_awareness_seen_through`: 1 if this observer saw through the
/// disguise.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_awareness_seen_through(a: *const SxAwareness) -> i32 {
    i32::from(input!(a).seen_through())
}

/// `sx_awareness_reset`: back to zero.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_awareness_reset(a: *mut SxAwareness) -> i32 {
    get!(a).reset();
    0
}

/// `sx_attention_cursor`: where the next round robin starts.
///
/// # Safety
/// `a` is null or valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sx_attention_cursor(a: *const SxAttention) -> i64 {
    // SAFETY: null or valid (see above).
    match unsafe { a.as_ref() } {
        Some(a) => i64::from(a.cursor()),
        None => i64::from(ERROR_ARGUMENT),
    }
}
