//! PTY spawning and reader loop for mux panes.

use std::borrow::Cow;
use std::io::Read;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::scrollback_filter::{AgentStatusOscScanner, strip_pty_output_for_scrollback_write};
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    AgentStatusReportSender, DetachReason, MuxPane, NotificationSender, PaneId, PaneOutputTarget,
    PtyOutputChunk, SharedAgentStatusReportSender, SharedNotificationSender, SharedOutputTarget,
    SharedPaneExitSender, SharedScrollback, SharedShadowParser, SharedTitleSender,
    TitleChangeSender, lock_shadow_parser,
};
use crate::pty::passthrough_scanner::PassthroughScanner;
use crate::pty::visibility::RawPassthroughBuffer;

/// Shared per-pane raw passthrough buffer (image / Markdown OSC bytes
/// captured while detached or hidden). Drained into the resume snapshot.
type SharedRawPassthrough = Arc<StdMutex<RawPassthroughBuffer>>;

/// Shared per-pane stateful passthrough scanner. Lives outside the buffer
/// so partial sequences spanning chunk boundaries are recovered.
type SharedPassthroughScanner = Arc<StdMutex<PassthroughScanner>>;

/// Detect the default shell for the current platform.
fn detect_default_shell() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }

    #[cfg(windows)]
    {
        "powershell.exe".to_string()
    }
}

/// Result of spawning a PTY with shell process.
pub(super) struct SpawnedPty {
    pub(super) master: Box<dyn MasterPty + Send>,
    pub(super) writer: Box<dyn std::io::Write + Send>,
    pub(super) reader: Box<dyn std::io::Read + Send>,
}

/// Build the fixed environment variables set on every newly spawned mux
/// pane's shell process. Extracted as a pure function (separate from the
/// real `portable_pty` spawn call) so `EMTERM_PANE_ID` injection is
/// testable without spawning a real PTY (AC-6, IMPLEMENTATION.md FR13:
/// mux pane spawn injects `EMTERM_PANE_ID` into the pane's environment,
/// resolved by `emterm mux read|send|wait --pane current`).
///
/// `public_pane_id` is minted by the caller via
/// `SessionManager::public_pane_id` (SPEC FR13) before `spawn_pty` runs —
/// this module has no direct dependency on `SessionManager`'s incarnation
/// state.
fn pane_env_vars(public_pane_id: &str) -> Vec<(&'static str, String)> {
    vec![
        ("TERM", "xterm-256color".to_string()),
        ("COLORTERM", "truecolor".to_string()),
        ("TERM_PROGRAM", "emterm".to_string()),
        ("EMTERM_MUX", "1".to_string()),
        ("EMTERM_PANE_ID", public_pane_id.to_string()),
    ]
}

/// Spawn a PTY with a shell process at the given size.
///
/// `public_pane_id` is the pane's public (opaque, daemon-incarnation-
/// scoped) ID, minted by the caller (`SessionManager::public_pane_id`)
/// BEFORE spawn — environment variables must be set before the shell
/// process starts, so the ID cannot be injected after the fact. Resolved
/// client-side by `emterm mux read|send|wait --pane current` via
/// `EMTERM_PANE_ID` (IMPLEMENTATION.md FR13 / "Public pane ID format").
pub(super) fn spawn_pty(cols: u16, rows: u16, public_pane_id: &str) -> Result<SpawnedPty, String> {
    let pty_system = portable_pty::native_pty_system();
    let pty_size = portable_pty::PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };

    let pair = pty_system
        .openpty(pty_size)
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    let shell = detect_default_shell();
    let mut cmd = portable_pty::CommandBuilder::new(&shell);
    for (key, value) in pane_env_vars(public_pane_id) {
        cmd.env(key, value);
    }
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");

    #[cfg(unix)]
    if let Ok(home) = std::env::var("HOME") {
        cmd.cwd(&home);
    }
    #[cfg(windows)]
    if let Ok(home) = std::env::var("USERPROFILE") {
        cmd.cwd(&home);
    }

    pair.slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn shell: {}", e))?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("Failed to take PTY writer: {}", e))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;

    Ok(SpawnedPty {
        master: pair.master,
        writer,
        reader,
    })
}

/// Register a new pane in the session manager and start its reader thread.
///
/// `pane_id` is pre-allocated by the caller (`SessionManager::alloc_pane_id`)
/// BEFORE `spawn_pty` runs, so `EMTERM_PANE_ID` can be injected into the
/// shell's environment at spawn time. Returns the pane_id and its output
/// target (for the reader thread) — `None` only when `session_id` /
/// `window_id` no longer resolve (a race with concurrent teardown).
#[allow(clippy::too_many_arguments)]
pub(super) fn register_pane_and_start_reader(
    mgr: &mut SessionManager,
    session_id: u32,
    window_id: u32,
    pane_id: PaneId,
    cols: u16,
    rows: u16,
    spawned: SpawnedPty,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    agent_status_tx: &AgentStatusReportSender,
    pane_exit_sender: &SharedPaneExitSender,
) -> Option<PaneId> {
    // Verify session/window still exist (pane_id was already allocated by
    // the caller before spawn_pty, so there is nothing to allocate here).
    {
        let session = mgr.get_session(session_id)?;
        session.windows.get(&window_id)?;
    }

    let session = mgr.get_session_mut(session_id)?;
    let window = session.windows.get_mut(&window_id)?;

    let output_target: SharedOutputTarget = Arc::new(std::sync::Mutex::new(
        PaneOutputTarget::Connected(pane_output_tx.clone()),
    ));
    let pane = MuxPane::new(
        pane_id,
        cols,
        rows,
        output_target.clone(),
        spawned.writer,
        spawned.master,
    );
    let shadow_parser = pane.shadow_parser.clone();
    let pane_cwd = pane.cwd.clone();
    let pane_title = pane.title.clone();
    let title_sender = pane.title_sender.clone();
    let notification_sender = pane.notification_sender.clone();
    let agent_status_report_sender = pane.agent_status_report_sender.clone();
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    let pane_dims = pane.dims.clone();
    // Store initial title_tx in the swappable sender (reattach will swap in a new one)
    *title_sender.lock().unwrap() = Some(title_tx.clone());
    // The notification channel lives for the daemon lifetime; populate it once.
    *notification_sender.lock().unwrap() = Some(notification_tx.clone());
    // The agent-status channel lives for the daemon lifetime; populate it
    // once (SPEC FR3 — never swapped, mirrors notification_sender).
    *agent_status_report_sender.lock().unwrap() = Some(agent_status_tx.clone());
    window.add_pane(pane);

    // The pane-exit sender is fixed at pane creation and never swapped on
    // attach/detach (M1): clone the shared Arc straight into the reader thread.
    let pane_exit_sender = pane_exit_sender.clone();

    let reader = spawned.reader;
    std::thread::spawn(move || {
        pty_reader_loop(
            pane_id,
            reader,
            output_target,
            shadow_parser,
            pane_cwd,
            pane_title,
            title_sender,
            notification_sender,
            agent_status_report_sender,
            raw_passthrough,
            passthrough_scanner,
            scrollback,
            pane_dims,
            pane_exit_sender,
        );
    });

    Some(pane_id)
}

/// Read PTY output in a blocking loop and forward to the output target.
/// Runs in a dedicated std::thread since PTY reads are blocking I/O.
///
/// When the connected channel fails (GUI disconnected), the reader automatically
/// switches to buffering mode using the per-pane scrollback buffer. The reader
/// thread stays alive so the PTY process output is never lost.
///
/// Phase B: bytes are written into `scrollback` only on the detached arms
/// (matching the previous per-detach-cycle ring buffer behavior). Phase C
/// will move the write above the `output_target` match so attach-time bytes
/// are also retained.
/// Extract the main-buffer byte spans of a raw PTY chunk for the scrollback
/// ring, given `alt_at_start` (the shadow parser's alt-screen state *before*
/// this chunk). The alternate screen has no scrollback, so its output — and
/// the buffer-switch toggles themselves (`?1049` / `?1047` / `?47` `h`/`l`) —
/// are dropped; only main-buffer bytes survive. A chunk that crosses a buffer
/// switch keeps the main-buffer side rather than being discarded wholesale
/// (which would lose e.g. command output emitted just before a TUI opens in
/// the same read).
///
/// Returns `(bytes, final_alt)`. `final_alt` is the alt state the scan ended
/// in; the caller cross-checks it against the authoritative post-chunk parser
/// state to detect a toggle that straddled the read boundary (or an
/// unrecognized form) and fall back conservatively. `Cow::Borrowed` is
/// returned for the common no-toggle chunk (whole chunk on main, empty on
/// alt) so the hot path avoids a copy.
/// Cap on the per-pane pending buffer inside [`ScrollbackWriteFilter`]. When
/// the pending run of bytes we could not yet strip grows past this many bytes,
/// the filter gives up the strip guarantee for that flush and forwards the
/// pending bytes verbatim to the ring. Sized comfortably above one
/// `emterm markdown|json|yaml` chunk (128 KiB payload, ~172 KiB after base64
/// framing) so the common case never trips it.
const SCROLLBACK_FILTER_PENDING_CAP: usize = 512 * 1024;

/// Stateful stream filter that strips viewer-launch rich content (OSC 777
/// emterm-{markdown,image,json,yaml} / Kitty APC / SIXEL DCS / OSC 9999
/// emterm-md) — AND a `resize` kind OSC 777 body (review round-1 rework,
/// finding `0c18ff55032328ab`: a forged in-band resize marker must never
/// reach the ring from PTY output) — BEFORE bytes land in the scrollback
/// ring.
///
/// **Why stateful:** PTY reads are chunked at 64 KiB, but an `emterm markdown`
/// / `image` / `json` / `yaml` CLI emits a single OSC 777 chunk of up to
/// 128 KiB payload (~172 KiB after base64 framing). One CLI chunk therefore
/// spans multiple `read()` calls, so a stateless per-chunk stripper sees
/// either (introducer, no terminator) or (terminator, no introducer) and —
/// by design — passes both fragments through verbatim. The fragments then
/// land in the 2 MiB ring, and a later overflow can evict the introducer
/// while its base64 tail survives; the snapshot-time stripper (which only
/// matches complete sequences) then replays that headerless tail into the
/// client's grid on tab-switch reattach.
///
/// This filter closes that gap by holding an unterminated introducer's bytes
/// in `pending` until a subsequent [`Self::feed`] carries the terminator, at
/// which point the fully-formed sequence is stripped in one shot.
/// `pending` is capped at [`SCROLLBACK_FILTER_PENDING_CAP`]; on overflow the
/// pending bytes are forwarded raw (the escape hatch — the ring may then
/// contain a partial sequence, but that is strictly better than an
/// unbounded per-pane buffer).
///
/// Live-forwarded `data.to_vec()` to the connected client is intentionally
/// untouched, so viewer launch on the client side is unaffected. The
/// snapshot-time stripper in `scrollback_filter.rs` remains as a
/// defense-in-depth guard for scrollback captured by an older daemon that
/// predates this filter.
pub(in crate::mux) struct ScrollbackWriteFilter {
    pending: Vec<u8>,
}

