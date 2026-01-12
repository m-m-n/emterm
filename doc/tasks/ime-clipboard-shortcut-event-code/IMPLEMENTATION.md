# Implementation Plan: IME ON状態でのクリップボードショートカット修正

## Overview

IME（Input Method Editor）がON状態でも、Ctrl+Shift+C/Vのクリップボードショートカットが正常に動作するよう、キー検出ロジックを`event.key`から`event.code`優先に変更する。

## Objectives

- IME ON状態でCtrl+Shift+Cによるコピーが動作すること
- IME ON状態でCtrl+Shift+Vによるペーストが動作すること
- IME OFF状態での既存動作が維持されること
- 他のキーボード処理への影響がないこと

## Prerequisites

### Development Environment

- Bun (パッケージマネージャー、テストランナー)
- TypeScript

### Dependencies

- 既存のKeyboardHandlerクラス
- 既存のテストインフラ（bun:test）

### Knowledge Requirements

- KeyboardEventの`key`と`code`プロパティの違い
- IMEがKeyboardEventに与える影響
- キャプチャフェーズでのイベント処理

## Architecture Overview

### Technology Stack

- **Language**: TypeScript
- **Runtime**: Bun
- **Test Framework**: bun:test

### Design Approach

IME検出時のみ物理キーコード（`event.code`）を使用し、通常時は論理キー（`event.key`）を使用する条件分岐アプローチ。これにより:
- IME ON時: `event.code`で物理キー位置を検出（"Process"問題を回避）
- IME OFF時: `event.key`で論理キーを検出（非QWERTYレイアウトでも正常動作）

### Component Interaction

```
KeyboardEvent (capture phase)
    |
    v
handleClipboardShortcut
    |
    +-- Check Ctrl+Shift modifiers
    |
    +-- Check IME blocking (key === "Process" || "Unidentified" || key.length > 1)
    |       |
    |       +-- IME blocking: use event.code (KeyC/KeyV)
    |       +-- Normal: use event.key (c/v)
    |
    +-- Detect Copy/Paste
    |
    +-- preventDefault/stopPropagation
    |
    +-- handleCopy/handlePaste (既存処理)
```

## Implementation Phases

### Phase 1: キー検出ロジックの修正

**Goal**: handleClipboardShortcutメソッドを修正し、IME検出時のみevent.codeを使用するキー検出に変更する

**Files to Modify**:
- `src/terminal-app/handlers/keyboard.ts`:
  - handleClipboardShortcutメソッドのキー検出ロジックを変更

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleClipboardShortcut | Ctrl+Shift+C/Vの検出と処理 | キャプチャフェーズでKeyboardEventを受信 | コピー/ペースト処理が実行され、イベント伝播が停止 |

**Processing Flow**:

```
1. KeyboardEvent受信
   ├─ Ctrl+Shift両方押下されていない → 何もしない（return）
   └─ Ctrl+Shift両方押下されている → 次へ
2. event.codeとevent.keyを取得
3. IMEブロック判定
   ├─ key === "Process" OR key === "Unidentified" OR key.length > 1 → IMEブロック中
   └─ それ以外 → 通常状態
4. コピー判定
   ├─ IMEブロック中: code === "KeyC" → コピー
   ├─ 通常状態: key === "c" → コピー
   └─ 条件不一致 → 次へ
5. ペースト判定
   ├─ IMEブロック中: code === "KeyV" → ペースト
   ├─ 通常状態: key === "v" → ペースト
   └─ 条件不一致 → 何もしない
```

**Implementation Steps**:

1. **IMEブロック検出の追加**
   - event.keyの値でIME状態を判定
   - "Process", "Unidentified", または長さが1より大きい場合はIMEブロック中
   - Key considerations:
     - IME ON時、event.keyは"Process"または"Unidentified"を返す
     - 一部のIMEでは複数文字を返す場合がある

2. **条件分岐によるキー検出**
   - IMEブロック中: event.codeで物理キー位置を検出
   - 通常状態: event.keyで論理キーを検出（非QWERTYレイアウト対応）
   - Key considerations:
     - 非QWERTYレイアウト（Dvorakなど）ではevent.codeが物理位置を返すため、
       IME OFFでevent.codeを使うとCキーの位置が異なるレイアウトで誤動作する
     - event.keyは論理キーを返すため、レイアウトに関係なく正しく動作

**Dependencies**:
- Requires: なし（自己完結した修正）
- Blocks: なし

**Testing Approach**:

*Unit Tests*:
- IME ON状態（event.key="Process"、event.code="KeyC/KeyV"）でのコピー/ペースト検出
- IME ON状態（event.key="Unidentified"）での検出
- IME OFF状態での既存動作維持
- 非QWERTYレイアウト（IME OFF時にevent.keyで検出されることを確認）

*Manual Testing*:
- [ ] IME ONでCtrl+Shift+C実行 → テキストがコピーされる
- [ ] IME ONでCtrl+Shift+V実行 → テキストがペーストされる
- [ ] IME OFFでCtrl+Shift+C/V実行 → 既存動作と同じ
- [ ] 通常のIME入力が妨げられない

**Acceptance Criteria**:
- [ ] IME ON状態でCtrl+Shift+Cがコピーを実行する
- [ ] IME ON状態でCtrl+Shift+Vがペーストを実行する
- [ ] IME OFF状態での動作が変わらない
- [ ] 既存のテストがすべてパスする
- [ ] 新規テストがすべてパスする

**Estimated Effort**: 小 (1-2 days)

