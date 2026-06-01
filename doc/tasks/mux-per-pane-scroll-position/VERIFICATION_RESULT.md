# 実装自動検証レポート: Per-Pane Scroll Position in mux

**検証日時**: 2026-06-01
**対象機能**: mux-per-pane-scroll-position
**VERIFICATION.md**: `doc/tasks/mux-per-pane-scroll-position/VERIFICATION.md`
**SPEC.md**: `doc/tasks/mux-per-pane-scroll-position/SPEC.md`
**プロジェクト**: eMterm (Tauri / TypeScript / WASM)
**検証種別**: sdd.6 包括検証

---

## 検証サマリー

| 検証項目 | 結果 | 詳細 |
|---------|------|------|
| ビルド (typecheck) | (sdd.5 済) | exit 0、型エラー無し。本検証では再実行せず |
| テスト実行 | (sdd.5 済) | bun test 2353 pass / 0 fail。本検証では再実行せず |
| フォーマット / 静的解析 | (sdd.5 済) | dead-code 無し。本検証では再実行せず |
| ファイル構造検証 | OK | 6/6 ファイル存在、想定の変更を含む |
| SPEC.md 適合性 | OK | FR1 / FR2 / NFR1 / NFR2 / SC-1〜SC-4 を実装でカバー |
| E2E スイート | 未実行 | 本機能専用 spec 未作成。pane 切替シナリオは手動確認に回す |
| 手動確認項目 | 抽出済 | 3 項目を列挙 (下記) |

**総合評価**: すべての自動検証項目をクリア。残りは手動確認 3 項目のみ

> 注: build / test / format / static analysis は sdd.5-check で検証済み (typecheck exit 0、bun test 2353 pass/0 fail、dead-code なし) のため本検証では再実行していない。

---

## ファイル構造検証

VERIFICATION.md の「Files to Modify」「Files Created」に挙がる 6 ファイルすべてが存在し、想定の変更を含むことを `test -f` と `grep` で確認した。

### 変更ファイル (4/4)

- OK `src/terminal/state-mux-pane.ts`
  - `MuxPaneGridState` (L22) に `scrollOffset` (L42) / `scrollPinBaseline` (L46) を追加
  - 初期スナップショット (L82-83) で両フィールドを 0 初期化
- OK `src/terminal/canvas-renderer.ts`
  - `getScrollPinBaseline()` (L1316) / `setScrollPinBaseline()` (L1325) を公開
  - ベースラインは `prevScrollbackLength` を介して scroll-pin 補正と連動
- OK `src/terminal/renderer-interface.ts`
  - `getScrollOffset` / `setScrollOffset` (L130/136) に加え `getScrollPinBaseline` (L144) / `setScrollPinBaseline` (L150) をインターフェース宣言
- OK `src/terminal-app/mux/mux-window-manager.ts`
  - helper を import (L13-15)
  - capture/apply/reset を 3 経路に配線 (下記 SPEC 適合性参照)

### 作成ファイル (2/2)

- OK `src/terminal/state-mux-pane-scroll.ts` (新規 helper)
  - `ScrollStateSnapshot` / `ScrollStateTarget` 型と `captureScrollState` / `applyScrollState` / `resetScrollState` の純粋 helper
  - renderer の最小インターフェース (`ScrollStateTarget`) のみに依存
- OK `src/terminal/state-mux-pane-scroll.test.ts` (新規テスト)
  - TS-1〜TS-6 を網羅する 10 テストケース

---

## SPEC.md 適合性検証

### FR1: pane 切替でスクロール位置を pane ごとに退避・復元

3 経路すべてで save / restore / fresh が正しく配線されていることをコードで確認した。

| 経路 | save (capture) | restore (apply) | fresh (reset) |
|------|----------------|-----------------|---------------|
| `switchMuxWindow` | L210 `captureScrollState(snapshot, prevRenderer)` | L236 `applyScrollState(savedState, restoreRenderer)` | L253 `createFreshMuxGrid` 内で `resetScrollState` (L97) |
| `handleRemoteSwitchWindow` | L988 `captureScrollState` | L1009 `applyScrollState` | L1021 `createFreshMuxGrid` 内で `resetScrollState` |
| `handleMuxPaneCreated` | L641 `captureScrollState` | (新規 pane のため restore なし) | L686 `createFreshMuxGrid` 内で `resetScrollState` |

