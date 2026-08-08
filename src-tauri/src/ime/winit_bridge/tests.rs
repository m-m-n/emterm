use std::sync::Mutex;

use super::*;
use crate::pty::input::Modifiers;

/// Mock [`BridgeWindow`] that records every call so tests can
/// assert dedup behaviour.
#[derive(Default)]
struct MockWindow {
    inner: Mutex<MockWindowInner>,
}

#[derive(Default, Debug, Clone)]
struct MockWindowInner {
    allowed_calls: Vec<bool>,
    cursor_calls: Vec<(i32, i32, i32, i32)>,
    /// Interleaved record of which kind fired, in call order —
    /// `allowed_calls` / `cursor_calls` alone lose the relative
    /// ordering between the two kinds (AC-4 / SPEC TS-3).
    call_order: Vec<CallKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallKind {
    Allowed,
    Cursor,
}

impl BridgeWindow for MockWindow {
    fn set_ime_allowed(&self, allowed: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.allowed_calls.push(allowed);
        inner.call_order.push(CallKind::Allowed);
    }
    fn set_ime_cursor_area(&self, x: i32, y: i32, w: i32, h: i32) {
        let mut inner = self.inner.lock().unwrap();
        inner.cursor_calls.push((x, y, w, h));
        inner.call_order.push(CallKind::Cursor);
    }
}

impl MockWindow {
    /// Clone the recorded state out from behind the lock and drop
    /// the guard immediately. Tests must assert against the
    /// snapshot, never against a `MutexGuard` held across
    /// `assert_eq!` — if the assertion panics while the guard is
    /// still alive the mutex is left poisoned, and `WinitImeBridge`'s
    /// `Drop` locking the same mutex during unwind turns one failed
    /// assertion into a full test-binary abort (a real failure mode
    /// hit while red-checking this suite; see task0001 report).
    fn snapshot(&self) -> MockWindowInner {
        self.inner.lock().unwrap().clone()
    }
    /// Reset the recorded state, e.g. to discharge the
    /// constructor's initial recorded enable before asserting on a
    /// later turn's calls.
    fn reset(&self) {
        *self.inner.lock().unwrap() = MockWindowInner::default();
    }
}

fn raw_press() -> RawKeyEvent {
    RawKeyEvent {
        physical_key_code: 0x26,
        state_pressed: true,
        mods: Modifiers::NONE,
    }
}

fn raw_release() -> RawKeyEvent {
    RawKeyEvent {
        physical_key_code: 0x26,
        state_pressed: false,
        mods: Modifiers::NONE,
    }
}

// We can't construct an `Arc<MockWindow>` and then turn it into a
// `Box<dyn BridgeWindow>` while still inspecting it from the test;
// share through `Arc<MockWindow>` and pass a thin shim.
struct SharedMock(Arc<MockWindow>);
impl BridgeWindow for SharedMock {
    fn set_ime_allowed(&self, allowed: bool) {
        self.0.set_ime_allowed(allowed);
    }
    fn set_ime_cursor_area(&self, x: i32, y: i32, w: i32, h: i32) {
        self.0.set_ime_cursor_area(x, y, w, h);
    }
}

fn make_bridge() -> (WinitImeBridge, Arc<MockWindow>) {
    let mock = Arc::new(MockWindow::default());
    let bridge = WinitImeBridge::with_handle(Box::new(SharedMock(mock.clone())));
    (bridge, mock)
}

// ── TS-winit-1: composition starts on first Preedit, gates key path
#[test]
fn preedit_starts_composition_and_dispatch_returns_consumed() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Preedit("に".into(), None));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Consumed
    );
}

// ── TS-winit-2: Preedit text rides through pump verbatim
#[test]
fn preedit_text_is_routed_through_pump() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("foo".into(), None));
    let mut out = Vec::new();
    b.pump(&mut out);
    assert_eq!(out, vec![ImeEvent::Preedit("foo".into())]);
}

