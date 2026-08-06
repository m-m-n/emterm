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
use crate::agent_status_exit_latch::AgentStatusExitLatch;

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

/// A true-order, live-only input to a plain tab's inferred-clear latch
/// (agent-exit-after-icon SPEC FR2/FR4/FR5), produced by
/// [`reconcile_latch_feed`] from `callbacks::LatchFeedEvent` candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedLatchInput {
    /// A live OSC 777 agent-status `Set` report.
    Set,
    /// A live OSC 777 agent-status `Clear` report.
    Clear,
    /// A live, alt-screen-confirmed OSC 133 mark.
    Mark(crate::prompts::PromptMarkKind),
}

/// Reconcile this pump's OSC 133 mark CANDIDATES
/// (`callbacks::LatchFeedEvent`, which may include alt-screen-suppressed
/// marks — see that type's doc) against `live_marks`, the alt-screen
/// -filtered ground truth for the SAME pump (e.g.
/// `TerminalCore::take_prompt_marks`'s output, converted to
/// `PromptMarkKind`), to produce a single true-order, live-only sequence
/// for [`AgentStatusModel`]'s per-tab inferred-clear latch
/// (agent-exit-after-icon FR4/FR5).
///
/// `live_marks` is, by construction, an ordered subsequence of the
/// `PromptMark` candidates in `feed` (every live mark also fired the
/// candidate-producing callback, in the same relative position) — so a
/// single forward walk correctly tells live candidates from
/// alt-screen-suppressed ones without this function (or any caller) ever
/// re-deriving alt-screen state itself. OSC 777 `Set`/`Clear` candidates
/// are never suppressed and always pass through unchanged, in their
/// original position.
pub fn reconcile_latch_feed(
    feed: Vec<crate::callbacks::LatchFeedEvent>,
    live_marks: &[crate::prompts::PromptMarkKind],
) -> Vec<ResolvedLatchInput> {
    let mut live_idx = 0;
    let mut resolved = Vec::with_capacity(feed.len());
    for candidate in feed {
        match candidate {
            crate::callbacks::LatchFeedEvent::Set => resolved.push(ResolvedLatchInput::Set),
            crate::callbacks::LatchFeedEvent::Clear => resolved.push(ResolvedLatchInput::Clear),
            crate::callbacks::LatchFeedEvent::PromptMark(kind) => {
                if live_marks.get(live_idx) == Some(&kind) {
                    live_idx += 1;
                    resolved.push(ResolvedLatchInput::Mark(kind));
                }
                // else: alt-screen-suppressed (or otherwise non-live)
                // candidate — dropped, `live_idx` not advanced.
            }
        }
    }
    resolved
}

/// The merged agent-status store. Pure state — no I/O, no egui, no protocol
/// concerns — so it is unit-tested directly (see `tests` below).
#[derive(Debug, Default)]
pub struct AgentStatusModel {
    entries: HashMap<PaneKey, AgentStatus>,
    transitions: VecDeque<Transition>,
    /// Per-plain-tab inferred-clear latches (agent-exit-after-icon FR2),
    /// keyed by the same `u64` `PaneKey::Tab` uses. Lazily created on
    /// first use (`Set` or a live mark); discarded together with the
    /// tab's [`AgentStatus`] entry in [`AgentStatusModel::discard`].
    latches: HashMap<u64, AgentStatusExitLatch>,
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
    /// No-op when the key is not tracked. Also discards the tab's
    /// inferred-clear latch instance, if any (agent-exit-after-icon
    /// AC-6) — mirrors the existing entry-discard handling so a closed
    /// tab never leaves a stale latch behind.
    pub fn discard(&mut self, pane: &PaneKey) {
        self.entries.remove(pane);
        if let PaneKey::Tab(tab_stable_id) = pane {
            self.latches.remove(tab_stable_id);
        }
    }

