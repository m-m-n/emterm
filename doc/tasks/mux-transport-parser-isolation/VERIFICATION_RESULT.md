# 実装自動検証レポート (sdd.6-verify)

**検証日時**: 2026-06-20 00:18 JST (2026-06-20T00:18:03+09:00)
**対象機能**: mux Transport/Content Parser Isolation (`mux-transport-parser-isolation`)
**VERIFICATION.md**: `doc/tasks/mux-transport-parser-isolation/VERIFICATION.md`
**SPEC.md**: `doc/tasks/mux-transport-parser-isolation/SPEC.md`
**プロジェクト**: eMterm（Rust ネイティブターミナル + 子WebView）
**検証種別**: sdd.6 包括検証（ビルド/テスト/フォーマット/静的解析は sdd.5-check 済みのため再実行せず）

---

## 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ファイル構造 | OK | 作成1 / 変更8（増分対象4ファイル全て実在・想定変更入り） |
| SPEC.md 適合性 | OK | FR1–FR7 / NFR1–NFR5 実装済み、SC-1〜SC-9 の自動検証可能分は全達成 |
| TS カバレッジ | OK | TS-1〜TS-13 が実テストとして存在（TS-10 は静的レビュー、TS-11/12/13 が今回焦点） |
| NFR5 静的 grep | OK | term_core に mux プロトコル定数なし、9999 認識は app 層へ移動 |
| FR5 / finding ④ | OK | `process_combined` が detach tail を `self.core` に再ルート（TS-11 PASS） |
| NFR5 / finding ⑤ | OK | extractor は `mux_ipc::protocol` 注入、`on_osc` で 9999 受理（TS-13 PASS） |
| TS-10 プロトコル不変 | OK | mux_ipc / bridge / daemon は直近コミットで未変更 |
| NFR4 パフォーマンス | OK（レビュー） | `FRAME_BUDGET_MS=12` / `COALESCE_CAP=1MB` 不変 |
| セキュリティ | N/A | 新規外部入力面なし（APC/OSC は既存境界内） |
| ビルド/テスト/fmt/静的 | 再実行せず | sdd.5-check 済み（default+CLI check OK / test 1870+662 passed / fmt-check OK / デッドコードなし） |
| E2E | N/A | プロジェクト E2E フレームワーク無し（VERIFICATION.md 記載どおり） |

**総合評価**: すべての自動検証項目に合格。手動 GUI 確認（M1〜M6 + 追加 ④/⑤）は未実施のため残課題として列挙。

---

## ファイル構造検証

VERIFICATION.md「Files to Create / Files to Modify」記載のファイルは全て実在。今回の増分（P5/P6）対象4ファイルについて、想定の変更が入っていることをコードレビューで確認した。

### 作成ファイル (1)
- OK `crates/term_core/src/mux_apc_extractor.rs` — 独立トランスポート抽出器（`MuxApcExtractor`）

### 変更ファイル（増分対象4ファイル＝今回の焦点）
- OK `crates/term_core/src/mux_apc_extractor.rs`
  - `new(osc_param: u16, prefix: &'static str)` 注入コンストラクタ（`mux_apc_extractor.rs:64`）
  - `feed_with_offsets(&[u8]) -> Vec<(Vec<u8>, usize)>` 追加（フレーム末尾オフセット報告、FR5用）。`feed` はこれに委譲
  - `MUX_OSC_PARAM` / `MUX_PREFIX` 定数および `drift_*` テストは削除済み（grep で 0 件）
- OK `crates/term_core/src/osc_handler.rs`
  - OSC 9999 `emterm-mux;` 特別扱い（旧 `fire_apc_callback`）撤去。`9999 => 102` の単純な action-type マップのみ（`osc_handler.rs:90`）。`fire_apc_callback`/`emterm-mux` 文字列は handle_osc_internal に残存せず
