//! Rendering preparation: wgpu surface lifecycle (reconfigure / acquire /
//! recreate), the per-frame `render` pass, and egui raw-input assembly.

use std::time::Instant;

use egui::ViewportId;
use egui_wgpu::ScreenDescriptor;
use egui_wgpu::wgpu::SurfaceError;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::app::App;
use crate::mux::dialog::{MuxDialogOutcome, MuxDialogState};

use super::WindowHost;
use super::frame_pacing::{
    has_actionable_egui_input, preedit_effective_dirty_rows, record_drawn_frame,
    record_rebuilt_rows, resolve_build_dirty_rows, should_rotate_row_cache_for_scroll_event,
    should_skip_frame,
};
use super::input_translate::input_mods_to_egui;

impl WindowHost {
    /// Configure the wgpu surface to `width` × `height` (physical pixels).
    /// The ONLY `surface.configure` call site: bundling the
    /// `surface_config` update, the configure, and the `surface_dirty`
    /// reset keeps the "a configure clears `surface_dirty`" invariant
    /// inside this method instead of relying on every caller (and every
    /// future caller) to restore it. Dimensions are clamped to at least
    /// 1 here for the same reason: wgpu panics on a zero-sized configure,
    /// so the never-zero invariant lives with the configure instead of
    /// in every caller.
    pub(super) fn configure_surface_to(&mut self, width: u32, height: u32) {
        self.surface_config.width = width.max(1);
        self.surface_config.height = height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.surface_dirty = false;
    }

    /// Reconfigure the wgpu surface for the current window size.
    fn reconfigure_surface(&mut self) {
        let size = self.window.surface_size();
        self.configure_surface_to(size.width, size.height);
    }

    /// Acquire the next swapchain texture, transparently recovering from
    /// `suboptimal` results by reconfiguring once and re-acquiring before
    /// returning. Without this, every frame whose swapchain is suboptimal
    /// would trigger a `wgpu_hal` "Suboptimal present of frame N" warn at
    /// `present()` — the swapchain stays in that state until a resize
    /// event happens to land, so the warn loops on every frame in between.
    ///
    /// Returns `None` when the texture is unrecoverable for this frame
    /// (`Lost` / `Outdated` / `OutOfMemory` / `Timeout`); the caller must
    /// return without rendering — `surface_dirty` is already flagged so
    /// the next frame reconfigures.
    fn acquire_surface_texture(&mut self) -> Option<wgpu::SurfaceTexture> {
        let mut tries: u8 = 0;
        loop {
            match self.surface.get_current_texture() {
                Ok(tex) if tex.suboptimal && tries == 0 => {
                    // Drop the suboptimal texture and reconfigure to the
                    // current physical size. Cap at one retry so a
                    // compositor that keeps reporting suboptimal cannot
                    // spin us in a tight acquire loop.
                    drop(tex);
                    log::debug!("wgpu surface suboptimal; reconfiguring before retry");
                    self.reconfigure_surface();
                    tries += 1;
                    continue;
                }
                Ok(tex) => return Some(tex),
                Err(SurfaceError::Lost) | Err(SurfaceError::Outdated) => {
                    // Mark dirty so the next frame reconfigures before
                    // acquire, and request a redraw so the event loop
                    // schedules one.
                    log::warn!("wgpu surface Lost/Outdated; will reconfigure next frame");
                    self.surface_dirty = true;
                    self.window.request_redraw();
                    return None;
                }
                Err(SurfaceError::OutOfMemory) => {
                    log::error!("wgpu surface out of memory; will recreate next frame");
                    self.surface_dirty = true;
                    self.window.request_redraw();
                    return None;
                }
                Err(SurfaceError::Timeout) => {
                    log::warn!("wgpu surface timeout; skipping frame");
                    return None;
                }
            }
        }
    }

