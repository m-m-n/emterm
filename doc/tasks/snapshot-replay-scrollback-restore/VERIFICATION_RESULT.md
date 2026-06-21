# Verification Result: snapshot-replay-scrollback-restore

検証日: 2026-06-22
検証者: sdd.6-verify (automated structural verification)
対象 feature: snapshot-replay-scrollback-restore
SPEC.md: `doc/tasks/snapshot-replay-scrollback-restore/SPEC.md`
IMPLEMENTATION.md: `doc/tasks/snapshot-replay-scrollback-restore/IMPLEMENTATION.md`
VERIFICATION.md: `doc/tasks/snapshot-replay-scrollback-restore/VERIFICATION.md`

## 0. sdd.5-check で既に確定済みの項目 (再実行しない)

| 項目 | 結果 |
|---|---|
| `cargo check` (default `gui` features) | OK / no warnings |
| `cargo check --no-default-features` (CLI-only, NFR8/SC-8) | OK / no warnings |
| `cargo test --lib -- --test-threads=1` (term_core + src-tauri) | 1903 passed / 0 failed / 3 ignored |
| `cargo test --lib` (term_core 単体) | 682 passed / 0 failed / 7 ignored |
| `cargo fmt` (touched 6 files) | clean (PostToolUse hook 経由) |
| dead-code 検出 | 0 件 |
| 新規 API への参照存在 | すべて参照あり |
| NFR7 ログ点マッピング | 全カバー |

これらは `sdd.5-check` で OK 確定済み。本 sdd.6 では再実行せず、構造確認・要件マッピング・手動 smoke 整理に専念する。

## 1. File Structure Verification

`IMPLEMENTATION.md` で「触る」と書かれている全ファイル + 期待要素の存在確認 (`grep` / `Read` で構造的に検証)。

| ファイル | 期待要素 | 検証結果 | 根拠 |
|---|---|---|---|
| `crates/term_core/src/terminal_core.rs` | `build_from_snapshot` (bypass=on wrapper) | OK | line 629 |
| 〃 | `build_scrollback_only_from_snapshot` (bypass=off wrapper) | OK | line 654 |
| 〃 | `build_from_snapshot_inner` (共通本体) | OK | line 669 |
| 〃 | `merge_scrollback_from(&mut self, other, live_trim_rows)` | OK | line 787 (シグネチャは IMPLEMENTATION.md の進化通り `live_trim_rows: usize` を取る) |
| `crates/term_core/src/ring_buffer.rs` | `prepend_scrollback_rows` (pub(crate)) | OK | line 266 |
| `crates/term_core/src/bench.rs` | `scrollback_restore_bench_2mib_seq` + `#[ignore]` 属性 | OK | line 394-395 (`#[test]` の直下に `#[ignore]`) |
| `src-tauri/src/tabs.rs` | `ScrollbackBuild` 構造体 | OK | line 114 |
| 〃 | `PendingScrollbackRestore` 構造体 | OK | line 131 |
| 〃 | `ScrollbackRestoreOutcome` enum | OK | line 156 |
| 〃 | `Tab.pending_scrollback_restore: Option<…>` フィールド | OK | line 340 |
| 〃 | `Tab::poll_pending_scrollback_restore` | OK | line 906 |
| 〃 | `Tab::apply_scrollback_restore` (private) | OK | line 956 |
| 〃 | `Tab::spawn_scrollback_restore` (private) | OK | line 982 |
| 〃 | `Tab::cancel_pending_scrollback_restore` | OK | line 884 |
| 〃 | `dispatch_offthread_replay` の supersede 分岐 (FR5) | OK | line 705 |
| 〃 | `Tab::resize` の cancel 分岐 (FR5/UC03) | OK | line 2868 |
| 〃 | test helper: `test_has_pending_scrollback_restore` | OK | line 2770 |
| 〃 | test helper: `test_drain_pending_scrollback_restore_for_blocking_recv` | OK | line 2782 |
| 〃 | test helper: `test_force_scrollback_restore_disconnect` | OK | line 2797 |
| 〃 | test helper: `test_scrollback_length` | OK | line 2808 |
| `src-tauri/src/app.rs` | `App::pump_all` 内で `poll_pending_scrollback_restore` を call | OK | line 2783 (`poll_pending_switch` の直後、`changed`/`active_changed` を立てる) |
| `src-tauri/src/window_host.rs` | `WindowEvent::CloseRequested` (line 2146) の `tabs.clear()` 直前で cancel sweep | OK | line 2159-2161 (`for tab in self.app.tabs.iter() { tab.cancel_pending_scrollback_restore(); }`、直後 line 2162 が `self.app.tabs.clear()`) |
| `sdd.yaml` | FR2 含む全要件 `status: ok` | OK | requirements 全 16 個 (FR1-8 + NFR1-8) すべて `ok` |