impl ScrollbackWriteFilter {
    pub(in crate::mux) fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// Feed one PTY read chunk. Returns the bytes safe to write to the
    /// scrollback ring right now. Any trailing bytes belonging to an
    /// unterminated strip-target introducer are held in `pending` until the
    /// next feed.
    ///
    /// Overflow escape hatch: if `pending` (after appending `chunk`) exceeds
    /// [`SCROLLBACK_FILTER_PENDING_CAP`], the entire pending run is flushed
    /// early WITHOUT waiting for a safe boundary. This trades "flush at a
    /// structurally clean boundary" for a bounded per-pane memory footprint
    /// — a wedged / adversarial stream cannot pin arbitrary bytes in the
    /// buffer.
    ///
    /// task0003 D1 (review round-2 finding `a6ab9b340119beed`, critical):
    /// the flushed bytes still go through
    /// [`strip_pty_output_for_scrollback_write`] — they are NOT forwarded
    /// raw. Before this fix, the overflow path returned `pending` verbatim,
    /// so a child process could force this branch (an unterminated OSC/DCS/
    /// APC introducer padded past the cap) and have a forged resize marker
    /// anywhere in that padding reach the scrollback ring completely
    /// unfiltered — the cap bounds MEMORY, not the strip guarantee; a
    /// complete marker is removed in a single linear pass regardless of how
    /// this batch was flushed.
    pub(in crate::mux) fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        if chunk.is_empty() {
            return Vec::new();
        }
        self.pending.extend_from_slice(chunk);

        if self.pending.len() > SCROLLBACK_FILTER_PENDING_CAP {
            log::warn!(
                "scrollback write filter: pending exceeded {} bytes, flushing early",
                SCROLLBACK_FILTER_PENDING_CAP
            );
            let pending = std::mem::take(&mut self.pending);
            return strip_pty_output_for_scrollback_write(&pending);
        }

        let boundary = find_safe_boundary(&self.pending);
        if boundary == 0 {
            return Vec::new();
        }
        let strippable: Vec<u8> = self.pending.drain(..boundary).collect();
        strip_pty_output_for_scrollback_write(&strippable)
    }

    /// Number of bytes currently held in `pending` (test / diagnostic).
    #[cfg(test)]
    pub(in crate::mux) fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

/// Find the position of the first unterminated strip-target introducer in
/// `bytes`. If every strip-target sequence in `bytes` is closed (or there
/// are none), returns `bytes.len()` — everything is safe to emit.
///
/// Strip-target introducers we look for (matches
/// [`strip_replayable_rich_content`]):
/// - `ESC _ G` — Kitty APC. Terminator: `ESC \`.
/// - `ESC P` — any DCS. Terminator: `ESC \`. (Non-SIXEL DCS bodies still ride
///   here so an unterminated DCS is not accidentally split — the strip
///   function will decide whether to drop it once complete.)
/// - `ESC ]` — any OSC. Terminator: BEL (`0x07`) or `ESC \`.
///
/// Any other byte after `ESC` (`[` = CSI, standalone escape, etc.) is not a
/// strip target and does not force a boundary.
fn find_safe_boundary(bytes: &[u8]) -> usize {
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if bytes[i] != 0x1b || i + 1 >= n {
            i += 1;
            continue;
        }
        let intro_start = i;
        match bytes[i + 1] {
            b'_' => {
                // APC. Only Kitty (ESC _ G) is a strip target — but even a
                // non-Kitty APC is still an APC and needs an ESC \ terminator
                // before its body is safe to emit; without one, we cannot tell
                // where its body ends. Same tail-buffer rule either way.
                match find_st(bytes, i + 2) {
                    Some(end) => i = end,
                    None => return intro_start,
                }
            }
            b'P' => match find_st(bytes, i + 2) {
                Some(end) => i = end,
                None => return intro_start,
            },
            b']' => match find_osc_end(bytes, i + 2) {
                Some(end) => i = end,
                None => return intro_start,
            },
            _ => {
                i += 2;
            }
        }
    }
    n
}

/// Find the index just past an ST (`ESC \`) terminator starting at or after
/// `from`. Returns the byte index immediately AFTER the trailing `\\`, or
/// `None` if no ST is present. Mirrors the terminator scan in
/// [`crate::mux::scrollback_filter`] so the boundary detector and the
/// stripper agree on what "complete" means.
fn find_st(bytes: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j + 1 < bytes.len() {
        if bytes[j] == 0x1b && bytes[j + 1] == b'\\' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

/// Find the index just past an OSC terminator (BEL `0x07` or ST `ESC \`)
/// starting at `from`. Returns `None` if the OSC is unterminated. A bare
/// `ESC` that is not the start of ST aborts the scan and returns `None`
/// (mirrors `scrollback_filter::find_osc_terminator`).
fn find_osc_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == 0x07 {
            return Some(j + 1);
        }
        if bytes[j] == 0x1b {
            if j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                return Some(j + 2);
            }
            return None;
        }
        j += 1;
    }
    None
}

fn extract_main_buffer_bytes(data: &[u8], alt_at_start: bool) -> (Cow<'_, [u8]>, bool) {
    // (pattern, is_enter). `h` enters the alternate screen, `l` returns to main.
    const TOGGLES: [(&[u8], bool); 6] = [
        (b"\x1b[?1049h", true),
        (b"\x1b[?1049l", false),
        (b"\x1b[?1047h", true),
        (b"\x1b[?1047l", false),
        (b"\x1b[?47h", true),
        (b"\x1b[?47l", false),
    ];
    let matches_toggle = |d: &[u8]| TOGGLES.iter().find(|(p, _)| d.starts_with(p)).copied();

    // Fast scan for any toggle. Most chunks (plain output, even SGR-colored)
    // contain none, so we can borrow without building a filtered copy.
    let mut has_toggle = false;
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b && matches_toggle(&data[i..]).is_some() {
            has_toggle = true;
            break;
        }
        i += 1;
    }
    if !has_toggle {
        return if alt_at_start {
            (Cow::Borrowed(&[]), true)
        } else {
            (Cow::Borrowed(data), false)
        };
    }

    // Slow path: split into main-buffer spans, dropping toggles and alt spans.
    let mut out = Vec::with_capacity(data.len());
    let mut alt = alt_at_start;
    let mut span_start: Option<usize> = if alt { None } else { Some(0) };
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b {
            if let Some((pat, is_enter)) = matches_toggle(&data[i..]) {
                if let Some(s) = span_start.take() {
                    out.extend_from_slice(&data[s..i]);
                }
                alt = is_enter;
                i += pat.len();
                if !alt {
                    span_start = Some(i);
                }
                continue;
            }
        }
        i += 1;
    }
    if let Some(s) = span_start {
        out.extend_from_slice(&data[s..]);
    }
    (Cow::Owned(out), alt)
}