// ── TS-winit-3: Commit ends composition + next dispatch is Passthrough
// Non-Windows only: the Windows gate is `ime_enabled`, which neither
// `Preedit` nor `Commit` changes here, so both dispatch assertions
// below only describe non-Windows semantics.
#[test]
#[cfg(not(windows))]
fn commit_clears_composition_and_unblocks_dispatch() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("に".into(), None));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Consumed
    );
    b.on_winit_ime(&WinitIme::Commit("日本".into()));
    // After commit the IM server has released the chord; the next
    // KeyboardInput must reach the PTY path.
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough
    );
    let mut out = Vec::new();
    b.pump(&mut out);
    // The queue ends with the Commit. Preedit("に") + Commit("日本").
    assert_eq!(
        out,
        vec![
            ImeEvent::Preedit("に".into()),
            ImeEvent::Commit("日本".into()),
        ]
    );
}

// ── TS-winit-4: Disabled produces FocusOut + clears composition
#[test]
fn disabled_produces_focus_out_and_clears_composition() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("ab".into(), None));
    b.on_winit_ime(&WinitIme::Disabled);
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough
    );
    let mut out = Vec::new();
    b.pump(&mut out);
    assert_eq!(
        out,
        vec![ImeEvent::Preedit("ab".into()), ImeEvent::FocusOut]
    );
}

// ── TS-winit-5: Commit → Disabled is idempotent (no double FocusOut clear)
#[test]
fn commit_then_disabled_is_idempotent() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
    b.on_winit_ime(&WinitIme::Commit("X".into()));
    // has_preedit is already false; a follow-up Disabled must
    // still surface FocusOut exactly once and leave both states
    // false.
    b.on_winit_ime(&WinitIme::Disabled);
    let mut out = Vec::new();
    b.pump(&mut out);
    assert_eq!(
        out,
        vec![
            ImeEvent::Preedit("x".into()),
            ImeEvent::Commit("X".into()),
            ImeEvent::FocusOut,
        ]
    );
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough
    );
}

// ── TS-winit-6: modifier-only release reaches dispatch as Passthrough
//               (Ghostty fcitx5 toggle scenario)
#[test]
fn modifier_release_passes_through_when_not_composing() {
    let (mut b, _mock) = make_bridge();
    // Composition off → release of any key (including modifier-only
    // synthetic ones the App may feed) returns Passthrough.
    assert_eq!(
        b.dispatch_key_event(&raw_release()),
        KeyDispatchResult::Passthrough
    );
}

// ── TS-winit-7 / AC-3 (SPEC TS-2): notify_cursor_rect dedups
//    identical rects; the same-rect dedup keeps working across a
//    flush boundary (each distinct rect reaches the window exactly
//    once, duplicates in between reach it zero times).
#[test]
fn cursor_rect_is_deduplicated_after_flush() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    b.notify_cursor_rect(10, 20, 9, 18);
    b.flush();
    b.notify_cursor_rect(10, 20, 9, 18); // exact same → not even recorded
    b.flush(); // → no-op, nothing pending
    b.notify_cursor_rect(11, 20, 9, 18); // moved → recorded, then flushed
    b.flush();
    let inner = mock.snapshot();
    assert_eq!(
        inner.cursor_calls,
        vec![(10, 20, 9, 18), (11, 20, 9, 18)],
        "duplicate calls must be suppressed"
    );
}

// ── AC-3 (SPEC TS-2): multiple notify_cursor_rect calls recorded
//    before a single flush coalesce into exactly one
//    set_ime_cursor_area call carrying the LAST recorded rect.
#[test]
fn cursor_rect_multiple_records_before_flush_coalesce_to_last() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    b.notify_cursor_rect(10, 20, 9, 18);
    b.notify_cursor_rect(10, 20, 9, 18); // dup → not recorded again
    b.notify_cursor_rect(11, 20, 9, 18); // moved → overwrites pending
    b.notify_cursor_rect(11, 20, 9, 18); // dup of the new rect → not recorded again
    b.flush();
    let inner = mock.snapshot();
    assert_eq!(
        inner.cursor_calls,
        vec![(11, 20, 9, 18)],
        "several pending updates before one flush must coalesce to a single call"
    );
}