**判定**: 全期待要素 存在確認 OK。

## 2. SPEC.md FR/NFR Compliance

`sdd.yaml.requirements` のマッピングと、コード位置の実存を突き合わせる。

### Functional Requirements

| Req | タイトル | 実装位置 | 担当 task ID (sdd.yaml) | テスト | 状態 |
|---|---|---|---|---|---|
| FR1 | 2nd-pass spawn after bypass-on swap | `tabs.rs:870` (`apply_offthread_swap` 末尾の `spawn_scrollback_restore` 呼び出し) / `tabs.rs:982` (`spawn_scrollback_restore`) | add-build-scrollback-only-from-snapshot, spawn-2nd-pass-in-apply-offthread-swap, add-tabs-test-helpers-and-integration-tests | TS-5, TS-6, TS-7, TS-13 | OK (sdd.5 で test pass) |
| FR2 | Merge primitive (id re-intern) | `terminal_core.rs:787` (`merge_scrollback_from`) + `ring_buffer.rs:266` (`prepend_scrollback_rows`) | add-prepend-scrollback-rows, add-merge-scrollback-from | TS-1, TS-2, TS-3, TS-4, TS-6 | OK |
| FR3 | Live-drain reconciliation via `base_evicted_total` | `tabs.rs:956-973` (`apply_scrollback_restore`、live_growth = live_now - base、merge に `live_trim_rows` として渡す) | implement-apply-scrollback-restore, add-tabs-test-helpers-and-integration-tests | TS-9, TS-14 | OK |
| FR4 | `poll_pending_scrollback_restore` 非ブロッキング polling | `tabs.rs:906` + `app.rs:2783` | implement-poll-pending-scrollback-restore, wire-app-pump-all | TS-7, TS-8, TS-13 | OK |
| FR5 | Cancel / supersede | dispatch: `tabs.rs:705` / resize: `tabs.rs:2868` / shutdown: `window_host.rs:2159` | cancel-on-supersede-and-resize, wire-app-pump-all, add-tabs-test-helpers-and-integration-tests | TS-8, TS-10 | OK |
| FR6 | Threshold parity (no 2nd-pass below 64 KiB) | sub-threshold は `dispatch_offthread_replay` 自体に入らず synchronous path 経由 → `pending_scrollback_restore` 未 install | add-tabs-test-helpers-and-integration-tests | TS-12, TS-13 | OK |
| FR7 | Spawn-fail / panic → warn only | spawn-fail: `tabs.rs:1037-1047` / panic-disconnect: `tabs.rs:920-932` | spawn-2nd-pass-in-apply-offthread-swap, implement-poll-pending-scrollback-restore, add-tabs-test-helpers-and-integration-tests | TS-11 | OK |
| FR8 | Mark non-duplication | `apply_scrollback_restore` は `rebuilt_core` の `scrollback_slim`/`scrollback_wrapped` のみを `merge_scrollback_from` に渡す。`prompt_marks`/`fold_marks`/`bypass_b_mark_texts` は touch しない (コメント `tabs.rs:947-955` 参照) | implement-apply-scrollback-restore, add-tabs-test-helpers-and-integration-tests | TS-15 | OK |

### Non-Functional Requirements

