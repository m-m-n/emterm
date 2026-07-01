# 実装自動検証レポート: Mermaid Diagram Zoom Popup + Copy Button Fix

**検証日時**: 2026-07-02 08:09 JST (+0900)
**対象機能**: mermaid-zoom-popup
**VERIFICATION.md**: `doc/tasks/mermaid-zoom-popup/VERIFICATION.md`
**プロジェクト**: emterm（子 Markdown ビューア WebView）
**検証実行**: sdd.6 Comprehensive Verify（build/test/format/静的解析は sdd.5-check 済みのため再実行しない）

---

## 総合評価

**FAIL（実装修正ループを推奨）**

FR3（ズームステップ）に**確定した機能非準拠**があり、FR9（フォーカストラップ）が**部分実装**、加えてドラッグ由来のクローズ誤動作という実 UX バグを確認したよ。ユニットテストは 36/36 通過しているけど、FR3 のステップ意味論（ボタン/キーボードは 0.25 加算）を検証するテストが存在しないため、テスト緑でもこの非準拠は素通りしてる。これらは仕様に明記された受け入れ基準に反するので、full PASS にはできないよ。

| カテゴリ | 結果 |
|---------|------|
| ファイル構造検証 | PASS（作成4/4・変更5/5 すべて存在） |
| セキュリティ検証（grep） | PASS（4項目すべて充足） |
| FR 準拠（FR1〜FR10） | **一部 FAIL**（FR3 非準拠 / FR9 部分 / FR4 軽微逸脱 / FR2 要手動確認） |
| NFR 準拠（NFR1〜NFR5） | 一部要確認（NFR5 部分・NFR1/3 手動） |
| 手動テスト項目 | 抽出済み（下記、GUI 操作は未実施） |

---

## 自動検証（sdd.5-check 済み・本レポートでは再実行せず）

VERIFICATION.md 記載の実測結果を引き継ぐよ。

- `bun run build:viewer` — exit 0（`Bundled 2332 modules`）
- `bun test` — 36 pass / 0 fail / 123 expect() 呼び出し
- `bun run typecheck` — clean（出力なし）
- Rust `--lib` — 変更なし（本機能は TS/CSS/JSON/test-setup のみ変更、Rust 無改変）。`tabs::tests::*` の既知フレークは本機能と無関係

> 注意: `bun test` 36/36 通過は FR の**網羅的**な保証ではないよ。特に FR3 のステップ意味論（ボタン/キーボード 0.25 加算 vs ホイール 1.1 乗算）を区別して検証するテストが無いため、後述の非準拠がテスト緑をすり抜けている。

---

## ファイル構造検証 — PASS

### 作成ファイル（4/4）
- OK `src-tauri/web-shared/markdown/mermaid-popup.ts`
- OK `src-tauri/web-shared/markdown/mermaid-popup.test.ts`
- OK `src-tauri/web-shared/markdown/mermaid-renderer.test.ts`
- OK `doc/tasks/mermaid-zoom-popup/tasks.yaml`

### 変更ファイル（5/5、付随変更 test-setup.ts 含む）
- OK `src-tauri/web-shared/markdown/mermaid-renderer.ts`
- OK `src-tauri/web-shared/markdown/fullscreen.css`
- OK `src-tauri/web-shared/i18n/locales/en.json`（`mermaidSpread` / `mermaidPopupClose` / `mermaidPopupZoomIn` / `mermaidPopupZoomOut` / `mermaidPopupReset` の5キー存在）
- OK `src-tauri/web-shared/i18n/locales/ja.json`（同5キー、日本語で存在）
- OK `test-setup.ts`（`globalThis.MouseEvent` バインド追加）

---

## セキュリティ検証（grep ベース）— PASS

VERIFICATION.md「Security Verification」の4項目すべて充足。

| 項目 | 結果 | 根拠 |
|------|------|------|
| `securityLevel: "strict"` 保持 | OK | `mermaid-renderer.ts:139`。working-tree diff に変更なし |
| popup モジュールが `innerHTML` を使わない | OK | `mermaid-popup.ts` に `innerHTML` 出現ゼロ。SVG は `appendChild`（`clone`）で挿入 |
| clipboard 書き込みは `source` そのまま | OK | `mermaid-renderer.ts:69` `clipboard.writeText(source)`。source = `data-mermaid-source` 無加工 |
| `role="dialog"` + `aria-modal="true"` | OK | `mermaid-popup.ts:66-67` |

