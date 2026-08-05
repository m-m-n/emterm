---
title: "test-tmp-file-cleanup"
created_date: 2026-08-04
status: draft
---

# test-tmp-file-cleanup - 要件定義書

## 1. 概要

### 1.1 背景

`/tmp` ディレクトリの消費量が 100% になり、Claude Code のツールを実行できない事象が発生した。テスト実行時などに `/tmp`（`std::env::temp_dir()`）へ書き出した一時ファイル・ディレクトリが、テストの正常終了後も削除されずに残留していることが確認されている。

### 1.2 目的

テスト実行が正常終了したときに `/tmp` へ書き出した一時ファイル・ディレクトリを残さないようにし、`/tmp` の容量枯渇（消費 100% で Claude Code ツールが実行不能になった事象）の再発要因を除去する。

### 1.3 スコープ

**対象**:

- テストコードが `/tmp` に残す一時ファイル・ディレクトリの後始末（FR1〜FR5）

**対象外**:

- プロダクションコードの `/tmp` 書き込み挙動（ビューアペイロードの親書き込み→子が読取後削除、`/tmp/emterm-mux-daemon.log` フォールバックログ）— 変更しない（NFR3、A-1）
- プログラムが途中で落ちた（プロセス異常終了・kill）場合の後始末（NFR1、A-2）

## 2. ビジネス要件

### 2.1 ビジネス目標

テスト実行が正常終了したときに `/tmp`（`std::env::temp_dir()`）へ書き出した一時ファイル・ディレクトリを残さないようにし、`/tmp` の容量枯渇（消費 100% で Claude Code ツールが実行不能になった事象）の再発要因を除去する。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 / CI 実行者 | 本リポジトリのテストスイートを実行し、その実行環境の `/tmp` を共有する主体 |

### 2.3 期待される効果

- テストスイートの繰り返し実行で `/tmp` の消費量が増え続けない
- `/tmp` 消費 100% による Claude Code ツール実行不能の再発要因が除去される

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | テストスイートを正常終了まで実行する | 開発者 / CI 実行者 | 高 |

### 3.2 ユースケース詳細

#### UC01: テストスイートを正常終了まで実行する

**アクター**: 開発者 / CI 実行者

**事前条件**:

- 文書化されたテストコマンド（`src-tauri --lib`、`--test cli_subcommands`、`crates` 各 `--lib`、`bun test`）が実行可能である

**基本フロー**:

1. 文書化されたテストコマンドを実行する
2. テストが全件成功で完走する
3. 実行に起因する `/tmp` 直下の新規ファイル・ディレクトリが残らない

**代替フロー**:

- テストが途中で落ちた（プロセス異常終了・kill、テスト失敗による panic）場合、後始末漏れは許容する（NFR1、A-2）

**事後条件**:

- 実行前後で `/tmp` 直下のエントリ一覧を比較して、実行起因の新規残留がない

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | settings_store.rs テストの一時ディレクトリ削除 | `tmp_path()` が作る一時ディレクトリを各テストの正常終了時に削除する | 高 |
| FR2 | settings_window/commands.rs の roundtrip テストの一時ディレクトリ削除 | `app_settings_full_roundtrip_through_patch_save` の一時ディレクトリを正常終了時に削除する | 高 |
| FR3 | mux/tmux_import.rs テストの一時ディレクトリ削除 | `tmp_settings_path()` を使う 5 テストの一時ディレクトリを正常終了時に削除する | 高 |
| FR4 | viewer/launch.rs の spawn エラーテストのペイロード削除 | `launch_with_propagates_spawn_error` が残すペイロードファイルを正常終了時に削除する | 高 |
| FR5 | スイート全体の正常終了時の残留ゼロ | 文書化されたテストコマンドの全件成功完走で `/tmp` 直下に新規残留を出さない | 高 |

### 4.2 機能詳細

#### FR1: settings_store.rs テストの一時ディレクトリ削除

**説明**: `src-tauri/src/settings_store.rs` のテストヘルパー `tmp_path()` が作成する `/tmp/emterm-settings-store-test-{pid}-{name}/` ディレクトリ（`settings.json`、corrupt テストの `.bak` を含む）を、各テストの正常終了時に削除する。

**現状**: 事前削除（`remove_file`）のみで事後削除が一切なく、正常終了ごとに複数ディレクトリが残留する（`settings_store.rs:101-108`、削除は 132/197/272 の事前クリーンのみ）。

**ビジネスルール**:

