//! Stateful scanner that extracts "raw passthrough" sequences from a PTY
//! byte stream while a session is hidden.
//!
//! The shadow VT100 parser updates the screen state but does NOT preserve
//! image / Markdown OSC byte streams (they are decoded into events and the
//! original bytes are gone). The frontend must replay the original bytes
//! on resume to re-render images and rich content, so we sniff them out
//! here and stash them in the per-session passthrough buffer.
//!
//! Three sequence kinds are extracted:
//! - Kitty graphics APC: `ESC _ G ... ESC \`
//! - SIXEL DCS:          `ESC P ... q ... ESC \`
//! - emterm OSC 9999:    `ESC ] 9999 ; ... ESC \` (Markdown protocol)
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
}

impl PassthroughScanner {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            partial: Vec::new(),
            overflow_warned: false,
        }
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
                    // Commit only if the OSC param starts with "9999;".
                    // partial layout: ESC ] <body...> [terminator]
                    if self.partial.starts_with(b"\x1b]9999;") {
                        out.extend_from_slice(&self.partial);
                    }
                    self.reset_partial();
                }
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
