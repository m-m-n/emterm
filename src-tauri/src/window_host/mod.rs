//! winit window + wgpu surface + egui integration.
//!
//! This module owns the GPU surface lifecycle and translates winit events
//! into egui inputs. The egui<->winit glue is intentionally minimal because
//! no published crate covers the winit+wgpu+egui combination as of this
//! writing (egui_winit exists but pulls in trackpad/file-drop helpers we
//! do not need).
//!
//! Phase 1 responsibilities:
//! - Create a winit window via the supplied event loop.
//! - Acquire a wgpu adapter/device and attach a surface.
//! - Recreate the surface on `SurfaceError::Lost` / `OutOfMemory`.
//! - Drive a per-frame egui pass that renders a placeholder UI.
//!
//! Phase 2 additions:
//! - Translate winit `KeyboardInput` events to PTY bytes via `pty::input`.
//! - Forward bytes to the active tab.
//! - Compute grid (cols, rows) from the window's pixel size and propagate to
//!   PTYs on resize.
//!
//! Phase 4-G redesign (2026-05-14): migrated from tao 0.34 to winit 0.30.
//! tao does not expose XKB keycodes, breaking self-built XIM. winit hosts
//! IME natively via `WindowEvent::Ime`; the Phase 4-G-3 bridge wires those
//! events into the existing Phase 4-E `on_ime_preedit / on_ime_commit /
//! on_ime_focus_lost` plumbing.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;

use egui::ViewportId;
use egui_wgpu::ScreenDescriptor;
use egui_wgpu::wgpu::SurfaceError;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::cursor::CursorIcon;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{ResizeDirection, Window, WindowAttributes, WindowId};

use crate::app::App;
use crate::ime::backend::{KeyDispatchResult, ProcessEnv, RawKeyEvent, build_backend_with_window};
use crate::mux::dialog::{MuxDialogOutcome, MuxDialogState};
use crate::mux::prefix::{KeyInput as MuxKeyInput, KeySym};
use crate::pty::input::{Modifiers, Target as EncodeTarget};
#[cfg(test)]
use crate::settings::ShiftEnterBehavior;

mod frame_pacing;
mod input_translate;

use frame_pacing::{
    FrameCounter, ResizeSettler, RowsRebuiltCounter, control_flow_for, has_actionable_egui_input,
    next_resize_settle_wake_deadline, preedit_effective_dirty_rows, record_drawn_frame,
    record_rebuilt_rows, resize_settle_self_wake_due, resolve_build_dirty_rows,
    should_rotate_row_cache_for_scroll_event, should_skip_frame, status_bar_insets_changed,
    toast_redraw_due,
};
#[cfg(test)]
use frame_pacing::{
    RESIZE_SETTLE_MAX_DURATION, RESIZE_SETTLE_QUIET_DURATION, RESIZE_SETTLE_SELF_WAKE_INTERVAL,
    next_wait_deadline,
};

#[cfg(test)]
use input_translate::MAX_ALT_SCROLL_NOTCHES;
use input_translate::{
    ShiftEnterRewrite, accumulate_alt_scroll_lines, alternate_scroll_wheel_bytes,
    input_mods_to_egui, is_skk_swallowed_chord, shift_enter_rewrite,
    should_drop_synthetic_key_event, winit_key_to_bytes, winit_key_to_egui,
    winit_physical_key_code, winit_to_egui_button,
};

/// Drive one frame of the open mux dialog: render via the UI layer
/// (`ui::mux_dialogs::draw`) and dispatch the resulting outcome into the
/// domain layer (`App::confirm_mux_*`). This is the orchestration glue
/// that previously lived in `ui::mux_dialogs::drive`; moved here so the UI
/// module no longer has to `use crate::app::App` (otherwise the UI layer
/// imports App, and App imports UI types like `TabEvent` — a cycle).
/// `window_host` already owns `App`, so dispatch lives at this boundary.
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

/// Convert an (egui::Key, current modifiers) pair from the winit event
/// pipeline into the framework-agnostic [`MuxKeyInput`] the mux prefix
/// latch consumes. Keeps the egui→domain translation pinned to this
/// single boundary site (gpt-architecture #4).
pub(super) fn egui_to_mux_input(mods: Modifiers, key: egui::Key) -> MuxKeyInput {
    let sym = match key {
        egui::Key::A => KeySym::Letter('a'),
        egui::Key::B => KeySym::Letter('b'),
        egui::Key::C => KeySym::Letter('c'),
        egui::Key::D => KeySym::Letter('d'),
        egui::Key::E => KeySym::Letter('e'),
        egui::Key::F => KeySym::Letter('f'),
        egui::Key::G => KeySym::Letter('g'),
        egui::Key::H => KeySym::Letter('h'),
        egui::Key::I => KeySym::Letter('i'),
        egui::Key::J => KeySym::Letter('j'),
        egui::Key::K => KeySym::Letter('k'),
        egui::Key::L => KeySym::Letter('l'),
        egui::Key::M => KeySym::Letter('m'),
        egui::Key::N => KeySym::Letter('n'),
        egui::Key::O => KeySym::Letter('o'),
        egui::Key::P => KeySym::Letter('p'),
        egui::Key::Q => KeySym::Letter('q'),
        egui::Key::R => KeySym::Letter('r'),
        egui::Key::S => KeySym::Letter('s'),
        egui::Key::T => KeySym::Letter('t'),
        egui::Key::U => KeySym::Letter('u'),
        egui::Key::V => KeySym::Letter('v'),
        egui::Key::W => KeySym::Letter('w'),
        egui::Key::X => KeySym::Letter('x'),
        egui::Key::Y => KeySym::Letter('y'),
        egui::Key::Z => KeySym::Letter('z'),
        egui::Key::Num0 => KeySym::Digit(0),
        egui::Key::Num1 => KeySym::Digit(1),
        egui::Key::Num2 => KeySym::Digit(2),
        egui::Key::Num3 => KeySym::Digit(3),
        egui::Key::Num4 => KeySym::Digit(4),
        egui::Key::Num5 => KeySym::Digit(5),
        egui::Key::Num6 => KeySym::Digit(6),
        egui::Key::Num7 => KeySym::Digit(7),
        egui::Key::Num8 => KeySym::Digit(8),
        egui::Key::Num9 => KeySym::Digit(9),
        egui::Key::Comma => KeySym::Comma,
        egui::Key::Period => KeySym::Period,
        egui::Key::Semicolon => KeySym::Semicolon,
        egui::Key::Slash => KeySym::Slash,
        egui::Key::Backslash => KeySym::Backslash,
        egui::Key::Minus => KeySym::Minus,
        _ => KeySym::Other,
    };
    MuxKeyInput {
        ctrl: mods.ctrl,
        shift: mods.shift,
        alt: mods.alt,
        key: sym,
    }
}
use crate::render::terminal_grid_pass::TerminalGridPass;
use crate::selection::{Pos, Selection, SelectionMode};
use crate::ui::keybinds::Chord;

/// Maximum time between successive clicks that still counts as a "multi-click".
/// Within this window the click counter increments; beyond it the counter
/// resets to 1. 500 ms matches xterm's `multiClickTime` default.
pub(super) const MULTI_CLICK_WINDOW_MS: u128 = 500;

use crate::ui::chrome::{RESIZE_EDGE_PX, classify_resize_edge, configure_egui_fonts};

/// Tracks last-click metadata so a double / triple click can be detected by
/// comparing time + position against the next press.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ClickTracker {
    last_press_at: Option<Instant>,
    /// Last press cell as `(abs_row, col)`. Using the absolute row keeps a
    /// double / triple click from spuriously matching when the viewport
    /// scrolled a different buffer line under the same screen cell between
    /// clicks.
    last_press_pos: Option<(u32, u16)>,
    /// Click counter: 1 → Character, 2 → Word, 3 → Line. After 3, the next
    /// press resets to 1.
    count: u32,
}

/// Output of the click classifier: the click count and the matching
/// selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClickClassification {
    pub(super) count: u32,
    pub(super) mode: SelectionMode,
}

impl ClickTracker {
    /// Classify a new press at absolute `(row, col)` happening at `now`. The
    /// internal state is updated for the next call.
    pub(super) fn classify(&mut self, now: Instant, row: u32, col: u16) -> ClickClassification {
        let mut count = 1u32;
        if let (Some(prev_at), Some(prev_pos)) = (self.last_press_at, self.last_press_pos) {
            let elapsed_ms = now.duration_since(prev_at).as_millis();
            if elapsed_ms <= MULTI_CLICK_WINDOW_MS && prev_pos == (row, col) && self.count < 3 {
                count = self.count + 1;
            }
        }
        self.last_press_at = Some(now);
        self.last_press_pos = Some((row, col));
        self.count = count;
        ClickClassification {
            count,
            mode: match count {
                1 => SelectionMode::Character,
                2 => SelectionMode::Word,
                _ => SelectionMode::Line, // 3 or more
            },
        }
    }
}