// ── AC-7 (SPEC TS-6): construction-time enable is recorded, not
//    called immediately — the window sees nothing until the first
//    flush, then exactly `[true]`.
#[test]
fn init_records_allow_true_and_flushes_it_on_first_flush() {
    let (mut b, mock) = make_bridge();
    assert!(
        mock.snapshot().allowed_calls.is_empty(),
        "construction must only record the enable, not call the window yet"
    );
    b.flush();
    assert_eq!(mock.snapshot().allowed_calls, vec![true]);
}

// ── AC-2 (SPEC TS-1): notify_focus(true) records; no window call
//    happens until flush, and flush produces exactly one call.
#[test]
fn notify_focus_true_records_until_flush_then_calls_exactly_once() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    let before = mock.snapshot().allowed_calls.len();
    b.notify_focus(true);
    assert_eq!(
        mock.snapshot().allowed_calls.len(),
        before,
        "notify_focus must not reach the window before flush"
    );
    b.flush();
    assert_eq!(&mock.snapshot().allowed_calls[before..], &[true]);
}

#[test]
fn notify_focus_propagates_to_set_ime_allowed_after_flush() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    b.notify_focus(false);
    b.flush();
    b.notify_focus(true);
    b.flush();
    assert_eq!(mock.snapshot().allowed_calls, vec![true, false, true]);
}

// ── AC-4 (SPEC TS-3): allow-state and cursor-area recorded in the
//    same turn flush in order allow → cursor.
#[test]
fn flush_orders_allow_before_cursor_within_same_turn() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    mock.reset();
    b.notify_focus(true);
    b.notify_cursor_rect(5, 6, 7, 8);
    b.flush();
    assert_eq!(
        mock.snapshot().call_order,
        vec![CallKind::Allowed, CallKind::Cursor]
    );
}

// ── AC-5 (SPEC TS-4): a flush with nothing recorded makes no calls
//    at all, of either kind.
#[test]
fn flush_with_nothing_recorded_makes_no_calls() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    let before = mock.snapshot();
    b.flush(); // nothing new recorded since the previous flush
    let after = mock.snapshot();
    assert_eq!(after.allowed_calls.len(), before.allowed_calls.len());
    assert_eq!(after.cursor_calls.len(), before.cursor_calls.len());
}

// ── AC-6 (SPEC TS-5): Drop with a pending (never-flushed) allow
//    request discards it and calls set_ime_allowed(false) exactly
//    once — the constructor's recorded `true` never reaches the
//    window at all.
#[test]
fn drop_disables_ime() {
    let mock = Arc::new(MockWindow::default());
    {
        let _b = WinitImeBridge::with_handle(Box::new(SharedMock(mock.clone())));
        // drop happens at scope end; the constructor's pending
        // allow=true was never flushed.
    }
    assert_eq!(mock.snapshot().allowed_calls, vec![false]);
}

// ── AC-6 (SPEC TS-5): a pending cursor-area request left over at
//    Drop time is discarded too, not flushed.
#[test]
fn drop_discards_pending_cursor_rect() {
    let mock = Arc::new(MockWindow::default());
    {
        let mut b = WinitImeBridge::with_handle(Box::new(SharedMock(mock.clone())));
        b.notify_cursor_rect(1, 2, 3, 4); // recorded, never flushed
        drop(b);
    }
    let inner = mock.snapshot();
    assert_eq!(inner.allowed_calls, vec![false]);
    assert!(
        inner.cursor_calls.is_empty(),
        "pending cursor rect must be discarded, not flushed, on Drop"
    );
}

// ── task0001 (windows-imm32-ime-direct) "Deferred detach" ──────────
// Gated to Windows via `hold_pending_detach` (SPEC FR5 / AC6): see
// `hold_pending_detach_truth_table_is_total_over_all_combinations`
// below for the platform-selector proof that runs on every host.

