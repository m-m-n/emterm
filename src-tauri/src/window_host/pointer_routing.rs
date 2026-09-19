//! Pointer routing for the `window_event` dispatch: the PointerLeft /
//! PointerMoved / PointerButton / MouseWheel arm bodies, moved verbatim
//! from `event_loop.rs` as the pointer-side counterpart of
//! `key_routing.rs`.

use std::time::Instant;

use winit::cursor::CursorIcon;
use winit::dpi::PhysicalPosition;
use winit::event::{ButtonSource, ElementState, MouseButton, MouseScrollDelta};

use crate::app::App;
use crate::selection::{Pos, Selection, SelectionMode};

use super::WindowHost;
use super::input_translate::{
    accumulate_alt_scroll_lines, alternate_scroll_wheel_bytes, wheel_consumer,
    wheel_report_notches, winit_button_to_report_identity, winit_to_egui_button,
};
use super::input_translate::WheelConsumer;
use super::mouse_report::{self, MouseButtonId, MouseEventKind, MouseReportEncoding};

/// Snapshot of the SC-1 mouse-mode bits (IMPLEMENTATION.md; owned by
/// task0001), read from the active tab's core on every event rather than
/// mirrored host-side (decision D2 — the same route the alternate-scroll
/// path already uses via `get_mode`).
#[derive(Debug, Clone, Copy, Default)]
struct TrackingState {
    mode_1000: bool,
    mode_1002: bool,
    mode_1003: bool,
    sgr: bool,
}

impl TrackingState {
    fn read(core: &term_core::terminal_core::TerminalCore) -> Self {
        Self {
            mode_1000: core.get_mode(term_core::terminal_core::MODE_MOUSE_NORMAL_TRACKING),
            mode_1002: core.get_mode(term_core::terminal_core::MODE_MOUSE_BUTTON_EVENT_TRACKING),
            mode_1003: core.get_mode(term_core::terminal_core::MODE_MOUSE_ANY_EVENT_TRACKING),
            sgr: core.get_mode(term_core::terminal_core::MODE_MOUSE_SGR_ENCODING),
        }
    }

    /// Step 3 of the precedence order: is ANY tracking mode active.
    fn any_active(&self) -> bool {
        self.mode_1000 || self.mode_1002 || self.mode_1003
    }

    fn encoding(&self) -> MouseReportEncoding {
        if self.sgr {
            MouseReportEncoding::Sgr
        } else {
            MouseReportEncoding::X10
        }
    }
}

/// `WindowEvent::PointerLeft` arm body: clear the pointer-inside flag,
/// the cached resize hint / cursor, the link hover, and the mux-sidebar
/// hover feed.
pub(super) fn handle_pointer_left(host: &mut WindowHost, app: &mut App) {
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
    // one). `apply_cursor_icon` skips the IPC when the
    // arrow is already showing.
    host.current_resize_dir = None;
    host.apply_cursor_icon(CursorIcon::Default);
    // Drop any link-hover underline + hand cursor when the
    // pointer leaves the window.
    host.invalidate_link_hover();
    // task0002 FR3: the pointer can't be inside the overlay
    // card if it isn't even inside the window.
    app.set_mux_sidebar_hovered(false);
}

