//! Stateful scanner that extracts "raw passthrough" sequences from a PTY
//! byte stream while a session is hidden.
//!
//! The shadow VT100 parser updates the screen state but does NOT preserve
//! image / Markdown OSC byte streams (they are decoded into events and the
//! original bytes are gone). The frontend must replay the original bytes
//! on resume to re-render images and rich content, so we sniff them out
//! here and stash them in the per-session passthrough buffer.
//!
//! Three sequence kinds are extracted into the replayable passthrough output:
//! - Kitty graphics APC: `ESC _ G ... ESC \`
//! - SIXEL DCS:          `ESC P ... q ... ESC \`
//! - emterm OSC 9999:    `ESC ] 9999 ; ... ESC \` (Markdown protocol)
//!
//! In addition the scanner recognizes desktop-notification sequences:
//! - OSC 9 notification: `ESC ] 9 ; <message> (BEL | ESC \)`
//!
//! Notifications are NOT replayable passthrough: they are side-effect events
//! that must fire exactly once and must never be added to the resume/replay
//! byte stream (FR5). They are therefore reported through a SEPARATE channel
//! (`take_notifications`) from the replay bytes returned by `process`.
//! `OSC 9 ; 4 ; ...` is a progress-bar sequence and is excluded from
//! notification recognition (FR4).
//!
//! The scanner is stateful so a sequence that crosses a chunk boundary is
//! still recovered. If a partial sequence grows beyond `PARTIAL_SEQUENCE_MAX`
//! the in-flight sequence is dropped (with a single warn) and scanning
//! resumes from the next byte.

use super::visibility::PARTIAL_SEQUENCE_MAX;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// No partial sequence in flight.
    Idle,
    /// Just saw `ESC`. Waiting for the introducer byte.
    AfterEsc,
    /// Inside `ESC _ ...`. Need to disambiguate Kitty (`G` next) vs other APC.
    InApcAwaitingG,
    /// Inside `ESC _ G ...`. Collecting until `ESC \`.
    InKittyApc,
    /// Inside `ESC P ...`. Collecting params; may or may not be SIXEL.
    /// We capture from the leading `ESC P` and only commit on `ESC \`.
    InDcs,
    /// Inside `ESC ] ...`. Collecting until `ESC \` or `BEL`. We only
    /// commit if the param prefix is `9999;`.
    InOsc,
}

pub struct PassthroughScanner {
    state: State,
    /// Bytes accumulated for the in-flight sequence (including the leading
    /// `ESC` introducer). Committed to output on a successful terminator.
    partial: Vec<u8>,
    /// True after we've already warned about a single partial-buffer
    /// overflow; rearmed by `reset_partial`.
    overflow_warned: bool,
    /// Recognized OSC 9 notification messages awaiting delivery. Kept
    /// strictly separate from the replayable passthrough output so they are
    /// fired once and never replayed (FR5). Drained by `take_notifications`.
    notifications: Vec<String>,
}

