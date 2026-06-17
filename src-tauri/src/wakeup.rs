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
//!
//! Status-bar providers receive a [`WakeFn`] handle via their
//! constructors so they can:
//! - call it from a self-owned timer / worker thread when a cached
//!   value updates;
//! - be swapped for a test double (counting closure) under
//!   `#[cfg(test)]`.
//!
//! Production code constructs a [`WakeFn`] via [`shared_wake_fn`]
//! which forwards each invocation to the global [`wake`].
use std::sync::{Arc, OnceLock};

type InstalledWakeFn = Box<dyn Fn() + Send + Sync>;

/// Shareable wake handle.
///
/// Cloning is cheap (`Arc::clone`). Each registered status-bar
/// provider holds its own clone and invokes it when its cached value
/// changes so the winit event loop schedules the next frame even if no
/// PTY / keyboard / mouse input is active.
pub type WakeFn = Arc<dyn Fn() + Send + Sync>;

static WAKE: OnceLock<InstalledWakeFn> = OnceLock::new();

/// Install the global wake function. Idempotent: subsequent calls are
/// ignored (`OnceLock::set` returns `Err` and is dropped).
pub fn install(f: InstalledWakeFn) {
    let _ = WAKE.set(f);
}

/// Signal the main event loop to wake up. No-op when no wake function
/// has been installed yet (unit tests, headless harnesses).
pub fn wake() {
    if let Some(f) = WAKE.get() {
        f();
    }
}

/// Build a [`WakeFn`] that forwards each call to the global
/// [`wake()`]. Used by `App::new` to hand a clone-friendly handle to
/// the status-bar runtime; the runtime then injects one clone per
/// provider via their constructors.
///
/// Calling the returned closure before [`install`] has run is a no-op
/// (matches the existing semantics of [`wake`]).
pub fn shared_wake_fn() -> WakeFn {
    Arc::new(|| wake())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn shared_wake_fn_is_invokable_even_before_install() {
        // `install` is process-wide and may already be set by another
        // test; this test asserts only that calling the returned
        // closure is safe.
        let f = shared_wake_fn();
        f();
    }

    #[test]
    fn wake_fn_arc_clones_invoke_same_target() {
        // A custom WakeFn (test double) should fan-out to every clone.
        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = counter.clone();
        let f: WakeFn = Arc::new(move || {
            c2.fetch_add(1, Ordering::Relaxed);
        });
        let g = f.clone();
        f();
        g();
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }
}