| Req | タイトル | 実装位置 | テスト/ベンチ | 状態 |
|---|---|---|---|---|
| NFR1 | 1st-pass swap ≤ 60 ms for 2 MiB | 既存の `build_from_snapshot` バイパス path (本 feature では非改修) | `snapshot_replay_bench_2mib_seq` (term_core/bench.rs:168, `#[ignore]`) | 構造確認 OK / 実測は 4.に転記 |
| NFR2 | 2nd-pass + merge ≤ 5 s for 2 MiB | `build_scrollback_only_from_snapshot` + `merge_scrollback_from` を直列計測 | `scrollback_restore_bench_2mib_seq` (term_core/bench.rs:395, `#[ignore]`) | 構造確認 OK / 実測は 4.に転記 |
| NFR3 | UI non-blocking | `try_recv` only (`tabs.rs:910`、`recv`/`join` なし) | TS-7 | OK |
| NFR4 | One in-flight 2nd-pass per tab | `Option<PendingScrollbackRestore>` (`tabs.rs:340`) + supersede in `spawn_scrollback_restore` (`tabs.rs:986`) | TS-8 | OK |
| NFR5 | `scrollback_evicted_total` 単調性 | `merge_scrollback_from` は counter を touch しない (`terminal_core.rs:787` 周辺、コメント line 756-787) | TS-2, TS-9 | OK |
| NFR6 | Equivalence with synchronous bypass-off build | `bypass_plus_merge_equivalence` テストで担保 (TS-6) | TS-5, TS-6 | OK |
| NFR7 | Logging at spawn / cancel / completion / failure | spawn-info: `tabs.rs:1027` / merge-info: `tabs.rs:967` / supersede-warn (new switch): `tabs.rs:707` / supersede-warn (newer offthread): `tabs.rs:988` / resize-warn: `tabs.rs:2870` / shutdown-info: `tabs.rs:887` / panic-warn: `tabs.rs:926` / spawn-fail-warn: `tabs.rs:1042` | 手動 smoke (UC01-UC03) でログ目視 | OK (sdd.5-check で点マッピング全カバー確認済み) |
| NFR8 | WebView (`src/`) untouched | `git diff --stat refactor/native-terminal-hybrid…HEAD -- src/` 空想定 / 本 feature の触ったファイルは `crates/term_core/` + `src-tauri/src/` のみ | SC-9 | OK |

**判定**: 全 FR/NFR について実装位置・テスト ID が確認できた。

## 3. Test Coverage (TS-1 … TS-15)

VERIFICATION.md §Test Scenarios の各 TS が、ソース内に実関数として存在するか確認。

| ID | 役割 | 期待関数 | ファイル / 行 | 存在 |
|---|---|---|---|---|
| TS-1 | merge id re-intern | `test_merge_scrollback_from_intern_rewrites_ids` | `terminal_core.rs:1616` | OK |
| TS-2 | merge preserves `scrollback_evicted_total` | `test_merge_scrollback_from_preserves_evicted_total` | `terminal_core.rs:1663` | OK |
| TS-3 | merge respects ring capacity | `test_prepend_scrollback_rows_fits_capacity_preserves_order` + `test_prepend_scrollback_rows_drops_front_most_incoming_on_overflow` | `ring_buffer.rs:1778` + `:1817` | OK |
| TS-4 | merge no-op on cols mismatch | `test_merge_scrollback_from_cols_mismatch_is_noop` | `terminal_core.rs:1689` | OK |
| TS-5 | `build_scrollback_only_from_snapshot` matches sync | `test_build_scrollback_only_from_snapshot_matches_sync_build` | `terminal_core.rs:1526` | OK |
| TS-6 | `bypass_plus_merge_equivalence` (FR1/NFR6 gate) | `test_bypass_plus_merge_equivalence` | `terminal_core.rs:1723` | OK |
| TS-7 | off-thread switch → scrollback restored | `ts7_offthread_swap_then_restored_scrollback_matches_reference` | `tabs.rs:5665` | OK |
| TS-8 | supersede cancels in-flight restore | `ts8_new_offthread_switch_supersedes_in_flight_restore` | `tabs.rs:5710` | OK |
| TS-9 | concurrent live drain (no duplicates) | `ts9_concurrent_live_drain_trims_rebuilt_tail_no_duplicates` | `tabs.rs:5788` | OK |
| TS-10 | resize during restore cancels (FR5/UC03) | `ts10_resize_cancels_pending_restore_without_respawn` | `tabs.rs:5730` | OK |
| TS-11 | worker panic → warn + state cleared | `ts11_restore_worker_panic_returns_failed_and_clears_state` | `tabs.rs:5753` | OK |
| TS-12 | sub-threshold no 2nd-pass | `ts12_subthreshold_payload_does_not_install_scrollback_restore` | `tabs.rs:5644` | OK |
| TS-13 | at-or-above threshold installs restore | `ts13_offthread_swap_installs_pending_scrollback_restore` | `tabs.rs:5621` | OK |
| TS-14 | `live_growth` exceeds rebuilt → no-op | `ts14_live_growth_exceeds_rebuilt_count_full_noop` | `tabs.rs:5857` | OK |
| TS-15 | merge keeps prompt/fold marks out of live (FR8) | `ts15_merge_does_not_duplicate_prompt_marks_or_fold_marks` | `tabs.rs:5897` | OK |

