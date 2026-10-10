# Feature: pane-state-rank-unify

## Overview

The in-pane state order blocked > working > error > done > idle is defined
once, in `AgentState::compose_rank` (`src-tauri/src/agent_status.rs`).
`ProgramState::rank` (`src-tauri/src/program_status.rs`) derives its value
from it through an exhaustive ProgramState -> AgentState correspondence. A
unit test pins that the two rank functions agree for all five states, and
the doc comments of both functions describe this relationship accurately.

## Objectives

- The in-pane state order blocked > working > error > done > idle is
  defined in exactly one place, and every other module that needs it derives
  the order from that place.
- A test pins that the two rank functions agree for all five states, so the
  daemon's aggregate state (`Table::summary`) and the composition decision
  (`agent_status::compose` / `composite_name`) cannot drift apart.
- The doc comment on `AgentState::compose_rank` matches what the code
  actually does.

## Acceptance Criteria

- [ ] **AC-1** (FR1, FR2): The rank values for blocked / working / error /
  done / idle appear in one match only, inside `AgentState::compose_rank`.
  `ProgramState::rank` gets its result from `compose_rank` through an
  exhaustive five-variant correspondence.
- [ ] **AC-2** (FR3): A unit test asserts that `ProgramState::rank` and
  `AgentState::compose_rank` order all 25 ordered pairs of the five states
  the same way, and that each `ProgramState` maps to the `AgentState` with
  the same protocol word.
- [ ] **AC-3** (FR4, FR5): The `compose_rank` doc names itself as the single
  definition and names `ProgramState::rank` / `Table::summary` as derived
  users. Its statement about `agent_status_model`'s cross-pane aggregation
  matches the code. The `ProgramState::rank` doc refers to `compose_rank`.
- [ ] **AC-4** (NFR1, NFR3):
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
  passes, and the existing `program_status` / `agent_status` /
  `agent_status_model` tests pass unchanged under their current names.
- [ ] **AC-5** (NFR2):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  succeeds.

## Technical Requirements

### Functional Requirements

- **FR1:** Single definition of the in-pane order.
  `AgentState::compose_rank` (`src-tauri/src/agent_status.rs`) is the only
  place where the in-pane order blocked > working > error > done > idle is
  written as rank values. `ProgramState::rank`
  (`src-tauri/src/program_status.rs`) keeps its signature
  `pub fn rank(self) -> u8` and gets its value from
  `AgentState::compose_rank` through a ProgramState -> AgentState
  correspondence instead of its own match on rank values.
- **FR2:** Exhaustive ProgramState -> AgentState correspondence. The
  correspondence `ProgramState::rank` uses maps each of the five
  `ProgramState` variants (`Idle`, `Working`, `Done`, `Blocked`, `Error`) to
  the `AgentState` variant of the same name. It is an exhaustive match with
  no wildcard arm, so adding a `ProgramState` variant does not compile until
  the variant is mapped.
- **FR3:** Agreement test across all five states. A unit test iterates over
  every ordered pair (a, b) of `ProgramState::ALL` and asserts that
  `a.rank()` compared with `b.rank()` gives the same ordering as
  `compose_rank` of the corresponding `AgentState` values. The test also
  asserts that each `ProgramState` maps to the `AgentState` of the same
  protocol word.
- **FR4:** `compose_rank` doc matches reality. The doc comment of
  `AgentState::compose_rank` says that it is the single definition of the
  in-pane order, and that `ProgramState::rank` (used by `Table::summary` to
  pick the deciding record) takes its value from it. The doc's statement
  about the GUI's cross-pane aggregation is accurate: `agent_status_model`'s
  `priority_rank` is a separately defined, unseen-aware cross-pane order and
  does not derive its values from `compose_rank`.
- **FR5:** `ProgramState::rank` doc points to the single definition. The doc
  comment of `ProgramState::rank` refers to `AgentState::compose_rank` as
  the source of the order. It does not present the order as its own
  independent definition.

### Non-Functional Requirements

- **NFR1 - No behavior change:** For every input, `Table::summary`,
  `agent_status::compose` and `agent_status_model`'s `composite_name` return
  the same results as before the change. This includes tie-breaking:
  `Table::summary` picks the more recently updated record on a rank tie, and
  `compose` and `composite_name` favor the OSC 7501 side on a tie.
- **NFR2 - Modules stay build-agnostic:** `agent_status` and
  `program_status` still compile without the `gui` feature, and neither
  gains a dependency on a feature-gated module.
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  succeeds.
- **NFR3 - Existing tests unchanged:** Existing tests keep their names and
  pass without changes. These include
  `program_status::tests::ac5_rank_is_blocked_working_error_done_idle`, the
  `ac5_*` summary tests, and
  `agent_status::tests::compose_returns_the_higher_input_by_blocked_working_error_done_idle`
  together with the other `compose_*` tests. Because no tests are renamed,
  no predecessor test-docs record needs updating.

## Implementation Approach

### Architecture