// ── TS-hold: predicate truth table. Total over all eight
//    (allowed, composition_alive, windows_gate) combinations: for
//    windows_gate = true the answer is `!allowed && composition_alive`,
//    and for windows_gate = false the answer is always `false` — a
//    pending DISABLE is never held on non-Windows targets (SPEC FR5
//    / AC6 regression guard). Runs on every host.
#[test]
fn hold_pending_detach_truth_table_is_total_over_all_combinations() {
    for allowed in [false, true] {
        for composition_alive in [false, true] {
            assert_eq!(
                hold_pending_detach(allowed, composition_alive, true),
                !allowed && composition_alive,
                "windows_gate=true must answer exactly !allowed && composition_alive \
                 (allowed={allowed}, composition_alive={composition_alive})"
            );
            assert!(
                !hold_pending_detach(allowed, composition_alive, false),
                "windows_gate=false must never hold \
                 (allowed={allowed}, composition_alive={composition_alive})"
            );
        }
    }
}

// ── AC-1 (SPEC TS1, Windows target only): with a composition open,
//    focus loss followed by a flush delivers no detach; once
//    Disabled arrives, the next flush delivers the detach exactly
//    once, and a further flush delivers nothing.
#[test]
#[cfg(windows)]
fn held_detach_delivers_exactly_once_after_disabled() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    mock.reset();

    b.on_winit_ime(&WinitIme::Enabled);
    b.notify_focus(false);
    b.flush();
    assert!(
        mock.snapshot().allowed_calls.is_empty(),
        "a detach must be held (not delivered) while the composition is alive"
    );

    b.on_winit_ime(&WinitIme::Disabled);
    b.flush();
    assert_eq!(
        mock.snapshot().allowed_calls,
        vec![false],
        "the held detach must deliver exactly once after Disabled"
    );

    mock.reset();
    b.flush();
    assert!(
        mock.snapshot().allowed_calls.is_empty(),
        "a further flush must deliver nothing — delivery consumed the pending state"
    );
}

// ── SPEC FR5 / AC6 regression guard (non-Windows targets): with a
//    composition open, focus loss delivers the detach on the SAME
//    flush — the hold is Windows-only, so X11 / Wayland behavior is
//    unchanged by task0001.
#[test]
#[cfg(not(windows))]
fn detach_with_live_composition_delivers_immediately_on_non_windows() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    mock.reset();

    b.on_winit_ime(&WinitIme::Enabled);
    b.notify_focus(false);
    b.flush();
    assert_eq!(
        mock.snapshot().allowed_calls,
        vec![false],
        "non-Windows targets must never hold a pending detach (SPEC FR5 / AC6)"
    );
}

// ── AC-2 (SPEC TS2, Windows target only): a focus-in recorded while
//    a detach is held overwrites the pending allow-state
//    (last-writer-wins), so the detach is never delivered.
#[test]
#[cfg(windows)]
fn focus_in_during_held_detach_overwrites_pending_state_so_detach_never_delivers() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    mock.reset();

    b.on_winit_ime(&WinitIme::Enabled);
    b.notify_focus(false);
    b.flush();
    assert!(
        mock.snapshot().allowed_calls.is_empty(),
        "detach must still be held"
    );

    b.notify_focus(true); // overwrites pending_allow = Some(false)
    b.flush();
    assert_eq!(
        mock.snapshot().allowed_calls,
        vec![true],
        "the focus-in must win; the held detach must never reach the window"
    );
}

// ── AC-3 (FR3 regression guard): with no composition alive, focus
//    loss followed by a flush delivers the detach on that same
//    flush — current (non-composing) behavior is preserved on every
//    target, Windows included.
#[test]
fn detach_without_live_composition_delivers_on_same_flush() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    mock.reset();

    // No Enabled was ever observed, so composition_alive is false.
    b.notify_focus(false);
    b.flush();
    assert_eq!(mock.snapshot().allowed_calls, vec![false]);
}