    /// Fully recreate the surface from the underlying instance. Reserved
    /// for severe failure modes (e.g. `OutOfMemory`) where simply
    /// reconfiguring the current handle is insufficient. The Lost/Outdated
    /// recovery path uses [`reconfigure_surface`] driven by `surface_dirty`.
    #[allow(dead_code)]
    fn recreate_surface(&mut self) {
        log::warn!("recreating wgpu surface after device/surface loss");
        let new_surface: wgpu::Surface<'static> = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: self
                        .window
                        .display_handle()
                        .expect("display handle")
                        .as_raw(),
                    raw_window_handle: self.window.window_handle().expect("window handle").as_raw(),
                })
                .expect("recreate surface")
        };
        self.surface = new_surface;
        self.reconfigure_surface();
    }

    /// Run a single egui frame and present.
    pub fn render(&mut self, app: &mut App) {
        // task0005 AC-1: consume the hover-span-changed latch (set by
        // `refresh_link_hover` / `invalidate_link_hover` since the last
        // frame) before the dirty-row skip decision below. Forcing a full
        // redraw here makes `dirty_rows_this_frame` return every row this
        // turn, so the hover underline's rows actually rebuild in the row
        // cache instead of a stale cached row surviving the skip.
        if std::mem::take(&mut self.hover_span_changed) {
            app.mark_full_redraw();
        }

        // Refresh the cached status-bar insets first: the deferred-
        // resize path below reads them to compute the PTY grid size,
        // and the grid-pass origin in this same frame also reads them.
        // A change here flips `pending_resize` so the PTY is reshaped
        // in step with the panel growing / shrinking (e.g. when the
        // mux session attaches and the OSC row pops in).
        self.refresh_status_bar_insets(app);
        // Same coalescing for the persistent mux sidebar's grid inset
        // (task0005 D2 / NFR1): entering/leaving a mux-attached tab in
        // persistent mode, or flipping `mux.window_sidebar_overlay`, are
        // the only inputs that move this value, so those are the only
        // cases that reshape the PTY — overlay open/close and plain
        // window/tab switching leave it untouched.
        self.refresh_mux_sidebar_inset(app);

        // Alacritty-style deferred resize: apply pending window-size changes
        // here, not inside the winit `Resized` handler. This coalesces
        // bursts of compositor resize events into a single configure +
        // PTY-resize per frame, and keeps `surface.configure()` paired with
        // the swapchain acquire that follows so Wayland / X11 don't lock
        // the back buffer between the two calls. Done before `surface_dirty`
        // so a pending resize subsumes any prior Lost/Outdated reconfigure —
        // `configure_surface_to` clears `surface_dirty` as part of the
        // configure itself (a zero-sized resize configures nothing and so
        // leaves any pending recovery armed).
        let had_pending_resize = self.pending_resize;
        self.apply_pending_resize(app);
        if had_pending_resize {
            // Resize changes the swapchain extent; everything must repaint.
            app.mark_full_redraw();
        }

        // task0003 rework (D2, activation-reconcile request/execute split):
        // consume any pending activation-reconcile request now that the
        // status-bar insets, the mux-sidebar inset, and any pending
        // display-area resize have all settled `App::cell_size` for the
        // active tab this frame. This is the ONLY call point — running it
        // any earlier (or inside the activation path itself) would compare
        // against dims still describing the OUTGOING tab.
        app.execute_pending_reconcile();

        // Phase 0: lazy first-frame configure + recovery from Lost/Outdated.
        // `surface_dirty` is true on construction (deferred configure) and
        // whenever a previous frame returned `Lost` / `Outdated`. We
        // reconfigure with the current physical size before acquiring the
        // next swapchain texture.
        let was_surface_dirty = self.surface_dirty || had_pending_resize;
        if self.surface_dirty {
            self.reconfigure_surface();
            // Reconfiguring the swapchain produces a fresh-but-uninitialized
            // surface texture; the next present needs a full paint.
            app.mark_full_redraw();
        }

        // task0002 D1: resolve the overlay card's opacity ONCE per call,
        // unconditionally (even on a turn that ends up skipped below) —
        // `App::resolve_mux_sidebar_opacity` only arms/resets a fade-
        // bookkeeping instant, which is cheap and idempotent whether or
        // not this turn goes on to actually paint. `render::draw_terminal`
        // only holds `&App` (the draw layer stays a pure consumer), so
        // the value is threaded down as a parameter below. `animating`
        // feeds the `overlay_work` expression a few lines down so an
        // in-flight fade is never dropped by the clean-grid skip gate
        // (FR8 / AC-9).
        //
        // Gated on the overlay actually being shown: SPEC.md's edge case
        // "sidebar closed — no fade state advances" — a hidden/persistent
        // sidebar's dim state (if any, from an earlier overlay session)
        // stays frozen rather than continuing to tick while nothing reads
        // it; `draw_persistent` ignores the value regardless, so `1.0` /
        // not-animating is a harmless default for every other case.
        let (mux_sidebar_opacity, mux_sidebar_animating) =
            if app.mux_sidebar_visibility() == crate::app::MuxSidebarVisibility::Overlay {
                app.resolve_mux_sidebar_opacity(Instant::now())
            } else {
                (1.0, false)
            };

        // Sub-phase 2 dirty-row diff: skip the entire egui+wgpu cycle when
        // nothing in the active tab needs to repaint. The first frame
        // (or any frame that follows a surface reconfigure) bypasses this
        // skip because `App::mark_full_redraw()` forces the dirty set to
        // the full row range.
        //
        // The status-bar carve-out: provider-owned wake chains
        // (`TimeProvider` timer thread, `GitBranchProvider` worker,
        // OSC 777 push) drive `EventLoopProxy::send_event(())` ->
        // `user_event` -> `request_redraw`, but on an idle shell the
        // resulting `render()` would observe `dirty_rows_this_frame()
        // == 0` and short-circuit here, freezing the `{time}` display.
        // `App::status_bar_view_model_changed` does the comparison
        // against the previous frame's view model so the skip path
        // only triggers when neither the terminal nor the status bar
        // moved.
        // task0003 D2: `dirty` is captured into `frame_dirty_rows` so the
        // per-row instance cache rebuild below drives off the exact same
        // row set the skip decision used, instead of recomputing (and
        // potentially observing a different result if `app.fold_layout()`
        // changes between here and there — see the design note at that
        // call site). `None` when this block never ran (`was_surface_dirty`
        // — a forced full redraw) or there is no active tab.
        let mut frame_dirty_rows: Option<Vec<u16>> = None;
        if !was_surface_dirty {
            let dirty_count = if let Some(tab) = app.active_tab() {
                let core = tab.core.lock();
                let dirty = app.dirty_rows_this_frame(&core);
                let rows = core.rows();
                log::debug!(
                    "native-poc: dirty rows this frame = {} / {}",
                    dirty.len(),
                    rows
                );
                let count = dirty.len();
                frame_dirty_rows = Some(dirty);
                Some(count)
            } else {
                // No tab: still render once to draw the hint message; rely
                // on the `needs_full_redraw` flag bookkeeping for that.
                None
            };
            let status_bar_changed = app.status_bar_view_model_changed();
            // Overlay work in flight (a restart/SFTP toast counting down to
            // auto-dismiss, a visual-bell flash still decaying, the search
            // UI being open, or the one-shot bell-erase-frame signal) needs
            // the egui pass to run every frame just like the status bar
            // carve-out above — otherwise the 60 Hz wake this scheduled in
            // `about_to_wait` (see `toast_pending` / `bell_due` there) spins
            // uselessly, discarding every frame here before `pump_sftp` /
            // the toast prune / the bell-flash paint / the search overlay
            // ever run.
            //
            // task0005 AC-2: `app.search_visible()` keeps every frame live
            // while the search UI is open (interactive query editing,
            // auto-research match movement) and is `false` — so it
            // contributes nothing — once the overlay is closed, preserving
            // idle skipping.
            //
            // task0005 AC-4: `app.take_bell_erase_pending()` consumes the
            // one-shot signal `App::needs_bell_repaint` latches in
            // `about_to_wait` when the flash crosses its expiry — by the
            // time this frame runs, `visual_bell_progress()` already reads
            // `None`, so without this the final erase frame would look
            // identical to a fully idle frame and get skipped, freezing the
            // flash at its last painted alpha instead of fading it out.
            // Consumed unconditionally (not folded into the `||` chain
            // below) so the one-shot latch is always drained exactly once
            // per frame regardless of short-circuit evaluation — otherwise
            // an active toast/bell-progress condition earlier in the chain
            // would skip evaluating this call via `||` short-circuiting,
            // leaving the latch to fire a frame later than the actual
            // expiry.
            let bell_erase_pending = app.take_bell_erase_pending();
            // task0002 AC-9 / FR8: `mux_sidebar_animating` (already resolved
            // above, before the skip decision) keeps an in-flight bright-
            // to-dim fade from being dropped on an otherwise clean grid.
            // `mux_sidebar_hover_changed` covers the OTHER transition this
            // feature needs — hover entering the card brightens it
            // IMMEDIATELY (D4: no interpolation, so `animating` alone never
            // sees it) — bare `PointerMoved` is deliberately excluded from
            // `egui_input_pending` below, so without this the card would
            // never actually paint its brightened state on hover-enter.
            let overlay_work = app.restart_toast.active()
                || !app.sftp_ui.toasts.toasts.is_empty()
                || app.visual_bell_progress().is_some()
                || app.search_visible()
                || bell_erase_pending
                || mux_sidebar_animating
                || app.mux_sidebar_hover_changed();
            // Undrained *actionable* egui input (a click, wheel scroll, key,
            // text, or clipboard event) vetoes the skip: `build_raw_input`
            // below is the only drain, so a skipped frame would park it
            // until the next unrelated wakeup (worst case a blink flip,
            // ~530 ms). `PointerMoved` alone is excluded from this veto:
            // `PointerMoved` pushes one unconditionally on every mouse
            // motion, so without the exclusion an idle terminal would run a
            // full egui+GPU frame on every hover pixel. A click always
            // arrives as `[PointerMoved, PointerButton]`, so the trailing
            // `PointerButton` still vetoes the skip and click latency is
            // unaffected — only chrome hover feedback (e.g. a tab-bar
            // highlight) is deferred until the next discrete event or a
            // content change, which is an intentional trade-off.
            // While a pointer button is held, motion IS actionable: egui
            // chrome drags (scrollbar thumb, tab reorder) are driven by
            // the press→release motion stream and must keep their live
            // tracking even over an idle grid.
            let egui_input_pending =
                has_actionable_egui_input(&self.pending_egui_events, self.pointer_buttons_down > 0);
            if should_skip_frame(
                dirty_count,
                status_bar_changed,
                overlay_work,
                egui_input_pending,
            ) {
                return;
            }
        }

        // task0002 FR6-half: count frames that proceed past the skip
        // decision above (i.e. frames that are actually drawn), gated
        // behind `EMTERM_RENDER_PERF=1` so there is zero overhead when
        // unset. Logged at warn level (release builds drop below warn)
        // with the `[EMTERM_RENDER_PERF]` prefix, same idiom as
        // `EMTERM_FONT_PERF`.
        //
        // task0005 AC-6: the cached `render_perf_enabled` flag gates this
        // whole block, including the `Instant::now()` timestamp — with the
        // gate off, no timestamp is acquired and `record_drawn_frame`'s
        // `enabled` branch is never reached at all (as opposed to reading
        // `Instant::now()` unconditionally and only checking the gate
        // inside the helper).
        if self.render_perf_enabled {
            if let Some(total) = record_drawn_frame(true, &mut self.frame_counter, Instant::now()) {
                log::warn!("[EMTERM_RENDER_PERF] frames drawn: {total}");
            }
        }

        // Build this frame's fold layout for the active tab before any paint
        // pass. `collect_cell_inputs` (cell rows), `draw_fold_summaries`
        // (summary overlays), and `draw_search_highlights` (fold-aware match
        // mapping) all read `App::fold_layout()`, which needs `&mut self`
        // here once so those passes can borrow `&App` for the rest of the
        // frame. No-op (clears to `None`) when no region is collapsed.
        app.refresh_fold_layout();

        let raw_input = self.build_raw_input();
        let mut frame_events = crate::render::FrameEvents {
            title: None,
            tab: None,
            scroll_to: None,
            search: None,
            profile: None,
            sftp: None,
        };
        // Snapshot the current maximized state so the title bar can
        // swap its middle glyph between Maximize and Restore. Reading
        // the window here (instead of inside `draw_placeholder`)
        // keeps the render module free of winit dependencies.
        let window_maximized = self.window.is_maximized();
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            frame_events =
                crate::render::draw_placeholder(ctx, app, window_maximized, mux_sidebar_opacity);
            // Search overlay is drawn after the chrome so it floats above
            // the tab bar / status bar; it needs `&mut App` for its
            // TextEdit, so it runs as a separate call from `draw_terminal`
            // (which holds `&App`).
            frame_events.search = crate::render::draw_search_overlay(ctx, app);
            // Profile selector modal floats above everything (scrim +
            // dialog); same `&mut App` split as the search overlay.
            frame_events.profile = crate::render::draw_profile_selector_overlay(ctx, app);
            // SFTP: drain progress + duplicate-check channels using the egui
            // frame time (monotonic, wall-clock-free) so terminal toasts can
            // schedule their auto-dismiss, then draw the overlay/dialogs/toasts.
            // The binary-mismatch restart toast is pumped as its own call so
            // its auto-dismiss never depends on the SFTP pump staying
            // unconditional.
            let now = ctx.input(|i| i.time);
            let mut toasts_changed = app.pump_restart_toast(now);
            toasts_changed |= app.pump_sftp(now);
            if toasts_changed {
                ctx.request_repaint();
            }
            frame_events.sftp = crate::render::draw_sftp_overlay(ctx, app);
            // mux rename / move modals (same `&mut App` split). Drawn last so
            // they float above the other chrome.
            if drive_mux_dialogs(app, ctx) {
                ctx.request_repaint();
            }
        });
        // FR4: clear the one-shot scroll-into-view signal now that the egui
        // pass (which read it into the tab strip) has run. Clearing every
        // frame gives it exactly one-frame lifetime, so it never re-fires on
        // an unrelated repaint (e.g. after a mouse-driven horizontal scroll).
        app.clear_scroll_active_tab_into_view();
        // Captured before the handlers below consume the `frame_events`
        // options: whether ANY event fired this frame. Gates the second
        // `refresh_fold_layout()` after the block — on the ordinary frame
        // (no events) the layout built before the egui pass is still
        // valid, and re-building it would double the per-frame fold cost
        // while a region is collapsed. The field enumeration lives on
        // `FrameEvents::any` (next to the struct) so a future event field
        // can't silently fall out of this gate.
        let frame_events_applied = frame_events.any();
        // CSD title-bar actions hit `winit::Window` directly except
        // for Close, which defers to `about_to_wait` via
        // `pending_close` so teardown follows the same handshake as
        // the last-tab-closed path.
        if let Some(evt) = frame_events.title {
            self.apply_title_bar_event(evt);
        }
        // Apply any tab bar interaction emitted this frame. Closing
        // the last tab returns `true` and the next event loop tick
        // observes `app.tabs.is_empty()` to exit the window.
        if let Some(evt) = frame_events.tab {
            let _ = app.apply_tab_event(evt);
            // Tab roster changed; force a full redraw next frame.
            app.mark_full_redraw();
            // Active tab changed: grid content under the pointer is now
            // different. Drop the cached hover so the stale underline /
            // hand cursor from the previous tab doesn't bleed through.
            self.invalidate_link_hover();
            // The event was applied *after* this frame's egui pass, so the
            // chrome just painted reflects the pre-event state. Schedule a
            // follow-up frame immediately — without this the new state
            // (e.g. the settings tab opening) only appears on the next OS
            // input event. The wake covers Wayland, where a redraw
            // requested from inside RedrawRequested can be folded into
            // the in-flight frame (see the repaint_delay comment below).
            self.window.request_redraw();
            crate::wakeup::wake();
        }
        // Scrollbar thumb moved: jump the viewport. `scroll_set_offset`
        // marks the frame dirty itself, so the new position paints on
        // the next redraw (already requested by the pointer event that
        // produced this interaction).
        if let Some(offset) = frame_events.scroll_to {
            app.scroll_set_offset(offset);
            // Viewport shifted under the pointer; cached hover is stale.
            self.invalidate_link_hover();
        }
        // Search-bar interaction: re-run the search on query / option
        // changes (incremental), navigate on the prev / next buttons, or
        // close the overlay. `App` owns the search state + the scroll-to-
        // match side effect.
        if let Some(evt) = frame_events.search {
            use crate::ui::search_bar::SearchBarEvent;
            match evt {
                SearchBarEvent::QueryChanged(_) | SearchBarEvent::OptionsChanged => {
                    app.run_search();
                }
                // Match navigation can scroll the viewport (and expand
                // folds), so the cached hover spans index the pre-jump
                // viewport — same invalidation the scrollbar-jump handler
                // above does.
                SearchBarEvent::Next => {
                    app.search_next();
                    self.invalidate_link_hover();
                }
                SearchBarEvent::Prev => {
                    app.search_prev();
                    self.invalidate_link_hover();
                }
                SearchBarEvent::Close => app.close_search(),
            }
        }
        // Profile-selector pointer interaction: a row click spawns the
        // tab (modal closes inside `confirm_profile_selection`); a scrim
        // click dismisses. Applied post-pass like the tab events, with
        // the same immediate-repaint kick so the new state paints without
        // waiting for the next OS input event.
        if let Some(evt) = frame_events.profile {
            use crate::ui::profile_selector::ProfileSelectorEvent;
            match evt {
                ProfileSelectorEvent::Confirm(idx) => app.confirm_profile_selection(idx),
                ProfileSelectorEvent::Cancel => app.profile_selector.close(),
            }
            app.mark_full_redraw();
            self.window.request_redraw();
            crate::wakeup::wake();
        }
        // SFTP overlay interaction: confirm/cancel the upload or overwrite
        // dialog, or cancel a running upload. Applied post-frame like the
        // other overlays, with the same immediate-repaint kick.
        if let Some(evt) = frame_events.sftp {
            use crate::render::SftpFrameEvent;
            // Frame time for any error toast surfaced by the confirm paths
            // (monotonic, wall-clock-free; same source as `pump_sftp`).
            let now = self.egui_ctx.input(|i| i.time);
            match evt {
                SftpFrameEvent::ConfirmUpload => app.confirm_upload_dialog(now),
                SftpFrameEvent::CancelUpload => {
                    app.sftp_ui.upload_dialog = None;
                }
                SftpFrameEvent::ConfirmOverwrite => app.confirm_overwrite_dialog(now),
                SftpFrameEvent::CancelOverwrite => {
                    app.sftp_ui.overwrite_dialog = None;
                }
                SftpFrameEvent::CancelSession(id) => app.cancel_sftp_upload(&id),
                SftpFrameEvent::ConfirmClose => {
                    // Cancel the guarded tab's uploads and close it. Emptying
                    // the roster is handled by `about_to_wait`'s
                    // `tabs.is_empty()` teardown check on the next turn.
                    let _ = app.confirm_close_guard();
                    app.mark_full_redraw();
                    self.invalidate_link_hover();
                }
                SftpFrameEvent::CancelClose => app.cancel_close_guard(),
            }
            self.window.request_redraw();
            crate::wakeup::wake();
        }
        // Re-derive the fold layout now that this frame's tab switch /
        // scrollbar jump have been applied above. The first
        // `refresh_fold_layout()` call (before the egui pass) fed the
        // chrome that was just painted; the grid build below still reads
        // `App::fold_layout()`, so it needs the layout recomputed against
        // the post-event active tab / scroll offset. Gated on an event
        // actually having fired: on the ordinary no-event frame the
        // pre-egui layout is still valid, and an unconditional re-build
        // would double the per-frame fold cost while a region is
        // collapsed.
        if frame_events_applied {
            app.refresh_fold_layout();
        }
        // egui requested an immediate repaint (a popup opening, a widget
        // state transition, a one-frame animation step). Without honoring
        // this the next frame only renders on the next OS input event, so
        // e.g. a ComboBox click appears to need a second click before its
        // popup shows, and a tab-event state change (settings tab opening)
        // presents a half-applied frame. Mirrors `viewer::shell`'s
        // repaint_delay handling.
        //
        // `wakeup::wake()` in addition to `request_redraw()`: we are
        // *inside* the RedrawRequested handler here, and winit's Wayland
        // backend can fold a redraw requested mid-redraw into the frame
        // currently being presented (X11 reliably schedules a fresh one).
        // The proxy-driven wake lands as a `user_event` on the next loop
        // cycle whose own `request_redraw()` is unambiguously "new" —
        // the same bridge the status-bar clock relies on.
        if full_output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|v| v.repaint_delay.is_zero())
        {
            self.window.request_redraw();
            crate::wakeup::wake();
        }
        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, self.pixels_per_point);
        let textures_delta = full_output.textures_delta;

        let surface_texture = match self.acquire_surface_texture() {
            Some(tex) => tex,
            None => return,
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("native-poc-encoder"),
            });

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: self.pixels_per_point,
        };

        for (id, image_delta) in &textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }
        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        // Phase 4-H (FR12): build the per-cell input list against the
        // active tab's core + selection + theme, then hand it to
        // `TerminalGridPass::prepare`. The pass clears the swapchain to
        // the theme background and emits one instanced draw call for
        // every visible cell (background + glyph + decorations). egui
        // runs second with `LoadOp::Load` and draws the UI overlay
        // (tab bar / status bar / cursor / IME preedit) on top.
        //
        // Origin/metrics captured before `grid_pass.as_mut()` so the
        // mutable borrow doesn't conflict with `&self` on the metrics
        // call inside the branch (both go through `cell_metrics_px`).
        let (_, _, origin_x_px, origin_y_px) = self.cell_metrics_px(app);
        // Borrow the hovered-link cell spans before the `grid_pass`
        // mutable borrow so `collect_cell_inputs` can underline them
        // without re-borrowing `self`. Using a slice reference avoids
        // a per-frame heap allocation while `self.hover.link_cells` and
        // `self.grid_pass` are disjoint fields.
        let hover_link_cells: Option<&[(u16, u16, u16)]> = if self.hover.link_cells.is_empty() {
            None
        } else {
            Some(&self.hover.link_cells)
        };
        if let Some(pass) = self.grid_pass.as_mut() {
            // Theme is seeded from settings (font_size_pt + cursor
            // style) and then overlaid with the active tab's OSC
            // mutations when a tab is present, mirroring the layering
            // `render::draw_terminal` uses for the egui overlay. The
            // no-tab fallback overrides `font_size_pt` with the live
            // zoom level so the grid pass agrees with `cell_w_logical` /
            // `cell_h_logical` (also re-derived from the runtime size).
            let theme = match app.active_tab() {
                Some(tab) => tab.theme.lock().clone(),
                None => {
                    let mut t = crate::render::theme::Theme::from_settings(app.settings.as_ref());
                    t.font_size_pt = app.runtime_font_size_pt;
                    t
                }
            };
            let width_mode = app.settings.ambiguous_width_mode;
            // Cell metrics come from `App::cell_w_logical` /
            // `App::cell_h_logical` so the wgpu-rendered cells line up
            // with the egui-side cursor and preedit overlays. The
            // vertical origin reserves the same logical-px the tab bar
            // widget actually occupies (see
            // `crate::ui::tab_bar::TAB_BAR_HEIGHT`) plus the
            // `settings.padding` strip applied inside `cell_metrics_px`.
            //
            // HiDPI: the swapchain is sized in physical pixels while
            // `cell_w_logical` / `cell_h_logical` / `padding` are logical
            // pixels. egui scales its pass via `pixels_per_point` in
            // the `ScreenDescriptor`; we apply the same scale to every
            // length we hand wgpu (cell rect + origin + glyph
            // rasterize size) so cells line up with the egui-side
            // cursor / preedit on 2.0× hosts. Computed before the cell
            // collection below (task0003): the per-row cache rebuild
            // needs the same metrics the upload step uses.
            //
            // Origin already captured above and lines up with
            // `cell_metrics_px` so the status-bar top inset (when
            // configured) shifts cells down to sit below the panel —
            // otherwise the top row would paint behind the egui
            // status-bar.
            let scale = self.pixels_per_point.max(1.0);
            let metrics = crate::render::terminal_grid_pass::CellMetrics {
                cell_w: app.cell_w_logical * scale,
                cell_h: app.cell_h_logical * scale,
                origin: [origin_x_px as f32, origin_y_px as f32],
                // `theme.font_size_pt` is in CSS-compatible points;
                // the rasterizer takes pixels, so apply the same
                // `pt → px` conversion the legacy WebView build
                // does (96/72). Without this the glyph atlas is
                // built at ~75% of the cell size.
                font_size_px: theme.font_size_px() * scale,
            };

            // task0003: resolve this frame's instance list either through
            // the per-row cache (normal path) or a full uncached rebuild
            // (IME preedit bypass — see the comment below), then hand it
            // to `prepare` for GPU upload.
            let (instances, rows_rebuilt) = if let Some(tab) = app.active_tab() {
                let mut core = tab.core.lock();
                let row_count = core.rows();
                // The egui pass above may have applied events that
                // invalidate the frame-top dirty snapshot — a tab-bar
                // click switched `app.active_tab()` (this `core` is not
                // the one the snapshot was computed against), a scrollbar
                // jump moved the viewport. Every such path raises
                // `mark_full_redraw`, so widen the snapshot to every row
                // when the flag is pending at build time; otherwise the
                // per-row cache would keep serving the previous tab's
                // rows with only the new tab's dirty rows patched in.
                frame_dirty_rows =
                    resolve_build_dirty_rows(frame_dirty_rows.take(), app.full_redraw_pending());
                // task0006 (fixes review round-2 critical finding
                // 779c9130c103c55b): consume term_core's accumulated
                // scroll event exactly once per rendered frame, before
                // either branch below decides which rows the per-row
                // cache should serve from cache vs rebuild — the core's
                // full-screen count==1 scroll optimization
                // (`ring_buffer::scroll_up_internal`) shifts its own
                // dirty bits + viewport mapping on the promise that the
                // renderer shifts its own representation (`row_cache`,
                // owned by `pass`) by the same amount; without this the
                // cache kept serving stale upper rows after every
                // ordinary live-tail scroll.
                //
                // `frame_dirty_rows` names fewer than every row only on
                // the ordinary cached path — a forced full redraw
                // (`was_surface_dirty`, `needs_full_redraw`/
                // `force_full_redraw`, a fold layout, or a scrolled-back
                // viewport reacting to new output — see
                // `App::on_pty_output`) already names every row via
                // `dirty_rows_this_frame`, so the rebuild below overwrites
                // the whole cache regardless of any rotation; skip
                // rotating in that case and only clear the now-stale
                // event (task0006 Design: "needs_full_redraw frames: full
                // rebuild already; just clear the event"). Otherwise (the
                // ordinary cached path, and the IME preedit shadow-rebuild
                // path below which shares the same `row_cache`) rotate
                // first so both branches read a cache already tracking
                // the shift.
                let scroll_count = core.get_scroll_event_count();
                if scroll_count > 0 {
                    let partial_dirty_rows = frame_dirty_rows
                        .as_ref()
                        .is_some_and(|rows| (rows.len() as u16) < row_count);
                    if should_rotate_row_cache_for_scroll_event(scroll_count, partial_dirty_rows) {
                        pass.apply_scroll_event(
                            core.get_scroll_event_direction(),
                            scroll_count,
                            metrics.cell_h,
                        );
                    }
                    core.clear_scroll_event();
                }
                // The filled block cursor is painted by the egui overlay
                // (`render::cursor::draw_block_cursor`), not baked into
                // the grid — grid instance data never depends on cursor
                // position, blink phase, or window focus. Suppression
                // (scrolled back into history, hidden by a fold) and the
                // focused/blink/style visibility gate live on the
                // overlay side now.
                let scroll_offset = app.scroll_offset();
                if tab.preedit_state.active() {
                    // IME preedit overlay (Phase 4-G): paint composition
                    // glyphs inline at the anchor so the user can see
                    // what they are typing. `apply_preedit_overlay`
                    // mutates `CellInput`s *after* `collect_cell_inputs`
                    // in a way the per-row cache must never observe
                    // (baking the transient composition glyphs into
                    // `row_cache` would leak them into every subsequent
                    // cache-served frame after preedit ends) — the
                    // frame actually painted below still comes from a
                    // full, uncached `build_instances` call.
                    //
                    // That used to mean skipping `row_cache` entirely
                    // for the whole preedit-active stretch (task0003
                    // design note under D3), but `record_render_state`
                    // unconditionally calls `core.clear_dirty()` at the
                    // end of every frame regardless of this branch —
                    // so any row changed by async PTY output or a
                    // resize *while* composing was marked clean without
                    // the cache ever learning about it, leaving stale
                    // or (post-resize) `None`/blank rows once the cache
                    // path resumed after preedit closed. Fix: still
                    // feed the same dirty-row set through
                    // `rebuild_and_collect` here (a "shadow" rebuild —
                    // its returned instances are discarded, only its
                    // cache-side effect matters) using the *clean*
                    // (pre-overlay) cells, so `row_cache` keeps tracking
                    // `term_core` continuously even during preedit.
                    let anchor = tab.preedit_state.anchor();
                    let effective_dirty_rows = preedit_effective_dirty_rows(
                        frame_dirty_rows.take(),
                        row_count,
                        anchor.row,
                    );

                    let mut inputs = crate::render::collect_cell_inputs(
                        &core,
                        &theme,
                        app.selection.as_ref(),
                        width_mode,
                        hover_link_cells,
                        scroll_offset,
                        // Fold layout (built once at the top of `render`
                        // via `App::refresh_fold_layout`). `Some` only
                        // when the active tab has a collapsed region;
                        // selects the fold-aware row mapping +
                        // summary-row cell skip.
                        app.fold_layout(),
                        None,
                    );

                    // Shadow rebuild: `inputs` still holds the clean
                    // (pre-overlay) cells here, so filtering it down to
                    // `effective_dirty_rows` — preserving the ascending
                    // row-major order `rebuild_dirty_rows` requires —
                    // gives exactly the per-row cell slices `row_cache`
                    // needs, without a second walk of `core`.
                    let dirty_cells: Vec<_> = inputs
                        .iter()
                        .filter(|c| effective_dirty_rows.contains(&c.row))
                        .cloned()
                        .collect();
                    let (_, cache_rows_rebuilt) = pass.rebuild_and_collect(
                        &effective_dirty_rows,
                        &dirty_cells,
                        metrics,
                        row_count,
                    );

                    // No bg extension: glyph is clamped inside the
                    // cell rect by `fit_glyph_to_cell` so the
                    // reverse-video bg never has to spill into the
                    // next row to cover descenders.
                    crate::render::apply_preedit_overlay(
                        &mut inputs,
                        anchor,
                        tab.preedit_state.text(),
                        &theme,
                        core.cols(),
                        core.rows(),
                        0.0,
                    );
                    (pass.build_instances(&inputs, metrics), cache_rows_rebuilt)
                } else {
                    // Reuse the row set `dirty_rows_this_frame` already
                    // computed for the skip decision above (task0003 D2):
                    // a forced full redraw (surface reconfigure, or the
                    // skip-check block never ran) rebuilds every row —
                    // matching the row cache's "resize drops everything"
                    // path; otherwise only the actual dirty rows are
                    // rebuilt and every other row is served from cache.
                    let effective_dirty_rows: Vec<u16> = match frame_dirty_rows.take() {
                        Some(rows) => rows,
                        None => (0..row_count).collect(),
                    };
                    let dirty_cells = crate::render::collect_cell_inputs(
                        &core,
                        &theme,
                        app.selection.as_ref(),
                        width_mode,
                        hover_link_cells,
                        scroll_offset,
                        app.fold_layout(),
                        Some(&effective_dirty_rows),
                    );
                    pass.rebuild_and_collect(
                        &effective_dirty_rows,
                        &dirty_cells,
                        metrics,
                        row_count,
                    )
                }
            } else {
                pass.rebuild_and_collect(&[], &[], metrics, 0)
            };

            // task0003 FR6-half: count rows rebuilt this frame (0 on a
            // fully cache-served frame), gated behind
            // `EMTERM_RENDER_PERF=1` like the frames-drawn counter above.
            //
            // task0005 AC-6: same gate-before-argument-evaluation fix as
            // `record_drawn_frame` above — `Instant::now()` is only
            // acquired inside the `render_perf_enabled` branch.
            if self.render_perf_enabled {
                if let Some(total) = record_rebuilt_rows(
                    true,
                    &mut self.rows_rebuilt_counter,
                    rows_rebuilt as u64,
                    Instant::now(),
                ) {
                    log::warn!("[EMTERM_RENDER_PERF] rows rebuilt: {total}");
                }
            }

            pass.prepare(
                &self.device,
                &self.queue,
                &instances,
                metrics,
                self.surface_config.width,
                self.surface_config.height,
            );
        }

        {
            // Clear to the active theme's bg so the padding strip around
            // the cell grid (TOP_PAD / LEFT_PAD and the right/bottom
            // remainder rows) blends into the terminal background instead
            // of showing as a visible rim around the content.
            let theme_bg = match app.active_tab() {
                Some(tab) => tab.theme.lock().bg,
                None => crate::render::theme::Theme::from_settings(app.settings.as_ref()).bg,
            };
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("native-poc-terminal-grid-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: theme_bg.0 as f64 / 255.0,
                                g: theme_bg.1 as f64 / 255.0,
                                b: theme_bg.2 as f64 / 255.0,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                })
                .forget_lifetime();
            if let Some(grid) = self.grid_pass.as_ref() {
                grid.draw(&mut pass);
            }
        }

        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("native-poc-egui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                })
                .forget_lifetime();
            self.egui_renderer
                .render(&mut pass, &paint_jobs, &screen_descriptor);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        // winit's Wayland backend asks us to call `pre_present_notify`
        // immediately before `present()` so the compositor can pace the
        // next frame; on X11/Windows it is a cheap no-op.
        self.window.pre_present_notify();
        surface_texture.present();

        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        // Sub-phase 2: snapshot cursor/selection, clear the core's dirty
        // bits, and drop the `needs_full_redraw` flag. The next frame will
        // be skipped entirely unless something dirties a row again. We
        // clone the `Arc<Mutex<TerminalCore>>` first so the immutable
        // borrow of `app` ends before the mutable `record_render_state`
        // call.
        let core_arc = app.active_tab().map(|t| t.core.clone());
        match core_arc {
            Some(arc) => {
                let mut core = arc.lock();
                app.record_render_state(&mut core);
            }
            None => app.record_render_state_no_tab(),
        }
    }

    /// Translate winit state into a minimal `egui::RawInput`. Phase 1 only
    /// needs screen-rect + pixels-per-point; later phases populate events.
    pub(super) fn build_raw_input(&mut self) -> egui::RawInput {
        let size = self.window.surface_size();
        let logical = size.to_logical::<f32>(self.pixels_per_point as f64);
        egui::RawInput {
            viewport_id: ViewportId::ROOT,
            viewports: std::iter::once((
                ViewportId::ROOT,
                egui::ViewportInfo {
                    native_pixels_per_point: Some(self.pixels_per_point),
                    ..Default::default()
                },
            ))
            .collect(),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(logical.width, logical.height),
            )),
            // Real elapsed time, NOT `None`: egui replaces `None` with
            // `previous_time + predicted_dt`, i.e. a frame counter scaled by
            // 1/60 s. The toast auto-dismiss deadlines (`App::pump_sftp`
            // reads `ctx.input(|i| i.time)`) are scheduled against this
            // clock, so it must track wall time regardless of the actual
            // frame cadence (frame-skips below 60 Hz, Mailbox-present bursts
            // above it).
            time: Some(self.egui_start.elapsed().as_secs_f64()),
            predicted_dt: 1.0 / 60.0,
            // Forward the live modifier state so egui's TextEdit (search
            // bar) interprets editing chords like Ctrl+A / Ctrl+C / Ctrl+V
            // correctly. Harmless for the chrome widgets, which ignore it.
            modifiers: input_mods_to_egui(self.current_mods),
            events: std::mem::take(&mut self.pending_egui_events),
            hovered_files: Vec::new(),
            dropped_files: Vec::new(),
            focused: true,
            max_texture_side: Some(8192),
            system_theme: None,
        }
    }
}

