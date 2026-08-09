//! PTY spawning and reader loop for mux panes.

use std::borrow::Cow;
use std::io::Read;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use portable_pty::MasterPty;
use tokio::sync::mpsc;

use crate::mux::scrollback_filter::strip_pty_output_for_scrollback_write;
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    AgentStatusFeedItem, AgentStatusReportSender, DetachReason, MuxPane, NotificationSender,
    PaneId, PaneOutputTarget, PtyOutputChunk, SharedAgentStatusReportSender,
    SharedNotificationSender, SharedOutputTarget, SharedPaneExitSender, SharedScrollback,
    SharedShadowParser, SharedTitleSender, TitleChangeSender, lock_shadow_parser,
};
use crate::pty::passthrough_scanner::PassthroughScanner;
use crate::pty::visibility::RawPassthroughBuffer;

/// Shared per-pane raw passthrough buffer (image / Markdown OSC bytes
/// captured while detached or hidden). Drained into the resume snapshot.
type SharedRawPassthrough = Arc<StdMutex<RawPassthroughBuffer>>;

mod write_filter;

use write_filter::*;

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
    /// Shell child-process handle returned by `spawn_command` (task0001
    /// FR1). Carried through to `MuxPane` so it can be reaped on teardown
    /// instead of being discarded here, which previously left the process
    /// as an unreaped zombie once it exited.
    pub(super) child: Box<dyn portable_pty::Child + Send + Sync>,
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

    // task0001 FR1: retain the child handle instead of discarding it — an
    // unreaped exited child stays a zombie (`<defunct>`) in the kernel
    // process table forever, since neither `std::process::Child` nor
    // portable-pty's Unix implementation calls `wait()` on drop.
    let child = pair
        .slave
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
        child,
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
        Some(spawned.child),
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

