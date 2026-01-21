# Implementation Plan: Default Font Settings

## Overview

eMterm端末エミュレータのデフォルトフォント設定を更新し、Inconsolata（ASCII用）、Noto Sans JP（日本語用）、Noto Color Emoji（絵文字用）をフォントスタックとして設定する。フォントサイズは13pt、行高は15ptに変更する。

## Objectives

- Inconsolataを主要モノスペースフォントとして設定
- Noto Sans JPで日本語文字を一貫してレンダリング
- Noto Color Emojiでカラー絵文字を表示
- フォントサイズを13pt（約17.33px）に設定
- 行高を15ptに設定して適切な行間を維持

## Prerequisites

### Development Environment
- Bun（パッケージマネージャ）
- Tauri開発環境

### Dependencies
- 外部依存なし（CSSの変更のみ）

### Knowledge Requirements
- CSS font-family の仕組み
- CSSカスタムプロパティ（CSS変数）

## Architecture Overview

### Technology Stack
- **Language**: CSS
- **Framework**: N/A（純粋なCSSスタイルシート変更）

### Design Approach
フォント設定はCSSで集中管理され、`body`要素のfont-familyプロパティから全体に継承される。CSS変数でフォントサイズと行高を定義し、ターミナルコンポーネントがこれを参照する。

### Component Interaction
```
:root (CSS変数定義)
    ├── --terminal-font-size: 13pt
    └── --terminal-line-height: 15pt
         │
         ▼
body (font-family定義)
    └── font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace
         │
         ▼
#terminal (継承 + CSS変数使用)
    ├── font-family: 継承
    ├── font-size: var(--terminal-font-size)
    └── line-height: var(--terminal-line-height)
         │
         ▼
.markdown-content code (個別設定)
    └── font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace
```

## Implementation Phases

### Phase 1: CSS Font Stack and Size Configuration

**Goal**: ターミナル全体のフォント設定を更新し、指定されたフォントスタックとサイズを適用する

**Files to Modify**:
- `src/styles.css` - フォントファミリーとサイズの設定

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| :root CSS変数 | フォントサイズと行高の定義 | 既存の14px/16px設定 | 13pt/15pt設定に更新 |
| body font-family | 全体のフォントスタック定義 | Menlo系フォントスタック | Inconsolata系スタックに更新 |
| .markdown-content code | コードブロックのフォント | Menlo系フォントスタック | Inconsolata系スタックに更新 |
| .link-confirm-url | リンク確認ダイアログのURL表示 | monospace | Inconsolata系スタックに更新 |
| .image-viewer-info | 画像ビューア情報表示 | monospace | Inconsolata系スタックに更新 |

**Processing Flow**:
```
1. CSS変数の更新
   └─ --terminal-font-size と --terminal-line-height を変更
2. body要素のfont-family更新
   └─ 新しいフォントスタックを設定
3. Markdownコードブロックのfont-family更新
   └─ 新しいフォントスタックを設定（body継承では不足のため）
4. リンク確認ダイアログURL表示のfont-family更新
   └─ .link-confirm-url のフォントスタックを更新
5. 画像ビューア情報表示のfont-family更新
   └─ .image-viewer-info のフォントスタックを更新
```

**Implementation Steps**:

1. **CSS変数の更新**
   - `:root`セレクタ内の`--terminal-font-size`を`14px`から`13pt`に変更
   - `--terminal-line-height`を`16px`から`15pt`に変更
   - Key consideration: pt単位への変更（ユーザーのシステム設定に依存しない）

2. **body要素のfont-family更新**
   - `font-family`プロパティを新しいフォントスタックに置換
   - フォールバック順序: Inconsolata -> Noto Sans JP -> Noto Color Emoji -> monospace
   - Key consideration: 各フォントが利用可能でない場合の適切なフォールバック

3. **Markdownコードブロックのfont-family更新**
   - `.markdown-content code`セレクタのfont-familyを更新
   - body継承ではなく明示的に設定（Markdownコンテンツは別のフォントファミリーを持つため）

4. **リンク確認ダイアログURL表示のfont-family更新**
   - `.link-confirm-url`セレクタのfont-familyを更新（Line 467）
   - URLを表示するためモノスペースフォントが適切
   - Key consideration: URL表示の一貫性を維持

5. **画像ビューア情報表示のfont-family更新**
   - `.image-viewer-info`セレクタのfont-familyを更新（Line 553）
   - 画像サイズやズーム率などの情報表示に使用
   - Key consideration: 数値表示の一貫性を維持

**Dependencies**:
- Requires: なし
- Blocks: なし（単一フェーズ）

**Testing Approach**:

*Manual Testing*:
- [ ] ASCII文字の表示確認
- [ ] 日本語文字（ひらがな、カタカナ、漢字）の表示確認
- [ ] カラー絵文字の表示確認
- [ ] 混合テキスト（ASCII + 日本語 + 絵文字）の表示確認
- [ ] フォントサイズが適切か目視確認
- [ ] 行間が適切か目視確認
- [ ] Markdownインラインコードのフォント確認
- [ ] IME入力時のフォント継承確認

**Acceptance Criteria**:
- [ ] body要素のfont-familyが"Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospaceに設定されている
- [ ] --terminal-font-sizeが13ptに設定されている
- [ ] --terminal-line-heightが15ptに設定されている
- [ ] .markdown-content codeのfont-familyが更新されている
- [ ] .link-confirm-urlのfont-familyが更新されている
- [ ] .image-viewer-infoのfont-familyが更新されている
- [ ] 既存のターミナル機能に影響がない

