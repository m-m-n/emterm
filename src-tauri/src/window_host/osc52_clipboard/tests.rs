//! Unit tests for the OSC 52 clipboard application module (TS-1, TS-2).
//! None of these needs a window: the sink seam is replaced by doubles.

use super::*;

use Osc52Destination::{Clipboard, Primary};

/// One operation a sink double received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SinkCall {
    Write(Osc52Destination, String),
    Clear(Osc52Destination),
}

/// Recording sink double: every call succeeds and is recorded in order.
/// Shared with `app/tests/osc52_clipboard.rs`.
#[derive(Debug, Default)]
pub(crate) struct RecordingSink {
    pub(crate) calls: Vec<SinkCall>,
}

impl Osc52ClipboardSink for RecordingSink {
    fn write(&mut self, destination: Osc52Destination, text: &str) -> Result<(), String> {
        self.calls
            .push(SinkCall::Write(destination, text.to_string()));
        Ok(())
    }

    fn clear(&mut self, destination: Osc52Destination) -> Result<(), String> {
        self.calls.push(SinkCall::Clear(destination));
        Ok(())
    }
}

/// Failing sink double: every call is recorded and reports `description`.
struct FailingSink {
    description: &'static str,
    attempts: Vec<SinkCall>,
}

impl FailingSink {
    fn new(description: &'static str) -> Self {
        Self {
            description,
            attempts: Vec::new(),
        }
    }
}

impl Osc52ClipboardSink for FailingSink {
    fn write(&mut self, destination: Osc52Destination, text: &str) -> Result<(), String> {
        self.attempts
            .push(SinkCall::Write(destination, text.to_string()));
        Err(self.description.to_string())
    }

    fn clear(&mut self, destination: Osc52Destination) -> Result<(), String> {
        self.attempts.push(SinkCall::Clear(destination));
        Err(self.description.to_string())
    }
}

fn entry(target: &str, text: &str) -> (String, String) {
    (target.to_string(), text.to_string())
}

fn apply(entries: &[(String, String)], primary_available: bool) -> Vec<SinkCall> {
    let mut sink = RecordingSink::default();
    apply_osc52_batch(entries, &mut sink, primary_available);
    sink.calls
}

// ── AC-1 / TS-1: destination mapping ────────────────────────────────

/// AC-1 (FR3): the target table with PRIMARY available.
#[test]
fn ac1_destinations_map_each_target_with_primary_available() {
    let table: &[(&str, &[Osc52Destination])] = &[
        ("c", &[Clipboard]),
        ("p", &[Primary]),
        ("cp", &[Clipboard, Primary]),
        ("pc", &[Clipboard, Primary]),
        ("", &[Clipboard]),
        ("s", &[Clipboard]),
        ("0", &[Clipboard]),
        ("7", &[Clipboard]),
        ("q", &[Clipboard]),
        ("s0", &[Clipboard]),
        ("x", &[Clipboard]),
    ];
    for (target, expected) in table {
        assert_eq!(
            osc52_destinations(target, true),
            expected.to_vec(),
            "target {target:?} with PRIMARY available"
        );
    }
}

/// AC-1 (NFR6): with PRIMARY unavailable every target maps to CLIPBOARD
/// once.
#[test]
fn ac1_destinations_map_every_target_to_clipboard_without_primary() {
    for target in ["c", "p", "cp", "pc", "", "s", "0", "7", "q", "s0", "x"] {
        assert_eq!(
            osc52_destinations(target, false),
            vec![Clipboard],
            "target {target:?} with PRIMARY unavailable"
        );
    }
}

/// The platform constant follows the condition the PRIMARY code in
/// `window_host/mod.rs` uses.
#[test]
fn primary_available_matches_the_existing_primary_code_condition() {
    assert_eq!(PRIMARY_AVAILABLE, cfg!(all(unix, not(target_os = "macos"))));
}

// ── AC-2 / TS-2: batch apply (coalescing, clears) ───────────────────

/// AC-2: two writes with target `c` give one CLIPBOARD write of the last
/// text.
#[test]
fn ac2_two_writes_to_the_same_destination_apply_only_the_last() {
    let calls = apply(&[entry("c", "a"), entry("c", "b")], true);
    assert_eq!(calls, vec![SinkCall::Write(Clipboard, "b".to_string())]);
}

/// AC-2: a `cp` write followed by a `p` clear gives a CLIPBOARD write and a
/// PRIMARY clear (CLIPBOARD first).
#[test]
fn ac2_both_target_write_then_primary_clear_splits_per_destination() {
    let calls = apply(&[entry("cp", "a"), entry("p", "")], true);
    assert_eq!(
        calls,
        vec![
            SinkCall::Write(Clipboard, "a".to_string()),
            SinkCall::Clear(Primary),
        ]
    );
}

