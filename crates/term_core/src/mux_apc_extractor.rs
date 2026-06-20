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
//! Print / CSI / Esc / Execute / DCS. Both transports normalize to the same
//! APC payload form, so the OSC fallback (used on Windows ConPTY, which strips
//! APC but passes OSC) does not regress:
//!
//! - APC frame -> the raw APC payload bytes.
//! - OSC frame whose param equals the injected `osc_param` and whose data
//!   starts with the injected `prefix` -> that data string as an
//!   APC-equivalent payload.
//! - Any other OSC -> discarded (the outer mux stream carries no other
//!   meaningful OSC).
//!
//! The mux application-protocol values (`osc_param`, `prefix`) are injected by
//! the caller (`mux_ipc::protocol`); `term_core` itself holds no mux protocol
//! constant (NFR5).
//!
//! Parser state carries across calls: a frame split across two [`Self::feed`]
//! calls reassembles into one payload.

use crate::parser::Parser;
use crate::parser_types::ParsedAction;

/// Independent transport extractor wrapping its own [`Parser`].
///
/// Feed it the coalesced PTS bytes of a mux-attached tab; it returns the
/// mux-APC payloads found, retaining any partial frame for the next feed.
///
/// `term_core` knows nothing about the mux application protocol: the OSC
/// fallback parameter and the inband frame prefix are supplied by the caller
/// at construction (the cross-crate SSOT is `mux_ipc::protocol::{MUX_OSC_PARAM,
/// APC_PREFIX}`), so no protocol constant lives in this crate (NFR5).
#[derive(Debug)]
pub struct MuxApcExtractor {
    parser: Parser,
    /// OSC parameter carrying mux inband frames over the OSC fallback
    /// transport (Windows ConPTY strips APC but passes OSC). Injected by the
    /// caller from `mux_ipc::protocol::MUX_OSC_PARAM`.
    osc_param: u16,
    /// Inband mux frame prefix shared by both the APC and OSC fallback
    /// transports. Injected by the caller from `mux_ipc::protocol::APC_PREFIX`.
    prefix: &'static str,
}

