//! APC payload decoder for the `emterm-mux` inband protocol.
//!
//! Legacy mux runs `emterm mux` as a *bridge* CLI inside a regular PTY. The
//! bridge owns a Unix socket connection to the daemon and translates between
//! `MuxMessage` frames on the socket and APC escape sequences on the PTY
//! stream:
//!
//! ```text
//!   PTY output: ESC _ emterm-mux;<base64(frame_body)> ESC \
//! ```
//!
//! native-poc therefore never connects to the daemon directly. It simply
//! treats the bridge as an ordinary shell, observes APC sequences in the
//! resulting PTY output via [`term_core::TerminalCallbacks::on_apc`], and
//! routes the decoded `MuxMessage` into the active tab via
//! [`crate::app::App::on_mux_message`].
//!
//! This module is the seam between the raw APC payload (already stripped of
//! `ESC _` introducer and `ESC \` terminator by the WASM/native parser) and
//! the typed `mux_ipc::protocol::MuxMessage` API.
//!
//! See `doc/tasks/mux-inband-protocol/SPEC.md` for the wire format.

use mux_ipc::protocol::MuxMessage;

/// Try to decode an APC payload as an `emterm-mux` inband-protocol frame.
///
/// `data` is the raw payload between `ESC _` and `ESC \` (term_core's
/// `on_apc` callback hands us this slice verbatim). The function returns
/// `Some(msg)` when the payload begins with the `emterm-mux;` prefix and the
/// trailing base64 frame body decodes into a valid `MuxMessage`.
///
/// All other inputs return `None`:
///
/// - Kitty Graphics APCs (`G,...`) — handled by the image pipeline.
/// - Other vendor APCs that happen to share the channel.
/// - Empty payloads.
/// - Payloads that look like `emterm-mux;` but carry malformed base64 / a
///   truncated frame body (logged at `warn`).
///
/// The function never panics on malformed input.
pub fn try_decode_emterm_mux(data: &[u8]) -> Option<MuxMessage> {
    // UTF-8 validation: emterm-mux APC is always ASCII (prefix + base64).
    // Non-UTF8 payloads cannot be addressed to us — short-circuit cheaply.
    let s = std::str::from_utf8(data).ok()?;
    if !s.starts_with("emterm-mux;") {
        return None;
    }
    match MuxMessage::from_apc(s) {
        Ok(msg) => Some(msg),
        Err(e) => {
            log::warn!("mux APC decode failed: {e} (payload len = {})", data.len());
            None
        }
    }
}

