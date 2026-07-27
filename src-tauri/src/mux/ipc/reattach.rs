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
use crate::mux::session::manager::SessionManager;
use crate::mux::session::pane::{
    DetachReason, PaneId, PaneOutputTarget, PtyOutputChunk, SharedShadowParser, TitleChangeSender,
    lock_shadow_parser,
};
use crate::mux::snapshot_bytes::build_snapshot_bytes;

/// Build a self-contained ANSI byte sequence that reproduces the current
/// screen state tracked by the given shadow parser, prefixed with the pane's
/// scrollback so the client can scroll to past output.
///
/// The byte-layout helpers (`build_snapshot_bytes`, `build_resume_snapshot_bytes`)
/// live in [`crate::mux::snapshot_bytes`] so the SSOT can be shared with the
/// visibility-resume path (`mux::session::pane::resume_pane_with_permit`)
/// without creating a `mux::session` ↔ `mux::ipc` import cycle. This wrapper
/// stays here because it depends on `SharedShadowParser` from
/// `mux::session::pane`.
///
/// This funnels through [`build_snapshot_bytes`] and inherits its main/alt
/// split: for main-buffer panes the daemon vt100 `contents_formatted()` dump
/// is omitted from the output and the client rebuilds the visible viewport
/// from scrollback alone; for alt-screen panes the dump is included so the
/// TUI surface is restored. Callers do not need to pre-filter `screen`
/// themselves — `build_snapshot_bytes` decides whether to include it based
/// on the parser's `alternate_screen()` flag.
///
/// `scrollback` / `scrollback_segments` are read by the caller WITHOUT
/// clearing (the buffer lives for the lifetime of the pane), via
/// `ScrollbackRingBuffer::read_segments` (task0004 round-4 rework D1'). An
/// empty `scrollback` yields a valid clear + (optional) shadow snapshot
/// (history replays as empty). Returns the assembled payload bytes and the
/// segments re-expressed as offsets into THAT payload — see
/// [`build_snapshot_bytes`] for why the offsets need adjusting.
///
/// Used by both the reattach path (combined with ring buffer delta) and the
/// on-demand `RequestPaneSnapshot` path.
pub(super) fn build_shadow_parser_snapshot(
    shadow_parser: &SharedShadowParser,
    scrollback: &[u8],
    scrollback_segments: &[(usize, u16, u16)],
) -> (Vec<u8>, Vec<(usize, u16, u16)>) {
    // D7'' (task0005 rework, review round-4 finding `5ba2063e993baf6c`):
    // the shadow parser's OWN size is kept in lockstep with every
    // `MuxPane::resize` call (`parser.screen_mut().set_size(rows, cols)`),
    // so it is the pane's dimensions AT THE MOMENT this snapshot is
    // assembled — exactly what `screen_data` (below) was produced at.
    // Passed through so `build_snapshot_bytes` can tag the trailing screen
    // dump with its OWN segment instead of silently inheriting whatever the
    // last scrollback segment says.
    let (screen_data, alt_screen, current_dims) = {
        let parser = lock_shadow_parser(shadow_parser);
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        (
            screen.contents_formatted(),
            screen.alternate_screen(),
            (cols, rows),
        )
    };
    build_snapshot_bytes(
        scrollback,
        scrollback_segments,
        &screen_data,
        alt_screen,
        current_dims,
    )
}

