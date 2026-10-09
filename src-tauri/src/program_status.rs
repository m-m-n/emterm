//! Build-agnostic Program Status core (OSC 7501): body parser, per-terminal
//! record table, lifecycle operations and the table's aggregate summary
//! (IMPLEMENTATION.md SC-1).
//!
//! Compiled WITHOUT the `gui` feature (CLI-shared, like
//! [`crate::agent_status`]): the mux daemon depends on it without pulling in
//! GUI-only crates.
//!
//! This is the minimum of SC-1 that the mux daemon ingestion needs
//! (IMPLEMENTATION.md D10): parse, apply, prompt start, reset, summary and
//! the title sanitization for names. The owning task supersedes it.
//!
//! Rejected or ignored input is never logged at warn level or above: it is
//! untrusted and could flood the log.

use base64::Engine as _;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

/// Bytes of the `ESC ] 7501 ;` introducer.
const INTRODUCER_LEN: usize = 7;

/// A whole sequence above this many bytes (introducer, body, terminator)
/// discards the report.
const MAX_SEQUENCE_LEN: usize = 4096;

const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE_DECODED: usize = 192;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG_DECODED: usize = 2048;

const MAX_ID_BYTES: usize = 128;
const MAX_ID_SEGMENTS: usize = 8;
const MAX_SEGMENT_CHARS: usize = 32;
const MAX_APP_CHARS: usize = 32;

/// Records one table holds; inserting a new id beyond this evicts the least
/// recently updated record.
pub const MAX_RECORDS: usize = 256;

/// Characters of a title that are kept when it becomes a name.
const MAX_NAME_CHARS: usize = 80;

/// Standard base64 alphabet; padded and unpadded input are both accepted.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// How the OSC sequence was terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// `BEL` (0x07), one byte.
    Bel,
    /// `ST` (`ESC \`), two bytes.
    St,
}

impl Terminator {
    fn len(self) -> usize {
        match self {
            Terminator::Bel => 1,
            Terminator::St => 2,
        }
    }
}

/// The stored record states, ranked for composition: blocked > working >
/// error > done > idle. `clear` is a report action, never a stored state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

impl RecordState {
    /// The protocol word of the state.
    pub fn word(self) -> &'static str {
        match self {
            RecordState::Idle => "idle",
            RecordState::Working => "working",
            RecordState::Done => "done",
            RecordState::Blocked => "blocked",
            RecordState::Error => "error",
        }
    }

    /// Order used to pick a table's deciding record.
    pub fn rank(self) -> u8 {
        match self {
            RecordState::Idle => 0,
            RecordState::Done => 1,
            RecordState::Error => 2,
            RecordState::Working => 3,
            RecordState::Blocked => 4,
        }
    }
}

/// A stored record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub state: RecordState,
    pub app: Option<String>,
    /// Decoded title (plain text, never interpreted).
    pub title: Option<String>,
}

/// What a validated report does to its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// `state=clear`: remove the id and its descendants; with the root id,
    /// remove every record.
    Clear,
    /// Replace the id's record.
    Set(Record),
}

/// A fully validated report, ready to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Hierarchical id; the empty text is the root.
    pub id: String,
    pub action: Action,
}

/// Result of [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// The body is exactly `?`.
    Query,
    /// A validated report.
    Report(Report),
    /// Discarded or ignored: nothing changes.
    Ignored,
}

/// Parse the text that follows `7501;` of a terminated OSC sequence.
pub fn parse(body: &str, terminator: Terminator) -> Parsed {
    if body == "?" {
        return Parsed::Query;
    }
    if INTRODUCER_LEN + body.len() + terminator.len() > MAX_SEQUENCE_LEN {
        return Parsed::Ignored;
    }

    let mut state = None;
    let mut id = None;
    let mut app = None;
    let mut title = None;
    let mut msg = None;
    for pair in body.split(':') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase()) {
            continue;
        }
        if !value.bytes().all(is_value_byte) {
            continue;
        }
        match key {
            "state" => state = Some(value),
            "id" => id = Some(value),
            "app" => app = Some(value),
            "title" => title = Some(value),
            "msg" => msg = Some(value),
            _ => {}
        }
    }

    let Ok(title) = decode_text(title, MAX_TITLE_ENCODED, MAX_TITLE_DECODED) else {
        return Parsed::Ignored;
    };
    if decode_text(msg, MAX_MSG_ENCODED, MAX_MSG_DECODED).is_err() {
        return Parsed::Ignored;
    }
    let Some(state) = state else {
        return Parsed::Ignored;
    };
    let id = match id {
        None => "",
        Some(value) if valid_id(value) => value,
        Some(_) => return Parsed::Ignored,
    };
    let action = match state {
        "clear" => Action::Clear,
        word => {
            let state = match word {
                "idle" => RecordState::Idle,
                "working" => RecordState::Working,
                "done" => RecordState::Done,
                "blocked" => RecordState::Blocked,
                "error" => RecordState::Error,
                _ => return Parsed::Ignored,
            };
            Action::Set(Record {
                state,
                app: app.filter(|a| valid_app(a)).map(str::to_string),
                title,
            })
        }
    };
    Parsed::Report(Report {
        id: id.to_string(),
        action,
    })
}