// ── AC-5 (Windows target only): a held detach does not block
//    cursor-area delivery — with a composition alive, a pending
//    detach plus a pending cursor area flush as a cursor-area-only
//    delivery in that turn.
#[test]
#[cfg(windows)]
fn held_detach_does_not_block_pending_cursor_area_delivery() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    mock.reset();

    b.on_winit_ime(&WinitIme::Enabled);
    b.notify_focus(false);
    b.notify_cursor_rect(1, 2, 3, 4);
    b.flush();

    let inner = mock.snapshot();
    assert!(inner.allowed_calls.is_empty(), "the detach must stay held");
    assert_eq!(
        inner.cursor_calls,
        vec![(1, 2, 3, 4)],
        "the cursor area must still flush even while the detach is held"
    );
}

// ── Edge case (Test Notes, Windows target only): Enabled → Disabled
//    → focus loss → flush delivers the detach immediately, because
//    the composition was already closed before focus was lost.
#[test]
#[cfg(windows)]
fn focus_loss_after_composition_already_closed_delivers_detach_immediately() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    mock.reset();

    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Disabled);
    b.notify_focus(false);
    b.flush();
    assert_eq!(mock.snapshot().allowed_calls, vec![false]);
}

// ── Regression guard: an ENABLE recorded while a composition is
//    alive is never held — only a pending DISABLE can be held —
//    true on every target, since `hold_pending_detach` never even
//    considers `allowed == true`.
#[test]
fn pending_enable_is_never_held_even_with_live_composition() {
    let (mut b, mock) = make_bridge();
    b.flush(); // discharge the constructor's recorded allow=true
    mock.reset();

    b.on_winit_ime(&WinitIme::Enabled);
    b.notify_focus(true); // records allow=true, not a detach
    b.flush();
    assert_eq!(
        mock.snapshot().allowed_calls,
        vec![true],
        "an enable must never be held regardless of composition_alive"
    );
}

// ── AC-9 (SPEC TS-8): a second consecutive Enabled (no intervening
//    Disabled) triggers the anomaly latch on its second occurrence
//    only; the first Enabled from a fresh bridge is a normal
//    transition and must not warn.
#[test]
fn double_enabled_second_occurrence_warns_and_latches() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    assert!(
        !b.enabled_anomaly_warned,
        "first Enabled from a fresh bridge must not warn"
    );
    b.on_winit_ime(&WinitIme::Enabled);
    assert!(
        b.enabled_anomaly_warned,
        "second consecutive Enabled must warn and latch"
    );
    // Latch stays set on further repeats — no un-latching.
    b.on_winit_ime(&WinitIme::Enabled);
    assert!(b.enabled_anomaly_warned);
}

// ── AC-9 (SPEC TS-8): a second consecutive Disabled (no
//    intervening Enabled) triggers the anomaly latch on its second
//    occurrence only; the first Disabled that closes a real
//    composition is a normal transition and must not warn.
#[test]
fn double_disabled_second_occurrence_warns_and_latches() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Disabled);
    assert!(
        !b.disabled_anomaly_warned,
        "the Disabled that closes a real Enabled lifecycle must not warn"
    );
    b.on_winit_ime(&WinitIme::Disabled);
    assert!(
        b.disabled_anomaly_warned,
        "second consecutive Disabled must warn and latch"
    );
}

#[test]
fn empty_preedit_is_surfaced_for_overlay_clear() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    let mut out = Vec::new();
    b.pump(&mut out);
    assert_eq!(out, vec![ImeEvent::Preedit(String::new())]);
    // A bare empty preedit with no preceding Enable leaves both
    // states false: has_preedit stays false (no non-empty preedit
    // was ever observed) and ime_enabled stays false (no Enable
    // event fired). dispatch_key_event is Passthrough on every
    // target here, but for different reasons per platform —
    // non-Windows checks has_preedit, Windows checks ime_enabled,
    // and both happen to be false.
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough
    );
}

#[test]
fn name_is_winit() {
    let (b, _mock) = make_bridge();
    assert_eq!(b.name(), "winit");
}

