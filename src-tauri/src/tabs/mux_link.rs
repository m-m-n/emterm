//! Mux-link surface of [`Tab`]: inbound mux-message routing
//! (`apply_mux_message` + the close-reconcile helper), outbound control
//! frames (`request_pane_snapshot` / `send_attach`), and the APC
//! partitioning between the image pipeline and the mux decoder.

use mux_ipc::protocol::{
    DecodedSnapshotPayload, MessageType, MuxMessage, RenameWindowMsg, ResizeMsg, WelcomeMsg,
    decode_snapshot_payload_typed,
};
use term_core::terminal_core::ReplaySegment;

use crate::mux::window_group::{MuxWindow, MuxWindowGroup};

#[cfg(test)]
use super::ResizeFrameRecord;
use super::{
    OFFTHREAD_LIVE_QUEUE_CAP_BYTES, OFFTHREAD_REPLAY_SEGMENT_THRESHOLD,
    OFFTHREAD_REPLAY_THRESHOLD_BYTES, Tab,
};

impl Tab {
    /// Decide which pane (if any) needs a screen reconcile after a window
    /// close. Given the active pane id captured **before** `remove_pane` and
    /// the active pane id read **after** the removal, return the now-active
    /// pane id to request a snapshot for, or `None` when nothing needs
    /// redrawing.
    ///
    /// Comparison is by pane **id**, not index, so a non-active window close
    /// that shifts indices but leaves the displayed window's content unchanged
    /// correctly yields `None` (FR2). When the group is emptied the post-removal
    /// active pane id is `None`, which also yields `None` (FR3, no request). A
    /// genuine active-window close (FR1) produces a different post-removal pane
    /// id and returns it.
    pub(super) fn close_reconcile_target(
        before_active: Option<u32>,
        after_active: Option<u32>,
    ) -> Option<u32> {
        match after_active {
            Some(after) if Some(after) != before_active => Some(after),
            _ => None,
        }
    }

