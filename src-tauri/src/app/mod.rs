//! Top-level `App` state container.
//!
//! Phase 1 was empty. Phase 2 holds a PTY-backed tab and offers an
//! `on_resize` hook that propagates the new grid dimensions to all PTYs.
//! Sub-phase 2 adds dirty-row tracking state (cursor + selection history,
//! `needs_full_redraw` flag, `EMTERM_FULL_REDRAW` toggle) used by the
//! renderer to skip frames that wouldn't change a pixel.
//! Later phases extend this with a viewer registry and settings.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use term_core::terminal_core::TerminalCore;

use crate::callbacks::{NotificationSink, NotifyRustSink};
use crate::ime::backend::ImeBackend;
use crate::ime::null::NullBackend;
use crate::render::font::cache::GlyphCache;
use crate::render::font::fallback::FallbackChain;
use crate::render::font::resolver::Resolver;
use crate::render::font::traits::{FontId, GlyphRasterizer};
use crate::selection::Selection;
use crate::settings::{FontEngine, Settings};
use crate::status_bar::StatusBarRuntime;
use crate::tabs::Tab;
use crate::ui::emoji_cache::EmojiTextureCache;

mod ime;
mod sftp;
mod timing;

pub use timing::{BELL_FLASH_MS, BLINK_HALF_MS, RestartToast, TOAST_POLL_MS};
#[cfg(test)]
use timing::RESTART_TOAST_LINGER_SECS;

/// Minimum wall-clock gap between two automatic (per-frame) search
/// re-resolves. A burst of PTY output flags the search document dirty on
/// every chunk; without this throttle [`App::auto_research_if_dirty`] would
/// rebuild the logical-line document and re-run the match on every painted
/// frame. The dirty flag is *preserved* when a re-search is skipped here, so
/// the pending change is reflected on the next frame past the gap.
pub const AUTO_RESEARCH_THROTTLE: std::time::Duration = std::time::Duration::from_millis(150);

/// Whether an automatic re-search may run now, given the instant of the
/// previous one. `None` (no prior auto re-search) always allows. Split out
/// as a pure function so the throttle policy is unit-testable without an
/// `App` or real wall-clock sleeps.
pub fn auto_research_allowed(last: Option<Instant>, now: Instant) -> bool {
    match last {
        None => true,
        Some(prev) => now.duration_since(prev) >= AUTO_RESEARCH_THROTTLE,
    }
}

// ── task0002: mux sidebar overlay auto-dim ──────────────────────────────
//
// Three named constants (IMPLEMENTATION.md conventions, NFR2 — single
// definition site next to the resolver they feed). The pre-existing
// `OVERLAY_FILL_ALPHA` in `ui::mux_sidebar` is unrelated (the bright-state
// fill's own alpha) and keeps its current value/location untouched.

/// Whole-card opacity multiplier while the overlay card is idle (no hover,
/// no mux window switch recorded within [`OVERLAY_BRIGHT_HOLD`]). SPEC.md
/// "Concrete values" / assumption A2.
pub const OVERLAY_IDLE_OPACITY: f32 = 0.35;
/// Duration of the bright -> dim fade. Entering the bright state is
/// immediate (no fade-in) — D4. SPEC.md "Concrete values" / assumption A3.
pub const OVERLAY_DIM_FADE: std::time::Duration = std::time::Duration::from_millis(200);
/// How long the overlay card stays at full opacity after a mux window
/// switch (or while hovered), before the fade begins. SPEC.md "Concrete
/// values".
pub const OVERLAY_BRIGHT_HOLD: std::time::Duration = std::time::Duration::from_millis(3000);

/// Pure opacity resolver (IMPLEMENTATION.md D1): the overlay card's
/// whole-card opacity multiplier plus whether a further frame is needed to
/// continue an in-flight fade, given the dim state and `now`. No side
/// effects — the `App` wrapper ([`App::resolve_mux_sidebar_opacity`]) owns
/// arming/resetting `fade_started` between calls.
///
/// - **Bright** (`hovered` true, OR `last_switch` is within
///   [`OVERLAY_BRIGHT_HOLD`] of `now`): full opacity, immediately, no
///   interpolation (D4). Hover and the post-switch hold are independent
///   sufficient conditions (SPEC.md A5) — releasing hover inside a pending
///   hold keeps the card bright for the remainder of the hold.
/// - **Dim, no fade tracked** (`fade_started` is `None`): the idle opacity
///   directly — covers a fresh session with no switch ever recorded (FR5)
///   and a fully-settled dim state.
/// - **Dim, fade in flight**: linear interpolation from full opacity down
///   to the idle opacity over [`OVERLAY_DIM_FADE`], clamped to `0.0..=1.0`
///   (AC-4 — a `fade_started` in the future clamps `elapsed` to zero via
///   `checked_duration_since`, so the output never leaves the valid range
///   even for adversarial inputs).
pub fn resolve_mux_sidebar_dim_opacity(
    hovered: bool,
    last_switch: Option<Instant>,
    fade_started: Option<Instant>,
    now: Instant,
) -> (f32, bool) {
    let bright = hovered || last_switch.is_some_and(|t| now < t + OVERLAY_BRIGHT_HOLD);
    if bright {
        return (1.0, false);
    }
    match fade_started {
        None => (OVERLAY_IDLE_OPACITY, false),
        Some(started) => {
            let elapsed = now
                .checked_duration_since(started)
                .unwrap_or(std::time::Duration::ZERO);
            if elapsed >= OVERLAY_DIM_FADE {
                (OVERLAY_IDLE_OPACITY, false)
            } else {
                let t = elapsed.as_secs_f32() / OVERLAY_DIM_FADE.as_secs_f32();
                let opacity = 1.0 + t * (OVERLAY_IDLE_OPACITY - 1.0);
                (opacity.clamp(0.0, 1.0), true)
            }
        }
    }
}

/// Pure deadline provider companion to [`resolve_mux_sidebar_dim_opacity`]
/// (IMPLEMENTATION.md D5 / Scheduling): the next `Instant` the event loop
/// must wake for this feature, or `None` when settled. Mirrors
/// `next_bell_deadline`'s "single upcoming instant" shape rather than a
/// bounded poll — while hovered there is nothing to schedule (the hover
/// predicate's own transition requests its redraw separately, in
/// `window_host`).
pub fn resolve_mux_sidebar_dim_deadline(
    hovered: bool,
    last_switch: Option<Instant>,
    fade_started: Option<Instant>,
    now: Instant,
) -> Option<Instant> {
    if hovered {
        return None;
    }
    if let Some(t) = last_switch {
        let hold_end = t + OVERLAY_BRIGHT_HOLD;
        if now < hold_end {
            return Some(hold_end);
        }
    }
    let started = fade_started?;
    let elapsed = now
        .checked_duration_since(started)
        .unwrap_or(std::time::Duration::ZERO);
    if elapsed < OVERLAY_DIM_FADE {
        Some(started + OVERLAY_DIM_FADE)
    } else {
        None
    }
}

/// Per-step change applied by the `ZoomIn` / `ZoomOut` keybinds, in
/// logical points.
pub const FONT_SIZE_PT_STEP: f32 = 1.0;
/// Lower clamp for the runtime terminal font size (logical points).
pub const FONT_SIZE_PT_MIN: f32 = 6.0;
/// Upper clamp for the runtime terminal font size (logical points).
pub const FONT_SIZE_PT_MAX: f32 = 72.0;

/// Clamp a candidate terminal font size (logical points) into
/// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]. Split out as a pure
/// function so the zoom clamp can be unit-tested without constructing a
/// full `App` (which builds the font stack and a status-bar runtime).
pub fn clamp_font_size_pt(pt: f32) -> f32 {
    pt.clamp(FONT_SIZE_PT_MIN, FONT_SIZE_PT_MAX)
}

/// Where the viewport currently sits relative to the live tail.
///
/// Scrollback position. Defined in [`crate::scroll`] as a layer-free value
/// type and re-exported here for backward compatibility; `mux::window_group`
/// imports it from `crate::scroll` so the pure mux model does not depend on
/// the `app` layer (no `app ↔ mux` cycle).
pub use crate::scroll::ScrollPosition;

/// Direction for [`App::jump_to_prompt`]: `Prev` scrolls toward older
/// prompts (up), `Next` toward newer prompts (down).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpDirection {
    Prev,
    Next,
}

/// Result of dispatching a mux prefix action ([`App::dispatch_mux_action`]).
/// The caller (the key path in `window_host`) reacts to it: redraw on
/// `Changed`, open the corresponding egui dialog on `OpenRename` /
/// `OpenMove`, tear down the group on `Detach`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxActionOutcome {
    /// The action did not apply (no mux tab, single-window switch, unknown
    /// follow-up, …). Caller does nothing.
    None,
    /// Local mux state changed and a control message was sent; redraw.
    Changed,
    /// A `Detach` was sent; the daemon's `Detached` reply dissolves the
    /// group through the inbound path.
    Detach,
    /// Open the rename dialog for the window with this stable id, seeded with
    /// `current_name`. Confirmed via [`App::confirm_mux_rename`].
    OpenRename {
        window_id: u32,
        current_name: String,
    },
    /// Open the move dialog for the window with this stable id. Confirmed via
    /// [`App::confirm_mux_move`].
    OpenMove {
        window_id: u32,
        current_position: usize,
        window_count: usize,
    },
}

/// Build the mux prefix latch from settings: the prefix chord
/// (`mux_prefix_key`, falling back to `Ctrl+Z` on a parse error) and the
/// action bindings (`mux.keybinds`).
pub(super) fn build_mux_latch(settings: &Settings) -> crate::mux::prefix::Latch {
    use crate::mux::prefix::{ActionBindings, Latch, PrefixChord, parse_prefix_key};
    let chord = parse_prefix_key(&settings.mux_prefix_key).unwrap_or_else(|| {
        log::warn!(
            "settings.mux.prefix: invalid chord {:?}, falling back to Ctrl+Z",
            settings.mux_prefix_key
        );
        PrefixChord::default()
    });
    let bindings = ActionBindings::from_settings_map(&settings.mux.keybinds);
    Latch::with_bindings(chord, crate::mux::prefix::DEFAULT_ARMED_TIMEOUT, bindings)
}

/// The `App::agent_status` keys `tab` occupies: its own plain-tab key
/// (`PaneKey::Tab`, harmless to include even when unoccupied — `discard` /
/// `mark_seen` on a missing key are no-ops) plus one `PaneKey::MuxPane` per
/// pane in its window group, if attached (task0005). Shared by the
/// tab-close (AC-6) and mark_seen-on-foreground-display (AC-5) call sites so
/// "which panes belong to this tab" is defined in exactly one place.
pub(super) fn agent_status_keys_for_tab(tab: &crate::tabs::Tab) -> Vec<crate::agent_status_model::PaneKey> {
    use crate::agent_status_model::PaneKey;
    let mut keys = vec![PaneKey::Tab(tab.stable_id)];
    if let Some(group) = tab.mux_group.as_ref() {
        keys.extend(group.pane_ids().iter().map(|&id| PaneKey::MuxPane(id)));
    }
    keys
}

/// Whether a drained agent-status transition's `pane` is currently
/// displayed (task0009 Design: "Resolve pane_visible"). `true` only when
/// the OS window is focused AND `pane` is one of the active tab's
/// agent-status keys — the SAME "displayed" definition `pump_all`'s
/// mark_seen call already uses (`agent_status_keys_for_tab`), so a
/// mux-attached tab's whole window group counts as displayed while that
/// tab is active/focused, not just the group's currently-active window.
/// Free function (not an `App` method) so the visibility rule is testable
/// against arbitrary tab fixtures without constructing a full `App`.
pub(super) fn agent_status_pane_visible(
    window_focused: bool,
    active_tab: Option<&crate::tabs::Tab>,
    pane: &crate::agent_status_model::PaneKey,
) -> bool {
    if !window_focused {
        return false;
    }
    let Some(active_tab) = active_tab else {
        return false;
    };
    agent_status_keys_for_tab(active_tab).contains(pane)
}

/// Resolve the tab title for a drained transition's `pane`, by locating
/// its containing tab (task0009 Design: "Resolve tab_title from the
/// transition's pane by locating its containing tab"). `None` when no
/// tracked tab currently owns `pane` (it closed between the transition
/// firing and this drain — the caller falls back to an empty title).
pub(super) fn agent_status_pane_tab_title<'a>(
    tabs: &'a [crate::tabs::Tab],
    pane: &crate::agent_status_model::PaneKey,
) -> Option<&'a str> {
    use crate::agent_status_model::PaneKey;
    tabs.iter()
        .find(|tab| match pane {
            PaneKey::Tab(id) => tab.stable_id == *id,
            PaneKey::MuxPane(pane_id) => tab
                .mux_group
                .as_ref()
                .is_some_and(|g| g.pane_ids().contains(pane_id)),
        })
        .map(|tab| tab.title.as_str())
}

