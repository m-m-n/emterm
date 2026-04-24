//! Reattach and detach logic for mux sessions.

use std::sync::Arc;

use futures::SinkExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;
use crate::mux::ring_buffer::DetachRingBuffer;
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    PaneId, PaneOutputTarget, PtyOutputChunk, SharedShadowParser, TitleChangeSender,
};

/// Build a self-contained ANSI byte sequence that reproduces the current
/// screen state tracked by the given shadow parser.
///
/// Output layout: `ESC[H ESC[2J` + `vt100::Screen::contents_formatted()`.
/// The first fragment clears the screen and homes the cursor so the client
/// starts from a known state; the second fragment replays the full screen
/// including alt-screen toggle, SGR attributes, cursor position, and cells.
///
/// Used by both the reattach path (combined with ring buffer delta) and the
/// on-demand `RequestPaneSnapshot` path (shadow parser output only).
pub(super) fn build_shadow_parser_snapshot(shadow_parser: &SharedShadowParser) -> Vec<u8> {
    let screen_data = {
        let parser = shadow_parser.lock().unwrap();
        parser.screen().contents_formatted()
    };
    let mut combined = Vec::with_capacity(screen_data.len() + 10);
    combined.extend_from_slice(b"\x1b[H\x1b[2J");
    combined.extend_from_slice(&screen_data);
    combined
}

/// Collect reattach data for panes in the given session.
///
/// Drains buffered output from detached panes and switches them to connected mode.
/// Also swaps the session's `active_client_kick` to the caller's sender. Any
/// previously registered kick sender is fired (after releasing the session lock)
/// so the prior attached client is signalled to detach.
pub(super) async fn collect_reattach_data(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
    title_tx: &TitleChangeSender,
    new_kick: oneshot::Sender<()>,
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

                    // Get screen restoration data from shadow parser
                    let mut combined = build_shadow_parser_snapshot(&pane.shadow_parser);
                    let is_alternate_screen = pane
                        .shadow_parser
                        .lock()
                        .unwrap()
                        .screen()
                        .alternate_screen();
                    let screen_len = combined.len();

                    // Get ring buffer data from detached panes
                    let mut target = pane.output_target.lock().unwrap();
                    let target_was = match &*target {
                        PaneOutputTarget::Connected(_) => "Connected",
                        PaneOutputTarget::Detached(_) => "Detached",
                    };
                    let ring_data = if let PaneOutputTarget::Detached(ref mut ring) = *target {
                        let buf = ring.read_all();
                        ring.clear();
                        buf
                    } else {
                        Vec::new()
                    };
                    *target = PaneOutputTarget::Connected(pane_output_tx.clone());
                    // Swap in new title sender so reader threads notify the new connection
                    *pane.title_sender.lock().unwrap() = Some(title_tx.clone());
                    log::info!(
                        "collect_reattach: pane {} was={}, screen={}B, ring={}B, total={}B, alt_screen={}, exited={}",
                        pane.id,
                        target_was,
                        screen_len,
                        ring_data.len(),
                        screen_len + ring_data.len(),
                        is_alternate_screen,
                        pane.exited
                    );

                    // Ensure the ring buffer (up to 64MB) fits without incremental
                    // reallocation. `build_shadow_parser_snapshot` only reserves
                    // `screen_data.len() + 10`, so without this the reattach path
                    // would grow `combined` in ~log2(ring_data.len() / screen_len)
                    // doubling steps.
                    combined.reserve(ring_data.len());
                    combined.extend_from_slice(&ring_data);

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

/// Send reattach data (PaneCreated + buffered output) to the client.
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
        if !buffered.is_empty() {
            let msg = MuxMessage::pty_output(*pane_id, buffered.clone());
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
                    PaneOutputTarget::Detached(_) => "Detached",
                };
                let owned_by_caller = match &*target {
                    PaneOutputTarget::Connected(tx) => tx.same_channel(owned_tx),
                    PaneOutputTarget::Detached(_) => false,
                };
                if owned_by_caller {
                    *target = PaneOutputTarget::Detached(DetachRingBuffer::new(
                        crate::mux::ring_buffer::DEFAULT_RING_CAPACITY,
                    ));
                    log::info!(
                        "detach_session_panes: pane {} switched {} -> Detached",
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
    use crate::mux::session::pane::{MuxPane, PaneOutputTarget, SharedOutputTarget};
    use std::sync::Mutex as StdMutex;

    fn make_test_pane_with_target(id: u32, output_target: SharedOutputTarget) -> MuxPane {
        MuxPane::new_test(id, 80, 24, output_target)
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
        let data = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx).await;

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

        let mut ring1 = DetachRingBuffer::new(crate::mux::ring_buffer::DEFAULT_RING_CAPACITY);
        ring1.write(b"hello from pane 1");
        let mut ring2 = DetachRingBuffer::new(crate::mux::ring_buffer::DEFAULT_RING_CAPACITY);
        ring2.write(b"hello from pane 2");

        let target1: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Detached(ring1)));
        let target2: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Detached(ring2)));

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

        let (new_tx, _new_rx) = mpsc::channel::<PtyOutputChunk>(256);
        let (title_tx, _title_rx) = mpsc::channel::<(u32, String)>(16);
        let (kick_tx, _kick_rx) = oneshot::channel::<()>();
        let data = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx).await;

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
        let data = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx).await;

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
            PaneOutputTarget::Detached(_)
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
        let _ = collect_reattach_data(&mgr, session_id, &tx1, &title_tx, kick_tx1).await;

        // Receiver must still be pending (no kick yet).
        assert!(
            kick_rx1.try_recv().is_err(),
            "first client should not be kicked before second attach"
        );

        // Second client attaches: should fire kick1 and install kick2.
        let (tx2, _rx2) = mpsc::channel::<PtyOutputChunk>(256);
        let (kick_tx2, mut kick_rx2) = oneshot::channel::<()>();
        let _ = collect_reattach_data(&mgr, session_id, &tx2, &title_tx, kick_tx2).await;

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
                    PaneOutputTarget::Detached(_)
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
        let _ = collect_reattach_data(&mgr, session_id, &new_tx, &title_tx, kick_tx).await;

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
}
