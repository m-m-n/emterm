# Feature: tmux-startup-query-response-leak

## Overview

Starting tmux inside eMterm leaks terminal device-query responses into the
visible pane content. The reported symptom is the string
`\x1b[>65;1;0c\x1b[8;51;207t\x1b[4;816;1656t` displayed twice before the
shell output (a DA2 response, an XTWINOPS text-area-in-chars response, and an
XTWINOPS text-area-in-pixels response). This feature routes those responses to
the application that issued the query — tmux — so nothing is rendered as
visible text while tmux still receives everything its capability negotiation
needs.

Requirements source: `feature-docs/tmux-startup-query-response-leak/REQUIREMENTS.md`.

## Objectives

- When tmux starts inside eMterm, terminal device-query responses must not leak
  into the visible terminal content, while tmux still receives the responses it
  needs for capability negotiation.

## User Stories

### US1: Clean tmux startup with working capability negotiation
As an eMterm user starting tmux, I want device-query responses to reach tmux
instead of the visible pane, so that the prompt after tmux startup is clean and
tmux still negotiates its capabilities.

**Acceptance Criteria:**
- [ ] Launching tmux in a fresh eMterm tab leaves no query-response text visible at or after the tmux/shell prompt.
- [ ] tmux functions normally after startup (colors, resize handling, status line), demonstrating its queries were answered, not dropped.
- [ ] All in-scope response types (per FR5 resolution) are exercised by at least one automated test each, and the full `--lib` suite passes.
- [ ] `--no-default-features` check passes.
- [ ] In-scope runtime contexts (per FR4 resolution) are each verified.
- [ ] The tab-switch leak resolved by per-tab-grid-size does not recur.

## Technical Requirements

### Functional Requirements

- **FR1 - No visible leak of startup query responses** (status: resolved):
  Starting tmux in eMterm produces no device-query response bytes rendered as
  visible text in the pane; the prompt after tmux startup is clean. The
  reported symptom is the string
  `\x1b[>65;1;0c\x1b[8;51;207t\x1b[4;816;1656t` repeated twice (DA2 response,
  XTWINOPS text-area-in-chars response, XTWINOPS text-area-in-pixels response),
  displayed before the shell output.
- **FR2 - Query responses still delivered to the querying application**
  (status: resolved): The fix routes responses to the application that issued
  the query (tmux) rather than suppressing them; each response is still
  delivered exactly once. tmux capability negotiation continues to work.
- **FR3 - Automated regression coverage** (status: resolved): The leak
  mechanism is covered by Rust unit tests (inline `#[cfg(test)]` next to the
  code under test, `--lib` suite) so the regression is caught without manual
  reproduction.
- **FR4 - Runtime contexts covered** (status: **tbd**): Which runtime contexts
  the fix must cover — a plain eMterm tab running tmux, a tmux inside a mux
  pane, or both.
  *tbd_reason*: Batch policy `unresolved: record_tbd` (Codex consultation
  unavailable — usage limit). Tentative position: `both` (cover BOTH the plain
  eMterm tab running tmux and a tmux inside a mux pane). To be resolved by
  create-plan's `create-plan.tbd-resolution` assume gate.
- **FR5 - Query-response set in scope** (status: **tbd**): Which device-query
  responses are in scope — only the sequences observed in the reported leak
  (DA2, XTWINOPS 8 and 4), or the generalized set.
  *tbd_reason*: Batch policy `unresolved: record_tbd` (Codex unavailable).
  Tentative position: `generalize` — target the response set defined by
  mux-snapshot-device-query-strip: DA1 / DA2 / DSR / CPR / XTWINOPS 14,16,18 /
  DECRPM. To be resolved by create-plan's assume gate.
- **FR6 - No regression of the tab-switch leak fix** (status: resolved): The
  escape-string leak on tab switching that feature `per-tab-grid-size` resolved
  does not recur; that feature's existing tests stay green.

