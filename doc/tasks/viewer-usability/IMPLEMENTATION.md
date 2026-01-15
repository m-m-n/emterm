# Implementation Plan: Viewer Usability Improvements

## Overview

ImageViewerとMarkdownビューアーに共通のズーム機能（25%-400%）と閉じるボタンを追加し、ユーザビリティを向上させる。

## Objectives

- 共通のZoomControllerコンポーネントを作成し、両ビューアーで使用
- マウスホイール（Ctrl併用）、キーボード、UIボタンの3つのズーム入力方法を実装
- 閉じるボタン（右上固定）とズームコントロールバー（右下固定）を追加

## Prerequisites

### Development Environment

- Bun（パッケージ管理・ビルド）
- TypeScript

### Dependencies

- 追加の外部依存なし（ブラウザ標準APIのみ使用）

### Knowledge Requirements

- CSS transform（scale）の理解
- DOM イベントハンドリング（wheel, keydown）
- 既存のImageViewerとFullscreenMarkdownViewの構造

## Architecture Overview

### Technology Stack

- **Language**: TypeScript
- **Styling**: CSS-in-JS（文字列テンプレート）
- **Key Libraries**: 追加なし（ブラウザ標準API）

### Design Approach

共通のZoomControllerクラスを作成し、両ビューアーがコンポジションで使用する。ズームロジック、UI生成、イベント処理を一元化し、コードの重複を排除する。

### Component Interaction

```
┌─────────────────────────────────────────────────┐
│              ZoomController                      │
│  - Zoom state management (level, origin)        │
│  - Event handling (wheel, keyboard, click)      │
│  - UI rendering (close button, zoom bar)        │
│  - Scale transform application                  │
└─────────────────────────────────────────────────┘
         │                    │
         ▼                    ▼
┌─────────────────┐  ┌─────────────────────────┐
│  ImageViewer    │  │  FullscreenMarkdownView │
│  - Uses zoom    │  │  - Uses zoom controller │
│    controller   │  │  - Existing scroll      │
│  - Canvas render│  │    functionality        │
└─────────────────┘  └─────────────────────────┘
```

## Implementation Phases

### Phase 1: Core Zoom Logic

**Goal**: ズーム状態管理とズーム演算の基盤を実装する

**Files to Create**:

- `src/shared/zoom-controller.ts` - ZoomControllerクラス本体
- `src/shared/zoom-styles.ts` - CSS スタイル定義

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ZoomState | ズーム倍率と基準点を保持 | 初期化時に100%を設定 | 25-400%の範囲で倍率を保持 |
| zoomIn() | 現在の倍率を10%増加 | 倍率が400%未満 | 倍率が10%増加（最大400%） |
| zoomOut() | 現在の倍率を10%減少 | 倍率が25%より大きい | 倍率が10%減少（最小25%） |
| zoomTo(level) | 指定倍率に設定 | 任意の数値 | 25-400%にクランプされた倍率 |
| resetZoom() | 倍率を100%にリセット | 任意の状態 | 倍率が100% |
| applyZoom() | transform:scaleを適用 | コンテナ要素が存在 | CSS transformが更新される |

**Processing Flow**:

```
1. ズーム操作を受信
   ├─ zoomIn/zoomOut → 現在倍率 ± 10%
   └─ zoomTo → 指定倍率
2. 新倍率を25-400%にクランプ
3. 状態を更新
4. applyZoom()でtransform:scaleを適用
5. UI表示を更新
```

**Implementation Steps**:

1. **ZoomController クラスの骨格を作成**
   - コンストラクタでオプション（container, overlay, callbacks）を受け取る
   - 初期状態（100%、中央基準点）を設定

2. **ズーム演算メソッドを実装**
   - zoomIn/zoomOut/zoomTo/resetZoomの各メソッド
   - クランプ処理（25-400%範囲制限）
   - transform:scale適用ロジック

**State Management Notes (IMPORTANT)**:

ZoomStateとZoomControllerの状態は密接に関連しており、以下の注意が必要:

1. **単一の状態源**
   - ZoomStateはZoomControllerの内部状態として保持
   - 外部からの直接操作は禁止（publicメソッド経由のみ）