**判定**: TS-1 … TS-15 全 15 件、実関数として存在 + sdd.5 で全 pass 確定。

## 4. Performance (NFR1, NFR2)

本 sdd.6 では実行しない (`--release` 必要 + 5 秒超の bench で user 判断必要)。

### 状態

- bench コンパイル通過: `sdd.4-implement` で確認済み (term_core 単体テスト 682 passed / 7 ignored — `scrollback_restore_bench_2mib_seq` を含む)。
- 実測: manual / 別途実行
- 想定 NFR1 (1st-pass ≤ 60 ms) と NFR2 (2nd-pass+merge ≤ 5 s) は要 manual 実測

### 実行コマンド (VERIFICATION.md §Performance Verification から転記)

NFR1:
```sh
CARGO_TARGET_DIR=src-tauri/target cargo test --release \
  --manifest-path crates/term_core/Cargo.toml --lib \
  snapshot_replay_bench_2mib_seq \
  -- --nocapture --include-ignored
```
期待: `[bench] build_from_snapshot 2MiB seq-N payload … {per-call}` ≤ 60 ms (reference machine) / in-test の `MUST < 1000 ms` 不変。

NFR2:
```sh
CARGO_TARGET_DIR=src-tauri/target cargo test --release \
  --manifest-path crates/term_core/Cargo.toml --lib \
  scrollback_restore_bench_2mib_seq \
  -- --nocapture --include-ignored
```
期待: per-call total (build bypass-on + build bypass-off + merge) < 5 s; in-test assertion enforces。

## 5. Manual Smoke (UC01, UC02, UC03, UC05)

VERIFICATION.md §Manual Testing から転記したチェックリスト。実機で確認すること。

- [ ] **UC01 — Mux smoke (大容量 scrollback の復元)** — 担当: FR1, FR3, NFR1, NFR2, NFR6, NFR7
    1. `make dev` で emterm 起動
    2. mux session を開く。window A で ≥ 2 MiB の scrollback を生成 (例: `seq 1 500000`)
    3. window B に switch → 別作業 → window A に戻す
    4. 可視 grid 描画後 (~50 ms 以内) 即 scroll up。期待: 一瞬 scrollback は空かも
    5. ~5 秒待って再 scroll up。期待: history が見える
    6. `~/.local/share/net.laser5.app.emterm/logs/emterm.log` を `RUST_LOG=info make dev` で確認。期待: `scrollback restore worker spawned for tab …` + `scrollback restored for tab …: N rows prepended` info 行

- [ ] **UC02 — Rapid switch supersede** — 担当: FR5, NFR4, NFR7
    1. 重い mux window を 2 つ用意、A → B → A を < 1 秒で switch
    2. ログ確認。期待: 最初の 2nd-pass に `scrollback restore cancelled (superseded by …)` warn、2 回目の switch で fresh spawn + merge

- [ ] **UC03 — Resize cancel** — 担当: FR5, UC03, NFR7
    1. 重い mux window に switch
    2. 5 秒の restore window 内で emterm の window resize (border drag)
    3. ログ確認。期待: `scrollback restore cancelled (resize) for tab …` warn。scroll up に history なし (または resize 後の live drain だけ)

- [ ] **UC05 — Small-payload regression (sub-threshold)** — 担当: FR6
    1. scrollback がほぼ無い (< 64 KiB payload) mux window に switch
    2. 期待: switch 直後に scrollback 即可用 (synchronous path 不変)
    3. 期待: ログに `mux-scrollback-restore` spawn 行 が **無い**

(UC04 は VERIFICATION.md に列挙なし — `要件定義書.md` 由来で本 verify では対象外)

## 6. Security Verification

本 feature が触るのは内部 mux IPC (process 内 daemon ↔ frontend) のみ。新規 attack surface はない。

| 項目 | 確認内容 | 状態 |
|---|---|---|
| 外部入力の取り扱い | network input / user file / clipboard など外部由来の payload なし。snapshot payload は同一 trust domain (process 内 daemon) 由来 | OK |
| payload validation | 既存の `build_from_snapshot` が担う (`build_from_snapshot_inner` 経由で bypass on/off 両方で共通) | OK (既存 surface に依存) |
| panic safety | FR7 で担保: spawn-fail (`tabs.rs:1037-1047`) / panic-disconnect (`tabs.rs:920-932`) いずれも `log::warn!` のみで継続。synchronous fallback なし (= history 復元失敗時は静かに諦め、可視 grid は 1st-pass で正しいまま) | OK |
| メモリ peak | 2nd-pass 中は 2 つめの `TerminalCore` を保持 → ~2× live-core RSS の一過性 spike (5 秒以内に baseline 復帰)。scrollback cap = 2 MiB で bound | 文書化済み (SPEC §Security) |
| 2nd-pass worker の live core 参照 | `spawn_scrollback_restore` は payload を `clone` して worker に move、live core への参照なし (`tabs.rs:1005`) | OK |

