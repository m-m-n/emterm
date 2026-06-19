//! Independent transport extractor for the mux inband protocol.
//!
//! A mux tab receives one PTS byte stream that is intrinsically two layers:
//! an outer transport (`emterm-mux;` APC frames) and, inside `PtyOutput`
//! messages, the inner content (the remote pane's raw terminal output).
//! Sharing the tab's single `TerminalCore` parser across both layers corrupts
//! parser state when an inner Kitty image chunk is split across `PtyOutput`
//! boundaries, leaking base64 and breaking image decode.
//!
//! [`MuxApcExtractor`] owns its **own** [`Parser`] instance, isolated from the
//! tab's `TerminalCore`. Once mux is established the tab's outer parse is fed
//! here instead of into the core, so the core is driven by inner content only.
//!
//! The extractor surfaces a unified **mux-APC payload list** and discards
//! Print / CSI / Esc / Execute / DCS. Normalization mirrors
//! [`crate::terminal_core::TerminalCore::handle_osc_internal`]
//! (`osc_handler.rs`):
//!
//! - APC frame -> the raw APC payload bytes.
//! - OSC 9999 frame whose data starts with `emterm-mux;` -> that data string as
//!   an APC-equivalent payload (the same bytes the existing
//!   `fire_apc_callback(data.as_bytes())` path produces, so the OSC 9999
//!   (Windows ConPTY) transport does not regress).
//! - Any other OSC -> discarded (the outer mux stream carries no other
//!   meaningful OSC).
//!
//! Parser state carries across calls: a frame split across two [`Self::feed`]
//! calls reassembles into one payload.

use crate::parser::Parser;
use crate::parser_types::ParsedAction;

/// The OSC parameter carrying mux inband frames over the OSC fallback
/// transport (Windows ConPTY strips APC but passes OSC).
///
/// term_core-internal SSOT for the mux transport constants: both this
/// extractor and `osc_handler.rs::handle_osc_internal` key off these, so the
/// value lives in exactly one place inside the crate. The cross-crate SSOT is
/// `mux_ipc::protocol::MUX_OSC_PARAM`; the `drift_*` tests below assert these
/// stay in lockstep so a protocol change there fails term_core's test suite.
pub(crate) const MUX_OSC_PARAM: u16 = 9999;

/// The inband mux frame prefix shared by both the APC and OSC 9999 transports.
///
/// Mirrors `mux_ipc::protocol::APC_PREFIX` (cross-crate SSOT); kept in lockstep
/// by the `drift_*` tests below.
pub(crate) const MUX_PREFIX: &str = "emterm-mux;";

/// Independent transport extractor wrapping its own [`Parser`].
///
/// Feed it the coalesced PTS bytes of a mux-attached tab; it returns the
/// mux-APC payloads found, retaining any partial frame for the next feed.
#[derive(Debug, Default)]
pub struct MuxApcExtractor {
    parser: Parser,
}

impl MuxApcExtractor {
    /// Create an extractor with a fresh, independent parser.
    pub fn new() -> Self {
        Self {
            parser: Parser::new(),
        }
    }

    /// Reset the parser to a clean Ground state, dropping any partial frame.
    ///
    /// Called on detach so a subsequent re-attach (or the pre-mux branch
    /// resuming) does not inherit a half-parsed sequence.
    pub fn reset(&mut self) {
        self.parser.reset();
    }

