//! `tmux.conf` parsing and conversion for the one-shot importer
//! ([`crate::mux::tmux_import`]).
//!
//! Ported from `src-tauri/src/mux/tmux_conf/`. native-poc applies the
//! conversion result onto `settings.json` as a JSON patch via
//! `settings_store::save_patch_to`, instead of going through the
//! `AppSettings` round-trip path the WebView importer uses.

pub mod converter;
pub mod parser;
