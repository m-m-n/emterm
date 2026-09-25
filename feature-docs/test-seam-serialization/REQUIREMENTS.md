---
title: "test-seam-serialization"
created_date: 2026-09-25
status: draft
---

# test-seam-serialization - 要件定義書

## 1. 概要

### 1.1 背景
`#[cfg(test)]` のテストシーム経由でプロセスグローバル状態（`RESTART_REQUIRED`、SFTP の progress / result チャネル）に触れるテストがある。本件は次の 2 点を扱う。

- `RESTART_REQUIRED` に触れるテストの直列化（`self_exec::RestartFlagTestGuard`）。現在のベースで実装済みであり、本件はその状態を保全する。
- SFTP のテストシーム `SftpService::test_push_progress_event` / `test_push_result_event` が送信ヘルパ `send_progress` / `send_result` を通さずにチャネルへ送信している状態の是正。

### 1.2 目的
- 既定の並列 `cargo test --lib` の下で、`#[cfg(test)]` シーム経由でプロセスグローバル状態に触れるテストを決定的に保つ。
- SFTP のテストシームに、「progress / result チャネルへの送信はすべて `send_progress` / `send_result`（wake ヘルパ）を通す」というモジュールの不変条件を守らせる。

### 1.3 スコープ
**対象**:
- FR1〜FR3: `RESTART_REQUIRED` のテスト直列化・panic 安全な復元・`self_exec.rs` の doc の保全
- FR4〜FR6: SFTP テストシームのヘルパ経由化・doc の整合・構造チェックの走査範囲拡大
- FR7: `timing.rs` / `self_exec.rs` 以外でフラグに触れるテストの確認

**対象外**:
- `src-tauri/` 全体に既存する rustfmt ドリフト（本件が触れていない 7 ファイル）（NFR4）
- 既知の無関係な並列フレーク（例: `test/README.md:40` に記載の `tabs.rs` replay テスト）の修正（NFR5）

## 2. ビジネス要件

### 2.1 ビジネス目標
- 既定の並列 `cargo test --lib` の下で、`#[cfg(test)]` シーム経由でプロセスグローバル状態に触れるテストを決定的に保つ。
- SFTP のテストシームに、progress / result チャネルへの送信はすべて `send_progress` / `send_result`（wake ヘルパ）を通すというモジュールの不変条件を守らせる。

### 2.2 対象ユーザー
| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 | `cargo test --lib` を実行し、`src-tauri/` のテストを保守する |

### 2.3 期待される効果
- `RESTART_REQUIRED` 関連テストと SFTP シーム関連テストが、`--test-threads=1` 無しの並列実行で決定的に通る
- SFTP テストシームが bare な `progress_tx` / `result_tx` 送信に戻ると、構造チェックが失敗して検出される

## 3. ユースケース

### 3.1 ユースケース一覧
該当なし（テスト・テストシーム・doc コメントのみの変更で、UI を伴わない）。

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | RESTART_REQUIRED のテスト直列化を保全する（実装済み） | フラグに触れるテストが `RestartFlagTestGuard` を保持する状態を後退させない | confirmed |
| FR2 | フラグ復元の panic 安全性を保全する（実装済み） | ガードの Drop による復元と poison 回復を維持する | confirmed |
| FR3 | self_exec.rs の直列化 doc を保全する（実装済み） | `RESTART_FLAG_TEST_LOCK` の doc がガードによる排他を説明する状態を維持する | confirmed |
| FR4 | SFTP テストシームをヘルパ経由にする | `test_push_progress_event` / `test_push_result_event` を `send_progress` / `send_result` 経由にする | confirmed |
| FR5 | テストシームの doc を実装に合わせる | `#[cfg(test)] impl SftpService` の doc をヘルパ経由の記述に改める | confirmed |
| FR6 | 構造チェックの走査範囲をテストシームまで広げる | `every_progress_and_result_send_site_routes_through_the_wake_helpers` の走査対象にテストシームを含める | confirmed |
| FR7 | スキャン範囲外のフラグ接触テストを確認する | `timing.rs` / `self_exec.rs` 以外でフラグに触れるテストにもガードを保持させる | confirmed |

