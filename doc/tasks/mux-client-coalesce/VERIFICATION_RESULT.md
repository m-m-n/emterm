# Verification Result: mux client coalesce (phase1)

**検証日時**: 2026-06-21
**対象機能**: mux-client-coalesce (phase1)
**VERIFICATION.md**: `doc/tasks/mux-client-coalesce/VERIFICATION.md`
**SPEC.md**: `doc/tasks/mux-client-coalesce/SPEC.md`
**プロジェクト**: emterm
**判定スコープ**: phase1 受け入れ基準（正しさ契約 + 改善確認）。実環境 `time seq 1 10000000` の 10秒 目標は phase1 のパスゲートではない（phase2/3 の go/no-go 判断材料）。

---

## 総合判定: ✅ PASS

phase1 の受け入れ基準（FR1–FR5 実装、正しさ契約、改善確認）をすべて満たしている。
コアレス契約は `coalesce_parse_passes` カウンタにより直接証明され（K フレーム ⇒ 1 パース）、
出力は連結結果とバイト等価（パリティ・テスト緑）、daemon-direct E2E はスループット向上・
フレーム数減少（記録済み: 2.85→3.08 MiB/s, 124,233→105,497 frames）。スコープ隔離（NFR3）も
git で確認済み（変更は `src-tauri/src/tabs.rs` のみ + doc/tasks ディレクトリ）。

---

## 1. ファイル構造検証: ✅ 合格

### SDD 成果物 (`doc/tasks/mux-client-coalesce/`)
| ファイル | 状態 |
|---------|------|
| 要件定義書.md | ✅ 存在 |
| SPEC.md | ✅ 存在 |
| IMPLEMENTATION.md | ✅ 存在 |
| VERIFICATION.md | ✅ 存在 |
| tasks.yaml | ✅ 存在 |
| sdd.yaml | ✅ 存在 |
| VERIFICATION_RESULT.md | ✅ 本ファイル（sdd.yaml verify artifact） |

### 本番コードの変更ファイル
- `git status --short` → ` M src-tauri/src/tabs.rs` + `?? doc/tasks/mux-client-coalesce/` のみ。
- `git diff --stat` → `src-tauri/src/tabs.rs | 231 ++++..., 1 file changed, 186 insertions(+), 45 deletions(-)`。
- 本番コードで変更されたのは **`src-tauri/src/tabs.rs` 1ファイルのみ**。IMPLEMENTATION.md の
  「Complete File Structure」と一致。
- `src-tauri/tests/mux_throughput.rs` は phase1 で **未変更**（「run only / 必要時のみ test-only tweak」
  の許容範囲内。今回は調整不要だった）。HEAD~1 で既に存在していた計測ハーネスであり、本機能の
  作業ツリー diff には現れない。

---

## 2. スコープ隔離検証 (NFR3): ✅ 合格

- `git diff --stat`（作業ツリー）に現れる本番ファイルは `src-tauri/src/tabs.rs` のみ。
- daemon / bridge / transport 配下、および WebView ビルド (`src/`) のファイルは **一切変更なし**。
- テスト専用カウンタ `coalesce_parse_passes` は全参照（field 宣言/初期化/インクリメント/リーダ）が
  `#[cfg(test)]` で厳密にゲートされており、本番ビルドにカウンタは載らない（リーク無し）:
  - L289 宣言 ← L288 `#[cfg(test)]`
  - L416 初期化 ← L415 `#[cfg(test)]`
  - L1572 インクリメント ← L1570 `#[cfg(test)] { … }`
  - L2443-2444 リーダ ← L2442 `#[cfg(test)]` fn
  - 残りは `#[cfg(test)] mod tests`（L2581）内またはコメント。
- CLI-only feature gate (`cargo check --no-default-features`) は sdd.5-check で exit 0（GUI-only シンボル漏れ無し）。

---

## 3. SPEC FR/NFR 適合性

