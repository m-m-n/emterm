//! Build-agnostic per-pane "inferred-clear" latch implementing SPEC.md FR1:
//! decides when a stale agent-status icon should be inferred-cleared from an
//! OSC 133 `D` (command end) → `A` (prompt start) transition observed after
//! the pane's agent-status was last `Set`.
//!
//! Compiled WITHOUT the `gui` feature (CLI-shared, mirrors
//! [`crate::agent_status`]): both the GUI plain-tab path and the mux daemon
//! pane path depend on this module without pulling in GUI-only crates.
//!
//! One [`AgentStatusExitLatch`] instance covers one pane/tab's lifecycle.
//! It has no knowledge of `agent_status`/UI/revision state, no I/O, and no
//! fallible operations — a live mark that doesn't match any transition is
//! simply a no-op. The latch does not itself decide whether a mark is
//! "live" vs. replay-derived or main-screen vs. alt-screen; the caller
//! (task0002 GUI wiring / task0003 mux daemon wiring) is responsible for
//! only ever handing it live, main-screen-observed marks, in true arrival
//! order alongside that pane's `Set`/`Clear` calls (SPEC.md FR4, FR5).

use crate::prompts::PromptMarkKind;

/// Per-pane state machine deciding when an inferred `Clear` should fire from
/// a live OSC 133 `D`→`A` transition observed after the pane's agent-status
/// was last `Set` (SPEC.md FR1).
///
/// State is exactly: whether the latch is armed (a `Set` has been recorded
/// and not yet cleared), whether the current generation's command has
/// ended (a `D` mark was seen while armed), and a generation counter that
/// invalidates any `D` recorded before the most recent `Set`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentStatusExitLatch {
    armed: bool,
    command_ended: bool,
    generation: u64,
}

impl AgentStatusExitLatch {
    /// A fresh, disarmed latch (no `Set` recorded yet).
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an explicit agent-status `Set` report for this pane.
    ///
    /// Arms the latch and starts a fresh generation, so a `D` observed
    /// against the OLD generation can never combine with an `A` observed
    /// against the NEW generation. Idempotent under repeated calls — each
    /// call simply re-arms with a new generation, discarding any
    /// `command_ended` state accumulated under the previous generation.
    pub fn record_set(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.armed = true;
        self.command_ended = false;
    }

    /// Record an explicit agent-status `Clear` report for this pane.
    ///
    /// Disarms the latch. Never itself produces an inferred-clear signal —
    /// the caller is already applying the explicit clear it just recorded.
    pub fn record_clear(&mut self) {
        self.armed = false;
        self.command_ended = false;
    }

    /// Record a live OSC 133 mark for this pane (pre: LIVE, main-screen-
    /// observed, in true arrival order — see the caller contract in the
    /// module docs). Only `CommandEnd` (`D`) and `PromptStart` (`A`) are
    /// meaningful; every other kind is a no-op. Returns `true` exactly when
    /// this call should fire an inferred `Clear` — i.e. a live `A` arrived
    /// while armed and the current generation's command had already ended.
    /// Firing resets the latch to the same terminal state as an explicit
    /// `Clear`.
    pub fn record_mark(&mut self, kind: PromptMarkKind) -> bool {
        match kind {
            PromptMarkKind::CommandEnd => {
                if self.armed && !self.command_ended {
                    self.command_ended = true;
                }
                false
            }
            PromptMarkKind::PromptStart => {
                if self.armed && self.command_ended {
                    self.armed = false;
                    self.command_ended = false;
                    true
                } else {
                    false
                }
            }
            PromptMarkKind::CommandStart | PromptMarkKind::CommandExec => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AC-1 / TS-1: Set -> live D -> live A fires exactly once ─────────

    #[test]
    fn set_then_d_then_a_fires_exactly_once() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd));
        assert!(latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── AC-2 / TS-2: Set -> live A (no prior D) -> never signals ────────

    #[test]
    fn set_then_a_without_prior_d_never_signals() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(PromptMarkKind::PromptStart));
        // Repeating the same A again still never signals.
        assert!(!latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── AC-3 / TS-3: Set -> explicit Clear -> live D -> live A -> no signal

    #[test]
    fn explicit_clear_suppresses_subsequent_d_and_a() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        latch.record_clear();
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd));
        assert!(!latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── AC-4 / TS-4: Set -> live D -> Set again (new generation) -> live A
    //    -> no signal (the D belonged to the invalidated generation) ─────

    #[test]
    fn re_set_invalidates_the_prior_generations_d() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd));
        latch.record_set(); // re-arm: new generation, command_ended reset
        assert!(!latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── AC-5 / TS-5: no Set ever recorded -> live D -> live A -> no signal

    #[test]
    fn no_set_ever_recorded_never_signals() {
        let mut latch = AgentStatusExitLatch::new();
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd));
        assert!(!latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── AC-6 / TS-12: Set -> live D -> live D again (repeated) -> live A
    //    -> exactly one signal (repeated D does not break the machine) ───

    #[test]
    fn repeated_d_before_matching_a_still_fires_exactly_once() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd));
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd)); // repeated D: no-op
        assert!(latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── AC-7: after firing, the latch is observably equivalent to having
    //    just received an explicit Clear ─────────────────────────────────

    #[test]
    fn firing_leaves_latch_equivalent_to_post_clear_state() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        latch.record_mark(PromptMarkKind::CommandEnd);
        assert!(latch.record_mark(PromptMarkKind::PromptStart)); // fires

        // A subsequent D/A pair with no intervening Set produces no signal,
        // exactly as it would right after an explicit Clear.
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd));
        assert!(!latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── Full transition table: kinds other than D/A are always no-ops ───

    #[test]
    fn non_d_a_kinds_are_always_no_ops_regardless_of_state() {
        let mut latch = AgentStatusExitLatch::new();
        // Disarmed.
        assert!(!latch.record_mark(PromptMarkKind::PromptStart)); // no-op (not armed)

        latch.record_set();
        // Armed, command not ended.
        assert!(!latch.record_mark(PromptMarkKind::CommandStart));
        assert!(!latch.record_mark(PromptMarkKind::CommandExec));

        latch.record_mark(PromptMarkKind::CommandEnd);
        // Armed, command ended.
        assert!(!latch.record_mark(PromptMarkKind::CommandStart));
        assert!(!latch.record_mark(PromptMarkKind::CommandExec));

        // The pending D->A transition still fires after the no-op kinds.
        assert!(latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── Design note: repeated Set before any D still behaves normally ───

    #[test]
    fn repeated_set_before_d_still_arms_correctly() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        latch.record_set(); // idempotent re-arm
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd));
        assert!(latch.record_mark(PromptMarkKind::PromptStart));
    }

    // ── D while not armed is a no-op (does not spuriously arm command_ended)

    #[test]
    fn d_while_not_armed_is_a_no_op() {
        let mut latch = AgentStatusExitLatch::new();
        assert!(!latch.record_mark(PromptMarkKind::CommandEnd));
        // Now Set and go straight to A: since the earlier D was a no-op
        // while unarmed, this A must not fire.
        latch.record_set();
        assert!(!latch.record_mark(PromptMarkKind::PromptStart));
    }
}
