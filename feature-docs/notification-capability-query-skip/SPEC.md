# Feature: notification-capability-query-skip

## Overview

Unix の通知ワーカー（`src-tauri/src/callbacks.rs` の `notify_worker`）が通知のたびに無条件で呼んでいる `notify_rust::get_capabilities()` を、タイトルか本文にメタ文字（`&`、`<`、`>`）が含まれるときだけ呼ぶよう変更する。
送出されるテキストと fail-closed のマークアップエスケープ保証は変えない。
要件の詳細は [REQUIREMENTS.md](REQUIREMENTS.md) を参照する。

## Objectives

- 通知のたびに無条件で `notify_rust::get_capabilities()` を呼ぶのをやめる。現状は 1 通知あたり、ブロッキングの D-Bus 接続 2 本と往復 2 回（capability 照会と `.show()`）がかかっており、接続 1 本と往復 1 回で済んでいない。
- notification-markup-fail-closed で導入した fail-closed のマークアップエスケープ保証を、そのまま維持する。

## User Stories

該当なし。本フィーチャーは通知ワーカー内に閉じたバックエンドのみの Rust 変更であり、UI の追加・変更はない。受け入れ基準は「Success Criteria」に記載する。

## Technical Requirements

### Functional Requirements
- **FR1:** メタ文字を含まないテキストでは capability 照会を行わない。Unix において、`notify_worker` はタイトル（summary）にも本文（body）にも `&`、`<`、`>` のいずれの文字も含まれないとき、`notify_rust::get_capabilities()` を呼ばない。両フィールドを変更せずに送出する。
- **FR2:** メタ文字を含むときは通知ごとに 1 回だけ照会する。タイトルか本文（または両方）に `&`、`<`、`>` が含まれるとき、ワーカーはその通知について `get_capabilities()` をちょうど 1 回呼び、通知をまたいだキャッシュは行わない。その 1 回の結果で、既存の fail-closed 規則に従い両フィールドのエスケープを決める。照会が成功し、かつ一覧に `body-markup` が明示的に含まれないときだけテキストをそのまま通す。照会が失敗したとき、または一覧に `body-markup` が含まれるときは両フィールドをエスケープする。
- **FR3:** 送出テキストはバイト単位で同一とする。入力テキストと capability 照会結果のあらゆる組み合わせについて、`notify_rust::Notification` に渡す summary と body は、現行実装が生成するものとバイト単位で同一とする。
- **FR4:** 照会判定を単体テスト可能にする。照会するかどうかの判定と、それに続くエスケープ判定を、D-Bus 接続なしでテストが実行できる単位に置く。たとえば、capability 照会を注入された遅延評価の呼び出し可能オブジェクトとして受け取る形にできる。テストは、照会が実行されたかどうかと、その回数を観測できる。
- **FR5:** doc コメントを新しい挙動に合わせる。`src-tauri/src/callbacks.rs` 内で capability 照会を通知ごとに毎回行うと記述しているコメントを、条件付き照会の記述に書き換える。対象は `notify_worker` の doc コメント、エスケープ判定の上にある D3 コメント、`NotifyRustSink` の doc コメント、`escape_for_send` の doc コメント（「queries get_capabilities() exactly once per send」）とする。PTY 処理スレッドまたは UI スレッド上で同期的な D-Bus 往復が発生すると主張するコメントを残さない。

### Non-Functional Requirements
- **NFR1 - キャッシュ禁止:** capability のキャッシュを一切行わない。`OnceLock`、TTL、通知をまたいだメモ化のいずれも使わない。notification-worker-thread の D3 の鮮度に関する決定と、fail-closed のセキュリティ方針を維持する。
- **NFR2 - エスケープ出力の不変:** エスケープ出力を変えない。`escape_body_markup`（`&`、`<`、`>` の順）、`body_markup_absence_confirmed` の意味、`sanitize_title`、`NotificationRateLimiter` はすべて現状のままとする。
- **NFR3 - プラットフォーム条件分岐の不変:** capability 照会とエスケープ判定は `#[cfg(unix)]` のままとする。Windows の送出経路と、enqueue / receive / dispatch の経路には新しい cfg 分岐を加えない。CLI のみのビルド（`--no-default-features`）が引き続きコンパイルできる。
- **NFR4 - 秘匿化順序の不変:** 秘匿化したログ表示は、引き続きエスケープ前の、キューから受け取った生の値から導出する。
- **NFR5 - 依存の追加なし:** 新しい依存を追加しない。
- **NFR6 - 既存テストの期待値の不変:** `src-tauri/src/callbacks/tests.rs` にある既存の `escape_for_send`、`body_markup_absence_confirmed`、`escape_body_markup` のテストは期待値を変えない。シグネチャが変わる場合も、変えてよいのは呼び出し側の書き方だけで、アサーションは変えない。

