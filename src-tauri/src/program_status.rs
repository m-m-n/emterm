//! Build-agnostic Program Status core (OSC 7501, Program Status Protocol
//! draft 0.3): record-state vocabulary, body parser, per-terminal record
//! table, lifecycle operations, aggregate summary, title sanitization for
//! names, and re-validating export/import.
//!
//! Compiled WITHOUT the `gui` feature (CLI-shared): the plain-tab ingestion,
//! the mux daemon and the GUI model all consume this module, and it depends
//! on nothing feature-gated (SPEC NFR1).
//!
//! Written from the protocol text only; no external implementation was
//! consulted (SPEC NFR2).
//!
//! Wire grammar (the OSC body after the `ESC ] 7501 ;` introducer and before
//! the BEL / `ESC \` terminator):
//! - Query: the body is exactly `?`.
//! - Report: `key=value` pairs joined by `:`. Keys are lowercase ASCII
//!   letters; values use the bytes `A-Z a-z 0-9 _ . , + / = -`. Pairs that
//!   do not fit are skipped, unknown keys are ignored and the last
//!   surviving occurrence of a repeated key wins.
//!
//! Parsing is pure and side-effect free. A rejected report is never logged:
//! it is untrusted input and could flood the log.

use base64::Engine as _;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

/// Longest accepted whole sequence, introducer and terminator included.
pub const MAX_SEQUENCE_BYTES: usize = 4096;
/// Records kept per terminal; the least recently updated one is evicted
/// when a new id would exceed it.
pub const MAX_RECORDS: usize = 256;
/// Longest title used as a name, in Unicode scalar values.
pub const MAX_NAME_TITLE_CHARS: usize = 80;

/// `ESC ] 7501 ;` — ESC, `]`, the four digits and `;`.
const INTRODUCER_BYTES: usize = 7;
/// `ESC ]` — the part of the introducer that is not the OSC string.
const OSC_INTRODUCER_BYTES: usize = 2;
const MAX_ENCODED_TITLE_BYTES: usize = 256;
const MAX_DECODED_TITLE_BYTES: usize = 192;
const MAX_ENCODED_MSG_BYTES: usize = 2732;
const MAX_DECODED_MSG_BYTES: usize = 2048;
const MAX_ID_SEGMENTS: usize = 8;
const MAX_ID_SEGMENT_BYTES: usize = 32;
const MAX_ID_BYTES: usize = 128;
const MAX_APP_BYTES: usize = 32;
const MAX_PROGRESS: u32 = 100;

/// Standard-alphabet base64 that accepts both the padded and the unpadded
/// form.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

// ── Vocabulary ────────────────────────────────────────────────────────

/// The states a stored record can hold. `clear` is a report action, never
/// a stored state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

impl ProgramState {
    /// Every stored state, for exhaustive iteration.
    pub const ALL: [ProgramState; 5] = [
        ProgramState::Idle,
        ProgramState::Working,
        ProgramState::Done,
        ProgramState::Blocked,
        ProgramState::Error,
    ];

    /// The lowercase protocol word.
    pub fn word(self) -> &'static str {
        match self {
            ProgramState::Idle => "idle",
            ProgramState::Working => "working",
            ProgramState::Done => "done",
            ProgramState::Blocked => "blocked",
            ProgramState::Error => "error",
        }
    }

    /// The state for a protocol word; `None` for anything else (including
    /// `clear`, which is not a stored state).
    pub fn from_word(word: &str) -> Option<ProgramState> {
        match word {
            "idle" => Some(ProgramState::Idle),
            "working" => Some(ProgramState::Working),
            "done" => Some(ProgramState::Done),
            "blocked" => Some(ProgramState::Blocked),
            "error" => Some(ProgramState::Error),
            _ => None,
        }
    }

    /// The composition rank: blocked > working > error > done > idle. A
    /// larger value is a higher state. There is no read flag at this level.
    pub fn rank(self) -> u8 {
        match self {
            ProgramState::Idle => 0,
            ProgramState::Done => 1,
            ProgramState::Error => 2,
            ProgramState::Working => 3,
            ProgramState::Blocked => 4,
        }
    }
}

