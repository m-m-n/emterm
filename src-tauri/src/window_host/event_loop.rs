//! The winit application handler: PocApp, its `ApplicationHandler`
//! implementation (window creation, the window_event dispatch, and
//! about_to_wait pacing), shutdown via `Drop`, and the user-event
//! redraw hook.

use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::WindowId;

use crate::app::App;
use crate::ime::backend::{KeyDispatchResult, ProcessEnv, RawKeyEvent, build_backend_with_window};
use crate::pty::input::{Modifiers, Target as EncodeTarget};

use super::frame_pacing::{
    control_flow_for, next_resize_settle_wake_deadline, resize_settle_self_wake_due,
    toast_redraw_due,
};
use super::input_translate::{
    ShiftEnterRewrite, is_skk_swallowed_chord, shift_enter_rewrite,
    should_drop_synthetic_key_event, winit_key_to_bytes, winit_key_to_egui,
    winit_physical_key_code,
};
use super::key_routing::{
    egui_to_mux_input, handle_mux_dialog_key, handle_profile_selector_key, handle_search_key,
    handle_special_chord,
};
use super::pointer_routing::{
    handle_mouse_wheel, handle_pointer_button, handle_pointer_left, handle_pointer_moved,
};
use super::{WindowHost, terminal_font_family};

/// `ApplicationHandler` impl driving the App + WindowHost on winit 0.31.
///
/// `can_create_surfaces` creates the window the first time the platform
/// is ready to accept a render surface (the only lifecycle hook winit
/// 0.31 guarantees on desktop platforms — `resumed`/`suspended` are now
/// iOS/Web/Android-only), `window_event` mirrors what used to be the
/// inner `match event` arm, and `about_to_wait` does the periodic pump
/// (PTY drain, IME pump, cursor-rect notification) that the old
/// `StartCause::Poll` path handled.
pub(super) struct PocApp {
    pub(super) app: App,
    pub(super) host: Option<WindowHost>,
}

impl ApplicationHandler for PocApp {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.host.is_some() {
            // Back-to-back `can_create_surfaces()` calls are expected
            // per the trait's portability contract; keep the existing
            // host so a stray re-entry does not reinitialize the
            // surface (the PoC has no Android target).
            return;
        }
        let mut host = WindowHost::new(
            event_loop,
            &self.app.settings.ui_font_family,
            terminal_font_family(&self.app.settings),
        );

        // Phase 4-H: construct the TerminalGridPass against the wgpu
        // device now that the surface exists. The App owns the font
        // stack; the pass borrows clones of each `Arc`.
        host.ensure_grid_pass(&self.app);

        // Push the initial grid size into the App before the first tab spawn.
        let (cols, rows) = host.grid_size(&self.app);
        self.app.cell_size = crate::app::GridDims { cols, rows };
        self.app.spawn_initial_tab();

        // Phase 4-G-3: resolve the IME backend now that the winit
        // window exists. The factory consults `EMTERM_NATIVE_IME` and
        // `settings.ime.native_integration`, then either installs a
        // `WinitImeBridge` (real backend) or falls back to
        // `NullBackend` on init failure.
        let backend =
            build_backend_with_window(host.window_arc(), &self.app.settings.ime, &ProcessEnv);
        self.app.set_ime_backend(backend);

