//! IPC message types and frame format for mux daemon communication.
//!
//! Frame format: [length: u32][type: u8][pane_id: u32][payload: variable]
//! - length: remaining bytes after the length field (= 5 + payload_len)
//! - PTY data uses raw bytes payload
//! - Control messages use bincode-serialized payload

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};

/// Protocol version for handshake compatibility check.
///
/// Bumped from 1 to 2 for the mux agent-status / agent-API message
/// additions (`AgentStatusUpdate`, `ReadPane`/`ReadPaneResult`,
/// `SendText`/`SendTextResult`, `WaitAgentState`/`WaitAgentStateResult`,
/// `AgentApiError`). No existing message's encoded bytes changed.
///
/// Bumped from 2 to 3 (mux-render-corruption round-4/5 rework, review
/// round-4 finding `fdfd391ba97167de`): the `Snapshot` / `SnapshotRestore`
/// payload format changed INCOMPATIBLY — it is now the `EMSNAP2`-prefixed
/// structural-segment envelope (see [`encode_snapshot_payload`] /
/// [`decode_snapshot_payload_typed`]), not the plain ANSI bytes a v2 client
/// expects. A v2 GUI reading a v3 daemon's snapshot would render the magic
/// bytes and segment table as literal terminal content. The decode side's
/// magic-prefix sniff is a DELIBERATE compatibility shim for the OTHER
/// direction only (an old, v2-speaking daemon that a current GUI reattaches
/// to — see [`decode_snapshot_payload_typed`]'s `Legacy` variant) and does
/// not contradict the handshake guarantee below: a v3 daemon meeting a v2
/// `HelloMsg` still rejects it via `WelcomeMsg::Rejected`, exactly as the
/// previous bump did, precisely so this incompatible payload change cannot
/// reach a client that would misinterpret it.
///
/// The handshake path (`HelloMsg::protocol_version` vs this constant,
/// checked in `mux/ipc/connection.rs`) rejects a mismatched client
/// cleanly via `WelcomeMsg::Rejected` — there is no silent compatibility
/// shim between protocol versions on the daemon side.
///
/// Client-side recovery (task0010 rework, strategy B): a hard version bump
/// alone would strand a long-lived old daemon after an eMterm upgrade, since
/// daemon discovery (`ensure_daemon_running`) was presence-based and never
/// probed compatibility. `mux/daemon.rs` now performs a real handshake
/// before trusting an already-running daemon; on a version mismatch it
/// retries with [`PREVIOUS_PROTOCOL_VERSION`] (which the older daemon
/// accepts) and sends a version-tolerant `Shutdown`, then relaunches a
/// current-version daemon. See IMPLEMENTATION.md "Old GUI × new daemon
/// pairing".
pub const PROTOCOL_VERSION: u32 = 3;

/// The protocol version immediately preceding [`PROTOCOL_VERSION`].
///
/// Used only for the client-side legacy-daemon recovery handshake retry
/// (`mux/daemon.rs::recover_from_legacy_daemon` /
/// `shutdown_daemon_any_version`, task0010 rework): a v2 client that meets a
/// daemon rejecting its v2 Hello retries with this version so an adjacent
/// older daemon accepts the connection and can be sent a `Shutdown`.
/// Deliberately supports only one version back — recovering a daemon more
/// than one bump behind is out of scope (see task0010's plan "Out of
/// Scope").
pub const PREVIOUS_PROTOCOL_VERSION: u32 = PROTOCOL_VERSION - 1;

/// APC prefix for identifying emterm mux APC sequences.
pub const APC_PREFIX: &str = "emterm-mux;";

/// APC introducer: ESC _
const APC_START: &str = "\x1b_";

/// APC string terminator: ESC \
const APC_ST: &str = "\x1b\\";

/// OSC parameter for emterm mux messages.
pub const MUX_OSC_PARAM: u16 = 9999;

/// Plaintext transport prefix for mux messages on the Windows ConPTY input
/// direction (`EMUX;<base64>\r`, where APC/OSC escapes do not survive ConPTY
/// input and a raw LF is dropped under `PSEUDOCONSOLE_WIN32_INPUT_MODE`).
/// The bridge parser also accepts LF / CRLF / LFCR for resilience. Kept here
/// alongside `APC_PREFIX` and `MUX_OSC_PARAM` so all three mux transport
/// markers share one SSOT.
pub const PLAINTEXT_PREFIX: &[u8] = b"EMUX;";

/// OSC introducer: ESC ]
const OSC_START: &str = "\x1b]";

/// Maximum IPC frame size (16MB) to prevent OOM.
pub const MAX_FRAME_LENGTH: usize = 16 * 1024 * 1024;

/// Bytes consumed by a frame body's fixed header (`[type: u8][pane_id: u32]`,
/// see [`MuxMessage::to_frame_body`]) — the only overhead a payload pays
/// before it must fit inside [`MAX_FRAME_LENGTH`].
pub const FRAME_HEADER_LEN: usize = 5;

/// The largest a single message's `payload` may be while still fitting in
/// one codec frame (task0004 round-4 rework, D4' / review round-3 finding
/// `ea222e74bb0a046c`): derived directly from the protocol's own hard limit
/// rather than an ad-hoc "comfortably below today's realistic max" margin,
/// so a caller deciding "does this snapshot need chunking" cannot pick a
/// threshold that under- or over-shoots the actual wire constraint.
pub const MAX_SNAPSHOT_FRAME_PAYLOAD: usize = MAX_FRAME_LENGTH - FRAME_HEADER_LEN;