/// Why a `blocked` record is blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Permission,
    Question,
    Auth,
}

impl Kind {
    /// The lowercase protocol word.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Permission => "permission",
            Kind::Question => "question",
            Kind::Auth => "auth",
        }
    }

    /// The kind for a protocol word; `None` for anything else.
    pub fn from_word(word: &str) -> Option<Kind> {
        match word {
            "permission" => Some(Kind::Permission),
            "question" => Some(Kind::Question),
            "auth" => Some(Kind::Auth),
            _ => None,
        }
    }
}

/// How the OSC sequence ended. It decides the whole-sequence length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    /// BEL, one byte.
    Bel,
    /// ST (`ESC \`), two bytes.
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

// ── Parsed forms ──────────────────────────────────────────────────────

/// One stored record. Title and msg are decoded plain text and are never
/// interpreted; kind is held only for `blocked`, progress only for
/// `working` / `blocked`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub state: ProgramState,
    pub kind: Option<Kind>,
    pub progress: Option<u8>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
}

/// A validated report, ready to apply. The id is the empty string for the
/// root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// Replace the id's record with this one.
    Set { id: String, record: Record },
    /// Remove the id and its descendants; the empty id removes everything.
    Clear { id: String },
}

/// The outcome of parsing one OSC 7501 body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// The body is exactly `?`.
    Query,
    /// A fully validated report.
    Report(Report),
    /// Discarded or ignored: nothing changes.
    Ignored,
}

/// Parse the text that follows `7501;` in a terminated sequence.
pub fn parse(body: &str, terminator: Terminator) -> Parsed {
    parse_bytes(body.as_bytes(), terminator)
}

/// [`parse`] for callers that hold the body as raw bytes. The body is taken
/// to follow the canonical `7501;` introducer, so the received length of the
/// OSC string is that introducer plus the body.
pub fn parse_bytes(body: &[u8], terminator: Terminator) -> Parsed {
    parse_received(
        body,
        (INTRODUCER_BYTES - OSC_INTRODUCER_BYTES).saturating_add(body.len()),
        terminator,
    )
}

/// The single place the whole-sequence limit is computed
/// (osc7501-leading-zero-length SC-3).
///
/// `body` is the data of the OSC string as bytes (the lossily decoded text's
/// bytes). `received_osc_len` is the number of bytes of the OSC string as
/// received — leading zeros, every digit, the first `;` and every data byte
/// before any replacement included, `ESC ]` and the terminator excluded.
/// The string ended with BEL or ST: an unterminated string never reaches it.
///
/// A body that is exactly `?` is a query for any received length. Otherwise
/// the sequence is ignored when `ESC ]`, the received string and the
/// terminator, summed with saturation, exceed [`MAX_SEQUENCE_BYTES`];
/// otherwise the result is exactly what the body grammar gives for `body`.
/// Pure: no logging, no panic.
pub fn parse_received(body: &[u8], received_osc_len: usize, terminator: Terminator) -> Parsed {
    if body == b"?" {
        return Parsed::Query;
    }
    let sequence_len = OSC_INTRODUCER_BYTES
        .saturating_add(received_osc_len)
        .saturating_add(terminator.len());
    if sequence_len > MAX_SEQUENCE_BYTES {
        return Parsed::Ignored;
    }

    let pairs = Pairs::collect(body);

    let Ok(title) = decode_text(
        pairs.title,
        MAX_ENCODED_TITLE_BYTES,
        MAX_DECODED_TITLE_BYTES,
    ) else {
        return Parsed::Ignored;
    };
    let Ok(msg) = decode_text(pairs.msg, MAX_ENCODED_MSG_BYTES, MAX_DECODED_MSG_BYTES) else {
        return Parsed::Ignored;
    };

    let Some(state_word) = pairs.state.map(ascii) else {
        return Parsed::Ignored;
    };
    let clear = state_word == "clear";
    let state = ProgramState::from_word(state_word);
    if !clear && state.is_none() {
        return Parsed::Ignored;
    }

    let id = match pairs.id {
        None => String::new(),
        Some(value) if valid_id(value) => ascii(value).to_owned(),
        Some(_) => return Parsed::Ignored,
    };

    let Some(state) = state else {
        return Parsed::Report(Report::Clear { id });
    };

    let kind = match state {
        ProgramState::Blocked => pairs.kind.map(ascii).and_then(Kind::from_word),
        _ => None,
    };
    let progress = match state {
        ProgramState::Working | ProgramState::Blocked => pairs.progress,
        _ => None,
    };
    let app = pairs
        .app
        .filter(|value| valid_app(value))
        .map(|value| ascii(value).to_owned());

    Parsed::Report(Report::Set {
        id,
        record: Record {
            state,
            kind,
            progress,
            app,
            title,
            msg,
        },
    })
}

