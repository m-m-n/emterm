use super::*;
use term_core::terminal_core::PendingPromptMark;

use mux_ipc::protocol::{SessionInfo, WindowInfo};

mod marks_fold;
mod mux_link;
mod output_pipeline;
mod replay;

struct NoopSink;
impl crate::callbacks::NotificationSink for NoopSink {
    fn send(&self, _title: &str, _body: &str) {}
}

fn test_tab() -> Tab {
    Tab::spawn_shell(
        "test",
        80,
        24,
        100,
        Arc::new(Settings::default()),
        None,
        None,
        Arc::new(NoopSink),
        None,
    )
}

/// Build a `PendingPromptMark` of an arbitrary kind at `abs_row` with no
/// eviction (the common test frame: `evicted_total == 0`).
fn pending_kind(kind: u8, abs_row: u32, exit_code: Option<i32>) -> PendingPromptMark {
    PendingPromptMark {
        kind,
        abs_row,
        exit_code,
        evicted_total: 0,
    }
}

fn welcome_msg(windows: &[(u32, &str, u32)], active: u32) -> MuxMessage {
    let windows: Vec<WindowInfo> = windows
        .iter()
        .map(|(id, name, pane)| WindowInfo {
            id: *id,
            name: name.to_string(),
            active_pane_id: *pane,
        })
        .collect();
    let session = SessionInfo {
        id: 1,
        name: "main".to_string(),
        window_count: windows.len() as u32,
        pane_count: windows.len() as u32,
        active_window_index: active,
        windows,
    };
    MuxMessage::control(
        MessageType::Welcome,
        0,
        &WelcomeMsg::Accepted {
            server_version: 1,
            sessions: vec![session],
        },
    )
}

fn switch_window(pane_id: u32) -> MuxMessage {
    MuxMessage {
        msg_type: MessageType::SwitchWindow,
        pane_id,
        payload: Vec::new(),
    }
}

fn snapshot_msg(pane_id: u32, payload: Vec<u8>) -> MuxMessage {
    MuxMessage {
        msg_type: MessageType::Snapshot,
        pane_id,
        payload,
    }
}

fn pty_output(pane_id: u32, payload: Vec<u8>) -> MuxMessage {
    MuxMessage {
        msg_type: MessageType::PtyOutput,
        pane_id,
        payload,
    }
}

/// A payload at or above the off-thread threshold whose first row, once
/// replayed, is `marker` followed by a newline so subsequent live output
/// lands on row 1 (the worker-built core is identifiable by row 0). The
/// trailing NUL padding is ignored by the parser and leaves the cursor at
/// the start of row 1.
fn large_payload(marker: &str) -> Vec<u8> {
    let mut p = marker.as_bytes().to_vec();
    p.extend_from_slice(b"\r\n");
    // Pad past the threshold with NULs (ignored by the parser; they do
    // not advance the cursor) so row 0 stays exactly `marker`.
    p.resize(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16, 0);
    p
}

/// A small snapshot payload whose first row replays to `marker` (stays on
/// the synchronous path; reused as the contiguous-parse reference).
fn small_snapshot_bytes(marker: &str) -> Vec<u8> {
    marker.as_bytes().to_vec()
}

/// Grid fingerprint of a tab's displayed core (all rows trimmed of
/// trailing blanks + cursor position), for parity assertions.
fn displayed_fingerprint(tab: &Tab) -> (Vec<String>, u16, u16) {
    let c = tab.core.lock();
    let mut rows = Vec::with_capacity(c.rows() as usize);
    for r in 0..c.rows() {
        let line: String = (0..c.cols()).map(|col| c.get_cell_char(col, r)).collect();
        rows.push(line.trim_end().to_string());
    }
    (rows, c.get_cursor_col(), c.get_cursor_row())
}

/// Wrap inner-content bytes as an outer `emterm-mux;` PtyOutput APC frame
/// for pane `pane_id`, exactly as the daemon/bridge writes it to the PTS
/// stream (`ESC _ emterm-mux;<base64(frame)> ESC \`).
fn pty_output_apc(pane_id: u32, inner: &[u8]) -> Vec<u8> {
    let msg = MuxMessage {
        msg_type: MessageType::PtyOutput,
        pane_id,
        payload: inner.to_vec(),
    };
    crate::mux::apc::encode_emterm_mux(&msg)
}

/// Build a tab attached to a single-window mux session whose active pane is
/// `pane`, so `PtyOutput` for `pane` flows straight into the displayed core
/// (no pending switch, pane filter satisfied).
fn mux_tab_active_pane(pane: u32) -> Tab {
    let mut tab = test_tab();
    tab.apply_mux_message(welcome_msg(&[(1, "win", pane)], 0));
    assert!(tab.mux_session_name.is_some(), "mux session established");
    assert!(
        !tab.test_has_pending_switch(),
        "no snapshot pending: PtyOutput must reach core directly"
    );
    tab
}
