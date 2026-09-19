# Implementation Plan: mouse-reporting

## Overview

eMterm gains DEC private mouse-tracking (modes 1000 / 1002 / 1003) with the X10
and SGR (1006) report encodings, emitted to the active tab's PTY, behind the
existing egui chrome guards and a single Shift local override. The work splits
into one core-side mode-state change, one window-free pure decision/encoding
module, and one pointer-routing integration that wires the two together.

## Technology Stack

- **Language**: Rust (existing workspace: `crates/term_core` and the
  `src-tauri` binary crate). No TypeScript change (NFR2).
- **Key libraries**: winit (already present) supplies the platform-independent
  pointer button / scroll-delta / modifier state the routing layer reads; egui
  (already present) owns the chrome guards that keep precedence.
- **New dependencies**: none. No runtime crate and no test-framework crate is
  added (NFR5), so no new license enters the project and `project.license: MIT`
  is unconstrained by this feature. New-dependency license record for this
  feature: *(empty — nothing added)*.

## Layer Structure

| Layer | Responsibility | May depend on |
|---|---|---|
| L1 — core mode state (`term_core`) | Owns the DECSET 1000/1002/1003/1006 mode bits; answers "is this mode active?" | Nothing GUI-only (NFR3) |
| L2 — pure decision / encoding (`window_host`, gui-gated) | Button-code composition, coordinate biasing, X10 overflow decision, motion gate, cell-change filter, wheel-consumer choice | Plain values only — not L1 types, not winit types, not egui types (NFR6) |
| L3 — pointer routing (`window_host`, gui-gated) | Chrome guards, Shift override, reads L1 state, calls L2, emits through L4 | L1, L2, L4 |
| L4 — tab input transport (existing) | Writes bytes to the active tab's PTY (local or mux-attached remote pane) | — (unchanged by this feature) |

Allowed dependency direction is strictly L3 → {L1, L2, L4}. L2 never depends on
L1, L3 or L4; that is what makes every L2 unit exercisable without a winit
window, a GPU surface or a live PTY (NFR6, AC16). L1 never depends on anything
above it, which is what keeps the CLI-only build compiling (NFR3, AC14).

## Shared Components

Contracts are stated as behaviour (inputs → outputs, pre/postconditions). Each
is implemented by exactly one owning task and consumed against this table by the
others, since all tasks run in parallel from the same base commit (see decision
D3).