- 削除対象は当該テストが作成した一時ディレクトリ配下に限る

#### FR2: settings_window/commands.rs の roundtrip テストの一時ディレクトリ削除

**説明**: `src-tauri/src/settings_window/commands.rs` の `app_settings_full_roundtrip_through_patch_save`（行 473-503）が作成する `/tmp/emterm-settings-window-test-{pid}/` を正常終了時に削除する。

**現状**: 同ファイルの他テスト（行 565/591/626/662 等）は末尾で `remove_dir_all` しており、このテストのみ削除がない。

#### FR3: mux/tmux_import.rs テストの一時ディレクトリ削除

**説明**: `src-tauri/src/mux/tmux_import.rs` のテストヘルパー `tmp_settings_path()`（行 174-182、`remove_dir_all` は作成前の事前クリーンのみ）を使う 5 テスト（missing / latch / apply / preserve / keybinds_nonobject）が作成する `/tmp/emterm-tmux-import-test-{pid}-{name}/` を正常終了時に削除する。

**現状**: pid が実行ごとに変わるため事前クリーンでは回収されず、実行ごとに 5 ディレクトリが残留する。

**ビジネスルール**:

- `auto_import_tmux_conf_skips_oversized_file`（行 324）は既に削除済みで変更不要

#### FR4: viewer/launch.rs の spawn エラーテストのペイロード削除

**説明**: `src-tauri/src/viewer/launch.rs` の `launch_with_propagates_spawn_error`（行 280-291）で、`launch_with` が spawn 失敗時にペイロードファイルを削除しない（`launch.rs:169-178`）ため `/tmp/emterm-viewer-{pid}-{nanos}-{n}.json` が残留する。テスト正常終了時にこのファイルを削除する。

**ビジネスルール**:

- 同ファイルの `launch_with_writes_payload_and_invokes_spawn_once` は行 277 で削除済み

#### FR5: スイート全体の正常終了時の残留ゼロ

**説明**: 文書化されたテストコマンド（`src-tauri --lib`、`--test cli_subcommands`、`crates` 各 `--lib`、`bun test`）が全件成功で完走したとき、その実行に起因する `/tmp` 直下の新規ファイル・ディレクトリが残らない。

**ビジネスルール**:

- 既に片付けている以下の箇所の挙動は維持する
  - `render/font/user_dir.rs`
  - `viewer/html_window.rs`・`html_resolver.rs`・`image_resolver.rs`・`image_window.rs`・`html.rs`・`data_payload.rs`・`image_payload.rs`
  - `settings.rs`
  - `git_branch.rs`
  - `tempfile` クレート利用箇所
  - `tests/mux_hot_upgrade.rs`・`mux_throughput.rs` の `DaemonGuard`
  - `notify-status.test.ts` の `finally` 内 `rmSync`

## 5. 非機能要件

### 5.1 パフォーマンス要件

要件なし。

### 5.2 セキュリティ要件

要件なし。

### 5.3 可用性要件

- NFR1: プログラムが途中で落ちた（プロセス異常終了・kill）場合の後始末漏れは許容する。テスト失敗（panic）経路の残留も要求対象外とする（ただし RAII 化により自然に片付く実装は妨げない）。

### 5.4 保守性要件

- NFR3: プロダクションコードの `/tmp` 書き込み挙動（ビューアペイロードの親書き込み→子が読取後削除、`/tmp/emterm-mux-daemon.log` フォールバックログ）は変更しない。

### 5.5 互換性要件

- NFR2: 新規依存を追加しない。RAII による削除が必要な場合は `src-tauri` の既存 dev-dependency である `tempfile = "3"`（`src-tauri/Cargo.toml:207`）を使ってよい。

## 6. UI/UX要件

対象外。テストの一時ファイル後始末のバグ修正であり、UI/UX・画面表示への影響が一切ないため設計ステップは不要と判断されている。

## 7. データ要件

### 7.1 データ項目

| 対象 | パス | 説明 |
|------|------|------|
| settings_store テスト | `/tmp/emterm-settings-store-test-{pid}-{name}/` | `settings.json`、corrupt テストの `.bak` を含む一時ディレクトリ |
| settings_window テスト | `/tmp/emterm-settings-window-test-{pid}/` | roundtrip テストの一時ディレクトリ |
| tmux_import テスト | `/tmp/emterm-tmux-import-test-{pid}-{name}/` | 5 テストが作成する一時ディレクトリ |
| viewer launch テスト | `/tmp/emterm-viewer-{pid}-{nanos}-{n}.json` | spawn 失敗時に残るペイロードファイル |

