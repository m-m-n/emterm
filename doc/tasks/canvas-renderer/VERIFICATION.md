# Verification Document: Canvas 2D Renderer

## Overview
**Feature**: Canvas 2D Renderer
**要件定義書**: `doc/tasks/canvas-renderer/要件定義書.md`
**技術仕様書**: `doc/tasks/canvas-renderer/SPEC.md`
**実装計画書**: `doc/tasks/canvas-renderer/IMPLEMENTATION.md`

---

## Implementation Results (2026-01-24)

**Status**: Phase 1-4 Complete
**All Tests**: PASS

### Implementation Summary

Canvas 2D APIを使用したターミナルレンダラーを実装し、既存のDOMレンダラーと共存させ、フィーチャーフラグで切り替え可能にした。

### Phase Summary
- [x] Phase 1: Core Rendering Infrastructure
- [x] Phase 2: Attributes and Styling
- [x] Phase 3: Cursor and Selection
- [x] Phase 4: Integration and Feature Flag
- [ ] Phase 5: Migration Complete (pending verification)

### Test Results
```
$ bun test src/terminal/canvas-renderer.test.ts src/terminal/renderer-factory.test.ts
34 pass
12 todo
0 fail
79 expect() calls
Ran 46 tests across 2 files
```

### Files Created
- `src/terminal/canvas-renderer.ts` (886 lines)
- `src/terminal/canvas-renderer.test.ts`
- `src/terminal/renderer-interface.ts` (78 lines)
- `src/terminal/renderer-factory.ts` (54 lines)
- `src/terminal/renderer-factory.test.ts`

### Files Modified
- `src/terminal/index.ts` (added exports)

### Key Functions Implemented
- `groupCellsIntoSpans()` - Cell grouping for efficient rendering
- `getVisibleLines()` - Visible line retrieval
- `calculateScrollPosition()` - Scroll position calculation
- `buildFontString()` - CSS font string construction
- `applyTextAttributes()` - Text attribute extraction
- `normalizeSelection()` - Selection range normalization
- `createRenderer()` - Renderer factory function
- `getRendererType()` - Environment variable reader

### Next Steps
1. Perform manual testing checklist
2. Run `/sdd.6-verify` for automated verification
3. Run `/sdd.7-review` for code review
4. Phase 5 (DOM renderer removal) after verification complete

---

## Build Verification

### Build Command
```bash
bun tauri build
```

### Type Check
```bash
bun run typecheck
```

### Expected Result
- Exit code: 0
- No error messages
- No TypeScript errors

## Test Verification

### Test Command
```bash
bun test
```

### Coverage Target
- **Minimum**: 80%
- **Target**: 90%

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | renderLineが正しくテキストを描画 | fillTextが正しい座標で呼ばれる | Unit |
| TS-2 | renderLineが前景色を適用 | fillStyleが正しく設定される | Unit |
| TS-3 | renderLineが背景色を適用 | fillRectが背景用に呼ばれる | Unit |
| TS-4 | renderLineが太字を処理 | fontにboldが含まれる | Unit |
| TS-5 | renderLineが斜体を処理 | fontにitalicが含まれる | Unit |
| TS-6 | renderLineが下線を描画 | lineTo/strokeが呼ばれる | Unit |
| TS-7 | renderLineが取り消し線を描画 | lineTo/strokeが呼ばれる | Unit |
| TS-8 | renderLineがdim属性を処理 | globalAlphaが0.5に設定される | Unit |
| TS-9 | renderLineがhiddenテキストをスキップ | fillTextが呼ばれない | Unit |
| TS-10 | renderCursorがblockカーソルを描画 | fillRectが1セル分で呼ばれる | Unit |
| TS-11 | renderCursorがunderlineカーソルを描画 | fillRectが下部2pxで呼ばれる | Unit |
| TS-12 | renderCursorがbarカーソルを描画 | fillRectが左端2pxで呼ばれる | Unit |
| TS-13 | measureCharacterSizeが正しいサイズを返す | 正の値のcharWidth/charHeight | Unit |
| TS-14 | Wide文字が2セル幅を占有 | fillTextの位置が2セル分進む | Unit |
| TS-15 | 全画面レンダリングが正しく動作 | 全行が描画される | Integration |
| TS-16 | dirty row最適化が変更行のみ描画 | 未変更行は描画されない | Integration |
| TS-17 | リサイズでCanvas寸法が更新 | canvas.width/heightが更新 | Integration |
| TS-18 | 選択ハイライトが複数行に対応 | 各行にfillRectが呼ばれる | Integration |
| TS-19 | カーソルブリンクが切り替わる | setIntervalが設定される | Integration |
| TS-20 | 環境変数でCanvasレンダラー選択 | CanvasRendererインスタンス | E2E |
| TS-21 | テキスト選択とコピーが動作 | クリップボードにテキスト | E2E |

