//! OSC 52 clipboard application (osc52-clipboard-write-drain).
//!
//! `NativeCallbacks::handle_clipboard` queues accepted OSC 52 writes and
//! clears on each tab. `App::pump_all` drains those queues into one per-pump
//! batch (FR5, FR6); this module maps each entry's target to system-clipboard
//! destinations (FR3) and applies the batch through a sink seam on the UI
//! thread (FR1, FR2, NFR2). The production sink is `WindowHost`.

use crate::app::App;

/// Whether the platform has a PRIMARY selection distinct from CLIPBOARD. The
/// same platform condition the PRIMARY code in `window_host/mod.rs` uses
/// (`set_primary` / `get_primary`): Linux yes, Windows no (NFR6).
pub(crate) const PRIMARY_AVAILABLE: bool = cfg!(all(unix, not(target_os = "macos")));

/// A system-clipboard destination an OSC 52 target can address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Osc52Destination {
    Clipboard,
    Primary,
}

impl Osc52Destination {
    /// Name used in log lines (NFR3: destination only, never text).
    pub(crate) fn label(self) -> &'static str {
        match self {
            Osc52Destination::Clipboard => "CLIPBOARD",
            Osc52Destination::Primary => "PRIMARY",
        }
    }
}

/// FR3: map an OSC 52 target to its destinations, CLIPBOARD before PRIMARY.
///
/// A target containing `c` selects CLIPBOARD, one containing `p` selects
/// PRIMARY, one containing both selects both. A target containing neither
/// (empty, `s`, `0`-`7`, `q`, or any other character) selects CLIPBOARD (A3).
/// When `primary_available` is false PRIMARY is replaced by CLIPBOARD and the
/// duplicate collapses, so a both-target yields CLIPBOARD once (NFR6). The
/// result is never empty.
pub(crate) fn osc52_destinations(target: &str, primary_available: bool) -> Vec<Osc52Destination> {
    let wants_clipboard = target.contains('c');
    let wants_primary = target.contains('p');
    let clipboard = wants_clipboard || !wants_primary || !primary_available;
    let primary = wants_primary && primary_available;
    let mut destinations = Vec::with_capacity(2);
    if clipboard {
        destinations.push(Osc52Destination::Clipboard);
    }
    if primary {
        destinations.push(Osc52Destination::Primary);
    }
    destinations
}

/// Seam over the system clipboard for OSC 52 (FR1, FR2, FR8). A failure
/// description never contains clipboard text (NFR3).
pub(crate) trait Osc52ClipboardSink {
    /// Set `destination`'s text to `text` (non-empty).
    fn write(&mut self, destination: Osc52Destination, text: &str) -> Result<(), String>;
    /// Leave `destination` without text. Never an empty-text write (FR2).
    fn clear(&mut self, destination: Osc52Destination) -> Result<(), String>;
}

/// NFR3, TM-3: the `warn` line for one failed operation, built from the
/// destination, the text length (0 for a clear) and the failure description
/// only. It never receives the text, so it cannot carry it.
pub(crate) fn format_osc52_failure(
    destination: Osc52Destination,
    text_len: usize,
    description: &str,
) -> String {
    format!(
        "OSC 52 clipboard apply failed: dest={} len={text_len}: {description}",
        destination.label()
    )
}

/// One destination's pending operation while a batch is coalesced.
#[derive(Clone, Copy)]
enum PendingOp<'a> {
    Write(&'a str),
    Clear,
}

/// FR1, FR2, FR8: apply a drained batch, logging failures with `log::warn!`.
///
/// Entries are walked in order; each destination keeps only the operation of
/// the last entry addressed to it (empty text = clear, otherwise write).
/// Each destination that has an operation then receives exactly that one
/// operation, CLIPBOARD before PRIMARY (TM-4). A failed operation is logged
/// (destination, length, failure description) and the apply goes on.
pub(crate) fn apply_osc52_batch(
    entries: &[(String, String)],
    sink: &mut dyn Osc52ClipboardSink,
    primary_available: bool,
) {
    apply_osc52_batch_logged(entries, sink, primary_available, &mut |line| {
        log::warn!("{line}")
    });
}

/// [`apply_osc52_batch`] with the failure-log lines handed to `log` (tests
/// capture them; the project has no log-capture facility).
pub(crate) fn apply_osc52_batch_logged(
    entries: &[(String, String)],
    sink: &mut dyn Osc52ClipboardSink,
    primary_available: bool,
    log: &mut dyn FnMut(&str),
) {
    // Indexed by `Osc52Destination as usize`: CLIPBOARD = 0, PRIMARY = 1.
    let mut pending: [Option<PendingOp<'_>>; 2] = [None, None];
    for (target, text) in entries {
        let op = if text.is_empty() {
            PendingOp::Clear
        } else {
            PendingOp::Write(text.as_str())
        };
        for destination in osc52_destinations(target, primary_available) {
            pending[destination as usize] = Some(op);
        }
    }
    for destination in [Osc52Destination::Clipboard, Osc52Destination::Primary] {
        let Some(op) = pending[destination as usize] else {
            continue;
        };
        let (text_len, result) = match op {
            PendingOp::Write(text) => (text.len(), sink.write(destination, text)),
            PendingOp::Clear => (0, sink.clear(destination)),
        };
        if let Err(description) = result {
            log(&format_osc52_failure(destination, text_len, &description));
        }
    }
}

/// Pump-to-sink step: take the App's per-pump OSC 52 batch and apply it with
/// the platform's PRIMARY availability. An empty batch makes no sink call.
pub(crate) fn apply_pending_osc52(app: &mut App, sink: &mut dyn Osc52ClipboardSink) {
    let batch = app.take_osc52_clipboard_batch();
    if batch.is_empty() {
        return;
    }
    apply_osc52_batch(&batch, sink, PRIMARY_AVAILABLE);
}

#[cfg(test)]
pub(crate) mod tests;