| Component | Responsibility | Contract (pre/postcondition) | Owner / Used by |
|---|---|---|---|
| **SC-1 Core mouse-mode bits** | Hold the four DEC private mode bits and expose them to the host | Four mode identifiers named for normal tracking (1000), button-event tracking (1002), any-event tracking (1003) and SGR encoding (1006), declared in the same module, in the same shape and with the same naming style as the existing alternate-scroll mode identifier, and readable through the existing mode-query accessor. **Pre**: a DEC private set/reset request names one of the four modes. **Post**: the corresponding bit is set (on set) or cleared (on reset); the handler returns the "no host-side action" action value; the mode-query accessor reports the new state; no existing mode's bit position is renumbered; mode 1005 keeps its existing TS-fallback arm and mode 1015 keeps falling through the unknown-mode arm | task0001 / task0003 |
| **SC-2 Button-code composition** | Turn an event description into the numeric button code both encodings carry | Inputs: event kind (press / release / motion / wheel-up / wheel-down), button identity (left / middle / right / none), encoding (x10 / sgr), and the ctrl and alt modifier flags. Output: one non-negative integer. **Post**: base is left 0, middle 1, right 2, none 3; a wheel-up event is base 64 and a wheel-down event is base 65 regardless of button identity; a release under x10 replaces the base with 3 while a release under sgr retains the press base; a motion event adds 32 to the base; ctrl adds 16; alt adds 8; no input combination ever contributes the value 4 (see D4) | task0002 / task0003 |
| **SC-3 Report encoder** | Turn a button code plus a cell coordinate into the emitted byte sequence | Inputs: the button code of SC-2, a 1-based column, a 1-based row, the encoding, and whether this is a release. Output: an optional byte sequence — absent means "emit nothing". **Pre**: column and row are 1-based and at least 1. **Post (x10)**: CSI (the two-byte ESC-left-bracket introducer) then the letter M then exactly three bytes, each of the button code, the column and the row biased by 32; absent when the column or the row exceeds 223, with no clamping and no partial sequence. **Post (sgr)**: CSI then a less-than sign, then the unbiased decimal button code, a semicolon, the decimal column, a semicolon, the decimal row, then the letter M for a press or motion and the letter m for a release; no coordinate limit | task0002 / task0003 |
| **SC-4 Cell-change filter** | Cap motion report volume at grid resolution | Holds the last reported cell. Operation "should report this cell": inputs a 1-based column and row; output a boolean. **Post**: true when no cell is cached or the cached cell differs, and the cache then holds the new cell; false otherwise, with the cache unchanged. A separate reset operation empties the cache so the next cell always reports. **Pre for reset**: called by the host whenever it observes that no tracking mode is active, and on active-tab change (D7) | task0002 / task0003 |
| **SC-5 Motion gate** | Decide whether a pointer motion is reportable at all | Inputs: the three tracking-mode flags (1000, 1002, 1003) and the set of currently held buttons. Output: an optional button identity to report with. **Post**: absent when no tracking mode is active; absent when only 1000 is active; absent when 1002 is the highest active tracking mode and no button is held; present with the held button when 1002 is active and at least one button is held; present always when 1003 is active, carrying the held button or the "none" identity when no button is held. When several buttons are held the lowest-numbered one is reported (D6). 1003 takes precedence over 1002, which takes precedence over 1000 | task0002 / task0003 |
| **SC-6 Wheel-consumer decision** | Choose exactly one of the three mutually exclusive wheel consumers | Inputs: tracking-active, shift-held, on-alternate-screen, the alternate-scroll mode bit and the alternate-scroll setting flag. Output: exactly one of report-to-application, translate-to-arrows, scroll-scrollback. **Post**: the decision branches on tracking-active FIRST, and the two branches share no rows. *Tracking-active branch*: scroll-scrollback when shift is held, report-to-application otherwise — translate-to-arrows is unreachable in this branch, so while an application is tracking the mouse no wheel notch ever produces arrow bytes. *Tracking-inactive branch*: today's matrix, reproduced unchanged and without consulting shift at all — translate-to-arrows when the pointer is on the alternate screen and both the alternate-scroll mode bit and the alternate-scroll setting are on, scroll-scrollback otherwise. A scroll-scrollback outcome on a screen with no scrollback to move is a no-op, which is the existing behaviour of that path and is not special-cased here (D5) | task0002 / task0003 |
| **SC-7 Report emission** | Get the encoded bytes onto the wire | Every emitted report is written through the active tab's existing PTY input write path — the same one the alternate-scroll arrow translation already uses — so a local PTY and a mux-attached remote pane receive reports identically. **Post**: no new channel, no new transport state; the scrollback offset is never touched for a reported wheel notch | task0003 / — |
| **SC-8 Grid ownership decision** | Answer once, for every pointer handler, whether an event belongs to the terminal grid | Inputs: the pointer position plus the plain geometry of each chrome region the host already hit-tests — the top strip, the bottom strip, the right-edge scrollbar overlay, the mux sidebar in whichever placement is active, the CSD edge-resize hot zone — and whether the profile selector is visible. Output: a boolean. **Post**: false for a position inside any of those regions and false unconditionally while the profile selector is visible; true only for a position over the grid with no region claiming it. The answer depends on POSITION ONLY — it is identical for a press and a release, for every button identity, for motion and for a wheel notch. **Pre**: evaluated before any reporting work on every emitting path, which on the motion path means before the SC-5 gate and the SC-4 filter, and on the wheel path means before the tracking-active read, so a suppressed event can neither emit nor mutate SC-4's cache. A false answer leaves the event's current local behaviour exactly as it is; it never changes the left-press-only gating of the existing local side effects | task0004 / task0003 |
| **SC-9 Gesture ownership record** | Keep a button gesture on the side that took its press | Holds, per button identity, which side took the press: the report path, the local path, or nothing. Operations: record an owner at press, read-and-clear the owner at release, and clear all owners. **Post**: a release is routed to the owner its press recorded, whatever the shift state at release time; a press SC-8 rejected records no owner, so its release routes nowhere new; recording is idempotent per button identity, and a second button pressed mid-gesture is owned independently. **Pre for clear-all**: called on the same two observations that reset SC-4 — the host observing that no tracking mode is active, and an active-tab change — so a mode cleared mid-gesture strands no record | task0004 / task0003 |
| **SC-10 Pointer decision sequence** | Answer, for one raw pointer event, what the host must do — as ONE value computed from plain inputs | One unit per pointer path (button, motion, wheel). Inputs, all plain values: the event kind (press / release / motion / wheel notch with its direction), the button identity, the pointer position together with the same chrome-region geometry and profile-selector visibility SC-8 takes, the ctrl / alt / shift flags, the three tracking-mode flags and the encoding flag as read at THIS event, an identifier for the currently active tab, the currently held buttons, and the current contents of the two records (SC-4's cached cell and SC-9's ownership record). Output: one enumerated outcome naming exactly one disposition plus the record updates it implies. **Post (shape)**: the dispositions are disjoint and each is named — emit a report (carrying the exact bytes AND the identifier of the tab the report is destined for), take one named local arm (begin a selection drag; complete a selection and publish it to PRIMARY; open a hovered link; paste PRIMARY; scroll the scrollback; translate to arrow bytes), or do nothing — so a test distinguishes "nothing happened" from "the local arm ran" without a window. **Post (ownership first)**: SC-8's answer is consulted first on all three sequences, ahead of SC-5, SC-4 and the tracking-active read, so a rejected event can neither emit nor update a record. **Post (release)**: a release's disposition follows SC-9's recorded owner and the tracking state, where tracking state means "at least one tracking mode is active" and not merely which encoding is selected; a reported release carries the tab identifier its PRESS recorded, never whichever tab is active at release time; a release with no recorded owner emits nothing. **Post (motion)**: while a gesture is owned by the local side, the motion disposition is local whatever the instantaneous shift flag has become. **Post (resets)**: every one of the three sequences carries the same two reset observations (no tracking mode active; the active tab differs from the one the records were built against) in its outcome, so a click or a wheel notch with no intervening motion resets exactly as a motion does. **Post (testability)**: constructible and callable with no winit window, no GPU surface and no live PTY | task0005 / task0006 |
| **SC-11 Outcome application** | Perform an SC-10 outcome against plain state and a byte destination | Inputs: an SC-10 outcome, the pair of records (SC-4's cache and SC-9's ownership record) as a plain value a test constructs directly, and a caller-supplied byte destination. **Post**: report bytes are appended to that destination paired with their target tab identifier — never written at the decision site; the outcome's record updates are applied exactly once; a do-nothing or local-arm outcome appends no bytes and leaves the cached cell exactly as it was. The named local arm is NOT performed here — it is returned to the caller to perform, which is what keeps this unit free of window, GPU surface and PTY. A separate clear-all operation empties the ownership record and the held-button record together, for the host to invoke on focus loss | task0005 / task0006 |

