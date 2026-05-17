//! Cross-thread wakeup for the winit event loop.
//!
//! Background threads (PTY readers, future fcitx5 watchers, …) that
//! enqueue work for the main thread need a way to ensure winit observes
//! the change before its next idle expiry. Without a wakeup, winit's
//! `ControlFlow::WaitUntil` on Wayland frequently delays `about_to_wait`
//! beyond the requested deadline, so a PTY echo of a freshly-committed
//! IME glyph sits in the channel until the user types again.
//!
//! `window_host::run` installs an `EventLoopProxy`-backed wake function
//! at startup; producers call `wake()` after pushing work onto a
//! channel the main thread polls. A `OnceLock` is used because the
//! event loop is built exactly once per process.
use std::sync::OnceLock;

type WakeFn = Box<dyn Fn() + Send + Sync>;

static WAKE: OnceLock<WakeFn> = OnceLock::new();

/// Install the global wake function. Idempotent: subsequent calls are
/// ignored (`OnceLock::set` returns `Err` and is dropped).
pub fn install(f: WakeFn) {
    let _ = WAKE.set(f);
}

/// Signal the main event loop to wake up. No-op when no wake function
/// has been installed yet (unit tests, headless harnesses).
pub fn wake() {
    if let Some(f) = WAKE.get() {
        f();
    }
}
