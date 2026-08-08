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
        let fire = self
            .latches
            .entry(tab_stable_id)
            .or_default()
            .record_mark(kind);
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
mod tests;