### Non-Functional Requirements

- **NFR1 - Performance / no output-pipeline latency regression** (status:
  resolved): The fix adds no measurable latency to the PTY output path for
  non-query traffic (normal rendering throughput unchanged).
- **NFR2 - Feature-gate integrity** (status: resolved): The CLI-only build
  (`--no-default-features`) still compiles; any GUI-only code touched stays
  behind `#[cfg(feature = "gui")]`.
- **NFR3 - No behavior change for non-tmux workloads** (status: resolved):
  Applications that legitimately echo or display received response bytes, and
  sessions not running tmux, behave exactly as before.
- **NFR4 - Snapshot-replay strip not regressed** (status: resolved): The
  existing mux snapshot/replay protection (`strip_replayable_rich_content`,
  feature mux-snapshot-device-query-strip) is not weakened or bypassed.

## Implementation Approach

### Architecture

No architectural design was produced for this feature: the design step is
`skipped`, on the grounds that this is an internal Rust escape-response routing
fix with no visual or design-token surface — the same call as the
`per-tab-grid-size` precedent.

What the requirements do fix about the approach:

- The correction is a **routing** change, not a suppression change: responses
  go to the querying application's PTY writer and are delivered exactly once
  (FR2).
- The change is internal Rust escape-response routing and requires no new
  dependencies (assumption A2).
- GUI-only code touched stays behind `#[cfg(feature = "gui")]` (NFR2).

The concrete component-level design, including which code path performs the
routing, is deferred to the create-plan phase, together with the FR4 and FR5
`tbd` resolutions.

### Data Flow

```
tmux issues device query → PTY → eMterm parser → response generated
                                                     │
                          delivered exactly once ────┴──→ querying PTY writer (tmux)
                          never entering ─────────────────→ visible grid / scrollback
```

### API Design

Not applicable — no external API surface is defined by the requirements.

### Database Schema

Not applicable — no data-storage surface is defined by the requirements.

### Dependencies

**Internal Dependencies:**
- `per-tab-grid-size`: its tab-switch escape-string leak fix must not regress,
  and its existing tests must stay green (FR6).
- `mux-snapshot-device-query-strip`: its `strip_replayable_rich_content`
  protection must not be weakened or bypassed (NFR4); its response taxonomy
  (DA1 / DA2 / DSR / CPR / XTWINOPS 14,16,18 / DECRPM) is the tentative FR5
  generalization target.

**External Dependencies:**
- None. No new dependencies are required; the fix is internal Rust
  escape-response routing (assumption A2).

### File Structure

Deferred to create-plan. The requirements fix only that the automated tests are
inline `#[cfg(test)]` modules next to the code under test, run as part of the
`--lib` suite (FR3).

## Test Scenarios

### Unit Tests
- [ ] **TS4** (FR1, FR2, FR3, FR5): Rust unit tests on the response-routing
      mechanism — feed the in-scope query sequences (per FR5) through the core
      path and assert the responses are delivered to the querying PTY writer and
      never enter the visible grid or scrollback. Run via
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      (tabs.rs replay tests may need `-- --test-threads=1`).
- [ ] **TS5** (NFR3): Negative test — byte streams that merely resemble
      responses inside ordinary application output remain untouched.