    /// Route one decoded mux message into this tab. Called by `App::pump_all`
    /// after the APC decoder ([`crate::mux::apc::try_decode_emterm_mux`])
    /// produced a typed `MuxMessage`. Returns true when the visible state
    /// changed (caller schedules a redraw).
    ///
    /// `Snapshot` payloads are raw PTY-shaped bytes the daemon captured
    /// from the active window — they are replayed into `term_core` via
    /// `reset_and_replay`. Everything else either updates a side-channel
    /// (status bar, session name) or is logged and ignored — the bridge
    /// CLI continues to own the underlying socket protocol, so native-poc
    /// only needs to react to messages that mutate its own state.
    pub fn apply_mux_message(&mut self, msg: MuxMessage) -> bool {
        match msg.msg_type {
            MessageType::Snapshot | MessageType::SnapshotRestore => {
                self.handle_snapshot(msg.msg_type, msg.pane_id, &msg.payload)
            }
            MessageType::PtyOutput => self.handle_pty_output(msg.pane_id, msg.payload),
            // The former mux status-bar daemon push (opcode 0x16, see
            // `mux_ipc::protocol`'s reserved-opcode comment) was retired
            // by mux-status-bar-removal task0001; that opcode no longer
            // decodes into a `MuxMessage` at all (see
            // `mux_ipc::protocol::MessageType::from_u8`), so it falls
            // through to the wildcard arm below like any other
            // unrecognized message type.
            MessageType::AgentStatusUpdate => {
                // Daemon → GUI unsolicited push (task0005 AC-2). Applying it
                // to `App::agent_status` needs `&mut App`, which this method
                // does not have — latch the decoded payload for
                // `App::pump_all` to apply after the per-tab loop, mirroring
                // the `pending_pane_switch_from` / `pending_window_appended`
                // latch pattern used elsewhere in this match.
                match msg.decode_payload::<mux_ipc::protocol::AgentStatusUpdateMsg>() {
                    Some(update) => {
                        self.pending_agent_status_updates.push(update);
                        true
                    }
                    None => {
                        log::warn!("mux apc: malformed AgentStatusUpdate payload");
                        false
                    }
                }
            }
            MessageType::Welcome => match msg.decode_payload::<WelcomeMsg>() {
                Some(WelcomeMsg::Accepted { sessions, .. }) => {
                    match sessions.first() {
                        Some(session) => {
                            log::info!(
                                "mux apc: tab {:?} attached to session {}",
                                self.title,
                                session.name
                            );
                            // Detect the first Welcome of this attach *before*
                            // recording the session name. The bridge/daemon can
                            // deliver Welcome twice (a known duplication); a
                            // second Attach would make the daemon replay its
                            // buffered output a second time, interleaving two
                            // large base64 APC frames into a stream that no
                            // longer decodes ("invalid base64 encoding").
                            // `mux_session_name` is None before the first
                            // Welcome and cleared again on Detach, so it doubles
                            // as the per-attach guard without a new field.
                            let first_welcome = self.mux_session_name.is_none();
                            // Keep the existing session-name extraction intact
                            // (F3): the status bar badge reads it.
                            self.mux_session_name = Some(session.name.clone());
                            // Become the live-output owner so continuous PTY
                            // output (e.g. `top`) streams to native instead of
                            // only on-demand snapshots. The daemon delivers its
                            // live stream to a pane's single owning connection;
                            // native must Attach to take ownership, exactly as
                            // the WebView reattach path does (`mux-session.ts`).
                            // Gate on the *targeted* session's pane_count so
                            // when (in a future multi-session daemon)
                            // `sessions[0].pane_count == 0` and a later session
                            // has panes, we don't send an Attach to the empty
                            // session — the WebView `existingPanes > 0` check
                            // applies to the session being attached, not the
                            // sum across every session.
                            if first_welcome && session.pane_count > 0 {
                                self.send_attach(session.id);
                            }
                            // Seed the window group from the session's window
                            // list (additive). `windows` carries the daemon
                            // window id / name / active pane id; the pane ids
                            // are the per-window active panes, parallel to the
                            // window list (F1). When the daemon omits the
                            // window list (older daemon), leave the group
                            // unseeded — it stays a plain tab.
                            //
                            // Gate the entire seed + snapshot block behind
                            // `first_welcome` for the same reason as the Attach
                            // guard above. On the (known) duplicate Welcome
                            // delivery, replaying `group.seed(...)` would wipe
                            // out anything accumulated between the two Welcomes
                            // — a window appended from `PaneCreated`, an
                            // optimistic `confirm_mux_rename`/`confirm_mux_move`
                            // edit, an inbound `SwitchWindow` that moved
                            // `active` — and a second `request_pane_snapshot`
                            // would race the user's just-applied local change.
                            if first_welcome && !session.windows.is_empty() {
                                let (active_pane_id, seeded_pane_ids) = {
                                    let group =
                                        self.mux_group.get_or_insert_with(MuxWindowGroup::new);
                                    let windows: Vec<MuxWindow> = session
                                        .windows
                                        .iter()
                                        .map(|w| MuxWindow {
                                            id: w.id,
                                            name: w.name.clone(),
                                        })
                                        .collect();
                                    let pane_ids: Vec<u32> =
                                        session.windows.iter().map(|w| w.active_pane_id).collect();
                                    group.seed(
                                        windows,
                                        pane_ids,
                                        session.active_window_index as usize,
                                    );
                                    (group.active_pane_id(), group.pane_ids().to_vec())
                                };
                                // Tell the daemon every seeded pane's PTY size
                                // up front, so a freshly attached client picks
                                // up the GUI's current grid dimensions instead
                                // of inheriting whatever the previous owner
                                // (or the daemon's 80x24 default) left behind.
                                // Without this the daemon-side wrap column
                                // stays mismatched until the user happens to
                                // resize the window.
                                let (cols, rows) = {
                                    let core = self.core.lock();
                                    (core.cols(), core.rows())
                                };
                                for pane_id in &seeded_pane_ids {
                                    self.send_control(&MuxMessage::control(
                                        MessageType::Resize,
                                        *pane_id,
                                        &ResizeMsg { cols, rows },
                                    ));
                                    // task0003 AC-6: record this emission (mux
                                    // attach/Welcome pane seeding site — see
                                    // `ResizeFrameRecord`).
                                    #[cfg(test)]
                                    {
                                        self.resize_frame_log.push(ResizeFrameRecord {
                                            tab_stable_id: self.stable_id,
                                            pane_id: *pane_id,
                                            cols,
                                            rows,
                                        });
                                    }
                                }
                                // Pull the active window's screen on attach — the
                                // daemon does not push it unprompted, so without
                                // this the freshly attached tab stays blank
                                // (parity with the WebView reattach path's
                                // `requestPaneSnapshot`).
                                if let Some(pane_id) = active_pane_id {
                                    self.request_pane_snapshot(pane_id);
                                }
                            } else if first_welcome && session.pane_count == 0 {
                                // Fresh-start mux: the daemon has no panes yet,
                                // so `windows` is empty and the seed/attach
                                // branches above don't run. Legacy webview's
                                // `enterMuxMode` sent CreateWindow on this path
                                // to bootstrap the initial window — the native
                                // port was missing that step, which is the
                                // upstream cause of "shell freezes / status bar
                                // alive": with no seeded pane, `mux_group` stays
                                // `None`, `active_pane_id()` is `None`, and every
                                // keystroke gets dropped in `write_input` while
                                // the (historical, now-removed) mux status-bar
                                // daemon push kept the status bar updating
                                // regardless.
                                //
                                // Pre-install an empty group so the daemon's
                                // subsequent `PaneCreated` reply can land — the
                                // PaneCreated handler intentionally refuses to
                                // install a group on its own (M4 guard against
                                // pre-Welcome leakage).
                                //
                                // Send CreateWindow with an empty payload to
                                // match legacy's `sendControl(CreateWindow, 0)`
                                // wire form exactly — the daemon's
                                // empty-payload backward-compat path (pinned by
                                // `test_create_window_payload_empty_payload_backward_compat`)
                                // applies CreateWindowPayload defaults.
                                self.mux_group.get_or_insert_with(MuxWindowGroup::new);
                                self.send_control(&MuxMessage {
                                    msg_type: MessageType::CreateWindow,
                                    pane_id: 0,
                                    payload: Vec::new(),
                                });
                            }
                            true
                        }
                        None => false,
                    }
                }
                Some(WelcomeMsg::Rejected { reason }) => {
                    log::warn!("mux apc: handshake rejected: {reason}");
                    false
                }
                None => {
                    log::warn!("mux apc: malformed Welcome payload");
                    false
                }
            },
            MessageType::PaneCreated => {
                // SPEC FR4 / Message Mapping: the daemon's PaneCreated is the
                // authoritative "append window" signal — it fires for every
                // pane the daemon creates, whether this client requested the
                // create or another client did. Treat it as such:
                //
                // - Require an existing group: a PaneCreated arriving before
                //   Welcome installs nothing (no `get_or_insert_with`, so the
                //   empty-group leakage that made other handlers spuriously
                //   think this tab was mux-attached is gone — M4).
                // - Idempotent: if the pane id is already in our group (resend
                //   / replay), don't double-append.
                // - Daemon-authoritative: append even when no pending-create
                //   credit exists. `pending_create` is now purely an
                //   optimistic-UX counter — consume it when present so a
                //   subsequent CreateWindow request still gets its own credit,
                //   but never gate the append on it (the spec finding #5).
                let Some(group) = self.mux_group.as_mut() else {
                    log::debug!(
                        "mux apc: PaneCreated pane={} before attach (no group), ignored",
                        msg.pane_id
                    );
                    return false;
                };
                if group.index_of_pane_id(msg.pane_id).is_some() {
                    log::debug!(
                        "mux apc: PaneCreated pane={} already in group, ignored (idempotency)",
                        msg.pane_id
                    );
                    return false;
                }
                let pending = group.take_pending_create();
                // Locally-unique window id (one past the current max) so the
                // synthetic id never collides with a daemon-seeded one. Initial
                // name "Terminal" (OQ1 resolved); daemon-pushed RenameWindow
                // later overwrites it.
                let new_id = group.fresh_window_id();
                // The new window becomes the active sub-tab (see `push`). Treat
                // that like a pane switch for scroll bookkeeping: latch the
                // outgoing pane id so `App::pump_all` parks the outgoing pane's
                // scroll into its slot and reloads the new pane's (default
                // `Live`) slot — first-latch-only, matching the SwitchWindow
                // path. `active_pane_id()` is `None` for the tab's first mux
                // window (empty group before this push), so that case correctly
                // latches nothing.
                let from_pane = group.active_pane_id();
                group.push(
                    MuxWindow {
                        id: new_id,
                        name: "Terminal".to_string(),
                    },
                    msg.pane_id,
                );
                // FR6 (mux): the push made the new window the active sub-tab.
                // Latch it at the event source so `App::pump_all` scrolls it into
                // view when this is the active tab — immune to a same-pump
                // `PtyExited` or a `Welcome` reseed, unlike a window-count delta.
                self.pending_window_appended = true;
                if let Some(from) = from_pane {
                    if self.pending_pane_switch_from.is_none() {
                        self.pending_pane_switch_from = Some(from);
                    }
                }
                log::info!(
                    "mux apc: pane {} created (window {}, pending_consumed={}) for tab {:?}",
                    msg.pane_id,
                    new_id,
                    pending,
                    self.title
                );
                // The newly created window becomes the active sub-tab (see
                // `MuxWindowGroup::push`). Without a core reset here, the
                // previous active window's grid + scrollback stay painted
                // until the new shell's first byte arrives — and even after
                // it does, the old content lingers in scrollback. The
                // shared `reset_frame_for_replay` recipe drops prompts /
                // folds, runs `reset_and_replay(b"")`, and routes through
                // `backfill_marks` so `pending_frame_reset` latches and
                // any active selection / press anchor on this tab is
                // dropped by `App::pump_all`.
                let _ = self.reset_frame_for_replay(b"", &[]);
                // The daemon spawns every new PTY at a hardcoded 80x24
                // (`handle_create_window`); without this, the pane stays at
                // 80 columns even though the GUI grid is wider, so output
                // wraps early. Push the current grid dimensions immediately
                // after the append so the daemon-side PTY catches up.
                let (cols, rows) = {
                    let core = self.core.lock();
                    (core.cols(), core.rows())
                };
                self.send_control(&MuxMessage::control(
                    MessageType::Resize,
                    msg.pane_id,
                    &ResizeMsg { cols, rows },
                ));
                // task0003 AC-6: record this emission (PaneCreated site —
                // see `ResizeFrameRecord`).
                #[cfg(test)]
                {
                    self.resize_frame_log.push(ResizeFrameRecord {
                        tab_stable_id: self.stable_id,
                        pane_id: msg.pane_id,
                        cols,
                        rows,
                    });
                }
                true
            }
            MessageType::SwitchWindow => {
                // Daemon-initiated switch (e.g. CLI `switch-window`): sync the
                // active index to the window owning this pane. Port of
                // `handleRemoteSwitchWindow`'s index resolution.
                //
                // Capture the outgoing active pane id before the sync so the
                // App-side per-pane scroll save/restore (FR3) can park the
                // outgoing pane's position — the daemon handler runs inside
                // `pump`, with no access to `App::scroll_position`, so it
                // latches the transition for `App::pump_all` to apply.
                let from_pane = self.mux_group.as_ref().and_then(|g| g.active_pane_id());
                let synced = self
                    .mux_group
                    .as_mut()
                    .map(|g| g.set_active_by_pane(msg.pane_id))
                    .unwrap_or(false);
                if synced {
                    log::info!(
                        "mux apc: remote switch to pane {} for tab {:?}",
                        msg.pane_id,
                        self.title
                    );
                    // Latch the outgoing pane id only when the switch actually
                    // moved the active pane (a no-op switch onto the current
                    // pane must not park/reload scroll or force a redraw), and
                    // only for the FIRST move in this pump. Several SwitchWindow
                    // messages can drain in one `pump` (A→B→C); only A is the
                    // genuinely-displayed outgoing pane whose live scroll must be
                    // parked — intermediate panes were never rendered. Keeping
                    // the first `from` avoids parking the live scroll into a
                    // wrong (intermediate) slot.
                    let to_pane = self.mux_group.as_ref().and_then(|g| g.active_pane_id());
                    if let (Some(from), Some(to)) = (from_pane, to_pane) {
                        if from != to && self.pending_pane_switch_from.is_none() {
                            self.pending_pane_switch_from = Some(from);
                        }
                    }
                    // Reconcile the screen with the now-active window (parity
                    // with the WebView remote-switch path's `requestPaneSnapshot`).
                    self.request_pane_snapshot(msg.pane_id);
                }
                synced
            }
            MessageType::RenameWindow => {
                // Daemon-broadcast rename. The wire field is the *pane id* —
                // `confirm_mux_rename` sends `pane_ids()[idx]`, and the daemon
                // re-broadcasts the frame with the same field unchanged. The
                // earlier code interpreted `msg.pane_id` directly as a window
                // id (commented "WebView `const windowId = paneId`"), which
                // only worked when window ids and pane ids happened to
                // coincide; for windows where they differ (locally-created
                // windows get a synthetic window id from `fresh_window_id`
                // while the daemon assigns its own pane id), the daemon's
                // broadcast targeted the wrong window or no window at all.
                // Resolve by pane id so producer and consumer agree on the
                // contract (gpt-architecture + gpt-spec cross-model finding).
                match msg.decode_payload::<RenameWindowMsg>() {
                    Some(rename) => {
                        let renamed = self
                            .mux_group
                            .as_mut()
                            .and_then(|g| {
                                let idx = g.index_of_pane_id(msg.pane_id)?;
                                let window_id = g.windows().get(idx)?.id;
                                Some(g.rename_window_id(window_id, rename.name.clone()))
                            })
                            .unwrap_or(false);
                        if renamed {
                            log::info!(
                                "mux apc: pane {} renamed to {:?} for tab {:?}",
                                msg.pane_id,
                                rename.name,
                                self.title
                            );
                        }
                        renamed
                    }
                    None => {
                        log::warn!("mux apc: malformed RenameWindow payload");
                        false
                    }
                }
            }
            MessageType::PtyExited => {
                // A window's shell exited: remove its window/pane. The group
                // keeps rendering sub-tabs down to a single window; only
                // dropping to zero ends the mux session for this tab. Unlike an
                // explicit `Detach` (which reverts to a plain tab), the last
                // window's shell exiting means there is nothing left to show, so
                // the tab itself is closed — `exited` makes `App::pump_all` reap
                // it just like a local shell that ran out (otherwise the empty
                // mux tab lingers and blocks `mux kill`).
                // Capture the displayed (active) pane id before removal so the
                // close-reconcile decision can tell "active window closed"
                // (redraw needed) from "non-active window closed, indices
                // shifted but the displayed pane is unchanged" (no redraw).
                let before_active = self.mux_group.as_ref().and_then(|g| g.active_pane_id());
                let reconcile_target = match self.mux_group.as_mut() {
                    Some(group) => match group.remove_pane(msg.pane_id) {
                        Some(idx) => {
                            log::info!(
                                "mux apc: pane {} exited (window {}) for tab {:?}",
                                msg.pane_id,
                                idx,
                                self.title
                            );
                            // task0005 AC-6: latch the removed pane id for
                            // `App::pump_all` to discard the matching
                            // `App::agent_status` entry (this method has no
                            // `&mut App` access).
                            self.pending_closed_agent_status_panes.push(msg.pane_id);
                            if group.is_empty() {
                                self.mux_group = None;
                                self.exited = true;
                                // Group emptied: nothing to redraw (FR3).
                                None
                            } else {
                                // Active window may have changed; decide by pane
                                // id whether the screen needs a reconcile.
                                let after_active = group.active_pane_id();
                                Tab::close_reconcile_target(before_active, after_active)
                            }
                        }
                        // Unknown pane id: no removal, no reconcile.
                        None => return false,
                    },
                    None => {
                        log::info!(
                            "mux apc: remote pane {} exited for tab {:?}",
                            msg.pane_id,
                            self.title
                        );
                        return false;
                    }
                };
                // Reconcile the screen with the now-active window (parity with
                // the inbound `SwitchWindow` reconcile). `request_pane_snapshot`
                // is a fire-and-forget PTY write, so this is gated on the
                // decision rather than asserted directly in unit tests (FR1).
                if let Some(pane_id) = reconcile_target {
                    // Latch the outgoing (exited) pane id so App::pump_all's
                    // existing per-pane scroll save/restore block runs for this
                    // close path, mirroring the SwitchWindow arm. First-latch-
                    // only: if several PtyExited drain in one pump we keep the
                    // genuinely-displayed outgoing pane (intermediate panes were
                    // never rendered). The exited pane is already removed by
                    // remove_pane above, so App::index_of_pane_id returns None
                    // for it — the park is correctly skipped and only the new
                    // active pane's active_pane_scroll() is reloaded.
                    if let Some(before) = before_active {
                        if self.pending_pane_switch_from.is_none() {
                            self.pending_pane_switch_from = Some(before);
                        }
                    }
                    self.request_pane_snapshot(pane_id);
                }
                true
            }
            MessageType::Detached => {
                // The daemon confirmed our `Detach`: exit mux mode. Clear the
                // window group (the tab reverts to a plain tab) and the
                // session name (status-bar mux badge clears). Port of the
                // WebView `onDetached → exitMuxMode`.
                log::info!("mux apc: detached from session for tab {:?}", self.title);
                self.mux_group = None;
                self.mux_session_name = None;
                // Restore pre-mux routing: the next pump parses the PTS stream
                // with `self.core` again (the bridge process exits and hands the
                // PTY back to the shell). Drop any partial outer frame the
                // extractor was carrying so a stale half-sequence cannot corrupt
                // a future re-attach (FR5).
                self.mux_apc_extractor.reset();
                // Cancel any in-flight off-thread snapshot replay before
                // clearing the grid. Otherwise a switch dispatched just before
                // detach (target snapshot >= OFFTHREAD_REPLAY_THRESHOLD_BYTES)
                // would still resolve on a later `poll_pending_switch`, swapping
                // the worker-built core (the detached window's content) back
                // over the grid we clear below. Mirrors the synchronous
                // `Snapshot` arm's supersede-the-pending-switch step.
                // `supersede_pending_replay` also drops any coalesced
                // same-pane re-dispatch (FR7/FR8, task0003 — it belonged to
                // the pane being cleared too, and letting a later
                // `poll_pending_switch` dispatch it would revive a stale
                // request for a pane this tab no longer shows) and cancels
                // any in-flight 2nd-pass scrollback restore.
                let _ = self.supersede_pending_replay("mux detached");
                // The displayed grid still holds the detached mux window's
                // content. The bridge process exits right after this Detached
                // frame (mux::bridge → process::exit), handing the PTY back to
                // the shell that ran `emterm mux attach`, which reprints its
                // prompt — but on a clean screen only if we drop the stale mux
                // frame now. Reuse the PaneCreated append recipe (clear grid +
                // prompts/folds via reset_and_replay(b""), latch
                // pending_frame_reset so App::pump_all drops any selection and
                // forces a full redraw). Without this the detached session's
                // screen lingers until the shell happens to overwrite it.
                let _ = self.reset_frame_for_replay(b"", &[]);
                true
            }
            other => {
                log::debug!("mux apc: unhandled message type {other:?}");
                false
            }
        }
    }