// ── TS-1 (SKK regression, all targets): Enabled → Preedit("▽A") →
//    dispatch is Consumed on every target (Windows via the open
//    lifecycle, everywhere else via the non-empty preedit) →
//    Preedit("") → dispatch is Consumed on Windows (the lifecycle is
//    still open) and Passthrough elsewhere. Both preedits reach pump
//    in order regardless of platform (FR2, platform-independent).
#[test]
fn enabled_then_non_empty_then_empty_preedit_matches_ts1_on_every_target() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Preedit("▽A".into(), None));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Consumed,
        "dispatch must be consumed while the non-empty preedit is current, on every target"
    );
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    let expected_after_empty = if cfg!(windows) {
        KeyDispatchResult::Consumed
    } else {
        KeyDispatchResult::Passthrough
    };
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        expected_after_empty,
        "Windows keeps suppressing inside the still-open lifecycle; every other target passes through"
    );
    let mut out = Vec::new();
    b.pump(&mut out);
    assert_eq!(
        out,
        vec![
            ImeEvent::Preedit("▽A".into()),
            ImeEvent::Preedit(String::new()),
        ]
    );
}

// ── TS-5 (X11 ambiguous start/end shape, non-Windows only):
//    SPEC.md assumption A3 — winit-x11 emits an empty Ime::Preedit
//    for both composition start and composition end. This sequence
//    exercises that ambiguous shape end to end: passthrough while no
//    preedit has been observed yet, consumed once the preedit is
//    non-empty, passthrough again once it goes back to empty.
#[cfg(not(windows))]
#[test]
fn x11_ambiguous_empty_preedit_start_and_end_toggles_dispatch() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough,
        "leading empty preedit (X11 composition-start shape) must not suppress keys"
    );
    b.on_winit_ime(&WinitIme::Preedit("あ".into(), None));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Consumed,
        "non-empty preedit must suppress keys"
    );
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough,
        "trailing empty preedit (X11 composition-end shape) must unblock dispatch again"
    );
}

// ── AC-2: a subsequent non-empty preedit re-engages suppression on
//          non-Windows targets.
#[cfg(not(windows))]
#[test]
fn preedit_after_empty_reengages_dispatch_on_non_windows() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    b.on_winit_ime(&WinitIme::Preedit("y".into(), None));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Consumed
    );
}

// ── AC-3: Enable alone never causes Consumed on non-Windows
//          targets (Wayland/X11 fire it for the whole focus
//          lifetime, spanning ordinary direct input).
#[cfg(not(windows))]
#[test]
fn enable_alone_never_consumes_on_non_windows() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough
    );
}

// ── Edge case: a commit arriving directly after an empty preedit
//    keeps the queue ordered preedit, empty preedit, commit, and
//    still unblocks dispatch on non-Windows targets.
#[cfg(not(windows))]
#[test]
fn commit_after_empty_preedit_orders_queue_and_unblocks_dispatch_on_non_windows() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    b.on_winit_ime(&WinitIme::Commit("X".into()));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough
    );
    let mut out = Vec::new();
    b.pump(&mut out);
    assert_eq!(
        out,
        vec![
            ImeEvent::Preedit("x".into()),
            ImeEvent::Preedit(String::new()),
            ImeEvent::Commit("X".into()),
        ]
    );
}

// ── AC-6 (non-Windows halves): DeleteSurrounding leaves the
//    dispatch answer exactly as it was, whether a preedit is
//    currently present or absent.
#[cfg(not(windows))]
#[test]
fn delete_surrounding_leaves_dispatch_unchanged_when_preedit_present_non_windows() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
    b.on_winit_ime(&WinitIme::DeleteSurrounding {
        before_bytes: 1,
        after_bytes: 0,
    });
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Consumed
    );
}

#[cfg(not(windows))]
#[test]
fn delete_surrounding_leaves_dispatch_unchanged_when_preedit_absent_non_windows() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::DeleteSurrounding {
        before_bytes: 1,
        after_bytes: 0,
    });
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough
    );
}

