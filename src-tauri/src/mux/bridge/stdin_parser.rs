//! Stdin APC parser: the INPUT-direction state machine that extracts
//! mux frames (APC and Plaintext `EMUX;` transports) from the raw
//! stdin byte stream and classifies everything else as passthrough.

use mux_ipc::protocol::{APC_PREFIX, MUX_OSC_PARAM, MuxMessage, PLAINTEXT_PREFIX};

use super::Transport;

/// Actions produced by the stdin mux parser.
#[derive(Debug)]
#[allow(dead_code)]
pub(super) enum StdinAction {
    /// A decoded mux message to forward to the daemon, with transport info.
    MuxMessage(MuxMessage, Transport),
    /// Passthrough data (non-mux bytes).
    Passthrough(Vec<u8>),
}

/// Maximum APC payload size (matches MAX_FRAME_LENGTH after Base64 expansion).
/// Base64 expands by ~4/3, so 22MB covers the 16MB frame limit.
const MAX_APC_PAYLOAD: usize = 22 * 1024 * 1024;

// `PLAINTEXT_PREFIX` (and `APC_PREFIX` / `MUX_OSC_PARAM`) come from the
// `mux_ipc::protocol` SSOT, so all three mux transport markers have a
// single owner.

/// State machine that separates APC/OSC/plaintext mux sequences from passthrough data on stdin.
///
/// Handles partial reads across buffer boundaries.
/// Recognizes APC (ESC _), OSC 9999 (ESC ]), and plaintext (`EMUX;<base64>\r`,
/// also accepting LF / CRLF / LFCR) mux sequences.
///
/// This is the bridge subprocess's INPUT-direction scanner and is intentionally
/// separate from `term_core::MuxApcExtractor` (the GUI's OUTPUT-direction outer
/// parse): only this side handles the Plaintext `EMUX;` transport and forwards
/// non-mux bytes as passthrough. Both lean on the same `mux_ipc::protocol`
/// markers and `MuxMessage::from_apc` decode, so the wire format has a single
/// SSOT even though the two byte-level state machines are not shared.
pub(super) struct StdinApcParser {
    state: ParserState,
    apc_buf: Vec<u8>,
    passthrough_buf: Vec<u8>,
    /// Accumulator for OSC numeric parameter.
    osc_param_accum: u16,
    /// Number of digits accumulated for the OSC parameter (to reconstruct on rejection).
    osc_param_digits: Vec<u8>,
    /// Whether the current sequence entered via the OSC path (ESC ] 9999 ;).
    is_osc: bool,
    /// Number of bytes matched in the EMUX; prefix so far.
    plaintext_prefix_matched: usize,
    /// True iff a Plaintext message just completed and the next byte, if
    /// it is the partner half of a CRLF / LFCR terminator pair, must be
    /// dropped silently (not passthrough'd). Reset by any non-EOL byte.
    swallow_partner_eol: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    /// Normal text / passthrough mode.
    Ground,
    /// Seen ESC, waiting for _ (APC start), ] (OSC start), or \ (APC end inside accumulation).
    EscSeen,
    /// Inside OSC parameter accumulation (digits after ESC ]).
    InOscParam,
    /// Inside APC/OSC body accumulation.
    InApc,
    /// Inside APC/OSC body, seen ESC (could be ST = ESC \).
    InApcEsc,
    /// Matching EMUX; prefix (plaintext_prefix_matched tracks position).
    InPlaintextPrefix,
    /// Inside plaintext body accumulation (after EMUX; prefix matched).
    InPlaintext,
}

impl StdinApcParser {
    pub fn new() -> Self {
        Self {
            state: ParserState::Ground,
            apc_buf: Vec::new(),
            passthrough_buf: Vec::new(),
            osc_param_accum: 0,
            osc_param_digits: Vec::new(),
            is_osc: false,
            plaintext_prefix_matched: 0,
            swallow_partner_eol: false,
        }
    }

    /// Complete a plaintext mux sequence (`EMUX;<base64>\r`; also accepts
    /// LF / CRLF / LFCR — see the `InPlaintext` arm in [`Self::feed`]).
    fn complete_plaintext_sequence(apc_buf: &mut Vec<u8>, actions: &mut Vec<StdinAction>) {
        let payload_str = String::from_utf8_lossy(apc_buf).to_string();
        apc_buf.clear();

        // The buf contains the base64 data (after EMUX; prefix, before the
        // CR/LF terminator). Wrap it with APC_PREFIX so from_apc can decode it.
        let with_prefix = format!("{}{}", APC_PREFIX, payload_str);
        match MuxMessage::from_apc(&with_prefix) {
            Ok(msg) => actions.push(StdinAction::MuxMessage(msg, Transport::Plaintext)),
            Err(e) => {
                eprintln!("Bridge: plaintext mux decode error: {}", e);
            }
        }
    }