補足: `mermaid-renderer.ts` の `innerHTML` 使用箇所（209/229/236/242）は、それぞれ mermaid strict サニタイズ済み SVG 文字列と静的定数アイコン（`CHART_ICON`/`CODE_ICON`/`SPREAD_ICON`）であり、ユーザー制御データではない。popup モジュール自体は `innerHTML` 不使用。セキュリティ懸念なし。

---

## 機能要件（FR）準拠検証 — 実コード照合

multi-review（9視点マルチモデルレビュー）で報告された6件の逸脱を、実コードに対して独立検証した結果を各 FR 判定に反映してるよ。

### FR1（ツールバー Spread ボタン）— 準拠

- ツールバー DOM 順序 `mermaid-renderer.ts:265-268` = `chart, code, spread, copy`。仕様どおり Spread は Code と Copy の間。
- Spread ボタン: `type="button"`、`aria-label = t("markdown.mermaidSpread")`、`.mermaid-spread-btn` クラス、14x14 viewBox の展開アイコン（`SPREAD_ICON`）。
- レンダー失敗時は `catch` 分岐でツールバー未構築 → Spread 未生成（EC-1 準拠）。
- **判定: 準拠**（TS-1/TS-2 で担保）

### FR2（ポップアップ開く + fit-to-stage）— 部分準拠（要手動確認）

- overlay 構築（`fixed`/`inset:0`/`z-index:2000`）、`document.body` へ append、`cloneNode(true)` でクローン挿入、fit 係数 `k = min(aw/sw, ah/sh)` を viewBox 由来で算出（`mermaid-popup.ts:150-155`）。数式は仕様どおり。
- **[レビュー claim 6 を確認: 妥当]** クローンは `cloneNode(true)`（`:75`）で mermaid の **インライン `style="max-width:…px"` と `width="100%"`（`useMaxWidth:true`, `mermaid-renderer.ts:161`）を継承**する。CSS `.mermaid-popup-stage svg { max-width: none }`（`fullscreen.css:503`）は**スタイルシート規則なのでインライン style 属性を上書きできない**（詳細度でインラインが勝つ）。その結果、クローンの実描画ベースサイズが viewBox 実寸ではなく max-width 制約下になり、`scale(scale*fitK)` が真の fit-to-stage を生まない可能性がある。
- happy-dom はレイアウト計算をしない（`getBoundingClientRect()` は 0 を返す）ため、TS-15 は fitK の**数式**のみ検証しており、この描画不整合はユニットテストでは検出不能。
- **判定: 部分準拠**（fit 係数計算は仕様準拠だが、クローンのインライン max-width により実描画の fit-to-stage が崩れるリスクあり。SC-5/SC-6 手動確認必須。確認されれば実装修正推奨）

### FR3（ズーム操作 + クランプ）— **非準拠**

- **[レビュー claim 1 を確認: 妥当・High]** `STEP_FACTOR = 1.1`（`:37`）、`zoomIn = clamp(scale * 1.1)` / `zoomOut = clamp(scale / 1.1)`（`:165-172`）。この乗算ステップが **ボタン（`:184-186`）・ホイール（`:188-195`）・キーボード（`:228-237`）のすべて**に使われている。
- 仕様 FR3: 「Button clicks and keyboard `+`/`-` step scale by **0.25**; wheel events multiply/divide by 1.1」。US2 受け入れ基準・VERIFICATION 手動チェック「`+` and `-` buttons visibly step the zoom by **0.25**」とも一致。
- 実装はボタン/キーボードを**0.25 加算ではなく 1.1 乗算**にしている → 明確な仕様違反。
- **[レビュー claim 4 を確認: 妥当・Medium]** コントロールの DOM append 順（`:109-111`）= `zoom-in, reset, zoom-out`。CSS `flex-direction: column`（`fullscreen.css:513`）なので視覚的な上→下は `+, 0, −`。仕様 FR3 の列挙は `zoom-out(−), reset(0), zoom-in(+)` で**順序が逆**。
- クランプ `[0.25, 5.0]` 自体は正しく機能（TS-8/TS-9 が担保）。ただし TS-8/TS-9 は40回クリックでクランプ端に到達させるだけで、**ステップ量が 0.25 か 1.1 かを区別しない**ため本非準拠を検出できていない。TS-13 はホイールの `baseline*1.1` のみ検証。
- **判定: 非準拠**（ステップ意味論違反 + コントロール順逆転。実装修正ループ推奨）