// ── TS-10 / AC-1: predicate truth table. Total over all eight
//    (has_preedit, ime_enabled, windows_gate) combinations: for
//    windows_gate = true the answer is exactly ime_enabled
//    regardless of has_preedit, and for windows_gate = false the
//    answer is exactly has_preedit regardless of ime_enabled. Runs
//    on every host — this is what pins FR11's platform selector as
//    a runtime argument rather than a compile-time branch.
#[test]
fn should_suppress_key_truth_table_is_total_over_all_combinations() {
    for has_preedit in [false, true] {
        for ime_enabled in [false, true] {
            assert_eq!(
                should_suppress_key(has_preedit, ime_enabled, true),
                ime_enabled,
                "windows_gate=true must answer exactly ime_enabled \
                 (has_preedit={has_preedit}, ime_enabled={ime_enabled})"
            );
            assert_eq!(
                should_suppress_key(has_preedit, ime_enabled, false),
                has_preedit,
                "windows_gate=false must answer exactly has_preedit \
                 (has_preedit={has_preedit}, ime_enabled={ime_enabled})"
            );
        }
    }
}

// ── TS-6 / TS-11 (Windows empty active composition, asserted
//    through the predicate so it executes on every host, not only
//    when compiled for the Windows target): an empty preedit inside
//    a live composition still suppresses under the Windows gate,
//    because the gate is the lifecycle flag, not preedit emptiness.
//    Passthrough again once Disabled closes the lifecycle.
#[test]
fn windows_gate_empty_preedit_inside_live_composition_still_suppresses() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    assert!(
        should_suppress_key(b.has_preedit, b.ime_enabled, true),
        "Windows gate must still suppress: the lifecycle is open even though the preedit is empty"
    );
    b.on_winit_ime(&WinitIme::Disabled);
    assert!(
        !should_suppress_key(b.has_preedit, b.ime_enabled, true),
        "Disabled closes the lifecycle, so the Windows gate must release"
    );
}

// ── TS-7 / TS-11 (Windows commit does not end the lifecycle,
//    asserted through the predicate so it executes on every host):
//    a commit occurring mid-composition does not release
//    suppression before Disabled closes the lifecycle.
#[test]
fn windows_gate_commit_inside_live_composition_does_not_unblock() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    b.on_winit_ime(&WinitIme::Commit("X".into()));
    assert!(
        should_suppress_key(b.has_preedit, b.ime_enabled, true),
        "a commit mid-composition must not release the Windows gate"
    );
    b.on_winit_ime(&WinitIme::Disabled);
    assert!(
        !should_suppress_key(b.has_preedit, b.ime_enabled, true),
        "Disabled must release the Windows gate"
    );
}

// ── AC-6 (Windows half, asserted through the predicate so it
//    executes on every host): DeleteSurrounding is state-neutral on
//    the Windows gate too, whether a composition is currently open
//    or not.
#[test]
fn windows_gate_delete_surrounding_leaves_state_unchanged_when_preedit_present() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
    b.on_winit_ime(&WinitIme::DeleteSurrounding {
        before_bytes: 1,
        after_bytes: 0,
    });
    assert!(should_suppress_key(b.has_preedit, b.ime_enabled, true));
}

#[test]
fn windows_gate_delete_surrounding_leaves_state_unchanged_when_preedit_absent() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::DeleteSurrounding {
        before_bytes: 1,
        after_bytes: 0,
    });
    assert!(!should_suppress_key(b.has_preedit, b.ime_enabled, true));
}

// ── TS-12 / AC-4: losing focus mid-composition clears both gate
//    states, so the predicate answers passthrough for BOTH platform
//    selectors — not just the host's own target.
#[test]
fn focus_loss_clears_gate_for_both_platform_selectors() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
    b.notify_focus(false);
    assert!(
        !should_suppress_key(b.has_preedit, b.ime_enabled, true),
        "focus loss must release the Windows gate even mid-composition"
    );
    assert!(
        !should_suppress_key(b.has_preedit, b.ime_enabled, false),
        "focus loss must release the non-Windows gate too"
    );
}