2. **状態の一貫性**
   - zoomIn/zoomOut/zoomTo/resetZoom は全て内部でapplyZoom()を呼び出す
   - UIの更新(updateZoomDisplay)とtransform適用は常にセットで行う
   - 状態変更→transform適用→UI更新の順序を厳守

3. **イベントハンドラからの状態変更**
   - Phase 3でイベントハンドラを追加する際、必ずpublicメソッド経由で状態を変更
   - 直接state.levelを変更しない

4. **破棄時の状態クリア**
   - dispose()時にイベントリスナー解除とUI要素削除を行う
   - 状態のリセットは不要（インスタンスごと破棄されるため）

**Dependencies**:

- Requires: なし（新規ファイル）
- Blocks: Phase 2, 3, 4

**Testing Approach**:

*Unit Tests*:

- 初期化時の倍率が100%であること
- zoomIn()で倍率が10%増加すること
- zoomOut()で倍率が10%減少すること
- 400%でzoomIn()しても400%のままであること
- 25%でzoomOut()しても25%のままであること
- resetZoom()で100%になること
- getZoomLevel()が現在の倍率を返すこと

**Acceptance Criteria**:

- [ ] ZoomControllerが初期倍率100%で初期化される
- [ ] zoomIn()/zoomOut()が10%刻みで動作する
- [ ] 倍率が25%-400%の範囲に制限される
- [ ] resetZoom()で100%にリセットされる

**Estimated Effort**: 小 (1-2 days)

---

### Phase 2: UI Components

**Goal**: 閉じるボタンとズームコントロールバーのUIを実装する

**Files to Modify**:

- `src/shared/zoom-controller.ts`:
  - UI要素（閉じるボタン、ズームバー）の生成メソッドを追加
  - スタイル注入メソッドを追加

- `src/shared/zoom-styles.ts`:
  - 閉じるボタン、ズームボタン、倍率表示のCSSを定義

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| createUI() | 閉じるボタンとズームバーを生成 | overlay要素が存在 | UI要素がDOMに追加される |
| updateZoomDisplay() | 倍率表示を更新 | ズームバーが存在 | 現在倍率が表示される |
| closeButton | クリックで閉じる | 表示中 | onClose()が呼ばれる |
| zoomBar | +/-ボタンと倍率表示 | 表示中 | ズーム操作が可能 |

**Processing Flow**:

```
1. createUI()呼び出し
2. 閉じるボタンを生成してoverlayに追加
   └─ position: fixed, top-right
3. ズームバーを生成してoverlayに追加
   └─ position: fixed, bottom-right
   └─ [-] [100%] [+] レイアウト
4. ボタンクリックハンドラを設定
```

**Implementation Steps**:

1. **CSSスタイルを定義**
   - 閉じるボタン（32x32px、半透明背景、ホバーエフェクト）
   - ズームバー（コンパクトな横並びレイアウト）
   - position: fixedで画面端に固定

2. **UI生成メソッドを実装**
   - createCloseButton(): HTMLElement
   - createZoomBar(): HTMLElement
   - createUI()で両方を生成してoverlayに追加

3. **倍率表示更新を実装**
   - updateZoomDisplay()でテキスト更新

**Dependencies**:

- Requires: Phase 1
- Blocks: Phase 3, 4

**Testing Approach**:

*Unit Tests*:

- UI要素が正しく生成されること
- 閉じるボタンのクリックでonCloseが呼ばれること
- +ボタンでzoomIn()が呼ばれること
- -ボタンでzoomOut()が呼ばれること
- 倍率表示クリックでresetZoom()が呼ばれること
- dispose()でUI要素が削除されること

**Acceptance Criteria**:

- [ ] 閉じるボタンが右上に固定表示される
- [ ] ズームバーが右下に固定表示される
- [ ] 各ボタンにホバーフィードバックがある
- [ ] 倍率表示が現在の倍率を反映する

**Estimated Effort**: 小 (1-2 days)

---

### Phase 3: Event Handling

**Goal**: マウスホイール、キーボード、ボタンクリックのイベント処理を実装する

**Files to Modify**:

- `src/shared/zoom-controller.ts`:
  - イベントリスナー登録・解除
  - 各入力種別のハンドラ実装

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleWheel() | Ctrl+ホイールでズーム | Ctrlキー押下中 | マウス位置基準でズーム |
| handleKeydown() | +/-/0キーでズーム | ビューアーがアクティブ | 中央基準でズーム |
| setupEventListeners() | イベントリスナーを登録 | コンストラクタ呼び出し時 | イベントがハンドルされる |
| removeEventListeners() | イベントリスナーを解除 | dispose()呼び出し時 | リスナーが解除される |

