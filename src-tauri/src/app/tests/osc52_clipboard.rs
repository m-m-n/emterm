//! `App::pump_all`-level tests for the OSC 52 clipboard drain
//! (osc52-clipboard-write-drain task0001: AC-4 .. AC-7, TS-2 queue part,
//! TS-3, TS-4). Every test drives shell-less tabs through their PTY event
//! channel and one `pump_all`; none loops on `pump_all` and none loads the
//! CPU to reproduce timing.

use super::*;
use crate::pty::PtyEvent;
use crate::tabs::{OFFTHREAD_REPLAY_THRESHOLD_BYTES, Tab};
use crate::window_host::osc52_clipboard::Osc52Destination::Clipboard;
use crate::window_host::osc52_clipboard::apply_pending_osc52;
use crate::window_host::osc52_clipboard::tests::{RecordingSink, SinkCall};
use base64::Engine;
use mux_ipc::protocol::{MessageType, MuxMessage, SessionInfo, WelcomeMsg, WindowInfo};

// ── helpers ──────────────────────────────────────────────────────────

fn shell_less_tab(app: &App) -> Tab {
    let dims = app.cell_size;
    Tab::test_shell_less(
        "shell",
        dims.cols,
        dims.rows,
        app.settings.scrollback_lines,
        app.settings.clone(),
        app.notification_sink.clone(),
    )
}

/// An `App` with `n` shell-less tabs, tab 0 active.
fn app_with_shell_less_tabs(n: usize) -> App {
    let mut app = App::new();
    for _ in 0..n {
        let tab = shell_less_tab(&app);
        app.tabs.push(tab);
    }
    app.active = 0;
    app
}

/// `ESC ] 52 ; <target> ; <base64(text)> BEL`.
fn osc52_write(target: &str, text: &str) -> Vec<u8> {
    let payload = base64::engine::general_purpose::STANDARD.encode(text);
    format!("\x1b]52;{target};{payload}\x07").into_bytes()
}

/// `ESC ] 52 ; <target> ; ? BEL` (a read query).
fn osc52_query(target: &str) -> Vec<u8> {
    format!("\x1b]52;{target};?\x07").into_bytes()
}

/// Replace the tab's PTY event channel with one pre-loaded with `chunks`
/// (the next `Tab::pump` coalesces them into one buffer).
fn feed_pty(tab: &mut Tab, chunks: Vec<Vec<u8>>) {
    let (tx, rx) = crossbeam_channel::unbounded::<PtyEvent>();
    for chunk in chunks {
        tx.send(PtyEvent::Data(chunk)).expect("channel open");
    }
    tab.events = rx;
}

fn welcome(pane: u32) -> MuxMessage {
    MuxMessage::control(
        MessageType::Welcome,
        0,
        &WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![SessionInfo {
                id: 1,
                name: "main".to_string(),
                window_count: 1,
                pane_count: 1,
                active_window_index: 0,
                windows: vec![WindowInfo {
                    id: 1,
                    name: "win".to_string(),
                    active_pane_id: pane,
                }],
            }],
        },
    )
}

/// A shell-less tab attached to a single-window mux session whose active
/// pane is `pane`.
fn mux_shell_less_tab(app: &App, pane: u32) -> Tab {
    let mut tab = shell_less_tab(app);
    tab.apply_mux_message(welcome(pane));
    assert!(tab.mux_session_name.is_some(), "mux session established");
    tab
}

/// The outer `emterm-mux;` APC frame carrying `payload` as `PtyOutput` for
/// `pane`, exactly as the bridge writes it to the PTS stream.
fn pty_output_frame(pane: u32, payload: &[u8]) -> Vec<u8> {
    crate::mux::apc::encode_emterm_mux(&MuxMessage {
        msg_type: MessageType::PtyOutput,
        pane_id: pane,
        payload: payload.to_vec(),
    })
}

fn detached_frame() -> Vec<u8> {
    crate::mux::apc::encode_emterm_mux(&MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    })
}

fn entry(target: &str, text: &str) -> (String, String) {
    (target.to_string(), text.to_string())
}

fn queues_are_empty(tab: &Tab) -> bool {
    let s = tab.cb_state.lock();
    s.pending_clipboard_writes.is_empty() && s.pending_clipboard_reads.is_empty()
}

// ── AC-4: queues empty after one pump, reads discarded (FR4, FR6, NFR1) ──