// ── TS-13 / AC-5: focus gain on a freshly built bridge does not
//    open the gate — both states stay false, so the predicate
//    answers passthrough for both platform selectors. The sequence
//    assertion in `notify_focus_propagates_to_set_ime_allowed` is
//    unchanged by this test.
#[test]
fn focus_gain_on_fresh_bridge_leaves_gate_closed_for_both_platform_selectors() {
    let (mut b, _mock) = make_bridge();
    b.notify_focus(true);
    assert!(!should_suppress_key(b.has_preedit, b.ime_enabled, true));
    assert!(!should_suppress_key(b.has_preedit, b.ime_enabled, false));
}

// ── task0004 AC-1 / TS-6 dispatch counterpart (Windows target only):
//    `windows_gate_empty_preedit_inside_live_composition_still_suppresses`
//    above proves the RULE — that the predicate suppresses when
//    `windows_gate = true` and the preedit is empty inside an open
//    lifecycle. This test proves the WIRING — that
//    `dispatch_key_event` itself reaches that answer through
//    `cfg!(windows)`, not just that the predicate can be made to
//    answer it when driven directly. A predicate-only pair would
//    still pass if `dispatch_key_event` hardcoded `windows_gate =
//    false`; only a test that calls `dispatch_key_event` under a
//    real `cfg(windows)` build can catch that.
#[test]
#[cfg(windows)]
fn windows_dispatch_empty_preedit_inside_live_composition_still_suppresses() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Consumed,
        "dispatch_key_event must still suppress: the lifecycle is open even though the preedit is empty"
    );
    b.on_winit_ime(&WinitIme::Disabled);
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough,
        "Disabled closes the lifecycle, so dispatch_key_event must release"
    );
}

// ── task0004 AC-2 / TS-7 dispatch counterpart (Windows target only):
//    `windows_gate_commit_inside_live_composition_does_not_unblock`
//    above proves the RULE — that a commit mid-composition does not
//    release the predicate when `windows_gate = true`. This test
//    proves the WIRING — that `dispatch_key_event` reaches that same
//    answer through `cfg!(windows)` rather than a hardcoded or
//    miswired selector.
#[test]
#[cfg(windows)]
fn windows_dispatch_commit_inside_live_composition_does_not_unblock() {
    let (mut b, _mock) = make_bridge();
    b.on_winit_ime(&WinitIme::Enabled);
    b.on_winit_ime(&WinitIme::Preedit("x".into(), None));
    b.on_winit_ime(&WinitIme::Preedit("".into(), None));
    b.on_winit_ime(&WinitIme::Commit("X".into()));
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Consumed,
        "a commit mid-composition must not release dispatch_key_event's suppression"
    );
    b.on_winit_ime(&WinitIme::Disabled);
    assert_eq!(
        b.dispatch_key_event(&raw_press()),
        KeyDispatchResult::Passthrough,
        "Disabled must release dispatch_key_event's suppression"
    );
}

// ── TS-winit-int-1: Xvfb-backed integration test (#[ignore]).
//
// The harness creates a real winit window with `Visible::Hidden`
// semantics on Xvfb, attaches a WinitImeBridge, and asserts that
// `WindowEvent::Ime::Disabled` fires within one tick when no IM
// server is present. Running this without `#[ignore]` requires
// `Xvfb :99 -screen 0 1024x768x24` + `DISPLAY=:99` in the
// environment, which is host-deferred (see VERIFICATION.md).
#[test]
#[ignore = "requires Xvfb + DISPLAY; host-deferred"]
fn winit_event_loop_surfaces_ime_disabled_without_im_server() {
    // Marker test — the actual harness is the manual gate.
}

// ── TS-winit-int-2: Windows IMM32 integration (#[cfg(windows)]).
//
// Marker for the host-deferred Windows IMM32 check. The cargo
// tree on the Linux CI compiles this stub but the body asserts
// nothing because `cfg(windows)` is false. The real assertion is
// run on the Windows host (see TS-manual-ime-windows).
#[test]
#[cfg(windows)]
fn winit_imm32_surfaces_ime_commit() {
    // Marker test — the actual harness is the manual gate.
}