/// Drive one frame of the open mux dialog: render via the UI layer
/// (`ui::mux_dialogs::draw`) and dispatch the resulting outcome into the
/// domain layer (`App::confirm_mux_*`). This is the orchestration glue
/// that previously lived in `ui::mux_dialogs::drive`; moved here so the UI
/// module no longer has to `use crate::app::App` (otherwise the UI layer
/// imports App, and App imports UI types like `TabEvent` — a cycle).
/// `window_host` already owns `App`, so dispatch lives at this boundary,
/// alongside its only caller (`WindowHost::render`).
pub(super) fn drive_mux_dialogs(app: &mut App, ctx: &egui::Context) -> bool {
    if !app.mux_dialog.is_open() {
        return false;
    }
    // Reconcile against any daemon-driven changes that arrived since the
    // dialog opened (PaneCreated / PtyExited / SwitchWindow). If the
    // captured window vanished, refresh_mux_dialog flips the state to
    // Closed; we then early-return without drawing.
    app.refresh_mux_dialog();
    if !app.mux_dialog.is_open() {
        return false;
    }
    let locale = app.locale;
    let outcome = crate::ui::mux_dialogs::draw(&mut app.mux_dialog, ctx, locale);
    match outcome {
        MuxDialogOutcome::Pending => {}
        MuxDialogOutcome::ConfirmRename { window_id, name } => {
            app.mux_dialog = MuxDialogState::Closed;
            app.confirm_mux_rename(window_id, name);
        }
        MuxDialogOutcome::ConfirmMove { window_id, target } => {
            app.mux_dialog = MuxDialogState::Closed;
            app.confirm_mux_move(window_id, target);
        }
        MuxDialogOutcome::Cancelled => {
            app.mux_dialog = MuxDialogState::Closed;
        }
    }
    true
}