**Risks and Mitigation**:
- **Risk**: event.codeがundefinedまたは空の環境
  - **Mitigation**: event.keyへのフォールバックを維持

---

## Complete File Structure

```
src/terminal-app/handlers/
├── keyboard.ts          # 修正対象: handleClipboardShortcutメソッド
└── keyboard.test.ts     # テスト追加: IME ON状態のテストケース
```

**File Descriptions**:
- `keyboard.ts`: KeyboardHandlerクラス。handleClipboardShortcutメソッドを修正
- `keyboard.test.ts`: 既存テストに加えてIME ON状態のテストケースを追加

## Testing Strategy

### Unit Testing

**Approach**:
- 既存のテストヘルパー（createKeyEvent）を拡張してevent.codeをサポート
- テーブル駆動テストでIME ON/OFF両方のシナリオをカバー

**Test Coverage Goals**:
- handleClipboardShortcut: 100%（修正対象メソッド）
- 新規追加ケース: IME ON状態でのコピー/ペースト検出

**Key Test Areas**:

1. **IME ON状態でのキー検出**
   - event.key="Process"、event.code="KeyC" → コピー実行
   - event.key="Process"、event.code="KeyV" → ペースト実行
   - event.key="Unidentified"、event.code="KeyC" → コピー実行

2. **IME OFF状態での既存動作**
   - event.key="c"、event.code="KeyC" → コピー実行（回帰テスト）
   - event.key="v"、event.code="KeyV" → ペースト実行（回帰テスト）

3. **非QWERTYレイアウト対応**
   - event.key="c"、event.code="KeyI"（Dvorak）→ コピー実行（keyで判定）
   - event.key="v"、event.code="KeyDot"（Dvorak）→ ペースト実行（keyで判定）
   - 注: IME OFFではevent.keyを使用するため、物理位置に関係なく正しく動作

### Manual Testing Checklist

Based on SPEC.md test scenarios:
- [ ] TC-01: IME OFFでコピー
- [ ] TC-02: IME OFFでペースト
- [ ] TC-03: IME ONでコピー
- [ ] TC-04: IME ONでペースト
- [ ] TC-05: 選択なしでコピー
- [ ] TC-06: 空クリップボードでペースト
- [ ] TC-07: 通常キー入力（IME OFF）
- [ ] TC-08: 通常キー入力（IME ON）
- [ ] TC-09: Ctrl+C（シグナル）

## Dependencies

### External Dependencies

なし（既存の依存関係のみ使用）

### Internal Dependencies

**Implementation Order**:
1. Phase 1（単一フェーズ、依存関係なし）

**Component Dependencies**:
- handleClipboardShortcut → handleCopy/handlePaste（既存、変更なし）

## Risk Assessment

### Technical Risks

1. **非QWERTYキーボード + IME ON での制限** [既知の制限]
   - **Risk**: Dvorak、Colemak等の非QWERTYレイアウトでIME ON時にショートカットが動作しない
   - **Likelihood**: 低（非QWERTYレイアウト + IME ON の組み合わせは稀）
   - **Impact**: 中（該当ユーザーはショートカットを使用できない）
   - **Status**: 既知の制限として受容（SPEC.md セクション8.1に文書化）
   - **Workaround**: IMEをOFF状態でショートカットを使用、または右クリックメニューを使用
   - **Future**: ユーザー設定でショートカットキーをカスタマイズ可能にする（将来対応可能）

2. **古いWebViewでのevent.codeサポート**
   - **Risk**: event.codeがサポートされていない
   - **Likelihood**: 極低（主要ブラウザは全てサポート）
   - **Impact**: 低（IME ON時のみ影響）
   - **Mitigation**: Tauriの最小サポートWebViewは全てevent.codeをサポート

3. **IME検出の精度**
   - **Risk**: 一部のIMEで"Process"/"Unidentified"以外の値を返す可能性
   - **Likelihood**: 低（主要なIMEは標準的な値を返す）
   - **Impact**: 低（key.length > 1の条件で追加カバー）
   - **Mitigation**: key.length > 1の条件で複数文字キー（IME変換中の文字列等）もIMEブロックとして扱う

## Performance Considerations

1. **追加のプロパティアクセス**
   - event.codeへの追加アクセスは無視できるオーバーヘッド
   - キー入力ごとに1回のみ実行

## Security Considerations

この修正ではセキュリティ上の考慮事項はなし。キー検出ロジックの変更のみであり、入力検証や権限には影響しない。

## Open Questions

### From Specification:

なし（仕様書で確認済み）

### Implementation-Specific:

なし

## Future Enhancements

この修正はバグフィックスであり、将来の拡張予定はなし。

## Success Metrics

### Functional Completeness
- [ ] IME ON状態でのコピー/ペーストが動作
- [ ] 全テストケースがパス

### Quality Metrics
- [ ] 既存テストが全てパス
- [ ] 新規テストが全てパス
- [ ] TypeScriptの型エラーなし

### Performance Metrics
- [ ] キーボード入力のレスポンスに変化なし

### User Experience
- [ ] IME ON/OFF両方でクリップボード操作が直感的に動作

## References

- **Specification**: `doc/tasks/ime-clipboard-shortcut-event-code/SPEC.md`
- **KeyboardEvent.code**: https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/code
- **KeyboardEvent.key**: https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/key

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - 仕様書との整合性確認
   - 実装アプローチの承認

2. **Begin Implementation**
   - Phase 1の実装開始
   - TDDアプローチ（テスト先行）

3. **Verification**
   - 単体テスト実行
   - 手動テスト実行
   - 回帰テスト確認