### FR4（ドラッグでパン）— 部分準拠（軽微逸脱）

- `mousedown`（`:200-204`）で `dragging=true` + `.mermaid-popup-dragging` 付与、`mousemove`（`:205-210`, window）で `panX/panY += movementX/Y`、`mouseup`（`:211-214`, window）で `dragging=false`。カーソルは CSS で `grab`↔`grabbing`。パン自体は機能（TS-14 が担保）。
- **[レビュー claim 3 を確認: 妥当・軽微]** 仕様 FR4 の文言「captures the pointer …（`setPointerCapture`）」「`mouseup` / **`mouseleave`** sets dragging=false」に対し、実装は**ポインタキャプチャなし・`mouseleave` ハンドラなし**で、代わりに **window レベルの mouseup** で解放を捕捉している。
- 機能的には window-mouseup 方式はステージ外リリースも拾えるため、`mouseleave` と同等以上に堅牢。よって挙動上の欠陥ではなく、**仕様テキストとの逐語的逸脱**にとどまる。
- **判定: 部分準拠**（挙動は成立。ポインタキャプチャ/mouseleave の仕様文言は未実装 → 仕様側の追認、または軽微な実装補強のいずれか。優先度低）

### FR5（`0` でリセット）— 準拠

- `resetView`（`:173-178`）が `scale=1.0, panX=0, panY=0` に設定。`0` キー（`:238-241`）とリセットボタン（`:186`）両方から呼ばれる。
- **判定: 準拠**（TS-10 が担保）

### FR6（× / 背景 / ESC でクローズ）— 準拠（ただし claim 5 の UX バグを内包）

- × ボタン（`:256`）、`ev.target === overlay` の背景クリック（`:249-254`）、ESC（`:223-227`, capture + `stopPropagation` + `preventDefault`）の3経路。クローズ時に overlay 除去・`body.style.overflow` 復元・trigger へ `focus()`。
- **[レビュー claim 5 を確認: 妥当・Medium／実 UX バグ]** ドラッグ判定ガードが無いため、**ステージ上で mousedown → 背景で mouseup したパンドラッグ**が `click` を合成し、その `target` が共通祖先の overlay になる → `onOverlayClick` が `target===overlay` を満たしてポップアップを**意図せずクローズ**する。FR6 直接の違反ではない（仕様はドラッグ解放時の扱いを規定していない）が、実使用で起きうる品質バグ。
- **判定: 準拠（FR6 の3クローズ経路は成立）。ただしドラッグ由来の誤クローズを実装修正推奨**

### FR7（背景スクロールロック）— 準拠

- open 時 `previousOverflow = document.body.style.overflow` を退避し `hidden` に（`:265-266`）、close 時に復元（`:285`）。
- **判定: 準拠**（TS-6 が担保）

### FR8（Copy クリックハンドラ）— 準拠

- `attachMermaidCopyHandler`（`:40-83`）が `clipboard.writeText(source)` を await。成功で `.copy-success` + `markdown.copySuccess`、失敗で `.copy-error` + `markdown.copyFailed` + `console.warn`、1500ms で復元。clipboard 不在時も reject 扱いでエラー UI 表示（防御的）。
- **判定: 準拠**（TS-3/TS-4 が担保）

### FR9（フォーカス管理）— 部分準拠

- open 時に close ボタンへ `focus()`（`:268`）、close 時に trigger へ `focus()`（`:289`）は実装済み。
- **[レビュー claim 2 を確認: 妥当・High]** `onKeydown`（`:221-245`）は Escape/+/=/-/_/0 のみ処理し、**`Tab` ケースが無く、フォーカストラップが一切実装されていない**。`aria-modal="true"` を宣言しているのに、Tab で4ボタン内を循環させる仕様 FR9「A minimal focus trap keeps Tab cycling within the four popup buttons」を満たしていない。
- **判定: 部分準拠**（open/close のフォーカス移動は成立するが、フォーカストラップ未実装。アクセシビリティ観点で実装修正推奨）

