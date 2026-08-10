use super::*;
use crate::agent_status::AgentState as CoreAgentState;
use crate::mux::session::pane::{
    AgentWaiter, DeferredOutputItem, MuxPane, PaneOutputTarget, SharedOutputTarget,
};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::oneshot;

/// Decode a `Snapshot`-kind chunk's wire-encoded `data` (task0004
/// round-4 rework D1', `mux_ipc::protocol::decode_snapshot_payload`)
/// back into its plain content bytes, discarding the structural
/// segment header — used by tests that only care about the ANSI
/// content layout (clear prefix / scrollback / screen ordering), not
/// the segments themselves.
fn decode_snapshot_chunk_content(data: &[u8]) -> Vec<u8> {
    mux_ipc::protocol::decode_snapshot_payload(data).1.to_vec()
}

fn add_pane(
    mgr: &mut SessionManager,
    session_id: u32,
    window_id: u32,
    pane_id: u32,
    target: SharedOutputTarget,
) {
    let pane = MuxPane::new_test(pane_id, 80, 24, target);
    mgr.get_session_mut(session_id)
        .unwrap()
        .windows
        .get_mut(&window_id)
        .unwrap()
        .add_pane(pane);
}

/// FR3 byte-identity guard-rail: the lock-scope refactor in
/// `handle_request_pane_snapshot` (scoped `read_all` block) must NOT change
/// the assembled snapshot bytes. This reconstructs the same inputs the
/// handler feeds to `build_shadow_parser_snapshot` (an owned `read_all`
/// copy + the shadow screen) and asserts the result follows the
/// `ESC[H ESC[2J + scrollback + screen` layout — for both a representative
/// screen + scrollback and the empty-scrollback case.
///
/// Driven through the `alt_screen = true` branch (parser flipped via
/// ESC[?1049h before feeding the screen bytes) because the layout-split
/// contract omits the daemon vt100 dump for main-buffer panes; the
/// SCREEN-CONTENT presence assertion is only meaningful for the alt
/// branch.
#[test]
fn snapshot_bytes_unchanged_after_lock_scope_guardrail() {
    use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
    use crate::mux::session::pane::new_shadow_parser;
    use std::sync::Mutex as StdMutex;

    // Representative screen + scrollback. Switch to alt-screen first so
    // build_shadow_parser_snapshot follows the alt branch and includes
    // the screen dump.
    let shadow_parser: SharedShadowParser = Arc::new(StdMutex::new(new_shadow_parser(24, 80)));
    shadow_parser.lock().unwrap().process(b"\x1b[?1049h");
    shadow_parser
        .lock()
        .unwrap()
        .process(b"\x1b[31mSCREEN-CONTENT\x1b[0m");

    let scrollback: SharedScrollback =
        Arc::new(StdMutex::new(ScrollbackRingBuffer::new(64 * 1024)));
    scrollback
        .lock()
        .unwrap()
        .write(b"HISTORY-LINE-ONE\r\nHISTORY-LINE-TWO\r\n");

    // Mirror the handler's scoped-copy step, then assemble.
    let (scrollback_data, scrollback_segments): (Vec<u8>, Vec<(usize, u16, u16)>) = {
        let guard = scrollback.lock().unwrap();
        guard.read_segments()
    };
    let (assembled, _segments) =
        build_shadow_parser_snapshot(&shadow_parser, &scrollback_data, &scrollback_segments);

    // Established layout: ESC[3J ESC[H ESC[2J + scrollback + shadow screen.
    assert!(
        assembled.starts_with(b"\x1b[3J\x1b[H\x1b[2J"),
        "snapshot must start with the clear+home prefix"
    );
    let find = |needle: &[u8]| {
        assembled
            .windows(needle.len())
            .position(|w| w == needle)
            .unwrap_or_else(|| panic!("needle {:?} not found", needle))
    };
    let sb_at = find(b"HISTORY-LINE-ONE");
    let screen_at = find(b"SCREEN-CONTENT");
    assert!(
        sb_at >= b"\x1b[3J\x1b[H\x1b[2J".len(),
        "scrollback after clear prefix"
    );
    assert!(
        sb_at < screen_at,
        "scrollback must precede the shadow screen"
    );
    // The owned-copy path produces the exact same bytes as feeding the
    // scrollback slice straight through (no behavioral divergence).
    let (sb_direct, seg_direct) = scrollback.lock().unwrap().read_segments();
    let (direct, _) = build_shadow_parser_snapshot(&shadow_parser, &sb_direct, &seg_direct);
    assert_eq!(assembled, direct, "scoped copy must be byte-identical");

    // Empty-scrollback case: still a valid clear + shadow snapshot.
    let empty_sb: SharedScrollback = Arc::new(StdMutex::new(ScrollbackRingBuffer::new(64 * 1024)));
    let (empty_data, empty_segments): (Vec<u8>, Vec<(usize, u16, u16)>) = {
        let guard = empty_sb.lock().unwrap();
        guard.read_segments()
    };
    assert!(empty_data.is_empty(), "fresh buffer reads back empty");
    let (empty_assembled, _) =
        build_shadow_parser_snapshot(&shadow_parser, &empty_data, &empty_segments);
    assert!(empty_assembled.starts_with(b"\x1b[3J\x1b[H\x1b[2J"));
    assert!(
        empty_assembled
            .windows(b"SCREEN-CONTENT".len())
            .any(|w| w == b"SCREEN-CONTENT"),
        "shadow screen present with empty scrollback"
    );
}

/// TS-7: SetVisibility(false) flips identity-owned panes to Detached.
/// While hidden, no PTY chunks must reach the channel (the reader thread
/// would push into the per-pane ring buffer instead).
#[tokio::test]
async fn handle_set_visibility_false_switches_owned_pane_to_detached() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    let visible_state = Arc::new(AtomicBool::new(true));
    let mut deferred = DeferredOutputQueue::new();
    handle_set_visibility(
        false,
        &mgr,
        session_id,
        &owned_tx,
        &visible_state,
        &mut deferred,
    )
    .await;

    assert!(!visible_state.load(Ordering::Acquire));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Detached { .. }
    ));
    // No snapshot enqueued on hidden transition.
    assert!(rx.try_recv().is_err(), "no snapshot expected on hidden");
}

/// TS-7 / TS-14b: SetVisibility(true) after hidden enqueues exactly one
/// snapshot per pane onto the channel and restores Connected.
#[tokio::test]
async fn handle_set_visibility_true_after_hidden_enqueues_snapshot() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

    // Start in Detached as if SetVisibility(false) ran earlier and the
    // reader had captured shadow + raw_passthrough state. Owner = the
    // caller's tx so the visibility resume is permitted.
    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        // Seed shadow + raw_passthrough on the just-added pane.
        let pane_ref = m
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&1)
            .unwrap();
        pane_ref.shadow_parser.lock().unwrap().process(b"hi-shadow");
        pane_ref
            .raw_passthrough
            .lock()
            .unwrap()
            .append(b"\x1b_Gi=9;XX\x1b\\");
        sid
    };

    // visible_state was false before this call — same precondition the
    // hidden -> visible transition exhibits in production.
    let visible_state = Arc::new(AtomicBool::new(false));
    let mut deferred = DeferredOutputQueue::new();
    handle_set_visibility(
        true,
        &mgr,
        session_id,
        &owned_tx,
        &visible_state,
        &mut deferred,
    )
    .await;

    assert!(visible_state.load(Ordering::Acquire));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));

    // Exactly one snapshot chunk must have landed on the channel.
    let chunk = rx.try_recv().expect("snapshot chunk expected");
    assert_eq!(chunk.pane_id, 1);
    assert!(decode_snapshot_chunk_content(&chunk.data).starts_with(b"\x1b[H\x1b[2J"));
    // Captured passthrough must NOT be replayed (would re-render the image).
    let needle = b"\x1b_Gi=9;XX\x1b\\";
    assert!(
        !decode_snapshot_chunk_content(&chunk.data)
            .windows(needle.len())
            .any(|w| w == needle),
        "snapshot must NOT include the captured passthrough sequence"
    );
    assert!(
        rx.try_recv().is_err(),
        "no further chunk expected for a single-pane session"
    );
}

/// F2 regression: SetVisibility(true) holds the pane's `output_target`
/// mutex across (snapshot enqueue → Connected swap). A reader that takes
/// the same mutex cannot interleave a live chunk between those steps,
/// so the channel FIFO guarantees the snapshot lands first.
///
/// The test inspects the per-chunk ordering on the channel: the
/// snapshot chunk must appear with `pane_output_tx` already in
/// `Connected` mode is impossible to assert with deterministic timing
/// in a unit test, so we instead verify the post-conditions that prove
/// the lock was held across both steps:
/// - target is Connected
/// - snapshot chunk is on the channel
/// - no concurrent reader could have raced because the test does not
///   spawn a reader and the resume path is single-threaded
///
/// Combined with `pane_output_tx` having capacity 1 *and* the receiver
/// being unread until after `handle_set_visibility` completes, the
/// existence of the chunk in the channel after the swap proves the
/// permit-based synchronous send happened under the pane lock.
#[tokio::test]
async fn handle_set_visibility_resume_uses_permit_under_pane_lock() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    // Capacity 1: the only way the snapshot can land while the swap to
    // Connected also succeeds is if the resume path reserved a permit
    // and used it synchronously inside the pane lock.
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);

    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    let visible_state = Arc::new(AtomicBool::new(false));
    let mut deferred = DeferredOutputQueue::new();
    handle_set_visibility(
        true,
        &mgr,
        session_id,
        &owned_tx,
        &visible_state,
        &mut deferred,
    )
    .await;

    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
    let chunk = rx.try_recv().expect("snapshot chunk must be queued");
    assert_eq!(chunk.pane_id, 1);
    assert!(decode_snapshot_chunk_content(&chunk.data).starts_with(b"\x1b[H\x1b[2J"));
}