### 4.2 機能詳細

#### FR1: RESTART_REQUIRED のテスト直列化を保全する（実装済み）

**説明**: `RESTART_REQUIRED` を立てる・下ろす・消費する・観測するテスト（`restart_pending` / `restart_required` を直接呼ぶもの、および `frame_work_pending` / `next_toast_deadline` / `pump_toasts` / `pump_restart_toast` 経由で触れるもの）はすべて、最初に触れてから最後に観測するまでの間、`self_exec::RestartFlagTestGuard` を保持する。

**ビジネスルール**:
- 現在のベースで `self_exec.rs` の 4 本と `app/tests/timing.rs` の 9 本がすでに満たしている。この状態を後退させない。

#### FR2: フラグ復元の panic 安全性を保全する（実装済み）

**説明**: `RestartFlagTestGuard` は Drop で `RESTART_REQUIRED` を `false` に戻す（unwind 中も同じ）。ロックの poison からは `PoisonError::into_inner` で回復する。

**ビジネスルール**:
- 既存テスト `restart_flag_test_guard_clears_the_flag_when_the_span_ends` / `restart_flag_test_guard_stays_usable_after_a_panicking_span` がこの挙動を固定しており、これを維持する。

#### FR3: self_exec.rs の直列化 doc を保全する（実装済み）

**説明**: `self_exec.rs` の `RESTART_FLAG_TEST_LOCK` の doc コメントは、並列実行が既定であることと、ガードによる排他を説明している。「The suite runs single-threaded」の記述は残っていない。この状態を維持する。

#### FR4: SFTP テストシームをヘルパ経由にする

**説明**: `SftpService::test_push_progress_event` は `send_progress(&self.progress_tx, ...)` で、`test_push_result_event` は `send_result(&self.result_tx, ...)` で送信する。

**ビジネスルール**:
- 送る合成イベントの内容は変えない。
  - `session_id`: `"test-pending"`
  - `file_name`: `"test.txt"`
  - `bytes_transferred`: `0`
  - `total_bytes`: `0`
  - `status`: `Uploading`
  - `error_message`: `None`
  - `request_id`: `0`
  - `outcome`: `Ok(vec![])`
- メソッド名・可視性は変えない。

#### FR5: テストシームの doc を実装に合わせる

**説明**: `service.rs` の `#[cfg(test)] impl SftpService` の doc コメントにある「チャネルへ directly 置く」旨を、送信ヘルパ経由で置く記述に改める。

**ビジネスルール**:
- `send_progress` の doc にある「このモジュールの progress 送信はすべてこのヘルパを通す」という不変条件は、変更後そのまま真になるので改めない。

#### FR6: 構造チェックの走査範囲をテストシームまで広げる

**説明**: `every_progress_and_result_send_site_routes_through_the_wake_helpers` の走査対象に `#[cfg(test)] impl SftpService` ブロックを含める。

**ビジネスルール**:
- 除外するのは、禁止形を文字列リテラルで組み立てている `mod tests` 本体だけにする。
- テストシームに bare な `progress_tx` / `result_tx` の送信が戻ると、このテストが失敗すること。

#### FR7: スキャン範囲外のフラグ接触テストを確認する

**説明**: `src-tauri/src` 配下で、`timing.rs` と `self_exec.rs` 以外に `RESTART_REQUIRED` に（直接または FR1 の App メソッド経由で）触れるテストがあれば、同じく `RestartFlagTestGuard` を保持させる。無ければ変更しない。

## 5. 非機能要件