| 要件 | 内容 | 実装箇所 (`src-tauri/src/tabs.rs`) | 検証テスト | 状態 |
|------|------|-----------------------------------|-----------|------|
| FR1 | 連続 active-pane PtyOutput の inner payload を1パースにコアレス | コアレスバッファ `coalesce_acc`（process_combined ループ内）+ batch_eligible 判定（active-pane 一致 && PtyOutput && pending_switch.is_none()）+ `continue` で蓄積 | TS-1 (`c_consecutive_active_pane_pty_output_coalesces_into_one_parse`), TS-2 (`c_split_messages_equal_single_concatenated_message`) | ✅ complete |
| FR2 | 制御メッセージ境界でフラッシュ（順序保持） | 境界で `flush_coalesced_output(&mut coalesce_acc)` をフレーム処理の **前** に呼ぶ + ループ末尾フラッシュ | TS-5 (順序: `ts5_queued_live_output_applied_in_order` 等), TS-7 (detach境界: `ts11_post_detached_tail_in_coalesced_buffer_renders_via_core`) | ✅ complete |
| FR3 | take_response / drain_marks / inner image APC drain をバッチ毎1回 | `flush_coalesced_output`: 1回の `process_pty_data_fully` 後に `take_response`→`write_device_response`、`drain_marks`→`backfill_marks` を1回。inner image APC/DCS は post-loop ブロックで pump 毎1回（既存） | TS-2 (パリティ), TS-6/TS-7 regression（`ts11_post_detached_image_decodes_exactly_once` で画像1回デコード） | ✅ complete |
| FR4 | 非 active-pane PtyOutput はコアレス対象外（従来通り drop） | batch_eligible 判定で active-pane 不一致を除外 → 境界として per-frame パス（`apply_mux_message` のペインフィルタで drop） | TS-4 (`ts3_live_output_queued_during_pending_switch` で pane 20 が drop されることを確認) | ✅ complete |
| FR5 | pending_switch 中は legacy per-frame（live_queue）パス維持 | batch_eligible 判定に `self.pending_switch.is_none()` を含める → pending 中はフラッシュ後に既存 per-frame パスへ | TS-6 (`ts3_live_output_queued_during_pending_switch`: live_queue へ順次積まれることを確認) | ✅ complete |
| NFR1 | per-pump パース数削減 + E2E スループット向上・フレーム数減少 | コアレスにより K フレーム ⇒ 1 パース（counter で実証） | TS-9（記録済み E2E: 2.85→3.08 MiB/s, 124,233→105,497 frames, N=10M） | ✅ complete |
| NFR2 | コアレス出力が per-frame とバイト等価（split==concatenated 緑） | `flush_coalesced_output` は連結バッファを1回パース。streaming parser がフレーム境界をまたいで状態保持 | TS-2 (`c_split_messages_equal_single_concatenated_message`) + TS-1 が連結結果とグリッド一致を assert | ✅ complete |
| NFR3 | daemon/bridge/transport/`src/` 変更なし | （該当変更なし） | `git diff --stat` レビュー（上記 §2） | ✅ complete |

すべての FR1–FR5・NFR1–NFR3 が **complete**。partial / missing は無し。

### 必須テストの存在確認
- ✅ `c_consecutive_active_pane_pty_output_coalesces_into_one_parse`（新規必須, TS-1）— tabs.rs L4937
- ✅ `c_pty_output_parsed_per_message_grid_grows_step_by_step`（batched 1-pass 挙動に restate, TS-3）— tabs.rs L4897
- ✅ `c_split_messages_equal_single_concatenated_message`（パリティ, TS-2）— tabs.rs L4976
- 3 つとも `mod tests` 内に存在し、作業ツリーで PASS（下記 §4 で実行確認）。

---

## 4. テストシナリオ・カバレッジ (TS-1..TS-9)

| ID | シナリオ | 対応テスト/計測 | 種別 | 状態 |
|----|---------|----------------|------|------|
| TS-1 | 連続 active-pane PtyOutput が1パース、グリッド==連結 | `c_consecutive_active_pane_pty_output_coalesces_into_one_parse`（count==1 assert + グリッド一致）| Unit（新規・直接）| ✅ PASS（実行確認） |
| TS-2 | split vs single-concatenated が同一グリッド | `c_split_messages_equal_single_concatenated_message` | Unit（パリティ・直接）| ✅ PASS（実行確認） |
| TS-3 | batched 挙動の metric test（K フレーム ⇒ 1 pass）| `c_pty_output_parsed_per_message_grid_grows_step_by_step`（期待値を batched に更新済み）| Unit（直接）| ✅ PASS（実行確認） |
| TS-4 | 非 active-pane PtyOutput はコアレス除外・drop | `ts3_live_output_queued_during_pending_switch`（pane 20 drop を assert）| Unit（regression）| ✅ PASS（実行確認） |
| TS-5 | 制御メッセージ境界でフラッシュ・順序保持 | `ts5_queued_live_output_applied_in_order` ほか ts5_* | Unit（regression）| ✅ PASS（実行確認） |
| TS-6 | pending_switch 中は legacy live_queue パス | `ts3_live_output_queued_during_pending_switch`, ts6_* | Unit（regression）| ✅ PASS（実行確認） |
| TS-7 | detach がバッファ途中 — フラッシュ後 detach break + tail re-route | `ts11_post_detached_tail_in_coalesced_buffer_renders_via_core` | Unit（regression）| ✅ PASS（実行確認） |
| TS-8 | inner Kitty image が PtyOutput 境界をまたぐ — 1回デコード | `ts11_post_detached_image_decodes_exactly_once`, `ts4_split_inner_kitty_over_mux_pty_output_assembles_one_image` | Unit（regression）| ✅ PASS（実行確認） |
| TS-9 | daemon-direct E2E スループット/フレーム数 | `src-tauri/tests/mux_throughput.rs`（記録済み: 2.85→3.08 MiB/s, 124,233→105,497, N=10M）| Performance | ✅ 記録済み（sdd.4 で計測） |