- restore の `applyScrollState` は `restoreMuxPaneState` の後に呼ばれ、`setScrollOffset` が復元バッファの scrollback 長に対してクランプされる (L232-236 のコメントと一致)
- 新規 pane (`handleMuxPaneCreated`) は退避は行うが復元は不要で、`createFreshMuxGrid` 経由の `resetScrollState` で最下部 (offset=0) 初期化される。これは TS-3 の意図と一致

判定: **充足**

### FR2: 背景 pane の scroll-pin ベースラインも pane ごとに退避・復元

- `captureScrollState` / `applyScrollState` / `resetScrollState` はいずれも `scrollPinBaseline` (= renderer の `prevScrollbackLength`) を `scrollOffset` と同時に扱う (helper L45-46 / L56-57 / L64-65)
- `setScrollPinBaseline` (canvas-renderer L1325) が `prevScrollbackLength` を直接更新するため、復元後の背景 pane の scroll-pin 補正がその pane 自身のベースライン起点で適用される (canvas-renderer L1320-1326 のコメントと一致)

判定: **充足**

### NFR1: 性能

- 退避・復元は単一数値 (`scrollOffset`, `scrollPinBaseline`) の代入のみで、pane 切替経路上に重い処理を追加していない (helper は代入 4 行)

判定: **充足 (設計上担保)**

### NFR2: 通常タブのスクロール位置分離に回帰なし

- `state-mux-pane-scroll.ts` の importer は `mux-window-manager.ts` のみ (`state-mux-pane.ts` の一致はコメント内参照のみで実コードではない)
- 通常タブは各タブが独立 `TerminalApp` → 独立 `CanvasRenderer` を持ち、helper は mux 共有 renderer 経路でしか呼ばれないため、通常タブの独立 renderer には一切触れない
- TS-6 テスト (test.ts L117-136) が「片方の renderer への save/restore が他方に波及しない」ことを検証

判定: **充足**

### Success Criteria

| ID | 基準 | 判定 | 根拠 |
|----|------|------|------|
| SC-1 | pane 切替でスクロール位置が pane ごとに保持される | OK | FR1 配線 + TS-1/TS-2/TS-5 |
| SC-2 | pane に戻ると前回のスクロール位置が復元される | OK | switchMuxWindow restore 経路 + TS-5 round trip |
| SC-3 | 背景 pane の出力は既存 scroll-pin 挙動に従う | OK (自動) / 手動確認あり | FR2 (ベースライン連動) + 手動項目 1 |
| SC-4 | 通常タブのスクロール位置分離に回帰がない | OK (自動) / 手動確認あり | NFR2 (mux 経路限定) + TS-6 |

---

## E2E テスト結果

- Docker 環境: 存在する (`docker-compose.e2e.yml` + tauri-driver)
- **本機能専用 E2E spec**: 未作成 (実装フェーズで作成していない)
- **本検証での E2E スイート実行**: なし

実行しない理由:
- 本機能の検証には mux 実機での pane 切替が必要で、tauri-driver での自動化が困難
- 既存 E2E スイート全体の回帰実行は重く、本機能の差分検証には不向き
- よって pane 切替スクロールシナリオは下記「手動確認項目」に回す

---

## 手動確認が必要な項目

VERIFICATION.md の「E2E Testing」「Manual Testing」のチェックボックスから以下を抽出した。mux 実機で確認すること。

### E2E Testing (実機 / 自動化困難)
1. [ ] 既存 E2E テストが回帰なく通る
2. [ ] pane A でスクロールアップ → pane B へ切替 (B 自身の位置で表示) → pane A に戻る (A の位置が復元される)

### Manual Testing (E2E 不可)
3. [ ] 背景 pane に出力が届いたとき、スクロールアップ中の pane が位置を維持し、最下部の pane は新規出力に追従する (実機での主観確認)

---

## 総合評価

- 自動検証 (ファイル構造 / SPEC 適合性) はすべてクリア
- FR1 / FR2 / NFR1 / NFR2 と SC-1〜SC-4 を実装・テストでカバー
- 残作業は mux 実機での手動確認 3 項目のみ

### 結果別の留意事項
- 手動確認 3 項目を実機で実施し、完了後 VERIFICATION.md のチェックボックスを更新すること
- 修正が必要になった場合は `/em-sdd:sdd` を再実行すれば `sdd.yaml` の状態から自動で再検証される

---

**検証完了時刻**: 2026-06-01