| ID | 要件名 | 内容 |
|----|--------|------|
| NFR1 | 本番挙動は不変 | 本番挙動を変えない。コード変更はすべて `#[cfg(test)]` アイテムと doc コメントに限る。 |
| NFR2 | 新規依存を追加しない | 新しい依存を追加しない（`serial_test` は追加しない）。 |
| NFR3 | `cargo check` 3 構成で警告ゼロ | `cargo check` が 3 構成で警告ゼロで通る: Linux GUI（既定 feature）、`--no-default-features`、x86_64-pc-windows-msvc 向けの `cargo xwin check --lib --tests`。 |
| NFR4 | フォーマットは本機能が触れるファイルに限る | フォーマットは本機能が触れるファイルにだけ適用する。触れていない 7 ファイルの既存ドリフトは対象外のため、クレート全体の rustfmt は行わない。 |
| NFR5 | 既知の無関係な並列フレークは対象外 | 既知の無関係な並列フレーク（例: `test/README.md:40` に記載の `tabs.rs` replay テスト）の修正は対象外とする。 |

### 5.1 パフォーマンス要件
該当なし。

### 5.2 セキュリティ要件
該当なし。

### 5.3 可用性要件
該当なし。

### 5.4 保守性要件
- ドキュメント: FR3（`RESTART_FLAG_TEST_LOCK` の doc の保全）、FR5（テストシームの doc の整合）
- フォーマット: NFR4

### 5.5 互換性要件
- ビルド構成: NFR3

## 6. UI/UX要件

該当なし（UI を伴わない）。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約
- コード変更は `#[cfg(test)]` アイテムと doc コメントに限る（NFR1）
- 新しい依存を追加しない（NFR2）
- クレート全体の rustfmt は行わない（NFR4）

### 9.2 ビジネス上の制約
該当なし。