/// F2 regression: with two panes, each gets exactly one snapshot
/// chunk and the per-pane (send, swap) sequence cannot interleave
/// because `resume_pane_with_permit` holds the per-pane mutex.
#[tokio::test]
async fn handle_set_visibility_resume_two_panes_each_gets_one_snapshot() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

    let target1: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let target2: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid1 = m.create_window(sid, "shell".to_string()).unwrap();
        let wid2 = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid1, 1, target1.clone());
        add_pane(&mut m, sid, wid2, 2, target2.clone());
        sid
    };

    let visible_state = Arc::new(AtomicBool::new(false));
    let mut deferred = DeferredOutputQueue::new();
    handle_set_visibility(
        true,
        &mgr,
        session_id,
        &owned_tx,
        &visible_state,
        &mut deferred,
    )
    .await;

    let mut seen = std::collections::HashSet::new();
    for _ in 0..2 {
        let chunk = rx.try_recv().expect("snapshot chunk expected");
        assert!(seen.insert(chunk.pane_id), "duplicate snapshot for pane");
        assert!(decode_snapshot_chunk_content(&chunk.data).starts_with(b"\x1b[H\x1b[2J"));
    }
    assert!(rx.try_recv().is_err(), "exactly two snapshots expected");
    assert!(matches!(
        *target1.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
    assert!(matches!(
        *target2.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
}

/// Idempotent: SetVisibility with the same value as the current state
/// must be a no-op (no pane churn, no snapshot).
#[tokio::test]
async fn handle_set_visibility_same_state_is_noop() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    let visible_state = Arc::new(AtomicBool::new(true));
    let mut deferred = DeferredOutputQueue::new();
    handle_set_visibility(
        true,
        &mgr,
        session_id,
        &owned_tx,
        &visible_state,
        &mut deferred,
    )
    .await;

    // No state change, no snapshot.
    assert!(visible_state.load(Ordering::Acquire));
    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
    assert!(rx.try_recv().is_err(), "no snapshot expected on no-op");
}

/// TS-2 (FR1, FR3): `handle_request_pane_snapshot` enqueues a chunk
/// whose discriminator is `ChunkKind::Snapshot` (not the default
/// `ChunkKind::PtyOutput`). The drain layer (`mux::ipc::connection`)
/// is responsible for encoding `Snapshot` chunks as
/// `MessageType::Snapshot` on the wire so the client routes them to
/// the `apply_mux_message::Snapshot|SnapshotRestore` arm and the
/// `build_from_snapshot` + `scrollback_bypass` fast path.
///
/// The assembled payload follows the `ESC[3J ESC[H ESC[2J` clear-prefix,
/// then scrollback, then (for alt-screen panes) shadow screen contents
/// layout. The shadow parser is driven into alt-screen mode before
/// feeding the screen bytes because the layout-split contract omits the
/// daemon vt100 dump for main-buffer panes.
#[tokio::test]
async fn handle_request_pane_snapshot_emits_snapshot_kind() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        // Seed shadow + scrollback so the assembled snapshot has
        // recognisable bytes for the post-conditions. Flip to alt-screen
        // first so the daemon vt100 dump is included.
        let pane_ref = m
            .get_session(sid)
            .unwrap()
            .windows
            .get(&wid)
            .unwrap()
            .panes
            .get(&1)
            .unwrap();
        pane_ref
            .shadow_parser
            .lock()
            .unwrap()
            .process(b"\x1b[?1049h");
        pane_ref
            .shadow_parser
            .lock()
            .unwrap()
            .process(b"\x1b[31mSCREEN-CONTENT\x1b[0m");
        pane_ref
            .scrollback
            .lock()
            .unwrap()
            .write(b"HISTORY-LINE-ONE\r\n");
        sid
    };

    let req = MuxMessage {
        msg_type: MessageType::RequestPaneSnapshot,
        pane_id: 1,
        payload: Vec::new(),
    };
    let mut deferred = DeferredOutputQueue::new();
    handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx, &mut deferred)
        .await
        .expect("handle_request_pane_snapshot");

    let chunk = rx.try_recv().expect("snapshot chunk expected");
    assert_eq!(chunk.pane_id, 1);
    assert_eq!(
        chunk.kind,
        crate::mux::session::pane::ChunkKind::Snapshot,
        "snapshot reply must carry kind = Snapshot (FR1, FR3)"
    );
    // Byte-identity guardrail: clear+home prefix, then scrollback,
    // then shadow screen. `chunk.data` is the D1' wire-encoded payload
    // (structural segment header + content bytes) — decode it first.
    let content = decode_snapshot_chunk_content(&chunk.data);
    assert!(
        content.starts_with(b"\x1b[3J\x1b[H\x1b[2J"),
        "snapshot must start with the clear+home prefix"
    );
    let find = |needle: &[u8]| {
        content
            .windows(needle.len())
            .position(|w| w == needle)
            .unwrap_or_else(|| panic!("needle {:?} not found in snapshot", needle))
    };
    let sb_at = find(b"HISTORY-LINE-ONE");
    let screen_at = find(b"SCREEN-CONTENT");
    assert!(
        sb_at < screen_at,
        "scrollback must precede the shadow screen"
    );
    assert!(
        rx.try_recv().is_err(),
        "exactly one snapshot chunk expected"
    );
}

/// TS-3 (FR1, FR5): FIFO ordering between PTY chunks and a snapshot
/// reply on the same pane. The on-channel order MUST be
/// `[PRE(PtyOutput), snapshot(Snapshot), POST(PtyOutput)]`. The
/// drain layer's `merge_consecutive_chunks` must not collapse across
/// `kind`, so the snapshot stays a standalone chunk between the two
/// PTY chunks.
#[tokio::test]
async fn handle_request_pane_snapshot_preserves_fifo_ordering() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(16);

    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    // PRE PTY chunk (simulates a reader-thread chunk already in flight).
    owned_tx
        .send(PtyOutputChunk::pty_output(1, b"PRE".to_vec()))
        .await
        .expect("send PRE");

    // Snapshot reply runs *between* the PRE and POST PTY chunks.
    let req = MuxMessage {
        msg_type: MessageType::RequestPaneSnapshot,
        pane_id: 1,
        payload: Vec::new(),
    };
    let mut deferred = DeferredOutputQueue::new();
    handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx, &mut deferred)
        .await
        .expect("handle_request_pane_snapshot");

    // POST PTY chunk after the snapshot.
    owned_tx
        .send(PtyOutputChunk::pty_output(1, b"POST".to_vec()))
        .await
        .expect("send POST");

    let pre = rx.try_recv().expect("PRE chunk");
    let snap = rx.try_recv().expect("snapshot chunk");
    let post = rx.try_recv().expect("POST chunk");
    assert!(
        rx.try_recv().is_err(),
        "exactly three chunks expected in this order"
    );

    assert_eq!(pre.data, b"PRE");
    assert_eq!(pre.kind, crate::mux::session::pane::ChunkKind::PtyOutput);

    assert_eq!(snap.pane_id, 1);
    assert_eq!(snap.kind, crate::mux::session::pane::ChunkKind::Snapshot);
    assert!(decode_snapshot_chunk_content(&snap.data).starts_with(b"\x1b[3J\x1b[H\x1b[2J"));

    assert_eq!(post.data, b"POST");
    assert_eq!(post.kind, crate::mux::session::pane::ChunkKind::PtyOutput);
}

// ── mux-window-switch-output-hang task0001 (self-deadlock fix) reworked
// in task0002 (bounded, order-preserving deferred delivery via a
// connection-owned `DeferredOutputQueue` instead of a spawned task per
// full-channel occurrence) ──

