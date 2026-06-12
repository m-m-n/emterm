//! Test module for `commands::config`.
//!
//! Tests are grouped by responsibility into submodules. The shared imports
//! are re-exported here so each submodule can `use super::*;`.

#![cfg(test)]

pub(super) use super::settings::{
    AppSettings, KeybindSettings, MuxSettings, Profile, SshConnection, SshOption,
    StatusbarCustomCommand,
};
pub(super) use super::types::*;
pub(super) use super::validation::validate_settings;

mod color_schemes;
mod defaults;
mod deserialization;
mod font_family_migration;
mod language;
mod markdown;
mod notification;
mod profile;
mod serialization;
mod show_tab_bar;
mod ssh;
mod statusbar;
mod ui_font_family;
mod ui_theme_preset;
mod validation_tests;