/// Owns the window, the wgpu surface, the egui context, and the
/// egui-wgpu renderer.
///
/// Field declaration order is also the drop order — wgpu resources that
/// depend on the surface / device / window are declared first so they
/// run their destructors before the underlying `Surface`, `Instance`,
/// and `Window` go away. In particular: the `Surface<'static>` we
/// constructed via `create_surface_unsafe` from `&Window` references the
/// window's native handle; tearing the window down before the surface
/// produces a use-after-free on the Vulkan WSI side and was the cause
/// of the segfault observed when closing the title-bar X button. See
/// `Drop` impl below for the explicit shutdown handshake.
pub struct WindowHost {
    /// Phase 4-H (font-swash-migration FR12): custom wgpu pass that
    /// draws terminal cells (foreground glyph + background fill +
    /// underline / strikethrough). Constructed lazily once the App is
    /// available so the font stack can be taken from `App::font_*`.
    /// Frame draw order is `clear → TerminalGridPass → egui (LoadOp::Load)`.
    grid_pass: Option<TerminalGridPass>,
    egui_renderer: egui_wgpu::Renderer,
    egui_ctx: egui::Context,
    /// Construction instant, fed to egui as `RawInput::time` every frame so
    /// egui's clock advances in real time. With `time: None` egui substitutes
    /// `previous_time + predicted_dt` (a fixed 1/60 s per pass), turning its
    /// clock into a frame counter: anything scheduled against it — the
    /// restart/SFTP toast auto-dismiss in `App::pump_sftp`, double-click
    /// detection — then runs fast whenever frames outpace 60 Hz (Mailbox
    /// present never blocks) and stalls when frames are skipped.
    egui_start: Instant,
    /// Last time `about_to_wait` requested a redraw on behalf of an active
    /// toast. Rate-limits that request to the `TOAST_POLL_MS` cadence:
    /// an unconditional request would re-enter `RedrawRequested` immediately
    /// (never reaching the `WaitUntil` timer), and with a non-blocking
    /// present mode the loop then spins at full speed for the toast's
    /// entire lifetime.
    last_toast_redraw: Option<Instant>,
    surface_config: wgpu::SurfaceConfiguration,
    queue: wgpu::Queue,
    device: wgpu::Device,
    surface: wgpu::Surface<'static>,
    instance: wgpu::Instance,
    window: Arc<dyn Window>,
    pixels_per_point: f32,
    /// True when the surface must be recreated on the next frame (e.g. after
    /// `SurfaceError::Lost`).
    surface_dirty: bool,
    /// Alacritty-style deferred resize: `WindowEvent::SurfaceResized` only flips
    /// this flag and requests a redraw; the next `render()` call reads the
    /// current `window.surface_size()` once and runs `surface.configure()` +
    /// `app.set_grid_size()` together. This coalesces bursts of compositor
    /// resize events (one configure per frame instead of one per event) and
    /// avoids back-buffer locking when configure and draw happen out of
    /// order on Wayland / X11. See
    /// `wezterm/window/src/os/x11/window.rs:298` (coalesce) and
    /// `alacritty/src/display/mod.rs:739` (defer-to-render).
    pending_resize: bool,
    /// Cached status-bar panel insets in egui logical points, refreshed
    /// each frame from `App::status_bar_view_model`. Drawing / pointer-
    /// routing values ONLY (task0005 D-D sharpening, findings
    /// `0029db1c89ab226f` / `5b2f22c5a14f7364`) — [`Self::grid_size`]
    /// must NOT read `status_bar_bot_inset_logical` directly; it reads
    /// [`Self::status_bar_bot_inset_settled_logical`] below instead, so a
    /// transient (not-yet-settled) height written here can never reach
    /// the PTY grid computation or the group-wide `Resize` broadcast.
    status_bar_top_inset_logical: f32,
    status_bar_bot_inset_logical: f32,
    /// task0005 (findings `0029db1c89ab226f` / `5b2f22c5a14f7364`): the
    /// bottom inset value [`Self::grid_size`] actually consumes,
    /// decoupled from `status_bar_bot_inset_logical` above. Advanced by
    /// [`resolve_grid_bot_inset`] — see that function's doc comment for
    /// the exact update rule and why it closes the defect (a
    /// non-settler `pending_resize` source, e.g. the mux-sidebar inset
    /// refresh or a compositor `Resized` / `ScaleFactorChanged`, could
    /// otherwise apply a size derived from a not-yet-settled status-bar
    /// height during the settling window).
    status_bar_bot_inset_settled_logical: f32,
    /// Cached persistent mux-sidebar horizontal grid inset in egui logical
    /// points (task0005 D2), refreshed each frame from
    /// [`App::mux_sidebar_visibility`] via [`Self::refresh_mux_sidebar_inset`].
    /// Subtracted from the usable WIDTH in [`Self::grid_size`] so the
    /// terminal grid never renders behind the right-edge persistent
    /// sidebar (task0006 update) — it does NOT touch `origin_x` in
    /// [`Self::cell_metrics_px`]; the grid's x-origin is identical with
    /// and without the sidebar. Always `0.0` in overlay mode or when the
    /// active tab is not mux-attached (NFR1: those cases must not reshape
    /// the PTY).
    mux_sidebar_inset_logical: f32,
    /// FR6 (mux-tab-switch-replay-latency task0002): debounces the
    /// status-bar-height-driven grid-size candidate computed in
    /// [`Self::refresh_status_bar_insets`] so a startup/reattach settling
    /// storm reaches [`Tab::resize`](crate::tabs::Tab::resize)'s
    /// group-wide broadcast at most once, for the settled size — see
    /// [`ResizeSettler`].
    resize_settler: ResizeSettler,
    /// Last-observed "is the active tab mux-attached" state, used to
    /// detect the `None` → `Some` `mux_session_name` transition (a fresh
    /// attach or reattach) and reopen `resize_settler`'s settling window.
    mux_was_attached: bool,
    /// Last time a resize-settle self-wake actually requested a redraw
    /// (mux-tab-switch-bypass-refix task0002 Change 1, finding
    /// `81507f39e384b34e`). Rate-limits
    /// [`Self::refresh_status_bar_insets`]'s awaiting-decision wake and
    /// `PocApp::about_to_wait`'s `WaitUntil`-driven follow-up to
    /// [`RESIZE_SETTLE_SELF_WAKE_INTERVAL`], mirroring `last_toast_redraw`'s
    /// role for `toast_redraw_due`. `None` means no self-wake has fired yet
    /// in the currently-open settling window, so the first one fires
    /// immediately. Reset to `None` alongside `resize_settler.reset()` so a
    /// fresh settling window's first wake is never delayed by a stale
    /// timestamp from a previously-closed one.
    last_resize_settle_wake: Option<Instant>,
    current_mods: Modifiers,
    /// Last cursor position in physical pixels (updated on `PointerMoved`).
    cursor_pos: PhysicalPosition<f64>,
    /// Whether the left button is currently held — used as the gate for
    /// turning subsequent `PointerMoved` events into selection extends.
    dragging: bool,
    /// Click tracker for double / triple click detection.
    click_tracker: ClickTracker,
    /// Lazily-initialized arboard clipboard. We only fail-loud once if the
    /// platform clipboard cannot be acquired (X11 without display, etc.).
    clipboard: Option<arboard::Clipboard>,
    /// Pointer / scroll events accumulated between renders, drained by
    /// `build_raw_input` so the egui-side widgets (tab bar, status bar,
    /// future settings panel) can observe clicks / hovers / drags.
    /// Without this, `build_raw_input` ships `events: vec![]` and egui
    /// never sees pointer input even though winit already delivered it.
    pending_egui_events: Vec<egui::Event>,
    /// Pointer buttons currently held (pressed-minus-released count).
    /// While non-zero, `PointerMoved` counts as actionable input for the
    /// frame-skip veto: egui chrome drags (scrollbar thumb, tab reorder)
    /// are driven purely by motion between press and release, so skipping
    /// motion-only frames mid-drag would freeze their live tracking on an
    /// idle terminal. Reset on focus loss (the release may never arrive).
    pointer_buttons_down: u8,
    /// Set when the user clicks the CSD title-bar's `×` button. The
    /// `about_to_wait` handler picks this up and runs the same
    /// teardown handshake (drop `host` → `event_loop.exit()`) used
    /// for the last-tab-closed path, so the wgpu / X11 resources
    /// unwind in the same order regardless of the close path.
    pending_close: bool,
    /// Cached CSD resize direction under the pointer. Refreshed on
    /// every `PointerMoved` (when not selection-dragging) so the next
    /// left-press can hand the matching [`ResizeDirection`] to
    /// `Window::drag_resize_window` without re-running the hit test.
    /// `None` means the pointer is in the window interior — a press
    /// falls through to the existing selection / tab-bar handlers.
    current_resize_dir: Option<ResizeDirection>,
    /// Last cursor icon pushed to winit. Cached so [`update_resize_hint`]
    /// can skip the IPC round-trip when the icon would not change —
    /// `set_cursor` is otherwise called on every `PointerMoved`, which
    /// floods the compositor with redundant requests.
    current_cursor: CursorIcon,
    /// Link-hover state (URL / file-path auto-detection). Refreshed only
    /// when the pointer crosses into a new grid cell so the detection
    /// regex doesn't run per pixel.
    hover: HoverState,
    /// Set by [`WindowHost::refresh_link_hover`] /
    /// [`WindowHost::invalidate_link_hover`] when the hovered link
    /// cell-span actually changed (appear, move, disappear) since the
    /// last frame (task0005 AC-1, finding 63c273cd4e0d66b1). `render`
    /// consumes this once per frame and forces a full redraw so the
    /// affected rows — baked into the grid instances as the hover
    /// underline — actually rebuild in the row cache instead of the
    /// honest-dirty-set skip serving a stale cached row. Hover spans
    /// change on enter/leave-style events (not per-pixel), so a full
    /// redraw on change is cheap.
    hover_span_changed: bool,
    /// True while the pointer is inside the window. Set to `true` by
    /// `PointerMoved` (there is no `PointerEntered` handler) and to `false`
    /// by `PointerLeft`. Used to gate PTY-output re-detection in
    /// `about_to_wait`: when the pointer has left the window there is
    /// nothing to underline, so we skip the `find_link_at` work entirely.
    pointer_in_window: bool,
    /// Accumulated sub-notch wheel delta for the alternate-scroll path
    /// (DECSET 1007 / FR1). Carries fractional pixel-delta lines across
    /// events so trackpad micro-scrolls eventually resolve to one arrow
    /// key per whole-line boundary.
    alt_scroll_accum: f32,
    /// `EMTERM_RENDER_PERF=1` gate (task0002 FR6-half). Read once at
    /// construction and cached here — mirrors the `perf_log` field idiom
    /// in `render::font::cache::GlyphCache::new`.
    render_perf_enabled: bool,
    /// Frames-drawn counter, active only while `render_perf_enabled`.
    frame_counter: FrameCounter,
    /// Rows-rebuilt counter (task0003 FR6-half), active only while
    /// `render_perf_enabled`. Same env gate as `frame_counter`.
    rows_rebuilt_counter: RowsRebuiltCounter,
}

/// Cached link-hover state for the active tab's grid. Mirrors the
/// WebView build's `LinkHandler`: a detected link under the pointer gets
/// its physical cells underlined (hover-only, no Ctrl), and the pointer
/// turns into a hand cursor while Ctrl is held over a link (Ctrl is what
/// arms the click-to-open).
#[derive(Default)]
pub(super) struct HoverState {
    /// Grid cell the last detection ran for (`None` = pointer outside the
    /// grid / no detection yet). Used to skip re-running detection on
    /// sub-cell pointer motion.
    cell: Option<(u16, u16)>,
    /// Physical cell spans of the link currently under the pointer
    /// (`(row, col_start, col_end)`), empty when no link is hovered. Read
    /// by the grid pass to underline the matched cells.
    pub(super) link_cells: Vec<(u16, u16, u16)>,
    /// The detected link itself, reused by the Ctrl+click handler so the
    /// click doesn't have to re-run detection for the same cell.
    link: Option<crate::links::DetectedLink>,
    /// Cached logical-line text for the last cell that ran detection.
    /// PTY-output re-detection skips `find_link_at` when this matches the
    /// current line text — meaning the PTY changed but not on the hovered
    /// line — to avoid per-frame regex work under a stationary pointer.
    last_line_text: Option<String>,
    /// FR3 discriminator: whether the active `link` was produced by the
    /// OSC 8 hyperlink path (`true`) or by the regex URL / file-path
    /// detector (`false`). OSC 8 hits survive AltScreen and the PTY-
    /// output invalidation guard; regex hits do not. The two paths
    /// share the rest of the `HoverState` fields (`link_cells` for
    /// the underline, `link.kind` for the click dispatch) so the
    /// renderer / cursor / click branches do not need a parallel API.
    is_osc8: bool,
}