/// The surviving value of each known key, after the pair grammar and the
/// last-occurrence-wins rule.
#[derive(Default)]
struct Pairs<'a> {
    state: Option<&'a [u8]>,
    id: Option<&'a [u8]>,
    kind: Option<&'a [u8]>,
    progress: Option<u8>,
    app: Option<&'a [u8]>,
    title: Option<&'a [u8]>,
    msg: Option<&'a [u8]>,
}

impl<'a> Pairs<'a> {
    fn collect(body: &'a [u8]) -> Self {
        let mut pairs = Pairs::default();
        for pair in body.split(|&b| b == b':') {
            let Some(eq) = pair.iter().position(|&b| b == b'=') else {
                continue;
            };
            let (key, value) = (&pair[..eq], &pair[eq + 1..]);
            if key.is_empty() || !key.iter().all(u8::is_ascii_lowercase) {
                continue;
            }
            if !value.iter().all(|&b| is_value_byte(b)) {
                continue;
            }
            match key {
                b"state" => pairs.state = Some(value),
                b"id" => pairs.id = Some(value),
                b"kind" => pairs.kind = Some(value),
                b"progress" => {
                    if let Some(progress) = parse_progress(value) {
                        pairs.progress = Some(progress);
                    }
                }
                b"app" => pairs.app = Some(value),
                b"title" => pairs.title = Some(value),
                b"msg" => pairs.msg = Some(value),
                _ => {}
            }
        }
        pairs
    }
}

/// `A-Z a-z 0-9 _ . , + / = -`
fn is_value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b',' | b'+' | b'/' | b'=' | b'-')
}

/// `A-Z a-z 0-9 _ . + -`
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-')
}

/// Value bytes are validated ASCII before this is reached.
fn ascii(value: &[u8]) -> &str {
    std::str::from_utf8(value).unwrap_or("")
}

/// Decimal digits only, 0 to 100.
fn parse_progress(value: &[u8]) -> Option<u8> {
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut n: u32 = 0;
    for &digit in value {
        n = n * 10 + u32::from(digit - b'0');
        if n > MAX_PROGRESS {
            return None;
        }
    }
    u8::try_from(n).ok()
}

/// One to eight `/`-separated segments of one to 32 name bytes, 128 bytes
/// at most in total.
fn valid_id(id: &[u8]) -> bool {
    if id.is_empty() || id.len() > MAX_ID_BYTES {
        return false;
    }
    let mut segments = 0;
    for segment in id.split(|&b| b == b'/') {
        segments += 1;
        if segments > MAX_ID_SEGMENTS
            || segment.is_empty()
            || segment.len() > MAX_ID_SEGMENT_BYTES
            || !segment.iter().all(|&b| is_name_byte(b))
        {
            return false;
        }
    }
    true
}

fn valid_app(app: &[u8]) -> bool {
    !app.is_empty() && app.len() <= MAX_APP_BYTES && app.iter().all(|&b| is_name_byte(b))
}

