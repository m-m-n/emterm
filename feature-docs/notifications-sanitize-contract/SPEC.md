# Feature: notifications-sanitize-contract

## Overview

`crate::notifications` の 2 つの通知本文ビルダー（`notification_body`、`agent_notification_body`）のタブタイトルのサニタイズ規約を、`SanitizedTitle` 型で一本化する。生のタブタイトルがビルダーに渡ることを、レビューではなくコンパイル時に防ぐ。要件の詳細は `REQUIREMENTS.md` を参照する。

## Objectives

- 2 つの本文ビルダーのタブタイトルのサニタイズ規約を一本化し、「OS 通知の本文には、信頼できないタブタイトル由来の制御バイトや CSI が含まれない」という不変条件を 1 つの規約で維持する。
- 生のタブタイトルが本文ビルダーに渡ることへのレビュー頼みの防御を、コンパイル時の仕組みに置き換える。
- doc コメントに規約の実際の適用範囲を書く。「single choke point for both existing call sites and any future one」の記述を取り除き、エージェント名の信頼契約を明記する。

## User Stories

### US1: 本文ビルダーが単一のサニタイズ規約に従う
開発者として、2 つの本文ビルダーがタブタイトルを同じ型（`&SanitizedTitle`）で受け取るようにしたい。それにより、タブタイトルの不変条件が 1 つの規約で維持される。

**Acceptance Criteria:**
- [ ] AC1: `notification_body` と `agent_notification_body` はどちらもタブタイトルに `&SanitizedTitle` を受け取り、生の `&str` を受け取らない。（FR1、FR2、FR4）
- [ ] AC4: `cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。（NFR1、FR6）

### US2: 生タイトル渡しを仕組みで検知する
開発者として、生タイトルを誤って本文ビルダーに渡す新規の呼び出し側を、レビューではなく型エラーで検知したい。

**Acceptance Criteria:**
- [ ] AC2: `crate::notifications` の外の呼び出し側は、どちらのビルダーにも生の `&str` / `String` のタイトルを渡せない（型エラー）。`SanitizedTitle` は `sanitize_title` からしか得られない。（FR1、FR2、FR3、NFR4）

### US3: doc コメントを実際の適用範囲に合わせる
開発者として、doc コメントから規約の実際の適用範囲を読み取りたい。

**Acceptance Criteria:**
- [ ] AC3: doc コメントは、型で強制されるタブタイトルの契約と、上流のエージェント名の契約を説明する。「single choke point for both existing call sites and any future one」の文言はなくなっている。（FR5）

## Technical Requirements

### Functional Requirements
- **FR1:** SanitizedTitle newtype — `crate::notifications` に、非公開の内部フィールドを持つ `SanitizedTitle` 型を導入する。モジュールの外から値を得る手段は `sanitize_title` だけとし、その戻り値型を `String` から `SanitizedTitle` に変更する。型はサニタイズ済みテキストへの読み取り専用のアクセス（例: `as_str()`、`Display`）を提供する。任意の `String` / `&str` からの公開コンストラクタや相互変換は提供しない（`From<String>`、`From<&str>`、公開フィールド、`new(raw)` を持たない）。
- **FR2:** Both body builders take &SanitizedTitle — `notification_body` は `sanitized_title: &str` の代わりに `&SanitizedTitle` を受け取る。`agent_notification_body` は `tab_title: &str` の代わりに `&SanitizedTitle` を受け取り、内部で `sanitize_title` を呼ばなくなる。notifications モジュールの外から、どちらのビルダーにも生の `&str` / `String` を渡すとコンパイルエラーになる。
- **FR3:** Call-site migration — `App::pump_all` のタブアクティビティ経路は、取得時のサニタイズを維持する。`pending_notifications` を `Vec<(SanitizedTitle, ActivityKind)>` にし、保存した値を `notification_body` に渡す。`App::maybe_notify_agent_transition` は `tab_title: &str` 引数を維持し、`agent_notification_body` の前でそれに `sanitize_title` を適用する。適用するのは現状どおり発火経路だけとする。
- **FR4:** Agent name keeps its upstream contract — `AgentTransition::name` は `Option<String>` のままとし、上流のサニタイズ契約（パース時の `agent_status::sanitize_name`）を維持する。本文ビルダーはこれを再サニタイズしない。`AgentTransition::name` の doc と `agent_notification_body` の doc にこのことを明記する。
- **FR5:** Doc comment rewrite — `agent_notification_body` の doc（notifications.rs:360-369）と `notification_body` の doc（notifications.rs:161-163）を書き直す。両ビルダーが `SanitizedTitle` を要求すること、`SanitizedTitle` を生成するのは `sanitize_title` だけであること、タブタイトルの不変条件はすべての呼び出し箇所でその型によって強制されることを書く。「single choke point for both existing call sites and any future one」の主張は取り除く。エージェント名は上流で信頼済みのものとして説明する。
- **FR6:** Behavior preservation — `sanitize_title` が生成するテキストは、どの入力に対してもバイト単位で変わらない（4096 文字の入力上限、CSI 除去、C0/C1/DEL の除去、100 文字の切り出し）。両ビルダー・両ロケールで生成される本文文字列は変わらない。通知の発火・ゲート・レート制限・送出は変わらない。

### Non-Functional Requirements
- **NFR1 - lib テスト:** `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。
- **NFR2 - 両 feature 構成のビルド:** `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`（デフォルト features）と `... --no-default-features` の両方がコンパイルできる。notifications モジュールは GUI 専用のままとし、常時ビルドされる依存を追加しない。
- **NFR3 - 保存タイトルの長さ上限:** 取得時のサニタイズにより、保存されるタブアクティビティのタイトルは 100 文字以下に保たれる。数 MiB の OSC タイトルを clone しない。
- **NFR4 - コンパイル時保証の検証方法:** コンパイル時の保証は本文ビルダーのシグネチャによって得る。すべての呼び出し側が `SanitizedTitle` を渡した状態でクレートがコンパイルできることと、ビルダーの出力に対する単体テストで検証する。AC4 の `--lib` コマンドでは doctest が実行されないため、compile_fail doctest は必須としない。

