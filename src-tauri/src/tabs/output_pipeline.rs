//! Output pipeline of [`Tab`]: PTY event pump, mux-frame extraction and
//! coalesced parsing of the combined byte stream.

use mux_ipc::protocol::{MessageType, MuxMessage};

use crate::pty::{ExitReason, PtyEvent};

use super::Tab;
use super::input::payload_has_device_query;
use super::marks_fold::drain_marks;
use super::mux_link::partition_apc_for_mux;

impl Tab {
    /// Drain pending PTY events into the terminal core. Returns true if
    /// anything changed (caller should request a redraw).
    ///
    /// Frame budget (FRAME_BUDGET_MS): bound how long one `pump` call
    /// spends inside `process_pty_data`. Bursty producers like
    /// `seq 1 10000000` would otherwise let a single pump tick eat the
    /// whole frame and freeze input/render until the burst drained.
    /// When the budget is exhausted we stop draining and request another
    /// frame via `crate::wakeup::wake()` so the remainder is processed
    /// on the next about_to_wait pass.
    pub fn pump(&mut self) -> bool {
        const FRAME_BUDGET_MS: u128 = 12;
        // Coalesce target: drain as many Data chunks as the frame budget
        // allows into one contiguous buffer, then run a SINGLE
        // process_pty_data + flush_grapheme_buffer + take_mode_actions
        // cycle. Per-chunk lock/flush/take overhead was the dominant
        // cost when the shell wrote tiny lines (`seq` produced ~41
        // bytes/chunk in benchmarking, so 200 chunks × 60µs overhead
        // ate the whole 12ms budget per frame and capped throughput
        // around 1MB/s). Coalescing makes one PTY chunk and 200 PTY
        // chunks cost roughly the same.
        const COALESCE_CAP: usize = 1024 * 1024;
        let start = std::time::Instant::now();
        let mut changed = false;
        let mut yielded = false;
        let mut combined: Vec<u8> = Vec::new();
        let mut saw_exit: Option<ExitReason> = None;
        while let Ok(evt) = self.events.try_recv() {
            match evt {
                PtyEvent::Data(bytes) => {
                    if combined.is_empty() {
                        combined = bytes;
                    } else {
                        combined.extend_from_slice(&bytes);
                    }
                    if combined.len() >= COALESCE_CAP
                        || start.elapsed().as_millis() >= FRAME_BUDGET_MS
                    {
                        yielded = true;
                        break;
                    }
                }
                PtyEvent::Exited { reason } => {
                    saw_exit = Some(reason);
                    break;
                }
            }
        }
        if self.process_combined(combined) {
            changed = true;
        }
        if let Some(reason) = saw_exit {
            match reason {
                ExitReason::Eof => log::info!("tab {:?} exited: EOF", self.title),
                ExitReason::ReadError(e) => log::warn!("tab {:?} read error: {e}", self.title),
            }
            self.exited = true;
            changed = true;
        }
        // Yielded mid-burst: schedule another wakeup so the next about_to_wait
        // continues draining instead of waiting for the 16ms WaitUntil deadline.
        if yielded {
            crate::wakeup::wake();
        }

        changed
    }

