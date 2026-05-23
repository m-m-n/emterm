//! Built-in variable providers for the status-bar template engine.
//!
//! Each provider implements
//! [`crate::status_bar::template_engine::VariableProvider`]. IO-bound
//! providers (Git, custom commands) own a dedicated worker thread —
//! the UI thread reads cached values without blocking.

pub mod command;
pub mod cwd;
pub mod git_branch;
pub mod time;
pub mod worker;

pub use command::CommandProvider;
pub use cwd::{CwdProvider, CwdSource};
pub use git_branch::GitBranchProvider;
pub use time::TimeProvider;

// Re-exports retained for tests that exercise the helpers directly
// (e.g. `basename` unit tests in `cwd`). Marked `#[allow(unused)]`
// so the bin target stays warning-clean while keeping them
// available.
#[allow(unused_imports)]
pub use command::is_valid_command_name;
#[allow(unused_imports)]
pub use cwd::basename;
#[allow(unused_imports)]
pub use git_branch::BranchStatus;
#[allow(unused_imports)]
pub use time::format_with as format_time_with;