### 9.3 スケジュール制約
該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/test-seam-serialization/**`
- `test-docs/test-seam-serialization/**`

`feature-docs/test-seam-serialization/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/test-seam-serialization/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/test-seam-serialization/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/test-seam-serialization/` ディレクトリを生成しないが、宣言された `test-docs/test-seam-serialization/**` は依然として正しい。

## 10. 想定される課題とリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC-1: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` を `--test-threads=1` 無しで全体実行する。合格条件は、その並列実行で `RESTART_REQUIRED` 関連テストと SFTP シーム関連テストが決定的に通ること。失敗を除外できるのは、既知の無関係なフレーク（例: `test/README.md:40` に記載の `tabs.rs` replay テスト）と確認できた場合だけで、その失敗は再実行して記録し、実行結果を全体合格として報告しない。新規の失敗と関連テストの失敗は除外しない。
- [ ] AC-2: `RESTART_REQUIRED` のテスト（`self_exec.rs` の 4 本、`timing.rs` のガード付き 9 本）が並列実行で決定的に通る。
- [ ] AC-3: ガード区間内で assert が panic しても、フラグが後続テストに raised のまま残らない。`restart_flag_test_guard_stays_usable_after_a_panicking_span` が固定する。
- [ ] AC-4: `self_exec.rs` に「single-threaded」の主張が無く、その doc が Mutex + RAII ガードによる直列化を説明している。
- [ ] AC-5: `test_push_progress_event` / `test_push_result_event` が `send_progress` / `send_result` 経由で送信し、どちらかのシームを bare send に戻すと、走査範囲を広げた構造チェックが失敗する。
- [ ] AC-6: `cargo check` が次の 3 構成で警告ゼロで通る。
  - `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
  - `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
  - `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --lib --tests --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml`

### 11.2 KPI
該当なし。

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] TS-1（FR4, AC-5）: `test_push_progress_event` / `test_push_result_event` を呼ぶ `timing.rs` のテスト（`frame_work_pending_true_when_progress_channel_nonempty_and_consumes_nothing`、`frame_work_pending_true_when_result_channel_nonempty_and_consumes_nothing`、`next_toast_deadline_some_when_pretoast_progress_event_pending`）が、シームをヘルパ経由にした後も通る。
- [ ] TS-2（FR6, AC-5）: `every_progress_and_result_send_site_routes_through_the_wake_helpers` が走査範囲を広げた状態で通る。シームを一時的に bare な `progress_tx` / `result_tx` 送信に戻すと失敗する（red チェック）。
- [ ] TS-3（FR1, FR2, AC-2, AC-3）: `self_exec.rs` の 4 本と `timing.rs` のガード付き 9 本が並列の `--lib` 実行で通る。`restart_flag_test_guard_stays_usable_after_a_panicking_span` を含む。
- [ ] TS-4（FR3, AC-4）: `self_exec.rs` に「single-threaded」の文言が無く、`RESTART_FLAG_TEST_LOCK` の doc がガードによる直列化を説明している。
- [ ] TS-5（FR7）: `src-tauri/src` から、`timing.rs` / `self_exec.rs` 以外で `RESTART_REQUIRED` に直接、または `frame_work_pending` / `next_toast_deadline` / `pump_toasts` / `pump_restart_toast` 経由で触れるテストを探す。見つかったものはすべて `RestartFlagTestGuard` を保持している。
- [ ] TS-6（AC-1, NFR5）: 並列の `cargo test --lib` を全体実行する。失敗はすべて、関連（除外しない）か、確認済みの既知の無関係なフレーク（再実行して記録し、全体合格として報告しない）に分類する。
- [ ] TS-7（AC-6, NFR3）: 3 構成の `cargo check` が警告ゼロで完了する。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `RESTART_REQUIRED` | `self_exec` が持つ、プロセス全体で共有される再起動要求フラグ |
| `RestartFlagTestGuard` | `self_exec` の test-only ガード。保持中は `RESTART_FLAG_TEST_LOCK` で排他し、Drop でフラグを `false` に戻す |
| テストシーム | `#[cfg(test)]` 限定で公開されるテスト用の入口（例: `SftpService::test_push_progress_event`） |
| 送信ヘルパ（wake ヘルパ） | `sftp/service.rs` の `send_progress` / `send_result` |
| bare send | 送信ヘルパを通さず `progress_tx` / `result_tx` へ直接行う送信 |

## 14. 確認事項

### 14.1 確認済み事項

以下は前提として記録する（いずれも可逆）。

- [x] A-1（AC-1 の合格範囲）: AC-1 の合格範囲は `RESTART_REQUIRED` 関連テストと SFTP シーム関連テストに限る。並列の `--lib` 全体実行は行い、確認済みの既知の無関係なフレーク（例: `tabs.rs` replay テスト）は再実行後に記録した例外としてのみ除外し、結果を全体合格として報告しない。（出典: answer `requirement.parallel-lib-pass-scope`（create-spec-q0001、batch codex consultation、record_as_assumption））
- [x] A-2（wake の副作用）: テストバイナリでは `crate::wakeup::wake()` は no-op である（`wakeup::install` がそこで実行されない）。そのため、シームを `send_progress` / `send_result` 経由にしてもテスト時の副作用は増えない。（出典: `src-tauri/src/sftp/service.rs` のコメント。`wakeup.rs` 自体は調査対象外）
- [x] A-3（FR1〜FR3 の実装状態）: FR1〜FR3 は現在のベースですでに満たされている（`RestartFlagTestGuard`、panic 安全な Drop、更新済みの doc）。本件はこれらを再実装せず保全する。（出典: `src-tauri/src/self_exec.rs`、`src-tauri/src/app/tests/timing.rs`）

### 14.2 未確認・保留事項
なし。

## 15. 参考資料

- `src-tauri/src/self_exec.rs`: `RESTART_REQUIRED`、`RESTART_FLAG_TEST_LOCK`、`RestartFlagTestGuard`
- `src-tauri/src/app/tests/timing.rs`: フラグに触れるガード付きテスト
- `src-tauri/src/sftp/service.rs`: `send_progress` / `send_result`、テストシーム、構造チェック
- `test/README.md:40`: 既知の並列フレークの記載