- OK `src-tauri/src/tabs.rs`
  - `Tab` に `mux_apc_extractor: term_core::MuxApcExtractor` フィールド（`tabs.rs:282`）
  - 構築は `MuxApcExtractor::new(mux_ipc::protocol::MUX_OSC_PARAM, mux_ipc::protocol::APC_PREFIX)`（`tabs.rs:394-396`）= cross-crate SSOT 注入
  - `process_combined`: `mux_session_name.is_some()` 時に `feed_with_offsets` で抽出（`tabs.rs:1541`）。detach tail 再ルート（`tabs.rs:1649-1654`）
  - detach 時に `mux_apc_extractor.reset()`（`tabs.rs:1360`）＋ `reset_frame_for_replay(b"")`（grid クリア）
- OK `src-tauri/src/callbacks.rs`
  - `OSC_MUX_INBAND: u8 = 102` 定数（`callbacks.rs:81`）
  - `on_osc` の `OSC_MUX_INBAND` アーム（`callbacks.rs:492-500`）: `data.starts_with(APC_PREFIX)` を app 層で判定し `pending_apc` へ投入

### その他の宣言変更ファイル（IMPLEMENTATION.md / SPEC.md 記載）
- OK `crates/term_core/src/lib.rs` — `pub mod mux_apc_extractor;`（34行）＋ `pub use mux_apc_extractor::MuxApcExtractor;`（59行）
- OK `crates/term_core/src/terminal_core.rs` — `parser_mid_sequence()` 撤去（grep 0 件）
- OK `src-tauri/src/mux/apc.rs` — `try_decode_emterm_mux` 失敗ログを単純 warn に復元（`apc.rs:52` `"mux APC decode failed: {e} (payload len = {})"`、DIAG なし）

---

## SPEC.md 適合性検証

### 機能要件 (FR) カバレッジ

| 要件 | 実装状況 | 根拠 |
|------|----------|------|
| FR1 専用トランスポート抽出器 | OK | `process_combined` mux 分岐が `feed_with_offsets` で APC/OSC ペイロードのみ抽出（`tabs.rs:1532-1543`） |
| FR2 inner-content-only `self.core` | OK | mux 確立後の outer は extractor へ。`self.core` は `apply_mux_message` PtyOutput アーム経由の inner のみ |
| FR3 APC + OSC fallback | OK | extractor が APC と OSC `osc_param` 一致＋`prefix` 始まりを正規化（`mux_apc_extractor.rs:116-129`） |
| FR4 pre-mux ルーティング不変 | OK | `mux_session_name` None 時は `process_outer_via_core`（`tabs.rs:1544-1546`） |
| FR5 detach がルーティングを復元（焦点・finding ④） | OK | 下記「FR5 詳細」参照 |
| FR6 Welcome 二重配送許容 | OK | `first_welcome` ガード（`tabs.rs:995`）。TS-7 PASS |
| FR7 DIAG 撤去 | OK | touched ファイルに DIAG 診断ログなし（unicode_width.rs の "DIAGONAL" はコメント、無関係） |
| NFR1 非mux退行なし | OK | TS-9（非mux Kitty デコード）PASS |
| NFR2 プロトコル安定 | OK | TS-10。mux_ipc / bridge / daemon 未変更 |
| NFR3 WebView 対象外 | OK | `src/` 未変更（ブランチ運用方針どおり） |
| NFR4 pump coalesce/budget 保持 | OK | `FRAME_BUDGET_MS=12` / `COALESCE_CAP=1MB` 不変（`tabs.rs:1413,1423`） |
| NFR5 term_core に mux 定数なし（焦点・finding ⑤） | OK | 下記「NFR5 詳細」参照 |

### FR5 詳細（finding ④ / SC-8 / TS-11）

`process_combined`（`src-tauri/src/tabs.rs:1523-1678`）が、単一 coalesced buffer 内で
`[inner PtyOutput frame][Detached frame][plain shell bytes]` を受けたとき:

1. `feed_with_offsets` が各フレームの末尾オフセットを報告（`tabs.rs:1541`）
2. 抽出フレーム適用ループ内で `mux_session_name` の Some→None 遷移を監視し、遷移を起こした
   フレームの `end_offset` を `detach_tail_start` に記録（`tabs.rs:1609-1621`）