    /// Record a live OSC 777 `Set` report for a plain tab's inferred-clear
    /// latch (agent-exit-after-icon FR2). Lazily creates the latch on
    /// first use. Pure bookkeeping — does not itself touch
    /// state/name/revision (the caller separately applies the real report
    /// via [`Self::apply_plain_tab_event`]).
    pub fn record_latch_set(&mut self, tab_stable_id: u64) {
        self.latches.entry(tab_stable_id).or_default().record_set();
    }

    /// Record a live OSC 777 `Clear` report for a plain tab's
    /// inferred-clear latch (agent-exit-after-icon FR2). See
    /// [`Self::record_latch_set`]'s doc for the bookkeeping-only note.
    pub fn record_latch_clear(&mut self, tab_stable_id: u64) {
        self.latches
            .entry(tab_stable_id)
            .or_default()
            .record_clear();
    }

    /// Record a live, alt-screen-confirmed OSC 133 mark for a plain tab's
    /// inferred-clear latch (agent-exit-after-icon FR2/FR4/FR5). Callers
    /// (the plain-tab wiring; see [`reconcile_latch_feed`]) must supply
    /// only live, main-screen marks, in true arrival order relative to
    /// this tab's [`Self::record_latch_set`] / [`Self::record_latch_clear`]
    /// calls. When the latch reports an inferred clear, it is applied
    /// through [`Self::apply_plain_tab_event`] — the EXACT same code path
    /// an explicit `Clear` already uses (FR2); there is no parallel/
    /// duplicate clear-application logic.
    pub fn record_live_prompt_mark(
        &mut self,
        tab_stable_id: u64,
        kind: crate::prompts::PromptMarkKind,
    ) {
        let fire = self.latches.entry(tab_stable_id).or_default().record_mark(kind);
        if fire {
            self.apply_plain_tab_event(tab_stable_id, AgentStatusEvent::Clear);
        }
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

    /// Whether any of the given mux pane ids currently carries a reported
    /// (uncleared) agent status — one of Idle / Working / Blocked / Done.
    /// Cleared (`state: None`) and never-reported (no tracked entry) panes
    /// do not count. Used by the `next-agent-window` mux action (SPEC
    /// mux-agent-tab-cycle FR6) to decide whether a mux window qualifies for
    /// the cycle: a window qualifies when at least one of its panes
    /// qualifies (existential), per IMPLEMENTATION.md's any-reported-state
    /// assumption.
    pub fn any_pane_has_reported_state<'a, I>(&self, pane_ids: I) -> bool
    where
        I: IntoIterator<Item = &'a u32>,
    {
        pane_ids.into_iter().any(|pid| {
            self.entries
                .get(&PaneKey::MuxPane(*pid))
                .is_some_and(|e| e.state.is_some())
        })
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

/// The inverse of [`state_from_wire`]: convert the core
/// `crate::agent_status::AgentState` this model stores back into the
/// wire-level `mux_ipc::protocol::AgentState` mirror. Used by the
/// notification wiring (task0009) to build a
/// `crate::notifications::AgentTransition` from a drained [`Transition`],
/// whose `old_state`/`new_state` are core-enum values.
pub fn state_to_wire(state: AgentState) -> mux_ipc::protocol::AgentState {
    match state {
        AgentState::Idle => mux_ipc::protocol::AgentState::Idle,
        AgentState::Working => mux_ipc::protocol::AgentState::Working,
        AgentState::Blocked => mux_ipc::protocol::AgentState::Blocked,
        AgentState::Done => mux_ipc::protocol::AgentState::Done,
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

    // ── mux-agent-tab-cycle task0001 AC-7: any_pane_has_reported_state ────

    #[test]
    fn any_pane_has_reported_state_true_for_each_reported_state_kind() {
        for state in [
            AgentState::Idle,
            AgentState::Working,
            AgentState::Blocked,
            AgentState::Done,
        ] {
            let mut model = AgentStatusModel::new();
            model.apply_daemon_update(1, Some(state), None, 1, false);
            assert!(
                model.any_pane_has_reported_state(&[1]),
                "{state:?} must qualify"
            );
        }
    }

    #[test]
    fn any_pane_has_reported_state_false_for_cleared_and_never_reported() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(1, Some(AgentState::Working), None, 1, false);
        model.apply_daemon_update(1, None, None, 2, false); // cleared
        assert!(
            !model.any_pane_has_reported_state(&[1]),
            "cleared pane must not qualify"
        );
        assert!(
            !model.any_pane_has_reported_state(&[999]),
            "never-reported pane must not qualify"
        );
    }

    #[test]
    fn any_pane_has_reported_state_multi_pane_qualifies_existentially() {
        let mut model = AgentStatusModel::new();
        model.apply_daemon_update(2, Some(AgentState::Idle), None, 1, false);
        // pane 1 never reported, pane 2 reported Idle — the set qualifies
        // because at least one pane does (existential, FR6).
        assert!(model.any_pane_has_reported_state(&[1, 2]));
        // Neither pane in this set ever reported: does not qualify.
        assert!(!model.any_pane_has_reported_state(&[3, 4]));
    }

    #[test]
    fn any_pane_has_reported_state_empty_set_is_false() {
        let model = AgentStatusModel::new();
        assert!(!model.any_pane_has_reported_state(&[]));
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

    #[test]
    fn state_to_wire_maps_every_variant() {
        assert_eq!(
            state_to_wire(AgentState::Idle),
            mux_ipc::protocol::AgentState::Idle
        );
        assert_eq!(
            state_to_wire(AgentState::Working),
            mux_ipc::protocol::AgentState::Working
        );
        assert_eq!(
            state_to_wire(AgentState::Blocked),
            mux_ipc::protocol::AgentState::Blocked
        );
        assert_eq!(
            state_to_wire(AgentState::Done),
            mux_ipc::protocol::AgentState::Done
        );
    }

    #[test]
    fn state_to_wire_and_state_from_wire_round_trip() {
        for state in [
            AgentState::Idle,
            AgentState::Working,
            AgentState::Blocked,
            AgentState::Done,
        ] {
            assert_eq!(state_from_wire(state_to_wire(state)), state);
        }
    }

    // ── agent-exit-after-icon (task0002): plain-tab inferred-clear latch ──
    //
    // Integration tests exercising the actual callbacks.rs (`LatchFeedEvent`
    // candidates) -> reconcile_latch_feed -> latch -> AgentStatusModel path,
    // per task0002.md's Test Notes.

    use crate::callbacks::LatchFeedEvent;
    use crate::prompts::PromptMarkKind;

    fn set_event(state: AgentState) -> AgentStatusEvent {
        AgentStatusEvent::Set { state, name: None }
    }

    // ── AC-1: Set -> live D -> live A clears via the existing Clear path ──

    #[test]
    fn ac1_set_then_live_d_then_live_a_clears_via_existing_clear_path() {
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(1, set_event(AgentState::Working));
        model.record_latch_set(1);
        model.drain_transitions();

        model.record_live_prompt_mark(1, PromptMarkKind::CommandEnd);
        assert_eq!(model.status(&tab(1)).unwrap().state, Some(AgentState::Working));

        model.record_live_prompt_mark(1, PromptMarkKind::PromptStart);

        let status = model.status(&tab(1)).unwrap();
        assert_eq!(status.state, None);
        assert_eq!(status.revision, 2, "went through the real revision-minting apply path");

        let transitions = model.drain_transitions();
        assert_eq!(
            transitions,
            vec![Transition {
                pane: tab(1),
                old_state: Some(AgentState::Working),
                new_state: None,
                name: None,
            }],
            "the inferred clear enqueued exactly the transition an explicit Clear would"
        );
    }

    // ── AC-2: Set -> live A only (no D) leaves state unchanged ────────────

    #[test]
    fn ac2_set_then_live_a_without_d_leaves_state_unchanged() {
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(1, set_event(AgentState::Working));
        model.record_latch_set(1);
        model.drain_transitions();

        model.record_live_prompt_mark(1, PromptMarkKind::PromptStart);

        assert_eq!(model.status(&tab(1)).unwrap().state, Some(AgentState::Working));
        assert!(model.drain_transitions().is_empty());
    }

    // ── AC-3: explicit Clear -> live D/A produces no duplicate clear ──────

    #[test]
    fn ac3_explicit_clear_then_live_d_a_does_not_duplicate_clear() {
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(1, set_event(AgentState::Working));
        model.record_latch_set(1);
        model.drain_transitions();

        model.apply_plain_tab_event(1, AgentStatusEvent::Clear);
        model.record_latch_clear(1);
        let explicit_clear_transitions = model.drain_transitions();
        assert_eq!(explicit_clear_transitions.len(), 1, "the explicit clear itself");
        let revision_after_explicit_clear = model.status(&tab(1)).unwrap().revision;

        model.record_live_prompt_mark(1, PromptMarkKind::CommandEnd);
        model.record_live_prompt_mark(1, PromptMarkKind::PromptStart);

        let status = model.status(&tab(1)).unwrap();
        assert_eq!(status.state, None);
        // Revision is the discriminator: `apply_plain_tab_event` already
        // dedupes a same-state re-report's TRANSITION even without the
        // latch's own disarm, so an empty transition queue alone would not
        // prove the latch stayed disarmed. Revision still advances on every
        // `apply_plain_tab_event` call regardless of whether the state
        // changed, so it catches a disarmed-latch bug that a
        // transition-only assertion would miss.
        assert_eq!(
            status.revision, revision_after_explicit_clear,
            "the disarmed latch must not apply a second Clear at all"
        );
        assert!(
            model.drain_transitions().is_empty(),
            "no second/duplicate clear transition from the disarmed latch"
        );
    }

    // ── AC-4: marks from a snapshot/replay-equivalent scenario never fire ─

    #[test]
    fn ac4_reconcile_latch_feed_drops_replay_equivalent_candidates() {
        // A "scenario equivalent to snapshot/replay": the candidate never
        // reached `take_prompt_marks()`'s live-mark output at all (replay
        // bypasses `on_osc` entirely — see `LatchFeedEvent`'s doc), so
        // `live_marks` here is empty even though candidates exist.
        let feed = vec![
            LatchFeedEvent::Set,
            LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd),
            LatchFeedEvent::PromptMark(PromptMarkKind::PromptStart),
        ];
        let resolved = reconcile_latch_feed(feed, &[]);
        assert_eq!(resolved, vec![ResolvedLatchInput::Set]);

        // Feeding the resolved (Mark-free) sequence to the model: no fire.
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(1, set_event(AgentState::Working));
        for input in resolved {
            apply_resolved(&mut model, 1, input);
        }
        assert_eq!(model.status(&tab(1)).unwrap().state, Some(AgentState::Working));
    }

    // ── AC-5: marks captured on the alternate screen never fire ───────────

    #[test]
    fn ac5_reconcile_latch_feed_drops_alt_screen_suppressed_candidates() {
        // Two D candidates observed by `on_osc` (fires unconditionally),
        // but `take_prompt_marks()` (alt-screen-gated) only ever captured
        // the SECOND one — the first was suppressed while on the alt
        // screen. `live_marks` reflects that: only one CommandEnd.
        let feed = vec![
            LatchFeedEvent::Set,
            LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd), // alt-screen: suppressed
            LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd), // live
            LatchFeedEvent::PromptMark(PromptMarkKind::PromptStart), // live
        ];
        let live_marks = [PromptMarkKind::CommandEnd, PromptMarkKind::PromptStart];
        let resolved = reconcile_latch_feed(feed, &live_marks);
        assert_eq!(
            resolved,
            vec![
                ResolvedLatchInput::Set,
                ResolvedLatchInput::Mark(PromptMarkKind::CommandEnd),
                ResolvedLatchInput::Mark(PromptMarkKind::PromptStart),
            ],
            "the alt-screen-suppressed D candidate is dropped, not the live pair"
        );

        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(1, set_event(AgentState::Working));
        for input in resolved {
            apply_resolved(&mut model, 1, input);
        }
        assert_eq!(
            model.status(&tab(1)).unwrap().state,
            None,
            "the live D/A pair still fires normally"
        );
    }

    #[test]
    fn ac5_alt_screen_suppressed_d_a_pair_never_reaches_the_model() {
        // The alt-screen scenario that must NOT fire: a D/A pair observed
        // by `on_osc` while on the alt screen never appears in
        // `take_prompt_marks()`'s live output at all.
        let feed = vec![
            LatchFeedEvent::Set,
            LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd),
            LatchFeedEvent::PromptMark(PromptMarkKind::PromptStart),
        ];
        let resolved = reconcile_latch_feed(feed, &[]); // nothing was live
        assert_eq!(resolved, vec![ResolvedLatchInput::Set]);

        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(1, set_event(AgentState::Working));
        for input in resolved {
            apply_resolved(&mut model, 1, input);
        }
        assert_eq!(model.status(&tab(1)).unwrap().state, Some(AgentState::Working));
    }