- [ ] **TS6** (NFR2): Feature-gate check —
      `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      succeeds.
- [ ] **TS7** (FR6, NFR4): The existing per-tab-grid-size and
      mux-snapshot-device-query-strip test suites remain green in the `--lib`
      run.

### Integration Tests
Covered by the `--lib` suite above; no separate integration suite is specified
by the requirements.

### E2E Tests
**Existing E2E tests**: None — this project has no E2E infrastructure, so the
runtime-context scenarios below are manual.
**Run command**: Not detected

- [ ] **TS1** (FR1, FR4) — manual, plain tab: launch the release eMterm binary,
      run `tmux` in a fresh tab, observe the pane during and after startup.
      Expect no stray sequences (`^[[>65;1;0c`, `^[[8;R;Ct`, `^[[4;H;Wt`) at the
      prompt.
- [ ] **TS2** (FR1, FR4) — manual, mux pane (if FR4 resolves to `both`): attach
      to a mux session, run `tmux` inside a mux pane, verify the same clean
      startup; then detach/reattach and verify no replayed leak.
- [ ] **TS3** (FR2) — manual, tmux health after the fix: inside the started
      tmux, split panes, resize the window, confirm colors and status line
      render correctly, proving responses were routed, not suppressed.

### Edge Cases
- [ ] Byte streams resembling device-query responses inside ordinary
      application output must remain untouched (NFR3 / TS5).
- [ ] Applications that legitimately echo or display received response bytes
      behave exactly as before (NFR3).
- [ ] Sessions not running tmux behave exactly as before (NFR3).
- [ ] Snapshot/replay paths keep their existing device-query stripping (NFR4 /
      TS7).

### Performance Tests
- [ ] NFR1: the PTY output path for non-query traffic shows no measurable
      latency addition (normal rendering throughput unchanged).

## Security Considerations

Not applicable — the requirements define no authentication, authorization,
input-validation, or data-protection surface for this feature.

## Error Handling

Not applicable — the requirements define no error-code surface for this
feature.

## Performance Optimization

### Performance Goals
- No measurable latency added to the PTY output path for non-query traffic;
  normal rendering throughput unchanged (NFR1).

## Assumptions

Carried over from the requirements analysis; these are working assumptions, not
confirmed conclusions.

- **A1**: The leak mechanism is analogous to the previously fixed mux-reattach
  DA1 leak (device-query responses entering a replay/echo path), making the
  mux-snapshot-device-query-strip response taxonomy the natural generalization
  target for FR5.
- **A2**: No new dependencies are required; the fix is internal Rust
  escape-response routing.
- **A3**: The leaked XTWINOPS pixel values (816x1656 = 51 rows x 16px, 207 cols
  x 8px) match term_core's DEFAULT cell metrics, indicating the responding
  parser never received real font metrics — an investigative lead, not a
  confirmed root cause.

## Success Criteria

- [ ] Launching tmux in a fresh eMterm tab leaves no query-response text visible at or after the tmux/shell prompt.
- [ ] tmux functions normally after startup (colors, resize handling, status line), demonstrating its queries were answered, not dropped.
- [ ] All in-scope response types (per FR5 resolution) are exercised by at least one automated test each, and the full `--lib` suite passes.
- [ ] `--no-default-features` check passes.
- [ ] In-scope runtime contexts (per FR4 resolution) are each verified.
- [ ] The tab-switch leak resolved by per-tab-grid-size does not recur.

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- [ ] **FR4**: Runtime contexts covered — Batch policy `unresolved: record_tbd`
      (Codex consultation unavailable — usage limit). Tentative position:
      `both` (cover BOTH the plain eMterm tab running tmux and a tmux inside a
      mux pane). To be resolved by create-plan's
      `create-plan.tbd-resolution` assume gate.
- [ ] **FR5**: Query-response set in scope — Batch policy
      `unresolved: record_tbd` (Codex unavailable). Tentative position:
      `generalize` — target the response set defined by
      mux-snapshot-device-query-strip: DA1 / DA2 / DSR / CPR /
      XTWINOPS 14,16,18 / DECRPM. To be resolved by create-plan's assume gate.

## References

- Requirements document: `feature-docs/tmux-startup-query-response-leak/REQUIREMENTS.md`
- Feature `per-tab-grid-size`: resolved the tab-switch escape-string leak (FR6)
- Feature `mux-snapshot-device-query-strip`: `strip_replayable_rich_content`
  snapshot/replay protection and the response taxonomy proposed for FR5 (NFR4,
  A1)
