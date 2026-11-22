//! TAKO headless engineering engine (pre-alpha).
//!
//! Drives one mission: plan approval -> isolated execution -> verification
//! evidence -> human accept/reject. Unix only. See `docs/architecture/`.

#[cfg(not(unix))]
compile_error!("tako-engine currently supports Unix-like systems only");

pub mod engine;
pub mod error;
pub mod git;
pub mod model;
pub mod runner;

pub use engine::{AgentKind, AgentSpec, Engine};
pub use error::{EngineError, Result};
pub use model::*;
