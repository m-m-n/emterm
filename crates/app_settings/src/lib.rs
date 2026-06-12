//! Shared eMterm settings schema.
//!
//! `AppSettings` (and its nested types) is the single source of truth for
//! the on-disk `settings.json` format. It was extracted from
//! `src-tauri/src/commands/config/` so the native build's child settings
//! window can load/save the file with the exact same serde defaults and
//! null-tolerant deserialization as the Tauri build. `src-tauri` re-exports
//! these modules from its `commands::config` path, so existing imports keep
//! working unchanged.

pub mod settings;
pub mod types;

pub use settings::*;
pub use types::*;