#[allow(clippy::too_many_arguments)]
fn pty_reader_loop(
    pane_id: u32,
    mut reader: Box<dyn Read + Send>,
    output_target: SharedOutputTarget,
    shadow_parser: SharedShadowParser,
    pane_cwd: Arc<std::sync::Mutex<Option<String>>>,
    last_title: Arc<std::sync::Mutex<Option<String>>>,
    title_sender: SharedTitleSender,
    notification_sender: SharedNotificationSender,
    agent_status_report_sender: SharedAgentStatusReportSender,
    raw_passthrough: SharedRawPassthrough,
    passthrough_scanner: SharedPassthroughScanner,
    scrollback: SharedScrollback,
    pane_dims: crate::mux::session::pane::SharedPaneDims,
    pane_exit_sender: SharedPaneExitSender,
) {
    let mut buf = [0u8; 65536];
    // Per-pane stateful scrollback-write filter: strips viewer-launch rich
    // content across PTY read boundaries. See [`ScrollbackWriteFilter`] for
    // why a stateless per-chunk stripper is insufficient (128 KiB CLI chunks
    // straddle the 64 KiB PTY read buffer).
    let mut scrollback_filter = ScrollbackWriteFilter::new();
    // Per-pane stateful agent-status OSC decoder: retains a partial
    // `agent-status` OSC 777 sequence across PTY read boundaries so a
    // report split across reads is still detected exactly once (SPEC
    // FR1/FR3; review round-1 rework, stable_id `osc_split_lost`).
    let mut agent_status_scanner = AgentStatusOscScanner::new();
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                let target_state = {
                    let t = output_target.lock().unwrap();
                    match &*t {
                        PaneOutputTarget::Connected(_) => "Connected",
                        PaneOutputTarget::Detached { .. } => "Detached",
                    }
                };
                log::info!(
                    "PTY reader EOF for pane {} (output_target={})",
                    pane_id,
                    target_state
                );
                // FR3: signal exit to the connected client (if any) so the GUI
                // tears the pane/tab down. Scope the lock so it is released
                // before the pane-exit enqueue below.
                {
                    let target = output_target.lock().unwrap();
                    if let PaneOutputTarget::Connected(ref tx) = *target {
                        let _ = tx.blocking_send(PtyOutputChunk::pty_output(pane_id, Vec::new()));
                    }
                }
                // FR1: notify the daemon of the pane exit regardless of attach
                // state so a detached pane is reaped authoritatively (the
                // Connected empty-chunk path above only reaches an attached
                // client). The sender is fixed at pane creation and never
                // swapped (M1), so this works even while detached.
                //
                // M2: a non-blocking `try_send` keeps the exiting reader thread
                // from blocking. A `None` sender (CLI / test path) or a dropped
                // receiver (daemon already shutting down) is ignored.
                if let Some(tx) = pane_exit_sender.lock().unwrap().as_ref() {
                    if let Err(e) = tx.try_send(pane_id) {
                        log::debug!("pane {} exit notification not delivered: {}", pane_id, e);
                    }
                }
                break;
            }
            Ok(n) => {
                let data = &buf[..n];
                // task0003 D5 (finding `0bebe3e6f7b416dd`): snapshot the
                // pane's dims as early as possible after `read()` returns —
                // before any of this chunk's own processing — so the
                // scrollback write below attributes it to what was actually
                // in effect when the data was produced, not to whatever
                // `MuxPane::resize` happens to have already published by the
                // time we get around to writing (see
                // `ScrollbackRingBuffer::attribute_write`).
                let (read_cols, read_rows) = pane_dims.get();

                // Feed the shadow parser (OSC title + alt-screen state) FIRST,
                // in a single lock scope, so the scrollback write below can be
                // gated on the alt-screen state.
                let (title_changed, alt_before, alt_after) = {
                    let mut parser = lock_shadow_parser(&shadow_parser);
                    // alt-screen state BEFORE this chunk; paired with the
                    // post-process state it identifies pure main-buffer chunks.
                    let alt_before = parser.screen().alternate_screen();
                    // vt100 has internal panics (wide-character bookkeeping
                    // can `unwrap` a `None`). Catch the unwind here so the
                    // panic neither kills the reader thread nor poisons the
                    // mutex; rebuild the parser so subsequent output
                    // re-populates the shadow screen.
                    let (rows, cols) = parser.screen().size();
                    let processed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        parser.process(data);
                    }));
                    if processed.is_err() {
                        *parser = crate::mux::session::pane::new_shadow_parser(rows, cols);
                        log::error!(
                            "pane {}: shadow parser panicked while processing {} bytes; parser reset",
                            pane_id,
                            data.len()
                        );
                    }
                    let alt_after = parser.screen().alternate_screen();
                    // vt100 0.16 reports OSC 0/2 titles via the Callbacks
                    // API; the TitleSink records the latest one per chunk.
                    let title_changed = match parser.callbacks_mut().take_title() {
                        Some(new_title) if !new_title.is_empty() => {
                            let mut current = last_title.lock().unwrap();
                            if Some(new_title.as_str()) != current.as_deref() {
                                *current = Some(new_title.clone());
                                Some(new_title)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    (title_changed, alt_before, alt_after)
                };

                // Phase C: scrollback write, restricted to the chunk's
                // MAIN-buffer byte spans. Like a real terminal the alternate
                // screen has NO scrollback, so its output and the
                // buffer-switch toggles themselves are dropped — keeping any
                // unpaired `?1049h` out of the scrollback so the on-demand
                // snapshot (which replays scrollback into the client) can't
                // strand the client in alt-screen. Unlike a whole-chunk gate,
                // this preserves main-buffer output that shares a read with a
                // buffer switch (e.g. command output emitted right before a
                // TUI opens). Capture still happens regardless of attach state
                // so a later reattach can replay pre-detach history.
                let (main_bytes, scan_alt) = extract_main_buffer_bytes(data, alt_before);
                let to_write: &[u8] = if scan_alt == alt_after {
                    &main_bytes
                } else {
                    // The scan ended in a different buffer than the
                    // authoritative shadow parser: a toggle straddled this
                    // read boundary or used an unrecognized form. Fall back to
                    // the conservative whole-chunk gate so we never emit a
                    // partial toggle sequence into scrollback.
                    if !alt_before && !alt_after { data } else { &[] }
                };
                if !to_write.is_empty() {
                    let filtered = scrollback_filter.feed(to_write);
                    if !filtered.is_empty() {
                        scrollback
                            .lock()
                            .unwrap()
                            .attribute_write(read_cols, read_rows, &filtered);
                    }
                }
                if let Some(new_title) = title_changed {
                    if let Some(tx) = title_sender.lock().unwrap().as_ref() {
                        let _ = tx.try_send((pane_id, new_title));
                    }
                }

                // Detect agent-status OSC 777 reports (SPEC FR3) and forward
                // each to the daemon-level agent-status task. Unlike OSC 9
                // notification scanning (Detached-only, to avoid double-
                // firing with the GUI's own live parse), this runs
                // regardless of attach state: the daemon owns per-pane
                // agent-status state unconditionally, and the GUI never
                // parses this OSC itself for mux panes. The scanner is
                // per-pane stateful (see `agent_status_scanner` above) so a
                // report split across this read and the next is still
                // detected.
                let reports = agent_status_scanner.feed(data);
                forward_agent_status_reports(pane_id, reports, &agent_status_report_sender);

                // Detect OSC 7 (cwd reporting) and cache the path
                if let Some(cwd) = crate::mux::ipc::statusbar::detect_osc7_cwd(data) {
                    *pane_cwd.lock().unwrap() = Some(cwd);
                }

                // OSC-probe (temporary): flag when the PTY reader saw an
                // `emterm` viewer-launch sequence in this chunk. Together with
                // the mirrored probes in bridge.rs / tabs.rs this pins down
                // where a viewer OSC 777 is lost between daemon → bridge → GUI.
                // Only metadata is logged (never the payload bytes) so this
                // probe cannot leak user file content into persisted release
                // logs.
                const OSC_PROBE_NEEDLE: &[u8] = b"\x1b]777;emterm;";
                if let Some(off) = data
                    .windows(OSC_PROBE_NEEDLE.len())
                    .position(|w| w == OSC_PROBE_NEEDLE)
                {
                    let target_state = match &*output_target.lock().unwrap() {
                        PaneOutputTarget::Connected(_) => "Connected",
                        PaneOutputTarget::Detached { .. } => "Detached",
                    };
                    log::warn!(
                        "[osc-probe daemon] pane={} data_len={} osc_off={} target={}",
                        pane_id,
                        data.len(),
                        off,
                        target_state,
                    );
                }

                // Lock briefly to try non-blocking send or clone the sender.
                // IMPORTANT: release lock before blocking_send to avoid deadlock
                // with session_manager lock held by collect_reattach_data.
                //
                // The Detached arms also feed `passthrough_scanner` so that
                // image / Markdown OSC byte runs survive a hidden / network
                // detach window and can be replayed via the resume snapshot.
                let send_result = {
                    let mut target = output_target.lock().unwrap();
                    match &mut *target {
                        PaneOutputTarget::Connected(tx) => {
                            // Single allocation: data owned by PtyOutputChunk
                            let chunk = PtyOutputChunk::pty_output(pane_id, data.to_vec());
                            match tx.try_send(chunk) {
                                Ok(()) => None, // sent successfully
                                Err(mpsc::error::TrySendError::Full(chunk)) => {
                                    // Channel full — need blocking send outside lock
                                    Some(Ok((tx.clone(), chunk)))
                                }
                                Err(mpsc::error::TrySendError::Closed(_)) => {
                                    // Channel closed — switch to detached.
                                    // Scrollback was already captured above
                                    // (Phase C, gated on main-buffer state);
                                    // only the passthrough scan needs to run
                                    // here.
                                    capture_passthrough(
                                        pane_id,
                                        data,
                                        &raw_passthrough,
                                        &passthrough_scanner,
                                        &notification_sender,
                                    );
                                    *target = PaneOutputTarget::Detached {
                                        reason: DetachReason::NetworkDetach,
                                        owner: None,
                                    };
                                    Some(Err(()))
                                }
                            }
                        }
                        PaneOutputTarget::Detached { .. } => {
                            // Scrollback already captured above (FR4);
                            // only passthrough bytes need separate capture.
                            capture_passthrough(
                                pane_id,
                                data,
                                &raw_passthrough,
                                &passthrough_scanner,
                                &notification_sender,
                            );
                            None
                        }
                    }
                }; // output_target lock released here

                // Handle backpressure outside the lock to avoid deadlock
                if let Some(Ok((tx, chunk))) = send_result {
                    log::debug!("Pane {} backpressure: channel full, blocking", pane_id);
                    if tx.blocking_send(chunk).is_err() {
                        log::info!("Pane {} switching to detached buffering mode", pane_id);
                        let mut target = output_target.lock().unwrap();
                        // Scrollback already captured above; only passthrough.
                        capture_passthrough(
                            pane_id,
                            data,
                            &raw_passthrough,
                            &passthrough_scanner,
                            &notification_sender,
                        );
                        *target = PaneOutputTarget::Detached {
                            reason: DetachReason::NetworkDetach,
                            owner: None,
                        };
                    }
                }
            }
            Err(e) => {
                log::info!(
                    "PTY reader error for pane {}: {} (kind={:?})",
                    pane_id,
                    e,
                    e.kind()
                );
                break;
            }
        }
    }
}

/// Run `data` through the per-pane passthrough scanner and append any
/// completed image / Markdown OSC sequences to the per-pane raw buffer. Any
/// recognized OSC 9 desktop-notification messages are forwarded through
/// `notification_sender` to the daemon (which relays them to the GUI client).
///
/// Called ONLY from the Detached arms of `pty_reader_loop`. On the Connected
/// arm the scanner is never run, so an active pane's OSC 9 is handled solely
/// by the GUI foreground WASM path — this is what prevents double-firing
/// (FR5 / NFR5 / TS-14). Notifications are side-effect events: they are NOT
/// added to `raw_passthrough`, so a reattach replay never re-fires them.
///
/// Logs a single warn when the buffer drops the oldest captured bytes due to
/// capacity overflow.
fn capture_passthrough(
    pane_id: PaneId,
    data: &[u8],
    raw_passthrough: &SharedRawPassthrough,
    passthrough_scanner: &SharedPassthroughScanner,
    notification_sender: &SharedNotificationSender,
) {
    let (extracted, notifications) = {
        let mut scanner = passthrough_scanner.lock().unwrap();
        let extracted = scanner.process(data);
        (extracted, scanner.take_notifications())
    };

    // Forward desktop notifications detected while detached (FR2). Kept out
    // of raw_passthrough so they never replay on reattach (FR5).
    if !notifications.is_empty() {
        if let Some(tx) = notification_sender.lock().unwrap().as_ref() {
            for message in notifications {
                if let Err(e) = tx.try_send((pane_id, message)) {
                    log::warn!(
                        "[WARN][BACKEND] mux pane {} notification channel send failed: {}",
                        pane_id,
                        e
                    );
                }
            }
        }
    }

    if extracted.is_empty() {
        return;
    }
    let dropped = raw_passthrough.lock().unwrap().append(&extracted);
    if dropped {
        log::warn!(
            "[WARN][BACKEND] mux pane {} raw_passthrough capacity exceeded; oldest captured bytes dropped",
            pane_id
        );
    }
}

