# Verification Document: mux-agent-tab-cycle

## Overview

**Feature**: mux-agent-tab-cycle
**SPEC.md**: `feature-docs/mux-agent-tab-cycle/SPEC.md`
**IMPLEMENTATION.md**: `feature-docs/mux-agent-tab-cycle/IMPLEMENTATION.md`

## Build Verification

- rust-native:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  — expected: exit code 0, no errors.
- webview-typescript: `bun run build:viewer && bun run build:settings`
  — expected: exit code 0.
- **NFR1 — CLI-only feature check**:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  — expected: exit code 0 (the `gui` feature gate must not leak into the
  CLI-only build).

## Test Verification

- rust-native:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`
- webview-typescript: `bun run typecheck && bun test`
- Coverage target: no numeric project floor; every scenario below is
  covered by at least one automated test or an explicit manual item.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Several mux windows, a subset with a reported agent state; the key operation is repeated | Only qualifying windows are visited, in display order | Unit |
| TS-2 | Qualifying and non-qualifying windows alternate | Non-qualifying windows are skipped | Unit |
| TS-3 | Operation from the last qualifying window | Wraps around to the first qualifying window | Unit |
| TS-4 | Exactly one qualifying window | Lands on / stays on it; never activates a non-qualifying window | Unit |
| TS-5 | Zero qualifying windows | Active window unchanged (no-op) | Unit (resolution level) + Manual backup (MT-1) |
| TS-6 | Non-mux tab active | Nothing happens (no-op) | Integration (dispatch guard) or Manual (MT-2) |

## Code Quality Verification

- Format (Rust): `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- Format / lint (TypeScript): `bunx biome check .`

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | The key operation switches only among mux windows with a running agent | TS-1, TS-2 |
| SC-2 | Windows without a running agent are skipped while cycling | TS-2 |
| SC-3 | Non-mux active tab → no-op | TS-6 |
| SC-4 | No qualifying window → no-op | TS-5 |
| SC-5 | Display order with wrap-around from last to first | TS-1, TS-3 |
| SC-6 | All test scenarios (TS-1 … TS-6) pass | Test commands above |
| SC-7 | `--no-default-features` (CLI-only) build succeeds | NFR1 check in Build Verification |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0002 | TS-1, TS-4; MT-3 (rebinding via settings) |
| FR2 | task0001 | TS-1, TS-3, TS-4 |
| FR3 | task0001 | TS-1, TS-2 |
| FR4 | task0001 | TS-6 |
| FR5 | task0001 | TS-5 |
| FR6 | task0001 | TS-1, TS-2 (qualification-predicate unit tests) |
| NFR1 | task0001 | CLI-only feature check (Build Verification) |
| NFR2 | task0001 | Review: resolution happens only in the key-event path; no timers or polling added |
| NFR3 | task0002 | ja/en locale keys present and resolving; review of added strings |

## E2E Testing

No E2E framework exists in this project; end-to-end confirmation is manual
(next section).

## Manual Testing (E2E Not Possible)

- [ ] MT-1: 実機で mux を起動し、複数 window の一部でエージェント状態が報告
      されている状態を作る。prefix（既定 Ctrl+Z）→ Ctrl+A の繰り返しで対象
      window だけを表示順に巡回し、末尾から先頭へラップアラウンドすること。
      全 window の状態をクリアした後は同じ操作でアクティブ window が変化し
      ないこと（TS-1 / TS-3 / TS-5 の実機確認）。
- [ ] MT-2: 非 mux タブをアクティブにして prefix → Ctrl+A を押しても何も
      起きないこと（TS-6 の実機確認）。
- [ ] MT-3: 設定パネルの mux セクションに next-agent-window の行が日英ラベル
      で表示されること。キーバインドを変更すると新キーで巡回が動作し、既定の
      Ctrl+A では動作しなくなること（FR1 の設定変更可能性）。

## Performance / Security Verification

- NFR2 (event-driven): verified by review — no timers, no periodic wakeups,
  no cached qualify lists; resolution is computed only on the key event.
- Security: the feature adds no input parsing, no persistence and no
  external interface (SPEC Security Considerations) — confirm during review
  that this remains true of the diff.

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build (incl. NFR1 check) | 3 | 3 | 0 | 0 |
| Test scenarios (TS-1 … TS-6) | 6 | 6 | 0 | 2 (backup for TS-5 / TS-6) |
| Code quality | 2 | 2 | 0 | 0 |
| Manual (MT-1 … MT-3) | 3 | 0 | 0 | 3 |
