//! Reattach and detach logic for mux sessions.

use std::sync::Arc;

use futures::SinkExt;
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::codec::Framed;

use super::codec::MuxCodec;
use super::protocol::*;
use crate::mux::ring_buffer::DetachRingBuffer;
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{PaneId, PaneOutputTarget, PtyOutputChunk};

/// Collect reattach data for panes in the given session.
///
/// Drains buffered output from detached panes and switches them to connected mode.
pub(super) async fn collect_reattach_data(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
    pane_output_tx: &mpsc::Sender<PtyOutputChunk>,
) -> Vec<(PaneId, Vec<u8>)> {
    let mgr = session_manager.lock().await;
    let mut data: Vec<(PaneId, Vec<u8>)> = Vec::new();
    if let Some(session) = mgr.get_session(session_id) {
        for window in session.windows.values() {
            for pane in window.panes.values() {
                if pane.exited {
                    continue;
                }

                // Get screen restoration data from shadow parser
                let screen_data = {
                    let parser = pane.shadow_parser.lock().unwrap();
                    parser.screen().contents_formatted()
                };

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
                log::info!(
                    "collect_reattach: pane {} was={}, screen={}B, ring={}B, total={}B, exited={}",
                    pane.id,
                    target_was,
                    screen_data.len(),
                    ring_data.len(),
                    screen_data.len() + ring_data.len(),
                    pane.exited
                );

                // Combine: reset screen + shadow parser contents + ring buffer replay
                let mut combined = Vec::with_capacity(screen_data.len() + ring_data.len() + 10);
                // Reset: clear screen + home cursor
                combined.extend_from_slice(b"\x1b[H\x1b[2J");
                combined.extend_from_slice(&screen_data);
                combined.extend_from_slice(&ring_data);

                data.push((pane.id, combined));
            }
        }
    }
    data
}

/// Send reattach data (PaneCreated + buffered output) to the client.
pub(super) async fn send_reattach_data(
    framed: &mut Framed<UnixStream, MuxCodec>,
    reattach_data: &[(PaneId, Vec<u8>)],
) -> Result<(), ()> {
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

/// Switch panes in a session to detached buffering mode.
pub(super) async fn detach_session_panes(
    session_manager: &Arc<Mutex<SessionManager>>,
    session_id: u32,
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
                if let PaneOutputTarget::Connected(_) = &*target {
                    *target = PaneOutputTarget::Detached(DetachRingBuffer::new(
                        crate::mux::ring_buffer::DEFAULT_RING_CAPACITY,
                    ));
                    log::info!(
                        "detach_session_panes: pane {} switched {} -> Detached",
                        pane.id,
                        was
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
        let data = collect_reattach_data(&mgr, session_id, &new_tx).await;

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
        let data = collect_reattach_data(&mgr, session_id, &new_tx).await;

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
        let data = collect_reattach_data(&mgr, session_id, &new_tx).await;

        assert_eq!(
            data.len(),
            1,
            "Should only return 1 entry (pane 2 is exited)"
        );
        assert_eq!(data[0].0, 1, "Only pane 1 should be included");
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