        host.window().request_redraw();
        // task0004 D4: the initial control flow follows the same
        // pending-timed-work rule `about_to_wait` uses below, rather than
        // unconditionally rearming a 16 ms `WaitUntil`. `ResizeSettler::new`
        // opens its settling window immediately, so this also arms the
        // resize-settle self-wake deadline from construction (mux-tab-
        // switch-bypass-refix task0002) — both places computing
        // `ControlFlow` agree on the same rule.
        event_loop.set_control_flow(control_flow_for(
            &self.app,
            next_resize_settle_wake_deadline(
                host.resize_settler.awaiting_decision(),
                Instant::now(),
            ),
        ));
        self.host = Some(host);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        let Some(host) = self.host.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                // winit's `EventLoop::run_app` returns control to the
                // caller, but PTY-owning tabs would otherwise be dropped
                // on the unwind. Tear them down explicitly so the kill
                // + reader/writer thread join from `PtySession::Drop`
                // happens before the WM destroys the window.
                log::info!("native-poc: CloseRequested → shutting down PTY tabs");
                // FR5 / NFR4: signal cancel on every in-flight 2nd-pass
                // scrollback restore worker BEFORE dropping the tabs.
                // Dropping the receiver does not fire the worker's cancel
                // flag (the worker holds an `Arc<AtomicBool>` independently
                // of the channel), so an explicit cancel store bounds
                // wasted worker CPU on shutdown. Best-effort: no join.
                for tab in self.app.tabs.iter() {
                    tab.cancel_pending_scrollback_restore();
                }
                self.app.tabs.clear();
                // Drop the wgpu Surface (and the rest of WindowHost) while
                // winit's EventLoop is still alive. The Vulkan WSI surface
                // is tied to the X11 display connection that EventLoop
                // owns; if we let WindowHost outlive the EventLoop, the
                // surface destructor calls into a freed display and
                // segfaults. Same reason applies to the egui-wgpu
                // Renderer and the Window arc.
                self.host = None;
                event_loop.exit();
            }
            WindowEvent::SurfaceResized(new_size) => {
                // Alacritty-style deferral: do not call `surface.configure()`
                // or resize the PTY here. Both run together at the head of
                // the next `render()` so a burst of compositor resize events
                // collapses to one configure + one PTY ioctl per frame.
                // Zero-size events (Windows minimize) are silently ignored
                // by `apply_pending_resize`.
                if new_size.width == 0 || new_size.height == 0 {
                    return;
                }
                host.request_resize();
                host.window().request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // pixels_per_point is consumed by `cell_metrics_px` (IME
                // spot, hit-test) which can run from `about_to_wait` between
                // events, so update it immediately. The expensive surface
                // configure + PTY grid resize stays deferred to render time
                // for the same reasons as `Resized`.
                host.pixels_per_point = scale_factor as f32;
                host.request_resize();
                host.window().request_redraw();
            }
            WindowEvent::ModifiersChanged(state) => {
                let s: ModifiersState = state.state();
                host.current_mods = Modifiers {
                    ctrl: s.contains(ModifiersState::CONTROL),
                    shift: s.contains(ModifiersState::SHIFT),
                    alt: s.contains(ModifiersState::ALT),
                };
                // Pressing / releasing Ctrl toggles the hand cursor over a
                // hovered link without any pointer motion. The cell hasn't
                // moved, so detection is reused — only the icon updates.
                host.update_link_cursor();
            }
            // Phase 4-G-3: winit surfaces composition events via
            // `WindowEvent::Ime { Enabled, Preedit, Commit, Disabled }`.
            // Route them to the active backend; `WinitImeBridge`
            // translates each variant into `ImeEvent`s consumed by
            // `App::pump_ime` on the next tick. `NullBackend`
            // overrides the trait default with a no-op, so this is
            // safe to call unconditionally.
            WindowEvent::Ime(ime) => {
                // While the profile-selector modal owns the keyboard,
                // swallow IME events entirely — the modal has no text
                // field, and a commit must not leak into the PTY.
                if self.app.profile_selector.visible {
                    host.window().request_redraw();
                    return;
                }
                // While the search bar owns the keyboard, route IME
                // commits into egui's TextEdit instead of the terminal
                // IME backend so Japanese / CJK input lands in the
                // focused field. Only `Commit` carries text we forward;
                // preedit display in the field is omitted (best-effort
                // CJK support per spec).
                if self.app.search_visible() {
                    if let winit::event::Ime::Commit(text) = &ime {
                        if !text.is_empty() {
                            host.pending_egui_events
                                .push(egui::Event::Text(text.clone()));
                        }
                    }
                    host.window().request_redraw();
                    return;
                }
                // Same capture for an open mux dialog: route CJK commits into
                // the dialog's TextEdit, never the terminal IME backend.
                if self.app.mux_dialog_open() {
                    if let winit::event::Ime::Commit(text) = &ime {
                        if !text.is_empty() {
                            host.pending_egui_events
                                .push(egui::Event::Text(text.clone()));
                        }
                    }
                    host.window().request_redraw();
                    return;
                }
                self.app.pass_winit_ime(&ime);
                host.window().request_redraw();
            }
            // Focus loss / window deactivation → clear any in-progress
            // preedit overlay so a stale composition doesn't ghost the
            // cursor after the user tabs away. Also forward focus
            // state to the IME backend so it can disable/enable IME on
            // the IM-server side.
            WindowEvent::Focused(focused) => {
                self.app.window_focused = focused;
                self.app.notify_ime_focus(focused);
                if !focused {
                    self.app.on_ime_focus_lost();
                    // ModifiersChanged is not guaranteed to fire while
                    // the window is unfocused, so a Ctrl held across an
                    // Alt+Tab would stay latched and arm the link hand
                    // cursor / Ctrl+click-open on return. Drop all
                    // modifiers on focus loss; the next real
                    // ModifiersChanged re-seeds them.
                    host.current_mods = Modifiers::default();
                    // Same staleness class for the held-button count: the
                    // matching Released may never arrive once focus is
                    // gone, and a latched count would keep treating every
                    // hover motion as an actionable drag forever.
                    host.pointer_buttons_down = 0;
                    host.update_link_cursor();
                } else {
                    // Drop the user back into the cursor's "on" half-
                    // cycle on focus regain so the filled block appears
                    // immediately instead of waiting up to 530 ms for
                    // the next blink boundary.
                    self.app.reset_blink_phase();
                }
                // Cursor shape switches between filled (focused) and
                // outline (unfocused), so we need a repaint on every
                // focus transition. The overlay cursor's filled/hollow
                // state depends on `window_focused`, which the dirty-row
                // tracking never sees, so a plain request_redraw() would
                // be skipped by should_skip_frame; force a full redraw.
                self.app.mark_full_redraw();
                host.window().request_redraw();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if event.state == ElementState::Pressed => {
                // Synthetic key press gate (task0002): drop X11 FocusIn
                // replay presses before any state mutation, keybinding
                // dispatch, IME forwarding, or PTY write. See
                // `should_drop_synthetic_key_event`.
                if should_drop_synthetic_key_event(is_synthetic) {
                    log::warn!(
                        "native-poc: dropping synthetic key press: physical_key={:?}",
                        event.physical_key
                    );
                    return;
                }
                // Search overlay capture: while the search bar is visible
                // it owns the keyboard. Navigation / close chords are
                // handled here directly; copy / paste are translated to
                // egui clipboard events; everything else is forwarded to
                // egui's TextEdit (bypassing the terminal IME dispatch and
                // the PTY encoder entirely). Returns early so the normal
                // Phase 4 key path below never runs while searching.
                // Profile-selector capture: the modal owns the keyboard
                // entirely (navigation / confirm / cancel); nothing
                // reaches the search overlay, the IME, or the PTY.
                if self.app.profile_selector.visible {
                    handle_profile_selector_key(&event, &mut self.app);
                    host.window().request_redraw();
                    return;
                }

                if self.app.search_visible() {
                    handle_search_key(&event, host.current_mods, host, &mut self.app);
                    host.window().request_redraw();
                    return;
                }

                // While a mux rename / move dialog owns the keyboard, forward
                // keys into egui (its TextEdit / DragValue / Enter / Escape)
                // and return early so the chord never reaches the terminal
                // IME, the keybind dispatcher, or the PTY encoder.
                if self.app.mux_dialog_open() {
                    handle_mux_dialog_key(&event, host.current_mods, host);
                    host.window().request_redraw();
                    return;
                }

                // Phase 4-G: offer the raw key event to the IME backend
                // first. `Consumed` means the IM server swallowed the
                // key (composition open, candidate chosen) and we must
                // skip both the keybinds dispatcher and the generic
                // encoder; the resulting `ImeEvent::Commit` / `Preedit`
                // will arrive via `pump_ime` on the next tick.
                // `Passthrough` lets the existing Phase 4 path run
                // unchanged.
                let raw_key = RawKeyEvent {
                    physical_key_code: winit_physical_key_code(&event),
                    state_pressed: true,
                    mods: host.current_mods,
                };
                if matches!(
                    self.app.dispatch_key_event_via_ime(&raw_key),
                    KeyDispatchResult::Consumed
                ) {
                    host.window().request_redraw();
                    return;
                }

                // Translate the logical key once; the result is shared by
                // `handle_special_chord` (clipboard chords) and the
                // settings-driven keybinds dispatch that follows, avoiding
                // a second translation on every keystroke.
                //
                // Special chords intercept the generic encoder path. The
                // clipboard chords are settings-driven (`keybinds.copy` /
                // `keybinds.paste`, defaults shown); the scrollback chords
                // are fixed native-poc conventions:
                //   keybinds.copy  (default Ctrl+Shift+C) → copy selection to CLIPBOARD
                //   keybinds.paste (default Ctrl+Shift+V) → paste CLIPBOARD into PTY (bracketed if 2004)
                //   Shift+PageUp   → scroll back one page
                //   Shift+PageDown → scroll forward one page
                //   Shift+Home     → scroll to top of scrollback
                //   Shift+End      → scroll back to live tail
                let egui_key = winit_key_to_egui(&event.logical_key);
                let handled =
                    handle_special_chord(&event, host.current_mods, egui_key, host, &mut self.app);
                // mux prefix latch: intercept keys for the active mux tab
                // ahead of the keybind dispatch / PTY passthrough. Only fires
                // when the active tab is mux-attached.
                let mut mux_consumed = false;
                if !handled {
                    if let Some(k) = egui_key {
                        // Convert the framework-native (egui::Modifiers, egui::Key)
                        // into the framework-agnostic mux::prefix::KeyInput right
                        // here at the UI boundary so the domain layer never sees
                        // egui types (gpt-architecture #4). `command` is folded
                        // into `ctrl` because egui aliases Cmd to Ctrl on non-mac.
                        let input = egui_to_mux_input(host.current_mods, k);
                        let (consumed, outcome) =
                            self.app.observe_mux_key(&input, std::time::Instant::now());
                        mux_consumed = consumed;
                        self.app.handle_mux_outcome(outcome);
                        if consumed {
                            self.app.mark_full_redraw();
                        }
                    }
                }
                if !handled && !mux_consumed {
                    // Settings-driven global keybinds (tab roster) take
                    // priority over the generic PTY encoder. The chord
                    // table comes from `settings.keybinds` (resolved into
                    // `App::keybinds` at startup).
                    let egui_mods = egui::Modifiers {
                        ctrl: host.current_mods.ctrl,
                        shift: host.current_mods.shift,
                        alt: host.current_mods.alt,
                        command: false,
                        mac_cmd: false,
                    };
                    let action = egui_key.and_then(|k| {
                        crate::ui::keybinds::dispatch(&self.app.keybinds, egui_mods, k)
                    });
                    if let Some(act) = action {
                        // View-level actions that need the window handle
                        // or the deferred-resize machinery are applied
                        // against `host` here; everything else routes
                        // through `App::apply_action`.
                        match act {
                            crate::ui::AppAction::ToggleFullscreen => {
                                host.toggle_fullscreen();
                                self.app.mark_full_redraw();
                            }
                            crate::ui::AppAction::ZoomIn => {
                                if self.app.zoom_in() {
                                    host.request_resize();
                                    self.app.mark_full_redraw();
                                }
                            }
                            crate::ui::AppAction::ZoomOut => {
                                if self.app.zoom_out() {
                                    host.request_resize();
                                    self.app.mark_full_redraw();
                                }
                            }
                            crate::ui::AppAction::ZoomReset => {
                                if self.app.zoom_reset() {
                                    host.request_resize();
                                    self.app.mark_full_redraw();
                                }
                            }
                            crate::ui::AppAction::OpenSearch => {
                                // Open (or re-focus) the search overlay. The
                                // overlay then captures keystrokes via the
                                // `search_visible()` branch at the top of the
                                // KeyboardInput handler on subsequent presses.
                                self.app.open_search();
                            }
                            crate::ui::AppAction::ToggleTabBar => {
                                self.app.show_tab_bar = !self.app.show_tab_bar;
                                // The tab strip's row count changed, so the
                                // grid origin / available rows shift: defer
                                // a resize so the PTY is reshaped before the
                                // next frame paints.
                                host.request_resize();
                                self.app.mark_full_redraw();
                            }
                            other => {
                                let _ = self.app.apply_action(other);
                                self.app.mark_full_redraw();
                                // Tab-switch actions (NextTab/PrevTab/JumpTab/
                                // NewTab/CloseTab) change the active grid; drop
                                // the hover so the stale underline / hand cursor
                                // from the old tab doesn't bleed into the new one.
                                host.invalidate_link_hover();
                            }
                        }
                    } else if self.app.settings.skk_mode
                        && is_skk_swallowed_chord(&event.logical_key, host.current_mods)
                    {
                        // `skk_mode` (default on): swallow bare Ctrl+J so
                        // SKK-style IMEs keep their mode-switch chord (see
                        // `is_skk_swallowed_chord`).
                    } else {
                        // `shift_enter_behavior`: three-way rewrite decision
                        // (task0001 design D1) for the bare Shift+Enter
                        // chord. Only the bare Shift-on-Enter case is
                        // rewritten — Ctrl/Alt already pass through
                        // unchanged (see `shift_enter_rewrite`).
                        let is_enter =
                            matches!(event.logical_key, WinitKey::Named(NamedKey::Enter));
                        let rewrite = shift_enter_rewrite(
                            is_enter,
                            host.current_mods,
                            self.app.settings.shift_enter_behavior,
                        );
                        // FR2 (key-resume): capture whether the key was
                        // forwarded to the PTY into a local flag. The
                        // `active_tab()` borrow holds `&self.app`, so we
                        // cannot call the `&mut self`-taking
                        // `scroll_to_live` until after the block ends.
                        let forwarded = if let Some(tab) = self.app.active_tab() {
                            // In mux mode the bytes will be wrapped as a
                            // `PtyInput` frame and reach a remote (canonically
                            // Linux) daemon, so we must skip the Windows-host
                            // Win32 Input Mode shim or the remote shell sees
                            // unknown CSI for Backspace / Escape / Ctrl+[.
                            let target = if tab.mux_session_name.is_some() {
                                EncodeTarget::PosixPty
                            } else {
                                EncodeTarget::HostPty
                            };
                            if let ShiftEnterRewrite::RawBytes(bytes) = rewrite {
                                // `kitty_csi_u`: bypass the key encoder
                                // entirely and write the literal CSI u
                                // sequence through the same output path as
                                // encoder-produced bytes (host-PTY raw
                                // write / mux PtyInput frame), per D1 —
                                // the encoder cannot express CSI u.
                                tab.write_input(bytes.to_vec());
                                true
                            } else {
                                let mods = match rewrite {
                                    ShiftEnterRewrite::Modifiers(m) => m,
                                    _ => host.current_mods,
                                };
                                if let Some(bytes) = winit_key_to_bytes(&event, mods, target) {
                                    // mux-aware: wraps as PtyInput in mux mode so the
                                    // bridge forwards it (raw stdin is dropped there).
                                    tab.write_input(bytes);
                                    true
                                } else {
                                    false
                                }
                            }
                        } else {
                            false
                        };
                        if forwarded {
                            // FR2: any key we forward to the PTY also snaps
                            // the viewport back to live tail. Bare modifiers
                            // return `None` (so `forwarded == false`); search
                            // overlay / profile selector / mux dialog / IME
                            // consume / special chord / mux prefix latch /
                            // settings keybinds all early-return before
                            // reaching here, so they never snap.
                            self.app.scroll_to_live();
                        }
                    }
                }
                host.window().request_redraw();
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if event.state == ElementState::Released => {
                // Synthetic key press gate (task0002): a synthetic
                // release is dropped by the same rule as a synthetic
                // press (see `should_drop_synthetic_key_event`).
                if should_drop_synthetic_key_event(is_synthetic) {
                    log::warn!(
                        "native-poc: dropping synthetic key release: physical_key={:?}",
                        event.physical_key
                    );
                    return;
                }
                // Phase 4-G-3: forward releases too so the
                // WinitImeBridge can observe Ghostty-style
                // modifier-only release events (fcitx5 toggles on bare
                // modifier release). The Phase 4-G-1 NullBackend
                // ignores releases.
                let raw_key = RawKeyEvent {
                    physical_key_code: winit_physical_key_code(&event),
                    state_pressed: false,
                    mods: host.current_mods,
                };
                let _ = self.app.dispatch_key_event_via_ime(&raw_key);
            }
            WindowEvent::PointerLeft { .. } => handle_pointer_left(host, &mut self.app),
            WindowEvent::PointerMoved { position, .. } => {
                handle_pointer_moved(position, host, &mut self.app)
            }
            WindowEvent::PointerButton { state, button, .. } => {
                handle_pointer_button(state, button, host, &mut self.app)
            }
            WindowEvent::MouseWheel { delta, .. } => handle_mouse_wheel(delta, host, &mut self.app),
            WindowEvent::DragEntered { .. } => {
                // A drag entered the window: show the drop overlay. The
                // message depends on whether the active tab is an SSH tab
                // (upload) or not (paste).
                let overlay = if self
                    .app
                    .active_tab()
                    .map(|t| t.is_ssh_tab())
                    .unwrap_or(false)
                {
                    crate::sftp::ui::HoverOverlay::SshUpload
                } else {
                    crate::sftp::ui::HoverOverlay::Paste
                };
                self.app.sftp_ui.hover = Some(overlay);
                host.window().request_redraw();
            }
            // The pointer moving while files are dragged over the window
            // carries no paths and needs no state change here — the
            // `DragEntered` overlay set above stays up until `DragLeft` /
            // `DragDropped`.
            WindowEvent::DragMoved { .. } => {}
            WindowEvent::DragLeft { .. } => {
                self.app.sftp_ui.hover = None;
                host.window().request_redraw();
            }
            WindowEvent::DragDropped { paths, .. } => {
                // winit 0.31 delivers the whole drag session's paths in
                // one event (FR3 / IMPLEMENTATION.md D3) — no cross-event
                // batching needed.
                self.app.sftp_ui.hover = None;
                if let Some(batch) = crate::sftp::ui::drop_batch_from_paths(paths) {
                    self.app.dispatch_drop(batch);
                    host.window().request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                host.render(&mut self.app);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let Some(host) = self.host.as_mut() else {
            return;
        };
        // Phase 4-G: drain any pending IME events from the active
        // backend into the existing on_ime_* routes before touching
        // PTY output. A real backend may have queued events while we
        // were idle; the NullBackend always returns an empty drain so
        // this is a cheap no-op when disabled.
        let ime_changed = self.app.pump_ime();
        let pty_changed = self.app.pump_all();
        // The child settings window reported a persisted save (its stdout
        // watcher raised the flag and woke this loop via the proxy):
        // reload settings.json and apply it live.
        if crate::settings_launcher::take_saved() {
            host.reload_settings_from_disk(&mut self.app);
        }
        // If the search overlay is open with live results, the pumps (PTY
        // output / resize) may have shifted matched text into scrollback,
        // staling the cached document and the matches' absolute rows. Re-run
        // the search once here so highlights track the text without yanking
        // the viewport. Per-frame cadence throttles bursts of PTY chunks.
        let search_changed = self.app.auto_research_if_dirty();
        // Cursor blink advances on a 530 ms half-cycle (BLINK_HALF_MS).
        // egui's request_repaint_after is silent (no callback bridges
        // it back to winit), so we have to detect the phase flip
        // ourselves and request a redraw — otherwise the cursor freezes
        // at whatever phase the last paint landed on.
        let blink_due = self.app.needs_blink_repaint();
        // Visual-bell flash decays over 150 ms; like blink, nothing
        // else would schedule the intermediate frames, so poll it here.
        let bell_due = self.app.needs_bell_repaint();
        // PTY content may have changed under a stationary pointer. Only
        // re-run detection when the pointer is inside the window and no
        // selection drag is in progress — PTY output during a drag must
        // not flip the cursor to a hand or underline a link mid-selection.
        // The staleness comparison + cache invalidation lives in
        // `refresh_link_hover_on_pty_change` so the `HoverState` fields
        // are never poked from the event loop body.
        if pty_changed && host.pointer_in_window && !host.dragging {
            host.refresh_link_hover_on_pty_change(&self.app);
        }
        // Toasts auto-dismiss on frame time, but nothing else schedules the
        // intermediate frames: on an idle / unfocused terminal the redraw
        // triggers above can all be false, so a visible toast would never be
        // pruned until an unrelated event. While any toast is up, keep frames
        // flowing so the restart / SFTP toasts dismiss on schedule.
        //
        // Rate-limited to the `TOAST_POLL_MS` cadence via `last_toast_redraw`:
        // the `WaitUntil` timer below does NOT bound this by itself, because
        // an unconditional `request_redraw()` here re-enters
        // `RedrawRequested` → `about_to_wait` immediately (the loop never
        // becomes idle enough to reach the timer), and with a non-blocking
        // present mode (Mailbox/Immediate, see `WindowHost::new`) nothing
        // else brakes the cycle — the loop spins at full speed for the
        // toast's entire lifetime.
        let toast_pending =
            self.app.restart_toast.active() || !self.app.sftp_ui.toasts.toasts.is_empty();
        let toast_due = toast_redraw_due(toast_pending, host.last_toast_redraw, Instant::now());
        if toast_due {
            host.last_toast_redraw = Some(Instant::now());
        }
        // task0002 D5: the overlay card's dim/fade needs a redraw request
        // at the same two junctures blink/bell/toast do — nothing else
        // would wake the loop at the bright-hold expiry or during the
        // fade, since `ControlFlow::WaitUntil` only re-enters this
        // function, it does not itself trigger a repaint. The actual
        // state mutation (arming/resetting the fade-bookkeeping instant)
        // happens in `WindowHost::render` via
        // `App::resolve_mux_sidebar_opacity`, once the redraw this
        // requests actually runs — this check is read-only.
        let mux_sidebar_dim_due = self.app.mux_sidebar_dim_due(Instant::now());
        // mux-tab-switch-bypass-refix task0002 Change 1 (finding
        // `81507f39e384b34e`): the fallback path for the resize-settle
        // self-wake — `refresh_status_bar_insets` (inside `render()`)
        // handles the fast path when a render is already happening; this
        // is what fires the wake when NOTHING else does (the exact
        // fully-idle-window case findings 02546e5e10deb500 /
        // 5b1878c41d3e02d6-perf-P2 describe), reached via the
        // `next_resize_settle_wake_deadline`-armed `WaitUntil` below. Both
        // sites read/write the same `last_resize_settle_wake`, so whichever
        // runs first for a given tick gates the other out.
        let resize_settle_wake_due = resize_settle_self_wake_due(
            host.resize_settler.awaiting_decision(),
            host.last_resize_settle_wake,
            Instant::now(),
        );
        if resize_settle_wake_due {
            host.last_resize_settle_wake = Some(Instant::now());
        }
        if ime_changed
            || pty_changed
            || search_changed
            || blink_due
            || bell_due
            || toast_due
            || mux_sidebar_dim_due
            || resize_settle_wake_due
        {
            host.window().request_redraw();
        }
        // Cursor cell may have moved as a side effect of pumps; notify
        // the IME backend if the (row, col) changed. Use the same
        // physical-pixel metrics + origin as the grid renderer so the
        // IME spot lands on the actual cursor cell, not a HiDPI-off
        // approximation.
        let (cell_w_px, cell_h_px, origin_x_px, origin_y_px) = host.cell_metrics_px(&self.app);
        self.app.notify_cursor_rect_if_changed(
            cell_w_px.round().max(1.0) as u32,
            cell_h_px.round().max(1.0) as u32,
            origin_x_px.round() as i32,
            origin_y_px.round() as i32,
        );
        // task0001 (windows-skk-ime-hang) FR1: flush any IME requests
        // recorded this turn (construction-time enable, notify_focus,
        // notify_cursor_rect_if_changed above) here — outside any
        // wndproc/event-dispatch frame — instead of calling the OS IME
        // APIs synchronously from inside `window_event`.
        self.app.flush_ime();
        if self.app.tabs.is_empty() || host.pending_close() {
            // Same teardown handshake as the CloseRequested path: drop
            // the wgpu / window resources before EventLoop unwinds so
            // the Vulkan WSI surface destructor sees a live X11
            // connection. Two close paths converge here:
            //   - the last tab closed (`apply_tab_event` removed it), or
            //   - the user clicked the CSD title-bar `×` (which sets
            //     `pending_close` rather than touching the event loop
            //     directly, so the drop order matches both other paths).
            self.host = None;
            event_loop.exit();
            return;
        }
        // task0004 D4: stop unconditionally rearming a 16 ms `WaitUntil`.
        // With no timed work pending (blink disabled or unfocused, no bell
        // decay, no toast) the loop drops to a true `ControlFlow::Wait` —
        // every producer that used to rely on this 60 Hz pump now wakes the
        // loop explicitly (PTY reader threads / mux off-thread workers via
        // `crate::wakeup::wake()`, IME/input via winit's native wake, this
        // turn's own blink/bell/toast deadlines via `control_flow_for`).
        event_loop.set_control_flow(control_flow_for(
            &self.app,
            next_resize_settle_wake_deadline(
                host.resize_settler.awaiting_decision(),
                Instant::now(),
            ),
        ));
    }

    /// Phase E (TS-32): winit `EventLoopProxy::wake_up()` calls land
    /// here (renamed from `user_event` in winit 0.31 — wake-ups no
    /// longer carry a payload). Without this override, the trait-default
    /// `proxy_wake_up` is a no-op and the provider-owned wake chain
    /// (`TimeProvider` timer thread → `WakeFn` → `EventLoopProxy::wake_up()`
    /// → here → `request_redraw`) is silently broken, freezing the
    /// status-bar clock when the shell is idle.
    ///
    /// Defensive: if `self.host` is `None` (we are between
    /// `can_create_surfaces`-time construction failure and process exit,
    /// or already torn down in `CloseRequested`), this is a no-op.
    fn proxy_wake_up(&mut self, _event_loop: &dyn ActiveEventLoop) {
        request_redraw_on_user_event(self.host.as_ref(), |host| {
            host.window().request_redraw();
        });
    }
}

impl Drop for PocApp {
    /// winit 0.31 removed `ApplicationHandler::exiting`; the same
    /// defense-in-depth shutdown step (for any code path that flagged
    /// exit without zeroing `self.host`, e.g. future error-path exits)
    /// now runs in `Drop`, called once `run_app` unwinds after
    /// `event_loop.exit()`. The Vulkan / X11 teardown must happen while
    /// EventLoop is still alive — see the field-order note on
    /// `WindowHost`.
    fn drop(&mut self) {
        if self.host.is_some() {
            log::info!("native-poc: exiting handler dropping WindowHost");
            self.host = None;
        }
    }
}

/// Pure-logic decision for `PocApp::user_event` (TS-32).
///
/// Extracted as a free function so unit tests can exercise the
/// "redraw if host is present, no-op otherwise" contract without
/// instantiating a real winit window (which requires an active
/// event loop and a display). The `redraw` callback is invoked at
/// most once, and only when `host` is `Some`.
pub(super) fn request_redraw_on_user_event<H, F>(host: Option<&H>, redraw: F)
where
    F: FnOnce(&H),
{
    if let Some(h) = host {
        redraw(h);
    }
}
