//! IME preedit + commit routing (Phase 4-E).
//!
//! `preedit::State` tracks the IME composition string that the renderer
//! draws as an underline overlay under the active cursor cell. `commit`
//! writes the finalized composition to the active PTY exactly once.
//!
//! Both directions share the [`preedit::sanitize`] helper so the displayed
//! text and the bytes pushed to the PTY agree on which C0 / C1 controls
//! are dropped.
//!
//! Phase 4-E auto-scope only covers the routing + rendering layer. The
//! actual platform IME path (Linux fcitx5 via tao, Windows MS-IME via
//! egui's IME hooks) is already wired up by Phase 1 — this module hangs
//! state off the events the platform already delivers.

pub mod commit;
pub mod preedit;
