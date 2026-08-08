use super::*;
use crate::mux::ipc::reattach::collect_reattach_data;

// ── G2 starvation guard (AC-2, mux-window-switch-output-hang task0004
// rework, review round 3 findings `dd23cfc388062939`/
// `5c01ffb8d53dc9f7`): deterministic coverage for
// `allow_client_message_arm`, with no live connection, timing, or
// scheduling involved — this is what actually pins the
// bounded-iterations guarantee AC-2 requires (see that function's own
// doc for why the accompanying live-connection test cannot reliably
// force the underlying race either way). ──

/// When there is no deferred work outstanding, the client arm is
/// always allowed — the counter is irrelevant, including past the
/// quota. Ordinary traffic (no pending deferral) sees ZERO behavior
/// change from this guard.
#[test]
fn allow_client_message_arm_true_when_no_deferred_work_regardless_of_counter() {
    assert!(allow_client_message_arm(false, 0));
    assert!(allow_client_message_arm(false, CLIENT_MSG_STARVATION_QUOTA));
    assert!(allow_client_message_arm(
        false,
        CLIENT_MSG_STARVATION_QUOTA + 100
    ));
}

/// While deferred work IS outstanding: the client arm remains allowed
/// for exactly `CLIENT_MSG_STARVATION_QUOTA` consecutive wins, then is
/// excluded on the very next one — the bounded-iterations guarantee
/// AC-2 requires. Regression shape: reverting the guard to
/// unconditionally `true` (the pre-G2-fix behavior) makes this fail at
/// `n == CLIENT_MSG_STARVATION_QUOTA` (confirmed during development).
#[test]
fn allow_client_message_arm_true_for_quota_iterations_then_excludes_on_the_next() {
    for n in 0..CLIENT_MSG_STARVATION_QUOTA {
        assert!(
            allow_client_message_arm(true, n),
            "iteration {n} (< quota) must still allow the client arm"
        );
    }
    assert!(
        !allow_client_message_arm(true, CLIENT_MSG_STARVATION_QUOTA),
        "the quota'th consecutive iteration must exclude the client arm so the \
         reservation/drain arms are guaranteed a poll"
    );
    // Past the quota stays excluded too (the counter only grows further
    // if some other bug let it; the guard must not start allowing
    // again just because the count exceeded the quota by more).
    assert!(!allow_client_message_arm(
        true,
        CLIENT_MSG_STARVATION_QUOTA + 5
    ));
}

// ── G4 (AC-4, mux-window-switch-output-hang task0005, review round 4
// finding `e6ac2a334424ebd7`): deterministic coverage for
// `has_unforwarded_pane_output` — the queue -> channel -> client
// transition. A real `tokio::sync::mpsc` channel is used (so
// `Receiver::is_empty()` reflects genuine channel state), but nothing
// here depends on a live connection, timing, or scheduling. ──

/// The exact gap this rework closes: an item already admitted into
/// `pane_output_tx` (so the connection's OWN deferred-output bookkeeping
/// — `has_deferred_work` — has already gone back to `false`) but not yet
/// drained by the `chunk = pane_output_rx.recv()` arm must still count
/// as pending output. Before this rework, `has_deferred_work` alone
/// would have reported `false` here, letting the client-message arm's
/// guard `allow_client_message_arm` return `true` unconditionally.
#[tokio::test]
async fn has_unforwarded_pane_output_true_when_channel_holds_an_undelivered_item_even_with_deferred_bookkeeping_clear()
 {
    let (tx, rx) = mpsc::channel::<PtyOutputChunk>(4);
    tx.send(PtyOutputChunk::pty_output(1, vec![1, 2, 3]))
        .await
        .unwrap();
    assert!(
        has_unforwarded_pane_output(false, &rx),
        "AC-4: an item admitted into the channel but not yet drained must still \
         count as pending output, even when the connection's own deferred-output \
         bookkeeping (`has_deferred_work`) alone already reports none outstanding"
    );
}

/// The negative case: an empty channel AND no deferred bookkeeping means
/// genuinely nothing is pending — the guard must not be held open
/// forever by a stale/incorrect signal.
#[tokio::test]
async fn has_unforwarded_pane_output_false_when_channel_and_deferred_bookkeeping_both_clear() {
    let (_tx, rx) = mpsc::channel::<PtyOutputChunk>(4);
    assert!(!has_unforwarded_pane_output(false, &rx));
}

/// `has_deferred_work == true` short-circuits to `true` regardless of
/// channel state — the ORIGINAL signal (reservation pending / deferred
/// queue non-empty) must keep working exactly as before; this rework
/// only ADDS a second way to be `true`, it never removes the first.
#[tokio::test]
async fn has_unforwarded_pane_output_true_when_deferred_work_outstanding_even_with_empty_channel() {
    let (_tx, rx) = mpsc::channel::<PtyOutputChunk>(4);
    assert!(has_unforwarded_pane_output(true, &rx));
}

// ── AC-5 (mux-window-switch-output-hang task0005, medium finding
// connection.rs:820, review round 4): deterministic coverage for
// `next_client_msg_starvation_count`'s increment/reset arithmetic. ──

/// The client arm running while pending output was already outstanding
/// increments the count.
#[test]
fn next_client_msg_starvation_count_increments_when_client_arm_ran_with_pending_output() {
    assert_eq!(
        next_client_msg_starvation_count(true, true, 0),
        1,
        "first consecutive win increments from 0"
    );
    assert_eq!(
        next_client_msg_starvation_count(true, true, 3),
        4,
        "a subsequent consecutive win increments further"
    );
}

/// A different arm winning resets the count to 0, regardless of how
/// high it had climbed — the one-iteration exclusion never compounds.
#[test]
fn next_client_msg_starvation_count_resets_when_a_different_arm_won() {
    assert_eq!(
        next_client_msg_starvation_count(false, true, CLIENT_MSG_STARVATION_QUOTA),
        0
    );
}

/// No pending output outstanding resets the count to 0 even if the
/// client arm itself won — ordinary traffic (no deferral pending) sees
/// ZERO behavior change from this guard.
#[test]
fn next_client_msg_starvation_count_resets_when_no_pending_output_outstanding() {
    assert_eq!(
        next_client_msg_starvation_count(true, false, CLIENT_MSG_STARVATION_QUOTA),
        0
    );
}

/// Full cycle: the counter climbs for exactly `CLIENT_MSG_STARVATION_QUOTA`
/// consecutive client-arm wins (each still allowed by
/// `allow_client_message_arm`), reaches the quota (now excluded), and
/// then — once a different arm wins the excluded iteration — resets to 0
/// and the client arm is allowed again. Wires the two extracted pure
/// functions together exactly as `handle_connection`'s loop does, without
/// any live connection.
#[test]
fn next_client_msg_starvation_count_full_quota_cycle_then_reset() {
    let mut count = 0u32;
    for _ in 0..CLIENT_MSG_STARVATION_QUOTA {
        assert!(
            allow_client_message_arm(true, count),
            "must still be allowed below the quota"
        );
        count = next_client_msg_starvation_count(true, true, count);
    }
    assert_eq!(count, CLIENT_MSG_STARVATION_QUOTA);
    assert!(
        !allow_client_message_arm(true, count),
        "quota reached: the client arm must be excluded for exactly one iteration"
    );
    // The excluded iteration: some other arm wins (`took_client_arm ==
    // false`), resetting the counter for the next round.
    count = next_client_msg_starvation_count(false, true, count);
    assert_eq!(count, 0, "the excluded iteration resets the counter");
    assert!(
        allow_client_message_arm(true, count),
        "the very next iteration reverts to prioritizing client messages"
    );
}

fn chunk(pane_id: u32, data: &[u8]) -> PtyOutputChunk {
    PtyOutputChunk::pty_output(pane_id, data.to_vec())
}

fn exit_chunk(pane_id: u32) -> PtyOutputChunk {
    PtyOutputChunk::pty_output(pane_id, Vec::new())
}

fn snapshot_chunk(pane_id: u32, data: &[u8]) -> PtyOutputChunk {
    PtyOutputChunk::snapshot(pane_id, data.to_vec())
}

#[test]
fn merge_single_chunk() {
    let chunks = vec![chunk(1, b"hello")];
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].pane_id, 1);
    assert_eq!(merged[0].data, b"hello");
}

#[test]
fn merge_same_pane_consecutive() {
    let chunks = vec![chunk(1, b"hel"), chunk(1, b"lo"), chunk(1, b"!")];
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].pane_id, 1);
    assert_eq!(merged[0].data, b"hello!");
}

#[test]
fn merge_different_panes_not_merged() {
    let chunks = vec![chunk(1, b"a"), chunk(2, b"b"), chunk(1, b"c")];
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].pane_id, 1);
    assert_eq!(merged[0].data, b"a");
    assert_eq!(merged[1].pane_id, 2);
    assert_eq!(merged[1].data, b"b");
    assert_eq!(merged[2].pane_id, 1);
    assert_eq!(merged[2].data, b"c");
}

#[test]
fn merge_exit_signal_not_merged() {
    let chunks = vec![chunk(1, b"data"), exit_chunk(1)];
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].data, b"data");
    assert!(merged[1].data.is_empty());
}

#[test]
fn merge_exit_signal_mid_batch() {
    // pane 1 data, pane 1 exit, pane 1 data (from new process or leftover)
    let chunks = vec![chunk(1, b"before"), exit_chunk(1), chunk(1, b"after")];
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0].data, b"before");
    assert!(merged[1].data.is_empty());
    assert_eq!(merged[2].data, b"after");
}

