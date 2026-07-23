//! GUI-side merged agent-status store (task0005, SPEC FR5 / FR6 / NFR3).
//!
//! One [`AgentStatusModel`] covers both plain tabs (the GUI parses the OSC
//! 777 `agent-status` payload itself via [`crate::agent_status::parse`]) and
//! mux panes (the daemon pushes `AgentStatusUpdate` messages, applied here
//! by pane id). It tracks, per pane, the current semantic state, the
//! sanitized display name, a monotonic revision, and a GUI-local "unseen"
//! flag, plus a queue of real-transition events for the notification layer
//! (task0007).
//!
//! Semantics are pinned by `IMPLEMENTATION.md`'s `AgentStatusModel` shared
//! component and the "Revision semantics" / "Replay separation" cross-task
//! decisions:
//! - Plain tabs mint their own revision (starting at 0, incremented on every
//!   accepted report); mux panes carry the daemon-minted revision verbatim.
//! - `replay_derived` updates apply state/name/revision silently: never
//!   enqueue a transition, regardless of whether the state actually changed.
//! - The "unseen" flag is preserved across a report that does not change the
//!   semantic state (e.g. a same-state re-report or a replay restating the
//!   state the GUI already had) and reset to unseen on any real state
//!   change — independently of `replay_derived`.
//! - `aggregate` ranks by `blocked > unseen done > working > seen done >
//!   idle` (`doc/tasks/mux-agent-status-api/IMPLEMENTATION.md` Conventions).

use std::collections::{HashMap, VecDeque};

use crate::agent_status::{AgentState, AgentStatusEvent};

/// Identifies one agent-status-bearing entity tracked by the model.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PaneKey {
    /// A plain (non-mux) tab, keyed by its stable identity
    /// (`Tab::stable_id`), which survives close-driven index shifts.
    Tab(u64),
    /// A mux pane, keyed by the wire `pane_id` (the same id carried by
    /// `MuxMessage::pane_id` / `MuxWindowGroup::pane_ids`, not the
    /// API-facing `public_pane_id` string).
    MuxPane(u32),
}

/// One pane's tracked agent status.
///
/// `state: None` means the pane has no current status (never reported, or
/// most recently cleared) — such entries are excluded from [`aggregate`]
/// and [`counts`] but still occupy a slot (revision keeps advancing) until
/// [`AgentStatusModel::discard`] removes them on tab/pane close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStatus {
    pub state: Option<AgentState>,
    pub name: Option<String>,
    pub revision: u64,
    pub unseen: bool,
}

/// A real (non-replay, state-changing) transition, queued for the
/// notification layer (task0007) to drain and act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub pane: PaneKey,
    pub old_state: Option<AgentState>,
    pub new_state: Option<AgentState>,
    pub name: Option<String>,
}

/// Result of [`AgentStatusModel::aggregate`]: the highest-priority state
/// among the queried panes, plus that state's actual unseen flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aggregated {
    pub state: AgentState,
    pub unseen: bool,
}

/// Per-state counts across every tracked (non-cleared) pane, ignoring the
/// unseen flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub idle: u32,
    pub working: u32,
    pub blocked: u32,
    pub done: u32,
}

/// The merged agent-status store. Pure state — no I/O, no egui, no protocol
/// concerns — so it is unit-tested directly (see `tests` below).
#[derive(Debug, Default)]
pub struct AgentStatusModel {
    entries: HashMap<PaneKey, AgentStatus>,
    transitions: VecDeque<Transition>,
}

