//! UI widgets and event types for the native-poc app shell.
//!
//! Phase 4-B introduces two enums:
//!
//! - [`TabEvent`] — emitted by [`tab_bar::draw`] when the user interacts
//!   with the tab strip (new / close / switch). At most one event per
//!   frame so the app loop can apply it atomically.
//! - [`AppAction`] — emitted by [`keybinds::dispatch`] when an incoming
//!   key chord matches a known global binding. `None` means the chord
//!   should fall through to the active PTY writer.
//!
//! Both enums are exhaustive over Phase 4 keybinds and tab UI
//! affordances; later phases (4-C prefix latch, 4-D status bar) extend
//! them rather than redefining them.

pub mod chrome;
pub mod emoji_cache;
pub mod keybinds;
pub mod md3;
pub mod md3_widgets;
pub mod profile_selector;
pub mod scrollbar;
pub mod search_bar;
pub mod status_bar;
pub mod tab_bar;
pub mod title_bar;

/// User intents originating from the global keybind layer.
///
/// The dispatcher in [`keybinds::dispatch`] is pure: it only inspects
/// `(egui::Modifiers, egui::Key)` and never touches application state.
/// The app loop is responsible for translating the resulting action
/// into mutations on the tabs vector (or PTY writes).
///
/// The variant set spans both tab-roster actions (resolved from
/// `settings.keybinds` or the built-in `Ctrl+Tab` / `Ctrl+1..9`
/// conventions) and the view-level actions ported from the WebView
/// build (`SelectAll`, the zoom trio, `ToggleFullscreen`,
/// `ToggleTabBar`). The latter need window / host resources, so the
/// keyboard handler dispatches them at the `window_host` layer rather
/// than through `App::apply_action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    /// Ctrl+Shift+T — spawn a new shell tab and switch to it.
    NewTab,
    /// Ctrl+Shift+W — close the currently active tab. If it is the
    /// last tab, the app loop translates this into `ExitWindow`.
    CloseTab,
    /// Ctrl+Tab — switch to the next tab, wrapping at the end.
    NextTab,
    /// Ctrl+Shift+Tab — switch to the previous tab, wrapping at the
    /// start.
    PrevTab,
    /// Ctrl+1..Ctrl+9 — jump to the Nth tab (1-based). The payload is
    /// the **1-based** index as typed by the user; the app loop clamps
    /// it to the existing tab range (`min(n - 1, tabs.len() - 1)`).
    JumpTab(u8),
    /// Ctrl+Shift+A — select the entire visible viewport of the active
    /// tab. Handled in `App::apply_action`.
    SelectAll,
    /// Ctrl+Shift+F — open (or re-focus) the in-terminal search overlay.
    /// Handled at the `window_host` layer because the overlay drives the
    /// per-frame key-forwarding state and the highlight render path.
    OpenSearch,
    /// Ctrl+Shift+ArrowUp — scroll to the nearest OSC 133 prompt mark
    /// above the current view top. Handled in `App::apply_action` via
    /// `App::jump_to_prompt`. Port of the WebView `handlePromptJump`.
    JumpToPrevPrompt,
    /// Ctrl+Shift+ArrowDown — scroll to the nearest OSC 133 prompt mark
    /// below the current view top. See [`AppAction::JumpToPrevPrompt`].
    JumpToNextPrompt,
    /// Ctrl+Plus — increase the terminal font size by one point
    /// (clamped). Handled at the `window_host` layer because the cell
    /// metrics + PTY grid must be reshaped after the change.
    ZoomIn,
    /// Ctrl+Minus — decrease the terminal font size by one point
    /// (clamped). See [`AppAction::ZoomIn`].
    ZoomOut,
    /// Ctrl+0 — reset the terminal font size back to the configured
    /// `settings.font_size`. See [`AppAction::ZoomIn`].
    ZoomReset,
    /// F11 — toggle borderless full-screen. Handled at the
    /// `window_host` layer (needs the `winit::window::Window` handle).
    ToggleFullscreen,
    /// Ctrl+Shift+B — toggle the tab bar's visibility. Handled at the
    /// `window_host` layer so the grid can be reshaped for the
    /// reclaimed / surrendered row.
    ToggleTabBar,
    /// Ctrl+, — open the settings window (child `--settings` process).
    /// Handled in `App::apply_action` via `App::open_settings_window`.
    /// Port of the WebView `open_settings` keybind.
    OpenSettings,
    /// Ctrl+Shift+G — spawn a new tab with the **global** settings,
    /// ignoring any default profile. Port of the WebView
    /// `new_tab_global` keybind. Checked before `NewTab` in the
    /// dispatcher so this chord wins when both keybinds resolve to the
    /// same combination (WebView parity).
    NewTabGlobal,
    /// Ctrl+Shift+P — open the modal profile selector. No-op when no
    /// profiles are configured. Port of the WebView `profile_selector`
    /// keybind.
    OpenProfileSelector,
}

/// User intents originating from the custom (client-side) title bar.
///
/// The widget itself is pure (no winit calls); the app loop owns the
/// `Window` handle and translates each variant into the corresponding
/// `winit::window::Window` method. `Close` does NOT exit directly —
/// the loop flips a `pending_close` flag and lets `about_to_wait`
/// drive the same teardown handshake used for the last-tab path so
/// the wgpu / X11 resources unwind in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarEvent {
    /// `_` button — minimize to the taskbar.
    Minimize,
    /// `□` button — toggle between maximized and the previous floating
    /// size. The widget does not track current state; the caller
    /// inspects `window.is_maximized()` to pick the destination.
    MaximizeToggle,
    /// `×` button — request window close. Triggers the same teardown
    /// path as the WM-supplied close button.
    Close,
    /// Primary-button press-and-drag over the title region. The
    /// caller forwards this to `Window::drag_window()` so the WM
    /// takes over the move loop.
    DragStart,
}

/// User intents originating from the tab bar widget.
///
/// One event per user interaction. The app loop applies the event
/// before the next frame; it is the widget's responsibility never to
/// emit multiple events for the same logical action (e.g. a single
/// click of the "+" button must produce exactly one `New`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabEvent {
    /// "+" button on the tab strip.
    New,
    /// Gear button on the tab strip's fixed area — open (or focus) the
    /// Settings tab. Mirrors the WebView `.tab-button-settings`.
    OpenSettings,
    /// Close ("×") button on the tab at `index` (0-based).
    Close(usize),
    /// Switch to the tab at `index` (0-based) via mouse click.
    Switch(usize),
    /// Drag-and-drop reorder: move the tab at `from` so that it lands at
    /// `to`. Both indices are 0-based and refer to the current roster;
    /// `to == from` and `to == from + 1` are no-ops (the tab would land
    /// in the same slot).
    Reorder { from: usize, to: usize },
}
