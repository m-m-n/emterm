//! Status-bar runtime: template engine, providers, OSC dispatcher,
//! and the per-frame view model. Phase 4-D's clock-only widget is
//! superseded by the 3-row layout assembled here.
//!
//! Submodules land across implementation phases:
//! - Phase C: `template_engine`, `providers/`
//! - Phase D: `osc_dispatcher`
//! - Phase E: `runtime`, `view_model`

pub mod osc_dispatcher;
pub mod providers;
pub mod runtime;
pub mod template_engine;
pub mod view_model;

pub use runtime::StatusBarRuntime;
pub use view_model::{AppRow, OscRow, StatusBarViewModel};
