---
title: "notifications-sanitize-contract"
created_date: 2026-09-25
status: draft
---

# notifications-sanitize-contract - 要件定義書

## 1. 概要

### 1.1 背景
Notion タスク「notifications モジュールのサニタイズ契約を一本化する」に基づく。

`crate::notifications` には通知本文を組み立てるビルダーが 2 つある（`notification_body`、`agent_notification_body`）。タブタイトルのサニタイズ規約が両者で揃っていない。生のタブタイトルがビルダーに渡らないことは、現状レビューで守っている。

### 1.2 目的
- 2 つの本文ビルダーのタブタイトルのサニタイズ規約を一本化する。「OS 通知の本文には、信頼できないタブタイトル由来の制御バイトや CSI が含まれない」という不変条件を、1 つの規約で維持する。
- 生のタブタイトルが本文ビルダーに渡ることへの防御を、レビューからコンパイル時の仕組みに置き換える。
- doc コメントに規約の実際の適用範囲を書く。「single choke point for both existing call sites and any future one」の記述を取り除き、エージェント名の信頼契約を明記する。

### 1.3 スコープ
**対象**:
- `SanitizedTitle` 型の新設と、`sanitize_title` の戻り値型の変更
- `notification_body` / `agent_notification_body` の引数型の変更
- 呼び出し側（`App::pump_all`、`App::maybe_notify_agent_transition`）の移行
- `notifications.rs` の doc コメントの書き直し
- 既存テストの入力・比較方法の移行

**対象外**:
- `sanitize_title` が生成するテキストの内容（戻り値の型のみ変更する）
- 通知の発火・ゲート・レート制限・送出の挙動
- `AgentTransition::name` の型とサニタイズ契約

## 2. ビジネス要件

### 2.1 ビジネス目標
- `crate::notifications` の 2 つの通知本文ビルダー（`notification_body`、`agent_notification_body`）のタブタイトルのサニタイズ規約を一本化する。これにより「OS 通知の本文には、信頼できないタブタイトル由来の制御バイトや CSI が含まれない」という不変条件を 1 つの規約で維持する。
- 生のタブタイトルが本文ビルダーに渡ることへのレビュー頼みの防御を、コンパイル時の仕組みに置き換える。
- doc コメントに規約の実際の適用範囲を書く。「single choke point for both existing call sites and any future one」の記述を取り除き、エージェント名の信頼契約を明記する。

### 2.2 対象ユーザー
| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 | `crate::notifications` の本文ビルダーを呼び出すコードを書く開発者 |

### 2.3 期待される効果
- 本文ビルダーが単一のサニタイズ規約に従う
- 生タイトルを誤って渡す新規の呼び出し側が、レビューではなく型エラーで検知される
- doc コメントが実際の規約の適用範囲と一致する

## 3. ユースケース

### 3.1 ユースケース一覧
| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 本文ビルダーへの生タイトル渡しをコンパイル時に検知する | 開発者 | 高 |

### 3.2 ユースケース詳細

#### UC01: 本文ビルダーへの生タイトル渡しをコンパイル時に検知する

**アクター**: `crate::notifications` の外で本文ビルダーを呼び出す開発者

**事前条件**:
- `SanitizedTitle` と、`&SanitizedTitle` を受け取る 2 つの本文ビルダーが導入済みである

**基本フロー**:
1. 開発者がタブタイトルを `sanitize_title` に渡し、`SanitizedTitle` を得る
2. 開発者が `&SanitizedTitle` を `notification_body` または `agent_notification_body` に渡す
3. クレートがコンパイルされる

**代替フロー**:
- 開発者が生の `&str` / `String` をビルダーに渡した場合、型エラーでコンパイルが失敗する