## Implementation Approach

### Architecture

**System Architecture:**

`NotifyRustSink::send` は上限付きキューに積むだけで、専用ワーカースレッド `emterm-notify`（`notify_worker`）が D-Bus の処理を行う。この構成は notification-worker-thread で導入済みであり、本フィーチャーでは変えない。変更対象はワーカー上の capability 照会の条件だけである。

```
┌──────────────────────────────────────────────┐
│ PTY 読み取りスレッド / UI フレーム              │
│   NotifyRustSink::send → 上限付きキューに積む    │
├──────────────────────────────────────────────┤
│ ワーカースレッド emterm-notify (notify_worker) │
│   キューから受け取る                           │
│   → 秘匿化ログ（生の値から導出）                │
│   → [cfg(unix)] 照会判定とエスケープ判定         │
│   → notify_rust::Notification の送出           │
├──────────────────────────────────────────────┤
│ デスクトップ通知サーバー（D-Bus）               │
└──────────────────────────────────────────────┘
```

**Component Diagram:**
```
照会判定とエスケープ判定の単位（FR4）
  入力: summary, body, capability 照会（遅延評価で注入できる形）
  ├─ メタ文字なし → 照会しない → (summary, body) をそのまま返す
  └─ メタ文字あり → 照会を 1 回実行 → fail-closed 規則で両フィールドを決める
```

**判定表:**

| タイトル・本文のメタ文字 | 照会回数 | 照会結果 | 送出する summary / body |
|---|---|---|---|
| どちらにもない | 0 | - | 入力のまま |
| いずれかまたは両方にある | 1 | Err | 両方エスケープ |
| いずれかまたは両方にある | 1 | Ok（`body-markup` を含む） | 両方エスケープ |
| いずれかまたは両方にある | 1 | Ok（`body-markup` を含まない） | 入力のまま |

**notification-worker-thread の D3 との関係:**
notification-worker-thread の D3 は「1 通知につきちょうど 1 回照会する」としていた。本フィーチャーでこれは「メタ文字があるときだけ、1 通知につき高々 1 回照会する」に変わる。「キャッシュしない」は維持する。過去のフィーチャー文書は履歴として残し、編集しない。

### Data Flow

```
キュー → notify_worker → (生の値から秘匿化ログを導出)
       → 照会判定: メタ文字なし → Notification(summary, body) → .show()
                   メタ文字あり → get_capabilities() 1 回 → fail-closed 判定 → Notification(summary', body') → .show()
```

### API Design

該当なし。外部公開 API の追加・変更はない。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- `escape_for_send`: 現行のエスケープ判定。出力は変えない（FR3、NFR2）。
- `escape_body_markup`: `&`、`<`、`>` の順にエスケープする。変更しない（NFR2）。
- `body_markup_absence_confirmed`: 意味を変えない（NFR2）。
- `sanitize_title`、`NotificationRateLimiter`: 変更しない（NFR2）。

**External Dependencies:**
- notify-rust: 既存の依存。`get_capabilities()` と `Notification` を使う。新しい依存は追加しない（NFR5）。

### File Structure

```
src-tauri/src/
├── callbacks.rs         # notify_worker、NotifyRustSink、escape_for_send、doc コメント（FR1〜FR5）
└── callbacks/
    └── tests.rs         # 既存のエスケープ関連テストと、照会判定の単体テスト
```

## Declared Change Set

このセクションは手作業の列挙ではなく create-plan での導出を記述する。上記のフィーチャー固有のパスは、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、上記のフィーチャー固有のパスに加えて、ワークフローが生成する次の 2 つのエントリをデフォルトで宣言する。