/// Terminal font family used to skin the egui `Monospace` chain
/// (status-bar text). Runtime settings flatten `font_family_primary`
/// into `font_family_fallback[0]`, so we read the first entry — empty
/// when the user did not configure one, in which case chrome falls
/// back to egui's default Hack.
fn terminal_font_family(settings: &crate::settings::Settings) -> &str {
    settings
        .font_family_fallback
        .first()
        .map(String::as_str)
        .unwrap_or("")
}

impl WindowHost {
    /// Build the window + GPU resources.
    pub fn new(
        event_loop: &dyn ActiveEventLoop,
        ui_font_family: &str,
        terminal_font_family: &str,
    ) -> Self {
        let attrs = WindowAttributes::default()
            .with_title("eMterm PoC")
            .with_decorations(false)
            .with_surface_size(LogicalSize::new(960.0, 600.0))
            .with_min_surface_size(LogicalSize::new(320.0, 200.0))
            .with_maximized(true)
            // FR2: attach the bundled app icon to the main winit window so
            // the title bar and taskbar (Windows fallbacks) render the
            // eMterm glyph. `None` from `app_icon()` is a clean no-op.
            .with_window_icon(crate::window_icon::app_icon());
        // FR5: report the canonical dock-grouping identifier (X11
        // `WM_CLASS` / Wayland `app_id`) so every window groups under one
        // `emterm` dock icon. The active event loop decides between the
        // X11 / Wayland platform-attribute builders (see `linux_wm`).
        #[cfg(target_os = "linux")]
        let attrs = crate::linux_wm::with_app_id(event_loop, attrs);
        let window: Arc<dyn Window> = Arc::from(
            event_loop
                .create_window(attrs)
                .expect("native-poc: failed to create winit window"),
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // SAFETY: `window` is kept alive in `Arc<dyn Window>` and stored
        // alongside the surface for the whole `WindowHost` lifetime.
        let surface: wgpu::Surface<'static> = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: window.display_handle().expect("display handle").as_raw(),
                    raw_window_handle: window.window_handle().expect("window handle").as_raw(),
                })
                .expect("create surface")
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .expect("native-poc: no compatible wgpu adapter found");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("native-poc-device"),
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .expect("native-poc: failed to request wgpu device");

