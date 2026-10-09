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
//! - `aggregate` ranks by `blocked > unseen error > unseen done > working >
//!   seen error > seen done > idle` (osc7501-program-status D2; `error` has
//!   the same unseen semantics as `done`).
//! - Composite state (osc7501-program-status SC-4): each entry stores the
//!   OSC 777 part (state, name) and the OSC 7501 summary (state, sanitized
//!   title, effective app) separately. Everything the model reports for an
//!   entry — status, aggregate, counts, any-reported-state, unseen
//!   tracking, transitions — uses the SC-2 composite
//!   ([`crate::agent_status::compose`]) of the two. An entry whose only
//!   input is OSC 777 behaves exactly as before.

use std::collections::{HashMap, VecDeque};

use crate::agent_status::{AgentState, AgentStatusEvent};
use crate::agent_status_exit_latch::AgentStatusExitLatch;

/// The GUI-local mux connection a pane belongs to
/// (`doc/tasks/mux-agent-status-pane-key-collision/IMPLEMENTATION.md`'s
/// "Connection scope value" shared component). Two panes that carry the
/// same wire `pane_id` on two different mux daemons must never collapse
/// onto one [`AgentStatusModel`] entry — the scope is what tells them
/// apart.
///
/// Currently equal to the owning [`crate::tabs::Tab`]'s `stable_id`
/// (IMPLEMENTATION.md D2: available from attach time, before the daemon's
/// first status update, unlike the daemon-minted `public_pane_id`), but
/// named after the *connection* it identifies rather than the tab that
/// currently supplies it, so the name survives if a dedicated mux client
/// object later replaces the tab as the connection owner. Never
/// transmitted on the wire; never rendered to the user.
///
/// The scope VALUE is constant for a given tab: every derivation site
/// computes it from the tab's own `stable_id`, unaffected by detach or
/// re-attach. The ENTRIES it keys are not similarly persistent —
/// mux-detach-agent-status-cleanup task0001 (D1/D3): a daemon-confirmed
/// detach releases every entry keyed by this scope (the model entry, the
/// scoped public-pane-id mapping and the notification rate-limit
/// identity, per pane), and a later re-attach re-mints them from the new
/// connection's first report. A doc comment that once read "constant for
/// the tab's whole lifetime, including across detach and re-attach" was
/// corrected here: that phrasing described the scope value, but read as a
/// promise about the entries it keys, which the implementation never
/// provided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionScope(pub u64);

/// Identifies one agent-status-bearing entity tracked by the model.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PaneKey {
    /// A plain (non-mux) tab, keyed by its stable identity
    /// (`Tab::stable_id`), which survives close-driven index shifts.
    Tab(u64),
    /// A mux pane, keyed by the pair (connection scope, wire `pane_id`) —
    /// the wire `pane_id` (the same id carried by `MuxMessage::pane_id` /
    /// `MuxWindowGroup::pane_ids`, not the API-facing `public_pane_id`
    /// string) alone is NOT unique: two different mux daemons attached
    /// from two different tabs can both mint pane 1. The
    /// [`ConnectionScope`] disambiguates them (SPEC
    /// mux-agent-status-pane-key-collision FR1).
    MuxPane(ConnectionScope, u32),
}

/// A pane's OSC 7501 (Program Status Protocol) summary as the model stores
/// it: the aggregate state, the deciding record's title (already sanitized
/// by the sender, used as received) and its effective app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatusSummary {
    pub state: AgentState,
    pub title: Option<String>,
    pub app: Option<String>,
}

