//! Pane output-target evaluation: detach-reason bookkeeping, the
//! attach / detach / hidden state machine, and the permit-guarded
//! resume path.

use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::mpsc;

use super::handles::lock_shadow_parser;
use super::output_queue::PtyOutputChunk;
use super::{MuxPane, encode_snapshot_segments};
use crate::mux::scrollback_buffer::ScrollbackRingBuffer;
use crate::mux::snapshot_bytes::build_resume_snapshot_bytes_for_ring;

/// Why a pane is currently detached. Combines `NetworkDetach`
/// (no client connected / kicked / explicit detach) with
/// `HiddenByVisibility` (client connected but reported hidden).
///
/// A pane stays Detached until **all** active reasons clear:
/// - hidden -> visible resolves the `HiddenByVisibility` bit
/// - reattach resolves the `NetworkDetach` bit
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetachReason {
    NetworkDetach,
    HiddenByVisibility,
    Both,
}

impl DetachReason {
    pub fn has_network(self) -> bool {
        matches!(self, DetachReason::NetworkDetach | DetachReason::Both)
    }

    pub fn has_hidden(self) -> bool {
        matches!(self, DetachReason::HiddenByVisibility | DetachReason::Both)
    }

    /// Combine two reasons (set union).
    pub fn combine(a: Self, b: Self) -> Self {
        match (
            a.has_network() || b.has_network(),
            a.has_hidden() || b.has_hidden(),
        ) {
            (true, true) => DetachReason::Both,
            (true, false) => DetachReason::NetworkDetach,
            (false, true) => DetachReason::HiddenByVisibility,
            (false, false) => DetachReason::HiddenByVisibility,
        }
    }

    /// Clear the `NetworkDetach` bit. Returns `None` when the result is empty.
    pub fn clear_network(self) -> Option<Self> {
        match self {
            DetachReason::NetworkDetach => None,
            DetachReason::HiddenByVisibility => Some(DetachReason::HiddenByVisibility),
            DetachReason::Both => Some(DetachReason::HiddenByVisibility),
        }
    }

    /// Clear the `HiddenByVisibility` bit. Returns `None` when the result is empty.
    pub fn clear_hidden(self) -> Option<Self> {
        match self {
            DetachReason::HiddenByVisibility => None,
            DetachReason::NetworkDetach => Some(DetachReason::NetworkDetach),
            DetachReason::Both => Some(DetachReason::NetworkDetach),
        }
    }
}

/// Where the PTY reader thread sends output data.
///
/// When a GUI client is connected, output goes directly to the channel.
/// When disconnected, the reader still drains the PTY but suppresses sends;
/// the scrollback ring (Phase B: `MuxPane::scrollback`) captures recent
/// bytes for replay on reattach.
///
/// `Detached` carries identity (`owner`) and cause (`reason`) so a second
/// connection cannot reclaim a pane that the first connection put into
/// `HiddenByVisibility`. `owner = None` marks system-origin detaches (e.g.
/// pane spawned before any client attached, or PTY EOF fallback) which any
/// connection may reclaim.
pub enum PaneOutputTarget {
    /// Connected: send output to the GUI via channel.
    Connected(mpsc::Sender<PtyOutputChunk>),
    /// Detached: the reader keeps draining the PTY into the per-pane
    /// scrollback (`MuxPane::scrollback`) and `raw_passthrough`; no
    /// channel send happens until the pane returns to `Connected`.
    Detached {
        reason: DetachReason,
        owner: Option<mpsc::Sender<PtyOutputChunk>>,
    },
}

/// Thread-safe shared reference to a pane's output target.
pub type SharedOutputTarget = Arc<StdMutex<PaneOutputTarget>>;

/// Thread-safe shared reference to a pane's scrollback ring buffer.
pub type SharedScrollback = Arc<StdMutex<ScrollbackRingBuffer>>;

/// Result of `evaluate_output_target`. Only a Connected -> Detached
/// transition is ever driven through this function; a Detached ->
/// Connected resume is handled solely by `resume_pane_with_permit` (FR10,
/// mux-suppressed-output-fixes task0004 — see that function's doc comment
/// for the race-freedom this split protects).
pub enum EvalResult {
    /// No state change required; output_target is still correct. Also
    /// returned for a would-be resume (Detached pane, owner matches, every
    /// reason would clear): the target, its reason and its owner are left
    /// exactly as they were, and nothing is sent or recorded.
    Unchanged,
    /// Pane was switched into Detached buffering mode.
    SwitchedToDetached,
}