impl AgentStatusModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a plain-tab OSC event (already parsed by
    /// `crate::agent_status::parse`). The model mints the revision — plain
    /// tabs are never targeted by the mux agent API, so nothing else needs
    /// revision continuity with a daemon-side counter.
    pub fn apply_plain_tab_event(&mut self, tab_stable_id: u64, event: AgentStatusEvent) {
        let (new_state, name) = match event {
            AgentStatusEvent::Set { state, name } => (Some(state), name),
            AgentStatusEvent::Clear => (None, None),
        };
        let key = PaneKey::Tab(tab_stable_id);
        let next_revision = self.entries.get(&key).map_or(1, |e| e.revision + 1);
        self.apply_report(key, new_state, name, next_revision, false);
    }

    /// Apply a daemon-pushed `AgentStatusUpdate` for a mux pane. `revision`
    /// is the daemon-authoritative value and is stored verbatim (the model
    /// never increments it itself for mux panes).
    pub fn apply_daemon_update(
        &mut self,
        pane_id: u32,
        state: Option<AgentState>,
        name: Option<String>,
        revision: u64,
        replay_derived: bool,
    ) {
        self.apply_report(
            PaneKey::MuxPane(pane_id),
            state,
            name,
            revision,
            replay_derived,
        );
    }

    /// Shared apply path for both ingestion sources.
    ///
    /// - The "unseen" flag is reset to `true` on any real state change
    ///   (including the pane's very first report) and otherwise left
    ///   untouched — regardless of `replay_derived`.
    /// - A transition is enqueued only for a real state change AND
    ///   `!replay_derived`.
    fn apply_report(
        &mut self,
        key: PaneKey,
        new_state: Option<AgentState>,
        name: Option<String>,
        revision: u64,
        replay_derived: bool,
    ) {
        let entry_existed = self.entries.contains_key(&key);
        let prev_state = self.entries.get(&key).and_then(|e| e.state);
        let state_changed = !entry_existed || prev_state != new_state;

        let entry = self
            .entries
            .entry(key.clone())
            .or_insert_with(|| AgentStatus {
                state: None,
                name: None,
                revision: 0,
                unseen: false,
            });
        entry.state = new_state;
        entry.name = name.clone();
        entry.revision = revision;
        if state_changed {
            entry.unseen = true;
        }

        if state_changed && !replay_derived {
            self.transitions.push_back(Transition {
                pane: key,
                old_state: prev_state,
                new_state,
                name,
            });
        }
    }

    /// Discard a pane/tab's entry entirely (tab close / mux pane close).
    /// No-op when the key is not tracked.
    pub fn discard(&mut self, pane: &PaneKey) {
        self.entries.remove(pane);
    }

    /// Clear the unseen flag on every currently-tracked entry among `panes`.
    /// Does not touch semantic state or revision. Missing keys are no-ops.
    pub fn mark_seen<'a, I>(&mut self, panes: I)
    where
        I: IntoIterator<Item = &'a PaneKey>,
    {
        for pane in panes {
            if let Some(entry) = self.entries.get_mut(pane) {
                entry.unseen = false;
            }
        }
    }

    /// Read a single pane's tracked status, if any.
    pub fn status(&self, pane: &PaneKey) -> Option<&AgentStatus> {
        self.entries.get(pane)
    }

    /// Highest-priority state + that state's actual unseen flag among
    /// `panes`, ranked `blocked > unseen-done > working > seen-done > idle`.
    /// Panes with no tracked entry, or a cleared (`state: None`) entry, do
    /// not participate. Returns `None` when no queried pane currently
    /// carries a status.
    pub fn aggregate<'a, I>(&self, panes: I) -> Option<Aggregated>
    where
        I: IntoIterator<Item = &'a PaneKey>,
    {
        panes
            .into_iter()
            .filter_map(|k| self.entries.get(k))
            .filter_map(|e| e.state.map(|s| (s, e.unseen)))
            .max_by_key(|&(state, unseen)| (priority_rank(state, unseen), unseen))
            .map(|(state, unseen)| Aggregated { state, unseen })
    }

    /// Per-state counts across every tracked pane (all tabs/panes, not
    /// scoped to one tab), ignoring the unseen flag. Cleared entries are
    /// excluded. An empty model (or a model with only cleared entries)
    /// reports all-zero counts.
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for entry in self.entries.values() {
            match entry.state {
                Some(AgentState::Idle) => counts.idle += 1,
                Some(AgentState::Working) => counts.working += 1,
                Some(AgentState::Blocked) => counts.blocked += 1,
                Some(AgentState::Done) => counts.done += 1,
                None => {}
            }
        }
        counts
    }

    /// Drain every real-transition event queued since the last drain.
    pub fn drain_transitions(&mut self) -> Vec<Transition> {
        self.transitions.drain(..).collect()
    }
}