#[test]
fn merge_mixed_pane_ordering_preserved() {
    // Interleaved panes: A, B, A, B — ordering must be preserved
    let chunks = vec![
        chunk(1, b"a1"),
        chunk(1, b"a2"),
        chunk(2, b"b1"),
        chunk(2, b"b2"),
        chunk(1, b"a3"),
        chunk(3, b"c1"),
    ];
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(merged.len(), 4);
    assert_eq!(merged[0].pane_id, 1);
    assert_eq!(merged[0].data, b"a1a2");
    assert_eq!(merged[1].pane_id, 2);
    assert_eq!(merged[1].data, b"b1b2");
    assert_eq!(merged[2].pane_id, 1);
    assert_eq!(merged[2].data, b"a3");
    assert_eq!(merged[3].pane_id, 3);
    assert_eq!(merged[3].data, b"c1");
}

// ── merge / drain efficiency metrics (IPC frame-count regression guard) ──
//
// The daemon drains up to `DRAIN_BATCH_LIMIT` chunks per select! iteration
// and merges consecutive same-pane chunks into one before sending. Each
// surviving merged chunk becomes one IPC frame (one base64-encoded APC/OSC
// envelope on the bridge transport), so "output chunk count" is the
// frame-count metric the perf work optimizes. These tests pin the
// input-vs-output chunk reduction deterministically (counts only, no
// timing) so a regression that stops coalescing is caught.

/// Total bytes are conserved across the merge (no data lost/duplicated)
/// while the chunk count collapses — the core efficiency invariant.
fn total_bytes(chunks: &[PtyOutputChunk]) -> usize {
    chunks.iter().map(|c| c.data.len()).sum()
}

#[test]
fn merge_efficiency_single_pane_n_to_one() {
    // N consecutive same-pane chunks → exactly 1 IPC frame.
    for n in [2usize, 8, 64] {
        let chunks: Vec<PtyOutputChunk> = (0..n).map(|_| chunk(1, b"abcd")).collect();
        let bytes_in = total_bytes(&chunks);
        let merged = merge_consecutive_chunks(chunks);
        assert_eq!(
            merged.len(),
            1,
            "{n} consecutive same-pane chunks must merge to 1 frame"
        );
        // Frame-count reduction: N inputs → 1 output.
        assert_eq!(total_bytes(&merged), bytes_in, "no bytes lost in merge");
        assert_eq!(merged[0].data.len(), n * 4, "all payloads concatenated");
    }
}

#[test]
fn merge_efficiency_only_consecutive_same_pane_collapses() {
    // Mixed panes: only *runs* of the same pane collapse. Six input chunks
    // across runs [1,1,1 | 2,2 | 1] → three output frames (one per run).
    let chunks = vec![
        chunk(1, b"a"),
        chunk(1, b"b"),
        chunk(1, b"c"),
        chunk(2, b"d"),
        chunk(2, b"e"),
        chunk(1, b"f"),
    ];
    let bytes_in = total_bytes(&chunks);
    let input_count = chunks.len();
    let merged = merge_consecutive_chunks(chunks);

    // 6 input chunks → 3 output frames (a 50% frame reduction here).
    assert_eq!(input_count, 6);
    assert_eq!(
        merged.len(),
        3,
        "only consecutive same-pane runs collapse; interleaving stays split"
    );
    assert_eq!(total_bytes(&merged), bytes_in, "no bytes lost");
    assert_eq!(merged[0].pane_id, 1);
    assert_eq!(merged[0].data, b"abc");
    assert_eq!(merged[1].pane_id, 2);
    assert_eq!(merged[1].data, b"de");
    assert_eq!(merged[2].pane_id, 1);
    assert_eq!(merged[2].data, b"f");
}

#[test]
fn merge_efficiency_alternating_panes_no_reduction() {
    // Worst case: strictly alternating panes never collapse, so the frame
    // count is unchanged (input count == output count). This pins the
    // lower bound of the optimization (merge never *increases* frames).
    let chunks = vec![
        chunk(1, b"a"),
        chunk(2, b"b"),
        chunk(1, b"c"),
        chunk(2, b"d"),
    ];
    let input_count = chunks.len();
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(
        merged.len(),
        input_count,
        "alternating panes cannot merge; frame count is unchanged"
    );
}

#[test]
fn merge_efficiency_full_drain_batch_single_pane() {
    // A full drain batch (DRAIN_BATCH_LIMIT chunks) from one busy pane —
    // the bulk-output hot path — collapses to a single IPC frame. This is
    // the headline win the perf work relies on: 64 drained chunks → 1 send.
    let chunks: Vec<PtyOutputChunk> = (0..DRAIN_BATCH_LIMIT)
        .map(|_| chunk(42, &[0u8; 1024]))
        .collect();
    let bytes_in = total_bytes(&chunks);
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(
        merged.len(),
        1,
        "a full {DRAIN_BATCH_LIMIT}-chunk single-pane drain merges to 1 frame"
    );
    assert_eq!(total_bytes(&merged), bytes_in);
    assert_eq!(
        merged[0].data.len(),
        DRAIN_BATCH_LIMIT * 1024,
        "merged frame carries the whole batch"
    );
}

/// Phase 2 (FR1, FR5): a `Snapshot`-kind chunk inserted between
/// same-pane `PtyOutput` chunks MUST NOT be folded into either
/// neighbour. The on-wire framing for `Snapshot` is
/// `MessageType::Snapshot` while neighbours are `MessageType::PtyOutput`;
/// collapsing them would smuggle snapshot bytes into a live-input frame
/// (or vice versa) and break the routing to the off-thread replay path.
#[test]
fn merge_does_not_fold_across_kind() {
    let chunks = vec![
        chunk(1, b"pre1"),
        chunk(1, b"pre2"),
        snapshot_chunk(1, b"SNAPSHOT"),
        chunk(1, b"post"),
    ];
    let merged = merge_consecutive_chunks(chunks);
    // Expected: [merged-PtyOutput("pre1pre2"), Snapshot("SNAPSHOT"), PtyOutput("post")]
    assert_eq!(merged.len(), 3, "kind boundary must split the run");
    assert_eq!(merged[0].pane_id, 1);
    assert_eq!(merged[0].kind, ChunkKind::PtyOutput);
    assert_eq!(merged[0].data, b"pre1pre2");
    assert_eq!(merged[1].pane_id, 1);
    assert_eq!(merged[1].kind, ChunkKind::Snapshot);
    assert_eq!(merged[1].data, b"SNAPSHOT");
    assert_eq!(merged[2].pane_id, 1);
    assert_eq!(merged[2].kind, ChunkKind::PtyOutput);
    assert_eq!(merged[2].data, b"post");
}

/// Phase 2 (FR5): two consecutive `Snapshot`-kind chunks for the same
/// pane MUST remain separate frames. Each `RequestPaneSnapshot` reply
/// is one snapshot payload; concatenating two snapshot payloads on the
/// wire would produce a malformed single frame whose recipient cannot
/// segment them. (In practice the daemon only emits one snapshot per
/// request, but the merge logic must not assume that.)
#[test]
fn merge_does_not_collapse_consecutive_snapshots() {
    let chunks = vec![snapshot_chunk(1, b"SNAP-A"), snapshot_chunk(1, b"SNAP-B")];
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(merged.len(), 2, "snapshots are never coalesced");
    assert_eq!(merged[0].data, b"SNAP-A");
    assert_eq!(merged[0].kind, ChunkKind::Snapshot);
    assert_eq!(merged[1].data, b"SNAP-B");
    assert_eq!(merged[1].kind, ChunkKind::Snapshot);
}

#[test]
fn merge_efficiency_exit_signals_stay_separate() {
    // Exit signals (empty data) are never merged, so they always cost their
    // own frame even amid same-pane data. Pin this so the frame-count model
    // accounts for them: 3 data chunks + 1 exit (same pane) → 2 frames.
    let chunks = vec![
        chunk(1, b"x"),
        chunk(1, b"y"),
        chunk(1, b"z"),
        exit_chunk(1),
    ];
    let merged = merge_consecutive_chunks(chunks);
    assert_eq!(
        merged.len(),
        2,
        "same-pane data collapses to 1 frame; the exit signal is a 2nd frame"
    );
    assert_eq!(merged[0].data, b"xyz");
    assert!(
        merged[1].data.is_empty(),
        "exit signal preserved separately"
    );
}

// ---- mux-daemon-hot-upgrade task0004: MessageType::Upgrade CLI reply
// translation ----
//
// `upgrade_reply_to_message` is the pure core of `handle_cli_client`'s
// `MessageType::Upgrade` arm: given the accept loop's reply, decide what
// (if anything) goes back to the client. Extracted so these branches are
// unit-testable without a live connection or a real accept loop.

/// Successful preparation produces no explicit reply (the connection is
/// simply dropped once the process is replaced, IMPLEMENTATION.md D2).
#[test]
fn upgrade_reply_to_message_success_is_none() {
    assert!(upgrade_reply_to_message(Ok(Ok(()))).is_none());
}

/// AC-4: an abort reason reported by the accept loop becomes an `Error`
/// control message carrying that exact reason.
#[test]
fn upgrade_reply_to_message_abort_reason_becomes_error_message() {
    let msg = upgrade_reply_to_message(Ok(Err("disk full".to_string())))
        .expect("an abort reason must produce a reply message");
    assert_eq!(msg.msg_type, MessageType::Error);
    let payload: ErrorMsg = msg.decode_payload().unwrap();
    assert_eq!(payload.message, "disk full");
}

/// A closed reply channel (accept loop dropped it without answering,
/// e.g. mid-shutdown) still produces a client-facing `Error` message
/// rather than silently dropping the connection with no explanation.
#[tokio::test]
async fn upgrade_reply_to_message_channel_closed_becomes_generic_error() {
    let (reply_tx, reply_rx) = oneshot::channel::<Result<(), String>>();
    drop(reply_tx);
    let recv_result = reply_rx.await;
    assert!(recv_result.is_err(), "dropped sender must yield RecvError");

    let msg = upgrade_reply_to_message(recv_result)
        .expect("a closed reply channel must still produce a reply message");
    assert_eq!(msg.msg_type, MessageType::Error);
    let payload: ErrorMsg = msg.decode_payload().unwrap();
    assert!(!payload.message.is_empty());
}

