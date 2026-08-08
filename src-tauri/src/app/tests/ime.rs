use super::*;

// ── Phase 4-E: IME preedit/commit routing ────────────────────────

#[test]
fn ime_preedit_no_active_tab_is_noop() {
    let mut app = App::new();
    // No spawn → no tabs. Must not panic.
    app.on_ime_preedit("abc");
}

#[test]
fn ime_preedit_updates_active_tab_state() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.on_ime_preedit("hi");
    let tab = app.active_tab().unwrap();
    assert!(tab.preedit_state.active());
    assert_eq!(tab.preedit_state.text(), "hi");
}

#[test]
fn ime_preedit_anchors_to_current_cursor() {
    let mut app = App::new();
    app.spawn_initial_tab();
    // Move cursor to (row=2, col=3) via CSI CUP.
    {
        let tab = app.active_tab().unwrap();
        tab.core.lock().process_pty_data(b"\x1b[3;4H");
    }
    app.on_ime_preedit("xy");
    let tab = app.active_tab().unwrap();
    let a = tab.preedit_state.anchor();
    assert_eq!(a.row, 2);
    assert_eq!(a.col, 3);
}

#[test]
fn ime_preedit_sanitizes_control_bytes() {
    let mut app = App::new();
    app.spawn_initial_tab();
    // ESC must NOT survive into the preedit overlay text.
    app.on_ime_preedit("a\x1bb");
    assert_eq!(app.active_tab().unwrap().preedit_state.text(), "ab");
}

#[test]
fn ime_commit_clears_preedit_state() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.on_ime_preedit("abc");
    assert!(app.active_tab().unwrap().preedit_state.active());
    app.on_ime_commit("abc");
    assert!(!app.active_tab().unwrap().preedit_state.active());
}

#[test]
fn ime_commit_no_active_tab_is_noop() {
    let mut app = App::new();
    app.on_ime_commit("abc");
}

#[test]
fn ime_focus_lost_clears_preedit() {
    let mut app = App::new();
    app.spawn_initial_tab();
    app.on_ime_preedit("xy");
    assert!(app.active_tab().unwrap().preedit_state.active());
    app.on_ime_focus_lost();
    assert!(!app.active_tab().unwrap().preedit_state.active());
}

#[test]
fn ime_preedit_requests_full_redraw() {
    let mut app = App::new();
    app.spawn_initial_tab();
    // Clear the initial full-redraw flag so we can observe the
    // routing-time mutation.
    {
        let arc = app.active_tab().unwrap().core.clone();
        let mut core = arc.lock();
        app.record_render_state(&mut core);
    }
    assert!(!app.needs_full_redraw);
    app.on_ime_preedit("ab");
    assert!(app.needs_full_redraw);
}

#[test]
fn ime_commit_requests_full_redraw() {
    let mut app = App::new();
    app.spawn_initial_tab();
    {
        let arc = app.active_tab().unwrap().core.clone();
        let mut core = arc.lock();
        app.record_render_state(&mut core);
    }
    assert!(!app.needs_full_redraw);
    app.on_ime_commit("a");
    assert!(app.needs_full_redraw);
}

// ── Phase 4-G-A: ImeBackend wiring on App ────────────────────────

use crate::ime::backend::testing::{MockBackend, MockState};
use crate::ime::backend::{ImeEvent, KeyDispatchResult, RawKeyEvent};
use crate::pty::input::Modifiers;
use std::sync::{Arc, Mutex};

fn mock_app() -> (App, Arc<Mutex<MockState>>) {
    let mut app = App::new();
    let (mock, state) = MockBackend::new();
    app.set_ime_backend(Box::new(mock));
    (app, state)
}

fn raw(pressed: bool) -> RawKeyEvent {
    RawKeyEvent {
        physical_key_code: 0x26,
        state_pressed: pressed,
        mods: Modifiers::NONE,
    }
}

// ── TS-backend-3: pump_ime routes events to on_ime_* ──────────

#[test]
fn pump_ime_routes_preedit_to_active_tab() {
    let (mut app, state) = mock_app();
    app.spawn_initial_tab();
    state
        .lock()
        .unwrap()
        .queue
        .push(ImeEvent::Preedit("hi".into()));
    let routed = app.pump_ime();
    assert!(routed);
    assert_eq!(app.active_tab().unwrap().preedit_state.text(), "hi");
}

#[test]
fn pump_ime_routes_commit_clears_preedit() {
    let (mut app, state) = mock_app();
    app.spawn_initial_tab();
    // Stage a preedit so commit has something to clear.
    app.on_ime_preedit("ab");
    assert!(app.active_tab().unwrap().preedit_state.active());
    state
        .lock()
        .unwrap()
        .queue
        .push(ImeEvent::Commit("ab".into()));
    app.pump_ime();
    assert!(!app.active_tab().unwrap().preedit_state.active());
}

#[test]
fn pump_ime_routes_focus_out_clears_preedit() {
    let (mut app, state) = mock_app();
    app.spawn_initial_tab();
    app.on_ime_preedit("xy");
    assert!(app.active_tab().unwrap().preedit_state.active());
    state.lock().unwrap().queue.push(ImeEvent::FocusOut);
    app.pump_ime();
    assert!(!app.active_tab().unwrap().preedit_state.active());
}

#[test]
fn pump_ime_with_empty_queue_returns_false() {
    let (mut app, _state) = mock_app();
    app.spawn_initial_tab();
    let routed = app.pump_ime();
    assert!(!routed);
}

