//! Agent-status badges, notifications and rate limiting for [`App`].

use std::time::Instant;

use crate::tabs::Tab;

use super::App;

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
