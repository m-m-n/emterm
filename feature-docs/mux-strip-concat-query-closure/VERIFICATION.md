# Verification Document: mux-strip-concat-query-closure

## Overview
**Feature**: mux-strip-concat-query-closure / **SPEC.md**: `feature-docs/mux-strip-concat-query-closure/SPEC.md` / **IMPLEMENTATION.md**: `feature-docs/mux-strip-concat-query-closure/IMPLEMENTATION.md`

Run every command from the project root, without changing into `src-tauri/`.

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Command (CLI-only feature gate, NFR4): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Expected: exit code 0. The `tabs.rs` replay tests are known to be
  nondeterministic under parallel runs. If one of them fails, rerun it alone
  with `--test-threads=1` before treating it as a failure. They are not part
  of this feature.
- Coverage target: not measured (no coverage tooling is configured). Every
  scenario below must have at least one passing test.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Strip table: open head × removed construct kind × continuation (`n`, `c`, `t`, `m`, text), on every strip entry point | Exactly one closing at the construct position, before re-emitted C0. Reported state is none after the closing. The term_core view equals the raw-stream reference. No response. | Unit |
| TS-2 | Write filter, one cut-free call of `ESC[6` + target + `n` for every target kind, including an embedded-C0 query (EC-3) and several targets in one CSI (EC-4) | One closing per open CSI. Replay gives no response and matches the reference. | Unit |
| TS-3 | Reader level, through `run_visibility_restore_at`: chunks [`ESC[6` + target, `n`] and [`ESC[6` + target, CR, `n`], restore after the first read, no screen switch. Also the held-target variant (EC-2) and the reattach layout. | The client's responses, rows and cursor equal the raw-stream reference. No closing at the snapshot end. | Integration |
| TS-4 | Write filter: each answered query form split at every position and fed byte by byte, with and without C0 in the continuation, then replayed. Includes EC-8 and EC-9. | No response and no executable query in the ring. C0 effects kept once. Non-query CSIs written unchanged. Pending length 0 after every call. | Unit |
| TS-5 | Classification parity: open CSI prefixes (private marker, intermediates, long or saturating parameters, `ESC[261`) × every next byte | The filter writes the closing in place of the byte exactly when term_core answers the raw stream | Unit |
| TS-6 | Strip and write filter: `ESC ESC` + removed construct + `[c` / `[6n`, and the AC-4 forms, in one call and split at every position, including a lone trailing ESC held across calls (EC-11) | Replay equals the raw-stream reference with no response. The carried state is never an escape state. | Unit |
| TS-7 | Remap: watch offsets at the start of, inside and right after a span removed inside an open CSI and after a lone ESC; `build_snapshot_bytes` segments; entry-point identity and state-form identity | Offsets are non-decreasing and within the output. Segments point at the same content. Outputs are identical across entry points for equal start states. | Unit |
| TS-8 | The updated predecessor suites rerun (`round4_cut_csi`, `post_strip_cut_csi`, `round4_chain`, `round3_write_path`, `scrollback_filter`, `snapshot_bytes`): cut closings, designator exclusivity (EC-6), split invariance in replay terms, overflow flush (EC-7), EC-1 inversion | All pass. Changed expectations are only those listed in DECISIONS.md. | Unit |
| TS-9 | Budget: open CSI + target and lone ESC + target alternations at 100k repetitions, long ESC ESC chains before removed constructs, a 300k-byte CSI fed whole and byte by byte, the overflow path | Each finishes in under 10 s with no panic | Unit (performance) |

## Code Quality Verification
- Format: no format command is configured (`format_command` is empty). Do
  not run a crate-wide formatter. A diff limited to the files this feature
  touches is acceptable.
- Static analysis: the two `cargo check` commands in Build Verification.