### FR10（リサイズ再フィット）— 準拠

- `window.resize` リスナ（`:258-262`）が `recomputeFit()` + `applyTransform()` を呼ぶ。
- **判定: 準拠**（TS-15 が fitK 再計算を担保）

### FR 準拠サマリー

| FR | 判定 | 備考 |
|----|------|------|
| FR1 Spread ボタン | 準拠 | — |
| FR2 ポップアップ + fit | 部分準拠 | claim6: クローンのインライン max-width で fit 崩れリスク（要手動確認） |
| FR3 ズーム + クランプ | **非準拠** | claim1: ボタン/キーボードが 0.25 加算でなく 1.1 乗算 / claim4: コントロール順逆転 |
| FR4 パン | 部分準拠 | claim3: ポインタキャプチャ/mouseleave 未実装（window mouseup で代替、挙動は成立） |
| FR5 リセット | 準拠 | — |
| FR6 クローズ | 準拠 | claim5: ドラッグ解放が背景で起きると誤クローズ（実 UX バグ） |
| FR7 スクロールロック | 準拠 | — |
| FR8 Copy | 準拠 | — |
| FR9 フォーカス管理 | 部分準拠 | claim2: Tab フォーカストラップ未実装 |
| FR10 リサイズ再フィット | 準拠 | — |

---

## 非機能要件（NFR）準拠検証

| NFR | 判定 | 根拠 |
|-----|------|------|
| NFR1 性能（60fps / <100ms） | 手動確認要 | CSS `transform` のみで再レンダーなし（設計は妥当）。実測は手動 |
| NFR2 セキュリティ | 準拠 | 上記セキュリティ検証4項目すべて充足 |
| NFR3 クロスプラットフォーム | 手動確認要 | Linux WebKitGTK / Windows WebView2 両方で手動確認 |
| NFR4 保守性 | 準拠 | `mermaid-popup.ts` 分離・CSS は `fullscreen.css`（ファイル構造どおり） |
| NFR5 アクセシビリティ | 部分準拠 | `role=dialog`/`aria-modal`/各ボタン `aria-label` は充足（TS-5）。ただし FR9 のフォーカストラップ未実装のため `aria-modal` の期待挙動と齟齬 |

---

## 手動確認が必要な項目（E2E 不可・GUI 操作は未実施）

VERIFICATION.md から抽出。実行には Mermaid を含む Markdown を eMterm 内で `emterm markdown fixture.md` として表示する必要があるよ。**本検証では GUI 操作は行っていない**ので、以下はユーザー側で実施してね。

**Linux（WebKitGTK）**
- [ ] Mermaid ブロックにホバー → ツールバー `[Chart | Code | Spread | Copy]` が右上に表示
- [ ] Spread クリック → overlay が開き、図が中央に fit-to-stage で表示 ← **FR2 claim6 の fit 崩れをここで重点確認**
- [ ] `+` / `-` ボタンでズームが **0.25 刻み**で変化 ← **FR3 claim1 非準拠のため現状は 1.1 刻みになるはず。要確認**
- [ ] ホイールで滑らかにズーム、背景はスクロールしない
- [ ] `+` / `-` / `0` キーがボタンと同じ挙動 ← **`+`/`-` は FR3 非準拠の影響を受ける**
- [ ] 左ドラッグでパン、押下中カーソルが `grabbing`
- [ ] 背景（図/コントロール外）クリックで overlay クローズ ← **claim5: ドラッグを背景で離すと誤クローズしないか併せて確認**
- [ ] `×` ボタンでクローズ
- [ ] ESC で overlay クローズ、かつ Markdown ビューアは閉じない（2回目 ESC でビューアが閉じるのは仕様どおり）
- [ ] クローズ後、Spread ボタンにフォーカスが戻る
- [ ] overlay 中は body スクロールがロック、クローズで復元
- [ ] overlay を開いたままウィンドウリサイズ → 図が新ステージに再フィット
- [ ] Copy ボタンで成功表示が点滅し、貼り付けたテキストが元 Mermaid ソースと一致
- [ ] （追加確認）Tab キーでフォーカスがポップアップ内4ボタンを循環するか ← **FR9 claim2 未実装のため現状は循環しないはず**

