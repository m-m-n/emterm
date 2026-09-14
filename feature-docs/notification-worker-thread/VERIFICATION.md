# Verification Document: notification-worker-thread

## Overview

**Feature**: notification-worker-thread /
**SPEC.md**: `feature-docs/notification-worker-thread/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/notification-worker-thread/IMPLEMENTATION.md`

統合後のフィーチャー全体を検証する手順。タスク単位の受け入れ基準は
`feature-docs/notification-worker-thread/tasks/task0001.md` にある。

## Build Verification

すべてプロジェクトルートから実行する（`cd src-tauri/` しない）。

- コマンド（rust）:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- コマンド（rust-cli-only）:
  `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
- 期待: いずれも終了コード 0、エラーなし。警告の新規増加なし。

## Test Verification

- コマンド:
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
- 期待: 終了コード 0。既存テストの失敗・無効化・書き換えが 1 件もないこと。
- カバレッジ目標: 本プロジェクトにカバレッジ計測ツールは導入されていないため数値目標は
  置かない。代わりに「Functional Requirements Coverage」の表で、全要件が 1 つ以上の
  テストシナリオに対応していることをもって充足とする。
- 注意: 本プロジェクトのテストは `--lib` 配下にある。`tabs.rs` の replay テストは
  並列実行で非決定的に落ちることがあるため、疑わしい失敗は `--test-threads=1` で
  再確認する。

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS1 | ワーカーが占有されている状態で投入する（ヘッドレス。キュー側の抽象を notify-rust トランスポートから切り離して直接検証する） | 投入がブロックせずに戻る | Unit |
| TS2 | 容量 8 のキューを満たしてもう 1 件投入する（ヘッドレス） | 超過分がドロップされ、ドロップが報告され、プロデューサはブロックしない。キューは無制限に伸びない | Unit |
| TS3 | ドロップ警告の武装（ヘッドレス）: エピソード最初のドロップ → 同一エピソード内の以降のドロップ → 投入成功で再武装 → 次のドロップ | 1 件目のみ報告、以降は無報告、再武装後は再び報告 | Unit |
| TS4 | シャットダウン（ヘッドレス）: 何件か投入したシンクをドロップする | 送信側がドロップされ、ワーカーループが終了し、境界付き join が期限内に戻る | Unit |
| TS5 | 回帰: 既存の `escape_for_send` / fail-closed capability テスト（`src-tauri/src/callbacks/tests.rs:691-860`）と既存のレートリミット / redaction テスト | すべて無変更のまま通る | Integration |
| TS6 | 回帰: 既存のエージェント状態通知テスト（`src-tauri/src/app/tests/agent_status.rs`） | キャプチャ用シンクを変更しないまま通る | Integration |
| TS7 | ビルド: CLI 専用フィーチャーゲート（`cargo check --no-default-features`） | コンパイルが通る。通知関連のコードは `gui` フィーチャー配下に留まっている | Build |
| TS8 | 通知を 1 件も送らずにシンクを構築して即座にドロップする（ヘッドレス。実 `App` を構築するテストが踏む経路 = SPEC A9） | 境界付き join が期限内に戻り、`cargo test` 下でドロップ経路が健全である | Unit |
| TS9 | 静的検査（差分レビュー）: (a) トレイト宣言がバイト単位で不変、(b) `send` 本体に capability 照会と送出呼び出しが無い、(c) ディスパッチ経路に cfg 分岐が無い、(d) プロセスグローバルな一度きりの初期化を新設していない、(e) 既存プロデューサ呼び出し箇所が無変更 | 5 項目すべて充足 | Static |

TS8 / TS9 は create-plan で追加したシナリオ。SPEC の TS1-TS7 に対し、FR7 / FR9 / FR10 と
SPEC AC1 / AC2 / AC3 を検証するシナリオが無かったための補完であり、既存シナリオの
置き換えではない。

## Code Quality Verification

- Format: `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
  - 注意: 本プロジェクトはクレート全体への `cargo fmt` 適用を避ける方針
    （無関係な多数のファイルが書き換わるため）。差分が出た場合は本フィーチャーが
    触ったファイルのみを整形し、それ以外は元に戻す。