    /// Drive the independent parser over `input` and collect the mux-APC
    /// payloads found:
    ///
    /// - every `ApcDispatch` payload (raw bytes), and
    /// - every OSC 9999 dispatch whose data starts with `emterm-mux;`,
    ///   normalized to its data string as bytes.
    ///
    /// All other actions (Print / CSI / Esc / Execute / DCS / other OSC) are
    /// discarded. Parser state is preserved across calls, so a frame split
    /// across feeds reassembles into a single payload.
    pub fn feed(&mut self, input: &[u8]) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();
        // `parse_interruptible` is the non-test public parse entry point; the
        // closure always returns `true` so the whole slice is consumed.
        self.parser.parse_interruptible(input, |action| {
            match action {
                ParsedAction::ApcDispatch(payload) => out.push(payload),
                ParsedAction::OscDispatch { param, data }
                    if param == MUX_OSC_PARAM && data.starts_with(MUX_PREFIX) =>
                {
                    out.push(data.into_bytes());
                }
                _ => {}
            }
            true
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SSOT drift guard ────────────────────────────────────────────────
    // The mux transport constants are duplicated across layers (mux_ipc owns
    // the cross-crate SSOT; term_core keeps its own copy because it cannot take
    // an upward dependency on mux_ipc in production). These tests fail if the
    // term_core copy ever drifts from `mux_ipc::protocol`, so a protocol change
    // there can no longer silently break the OSC 9999 / APC mux transports.
    #[test]
    fn drift_osc_param_matches_mux_ipc_ssot() {
        assert_eq!(MUX_OSC_PARAM, mux_ipc::protocol::MUX_OSC_PARAM);
    }

    #[test]
    fn drift_prefix_matches_mux_ipc_ssot() {
        assert_eq!(MUX_PREFIX, mux_ipc::protocol::APC_PREFIX);
    }

    /// ESC _ <payload> ESC \  (APC frame).
    fn apc_frame(payload: &str) -> Vec<u8> {
        let mut v = vec![0x1b, b'_'];
        v.extend_from_slice(payload.as_bytes());
        v.extend_from_slice(&[0x1b, b'\\']);
        v
    }

    /// ESC ] 9999 ; <data> ESC \  (OSC frame, ST-terminated).
    fn osc_frame(param: u16, data: &str) -> Vec<u8> {
        let mut v = vec![0x1b, b']'];
        v.extend_from_slice(param.to_string().as_bytes());
        v.push(b';');
        v.extend_from_slice(data.as_bytes());
        v.extend_from_slice(&[0x1b, b'\\']);
        v
    }

    // ── TS-1: complete APC frame in one feed ─────────────────────────────
    #[test]
    fn ts1_complete_apc_frame_returned_intact() {
        let mut ex = MuxApcExtractor::new();
        let frame = apc_frame("emterm-mux;SGVsbG8=");
        let out = ex.feed(&frame);
        assert_eq!(out, vec![b"emterm-mux;SGVsbG8=".to_vec()]);
    }

    // ── TS-2: APC frame split across two feeds reassembles ───────────────
    #[test]
    fn ts2_apc_frame_split_across_feeds_reassembles() {
        let mut ex = MuxApcExtractor::new();
        let frame = apc_frame("emterm-mux;AAAABBBBCCCCDDDD");
        // Split mid-payload — the second feed must complete the same frame.
        let split = 10;
        let out1 = ex.feed(&frame[..split]);
        assert!(out1.is_empty(), "no complete frame yet on the first half");
        let out2 = ex.feed(&frame[split..]);
        assert_eq!(out2, vec![b"emterm-mux;AAAABBBBCCCCDDDD".to_vec()]);
    }

    #[test]
    fn ts2_apc_frame_split_inside_introducer_reassembles() {
        // The split lands between ESC and `_` (mid-introducer) — the most
        // hostile boundary for a shared parser.
        let mut ex = MuxApcExtractor::new();
        let frame = apc_frame("emterm-mux;Zm9v");
        let out1 = ex.feed(&frame[..1]); // just ESC
        assert!(out1.is_empty());
        let out2 = ex.feed(&frame[1..]);
        assert_eq!(out2, vec![b"emterm-mux;Zm9v".to_vec()]);
    }

    // ── TS-3: OSC 9999 emterm-mux; normalized to APC payload form ────────
    #[test]
    fn ts3_osc_9999_emterm_mux_normalized_to_apc_payload() {
        let mut ex = MuxApcExtractor::new();
        let frame = osc_frame(9999, "emterm-mux;Zm9vYmFy");
        let out = ex.feed(&frame);
        // Same payload form the APC transport produces (parity with
        // handle_osc_internal's fire_apc_callback(data.as_bytes())).
        assert_eq!(out, vec![b"emterm-mux;Zm9vYmFy".to_vec()]);
    }

    #[test]
    fn ts3_osc_9999_bel_terminated_also_normalized() {
        // OSC may terminate with BEL (0x07) instead of ST.
        let mut ex = MuxApcExtractor::new();
        let mut frame = vec![0x1b, b']'];
        frame.extend_from_slice(b"9999;emterm-mux;QQ==");
        frame.push(0x07);
        let out = ex.feed(&frame);
        assert_eq!(out, vec![b"emterm-mux;QQ==".to_vec()]);
    }

    #[test]
    fn ts3_osc_9999_non_mux_discarded() {
        // OSC 9999 that is NOT an emterm-mux frame must be dropped (parity:
        // handle_osc_internal ignores non-emterm-mux OSC 9999).
        let mut ex = MuxApcExtractor::new();
        let frame = osc_frame(9999, "something-else;data");
        assert!(ex.feed(&frame).is_empty());
    }

    #[test]
    fn ts3_other_osc_discarded() {
        // A title OSC (OSC 0) is not transport — discard it.
        let mut ex = MuxApcExtractor::new();
        let frame = osc_frame(0, "my title");
        assert!(ex.feed(&frame).is_empty());
    }

    // ── Non-transport output (Print etc.) discarded ──────────────────────
    #[test]
    fn print_and_csi_discarded_apc_kept() {
        let mut ex = MuxApcExtractor::new();
        let mut input = b"hello world".to_vec();
        input.extend_from_slice(b"\x1b[31m"); // SGR red (CSI)
        input.extend_from_slice(&apc_frame("emterm-mux;UEFZ"));
        input.extend_from_slice(b"more text\r\n"); // print + execute
        let out = ex.feed(&input);
        assert_eq!(out, vec![b"emterm-mux;UEFZ".to_vec()]);
    }

    #[test]
    fn multiple_apc_frames_in_one_feed() {
        let mut ex = MuxApcExtractor::new();
        let mut input = apc_frame("emterm-mux;AA==");
        input.extend_from_slice(b"interleaved text");
        input.extend_from_slice(&apc_frame("emterm-mux;BB=="));
        let out = ex.feed(&input);
        assert_eq!(
            out,
            vec![b"emterm-mux;AA==".to_vec(), b"emterm-mux;BB==".to_vec()]
        );
    }

    #[test]
    fn reset_drops_partial_frame() {
        let mut ex = MuxApcExtractor::new();
        let frame = apc_frame("emterm-mux;partial");
        let split = 8;
        let _ = ex.feed(&frame[..split]); // leave parser mid-frame
        ex.reset();
        // After reset, the remainder is parsed from Ground — it begins
        // mid-payload (no introducer), so it does NOT produce a mux frame.
        let out = ex.feed(&frame[split..]);
        assert!(
            out.is_empty(),
            "reset must drop the partial frame, not resume it"
        );
    }

    #[test]
    fn non_emterm_apc_payload_still_surfaced() {
        // A bare Kitty APC (no emterm-mux; prefix) is still surfaced as an APC
        // payload — partition_apc_for_mux routes it to the image pipeline.
        let mut ex = MuxApcExtractor::new();
        let frame = apc_frame("Gf=24,s=1,v=1;AAAA");
        let out = ex.feed(&frame);
        assert_eq!(out, vec![b"Gf=24,s=1,v=1;AAAA".to_vec()]);
    }
}
