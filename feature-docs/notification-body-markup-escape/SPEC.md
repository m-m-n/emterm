# Feature: notification-body-markup-escape

## Overview

OS デスクトップ通知の本文に渡されるタブタイトルは、OSC 0/2 により任意の子プロセスから書き換え可能で、攻撃者の影響下にある。本文マークアップ対応の通知サーバー（GNOME Shell / Plasma / dunst の markup=full）では、そのタイトルがマークアップとして解釈される。本機能は、通知本文へ渡るタイトルのマークアップメタ文字（`&` `<` `>`）を、`get_capabilities()` が `"body-markup"` を報告する場合に限りエスケープする。

要件の詳細は [REQUIREMENTS.md](./REQUIREMENTS.md) を参照。

## Objectives

- 本文マークアップ対応の通知サーバー上で、OSC 0/2 由来のタブタイトルによるマークアップインジェクション（フィッシングリンク、偽装された書式）を防ぐ。
- 通知本文中の不正マークアップに起因する通知消失・パース失敗を防ぐ。
- レビュー指摘 afa7e833c3f394a8（PR #31 round1、severity medium、category security、confidence 95、Claude と Codex のクロスモデル合意）をクローズする。

## User Stories

### US1: 本文マークアップ対応サーバーでのインジェクション防止

Linux 上の eMterm 利用者として、本文マークアップ対応の通知サーバーでも、タブタイトルがマークアップとして解釈されずリテラルテキストとして表示されてほしい。攻撃者が制御するタイトルによるフィッシングリンクや偽装書式を防ぐため。

**Acceptance Criteria:**

- [ ] 通知本文タイトルのマークアップメタ文字（`&` -> `&amp;`、`<` -> `&lt;`、`>` -> `&gt;`）をエスケープするコード経路が存在する。
- [ ] `<a href="https://attacker.invalid">Sign in</a>` のようなタイトル（タグ・実体参照を含む）が本文でリテラルテキストのみになることを固定するユニットテストが存在する。

### US2: 本文マークアップ非対応サーバーでの表示汚染回避

本文マークアップ非対応の通知サーバーを使う eMterm 利用者として、通知本文に `&amp;` のような実体参照が生テキストとして現れないでほしい。通知の可読性を保つため。

**Acceptance Criteria:**

- [ ] エスケープは notify_rust の `get_capabilities()` が `"body-markup"` を確認した場合にのみ適用され、body-markup を通知しないサーバーで `&amp;` が生表示されない。
- [ ] エスケープは 100 文字トランケートの後に実行され、実体参照がシーケンス途中で分断されない。

## Technical Requirements

### Functional Requirements

- **FR1 - Escape markup metacharacters in the notification body title:** OS 通知の本文に渡されるタイトルテキストを、notify_rust の `.body()` に到達する前に `&` -> `&amp;`、`<` -> `&lt;`、`>` -> `&gt;` へエスケープし、本文マークアップ対応サーバー（GNOME Shell / Plasma / dunst の markup=full）がリテラルテキストとして描画するようにする。生成済みの実体参照が二重エスケープされないよう、`&` を最初に置換する。
- **FR2 - Escape after the 100-character truncation:** エスケープはサニタイズパイプラインの既存の 100 文字トランケートの後に実行し、エスケープ実体参照がシーケンス途中で切断されないようにする。
- **FR3 - Gate escaping on the server's body-markup capability:** エスケープは notify_rust の `get_capabilities()` が `"body-markup"` を報告する場合にのみ適用する。ケイパビリティが存在しない場合、または確認できない場合は本文を未エスケープで渡し、プレーンテキストサーバーで `&amp;` が生テキストとして見えないようにする。
- **FR4 - Cover both notification paths that share sanitize_title:** 本修正はエージェント通知経路とタブアクティビティ通知経路の双方を対象とする。両経路は `sanitize_title`（src-tauri/src/notifications.rs:145）を共有し、`NotifyRustSink::send`（src-tauri/src/callbacks.rs:145）に接続する。
- **FR5 - Windows notification path unchanged:** ケイパビリティ判定は Linux 固有（D-Bus 上の org.freedesktop.Notifications）である。Windows の通知経路にはケイパビリティ判定もエスケープ処理も追加しない。

### Non-Functional Requirements

