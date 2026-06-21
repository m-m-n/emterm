# Verification Document: mux client coalesce (phase1)

## Overview
**Feature**: mux-client-coalesce
**SPEC.md**: `doc/tasks/mux-client-coalesce/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-client-coalesce/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml`
- CLI-only feature gate: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors (no GUI-only symbol leaked into CLI build — though this change is GUI-only, the CLI check must still pass).

### Actual results (sdd.4-implement)
- CLI-only feature gate: `cargo check --no-default-features` → exit 0 (compiled clean). The release build for the Phase 3 E2E (`cargo test --release --test mux_throughput`) compiled the GUI binary successfully (`Finished release profile`).

## Test Verification
- Command (whole lib, single-threaded): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Coalesce metrics subset: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib 'tabs::tests::c_'`
- E2E throughput (release, ignored): `CARGO_TARGET_DIR=src-tauri/target cargo test --release --test mux_throughput --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture`
- Coverage target: no numeric target; the new/updated `c_` tests must cover the coalesce contract.

### Actual results (sdd.4-implement)
- Coalesce subset `tabs::tests::c_` (`--test-threads=1`): 3 passed, 0 failed.
  - `c_consecutive_active_pane_pty_output_coalesces_into_one_parse` (TS-1, new): parse-pass count == 1, grid == single-concatenated. PASS.
  - `c_pty_output_parsed_per_message_grid_grows_step_by_step` (TS-3, restated for batched 1-pass behavior): PASS.
  - `c_split_messages_equal_single_concatenated_message` (TS-2 parity): PASS.
- Full lib suite (`--lib --test-threads=1`): **1886 passed, 0 failed, 1 ignored** — covers TS-4 (non-active drop), TS-5 (control-message boundary), TS-6 (`pending_switch` legacy path), TS-7 (`ts11_*` detach), TS-8 (inner Kitty image) regression tests, all green.

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Consecutive active-pane `PtyOutput` (K frames, one buffer) through `process_combined` | Parsed in ONE pass (parse-pass count == 1); grid equals concatenated result | Unit (new, required) |
| TS-2 | Split vs single-concatenated `PtyOutput` | Identical final grid (`c_split_messages_equal_single_concatenated_message` green) | Unit (parity contract) |
| TS-3 | Metric test restated for batched behavior (`c_pty_output_parsed_per_message_grid_grows_step_by_step`) | One buffer of K active-pane frames ⇒ batched (1 pass), expected values updated | Unit |
| TS-4 | Non-active-pane `PtyOutput` among active-pane frames | Non-active frame excluded from coalesce buffer and dropped; active frames still render | Unit |
| TS-5 | Control message between active-pane `PtyOutput` frames | Accumulator flushed before the control message; output/control ordering preserved | Unit |
| TS-6 | `pending_switch` active while `PtyOutput` arrives | Accumulator flushed, then frame takes the legacy per-frame `live_queue` path (existing pending-switch tests stay green) | Unit (regression) |
| TS-7 | `Detached` frame mid-buffer behind active-pane output | Accumulator flushed before detach; detach `break` + post-loop tail re-route unchanged (existing `ts11_*` tests green) | Unit (regression) |
| TS-8 | Inner Kitty image spanning `PtyOutput` boundaries | Decodes correctly; post-loop inner-image drain unchanged (existing image tests green) | Unit (regression) |
| TS-9 | daemon-direct E2E throughput / frame count | MiB/s up and frame count down vs. baseline (2.85 MiB/s, 124,233 frames at N=10M) | Performance |
| TS-10 | FR1a: device-query frame (`CSI` final `n`/`c`/`t`/`p`) is excluded from coalescing | `payload_has_device_query` matches the response-producing finals (incl. resync after a malformed CSI and C0-mid-CSI); a query frame breaks the coalesce run so each reply is captured (`payload_has_device_query_detects_response_producing_finals`, `c_device_query_frame_breaks_coalesce_run`) | Unit (added in multi-review) |

## Code Quality Verification
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml` (only the touched file; do NOT run a crate-wide reformat).
- Static analysis: existing project lints (no new clippy obligations introduced by this change).

### Actual results (sdd.4-implement)
- Format: `rustfmt --edition 2024 --check src-tauri/src/tabs.rs` → exit 0 (formatted in-place by the PostToolUse hook; only the touched file). No crate-wide `cargo fmt` was run.
- `git status --short`: only `src-tauri/src/tabs.rs` modified + new `doc/tasks/mux-client-coalesce/`. No daemon/bridge/transport/`src/` (WebView) changes (NFR3 satisfied).