/// Whether an encoded snapshot payload (segment header + content bytes,
/// [`encode_snapshot_payload`]'s output) fits in a single codec frame — the
/// ONE size-policy check every snapshot producer must go through (task0005
/// rework D6'', review round-4 finding `1d4a0c96821da0ef`: producers had
/// been checking this independently — `mux::ipc::reattach`'s
/// `send_reattach_data` did, `mux::ipc::handlers`'
/// `handle_request_pane_snapshot` and the visibility-resume path did not —
/// so whether an oversized snapshot degraded gracefully or tore down the
/// connection depended on which code path produced it).
pub fn fits_single_snapshot_frame(encoded_len: usize) -> bool {
    encoded_len <= MAX_SNAPSHOT_FRAME_PAYLOAD
}

/// A structural dimension segment (task0004 round-4 rework, D1'): content
/// starting at byte `offset` into a snapshot payload's bytes was produced
/// under `(cols, rows)`, until the next segment (if any, in the same list)
/// takes over. Carried ALONGSIDE the payload bytes (see
/// [`encode_snapshot_payload`] / [`decode_snapshot_payload`]) — never
/// encoded as a recognizable byte sequence inside them. This is the
/// structural replacement for the in-band `OSC 777;emterm;resize;…` marker
/// bytes rounds 1-3 tried (and repeatedly failed) to filter out of the byte
/// stream: with dimensions carried here, no byte sequence a child process
/// can produce is ever interpreted as a dimension change, because nothing
/// scans the payload for one any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimSegment {
    pub offset: u32,
    pub cols: u16,
    pub rows: u16,
}

/// Magic prefix identifying the structural-segments snapshot payload format.
/// Chosen to be vanishingly unlikely to appear at the start of a LEGACY
/// snapshot payload (produced by a daemon that predates this format): such a
/// payload is either empty or begins with the snapshot clear-prefix
/// (`ESC[3J…` / `ESC[H…`, always starting with the single byte `0x1b`), never
/// with these 8 specific bytes. See [`decode_snapshot_payload`]'s doc comment
/// for the compatibility contract this enables (task0004 AC-11).
const SNAPSHOT_PAYLOAD_MAGIC: [u8; 8] = *b"EMSNAP2\0";

/// Maximum number of [`DimSegment`] entries a decoded structured payload may
/// declare (task0005 rework D2'', review round-4 finding
/// `1cd7b5e593f3b901`; round-6 rework D4''', review round-5 finding
/// `bfebad60b3862d3e`).
///
/// The old decoder's `count.min(4096)` only bounded the initial `Vec`
/// allocation — the per-entry parse loop still ran the FULL declared
/// `count`, so a frame well under the 64 KiB off-thread-replay threshold
/// could still declare a segment table approaching `MAX_FRAME_LENGTH / 8`
/// (~2,000,000 entries) and force that many synchronous reflows on the
/// caller's UI thread. Round-6 tightened the ceiling itself to a small,
/// generous multiple of the daemon's OWN recording bound
/// (`mux::scrollback_buffer::MAX_DIM_MARKERS`, then 24) — since every
/// non-empty segment costs a reflow, a compact hostile frame well under
/// any byte-size threshold could still buy replay-worker time
/// proportional to segment count (round-4 measured 80 segments / 0.95 MiB
/// → 5.35 s).
///
/// D1'''''' (round-9 rework, review round-8 finding `6082de4e619d7f51`):
/// the DERIVATION direction reversed. `MAX_DIM_MARKERS` was raised to 62 —
/// `mux::scrollback_buffer::MAX_DAEMON_SNAPSHOT_SEGMENTS`'s doc explains
/// why — chosen as `MAX_SEGMENTS - 2` (one slot for a synthesized head
/// segment, one for a trailing alt-screen dump), so a genuine
/// daemon-recorded payload can now legitimately reach this ceiling
/// EXACTLY, not "never remotely close to it" as before. This is
/// intentional and still safe: `count > MAX_SEGMENTS` is the rejection
/// test (strictly greater), so a legitimate `count == MAX_SEGMENTS`
/// payload decodes cleanly (see
/// `largest_daemon_producible_segment_list_round_trips_cleanly` /
/// `largest_real_producer_segment_list_round_trips_cleanly` in
/// `mux::session::pane`'s tests) while a hostile frame still cannot
/// exceed it. `mux_ipc` has no dependency on the daemon crate, so the
/// daemon-side constant is duplicated here as a literal; keep the two in
/// sync if either changes.
pub const MAX_SEGMENTS: usize = 64;

/// Maximum total cell count (`cols as u32 * rows as u32`) a single
/// [`DimSegment`] may declare (round-6 rework D5''', review round-5 finding
/// `1227fc04fb9368d0`).
///
/// `term_core::clamp_resize_dims` bounds `cols` and `rows` individually to
/// 4096 each, but a single segment at exactly that bound still allocates a
/// grid on the order of half a gigabyte (4096 × 4096 cells at 32 bytes/cell,
/// before accounting for the previous grid it replaces or any scrollback
/// capacity added on top) — clamping each DIMENSION separately still admits
/// an adversarial PRODUCT. No real terminal approaches this cell count (a
/// 4K display at a tiny font is on the order of a few hundred thousand
/// cells); rejecting the whole snapshot before any per-segment allocation
/// happens costs no legitimate payload.
pub const MAX_SEGMENT_CELLS: u32 = 1_000_000;