    /// Process one coalesced PTS buffer + the callback-state side effects it
    /// produced, returning whether the visible state changed.
    ///
    /// Split out of [`Self::pump`] so the parse / mux-decode / image-drain path
    /// is exercised by deterministic unit tests (which feed a known buffer)
    /// rather than the live PTY channel. `pump` calls this once per frame with
    /// the bytes it coalesced (possibly empty — the callback drains still run).
    /// Drive `self.core` over an outer-stream byte slice (the pre-mux parse
    /// path), running the grapheme flush, device-response write-back, and
    /// OSC 133 / fold mark drains that apply when `self.core` itself parses the
    /// outer bytes.
    ///
    /// Used by the pre-mux branch of [`Self::process_combined`] and, when a
    /// `Detached` frame appears mid-buffer, by the post-detach tail re-route
    /// (FR5): the bytes coalesced behind the `Detached` frame are plain shell
    /// output that must reach `self.core`, not the (now reset) transport
    /// extractor.
    fn process_outer_via_core(&mut self, bytes: &[u8]) {
        let mut c = self.core.lock();
        c.process_pty_data_fully(bytes);
        // Force-flush any grapheme cluster left buffered by the
        // parser (e.g. a lone emoji codepoint at the tail of an
        // IME-commit echo). Without this the cluster sits in
        // `grapheme_buffer` until the next non-extending codepoint
        // arrives, so the glyph stays invisible and the cursor
        // doesn't advance until the user types something else
        // (typical symptom: SKK `/smile` → 😄 only appears after
        // pressing space).
        c.flush_grapheme_buffer();
        // Pick up any device-status / DA / XTWINOPS reply term_core
        // synthesized while processing this chunk. PowerShell +
        // PSReadLine issue `\x1b[6n` cursor-position queries during
        // every line redraw; without writing the reply back into the
        // PTY, PSReadLine recomputes the redraw against a stale
        // cursor and a single Backspace erases multiple cells.
        let device_response = c.take_response();
        // Drain the OSC 133 marks `term_core` captured during this pump
        // (each already stamped with its emit-time row + eviction count)
        // and read the current eviction total — all under the core lock
        // so they are consistent with the bytes just processed. The
        // actual backfill runs after `drop(c)` because it needs
        // `&mut self`.
        let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
        drop(c);
        if !device_response.is_empty() {
            self.write_device_response(device_response);
        }
        // agent-exit-after-icon (task0002 deviation — see task0002's
        // implementer report): reconcile this pump's OSC 133 mark
        // CANDIDATES (`cb_state.pending_latch_feed`, populated by
        // `NativeCallbacks::on_osc` in true synchronous order alongside
        // OSC 777 Set/Clear — see `callbacks::LatchFeedEvent`'s doc)
        // against `pending_marks` (`term_core`'s alt-screen-filtered,
        // authoritative live-mark list just drained above) to produce a
        // true-order, live-only sequence for this tab's inferred-clear
        // latch (FR4/FR5). Computed from `&pending_marks` BEFORE
        // `backfill_marks` below consumes it by value.
        let live_kinds: Vec<crate::prompts::PromptMarkKind> = pending_marks
            .iter()
            .filter_map(|m| crate::prompts::PromptMarkKind::from_byte(m.kind))
            .collect();
        let latch_feed = std::mem::take(&mut self.cb_state.lock().pending_latch_feed);
        if !latch_feed.is_empty() {
            self.pending_latch_inputs
                .extend(crate::agent_status_model::reconcile_latch_feed(
                    latch_feed,
                    &live_kinds,
                ));
        }
        self.backfill_marks(evicted_total, pending_marks, pending_fold_marks);
        // New PTY bytes reached the core — latch for the
        // inactive-tab activity path (WebView `onOutputActivity`).
        self.output_pending = true;
    }

    /// FR1/FR4/FR5: whether a `PtyOutput` frame may join the coalesce
    /// accumulator. Eligible only when it is addressed to the active pane (or
    /// the tab has no window group, so all output is accepted), there is no
    /// in-flight off-thread replay (`pending_switch`), and it carries no device
    /// query (see [`payload_has_device_query`]) — a query-bearing frame must be
    /// parsed on its own so its reply is captured before a later query
    /// overwrites `term_core`'s single-slot response buffer. Anything failing this gate is
    /// a boundary handled per-frame by [`Self::apply_mux_message`]. This is the
    /// single definition of "batch-eligible"; `process_combined` calls it so
    /// the classification is not duplicated inline.
    fn pty_output_batch_eligible(&self, msg: &MuxMessage) -> bool {
        if msg.msg_type != MessageType::PtyOutput {
            return false;
        }
        let active_pane = self.mux_group.as_ref().and_then(|g| g.active_pane_id());
        active_pane.map(|a| a == msg.pane_id).unwrap_or(true)
            && self.pending_switch.is_none()
            && !payload_has_device_query(&msg.payload)
    }