// ── mux-window-switch-output-hang task0003 rework: connection-level
// coverage for AC-3 (starvation-freedom) and AC-4 (FR2 progress under a
// pending deferred snapshot) — review round 2 findings
// `dda847f76f68fea7`/`9361b9b42c69fb92` (round 1 also raised this; every
// prior test in this feature drove handlers directly and called
// `flush_deferred_output` by hand instead of the real `select!` loop). ──

use crate::mux::session::pane::{DetachReason, MuxPane, PaneOutputTarget, SharedOutputTarget};
use std::sync::Mutex as StdMutex;
use std::sync::atomic::Ordering as StdOrdering;

fn no_ack_slot() -> SharedUpgradeAckSlot {
    Arc::new(StdMutex::new(None))
}

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

/// task0001 (AC-4/AC-6): a `tokio::io::DuplexStream` wrapper whose
/// WRITE side (`poll_write`/`poll_flush`) starts unconditionally
/// failing once `fail_writes` is flipped `true` — deterministic,
/// test-controlled "the socket write failed" injection, sharper than
/// a saturated-but-not-broken duplex (AC-1's own harness): reads keep
/// working (delegated straight through) so the handshake/attach
/// sequence still completes normally before the test flips the flag.
struct FailableWriteStream {
    inner: tokio::io::DuplexStream,
    fail_writes: Arc<std::sync::atomic::AtomicBool>,
}

impl tokio::io::AsyncRead for FailableWriteStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for FailableWriteStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.fail_writes.load(StdOrdering::Relaxed) {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "test-injected write failure",
            )));
        }
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.fail_writes.load(StdOrdering::Relaxed) {
            return std::task::Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "test-injected write failure",
            )));
        }
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Read frames off `client` until a `PaneCreated` has been seen for
/// every id in `expected_pane_ids` (ignoring every other frame type
/// in between, e.g. `SnapshotRestore`).
async fn drain_until_pane_created(
    client: &mut Framed<tokio::io::DuplexStream, MuxCodec>,
    expected_pane_ids: &[u32],
) {
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
    while seen.len() < expected_pane_ids.len() {
        let msg = tokio::time::timeout(Duration::from_secs(5), client.next())
            .await
            .expect("must not hang waiting for reattach frames")
            .expect("stream must not end")
            .expect("frame must decode");
        if msg.msg_type == MessageType::PaneCreated && expected_pane_ids.contains(&msg.pane_id) {
            seen.insert(msg.pane_id);
        }
    }
}

/// AC-3/AC-4: drives the REAL `handle_connection` `select!` loop over a
/// duplex stream (mirroring `mux::daemon`'s own `handle_connection`
/// spawn test). Pane A's channel is saturated by 8 background OS threads
/// calling `blocking_send` exactly the way `pty_spawn.rs`'s reader
/// thread does — the scenario `flush_deferred_output`'s `try_send`/
/// `try_reserve` retries lose to systematically (G2). A single producer
/// thread and small payloads were tried first and did not reliably
/// reproduce the starvation (this test's own red/green history: it
/// failed reliably, and only with, this many concurrent producers and
/// this payload size before the AC-3 fix existed) — 8 threads racing
/// continuously and a 32KB payload per chunk make the channel
/// genuinely, continuously saturated rather than momentarily.
///
/// G4 rework (AC-4, mux-window-switch-output-hang task0004, review
/// round 3 finding `30cdb3b4400888fc`) — ACTUALLY IMPLEMENTED in
/// task0005 (review round 4 findings `e725f0403d431092` /
/// `9078d44a6c2897ec` / `ead53217898d6933`): task0004's own doc comment
/// and `test-docs/mux-window-switch-output-hang/task0004.tests.yaml`
/// AC-4 entry claimed this exact rework had already happened and had
/// been observed red-then-green. Neither was true — the code still sent
/// `RequestPaneSnapshot` for `PANE_B`, the assertion was still a
/// post-loop check, and no ordering flag existed anywhere in this file.
/// Round 4 caught the discrepancy directly against the source; see
/// `test-docs/mux-window-switch-output-hang/task0004.tests.yaml`'s AC-4
/// entry (corrected by task0005) for the full account. What follows is
/// the rework as actually implemented and observed red-then-green under
/// task0005: the `RequestPaneSnapshot` now targets PANE_A ITSELF — the
/// very pane whose reader threads are saturating the channel — while
/// `PtyInput` still targets the DIFFERENT pane B. This is SPEC Unit Test
/// 1's exact composition ("input to a *different* pane keeps flowing
/// while [the requesting] pane's snapshot is pending") AND SPEC Edge
/// case 1 ("snapshot requested for the exact pane producing the
/// high-volume output") at once — the pre-task0005 version of this test
/// requested PANE_B's own snapshot, so cross-pane input-vs-
/// snapshot-pending was never actually exercised. `input_processed_
/// before_snapshot` proves ORDER, not just eventual truth, for pane B's
/// input: captured at the exact moment the `Snapshot(A)` frame is
/// observed rather than re-derived after the loop ends, a build that
/// processes B's `PtyInput` only AFTER delivering A's snapshot (instead
/// of interleaved with it, as `route_message` does synchronously)
/// cannot pass this assertion merely by the time the test ends.
/// `output_flowed_before_snapshot` is WEAKER than that (task0006
/// rework, review round 5): it only proves SOME `PtyOutput(PANE_A)`
/// frame was observed before the `Snapshot(A)` frame, which the
/// pre-request channel saturation (8 background producer threads
/// already flooding pane A's channel before `RequestPaneSnapshot` is
/// even sent) guarantees trivially, regardless of whether the
/// connection keeps draining pane A's output correctly WHILE the
/// deferred snapshot is pending. It does not, by itself, distinguish
/// "kept draining throughout the wait" from "some output happened to
/// already be buffered/in flight before the request landed" — closing
/// that gap would require tagging payloads pre/post-request and
/// asserting on the tagged sequence, which this task's scope does not
/// include.
/// Confirmed load-bearing by reverting JUST the wait-loop match arm back
/// to `(Snapshot, PANE_B)` (the pre-task0005 target) while leaving
/// everything else — the retargeted `RequestPaneSnapshot`, the
/// at-the-moment `input_processed_before_snapshot`/
/// `output_flowed_before_snapshot` capture — as implemented: the test
/// hung and panicked ("must not hang") since the real `Snapshot(A)`
/// frame the server actually sends is never matched. Restored the
/// correct `(Snapshot, PANE_A)` arm afterward. The "strict same-moment
/// ordering" dimension specifically — capturing the flags at the exact
/// instant the `Snapshot(A)` frame is observed, rather than re-deriving
/// them after the loop ends — could NOT independently be forced red in
/// this environment (see `test-docs/mux-window-switch-output-hang/
/// task0005.tests.yaml`'s AC-1 entry for the honest account of why: by
/// this test's own construction, pane B's `PtyInput` is processed
/// almost immediately, so input landing before the snapshot is
/// guaranteed by timing, not something a regression could plausibly
/// invert here). Recorded accordingly — only the retargeting/matching
/// revert is claimed as observed red.
#[tokio::test]
async fn connection_level_deferred_snapshot_survives_sustained_saturation_and_input_keeps_flowing()
{
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    const PANE_B: u32 = 2;
    let captured_input: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
    {
        let mut mgr = session_manager.lock().await;
        let sid = mgr.create_session("default".to_string());
        let wid = mgr.create_window(sid, "shell".to_string()).unwrap();

        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);

        let target_b: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_b = MuxPane::new_test_with_writer(
            PANE_B,
            80,
            24,
            target_b,
            Box::new(CapturingWriter(captured_input.clone())),
        );
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_b);
    }

    let (server_stream, client_stream) = tokio::io::duplex(16 * 1024);
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        server_stream,
        session_manager.clone(),
        shutdown_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());

    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);
    let welcome_payload: WelcomeMsg = welcome.decode_payload().unwrap();
    let session_id = match welcome_payload {
        WelcomeMsg::Accepted { sessions, .. } => sessions[0].id,
        WelcomeMsg::Rejected { reason } => panic!("unexpected rejection: {reason}"),
    };

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A, PANE_B]).await;

    // Extract the connection's OWN `pane_output_tx` clone now that pane
    // A is Connected through it (installed by `collect_reattach_data`
    // during the Attach above) — the REAL channel `handle_connection`'s
    // own `select!` loop drains, not a stand-in.
    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    // Background OS thread saturating pane A's channel via
    // `blocking_send` — exactly the shape `pty_spawn.rs`'s reader
    // thread uses (AC-3's stated scenario).
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut producers = Vec::new();
    for _ in 0..8 {
        let stop_clone = stop.clone();
        let producer_tx = owned_tx.clone();
        producers.push(std::thread::spawn(move || {
            while !stop_clone.load(StdOrdering::Relaxed) {
                if producer_tx
                    .blocking_send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 32768]))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    // Give the producer time to actually saturate the channel (a
    // parked `blocking_send` waiter) before issuing the request this
    // test is about.
    tokio::time::sleep(Duration::from_millis(100)).await;

    client
        .send(MuxMessage {
            msg_type: MessageType::RequestPaneSnapshot,
            pane_id: PANE_A,
            payload: Vec::new(),
        })
        .await
        .unwrap();
    client
        .send(MuxMessage {
            msg_type: MessageType::PtyInput,
            pane_id: PANE_B,
            payload: b"hi".to_vec(),
        })
        .await
        .unwrap();

    let mut saw_output_for_pane_a = false;
    let mut saw_snapshot_for_pane_a = false;
    // AC-4 (task0005 rework, review round 4 finding G1): captured the
    // FIRST time the Snapshot(A) frame is observed, not re-derived
    // after the loop ends — see this test's own doc for why a
    // post-loop check cannot distinguish "processed before delivery"
    // from "processed only because the test kept reading afterward".
    let mut input_processed_before_snapshot = false;
    let mut output_flowed_before_snapshot = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !(saw_output_for_pane_a && saw_snapshot_for_pane_a) {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "AC-3/AC-4: connection must keep forwarding pane A's output AND \
             deliver pane A's own deferred snapshot within a bounded time despite \
             sustained saturating output; saw_output_for_pane_a={saw_output_for_pane_a} \
             saw_snapshot_for_pane_a={saw_snapshot_for_pane_a}"
        );
        let msg = tokio::time::timeout(remaining, client.next())
            .await
            .expect("must not hang")
            .expect("stream must not end")
            .expect("frame must decode");
        match (msg.msg_type, msg.pane_id) {
            (MessageType::PtyOutput, PANE_A) => saw_output_for_pane_a = true,
            (MessageType::Snapshot, PANE_A) => {
                // Record ordering at the EXACT moment this frame is
                // observed — captured_input/saw_output_for_pane_a may
                // both still change AFTER this point in a buggy build,
                // so evaluating them here (rather than post-loop) is
                // what actually proves "strictly before", not just
                // "eventually true by the time the test ends".
                input_processed_before_snapshot = *captured_input.lock().unwrap() == b"hi";
                output_flowed_before_snapshot = saw_output_for_pane_a;
                saw_snapshot_for_pane_a = true;
            }
            _ => {}
        }
    }

    // AC-4: at least one PtyOutput(PANE_A) frame must have already been
    // observed before the Snapshot(A) frame. Weaker than it may look
    // (task0006 rework, review round 5): the pre-request channel
    // saturation guarantees this trivially, so it does not by itself
    // distinguish "kept draining throughout the deferred-snapshot
    // wait" from "some output was already buffered/in flight before
    // the request landed" — see this test's own doc.
    assert!(
        output_flowed_before_snapshot,
        "AC-4: at least one PtyOutput(PANE_A) frame must be observed STRICTLY BEFORE \
         its deferred snapshot is delivered"
    );
    // AC-4: the PtyInput for pane B must have already been processed
    // (the write is synchronous inside `route_message`) STRICTLY
    // BEFORE pane A's deferred snapshot was delivered — not merely by
    // the time this loop happens to end.
    assert!(
        input_processed_before_snapshot,
        "AC-4: PtyInput for pane B must be processed STRICTLY BEFORE pane A's \
         deferred snapshot is delivered, not merely by the time the test ends"
    );

    stop.store(true, StdOrdering::Relaxed);
    drop(client);
    conn_task.abort();
    // Dropping `client` closes the duplex pair, which fails the
    // connection's own sends/receives and ends `handle_connection`,
    // which drops `pane_output_rx` — every subsequent `blocking_send`
    // on `producer_tx` then observes `Closed` and the thread exits.
    // `spawn_blocking` so this async test doesn't itself block on a
    // native thread join.
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            for p in producers {
                p.join().unwrap();
            }
        }),
    )
    .await
    .expect("producer threads must exit once the channel closes")
    .expect("spawn_blocking must not panic");
}