/// Maximum CUMULATIVE cell count (`sum of cols as u64 * rows as u64`)
/// across every segment a single decoded payload may declare (round-7
/// rework, D2'''', review round-6 finding `02bb52aaff9638e5`).
///
/// [`MAX_SEGMENT_CELLS`] bounds one segment's own declared cost, but not
/// the SUM across up to [`MAX_SEGMENTS`] (64) of them — a small,
/// individually-valid payload can still declare 64 segments each at the
/// per-segment ceiling, forcing roughly 64,000,000 cells of grid
/// allocation / reflow work. Fewer than
/// `tabs::OFFTHREAD_REPLAY_SEGMENT_THRESHOLD` (8) non-empty segments still
/// replay SYNCHRONOUSLY on the caller's UI thread, so this is not merely a
/// background-CPU concern — it can stall the switch outright. A REAL
/// daemon-recorded payload never approaches this: the daemon's own
/// `MAX_DIM_MARKERS` bounds segment count, and a real terminal's cell
/// count is orders of magnitude below `MAX_SEGMENT_CELLS`.
///
/// D1'''''' (round-9 rework, review round-8 finding `6082de4e619d7f51` /
/// `45033eaafbdf8e25`): raised from 8x to 32x `MAX_SEGMENT_CELLS` (8,000,000
/// → 32,000,000). `MAX_DIM_MARKERS` rose to 62 (`mux::scrollback_buffer`'s
/// doc) alongside `MAX_DAEMON_SNAPSHOT_SEGMENTS` rising to 64
/// (`mux::session::pane`'s doc), which — left at the OLD 8,000,000 —
/// would derive a producer per-segment budget of 8,000,000 / 64 =
/// 125,000 cells: not comfortably above "a 4K display at a tiny font"
/// (this constant's sibling [`MAX_SEGMENT_CELLS`] doc's own "a few
/// hundred thousand cells" estimate already exceeds it). Raised so the
/// derived per-segment producer budget (32,000,000 / 64 = 500,000) stays
/// ABOVE the old budget (307,692 at the old 26-segment count) instead of
/// shrinking underneath a wider real display — real terminal dimensions
/// fit comfortably again rather than by coincidence. Still well below the
/// worst case a full `MAX_SEGMENTS` table of max-size segments could
/// declare (64,000,000).
pub const MAX_CUMULATIVE_SEGMENT_CELLS: u64 = 32 * MAX_SEGMENT_CELLS as u64;

/// Result of decoding a snapshot payload (task0005 rework D2'', replacing
/// the ambiguous `(Vec<DimSegment>, &[u8])` tuple [`decode_snapshot_payload`]
/// still returns for existing callers). The tuple form could not
/// distinguish "no magic prefix — legacy raw content" from "magic prefix
/// present but the segment table is corrupt", so a corrupted structured
/// frame silently fell back to "the whole payload (magic bytes and all) is
/// terminal content" — review round-4 finding `5299d50f586b8cb8`: that
/// literally renders the protocol envelope on screen instead of being
/// recognized as damaged transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedSnapshotPayload<'a> {
    /// `payload` does not start with [`SNAPSHOT_PAYLOAD_MAGIC`]: the WHOLE
    /// input is legacy raw content, no structural dimension segments — the
    /// pre-task0001 single-dimension replay degrade path (task0004 AC-11).
    Legacy(&'a [u8]),
    /// Magic prefix present and the segment table parsed successfully
    /// (`count` bounded by [`MAX_SEGMENTS`], table length verified with
    /// checked arithmetic before any entry is parsed).
    Structured {
        segments: Vec<DimSegment>,
        content: &'a [u8],
    },
    /// Magic prefix present but the segment table is truncated, declares a
    /// count above [`MAX_SEGMENTS`], or its length overflows the
    /// checked-arithmetic bounds check. This is a CORRUPTED structured
    /// frame — never legacy content — and a caller MUST NOT replay
    /// `payload` as terminal bytes for it (see [`decode_snapshot_payload`]'s
    /// doc for how the tuple-form compatibility wrapper handles this).
    Malformed,
}

/// Encode a snapshot payload in the D1' structural format: `segments` (the
/// sole authority for "which dimensions applied to which bytes") followed by
/// `bytes` (the plain ANSI payload, carrying no dimension information of its
/// own). `segments` may be empty.
///
/// Wire layout: `MAGIC(8) | segment_count(u32 LE) | segment_count *
/// (offset(u32 LE) cols(u16 LE) rows(u16 LE)) | bytes`.
pub fn encode_snapshot_payload(segments: &[DimSegment], bytes: &[u8]) -> Vec<u8> {
    let mut out =
        Vec::with_capacity(SNAPSHOT_PAYLOAD_MAGIC.len() + 4 + segments.len() * 8 + bytes.len());
    out.extend_from_slice(&SNAPSHOT_PAYLOAD_MAGIC);
    out.extend_from_slice(&(segments.len() as u32).to_le_bytes());
    for seg in segments {
        out.extend_from_slice(&seg.offset.to_le_bytes());
        out.extend_from_slice(&seg.cols.to_le_bytes());
        out.extend_from_slice(&seg.rows.to_le_bytes());
    }
    out.extend_from_slice(bytes);
    out
}

