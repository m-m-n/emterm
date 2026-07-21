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
use std::time::{Duration, Instant};

use egui::ViewportId;
use egui_wgpu::ScreenDescriptor;
use egui_wgpu::wgpu::SurfaceError;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, ResizeDirection, Window, WindowAttributes, WindowId};

use crate::app::App;
use crate::ime::backend::{KeyDispatchResult, ProcessEnv, RawKeyEvent, build_backend_with_window};
use crate::mux::dialog::{MuxDialogOutcome, MuxDialogState};
use crate::mux::prefix::{KeyInput as MuxKeyInput, KeySym};
use crate::pty::input::{Key, Modifiers, Target as EncodeTarget, encode};
use crate::settings::ShiftEnterBehavior;

/// Drive one frame of the open mux dialog: render via the UI layer
/// (`ui::mux_dialogs::draw`) and dispatch the resulting outcome into the
/// domain layer (`App::confirm_mux_*`). This is the orchestration glue
/// that previously lived in `ui::mux_dialogs::drive`; moved here so the UI
/// module no longer has to `use crate::app::App` (otherwise the UI layer
/// imports App, and App imports UI types like `TabEvent` — a cycle).
/// `window_host` already owns `App`, so dispatch lives at this boundary.
fn drive_mux_dialogs(app: &mut App, ctx: &egui::Context) -> bool {
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
fn egui_to_mux_input(mods: Modifiers, key: egui::Key) -> MuxKeyInput {
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
const MULTI_CLICK_WINDOW_MS: u128 = 500;

use crate::ui::chrome::{RESIZE_EDGE_PX, classify_resize_edge, configure_egui_fonts};

/// Tracks last-click metadata so a double / triple click can be detected by
/// comparing time + position against the next press.
#[derive(Debug, Clone, Copy, Default)]
struct ClickTracker {
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
struct ClickClassification {
    count: u32,
    mode: SelectionMode,
}

impl ClickTracker {
    /// Classify a new press at absolute `(row, col)` happening at `now`. The
    /// internal state is updated for the next call.
    fn classify(&mut self, now: Instant, row: u32, col: u16) -> ClickClassification {
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
    window: Arc<Window>,
    pixels_per_point: f32,
    /// True when the surface must be recreated on the next frame (e.g. after
    /// `SurfaceError::Lost`).
    surface_dirty: bool,
    /// Alacritty-style deferred resize: `WindowEvent::Resized` only flips
    /// this flag and requests a redraw; the next `render()` call reads the
    /// current `window.inner_size()` once and runs `surface.configure()` +
    /// `app.set_grid_size()` together. This coalesces bursts of compositor
    /// resize events (one configure per frame instead of one per event) and
    /// avoids back-buffer locking when configure and draw happen out of
    /// order on Wayland / X11. See
    /// `wezterm/window/src/os/x11/window.rs:298` (coalesce) and
    /// `alacritty/src/display/mod.rs:739` (defer-to-render).
    pending_resize: bool,
    /// Cached status-bar panel insets in egui logical points, refreshed
    /// each frame from `App::status_bar_view_model`. Subtracted from the
    /// usable grid area in [`grid_size`] (and added to `origin_y` when
    /// the panel sits on top) so the terminal's bottom/top row never
    /// renders behind the status-bar panel.
    status_bar_top_inset_logical: f32,
    status_bar_bot_inset_logical: f32,
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
    current_mods: Modifiers,
    /// Last cursor position in physical pixels (updated on `CursorMoved`).
    cursor_pos: PhysicalPosition<f64>,
    /// Whether the left button is currently held — used as the gate for
    /// turning subsequent `CursorMoved` events into selection extends.
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
    /// every `CursorMoved` (when not selection-dragging) so the next
    /// left-press can hand the matching [`ResizeDirection`] to
    /// `Window::drag_resize_window` without re-running the hit test.
    /// `None` means the pointer is in the window interior — a press
    /// falls through to the existing selection / tab-bar handlers.
    current_resize_dir: Option<ResizeDirection>,
    /// Last cursor icon pushed to winit. Cached so [`update_resize_hint`]
    /// can skip the IPC round-trip when the icon would not change —
    /// `set_cursor` is otherwise called on every `CursorMoved`, which
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
    /// `CursorMoved` (there is no `CursorEntered` handler) and to `false`
    /// by `CursorLeft`. Used to gate PTY-output re-detection in
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
struct HoverState {
    /// Grid cell the last detection ran for (`None` = pointer outside the
    /// grid / no detection yet). Used to skip re-running detection on
    /// sub-cell pointer motion.
    cell: Option<(u16, u16)>,
    /// Physical cell spans of the link currently under the pointer
    /// (`(row, col_start, col_end)`), empty when no link is hovered. Read
    /// by the grid pass to underline the matched cells.
    link_cells: Vec<(u16, u16, u16)>,
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
        event_loop: &ActiveEventLoop,
        ui_font_family: &str,
        terminal_font_family: &str,
    ) -> Self {
        let attrs = WindowAttributes::default()
            .with_title("eMterm PoC")
            .with_decorations(false)
            .with_inner_size(LogicalSize::new(960.0, 600.0))
            .with_min_inner_size(LogicalSize::new(320.0, 200.0))
            .with_maximized(true)
            // FR2: attach the bundled app icon to the main winit window so
            // the title bar and taskbar (Windows fallbacks) render the
            // eMterm glyph. `None` from `app_icon()` is a clean no-op.
            .with_window_icon(crate::window_icon::app_icon());
        // FR5: report the canonical dock-grouping identifier (X11
        // `WM_CLASS` / Wayland `app_id`) so every window groups under one
        // `emterm` dock icon. winit applies the trait matching the active
        // backend; the other is a no-op. `with_name` is on both extension
        // traits, so call each via fully-qualified syntax to avoid an
        // ambiguous method resolution.
        #[cfg(target_os = "linux")]
        let attrs = crate::linux_wm::with_app_id(attrs);
        let window = event_loop
            .create_window(attrs)
            .expect("native-poc: failed to create winit window");
        let window = Arc::new(window);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // SAFETY: `window` is kept alive in `Arc<Window>` and stored
        // alongside the surface for the whole `WindowHost` lifetime.
        let surface: wgpu::Surface<'static> = unsafe {
            instance
                .create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&*window).expect("surface target"),
                )
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

        let size = window.inner_size();
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
            mux_sidebar_inset_logical: 0.0,
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

    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Hand a clone of the `Arc<Window>` to callers that need to retain
    /// the handle themselves (Phase 4-G-3 passes this to
    /// `WinitImeBridge::init` so the bridge can call
    /// `Window::set_ime_cursor_area`).
    pub fn window_arc(&self) -> Arc<Window> {
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
    /// The resulting `WindowEvent::Resized` drives the deferred
    /// `apply_pending_resize`, so the grid reshapes on the next frame
    /// without any extra plumbing here.
    fn toggle_fullscreen(&self) {
        use winit::window::Fullscreen;
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
            .inner_size()
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
    /// pointer position. Cheap to call on every `CursorMoved`: skips
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
        self.window.set_cursor(icon);
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
    fn refresh_link_hover(&mut self, app: &App) {
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
    fn refresh_link_hover_on_pty_change(&mut self, app: &App) {
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
    fn update_link_cursor(&mut self) {
        let icon = if self.current_resize_dir.is_some() {
            self.current_cursor // leave resize-hint icon untouched
        } else if self.current_mods.ctrl && self.hover.link.is_some() {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        };
        if icon != self.current_cursor {
            self.current_cursor = icon;
            self.window.set_cursor(icon);
        }
    }

    /// Drop any cached link-hover state and clear a hand cursor. Called
    /// when the grid content shifts under the pointer (scroll) or the
    /// pointer leaves the window, so a stale underline / cursor doesn't
    /// survive. Requests a redraw when an underline was showing.
    fn invalidate_link_hover(&mut self) {
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
    fn try_open_link_at_pointer(&mut self, app: &App) -> bool {
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
        let size = self.window.inner_size();
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
                .create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&*self.window).expect("surface target"),
                )
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

    /// Consume `pending_resize` and apply the latest `window.inner_size()`.
    ///
    /// Called once per `render()` so a burst of compositor resize events
    /// produces a single configure + PTY resize cycle aligned with the
    /// frame boundary. Zero-sized windows (Windows minimize, Wayland hidden)
    /// just clear the flag without reconfiguring.
    fn apply_pending_resize(&mut self, app: &mut App) {
        if !self.pending_resize {
            return;
        }
        self.pending_resize = false;
        let size = self.window.inner_size();
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
    /// panel. When the inset changes, flag a pending resize so the PTY
    /// is reshaped before the next frame paints — otherwise the bottom
    /// row would be hidden behind the panel for one frame.
    fn refresh_status_bar_insets(&mut self, app: &App) {
        let vm = app.status_bar_view_model();
        let height = crate::ui::status_bar::panel_height_logical(&vm);
        // The status bar is fixed at the bottom; reserve its height there.
        let (top, bot) = (0.0, height);
        if (self.status_bar_top_inset_logical - top).abs() > f32::EPSILON
            || (self.status_bar_bot_inset_logical - bot).abs() > f32::EPSILON
        {
            self.status_bar_top_inset_logical = top;
            self.status_bar_bot_inset_logical = bot;
            self.pending_resize = true;
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
    fn refresh_mux_sidebar_inset(&mut self, app: &App) {
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
    fn cell_metrics_px(&self, app: &App) -> (f64, f64, f64, f64) {
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
    pub fn grid_size(&self, app: &App) -> (u16, u16) {
        let w = self.surface_config.width.max(1) as f64;
        let h = self.surface_config.height.max(1) as f64;
        let (cell_w, cell_h, origin_x, origin_y) = self.cell_metrics_px(app);
        let scale = self.pixels_per_point.max(1.0) as f64;
        let bottom_inset_px = (self.status_bar_bot_inset_logical as f64) * scale;
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
    fn pixel_to_cell(&self, pos: PhysicalPosition<f64>, app: &App) -> (u16, u16) {
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
    fn screen_row_to_abs(&self, screen_row: u16, app: &App) -> u32 {
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
            let overlay_work = app.restart_toast.active()
                || !app.sftp_ui.toasts.toasts.is_empty()
                || app.visual_bell_progress().is_some()
                || app.search_visible()
                || bell_erase_pending;
            // Undrained *actionable* egui input (a click, wheel scroll, key,
            // text, or clipboard event) vetoes the skip: `build_raw_input`
            // below is the only drain, so a skipped frame would park it
            // until the next unrelated wakeup (worst case a blink flip,
            // ~530 ms). `PointerMoved` alone is excluded from this veto:
            // `CursorMoved` pushes one unconditionally on every mouse
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
            frame_events = crate::render::draw_placeholder(ctx, app, window_maximized);
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
    fn build_raw_input(&mut self) -> egui::RawInput {
        let size = self.window.inner_size();
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
fn hover_link_cells_changed(prev_cells: &[(u16, u16, u16)], new_cells: &[(u16, u16, u16)]) -> bool {
    prev_cells != new_cells
}

/// Sub-phase 2 dirty-row skip decision (task0002 AC-5): extracted from
/// `WindowHost::render` as a pure function — plain values in, plain bool
/// out, no window/app/egui types — so it is directly unit-testable.
///
/// `dirty_count` is `None` when there is no active tab (the hint-message
/// frame always proceeds so it can draw); `Some(n)` is
/// `App::dirty_rows_this_frame(..).len()`. `status_bar_changed` is
/// `App::status_bar_view_model_changed()` — the carve-out that keeps the
/// status bar's own wake chain (clock tick, git branch, OSC 777 push)
/// live even when the terminal grid itself is quiescent.
///
/// `overlay_work` is `true` when a restart/SFTP toast is counting down to
/// auto-dismiss, a visual-bell flash is still decaying, the search UI is
/// visible, or the one-shot bell-erase-frame signal is pending — any of
/// these needs the egui pass (`pump_sftp` / toast prune / bell paint /
/// search overlay) to run every frame, the same carve-out
/// `status_bar_changed` gets for the status bar's own wake chain.
///
/// `egui_input_pending` is `true` when `pending_egui_events` holds input
/// (a click, wheel, or key destined for the egui chrome) that no egui pass
/// has consumed yet. Those events are drained only by `build_raw_input`,
/// which runs *after* this decision — skipping such a frame would leave
/// the click queued until the next unrelated wakeup (worst case the next
/// blink flip, ~530 ms), which is exactly the sluggish tab-switch the
/// post-merge report described. Any pending egui input therefore vetoes
/// the skip.
///
/// Returns `true` (skip the frame) only when the dirty count is known to
/// be exactly zero AND the status bar did not change AND there is no
/// pending overlay work AND no egui input is waiting; every other
/// combination proceeds to a full frame.
fn should_skip_frame(
    dirty_count: Option<usize>,
    status_bar_changed: bool,
    overlay_work: bool,
    egui_input_pending: bool,
) -> bool {
    matches!(dirty_count, Some(0)) && !status_bar_changed && !overlay_work && !egui_input_pending
}

/// Whether `about_to_wait` should request a redraw on behalf of an active
/// toast this turn: a toast is up AND at least [`crate::app::TOAST_POLL_MS`]
/// has elapsed since the last toast-driven request (`None` = no request was
/// made yet, so the first one fires immediately). This is the rate limit
/// that keeps the toast-driven `request_redraw` → `RedrawRequested` →
/// `about_to_wait` cycle at the poll cadence instead of spinning at full
/// speed under a non-blocking present mode. Extracted as a pure function —
/// plain values in, plain bool out — so it is directly unit-testable
/// (mirrors [`should_skip_frame`] above).
fn toast_redraw_due(toast_pending: bool, last_redraw: Option<Instant>, now: Instant) -> bool {
    toast_pending
        && last_redraw.is_none_or(|last| {
            now.duration_since(last) >= Duration::from_millis(crate::app::TOAST_POLL_MS)
        })
}

/// Whether `events` contains at least one egui event that must veto the
/// idle-skip decision above. `egui::Event::PointerMoved` is deliberately
/// excluded: `CursorMoved` pushes one unconditionally on every mouse
/// motion, so treating it as actionable would force a full egui+GPU frame
/// on every hover pixel over an otherwise idle terminal. A click still
/// vetoes because it arrives as `[PointerMoved, PointerButton]` — the
/// trailing `PointerButton` is not excluded — so click responsiveness is
/// unaffected; only chrome hover feedback for motion-only frames is
/// deferred until the next discrete event or a content change.
///
/// Exception: while a pointer button is held (`pointer_button_held`),
/// motion IS actionable — egui chrome drags (scrollbar thumb, tab
/// reorder) live entirely in the press→release motion stream, and
/// skipping those frames would freeze the drag's live tracking on an
/// idle terminal until the release finally vetoes.
fn has_actionable_egui_input(events: &[egui::Event], pointer_button_held: bool) -> bool {
    if pointer_button_held {
        !events.is_empty()
    } else {
        events
            .iter()
            .any(|e| !matches!(e, egui::Event::PointerMoved(_)))
    }
}

/// The dirty-row snapshot the grid build may trust, given whether a full
/// redraw was raised since (or survived past) the frame-top snapshot —
/// extracted as a pure function (plain values in, plain values out)
/// mirroring `should_skip_frame` above.
///
/// The snapshot in `frame_dirty_rows` is taken at the top of `render` for
/// the skip decision, but the egui pass in the middle of the frame can
/// apply events that invalidate it: a tab-bar click switches the active
/// tab (the snapshot then indexes a *different* tab's core), a scrollbar
/// jump moves the viewport. Those paths call `App::mark_full_redraw`, so
/// "the flag is set at build time" is exactly the signal that the
/// snapshot is stale. Returning `None` routes both build branches to
/// their existing every-row path (`None` already means "forced full
/// redraw" there), rebuilding the whole cache against the current state.
///
/// Without this, the frame paints the new tab's dirty rows over the
/// previous tab's cached rows, and `record_render_state` then consumes
/// the mid-frame flag at end of frame — leaving the mixed content on
/// screen indefinitely (the post-merge "switching tabs keeps the old
/// tab's output, only the prompt row updates" report).
fn resolve_build_dirty_rows(
    snapshot: Option<Vec<u16>>,
    full_redraw_pending: bool,
) -> Option<Vec<u16>> {
    if full_redraw_pending { None } else { snapshot }
}

/// task0006: whether this frame's pending core scroll event should
/// rotate the per-row instance cache — extracted as a pure function
/// (plain values in, plain bool out) mirroring `should_skip_frame`
/// above, so the decision is directly unit-testable without a window.
///
/// `scroll_count` is `TerminalCore::get_scroll_event_count()`.
/// `partial_dirty_rows` is `true` only on the ordinary cached path,
/// where `frame_dirty_rows` names FEWER rows than the viewport's total —
/// `false` on any turn where the effective dirty set is already every
/// row (a forced full redraw: `was_surface_dirty`, `needs_full_redraw`
/// / `force_full_redraw`, a fold layout, or a scrolled-back viewport
/// reacting to new output — see `App::on_pty_output`). In every `false`
/// case the rebuild below overwrites the whole cache regardless of any
/// rotation, so rotating first would just be wasted work.
///
/// Callers still clear the core-side event whenever `scroll_count > 0`
/// regardless of this function's answer (task0006 Design:
/// "needs_full_redraw frames: full rebuild already; just clear the
/// event") — this function only gates the rotation itself.
fn should_rotate_row_cache_for_scroll_event(scroll_count: u16, partial_dirty_rows: bool) -> bool {
    scroll_count > 0 && partial_dirty_rows
}

/// The dirty-row set fed to [`crate::render::terminal_grid_pass::
/// TerminalGridPass::rebuild_and_collect`] during an IME-preedit-active
/// frame (task0003 High finding fix): extracted as a pure function —
/// plain values in, plain `Vec<u16>` out, no window/app/core types — so
/// every combination is directly unit-testable, mirroring
/// `should_skip_frame` above.
///
/// Starts from `frame_dirty_rows` (the same set `App::dirty_rows_this_frame`
/// already computed this turn) or every row `0..row_count` when `None` (a
/// forced full redraw). The preedit anchor row and the row immediately
/// below it (composition may wrap) are then force-included even if
/// `term_core` itself considers them clean — otherwise `row_cache` would
/// keep whatever content those rows had *before* preedit started, one
/// frame stale, the moment preedit ends. The result is sorted ascending
/// and deduplicated, matching the invariant `rebuild_dirty_rows` requires.
fn preedit_effective_dirty_rows(
    frame_dirty_rows: Option<Vec<u16>>,
    row_count: u16,
    anchor_row: u16,
) -> Vec<u16> {
    let mut rows: Vec<u16> = frame_dirty_rows.unwrap_or_else(|| (0..row_count).collect());
    for r in [anchor_row, anchor_row.saturating_add(1)] {
        if r < row_count && !rows.contains(&r) {
            rows.push(r);
        }
    }
    rows.sort_unstable();
    rows.dedup();
    rows
}

/// task0004 AC-1/AC-2: pure decision for the next winit control flow, given
/// the pending timed-work deadlines this turn observed. Extracted as a free
/// function — plain `Option<Instant>` in, plain `Option<Instant>` out, no
/// winit/App types — so every combination is directly unit-testable
/// (mirrors `should_skip_frame` above).
///
/// Each argument is `None` when that concern has no pending timed work:
/// `blink_deadline` is `None` when blink is disabled, the window is
/// unfocused, the cursor is hidden, or no tab is active
/// ([`App::next_blink_deadline`]); `bell_deadline` is `None` when no
/// visual-bell flash is decaying ([`App::next_bell_deadline`]);
/// `toast_deadline` is `None` when no restart/SFTP toast is up
/// ([`App::next_toast_deadline`]).
///
/// Returns `None` when every concern is quiescent — the caller maps this to
/// `ControlFlow::Wait` (AC-2: an idle terminal, e.g. blink disabled, never
/// reschedules a periodic wakeup). Returns the earliest deadline otherwise —
/// the caller maps this to `ControlFlow::WaitUntil`.
fn next_wait_deadline(
    blink_deadline: Option<Instant>,
    bell_deadline: Option<Instant>,
    toast_deadline: Option<Instant>,
) -> Option<Instant> {
    [blink_deadline, bell_deadline, toast_deadline]
        .into_iter()
        .flatten()
        .min()
}

/// Compute the winit `ControlFlow` for this turn from the App's pending
/// timed-work deadlines (task0004 D4). Thin wiring around
/// [`next_wait_deadline`] (the unit-tested pure decision) — used by both
/// `PocApp::resumed`'s initial control flow and `PocApp::about_to_wait`'s
/// end-of-turn rearm so the two follow the same rule.
fn control_flow_for(app: &App) -> ControlFlow {
    match next_wait_deadline(
        app.next_blink_deadline(),
        app.next_bell_deadline(),
        app.next_toast_deadline(),
    ) {
        Some(deadline) => ControlFlow::WaitUntil(deadline),
        None => ControlFlow::Wait,
    }
}

/// Frames-drawn counter for `EMTERM_RENDER_PERF=1` (task0002 AC-6).
/// Counts every frame `record_draw` is called for and reports the
/// running total at most once per second of activity, so an idle host
/// logging at 60 Hz doesn't flood `emterm.log`.
#[derive(Debug, Default)]
struct FrameCounter {
    drawn: u64,
    last_log_at: Option<Instant>,
}

impl FrameCounter {
    /// Record one drawn (non-skipped) frame. Returns `Some(total)` when
    /// at least a second has passed since the last reported log point
    /// (or this is the first call ever), `None` otherwise. The count
    /// itself always advances regardless of the return value.
    fn record_draw(&mut self, now: Instant) -> Option<u64> {
        self.drawn += 1;
        let should_log = match self.last_log_at {
            None => true,
            Some(t) => now.duration_since(t) >= Duration::from_secs(1),
        };
        if should_log {
            self.last_log_at = Some(now);
            Some(self.drawn)
        } else {
            None
        }
    }
}

/// Wires the `EMTERM_RENDER_PERF` gate to [`FrameCounter`]: a no-op that
/// never touches `counter` when `enabled` is `false` (AC-6's "no
/// counting side effects" half), otherwise delegates to
/// `FrameCounter::record_draw`. Kept separate from `WindowHost::render`
/// so both halves of AC-6 are unit-testable without a window.
fn record_drawn_frame(enabled: bool, counter: &mut FrameCounter, now: Instant) -> Option<u64> {
    if !enabled {
        return None;
    }
    counter.record_draw(now)
}

/// Rows-rebuilt counter for `EMTERM_RENDER_PERF=1` (task0003 FR6-half /
/// AC-5). Same idiom as [`FrameCounter`]: accumulates every rebuilt row
/// and reports the running total at most once per second of activity, so
/// an idle host doesn't flood `emterm.log`.
#[derive(Debug, Default)]
struct RowsRebuiltCounter {
    rebuilt: u64,
    last_log_at: Option<Instant>,
}

impl RowsRebuiltCounter {
    /// Record `rows` freshly rebuilt rows. Returns `Some(total)` when at
    /// least a second has passed since the last reported log point (or
    /// this is the first call ever), `None` otherwise. The running total
    /// always advances regardless of the return value.
    fn record_rebuilt(&mut self, rows: u64, now: Instant) -> Option<u64> {
        self.rebuilt += rows;
        let should_log = match self.last_log_at {
            None => true,
            Some(t) => now.duration_since(t) >= Duration::from_secs(1),
        };
        if should_log {
            self.last_log_at = Some(now);
            Some(self.rebuilt)
        } else {
            None
        }
    }
}

/// Wires the `EMTERM_RENDER_PERF` gate to [`RowsRebuiltCounter`]: a no-op
/// that never touches `counter` when `enabled` is `false` (AC-5's "no
/// side effects when unset" half) or when `rows == 0` (a fully
/// cache-served frame has nothing to report), otherwise delegates to
/// `RowsRebuiltCounter::record_rebuilt`. Kept separate from
/// `WindowHost::render` so both halves of AC-5 are unit-testable without a
/// window, mirroring `record_drawn_frame`.
fn record_rebuilt_rows(
    enabled: bool,
    counter: &mut RowsRebuiltCounter,
    rows: u64,
    now: Instant,
) -> Option<u64> {
    if !enabled || rows == 0 {
        return None;
    }
    counter.record_rebuilt(rows, now)
}

/// Translate a winit `MouseButton` to its `egui::PointerButton`
/// equivalent. Returns `None` for buttons egui does not model (e.g.
/// extra side buttons).
fn winit_to_egui_button(b: MouseButton) -> Option<egui::PointerButton> {
    match b {
        MouseButton::Left => Some(egui::PointerButton::Primary),
        MouseButton::Right => Some(egui::PointerButton::Secondary),
        MouseButton::Middle => Some(egui::PointerButton::Middle),
        _ => None,
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
fn detect_osc8_link_at(
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

/// Translate a winit logical key into the `egui::Key` consumed by
/// `crate::ui::keybinds::dispatch` and `handle_special_chord`. Returns
/// `None` for keys that no chord can reference (the caller falls through
/// to PTY input).
///
/// The mapped set covers every main key the settings-driven keybind
/// parser can produce (`parse_main_key`): ASCII letters / digits, the
/// symbol keys, the navigation / editing named keys, and F1..F12.
fn winit_key_to_egui(logical: &WinitKey) -> Option<egui::Key> {
    match logical {
        WinitKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_alphabetic() {
                // Allocation-free mapping — avoids a heap String per keystroke.
                return match lower {
                    'a' => Some(egui::Key::A),
                    'b' => Some(egui::Key::B),
                    'c' => Some(egui::Key::C),
                    'd' => Some(egui::Key::D),
                    'e' => Some(egui::Key::E),
                    'f' => Some(egui::Key::F),
                    'g' => Some(egui::Key::G),
                    'h' => Some(egui::Key::H),
                    'i' => Some(egui::Key::I),
                    'j' => Some(egui::Key::J),
                    'k' => Some(egui::Key::K),
                    'l' => Some(egui::Key::L),
                    'm' => Some(egui::Key::M),
                    'n' => Some(egui::Key::N),
                    'o' => Some(egui::Key::O),
                    'p' => Some(egui::Key::P),
                    'q' => Some(egui::Key::Q),
                    'r' => Some(egui::Key::R),
                    's' => Some(egui::Key::S),
                    't' => Some(egui::Key::T),
                    'u' => Some(egui::Key::U),
                    'v' => Some(egui::Key::V),
                    'w' => Some(egui::Key::W),
                    'x' => Some(egui::Key::X),
                    'y' => Some(egui::Key::Y),
                    'z' => Some(egui::Key::Z),
                    _ => None,
                };
            }
            match lower {
                '0' => Some(egui::Key::Num0),
                '1' => Some(egui::Key::Num1),
                '2' => Some(egui::Key::Num2),
                '3' => Some(egui::Key::Num3),
                '4' => Some(egui::Key::Num4),
                '5' => Some(egui::Key::Num5),
                '6' => Some(egui::Key::Num6),
                '7' => Some(egui::Key::Num7),
                '8' => Some(egui::Key::Num8),
                '9' => Some(egui::Key::Num9),
                '+' => Some(egui::Key::Plus),
                '-' => Some(egui::Key::Minus),
                ',' => Some(egui::Key::Comma),
                '.' => Some(egui::Key::Period),
                '/' => Some(egui::Key::Slash),
                '\\' => Some(egui::Key::Backslash),
                '=' => Some(egui::Key::Equals),
                ';' => Some(egui::Key::Semicolon),
                ':' => Some(egui::Key::Colon),
                _ => None,
            }
        }
        WinitKey::Named(named) => match named {
            NamedKey::Tab => Some(egui::Key::Tab),
            NamedKey::PageUp => Some(egui::Key::PageUp),
            NamedKey::PageDown => Some(egui::Key::PageDown),
            NamedKey::Home => Some(egui::Key::Home),
            NamedKey::End => Some(egui::Key::End),
            NamedKey::ArrowUp => Some(egui::Key::ArrowUp),
            NamedKey::ArrowDown => Some(egui::Key::ArrowDown),
            NamedKey::ArrowLeft => Some(egui::Key::ArrowLeft),
            NamedKey::ArrowRight => Some(egui::Key::ArrowRight),
            NamedKey::Enter => Some(egui::Key::Enter),
            NamedKey::Escape => Some(egui::Key::Escape),
            NamedKey::Backspace => Some(egui::Key::Backspace),
            NamedKey::Delete => Some(egui::Key::Delete),
            NamedKey::Insert => Some(egui::Key::Insert),
            NamedKey::Space => Some(egui::Key::Space),
            NamedKey::F1 => Some(egui::Key::F1),
            NamedKey::F2 => Some(egui::Key::F2),
            NamedKey::F3 => Some(egui::Key::F3),
            NamedKey::F4 => Some(egui::Key::F4),
            NamedKey::F5 => Some(egui::Key::F5),
            NamedKey::F6 => Some(egui::Key::F6),
            NamedKey::F7 => Some(egui::Key::F7),
            NamedKey::F8 => Some(egui::Key::F8),
            NamedKey::F9 => Some(egui::Key::F9),
            NamedKey::F10 => Some(egui::Key::F10),
            NamedKey::F11 => Some(egui::Key::F11),
            NamedKey::F12 => Some(egui::Key::F12),
            // F13–F20 are accepted by parse_main_key in keybinds.rs;
            // extend here so a configured F13–F20 chord can reach dispatch
            // at runtime instead of silently falling through to PTY input.
            NamedKey::F13 => Some(egui::Key::F13),
            NamedKey::F14 => Some(egui::Key::F14),
            NamedKey::F15 => Some(egui::Key::F15),
            NamedKey::F16 => Some(egui::Key::F16),
            NamedKey::F17 => Some(egui::Key::F17),
            NamedKey::F18 => Some(egui::Key::F18),
            NamedKey::F19 => Some(egui::Key::F19),
            NamedKey::F20 => Some(egui::Key::F20),
            _ => None,
        },
        _ => None,
    }
}

/// Extract the OS-level physical key / scan code from a winit `KeyEvent`.
/// Phase 4-G-A captures it into [`RawKeyEvent`] so any future IME backend
/// can stash the original scan code without re-querying winit internals.
///
/// winit does not expose the raw scancode publicly on every platform, so
/// we hash the `PhysicalKey` debug representation as a stable stand-in.
/// The exact value is opaque to the App; backends that actually need a
/// real X11 keycode reconstruct it from their own platform layer. The
/// Phase 4-G-3 `WinitImeBridge` ignores this field — winit hands `KeyEvent`
/// directly through `dispatch_key_event_via_ime` if/when needed.
fn winit_physical_key_code(event: &KeyEvent) -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    format!("{:?}", event.physical_key).hash(&mut h);
    h.finish() as u32
}

/// Synthetic key press gate (task0002, IMPLEMENTATION.md Shared Components
/// "Synthetic key press gate"). Winit flags a `KeyboardInput` event
/// `is_synthetic` when it is generated internally rather than from a real
/// hardware press — notably X11 `FocusIn` replays of keys already held down,
/// which produced the stray-`q`-class bugs (see project memory
/// `project_stray_q_xwayland_synthetic_press`). Returns `true` when the
/// event must be dropped before any state mutation, keybinding dispatch, IME
/// forwarding, or PTY write. Applies identically at both call sites (the
/// `Pressed` and `Released` `KeyboardInput` arms): a synthetic release is
/// dropped by the same rule as a synthetic press.
fn should_drop_synthetic_key_event(is_synthetic: bool) -> bool {
    is_synthetic
}

/// Translate a winit `KeyEvent` into the PoC's `(Key, Modifiers)` pair and
/// produce the PTY byte sequence. Returns `None` for events that should be
/// ignored (e.g. modifier-only presses).
///
/// On winit the printable text of a key press is exposed via
/// `KeyEvent::text` (already UTF-8). For non-chord plain text (no
/// Ctrl/Alt held) we forward that string verbatim so layout-specific
/// glyphs, dead-key composition results, and shifted symbols all reach
/// the PTY. For chords (Ctrl+C, Alt+b) we go through the `encode`
/// path with the named-key dispatch table.
/// `skk_mode`: whether this press is the bare `Ctrl+J` chord that must be
/// withheld from the PTY. Emacs-style IMEs (SKK) bind `Ctrl+J` for mode
/// switching; without the skip the chord encodes to LF (`0x0A`) and inserts
/// unwanted newlines. Mirrors the WebView build's keyboard-handler skip
/// (`src/terminal-app/handlers/keyboard.ts`): Ctrl held, no Alt/Shift, key
/// `j` (case-insensitive).
fn is_skk_swallowed_chord(logical_key: &WinitKey, mods: Modifiers) -> bool {
    mods.ctrl
        && !mods.alt
        && !mods.shift
        && matches!(logical_key, WinitKey::Character(s) if s.eq_ignore_ascii_case("j"))
}

/// Outcome of the [`shift_enter_rewrite`] decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShiftEnterRewrite {
    /// Not a bare Shift+Enter press (or the behavior is out of scope):
    /// encode normally with the original modifiers.
    Unchanged,
    /// Encode normally after substituting these modifiers for the
    /// original ones (`none` drops Shift; `alt_enter` drops Shift and
    /// sets Alt).
    Modifiers(Modifiers),
    /// Bypass the key encoder and write this literal byte sequence
    /// (`kitty_csi_u`, `lf`).
    RawBytes(&'static [u8]),
}

/// Literal Kitty keyboard protocol CSI u sequence for Enter (Unicode key
/// code 13) with the Shift modifier (xterm modifier parameter 2):
/// `ESC [ 1 3 ; 2 u`. See task0001 design D1.
const KITTY_CSI_U_SHIFT_ENTER: [u8; 7] = [0x1B, b'[', b'1', b'3', b';', b'2', b'u'];

/// Literal single-byte line feed (0x0a) emitted for `lf`. See task0001
/// design D1.
const LF_SHIFT_ENTER: [u8; 1] = [0x0A];

/// Pure decision table for the `shift_enter_behavior` key rewrite
/// (task0001 design D1). `is_enter` / `mods` describe the pressed key;
/// the call site only reaches this after UI-layer handlers (search bar,
/// keybind dispatch, SKK swallow) have already run. Rewrite applies only
/// when the modifier state is exactly Shift (no Ctrl, no Alt).
fn shift_enter_rewrite(
    is_enter: bool,
    mods: Modifiers,
    behavior: ShiftEnterBehavior,
) -> ShiftEnterRewrite {
    if !is_enter || !mods.shift || mods.ctrl || mods.alt {
        return ShiftEnterRewrite::Unchanged;
    }
    match behavior {
        ShiftEnterBehavior::None => ShiftEnterRewrite::Modifiers(Modifiers {
            shift: false,
            ..mods
        }),
        ShiftEnterBehavior::AltEnter => ShiftEnterRewrite::Modifiers(Modifiers {
            shift: false,
            alt: true,
            ..mods
        }),
        ShiftEnterBehavior::KittyCsiU => ShiftEnterRewrite::RawBytes(&KITTY_CSI_U_SHIFT_ENTER),
        ShiftEnterBehavior::Lf => ShiftEnterRewrite::RawBytes(&LF_SHIFT_ENTER),
    }
}

fn winit_key_to_bytes(event: &KeyEvent, mods: Modifiers, target: EncodeTarget) -> Option<Vec<u8>> {
    // Named keys take precedence over the printable fast path. winit on
    // Windows fills `event.text` for Backspace with `"\x7f"` (DEL); if we
    // routed that through the fast path the PTY would receive DEL, which
    // ConPTY converts to a `Backspace + Ctrl` INPUT_RECORD that PSReadLine
    // binds to BackwardKillWord — `ssh[BS]` then wipes the whole token.
    // Resolving named keys first sends 0x08 (BS, Ctrl+H) instead, which
    // ConPTY passes through as a plain Backspace.
    let named_key: Option<Key> = match &event.logical_key {
        WinitKey::Named(NamedKey::Enter) => Some(Key::Enter),
        WinitKey::Named(NamedKey::Tab) => Some(Key::Tab),
        WinitKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
        WinitKey::Named(NamedKey::Escape) => Some(Key::Escape),
        WinitKey::Named(NamedKey::ArrowUp) => Some(Key::Up),
        WinitKey::Named(NamedKey::ArrowDown) => Some(Key::Down),
        WinitKey::Named(NamedKey::ArrowLeft) => Some(Key::Left),
        WinitKey::Named(NamedKey::ArrowRight) => Some(Key::Right),
        WinitKey::Named(NamedKey::Home) => Some(Key::Home),
        WinitKey::Named(NamedKey::End) => Some(Key::End),
        WinitKey::Named(NamedKey::PageUp) => Some(Key::PageUp),
        WinitKey::Named(NamedKey::PageDown) => Some(Key::PageDown),
        WinitKey::Named(NamedKey::Delete) => Some(Key::Delete),
        WinitKey::Named(NamedKey::Insert) => Some(Key::Insert),
        WinitKey::Named(NamedKey::F1) => Some(Key::F(1)),
        WinitKey::Named(NamedKey::F2) => Some(Key::F(2)),
        WinitKey::Named(NamedKey::F3) => Some(Key::F(3)),
        WinitKey::Named(NamedKey::F4) => Some(Key::F(4)),
        WinitKey::Named(NamedKey::F5) => Some(Key::F(5)),
        WinitKey::Named(NamedKey::F6) => Some(Key::F(6)),
        WinitKey::Named(NamedKey::F7) => Some(Key::F(7)),
        WinitKey::Named(NamedKey::F8) => Some(Key::F(8)),
        WinitKey::Named(NamedKey::F9) => Some(Key::F(9)),
        WinitKey::Named(NamedKey::F10) => Some(Key::F(10)),
        WinitKey::Named(NamedKey::F11) => Some(Key::F(11)),
        WinitKey::Named(NamedKey::F12) => Some(Key::F(12)),
        _ => None,
    };
    if let Some(key) = named_key {
        let bytes = encode(key, mods, target);
        return if bytes.is_empty() { None } else { Some(bytes) };
    }

    // Fast path for plain printable text — winit already accounts for the
    // current keyboard layout (X11 / Wayland / Win32). When IME is
    // composing, winit suppresses `text` and routes the result via
    // `WindowEvent::Ime` instead, so this branch never double-delivers.
    if !mods.ctrl && !mods.alt {
        if let Some(text) = &event.text {
            if !text.is_empty() {
                return Some(text.as_bytes().to_vec());
            }
        }
    }

    let key = match &event.logical_key {
        WinitKey::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            Key::Char(c)
        }
        WinitKey::Named(NamedKey::Space) => Key::Char(' '),
        _ => return None,
    };
    let bytes = encode(key, mods, target);
    if bytes.is_empty() { None } else { Some(bytes) }
}

/// Upper bound for a single wheel event's arrow-key emission; protects against runaway/non-finite delta inputs.
const MAX_ALT_SCROLL_NOTCHES: u32 = 100;

/// Accumulate a fractional wheel delta `lines` into `acc` and return
/// `(consumed_whole, new_accum)`. `consumed_whole` is the integer
/// portion of the new total (the "ready to fire" line count); `new_accum`
/// is the leftover fractional remainder the caller should store back.
/// Both signs are preserved: a downward scroll accumulates a negative
/// whole and returns a negative `consumed_whole`.
fn accumulate_alt_scroll_lines(acc: f32, lines: f32) -> (f32, f32) {
    let new_acc = acc + lines;
    let whole = if new_acc >= 0.0 {
        new_acc.floor()
    } else {
        new_acc.ceil()
    };
    let frac = new_acc - whole;
    (whole, frac)
}

/// FR1 (DECSET 1007): compute the PTY bytes to emit for one wheel
/// event, or `None` when the gates do not let alternate-scroll
/// translation fire (the caller then falls back to the existing
/// scrollback-view branch). All three gates must be ON: AltScreen is
/// active, the terminal-side `MODE_ALTERNATE_SCROLL` bit is set, and
/// the user setting `alternate_scroll_enabled` is true. `lines` is the
/// y-axis wheel delta in cell rows (positive = wheel-up). Sub-notch
/// fractional pixel deltas (|lines| < 1.0) are treated as no-ops to
/// match a discrete wheel click. xterm convention: 3 arrow bytes per
/// notch, Shift modifier is intentionally ignored at the call site.
fn alternate_scroll_wheel_bytes(
    lines: f32,
    alt_screen: bool,
    mode_bit_on: bool,
    setting_on: bool,
) -> Option<Vec<u8>> {
    if !lines.is_finite() {
        return None;
    }
    if !alt_screen || !mode_bit_on || !setting_on {
        return None;
    }
    let notches = (lines.abs().floor() as u32).min(MAX_ALT_SCROLL_NOTCHES);
    if notches == 0 {
        return None;
    }
    let arrow: &[u8] = if lines > 0.0 { b"\x1b[A" } else { b"\x1b[B" };
    let count = (notches as usize) * 3;
    let mut buf = Vec::with_capacity(arrow.len() * count);
    for _ in 0..count {
        buf.extend_from_slice(arrow);
    }
    Some(buf)
}

/// `ApplicationHandler` impl driving the App + WindowHost on winit 0.30.
///
/// winit 0.30 replaced the closure-based event-loop API with the
/// `ApplicationHandler` trait. `resumed` creates the window the first
/// time the platform is ready, `window_event` mirrors what used to be
/// the inner `match event` arm, and `about_to_wait` does the periodic
/// pump (PTY drain, IME pump, cursor-rect notification) that the old
/// `StartCause::Poll` path handled.
struct PocApp {
    app: App,
    host: Option<WindowHost>,
}

impl ApplicationHandler for PocApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.host.is_some() {
            // Re-entering `Resumed` is an Android lifecycle artifact;
            // desktop winit only fires this once at startup. Keep the
            // existing host so a stray resume does not reinitialize the
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
        // unconditionally rearming a 16 ms `WaitUntil`.
        event_loop.set_control_flow(control_flow_for(&self.app));
        self.host = Some(host);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
            WindowEvent::Resized(new_size) => {
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
            WindowEvent::CursorLeft { .. } => {
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
                    host.window.set_cursor(CursorIcon::Default);
                }
                // Drop any link-hover underline + hand cursor when the
                // pointer leaves the window.
                host.invalidate_link_hover();
            }
            WindowEvent::CursorMoved { position, .. } => {
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
            WindowEvent::MouseInput { state, button, .. } => {
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
                        .inner_size()
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
                        // (refreshed on the CursorMoved that brought us
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
                // `PointerMoved` events forwarded on `CursorMoved`, so the
                // wheel only reaches the strip when the pointer is over it.
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
                            .inner_size()
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
                        // drop the cached hover so the next CursorMoved
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
                    // it; the next CursorMoved re-detects.
                    host.invalidate_link_hover();
                    host.window().request_redraw();
                } else if lines < 0.0 {
                    self.app.scroll_down_by(step);
                    host.invalidate_link_hover();
                    host.window().request_redraw();
                }
            }
            WindowEvent::HoveredFile(_path) => {
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
            WindowEvent::HoveredFileCancelled => {
                self.app.sftp_ui.hover = None;
                host.window().request_redraw();
            }
            WindowEvent::DroppedFile(path) => {
                // Accumulate one path; `about_to_wait` finalizes the batch on
                // the next loop turn (winit gives no drop-complete signal).
                self.app.sftp_ui.hover = None;
                self.app.sftp_ui.aggregator.push(path);
                host.window().request_redraw();
            }
            WindowEvent::RedrawRequested => {
                host.render(&mut self.app);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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
        // Finalize a drag-drop gesture once per loop turn: winit delivers each
        // dropped file as a separate `DroppedFile` event with no completion
        // signal, so the per-file paths accumulated since the last turn are
        // dispatched here as a single batch (upload on SSH tabs, paste on
        // non-SSH tabs).
        if self.app.sftp_ui.aggregator.is_armed() {
            if let Some(batch) = self.app.sftp_ui.aggregator.take_batch() {
                self.app.dispatch_drop(batch);
                host.window().request_redraw();
            }
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
        if ime_changed || pty_changed || search_changed || blink_due || bell_due || toast_due {
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
        event_loop.set_control_flow(control_flow_for(&self.app));
    }

    /// Phase E (TS-32): winit `EventLoopProxy::send_event(())` calls land
    /// here. Without this override, the trait-default `user_event` is a
    /// no-op and the provider-owned wake chain (`TimeProvider` timer
    /// thread → `WakeFn` → `EventLoopProxy::send_event(())` → here →
    /// `request_redraw`) is silently broken, freezing the status-bar
    /// clock when the shell is idle.
    ///
    /// Defensive: if `self.host` is `None` (we are between
    /// `Resumed`-time construction failure and process exit, or already
    /// torn down in `CloseRequested`), this is a no-op.
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        request_redraw_on_user_event(self.host.as_ref(), |host| {
            host.window().request_redraw();
        });
    }

    /// winit calls this once after `event_loop.exit()` and before
    /// `run_app` returns. Use it as a defense-in-depth shutdown step
    /// for any code path that flagged exit without zeroing `self.host`
    /// (e.g. future error-path exits). The Vulkan / X11 teardown must
    /// happen while EventLoop is still alive — see the field-order
    /// note on `WindowHost`.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
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
fn request_redraw_on_user_event<H, F>(host: Option<&H>, redraw: F)
where
    F: FnOnce(&H),
{
    if let Some(h) = host {
        redraw(h);
    }
}

/// Run the event loop until the window is closed. Owns the App.
pub fn run(event_loop: EventLoop<()>, app: App) -> ! {
    let mut handler = PocApp { app, host: None };
    if let Err(e) = event_loop.run_app(&mut handler) {
        log::error!("native-poc: winit event loop returned an error: {e}");
    }
    // Drop the PTY-owning tabs explicitly before the process exits so
    // reader/writer threads can shut down cleanly; without this they
    // outlive `main` and produce noisy platform-specific cleanup
    // warnings.
    drop(handler);
    std::process::exit(0);
}

/// Intercept Phase 4 chords. Returns `true` when the event was consumed
/// (the generic encoder should not run).
///
/// `egui_key` is the pre-computed result of `winit_key_to_egui` for this
/// event, passed in so the caller can reuse the same value for the
/// keybinds dispatch path without a second translation.
fn handle_special_chord(
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

/// Convert the PTY-side [`Modifiers`] (`input::Modifiers`) into the
/// `egui::Modifiers` shape egui events / `RawInput` expect. `command` /
/// `mac_cmd` are always false — native-poc targets Linux + Windows only.
fn input_mods_to_egui(mods: Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        ctrl: mods.ctrl,
        shift: mods.shift,
        alt: mods.alt,
        command: false,
        mac_cmd: false,
    }
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
fn handle_profile_selector_key(event: &KeyEvent, app: &mut App) {
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
        WinitKey::Named(NamedKey::Enter) | WinitKey::Named(NamedKey::Space) => {
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
fn handle_mux_dialog_key(event: &KeyEvent, mods: Modifiers, host: &mut WindowHost) {
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

fn handle_search_key(event: &KeyEvent, mods: Modifiers, host: &mut WindowHost, app: &mut App) {
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
mod tests {
    use super::*;
    use crate::ui::chrome::build_egui_fonts;
    use std::time::Duration;

    // ── task0006 AC-2: grid x-origin carries no sidebar term ───────────

    /// Regression guard for the right-edge placement update:
    /// `cell_metrics_px`'s `origin_x` computation must not read the
    /// persistent mux-sidebar inset — only `grid_size`'s usable-WIDTH
    /// computation may. Scans each function's own source text so a future
    /// edit that moves the sidebar term back onto `origin_x` fails loudly.
    #[test]
    fn cell_metrics_px_origin_x_has_no_sidebar_term() {
        let src = include_str!("window_host.rs");
        let start = src
            .find("fn cell_metrics_px(&self, app: &App)")
            .expect("marker `fn cell_metrics_px` not found in window_host.rs");
        let body = &src[start..];
        let end = body
            .find("\n    pub fn grid_size(")
            .expect("`cell_metrics_px` should be immediately followed by `grid_size`");
        let cell_metrics_px_src = &body[..end];
        // Target the specific inset code terms rather than the bare word
        // "sidebar" — the function's own explanatory comment legitimately
        // mentions the sidebar in prose (documenting why there is no term).
        for needle in [
            "sidebar_inset",
            "mux_sidebar_inset_logical",
            "mux_sidebar_grid_inset",
        ] {
            assert!(
                !cell_metrics_px_src.contains(needle),
                "cell_metrics_px's origin_x must contain no sidebar term \
                 (AC-2): found `{needle}` — the grid x-origin must be \
                 identical with and without the persistent sidebar; only \
                 grid_size's usable-width computation may read the \
                 sidebar inset"
            );
        }
    }

    // ── task0002 AC-5: should_skip_frame pure decision ───────────────

    /// AC-5: `Some(0)` dirty AND status bar unchanged AND no overlay work
    /// AND no pending egui input → skip.
    #[test]
    fn should_skip_frame_when_no_dirty_rows_and_status_bar_unchanged() {
        assert!(should_skip_frame(Some(0), false, false, false));
    }

    /// AC-5: dirty rows present (even with an unchanged status bar and no
    /// overlay work) → never skip.
    #[test]
    fn should_skip_frame_false_when_dirty_rows_present() {
        assert!(!should_skip_frame(Some(3), false, false, false));
    }

    /// AC-5: status bar changed (even with zero dirty rows and no overlay
    /// work) → never skip — this is the carve-out that keeps the clock /
    /// git-branch / OSC 777 wake chain alive on an otherwise-idle shell.
    #[test]
    fn should_skip_frame_false_when_status_bar_changed() {
        assert!(!should_skip_frame(Some(0), true, false, false));
    }

    /// AC-5: no active tab (`None`) → never skip; the hint-message frame
    /// must still draw.
    #[test]
    fn should_skip_frame_false_when_no_active_tab() {
        assert!(!should_skip_frame(None, false, false, false));
    }

    /// Overlay work pending (a toast counting down or a visual-bell flash
    /// still decaying), even with zero dirty rows and an unchanged status
    /// bar, must never skip — otherwise the 60 Hz wake `about_to_wait`
    /// schedules while a toast/bell is active spins uselessly without the
    /// egui pass ever running `pump_sftp` / the toast prune / the bell
    /// paint.
    #[test]
    fn should_skip_frame_false_when_overlay_work_pending() {
        assert!(!should_skip_frame(Some(0), false, true, false));
    }

    /// task0005 AC-2: the search UI being visible must also veto the skip,
    /// exercised through the same `overlay_work` parameter as the toast /
    /// bell carve-out above (the call site ORs `App::search_visible()` into
    /// it).
    #[test]
    fn should_skip_frame_false_when_search_visible() {
        assert!(!should_skip_frame(Some(0), false, true, false));
    }

    // ── toast_redraw_due pure decision ──────────────────────────────────

    /// No active toast → no toast-driven redraw, regardless of when the
    /// last one fired.
    #[test]
    fn toast_redraw_due_false_when_no_toast() {
        let now = Instant::now() + Duration::from_secs(10);
        assert!(!toast_redraw_due(false, None, now));
        assert!(!toast_redraw_due(false, Some(now), now));
    }

    /// First request for a freshly armed toast fires immediately (no
    /// previous toast-driven redraw recorded).
    #[test]
    fn toast_redraw_due_true_on_first_request() {
        assert!(toast_redraw_due(true, None, Instant::now()));
    }

    /// Within the poll interval of the previous toast-driven redraw the
    /// request is suppressed — this is what keeps the redraw →
    /// `about_to_wait` cycle from spinning at full speed while a toast is
    /// up (the egui pass would otherwise consume the toast's lifetime at
    /// frame-rate speed; with the old `time: None` frame-counter clock
    /// that dismissed a 4 s toast almost instantly).
    #[test]
    fn toast_redraw_due_false_within_poll_interval() {
        let now = Instant::now() + Duration::from_secs(10);
        let last = now - Duration::from_millis(crate::app::TOAST_POLL_MS / 2);
        assert!(!toast_redraw_due(true, Some(last), now));
    }

    /// Once the poll interval has elapsed the next request fires, keeping
    /// the toast's prune cadence at ~`TOAST_POLL_MS`.
    #[test]
    fn toast_redraw_due_true_after_poll_interval() {
        let now = Instant::now() + Duration::from_secs(10);
        let last = now - Duration::from_millis(crate::app::TOAST_POLL_MS);
        assert!(toast_redraw_due(true, Some(last), now));
    }

    // ── has_actionable_egui_input pure decision ─────────────────────────

    /// A `PointerMoved`-only queue (plain mouse-move hover over the
    /// terminal body, no button held) must NOT be actionable — this is
    /// the fix that lets an idle terminal skip the frame while the mouse
    /// hovers over it.
    #[test]
    fn has_actionable_egui_input_false_for_pointer_moved_only() {
        let events = vec![egui::Event::PointerMoved(egui::pos2(1.0, 2.0))];
        assert!(!has_actionable_egui_input(&events, false));
    }

    /// A queue containing a `PointerButton` (the discrete event a click
    /// delivers after its leading `PointerMoved`) must be actionable, so
    /// click latency is unaffected by the `PointerMoved` exclusion above.
    #[test]
    fn has_actionable_egui_input_true_with_pointer_button() {
        let events = vec![
            egui::Event::PointerMoved(egui::pos2(1.0, 2.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(1.0, 2.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ];
        assert!(has_actionable_egui_input(&events, false));
    }

    /// An empty queue (no egui input arrived this frame) must not be
    /// actionable — even mid-drag (a held button with no new motion needs
    /// no frame).
    #[test]
    fn has_actionable_egui_input_false_when_empty() {
        assert!(!has_actionable_egui_input(&[], false));
        assert!(!has_actionable_egui_input(&[], true));
    }

    /// While a pointer button is held, motion alone IS actionable: egui
    /// chrome drags (scrollbar thumb, tab reorder) are driven purely by
    /// the press→release motion stream, and skipping those frames would
    /// freeze the drag's live tracking over an idle grid.
    #[test]
    fn has_actionable_egui_input_true_for_motion_while_button_held() {
        let events = vec![egui::Event::PointerMoved(egui::pos2(1.0, 2.0))];
        assert!(has_actionable_egui_input(&events, true));
    }

    /// Post-merge regression fix: undrained egui input (a tab-bar click,
    /// wheel over the chrome, a search-box key) must veto the skip even on
    /// a fully idle grid — `build_raw_input` is the only drain and runs
    /// after this decision, so skipping would park the click until the
    /// next unrelated wakeup (worst case a blink flip, ~530 ms of
    /// perceived tab-switch lag).
    #[test]
    fn should_skip_frame_false_when_egui_input_pending() {
        assert!(!should_skip_frame(Some(0), false, false, true));
    }

    // ── post-merge regression fix: resolve_build_dirty_rows ─────────────

    /// A full redraw raised mid-frame (tab switch / scrollbar jump applied
    /// from this frame's egui pass) invalidates the frame-top snapshot:
    /// the build must widen to every row (`None` routes both build
    /// branches to their existing full-rebuild path).
    #[test]
    fn resolve_build_dirty_rows_widens_to_full_when_flag_pending() {
        assert_eq!(resolve_build_dirty_rows(Some(vec![3, 7]), true), None);
    }

    /// No mid-frame invalidation → the snapshot is trusted as-is (the
    /// ordinary cached path keeps its dirty-rows-only rebuild).
    #[test]
    fn resolve_build_dirty_rows_keeps_snapshot_when_no_flag() {
        assert_eq!(
            resolve_build_dirty_rows(Some(vec![3, 7]), false),
            Some(vec![3, 7])
        );
    }

    /// An absent snapshot (forced full redraw path, `was_surface_dirty`)
    /// stays absent regardless of the flag.
    #[test]
    fn resolve_build_dirty_rows_none_snapshot_stays_none() {
        assert_eq!(resolve_build_dirty_rows(None, false), None);
        assert_eq!(resolve_build_dirty_rows(None, true), None);
    }

    // ── task0006: should_rotate_row_cache_for_scroll_event pure decision ──

    /// A pending scroll event on the ordinary cached path (dirty rows
    /// captured this turn) must rotate the cache.
    #[test]
    fn should_rotate_row_cache_for_scroll_event_true_on_cached_path() {
        assert!(should_rotate_row_cache_for_scroll_event(1, true));
    }

    /// A turn whose effective dirty set is already every row (forced full
    /// redraw, fold layout, or a scrolled-back viewport reacting to new
    /// output) must NOT rotate — every row rebuilds from scratch
    /// regardless, so rotating first would just be overwritten (task0006
    /// Design: "needs_full_redraw frames: full rebuild already; just
    /// clear the event").
    #[test]
    fn should_rotate_row_cache_for_scroll_event_false_on_full_redraw() {
        assert!(!should_rotate_row_cache_for_scroll_event(1, false));
    }

    /// No pending scroll event (`scroll_count == 0`) never rotates, even
    /// on the cached path.
    #[test]
    fn should_rotate_row_cache_for_scroll_event_false_when_no_event() {
        assert!(!should_rotate_row_cache_for_scroll_event(0, true));
    }

    /// Neither a pending event nor the cached path → false (defensive
    /// combination; never actually reached since the call site only
    /// calls this inside `scroll_count > 0`).
    #[test]
    fn should_rotate_row_cache_for_scroll_event_false_when_neither() {
        assert!(!should_rotate_row_cache_for_scroll_event(0, false));
    }

    // ── task0005 AC-1: hover_link_cells_changed pure decision ─────────

    /// AC-1: a link span appearing (empty → non-empty) counts as a change.
    #[test]
    fn hover_link_cells_changed_true_on_appear() {
        assert!(hover_link_cells_changed(&[], &[(3, 5, 9)]));
    }

    /// AC-1: a link span moving (different cell range) counts as a change.
    #[test]
    fn hover_link_cells_changed_true_on_move() {
        assert!(hover_link_cells_changed(&[(3, 5, 9)], &[(3, 10, 14)]));
    }

    /// AC-1: a link span disappearing (non-empty → empty) counts as a
    /// change.
    #[test]
    fn hover_link_cells_changed_true_on_disappear() {
        assert!(hover_link_cells_changed(&[(3, 5, 9)], &[]));
    }

    /// AC-1: an unchanged span (hover-stable idle frame) must not be
    /// reported as a change, so the idle-skip path stays honest.
    #[test]
    fn hover_link_cells_changed_false_when_unchanged() {
        assert!(!hover_link_cells_changed(&[(3, 5, 9)], &[(3, 5, 9)]));
        assert!(!hover_link_cells_changed(&[], &[]));
    }

    // ── task0004 AC-1/AC-2: next_wait_deadline pure decision ──────────

    /// AC-2: nothing pending → `None` (the caller maps this to
    /// `ControlFlow::Wait`) — an idle terminal never reschedules a
    /// periodic wakeup.
    #[test]
    fn next_wait_deadline_none_when_nothing_pending() {
        assert_eq!(next_wait_deadline(None, None, None), None);
    }

    /// AC-1: only the blink deadline is pending → that deadline wins.
    #[test]
    fn next_wait_deadline_blink_only() {
        let t = Instant::now() + Duration::from_millis(530);
        assert_eq!(next_wait_deadline(Some(t), None, None), Some(t));
    }

    /// AC-1: only the bell deadline is pending → that deadline wins.
    #[test]
    fn next_wait_deadline_bell_only() {
        let t = Instant::now() + Duration::from_millis(150);
        assert_eq!(next_wait_deadline(None, Some(t), None), Some(t));
    }

    /// AC-1: only the toast deadline is pending → that deadline wins.
    #[test]
    fn next_wait_deadline_toast_only() {
        let t = Instant::now() + Duration::from_millis(16);
        assert_eq!(next_wait_deadline(None, None, Some(t)), Some(t));
    }

    /// AC-1: blink and bell both pending, blink is the sooner deadline →
    /// the nearer (blink) deadline wins.
    #[test]
    fn next_wait_deadline_picks_sooner_of_blink_and_bell() {
        let now = Instant::now();
        let sooner = now + Duration::from_millis(50);
        let later = now + Duration::from_millis(500);
        assert_eq!(
            next_wait_deadline(Some(sooner), Some(later), None),
            Some(sooner)
        );
        // Order of arguments must not matter — the later one is bell here.
        assert_eq!(
            next_wait_deadline(Some(later), Some(sooner), None),
            Some(sooner)
        );
    }

    /// AC-1: all three concerns pending → the earliest of the three wins.
    #[test]
    fn next_wait_deadline_picks_earliest_of_all_three() {
        let now = Instant::now();
        let blink = now + Duration::from_millis(500);
        let bell = now + Duration::from_millis(10);
        let toast = now + Duration::from_millis(16);
        assert_eq!(
            next_wait_deadline(Some(blink), Some(bell), Some(toast)),
            Some(bell)
        );
    }

    // ── task0002 AC-6: EMTERM_RENDER_PERF frame counter ──────────────

    /// AC-6: the first recorded frame always logs (no prior log point).
    #[test]
    fn frame_counter_logs_first_frame_immediately() {
        let mut counter = FrameCounter::default();
        let now = Instant::now();
        assert_eq!(counter.record_draw(now), Some(1));
    }

    /// AC-6: a second frame within the same one-second window still
    /// counts but does not re-log.
    #[test]
    fn frame_counter_suppresses_log_within_one_second_window() {
        let mut counter = FrameCounter::default();
        let t0 = Instant::now();
        assert_eq!(counter.record_draw(t0), Some(1));
        let t1 = t0 + Duration::from_millis(500);
        assert_eq!(counter.record_draw(t1), None);
        assert_eq!(counter.drawn, 2, "count must still advance without logging");
    }

    /// AC-6: once a full second has elapsed since the last log, the next
    /// drawn frame logs again with the updated running total.
    #[test]
    fn frame_counter_logs_again_after_one_second_elapsed() {
        let mut counter = FrameCounter::default();
        let t0 = Instant::now();
        assert_eq!(counter.record_draw(t0), Some(1));
        let t1 = t0 + Duration::from_secs(1);
        assert_eq!(counter.record_draw(t1), Some(2));
    }

    /// AC-6: with the gate disabled, `record_drawn_frame` never touches
    /// the counter — "no counting side effects occur" when
    /// `EMTERM_RENDER_PERF` is unset.
    #[test]
    fn record_drawn_frame_disabled_never_touches_counter() {
        let mut counter = FrameCounter::default();
        let now = Instant::now();
        assert_eq!(record_drawn_frame(false, &mut counter, now), None);
        assert_eq!(counter.drawn, 0, "disabled gate must not count frames");
    }

    /// AC-6: with the gate enabled, `record_drawn_frame` delegates to
    /// the counter and surfaces its log payload.
    #[test]
    fn record_drawn_frame_enabled_delegates_to_counter() {
        let mut counter = FrameCounter::default();
        let now = Instant::now();
        assert_eq!(record_drawn_frame(true, &mut counter, now), Some(1));
        assert_eq!(counter.drawn, 1);
    }

    // ── task0003 AC-5: EMTERM_RENDER_PERF rows-rebuilt counter ────────

    /// AC-5: the first recorded batch always logs (no prior log point).
    #[test]
    fn rows_rebuilt_counter_logs_first_batch_immediately() {
        let mut counter = RowsRebuiltCounter::default();
        let now = Instant::now();
        assert_eq!(counter.record_rebuilt(3, now), Some(3));
    }

    /// AC-5: a second batch within the same one-second window still
    /// accumulates but does not re-log.
    #[test]
    fn rows_rebuilt_counter_suppresses_log_within_one_second_window() {
        let mut counter = RowsRebuiltCounter::default();
        let t0 = Instant::now();
        assert_eq!(counter.record_rebuilt(3, t0), Some(3));
        let t1 = t0 + Duration::from_millis(500);
        assert_eq!(counter.record_rebuilt(2, t1), None);
        assert_eq!(
            counter.rebuilt, 5,
            "total must still advance without logging"
        );
    }

    /// AC-5: once a full second has elapsed since the last log, the next
    /// rebuilt batch logs again with the updated running total.
    #[test]
    fn rows_rebuilt_counter_logs_again_after_one_second_elapsed() {
        let mut counter = RowsRebuiltCounter::default();
        let t0 = Instant::now();
        assert_eq!(counter.record_rebuilt(1, t0), Some(1));
        let t1 = t0 + Duration::from_secs(1);
        assert_eq!(counter.record_rebuilt(1, t1), Some(2));
    }

    /// AC-5: with the gate disabled, `record_rebuilt_rows` never touches
    /// the counter — "no side effects" when `EMTERM_RENDER_PERF` is unset.
    #[test]
    fn record_rebuilt_rows_disabled_never_touches_counter() {
        let mut counter = RowsRebuiltCounter::default();
        let now = Instant::now();
        assert_eq!(record_rebuilt_rows(false, &mut counter, 5, now), None);
        assert_eq!(counter.rebuilt, 0, "disabled gate must not count rows");
    }

    /// AC-3/AC-5: a stable (fully cache-served) frame reports zero rebuilt
    /// rows; even with the gate enabled this must not touch the counter
    /// (nothing meaningful to log on a frame with no rebuild work).
    #[test]
    fn record_rebuilt_rows_enabled_with_zero_rows_never_touches_counter() {
        let mut counter = RowsRebuiltCounter::default();
        let now = Instant::now();
        assert_eq!(record_rebuilt_rows(true, &mut counter, 0, now), None);
        assert_eq!(counter.rebuilt, 0);
    }

    /// AC-5: with the gate enabled, `record_rebuilt_rows` delegates to the
    /// counter and surfaces its log payload.
    #[test]
    fn record_rebuilt_rows_enabled_delegates_to_counter() {
        let mut counter = RowsRebuiltCounter::default();
        let now = Instant::now();
        assert_eq!(record_rebuilt_rows(true, &mut counter, 4, now), Some(4));
        assert_eq!(counter.rebuilt, 4);
    }

    // ── skk_mode: bare Ctrl+J swallow ────────────────────────────────

    // ── FR3 (OSC 8 hyperlink) detect_osc8_link_at helper ─────

    /// TS-19: cell carries a safe `http://` OSC 8 URI → `Some(link)`
    /// with `LinkKind::Url(uri)` and the cell range covering the run.
    #[test]
    fn fr3_osc8_safe_uri_returns_link_with_run() {
        let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
        // Open OSC 8 with safe URI, write 5 chars, close OSC 8, then
        // a few non-hyperlinked chars.
        core.process_pty_data(b"\x1b]8;;https://example.com/pr/1\x07Hello\x1b]8;;\x07world");

        let link = detect_osc8_link_at(&core, 0, 2).expect("hover on 'l' (col 2) should hit");
        match &link.kind {
            crate::links::LinkKind::Url(u) => assert_eq!(u, "https://example.com/pr/1"),
            other => panic!("expected Url, got {other:?}"),
        }
        // The whole run (cols 0..5 inclusive-exclusive) underlines.
        assert_eq!(link.cells, vec![(0u16, 0u16, 5u16)]);
    }

    /// TS-20: cell carries an unsafe `javascript:` URI → `None` (and a
    /// `warn` log line, not asserted here).
    #[test]
    fn fr3_osc8_unsafe_uri_returns_none() {
        let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
        core.process_pty_data(b"\x1b]8;;javascript:alert(1)\x07x\x1b]8;;\x07");
        assert_eq!(detect_osc8_link_at(&core, 0, 0), None);
    }

    /// TS-21: cell with `hyperlink_id == 0` (no OSC 8 marker) → `None`.
    #[test]
    fn fr3_osc8_plain_cell_returns_none() {
        let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
        // No OSC 8 at all — just plain text.
        core.process_pty_data(b"plain text");
        assert_eq!(detect_osc8_link_at(&core, 0, 0), None);
        assert_eq!(detect_osc8_link_at(&core, 0, 3), None);
    }

    /// TS-22: cell has a non-zero hyperlink_id but the URI is missing
    /// from the table → `None`. Synthesize this by writing a cell with
    /// a stale id via direct table manipulation. Falls back to a
    /// process-cleared scenario: the helper sees `get_hyperlink_uri()`
    /// return an empty string and returns `None`.
    #[test]
    fn fr3_osc8_missing_uri_returns_none() {
        let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
        // Real-world reproduction is hard without internal accessors;
        // instead we lean on the documented behaviour of
        // `get_hyperlink_uri()` returning empty when the id is missing.
        // Set up a hyperlink, then call detect on an unrelated cell
        // whose id is 0 — that's TS-21. To exercise the empty-URI
        // branch specifically, use an OSC 8 with an empty URI string
        // (also documented to be treated as "no link" per SPEC edge
        // cases).
        core.process_pty_data(b"\x1b]8;;\x07x\x1b]8;;\x07");
        assert_eq!(detect_osc8_link_at(&core, 0, 0), None);
    }

    /// FR3: out-of-bounds cell coordinates → `None`.
    #[test]
    fn fr3_osc8_out_of_bounds_returns_none() {
        let core = term_core::terminal_core::TerminalCore::new(80, 24, 0);
        assert_eq!(detect_osc8_link_at(&core, 100, 0), None);
        assert_eq!(detect_osc8_link_at(&core, 0, 100), None);
    }

    /// FR3: hover on a cell in the middle of a 5-cell OSC 8 run yields
    /// the run that starts at col 0 and extends to col 5.
    #[test]
    fn fr3_osc8_run_expansion_from_middle_cell() {
        let mut core = term_core::terminal_core::TerminalCore::new(80, 24, 100);
        core.process_pty_data(b"\x1b]8;;https://example.com\x07Click\x1b]8;;\x07");
        // Hover the last cell of the run.
        let link = detect_osc8_link_at(&core, 0, 4).expect("hover on 'k' should hit");
        assert_eq!(link.cells, vec![(0u16, 0u16, 5u16)]);
    }

    // ── FR1 (DECSET 1007) wheel → arrow bytes ────────────────

    /// TS-3: AltScreen + mode bit + setting all ON + wheel-up 1 notch
    /// emits three `ESC[A` bytes (xterm: 3 arrows per notch).
    #[test]
    fn fr1_wheel_up_in_alt_screen_emits_three_arrow_up() {
        let bytes = alternate_scroll_wheel_bytes(1.0, true, true, true);
        assert_eq!(bytes.as_deref(), Some(b"\x1b[A\x1b[A\x1b[A".as_slice()));
    }

    /// FR1: wheel-down emits `ESC[B` instead of `ESC[A`.
    #[test]
    fn fr1_wheel_down_in_alt_screen_emits_three_arrow_down() {
        let bytes = alternate_scroll_wheel_bytes(-1.0, true, true, true);
        assert_eq!(bytes.as_deref(), Some(b"\x1b[B\x1b[B\x1b[B".as_slice()));
    }

    /// FR1: notch count scales the byte count (2 notches → 6 arrows).
    #[test]
    fn fr1_wheel_scales_with_notches() {
        let bytes = alternate_scroll_wheel_bytes(2.0, true, true, true);
        assert_eq!(
            bytes.as_deref(),
            Some(b"\x1b[A\x1b[A\x1b[A\x1b[A\x1b[A\x1b[A".as_slice())
        );
    }

    /// TS-4: same gates as TS-3 but the user setting is OFF; the
    /// helper declines so the caller falls through to scrollback.
    #[test]
    fn fr1_wheel_suppressed_when_setting_off() {
        assert_eq!(alternate_scroll_wheel_bytes(1.0, true, true, false), None);
    }

    /// TS-5: the terminal-side mode bit (DECSET 1007) is OFF; helper
    /// declines.
    #[test]
    fn fr1_wheel_suppressed_when_mode_bit_off() {
        assert_eq!(alternate_scroll_wheel_bytes(1.0, true, false, true), None);
    }

    /// TS-6: AltScreen is OFF (normal screen); helper always declines
    /// so the existing scrollback-view wheel path runs unchanged.
    #[test]
    fn fr1_wheel_inert_outside_alt_screen() {
        assert_eq!(alternate_scroll_wheel_bytes(1.0, false, true, true), None);
        assert_eq!(alternate_scroll_wheel_bytes(-1.0, false, true, true), None);
    }

    /// FR1 edge case: sub-notch pixel deltas (|lines| < 1) round to 0
    /// notches and are treated as no-ops. Without this guard a tiny
    /// drift would send a stream of arrow bytes per pixel of motion.
    #[test]
    fn fr1_wheel_sub_notch_pixel_delta_is_noop() {
        assert_eq!(alternate_scroll_wheel_bytes(0.4, true, true, true), None);
        assert_eq!(alternate_scroll_wheel_bytes(-0.4, true, true, true), None);
    }

    // ── task0010 AC-2/AC-3: mux sidebar wheel-routing guard wiring ─────

    /// Regression guard: the `MouseWheel` handler must query
    /// `ui::mux_sidebar::point_in_sidebar` (the shared hit-region
    /// derivation task0010 introduces) and `return` early on a hit, BEFORE
    /// it reaches the terminal scroll path — this is what makes AC-2 ("the
    /// terminal scroll path is skipped": no scrollback movement, no
    /// AltScreen arrow bytes, no alt-scroll accumulator change) true, and
    /// what makes AC-3 (byte-identical behavior everywhere the helper
    /// returns `false`) hold — the branch does nothing but query-and-maybe-
    /// return, so a `false` answer falls through to the untouched code
    /// below unconditionally. Source-scans the `MouseWheel` arm's body the
    /// same way `cell_metrics_px_origin_x_has_no_sidebar_term` guards
    /// `cell_metrics_px`'s origin math: the correctness of the DECISION
    /// itself (which points are "inside" the sidebar) is exercised by
    /// `ui::mux_sidebar::tests::ac1_*` / `ac4_*`; this test pins the
    /// STRUCTURAL property that wires that decision to the right place in
    /// the winit handler. Pixel-level scroll feel is manual (M-4, per the
    /// task plan's Test Notes).
    #[test]
    fn mouse_wheel_handler_routes_sidebar_hits_to_egui_before_the_terminal_scroll_path() {
        let src = include_str!("window_host.rs");
        let start = src
            .find("WindowEvent::MouseWheel { delta, .. } =>")
            .expect("MouseWheel arm not found in window_host.rs");
        let body = &src[start..];
        let sidebar_guard_pos = body.find("mux_sidebar::point_in_sidebar").expect(
            "MouseWheel handler must query ui::mux_sidebar::point_in_sidebar (AC-4: the \
                 shared hit-region derivation, not a re-derived guard)",
        );
        let terminal_scroll_pos = body
            .find("let lines = match delta {")
            .expect("terminal scroll path marker (`let lines = match delta {`) not found");
        assert!(
            sidebar_guard_pos < terminal_scroll_pos,
            "the sidebar hit-region guard must run BEFORE the terminal scroll path so a hit \
             skips scrollback / AltScreen-arrow movement (AC-2)"
        );
        let between_guard_and_scroll = &body[sidebar_guard_pos..terminal_scroll_pos];
        assert!(
            between_guard_and_scroll.contains("return;"),
            "the sidebar hit-region guard must `return` on a hit so the terminal scroll path \
             is genuinely skipped, not merely forwarded-then-continued (AC-2)"
        );
    }

    // ── task0011 AC-1/AC-3/AC-4: mux sidebar press-suppression guard ───

    /// AC-1/AC-3: the MouseInput handler's Pressed-edge suppression guard
    /// (the same `if button == MouseButton::Left && state ==
    /// ElementState::Pressed` block that already covers the bottom status
    /// bar and the scrollbar) must query the shared
    /// `ui::mux_sidebar::point_in_sidebar` helper and `return` on a hit —
    /// this is what makes a press on the overlay card (zero grid inset, so
    /// the old persistent-only width test missed it) stop before the
    /// selection-start arm, while keeping the guard scoped to the Pressed
    /// edge only (a drag that started inside the terminal still gets its
    /// Released event processed normally, since this block never runs for
    /// `ElementState::Released`). Source-scans the way
    /// `mouse_wheel_handler_routes_sidebar_hits_to_egui_before_the_terminal_scroll_path`
    /// does; the geometric correctness of "is this point inside the
    /// sidebar" is exercised by `ui::mux_sidebar::tests::ac1_*`/`ac4_*`.
    /// AC-2 (overlay closed / local tab: selection starts as before)
    /// follows from `point_in_sidebar` answering `false` there — pinned by
    /// `ui::mux_sidebar::tests` (`visible_placement: None` returns
    /// `false` unconditionally), so the guard here is a complete no-op in
    /// that case and this test does not re-derive that coverage.
    #[test]
    fn mouse_input_press_guard_queries_shared_sidebar_hit_region_before_selection_start() {
        let src = include_str!("window_host.rs");
        let arm_start = src
            .find("WindowEvent::MouseInput { state, button, .. } =>")
            .expect("MouseInput arm not found in window_host.rs");
        let arm_body = &src[arm_start..];
        let guard_start = arm_body
            .find("// Same rule for the bottom status-bar panel")
            .expect("bottom-strip/scrollbar/sidebar press guard comment not found");
        let guard_end = arm_body
            .find("// While the profile-selector modal is up")
            .expect("profile-selector guard marker not found after the press guard");
        let guard_section = &arm_body[guard_start..guard_end];
        assert!(
            guard_section
                .contains("if button == MouseButton::Left && state == ElementState::Pressed {"),
            "the sidebar press guard must stay inside the Pressed-edge-only conditional \
             shared with the bottom-strip/scrollbar guards (AC-3)"
        );
        let sidebar_guard_pos = guard_section.find("mux_sidebar::point_in_sidebar(").expect(
            "MouseInput's press guard must query ui::mux_sidebar::point_in_sidebar \
             (AC-4: the shared hit-region derivation, not a re-derived guard)",
        );
        assert!(
            guard_section.contains("return;"),
            "the sidebar press guard must `return` on a hit so the selection-start arm \
             is genuinely skipped (AC-1)"
        );
        let selection_start_pos = arm_body
            .find("(MouseButton::Left, ElementState::Pressed) => {")
            .expect("selection-start arm not found in the MouseInput handler");
        assert!(
            guard_start + sidebar_guard_pos < selection_start_pos,
            "the sidebar hit-region guard must run BEFORE the selection-start arm so a hit \
             on the overlay card never starts a terminal selection (AC-1)"
        );
    }

    /// AC-4: the press guard and the wheel guard both resolve the sidebar
    /// region through `ui::mux_sidebar::point_in_sidebar` — neither
    /// independently re-derives the sidebar's geometry (e.g. by calling
    /// `sidebar_width` directly), which is exactly the class of drift the
    /// round-2 scrollbar click-guard regression came from
    /// (IMPLEMENTATION.md decision 3.5).
    #[test]
    fn press_and_wheel_guards_share_the_same_sidebar_hit_region_helper() {
        let src = include_str!("window_host.rs");
        let press_start = src
            .find("WindowEvent::MouseInput { state, button, .. } =>")
            .expect("MouseInput arm not found in window_host.rs");
        let wheel_start = src
            .find("WindowEvent::MouseWheel { delta, .. } =>")
            .expect("MouseWheel arm not found in window_host.rs");
        assert!(
            press_start < wheel_start,
            "expected the MouseInput arm to appear before the MouseWheel arm"
        );
        let press_body = &src[press_start..wheel_start];
        assert!(
            press_body.contains("mux_sidebar::point_in_sidebar("),
            "MouseInput press guard must call the shared hit-region helper"
        );
        assert!(
            !press_body.contains("mux_sidebar::sidebar_width("),
            "MouseInput press guard must not re-derive the sidebar width itself"
        );
        let wheel_body = &src[wheel_start..];
        let wheel_arm_end = wheel_body
            .find("let lines = match delta {")
            .expect("terminal scroll path marker not found after the MouseWheel guard");
        let wheel_guard_section = &wheel_body[..wheel_arm_end];
        assert!(
            wheel_guard_section.contains("mux_sidebar::point_in_sidebar("),
            "MouseWheel guard must call the shared hit-region helper"
        );
        assert!(
            !wheel_guard_section.contains("mux_sidebar::sidebar_width("),
            "MouseWheel guard must not re-derive the sidebar width itself"
        );
    }

    // ── skk_mode: bare Ctrl+J swallow ────────────────────────────────

    // ── preedit_effective_dirty_rows: row-cache invalidation during IME
    //    preedit (fix for the stale/blank-row High finding) ─────────────

    /// The anchor row is force-included even when `term_core`'s own dirty
    /// set is empty, and the row below it (composition wrap) too — the
    /// core bug this fixes: without this, `row_cache` would never learn
    /// about the row the composition overlays while term_core considers
    /// it clean.
    #[test]
    fn preedit_dirty_rows_forces_anchor_and_next_row() {
        let rows = preedit_effective_dirty_rows(Some(vec![]), 24, 5);
        assert_eq!(rows, vec![5, 6]);
    }

    /// `None` (a forced full redraw) still expands to the full row range
    /// with the anchor rows folded in (already present, so no duplicates).
    #[test]
    fn preedit_dirty_rows_none_means_full_redraw() {
        let rows = preedit_effective_dirty_rows(None, 4, 1);
        assert_eq!(rows, vec![0, 1, 2, 3]);
    }

    /// An anchor row already present in term_core's dirty set is not
    /// duplicated, and the existing dirty rows are preserved alongside it.
    #[test]
    fn preedit_dirty_rows_merges_without_duplicates() {
        let rows = preedit_effective_dirty_rows(Some(vec![2, 5]), 24, 5);
        assert_eq!(rows, vec![2, 5, 6]);
    }

    /// The anchor row's "next row" (wrap case) is clamped at the grid
    /// bottom — no out-of-range row index is ever produced.
    #[test]
    fn preedit_dirty_rows_clamps_anchor_at_last_row() {
        let rows = preedit_effective_dirty_rows(Some(vec![]), 24, 23);
        assert_eq!(rows, vec![23]);
    }

    #[test]
    fn skk_chord_matches_bare_ctrl_j_case_insensitive() {
        let ctrl = Modifiers {
            ctrl: true,
            shift: false,
            alt: false,
        };
        assert!(is_skk_swallowed_chord(
            &WinitKey::Character("j".into()),
            ctrl
        ));
        assert!(is_skk_swallowed_chord(
            &WinitKey::Character("J".into()),
            ctrl
        ));
    }

    #[test]
    fn skk_chord_rejects_extra_mods_and_other_keys() {
        let ctrl = Modifiers {
            ctrl: true,
            shift: false,
            alt: false,
        };
        // Extra modifiers — the WebView skip requires Ctrl alone.
        assert!(!is_skk_swallowed_chord(
            &WinitKey::Character("j".into()),
            Modifiers {
                shift: true,
                ..ctrl
            }
        ));
        assert!(!is_skk_swallowed_chord(
            &WinitKey::Character("j".into()),
            Modifiers { alt: true, ..ctrl }
        ));
        // No Ctrl at all.
        assert!(!is_skk_swallowed_chord(
            &WinitKey::Character("j".into()),
            Modifiers::NONE
        ));
        // Other keys keep flowing to the PTY encoder.
        assert!(!is_skk_swallowed_chord(
            &WinitKey::Character("k".into()),
            ctrl
        ));
        assert!(!is_skk_swallowed_chord(
            &WinitKey::Named(NamedKey::Enter),
            ctrl
        ));
    }

    // ── task0001: shift_enter_rewrite pure decision (AC-3 / AC-4) ──────

    #[test]
    fn shift_enter_rewrite_none_drops_shift_and_encodes_plain_enter() {
        // AC-3: `none` -> the plain Enter encoding (Shift dropped, no Alt).
        let mods = Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
        };
        let rewrite = shift_enter_rewrite(true, mods, ShiftEnterBehavior::None);
        assert_eq!(
            rewrite,
            ShiftEnterRewrite::Modifiers(Modifiers {
                shift: false,
                ctrl: false,
                alt: false,
            })
        );
    }

    #[test]
    fn shift_enter_rewrite_alt_enter_drops_shift_and_sets_alt() {
        // AC-3: `alt_enter` -> the Alt+Enter encoding.
        let mods = Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
        };
        let rewrite = shift_enter_rewrite(true, mods, ShiftEnterBehavior::AltEnter);
        assert_eq!(
            rewrite,
            ShiftEnterRewrite::Modifiers(Modifiers {
                shift: false,
                ctrl: false,
                alt: true,
            })
        );
    }

    #[test]
    fn shift_enter_rewrite_kitty_csi_u_emits_exact_raw_bytes() {
        // AC-3: `kitty_csi_u` -> the exact bytes
        // 0x1B 0x5B 0x31 0x33 0x3B 0x32 0x75, independent of host-PTY vs
        // mux encode target (the raw-bytes path bypasses the encoder
        // entirely, so the target never enters this decision).
        let mods = Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
        };
        let rewrite = shift_enter_rewrite(true, mods, ShiftEnterBehavior::KittyCsiU);
        match rewrite {
            ShiftEnterRewrite::RawBytes(bytes) => {
                assert_eq!(bytes, &[0x1B, 0x5B, 0x31, 0x33, 0x3B, 0x32, 0x75]);
            }
            other => panic!("expected RawBytes, got {other:?}"),
        }
    }

    #[test]
    fn shift_enter_rewrite_lf_emits_exact_raw_byte() {
        // AC-1 (task0001): `lf` -> the exact single byte 0x0a, independent
        // of host-PTY vs mux encode target (the raw-bytes path bypasses
        // the encoder entirely, so the target never enters this decision).
        let mods = Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
        };
        let rewrite = shift_enter_rewrite(true, mods, ShiftEnterBehavior::Lf);
        match rewrite {
            ShiftEnterRewrite::RawBytes(bytes) => {
                assert_eq!(bytes, &[0x0A]);
            }
            other => panic!("expected RawBytes, got {other:?}"),
        }
    }

    #[test]
    fn shift_enter_rewrite_unchanged_when_ctrl_held() {
        // AC-4: Enter with Ctrl+Shift is not rewritten under any value.
        let mods = Modifiers {
            shift: true,
            ctrl: true,
            alt: false,
        };
        for behavior in [
            ShiftEnterBehavior::None,
            ShiftEnterBehavior::AltEnter,
            ShiftEnterBehavior::KittyCsiU,
            ShiftEnterBehavior::Lf,
        ] {
            assert_eq!(
                shift_enter_rewrite(true, mods, behavior),
                ShiftEnterRewrite::Unchanged
            );
        }
    }

    #[test]
    fn shift_enter_rewrite_unchanged_when_alt_already_held() {
        // AC-4: Enter with Alt (Shift+Alt) is not rewritten under any value.
        let mods = Modifiers {
            shift: true,
            ctrl: false,
            alt: true,
        };
        for behavior in [
            ShiftEnterBehavior::None,
            ShiftEnterBehavior::AltEnter,
            ShiftEnterBehavior::KittyCsiU,
            ShiftEnterBehavior::Lf,
        ] {
            assert_eq!(
                shift_enter_rewrite(true, mods, behavior),
                ShiftEnterRewrite::Unchanged
            );
        }
    }

    #[test]
    fn shift_enter_rewrite_unchanged_when_plain_ctrl_enter_no_shift() {
        // AC-4: Enter with Ctrl (no Shift) is not rewritten under any value.
        let mods = Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
        };
        for behavior in [
            ShiftEnterBehavior::None,
            ShiftEnterBehavior::AltEnter,
            ShiftEnterBehavior::KittyCsiU,
            ShiftEnterBehavior::Lf,
        ] {
            assert_eq!(
                shift_enter_rewrite(true, mods, behavior),
                ShiftEnterRewrite::Unchanged
            );
        }
    }

    #[test]
    fn shift_enter_rewrite_unchanged_when_not_enter_key() {
        // Bare Shift on a non-Enter key is never rewritten.
        let mods = Modifiers {
            shift: true,
            ctrl: false,
            alt: false,
        };
        assert_eq!(
            shift_enter_rewrite(false, mods, ShiftEnterBehavior::KittyCsiU),
            ShiftEnterRewrite::Unchanged
        );
        assert_eq!(
            shift_enter_rewrite(false, mods, ShiftEnterBehavior::Lf),
            ShiftEnterRewrite::Unchanged
        );
    }

    // ── task0002: synthetic key press gate (AC-1 / AC-2) ──────────────

    #[test]
    fn synthetic_key_press_gate_drops_synthetic_press() {
        // AC-1: a synthetic Pressed event must be gated (dropped) so it
        // never reaches keybinding dispatch or a PTY write.
        assert!(should_drop_synthetic_key_event(true));
    }

    #[test]
    fn synthetic_key_press_gate_drops_synthetic_release() {
        // AC-1 (Released arm): the same predicate governs the Released
        // arm — a synthetic release is dropped by the same gate (design
        // note in IMPLEMENTATION.md Shared Components). The gate does not
        // take press/release state, so a synthetic flag alone is enough
        // to prove the release arm is covered too.
        assert!(should_drop_synthetic_key_event(true));
    }

    #[test]
    fn synthetic_key_press_gate_allows_non_synthetic_press() {
        // AC-2 (Pressed arm): a non-synthetic press is processed exactly
        // as before — the gate must not drop it.
        assert!(!should_drop_synthetic_key_event(false));
    }

    #[test]
    fn synthetic_key_press_gate_allows_non_synthetic_release() {
        // AC-2 (Released arm): a non-synthetic release is processed
        // exactly as before — the gate must not drop it.
        assert!(!should_drop_synthetic_key_event(false));
    }

    #[test]
    fn egui_fonts_empty_ui_font_keeps_default_proportional_head() {
        let fonts = build_egui_fonts("", "");
        assert!(!fonts.font_data.contains_key("EmtermUiFont"));
        assert!(!fonts.font_data.contains_key("EmtermTerminalFont"));
        // Bundled CJK / emoji fallbacks are appended to both chains.
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let chain = &fonts.families[&family];
            assert!(chain.iter().any(|n| n == "EmtermBundledCJK"));
            assert!(chain.iter().any(|n| n == "EmtermBundledEmoji"));
            assert!(chain.iter().any(|n| n == "EmtermBundledSymbols"));
            // …but never as the primary face.
            assert_ne!(chain[0], "EmtermBundledCJK");
        }
        // Empty terminal font → Monospace HEAD falls back to bundled
        // Inconsolata (mirrors the terminal grid's BUNDLED_BASE_FONT
        // behavior). Without this, chrome would render on egui's
        // bundled Hack while the grid renders on Inconsolata.
        assert_eq!(
            fonts.families[&egui::FontFamily::Monospace][0],
            "EmtermBundledBase"
        );
        // The bundled base is Monospace-only — it must not leak into
        // Proportional (the tab-bar / title-bar font).
        assert!(
            fonts.families[&egui::FontFamily::Proportional]
                .iter()
                .all(|n| n != "EmtermBundledBase")
        );
    }

    #[test]
    fn egui_fonts_unknown_ui_font_falls_back_to_default() {
        let fonts = build_egui_fonts("Emterm No Such Font Family 9000", "");
        assert!(!fonts.font_data.contains_key("EmtermUiFont"));
        let prop = &fonts.families[&egui::FontFamily::Proportional];
        assert_ne!(prop[0], "EmtermUiFont");
    }

    #[test]
    fn egui_fonts_unknown_terminal_font_falls_back_to_default() {
        let fonts = build_egui_fonts("", "Emterm No Such Terminal Font 9000");
        assert!(!fonts.font_data.contains_key("EmtermTerminalFont"));
        let mono = &fonts.families[&egui::FontFamily::Monospace];
        assert_ne!(mono[0], "EmtermTerminalFont");
    }

    #[test]
    fn egui_fonts_known_ui_font_prepends_to_proportional_only() {
        // Resolve a family that actually exists on this host via the
        // same fontdb scan the production path uses; skip silently on
        // fontless CI hosts.
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let Some(family) = db
            .faces()
            .flat_map(|f| f.families.first())
            .map(|(name, _)| name.clone())
            .next()
        else {
            return;
        };
        let fonts = build_egui_fonts(&family, "");
        assert!(
            fonts.font_data.contains_key("EmtermUiFont"),
            "host family {family:?} should load"
        );
        assert_eq!(
            fonts.families[&egui::FontFamily::Proportional][0],
            "EmtermUiFont"
        );
        // Monospace mirrors --terminal-font-family in the WebView build
        // and must not pick up the UI font.
        assert!(
            fonts.families[&egui::FontFamily::Monospace]
                .iter()
                .all(|n| n != "EmtermUiFont")
        );
    }

    #[test]
    fn egui_fonts_known_terminal_font_prepends_to_monospace_only() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let Some(family) = db
            .faces()
            .flat_map(|f| f.families.first())
            .map(|(name, _)| name.clone())
            .next()
        else {
            return;
        };
        let fonts = build_egui_fonts("", &family);
        assert!(
            fonts.font_data.contains_key("EmtermTerminalFont"),
            "host family {family:?} should load"
        );
        assert_eq!(
            fonts.families[&egui::FontFamily::Monospace][0],
            "EmtermTerminalFont"
        );
        // The terminal font must not leak into Proportional (that
        // chain is skinned by --ui-font-family).
        assert!(
            fonts.families[&egui::FontFamily::Proportional]
                .iter()
                .all(|n| n != "EmtermTerminalFont")
        );
    }

    #[test]
    fn click_classifier_single_click_is_character() {
        let mut t = ClickTracker::default();
        let now = Instant::now();
        let cls = t.classify(now, 5, 10);
        assert_eq!(cls.count, 1);
        assert_eq!(cls.mode, SelectionMode::Character);
    }

    #[test]
    fn click_classifier_double_click_within_window_at_same_cell() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        let t1 = t0 + Duration::from_millis(200);
        let cls = t.classify(t1, 5, 10);
        assert_eq!(cls.count, 2);
        assert_eq!(cls.mode, SelectionMode::Word);
    }

    #[test]
    fn click_classifier_triple_click_at_same_cell() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        let _ = t.classify(t0 + Duration::from_millis(100), 5, 10);
        let cls = t.classify(t0 + Duration::from_millis(200), 5, 10);
        assert_eq!(cls.count, 3);
        assert_eq!(cls.mode, SelectionMode::Line);
    }

    #[test]
    fn click_classifier_resets_after_triple() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        let _ = t.classify(t0 + Duration::from_millis(100), 5, 10);
        let _ = t.classify(t0 + Duration::from_millis(200), 5, 10);
        // Fourth click within window collapses back to Character.
        let cls = t.classify(t0 + Duration::from_millis(300), 5, 10);
        assert_eq!(cls.count, 1);
        assert_eq!(cls.mode, SelectionMode::Character);
    }

    #[test]
    fn click_classifier_resets_when_position_changes() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        let cls = t.classify(t0 + Duration::from_millis(100), 5, 11);
        // Different cell → back to single click.
        assert_eq!(cls.count, 1);
        assert_eq!(cls.mode, SelectionMode::Character);
    }

    #[test]
    fn click_classifier_resets_when_window_expires() {
        let mut t = ClickTracker::default();
        let t0 = Instant::now();
        let _ = t.classify(t0, 5, 10);
        // 600 ms > MULTI_CLICK_WINDOW_MS (500 ms) → reset.
        let cls = t.classify(t0 + Duration::from_millis(600), 5, 10);
        assert_eq!(cls.count, 1);
        assert_eq!(cls.mode, SelectionMode::Character);
    }

    /// TS-32 (host=Some): `PocApp::user_event` must call `request_redraw`
    /// on the active window exactly once. We exercise the extracted
    /// `request_redraw_on_user_event` helper because constructing a
    /// real `winit::Window` here would require an active event loop +
    /// display, which is unavailable in `cargo test`.
    ///
    /// A `Cell<u32>` counter stands in for the winit window's
    /// `request_redraw()` side effect. Without the `user_event`
    /// override the provider-owned wake chain (`WakeFn` →
    /// `EventLoopProxy::send_event(())` → `user_event`) was silently
    /// dropped, freezing the status-bar clock on idle (release-build
    /// regression observed twice during sdd.6-verify).
    #[test]
    fn user_event_dispatches_redraw_when_host_present() {
        use std::cell::Cell;
        let redraws: Cell<u32> = Cell::new(0);
        let host_stub: u8 = 0;
        request_redraw_on_user_event(Some(&host_stub), |_| {
            redraws.set(redraws.get() + 1);
        });
        assert_eq!(redraws.get(), 1);
    }

    #[test]
    fn resize_edge_interior_is_none() {
        // Dead-center of a 800×600 window: nowhere near any edge.
        assert_eq!(classify_resize_edge(800.0, 600.0, 400.0, 300.0, 6.0), None);
    }

    #[test]
    fn resize_edge_corners_classify_to_diagonals() {
        use ResizeDirection::*;
        // Each corner pixel grabs the diagonal direction so the user
        // can resize width + height together.
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 1.0, 1.0, 6.0),
            Some(NorthWest)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 799.0, 1.0, 6.0),
            Some(NorthEast)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 1.0, 599.0, 6.0),
            Some(SouthWest)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 799.0, 599.0, 6.0),
            Some(SouthEast)
        );
    }

    #[test]
    fn resize_edge_sides_classify_to_cardinals() {
        use ResizeDirection::*;
        // Mid-edge sample on each of the four sides.
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 400.0, 1.0, 6.0),
            Some(North)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 400.0, 599.0, 6.0),
            Some(South)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 1.0, 300.0, 6.0),
            Some(West)
        );
        assert_eq!(
            classify_resize_edge(800.0, 600.0, 799.0, 300.0, 6.0),
            Some(East)
        );
    }

    #[test]
    fn resize_edge_outside_window_is_none() {
        // Wayland can deliver negative or past-edge coords during
        // pointer leave; both must yield `None` so the hot-zone
        // cache doesn't latch a stale direction.
        assert_eq!(classify_resize_edge(800.0, 600.0, -1.0, 300.0, 6.0), None);
        assert_eq!(classify_resize_edge(800.0, 600.0, 400.0, 700.0, 6.0), None);
    }

    /// TS-32 (host=None): before `Resumed` constructs the `WindowHost`
    /// or after `CloseRequested` tears it down, `self.host` is `None`.
    /// In that window `user_event` must be a no-op rather than panic.
    #[test]
    fn user_event_is_noop_when_host_absent() {
        use std::cell::Cell;
        let redraws: Cell<u32> = Cell::new(0);
        let host: Option<&u8> = None;
        request_redraw_on_user_event(host, |_| {
            redraws.set(redraws.get() + 1);
        });
        assert_eq!(redraws.get(), 0);
    }

    /// Verify that winit_key_to_egui covers every function key F1..=F20.
    ///
    /// parse_main_key in keybinds.rs accepts F1..=F20 as valid chord keys.
    /// This test keeps the two domains in sync: if either side drifts, this
    /// test will catch it before a user-configured F13–F20 shortcut silently
    /// falls through to PTY input at runtime.
    #[test]
    fn winit_key_to_egui_covers_f1_through_f20() {
        let pairs: &[(WinitKey, egui::Key)] = &[
            (WinitKey::Named(NamedKey::F1), egui::Key::F1),
            (WinitKey::Named(NamedKey::F2), egui::Key::F2),
            (WinitKey::Named(NamedKey::F3), egui::Key::F3),
            (WinitKey::Named(NamedKey::F4), egui::Key::F4),
            (WinitKey::Named(NamedKey::F5), egui::Key::F5),
            (WinitKey::Named(NamedKey::F6), egui::Key::F6),
            (WinitKey::Named(NamedKey::F7), egui::Key::F7),
            (WinitKey::Named(NamedKey::F8), egui::Key::F8),
            (WinitKey::Named(NamedKey::F9), egui::Key::F9),
            (WinitKey::Named(NamedKey::F10), egui::Key::F10),
            (WinitKey::Named(NamedKey::F11), egui::Key::F11),
            (WinitKey::Named(NamedKey::F12), egui::Key::F12),
            (WinitKey::Named(NamedKey::F13), egui::Key::F13),
            (WinitKey::Named(NamedKey::F14), egui::Key::F14),
            (WinitKey::Named(NamedKey::F15), egui::Key::F15),
            (WinitKey::Named(NamedKey::F16), egui::Key::F16),
            (WinitKey::Named(NamedKey::F17), egui::Key::F17),
            (WinitKey::Named(NamedKey::F18), egui::Key::F18),
            (WinitKey::Named(NamedKey::F19), egui::Key::F19),
            (WinitKey::Named(NamedKey::F20), egui::Key::F20),
        ];
        for (winit_key, expected) in pairs {
            assert_eq!(
                winit_key_to_egui(winit_key),
                Some(*expected),
                "winit_key_to_egui({winit_key:?}) did not return {expected:?}"
            );
        }
    }

    // ── FR1 clamp + non-finite guard (Finding B) + accumulator (Finding A) ──

    /// Non-finite inputs (NaN, Infinity) must return None without
    /// panicking or triggering a runaway Vec allocation.
    #[test]
    fn alternate_scroll_wheel_bytes_rejects_non_finite() {
        assert_eq!(
            alternate_scroll_wheel_bytes(f32::NAN, true, true, true),
            None
        );
        assert_eq!(
            alternate_scroll_wheel_bytes(f32::INFINITY, true, true, true),
            None
        );
    }

    /// A huge positive delta is clamped to MAX_ALT_SCROLL_NOTCHES notches;
    /// the resulting Vec is never a multi-GB allocation.
    #[test]
    fn alternate_scroll_wheel_bytes_clamps_huge_delta() {
        let bytes = alternate_scroll_wheel_bytes(1.0e9, true, true, true).unwrap();
        // 3 bytes per arrow, 3 arrows per notch, at most MAX_ALT_SCROLL_NOTCHES notches.
        assert!(bytes.len() <= (MAX_ALT_SCROLL_NOTCHES as usize) * 3 * 3);
    }

    /// Four successive 0.3-line trackpad events accumulate: the first
    /// three resolve to 0.0 whole lines (no arrow fired), and on the
    /// fourth the accumulator crosses 1.0 and one notch is consumed
    /// with ~0.2 fractional remainder.
    #[test]
    fn accumulate_alt_scroll_lines_collects_sub_notch_deltas() {
        let (w, a) = accumulate_alt_scroll_lines(0.0, 0.3);
        assert_eq!(w, 0.0);
        assert!((a - 0.3).abs() < 1e-6, "after 1st event: accum={a}");

        let (w, a) = accumulate_alt_scroll_lines(a, 0.3);
        assert_eq!(w, 0.0);
        assert!((a - 0.6).abs() < 1e-6, "after 2nd event: accum={a}");

        let (w, a) = accumulate_alt_scroll_lines(a, 0.3);
        assert_eq!(w, 0.0);
        assert!((a - 0.9).abs() < 1e-6, "after 3rd event: accum={a}");

        let (w, a) = accumulate_alt_scroll_lines(a, 0.3);
        assert_eq!(w, 1.0, "4th event should yield one notch");
        assert!((a - 0.2).abs() < 1e-6, "4th event remainder={a}");
    }
}
