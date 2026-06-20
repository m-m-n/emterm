# Verification Document: mux Transport/Content Parser Isolation

## Overview
**Feature**: mux-transport-parser-isolation
**SPEC.md**: `doc/tasks/mux-transport-parser-isolation/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-transport-parser-isolation/IMPLEMENTATION.md`

## Build Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- CLI-only: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- Expected: exit code 0, no errors

### Actual Results (sdd.4 implementation)
- Default build: `cargo check` — exit 0, no errors.
- CLI-only build: `cargo check --no-default-features` — exit 0, no errors.
- (Release build NOT run — left to the user's explicit call per project policy.)

## Test Verification
- Command: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- Note: `tabs.rs` replay tests are non-deterministic in parallel; `--test-threads=1` for stability.
- Coverage target: new extractor logic and the split-chunk path covered by unit/integration tests.

### Actual Results (sdd.4 implementation)
- `cargo test --lib -- --test-threads=1`: **1847 passed; 0 failed; 1 ignored**.
- `cargo test -p term_core` (extractor unit tests TS-1/2/3 + crate suite): **658 passed; 0 failed; 4 ignored**.
- New tests added:
  - `crates/term_core/src/mux_apc_extractor.rs` — 11 unit tests (TS-1 complete frame, TS-2 split-across-feeds reassembly incl. mid-introducer split, TS-3 OSC 9999 `emterm-mux;` normalization incl. BEL-terminated + non-mux discard, Print/CSI discard, multi-frame, reset drops partial).
  - `src-tauri/src/tabs.rs` — TS-4 (split inner Kitty over mux PtyOutput boundaries assembles one image, no base64 leak), TS-9 (non-mux Kitty decodes), TS-5 (pre-mux routes through core + post-Welcome outer Print discarded), TS-6 (detach restores core routing + extractor partial-frame reset), TS-7 (double-Welcome image decode intact).

### Test Scenarios from SPEC.md
| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Extractor: complete `emterm-mux;` APC frame in one feed | Frame payload returned intact | Unit | ✅ PASS |
| TS-2 | Extractor: APC frame split across two feeds | Reassembled into one payload (no corruption) | Unit | ✅ PASS |
| TS-3 | Extractor: OSC 9999 `emterm-mux;` fallback frame | Normalized to the same APC payload form (parity with `handle_osc_internal`) | Unit | ✅ PASS |
| TS-4 | Kitty image inner content split across mux PtyOutput messages with interleaving outer pumps | Single decodable image; no base64 leak | Integration | ✅ PASS |
| TS-5 | Pre-mux (before Welcome) PTS bytes | Routed through `Tab::core` (extractor not engaged) | Unit/Integration | ✅ PASS |
| TS-6 | Detach clears `mux_session_name` | PTS routing returns to `Tab::core` | Unit/Integration | ✅ PASS |
| TS-7 | Double-Welcome delivery | No replay corruption; extractor state consistent | Integration | ✅ PASS |
| TS-8 | DIAG removal | No `DIAG` strings / `parser_mid_sequence()` references; build passes | Build/Static | ✅ PASS |
| TS-9 | Non-mux Kitty image | Decodes as before (no regression) | Integration | ✅ PASS |
| TS-10 | Protocol files unchanged | mux daemon / mux_ipc / bridge untouched | Static/Review | ✅ PASS (no edits to mux daemon / mux_ipc / bridge) |
| TS-11 | (FR5/④) `process_combined` fed `[inner PtyOutput frame][Detached frame][plain shell prompt bytes]` | Plain prompt bytes render via `self.core` (not dropped across the detach transition) | Integration | ✅ PASS (Phase 5) |
| TS-12 | (NFR5/⑤) `MuxApcExtractor::new(param, prefix)` with injected values | Extracts using injected values; discards an OSC frame whose param differs | Unit | ✅ PASS (`ts12_injected_osc_param_and_prefix_are_used`) |
| TS-13 | (NFR5/⑤) Pre-mux OSC 9999 `emterm-mux;` Welcome through `self.core` | Reaches the mux APC path via the app-layer `on_osc` (no `term_core` special-casing); Windows ConPTY parity | Unit/Integration | ✅ PASS (`osc_9999_emterm_mux_inband_routed_to_pending_apc` + `osc_9999_non_mux_prefix_is_dropped`) |
| TS-14 | (Phase 7/B) Coalesced `[Detached frame][non-mux Kitty image APC]` | Image decodes EXACTLY once (loop `break` at detach prevents extracted-frame + tail-reroute double-decode) | Integration | ✅ PASS (`ts11_post_detached_image_decodes_exactly_once`) |
| TS-15 | (Phase 7/C) term_core OSC param override via `register_osc_app_param` | Registered core: OSC 9999 → `on_osc(102)`; unregistered → `on_osc(255)`; override never shadows a native OSC (OSC 2 stays 2) | Unit | ✅ PASS (3 tests in `term_core/callbacks.rs`) |

## Code Quality Verification
- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml` (functional files only; do not crate-wide reformat unrelated files)
- Static: `cargo check` clean (no unused-symbol warnings from the removed accessor)

### Actual Results (sdd.4 implementation)
- Formatting: `rustfmt` run per-file on the changed files only (`mux_apc_extractor.rs`, `tabs.rs`, `terminal_core.rs`, `mux/apc.rs`) — no crate-wide reformat (project does not enforce rustfmt).
- Static: `cargo check` (default + `--no-default-features`) clean, exit 0. No unused-symbol warning from the removed `parser_mid_sequence()` accessor.
- DIAG verification: `grep -rn "DIAG" crates/term_core/src src-tauri/src` returns no `DIAG`-diagnostic matches in the touched files; `grep -rn "parser_mid_sequence"` returns empty.

## File Structure Verification
### Files to Create
- [x] `crates/term_core/src/mux_apc_extractor.rs` - public independent transport extractor

### Files to Modify
- [x] `crates/term_core/src/lib.rs` - export extractor (`pub mod mux_apc_extractor;` + `pub use ...MuxApcExtractor`)
- [x] `crates/term_core/src/terminal_core.rs` - remove `parser_mid_sequence()` accessor
- [x] `src-tauri/src/tabs.rs` - `Tab` extractor field; `pump`/`process_combined` branch; detach reset; remove DIAG logs
- [x] `src-tauri/src/mux/apc.rs` - restore original simple warn (remove DIAG)
- [x] (P5) `crates/term_core/src/mux_apc_extractor.rs` - `feed` reports per-frame end offsets
- [x] (P5) `src-tauri/src/tabs.rs` - `process_combined` re-routes the post-`Detached` tail to `self.core`
- [x] (P6) `crates/term_core/src/mux_apc_extractor.rs` - `new(osc_param, prefix)` injection; remove `MUX_OSC_PARAM`/`MUX_PREFIX` + `drift_*` tests
- [x] (P6) `crates/term_core/src/osc_handler.rs` - remove OSC 9999 `emterm-mux;` special-casing; map 9999 → action-type
- [x] (P6) `src-tauri/src/tabs.rs` - construct extractor with `mux_ipc::protocol::{MUX_OSC_PARAM, APC_PREFIX}`
- [x] (P6) `src-tauri/src/callbacks.rs` - `on_osc` arm: OSC 9999 `emterm-mux;` → mux APC path

## SPEC.md Compliance

### Success Criteria
| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | mux inline Kitty image renders, no base64 leak | Manual M1 + TS-4 |
| SC-2 | Large image assembles across chunk boundaries | Manual M2 |
| SC-3 | Non-mux path unaffected | TS-9 + Manual M4 |
| SC-4 | SIXEL renders in mux | Manual M5 |
| SC-5 | Markdown / text / TUI parity in mux | Manual M6 |
| SC-6 | DIAG diagnostics removed | TS-8 |
| SC-7 | Split-chunk regression test added & passes | TS-4 |
| SC-8 | (FR5/④) Post-detach shell bytes coalesced behind `Detached` render via `self.core` | TS-11 |
| SC-9 | (NFR5/⑤) `term_core` holds no mux constants; extractor injected; OSC 9999 recognition in app layer | TS-12 + TS-13 + static grep |

### Functional Requirements Coverage
| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 Dedicated extractor | Phase 1, 2 | TS-1, TS-2, TS-3 |
| FR2 Inner-content-only core | Phase 2 | TS-4 |
| FR3 APC + OSC fallback | Phase 1 | TS-3 |
| FR4 Pre-mux routing unchanged | Phase 3 | TS-5 |
| FR5 Detach restores routing | Phase 3, 5 | TS-6, TS-11 |
| FR6 Welcome duplication tolerance | Phase 3 | TS-7 |
| FR7 Remove DIAG | Phase 4 | TS-8 |
| NFR1 No regression (non-mux) | Phase 2 | TS-9 |
| NFR2 Protocol stability | (no code change) | TS-10 |
| NFR3 WebView out of scope | (excluded) | Review |
| NFR4 pump coalesce/budget preserved | Phase 2 | Review of `pump` (FRAME_BUDGET_MS / COALESCE_CAP unchanged) + Manual M6 |
| NFR5 term_core holds no mux constants | Phase 6 | TS-12, TS-13 + static `grep` (no `MUX_OSC_PARAM`/`MUX_PREFIX`/`emterm-mux` in `term_core`) |

## E2E Testing
No project E2E framework. Not applicable.

### Existing E2E Regression (sdd.4 Phase 3.8)
- Skipped: no E2E framework detected for this feature path (SPEC: "No project E2E framework"). The project's broader Docker E2E suite is reserved for final verify (sdd.6), not the TDD cycle.

## Manual Testing (E2E Not Possible)
- [ ] M1: In a mux tab, `emterm image <file>` → inline image renders, no base64 leak.
- [ ] M2: Large image (several MB) assembles correctly across chunk boundaries.
- [ ] M3: `emterm.log` shows no `Kitty image decode failed` / `mux APC decode failed` during the run.
- [ ] M4: Non-mux tab still renders images as before.
- [ ] M5: SIXEL (`emterm image --protocol sixel`) renders in mux.
- [ ] M6: Markdown viewer, plain text, TUI (vim) behave as before in mux (no side effects).

## Performance Verification (if applicable)
- `pump` frame budget (`FRAME_BUDGET_MS = 12ms`) and coalesce cap (`COALESCE_CAP = 1MB`)
  unchanged; extractor adds minimal per-pump overhead (NFR4). Verify by code review of `pump`
  and absence of throughput regression in M6.

## Security Verification (if applicable)
- Not applicable (no new external input surface; APC/OSC parse stays within existing bounds).

## Verification Summary
| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Test Scenarios | 13 | 12 | 0 | 1 (TS-10 review) |
| Success Criteria | 9 | 4 | 0 | 5 |
| Manual checks | 6 | 0 | 0 | 6 |

> Test Scenarios automated = TS-1..9, TS-11, TS-12, TS-13 (12); TS-10 is a
> static/review check. Success Criteria automated = SC-6, SC-7, SC-8, SC-9 (4;
> SC-8/SC-9 added by findings ④/⑤); SC-1/SC-2/SC-3/SC-4/SC-5 still need the
> manual M1/M2/M4/M5/M6 passes.