**判定**: 新規攻撃面なし。

## 7. SC-1 … SC-9 Success Criteria

| ID | Criterion | 検証方法 | 状態 |
|---|---|---|---|
| SC-1 | All FRs implemented + covered | TS-1 … TS-15 全 pass + `merge_scrollback_from` / `pending_scrollback_restore` 参照あり | OK |
| SC-2 | NFR1 1st-pass 非リグレッション | `snapshot_replay_bench_2mib_seq` を `--release` で実測 | pending (manual) |
| SC-3 | NFR2 2nd-pass within budget | `scrollback_restore_bench_2mib_seq` を `--release` で実測、< 5 s | pending (manual) |
| SC-4 | NFR6 equivalence with sync build | TS-6 (`test_bypass_plus_merge_equivalence`) 存在 + sdd.5 で pass | OK |
| SC-5 | Threshold contract drift eliminated | TS-12 + TS-13 + TS-7 全 pass | OK |
| SC-6 | `scrollback_evicted_total` 単調性 | TS-2 (unit) + TS-9 (integration) 全 pass | OK |
| SC-7 | No new `cargo` warnings | `cargo check` (default + `--no-default-features`) 両方 warning-free (sdd.5) | OK |
| SC-8 | CLI-only build unaffected | `--no-default-features cargo check` clean (sdd.5) | OK |
| SC-9 | WebView (`src/`) untouched (NFR8) | 本 feature の触ったファイル一覧に `src/` 配下なし (本 sdd.6 で grep 確認) + branch policy `refactor/native-terminal-hybrid` 配下で WebView 触らず | OK |

**判定**: 自動検証 7/9 OK、残 2 件 (SC-2, SC-3) は bench 実測待ち。

## 8. Overall

### 8.1 Status

| カテゴリ | 結果 |
|---|---|
| File structure verification | OK (全ファイル/全期待要素存在) |
| FR1-8 compliance | OK (全 8 件、テスト & 実装位置確認) |
| NFR1-8 compliance | OK (全 8 件、NFR1/NFR2 のみ実測 pending) |
| TS-1 … TS-15 test coverage | OK (全 15 件存在 + sdd.5 で pass) |
| Performance NFR1/NFR2 | pending (manual `--release` bench 実測待ち) |
| Manual smoke UC01/02/03/05 | pending (実機操作 4 件) |
| Security | OK (新規攻撃面なし) |
| SC-1 … SC-9 | 自動 7/9 OK、残 2 件 (SC-2/SC-3) bench 実測 pending |

### 8.2 自動検証サマリ

- 全自動検証項目 pass
- sdd.5 の結果 (1903 tests pass + warning-free + CLI-only clean) を踏襲
- 構造確認 (file/関数/フィールド 位置) すべて IMPLEMENTATION.md と一致

### 8.3 手動検証残件

- bench 実測 2 件 (NFR1: `snapshot_replay_bench_2mib_seq`, NFR2: `scrollback_restore_bench_2mib_seq`)
- 実機 smoke 4 件 (UC01, UC02, UC03, UC05)

### 8.4 残存 risk

- **bench 実測時の reference machine 差**: NFR2 budget 5 s に対し、計画時 baseline は ~4040 ms (2nd-pass 単体)。merge cost が 500 ms 超なら NFR2 が flake する可能性 (IMPLEMENTATION.md §Open Questions と一致)
- **manual smoke で RUST_LOG=info が必要**: release build は warn 以上のみ persist。`scrollback restored …` info 行は `RUST_LOG=info make dev` でないと出ない (NFR7 の運用前提)
- **PSReadLine 等 Windows 側固有問題は本 feature 範囲外** (既知の MEMORY.md 記載通り)

### 8.5 sdd.yaml 整合性

- `workflow[].verify.status = in_progress` → 本 sdd.6 完了で `completed` 候補
- `requirements.*.status` 全 16 項目 `ok` 確定済み (sdd.5 で flip 済み)