#[allow(clippy::too_many_arguments)]
/// Widened from module-private to `pub(in crate::mux)` (task0003, mux daemon
/// hot-upgrade): the restore path (`mux::upgrade`) re-establishes a restored
/// live pane's reader thread through this SAME function — not a
/// reimplementation — so a restored pane's scrollback filtering, agent-status
/// forwarding, and title/cwd detection stay byte-for-byte identical to a
/// freshly spawned pane's (IMPLEMENTATION.md "Upgrade snapshot / restore"
/// postcondition: "writer and reader thread re-established"). No behavior
/// change for existing callers.
pub(in crate::mux) fn pty_reader_loop(
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
    // Per-pane stateful agent-status feed decoder: retains a partial OSC
    // body across PTY read boundaries so a report/mark split across reads
    // is still detected exactly once (SPEC FR1/FR3; review round-1 rework,
    // stable_id `osc_split_lost`), and — unlike a pair of independently
    // scanned lists — emits reports and OSC 133 marks from the SAME chunk
    // in true relative byte order (SPEC FR4). Marks are still gated to the
    // live, main-buffer spans of each chunk (SPEC FR5) via the
    // `live_spans` argument passed to `feed` below.
    let mut agent_status_feed_scanner = AgentStatusFeedScanner::new();
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
                // tears the pane/tab down.
                //
                // G1 fix (mux-window-switch-output-hang task0004 rework,
                // review round 3 finding `22251d51cc98261e`): clone the
                // sender and drop the `output_target` guard BEFORE the
                // blocking send, mirroring the data path's documented
                // discipline below ("IMPORTANT: release lock before
                // blocking_send to avoid deadlock"), which this EOF branch
                // previously did not follow. Holding the guard across
                // `blocking_send` let this reader thread park (channel
                // saturated) while still holding the pane's `output_target`
                // mutex; task0003 made that mutex reachable from the
                // connection task itself (`resume_pane_with_permit`,
                // `pane.rs`, called via the fair-permit path), which takes
                // the SAME std mutex synchronously (no `.await` yield
                // point) — so the connection task would block on
                // `lock()` until this reader thread's `blocking_send`
                // completed, which in turn could only complete once the
                // connection task's own drain arm ran, which it cannot
                // while blocked on that same `lock()`. A self-deadlock
                // across two different threads, same shape as the
                // documented data-path hazard.
                let connected_tx = {
                    let target = output_target.lock().unwrap();
                    match &*target {
                        PaneOutputTarget::Connected(tx) => Some(tx.clone()),
                        PaneOutputTarget::Detached { .. } => None,
                    }
                }; // output_target lock released here, before any send.
                if let Some(tx) = connected_tx {
                    let _ = tx.blocking_send(PtyOutputChunk::pty_output(pane_id, Vec::new()));
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
                let (main_bytes, scan_alt, main_spans) =
                    extract_main_buffer_bytes(data, alt_before);
                let (to_write, live_spans): (&[u8], Vec<std::ops::Range<usize>>) =
                    if scan_alt == alt_after {
                        (&main_bytes, main_spans)
                    } else {
                        // The scan ended in a different buffer than the
                        // authoritative shadow parser: a toggle straddled this
                        // read boundary or used an unrecognized form. Fall back to
                        // the conservative whole-chunk gate so we never emit a
                        // partial toggle sequence into scrollback.
                        if !alt_before && !alt_after {
                            (data, vec![0..data.len()])
                        } else {
                            (&[], Vec::new())
                        }
                    };
                if !to_write.is_empty() {
                    let (attribution_dims, filtered) =
                        scrollback_filter.feed(to_write, (read_cols, read_rows));
                    if !filtered.is_empty() {
                        scrollback.lock().unwrap().attribute_write(
                            attribution_dims.0,
                            attribution_dims.1,
                            &filtered,
                        );
                    }
                }
                if let Some(new_title) = title_changed {
                    if let Some(tx) = title_sender.lock().unwrap().as_ref() {
                        let _ = tx.try_send((pane_id, new_title));
                    }
                }

                // Detect agent-status OSC 777 reports (SPEC FR3) AND live
                // OSC 133 marks (task0003, SPEC FR1/FR5) in a SINGLE ordered
                // pass over the FULL chunk (`data`), then forward both to
                // the daemon-level agent-status task in that exact byte
                // order (SPEC FR4). Unlike OSC 9 notification scanning
                // (Detached-only, to avoid double-firing with the GUI's own
                // live parse), this runs regardless of attach state: the
                // daemon owns per-pane agent-status state unconditionally,
                // and the GUI never parses this OSC itself for mux panes.
                // The scanner is per-pane stateful (see
                // `agent_status_feed_scanner` above) so a report/mark split
                // across this read and the next is still detected exactly
                // once. Reports are unconditional — a report's validity does
                // not depend on screen content (unchanged from before
                // task0003) — while marks are only accepted when their
                // completing byte falls inside `live_spans`, this chunk's
                // LIVE, MAIN-BUFFER spans computed above: a mark produced
                // while the pane is on the alternate screen never reaches
                // the scanner (mirrors `term_core`'s own OSC 133 alt-screen
                // suppression), and only live PTY bytes (never
                // scrollback/snapshot/reattach-reconstructed bytes) ever do,
                // since `live_spans` is computed fresh from THIS read.
                let items = agent_status_feed_scanner.feed(data, &live_spans);
                if !items.is_empty() {
                    forward_agent_status_items(pane_id, items, &agent_status_report_sender);
                }

                // Detect OSC 7 (cwd reporting) and cache the path
                if let Some(cwd) = detect_osc7_cwd(data) {
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

/// Forward each decoded agent-status-relevant item (an OSC 777 report body
/// OR a live OSC 133 mark, task0003 SPEC FR4 — see [`AgentStatusFeedItem`])
/// to the daemon-level agent-status task via `agent_status_report_sender`,
/// IN THE ORDER GIVEN. Callers build `items` by appending this chunk's
/// reports and marks in their own already-correct relative scan order
/// (reports scanned from the full chunk, marks scanned from the live
/// main-buffer span of it) so a single sequential forward here — one
/// channel, one send per item, in order — is what gives FR4 its ordering
/// guarantee: no separate queue/task exists that could reorder a `Set`
/// relative to a `D`/`A` pair from the same PTY read.
///
/// Unlike the best-effort PTY-output passthrough, an accepted report MUST
/// reach the daemon — SPEC FR3 requires every accepted report to advance the
/// pane's revision, so silently dropping one on a full channel would be a
/// spec bug. A full channel therefore falls back to a blocking send instead
/// of dropping (review round-1 stable_id `try_send_drops_reports`, addressed
/// alongside the per-pane statefulness rework since the fix naturally
/// extends here). The same "must not silently drop" guarantee applies to
/// live OSC 133 marks: dropping one could leave the latch stuck armed
/// forever with no future mark able to complete the D→A transition it was
/// waiting on.
///
/// Runs OUTSIDE the `agent_status_report_sender` lock (the sender is cloned
/// out and the lock released before any send), mirroring the "release lock
/// before blocking_send" discipline the PTY-output backpressure path above
/// already follows — a blocked send here cannot deadlock against the
/// session-manager lock the consuming `run_agent_status_task` also needs.
fn forward_agent_status_items(
    pane_id: PaneId,
    items: Vec<AgentStatusFeedItem>,
    agent_status_report_sender: &SharedAgentStatusReportSender,
) {
    if items.is_empty() {
        return;
    }
    let sender = agent_status_report_sender.lock().unwrap().clone();
    let Some(tx) = sender else {
        return;
    };
    for item in items {
        match tx.try_send((pane_id, item)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(msg)) => {
                log::debug!(
                    "pane {} agent-status channel full; falling back to blocking send",
                    pane_id
                );
                if tx.blocking_send(msg).is_err() {
                    log::warn!(
                        "pane {} agent-status item not delivered: receiver dropped",
                        pane_id
                    );
                }
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                log::debug!(
                    "pane {} agent-status channel closed; item not delivered",
                    pane_id
                );
            }
        }
    }
}

/// Detect OSC 7 sequence in PTY output bytes and extract the CWD path.
///
/// OSC 7 format: ESC ] 7 ; file://hostname/path ST
/// ST can be: ESC \ (0x1B 0x5C) or BEL (0x07)
///
/// Returns the extracted path on success, or None.
///
/// Relocated from the (now-deleted) mux status-bar engine module
/// (mux-status-bar-removal task0001, IMPLEMENTATION.md D3): `Pane.cwd`
/// persists across daemon hot-upgrade and pane restoration, so this
/// detector belongs with its sole call site (the PTY reader loop above)
/// rather than a status-bar-only module. Behavior and unit tests are
/// unchanged by the move.
fn detect_osc7_cwd(data: &[u8]) -> Option<String> {
    // Look for ESC ] 7 ; pattern
    let pattern = b"\x1b]7;";
    let pos = data.windows(pattern.len()).position(|w| w == pattern)?;
    let start = pos + pattern.len();

    // Find the ST terminator (ESC \ or BEL)
    let rest = &data[start..];
    let end = rest.iter().enumerate().find_map(|(i, &b)| {
        if b == 0x07 {
            Some(i)
        } else if b == 0x1b && rest.get(i + 1) == Some(&0x5c) {
            Some(i)
        } else {
            None
        }
    })?;

    let uri = std::str::from_utf8(&rest[..end]).ok()?;

    // Strip file://hostname/ prefix to get the path
    if let Some(after_scheme) = uri.strip_prefix("file://") {
        // Find the first / after the hostname
        if let Some(slash_pos) = after_scheme.find('/') {
            let path = &after_scheme[slash_pos..];
            // URL-decode the path
            let decoded = url_decode(path);
            return Some(decoded);
        }
    }

    None
}

/// Simple percent-decoding for file paths (handles %XX sequences).
/// Collects decoded bytes into a Vec<u8> to correctly handle multi-byte UTF-8.
///
/// Relocated alongside [`detect_osc7_cwd`] (mux-status-bar-removal
/// task0001, IMPLEMENTATION.md D3).
fn url_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let hi = iter.next();
            let lo = iter.next();
            if let (Some(h), Some(l)) = (hi, lo) {
                let hex = [h, l];
                if let Ok(s) = std::str::from_utf8(&hex) {
                    if let Ok(val) = u8::from_str_radix(s, 16) {
                        bytes.push(val);
                        continue;
                    }
                }
                // Invalid hex - output as-is
                bytes.push(b'%');
                bytes.push(h);
                bytes.push(l);
            }
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests;