3. ループ後、`detach_tail_start` 以降の tail を `process_outer_via_core(&combined[tail..])`
   で `self.core` に再ルート（`tabs.rs:1649-1654`）

detach アームは `reset_frame_for_replay(b"")` で grid をクリアし extractor も reset 済み
（`tabs.rs:1360,1381`）なので、再ルートされた tail が再描画される。
→ **TS-11 (`ts11_post_detached_tail_in_coalesced_buffer_renders_via_core`) で検証済み**。
テストは `detached-prompt$` が grid に描画され、`emterm-mux` プレフィックスが grid に漏れないことを assert。

### NFR5 詳細（finding ⑤ / SC-9 / TS-12 + TS-13 + static grep）

**静的 grep 結果（term_core/src 配下）**:
- `MUX_OSC_PARAM` / `MUX_PREFIX` 定数定義: **0 件**（残存は `mux_apc_extractor.rs` の doc-comment と
  テストコードが `mux_ipc::protocol::MUX_OSC_PARAM` を注入参照しているのみ＝ハードコピーではない）
- `drift_*` テスト: **0 件**
- `emterm-mux` リテラル: 本体コードには無し（doc-comment / テストのみ）
- `9999` リテラル: `osc_handler.rs:90` の `9999 => 102` マップ（action-type 変換のみ。プロトコル認識ではない）

**extractor の注入化**: `MuxApcExtractor::new(osc_param, prefix)` がコンストラクタ注入
（`mux_apc_extractor.rs:64`）。production caller (`tabs.rs:394-396`) は
`mux_ipc::protocol::{MUX_OSC_PARAM, APC_PREFIX}`（SSOT = `crates/mux_ipc/src/protocol.rs:15,24`）を渡す。
→ **TS-12 (`ts12_injected_osc_param_and_prefix_are_used`)** で、注入値 `1234`/`"myprefix;"` を使い、
デフォルト値 `9999`/`"emterm-mux;"` のフレームが破棄されることを assert（ハードコード不在を証明）。

**OSC 9999 認識の app 層移設**: `handle_osc_internal` は `9999 => 102` マップのみで
特別な `fire_apc_callback` 呼び出しは撤去済み。認識は `callbacks.rs` の `on_osc` の
`OSC_MUX_INBAND` アーム（`callbacks.rs:492-500`）で `APC_PREFIX` 判定し `pending_apc` へ投入。
→ **TS-13** = `osc_9999_emterm_mux_inband_routed_to_pending_apc`（mux frame が pending_apc へ）
＋ `osc_9999_non_mux_prefix_is_dropped`（非mux prefix は破棄）。
pre-mux Windows ConPTY OSC 9999 Welcome 経路が `term_core` を経ずに維持されることを検証。

### Success Criteria (SC-1〜SC-9) 達成状況

| ID | 基準 | 検証方法 | 状況 |
|----|------|----------|------|
| SC-1 | mux inline Kitty 画像描画・base64 漏れなし | M1 + TS-4 | TS-4 自動 PASS（最終確認は手動 M1） |
| SC-2 | 大画像がチャンク境界またぎで組み立て | M2 | 手動 M2 未実施 |
| SC-3 | 非mux 経路に影響なし | TS-9 + M4 | TS-9 自動 PASS（手動 M4 残） |
| SC-4 | SIXEL が mux で描画 | M5 | 手動 M5 未実施 |
| SC-5 | Markdown/text/TUI パリティ | M6 | 手動 M6 未実施 |
| SC-6 | DIAG 診断撤去 | TS-8 | 自動 PASS（grep 0 件） |
| SC-7 | split-chunk 回帰テスト追加・通過 | TS-4 | 自動 PASS |
| SC-8 | (FR5/④) detach 後シェルbytesが `self.core` 描画 | TS-11 | **自動 PASS（今回焦点）** |
| SC-9 | (NFR5/⑤) term_core に mux 定数なし・注入・app層認識 | TS-12+TS-13+grep | **自動 PASS（今回焦点）** |