/// AC-2 (G2 rework, mux-window-switch-output-hang task0004, review
/// round 3 findings `dd23cfc388062939`/`5c01ffb8d53dc9f7`): a client
/// that keeps the server's client-message arm (`client_reader.next()`,
/// task0001 rename) CONTINUOUSLY ready must not be able to starve the
/// fair-reservation/drain machinery forever. Pane A's
/// channel is saturated exactly as in the sibling test above; a
/// `RequestPaneSnapshot` for pane A is deferred against the full
/// channel, and then — for the ENTIRE wait — a genuinely CONCURRENT
/// background task keeps sending harmless `PtyInput` messages for
/// pane B as fast as the socket accepts them, so the server's
/// client-message arm is essentially always immediately ready to be
/// polled. Before the G2 fix, `biased` ordering meant this arm won
/// EVERY iteration under that pressure, so the fair-reservation arm
/// (and the drain arm behind it) was never even POLLED — the deferred
/// snapshot would starve forever. Asserts the snapshot is still
/// delivered within a bounded timeout despite the continuous traffic.
///
/// Honesty note (development history): a real socket's send/read
/// timing has enough natural jitter that this end-to-end test was NOT
/// observed to reliably fail even with the G2 guard reverted to
/// unconditionally `true` — the exact race the finding describes
/// depends on genuinely gapless message arrival, which real
/// scheduling does not reliably force either way in this environment.
/// This test still exercises the real connection path end-to-end and
/// guards against gross regressions (e.g. the mechanism being removed
/// or the connection genuinely deadlocking), but the DETERMINISTIC
/// proof of the bounded-iterations guarantee is
/// `allow_client_message_arm`'s own unit tests, above in this module.
///
/// Honesty note 2 (mux-window-switch-output-hang task0005, review round
/// 4 finding `bc0e5ae9c626fb31`, G2): this test's own cleanup used to
/// `flood_task.await` BEFORE `conn_task.abort()`, which could hang the
/// whole `cargo test` process if the flood's `send()` happened to be
/// parked at that exact moment (see the fix's own comment at the
/// cleanup site below). Reverting to that exact ordering and re-running
/// this test (including 5 repeated runs) in this development
/// environment did NOT reproduce a hang — the same class of
/// real-socket timing jitter noted above evidently also affects
/// whether the flood's `send()` is caught mid-flight at cleanup time.
/// The reordering is kept as a structural fix (aborting the connection
/// first is correct regardless of whether this exact interleaving is
/// forced in any given run), but this is recorded honestly as NOT
/// independently red-confirmed in this environment, rather than
/// claimed as observed.
///
/// Uses a REAL TCP loopback connection (`TcpStream::into_split()`),
/// not this file's usual `tokio::io::duplex()` in-memory pair: this
/// test's own red/green history — a single duplex stream split either
/// via `Framed::split()` or the generic `tokio::io::split()` was
/// observed, empirically, to serialize the flood task's sends and the
/// main task's reads behind an internal lock, starving the read side
/// regardless of the server's own scheduling (a test-harness artifact,
/// not the G2 mechanism this test exists to exercise — that variant
/// hung even WITH the G2 fix in place). A real socket's split halves
/// are natively lock-free and full-duplex, which is what genuine
/// concurrent send-while-read requires here.
///
/// Multi-threaded runtime (unlike this file's other connection-level
/// tests): the flood task, the connection task, and the main task's
/// own reads are three independently-progressing actors here; a
/// single-threaded (`current_thread`) runtime's cooperative
/// scheduling was observed, empirically, to occasionally starve one of
/// them of a turn entirely under sustained flood pressure. Separate OS
/// worker threads remove that test-harness-only risk.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connection_level_deferred_snapshot_delivered_despite_continuous_client_traffic() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    const PANE_B: u32 = 2;
    {
        let mut mgr = session_manager.lock().await;
        let sid = mgr.create_session("default".to_string());
        let wid = mgr.create_window(sid, "shell".to_string()).unwrap();

        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);

        let target_b: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_b = MuxPane::new_test(PANE_B, 80, 24, target_b);
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_b);
    }

    // Real TCP loopback (not an in-memory `tokio::io::duplex()` pair):
    // the flood task below needs to send CONTINUOUSLY while the main
    // task reads CONCURRENTLY, and splitting either a combined
    // `Framed` (`.split()`) or a raw duplex stream (`tokio::io::split`)
    // for that was observed, empirically, to serialize reads and
    // writes behind an internal lock and starve the read side
    // regardless of the server's own scheduling — a test-harness
    // artifact unrelated to the G2 mechanism this test exists to
    // exercise. `TcpStream::into_split()` provides genuinely
    // independent, lock-free read/write halves (a real socket is
    // natively full-duplex), avoiding that pitfall.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback listener");
    let addr = listener.local_addr().unwrap();

    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let session_manager_for_conn = session_manager.clone();
    let conn_task = tokio::spawn(async move {
        let (server_stream, _peer) = listener.accept().await.expect("accept loopback conn");
        handle_connection(
            server_stream,
            session_manager_for_conn,
            shutdown_tx,
            title_tx,
            notification_tx,
            agent_status_tx,
            pane_exit_sender,
            upgrade_tx,
            no_ack_slot(),
        )
        .await;
    });

    let client_stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect loopback client");
    let _ = client_stream.set_nodelay(true);
    let (client_read_half, client_write_half) = client_stream.into_split();
    let mut client_writer = tokio_util::codec::FramedWrite::new(client_write_half, MuxCodec::new());
    let mut client_reader = tokio_util::codec::FramedRead::new(client_read_half, MuxCodec::new());

    client_writer
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client_reader.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);
    let welcome_payload: WelcomeMsg = welcome.decode_payload().unwrap();
    let session_id = match welcome_payload {
        WelcomeMsg::Accepted { sessions, .. } => sessions[0].id,
        WelcomeMsg::Rejected { reason } => panic!("unexpected rejection: {reason}"),
    };

    client_writer
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    // Inlined equivalent of the sibling test's `drain_until_pane_created`
    // helper (which is typed against a combined `Framed`, not the
    // split `FramedRead` half used here).
    {
        let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let expected_pane_ids = [PANE_A, PANE_B];
        while seen.len() < expected_pane_ids.len() {
            let msg = tokio::time::timeout(Duration::from_secs(5), client_reader.next())
                .await
                .expect("must not hang waiting for reattach frames")
                .expect("stream must not end")
                .expect("frame must decode");
            if msg.msg_type == MessageType::PaneCreated && expected_pane_ids.contains(&msg.pane_id)
            {
                seen.insert(msg.pane_id);
            }
        }
    }

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut producers = Vec::new();
    for _ in 0..8 {
        let stop_clone = stop.clone();
        let producer_tx = owned_tx.clone();
        producers.push(std::thread::spawn(move || {
            while !stop_clone.load(StdOrdering::Relaxed) {
                if producer_tx
                    .blocking_send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 32768]))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    // Give the producers time to actually saturate the channel before
    // issuing the request this test is about.
    tokio::time::sleep(Duration::from_millis(100)).await;

    client_writer
        .send(MuxMessage {
            msg_type: MessageType::RequestPaneSnapshot,
            pane_id: PANE_A,
            payload: Vec::new(),
        })
        .await
        .unwrap();

    // Continuous client traffic: a genuinely CONCURRENT background
    // task floods harmless `PtyInput` for pane B as fast as the socket
    // accepts sends, for as long as the main task is still waiting
    // below — the loopback TCP connection's independent read/write
    // halves (see this test's doc) make this safe, unlike a split
    // in-memory duplex stream.
    let flood_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flood_stop_clone = flood_stop.clone();
    let flood_task = tokio::spawn(async move {
        while !flood_stop_clone.load(StdOrdering::Relaxed) {
            if client_writer
                .send(MuxMessage {
                    msg_type: MessageType::PtyInput,
                    pane_id: PANE_B,
                    payload: b"x".to_vec(),
                })
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let mut saw_snapshot_for_pane_a = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !saw_snapshot_for_pane_a {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "AC-2: pane A's deferred snapshot must be delivered within a bounded \
             time despite CONTINUOUS client-message traffic (the starvation this \
             arm's quota guard exists to bound)"
        );
        let msg = tokio::time::timeout(remaining, client_reader.next())
            .await
            .expect("must not hang")
            .expect("stream must not end")
            .expect("frame must decode");
        if msg.msg_type == MessageType::Snapshot && msg.pane_id == PANE_A {
            saw_snapshot_for_pane_a = true;
        }
    }

    flood_stop.store(true, StdOrdering::Relaxed);
    stop.store(true, StdOrdering::Relaxed);
    // G2 rework (review round 4 finding `bc0e5ae9c626fb31`): `abort()`
    // the connection task BEFORE awaiting `flood_task`, and wrap that
    // join in a timeout, mirroring the sibling test's `drop(client)` ->
    // `abort()` -> timeout-wrapped join. Pre-fix, `flood_task.await`
    // (which owns `client_writer`, so the main body cannot `drop` it to
    // unstick the flood loop) sat BEFORE `conn_task.abort()`: the flood
    // loop only rechecks its stop flag after an in-flight `send()`
    // completes, and in this test's steady state that send is parked —
    // the server has already stopped reading (it drains only up to the
    // one Snapshot(A) frame the wait loop above needed) and blocks
    // forever in its own `framed.flush().await`, so the flood's `send`
    // never completes either. `cargo test` has no per-test timeout, so
    // that ordering could hang the whole suite. Aborting the connection
    // task FIRST drops its half of the TCP socket, which fails the
    // flood task's in-flight `send()` (broken pipe / reset) and lets it
    // exit; the timeout is a bounded fallback in case OS-level socket
    // teardown is slower than expected in some environment, rather than
    // relying on it alone.
    conn_task.abort();
    tokio::time::timeout(Duration::from_secs(5), flood_task)
        .await
        .expect("flood task must exit once the connection is aborted")
        .expect("flood task must not panic");
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            for p in producers {
                p.join().unwrap();
            }
        }),
    )
    .await
    .expect("producer threads must exit once the channel closes")
    .expect("spawn_blocking must not panic");
}

