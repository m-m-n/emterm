//! PTY spawning and reader loop for mux panes.

use std::borrow::Cow;
use std::io::Read;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    DetachReason, MuxPane, NotificationSender, PaneId, PaneOutputTarget, PtyOutputChunk,
    SharedNotificationSender, SharedOutputTarget, SharedPaneExitSender, SharedScrollback,
    SharedShadowParser, SharedTitleSender, TitleChangeSender, lock_shadow_parser,
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

/// Spawn a PTY with a shell process at the given size.
pub(super) fn spawn_pty(cols: u16, rows: u16) -> Result<SpawnedPty, String> {
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
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "emterm");
    cmd.env("EMTERM_MUX", "1");
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
/// Returns the new pane_id and its output target (for the reader thread).
#[allow(clippy::too_many_arguments)]
pub(super) fn register_pane_and_start_reader(
    mgr: &mut SessionManager,
    session_id: u32,
    window_id: u32,
    cols: u16,
    rows: u16,
    spawned: SpawnedPty,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    title_tx: &TitleChangeSender,
    notification_tx: &NotificationSender,
    pane_exit_sender: &SharedPaneExitSender,
) -> Option<PaneId> {
    // Verify session/window exist before allocating pane ID
    {
        let session = mgr.get_session(session_id)?;
        session.windows.get(&window_id)?;
    }

    let pane_id = mgr.alloc_pane_id();
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
    let raw_passthrough = pane.raw_passthrough.clone();
    let passthrough_scanner = pane.passthrough_scanner.clone();
    let scrollback = pane.scrollback.clone();
    // Store initial title_tx in the swappable sender (reattach will swap in a new one)
    *title_sender.lock().unwrap() = Some(title_tx.clone());
    // The notification channel lives for the daemon lifetime; populate it once.
    *notification_sender.lock().unwrap() = Some(notification_tx.clone());
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
    raw_passthrough: SharedRawPassthrough,
    passthrough_scanner: SharedPassthroughScanner,
    scrollback: SharedScrollback,
    pane_exit_sender: SharedPaneExitSender,
) {
    let mut buf = [0u8; 65536];
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
                        let _ = tx.blocking_send(PtyOutputChunk {
                            pane_id,
                            data: Vec::new(),
                        });
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
                    scrollback.lock().unwrap().write(to_write);
                }
                if let Some(new_title) = title_changed {
                    if let Some(tx) = title_sender.lock().unwrap().as_ref() {
                        let _ = tx.try_send((pane_id, new_title));
                    }
                }

                // Detect OSC 7 (cwd reporting) and cache the path
                if let Some(cwd) = crate::mux::ipc::statusbar::detect_osc7_cwd(data) {
                    *pane_cwd.lock().unwrap() = Some(cwd);
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
                            let chunk = PtyOutputChunk {
                                pane_id,
                                data: data.to_vec(),
                            };
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