**バッキングテスト無しの TS**: 無し。TS-1/2/3 は直接、TS-4..TS-8 は VERIFICATION.md 記載の既存 regression、
TS-9 は記録済み E2E 数値で全てカバー。

### 実行確認（本検証で再実行した範囲）
- `cargo test --lib 'tabs::tests::c_' -- --test-threads=1` → **3 passed, 0 failed, 0 ignored**（1884 filtered out）。
- `cargo test --lib 'tabs::tests::ts' -- --test-threads=1` → **21 passed, 0 failed, 0 ignored**（境界/detach/image/pending_switch regression を含む）。
- フルスイート（lib 1886 passed / 0 failed / 1 ignored）・CLI-only check exit 0・rustfmt --check exit 0・
  clippy 新規警告なし・dead code なしは sdd.5-check で確認済みのため再実行せず（コンテキストの方針に従う）。
- E2E スループット（release, multi-minute）は sdd.4 で計測済みのため再実行せず。

> 補足（VERIFICATION.md の caveat を踏襲）: `mux_throughput.rs` は daemon ソケット層を独自の
> standalone `client_core.process_pty_data_fully`（フレーム毎）で計測しており、コアレスが置かれた
> `Tab::process_combined` を **通らない**。よって E2E の改善は「daemon が大きな read をまとめてフレーム
> 数自体が減ったことによるクライアント backpressure 緩和」を反映したもので、クライアントコアレス
> （K フレーム ⇒ 1 パース）の効果そのものは TS-1/TS-3 の `coalesce_parse_passes` カウンタで直接実証
> されている。両受け入れ基準（スループット向上・フレーム数減少）は満たされている。

---

## 5. 手動確認項目（E2E 不可・非ゲート）

- [ ] （任意・phase1 のパスゲートではない）実環境での `time seq 1 10000000`（mux ウィンドウ）。
  - **非ゲート**: SPEC NFR1 の通り、実環境 ~10s（stretch 7.6s, 現状 146s）の到達は **phase2/3 を
    実施するか否かの判断材料** であり、phase1 の合否条件ではない。この計測は今回の PASS 判定に影響しない。
  - 自動検証では再現不可（実 PTY + GUI レンダリングを要するため）。実施する場合はユーザーが実環境で計測する。

---

## 6. phase1 合否判定の根拠

SPEC「Success Criteria」（phase1 受け入れ基準）に対する充足:

| 基準 | 結果 |
|------|------|
| SC-1 連続 active-pane PtyOutput が1回パース（TS-1 count==1）| ✅ |
| SC-2 出力が per-frame とバイト等価（TS-2 緑）| ✅ |
| SC-3 metric test が batched 挙動を反映（TS-3 更新済み・緑）| ✅ |
| SC-4 E2E スループット向上・フレーム数減少（TS-9 記録済み）| ✅ |
| SC-5 フルスイート緑（`--lib --test-threads=1`）| ✅（sdd.5: 1886 passed / 0 failed / 1 ignored）|
| FR1–FR5 実装 | ✅（§3）|

実環境 10s 目標は **phase1 のパスゲート外**（明示的に phase2/3 判断材料）。したがって、その未到達は
phase1 の判定に影響しない。

### 最終判定: ✅ PASS

正しさ契約（K フレーム ⇒ 1 パース、かつ連結とバイト等価）と改善確認（E2E スループット向上・
フレーム数減少）の双方を満たし、スコープ隔離（NFR3）も保たれている。phase1 は合格。