**事後条件**:
- ビルダーに渡るタブタイトルは、すべて `sanitize_title` を通過している

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | SanitizedTitle newtype | `sanitize_title` だけが生成できる、サニタイズ済みタイトルの型 | 高 |
| FR2 | Both body builders take &SanitizedTitle | 2 つの本文ビルダーのタブタイトル引数を `&SanitizedTitle` にする | 高 |
| FR3 | Call-site migration | 既存の呼び出し側を新しい型に移行する | 高 |
| FR4 | Agent name keeps its upstream contract | エージェント名は上流のサニタイズ契約を維持する | 高 |
| FR5 | Doc comment rewrite | 本文ビルダーの doc コメントを書き直す | 高 |
| FR6 | Behavior preservation | サニタイズ結果・本文・通知挙動を変えない | 高 |

### 4.2 機能詳細

#### FR1: SanitizedTitle newtype

**説明**: `crate::notifications` に、非公開の内部フィールドを持つ `SanitizedTitle` 型を導入する。モジュールの外から値を得る手段は `sanitize_title` だけとし、その戻り値型を `String` から `SanitizedTitle` に変更する。型はサニタイズ済みテキストへの読み取り専用のアクセス（例: `as_str()`、`Display`）を提供する。任意の `String` / `&str` からの公開コンストラクタや相互変換は提供しない（`From<String>`、`From<&str>`、公開フィールド、`new(raw)` を持たない）。

**入力**:
- `sanitize_title` の引数: 生のタブタイトル

**出力**:
- `sanitize_title` の戻り値: `SanitizedTitle` - サニタイズ済みテキストを保持する

**ビジネスルール**:
- `SanitizedTitle` をモジュールの外で得る手段は `sanitize_title` だけである
- `crate::notifications` の内部のコードは `SanitizedTitle` を直接構築できる。保証はモジュール境界をまたぐ場合に適用される（前提 a4）

#### FR2: Both body builders take &SanitizedTitle

**説明**: `notification_body` は `sanitized_title: &str` の代わりに `&SanitizedTitle` を受け取る。`agent_notification_body` は `tab_title: &str` の代わりに `&SanitizedTitle` を受け取り、内部で `sanitize_title` を呼ばなくなる。notifications モジュールの外から、どちらのビルダーにも生の `&str` / `String` を渡すとコンパイルエラーになる。

**入力**:
- タブタイトル: `&SanitizedTitle`

**エラーケース**:
| エラー | 条件 | 対応 |
|--------|------|------|
| コンパイルエラー（型エラー） | モジュールの外から生の `&str` / `String` をビルダーに渡す | 呼び出し側で `sanitize_title` を通して `SanitizedTitle` を得てから渡す |

#### FR3: Call-site migration

**説明**:
- `App::pump_all` のタブアクティビティ経路は、取得時のサニタイズを維持する。`pending_notifications` を `Vec<(SanitizedTitle, ActivityKind)>` にし、保存した値を `notification_body` に渡す。
- `App::maybe_notify_agent_transition` は `tab_title: &str` 引数を維持し、`agent_notification_body` の前でそれに `sanitize_title` を適用する。適用するのは現状どおり発火経路だけとする。

#### FR4: Agent name keeps its upstream contract

**説明**: `AgentTransition::name` は `Option<String>` のままとし、上流のサニタイズ契約（パース時の `agent_status::sanitize_name`）を維持する。本文ビルダーはこれを再サニタイズしない。`AgentTransition::name` の doc と `agent_notification_body` の doc にこのことを明記する。

#### FR5: Doc comment rewrite

**説明**: `agent_notification_body` の doc（notifications.rs:360-369）と `notification_body` の doc（notifications.rs:161-163）を書き直す。書き直した doc には次を書く。
- 両ビルダーが `SanitizedTitle` を要求する
- `SanitizedTitle` を生成するのは `sanitize_title` だけである
- タブタイトルの不変条件は、すべての呼び出し箇所でその型によって強制される
- エージェント名は上流で信頼済みのものとして扱う

「single choke point for both existing call sites and any future one」の主張は取り除く。

#### FR6: Behavior preservation

**説明**: `sanitize_title` が生成するテキストは、どの入力に対してもバイト単位で変わらない（4096 文字の入力上限、CSI 除去、C0/C1/DEL の除去、100 文字の切り出し）。両ビルダー・両ロケールで生成される本文文字列は変わらない。通知の発火・ゲート・レート制限・送出は変わらない。

