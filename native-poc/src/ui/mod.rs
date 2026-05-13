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

pub mod keybinds;
pub mod status_bar;
pub mod tab_bar;

/// User intents originating from the global keybind layer.
///
/// The dispatcher in [`keybinds::dispatch`] is pure: it only inspects
/// `(egui::Modifiers, egui::Key)` and never touches application state.
/// The app loop is responsible for translating the resulting action
/// into mutations on the tabs vector (or PTY writes).
///
/// Note: every variant carries a `*Tab` suffix because Phase 4-B only
/// binds tab-roster actions globally. Later phases will introduce
/// non-tab actions (prefix latch, status bar toggle); we keep the
/// `clippy::enum_variant_names` allow until that diversification
/// happens so the variant names stay self-describing at call sites.
#[allow(clippy::enum_variant_names)]
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
    /// Close ("×") button on the tab at `index` (0-based).
    Close(usize),
    /// Switch to the tab at `index` (0-based) via mouse click.
    Switch(usize),
}