## Conventions

- **Naming**: the four new mode identifiers follow the naming style of the
  existing alternate-scroll mode identifier in the same module. The new
  window_host module is named for its subject (mouse report); its units are
  named for what they answer, not for the caller that asks.
- **Error handling**: this feature introduces no error type and no error code.
  Its single "no output" condition is the unencodable X10 coordinate (FR9),
  expressed as an absent optional value returned by SC-3. A suppressed report is
  never an error, never logged, and never partially emitted.
- **Logging**: no logging is added on the pointer hot path. Motion routing runs
  once per raw pointer-motion event on a path the project already optimised for
  CPU (NFR1), so a per-event log line is a regression by itself.
- **Tests**: inline test modules next to the code under test, with the
  subject-scenario-expectation function-naming style already dominant in the
  core crate. Each unit under test is constructed explicitly inside its own
  test; no shared global fixture, no new test crate, no E2E harness (NFR5).
- **Feature gating**: everything under the window_host module tree stays inside
  the existing gui-feature gate; the core crate change stays free of GUI-only
  crates (NFR3).
- **Portability**: no platform-conditional branch is introduced anywhere in this
  feature; encoding and routing derive only from winit's platform-independent
  button / scroll-delta / modifier state (NFR4).

## Cross-task Design Decisions