/// Bytes a pair value may hold: `A-Z a-z 0-9 _ . , + / = -`.
fn is_value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b',' | b'+' | b'/' | b'=' | b'-')
}

/// `[A-Za-z0-9_.+-]{1,32}`.
fn valid_segment(segment: &str) -> bool {
    (1..=MAX_SEGMENT_CHARS).contains(&segment.len())
        && segment
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
}

fn valid_app(app: &str) -> bool {
    valid_segment(app) && app.len() <= MAX_APP_CHARS
}

/// 1 to 8 `/`-separated segments, 128 bytes at most.
fn valid_id(id: &str) -> bool {
    id.len() <= MAX_ID_BYTES
        && id.split('/').count() <= MAX_ID_SEGMENTS
        && id.split('/').all(valid_segment)
}

/// Decode a `title` / `msg` value. `Err` discards the whole report; an
/// empty result counts as absent.
fn decode_text(
    value: Option<&str>,
    max_encoded: usize,
    max_decoded: usize,
) -> Result<Option<String>, ()> {
    let Some(encoded) = value else {
        return Ok(None);
    };
    if encoded.len() > max_encoded {
        return Err(());
    }
    let bytes = BASE64.decode(encoded).map_err(|_| ())?;
    if bytes.len() > max_decoded {
        return Err(());
    }
    let text = String::from_utf8(bytes).map_err(|_| ())?;
    if text.chars().any(char::is_control) {
        return Err(());
    }
    Ok(if text.is_empty() { None } else { Some(text) })
}

/// Control characters and invisible formatting characters are removed from a
/// title that becomes a name, and the result is cut to 80 characters.
pub fn sanitize_title_for_name(title: &str) -> String {
    title
        .chars()
        .filter(|&c| !c.is_control() && !is_invisible_formatting(c))
        .take(MAX_NAME_CHARS)
        .collect()
}

fn is_invisible_formatting(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD
            | 0x061C
            | 0x200B..=0x200F
            | 0x2028..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x2069
            | 0xFEFF
    )
}

/// The aggregate of a non-empty table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// State of the deciding record.
    pub state: RecordState,
    /// Deciding record's title after [`sanitize_title_for_name`]; absent when
    /// missing or empty after sanitization.
    pub title: Option<String>,
    /// Deciding record's own app, else the nearest ancestor record's app.
    pub app: Option<String>,
}

/// One terminal's records, least recently updated first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    records: Vec<(String, Record)>,
}

impl Table {
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Apply a validated report. Every applied report counts as accepted,
    /// whether or not stored data changed.
    pub fn apply(&mut self, report: Report) {
        match report.action {
            Action::Clear => {
                if report.id.is_empty() {
                    self.records.clear();
                } else {
                    let prefix = format!("{}/", report.id);
                    self.records
                        .retain(|(id, _)| *id != report.id && !id.starts_with(&prefix));
                }
            }
            Action::Set(record) => {
                if let Some(at) = self.records.iter().position(|(id, _)| *id == report.id) {
                    self.records.remove(at);
                } else if self.records.len() >= MAX_RECORDS {
                    self.records.remove(0);
                }
                self.records.push((report.id, record));
            }
        }
    }

    /// Prompt start: remove the `working`, `blocked` and `idle` records,
    /// keeping `done` and `error`. Returns whether anything was removed.
    pub fn prompt_start(&mut self) -> bool {
        let before = self.records.len();
        self.records.retain(|(_, r)| {
            !matches!(
                r.state,
                RecordState::Working | RecordState::Blocked | RecordState::Idle
            )
        });
        self.records.len() != before
    }

    /// Reset: remove every record. Returns whether any existed.
    pub fn reset(&mut self) -> bool {
        let existed = !self.records.is_empty();
        self.records.clear();
        existed
    }

    /// The aggregate: absent for an empty table, else the deciding record
    /// (highest rank, ties to the most recently updated).
    pub fn summary(&self) -> Option<Summary> {
        let mut deciding: Option<&(String, Record)> = None;
        for entry in &self.records {
            if deciding.is_none_or(|d| entry.1.state.rank() >= d.1.state.rank()) {
                deciding = Some(entry);
            }
        }
        let (id, record) = deciding?;
        let title = record
            .title
            .as_deref()
            .map(sanitize_title_for_name)
            .filter(|t| !t.is_empty());
        Some(Summary {
            state: record.state,
            title,
            app: self.effective_app(id, record),
        })
    }

    /// The record's own app, else the app of its nearest ancestor record
    /// present (the root is every id's ancestor).
    fn effective_app(&self, id: &str, record: &Record) -> Option<String> {
        if record.app.is_some() {
            return record.app.clone();
        }
        let mut ancestor = id;
        while !ancestor.is_empty() {
            ancestor = ancestor.rsplit_once('/').map_or("", |(parent, _)| parent);
            let inherited = self
                .records
                .iter()
                .find(|(rid, _)| rid == ancestor)
                .and_then(|(_, r)| r.app.clone());
            if inherited.is_some() {
                return inherited;
            }
        }
        None
    }
}
