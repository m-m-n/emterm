//! Reattach and detach logic for mux sessions.

use std::sync::Arc;

use futures::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    lock_shadow_parser, DetachReason, PaneId, PaneOutputTarget, PtyOutputChunk, SharedShadowParser,
    TitleChangeSender,
};

/// The clear-and-home prefix every snapshot starts with: `ESC[H ESC[2J`.
/// Homes the cursor and clears the screen so the client replays from a known
/// state before the scrollback / screen bytes arrive.
const SNAPSHOT_CLEAR_HOME: &[u8] = b"\x1b[H\x1b[2J";

/// Assemble the shared snapshot byte layout used by both the reattach path
/// and the on-demand `RequestPaneSnapshot` path:
///
/// ```text
/// ESC[H ESC[2J + scrollback + screen
/// ```
///
/// The scrollback bytes replay into the client's grid *before* the screen
/// snapshot overwrites the visible region with the final state, so the
/// client's `reset_and_replay` rebuilds the pane's history (FR1). Both paths
/// route through this one function so the byte ordering stays a single source
/// of truth. The reattach path appends its per-pane `raw_passthrough` bytes
/// after the screen on top of this layout.
pub(super) fn build_snapshot_bytes(scrollback: &[u8], screen: &[u8]) -> Vec<u8> {
    let mut combined =
        Vec::with_capacity(SNAPSHOT_CLEAR_HOME.len() + scrollback.len() + screen.len());
    combined.extend_from_slice(SNAPSHOT_CLEAR_HOME);
    combined.extend_from_slice(scrollback);
    combined.extend_from_slice(screen);
    combined
}

/// Build a self-contained ANSI byte sequence that reproduces the current
/// screen state tracked by the given shadow parser, optionally prefixed with
/// the pane's scrollback so the client can scroll to past output.
///
/// Output layout (shared with the reattach path via [`build_snapshot_bytes`]):
/// `ESC[H ESC[2J` + `scrollback` + `vt100::Screen::contents_formatted()`.
/// The first fragment clears the screen and homes the cursor so the client
/// starts from a known state; the scrollback fragment rebuilds history; the
/// final fragment replays the full screen including alt-screen toggle, SGR
/// attributes, cursor position, and cells.
///
/// `scrollback` is read by the caller WITHOUT clearing (the buffer lives for
/// the lifetime of the pane). An empty `scrollback` yields a valid
/// clear + shadow snapshot (history replays as empty).
///
/// Used by both the reattach path (combined with ring buffer delta) and the
/// on-demand `RequestPaneSnapshot` path.
pub(super) fn build_shadow_parser_snapshot(
    shadow_parser: &SharedShadowParser,
    scrollback: &[u8],
) -> Vec<u8> {
    let screen_data = {
        let parser = lock_shadow_parser(shadow_parser);
        parser.screen().contents_formatted()
    };
    build_snapshot_bytes(scrollback, &screen_data)
}