// ── task0001 (mux-connection-input-freeze): AC-1 regression test ──

/// AC-1 (FR1/FR5): the daemon connection task's drain arm must not park
/// the WHOLE `select!` loop when the socket's send buffer is full. This
/// is VERIFICATION TS1 / the regression test for the residual freeze
/// this feature closes.
///
/// Floods pane A's output channel via background `blocking_send`
/// threads (same shape as the sibling task0003/task0004 tests above)
/// over a SMALL duplex whose server->client capacity saturates almost
/// immediately once the client stops reading, then sends `PtyInput`
/// for a DIFFERENT pane (B) and asserts it is processed — observable
/// directly at pane B's captured writer, NOT via a reply frame (the
/// client never reads again after draining the reattach handshake, so
/// a reply-frame-based assertion could never resolve either way) —
/// within the named 5s timeout.
///
/// Pre-task0001 this test fails: the drain arm's
/// `framed.feed`/`framed.flush` calls block the WHOLE connection task
/// once the duplex buffer fills (nobody is reading), so `framed.next()`
/// never gets polled again and pane B's `PtyInput` is never processed
/// — the test's own timeout fires.
#[tokio::test]
async fn connection_level_client_input_processed_while_outbound_socket_saturated() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    const PANE_B: u32 = 2;
    let captured_input: Arc<StdMutex<Vec<u8>>> = Arc::new(StdMutex::new(Vec::new()));
    {
        let mut mgr = session_manager.lock().await;
        let sid = mgr.create_session("default".to_string());
        let wid = mgr.create_window(sid, "shell".to_string()).unwrap();

        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);

        let target_b: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_b = MuxPane::new_test_with_writer(
            PANE_B,
            80,
            24,
            target_b,
            Box::new(CapturingWriter(captured_input.clone())),
        );
        mgr.get_session_mut(sid)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_b);
    }

    // Small duplex: comfortably fits the handshake/reattach exchange
    // but saturates almost immediately once the flood begins and the
    // client stops reading (matches the sibling AC-3/AC-4 tests'
    // duplex sizing above, for the identical reason).
    let (server_stream, client_stream) = tokio::io::duplex(16 * 1024);
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        server_stream,
        session_manager.clone(),
        shutdown_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());

    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);
    let welcome_payload: WelcomeMsg = welcome.decode_payload().unwrap();
    let session_id = match welcome_payload {
        WelcomeMsg::Accepted { sessions, .. } => sessions[0].id,
        WelcomeMsg::Rejected { reason } => panic!("unexpected rejection: {reason}"),
    };

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A, PANE_B]).await;

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    // Background OS threads saturating pane A's channel via
    // `blocking_send` — same shape as the sibling AC-3/AC-4 tests
    // above (mirrors `pty_spawn.rs`'s reader thread).
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut producers = Vec::new();
    for _ in 0..8 {
        let stop_clone = stop.clone();
        let producer_tx = owned_tx.clone();
        producers.push(std::thread::spawn(move || {
            while !stop_clone.load(StdOrdering::Relaxed) {
                if producer_tx
                    .blocking_send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 32768]))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    // The client deliberately stops reading from here on (AC-1's own
    // scenario): give the flood time to genuinely saturate the
    // outbound socket (small duplex, nobody draining it) before
    // sending the probe message.
    tokio::time::sleep(Duration::from_millis(200)).await;

    client
        .send(MuxMessage {
            msg_type: MessageType::PtyInput,
            pane_id: PANE_B,
            payload: b"hi".to_vec(),
        })
        .await
        .expect(
            "client->server direction is independent of the saturated \
             server->client direction",
        );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if *captured_input.lock().unwrap() == b"hi" {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "AC-1: PtyInput for pane B must be processed within the named 5s \
             timeout despite pane A's output channel and the outbound socket \
             both being saturated"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    stop.store(true, StdOrdering::Relaxed);
    drop(client);
    conn_task.abort();
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            for p in producers {
                p.join().unwrap();
            }
        }),
    )
    .await
    .expect("producer threads must exit once the channel closes")
    .expect("spawn_blocking must not panic");
}