/// AC-1: with the pane output channel filled to capacity by pane A's own
/// simulated high-volume output, issuing a snapshot request for pane A
/// (the SAME pane) must still return promptly. Before task0001,
/// `handle_request_pane_snapshot` performed a blocking
/// `pane_output_tx.send(...).await` here — this same connection task is
/// the ONLY consumer able to free capacity, and it cannot run its own
/// drain arm while suspended inside this call, so the pre-fix code would
/// hang instead of returning (the exact self-deadlock SPEC.md describes).
///
/// task0002 rework: the deferred chunk now lands in `deferred` rather
/// than being handed to a spawned task, and is only actually sent once
/// `flush_deferred_output` is called — exactly what the connection's own
/// event loop does right after its own drain of `pane_output_rx`. The
/// explicit `flush_deferred_output` call below stands in for that.
#[tokio::test]
async fn handle_request_pane_snapshot_returns_promptly_when_own_pane_channel_full() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    // Small capacity so it is trivial to fill.
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(2);

    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    // Fill the channel to capacity with pane A's own high-volume output.
    owned_tx
        .send(PtyOutputChunk::pty_output(1, b"a".to_vec()))
        .await
        .expect("send a");
    owned_tx
        .send(PtyOutputChunk::pty_output(1, b"b".to_vec()))
        .await
        .expect("send b");
    assert!(
        owned_tx
            .try_send(PtyOutputChunk::pty_output(1, b"never".to_vec()))
            .is_err(),
        "test prerequisite: channel must be at capacity"
    );

    let req = MuxMessage {
        msg_type: MessageType::RequestPaneSnapshot,
        pane_id: 1,
        payload: Vec::new(),
    };

    // The bug this task fixes: this call must return within a bounded,
    // short time even though the channel is completely full.
    let mut deferred = DeferredOutputQueue::new();
    tokio::time::timeout(
        std::time::Duration::from_millis(300),
        handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx, &mut deferred),
    )
    .await
    .expect(
        "handle_request_pane_snapshot must return promptly even when \
         pane_output_tx is at capacity for the SAME pane (AC-1)",
    )
    .expect("handler itself must not error");
    assert_eq!(
        deferred.len(),
        1,
        "a full channel must defer the snapshot chunk, not send it"
    );

    // The two already-queued chunks must be observed before the
    // deferred snapshot chunk (FIFO, AC-3) — draining exactly as the
    // connection's own event loop would.
    let c1 = rx.recv().await.expect("chunk a");
    assert_eq!(c1.data, b"a");
    let c2 = rx.recv().await.expect("chunk b");
    assert_eq!(c2.data, b"b");

    // ...then flushing (mirrors the connection loop calling
    // `flush_deferred_output` right after its own drain) delivers the
    // deferred snapshot now that capacity is free.
    let visible_state = Arc::new(AtomicBool::new(true));
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;
    assert!(deferred.is_empty());

    let snap = rx
        .recv()
        .await
        .expect("deferred snapshot chunk must have been sent");
    assert_eq!(snap.pane_id, 1);
    assert_eq!(snap.kind, crate::mux::session::pane::ChunkKind::Snapshot);
}

/// AC-2: same full-channel setup as AC-1, but the snapshot request
/// targets a DIFFERENT pane (B) while pane A is the one whose output
/// filled the channel. The connection must keep making progress: this
/// call returns promptly, and pane A's already-queued output is still
/// forwarded ahead of pane B's snapshot once capacity frees and the
/// queue is flushed.
#[tokio::test]
async fn handle_request_pane_snapshot_for_different_pane_returns_promptly_while_channel_full() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(2);

    let target_a: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let target_b: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid_a = m.create_window(sid, "shell".to_string()).unwrap();
        let wid_b = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid_a, 1, target_a.clone());
        add_pane(&mut m, sid, wid_b, 2, target_b.clone());
        sid
    };

    // Pane A fills the channel with its own high-volume output.
    owned_tx
        .send(PtyOutputChunk::pty_output(1, b"a1".to_vec()))
        .await
        .expect("send a1");
    owned_tx
        .send(PtyOutputChunk::pty_output(1, b"a2".to_vec()))
        .await
        .expect("send a2");
    assert!(
        owned_tx
            .try_send(PtyOutputChunk::pty_output(1, b"never".to_vec()))
            .is_err(),
        "test prerequisite: channel must be at capacity"
    );

    // Snapshot requested for pane B, not the pane producing the output.
    let req = MuxMessage {
        msg_type: MessageType::RequestPaneSnapshot,
        pane_id: 2,
        payload: Vec::new(),
    };
    let mut deferred = DeferredOutputQueue::new();
    tokio::time::timeout(
        std::time::Duration::from_millis(300),
        handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx, &mut deferred),
    )
    .await
    .expect("handle_request_pane_snapshot must return promptly (AC-2)")
    .expect("handler itself must not error");
    assert_eq!(deferred.len(), 1);

    let c1 = rx.recv().await.expect("pane A chunk 1");
    assert_eq!(c1.pane_id, 1);
    assert_eq!(c1.data, b"a1");
    let c2 = rx.recv().await.expect("pane A chunk 2");
    assert_eq!(c2.pane_id, 1);
    assert_eq!(c2.data, b"a2");

    let visible_state = Arc::new(AtomicBool::new(true));
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;
    assert!(deferred.is_empty());

    let snap = rx
        .recv()
        .await
        .expect("deferred snapshot for pane B must have been sent");
    assert_eq!(snap.pane_id, 2);
    assert_eq!(snap.kind, crate::mux::session::pane::ChunkKind::Snapshot);
}

/// Audit fix (mux-window-switch-output-hang task0001, reworked task0002):
/// `handle_set_visibility`'s visibility-resume path used to
/// `pane_output_tx.reserve().await` per pane — the same self-blockable
/// shape as the snapshot bug, reachable from the same connection task
/// via `route_message`'s `SetVisibility` arm. With the channel full, the
/// call must still return promptly, and the deferred resume (snapshot
/// chunk + Connected swap) must still land once capacity frees and the
/// connection-owned queue is flushed.
#[tokio::test]
async fn handle_set_visibility_true_returns_promptly_when_channel_full_and_resumes_after_drain() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);

    // Fill the single slot with an unrelated chunk.
    owned_tx
        .send(PtyOutputChunk::pty_output(99, b"x".to_vec()))
        .await
        .expect("send filler");
    assert!(
        owned_tx
            .try_send(PtyOutputChunk::pty_output(99, b"never".to_vec()))
            .is_err(),
        "test prerequisite: channel must be at capacity"
    );

    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    let visible_state = Arc::new(AtomicBool::new(false));
    let mut deferred = DeferredOutputQueue::new();
    tokio::time::timeout(
        std::time::Duration::from_millis(300),
        handle_set_visibility(
            true,
            &mgr,
            session_id,
            &owned_tx,
            &visible_state,
            &mut deferred,
        ),
    )
    .await
    .expect(
        "handle_set_visibility must return promptly even when \
         pane_output_tx is at capacity",
    );

    assert!(visible_state.load(Ordering::Acquire));
    assert_eq!(
        deferred.len(),
        1,
        "a full channel must defer the visibility resume, not spawn a task for it"
    );

    // Drain the filler chunk, freeing capacity for the deferred resume —
    // exactly what the connection's own event loop does before it calls
    // `flush_deferred_output`.
    let filler = rx.recv().await.expect("filler chunk");
    assert_eq!(filler.pane_id, 99);

    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;
    assert!(deferred.is_empty());

    let resumed = rx
        .recv()
        .await
        .expect("deferred resume snapshot must have been sent");
    assert_eq!(resumed.pane_id, 1);
    assert_eq!(resumed.kind, crate::mux::session::pane::ChunkKind::Snapshot);

    assert!(matches!(
        *target.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));
}

/// AC-1/F1/F2/F3 regression: a visibility-resume deferred while the
/// channel was full must NOT be applied if the pane was hidden again
/// (`SetVisibility(false)`) before capacity freed and the queue was
/// flushed. `flush_deferred_output` re-validates `visible_state` fresh
/// at flush time — this pins that the stale resume is dropped rather
/// than incorrectly swapping the pane back to `Connected`.
#[tokio::test]
async fn flush_deferred_output_drops_stale_visibility_resume_when_hidden_again() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);

    owned_tx
        .send(PtyOutputChunk::pty_output(99, b"x".to_vec()))
        .await
        .expect("send filler");

    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    // visible=true while the channel is full defers the resume.
    let visible_state = Arc::new(AtomicBool::new(false));
    let mut deferred = DeferredOutputQueue::new();
    handle_set_visibility(
        true,
        &mgr,
        session_id,
        &owned_tx,
        &visible_state,
        &mut deferred,
    )
    .await;
    assert_eq!(deferred.len(), 1);

    // Before capacity frees, the SAME connection processes a
    // SetVisibility(false) for the pane (e.g. a rapid window switch) —
    // handle_set_visibility's own `!visible` branch re-detaches eligible
    // panes and flips `visible_state` back to false.
    handle_set_visibility(
        false,
        &mgr,
        session_id,
        &owned_tx,
        &visible_state,
        &mut deferred,
    )
    .await;
    assert!(!visible_state.load(Ordering::Acquire));

    // Now capacity frees and the queue is flushed.
    let filler = rx.recv().await.expect("filler chunk");
    assert_eq!(filler.pane_id, 99);
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;
    assert!(
        deferred.is_empty(),
        "stale item must be dropped, not requeued"
    );

    // The stale resume must have been dropped, not applied: no snapshot
    // chunk was sent, and the pane must still be Detached.
    assert!(
        rx.try_recv().is_err(),
        "a stale visibility-resume must not send a snapshot"
    );
    assert!(
        matches!(*target.lock().unwrap(), PaneOutputTarget::Detached { .. }),
        "pane hidden again before flush must stay Detached (AC-1/F1/F2/F3)"
    );
}

/// AC-1 regression (mux-window-switch-output-hang task0006 rework,
/// review round 5 high findings `4043ee676f69ca15` / `1c8d86389ab4bf40`):
/// a `RequestPaneSnapshot` for a pane that already has a
/// `VisibilityResume` queued for it must still be delivered even when
/// that Resume no-ops at flush time — here because the pane is already
/// `Connected` by then, one of the four situations
/// `resume_pane_with_permit`/`resolve_pane_and_resume` return
/// `NoChange` without sending anything (see `defer_chunk`'s own doc for
/// the other three). task0005's now-reverted fix dropped the Chunk
/// outright in this exact situation, so the client's request got NO
/// reply at all — the tab stayed stale until the next unrelated output.
/// This is not an exotic setup: `handle_set_visibility` queues a
/// `VisibilityResume` for every non-exited pane in the session on a
/// visible edge without checking whether that pane is actually
/// detached-hidden, so a resume that will no-op is the NORMAL case.
#[tokio::test]
async fn flush_deferred_output_delivers_chunk_even_when_its_queued_visibility_resume_no_ops() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(4);

    // Pane 1 is already Connected — its queued VisibilityResume will
    // resolve to `ResumeOutcome::NoChange` at flush time.
    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    let visible_state = Arc::new(AtomicBool::new(true));
    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_visibility_resume(1);
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"requested-snapshot".to_vec()));
    assert_eq!(
        deferred.len(),
        2,
        "the Chunk must be inserted alongside the Resume, not dropped"
    );

    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;
    assert!(deferred.is_empty());

    let delivered = rx.try_recv().expect(
        "the snapshot Chunk must still be delivered even though its queued \
         VisibilityResume no-ops (pane already Connected)",
    );
    assert_eq!(delivered.pane_id, 1);
    assert_eq!(delivered.data, b"requested-snapshot");
    assert_eq!(
        delivered.kind,
        crate::mux::session::pane::ChunkKind::Snapshot
    );

    // The no-op resume must not have sent a second chunk of its own.
    assert!(
        rx.try_recv().is_err(),
        "a no-op VisibilityResume must not send anything"
    );
    assert!(
        matches!(*target.lock().unwrap(), PaneOutputTarget::Connected(_)),
        "pane must remain Connected — the resume genuinely no-ops here"
    );
}