/// Collect reattach data for panes in the given session.
///
/// When `visible == true`, drains buffered output from detached panes and
/// switches each pane to `Connected(pane_output_tx)`. Each returned tuple
/// carries the pane id and the resume snapshot bytes (shadow + ring +
/// raw_passthrough).
///
/// When `visible == false` (FR13: hidden reattach), the panes are NOT
/// flipped to `Connected`. Instead each pane is set / kept in
/// `Detached { reason = HiddenByVisibility, owner = Some(pane_output_tx) }`
/// so the reader thread continues to accumulate ring + raw_passthrough
/// bytes. The returned tuples carry empty buffers, which `send_reattach_data`
/// emits as bare `PaneCreated` frames (no `PtyOutput`). The next
/// `SetVisibility(true)` from this connection then triggers the resume
/// snapshot via `resume_pane_with_permit`.
///
/// In both modes, the session's `active_client_kick` is swapped to the
/// caller's sender. Any previously registered kick sender is fired (after
/// releasing the session lock) so the prior attached client is signalled
/// to detach.
pub(super) async fn collect_reattach_data(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    title_tx: &TitleChangeSender,
    new_kick: oneshot::Sender<()>,
    visible: bool,
) -> Vec<(PaneId, Vec<u8>)> {
    let mut new_kick_opt = Some(new_kick);
    let mut old_kick: Option<oneshot::Sender<()>> = None;
    let mut data: Vec<(PaneId, Vec<u8>)> = Vec::new();
    {
        let mut mgr = session_manager.lock().await;
        if let Some(session) = mgr.get_session_mut(session_id) {
            old_kick = session.active_client_kick.take();
            session.active_client_kick = new_kick_opt.take();
            for window in session.windows.values() {
                for pane in window.panes.values() {
                    if pane.exited {
                        continue;
                    }

                    // Swap in new title sender so reader threads notify the new connection
                    *pane.title_sender.lock().unwrap() = Some(title_tx.clone());

                    if !visible {
                        // FR13 hidden reattach: keep the pane Detached so the
                        // reader keeps filling scrollback + raw_passthrough.
                        // Adopt the caller as `owner` and set the reason to
                        // HiddenByVisibility so a subsequent
                        // SetVisibility(true) from this connection can
                        // resume it via `resume_pane_with_permit`. Existing
                        // scrollback / raw_passthrough contents are preserved.
                        let mut target = pane.output_target.lock().unwrap();
                        match &mut *target {
                            PaneOutputTarget::Connected(_) => {
                                *target = PaneOutputTarget::Detached {
                                    reason: DetachReason::HiddenByVisibility,
                                    owner: Some(pane_output_tx.clone()),
                                };
                            }
                            PaneOutputTarget::Detached { reason, owner } => {
                                *reason = DetachReason::combine(
                                    *reason,
                                    DetachReason::HiddenByVisibility,
                                );
                                // The NetworkDetach bit is cleared on
                                // reattach because the caller is now the
                                // owning client. Hidden bit stays.
                                if let Some(without_network) = reason.clear_network() {
                                    *reason = without_network;
                                } else {
                                    *reason = DetachReason::HiddenByVisibility;
                                }
                                *owner = Some(pane_output_tx.clone());
                            }
                        }
                        log::info!(
                            "collect_reattach: pane {} hidden reattach, kept Detached (snapshot deferred)",
                            pane.id
                        );
                        data.push((pane.id, Vec::new()));
                        continue;
                    }

                    // Visible reattach: build the FR5-ordered resume snapshot
                    // and switch to Connected. Order is
                    //   ESC[H ESC[2J + scrollback + shadow + passthrough
                    // so the scrollback bytes replay into the client's WASM
                    // grid (populating its history) before the shadow snapshot
                    // overwrites the visible screen with a known good final
                    // state. Scrollback is read WITHOUT clearing (FR6: the
                    // buffer lives for the lifetime of the pane).
                    let (screen_data, is_alternate_screen) = {
                        let parser = lock_shadow_parser(&pane.shadow_parser);
                        let screen = parser.screen();
                        (screen.contents_formatted(), screen.alternate_screen())
                    };
                    let scrollback_data = pane.scrollback.lock().unwrap().read_all();

                    let mut target = pane.output_target.lock().unwrap();
                    let target_was = match &*target {
                        PaneOutputTarget::Connected(_) => "Connected",
                        PaneOutputTarget::Detached { .. } => "Detached",
                    };
                    *target = PaneOutputTarget::Connected(pane_output_tx.clone());
                    drop(target);

                    // Drain the per-pane raw passthrough buffer so image /
                    // Markdown OSC byte runs captured while detached are
                    // replayed as part of the resume snapshot. Cleared
                    // unconditionally so the next detach cycle starts fresh.
                    let passthrough_data = {
                        let mut buf = pane.raw_passthrough.lock().unwrap();
                        let bytes = buf.read_all();
                        buf.clear();
                        bytes
                    };

                    log::info!(
                        "collect_reattach: pane {} was={}, scrollback={}B, screen={}B, passthrough={}B, total={}B, alt_screen={}, exited={}",
                        pane.id,
                        target_was,
                        scrollback_data.len(),
                        screen_data.len(),
                        passthrough_data.len(),
                        8 + scrollback_data.len() + screen_data.len() + passthrough_data.len(),
                        is_alternate_screen,
                        pane.exited
                    );

                    // Shared layout: ESC[H ESC[2J + scrollback + screen.
                    // The reattach path appends its per-pane raw_passthrough
                    // after the screen on top of that shared base.
                    let mut combined = build_snapshot_bytes(&scrollback_data, &screen_data);
                    combined.extend_from_slice(&passthrough_data);

                    data.push((pane.id, combined));
                }
            }
        }
        // If session is not found, new_kick_opt is dropped here (nothing to kick).
    }
    if let Some(old) = old_kick {
        // Notify the previously attached client to detach. Err means the
        // receiver was already dropped (client gone) — harmless.
        let _ = old.send(());
    }
    data
}