/// Decode a snapshot payload produced by [`encode_snapshot_payload`],
/// distinguishing legacy content, a successfully parsed structured payload,
/// and a corrupted structured payload (task0005 rework D2'', review
/// round-4 finding `5299d50f586b8cb8`).
///
/// **Older-daemon compatibility (task0004 AC-11):** when `payload` does not
/// start with [`SNAPSHOT_PAYLOAD_MAGIC`], this returns
/// [`DecodedSnapshotPayload::Legacy`]: the WHOLE input is legacy raw bytes
/// with no structural dimension info. A caller feeding that through the
/// segment-aware replay path
/// (`term_core::terminal_core::TerminalCore::reset_and_replay_segments`
/// with an empty segment list) gets exactly the pre-task0001
/// single-dimension replay behavior — the documented degradation for an
/// older daemon that never adopted this wire format. This is a graceful
/// fallback, not a security boundary: a payload that happens to start with
/// the 8-byte magic by coincidence would be misparsed, but real ANSI
/// snapshot payloads always begin with the clear-prefix `ESC` byte (see
/// [`SNAPSHOT_PAYLOAD_MAGIC`]'s doc comment).
///
/// When the magic prefix IS present, the segment table's declared `count`
/// is validated with checked arithmetic (against both [`MAX_SEGMENTS`] and
/// the actual remaining byte length) BEFORE any entry is parsed — a
/// truncated table, an overflow-inducing `count`, or a `count` above
/// `MAX_SEGMENTS` all yield [`DecodedSnapshotPayload::Malformed`]. This is
/// DELIBERATELY not the same as `Legacy`: the magic prefix proves the
/// producer meant this to be a structured payload, so a corrupted table
/// must never be handed to a caller as if it were plain terminal content
/// (that would render the magic bytes and partial table literally on
/// screen).
pub fn decode_snapshot_payload_typed(payload: &[u8]) -> DecodedSnapshotPayload<'_> {
    if !payload.starts_with(&SNAPSHOT_PAYLOAD_MAGIC) {
        return DecodedSnapshotPayload::Legacy(payload);
    }
    let rest = &payload[SNAPSHOT_PAYLOAD_MAGIC.len()..];
    let Some(count_bytes) = rest.get(0..4) else {
        return DecodedSnapshotPayload::Malformed;
    };
    let count = u32::from_le_bytes(count_bytes.try_into().expect("4-byte slice")) as usize;
    if count > MAX_SEGMENTS {
        return DecodedSnapshotPayload::Malformed;
    }
    // Checked arithmetic (D2''): verify the WHOLE table fits within `rest`
    // before parsing a single entry, rather than discovering truncation
    // only after iterating partway through a possibly-huge declared count.
    let Some(table_bytes) = count.checked_mul(8) else {
        return DecodedSnapshotPayload::Malformed;
    };
    let Some(table_end) = 4usize.checked_add(table_bytes) else {
        return DecodedSnapshotPayload::Malformed;
    };
    if rest.len() < table_end {
        return DecodedSnapshotPayload::Malformed;
    }
    let mut segments = Vec::with_capacity(count);
    let mut cursor = 4usize;
    // D2''' (round-6 rework, review round-5 finding `58db33c799bedf87`):
    // validated ALONGSIDE parsing, not as a separate pass — a non-monotonic
    // offset, or one past `content`'s length, are malformed-envelope
    // conditions (never assigned authority over content, per the same
    // policy the count/table-length checks above already apply): a
    // non-monotonic entry produces an `end < start` range that drops
    // content.
    //
    // D1''''' (round-8 rework, review round-7 finding `01f91fe698ceb287`):
    // a non-zero LEADING offset is NO LONGER rejected here. The daemon-side
    // cap-eviction gap (`ScrollbackRingBuffer::read_segments`, 2+ evicted
    // `dim_markers` entries) now legitimately produces a segment list whose
    // first entry starts after offset 0, leaving that leading span
    // deliberately unattributed. `TerminalCore::replay_segments` was fixed
    // in lockstep to replay that leading span (at the caller's target dims)
    // instead of silently dropping it — see that function's doc — so a
    // non-zero leading offset is a normal, well-formed payload now, not the
    // "drops content" hazard the pre-fix comment described.
    let content_len = rest.len() - table_end;
    let mut prev_offset: Option<u32> = None;
    // D5''' (round-6 rework, review round-5 finding `1227fc04fb9368d0`):
    // clamping `cols`/`rows` to `RESIZE_MARKER_MAX_COLS`/`_ROWS`
    // individually (term_core's `clamp_resize_dims`, applied again at
    // replay time) still admits a single segment whose PRODUCT allocates
    // on the order of half a gigabyte (4096 × 4096 cells). Reject the
    // whole snapshot — before any per-segment allocation happens — if any
    // segment's total cell count exceeds a budget no legitimate terminal
    // size approaches.
    //
    // D2'''' (round-7 rework, review round-6 finding `02bb52aaff9638e5`):
    // the per-segment budget bounds ONE segment but not their SUM — track
    // the running total (as `u64`; `MAX_SEGMENTS * MAX_SEGMENT_CELLS` is
    // ~64,000,000, far below any overflow risk) and reject the whole
    // snapshot the moment it exceeds `MAX_CUMULATIVE_SEGMENT_CELLS`, before
    // any per-segment allocation happens.
    let mut cumulative_cells: u64 = 0;
    for _ in 0..count {
        // Bounds already verified above (`table_end <= rest.len()`); this
        // slice cannot panic.
        let seg_bytes = &rest[cursor..cursor + 8];
        let offset = u32::from_le_bytes(seg_bytes[0..4].try_into().expect("4-byte slice"));
        let cols = u16::from_le_bytes(seg_bytes[4..6].try_into().expect("2-byte slice"));
        let rows = u16::from_le_bytes(seg_bytes[6..8].try_into().expect("2-byte slice"));
        if let Some(prev) = prev_offset {
            if offset < prev {
                return DecodedSnapshotPayload::Malformed;
            }
        }
        if offset as usize > content_len {
            return DecodedSnapshotPayload::Malformed;
        }
        prev_offset = Some(offset);
        let cell_count = (cols as u32) * (rows as u32);
        if cell_count > MAX_SEGMENT_CELLS {
            return DecodedSnapshotPayload::Malformed;
        }
        cumulative_cells += cell_count as u64;
        if cumulative_cells > MAX_CUMULATIVE_SEGMENT_CELLS {
            return DecodedSnapshotPayload::Malformed;
        }
        segments.push(DimSegment { offset, cols, rows });
        cursor += 8;
    }
    DecodedSnapshotPayload::Structured {
        segments,
        content: &rest[cursor..],
    }
}