## Code Quality Verification

### Format Check
```bash
bun run format:check
```

### Static Analysis
```bash
bun run typecheck
```

### Lint Check
```bash
bun run lint
```

## File Structure Verification

### Files to Create

| Path | Purpose | Phase |
|------|---------|-------|
| `src/terminal/canvas-renderer.ts` | Canvas 2Dレンダラー実装 | Phase 1 |
| `src/terminal/renderer-factory.ts` | レンダラー生成ファクトリ | Phase 4 |
| `src/terminal/renderer-interface.ts` | 共通インターフェース定義 | Phase 4 |
| `src/terminal/canvas-renderer.test.ts` | CanvasRendererのテスト | Phase 1-4 |

### Files to Modify

| Path | Changes | Phase |
|------|---------|-------|
| `src/terminal/renderer.ts` | ITerminalRenderer実装を明示化 | Phase 4 |
| `src/terminal/index.ts` | ファクトリとCanvasRendererのエクスポート追加 | Phase 4 |

### Files to Delete (Phase 5)

| Path | Reason |
|------|--------|
| `src/terminal/renderer.ts` | DOMレンダラー不要 |
| `src/terminal/style-cache.ts` | DOM専用キャッシュ不要 |
| `src/terminal/renderer-interface.ts` | 単一レンダラーのため不要 |

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-1 | 全機能要件が実装されテストされている | bun testで全テストパス |
| SC-2 | 視覚出力がDOMレンダラーと一致 | 手動比較テスト |
| SC-3 | パフォーマンスがDOMより改善 | PerformanceMonitorで計測比較 |
| SC-4 | フィーチャーフラグが正しくレンダラーを切り替え | 環境変数設定テスト |
| SC-5 | 全既存ターミナル機能が正常動作 | 手動機能テスト |

### Functional Requirements Coverage

| Requirement | Description | Implementation Phase | Verification |
|-------------|-------------|---------------------|--------------|
| FR1 | Canvas 2D APIでfillTextを使用してテキスト描画 | Phase 1 | Unit test: fillTextが呼ばれる |
| FR2 | 全CellAttributes対応（bold, italic, underline, strikethrough, blink, reverse, hidden, dim） | Phase 2 | Unit test: 各属性の適用確認 |
| FR3 | 前景色/背景色対応（16, 256, RGB） | Phase 2 | Unit test: 色変換確認 |
| FR4 | Canvas内でblock/underline/barカーソル描画 | Phase 3 | Unit test: 各スタイル描画確認 |
| FR5 | JavaScript setIntervalでカーソルブリンク | Phase 3 | Unit test: タイマー設定確認 |
| FR6 | 選択ハイライトオーバーレイ描画 | Phase 3 | Unit test: 選択範囲描画確認 |
| FR7 | Wide文字対応（既存charWidth関数使用） | Phase 1 | Unit test: 2セル幅描画確認 |
| FR8 | スクロールバックバッファ内容表示 | Phase 1 | Integration test: スクロール時表示確認 |
| FR9 | EMTERM_RENDERER環境変数でレンダラー選択 | Phase 4 | E2E test: 環境変数切り替え確認 |

### Non-Functional Requirements Coverage