/// Priority bucket for [`AgentStatusModel::aggregate`]'s ranking:
/// `blocked(4) > unseen-done(3) > working(2) > seen-done(1) > idle(0)`.
fn priority_rank(state: AgentState, unseen: bool) -> u8 {
    match (state, unseen) {
        (AgentState::Blocked, _) => 4,
        (AgentState::Done, true) => 3,
        (AgentState::Working, _) => 2,
        (AgentState::Done, false) => 1,
        (AgentState::Idle, _) => 0,
    }
}

/// Convert the wire-level `mux_ipc::protocol::AgentState` mirror into the
/// core `crate::agent_status::AgentState` the model stores. Both enums
/// share the same four variants by contract (SPEC FR1 / `mux_ipc`'s "local
/// mirror" doc comment); this is a straight, total mapping.
pub fn state_from_wire(state: mux_ipc::protocol::AgentState) -> AgentState {
    match state {
        mux_ipc::protocol::AgentState::Idle => AgentState::Idle,
        mux_ipc::protocol::AgentState::Working => AgentState::Working,
        mux_ipc::protocol::AgentState::Blocked => AgentState::Blocked,
        mux_ipc::protocol::AgentState::Done => AgentState::Done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: u64) -> PaneKey {
        PaneKey::Tab(id)
    }

    fn pane(id: u32) -> PaneKey {
        PaneKey::MuxPane(id)
    }

    // ── AC-1: plain-tab OSC updates the model like a daemon update would ──

    #[test]
    fn plain_tab_set_mints_revision_and_enqueues_transition() {
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(
            1,
            AgentStatusEvent::Set {
                state: AgentState::Working,
                name: Some("claude".to_string()),
            },
        );

        let status = model.status(&tab(1)).expect("entry created");
        assert_eq!(status.state, Some(AgentState::Working));
        assert_eq!(status.name, Some("claude".to_string()));
        assert_eq!(status.revision, 1);
        assert!(status.unseen);

        let transitions = model.drain_transitions();
        assert_eq!(
            transitions,
            vec![Transition {
                pane: tab(1),
                old_state: None,
                new_state: Some(AgentState::Working),
                name: Some("claude".to_string()),
            }]
        );
    }

    #[test]
    fn plain_tab_clear_advances_revision_and_enqueues_transition() {
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(
            1,
            AgentStatusEvent::Set {
                state: AgentState::Working,
                name: None,
            },
        );
        model.drain_transitions();

        model.apply_plain_tab_event(1, AgentStatusEvent::Clear);

        let status = model.status(&tab(1)).expect("entry retained after clear");
        assert_eq!(status.state, None);
        assert_eq!(status.revision, 2);

        let transitions = model.drain_transitions();
        assert_eq!(
            transitions,
            vec![Transition {
                pane: tab(1),
                old_state: Some(AgentState::Working),
                new_state: None,
                name: None,
            }]
        );
    }

    #[test]
    fn plain_tab_and_daemon_paths_produce_equivalent_status_shape() {
        let mut plain = AgentStatusModel::new();
        plain.apply_plain_tab_event(
            1,
            AgentStatusEvent::Set {
                state: AgentState::Blocked,
                name: Some("agent".to_string()),
            },
        );

        let mut daemon = AgentStatusModel::new();
        daemon.apply_daemon_update(
            10,
            Some(AgentState::Blocked),
            Some("agent".to_string()),
            1,
            false,
        );

        let plain_status = plain.status(&tab(1)).unwrap();
        let daemon_status = daemon.status(&pane(10)).unwrap();
        assert_eq!(plain_status.state, daemon_status.state);
        assert_eq!(plain_status.name, daemon_status.name);
        assert_eq!(plain_status.revision, daemon_status.revision);
        assert_eq!(plain_status.unseen, daemon_status.unseen);
    }

    // ── AC-2: daemon replay_derived / same-state re-report gating ─────────

    #[test]
    fn daemon_update_real_change_enqueues_transition() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(10, Some(AgentState::Working), None, 1, false);
        assert_eq!(model.drain_transitions().len(), 1);
    }

    #[test]
    fn daemon_update_same_state_re_report_enqueues_nothing() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(10, Some(AgentState::Working), None, 1, false);
        model.drain_transitions();

        model.apply_daemon_update(10, Some(AgentState::Working), None, 2, false);
        assert!(model.drain_transitions().is_empty());
        // Revision still advances even though nothing else changed.
        assert_eq!(model.status(&pane(10)).unwrap().revision, 2);
    }

    #[test]
    fn daemon_update_replay_derived_never_enqueues_even_on_real_change() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(10, Some(AgentState::Blocked), None, 5, true);
        assert!(model.drain_transitions().is_empty());
        assert_eq!(
            model.status(&pane(10)).unwrap().state,
            Some(AgentState::Blocked)
        );
    }

    #[test]
    fn daemon_update_replay_derived_state_change_still_marks_unseen() {
        // Replay silences the *notification* (no transition) but the
        // "unseen" flag still reflects the real change, per the design
        // note: "seen flags are ... reset to unseen on a real state
        // change" (stated independently of replay_derived).
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(10, Some(AgentState::Working), None, 1, false);
        model.drain_transitions(); // discard the first (non-replay) transition
        model.mark_seen([&pane(10)]);
        assert!(!model.status(&pane(10)).unwrap().unseen);

        model.apply_daemon_update(10, Some(AgentState::Blocked), None, 2, true);
        assert!(model.status(&pane(10)).unwrap().unseen);
        assert!(model.drain_transitions().is_empty());
    }

    // ── AC-3: aggregate priority order + unseen distinction ───────────────

    #[test]
    fn aggregate_prefers_blocked_over_everything() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
        model.apply_daemon_update(2, Some(AgentState::Blocked), None, 1, false);
        model.apply_daemon_update(3, Some(AgentState::Done), None, 1, false);

        let panes = vec![pane(1), pane(2), pane(3)];
        let agg = model.aggregate(&panes).unwrap();
        assert_eq!(agg.state, AgentState::Blocked);
    }

    #[test]
    fn aggregate_ranks_unseen_done_above_working_above_seen_done_above_idle() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Idle), None, 1, false);
        model.apply_daemon_update(2, Some(AgentState::Done), None, 1, false);
        model.mark_seen([&pane(2)]); // seen-done
        model.apply_daemon_update(3, Some(AgentState::Working), None, 1, false);

        // seen-done(1) < working(2): working wins over idle+seen-done.
        let agg = model.aggregate(&[pane(1), pane(2), pane(3)]).unwrap();
        assert_eq!(agg.state, AgentState::Working);

        // unseen-done(3) > working(2): add an unseen-done pane and it wins.
        model.apply_daemon_update(4, Some(AgentState::Done), None, 1, false);
        let agg = model
            .aggregate(&[pane(1), pane(2), pane(3), pane(4)])
            .unwrap();
        assert_eq!(agg.state, AgentState::Done);
        assert!(agg.unseen);
    }

    #[test]
    fn aggregate_seen_done_reports_seen_unseen_false() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Done), None, 1, false);
        model.mark_seen([&pane(1)]);

        let agg = model.aggregate(&[pane(1)]).unwrap();
        assert_eq!(agg.state, AgentState::Done);
        assert!(!agg.unseen);
    }

    #[test]
    fn aggregate_returns_none_when_no_pane_has_status() {
        let model = AgentStatusModel::new();
        assert_eq!(model.aggregate(&[pane(1), tab(2)]), None);
    }

    #[test]
    fn aggregate_ignores_cleared_panes() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
        model.apply_daemon_update(1, None, None, 2, false); // cleared
        assert_eq!(model.aggregate(&[pane(1)]), None);
    }

    // ── AC-4: counts ignore seen, empty model reports zero ───────────────

    #[test]
    fn counts_are_zero_for_empty_model() {
        let model = AgentStatusModel::new();
        assert_eq!(model.counts(), Counts::default());
    }

    #[test]
    fn counts_reflect_semantic_state_regardless_of_seen() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Blocked), None, 1, false);
        model.apply_daemon_update(2, Some(AgentState::Blocked), None, 1, false);
        model.mark_seen([&pane(2)]); // seen, still counted as blocked
        model.apply_daemon_update(3, Some(AgentState::Working), None, 1, false);
        model.apply_daemon_update(4, Some(AgentState::Done), None, 1, false);
        model.apply_daemon_update(5, Some(AgentState::Idle), None, 1, false);
        model.apply_daemon_update(6, None, None, 1, false); // cleared, excluded

        let counts = model.counts();
        assert_eq!(
            counts,
            Counts {
                idle: 1,
                working: 1,
                blocked: 2,
                done: 1,
            }
        );
    }

    // ── AC-5: mark_seen clears unseen without touching state/revision ────

    #[test]
    fn mark_seen_clears_unseen_only_for_listed_panes() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Blocked), Some("a".into()), 3, false);
        model.apply_daemon_update(2, Some(AgentState::Working), None, 1, false);

        model.mark_seen([&pane(1)]);

        let seen = model.status(&pane(1)).unwrap();
        assert!(!seen.unseen);
        assert_eq!(seen.state, Some(AgentState::Blocked));
        assert_eq!(seen.name, Some("a".to_string()));
        assert_eq!(seen.revision, 3);

        // pane(2) was not in the mark_seen call: still unseen.
        assert!(model.status(&pane(2)).unwrap().unseen);
    }

    #[test]
    fn mark_seen_on_missing_pane_is_a_no_op() {
        let mut model = AgentStatusModel::new();
        model.mark_seen([&pane(99)]); // must not panic
        assert!(model.status(&pane(99)).is_none());
    }

    // ── AC-6: tab/pane close removes entries ──────────────────────────────

    #[test]
    fn discard_removes_entry_and_updates_aggregate_and_counts() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Blocked), None, 1, false);
        model.apply_daemon_update(2, Some(AgentState::Working), None, 1, false);

        model.discard(&pane(1));

        assert!(model.status(&pane(1)).is_none());
        assert_eq!(
            model.aggregate(&[pane(1), pane(2)]).unwrap().state,
            AgentState::Working
        );
        assert_eq!(
            model.counts(),
            Counts {
                working: 1,
                ..Counts::default()
            }
        );
    }

    #[test]
    fn discard_plain_tab_entry_via_tab_key() {
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(
            7,
            AgentStatusEvent::Set {
                state: AgentState::Done,
                name: None,
            },
        );
        model.discard(&tab(7));
        assert!(model.status(&tab(7)).is_none());
    }

    // ── AC-7: a real state change resets seen to unseen ───────────────────

    #[test]
    fn real_state_change_resets_seen_to_unseen() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
        model.mark_seen([&pane(1)]);
        assert!(!model.status(&pane(1)).unwrap().unseen);

        model.apply_daemon_update(1, Some(AgentState::Blocked), None, 2, false);
        assert!(model.status(&pane(1)).unwrap().unseen);
    }

    #[test]
    fn same_state_re_report_preserves_seen() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
        model.mark_seen([&pane(1)]);
        assert!(!model.status(&pane(1)).unwrap().unseen);

        model.apply_daemon_update(1, Some(AgentState::Working), None, 2, false);
        assert!(!model.status(&pane(1)).unwrap().unseen);
    }

    // ── wire-state conversion helper ───────────────────────────────────────

    #[test]
    fn state_from_wire_maps_every_variant() {
        assert_eq!(
            state_from_wire(mux_ipc::protocol::AgentState::Idle),
            AgentState::Idle
        );
        assert_eq!(
            state_from_wire(mux_ipc::protocol::AgentState::Working),
            AgentState::Working
        );
        assert_eq!(
            state_from_wire(mux_ipc::protocol::AgentState::Blocked),
            AgentState::Blocked
        );
        assert_eq!(
            state_from_wire(mux_ipc::protocol::AgentState::Done),
            AgentState::Done
        );
    }
}
