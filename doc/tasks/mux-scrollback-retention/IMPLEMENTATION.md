# Implementation Plan: mux Scrollback Retention

## Overview
Convert the per-pane mux buffer from a detach-only ring (64 MiB pre-allocated on detach) into a permanent scrollback buffer (2 MiB pre-allocated at pane creation), write to it on every PTY read, and reorder the reattach snapshot so the scrollback bytes precede the shadow-parser screen.

The plan is split into **three phased commits plus one chore commit**, each individually compile- and test-green, on branch `feat/mux-scrollback-retention`. The phasing keeps every step reviewable and rollback-able even though Phases B and C touch overlapping files.

## Objectives
- Restore pre-detach scrollback on reattach
- Reduce daemon memory footprint from `pane_count × 64 MiB` (peak on detach) to `pane_count × 2 MiB` (steady)
- Preserve mux IPC protocol, WASM-side parser, and existing E2E behavior

## Prerequisites

### Development Environment
- Rust toolchain matching `rust-toolchain.toml` / project default
- Docker Compose (for Docker-first test execution per CLAUDE.md)

### Dependencies
- No new crates
- Internal: existing `vt100` shadow parser, existing `RawPassthroughBuffer`, existing mux IPC framing

## Architecture Overview

### Technology Stack
- **Language**: Rust (Tauri backend)
- **Framework**: Tauri / Tokio
- **Key Libraries**:
  - `vt100` — shadow parser (unchanged role)
  - `portable-pty` — PTY backend (unchanged)
  - `tokio` — async runtime (unchanged)

### Design Approach
- Replace the role of the existing ring buffer (detach-only buffering) with a permanent scrollback buffer that lives for the lifetime of the pane.
- Keep the ring-buffer algorithm (fixed-capacity wrap-around byte queue); only the capacity, location, and write-cadence change.
- Keep `RawPassthroughBuffer` untouched — image / Markdown OSC handling is independent of scrollback.
- Keep the reattach IPC frame shape; only the byte-order inside the resume snapshot payload changes.

### Component Interaction

Steady-state write path: PTY reader writes to `scrollback` on every chunk; the existing `PaneOutputTarget` branch decides whether to also forward to the connected channel or stay silent.

Reattach path: `collect_reattach_data` builds the resume snapshot by concatenating, in order, screen-clear sequence, scrollback bytes, shadow-parser current screen, and passthrough bytes. The scrollback buffer is read but **not cleared**.

## Implementation Phases

### Phase A — Cap the existing ring at 2 MiB (commit `7b668f3`)

**Goal**: The detach-time memory spike shrinks from 64 MiB to 2 MiB per pane without changing any other behavior.

**Files to Modify**:
- `src-tauri/src/mux/ring_buffer.rs` — Change `DEFAULT_RING_CAPACITY` from `64 * 1024 * 1024` to `2 * 1024 * 1024`. Refresh the doc comment.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `DEFAULT_RING_CAPACITY` | Compile-time constant defining the per-pane detach ring size | n/a | Value is `2 * 1024 * 1024` |

**Implementation Steps**:
1. Edit the constant.
2. Update the file-level doc comment and the constant's doc to mention 2 MiB and link the rationale (≈10k lines at 206 cols).
3. Run `cargo test mux::` to confirm no regression.

**Acceptance Criteria**:
- [x] `cargo test mux::` 252/252 green
- [x] `grep -rn "64 \\* 1024 \\* 1024" src-tauri/src/mux/` returns no hits
- [x] No behavior change other than smaller cap (still detach-only buffering)

**Estimated Effort**: small

---

### Phase B — Move the buffer to MuxPane.scrollback (commit `dbe85b0`)

**Goal**: Reposition the buffer so it lives on the pane itself, rename it to match its new purpose, but **keep the write timing (detach-only) and the reattach send order (shadow → ring)** unchanged. This keeps the structural refactor small.

**Files to Create**:
- `src-tauri/src/mux/scrollback_buffer.rs` — Renamed from `ring_buffer.rs`. Hosts `ScrollbackRingBuffer` and `DEFAULT_SCROLLBACK_CAPACITY`. Algorithm body unchanged.