## SPEC.md Compliance
### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC-1 | One cut-free call of each open head + each removed construct kind + `n` / `c` / `t`: replay gives no response, and the same rows and cursor as the raw-stream reference | TS-1, TS-2 pass |
| AC-2 | Production reader + visibility-restore harness, and the reattach layout, with no cut: the client equals the raw-stream reference | TS-3 passes |
| AC-3 | Each answered query form split at every position, fed byte by byte and with C0: no executable query, no response, C0 kept, non-queries unchanged, pending empty | TS-4, TS-5 pass |
| AC-4 | `ESC` + `ESC[6n` + `[c`, `ESC` + launch + `[6n`, and `ESC` + Kitty + `[c`, in one call and split: no response, equal to the reference | TS-6 passes |
| AC-5 | Identity across entry points, remap and `snapshot_bytes` segment tests pass, including the new offset cases | TS-7 passes |
| AC-6 | Predecessor tests pass with expectations changed only where FR1/FR3/FR4 change bytes, each change listed in DECISIONS.md | TS-8 passes, plus the Records checks below |
| AC-7 | Linear-pass and adversarial-budget tests within budget; `--lib` and `--no-default-features` succeed | TS-9 passes, plus Build and Test Verification |

### Functional Requirements Coverage
| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS-1, TS-2 |
| FR2 | task0001 | TS-3 |
| FR3 | task0001 | TS-4, TS-5 |
| FR4 | task0001 | TS-6 |
| FR5 | task0001 | TS-7 |
| FR6 | task0001 | TS-7 |
| FR7 | task0001 | TS-8 |
| FR8 | task0001 | TS-8, plus the Records checks below |
| NFR1 | task0001 | TS-9 |
| NFR2 | task0001 | TS-9 |
| NFR3 | task0001 | TS-1, TS-6 (replay through term_core shows no display, cursor or response effect from inserted bytes) |
| NFR4 | task0001 | TS-9, plus Build and Test Verification |

### Records checks (FR8)
- [ ] `feature-docs/mux-strip-concat-query-closure/DECISIONS.md` has a
  "Behavior-changing tests" section. Every test path it lists appears as a
  `<name>: test` line in the output of
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list`.
- [ ] `feature-docs/mux-cut-csi-post-strip-closure/DECISIONS.md` and the
  predecessor `test-docs/` records are unchanged against the feature's base
  revision. If a test was renamed, which the plan does not foresee, the
  records were updated per `.claude/rules/test-docs-records.md` and resolve
  in the same listing.

## Manual Testing (E2E Not Possible)
There is no E2E suite (SPEC: none detected). This optional check needs a
release binary that the user builds.
- [ ] In a mux pane, print `ESC[6` + an OSC 777 emterm markdown launch + `n`
  in one write. Then switch tabs or reattach. No `ESC[row;colR` text appears
  at the shell prompt.
- [ ] In a mux pane, print `ESC[6` + a launch, switch tabs and back, then
  print `n`. No unsolicited reply appears.

## Performance / Security Verification (if applicable)
- NFR1 / NFR2: each TS-9 input finishes in under 10 s with no panic. The
  pending length stays 0 for CSI-only input fed byte by byte.
- TM-1: At a removed construct, the strip writes one CSI_CLOSING while the
  written stream is inside a CSI or after a lone ESC. No strip join
  completes a query. Checked by TS-1, TS-2, TS-3 and TS-6: replaying the
  produced ring, including after a visibility restore or reattach, gives no
  response.
- TM-2: A split device query is closed by CSI_CLOSING in place of its
  completing final byte, using a carried classification that matches
  term_core. Checked by TS-4: no executable query in the ring and no
  response. Checked by TS-5: closing decision parity with term_core for
  every next byte.
- TM-3: The strip stays linear with O(1) state, and no CSI byte is held.
  Checked by TS-9 under the 10 s budget with no panic.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test scenarios (TS-1 to TS-9) | 9 | 9 | 0 | 0 |
| Success criteria (AC-1 to AC-7) | 7 | 7 | 0 | 0 |
| Security (TM-1 to TM-3) | 3 | 3 | 0 | 0 |
| Records (FR8) | 2 | 2 | 0 | 0 |
| Manual reproduction | 2 | 0 | 0 | 2 |