/// One pane's tracked agent status.
///
/// `state` and `name` are the COMPOSITE the rest of the app reads: `state`
/// is [`crate::agent_status::compose`] of the OSC 777 part and the OSC 7501
/// summary, and `name` follows the D5 name selection (see
/// [`composite_name`]). `osc777_state` / `osc777_name` / `summary` are the
/// stored inputs they were derived from.
///
/// `state: None` means the pane has no current status (never reported, or
/// most recently cleared on every source) — such entries are excluded from
/// [`aggregate`] and [`counts`] but still occupy a slot (revision keeps
/// advancing) until [`AgentStatusModel::discard`] removes them on tab/pane
/// close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStatus {
    pub state: Option<AgentState>,
    pub name: Option<String>,
    pub revision: u64,
    pub unseen: bool,
    /// The OSC 777 part: state as last reported (`None` when never
    /// reported or cleared).
    pub osc777_state: Option<AgentState>,
    /// The OSC 777 part: the sanitized name that came with that report.
    pub osc777_name: Option<String>,
    /// The OSC 7501 part (`None` when the pane has no records).
    pub summary: Option<ProgramStatusSummary>,
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
    pub error: u32,
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

    /// Apply a plain-tab OSC 777 event (already parsed by
    /// `crate::agent_status::parse`). The model mints the revision — plain
    /// tabs are never targeted by the mux agent API, so nothing else needs
    /// revision continuity with a daemon-side counter. The composite is
    /// recomputed with the tab's stored OSC 7501 summary.
    pub fn apply_plain_tab_event(&mut self, tab_stable_id: u64, event: AgentStatusEvent) {
        let (new_state, name) = match event {
            AgentStatusEvent::Set { state, name } => (Some(state), name),
            AgentStatusEvent::Clear => (None, None),
        };
        let key = PaneKey::Tab(tab_stable_id);
        let (next_revision, summary) = self.minted_revision_and_summary(&key);
        self.apply_report(key, new_state, name, summary, next_revision, false);
    }

    /// Set (or, with `None`, remove) a plain tab's OSC 7501 summary.
    /// Stores the summary, advances the model-minted revision, recomputes
    /// the composite with the tab's stored OSC 777 part and applies the
    /// same unseen / transition rules as every other update.
    pub fn apply_plain_tab_summary(
        &mut self,
        tab_stable_id: u64,
        summary: Option<ProgramStatusSummary>,
    ) {
        let key = PaneKey::Tab(tab_stable_id);
        let next_revision = self.entries.get(&key).map_or(1, |e| e.revision + 1);
        let (osc777_state, osc777_name) = self
            .entries
            .get(&key)
            .map_or((None, None), |e| (e.osc777_state, e.osc777_name.clone()));
        self.apply_report(
            key,
            osc777_state,
            osc777_name,
            summary,
            next_revision,
            false,
        );
    }

    /// The next model-minted revision for `key` and the entry's stored
    /// OSC 7501 summary (both are what an OSC 777 report must keep).
    fn minted_revision_and_summary(&self, key: &PaneKey) -> (u64, Option<ProgramStatusSummary>) {
        match self.entries.get(key) {
            Some(e) => (e.revision + 1, e.summary.clone()),
            None => (1, None),
        }
    }

    /// Apply a daemon-pushed `AgentStatusUpdate` for a mux pane that
    /// carries NO OSC 7501 summary (the existing signature keeps meaning
    /// "no summary"; see [`Self::apply_daemon_update_with_summary`]).
    /// `scope` identifies the connection that delivered the update (the tab
    /// whose mux attach carried it); `revision` is the daemon-authoritative
    /// value and is stored verbatim (the model never increments it itself
    /// for mux panes).
    pub fn apply_daemon_update(
        &mut self,
        scope: ConnectionScope,
        pane_id: u32,
        state: Option<AgentState>,
        name: Option<String>,
        revision: u64,
        replay_derived: bool,
    ) {
        self.apply_daemon_update_with_summary(
            scope,
            pane_id,
            state,
            name,
            revision,
            None,
            replay_derived,
        );
    }

    /// Apply a daemon-pushed `AgentStatusUpdate` for a mux pane together
    /// with the pane's OSC 7501 `summary` (`None` when the pane has no
    /// records). Both parts are stored verbatim with the daemon `revision`
    /// and the composite is recomputed from them.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_daemon_update_with_summary(
        &mut self,
        scope: ConnectionScope,
        pane_id: u32,
        state: Option<AgentState>,
        name: Option<String>,
        revision: u64,
        summary: Option<ProgramStatusSummary>,
        replay_derived: bool,
    ) {
        self.apply_report(
            PaneKey::MuxPane(scope, pane_id),
            state,
            name,
            summary,
            revision,
            replay_derived,
        );
    }

    /// Shared apply path for every ingestion source. `osc777_state` /
    /// `osc777_name` / `summary` are the entry's complete new inputs; the
    /// reported composite is derived from them here.
    ///
    /// - The "unseen" flag is reset to `true` on any real change of the
    ///   COMPOSITE state (including the pane's very first report) and
    ///   otherwise left untouched — regardless of `replay_derived`.
    /// - A transition is enqueued only for a real composite change AND
    ///   `!replay_derived`; its name follows D5 ([`composite_name`]).
    fn apply_report(
        &mut self,
        key: PaneKey,
        osc777_state: Option<AgentState>,
        osc777_name: Option<String>,
        summary: Option<ProgramStatusSummary>,
        revision: u64,
        replay_derived: bool,
    ) {
        let entry_existed = self.entries.contains_key(&key);
        let prev_state = self.entries.get(&key).and_then(|e| e.state);
        let new_state =
            crate::agent_status::compose(osc777_state, summary.as_ref().map(|s| s.state));
        let name = composite_name(
            new_state,
            osc777_state,
            osc777_name.as_deref(),
            summary.as_ref(),
        );
        let state_changed = !entry_existed || prev_state != new_state;

        let entry = self
            .entries
            .entry(key.clone())
            .or_insert_with(|| AgentStatus {
                state: None,
                name: None,
                revision: 0,
                unseen: false,
                osc777_state: None,
                osc777_name: None,
                summary: None,
            });
        entry.state = new_state;
        entry.name = name.clone();
        entry.revision = revision;
        entry.osc777_state = osc777_state;
        entry.osc777_name = osc777_name;
        entry.summary = summary;
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

    /// Whether any of the given mux pane ids, within `scope`, currently
    /// carries a reported (uncleared) agent status — one of Idle / Working
    /// / Blocked / Done / Error (the composite). Cleared (`state: None`)
    /// and never-reported (no tracked entry) panes do not count; a
    /// same-numbered pane in a DIFFERENT scope never qualifies (SPEC
    /// mux-agent-status-pane-key-collision FR2). Used by the
    /// `next-agent-window` mux action (SPEC mux-agent-tab-cycle FR6) to
    /// decide whether a mux window qualifies for the cycle: a window
    /// qualifies when at least one of its panes qualifies (existential),
    /// per IMPLEMENTATION.md's any-reported-state assumption.
    pub fn any_pane_has_reported_state<'a, I>(&self, scope: ConnectionScope, pane_ids: I) -> bool
    where
        I: IntoIterator<Item = &'a u32>,
    {
        pane_ids.into_iter().any(|pid| {
            self.entries
                .get(&PaneKey::MuxPane(scope, *pid))
                .is_some_and(|e| e.state.is_some())
        })
    }

    /// Highest-priority state + that state's actual unseen flag among
    /// `panes`, ranked `blocked > unseen-error > unseen-done > working >
    /// seen-error > seen-done > idle` (each pane contributes its composite
    /// state).
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
                Some(AgentState::Error) => counts.error += 1,
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
/// `blocked(6) > unseen-error(5) > unseen-done(4) > working(3) >
/// seen-error(2) > seen-done(1) > idle(0)`. `error` shares `done`'s unseen
/// semantics (FR11).
fn priority_rank(state: AgentState, unseen: bool) -> u8 {
    match (state, unseen) {
        (AgentState::Blocked, _) => 6,
        (AgentState::Error, true) => 5,
        (AgentState::Done, true) => 4,
        (AgentState::Working, _) => 3,
        (AgentState::Error, false) => 2,
        (AgentState::Done, false) => 1,
        (AgentState::Idle, _) => 0,
    }
}

/// The name of an entry's composite (osc7501-program-status D5).
///
/// When the OSC 7501 summary's state ranks at or above the OSC 777 state by
/// [`AgentState::compose_rank`] (ties go to OSC 7501; an absent OSC 777
/// state ranks below everything), the name is the summary's title, else its
/// app, else the OSC 777 name, else none. Otherwise — or without a summary —
/// the name is the OSC 777 name, else none. A pane with no composite state
/// (`composite` is `None`) has no name. "None" falls back to the existing
/// default name at the notification layer.
fn composite_name(
    composite: Option<AgentState>,
    osc777_state: Option<AgentState>,
    osc777_name: Option<&str>,
    summary: Option<&ProgramStatusSummary>,
) -> Option<String> {
    composite?;
    let osc777_name = osc777_name.map(str::to_string);
    let Some(summary) = summary else {
        return osc777_name;
    };
    let summary_wins =
        osc777_state.is_none_or(|s| summary.state.compose_rank() >= s.compose_rank());
    if summary_wins {
        summary
            .title
            .clone()
            .or_else(|| summary.app.clone())
            .or(osc777_name)
    } else {
        osc777_name
    }
}

/// Convert the wire-level `mux_ipc::protocol::AgentState` mirror into the
/// core `crate::agent_status::AgentState` the model stores. Both enums
/// share the same five variants by contract (SPEC FR1 / FR11 / `mux_ipc`'s
/// "local mirror" doc comment); this is a straight, total mapping.
pub fn state_from_wire(state: mux_ipc::protocol::AgentState) -> AgentState {
    match state {
        mux_ipc::protocol::AgentState::Idle => AgentState::Idle,
        mux_ipc::protocol::AgentState::Working => AgentState::Working,
        mux_ipc::protocol::AgentState::Blocked => AgentState::Blocked,
        mux_ipc::protocol::AgentState::Done => AgentState::Done,
        mux_ipc::protocol::AgentState::Error => AgentState::Error,
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
        AgentState::Error => mux_ipc::protocol::AgentState::Error,
    }
}

#[cfg(test)]
mod tests;