**Processing Flow**:

```
1. イベント受信
   ├─ wheel + Ctrl → handleWheel()
   │   └─ マウス位置から基準点を計算
   ├─ keydown +/=/- → handleKeydown()
   │   └─ 中央を基準点に設定
   └─ button click → handleButtonClick()
       └─ 中央を基準点に設定
2. ズーム演算を実行
3. transform適用
4. UI更新
```

**Implementation Steps**:

1. **ホイールイベントハンドラを実装**
   - Ctrlキー併用時のみズーム処理
   - マウス位置（clientX, clientY）を基準点として記録
   - イベントスロットリング（16ms）で60fps維持

2. **キーボードイベントハンドラを実装**
   - +/=キー: zoomIn()
   - -キー: zoomOut()
   - 0キー: resetZoom()
   - 既存キー（Escape、矢印等）は干渉しない

3. **ボタンクリックハンドラを実装**
   - +ボタン: zoomIn()
   - -ボタン: zoomOut()
   - 倍率表示クリック: resetZoom()

**Implementation Notes (Coordinate Transform)**:

マウス位置基準のズームを実装する際、以下の点に注意が必要:

1. **transform-originの正確な設定**
   - `transform-origin`はコンテナのローカル座標で指定する必要がある
   - `clientX/Y`（ビューポート座標）からの変換が必要:
     ```typescript
     const rect = container.getBoundingClientRect();
     const localX = clientX - rect.left;
     const localY = clientY - rect.top;
     // スケール済みの場合は補正が必要
     const adjustedX = localX / currentScale;
     const adjustedY = localY / currentScale;
     ```

2. **連続ズーム時の基準点維持**
   - ズーム操作ごとにtransform-originを再計算すると、ジャンプが発生する
   - 解決策: ズーム開始時の基準点を記録し、ズーム操作中は維持する

3. **代替アプローチ: translate + scale**
   - より予測可能な動作を得るには、transform-originを中央に固定し、translateで位置を調整:
     ```typescript
     container.style.transformOrigin = 'center center';
     container.style.transform = `translate(${offsetX}px, ${offsetY}px) scale(${scale})`;
     ```

4. **テスト時の確認ポイント**
   - 画像の中央でズームしたとき、中央が維持されること
   - 画像の端でズームしたとき、その点が維持されること
   - 連続ズームイン/アウトで位置がずれないこと

**Dependencies**:

- Requires: Phase 1, Phase 2
- Blocks: Phase 4

**Testing Approach**:

*Unit Tests*:

- Ctrl+ホイールアップでzoomIn()が呼ばれること
- Ctrl+ホイールダウンでzoomOut()が呼ばれること
- Ctrlなしホイールではズームが発生しないこと
- +キーでzoomIn()が呼ばれること
- -キーでzoomOut()が呼ばれること
- 0キーでresetZoom()が呼ばれること
- Escapeキーでズームが発生しないこと（既存動作に干渉しない）

**Acceptance Criteria**:

- [ ] Ctrl+ホイールでマウス位置基準のズームが動作する
- [ ] +/-/0キーで中央基準のズームが動作する
- [ ] 既存のキーボードショートカット（Escape、矢印）が正常動作する
- [ ] イベントスロットリングでパフォーマンスが維持される

**Estimated Effort**: 中 (3-5 days)

---

### Phase 4: Integration

**Goal**: ZoomControllerを両ビューアーに統合する

**Files to Modify**:

- `src/image-viewer/index.ts`:
  - ZoomControllerのインポートと初期化
  - show()でZoomController生成
  - hide()でZoomController破棄
  - 既存のキーボードハンドラを調整

- `src/markdown/fullscreen.ts`:
  - ZoomControllerのインポートと初期化
  - show()でZoomController生成
  - close()でZoomController破棄
  - 既存のキーボードハンドラを調整

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ImageViewer.show() | ZoomController初期化を追加 | 画像表示時 | ズーム機能が有効 |
| ImageViewer.hide() | ZoomController破棄を追加 | 閉じる時 | リソース解放 |
| FullscreenMarkdownView.show() | ZoomController初期化を追加 | Markdown表示時 | ズーム機能が有効 |
| FullscreenMarkdownView.close() | ZoomController破棄を追加 | 閉じる時 | リソース解放 |