自動検証可能な SC-6/SC-7/SC-8/SC-9 は全達成。SC-1〜SC-5 は GUI 実機での手動確認（M1/M2/M4/M5/M6）に依存。

---

## テストシナリオ TS-1〜TS-13 カバレッジ

| ID | テスト所在 | 実テスト名 | 状況 |
|----|-----------|-----------|------|
| TS-1 | `mux_apc_extractor.rs` | `ts1_complete_apc_frame_returned_intact` | 存在・前回 PASS |
| TS-2 | `mux_apc_extractor.rs` | `ts2_apc_frame_split_across_feeds_reassembles` ほか mid-introducer | 存在・前回 PASS |
| TS-3 | `mux_apc_extractor.rs` | `ts3_osc_9999_*`（4本: ST/BEL/non-mux/other-osc） | 存在・前回 PASS |
| TS-4 | `tabs.rs` | split Kitty over mux PtyOutput（replay 系） | 存在・前回 PASS |
| TS-5 | `tabs.rs` | pre-mux routing | 存在・前回 PASS |
| TS-6 | `tabs.rs` | `ts6_detach_restores_core_routing` / `ts6_detach_resets_extractor_partial_frame` | 存在・前回 PASS |
| TS-7 | `tabs.rs` | `ts7_double_welcome_does_not_corrupt_decoding` | 存在・前回 PASS |
| TS-8 | (Build/Static) | DIAG / parser_mid_sequence grep | 自動 PASS（grep 0 件） |
| TS-9 | `tabs.rs` | 非mux Kitty デコード | 存在・前回 PASS |
| TS-10 | (Static/Review) | mux_ipc/bridge/daemon 未変更 | レビュー PASS（git diff 空） |
| TS-11 | `tabs.rs` | **`ts11_post_detached_tail_in_coalesced_buffer_renders_via_core`** | **存在・前回 PASS（焦点）** |
| TS-12 | `mux_apc_extractor.rs` | **`ts12_injected_osc_param_and_prefix_are_used`** | **存在・前回 PASS（焦点）** |
| TS-13 | `callbacks.rs` | **`osc_9999_emterm_mux_inband_routed_to_pending_apc` + `osc_9999_non_mux_prefix_is_dropped`** | **存在・前回 PASS（焦点）** |

TS-11/TS-12/TS-13 の実テストが想定どおりの所在に存在し、各 assert がそれぞれ
FR5（detach tail 再ルート）/ NFR5（注入値使用）/ NFR5（app 層 OSC 9999 認識）を正しく検証している
ことをコードレビューで確認。テスト本体の再実行は sdd.5-check 済みのため行っていない
（前回 check: term_core 662 passed・全体 1870 passed）。

---

## E2E テスト結果

- Docker 環境: 当機能に対する専用 E2E フレームワーク無し（VERIFICATION.md「No project E2E framework. Not applicable.」）
- E2E テスト: **N/A（対象外）**
- プロジェクト広域 Docker E2E スイートは当機能パスの対象外。リッチコンテンツ描画は GUI 実機確認（下記 M1〜M6）に委ねる。

---

## 手動確認が必要な項目（E2E 不可・GUI 実機確認）

VERIFICATION.md「Manual Testing (E2E Not Possible)」より6項目、加えて今回の finding ④/⑤ の
実機確認候補2項目を列挙。いずれも wgpu 描画ネイティブターミナル＋子WebView のため自動化不可。

### M1〜M6（SPEC 由来）
- [ ] **M1**: mux タブで `emterm image <file>` → インライン画像が描画され、base64 が画面に漏れない（SC-1）
- [ ] **M2**: 大画像（数MB）がチャンク境界をまたいで正しく組み立てられる（SC-2）
- [ ] **M3**: 実行中 `emterm.log` に `Kitty image decode failed` / `mux APC decode failed` が出ない
- [ ] **M4**: 非mux タブでも従来どおり画像が描画される（退行なし、SC-3）
- [ ] **M5**: SIXEL（`emterm image --protocol sixel`）が mux で描画される（SC-4）
- [ ] **M6**: Markdown ビューア・プレーンテキスト・TUI（vim）が mux で従来どおり動作（副作用なし、SC-5）