/// `WindowEvent::PointerMoved` arm body: egui motion forwarding, the
/// mux-sidebar hover feed, the CSD resize hint / link hover refresh,
/// and selection-drag extension.
pub(super) fn handle_pointer_moved(
    position: PhysicalPosition<f64>,
    host: &mut WindowHost,
    app: &mut App,
) {
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
            app.mux_sidebar_visibility(),
            crate::app::MuxSidebarVisibility::Overlay
        );
        let placement = overlay_visible.then_some(crate::ui::mux_sidebar::Placement::Overlay);
        let window_size_logical = host
            .window
            .surface_size()
            .to_logical::<f32>(host.pixels_per_point as f64);
        let top_chrome = crate::ui::mux_sidebar::top_chrome_inset(app.show_tab_bar);
        let in_overlay = crate::ui::mux_sidebar::point_in_sidebar(
            egui_pos,
            placement,
            egui::vec2(window_size_logical.width, window_size_logical.height),
            top_chrome,
            host.status_bar_bot_inset_logical,
        );
        app.set_mux_sidebar_hovered(in_overlay);
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
        host.refresh_link_hover(app);
    }
    host.window().request_redraw();
    if host.dragging {
        let (screen_row, col) = host.pixel_to_cell(position, app);
        // Convert the screen row to its absolute buffer row so the
        // extended endpoint stays pinned to the content as the
        // viewport scrolls.
        let abs_row = host.screen_row_to_abs(screen_row, app);
        // First motion since the press in Character mode
        // upgrades the pending click into a real Selection.
        // Word / line selections (double / triple click)
        // were already committed at press time and the
        // pending anchor was cleared there.
        if app.selection.is_none() {
            if let Some(anchor) = app.pending_selection_anchor.take() {
                app.selection = Some(Selection::new_with_mode(anchor, SelectionMode::Character));
            }
        }
        if let Some(sel) = app.selection.as_mut() {
            if let Some(tab) = app.tabs.get(app.active) {
                let core = tab.core.lock();
                sel.extend(Pos { row: abs_row, col }, &core);
            }
        }
    }

    // ── Mouse reporting (task0003 FR3/FR7/FR9/FR11, SC-4/SC-5) ────────
    // Decided independently of the local selection-drag path above: with
    // no tracking mode active, or with Shift held, this block is a
    // no-op and today's drag-to-select behaviour is untouched.
    if let Some(tab) = app.active_tab() {
        let tracking = TrackingState::read(&tab.core.lock());
        // D7: the cell-change cache is reset on an active-tab change —
        // checked regardless of tracking/Shift state, so a cell cached
        // against one tab never suppresses a report for another.
        if host.mouse_report_last_active_tab != Some(app.active) {
            host.mouse_report_cell_cache.reset();
            host.mouse_report_last_active_tab = Some(app.active);
        }
        if !tracking.any_active() {
            // D7: also reset whenever no tracking mode is active, so the
            // first motion of the next tracking session always reports.
            host.mouse_report_cell_cache.reset();
        } else if !host.current_mods.shift {
            if let Some(identity) = mouse_report::motion_gate(
                tracking.mode_1000,
                tracking.mode_1002,
                tracking.mode_1003,
                host.mouse_report_held_left,
                host.mouse_report_held_middle,
                host.mouse_report_held_right,
            ) {
                let (screen_row, col) = host.pixel_to_cell(position, app);
                let col1 = col as u32 + 1;
                let row1 = screen_row as u32 + 1;
                if host.mouse_report_cell_cache.should_report(col1, row1) {
                    let encoding = tracking.encoding();
                    let code = mouse_report::compose_button_code(
                        MouseEventKind::Motion,
                        identity,
                        encoding,
                        host.current_mods,
                    );
                    if let Some(bytes) = mouse_report::encode_report(code, col1, row1, encoding, false)
                    {
                        tab.write_input(bytes);
                    }
                }
            }
        }
    }
}