**Processing Flow**:

```
1. show()呼び出し
2. 既存のオーバーレイ・コンテンツ生成
3. ZoomController生成
   └─ container: canvas/content
   └─ overlay: オーバーレイ要素
   └─ onClose: hide()/close()
4. 閉じる操作
5. ZoomController.dispose()
6. 既存のクリーンアップ処理
```

**Implementation Steps**:

1. **ImageViewerへの統合**
   - zoomControllerプロパティを追加
   - show()でZoomController初期化
   - hide()でdispose()呼び出し
   - 既存のEscapeキー処理との統合

2. **FullscreenMarkdownViewへの統合**
   - zoomControllerプロパティを追加
   - show()でZoomController初期化
   - close()でdispose()呼び出し
   - 既存のキーボードナビゲーションとの統合

3. **既存機能との整合性確認**
   - Escapeキーでの閉じる動作
   - 矢印キーでのスクロール（Markdown）
   - アニメーション再生（Image）

**Dependencies**:

- Requires: Phase 1, Phase 2, Phase 3
- Blocks: なし

**Testing Approach**:

*Integration Tests*:

- ImageViewerでズーム操作が動作すること
- MarkdownViewでズーム操作が動作すること
- 閉じるボタンでビューアーが閉じること
- Escapeキーでビューアーが閉じること
- 矢印キーでMarkdownがスクロールすること
- ズーム状態がビューアー再表示時にリセットされること

*Manual Testing*:

- [ ] 画像ビューアーでCtrl+ホイールズームが動作
- [ ] Markdownビューアーでキーボードズームが動作
- [ ] GIFアニメーションがズーム中も再生される
- [ ] 長いMarkdownでスクロールとズームが共存

**Acceptance Criteria**:

- [ ] 両ビューアーでズーム機能が動作する
- [ ] 両ビューアーで閉じるボタンが動作する
- [ ] 既存機能（Escape、スクロール）に影響がない
- [ ] ビューアー再表示時にズームが100%にリセットされる

**Estimated Effort**: 中 (3-5 days)

---

## Complete File Structure

```
src/
├── shared/
│   ├── zoom-controller.ts      # ZoomControllerクラス
│   └── zoom-styles.ts          # CSSスタイル定義
├── image-viewer/
│   └── index.ts                # 修正: ZoomController統合
└── markdown/
    └── fullscreen.ts           # 修正: ZoomController統合
```

**File Descriptions**:

- `zoom-controller.ts`: ズーム状態管理、イベント処理、UI生成を担当する共通コンポーネント
- `zoom-styles.ts`: 閉じるボタン、ズームバーのCSSスタイルを文字列として定義
- `index.ts`: ImageViewer - ZoomControllerをコンポジションで使用
- `fullscreen.ts`: FullscreenMarkdownView - ZoomControllerをコンポジションで使用

## Testing Strategy

### Unit Testing

**Approach**:

- Bunのテストランナーを使用
- DOMのモック（happy-dom等）
- イベントシミュレーション

**Test Coverage Goals**:

- ZoomController: 80%+
- ズーム演算ロジック: 100%
- UI生成: 70%+

**Key Test Areas**:

| Area | Test Cases |
|------|------------|
| Zoom Level | 初期値100%、増減10%、範囲制限25-400% |
| Zoom Reset | 100%へのリセット、任意状態からのリセット |
| Event Handling | Ctrl+wheel、+/-/0キー、ボタンクリック |
| UI Generation | 閉じるボタン生成、ズームバー生成、破棄 |
| Dispose | イベントリスナー解除、DOM要素削除 |

### Integration Testing

**Scenarios**:

| Scenario | Steps | Expected |
|----------|-------|----------|
| Image Zoom | 画像表示→Ctrl+ホイール→倍率変化 | 画像がスケール変換される |
| Markdown Zoom | Markdown表示→+キー→倍率変化 | コンテンツがスケール変換される |
| Close Button | ビューア表示→閉じるボタンクリック | ビューアーが閉じる |
| Existing Keys | ビューア表示→Escapeキー | ビューアーが閉じる |