    /// Complete a mux sequence: decode the accumulated APC buffer and produce an action.
    fn complete_mux_sequence(apc_buf: &mut Vec<u8>, is_osc: bool, actions: &mut Vec<StdinAction>) {
        let payload = String::from_utf8_lossy(apc_buf).to_string();
        apc_buf.clear();

        let transport = if is_osc {
            Transport::Osc
        } else {
            Transport::Apc
        };

        if payload.starts_with(APC_PREFIX) {
            match MuxMessage::from_apc(&payload) {
                Ok(msg) => actions.push(StdinAction::MuxMessage(msg, transport)),
                Err(e) => {
                    eprintln!("Bridge: mux decode error: {}", e);
                }
            }
        } else if !is_osc {
            // Non-mux APC: forward as passthrough (ESC_ + body + ESC\)
            let mut pdata = Vec::with_capacity(2 + payload.len() + 2);
            pdata.extend_from_slice(b"\x1b_");
            pdata.extend_from_slice(payload.as_bytes());
            pdata.extend_from_slice(b"\x1b\\");
            actions.push(StdinAction::Passthrough(pdata));
        }
        // Non-mux OSC with param 9999 shouldn't happen (we only enter InApc for 9999),
        // but if the body doesn't have APC_PREFIX, just discard it.
    }