/// AC-3 (FR4): when the outbound admission path is saturated, the
/// connection STOPS consuming `pane_output_rx` — upstream backpressure
/// is directly observable (the channel's own reported capacity drops
/// by exactly what was sent and then stays put) — rather than
/// continuing to drain it into some other, unbounded buffer.
///
/// The writer task ALWAYS eagerly dequeues whatever `outbound_tx`
/// currently holds the instant it is scheduled (freeing that channel
/// capacity immediately, independent of how long the actual socket
/// write of that dequeued batch then takes) — so a single slow-to-flush
/// chunk alone does not keep `outbound_tx` looking "full" to a `try_send`
/// arriving later. Saturating `outbound_tx` itself (`OUTBOUND_QUEUE_CAPACITY`
/// = 2) therefore needs more DISTINCT frames admitted in ONE drain-arm
/// iteration than fit — three different, non-mergeable pane ids' chunks
/// queued into `pane_output_rx` back-to-back (no `.await` yield point in
/// between: each `send()` resolves immediately since `pane_output_tx`
/// has ample capacity, so the writer task never gets scheduled in
/// between on this test's single-threaded runtime) does that
/// deterministically. Only pane A is a REAL registered pane (needed for
/// Attach); the other two ids are fictional — `PtyOutput` classification
/// builds a frame straight from the chunk's own `pane_id`/`data`, with
/// no `SessionManager` lookup, so this is a faithful, minimal probe of
/// the admission mechanism alone.
#[tokio::test]
async fn connection_level_stops_consuming_pane_output_rx_when_outbound_queue_saturated() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    let session_id;
    {
        let mut mgr = session_manager.lock().await;
        session_id = mgr.create_session("default".to_string());
        let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);
    }

    // Tiny duplex, exactly as AC-5's test: a single chunk past this is
    // enough to get the writer's flush genuinely stuck.
    let (server_stream, client_stream) = tokio::io::duplex(256);
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        server_stream,
        session_manager.clone(),
        shutdown_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());
    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A]).await;

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    const PANE_B: u32 = 2;
    const PANE_C: u32 = 3;

    let capacity_before = owned_tx.capacity();

    // Phase 1: get the writer's own `flush()` genuinely, permanently
    // stuck (client never reads again from here on) — ONE oversized
    // chunk against the 256-byte duplex. This alone does NOT yet
    // saturate `outbound_tx` itself (the writer eagerly dequeues the
    // instant it is scheduled, freeing that one channel slot
    // immediately — see this test's own doc), but it DOES mean the
    // writer never calls `recv()` again afterward, so whatever gets
    // admitted next stays admitted (and any remainder stays held) for
    // good.
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 4096]))
        .await
        .expect("pane_output_tx must accept the saturating chunk");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        owned_tx.capacity(),
        capacity_before,
        "the saturating chunk must have been consumed (capacity restored) — \
         the writer is now stuck flushing it, not the channel holding it"
    );

    // Phase 2: three DISTINCT (non-mergeable — different pane ids)
    // chunks, queued into `pane_output_rx` back-to-back with NO
    // intervening yield point: each `send()` resolves immediately
    // (`pane_output_tx` has ample capacity), so nothing else gets
    // scheduled in between. The drain arm's next run sees all three at
    // once and tries to admit three distinct frames into `outbound_tx`
    // (capacity 2, both slots free since the writer already dequeued
    // phase 1's chunk) in ONE synchronous, non-blocking pass — the 3rd
    // has nowhere to go and becomes `outbound_remainder`. Because the
    // writer is stuck on phase 1's flush (client never reads), it never
    // calls `recv()` again, so this remainder is never cleared either.
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'a'; 16]))
        .await
        .expect("pane_output_tx must accept the first chunk");
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_B, vec![b'b'; 16]))
        .await
        .expect("pane_output_tx must accept the second chunk");
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_C, vec![b'c'; 16]))
        .await
        .expect("pane_output_tx must accept the third chunk");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        owned_tx.capacity(),
        capacity_before,
        "AC-3: all three chunks must have been CONSUMED from pane_output_rx \
         (capacity restored) — the unsent one lives in the connection's own \
         outbound_remainder state, not left sitting in pane_output_rx"
    );

    // AC-3: with a genuine remainder held, `pane_output_rx` must now
    // sit UNCONSUMED — the drain arm's own guard (`if
    // outbound_remainder.is_empty()`) excludes it from the loop.
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'd'; 16]))
        .await
        .expect("pane_output_tx must still accept sends up to its own capacity");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        owned_tx.capacity(),
        capacity_before - 1,
        "AC-3: a chunk sent while an outbound remainder is held must sit \
         UNCONSUMED in pane_output_rx (capacity down by exactly the one \
         chunk sent), not be drained into some other, unbounded buffer"
    );

    // And stays put — the drain arm has genuinely STOPPED, not merely
    // fallen behind.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        owned_tx.capacity(),
        capacity_before - 1,
        "AC-3: pane_output_rx's backlog must remain UNCONSUMED (stable, not \
         shrinking) while the outbound path stays saturated — this IS the \
         upstream backpressure propagation FR4 requires"
    );

    drop(client);
    conn_task.abort();
}

/// AC-4 (NFR4): a PTY-exit chunk consumed by the drain arm still reaps
/// the pane even when delivery of the corresponding `PtyExited` frame
/// to the client CANNOT succeed — reap is decided at consumption time
/// (Design invariant 5), independent of the outbound writer's outcome.
///
/// Uses [`FailableWriteStream`] rather than a saturated-but-recoverable
/// duplex (AC-1's harness): flipping `fail_writes` makes delivery
/// genuinely, permanently impossible from that point on, which is a
/// sharper proof than "delivery hasn't happened YET" — it can never
/// happen at all, so a reap gated on delivery succeeding would never
/// fire.
#[tokio::test]
async fn connection_level_pty_exit_reaps_pane_even_when_delivery_can_never_succeed() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    let session_id;
    {
        let mut mgr = session_manager.lock().await;
        session_id = mgr.create_session("default".to_string());
        let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);
    }

    let (server_stream, client_stream) = tokio::io::duplex(16 * 1024);
    let fail_writes = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let wrapped_server = FailableWriteStream {
        inner: server_stream,
        fail_writes: fail_writes.clone(),
    };
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        wrapped_server,
        session_manager.clone(),
        shutdown_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());
    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A]).await;

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    // From here on, delivery of ANY client-bound frame is permanently
    // impossible.
    fail_writes.store(true, StdOrdering::Relaxed);

    // PTY-exit signal: empty-data chunk.
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, Vec::new()))
        .await
        .expect("pane_output_tx must still accept the exit chunk");

    // AC-4: the pane must be reaped (removed from the SessionManager)
    // within a bounded time, even though the resulting `PtyExited`
    // frame can never be delivered.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if session_manager.lock().await.is_empty() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "AC-4: the exited pane must be reaped within a bounded time \
             regardless of whether delivery to the client can succeed"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    drop(client);
    conn_task.abort();
}

/// AC-5 (NFR4): the `Upgrading` ack does not fire while the frame is
/// merely QUEUED behind a saturated socket, and fires once it has
/// actually been written AND flushed (socket drained) — not merely
/// admitted into the outbound queue.
///
/// Deliberately a SMALL saturation, unlike AC-1/AC-2's multi-thread
/// flood: a single oversized chunk against a tiny duplex is enough to
/// get the writer's `flush()` genuinely stuck (nothing drains it while
/// the client doesn't read), and keeps the backlog the client must
/// drain to observe the ack small and bounded — a multi-thread flood
/// here would keep re-winning the outbound queue's fair admission
/// ahead of the Upgrading notification for as long as it kept
/// producing, making the bound on "how much has to drain before the
/// ack fires" open-ended rather than the single small chunk this test
/// actually needs to prove the ordering.
#[tokio::test]
async fn connection_level_upgrading_ack_fires_only_after_flush_not_at_admission() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    let session_id;
    {
        let mut mgr = session_manager.lock().await;
        session_id = mgr.create_session("default".to_string());
        let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);
    }

    // Tiny duplex: still comfortably fits the handshake/attach
    // exchange (the client drains those promptly), but a single
    // few-KB chunk sent afterward already exceeds it.
    let (server_stream, client_stream) = tokio::io::duplex(256);
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    // Real ack channel, installed into the slot up front (mirrors
    // `mux::daemon::prepare_upgrade`'s own wiring).
    let (ack_tx, mut ack_rx) = mpsc::channel::<()>(4);
    let upgrade_ack_slot: SharedUpgradeAckSlot = Arc::new(StdMutex::new(Some(ack_tx)));

    let conn_task = tokio::spawn(handle_connection(
        server_stream,
        session_manager.clone(),
        shutdown_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        upgrade_ack_slot,
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());
    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A]).await;

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    // A single chunk (a few KB, well past the 256-byte duplex) is
    // enough to get the writer's `flush()` genuinely stuck once the
    // client stops reading — no background flood needed.
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 4096]))
        .await
        .expect("pane_output_tx must accept the saturating chunk");
    // Give the drain arm / writer time to actually reach the stuck
    // `flush()` before forwarding the notification.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Forward the Upgrading announcement (mirrors
    // `mux::daemon::prepare_upgrade`'s own construction) while the
    // socket is saturated.
    {
        let mgr = session_manager.lock().await;
        let upgrading = MuxMessage {
            msg_type: MessageType::Upgrading,
            pane_id: 0,
            payload: Vec::new(),
        };
        let _ = mgr.notify_tx().send(upgrading);
    }

    // AC-5: the ack must NOT have fired yet — the frame is, at best,
    // queued behind a saturated socket.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        ack_rx.try_recv().is_err(),
        "AC-5: the Upgrading ack must not fire while the frame is queued behind \
         a saturated socket"
    );

    // Resume client reads until the Upgrading frame itself is
    // observed — only the one saturating chunk plus the Upgrading
    // frame need to drain, a small, bounded backlog.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "must observe the Upgrading frame within a bounded time once reads resume"
        );
        let msg = tokio::time::timeout(remaining, client.next())
            .await
            .expect("must not hang")
            .expect("stream must not end")
            .expect("frame must decode");
        if msg.msg_type == MessageType::Upgrading {
            break;
        }
    }

    // AC-5: now that the frame has actually been written AND flushed,
    // the ack must fire, within a bounded time.
    tokio::time::timeout(Duration::from_secs(5), ack_rx.recv())
        .await
        .expect("must not hang waiting for the ack")
        .expect("ack channel must not be closed");

    drop(client);
    conn_task.abort();
}

/// AC-6: a socket write failure terminates the connection loop with
/// today's outcome — the loop exits and panes detach via the existing
/// teardown path (`detach_session_panes`, NOT the reap path AC-4
/// covers: the pane itself never exited here, only the socket did).
#[tokio::test]
async fn connection_level_socket_write_failure_terminates_loop_and_detaches_panes() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    let session_id;
    let target_a: SharedOutputTarget;
    {
        let mut mgr = session_manager.lock().await;
        session_id = mgr.create_session("default".to_string());
        let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
        target_a = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a.clone());
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);
    }

    let (server_stream, client_stream) = tokio::io::duplex(16 * 1024);
    let fail_writes = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let wrapped_server = FailableWriteStream {
        inner: server_stream,
        fail_writes: fail_writes.clone(),
    };
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        wrapped_server,
        session_manager.clone(),
        shutdown_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());
    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A]).await;
    assert!(matches!(
        *target_a.lock().unwrap(),
        PaneOutputTarget::Connected(_)
    ));

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    // Inject the write failure, then produce SOME output so the
    // writer actually attempts (and fails) a write.
    fail_writes.store(true, StdOrdering::Relaxed);
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, b"trigger".to_vec()))
        .await
        .expect("pane_output_tx must still accept the chunk");

    // AC-6: the connection loop must exit (the spawned task
    // completes) within a bounded time.
    tokio::time::timeout(Duration::from_secs(5), conn_task)
        .await
        .expect("connection loop must terminate after a socket write failure")
        .expect("handle_connection task must not panic");

    // AC-6: panes detach via the existing teardown path — the pane
    // itself never exited (only the socket did), so it must be back
    // to Detached, not reaped.
    assert!(
        matches!(*target_a.lock().unwrap(), PaneOutputTarget::Detached { .. }),
        "AC-6: a socket write failure must detach panes via the existing \
         teardown path, the same outcome as today"
    );
    assert!(
        !session_manager.lock().await.is_empty(),
        "AC-6: the pane itself never exited, so it must NOT be reaped — only \
         detached"
    );

    drop(client);
}

