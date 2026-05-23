# Verification Result: Pin Viewport When Scrolled Up

- 検証日時: 2026-05-23
- 対象機能: pin-viewport-when-scrolled-up
- 対象 commit: 3cc2d73 (sdd.5-check 完了時点と同一、staleness なし)
- VERIFICATION.md: `doc/tasks/pin-viewport-when-scrolled-up/VERIFICATION.md`
- SPEC.md: `doc/tasks/pin-viewport-when-scrolled-up/SPEC.md`

## 総合評価

**合格** — 実装は SPEC.md FR1〜FR6 / NFR1〜NFR2 に完全準拠。ファイル構造・純粋関数の単体テスト・renderer 組み込み・公開 API 不変・E2E (TS-7) すべて検証済み。

> Note: 初回検証時は E2E spec (`scroll-pin.e2e.js`) のデザインが FR4 (User-initiated scroll unchanged) と衝突して TS-7 がアサート失敗していた。spec を `ptyClient.write` 直接呼び出し方式（案 A）に修正後、再実行で pass を確認。詳細は下記「E2E 実行結果」と「修正履歴」を参照。

## 検証サマリー

| 検証項目 | 結果 | 備考 |
|---------|------|------|
| ファイル構造 | OK | 新規 3 / 変更 1 すべて存在、行数も計画通り |
| SPEC 機能要件適合性 (FR1〜FR6) | OK | 全 FR がコードと単体テストで担保 |
| SPEC 非機能要件 (NFR1, NFR2) | OK | 既存 perf テスト pass + renderer interface 不変 |
| 単体テスト (TS-1〜TS-6) | OK | 11/11 pass（sdd.5-check で実行済み） |
| E2E (TS-7, `scroll-pin.e2e.js`) | OK（spec 修正後 pass） | spec を `ptyClient.write` 直接呼び出し方式に修正。`scrollOffset` が 40 → 241 (Δ=201) に追従し、pin された絶対行のテキストが `"340"` のまま不変であることを確認 |
| パフォーマンス検証 (NFR1) | OK | `performance.test.ts` に regression なし（sdd.5-check で全 pass） |
| セキュリティ検証 | OK | 純粋関数追加 + scrollOffset 数値演算のみ。攻撃面なし |
| 手動テスト項目抽出 | OK | 下記「Manual Testing」セクションに項目化 |

## ファイル構造検証

VERIFICATION.md の「Files to Create / Files to Modify」と実ファイルを照合。

| 期待 | 期待行数 | 実ファイル | 実行数 | 結果 |
|------|---------|-----------|--------|------|
| `src/terminal/scroll-pin.ts`（新規） | 47 | 同左 | 47 | OK |
| `src/terminal/scroll-pin.test.ts`（新規） | 91 | 同左 | 91 | OK |
| `e2e-tests/specs/scroll-pin.e2e.js`（新規） | 145 | 同左 | 145 | OK |
| `src/terminal/canvas-renderer.ts`（変更） | 1422 | 同左 | 1422 | OK |

追加の `git status` 上の変更:
- `src/terminal/custom-glyphs.ts` — 本機能と無関係（grep で `scroll-pin` / `scrollOffset` / `prevScrollbackLength` のヒットなし）。検証スコープ外。

## SPEC.md 機能要件 (FR) カバレッジ

| ID | 要件 | 実装場所 | 検証手段 | 結果 |
|----|------|----------|---------|------|
| FR1 | Pin offset on PTY scrollback growth | `scroll-pin.ts:32-46`（Δ>0 && offset>0 で offset+=Δ）+ `canvas-renderer.ts:625-634` で render 冒頭に組込 | Unit TS-1（pass）+ E2E TS-7（spec 修正後 pass） | OK |
| FR2 | Follow-tail when offset is zero | `scroll-pin.ts:34`（offset===0 で no-op、prevSbLen のみ更新） | Unit TS-2（pass） | OK |
| FR3 | Clamp at scrollback top | `scroll-pin.ts:41-42`（`clamped = grown > currSbLen ? currSbLen : grown`） | Unit TS-3a/3b/3c（3 case 全 pass） | OK |
| FR4 | User-initiated scroll unchanged | `canvas-renderer.ts:1288-1305` の `scrollUp` / `scrollDown` / `setScrollOffset` は `prevScrollbackLength` に触らない（grep で確認） | コードレビュー + 既存 `keyboard.test.ts` pass | OK |
| FR5 | Alt-screen unaffected | `state.getScrollbackLength()` が常に primary を返すため Δ===0、`scroll-pin.ts:34` で no-op | Unit TS-5 / TS-5b（pass） | OK |
| FR6 | Partial scroll region unaffected | partial scroll region 時は WASM 側で scrollback push されない既存挙動 → Δ===0 で `scroll-pin.ts:34` no-op | Unit TS-6 / TS-6b（pass） | OK |

## SPEC.md 非機能要件 (NFR) カバレッジ

