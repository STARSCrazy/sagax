//! Mission planner: a motive, a plan of steps, detours and replanning.

mod rules;
mod runner;
mod types;

pub use rules::{BuiltInRules, FailCtx, MissionRules, PlanCtx};
pub use runner::{
    DEFAULT_PREFERENCE, DIST_CAP, IN_DIST, IN_EXTRA, IN_FEASIBLE, IN_KNOWN, IN_PREF, IN_WIDEN,
    KNOWN_CAP, Mission, Output, WIDEN_PREFERENCE,
};
pub use types::*;

#[cfg(test)]
mod tests;
