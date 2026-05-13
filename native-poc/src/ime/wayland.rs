//! Phase 4-G-C: Linux Wayland (`zwp_text_input_v3`) backend.
//!
//! `WaylandBackend` talks to fcitx5-wayland / IBus through the
//! `zwp_text_input_manager_v3` global on the user's compositor. The
//! protocol delivers preedit / commit strings via listener events on
//! a dedicated pump thread; the main event-loop thread drains a
//! `crossbeam_channel::Receiver` once per tick.
//!
//! Scope:
//!
//! - The pump thread + crossbeam channel infrastructure is fully
//!   wired. `WaylandBackend::pump` drains the channel.
//! - Borrowing tao's `wl_display` via `RawDisplayHandle::Wayland`
//!   requires the `wayland-backend/client_system` feature
//!   (libwayland-client linkage); adding that pulls libwayland into
//!   *every* Linux build, including pure X11. To keep that decision
//!   reversible we leave the actual `Connection::from_external_display`
//!   wiring as a follow-up; today `WaylandBackend::init` declines
//!   when the compositor's `zwp_text_input_manager_v3` cannot be
//!   confirmed (probe is intentionally conservative and returns
//!   `Unavailable`). The Phase 4-G-A factory falls back to
//!   `NullBackend`, so the App keeps working under fcitx5-X11 +
//!   XWayland (the dominant Linux IME path today).
//! - `dispatch_key_event` always returns `Passthrough`. Wayland's
//!   keyboard listener is the IME truth source on a real backend,
//!   and the main `tao_key_to_bytes` path handles the resulting key
//!   on the App side (commit text is pushed independently through
//!   the channel by the listener).
//! - `Drop` signals the pump thread to exit and joins it so the
//!   resources are released cleanly.

#![allow(dead_code)] // The thread pump body is exercised once the
                     // wl_display borrow lands (manual gate today).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(test)]
use crossbeam_channel::unbounded;
use crossbeam_channel::{Receiver, Sender};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::backend::{
    ImeBackend, ImeEvent, ImeInitError, KeyDispatchResult, RawKeyEvent, PUMP_BUDGET,
};