    // ── AC-6: closing a tab discards its latch instance too ───────────────

    #[test]
    fn ac6_discard_removes_latch_so_a_later_mark_cannot_resurrect_state() {
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(1, set_event(AgentState::Working));
        model.record_latch_set(1);
        model.drain_transitions();

        model.discard(&tab(1));
        assert!(model.status(&tab(1)).is_none());

        // A stray D/A pair for the now-closed tab id creates a FRESH
        // (unarmed) latch and must not resurrect an entry or fire.
        model.record_live_prompt_mark(1, PromptMarkKind::CommandEnd);
        model.record_live_prompt_mark(1, PromptMarkKind::PromptStart);
        assert!(model.status(&tab(1)).is_none());
        assert!(model.drain_transitions().is_empty());
    }

    // ── AC-7: no OSC 133 ever -> Set persists indefinitely (regression) ───

    #[test]
    fn ac7_tab_without_osc133_support_never_auto_clears() {
        let mut model = AgentStatusModel::new();
        model.apply_plain_tab_event(1, set_event(AgentState::Working));
        model.record_latch_set(1);
        model.drain_transitions();

        // No live marks ever arrive for this tab (shell has no OSC 133
        // integration) — the icon must stay exactly as reported.
        assert_eq!(model.status(&tab(1)).unwrap().state, Some(AgentState::Working));
        assert!(model.drain_transitions().is_empty());
    }

