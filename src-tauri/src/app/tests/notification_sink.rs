use super::*;
use crate::callbacks::NotifyRustSink;
use std::any::TypeId;

// ── test-notify-dbus-isolation task0001: the test-build App's sink ──
//
// In the lib's test build every `App` (built through `App::new()` or
// `App::with_settings`) holds a notification sink that does nothing, so no
// App-constructing test can reach notify-rust (D-Bus on Linux, the toast
// API on Windows). These tests pin that. They never construct a
// `NotifyRustSink` themselves: construction alone starts its worker
// thread, and the sink is identified through its type identity instead.

/// AC-1 / TS-1: `App::new()` does not hold a `NotifyRustSink` in the test
/// build. Fails when `App::with_settings` builds `NotifyRustSink::new()`
/// there.
#[test]
fn ac1_ts1_app_new_notification_sink_is_not_notify_rust_sink() {
    let app = App::new();

    let held = app.notification_sink.sink_type_id();

    assert_ne!(
        held,
        TypeId::of::<NotifyRustSink>(),
        "the test-build App must hold the no-op sink, not the production NotifyRustSink"
    );
}

/// AC-2 / TS-2: `App::notify` on `App::new()` returns without panicking,
/// and the sink it reaches is the no-op sink, whose `send` has no side
/// effect to deliver (no worker thread, no notify-rust call, no log
/// record).
#[test]
fn ac2_ts2_app_notify_on_app_new_returns_without_panicking() {
    let app = App::new();

    assert_eq!(
        app.notification_sink.sink_type_id(),
        TypeId::of::<NoopNotificationSink>(),
        "App::notify must reach the no-op sink in the test build"
    );
    app.notify("t", "b");
}
