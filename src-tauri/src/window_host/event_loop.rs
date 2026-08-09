//! The winit application handler: PocApp, its `ApplicationHandler`
//! implementation (window creation, the window_event dispatch, and
//! about_to_wait pacing), shutdown via `Drop`, and the user-event
//! redraw hook.

use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::cursor::CursorIcon;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::WindowId;

use crate::app::App;
use crate::ime::backend::{KeyDispatchResult, ProcessEnv, RawKeyEvent, build_backend_with_window};
use crate::pty::input::{Modifiers, Target as EncodeTarget};
use crate::selection::{Pos, Selection, SelectionMode};

use super::frame_pacing::{
    control_flow_for, next_resize_settle_wake_deadline, resize_settle_self_wake_due,
    toast_redraw_due,
};
use super::input_translate::{
    ShiftEnterRewrite, accumulate_alt_scroll_lines, alternate_scroll_wheel_bytes,
    is_skk_swallowed_chord, shift_enter_rewrite, should_drop_synthetic_key_event,
    winit_key_to_bytes, winit_key_to_egui, winit_physical_key_code, winit_to_egui_button,
};
use super::key_routing::{
    egui_to_mux_input, handle_mux_dialog_key, handle_profile_selector_key, handle_search_key,
    handle_special_chord,
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
            WindowEvent::PointerLeft { .. } => {
                // Mark the pointer as outside the window so PTY-output
                // re-detection in `about_to_wait` is suppressed — there
                // is nothing to underline when no pointer is inside.
                host.pointer_in_window = false;
                // Reset the resize hint when the pointer leaves the
                // window so the cached direction doesn't outlive its
                // hit zone — without this, re-entering the interior
                // through a non-edge route keeps the last edge's
                // cursor + direction stuck (since `update_resize_hint`
                // short-circuits when the new dir matches the cached
                // one).
                if host.current_resize_dir.is_some() || host.current_cursor != CursorIcon::Default {
                    host.current_resize_dir = None;
                    host.current_cursor = CursorIcon::Default;
                    host.window.set_cursor(CursorIcon::Default.into());
                }
                // Drop any link-hover underline + hand cursor when the
                // pointer leaves the window.
                host.invalidate_link_hover();
                // task0002 FR3: the pointer can't be inside the overlay
                // card if it isn't even inside the window.
                self.app.set_mux_sidebar_hovered(false);
            }
            WindowEvent::PointerMoved { position, .. } => {
                host.pointer_in_window = true;
                host.cursor_pos = position;
                // Forward to egui so the tab bar / status bar widgets
                // observe hover + drag motion.
                let logical = position.to_logical::<f32>(host.pixels_per_point as f64);
                let egui_pos = egui::pos2(logical.x, logical.y);
                // Coalesce consecutive `PointerMoved`s: motion-only frames
                // are skippable (see `has_actionable_egui_input`), so
                // without coalescing a sustained motion burst with no
                // drawn frame in between (cursor_blink=false, or an
                // unfocused window — nothing else forces a drain) would
                // grow this queue one entry per motion event and rescan
                // it per event. Only the latest position matters to egui.
                if let Some(egui::Event::PointerMoved(last)) = host.pending_egui_events.last_mut() {
                    *last = egui_pos;
                } else {
                    host.pending_egui_events
                        .push(egui::Event::PointerMoved(egui_pos));
                }
                // task0002 FR3 / D5 "Hover feed": maintain the overlay
                // card's hover flag with the SAME hit test the press/wheel
                // routing below already query
                // (`ui::mux_sidebar::point_in_sidebar`, evaluated against
                // the `Overlay` placement only — the persistent panel and
                // hidden state never dim, and `point_in_sidebar` already
                // answers `false` for a `None` placement). Sharing the
                // derivation means hover and click can never disagree
                // about the boundary (IMPLEMENTATION.md cross-task
                // decision 3.5).
                {
                    let overlay_visible = matches!(
                        self.app.mux_sidebar_visibility(),
                        crate::app::MuxSidebarVisibility::Overlay
                    );
                    let placement =
                        overlay_visible.then_some(crate::ui::mux_sidebar::Placement::Overlay);
                    let window_size_logical = host
                        .window
                        .surface_size()
                        .to_logical::<f32>(host.pixels_per_point as f64);
                    let top_chrome =
                        crate::ui::mux_sidebar::top_chrome_inset(self.app.show_tab_bar);
                    let in_overlay = crate::ui::mux_sidebar::point_in_sidebar(
                        egui_pos,
                        placement,
                        egui::vec2(window_size_logical.width, window_size_logical.height),
                        top_chrome,
                        host.status_bar_bot_inset_logical,
                    );
                    self.app.set_mux_sidebar_hovered(in_overlay);
                }
                // CSD edge-resize hot zone: refresh the cached
                // ResizeDirection + pointer icon so the next left-press
                // can hand the matching direction to
                // `Window::drag_resize_window`. Skipped while a
                // terminal selection drag is in flight — the pointer
                // can pass through an edge band on its way to the
                // selection target, and swapping to a resize icon
                // mid-drag would be jarring.
                if !host.dragging {
                    host.update_resize_hint(logical.x, logical.y);
                    // Link hover: skipped while selection-dragging so a
                    // drag through a link doesn't flip to a hand cursor /
                    // underline mid-selection.
                    host.refresh_link_hover(&self.app);
                }
                host.window().request_redraw();
                if host.dragging {
                    let (screen_row, col) = host.pixel_to_cell(position, &self.app);
                    // Convert the screen row to its absolute buffer row so the
                    // extended endpoint stays pinned to the content as the
                    // viewport scrolls.
                    let abs_row = host.screen_row_to_abs(screen_row, &self.app);
                    // First motion since the press in Character mode
                    // upgrades the pending click into a real Selection.
                    // Word / line selections (double / triple click)
                    // were already committed at press time and the
                    // pending anchor was cleared there.
                    if self.app.selection.is_none() {
                        if let Some(anchor) = self.app.pending_selection_anchor.take() {
                            self.app.selection =
                                Some(Selection::new_with_mode(anchor, SelectionMode::Character));
                        }
                    }
                    if let Some(sel) = self.app.selection.as_mut() {
                        if let Some(tab) = self.app.tabs.get(self.app.active) {
                            let core = tab.core.lock();
                            sel.extend(Pos { row: abs_row, col }, &core);
                        }
                    }
                }
            }
            WindowEvent::PointerButton { state, button, .. } => {
                // winit 0.31's pointer-event overhaul folds mouse/touch/
                // pen buttons into `ButtonSource`; normalize to the plain
                // `MouseButton` this handler already speaks. Non-mouse
                // sources with no natural `MouseButton` mapping are
                // ignored (touch already normalizes to `Left` inside
                // `mouse_button()`).
                let Some(button) = button.mouse_button() else {
                    return;
                };
                // CSD edge-resize: a left press on the edge hot zone
                // hands off to the WM via `drag_resize_window`. Run
                // before the egui forward so the tab bar / title bar
                // never see a phantom click on the corner pixel they
                // happen to overlap with the resize gutter, and skip
                // the rest of this handler so no terminal selection
                // gets started under the cursor.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    if let Some(dir) = host.current_resize_dir {
                        if let Err(e) = host.window.drag_resize_window(dir) {
                            log::warn!("native-poc: drag_resize_window failed: {e}");
                        }
                        return;
                    }
                }
                // Forward to egui first so the tab bar / status bar can
                // see the click before we decide whether to start a
                // terminal selection.
                let logical = host
                    .cursor_pos
                    .to_logical::<f32>(host.pixels_per_point as f64);
                let egui_pos = egui::pos2(logical.x, logical.y);
                if let Some(eb) = winit_to_egui_button(button) {
                    host.pending_egui_events.push(egui::Event::PointerButton {
                        pos: egui_pos,
                        button: eb,
                        pressed: matches!(state, ElementState::Pressed),
                        modifiers: egui::Modifiers::default(),
                    });
                }
                // Held-button bookkeeping for the frame-skip veto: while
                // an egui-mapped button is down, `PointerMoved` counts as
                // actionable so egui chrome drags keep their live tracking
                // (see `has_actionable_egui_input`). Only buttons egui can
                // observe are counted — a held side button can't drive a
                // chrome drag, so it must not defeat the idle skip during
                // motion. Saturating on both edges — a stray release (e.g.
                // after focus loss reset the count) must not underflow.
                if winit_to_egui_button(button).is_some() {
                    match state {
                        ElementState::Pressed => {
                            host.pointer_buttons_down = host.pointer_buttons_down.saturating_add(1);
                        }
                        ElementState::Released => {
                            host.pointer_buttons_down = host.pointer_buttons_down.saturating_sub(1);
                        }
                    }
                }
                host.window().request_redraw();

                // Clicks that land on the egui-owned strip (CSD title
                // bar + tab bar at the top, status bar at the bottom
                // when enabled) must not also kick off a terminal
                // selection — otherwise pressing the × on a tab (or
                // the close button on the title bar) would
                // simultaneously start a selection on the cell behind
                // it.
                let top_strip_h = crate::ui::title_bar::TITLE_BAR_HEIGHT
                    + crate::ui::tab_bar::effective_tab_bar_height(self.app.show_tab_bar);
                let if_in_egui_strip = egui_pos.y < top_strip_h;
                if if_in_egui_strip {
                    return;
                }
                // Same rule for the bottom status-bar panel and the
                // right-edge scrollbar overlay: a press on either
                // would otherwise drag-select the terminal row that
                // happens to sit under the bar. Gated to the Pressed
                // edge only so a drag that *started* inside the
                // terminal still gets its Released event processed
                // (clears `host.dragging`, commits selection) when the
                // user happens to lift the button over the strip.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    let window_size_logical = host
                        .window
                        .surface_size()
                        .to_logical::<f32>(host.pixels_per_point as f64);
                    let bottom_strip_top =
                        window_size_logical.height - host.status_bar_bot_inset_logical;
                    let in_bottom_strip =
                        host.status_bar_bot_inset_logical > 0.0 && egui_pos.y >= bottom_strip_top;
                    let scrollbar_visible = self
                        .app
                        .active_tab()
                        .map(|tab| {
                            let core = tab.core.lock();
                            crate::ui::scrollbar::ScrollbarView {
                                mode: self.app.settings.show_scrollbar,
                                scrollback_len: core.get_scrollback_length(),
                                viewport_rows: core.rows() as u32,
                                scroll_offset: self.app.scroll_offset(),
                                alt_screen: self.app.alt_screen,
                            }
                            .visible()
                        })
                        .unwrap_or(false);
                    let central_right = window_size_logical.width - host.mux_sidebar_inset_logical;
                    let in_scrollbar = scrollbar_visible
                        && egui_pos.x >= central_right - crate::ui::scrollbar::TRACK_W
                        && egui_pos.x < central_right;
                    // AC-1/AC-4: query the SAME shared hit-region helper the
                    // MouseWheel guard below uses (IMPLEMENTATION.md cross-
                    // task decision 3.5), instead of the persistent-only
                    // width test above — that test's inset is 0 for the
                    // overlay placement, so a press on the floating overlay
                    // card used to fall through this guard and start a
                    // terminal selection on the cell underneath it.
                    let visible_placement = match self.app.mux_sidebar_visibility() {
                        crate::app::MuxSidebarVisibility::Hidden => None,
                        crate::app::MuxSidebarVisibility::Persistent => {
                            Some(crate::ui::mux_sidebar::Placement::Persistent)
                        }
                        crate::app::MuxSidebarVisibility::Overlay => {
                            Some(crate::ui::mux_sidebar::Placement::Overlay)
                        }
                    };
                    let top_chrome =
                        crate::ui::mux_sidebar::top_chrome_inset(self.app.show_tab_bar);
                    let in_sidebar = crate::ui::mux_sidebar::point_in_sidebar(
                        egui_pos,
                        visible_placement,
                        egui::vec2(window_size_logical.width, window_size_logical.height),
                        top_chrome,
                        host.status_bar_bot_inset_logical,
                    );
                    if in_bottom_strip || in_scrollbar || in_sidebar {
                        return;
                    }
                }

                // While the profile-selector modal is up, every click
                // belongs to egui (a row, or the scrim which dismisses);
                // never start a terminal selection underneath it.
                if self.app.profile_selector.visible {
                    return;
                }

                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        // Ctrl+click opens a hovered URL / file path and
                        // skips starting a selection. Reuses the cached
                        // hover detection for the cell under the pointer
                        // (refreshed on the PointerMoved that brought us
                        // here), re-detecting only if the cached cell no
                        // longer matches the click cell.
                        if host.current_mods.ctrl && host.try_open_link_at_pointer(&self.app) {
                            return;
                        }
                        let (screen_row, col) = host.pixel_to_cell(host.cursor_pos, &self.app);
                        // Anchor the press at its absolute buffer row so the
                        // selection (and double / triple-click classification)
                        // tracks the content across scrolls.
                        let abs_row = host.screen_row_to_abs(screen_row, &self.app);
                        let cls = host.click_tracker.classify(Instant::now(), abs_row, col);
                        if cls.mode == SelectionMode::Character {
                            // Single click in character mode: do not
                            // materialize a one-cell selection yet — the
                            // user may just be moving the cursor / focus
                            // / clearing a prior selection. Record the
                            // press cell so the first motion (if any)
                            // can upgrade this into a real drag-select.
                            self.app.selection = None;
                            self.app.pending_selection_anchor = Some(Pos { row: abs_row, col });
                            host.window().request_redraw();
                        } else {
                            // Word (double click) / line (triple click)
                            // commit immediately so a static click still
                            // selects the targeted word or line.
                            let mut sel =
                                Selection::new_with_mode(Pos { row: abs_row, col }, cls.mode);
                            if let Some(tab) = self.app.tabs.get(self.app.active) {
                                let core = tab.core.lock();
                                sel.extend(Pos { row: abs_row, col }, &core);
                            }
                            self.app.selection = Some(sel);
                            self.app.pending_selection_anchor = None;
                        }
                        host.dragging = true;
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        host.dragging = false;
                        // A press with no motion in Character mode left
                        // selection == None (see the Pressed branch);
                        // there is nothing to copy in that case. `pending`
                        // is `Some` exactly for that case: a single (not
                        // word/line) press whose motion never upgraded it to
                        // a drag-select. Capture it before the reset so the
                        // fold-click path below can detect a plain click.
                        let pending = self.app.pending_selection_anchor.take();
                        // Plain left-click (no Ctrl; meta does not exist on
                        // Linux/Windows), no active selection, no drag: this
                        // is a candidate for a fold toggle. Mirrors the
                        // WebView `input-wiring.ts` routing (Ctrl/Meta →
                        // URL, else → handleFoldClick) plus
                        // `handleFoldClick`'s own "no text selection" guard.
                        // `handle_fold_click` is a no-op (returns false)
                        // when the click is not over a foldable region, so
                        // ordinary clicks-to-deselect fall through unchanged.
                        if pending.is_some()
                            && self.app.selection.is_none()
                            && !host.current_mods.ctrl
                        {
                            if let Some((row, _col)) =
                                host.pixel_to_grid_cell(host.cursor_pos, &self.app)
                            {
                                if self.app.handle_fold_click(row) {
                                    host.invalidate_link_hover();
                                    host.window().request_redraw();
                                    return;
                                }
                            }
                        }
                        if let Some(sel) = self.app.selection {
                            if let Some(tab) = self.app.tabs.get(self.app.active) {
                                let core = tab.core.lock();
                                let text = sel.resolve(&core, self.app.fold_layout());
                                drop(core);
                                host.set_primary(&text);
                                // `copy_on_select` opts into mirroring the
                                // selection to the system CLIPBOARD as
                                // well, matching the WebView build's
                                // toggle. PRIMARY is always updated above
                                // so the middle-click flow keeps working
                                // regardless.
                                if self.app.settings.copy_on_select && !text.is_empty() {
                                    host.set_clipboard(&text);
                                }
                            }
                        }
                    }
                    (MouseButton::Middle, ElementState::Pressed) => {
                        if self.app.settings.middle_click_paste {
                            if let Some(text) = host.get_primary() {
                                host.deliver_paste(&self.app, &text);
                            }
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // While the profile-selector modal is up, the wheel
                // scrolls the modal's list: translate to an egui
                // MouseWheel event (the raw-input builder does not
                // forward wheel deltas on the terminal path) and skip
                // the terminal viewport scroll.
                if self.app.profile_selector.visible {
                    let (unit, delta) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => {
                            (egui::MouseWheelUnit::Line, egui::vec2(x, y))
                        }
                        MouseScrollDelta::PixelDelta(p) => (
                            egui::MouseWheelUnit::Point,
                            egui::vec2(p.x as f32, p.y as f32),
                        ),
                    };
                    host.pending_egui_events.push(egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers: egui::Modifiers::default(),
                    });
                    host.window().request_redraw();
                    return;
                }
                // FR2/FR3: a wheel over the tab-bar strip scrolls the tab
                // strip horizontally instead of the terminal scrollback.
                // Forward the wheel to egui — the tab strip's horizontal
                // ScrollArea consumes it, and with
                // `always_scroll_the_only_direction` set both bare and
                // Shift+wheel fold onto the horizontal axis. egui hit-tests
                // against the hover position kept current by the
                // `PointerMoved` events forwarded on every winit
                // `WindowEvent::PointerMoved`, so the wheel only reaches
                // the strip when the pointer is over it.
                // Restricted to the tab-bar band (below the CSD title bar);
                // the title bar's existing wheel behaviour is left untouched.
                {
                    let logical = host
                        .cursor_pos
                        .to_logical::<f32>(host.pixels_per_point as f64);
                    let top_strip_h = crate::ui::title_bar::TITLE_BAR_HEIGHT
                        + crate::ui::tab_bar::effective_tab_bar_height(self.app.show_tab_bar);
                    if logical.y >= crate::ui::title_bar::TITLE_BAR_HEIGHT
                        && logical.y < top_strip_h
                    {
                        let (unit, ev_delta) = match delta {
                            MouseScrollDelta::LineDelta(x, y) => {
                                (egui::MouseWheelUnit::Line, egui::vec2(x, y))
                            }
                            MouseScrollDelta::PixelDelta(p) => (
                                egui::MouseWheelUnit::Point,
                                egui::vec2(p.x as f32, p.y as f32),
                            ),
                        };
                        host.pending_egui_events.push(egui::Event::MouseWheel {
                            unit,
                            delta: ev_delta,
                            modifiers: egui::Modifiers::default(),
                        });
                        host.window().request_redraw();
                        return;
                    }
                }
                // task0010 FR2/NFR2: a wheel over the mux sidebar
                // (persistent panel OR overlay card) scrolls the sidebar's
                // window list instead of the terminal scrollback /
                // AltScreen arrow-scroll path. `point_in_sidebar` is the
                // SAME hit-region derivation `ui::mux_sidebar`'s draw path
                // uses (IMPLEMENTATION.md cross-task decision 3.5), so this
                // guard can never independently drift from what's actually
                // painted — the round-2 lesson a manual, re-derived
                // winit-side guard caused. `visible_placement` resolves to
                // `None` on local tabs and sidebar-hidden states, so
                // `point_in_sidebar` always answers `false` there and this
                // block is a complete no-op (NFR2).
                {
                    let visible_placement = match self.app.mux_sidebar_visibility() {
                        crate::app::MuxSidebarVisibility::Hidden => None,
                        crate::app::MuxSidebarVisibility::Persistent => {
                            Some(crate::ui::mux_sidebar::Placement::Persistent)
                        }
                        crate::app::MuxSidebarVisibility::Overlay => {
                            Some(crate::ui::mux_sidebar::Placement::Overlay)
                        }
                    };
                    if visible_placement.is_some() {
                        let logical = host
                            .cursor_pos
                            .to_logical::<f32>(host.pixels_per_point as f64);
                        let window_size_logical = host
                            .window
                            .surface_size()
                            .to_logical::<f32>(host.pixels_per_point as f64);
                        let top_chrome =
                            crate::ui::mux_sidebar::top_chrome_inset(self.app.show_tab_bar);
                        if crate::ui::mux_sidebar::point_in_sidebar(
                            egui::pos2(logical.x, logical.y),
                            visible_placement,
                            egui::vec2(window_size_logical.width, window_size_logical.height),
                            top_chrome,
                            host.status_bar_bot_inset_logical,
                        ) {
                            let (unit, ev_delta) = match delta {
                                MouseScrollDelta::LineDelta(x, y) => {
                                    (egui::MouseWheelUnit::Line, egui::vec2(x, y))
                                }
                                MouseScrollDelta::PixelDelta(p) => (
                                    egui::MouseWheelUnit::Point,
                                    egui::vec2(p.x as f32, p.y as f32),
                                ),
                            };
                            host.pending_egui_events.push(egui::Event::MouseWheel {
                                unit,
                                delta: ev_delta,
                                modifiers: egui::Modifiers::default(),
                            });
                            host.window().request_redraw();
                            return;
                        }
                    }
                }
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => {
                        let (_, cell_h_px, _, _) = host.cell_metrics_px(&self.app);
                        (p.y as f32) / (cell_h_px.max(1.0) as f32)
                    }
                };

                // FR1 (DECSET 1007): in alternate screen, when the
                // terminal-side mode bit AND the user setting are both
                // ON, translate the wheel notches into arrow-key bytes
                // sent to the active PTY so AltScreen apps (Claude
                // Code, vim, less) scroll their own log instead of
                // moving eMterm's scrollback view. xterm convention:
                // 3 arrow bytes per notch; Shift is ignored.
                let mode_bit_on = self
                    .app
                    .active_tab()
                    .map(|t| {
                        t.core
                            .lock()
                            .get_mode(term_core::terminal_core::MODE_ALTERNATE_SCROLL)
                    })
                    .unwrap_or(false);
                // FR1 accumulator: reset fractional state when not in AltScreen
                // so entering AltScreen always starts clean.
                if !self.app.alt_screen {
                    host.alt_scroll_accum = 0.0;
                }
                let (whole, new_frac) = accumulate_alt_scroll_lines(host.alt_scroll_accum, lines);
                host.alt_scroll_accum = new_frac;
                if whole != 0.0 {
                    if let Some(buf) = alternate_scroll_wheel_bytes(
                        whole,
                        self.app.alt_screen,
                        mode_bit_on,
                        self.app.settings.alternate_scroll_enabled,
                    ) {
                        if let Some(tab) = self.app.active_tab() {
                            tab.write_input(buf);
                        }
                        // Visible content may shift under the pointer;
                        // drop the cached hover so the next PointerMoved
                        // re-detects.
                        host.invalidate_link_hover();
                        host.window().request_redraw();
                        return;
                    }
                }

                // `settings.scroll_speed` is clamped to 1..=10 by the
                // loader, so it's safe to feed directly into the scroll
                // helpers (a runaway typo can't fly the viewport 1000
                // rows per notch).
                let step = self.app.settings.scroll_speed.max(1);
                if lines > 0.0 {
                    self.app.scroll_up_by(step);
                    // Scrollback content shifts under the pointer, so the
                    // cached hover no longer maps to the same text. Drop
                    // it; the next PointerMoved re-detects.
                    host.invalidate_link_hover();
                    host.window().request_redraw();
                } else if lines < 0.0 {
                    self.app.scroll_down_by(step);
                    host.invalidate_link_hover();
                    host.window().request_redraw();
                }
            }
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