/// Decode a base64 title / msg value. `Err` means the whole report is
/// discarded; an empty decoded text counts as absent.
fn decode_text(
    value: Option<&[u8]>,
    max_encoded: usize,
    max_decoded: usize,
) -> Result<Option<String>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > max_encoded {
        return Err(());
    }
    let bytes = BASE64.decode(value).map_err(|_| ())?;
    if bytes.len() > max_decoded {
        return Err(());
    }
    let text = String::from_utf8(bytes).map_err(|_| ())?;
    if text.chars().any(char::is_control) {
        return Err(());
    }
    Ok(if text.is_empty() { None } else { Some(text) })
}

// ── Title sanitization for names ──────────────────────────────────────

/// Turn a title into something safe to use as a name: control characters
/// (C0, DEL, C1) and invisible formatting characters are removed, then the
/// result is cut to [`MAX_NAME_TITLE_CHARS`] Unicode scalar values. The
/// text is otherwise left as is; it is never interpreted.
pub fn sanitize_title(title: &str) -> String {
    title
        .chars()
        .filter(|&c| !c.is_control() && !is_invisible_format(c))
        .take(MAX_NAME_TITLE_CHARS)
        .collect()
}

fn is_invisible_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD                  // soft hyphen
        | 0x061C                // Arabic letter mark
        | 0x180E                // Mongolian vowel separator
        | 0x200B..=0x200F       // zero-width space / joiners / direction marks
        | 0x2028..=0x202E       // line / paragraph separators, bidi embeddings
        | 0x2060..=0x206F       // word joiner, invisible operators, bidi isolates
        | 0xFEFF                // zero-width no-break space / BOM
        | 0xFFF9..=0xFFFB       // interlinear annotation
        | 0xE0000..=0xE007F     // tag characters
    )
}

// ── Record table ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: String,
    record: Record,
}

/// The aggregate of a table: the deciding record's state, its sanitized
/// title, and its effective app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub state: ProgramState,
    /// Absent when the deciding record has no title or the title is empty
    /// after sanitization.
    pub title: Option<String>,
    /// The deciding record's own app, else the app of its nearest ancestor
    /// record present at summary time.
    pub app: Option<String>,
}

/// One record as plain values, for handoff between processes. The empty id
/// denotes the root; the state and the kind are protocol words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedRecord {
    pub id: String,
    pub state: String,
    pub kind: Option<String>,
    pub progress: Option<u32>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
}

/// The record table of one terminal. Records are kept least recently
/// updated first, which is the order eviction and export follow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    entries: Vec<Entry>,
}