**Files to Modify**:
- `src-tauri/src/mux/mod.rs` — Replace `pub mod ring_buffer;` with `pub mod scrollback_buffer;`.
- `src-tauri/src/mux/session/pane.rs` — Add `pub scrollback: SharedScrollback` field on `MuxPane`; allocate in `MuxPane::new` and `MuxPane::new_test`. Drop the `ring` field from `PaneOutputTarget::Detached`. Update `evaluate_output_target` and `resume_pane_with_permit` to read/clear `pane.scrollback` on the Detached → Connected snapshot. Update tests that built `Detached { … ring }` literals.
- `src-tauri/src/mux/ipc/handlers.rs` — Update Detached construction sites in unit tests to omit `ring`.
- `src-tauri/src/mux/ipc/pty_spawn.rs` — Take `scrollback: SharedScrollback` in `pty_reader_loop`; replace the three per-arm `ring.write(data)` calls (Closed-channel fallback, Detached arm, blocking-send fallback) with `scrollback.lock().unwrap().write(data)`. Update Detached construction sites to omit `ring`.
- `src-tauri/src/mux/ipc/reattach.rs` — Update Detached construction (hidden-reattach branch) and the visible-branch destructure of `ring` so the buffer is read from `pane.scrollback` instead. Send order at this phase is still shadow → scrollback (Phase C reorders it). Tests that constructed `Detached { … ring }` are rewritten to populate `pane.scrollback` after pane creation.

**Files to Remove**:
- `src-tauri/src/mux/ring_buffer.rs` — Replaced by `scrollback_buffer.rs`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `MuxPane.scrollback` | Permanent per-pane scrollback storage shared between the reader thread and the reattach path | Pane is constructed | A `ScrollbackRingBuffer` of capacity `DEFAULT_SCROLLBACK_CAPACITY` is allocated and held under `Arc<StdMutex<…>>` |
| `PaneOutputTarget::Detached` | Marker that the pane has no live channel, with reason and owner identity (no buffering payload) | Pane lacks a live `Sender` | No buffering state inside the variant — buffering lives on `MuxPane.scrollback` |

**Phase B + C grouping note**: Phase B removes the `ring` field. The legacy `ring.write` calls and the `ring`-destructure used to live in pty_spawn.rs / reattach.rs. To keep Phase B compile-clean we redirect those writes to `pane.scrollback` here, even though the semantically-different "always-on write" change is deferred to Phase C. Phase C's job is then to **hoist** the write above the `output_target` match (one write per chunk instead of three), reorder the reattach payload, and stop clearing on reattach.

**Implementation Steps**:
1. `git mv` rename file, then bulk `sed` rename `DetachRingBuffer` → `ScrollbackRingBuffer`, `DEFAULT_RING_CAPACITY` → `DEFAULT_SCROLLBACK_CAPACITY`, `crate::mux::ring_buffer` → `crate::mux::scrollback_buffer` across the touched files.
2. Update `mod.rs`.
3. Drop the `ring` field from the `PaneOutputTarget::Detached` enum and from every construction / destructure site (8 across pane/handlers/pty_spawn/reattach).
4. Add `scrollback: SharedScrollback` to `MuxPane`; initialize in `new` / `new_test`.
5. Wire `scrollback` through `register_pane_and_start_reader` → `pty_reader_loop` as an extra argument.
6. Replace each `ring.write(data)` with `scrollback.lock().unwrap().write(data)` and each `ring.read_all() + ring.clear()` with `pane.scrollback.lock().unwrap().read_all() + clear()` (the clear is still done at this step — Phase C removes it).
7. Update pane.rs tests so they seed `pane.scrollback` after pane creation instead of via `Detached { ring }`.
8. `cargo test mux::` green; `cargo fmt --edition 2024` over the touched files.

**Acceptance Criteria**:
- [x] `MuxPane` carries `scrollback`
- [x] `PaneOutputTarget::Detached` no longer contains `ring`
- [x] Write timing is unchanged from Phase A (still detach-only)
- [x] Reattach send order is unchanged from Phase A (still shadow → scrollback)
- [x] `cargo test mux::` 252/252 green
- [x] No grep hits for `DetachRingBuffer`, `DEFAULT_RING_CAPACITY`, `ring_buffer` in `src-tauri/src/`

**Estimated Effort**: medium

---

### Phase B chore — cargo-fmt drift (commit `fda9e26`)

**Goal**: Capture the incidental fmt drift that `cargo fmt --edition 2024` applied to mux files outside the Phase B structural change.

**Files to Modify**:
- `src-tauri/src/mux/bridge.rs`
- `src-tauri/src/mux/cli.rs`
- `src-tauri/src/mux/daemon.rs`
- `src-tauri/src/mux/ipc/codec.rs`
- `src-tauri/src/mux/ipc/connection.rs`
- `src-tauri/src/mux/ipc/protocol.rs`
- `src-tauri/src/mux/session/manager.rs`
- `src-tauri/src/mux/tmux_conf/converter.rs`