**Estimated Effort**: 小 (1-2 hours)

**Risks and Mitigation**:
- **Risk**: フォントがシステムにインストールされていない場合、表示が異なる
  - **Mitigation**: フォールバックとしてmonospaceを最後に指定

---

## Complete File Structure

```
src/
└── styles.css    # フォントファミリーとサイズの設定（変更対象）
```

**File Descriptions**:
- `src/styles.css`: アプリケーション全体のスタイル定義。フォント設定は:root（CSS変数）、body（継承元）、および特定コンポーネント（Markdownコード）で定義。

## Testing Strategy

### Unit Testing

**Approach**:
- CSS変更のため自動単体テストは不要
- 視覚的な検証が主体

### Integration Testing

**Scenarios**:
- ターミナル全体でのフォント適用確認
- Markdownレンダリングでのフォント適用確認

### Manual Testing Checklist

Based on spec test scenarios:

**Visual Tests**:
- [ ] ASCII text displays correctly with Inconsolata
- [ ] Japanese text (ひらがな、カタカナ、漢字) displays correctly
- [ ] Emoji characters display in color
- [ ] Mixed text (ASCII + Japanese + Emoji) displays correctly
- [ ] Font size appears as 13pt (approximately 17.33px)
- [ ] Line spacing is appropriate at 15pt (no overlap, no excessive gaps)

**Fallback Tests**:
- [ ] Terminal displays correctly when Inconsolata is not installed
- [ ] Terminal displays correctly when Noto Sans JP is not installed
- [ ] Terminal displays correctly when Noto Color Emoji is not installed

**Regression Tests**:
- [ ] Markdown inline code uses the updated font
- [ ] IME composition view inherits correct font
- [ ] Existing terminal functionality is not affected

**Test Commands**:
```bash
# Display ASCII characters
echo "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
echo "abcdefghijklmnopqrstuvwxyz"
echo "0123456789"
echo '!"#$%&'\''()*+,-./:;<=>?@[\]^_`{|}~'

# Display Japanese characters
echo "あいうえお かきくけこ"
echo "アイウエオ カキクケコ"
echo "日本語表示テスト 漢字"

# Display emoji
echo "Hello 🎉 World 🌍 Test 💻"

# Mixed content
echo "Hello 世界 🌍"
echo "ファイル名: test.txt 📄"
```

## Dependencies

### External Dependencies

| Package | Version | Purpose | Installation |
|---------|---------|---------|--------------|
| N/A | - | CSSのみの変更のため外部依存なし | - |

### Internal Dependencies

**Implementation Order**:
1. Phase 1 (単一フェーズ、依存なし)

## Risk Assessment

### Technical Risks

1. **フォント未インストール時の表示**
   - **Risk**: ユーザーのシステムにInconsolata、Noto Sans JP、Noto Color Emojiがインストールされていない場合、システムのデフォルトmonospaceフォントにフォールバックする
   - **Likelihood**: Medium（フォントはシステムによって異なる）
   - **Impact**: Low（フォールバックが機能する）
   - **Mitigation**: フォントスタックの最後にmonospaceを指定してフォールバックを保証

2. **pt単位での表示差異**
   - **Risk**: pt単位はDPI設定により物理サイズが異なる可能性
   - **Likelihood**: Low（モダンなシステムでは一貫性がある）
   - **Impact**: Low（読みやすさに大きな影響なし）
   - **Mitigation**: 13pt/15ptは一般的なサイズで問題なし

## Performance Considerations

1. **フォント読み込み**
   - システムフォントを使用するため、追加のネットワーク要求なし
   - パフォーマンスへの影響なし

## Security Considerations

1. **フォント設定**
   - システムフォントの参照のみでセキュリティリスクなし

## Open Questions

### From Specification:
- なし（仕様は明確）

### Implementation-Specific:
- なし

## Future Enhancements

Items deferred to later phases or releases:

- Configuration file support for user-customizable fonts
- Font weight customization
- Support for additional font fallbacks
- Per-character-class font selection (more granular control)

## Success Metrics

### Functional Completeness
- [ ] All font-family properties updated (body, .markdown-content code, .link-confirm-url, .image-viewer-info)
- [ ] Font size changed to 13pt
- [ ] Line height changed to 15pt
- [ ] All visual tests pass

### Quality Metrics
- [ ] No regression in existing functionality
- [ ] Code follows existing CSS conventions

### User Experience
- [ ] ASCII characters are readable
- [ ] Japanese text renders correctly
- [ ] Emoji display in color

## References

- **Specification**: `doc/tasks/default-font-settings/SPEC.md`
- **Requirements Document**: `doc/tasks/default-font-settings/要件定義書.md`
- **Current CSS**: `src/styles.css`
- **Inconsolata**: https://fonts.google.com/specimen/Inconsolata
- **Noto Sans JP**: https://fonts.google.com/specimen/Noto+Sans+JP
- **Noto Color Emoji**: https://fonts.google.com/noto/specimen/Noto+Color+Emoji

## Next Steps

After reviewing this implementation plan:

1. **Review and Approval**
   - 計画内容の確認

2. **Begin Implementation**
   - `/sdd.4-implement` で実装を開始

3. **Verification**
   - 手動テストで視覚的確認
   - フォールバック動作の確認