// ── task0003 (mux-connection-input-freeze): AC-1/AC-2/AC-3/AC-5
// regression tests for the consolidated `OutboundAdmission` component. ──

/// AC-1 (FR4, findings 7f9f9ad6fb4dd977 / ee54e1d2ff740104 /
/// 4e6f23b7d53527e5 / f9cde80880407a13) / AC-2 (FR1) — VERIFICATION
/// TS8: with a PtyOutput remainder held (client deliberately not
/// reading), a sustained stream of notifications — individually-spaced
/// sends plus a burst that overflows the broadcast channel's fixed
/// capacity (16, `SessionManager::notify_tx`), forcing a Lagged resync
/// — must NOT grow the held-frame count in proportion to how many
/// notifications were sent (AC-1): once the client resumes reading,
/// the RenameWindow frames delivered from the resync are bounded by
/// the session's own window count, not by the notification count.
/// AC-2: the SAME resumed read also proves the remainder does not
/// permanently close the drain arm — a freshly produced PtyOutput
/// chunk reaches the client within the named 5s timeout.
///
/// Pre-fix (notify arm ungated) this test fails: each individually-
/// spaced notification is forwarded into the held remainder as it
/// arrives (`outbound_remainder.push_back`), so the delivered
/// RenameWindow count after resuming grows with the notification
/// count instead of staying bounded by `N_WINDOWS`.
#[tokio::test]
async fn connection_level_notification_traffic_does_not_grow_held_remainder_and_drain_arm_resumes()
{
    const N_WINDOWS: usize = 3;
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    let session_id;
    {
        let mut mgr = session_manager.lock().await;
        session_id = mgr.create_session("default".to_string());
        let mut first_window = None;
        for _ in 0..N_WINDOWS {
            let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
            first_window.get_or_insert(wid);
        }
        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&first_window.unwrap())
            .unwrap()
            .add_pane(pane_a);
    }

    let (server_stream, client_stream) = tokio::io::duplex(256);
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        server_stream,
        session_manager.clone(),
        shutdown_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());
    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A]).await;

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    const PANE_B: u32 = 2;
    const PANE_C: u32 = 3;
    const REMAINDER_MARKER: &[u8] = b"AC1-REMAINDER-MARKER";

    // Phase 1 (mirrors the existing outbound-saturation test): stick
    // the writer's flush permanently (client never reads again from
    // here on).
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 4096]))
        .await
        .expect("pane_output_tx must accept the saturating chunk");
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Phase 2: three distinct, non-mergeable chunks admitted in one
    // synchronous drain pass — two fit `OUTBOUND_QUEUE_CAPACITY` (2),
    // the third becomes the held remainder. Tag it with a recognizable
    // marker.
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'a'; 16]))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_B, vec![b'b'; 16]))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(
            PANE_C,
            REMAINDER_MARKER.to_vec(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    // DIAGNOSTIC precondition check (mirrors the existing outbound-
    // saturation test's technique): confirm the drain arm has
    // genuinely stopped consuming `pane_output_rx` — i.e. a remainder
    // is actually held — before starting the notification phase.
    let capacity_before_probe = owned_tx.capacity();
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'z'; 8]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        owned_tx.capacity(),
        capacity_before_probe - 1,
        "DIAGNOSTIC: test precondition failed — the drain arm must have \
         stopped consuming pane_output_rx (a remainder must be held) before \
         the notification phase begins"
    );

    // AC-1: sustained notification stream while the remainder is
    // held — a SUSTAINED count (40), each send followed by a short
    // sleep so a pre-fix, UNGATED notify arm gets a genuine chance to
    // run and process (append to the held remainder) EACH one as it
    // arrives, rather than all piling up while the connection task
    // never gets scheduled.
    //
    // Why this differentiates pre-/post-fix even though BOTH are
    // eventually bounded by the SAME pre-existing broadcast channel
    // capacity (`SessionManager::notify_tx`, 16) once the connection
    // task finally reads: pre-fix, the connection task DOES keep up
    // (it actively consumes `notify_rx.recv()` on every iteration,
    // no gate), so none of the 40 notifications are ever evicted —
    // all 40 get individually appended to the ALREADY-held remainder,
    // proportional growth. Post-fix, the gate means the connection
    // task never calls `notify_rx.recv()` AT ALL while holding, so
    // the broadcast channel's OWN pre-existing (unrelated to this
    // fix) 16-slot ring buffer is what ends up bounding how many of
    // the 40 sends are even still available once the gate reopens —
    // the rest are evicted and recovered via ONE bounded Lagged
    // resync (contributing at most the session's window count).
    const NOTIFICATION_COUNT: u32 = 40;
    for i in 0..NOTIFICATION_COUNT {
        let msg = MuxMessage::control(
            MessageType::RenameWindow,
            9000 + i,
            &RenameWindowMsg {
                name: format!("notif-{i}"),
            },
        );
        let _ = session_manager.lock().await.notify_tx().send(msg);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Resume reading: collect frames until the stream goes quiet,
    // sending AC-2's fresh probe chunk once the held marker has been
    // observed.
    let mut rename_count = 0usize;
    let mut saw_marker = false;
    let mut saw_post_resume = false;
    let overall_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = overall_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining.min(Duration::from_millis(500)), client.next()).await {
            Ok(Some(Ok(msg))) => {
                if msg.msg_type == MessageType::RenameWindow {
                    rename_count += 1;
                } else if msg.msg_type == MessageType::PtyOutput
                    && msg.pane_id == PANE_C
                    && msg.payload == REMAINDER_MARKER
                {
                    saw_marker = true;
                } else if msg.msg_type == MessageType::PtyOutput
                    && msg.pane_id == PANE_A
                    && msg.payload == b"post-resume"
                {
                    saw_post_resume = true;
                }
            }
            Ok(Some(Err(e))) => panic!("frame decode error: {e}"),
            Ok(None) => break,
            Err(_) => {
                // 500ms quiet gap: send AC-2's probe once the held
                // marker has been observed, then keep reading.
                if saw_marker && !saw_post_resume {
                    owned_tx
                        .send(PtyOutputChunk::pty_output(PANE_A, b"post-resume".to_vec()))
                        .await
                        .expect("pane_output_tx must accept the post-resume chunk");
                    continue;
                }
                break;
            }
        }
    }

    assert!(
        saw_marker,
        "the originally held remainder frame must still be delivered"
    );
    assert!(
        rename_count <= 25,
        "AC-1: notification-driven remainder growth must be bounded — \
         regardless of how many notifications ({NOTIFICATION_COUNT}) were sent \
         while a remainder was held, the eventual delivery count must stay \
         capped by the broadcast channel's own pre-existing fixed capacity plus \
         one resync's window count ({N_WINDOWS}), not grow proportionally with \
         the notification count. Got {rename_count} RenameWindow frames \
         (unbounded pre-fix growth would approach {NOTIFICATION_COUNT})."
    );
    assert!(
        saw_post_resume,
        "AC-2: a freshly produced PtyOutput chunk must reach the client within \
         the named timeout once the client resumes reading — the remainder must \
         not permanently close the drain arm"
    );

    drop(client);
    conn_task.abort();
}

