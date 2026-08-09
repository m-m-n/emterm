//! Input path of [`Tab`]: writes to the PTY (raw, key input, paste,
//! device responses), mux control frames, and the device-query detector
//! used by the output coalescer.

use mux_ipc::protocol::{MessageType, MuxMessage};

use super::Tab;

impl Tab {
    pub fn write(&self, bytes: Vec<u8>) {
        #[cfg(test)]
        self.outbound_write_log.lock().push(bytes.clone());
        if let Some(p) = &self.pty {
            p.write(bytes);
        }
    }

    /// Send user input (keystrokes / paste / IME commits) to the active pane.
    ///
    /// In mux mode the `emterm mux` bridge **drops raw stdin bytes** (only
    /// APC-framed mux messages are relayed to the daemon — see
    /// `src-tauri/src/mux/bridge.rs`), so input must be wrapped as a
    /// `PtyInput` frame carrying the active pane id (parity with the WebView
    /// `MuxClient.sendInput`). Outside mux mode this is a plain raw PTY write.
    pub fn write_input(&self, bytes: Vec<u8>) {
        // Two-step gate so a *half-attached* state (mux session present but the
        // window group is None / unseeded, or seeded but with no active pane)
        // does not silently fall back to raw PTY write — which the bridge
        // would then drop, leaving the user staring at a mux-badged tab that
        // ignores keystrokes / IME commits / paste. When attached we always
        // take the PtyInput path; if no active pane id is yet available we log
        // and drop so the failure mode is visible during development instead
        // of silently swallowed.
        if self.mux_session_name.is_some() {
            match self.mux_group.as_ref().and_then(|g| g.active_pane_id()) {
                Some(pane_id) => {
                    self.send_control(&MuxMessage {
                        msg_type: MessageType::PtyInput,
                        pane_id,
                        payload: bytes,
                    });
                }
                None => {
                    log::warn!(
                        "mux: write_input dropped {} bytes — tab {:?} attached \
                         but no active pane id (group not yet seeded)",
                        bytes.len(),
                        self.title
                    );
                }
            }
        } else {
            self.write(bytes);
        }
    }

    /// Route a terminal-generated device response (DSR/DA/XTWINOPS reply from
    /// `term_core`) back to the active pane, mirroring `write_input`'s routing
    /// decision.
    ///
    /// In mux mode the `emterm mux` bridge **drops raw stdin bytes**, so the
    /// response must be wrapped as a `PtyInput` frame — identical to how user
    /// keystrokes are routed. Outside mux mode a plain raw PTY write is used.
    ///
    /// The two-step gate from `write_input` is replicated here: when attached
    /// but no active pane id is yet available the bytes are dropped with a
    /// warning (observable failure mode) rather than falling back to a raw
    /// write that the bridge would silently discard anyway.
    pub(super) fn write_device_response(&self, bytes: Vec<u8>) {
        if self.mux_session_name.is_some() {
            match self.mux_group.as_ref().and_then(|g| g.active_pane_id()) {
                Some(pane_id) => {
                    self.send_control(&MuxMessage {
                        msg_type: MessageType::PtyInput,
                        pane_id,
                        payload: bytes,
                    });
                }
                None => {
                    log::warn!(
                        "mux: write_device_response dropped {} bytes — tab {:?} attached \
                         but no active pane id (group not yet seeded)",
                        bytes.len(),
                        self.title
                    );
                }
            }
        } else {
            self.write(bytes);
        }
    }

    /// Paste-aware input: bracketed-paste-wrap (DECSET 2004) then route via
    /// [`Self::write_input`] so the paste reaches the active mux pane too.
    pub fn write_paste_input(&self, text: &str, bracketed: bool) {
        let wrapped = crate::selection::bracketed_paste(text, bracketed);
        self.write_input(wrapped.into_bytes());
    }

    /// Send a structured mux control message to the daemon by APC-encoding it
    /// and writing the bytes to this tab's PTY (fire-and-forget). The
    /// `emterm mux` bridge running in the PTY relays the frame to the daemon
    /// over its Unix socket; native-poc never opens that socket (NFR2).
    /// Responses arrive as inbound APC through the existing decode route.
    ///
    /// Port of the WebView `MuxClient.sendControl` (`writeDirect`). Returns
    /// `false` when the tab has no live PTY (the message is dropped).
    pub fn send_control(&self, msg: &MuxMessage) -> bool {
        #[cfg(test)]
        self.outbound_write_log.lock().push(msg.payload.clone());
        let bytes = crate::mux::apc::encode_emterm_mux(msg);
        match &self.pty {
            Some(p) => {
                p.write(bytes);
                true
            }
            None => {
                log::warn!(
                    "mux: send_control({:?}) dropped — tab {:?} has no PTY",
                    msg.msg_type,
                    self.title
                );
                false
            }
        }
    }
}

