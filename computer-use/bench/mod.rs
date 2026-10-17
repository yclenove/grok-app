//! Shared performance contracts. xtask includes the same budget/report sources
//! directly, keeping the checker independent of the evaluation runner and drivers.
//! Nothing here starts a desktop or generates synthetic native measurements.

pub mod budgets;
pub mod collector;
pub mod report;