// ── mux-window-switch-output-hang task0003 rework: overflow policy
// (AC-1/AC-2) and flush arm coverage (AC-5) ──

/// AC-1: with the channel full and MORE candidate panes than the
/// (former, now chunk-only) `MAX_DEFERRED_ITEMS` cap, every pane that
/// should resume is resumed once capacity frees — none is left
/// `Detached { HiddenByVisibility }`.
#[tokio::test]
async fn handle_set_visibility_resumes_every_pane_even_when_candidates_exceed_the_former_cap() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let pane_count = crate::mux::session::pane::MAX_DEFERRED_ITEMS + 3;
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(pane_count);

    let mut targets: Vec<SharedOutputTarget> = Vec::new();
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        for pane_id in 1..=(pane_count as u32) {
            let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
                reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
                owner: Some(owned_tx.clone()),
            }));
            add_pane(&mut m, sid, wid, pane_id, target.clone());
            targets.push(target);
        }
        sid
    };

    // Fill the channel completely with unrelated filler chunks so every
    // candidate pane's `try_reserve()` observes Full.
    for _ in 0..pane_count {
        owned_tx
            .send(PtyOutputChunk::pty_output(9999, b"x".to_vec()))
            .await
            .expect("send filler");
    }
    assert!(
        owned_tx
            .try_send(PtyOutputChunk::pty_output(9999, b"never".to_vec()))
            .is_err(),
        "test prerequisite: channel must be at capacity"
    );

    let visible_state = Arc::new(AtomicBool::new(false));
    let mut deferred = DeferredOutputQueue::new();
    handle_set_visibility(
        true,
        &mgr,
        session_id,
        &owned_tx,
        &visible_state,
        &mut deferred,
    )
    .await;

    assert_eq!(
        deferred.len(),
        pane_count,
        "every candidate pane's resume must be deferred, none dropped for capacity"
    );

    // Drain the filler chunks, freeing full capacity — exactly what the
    // connection's own event loop does before calling
    // `flush_deferred_output`.
    for _ in 0..pane_count {
        let filler = rx.recv().await.expect("filler chunk");
        assert_eq!(filler.pane_id, 9999);
    }

    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;
    assert!(
        deferred.is_empty(),
        "all deferred resumes must be flushed once capacity is fully free"
    );

    for target in &targets {
        assert!(
            matches!(*target.lock().unwrap(), PaneOutputTarget::Connected(_)),
            "every pane must resume — none left Detached"
        );
    }

    let mut resumed_panes = std::collections::HashSet::new();
    while let Ok(chunk) = rx.try_recv() {
        assert_eq!(chunk.kind, crate::mux::session::pane::ChunkKind::Snapshot);
        resumed_panes.insert(chunk.pane_id);
    }
    assert_eq!(
        resumed_panes.len(),
        pane_count,
        "each pane must receive exactly one resume snapshot"
    );
}

/// AC-2 (task0003) / AC-3 option (a) (mux-window-switch-output-hang
/// task0004 rework, review round 3 finding `b4eee6700d643640`): N (>
/// cap) `RequestPaneSnapshot`s for DISTINCT panes deferred against a
/// full channel, then drained — the panes evicted are the OLDEST
/// distinct ones; every surviving (most-recently-requested) pane gets
/// its own snapshot delivered, and the evicted panes get nothing. This
/// eviction IS a dropped delivery — the evicted chunk was that pane's
/// ONLY queued reply, so the evicted pane receives nothing for its
/// request, unlike same-pane coalescing (which only ever discards an
/// already-superseded reply). It is nonetheless an explicitly
/// SPEC-SANCTIONED bounded-backlog policy (SPEC.md FR3, task0004 G3
/// option (a); wording corrected task0006 rework, review round 5, after
/// task0005 G3/AC-3 review round 4 finding `329f746349f592e8` merged
/// the two exceptions' justifications into one that only actually held
/// for coalescing) — justified solely by keeping the backlog's memory
/// bound (FR4) finite, with recovery left client-driven: the client
/// recovers by switching to the evicted pane again, which re-issues
/// `RequestPaneSnapshot`.
#[tokio::test]
async fn handle_request_pane_snapshot_evicts_oldest_distinct_pane_per_spec_sanctioned_backlog_policy()
 {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let max_deferred = crate::mux::session::pane::MAX_DEFERRED_ITEMS;
    let pane_count = max_deferred + 3;
    // Capacity large enough to hold every SURVIVING (post-eviction)
    // chunk at once, so draining every filler below frees enough
    // capacity in one go for `flush_deferred_output` to clear the
    // entire backlog in a single call.
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(max_deferred + 1);

    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        for pane_id in 1..=(pane_count as u32) {
            let target: SharedOutputTarget =
                Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
            add_pane(&mut m, sid, wid, pane_id, target);
        }
        sid
    };

    // Fill the channel completely so every request below defers.
    for _ in 0..(max_deferred + 1) {
        owned_tx
            .send(PtyOutputChunk::pty_output(9999, b"filler".to_vec()))
            .await
            .expect("send filler");
    }
    assert!(
        owned_tx
            .try_send(PtyOutputChunk::pty_output(9999, b"never".to_vec()))
            .is_err(),
        "test prerequisite: channel must be at capacity"
    );

    let mut deferred = DeferredOutputQueue::new();
    for pane_id in 1..=(pane_count as u32) {
        let req = MuxMessage {
            msg_type: MessageType::RequestPaneSnapshot,
            pane_id,
            payload: Vec::new(),
        };
        handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx, &mut deferred)
            .await
            .expect("handler itself must not error");
    }

    assert_eq!(
        deferred.len(),
        max_deferred,
        "queue must never grow past the documented cap"
    );

    for _ in 0..(max_deferred + 1) {
        let filler = rx.recv().await.expect("filler chunk");
        assert_eq!(filler.pane_id, 9999);
    }

    let visible_state = Arc::new(AtomicBool::new(true));
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;
    assert!(deferred.is_empty());

    let mut delivered_pane_ids: Vec<u32> = Vec::new();
    while let Ok(chunk) = rx.try_recv() {
        assert_eq!(chunk.kind, crate::mux::session::pane::ChunkKind::Snapshot);
        delivered_pane_ids.push(chunk.pane_id);
    }
    let expected: Vec<u32> = ((pane_count - max_deferred) as u32 + 1..=pane_count as u32).collect();
    assert_eq!(
        delivered_pane_ids, expected,
        "only the most-recently-requested MAX_DEFERRED_ITEMS panes must receive a \
         snapshot — the oldest distinct panes must get nothing, never the newest"
    );
}

/// AC-2: a SECOND `RequestPaneSnapshot` for a pane whose FIRST reply is
/// still queued (channel stays full both times) coalesces — the queue
/// never grows to two entries for the same pane — proven end-to-end
/// through the actual handler + flush pipeline (the data-structure-level
/// proof with distinguishable payloads lives in
/// `mux::session::pane::tests::deferred_output_queue_coalesces_repeated_chunk_for_same_pane_newest_wins`).
#[tokio::test]
async fn handle_request_pane_snapshot_repeated_request_for_same_pane_does_not_grow_queue() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);
    let target: SharedOutputTarget =
        Arc::new(StdMutex::new(PaneOutputTarget::Connected(owned_tx.clone())));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    owned_tx
        .send(PtyOutputChunk::pty_output(99, b"filler".to_vec()))
        .await
        .expect("send filler");
    assert!(
        owned_tx
            .try_send(PtyOutputChunk::pty_output(99, b"never".to_vec()))
            .is_err(),
        "test prerequisite: channel must be at capacity"
    );

    let req = MuxMessage {
        msg_type: MessageType::RequestPaneSnapshot,
        pane_id: 1,
        payload: Vec::new(),
    };
    let mut deferred = DeferredOutputQueue::new();

    handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx, &mut deferred)
        .await
        .expect("handler itself must not error");
    assert_eq!(deferred.len(), 1);

    // Second request for the SAME pane while still full.
    handle_request_pane_snapshot(&req, session_id, &mgr, &owned_tx, &mut deferred)
        .await
        .expect("handler itself must not error");
    assert_eq!(
        deferred.len(),
        1,
        "a second request for the same pane must coalesce, not queue a second entry"
    );

    let filler = rx.recv().await.expect("filler chunk");
    assert_eq!(filler.pane_id, 99);
    let visible_state = Arc::new(AtomicBool::new(true));
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;
    assert!(deferred.is_empty());

    let snap = rx
        .recv()
        .await
        .expect("deferred snapshot must have been sent");
    assert_eq!(snap.pane_id, 1);
    assert!(
        rx.try_recv().is_err(),
        "only one coalesced snapshot must reach the client"
    );
}