// ── TS-backend-4: Consumed result skips tao_key_to_bytes ──────

#[test]
fn dispatch_consumed_does_not_invoke_pty_path() {
    // We assert on the dispatch result; the App caller (window_host)
    // is the one that branches. Here we pin the contract that the
    // App's helper returns the backend's result verbatim.
    let (mut app, state) = mock_app();
    state.lock().unwrap().next_dispatch = KeyDispatchResult::Consumed;
    let r = app.dispatch_key_event_via_ime(&raw(true));
    assert_eq!(r, KeyDispatchResult::Consumed);
}

// ── TS-backend-5: Passthrough lets caller run encoder path ─────

#[test]
fn dispatch_passthrough_returns_passthrough() {
    // Default MockBackend state returns Passthrough.
    let (mut app, _state) = mock_app();
    let r = app.dispatch_key_event_via_ime(&raw(true));
    assert_eq!(r, KeyDispatchResult::Passthrough);
}

// ── TS-cursor-1: notify_cursor_rect rate-limited on cell change

#[test]
fn notify_cursor_rect_fires_once_per_cell_change() {
    let (mut app, state) = mock_app();
    app.spawn_initial_tab();
    // First call: cell (0,0) — should record one notification.
    app.notify_cursor_rect_if_changed(9, 18, 0, 28);
    assert_eq!(state.lock().unwrap().cursor_calls.len(), 1);
    // Second call without cursor movement: must NOT fire again.
    app.notify_cursor_rect_if_changed(9, 18, 0, 28);
    assert_eq!(state.lock().unwrap().cursor_calls.len(), 1);
    // Move the cursor → next call must fire.
    {
        let tab = app.active_tab().unwrap();
        tab.core.lock().process_pty_data(b"\x1b[5;3H");
    }
    app.notify_cursor_rect_if_changed(9, 18, 0, 28);
    assert_eq!(state.lock().unwrap().cursor_calls.len(), 2);
}

#[test]
fn notify_cursor_rect_uses_pixel_size_from_args() {
    let (mut app, state) = mock_app();
    app.spawn_initial_tab();
    {
        let tab = app.active_tab().unwrap();
        tab.core.lock().process_pty_data(b"\x1b[3;4H"); // (row=2, col=3)
    }
    app.notify_cursor_rect_if_changed(9, 18, 4, 32);
    let calls = &state.lock().unwrap().cursor_calls;
    assert_eq!(calls.len(), 1);
    // x = col * cell_w + origin_x = 3 * 9 + 4 = 31.
    // y = row * cell_h + origin_y = 2 * 18 + 32 = 68.
    assert_eq!(calls[0].0, 31);
    assert_eq!(calls[0].1, 68);
    assert_eq!(calls[0].2, 9);
    assert_eq!(calls[0].3, 18);
}

#[test]
fn notify_cursor_rect_with_no_active_tab_is_noop() {
    let (mut app, state) = mock_app();
    // No spawn → no tabs.
    app.notify_cursor_rect_if_changed(9, 18, 0, 28);
    assert!(state.lock().unwrap().cursor_calls.is_empty());
}

// ── TS-focus-1: notify_ime_focus + on_ime_focus_lost wiring ─────

#[test]
fn notify_ime_focus_propagates_to_backend() {
    let (mut app, state) = mock_app();
    app.notify_ime_focus(true);
    app.notify_ime_focus(false);
    assert_eq!(state.lock().unwrap().focus_calls, vec![true, false]);
}

// ── TS-route-1 (regression of Phase 4-E): Preedit via pump → sanitize

#[test]
fn pump_ime_preedit_with_esc_is_sanitized_via_phase4e_layer() {
    let (mut app, state) = mock_app();
    app.spawn_initial_tab();
    state
        .lock()
        .unwrap()
        .queue
        .push(ImeEvent::Preedit("a\x1bb".into()));
    app.pump_ime();
    // Phase 4-E sanitize must strip ESC (0x1b).
    assert_eq!(app.active_tab().unwrap().preedit_state.text(), "ab");
}

// ── TS-route-2 (regression): Commit via pump → sanitize + no
//    bracketed-paste wrap. We can't easily inspect the real
//    PtySession bytes here, but the route is identical to the
//    direct `on_ime_commit` path which `ime::commit::tests`
//    already pins. The piece we *can* verify is that the pump
//    drops into `on_ime_commit` (preedit clears) and never panics
//    on control bytes.
#[test]
fn pump_ime_commit_with_esc_does_not_panic_and_clears_overlay() {
    let (mut app, state) = mock_app();
    app.spawn_initial_tab();
    app.on_ime_preedit("draft");
    state
        .lock()
        .unwrap()
        .queue
        .push(ImeEvent::Commit("a\x1bb".into()));
    app.pump_ime();
    assert!(!app.active_tab().unwrap().preedit_state.active());
}

// ── set_ime_backend updates ime_is_null flag ────────────────────

#[test]
fn default_app_holds_null_backend() {
    let app = App::new();
    assert!(app.ime_is_null());
}

#[test]
fn set_ime_backend_to_mock_clears_is_null_flag() {
    let mut app = App::new();
    assert!(app.ime_is_null());
    let (mock, _) = MockBackend::new();
    app.set_ime_backend(Box::new(mock));
    assert!(!app.ime_is_null());
}