| ID | 要件 | 検証手段 | 結果 |
|----|------|---------|------|
| NFR1 | Performance（per-frame work = 1 比較 + 1 代入のみ） | `scroll-pin.ts` の計算は加算/比較/Math.min のみ。`performance.test.ts` (PerformanceMonitor / ThroughputMeter / Terminal Action Processing / Memory Efficiency / Dirty Row Tracking 系列）が sdd.5-check で全 pass | OK |
| NFR2 | Compatibility（`ITerminalRenderer` の `scrollUp/scrollDown/getScrollOffset/setScrollOffset` 不変） | `renderer-interface.ts:118-136` のシグネチャを確認、変更なし。`prevScrollbackLength` は `private` フィールドで公開 API に出ない | OK |

## E2E 実行結果

実行コマンド: `./scripts/run-e2e-docker.sh test scroll-pin.e2e.js`

実行サマリー（spec 修正後）:
- 起動・初期化: 正常（Tauri 起動、wry 0.53.5、tab-content 取得 OK）
- `seq 1 400` 入力後: `scrollbackLength=375, scrollOffset=0, rows=27`（OK）
- `setScrollOffset(40)`: `scrollOffset=40`（OK）
- `before` サンプル: `text='340', scrollOffset=40, scrollbackLength=375`（OK）
- `ptyWrite("seq 1 200")` 実行: `{ok:true}`（PTY 直接書き込みで keyboard handler bypass）
- `after` サンプル: `text='340', scrollOffset=241, scrollbackLength=576`（OK、Δ=201 で offset 追従、pin 行不変）
- アサート `expect(after.scrollOffset).toBeGreaterThan(40)` および `after.text === before.text` 両方 pass

結論: **1 passing (8.8s)**。FR1 (pin offset on scrollback growth) が実環境で検証された。

screenshot:
- `e2e-tests/screenshots/scroll-pin-before-burst.png` — 生成あり
- `e2e-tests/screenshots/scroll-pin-after-burst.png` — 生成あり

### 修正履歴

初回検証時は E2E spec の `typeCommand("seq 1 200")` が内部で `browser.keys(ch)` を呼び、emterm の `src/terminal-app/handlers/keyboard.ts:362` の `onExitScrollback()` → `setScrollOffset(0)` が発火して pin が解除されていた。これは SPEC.md FR4 (User-initiated scroll unchanged) の意図的な仕様で、spec の検証ストラテジが FR4 と衝突していた。

修正内容（実装本体は変更なし）:
- `e2e-tests/specs/scroll-pin.e2e.js` に `ptyWrite(cmd)` ヘルパーを追加
- `browser.execute` 内で `window.terminalApp.pty.write(`${cmd}\n`)` を直接呼ぶ
- keyboard handler を bypass するため `onExitScrollback()` が発火せず pin が維持される
- WASM scrollback growth は本物の PTY chunk 由来で FR1 の実環境検証として十分

### sdd.4 戻しの要否

不要。実装側 (`scroll-pin.ts` / `canvas-renderer.ts`) は SPEC.md 通りで正しい。修正は E2E spec のみ。

## パフォーマンス検証

- `bun test` の `performance.test.ts` は sdd.5-check 時点で全 pass（2343 pass / 0 fail）
- 追加コードのコスト: render パスあたり「`getScrollbackLength()` 1 回 + 加算 1 回 + 比較 1 回 + Math.min 1 回 + フィールド代入 2 回」
- VERIFICATION.md「per-frame work added beyond a comparison and a counter update」と一致

## セキュリティ検証

該当事項なし。

- 外部入力を新規に受け付けない
- 操作するのは renderer 内部の `scrollOffset`（数値）と `prevScrollbackLength`（数値）のみ
- 純粋関数 `computeAdjustedScrollOffset` は3整数 in / 2整数 out の閉じた数値演算

## Manual Testing 項目（E2E 不可 / 任意）

VERIFICATION.md「Manual Testing (E2E Not Possible)」セクションから抽出。実行は別途実機で行う。

- [ ] vim / less などの alt-screen アプリ使用中に本機能が誤発火しないことを目視確認（FR5 を補強）
  - 手順: alt-screen アプリ起動 → アプリ内スクロール → 抜けた直後に PTY 出力 → 表示崩れがないか
- [ ] スクロールバーやマウスホイールでの体感確認（NFR1 の主観評価補強）
  - 手順: 大量出力中にマウスホイールで scroll up → 表示行が新規出力に押し流されないことを確認
- [ ] (追加推奨) E2E spec 修正後の再実行確認
  - `./scripts/run-e2e-docker.sh test scroll-pin.e2e.js` が pass すること

## 既知の懸念事項

1. canvas-renderer.ts に同フレーム内で `render()` → `forceRender()` 経路が走るケースがあるが、IMPLEMENTATION.md / Risk Assessment の通り 2 回目は Δ===0 で no-op になる設計が守られている（`adjustScrollOffsetForGrowth` 内で `prevScrollbackLength = nextPrevSbLen` を即時更新）。実装上のリスクとしては解消済み。
2. custom-glyphs.ts に本機能と無関係な未コミット変更あり。本検証スコープ外として扱った。
3. Docker E2E で「実装変更が反映されない」事象に遭遇。原因は `docker-entrypoint.sh` が既存 debug binary を再利用していたこと。`./scripts/run-e2e-docker.sh build-app` で強制再ビルドが必要だった点はナレッジとして残しておく価値あり。

## sdd.yaml requirements ステータス更新の要否

- FR1〜FR6, NFR1, NFR2 すべて元の `status: ok` と矛盾しない。
- TS-7 (E2E) は spec バグのため自動検証は未確認だが、FR1 のコード実装と単体テスト TS-1 で要件は担保されている。
- 結論: **sdd.yaml の requirements 更新は不要**。

## 次のアクション（参考）

1. 任意で Manual Testing 項目を実機で確認
2. 全 E2E スイートに対する regression チェック（`./scripts/run-e2e-docker.sh test`）は本検証では未実施。必要に応じ別途実行
