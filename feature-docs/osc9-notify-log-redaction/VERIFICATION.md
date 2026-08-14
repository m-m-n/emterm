# Verification Document: OSC 9 Notification Log Redaction

## Overview

**Feature**: osc9-notify-log-redaction
**SPEC.md**: `feature-docs/osc9-notify-log-redaction/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/osc9-notify-log-redaction/IMPLEMENTATION.md`

Test IDs below use the SPEC's hyphen-less form. TS1-TS6 carry the same meaning as the
identically-numbered scenarios in SPEC.md; TS7-TS14 are added by this plan to give every
requirement a verifying item.

## Build Verification

- Command (main): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (cli, feature-gate containment): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors, no new warnings attributable to the feature's files.

## Test Verification

- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0, zero failures, and the pre-existing test count strictly increased by
  the new redaction tests (no existing test removed or renamed).
- Coverage target: the project defines no percentage threshold, so none is imposed here. The
  coverage requirement is per-requirement instead: every scenario in the table below has a
  concrete verifying item, and every FR/NFR appears in the coverage table further down.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | Render metadata for a (title, body) pair containing a URL, a token-like string and a command line | The rendering contains none of those three substrings | Unit |
| TS2 | Render metadata for a known (title, body) pair | The rendering carries the title and body lengths as UTF-8 byte counts, in the fixed order / field naming of IMPLEMENTATION.md, with no fourth field | Unit |
| TS3 | Derive the diagnostic ID twice for the same pair, and once for a pair differing only in the body | Equal ID for the same pair within one run; different ID for the differing pair; 16 lowercase hex characters in both cases | Unit |
| TS4 | Existing rate-limiter behaviour tests (`rate_limiter_dedupes_identical_pair_within_window`, `rate_limiter_allows_after_window_elapsed`, `rate_limiter_distinct_pairs_not_deduped`) | Pass unmodified — sink delivery and dedupe semantics untouched | Unit (regression) |
| TS5 | Existing OSC 9 parse micro-tests (`parse_osc9_*`) | Pass unmodified — title/body derivation untouched | Unit (regression) |
| TS6 | Manual: on a release build, trigger a duplicate OSC 9 notification within the 1 s dedupe window and read `~/.local/share/net.laser5.app.emterm/logs/emterm.log` | A warn record names the suppression via the marker and carries only the three allow-listed fields; no fragment of the notification title or body appears | Manual |
| TS7 | Assert the rate-limit marker constant's value | Still exactly `LOG_NOTIFY_RATE_LIMIT`, so existing log greps keep matching | Unit |
| TS8 | Run the CLI-only check command | Exit code 0 — the change stays inside the GUI-gated module and adds nothing to the CLI build surface | Build |
| TS9 | Run the full library test command | Exit code 0, zero failures | Integration |
| TS10 | Render the same body twice — once raw, once in the form the escape gate produces for a body containing markup meta-characters | The two renderings differ, pinning the premise that the capture point (pre-escape) determines the ID | Unit |
| TS11 | Inspect the feature diff of `src-tauri/src/callbacks.rs` | The dispatch-error record is unchanged; neither rewritten record binds the title or the body; no log statement anywhere in the diff interpolates notification text; the per-run key is not logged | Inspection |
| TS12 | Existing body-markup escape tests (the `body_markup_escape` test module) | Pass unmodified — the escape pipeline is untouched | Unit (regression) |
| TS13 | Inspect the feature diff of the crate and workspace manifests | No dependency added or version-changed | Inspection |
| TS14 | Inspect the redaction call sites | The rendering is produced at most once per notification event, and its cost is one hash over two short strings plus one string allocation on a path that already allocates two strings | Inspection |

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — expected exit code 0.
  A failure attributable only to files this feature did not touch is a pre-existing condition,
  not a feature defect; a failure in `src-tauri/src/callbacks.rs` or
  `src-tauri/src/callbacks/tests.rs` is a defect and must be fixed.
- Static analysis: no separate lint command is configured for this crate; the build commands
  above carry the compiler's own diagnostics.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | No record from the OSC 9 notification path contains any substring of the title or body | TS1 for the renderer, TS11 for the two sites, TS6 for the end-to-end observation |
| AC2 | The rate-limit record still identifies the event via the marker at warn level and carries title length, body length and a diagnostic ID | TS2, TS7, TS6 |
| AC3 | Two suppressions of the same pair within one run share a diagnostic ID; different pairs differ | TS3 |
| AC4 | The success-path record carries metadata only, at debug level, with the redact-both rationale recorded in SPEC.md | TS2, TS10, TS11 plus the SPEC.md section "Rationale: why the success-path debug line is redacted too" |
| AC5 | The library test command passes | TS9 |
| AC6 | The CLI-only check command passes | TS8 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS2, TS7, TS11, TS6 |
| FR2 | task0001 | TS1, TS2, TS3 |
| FR3 | task0001 | TS3 |
| FR4 | task0001 | TS7, TS6 |
| FR5 | task0001 | TS1, TS2, TS3, TS10, TS11 |
| FR6 | task0001 | TS1, TS2, TS3, TS10 |
| FR7 | task0001 | TS11 |
| NFR1 | task0001 | TS4, TS5, TS9 |
| NFR2 | task0001 | TS12 |
| NFR3 | task0001 | TS7, TS6 |
| NFR4 | task0001 | TS13 |
| NFR5 | task0001 | TS14 |
| NFR6 | task0001 | TS8 |

## E2E Testing

Not applicable. The project has no E2E harness for this path and the resolved E2E input set is
empty (SPEC A5); `e2e_test_command` is empty for both components in workflow.yaml. No E2E
coverage is added.

## Manual Testing (E2E Not Possible)

- [ ] TS6: On a release build, emit an OSC 9 notification twice within the 1 s dedupe window
      with a title and body containing recognisable text, then read
      `~/.local/share/net.laser5.app.emterm/logs/emterm.log`. Confirm the warn record names the
      suppression via the marker, carries only the three allow-listed fields, and contains no
      fragment of the text. Confirm the record still follows the `[LEVEL] <message>` convention.
- [ ] TS11: Read the feature diff of `src-tauri/src/callbacks.rs` end to end and confirm the
      dispatch-error record is untouched, that no record on this path binds notification text,
      and that the per-run key is never logged.
- [ ] TS13: Read the feature diff of the manifests and confirm no dependency was added.
- [ ] TS14: Confirm at the call sites that the rendering is produced at most once per
      notification event.

## Performance / Security Verification

- NFR5 (negligible per-notification cost): no measured threshold is defined (SPEC.md declares
  performance tests not applicable). Verified by TS14's inspection only.
- Information disclosure (OBJ1, FR1, FR5): verified by TS1 for the renderer and TS11 for the
  sites; the manual TS6 confirms the end result in a real log file.
- One-way diagnostic ID (FR3): verified by TS3 plus the TS11 confirmation that the per-run key
  never reaches a log record. Accepted residual: an attacker who can both inject notifications
  into the running process and read the log can confirm a guessed pair by matching IDs — see
  IMPLEMENTATION.md's Risk Assessment.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit tests | 8 (TS1, TS2, TS3, TS4, TS5, TS7, TS10, TS12) | 8 | 0 | 0 |
| Build / integration commands | 2 (TS8, TS9) | 2 | 0 | 0 |
| Inspection / manual | 4 (TS6, TS11, TS13, TS14) | 0 | 0 | 4 |
| **Total** | **14** | **10** | **0** | **4** |