    /// `Snapshot` / `SnapshotRestore` arm of [`Self::apply_mux_message`]:
    /// pane-filter, payload decode, and sync / off-thread replay dispatch.
    fn handle_snapshot(&mut self, msg_type: MessageType, pane_id: u32, payload: &[u8]) -> bool {
        // task0003 D3 (review round-2 findings `200b2c8beeb68fe4` /
        // `87ba3cc2911d104e`): a frame that RESETS the tab's single
        // core must only be applied when it belongs to the pane this
        // tab is currently displaying — mirrors the `PtyOutput` arm's
        // filter below. Both the reattach path (per-pane
        // `SnapshotRestore`) and the visibility-resume path
        // (per-pane `Snapshot`) send one such frame per pane in the
        // session, relying on the CLIENT to pick the right one; this
        // arm used to apply whatever arrived last unconditionally,
        // so a background window's reattach / resume snapshot
        // silently overwrote the visible pane's content with a
        // different window's screen — re-introducing, via this
        // newer per-pane framing, the exact "switch shows the wrong
        // pane's content" symptom this feature exists to fix. When
        // the tab has no window group (older daemon / single pane),
        // `active_pane_id()` is `None` and every frame is accepted,
        // matching the `PtyOutput` arm's fallback.
        if let Some(active) = self.mux_group.as_ref().and_then(|g| g.active_pane_id()) {
            if pane_id != active {
                log::debug!(
                    "mux apc: dropping {:?} for non-active pane {} (active {}) for tab {:?}",
                    msg_type,
                    pane_id,
                    active,
                    self.title
                );
                return false;
            }
        }
        // task0004 round-4 rework (D1'): decode the wire payload
        // into its structural dimension segments + plain content
        // bytes (`mux_ipc::protocol::decode_snapshot_payload_typed`).
        // An older daemon's payload (no magic prefix) decodes as
        // `Legacy`, degrading to single-dimension replay (AC-11) —
        // see `reset_and_replay_segments`'s doc comment.
        //
        // D3''' (round-6 rework, review round-5 finding
        // `b45fb09344067621`): use the TYPED result, not the tuple
        // compatibility wrapper — `Malformed` there maps to
        // `(Vec::new(), &[])`, which this arm would apply as "empty
        // snapshot," blanking the pane the same way rendering the
        // corrupt envelope literally would have. A `Malformed`
        // frame here instead logs and skips applying it entirely,
        // leaving whatever is currently displayed intact.
        #[cfg(test)]
        {
            // task0006 FR8 AC-6: counts every decode attempt,
            // including one that will end up coalescing into an
            // already-in-flight same-pane switch below — see
            // `test_snapshot_decode_count`'s doc for what this
            // does (and does not) claim about FR8's scope.
            self.snapshot_decode_count += 1;
        }
        let (dim_segments, content_bytes) = match decode_snapshot_payload_typed(payload) {
            DecodedSnapshotPayload::Legacy(content) => (Vec::new(), content.to_vec()),
            DecodedSnapshotPayload::Structured { segments, content } => {
                (segments, content.to_vec())
            }
            DecodedSnapshotPayload::Malformed => {
                log::warn!(
                    "mux apc: dropping malformed {:?} payload ({} bytes) for tab {:?} \
                     (pane {}); keeping the current display",
                    msg_type,
                    payload.len(),
                    self.title,
                    pane_id
                );
                return false;
            }
        };
        let segments: Vec<ReplaySegment> = dim_segments
            .iter()
            .map(|d| ReplaySegment {
                offset: d.offset,
                cols: d.cols,
                rows: d.rows,
            })
            .collect();
        // FR4: branch on payload size. Small snapshots replay
        // synchronously (no perceptible block, no swap gap); large
        // ones go off-thread so the switch stays responsive.
        //
        // D3''/AC-5 (task0005 rework, review round-4 finding
        // `b1de83542bfe60bc`): ALSO branch on segment count — a
        // small-payload, many-segment snapshot (a resize-drag-shaped
        // sequence) can still cost real reflow time on the
        // synchronous path, since each segment's reflow cost scales
        // with the core's accumulated size, not the segment's own
        // byte count.
        if content_bytes.len() < OFFTHREAD_REPLAY_THRESHOLD_BYTES
            && segments.len() < OFFTHREAD_REPLAY_SEGMENT_THRESHOLD
        {
            // Synchronous path (legacy). `reset_frame_for_replay`
            // owns the recipe (prompt clear, fold rebuild, drain +
            // backfill marks so `pending_frame_reset` latches) so the
            // PaneCreated path stays in lockstep. A pending off-thread
            // switch (if any) is superseded by this newer, now-applied
            // switch — `supersede_pending_replay` signals its worker
            // to bail before dropping it, drops any coalesced
            // same-pane re-dispatch (FR7/FR8, task0003 — now moot,
            // this sync application is itself the newest switch), and
            // cancels any in-flight 2nd-pass scrollback restore.
            let _ = self.supersede_pending_replay("superseded by sync switch");
            let _actions = self.reset_frame_for_replay(&content_bytes, &segments);
            log::debug!(
                "mux apc: applied {:?} ({} bytes, {} segments, sync) for tab {:?}",
                msg_type,
                content_bytes.len(),
                segments.len(),
                self.title
            );
        } else {
            // Off-thread path (FR1/FR4): copy the payload, do the
            // frame-discard portion now (prompts/folds belonged to
            // the outgoing frame), and dispatch a worker. The
            // displayed core is left intact so the outgoing pane
            // stays visible until the swap. A newer switch supersedes
            // any prior in-flight parse.
            //
            // The live-output queue is keyed on the tab's *active*
            // pane id (the pane `switch_to` already moved to), the
            // same id the `PtyOutput` arm filters on, so live bytes
            // for the just-switched-to pane queue while the parse runs
            // instead of being dropped. Fall back to the snapshot's
            // own `pane_id` when the tab has no window group.
            let target_pane = self
                .mux_group
                .as_ref()
                .and_then(|g| g.active_pane_id())
                .unwrap_or(pane_id);
            let segments_len = segments.len();
            self.dispatch_offthread_replay(target_pane, content_bytes, segments);
            log::debug!(
                "mux apc: dispatched {:?} ({} bytes, {} segments, off-thread) for tab {:?} pane {}",
                msg_type,
                self.pending_switch
                    .as_ref()
                    .map(|p| p.payload.len())
                    .unwrap_or(0),
                segments_len,
                self.title,
                target_pane
            );
        }
        true
    }

