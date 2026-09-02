//! Agent-status badges, notifications and rate limiting for [`App`].

use std::time::Instant;

use crate::tabs::Tab;

use super::App;

/// The `App::agent_status` keys `tab` occupies: its own plain-tab key
/// (`PaneKey::Tab`, harmless to include even when unoccupied — `discard` /
/// `mark_seen` on a missing key are no-ops) plus one `PaneKey::MuxPane` per
/// pane in its window group, if attached (task0005), scoped to `tab`'s OWN
/// [`crate::agent_status_model::ConnectionScope`] (mux-agent-status-pane-
/// key-collision D2: the tab's own `stable_id`) — so two tabs whose groups
/// hold identical wire pane ids yield fully disjoint key sets. Shared by
/// the tab-close (AC-6) and mark_seen-on-foreground-display (AC-5) call
/// sites so "which panes belong to this tab" is defined in exactly one
/// place.
pub(super) fn agent_status_keys_for_tab(
    tab: &crate::tabs::Tab,
) -> Vec<crate::agent_status_model::PaneKey> {
    use crate::agent_status_model::{ConnectionScope, PaneKey};
    let mut keys = vec![PaneKey::Tab(tab.stable_id)];
    if let Some(group) = tab.mux_group.as_ref() {
        let scope = ConnectionScope(tab.stable_id);
        keys.extend(
            group
                .pane_ids()
                .iter()
                .map(|&id| PaneKey::MuxPane(scope, id)),
        );
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
/// the tab whose OWN connection reported it (task0009 Design: "Resolve
/// tab_title from the transition's pane by locating its containing tab";
/// mux-agent-status-pane-key-collision FR5). A mux pane resolves through
/// its [`crate::agent_status_model::ConnectionScope`] alone — equal to the
/// owning tab's `stable_id` (D2) — NEVER through wire `pane_id` membership,
/// so a transition never resolves to a different tab that merely happens
/// to hold the same wire `pane_id` on another mux connection. `None` when
/// no tracked tab currently owns `pane` (its scope's tab closed between the
/// transition firing and this drain — the caller falls back to an empty
/// title; EC-4).
pub(super) fn agent_status_pane_tab_title<'a>(
    tabs: &'a [crate::tabs::Tab],
    pane: &crate::agent_status_model::PaneKey,
) -> Option<&'a str> {
    use crate::agent_status_model::PaneKey;
    tabs.iter()
        .find(|tab| match pane {
            PaneKey::Tab(id) => tab.stable_id == *id,
            PaneKey::MuxPane(scope, _pane_id) => tab.stable_id == scope.0,
        })
        .map(|tab| tab.title.as_str())
}

/// Resolve the per-pane notification rate-limit key for `pane` (task0009
/// Design: "Resolve rate_limit_key"). Mux panes prefer the daemon-learned
/// `public_pane_id`, looked up by the pane's scoped key — (`ConnectionScope`,
/// wire `pane_id`) — so two connections' same-numbered panes never share a
/// learned id (mux-agent-status-pane-key-collision FR3/FR4/D4); plain tabs
/// use a prefixed stable-id string. When no public id was ever learned, the
/// fallback embeds BOTH the scope and the wire `pane_id`
/// (`"mux:<scope>:<pane_id>"`) so two connections' unlearned panes still
/// derive distinct keys, and the existing `"mux:"` prefix keeps that
/// fallback from ever colliding with a plain-tab key (`"tab:"`). Shared by
/// every discard site (`close_tab`, the reaped-tab loop, `pump_all`'s
/// closed-mux-pane loop) and the transition-drain loop so all four derive
/// the same key for the same pane. Takes `mux_public_pane_ids` explicitly
/// (rather than `&App`) so it is testable without constructing a full
/// `App`.
pub(super) fn agent_notification_rate_limit_key(
    mux_public_pane_ids: &std::collections::HashMap<
        (crate::agent_status_model::ConnectionScope, u32),
        String,
    >,
    pane: &crate::agent_status_model::PaneKey,
) -> String {
    use crate::agent_status_model::PaneKey;
    match pane {
        PaneKey::Tab(id) => format!("tab:{id}"),
        PaneKey::MuxPane(scope, pane_id) => mux_public_pane_ids
            .get(&(*scope, *pane_id))
            .cloned()
            .unwrap_or_else(|| format!("mux:{}:{pane_id}", scope.0)),
    }
}

impl App {
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

    /// A single mux pane's aggregated badge, by (connection scope, wire
    /// `pane_id`) (task0006: `ui::mux_sidebar` window-entry badge — one
    /// pane per window entry; scoped per mux-agent-status-pane-key-
    /// collision FR2 so the caller must supply the owning tab's own
    /// scope).
    pub fn agent_status_pane_badge(
        &self,
        scope: crate::agent_status_model::ConnectionScope,
        pane_id: u32,
    ) -> Option<crate::agent_status_model::Aggregated> {
        self.agent_status
            .aggregate([&crate::agent_status_model::PaneKey::MuxPane(scope, pane_id)])
    }

    /// The daemon-minted public ID for mux pane `pane_id` within `scope`,
    /// if the GUI has learned it yet (task0006 AC-5; scoped per
    /// mux-agent-status-pane-key-collision FR3). `None` until the daemon
    /// pushes at least one `AgentStatusUpdate` for the pane — see
    /// [`Self::mux_public_pane_ids`].
    pub fn mux_public_pane_id(
        &self,
        scope: crate::agent_status_model::ConnectionScope,
        pane_id: u32,
    ) -> Option<&str> {
        self.mux_public_pane_ids
            .get(&(scope, pane_id))
            .map(String::as_str)
    }

