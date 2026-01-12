# Implementation Plan: IME Clipboard Shortcut Support

## Overview

IME (Input Method Editor) がアクティブな状態でも Ctrl+Shift+C/V によるコピー/ペースト操作を可能にするため、KeyboardHandler クラスにキャプチャフェーズのイベントリスナーを追加する。

## Objectives

- Ctrl+Shift+C/V ショートカットを IME アクティブ時にも動作させる
- 既存のキーボード処理との後方互換性を維持する
- イベントリスナーの適切なクリーンアップを保証する

## Prerequisites

### Development Environment
- Bun (package manager)
- TypeScript 5.x
- Tauri development environment

### Dependencies
- 外部依存なし（既存のモジュールのみ使用）

### Knowledge Requirements
- DOM Event capture/bubble phase の理解
- KeyboardHandler クラスの既存構造の把握

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (Vanilla)
- **Framework**: DOM Events API
- **Key Libraries**: なし（標準 API のみ）

### Design Approach

現在のバブルフェーズリスナーに加えて、キャプチャフェーズリスナーを追加する。キャプチャフェーズは DOM イベントフローの最初に実行されるため、IME やOS のデフォルト処理よりも先にショートカットを捕捉できる。

### Component Interaction

```
Event Flow:
  keydown event
    |
    v
  Capture Phase (root -> target)
    |
    +-> handleClipboardShortcut (NEW)
    |     - Ctrl+Shift+C/V のみ処理
    |     - 該当時は stopPropagation()
    |
    v
  Bubble Phase (target -> root)
    |
    +-> handleKeyDown (existing)
          - 通常のキー処理
```

## Implementation Phases

### Phase 1: Capture Phase Listener Implementation

**Goal**: キャプチャフェーズでクリップボードショートカットを処理する機能を追加する

**Files to Modify**:
- `src/terminal-app/handlers/keyboard.ts`:
  - 新しいプライベートプロパティ追加
  - 新しいプライベートメソッド追加
  - attach() メソッド修正
  - detach() メソッド修正

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| boundHandleClipboardShortcut | キャプチャリスナーの関数参照を保持 | null | 関数参照またはnull |
| handleClipboardShortcut | Ctrl+Shift+C/V を検出して処理 | KeyboardEvent を受信 | 該当イベントは stopPropagation |
| attach (修正) | キャプチャリスナーを登録 | target が有効 | 両リスナーが登録済み |
| detach (修正) | キャプチャリスナーを解除 | attach 済み | 両リスナーが解除済み |

**Processing Flow**:
```
handleClipboardShortcut(event):
1. modifier key check
   +-- ctrlKey == false OR shiftKey == false -> return (let event continue)
   |
2. key identification
   +-- key == "c" -> preventDefault, stopPropagation, call handleCopy, return
   +-- key == "v" -> preventDefault, stopPropagation, call handlePaste, return
   +-- otherwise -> return (let event continue)

CRITICAL: preventDefault/stopPropagation must be called SYNCHRONOUSLY
before any async operation (handleCopy/handlePaste may be async).
This ensures the event is cancelled before the browser's event loop continues.
```

**Implementation Steps**:

1. **Add private property for capture listener reference**
   - boundHandleClipboardShortcut プロパティを追加
   - 型: `((e: KeyboardEvent) => void) | null`
   - 初期値: null

2. **Implement handleClipboardShortcut method**
   - Ctrl+Shift の組み合わせを早期チェック
   - "c" キーの場合: **先に** preventDefault() と stopPropagation() を**同期的に**呼び出し、その後 handleCopy を実行
   - "v" キーの場合: **先に** preventDefault() と stopPropagation() を**同期的に**呼び出し、その後 handlePaste を実行
   - **CRITICAL**: 非同期処理（handleCopy/handlePaste）の前に必ず同期的にイベントをキャンセルすること

3. **Modify attach method**
   - **既にアタッチ済みの場合は先に detach() を呼び出してクリーンアップ**
     - boundHandleClipboardShortcut または boundHandleKeyDown が非 null の場合
     - これによりリスナーの二重登録を防止
   - キャプチャリスナーをバブルリスナーより先に登録
   - `{ capture: true }` オプションを指定

4. **Modify detach method**
   - キャプチャリスナーの解除処理を追加
   - `{ capture: true }` オプションを指定して正しく解除
   - boundHandleClipboardShortcut を null にリセット

**Dependencies**:
- Requires: なし
- Blocks: なし

**Testing Approach**:

*Unit Tests*:
- handleClipboardShortcut が Ctrl キーなしで何もしないことを検証
- handleClipboardShortcut が Shift キーなしで何もしないことを検証
- handleClipboardShortcut が Ctrl+Shift+C で handleCopy を呼ぶことを検証
- handleClipboardShortcut が Ctrl+Shift+V で handlePaste を呼ぶことを検証
- handleClipboardShortcut が他の Ctrl+Shift 組み合わせを無視することを検証
- attach() がキャプチャフェーズリスナーを登録することを検証
- detach() がキャプチャフェーズリスナーを解除することを検証
- attach/detach の複数回サイクルが正常に動作することを検証
- attach() の二重呼び出しで自動的に detach() が呼ばれることを検証

*Manual Testing*:
- [ ] 日本語 IME を有効にして Ctrl+Shift+C でコピーできることを確認
- [ ] 日本語 IME を有効にして Ctrl+Shift+V でペーストできることを確認
- [ ] IME 無効時の通常操作が影響を受けないことを確認

