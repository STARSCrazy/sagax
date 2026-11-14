//! The names most games need: `use sagax::prelude::*;`.
//!
//! Builders such as `add_consideration` answer `false` for an unknown
//! option, and the compiler warns when that answer is thrown away:
//!
//! ```compile_fail
//! #![deny(unused_must_use)]
//! use sagax::prelude::*;
//! let mut d = Decider::new();
//! let o = d.add_option(ONE);
//! let c = Consideration::new(0, Curve::Linear, 0, 10);
//! d.add_consideration(o, c);
//! ```

pub use sagax_core::{Cell, HexLayout, Pos};
pub use sagax_decide::group::Status as SquadStatus;
pub use sagax_decide::group::{
    BuiltInRules, Crew, CrewConfig, CrewEvent, Goal, GoalKeeper, KeeperInputs, Member, MemberState,
    Spot, Squad, SquadConfig, SquadRules, SquadView, Tactic, tactics,
};
pub use sagax_decide::memory::{Areas, Evict, Fact, Knowledge, Message, Metric, PointPattern};
pub use sagax_decide::mission::{
    Action, BreakReason, Detour, DetourEnd, Event, Fail, Leash, Leeway, Mission, MissionConfig,
    MissionRules, Motive, OnLost, Plan, Step, Where,
};
pub use sagax_decide::{Candidate, Choice, Consideration, Curve, DecideError, Decider, ONE, Pick};
pub use sagax_nav::Budget;
pub use sagax_nav::grid::{CellMap, DenseGrid, GridRules, Preset};
pub use sagax_nav::nav::Status as NavStatus;
pub use sagax_nav::nav::{ANY_LEVEL, Navigator, Output, Report};
pub use sagax_nav::topology::Topology;