### D1 — A pure-value seam separates deciding from routing

Every decision this feature makes (what button code, what bytes, report or not,
which wheel consumer) is a function of plain values and lives in L2. L3 only
collects those values from winit/egui/core state, calls L2, and performs the
side effect. **Rationale**: NFR6 requires the decision logic to be testable with
no window and no PTY, and AC16 requires byte-exact tests; a seam anywhere else
forces a windowed test. **Affects**: task0002 (owns L2), task0003 (owns L3).

### D2 — Mode state lives in the core, never mirrored in the host

The host never keeps its own copy of the tracking or encoding mode state; it
reads SC-1 through the existing mode-query accessor on each event, exactly as
the alternate-scroll path already does. **Rationale**: a host-side mirror needs
invalidation on tab switch, mux reattach and snapshot replay — three paths this
feature would otherwise have to touch. **Affects**: task0001, task0003.

### D3 — Parallel-worktree compile dependency and file-overlap protocol

All tasks are implemented in parallel from the same base commit, so a consuming
task's worktree does not contain the producing task's file. A task that consumes
a component owned by another task therefore declares that component's file in
its own file list and, if the file is absent in its worktree, creates the
minimum needed to satisfy the contract in this document — never more, and never
a different contract. On merge into integration the **owning** task's version is
authoritative: the consuming task adopts the integration (parent) side for that
file. A consumer never alters a contract; a contract change is a plan deviation
to report, not a local decision. **Affects**: task0003 (consumer of SC-1 through
SC-6), task0001 and task0002 (owners).

### D4 — Modifier bits apply to every report kind, wheel included

The ctrl (16) and alt (8) bits are added to the composed button code for press,
release, motion and wheel reports alike. The shift bit (4) is never added by any
path, because FR7 consumes shift locally before an event can reach emission.
**Rationale**: FR6 states the rule over "the button code" without excepting the
wheel, and a wheel-with-ctrl report that silently drops the modifier is
indistinguishable from a bare wheel to the application. **Affects**: task0002
(composition), task0003 (supplies the modifier state).

### D5 — While a tracking mode is active, Shift+wheel is purely local

While any tracking mode is active, a shift-held wheel notch emits **nothing** to
the PTY — neither a mouse report nor alternate-scroll arrow bytes — and drives
eMterm's scrollback instead. When the active screen has no scrollback to move
that outcome is a no-op, which is what the scrollback path already does today
and is not special-cased. When **no** tracking mode is active, wheel behaviour is
exactly today's, including the alternate-scroll path, which does not consult
Shift; that half is outside this feature's scope and is unchanged (AC7).

**Rationale**: Shift is FR7's single local override, and its other three
consequences — drag selects text, Ctrl+click opens a link, middle-click pastes
PRIMARY — all emit zero PTY bytes. Routing Shift+wheel to the arrow translation
would make it the only one of the four that still feeds the application, which
is not what a local override means. AC9 states this option literally
("Shift+wheel produces no report and scrolls eMterm's scrollback"); FR7's
"existing local handling" is the vaguer of the two phrases, and reading it as
the scrollback scroll removes the apparent collision without contradicting
anything in SPEC.md.

