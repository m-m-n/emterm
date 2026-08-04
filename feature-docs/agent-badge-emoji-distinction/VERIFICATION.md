# Verification Document: agent-badge-emoji-distinction

## Overview

**Feature**: agent-badge-emoji-distinction
**SPEC.md**: `feature-docs/agent-badge-emoji-distinction/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/agent-badge-emoji-distinction/IMPLEMENTATION.md`

## Build Verification

- Command (rust):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Feature-gate check (rust, NFR2):
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors.
- TypeScript component: not applicable — this feature changes no TypeScript
  file (no `bun test` / `bun run typecheck` delta expected beyond the
  existing baseline).

## Test Verification

- Command:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Coverage target: the new pure decision logic (state→presentation
  selection and fallback resolution) fully branch-covered (all four agent
  states × unseen flag; texture availability both ways). No project-wide
  numeric coverage threshold is configured in this repository.
- Note: tests live under `--lib`; the pre-existing `tabs.rs` replay tests
  are known to be non-deterministic under parallel execution
  (`--test-threads=1` stabilizes them) — unrelated to this feature.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | State→presentation (glyph / drawing mode) selection logic | `working` selects the U+26A1 emoji presentation, `idle` selects U+1F4A4; `blocked` / `done` select the circle presentation with current filled/ring semantics; emoji-with-no-texture resolves to the current filled circle | Unit |
| TS2 | CLI feature gate check | `--no-default-features` build compiles, exit code 0 | Integration (build) |
| TS3 | Manual on-device visual confirmation | `working` and `idle` are distinguishable at a glance in both the tab bar and the sidebar | Manual |

## Code Quality Verification

- Format / static analysis: no dedicated commands are configured in
  workflow.yaml `project.components`; formatting is enforced by the
  project's PostToolUse hook (rustfmt with pinned style edition).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC1 | All functional requirements implemented and tested | FR coverage table below; TS1/TS2 automated |
| SC2 | All test scenarios pass | TS1/TS2 automated runs green; TS3 manual check done |
| SC3 | `working` and `idle` distinguishable at a glance | TS3 manual check |
| SC4 | Distinction reflected in both tab bar and sidebar | TS1 (shared decision function) + TS3 (both surfaces inspected) |
| SC5 | States other than `working` / `idle` unchanged (edge case) | Pre-existing badge tests in both widget modules still pass; TS3 spot check |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1 (selection logic unit tests), TS3 (tab bar on-device check) |
| FR2 | task0001 | TS1 (same shared decision function), TS3 (sidebar on-device check) |
| FR3 | task0001 | TS1 (fallback decision unit tests — default text path never used for these glyphs); code review confirms the blit goes through the emoji texture cache + swash path |
| NFR1 | task0001 | TS3 (both surfaces compared on device); AC-4/AC-5 layout assertions in both widget test modules |
| NFR2 | task0001 | TS2 (feature-gate build check) |

## E2E Testing

None — the project has no E2E infrastructure (no `docker-compose.e2e.yml`,
no `e2e-tests/`; SPEC.md "E2E Tests"). No automated E2E is added.

## Manual Testing (E2E Not Possible)

- [ ] TS3: On-device check — with panes in `working` and in `idle` state,
      inspect BOTH the tab bar badge and the sidebar badge; confirm the two
      states are distinguishable at a glance and both surfaces agree.
      Investigation of any anomaly goes through `emterm.log` (DevTools are
      unavailable in this project).
- [ ] Mockup visual comparison (モックとの目視照合): compare the rendered
      badges against the design mockup
      `feature-docs/agent-badge-emoji-distinction/design/mockups/screen-agent-badges.html`
      (glyph choice ⚡/💤, replacement of the dot, 12px badge scale,
      untinted emoji color).
- [ ] Edge case: a pane in `blocked` and in `done` state still shows the
      current circle badge (filled when unseen, ring when seen) in both
      surfaces.
- [ ] Transition stability: drive a pane `working` → `done` and confirm the
      tab title / sidebar name does not shift horizontally.

## Performance / Security Verification

Not applicable — SPEC.md raises no performance or security requirement for
this feature.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build / feature gate | 2 | 2 | 0 | 0 |
| Unit tests (TS1 + layout assertions) | 1 group | 1 group | 0 | 0 |
| Feature gate scenario (TS2) | 1 | 1 | 0 | 0 |
| Visual / consistency (TS3 + mockup + edge + transition) | 4 | 0 | 0 | 4 |
