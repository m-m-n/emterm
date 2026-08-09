//! Mux sidebar / prefix-key dispatch / mux dialogs for [`App`].

use std::time::Instant;

use crate::settings::Settings;
use crate::tabs::Tab;

use super::App;

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

    /// Whether the mux window-list sidebar overlay is currently open.
    /// Rendering input only — see the `mux_sidebar_overlay_open` field doc.
    pub fn mux_sidebar_overlay_open(&self) -> bool {
        self.mux_sidebar_overlay_open
    }

    /// Phase 4-C (APC redesign): route one decoded `MuxMessage` to the
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
}