**Structural consequence**: SC-6 branches on tracking-active first, so the two
matrices never merge into a single table needing a tie-break. The collision is
removed by structure, not arbitrated.

**Provenance**: resolved by second-opinion consultation rather than by planner
judgment, and adopted at the coordinator's direction. The supporting
terminal-convention half of that verdict — that Shift+wheel conventionally means
terminal scrollback rather than arrow bytes, and that a shift-shaped bypass
modifier for mouse reporting is the established idiom — is **documented
convention reported at medium-high confidence, not a primary-source citation**;
the decision rests on the AC9/FR7 reading above, with the convention as
corroboration only. Reversing this decision is a change to one branch of SC-6
and its tests.

**Affects**: task0002 (SC-6 and its tests), task0003 (the wheel call site).

### D6 — A multi-button motion report carries the lowest-numbered held button

When more than one button is held during a motion report under 1002 or 1003,
the reported base is the lowest-numbered held button (left before middle before
right). **Rationale**: SPEC fixes the single-button cases only; a deterministic
tie-break is required for the gate to be a pure function. Confirmed by the same
second-opinion consultation as D5 (evidence strength: medium): it matches the
held-button-mask derivation conventionally used for this protocol, it is
order-independent and therefore expressible as a pure function, and it keeps a
left-drag reported as left when another button is incidentally pressed
mid-drag. Recorded as open question OQ-3 because SPEC.md still does not state
it; reversible. **Affects**: task0002 (SC-5), task0003.

### D7 — The cell-change cache is host state with two reset points

The last-reported cell is cached by the host for the lifetime of a tracking
session (NFR1). It is reset when the host observes that no tracking mode is
active, and on active-tab change, so the first motion of a new tracking session
always reports and a cell cached against one tab never suppresses a report for
another. **Affects**: task0002 (SC-4 provides the reset operation), task0003
(owns calling it).

### D8 — No new dependency, so no license question arises

The feature is implemented entirely with crates already in the workspace, and
NFR5 forbids adding a test-framework crate. No dependency is added, so there is
no new license to record against `project.license: MIT` and no compatibility
check to perform. **Affects**: all tasks.

### D9 — Modes 1005 and 1015 are left exactly as they are

Mode 1005 keeps its existing TS-fallback arm and mode 1015 is deliberately given
no arm, so it continues to fall through the unknown-mode arm as a silent no-op.
Neither is given a bit, a test, or a host-side consumer. **Affects**: task0001.

### D10 — Shift is applied once per gesture, at the press (supersedes the per-raw-event reading)

A button press and its matching release are one gesture. The Shift override
(FR7) is consulted when the gesture starts, the side that takes the press owns
the gesture (SC-9), and the matching release goes to that same side whatever
the shift state has become by then.

**Rationale**: AC9 requires that, with a tracking mode active, Shift+drag
"produces a text selection". A selection is only produced when the release
completes it and updates PRIMARY, so a reading that re-asks the Shift question
at release time fails AC9 the moment Shift is lifted mid-drag — and, in the
mirror ordering, strands the application's button state down when Shift arrives
mid-drag. Per-gesture ownership is the reading under which AC9, FR2's
press-and-*matching*-release pairing and FR7 hold simultaneously. FR7's
sole-override rule is untouched: Shift remains the only thing that diverts a
gesture from reporting, and no second suppression condition is introduced on
the release path. AC10 is untouched: the shift bit is still never set in any
emitted report, including a release emitted while Shift happens to be held.

**Supersedes**: task0003's plan recorded the opposite reading ("each event is
decided on its own … no state carries the drag's initial Shift decision
forward"). That reading is what review round 1 finding 97676659aa15db37
reports as a defect; this decision replaces it. task0003's plan is left as the
record of the work at the time and is not rewritten.