**Acceptance Criteria**:
- [ ] boundHandleClipboardShortcut プロパティが追加されている
- [ ] handleClipboardShortcut メソッドが実装されている
- [ ] attach() がキャプチャリスナーを登録している
- [ ] detach() がキャプチャリスナーを解除している
- [ ] 全ユニットテストがパスする
- [ ] 型チェックがパスする

**Estimated Effort**: 小 (1-2 days)

**Risks and Mitigation**:
- **Risk**: キャプチャリスナーが他のショートカットに影響を与える
  - **Mitigation**: Ctrl+Shift+C/V のみを処理し、他は早期リターン

---

## Complete File Structure

```
src/
└── terminal-app/
    └── handlers/
        ├── keyboard.ts         # Modified - capture listener added
        └── keyboard.test.ts    # New - unit tests
```

**File Descriptions**:
- `keyboard.ts`: KeyboardHandler クラスにキャプチャフェーズリスナーを追加
- `keyboard.test.ts`: 新規追加するユニットテストファイル

## Testing Strategy

### Unit Testing

**Approach**:
- Bun の組み込みテストフレームワークを使用
- モック KeyboardEvent を使用したテスト
- 既存の `pty/keyboard.test.ts` のパターンに従う

**Test Coverage Goals**:
- handleClipboardShortcut: 100% 分岐カバレッジ
- attach/detach: リスナー登録/解除の検証

**Key Test Areas**:

1. **handleClipboardShortcut メソッド**
   - Ctrl キーなしのイベント → 何もしない
   - Shift キーなしのイベント → 何もしない
   - Ctrl+Shift+C → handleCopy 呼び出し
   - Ctrl+Shift+V → handlePaste 呼び出し
   - Ctrl+Shift+X (他のキー) → 何もしない
   - 大文字 "C"/"V" キーの処理

2. **attach/detach メソッド**
   - addEventListener が capture: true で呼ばれる
   - removeEventListener が capture: true で呼ばれる
   - 複数回の attach/detach サイクル

### Manual Testing Checklist

- [ ] 日本語 IME (Google 日本語入力) でひらがな入力中に Ctrl+Shift+C
- [ ] 日本語 IME (Google 日本語入力) でひらがな入力中に Ctrl+Shift+V
- [ ] IME オフ状態での Ctrl+Shift+C
- [ ] IME オフ状態での Ctrl+Shift+V
- [ ] 選択なしでの Ctrl+Shift+C (エラーなし)
- [ ] 空クリップボードでの Ctrl+Shift+V (エラーなし)
- [ ] 複数行テキストのペースト確認ダイアログ表示

## Dependencies

### External Dependencies

なし

### Internal Dependencies

| Module | Purpose |
|--------|---------|
| SelectionController | コピー/ペースト操作 |
| showPasteDialog | マルチラインペースト確認 |
| sendTextInChunks | テキスト送信 |

**Implementation Order**:
1. Phase 1 (単一フェーズのため順序なし)

## Risk Assessment

### Technical Risks

1. **イベント処理順序の影響**
   - **Risk**: キャプチャリスナーが意図しないイベントを消費する可能性
   - **Likelihood**: Low
   - **Impact**: High
   - **Mitigation**: Ctrl+Shift+C/V のみを厳密にチェックし、早期リターン

2. **既存動作への影響**
   - **Risk**: 既存の handleKeyDown の処理が変わる可能性
   - **Likelihood**: Low
   - **Impact**: Medium
   - **Mitigation**: handleKeyDown は変更せず、キャプチャリスナーは独立して動作

## Performance Considerations

1. **イベント処理時間**
   - キャプチャリスナーは Ctrl+Shift チェックを最初に行い、不一致時は即座にリターン
   - 処理時間: < 1ms (仕様要件 NFR1)

2. **メモリオーバーヘッド**
   - 追加される参照: 1 つの関数参照 (~8 bytes)
   - 無視できるレベル

## Security Considerations

1. **クリップボードアクセス**
   - 既存の SelectionController 経由でセキュアにアクセス
   - 新たなセキュリティ面の変更なし

2. **イベント伝播**
   - stopPropagation() は意図したショートカットのみに適用
   - 他のイベントハンドラへの影響なし

## Open Questions

なし - 仕様書で実装方針が決定済み

## Success Metrics

### Functional Completeness
- [ ] FR1-FR5 すべて実装完了
- [ ] 全ユニットテストパス
- [ ] 手動テストで IME + ショートカット動作確認

### Quality Metrics
- [ ] TypeScript 型チェックパス (`bun run typecheck`)
- [ ] 既存テストに影響なし

### User Experience
- [ ] IME アクティブ時に Ctrl+Shift+C でコピー可能
- [ ] IME アクティブ時に Ctrl+Shift+V でペースト可能
- [ ] 既存のワークフローに影響なし

## References

- **Specification**: `doc/tasks/ime-clipboard-shortcut/SPEC.md`
- **Existing implementation**: `src/terminal-app/handlers/keyboard.ts`
- **Test pattern reference**: `src/pty/keyboard.test.ts`
- **MDN EventTarget.addEventListener()**: https://developer.mozilla.org/en-US/docs/Web/API/EventTarget/addEventListener
- **DOM Events Capture/Bubble**: https://javascript.info/bubbling-and-capturing

## Next Steps

1. **Review and Approval**
   - 本計画書のレビュー
   - 不明点があれば確認

2. **Begin Implementation**
   - `/sdd.4-implement` で実装開始
   - TDD アプローチ (テスト先行)

3. **Verification**
   - `/sdd.6-verify` で検証実行