**Component Diagram:**
```
program_status::ProgramState::rank()
        |
        |  exhaustive ProgramState -> AgentState correspondence (FR2)
        v
agent_status::AgentState::compose_rank()   <-- single definition of
                                               blocked > working > error > done > idle (FR1)

program_status::Table::summary()  --uses-->  ProgramState::rank()
agent_status::compose()           --uses-->  AgentState::compose_rank()

agent_status_model priority_rank  : separately defined cross-pane,
                                    unseen-aware order (unchanged, FR4 / A4)
```

### Dependencies

**Internal Dependencies:**
- `program_status` -> `agent_status`: `ProgramState::rank` calls
  `AgentState::compose_rank`. `agent_status` gains no dependency on
  `program_status`.
- Both modules are non-feature-gated (NFR2).

**External Dependencies:**
- None.

### Affected Code

- `src-tauri/src/agent_status.rs` — `AgentState::compose_rank` (single
  definition, doc comment per FR4).
- `src-tauri/src/program_status.rs` — `ProgramState::rank` (derived value,
  doc comment per FR5). Its caller `Table::summary` stays as it is.
- `src-tauri/src/program_status/tests.rs` — existing caller of
  `ProgramState::rank`, stays as it is.

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/pane-state-rank-unify/**`
- `test-docs/pane-state-rank-unify/**`

`feature-docs/pane-state-rank-unify/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/pane-state-rank-unify/**` covers
`test-docs/pane-state-rank-unify/{T}.tests.yaml`, the per-task test record.
It is generated and owned by `implement-phase.md`; this section cites it and
restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/pane-state-rank-unify/` directory at all; the declared
`test-docs/pane-state-rank-unify/**` entry is still correct in that case — a
declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS-1** (AC-2; FR3): For each (a, b) in `ProgramState::ALL` x
  `ProgramState::ALL`, compare `a.rank().cmp(&b.rank())` with the
  `compose_rank` cmp of the mapped `AgentState` values. Also assert that the
  mapped `AgentState`'s Display word equals `ProgramState::word()`.
- [ ] **TS-2** (AC-1; FR1, FR2): For each `ProgramState` s, assert
  `s.rank() == compose_rank` of its mapped `AgentState`.

### Regression Tests

- [ ] **TS-3** (AC-4; NFR1, NFR3): Run the full `--lib` suite, including
  `ac5_rank_is_blocked_working_error_done_idle`, the `ac5_*`
  `Table::summary` tests (tie goes to the more recently updated record; a
  higher rank beats recency) and the `compose_*` tests (tie goes to the
  OSC 7501 side). All pass unchanged.

### Build Checks

- [ ] **TS-4** (AC-5; NFR2): The `--no-default-features` cargo check
  compiles.

### Review

- [ ] **TS-5** (AC-3; FR4, FR5): Review the doc comments on
  `AgentState::compose_rank` and `ProgramState::rank` against FR4 / FR5.

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] A tie in `Table::summary` is decided by recency (later record wins). A
  tie in `compose` / `composite_name` is decided by source (OSC 7501 wins).
  Both must stay as they are.
- [ ] `Error` is not part of the OSC 777 grammar but has a rank (2). The
  correspondence and the test must cover it (`ProgramState::ALL` has five
  members, while `AgentState::ALL` has four and excludes `Error`; the test
  iterates over `ProgramState::ALL`, or over `AgentState::ALL_WITH_ERROR` on
  the `AgentState` side).
- [ ] If a future variant is added to `ProgramState`, the build fails until
  the variant is mapped (FR2). If one is added to `AgentState`,
  `compose_rank`'s existing exhaustive match already fails to compile.

## Assumptions

- **A1:** The task allows unifying the definition or only adding a
  consistency test. This spec does both: the definition is unified (the
  primary goal) and the agreement test is added as a guard on the mapping.
- **A2:** The single definition lives in `AgentState::compose_rank`, whose
  doc already claims that role. Dependency direction:
  `program_status` -> `agent_status`. `agent_status` gains no dependency on
  `program_status`. Both modules are non-feature-gated, so NFR2 holds.
- **A3:** `ProgramState::rank` is kept (neither deleted nor renamed), so its
  callers `program_status.rs:525` (`Table::summary`) and
  `program_status/tests.rs:120` stay as they are. No symbol is scheduled for
  deletion or renaming.
- **A4:** `agent_status_model.rs` `priority_rank`, the cross-pane
  unseen-aware order, is out of scope except that the `compose_rank` doc
  must describe its relationship accurately (FR4). Its values are not
  changed and not derived from `compose_rank` in this feature.
- **A5:** Only the relative order of rank values can be observed (they are
  only compared). The existing numeric values 0..4 may stay as they are.
- **A6:** The code to change is the integration worktree at base_revision
  e461335d (equal to main HEAD, with osc7501-program-status already merged).
  The task text's mention of the
  `em-workflow/osc7501-program-status/integration` branch is historical
  context.

## Success Criteria

- [ ] All functional requirements (FR1-FR5) are implemented and tested
- [ ] All test scenarios (TS-1 to TS-5) pass
- [ ] NFR1-NFR3 are satisfied
- [ ] Code review is completed

## Open Questions

None.