## Implementation Approach

### Architecture

**System Architecture:**
```
crate::notifications
  sanitize_title(raw) -> SanitizedTitle   （モジュール外で SanitizedTitle を得る唯一の手段）
  notification_body(&SanitizedTitle, ...)
  agent_notification_body(&SanitizedTitle, ...)   （AgentTransition::name は再サニタイズしない）
```

**Component Diagram:**
```
App::pump_all
  -> sanitize_title（取得時）-> pending_notifications: Vec<(SanitizedTitle, ActivityKind)>
  -> notification_body(&SanitizedTitle, ...)

App::maybe_notify_agent_transition(tab_title: &str, ...)
  -> sanitize_title（発火経路のみ）-> SanitizedTitle
  -> agent_notification_body(&SanitizedTitle, ...)
```

### Data Flow

```
生のタブタイトル → sanitize_title → SanitizedTitle → notification_body / agent_notification_body → OS 通知の本文
AgentTransition::name（パース時に agent_status::sanitize_name 済み）→ agent_notification_body（再サニタイズしない）
```

### API Design

外部 API は該当なし。変更するクレート内の関数シグネチャは次のとおり。

| 項目 | 変更前 | 変更後 |
|------|--------|--------|
| `sanitize_title` の戻り値 | `String` | `SanitizedTitle` |
| `notification_body` のタブタイトル引数 | `sanitized_title: &str` | `&SanitizedTitle` |
| `agent_notification_body` のタブタイトル引数 | `tab_title: &str`（内部で `sanitize_title` を呼ぶ） | `&SanitizedTitle`（内部で `sanitize_title` を呼ばない） |
| `App::maybe_notify_agent_transition` | `tab_title: &str` | 変更なし（内部で `sanitize_title` を適用する） |
| `AgentTransition::name` | `Option<String>` | 変更なし |

`SanitizedTitle` の公開面:
- 読み取り専用のアクセス（例: `as_str()`、`Display`）を提供する
- `From<String>`、`From<&str>`、公開フィールド、`new(raw)` を持たない

### Database Schema

該当なし

### Dependencies

**Internal Dependencies:**
- `agent_status::sanitize_name`: `AgentTransition::name` の上流のサニタイズ契約（FR4）
- `App::pump_all` / `App::maybe_notify_agent_transition`: 本文ビルダーの呼び出し側（FR3）

**External Dependencies:**
- 該当なし（常時ビルドされる依存を追加しない。NFR2）

### File Structure

```
notifications.rs      # SanitizedTitle、sanitize_title、notification_body、agent_notification_body、doc コメント、単体テスト
callbacks/tests.rs    # body_markup_escape / summary_markup_escape テストの読み取りアクセサ経由への移行
App（pump_all / maybe_notify_agent_transition / pending_notifications）  # 呼び出し側の移行
```

## Declared Change Set

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、フィーチャー固有のパスに加えて、次の 2 つのワークフロー生成エントリをデフォルトで宣言する。

- `feature-docs/notifications-sanitize-contract/**`
- `test-docs/notifications-sanitize-contract/**`

`feature-docs/notifications-sanitize-contract/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` であり、本節は引用のみでルールは再掲しない。

`test-docs/notifications-sanitize-contract/**` に含まれるもの: タスクごとのテスト記録 `test-docs/notifications-sanitize-contract/{T}.tests.yaml`。生成主体は `implement-phase.md` であり、本節は引用のみでルールは再掲しない。

この 2 つのデフォルトエントリは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載がないことを除外とみなさない。除外は意図的な絞り込みである。

