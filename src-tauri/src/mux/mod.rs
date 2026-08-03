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
// mux-daemon-binary-update-detect task0002: minimal stand-in for the
// binary-update identity check (see identity.rs module docs). Unix only,
// matching `inherited_pty` / `upgrade` below — the real module's
// rename-replacement / stat semantics have no Windows equivalent.
// task0001 owns the real implementation; this file is expected to be
// replaced wholesale at merge (IMPLEMENTATION.md D2).
#[cfg(unix)]
pub mod identity;
// task0002 (mux daemon hot-upgrade): raw-descriptor → `MasterPty` adapter
// for descriptors inherited across a process replacement. Unix only —
// process replacement (`execve`) and the descriptor semantics it depends on
// (raw fds, `dup`, `fcntl`) have no Windows equivalent.
#[cfg(unix)]
pub mod inherited_pty;
pub mod ipc;
pub mod prefix;
pub mod scrollback_buffer;
pub mod scrollback_filter;
pub mod session;
pub mod snapshot;
pub mod snapshot_bytes;
pub mod tmux_conf;
// `tmux_import` writes to `crate::settings_store` (GUI-only), so the submodule
// itself is GUI-only. Its sole caller (`main.rs:run_gui`) is already gated on
// `feature = "gui"`.
#[cfg(feature = "gui")]
pub mod tmux_import;
// task0003 (mux daemon hot-upgrade): snapshot / restore of the live session
// tree to and from the handoff document, and the handoff file's lifetime.
// Unix only, matching `inherited_pty` — descriptor inheritance across
// `execve` has no Windows equivalent.
#[cfg(unix)]
pub mod upgrade;
pub mod window_group;