### 7.2 データ保持期間

| データ種別 | 保持期間 |
|------------|----------|
| テストが作成する上記の一時ファイル・ディレクトリ | 当該テストの正常終了まで（正常終了時に削除） |

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 新規依存を追加しない（NFR2）。RAII による削除が必要な場合は既存 dev-dependency の `tempfile = "3"` を使ってよい
- プロダクションコードの `/tmp` 書き込み挙動は変更しない（NFR3）

### 9.2 ビジネス上の制約

該当なし。

### 9.3 スケジュール制約

該当なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 今回の修正対象は小容量の残留（JSON ファイル・小ディレクトリ）であり、`/tmp` 100% 枯渇の最大要因だった数百 MB 級の daemon バイナリコピーは修正済みの前提である（A-3） | 中 | `tests/mux_hot_upgrade.rs:152-192` のコメントと実装がバイナリコピー先を `/tmp` から cargo ビルド出力ディレクトリ隣接に移動済みであることを示し、`DaemonGuard::Drop`（行 106-113）が panic 時も runtime dir / bin dir を削除する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: テスト等が正常終了した場合、`/tmp` に書き出した一時ファイルがクリーンアップされる。具体的検証: FR1〜FR4 の各サイトについて、該当テストの成功後に対応する `/tmp` パスが存在しない
- [ ] AC-2: 文書化された全テストコマンドの全件成功実行の前後で `/tmp` 直下のエントリ一覧を比較し、実行起因の新規残留がない
- [ ] AC-3: プログラムが途中で落ちた場合の後始末漏れは許容する（この経路への要求なし）
- [ ] AC-4: 既存テストスイート（Rust `--lib` / 統合テスト / `bun test`）が引き続き全件成功する

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS1（正常系）: `/tmp` 直下のエントリを記録 → `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` を成功完走 → 実行起因の `emterm-*` 新規エントリが `/tmp` にないことを確認
- [ ] TS2（正常系）: 同様に `--test cli_subcommands`、`crates/{term_core,term_images,app_settings,mux_ipc}` の `--lib`、`bun test` について前後比較で残留ゼロを確認
- [ ] TS3（境界値）: FR1〜FR4 の各修正テストを個別実行し、成功後に当該一時パス（`emterm-settings-store-test-*` / `emterm-settings-window-test-*` / `emterm-tmux-import-test-*` / `emterm-viewer-*.json`）が存在しないことを確認
- [ ] TS4（回帰）: 修正後も全対象テストが成功することを確認（`tabs.rs` replay テストは既知の並列フレークがあるため必要に応じ `--test-threads=1`）

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `/tmp` | `std::env::temp_dir()` が返す一時ディレクトリ |
| 正常終了 | テスト等が成功して最後まで実行を終えた状態 |
| 途中で落ちた | プロセス異常終了・kill、およびテスト失敗（assertion panic）による中断 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] クリーンアップの対象範囲: テストコードの残留のみを対象とし、プロダクションの `/tmp` 書き込みは対象外とする（A-1）。根拠: プロダクション経路は正常動作で既に片付く — ビューアペイロードは子プロセスが読取後に削除（`viewer/html_window.rs:68`、`viewer/window.rs:49`、`viewer/image_payload.rs:191`、`viewer/data_payload.rs:126`）、spawn 失敗時も `viewer/mod.rs:278` で削除。`/tmp/emterm-mux-daemon.log`（`mux/daemon.rs:271`）はソケットディレクトリにログを開けない場合のみのフォールバックで、意図的な永続ログ
- [x] テスト失敗（assertion panic）による残留の扱い: 「途中で落ちた場合」に含め、許容範囲とする（A-2）。根拠: 受け入れ条件が「プログラムが途中で落ちた場合の後始末漏れは許容する」と明記している
- [x] 大容量残留の扱い: 今回の修正対象は小容量の残留（JSON ファイル・小ディレクトリ）であり、`/tmp` 100% 枯渇の最大要因だった数百 MB 級の daemon バイナリコピーは修正済みの前提とする（A-3）
- [x] 設計ステップの要否: 不要（skipped）。テストの一時ファイル後始末のバグ修正であり、UI/UX・画面表示への影響が一切ないため

### 14.2 未確認・保留事項

なし。

## 15. 参考資料

- SPEC.md: `feature-docs/test-tmp-file-cleanup/SPEC.md`
