//! Build-agnostic Program Status core (OSC 7501, Program Status Protocol
//! draft 0.3): body parser, per-terminal record table, lifecycle
//! operations and the table's summary (IMPLEMENTATION.md SC-1).
//!
//! Compiled WITHOUT the `gui` feature (CLI-shared), like
//! [`crate::agent_status`]; it depends only on always-built crates.
//!
//! This worktree carries the MINIMUM of SC-1 that plain-tab ingestion
//! (task0003) needs (IMPLEMENTATION.md D10): the parser, the table's
//! replacement / clear / cap rules, the two lifecycle operations and the
//! summary. The module's owner (task0001) supersedes this file; export /
//! import and any behavior beyond what plain-tab ingestion reads are
//! deliberately absent here. `kind`, `progress` and `msg` are validated
//! but not stored: no consumer displays them (SPEC FR5).
//!
//! Rejected or ignored input is never logged: it is untrusted and could
//! flood the log.

use std::collections::HashMap;

use base64::Engine as _;
use base64::alphabet;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};

/// Introducer bytes counted toward the whole-sequence limit: `ESC`, `]`,
/// `7501` and `;`.
const INTRODUCER_BYTES: usize = 7;
/// Whole-sequence limit in bytes (introducer + body + terminator).
const MAX_SEQUENCE_BYTES: usize = 4096;
/// Record cap per terminal (SPEC FR6).
pub const MAX_RECORDS: usize = 256;
const MAX_TITLE_ENCODED_BYTES: usize = 256;
const MAX_TITLE_DECODED_BYTES: usize = 192;
const MAX_MSG_ENCODED_BYTES: usize = 2732;
const MAX_MSG_DECODED_BYTES: usize = 2048;
const MAX_ID_SEGMENTS: usize = 8;
const MAX_ID_BYTES: usize = 128;
const MAX_SEGMENT_BYTES: usize = 32;
const MAX_APP_BYTES: usize = 32;
/// Title length cap for names, in Unicode scalar values (IMPLEMENTATION.md
/// D9).
const MAX_NAME_TITLE_CHARS: usize = 80;

/// Padded and unpadded base64 are both accepted.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Which string terminator ended the OSC sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// BEL (0x07).
    Bel,
    /// ST (`ESC \`).
    St,
}

impl Terminator {
    fn byte_len(self) -> usize {
        match self {
            Terminator::Bel => 1,
            Terminator::St => 2,
        }
    }

    /// The terminator's bytes as written on the wire.
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Terminator::Bel => b"\x07",
            Terminator::St => b"\x1b\\",
        }
    }
}

/// A stored record state. `clear` is a report action, never a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

impl RecordState {
    /// The protocol's lowercase state word.
    pub fn word(self) -> &'static str {
        match self {
            RecordState::Idle => "idle",
            RecordState::Working => "working",
            RecordState::Done => "done",
            RecordState::Blocked => "blocked",
            RecordState::Error => "error",
        }
    }

    /// Composition rank (IMPLEMENTATION.md D2): blocked > working > error >
    /// done > idle.
    fn rank(self) -> u8 {
        match self {
            RecordState::Blocked => 4,
            RecordState::Working => 3,
            RecordState::Error => 2,
            RecordState::Done => 1,
            RecordState::Idle => 0,
        }
    }
}

/// One validated, ready-to-apply report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// `state=clear`: remove the id and its descendants, or every record
    /// when no id is given.
    Clear { id: Option<String> },
    /// Any other state: fully replace the id's record.
    Set {
        id: Option<String>,
        state: RecordState,
        title: Option<String>,
        app: Option<String>,
    },
}

/// Result of [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// The body is exactly `?`.
    Query,
    /// A fully validated report.
    Report(Report),
    /// Discarded or ignored: nothing changes.
    Ignored,
}

/// Parse the text after `7501;` of a terminated OSC sequence. Pure: no side
/// effects and no logging.
pub fn parse(body: &str, terminator: Terminator) -> Parsed {
    if body == "?" {
        return Parsed::Query;
    }
    if INTRODUCER_BYTES + body.len() + terminator.byte_len() > MAX_SEQUENCE_BYTES {
        return Parsed::Ignored;
    }

    let mut state = None;
    let mut id = None;
    let mut title = None;
    let mut msg = None;
    let mut app = None;
    for pair in body.split(':') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase()) {
            continue;
        }
        if !value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b',' | b'+' | b'/' | b'=' | b'-')
        }) {
            continue;
        }
        match key {
            "state" => state = Some(value),
            "id" => id = Some(value),
            "title" => title = Some(value),
            "msg" => msg = Some(value),
            "app" => app = Some(value),
            _ => {}
        }
    }

    // Whole-report discard conditions.
    let Some(title) = decode_text(title, MAX_TITLE_ENCODED_BYTES, MAX_TITLE_DECODED_BYTES) else {
        return Parsed::Ignored;
    };
    if decode_text(msg, MAX_MSG_ENCODED_BYTES, MAX_MSG_DECODED_BYTES).is_none() {
        return Parsed::Ignored;
    }

    // Whole-report ignore conditions.
    let Some(state) = state else {
        return Parsed::Ignored;
    };
    let id = match id {
        None => None,
        Some(value) if valid_id(value) => Some(value.to_string()),
        Some(_) => return Parsed::Ignored,
    };

    if state == "clear" {
        return Parsed::Report(Report::Clear { id });
    }
    let state = match state {
        "idle" => RecordState::Idle,
        "working" => RecordState::Working,
        "done" => RecordState::Done,
        "blocked" => RecordState::Blocked,
        "error" => RecordState::Error,
        _ => return Parsed::Ignored,
    };
    let app = app
        .filter(|a| valid_segment(a, MAX_APP_BYTES))
        .map(str::to_string);
    Parsed::Report(Report::Set {
        id,
        state,
        title,
        app,
    })
}

