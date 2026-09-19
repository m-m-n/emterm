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
    MAX_WHEEL_REPORT_NOTCHES, accumulate_alt_scroll_lines, alternate_scroll_wheel_bytes,
    winit_button_to_report_identity, winit_to_egui_button,
};
use super::mouse_report;

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

    /// Is ANY tracking mode active.
    fn any_active(&self) -> bool {
        self.mode_1000 || self.mode_1002 || self.mode_1003
    }

    fn encoding(&self) -> mouse_report::MouseReportEncoding {
        if self.sgr {
            mouse_report::MouseReportEncoding::Sgr
        } else {
            mouse_report::MouseReportEncoding::X10
        }
    }
}

/// task0004 SC-8 (D11): collects the plain-value inputs to
/// [`mouse_report::point_belongs_to_grid`] (consulted inside SC-10, task0005)
/// from the current pointer position — the SAME hit tests the chrome guards
/// this file used to run individually (CSD title-bar band, tab-bar band,
/// bottom strip, scrollbar overlay, mux sidebar, CSD edge-resize hot zone),
/// gathered once so all three pointer paths consult the identical decision
/// (IMPLEMENTATION.md cross-task decision 3.5's sharing principle, extended
/// to SC-8).
/// `position` is the pointer's egui logical, window-relative coordinate.
fn grid_ownership_inputs(
    position: egui::Pos2,
    host: &WindowHost,
    app: &App,
) -> mouse_report::GridOwnershipInputs {
    // task0001 (FR9): the combined top-area flag is split into two
    // independent region flags at the same boundary the tab-bar wheel
    // guard in `handle_mouse_wheel` already uses — `effective_tab_bar_height`
    // is zero when the tab bar is hidden, so the tab-bar band collapses to
    // empty and the two flags can never both be true for one position.
    let title_bar_h = crate::ui::title_bar::TITLE_BAR_HEIGHT;
    let top_strip_h = title_bar_h + crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar);
    let in_title_bar_band = position.y < title_bar_h;
    let in_tab_bar_band = position.y >= title_bar_h && position.y < top_strip_h;
    let window_size_logical = host
        .window
        .surface_size()
        .to_logical::<f32>(host.pixels_per_point as f64);
    let bottom_strip_top = window_size_logical.height - host.status_bar_bot_inset_logical;
    let in_bottom_strip =
        host.status_bar_bot_inset_logical > 0.0 && position.y >= bottom_strip_top;
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
    let in_scrollbar_overlay = scrollbar_visible
        && position.x >= central_right - crate::ui::scrollbar::TRACK_W
        && position.x < central_right;
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
    let in_mux_sidebar = crate::ui::mux_sidebar::point_in_sidebar(
        position,
        visible_placement,
        egui::vec2(window_size_logical.width, window_size_logical.height),
        top_chrome,
        host.status_bar_bot_inset_logical,
    );
    let in_resize_hot_zone = host.resize_direction_at(position.x, position.y).is_some();
    mouse_report::GridOwnershipInputs {
        in_title_bar_band,
        in_tab_bar_band,
        in_bottom_strip,
        in_scrollbar_overlay,
        in_mux_sidebar,
        in_resize_hot_zone,
        profile_selector_visible: app.profile_selector.visible,
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
/// selection-drag extension, and mouse reporting (D12: gather / decide /
/// apply / perform, over task0005's SC-10/SC-11 seam).
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

    // ── Mouse reporting (D12: gather / decide / apply / perform) ───────
    // Gather: plain values only, out of the host / app / core mode state —
    // no branch choosing between reporting and local handling, no record
    // mutation. Decided independently of the local selection-drag path
    // above: with no tracking mode active, over a guarded region, or with
    // the currently-held gesture owned locally, SC-10/SC-11 (task0005)
    // name no report and today's drag-to-select behaviour above is
    // untouched.
    if let Some(tab) = app.active_tab() {
        let tracking = TrackingState::read(&tab.core.lock());
        let (screen_row, col) = host.pixel_to_cell(position, app);
        let col1 = col as u32 + 1;
        let row1 = screen_row as u32 + 1;
        let inputs = mouse_report::MotionEventInputs {
            grid: grid_ownership_inputs(egui_pos, host, app),
            mods: host.current_mods,
            mode_1000: tracking.mode_1000,
            mode_1002: tracking.mode_1002,
            mode_1003: tracking.mode_1003,
            encoding: tracking.encoding(),
            active_tab: app.active,
            held_left: host.mouse_report_held.left,
            held_middle: host.mouse_report_held.middle,
            held_right: host.mouse_report_held.right,
            column: col1,
            row: row1,
            records: host.mouse_report_records(),
        };
        // Decide (SC-10).
        let outcome = mouse_report::decide_motion_event(inputs);
        // Execute (SC-11) + perform: motion never names a local arm (it
        // is either a report or nothing), so writing any report bytes to
        // the tab the outcome names is all there is to do here.
        let mut records = host.mouse_report_records();
        let mut dest = Vec::new();
        mouse_report::apply_outcome(outcome, &mut records, &mut dest);
        host.set_mouse_report_records(records);
        for (tab_id, bytes) in dest {
            if let Some(tab) = app.tabs.get(tab_id) {
                tab.write_input(bytes);
            }
        }
    }
}