/// AC-4: after one `pump_all` over an active tab and a background tab that
/// both parsed OSC 52 writes and a query, both tabs' queues are empty, the
/// batch holds the writes in tab order then queue order, and no byte went
/// to either tab's PTY. A second pump leaves everything empty.
#[test]
fn ac4_pump_all_drains_every_tab_and_discards_reads_without_pty_output() {
    let mut app = app_with_shell_less_tabs(2);
    // Parse directly so the queued state is observable BEFORE the pump (the
    // query is only queued when `clipboard_read_osc52` is on, the default).
    app.tabs[0].test_process_combined(
        [
            osc52_write("c", "a0"),
            osc52_query("c"),
            osc52_write("p", "b0"),
        ]
        .concat(),
    );
    app.tabs[1].test_process_combined([osc52_query("p"), osc52_write("c", "a1")].concat());
    for (i, expected_writes) in [(0usize, 2usize), (1, 1)] {
        let s = app.tabs[i].cb_state.lock();
        assert_eq!(s.pending_clipboard_writes.len(), expected_writes, "tab {i}");
        assert_eq!(
            s.pending_clipboard_reads.len(),
            1,
            "tab {i}: query accepted and queued"
        );
    }
    let writes_before: Vec<usize> = app
        .tabs
        .iter()
        .map(|t| t.test_outbound_writes().len())
        .collect();

    app.pump_all();

    for (i, tab) in app.tabs.iter().enumerate() {
        assert!(
            queues_are_empty(tab),
            "tab {i}: both queues empty after the pump"
        );
        assert_eq!(
            tab.test_outbound_writes().len(),
            writes_before[i],
            "tab {i}: no byte written to the PTY in response to the query"
        );
    }
    assert_eq!(
        app.take_osc52_clipboard_batch(),
        vec![entry("c", "a0"), entry("p", "b0"), entry("c", "a1")],
        "writes in tab order, then queue order"
    );

    // A second pump with no new output: queues and batch stay empty.
    app.pump_all();
    assert!(app.tabs.iter().all(queues_are_empty));
    assert!(app.take_osc52_clipboard_batch().is_empty());
}

/// AC-4 (FR6): a background tab attached to a mux session is drained too.
#[test]
fn ac4_background_mux_tab_is_drained() {
    let mut app = app_with_shell_less_tabs(1);
    let mut mux_tab = mux_shell_less_tab(&app, 100);
    feed_pty(
        &mut mux_tab,
        vec![pty_output_frame(
            100,
            &[osc52_write("c", "bg"), osc52_query("c")].concat(),
        )],
    );
    app.tabs.push(mux_tab);
    assert_eq!(app.active, 0, "the mux tab is a background tab");

    app.pump_all();

    assert!(app.tabs.iter().all(queues_are_empty));
    assert_eq!(app.take_osc52_clipboard_batch(), vec![entry("c", "bg")]);
}

// ── AC-5: every parse path reaches this pump's batch (FR5, FR6) ──────

/// AC-5 (TS-4): the pre-mux outer parse of plain shell output.
#[test]
fn ac5_pre_mux_outer_parse_reaches_the_batch() {
    let mut app = app_with_shell_less_tabs(1);
    feed_pty(&mut app.tabs[0], vec![osc52_write("c", "plain")]);

    app.pump_all();

    assert_eq!(app.take_osc52_clipboard_batch(), vec![entry("c", "plain")]);
    assert!(queues_are_empty(&app.tabs[0]));
}

/// AC-5 (TS-3): an active-pane mux `PtyOutput` frame. The frame-apply loop
/// of `process_combined` runs after its early `cb_state` block, so a drain
/// placed in that block would leave this entry queued.
#[test]
fn ac5_active_pane_mux_pty_output_reaches_the_batch() {
    let mut app = app_with_shell_less_tabs(0);
    let mut tab = mux_shell_less_tab(&app, 100);
    feed_pty(
        &mut tab,
        vec![pty_output_frame(100, &osc52_write("c", "mux"))],
    );
    app.tabs.push(tab);

    app.pump_all();

    assert_eq!(app.take_osc52_clipboard_batch(), vec![entry("c", "mux")]);
    assert!(queues_are_empty(&app.tabs[0]));
}

/// AC-5 (TS-3): coalesced consecutive `PtyOutput` frames arrive in order.
#[test]
fn ac5_coalesced_mux_frames_keep_queue_order() {
    let mut app = app_with_shell_less_tabs(0);
    let mut tab = mux_shell_less_tab(&app, 100);
    feed_pty(
        &mut tab,
        vec![
            pty_output_frame(100, &osc52_write("c", "first")),
            pty_output_frame(100, &osc52_write("p", "second")),
        ],
    );
    app.tabs.push(tab);

    app.pump_all();

    assert_eq!(
        app.take_osc52_clipboard_batch(),
        vec![entry("c", "first"), entry("p", "second")]
    );
}

/// AC-5 (TS-3): the post-`Detached` tail of the same coalesced buffer is
/// re-routed to the tab's own core and its OSC 52 write reaches the same
/// pump's batch.
#[test]
fn ac5_post_detached_tail_reaches_the_batch() {
    let mut app = app_with_shell_less_tabs(0);
    let mut tab = mux_shell_less_tab(&app, 100);
    let mut buffer = pty_output_frame(100, &osc52_write("c", "inner"));
    buffer.extend_from_slice(&detached_frame());
    buffer.extend_from_slice(&osc52_write("p", "tail"));
    feed_pty(&mut tab, vec![buffer]);
    app.tabs.push(tab);

    app.pump_all();

    assert!(app.tabs[0].mux_session_name.is_none(), "Detached applied");
    assert_eq!(
        app.take_osc52_clipboard_batch(),
        vec![entry("c", "inner"), entry("p", "tail")]
    );
    assert!(queues_are_empty(&app.tabs[0]));
}