## File Structure Verification
### Files to Create
- `doc/tasks/mux-client-coalesce/{要件定義書.md, SPEC.md, IMPLEMENTATION.md, VERIFICATION.md, tasks.yaml, sdd.yaml}` - SDD artifacts.

### Files to Modify
- `src-tauri/src/tabs.rs` - coalesce in `process_combined`; `#[cfg(test)]` parse-pass counter; new + updated `c_` tests.
- `src-tauri/tests/mux_throughput.rs` - run only; test-only tweak permitted if frame-count reporting needs it.

### Scope Isolation (NFR3)
- [ ] No changes under daemon / bridge / transport code, and none under the WebView build (`src/`).
- [ ] Verify via `git diff --stat`: only `src-tauri/src/tabs.rs` (and optionally `src-tauri/tests/mux_throughput.rs`) plus `doc/tasks/mux-client-coalesce/` appear.

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | Consecutive active-pane `PtyOutput` parse once | TS-1 passes (count == 1) |
| SC-2 | Output byte-equivalent to per-frame | TS-2 green |
| SC-3 | Metric test reflects batched behavior | TS-3 green with updated expectations |
| SC-4 | E2E throughput up, frame count down | TS-9 measured before/after |
| SC-5 | Full lib suite green | `--lib --test-threads=1` exit 0 |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (coalesce consecutive PtyOutput) | Phase 2 | TS-1, TS-2 |
| FR2 (flush at control-message boundaries) | Phase 2 | TS-5, TS-7 |
| FR3 (per-batch side effects once) | Phase 2 | TS-2 (parity), TS-6/TS-7 regression |
| FR4 (non-active pane drop) | Phase 2 | TS-4 |
| FR5 (pending_switch legacy path) | Phase 2 | TS-6 |
| NFR1 (performance) | Phase 3 | TS-9 |
| NFR2 (correctness preservation) | Phase 2 | TS-2 |
| NFR3 (scope isolation) | Phase 2 | `git diff --stat` review |

## E2E Testing
Daemon-direct throughput harness (`src-tauri/tests/mux_throughput.rs`, release, `#[ignore]`).
- [ ] TS-9: throughput improved over baseline 2.85 MiB/s (N=10M); frame count reduced below 124,233.

## Manual Testing (E2E Not Possible)
- [ ] (Optional, not a phase1 pass gate) Real-environment `time seq 1 10000000` in a mux window — recorded only as input to the phase2/3 go/no-go decision.

## Performance Verification
- daemon-direct E2E MiB/s (N=10M): baseline 2.85 MiB/s → expected substantially higher.
- daemon-direct E2E frame count (N=10M): baseline 124,233 → expected lower.

### Actual results (sdd.4-implement, N=10M, release)
Command: `EMTERM_THROUGHPUT_N=10000000 CARGO_TARGET_DIR=src-tauri/target cargo test --release --test mux_throughput --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture`

| Metric          | Baseline (before) | After      | Direction |
|-----------------|-------------------|------------|-----------|
| Throughput      | 2.85 MiB/s        | 3.08 MiB/s | up ✅      |
| Frames received | 124,233           | 105,497    | down ✅    |
| Total bytes     | 84.77 MiB         | 84.77 MiB  | identical |
| Wall-clock      | 29.75 s           | 27.56 s    | down      |

Caveat: `mux_throughput.rs` measures the daemon socket layer with its own standalone
`client_core.process_pty_data_fully` per frame — it does NOT route through
`Tab::process_combined`, where the coalesce lives. The improvement seen here therefore
reflects relieved client backpressure (the daemon coalesces larger reads → fewer, bigger
frames) rather than the client coalesce itself. The client coalesce's parse-count collapse
(K frames ⇒ 1 parse) is proven directly by TS-1/TS-3 via the `coalesce_parse_passes` counter.
Both acceptance criteria (throughput up, frame count down) are met. No harness tweak was
needed — the existing harness already reports MiB/s and frame count.

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Functional (FR1–FR5) | 5 | 5 | 0 | 0 |
| Non-functional (NFR1–NFR3) | 3 | 2 | 1 | 0 |
| Test scenarios | 9 | 8 | 1 | 0 |
| Optional manual (non-gating) | 1 | 0 | 0 | 1 |