/// Decode an optional base64 `title` / `msg` value. `None` means the whole
/// report is discarded; `Some(None)` means the value is absent or empty.
fn decode_text(
    encoded: Option<&str>,
    max_encoded: usize,
    max_decoded: usize,
) -> Option<Option<String>> {
    let Some(encoded) = encoded else {
        return Some(None);
    };
    if encoded.len() > max_encoded {
        return None;
    }
    let bytes = BASE64.decode(encoded.as_bytes()).ok()?;
    if bytes.len() > max_decoded {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(if text.is_empty() { None } else { Some(text) })
}

/// `[A-Za-z0-9_.+-]{1,max}`.
fn valid_segment(segment: &str, max: usize) -> bool {
    !segment.is_empty()
        && segment.len() <= max
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
}

/// 1 to 8 `/`-separated segments, 128 bytes at most in total.
fn valid_id(id: &str) -> bool {
    id.len() <= MAX_ID_BYTES
        && id.split('/').count() <= MAX_ID_SEGMENTS
        && id.split('/').all(|s| valid_segment(s, MAX_SEGMENT_BYTES))
}

#[derive(Debug, Clone)]
struct StoredRecord {
    state: RecordState,
    title: Option<String>,
    app: Option<String>,
    /// Update order: larger is more recently updated.
    updated: u64,
}

/// The table's summary: the deciding record's state, sanitized title and
/// effective app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub state: RecordState,
    /// The deciding record's title after [`sanitize_title_for_name`];
    /// absent when missing or empty after sanitization.
    pub title: Option<String>,
    /// The deciding record's own app, else the nearest ancestor record's.
    pub app: Option<String>,
}

/// One terminal's record table. The root record (no id) is keyed by the
/// empty string and is an ancestor of every id.
#[derive(Debug, Clone, Default)]
pub struct ProgramStatusTable {
    records: HashMap<String, StoredRecord>,
    clock: u64,
}

impl ProgramStatusTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Apply one validated report.
    pub fn apply(&mut self, report: Report) {
        match report {
            Report::Clear { id: None } => self.records.clear(),
            Report::Clear { id: Some(id) } => {
                let prefix = format!("{id}/");
                self.records
                    .retain(|key, _| key != &id && !key.starts_with(&prefix));
            }
            Report::Set {
                id,
                state,
                title,
                app,
            } => {
                let key = id.unwrap_or_default();
                if !self.records.contains_key(&key) && self.records.len() >= MAX_RECORDS {
                    self.evict_least_recently_updated();
                }
                self.clock += 1;
                self.records.insert(
                    key,
                    StoredRecord {
                        state,
                        title,
                        app,
                        updated: self.clock,
                    },
                );
            }
        }
    }

    fn evict_least_recently_updated(&mut self) {
        if let Some(oldest) = self
            .records
            .iter()
            .min_by_key(|(_, record)| record.updated)
            .map(|(key, _)| key.clone())
        {
            self.records.remove(&oldest);
        }
    }

    /// Live main-screen OSC 133 A: remove `working`, `blocked` and `idle`
    /// records; keep `done` and `error`. Returns whether anything was
    /// removed.
    pub fn prompt_start(&mut self) -> bool {
        let before = self.records.len();
        self.records
            .retain(|_, record| matches!(record.state, RecordState::Done | RecordState::Error));
        self.records.len() != before
    }

    /// RIS: remove every record. Returns whether any existed.
    pub fn reset(&mut self) -> bool {
        let existed = !self.records.is_empty();
        self.records.clear();
        existed
    }

    /// Absent for an empty table; otherwise the deciding record (highest
    /// rank, ties to the most recently updated) with its sanitized title and
    /// effective app.
    pub fn summary(&self) -> Option<Summary> {
        let (key, record) = self
            .records
            .iter()
            .max_by_key(|(_, record)| (record.state.rank(), record.updated))?;
        let title = record
            .title
            .as_deref()
            .map(sanitize_title_for_name)
            .filter(|t| !t.is_empty());
        let app = record.app.clone().or_else(|| self.inherited_app(key));
        Some(Summary {
            state: record.state,
            title,
            app,
        })
    }

    /// The app of the nearest ancestor record that has one. `a/b/c` has the
    /// ancestors `a/b`, `a` and the root.
    fn inherited_app(&self, key: &str) -> Option<String> {
        let mut current = key;
        while !current.is_empty() {
            current = current.rsplit_once('/').map_or("", |(parent, _)| parent);
            if let Some(app) = self.records.get(current).and_then(|r| r.app.clone()) {
                return Some(app);
            }
        }
        None
    }
}

/// IMPLEMENTATION.md D9: remove control characters and invisible
/// formatting characters, then truncate to 80 Unicode scalar values.
pub fn sanitize_title_for_name(title: &str) -> String {
    title
        .chars()
        .filter(|c| !c.is_control() && !is_invisible_formatting(*c))
        .take(MAX_NAME_TITLE_CHARS)
        .collect()
}

fn is_invisible_formatting(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}
