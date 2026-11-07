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

/// A `SnapshotRestore` frame (the reattach shape) for `pane_id`.
fn snapshot_restore_msg(pane_id: u32, payload: Vec<u8>) -> MuxMessage {
    MuxMessage {
        msg_type: MessageType::SnapshotRestore,
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

/// An OSC 52 write sequence (`ESC ] 52 ; target ; base64(text) BEL`).
/// Callers pass non-empty `text`: an empty one encodes to an empty payload,
/// which `NativeCallbacks` reads as a clear (see `osc52_clear`).
fn osc52_write(target: &str, text: &str) -> Vec<u8> {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;{target};{encoded}\x07").into_bytes()
}

/// An OSC 52 clear sequence (`ESC ] 52 ; target ; BEL`, empty payload).
fn osc52_clear(target: &str) -> Vec<u8> {
    format!("\x1b]52;{target};\x07").into_bytes()
}

/// An OSC 52 read query (`ESC ] 52 ; target ; ? BEL`).
fn osc52_query(target: &str) -> Vec<u8> {
    format!("\x1b]52;{target};?\x07").into_bytes()
}

/// Concatenate byte fragments (OSC sequences interleaved with text).
fn concat_bytes(parts: &[&[u8]]) -> Vec<u8> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

/// Pad `content` with trailing NULs (ignored by the parser) up to the
/// off-thread replay threshold, so the snapshot takes the off-thread path
/// without changing what the content paints or queues.
fn pad_to_offthread_threshold(mut content: Vec<u8>) -> Vec<u8> {
    content.resize(content.len().max(OFFTHREAD_REPLAY_THRESHOLD_BYTES + 16), 0);
    content
}

/// Snapshot of a tab's OSC 52 queues: `(writes, reads)`, in queue order. A
/// clear is a write entry with empty text.
fn osc52_queues(tab: &Tab) -> (Vec<(String, String)>, Vec<String>) {
    let s = tab.cb_state.lock();
    (
        s.pending_clipboard_writes.clone(),
        s.pending_clipboard_reads.clone(),
    )
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

/// The outer `emterm-mux;` APC frame of a `Detached` control message, as the
/// bridge writes it to the PTS stream.
fn detached_apc() -> Vec<u8> {
    crate::mux::apc::encode_emterm_mux(&MuxMessage {
        msg_type: MessageType::Detached,
        pane_id: 0,
        payload: Vec::new(),
    })
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