**Windows（WebView2）** — 上記と同一チェックリストを Windows ビルドで実施

**両プラットフォーム — クリーンアップ確認**
- [ ] 約1分ポップアップを操作後、`~/.local/share/net.laser5.app.emterm/logs/emterm.log` に Mermaid/popup/clipboard 関連の新規 `warn`/`error` が出ていない

**パフォーマンス（手動）**
- [ ] NFR-P1: クリック → overlay 表示に体感遅延なし（<100ms）
- [ ] NFR-P2: ドラッグ/ホイールズームでカクつきなし（60fps）

---

## 修正推奨（fix ループ振り分け）

### 実装修正ループ（sdd.4-implement へ戻す）を推奨

1. **FR3 ステップ意味論（claim1・High・必須）** — `zoomIn`/`zoomOut` をボタン/キーボード経路では **0.25 加算**（`clamp(scale ± 0.25)`）に変更し、ホイール経路のみ 1.1 乗算を維持。加えて FR3 のステップ量（0.25 加算 vs 1.1 乗算）を区別して検証するユニットテストを追加（現状 TS-8/TS-9 はクランプのみでこの差異を捕捉できていない）。
   - ※ 代替として「全経路 1.1 乗算」を正とするなら**仕様側修正**（SPEC FR3 / US2 / VERIFICATION 手動チェックの「0.25」記述を更新）。ただしユーザーの手動チェックと US2 が明示的に 0.25 を要求しているため、意図は 0.25 と判断。実装修正を第一推奨。
2. **FR9 フォーカストラップ（claim2・High）** — `onKeydown` に `Tab` ケースを追加し、4ボタン（close/zoom-out/reset/zoom-in）内で循環させる。`aria-modal="true"` 宣言との整合のため実装修正を推奨。
3. **ドラッグ誤クローズ（claim5・Medium・実 UX バグ）** — `onOverlayClick` にドラッグ判定ガードを追加（例: ドラッグ移動が発生したフレームでは背景クリックを無視する／`dragging` 由来のクリックを抑制）。

### 手動確認の上で実装修正を判断

4. **FR2 fit-to-stage（claim6・Medium）** — 手動で fit 崩れが確認された場合、クローンの**インライン `max-width` / `width` 属性を除去**（`clone.style.maxWidth = "none"; clone.removeAttribute("width")` 等）してから transform を適用。CSS 規則だけではインライン style を上書きできない点が根本原因。

### 仕様側追認 or 低優先の実装補強

5. **FR3 コントロール順（claim4・Medium・整容）** — DOM append 順を `zoom-out, reset, zoom-in` に並べ替えて仕様に合わせる（1行修正）か、仕様側の列挙順を実装に合わせて更新。
6. **FR4 ポインタキャプチャ/mouseleave（claim3・軽微）** — window-mouseup 方式で挙動は成立しているため、仕様文言を「window レベル mouseup で解放捕捉」と追認するのが低コスト。厳密準拠を求める場合のみ `setPointerCapture` / `mouseleave` を追加。

---

## multi-review 6件の独立検証結果まとめ

| # | レビュー主張 | 深刻度 | 独立検証 | 該当 FR 判定への反映 |
|---|-------------|--------|----------|------------------|
| 1 | FR3: ボタン/キー/ホイール全部が 1.1 乗算（0.25 加算でない） | High | **妥当（確認）** | FR3 → 非準拠 |
| 2 | FR9: Tab フォーカストラップ無し | High | **妥当（確認）** | FR9 → 部分準拠 |
| 3 | FR4: ポインタキャプチャ/mouseleave 無し（window mouseup のみ） | High(GPT) | **妥当だが挙動は成立** | FR4 → 部分準拠（軽微） |
| 4 | FR3: コントロール順が [in,reset,out]（仕様は [out,reset,in]） | Medium | **妥当（確認）** | FR3 → 非準拠（詳細） |
| 5 | ドラッグ解放が背景で起きるとポップアップ誤クローズ | Medium | **妥当（確認・実バグ）** | FR6 は経路成立だが UX バグ内包 |
| 6 | FR2: クローンのインライン max-width で真の fit にならない | Medium | **妥当（コード上のリスク・要手動確認）** | FR2 → 部分準拠 |