/// AC-5 (D3): live output queued during an off-thread switch is applied on
/// the swap inside `pump_all` and its OSC 52 write reaches that pump's
/// batch. `test_block_worker_ready` makes the swap deterministic: one
/// `pump_all`, no polling loop.
#[test]
fn ac5_live_output_queued_during_an_offthread_switch_reaches_the_batch() {
    let mut app = app_with_shell_less_tabs(0);
    let mut tab = mux_shell_less_tab(&app, 100);
    let mut snapshot = b"SNAP\r\n".to_vec();
    snapshot.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
    tab.apply_mux_message(MuxMessage {
        msg_type: MessageType::Snapshot,
        pane_id: 100,
        payload: snapshot,
    });
    assert!(tab.test_has_pending_switch(), "off-thread replay in flight");
    tab.apply_mux_message(MuxMessage {
        msg_type: MessageType::PtyOutput,
        pane_id: 100,
        payload: osc52_write("c", "live"),
    });
    assert_eq!(tab.test_pending_live_queue().len(), 1, "live output queued");
    tab.test_block_worker_ready();
    app.tabs.push(tab);

    app.pump_all();

    assert!(!app.tabs[0].test_has_pending_switch(), "swap applied");
    assert_eq!(app.take_osc52_clipboard_batch(), vec![entry("c", "live")]);
    assert!(queues_are_empty(&app.tabs[0]));
}

// ── AC-6: pump-to-sink step (FR1, FR2) ───────────────────────────────

/// AC-6: a tab's BEL-terminated `c` write of `hello` leaves a CLIPBOARD
/// write of `hello` on the sink; the batch is consumed, so a second step
/// with nothing new makes no sink call.
#[test]
fn ac6_pump_to_sink_step_applies_the_pumped_write() {
    let mut app = app_with_shell_less_tabs(1);
    feed_pty(&mut app.tabs[0], vec![osc52_write("c", "hello")]);
    app.pump_all();

    let mut sink = RecordingSink::default();
    apply_pending_osc52(&mut app, &mut sink);
    assert_eq!(
        sink.calls,
        vec![SinkCall::Write(Clipboard, "hello".to_string())]
    );

    let mut again = RecordingSink::default();
    apply_pending_osc52(&mut app, &mut again);
    assert!(again.calls.is_empty(), "the batch was consumed");
}

/// AC-6: an empty batch makes no sink call.
#[test]
fn ac6_pump_to_sink_step_with_an_empty_batch_makes_no_sink_call() {
    let mut app = app_with_shell_less_tabs(1);
    app.pump_all();
    let mut sink = RecordingSink::default();
    apply_pending_osc52(&mut app, &mut sink);
    assert!(sink.calls.is_empty());
}

/// AC-6 (FR2): a clear from a tab reaches the sink as a clear, not as an
/// empty-text write.
#[test]
fn ac6_pump_to_sink_step_applies_a_clear_as_a_clear() {
    let mut app = app_with_shell_less_tabs(1);
    feed_pty(&mut app.tabs[0], vec![b"\x1b]52;c;\x07".to_vec()]);
    app.pump_all();

    let mut sink = RecordingSink::default();
    apply_pending_osc52(&mut app, &mut sink);
    assert_eq!(sink.calls, vec![SinkCall::Clear(Clipboard)]);
}

// ── AC-7: acceptance gates unchanged (FR9, TM-1) ─────────────────────

/// AC-7 (TM-1): over-quota, invalid base64 and non-UTF-8 payloads add no
/// batch entry; a valid write in the same pump still does.
#[test]
fn ac7_rejected_payloads_add_no_batch_entry() {
    let mut settings = crate::settings::Settings::default();
    settings.clipboard_max_size_osc52 = 3;
    let mut app = App::with_settings(settings);
    let tab = shell_less_tab(&app);
    app.tabs.push(tab);
    app.active = 0;

    let non_utf8 = base64::engine::general_purpose::STANDARD.encode([0xffu8, 0xfe]);
    feed_pty(
        &mut app.tabs[0],
        vec![
            osc52_write("c", "hello"),                        // 5 bytes > cap 3
            b"\x1b]52;c;!!!not-base64!!!\x07".to_vec(),       // invalid base64
            format!("\x1b]52;c;{non_utf8}\x07").into_bytes(), // non-UTF-8
            osc52_write("c", "ok"),                           // within cap
        ],
    );

    app.pump_all();

    assert_eq!(app.take_osc52_clipboard_batch(), vec![entry("c", "ok")]);
    assert!(queues_are_empty(&app.tabs[0]));
}
