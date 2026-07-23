//! PTY spawning and reader loop for mux panes.

use std::borrow::Cow;
use std::io::Read;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::scrollback_filter::{scan_agent_status_reports, strip_replayable_rich_content};
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
/// emterm-md) BEFORE bytes land in the scrollback ring.
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
    /// [`SCROLLBACK_FILTER_PENDING_CAP`], the entire pending run is forwarded
    /// raw and `pending` is reset. This trades the strip guarantee for a
    /// bounded per-pane memory footprint — a wedged / adversarial stream
    /// cannot pin arbitrary bytes in the buffer.
    pub(in crate::mux) fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        if chunk.is_empty() {
            return Vec::new();
        }
        self.pending.extend_from_slice(chunk);

        if self.pending.len() > SCROLLBACK_FILTER_PENDING_CAP {
            log::warn!(
                "scrollback write filter: pending exceeded {} bytes, flushing raw",
                SCROLLBACK_FILTER_PENDING_CAP
            );
            return std::mem::take(&mut self.pending);
        }

        let boundary = find_safe_boundary(&self.pending);
        if boundary == 0 {
            return Vec::new();
        }
        let strippable: Vec<u8> = self.pending.drain(..boundary).collect();
        strip_replayable_rich_content(&strippable)
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
    pane_exit_sender: SharedPaneExitSender,
) {
    let mut buf = [0u8; 65536];
    // Per-pane stateful scrollback-write filter: strips viewer-launch rich
    // content across PTY read boundaries. See [`ScrollbackWriteFilter`] for
    // why a stateless per-chunk stripper is insufficient (128 KiB CLI chunks
    // straddle the 64 KiB PTY read buffer).
    let mut scrollback_filter = ScrollbackWriteFilter::new();
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
                        scrollback.lock().unwrap().write(&filtered);
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
                // parses this OSC itself for mux panes.
                for report in scan_agent_status_reports(data) {
                    if let Some(tx) = agent_status_report_sender.lock().unwrap().as_ref() {
                        let _ = tx.try_send((pane_id, report));
                    }
                }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::visibility::HIDDEN_PASSTHROUGH_CAPACITY_MUX;

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
}