### Manual Testing Checklist

**基本機能**:

- [ ] Ctrl+ホイール上でズームイン
- [ ] Ctrl+ホイール下でズームアウト
- [ ] +キーでズームイン
- [ ] -キーでズームアウト
- [ ] 0キーでリセット
- [ ] +ボタンでズームイン
- [ ] -ボタンでズームアウト
- [ ] 倍率表示クリックでリセット
- [ ] 閉じるボタンで閉じる

**境界値**:

- [ ] 400%でさらにズームインしても400%維持
- [ ] 25%でさらにズームアウトしても25%維持

**回帰**:

- [ ] Escapeキーでビューアーが閉じる
- [ ] Markdownで矢印キースクロール動作
- [ ] GIFアニメーション再生継続

## Dependencies

### External Dependencies

なし（ブラウザ標準APIのみ）

### Internal Dependencies

**Implementation Order**:

1. Phase 1: Core Zoom Logic（依存なし）
2. Phase 2: UI Components（Phase 1に依存）
3. Phase 3: Event Handling（Phase 1, 2に依存）
4. Phase 4: Integration（Phase 1, 2, 3に依存）

**Component Dependencies**:

- `zoom-controller.ts` は `zoom-styles.ts` をインポート
- `image-viewer/index.ts` は `zoom-controller.ts` をインポート
- `markdown/fullscreen.ts` は `zoom-controller.ts` をインポート

## Risk Assessment

### Technical Risks

1. **ホイールイベントの競合**
   - **Risk**: Markdownのスクロールとズームが干渉する可能性
   - **Likelihood**: 中
   - **Impact**: 中（UX低下）
   - **Mitigation**: Ctrlキー併用でズーム操作を明示的に区別

2. **マウス位置基準ズームの複雑性**
   - **Risk**: transform-originの動的計算が複雑
   - **Likelihood**: 中
   - **Impact**: 低（動作には影響しない）
   - **Mitigation**: シンプルな実装から始め、必要に応じて改良

3. **既存キーボードハンドラとの干渉**
   - **Risk**: +/-キーが既存処理と競合する可能性
   - **Likelihood**: 低（既存コードを確認済み）
   - **Impact**: 高（機能破壊）
   - **Mitigation**: 既存ハンドラを維持しつつ、ズームキーを追加

## Performance Considerations

1. **ズーム操作のレスポンス**
   - transform: scale()はGPUアクセラレーションを活用
   - 16ms以内（60fps）を目標

2. **イベントスロットリング**
   - ホイールイベントを16msでスロットル
   - 過剰な再計算を防止

3. **メモリ**
   - ズーム操作中の追加アロケーションを最小化
   - dispose()で確実にリソース解放

## Security Considerations

1. **入力バリデーション**
   - ズーム倍率を25-400%にクランプ
   - 不正な値を受け付けない

2. **イベントハンドリング**
   - ユーザーデータの送信や保存なし
   - 既存のセキュリティモデルを維持

## Open Questions

### From Specification

なし（全て確認済み）

### Implementation-Specific

なし

## Future Enhancements

仕様書に記載なし。追加機能は別タスクで検討。

## Success Metrics

### Functional Completeness

- [ ] 全ズーム操作（ホイール、キー、ボタン）が動作
- [ ] 閉じるボタンが動作
- [ ] 両ビューアーで統一された操作性

### Quality Metrics

- [ ] ZoomControllerのテストカバレッジ80%以上
- [ ] 手動テスト項目全パス
- [ ] 既存機能への回帰なし

### Performance Metrics

- [ ] ズーム操作16ms以内
- [ ] UI更新5ms以内

## References

- **Specification**: `doc/tasks/viewer-usability/SPEC.md`
- **Requirements**: `doc/tasks/viewer-usability/要件定義書.md`
- **Existing Implementation**: `src/image-viewer/index.ts`
- **Existing Implementation**: `src/markdown/fullscreen.ts`

## Next Steps

1. **レビューと承認**
   - 実装計画の確認
   - 不明点の解消

2. **環境準備**
   - `src/shared/` ディレクトリ作成

3. **実装開始**
   - Phase 1から順次実装
   - TDDアプローチ（テスト先行）

4. **検証**
   - `/sdd.3-verify-plan` で整合性検証
   - テスト実行
   - 手動テスト