    /// Shared post-parse recipe for active-pane inner output, called by BOTH
    /// the coalesce flush ([`Self::flush_coalesced_output`]) and the per-frame
    /// `PtyOutput` arm of [`Self::apply_mux_message`]. Keeping it in one place
    /// means the "coalesce is a pure performance change" invariant (SPEC NFR2)
    /// is enforced by a single source of truth instead of two hand-mirrored
    /// copies that could silently drift. Parses `bytes` in one
    /// `process_pty_data_fully` call, writes back any device-status reply
    /// (`take_response`), and drains + backfills OSC 133 / fold marks. Always
    /// returns `true` (the bytes reached the core).
    pub(super) fn apply_active_pane_output(&mut self, bytes: &[u8]) -> bool {
        let (evicted_total, pending_marks, pending_fold_marks, device_response) = {
            let mut c = self.core.lock();
            c.process_pty_data_fully(bytes);
            let device_response = c.take_response();
            let (evicted_total, pending_marks, pending_fold_marks) = drain_marks(&mut c);
            (
                evicted_total,
                pending_marks,
                pending_fold_marks,
                device_response,
            )
        };
        // Route any device-status reply (e.g. CPR synthesized for a PSReadLine
        // `\x1b[6n` query) back to the originating remote pane via PtyInput
        // framing so PSReadLine cursor tracking stays accurate over mux.
        if !device_response.is_empty() {
            self.write_device_response(device_response);
        }
        // Drain/backfill so prompt marks and custom-fold begin/end pairs
        // arriving over the mux stream are navigable / foldable too.
        self.backfill_marks(evicted_total, pending_marks, pending_fold_marks);
        true
    }

    /// FR1/FR3: flush the coalesce accumulator built in
    /// [`Self::process_combined`]. Parses the concatenated inner payloads of a
    /// consecutive active-pane `PtyOutput` run in ONE `process_pty_data_fully`
    /// call via [`Self::apply_active_pane_output`], running the per-batch side
    /// effects exactly once. Inner image APC/DCS emitted by the parse are
    /// drained once per pump by the post-loop block in `process_combined`, so
    /// they need no per-batch handling here. The accumulator is cleared.
    ///
    /// Returns `true` when bytes were applied (the caller sets `changed`).
    /// An empty accumulator is a no-op returning `false`.
    fn flush_coalesced_output(&mut self, acc: &mut Vec<u8>) -> bool {
        if acc.is_empty() {
            return false;
        }
        let applied = self.apply_active_pane_output(acc.as_slice());
        #[cfg(test)]
        {
            self.coalesce_parse_passes += 1;
        }
        acc.clear();
        applied
    }