/// AC-5 (review round 2 finding `6574d4221dcb5efe`): with two chunks
/// deferred (different panes) and exactly ONE slot of capacity freed,
/// `flush_deferred_output`'s `Chunk` arm sends the front one and
/// `requeue_front`s the second when it observes `Full` again —
/// exercising the arm this file's other tests never happened to hit.
#[tokio::test]
async fn flush_deferred_output_requeues_chunk_at_front_when_channel_still_full() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(2);
    owned_tx
        .send(PtyOutputChunk::pty_output(99, b"f1".to_vec()))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(99, b"f2".to_vec()))
        .await
        .unwrap();

    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"A".to_vec()));
    deferred.defer_chunk(PtyOutputChunk::snapshot(2, b"B".to_vec()));
    assert_eq!(deferred.len(), 2);

    // Free exactly ONE slot.
    let f1 = rx.recv().await.expect("first filler");
    assert_eq!(f1.data, b"f1");

    let visible_state = Arc::new(AtomicBool::new(true));
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, 0, &visible_state).await;

    assert_eq!(
        deferred.len(),
        1,
        "the second chunk must be requeued at the front when Full is observed again"
    );
    match deferred.pop_front() {
        Some(DeferredOutputItem::Chunk(chunk)) => assert_eq!(chunk.pane_id, 2),
        other => panic!("expected pane 2's chunk requeued, got {other:?}"),
    }

    // Only pane 1's chunk made it onto the channel (behind the
    // still-unread second filler).
    let f2 = rx.recv().await.expect("second filler");
    assert_eq!(f2.data, b"f2");
    let sent = rx.recv().await.expect("pane 1's chunk must have been sent");
    assert_eq!(sent.pane_id, 1);
    assert!(
        rx.try_recv().is_err(),
        "pane 2's chunk must not have been sent"
    );
}

/// AC-5: a `Closed` channel drops the ENTIRE remaining `Chunk` backlog,
/// not just the one item that observed the closure.
#[tokio::test]
async fn flush_deferred_output_clears_chunk_backlog_when_channel_closed() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, rx) = mpsc::channel::<PtyOutputChunk>(4);
    drop(rx);

    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"A".to_vec()));
    deferred.defer_chunk(PtyOutputChunk::snapshot(2, b"B".to_vec()));
    assert_eq!(deferred.len(), 2);

    let visible_state = Arc::new(AtomicBool::new(true));
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, 0, &visible_state).await;

    assert!(
        deferred.is_empty(),
        "the whole backlog must be dropped once the channel is observed Closed"
    );
}

/// AC-5: with two visibility resumes deferred (different panes) and
/// exactly ONE slot of capacity freed, `flush_deferred_output`'s
/// `VisibilityResume` arm resumes the front one and `requeue_front`s the
/// second when it observes `Full` again.
#[tokio::test]
async fn flush_deferred_output_requeues_visibility_resume_at_front_when_channel_still_full() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(2);

    let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let target_b: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target_a.clone());
        add_pane(&mut m, sid, wid, 2, target_b.clone());
        sid
    };

    owned_tx
        .send(PtyOutputChunk::pty_output(99, b"f1".to_vec()))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(99, b"f2".to_vec()))
        .await
        .unwrap();

    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_visibility_resume(1);
    deferred.defer_visibility_resume(2);
    assert_eq!(deferred.len(), 2);

    // Free exactly ONE slot.
    let f1 = rx.recv().await.expect("first filler");
    assert_eq!(f1.data, b"f1");

    let visible_state = Arc::new(AtomicBool::new(true));
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, session_id, &visible_state).await;

    assert_eq!(
        deferred.len(),
        1,
        "pane 2's resume must be requeued at the front when Full is observed again"
    );
    match deferred.pop_front() {
        Some(DeferredOutputItem::VisibilityResume(pane_id)) => assert_eq!(pane_id, 2),
        other => panic!("expected pane 2's resume requeued, got {other:?}"),
    }

    assert!(
        matches!(*target_a.lock().unwrap(), PaneOutputTarget::Connected(_)),
        "pane 1 must have resumed"
    );
    assert!(
        matches!(*target_b.lock().unwrap(), PaneOutputTarget::Detached { .. }),
        "pane 2 must still be Detached — its resume did not get a permit"
    );
}

/// AC-5: a `Closed` channel drops the ENTIRE remaining
/// `VisibilityResume` backlog too.
#[tokio::test]
async fn flush_deferred_output_clears_visibility_resume_backlog_when_channel_closed() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, rx) = mpsc::channel::<PtyOutputChunk>(4);
    drop(rx);

    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_visibility_resume(1);
    deferred.defer_visibility_resume(2);
    assert_eq!(deferred.len(), 2);

    let visible_state = Arc::new(AtomicBool::new(true));
    flush_deferred_output(&mut deferred, &owned_tx, &mgr, 0, &visible_state).await;

    assert!(
        deferred.is_empty(),
        "the whole backlog must be dropped once the channel is observed Closed"
    );
}

// ── AC-6 (mux-window-switch-output-hang task0004 rework, review round 3
// finding `c60b56cac9be2557`): direct unit coverage for
// `apply_fair_permit_to_front_deferred_item`, which previously had NO
// test driving it directly (only reachable via the real connection
// `select!` loop) — including the `AnyPermit::Owned` arm, which no test
// in the suite ever exercised. ──

/// AC-6: an EMPTY queue is a no-op — the permit is dropped without
/// sending anything, releasing its reserved slot back to the channel
/// (e.g. another path already drained the queue between the
/// reservation being armed and it resolving).
#[tokio::test]
async fn apply_fair_permit_to_front_deferred_item_empty_queue_drops_permit() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
    let permit = owned_tx
        .clone()
        .reserve_owned()
        .await
        .expect("reserve on a fresh channel must succeed");

    let mut deferred = DeferredOutputQueue::new();
    let visible_state = Arc::new(AtomicBool::new(true));
    apply_fair_permit_to_front_deferred_item(
        &mut deferred,
        permit,
        &owned_tx,
        &mgr,
        0,
        &visible_state,
    )
    .await;

    assert!(
        owned_tx.try_reserve().is_ok(),
        "the unused permit must release its slot back to the channel"
    );
}

/// AC-6: a front `Chunk` item is sent via the fair permit — the queue
/// is drained by one and the chunk reaches the channel.
#[tokio::test]
async fn apply_fair_permit_to_front_deferred_item_sends_front_chunk() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);
    let permit = owned_tx
        .clone()
        .reserve_owned()
        .await
        .expect("reserve on a fresh channel must succeed");

    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_chunk(PtyOutputChunk::snapshot(1, b"A".to_vec()));

    let visible_state = Arc::new(AtomicBool::new(true));
    apply_fair_permit_to_front_deferred_item(
        &mut deferred,
        permit,
        &owned_tx,
        &mgr,
        0,
        &visible_state,
    )
    .await;

    assert!(
        deferred.is_empty(),
        "the chunk must be popped off the queue"
    );
    let sent = rx
        .try_recv()
        .expect("the chunk must have been sent via the fair permit");
    assert_eq!(sent.pane_id, 1);
    assert_eq!(sent.data, b"A");
}

/// AC-6: a front `VisibilityResume` that is STALE (pane hidden again
/// since it was deferred, `visible_state == false`) is discarded — the
/// permit is dropped without resuming the pane, and the pane is left
/// untouched (still `Detached`).
#[tokio::test]
async fn apply_fair_permit_to_front_deferred_item_drops_stale_visibility_resume() {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, _rx) = mpsc::channel::<PtyOutputChunk>(1);
    let permit = owned_tx
        .clone()
        .reserve_owned()
        .await
        .expect("reserve on a fresh channel must succeed");

    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_visibility_resume(1);

    let visible_state = Arc::new(AtomicBool::new(false)); // hidden again
    apply_fair_permit_to_front_deferred_item(
        &mut deferred,
        permit,
        &owned_tx,
        &mgr,
        session_id,
        &visible_state,
    )
    .await;

    assert!(
        deferred.is_empty(),
        "the stale item is discarded (not requeued)"
    );
    assert!(
        matches!(*target.lock().unwrap(), PaneOutputTarget::Detached { .. }),
        "a stale resume must not resume the pane"
    );
    assert!(
        owned_tx.try_reserve().is_ok(),
        "the unused permit must release its slot back to the channel"
    );
}

/// AC-6: a front `VisibilityResume` that is still LIVE (`visible_state
/// == true`) resumes the pane via `AnyPermit::Owned` — the arm the
/// pre-fix test suite never exercised (review round 3 finding
/// `c60b56cac9be2557`: "pane.rs's tests are all `AnyPermit::Borrowed`").
#[tokio::test]
async fn apply_fair_permit_to_front_deferred_item_resumes_live_visibility_resume_via_owned_permit()
{
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (owned_tx, mut rx) = mpsc::channel::<PtyOutputChunk>(1);
    let permit = owned_tx
        .clone()
        .reserve_owned()
        .await
        .expect("reserve on a fresh channel must succeed");

    let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
        reason: crate::mux::session::pane::DetachReason::HiddenByVisibility,
        owner: Some(owned_tx.clone()),
    }));
    let session_id = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        add_pane(&mut m, sid, wid, 1, target.clone());
        sid
    };

    let mut deferred = DeferredOutputQueue::new();
    deferred.defer_visibility_resume(1);

    let visible_state = Arc::new(AtomicBool::new(true));
    apply_fair_permit_to_front_deferred_item(
        &mut deferred,
        permit,
        &owned_tx,
        &mgr,
        session_id,
        &visible_state,
    )
    .await;

    assert!(deferred.is_empty());
    assert!(
        matches!(*target.lock().unwrap(), PaneOutputTarget::Connected(_)),
        "pane must resume to Connected"
    );
    let snap = rx
        .try_recv()
        .expect("the resume snapshot must have been sent via the owned permit");
    assert_eq!(snap.pane_id, 1);
    assert_eq!(snap.kind, crate::mux::session::pane::ChunkKind::Snapshot);
}