- Static analysis: 専用の静的解析コマンドは設定されていない。`cargo check` の
  警告と TS9 の差分検査で代替する。

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| AC1 | `send` が呼び出しスレッドで capability 照会も送出も行わない | TS9 (b) + TS1 |
| AC2 | トレイト宣言がバイト単位で同一、既存テストシンク 3 種が無変更でコンパイルできる | TS9 (a) + TS5 + TS6 |
| AC3 | プロデューサ呼び出し箇所が無変更で、本番側の変更は構築 1 箇所のみ | TS9 (e) + 差分レビュー |
| AC4 | ブロック中のワーカーに対して `send` が速やかに戻り、9 件目以降はドロップ | TS1 + TS2 |
| AC5 | エピソード先頭のドロップのみ警告 1 件、捌けたら再武装 | TS3 |
| AC6 | redaction が受領値から導出され、2 記録のリテラル接頭辞が不変 | TS5 |
| AC7 | capability 照会が通知ごとに 1 回、単一結果がタイトル・本文双方を駆動 | TS5 + TS9 (b) |
| AC8 | ワーカーがブロックしていても最後の共有参照のドロップが期限内に戻る | TS4 + TS8 |
| AC9 | `cargo test` が通り、`cargo check --no-default-features` が通る | Test Verification + TS7 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001 | TS1, TS9 |
| FR2 | task0001 | TS5, TS6, TS9 |
| FR3 | task0001 | TS5 |
| FR4 | task0001 | TS5 |
| FR5 | task0001 | TS1, TS2 |
| FR6 | task0001 | TS2, TS3 |
| FR7 | task0001 | TS4, TS8 |
| FR8 | task0001 | TS4, TS8 |
| FR9 | task0001 | TS7, TS9 |
| FR10 | task0001 | TS4, TS9 |
| NFR1 | task0001 | TS1, TS9 |
| NFR2 | task0001 | TS4 |
| NFR3 | task0001 | TS2 |
| NFR4 | task0001 | TS1, TS2, TS3, TS4 |
| NFR5 | task0001 | TS7 |

## E2E Testing

本プロジェクトに Rust 側の E2E フレームワークは設定されていない
（workflow.yaml の `e2e_test_command` は空）。本フィーチャーはユーザーに見える面を
持たないため、E2E シナリオは追加しない。

## Manual Testing (E2E Not Possible)

実機でのみ確認できる項目。デザインステップはスキップされているため、モックとの
目視照合は対象外。

- [ ] MT-1: リリースビルドを起動し、OSC 9 / タブ活動 / エージェント状態のいずれかで
      通知を発火させ、デスクトップ通知が従来どおり表示されることを確認する。
- [ ] MT-2: 通知発火時にタイプ入力の引っかかりやフレームの詰まりが起きないことを
      確認する（本フィーチャーの目的そのもの）。
- [ ] MT-3: `emterm.log` を読み、失敗時の `notify-rust failed: ` 記録が従来の文言で
      出ることを確認する。ログは
      `~/.local/share/net.laser5.app.emterm/logs/emterm.log`。成功時の
      `notify-rust dispatched: ` は `debug` レベルのためリリースビルドには残らない
      （`.claude/rules/debugging-constraints.md`）。
- [ ] MT-4: 通知デーモンを止めた状態（または通知デーモンが無い環境）で通知を連続
      発火させ、アプリがフリーズせず、`emterm.log` にドロップ警告がエピソードごとに
      1 件だけ出ることを確認する。
- [ ] MT-5: 上記の停滞状態のままアプリを終了し、終了が引っかからないことを確認する
      （境界付き join の期限内に戻ること）。
- [ ] MT-6: Windows ビルドでの通知送出（クロスビルド環境がある場合のみ。
      `make win-build`）。実機が無い場合は未実施として記録する。

## Performance / Security Verification

- **NFR1（イベントループのレイテンシ）**: 呼び出しスレッド上に残る、外部デーモンで
  ブロックしうる呼び出しの数が 0 件であること。TS9 (b) のコード検査で判定する。
  専用の負荷試験は設けない（SPEC「Performance Tests」の方針）。
- **NFR3（メモリ境界）**: 保留通知が容量 8 のキューに収まること。TS2 で判定する。
- **セキュリティ（fail-closed エスケープゲート）**: capability 照会結果を
  キャッシュせず、単一の照会結果がタイトルと本文双方のエスケープ判定を駆動する
  こと。TS5 の既存 fail-closed テストが無変更で通ることをもって判定する。
  照会をワーカーへ移した結果としてエスケープ判定が緩む経路が生まれていないことを、
  TS9 の差分検査でも確認する。

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit (TS1-TS4, TS8) | 5 | 5 | 0 | 0 |
| Integration 回帰 (TS5, TS6) | 2 | 2 | 0 | 0 |
| Build ゲート (TS7) | 1 | 1 | 0 | 0 |
| Static (TS9) | 1 | 0 | 0 | 1 |
| Manual (MT-1..MT-6) | 6 | 0 | 0 | 6 |
| 合計 | 17 | 10 | 0 | 7 |