/// Encode a `MuxMessage` into an outbound `emterm-mux` APC byte sequence
/// (`ESC _ emterm-mux;<base64(frame_body)> ESC \`), the counterpart to
/// [`try_decode_emterm_mux`].
///
/// native-poc writes the result straight to the active tab's PTY
/// (fire-and-forget); the `emterm mux` bridge reads it off the PTY and
/// relays the frame to the daemon over its Unix socket. native never opens
/// the socket itself (NFR2 — SSH transparency). Responses arrive back as
/// inbound APC through the existing `on_apc` route.
///
/// Mirrors the WebView `MuxClient.sendControl` → `encodeApc` path. The wire
/// shape is identical to `MuxMessage::to_apc`, so the result round-trips
/// with the decoder above.
pub fn encode_emterm_mux(msg: &MuxMessage) -> Vec<u8> {
    msg.to_apc().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mux_ipc::protocol::{
        CreateWindowPayload, MessageType, MoveWindowMsg, MuxMessage, RenameWindowMsg,
        StatusUpdateMsg,
    };

    /// Strip the `ESC _` / `ESC \` framing the way `term_core`'s `on_apc`
    /// hands the slice to [`try_decode_emterm_mux`].
    fn strip_apc(apc: &[u8]) -> Vec<u8> {
        let s = std::str::from_utf8(apc).unwrap();
        let inner = s
            .strip_prefix("\x1b_")
            .and_then(|s| s.strip_suffix("\x1b\\"))
            .expect("apc framing");
        inner.as_bytes().to_vec()
    }

    // ── TS-5: outbound encode round-trips with the decoder ────────────────

    #[test]
    fn encode_create_window_round_trips() {
        let payload = CreateWindowPayload::default();
        let msg = MuxMessage::control(MessageType::CreateWindow, 0, &payload);
        let apc = encode_emterm_mux(&msg);
        let decoded = try_decode_emterm_mux(&strip_apc(&apc)).expect("decoded");
        assert_eq!(decoded.msg_type, MessageType::CreateWindow);
        assert_eq!(decoded.pane_id, 0);
    }

    #[test]
    fn encode_switch_window_round_trips() {
        let msg = MuxMessage {
            msg_type: MessageType::SwitchWindow,
            pane_id: 42,
            payload: Vec::new(),
        };
        let apc = encode_emterm_mux(&msg);
        let decoded = try_decode_emterm_mux(&strip_apc(&apc)).unwrap();
        assert_eq!(decoded.msg_type, MessageType::SwitchWindow);
        assert_eq!(decoded.pane_id, 42);
    }

    #[test]
    fn encode_rename_window_round_trips() {
        let rename = RenameWindowMsg {
            name: "editor 🎉".to_string(),
        };
        let msg = MuxMessage::control(MessageType::RenameWindow, 7, &rename);
        let apc = encode_emterm_mux(&msg);
        let decoded = try_decode_emterm_mux(&strip_apc(&apc)).unwrap();
        assert_eq!(decoded.msg_type, MessageType::RenameWindow);
        assert_eq!(decoded.pane_id, 7);
        let back: RenameWindowMsg = decoded.decode_payload().unwrap();
        assert_eq!(back.name, "editor 🎉");
    }

    #[test]
    fn encode_move_window_round_trips() {
        let mv = MoveWindowMsg { target_index: 3 };
        let msg = MuxMessage::control(MessageType::MoveWindow, 9, &mv);
        let apc = encode_emterm_mux(&msg);
        let decoded = try_decode_emterm_mux(&strip_apc(&apc)).unwrap();
        assert_eq!(decoded.msg_type, MessageType::MoveWindow);
        assert_eq!(decoded.pane_id, 9);
        let back: MoveWindowMsg = decoded.decode_payload().unwrap();
        assert_eq!(back.target_index, 3);
    }

    #[test]
    fn encode_request_pane_snapshot_round_trips() {
        // Screen-restore request: control frame carrying just the pane id.
        let msg = MuxMessage {
            msg_type: MessageType::RequestPaneSnapshot,
            pane_id: 5,
            payload: Vec::new(),
        };
        let apc = encode_emterm_mux(&msg);
        let decoded = try_decode_emterm_mux(&strip_apc(&apc)).unwrap();
        assert_eq!(decoded.msg_type, MessageType::RequestPaneSnapshot);
        assert_eq!(decoded.pane_id, 5);
    }

    // ── TS-apc-1: happy path round-trip ──────────────────────────────────

    #[test]
    fn decodes_status_update_apc_payload() {
        let status = StatusUpdateMsg {
            left: "[default] *win1 win2".to_string(),
            right: "12:34:56".to_string(),
        };
        let msg = MuxMessage::control(MessageType::StatusUpdate, 0, &status);
        let apc = msg.to_apc();
        // Strip the ESC _ and ESC \ surroundings to match what term_core
        // hands `on_apc`.
        let payload = apc
            .strip_prefix("\x1b_")
            .and_then(|s| s.strip_suffix("\x1b\\"))
            .expect("apc framing");

        let decoded = try_decode_emterm_mux(payload.as_bytes()).expect("decoded");
        assert_eq!(decoded.msg_type, MessageType::StatusUpdate);
        let parsed: StatusUpdateMsg = decoded.decode_payload().expect("status payload");
        assert_eq!(parsed.left, status.left);
        assert_eq!(parsed.right, status.right);
    }

    #[test]
    fn decodes_pty_output_apc_payload() {
        let msg = MuxMessage::pty_output(7, b"hello world".to_vec());
        let apc = msg.to_apc();
        let payload = apc
            .strip_prefix("\x1b_")
            .and_then(|s| s.strip_suffix("\x1b\\"))
            .unwrap();
        let decoded = try_decode_emterm_mux(payload.as_bytes()).unwrap();
        assert_eq!(decoded.msg_type, MessageType::PtyOutput);
        assert_eq!(decoded.pane_id, 7);
        assert_eq!(decoded.payload, b"hello world");
    }

    // ── TS-apc-2: non-mux APCs return None ───────────────────────────────

    #[test]
    fn ignores_kitty_graphics_apc() {
        // Real Kitty payloads start with `G,...`. They must never decode as
        // mux frames — they belong to the image pipeline.
        let kitty = b"Ga=q,i=1;";
        assert!(try_decode_emterm_mux(kitty).is_none());
    }

    #[test]
    fn ignores_unknown_vendor_apc() {
        let other = b"vendor-x;some-payload";
        assert!(try_decode_emterm_mux(other).is_none());
    }

    // ── TS-apc-3: malformed mux payloads warn and return None ────────────

    #[test]
    fn rejects_invalid_base64_after_prefix() {
        // Prefix is correct but the base64 body is garbage.
        let payload = b"emterm-mux;!!!not_base64!!!";
        assert!(try_decode_emterm_mux(payload).is_none());
    }

    #[test]
    fn rejects_truncated_frame_body() {
        // Valid base64, but the resulting bytes are < 5 (the
        // `[type][pane_id]` header minimum).
        let payload = b"emterm-mux;AA==";
        assert!(try_decode_emterm_mux(payload).is_none());
    }

    // ── TS-apc-4: empty / boundary payloads ──────────────────────────────

    #[test]
    fn empty_payload_returns_none() {
        assert!(try_decode_emterm_mux(b"").is_none());
    }

    #[test]
    fn bare_prefix_without_payload_returns_none() {
        // `emterm-mux;` with nothing after it: zero-byte base64 → from_apc
        // fails on the `< 5` frame body check.
        let payload = b"emterm-mux;";
        assert!(try_decode_emterm_mux(payload).is_none());
    }

    #[test]
    fn non_utf8_payload_returns_none() {
        let payload: &[u8] = &[0xFF, 0xFE, 0xFD];
        assert!(try_decode_emterm_mux(payload).is_none());
    }
}