/// Backward-compatible tuple-shaped wrapper over
/// [`decode_snapshot_payload_typed`], for callers that only ever cared
/// about "segments + content" and treated any decode failure as "fall back
/// to legacy interpretation" (most existing call sites and tests).
///
/// [`DecodedSnapshotPayload::Malformed`] maps to `(Vec::new(), &[])` — empty
/// segments AND empty content — NOT `(Vec::new(), payload)` as the
/// pre-task0005 implementation did. Handing a caller the raw magic + corrupt
/// table bytes as "content" would have them replayed as literal terminal
/// text (review round-4 finding `5299d50f586b8cb8`); an empty replay is the
/// safe degradation instead. A caller that needs to distinguish "nothing to
/// show" (`Malformed`) from "legitimately empty snapshot" should use
/// [`decode_snapshot_payload_typed`] directly (`tabs.rs`'s
/// `apply_mux_message` does, so it can log and skip applying the frame
/// entirely rather than resetting the display to blank).
pub fn decode_snapshot_payload(payload: &[u8]) -> (Vec<DimSegment>, &[u8]) {
    match decode_snapshot_payload_typed(payload) {
        DecodedSnapshotPayload::Legacy(content) => (Vec::new(), content),
        DecodedSnapshotPayload::Structured { segments, content } => (segments, content),
        DecodedSnapshotPayload::Malformed => (Vec::new(), &[]),
    }
}

/// Message type identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    PtyOutput = 0x01,
    PtyInput = 0x02,
    Hello = 0x03,
    Welcome = 0x04,
    CreatePane = 0x05,
    PaneCreated = 0x06,
    DestroyPane = 0x07,
    Resize = 0x08,
    Attach = 0x09,
    Detach = 0x0A,
    Detached = 0x0B,
    Snapshot = 0x0C,
    SnapshotRestore = 0x0D,
    SessionList = 0x0E,
    Error = 0x0F,
    PtyExited = 0x10,
    // Phase 3+ message types
    CreateWindow = 0x12,
    SwitchWindow = 0x13,
    RenameWindow = 0x14,
    DestroyWindow = 0x15,
    // 0x16 / 0x17 were `StatusUpdate` / `RequestStatusUpdate` (the mux
    // status-bar daemon→GUI push and its GUI→daemon request), removed by
    // mux-status-bar-removal task0001. RESERVED — never reassign these
    // values, so a stale peer still emitting either opcode decodes as a
    // benign unknown-type frame (discarded, at most a warn log) rather
    // than colliding with a new message.
    Shutdown = 0x18,
    RequestPaneSnapshot = 0x19,
    MoveWindow = 0x1A,
    SetVisibility = 0x1B,
    /// Daemon-originated desktop notification (OSC 9) detected on a Detached
    /// pane. Forwarded to the GUI client, which fires the OS notification.
    Notify = 0x1C,
    /// Daemon → GUI unsolicited push: a mux pane's agent status changed (or
    /// is being restated after snapshot/reattach with `replay_derived`).
    AgentStatusUpdate = 0x1D,
    /// Request: read the last N lines of a mux pane's visible content.
    ReadPane = 0x1E,
    /// Response to `ReadPane`.
    ReadPaneResult = 0x1F,
    /// Request: send text (raw bytes) to a mux pane's PTY.
    SendText = 0x20,
    /// Response to `SendText`.
    SendTextResult = 0x21,
    /// Request: block until a mux pane's agent state matches a target set.
    WaitAgentState = 0x22,
    /// Response to `WaitAgentState`.
    WaitAgentStateResult = 0x23,
    /// Structured error response shared by `ReadPane` / `SendText` /
    /// `WaitAgentState`.
    AgentApiError = 0x24,
    /// Client → daemon: request an in-place upgrade of the running daemon
    /// via `execve` (SPEC FR1, mux-daemon-hot-upgrade task0001). Mirrors
    /// `Shutdown`'s wire shape exactly: type byte, pane id zero, empty
    /// payload. An older peer that does not recognise this discriminant
    /// discards the frame through the existing unknown-type path in
    /// [`MuxMessage::from_frame_body`] rather than erroring.
    Upgrade = 0x25,
    /// Daemon → client: broadcast to every connected client immediately
    /// before `execve` replaces the process (SPEC FR2), so a client can
    /// distinguish an upgrade-induced disconnect from an ordinary shutdown.
    /// Same empty-payload, pane-id-zero wire shape as `Upgrade` /
    /// `Shutdown`.
    Upgrading = 0x26,
}

