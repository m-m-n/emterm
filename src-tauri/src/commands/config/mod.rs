#[cfg(feature = "gui")]
pub mod io;
// The settings schema lives in the shared `app_settings` crate (also used
// by native-poc's child settings window); re-export the modules here so
// `crate::commands::config::settings::…` / `…::types::…` paths keep working.
pub use app_settings::{settings, types};
#[cfg(any(feature = "gui", test))]
mod validation;

#[cfg(test)]
mod tests;

// Re-export main types for external use
#[cfg(feature = "gui")]
pub use io::{load_settings, save_settings};
pub use settings::{
    AppSettings, KeybindSettings, MuxSettings, MuxStatusbarCommand, MuxStatusbarSettings, Profile,
    SshConnection, SshOption, StatusbarCustomCommand,
};
pub use types::*;