/// Outcome of `resume_pane_with_permit`. Mirrors the in-lock decision so
/// callers can branch on the side effects (snapshot enqueued + Connected
/// swap, or no transition) without re-locking the pane.
pub enum ResumeOutcome {
    /// Pane was Detached and resolved cleanly. The snapshot was sent via
    /// the supplied permit and the target was swapped to `Connected`.
    Resumed,
    /// No transition: pane is already Connected, owner mismatches, or
    /// `NetworkDetach` is still active (only reattach can clear it).
    NoChange,
}

/// Decide the correct `output_target` for `pane` given the current network
/// detach flag and the connection-scoped visible flag, and apply the
/// transition in place.
///
/// Identity-scoped on both edges:
/// - Connected -> Detached: only flip panes whose current `Connected(tx)`
///   is `same_channel(owned_tx)`. The new `Detached` records `owned_tx` as
///   `owner` so a different connection cannot reclaim it via SetVisibility.
/// - Detached -> Connected: only resume when `owner` matches `owned_tx`
///   (or `owner == None`, system origin), AND the resolved reason becomes
///   empty after the caller's transition (`HiddenByVisibility` clears on
///   `visible=true`; `NetworkDetach` only clears via the reattach path).
pub fn evaluate_output_target(
    pane: &MuxPane,
    network_detach: bool,
    visible: bool,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
) -> EvalResult {
    let mut target = pane.output_target.lock().unwrap();
    let new_reason = match (network_detach, visible) {
        (true, true) => Some(DetachReason::NetworkDetach),
        (false, false) => Some(DetachReason::HiddenByVisibility),
        (true, false) => Some(DetachReason::Both),
        (false, true) => None,
    };
    match &mut *target {
        PaneOutputTarget::Connected(current_tx) => {
            let owned_by_caller = current_tx.same_channel(owned_tx);
            match new_reason {
                None => EvalResult::Unchanged,
                Some(reason) if owned_by_caller => {
                    *target = PaneOutputTarget::Detached {
                        reason,
                        owner: Some(owned_tx.clone()),
                    };
                    EvalResult::SwitchedToDetached
                }
                Some(_) => EvalResult::Unchanged,
            }
        }
        PaneOutputTarget::Detached { reason, owner } => {
            let owner_matches = match owner {
                Some(o) => o.same_channel(owned_tx),
                None => true,
            };
            if !owner_matches {
                return EvalResult::Unchanged;
            }
            // Resolve current reason against the caller's transition.
            let resolved = if visible {
                reason.clear_hidden()
            } else {
                Some(*reason)
            };
            let resolved = match (network_detach, resolved) {
                (true, Some(r)) => Some(DetachReason::combine(r, DetachReason::NetworkDetach)),
                (true, None) => Some(DetachReason::NetworkDetach),
                (false, r) => r,
            };
            match resolved {
                Some(r) => {
                    *reason = r;
                    if owner.is_none() {
                        *owner = Some(owned_tx.clone());
                    }
                    EvalResult::Unchanged
                }
                None => {
                    // FR10 (mux-suppressed-output-fixes task0004): a
                    // would-be resume (owner matches, every reason would
                    // clear) is a no-op here — the target, its reason and
                    // its owner are left exactly as they were, and nothing
                    // is sent or recorded. Only `resume_pane_with_permit`
                    // performs a Detached -> Connected transition; see its
                    // doc comment for the race-freedom (FR9) this split
                    // makes possible.
                    EvalResult::Unchanged
                }
            }
        }
    }
}

/// A `pane_output_tx` permit obtained either the FAST way (`try_reserve()`
/// or `reserve()`, borrowed from a live `&Sender`) or the FAIR way
/// (`reserve_owned()`, owned — mux-window-switch-output-hang task0003
/// rework, AC-3/G2: `mux::ipc::connection` polls a `reserve_owned()` future
/// as its own `select!` arm to avoid the starvation a `try_reserve`-only
/// retry suffers while a PTY reader thread has a `blocking_send` parked on
/// the same channel). `resume_pane_with_permit` accepts either so the same
/// atomic (build snapshot, send, swap to `Connected`) logic serves both
/// callers without duplicating it per permit type.
pub enum AnyPermit<'a> {
    Borrowed(mpsc::Permit<'a, PtyOutputChunk>),
    Owned(mpsc::OwnedPermit<PtyOutputChunk>),
}

impl AnyPermit<'_> {
    /// Consume the permit to send `chunk`. Infallible either way — the slot
    /// was already reserved — mirroring `mpsc::Permit::send`'s own contract.
    fn send(self, chunk: PtyOutputChunk) {
        match self {
            AnyPermit::Borrowed(p) => p.send(chunk),
            AnyPermit::Owned(p) => {
                // `OwnedPermit::send` hands back the underlying `Sender`;
                // callers here already hold their own clone, so it is
                // dropped.
                let _ = p.send(chunk);
            }
        }
    }
}