/// Collect reattach data for panes in the given session.
///
/// When `visible == true`, drains buffered output from detached panes and
/// switches each pane to `Connected(pane_output_tx)`. Each returned tuple
/// carries the pane id and the resume snapshot bytes (clear + ring + shadow).
/// The per-pane `raw_passthrough` buffer is drained and cleared but NOT
/// concatenated onto the snapshot — replaying captured viewer / image launch
/// sequences would re-spawn viewers on every reattach.
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
) -> Vec<(PaneId, Vec<u8>, Vec<(usize, u16, u16)>)> {
    let mut new_kick_opt = Some(new_kick);
    let mut old_kick: Option<oneshot::Sender<()>> = None;
    let mut data: Vec<(PaneId, Vec<u8>, Vec<(usize, u16, u16)>)> = Vec::new();
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
                        data.push((pane.id, Vec::new(), Vec::new()));
                        continue;
                    }

                    // Visible reattach: build the FR5-ordered resume snapshot
                    // and switch to Connected. Order is
                    //   ESC[H ESC[2J + scrollback + shadow + passthrough
                    // so the scrollback bytes replay into the client's WASM
                    // grid (populating its history) before the shadow snapshot
                    // overwrites the visible screen with a known good final
                    // state. Scrollback is read WITHOUT clearing (FR6: the
                    // buffer lives for the lifetime of the pane), via
                    // `read_segments` (task0004 round-4 rework D1') so the
                    // structural dimension segments travel alongside the
                    // bytes instead of as in-band markers.
                    // D7'' (task0005 rework, review round-4 finding
                    // `5ba2063e993baf6c`): the shadow parser's own size
                    // tracks every `MuxPane::resize` call, so it is the
                    // pane's dims AT THE MOMENT this snapshot is assembled
                    // — what `screen_data` was actually produced at.
                    let (screen_data, is_alternate_screen, current_dims) = {
                        let parser = lock_shadow_parser(&pane.shadow_parser);
                        let screen = parser.screen();
                        let (rows, cols) = screen.size();
                        (
                            screen.contents_formatted(),
                            screen.alternate_screen(),
                            (cols, rows),
                        )
                    };
                    let (scrollback_data, scrollback_segments) =
                        pane.scrollback.lock().unwrap().read_segments();

                    let mut target = pane.output_target.lock().unwrap();
                    let target_was = match &*target {
                        PaneOutputTarget::Connected(_) => "Connected",
                        PaneOutputTarget::Detached { .. } => "Detached",
                    };
                    *target = PaneOutputTarget::Connected(pane_output_tx.clone());
                    drop(target);

                    // Drain (and clear) the per-pane raw passthrough buffer so
                    // it does not leak into the next detach cycle. The captured
                    // image / Markdown OSC byte runs are NOT concatenated onto
                    // the snapshot: replaying them would re-spawn viewers /
                    // re-render inline images on every window switch. The
                    // snapshot restores plain-text history only.
                    let drained_passthrough_len = {
                        let mut buf = pane.raw_passthrough.lock().unwrap();
                        let bytes = buf.read_all();
                        buf.clear();
                        bytes.len()
                    };

                    log::info!(
                        "collect_reattach: pane {} was={}, scrollback={}B, screen={}B, dropped_passthrough={}B, total={}B, alt_screen={}, exited={}",
                        pane.id,
                        target_was,
                        scrollback_data.len(),
                        screen_data.len(),
                        drained_passthrough_len,
                        8 + scrollback_data.len() + screen_data.len(),
                        is_alternate_screen,
                        pane.exited
                    );

                    // Shared layout: ESC[3J ESC[H ESC[2J + scrollback (rich
                    // content stripped) + screen + alt-mode.
                    let (combined, combined_segments) = build_snapshot_bytes(
                        &scrollback_data,
                        &scrollback_segments,
                        &screen_data,
                        is_alternate_screen,
                        current_dims,
                    );

                    data.push((pane.id, combined, combined_segments));
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

/// Maximum payload bytes per `PtyOutput` frame emitted during reattach replay
/// — and the threshold above which a pane's snapshot falls back to
/// segment-blind chunked framing at all.
///
/// task0004 round-4 rework (D4', review round-3 finding
/// `ea222e74bb0a046c`): derived directly from the protocol's own hard limit
/// (`mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD` = `MAX_FRAME_LENGTH` minus
/// the fixed 5-byte frame header) rather than an ad-hoc "comfortably below
/// today's realistic max" margin — the previous 1 MiB headroom had no
/// structural justification and under-utilized the single-frame path for no
/// reason. A pane's ring buffer holds at most `DEFAULT_SCROLLBACK_CAPACITY`
/// (2 MiB) plus a shadow-parser screen dump (bounded by cols × rows, a few
/// hundred KiB at most even for a very large terminal) plus the segment
/// header's own small overhead, so every realistic snapshot still takes the
/// single-frame path; only a pane whose payload genuinely exceeds the
/// codec's hard per-frame limit needs to split at all.
const REATTACH_CHUNK_SIZE: usize = MAX_SNAPSHOT_FRAME_PAYLOAD;