- `feature-docs/notification-capability-query-skip/**`
- `test-docs/notification-capability-query-skip/**`

`feature-docs/notification-capability-query-skip/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。これらは各フェーズドキュメントと `references/phase-state.md` が生成・所有する。このセクションは引用のみで、ルールは再掲しない。

`test-docs/notification-capability-query-skip/**` に含まれるもの: タスクごとのテスト記録 `test-docs/notification-capability-query-skip/{T}.tests.yaml`。これは `implement-phase.md` が生成・所有する。このセクションは引用のみで、ルールは再掲しない。

この 2 つのデフォルトエントリは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載がないことを除外とはみなさない。除外は意図的で明示的な絞り込みである。

この宣言はスーパーセットの主張である。検証時に観測される実際の変更集合は、宣言された集合と等しい必要はなく、宣言された集合に含まれる（CONTAINED IN）必要がある。implement タスクを 1 つも生成しないフィーチャーは `test-docs/notification-capability-query-skip/` ディレクトリをまったく生成しないが、その場合も宣言された `test-docs/notification-capability-query-skip/**` は正しい。宣言されたパスが実際に生成されなくても違反ではない。

## Test Scenarios

### Unit Tests
- [ ] TS1: `&`、`<`、`>` を含まない ASCII および非 ASCII のタイトル・本文（空のタイトルまたは本文、フォールバックタイトル `emterm` を含む） - 注入した照会スタブの呼び出し記録が 0 回、出力が入力と等しい（AC1）
- [ ] TS2: タイトルだけにメタ文字がある場合（例: OSC 9 フォールバック由来の `<tab title>`）を、3 通りの capability 結果（Err、`body-markup` を含む Ok、`body-markup` を含まない Ok）それぞれで確認する - スタブの呼び出し記録がちょうど 1 回、出力が現行の `escape_for_send` の結果と一致する（AC2）
- [ ] TS3: 本文だけにメタ文字がある場合（単独の `&` と、既存の `&amp;` を含む）を、同じ 3 通りの結果で確認する - 呼び出しはちょうど 1 回、出力は同様に一致する（AC3）
- [ ] TS4: 両フィールドにメタ文字がある場合 - 呼び出しはちょうど 1 回で、その 1 回の結果で両フィールドが決まる（AC2、AC3）
- [ ] TS5: 引用符（`"`、`'`）やその他メタ文字以外の記号 - 照会が発生しない（AC1）

### Integration Tests
- [ ] TS6: `--lib` のテストスイート全体と `--no-default-features` のチェック - 既存のエスケープ関連テストが期待値を変えずに通り、両コマンドが成功する（AC4、AC5、AC6）

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] 空のタイトルまたは本文: メタ文字を含まないものとして扱い、照会しない（TS1）
- [ ] フォールバックタイトル `emterm`: 照会しない（TS1）
- [ ] OSC 9 フォールバック由来の `<tab title>`: タイトルのメタ文字として照会を 1 回行う（TS2）
- [ ] 単独の `&` と既存の `&amp;`: 本文のメタ文字として照会を 1 回行い、出力は現行の `escape_for_send` と一致する（TS3）
- [ ] 引用符（`"`、`'`）: 照会しない（TS5）

### Performance Tests
該当なし。

## Security Considerations

- **Authentication:** 該当なし。
- **Authorization:** 該当なし。
- **Input Validation:** `sanitize_title` は変更しない（NFR2）。
- **Data Protection:** 秘匿化したログ表示は、エスケープ前の生の値から導出する（NFR4）。
- **XSS Prevention:** 該当なし。
- **Markup Injection Prevention:** fail-closed のエスケープ規則を維持する。照会が成功し、かつ一覧に `body-markup` が明示的に含まれないときだけテキストをそのまま通す（FR2、NFR2）。capability のキャッシュは行わない（NFR1）。
- **SQL Injection Prevention:** 該当なし。
- **CSRF Protection:** 該当なし。

## Error Handling

### Error Codes

該当なし。新しいエラーコードは定義しない。

### Error Flow

```
get_capabilities() が失敗 → fail-closed 規則に従い両フィールドをエスケープ → 送出
```

## Performance Optimization

### Performance Goals
- タイトルにも本文にもメタ文字を含まない通知では、capability 照会の D-Bus 接続と往復を発生させない（FR1）。

### Optimization Strategies
- メタ文字による短絡: タイトルと本文に `&`、`<`、`>` がないときは照会を省く（FR1）。

### Caching Strategy
- capability はキャッシュしない。`OnceLock`、TTL、通知をまたいだメモ化のいずれも使わない（NFR1）。

## Success Criteria

- [ ] AC1: タイトルにも本文にも `&`、`<`、`>` が含まれないとき、capability 照会の実行回数が 0 回で、送出される組は入力の組と等しい。（FR1、FR3）
- [ ] AC2: タイトルだけにメタ文字が含まれるとき、capability 照会がちょうど 1 回実行される。照会の失敗時、または一覧に `body-markup` があるときは、両フィールドがエスケープされる。照会が成功し一覧に `body-markup` がないときは、両フィールドとも変更されない。（FR2、FR3）
- [ ] AC3: 本文だけにメタ文字が含まれるとき、フィールドを入れ替えた AC2 と同じ結果になる。（FR2、FR3）
- [ ] AC4: 既存のエスケープ関連テストが、期待値を変えずにすべて通る。（NFR2、NFR6）
- [ ] AC5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。
- [ ] AC6: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。（NFR3）
- [ ] AC7: `src-tauri/src/callbacks.rs` の doc コメントに、capability 照会を無条件に行う、または通知ごとにちょうど 1 回行うと記述している箇所が残っていない。（FR5）
- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## Assumptions

requirements-analyst が採用した前提（いずれも可逆）。

- **A1:** タスクの選択肢のうち (a) メタ文字による短絡を採用する。(c) ホットパスからの送出の切り離しは、すでにコードベースにある。`NotifyRustSink::send` は上限付きキューに積むだけで、専用ワーカースレッド `emterm-notify` が D-Bus の処理を行う（callbacks.rs 202-429、フィーチャー notification-worker-thread 由来）。そのため PTY 読み取りスレッドと UI フレームは D-Bus でブロックしない。ワーカー上では照会が今も無条件に実行されており（callbacks.rs:323）、これが残る欠陥である。(b) キャッシュは、notification-worker-thread の D3（通知ごとに新しく照会し、キャッシュしない）を覆し fail-closed の方針を弱めるため採用しない。`escape_body_markup` は `&`、`<`、`>` を含まないテキストに対して恒等関数なので、(a) は出力を変えない。
- **A2:** 短絡の判定はタイトルと本文の両方を見る。現行の `escape_for_send` が両フィールドをエスケープしているため（callbacks.rs:444-454）。タスク記述が本文だけに触れているのは、タイトルのエスケープ導入より前に書かれたためである。
- **A3:** タスクの代替基準（callbacks.rs:279-281 の `pending_notifications` の doc コメントを更新する）は適用しない。そのコメントは callbacks.rs にもう存在しない。`pending_notifications` は現在 app/mod.rs（1016、1288、1391）のローカル変数で、そのコメントは D-Bus について何も主張していない。タスク記述の行番号（callbacks.rs:157、:279-281、:458、app/mod.rs:827、:1328）は現在のツリーでは古い。コメントの正確さに関する意図は FR5 が引き継ぐ。
- **A4:** 過去のフィーチャー文書（`feature-docs/notification-worker-thread/IMPLEMENTATION.md` の D3「exactly once per notification」、`feature-docs/notification-markup-fail-closed/*`）は履歴として残し、編集しない。D3 の変更は本 SPEC の「notification-worker-thread の D3 との関係」に記録する。
- **A5:** Windows の挙動は固定する。Windows の notify-rust には `get_capabilities()` がないため、Windows では照会もエスケープも行わない。これは変更前も変更後も同じである。

## References

- 要件定義書: [REQUIREMENTS.md](REQUIREMENTS.md)
- 実装対象: `src-tauri/src/callbacks.rs`
- 既存テスト: `src-tauri/src/callbacks/tests.rs`
- notification-worker-thread の D3: `feature-docs/notification-worker-thread/IMPLEMENTATION.md`
- fail-closed のマークアップエスケープ: `feature-docs/notification-markup-fail-closed/`
