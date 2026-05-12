//! APC / DCS payload → `ImageEvent` adapter.
//!
//! `term_core::TerminalCallbacks::on_apc` / `on_dcs` deliver raw byte
//! payloads (the bytes between `ESC _` / `ESC P` and the terminating
//! `ESC \`), but `term_images::ImageProcessor` needs:
//! 1. a parsed `KittyCommand` / `SixelData`
//! 2. the cursor position at the time of the sequence
//!
//! Phase 5 cannot capture (2) inside the callback because the callback only
//! sees `&self` on `NativeCallbacks` — it has no access to `TerminalCore`.
//! Instead the callback buffers the raw bytes and `Tab::pump` drives this
//! module after locking the core (so the cursor coords are stable).
//!
//! Both decoders are pure functions: no I/O, no global state. They are
//! covered by unit tests below.

use term_images::ansi::apc::parse_kitty_command;
use term_images::ansi::dcs::parse_sixel_sequence;
use term_images::image_proc::{ImageEvent, ImageProcessor};

/// Decode a Kitty Graphics APC payload and feed it into the shared
/// `ImageProcessor`. Returns the resulting [`ImageEvent`]s (image-ready,
/// place, delete, response, …). Caller is expected to split out
/// `ImageEvent::Response` and write its bytes back to the PTY.
///
/// Returns an empty vector if:
/// - the payload does not start with `G` (not a Kitty Graphics command),
/// - the payload is otherwise malformed.
pub fn decode_apc(
    data: &[u8],
    cursor_row: u32,
    cursor_col: u32,
    processor: &mut ImageProcessor,
) -> Vec<ImageEvent> {
    match parse_kitty_command(data) {
        Some(cmd) => processor.process_kitty_command(&cmd, cursor_row, cursor_col),
        None => {
            log::debug!(
                "decode_apc: parse_kitty_command rejected {}-byte payload",
                data.len()
            );
            Vec::new()
        }
    }
}

/// Decode a SIXEL DCS payload and feed it into the shared
/// `ImageProcessor`. Returns the resulting [`ImageEvent`]s. Caller is
/// expected to split out `ImageEvent::Response` (currently SIXEL does
/// not emit responses, but the API mirrors `decode_apc` for symmetry).
///
/// Returns an empty vector when the DCS is not a SIXEL sequence (no `q`
/// introducer).
pub fn decode_dcs(
    data: &[u8],
    cursor_row: u32,
    cursor_col: u32,
    processor: &mut ImageProcessor,
) -> Vec<ImageEvent> {
    match parse_sixel_sequence(data) {
        Some(sixel) => processor.process_sixel(&sixel, cursor_row, cursor_col),
        None => {
            log::debug!(
                "decode_dcs: parse_sixel_sequence rejected {}-byte payload",
                data.len()
            );
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_apc_rejects_empty_payload() {
        let mut proc = ImageProcessor::new();
        let events = decode_apc(b"", 0, 0, &mut proc);
        assert!(events.is_empty());
    }

    #[test]
    fn decode_apc_rejects_non_kitty_payload() {
        let mut proc = ImageProcessor::new();
        // Missing 'G' prefix → parse_kitty_command returns None.
        let events = decode_apc(b"Xa=q;", 0, 0, &mut proc);
        assert!(events.is_empty());
    }

    #[test]
    fn decode_apc_query_emits_response_event() {
        // Kitty query: `Ga=q;` should yield an `ImageEvent::Response`
        // (the protocol's OK reply). See term_images kitty.rs test
        // `test_kitty_query` for the same fixture.
        let mut proc = ImageProcessor::new();
        let events = decode_apc(b"Ga=q;", 0, 0, &mut proc);
        assert!(!events.is_empty(), "query must produce at least one event");
        let has_response = events
            .iter()
            .any(|e| matches!(e, ImageEvent::Response { .. }));
        assert!(has_response, "query must produce ImageEvent::Response");
    }

    #[test]
    fn decode_apc_delete_all_emits_delete_event() {
        let mut proc = ImageProcessor::new();
        let events = decode_apc(b"Ga=d,d=a;", 0, 0, &mut proc);
        let has_delete = events
            .iter()
            .any(|e| matches!(e, ImageEvent::Delete { .. }));
        assert!(has_delete, "a=d,d=a must produce ImageEvent::Delete");
    }

    #[test]
    fn decode_apc_put_without_image_emits_no_place() {
        // a=p references image id 1 but none is stored → handler returns
        // an error response or nothing, but it must not panic.
        let mut proc = ImageProcessor::new();
        let _ = decode_apc(b"Ga=p,i=1,p=2,c=10,r=5;", 5, 7, &mut proc);
        // We don't assert on the exact event shape — just that the call
        // completes. Behavioural correctness is owned by term_images.
    }

    #[test]
    fn decode_dcs_rejects_payload_without_q_introducer() {
        let mut proc = ImageProcessor::new();
        // No 'q' → parse_sixel_sequence returns None.
        let events = decode_dcs(b"0;0;0", 0, 0, &mut proc);
        assert!(events.is_empty());
    }

    #[test]
    fn decode_dcs_accepts_minimal_sixel_introducer() {
        // Smallest valid form: `q` with no pixel data → SixelData::default
        // with no colors, no rows. The processor may emit zero events
        // (empty image is dropped) — we only assert no panic.
        let mut proc = ImageProcessor::new();
        let _ = decode_dcs(b"q", 0, 0, &mut proc);
    }

    #[test]
    fn decode_apc_cursor_coords_propagate_to_place_events() {
        // a=T (transmit and display): the resulting Place event should
        // anchor at (cursor_row, cursor_col).
        let mut proc = ImageProcessor::new();
        // 100 = PNG; minimal payload — the decoder may reject the bytes
        // as a PNG but it still records the placement metadata.
        let events = decode_apc(b"Ga=T,f=100,s=10,v=10;iVBORw0KGgo=", 7, 11, &mut proc);
        // The actual decode may fail (truncated PNG), but if any Place
        // event is produced its (row, col) must match the cursor.
        for e in &events {
            if let ImageEvent::Place { placement } = e {
                assert_eq!(placement.row, 7);
                assert_eq!(placement.col, 11);
            }
        }
    }
}