**Implementation Steps**:
1. Already applied by `cargo fmt --edition 2024` during Phase B; only the rename-related files were staged into the Phase B commit. This chore commit picks up the rest.

**Acceptance Criteria**:
- [x] All changes are mechanical (import order, line wrap, trailing parens)
- [x] No semantic changes

**Estimated Effort**: trivial

---

### Phase C — Always-on write, FR5 reorder, no-clear (commit `a1321a7`)

**Goal**: Switch the reader to write into scrollback on every chunk, regardless of attach state, and reorder the resume snapshot so scrollback bytes precede the shadow-parser snapshot. Stop clearing the buffer on reattach so it lives as long as the pane.

**Files to Modify**:
- `src-tauri/src/mux/ipc/pty_spawn.rs` — Hoist `scrollback.lock().unwrap().write(data)` to the top of the per-chunk handler in `pty_reader_loop`, before the `output_target.lock()` block. Remove the three per-arm `scrollback.write` calls (now redundant) and their comments. Update inline comments to describe Phase C semantics.
- `src-tauri/src/mux/ipc/reattach.rs` — In the `collect_reattach_data` visible branch, replace the `build_shadow_parser_snapshot` call (which prepends shadow before scrollback) with explicit composition in the new FR5 order: `ESC[H ESC[2J` → `scrollback` → `shadow` → `passthrough`. Drop the `scrollback.clear()` call. Update logging fields (`ring=` → `scrollback=`). `build_shadow_parser_snapshot` itself stays unchanged — it is still used by the on-demand `RequestPaneSnapshot` path in `handlers.rs`.
- `src-tauri/src/mux/session/pane.rs` — Apply the same FR5 reorder inside `evaluate_output_target` (Detached → Connected branch) and `resume_pane_with_permit` (hidden → visible resume). Drop the `pane.scrollback.lock().unwrap().clear()` call in both paths.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `pty_reader_loop` (per-chunk handler) | Always persist to scrollback; then branch on attach state | Reader thread holds a chunk `data: &[u8]` and `pane_id` | After this handler returns, `pane.scrollback` contains the bytes; downstream `output_target` handling unchanged in semantics |
| `collect_reattach_data` (visible branch) | Build resume snapshot in FR5 order; flip output_target to Connected without clearing scrollback | Session has at least one non-exited pane | Returned `Vec<(PaneId, Vec<u8>)>` carries the new-order bytes; `pane.scrollback.len()` is preserved; `pane.raw_passthrough` is drained |
| `resume_pane_with_permit` (hidden → visible) | Same FR5 ordering as `collect_reattach_data`; emit snapshot via the held permit and flip output_target to Connected | Pane currently Detached with `HiddenByVisibility`-only reason and owner matches the caller | Connected; `pane.scrollback.len()` preserved; passthrough drained |
| `evaluate_output_target` (Detached → Connected via `EvalResult::ResumeWithSnapshot`) | Same FR5 ordering; snapshot returned via `EvalResult` so the caller can enqueue it | Pane is Detached and the caller's transition resolves all detach reasons | Connected; scrollback preserved; passthrough drained |

**Processing Flow** (diagram-convertible):

```
[Per-chunk reader]
  ↓
  scrollback.lock().write(data)        ← FR4 always-on (Phase C hoist)
  ↓
  shadow_parser.process(data)
  ↓
  match output_target:
    Connected(tx) →  try_send / blocking_send / on close: switch to Detached
    Detached     →  capture_passthrough only (scrollback already done)

[Reattach (visible)]
  read pane.scrollback (no clear)
  read pane.shadow_parser.contents_formatted()
  drain + clear pane.raw_passthrough
  flip output_target to Connected
  compose: ESC[H ESC[2J + scrollback + shadow + passthrough
```

**Implementation Steps**:
1. **Hoist scrollback write** in `pty_reader_loop`: insert `scrollback.lock().unwrap().write(data)` between the `let data = &buf[..n];` line and the existing shadow-parser block.
2. **Drop the per-arm scrollback writes** (Closed-channel fallback, Detached arm, backpressure fallback). Leave only the `capture_passthrough` calls there; the comments now read "Scrollback already captured above (FR4)".
3. **Reorder the snapshot composer** in `collect_reattach_data`: read `scrollback_data` and `screen_data` separately, then build `combined` in the new FR5 order with a single `Vec::with_capacity(8 + scrollback + screen + passthrough)`.
4. **Drop `scrollback.clear()`** from the reattach path (FR6). Keep the `raw_passthrough.clear()` (FR9 unchanged).
5. **Apply the same reorder** to `evaluate_output_target` and `resume_pane_with_permit` in pane.rs.
6. `cargo fmt --edition 2024` over the touched files.
7. `cargo test mux::` green.