/// Send reattach data (PaneCreated + buffered output) to the client.
///
/// A non-empty snapshot whose WIRE-ENCODED size (payload bytes +
/// `mux_ipc::protocol::encode_snapshot_payload`'s structural segment header,
/// task0004 round-4 rework D1') fits in a single codec frame is sent as ONE
/// `MessageType::SnapshotRestore` frame carrying that encoded payload. The
/// client's `apply_mux_message::Snapshot|SnapshotRestore` arm decodes the
/// segments and routes through `reset_and_replay_segments`, which resizes
/// per segment instead of scanning the payload for markers. A
/// `SnapshotRestore` frame MUST arrive whole — the client resets its core
/// before replaying it — so this only applies while the encoded buffer fits
/// in one frame.
///
/// D6''' (round-6 rework, review round-5 finding `c1605e6978ee5e48`): a
/// buffer too large for a single frame (see [`REATTACH_CHUNK_SIZE`]'s doc
/// for why this is not reachable by any realistic per-pane snapshot) no
/// longer falls back to chunked `PtyOutput` framing of the RAW
/// (un-encoded, segment-less) bytes. That fallback discarded the segment
/// table entirely and replayed every byte at the client's CURRENT
/// dimensions regardless of what a resize-spanning buffer actually
/// recorded — precisely the rendering-corruption class this feature exists
/// to close, reintroduced by a payload-size threshold with no relation to
/// whether the buffer spans a resize. Full segment-preserving multi-frame
/// snapshot framing (a versioned chunked format carrying identity,
/// ordering, completion, and per-chunk segment metadata) is out of scope
/// for this task; until it exists, an oversize pane FAILS RECOVERABLY
/// instead: this pane's buffered history is skipped (the client still gets
/// its `PaneCreated`, so the pane itself attaches — just without
/// scrollback) rather than replayed under changed semantics. No other
/// pane's reattach is affected.
///
/// Large per-pane buffers therefore never produce an oversized single
/// frame (the codec would fail to encode it, tearing the socket down) —
/// this size check is what prevents that, not a multi-frame split.
pub(super) async fn send_reattach_data<S>(
    framed: &mut Framed<S, MuxCodec>,
    reattach_data: &[(PaneId, Vec<u8>, Vec<(usize, u16, u16)>)],
) -> Result<(), ()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    for (pane_id, buffered, segments) in reattach_data {
        let resp = MuxMessage::control(MessageType::PaneCreated, *pane_id, pane_id);
        if framed.send(resp).await.is_err() {
            return Err(());
        }
        if buffered.is_empty() {
            continue;
        }
        let encoded = crate::mux::session::pane::encode_snapshot_segments(buffered, segments);
        // D6'' (task0005 rework, review round-4 finding `1d4a0c96821da0ef`):
        // route through the SAME shared size-policy check
        // `mux::ipc::handlers::handle_request_pane_snapshot` and the
        // visibility-resume path now use, rather than each producer
        // re-deriving its own comparison against `MAX_SNAPSHOT_FRAME_PAYLOAD`
        // (here via the `REATTACH_CHUNK_SIZE` alias — same value, same
        // check, one implementation).
        if mux_ipc::protocol::fits_single_snapshot_frame(encoded.len()) {
            let msg = MuxMessage {
                msg_type: MessageType::SnapshotRestore,
                pane_id: *pane_id,
                payload: encoded,
            };
            if framed.send(msg).await.is_err() {
                return Err(());
            }
            continue;
        }
        log::error!(
            "reattach: pane {} snapshot {}B exceeds the single-frame limit \
             ({}B); skipping this pane's buffered history rather than \
             replaying it segment-blind at the client's current dimensions \
             (D6''')",
            pane_id,
            encoded.len(),
            REATTACH_CHUNK_SIZE
        );
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
        MuxPane, PaneOutputTarget, SharedOutputTarget, new_shadow_parser,
    };
    use std::sync::Mutex as StdMutex;

    fn make_test_pane_with_target(id: u32, output_target: SharedOutputTarget) -> MuxPane {
        MuxPane::new_test(id, 80, 24, output_target)
    }

    // ── TS-4 / TS-6: on-demand snapshot builder (FR1) ────────────────────

    /// TS-4: the on-demand snapshot builder emits scrollback BEFORE the
    /// shadow screen, matching the reattach construction (clear + scrollback
    /// + shadow).
    ///
    /// Driven through the `alt_screen = true` branch (parser is switched into
    /// alt-screen mode before the screen bytes are fed) so the daemon vt100
    /// dump is included in the assembled snapshot — that is the only branch
    /// where the SCREEN-CONTENT byte-ordering assertion applies under the
    /// main/alt split contract.
    #[test]
    fn build_shadow_parser_snapshot_emits_scrollback_before_screen() {
        let parser: SharedShadowParser = Arc::new(StdMutex::new(new_shadow_parser(24, 80)));
        // Flip the shadow parser into alt-screen mode before feeding the
        // screen bytes so build_snapshot_bytes follows the alt branch.
        parser.lock().unwrap().process(b"\x1b[?1049h");
        parser.lock().unwrap().process(b"SCREEN-CONTENT");
        let scrollback = b"HISTORY-LINE-ONE";

        let (snapshot, _segments) = build_shadow_parser_snapshot(&parser, scrollback, &[]);

        // Leading clear-and-home.
        assert!(
            snapshot.starts_with(b"\x1b[3J\x1b[H\x1b[2J"),
            "snapshot must start with ESC[3J ESC[H ESC[2J"
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
        assert!(scrollback_at >= b"\x1b[3J\x1b[H\x1b[2J".len());
    }

    /// TS-6: an empty scrollback yields a valid clear + shadow snapshot
    /// (no panic, history replays empty).
    ///
    /// Driven through the `alt_screen = true` branch so the daemon vt100
    /// shadow dump is included — the SCREEN-presence assertion is only
    /// meaningful for that branch under the main/alt split contract. The
    /// `alt_screen = false` empty case (clear + ESC[?1049l only) is covered
    /// by `build_snapshot_bytes_layout_is_clear_scrollback_screen` and
    /// `build_snapshot_bytes_main_buffer_omits_screen_part`.
    #[test]
    fn build_shadow_parser_snapshot_empty_scrollback_is_clear_plus_shadow() {
        let parser: SharedShadowParser = Arc::new(StdMutex::new(new_shadow_parser(24, 80)));
        parser.lock().unwrap().process(b"\x1b[?1049h");
        parser.lock().unwrap().process(b"ONLY-SCREEN");

        let (snapshot, _segments) = build_shadow_parser_snapshot(&parser, b"", &[]);

        assert!(snapshot.starts_with(b"\x1b[3J\x1b[H\x1b[2J"));
        assert!(
            snapshot
                .windows(b"ONLY-SCREEN".len())
                .any(|w| w == b"ONLY-SCREEN"),
            "shadow screen must still be present with empty scrollback"
        );
    }

    // ── Byte-layout helpers moved to `crate::mux::snapshot_bytes` ─────────
    //
    // The pure-byte tests for `build_snapshot_bytes` and
    // `build_resume_snapshot_bytes` (strip / main-alt split / clear prefix /
    // alt-mode toggle) live alongside the helpers in
    // `src-tauri/src/mux/snapshot_bytes.rs`. The two
    // `build_shadow_parser_snapshot_*` tests above stay here because they
    // exercise the wrapper that funnels a `SharedShadowParser` through
    // `build_snapshot_bytes`.

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
        let mut pane_ids: Vec<u32> = data.iter().map(|(id, _, _)| *id).collect();
        pane_ids.sort();
        assert_eq!(pane_ids, vec![1, 2], "Should contain pane IDs 1 and 2");

        // Verify all buffers start with the reset sequence (screen restoration)
        for (_, buf, _) in &data {
            assert!(
                !buf.is_empty(),
                "Reattach data should include screen restoration"
            );
            assert!(
                buf.starts_with(b"\x1b[3J\x1b[H\x1b[2J"),
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
        for (_, buf, _) in &data {
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

    /// D6''' (round-6 rework, review round-5 finding `c1605e6978ee5e48`): a
    /// pane whose ENCODED snapshot exceeds the single-frame limit must skip
    /// its buffered history entirely (`PaneCreated` still arrives, so the
    /// pane attaches) rather than falling back to segment-blind chunked
    /// `PtyOutput` framing — that fallback discarded the segment table and
    /// replayed the whole buffer at the client's CURRENT dimensions,
    /// reproducing the coordinate-drift class this feature exists to close
    /// for any oversize resize-spanning buffer. A single oversized frame
    /// would ALSO make the codec encoder fail and tear the connection down,
    /// which is exactly why this size check exists at all.
    ///
    /// Confirmed to fail pre-fix: the old fallback emitted 3 `PtyOutput`
    /// chunks reassembling to the original buffer — this test's "no further
    /// frames" assertion would have seen the first chunk instead.
    #[tokio::test]
    async fn test_send_reattach_data_skips_history_for_oversize_buffer() {
        use futures::StreamExt;
        use tokio_util::codec::Framed;

        let (client, server) = tokio::io::duplex(64 * 1024 * 1024);
        let mut server_framed = Framed::new(server, MuxCodec::new());
        let client_framed = Framed::new(client, MuxCodec::new());

        // Payload that spans just over two full chunks — its ENCODED form
        // exceeds `MAX_SNAPSHOT_FRAME_PAYLOAD`.
        let payload_len = REATTACH_CHUNK_SIZE * 2 + 123;
        let big: Vec<u8> = (0..payload_len).map(|i| (i % 251) as u8).collect();
        let reattach_data = vec![(42u32, big.clone(), Vec::new())];

        let sender = tokio::spawn(async move {
            let mut framed = client_framed;
            send_reattach_data(&mut framed, &reattach_data).await
        });

        // First (and only) frame: PaneCreated — the pane still attaches.
        let first = server_framed.next().await.unwrap().unwrap();
        assert_eq!(first.msg_type, MessageType::PaneCreated);
        assert_eq!(first.pane_id, 42);

        sender.await.unwrap().expect("send_reattach_data ok");

        // No history frames follow — dropping the sender closes the stream.
        let next =
            tokio::time::timeout(std::time::Duration::from_millis(50), server_framed.next()).await;
        match next {
            Ok(None) | Err(_) => {} // stream closed or timed out: both OK
            Ok(Some(Ok(frame))) => panic!(
                "unexpected extra frame for an oversize buffer: {:?}",
                frame.msg_type
            ),
            Ok(Some(Err(e))) => panic!("unexpected stream error: {}", e),
        }
    }

    /// Empty buffer emits PaneCreated and nothing else.
    #[tokio::test]
    async fn test_send_reattach_data_empty_buffer_emits_only_pane_created() {
        use futures::StreamExt;
        use tokio_util::codec::Framed;

        let (client, server) = tokio::io::duplex(64 * 1024);
        let mut server_framed = Framed::new(server, MuxCodec::new());
        let client_framed = Framed::new(client, MuxCodec::new());

        let reattach_data = vec![(7u32, Vec::<u8>::new(), Vec::new())];
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

    /// review round-1 rework, finding 20b2bed0aaf48f94 / task0002 AC-6: a
    /// normal-sized (well under `REATTACH_CHUNK_SIZE`) non-empty buffer is
    /// sent as ONE `MessageType::SnapshotRestore` frame — not chunked
    /// `PtyOutput` — so the client routes it through the segment-aware
    /// `reset_and_replay_segments` path (task0004 round-4 rework D1'). The
    /// wire payload is the D1'-encoded form
    /// (`mux_ipc::protocol::encode_snapshot_payload`); decoding it back must
    /// recover the original bytes with no segments (this test supplies
    /// none).
    #[tokio::test]
    async fn test_send_reattach_data_sends_snapshot_restore_for_normal_sized_buffer() {
        use futures::StreamExt;
        use tokio_util::codec::Framed;

        let (client, server) = tokio::io::duplex(64 * 1024);
        let mut server_framed = Framed::new(server, MuxCodec::new());
        let client_framed = Framed::new(client, MuxCodec::new());

        let payload = b"\x1b[3J\x1b[H\x1b[2Jsome scrollback bytes".to_vec();
        let reattach_data = vec![(9u32, payload.clone(), Vec::new())];
        let sender = tokio::spawn(async move {
            let mut framed = client_framed;
            send_reattach_data(&mut framed, &reattach_data).await
        });

        let first = server_framed.next().await.unwrap().unwrap();
        assert_eq!(first.msg_type, MessageType::PaneCreated);
        assert_eq!(first.pane_id, 9);

        let second = server_framed.next().await.unwrap().unwrap();
        assert_eq!(second.msg_type, MessageType::SnapshotRestore);
        assert_eq!(second.pane_id, 9);
        let (segments, content) = mux_ipc::protocol::decode_snapshot_payload(&second.payload);
        assert!(segments.is_empty());
        assert_eq!(content, payload.as_slice());

        sender.await.unwrap().expect("send_reattach_data ok");
    }

    /// End-to-end companion to the above: a reattach snapshot whose
    /// structural segments (task0004 round-4 rework D1') describe a
    /// mid-stream dimension change, sent through `send_reattach_data` and
    /// decoded on the client side via the SAME `MuxCodec` the real
    /// connection uses, is recognized as `SnapshotRestore`, decodes back to
    /// the same segments (`mux_ipc::protocol::decode_snapshot_payload`),
    /// and — once fed through `TerminalCore::reset_and_replay_segments`,
    /// mirroring what `apply_mux_message` does for that arm — actually
    /// resizes the replay core mid-drain (witnessed via
    /// `reflow_call_count`, since the final size always equals the
    /// caller's target regardless of what happened mid-stream). This is
    /// the behavior the old `PtyOutput` framing could not provide (the
    /// live path never calls `reset_and_replay_segments`).
    #[tokio::test]
    async fn test_send_reattach_data_snapshot_restore_payload_is_segment_interpretable() {
        use futures::StreamExt;
        use tokio_util::codec::Framed;

        let (client, server) = tokio::io::duplex(64 * 1024);
        let mut server_framed = Framed::new(server, MuxCodec::new());
        let client_framed = Framed::new(client, MuxCodec::new());

        let before = b"before\r\n".to_vec();
        let after = b"after\r\n".to_vec();
        let mut scrollback = before.clone();
        scrollback.extend_from_slice(&after);
        let segments = vec![(0usize, 80u16, 24u16), (before.len(), 100u16, 30u16)];
        let (snapshot, snapshot_segments) =
            build_snapshot_bytes(&scrollback, &segments, b"", false, (80, 24));
        let reattach_data = vec![(3u32, snapshot, snapshot_segments)];

        let sender = tokio::spawn(async move {
            let mut framed = client_framed;
            send_reattach_data(&mut framed, &reattach_data).await
        });

        let _pane_created = server_framed.next().await.unwrap().unwrap();
        let snapshot_frame = server_framed.next().await.unwrap().unwrap();
        assert_eq!(snapshot_frame.msg_type, MessageType::SnapshotRestore);

        let (dim_segments, content) =
            mux_ipc::protocol::decode_snapshot_payload(&snapshot_frame.payload);
        assert!(
            !dim_segments.is_empty(),
            "the mid-stream dimension segment must survive the wire round-trip"
        );
        let replay_segments: Vec<term_core::terminal_core::ReplaySegment> = dim_segments
            .iter()
            .map(|d| term_core::terminal_core::ReplaySegment {
                offset: d.offset,
                cols: d.cols,
                rows: d.rows,
            })
            .collect();

        let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 1000);
        let before_reflows = core.reflow_call_count();
        core.reset_and_replay_segments(content, &replay_segments);
        assert!(
            core.reflow_call_count() > before_reflows,
            "the mid-stream segment must actually trigger a resize during replay"
        );
        assert_eq!(
            core.cols(),
            80,
            "core must end back at the caller's original size"
        );
        assert_eq!(core.rows(), 24);

        sender.await.unwrap().expect("send_reattach_data ok");
    }

    /// task0003 AC-5 (D7, review round-2 finding `98eec9bbef67704a`) /
    /// task0004 AC-10 (D4', review round-3 finding `ea222e74bb0a046c`): a
    /// reattach snapshot ABOVE the OLD 8 MiB chunking threshold (and
    /// comfortably fitting the CURRENT `MAX_SNAPSHOT_FRAME_PAYLOAD`
    /// derived-from-the-protocol-limit threshold) still goes out as ONE
    /// `SnapshotRestore` frame (not split into segment-blind `PtyOutput`
    /// chunks) and replays with its dimension segments honored — exactly
    /// as a smaller snapshot does. Uses the same cursor-addressed
    /// coordinate-drift technique `mux::ipc::pty_spawn`'s
    /// `tui_cursor_addressed_recording_replays_without_cross_line_mixing`
    /// test proves catches genuinely dropped segment attribution, padded
    /// well past the old threshold with plain scrollback content.
    #[tokio::test]
    async fn test_send_reattach_data_above_old_chunking_threshold_still_segment_aware() {
        use futures::StreamExt;
        use tokio_util::codec::Framed;

        let (client, server) = tokio::io::duplex(64 * 1024 * 1024);
        let mut server_framed = Framed::new(server, MuxCodec::new());
        let client_framed = Framed::new(client, MuxCodec::new());

        const OLD_CHUNK_THRESHOLD: usize = 8 * 1024 * 1024;

        let cols: u16 = 100;
        let rows_a: u16 = 32;
        let rows_b: u16 = 30;
        let mut recording = Vec::new();
        for i in 0..rows_a.max(rows_b) + 20 {
            recording.extend_from_slice(format!("chat history line {i}\r\n").as_bytes());
        }
        recording.extend_from_slice(b"\n\x1b7");
        recording.extend_from_slice(format!("\x1b[0;{}r", rows_a - 1).as_bytes());
        recording.extend_from_slice(b"\x1b8\x1b[1A");
        for tick in 0..3u32 {
            recording.extend_from_slice(format!("chat reply A line {tick}\r\n").as_bytes());
            recording.extend_from_slice(
                format!("\x1b7\x1b[{rows_a};0fSTATUS-A[{tick}]\x1b8").as_bytes(),
            );
        }
        let mid_offset = recording.len();
        recording.extend_from_slice(b"\n\x1b7");
        recording.extend_from_slice(format!("\x1b[0;{}r", rows_b - 1).as_bytes());
        recording.extend_from_slice(b"\x1b8\x1b[1A");
        for tick in 0..3u32 {
            recording.extend_from_slice(format!("chat reply B line {tick}\r\n").as_bytes());
            recording.extend_from_slice(
                format!("\x1b7\x1b[{rows_b};0fSTATUS-B[{tick}]\x1b8").as_bytes(),
            );
        }
        // Pad (as plain scrollback content, no segment authority attached)
        // past the OLD chunking threshold.
        while recording.len() < OLD_CHUNK_THRESHOLD + 1024 {
            recording.extend_from_slice(b"padding line to grow past the old threshold\r\n");
        }
        let segments = vec![(0usize, cols, rows_a), (mid_offset, cols, rows_b)];
        let (snapshot, snapshot_segments) =
            build_snapshot_bytes(&recording, &segments, b"", false, (80, 24));
        assert!(
            snapshot.len() > OLD_CHUNK_THRESHOLD,
            "test prerequisite: snapshot must exceed the OLD chunking threshold"
        );
        assert!(
            snapshot.len() <= REATTACH_CHUNK_SIZE,
            "test prerequisite: snapshot must still fit the CURRENT single-frame \
             threshold (derived from the protocol's actual payload limit)"
        );

        let reattach_data = vec![(5u32, snapshot, snapshot_segments)];
        let sender = tokio::spawn(async move {
            let mut framed = client_framed;
            send_reattach_data(&mut framed, &reattach_data).await
        });

        let _pane_created = server_framed.next().await.unwrap().unwrap();
        let frame = server_framed.next().await.unwrap().unwrap();
        assert_eq!(
            frame.msg_type,
            MessageType::SnapshotRestore,
            "a snapshot above the OLD chunking threshold must still arrive \
             as a single segment-aware SnapshotRestore frame, not chunked \
             PtyOutput"
        );

        let (dim_segments, content) = mux_ipc::protocol::decode_snapshot_payload(&frame.payload);
        let replay_segments: Vec<term_core::terminal_core::ReplaySegment> = dim_segments
            .iter()
            .map(|d| term_core::terminal_core::ReplaySegment {
                offset: d.offset,
                cols: d.cols,
                rows: d.rows,
            })
            .collect();
        let mut core = term_core::terminal_core::TerminalCore::new(cols, rows_a, 10_000);
        core.reset_and_replay_segments(content, &replay_segments);
        let mut tainted = Vec::new();
        for r in 0..rows_a {
            let line = core.get_line_text(r);
            if line.contains("STATUS-") && line.contains(" line ") {
                tainted.push(format!("row {r}: {line}"));
            }
        }
        assert!(
            tainted.is_empty(),
            "a snapshot above the old chunking threshold must replay with \
             its dimension segments honored — zero cross-phase-mixed rows, got \
             {tainted:?}"
        );

        sender.await.unwrap().expect("send_reattach_data ok");
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

    /// TS-20 (revised): collect_reattach_data must NOT concatenate the per-pane
    /// `raw_passthrough` buffer onto the resume snapshot — replaying the
    /// captured image / Markdown OSC sequences would re-spawn viewers /
    /// re-render inline images on every window switch. The buffer must still be
    /// drained + cleared so it does not leak across the next detach cycle.
    /// Plain-text ring history is still restored.
    #[tokio::test]
    async fn test_collect_reattach_data_drops_raw_passthrough_and_clears_it() {
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
        let (pane_id, snapshot, _segments) = &data[0];
        assert_eq!(*pane_id, 1);
        assert!(snapshot.starts_with(b"\x1b[3J\x1b[H\x1b[2J"));
        // The captured passthrough sequence must NOT be in the snapshot.
        let needle = b"\x1b_Gi=42;PNG-bytes\x1b\\";
        assert!(
            !snapshot.windows(needle.len()).any(|w| w == needle),
            "reattach snapshot must NOT include captured passthrough sequence"
        );
        // The plain-text ring data must still be present.
        assert!(
            snapshot
                .windows(b"buffered-from-ring".len())
                .any(|w| w == b"buffered-from-ring"),
            "reattach snapshot must include ring data"
        );

        // raw_passthrough must be drained — otherwise the next detach cycle
        // would carry stale bytes.
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
        let (pane_id, snapshot, _segments) = &data[0];
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
        let (_pid, snapshot, _segments) = &data2[0];
        assert!(snapshot.starts_with(b"\x1b[3J\x1b[H\x1b[2J"));
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