/// True when `payload` contains a complete CSI device query that `term_core`
/// answers by appending to its ordered pending-response store (task0002 D5).
/// The set is kept in lockstep with the response-synthesizing arms of
/// `crates/term_core/src/csi_dispatch.rs`: final byte `n` (DSR), `c` (Device
/// Attributes), `t` (XTWINOPS size reports), or `p` (DECRPM `CSI ? Ps $ p`).
/// Detection is intentionally conservative — it matches on the final byte
/// alone, so a few non-response sequences sharing those finals (e.g. DA3
/// `CSI = c`, non-size XTWINOPS ops, a non-DECRPM `p`) are also treated as
/// queries. The only cost of a false positive is parsing that one frame on
/// its own instead of coalescing it; correctness is unaffected.
///
/// Used by [`Tab::pty_output_batch_eligible`] to keep query-bearing
/// `PtyOutput` frames OUT of the coalesce accumulator. This is now a
/// LATENCY/isolation choice rather than a correctness requirement:
/// `term_core`'s ordered pending-response store (task0002 D5) no longer
/// loses replies when several query frames are concatenated into one parse
/// — `take_response` drains every reply, in order, regardless of how many
/// queries a single `process_pty_data_fully` call answered. Parsing a
/// query-bearing frame on its own keeps its reply from waiting behind an
/// unrelated coalesce run and matches the pre-coalesce per-frame timing
/// byte-for-byte; task0002 leaves this gate's behavior unchanged (out of
/// scope — see that task's plan), only its rationale no longer includes
/// "or a reply is lost".
///
/// A CSI starts at `ESC [` (`0x1b 0x5b`); parameter bytes are `0x30..=0x3f`,
/// intermediate bytes `0x20..=0x2f`, and the final byte is `0x40..=0x7e`. A C0
/// control byte other than `ESC` appearing mid-CSI is executed by `term_core`'s
/// parser without aborting the sequence, so it is skipped here too (the CSI
/// keeps accumulating). A CSI left incomplete at the end of the payload is NOT a
/// complete query (it would complete in a later frame, where it still yields a
/// single reply — no loss), so it does not force a split.
pub(super) fn payload_has_device_query(payload: &[u8]) -> bool {
    let n = payload.len();
    let mut i = 0;
    while i + 1 < n {
        if payload[i] == 0x1b && payload[i + 1] == b'[' {
            // Scan the CSI body for its final byte.
            let mut j = i + 2;
            loop {
                if j >= n {
                    // Incomplete CSI runs to the end of the payload — not a
                    // complete query, and nothing complete can follow it.
                    return false;
                }
                let b = payload[j];
                if (0x40..=0x7e).contains(&b) {
                    // Final byte: device-response producers per term_core.
                    if matches!(b, b'n' | b'c' | b't' | b'p') {
                        return true;
                    }
                    i = j + 1; // resume past this non-query CSI
                    break;
                }
                if matches!(b, 0x00..=0x1a | 0x1c..=0x1f) {
                    // A C0 control byte (other than ESC) mid-CSI is executed by
                    // `term_core`'s parser WITHOUT aborting the CSI — the
                    // sequence keeps accumulating after it (see
                    // crates/term_core/src/parser/csi.rs). So skip it and keep
                    // scanning this CSI for its final byte; e.g. `\x1b[\x076n`
                    // still fires a CPR and must be detected. ESC (0x1b) is the
                    // genuine new-sequence boundary, handled by the resync below.
                    j += 1;
                    continue;
                }
                if !(0x20..=0x3f).contains(&b) {
                    // Neither a CSI body byte, a C0 control, nor a final: this
                    // CSI is malformed (e.g. an `ESC` starting a new sequence, or
                    // a 0x7f / 0x80..=0xff byte). Re-examine the offending byte
                    // rather than skipping it — it may itself begin a new CSI
                    // (e.g. the `ESC` that starts the real query right after a
                    // truncated one, `\x1b[2\x1b[6n`).
                    i = j;
                    break;
                }
                j += 1;
            }
            continue;
        }
        i += 1;
    }
    false
}