**History**: a bounded auto-fix during review (b1244629, reverted at ae47aa35)
expressed this as an extra suppression condition on a non-Shift release. That
form covered one ordering only and reads as a second local override alongside
Shift, which is why ownership is recorded at press time instead.

**Affects**: task0004 (owns SC-9), task0003 (the routing call sites this
supersedes).

### D11 — One grid ownership test, ahead of every emitting path

The question "does this pointer event belong to the terminal grid?" is answered
once, by SC-8, from position alone, and is consulted by the button, motion and
wheel handlers alike before any reporting work.

**Rationale**: FR10 and AC12 state the guard precedence unconditionally, but
the guards were implemented per handler and, within the button handler, partly
per button identity — while the reporting path accepts every identity the
feature can encode and the pixel-to-cell mapping clamps an out-of-grid position
to an edge cell rather than rejecting it. Three separate answers to one
question is what produced review round 1 findings a4377ac1f3430e7c,
5175da13ae0becec and c99335940bf23416; one answer is what makes FR10 hold on
every path at once, including the paths that never had a region test at all.

**Consequence for NFR1**: SC-8 is evaluated before SC-5 and SC-4 on the motion
path, so a suppressed motion advances no cached cell and cannot silently
swallow the next reportable motion.

**Boundary**: the existing chrome guards are not rewritten, reordered relative
to each other, or folded into the reporting path — their hit tests become
SC-8's inputs. The left-press-only gating of the existing LOCAL side effects
stays exactly as it is; only the reporting question becomes
identity-independent.

**Affects**: task0004 (owns SC-8), task0003 (the three handlers it supersedes).

### D12 — The three pointer handlers become gather-and-execute over a testable seam

The three pointer handlers in `pointer_routing.rs` each take mutable references
to the window host and to the application object, so none of them can be
constructed in a unit test. Their decision sequences were therefore pinned only
by source-text substring-ordering assertions, which verify round 1 rejected as
coverage for TS-19, TS-20 and TS-21: a correct decision unit whose call site is
miswired keeps those assertions green.

**Structure.** Each pointer path is split into three parts:

1. **Gather** — the handler reads plain values out of the host, the application
   and the core mode state. This step contains no decision: no branch that
   chooses between reporting and local handling, and no record mutation.
2. **Decide** — SC-10 turns those plain values into one enumerated outcome.
3. **Execute** — SC-11 applies the outcome's record updates and appends any
   report bytes to a caller-supplied destination; the handler then performs the
   outcome's named local arm and writes the captured bytes to the tab identifier
   the outcome names.

The residue that still cannot be unit-tested is step 1, a straight-line read
with no decision in it. Steps 2 and 3 carry every decision this feature makes
and are exercisable with no window, no GPU surface and no live PTY, which is
what makes TS-19, TS-20 and TS-21 assertable as behaviour (bytes produced or
not, record state after the call) instead of as source-text order.

**Corrections this structure absorbs.** Four defects live in exactly the wiring
this seam makes testable, and SC-10's postconditions state each as a contract
rather than leaving it to the call site: the release path reading only the
encoding instead of "any tracking mode active", and writing to the tab active at
release time instead of the tab the press was recorded on; the two reset
observations existing only on the motion path, so a click with no intervening
motion uses stale records; the motion path branching on the instantaneous shift
flag instead of the recorded gesture owner; and the absence of any focus-loss
reset for the held-button and ownership records, unlike the existing pointer
button-down record which is zeroed on focus loss.

**Boundary.** The decision content of SC-2 through SC-9 is unchanged — the
button codes, the encodings, the overflow rule, the motion gate's table, the
wheel matrix and the grid-ownership region set are all carried into SC-10
unaltered. What changes is where those decisions are evaluated from and how the
result reaches the side effect.