// ========================================================================
// Agent-facing API tests (task0004): ReadPane / SendText / WaitAgentState
// ========================================================================

/// Build a session with one pane (Connected, sink writer) and return
/// `(session_manager, session_id, window_id)`; `pane_id` is the caller's
/// own choice so tests can build the matching public pane ID via
/// `mgr.lock().await.public_pane_id(pane_id)`.
async fn setup_session_with_pane(pane_id: u32) -> (Arc<Mutex<SessionManager>>, u32, u32) {
    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let (sid, wid) = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
        add_pane(&mut m, sid, wid, pane_id, target);
        (sid, wid)
    };
    (mgr, sid, wid)
}

/// Like [`setup_session_with_pane`] but installs a `Vec`-backed writer
/// so `SendText`'s exact-bytes contract is directly observable.
async fn setup_session_with_capturing_pane(
    pane_id: u32,
) -> (Arc<Mutex<SessionManager>>, u32, u32, Arc<StdMutex<Vec<u8>>>) {
    struct CapturingWriter(Arc<StdMutex<Vec<u8>>>);
    impl std::io::Write for CapturingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    let captured: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
    let (sid, wid) = {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
        let pane = MuxPane::new_test_with_writer(
            pane_id,
            80,
            24,
            target,
            Box::new(CapturingWriter(captured.clone())),
        );
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
        (sid, wid)
    };
    (mgr, sid, wid, captured)
}

fn get_pane<'a>(mgr: &'a SessionManager, sid: u32, wid: u32, pane_id: u32) -> &'a MuxPane {
    mgr.get_session(sid)
        .unwrap()
        .windows
        .get(&wid)
        .unwrap()
        .panes
        .get(&pane_id)
        .unwrap()
}

/// Poll `cond` (yielding to the executor between checks) until it
/// returns true, or panic after a bounded number of iterations. Used
/// instead of a real sleep to deterministically wait for a spawned
/// task to reach its registration point under the (single-threaded)
/// `#[tokio::test]` runtime.
async fn wait_until(mut cond: impl FnMut() -> bool) {
    for _ in 0..2000 {
        if cond() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("condition not met in time");
}

// ---- ReadPane (AC-1) ----

#[test]
fn render_pane_tail_combines_scrollback_and_screen_in_order() {
    // Realistic PTY scrollback bytes: `\r\n` line endings (a real PTY's
    // ONLCR translation turns every program `\n` into `\r\n`).
    let text = render_pane_tail(b"line1\r\nline2\r\n", "line3\nline4", 10, 80);
    assert_eq!(text, "line1\nline2\nline3\nline4");
}

#[test]
fn render_pane_tail_returns_only_the_last_n_lines() {
    let text = render_pane_tail(b"a\r\nb\r\nc\r\n", "d\ne", 2, 80);
    assert_eq!(text, "d\ne");
}

#[test]
fn render_pane_tail_caps_total_bytes() {
    let huge_screen = "x".repeat(READ_MAX_BYTES + 1000);
    let text = render_pane_tail(b"", &huge_screen, 1, 80);
    assert!(text.len() <= READ_MAX_BYTES);
}

/// AC-2: when the byte cap is exceeded, the response is the NEWEST
/// suffix (not the oldest prefix, which the previous `truncate`-based
/// implementation kept).
#[test]
fn render_pane_tail_byte_cap_retains_newest_suffix_not_oldest_prefix() {
    let screen = format!("{}TAIL-MARKER", "a".repeat(READ_MAX_BYTES + 10));
    let text = render_pane_tail(b"", &screen, 1, 80);
    assert!(text.len() <= READ_MAX_BYTES);
    assert!(
        text.ends_with("TAIL-MARKER"),
        "byte cap must retain the newest suffix, got tail: {:?}",
        &text[text.len().saturating_sub(30)..]
    );
}

/// AC-1: a CR-based overwrite (e.g. a progress bar redrawn in place)
/// must render to its FINAL state, not the raw concatenated byte
/// stream. The previous ANSI-strip + `.lines()` implementation left
/// the embedded `\r` as a literal character (since `str::lines()`
/// only splits on `\n`), so "10%" would still appear in the output.
#[test]
fn render_pane_tail_renders_cr_overwrite_to_final_state() {
    let scrollback = b"Progress: 10%\rProgress: 100%\r\n";
    let text = render_pane_tail(scrollback, "", 5, 80);
    assert_eq!(text, "Progress: 100%");
    assert!(!text.contains("10%"), "got {text:?}");
}

/// AC-1: cursor-movement escapes (here, CUB — cursor-backward) must
/// also be honored: overwriting the tail of a line in place must
/// reflect the FINAL rendered text, not the raw byte stream.
#[test]
fn render_pane_tail_renders_cursor_movement_overwrite_to_final_state() {
    // "Hello World" then move left 5 columns (CSI 5 D) and overwrite
    // "World" with "Earth".
    let scrollback = b"Hello World\x1b[5DEarth\r\n";
    let text = render_pane_tail(scrollback, "", 5, 80);
    assert_eq!(text, "Hello Earth");
}

#[tokio::test]
async fn handle_read_pane_returns_ansi_stripped_tail() {
    let pane_id = 100;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        pane.scrollback
            .lock()
            .unwrap()
            .write(b"\x1b[31mhistory-line\x1b[0m\r\n");
        pane.shadow_parser.lock().unwrap().process(b"current-line");
        m.public_pane_id(pane_id)
    };

    let req = ReadPaneMsg {
        public_pane_id,
        lines: 100,
    };
    let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
    let result = handle_read_pane(&msg, &mgr)
        .await
        .expect("read should succeed");

    assert!(
        result.text.contains("history-line"),
        "got {:?}",
        result.text
    );
    assert!(
        result.text.contains("current-line"),
        "got {:?}",
        result.text
    );
    assert!(
        !result.text.contains('\x1b'),
        "ANSI escapes must be stripped, got {:?}",
        result.text
    );
}

#[tokio::test]
async fn handle_read_pane_unknown_pane_id_errors() {
    let (mgr, _sid, _wid) = setup_session_with_pane(1).await;
    let req = ReadPaneMsg {
        public_pane_id: "deadbeef00000000-999".to_string(),
        lines: 10,
    };
    let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
    let err = handle_read_pane(&msg, &mgr).await.unwrap_err();
    assert_eq!(err.kind, AgentApiErrorKind::UnknownPane);
}

#[tokio::test]
async fn handle_read_pane_malformed_public_id_errors_unknown_pane() {
    let (mgr, _sid, _wid) = setup_session_with_pane(1).await;
    let req = ReadPaneMsg {
        public_pane_id: "not-a-valid-id".to_string(),
        lines: 10,
    };
    let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
    let err = handle_read_pane(&msg, &mgr).await.unwrap_err();
    assert_eq!(err.kind, AgentApiErrorKind::UnknownPane);
}

#[tokio::test]
async fn handle_read_pane_clamps_lines_above_max() {
    let pane_id = 101;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        let many_lines: String = (0..(READ_LINES_MAX + 500))
            .map(|i| format!("l{i}\n"))
            .collect();
        pane.scrollback.lock().unwrap().write(many_lines.as_bytes());
        m.public_pane_id(pane_id)
    };
    let req = ReadPaneMsg {
        public_pane_id,
        lines: READ_LINES_MAX + 500,
    };
    let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
    let result = handle_read_pane(&msg, &mgr)
        .await
        .expect("read should succeed");
    let line_count = result.text.lines().count();
    assert!(
        (line_count as u32) <= READ_LINES_MAX,
        "line count {line_count} must be clamped to {READ_LINES_MAX}"
    );
}

/// AC-1 (task0011 REWORK), full handler round trip: a pane whose
/// scrollback contains a CR-based overwrite (simulating a redrawn
/// progress bar) must read back as its FINAL rendered state. The
/// previous ANSI-strip + `.lines()` implementation left the embedded
/// `\r` as a literal character, so the overwritten "10%" text would
/// still appear verbatim in the response.
#[tokio::test]
async fn handle_read_pane_renders_cr_overwrite_to_final_state_via_handler() {
    let pane_id = 102;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        pane.scrollback
            .lock()
            .unwrap()
            .write(b"Progress: 10%\rProgress: 100%\r\n");
        m.public_pane_id(pane_id)
    };

    let req = ReadPaneMsg {
        public_pane_id,
        lines: 50,
    };
    let msg = MuxMessage::control(MessageType::ReadPane, 0, &req);
    let result = handle_read_pane(&msg, &mgr)
        .await
        .expect("read should succeed");

    assert!(
        result.text.contains("Progress: 100%"),
        "got {:?}",
        result.text
    );
    assert!(
        !result.text.contains("10%"),
        "overwritten content must not leak through, got {:?}",
        result.text
    );
}