| Requirement | Description | Verification |
|-------------|-------------|--------------|
| NFR1 | レンダリング時間がDOMレンダラーより短い | PerformanceMonitorで計測比較 |
| NFR2 | TerminalRendererと同じ公開APIを維持 | TypeScript型チェック |
| NFR3 | 出力がDOMレンダラーと視覚的に同一 | 手動比較テスト |
| NFR4 | DOMレンダラーと共通の型・ユーティリティを共有 | コードレビュー |

## Manual Testing Checklist

### Basic Functionality (from 要件定義書 12.1)

- [ ] 通常のテキスト描画が正しく表示される
- [ ] 各種属性（色、太字、斜体、下線）が正しく描画される
- [ ] カーソルが正しい位置に表示され、ブリンクする
- [ ] Wide文字（全角）が正しく2セル幅で表示される
- [ ] テキスト選択時にハイライトが表示される
- [ ] スクロールバックバッファの内容が表示される
- [ ] 環境変数`EMTERM_RENDERER=canvas`でCanvas版が使用される
- [ ] 環境変数未設定でDOM版が使用される（デフォルト）

### Edge Cases

- [ ] 空行が正しく処理される
- [ ] 画面いっぱいのテキストが正しく表示される
- [ ] 不正な文字コード（制御文字など）が安全に処理される
- [ ] Wide文字とASCII文字が混在する行が正しく表示される
- [ ] 長い行（画面幅を超える）が正しく処理される

### Error Handling

- [ ] Canvas 2Dコンテキストが取得できない場合のエラー処理
- [ ] 無効なフォント設定時のフォールバック動作
- [ ] 範囲外のカーソル位置が正しくクランプされる

### Performance

- [ ] 高速スクロール時にフレームドロップが減少している
- [ ] 大量のテキスト出力（`cat large_file.txt`）で応答性が維持される
- [ ] DOMレンダラーより描画時間が短い（PerformanceMonitorで確認）

### Visual Parity

- [ ] デフォルトの前景色/背景色がDOMと一致
- [ ] 16色パレットがDOMと一致
- [ ] 256色がDOMと一致
- [ ] RGB色がDOMと一致
- [ ] 太字の太さがDOMと同等
- [ ] 下線の位置と太さがDOMと同等
- [ ] カーソルの色とサイズがDOMと一致
- [ ] 選択ハイライトの色がDOMと一致

### User Cases (from 要件定義書 3.2)

- [ ] UC01: 通常のターミナル操作（コマンド入力と出力表示）
- [ ] UC02: 高速スクロール（マウスホイール、Page Up/Down）
- [ ] UC03: テキスト選択（ドラッグで範囲選択、コピー）
- [ ] UC04: レンダラー切り替え（環境変数設定後の起動）

## Performance Verification

### Benchmarks

**Performance Requirement**: DOMレンダラーより改善

**Measurement Method**:
```typescript
// PerformanceMonitorを使用した計測
const monitor = getPerformanceMonitor();
monitor.enable();

// テスト操作実行
// ...

const stats = monitor.getStats();
console.log('Average render time:', stats.avgRenderTime);
console.log('Frame drops:', stats.frameDrops);
```

**Comparison Test**:
1. 同じターミナル操作をDOMレンダラーで実行し、メトリクスを記録
2. 同じ操作をCanvasレンダラーで実行し、メトリクスを記録
3. 平均描画時間とフレームドロップ数を比較

**Expected Results**:
- Canvas平均描画時間 < DOM平均描画時間
- Canvasフレームドロップ数 <= DOMフレームドロップ数

## Security Verification

### Security Checks

- [ ] fillTextがHTMLを解釈しないことを確認（XSS対策）
- [ ] 入力データの検証が既存ロジックで継続されている
- [ ] Canvasサイズがターミナルサイズに制限されている

## Phase-by-Phase Verification

### Phase 1: Core Rendering Infrastructure

**Verification Command**:
```bash
bun test src/terminal/canvas-renderer.test.ts --grep "Phase 1"
```