/// Resolve the per-pane notification rate-limit key for `pane` (task0009
/// Design: "Resolve rate_limit_key"). Mux panes prefer the daemon-learned
/// `public_pane_id` (stable across the pane's lifetime, unique across
/// concurrent panes by the "Public pane ID format" shared component);
/// plain tabs use a prefixed stable-id string. Both branches are prefixed
/// (`"tab:"` / `"mux:"`) so the fallback path (a mux pane discarded before
/// ever learning a public id — not expected in practice, since learning
/// and applying a daemon update happen in the same `pump_all` batch) can
/// never collide with a plain-tab key. Shared by every discard site
/// (`close_tab`, the reaped-tab loop, `pump_all`'s closed-mux-pane loop)
/// and the transition-drain loop so all four derive the same key. Takes
/// `mux_public_pane_ids` explicitly (rather than `&App`) so it is testable
/// without constructing a full `App`.
pub(super) fn agent_notification_rate_limit_key(
    mux_public_pane_ids: &std::collections::HashMap<u32, String>,
    pane: &crate::agent_status_model::PaneKey,
) -> String {
    use crate::agent_status_model::PaneKey;
    match pane {
        PaneKey::Tab(id) => format!("tab:{id}"),
        PaneKey::MuxPane(pane_id) => mux_public_pane_ids
            .get(pane_id)
            .cloned()
            .unwrap_or_else(|| format!("mux:{pane_id}")),
    }
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Settings-window launcher: the settings UI runs in a child
    /// `--settings` process (Wry WebView), spawned on the settings icon /
    /// `Ctrl+,`. Boxed so tests can swap in a counting double instead of
    /// spawning real processes.
    pub settings_launcher: Box<dyn crate::settings_launcher::SettingsWindowLauncher>,
    /// Last known grid size in cells. Updated by `window_host` whenever the
    /// window resizes; PTYs are resized to match.
    pub cell_size: GridDims,
    /// Active mouse selection on the active tab, if any. Phase 4 owns this;
    /// later phases may move it per-tab when tabs preserve selection.
    pub selection: Option<Selection>,
    /// Absolute-row anchor of a single-click press that has not yet been
    /// upgraded to a drag-selection. Lives here (not in `WindowHost`) so the
    /// `pump_all` eviction shift treats it as absolute-row state alongside
    /// `selection` — otherwise a scrollback eviction between the press and
    /// the first drag motion would leave the anchor pointing at a stale
    /// buffer row. `None` when no press is pending.
    pub pending_selection_anchor: Option<crate::selection::Pos>,
    /// In-terminal text-search state (query, options, matches, current
    /// match cursor). App-global like [`Self::selection`]: one search
    /// overlay shared by the window. Matches are resolved against the
    /// active tab's scrollback + viewport; switching tabs closes the
    /// overlay and clears the state (see `window_host` tab-switch path)
    /// because match rows are indexed into the previous tab's buffer.
    pub search: crate::search::SearchState,
    /// Set on the frame the search overlay is (re)opened so the UI layer
    /// grabs keyboard focus + selects the existing query once, mirroring
    /// the WebView `SearchBar.show()` `focus()` + `select()`. Cleared by
    /// the renderer after it consumes the request.
    pub search_focus_request: bool,
    /// Modal profile-selector state (visibility + highlighted row).
    /// Opened by the `profile_selector` keybind; while visible the
    /// keyboard is captured in `window_host` (navigation / confirm /
    /// cancel) and never reaches the PTY.
    pub profile_selector: crate::ui::profile_selector::ProfileSelectorState,
    /// Timestamp of the last *automatic* re-search (the per-frame
    /// [`Self::auto_research_if_dirty`] path). The auto re-search is
    /// time-throttled to at most one run per [`AUTO_RESEARCH_THROTTLE`] so a
    /// burst of PTY output does not rebuild the logical-line document and
    /// recompile the match on every frame. `None` until the first auto
    /// re-search runs. User-driven [`Self::run_search`] / navigation bypass
    /// this gate entirely.
    last_auto_research: Option<Instant>,
    /// Runtime settings (ambiguous-width policy, OSC 52 policy, and
    /// future fields). Loaded from `settings.json` in Phase 7; today
    /// initialized to default. Wrapped in `Arc` so the per-tab
    /// `NativeCallbacks` can share an immutable view without copying.
    pub settings: Arc<Settings>,
    /// Markdown viewer subsystem: drains each tab's emterm OSC queue,
    /// reassembles `markdown` sessions, and spawns child viewer processes
    /// on completion. Lives on `App` so session state survives across
    /// `pump_all` passes.
    viewer_spawner: crate::viewer::ViewerSpawner,
    /// Sink the spawner emits completed render requests to — spawns a
    /// `--viewer` child process per request and reaps closed children.
    viewer_sink: crate::viewer::ProcessViewerSink,
    /// Image-viewer router: stores decoded Kitty/SIXEL images (LRU,
    /// `image_memory_quota_mb` cap) and spawns a native `--image-viewer`
    /// child window per `Place` event.
    image_viewer: crate::viewer::image::ImageViewerRouter,
    /// Resolved keyboard-chord table, parsed once from
    /// `settings.keybinds` at construction. The keyboard handler in
    /// `window_host` matches incoming events against this for tab-roster
    /// actions (`new_tab` / `close_tab` / `next_tab` / `prev_tab`) and
    /// clipboard chords (`copy` / `paste`).
    pub keybinds: crate::ui::keybinds::KeybindTable,
    /// mux prefix-key latch. Armed on the prefix chord; the follow-up key is
    /// mapped to a [`crate::mux::prefix::PrefixAction`] via the configured
    /// `mux.keybinds`. Only consulted for the active tab when it is
    /// mux-attached (has a `mux_group`). Rebuilt on `apply_settings`.
    pub mux_latch: crate::mux::prefix::Latch,
    /// Currently-open mux rename / move dialog (if any). Plain-data state
    /// so the domain layer stays free of egui widget types (the rendering
    /// for these dialogs lives in `ui::mux_dialogs`, the only place that
    /// imports `egui::Context` for them). Single field replaces the two
    /// `Option<Widget>` fields; reentry is guarded by the
    /// `MuxDialogState::Closed` variant.
    pub mux_dialog: crate::mux::dialog::MuxDialogState,
    /// Runtime open/closed flag for the mux window-list sidebar overlay
    /// (`mux.window_sidebar_overlay` placement mode). Toggled by
    /// `PrefixAction::ToggleWindowSidebar` in `dispatch_mux_action` — a
    /// strict no-op while overlay mode is off (FR4). Reset to `false` in
    /// `pump_all` when the focused tab's mux group tears down, and set to
    /// `true` in `pump_all` on the not-attached -> attached transition
    /// (task0001 FR1/FR2/FR3 — covers startup attach and any later
    /// reattach, including after an explicit close). Rendering input only;
    /// this field does not drive any drawing itself (task0005).
    mux_sidebar_overlay_open: bool,
    /// Bookkeeping for the reset/reopen above: `Some(tab_index)` when
    /// `tab_index` was the active tab AND was mux-attached as of the end of
    /// the previous `pump_all` call; `None` otherwise. A
    /// same-tab `Some -> not-attached` transition is a genuine teardown
    /// (Detach confirmed / last window's PtyExited emptied the group) and
    /// resets the overlay-open flag; a `None -> attached` transition is a
    /// fresh attach or reattach and opens it (task0001). A changed
    /// `self.active` between calls is just a tab switch and must NOT reset
    /// the flag on its own (the overlay's visibility already gates on
    /// "active tab is mux-attached" separately — see the Shared Components
    /// contract in IMPLEMENTATION.md) — though switching onto an
    /// already-attached tab does satisfy the `None -> attached` reopen rule
    /// above when this field held no value, per IMPLEMENTATION.md D2.
    active_mux_attached_prev_pump: Option<usize>,
    /// task0002 FR3: whether the pointer is currently inside the overlay
    /// card's rect. Fed by `window_host`'s `PointerMoved` handler, using
    /// the SAME hit test (`ui::mux_sidebar::point_in_sidebar`) the press
    /// and wheel routing already query, evaluated against
    /// [`crate::ui::mux_sidebar::Placement::Overlay`] — always `false`
    /// outside overlay mode (the persistent panel and hidden state never
    /// dim). Read by [`Self::resolve_mux_sidebar_opacity`].
    mux_sidebar_overlay_hovered: bool,
    /// Snapshot of [`Self::mux_sidebar_overlay_hovered`] as of the last
    /// actually-rendered frame (`record_render_state` /
    /// `record_render_state_no_tab`). [`Self::mux_sidebar_hover_changed`]
    /// compares against this so a hover-predicate transition can feed
    /// `window_host`'s `overlay_work` — mirrors
    /// `previous_status_bar_view_model`'s read-only-comparison-against-a-
    /// snapshot shape.
    mux_sidebar_hover_prev_render: bool,
    /// task0002 FR4: instant of the most recent mux window switch (next /
    /// prev / select-by-digit via [`Self::dispatch_mux_action`], or a
    /// sidebar row click via [`Self::apply_tab_event`]'s `MuxSwitch` arm).
    /// `None` before any switch has ever happened — [`resolve_mux_sidebar_dim_opacity`]
    /// treats that as "not recently switched" (FR5).
    mux_sidebar_last_switch: Option<Instant>,
    /// task0002 fade bookkeeping: the instant the bright condition (hover
    /// OR pending hold) most recently stopped holding. Seeded on
    /// construction to an instant already [`OVERLAY_DIM_FADE`] in the past
    /// so a freshly constructed app resolves directly to
    /// [`OVERLAY_IDLE_OPACITY`] (AC-1) rather than opening with a phantom
    /// fade-in; reset to `None` by [`Self::resolve_mux_sidebar_opacity`]
    /// whenever bright, and re-armed to `Some(now)` the next time it
    /// observes "not bright" (the `Option::get_or_insert` idiom — a `None`
    /// reached from there is therefore always a genuine, just-happened
    /// bright-to-dim transition, never the fresh-construction case).
    mux_sidebar_fade_started: Option<Instant>,
    /// Merged agent-status store (mux-agent-status-api task0005, SPEC FR5 /
    /// FR6 / NFR3): one [`crate::agent_status_model::AgentStatusModel`]
    /// covering both plain tabs (this tab's own OSC 777 `agent-status`
    /// events) and mux panes (daemon-pushed `AgentStatusUpdate`). Owned here
    /// so the UI (task0006) and notification (task0007) layers can read it;
    /// `pump_all` is the only writer.
    pub agent_status: crate::agent_status_model::AgentStatusModel,
    /// pane_id -> daemon-minted public pane ID (task0006 AC-5, `mux_ipc`'s
    /// "Public pane ID format" shared component). The GUI only LEARNS a
    /// pane's public ID when the daemon pushes an `AgentStatusUpdate` for
    /// it (a real report or a replay-on-attach restatement) — there is no
    /// separate wire message that hands it over. Populated alongside
    /// `Self::agent_status` in `pump_all` from the same
    /// `AgentStatusUpdateMsg` batch and cleared on the same closed-pane
    /// list, so a pane with no entry here (never reported) simply hides
    /// the sidebar's copy-to-clipboard row (`ui::mux_sidebar` AC-5).
    mux_public_pane_ids: std::collections::HashMap<u32, String>,
    /// Display locale resolved once from `settings.language` at
    /// construction (`Auto` consults the OS locale). Consumed by the
    /// desktop-notification body formatting in `pump_all`.
    pub locale: crate::i18n::Locale,
    /// Scrollback position. `Live` = auto-follow; `OffsetFromLive(n)` = the
    /// viewport is pinned `n` rows above the live tail.
    pub scroll_position: ScrollPosition,
    /// Whether the alternate screen buffer (DECSET 1049/47/1047) is active.
    /// Tracked by draining `core.take_mode_actions()` after every chunk in
    /// `Tab::pump`. While true, scrollback inputs are suppressed and the
    /// position is pinned to `Live`.
    pub alt_screen: bool,
    /// OS focus state. Updated from `WindowEvent::Focused`. WezTerm-style
    /// rendering: focused → filled block cursor (fg/bg swap in grid pass),
    /// unfocused → outline-only via the egui overlay.
    pub window_focused: bool,
    /// Reference point for cursor-blink phase computation.
    blink_started: Instant,
    /// Cursor-blink "visible" phase observed during the previous render.
    /// When the phase flips, the cursor row joins the dirty union so the
    /// renderer can paint/erase the cursor overlay.
    previous_blink_visible: bool,
    /// Start of the in-flight visual-bell flash (`bell_action =
    /// "visual"`). Set by `pump_all` when a tab drained a BEL, cleared
    /// by [`App::needs_bell_repaint`] once [`BELL_FLASH_MS`] elapsed.
    /// `render::draw_terminal` reads the decay via
    /// [`App::visual_bell_progress`].
    visual_bell_started: Option<Instant>,
    /// One-shot latch (task0005 AC-4, finding 063320466ae233fe): set by
    /// [`App::needs_bell_repaint`] in the same turn it observes the flash
    /// crossing [`BELL_FLASH_MS`] and clears `visual_bell_started`. By the
    /// time `window_host::render` runs, `visual_bell_progress()` already
    /// reads `None` (the overlay is gone), so without this signal the
    /// frame that erases the overlay would look identical to a fully idle
    /// frame and get skipped — freezing the flash at its last painted
    /// alpha instead of fading it out. Consumed (read + cleared) via
    /// [`App::take_bell_erase_pending`].
    bell_erase_pending: bool,
    /// Cursor row/col from the previous rendered frame. The renderer dirties
    /// this row so a moved cursor doesn't ghost the old position.
    previous_cursor: Option<(u16, u16)>,
    /// Selection from the previous rendered frame. Vacated selection rows
    /// must be repainted to clear highlight.
    previous_selection: Option<Selection>,
    /// `visible_start` (`scrollback_len - scroll_offset`, saturating) from
    /// the previous rendered frame. Used to translate the absolute-row
    /// [`Self::previous_selection`] back into screen rows when computing the
    /// dirty set, so the highlight cleared on the rows it occupied *last*
    /// frame even after the viewport scrolled.
    previous_visible_start: u32,
    /// Set on construction, resize, and surface recovery. Forces the next
    /// frame to repaint every row regardless of `term_core` dirty bits.
    needs_full_redraw: bool,
    /// FR4 (tab-bar scroll-into-view): one-shot signal raised when the active
    /// cell changes via the keyboard (a committed plain-tab switch through
    /// `switch_to_tab`, or a committed mux window switch in
    /// `dispatch_mux_action`). The tab strip reads it for one frame and
    /// scrolls the active visual cell into view; `window_host` clears it
    /// post-frame so it never re-fires on an unrelated repaint (e.g. after a
    /// mouse-driven scroll). Default off.
    scroll_active_tab_into_view: bool,
    /// Debug toggle (env `EMTERM_FULL_REDRAW=1`) that permanently disables
    /// the dirty-row optimization for triage.
    force_full_redraw: bool,
    /// D2 (FR3, task0003 rework): a one-shot request left by an activation
    /// path (`switch_to_tab`, the close-tab active-index fix-up, the
    /// exited-tab reap fix-up) that the now-active tab may need reconciling
    /// against `cell_size`. NOT executed synchronously where it is set — at
    /// that moment `cell_size` may still hold dims computed for the
    /// OUTGOING tab (the persistent mux sidebar's width inset depends on
    /// whether the ACTIVE tab is mux-attached). `window_host::render`
    /// consumes it via [`Self::execute_pending_reconcile`] once insets /
    /// any pending display-area resize have settled `cell_size` for the
    /// INCOMING tab. Consecutive requests before that point collapse into
    /// one (idempotent `bool`, not a queue).
    pending_active_tab_reconcile: bool,
    /// Phase 4-G: native IME backend. The App holds a `Box<dyn ImeBackend>`
    /// so the OS-specific clients (X11 / Wayland / Windows) and the
    /// passthrough `NullBackend` share the same seam. Default-constructed
    /// to `NullBackend` so unit tests do not require window / display
    /// handles; `window_host::run` replaces it with the factory-resolved
    /// backend at startup via `App::set_ime_backend`.
    ime_backend: Box<dyn ImeBackend>,
    /// Last `(row, col)` reported to `ImeBackend::notify_cursor_rect`.
    /// `None` until the first cell-position notification. Updated in
    /// `notify_cursor_rect_if_changed` so we never spam the IM server
    /// when the cursor stays put (SPEC.md FR7).
    ime_last_cursor_cell: Option<(u16, u16)>,
    /// Whether the active backend is the passthrough `NullBackend`. The
    /// event loop uses this to decide whether `WindowEvent::ReceivedImeText`
    /// should drive the commit path (NullBackend only) or be ignored
    /// (real backend, which routes commits via `ImeEvent::Commit`). This
    /// avoids double-committing the same composition (SPEC.md FR9).
    ime_is_null: bool,
    /// Whether the IME pump has already reported a budget-exceeded
    /// drop. Latched so the warn log fires at most once per process
    /// (`IME_E901`).
    ime_overflow_warned: bool,
    /// Phase 4-H (font-swash-migration FR12): font stack shared with
    /// the `TerminalGridPass`. The rasterizer is selected once at
    /// startup based on `Settings::font_engine` (Swash default /
    /// AbGlyph escape hatch). `GlyphCache` lives behind a mutex so the
    /// same handle can be reused across frames; `FallbackChain` is
    /// immutable after `new` returns and is wrapped in an `Arc` so the
    /// renderer pass can hold its own clone.
    #[allow(dead_code)]
    pub font_resolver: Arc<Resolver>,
    pub font_fallback: Arc<FallbackChain>,
    pub font_cache: Arc<Mutex<GlyphCache>>,
    pub font_rasterizer: Arc<dyn GlyphRasterizer>,
    /// `FontId` returned by the resolver for the bundled CJK base font.
    /// `TerminalGridPass` uses this as the chain root when no user font
    /// override is registered.
    #[allow(dead_code)]
    pub font_base_id: FontId,
    /// Per-cell width in egui logical pixels (1.0× scale). Computed
    /// once at startup from the base font's advance for "M" at
    /// `settings.font_size`. The renderer multiplies by
    /// `pixels_per_point` for the physical-pixel `CellMetrics` handed
    /// to wgpu. Mirrors the legacy WebView build's
    /// `ctx.measureText("M").width` so settings.json's `font_size`
    /// produces visually-matching cells across both binaries.
    pub cell_w_logical: f32,
    /// Per-cell height in egui logical pixels (1.0× scale). Computed
    /// once at startup from the base font's ascent + descent at
    /// `settings.font_size`. See [`Self::cell_w_logical`].
    pub cell_h_logical: f32,
    /// Runtime terminal font size in logical points. Seeded from
    /// `settings.font_size` at startup and mutated by the zoom keybinds
    /// (`ZoomIn` / `ZoomOut` / `ZoomReset`). Kept distinct from
    /// `settings.font_size` (the persisted baseline) so `ZoomReset` can
    /// restore the configured value and new tabs can inherit the live
    /// zoom level. `cell_w_logical` / `cell_h_logical` and every tab's
    /// `Theme::font_size_pt` are re-derived from this whenever it
    /// changes (see [`Self::set_font_size_pt`]).
    pub runtime_font_size_pt: f32,
    /// Runtime tab-bar visibility, toggled by the `ToggleTabBar`
    /// keybind. Seeded from `settings.show_tab_bar`; the renderer and
    /// the grid-size / hit-test paths read this instead of the
    /// persisted setting so the toggle takes effect without rewriting
    /// `settings.json`.
    pub show_tab_bar: bool,
    /// Color-emoji texture cache for the status bar. egui's text path
    /// (ab_glyph) cannot raster CBDT/COLR glyphs, so the widget walks
    /// each run via [`crate::ui::emoji_cache::split_segments`] and
    /// substitutes a swash-rasterized `egui::Image` for emoji spans.
    /// The cache lives behind a `Mutex` so the renderer can take it
    /// from `&App` (mirroring `font_cache`).
    pub emoji_texture_cache: Arc<Mutex<EmojiTextureCache>>,
    /// Status-bar runtime (template engine + providers + OSC
    /// dispatcher). Constructed once at startup; per-frame snapshots
    /// flow through [`App::status_bar_view_model`].
    pub status_bar_runtime: StatusBarRuntime,
    /// Snapshot of the active tab's cwd, updated once per frame in
    /// [`App::sync_active_cwd`]. The status-bar `{cwd}` /
    /// `{git_branch}` providers read this through a `CwdSource` closure
    /// so worker threads stay isolated from `App`.
    pub active_cwd: Arc<Mutex<Option<String>>>,
    /// View model rendered in the previous frame. Compared against the
    /// freshly-built model in [`App::status_bar_view_model_changed`]
    /// so the dirty-row skip path in `WindowHost::render` can bypass
    /// the early return when only the status bar (e.g. the wall clock,
    /// the git branch worker, or an OSC 777 push) changed since the
    /// last frame. Without this, the provider-owned wake chain reaches
    /// `user_event` -> `request_redraw` but the next `render()` call
    /// observes `dirty_rows_this_frame() == 0` on an idle shell and
    /// returns early, freezing the clock display.
    previous_status_bar_view_model: Option<crate::status_bar::StatusBarViewModel>,
    /// Desktop-notification surface shared with every tab's
    /// `NativeCallbacks` (OSC 9 notifications) and used directly by
    /// link handling (`WindowHost::open_file_in_editor`) to surface
    /// file-not-found / editor-launch failures to the user. Constructed
    /// once in [`App::with_settings`] as the production [`NotifyRustSink`]
    /// and cloned into each tab so a single sink instance is shared.
    ///
    /// アプリケーションドメイン外からの直接アクセスは禁止。通知送信は
    /// [`App::notify`] を経由すること。
    pub(crate) notification_sink: Arc<dyn NotificationSink>,
    /// Per-frame fold layout for the active tab, recomputed at the top of
    /// each `WindowHost::render` via [`App::refresh_fold_layout`]. `Some`
    /// only when the active tab has at least one *collapsed* region
    /// (mirroring the WebView's `getCollapsedRegions().length > 0` gate);
    /// `None` otherwise so the renderer takes the unchanged linear path.
    /// Read by `collect_cell_inputs` (cell row selection),
    /// `draw_fold_summaries` (summary overlays), and
    /// `draw_search_highlights` (fold-aware match → screen-row mapping).
    /// Building it once with `&mut self` lets the renderer query it
    /// immutably for the rest of the frame (the fold line-mapping otherwise
    /// needs `&mut FoldManager` for its collapsed cache).
    fold_layout: Option<crate::fold::FoldLayout>,
    /// In-process SFTP upload service (process manager + concurrency pool +
    /// progress/result senders). Constructed from
    /// `settings.sftp_max_concurrent_uploads`. Shared via `Arc` so worker
    /// threads spawned by `start_upload` outlive a single `pump`.
    pub sftp_service: Arc<crate::sftp::service::SftpService>,
    /// SFTP UI state (drop aggregation + overlay + dialogs + toasts).
    pub sftp_ui: crate::sftp::ui::SftpUiState,
    /// Progress receiver drained each frame by [`App::pump_sftp`].
    sftp_progress_rx: crate::sftp::service::ProgressReceiver,
    /// Duplicate-check result receiver drained each frame by [`App::pump_sftp`].
    sftp_result_rx: crate::sftp::service::ResultReceiver,
    /// Binary-mismatch restart toast (armed by a failed self-spawn, drawn by
    /// the render path, auto-dismissed by [`App::pump_sftp`]).
    pub restart_toast: RestartToast,
    /// Per-pane rate limiter for agent-status (blocked/done) desktop
    /// notifications (task0007 AC-4). Keyed by whatever stable pane
    /// identity the caller of [`App::maybe_notify_agent_transition`]
    /// supplies (a mux pane's `public_pane_id`, or a caller-chosen key for
    /// plain tabs).
    agent_notification_rate_limiter: crate::notifications::AgentNotificationRateLimiter<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridDims {
    pub cols: u16,
    pub rows: u16,
}

impl Default for GridDims {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

/// Which mux window-sidebar variant (if any) is visible on the current
/// frame. See [`App::mux_sidebar_visibility`] for the resolution rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuxSidebarVisibility {
    /// Neither variant renders (local tab, or overlay mode with the
    /// runtime flag closed).
    Hidden,
    /// Persistent right panel — reserves grid WIDTH only (task0006
    /// right-edge placement update: the grid's x-origin is identical with
    /// and without it).
    Persistent,
    /// Right-edge overlay — draws over the terminal area, zero grid inset.
    Overlay,
}

/// Horizontal grid inset (logical px) the persistent sidebar reserves,
/// given this frame's [`MuxSidebarVisibility`] and the window's logical
/// width. Only `Persistent` contributes a non-zero inset — overlay mode
/// draws over the grid without reshaping it (task0005 NFR1). Pure function
/// consumed by `window_host::grid_size` as a WIDTH-only reduction (task0006:
/// the right-edge placement means this value never feeds an x-origin —
/// `window_host::cell_metrics_px`, `render::cursor`, and
/// `draw_search_highlights` all compute the grid/cursor/search x-origin the
/// same way with or without the sidebar).
pub fn mux_sidebar_grid_inset(visibility: MuxSidebarVisibility, window_width_logical: f32) -> f32 {
    match visibility {
        MuxSidebarVisibility::Persistent => {
            crate::ui::mux_sidebar::sidebar_width(window_width_logical)
        }
        MuxSidebarVisibility::Hidden | MuxSidebarVisibility::Overlay => 0.0,
    }
}

impl App {
    /// Construct an `App` with built-in default settings. Retained for
    /// tests; binary callers should prefer [`App::with_settings`] so
    /// `settings.json` overrides are honored.
    pub fn new() -> Self {
        Self::with_settings(Settings::new())
    }

    /// Construct an `App` from a pre-built [`Settings`]. `main.rs` calls
    /// this with [`Settings::load_or_default`] so on-disk overrides
    /// (`~/.config/net.laser5.app.emterm/settings.json`) take effect.
    pub fn with_settings(settings: Settings) -> Self {
        let force_full_redraw = std::env::var("EMTERM_FULL_REDRAW")
            .ok()
            .map(|v| v == "1")
            .unwrap_or(false);
        if force_full_redraw {
            log::warn!("EMTERM_FULL_REDRAW=1: dirty-row optimization disabled");
        }
        // Surface the user-configured terminal settings so a
        // typo / unexpected default is visible in the log instead of
        // silently rendering at the wrong size or with the wrong
        // cursor shape. Mirrors the existing `font.base / font.jp /
        // font.emoji` lines emitted by `build_font_stack` below.
        log::info!(
            "settings: font_size={}pt padding={}px cursor_style={:?} cursor_blink={}",
            settings.font_size,
            settings.padding,
            settings.cursor_style,
            settings.cursor_blink
        );
        log::info!(
            "settings: ui_theme={:?} ui_theme_preset={:?} show_tab_bar={} terminal_color_scheme={:?}",
            settings.ui_theme,
            settings.ui_theme_preset,
            settings.show_tab_bar,
            settings.terminal_color_scheme
        );
        // Seed the MD3 palette slot (accent preset × brightness) before
        // any widget runs. `OnceLock` means subsequent calls (e.g. a
        // reload path) are no-ops, which matches the WebView build's
        // startup-only behavior for the UI theme preset. `System`
        // resolves to dark inside `set_preset` — no desktop-portal
        // brightness lookup in the native build yet.
        crate::ui::md3::set_preset(settings.ui_theme_preset, settings.ui_theme);
        let settings = Arc::new(settings);
        // Construct the in-process SFTP service from the configured
        // concurrency cap; the receivers are drained each frame by
        // `App::pump_sftp`.
        let (sftp_service, sftp_progress_rx, sftp_result_rx) =
            crate::sftp::service::SftpService::new(settings.sftp_max_concurrent_uploads);
        let sftp_service = Arc::new(sftp_service);
        // Resolve the user-configured chord table once. Unparseable
        // specs fall back to their built-in defaults with a warn log
        // (see `KeybindTable::from_settings`).
        let keybinds = crate::ui::keybinds::KeybindTable::from_settings(&settings.keybinds);
        let mux_latch = build_mux_latch(&settings);
        // Resolve `language` to a concrete locale once; `Auto` consults
        // the OS locale (sys-locale), unsupported tags fall back to En.
        let locale = crate::i18n::resolve(settings.language);
        log::info!(
            "settings: language={:?} -> locale={:?}",
            settings.language,
            locale
        );
        let (font_resolver, font_fallback, font_cache, font_rasterizer, font_base_id) =
            Self::build_font_stack(&settings);

        // Compute per-cell logical-pixel dimensions from the freshly
        // built font stack so the grid matches the legacy WebView
        // build's `ctx.measureText("M")` path. Done after the font
        // stack so the rasterizer + fallback chain are available.
        // `font_size_px()` applies the CSS-compatible `pt → px`
        // conversion (96/72) that the legacy WebView build does in
        // `renderer-settings.ts` — without it, native-poc rasterizes
        // at ~75% of the WebView size for the same settings value.
        let font_size_px = settings.font_size_px();
        let (cell_w_logical, cell_h_logical) = crate::render::compute_cell_dims(
            font_rasterizer.as_ref(),
            font_fallback.as_ref(),
            font_size_px,
        );
        log::info!(
            "cell metrics: {}x{} logical px (font_size={}pt = {}px)",
            cell_w_logical,
            cell_h_logical,
            settings.font_size,
            font_size_px
        );

        // Per-frame cwd snapshot shared with status-bar providers. The
        // app updates it in `sync_active_cwd` before the runtime is
        // queried; the runtime hands a closure reading this `Arc` to
        // the Cwd / GitBranch providers.
        let active_cwd: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let cwd_for_source = active_cwd.clone();
        let cwd_source: crate::status_bar::providers::CwdSource =
            Arc::new(move || cwd_for_source.lock().clone());
        // Hand the providers a clone of the global `wake` so each
        // owns its own refresh-redraw seam (SPEC.md Notes section).
        let status_bar_runtime = StatusBarRuntime::new(
            &settings.statusbar,
            cwd_source,
            crate::wakeup::shared_wake_fn(),
        );

        // Seed the runtime view state from settings before the `Arc`
        // moves into the struct literal's `settings` field (the literal
        // initializes fields in source order, so a later
        // `settings.font_size` read would be a use-after-move).
        let runtime_font_size_pt = settings.font_size;
        let show_tab_bar = settings.show_tab_bar;

        // Single production notification sink, shared with every tab's
        // callbacks (OSC 9) and used directly by link handling for
        // file-not-found / editor-launch failures.
        let notification_sink: Arc<dyn NotificationSink> = Arc::new(NotifyRustSink);

        // task0002: seed the fade-bookkeeping instant already
        // `OVERLAY_DIM_FADE` in the past so a freshly constructed app
        // resolves directly to `OVERLAY_IDLE_OPACITY` (AC-1) instead of
        // opening with a phantom fade-in. `checked_sub` only fails within
        // `OVERLAY_DIM_FADE` of process start (an `Instant` reference
        // point that near boot); falling back to `now` there costs at
        // most one imperceptible extra fade-in frame.
        let mux_sidebar_fade_started = Instant::now().checked_sub(OVERLAY_DIM_FADE);

        Self {
            tabs: Vec::new(),
            active: 0,
            mux_latch,
            mux_dialog: crate::mux::dialog::MuxDialogState::Closed,
            // FR2/AC-7: the overlay's runtime open flag starts open so a
            // default-configured session shows the window list without a
            // toggle press.
            mux_sidebar_overlay_open: true,
            active_mux_attached_prev_pump: None,
            mux_sidebar_overlay_hovered: false,
            mux_sidebar_hover_prev_render: false,
            mux_sidebar_last_switch: None,
            mux_sidebar_fade_started,
            agent_status: crate::agent_status_model::AgentStatusModel::new(),
            mux_public_pane_ids: std::collections::HashMap::new(),
            settings_launcher: Box::new(crate::settings_launcher::ProcessSettingsLauncher::new()),
            cell_size: GridDims::default(),
            selection: None,
            pending_selection_anchor: None,
            search: crate::search::SearchState::default(),
            search_focus_request: false,
            profile_selector: crate::ui::profile_selector::ProfileSelectorState::default(),
            last_auto_research: None,
            viewer_spawner: crate::viewer::ViewerSpawner::new(),
            viewer_sink: crate::viewer::ProcessViewerSink::new(settings.clone()),
            image_viewer: crate::viewer::image::ImageViewerRouter::new(&settings),
            settings,
            keybinds,
            locale,
            scroll_position: ScrollPosition::Live,
            alt_screen: false,
            window_focused: true,
            blink_started: Instant::now(),
            previous_blink_visible: true,
            visual_bell_started: None,
            bell_erase_pending: false,
            previous_cursor: None,
            previous_selection: None,
            previous_visible_start: 0,
            needs_full_redraw: true,
            scroll_active_tab_into_view: false,
            force_full_redraw,
            pending_active_tab_reconcile: false,
            ime_backend: Box::new(NullBackend::new()),
            ime_last_cursor_cell: None,
            ime_is_null: true,
            ime_overflow_warned: false,
            font_resolver,
            font_fallback,
            font_cache,
            font_rasterizer,
            font_base_id,
            cell_w_logical,
            cell_h_logical,
            runtime_font_size_pt,
            show_tab_bar,
            emoji_texture_cache: Arc::new(Mutex::new(EmojiTextureCache::new())),
            status_bar_runtime,
            active_cwd,
            previous_status_bar_view_model: None,
            notification_sink,
            fold_layout: None,
            sftp_service,
            sftp_ui: crate::sftp::ui::SftpUiState::default(),
            sftp_progress_rx,
            sftp_result_rx,
            restart_toast: RestartToast::default(),
            agent_notification_rate_limiter:
                crate::notifications::AgentNotificationRateLimiter::default(),
        }
    }

    /// Phase 4-H startup wiring (FR12 + FR6 + FR7 + FR11). Build the
    /// resolver, register bundled fonts, branch on
    /// `Settings::font_engine` to construct either the Swash or AbGlyph
    /// rasterizer, build the fallback chain, and seed the glyph cache.
    /// The returned tuple is owned by `App`; the renderer's
    /// `TerminalGridPass` borrows clones of each `Arc`.
    ///
    /// The rasterizer in the returned tuple is fully initialized:
    /// `set_base_font` has already been called with `base_id` before
    /// returning, so callers do not need to call it again.
    pub(super) fn build_font_stack(
        settings: &Settings,
    ) -> (
        Arc<Resolver>,
        Arc<FallbackChain>,
        Arc<Mutex<GlyphCache>>,
        Arc<dyn GlyphRasterizer>,
        FontId,
    ) {
        #[cfg(not(test))]
        use crate::render::font::resolver::FontRole;

        let mut resolver = Resolver::new();
        // FR6 resolution priority (highest first):
        //   1. settings-supplied family
        //   2. user override directory
        //   3. system fonts
        //   4. bundled fonts
        //
        // Registration order matters for `by_family` lookups —
        // `Resolver::register_bytes` short-circuits on the first entry
        // for a given family name. We therefore register in highest →
        // lowest priority order.

        // Emoji families are bundled and fixed; user-side selection was
        // removed (see font-bundle-cleanup report). Only the bundled
        // `Noto Color Emoji` / `Noto Emoji` faces serve `FontRole::ColorEmoji`
        // / `FontRole::MonochromeEmoji` from now on.

        // 2. User override directory. The scan is silently a no-op when
        //    the directory does not exist. Skipped during tests so unit
        //    tests don't touch the real user env.
        #[cfg(not(test))]
        resolver.scan_user_dir();

        // 4. Bundled fonts. `register_bundled` registers CJK, color
        //    emoji, monochrome emoji, the base monospace face, and the
        //    symbols face (Noto Sans Symbols 2 → `❯` U+276F / `⏵`
        //    U+23F5 etc.). We keep handles to all of them so the chain
        //    composition below can promote the bundled base font over
        //    the bundled CJK font when the host monospace family is
        //    absent. The symbols face is registered as
        //    `FontRole::Secondary`, so the fallback chain picks it up
        //    automatically without an explicit local.
        let (bundled_cjk_id, emoji_id, bundled_mono_emoji_id, bundled_base_id, _symbols_id) =
            resolver.register_bundled();
        // Bundled Bold cuts so SGR-bold renders with real Bold weight even
        // when the host has no Inconsolata / Noto Sans JP installation.
        // Wired into the chain via `set_bold_variant` below.
        let (bundled_base_bold_id, bundled_cjk_bold_id) = resolver.register_bundled_bold_faces();

        // Host-font preferences sourced from `settings.font_family_fallback`:
        //   fallback[0] -> base (Latin / monospace)
        //   fallback[1] -> CJK fallback
        // Both slots are `Option<String>`: when the user has not specified
        // a family in that slot, the host scan is skipped entirely and the
        // bundled face wins — otherwise an installed `Inconsolata` /
        // `Noto Sans JP` would silently override the bundled fonts even
        // for users with an empty settings.json. The bundled CJK font's
        // Latin sub-set is not monospaced, so the chain still keeps the
        // bundled Inconsolata as the base when no host family is requested
        // (see `base_id` below).
        #[cfg(not(test))]
        let base_family: Option<String> = settings
            .font_family_fallback
            .first()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        #[cfg(not(test))]
        let cjk_family: Option<String> = settings
            .font_family_fallback
            .get(1)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        #[cfg(not(test))]
        let inconsolata_id = base_family
            .as_deref()
            .and_then(|f| resolver.register_system_family(f, FontRole::Base));
        #[cfg(test)]
        let inconsolata_id: Option<FontId> = None;

        // SGR-bold faces: register the real Bold cut of the base / CJK
        // families when the user requested a host family AND that family
        // ships a face of weight ≥ 600. Either condition failing leaves
        // bold cells on the regular face.
        #[cfg(not(test))]
        let base_bold_id = match (base_family.as_deref(), inconsolata_id) {
            (Some(f), Some(_)) => resolver.register_system_family_bold(f, FontRole::Base),
            _ => None,
        };
        #[cfg(test)]
        let base_bold_id: Option<FontId> = None;

        #[cfg(not(test))]
        let noto_sans_jp_id = cjk_family
            .as_deref()
            .and_then(|f| resolver.register_system_family(f, FontRole::Cjk));
        #[cfg(test)]
        let noto_sans_jp_id: Option<FontId> = None;

        #[cfg(not(test))]
        let cjk_bold_id = match (cjk_family.as_deref(), noto_sans_jp_id) {
            (Some(f), Some(_)) => resolver.register_system_family_bold(f, FontRole::Cjk),
            _ => None,
        };
        #[cfg(test)]
        let cjk_bold_id: Option<FontId> = None;

        // `host_emoji_id` / `host_mono_emoji_id` are registered above
        // (before `register_bundled`) so that a settings-supplied family
        // wins the resolver's `by_family` lookup against bundled +
        // system entries (FR6 priority #1).

        // Symbol fallback: covered by the bundled
        // `Noto Sans Symbols 2 (bundled)` registered in
        // `register_bundled()` above (FontRole::Secondary). The
        // previous host-side family probe (Noto Sans Symbols2 /
        // Symbola / DejaVu) is now redundant on Linux and never fired
        // on Windows anyway — bundling guarantees `❯` U+276F /
        // `⏵` U+23F5 / surrounding shapes everywhere.

        // Best-effort wider system scan (logged at WARN on failure;
        // family-name only, byte loading is deferred). Tests skip the
        // scan to keep cargo test deterministic.
        #[cfg(not(test))]
        resolver.scan_system_fonts();

        let rasterizer: Arc<dyn GlyphRasterizer> = match settings.font_engine {
            FontEngine::Swash => {
                let swash = Arc::new(
                    crate::render::font::swash_adapter::SwashRasterizer::with_axes(
                        &settings.variable_font_axes,
                    ),
                );
                swash.ingest_resolver(&resolver);
                log::info!(
                    "font.antialias = {}",
                    if swash.subpixel() {
                        "subpixel-rgb"
                    } else {
                        "grayscale (EMTERM_SUBPIXEL=0)"
                    }
                );
                swash
            }
            FontEngine::AbGlyph => {
                // ab_glyph escape hatch: we wrap the bundled CJK font
                // (which carries a Latin sub-set) so ASCII still
                // renders. CJK / emoji return None and the fallback
                // chain stops — that is the documented degradation
                // path (FR5).
                if !settings.variable_font_axes.is_empty() {
                    log::warn!(
                        "font.variable_axes: ignored under font_engine = ab_glyph (swash only)"
                    );
                }
                match crate::render::font::ab_glyph_adapter::AbGlyphRasterizer::from_static_bytes(
                    crate::render::font::resolver::BUNDLED_CJK_FONT,
                    bundled_cjk_id,
                ) {
                    Some(r) => {
                        log::info!("font_engine = ab_glyph (escape hatch); CJK / emoji may tofu");
                        Arc::new(r)
                    }
                    None => {
                        // Honor the "axes ignored under ab_glyph" contract
                        // even on this fallback: the user explicitly chose
                        // the no-variable-font escape hatch, so a parse
                        // failure that lands us back on swash must not
                        // silently re-enable the axes we just warned were
                        // off (`new()`, not `with_axes`).
                        log::warn!(
                            "font.unknown_engine: ab_glyph failed to parse bundled CJK; falling back to swash (variable_font_axes stay ignored)"
                        );
                        let swash =
                            Arc::new(crate::render::font::swash_adapter::SwashRasterizer::new());
                        swash.ingest_resolver(&resolver);
                        swash
                    }
                }
            }
        };

        // Pick the chain root: prefer the host-installed monospace
        // family when it loaded successfully; otherwise the bundled
        // Inconsolata covers the ASCII / Latin role so the base layer
        // still renders monospaced even when no host font is available.
        // We only fall back to the bundled CJK font as a last resort
        // (its Latin sub-set is not monospaced and visibly skews grid
        // alignment) — that case only occurs if both the host base
        // family and the bundled base font registration failed.
        let base_id = inconsolata_id.unwrap_or(bundled_base_id);
        let mut extras: Vec<FontId> = Vec::new();
        // FR6 priority #2: user-override font directory. The fonts are
        // already registered under `FontRole::User`, but they have to
        // appear in the chain ahead of CJK + emoji + secondary so the
        // per-codepoint walk consults them first.
        #[cfg(not(test))]
        for f in resolver.by_role(FontRole::User) {
            extras.push(f.id);
            log::info!("font.user = {} (id={:?})", f.family, f.id);
        }
        if let Some(jp) = noto_sans_jp_id {
            extras.push(jp);
        }
        if base_id != bundled_cjk_id {
            // Keep the bundled CJK font as a last-resort CJK fallback
            // (covers KR / TC / SC / extended CJK that NSJP omits).
            extras.push(bundled_cjk_id);
        }
        // Symbol fallback families registered above as
        // FontRole::Secondary (`Noto Sans Symbols2`, `Symbola`, etc.)
        // occupy the `font_family_fallback...` slot in SPEC FR8's
        // `[base, font_family_fallback..., emoji_font]` chain order.
        // They cover codepoints the base + CJK fonts miss — most
        // visibly `❯` U+276F shown by starship.
        //
        // Note: for codepoints in `is_pictographic`'s range
        // (0x2600..=0x27BF + emoji blocks), `FallbackChain::resolve`
        // checks `self.emoji` FIRST regardless of chain order, so
        // Secondary only catches a pictographic codepoint when the
        // emoji font does NOT cover it. Today Noto Color Emoji omits
        // dingbat ornaments like U+276F / U+2731, so the Secondary
        // chain catches them as intended. If a future emoji font
        // gains dingbat coverage, the `resolve_pictographic_falls_to_
        // secondary_when_emoji_misses` regression test in
        // `render/font/fallback.rs` pins the "miss → Secondary"
        // contract so the regression surfaces immediately.
        #[cfg(not(test))]
        for f in resolver.by_role(FontRole::Secondary) {
            extras.push(f.id);
            log::info!("font.symbol = {} (id={:?})", f.family, f.id);
        }
        // Color and monochrome emoji come from the bundled Noto faces
        // exclusively. The bundle is SSOT — host emoji fonts are not
        // consulted (e.g. Windows' system Noto Color Emoji ships as
        // COLRv1+SVG which swash cannot raster).
        extras.push(emoji_id);
        extras.push(bundled_mono_emoji_id);
        let preferred_emoji_id = emoji_id;
        #[cfg(not(test))]
        match (&base_family, inconsolata_id) {
            (Some(family), Some(id)) => {
                log::info!("font.base = {} (id={:?})", family, id);
            }
            (Some(family), None) => {
                log::warn!(
                    "font.base = bundled Inconsolata ({:?} not found on host)",
                    family
                );
            }
            (None, _) => {
                log::info!("font.base = bundled Inconsolata (no user override)");
            }
        }
        #[cfg(not(test))]
        match (&cjk_family, noto_sans_jp_id) {
            (Some(family), Some(id)) => {
                log::info!("font.jp = {} (id={:?})", family, id);
            }
            (Some(family), None) => {
                log::warn!(
                    "font.jp = bundled Noto Sans CJK JP ({:?} not found on host)",
                    family
                );
            }
            (None, _) => {
                log::info!("font.jp = bundled Noto Sans CJK JP (no user override)");
            }
        }
        log::info!("font.emoji = bundled Noto Color Emoji (id={:?})", emoji_id);
        let mut chain = FallbackChain::new(base_id, extras);
        // Mark the preferred emoji font as the color-emoji source so
        // VS-16-bearing clusters (e.g. ⚠️ = U+26A0 + U+FE0F) and bare
        // pictographs (✅ U+2705, 🟢 U+1F7E2) resolve to it instead of
        // the BW base / CJK fonts that may also cover those codepoints.
        chain.set_emoji(preferred_emoji_id);
        // Mark the bundled monochrome-emoji font so text-default emoji
        // code points (e.g. U+23F5 `⏵`) and VS15-attached clusters
        // route to the outline face instead of the BW base monospace
        // font (which has no glyph for them). FR5 "opposite-side
        // fallback before tofu" is handled inside
        // `FallbackChain::resolve_for_cluster`.
        chain.set_mono_emoji(bundled_mono_emoji_id);
        // Wire the real bold faces so SGR-bold cells render with them.
        // Bundled Bold cuts always cover the bundled Regular faces so the
        // default config (no system override) still renders bold correctly.
        chain.set_bold_variant(bundled_base_id, bundled_base_bold_id);
        chain.set_bold_variant(bundled_cjk_id, bundled_cjk_bold_id);
        log::info!(
            "font.base.bold = bundled Inconsolata (id={:?})",
            bundled_base_bold_id
        );
        log::info!(
            "font.jp.bold = bundled Noto Sans CJK JP (id={:?})",
            bundled_cjk_bold_id
        );
        // Layer the host-installed Bold cuts on top when the user asked for
        // a system family and it ships a weight ≥ 600 face. These shadow
        // the bundled wiring above only for the system Regular ids.
        #[cfg(not(test))]
        if let (Some(regular), Some(bold)) = (inconsolata_id, base_bold_id) {
            chain.set_bold_variant(regular, bold);
            log::info!(
                "font.base.bold = {} (id={:?})",
                base_family.as_deref().unwrap_or(""),
                bold
            );
        }
        #[cfg(not(test))]
        if let (Some(regular), Some(bold)) = (noto_sans_jp_id, cjk_bold_id) {
            chain.set_bold_variant(regular, bold);
            log::info!(
                "font.jp.bold = {} (id={:?})",
                cjk_family.as_deref().unwrap_or(""),
                bold
            );
        }
        #[cfg(test)]
        {
            // Silence unused-variable lints in the test cfg where the
            // bold ids are compile-time `None`.
            let _ = (base_bold_id, cjk_bold_id);
        }

        // Font smoke diagnostic — release/non-test only. Forces the
        // result into `emterm.log` even when the host has not enabled
        // `log_recording_enabled`, so a Windows user can hand us the
        // log file without editing settings.json first. Runs once at
        // startup so the cost is bounded.
        #[cfg(not(test))]
        {
            let probe_cp: u32 = 0x1F600; // 😀
            let covers = rasterizer.has_codepoint(preferred_emoji_id, probe_cp);
            let summary_chain: Vec<String> = chain
                .chain()
                .iter()
                .map(|id| {
                    resolver
                        .font(*id)
                        .map(|f| format!("{:?}={}", id, f.family))
                        .unwrap_or_else(|| format!("{:?}=?", id))
                })
                .collect();
            crate::logging::force_log_line(
                log::Level::Info,
                &format!(
                    "font.diag.chain = [{}] base={:?} emoji={:?} covers_U+1F600={}",
                    summary_chain.join(", "),
                    base_id,
                    preferred_emoji_id,
                    covers,
                ),
            );
            log::info!(
                "font.diag.chain = [{}] base={:?} emoji={:?} covers_U+1F600={}",
                summary_chain.join(", "),
                base_id,
                preferred_emoji_id,
                covers,
            );
            // Shape + raster smoke test for U+1F600 at a representative
            // terminal cell pixel size. Empty shape / None raster is the
            // first visible symptom on Windows; the log line tells us
            // which stage broke (charmap miss vs raster failure vs
            // bitmap-strike unavailable).
            let shaped = rasterizer.shape("\u{1F600}", preferred_emoji_id, 17.0);
            let first = shaped.into_iter().next();
            match first {
                None => {
                    let msg = format!(
                        "font.diag.smoke: shape returned no glyphs for U+1F600 (font={:?})",
                        preferred_emoji_id,
                    );
                    crate::logging::force_log_line(log::Level::Warn, &msg);
                    log::warn!("{}", msg);
                }
                Some(g) => match rasterizer.raster(g.font, g.glyph_id, g.size_px) {
                    None => {
                        let msg = format!(
                            "font.diag.smoke: raster returned None glyph_id={} size_px={} font={:?}",
                            g.glyph_id, g.size_px, g.font,
                        );
                        crate::logging::force_log_line(log::Level::Warn, &msg);
                        log::warn!("{}", msg);
                    }
                    Some(b) => {
                        let nonzero = match b.format {
                            crate::render::font::traits::AtlasFormat::Rgba => {
                                b.pixels.chunks_exact(4).filter(|px| px[3] != 0).count()
                            }
                            _ => b.pixels.iter().filter(|&&v| v != 0).count(),
                        };
                        let msg = format!(
                            "font.diag.smoke: raster ok format={:?} w={} h={} advance={:.1} bytes={} nonzero={}",
                            b.format,
                            b.width,
                            b.height,
                            b.advance,
                            b.pixels.len(),
                            nonzero,
                        );
                        crate::logging::force_log_line(log::Level::Info, &msg);
                        log::info!("{}", msg);
                    }
                },
            }
        }

        rasterizer.set_base_font(base_id);
        (
            Arc::new(resolver),
            Arc::new(chain),
            Arc::new(Mutex::new(GlyphCache::new())),
            rasterizer,
            base_id,
        )
    }

    /// Spawn the initial shell tab. Called once at startup.
    pub fn spawn_initial_tab(&mut self) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
            Some(self.status_bar_runtime.dispatcher()),
            Some(self.status_bar_runtime.cwd_provider()),
            self.notification_sink.clone(),
            None,
        );
        self.tabs.push(tab);
        self.active = 0;
        // A brand-new tab populated rows; ensure the first frame draws them.
        self.needs_full_redraw = true;
    }

    /// Spawn an additional shell tab using the global settings, switch to
    /// it, and request a repaint. Used by `AppAction::NewTabGlobal` and as
    /// the no-default-profile fallback of [`App::spawn_new_tab_profile_aware`].
    pub fn spawn_new_tab(&mut self) {
        self.spawn_new_tab_with_overrides(None);
    }

    /// Spawn an additional shell tab with optional profile spawn
    /// overrides, switch to it, and request a repaint.
    pub fn spawn_new_tab_with_overrides(
        &mut self,
        overrides: Option<crate::profiles::SpawnOverrides>,
    ) {
        let dims = self.cell_size;
        let tab = Tab::spawn_shell(
            "shell",
            dims.cols,
            dims.rows,
            self.settings.scrollback_lines,
            self.settings.clone(),
            Some(self.status_bar_runtime.dispatcher()),
            Some(self.status_bar_runtime.cwd_provider()),
            self.notification_sink.clone(),
            overrides,
        );
        // A fresh tab seeds its theme from `settings.font_size`; carry
        // the live zoom level over so a tab opened after the user zoomed
        // matches the existing tabs instead of snapping back to the
        // configured baseline.
        if (self.runtime_font_size_pt - self.settings.font_size).abs() >= f32::EPSILON {
            tab.theme.lock().font_size_pt = self.runtime_font_size_pt;
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        // FR4-adjacent: a freshly created tab lands at the end of the strip,
        // which is off-screen when the tabs overflow. Raise the same one-shot
        // scroll-into-view flag the keyboard switch path uses so the new tab
        // surfaces next frame. Unlike an existing-tab mouse click (already
        // visible, so it must NOT scroll — see `switch_to_tab`), a tab the user
        // has not seen yet should always surface, whether opened via the `+`
        // button or a keybind. All new-tab paths funnel through here.
        self.scroll_active_tab_into_view = true;
        self.needs_full_redraw = true;
    }

    /// `new_tab` keybind: apply the `is_default` profile when one exists,
    /// otherwise spawn with the global settings. Port of the WebView's
    /// `keyboard-handler.ts::handleNewTab`. A profile that fails to
    /// resolve (unknown SSH connection / unconfigured ssh path) logs an
    /// error and spawns nothing, matching the WebView's alert-and-abort.
    pub fn spawn_new_tab_profile_aware(&mut self) {
        let Some(profile) = crate::profiles::default_profile(&self.settings.profiles) else {
            self.spawn_new_tab();
            return;
        };
        match crate::profiles::resolve_spawn(profile, &self.settings) {
            Ok(overrides) => self.spawn_new_tab_with_overrides(Some(overrides)),
            Err(e) => log::error!("profile {:?}: {e}", profile.name),
        }
    }

    /// `profile_selector` keybind: open the modal selector. No-op when no
    /// profiles are configured (WebView parity:
    /// `keyboard-handler.ts::handleProfileSelector`).
    pub fn open_profile_selector(&mut self) {
        if self.settings.profiles.is_empty() {
            return;
        }
        self.profile_selector.open();
        self.needs_full_redraw = true;
    }

    /// Tab-bar `+` button (`TabEvent::New`): spawn directly with the
    /// global settings when no profiles exist and no tmux rows were
    /// found, otherwise open the new-tab chooser (a "Global Settings"
    /// row + each profile + each discovered tmux session / fallback
    /// socket row, with the default profile preselected). Port of the
    /// WebView's `tab-bar-ui.ts::handleNewTabClick` dialog, extended per
    /// task0001 AC-6: the fast path now requires BOTH profiles and tmux
    /// entries to be empty (previously profiles alone).
    pub fn open_new_tab_chooser(&mut self) {
        let entries = discover_tmux_entries();
        self.open_new_tab_chooser_with_entries(entries);
    }

    /// Core decision logic behind [`Self::open_new_tab_chooser`], taking
    /// the discovered tmux entries as a parameter. Split out so AC-6
    /// (profiles-empty × entries-empty/non-empty) is unit-testable
    /// without touching the real socket directory (`tdd-testing`: "test
    /// the pure decision core").
    pub(super) fn open_new_tab_chooser_with_entries(
        &mut self,
        entries: Vec<crate::ui::profile_selector::TmuxRow>,
    ) {
        if self.settings.profiles.is_empty() && entries.is_empty() {
            self.spawn_new_tab();
            return;
        }
        // Preselect the default profile's row (chooser mode prepends a
        // "Global Settings" row, so the row<->profile offset lives in
        // `ProfileSelectorState::profile_row`). No default → row 0
        // (Global Settings). Tmux entries never carry a "default", so
        // they never move the initial selection.
        self.profile_selector.open_with_global(0);
        self.profile_selector.tmux_entries = entries;
        if let Some(i) = self.settings.profiles.iter().position(|p| p.is_default) {
            self.profile_selector.selected = self.profile_selector.profile_row(i);
        }
        self.needs_full_redraw = true;
    }

    /// Number of rows the open selector shows (profiles + discovered tmux
    /// entries, plus the leading "Global Settings" row in new-tab chooser
    /// mode). Drives the keyboard wrap-around in `window_host`.
    pub fn profile_selector_row_count(&self) -> usize {
        self.settings.profiles.len()
            + self.profile_selector.tmux_entries.len()
            + usize::from(self.profile_selector.include_global)
    }

    /// Selector confirmed: resolve the chosen row and spawn a tab. The
    /// row→choice decode (including the chooser-mode "Global Settings" /
    /// tmux-entry offsets) lives in `ProfileSelectorState::row_to_choice`,
    /// the single authority shared with the renderer. Resolution failures
    /// log an error and spawn nothing (WebView parity with
    /// `launchSshProfile`'s alert path).
    pub fn confirm_profile_selection(&mut self, index: usize) {
        let choice = self
            .profile_selector
            .row_to_choice(index, self.settings.profiles.len());
        self.profile_selector.close();
        let profile_index = match choice {
            crate::ui::profile_selector::Choice::Global => {
                self.spawn_new_tab();
                return;
            }
            crate::ui::profile_selector::Choice::Tmux(i) => {
                // AC-5: attach is a plain PTY spawn (`tmux -S <socket>
                // attach[-session]`), not a mux-subsystem integration
                // (IMPLEMENTATION.md). `argv` was built by the shared
                // attach-argument rule (`tmux_sockets::attach_args`) when
                // the entry was discovered.
                let Some(entry) = self.profile_selector.tmux_entries.get(i) else {
                    return;
                };
                let overrides = crate::profiles::SpawnOverrides {
                    shell_path: Some("tmux".to_string()),
                    shell_args: Some(entry.argv.clone()),
                    ..Default::default()
                };
                self.spawn_new_tab_with_overrides(Some(overrides));
                return;
            }
            crate::ui::profile_selector::Choice::Profile(i) => i,
        };
        let Some(profile) = self.settings.profiles.get(profile_index) else {
            return;
        };
        match crate::profiles::resolve_spawn(profile, &self.settings) {
            Ok(overrides) => self.spawn_new_tab_with_overrides(Some(overrides)),
            Err(e) => log::error!("profile {:?}: {e}", profile.name),
        }
    }

    /// Close the tab at `idx`. Returns `true` when the close emptied
    /// the tabs vector, signaling the app loop that the window should
    /// exit. `tabs.is_empty()` is the same signal; this is a
    /// convenience for code that needs to branch immediately after
    /// the close.
    /// Request closing the tab at `idx`. When that tab has active SFTP
    /// uploads, the close is deferred behind a confirmation dialog (the
    /// `close_guard` is armed and the tab stays open) and `false` is returned.
    /// Otherwise it closes immediately via [`Self::close_tab`].
    pub fn request_close_tab(&mut self, idx: usize) -> bool {
        if let Some(tab) = self.tabs.get(idx) {
            if self.sftp_service.has_active_for_tab(tab.stable_id) {
                // Store the tab's stable_id, not its index: the roster can
                // change (tabs added/removed/reordered) while the guard dialog
                // is open, which would invalidate a stored index (#7).
                self.sftp_ui.close_guard = Some(tab.stable_id);
                return false;
            }
        }
        self.close_tab(idx)
    }

    /// Confirm a guarded tab close: cancel the guarded tab's active uploads,
    /// then close it. The guard holds a stable_id, resolved to a current index
    /// here; if the tab is already gone the guard is cleared cleanly.
    /// Returns true when the close emptied the tabs vector.
    pub fn confirm_close_guard(&mut self) -> bool {
        let Some(id) = self.sftp_ui.close_guard.take() else {
            return false;
        };
        let Some(idx) = self.tabs.iter().position(|t| t.stable_id == id) else {
            // The guarded tab no longer exists; nothing to close.
            return self.tabs.is_empty();
        };
        // Cancel only the guarded tab's sessions.
        self.sftp_service.cancel_for_tab(id);
        self.close_tab(idx)
    }

    /// Dismiss the close-guard dialog without closing the tab.
    pub fn cancel_close_guard(&mut self) {
        self.sftp_ui.close_guard = None;
    }

    pub fn close_tab(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return self.tabs.is_empty();
        }
        // task0005 AC-6 / task0009 AC-4: discard this tab's agent-status
        // entries AND its notification rate-limit bookkeeping before
        // removal (its own plain-tab key plus every mux pane in its window
        // group, if attached).
        for key in agent_status_keys_for_tab(&self.tabs[idx]) {
            let rate_limit_key = agent_notification_rate_limit_key(&self.mux_public_pane_ids, &key);
            self.discard_agent_notification_state(&rate_limit_key);
            self.agent_status.discard(&key);
        }
        // Drop the tab — its `PtySession::Drop` impl kills the child
        // and joins reader/writer threads.
        self.tabs.remove(idx);
        // Closing a tab shifts the active buffer; any open search overlay
        // is now indexed into a buffer that may no longer be active, so
        // clear it (mirrors the switch_to_tab rationale).
        if self.search.visible {
            self.search.close();
        }
        if self.tabs.is_empty() {
            self.active = 0;
            self.needs_full_redraw = true;
            return true;
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if idx < self.active {
            // Removed tab was to the left of the active one; shift left.
            self.active -= 1;
        }
        self.needs_full_redraw = true;
        // D2: the active index may have just moved onto a different tab
        // (the closed tab was the active one, or the last one); request a
        // reconcile of its size against `cell_size` (see
        // `request_active_tab_reconcile` for why this is deferred rather
        // than executed here). No-op when the shift only re-indexed the
        // SAME tab (still at `cell_size` already) or landed on a tab
        // already at size.
        self.request_active_tab_reconcile();
        false
    }

    /// Switch to the tab at `idx` (no-op for out-of-range / same idx).
    pub fn switch_to_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() || idx == self.active {
            return;
        }
        // FR3: park the outgoing tab's live scroll position into its own slot,
        // then commit the new active index, then reload the incoming tab's
        // saved position into the single active value (`scroll_position`) the
        // renderer and the scroll mutators read. `Live` restores to the
        // bottom; `OffsetFromLive(n)` restores to that offset. The scroll-pin
        // hot path (wheel / PageUp) is untouched — it still reads/writes the
        // single `scroll_position`; only the switch boundary swaps it.
        if let Some(outgoing) = self.tabs.get_mut(self.active) {
            outgoing.scroll_position = self.scroll_position;
        }
        self.active = idx;
        // FR4: the scroll-into-view flag is intentionally NOT raised here.
        // `switch_to_tab` is also reached by mouse paths (`TabEvent::Switch`
        // and `MuxSwitch`), and FR4 fires only on keyboard activation. The
        // keyboard handlers (`NextTab` / `PrevTab` / `JumpTab`) raise the flag
        // themselves after confirming the active index actually moved.
        if let Some(incoming) = self.tabs.get(self.active) {
            self.scroll_position = incoming.scroll_position;
        }
        // The search overlay's matches are indexed into the previous
        // tab's scrollback + viewport, so they are meaningless against
        // the new tab. Close the overlay + clear state on every active-
        // tab change (the WebView keeps a single SearchHandler with no
        // per-tab restore hook in the basic build, so closing is the
        // faithful, non-stale behavior).
        if self.search.visible {
            self.search.close();
        }
        // The selection holds absolute-row buffer coordinates of the
        // *previous* tab, which are meaningless against the new tab's
        // buffer (same rationale as closing the search overlay above), so
        // drop it on every active-tab change.
        self.selection = None;
        // The pending press anchor is likewise scoped to the previous tab's
        // buffer, so drop it as well.
        self.pending_selection_anchor = None;
        self.needs_full_redraw = true;
        // D2: the incoming tab just became the committed active tab;
        // request that its size be brought in line with the current
        // display area — deferred to `execute_pending_reconcile` rather
        // than run here (see that method's doc for why: at this point in
        // the frame `cell_size` may still describe the OUTGOING tab's
        // display area).
        self.request_active_tab_reconcile();
    }

    /// D2 (FR3, task0003 rework): record that the active tab may need
    /// reconciling against `self.cell_size`. Deliberately does NOT compare
    /// or resize here — at the moment every activation path (explicit
    /// switch, close-tab fix-up, exited-tab reap fix-up) calls this,
    /// `self.cell_size` can still hold dims computed for the OUTGOING tab:
    /// the persistent mux sidebar's width inset depends on whether the
    /// ACTIVE tab is mux-attached (`App::mux_sidebar_visibility`), so a
    /// synchronous compare-and-resize here would size the incoming tab
    /// against the wrong display area (round-1 findings dbb7766a6212fb1a /
    /// 09f0e6096bbc36ee). The actual comparison happens in
    /// [`Self::execute_pending_reconcile`], invoked by `window_host::render`
    /// once insets for the NEW active tab have settled. Consecutive
    /// requests before that point collapse into one.
    pub(super) fn request_active_tab_reconcile(&mut self) {
        self.pending_active_tab_reconcile = true;
    }

    /// D2 (FR3, task0003 rework): consume a pending activation-reconcile
    /// request (if any). Called by `window_host::render` at exactly one
    /// point — after the status-bar insets, the mux-sidebar inset, and any
    /// pending display-area resize have settled `self.cell_size` for the
    /// NEW active tab — so the comparison below is against the INCOMING
    /// tab's own settled display area, never the outgoing tab's stale one.
    /// On a mismatch, resizes that tab (and ONLY that tab) through the App
    /// resize application path ([`Self::apply_tab_resize`], FR6); on a
    /// match, issues nothing and clears nothing (FR3's no-op guarantee) —
    /// most calls are no-ops, since a tab's size only ever drifts from
    /// `cell_size` while it sits inactive. A request with no matching
    /// active tab (e.g. all tabs closed) is dropped silently.
    pub fn execute_pending_reconcile(&mut self) {
        if !std::mem::take(&mut self.pending_active_tab_reconcile) {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let (cols, rows) = {
            let core = tab.core.lock();
            (core.cols(), core.rows())
        };
        if cols != self.cell_size.cols || rows != self.cell_size.rows {
            self.apply_tab_resize(self.active, self.cell_size.cols, self.cell_size.rows);
        }
    }

    /// Design 2 (FR6, task0003 rework): the single App-side path for
    /// issuing a `Tab::resize` on ANY origin — a display-area change
    /// (`set_grid_size`) or an activation reconcile
    /// (`execute_pending_reconcile`). Reads the target tab's pre-resize
    /// column count, applies the resize (`Tab::resize` owns the clamp, the
    /// tab's OWN reflow-invalidated trackers, and mux pane `Resize` frame
    /// emission — contracts (a)-(e), untouched), then — if the resize
    /// changed the tab's (post-clamp) column count — clears the App-OWNED
    /// reflow-invalidated trackers (selection, pending selection anchor;
    /// D3). BOTH callers issue their resize through this path, so the
    /// App-side half of the D3 split holds on every origin regardless of
    /// which activation path (or `set_grid_size`) triggered it — it is
    /// never left to caller preconditions (round-1 findings
    /// a172de726b3cbc29 / d39a6a9468ff892e).
    pub(super) fn apply_tab_resize(&mut self, idx: usize, cols: u16, rows: u16) {
        let Some(tab) = self.tabs.get_mut(idx) else {
            return;
        };
        let pre_resize_cols = tab.core.lock().cols();
        tab.resize(cols, rows);
        let post_resize_cols = tab.core.lock().cols();
        if post_resize_cols != pre_resize_cols {
            self.selection = None;
            self.pending_selection_anchor = None;
        }
    }

    /// Move the tab at `from` to land at position `to` (drag-and-drop
    /// reorder). `to` is the **insertion index** into the post-removal
    /// vector, in 0..=tabs.len(). No-op when:
    ///
    /// - `from` is out of range
    /// - `to` falls on either side of `from` (would land in the same
    ///   slot — `from` itself or immediately after it).
    ///
    /// The active index is fixed up so the same logical tab remains
    /// active after the move.
    pub fn reorder_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() {
            return;
        }
        // Skip moves that would land the tab in its current slot.
        if to == from || to == from + 1 {
            return;
        }
        let to = to.min(self.tabs.len());
        let tab = self.tabs.remove(from);
        // After `remove`, indices >= `from` shifted down by 1; adjust
        // the insertion point to match the user's intent.
        let insert_at = if to > from { to - 1 } else { to };
        self.tabs.insert(insert_at, tab);

        // Fix up `self.active` so the logically-active tab follows the
        // move. Four cases:
        //   - The moved tab was active → it now lives at `insert_at`.
        //   - Active was to the left of `from` and to the left of
        //     `insert_at` → unchanged.
        //   - Active was to the right of `from` and to the right of
        //     `insert_at` → unchanged.
        //   - Otherwise the active tab shifts by ±1.
        if self.active == from {
            self.active = insert_at;
        } else if from < self.active && insert_at >= self.active {
            self.active -= 1;
        } else if from > self.active && insert_at <= self.active {
            self.active += 1;
        }

        self.needs_full_redraw = true;
    }

    /// Open the settings window (child `--settings` process). Replaces
    /// the former in-app egui settings tab: the WebView settings panel
    /// runs in its own window, and saves are applied live through the
    /// launcher's save-event watcher.
    pub fn open_settings_window(&mut self) {
        self.settings_launcher.open();
    }

    /// Apply a [`crate::ui::TabEvent`] emitted by the tab bar widget.
    /// Returns `true` when the resulting state should exit the window
    /// (i.e. the last tab was closed).
    pub fn apply_tab_event(&mut self, evt: crate::ui::TabEvent) -> bool {
        // Mux rename / move dialogs commit against `self.tabs.get_mut(self.active)`
        // (they only carry a `window_id`, not an owning-tab anchor). Any tab
        // event that mutates `self.tabs` / `self.active` (creation, close,
        // reorder, switch, mux-switch — and any future variant we add) makes
        // the captured anchor stale or out-of-range, so the conservative
        // choice is to drop *every* tab event while a dialog is open. Fail
        // closed: a new `TabEvent` variant added later defaults to "blocked
        // while dialog open", not "silently leaks past the guard".
        if self.mux_dialog_open() {
            return false;
        }
        match evt {
            crate::ui::TabEvent::New => {
                // `+` button: plain spawn without profiles, otherwise the
                // new-tab chooser modal (WebView `handleNewTabClick`).
                self.open_new_tab_chooser();
                false
            }
            crate::ui::TabEvent::OpenSettings => {
                self.open_settings_window();
                false
            }
            crate::ui::TabEvent::Close(idx) => self.request_close_tab(idx),
            crate::ui::TabEvent::Switch(idx) => {
                if idx < self.tabs.len() {
                    self.switch_to_tab(idx);
                }
                false
            }
            crate::ui::TabEvent::Reorder { from, to } => {
                if from >= self.tabs.len() {
                    return false;
                }
                self.reorder_tab(from, to.min(self.tabs.len()));
                false
            }
            crate::ui::TabEvent::MuxSwitch { tab, window } => {
                // Sub-tab click: focus the tab if needed, then switch the
                // mux window (local active index + `SwitchWindow`), mirroring
                // the keyboard `prefix 0..9` path (FR5).
                if tab < self.tabs.len() {
                    if self.active != tab {
                        self.switch_to_tab(tab);
                    }
                    // Swap the active scroll value through the per-pane slots
                    // (FR3) and force a full redraw on a committed switch
                    // (FR2), mirroring the keyboard prefix path.
                    let mut scroll = self.scroll_position;
                    let outcome = match self.tabs.get_mut(self.active) {
                        Some(t) => Self::switch_to(t, Some(window), &mut scroll),
                        None => MuxActionOutcome::None,
                    };
                    if outcome == MuxActionOutcome::Changed {
                        self.scroll_position = scroll;
                        self.needs_full_redraw = true;
                        // task0002 AC-6: the sidebar-row-click switch path
                        // also counts as a switch (SPEC.md A6).
                        self.record_mux_window_switch();
                    }
                }
                false
            }
        }
    }

    /// Apply a global keybind [`crate::ui::AppAction`]. Returns `true`
    /// when the resulting state should exit the window.
    pub fn apply_action(&mut self, action: crate::ui::AppAction) -> bool {
        match action {
            crate::ui::AppAction::NewTab => {
                // Profile-aware: applies the `is_default` profile when one
                // exists (WebView `handleNewTab` parity).
                self.spawn_new_tab_profile_aware();
                false
            }
            crate::ui::AppAction::NewTabGlobal => {
                self.spawn_new_tab();
                false
            }
            crate::ui::AppAction::OpenProfileSelector => {
                self.open_profile_selector();
                false
            }
            crate::ui::AppAction::CloseTab => {
                let idx = self.active;
                self.close_tab(idx)
            }
            crate::ui::AppAction::NextTab => {
                let total = self.tabs.len();
                if total == 0 {
                    return false;
                }
                let next = (self.active + 1) % total;
                let before = self.active;
                self.switch_to_tab(next);
                // FR4: keyboard activation → scroll the new active cell into
                // view, but only when the active index actually moved.
                if self.active != before {
                    self.scroll_active_tab_into_view = true;
                }
                false
            }
            crate::ui::AppAction::PrevTab => {
                let total = self.tabs.len();
                if total == 0 {
                    return false;
                }
                let prev = if self.active == 0 {
                    total - 1
                } else {
                    self.active - 1
                };
                let before = self.active;
                self.switch_to_tab(prev);
                // FR4: keyboard activation → scroll the new active cell into
                // view, but only when the active index actually moved.
                if self.active != before {
                    self.scroll_active_tab_into_view = true;
                }
                false
            }
            crate::ui::AppAction::JumpTab(n) => {
                let total = self.tabs.len();
                if total == 0 {
                    return false;
                }
                // n is 1-based and clamped to the existing tab range.
                let idx = (n.saturating_sub(1) as usize).min(total - 1);
                let before = self.active;
                self.switch_to_tab(idx);
                // FR4: keyboard activation → scroll the new active cell into
                // view, but only when the active index actually moved.
                if self.active != before {
                    self.scroll_active_tab_into_view = true;
                }
                false
            }
            crate::ui::AppAction::SelectAll => {
                self.select_all();
                false
            }
            crate::ui::AppAction::JumpToPrevPrompt => {
                self.jump_to_prompt(JumpDirection::Prev);
                false
            }
            crate::ui::AppAction::JumpToNextPrompt => {
                self.jump_to_prompt(JumpDirection::Next);
                false
            }
            crate::ui::AppAction::OpenSettings => {
                self.open_settings_window();
                false
            }
            // The remaining view-level actions need the host's
            // `winit::window::Window` handle and/or the deferred-resize
            // machinery, so the keyboard handler in `window_host`
            // dispatches them directly against `WindowHost` instead of
            // routing them here. They are listed explicitly (rather than
            // a catch-all) so a future variant cannot silently fall into
            // a no-op arm.
            crate::ui::AppAction::ZoomIn
            | crate::ui::AppAction::ZoomOut
            | crate::ui::AppAction::ZoomReset
            | crate::ui::AppAction::ToggleFullscreen
            | crate::ui::AppAction::ToggleTabBar
            | crate::ui::AppAction::OpenSearch => false,
        }
    }

    /// Select the entire visible viewport of the active tab. No-op when
    /// there is no active tab or the grid is empty.
    ///
    /// Coordinate-system note: [`crate::selection::Pos`] addresses
    /// **absolute** buffer rows (the same frame `fold` / `prompts` /
    /// `search` use). "Select all" covers the on-screen rows, expressed in
    /// absolute coordinates: the top visible row is
    /// `visible_start = scrollback_len - scroll_offset` (saturating), and
    /// the selection spans `(visible_start, 0)` to
    /// `(visible_start + rows - 1, cols - 1)`. Because the endpoints are
    /// absolute, the highlight stays anchored to the same buffer lines if
    /// the viewport scrolls afterward (and a later scrollback-aware
    /// "select all including history" only needs a wider row range).
    ///
    /// When a fold layout is active the screen rows are non-contiguous in
    /// absolute space (collapsed bodies are hidden), so the top/bottom
    /// endpoints are read from the layout's first/last visible rows rather
    /// than the linear `visible_start + (rows - 1)` model — mirroring the
    /// mouse path's `screen_row_to_abs` mapping.
    pub fn select_all(&mut self) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let (cols, rows, scrollback_len) = {
            let core = tab.core.lock();
            (core.cols(), core.rows(), core.get_scrollback_length())
        };
        if cols == 0 || rows == 0 {
            return;
        }
        // Derive the visible row span from the fold layout when one is active
        // (collapsed bodies make the screen rows non-contiguous in absolute
        // space), mirroring the mouse path's screen_row_to_abs mapping. Falls
        // back to the linear scrollback model when no layout is active.
        let row_to_abs = |kind: &crate::fold::FoldRowKind| -> u32 {
            match kind {
                crate::fold::FoldRowKind::Cells { actual_line } => *actual_line,
                crate::fold::FoldRowKind::Summary { region } => region.start_line,
            }
        };
        let (anchor_row, extent_row) = match self.fold_layout() {
            Some(layout) if !layout.rows.is_empty() => (
                row_to_abs(layout.rows.first().unwrap()),
                row_to_abs(layout.rows.last().unwrap()),
            ),
            _ => {
                let visible_start = scrollback_len.saturating_sub(self.scroll_offset());
                (visible_start, visible_start + (rows - 1) as u32)
            }
        };
        self.selection = Some(Selection {
            anchor: crate::selection::Pos {
                row: anchor_row,
                col: 0,
            },
            extent: crate::selection::Pos {
                row: extent_row,
                col: cols - 1,
            },
            mode: crate::selection::SelectionMode::Character,
            origin: crate::selection::Pos {
                row: anchor_row,
                col: 0,
            },
        });
        self.needs_full_redraw = true;
    }

    /// Increase the runtime terminal font size by one point (clamped to
    /// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]). Returns `true` when
    /// the size actually changed so the caller can reshape the grid.
    pub fn zoom_in(&mut self) -> bool {
        self.set_font_size_pt(self.runtime_font_size_pt + FONT_SIZE_PT_STEP)
    }

    /// Decrease the runtime terminal font size by one point (clamped).
    /// Returns `true` when the size actually changed.
    pub fn zoom_out(&mut self) -> bool {
        self.set_font_size_pt(self.runtime_font_size_pt - FONT_SIZE_PT_STEP)
    }

    /// Reset the runtime terminal font size back to the configured
    /// `settings.font_size`. Returns `true` when the size actually
    /// changed.
    pub fn zoom_reset(&mut self) -> bool {
        self.set_font_size_pt(self.settings.font_size)
    }

    /// Set the runtime terminal font size to `new_pt` (clamped to
    /// [`FONT_SIZE_PT_MIN`]..=[`FONT_SIZE_PT_MAX`]). On a real change:
    /// re-derive `cell_w_logical` / `cell_h_logical` from the font stack
    /// at the new pixel size, push the new point size into every tab's
    /// `Theme` (leaving the rest of each theme's OSC-mutated state
    /// intact), force a full redraw, and return `true`. Returns `false`
    /// (no mutation) when the clamped target equals the current size.
    ///
    /// The PTY grid is *not* reshaped here — the caller (`window_host`)
    /// owns the window pixel size and triggers a deferred resize so the
    /// new cell metrics produce the right `(cols, rows)` on the next
    /// frame.
    pub fn set_font_size_pt(&mut self, new_pt: f32) -> bool {
        let clamped = clamp_font_size_pt(new_pt);
        if (clamped - self.runtime_font_size_pt).abs() < f32::EPSILON {
            return false;
        }
        self.runtime_font_size_pt = clamped;
        // Re-derive cell metrics at the new size. `font_size_px` applies
        // the same 96/72 pt→px conversion `settings.font_size_px()` uses
        // at startup so the grid stays consistent with the WebView build.
        let new_px = clamped * crate::settings::PT_TO_PX;
        let (cell_w, cell_h) = crate::render::compute_cell_dims(
            self.font_rasterizer.as_ref(),
            self.font_fallback.as_ref(),
            new_px,
        );
        self.cell_w_logical = cell_w;
        self.cell_h_logical = cell_h;
        // Push the new point size into every tab's theme. Only
        // `font_size_pt` is touched so OSC-driven palette / cursor
        // mutations a tab accumulated are preserved.
        for tab in &self.tabs {
            tab.theme.lock().font_size_pt = clamped;
        }
        self.needs_full_redraw = true;
        true
    }

    /// Apply a settings draft committed by the in-app settings panel to
    /// the running app. Re-derives every startup-resolved state that
    /// the panel's categories can affect; settings that only bind at
    /// tab spawn time (`shell_path` / `shell_args` /
    /// `scrollback_lines`) intentionally reach new tabs only, matching
    /// the WebView build.
    ///
    /// Returns `true` when the caller (`window_host`) must reshape the
    /// window grid (cell metrics or padding changed).
    pub fn apply_settings(&mut self, mut new: Settings) -> bool {
        crate::settings_store::clamp_for_save(&mut new);
        let old = Arc::clone(&self.settings);

        // The profile selector / new-tab chooser renders its rows live
        // from `self.settings.profiles` and its highlight index is bound
        // to that list. A settings save (from the external WebView
        // settings window) can add / remove / reorder profiles while the
        // modal is open, leaving `selected` pointing past the new list or
        // at a different profile than the highlighted row. Close it so the
        // user never confirms against a stale list (the WebView rebuilt
        // the list on every open; closing is the equivalent invariant).
        if self.profile_selector.visible {
            self.profile_selector.close();
            self.needs_full_redraw = true;
        }

        // UI chrome palette: preset × brightness swaps live (the md3
        // slot is process-wide, so the next frame re-skins every
        // widget).
        crate::ui::md3::set_preset(new.ui_theme_preset, new.ui_theme);
        // Keybinds / locale resolve the same way as startup.
        self.keybinds = crate::ui::keybinds::KeybindTable::from_settings(&new.keybinds);
        self.locale = crate::i18n::resolve(new.language);

        // mux: rebuild the prefix latch (chord + action bindings) from the
        // new settings (FR11 dynamic apply). The tab group always renders its
        // windows as sub-tabs (WebView parity), so there is no expand
        // preference to push onto tabs.
        self.mux_latch = build_mux_latch(&new);

        let font_families_changed = new.font_family_fallback != old.font_family_fallback;
        let font_size_changed = (new.font_size - old.font_size).abs() >= f32::EPSILON;
        let padding_changed = new.padding != old.padding;

        self.settings = Arc::new(new);

        // Reflect the (possibly changed) SFTP concurrency cap onto the live
        // pool so reload takes effect without restarting in-flight uploads.
        self.sftp_service
            .set_max_concurrent(self.settings.sftp_max_concurrent_uploads);

        if font_families_changed {
            let (resolver, fallback, cache, rasterizer, base_id) =
                Self::build_font_stack(&self.settings);
            self.font_resolver = resolver;
            self.font_fallback = fallback;
            self.font_cache = cache;
            self.font_rasterizer = rasterizer;
            self.font_base_id = base_id;
        }

        // Re-derive cell metrics. A font_size change routes through
        // `set_font_size_pt` (which also pushes the size into every tab
        // theme); a pure family swap keeps the size but the new chain's
        // advance may differ, so recompute the dims in place.
        if font_size_changed {
            self.set_font_size_pt(self.settings.font_size);
        } else if font_families_changed {
            let px = self.runtime_font_size_pt * crate::settings::PT_TO_PX;
            let (w, h) = crate::render::compute_cell_dims(
                self.font_rasterizer.as_ref(),
                self.font_fallback.as_ref(),
                px,
            );
            self.cell_w_logical = w;
            self.cell_h_logical = h;
        }

        // Rebuild every tab's theme from the new settings (color scheme
        // / cursor style / bold-brighten), preserving the live zoom
        // level. OSC-driven palette mutations a tab accumulated are
        // reset — same outcome as the WebView's
        // `applyTerminalColorScheme` full remap.
        for tab in &mut self.tabs {
            let mut theme = crate::render::theme::Theme::from_settings(self.settings.as_ref());
            theme.font_size_pt = self.runtime_font_size_pt;
            {
                // FR5 (cursor-settings-fix task0004 AC-2/AC-3): an active
                // OSC 12 cursor-color override survives this rebuild.
                // `scheme_cursor_fg` above already reflects the NEW
                // settings' scheme (so a later OSC 112 restores THAT
                // color); only `cursor_fg` + the override flag carry
                // forward from the old theme.
                let old_theme = tab.theme.lock();
                if old_theme.cursor_fg_override_active {
                    theme.cursor_fg = old_theme.cursor_fg;
                    theme.cursor_fg_override_active = true;
                }
            }
            *tab.theme.lock() = theme;
            {
                let mut core = tab.core.lock();
                core.set_cursor_blink(self.settings.cursor_blink);
                core.set_cursor_style(self.settings.cursor_style.as_cursor_shape_u8());
                core.mark_all_dirty();
            }
            tab.set_fold_enabled(self.settings.fold_enabled);
        }

        self.needs_full_redraw = true;
        font_size_changed || font_families_changed || padding_changed
    }

    #[allow(dead_code)] // retained for window_host / tests
    pub fn has_tabs(&self) -> bool {
        !self.tabs.is_empty()
    }

    /// Build a per-frame view model for the status-bar widget. The
    /// runtime owns the template engine, providers, and OSC
    /// dispatcher; this method refreshes the shared cwd cell the
    /// providers read through.
    ///
    /// The render pipeline calls this once per frame and hands the
    /// result to [`crate::ui::status_bar::draw`]. Mux attach state is
    /// not an input (mux-status-bar-removal task0001, FR1/FR5): the
    /// view model is a pure function of settings and the OSC
    /// `777;statusbar` dispatcher's own state.
    pub fn status_bar_view_model(&self) -> crate::status_bar::StatusBarViewModel {
        // Refresh the cwd snapshot the providers read through their
        // `CwdSource` closure. The lock is held only for the duration
        // of the swap.
        let active_cwd_value = self
            .active_tab()
            .and_then(|t| t.cb_state.lock().cwd.clone());
        *self.active_cwd.lock() = active_cwd_value;

        self.status_bar_runtime
            .build_view_model(&self.settings.statusbar)
    }

    /// Build the current status-bar view model and compare it against
    /// the snapshot stored in [`Self::previous_status_bar_view_model`].
    ///
    /// Returns `true` when the model differs from the previous frame
    /// (or no previous frame was recorded yet). Called by
    /// `WindowHost::render` so the dirty-row skip path bypasses its
    /// early-return when the status bar's content has changed even
    /// though no terminal cell needs to repaint — the canonical case
    /// being the wall-clock `{time}` provider's per-second tick on an
    /// otherwise-idle PTY.
    ///
    /// Constructing the view model here re-runs the per-frame resolve
    /// cache; the second call inside `egui::run` hits that cache on
    /// the no-change path, so the extra work is bounded to one
    /// `VariableProvider::version` poll per registered variable.
    pub fn status_bar_view_model_changed(&self) -> bool {
        let current = self.status_bar_view_model();
        match &self.previous_status_bar_view_model {
            Some(prev) => *prev != current,
            None => true,
        }
    }
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    // ── task0006: agent-status query surface for the UI layer ──────────
    // Read-only projections of `Self::agent_status` /
    // `Self::mux_public_pane_ids` for `ui::tab_bar` / `ui::mux_sidebar` /
    // `ui::status_bar`. The render pipeline calls these once per frame;
    // none of them mutate state (mirrors `status_bar_view_model`'s
    // read-only contract).

    /// `tab`'s aggregated agent-status badge (task0006 AC-1/AC-2):
    /// highest-priority state across the tab's own plain-tab status and
    /// every pane in its mux window group (if attached), or `None` when
    /// nothing has ever reported a state — the caller renders no badge and
    /// reserves no layout space for it in that case.
    pub fn agent_status_badge_for(
        &self,
        tab: &Tab,
    ) -> Option<crate::agent_status_model::Aggregated> {
        let keys = agent_status_keys_for_tab(tab);
        self.agent_status.aggregate(keys.iter())
    }

    /// A single mux pane's aggregated badge, by wire `pane_id` (task0006:
    /// `ui::mux_sidebar` window-entry badge — one pane per window entry).
    pub fn agent_status_pane_badge(
        &self,
        pane_id: u32,
    ) -> Option<crate::agent_status_model::Aggregated> {
        self.agent_status
            .aggregate([&crate::agent_status_model::PaneKey::MuxPane(pane_id)])
    }

    /// The daemon-minted public ID for mux pane `pane_id`, if the GUI has
    /// learned it yet (task0006 AC-5). `None` until the daemon pushes at
    /// least one `AgentStatusUpdate` for the pane — see
    /// [`Self::mux_public_pane_ids`].
    pub fn mux_public_pane_id(&self, pane_id: u32) -> Option<&str> {
        self.mux_public_pane_ids.get(&pane_id).map(String::as_str)
    }

    /// Whether the active tab carries an attached mux window group with at
    /// least one window (`MuxWindowGroup::is_group`). Shared predicate for
    /// [`Self::mux_sidebar_visibility`] and the tab-bar label (task0005
    /// AC-1/AC-6).
    fn active_tab_mux_attached(&self) -> bool {
        self.active_tab()
            .and_then(|t| t.mux_group.as_ref())
            .map(|g| g.is_group())
            .unwrap_or(false)
    }

    /// Which mux window-sidebar variant (if any) is visible on the current
    /// frame, given `settings.mux.window_sidebar_overlay`, the runtime
    /// overlay flag ([`Self::mux_sidebar_overlay_open`]), and whether the
    /// active tab is mux-attached ([`Self::active_tab_mux_attached`]) —
    /// task0005 FR2/FR4/FR5:
    ///
    /// - `Persistent`: overlay-mode setting is `false` AND the active tab
    ///   is mux-attached.
    /// - `Overlay`: overlay-mode setting is `true` AND the runtime overlay
    ///   flag is set AND the active tab is mux-attached.
    /// - `Hidden`: neither of the above (in particular, local tabs never
    ///   show either variant).
    ///
    /// Read by both `render` (which widget variant to draw, if any) and
    /// geometry code ([`mux_sidebar_grid_inset`] — only `Persistent`
    /// contributes a grid WIDTH inset; task0006 right-edge placement means
    /// no x-origin math reads this).
    pub fn mux_sidebar_visibility(&self) -> MuxSidebarVisibility {
        if !self.active_tab_mux_attached() {
            return MuxSidebarVisibility::Hidden;
        }
        if self.settings.mux.window_sidebar_overlay {
            if self.mux_sidebar_overlay_open {
                MuxSidebarVisibility::Overlay
            } else {
                MuxSidebarVisibility::Hidden
            }
        } else {
            MuxSidebarVisibility::Persistent
        }
    }

    /// Horizontal grid WIDTH inset (logical px) the persistent sidebar
    /// reserves for `window_width_logical` on the current frame. Thin
    /// wrapper around [`mux_sidebar_grid_inset`] for callers holding
    /// `&App`. Despite the name, this is consumed as a width-only
    /// reduction, never an x-origin term (task0006 right-edge placement
    /// update) — `window_host::grid_size` reads the equivalent value via
    /// [`mux_sidebar_grid_inset`] directly (deriving the window width from
    /// `surface_config` rather than an egui context).
    pub fn mux_sidebar_x_inset(&self, window_width_logical: f32) -> f32 {
        mux_sidebar_grid_inset(self.mux_sidebar_visibility(), window_width_logical)
    }

    // ── task0002: mux sidebar overlay auto-dim (app-side wiring) ────────

    /// Set the overlay card's hover flag (task0002 FR3). Called from
    /// `window_host`'s `PointerMoved` / `PointerLeft` handlers with the
    /// result of `ui::mux_sidebar::point_in_sidebar` evaluated against
    /// [`crate::ui::mux_sidebar::Placement::Overlay`] — the same hit test
    /// press/wheel routing already query, so hover and click routing can
    /// never disagree about the boundary.
    pub fn set_mux_sidebar_hovered(&mut self, hovered: bool) {
        self.mux_sidebar_overlay_hovered = hovered;
    }

    /// Whether the pointer is currently inside the overlay card's rect.
    pub fn mux_sidebar_overlay_hovered(&self) -> bool {
        self.mux_sidebar_overlay_hovered
    }

    /// task0002 AC-9 / D5: whether [`Self::mux_sidebar_overlay_hovered`]
    /// differs from the snapshot taken at the last actually-rendered frame
    /// (`record_render_state` / `record_render_state_no_tab`). `window_host`
    /// folds this into `overlay_work` so a hover-predicate transition is
    /// never dropped by the frame-skip gate — bare `PointerMoved` is
    /// deliberately excluded from `has_actionable_egui_input`, so without
    /// this the card would never actually brighten on hover-enter.
    pub fn mux_sidebar_hover_changed(&self) -> bool {
        self.mux_sidebar_overlay_hovered != self.mux_sidebar_hover_prev_render
    }

    /// Record a mux window switch (task0002 FR4): stamps
    /// [`Self::mux_sidebar_last_switch`] with `now`. A later switch
    /// overwrites the timestamp, which is what lets a rapid sequence
    /// extend brightness (FR4 / AC-3). Called from
    /// [`Self::dispatch_mux_action`]'s `NextWindow` / `PrevWindow` /
    /// `SelectWindow` arms and from [`Self::apply_tab_event`]'s
    /// `MuxSwitch` arm (the sidebar-row-click path) — AC-6.
    pub(super) fn record_mux_window_switch(&mut self) {
        self.mux_sidebar_last_switch = Some(Instant::now());
    }

    /// Resolve the overlay card's current whole-card opacity plus whether
    /// a further frame is needed (task0002 D1). Mutating: arms/resets
    /// [`Self::mux_sidebar_fade_started`] before delegating to the pure
    /// [`resolve_mux_sidebar_dim_opacity`] — while bright, the fade origin
    /// is cleared (ready to arm fresh on the next bright-to-dim
    /// transition); while dim, it is armed via `get_or_insert` (a no-op if
    /// a fade is already tracked, so calling this more than once per tick
    /// does not restart an in-flight fade). Called once per frame from
    /// `window_host::render`, immediately before the egui pass, so
    /// `render::draw_terminal` (which only holds `&App`) can thread the
    /// already-resolved value down to `ui::mux_sidebar::draw`.
    pub fn resolve_mux_sidebar_opacity(&mut self, now: Instant) -> (f32, bool) {
        let bright = self.mux_sidebar_overlay_hovered
            || self
                .mux_sidebar_last_switch
                .is_some_and(|t| now < t + OVERLAY_BRIGHT_HOLD);
        if bright {
            self.mux_sidebar_fade_started = None;
        } else {
            self.mux_sidebar_fade_started.get_or_insert(now);
        }
        resolve_mux_sidebar_dim_opacity(
            self.mux_sidebar_overlay_hovered,
            self.mux_sidebar_last_switch,
            self.mux_sidebar_fade_started,
            now,
        )
    }

    /// Next `Instant` the event loop must wake for the overlay dim/fade
    /// feature (task0002 D5 / AC-5), or `None` when settled OR the overlay
    /// is not currently shown (a hidden sidebar / persistent mode arms no
    /// deadline at all). Read-only — delegates to
    /// [`resolve_mux_sidebar_dim_deadline`] once the visibility gate
    /// passes.
    pub fn next_mux_sidebar_dim_deadline(&self, now: Instant) -> Option<Instant> {
        if self.mux_sidebar_visibility() != MuxSidebarVisibility::Overlay {
            return None;
        }
        resolve_mux_sidebar_dim_deadline(
            self.mux_sidebar_overlay_hovered,
            self.mux_sidebar_last_switch,
            self.mux_sidebar_fade_started,
            now,
        )
    }

    /// Whether `about_to_wait` should request a redraw right now for the
    /// overlay dim/fade feature (task0002 D5, mirrors `needs_bell_repaint`
    /// / `needs_blink_repaint`'s role for their own concerns). Read-only —
    /// all state mutation happens in [`Self::resolve_mux_sidebar_opacity`],
    /// called from `window_host::render` once the redraw this triggers
    /// actually runs.
    pub fn mux_sidebar_dim_due(&self, now: Instant) -> bool {
        // Edge case (SPEC.md): a closed/hidden sidebar arms no deadline
        // and requests no redraw for this feature — its dim state is
        // simply frozen wherever it was until the overlay is shown again
        // (`resolve_mux_sidebar_opacity` is likewise only called while
        // visible — see `window_host::render`).
        if self.mux_sidebar_visibility() != MuxSidebarVisibility::Overlay {
            return false;
        }
        if self.mux_sidebar_overlay_hovered {
            return false;
        }
        let hold_active = self
            .mux_sidebar_last_switch
            .is_some_and(|t| now < t + OVERLAY_BRIGHT_HOLD);
        if hold_active {
            return false;
        }
        match self.mux_sidebar_fade_started {
            // Only reachable via a prior `resolve_mux_sidebar_opacity`
            // call's bright-branch reset (construction seeds a past
            // instant, never `None` — see the field doc), so this is
            // always a genuine just-happened transition needing one more
            // render to (re-)arm the fade.
            None => true,
            Some(started) => {
                let elapsed = now
                    .checked_duration_since(started)
                    .unwrap_or(std::time::Duration::ZERO);
                elapsed < OVERLAY_DIM_FADE
            }
        }
    }

    #[allow(dead_code)] // retained for future mutation paths / tests
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active)
    }

    /// Dispatch a mux prefix action against the active tab. Switch / new /
    /// detach are handled here; rename / move open dialogs (Phase 4) and are
    /// surfaced via [`MuxActionOutcome`] for the caller to drive. Returns the
    /// outcome so the caller (window_host) can react (open a dialog, redraw).
    ///
    /// Port of `handleMuxAction` in `mux-action-handler.ts`, fused with the
    /// `switchMuxWindow` index math (the native build sends `SwitchWindow`
    /// and lets the daemon snapshot swap the screen, so there is no local
    /// grid save/restore here).
    pub fn dispatch_mux_action(
        &mut self,
        action: crate::mux::prefix::PrefixAction,
    ) -> MuxActionOutcome {
        use crate::mux::prefix::PrefixAction;
        use mux_ipc::protocol::{CreateWindowPayload, MessageType, MuxMessage};

        let Some(tab) = self.tabs.get_mut(self.active) else {
            return MuxActionOutcome::None;
        };
        if tab.mux_group.is_none() {
            return MuxActionOutcome::None;
        }

        // FR3: the switch arms swap the App's single active scroll value
        // through `switch_to`. Copy it into a local so the `tab` borrow above
        // does not conflict with `&mut self.scroll_position`; write the
        // (possibly updated) value back after the match and force a full
        // redraw on a committed switch (FR2) below.
        let mut scroll = self.scroll_position;
        // FR4: capture the active window index before any switch so the
        // scroll-into-view flag can be raised strictly on a real window change
        // (TS-9 option b: a same-window digit jump reports `Changed` but does
        // not move `active`, so it must not raise the flag).
        let active_before = tab.mux_group.as_ref().map(|g| g.active_index());
        // task0002 AC-6: set by the NextWindow / PrevWindow / SelectWindow
        // arms below on a COMMITTED switch only; consumed after the match
        // (once the `tab` borrow has ended) to record the switch timestamp.
        // A plain `bool` local — not a `self.record_mux_window_switch()`
        // call inline — because `tab` still borrows `self.tabs` here and a
        // method call needs the whole `&mut self`.
        let mut window_switch_committed = false;
        let outcome = match action {
            PrefixAction::None | PrefixAction::Literal => MuxActionOutcome::None,
            PrefixAction::Detach => {
                tab.send_control(&MuxMessage {
                    msg_type: MessageType::Detach,
                    pane_id: 0,
                    payload: Vec::new(),
                });
                MuxActionOutcome::Detach
            }
            PrefixAction::NewWindow => {
                tab.mux_group.as_mut().unwrap().inc_pending_create();
                let sent = tab.send_control(&MuxMessage::control(
                    MessageType::CreateWindow,
                    0,
                    &CreateWindowPayload::default(),
                ));
                if !sent {
                    // No PTY — undo the optimistic pending credit so a later
                    // stray PaneCreated cannot append a phantom window.
                    tab.mux_group.as_mut().unwrap().take_pending_create();
                    return MuxActionOutcome::None;
                }
                MuxActionOutcome::Changed
            }
            PrefixAction::NextWindow => {
                let target = tab.mux_group.as_ref().unwrap().next_index();
                let result = Self::switch_to(tab, target, &mut scroll);
                window_switch_committed = result == MuxActionOutcome::Changed;
                result
            }
            PrefixAction::PrevWindow => {
                let target = tab.mux_group.as_ref().unwrap().prev_index();
                let result = Self::switch_to(tab, target, &mut scroll);
                window_switch_committed = result == MuxActionOutcome::Changed;
                result
            }
            PrefixAction::SelectWindow(d) => {
                let target = tab.mux_group.as_ref().unwrap().digit_index(d);
                let result = Self::switch_to(tab, target, &mut scroll);
                window_switch_committed = result == MuxActionOutcome::Changed;
                result
            }
            PrefixAction::RenameWindow => {
                // Re-resolved by the dialog handler; surface the active
                // window's stable id so the dialog can re-find it on confirm.
                match tab.mux_group.as_ref().unwrap().active_window() {
                    Some(w) => MuxActionOutcome::OpenRename {
                        window_id: w.id,
                        current_name: w.name.clone(),
                    },
                    None => MuxActionOutcome::None,
                }
            }
            PrefixAction::MoveWindow => {
                let group = tab.mux_group.as_ref().unwrap();
                if group.len() <= 1 {
                    return MuxActionOutcome::None;
                }
                match group.active_window() {
                    Some(w) => MuxActionOutcome::OpenMove {
                        window_id: w.id,
                        current_position: group.active_index() + 1,
                        window_count: group.len(),
                    },
                    None => MuxActionOutcome::None,
                }
            }
            PrefixAction::ToggleWindowSidebar => {
                // FR4: persistent mode (the common default) makes the
                // keybind a strict no-op — no state change, no dialog, no
                // PTY interaction. FR5: overlay mode flips the runtime
                // flag; toggling never touches the PTY (NFR1), so this
                // always reports `None` rather than `Changed` (rendering
                // is task0005's concern, not this dispatch).
                if self.settings.mux.window_sidebar_overlay {
                    self.mux_sidebar_overlay_open = !self.mux_sidebar_overlay_open;
                }
                MuxActionOutcome::None
            }
            PrefixAction::NextAgentWindow => {
                // SPEC mux-agent-tab-cycle: resolve the cycle target from
                // in-memory state only (window order + agent-status model),
                // at key-event time — no polling, no cached qualify list
                // (NFR2). A window qualifies when at least one of its panes
                // currently carries a reported (uncleared) agent status
                // (FR6, existential).
                let group = tab.mux_group.as_ref().unwrap();
                let current = group.active_index();
                let qualifies: Vec<bool> = group
                    .pane_ids()
                    .iter()
                    .map(|pane_id| self.agent_status.any_pane_has_reported_state([pane_id]))
                    .collect();
                let target = crate::mux::window_group::next_qualifying_index(&qualifies, current);
                // When the active window is the sole qualifying window,
                // next_qualifying_index returns Some(current); treat that as
                // a no-op instead of round-tripping SwitchWindow + a
                // snapshot replay onto the pane that's already active.
                if target == Some(current) {
                    MuxActionOutcome::None
                } else {
                    let result = Self::switch_to(tab, target, &mut scroll);
                    window_switch_committed = result == MuxActionOutcome::Changed;
                    result
                }
            }
        };
        // The `tab` borrow has ended; commit the swapped scroll value and, on
        // a committed pane switch, force a full redraw so a shorter incoming
        // pane leaves no residual rows from the longer outgoing one (FR2).
        // task0002 AC-6: record the switch timestamp for the overlay dim
        // feature — deliberately NOT folded into the `outcome ==
        // MuxActionOutcome::Changed` block below, since that block also
        // fires for `NewWindow` (which the task plan excludes).
        if window_switch_committed {
            self.record_mux_window_switch();
        }
        if outcome == MuxActionOutcome::Changed {
            self.scroll_position = scroll;
            self.needs_full_redraw = true;
            // FR4: raise the scroll-into-view flag only when the active window
            // index actually moved (TS-9 option b). `NewWindow` reports
            // `Changed` but appends asynchronously (awaiting the daemon's
            // `PaneCreated`), so `active` is unchanged this frame and the flag
            // stays down; the new window's sub-tab is scrolled in when the
            // daemon confirms it and the user is on it. A same-window digit
            // jump (`set_active_clamped` lands on the same index) also leaves
            // `active` unchanged, so the flag is not raised.
            let active_after = self
                .tabs
                .get(self.active)
                .and_then(|t| t.mux_group.as_ref())
                .map(|g| g.active_index());
            if active_before != active_after {
                self.scroll_active_tab_into_view = true;
            }
        }
        outcome
    }

    /// React to a [`MuxActionOutcome`] from [`Self::observe_mux_key`]. Switch
    /// / new / detach already applied their effects in `dispatch_mux_action`;
    /// here we open the rename / move dialogs (Phase 4). `None` / `Changed` /
    /// `Detach` need no further action at this layer.
    pub fn handle_mux_outcome(&mut self, outcome: MuxActionOutcome) {
        match outcome {
            MuxActionOutcome::None | MuxActionOutcome::Changed | MuxActionOutcome::Detach => {}
            MuxActionOutcome::OpenRename {
                window_id,
                current_name,
            } => {
                self.open_mux_rename_dialog(window_id, current_name);
            }
            MuxActionOutcome::OpenMove {
                window_id,
                current_position,
                window_count,
            } => {
                self.open_mux_move_dialog(window_id, current_position, window_count);
            }
        }
    }

    /// Open the rename dialog for `window_id`, seeded with `current_name`.
    /// Reentry guard: a no-op if a rename dialog is already open (port of
    /// `renameDialogOpen`).
    pub fn open_mux_rename_dialog(&mut self, window_id: u32, current_name: String) {
        if self.mux_dialog.is_open() {
            return;
        }
        self.mux_dialog = crate::mux::dialog::MuxDialogState::Rename {
            window_id,
            name: current_name,
        };
    }

    /// Open the move dialog for `window_id`. Reentry guard via the
    /// `MuxDialogState::Closed` discriminant (port of `moveDialogOpen`).
    pub fn open_mux_move_dialog(
        &mut self,
        window_id: u32,
        current_position: usize,
        window_count: usize,
    ) {
        if self.mux_dialog.is_open() {
            return;
        }
        self.mux_dialog = crate::mux::dialog::MuxDialogState::Move {
            window_id,
            current_position,
            window_count,
            target: current_position,
        };
    }

    /// Whether a mux rename / move dialog is currently open (the caller
    /// routes input to the dialog and swallows the PTY while it is).
    pub fn mux_dialog_open(&self) -> bool {
        self.mux_dialog.is_open()
    }

    /// Reconcile the open mux dialog against the active tab's current window
    /// group. Called by the render pipeline (`window_host::drive_mux_dialogs`)
    /// every frame before draw so a daemon-driven change that arrived while
    /// the dialog was open (a `PaneCreated` widened the window list, a
    /// `PtyExited` removed the captured window, a `SwitchWindow` shifted the
    /// current position) is reflected — instead of the dialog still showing
    /// a stale `current_position` / `window_count` captured at open time, or
    /// the user confirming a target the group can no longer accept.
    ///
    /// Outcomes:
    /// - The captured `window_id` no longer exists in the group → close the
    ///   dialog (`Closed`); the user's pending edit is silently discarded
    ///   since the target window is gone (parity with WebView, which closes
    ///   the dialog on a server-broadcast detach of the targeted window).
    /// - For the move dialog: refresh `current_position` to the captured
    ///   window's new 1-based index and `window_count` to `group.len()`, and
    ///   clamp `target` into the new `1..=window_count` range so an
    ///   in-flight edit can never confirm a value the group cannot accept.
    /// - For the rename dialog: nothing to refresh (the only display field is
    ///   the user-edited buffer).
    pub fn refresh_mux_dialog(&mut self) {
        use crate::mux::dialog::MuxDialogState;
        if matches!(self.mux_dialog, MuxDialogState::Closed) {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else {
            self.mux_dialog = MuxDialogState::Closed;
            return;
        };
        let Some(group) = tab.mux_group.as_ref() else {
            self.mux_dialog = MuxDialogState::Closed;
            return;
        };
        let captured_id = match &self.mux_dialog {
            MuxDialogState::Closed => return,
            MuxDialogState::Rename { window_id, .. } | MuxDialogState::Move { window_id, .. } => {
                *window_id
            }
        };
        let Some(idx) = group.index_of_window_id(captured_id) else {
            self.mux_dialog = MuxDialogState::Closed;
            return;
        };
        if let MuxDialogState::Move {
            current_position,
            window_count,
            target,
            ..
        } = &mut self.mux_dialog
        {
            let new_count = group.len();
            let new_current = idx + 1;
            *current_position = new_current;
            *window_count = new_count;
            *target = (*target).clamp(1, new_count.max(1));
        }
    }

    /// Feed one key event into the mux prefix latch and act on the result.
    /// Only intercepts when the active tab is mux-attached (has a
    /// `mux_group`); otherwise returns `(false, None)` so the caller falls
    /// through to the normal keybind / PTY path.
    ///
    /// Returns `(consumed, outcome)`:
    /// - `consumed = true` means the key was absorbed by the mux layer and
    ///   must NOT reach the keybind dispatch or PTY passthrough (covers
    ///   arming, the action follow-up, the double-prefix literal, and the
    ///   unknown-after-prefix ignore — FR2).
    /// - `outcome` is the action result (for the caller to open a dialog /
    ///   redraw); `None` when nothing actionable happened.
    ///
    /// `now` is the wall-clock instant (production passes `Instant::now()`;
    /// tests pass a synthetic clock). Port of `PrefixKeyHandler.handleKeyEvent`
    /// fused with `handleMuxAction` dispatch.
    pub fn observe_mux_key(
        &mut self,
        input: &crate::mux::prefix::KeyInput,
        now: Instant,
    ) -> (bool, MuxActionOutcome) {
        use crate::mux::prefix::PrefixAction;
        // Only a mux-attached active tab gets the prefix intercept.
        let is_mux = self
            .tabs
            .get(self.active)
            .map(|t| t.mux_group.is_some())
            .unwrap_or(false);
        if !is_mux {
            return (false, MuxActionOutcome::None);
        }
        let was_armed = self.mux_latch.is_armed();
        let action = self.mux_latch.observe(input, now);
        match action {
            PrefixAction::None => {
                // Two cases: (1) this key just armed the latch → consume it;
                // (2) it was an unknown follow-up after the prefix → consume
                // it (unknown-after-prefix is ignored, FR2). Otherwise the
                // key is unrelated and must fall through.
                if self.mux_latch.is_armed() || was_armed {
                    (true, MuxActionOutcome::None)
                } else {
                    (false, MuxActionOutcome::None)
                }
            }
            PrefixAction::Literal => {
                // Double-prefix: send the *configured* prefix's literal byte to
                // the active pane so programs that themselves use the chord
                // still receive it. Pre-fix this was hardcoded to
                // `DEFAULT_LITERAL_BYTE` (0x1A = Ctrl+Z), so a user with
                // `Ctrl+A` set would double-tap Ctrl+A and the pane would see
                // Ctrl+Z instead — silently breaking a nested tmux. Derive the
                // byte from the live chord via `Latch::literal_byte`.
                let byte = self.mux_latch.literal_byte();
                if let Some(tab) = self.tabs.get(self.active) {
                    tab.write_input(vec![byte]);
                }
                (true, MuxActionOutcome::None)
            }
            other => {
                let outcome = self.dispatch_mux_action(other);
                (true, outcome)
            }
        }
    }

    /// Apply a local switch to `target` (if any) and notify the daemon. The
    /// daemon-pushed snapshot swaps the on-screen content; here we only move
    /// the active index and send `SwitchWindow` with the new pane id. A
    /// `None` target (single window / out-of-range digit) is a no-op.
    ///
    /// Send-first / commit-after: when the PTY write fails (no PTY, broken
    /// pipe) the local active index is left untouched, mirroring the
    /// `NewWindow` rollback path. Pre-fix this committed the local switch
    /// unconditionally, leaving the UI and the pane-filter (`PtyOutput`
    /// drops bytes for non-active panes) pointing at a window the daemon
    /// never moved off of.
    ///
    /// `scroll` is the App's single active scroll value (`App::scroll_position`).
    /// On a committed switch this saves the outgoing pane's position into its
    /// per-pane slot, then reloads the incoming pane's saved position into
    /// `scroll` after the snapshot request (FR3 pane wiring). A failed send or
    /// empty group leaves `scroll` untouched.
    pub(super) fn switch_to(
        tab: &mut Tab,
        target: Option<usize>,
        scroll: &mut crate::app::ScrollPosition,
    ) -> MuxActionOutcome {
        use mux_ipc::protocol::{MessageType, MuxMessage};
        let Some(idx) = target else {
            return MuxActionOutcome::None;
        };
        // Peek the target pane via shared borrow so a failed send does not
        // leave `group.active` already mutated.
        let pane_id = {
            let Some(group) = tab.mux_group.as_ref() else {
                return MuxActionOutcome::None;
            };
            let panes = group.pane_ids();
            if panes.is_empty() {
                return MuxActionOutcome::None;
            }
            let clamped = idx.min(panes.len() - 1);
            panes[clamped]
        };
        let sent = tab.send_control(&MuxMessage {
            msg_type: MessageType::SwitchWindow,
            pane_id,
            payload: Vec::new(),
        });
        if !sent {
            return MuxActionOutcome::None;
        }
        // Daemon accepted; now commit local state and pull the new window's
        // screen on demand (the daemon does not push the active grid
        // unprompted — parity with `switchMuxWindow`'s `requestPaneSnapshot`).
        if let Some(group) = tab.mux_group.as_mut() {
            // FR3: park the outgoing pane's live scroll position before the
            // active index moves, then reload the incoming pane's saved
            // position after committing the switch + requesting its snapshot,
            // so the restore lands together with the replayed content.
            group.set_active_pane_scroll(*scroll);
            group.set_active_clamped(idx);
        }
        tab.request_pane_snapshot(pane_id);
        if let Some(group) = tab.mux_group.as_ref() {
            *scroll = group.active_pane_scroll();
        }
        MuxActionOutcome::Changed
    }

    /// Confirm an optimistic rename: relabel the window with `window_id` (if
    /// it still exists) and notify the daemon with the active pane id. An
    /// empty name is a no-op (matching the WebView). Returns true when a
    /// rename was applied.
    pub fn confirm_mux_rename(&mut self, window_id: u32, name: String) -> bool {
        use mux_ipc::protocol::{MessageType, MuxMessage, RenameWindowMsg};
        // Trim then reject empty. The originating dialog already returns
        // `trimmed.to_string()`, but this is a `pub` entry point — a future
        // caller (CLI rebind, test, remote-confirm) could pass `"   "` and
        // the literal-empty guard alone would let a whitespace-only name
        // reach the daemon and the local `MuxWindow.name`. Make the contract
        // explicit.
        let name = name.trim().to_string();
        if name.is_empty() {
            return false;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        let Some(group) = tab.mux_group.as_mut() else {
            return false;
        };
        let Some(idx) = group.index_of_window_id(window_id) else {
            return false; // window closed during the dialog
        };
        let pane_id = group.pane_ids()[idx];
        group.rename_window_id(window_id, name.clone());
        tab.send_control(&MuxMessage::control(
            MessageType::RenameWindow,
            pane_id,
            &RenameWindowMsg { name },
        ));
        true
    }

    /// Confirm an optimistic move: reorder the window with stable `window_id`
    /// to 1-based `target_position`, notify the daemon, and roll back the
    /// reorder if the send fails (the daemon does not broadcast order, so
    /// local state is authoritative). Returns true when a move was applied
    /// and survived (no rollback). Port of the `move-window` handler.
    pub fn confirm_mux_move(&mut self, window_id: u32, target_position: usize) -> bool {
        use mux_ipc::protocol::{MessageType, MoveWindowMsg, MuxMessage};
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return false;
        };
        let Some(group) = tab.mux_group.as_mut() else {
            return false;
        };
        let Some(from) = group.index_of_window_id(window_id) else {
            return false; // window closed during the dialog
        };
        let count = group.len();
        if target_position < 1 || target_position > count {
            return false; // out of range
        }
        let to = target_position - 1;
        if to == from {
            return false; // same position — no-op
        }
        let pane_id = group.pane_ids()[from];
        // Optimistic reorder so the UI updates immediately.
        group.reorder(from, to);
        let sent = tab.send_control(&MuxMessage::control(
            MessageType::MoveWindow,
            pane_id,
            &MoveWindowMsg {
                target_index: to as u32,
            },
        ));
        if !sent {
            // Roll back: re-insert from `to` back to `from`.
            if let Some(g) = tab.mux_group.as_mut() {
                g.reorder(to, from);
            }
            log::warn!("mux: MoveWindow send failed, reverted optimistic reorder");
            return false;
        }
        true
    }

    /// Drain PTY events on every tab. Returns true if any tab produced
    /// new bytes (caller schedules a redraw).
    pub fn pump_all(&mut self) -> bool {
        let mut changed = false;
        // Whether the *active* tab specifically produced new bytes this
        // pump. Only the active tab's output mutates the buffer the search
        // overlay is resolved against, so background-tab output must not
        // invalidate the active tab's cached logical-line document (H3).
        let mut active_changed = false;
        let mut bell_rang = false;
        let now = Instant::now();
        let active = self.active;
        // Scroll-stick: sample the active tab's scrollback length now and
        // again after the per-tab loop. The saturating difference is fed
        // to `on_pty_output` so an `OffsetFromLive(n)` view advances by Δ
        // and the visible row composition stays anchored while below the
        // configured `scrollback_lines` capacity.
        let before_scrollback_len = self
            .tabs
            .get(active)
            .map(|t| t.core.lock().get_scrollback_length())
            .unwrap_or(0);
        // Desktop notifications collected during the tab loop and
        // dispatched after it — `tab` holds `&mut self.tabs`, so
        // `self.notify()` (a `&self` call) can't run inside the loop.
        let mut pending_notifications: Vec<(String, crate::notifications::ActivityKind)> =
            Vec::new();
        // Absolute-row selection bookkeeping, captured from the active tab in
        // the loop and applied after it (the loop holds `&mut self.tabs`, so
        // mutating `self.selection` must wait until the borrow ends).
        let mut active_eviction_delta: u32 = 0;
        let mut active_frame_reset = false;
        // Outgoing pane index latched by a daemon-initiated SwitchWindow on
        // the active tab this pump (FR3 pane wiring); applied after the
        // `&mut self.tabs` borrow ends.
        let mut active_pane_switch_from: Option<u32> = None;
        // Whether the *active* tab's off-thread snapshot replay completed and
        // swapped in this pump. Drives the active-tab post-loop full redraw so
        // a shorter incoming pane leaves no residual rows (FR2) once the
        // worker-built core is shown. Selection drop is handled by the
        // `pending_frame_reset` latch (drained into `active_frame_reset`),
        // which `apply_offthread_swap` set during the swap.
        let mut active_offthread_swapped = false;
        // Whether the *active* tab's 2nd-pass scrollback-restore merged this
        // pump. `merge_scrollback_from` prepends historical rows to the front
        // of `scrollback_slim`, so `get_scrollback_length()` grows by
        // `merged_rows` — but those rows are older than the user's parked
        // `OffsetFromLive(n)` view of live, so the visible content must NOT
        // shift. Gates the scroll-stick Δ to 0 (the merge is unrelated to
        // live PTY output).
        let mut active_scrollback_restore_merged = false;
        // FR6 (mux): a new mux window appended to the ACTIVE tab this pump means
        // a fresh sub-tab became active (off-screen when the strip overflows),
        // so it should scroll into view next frame — the async mux analogue of
        // the `+`-button new-tab path. Sourced from each tab's one-shot
        // `take_pending_window_appended` latch (set at the `PaneCreated` push
        // site); applied for the active tab after the `&mut self.tabs` borrow ends.
        let mut active_mux_window_added = false;
        // emterm viewer OSC payloads collected across all tabs this pass,
        // routed to the spawner after the `&mut self.tabs` borrow ends.
        let mut viewer_osc: Vec<crate::callbacks::EmtermOscRequest> = Vec::new();
        // Decoded Kitty/SIXEL image events (ImageReady / Place / Delete),
        // likewise buffered during the loop and routed to the image-viewer
        // router after it. Every tab is drained — like the WebView build,
        // any tab's `emterm image` opens a viewer window.
        let mut image_events: Vec<term_images::image_proc::ImageEvent> = Vec::new();
        // Agent-status inputs collected across every tab this pass (task0005),
        // applied to `self.agent_status` after the `&mut self.tabs` borrow
        // ends: plain-tab OSC events (tagged with the originating tab's
        // `stable_id`), daemon-pushed `AgentStatusUpdate` messages, and mux
        // pane ids a `PtyExited` arm removed this pump.
        let mut agent_status_plain_events: Vec<(u64, crate::agent_status::AgentStatusEvent)> =
            Vec::new();
        let mut agent_status_updates: Vec<mux_ipc::protocol::AgentStatusUpdateMsg> = Vec::new();
        let mut agent_status_closed_panes: Vec<u32> = Vec::new();
        // agent-exit-after-icon (task0002 deviation): each tab's resolved,
        // true-order, live-only inferred-clear latch inputs this pump
        // (tagged with the originating tab's `stable_id`, mirroring
        // `agent_status_plain_events`). See `Tab::pending_latch_inputs`'s
        // doc.
        let mut agent_status_latch_inputs: Vec<(
            u64,
            crate::agent_status_model::ResolvedLatchInput,
        )> = Vec::new();
        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            // Phase 4-C (APC redesign): `Tab::pump` already routes
            // APC-encoded mux messages into the tab's own state via
            // `apply_mux_message` (see `crate::mux::apc`). There is no
            // separate `pump_mux` pass — the bridge CLI runs inside the
            // same PTY, so a single drain is sufficient.
            let was_exited = tab.exited;
            if tab.pump() {
                changed = true;
                if idx == active {
                    active_changed = true;
                }
            }
            // task0005: drain this tab's agent-status latches (plain-tab OSC
            // events, daemon `AgentStatusUpdate` pushes, closed mux pane
            // ids) every pump, not just the active tab — a background tab's
            // agent can report status too.
            let tab_stable_id = tab.stable_id;
            for event in tab.take_pending_agent_status_events() {
                agent_status_plain_events.push((tab_stable_id, event));
            }
            for input in tab.take_pending_latch_inputs() {
                agent_status_latch_inputs.push((tab_stable_id, input));
            }
            agent_status_updates.extend(tab.take_pending_agent_status_updates());
            agent_status_closed_panes.extend(tab.take_closed_agent_status_panes());
            // FR6 (mux): drain every tab's one-shot "window appended this pump"
            // latch (set at the `PaneCreated` push site) to avoid stale
            // carry-over; act on it only for the active tab, where the freshly
            // pushed window is now the active sub-tab and should scroll into view.
            if tab.take_pending_window_appended() && idx == active {
                active_mux_window_added = true;
            }
            // Non-blockingly poll this tab's in-flight off-thread snapshot
            // replay (the mux off-thread switch). Run per owning tab, not just
            // the active one, so a background tab's swap applies as well (its
            // selection/scroll are reconciled when it later becomes active,
            // parity with the existing background-tab bookkeeping). On a swap,
            // the displayed content changed, so mark `changed`; the active tab
            // additionally drives the post-loop full redraw + scroll restore.
            match tab.poll_pending_switch() {
                crate::tabs::SwapOutcome::Swapped => {
                    changed = true;
                    if idx == active {
                        active_changed = true;
                        active_offthread_swapped = true;
                    }
                }
                crate::tabs::SwapOutcome::Pending | crate::tabs::SwapOutcome::Idle => {}
            }
            // Non-blockingly poll this tab's in-flight 2nd-pass scrollback
            // restore (the bypass-off rebuild merged into the live core).
            // Run per owning tab so a background tab's restore lands as
            // well — the merge does not touch the viewport, so no
            // `active_offthread_swapped`-style full redraw is required;
            // only `active_changed` matters so the search overlay rebuilds
            // against the new scrollback.
            match tab.poll_pending_scrollback_restore() {
                crate::tabs::ScrollbackRestoreOutcome::Merged => {
                    changed = true;
                    if idx == active {
                        active_changed = true;
                        active_scrollback_restore_merged = true;
                    }
                }
                crate::tabs::ScrollbackRestoreOutcome::Failed => {
                    changed = true;
                    if idx == active {
                        active_changed = true;
                    }
                }
                crate::tabs::ScrollbackRestoreOutcome::Pending
                | crate::tabs::ScrollbackRestoreOutcome::Idle => {}
            }
            // Markdown viewer (FR1/FR2/FR3): collect this tab's emterm OSC
            // queue for the spawner. We cannot touch `self.viewer_spawner`
            // here because the loop holds `&mut self.tabs`, so the drained
            // payloads are buffered and routed after the loop. Draining
            // every tab (not just the active one) matches the WebView
            // build, where any tab's `emterm markdown` opens a viewer.
            let mut osc = tab.drain_osc();
            if !osc.is_empty() {
                // Release the parked `emterm markdown` CLI as soon as its
                // session end marker passes by. The release is gated on the
                // CLI's `interactive=1` flag (set only when its stdin was a
                // TTY, i.e. it is actually parked in the navigate/image/quit
                // stdin loop), which suppresses the common *accidental* case:
                // a non-interactive (piped/redirected) `emterm markdown`
                // omits the flag, so we don't inject `quit` after it has
                // already returned the prompt. The native viewer child
                // resolves images and links itself, so releasing here returns
                // the shell prompt immediately instead of holding it until the
                // viewer window closes. `.any()` caps this at one quit per tab
                // per drain — only one interactive markdown CLI can be parked
                // per PTY.
                //
                // SECURITY (accepted residual): the flag is plaintext in the
                // terminal output stream, which is attacker-controllable, so
                // untrusted output (a `cat`'d file, an SSH peer, a log line)
                // CAN forge `markdown;end;…;interactive=1` and make us write
                // `quit\n` into this tab's foreground program. The blast
                // radius is bounded to that single line (not arbitrary input);
                // accepted as documented in SPEC.md ("Interactive CLI release").
                if osc
                    .iter()
                    .any(|r| crate::viewer::markdown_end_wants_release(&r.payload))
                {
                    tab.write(crate::viewer::MARKDOWN_RELEASE_INPUT.to_vec());
                }
                viewer_osc.append(&mut osc);
            }
            // Image viewer: collect this tab's decoded image events. Must
            // run before the `idx == active` early-continue below so the
            // active tab's images open a viewer too.
            image_events.extend(tab.drain_image_events());
            // Any tab's BEL triggers the bell action — same as the
            // WebView build, where a background tab's BEL still flashes
            // the shared terminal container / beeps.
            let bell = tab.take_bell();
            if bell {
                bell_rang = true;
            }
            let output = tab.take_output();
            // Drain the eviction bookkeeping for every tab so stale deltas do
            // not accumulate on a background tab and then mis-shift the
            // selection the moment it becomes active. Only the active tab's
            // values feed the selection (it owns the buffer the selection
            // addresses).
            let eviction_delta = tab.take_eviction_delta();
            let frame_reset = tab.take_frame_reset();
            // Drain the inbound-pane-switch latch for every tab so a stale
            // transition recorded on a background tab cannot mis-apply when it
            // later becomes active; only the active tab's value is consumed.
            let pane_switch_from = tab.take_pending_pane_switch();
            if idx == active {
                active_eviction_delta = eviction_delta;
                active_frame_reset = frame_reset;
                active_pane_switch_from = pane_switch_from;
            } else if let Some(from_pane) = pane_switch_from {
                // Background tab: a daemon-initiated pane switch (or new-window
                // create) moved this tab's active pane while it was off screen.
                // Its live scroll lives in `Tab::scroll_position` (only the
                // active tab's lives in `App::scroll_position`), so park that
                // into the outgoing pane's slot and reload the now-active pane's
                // saved slot. Without this, a later `switch_to_tab` would
                // restore the OLD pane's scroll against the NEW active pane
                // (FR3 / NFR3 divergence). No full redraw — the tab is hidden.
                // The latch holds the outgoing pane *id*; resolve it to a
                // current index (a same-pump `PtyExited` may have shifted the
                // arrays) and skip the park if the pane has since exited.
                let saved = tab.scroll_position;
                let reloaded = tab.mux_group.as_mut().map(|group| {
                    if let Some(from_idx) = group.index_of_pane_id(from_pane) {
                        group.set_pane_scroll_at(from_idx, saved);
                    }
                    group.active_pane_scroll()
                });
                if let Some(scroll) = reloaded {
                    tab.scroll_position = scroll;
                }
            }
            let exited_now = !was_exited && tab.exited;

            // ── Inactive-tab activity (dot + desktop notification) ──
            // WebView parity: `TabActivityTracker.markActivity` ignores
            // the active tab entirely; clearing the active tab's dot
            // every frame covers all switch paths (click, keybind,
            // reorder, exited-tab reap) without per-path hooks.
            if idx == active {
                if tab.activity.has_activity {
                    tab.activity.clear();
                    changed = true;
                }
                continue;
            }
            for (fired, kind) in [
                (exited_now, crate::notifications::ActivityKind::ProcessExit),
                (output, crate::notifications::ActivityKind::Output),
                (bell, crate::notifications::ActivityKind::Bell),
            ] {
                if !fired || !crate::notifications::kind_enabled(&self.settings, kind) {
                    continue;
                }
                // `mark` owns the 1 s output throttle; a swallowed mark
                // produces neither a dot nor a notification.
                if !tab.activity.mark(kind, now) {
                    continue;
                }
                // Desktop notification gates, in WebView order:
                // master switch → window focus → per-tab 5 s throttle
                // (ProcessExit bypasses the check but re-arms it).
                if self.settings.notification_enabled
                    && !self.window_focused
                    && tab.activity.should_notify(kind, now)
                {
                    // Sanitize at capture time: carries ≤ 100 chars
                    // instead of cloning a potentially huge OSC title.
                    pending_notifications
                        .push((crate::notifications::sanitize_title(&tab.title), kind));
                }
            }
        }
        // Route this pass' collected emterm viewer OSC payloads now that the
        // `&mut self.tabs` borrow has ended. The spawner reassembles
        // markdown sessions and emits completed documents to `viewer_sink`,
        // which spawns a separate `--viewer` child process per document.
        // Always call so the spawner's idle-session timeout sweep runs even
        // on empty passes; cheap when there is nothing queued.
        self.viewer_spawner
            .drain(viewer_osc, now, &mut self.viewer_sink);
        // Route decoded image events: ImageReady fills the LRU store,
        // each Place spawns a native `--image-viewer` child window.
        if !image_events.is_empty() {
            self.image_viewer.handle_events(image_events);
        }
        // Apply this pass' collected agent-status inputs now that the
        // `&mut self.tabs` borrow has ended (task0005). FR4 requires a tab's
        // real-status events and its latch inputs to be applied as ONE
        // ordered stream: `reconcile_latch_feed` derives both lists from the
        // same `pending_latch_feed` in order, so within a single tab each
        // `Set`/`Clear` latch input corresponds 1:1 and same-order with one
        // `AgentStatusEvent`. Walking the latch inputs and pulling that tab's
        // next unconsumed plain event per `Set`/`Clear` reconstructs the true
        // order; a bare `Mark` consumes no plain event (it only ever applies
        // a real change indirectly, via `record_live_prompt_mark`'s own
        // inferred `apply_plain_tab_event(Clear)` when the latch fires).
        //
        // The pairing MUST be per tab, never positional across the flattened
        // lists: the two lists are filled by the same tab loop but a tab can
        // contribute to one and not the other. A mux-connected tab still
        // pushes real-status events while `process_combined` discards its
        // latch feed (that tab's pane status is daemon-authoritative), so it
        // contributes plain events and zero latch inputs. Consuming
        // positionally would let that tab's events satisfy ANOTHER tab's
        // `Set`/`Clear` and push the real events after the latch bookkeeping
        // they belong with — reintroducing the very reordering this loop
        // exists to prevent. Events with no matching latch input are applied
        // afterwards, in their original order.
        let mut agent_status_plain_events: Vec<
            Option<(u64, crate::agent_status::AgentStatusEvent)>,
        > = agent_status_plain_events.into_iter().map(Some).collect();
        for (tab_stable_id, input) in agent_status_latch_inputs {
            match input {
                crate::agent_status_model::ResolvedLatchInput::Set
                | crate::agent_status_model::ResolvedLatchInput::Clear => {
                    let paired = agent_status_plain_events
                        .iter_mut()
                        .find(|slot| matches!(slot, Some((id, _)) if *id == tab_stable_id))
                        .and_then(Option::take);
                    if let Some((_, event)) = paired {
                        self.agent_status
                            .apply_plain_tab_event(tab_stable_id, event);
                    }
                    if matches!(input, crate::agent_status_model::ResolvedLatchInput::Set) {
                        self.agent_status.record_latch_set(tab_stable_id);
                    } else {
                        self.agent_status.record_latch_clear(tab_stable_id);
                    }
                }
                crate::agent_status_model::ResolvedLatchInput::Mark(kind) => {
                    self.agent_status
                        .record_live_prompt_mark(tab_stable_id, kind);
                }
            }
        }
        for (tab_stable_id, event) in agent_status_plain_events.into_iter().flatten() {
            self.agent_status
                .apply_plain_tab_event(tab_stable_id, event);
        }
        for update in agent_status_updates {
            // task0006 AC-5: learn/refresh this pane's public ID from the
            // same message before applying it to the model — the daemon is
            // the only source for it (see `Self::mux_public_pane_ids`).
            self.mux_public_pane_ids
                .insert(update.pane_id, update.public_pane_id.clone());
            self.agent_status.apply_daemon_update(
                update.pane_id,
                update.state.map(crate::agent_status_model::state_from_wire),
                update.name,
                update.revision,
                update.replay_derived,
            );
        }
        for pane_id in agent_status_closed_panes {
            // task0009 AC-4: resolve the rate-limit key from the still-
            // present public-id mapping BEFORE removing it below.
            let key = crate::agent_status_model::PaneKey::MuxPane(pane_id);
            let rate_limit_key = agent_notification_rate_limit_key(&self.mux_public_pane_ids, &key);
            self.mux_public_pane_ids.remove(&pane_id);
            self.discard_agent_notification_state(&rate_limit_key);
            self.agent_status.discard(&key);
        }
        // task0009: drain queued real-transition events (task0005's
        // `AgentStatusModel::drain_transitions`) and dispatch qualifying
        // ones to the notification layer. Runs unconditionally — even
        // while `settings.agent_status_notifications` is off — so the
        // transition queue never grows unbounded while the setting is
        // toggled off (NFR3); the settings gate lives inside
        // `maybe_notify_agent_transition`. Must run BEFORE mark_seen below:
        // mark_seen would otherwise flip a freshly-arrived transition's
        // pane to "seen" before its own visibility is evaluated here (the
        // two operate on independent flags today, but ordering keeps the
        // gating and mark_seen concerns from becoming coupled).
        for transition in self.agent_status.drain_transitions() {
            let crate::agent_status_model::Transition {
                pane,
                old_state,
                new_state,
                name,
            } = transition;
            // AC-2: Clear transitions (new_state: None) are never
            // notification-eligible — only Set into blocked/done qualifies.
            let Some(new_state) = new_state else {
                continue;
            };
            let pane_visible =
                agent_status_pane_visible(self.window_focused, self.tabs.get(self.active), &pane);
            let rate_limit_key =
                agent_notification_rate_limit_key(&self.mux_public_pane_ids, &pane);
            let tab_title = agent_status_pane_tab_title(&self.tabs, &pane)
                .unwrap_or_default()
                .to_string();
            let agent_transition = crate::notifications::AgentTransition {
                old_state: old_state.map(crate::agent_status_model::state_to_wire),
                new_state: crate::agent_status_model::state_to_wire(new_state),
                name,
            };
            self.maybe_notify_agent_transition(
                rate_limit_key,
                pane_visible,
                &agent_transition,
                &tab_title,
            );
        }
        // mark_seen (task0005 AC-5): the active tab's panes are "displayed"
        // whenever the OS window is focused, regardless of whether this
        // pump produced any other change — the user could simply be looking
        // at an already-idle screen. Re-running every pump is intentionally
        // idempotent (`mark_seen` on an already-seen entry is a no-op).
        if self.window_focused {
            if let Some(active_tab) = self.tabs.get(self.active) {
                let panes = agent_status_keys_for_tab(active_tab);
                self.agent_status.mark_seen(panes.iter());
            }
        }
        // Apply the active tab's absolute-row selection bookkeeping now that
        // the `&mut self.tabs` borrow has ended. A frame reset drops the
        // selection outright (its rows belong to the discarded frame); an
        // eviction shifts it down by the number of evicted rows and drops it
        // when the whole range scrolled off the top. The `previous_selection`
        // is intentionally *not* shifted — it is interpreted against the
        // matching `previous_visible_start` captured in the same prior frame.
        // Inbound (daemon-initiated) pane switch on the active tab: park the
        // outgoing pane's scroll position, reload the now-active pane's saved
        // position into the single active value, and force a full redraw so a
        // shorter incoming pane leaves no residual rows (FR2 + FR3 pane wiring).
        // Mirrors the local switch path, but the active index has already moved
        // inside `apply_mux_message`. The latch holds the outgoing pane *id*;
        // resolve it to a current index (a same-pump `PtyExited` may have
        // shifted the arrays) and skip the park if the pane has since exited.
        if let Some(from_pane) = active_pane_switch_from {
            let restored = self.tabs.get_mut(self.active).and_then(|tab| {
                let group = tab.mux_group.as_mut()?;
                if let Some(from_idx) = group.index_of_pane_id(from_pane) {
                    group.set_pane_scroll_at(from_idx, self.scroll_position);
                }
                Some(group.active_pane_scroll())
            });
            if let Some(scroll) = restored {
                self.scroll_position = scroll;
                self.needs_full_redraw = true;
                changed = true;
            }
        }
        // The active tab's off-thread snapshot replay completed and swapped in
        // this pump: the displayed core changed from the outgoing pane to the
        // (possibly shorter) target pane, so force a full redraw to clear any
        // residual rows (FR2). The per-pane scroll position was already
        // restored at `switch_to` time (the active index moved synchronously
        // there); the swap only changes the rendered content, so no scroll
        // reload is needed here. Selection drop is handled by the
        // `active_frame_reset` latch below.
        if active_offthread_swapped {
            self.needs_full_redraw = true;
            changed = true;
        }
        if active_frame_reset {
            self.selection = None;
            self.pending_selection_anchor = None;
        } else if active_eviction_delta > 0 {
            if let Some(sel) = self.selection.as_mut() {
                if !sel.shift_rows_down(active_eviction_delta) {
                    self.selection = None;
                }
            }
            // Shift the pending press anchor in lockstep with the selection.
            // An anchor whose row scrolled off the top is dropped, since the
            // first drag motion would otherwise anchor against a stale row.
            if let Some(anchor) = self.pending_selection_anchor {
                match anchor.row.checked_sub(active_eviction_delta) {
                    Some(r) => {
                        self.pending_selection_anchor =
                            Some(crate::selection::Pos { row: r, ..anchor })
                    }
                    None => self.pending_selection_anchor = None,
                }
            }
        }
        // FR6 (mux): a new mux window was appended to the active tab this pump,
        // so its freshly-active sub-tab should scroll into view next frame (the
        // async mux analogue of the `+`-button new-tab path). Applied here,
        // after the `&mut self.tabs` borrow has ended.
        if active_mux_window_added {
            self.scroll_active_tab_into_view = true;
        }
        // Reset the mux sidebar overlay flag when the FOCUSED tab's mux
        // group tore down this pump (a `Detached` reply, or the last
        // window's `PtyExited` emptying the group — both routed into
        // `tab.mux_group` by `Tab::pump` above, before this read). Compared
        // against `self.active` before the exited-tab reap below (which can
        // renumber it, mirroring the before/after-scrollback sampling
        // rationale above the tab loop): the reap only runs when a tab is
        // fully removed, which already implies its mux group (if any) went
        // to `None` earlier in this same pass, so reading here still
        // targets the right tab. A changed `self.active` between pumps
        // (the user just switched tabs) intentionally does NOT reset the
        // flag — see the field doc on `active_mux_attached_prev_pump`.
        let active_mux_attached_now = self
            .tabs
            .get(self.active)
            .is_some_and(|t| t.mux_group.is_some());
        if self.active_mux_attached_prev_pump == Some(self.active) && !active_mux_attached_now {
            self.mux_sidebar_overlay_open = false;
        }
        // task0001 FR1/FR2/FR3: open the overlay flag on the
        // not-attached -> attached transition (bookkeeping field held no
        // value at the end of the previous pump, AND the active tab is
        // mux-attached now). Restores the AC-7 "default open" guarantee at
        // startup and on reattach, since the runtime flag only starts open
        // at construction — a later attach (the common case: the mux
        // daemon connection completes asynchronously after `App::new`)
        // would otherwise leave a stale `false` from an earlier detach.
        // Unconditional, no gate on `window_sidebar_overlay` (mirrors the
        // detach guard above): in persistent mode the flag drives no
        // rendering, so the assignment is inert there. Read BEFORE the
        // bookkeeping reassignment below, mirroring the detach guard.
        // Reattach after an explicit close re-opens the sidebar (FR3,
        // accepted) and switching the active tab onto an already-attached
        // mux tab also satisfies this transition and reopens the sidebar —
        // both accepted per IMPLEMENTATION.md D2 (the single-slot
        // bookkeeping cannot distinguish the two cases).
        if self.active_mux_attached_prev_pump.is_none() && active_mux_attached_now {
            self.mux_sidebar_overlay_open = true;
        }
        self.active_mux_attached_prev_pump = active_mux_attached_now.then_some(self.active);
        for (sanitized_title, kind) in pending_notifications {
            let body = crate::notifications::notification_body(&sanitized_title, kind, self.locale);
            self.notify(crate::notifications::NOTIFICATION_TITLE, &body);
        }
        if bell_rang {
            match self.settings.bell_action {
                crate::settings::BellAction::Visual => {
                    self.visual_bell_started = Some(Instant::now());
                    changed = true;
                }
                crate::settings::BellAction::Sound => crate::bell::play_beep(),
                crate::settings::BellAction::None => {}
            }
        }
        // Mirror the active tab's alt-screen state onto the App so the
        // scroll input routes can suppress wheel / Shift+Page during
        // alt-screen sessions. Read the core's authoritative
        // `MODE_ALT_SCREEN` bit (set at parse time on every 1049/47/1047
        // toggle) rather than the toggle-tracked `Tab::alt_screen`: a single
        // buffer-switch action lost across a reattach / off-thread replay /
        // snapshot gap would otherwise strand `Tab::alt_screen` true and kill
        // scrolling permanently even after the app returned to the main
        // buffer. The core's mode bit cannot desync from the displayed buffer.
        let active_alt = self.tabs.get(self.active).map(|t| {
            t.core
                .lock()
                .get_mode(term_core::terminal_core::MODE_ALT_SCREEN)
        });
        if let Some(active_alt) = active_alt {
            self.set_alt_screen(active_alt);
        }
        // Notify scroll-position state machine that new bytes arrived so
        // the auto-follow rule can preserve the off-tail offset. Pass
        // whether the *active* tab changed so only its output invalidates
        // the search overlay's cached document (H3). Sample the active
        // tab's scrollback length now (post-pump) so the saturating
        // difference from `before_scrollback_len` is the per-pump Δ that
        // anchors the visible content while parked at an `OffsetFromLive`
        // view. Reading the same `self.active` index as the before sample
        // is intentional; a same-pump tab reap shifts `self.active` later
        // (below), so this read still targets the tab that produced the
        // bytes.
        if changed {
            let after_scrollback_len = self
                .tabs
                .get(self.active)
                .map(|t| t.core.lock().get_scrollback_length())
                .unwrap_or(before_scrollback_len);
            // Pass Δ=0 whenever the active tab's scrollback length grew for a
            // reason other than live PTY output, so on_pty_output does not
            // corrupt the parked OffsetFromLive(n):
            //
            // * `active_pane_switch_from`: the active pane changed identity
            //   between the before_scrollback_len sample and now, so the
            //   saturating difference compares two distinct panes' scrollback
            //   lengths. The incoming pane's restored scroll_position is the
            //   authoritative state.
            // * `active_offthread_swapped`: `apply_offthread_swap` replaced
            //   `*self.core.lock()` wholesale; after_scrollback_len now reads
            //   a freshly-built snapshot core whose length is unrelated to
            //   before_scrollback_len. The per-pane scroll position was
            //   restored synchronously at switch_to time; the swap only
            //   changes the rendered content.
            // * `active_scrollback_restore_merged`: the 2nd-pass restore
            //   prepended historical rows to scrollback. get_scrollback_length
            //   grew by merged_rows, but those rows are older than the parked
            //   view — the visible content must not shift.
            let scrollback_delta = if active_pane_switch_from.is_some()
                || active_offthread_swapped
                || active_scrollback_restore_merged
            {
                0
            } else {
                after_scrollback_len.saturating_sub(before_scrollback_len)
            };
            self.on_pty_output(active_changed, scrollback_delta);
        }
        // Reap exited tabs (Phase 5 will refine the policy).
        let before = self.tabs.len();
        // task0005 AC-6 / task0009 AC-4: discard every reaped tab's
        // agent-status entries AND notification rate-limit bookkeeping
        // before removal (a mux tab whose last pane exited reaches this
        // path via `exited = true` rather than `close_tab`). Collected
        // into an owned `Vec` first so the immutable `self.tabs` borrow
        // (held by the iterator/filter) ends before the mutable
        // `self.discard_agent_notification_state` calls below.
        let reaped_agent_status_keys: Vec<crate::agent_status_model::PaneKey> = self
            .tabs
            .iter()
            .filter(|t| t.exited)
            .flat_map(agent_status_keys_for_tab)
            .collect();
        for key in reaped_agent_status_keys {
            let rate_limit_key = agent_notification_rate_limit_key(&self.mux_public_pane_ids, &key);
            self.discard_agent_notification_state(&rate_limit_key);
            self.agent_status.discard(&key);
        }
        self.tabs.retain(|t| !t.exited);
        if self.tabs.len() != before {
            if self.active >= self.tabs.len() && !self.tabs.is_empty() {
                self.active = self.tabs.len() - 1;
            }
            // The reap shifted `self.active` onto a different tab's buffer,
            // so an open search overlay is now indexed into a buffer that is
            // no longer active. Close it to match the `close_tab` /
            // `switch_to_tab` behavior (H4).
            if self.search.visible {
                self.search.close();
            }
            changed = true;
            // Tab roster changed; redraw everything to repaint the title bar
            // and the new active grid.
            self.needs_full_redraw = true;
            // D2: the reap may have moved the active index onto a different
            // tab (the exited tab was the active one, or the last one);
            // request a reconcile of its size against `cell_size` (deferred
            // — see `request_active_tab_reconcile`). No-op when empty
            // (nothing to reconcile) or already at size.
            self.request_active_tab_reconcile();
        }
        changed
    }

    /// Propagate a new grid size to the ACTIVE tab only (FR1, FR2). Inactive
    /// tabs are left untouched here — their PTYs must never receive a
    /// TIOCSWINSZ behind the user's back. A tab's size is reconciled to
    /// `self.cell_size` lazily, at the moment it becomes active (see
    /// `execute_pending_reconcile`, invoked by `window_host::render` after
    /// every activation path's deferred request).
    pub fn set_grid_size(&mut self, cols: u16, rows: u16) {
        // D3'''''' (round-9 rework, review round-8 finding
        // `1e7e069001cf22dc`, AC-5): clamp HERE, before comparing against or
        // assigning `self.cell_size` — `Tab::resize` (invoked below, via
        // `apply_tab_resize`) applies the IDENTICAL pure
        // `clamp_dims_to_wire_domain` clamp to the active tab's own core, so
        // computing it once up front and using the SAME clamped value for
        // both `self.cell_size` and the active-tab resize keeps the app's
        // own grid record from ever disagreeing with what the core it
        // drives actually holds. Before this fix, `self.cell_size` recorded
        // the caller's RAW request while `Tab::resize` silently narrowed
        // the core to a smaller wire-domain size — renderer / hit-testing /
        // `window_host::grid_size` all read `self.cell_size`, so they
        // described a size the core itself was never actually at.
        let (cols, rows) = crate::mux::session::pane::clamp_dims_to_wire_domain(cols, rows);
        if self.cell_size.cols == cols && self.cell_size.rows == rows {
            return;
        }
        self.cell_size = GridDims { cols, rows };
        // Design 2 (FR6, task0003 rework): issue the resize through the
        // SAME App-side application path the reconcile executor uses —
        // reads the active tab's pre-resize column count, applies
        // `Tab::resize`, and clears the App-owned reflow-invalidated
        // trackers (selection, pending anchor) if the resize changed the
        // column count (N3). The active tab's OWN reflow-invalidated
        // trackers (prompt / fold marks) are cleared by the tab itself
        // inside its own `resize` (IMPLEMENTATION.md D3); an inactive tab
        // is never resized here, so its trackers are unaffected.
        self.apply_tab_resize(self.active, cols, rows);
        // A reshape rewraps scrollback / viewport, so an open search
        // overlay's cached logical-line document no longer matches the
        // buffer; flag it for rebuild on the next `execute`.
        if self.search.visible {
            self.search.mark_buffer_dirty();
        }
        // Resize invalidates the previously rendered framebuffer; the
        // simplest correct response is a full redraw.
        self.needs_full_redraw = true;
    }

    /// Mark the next frame as needing a full repaint regardless of the
    /// `term_core` dirty bits. Use after resize / surface recovery /
    /// other structural changes.
    pub fn mark_full_redraw(&mut self) {
        self.needs_full_redraw = true;
    }

    /// Whether a full repaint is currently pending (`mark_full_redraw` /
    /// `force_full_redraw`). Read by `window_host::render` at the grid-build
    /// point: events applied from the frame's own egui pass (a tab switch,
    /// a scrollbar jump) raise the flag *mid-frame*, after the frame-top
    /// dirty snapshot was taken — the build must then widen to every row
    /// instead of trusting the stale snapshot, or the per-row cache keeps
    /// serving the previous tab's content.
    pub fn full_redraw_pending(&self) -> bool {
        self.needs_full_redraw || self.force_full_redraw
    }

    /// Whether the mux window-list sidebar overlay is currently open.
    /// Rendering input only — see the `mux_sidebar_overlay_open` field doc.
    pub fn mux_sidebar_overlay_open(&self) -> bool {
        self.mux_sidebar_overlay_open
    }

    /// FR4: whether the active tab/window cell should be scrolled into view
    /// this frame. Read by `render::draw_terminal` (immutable `&App`) and
    /// threaded into the tab strip; cleared post-frame by `window_host`.
    pub fn scroll_active_tab_into_view(&self) -> bool {
        self.scroll_active_tab_into_view
    }

    /// FR4: clear the one-shot scroll-into-view signal after the egui pass so
    /// it fires for exactly one frame and never re-fires on an unrelated
    /// repaint. Called from `window_host::render` where `&mut App` is held.
    pub fn clear_scroll_active_tab_into_view(&mut self) {
        self.scroll_active_tab_into_view = false;
    }

    // ── Scrollback (Phase 4 sub-phase 4) ─────────────────────

    /// Current scrollback offset in rows. `0` means `Live`. Saturates at the
    /// configured `scrollback_lines` ceiling.
    pub fn scroll_offset(&self) -> u32 {
        match self.scroll_position {
            ScrollPosition::Live => 0,
            ScrollPosition::OffsetFromLive(n) => n,
        }
    }

    /// Recompute the per-frame [`crate::fold::FoldLayout`] for the active
    /// tab. Called once at the top of `WindowHost::render` (before the
    /// immutable-borrow paint passes). Stores `Some(layout)` only when the
    /// active tab has at least one collapsed region — the renderer's
    /// fold-aware paths gate on `fold_layout().is_some()`, mirroring the
    /// WebView's `getCollapsedRegions().length > 0`. Otherwise stores `None`
    /// so the linear (non-folded) draw path runs unchanged.
    ///
    /// The viewport row count + scrollback length are read from the active
    /// tab's `TerminalCore`; the offset from [`Self::scroll_offset`].
    pub fn refresh_fold_layout(&mut self) {
        let scroll_offset = self.scroll_offset();
        // Snapshot the dimensions under the core lock, then build the layout
        // off the tab's `FoldManager` (which needs `&mut` for its collapsed
        // cache) without holding the core lock across the build.
        let dims = self.active_tab().map(|tab| {
            let core = tab.core.lock();
            (core.get_scrollback_length(), core.rows())
        });
        self.fold_layout = match (dims, self.active_tab_mut()) {
            (Some((scrollback_len, rows)), Some(tab)) => {
                if tab.folds.has_collapsed_regions() {
                    Some(tab.folds.build_layout(scrollback_len, rows, scroll_offset))
                } else {
                    None
                }
            }
            _ => None,
        };
    }

    /// The fold layout built for the current frame, if the active tab has
    /// collapsed regions. `None` selects the linear (non-folded) render
    /// path. See [`Self::refresh_fold_layout`].
    pub fn fold_layout(&self) -> Option<&crate::fold::FoldLayout> {
        self.fold_layout.as_ref()
    }

    /// Toggle (or expand) the fold region under a plain left-click at screen
    /// row `display_row` (0-based, counted from the top grid row). Port of
    /// the WebView `FoldHandler.handleFoldClick` (`handlers/fold.ts`); the
    /// caller (`window_host`) is responsible for the WebView's
    /// outer guards that have no equivalent in pure `App` state: a plain
    /// left-click (no Ctrl/Alt/Meta), with no active text selection, that
    /// did not become a drag-select.
    ///
    /// Behavior, mirroring the WebView:
    /// - Clicking a collapsed region's **summary row** expands that region.
    /// - Clicking inside an **expanded** foldable region collapses it. When
    ///   the region's summary sits *above* the current view top
    ///   (`regionDisplayLine < display_start`) the scroll offset is reduced
    ///   by `line_count - 1` (the rows the collapse hides) so the content
    ///   under the pointer stays visually anchored.
    /// - Clicking outside any region (or in scrollback below a region) is a
    ///   no-op.
    ///
    /// Returns `true` when fold state changed (the caller should request a
    /// redraw). `display_row >= rows` (a click below the grid) is rejected,
    /// matching the WebView's `displayRow < 0 || >= rows` guard.
    pub fn handle_fold_click(&mut self, display_row: u16) -> bool {
        // Snapshot the geometry under the core lock, then operate on the
        // fold manager (which needs `&mut` for its collapsed cache) without
        // holding the lock across the mutation — same pattern as
        // `refresh_fold_layout`.
        let scroll_offset = self.scroll_offset();
        let Some((scrollback_len, rows)) = self.active_tab().map(|tab| {
            let core = tab.core.lock();
            (core.get_scrollback_length(), core.rows())
        }) else {
            return false;
        };
        if display_row >= rows {
            return false;
        }
        let Some(tab) = self.active_tab_mut() else {
            return false;
        };
        if !tab.folds.is_enabled() {
            return false;
        }

        let total_actual = scrollback_len + rows as u32;
        let total_display = tab.folds.get_total_display_lines(total_actual);
        // display_start = max(0, total_display - rows - scroll_offset),
        // saturating to stay in u32 (matches `build_layout`).
        let display_start = total_display
            .saturating_sub(rows as u32)
            .saturating_sub(scroll_offset);
        let display_line = display_start + display_row as u32;

        // A click on a collapsed region's summary row expands it.
        if let Some(region) = tab.folds.get_summary_region(display_line) {
            tab.folds.expand_region_containing(region.start_line);
            self.needs_full_redraw = true;
            return true;
        }

        // Otherwise: a click inside an expanded foldable region collapses it.
        let actual_line = tab.folds.display_line_to_actual(display_line);
        let region = match tab.folds.get_region_at_line(actual_line) {
            Some(r) if !r.collapsed => (r.start_line, r.line_count),
            _ => return false,
        };
        let (region_start, line_count) = region;
        // Capture the region's display row BEFORE collapsing (matches the
        // WebView's `regionDisplayLine` computed before `toggleFold`).
        let region_display_line = tab.folds.actual_line_to_display(region_start);
        tab.folds.toggle_fold(actual_line);
        // Collapsing a region whose summary is above the view top hides
        // `line_count - 1` rows from the scrollback above us; pull the
        // offset down by the same amount so the click target stays put.
        if region_display_line < display_start {
            let delta = line_count - 1;
            let new_offset = scroll_offset.saturating_sub(delta);
            // Re-borrow `self` (the `tab` borrow ends here): `scroll_set_offset`
            // takes `&mut self` and clamps / snaps to Live at 0.
            self.scroll_set_offset(new_offset);
        }
        self.needs_full_redraw = true;
        true
    }

    /// Scroll back by `delta` rows (clamps to `scrollback_lines`). No-op
    /// when alt-screen is active.
    pub fn scroll_up_by(&mut self, delta: u32) {
        if self.alt_screen {
            return;
        }
        let new = self.scroll_offset().saturating_add(delta);
        let max = self.settings.scrollback_lines;
        self.scroll_position = if new == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(new.min(max))
        };
        self.needs_full_redraw = true;
    }

    /// Scroll forward (toward live tail) by `delta` rows. Snaps to `Live`
    /// when the resulting offset would be zero. No-op when alt-screen is
    /// active.
    pub fn scroll_down_by(&mut self, delta: u32) {
        if self.alt_screen {
            return;
        }
        let cur = self.scroll_offset();
        let new = cur.saturating_sub(delta);
        self.scroll_position = if new == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(new)
        };
        self.needs_full_redraw = true;
    }

    /// Jump to an absolute scrollback offset in rows back from live
    /// (`0` snaps to `Live`). Used by the scrollbar thumb, which
    /// computes its target against the *actual* scrollback length;
    /// the `scrollback_lines` clamp here is a safety net only. No-op
    /// when alt-screen is active.
    pub fn scroll_set_offset(&mut self, offset: u32) {
        if self.alt_screen {
            return;
        }
        self.scroll_position = if offset == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(offset.min(self.settings.scrollback_lines))
        };
        self.needs_full_redraw = true;
    }

    /// Jump to the top of the scrollback buffer (max offset).
    pub fn scroll_to_top(&mut self) {
        if self.alt_screen {
            return;
        }
        let max = self.settings.scrollback_lines;
        self.scroll_position = if max == 0 {
            ScrollPosition::Live
        } else {
            ScrollPosition::OffsetFromLive(max)
        };
        self.needs_full_redraw = true;
    }

    /// Jump back to the live tail.
    pub fn scroll_to_live(&mut self) {
        self.scroll_position = ScrollPosition::Live;
        self.needs_full_redraw = true;
    }

    // ── Prompt-to-prompt navigation (OSC 133) ────────────────

    /// Scroll to the nearest OSC 133 prompt mark in `direction` relative to
    /// the current view top. Port of the WebView
    /// `KeyboardHandler.handlePromptJump` (`handlers/keyboard.ts`).
    ///
    /// Coordinate model: marks carry an absolute scrollback-frame row
    /// (`0..scrollback_len` is scrollback, `scrollback_len..+rows` is the
    /// viewport). The current view top is `scrollback_len - scroll_offset`
    /// (offset is rows back from live). Scrolling to a mark sets the offset
    /// to `scrollback_len - mark_row`, putting the mark line at the top of
    /// the view — a viewport-row mark therefore resolves to a `Live`
    /// (`offset == 0`) position. When no mark exists in `direction`, fall
    /// to the top (`Prev`) or the live tail (`Next`), matching WebView.
    ///
    /// No-op on the alternate screen (same guard as the `scroll_*` family;
    /// alt-screen apps own the whole viewport and have no scrollback).
    ///
    /// Note: when the target prompt mark sits inside a *collapsed* fold
    /// region the region is auto-expanded first (mirroring the WebView
    /// `handlePromptJump`'s `foldManager.expandRegionContaining(marker.lineIndex)`),
    /// so the prompt is visible after the jump. The scroll offset is still
    /// computed against the mark's absolute row — expansion only changes the
    /// display↔actual mapping, not the buffer row the mark lives on, so the
    /// `scrollback_len - row` offset places the prompt at the view top either
    /// way.
    pub fn jump_to_prompt(&mut self, direction: JumpDirection) {
        if self.alt_screen {
            return;
        }
        let scrollback_len = match self.tabs.get(self.active) {
            Some(tab) => tab.core.lock().get_scrollback_length(),
            None => return,
        };
        let current_top_line = scrollback_len.saturating_sub(self.scroll_offset());
        let target = match self.tabs.get(self.active) {
            Some(tab) => match direction {
                JumpDirection::Prev => tab.prompts.find_prev_prompt(current_top_line),
                JumpDirection::Next => tab.prompts.find_next_prompt(current_top_line),
            },
            None => return,
        };
        match target {
            Some(row) => {
                // Auto-expand a collapsed fold region containing the mark so
                // jumping into folded output reveals the prompt. No-op when
                // the mark is in no region (or an already-expanded one).
                if let Some(tab) = self.tabs.get_mut(self.active) {
                    tab.folds.expand_region_containing(row);
                }
                let offset = scrollback_len.saturating_sub(row);
                self.scroll_set_offset(offset);
            }
            None => match direction {
                // No previous prompt: jump to the top of scrollback
                // (`scroll_set_offset` clamps to the configured ceiling).
                JumpDirection::Prev => self.scroll_set_offset(scrollback_len),
                // No next prompt: snap back to the live tail.
                JumpDirection::Next => self.scroll_set_offset(0),
            },
        }
    }

    // ── In-terminal search ───────────────────────────────────

    /// Open the search overlay (or re-focus it if already open). Sets the
    /// one-shot `search_focus_request` so the renderer grabs keyboard
    /// focus + selects the existing query on the next frame, mirroring
    /// `SearchHandler.toggleSearch` → `SearchBar.show()`.
    pub fn open_search(&mut self) {
        self.search.open();
        self.search_focus_request = true;
        self.needs_full_redraw = true;
    }

    /// Close the search overlay and clear its state + highlights.
    pub fn close_search(&mut self) {
        self.search.close();
        self.search_focus_request = false;
        self.needs_full_redraw = true;
    }

    /// Whether the search overlay is currently visible. Used by the key
    /// router in `window_host` to decide between the search-input path
    /// and the normal PTY/keybind path.
    pub fn search_visible(&self) -> bool {
        self.search.visible
    }

    /// Re-run the search against the active tab and update highlights.
    /// Called after the query / options change (incremental search). The
    /// current match is scrolled into view via [`Self::scroll_to_current_match`].
    pub fn run_search(&mut self) {
        if let Some(tab) = self.tabs.get(self.active) {
            let core = tab.core.lock();
            self.search.execute(&core);
        }
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Re-resolve the matches against the active tab's *current* buffer
    /// without scrolling. Called once per frame (after the pumps in the
    /// event loop) when [`SearchState::needs_research`] is set, so an open
    /// overlay's highlights keep tracking their text as new PTY output (or a
    /// resize) shifts rows into scrollback and changes their absolute row
    /// numbers.
    ///
    /// Time-throttled to one re-resolve per [`AUTO_RESEARCH_THROTTLE`]: a
    /// burst of PTY output flags the document dirty every frame, but
    /// rebuilding the logical-line document + recompiling the match that
    /// often is wasteful. When the gate blocks a run, the dirty flag is
    /// **kept** (we do not call `execute`), so the pending change is picked
    /// up on the next frame past the gap. User-driven [`Self::run_search`]
    /// bypasses the gate.
    ///
    /// Unlike [`Self::run_search`], this deliberately does **not** call
    /// [`Self::scroll_to_current_match`]: an automatic re-resolve must not
    /// yank the viewport, and it preserves the navigation cursor (clamped to
    /// the new match count) rather than snapping back to the first hit, so
    /// the N/M indicator does not jitter as the buffer scrolls (H5). Returns
    /// `true` when a re-search ran so the caller can request a repaint.
    pub fn auto_research_if_dirty(&mut self) -> bool {
        if !self.search.needs_research() {
            return false;
        }
        let now = Instant::now();
        if !auto_research_allowed(self.last_auto_research, now) {
            // Throttled: leave the dirty flag set so the next frame past the
            // gap re-resolves. No `execute`, no repaint request.
            return false;
        }
        self.last_auto_research = Some(now);
        if let Some(tab) = self.tabs.get(self.active) {
            let core = tab.core.lock();
            self.search.execute_preserving_current(&core);
        }
        self.needs_full_redraw = true;
        true
    }

    /// Advance to the next match (wrap-around) and scroll it into view.
    pub fn search_next(&mut self) {
        self.search.next_match();
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Step to the previous match (wrap-around) and scroll it into view.
    pub fn search_prev(&mut self) {
        self.search.prev_match();
        self.scroll_to_current_match();
        self.needs_full_redraw = true;
    }

    /// Scroll the viewport so the current match is roughly centered, if it
    /// is not already visible. No-op on the alt screen (no scrollback) or
    /// when there is no current match.
    ///
    /// Also auto-expands any collapsed fold region that contains the match,
    /// mirroring the WebView's `search.ts:154`
    /// `foldManager.expandRegionContaining(match.lineIndex)`. The expand
    /// happens before the scroll calculation so the geometry is already
    /// updated when the offset is computed.
    fn scroll_to_current_match(&mut self) {
        if self.alt_screen {
            return;
        }
        // Collect the match row first, then drop the borrow on `self.search`
        // so `self.tabs` can be mutably borrowed for the fold expand below.
        let abs_row = {
            let Some(m) = self.search.current_match() else {
                return;
            };
            // The match's first segment's absolute row anchors the scroll.
            let Some(seg) = m.segments.first() else {
                return;
            };
            seg.abs_row
        };
        // Auto-expand a collapsed fold region containing the match so the
        // hit is visible — mirroring the WebView's search.ts:154 call to
        // `foldManager.expandRegionContaining(match.lineIndex)`.
        // No-op when the match is in no region or an already-expanded one.
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.folds.expand_region_containing(abs_row);
        }
        let (scrollback_len, rows) = match self.tabs.get(self.active) {
            Some(tab) => {
                let core = tab.core.lock();
                (core.get_scrollback_length(), core.rows())
            }
            None => return,
        };
        if let Some(offset) = crate::search::scroll_offset_for_match(
            abs_row,
            scrollback_len,
            rows,
            self.scroll_offset(),
        ) {
            self.scroll_set_offset(offset);
        }
    }

    /// React to new PTY output on the active tab. `scrollback_delta` is the
    /// number of rows that spilled into scrollback during this `pump_all`
    /// pass (`after_len - before_len` of the active tab's
    /// `get_scrollback_length`). It is used to keep the visible content
    /// anchored when the user is parked at an `OffsetFromLive` view:
    ///
    /// * Below scrollback capacity, every pushed row grows `scrollback_len`
    ///   by 1, so the visible-row formula `scrollback_len - scroll_offset`
    ///   would advance unless we increment `scroll_offset` by the same Δ.
    /// * Once capacity is reached, `scrollback_len` is pinned and Δ == 0
    ///   — the visible row composition shifts (as intended; the user has
    ///   accepted that "you can't keep your place forever").
    ///
    /// `active_changed` is `true` only when the **active** tab produced
    /// the new bytes. The search overlay is resolved against the active
    /// tab's buffer, so background-tab output must not invalidate its
    /// cached logical-line document (H3) — a switch to that tab closes
    /// the overlay anyway, so a background change never needs to be
    /// reflected.
    pub fn on_pty_output(&mut self, active_changed: bool, scrollback_delta: u32) {
        // New PTY output on the active tab mutates its scrollback / viewport,
        // so an open search overlay's cached logical-line document is stale,
        // and the matches' absolute rows shift as rows spill into scrollback.
        // Flag it dirty here; the frame loop calls `auto_research_if_dirty`
        // once per frame to rebuild and re-resolve (the per-frame cadence
        // throttles bursts of PTY chunks — we do not re-search per chunk).
        // Only meaningful while the overlay is visible; harmless otherwise
        // since `clear`/`close` resets the flag anyway.
        if active_changed && self.search.visible {
            self.search.mark_buffer_dirty();
        }
        // No-op for `Live`; explicit branch documents intent.
        if let ScrollPosition::OffsetFromLive(n) = self.scroll_position {
            // Advance the offset by Δ so the visible row composition stays
            // anchored: `visible_start = scrollback_len - scroll_offset`
            // grows in lockstep with `scrollback_len` while below the
            // configured capacity. Clamp to `scrollback_lines` because the
            // offset cannot exceed the ring's depth — once we hit the cap,
            // further pushes evict rows and the parked view drifts (Δ == 0
            // at capacity, accepted).
            let max = self.settings.scrollback_lines;
            let new_n = n.saturating_add(scrollback_delta).min(max);
            self.scroll_position = ScrollPosition::OffsetFromLive(new_n);
            self.needs_full_redraw = true;
        }
    }

    /// Update the alt-screen flag. Called from `Tab::pump` after draining
    /// `core.take_mode_actions()`. Entering alt-screen forces the scroll
    /// position back to `Live` (alt buffers have no scrollback).
    pub fn set_alt_screen(&mut self, active: bool) {
        if self.alt_screen == active {
            return;
        }
        self.alt_screen = active;
        if active {
            self.scroll_position = ScrollPosition::Live;
        }
        // The selection holds absolute-row coordinates of the screen it was
        // made on; toggling between the main and alt screen changes the
        // buffer the selection addresses, so drop it (and any pending press
        // anchor) — matches current eMterm behavior and the SPEC's
        // "alt-screen toggle during selection → selection cleared".
        self.selection = None;
        self.pending_selection_anchor = None;
        self.needs_full_redraw = true;
    }

    /// Compute the rows that must be repainted on the next frame. Union of:
    /// 1. `term_core::get_dirty_rows()` for cell-level edits
    /// 2. previous + current cursor row, but only when the cursor cell
    ///    actually moved since the previous frame (clears the cursor ghost
    ///    on move; a stationary cursor with a stable blink phase costs no
    ///    repaint of its row now that the block cursor lives in the egui
    ///    overlay rather than the grid instances — see task0001)
    /// 3. the current cursor row again, but only on a blink phase flip
    ///    while blink is enabled (so the overlay can paint/erase the glyph)
    /// 4. previous + current selection rows (to clear highlight on shrink)
    ///
    /// Returns a sorted, deduplicated `Vec`. Returns `0..rows` when
    /// `needs_full_redraw` or `force_full_redraw` is set. Honest emptiness
    /// is the point: a frame with no content, cursor, blink, selection, or
    /// scroll change returns an empty `Vec` so the render-skip decision in
    /// `window_host.rs` can actually fire (task0002 AC-1).
    pub fn dirty_rows_this_frame(&self, core: &TerminalCore) -> Vec<u16> {
        let rows = core.rows();
        if rows == 0 {
            return Vec::new();
        }

        if self.force_full_redraw || self.needs_full_redraw {
            return (0..rows).collect();
        }

        let mut set: Vec<u16> = core.get_dirty_rows();

        let push_unique = |set: &mut Vec<u16>, r: u16| {
            if r < rows && !set.contains(&r) {
                set.push(r);
            }
        };

        // Cursor history: previous + current row, but only when the
        // cursor cell (row, col) actually moved since the previous frame —
        // a stationary cursor pushes no row here, letting an idle frame's
        // dirty set go empty.
        let cursor_row = core.get_cursor_row();
        let cursor_col = core.get_cursor_col();
        let cursor_moved = self.previous_cursor != Some((cursor_row, cursor_col));
        if cursor_moved {
            push_unique(&mut set, cursor_row);
            if let Some((prev_row, _)) = self.previous_cursor {
                push_unique(&mut set, prev_row);
            }
        }
        // Independently of cursor movement, a blink phase flip still needs
        // the cursor row repainted (or erased) — but only while blink is
        // enabled; a disabled blink never pushes a row here.
        let blink_enabled = core.get_cursor_blink();
        if blink_enabled && self.blink_visible_now(blink_enabled) != self.previous_blink_visible {
            push_unique(&mut set, cursor_row);
        }

        // Selection history: union of previous + current. The selection now
        // holds *absolute* buffer rows, so each range must be translated back
        // into the screen rows it occupies before being added to the dirty
        // set.
        //
        // With a fold layout the core's row coordinates (viewport rows, used
        // by core.get_dirty_rows()/get_cursor_row() above) and the absolute
        // → screen mapping are both non-linear with respect to screen rows
        // (collapsed bodies are hidden, summary rows draw no cells), so a
        // simple `abs - visible_start` does not hold and core-space dirty
        // rows cannot be reinterpreted as screen rows. Rather than reproduce
        // the fold walk here just for the dirty set, repaint every row
        // whenever a fold layout is active — folds are rare and the cost is
        // bounded to the viewport.
        if self.fold_layout.is_some() {
            return (0..rows).collect();
        }

        // Linear case: map the absolute selection range onto screen rows by
        // intersecting it with the visible window `[visible_start,
        // visible_start + rows)` and offsetting by `visible_start`.
        let push_abs_range = |set: &mut Vec<u16>, visible_start: u32, sel: &Selection| {
            let (start, end) = sel.ordered();
            let win_end = visible_start + rows as u32; // exclusive
            let lo = start.row.max(visible_start);
            let hi = (end.row + 1).min(win_end); // exclusive
            if lo >= hi {
                return;
            }
            for abs in lo..hi {
                push_unique(set, (abs - visible_start) as u16);
            }
        };
        let visible_start_now = core
            .get_scrollback_length()
            .saturating_sub(self.scroll_offset());
        if let Some(sel) = &self.selection {
            push_abs_range(&mut set, visible_start_now, sel);
        }
        if let Some(sel) = &self.previous_selection {
            // The previous selection's rows are interpreted against the
            // visible-start captured when that frame rendered.
            push_abs_range(&mut set, self.previous_visible_start, sel);
        }

        set.sort_unstable();
        set
    }

    /// Called after the renderer consumed the dirty set. Stores current
    /// cursor/selection/blink-phase for next-frame ghost prevention,
    /// clears the `needs_full_redraw` flag, and clears the core's dirty
    /// bits.
    pub fn record_render_state(&mut self, core: &mut TerminalCore) {
        self.previous_cursor = Some((core.get_cursor_row(), core.get_cursor_col()));
        self.previous_selection = self.selection;
        // Snapshot the visible-start used this frame so next frame's dirty
        // computation can translate `previous_selection`'s absolute rows back
        // into the screen rows it actually occupied.
        self.previous_visible_start = core
            .get_scrollback_length()
            .saturating_sub(self.scroll_offset());
        self.previous_blink_visible = self.blink_visible_now(core.get_cursor_blink());
        self.previous_status_bar_view_model = Some(self.status_bar_view_model());
        // task0002 AC-9: snapshot the hover flag so the NEXT frame's
        // `mux_sidebar_hover_changed` compares against what this frame
        // actually painted, not a stale earlier value.
        self.mux_sidebar_hover_prev_render = self.mux_sidebar_overlay_hovered;
        self.needs_full_redraw = false;
        core.clear_dirty();
    }

    /// Variant for the no-active-tab case (e.g. just after the last tab
    /// exited). Drops the `needs_full_redraw` flag so the next frame can
    /// short-circuit, but leaves cursor/selection history untouched
    /// because no core is available.
    pub fn record_render_state_no_tab(&mut self) {
        self.previous_cursor = None;
        self.previous_selection = None;
        self.previous_visible_start = 0;
        self.previous_blink_visible = true;
        self.previous_status_bar_view_model = Some(self.status_bar_view_model());
        self.mux_sidebar_hover_prev_render = self.mux_sidebar_overlay_hovered;
        self.needs_full_redraw = false;
    }

    /// tab at `tab_idx`. The actual routing logic lives on `Tab` so the
    /// tab can mutate its own grid / status state directly — this
    /// wrapper exists primarily as the test seam and as a stable name
    /// for future cross-tab mux behavior.
    ///
    /// Returns `true` when the tab's visible state changed (the caller
    /// should request a redraw).
    #[allow(dead_code)] // retained for future mux pumping / tests
    pub fn on_mux_message(&mut self, tab_idx: usize, msg: mux_ipc::protocol::MuxMessage) -> bool {
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            log::warn!("on_mux_message: tab_idx {tab_idx} out of range");
            return false;
        };
        tab.apply_mux_message(msg)
    }

    /// ユーザーにデスクトップ通知を送る。
    ///
    /// `notification_sink` フィールドの直接アクセスを避け、通知送信を
    /// アプリケーションドメインにカプセル化するためのメソッド。
    /// ウィンドウ層など外部からの通知送信はこのメソッドを経由すること。
    pub fn notify(&self, title: &str, body: &str) {
        self.notification_sink.send(title, body);
    }

    /// Fire (or suppress) a desktop notification for one drained
    /// agent-status transition (task0007 / FR9; task0001's event-type
    /// toggles).
    ///
    /// `pane_key` identifies the pane for the per-pane rate limit
    /// (task0005's mux `public_pane_id`, or a caller-chosen stable key for
    /// plain tabs) — an opaque string; the gating decision below never
    /// branches on its contents, so plain-tab-shaped and mux-pane-shaped
    /// keys produce identical judgements for identical settings/state
    /// inputs (task0001 AC-6). `pane_visible` is `true` when the pane is
    /// the one currently shown in the foreground OS window — the caller
    /// computes this (it owns the tab/pane visibility model; this method
    /// only applies the gating rule). `tab_title` feeds the notification
    /// body.
    ///
    /// This is the integration point IMPLEMENTATION.md assigns to
    /// task0007 ("read the model from app state"): once `AgentStatusModel`
    /// (task0005) is wired into `App`, its per-frame
    /// `drain_transitions()` calls this method once per drained event.
    /// `Settings::agent_notify_on_done` / `Settings::agent_notify_on_blocked`
    /// (task0001) are read here alongside the existing
    /// `agent_status_notifications` / `notification_enabled` gates and
    /// passed to [`crate::notifications::should_fire_agent_notification`],
    /// which selects the toggle matching `transition.new_state`. Returns
    /// whether the notification fired, for tests.
    pub fn maybe_notify_agent_transition(
        &mut self,
        pane_key: impl Into<String>,
        pane_visible: bool,
        transition: &crate::notifications::AgentTransition,
        tab_title: &str,
    ) -> bool {
        let pane_key = pane_key.into();
        let now = Instant::now();
        let rate_limit_ok = self
            .agent_notification_rate_limiter
            .is_within_limit(&pane_key, now);
        let fire = crate::notifications::should_fire_agent_notification(
            transition.new_state,
            pane_visible,
            self.settings.agent_status_notifications,
            self.settings.notification_enabled,
            self.settings.agent_notify_on_done,
            self.settings.agent_notify_on_blocked,
            rate_limit_ok,
        );
        if fire {
            self.agent_notification_rate_limiter.record(pane_key, now);
            let body =
                crate::notifications::agent_notification_body(transition, tab_title, self.locale);
            self.notify(crate::notifications::NOTIFICATION_TITLE, &body);
        }
        fire
    }

    /// Discard agent-notification rate-limit bookkeeping for a pane that
    /// closed (mirrors `AgentStatusModel`'s "discard on tab/pane close"
    /// contract — see [`App::maybe_notify_agent_transition`]).
    pub fn discard_agent_notification_state(&mut self, pane_key: &str) {
        self.agent_notification_rate_limiter
            .discard(&pane_key.to_string());
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Discover every tmux row for the new-tab chooser (task0001, SPEC A5):
/// one per live session, one fallback per un-enumerable socket. Always
/// empty on Windows (`crate::tmux_sockets` is Unix-only — task0001 Out
/// of Scope), so the chooser's fast-path / row-count logic in `App`
/// needs no platform branching beyond this one function. Labels and
/// spawn argv are precomputed here via the shared label / attach-
/// argument rules (`tmux_sockets::label` / `tmux_sockets::attach_args`)
/// so `ui::profile_selector::TmuxRow` stays a plain, cross-platform type
/// that never needs to name the Unix-only `tmux_sockets` module.
#[cfg(unix)]
fn discover_tmux_entries() -> Vec<crate::ui::profile_selector::TmuxRow> {
    crate::tmux_sockets::enumerate()
        .iter()
        .map(|entry| crate::ui::profile_selector::TmuxRow {
            label: crate::tmux_sockets::label(entry),
            argv: crate::tmux_sockets::attach_args(entry),
        })
        .collect()
}

#[cfg(not(unix))]
fn discover_tmux_entries() -> Vec<crate::ui::profile_selector::TmuxRow> {
    Vec::new()
}

#[cfg(test)]
mod tests;