    /// Fire (or suppress) a desktop notification for one drained
    /// agent-status transition (task0007 / FR9; task0001's event-type
    /// toggles; active-window-agent-notification task0001's visible-pane
    /// setting).
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
    /// (task0001) and `Settings::agent_notify_visible_pane`
    /// (active-window-agent-notification task0001) are read here alongside
    /// the existing `agent_status_notifications` / `notification_enabled`
    /// gates and passed to
    /// [`crate::notifications::should_fire_agent_notification`], which
    /// selects the toggle matching `transition.new_state` and applies the
    /// visible-pane setting to the visibility conjunct. Returns whether
    /// the notification fired, for tests.
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
            self.settings.agent_notify_visible_pane,
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

    /// Apply one `pump_all` pass' collected agent-status inputs, called
    /// after the tab loop once the `&mut self.tabs` borrow has ended
    /// (task0005): plain-tab OSC events, latch inputs, daemon
    /// `AgentStatusUpdate` pushes, and the mux pane ids a `PtyExited` arm
    /// removed this pump. Then drains the resulting real transitions and
    /// dispatches qualifying desktop notifications (task0009), and marks
    /// the active tab's panes seen while the OS window is focused
    /// (task0005 AC-5).
    ///
    /// FR4 requires a tab's real-status events and its latch inputs to be
    /// applied as ONE ordered stream: `reconcile_latch_feed` derives both
    /// lists from the same `pending_latch_feed` in order, so within a
    /// single tab each `Set`/`Clear` latch input corresponds 1:1 and
    /// same-order with one `AgentStatusEvent`. Walking the latch inputs
    /// and pulling that tab's next unconsumed plain event per `Set`/`Clear`
    /// reconstructs the true order; a bare `Mark` consumes no plain event
    /// (it only ever applies a real change indirectly, via
    /// `record_live_prompt_mark`'s own inferred
    /// `apply_plain_tab_event(Clear)` when the latch fires).
    ///
    /// The pairing MUST be per tab, never positional across the flattened
    /// lists: the two lists are filled by the same tab loop but a tab can
    /// contribute to one and not the other. A mux-connected tab still
    /// pushes real-status events while `process_combined` discards its
    /// latch feed (that tab's pane status is daemon-authoritative), so it
    /// contributes plain events and zero latch inputs. Consuming
    /// positionally would let that tab's events satisfy ANOTHER tab's
    /// `Set`/`Clear` and push the real events after the latch bookkeeping
    /// they belong with — reintroducing the very reordering this pairing
    /// exists to prevent. Events with no matching latch input are applied
    /// afterwards, in their original order.
    pub(super) fn apply_agent_status_batch(
        &mut self,
        plain_events: Vec<(u64, crate::agent_status::AgentStatusEvent)>,
        latch_inputs: Vec<(u64, crate::agent_status_model::ResolvedLatchInput)>,
        updates: Vec<(u64, mux_ipc::protocol::AgentStatusUpdateMsg)>,
        closed_panes: Vec<(u64, u32)>,
    ) {
        let mut plain_events: Vec<Option<(u64, crate::agent_status::AgentStatusEvent)>> =
            plain_events.into_iter().map(Some).collect();
        for (tab_stable_id, input) in latch_inputs {
            match input {
                crate::agent_status_model::ResolvedLatchInput::Set
                | crate::agent_status_model::ResolvedLatchInput::Clear => {
                    let paired = plain_events
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
        for (tab_stable_id, event) in plain_events.into_iter().flatten() {
            self.agent_status
                .apply_plain_tab_event(tab_stable_id, event);
        }
        for (tab_stable_id, update) in updates {
            // mux-agent-status-pane-key-collision FR1: every mux drain
            // reaches this batch apply already tagged with its originating
            // tab's scope — every key derived below comes from THIS pair,
            // never from `update.pane_id` alone.
            let scope = crate::agent_status_model::ConnectionScope(tab_stable_id);
            // task0006 AC-5: learn/refresh this pane's public ID from the
            // same message before applying it to the model — the daemon is
            // the only source for it (see `Self::mux_public_pane_ids`).
            self.mux_public_pane_ids
                .insert((scope, update.pane_id), update.public_pane_id.clone());
            self.agent_status.apply_daemon_update(
                scope,
                update.pane_id,
                update.state.map(crate::agent_status_model::state_from_wire),
                update.name,
                update.revision,
                update.replay_derived,
            );
        }
        for (tab_stable_id, pane_id) in closed_panes {
            // task0009 AC-4 / mux-agent-status-pane-key-collision FR6: the
            // closing tab's OWN scope only — never another tab's
            // same-numbered pane. Resolve the rate-limit key from the
            // still-present public-id mapping BEFORE removing it below.
            let scope = crate::agent_status_model::ConnectionScope(tab_stable_id);
            let key = crate::agent_status_model::PaneKey::MuxPane(scope, pane_id);
            let rate_limit_key = agent_notification_rate_limit_key(&self.mux_public_pane_ids, &key);
            self.mux_public_pane_ids.remove(&(scope, pane_id));
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
    }
}
