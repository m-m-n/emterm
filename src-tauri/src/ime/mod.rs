//! IME preedit + commit routing (Phase 4-E) + native IME backends (Phase 4-G).
//!
//! `preedit::State` tracks the IME composition string that the renderer
//! draws as an underline overlay under the active cursor cell. `commit`
//! writes the finalized composition to the active PTY exactly once.
//!
//! Both directions share the [`preedit::sanitize`] helper so the displayed
//! text and the bytes pushed to the PTY agree on which C0 / C1 controls
//! are dropped.
//!
//! Phase 4-G adds the `backend` + `null` modules that implement a
//! platform-neutral seam between [`crate::app::App`] and OS IME clients.
//! After the 2026-05-14 redesign, the only real platform backend is the
//! winit-driven `winit_bridge` (added in Phase 4-G-3); the previous
//! `x11` / `wayland` / `windows` modules were removed because tao 0.34
//! does not expose XKB keycodes, which broke self-built XIM.

pub mod commit;
pub mod preedit;

// Phase 4-G-A: backend trait + NullBackend (passthrough).
pub mod backend;
pub mod null;

// Phase 4-G-3: winit IME bridge (X11 / Wayland / Windows via winit 0.31).
pub mod winit_bridge;