    /// `PtyOutput` arm of [`Self::apply_mux_message`]: pane routing, the
    /// pending-switch live queue (with its overflow fallback), and the
    /// live-output apply.
    fn handle_pty_output(&mut self, pane_id: u32, payload: Vec<u8>) -> bool {
        // OSC-probe (temporary): flag when GUI-side sees a viewer
        // launch OSC 777 arrive from the mux extractor. Mirrors the
        // daemon (pty_spawn.rs) and bridge (bridge.rs) probes. Only
        // metadata is logged (never the payload bytes) so this probe
        // cannot leak user file content into persisted release logs.
        const OSC_PROBE_NEEDLE: &[u8] = b"\x1b]777;emterm;";
        let osc_probe = payload
            .windows(OSC_PROBE_NEEDLE.len())
            .position(|w| w == OSC_PROBE_NEEDLE);
        if let Some(off) = osc_probe {
            log::warn!(
                "[osc-probe gui] enter pane={} payload_len={} osc_off={}",
                pane_id,
                payload.len(),
                off,
            );
        }
        // Route by pane. Once attached (see the Welcome handler), the
        // daemon streams live output for *every* pane in the session to
        // this owning connection — but native renders one core per tab,
        // showing only the active window. Feeding another window's bytes
        // into this core interleaves unrelated screens (the "other
        // tabs' data mixing in" symptom). The WebView keeps a separate
        // core per pane; native instead drops non-active panes here and
        // reconciles each window's screen from the daemon's
        // authoritative state via `request_pane_snapshot` on switch.
        // When the tab has no window group (older daemon / single
        // pane), `active_pane_id()` is None and all output is accepted.
        if let Some(active) = self.mux_group.as_ref().and_then(|g| g.active_pane_id()) {
            if pane_id != active {
                if osc_probe.is_some() {
                    log::warn!(
                        "[osc-probe gui] DROP inactive-pane pane={} active={} payload_len={}",
                        pane_id,
                        active,
                        payload.len(),
                    );
                }
                log::debug!(
                    "mux apc: dropping PtyOutput for inactive pane {} (active {})",
                    pane_id,
                    active
                );
                return false;
            }
        }
        // FR3: while an off-thread replay for this pane is in flight,
        // the displayed core is still showing the *outgoing* pane —
        // feeding the just-switched-to pane's live bytes into it would
        // corrupt the visible screen. Queue them in arrival order
        // instead; `App::pump_all` replays the queue onto the
        // worker-built core after the swap. Output that races in for a
        // *different* target than the pending switch is dropped (it
        // belongs to a pane we are no longer switching to).
        if let Some(pending) = self.pending_switch.as_mut() {
            if pane_id == pending.target_pane {
                if osc_probe.is_some() {
                    log::warn!(
                        "[osc-probe gui] QUEUED pending-switch pane={} target={} payload_len={}",
                        pane_id,
                        pending.target_pane,
                        payload.len(),
                    );
                }
                pending.queued_bytes = pending.queued_bytes.saturating_add(payload.len());
                pending.live_queue.push(payload);
                // Bound the backlog: past the cap, abandon the
                // off-thread switch and reparse synchronously now,
                // applying the accumulated queue as ordinary output.
                // This caps both the swap-time replay burst and the
                // memory a fast pane can accumulate during a slow parse.
                if pending.queued_bytes > OFFTHREAD_LIVE_QUEUE_CAP_BYTES {
                    // Take the coalesced re-dispatch (if any) BEFORE
                    // superseding — `supersede_pending_replay` drops
                    // `pending_redispatch`, but this fallback still
                    // needs it as the latest known content for the
                    // pane (see the payload selection below).
                    let redispatch = self.pending_redispatch.take();
                    let pending = self
                        .supersede_pending_replay("live-queue overflow sync reparse")
                        .expect("pending_switch is Some in this arm");
                    log::warn!(
                        "mux off-thread replay live-queue exceeded {} bytes for tab {:?}; \
                         synchronous reparse fallback",
                        OFFTHREAD_LIVE_QUEUE_CAP_BYTES,
                        self.title
                    );
                    // FR8 (task0003): a coalesced same-pane
                    // re-dispatch (if any) is the LATEST known
                    // content for this pane — reparse that instead
                    // of the (possibly superseded) payload the
                    // abandoned worker was building, so this
                    // synchronous fallback never regresses to
                    // stale content. `self.core` is already at the
                    // right grid regardless (`Tab::resize` always
                    // resizes it directly, independent of any
                    // deferred `pending_resize`), so
                    // `reset_frame_for_replay` needs no extra
                    // resize step here.
                    //
                    // `pending.live_queue` is safe to apply
                    // unconditionally against whichever payload
                    // wins above (review round-1 finding
                    // `ebc9de26bb15fcb1`, task0006 redesign): the
                    // same-pane coalesce branch in
                    // `dispatch_offthread_replay` already cleared
                    // it at COALESCE time if `pending_redispatch`
                    // is `Some` here, so it holds exactly "output
                    // queued since that coalesce" either way —
                    // never a stale prefix the new payload might
                    // already contain.
                    let (payload, segments) = match redispatch {
                        Some((_, payload, segments)) => (payload, segments),
                        None => (pending.payload, pending.segments),
                    };
                    self.reset_frame_for_replay(&payload, &segments);
                    self.apply_queued_live_output(pending.live_queue);
                    // The swap-equivalent happened synchronously now;
                    // repaint the newly-visible pane.
                    return true;
                }
                // Queued, not yet visible — no redraw needed; the swap
                // will repaint.
                return false;
            }
            if osc_probe.is_some() {
                log::warn!(
                    "[osc-probe gui] DROP pending-switch pane={} target={} payload_len={}",
                    pane_id,
                    pending.target_pane,
                    payload.len(),
                );
            }
            log::debug!(
                "mux apc: dropping PtyOutput for pane {} during pending switch to {}",
                pane_id,
                pending.target_pane
            );
            return false;
        }
        if osc_probe.is_some() {
            log::warn!(
                "[osc-probe gui] APPLY pane={} payload_len={}",
                pane_id,
                payload.len(),
            );
        }
        // The daemon's continuous PTY stream: feed it into term_core
        // as a normal byte stream (NOT a reset). Without this the
        // mux session looks frozen after the initial Snapshot. Shares
        // the post-parse recipe (device-response write-back + mark
        // drain/backfill) with the coalesce flush via
        // `apply_active_pane_output`, so the per-frame and batched
        // paths can never drift (SPEC NFR2). Frames carrying a device
        // query (`CSI ... n` / `CSI ... c`) are routed here per-frame
        // by `process_combined`'s `batch_eligible` gate so each reply
        // is captured before the next query overwrites the core's
        // single-slot response buffer.
        self.apply_active_pane_output(&payload)
    }

