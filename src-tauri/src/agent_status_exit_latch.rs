//! Core inferred-clear latch (agent-exit-after-icon SPEC FR1).
//!
//! A small, build-agnostic, side-agnostic per-pane state machine deciding
//! when a live OSC 133 `D` (command end) followed by `A` (prompt start),
//! observed after an agent `Set`, should be treated as an inferred
//! `Clear` — so a stale agent-status icon clears itself when the agent
//! exits (Ctrl+C / crash / `exit`) without ever reporting an explicit
//! `Clear`.
//!
//! Deviation note (task0002): this module and its `lib.rs` registration
//! are task0001's file scope (`doc/tasks`/`feature-docs` planning). At the
//! time task0002 was implemented, task0001 had not yet merged into the
//! parent branch, and task0002 has a hard code dependency on this type
//! (IMPLEMENTATION.md's "Inferred-clear latch" Shared Component). task0002
//! implemented it here, matching task0001.md's design/Acceptance Criteria
//! as closely as possible, so task0002 itself is implementable. If
//! task0001 lands with a different shape, the parent-side-adoption
//! protocol resolves the conflict at merge time.
//!
//! [`LatchMarkKind`] is a local, build-agnostic mirror of the two mark
//! kinds this latch inspects. It intentionally does NOT reuse
//! `crate::prompts::PromptMarkKind`: `prompts` is gated behind the `gui`
//! feature (GUI-only), but this latch must stay build-agnostic — per
//! IMPLEMENTATION.md's Layer Structure, both the GUI process and the
//! build-agnostic mux daemon depend on it without pulling in GUI-only
//! crates. Callers translate their own mark representation into this
//! enum at the call site.

/// OSC 133 semantic-prompt sub-type, as far as this latch cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatchMarkKind {
    /// `A` — prompt start.
    PromptStart,
    /// `B` — command start (user input begins). Never a transition input
    /// for this latch; included for a complete, callers-convert-1:1
    /// mirror of the underlying OSC 133 sub-types.
    CommandStart,
    /// `C` — command exec (user input ends, command runs). Same note as
    /// `CommandStart`.
    CommandExec,
    /// `D` — command end (exit-code-bearing).
    CommandEnd,
}

/// Per-pane inferred-clear latch state (SPEC FR1).
///
/// State is exactly `armed` / `command_ended` / `generation`; no timeout,
/// no text inspection (SPEC explicitly excludes both). No fallible
/// operations — an unrecognized transition is simply a no-op, never a
/// panic.
///
/// Caller contract (enforced by callers, not this type): only live,
/// main-screen-observed marks may be passed to [`Self::record_mark`], in
/// true arrival order alongside this pane's [`Self::record_set`] /
/// [`Self::record_clear`] calls (SPEC FR4, FR5). This latch itself never
/// touches agent-status/UI/revision state — callers apply an inferred-clear
/// signal (`record_mark` returning `true`) through the exact same code path
/// an explicit `Clear` already uses (SPEC FR2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentStatusExitLatch {
    armed: bool,
    command_ended: bool,
    generation: u64,
}

impl AgentStatusExitLatch {
    /// A fresh, disarmed latch (generation 0).
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an explicit `Set` report. Idempotent under repeated `Set`
    /// calls: each call starts a fresh generation, so a `D` observed
    /// against the OLD generation can never combine with an `A` observed
    /// against the NEW generation (SPEC FR1 step 5).
    pub fn record_set(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.armed = true;
        self.command_ended = false;
    }

    /// Record an explicit `Clear` report. Returns to disarmed and never
    /// itself produces an inferred-clear signal — that would double up
    /// with the explicit clear the caller is already applying.
    pub fn record_clear(&mut self) {
        self.armed = false;
        self.command_ended = false;
    }

