// Phase 4-C (APC redesign): native-poc speaks the mux protocol *only* via
// APC sequences embedded in the PTY output. The legacy `emterm mux` CLI is
// the bridge to the daemon's Unix socket; native-poc never opens that
// socket itself. See `doc/tasks/mux-inband-protocol/SPEC.md`.
//
// Layout:
//
// - [`apc`]    — payload decoder for `ESC _ emterm-mux;<base64> ESC \`.
// - [`prefix`] — prefix-key state machine (default `Ctrl+B`). Follow-up
//                keys are translated to PTY writes (`Ctrl+B d` etc.) so the
//                bridge CLI sees the same byte sequences a tmux user would
//                type. native-poc does not encode mux control frames itself.
//
// The `prefix::Latch` API (`is_armed`, `observe`, `cancel`, `PrefixAction`,
// follow-up decoder) is **forward-staged**: the Phase 4-B keybinds dispatch
// does not yet call into it (the user types `Ctrl+B d` and those bytes are
// forwarded to the bridge CLI via the normal PTY write path, exactly like
// in legacy tmux). The state machine is exercised exclusively by the
// `TS-prefix-*` unit tests today, and will be wired through `keybinds` once
// a future sub-phase adds intercept points for `prefix d` (detach), `prefix
// n` (next window), etc. We `allow(dead_code)` at the module root rather
// than scattering attributes on each item so the intent is in one place.
#![allow(dead_code)]

pub mod apc;
pub mod dialog;
pub mod prefix;
pub mod window_group;