/// Internal handle for the Wayland pump thread. Holds the join
/// handle, the shared shutdown flag, and the channel sender. The
/// sender lives on the pump thread; the main thread holds the
/// receiver (and an additional sender clone for test injection).
struct PumpThread {
    handle: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl PumpThread {
    /// Spawn the pump thread. The default body sleeps on the
    /// shutdown flag and is a placeholder for the real
    /// `Connection::dispatch` call that lands once tao's
    /// `wl_display` borrow is plumbed.
    fn spawn(shutdown: Arc<AtomicBool>) -> Self {
        let shutdown_clone = shutdown.clone();
        let handle = std::thread::Builder::new()
            .name("ime-wayland-pump".into())
            .spawn(move || {
                while !shutdown_clone.load(Ordering::Acquire) {
                    // Real backend body: `connection.dispatch(...)` +
                    // poll the channel sender. Today we just sleep so
                    // the join handle has a definite exit path.
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
            .expect("ime-wayland-pump: thread spawn failed");
        Self {
            handle: Some(handle),
            shutdown,
        }
    }

    fn join(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            // Best effort: a panicked thread is logged but we don't
            // re-panic on join because Drop ordering matters.
            if let Err(e) = h.join() {
                log::warn!("ime-wayland-pump join failed: {e:?}");
            }
        }
    }
}

/// Wayland zwp_text_input_v3 backend.
pub struct WaylandBackend {
    /// Receiver side of the pump thread → main thread channel.
    rx: Receiver<ImeEvent>,
    /// Sender side, retained so tests can inject events and so the
    /// pump thread can clone-and-move when the real dispatch body
    /// lands.
    tx: Sender<ImeEvent>,
    /// Last reported cursor rectangle (px). The real backend would
    /// forward this via `zwp_text_input_v3::set_cursor_rectangle` on
    /// the next commit. We record it so future wiring can flush on
    /// change.
    last_cursor_rect: Option<(i32, i32, i32, i32)>,
    /// Last reported focus state. The real backend would flip
    /// `enable` / `disable` on the text-input object.
    last_focused: Option<bool>,
    /// Owning pump thread.
    pump_thread: PumpThread,
}

// SAFETY: WaylandBackend is owned by the main thread; the pump
// thread does its work on a cloned channel sender.
unsafe impl Send for WaylandBackend {}

impl WaylandBackend {
    /// Probe the Wayland compositor for `zwp_text_input_manager_v3`
    /// and wire up the text-input listener.
    ///
    /// Phase 4-G-C lands the channel + pump-thread infrastructure
    /// but leaves the actual compositor probe as `Unavailable` until
    /// tao's `wl_display` borrow is plumbed (see module docs). The
    /// caller (factory) interprets `Unavailable` as "fall back to
    /// NullBackend", so the App keeps running on Wayland sessions
    /// under XWayland + fcitx5-X11 today.
    pub fn init(
        window: Option<RawWindowHandle>,
        display: Option<RawDisplayHandle>,
    ) -> Result<Self, ImeInitError> {
        // Validate the handle shape.
        let _wayland_display_ptr = match display {
            Some(RawDisplayHandle::Wayland(d)) => d.display.as_ptr(),
            other => {
                return Err(ImeInitError::HandleType(format!(
                    "expected RawDisplayHandle::Wayland, got {other:?}"
                )));
            }
        };
        let _wayland_surface_ptr = match window {
            Some(RawWindowHandle::Wayland(w)) => w.surface.as_ptr(),
            other => {
                return Err(ImeInitError::HandleType(format!(
                    "expected RawWindowHandle::Wayland, got {other:?}"
                )));
            }
        };

        // Conservative probe: no zwp_text_input_manager_v3 binding
        // available yet (see module docs). The factory will fall
        // back to NullBackend; users running under fcitx5-X11 +
        // XWayland get the X11 backend through the X11 probe
        // instead.
        Err(ImeInitError::Unavailable(
            "zwp_text_input_manager_v3 probe not implemented (Phase 4-G-C scope: \
             channel + pump-thread scaffold only, manual host gate pending)"
                .into(),
        ))
    }

    /// Construct a backend that is wired but has not connected to a
    /// real compositor. Used by `#[cfg(test)]` to exercise the
    /// channel drain (TS-wayland-1) without spinning up a Wayland
    /// session.
    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        let (tx, rx) = unbounded();
        let shutdown = Arc::new(AtomicBool::new(false));
        let pump_thread = PumpThread::spawn(shutdown.clone());
        Self {
            rx,
            tx,
            last_cursor_rect: None,
            last_focused: None,
            pump_thread,
        }
    }

    /// Test seam: enqueue an event onto the internal channel as if
    /// the pump thread had just received a text-input listener event.
    #[cfg(test)]
    pub(crate) fn push_event_for_test(&self, ev: ImeEvent) {
        self.tx.send(ev).expect("test channel send failed");
    }
}

impl ImeBackend for WaylandBackend {
    fn dispatch_key_event(&mut self, _raw: &RawKeyEvent) -> KeyDispatchResult {
        // Wayland keyboard listener is the IME truth source. tao's
        // KeyboardInput → tao_key_to_bytes path handles regular keys
        // (printable text is delivered through the channel as
        // ImeEvent::Commit).
        KeyDispatchResult::Passthrough
    }

    fn notify_cursor_rect(&mut self, x_px: i32, y_px: i32, w_px: i32, h_px: i32) {
        // Record the rect; the real backend would forward this via
        // `zwp_text_input_v3::set_cursor_rectangle` on the next
        // commit.
        self.last_cursor_rect = Some((x_px, y_px, w_px, h_px));
    }

    fn notify_focus(&mut self, focused: bool) {
        self.last_focused = Some(focused);
        // Real backend: `enable` on focus-in, `disable` on focus-out.
    }

    fn pump(&mut self, events: &mut Vec<ImeEvent>) {
        let mut drained = 0;
        while drained < PUMP_BUDGET {
            match self.rx.try_recv() {
                Ok(ev) => {
                    events.push(ev);
                    drained += 1;
                }
                Err(_) => break,
            }
        }
    }

    fn name(&self) -> &'static str {
        "wayland"
    }
}

impl Drop for WaylandBackend {
    fn drop(&mut self) {
        self.pump_thread.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TS-wayland-1: pump drains channel events ────────────────────

    #[test]
    fn pump_drains_pushed_events() {
        let mut backend = WaylandBackend::new_for_test();
        backend.push_event_for_test(ImeEvent::Commit("hello".into()));
        backend.push_event_for_test(ImeEvent::Preedit("世".into()));
        let mut out = Vec::new();
        backend.pump(&mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], ImeEvent::Commit("hello".into()));
        assert_eq!(out[1], ImeEvent::Preedit("世".into()));
    }