    /// Feed bytes into the parser and return resulting actions.
    pub fn feed(&mut self, data: &[u8]) -> Vec<StdinAction> {
        let mut actions = Vec::new();

        for &byte in data {
            match self.state {
                ParserState::Ground => {
                    // Drop the partner half of a CRLF / LFCR terminator that
                    // just closed a Plaintext message, so a trailing CR or LF
                    // doesn't leak out as passthrough. Any non-EOL byte
                    // immediately disarms the swallow.
                    if self.swallow_partner_eol {
                        self.swallow_partner_eol = false;
                        if byte == b'\r' || byte == b'\n' {
                            continue;
                        }
                    }
                    if byte == 0x1B {
                        self.state = ParserState::EscSeen;
                    } else if byte == PLAINTEXT_PREFIX[0] {
                        // Potential start of EMUX; prefix
                        if !self.passthrough_buf.is_empty() {
                            actions.push(StdinAction::Passthrough(std::mem::take(
                                &mut self.passthrough_buf,
                            )));
                        }
                        self.plaintext_prefix_matched = 1;
                        self.state = ParserState::InPlaintextPrefix;
                    } else {
                        self.passthrough_buf.push(byte);
                    }
                }
                ParserState::EscSeen => {
                    if byte == b'_' {
                        // APC start: flush passthrough first
                        if !self.passthrough_buf.is_empty() {
                            actions.push(StdinAction::Passthrough(std::mem::take(
                                &mut self.passthrough_buf,
                            )));
                        }
                        self.state = ParserState::InApc;
                        self.apc_buf.clear();
                        self.is_osc = false;
                    } else if byte == b']' {
                        // OSC start: flush passthrough and begin parameter accumulation
                        if !self.passthrough_buf.is_empty() {
                            actions.push(StdinAction::Passthrough(std::mem::take(
                                &mut self.passthrough_buf,
                            )));
                        }
                        self.osc_param_accum = 0;
                        self.osc_param_digits.clear();
                        self.state = ParserState::InOscParam;
                    } else {
                        // Not APC/OSC start: treat ESC + byte as passthrough
                        self.passthrough_buf.push(0x1B);
                        self.passthrough_buf.push(byte);
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InOscParam => {
                    if byte.is_ascii_digit() {
                        self.osc_param_digits.push(byte);
                        self.osc_param_accum =
                            self.osc_param_accum.saturating_mul(10) + (byte - b'0') as u16;
                    } else if byte == b';' {
                        if self.osc_param_accum == MUX_OSC_PARAM {
                            // Matched OSC 9999; -> reuse APC accumulation for body
                            self.state = ParserState::InApc;
                            self.apc_buf.clear();
                            self.is_osc = true;
                        } else {
                            // Wrong OSC param: push ESC ] <digits> ; as passthrough
                            self.passthrough_buf.push(0x1B);
                            self.passthrough_buf.push(b']');
                            self.passthrough_buf
                                .extend_from_slice(&self.osc_param_digits);
                            self.passthrough_buf.push(b';');
                            self.osc_param_digits.clear();
                            self.state = ParserState::Ground;
                        }
                    } else {
                        // Non-digit, non-semicolon: not a valid OSC param, passthrough
                        self.passthrough_buf.push(0x1B);
                        self.passthrough_buf.push(b']');
                        self.passthrough_buf
                            .extend_from_slice(&self.osc_param_digits);
                        self.passthrough_buf.push(byte);
                        self.osc_param_digits.clear();
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InApc => {
                    if byte == 0x1B {
                        self.state = ParserState::InApcEsc;
                    } else if self.is_osc && byte == 0x07 {
                        // BEL as alternative OSC terminator
                        Self::complete_mux_sequence(&mut self.apc_buf, self.is_osc, &mut actions);
                        self.state = ParserState::Ground;
                    } else if self.apc_buf.len() < MAX_APC_PAYLOAD {
                        self.apc_buf.push(byte);
                    } else {
                        // APC/OSC payload too large: discard and reset to ground
                        eprintln!(
                            "Bridge: APC/OSC payload exceeds {} bytes, discarding",
                            MAX_APC_PAYLOAD
                        );
                        self.apc_buf.clear();
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InApcEsc => {
                    if byte == b'\\' {
                        // ST (ESC \) found: decode the payload
                        Self::complete_mux_sequence(&mut self.apc_buf, self.is_osc, &mut actions);
                        self.state = ParserState::Ground;
                    } else {
                        // ESC inside APC but not followed by \: keep accumulating
                        self.apc_buf.push(0x1B);
                        self.apc_buf.push(byte);
                        self.state = ParserState::InApc;
                    }
                }
                ParserState::InPlaintextPrefix => {
                    let expected = PLAINTEXT_PREFIX[self.plaintext_prefix_matched];
                    if byte == expected {
                        self.plaintext_prefix_matched += 1;
                        if self.plaintext_prefix_matched == PLAINTEXT_PREFIX.len() {
                            // Full prefix matched: start accumulating body
                            self.apc_buf.clear();
                            self.state = ParserState::InPlaintext;
                        }
                    } else {
                        // Prefix mismatch: push matched bytes + current as passthrough
                        self.passthrough_buf
                            .extend_from_slice(&PLAINTEXT_PREFIX[..self.plaintext_prefix_matched]);
                        self.passthrough_buf.push(byte);
                        self.plaintext_prefix_matched = 0;
                        self.state = ParserState::Ground;
                    }
                }
                ParserState::InPlaintext => {
                    if byte == b'\r' || byte == b'\n' {
                        // EITHER CR or LF terminates the plaintext message.
                        // The Windows host writes the envelope with a CR
                        // terminator because portable-pty 0.8 opens ConPTY
                        // with `PSEUDOCONSOLE_WIN32_INPUT_MODE` and raw LF is
                        // not delivered as a real key event on that channel
                        // (CR rides through as VK_RETURN). Intermediate
                        // layers can still substitute or duplicate the
                        // terminator: this branch accepts whichever arrives
                        // first; the Ground state then swallows the partner
                        // half via `swallow_partner_eol` so a trailing
                        // CRLF/LFCR doesn't surface as passthrough.
                        // base64 STANDARD's alphabet contains neither CR nor
                        // LF, so a CR/LF inside the body is always a
                        // terminator (the surrounding handshake is
                        // CR/LF-free).
                        Self::complete_plaintext_sequence(&mut self.apc_buf, &mut actions);
                        self.swallow_partner_eol = true;
                        self.state = ParserState::Ground;
                    } else if self.apc_buf.len() < MAX_APC_PAYLOAD {
                        self.apc_buf.push(byte);
                    } else {
                        eprintln!(
                            "Bridge: plaintext payload exceeds {} bytes, discarding",
                            MAX_APC_PAYLOAD
                        );
                        self.apc_buf.clear();
                        self.state = ParserState::Ground;
                    }
                }
            }
        }

        // Flush remaining passthrough
        if !self.passthrough_buf.is_empty() && self.state == ParserState::Ground {
            actions.push(StdinAction::Passthrough(std::mem::take(
                &mut self.passthrough_buf,
            )));
        }

        actions
    }
}