## 5. 非機能要件

### 5.1 パフォーマンス要件
- NFR3: 取得時のサニタイズにより、保存されるタブアクティビティのタイトルは 100 文字以下に保たれる。数 MiB の OSC タイトルを clone しない。

### 5.2 セキュリティ要件
- 入力検証: 信頼できないタブタイトルは `sanitize_title` を通してから本文ビルダーに渡す。これを本文ビルダーの引数型（`&SanitizedTitle`）で強制する（FR1、FR2）。

### 5.3 可用性要件
該当なし

### 5.4 保守性要件
- ドキュメント: FR5 のとおり doc コメントを書き直す。
- NFR4: コンパイル時の保証は本文ビルダーのシグネチャによって得る。すべての呼び出し側が `SanitizedTitle` を渡した状態でクレートがコンパイルできることと、ビルダーの出力に対する単体テストで検証する。AC4 の `--lib` コマンドでは doctest が実行されないため、compile_fail doctest は必須としない。

### 5.5 互換性要件
- NFR1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。
- NFR2: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`（デフォルト features）と `... --no-default-features` の両方がコンパイルできる。notifications モジュールは GUI 専用のままとし、常時ビルドされる依存を追加しない。

## 6. UI/UX要件

### 6.1 画面設計要件
該当なし（UI 面を持たない。関数シグネチャ・新しい newtype・doc コメントに閉じた Rust 内部の変更で、通知テキストは変わらない）

### 6.2 画面遷移
該当なし

### 6.3 レスポンシブ対応
該当なし

## 7. データ要件

### 7.1 データモデル概要
該当なし

### 7.2 データ項目
| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| `App` | `pending_notifications` | `Vec<(SanitizedTitle, ActivityKind)>` | ○ | 取得時にサニタイズしたタブタイトルと ActivityKind の組（FR3） |
| `AgentTransition` | `name` | `Option<String>` | × | 型は変えない。上流の `agent_status::sanitize_name` の契約を維持する（FR4） |

### 7.3 データ保持期間
該当なし

## 8. 外部連携

### 8.1 連携システム
該当なし

### 8.2 API仕様要件
該当なし

## 9. 制約条件

### 9.1 技術的制約
- notifications モジュールは GUI 専用のままとし、常時ビルドされる依存を追加しない（NFR2）
- `SanitizedTitle` による保証はモジュール境界をまたぐ場合に適用される。`crate::notifications` の内部では直接構築できる（前提 a4）

### 9.2 ビジネス上の制約
該当なし

### 9.3 スケジュール制約
該当なし

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/notifications-sanitize-contract/**`
- `test-docs/notifications-sanitize-contract/**`

`feature-docs/notifications-sanitize-contract/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/notifications-sanitize-contract/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/notifications-sanitize-contract/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/notifications-sanitize-contract/` ディレクトリを生成しないが、宣言された `test-docs/notifications-sanitize-contract/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題
該当なし

### 10.2 ビジネスリスク
該当なし

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC1: notifications モジュールの本文ビルダーが単一のサニタイズ規約に従う。`notification_body` と `agent_notification_body` はどちらもタブタイトルに `&SanitizedTitle` を受け取り、生の `&str` を受け取らない。（FR1、FR2、FR4）
- [ ] AC2: 生タイトルを誤って渡す新規呼び出し側が、レビューではなく仕組みで検知される。`crate::notifications` の外の呼び出し側は、どちらのビルダーにも生の `&str` / `String` のタイトルを渡せない（型エラー）。`SanitizedTitle` は `sanitize_title` からしか得られない。（FR1、FR2、FR3、NFR4）
- [ ] AC3: `agent_notification_body` の doc コメントの「single choke point」の記述を、実際の適用範囲に合わせて書き直す。doc コメントは、型で強制されるタブタイトルの契約と、上流のエージェント名の契約を説明する。「single choke point for both existing call sites and any future one」の文言はなくなっている。（FR5）
- [ ] AC4: `cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。（NFR1、FR6）