impl MessageType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::PtyOutput),
            0x02 => Some(Self::PtyInput),
            0x03 => Some(Self::Hello),
            0x04 => Some(Self::Welcome),
            0x05 => Some(Self::CreatePane),
            0x06 => Some(Self::PaneCreated),
            0x07 => Some(Self::DestroyPane),
            0x08 => Some(Self::Resize),
            0x09 => Some(Self::Attach),
            0x0A => Some(Self::Detach),
            0x0B => Some(Self::Detached),
            0x0C => Some(Self::Snapshot),
            0x0D => Some(Self::SnapshotRestore),
            0x0E => Some(Self::SessionList),
            0x0F => Some(Self::Error),
            0x10 => Some(Self::PtyExited),
            0x12 => Some(Self::CreateWindow),
            0x13 => Some(Self::SwitchWindow),
            0x14 => Some(Self::RenameWindow),
            0x15 => Some(Self::DestroyWindow),
            // 0x16 / 0x17: reserved, see the `MessageType` enum comment.
            0x18 => Some(Self::Shutdown),
            0x19 => Some(Self::RequestPaneSnapshot),
            0x1A => Some(Self::MoveWindow),
            0x1B => Some(Self::SetVisibility),
            0x1C => Some(Self::Notify),
            0x1D => Some(Self::AgentStatusUpdate),
            0x1E => Some(Self::ReadPane),
            0x1F => Some(Self::ReadPaneResult),
            0x20 => Some(Self::SendText),
            0x21 => Some(Self::SendTextResult),
            0x22 => Some(Self::WaitAgentState),
            0x23 => Some(Self::WaitAgentStateResult),
            0x24 => Some(Self::AgentApiError),
            0x25 => Some(Self::Upgrade),
            0x26 => Some(Self::Upgrading),
            _ => None,
        }
    }
}

/// Client type for handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientType {
    /// GUI client: full data path (PTY I/O + control)
    Gui,
    /// CLI client: control only (session list, kill, detach notification)
    Cli,
}

/// Handshake request from client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMsg {
    pub client_type: ClientType,
    pub protocol_version: u32,
}

/// Per-window metadata for IPC messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u32,
    pub name: String,
    pub active_pane_id: u32,
}

/// Session info returned in Welcome message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
    pub window_count: u32,
    pub pane_count: u32,
    /// Index of the active window (0-based) within the ordered window list.
    #[serde(default)]
    pub active_window_index: u32,
    /// Per-window details for target resolution.
    #[serde(default)]
    pub windows: Vec<WindowInfo>,
}

/// Handshake response from daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WelcomeMsg {
    Accepted {
        server_version: u32,
        sessions: Vec<SessionInfo>,
    },
    Rejected {
        reason: String,
    },
}

/// Best-effort parse of the daemon's reported protocol version out of a
/// `WelcomeMsg::Rejected` reason string produced by the version-mismatch
/// path in `mux/ipc/connection.rs`
/// (`"Protocol version mismatch: client={client}, server={server}"`).
///
/// This is deliberately NOT part of the `WelcomeMsg` wire shape: an older
/// daemon's `Rejected { reason }` bincode payload must decode against the
/// CURRENT `WelcomeMsg` definition unchanged (bincode has no
/// forward/backward field tolerance), so the recovery path
/// (task0010 rework) reads the server version out of the free-form reason
/// text instead of adding a structured field. Returns `None` for any other
/// reason text (a rejection for a different cause, or a future daemon that
/// changes this wording) — callers must treat that as "version unknown",
/// never panic, and fall back to a generic message (AC-3).
pub fn parse_rejected_server_version(reason: &str) -> Option<u32> {
    let after_marker = reason.rsplit_once("server=")?.1;
    let digits: String = after_marker
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

/// Resize request for a pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeMsg {
    pub cols: u16,
    pub rows: u16,
}

/// PTY process exit notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyExitedMsg {
    pub exit_code: Option<u32>,
}

/// Error notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMsg {
    pub message: String,
}

/// Attach request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachMsg {
    pub session_id: u32,
}

/// Rename window request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameWindowMsg {
    pub name: String,
}

/// Desktop-notification request pushed from daemon to GUI.
///
/// Carries the OSC 9 message body recognized on a Detached pane. The GUI
/// client fires the OS notification (permission-gated). Decoded on the
/// frontend as a bincode `String` (u64 LE length + UTF-8 bytes), matching
/// the existing `RenameWindowMsg` wire shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyMsg {
    pub message: String,
}

/// SetVisibility payload (1 byte: 0x01 = visible, 0x00 = hidden).
///
/// Carried as a raw 1-byte payload (NOT bincode) so the wire shape matches
/// the frontend `MuxClient.sendSetVisibility` encoding without requiring
/// any deserializer round-trip on the daemon side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetVisibilityPayload {
    pub visible: bool,
}

impl SetVisibilityPayload {
    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        let byte = *payload.first()?;
        Some(Self { visible: byte != 0 })
    }

    pub fn to_payload(self) -> Vec<u8> {
        vec![if self.visible { 0x01 } else { 0x00 }]
    }
}

/// Move window request (reorder within a session).
///
/// `target_index` is the 0-based position in `MuxSession::window_order`
/// the window should occupy after the move. The daemon clamps out-of-range
/// values to the valid range `[0, window_order.len() - 1]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveWindowMsg {
    pub target_index: u32,
}

/// Payload for CreateWindow message.
/// Carries optional window name and initial command.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateWindowPayload {
    pub name: Option<String>,
    pub command: Option<String>,
}

/// A complete IPC message with header and payload.
#[derive(Debug, Clone)]
pub struct MuxMessage {
    pub msg_type: MessageType,
    pub pane_id: u32,
    pub payload: Vec<u8>,
}

impl MuxMessage {
    /// Create a message with raw bytes payload (for PTY data).
    pub fn pty_output(pane_id: u32, data: Vec<u8>) -> Self {
        Self {
            msg_type: MessageType::PtyOutput,
            pane_id,
            payload: data,
        }
    }

