//! Deterministic backend and scenarios. No terminal or real device I/O.
mod backend;
mod check;
mod scenario;

pub use backend::{SimResources, Simulator};
pub use check::{CheckReport, check};
pub use scenario::Scenario;