### 11.2 KPI
該当なし

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] 正常系（TS1）: `notification_body` に `sanitize_title("\x1b[31mred\x1b[0m title")` を渡すと、各 `ActivityKind` について `red title: <msg>` を返し、ESC / CSI の残りを含まない（notifications.rs の単体テスト）。（AC1、AC2、AC4）
- [ ] 正常系（TS2）: 既存の `body_formats_match_webview_strings` / `body_formats_match_webview_ja_strings` が、入力を `sanitize_title("tab")` で作った状態で、期待出力を変えずに通る。（AC4）
- [ ] 正常系（TS3）: agent-notification-sanitize-title で追加した CSI・制御文字のテストを含む既存の `agent_notification_body_*` テストが、期待する本文を変えずに通る。生タイトルはビルダー呼び出しの前に、呼び出し側で `sanitize_title` を通す。（AC1、AC4）
- [ ] 境界値（TS4）: 既存の `sanitize_*` テストが、`SanitizedTitle` の読み取りアクセサ経由の比較で、期待テキストを変えずに通る。（AC4）
- [ ] 境界値（TS5）: `sanitize_title` の結果を使う callbacks/tests.rs の `body_markup_escape` / `summary_markup_escape` テストが、`SanitizedTitle` の読み取りアクセサを使ってアサーションを維持する（100 文字の境界、末尾の `&lt;`、`escape_for_send`）。（AC4）
- [ ] セキュリティ（TS6）: 生タイトルを渡していた呼び出し箇所をすべて移行した後、クレートがデフォルト features と `--no-default-features` の両方でコンパイルできる。すべての呼び出し側が `SanitizedTitle` を渡していることを、コンパイル時の証拠とする。（AC2）
- [ ] ドキュメント（TS7）: notifications.rs の doc コメントをレビューし、「single choke point for both existing call sites and any future one」の文言が残っていないことを確認する。`AgentTransition::name` と `agent_notification_body` の doc が、上流の `sanitize_name` の契約を明記している。（AC3）

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `SanitizedTitle` | `crate::notifications` の newtype。非公開の内部フィールドにサニタイズ済みのタブタイトルを保持し、モジュールの外では `sanitize_title` からしか得られない |
| `sanitize_title` | タブタイトルをサニタイズする関数。4096 文字の入力上限、CSI 除去、C0/C1/DEL の除去、100 文字の切り出しを行う。戻り値型を `SanitizedTitle` に変更する |
| 本文ビルダー | OS 通知の本文を組み立てる `notification_body` と `agent_notification_body` |
| `agent_status::sanitize_name` | パース時にエージェント名をサニタイズする上流の関数 |

## 14. 確認事項

### 14.1 確認済み事項
該当なし（本フェーズでユーザーとの対話による確認は行っていない）

### 14.2 未確認・保留事項
以下は requirements-analyst が置いた前提である。

- [ ] a1: `sanitize_title` のテキスト出力はどの入力に対しても変わらず、戻り値の型だけが変わる。理由: タスクの対象外であり、既存の `sanitize_*` テストと callbacks/tests.rs の切り詰め境界テストで固定されている。影響度: 低、可逆: はい
- [ ] a2: `App::maybe_notify_agent_transition` は `tab_title: &str` のシグネチャを維持し、`agent_notification_body` を呼ぶ前に内部でサニタイズする。理由: 呼び出し側が与えられた参照スキャン対象の外にあり、シグネチャを維持すると変更範囲が限られる。ビルダーの境界は引き続き型で強制される。影響度: 低、可逆: はい
- [ ] a3: compile_fail doctest は追加しない。型シグネチャと本文ビルダーの単体テストで検証する。理由: AC4 の `--lib` コマンドでは doctest が実行されない。影響度: 低、可逆: はい
- [ ] a4: `crate::notifications` の内部のコードは `SanitizedTitle` を直接構築できる。保証はモジュール境界をまたぐ場合に適用される。理由: Rust の可視性はモジュール単位である。影響度: 低、可逆: はい

## 15. 参考資料

- `SPEC.md`: `feature-docs/notifications-sanitize-contract/SPEC.md`