    /// Create a message with raw bytes payload (for keyboard input).
    pub fn pty_input(pane_id: u32, data: Vec<u8>) -> Self {
        Self {
            msg_type: MessageType::PtyInput,
            pane_id,
            payload: data,
        }
    }

    /// Create a snapshot-reply message (`MessageType::Snapshot`).
    ///
    /// Used by the daemon connection drain when emitting a snapshot-kind
    /// chunk produced by `handle_request_pane_snapshot`. The payload is the
    /// raw snapshot bytes (clear+home prefix, scrollback, shadow screen);
    /// no `MessageType` change is introduced — `Snapshot = 0x0C` already
    /// exists and the client's `apply_mux_message::Snapshot|SnapshotRestore`
    /// arm dispatches it to `build_from_snapshot` + `scrollback_bypass`.
    pub fn snapshot(pane_id: u32, data: Vec<u8>) -> Self {
        Self {
            msg_type: MessageType::Snapshot,
            pane_id,
            payload: data,
        }
    }

    /// Create a control message with bincode-serialized payload.
    pub fn control<T: Serialize>(msg_type: MessageType, pane_id: u32, data: &T) -> Self {
        Self {
            msg_type,
            pane_id,
            payload: bincode::serialize(data).expect("control message serialization"),
        }
    }

    /// Serialize this message into a frame: [type: u8][pane_id: u32][payload]
    /// The length prefix is handled by the codec layer.
    pub fn to_frame_body(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(5 + self.payload.len());
        buf.push(self.msg_type as u8);
        buf.extend_from_slice(&self.pane_id.to_le_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Parse a frame body into a MuxMessage.
    pub fn from_frame_body(body: &[u8]) -> Option<Self> {
        if body.len() < 5 {
            return None;
        }
        let msg_type = MessageType::from_u8(body[0])?;
        let pane_id = u32::from_le_bytes([body[1], body[2], body[3], body[4]]);
        let payload = body[5..].to_vec();
        Some(Self {
            msg_type,
            pane_id,
            payload,
        })
    }

    /// Deserialize control message payload.
    pub fn decode_payload<T: for<'a> Deserialize<'a>>(&self) -> Option<T> {
        bincode::deserialize(&self.payload).ok()
    }

    /// Encode this message as an APC escape sequence string.
    ///
    /// Format: `ESC _ emterm-mux;<base64(frame_body)> ESC \`
    pub fn to_apc(&self) -> String {
        let body = self.to_frame_body();
        let encoded = BASE64.encode(&body);
        format!("{}{}{}{}", APC_START, APC_PREFIX, encoded, APC_ST)
    }

    /// Encode this message as an OSC 9999 escape sequence string.
    ///
    /// Format: `ESC ] 9999 ; emterm-mux;<base64(frame_body)> ESC \`
    /// Used as fallback transport when ConPTY strips APC sequences.
    pub fn to_osc(&self) -> String {
        let body = self.to_frame_body();
        let encoded = BASE64.encode(&body);
        format!(
            "{}{};{}{}{}",
            OSC_START, MUX_OSC_PARAM, APC_PREFIX, encoded, APC_ST
        )
    }

    /// Encode this message as a plaintext (escape-free) sequence string.
    ///
    /// Format: `EMUX;<base64(frame_body)>\r`
    ///
    /// The mux GUI→bridge direction on Windows passes through ConPTY's input
    /// processing, which silently strips APC and OSC escape sequences. This
    /// printable-ASCII envelope survives intact. The bridge's
    /// `StdinApcParser` recognizes it alongside APC and OSC 9999, so the
    /// daemon protocol payload is unchanged — only the on-wire framing
    /// differs from `to_apc` / `to_osc`.
    ///
    /// The terminator is CR (`\r`), not LF. portable-pty 0.8 opens ConPTY
    /// with `PSEUDOCONSOLE_WIN32_INPUT_MODE` (see
    /// `pty::input::encode_backspace_win32` for the parallel case), and
    /// raw LF written to that channel is not delivered as a real key event
    /// — the message would otherwise stall at the bridge with the prefix
    /// matched but no terminator ever arriving. CR rides through as
    /// `VK_RETURN` reliably, and the bridge parser accepts CR / LF / CRLF
    /// interchangeably.
    pub fn to_plaintext(&self) -> String {
        let body = self.to_frame_body();
        let encoded = BASE64.encode(&body);
        // PLAINTEXT_PREFIX is `b"EMUX;"`, guaranteed ASCII; reuse the SSOT
        // instead of re-stating the literal so a future prefix change
        // propagates to both encoder and parser.
        let prefix = std::str::from_utf8(PLAINTEXT_PREFIX).expect("PLAINTEXT_PREFIX is ASCII");
        format!("{}{}\r", prefix, encoded)
    }

    /// Decode an APC payload string into a MuxMessage.
    ///
    /// The `payload` parameter is the content between `ESC _` and `ESC \`,
    /// which must start with the `emterm-mux;` prefix.
    pub fn from_apc(payload: &str) -> Result<Self, ApcDecodeError> {
        let b64 = payload
            .strip_prefix(APC_PREFIX)
            .ok_or(ApcDecodeError::MissingPrefix)?;
        let bytes = BASE64
            .decode(b64)
            .map_err(|_| ApcDecodeError::InvalidBase64)?;
        Self::from_frame_body(&bytes).ok_or(ApcDecodeError::InvalidFrameBody)
    }
}

/// Errors that can occur when decoding an APC payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApcDecodeError {
    /// Payload does not start with `emterm-mux;`.
    MissingPrefix,
    /// Base64 decoding failed.
    InvalidBase64,
    /// Frame body is invalid (too short or unknown message type).
    InvalidFrameBody,
}

impl std::fmt::Display for ApcDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPrefix => write!(f, "missing emterm-mux; prefix"),
            Self::InvalidBase64 => write!(f, "invalid base64 encoding"),
            Self::InvalidFrameBody => write!(f, "invalid frame body"),
        }
    }
}