impl Table {
    /// Number of records.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The record stored under `id` (the empty id is the root).
    pub fn get(&self, id: &str) -> Option<&Record> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.record)
    }

    /// Records with their ids, least recently updated first.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Record)> {
        self.entries
            .iter()
            .map(|entry| (entry.id.as_str(), &entry.record))
    }

    /// Apply an accepted report. A set replaces the id's record entirely and
    /// makes it the most recently updated; a clear removes a subtree or
    /// everything. Every accepted report counts as accepted whether or not
    /// the stored data changed.
    pub fn apply(&mut self, report: Report) {
        match report {
            Report::Set { id, record } => self.put(id, record),
            Report::Clear { id } if id.is_empty() => self.entries.clear(),
            Report::Clear { id } => self.entries.retain(|entry| !in_subtree(&entry.id, &id)),
        }
    }

    /// A live main-screen prompt start: removes `working`, `blocked` and
    /// `idle` records and keeps `done` and `error`. Returns whether anything
    /// was removed.
    pub fn prompt_start(&mut self) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|entry| matches!(entry.record.state, ProgramState::Done | ProgramState::Error));
        self.entries.len() != before
    }

    /// Remove every record. Returns whether any existed.
    pub fn reset(&mut self) -> bool {
        let existed = !self.entries.is_empty();
        self.entries.clear();
        existed
    }

    /// The aggregate of the table; `None` when it is empty.
    pub fn summary(&self) -> Option<Summary> {
        let mut deciding: Option<&Entry> = None;
        for entry in &self.entries {
            // Oldest first, so on a rank tie the later (more recently
            // updated) record replaces the earlier one.
            match deciding {
                Some(best) if entry.record.state.rank() < best.record.state.rank() => {}
                _ => deciding = Some(entry),
            }
        }
        let deciding = deciding?;
        Some(Summary {
            state: deciding.record.state,
            title: deciding
                .record
                .title
                .as_deref()
                .map(sanitize_title)
                .filter(|title| !title.is_empty()),
            app: self.effective_app(deciding),
        })
    }

    /// Every record as plain values, least recently updated first.
    pub fn export(&self) -> Vec<ExportedRecord> {
        self.entries
            .iter()
            .map(|entry| ExportedRecord {
                id: entry.id.clone(),
                state: entry.record.state.word().to_owned(),
                kind: entry.record.kind.map(|kind| kind.word().to_owned()),
                progress: entry.record.progress.map(u32::from),
                app: entry.record.app.clone(),
                title: entry.record.title.clone(),
                msg: entry.record.msg.clone(),
            })
            .collect()
    }

    /// Rebuild a table from exported records, oldest first. Each record is
    /// re-validated against the id, app, kind, progress, title and msg rules
    /// and dropped when it violates one; when more than [`MAX_RECORDS`]
    /// valid records remain, the most recently updated ones are kept.
    pub fn import(records: impl IntoIterator<Item = ExportedRecord>) -> Table {
        let mut table = Table::default();
        for exported in records {
            if let Some((id, record)) = validate_exported(exported) {
                table.put(id, record);
            }
        }
        table
    }

    fn put(&mut self, id: String, record: Record) {
        if let Some(pos) = self.entries.iter().position(|entry| entry.id == id) {
            self.entries.remove(pos);
        } else if self.entries.len() >= MAX_RECORDS {
            self.entries.remove(0);
        }
        self.entries.push(Entry { id, record });
    }

    /// The record's own app, else the app of the nearest ancestor record
    /// that has one (the root is every id's ancestor).
    fn effective_app(&self, entry: &Entry) -> Option<String> {
        if let Some(app) = &entry.record.app {
            return Some(app.clone());
        }
        let mut id = entry.id.as_str();
        while !id.is_empty() {
            id = id.rfind('/').map_or("", |slash| &id[..slash]);
            if let Some(app) = self.get(id).and_then(|record| record.app.as_ref()) {
                return Some(app.clone());
            }
        }
        None
    }
}

/// `id` is `root` itself or lies below it on a segment boundary.
fn in_subtree(id: &str, root: &str) -> bool {
    id == root
        || id
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn validate_exported(exported: ExportedRecord) -> Option<(String, Record)> {
    let ExportedRecord {
        id,
        state,
        kind,
        progress,
        app,
        title,
        msg,
    } = exported;

    if !id.is_empty() && !valid_id(id.as_bytes()) {
        return None;
    }
    let state = ProgramState::from_word(&state)?;
    let kind = match kind {
        None => None,
        Some(word) if state == ProgramState::Blocked => Some(Kind::from_word(&word)?),
        Some(_) => return None,
    };
    let progress = match progress {
        None => None,
        Some(value)
            if matches!(state, ProgramState::Working | ProgramState::Blocked)
                && value <= MAX_PROGRESS =>
        {
            Some(u8::try_from(value).ok()?)
        }
        Some(_) => return None,
    };
    if app.as_ref().is_some_and(|app| !valid_app(app.as_bytes())) {
        return None;
    }
    let title = validate_text(title, MAX_DECODED_TITLE_BYTES)?;
    let msg = validate_text(msg, MAX_DECODED_MSG_BYTES)?;

    Some((
        id,
        Record {
            state,
            kind,
            progress,
            app,
            title,
            msg,
        },
    ))
}

/// `None` when the text breaks a rule; an empty text counts as absent.
fn validate_text(text: Option<String>, max_bytes: usize) -> Option<Option<String>> {
    match text {
        None => Some(None),
        Some(text) if text.is_empty() => Some(None),
        Some(text) if text.len() > max_bytes || text.chars().any(char::is_control) => None,
        Some(text) => Some(Some(text)),
    }
}

#[cfg(test)]
mod tests;