/// `WindowEvent::PointerButton` arm body: CSD edge-resize handoff, egui
/// click forwarding, the strip / scrollbar / sidebar press guards, and
/// terminal selection start / commit plus middle-click paste.
pub(super) fn handle_pointer_button(
    state: ElementState,
    button: ButtonSource,
    host: &mut WindowHost,
    app: &mut App,
) {
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
    // Mouse reporting (SC-5 input): track which buttons are physically
    // held, independent of any guard below that may go on to consume
    // this event locally — a button held while the pointer later drags
    // into the terminal must still be reportable by the motion gate.
    if let Some(identity) = winit_button_to_report_identity(button) {
        let held = state == ElementState::Pressed;
        match identity {
            MouseButtonId::Left => host.mouse_report_held_left = held,
            MouseButtonId::Middle => host.mouse_report_held_middle = held,
            MouseButtonId::Right => host.mouse_report_held_right = held,
            MouseButtonId::None => {}
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
        + crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar);
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
        let bottom_strip_top = window_size_logical.height - host.status_bar_bot_inset_logical;
        let in_bottom_strip =
            host.status_bar_bot_inset_logical > 0.0 && egui_pos.y >= bottom_strip_top;
        let scrollbar_visible = app
            .active_tab()
            .map(|tab| {
                let core = tab.core.lock();
                crate::ui::scrollbar::ScrollbarView {
                    mode: app.settings.show_scrollbar,
                    scrollback_len: core.get_scrollback_length(),
                    viewport_rows: core.rows() as u32,
                    scroll_offset: app.scroll_offset(),
                    alt_screen: app.alt_screen,
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
        let visible_placement = match app.mux_sidebar_visibility() {
            crate::app::MuxSidebarVisibility::Hidden => None,
            crate::app::MuxSidebarVisibility::Persistent => {
                Some(crate::ui::mux_sidebar::Placement::Persistent)
            }
            crate::app::MuxSidebarVisibility::Overlay => {
                Some(crate::ui::mux_sidebar::Placement::Overlay)
            }
        };
        let top_chrome = crate::ui::mux_sidebar::top_chrome_inset(app.show_tab_bar);
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
    if app.profile_selector.visible {
        return;
    }

    // ── Mouse reporting (task0003 FR2/FR7/FR8/FR10, SC-2/SC-3) ────────
    // Every existing chrome guard above has already had its chance to
    // consume this event and return; this is precedence steps 2 and 3
    // (IMPLEMENTATION.md "Precedence order"): the Shift override, then
    // the tracking-active question. With a tracking mode active and
    // Shift not held, a press/release reports instead of running the
    // local selection / Ctrl+link-open / middle-paste handling below —
    // AC-5's Ctrl+left and middle-press cases are covered here because
    // this returns before the `match` arms that implement those paths.
    // A press and its matching release must be settled by the SAME owner
    // (report path vs. local path) even if Shift's state changes between
    // them — otherwise a Shift+press that started a local selection-drag
    // (which sets `host.dragging`) can have its release re-decided into
    // the report path if Shift is lifted first, leaving `host.dragging`
    // stuck and `pending_selection_anchor` never taken. Route a Left
    // release back to the local arm whenever a local drag is in flight,
    // regardless of the Shift state at release time.
    let release_owned_by_local_drag =
        state == ElementState::Released && button == MouseButton::Left && host.dragging;
    if !host.current_mods.shift && !release_owned_by_local_drag {
        if let Some(identity) = winit_button_to_report_identity(button) {
            if let Some(tab) = app.active_tab() {
                let tracking = TrackingState::read(&tab.core.lock());
                if tracking.any_active() {
                    let event_kind = match state {
                        ElementState::Pressed => MouseEventKind::Press,
                        ElementState::Released => MouseEventKind::Release,
                    };
                    let (screen_row, col) = host.pixel_to_cell(host.cursor_pos, app);
                    let col1 = col as u32 + 1;
                    let row1 = screen_row as u32 + 1;
                    let encoding = tracking.encoding();
                    let code = mouse_report::compose_button_code(
                        event_kind,
                        identity,
                        encoding,
                        host.current_mods,
                    );
                    let release = state == ElementState::Released;
                    if let Some(bytes) =
                        mouse_report::encode_report(code, col1, row1, encoding, release)
                    {
                        tab.write_input(bytes);
                    }
                    return;
                }
            }
        }
    }

    match (button, state) {
        (MouseButton::Left, ElementState::Pressed) => {
            // Ctrl+click opens a hovered URL / file path and
            // skips starting a selection. Reuses the cached
            // hover detection for the cell under the pointer
            // (refreshed on the PointerMoved that brought us
            // here), re-detecting only if the cached cell no
            // longer matches the click cell.
            if host.current_mods.ctrl && host.try_open_link_at_pointer(app) {
                return;
            }
            let (screen_row, col) = host.pixel_to_cell(host.cursor_pos, app);
            // Anchor the press at its absolute buffer row so the
            // selection (and double / triple-click classification)
            // tracks the content across scrolls.
            let abs_row = host.screen_row_to_abs(screen_row, app);
            let cls = host.click_tracker.classify(Instant::now(), abs_row, col);
            if cls.mode == SelectionMode::Character {
                // Single click in character mode: do not
                // materialize a one-cell selection yet — the
                // user may just be moving the cursor / focus
                // / clearing a prior selection. Record the
                // press cell so the first motion (if any)
                // can upgrade this into a real drag-select.
                app.selection = None;
                app.pending_selection_anchor = Some(Pos { row: abs_row, col });
                host.window().request_redraw();
            } else {
                // Word (double click) / line (triple click)
                // commit immediately so a static click still
                // selects the targeted word or line.
                let mut sel = Selection::new_with_mode(Pos { row: abs_row, col }, cls.mode);
                if let Some(tab) = app.tabs.get(app.active) {
                    let core = tab.core.lock();
                    sel.extend(Pos { row: abs_row, col }, &core);
                }
                app.selection = Some(sel);
                app.pending_selection_anchor = None;
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
            let pending = app.pending_selection_anchor.take();
            // Plain left-click (no Ctrl; meta does not exist on
            // Linux/Windows), no active selection, no drag: this
            // is a candidate for a fold toggle. Mirrors the
            // WebView `input-wiring.ts` routing (Ctrl/Meta →
            // URL, else → handleFoldClick) plus
            // `handleFoldClick`'s own "no text selection" guard.
            // `handle_fold_click` is a no-op (returns false)
            // when the click is not over a foldable region, so
            // ordinary clicks-to-deselect fall through unchanged.
            if pending.is_some() && app.selection.is_none() && !host.current_mods.ctrl {
                if let Some((row, _col)) = host.pixel_to_grid_cell(host.cursor_pos, app) {
                    if app.handle_fold_click(row) {
                        host.invalidate_link_hover();
                        host.window().request_redraw();
                        return;
                    }
                }
            }
            if let Some(sel) = app.selection {
                if let Some(tab) = app.tabs.get(app.active) {
                    let core = tab.core.lock();
                    let text = sel.resolve(&core, app.fold_layout());
                    drop(core);
                    host.set_primary(&text);
                    // `copy_on_select` opts into mirroring the
                    // selection to the system CLIPBOARD as
                    // well, matching the WebView build's
                    // toggle. PRIMARY is always updated above
                    // so the middle-click flow keeps working
                    // regardless.
                    if app.settings.copy_on_select && !text.is_empty() {
                        host.set_clipboard(&text);
                    }
                }
            }
        }
        (MouseButton::Middle, ElementState::Pressed) => {
            if app.settings.middle_click_paste {
                if let Some(text) = host.get_primary() {
                    host.deliver_paste(app, &text);
                }
            }
        }
        _ => {}
    }
}

/// `WindowEvent::MouseWheel` arm body: profile-selector / tab-strip /
/// mux-sidebar wheel forwarding to egui, the DECSET 1007 AltScreen
/// arrow translation, and the terminal scrollback scroll path.
pub(super) fn handle_mouse_wheel(delta: MouseScrollDelta, host: &mut WindowHost, app: &mut App) {
    // While the profile-selector modal is up, the wheel
    // scrolls the modal's list: translate to an egui
    // MouseWheel event (the raw-input builder does not
    // forward wheel deltas on the terminal path) and skip
    // the terminal viewport scroll.
    if app.profile_selector.visible {
        let (unit, delta) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (egui::MouseWheelUnit::Line, egui::vec2(x, y)),
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
            + crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar);
        if logical.y >= crate::ui::title_bar::TITLE_BAR_HEIGHT && logical.y < top_strip_h {
            let (unit, ev_delta) = match delta {
                MouseScrollDelta::LineDelta(x, y) => (egui::MouseWheelUnit::Line, egui::vec2(x, y)),
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
        let visible_placement = match app.mux_sidebar_visibility() {
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
            let top_chrome = crate::ui::mux_sidebar::top_chrome_inset(app.show_tab_bar);
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
            let (_, cell_h_px, _, _) = host.cell_metrics_px(app);
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
    let mode_bit_on = app
        .active_tab()
        .map(|t| {
            t.core
                .lock()
                .get_mode(term_core::terminal_core::MODE_ALTERNATE_SCROLL)
        })
        .unwrap_or(false);
    // Mouse reporting (task0003 FR4/FR7/FR8, SC-6): the wheel decision is
    // taken once, ahead of both existing wheel consumers below, and its
    // result selects exactly one of the three (IMPLEMENTATION.md
    // "Wheel call site"). Its tracking-active branch already folds in
    // the Shift override (D5), so `host.current_mods.shift` is passed
    // straight in rather than short-circuiting here.
    let tracking = app
        .active_tab()
        .map(|t| TrackingState::read(&t.core.lock()))
        .unwrap_or_default();
    if tracking.any_active() {
        match wheel_consumer(
            true,
            host.current_mods.shift,
            app.alt_screen,
            mode_bit_on,
            app.settings.alternate_scroll_enabled,
        ) {
            WheelConsumer::ReportToApplication => {
                // AC-3: the scrollback offset is never touched and no
                // alternate-scroll arrow bytes are written for a
                // reported notch.
                let notches = wheel_report_notches(lines);
                if notches != 0 {
                    if let Some(tab) = app.active_tab() {
                        let event_kind = if notches > 0 {
                            MouseEventKind::WheelUp
                        } else {
                            MouseEventKind::WheelDown
                        };
                        let (screen_row, col) = host.pixel_to_cell(host.cursor_pos, app);
                        let col1 = col as u32 + 1;
                        let row1 = screen_row as u32 + 1;
                        let encoding = tracking.encoding();
                        let code = mouse_report::compose_button_code(
                            event_kind,
                            MouseButtonId::None,
                            encoding,
                            host.current_mods,
                        );
                        let mut buf = Vec::new();
                        for _ in 0..notches.unsigned_abs() {
                            if let Some(bytes) =
                                mouse_report::encode_report(code, col1, row1, encoding, false)
                            {
                                buf.extend_from_slice(&bytes);
                            }
                        }
                        if !buf.is_empty() {
                            tab.write_input(buf);
                        }
                    }
                }
            }
            WheelConsumer::ScrollScrollback => {
                // AC-4: Shift+wheel while tracking is active moves
                // eMterm's scrollback and writes nothing at all to the
                // PTY — no arrow bytes either, even on the alternate
                // screen with the alternate-scroll mode bit and setting
                // both on (D5). `host.alt_scroll_accum` is deliberately
                // left untouched: it feeds only the arrow-translate
                // path, which this branch can never select.
                let step = app.settings.scroll_speed.max(1);
                if lines > 0.0 {
                    app.scroll_up_by(step);
                    host.invalidate_link_hover();
                    host.window().request_redraw();
                } else if lines < 0.0 {
                    app.scroll_down_by(step);
                    host.invalidate_link_hover();
                    host.window().request_redraw();
                }
            }
            WheelConsumer::TranslateToArrows => {
                unreachable!(
                    "SC-6 (D5): the tracking-active branch never selects arrow translation"
                );
            }
        }
        return;
    }

    // Tracking-inactive: today's matrix, reproduced byte-for-byte and
    // without consulting Shift at all (AC7) — this is the pre-existing
    // mechanism SC-6's inactive branch's contract text points back to
    // ("today's matrix, reproduced exactly"), including its own stateful
    // sub-notch accumulation (`host.alt_scroll_accum`), which is why it
    // stays a single unconditional block rather than being re-expressed
    // through a fresh `WheelConsumer` match: the arrow-vs-scrollback
    // choice here depends on carried-over fractional state from earlier
    // events, not only on this event's inputs.
    //
    // FR1 accumulator: reset fractional state when not in AltScreen so
    // entering AltScreen always starts clean.
    if !app.alt_screen {
        host.alt_scroll_accum = 0.0;
    }
    let (whole, new_frac) = accumulate_alt_scroll_lines(host.alt_scroll_accum, lines);
    host.alt_scroll_accum = new_frac;
    if whole != 0.0 {
        if let Some(buf) = alternate_scroll_wheel_bytes(
            whole,
            app.alt_screen,
            mode_bit_on,
            app.settings.alternate_scroll_enabled,
        ) {
            if let Some(tab) = app.active_tab() {
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
    let step = app.settings.scroll_speed.max(1);
    if lines > 0.0 {
        app.scroll_up_by(step);
        // Scrollback content shifts under the pointer, so the
        // cached hover no longer maps to the same text. Drop
        // it; the next PointerMoved re-detects.
        host.invalidate_link_hover();
        host.window().request_redraw();
    } else if lines < 0.0 {
        app.scroll_down_by(step);
        host.invalidate_link_hover();
        host.window().request_redraw();
    }
}