    // ── reconcile_latch_feed: ordering + pass-through behavior ────────────

    #[test]
    fn reconcile_latch_feed_preserves_true_relative_order() {
        let feed = vec![
            LatchFeedEvent::Set,
            LatchFeedEvent::PromptMark(PromptMarkKind::CommandEnd),
            LatchFeedEvent::PromptMark(PromptMarkKind::PromptStart),
            LatchFeedEvent::Clear,
        ];
        let live_marks = [PromptMarkKind::CommandEnd, PromptMarkKind::PromptStart];
        let resolved = reconcile_latch_feed(feed, &live_marks);
        assert_eq!(
            resolved,
            vec![
                ResolvedLatchInput::Set,
                ResolvedLatchInput::Mark(PromptMarkKind::CommandEnd),
                ResolvedLatchInput::Mark(PromptMarkKind::PromptStart),
                ResolvedLatchInput::Clear,
            ]
        );
    }

    #[test]
    fn reconcile_latch_feed_empty_feed_yields_empty_resolved() {
        assert_eq!(reconcile_latch_feed(vec![], &[]), vec![]);
    }

    /// Test helper mirroring what `App::pump_all` does with a
    /// [`ResolvedLatchInput`] drained from a tab (see `tabs.rs`'s
    /// `pending_latch_inputs` / `app.rs`'s consumer loop).
    fn apply_resolved(model: &mut AgentStatusModel, tab_stable_id: u64, input: ResolvedLatchInput) {
        match input {
            ResolvedLatchInput::Set => model.record_latch_set(tab_stable_id),
            ResolvedLatchInput::Clear => model.record_latch_clear(tab_stable_id),
            ResolvedLatchInput::Mark(kind) => model.record_live_prompt_mark(tab_stable_id, kind),
        }
    }
}