impl PassthroughScanner {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            partial: Vec::new(),
            overflow_warned: false,
            notifications: Vec::new(),
        }
    }

    /// Drain and return all OSC 9 notification messages recognized since the
    /// last call. These are side-effect events (desktop notifications), NOT
    /// replayable passthrough bytes, so the caller must route them to the
    /// notification sink and must not add them to any resume/replay buffer.
    pub fn take_notifications(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notifications)
    }

    /// Returns the current size of the in-flight partial buffer. Test-only
    /// observer.
    #[cfg(test)]
    pub fn partial_buffer_len(&self) -> usize {
        self.partial.len()
    }

    /// Process `data` and return all completed passthrough sequences
    /// (concatenated in stream order, original bytes including the
    /// introducer and terminator). Returns an empty `Vec` if nothing
    /// completes during this call.
    pub fn process(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &b in data {
            self.step(b, &mut out);
            if self.partial.len() > PARTIAL_SEQUENCE_MAX {
                if !self.overflow_warned {
                    log::warn!(
                        "[WARN][BACKEND] passthrough_scanner: partial buffer exceeded {}B; dropping in-flight sequence",
                        PARTIAL_SEQUENCE_MAX
                    );
                    self.overflow_warned = true;
                }
                self.reset_partial();
            }
        }
        out
    }

    fn step(&mut self, b: u8, out: &mut Vec<u8>) {
        match self.state {
            State::Idle => {
                if b == 0x1b {
                    self.partial.push(b);
                    self.state = State::AfterEsc;
                }
            }
            State::AfterEsc => {
                self.partial.push(b);
                match b {
                    b'_' => {
                        self.state = State::InApcAwaitingG;
                    }
                    b'P' => {
                        self.state = State::InDcs;
                    }
                    b']' => {
                        self.state = State::InOsc;
                    }
                    0x1b => {
                        // ESC ESC: keep the latest ESC as introducer
                        self.partial.clear();
                        self.partial.push(0x1b);
                        // Stay in AfterEsc.
                    }
                    _ => {
                        // Not an introducer we care about.
                        self.reset_partial();
                    }
                }
            }
            State::InApcAwaitingG => {
                self.partial.push(b);
                if b == b'G' {
                    self.state = State::InKittyApc;
                } else {
                    // Not a Kitty APC; abandon.
                    self.reset_partial();
                }
            }
            State::InKittyApc => {
                self.partial.push(b);
                if self.is_st_terminator() {
                    out.extend_from_slice(&self.partial);
                    self.reset_partial();
                }
            }
            State::InDcs => {
                self.partial.push(b);
                if self.is_st_terminator() {
                    // Commit only if it looked like SIXEL (contains 'q' in
                    // the param/intermediate region before any data byte).
                    // Cheap heuristic: any unescaped 'q' in the partial.
                    if self.partial.iter().any(|&c| c == b'q') {
                        out.extend_from_slice(&self.partial);
                    }
                    self.reset_partial();
                }
            }
            State::InOsc => {
                self.partial.push(b);
                let terminated_st = self.is_st_terminator();
                let terminated_bel = b == 0x07;
                if terminated_st || terminated_bel {
                    // partial layout: ESC ] <body...> [terminator]
                    if self.partial.starts_with(b"\x1b]9999;") {
                        // emterm Markdown protocol: replayable passthrough.
                        out.extend_from_slice(&self.partial);
                    } else if self.partial.starts_with(b"\x1b]9;") {
                        // OSC 9 desktop notification (NOT replay; FR5).
                        self.record_osc9_notification(terminated_st, terminated_bel);
                    }
                    self.reset_partial();
                }
            }
        }
    }

    /// Extract the OSC 9 notification message from `self.partial` and, unless
    /// it is a progress sequence (`4;` prefix), queue it for delivery.
    ///
    /// `self.partial` layout at call time is:
    ///   `ESC ] 9 ; <body> <terminator>`
    /// where the terminator is a single BEL byte or the two-byte ST (`ESC \`).
    /// The notification message is `<body>` (the bytes after `9;` up to the
    /// terminator). Invalid UTF-8 in the body is dropped (no notification).
    fn record_osc9_notification(&mut self, terminated_st: bool, terminated_bel: bool) {
        // Prefix length of `ESC ] 9 ;` is 4 bytes.
        const PREFIX_LEN: usize = 4;
        let term_len = if terminated_st {
            2 // ESC \
        } else if terminated_bel {
            1 // BEL
        } else {
            return;
        };
        let total = self.partial.len();
        if total < PREFIX_LEN + term_len {
            return;
        }
        let body = &self.partial[PREFIX_LEN..total - term_len];
        // Progress sequences (`OSC 9 ; 4 ; ...`) are not notifications (FR4).
        if body.starts_with(b"4;") {
            return;
        }
        match std::str::from_utf8(body) {
            Ok(msg) => self.notifications.push(msg.to_string()),
            Err(_) => {
                log::warn!(
                    "[WARN][BACKEND] passthrough_scanner: OSC 9 notification body is not valid UTF-8; dropped"
                );
            }
        }
    }

    fn is_st_terminator(&self) -> bool {
        // ST = ESC \ (0x1B 0x5C). Look at the trailing two bytes of partial.
        let n = self.partial.len();
        n >= 2 && self.partial[n - 2] == 0x1b && self.partial[n - 1] == b'\\'
    }

    fn reset_partial(&mut self) {
        self.partial.clear();
        self.state = State::Idle;
    }
}