### 今回 finding 由来の追加実機確認候補
- [ ] **④（FR5）**: mux セッションから detach した直後、同一 coalesced buffer 内に積まれたシェルプロンプトが
      即座に描画される（次のキー入力を待たずに画面が再表示される）。TS-11 で自動検証済みだが、実機の
      detach タイミング・bridge プロセス終了との競合は GUI で最終確認推奨。
- [ ] **⑤（NFR5）**: Windows ConPTY 環境で mux attach 時、OSC 9999 `emterm-mux;` Welcome 経由で
      mux が確立する（APC が ConPTY に剥がされても OSC fallback でハンドシェイク成立）。
      `term_core` から認識を外したため、Windows 実機での pre-mux ハンドシェイク確認が望ましい。

> M3 はログ確認のため半自動。検証時は `~/.local/share/net.laser5.app.emterm/logs/emterm.log` を参照。

---

## パフォーマンス検証（レビューレベル）

- `pump` フレームバジェット `FRAME_BUDGET_MS = 12`（`tabs.rs:1413`）と coalesce cap
  `COALESCE_CAP = 1024 * 1024`（`tabs.rs:1423`）は不変。
- extractor 追加オーバーヘッドは pump あたり最小（`feed_with_offsets` は coalesced buffer を1バイトずつ
  既存 `Parser` に通すのみ。新規アロケーションは抽出ペイロードのみ）。NFR4 充足。
- 実描画スループットの最終確認は手動 M6 に委ねる。

## セキュリティ検証

- 新規外部入力面なし。APC/OSC パースは既存の `term_core::Parser` 境界内に収まる。
- 抽出器は transport-only（Print/CSI/Esc/Execute/DCS/その他 OSC を破棄）のため、外部 PTS から
  inner content 描画に渡る経路は従来と同じ `apply_mux_message` PtyOutput アームに限定。
- N/A（新たなセキュリティ懸念なし）。

---

## 結果別の留意事項

**現状＝すべての自動検証項目に合格**:
- TS-11/TS-12/TS-13 が想定どおり実装され、FR5（④）/ NFR5（⑤）を検証している。
- 残作業は GUI 実機での手動確認（M1〜M6 + ④/⑤）のみ。手動テスト完了後 VERIFICATION.md の
  チェックボックスを更新し、最終コードレビュー・リリース準備へ進める。

---

## 未解決の懸念

1. **手動 GUI 確認が未実施（M1〜M6 + ④/⑤）**: SC-1〜SC-5 は自動検証不可で、mux 内の実画像描画・
   SIXEL・Markdown ビューア・大画像の境界またぎは GUI 実機でのみ最終確認できる。自動テスト
   （TS-4/TS-7/TS-9 等）はデコード成功・base64 非漏れをカバーするが、視覚的な最終確認は残る。
2. **Windows ConPTY 実機での ⑤ 確認**: OSC 9999 認識を app 層へ移した変更（NFR5）は TS-13 で
   論理検証済みだが、実際の Windows ConPTY transport での pre-mux ハンドシェイク成立は Linux ホスト
   では確認できない。Windows 実機での回帰確認を推奨。
3. **直近コミットの diff スコープ**: `git diff HEAD~1` は当機能以外（visibility-aware-pty-streaming /
   scrollback_filter 等）も含むため、ファイル変更行数の統計は当機能の増分とは一致しない。当機能の
   増分対象4ファイルの内容は個別レビューで確認済み（懸念ではなく注記）。

---

**検証完了時刻**: 2026-06-20 00:18 JST
**検証方法**: 静的 grep / Read / コードレビュー（ビルド・テスト再実行なし、sdd.5-check 済み）