impl std::error::Error for ApcDecodeError {}

/// Local mirror of the core agent-status module's state enum
/// (`src-tauri/src/agent_status.rs`). `mux_ipc` must not depend on the
/// binary crate, so this type owns its own serde representation; the
/// lowercase string values (`idle`/`working`/`blocked`/`done`) are the
/// wire contract shared between the two modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Idle,
    Working,
    Blocked,
    Done,
}

/// Daemon → GUI unsolicited push: a mux pane's agent status changed, or is
/// being restated after a snapshot/reattach (`replay_derived: true`, in
/// which case the receiver must apply it silently — no transition event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatusUpdateMsg {
    pub pane_id: u32,
    pub public_pane_id: String,
    pub state: Option<AgentState>,
    pub name: Option<String>,
    pub revision: u64,
    pub replay_derived: bool,
}

/// Request: read the last `lines` lines of a mux pane's visible content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadPaneMsg {
    pub public_pane_id: String,
    pub lines: u32,
}

/// Response to `ReadPane`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadPaneResultMsg {
    pub text: String,
}

/// Request: send text (raw bytes) to a mux pane's PTY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextMsg {
    pub public_pane_id: String,
    pub bytes: Vec<u8>,
}

/// Response to `SendText`: the pane's revision observed immediately before
/// the successful PTY write (the "watermark").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTextResultMsg {
    pub revision_watermark: u64,
}

/// Request: block until a mux pane's agent state is a member of `states`
/// (and, when `after_revision` is given, the pane's revision exceeds it),
/// or until `timeout_ms` elapses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitAgentStateMsg {
    pub public_pane_id: String,
    pub states: Vec<AgentState>,
    pub timeout_ms: u64,
    pub after_revision: Option<u64>,
}

/// Response to `WaitAgentState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitAgentStateResultMsg {
    pub state: AgentState,
    pub revision: u64,
}

/// Error kind for agent-API request failures (`ReadPane` / `SendText` /
/// `WaitAgentState`). The `emterm mux read/send/wait` CLI exit codes map
/// onto these kinds (see IMPLEMENTATION.md "Conventions"): `invalid_input`
/// → 2, `timeout` → 3, `unknown_pane`/`pane_gone` → 4, `not_mux_pane` → 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentApiErrorKind {
    UnknownPane,
    NotMuxPane,
    Timeout,
    PaneGone,
    InvalidInput,
}

/// Structured error response shared by `ReadPane` / `SendText` /
/// `WaitAgentState`, carried as the payload of `MessageType::AgentApiError`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentApiError {
    pub kind: AgentApiErrorKind,
    pub message: String,
}

/// A parsed public-facing pane ID: opaque string form
/// `"{incarnation}-{pane_id}"`, where `incarnation` is a lowercase-hex
/// token minted once at daemon start (never reused across restarts) and
/// `pane_id` is the existing wire `u32`. The daemon is the only minter;
/// clients treat the composed string as opaque and only need
/// [`PublicPaneId::compose`] / [`PublicPaneId::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicPaneId {
    pub incarnation: String,
    pub pane_id: u32,
}

impl PublicPaneId {
    /// Compose the opaque string form from an incarnation token and the
    /// wire pane ID.
    pub fn compose(incarnation: &str, pane_id: u32) -> String {
        format!("{incarnation}-{pane_id}")
    }

    /// Parse a public-facing pane ID string back into its incarnation
    /// token and wire pane ID.
    ///
    /// Never panics: malformed input (empty string, no `-` separator, a
    /// non-lowercase-hex incarnation token, or a pane number that does not
    /// fit in `u32`) yields [`PublicPaneIdError`].
    pub fn parse(id: &str) -> Result<Self, PublicPaneIdError> {
        let (incarnation, pane_id_str) = id
            .rsplit_once('-')
            .ok_or(PublicPaneIdError::MissingSeparator)?;
        if incarnation.is_empty() || !incarnation.chars().all(is_lowercase_hex_digit) {
            return Err(PublicPaneIdError::InvalidIncarnation);
        }
        let pane_id = pane_id_str
            .parse::<u32>()
            .map_err(|_| PublicPaneIdError::InvalidPaneNumber)?;
        Ok(Self {
            incarnation: incarnation.to_string(),
            pane_id,
        })
    }
}

fn is_lowercase_hex_digit(c: char) -> bool {
    c.is_ascii_digit() || ('a'..='f').contains(&c)
}

/// Errors that can occur when parsing a [`PublicPaneId`] string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicPaneIdError {
    /// No `-` separator between incarnation and pane number (also covers
    /// the empty-string input).
    MissingSeparator,
    /// The incarnation token is empty or contains non-lowercase-hex
    /// characters.
    InvalidIncarnation,
    /// The pane-number segment does not parse as a `u32` (non-digits or
    /// overflow).
    InvalidPaneNumber,
}

impl std::fmt::Display for PublicPaneIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSeparator => {
                write!(
                    f,
                    "missing '-' separator between incarnation and pane number"
                )
            }
            Self::InvalidIncarnation => write!(f, "incarnation token is not lowercase hex"),
            Self::InvalidPaneNumber => write!(f, "pane number is not a valid u32"),
        }
    }
}

impl std::error::Error for PublicPaneIdError {}

#[cfg(test)]
mod tests;