/// Race-free Detached -> Connected resume (originally documented against
/// an earlier feature's own "FR9"; mux-snapshot-output-boundary task0001
/// FR9/FR10 below are a DIFFERENT spec's requirements of the same number —
/// see that paragraph, not this one, for the terminal-query re-delivery
/// guarantee).
///
/// The caller obtains a permit for `pane_output_tx` *outside* the pane lock
/// (via `Sender::reserve().await`, `try_reserve()`, or the fair
/// `reserve_owned()` — see [`AnyPermit`]), then hands it in here. This
/// function holds the pane's `output_target` mutex for the full lifetime of
/// (build snapshot, record the boundary, send via permit, swap to
/// `Connected`). Because the PTY reader thread also takes the same
/// `output_target` mutex, then the boundary exclusion, before its
/// `try_send` / `blocking_send`, the reader cannot push a live chunk this
/// snapshot already covers to this same destination after the snapshot: a
/// chunk still in flight when the boundary is recorded here is suppressed
/// by the reader instead of racing the snapshot into the channel — the
/// mechanism the pre-existing FIFO-ordering claim below now rests on, not
/// FIFO ordering alone.
///
/// mux-snapshot-output-boundary task0001 (SPEC FR9/FR10): recording the
/// boundary here is also what lets the reader recognize, and re-deliver
/// after this snapshot, any terminal query or incomplete trailing sequence
/// a suppressed chunk carried — see `mux::ipc::pty_spawn::pty_reader_loop`
/// and `mux::ipc::pty_spawn::suppressed_output`.
///
/// `AnyPermit::send` is consumed and infallible (the slot is already
/// reserved), so the entire sequence runs under the std mutex without
/// `await`.
///
/// Returns `ResumeOutcome::NoChange` when the pane is not eligible to
/// resume (already Connected, owner mismatch, or `NetworkDetach` still
/// active). The caller should drop the permit on `NoChange` to release the
/// reserved slot.
pub fn resume_pane_with_permit(
    pane: &MuxPane,
    owned_tx: &mpsc::Sender<PtyOutputChunk>,
    permit: AnyPermit<'_>,
) -> ResumeOutcome {
    let mut target = pane.output_target.lock().unwrap();
    match &mut *target {
        PaneOutputTarget::Connected(_) => ResumeOutcome::NoChange,
        PaneOutputTarget::Detached { reason, owner } => {
            let owner_matches = match owner {
                Some(o) => o.same_channel(owned_tx),
                None => true,
            };
            if !owner_matches {
                return ResumeOutcome::NoChange;
            }
            let resolved = reason.clear_hidden();
            if let Some(r) = resolved {
                *reason = r;
                if owner.is_none() {
                    *owner = Some(owned_tx.clone());
                }
                return ResumeOutcome::NoChange;
            }
            // Phase C FR5 order: clear → scrollback → (alt-only, or
            // wrapped-ring) shadow. Routes through
            // `build_resume_snapshot_bytes_for_ring` so the strip +
            // main/alt split + wrap-restore dump-block logic stays in
            // lockstep with the reattach / on-demand snapshot SSOT
            // (`build_snapshot_bytes_with_layout`). Skip
            // `contents_formatted()` entirely for a non-wrapped main-buffer
            // pane: the helper would drop the slice anyway, so we avoid both
            // the computation and the longer shadow-parser lock hold.
            // D7'' (task0005 rework, review round-4 finding
            // `5ba2063e993baf6c`): the shadow parser's own size tracks
            // every `MuxPane::resize` call, so it is the pane's dims AT THE
            // MOMENT this snapshot is assembled — what `screen` was
            // actually produced at.
            //
            // mux-snapshot-output-boundary task0001 (IMPLEMENTATION.md
            // "Visibility resume"): the capture exclusion is taken ONLY
            // around these ring + shadow reads (NFR3) — it also yields
            // `boundary`, the output sequence number this read set
            // describes. Assembly, encoding and the size check below all
            // stay outside it.
            let (
                (buffered, buffered_segments, ring_wrapped, screen, alt_screen, current_dims),
                boundary,
            ) = pane.output_capture.captured_read(|| {
                let (buffered, buffered_segments, ring_wrapped) = pane
                    .scrollback
                    .lock()
                    .unwrap()
                    .read_segments_with_wrap_state();
                let (screen, alt_screen, current_dims) = {
                    let parser = lock_shadow_parser(&pane.shadow_parser);
                    let alt = parser.screen().alternate_screen();
                    let screen_bytes = if alt || ring_wrapped {
                        parser.screen().contents_formatted()
                    } else {
                        Vec::new()
                    };
                    let (rows, cols) = parser.screen().size();
                    (screen_bytes, alt, (cols, rows))
                };
                (
                    buffered,
                    buffered_segments,
                    ring_wrapped,
                    screen,
                    alt_screen,
                    current_dims,
                )
            });
            {
                // raw_passthrough is drained + cleared (so it does not leak
                // across detach cycles) but NOT concatenated — replaying the
                // captured image / Markdown OSC sequences would re-spawn
                // viewers on every visibility resume.
                let mut buf = pane.raw_passthrough.lock().unwrap();
                let _ = buf.read_all();
                buf.clear();
            }
            let (snapshot, snapshot_segments) = build_resume_snapshot_bytes_for_ring(
                &buffered,
                &buffered_segments,
                &screen,
                alt_screen,
                ring_wrapped,
                current_dims,
            );
            let encoded_snapshot = encode_snapshot_segments(&snapshot, &snapshot_segments);
            // D6''' (round-6 rework, review round-5 finding
            // `89b58cd82d7aa713`): on oversize, drop the reserved `permit`
            // WITHOUT sending (releasing its slot) and return `NoChange`
            // instead of `Resumed` — the pane stays Detached (fail
            // recoverably) rather than being handed a frame the codec will
            // reject, which previously tore the whole connection down.
            //
            // D3'''' (round-7 rework, review round-6 finding
            // `46c29c2c65970d26`): reachability of a PERMANENT freeze here
            // was disputed between round-6 reviewers — settled as
            // RECOVERABLE, not permanent. This branch's early `return`
            // happens BEFORE `*target` or `*reason` is ever touched, so the
            // pane is left in EXACTLY the state it was in on entry
            // (`Detached { HiddenByVisibility, .. }`). `handle_set_visibility`
            // (the only production caller) re-invokes this function for
            // every non-exited pane on every `visible -> true` edge that
            // is not itself a repeated no-op (its `prev == visible` guard
            // only suppresses a SECOND consecutive `true`, not a
            // false-then-true cycle) — so a later hide -> show toggle (the
            // client minimizing/restoring, or switching panes away and
            // back) unconditionally retries this exact call, and succeeds
            // once whatever inflated the snapshot (typically an
            // extreme-dimension alt-screen `contents_formatted()` dump,
            // D6'''' narrows the dimensions that can produce one) is no
            // longer true. See
            // `resume_pane_with_permit_recovers_after_oversize_condition_clears`
            // for the regression test pinning this: the pane is never left
            // detached with visibility latched on forever.
            if !mux_ipc::protocol::fits_single_snapshot_frame(encoded_snapshot.len()) {
                log::error!(
                    "visibility-resume: pane {} snapshot {}B exceeds the \
                     single-frame limit ({}B); staying detached rather \
                     than enqueuing a frame the codec would reject",
                    pane.id,
                    encoded_snapshot.len(),
                    mux_ipc::protocol::MAX_SNAPSHOT_FRAME_PAYLOAD
                );
                drop(permit);
                return ResumeOutcome::NoChange;
            }
            // D3 (IMPLEMENTATION.md): record (S, B) before the snapshot is
            // sent and before the swap to Connected — any chunk numbered
            // <= boundary that later reaches this same sender is therefore
            // either superseded by this snapshot or suppressed by the
            // reader (FR11).
            pane.output_capture.record_boundary(owned_tx, boundary);
            // review round-1 rework, finding `20b2bed0aaf48f94`: tag this as
            // a Snapshot-kind chunk (not the default PtyOutput) so the mux
            // connection drain (`mux::ipc::connection`) sends it as
            // `MessageType::Snapshot` on the wire. The client's
            // `apply_mux_message::Snapshot|SnapshotRestore` arm decodes the
            // structural dimension segments (task0004 round-4 rework D1')
            // and routes through `reset_and_replay_segments`, resizing per
            // segment instead of scanning the payload for markers — the
            // plain `PtyOutput` live path used before this fix does not, so
            // a resize-spanning visibility-resume snapshot replayed
            // coordinate-drifted content just like the reattach path did.
            permit.send(PtyOutputChunk::snapshot(pane.id, encoded_snapshot));
            *target = PaneOutputTarget::Connected(owned_tx.clone());
            ResumeOutcome::Resumed
        }
    }
}