    /// Record a live OSC 133 mark. Returns `true` exactly when an
    /// inferred clear should now be applied (exactly once per qualifying
    /// `D`→`A` pair), and resets to disarmed in that case — the same
    /// terminal state as an explicit [`Self::record_clear`].
    ///
    /// Transition table:
    /// - `CommandEnd` (`D`) while `armed && !command_ended`: latches
    ///   `command_ended`. Repeated `D`s before the matching `A` are
    ///   absorbed (state only, not a count) — never multiple fires.
    /// - `PromptStart` (`A`) while `armed && command_ended`: fires,
    ///   disarms.
    /// - Anything else (wrong state, or `CommandStart`/`CommandExec`):
    ///   no-op, no signal.
    pub fn record_mark(&mut self, kind: LatchMarkKind) -> bool {
        match kind {
            LatchMarkKind::CommandEnd if self.armed && !self.command_ended => {
                self.command_ended = true;
                false
            }
            LatchMarkKind::PromptStart if self.armed && self.command_ended => {
                self.armed = false;
                self.command_ended = false;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AC-1: Set -> D -> A fires exactly once ─────────────────────────

    #[test]
    fn set_then_d_then_a_fires_inferred_clear() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(LatchMarkKind::CommandEnd));
        assert!(latch.record_mark(LatchMarkKind::PromptStart));
    }

    // ── AC-2: Set -> A (no D) never fires ──────────────────────────────

    #[test]
    fn set_then_a_without_d_does_not_fire() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(LatchMarkKind::PromptStart));
    }

    // ── AC-3: Set -> explicit Clear -> D -> A never fires ──────────────

    #[test]
    fn explicit_clear_before_d_a_suppresses_inferred_clear() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        latch.record_clear();
        assert!(!latch.record_mark(LatchMarkKind::CommandEnd));
        assert!(!latch.record_mark(LatchMarkKind::PromptStart));
    }

    // ── AC-4: a fresh Set invalidates the prior generation's D ─────────

    #[test]
    fn d_from_invalidated_generation_never_combines_with_new_generation_a() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(LatchMarkKind::CommandEnd));
        latch.record_set(); // new generation; command_ended resets too
        assert!(!latch.record_mark(LatchMarkKind::PromptStart));
    }

    // ── AC-5: no Set ever recorded -> D -> A never fires ───────────────

    #[test]
    fn no_set_ever_recorded_never_fires() {
        let mut latch = AgentStatusExitLatch::new();
        assert!(!latch.record_mark(LatchMarkKind::CommandEnd));
        assert!(!latch.record_mark(LatchMarkKind::PromptStart));
    }

    // ── AC-6: repeated D before the matching A fires exactly once ─────

    #[test]
    fn repeated_d_before_matching_a_still_fires_exactly_once() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(LatchMarkKind::CommandEnd));
        assert!(!latch.record_mark(LatchMarkKind::CommandEnd));
        assert!(latch.record_mark(LatchMarkKind::PromptStart));
    }

    // ── AC-7: after firing, state mirrors an explicit Clear ────────────

    #[test]
    fn after_firing_a_subsequent_d_a_pair_produces_no_further_signal() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        latch.record_mark(LatchMarkKind::CommandEnd);
        assert!(latch.record_mark(LatchMarkKind::PromptStart));

        // No intervening Set: behaves as freshly cleared.
        assert!(!latch.record_mark(LatchMarkKind::CommandEnd));
        assert!(!latch.record_mark(LatchMarkKind::PromptStart));
    }

    // ── Non-A/D kinds are always no-ops ────────────────────────────────

    #[test]
    fn command_start_and_command_exec_are_always_no_ops() {
        let mut latch = AgentStatusExitLatch::new();
        latch.record_set();
        assert!(!latch.record_mark(LatchMarkKind::CommandStart));
        assert!(!latch.record_mark(LatchMarkKind::CommandExec));
        // Latch is still armed, not command_ended: a D/A pair still fires.
        assert!(!latch.record_mark(LatchMarkKind::CommandEnd));
        assert!(latch.record_mark(LatchMarkKind::PromptStart));
    }
}
