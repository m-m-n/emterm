#[cfg(feature = "gui")]
pub mod io;
pub mod settings;
pub mod types;
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