    pub(super) fn process_combined(&mut self, combined: Vec<u8>) -> bool {
        let mut changed = false;
        // Mux-transport frames extracted from the coalesced PTS bytes this
        // pump (mux branch only), each paired with its end offset in `combined`
        // so a `Detached` frame's boundary can be located (FR5). Merged into
        // `pending_apc` further down so they flow through the same
        // `partition_apc_for_mux` sink the pre-mux `self.core` parse feeds.
        let mut extracted_mux_apc: Vec<(Vec<u8>, usize)> = Vec::new();
        // task0005 rework (round1 findings 6b2e83f10c94ad7e / 929859ff2b4e431e
        // / 5cd6f305dcdeceb7): snapshot mux attachment as it stood at the
        // START of this pump, before anything below (including a `Detached`
        // frame extracted from `combined`) can change `self.mux_session_name`.
        // Used further down to decide whether `pending_agent_status` /
        // `pending_latch_feed` candidates parsed during this pump belong to
        // a mux pane's inner content (discard — the daemon is authoritative
        // for mux panes, SPEC FR3) rather than to plain shell output.
        let was_mux_at_pump_start = self.mux_session_name.is_some();
        if !combined.is_empty() {
            if self.mux_session_name.is_some() {
                // Mux established (FR1 / FR2): the outer PTS stream is the mux
                // transport. Parse it with the tab's INDEPENDENT extractor, not
                // `self.core`, so `self.core` is driven by the inner content
                // only (via `apply_mux_message`). This keeps an inner Kitty
                // chunk's parser state intact across `PtyOutput` boundaries.
                // The outer mux stream is APC-only (no printing, no device
                // queries, no OSC 133 marks), so the `self.core`-side grapheme
                // flush / device-response / mark-drain do NOT apply here.
                extracted_mux_apc = self.mux_apc_extractor.feed_with_offsets(&combined);
                changed = true;
                self.output_pending = true;
            } else {
                self.process_outer_via_core(&combined);
                changed = true;
            }
        }

        // Sync title from callback state if the shell sent a new one.
        {
            let mut s = self.cb_state.lock();
            if let Some(t) = s.title.take() {
                if t != self.title {
                    self.title = t;
                    changed = true;
                }
            }

            // Latch BEL rings for `App::pump_all` to dispatch per
            // `settings.bell_action` (visual flash / beep / none).
            if std::mem::take(&mut s.bell_count) > 0 {
                self.bell_pending = true;
            }

            // Device responses (DA1/DA2/DSR/XTWINOPS/DECRPM) are NOT
            // drained/written here (tmux-startup-query-response-leak
            // task0001/task0002). They are already delivered exactly once
            // — in synthesis order, ALL of them from the pump's parse, per
            // task0002's ordered-drain contract — by whichever write-back
            // site (`process_outer_via_core`, `apply_active_pane_output`,
            // `apply_queued_live_output`) just parsed the bytes that
            // produced them, via `take_response()` / `write_device_response`
            // — the SOLE PTY delivery route. A second delivery used to
            // happen here, sourced from `NativeCallbackState::
            // device_responses` (fed by `NativeCallbacks::
            // on_device_response`, a documented no-op after task0001) and
            // written raw via `Tab::write`, bypassing mux routing entirely;
            // that redundant channel caused the query's application (e.g.
            // tmux) to see the reply twice and forward the stray second
            // copy to the shell as ordinary input, which echoed onto the
            // screen. task0002 removed the channel outright — `on_device_
            // response` no longer exists on `TerminalCallbacks` at all.
            // Drain buffered image-protocol payloads so we can decode them
            // outside the lock (the decoder needs cursor coords from `core`
            // — see comment in `drain_and_decode_images` below).
            let pending_apc: Vec<Vec<u8>> = std::mem::take(&mut s.pending_apc);
            let pending_dcs: Vec<Vec<u8>> = std::mem::take(&mut s.pending_dcs);
            // Phase 6: drain the theme-dirty latch. When an OSC 4/10/11/12/
            // 22/104/110/111/112 mutated the shared `Theme`, every row
            // must repaint with the new palette on the next frame.
            let theme_changed = std::mem::take(&mut s.theme_dirty);
            // `pending_agent_status` (plain-tab OSC 777 events) and
            // `pending_latch_feed` (OSC 133/777 latch candidates) are NOT
            // drained here — deliberately, task0005 rework. Both are
            // populated by `NativeCallbacks::on_osc`, fired for EITHER a
            // pre-mux outer parse (already done above, before this block, if
            // `mux_session_name` was `None` at pump start) OR mux inner
            // content parsed by the frame-apply loop further down (which
            // runs AFTER this point). Draining here would race that loop:
            // for a mux-attached pump, whatever is queued right now is only
            // stale leftovers, and for a same-pump mux→detach transition the
            // loop's mux-inner-origin candidates would not exist yet to be
            // excluded. See the discard-then-final-drain below the
            // frame-apply loop (before/after the detach tail re-route) for
            // where both queues are actually consumed this pump.
            drop(s);
            if theme_changed {
                self.core.lock().mark_all_dirty();
                changed = true;
            }
            // Split the APC stream: payloads addressed to the `emterm-mux;`
            // inband protocol are decoded and applied to this tab's state;
            // everything else (Kitty graphics) falls through to the image
            // pipeline. `pending_dcs` is image-only (SIXEL).
            //
            // Pre-mux: `pending_apc` was populated by the `self.core` outer
            // parse above (which fired `on_apc`). Mux: the outer parse went to
            // the independent extractor instead, so the mux frames arrive via
            // `extracted_mux_apc` (each carrying its end offset in `combined`).
            // The extracted frames decode before any pre-mux `pending_apc` so
            // inner content this same pump applies in order.
            //
            // FR5 detach transition: a single coalesced buffer may carry
            // `[... Detached frame][post-detach shell bytes]`. The `Detached`
            // frame clears `mux_session_name` mid-loop; the bytes after it are
            // plain shell output the extractor would otherwise discard. Watch
            // for the Some→None transition while applying the extracted frames
            // and capture the offset just past the frame that triggered it, so
            // the tail can be re-routed through `self.core` below.
            let mut image_apc: Vec<Vec<u8>> = Vec::new();
            let mut detach_tail_start: Option<usize> = None;
            // FR1: concatenation buffer for the inner payloads of consecutive
            // batch-eligible active-pane `PtyOutput` frames. Parsed once per
            // run by `flush_coalesced_output` at every boundary and at loop end,
            // instead of once per frame — collapsing the ~1400-parse-per-pump
            // flood into one parse per consecutive run.
            let mut coalesce_acc: Vec<u8> = Vec::new();
            for (payload, end_offset) in extracted_mux_apc {
                if payload.starts_with(mux_ipc::protocol::APC_PREFIX.as_bytes()) {
                    if let Some(msg) = crate::mux::apc::try_decode_emterm_mux(&payload) {
                        // FR1/FR4/FR5 classify (see `pty_output_batch_eligible`):
                        // an active-pane `PtyOutput` with no in-flight off-thread
                        // replay and no device query is batch-eligible and
                        // accumulates without an immediate parse. Everything else
                        // (control message / non-active pane / pending_switch /
                        // detach / device-query frame) is a boundary: flush the
                        // accumulator first, then handle the frame via the
                        // existing per-frame path.
                        if self.pty_output_batch_eligible(&msg) {
                            coalesce_acc.extend_from_slice(&msg.payload);
                            // No immediate parse; continue accumulating the run.
                            continue;
                        }
                        // Boundary: flush the accumulated active-pane run BEFORE
                        // handling this frame so output/control ordering matches
                        // the per-frame path exactly.
                        if self.flush_coalesced_output(&mut coalesce_acc) {
                            changed = true;
                        }
                        let was_mux = self.mux_session_name.is_some();
                        if self.apply_mux_message(msg) {
                            changed = true;
                        }
                        // Detach: a frame just cleared `mux_session_name`. The
                        // remaining bytes in `combined` belong to the shell, not
                        // the mux transport — record where they start and STOP
                        // applying extracted frames. Every later frame was pulled
                        // from `combined[end_offset..]`, which the tail re-route
                        // below re-parses through `self.core`; continuing the loop
                        // would process those bytes twice (e.g. double-decoding a
                        // post-detach image, or leaking a re-attach frame).
                        if was_mux && self.mux_session_name.is_none() {
                            detach_tail_start = Some(end_offset);
                            break;
                        }
                    }
                    // Malformed mux payload — already logged inside the decoder;
                    // do NOT forward to the image pipeline.
                } else {
                    // A bare (non-mux) APC frame extracted from the transport
                    // stream is an inner Kitty image. It is a boundary for the
                    // active-pane run: flush before queueing it so ordering is
                    // preserved.
                    if self.flush_coalesced_output(&mut coalesce_acc) {
                        changed = true;
                    }
                    image_apc.push(payload);
                }
            }
            // FR1: flush the final accumulated run (loop ended without a
            // boundary frame).
            if self.flush_coalesced_output(&mut coalesce_acc) {
                changed = true;
            }
            // Pre-mux `pending_apc` (no offsets): partition + apply as before.
            let (pre_mux_images, pre_mux_messages) = partition_apc_for_mux(pending_apc);
            image_apc.extend(pre_mux_images);
            for msg in pre_mux_messages {
                if self.apply_mux_message(msg) {
                    changed = true;
                }
            }
            if (!image_apc.is_empty() || !pending_dcs.is_empty())
                && self.drain_and_decode_images(&image_apc, &pending_dcs)
            {
                changed = true;
            }
            // task0005 rework (round1 findings 6b2e83f10c94ad7e /
            // 929859ff2b4e431e): discard any `pending_agent_status` /
            // `pending_latch_feed` entries that accumulated from mux INNER
            // content this pump — the frame-apply loop just above drives
            // `self.core` for the active pane's inner payload
            // (`apply_active_pane_output` / `flush_coalesced_output`), which
            // fires `NativeCallbacks::on_osc` / OSC 133 capture exactly like
            // any other content, so an inner OSC 777 Set or OSC 133 D/A pair
            // lands in the same queues plain shell output would. Clearing
            // here, BEFORE the tail re-route below gets its own turn, means
            // the tail re-route's `process_outer_via_core` call (which
            // internally drains + reconciles `pending_latch_feed`) only ever
            // sees candidates from the bytes it itself just parsed — never
            // leftovers from the mux-inner portion of this same pump.
            // Gated on whether THIS PUMP started mux-attached, not the
            // current (possibly now-detached) state, so a same-pump
            // mux→detach transition cannot let a mux pane's inner OSC 777
            // Set / OSC 133 marks leak into the GUI-local plain-tab
            // agent-status model or inferred-clear latch (SPEC FR3: the
            // daemon is authoritative for mux panes; only its
            // `AgentStatusUpdate` messages may populate mux-pane status).
            if was_mux_at_pump_start {
                let mut s = self.cb_state.lock();
                s.pending_agent_status.clear();
                s.pending_latch_feed.clear();
            }
            // FR5: re-route the post-`Detached` tail through `self.core` in this
            // same pump. The `Detached` arm already cleared the grid via
            // `reset_frame_for_replay(b"")` and reset the extractor; the shell
            // (which now owns the PTY again) printed its prompt right behind the
            // `Detached` frame, and those bytes are still in `combined`. Without
            // this they would be dropped by the extractor and the screen would
            // stay blank until the next keystroke produced fresh PTS bytes.
            if let Some(tail) = detach_tail_start {
                if tail < combined.len() {
                    self.process_outer_via_core(&combined[tail..]);
                    changed = true;
                }
            }
            // Plain-tab `agent-status` OSC events parsed by
            // `NativeCallbacks::on_osc` this pump (task0005 AC-1/AC-3/AC-4),
            // sourced only from a pre-mux outer parse (top of this function)
            // or — when this pump carried a same-pump mux detach — the tail
            // re-route just above. Never from mux inner-content parsing,
            // which was discarded above. Drained once, here, after both
            // possible producers for this pump have already run.
            // `App::pump_all` (which owns `App::agent_status`) drains this
            // via `Tab::take_pending_agent_status_events` after the per-tab
            // loop's `&mut self.tabs` borrow ends.
            let agent_status_events: Vec<crate::agent_status::AgentStatusEvent> =
                std::mem::take(&mut self.cb_state.lock().pending_agent_status);
            if !agent_status_events.is_empty() {
                self.pending_agent_status_events.extend(agent_status_events);
                changed = true;
            }
            // Inner content applied by `apply_mux_message` (the `PtyOutput`
            // arm feeding `self.core`) fires `on_apc` / `on_dcs` for any inner
            // Kitty / SIXEL image — those land in `cb_state.pending_apc` /
            // `pending_dcs` only AFTER the loop above ran. Drain and decode
            // them now so an inner mux image is not deferred a frame (or, when
            // the next pump has no PTS bytes, never decoded). Inner content is
            // image-only here (mux protocol frames never re-enter `self.core`),
            // so this drain feeds the image pipeline directly.
            let (inner_apc, inner_dcs) = {
                let mut s = self.cb_state.lock();
                (
                    std::mem::take(&mut s.pending_apc),
                    std::mem::take(&mut s.pending_dcs),
                )
            };
            if (!inner_apc.is_empty() || !inner_dcs.is_empty())
                && self.drain_and_decode_images(&inner_apc, &inner_dcs)
            {
                changed = true;
            }
        }

        changed
    }
}