**Acceptance Criteria Checklist**:
- [ ] Canvas要素が正しいサイズで初期化される
- [ ] High DPI環境で鮮明に表示される（devicePixelRatio適用）
- [ ] デフォルト色でテキストが描画される
- [ ] Wide文字が2セル分の幅で描画される
- [ ] groupCellsIntoSpansが正しくセルをグループ化する

### Phase 2: Attributes and Styling

**Verification Command**:
```bash
bun test src/terminal/canvas-renderer.test.ts --grep "Phase 2"
```

**Acceptance Criteria Checklist**:
- [ ] 全SGR属性が正しく描画される
  - [ ] bold
  - [ ] italic
  - [ ] underline
  - [ ] strikethrough
  - [ ] blink
  - [ ] reverse
  - [ ] hidden
  - [ ] dim
- [ ] 16色が正確に表示される
- [ ] 256色が正確に表示される
- [ ] RGB色が正確に表示される
- [ ] reverse属性で前景色と背景色が入れ替わる

### Phase 3: Cursor and Selection

**Verification Command**:
```bash
bun test src/terminal/canvas-renderer.test.ts --grep "Phase 3"
```

**Acceptance Criteria Checklist**:
- [ ] blockカーソルが正しく描画される
- [ ] underlineカーソルが正しく描画される
- [ ] barカーソルが正しく描画される
- [ ] カーソルが500ms間隔でブリンクする
- [ ] 選択範囲が半透明ハイライトで表示される
- [ ] 複数行選択が正しく処理される
- [ ] 逆方向選択（end < start）が正しく正規化される

### Phase 4: Integration and Feature Flag

**Verification Command**:
```bash
bun test src/terminal/renderer-factory.test.ts
EMTERM_RENDERER=canvas bun test --grep "E2E"
```

**Acceptance Criteria Checklist**:
- [ ] EMTERM_RENDERER=canvasでCanvasRendererが生成される
- [ ] EMTERM_RENDERER=domでTerminalRendererが生成される
- [ ] 環境変数未設定でTerminalRendererが生成される（デフォルト）
- [ ] 両レンダラーがITerminalRendererを実装
- [ ] パフォーマンスがDOMレンダラーより改善

### Phase 5: Migration Complete

**Verification Command**:
```bash
bun run typecheck
bun test
bun tauri build
```

**Acceptance Criteria Checklist**:
- [ ] DOMレンダラー関連ファイルが削除されている
  - [ ] `src/terminal/renderer.ts`
  - [ ] `src/terminal/style-cache.ts`
  - [ ] `src/terminal/renderer-interface.ts`
- [ ] ビルドエラーがない
- [ ] TypeScriptエラーがない
- [ ] 全テストがパス
- [ ] アプリケーションが正常に起動する

## Verification Summary

| Category | Items | Automated | Manual |
|----------|-------|-----------|--------|
| Build | 1 | Yes | - |
| Type Check | 1 | Yes | - |
| Unit Tests | 14 | Yes | - |
| Integration Tests | 5 | Yes | - |
| E2E Tests | 2 | Partial | Yes |
| Code Quality | 3 | Yes | - |
| File Structure | 7 | Yes | - |
| SPEC Compliance (SC) | 5 | Partial | Yes |
| Functional Requirements (FR) | 9 | Yes | - |
| Non-Functional Requirements (NFR) | 4 | Partial | Yes |
| Manual Testing | 24 | - | Yes |
| Performance | 3 | Partial | Yes |
| Security | 3 | - | Yes |

**Total**: 約60項目の検証（約35項目が自動化、約25項目が手動）

## Continuous Verification

### CI/CD Integration

各プルリクエストで以下を自動実行:
```yaml
# .github/workflows/test.yml (例)
- bun run typecheck
- bun run lint
- bun test
- bun tauri build
```

### Pre-commit Hooks (推奨)

```bash
bun run format:check
bun run typecheck
bun test --changed
```

## Regression Testing

Phase 5（移行完了）後は以下を定期的に確認:

- [ ] 全既存テストがパスし続ける
- [ ] パフォーマンスメトリクスが維持される
- [ ] 新機能追加時にCanvasレンダラーが正しく動作する