- **NFR1 - Compatibility (existing sanitize_title behavior preserved):** CSI シーケンス除去、C0/DEL/C1 制御文字除去、入力上限、100 文字トランケートは従来どおりに動作する。追加されるのは新しいエスケープ手順のみ。
- **NFR2 - Security (injection choke point):** エスケープ処理は、OSC 0/2 由来のタイトルテキストが notify_rust の `.body()` に到達するすべての経路上に配置し、信頼できないタイトルが迂回できないようにする。
- **NFR3 - Maintainability (feature-gate and platform-gate hygiene):** notify-rust は `gui` フィーチャーのオプショナル依存であるため、新規コードは既存の `#[cfg(feature = "gui")]` / `#[cfg(unix)]` / `#[cfg(windows)]` ゲート規約に従い、`--no-default-features`（CLI のみ）ビルドと Windows ビルドがコンパイル可能な状態を維持する。

## Implementation Approach

### Architecture

**System Architecture:**

```
┌─────────────────────────────────────────────┐
│ 子プロセス (OSC 0/2 でタイトル設定)           │  ← 信頼できない入力
├─────────────────────────────────────────────┤
│ タブタイトル                                  │
├─────────────────────────────────────────────┤
│ sanitize_title                               │
│   (src-tauri/src/notifications.rs:145)       │
│   CSI 除去 / 制御文字除去 / 入力上限 /        │
│   100 文字トランケート                        │
├─────────────────────────────────────────────┤
│ マークアップエスケープ (FR1/FR2/FR3)          │
├─────────────────────────────────────────────┤
│ NotifyRustSink::send                         │
│   (src-tauri/src/callbacks.rs:145)           │
├─────────────────────────────────────────────┤
│ notify_rust .body()                          │
├─────────────────────────────────────────────┤
│ org.freedesktop.Notifications (D-Bus, Linux) │
└─────────────────────────────────────────────┘
```

**Component Diagram:**

エージェント通知経路とタブアクティビティ通知経路の 2 つが `sanitize_title` を共有し、`NotifyRustSink::send` を経て notify_rust の `.body()` に至る（FR4）。ケイパビリティ判定は notify_rust の `get_capabilities()` により Linux 側でのみ行う（FR3 / FR5）。

**実装位置は plan フェーズに委譲する。** タスクが提示する 2 案は次のとおりで、どちらも全受け入れ基準を満たす。

- (a) `sanitize_title` の末尾: 両タイトル経路の単一チョークポイント。既存の `sanitize_title` / `notification_body` のテスト期待値の更新が必要。
- (b) `NotifyRustSink::send` の `.body()` 直前: `window_host/link_hover.rs` の notify 呼び出しなど、他の本文生成元も追加でカバーする。

### Data Flow

```
子プロセス → OSC 0/2 → タブタイトル → sanitize_title
  → (CSI/制御文字除去・入力上限) → 100 文字トランケート
  → get_capabilities() に "body-markup" があるか?
       Yes → & -> &amp; を先に置換 → < -> &lt; → > -> &gt; → .body()
       No / 確認不可 → 未エスケープのまま .body()
```

### API Design

新規の HTTP/API エンドポイントはない。外部インターフェースは notify_rust 経由の D-Bus のみ。

| 呼び出し | 用途 | 失敗時の扱い |
|----------|------|--------------|
| notify_rust `get_capabilities()` | `"body-markup"` の有無を判定する | 「ケイパビリティ未確認」として扱い、エスケープしない |
| notify_rust `.body()` | 通知本文を渡す（エスケープ後の到達点） | - |

### Database Schema

該当なし（永続化データの追加・変更はない）。

### Dependencies

**Internal Dependencies:**

- `sanitize_title`（src-tauri/src/notifications.rs:145）: エスケープが接続されるサニタイズパイプライン。既存挙動は NFR1 のとおり保持する。
- `NotifyRustSink::send`（src-tauri/src/callbacks.rs:145）: 両通知経路が合流する送出処理。
- src-tauri/src/window_host/link_hover.rs の notify 呼び出し: 実装位置案 (b) を採る場合に追加でカバーされる本文生成元。

**External Dependencies:**

- notify-rust: `gui` フィーチャーのオプショナル依存。`get_capabilities()` と `.body()` を提供する。
- org.freedesktop.Notifications（D-Bus、Linux）: ケイパビリティ報告元の通知サーバー。

### File Structure

```
src-tauri/src/
├── notifications.rs          # sanitize_title (:145)
├── callbacks.rs              # NotifyRustSink::send (:145)
└── window_host/
    └── link_hover.rs         # 追加の notify 呼び出し（実装位置案 b の対象）
```

## Test Scenarios

### Unit Tests