// ---- SendText (AC-2) ----

#[tokio::test]
async fn handle_send_text_writes_exact_bytes_and_returns_pre_write_watermark() {
    let pane_id = 200;
    let (mgr, sid, wid, captured) = setup_session_with_capturing_pane(pane_id).await;
    let public_pane_id = {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        pane.agent_status.lock().unwrap().revision = 9;
        m.public_pane_id(pane_id)
    };

    let req = SendTextMsg {
        public_pane_id,
        bytes: b"hello agent".to_vec(),
    };
    let msg = MuxMessage::control(MessageType::SendText, 0, &req);
    let result = handle_send_text(&msg, &mgr)
        .await
        .expect("send should succeed");

    assert_eq!(result.revision_watermark, 9);
    assert_eq!(
        captured.lock().unwrap().as_slice(),
        b"hello agent",
        "must write exactly the given bytes, no trailing newline added"
    );
}

#[tokio::test]
async fn handle_send_text_rejects_nul_without_writing() {
    let pane_id = 201;
    let (mgr, sid, wid, captured) = setup_session_with_capturing_pane(pane_id).await;
    let public_pane_id = mgr.lock().await.public_pane_id(pane_id);
    let _ = (sid, wid);

    let req = SendTextMsg {
        public_pane_id,
        bytes: b"has\0nul".to_vec(),
    };
    let msg = MuxMessage::control(MessageType::SendText, 0, &req);
    let err = handle_send_text(&msg, &mgr).await.unwrap_err();

    assert_eq!(err.kind, AgentApiErrorKind::InvalidInput);
    assert!(
        captured.lock().unwrap().is_empty(),
        "NUL-containing input must not be written"
    );
}

#[tokio::test]
async fn handle_send_text_rejects_oversize_without_writing() {
    let pane_id = 202;
    let (mgr, sid, wid, captured) = setup_session_with_capturing_pane(pane_id).await;
    let public_pane_id = mgr.lock().await.public_pane_id(pane_id);
    let _ = (sid, wid);

    let req = SendTextMsg {
        public_pane_id,
        bytes: vec![b'a'; SEND_MAX_BYTES + 1],
    };
    let msg = MuxMessage::control(MessageType::SendText, 0, &req);
    let err = handle_send_text(&msg, &mgr).await.unwrap_err();

    assert_eq!(err.kind, AgentApiErrorKind::InvalidInput);
    assert!(
        captured.lock().unwrap().is_empty(),
        "oversize input must not be written"
    );
}

#[tokio::test]
async fn handle_send_text_unknown_pane_errors() {
    let (mgr, _sid, _wid) = setup_session_with_pane(1).await;
    let req = SendTextMsg {
        public_pane_id: "deadbeef00000000-999".to_string(),
        bytes: b"hi".to_vec(),
    };
    let msg = MuxMessage::control(MessageType::SendText, 0, &req);
    let err = handle_send_text(&msg, &mgr).await.unwrap_err();
    assert_eq!(err.kind, AgentApiErrorKind::UnknownPane);
}

/// A `Write` impl that signals `started` (a oneshot the async test can
/// `.await`) the moment it is entered, then BLOCKS synchronously on
/// `unblock_rx` until the test releases it — simulating a stalled /
/// non-consuming child on the other end of the PTY.
struct StallingWriter {
    started: Option<tokio::sync::oneshot::Sender<()>>,
    unblock_rx: std::sync::mpsc::Receiver<()>,
}
impl std::io::Write for StallingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(tx) = self.started.take() {
            let _ = tx.send(());
        }
        let _ = self.unblock_rx.recv();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// AC-3 (task0011 REWORK): `handle_send_text` releases the
/// session-manager lock BEFORE performing the PTY write. Pane A's
/// writer blocks synchronously until the test releases it; while it
/// is blocked, a concurrent `handle_read_pane` on a DIFFERENT pane
/// (same session, same manager lock) must complete well inside a
/// bounded timeout — proving the manager lock was already free. Under
/// the old implementation (lock held across `write_input`), this
/// would hang until the timeout fired.
#[tokio::test]
async fn handle_send_text_releases_manager_lock_before_slow_write() {
    let pane_a = 210;
    let pane_b = 211;

    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let (unblock_tx, unblock_rx) = std::sync::mpsc::channel::<()>();

    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();

        let (tx_a, _rx_a) = mpsc::channel(1);
        let target_a: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx_a)));
        let pane_a_obj = MuxPane::new_test_with_writer(
            pane_a,
            80,
            24,
            target_a,
            Box::new(StallingWriter {
                started: Some(started_tx),
                unblock_rx,
            }),
        );
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a_obj);

        let (tx_b, _rx_b) = mpsc::channel(1);
        let target_b: SharedOutputTarget =
            Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx_b)));
        add_pane(&mut m, sid, wid, pane_b, target_b);
    }

    let public_a = mgr.lock().await.public_pane_id(pane_a);
    let public_b = mgr.lock().await.public_pane_id(pane_b);

    let send_req = SendTextMsg {
        public_pane_id: public_a,
        bytes: b"hi".to_vec(),
    };
    let send_msg = MuxMessage::control(MessageType::SendText, 0, &send_req);
    let mgr_for_send = mgr.clone();
    let send_task = tokio::spawn(async move { handle_send_text(&send_msg, &mgr_for_send).await });

    // Wait until the write has actually started — the manager lock is
    // dropped BEFORE the write is invoked (see `handle_send_text`), so
    // this also proves the lock is already free by this point.
    started_rx.await.expect("write must start");

    // While pane A's write is still blocked, ReadPane on the
    // DIFFERENT pane B must complete promptly: it needs the same
    // manager lock, which must be free.
    let read_req = ReadPaneMsg {
        public_pane_id: public_b,
        lines: 10,
    };
    let read_msg = MuxMessage::control(MessageType::ReadPane, 0, &read_req);
    tokio::time::timeout(
        std::time::Duration::from_millis(500),
        handle_read_pane(&read_msg, &mgr),
    )
    .await
    .expect("ReadPane on a different pane must not be blocked by pane A's stalled write")
    .expect("read should succeed");

    // Release the stalled write and let SendText finish.
    unblock_tx.send(()).expect("unblock writer");
    send_task
        .await
        .expect("task join")
        .expect("send should succeed");
}

/// AC-5 (task0011 REWORK): `handle_send_text` still writes bytes
/// atomically per request — two concurrent sends to the SAME pane
/// must not interleave. `writer_handle` clones share the pane's
/// single `std::sync::Mutex`-guarded writer, so the second send's
/// `write_via_writer_handle` call blocks on that mutex until the
/// first send's write+flush fully completes, even though both calls
/// run on the (lock-free, per task0011 AC-3) blocking-pool write path.
#[tokio::test]
async fn handle_send_text_concurrent_sends_to_same_pane_do_not_interleave() {
    struct BlockFirstWriter {
        first_call_done: bool,
        started_first: Option<tokio::sync::oneshot::Sender<()>>,
        unblock_first_rx: Option<std::sync::mpsc::Receiver<()>>,
        started_second: Option<tokio::sync::oneshot::Sender<()>>,
        captured: Arc<StdMutex<Vec<u8>>>,
    }
    impl std::io::Write for BlockFirstWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if !self.first_call_done {
                self.first_call_done = true;
                if let Some(tx) = self.started_first.take() {
                    let _ = tx.send(());
                }
                if let Some(rx) = self.unblock_first_rx.take() {
                    let _ = rx.recv();
                }
            } else if let Some(tx) = self.started_second.take() {
                let _ = tx.send(());
            }
            self.captured.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let pane_id = 212;
    let (started_first_tx, started_first_rx) = tokio::sync::oneshot::channel::<()>();
    let (unblock_first_tx, unblock_first_rx) = std::sync::mpsc::channel::<()>();
    let (started_second_tx, started_second_rx) = tokio::sync::oneshot::channel::<()>();
    let captured: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));

    let mgr = Arc::new(Mutex::new(SessionManager::new()));
    {
        let mut m = mgr.lock().await;
        let sid = m.create_session("default".to_string());
        let wid = m.create_window(sid, "shell".to_string()).unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let target: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Connected(tx)));
        let pane = MuxPane::new_test_with_writer(
            pane_id,
            80,
            24,
            target,
            Box::new(BlockFirstWriter {
                first_call_done: false,
                started_first: Some(started_first_tx),
                unblock_first_rx: Some(unblock_first_rx),
                started_second: Some(started_second_tx),
                captured: captured.clone(),
            }),
        );
        m.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane);
    }
    let public_pane_id = mgr.lock().await.public_pane_id(pane_id);

    let req1 = SendTextMsg {
        public_pane_id: public_pane_id.clone(),
        bytes: b"AAAA".to_vec(),
    };
    let msg1 = MuxMessage::control(MessageType::SendText, 0, &req1);
    let mgr1 = mgr.clone();
    let task1 = tokio::spawn(async move { handle_send_text(&msg1, &mgr1).await });
    started_first_rx.await.expect("first write must start");

    let req2 = SendTextMsg {
        public_pane_id,
        bytes: b"BBBB".to_vec(),
    };
    let msg2 = MuxMessage::control(MessageType::SendText, 0, &req2);
    let mgr2 = mgr.clone();
    let task2 = tokio::spawn(async move { handle_send_text(&msg2, &mgr2).await });

    // The second send must NOT be able to enter its write while the
    // first is still stalled inside its own write — it is blocked on
    // the shared std::sync::Mutex, not merely racing for CPU time.
    let raced_in_early =
        tokio::time::timeout(std::time::Duration::from_millis(150), started_second_rx).await;
    assert!(
        raced_in_early.is_err(),
        "second send must not start its write while the first is still in progress"
    );

    // Release the first write; both complete in order.
    unblock_first_tx.send(()).expect("unblock first writer");
    task1
        .await
        .expect("task1 join")
        .expect("first send should succeed");
    task2
        .await
        .expect("task2 join")
        .expect("second send should succeed");

    assert_eq!(
        captured.lock().unwrap().as_slice(),
        b"AAAABBBB",
        "concurrent sends to the same pane must not interleave bytes"
    );
}