**Ownership split.** task0005 owns SC-10 and SC-11 and their behavioural tests,
in the mouse-report module. task0006 owns the handler reduction, the focus-loss
clear-all call site and the source-scanning assertions. Under D3 task0006
declares the mouse-report module in its own file list and adopts the integration
side for it on merge.

**Source-scanning assertions.** They are not deleted wholesale: after this
change their only legitimate residual subject is step 1's property that the
handler body holds no decision. Anything else they used to pin is re-expressed
as a behavioural assertion against SC-10 / SC-11 first.

**Affects**: task0005 (owns SC-10, SC-11), task0006 (owns the handlers, the
focus-loss call site and the assertions), task0003 and task0004 (the call sites
this supersedes; their plans are left as the record of the work at the time).

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Bare drag stops selecting text while an application tracks the mouse, surprising users (assumption A3) | High | Medium | Shift is the single, documented escape hatch and covers every local behaviour (FR7, D5); AC9 and manual scenario TS-11 verify all four of them in one pass |
| A chrome guard is bypassed and a click on the tab bar, status bar, scrollbar or mux sidebar leaks to the application | Medium | High | Reporting is inserted strictly after every existing guard in the routing function, never before or in parallel with them (FR10); manual scenario TS-12 exercises each guard |
| The three wheel consumers stop being mutually exclusive, double-consuming a notch (scroll *and* report) | Medium | High | The choice is one pure decision (SC-6) taken ahead of both existing paths, branching on tracking-active first so the two matrices cannot overlap, with an exhaustive table-driven test (TS-8) rather than nested conditionals at the call site |
| Motion reporting floods the PTY at pointer-pixel resolution and regresses the CPU work already done on this path | Medium | High | Cell-change filter (SC-4) plus the motion gate (SC-5) both run before encoding; TS-7 pins the filter and no logging is added on the path |
| Parallel worktrees produce two divergent copies of a shared helper (D3) | Medium | Medium | Contracts SC-1 through SC-6 are pinned here in full; the owning task is authoritative on merge and consumers adopt the integration side |
| The X10 overflow rule is implemented as a clamp, silently reporting the wrong cell on wide grids | Low | Medium | FR9 is expressed as an absent value in SC-3's contract, and TS-6 asserts the boundary exactly at 223/224 in both encodings |
| The core change accidentally pulls a GUI-only dependency into the CLI-only build | Low | High | L1 depends on nothing above it (Layer Structure) and the CLI-only check is an acceptance criterion of every task (TS-14) |
| task0005 and task0006 restructure the same pointer path in parallel and the merge drops one side's work (D12) | Medium | High | The two own disjoint files — task0005 the mouse-report module, task0006 the handlers, the focus-loss call site and the assertions — and SC-10 / SC-11 pin the seam's shape in full, so the consumer's stub is contract-identical to the owner's version it adopts on merge (D3) |
| The seam is introduced but the decision logic stays inside the handlers, so the tests remain unable to observe it | Medium | High | SC-10's testability postcondition and D12's step-1 rule are acceptance criteria of both tasks; the residual source-scanning assertion's only subject is that step 1 holds no decision |

## Open Questions

- [x] OQ-1 (resolved): AC9 and FR7 appeared to collide over Shift+wheel on the
      alternate screen with the alternate-scroll mode bit and setting both on.
      Resolved in favour of AC9's literal wording — Shift+wheel emits nothing to
      the PTY and scrolls eMterm's scrollback — and implemented as the
      tracking-active branch of SC-6. See decision D5 for the reasoning,
      provenance and evidence strength. No SPEC.md or REQUIREMENTS.md change is
      needed: this reading satisfies AC9 literally and is a valid reading of
      FR7.
- [ ] OQ-3: SPEC.md does not state which button a motion report carries when
      more than one button is held. Decision D6 (lowest-numbered held button)
      stands and was confirmed by second-opinion consultation at medium evidence
      strength; the question stays listed because the requirement documents
      remain silent on it. Reversible.