/// AC-2 (FR2): clears with `c`, `p`, `cp` and empty targets are clear
/// operations on their mapped destinations, never empty-text writes.
#[test]
fn ac2_clears_are_recorded_as_clear_operations_not_empty_writes() {
    let table: &[(&str, &[Osc52Destination])] = &[
        ("c", &[Clipboard]),
        ("p", &[Primary]),
        ("cp", &[Clipboard, Primary]),
        ("", &[Clipboard]),
    ];
    for (target, destinations) in table {
        let calls = apply(&[entry(target, "")], true);
        let expected: Vec<SinkCall> = destinations.iter().map(|d| SinkCall::Clear(*d)).collect();
        assert_eq!(calls, expected, "clear with target {target:?}");
    }
}

/// AC-2: a later write replaces an earlier clear, and a later clear
/// replaces an earlier write, per destination.
#[test]
fn ac2_last_entry_wins_across_write_and_clear() {
    assert_eq!(
        apply(&[entry("c", ""), entry("c", "x")], true),
        vec![SinkCall::Write(Clipboard, "x".to_string())]
    );
    assert_eq!(
        apply(&[entry("c", "x"), entry("c", "")], true),
        vec![SinkCall::Clear(Clipboard)]
    );
}

/// AC-2 (TM-4): at most one operation per destination however long the
/// batch is.
#[test]
fn ac2_at_most_one_operation_per_destination() {
    let mut entries = Vec::new();
    for i in 0..50 {
        entries.push(entry(if i % 2 == 0 { "c" } else { "cp" }, &format!("t{i}")));
    }
    let calls = apply(&entries, true);
    assert_eq!(calls.len(), 2, "one op for CLIPBOARD, one for PRIMARY");
    assert_eq!(
        calls,
        vec![
            SinkCall::Write(Clipboard, "t49".to_string()),
            SinkCall::Write(Primary, "t49".to_string()),
        ]
    );
}

/// AC-2 (NFR6): without PRIMARY the `p` half of a `cp` entry lands on
/// CLIPBOARD, and the last entry still wins.
#[test]
fn ac2_without_primary_everything_lands_on_clipboard() {
    let calls = apply(&[entry("cp", "a"), entry("p", "")], false);
    assert_eq!(calls, vec![SinkCall::Clear(Clipboard)]);
    let calls = apply(&[entry("p", "only")], false);
    assert_eq!(calls, vec![SinkCall::Write(Clipboard, "only".to_string())]);
}

/// An empty batch makes no sink call.
#[test]
fn ac2_empty_batch_makes_no_sink_call() {
    assert!(apply(&[], true).is_empty());
}

// ── AC-3 / TS-2: failures (FR8, NFR3, TM-3) ─────────────────────────

/// AC-3: a sink that fails every operation (here the "clipboard
/// unavailable" case) is attempted for every recorded destination, the
/// call returns without panic, and each failure yields one log line that
/// carries neither the text nor its base64 encoding.
#[test]
fn ac3_failing_sink_is_attempted_for_every_destination_and_logs_without_text() {
    let mut sink = FailingSink::new("clipboard unavailable");
    let mut lines: Vec<String> = Vec::new();
    // CLIPBOARD gets a write of "secret", PRIMARY gets a clear.
    apply_osc52_batch_logged(
        &[entry("c", "secret"), entry("p", "")],
        &mut sink,
        true,
        &mut |line| lines.push(line.to_string()),
    );
    assert_eq!(
        sink.attempts,
        vec![
            SinkCall::Write(Clipboard, "secret".to_string()),
            SinkCall::Clear(Primary),
        ],
        "every recorded destination is attempted even though the first failed"
    );
    assert_eq!(lines.len(), 2, "one log line per failed operation");
    for line in &lines {
        assert!(!line.contains("secret"), "no clipboard text: {line}");
        assert!(!line.contains("c2VjcmV0"), "no base64 of the text: {line}");
        assert!(line.contains("clipboard unavailable"), "{line}");
    }
    assert!(lines[0].contains("CLIPBOARD") && lines[0].contains("len=6"));
    assert!(lines[1].contains("PRIMARY") && lines[1].contains("len=0"));
}

/// AC-3: the failure line for a write of a known text carries the
/// destination and the length but not the text or its base64.
#[test]
fn ac3_failure_line_has_destination_and_length_but_no_text() {
    let line = format_osc52_failure(Clipboard, 6, "clipboard unavailable");
    assert!(line.contains("CLIPBOARD"), "{line}");
    assert!(line.contains("len=6"), "{line}");
    assert!(line.contains("clipboard unavailable"), "{line}");
    assert!(!line.contains("secret"), "{line}");
    assert!(!line.contains("c2VjcmV0"), "{line}");
    let clear_line = format_osc52_failure(Primary, 0, "boom");
    assert!(clear_line.contains("PRIMARY"), "{clear_line}");
    assert!(clear_line.contains("len=0"), "{clear_line}");
}

/// AC-3: a failing sink given a long batch still returns without panic and
/// is attempted once per destination, not once per entry.
#[test]
fn ac3_failing_sink_with_a_long_batch_attempts_each_destination_once() {
    let mut sink = FailingSink::new("fail");
    let entries: Vec<_> = (0..20).map(|i| entry("cp", &format!("t{i}"))).collect();
    apply_osc52_batch(&entries, &mut sink, true);
    assert_eq!(sink.attempts.len(), 2);
}