/// Maximum payload bytes per `PtyOutput` frame emitted during reattach replay.
///
/// A pane's ring buffer can hold up to `DEFAULT_SCROLLBACK_CAPACITY` but a
/// single codec frame must stay under `MAX_FRAME_LENGTH` (16 MiB). Chosen well
/// below the codec cap so the 5-byte frame-body header plus any future growth
/// stays safely within bounds.
const REATTACH_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// Send reattach data (PaneCreated + buffered output) to the client.
///
/// Large per-pane buffers are split into multiple `PtyOutput` frames so each
/// frame fits under `MAX_FRAME_LENGTH`. Without this split, a 34 MiB ring
/// buffer (e.g. a long-detached `glances` pane) produces a single oversized
/// frame, the codec encode fails, the socket tears down, and the bridge
/// synthesises a Detached that drops the GUI out of mux mode mid-reattach.
pub(super) async fn send_reattach_data<S>(
    framed: &mut Framed<S, MuxCodec>,
    reattach_data: &[(PaneId, Vec<u8>)],
) -> Result<(), ()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    for (pane_id, buffered) in reattach_data {
        let resp = MuxMessage::control(MessageType::PaneCreated, *pane_id, pane_id);
        if framed.send(resp).await.is_err() {
            return Err(());
        }
        for chunk in buffered.chunks(REATTACH_CHUNK_SIZE) {
            let msg = MuxMessage::pty_output(*pane_id, chunk.to_vec());
            if framed.send(msg).await.is_err() {
                return Err(());
            }
        }
    }
    Ok(())
}

