//! Reattach and detach logic for mux sessions.

use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::outbound::OutboundAdmission;
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
///
/// `admission` (task0001, task0003 rework): every frame goes through the
/// GUI loop's SINGLE outbound admission component (module doc "Admission
/// path"), via [`OutboundAdmission::admit_blocking`] — this replaces the
/// pre-task0001 direct `framed.send`. Each pane entry's `PaneCreated` (and
/// its optional `SnapshotRestore`) is admitted together in ONE
/// `admit_blocking` call: that call drains any remainder already held by
/// an earlier producer FIRST, so a same-pane `SnapshotRestore` can never
/// overtake older held `PtyOutput` for that same pane (FR3, the worst-case
/// scenario the task plan's Design section names) — the exact ordering
/// hazard chunking into two separate calls per entry would not, by
/// itself, break (each call drains what's ahead of it either way), but a
/// single call keeps the pair atomic relative to any OTHER producer that
/// might interleave between them. Only called from `handle_attach` (GUI
/// loop), never the CLI-client path, so no [`super::outbound::ReplySink`]
/// dual-mode is needed here.
pub(super) async fn send_reattach_data(
    admission: &mut OutboundAdmission,
    reattach_data: &[(PaneId, Vec<u8>, Vec<(usize, u16, u16)>)],
) -> Result<(), ()> {
    for (pane_id, buffered, segments) in reattach_data {
        let mut frames = vec![MuxMessage::control(
            MessageType::PaneCreated,
            *pane_id,
            pane_id,
        )];
        if !buffered.is_empty() {
            let encoded = crate::mux::session::pane::encode_snapshot_segments(buffered, segments);
            // D6'' (task0005 rework, review round-4 finding `1d4a0c96821da0ef`):
            // route through the SAME shared size-policy check
            // `mux::ipc::handlers::handle_request_pane_snapshot` and the
            // visibility-resume path now use, rather than each producer
            // re-deriving its own comparison against `MAX_SNAPSHOT_FRAME_PAYLOAD`
            // (here via the `REATTACH_CHUNK_SIZE` alias — same value, same
            // check, one implementation).
            if mux_ipc::protocol::fits_single_snapshot_frame(encoded.len()) {
                frames.push(MuxMessage {
                    msg_type: MessageType::SnapshotRestore,
                    pane_id: *pane_id,
                    payload: encoded,
                });
            } else {
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
        }
        admission.admit_blocking(frames).await?;
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
mod tests;