    /// Request an on-demand screen snapshot for `pane_id`. The daemon replies
    /// with a `PtyOutput` frame (a screen reset + shadow-parser replay) that
    /// `apply_mux_message` feeds into `term_core`, so the displayed grid is
    /// reconciled with the daemon's authoritative state. Without this, an
    /// attach / window switch leaves the target pane's screen blank or stale —
    /// the daemon does not push the active screen unprompted.
    ///
    /// Port of `requestPaneSnapshot` (`MuxClient.sendRequestPaneSnapshot`).
    /// Fire-and-forget; returns `false` when the tab has no live PTY.
    pub fn request_pane_snapshot(&self, pane_id: u32) -> bool {
        use mux_ipc::protocol::{MessageType, MuxMessage};
        self.send_control(&MuxMessage {
            msg_type: MessageType::RequestPaneSnapshot,
            pane_id,
            payload: Vec::new(),
        })
    }

    /// Register this connection as the live-output *owner* of `session_id` by
    /// sending an `Attach` control frame, mirroring the WebView reattach path
    /// (`enterMuxMode` in `src/terminal-app/mux/mux-session.ts`).
    ///
    /// The daemon streams continuous PTY output only to a pane's single owning
    /// connection. Without an Attach, native receives on-demand snapshots
    /// (`request_pane_snapshot`) but no live updates, so programs like `top`
    /// look frozen. Sending Attach installs this connection's output channel as
    /// the pane owner — evicting any prior client (e.g. an attached WebView) —
    /// and replays the daemon's buffered output for the session's panes.
    ///
    /// The `AttachMsg` payload is bincode-serialized (`session_id` as a 4-byte
    /// LE u32), matching the WebView wire shape. Fire-and-forget; returns
    /// `false` when the tab has no live PTY.
    pub fn send_attach(&self, session_id: u32) -> bool {
        use mux_ipc::protocol::{AttachMsg, MessageType, MuxMessage};
        self.send_control(&MuxMessage::control(
            MessageType::Attach,
            0,
            &AttachMsg { session_id },
        ))
    }
}

/// Split a drained `pending_apc` buffer into the (image-pipeline,
/// mux-message) halves. APC payloads that start with `emterm-mux;` are
/// decoded into typed `MuxMessage`s via
/// [`crate::mux::apc::try_decode_emterm_mux`]; the rest pass through to
/// the existing Kitty Graphics decoder. Decode failures on a clearly
/// mux-prefixed payload are dropped (the helper already logs at `warn`)
/// rather than fed to the image pipeline — they cannot be valid Kitty.
pub(super) fn partition_apc_for_mux(apc: Vec<Vec<u8>>) -> (Vec<Vec<u8>>, Vec<MuxMessage>) {
    let mut images: Vec<Vec<u8>> = Vec::with_capacity(apc.len());
    let mut mux: Vec<MuxMessage> = Vec::new();
    for payload in apc {
        if payload.starts_with(mux_ipc::protocol::APC_PREFIX.as_bytes()) {
            if let Some(msg) = crate::mux::apc::try_decode_emterm_mux(&payload) {
                mux.push(msg);
            }
            // Malformed mux payload — already logged inside the decoder;
            // do NOT forward to the image pipeline.
        } else {
            images.push(payload);
        }
    }
    (images, mux)
}