/// Forward each decoded agent-status report to the daemon-level agent-status
/// task via `agent_status_report_sender`.
///
/// Unlike the best-effort PTY-output passthrough, an accepted report MUST
/// reach the daemon — SPEC FR3 requires every accepted report to advance the
/// pane's revision, so silently dropping one on a full channel would be a
/// spec bug. A full channel therefore falls back to a blocking send instead
/// of dropping (review round-1 stable_id `try_send_drops_reports`, addressed
/// alongside the per-pane statefulness rework since the fix naturally
/// extends here).
///
/// Runs OUTSIDE the `agent_status_report_sender` lock (the sender is cloned
/// out and the lock released before any send), mirroring the "release lock
/// before blocking_send" discipline the PTY-output backpressure path above
/// already follows — a blocked send here cannot deadlock against the
/// session-manager lock the consuming `run_agent_status_task` also needs.
fn forward_agent_status_reports(
    pane_id: PaneId,
    reports: Vec<String>,
    agent_status_report_sender: &SharedAgentStatusReportSender,
) {
    if reports.is_empty() {
        return;
    }
    let sender = agent_status_report_sender.lock().unwrap().clone();
    let Some(tx) = sender else {
        return;
    };
    for report in reports {
        match tx.try_send((pane_id, report)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(msg)) => {
                log::debug!(
                    "pane {} agent-status channel full; falling back to blocking send",
                    pane_id
                );
                if tx.blocking_send(msg).is_err() {
                    log::warn!(
                        "pane {} agent-status report not delivered: receiver dropped",
                        pane_id
                    );
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                log::debug!(
                    "pane {} agent-status channel closed; report not delivered",
                    pane_id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::visibility::HIDDEN_PASSTHROUGH_CAPACITY_MUX;
    use term_core::terminal_core::ReplaySegment;

    /// Convert term_core's `ReplaySegment` (used by test-side recording
    /// construction) into the mux-layer's plain `(usize, u16, u16)` tuples
    /// `build_snapshot_bytes` accepts.
    fn to_tuples(segments: &[ReplaySegment]) -> Vec<(usize, u16, u16)> {
        segments
            .iter()
            .map(|s| (s.offset as usize, s.cols, s.rows))
            .collect()
    }

    /// Convert the mux-layer's plain `(usize, u16, u16)` tuples (as
    /// returned by `build_snapshot_bytes`) into `ReplaySegment`s for
    /// `reset_and_replay_segments`.
    fn to_replay_segments(tuples: &[(usize, u16, u16)]) -> Vec<ReplaySegment> {
        tuples
            .iter()
            .map(|&(offset, cols, rows)| ReplaySegment {
                offset: offset as u32,
                cols,
                rows,
            })
            .collect()
    }

    // ── pane_env_vars (EMTERM_PANE_ID injection, AC-6) ─────────────────────

    #[test]
    fn pane_env_vars_includes_emterm_pane_id_matching_the_given_public_id() {
        let vars = pane_env_vars("abc123-7");
        let found = vars.iter().find(|(k, _)| *k == "EMTERM_PANE_ID");
        assert_eq!(found.map(|(_, v)| v.as_str()), Some("abc123-7"));
    }

    #[test]
    fn pane_env_vars_pane_id_differs_per_pane() {
        let get = |vars: &[(&str, String)]| {
            vars.iter()
                .find(|(k, _)| *k == "EMTERM_PANE_ID")
                .unwrap()
                .1
                .clone()
        };
        let a = get(&pane_env_vars("abc123-1"));
        let b = get(&pane_env_vars("abc123-2"));
        assert_ne!(a, b, "distinct public pane ids must round-trip distinctly");
    }

    #[test]
    fn pane_env_vars_keeps_existing_fixed_vars() {
        let vars = pane_env_vars("abc123-1");
        let get = |key: &str| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
        assert_eq!(get("TERM"), Some("xterm-256color".to_string()));
        assert_eq!(get("COLORTERM"), Some("truecolor".to_string()));
        assert_eq!(get("TERM_PROGRAM"), Some("emterm".to_string()));
        assert_eq!(get("EMTERM_MUX"), Some("1".to_string()));
    }

    // ── extract_main_buffer_bytes (alt-screen scrollback gating) ──────────

    #[test]
    fn extract_main_buffer_pure_main_borrows_whole_chunk() {
        let (bytes, alt) = extract_main_buffer_bytes(b"hello \x1b[31mworld\x1b[0m", false);
        assert_eq!(&*bytes, b"hello \x1b[31mworld\x1b[0m");
        assert!(!alt);
        assert!(
            matches!(bytes, Cow::Borrowed(_)),
            "no-toggle main chunk must borrow"
        );
    }

    #[test]
    fn extract_main_buffer_pure_alt_yields_nothing() {
        let (bytes, alt) = extract_main_buffer_bytes(b"alt frame redraw", true);
        assert!(bytes.is_empty());
        assert!(alt);
    }

    #[test]
    fn extract_main_buffer_keeps_prefix_before_alt_enter() {
        // Command output then a TUI opens in the SAME read — the leading
        // main-buffer output must survive (the regression this guards).
        let (bytes, alt) = extract_main_buffer_bytes(b"PRE-OUTPUT\x1b[?1049hALTUI", false);
        assert_eq!(&*bytes, b"PRE-OUTPUT");
        assert!(alt);
    }

    #[test]
    fn extract_main_buffer_keeps_suffix_after_alt_exit() {
        let (bytes, alt) = extract_main_buffer_bytes(b"ALTUI\x1b[?1049lPOST-PROMPT", true);
        assert_eq!(&*bytes, b"POST-PROMPT");
        assert!(!alt);
    }

    #[test]
    fn extract_main_buffer_drops_balanced_internal_alt_span() {
        let (bytes, alt) = extract_main_buffer_bytes(b"A\x1b[?1049hHIDDEN\x1b[?1049lB", false);
        assert_eq!(&*bytes, b"AB");
        assert!(!alt);
    }

    #[test]
    fn extract_main_buffer_handles_47_and_1047_forms() {
        let (b1, a1) = extract_main_buffer_bytes(b"X\x1b[?47hY", false);
        assert_eq!(&*b1, b"X");
        assert!(a1);
        let (b2, a2) = extract_main_buffer_bytes(b"X\x1b[?1047lY", true);
        assert_eq!(&*b2, b"Y");
        assert!(!a2);
    }

    // ── ScrollbackWriteFilter (stateful scrollback-write stripper) ─────────

    fn new_scrollback(cap: usize) -> SharedScrollback {
        use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
        Arc::new(StdMutex::new(ScrollbackRingBuffer::new(cap)))
    }

    /// Feed `chunks` through a fresh [`ScrollbackWriteFilter`] and write each
    /// filter output to `scrollback`, mirroring what `pty_reader_loop` does.
    /// Bytes still held in the filter's `pending` at the end are NOT written
    /// — matching production semantics (a still-pending unterminated
    /// sequence is held until a subsequent read carries its terminator, or
    /// the pending cap flushes it).
    fn feed_all(scrollback: &SharedScrollback, chunks: &[&[u8]]) -> ScrollbackWriteFilter {
        let mut filter = ScrollbackWriteFilter::new();
        for chunk in chunks {
            let filtered = filter.feed(chunk);
            if !filtered.is_empty() {
                scrollback.lock().unwrap().write(&filtered);
            }
        }
        filter
    }

    /// A viewer-launch OSC 777 emterm-markdown sequence delivered in a single
    /// read is stripped BEFORE ever landing in the ring — so subsequent
    /// overflow can't leave a headerless base64 tail behind for snapshot
    /// replay.
    #[test]
    fn scrollback_filter_strips_osc777_markdown_viewer_in_one_feed() {
        let scrollback = new_scrollback(2048);
        let chunk = b"before\x1b]777;emterm;markdown;chunk;id=x;data=xxx\x07after";
        feed_all(&scrollback, &[chunk]);
        assert_eq!(scrollback.lock().unwrap().read_all(), b"beforeafter");
    }

    /// Fold marks (`777;emterm;fold;…`) and other non-viewer content are
    /// preserved — the stripper only targets replayable viewer kinds.
    #[test]
    fn scrollback_filter_keeps_fold_and_plain_text() {
        let scrollback = new_scrollback(2048);
        let chunk = b"$ ls\r\n\x1b]777;emterm;fold;start;42\x07file.rs\r\n";
        feed_all(&scrollback, &[chunk]);
        assert_eq!(scrollback.lock().unwrap().read_all(), &chunk[..]);
    }

    /// Kitty graphics APC and OSC 9999 emterm-md are also viewer-adjacent
    /// and must be stripped at write time (same coverage as the snapshot-time
    /// stripper's SSOT).
    #[test]
    fn scrollback_filter_strips_kitty_and_osc9999_emterm_md() {
        let scrollback = new_scrollback(2048);
        let chunk = b"A\x1b_Gi=1;PAYLOAD\x1b\\B\x1b]9999;emterm-md;begin\x07C";
        feed_all(&scrollback, &[chunk]);
        assert_eq!(scrollback.lock().unwrap().read_all(), b"ABC");
    }

    /// **Regression** for the reported bug shape: `emterm markdown` emits a
    /// 128 KiB payload per OSC 777 chunk while the PTY reader buffer is 64 KiB
    /// (`buf = [0u8; 65536]`), so ONE viewer chunk always arrives via ≥ 2
    /// `feed` calls — first with the introducer + partial base64, then with
    /// the rest + terminator. The pre-fix stateless helper let both halves
    /// land in the ring verbatim (the first half unterminated, the second
    /// with no introducer), so ring overflow could evict the introducer while
    /// the base64 tail survived and got replayed to the client on tab-switch.
    /// With the stateful filter, the unterminated first half is held in
    /// `pending`; the terminator arrives in the second half; the complete
    /// sequence is stripped in one shot, so ring content is clean.
    #[test]
    fn scrollback_filter_strips_osc777_chunk_split_across_reads() {
        let scrollback = new_scrollback(256 * 1024);
        // Build the CLI-shaped OSC 777 markdown chunk: prefix + 100 KiB
        // base64-looking body + ST terminator. Total ~100 KiB, larger than a
        // 64 KiB PTY read.
        let mut full = Vec::from(&b"pre\r\n"[..]);
        full.extend_from_slice(b"\x1b]777;emterm;markdown;chunk;id=abc;seq=0;data=");
        full.extend(std::iter::repeat_n(b'A', 100_000));
        full.extend_from_slice(b"\x1b\\");
        full.extend_from_slice(b"post");
        // Split at exactly 64 KiB — the boundary a real PTY reader would use.
        let (head, tail) = full.split_at(65_536);
        let filter = feed_all(&scrollback, &[head, tail]);
        // Post-completion, `pending` should be drained back to zero.
        assert_eq!(filter.pending_len(), 0);
        let stored = scrollback.lock().unwrap().read_all();
        assert_eq!(stored, b"pre\r\npost");
    }

    /// The filter must hold an unterminated introducer across an *arbitrary*
    /// split point (not just aligned on the reader-buffer boundary above).
    /// This variant splits at the introducer's `;data=` marker and again in
    /// the middle of the base64 body to exercise multiple pending / drain
    /// cycles.
    #[test]
    fn scrollback_filter_strips_osc777_chunk_split_multi_boundary() {
        let scrollback = new_scrollback(256 * 1024);
        let mut full = Vec::from(&b"A"[..]);
        full.extend_from_slice(b"\x1b]777;emterm;image;chunk;id=x;seq=0;data=");
        full.extend(std::iter::repeat_n(b'B', 90_000));
        full.extend_from_slice(b"\x1b\\");
        full.extend_from_slice(b"Z");
        // Splits: right before `data=` payload, then midway through the body.
        let mid1 = full
            .windows(5)
            .position(|w| w == b"data=")
            .expect("data= present")
            + 5;
        let mid2 = mid1 + 40_000;
        let a = &full[..mid1];
        let b = &full[mid1..mid2];
        let c = &full[mid2..];
        let filter = feed_all(&scrollback, &[a, b, c]);
        assert_eq!(filter.pending_len(), 0);
        assert_eq!(scrollback.lock().unwrap().read_all(), b"AZ");
    }

    /// The pending buffer must not accept unbounded growth from a stream that
    /// never terminates its introducer. Past
    /// [`SCROLLBACK_FILTER_PENDING_CAP`], the filter forwards the pending
    /// run raw and resets. This trades the strip guarantee for a bounded
    /// per-pane memory footprint (defensive escape hatch, not a correctness
    /// path — reported via warn log).
    #[test]
    fn scrollback_filter_flushes_raw_past_pending_cap() {
        let _scrollback = new_scrollback(2 * 1024 * 1024);
        // Start an unterminated OSC 777 introducer, then keep feeding padding
        // until pending exceeds the cap. No terminator ever arrives.
        let mut filter = ScrollbackWriteFilter::new();
        let intro = b"\x1b]777;emterm;markdown;chunk;id=x;seq=0;data=";
        let out = filter.feed(intro);
        assert!(out.is_empty(), "introducer alone is held pending");
        // Feed 520 KiB of body without a terminator; sum crosses 512 KiB.
        let padding: Vec<u8> = std::iter::repeat_n(b'A', 520 * 1024).collect();
        let flushed = filter.feed(&padding);
        // The cap escape hatch fired: pending drained raw, ring wrote the raw
        // bytes. The exact contents don't matter for correctness — the
        // invariant is that pending doesn't grow past the cap.
        assert!(!flushed.is_empty(), "cap escape hatch must emit raw bytes");
        assert_eq!(filter.pending_len(), 0);
    }

    /// Plain text that contains no ESC at all is emitted directly with no
    /// pending held over — the common hot path stays boundary-clean.
    #[test]
    fn scrollback_filter_plain_text_emits_immediately() {
        let scrollback = new_scrollback(2048);
        let chunk = b"$ echo hello world\r\nhello world\r\n$ ";
        let filter = feed_all(&scrollback, &[chunk]);
        assert_eq!(filter.pending_len(), 0);
        assert_eq!(scrollback.lock().unwrap().read_all(), &chunk[..]);
    }

    /// A non-strip-target CSI sequence (e.g. SGR color) does NOT force a
    /// pending hold — only OSC / APC / DCS introducers do. This keeps the
    /// per-chunk emission latency low on typical shell output.
    #[test]
    fn scrollback_filter_csi_does_not_force_pending() {
        let scrollback = new_scrollback(2048);
        let chunk = b"hi \x1b[31mred\x1b[0m done";
        let filter = feed_all(&scrollback, &[chunk]);
        assert_eq!(filter.pending_len(), 0);
        assert_eq!(scrollback.lock().unwrap().read_all(), &chunk[..]);
    }

    /// task0012 AC-5: an `agent-status` OSC report split across chunk
    /// boundaries must never land in scrollback. `ScrollbackWriteFilter`
    /// already holds ANY unterminated OSC introducer (not just viewer
    /// kinds) pending until a terminator arrives — this is a regression /
    /// confirmation test that the same guarantee covers `agent-status`
    /// reports now that the extraction side is stateful too.
    #[test]
    fn scrollback_filter_strips_agent_status_report_split_across_reads() {
        let scrollback = new_scrollback(4096);
        let full =
            b"pre\x1b]777;emterm;agent-status;v=1;state=working;name=claude\x07post".as_slice();
        let split = full
            .windows(4)
            .position(|w| w == b"stat")
            .expect("marker present");
        let (head, tail) = full.split_at(split);
        let filter = feed_all(&scrollback, &[head, tail]);
        assert_eq!(filter.pending_len(), 0);
        assert_eq!(scrollback.lock().unwrap().read_all(), b"prepost");
    }

    type TestRig = (
        SharedRawPassthrough,
        SharedPassthroughScanner,
        SharedNotificationSender,
        mpsc::Receiver<(PaneId, String)>,
    );

    fn shared_buffer() -> TestRig {
        let (notif_tx, notif_rx) = mpsc::channel::<(PaneId, String)>(16);
        (
            Arc::new(StdMutex::new(RawPassthroughBuffer::new(
                HIDDEN_PASSTHROUGH_CAPACITY_MUX,
            ))),
            Arc::new(StdMutex::new(PassthroughScanner::new())),
            Arc::new(StdMutex::new(Some(notif_tx))),
            notif_rx,
        )
    }

    /// TS-19: passthrough sequences feed raw_passthrough while detached.
    #[test]
    fn capture_passthrough_appends_completed_kitty_apc() {
        let (buf, scanner, notif, _rx) = shared_buffer();
        capture_passthrough(7, b"\x1b_Gi=1;ZZ\x1b\\", &buf, &scanner, &notif);
        let stored = buf.lock().unwrap().read_all();
        assert!(
            stored
                .windows(b"\x1b_Gi=1;ZZ\x1b\\".len())
                .any(|w| w == b"\x1b_Gi=1;ZZ\x1b\\"),
            "captured bytes must contain the original Kitty APC sequence"
        );
    }

    /// TS-19: a sequence split across two chunks is still recovered because
    /// the scanner is stateful and shared.
    #[test]
    fn capture_passthrough_handles_chunk_boundary() {
        let (buf, scanner, notif, _rx) = shared_buffer();
        capture_passthrough(7, b"\x1b_Gi=1;Z", &buf, &scanner, &notif);
        // Mid-sequence: nothing complete yet.
        assert_eq!(buf.lock().unwrap().len(), 0);
        capture_passthrough(7, b"Z\x1b\\", &buf, &scanner, &notif);
        let stored = buf.lock().unwrap().read_all();
        assert!(
            stored
                .windows(b"\x1b_Gi=1;ZZ\x1b\\".len())
                .any(|w| w == b"\x1b_Gi=1;ZZ\x1b\\"),
            "chunk-split sequence must be reassembled"
        );
    }

    /// Plain output that contains no image / Markdown OSC must not touch
    /// the raw buffer.
    #[test]
    fn capture_passthrough_ignores_plain_text() {
        let (buf, scanner, notif, _rx) = shared_buffer();
        capture_passthrough(7, b"hello world\n", &buf, &scanner, &notif);
        assert_eq!(buf.lock().unwrap().len(), 0);
    }

    /// TS-9: a Detached pane emitting `OSC 9 ; msg` forwards a notification
    /// through the notification channel and does NOT add it to raw_passthrough.
    #[test]
    fn capture_passthrough_forwards_osc9_notification() {
        let (buf, scanner, notif, mut rx) = shared_buffer();
        capture_passthrough(7, b"\x1b]9;deploy done\x07", &buf, &scanner, &notif);
        // Notification forwarded.
        let (pane_id, message) = rx.try_recv().expect("notification must be forwarded");
        assert_eq!(pane_id, 7);
        assert_eq!(message, "deploy done");
        // Must NOT be in raw_passthrough (no replay on reattach).
        assert_eq!(
            buf.lock().unwrap().len(),
            0,
            "OSC 9 notification must not enter raw_passthrough"
        );
    }

    /// FR4: a progress sequence on a Detached pane is not forwarded.
    #[test]
    fn capture_passthrough_ignores_osc9_progress() {
        let (buf, scanner, notif, mut rx) = shared_buffer();
        capture_passthrough(7, b"\x1b]9;4;1;50\x07", &buf, &scanner, &notif);
        assert!(
            rx.try_recv().is_err(),
            "progress sequence must not forward a notification"
        );
        assert_eq!(buf.lock().unwrap().len(), 0);
    }

    /// A chunk-split OSC 9 notification is forwarded once the closing chunk
    /// arrives, because the scanner is stateful and shared.
    #[test]
    fn capture_passthrough_forwards_chunk_split_osc9() {
        let (buf, scanner, notif, mut rx) = shared_buffer();
        capture_passthrough(7, b"\x1b]9;long ", &buf, &scanner, &notif);
        assert!(rx.try_recv().is_err(), "no completion yet");
        capture_passthrough(7, b"message\x1b\\", &buf, &scanner, &notif);
        let (_pane_id, message) = rx.try_recv().expect("notification after closing chunk");
        assert_eq!(message, "long message");
    }

    // ── forward_agent_status_reports (task0012 AC-6 / try_send_drops_reports) ─

    /// Baseline: a healthy channel delivers via the `try_send` fast path,
    /// no blocking.
    #[test]
    fn forward_agent_status_reports_delivers_via_try_send_when_channel_has_room() {
        let (tx, mut rx) = mpsc::channel::<(PaneId, String)>(4);
        let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(Some(tx)));
        forward_agent_status_reports(3, vec!["emterm;agent-status;clear".to_string()], &sender);
        let (pane_id, report) = rx.try_recv().expect("report must be delivered");
        assert_eq!(pane_id, 3);
        assert_eq!(report, "emterm;agent-status;clear");
    }

    /// Empty report list is a no-op — no send attempted, no panic on a
    /// `None` sender either.
    #[test]
    fn forward_agent_status_reports_empty_list_is_noop() {
        let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(None));
        // Must not panic even though the sender is unset.
        forward_agent_status_reports(1, Vec::new(), &sender);
    }

    /// AC-6: a full channel must NOT silently drop an accepted report
    /// (review round-1 stable_id `try_send_drops_reports`). This proves the
    /// blocking-send fallback actually delivers once capacity frees, rather
    /// than the old best-effort `try_send` that discarded on `Full`.
    #[test]
    fn forward_agent_status_reports_blocks_instead_of_dropping_on_full_channel() {
        let (tx, mut rx) = mpsc::channel::<(PaneId, String)>(1);
        // Fill the one available slot directly so the channel is Full.
        tx.try_send((1, "first".to_string())).unwrap();
        let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(Some(tx)));

        let sender_for_thread = sender.clone();
        let handle = std::thread::spawn(move || {
            forward_agent_status_reports(1, vec!["second".to_string()], &sender_for_thread);
        });

        // Drain the first item, freeing the slot the blocked send is
        // waiting on.
        let first = rx.blocking_recv().expect("first item must still arrive");
        assert_eq!(first, (1, "first".to_string()));

        // The blocking send only completes once the slot is free, so this
        // recv is what proves "second" was not dropped.
        let second = rx.blocking_recv().expect("second item must not be dropped");
        assert_eq!(second, (1, "second".to_string()));

        handle.join().expect("forwarding thread must not panic");
    }

    /// A closed channel (receiver dropped) is handled gracefully — the send
    /// is a no-op, no panic, no hang.
    #[test]
    fn forward_agent_status_reports_closed_channel_does_not_panic() {
        let (tx, rx) = mpsc::channel::<(PaneId, String)>(1);
        drop(rx);
        let sender: SharedAgentStatusReportSender = Arc::new(StdMutex::new(Some(tx)));
        forward_agent_status_reports(1, vec!["orphaned".to_string()], &sender);
    }

    // ── task0004 round-4 rework (D1'): dimensions travel as structural
    // segments (`term_core::terminal_core::ReplaySegment`) alongside the
    // payload — never as an in-band `OSC 777;emterm;resize;…` marker byte
    // sequence. Rounds 1-3's marker-scanning fix (IMPLEMENTATION.md D1,
    // `tmp/apt-progress-bar-regression-2026-07-09.md` PROBE D) is superseded:
    // the coordinate-drift regression tests below now construct explicit
    // segment lists instead of embedding marker bytes, and the forgery tests
    // (AC-1) prove marker-SHAPED byte sequences carry no authority at all —
    // whatever survives the write-path strip (now identical for every kind,
    // see `strip_pty_output_for_scrollback_write`'s doc comment) becomes
    // ordinary content with no bearing on replay dimensions, because nothing
    // scans for one any more.

    /// apt-style scroll-region + bottom-bar recording (mirrors PROBE D's
    /// synthesis, `install-progress.cc`'s SIGWINCH re-setup behavior). No
    /// marker bytes are embedded; the caller supplies the segment
    /// boundaries explicitly, mirroring what `ScrollbackRingBuffer::
    /// read_segments` would report for a real `MuxPane::new` (initial dims)
    /// + `MuxPane::resize` (mid-run SIGWINCH) sequence.
    fn synth_apt_bytes_with_midrun_resize(
        cols: u16,
        rows_a: u16,
        rows_b: u16,
    ) -> (Vec<u8>, Vec<ReplaySegment>) {
        let mut b = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols,
            rows: rows_a,
        }];
        // Fill history so the cursor starts at the bottom, matching a real
        // terminal's state when a long-running command begins.
        for i in 0..rows_a.max(rows_b) + 20 {
            b.extend_from_slice(format!("history line {i} filling the screen\r\n").as_bytes());
        }
        b.extend_from_slice(b"$ sudo apt reinstall ./build/emterm.deb\r\n");
        // Start (rows_a-shaped scroll region + bottom bar).
        b.extend_from_slice(b"\n\x1b7");
        b.extend_from_slice(format!("\x1b[0;{}r", rows_a - 1).as_bytes());
        b.extend_from_slice(b"\x1b8\x1b[1A");
        for pct in [0u32, 15, 30] {
            b.extend_from_slice(
                format!("emterm log line at {pct} percent unpacking something\r\n").as_bytes(),
            );
            let filled = (pct as usize * 60) / 100;
            b.extend_from_slice(
                format!(
                    "\x1b7\x1b[{rows_a};0f\x1b[42m\x1b[30m進捗: [{pct:3}%] [{}{}]\x1b[49m\x1b[39m\x1b[0m\x1b8",
                    "\u{2588}".repeat(filled),
                    " ".repeat(60 - filled),
                )
                .as_bytes(),
            );
        }
        // SIGWINCH: the daemon records a segment for rows_b at THIS offset
        // (this is what `ScrollbackRingBuffer::write_resize_marker` records
        // via `MuxPane::resize`), then apt re-sets up the scroll region +
        // bar for rows_b.
        segments.push(ReplaySegment {
            offset: b.len() as u32,
            cols,
            rows: rows_b,
        });
        b.extend_from_slice(b"\n\x1b7");
        b.extend_from_slice(format!("\x1b[0;{}r", rows_b - 1).as_bytes());
        b.extend_from_slice(b"\x1b8\x1b[1A");
        for pct in [45u32, 60, 75, 90, 100] {
            b.extend_from_slice(
                format!("emterm log line at {pct} percent installing package\r\n").as_bytes(),
            );
            let filled = (pct as usize * 60) / 100;
            b.extend_from_slice(
                format!(
                    "\x1b7\x1b[{rows_b};0f\x1b[42m\x1b[30m進捗: [{pct:3}%] [{}{}]\x1b[49m\x1b[39m\x1b[0m\x1b8",
                    "\u{2588}".repeat(filled),
                    " ".repeat(60 - filled),
                )
                .as_bytes(),
            );
        }
        // Stop (rows_b-shaped).
        b.extend_from_slice(b"\x1b7");
        b.extend_from_slice(format!("\x1b[0;{rows_b}r").as_bytes());
        b.extend_from_slice(b"\x1b8\x1b[J");
        b.extend_from_slice(b"$ done\r\n");
        (b, segments)
    }

    /// AC-2: the apt-style recording, replayed through the full snapshot
    /// pipeline into a fixed-size core, produces ZERO rows mixing bar
    /// fragments with log-line content — fails when the segment authority
    /// is dropped (PROBE D observed 1-3 tainted rows per case for the same
    /// synthesis without dimension attribution).
    #[test]
    fn apt_style_recording_replays_without_cross_line_mixing() {
        use term_core::terminal_core::TerminalCore;
        let cols: u16 = 120;
        for (rec_a, rec_b, replay_rows) in [
            (47u16, 48u16, 47u16),
            (47, 48, 48),
            (48, 47, 47),
            (48, 47, 48),
        ] {
            let (recording, segments) = synth_apt_bytes_with_midrun_resize(cols, rec_a, rec_b);
            let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
                &recording,
                &to_tuples(&segments),
                b"",
                false,
            );
            let mut core = TerminalCore::new(cols, replay_rows, 10_000);
            core.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));
            let mut tainted = Vec::new();
            for r in 0..replay_rows {
                let line = core.get_line_text(r);
                let has_bar = line.contains('\u{2588}') || line.trim_end().ends_with(']');
                let has_log = line.contains("percent");
                if has_bar && has_log {
                    tainted.push(format!("row {r}: {line}"));
                }
            }
            assert!(
                tainted.is_empty(),
                "rec {rec_a}->{rec_b} replay@{replay_rows}: expected zero cross-line-mixed rows \
                 with segment attribution, got {tainted:?}"
            );
        }
    }

    /// Cursor-addressed TUI-style recording: a status/input row pinned to
    /// the bottom of the screen via a scroll region excluding it (mirrors
    /// an app like Claude Code keeping its status/input line in place while
    /// chat content scrolls above — `project_status_bar_design`), re-painted
    /// in place via `ESC7 CUP <text> ESC8` on every tick while chat content
    /// scrolls via plain `\r\n` above it. Row count changes mid-run; the
    /// caller supplies the segment boundaries explicitly (no marker bytes).
    ///
    /// Distinguishes from AC-2's apt-style bar (bottom-row-only redraw, no
    /// scrolling *inside* the reserved region) via genuinely scrolling chat
    /// content interleaved with the fixed-row status — the mechanism PROBE D
    /// identified (a scroll-region boundary computed for the WRONG row
    /// count lets scrolled content invade a row that should have been
    /// exempt) reproducing for "an app with a pinned status line" shape, not
    /// only "an app with a growing progress bar" shape.
    fn synth_tui_cursor_addressed_bytes_with_midrun_resize(
        rows_a: u16,
        rows_b: u16,
    ) -> (Vec<u8>, Vec<ReplaySegment>) {
        let mut b = Vec::new();
        let mut segments = vec![ReplaySegment {
            offset: 0,
            cols: 100,
            rows: rows_a,
        }];
        for i in 0..rows_a.max(rows_b) + 20 {
            b.extend_from_slice(format!("chat history line {i}\r\n").as_bytes());
        }
        // Phase A: reserve the bottom row for a status/input line (scroll
        // region excludes it) while chat content scrolls above and the
        // status row is periodically re-painted in place.
        b.extend_from_slice(b"\n\x1b7");
        b.extend_from_slice(format!("\x1b[0;{}r", rows_a - 1).as_bytes());
        b.extend_from_slice(b"\x1b8\x1b[1A");
        for tick in 0..3u32 {
            b.extend_from_slice(format!("chat reply A line {tick}\r\n").as_bytes());
            b.extend_from_slice(format!("\x1b7\x1b[{rows_a};0fSTATUS-A[{tick}]\x1b8").as_bytes());
        }
        // SIGWINCH: a new segment at this offset, then the same pattern
        // re-established for rows_b.
        segments.push(ReplaySegment {
            offset: b.len() as u32,
            cols: 100,
            rows: rows_b,
        });
        b.extend_from_slice(b"\n\x1b7");
        b.extend_from_slice(format!("\x1b[0;{}r", rows_b - 1).as_bytes());
        b.extend_from_slice(b"\x1b8\x1b[1A");
        for tick in 0..3u32 {
            b.extend_from_slice(format!("chat reply B line {tick}\r\n").as_bytes());
            b.extend_from_slice(format!("\x1b7\x1b[{rows_b};0fSTATUS-B[{tick}]\x1b8").as_bytes());
        }
        (b, segments)
    }

    /// AC-2: the TUI-style cursor-addressed recording, replayed after the
    /// fix, shows no row mixing content from phase A (pre-resize) and phase
    /// B (post-resize) — the reported symptom's shape (Claude Code's
    /// status/input area), covering a pinned-status-row app rather than
    /// only apt's growing-bar pattern. Fails when segment attribution is
    /// dropped, for every combination (each pairs a row-count change with a
    /// replay target that differs from at least one recorded size).
    #[test]
    fn tui_cursor_addressed_recording_replays_without_cross_line_mixing() {
        use term_core::terminal_core::TerminalCore;
        let cols: u16 = 100;
        for (rec_a, rec_b, replay_rows) in [
            (30u16, 32u16, 30u16),
            (30, 32, 32),
            (32, 30, 30),
            (32, 30, 32),
        ] {
            let (recording, segments) =
                synth_tui_cursor_addressed_bytes_with_midrun_resize(rec_a, rec_b);
            let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
                &recording,
                &to_tuples(&segments),
                b"",
                false,
            );
            let mut core = TerminalCore::new(cols, replay_rows, 10_000);
            core.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));
            let mut tainted = Vec::new();
            for r in 0..replay_rows {
                let line = core.get_line_text(r);
                // A row is tainted if it shows a STATUS redraw fragment
                // glued to a scrolled chat-line fragment — the "bar
                // fragment landed on a log-content row" shape (mirrors
                // AC-2's `has_bar && has_log` detector; not phase-specific,
                // since the coordinate-drift bug glues ANY two logically
                // distinct writes onto one physical row).
                if line.contains("STATUS-") && line.contains(" line ") {
                    tainted.push(format!("row {r}: {line}"));
                }
            }
            assert!(
                tainted.is_empty(),
                "rec {rec_a}->{rec_b} replay@{replay_rows}: expected zero cross-phase-mixed rows, \
                 got {tainted:?}"
            );
        }
    }

    /// AC-11: a segment-free recording replays to the SAME grid as a
    /// straight `reset()` + full-drain `process_pty_data_fully` — the
    /// documented older-daemon degradation (single-dimension replay).
    #[test]
    fn segment_free_recording_replays_unchanged() {
        use term_core::terminal_core::TerminalCore;
        let cols: u16 = 80;
        let rows: u16 = 24;
        let mut recording = Vec::new();
        for i in 0..40 {
            recording.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        recording.extend_from_slice(b"\x1b[31mred\x1b[0m plain\r\n");

        let mut reference = TerminalCore::new(cols, rows, 1000);
        reference.reset();
        reference.process_pty_data_fully(&recording);

        let mut under_test = TerminalCore::new(cols, rows, 1000);
        under_test.reset_and_replay_segments(&recording, &[]);

        for r in 0..rows {
            assert_eq!(
                under_test.get_line_text(r),
                reference.get_line_text(r),
                "row {r} differs: segment-free replay must be byte-path unchanged"
            );
        }
        assert_eq!(under_test.cols(), reference.cols());
        assert_eq!(under_test.rows(), reference.rows());
    }

    // ── AC-1: marker-SHAPED byte sequences carry no segment authority,
    // however they are shaped, split, nested, or reconstructed ───────────

    /// Byte-for-byte the OLD (pre-round-4) marker wire format. Kept ONLY as
    /// adversarial test fixture data — no production decoder recognizes
    /// this shape any more.
    fn legacy_marker_shaped_bytes(cols: u16, rows: u16) -> Vec<u8> {
        format!("\x1b]777;emterm;resize;{cols};{rows}\x07").into_bytes()
    }

    /// Replay `bytes` with NO segments and return the reflow delta —
    /// the shared witness every AC-1 scenario below checks: per the task's
    /// test notes, `core.cols()`/`rows()` after a replay always equal the
    /// caller's target regardless of what happened mid-drain, so only the
    /// reflow counter (or a grid fingerprint) actually distinguishes
    /// "the adversarial bytes were honored" from "they were inert".
    fn reflow_delta_replaying_with_no_segments(bytes: &[u8]) -> u64 {
        use term_core::terminal_core::TerminalCore;
        let mut core = TerminalCore::new(80, 24, 1000);
        let before = core.reflow_call_count();
        core.reset_and_replay_segments(bytes, &[]);
        core.reflow_call_count() - before
    }

    /// AC-1 scenario 1 (bare sequence): a marker-shaped OSC 777 body
    /// arriving as ordinary child PTY output in ONE read now reaches the
    /// scrollback ring UNSTRIPPED (task0004 round-4 rework: there is no
    /// more `resize`-kind special-casing at the write path — see
    /// `strip_pty_output_for_scrollback_write`'s doc comment) — and that is
    /// fine: the ring's `write_resize_marker` was never called for it, so
    /// `read_segments()` reports zero segments regardless of what the byte
    /// content looks like, and replay never resizes on it.
    #[test]
    fn ac1_bare_marker_shaped_bytes_reach_ring_but_carry_no_segment_authority() {
        let scrollback = new_scrollback(4096);
        let marker = legacy_marker_shaped_bytes(65535, 65535);
        let mut chunk = b"before".to_vec();
        chunk.extend_from_slice(&marker);
        chunk.extend_from_slice(b"after");
        let filter = feed_all(&scrollback, &[&chunk]);
        assert_eq!(filter.pending_len(), 0);
        let (bytes, segments) = scrollback.lock().unwrap().read_segments();
        assert!(
            segments.is_empty(),
            "a ring that never called write_resize_marker must report zero \
             segments, regardless of marker-shaped byte content"
        );
        assert!(
            bytes.windows(marker.len()).any(|w| w == marker.as_slice()),
            "marker-shaped bytes now survive as ordinary (authority-less) \
             content — there is no more resize-kind stripping"
        );
        assert_eq!(
            reflow_delta_replaying_with_no_segments(&bytes),
            0,
            "surviving marker-shaped bytes must never trigger a resize"
        );
    }

    /// AC-1 scenario 2 (split across two filter batches): the SAME
    /// marker-shaped sequence, but delivered across TWO separate
    /// `ScrollbackWriteFilter::feed` calls (mirroring two PTY `read()`
    /// calls) — still zero segment authority, zero reflow.
    ///
    /// Confirmed to fail pre-fix: against the removed round-1/round-2
    /// write-path strip (`strip_pty_output_for_scrollback_write`'s old
    /// `resize`-kind special-casing), a marker split exactly at this point
    /// was the round-3 finding `4a22bd439fcdaf56` scenario — the flush
    /// boundary let a complete marker re-form on the ring side across the
    /// two batches, undetected by either single-batch strip pass.
    #[test]
    fn ac1_marker_shaped_bytes_split_across_two_filter_batches_carry_no_segment_authority() {
        let scrollback = new_scrollback(4096);
        let marker = legacy_marker_shaped_bytes(4096, 4096);
        let mut full = b"before".to_vec();
        full.extend_from_slice(&marker);
        full.extend_from_slice(b"after");
        let split_at = b"before".len() + marker.len() / 2;
        let (first, second) = full.split_at(split_at);
        let filter = feed_all(&scrollback, &[first, second]);
        assert_eq!(filter.pending_len(), 0);
        let (bytes, segments) = scrollback.lock().unwrap().read_segments();
        assert!(segments.is_empty());
        assert_eq!(reflow_delta_replaying_with_no_segments(&bytes), 0);
    }

    /// AC-1 scenario 3 (split at a lone-trailing-ESC boundary / pending
    /// overflow escape hatch): a child process opens an unterminated DCS
    /// introducer (`ESC P`, never closed) and keeps writing until
    /// `ScrollbackWriteFilter`'s pending buffer exceeds
    /// [`SCROLLBACK_FILTER_PENDING_CAP`], with a well-formed marker-shaped
    /// sequence sitting in the padding that gets flushed raw by the
    /// overflow escape hatch. Zero segment authority, zero reflow.
    #[test]
    fn ac1_marker_shaped_bytes_via_pending_overflow_escape_hatch_carry_no_segment_authority() {
        let scrollback = new_scrollback(4096);
        let intro = b"\x1bPtmux;".to_vec();
        let mut padding_and_marker: Vec<u8> = std::iter::repeat_n(b'A', 520 * 1024).collect();
        padding_and_marker.extend_from_slice(&legacy_marker_shaped_bytes(999, 999));

        let mut filter = ScrollbackWriteFilter::new();
        let first = filter.feed(&intro);
        assert!(
            first.is_empty(),
            "introducer alone must still be held pending"
        );
        let flushed = filter.feed(&padding_and_marker);
        assert!(
            !flushed.is_empty(),
            "the overflow escape hatch must have fired for this test to be meaningful"
        );
        assert_eq!(filter.pending_len(), 0);
        scrollback.lock().unwrap().write(&flushed);
        let (bytes, segments) = scrollback.lock().unwrap().read_segments();
        assert!(segments.is_empty());
        assert_eq!(reflow_delta_replaying_with_no_segments(&bytes), 0);
    }

    /// AC-1 scenario 4 (nested in a non-SIXEL DCS): mirrors
    /// `printf '\ePtmux;\e\e]777;emterm;resize;999;999\a\e\\'` — a doubled
    /// ESC is the tmux DCS passthrough convention for escaping a literal
    /// ESC inside the passthrough body, so the marker-shaped `ESC ]` is
    /// nested inside a non-SIXEL DCS. Zero segment authority, zero reflow.
    #[test]
    fn ac1_marker_shaped_bytes_nested_in_non_sixel_dcs_carry_no_segment_authority() {
        let scrollback = new_scrollback(4096);
        let mut chunk = b"before".to_vec();
        chunk.extend_from_slice(b"\x1bPtmux;\x1b");
        chunk.extend_from_slice(&legacy_marker_shaped_bytes(999, 999));
        chunk.extend_from_slice(b"\x1b\\");
        chunk.extend_from_slice(b"after");

        let filter = feed_all(&scrollback, &[&chunk]);
        assert_eq!(filter.pending_len(), 0);
        let (bytes, segments) = scrollback.lock().unwrap().read_segments();
        assert!(segments.is_empty());
        assert_eq!(reflow_delta_replaying_with_no_segments(&bytes), 0);
    }

    /// AC-1 scenario 5 (formed by concatenation after a strip pass): round-3
    /// finding `95fb7c115b0b64da`'s exact adversarial construction —
    /// `P + P + SIXEL_DCS + "1;1\a" + "999;999\a"` (`P` = the marker prefix,
    /// unterminated) — which, under the OLD single-pass literal strip
    /// (`strip_literal_resize_marker_occurrences`, removed by D1'),
    /// concatenated a surviving prefix with a surviving body into a
    /// complete forged marker. Feeds `strip_pty_output_for_scrollback_write`
    /// directly (the exact function the finding targeted) — since D1'
    /// carries dimensions structurally (never as bytes), there is no
    /// literal-marker strip pass left to defeat, and whatever this input
    /// reduces to can never carry segment authority regardless.
    #[test]
    fn ac1_marker_shaped_bytes_formed_by_concatenation_after_strip_carry_no_segment_authority() {
        let scrollback = new_scrollback(4096);
        let prefix = b"\x1b]777;emterm;resize;"; // unterminated marker prefix ("P")
        let sixel = b"\x1bP1;0;0q\"1;1;5;5#0;2;0;0;0\x1b\\"; // complete SIXEL DCS (stripped)
        let mut chunk = Vec::new();
        chunk.extend_from_slice(prefix);
        chunk.extend_from_slice(prefix);
        chunk.extend_from_slice(sixel);
        chunk.extend_from_slice(b"1;1\x07");
        chunk.extend_from_slice(b"999;999\x07");

        let stripped = strip_pty_output_for_scrollback_write(&chunk);
        scrollback.lock().unwrap().write(&stripped);
        let (bytes, segments) = scrollback.lock().unwrap().read_segments();
        assert!(
            segments.is_empty(),
            "no write_resize_marker call was ever made for this ring"
        );
        assert_eq!(reflow_delta_replaying_with_no_segments(&bytes), 0);
    }

    // ── round-trip fidelity: replayed grid matches a live-fed reference ───

    /// Round-trip fingerprint equality. The grid (line text + cursor) of a
    /// core fed a recording LIVE (`process_pty_data_fully`, resizing
    /// directly at each real PTY resize — mirrors what the daemon's own
    /// shadow parser sees) must equal that of a core replayed from the
    /// SNAPSHOT-ASSEMBLED bytes (`build_snapshot_bytes` ->
    /// `reset_and_replay_segments`, segment-aware), for both a
    /// segment-free recording and a resize-spanning one. This is the
    /// SPEC.md unit test list's "switch-time grid == replay grid"
    /// round-trip check: unlike the cross-line-mixing detectors above, it
    /// also catches a regression that drops, duplicates, or blanks content
    /// without literally interleaving two fragments onto one row.
    #[test]
    fn round_trip_grid_fingerprint_matches_live_feed_for_resize_free_and_resize_spanning() {
        use term_core::terminal_core::TerminalCore;
        let cols: u16 = 80;
        let rows: u16 = 24;

        fn fingerprint(core: &TerminalCore) -> (Vec<String>, u16, u16) {
            let mut lines = Vec::with_capacity(core.rows() as usize);
            for r in 0..core.rows() {
                lines.push(core.get_line_text(r));
            }
            (lines, core.get_cursor_col(), core.get_cursor_row())
        }

        // Case 1: segment-free recording.
        {
            let mut recording = Vec::new();
            for i in 0..30 {
                recording.extend_from_slice(format!("line {i}\r\n").as_bytes());
            }
            let mut live = TerminalCore::new(cols, rows, 1000);
            live.process_pty_data_fully(&recording);

            let (snap, snap_segments) =
                crate::mux::snapshot_bytes::build_snapshot_bytes(&recording, &[], b"", false);
            let mut replayed = TerminalCore::new(cols, rows, 1000);
            replayed.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));

            assert_eq!(
                fingerprint(&live),
                fingerprint(&replayed),
                "segment-free round-trip must match the live-fed reference"
            );
        }

        // Case 2: resize-spanning recording. The live reference resizes
        // directly at the exact point in the stream the segment boundary
        // represents, then restores to the target size at the end —
        // mirroring exactly what `reset_and_replay_segments` does.
        {
            let before = b"before\r\n".to_vec();
            let after = b"after\r\n".to_vec();
            let mut recording = before.clone();
            recording.extend_from_slice(&after);
            let segments = [
                ReplaySegment {
                    offset: 0,
                    cols,
                    rows,
                },
                ReplaySegment {
                    offset: before.len() as u32,
                    cols: 100,
                    rows: 30,
                },
            ];

            let mut live = TerminalCore::new(cols, rows, 1000);
            live.process_pty_data_fully(b"before\r\n");
            live.resize(100, 30);
            live.process_pty_data_fully(b"after\r\n");
            live.resize(cols, rows);

            let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
                &recording,
                &to_tuples(&segments),
                b"",
                false,
            );
            let mut replayed = TerminalCore::new(cols, rows, 1000);
            replayed.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));

            assert_eq!(
                fingerprint(&live),
                fingerprint(&replayed),
                "resize-spanning round-trip must match the live-fed reference"
            );
        }
    }

    /// Builds ONE phase's raw content (no marker bytes) for the
    /// differing-DIMENSIONS scenario below — reused both to assemble the
    /// segment-bearing recording AND to drive the segment-free LIVE
    /// reference (which resizes directly instead of via a segment).
    ///
    /// `narrower_cols` is fixed across both phases (the narrower of the
    /// recording's two cols values) so the long line wraps whenever the
    /// narrower width is in effect, regardless of which phase is active.
    fn synth_dims_phase_bytes(rows: u16, narrower_cols: u16, label: &str) -> Vec<u8> {
        let mut b = Vec::new();
        for i in 0..rows + 20 {
            b.extend_from_slice(format!("chat history line {i}\r\n").as_bytes());
        }
        b.extend_from_slice(b"\n\x1b7");
        b.extend_from_slice(format!("\x1b[0;{}r", rows.saturating_sub(1)).as_bytes());
        b.extend_from_slice(b"\x1b8\x1b[1A");
        // Longer than the narrower width in play across the whole
        // recording, so it wraps whenever that width is in effect.
        let long_line = "L".repeat(narrower_cols as usize + 20);
        for tick in 0..3u32 {
            b.extend_from_slice(format!("chat reply {label} {tick} {long_line}\r\n").as_bytes());
            b.extend_from_slice(
                format!("\x1b7\x1b[{rows};0f{label}-STATUS[{tick}]\x1b8").as_bytes(),
            );
        }
        b
    }

    /// The PREVIOUS version of this test (task0002 era) could never fail
    /// regardless of correctness — its rows were identical across both
    /// phases (so PROBE D's row-count coordinate-drift mechanism, which
    /// needs a ROW COUNT change to misinterpret DECSTBM / CUP coordinates,
    /// never fired at all), and its taint detector searched for a substring
    /// the cols-varying synthesized content never actually contained. This
    /// version varies BOTH rows and cols across the segment boundary
    /// (actually exercises the drift mechanism), still includes a line
    /// longer than the narrower width so it wraps, and compares against a
    /// LIVE-FED reference grid FINGERPRINT — confirmed, while developing
    /// this test, to FAIL when segment attribution is dropped (empty
    /// segments passed to `reset_and_replay_segments`).
    #[test]
    fn differing_dimensions_recording_matches_live_feed_fingerprint() {
        use term_core::terminal_core::TerminalCore;

        fn fingerprint(core: &TerminalCore) -> (Vec<String>, u16, u16) {
            let mut lines = Vec::with_capacity(core.rows() as usize);
            for r in 0..core.rows() {
                lines.push(core.get_line_text(r));
            }
            (lines, core.get_cursor_col(), core.get_cursor_row())
        }

        for ((cols_a, rows_a), (cols_b, rows_b), (replay_cols, replay_rows)) in [
            ((100u16, 32u16), (40u16, 24u16), (100u16, 32u16)),
            ((100, 32), (40, 24), (40, 24)),
            ((40, 24), (100, 32), (100, 32)),
            ((40, 24), (100, 32), (40, 24)),
        ] {
            let narrower_cols = cols_a.min(cols_b);
            let phase_a = synth_dims_phase_bytes(rows_a, narrower_cols, "A");
            let phase_b = synth_dims_phase_bytes(rows_b, narrower_cols, "B");

            let mut recording = phase_a.clone();
            recording.extend_from_slice(&phase_b);
            let segments = [
                ReplaySegment {
                    offset: 0,
                    cols: cols_a,
                    rows: rows_a,
                },
                ReplaySegment {
                    offset: phase_a.len() as u32,
                    cols: cols_b,
                    rows: rows_b,
                },
            ];

            // Live reference: a live parser resizes directly at the exact
            // points in the stream the segments represent, mirroring
            // exactly what `reset_and_replay_segments` does.
            let mut live = TerminalCore::new(replay_cols, replay_rows, 10_000);
            live.resize(cols_a, rows_a);
            live.process_pty_data_fully(&phase_a);
            live.resize(cols_b, rows_b);
            live.process_pty_data_fully(&phase_b);
            live.resize(replay_cols, replay_rows);

            let (snap, snap_segments) = crate::mux::snapshot_bytes::build_snapshot_bytes(
                &recording,
                &to_tuples(&segments),
                b"",
                false,
            );
            let mut replayed = TerminalCore::new(replay_cols, replay_rows, 10_000);
            replayed.reset_and_replay_segments(&snap, &to_replay_segments(&snap_segments));

            assert_eq!(
                fingerprint(&live),
                fingerprint(&replayed),
                "dims ({cols_a},{rows_a})->({cols_b},{rows_b}) replay@({replay_cols},{replay_rows}): \
                 replayed grid must match the live-fed reference"
            );
        }
    }
}