**Acceptance Criteria**:
- [x] `scrollback.write` happens once per PTY chunk, before the `output_target` match
- [x] No per-arm `scrollback.write` calls remain in `pty_reader_loop`
- [x] `collect_reattach_data` emits bytes in the order `ESC[H ESC[2J` → scrollback → shadow → passthrough
- [x] `resume_pane_with_permit` emits the same FR5 order
- [x] `evaluate_output_target` (Detached → Connected branch) emits the same FR5 order
- [x] No `scrollback.clear()` call remains in any reattach / resume path
- [x] `cargo test mux::` 252/252 green
- [x] `rustfmt --check --edition 2024` clean on touched files

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/mux/
├── mod.rs                              # MODIFIED (Phase B: module decl)
├── scrollback_buffer.rs                # NEW   (Phase B: replaces ring_buffer.rs)
├── ring_buffer.rs                      # REMOVED (Phase B)
├── session/
│   └── pane.rs                         # MODIFIED (Phase B + C: scrollback field, FR5 snapshot composer)
└── ipc/
    ├── handlers.rs                     # MODIFIED (Phase B: Detached construction sites in tests)
    ├── pty_spawn.rs                    # MODIFIED (Phase B + C: scrollback parameter, always-on write)
    └── reattach.rs                     # MODIFIED (Phase B + C: scrollback drain, FR5 snapshot composer)

doc/tasks/mux-scrollback-retention/
├── 要件定義書.md
├── SPEC.md
├── IMPLEMENTATION.md                   # this file
├── VERIFICATION.md
├── VERIFICATION_RESULT.md
├── sdd.yaml
└── tasks.yaml
```

## Testing Strategy

- **Unit**: `scrollback_buffer.rs` keeps the algorithm tests at 100% line coverage (pure data structure). `pane.rs` and `reattach.rs` carry the targeted tests for the new field and the new send order (behavior, not lines).
- **Integration**: `cargo test --manifest-path src-tauri/Cargo.toml` covers cross-module wiring under Docker. 252 / 252 mux tests pass at Phase C commit `a1321a7`.
- **E2E**: No new spec added per SPEC decision. Existing `mux*.e2e.js` specs currently fail on this branch — and on `main @ 2a0d903` with this branch reverted — so the failures are a pre-existing regression tracked separately.
- **Manual**: post-change verification of pre-detach scrollback restoration in a live `bun tauri dev` session is the gating user-facing check.

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none new) | n/a | This change is an internal refactor |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Construction-site oversight (a `Detached { ring }` left somewhere under `cfg(test)`) | Medium | Build fails | grep + compiler enumeration; verified at Phase B |
| Reattach send-order test brittleness | Low | Test flake | Use byte-prefix / `windows().any()` assertions, not exact-equality on the whole payload |
| Hidden-reattach regression (`resume_pane_with_permit`) | Medium | Behavior change for HiddenByVisibility flow | Phase C explicitly applies the FR5 reorder + no-clear to that path too |
| Scrollback lock contention on hot PTY paths | Low | Latency regression | Lock scope kept to the single `write` call; mirrors the existing tight scope used by `raw_passthrough` |
| Bigger Detached → Connected snapshot per pane (now always ≤ 2 MiB instead of 0–64 MiB) | Low | Network bandwidth on reattach | 2 MiB is 32× smaller than the previous cap; acceptable per NFR2 |
| Mux E2E specs `mux.e2e.js` / `mux-reattach.e2e.js` / `mux-multi-session.e2e.js` fail | (already realized) | Cannot guard regressions automatically | Reproduces on a fresh `main` checkout → pre-existing regression, triaged independently |

## Open Questions
- None. All ambiguities resolved during create-spec.

## Success Metrics

- [x] FR1–FR9 implemented and reflected in passing unit tests
- [x] No grep hits for `DetachRingBuffer`, `DEFAULT_RING_CAPACITY`, `ring_buffer.rs` in `src-tauri/src/`
- [~] Existing E2E specs pass without modification — currently 3 mux specs fail, but the failures predate this branch (verified on `main @ 2a0d903`)
- [x] Daemon scrollback memory bound matches `pane_count × 2 MiB` (verified by capacity constant + Rust unit test)