/// AC-3 (FR3, findings 79d48554f94fd3df / ebb33f43f48f612a): with a
/// PtyOutput remainder held, reply/reattach-path frames — a same-pane
/// PaneCreated + SnapshotRestore (via a re-Attach to the same session)
/// plus one plain `route_message` reply (a queued Detach) — are
/// observed by the client strictly AFTER every held remainder frame;
/// same-pane SnapshotRestore never precedes the older held PtyOutput.
///
/// Pre-fix this test fails: `handle_attach`/`send_reattach_data`
/// admitted directly into the raw outbound sender, bypassing the held
/// remainder — the reattach's PaneCreated/SnapshotRestore would arrive
/// BEFORE the older held marker chunk.
#[tokio::test]
async fn connection_level_reply_and_reattach_frames_never_overtake_held_remainder() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    let session_id;
    {
        let mut mgr = session_manager.lock().await;
        session_id = mgr.create_session("default".to_string());
        let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);
    }

    let (server_stream, client_stream) = tokio::io::duplex(256);
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        server_stream,
        session_manager.clone(),
        shutdown_tx,
        title_tx,
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());
    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A]).await;
    // The initial attach's own reattach-data snapshot: PaneCreated is
    // immediately followed by a SnapshotRestore (pane A's buffer is
    // never empty — it always carries at least the leading clear
    // sequence). Drain it now so it cannot contaminate the ordered-
    // sequence assertion below, which specifically tracks
    // SnapshotRestore(A) as a significant event for the LATER
    // (re-)Attach this test triggers.
    let initial_snapshot = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang draining the initial attach's own snapshot")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(initial_snapshot.msg_type, MessageType::SnapshotRestore);
    assert_eq!(initial_snapshot.pane_id, PANE_A);

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    const PANE_B: u32 = 2;
    const REMAINDER_MARKER: &[u8] = b"AC3-REMAINDER-MARKER";

    // Stick the writer's flush permanently.
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 4096]))
        .await
        .expect("pane_output_tx must accept the saturating chunk");
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Three distinct, non-mergeable chunks (A, B, A again — B in
    // between prevents same-pane merging) admitted in one synchronous
    // pass; the third (pane A's marker) becomes the held remainder.
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'a'; 16]))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_B, vec![b'b'; 16]))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(
            PANE_A,
            REMAINDER_MARKER.to_vec(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    // With the remainder held, queue a re-Attach to the SAME session
    // (same-pane PaneCreated + SnapshotRestore for pane A) followed
    // immediately by a Detach (a plain route_message reply) — both
    // writes succeed immediately (client->server is independent of
    // the stalled server->client direction).
    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    client
        .send(MuxMessage::control(MessageType::Detach, 0, &()))
        .await
        .unwrap();

    // Resume reading: collect the ordered sequence of "significant"
    // frames.
    #[derive(Debug, PartialEq, Eq)]
    enum Seen {
        RemainderMarker,
        PaneCreatedA,
        SnapshotRestoreA,
        Detached,
    }
    let mut order = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while order.last() != Some(&Seen::Detached) {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "must not hang waiting for the full ordered sequence, got {order:?}"
        );
        let msg = tokio::time::timeout(remaining, client.next())
            .await
            .expect("must not hang")
            .expect("stream must not end")
            .expect("frame must decode");
        if msg.msg_type == MessageType::PtyOutput
            && msg.pane_id == PANE_A
            && msg.payload == REMAINDER_MARKER
        {
            order.push(Seen::RemainderMarker);
        } else if msg.msg_type == MessageType::PaneCreated && msg.pane_id == PANE_A {
            order.push(Seen::PaneCreatedA);
        } else if msg.msg_type == MessageType::SnapshotRestore && msg.pane_id == PANE_A {
            order.push(Seen::SnapshotRestoreA);
        } else if msg.msg_type == MessageType::Detached {
            order.push(Seen::Detached);
        }
    }

    assert_eq!(
        order,
        vec![
            Seen::RemainderMarker,
            Seen::PaneCreatedA,
            Seen::SnapshotRestoreA,
            Seen::Detached,
        ],
        "AC-3: reply/reattach frames must arrive strictly after every held \
         remainder frame — no overtaking"
    );

    drop(client);
    conn_task.abort();
}

/// AC-5 (teardown delivery, findings 96633ec8862e6c64 /
/// 76e7d90468e859d0): a client kicked while a remainder is held and
/// THEN resumes reading receives the held frame(s) followed by
/// Detached, in order, within the named teardown budget.
///
/// Pre-fix this test fails: teardown only flushed frames already
/// admitted into the outbound queue — the held remainder (including
/// the kick arm's appended Detached) was dropped, so the client would
/// observe a bare socket EOF instead of the marker frame followed by
/// Detached.
#[tokio::test]
async fn connection_level_teardown_delivers_held_remainder_then_detached_to_a_resuming_client() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    let session_id;
    {
        let mut mgr = session_manager.lock().await;
        session_id = mgr.create_session("default".to_string());
        let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);
    }

    let (server_stream, client_stream) = tokio::io::duplex(256);
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        server_stream,
        session_manager.clone(),
        shutdown_tx,
        title_tx.clone(),
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());
    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A]).await;

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    const PANE_B: u32 = 2;
    const PANE_C: u32 = 3;
    const REMAINDER_MARKER: &[u8] = b"AC5-REMAINDER-MARKER";

    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 4096]))
        .await
        .expect("pane_output_tx must accept the saturating chunk");
    tokio::time::sleep(Duration::from_millis(150)).await;

    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'a'; 16]))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_B, vec![b'b'; 16]))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(
            PANE_C,
            REMAINDER_MARKER.to_vec(),
        ))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Fire the kick directly (mirrors what a second GUI client's
    // Attach would do): re-run `collect_reattach_data` for the SAME
    // session with a throwaway target — this swaps in a new kick
    // sender and fires the one this connection installed at its own
    // Attach above, evicting it.
    let (dummy_tx, _dummy_rx) = mpsc::channel::<PtyOutputChunk>(4);
    let (new_kick_tx, _new_kick_rx) = oneshot::channel::<()>();
    let _ = collect_reattach_data(
        &session_manager,
        session_id,
        &dummy_tx,
        &title_tx,
        new_kick_tx,
        true,
    )
    .await;

    // Give the connection task a moment to observe the kick and
    // append Detached to the held remainder before resuming reads.
    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut saw_marker = false;
    let mut saw_detached = false;
    let deadline = OUTBOUND_TEARDOWN_FLUSH_TIMEOUT + Duration::from_secs(2);
    let read_result = tokio::time::timeout(deadline, async {
        loop {
            let msg = client
                .next()
                .await
                .expect("stream must not end")
                .expect("frame must decode");
            if msg.msg_type == MessageType::PtyOutput
                && msg.pane_id == PANE_C
                && msg.payload == REMAINDER_MARKER
            {
                assert!(
                    !saw_detached,
                    "AC-5: the held remainder frame must arrive BEFORE Detached"
                );
                saw_marker = true;
            } else if msg.msg_type == MessageType::Detached {
                saw_detached = true;
                break;
            }
        }
    })
    .await;

    assert!(
        read_result.is_ok(),
        "AC-5: a resuming client must receive the held frame(s) followed by \
         Detached within the named teardown budget"
    );
    assert!(
        saw_marker,
        "the originally held remainder frame must be delivered"
    );
    assert!(
        saw_detached,
        "Detached must be delivered after the held remainder"
    );

    drop(client);
    let joined = tokio::time::timeout(Duration::from_secs(2), conn_task).await;
    assert!(
        joined.is_ok(),
        "connection task must complete after teardown"
    );
}

/// AC-5 companion: a client that NEVER resumes reading still lets the
/// connection task complete teardown within the bounded budget (rather
/// than hanging forever on the never-freeing outbound send).
#[tokio::test]
async fn connection_level_teardown_completes_within_budget_for_a_never_reading_client() {
    let session_manager = Arc::new(Mutex::new(SessionManager::new()));
    const PANE_A: u32 = 1;
    let session_id;
    {
        let mut mgr = session_manager.lock().await;
        session_id = mgr.create_session("default".to_string());
        let wid = mgr.create_window(session_id, "shell".to_string()).unwrap();
        let target_a: SharedOutputTarget = Arc::new(StdMutex::new(PaneOutputTarget::Detached {
            reason: DetachReason::NetworkDetach,
            owner: None,
        }));
        let pane_a = MuxPane::new_test(PANE_A, 80, 24, target_a);
        mgr.get_session_mut(session_id)
            .unwrap()
            .windows
            .get_mut(&wid)
            .unwrap()
            .add_pane(pane_a);
    }

    let (server_stream, client_stream) = tokio::io::duplex(256);
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let (title_tx, _title_rx): (TitleChangeSender, _) = mpsc::channel(16);
    let (notification_tx, _notification_rx): (NotificationSender, _) = mpsc::channel(16);
    let (agent_status_tx, _agent_status_rx): (AgentStatusReportSender, _) = mpsc::channel(16);
    let pane_exit_sender: SharedPaneExitSender = Arc::new(StdMutex::new(None));
    let (upgrade_tx, _upgrade_rx): (UpgradeSignalSender, _) = mpsc::channel(1);

    let conn_task = tokio::spawn(handle_connection(
        server_stream,
        session_manager.clone(),
        shutdown_tx,
        title_tx.clone(),
        notification_tx,
        agent_status_tx,
        pane_exit_sender,
        upgrade_tx,
        no_ack_slot(),
    ));

    let mut client = Framed::new(client_stream, MuxCodec::new());
    client
        .send(MuxMessage::control(
            MessageType::Hello,
            0,
            &HelloMsg {
                client_type: ClientType::Gui,
                protocol_version: PROTOCOL_VERSION,
            },
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(5), client.next())
        .await
        .expect("must not hang on Welcome")
        .expect("stream must not end")
        .expect("frame must decode");
    assert_eq!(welcome.msg_type, MessageType::Welcome);

    client
        .send(MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
        .await
        .unwrap();
    drain_until_pane_created(&mut client, &[PANE_A]).await;

    let owned_tx: mpsc::Sender<PtyOutputChunk> = {
        let mgr = session_manager.lock().await;
        let pane = mgr
            .get_session(session_id)
            .unwrap()
            .windows
            .values()
            .next()
            .unwrap()
            .panes
            .get(&PANE_A)
            .unwrap();
        match &*pane.output_target.lock().unwrap() {
            PaneOutputTarget::Connected(tx) => tx.clone(),
            PaneOutputTarget::Detached { .. } => {
                panic!("pane A must be Connected after attach, still Detached")
            }
        }
    };

    const PANE_B: u32 = 2;
    const PANE_C: u32 = 3;

    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'x'; 4096]))
        .await
        .expect("pane_output_tx must accept the saturating chunk");
    tokio::time::sleep(Duration::from_millis(150)).await;

    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_A, vec![b'a'; 16]))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_B, vec![b'b'; 16]))
        .await
        .unwrap();
    owned_tx
        .send(PtyOutputChunk::pty_output(PANE_C, vec![b'c'; 16]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    let (dummy_tx, _dummy_rx) = mpsc::channel::<PtyOutputChunk>(4);
    let (new_kick_tx, _new_kick_rx) = oneshot::channel::<()>();
    let _ = collect_reattach_data(
        &session_manager,
        session_id,
        &dummy_tx,
        &title_tx,
        new_kick_tx,
        true,
    )
    .await;

    // The client NEVER reads again from here on.
    let joined = tokio::time::timeout(
        OUTBOUND_TEARDOWN_FLUSH_TIMEOUT + Duration::from_secs(2),
        conn_task,
    )
    .await;
    assert!(
        joined.is_ok(),
        "AC-5: teardown must complete within the bounded budget even when the \
         client never resumes reading"
    );

    drop(client);
}