6件すべて実コードに対して妥当と確認したよ。ユニットテスト 36/36 通過は、これらがいずれも「テストが検証していない領域（ステップ量の区別／レイアウト依存の描画／ドラッグ合成クリック／Tab 挙動）」に存在するために緑をすり抜けている、という構図。

---

# 再検証レポート（phase-3 fix 後）

**再検証日時**: 2026-07-02 (JST +0900)
**対象機能**: mermaid-zoom-popup
**再検証の位置づけ**: 上記 FAIL（FR3 非準拠 / FR9 部分 / FR2 部分 / FR4 部分 / FR6 UX バグ）に対する phase-3 修正後の独立再検証。上の FAIL 記録は履歴として保持し、以下に再検証結果を追記するよ。
**対象コミット状態**: `src-tauri/web-shared/markdown/mermaid-popup.ts`（修正後）+ `mermaid-popup.test.ts`（TS-16〜TS-20 追加）、SPEC.md 更新（FR2/FR3/FR4/FR6/FR9 明確化、FR3 ボタン順・FR4 window レベル mouseup は仕様側で意図的として追認済み）

---

## 再検証 総合評価

**PASS（手動 GUI 確認のみ残存）**

前回 FAIL の原因となった全項目（FR3・FR9・FR6・FR2・FR4）が、実コードに対して仕様準拠へ修正されていることを独立確認したよ。自動検証で判定可能な FR チェックは全て PASS。残るのは Linux(WebKitGTK)/Windows(WebView2) 上での手動 GUI 確認のみで、これは自動検証の対象外（想定内の残タスク）なので、PASS 判定をブロックしないよ。

| カテゴリ | 前回 | 今回 |
|---------|------|------|
| FR3 ズームステップ意味論 | 非準拠 | **PASS** |
| FR9 Tab フォーカストラップ | 部分 | **PASS** |
| FR6 パン終端の誤クローズ | UX バグ内包 | **PASS** |
| FR2 クローンの sizing 正規化 | 部分（fit 崩れリスク） | **PASS** |
| FR4 スタックドラッグガード | 部分 | **PASS** |
| `bun test` スイート | 36 pass | **41 pass / 0 fail** |
| 手動 GUI 確認 | 未実施 | 未実施（想定内・下記チェックリスト参照） |

---

## `bun test` 独立実行結果

本再検証で `bun test`（スイート全体）を1回実行して独立確認したよ。

```
bun test v1.3.14
 41 pass
 0 fail
 148 expect() calls
Ran 41 tests across 5 files.
```

- 期待値（41 pass / 0 fail）と一致
- markdown モジュール単体でも 21 pass / 0 fail（`mermaid-popup.test.ts` の TS-5〜TS-20 + single-instance guard、`mermaid-renderer.test.ts` を含む）
- 新規追加テスト TS-16〜TS-20 が緑で、前回すり抜けていた「ステップ量の区別 / Tab 挙動 / パン合成クリック / クローン sizing / blur ドラッグ解除」を検証している

---

## 前回 FAIL 項目の FR 別 再判定（実コード照合）

### FR3（ズーム操作 + クランプ）— **PASS**（前回: 非準拠）