この宣言はスーパーセット（SUPERSET）の主張であり、検証時に観測される実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。等しい必要はない。implement タスクを 1 つも生成しないフィーチャーは `test-docs/notifications-sanitize-contract/` ディレクトリを生成しないが、その場合も宣言された `test-docs/notifications-sanitize-contract/**` は正しい。宣言されたパスが実際に生成されなくても違反にはならない。

## Test Scenarios

### Unit Tests
- [ ] TS1: `notification_body` に `sanitize_title("\x1b[31mred\x1b[0m title")` を渡す - 各 `ActivityKind` について `red title: <msg>` を返し、ESC / CSI の残りを含まない（notifications.rs の単体テスト）。（AC1、AC2、AC4）
- [ ] TS2: 既存の `body_formats_match_webview_strings` / `body_formats_match_webview_ja_strings` の入力を `sanitize_title("tab")` で作る - 期待出力を変えずに通る。（AC4）
- [ ] TS3: agent-notification-sanitize-title で追加した CSI・制御文字のテストを含む既存の `agent_notification_body_*` テスト - 生タイトルをビルダー呼び出しの前に呼び出し側で `sanitize_title` に通し、期待する本文を変えずに通る。（AC1、AC4）
- [ ] TS4: 既存の `sanitize_*` テスト - `SanitizedTitle` の読み取りアクセサ経由の比較で、期待テキストを変えずに通る。（AC4）
- [ ] TS5: `sanitize_title` の結果を使う callbacks/tests.rs の `body_markup_escape` / `summary_markup_escape` テスト - `SanitizedTitle` の読み取りアクセサを使い、アサーション（100 文字の境界、末尾の `&lt;`、`escape_for_send`）を維持する。（AC4）

### Integration Tests
- [ ] TS6: 生タイトルを渡していた呼び出し箇所をすべて移行した後のビルド - クレートがデフォルト features と `--no-default-features` の両方でコンパイルできる。すべての呼び出し側が `SanitizedTitle` を渡していることを、コンパイル時の証拠とする。（AC2）
- [ ] TS7: notifications.rs の doc コメントのレビュー - 「single choke point for both existing call sites and any future one」の文言が残っていない。`AgentTransition::name` と `agent_notification_body` の doc が、上流の `sanitize_name` の契約を明記している。（AC3）

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] CSI を含むタブタイトル: TS1、TS3 で扱う
- [ ] 制御文字を含むタブタイトル: TS3 で扱う
- [ ] 100 文字の切り出し境界: TS5 で扱う

### Performance Tests
- [ ] NFR3: 取得時のサニタイズにより、保存されるタブアクティビティのタイトルが 100 文字以下に保たれる

## Security Considerations

- **Authentication:** 該当なし
- **Authorization:** 該当なし
- **Input Validation:** 信頼できないタブタイトルは `sanitize_title` を通してから本文ビルダーに渡す。これを本文ビルダーの引数型 `&SanitizedTitle` で強制する（FR1、FR2）。`AgentTransition::name` は上流の `agent_status::sanitize_name` の契約に従い、本文ビルダーでは再サニタイズしない（FR4）。
- **Data Protection:** 該当なし
- **XSS Prevention:** 該当なし
- **SQL Injection Prevention:** 該当なし
- **CSRF Protection:** 該当なし

## Error Handling

### Error Codes

該当なし（実行時のエラーは追加しない。生タイトルをビルダーに渡すとコンパイル時の型エラーになる。FR2）

### Error Flow

```
モジュール外から生の &str / String を本文ビルダーに渡す → 型エラー（コンパイル失敗）
```

## Performance Optimization

### Performance Goals
- NFR3: 保存されるタブアクティビティのタイトルは 100 文字以下。数 MiB の OSC タイトルを clone しない。

### Optimization Strategies
- 取得時のサニタイズ: `App::pump_all` のタブアクティビティ経路は、取得時にサニタイズした `SanitizedTitle` を `pending_notifications` に保存する（FR3）。

### Caching Strategy
- 該当なし

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals
- [ ] Security requirements are satisfied
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] AC1〜AC4 を満たす

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（`status: tbd` の要件はない）

### Assumptions

- a1: `sanitize_title` のテキスト出力はどの入力に対しても変わらず、戻り値の型だけが変わる。
- a2: `App::maybe_notify_agent_transition` は `tab_title: &str` のシグネチャを維持し、`agent_notification_body` を呼ぶ前に内部でサニタイズする。
- a3: compile_fail doctest は追加しない。型シグネチャと本文ビルダーの単体テストで検証する。
- a4: `crate::notifications` の内部のコードは `SanitizedTitle` を直接構築できる。保証はモジュール境界をまたぐ場合に適用される。

## Implementation Phases (if applicable)

該当なし

## References

- 要件定義書: `feature-docs/notifications-sanitize-contract/REQUIREMENTS.md`
