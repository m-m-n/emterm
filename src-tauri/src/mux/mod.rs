// Phase 4-C (APC redesign): native-poc speaks the mux protocol *only* via
// APC sequences embedded in the PTY output. The legacy `emterm mux` CLI is
// the bridge to the daemon's Unix socket; native-poc never opens that
// socket itself. See `doc/tasks/mux-inband-protocol/SPEC.md`.
//
// Layout:
//
// - [`apc`]    — payload decoder for `ESC _ emterm-mux;<base64> ESC \`.
// - [`prefix`] — prefix-key state machine (default `Ctrl+Z`).
//
// The `prefix::Latch` API (`is_armed`, `observe`, `cancel`, `PrefixAction`,
// follow-up decoder) is wired live: `App::observe_mux_key` (see
// `window_host.rs`) feeds every keystroke on a mux-attached tab through
// `Latch::observe`. The latch consumes the prefix chord and its follow-up
// keys and turns them into `PrefixAction`s (Detach, NextWindow, PrevWindow,
// SelectWindow, NewWindow, RenameWindow, MoveWindow) that the app dispatches
// as mux actions — the follow-up keys are NOT forwarded to the PTY as raw
// bytes. Only the double-prefix Literal path writes the prefix byte itself.
// Some sibling items (legacy bridge/CLI byte paths, APC encode helpers) remain
// unused by native-poc, so we `allow(dead_code)` at the module root rather than
// scattering attributes on each item, keeping the intent in one place.
#![allow(dead_code)]

pub mod apc;
pub mod bridge;
pub mod cli;
pub mod daemon;
pub mod dialog;
pub mod ipc;
pub mod prefix;
pub mod scrollback_buffer;
pub mod scrollback_filter;
pub mod session;
pub mod snapshot;
pub mod tmux_conf;
pub mod tmux_import;
pub mod window_group;