/// Perform [`mouse_report::LocalArm::BeginSelectionDrag`]: classify the
/// press (single / double / triple click), start or commit the terminal
/// selection accordingly, and arm the drag flag. Unchanged from the
/// pre-task0006 `(MouseButton::Left, ElementState::Pressed)` local arm,
/// minus the Ctrl+link-open check now named as its own local arm (see
/// [`perform_open_hovered_link`]).
fn begin_selection_drag(host: &mut WindowHost, app: &mut App) {
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

/// Perform [`mouse_report::LocalArm::OpenHoveredLink`]: re-detect and open
/// the link under the pointer, falling back to
/// [`begin_selection_drag`] when the fresh, click-time detection
/// disagrees with the cached hover snapshot the gather step named this
/// arm from (`try_open_link_at_pointer` always re-runs detection against
/// the live grid rather than trusting the hover cache, exactly as it did
/// before this task).
fn perform_open_hovered_link(host: &mut WindowHost, app: &mut App) {
    if !host.try_open_link_at_pointer(app) {
        begin_selection_drag(host, app);
    }
}

/// Perform [`mouse_report::LocalArm::CompleteSelectionAndPublishToPrimary`]:
/// clear the drag flag, consume the pending single-click anchor (handling
/// the fold-click toggle for a plain click), and publish a completed
/// selection to PRIMARY (and CLIPBOARD when `copy_on_select` is on).
/// Unchanged from the pre-task0006
/// `(MouseButton::Left, ElementState::Released)` local arm.
fn complete_selection_and_publish_to_primary(host: &mut WindowHost, app: &mut App) {
    host.dragging = false;
    // A press with no motion in Character mode left
    // selection == None (see `begin_selection_drag`);
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

/// `WindowEvent::PointerButton` arm body: CSD edge-resize handoff, egui
/// click forwarding, the strip / scrollbar / sidebar / profile-selector
/// press guards (unchanged; left-press-only or unconditional, exactly as
/// before this task), and mouse reporting (D12: gather / decide / apply /
/// perform, over task0005's SC-10/SC-11 seam) for whichever press/release
/// survives them. A release never reaches the guards at all (D12): its
/// disposition is decided from the recorded gesture owner alone, position-
/// independent.
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
    // terminal selection. This runs ahead of the mouse-
    // reporting decision below so a report-owned release
    // still balances the press egui already saw.
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
    // held, independent of any outcome below that may go on to consume
    // this event locally — a button held while the pointer later drags
    // into the terminal must still be reportable by the motion gate.
    if let Some(identity) = winit_button_to_report_identity(button) {
        let held = state == ElementState::Pressed;
        match identity {
            mouse_report::MouseButtonId::Left => host.mouse_report_held.left = held,
            mouse_report::MouseButtonId::Middle => host.mouse_report_held.middle = held,
            mouse_report::MouseButtonId::Right => host.mouse_report_held.right = held,
            mouse_report::MouseButtonId::None => {}
        }
    }
    host.window().request_redraw();

    // ── Gesture-ownership release short-circuit (D12) ──────────────────
    // A release is decided by SC-10 from the recorded gesture owner (SC-9)
    // alone, never by the release position — so it must run before every
    // chrome guard below gets a vote (Test Notes: a press inside the grid
    // whose release arrives over a guarded region still gets its release,
    // and D12's own correction: a Report-owned release must not be
    // strandable behind a position check). The egui forward and the
    // held-button bookkeeping above have already run for this event.
    // `winit_button_to_report_identity` returning `None` (a side button
    // with no DEC mouse-reporting encoding) means there is nothing for
    // SC-10 to decide and no local arm this handler recognizes either —
    // matching the pre-task0006 bottom match's `_ => {}` fallback, so an
    // unmapped-button release returns here too with no observable effect.
    if state == ElementState::Released {
        if let Some(identity) = winit_button_to_report_identity(button) {
            run_button_decision(
                mouse_report::MouseEventKind::Release,
                identity,
                egui_pos,
                host,
                app,
            );
        }
        return;
    }

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
    // (handled by the release short-circuit above) when the
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
        // AC-1/AC-4 (task0011): query the SAME shared hit-region helper
        // the MouseWheel guard uses (IMPLEMENTATION.md cross-task
        // decision 3.5), instead of the persistent-only width test above
        // — that test's inset is 0 for the overlay placement, so a press
        // on the floating overlay card used to fall through this guard
        // and start a terminal selection on the cell underneath it.
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

    // ── Mouse reporting (D12: gather / decide / apply / perform) ───────
    // Every chrome guard above has already had its chance to consume a
    // LEFT press and return; SC-8 (inside SC-10 below) is the identity-
    // independent counterpart that also covers a middle/right press and
    // the CSD edge-resize hot zone, neither of which any guard above
    // tests. `winit_button_to_report_identity` returning `None` means
    // there is nothing for SC-10 to decide and no local arm this handler
    // recognizes either (matching the bottom match's old `_ => {}`).
    if let Some(identity) = winit_button_to_report_identity(button) {
        run_button_decision(
            mouse_report::MouseEventKind::Press,
            identity,
            egui_pos,
            host,
            app,
        );
    }
}

/// Gather / decide / apply / perform for one button press or release
/// (D12), over task0005's SC-10/SC-11 seam.
fn run_button_decision(
    kind: mouse_report::MouseEventKind,
    identity: mouse_report::MouseButtonId,
    egui_pos: egui::Pos2,
    host: &mut WindowHost,
    app: &mut App,
) {
    // Gather: plain values only — no branch choosing between reporting
    // and local handling, no record mutation.
    let tracking = app
        .active_tab()
        .map(|tab| TrackingState::read(&tab.core.lock()))
        .unwrap_or_default();
    let (screen_row, col) = host.pixel_to_cell(host.cursor_pos, app);
    let col1 = col as u32 + 1;
    let row1 = screen_row as u32 + 1;
    // Press-only inputs (SC-10 ignores them on a release): whether a link
    // is hovered at the pointer, from the same cache
    // `refresh_link_hover` maintains on every `PointerMoved` — the
    // perform step's `OpenHoveredLink` arm still re-detects fresh at
    // click time via `try_open_link_at_pointer` before actually opening
    // anything, so a stale cache here can only under-name this arm
    // (falling back to `BeginSelectionDrag`), never mis-open a link.
    let hovered_link = !host.hover.link_cells.is_empty();
    let middle_click_paste_enabled = app.settings.middle_click_paste;
    let inputs = mouse_report::ButtonEventInputs {
        kind,
        button: identity,
        grid: grid_ownership_inputs(egui_pos, host, app),
        mods: host.current_mods,
        mode_1000: tracking.mode_1000,
        mode_1002: tracking.mode_1002,
        mode_1003: tracking.mode_1003,
        encoding: tracking.encoding(),
        active_tab: app.active,
        column: col1,
        row: row1,
        hovered_link,
        middle_click_paste_enabled,
        records: host.mouse_report_records(),
    };
    // Decide (SC-10).
    let outcome = mouse_report::decide_button_event(inputs);
    let local_arm = match &outcome.disposition {
        mouse_report::Disposition::Local(arm) => Some(*arm),
        _ => None,
    };
    // Execute (SC-11): apply the record updates and collect any report
    // bytes, keyed by the tab identifier the outcome names (a release
    // targets the tab its press recorded, never necessarily `app.active`).
    let mut records = host.mouse_report_records();
    let mut dest = Vec::new();
    mouse_report::apply_outcome(outcome, &mut records, &mut dest);
    host.set_mouse_report_records(records);
    for (tab_id, bytes) in dest {
        if let Some(tab) = app.tabs.get(tab_id) {
            tab.write_input(bytes);
        }
    }
    // Perform: run the local arm the outcome named (if any) through the
    // existing, unchanged local code paths.
    match local_arm {
        Some(mouse_report::LocalArm::BeginSelectionDrag) => begin_selection_drag(host, app),
        Some(mouse_report::LocalArm::OpenHoveredLink) => perform_open_hovered_link(host, app),
        Some(mouse_report::LocalArm::CompleteSelectionAndPublishToPrimary) => {
            complete_selection_and_publish_to_primary(host, app)
        }
        Some(mouse_report::LocalArm::PastePrimary) => {
            if let Some(text) = host.get_primary() {
                host.deliver_paste(app, &text);
            }
        }
        Some(other) => debug_assert!(false, "unexpected local arm for a button event: {other:?}"),
        None => {} // Report (bytes already written above) or Nothing.
    }
}

/// Run a plain scrollback scroll by `settings.scroll_speed` lines in the
/// direction `lines` names — the mechanics shared by
/// [`mouse_report::LocalArm::ScrollScrollback`] regardless of which branch
/// of SC-6's wheel matrix named it (tracking active with Shift held, or
/// tracking inactive with the alternate-scroll translation gate not fully
/// satisfied).
fn scroll_by_wheel_notch(host: &mut WindowHost, app: &mut App, lines: f32) {
    // `settings.scroll_speed` is clamped to 1..=10 by the
    // loader, so it's safe to feed directly into the scroll
    // helpers (a runaway typo can't fly the viewport 1000
    // rows per notch).
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

/// task0001 (wheel-report-notch-clamp) IMPLEMENTATION.md Shared Components,
/// "Bounded wheel-report duplication helper": produces the buffer one wheel
/// event writes to the PTY by repeating a single-notch report `payload`.
/// Pure — reads no application, tab, host or terminal state; a function of
/// its two arguments only. `requested_count` may be anywhere in the `u32`
/// range; the effective count is capped at [`MAX_WHEEL_REPORT_NOTCHES`]
/// (task0001 D2 — the duplication-side half of the report path's
/// defense-in-depth cap; see `input_translate::wheel_report_notches` for
/// the conversion-side half). That ONE capped value derives both the
/// preallocation size and the repetition bound (task0001 D5), so they can
/// never diverge. Security property, not a feel-tuning knob.
pub(super) fn bounded_wheel_report_duplicate(payload: &[u8], requested_count: u32) -> Vec<u8> {
    let capped = requested_count.min(MAX_WHEEL_REPORT_NOTCHES);
    let mut buf = Vec::with_capacity(payload.len() * capped as usize);
    for _ in 0..capped {
        buf.extend_from_slice(payload);
    }
    buf
}

/// `WindowEvent::MouseWheel` arm body: profile-selector / tab-strip /
/// mux-sidebar wheel forwarding to egui, and mouse reporting (D12: gather /
/// decide / apply / perform, over task0005's SC-10/SC-11 seam) — which
/// selects exactly one of the DECSET 1007 AltScreen arrow translation, the
/// terminal scrollback scroll, or a report to the application.
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

    // ── Mouse reporting (D12: gather / decide / apply / perform) ───────
    // Gather: plain values only. SC-8 is consulted first inside SC-10
    // (task0005), identity-independently, ahead of the tracking-active
    // read — covering the bottom strip, the scrollbar overlay and the CSD
    // edge-resize hot zone in addition to the mux-sidebar / tab-bar-band
    // guards already handled above.
    let wheel_logical = host
        .cursor_pos
        .to_logical::<f32>(host.pixels_per_point as f64);
    let tracking = app
        .active_tab()
        .map(|t| TrackingState::read(&t.core.lock()))
        .unwrap_or_default();
    let (screen_row, col) = host.pixel_to_cell(host.cursor_pos, app);
    let col1 = col as u32 + 1;
    let row1 = screen_row as u32 + 1;
    let kind = if lines >= 0.0 {
        mouse_report::MouseEventKind::WheelUp
    } else {
        mouse_report::MouseEventKind::WheelDown
    };
    let inputs = mouse_report::WheelEventInputs {
        kind,
        grid: grid_ownership_inputs(egui::pos2(wheel_logical.x, wheel_logical.y), host, app),
        mods: host.current_mods,
        mode_1000: tracking.mode_1000,
        mode_1002: tracking.mode_1002,
        mode_1003: tracking.mode_1003,
        encoding: tracking.encoding(),
        active_tab: app.active,
        column: col1,
        row: row1,
        on_alt_screen: app.alt_screen,
        alt_scroll_mode_bit: mode_bit_on,
        alt_scroll_setting: app.settings.alternate_scroll_enabled,
        records: host.mouse_report_records(),
    };
    // Decide (SC-10).
    let outcome = mouse_report::decide_wheel_event(&inputs);
    let local_arm = match &outcome.disposition {
        mouse_report::Disposition::Local(arm) => Some(*arm),
        _ => None,
    };
    // Execute (SC-11): apply the record updates. A single `decide_wheel_event`
    // call names the direction of exactly one notch; AC-2's "report bytes
    // reach the tab the outcome names" reproduces the pre-task0006
    // multi-notch loop by repeating that one notch's bytes.
    //
    // task0001 (wheel-report-fraction-accum): the repetition count no
    // longer comes from the stateless `wheel_report_notches(lines)` — it
    // comes from the report-path accumulator. Applying the outcome first
    // is load-bearing (task plan "Per-event flow" step 3 before step 4):
    // a resetting event's own delta must fold into the freshly-zeroed
    // accumulator, not the stale one. `notches`' sign, when non-zero,
    // always agrees with the `kind` used to build `bytes` above (proof:
    // the accumulator's magnitude is always below one notch, so it can
    // never flip which whole notch `lines` crosses into) — duplicating
    // `bytes` verbatim by `notches.unsigned_abs()` therefore already
    // reports the direction matching the consumed notch's sign (D5).
    //
    // task0002 (D9): the per-event report step (`apply_wheel_report_step`)
    // owns BOTH the ordering above and the gate that was missing here — it
    // applies the outcome first, then folds `lines` into the accumulator
    // and stores the fraction back only when the outcome it just applied
    // is itself a report. This handler holds no fold-and-store logic of
    // its own on this path any more.
    let mut records = host.mouse_report_records();
    let mut dest = Vec::new();
    let notches = mouse_report::apply_wheel_report_step(outcome, &mut records, &mut dest, lines);
    host.set_mouse_report_records(records);
    if notches != 0 {
        if let Some((tab_id, bytes)) = dest.into_iter().next() {
            if let Some(tab) = app.tabs.get(tab_id) {
                let buf = bounded_wheel_report_duplicate(&bytes, notches.unsigned_abs());
                tab.write_input(buf);
            }
        }
    }
    // Perform: run the local arm the outcome named.
    match local_arm {
        Some(mouse_report::LocalArm::ScrollScrollback) => {
            if tracking.any_active() {
                // D5: Shift+wheel while tracking is active moves eMterm's
                // scrollback and writes nothing at all to the PTY — no
                // arrow bytes either, even on the alternate screen with
                // the alternate-scroll mode bit and setting both on.
                // `host.alt_scroll_accum` is deliberately left untouched:
                // it feeds only the arrow-translate path, which this
                // branch can never select while tracking is active.
                scroll_by_wheel_notch(host, app, lines);
            } else {
                // Tracking inactive, but the alternate-scroll translation
                // gate (alt screen + mode bit + setting all on) is not
                // fully satisfied: today's matrix still runs the
                // fractional-accumulator bookkeeping unconditionally
                // before falling back to a plain scroll, so a later
                // mode-bit toggle mid-AltScreen-session doesn't lose the
                // carried-over fractional notch.
                if !app.alt_screen {
                    host.alt_scroll_accum = 0.0;
                }
                let (_, new_frac) = accumulate_alt_scroll_lines(host.alt_scroll_accum, lines);
                host.alt_scroll_accum = new_frac;
                scroll_by_wheel_notch(host, app, lines);
            }
        }
        Some(mouse_report::LocalArm::TranslateToArrowBytes) => {
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
            // Sub-notch delta, or the gate failed after all: fall back to
            // a plain scroll using the raw (not accumulated) `lines`
            // sign, exactly as today's matrix does.
            scroll_by_wheel_notch(host, app, lines);
        }
        Some(other) => debug_assert!(false, "unexpected local arm for a wheel event: {other:?}"),
        None => {} // Report (bytes already written above) or Nothing (SC-8 rejected).
    }
}