- `ZOOM_STEP = 0.25`（`mermaid-popup.ts:37`）、`STEP_FACTOR = 1.1`（`:39`）に分離済み。
- ボタン: `zoomInBtn` → `zoomInStep`（`:207`）、`zoomOutBtn` → `zoomOutStep`（`:208`）。`zoomInStep`=`clamp(scale + ZOOM_STEP)`（`:179-182`）、`zoomOutStep`=`clamp(scale - ZOOM_STEP)`（`:183-186`）→ **0.25 加算**。
- キーボード: `+`/`=` → `zoomInStep`、`-`/`_` → `zoomOutStep`（`:279-288`）→ ボタンと同じ **0.25 加算**。
- ホイール: `onWheel` → `zoomInWheel`=`clamp(scale * 1.1)` / `zoomOutWheel`=`clamp(scale / 1.1)`（`:188-195, :211-219`）→ **1.1 乗算を維持**。
- クランプ `[0.25, 5.0]`（`:34-35`, `clamp` `:175-176`）。
- コントロール DOM 順は `zoom-in(+), reset(0), zoom-out(-)`（`:122-124`）で、更新後 SPEC の「zoom-in を最上段に置く一般的なズームUI慣例」と一致（仕様側で意図的として追認済み）。
- テスト TS-16 が「ボタン/キー 0.25 加算・ホイール 1.1 乗算」を明示検証（`mermaid-popup.test.ts:397-451`）。
- **判定: PASS**

### FR9（フォーカス管理 / Tab トラップ）— **PASS**（前回: 部分）

- `focusOrder = [closeBtn, zoomInBtn, resetBtn, zoomOutBtn]`（`:256`）= 仕様の DOM 順 [close, zoom-in, reset, zoom-out]。
- `onKeydown` に `Tab` ケース追加（`:275-278`）: `ev.preventDefault()` の上で `trapFocus(ev.shiftKey)` を呼ぶ。
- `trapFocus`（`:257-266`）: 前方は末尾→先頭にラップ（`current === length-1 ? 0 : current+1`）、後方は先頭→末尾にラップ（`current <= 0 ? length-1 : current-1`）。両端ラップ + `preventDefault` を満たす。
- open 時 close ボタンへ focus（`:325`）、close 時 trigger へ focus（`:347`）も維持。
- テスト TS-17 が末尾→先頭・先頭→末尾のラップを検証（`:453-493`）。
- **判定: PASS**

### FR6（パン終端の誤クローズ防止）— **PASS**（前回: UX バグ内包）

- `didPan` フラグ導入（`:225`）。`onMouseDown` で `didPan=false`（`:229`）、`onMouseMove`（実移動時）で `didPan=true`（`:234`）。
- `onOverlayClick`（`:300-310`）: `didPan` が真ならフラグを消費して `return`（クローズしない）、偽かつ `target===overlay` の時のみ `controller.close()`。
- → ステージ mousedown→背景 mouseup のパンドラッグ由来の合成クリックはクローズしない。クリーンな背景クリックは従来どおりクローズ。
- テスト TS-18 が「パン終端クリックは閉じない / クリーンクリックは閉じる」を検証（`:495-541`）。TS-12 のクリーン背景クローズも継続で緑。
- **判定: PASS**

### FR2（クローン sizing 正規化 / fit-to-stage）— **PASS**（前回: 部分・fit 崩れリスク）

- クローン後に `width`/`height` 属性を除去（`:83-84`）、インライン `width`/`height`/`maxWidth`/`maxHeight` を空文字クリア（`:85-88`）。
- これにより mermaid の `useMaxWidth:true` 由来のインライン `max-width` / `width="100%"` が除去され、クローンの未変形ベース箱が viewBox 実寸となり `scale * fitK` が真の fit-to-stage を生む。前回指摘の「インライン style が CSS 規則で上書きできず fit 崩れ」を根本解消。
- 元 SVG は `cloneNode(true)` 後のクローンにのみ変更を加えるため無改変（TS-19 が `svg.getAttribute("width")` の維持を検証）。
- テスト TS-19 が「クローンに width/height 属性なし・インライン width/height/max-* なし・元 SVG 無改変」を検証（`:543-571`）。
- **判定: PASS**

### FR4（パン + スタックドラッグガード）— **PASS**（前回: 部分）

- `onBlur`（`:244-247`）で `dragging=false` + `.mermaid-popup-dragging` クラス除去、`window.addEventListener("blur", onBlur)`（`:251`）、close 時に removeEventListener（`:335`）。
- window レベル `mousemove`/`mouseup`（`:249-250`）でステージ外リリースも捕捉（更新後 SPEC で意図的設計として追認済み。`setPointerCapture`/`mouseleave` 不要）。
- テスト TS-20 が「blur でドラッグ解除・以降の mousemove はパンしない・dragging クラス除去」を検証（`:573-605`）。
- **判定: PASS**