/// Switch panes in a session to detached buffering mode — identity-scoped.
///
/// Only panes whose current `Connected(tx)` matches the caller's
/// `owned_tx` (compared via `Sender::same_channel`) are flipped to
/// `Detached`. Panes already owned by a different connection (e.g., a
/// newer client that has taken over the session via `collect_reattach_data`)
/// are left untouched.
///
/// This makes the cleanup safe against races where:
/// - The `kick_fut` arm and `framed.next()` arm of the connection's select!
///   loop both become ready simultaneously and biased scheduling picks
///   `framed.next()`; the loop exits with `was_kicked == false` and reaches
///   this function, but the panes are already owned by the new client.
/// - `handle_attach` detaches the old session while switching sessions;
///   if another connection has concurrently taken the old session over,
///   that connection's `Connected(tx)` is preserved.
pub(in crate::mux) async fn detach_session_panes(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
) {
    let mgr = session_manager.lock().await;
    if let Some(session) = mgr.get_session(session_id) {
        for window in session.windows.values() {
            for pane in window.panes.values() {
                if pane.exited {
                    log::info!(
                        "detach_session_panes: pane {} already exited, skipping",
                        pane.id
                    );
                    continue;
                }
                let mut target = pane.output_target.lock().unwrap();
                let was = match &*target {
                    PaneOutputTarget::Connected(_) => "Connected",
                    PaneOutputTarget::Detached { .. } => "Detached",
                };
                let owned_by_caller = match &*target {
                    PaneOutputTarget::Connected(tx) => tx.same_channel(owned_tx),
                    PaneOutputTarget::Detached { .. } => false,
                };
                if owned_by_caller {
                    *target = PaneOutputTarget::Detached {
                        reason: DetachReason::NetworkDetach,
                        owner: None,
                    };
                    log::info!(
                        "detach_session_panes: pane {} switched {} -> Detached(NetworkDetach)",
                        pane.id,
                        was
                    );
                } else if matches!(&*target, PaneOutputTarget::Connected(_)) {
                    log::info!(
                        "detach_session_panes: pane {} Connected to other client, preserving",
                        pane.id
                    );
                } else {
                    log::info!(
                        "detach_session_panes: pane {} already {}, no change",
                        pane.id,
                        was
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::session::pane::{
        new_shadow_parser, MuxPane, PaneOutputTarget, SharedOutputTarget,
    };
    use std::sync::Mutex as StdMutex;

    fn make_test_pane_with_target(id: u32, output_target: SharedOutputTarget) -> MuxPane {
        MuxPane::new_test(id, 80, 24, output_target)
    }

    // ── TS-4 / TS-6: on-demand snapshot builder (FR1) ────────────────────

    /// TS-4: the on-demand snapshot builder emits scrollback BEFORE the
    /// shadow screen, matching the reattach construction (clear + scrollback
    /// + shadow). Asserted by byte-offset ordering.
    #[test]
    fn build_shadow_parser_snapshot_emits_scrollback_before_screen() {
        let parser: SharedShadowParser = Arc::new(StdMutex::new(new_shadow_parser(24, 80)));
        parser.lock().unwrap().process(b"SCREEN-CONTENT");
        let scrollback = b"HISTORY-LINE-ONE";

        let snapshot = build_shadow_parser_snapshot(&parser, scrollback);

        // Leading clear-and-home.
        assert!(
            snapshot.starts_with(b"\x1b[H\x1b[2J"),
            "snapshot must start with ESC[H ESC[2J"
        );
        let find = |needle: &[u8]| {
            snapshot
                .windows(needle.len())
                .position(|w| w == needle)
                .unwrap_or_else(|| panic!("needle {:?} not found in snapshot", needle))
        };
        let scrollback_at = find(b"HISTORY-LINE-ONE");
        let screen_at = find(b"SCREEN-CONTENT");
        assert!(
            scrollback_at < screen_at,
            "scrollback ({scrollback_at}) must precede the shadow screen ({screen_at})"
        );
        // And the scrollback must come after the clear prefix.
        assert!(scrollback_at >= b"\x1b[H\x1b[2J".len());
    }

    /// TS-6: an empty scrollback yields a valid clear + shadow snapshot
    /// (no panic, history replays empty).
    #[test]
    fn build_shadow_parser_snapshot_empty_scrollback_is_clear_plus_shadow() {
        let parser: SharedShadowParser = Arc::new(StdMutex::new(new_shadow_parser(24, 80)));
        parser.lock().unwrap().process(b"ONLY-SCREEN");

        let snapshot = build_shadow_parser_snapshot(&parser, b"");

        assert!(snapshot.starts_with(b"\x1b[H\x1b[2J"));
        assert!(
            snapshot
                .windows(b"ONLY-SCREEN".len())
                .any(|w| w == b"ONLY-SCREEN"),
            "shadow screen must still be present with empty scrollback"
        );
    }

    /// The shared layout helper places scrollback between the clear prefix
    /// and the screen, and an empty screen / empty scrollback both produce a
    /// well-formed buffer (the on-demand + reattach paths share this base).
    #[test]
    fn build_snapshot_bytes_layout_is_clear_scrollback_screen() {
        let out = build_snapshot_bytes(b"SB", b"SC");
        assert_eq!(out, b"\x1b[H\x1b[2JSBSC");
        // Empty inputs: just the clear prefix.
        assert_eq!(build_snapshot_bytes(b"", b""), b"\x1b[H\x1b[2J");
    }

    /// Test: collect_reattach_data returns entries for 2 panes in 2 windows.
    /// Simulates the reattach scenario where both panes are in Connected(dead_tx) state.
    #[tokio::test]
    async fn test_collect_reattach_data_two_windows_connected_dead() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        // Set up: 1 session, 2 windows, each with 1 pane (Connected to dead channels)
        let (dead_tx1, _dead_rx1) = mpsc::channel::<PtyOutputChunk>(1);
        let (dead_tx2, _dead_rx2) = mpsc::channel::<PtyOutputChunk>(1);

        // Drop receivers to simulate dead channels
        drop(_dead_rx1);
        drop(_dead_rx2);

        let target1: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx1)));
        let target2: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx2)));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let w1 = m.create_window(session_id, "shell".to_string()).unwrap();
            let w2 = m.create_window(session_id, "shell".to_string()).unwrap();

            let pane1 = make_test_pane_with_target(1, target1);
            let pane2 = make_test_pane_with_target(2, target2);

            let session = m.get_session_mut(session_id).unwrap();
            session.windows.get_mut(&w1).unwrap().add_pane(pane1);
            session.windows.get_mut(&w2).unwrap().add_pane(pane2);
        }

        // Verify session has 2 panes
        {
            let m = mgr.lock().await;
            let session = m.get_session(session_id).unwrap();
            assert_eq!(session.pane_count(), 2, "Session should have 2 panes");
            assert_eq!(session.window_count(), 2, "Session should have 2 windows");
        }

        // Create new channel for reattach
        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);

        // Call collect_reattach_data
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
        let (kick_tx, _kick_rx) = oneshot::channel::<()>();
        let data = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx, true).await;

        // CRITICAL: Must return 2 entries
        assert_eq!(
            data.len(),
            2,
            "collect_reattach_data should return 2 entries for 2 panes"
        );

        // Verify pane IDs
        let mut pane_ids: Vec<u32> = data.iter().map(|(id, _)| *id).collect();
        pane_ids.sort();
        assert_eq!(pane_ids, vec![1, 2], "Should contain pane IDs 1 and 2");

        // Verify all buffers start with the reset sequence (screen restoration)
        for (_, buf) in &data {
            assert!(
                !buf.is_empty(),
                "Reattach data should include screen restoration"
            );
            assert!(
                buf.starts_with(b"\x1b[H\x1b[2J"),
                "Reattach data should start with reset sequence"
            );
        }
    }

    /// Test: collect_reattach_data returns entries for panes in Detached state.
    #[tokio::test]
    async fn test_collect_reattach_data_two_windows_detached() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let target1: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let target2: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let w1 = m.create_window(session_id, "shell".to_string()).unwrap();
            let w2 = m.create_window(session_id, "shell".to_string()).unwrap();

            let pane1 = make_test_pane_with_target(1, target1);
            pane1.scrollback.lock().unwrap().write(b"hello from pane 1");
            let pane2 = make_test_pane_with_target(2, target2);
            pane2.scrollback.lock().unwrap().write(b"hello from pane 2");

            let session = m.get_session_mut(session_id).unwrap();
            session.windows.get_mut(&w1).unwrap().add_pane(pane1);
            session.windows.get_mut(&w2).unwrap().add_pane(pane2);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
        let (kick_tx, _kick_rx) = oneshot::channel::<()>();
        let data = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx, true).await;

        assert_eq!(
            data.len(),
            2,
            "collect_reattach_data should return 2 entries"
        );

        // Verify both have buffered data
        for (_, buf) in &data {
            assert!(!buf.is_empty(), "Detached panes should have buffered data");
        }
    }

    /// Test: collect_reattach_data skips exited panes.
    #[tokio::test]
    async fn test_collect_reattach_data_skips_exited() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let (dead_tx, _) = mpsc::channel::<PtyOutputChunk>(1);
        let target1: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx.clone())));
        let target2: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx)));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let w1 = m.create_window(session_id, "shell".to_string()).unwrap();
            let w2 = m.create_window(session_id, "shell".to_string()).unwrap();

            let pane1 = make_test_pane_with_target(1, target1);
            let mut pane2 = make_test_pane_with_target(2, target2);
            pane2.mark_exited(); // Mark pane 2 as exited

            let session = m.get_session_mut(session_id).unwrap();
            session.windows.get_mut(&w1).unwrap().add_pane(pane1);
            session.windows.get_mut(&w2).unwrap().add_pane(pane2);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
        let (kick_tx, _kick_rx) = oneshot::channel::<()>();
        let data = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx, true).await;

        assert_eq!(
            data.len(),
            1,
            "Should only return 1 entry (pane 2 is exited)"
        );
        assert_eq!(data[0].0, 1, "Only pane 1 should be included");
    }

    /// TS-12: detach_session_panes must preserve the pane's title_sender
    /// so the daemon-level title task keeps receiving OSC title updates
    /// while no GUI is attached.
    #[tokio::test]
    async fn test_detach_session_panes_preserves_title_sender() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));
        let (pane_out_tx, _pane_out_rx) = mpsc::channel::<PtyOutputChunk>(16);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);

        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(
            pane_out_tx.clone(),
        )));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let wid = m.create_window(session_id, "shell".to_string()).unwrap();
            let pane = make_test_pane_with_target(1, target);
            *pane.title_sender.lock().unwrap() = Some(title_tx.clone());
            m.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
        }

        detach_session_panes(&mgr, session_id, &pane_out_tx).await;

        let m = mgr.lock().await;
        let session = m.get_session(session_id).unwrap();
        let pane = session
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .values()
            .next()
            .unwrap();
        assert!(
            pane.title_sender.lock().unwrap().is_some(),
            "detach must not clear title_sender (daemon-level tx stays alive)"
        );
        assert!(matches!(
            *pane.output_target.lock().unwrap(),
            PaneOutputTarget::Detached { .. }
        ));
    }

    /// Test: a second reattach fires the kick sender installed by the first
    /// reattach, signalling the prior client to detach.
    #[tokio::test]
    async fn test_collect_reattach_data_fires_old_kick() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let wid = m.create_window(session_id, "shell".to_string()).unwrap();
            let pane = make_test_pane_with_target(1, target);
            m.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
        }

        let (tx1, _rx1) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);

        // First client attaches: installs kick1.
        let (kick_tx1, mut kick_rx1) = oneshot::channel::<()>();
        let _ = collect_reattach_data(&mgr, session_id, &tx1, &title_tx, kick_tx1, true).await;

        // Receiver must still be pending (no kick yet).
        assert!(
            kick_rx1.try_recv().is_err(),
            "first client should not be kicked before second attach"
        );

        // Second client attaches: should fire kick1 and install kick2.
        let (tx2, _rx2) = mpsc::channel::<PtyOutputChunk>(256);
        let (kick_tx2, mut kick_rx2) = oneshot::channel::<()>();
        let _ = collect_reattach_data(&mgr, session_id, &tx2, &title_tx, kick_tx2, true).await;

        // First client's kick_rx must now resolve with Ok(()).
        assert_eq!(
            kick_rx1.try_recv(),
            Ok(()),
            "second attach must fire first client's kick with Ok(())"
        );

        // Second client's kick_rx must still be pending.
        assert!(
            kick_rx2.try_recv().is_err(),
            "second client should not be kicked yet"
        );

        // Session must hold the second kick sender.
        let m = mgr.lock().await;
        assert!(
            m.get_session(session_id)
                .unwrap()
                .active_client_kick
                .is_some(),
            "session must hold the second kick sender"
        );
    }

    /// Regression: detach_session_panes must leave panes whose Connected(tx)
    /// belongs to another connection alone. This guards against the race
    /// where a kicked client's cleanup path would otherwise clobber the new
    /// client's freshly-attached panes.
    #[tokio::test]
    async fn test_detach_session_panes_identity_scoped() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        // Client A owns the pane (Connected(tx_a)).
        let (tx_a, _rx_a) = mpsc::channel::<PtyOutputChunk>(16);
        // Client B is a different connection (different channel).
        let (tx_b, _rx_b) = mpsc::channel::<PtyOutputChunk>(16);

        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx_a.clone())));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let wid = m.create_window(session_id, "shell".to_string()).unwrap();
            let pane = make_test_pane_with_target(1, target);
            m.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
        }

        // Client B calls detach with its own tx. Must NOT detach pane owned by A.
        detach_session_panes(&mgr, session_id, &tx_b).await;
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .values()
                .next()
                .unwrap();
            assert!(
                matches!(
                    *pane.output_target.lock().unwrap(),
                    PaneOutputTarget::Connected(_)
                ),
                "detach_session_panes must preserve pane owned by a different connection"
            );
        }

        // Now Client A calls detach with its own tx. MUST detach.
        detach_session_panes(&mgr, session_id, &tx_a).await;
        {
            let m = mgr.lock().await;
            let pane = m
                .get_session(session_id)
                .unwrap()
                .windows
                .values()
                .next()
                .unwrap()
                .panes
                .values()
                .next()
                .unwrap();
            assert!(
                matches!(
                    *pane.output_target.lock().unwrap(),
                    PaneOutputTarget::Detached { .. }
                ),
                "detach_session_panes must detach pane owned by caller"
            );
        }
    }

    /// Test: first attach with no prior client simply installs the kick.
    #[tokio::test]
    async fn test_collect_reattach_data_first_attach_no_old_kick() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let wid = m.create_window(session_id, "shell".to_string()).unwrap();
            let pane = make_test_pane_with_target(1, target);
            m.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
        let (kick_tx, mut kick_rx) = oneshot::channel::<()>();
        let _ = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx, true).await;

        assert!(
            kick_rx.try_recv().is_err(),
            "no prior client, so new kick must remain pending"
        );
        let m = mgr.lock().await;
        assert!(
            m.get_session(session_id)
                .unwrap()
                .active_client_kick
                .is_some(),
            "session must hold the newly-installed kick sender"
        );
    }

    /// Regression test: a pane whose ring buffer exceeds `MAX_FRAME_LENGTH` must
    /// be sent as multiple `PtyOutput` frames. A single oversized frame would
    /// make the codec encoder fail, tearing down the reattach connection.
    #[tokio::test]
    async fn test_send_reattach_data_splits_large_buffer() {
        use futures::StreamExt;
        use tokio_util::codec::Framed;

        let (client, server) = tokio::io::duplex(64 * 1024 * 1024);
        let mut server_framed = Framed::new(server, MuxCodec::new());
        let client_framed = Framed::new(client, MuxCodec::new());

        // Payload that spans just over two full chunks.
        let payload_len = REATTACH_CHUNK_SIZE * 2 + 123;
        let big: Vec<u8> = (0..payload_len).map(|i| (i % 251) as u8).collect();
        let reattach_data = vec![(42u32, big.clone())];

        // Sender task.
        let sender = tokio::spawn(async move {
            let mut framed = client_framed;
            send_reattach_data(&mut framed, &reattach_data).await
        });

        // First frame: PaneCreated.
        let first = server_framed.next().await.unwrap().unwrap();
        assert_eq!(first.msg_type, MessageType::PaneCreated);
        assert_eq!(first.pane_id, 42);

        // Subsequent frames: PtyOutput chunks, each <= REATTACH_CHUNK_SIZE.
        // Concatenated payload must reproduce the original big buffer byte-for-byte.
        let mut reassembled = Vec::with_capacity(payload_len);
        let mut chunks_seen = 0;
        while reassembled.len() < payload_len {
            let frame = server_framed.next().await.unwrap().unwrap();
            assert_eq!(frame.msg_type, MessageType::PtyOutput);
            assert_eq!(frame.pane_id, 42);
            assert!(
                frame.payload.len() <= REATTACH_CHUNK_SIZE,
                "chunk {} len {} exceeded REATTACH_CHUNK_SIZE {}",
                chunks_seen,
                frame.payload.len(),
                REATTACH_CHUNK_SIZE
            );
            reassembled.extend_from_slice(&frame.payload);
            chunks_seen += 1;
        }
        assert_eq!(chunks_seen, 3, "expected 3 chunks (2 full + 1 partial)");
        assert_eq!(reassembled, big, "reassembled payload must match input");

        sender.await.unwrap().expect("send_reattach_data ok");
    }

    /// Empty buffer emits PaneCreated and nothing else.
    #[tokio::test]
    async fn test_send_reattach_data_empty_buffer_emits_only_pane_created() {
        use futures::StreamExt;
        use tokio_util::codec::Framed;

        let (client, server) = tokio::io::duplex(64 * 1024);
        let mut server_framed = Framed::new(server, MuxCodec::new());
        let client_framed = Framed::new(client, MuxCodec::new());

        let reattach_data = vec![(7u32, Vec::<u8>::new())];
        let sender = tokio::spawn(async move {
            let mut framed = client_framed;
            send_reattach_data(&mut framed, &reattach_data).await
        });

        let first = server_framed.next().await.unwrap().unwrap();
        assert_eq!(first.msg_type, MessageType::PaneCreated);
        assert_eq!(first.pane_id, 7);

        sender.await.unwrap().expect("send_reattach_data ok");

        // No further frames expected — drop the sender so the stream closes.
        let next =
            tokio::time::timeout(std::time::Duration::from_millis(50), server_framed.next()).await;
        match next {
            Ok(None) | Err(_) => {} // stream closed or timed out: both OK
            Ok(Some(Ok(frame))) => panic!("unexpected extra frame: {:?}", frame.msg_type),
            Ok(Some(Err(e))) => panic!("unexpected stream error: {}", e),
        }
    }

    /// Test: session_list reports correct pane_count for multi-window session.
    #[tokio::test]
    async fn test_session_list_pane_count_multi_window() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        {
            let mut m = mgr.lock().await;
            let session_id = m.create_session("default".to_string());
            let w1 = m.create_window(session_id, "shell".to_string()).unwrap();
            let w2 = m.create_window(session_id, "shell".to_string()).unwrap();

            let (tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
            let t1: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx.clone())));
            let t2: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));

            let pane1 = make_test_pane_with_target(1, t1);
            let pane2 = make_test_pane_with_target(2, t2);

            let session = m.get_session_mut(session_id).unwrap();
            session.windows.get_mut(&w1).unwrap().add_pane(pane1);
            session.windows.get_mut(&w2).unwrap().add_pane(pane2);

            let list = m.session_list();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].pane_count, 2, "Session should report 2 panes");
            assert_eq!(list[0].window_count, 2, "Session should report 2 windows");
        }
    }

    /// TS-20: collect_reattach_data must concatenate the per-pane
    /// `raw_passthrough` buffer onto the resume snapshot AND clear the
    /// buffer after consumption. Without this, image / Markdown OSC
    /// sequences captured while the pane was Detached would be lost on
    /// reattach (and would also leak across the next detach cycle).
    #[tokio::test]
    async fn test_collect_reattach_data_includes_raw_passthrough_and_clears_it() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let wid = m.create_window(session_id, "shell".to_string()).unwrap();
            let pane = make_test_pane_with_target(1, target);
            // Seed scrollback + shadow + raw_passthrough as the reader would.
            pane.scrollback.lock().unwrap().write(b"buffered-from-ring");
            pane.shadow_parser
                .lock()
                .unwrap()
                .process(b"shadow-content");
            pane.raw_passthrough
                .lock()
                .unwrap()
                .append(b"\x1b_Gi=42;PNG-bytes\x1b\\");
            m.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
        let (kick_tx, _kick_rx) = oneshot::channel::<()>();
        let data = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx, true).await;

        assert_eq!(data.len(), 1, "expected 1 entry");
        let (pane_id, snapshot) = &data[0];
        assert_eq!(*pane_id, 1);
        assert!(snapshot.starts_with(b"\x1b[H\x1b[2J"));
        let needle = b"\x1b_Gi=42;PNG-bytes\x1b\\";
        assert!(
            snapshot.windows(needle.len()).any(|w| w == needle),
            "reattach snapshot must include captured passthrough sequence"
        );
        // The ring data must still be present.
        assert!(
            snapshot
                .windows(b"buffered-from-ring".len())
                .any(|w| w == b"buffered-from-ring"),
            "reattach snapshot must include ring data"
        );

        // raw_passthrough must be drained — otherwise the next detach cycle
        // would re-emit the same bytes.
        let m = mgr.lock().await;
        let session = m.get_session(session_id).unwrap();
        let pane = session
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .values()
            .next()
            .unwrap();
        assert_eq!(
            pane.raw_passthrough.lock().unwrap().len(),
            0,
            "raw_passthrough must be cleared after collect_reattach_data"
        );
    }

    /// F4: hidden reattach must NOT switch panes to Connected and must NOT
    /// drain ring/raw_passthrough. The returned tuples carry empty bytes so
    /// `send_reattach_data` emits only `PaneCreated` (no `PtyOutput`).
    #[tokio::test]
    async fn test_collect_reattach_data_hidden_keeps_detached_and_skips_snapshot() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));

        let session_id;
        let pane_scrollback;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let wid = m.create_window(session_id, "shell".to_string()).unwrap();
            let pane = make_test_pane_with_target(1, target.clone());
            pane.scrollback.lock().unwrap().write(b"buffered-from-ring");
            pane.shadow_parser.lock().unwrap().process(b"shadow-state");
            pane.raw_passthrough
                .lock()
                .unwrap()
                .append(b"\x1b_Gi=99;ZZ\x1b\\");
            pane_scrollback = pane.scrollback.clone();
            m.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
        let (kick_tx, _kick_rx) = oneshot::channel::<()>();
        let data =
            collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx, false).await;

        assert_eq!(data.len(), 1, "one entry for the live pane");
        let (pane_id, snapshot) = &data[0];
        assert_eq!(*pane_id, 1);
        assert!(
            snapshot.is_empty(),
            "hidden reattach must defer the snapshot (got {}B)",
            snapshot.len()
        );

        // Pane stayed Detached, owner adopted, reason is HiddenByVisibility.
        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, owner, .. } => {
                assert_eq!(*reason, DetachReason::HiddenByVisibility);
                let owner = owner.as_ref().expect("owner must be set to caller");
                assert!(owner.same_channel(&new_tx));
            }
            _ => panic!("hidden reattach must keep pane Detached"),
        }
        // Scrollback + raw_passthrough must be preserved (NOT drained).
        let buf = pane_scrollback.lock().unwrap().read_all();
        assert!(
            buf.windows(b"buffered-from-ring".len())
                .any(|w| w == b"buffered-from-ring"),
            "scrollback must still hold buffered bytes"
        );
        let m = mgr.lock().await;
        let pane = m
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .values()
            .next()
            .unwrap();
        assert!(
            matches!(
                &*pane.output_target.lock().unwrap(),
                PaneOutputTarget::Detached { .. }
            ),
            "expected Detached after hidden reattach"
        );
        assert!(
            !pane.raw_passthrough.lock().unwrap().is_empty(),
            "raw_passthrough must NOT be cleared on hidden reattach"
        );
    }

    /// F4: a previously Connected pane (e.g. owned by an earlier client
    /// that did not detach cleanly) must still drop into Detached on a
    /// hidden reattach.
    #[tokio::test]
    async fn test_collect_reattach_data_hidden_demotes_connected_to_detached() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let (dead_tx, _dead_rx) = mpsc::channel::<PtyOutputChunk>(1);
        let target: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(dead_tx)));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let wid = m.create_window(session_id, "shell".to_string()).unwrap();
            let pane = make_test_pane_with_target(1, target.clone());
            m.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
        let (kick_tx, _kick_rx) = oneshot::channel::<()>();
        let data =
            collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx, false).await;
        assert_eq!(data.len(), 1);
        assert!(data[0].1.is_empty());

        match &*target.lock().unwrap() {
            PaneOutputTarget::Detached { reason, owner, .. } => {
                assert_eq!(*reason, DetachReason::HiddenByVisibility);
                let owner = owner.as_ref().expect("owner must be set");
                assert!(owner.same_channel(&new_tx));
            }
            _ => panic!("expected Detached"),
        }
    }

    /// F4: visible reattach after a hidden reattach: SetVisibility(true)
    /// in the meantime would be the production trigger, but at the
    /// `collect_reattach_data` level the simpler invariant is that
    /// `visible=true` continues to flip the pane to Connected and drain.
    /// Already covered by the existing
    /// `test_collect_reattach_data_two_windows_detached` etc., so we add
    /// a focused round-trip: hidden reattach then visible reattach must
    /// produce exactly one non-empty snapshot.
    #[tokio::test]
    async fn test_collect_reattach_data_hidden_then_visible_round_trip() {
        let mgr = Arc::new(Mutex::new(SessionManager::new()));

        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));

        let session_id;
        {
            let mut m = mgr.lock().await;
            session_id = m.create_session("default".to_string());
            let wid = m.create_window(session_id, "shell".to_string()).unwrap();
            let pane = make_test_pane_with_target(1, target.clone());
            pane.scrollback.lock().unwrap().write(b"ring-bytes");
            pane.shadow_parser.lock().unwrap().process(b"shadow-x");
            m.get_session_mut(session_id)
                .unwrap()
                .windows
                .get_mut(&wid)
                .unwrap()
                .add_pane(pane);
        }

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);

        // Hidden reattach: empty payload, pane stays Detached.
        let (kick_tx, _kick_rx) = oneshot::channel::<()>();
        let data1 =
            collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx, false).await;
        assert!(data1[0].1.is_empty());

        // Visible reattach immediately after: pane flips Connected, full snapshot returned.
        let (kick_tx2, _kick_rx2) = oneshot::channel::<()>();
        let data2 =
            collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx2, true).await;
        assert_eq!(data2.len(), 1);
        let (_pid, snapshot) = &data2[0];
        assert!(snapshot.starts_with(b"\x1b[H\x1b[2J"));
        assert!(
            snapshot
                .windows(b"ring-bytes".len())
                .any(|w| w == b"ring-bytes"),
            "visible reattach must include ring data captured during hidden window"
        );
        assert!(matches!(
            *target.lock().unwrap(),
            PaneOutputTarget::Connected(_)
        ));
    }
}
