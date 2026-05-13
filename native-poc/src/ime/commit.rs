//! IME commit path.
//!
//! When the platform IME delivers a committed string (the user pressed
//! Enter or selected a candidate), we sanitize it with the same helper
//! the preedit overlay uses and write the bytes to the active PTY
//! exactly once. Bracketed-paste wrapping is **not** applied — a commit
//! is user typing, not a paste.
//!
//! The writer is abstracted behind the [`PtyWriter`] trait so tests can
//! substitute a channel-backed recorder without spinning up a real PTY.

use std::io;

use crate::ime::preedit::sanitize;

/// Minimal trait the commit path uses to dispatch bytes. Implemented by
/// [`crate::pty::PtySession`] (production) and by a channel-backed
/// recorder under `cfg(test)`.
pub trait PtyWriter {
    /// Send `bytes` to the underlying PTY. Returning `Err` indicates the
    /// PTY is no longer accepting writes (closed / disconnected).
    fn write_bytes(&self, bytes: Vec<u8>) -> io::Result<()>;
}

/// Production impl: forward straight to `PtySession::write`. The PtySession
/// path is best-effort (drops on a full queue with a warn log), so we
/// always return `Ok(())` — the caller has no signal to retry against
/// the bounded queue.
impl PtyWriter for crate::pty::PtySession {
    fn write_bytes(&self, bytes: Vec<u8>) -> io::Result<()> {
        crate::pty::PtySession::write(self, bytes);
        Ok(())
    }
}

/// Sanitize `text` with the same helper the preedit overlay uses, then
/// dispatch the resulting bytes to `pty_writer` exactly once.
///
/// Returns `Ok(())` on success. Empty / fully-sanitized-to-empty input
/// is a no-op (no zero-byte write).
pub fn write_commit<W: PtyWriter + ?Sized>(pty_writer: &W, text: &str) -> io::Result<()> {
    let cleaned = sanitize(text);
    if cleaned.is_empty() {
        return Ok(());
    }
    pty_writer.write_bytes(cleaned.into_bytes())
}

#[cfg(test)]
/// Test-only re-export of the sanitize helper so `preedit::tests` can
/// pin the contract that both directions share the same sanitizer
/// without making `sanitize` `pub`.
pub(crate) fn sanitize_for_test(input: &str) -> String {
    sanitize(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Channel-backed recorder used in lieu of a real PtySession. Each
    /// call to `write_bytes` appends to `records`. Failure injection
    /// flips `should_fail` to simulate a closed PTY.
    #[derive(Default)]
    struct MockWriter {
        records: Mutex<Vec<Vec<u8>>>,
        should_fail: Mutex<bool>,
    }

    impl PtyWriter for MockWriter {
        fn write_bytes(&self, bytes: Vec<u8>) -> io::Result<()> {
            if *self.should_fail.lock().unwrap() {
                return Err(io::Error::other("simulated disconnect"));
            }
            self.records.lock().unwrap().push(bytes);
            Ok(())
        }
    }

    impl MockWriter {
        fn records(&self) -> Vec<Vec<u8>> {
            self.records.lock().unwrap().clone()
        }
        fn call_count(&self) -> usize {
            self.records.lock().unwrap().len()
        }
    }

    // ── TS-ime-2: commit writes sanitized bytes exactly once ────────

    #[test]
    fn commit_writes_plain_ascii_once() {
        let w = MockWriter::default();
        write_commit(&w, "hi").unwrap();
        assert_eq!(w.call_count(), 1);
        assert_eq!(w.records()[0], b"hi".to_vec());
    }

    #[test]
    fn commit_writes_utf8_bytes() {
        let w = MockWriter::default();
        // "あ" → UTF-8 E3 81 82
        write_commit(&w, "あ").unwrap();
        assert_eq!(w.records()[0], vec![0xE3, 0x81, 0x82]);
    }

    #[test]
    fn commit_strips_c0_before_write() {
        let w = MockWriter::default();
        // ESC inside a commit must NOT reach the PTY — otherwise a
        // malicious / malformed IME could inject control sequences.
        write_commit(&w, "ab\x1bcd").unwrap();
        assert_eq!(w.records()[0], b"abcd".to_vec());
    }

    #[test]
    fn commit_strips_c1_before_write() {
        let w = MockWriter::default();
        write_commit(&w, "x\u{009B}y").unwrap();
        assert_eq!(w.records()[0], b"xy".to_vec());
    }

    #[test]
    fn commit_empty_input_writes_nothing() {
        let w = MockWriter::default();
        write_commit(&w, "").unwrap();
        assert_eq!(w.call_count(), 0);
    }

    #[test]
    fn commit_sanitizes_to_empty_writes_nothing() {
        // Pathological case: payload is ONLY C0/C1. After sanitize the
        // string is empty; we must not emit a zero-byte write.
        let w = MockWriter::default();
        write_commit(&w, "\x07\x1b\u{009B}").unwrap();
        assert_eq!(w.call_count(), 0);
    }

    #[test]
    fn commit_does_not_wrap_in_bracketed_paste() {
        // A commit is user typing, not a paste. The bytes written must
        // be exactly the sanitized input — no ESC[200~ / ESC[201~
        // sentinels around them.
        let w = MockWriter::default();
        write_commit(&w, "hello").unwrap();
        let bytes = &w.records()[0];
        assert!(
            !bytes.windows(6).any(|w| w == b"\x1b[200~"),
            "commit must not be wrapped as bracketed paste"
        );
        assert!(
            !bytes.windows(6).any(|w| w == b"\x1b[201~"),
            "commit must not be wrapped as bracketed paste"
        );
        assert_eq!(bytes, &b"hello".to_vec());
    }

    #[test]
    fn commit_writes_exactly_one_call_per_invocation() {
        // Regression guard: even with multi-character input, the writer
        // is invoked once per `write_commit` call.
        let w = MockWriter::default();
        write_commit(&w, "abcdefghijk").unwrap();
        assert_eq!(w.call_count(), 1);
    }

    #[test]
    fn commit_propagates_writer_error() {
        let w = MockWriter::default();
        *w.should_fail.lock().unwrap() = true;
        let result = write_commit(&w, "hello");
        assert!(result.is_err());
    }

    // ── cfg(windows) smoke: Event::Ime variants compile ─────────────

    #[cfg(windows)]
    #[test]
    fn windows_event_ime_variants_compile() {
        // Compile-only smoke: confirms that the egui IME event shape
        // (Preedit / Commit) is the same on Windows targets. Actual
        // MS-IME verification is the manual host gate
        // (TS-manual-ime-windows), which is out of scope here.
        let _preedit = egui::Event::Ime(egui::ImeEvent::Preedit(String::from("test")));
        let _commit = egui::Event::Ime(egui::ImeEvent::Commit(String::from("test")));
        let _enabled = egui::Event::Ime(egui::ImeEvent::Enabled);
        let _disabled = egui::Event::Ime(egui::ImeEvent::Disabled);
    }
}