---

## FR 準拠サマリー（再検証後）

| FR | 前回 | 今回 | 備考 |
|----|------|------|------|
| FR1 Spread ボタン | 準拠 | PASS | 変更なし |
| FR2 ポップアップ + fit | 部分 | **PASS** | クローン sizing 正規化で fit 崩れ解消 |
| FR3 ズーム + クランプ | 非準拠 | **PASS** | ボタン/キー 0.25 加算・ホイール 1.1 乗算に分離 |
| FR4 パン | 部分 | **PASS** | blur スタックドラッグガード追加 |
| FR5 リセット | 準拠 | PASS | 変更なし |
| FR6 クローズ | UX バグ内包 | **PASS** | didPan ガードでパン終端誤クローズ解消 |
| FR7 スクロールロック | 準拠 | PASS | 変更なし |
| FR8 Copy | 準拠 | PASS | 変更なし |
| FR9 フォーカス管理 | 部分 | **PASS** | Tab/Shift+Tab トラップ追加 |
| FR10 リサイズ再フィット | 準拠 | PASS | 変更なし |

自動検証可能な FR は FR1〜FR10 すべて PASS。

---

## 残存 手動確認項目（GUI 操作・自動検証対象外）

以下は eMterm 実機（Linux WebKitGTK / Windows WebView2）でのみ確認できる項目で、**本再検証では未実施**（想定内）。PASS 判定はブロックしないけど、リリース前にユーザー側で実施してね。前回の各注記のうち非準拠だった箇所は修正済みなので、期待挙動は「仕様どおり」に変わってるよ。

**Linux（WebKitGTK）**
- [ ] Mermaid ブロックにホバー → ツールバー `[Chart | Code | Spread | Copy]` 表示
- [ ] Spread クリック → overlay が開き、図が中央に fit-to-stage 表示（FR2 修正後、正しく実寸フィットするか確認）
- [ ] `+` / `-` ボタンでズームが **0.25 刻み**で変化（FR3 修正済み）
- [ ] ホイールで滑らかにズーム（1.1 倍）、背景はスクロールしない
- [ ] `+` / `-` / `0` キーがボタンと同じ挙動
- [ ] 左ドラッグでパン、押下中カーソルが `grabbing`
- [ ] 背景クリックで overlay クローズ／ドラッグを背景で離しても**閉じない**（FR6 修正済み）
- [ ] `×` ボタンでクローズ
- [ ] ESC で overlay クローズ、Markdown ビューアは閉じない（2回目 ESC でビューアが閉じるのは仕様どおり）
- [ ] クローズ後、Spread ボタンにフォーカスが戻る
- [ ] Tab / Shift+Tab でフォーカスがポップアップ内4ボタンを循環（FR9 修正済み）
- [ ] overlay 中は body スクロールがロック、クローズで復元
- [ ] overlay を開いたままウィンドウリサイズ → 図が新ステージに再フィット
- [ ] ウィンドウ blur（別ウィンドウへ切替）でドラッグが解除される（FR4 修正済み）
- [ ] Copy ボタンで成功表示が点滅し、貼り付けテキストが元 Mermaid ソースと一致

**Windows（WebView2）** — 上記と同一チェックリストを Windows ビルドで実施

**両プラットフォーム — クリーンアップ / 性能**
- [ ] 約1分操作後、`emterm.log` に Mermaid/popup/clipboard 関連の新規 `warn`/`error` が出ていない
- [ ] クリック → overlay 表示に体感遅延なし（<100ms）
- [ ] ドラッグ/ホイールズームでカクつきなし（60fps）

---

## 再検証 結論

- 前回 FAIL の5項目（FR3/FR9/FR6/FR2/FR4）はいずれも実コードで仕様準拠へ修正され、対応するユニットテスト TS-16〜TS-20 で回帰保護されているよ。
- `bun test` は 41 pass / 0 fail（期待どおり）。
- 自動検証可能な FR チェックは全て PASS。残るは手動 GUI 確認のみ（想定内・非ブロッキング）。
- **総合判定: PASS（手動 GUI 確認のみ残存）**