        let size = window.surface_size();
        let surface_caps = surface.get_capabilities(&adapter);
        // Prefer a NON-sRGB surface. All of our color sources (theme
        // palette, egui paint, glyph fg/bg) are sRGB-encoded byte values;
        // we want them written to the framebuffer verbatim, matching the
        // WebView build's Canvas 2D gamma-space pipeline. An *sRGB*
        // surface would treat shader output as linear and re-encode it,
        // brightening every mid-tone on screen (and egui itself warns
        // "egui prefers Rgba8Unorm or Bgra8Unorm" for the same reason).
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        // Prefer Mailbox over Fifo so window resize doesn't lag the mouse:
        // Fifo blocks `Present` on the next vsync (≈16.7 ms at 60 Hz), so
        // each compositor resize event has to wait a full frame before the
        // window catches up. Mailbox queues the most recent submission
        // non-blocking, replacing any older queued frame, which removes the
        // vsync wall while still avoiding tearing on most desktops.
        // Fall back to Immediate (allows tearing but never blocks) and
        // finally Fifo (always supported by spec) when Mailbox is absent —
        // some Mesa drivers / proprietary stacks only expose Fifo.
        let present_mode = if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Immediate)
        {
            wgpu::PresentMode::Immediate
        } else {
            wgpu::PresentMode::Fifo
        };
        log::info!(
            "native-poc: surface present mode = {:?} (available: {:?})",
            present_mode,
            surface_caps.present_modes
        );

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            // Mailbox / Immediate keep the GPU queue shallow on their own;
            // 2 keeps Fifo's existing pipelining intact.
            desired_maximum_frame_latency: 2,
        };
        // Phase 0: defer the very first `surface.configure` to the first
        // redraw. tao reports an initial size before the surface is fully
        // ready on some Linux/Vulkan stacks, producing
        // `ERROR_SURFACE_LOST_KHR` when configure runs in `new()`. By marking
        // `surface_dirty = true` here, the first `render()` call will run
        // `reconfigure_surface()` under the same dirty/lost recovery path
        // used for in-flight surface loss, which is already covered.

        let egui_ctx = egui::Context::default();
        configure_egui_fonts(&egui_ctx, ui_font_family, terminal_font_family);
        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

        let pixels_per_point = window.scale_factor() as f32;

        let clipboard = match arboard::Clipboard::new() {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("arboard clipboard unavailable: {e}");
                None
            }
        };

        Self {
            window,
            instance,
            surface,
            surface_config,
            device,
            queue,
            egui_ctx,
            egui_renderer,
            egui_start: Instant::now(),
            last_toast_redraw: None,
            pixels_per_point,
            surface_dirty: true,
            pending_resize: false,
            status_bar_top_inset_logical: 0.0,
            status_bar_bot_inset_logical: 0.0,
            status_bar_bot_inset_settled_logical: 0.0,
            mux_sidebar_inset_logical: 0.0,
            resize_settler: ResizeSettler::new(),
            mux_was_attached: false,
            last_resize_settle_wake: None,
            current_mods: Modifiers::NONE,
            cursor_pos: PhysicalPosition::new(0.0, 0.0),
            dragging: false,
            click_tracker: ClickTracker::default(),
            clipboard,
            grid_pass: None,
            pending_egui_events: Vec::new(),
            pointer_buttons_down: 0,
            pending_close: false,
            current_resize_dir: None,
            current_cursor: CursorIcon::Default,
            hover: HoverState::default(),
            hover_span_changed: false,
            pointer_in_window: false,
            alt_scroll_accum: 0.0,
            render_perf_enabled: std::env::var("EMTERM_RENDER_PERF")
                .map(|v| v == "1")
                .unwrap_or(false),
            frame_counter: FrameCounter::default(),
            rows_rebuilt_counter: RowsRebuiltCounter::default(),
        }
    }

    /// Phase 4-H: lazily construct the `TerminalGridPass` once the App
    /// is available. Called from `PocApp::resumed` after the App's
    /// font stack has been built (`App::new` already constructs it).
    /// Idempotent — repeated calls keep the existing pass.
    pub fn ensure_grid_pass(&mut self, app: &App) {
        if self.grid_pass.is_some() {
            return;
        }
        let pass = TerminalGridPass::new(
            &self.device,
            self.surface_config.format,
            app.font_cache.clone(),
            app.font_fallback.clone(),
            app.font_rasterizer.clone(),
        );
        self.grid_pass = Some(pass);
    }

    pub fn window(&self) -> &dyn Window {
        self.window.as_ref()
    }

    /// Hand a clone of the `Arc<dyn Window>` to callers that need to retain
    /// the handle themselves (Phase 4-G-3 passes this to
    /// `WinitImeBridge::init` so the bridge can call
    /// `Window::set_ime_cursor_area`).
    pub fn window_arc(&self) -> Arc<dyn Window> {
        self.window.clone()
    }

    /// True when the CSD title bar's `×` was clicked this frame and
    /// `about_to_wait` should drive the teardown handshake.
    pub fn pending_close(&self) -> bool {
        self.pending_close
    }

    /// Translate a [`crate::ui::TitleBarEvent`] into the matching
    /// `winit::Window` action. `Close` is deferred — it just flips
    /// `pending_close` so `about_to_wait` can run the teardown +
    /// `event_loop.exit()` handshake in the same place as the
    /// last-tab-closed path.
    fn apply_title_bar_event(&mut self, evt: crate::ui::TitleBarEvent) {
        use crate::ui::TitleBarEvent;
        match evt {
            TitleBarEvent::Minimize => {
                self.window.set_minimized(true);
            }
            TitleBarEvent::MaximizeToggle => {
                let was_maximized = self.window.is_maximized();
                self.window.set_maximized(!was_maximized);
            }
            TitleBarEvent::Close => {
                self.pending_close = true;
                self.window.request_redraw();
            }
            TitleBarEvent::DragStart => {
                // X11 / Wayland-backed winit hands the move loop to
                // the WM. The Err arm covers headless / unsupported
                // backends — log and continue rather than panic.
                if let Err(e) = self.window.drag_window() {
                    log::warn!("native-poc: drag_window failed: {e}");
                }
            }
        }
    }

    /// Toggle borderless full-screen for the window. When already
    /// full-screen, restore windowed mode (`None`); otherwise enter
    /// `Borderless(None)` so winit picks the window's current monitor.
    /// The resulting `WindowEvent::SurfaceResized` drives the deferred
    /// `apply_pending_resize`, so the grid reshapes on the next frame
    /// without any extra plumbing here.
    fn toggle_fullscreen(&self) {
        use winit::monitor::Fullscreen;
        if self.window.fullscreen().is_some() {
            self.window.set_fullscreen(None);
        } else {
            self.window
                .set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
    }

    /// Classify a pointer position (in egui logical points, relative to
    /// the window's top-left) as one of the eight CSD resize directions,
    /// or `None` when the pointer is in the window interior. The window
    /// being maximized always returns `None` so a click on the title
    /// bar of a maximized window stays a move/drag gesture rather than
    /// a phantom resize against the screen edge.
    fn resize_direction_at(&self, logical_x: f32, logical_y: f32) -> Option<ResizeDirection> {
        if self.window.is_maximized() {
            return None;
        }
        let size = self
            .window
            .surface_size()
            .to_logical::<f32>(self.pixels_per_point as f64);
        let dir = classify_resize_edge(
            size.width,
            size.height,
            logical_x,
            logical_y,
            RESIZE_EDGE_PX,
        )?;
        // CSD title-bar carve-out: the title bar spans y ∈ [0,
        // TITLE_BAR_HEIGHT) and hosts the Minimize / Maximize / Close
        // buttons on its right side plus a drag-to-move affordance in
        // the middle. If we let `North`/`NE`/`NW` fire anywhere inside
        // that band, the top-right 8×8 corner would hijack the Close
        // button and the rest of the bar would lose the move gesture.
        // Restrict the top-edge resize to the outermost `RESIZE_EDGE_PX`
        // strip — past that, the title bar wins and the user gets the
        // expected drag-to-move / button-click semantics.
        let title_bar_h = crate::ui::title_bar::TITLE_BAR_HEIGHT;
        if logical_y >= RESIZE_EDGE_PX && logical_y < title_bar_h {
            use ResizeDirection::*;
            if matches!(dir, North | NorthEast | NorthWest) {
                return None;
            }
        }
        Some(dir)
    }

    /// Refresh the cached resize direction + pointer icon for a new
    /// pointer position. Cheap to call on every `PointerMoved`: skips
    /// the `set_cursor` IPC round-trip when the resulting icon would
    /// match the one already in flight.
    fn update_resize_hint(&mut self, logical_x: f32, logical_y: f32) {
        let dir = self.resize_direction_at(logical_x, logical_y);
        if dir == self.current_resize_dir {
            return;
        }
        self.current_resize_dir = dir;
        let icon = dir.map(CursorIcon::from).unwrap_or(CursorIcon::Default);
        if icon == self.current_cursor {
            return;
        }
        self.current_cursor = icon;
        self.window.set_cursor(icon.into());
    }

    /// Map a physical pixel position to a grid cell, returning `None`
    /// when the pointer is outside the terminal grid area (over the CSD
    /// title bar / tab strip, in the left/top padding, or below/right of
    /// the last cell). Unlike [`pixel_to_cell`] this does *not* clamp to
    /// the grid — a clamped row/col would make the top strip read as
    /// row 0 col 0 and falsely underline a link there. Mirrors the
    /// WebView `LinkHandler`'s `displayRow < 0 || >= rows` guard.
    fn pixel_to_grid_cell(&self, pos: PhysicalPosition<f64>, app: &App) -> Option<(u16, u16)> {
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        if cell_w <= 0.0 || cell_h <= 0.0 {
            return None;
        }
        if pos.x < origin_x || pos.y < origin_y {
            return None;
        }
        let cols = app.cell_size.cols;
        let rows = app.cell_size.rows;
        if cols == 0 || rows == 0 {
            return None;
        }
        let col = ((pos.x - origin_x) / cell_w).floor() as i64;
        let row = ((pos.y - origin_y) / cell_h).floor() as i64;
        if col < 0 || row < 0 || col >= cols as i64 || row >= rows as i64 {
            return None;
        }
        Some((row as u16, col as u16))
    }

    /// Recompute the link-hover state for the current pointer position.
    /// Runs the detection regex only when the pointer crosses into a new
    /// grid cell (or leaves the grid); requests a redraw when the
    /// underlined span changes so the renderer repaints. Also refreshes
    /// the pointer icon (hand while Ctrl is held over a link).
    ///
    /// No-op for detection when both `url_detection` and
    /// `file_path_detection` are off, but the cursor icon is still
    /// reset so a stale hand cursor doesn't linger.
    pub(super) fn refresh_link_hover(&mut self, app: &App) {
        let detect_urls = app.settings.url_detection;
        let detect_paths = app.settings.file_path_detection;

        let new_cell = self.pixel_to_grid_cell(self.cursor_pos, app);

        // Re-run detection only when the target cell changed. The cursor
        // icon (Ctrl-dependent) is refreshed every call so toggling Ctrl
        // over a held position flips the hand on/off without motion.
        if new_cell != self.hover.cell {
            self.hover.cell = new_cell;
            let prev_cells = std::mem::take(&mut self.hover.link_cells);
            self.hover.link = None;
            self.hover.last_line_text = None;
            self.hover.is_osc8 = false;

            // FR3: OSC 8 hyperlink path runs FIRST, before the regex
            // gate and the AltScreen suppression. OSC 8 hits are a
            // first-class signal from the application (the program
            // explicitly marked the cell with `ESC]8;;<uri>ESC\`) so
            // they survive AltScreen where the regex auto-detector
            // does not. When the helper returns `Some(link)`, populate
            // the same hover fields the regex path uses and skip the
            // regex; the renderer underline + Ctrl-hand cursor + click
            // dispatch all reuse the existing infrastructure.
            if let (Some((row, col)), Some(tab)) = (new_cell, app.active_tab()) {
                let core = tab.core.lock();
                if let Some(link) = detect_osc8_link_at(&core, row, col) {
                    self.hover.link_cells = link.cells.clone();
                    self.hover.link = Some(link);
                    self.hover.is_osc8 = true;
                }
            }

            if !self.hover.is_osc8 && (detect_urls || detect_paths) && !app.alt_screen {
                if let (Some((row, col)), Some(tab)) = (new_cell, app.active_tab()) {
                    let core = tab.core.lock();
                    // Cache the logical-line text so PTY-output re-detection
                    // can skip `find_link_at` when the hovered line is
                    // unchanged.
                    self.hover.last_line_text = Some(crate::links::logical_line_text(&core, row));
                    if let Some(link) =
                        crate::links::find_link_at(&core, row, col, detect_urls, detect_paths)
                    {
                        self.hover.link_cells = link.cells.clone();
                        self.hover.link = Some(link);
                    }
                }
            }

            if hover_link_cells_changed(&prev_cells, &self.hover.link_cells) {
                // task0005 AC-1: latch so `render` forces a full redraw
                // and the affected rows actually rebuild in the row
                // cache (the request_redraw below only wakes the loop —
                // it does not by itself make `dirty_rows_this_frame`
                // non-empty).
                self.hover_span_changed = true;
                self.window().request_redraw();
            }
        }

        self.update_link_cursor();
    }

    /// Re-run link detection after a PTY update *only* when the hovered
    /// logical line actually changed under a stationary pointer.
    ///
    /// Called from the event loop's `about_to_wait` when `pty_changed &&
    /// pointer_in_window && !dragging`. The staleness check (compare the
    /// current logical-line text against the cached `last_line_text`) is
    /// owned here, alongside [`refresh_link_hover`], so the
    /// `HoverState` fields (`cell` / `last_line_text`) are never touched
    /// from the event loop body — that keeps the cache-invalidation
    /// policy in one place instead of duplicated across both sites.
    ///
    /// Applies the same detection gate as [`refresh_link_hover`]
    /// (`url_detection || file_path_detection`, and not on the alt
    /// screen) before doing any per-frame work, so high-throughput PTY
    /// output under a stationary pointer skips the `logical_line_text`
    /// allocation entirely when detection is disabled.
    pub(super) fn refresh_link_hover_on_pty_change(&mut self, app: &App) {
        // Alt-screen: clear any underline cache carried over from the normal
        // screen to prevent hover-underline bleed onto alt-screen content.
        // invalidate_link_hover is a no-op when link_cells is already empty,
        // so this is cheap on frames where no link was hovered.
        //
        // FR3: OSC 8 hover state survives AltScreen — the regex
        // detector is the only consumer with the AltScreen suppression
        // policy, so the guard only fires when the active hover came
        // from the regex path (`is_osc8 == false`). An OSC 8 hit
        // re-runs `refresh_link_hover` below so the cell range stays
        // accurate when scrollback shifts under the pointer.
        if app.alt_screen && !self.hover.is_osc8 {
            self.invalidate_link_hover();
            return;
        }

        let detect_urls = app.settings.url_detection;
        let detect_paths = app.settings.file_path_detection;
        // FR3: when the cached hover IS an OSC 8 hit, keep re-running
        // refresh_link_hover even if regex detection is off — OSC 8
        // doesn't go through the regex gate.
        if !self.hover.is_osc8 && !detect_urls && !detect_paths {
            return;
        }

        // Content-change guard: fetch the current logical-line text first.
        // If it matches the cache, the hovered line is unchanged and
        // `find_link_at` can be skipped entirely (avoiding the per-frame
        // alloc + regex during high-throughput output like `tail -f` or a
        // build log). Only when the text actually changed do we clear
        // `hover.cell` and let `refresh_link_hover` re-run detection.
        //
        // For OSC 8 hits the cached `last_line_text` is `None` (the
        // OSC 8 path doesn't populate it), so the comparison below
        // always falls through to the re-detection branch — that is
        // intentional: cheap to redo the cell read; correct when the
        // OSC 8 run shifts because content scrolled.
        if let Some((row, _col)) = self.hover.cell {
            let current_text = match app.active_tab() {
                Some(tab) => {
                    let core = tab.core.lock();
                    crate::links::logical_line_text(&core, row)
                }
                None => return,
            };
            let cached = self.hover.last_line_text.as_deref().unwrap_or("");
            if !self.hover.is_osc8 && current_text == cached {
                // Hovered line text is unchanged; existing hover state
                // (underline + link) is still valid.
                return;
            }
            // Line changed (or OSC 8 hover that always re-detects):
            // drop the cached cell so refresh re-detects.
            self.hover.cell = None;
            self.refresh_link_hover(app);
        } else {
            // No previously-hovered cell: let refresh_link_hover resolve
            // whether the pointer is now over a detectable cell.
            self.hover.cell = None;
            self.refresh_link_hover(app);
        }
    }

    /// Set the pointer icon by precedence: an active CSD resize zone wins,
    /// then a hand cursor when Ctrl is held over a detected link, else the
    /// default arrow. Skips the `set_cursor` IPC when the icon is
    /// unchanged. The hand is gated on Ctrl to match the WebView build,
    /// where Ctrl/Meta arms the click-to-open.
    pub(super) fn update_link_cursor(&mut self) {
        let icon = if self.current_resize_dir.is_some() {
            self.current_cursor // leave resize-hint icon untouched
        } else if self.current_mods.ctrl && self.hover.link.is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        };
        if icon != self.current_cursor {
            self.current_cursor = icon;
            self.window.set_cursor(icon.into());
        }
    }

    /// Drop any cached link-hover state and clear a hand cursor. Called
    /// when the grid content shifts under the pointer (scroll) or the
    /// pointer leaves the window, so a stale underline / cursor doesn't
    /// survive. Requests a redraw when an underline was showing.
    pub(super) fn invalidate_link_hover(&mut self) {
        self.hover.cell = None;
        self.hover.link = None;
        self.hover.last_line_text = None;
        self.hover.is_osc8 = false;
        let had = !self.hover.link_cells.is_empty();
        self.hover.link_cells.clear();
        if had {
            // task0005 AC-1: same latch as `refresh_link_hover` above —
            // a span disappearing needs its rows rebuilt just as much as
            // one appearing or moving.
            self.hover_span_changed = true;
            self.window().request_redraw();
        }
        self.update_link_cursor();
    }

    /// Open the link under the pointer (URL via the OS opener, file path
    /// via the configured editor). Returns `true` when a link was found
    /// at the click cell (so the caller skips starting a selection),
    /// regardless of whether the spawn ultimately succeeded — a blocked
    /// scheme or missing file is still "the user clicked a link".
    ///
    /// Always re-runs detection against the live grid at click time so
    /// the result reflects the current terminal content regardless of
    /// hover-cache staleness (terminal output, tab switches, alt-screen
    /// transitions, or settings changes since the last pointer-move can
    /// all mutate the visible content under a stationary pointer).
    pub(super) fn try_open_link_at_pointer(&mut self, app: &App) -> bool {
        let detect_urls = app.settings.url_detection;
        let detect_paths = app.settings.file_path_detection;
        let click_cell = self.pixel_to_grid_cell(self.cursor_pos, app);
        let Some((row, col)) = click_cell else {
            return false;
        };

        // FR3: OSC 8 lookup runs FIRST and is NOT gated on
        // `url_detection` / `file_path_detection` (those settings
        // control the regex auto-detector only — OSC 8 is an explicit
        // application-supplied signal). It also bypasses the AltScreen
        // short-circuit so PR-ID links inside Claude Code's AltScreen
        // are clickable. Dispatch via the same `LinkKind::Url ->
        // open_url` arm the regex hit uses.
        if let Some(tab) = app.active_tab() {
            let core = tab.core.lock();
            if let Some(link) = detect_osc8_link_at(&core, row, col) {
                drop(core);
                if let crate::links::LinkKind::Url(url) = link.kind {
                    if crate::links::is_safe_uri(&url) {
                        open_url(&url);
                    } else {
                        log::warn!("native-poc: refusing to open unsafe OSC 8 URI scheme: {url}");
                    }
                }
                return true;
            }
        }

        if !detect_urls && !detect_paths {
            return false;
        }
        // Guard against alt-screen: mirrors the same condition applied in
        // `refresh_link_hover` so hover and click use identical detection
        // rules. (OSC 8 already returned above for AltScreen cases.)
        if app.alt_screen {
            return false;
        }

        // Always re-detect from the live grid; clicks are infrequent so
        // the regex cost is negligible, and this prevents acting on a
        // stale DetectedLink from the hover cache.
        let link = if let Some(tab) = app.active_tab() {
            let core = tab.core.lock();
            crate::links::find_link_at(&core, row, col, detect_urls, detect_paths)
        } else {
            None
        };

        let Some(link) = link else {
            return false;
        };

        match link.kind {
            crate::links::LinkKind::Url(url) => {
                if crate::links::is_safe_uri(&url) {
                    open_url(&url);
                } else {
                    log::warn!("native-poc: refusing to open unsafe URI scheme: {url}");
                }
            }
            crate::links::LinkKind::FilePath { path, line, col } => {
                self.open_file_in_editor(app, &path, line, col);
            }
        }
        true
    }

    /// Resolve `file_path` against the active tab's OSC 7 CWD, verify the
    /// file exists, then spawn `settings.editor_command` with `{file}` /
    /// `{line}` / `{col}` expanded. Mirrors `openFileInEditor` in the
    /// WebView build's `link.ts`: existence is checked only at click time
    /// (not on hover), a relative path with no CWD passes through as-is
    /// (SPEC FR6), and a blank editor command / absent file is a logged
    /// no-op.
    fn open_file_in_editor(&self, app: &App, file_path: &str, line: u32, col: u32) {
        let editor = app.settings.editor_command.trim();
        if editor.is_empty() {
            log::warn!("native-poc: editor_command is blank; not opening {file_path}");
            return;
        }
        let cwd = app.active_tab().and_then(|t| t.cb_state.lock().cwd.clone());
        // SPEC FR6: with no CWD a relative path passes through as-is; the
        // is_file() check below then decides whether anything opens.
        let resolved = crate::links::resolve_path(file_path, cwd.as_deref());
        if !std::path::Path::new(&resolved).is_file() {
            log::warn!("native-poc: file not found, not opening: {resolved}");
            app.notify("ファイルが見つかりません", &resolved);
            return;
        }
        // Canonicalize to an absolute path so a leading-dash path (e.g.
        // `-S/tmp/x.vim:1`) cannot be passed as an option to the editor.
        let canonical = match std::fs::canonicalize(&resolved) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("native-poc: canonicalize failed for {resolved}: {e}");
                app.notify("ファイルが見つかりません", &resolved);
                return;
            }
        };
        let canonical_str = canonical.to_string_lossy();
        let Some((program, args)) = crate::links::build_editor_command(
            &app.settings.editor_command,
            &canonical_str,
            line,
            col,
        ) else {
            log::warn!("native-poc: editor_command produced no program; not opening");
            return;
        };
        match std::process::Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                log::info!("native-poc: opened {canonical_str} in editor ({program})");
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => {
                log::warn!("native-poc: failed to spawn editor {program}: {e}");
                app.notify("エディタの起動に失敗しました", &e.to_string());
            }
        }
    }

    /// Reconfigure the wgpu surface for the current window size.
    fn reconfigure_surface(&mut self) {
        let size = self.window.surface_size();
        self.surface_config.width = size.width.max(1);
        self.surface_config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
        self.surface_dirty = false;
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

    /// Mark the surface as needing a reconfigure on the next render.
    ///
    /// Alacritty-style deferred resize: the caller (winit `Resized` /
    /// `ScaleFactorChanged` handlers) only flips the flag and requests a
    /// redraw. The actual `surface.configure()` + PTY grid resize happens
    /// once in [`apply_pending_resize`] at the head of [`render`].
    pub fn request_resize(&mut self) {
        self.pending_resize = true;
    }

    /// Consume `pending_resize` and apply the latest `window.surface_size()`.
    ///
    /// Called once per `render()` so a burst of compositor resize events
    /// produces a single configure + PTY resize cycle aligned with the
    /// frame boundary. Zero-sized windows (Windows minimize, Wayland hidden)
    /// just clear the flag without reconfiguring.
    ///
    /// task0005 (findings `0029db1c89ab226f` / `5b2f22c5a14f7364`):
    /// `pending_resize` can be set by sources OTHER than
    /// [`ResizeSettler`]'s own forwarded decision — the mux-sidebar inset
    /// refresh, or a compositor `Resized` / `ScaleFactorChanged` — so this
    /// method must not assume the status-bar height has settled just
    /// because it is running. [`Self::grid_size`] enforces that: it reads
    /// [`Self::status_bar_bot_inset_settled_logical`], never the immediate
    /// `status_bar_bot_inset_logical`, so whichever source triggered this
    /// call, the size actually applied and broadcast is always derived
    /// from a settler-forwarded inset.
    pub(super) fn apply_pending_resize(&mut self, app: &mut App) {
        if !self.pending_resize {
            return;
        }
        self.pending_resize = false;
        let size = self.window.surface_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
        let (cols, rows) = self.grid_size(app);
        app.set_grid_size(cols, rows);
    }

    /// Refresh `status_bar_*_inset_logical` from the active view model.
    /// Called at the head of each `render()` (before
    /// [`apply_pending_resize`]) so the PTY row count and the grid
    /// origin stay in sync with the currently-rendered status-bar
    /// panel. Routes every computed candidate grid size through
    /// [`ResizeSettler`] rather than applying it immediately (FR6,
    /// mux-tab-switch-replay-latency task0002) — a startup/reattach
    /// settling storm otherwise flags a pending resize (and, once
    /// applied, broadcasts a group-wide `Resize`) once per transient
    /// transition instead of once for the settled size.
    ///
    /// task0005: the candidate is fed to the settler UNCONDITIONALLY on
    /// every render, even when it matches the currently-applied inset —
    /// filtering that upstream used to starve the settler of whichever
    /// side of a 2-state oscillation happened to match the still-applied
    /// value, biasing quiescence detection (finding 02546e5e10deb500-c).
    /// `ResizeSettler` itself now decides whether a candidate is a no-op
    /// repeat.
    ///
    /// mux-tab-switch-bypass-refix task0002 D-D: the drawing insets
    /// (below) and the PTY reshape trigger (`pending_resize`) are two
    /// separate concerns from here on. Change 2 (findings
    /// `a82206113b8160fd` / `aba5ebbdf9a9addb`) applies
    /// `status_bar_*_inset_logical` whenever the VALUES themselves change
    /// ([`status_bar_insets_changed`]), independent of whether
    /// `ResizeSettler::observe` forwards a grid-size decision this frame —
    /// a status-bar height change whose derived `(cols, rows)` candidate
    /// happens to be unchanged (cell height above `ROW_HEIGHT` at larger
    /// font sizes, or row clamping) previously left the insets stale
    /// forever, permanently mis-routing mux-sidebar pointer input which
    /// reads the same fields. No extra repaint is requested for this: the
    /// height change that drives a new inset value always changes
    /// `App::status_bar_view_model`, which `render()`'s `should_skip_frame`
    /// check already treats as a mandatory-paint signal via
    /// `status_bar_changed`, so the fresh inset lands in the very frame
    /// that computed it. `pending_resize` stays tied to the settler's
    /// forwarded decision only (unchanged from before this task), so an
    /// inset-only change never re-triggers FR6's reshape-storm guard.
    ///
    /// Change 1 (finding `81507f39e384b34e`): while the settler is still
    /// debouncing ([`ResizeSettler::awaiting_decision`]), this requests
    /// another redraw so a fully idle window does not strand a pending
    /// settle indefinitely (findings 02546e5e10deb500 /
    /// 5b1878c41d3e02d6-perf-P2) — but now rate-limited via
    /// [`resize_settle_self_wake_due`] instead of firing unconditionally
    /// on every frame; see that function's doc comment and
    /// [`RESIZE_SETTLE_SELF_WAKE_INTERVAL`] for why an unconditional wake
    /// spins the render loop at full speed, and
    /// [`next_resize_settle_wake_deadline`] / `PocApp::about_to_wait` for
    /// how the rate-limited wake still keeps arriving with zero other
    /// activity in the window.
    ///
    /// task0005 round-1 rework (findings `0029db1c89ab226f` /
    /// `5b2f22c5a14f7364`): D-D above split WHICH EVENTS set
    /// `pending_resize` from the settler, but left `grid_size()` reading
    /// `status_bar_bot_inset_logical` — the same field this method now
    /// writes immediately, every render, regardless of settler state. The
    /// traced firing order this closes: a fresh mux attach/reattach
    /// resets the settler (above) and this method writes the transient,
    /// first-frame status-bar height into `status_bar_bot_inset_logical`
    /// in the SAME call; `refresh_mux_sidebar_inset` (called next, in
    /// `render`) then raises `pending_resize` on its own, unrelated
    /// inset change; `apply_pending_resize` (called after that) used to
    /// compute the PTY grid from whatever inset was currently sitting in
    /// `status_bar_bot_inset_logical` — the just-written transient value,
    /// not anything the settler had judged stable. Sharpening the D-D
    /// boundary: drawing and pointer-routing insets stay immediate
    /// (unchanged, right above); grid computation and the `Resize`
    /// broadcast now consume ONLY [`Self::status_bar_bot_inset_settled_
    /// logical`], advanced below via [`resolve_grid_bot_inset`] exactly
    /// when the settler is not withholding judgment — never on a bare
    /// `pending_resize` flip from a non-settler source.
    pub(super) fn refresh_status_bar_insets(&mut self, app: &App) {
        // FR5 (mux-status-bar-removal task0001): `vm` — and therefore
        // `height`, the bottom inset, and the grid-size candidate computed
        // below — is a pure function of `app.settings.statusbar` and the
        // OSC `777;statusbar` dispatcher's own state. `App::status_bar_view_model`
        // takes no mux-attach input at all, so attaching/detaching a mux
        // session cannot change the row count or grid size this method
        // derives (see `status_bar/runtime.rs::build_view_model`).
        let vm = app.status_bar_view_model();
        let height = crate::ui::status_bar::panel_height_logical(&vm);
        // The status bar is fixed at the bottom; reserve its height there.
        let (top, bot) = (0.0, height);

        // A fresh mux attach/reattach reopens the settling window (unrelated
        // to the status-bar row count above): `mux_session_name` transitioning
        // from absent to present is the same `first_welcome` moment
        // `Tab::apply_mux_message` uses to seed the mux group and push its
        // own one-time Resize — that seed does not go through
        // `ResizeSettler` at all, so this reset only affects how quickly a
        // SEPARATE, unrelated settle (e.g. a concurrent window resize)
        // converges around the same moment.
        let mux_attached = app
            .active_tab()
            .is_some_and(|t| t.mux_session_name.is_some());
        if mux_attached && !self.mux_was_attached {
            self.resize_settler.reset();
            // A fresh settling window's first self-wake must fire
            // immediately, not be delayed by a stale timestamp left over
            // from a previously-closed window (see `last_resize_settle_
            // wake`'s doc comment).
            self.last_resize_settle_wake = None;
        }
        self.mux_was_attached = mux_attached;

        // Change 2: apply the inset values independent of the settler
        // below (D-D) — see this method's doc comment.
        if status_bar_insets_changed(
            self.status_bar_top_inset_logical,
            self.status_bar_bot_inset_logical,
            top,
            bot,
        ) {
            self.status_bar_top_inset_logical = top;
            self.status_bar_bot_inset_logical = bot;
        }

        let candidate = self.grid_size_for_bot_inset(app, bot);
        let now = Instant::now();
        let forwarded = self.resize_settler.observe(candidate, now).is_some();
        // task0005 (findings `0029db1c89ab226f` / `5b2f22c5a14f7364`): the
        // settled bot inset [`Self::grid_size`] consumes only ever tracks
        // `bot` when the settler is not mid-settle, evaluated AFTER this
        // render's `observe` call above — see `resolve_grid_bot_inset`'s
        // doc comment for the exact rule.
        self.status_bar_bot_inset_settled_logical = resolve_grid_bot_inset(
            self.status_bar_bot_inset_settled_logical,
            bot,
            self.resize_settler.awaiting_decision(),
        );
        if forwarded {
            self.pending_resize = true;
        } else if resize_settle_self_wake_due(
            self.resize_settler.awaiting_decision(),
            self.last_resize_settle_wake,
            now,
        ) {
            self.last_resize_settle_wake = Some(now);
            self.window().request_redraw();
        }
    }

    /// Refresh `mux_sidebar_inset_logical` from [`App::mux_sidebar_visibility`]
    /// (task0005 D2). Called at the head of each `render()` alongside
    /// [`Self::refresh_status_bar_insets`], mirroring its change-detection /
    /// `pending_resize` flip so the PTY reshapes exactly once when the inset
    /// actually moves — entering/leaving a mux-attached tab in persistent
    /// mode, or flipping `mux.window_sidebar_overlay` (NFR1). Overlay
    /// open/close never changes this value (overlay always resolves to 0
    /// inset — [`crate::app::mux_sidebar_grid_inset`]), so toggling it never
    /// touches `pending_resize` here.
    pub(super) fn refresh_mux_sidebar_inset(&mut self, app: &App) {
        let scale = self.pixels_per_point.max(1.0) as f64;
        let window_width_logical = (self.surface_config.width.max(1) as f64 / scale) as f32;
        let inset =
            crate::app::mux_sidebar_grid_inset(app.mux_sidebar_visibility(), window_width_logical);
        if (self.mux_sidebar_inset_logical - inset).abs() > f32::EPSILON {
            self.mux_sidebar_inset_logical = inset;
            self.pending_resize = true;
        }
    }

    /// Cell metrics in **physical pixels**, matching what
    /// `TerminalGridPass::prepare` is fed (see render path:
    /// `app.cell_w_logical * scale`, `app.cell_h_logical * scale`,
    /// origin = `(padding * scale, (TITLE_BAR + TAB_BAR +
    /// STATUS_BAR_TOP + padding) * scale)`).
    ///
    /// Returns `(cell_w_px, cell_h_px, origin_x_px, origin_y_px)`. All
    /// values are floats so the per-row stepping stays sub-pixel
    /// accurate — using rounded integers causes the click-to-cell hit
    /// test to drift further from the visual cell every row, which is
    /// exactly the bug `pixel_to_cell` used to hit by dividing by 18
    /// while cells were drawn at 17 px.
    pub(super) fn cell_metrics_px(&self, app: &App) -> (f64, f64, f64, f64) {
        let scale = self.pixels_per_point.max(1.0) as f64;
        // Cell dims come from the App's startup measurement of the
        // base font (see `App::with_settings` → `compute_cell_dims`)
        // so they track `settings.font_size` instead of the legacy
        // hard-coded 8.5×17.
        let cell_w = (app.cell_w_logical as f64) * scale;
        let cell_h = (app.cell_h_logical as f64) * scale;
        // Inner padding comes from settings.padding (logical pixels);
        // falls back to the renderer's default constants when the
        // user hasn't overridden them. task0006 (right-edge persistent
        // placement): the persistent mux sidebar reserves usable grid
        // WIDTH only (see `grid_size`) — it never contributes to
        // `origin_x`, so this is the same formula as before the sidebar
        // existed.
        let pad = (app.settings.padding as f64).max(0.0);
        let origin_x = pad * scale;
        // Vertical origin reserves room for every panel stacked above
        // the terminal grid: the CSD title bar, the tab strip, an
        // optional status-bar row pinned to the top, and the same
        // user-configured padding inset. Forgetting the title bar
        // here makes the first cell render behind the tab strip.
        let tab_h = crate::ui::tab_bar::effective_tab_bar_height(app.show_tab_bar) as f64;
        let origin_y = ((crate::ui::title_bar::TITLE_BAR_HEIGHT as f64)
            + tab_h
            + (self.status_bar_top_inset_logical as f64)
            + pad)
            * scale;
        (cell_w, cell_h, origin_x, origin_y)
    }

    /// Compute grid (cols, rows) from the current window pixel size,
    /// using the real cell metrics so the PTY size agrees with the
    /// number of cells the renderer actually paints.
    ///
    /// task0005 (findings `0029db1c89ab226f` / `5b2f22c5a14f7364`): reads
    /// [`Self::status_bar_bot_inset_settled_logical`], NOT the immediate
    /// `status_bar_bot_inset_logical` — grid computation (and, through
    /// [`Self::apply_pending_resize`], the group-wide `Resize` broadcast)
    /// must consume only a settler-forwarded inset value, so a transient
    /// height mid-settle can never reach the PTYs regardless of which
    /// source raised `pending_resize` this frame.
    pub fn grid_size(&self, app: &App) -> (u16, u16) {
        self.grid_size_for_bot_inset(app, self.status_bar_bot_inset_settled_logical)
    }

    /// Pure grid-size computation, factored out of [`Self::grid_size`]
    /// (FR6, mux-tab-switch-replay-latency task0002) so
    /// [`Self::refresh_status_bar_insets`] can compute the CANDIDATE grid
    /// size for a not-yet-applied `status_bar_bot_inset_logical` value —
    /// feeding it to [`ResizeSettler`] — without first mutating
    /// `self.status_bar_bot_inset_logical`. `grid_size` itself passes the
    /// settled inset through unchanged (task0005), so its behavior is
    /// identical to before this refactor except for WHICH inset field
    /// that is.
    pub(super) fn grid_size_for_bot_inset(&self, app: &App, bot_inset_logical: f32) -> (u16, u16) {
        let w = self.surface_config.width.max(1) as f64;
        let h = self.surface_config.height.max(1) as f64;
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        let scale = self.pixels_per_point.max(1.0) as f64;
        let bottom_inset_px = (bot_inset_logical as f64) * scale;
        // task0006 (right-edge persistent placement): the persistent mux
        // sidebar reserves usable WIDTH from the right edge — subtracted
        // here directly, not folded into `origin_x` (which stays at the
        // pre-sidebar pad-only value). `mux_sidebar_inset_logical` is
        // `0.0` in overlay mode / on a local tab, reproducing the
        // pre-sidebar usable width exactly.
        let sidebar_inset_px = (self.mux_sidebar_inset_logical as f64).max(0.0) * scale;
        // Usable area starts after the top bar (+ top status bar) +
        // top pad and the left pad, ends above the bottom status bar,
        // and is narrowed on the right by the persistent sidebar (if
        // any). Floor the resulting cell count so partial trailing
        // cells (which would clip at the surface edge) don't get
        // reported as a writable row/col.
        let usable_w = (w - origin_x - sidebar_inset_px).max(cell_w);
        let usable_h = (h - origin_y - bottom_inset_px).max(cell_h);
        let cols = (usable_w / cell_w).floor().clamp(20.0, 500.0) as u16;
        let rows = (usable_h / cell_h).floor().clamp(5.0, 200.0) as u16;
        (cols, rows)
    }

    /// Map a physical pixel position to a grid cell `(row, col)`,
    /// honoring the same origin + cell metrics the renderer uses.
    pub(super) fn pixel_to_cell(&self, pos: PhysicalPosition<f64>, app: &App) -> (u16, u16) {
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        let x = ((pos.x - origin_x).max(0.0)) / cell_w;
        let y = ((pos.y - origin_y).max(0.0)) / cell_h;
        let cols = app.cell_size.cols.max(1);
        let rows = app.cell_size.rows.max(1);
        let col = (x as u32).min((cols - 1) as u32) as u16;
        let row = (y as u32).min((rows - 1) as u32) as u16;
        (row, col)
    }

    /// Resolve the absolute buffer row shown at `screen_row`, honoring the
    /// current fold layout (collapsed bodies skew the linear mapping; a
    /// summary row maps to its region's start line).
    pub(super) fn screen_row_to_abs(&self, screen_row: u16, app: &App) -> u32 {
        if let Some(layout) = app.fold_layout() {
            match layout.rows.get(screen_row as usize) {
                Some(crate::fold::FoldRowKind::Cells { actual_line }) => return *actual_line,
                Some(crate::fold::FoldRowKind::Summary { region }) => return region.start_line,
                None => {} // past the layout → fall through to the linear map
            }
        }
        // No fold layout (or screen_row past it): the linear scrollback
        // model. `visible_start = scrollback_len - scroll_offset` (saturating)
        // is the absolute row at the top of the viewport.
        let scrollback_len = app
            .active_tab()
            .map(|t| t.core.lock().get_scrollback_length())
            .unwrap_or(0);
        scrollback_len.saturating_sub(app.scroll_offset()) + screen_row as u32
    }

    /// Write `text` to the X11 PRIMARY selection (auto-copy on mouse-up).
    /// No-op when arboard is unavailable.
    fn set_primary(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let cb = match &mut self.clipboard {
            Some(c) => c,
            None => return,
        };
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use arboard::{LinuxClipboardKind, SetExtLinux};
            if let Err(e) = cb.set().clipboard(LinuxClipboardKind::Primary).text(text) {
                log::warn!("arboard PRIMARY set failed: {e}");
            }
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            // PRIMARY is X11-only; fall through to CLIPBOARD on other
            // platforms (Phase 4 targets Linux; this keeps the type checked).
            if let Err(e) = cb.set_text(text) {
                log::warn!("clipboard set_text fallback failed: {e}");
            }
        }
    }

    /// Write `text` to the CLIPBOARD selection (Ctrl+Shift+C).
    fn set_clipboard(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let cb = match &mut self.clipboard {
            Some(c) => c,
            None => return,
        };
        if let Err(e) = cb.set_text(text) {
            log::warn!("arboard CLIPBOARD set failed: {e}");
        }
    }

    /// Read the CLIPBOARD selection (Ctrl+Shift+V).
    fn get_clipboard(&mut self) -> Option<String> {
        let cb = self.clipboard.as_mut()?;
        match cb.get_text() {
            Ok(s) => Some(s),
            Err(e) => {
                log::warn!("arboard CLIPBOARD get failed: {e}");
                None
            }
        }
    }

    /// Read the PRIMARY selection (middle-click paste).
    fn get_primary(&mut self) -> Option<String> {
        let cb = self.clipboard.as_mut()?;
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use arboard::{GetExtLinux, LinuxClipboardKind};
            match cb.get().clipboard(LinuxClipboardKind::Primary).text() {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("arboard PRIMARY get failed: {e}");
                    None
                }
            }
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        {
            match cb.get_text() {
                Ok(s) => Some(s),
                Err(e) => {
                    log::warn!("clipboard get_text fallback failed: {e}");
                    None
                }
            }
        }
    }

    /// Feed pasted text to the active tab, wrapping it in bracketed paste
    /// when DECSET 2004 is on.
    fn deliver_paste(&self, app: &App, text: &str) {
        if text.is_empty() {
            return;
        }
        let tab = match app.active_tab() {
            Some(t) => t,
            None => return,
        };
        let bracketed = tab
            .core
            .lock()
            .get_mode(term_core::terminal_core::MODE_BRACKETED_PASTE);
        // mux-aware: in mux mode the paste is wrapped as a PtyInput frame
        // (the bridge drops raw stdin); otherwise it is a plain bracketed
        // PTY write.
        tab.write_paste_input(text, bracketed);
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
        // so a pending resize subsumes any prior Lost/Outdated reconfigure.
        let had_pending_resize = self.pending_resize;
        self.apply_pending_resize(app);
        if had_pending_resize {
            // Resize changes the swapchain extent; everything must repaint.
            app.mark_full_redraw();
            // We just reconfigured to the new size, so any earlier
            // Lost/Outdated recovery request is now redundant.
            self.surface_dirty = false;
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
            let now = ctx.input(|i| i.time);
            if app.pump_sftp(now) {
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

    /// Reload `settings.json` and apply it to the running app. Called
    /// from `about_to_wait` when the child settings window reports a
    /// persisted save (see [`crate::settings_launcher`]); the child
    /// already validated and wrote the file, so the parent only loads
    /// and applies.
    fn reload_settings_from_disk(&mut self, app: &mut App) {
        let new = crate::settings::Settings::load_or_default();
        let old_ui_font = app.settings.ui_font_family.clone();
        let old_terminal_font = terminal_font_family(&app.settings).to_string();
        let needs_resize = app.apply_settings(new);
        // File-log recording is owned by the logging module, not App.
        crate::logging::set_recording_enabled(app.settings.log_recording_enabled);
        // The egui chrome font chain is owned by the host context, not
        // the App; rebuild it when either the UI font (Proportional
        // head) or the terminal font (Monospace head; drives status-bar
        // glyph shape) changed.
        if app.settings.ui_font_family != old_ui_font
            || terminal_font_family(&app.settings) != old_terminal_font
        {
            configure_egui_fonts(
                &self.egui_ctx,
                &app.settings.ui_font_family,
                terminal_font_family(&app.settings),
            );
        }
        if needs_resize {
            // Cell metrics / padding changed: reshape the PTY grid for
            // the unchanged window pixel size on the next frame.
            self.request_resize();
        }
        app.mark_full_redraw();
        self.window.request_redraw();
        crate::wakeup::wake();
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

/// task0005 AC-1: whether the hovered link's cell-span changed (appear,
/// move, or disappear). Extracted as a pure equality check so
/// `refresh_link_hover` / `invalidate_link_hover`'s latch-setting logic is
/// directly unit-testable without a window (mirrors `should_skip_frame`
/// below).
pub(super) fn hover_link_cells_changed(
    prev_cells: &[(u16, u16, u16)],
    new_cells: &[(u16, u16, u16)],
) -> bool {
    prev_cells != new_cells
}

/// Resolve the bottom inset value [`WindowHost::grid_size`] may consume
/// after THIS render's [`ResizeSettler::observe`] call (task0005 round-1
/// rework, findings `0029db1c89ab226f` / `5b2f22c5a14f7364`): grid
/// computation, and the group-wide `Resize` broadcast it feeds through
/// [`WindowHost::apply_pending_resize`], must consume only a
/// settler-forwarded inset value — never the transient one
/// `WindowHost::refresh_status_bar_insets` writes immediately into
/// `status_bar_bot_inset_logical` for drawing / pointer-routing (Change
/// 2, D-D, mux-tab-switch-bypass-refix task0002).
///
/// `settler_awaiting_decision` is [`ResizeSettler::awaiting_decision`]
/// evaluated immediately AFTER `observe` runs this render:
///
/// - `true` — the settling window is still open: either genuinely
///   mid-storm (candidate not yet held stable), or freshly reopened by
///   `ResizeSettler::reset` on a mux attach/reattach. `previously_settled`
///   is returned unchanged, so [`WindowHost::apply_pending_resize`] can
///   never pick up `transient` this frame — regardless of WHICH source
///   (the settler itself, the mux-sidebar inset refresh, or a compositor
///   `Resized` / `ScaleFactorChanged`) is what raised `pending_resize`.
///   This is what closes the traced firing order: a fresh attach resets
///   the settler and writes a transient inset in the same call, a
///   sidebar-driven `pending_resize` follows, and without this gate the
///   apply that comes after would have used the transient value.
/// - `false` — the settler is not withholding judgment: either it just
///   forwarded `transient` this render (closing the window on exactly the
///   value that produced the forwarded candidate — this is what keeps
///   [`ResizeSettler::last_forwarded`] and the size
///   `apply_pending_resize` actually computes in agreement, no
///   divergence), or the window was already closed from a prior render
///   (the steady-state case: every render's `bot` IS the current,
///   non-transient inset, so tracking it here with no lag avoids stale
///   lock-in across a run of inset changes whose derived grid-size
///   candidate happens not to move — the FR4 case `status_bar_insets_
///   changed` alone already applies for drawing).
pub(super) fn resolve_grid_bot_inset(
    previously_settled: f32,
    transient: f32,
    settler_awaiting_decision: bool,
) -> f32 {
    if settler_awaiting_decision {
        previously_settled
    } else {
        transient
    }
}

/// FR3: synthesize a `DetectedLink` for an OSC 8 hyperlinked cell at
/// `(row, col)`. Returns `None` for cells with `hyperlink_id == 0`
/// (no link), for ids whose URI is missing from `hyperlink_table`
/// (e.g. evicted from scrollback), for empty URIs, and for URIs whose
/// scheme fails [`crate::links::is_safe_uri`] (unsafe schemes such as
/// `javascript:` / `data:` are rejected with a `warn` log so the
/// click-time branch can dispatch unconditionally on `Some`).
///
/// On a hit the cell range is expanded leftward and rightward across
/// contiguous cells on the same row carrying the same `hyperlink_id`,
/// so the renderer underlines the whole OSC 8 run instead of a single
/// cell. The bound is the row width, which is small (≤ a few hundred).
pub(super) fn detect_osc8_link_at(
    core: &term_core::terminal_core::TerminalCore,
    row: u16,
    col: u16,
) -> Option<crate::links::DetectedLink> {
    let cols = core.cols();
    if cols == 0 || row >= core.rows() || col >= cols {
        return None;
    }
    let id = core.get_cell_hyperlink_id(col, row);
    if id == 0 {
        return None;
    }
    let uri = core.get_hyperlink_uri(id);
    if uri.is_empty() {
        // Missing id in `hyperlink_table` (id evicted with the cell row
        // it was last seen on, or never registered) or an OSC 8 with
        // an empty URI. Either way there is nothing to open.
        return None;
    }
    if !crate::links::is_safe_uri(&uri) {
        log::warn!("native-poc: refusing OSC 8 URI with unsafe scheme: {uri}");
        return None;
    }
    // Expand the run leftward.
    let mut col_start = col;
    while col_start > 0 && core.get_cell_hyperlink_id(col_start - 1, row) == id {
        col_start -= 1;
    }
    // Expand the run rightward (exclusive end column).
    let mut col_end = col + 1;
    while col_end < cols && core.get_cell_hyperlink_id(col_end, row) == id {
        col_end += 1;
    }
    Some(crate::links::DetectedLink {
        kind: crate::links::LinkKind::Url(uri),
        cells: vec![(row, col_start, col_end)],
    })
}

/// Hand a (safe-scheme-checked) URL to the OS opener. Linux uses
/// `xdg-open`; Windows uses ShellExecuteW via the `opener` crate. A
/// spawn failure is logged at `warn` and otherwise ignored — the
/// WebView build similarly swallows opener errors. The caller is
/// responsible for the `is_safe_uri` gate.
fn open_url(url: &str) {
    #[cfg(target_os = "linux")]
    {
        match std::process::Command::new("xdg-open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                log::info!("native-poc: opened URL via xdg-open: {url}");
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => log::warn!("native-poc: xdg-open failed for {url}: {e}"),
        }
    }
    #[cfg(target_os = "windows")]
    {
        // ShellExecuteW receives the URL as a bare parameter — unlike
        // `cmd /c start`, no cmd.exe parsing happens, so cmd
        // metacharacters (`&`, `|`, `^`) in a PTY-supplied URL cannot
        // inject commands.
        match opener::open(url) {
            Ok(()) => log::info!("native-poc: opened URL via ShellExecuteW: {url}"),
            Err(e) => log::warn!("native-poc: ShellExecuteW failed for {url}: {e}"),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        log::warn!("native-poc: no URL opener on this platform for {url}");
    }
}

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

/// Run the event loop until the window is closed. Owns the App.
pub fn run(event_loop: EventLoop, app: App) -> ! {
    let handler = PocApp { app, host: None };
    // winit 0.31's `run_app` takes the handler by value (no more
    // `&mut app`) and explicitly drops it before returning, so the
    // PTY-owning tabs' reader/writer threads shut down cleanly here —
    // no separate `drop(handler)` needed after this call.
    if let Err(e) = event_loop.run_app(handler) {
        log::error!("native-poc: winit event loop returned an error: {e}");
    }
    std::process::exit(0);
}

/// Intercept Phase 4 chords. Returns `true` when the event was consumed
/// (the generic encoder should not run).
///
/// `egui_key` is the pre-computed result of `winit_key_to_egui` for this
/// event, passed in so the caller can reuse the same value for the
/// keybinds dispatch path without a second translation.
pub(super) fn handle_special_chord(
    event: &KeyEvent,
    mods: Modifiers,
    egui_key: Option<egui::Key>,
    host: &mut WindowHost,
    app: &mut App,
) -> bool {
    // Clipboard chords are settings-driven (`keybinds.copy` /
    // `keybinds.paste`). Build the incoming chord from the winit event +
    // modifiers and compare against the resolved table.
    if let Some(key) = egui_key {
        let chord = Chord {
            ctrl: mods.ctrl,
            shift: mods.shift,
            alt: mods.alt,
            key,
        };
        if chord == app.keybinds.copy {
            // Copy current selection to CLIPBOARD. We consume the chord
            // even when there is no active selection so the configured
            // copy key never leaks through to the PTY.
            if let Some(sel) = app.selection {
                if let Some(tab) = app.tabs.get(app.active) {
                    let core = tab.core.lock();
                    let text = sel.resolve(&core, app.fold_layout());
                    drop(core);
                    host.set_clipboard(&text);
                }
            }
            return true;
        }
        if chord == app.keybinds.paste {
            if let Some(text) = host.get_clipboard() {
                host.deliver_paste(app, &text);
            }
            return true;
        }
    }

    // Scrollback chords use Shift + nav keys.
    if mods.shift && !mods.ctrl && !mods.alt {
        match &event.logical_key {
            WinitKey::Named(NamedKey::PageUp) => {
                let rows = app.cell_size.rows.max(1) as u32;
                app.scroll_up_by(rows);
                // Viewport shifted under the pointer; cached hover is stale.
                host.invalidate_link_hover();
                return true;
            }
            WinitKey::Named(NamedKey::PageDown) => {
                let rows = app.cell_size.rows.max(1) as u32;
                app.scroll_down_by(rows);
                host.invalidate_link_hover();
                return true;
            }
            WinitKey::Named(NamedKey::Home) => {
                app.scroll_to_top();
                host.invalidate_link_hover();
                return true;
            }
            WinitKey::Named(NamedKey::End) => {
                app.scroll_to_live();
                host.invalidate_link_hover();
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Route a key press into the search overlay while it owns the keyboard.
///
/// Precedence (mirrors `SearchBar.keydown` + the WebView nav bindings):
///   1. `Esc` → close the overlay + clear state.
///   2. `Enter` / `Shift+Enter` → next / previous match.
///   3. `keybinds.copy` / `keybinds.paste` chords → inject an
///      `egui::Event::Copy` / `egui::Event::Paste(text)` so the field's
///      own clipboard handling fires (copy of the field selection, paste
///      of the OS clipboard into the field).
///   4. Everything else → forward to egui as an `Event::Key` plus, when
///      the key produced committed text and no Ctrl/Alt is held, an
///      `Event::Text` so the TextEdit inserts the character.
///
/// The terminal IME dispatch + PTY encoder are intentionally bypassed:
/// while searching, keystrokes belong to the search field, not the shell.
/// Keyboard handling while the profile-selector modal is visible. The
/// modal owns the keyboard completely: navigation / confirm / cancel act
/// on the selector state, every other key is swallowed (never encoded to
/// the PTY). Port of `profile-selector.ts::handleKeydown` (ArrowUp /
/// ArrowDown wrap, Home / End, Enter / Space confirm, Escape cancel).
pub(super) fn handle_profile_selector_key(event: &KeyEvent, app: &mut App) {
    use winit::keyboard::NamedKey;

    // Row count includes the synthetic "Global Settings" row in new-tab
    // chooser mode.
    let len = app.profile_selector_row_count();
    match &event.logical_key {
        WinitKey::Named(NamedKey::Escape) => app.profile_selector.close(),
        WinitKey::Named(NamedKey::ArrowDown) => app.profile_selector.move_selection(1, len),
        WinitKey::Named(NamedKey::ArrowUp) => app.profile_selector.move_selection(-1, len),
        WinitKey::Named(NamedKey::Home) => app.profile_selector.select_edge(false, len),
        WinitKey::Named(NamedKey::End) => app.profile_selector.select_edge(true, len),
        // winit 0.31 removed `NamedKey::Space`; the space bar now arrives
        // as `Character(" ")`, already covered by the arm below.
        WinitKey::Named(NamedKey::Enter) => {
            let idx = app.profile_selector.selected;
            app.confirm_profile_selection(idx);
        }
        WinitKey::Character(c) if c == " " => {
            let idx = app.profile_selector.selected;
            app.confirm_profile_selection(idx);
        }
        _ => {}
    }
}

/// Forward a key press into egui while a mux rename / move dialog is open.
/// Mirrors the search-bar capture: editing keys (Backspace / arrows /
/// Enter / Escape …) go through as egui `Key` events and printable text as
/// `Text` events, so the dialog's `TextEdit` / `DragValue` and its
/// Enter-confirm / Escape-cancel handling work. The terminal IME backend,
/// the keybind dispatcher, and the PTY encoder never see the key — without
/// this gate, typing in the dialog would leak into the running shell.
pub(super) fn handle_mux_dialog_key(event: &KeyEvent, mods: Modifiers, host: &mut WindowHost) {
    if let Some(key) = winit_key_to_egui(&event.logical_key) {
        host.pending_egui_events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: event.repeat,
            modifiers: input_mods_to_egui(mods),
        });
    }
    // Printable characters insert into the focused field. Suppressed while
    // Ctrl/Alt is held so control chords do not also emit a literal glyph.
    if !mods.ctrl && !mods.alt {
        if let Some(text) = &event.text {
            let printable: String = text.chars().filter(|c| !c.is_control()).collect();
            if !printable.is_empty() {
                host.pending_egui_events.push(egui::Event::Text(printable));
            }
        }
    }
}

pub(super) fn handle_search_key(
    event: &KeyEvent,
    mods: Modifiers,
    host: &mut WindowHost,
    app: &mut App,
) {
    use winit::keyboard::NamedKey;

    // 1. Esc closes the overlay.
    if matches!(event.logical_key, WinitKey::Named(NamedKey::Escape)) {
        app.close_search();
        return;
    }

    // 2. Enter / Shift+Enter navigate. Handled before egui so the field's
    //    default Enter (which does nothing useful for a single-line edit)
    //    never swallows them.
    if matches!(event.logical_key, WinitKey::Named(NamedKey::Enter)) {
        if mods.shift {
            app.search_prev();
        } else {
            app.search_next();
        }
        // Same contract as the search-bar button path in `render()`:
        // match navigation can scroll the viewport (and expand folds),
        // so the cached hover spans index the pre-jump viewport.
        host.invalidate_link_hover();
        return;
    }

    let egui_key = winit_key_to_egui(&event.logical_key);

    // 3. Copy / paste chords → egui clipboard events targeting the field.
    if let Some(key) = egui_key {
        let chord = Chord {
            ctrl: mods.ctrl,
            shift: mods.shift,
            alt: mods.alt,
            key,
        };
        // Re-pressing the search chord while the overlay is open re-focuses
        // the field + reselects the query (rather than inserting an 'f').
        if chord == app.keybinds.search {
            app.open_search();
            return;
        }
        if chord == app.keybinds.copy {
            host.pending_egui_events.push(egui::Event::Copy);
            return;
        }
        if chord == app.keybinds.paste {
            if let Some(text) = host.get_clipboard() {
                host.pending_egui_events.push(egui::Event::Paste(text));
            }
            return;
        }
    }

    // 4. Forward as an egui key event so the TextEdit can act on editing
    //    keys (Backspace / Delete / arrows / Home / End / Ctrl+A …).
    if let Some(key) = egui_key {
        host.pending_egui_events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: event.repeat,
            modifiers: input_mods_to_egui(mods),
        });
    }

    // …and forward the committed text for character insertion. Suppressed
    // when Ctrl/Alt is held so control chords (e.g. Ctrl+A select-all) do
    // not also insert a literal character into the field.
    if !mods.ctrl && !mods.alt {
        if let Some(text) = &event.text {
            // Drop control characters (e.g. the Enter/Tab text payloads
            // winit attaches) — printable text only reaches the field.
            let printable: String = text.chars().filter(|c| !c.is_control()).collect();
            if !printable.is_empty() {
                host.pending_egui_events.push(egui::Event::Text(printable));
            }
        }
    }
}

#[cfg(test)]
mod tests;