impl MuxApcExtractor {
    /// Create an extractor with a fresh, independent parser.
    ///
    /// `osc_param` and `prefix` carry the mux application-protocol values from
    /// the caller (`mux_ipc::protocol`), keeping `term_core` ignorant of the
    /// protocol. There is intentionally no `Default`: a default cannot know
    /// the protocol values.
    pub fn new(osc_param: u16, prefix: &'static str) -> Self {
        Self {
            parser: Parser::new(),
            osc_param,
            prefix,
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
        self.feed_with_offsets(input)
            .into_iter()
            .map(|(payload, _end)| payload)
            .collect()
    }

    /// Like [`Self::feed`], but each returned payload is paired with the byte
    /// offset in `input` just past the frame that produced it (an exclusive
    /// end index into `input`).
    ///
    /// This lets the caller locate the boundary of the frame that triggered a
    /// state transition (e.g. a `Detached` control frame) so the bytes that
    /// follow it in the SAME coalesced buffer can be routed elsewhere instead
    /// of being discarded by this transport-only extractor (FR5).
    ///
    /// A frame split across feeds reports its end offset relative to the feed
    /// in which it completes; the offset is always within `0..=input.len()`.
    pub fn feed_with_offsets(&mut self, input: &[u8]) -> Vec<(Vec<u8>, usize)> {
        let mut out: Vec<(Vec<u8>, usize)> = Vec::new();
        // Single bulk pass over `input`: `parse_with_offsets` drives the parser
        // once over the whole slice and hands the closure each action together
        // with the exclusive end offset (`i + 1`) of the byte that produced it.
        // This avoids re-entering the parser per byte — the mux pump coalesces
        // up to 1 MiB per frame, so a per-byte loop here reintroduced the
        // per-unit overhead `pump`'s coalescing exists to eliminate.
        //
        // `osc_param` / `prefix` are copied out so the closure does not borrow
        // `self` while `self.parser` is mutably borrowed by the parse call.
        let osc_param = self.osc_param;
        let prefix = self.prefix;
        self.parser
            .parse_with_offsets(input, |action, end| match action {
                ParsedAction::ApcDispatch(payload) => out.push((payload, end)),
                ParsedAction::OscDispatch { param, data }
                    if param == osc_param && data.starts_with(prefix) =>
                {
                    out.push((data.into_bytes(), end));
                }
                _ => {}
            });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mux protocol values the production caller (`tabs.rs`) injects from
    /// `mux_ipc::protocol`. The tests construct the extractor with these so the
    /// crate stays ignorant of the protocol while still exercising the real
    /// param + prefix.
    fn new_test() -> MuxApcExtractor {
        MuxApcExtractor::new(
            mux_ipc::protocol::MUX_OSC_PARAM,
            mux_ipc::protocol::APC_PREFIX,
        )
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
        let mut ex = new_test();
        let frame = apc_frame("emterm-mux;SGVsbG8=");
        let out = ex.feed(&frame);
        assert_eq!(out, vec![b"emterm-mux;SGVsbG8=".to_vec()]);
    }

    // ── TS-2: APC frame split across two feeds reassembles ───────────────
    #[test]
    fn ts2_apc_frame_split_across_feeds_reassembles() {
        let mut ex = new_test();
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
        let mut ex = new_test();
        let frame = apc_frame("emterm-mux;Zm9v");
        let out1 = ex.feed(&frame[..1]); // just ESC
        assert!(out1.is_empty());
        let out2 = ex.feed(&frame[1..]);
        assert_eq!(out2, vec![b"emterm-mux;Zm9v".to_vec()]);
    }

    // ── TS-3: OSC 9999 emterm-mux; normalized to APC payload form ────────
    #[test]
    fn ts3_osc_9999_emterm_mux_normalized_to_apc_payload() {
        let mut ex = new_test();
        let frame = osc_frame(9999, "emterm-mux;Zm9vYmFy");
        let out = ex.feed(&frame);
        // Same payload form the APC transport produces (parity with
        // handle_osc_internal's fire_apc_callback(data.as_bytes())).
        assert_eq!(out, vec![b"emterm-mux;Zm9vYmFy".to_vec()]);
    }

    #[test]
    fn ts3_osc_9999_bel_terminated_also_normalized() {
        // OSC may terminate with BEL (0x07) instead of ST.
        let mut ex = new_test();
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
        let mut ex = new_test();
        let frame = osc_frame(9999, "something-else;data");
        assert!(ex.feed(&frame).is_empty());
    }

    #[test]
    fn ts3_other_osc_discarded() {
        // A title OSC (OSC 0) is not transport — discard it.
        let mut ex = new_test();
        let frame = osc_frame(0, "my title");
        assert!(ex.feed(&frame).is_empty());
    }

    // ── Non-transport output (Print etc.) discarded ──────────────────────
    #[test]
    fn print_and_csi_discarded_apc_kept() {
        let mut ex = new_test();
        let mut input = b"hello world".to_vec();
        input.extend_from_slice(b"\x1b[31m"); // SGR red (CSI)
        input.extend_from_slice(&apc_frame("emterm-mux;UEFZ"));
        input.extend_from_slice(b"more text\r\n"); // print + execute
        let out = ex.feed(&input);
        assert_eq!(out, vec![b"emterm-mux;UEFZ".to_vec()]);
    }

    #[test]
    fn multiple_apc_frames_in_one_feed() {
        let mut ex = new_test();
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
        let mut ex = new_test();
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

    // ── feed_with_offsets: per-frame end offset reporting (FR5) ──────────
    #[test]
    fn feed_with_offsets_reports_frame_end() {
        let mut ex = new_test();
        let frame = apc_frame("emterm-mux;Zm9v");
        // [frame][trailing bytes] — the offset must point just past the frame.
        let mut input = frame.clone();
        input.extend_from_slice(b"trailing");
        let out = ex.feed_with_offsets(&input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, b"emterm-mux;Zm9v".to_vec());
        // The frame ends at the `ESC \` terminator; the tail starts right after.
        assert_eq!(out[0].1, frame.len());
        assert_eq!(&input[out[0].1..], b"trailing");
    }

    #[test]
    fn feed_with_offsets_multiple_frames_distinct_offsets() {
        let mut ex = new_test();
        let f1 = apc_frame("emterm-mux;AA==");
        let f2 = apc_frame("emterm-mux;BB==");
        let mut input = f1.clone();
        input.extend_from_slice(b"mid");
        input.extend_from_slice(&f2);
        let out = ex.feed_with_offsets(&input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1, f1.len());
        assert_eq!(out[1].1, input.len());
    }

    #[test]
    fn feed_delegates_to_feed_with_offsets() {
        // The payload-only `feed` must stay byte-for-byte equivalent to
        // dropping the offsets from `feed_with_offsets`.
        let mut a = new_test();
        let mut b = new_test();
        let frame = apc_frame("emterm-mux;UEFZ");
        let with: Vec<Vec<u8>> = a
            .feed_with_offsets(&frame)
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        let plain = b.feed(&frame);
        assert_eq!(with, plain);
    }

    // ── TS-12: injected param + prefix are actually used (not hardcoded) ──
    #[test]
    fn ts12_injected_osc_param_and_prefix_are_used() {
        // Construct with arbitrary values that differ from the production
        // `mux_ipc::protocol` defaults (9999 / "emterm-mux;"). The extractor
        // must key off the *injected* values, proving `term_core` holds no
        // hardcoded mux protocol constant (NFR5).
        let mut ex = MuxApcExtractor::new(1234, "myprefix;");

        // An OSC frame matching the injected param + prefix is extracted,
        // normalized to its data string as an APC-equivalent payload.
        let matching = osc_frame(1234, "myprefix;Zm9v");
        assert_eq!(ex.feed(&matching), vec![b"myprefix;Zm9v".to_vec()]);

        // A frame using the *default* production param/prefix (9999 /
        // "emterm-mux;") — which this extractor was NOT given — is discarded.
        let default_form = osc_frame(9999, "emterm-mux;Zm9v");
        assert!(
            ex.feed(&default_form).is_empty(),
            "an OSC frame with the default param/prefix must NOT match an \
             extractor injected with a different param/prefix"
        );

        // The injected prefix gate also rejects the right param with a
        // different prefix.
        let wrong_prefix = osc_frame(1234, "other;Zm9v");
        assert!(ex.feed(&wrong_prefix).is_empty());
    }

    #[test]
    fn non_emterm_apc_payload_still_surfaced() {
        // A bare Kitty APC (no emterm-mux; prefix) is still surfaced as an APC
        // payload — partition_apc_for_mux routes it to the image pipeline.
        let mut ex = new_test();
        let frame = apc_frame("Gf=24,s=1,v=1;AAAA");
        let out = ex.feed(&frame);
        assert_eq!(out, vec![b"Gf=24,s=1,v=1;AAAA".to_vec()]);
    }
}
