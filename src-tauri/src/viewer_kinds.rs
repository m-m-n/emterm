//! CLI-shared single source of truth for the replayable viewer kinds.
//!
//! Originally lived in `viewer/mod.rs`, but that module is gated on
//! `feature = "gui"` (it pulls in wry). The mux snapshot rich-content
//! stripper (`crate::mux::scrollback_filter::strip_replayable_rich_content`)
//! must reach this constant under `--features mux` alone (no GUI), so the
//! SSOT is hoisted here and the original `viewer::REPLAYABLE_VIEWER_KINDS`
//! path is preserved via a `pub use` re-export inside `viewer/mod.rs`.
//!
//! Consumers:
//! - `crate::viewer` (GUI-only): re-exports this constant; `ViewerRouter::route`
//!   dispatches on these kinds, kept in lockstep by the `drift_*` test in
//!   `viewer/mod.rs`.
//! - `crate::mux::scrollback_filter` (mux-gated): strips OSC 777 launch
//!   sequences for exactly these kinds from a reattach snapshot so they are
//!   not re-launched.

/// The viewer kinds that an OSC 777 `emterm` launch sequence can dispatch
/// to a child viewer (Markdown / image / JSON / YAML / HTML). See the
/// module-level doc for the drift contract with the dispatch and the
/// stripper.
pub const REPLAYABLE_VIEWER_KINDS: &[&str] = &["markdown", "image", "json", "yaml", "html"];