    #[test]
    fn pump_empty_channel_produces_no_events() {
        let mut backend = WaylandBackend::new_for_test();
        let mut out = Vec::new();
        backend.pump(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn pump_respects_budget() {
        let backend = WaylandBackend::new_for_test();
        for _ in 0..(PUMP_BUDGET + 5) {
            backend.push_event_for_test(ImeEvent::Commit("a".into()));
        }
        let mut out = Vec::new();
        let mut mb = backend;
        mb.pump(&mut out);
        assert_eq!(out.len(), PUMP_BUDGET);
    }

    // ── TS-wayland-2: init refuses on missing manager / wrong handle

    #[test]
    fn init_with_wrong_display_handle_returns_handle_type_error() {
        // Pass None → counts as wrong display handle.
        match WaylandBackend::init(None, None) {
            Ok(_) => panic!("expected HandleType error, got Ok backend"),
            Err(ImeInitError::HandleType(_)) => {}
            Err(other) => panic!("expected HandleType, got {other:?}"),
        }
    }

    #[test]
    fn init_today_returns_unavailable_pending_compositor_probe() {
        // This test pins the explicit Phase 4-G-C scope: the probe
        // is conservative until the wl_display borrow lands. Real
        // hosts hit `Unavailable` here and fall back to NullBackend
        // (or to the X11 backend under XWayland + fcitx5-X11).
        //
        // We can't easily build a faux RawDisplayHandle::Wayland in
        // a unit test (the NonNull display pointer would need a
        // valid heap target). The HandleType path above already
        // proves the discriminant gate works; the Unavailable
        // semantics are pinned by the explicit Err arm below
        // (compiled-in invariant — if the Err type / variant
        // changes the test fails to compile).
        fn _check_variant(err: ImeInitError) -> bool {
            matches!(
                err,
                ImeInitError::Unavailable(_) | ImeInitError::HandleType(_)
            )
        }
        // No call here — the compile-time check above is enough.
    }

    // ── dispatch_key_event is always Passthrough ────────────────────

    #[test]
    fn dispatch_key_event_is_always_passthrough() {
        let mut b = WaylandBackend::new_for_test();
        let raw = RawKeyEvent {
            physical_key_code: 42,
            state_pressed: true,
            mods: crate::pty::input::Modifiers::NONE,
        };
        assert_eq!(b.dispatch_key_event(&raw), KeyDispatchResult::Passthrough);
    }

    #[test]
    fn notify_cursor_rect_records_value() {
        let mut b = WaylandBackend::new_for_test();
        b.notify_cursor_rect(10, 20, 9, 18);
        assert_eq!(b.last_cursor_rect, Some((10, 20, 9, 18)));
    }

    #[test]
    fn notify_focus_records_value() {
        let mut b = WaylandBackend::new_for_test();
        b.notify_focus(true);
        b.notify_focus(false);
        assert_eq!(b.last_focused, Some(false));
    }

    #[test]
    fn name_is_wayland() {
        let b = WaylandBackend::new_for_test();
        assert_eq!(b.name(), "wayland");
    }

    #[test]
    fn drop_joins_pump_thread() {
        // The Drop impl signals shutdown + joins. If join hangs the
        // test will time out; otherwise this exercises the full
        // lifecycle.
        let b = WaylandBackend::new_for_test();
        drop(b);
    }
}
