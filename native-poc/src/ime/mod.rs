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
//! Phase 4-G adds the `backend` + `null` (+ OS-specific) modules that
//! implement platform IME clients on top of tao 0.34. The Phase 4-E
//! routing layer (`preedit` + `commit`) is unchanged; backends simply
//! funnel events into `App::on_ime_{preedit,commit,focus_lost}`.

pub mod commit;
pub mod preedit;

// Phase 4-G-A: backend trait + NullBackend (passthrough).
pub mod backend;
pub mod null;

// Phase 4-G-B / 4-G-C / 4-G-D will add OS-specific submodules:
//   #[cfg(all(unix, not(target_os = "macos")))] pub mod x11;
//   #[cfg(all(unix, not(target_os = "macos")))] pub mod wayland;
//   #[cfg(windows)] pub mod windows;