impl Default for PassthroughScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        let mut s = PassthroughScanner::new();
        assert!(s.process(b"").is_empty());
    }

    #[test]
    fn plain_text_is_ignored() {
        let mut s = PassthroughScanner::new();
        let out = s.process(b"hello world\nfoo bar\r\n");
        assert!(out.is_empty());
    }

    #[test]
    fn ansi_csi_is_not_extracted() {
        // ESC [ 31 m   is SGR; not a passthrough sequence
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1b[31mred\x1b[0m");
        assert!(out.is_empty());
    }

    #[test]
    fn kitty_apc_is_extracted_in_one_chunk() {
        let mut s = PassthroughScanner::new();
        let payload = b"prefix\x1b_Gi=1,a=T;ABC\x1b\\suffix";
        let out = s.process(payload);
        assert_eq!(out, b"\x1b_Gi=1,a=T;ABC\x1b\\");
    }

    #[test]
    fn sixel_dcs_is_extracted_in_one_chunk() {
        let mut s = PassthroughScanner::new();
        let payload = b"\x1bP1;0;0q\"1;1;5;5#0;2;0;0;0\x1b\\";
        let out = s.process(payload);
        assert_eq!(out, payload);
    }

    #[test]
    fn osc_9999_markdown_is_extracted_but_other_osc_skipped() {
        let mut s = PassthroughScanner::new();
        let mut data = Vec::new();
        // OSC 0 (title) — must NOT be extracted
        data.extend_from_slice(b"\x1b]0;hello\x07");
        // OSC 9999 — must be extracted
        data.extend_from_slice(b"\x1b]9999;emterm-md;begin\x1b\\");
        let out = s.process(&data);
        assert_eq!(out, b"\x1b]9999;emterm-md;begin\x1b\\");
    }

    #[test]
    fn osc_9999_terminated_by_bel() {
        let mut s = PassthroughScanner::new();
        let data = b"\x1b]9999;hello\x07";
        let out = s.process(data);
        assert_eq!(out, data);
    }

    #[test]
    fn kitty_apc_split_across_chunks_is_recovered() {
        let mut s = PassthroughScanner::new();
        let part1 = b"prefix\x1b_Gi=1,a=T;ABCD";
        let part2 = b"EFGH\x1b\\suffix";
        let out1 = s.process(part1);
        assert!(out1.is_empty(), "no completion in first chunk");
        let out2 = s.process(part2);
        assert_eq!(out2, b"\x1b_Gi=1,a=T;ABCDEFGH\x1b\\");
    }

    #[test]
    fn kitty_apc_split_at_terminator_is_recovered() {
        let mut s = PassthroughScanner::new();
        // Split right between the terminator bytes
        let part1 = b"\x1b_Gi=1;ABC\x1b";
        let part2 = b"\\trailing";
        let out = s.process(part1);
        assert!(out.is_empty());
        let out = s.process(part2);
        assert_eq!(out, b"\x1b_Gi=1;ABC\x1b\\");
    }

    #[test]
    fn multiple_sequences_in_one_chunk() {
        let mut s = PassthroughScanner::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"\x1b_Gi=1;A\x1b\\");
        data.extend_from_slice(b"text");
        data.extend_from_slice(b"\x1b]9999;md\x07");
        let out = s.process(&data);
        let mut expected = Vec::new();
        expected.extend_from_slice(b"\x1b_Gi=1;A\x1b\\");
        expected.extend_from_slice(b"\x1b]9999;md\x07");
        assert_eq!(out, expected);
    }

    #[test]
    fn non_kitty_apc_is_not_extracted() {
        let mut s = PassthroughScanner::new();
        // ESC _ X ... ESC \  (X is not 'G')
        let out = s.process(b"\x1b_Xfoo\x1b\\bar");
        assert!(out.is_empty());
    }

    #[test]
    fn dcs_without_q_is_not_extracted() {
        // DCS that's not SIXEL (no 'q' anywhere) should be skipped.
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1bP$tnotsixel\x1b\\");
        assert!(out.is_empty());
    }

    // ---- OSC 9 notification recognition (Phase 1) ----

    #[test]
    fn osc9_notification_bel_terminated_is_recognized() {
        // TS-1: OSC 9 ; msg terminated by BEL.
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1b]9;build done\x07");
        // Notifications are NOT replayable passthrough output.
        assert!(out.is_empty(), "OSC 9 must not appear in replay output");
        assert_eq!(s.take_notifications(), vec!["build done".to_string()]);
        // Draining empties the queue.
        assert!(s.take_notifications().is_empty());
    }

    #[test]
    fn osc9_notification_st_terminated_is_recognized() {
        // TS-2: OSC 9 ; msg terminated by ST (ESC \).
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1b]9;all tests passed\x1b\\");
        assert!(out.is_empty());
        assert_eq!(s.take_notifications(), vec!["all tests passed".to_string()]);
    }

    #[test]
    fn osc9_progress_is_not_a_notification() {
        // TS-3: OSC 9 ; 4 ; 1 ; 50 (progress) must NOT fire a notification.
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1b]9;4;1;50\x07");
        assert!(out.is_empty());
        assert!(
            s.take_notifications().is_empty(),
            "progress sequence must not be a notification"
        );
    }

    #[test]
    fn osc9_split_across_chunks_is_recovered() {
        // TS-4: OSC 9 split across chunk boundaries.
        let mut s = PassthroughScanner::new();
        let out1 = s.process(b"prefix\x1b]9;deploy ");
        assert!(out1.is_empty());
        assert!(s.take_notifications().is_empty(), "no completion yet");
        let out2 = s.process(b"finished\x07suffix");
        assert!(out2.is_empty());
        assert_eq!(s.take_notifications(), vec!["deploy finished".to_string()]);
    }

    #[test]
    fn osc9_empty_message_is_recognized() {
        // Edge case: OSC 9 ; <empty> then terminator. No crash; empty body.
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1b]9;\x07");
        assert!(out.is_empty());
        assert_eq!(s.take_notifications(), vec![String::new()]);
    }

    #[test]
    fn osc0_title_is_not_a_notification() {
        // TS-6: OSC 0 ; title and other OSC are not notifications.
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1b]0;my title\x07");
        assert!(out.is_empty());
        assert!(s.take_notifications().is_empty());
    }

    #[test]
    fn osc9999_markdown_is_replay_not_notification() {
        // TS-7: OSC 9999 stays in replay output; never a notification.
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1b]9999;emterm-md;begin\x1b\\");
        assert_eq!(out, b"\x1b]9999;emterm-md;begin\x1b\\");
        assert!(s.take_notifications().is_empty());
    }

    #[test]
    fn osc9_notification_and_replay_passthrough_are_separated() {
        // TS-7: a chunk mixing image passthrough and an OSC 9 notification
        // keeps the two on separate channels.
        let mut s = PassthroughScanner::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"\x1b_Gi=1;A\x1b\\"); // Kitty (replay)
        data.extend_from_slice(b"text");
        data.extend_from_slice(b"\x1b]9;ping\x07"); // notification
        data.extend_from_slice(b"\x1b]9999;md\x07"); // Markdown (replay)
        let out = s.process(&data);
        let mut expected_replay = Vec::new();
        expected_replay.extend_from_slice(b"\x1b_Gi=1;A\x1b\\");
        expected_replay.extend_from_slice(b"\x1b]9999;md\x07");
        assert_eq!(out, expected_replay, "replay must exclude OSC 9");
        assert_eq!(s.take_notifications(), vec!["ping".to_string()]);
    }

    #[test]
    fn multiple_osc9_notifications_in_one_chunk() {
        let mut s = PassthroughScanner::new();
        let out = s.process(b"\x1b]9;first\x07\x1b]9;second\x1b\\");
        assert!(out.is_empty());
        assert_eq!(
            s.take_notifications(),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn osc9_notification_with_unicode_body() {
        let mut s = PassthroughScanner::new();
        let out = s.process("\x1b]9;ビルド完了 🎉\x07".as_bytes());
        assert!(out.is_empty());
        assert_eq!(s.take_notifications(), vec!["ビルド完了 🎉".to_string()]);
    }

    #[test]
    fn partial_buffer_overflow_drops_sequence_and_warns_once() {
        let mut s = PassthroughScanner::new();
        // Start an APC that never terminates and pump > PARTIAL_SEQUENCE_MAX
        // bytes. After overflow the partial buffer must reset to 0.
        s.process(b"\x1b_G");
        let big = vec![b'x'; PARTIAL_SEQUENCE_MAX + 1];
        let out = s.process(&big);
        assert!(out.is_empty());
        assert_eq!(
            s.partial_buffer_len(),
            0,
            "partial must reset after overflow"
        );
    }
}