- [ ] TS1 (FR1, NFR2): タグインジェクションのタイトル `<a href="https://attacker.invalid">Sign in</a>` が、解釈可能なマークアップを含まないエスケープ済みリテラルテキストのみの本文を生成する。
- [ ] TS2 (FR1): 既に `&amp;` を含む入力が `&amp;amp;` にエスケープされ、`&` 先行順序が二重エスケープの曖昧さを防ぐことを確認する。
- [ ] TS4 (FR3): `"body-markup"` が報告されない場合、本文は未エスケープのタイトルを保持する（`&amp;` が可視化されない）。

### Integration Tests

- [ ] TS5 (FR4, NFR2): エージェント通知経路とタブアクティビティ通知経路が使用する共有パイプライン経由でエスケープが実行される。
- [ ] TS7 (NFR1): `--lib` スイート全体が通る（テストは `--lib` にあり、`--bin emterm` では 0 件。`tabs.rs` の replay テストが不安定な場合は `-- --test-threads=1` を用いる）。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

- [ ] 該当なし（E2E 入力は解決されていない）

### Edge Cases

- [ ] TS3 (FR2): 100 文字を超え、境界付近にメタ文字を持つタイトルが、先にトランケートされてからエスケープされ、実体参照が分断されない（エスケープ結果が 100 文字を超えることは正当）。
- [ ] TS6 (NFR1): 既存の `sanitize_title` / `notification_body` のテスト期待値が NFR1 と整合したままである。

### Performance Tests

該当なし（パフォーマンス要件は提示されていない）。

## Security Considerations

- **Input Validation:** OSC 0/2 由来のタイトルは信頼できない入力として扱い、既存のサニタイズ（CSI 除去、C0/DEL/C1 制御文字除去、入力上限、100 文字トランケート）に加えてマークアップメタ文字をエスケープする（FR1 / NFR1）。
- **XSS / Markup Injection Prevention:** `&` -> `&amp;`、`<` -> `&lt;`、`>` -> `&gt;` のエスケープにより、本文マークアップ対応サーバーでのタグ・実体参照の解釈を防ぐ。`&` を最初に置換して二重エスケープを避ける（FR1）。エスケープ対象はこの 3 文字で十分であり、目的が本文でのタグ / 実体参照の解釈防止であって属性コンテキストのエスケープではないため、クォートのエスケープは不要。
- **Choke point:** エスケープは OSC 0/2 由来のタイトルが notify_rust `.body()` に到達するすべての経路上に置き、迂回経路を残さない（NFR2 / FR4）。
- **Fail-safe on capability lookup:** `get_capabilities()` の失敗は「ケイパビリティ未確認」として扱い、エスケープを適用しない（FR3）。
- **Authentication / Authorization / Data Protection / SQL Injection / CSRF:** 該当なし。

## Error Handling

| 条件 | 挙動 |
|------|------|
| `get_capabilities()` が `"body-markup"` を報告しない | エスケープを適用せず本文をそのまま渡す（FR3） |
| `get_capabilities()` が失敗する（確認不可） | 「ケイパビリティ未確認」として扱い、エスケープを適用しない（FR3） |

## Performance Optimization

該当なし（パフォーマンス目標は提示されていない）。

## Success Criteria

- [ ] 通知本文タイトルのマークアップメタ文字（`&` -> `&amp;`、`<` -> `&lt;`、`>` -> `&gt;`）をエスケープするコード経路が存在する。
- [ ] エスケープは 100 文字トランケートの後に実行され、実体参照がシーケンス途中で分断されない。
- [ ] エスケープは notify_rust の `get_capabilities()` が `"body-markup"` を確認した場合にのみ適用され、body-markup を通知しないサーバーで `&amp;` が生表示されない。
- [ ] `<a href="https://attacker.invalid">Sign in</a>` のようなタイトル（タグ・実体参照を含む）が本文でリテラルテキストのみになることを固定するユニットテストが存在する。
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- なし（FR1〜FR5、NFR1〜NFR3 はすべて resolved）。

## Implementation Phases (if applicable)

該当なし（フェーズ分割は提示されていない）。

## References

- REQUIREMENTS.md: ./REQUIREMENTS.md
- レビュー指摘: afa7e833c3f394a8（PR #31 round1、severity medium、category security、confidence 95）
- `sanitize_title`: src-tauri/src/notifications.rs:145
- `NotifyRustSink::send`: src-tauri/src/callbacks.rs:145
- 追加の本文生成元（実装位置案 b の対象）: src-tauri/src/window_host/link_hover.rs