// ---- WaitAgentState (AC-3, AC-4, AC-5) ----

#[tokio::test]
async fn wait_agent_state_succeeds_immediately_when_state_already_in_set() {
    let pane_id = 300;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        let mut st = pane.agent_status.lock().unwrap();
        st.state = Some(CoreAgentState::Blocked);
        st.revision = 3;
        drop(st);
        m.public_pane_id(pane_id)
    };

    let req = WaitAgentStateMsg {
        public_pane_id,
        states: vec![AgentState::Blocked, AgentState::Done],
        timeout_ms: 1000,
        after_revision: None,
    };
    let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
    let result = handle_wait_agent_state(&msg, &mgr)
        .await
        .expect("wait should succeed immediately");
    assert_eq!(result.state, AgentState::Blocked);
    assert_eq!(result.revision, 3);
}

#[tokio::test]
async fn wait_agent_state_no_state_yet_blocks_until_report_then_matches() {
    let pane_id = 301;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = mgr.lock().await.public_pane_id(pane_id);
    // Precondition: pane has no agent state yet.
    {
        let m = mgr.lock().await;
        assert!(
            get_pane(&m, sid, wid, pane_id)
                .agent_status
                .lock()
                .unwrap()
                .state
                .is_none()
        );
    }

    let req = WaitAgentStateMsg {
        public_pane_id,
        states: vec![AgentState::Working],
        timeout_ms: 5000,
        after_revision: None,
    };
    let mgr_clone = mgr.clone();
    let handle = tokio::spawn(async move {
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        handle_wait_agent_state(&msg, &mgr_clone).await
    });

    wait_until(|| {
        mgr.try_lock()
            .ok()
            .map(|m| {
                !get_pane(&m, sid, wid, pane_id)
                    .agent_waiters
                    .lock()
                    .unwrap()
                    .is_empty()
            })
            .unwrap_or(false)
    })
    .await;

    // Now report a qualifying accepted state change and re-evaluate
    // (mirrors what `mux::daemon::apply_agent_status_report` calls after
    // every accepted OSC report).
    {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        {
            let mut st = pane.agent_status.lock().unwrap();
            st.state = Some(CoreAgentState::Working);
            st.revision = 1;
        }
        reevaluate_agent_waiters(pane);
    }

    let result = handle.await.unwrap().expect("wait should resolve");
    assert_eq!(result.state, AgentState::Working);
    assert_eq!(result.revision, 1);
}

#[tokio::test]
async fn wait_agent_state_after_revision_does_not_satisfy_at_or_below_watermark() {
    let pane_id = 302;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        let mut st = pane.agent_status.lock().unwrap();
        st.state = Some(CoreAgentState::Done);
        st.revision = 5;
        drop(st);
        m.public_pane_id(pane_id)
    };

    let req = WaitAgentStateMsg {
        public_pane_id,
        states: vec![AgentState::Done],
        timeout_ms: 5000,
        after_revision: Some(5), // current revision (5) must NOT satisfy
    };
    let mgr_clone = mgr.clone();
    let handle = tokio::spawn(async move {
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        handle_wait_agent_state(&msg, &mgr_clone).await
    });

    // The immediate check must have registered a waiter (not resolved
    // immediately), since revision (5) is not > after_revision (5).
    wait_until(|| {
        mgr.try_lock()
            .ok()
            .map(|m| {
                !get_pane(&m, sid, wid, pane_id)
                    .agent_waiters
                    .lock()
                    .unwrap()
                    .is_empty()
            })
            .unwrap_or(false)
    })
    .await;

    // A same-state re-report bumps the revision past the watermark —
    // now it must satisfy (send-then-wait linearization, AC-4).
    {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        pane.agent_status.lock().unwrap().revision = 6;
        reevaluate_agent_waiters(pane);
    }

    let result = handle
        .await
        .unwrap()
        .expect("wait should resolve after revision bump");
    assert_eq!(result.state, AgentState::Done);
    assert_eq!(result.revision, 6);
}

#[tokio::test]
async fn wait_agent_state_times_out_when_condition_never_met() {
    let pane_id = 303;
    let (mgr, _sid, _wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = mgr.lock().await.public_pane_id(pane_id);
    let req = WaitAgentStateMsg {
        public_pane_id,
        states: vec![AgentState::Done],
        timeout_ms: 20,
        after_revision: None,
    };
    let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
    let err = handle_wait_agent_state(&msg, &mgr).await.unwrap_err();
    assert_eq!(err.kind, AgentApiErrorKind::Timeout);
}

#[tokio::test]
async fn wait_agent_state_pane_destroyed_resolves_pane_gone() {
    let pane_id = 304;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = mgr.lock().await.public_pane_id(pane_id);

    let req = WaitAgentStateMsg {
        public_pane_id,
        states: vec![AgentState::Done],
        timeout_ms: 5000,
        after_revision: None,
    };
    let mgr_clone = mgr.clone();
    let handle = tokio::spawn(async move {
        let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
        handle_wait_agent_state(&msg, &mgr_clone).await
    });

    wait_until(|| {
        mgr.try_lock()
            .ok()
            .map(|m| {
                !get_pane(&m, sid, wid, pane_id)
                    .agent_waiters
                    .lock()
                    .unwrap()
                    .is_empty()
            })
            .unwrap_or(false)
    })
    .await;

    {
        let m = mgr.lock().await;
        let pane = get_pane(&m, sid, wid, pane_id);
        fail_agent_waiters_pane_gone(pane);
    }

    let err = handle
        .await
        .unwrap()
        .expect_err("wait must fail once the pane is gone");
    assert_eq!(err.kind, AgentApiErrorKind::PaneGone);
}

#[tokio::test]
async fn wait_agent_state_unknown_pane_errors() {
    let (mgr, _sid, _wid) = setup_session_with_pane(1).await;
    let req = WaitAgentStateMsg {
        public_pane_id: "deadbeef00000000-999".to_string(),
        states: vec![AgentState::Idle],
        timeout_ms: 1000,
        after_revision: None,
    };
    let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
    let err = handle_wait_agent_state(&msg, &mgr).await.unwrap_err();
    assert_eq!(err.kind, AgentApiErrorKind::UnknownPane);
}

#[tokio::test]
async fn wait_agent_state_empty_states_is_invalid_input() {
    let pane_id = 305;
    let (mgr, _sid, _wid) = setup_session_with_pane(pane_id).await;
    let public_pane_id = mgr.lock().await.public_pane_id(pane_id);
    let req = WaitAgentStateMsg {
        public_pane_id,
        states: vec![],
        timeout_ms: 1000,
        after_revision: None,
    };
    let msg = MuxMessage::control(MessageType::WaitAgentState, 0, &req);
    let err = handle_wait_agent_state(&msg, &mgr).await.unwrap_err();
    assert_eq!(err.kind, AgentApiErrorKind::InvalidInput);
}

/// AC-5: client disconnect discards the waiter. Modeled at the
/// data-structure level per the Test Notes (handler-level, in-memory,
/// no live socket): dropping the `oneshot::Receiver` is exactly what a
/// disconnected CLI connection's abandoned future does, and
/// `reevaluate_agent_waiters`'s cleanup pass removes any waiter whose
/// responder is already closed — independent of whether the state
/// ever changes.
#[tokio::test]
async fn reevaluate_agent_waiters_discards_waiter_with_closed_receiver() {
    let pane_id = 306;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let m = mgr.lock().await;
    let pane = get_pane(&m, sid, wid, pane_id);

    let (tx, rx) = oneshot::channel();
    pane.agent_waiters.lock().unwrap().push(AgentWaiter {
        states: vec![CoreAgentState::Done],
        after_revision: None,
        responder: Some(tx),
    });
    drop(rx); // simulate client disconnect

    assert_eq!(pane.agent_waiters.lock().unwrap().len(), 1);
    reevaluate_agent_waiters(pane);
    assert!(
        pane.agent_waiters.lock().unwrap().is_empty(),
        "closed-receiver waiter must be discarded"
    );
}

/// A waiter whose `states` set does not match the current state stays
/// registered across a re-evaluation pass (no spurious firing/removal).
#[tokio::test]
async fn reevaluate_agent_waiters_keeps_non_matching_waiter() {
    let pane_id = 307;
    let (mgr, sid, wid) = setup_session_with_pane(pane_id).await;
    let m = mgr.lock().await;
    let pane = get_pane(&m, sid, wid, pane_id);
    pane.agent_status.lock().unwrap().state = Some(CoreAgentState::Idle);

    let (tx, _rx) = oneshot::channel();
    pane.agent_waiters.lock().unwrap().push(AgentWaiter {
        states: vec![CoreAgentState::Done],
        after_revision: None,
        responder: Some(tx),
    });

    reevaluate_agent_waiters(pane);
    assert_eq!(
        pane.agent_waiters.lock().unwrap().len(),
        1,
        "non-matching waiter must remain registered"
    );
}
