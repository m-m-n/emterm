---
title: "example-font-absolute-path"
created_date: 2026-09-21
status: draft
---

# example-font-absolute-path - 要件定義書

## 1. 概要

### 1.1 背景

`src-tauri/examples/` の font probe に開発者マシン固有の絶対パス
(`/home/sakura/workspace/Inconsolata/...`) が残っている。また `native-poc/` 時代のまま
残っている実行手順コメントが、現行の `src-tauri/` レイアウトと食い違っている。

### 1.2 目的

- `src-tauri/examples/` の font probe から開発者マシン固有の絶対パス
  (`/home/sakura/workspace/Inconsolata/...`) を除去し、任意のチェックアウトで probe が動く状態にする
- `native-poc/` 時代のまま残っている実行手順コメントを、現行の `src-tauri/` レイアウト
  (`.claude/rules/core-build-location.md`) に合わせて更新する
- 絶対パスと `native-poc/` の再混入をテストで機械的に検出できるようにする

### 1.3 スコープ

本フィーチャーの変更対象は `src-tauri/examples/` と新規テストに限る。
`src-tauri/src/render/font/resolver.rs` の `include_bytes!` によるコンパイル時埋め込みは変更しない (FR6)。

## 2. ビジネス要件

### 2.1 ビジネス目標

1. `src-tauri/examples/` の font probe から開発者マシン固有の絶対パス
   (`/home/sakura/workspace/Inconsolata/...`) を除去し、任意のチェックアウトで probe が動く状態にする
2. `native-poc/` 時代のまま残っている実行手順コメントを、現行の `src-tauri/` レイアウト
   (`.claude/rules/core-build-location.md`) に合わせて更新する
3. 絶対パスと `native-poc/` の再混入をテストで機械的に検出できるようにする

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 記載なし | requirements_analysis に対象ユーザーの記載なし |

### 2.3 期待される効果

- 任意のチェックアウトで probe が動作する
- 実行手順コメントが現行レイアウトと一致する
- 絶対パスと `native-poc/` の再混入を機械的に検出できる

## 3. ユースケース

### 3.1 ユースケース一覧

requirements_analysis にユースケースの記載なし。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | ステータス |
|----|--------|-----------|
| FR1 | bold_raster_probe のフォント読み込みを実行時解決にする | confirmed |
| FR2 | m_placement_probe のフォント読み込みを実行時解決にする | confirmed |
| FR3 | bold_raster_probe の実行手順コメントを現行レイアウトに更新する | confirmed |
| FR4 | font_select_probe の実行手順コメントを現行レイアウトに更新する | confirmed |
| FR5 | ソース走査による回帰テストを追加する | confirmed |
| FR6 | 本体側のフォント埋め込みは変更しない | confirmed |

### 4.2 機能詳細

#### FR1: bold_raster_probe のフォント読み込みを実行時解決にする

`src-tauri/examples/bold_raster_probe.rs` の Regular / Bold 2 本のフォントパス
(現行 22行目 `"/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Regular.otf"` /
26行目 `"...-Bold.otf"`) を、
`Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/Inconsolata-Regular.otf")` /
`.join("assets/fonts/Inconsolata-Bold.otf")` による実行時解決に置き換える。
区切り文字を文字列連結・`format!` で直書きせず `Path::join` を用いる。

#### FR2: m_placement_probe のフォント読み込みを実行時解決にする

`src-tauri/examples/m_placement_probe.rs` の 8 行目
`std::fs::read("/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Regular.otf")` を、
FR1 と同じ `Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/Inconsolata-Regular.otf")`
に置き換える。このファイルへの変更は絶対パス修正のみとし、実行手順コメントは新設しない。

#### FR3: bold_raster_probe の実行手順コメントを現行レイアウトに更新する

`src-tauri/examples/bold_raster_probe.rs` の 5-6 行目
(`"Run: CARGO_TARGET_DIR=native-poc/target cargo run --manifest-path native-poc/Cargo.toml --example bold_raster_probe"`)
を、プロジェクトルートから実行する現行形
(`CARGO_TARGET_DIR=src-tauri/target` / `--manifest-path src-tauri/Cargo.toml`) に書き換え、
`native-poc/` 文字列を残さない。

#### FR4: font_select_probe の実行手順コメントを現行レイアウトに更新する

`src-tauri/examples/font_select_probe.rs` の 6-7 行目の同様の `native-poc/` 実行手順コメントを、
`CARGO_TARGET_DIR=src-tauri/target` / `--manifest-path src-tauri/Cargo.toml --example font_select_probe -- Inconsolata`
の現行形に書き換える。このファイルは絶対パスを持たない (fontdb の system font 列挙のみ) ため、
フォントパス変更は行わない。

#### FR5: ソース走査による回帰テストを追加する

`src-tauri/tests/` に統合テスト (別コンパイル単位) を追加し、`src-tauri/examples/` 配下の
各 Rust ソースについて (a) `"/home/"` で始まる絶対パス literal を含まないこと、
(b) 文字列 `"native-poc/"` を含まないこと、を assert する。
テストはソースをテキストとして読むだけで、フォントバイナリの取得有無や example の実行に依存しない。

#### FR6: 本体側のフォント埋め込みは変更しない

`src-tauri/src/render/font/resolver.rs` の `include_bytes!("../../../assets/fonts/...")` による埋め込み
(`BUNDLED_BASE_FONT` / `BUNDLED_BASE_BOLD_FONT` ほか計 7 本) はコンパイル時埋め込みのまま変更しない。
本フィーチャーの変更対象は `src-tauri/examples/` と新規テストに限る。

## 5. 非機能要件

### 5.1 非機能要件一覧

| ID | 名称 | ステータス |
|----|------|-----------|
| NFR1 | テストのフォント非依存 | confirmed |
| NFR2 | プラットフォーム非依存のパス組み立て | confirmed |
| NFR3 | 既存のビルド構成を変えない | confirmed |
| NFR4 | probe の出力内容を変えない | confirmed |

### 5.2 非機能要件詳細

#### NFR1: テストのフォント非依存

FR5 のテストは `scripts/fetch-fonts.sh` によるフォント取得前のチェックアウトでも成立する。
フォントファイルの存在確認を assert に含めない。

#### NFR2: プラットフォーム非依存のパス組み立て

本プロジェクトは Linux + Windows 対応 (macOS 対象外)。
パス組み立ては `Path::join` に委ね、区切り文字を直書きしない。

#### NFR3: 既存のビルド構成を変えない

`src-tauri/Cargo.toml` の `[[example]]` 3 件の `required-features = ["gui"]` を変更しない。
新規依存クレートを追加しない。公開 API・ビルドコマンドを変更しない。

#### NFR4: probe の出力内容を変えない

3 つの probe が出力する診断内容 (coverage dump / placement dump / face 列挙) の書式と意味を変更しない。
変更はフォント取得元とコメントに限る。

## 6. UI/UX要件

該当なし。変更対象は `src-tauri/examples/` の診断用 probe のソース内文字列 (フォントパスと
実行手順コメント) とソース走査テストの追加のみで、UI 表示面・画面遷移・デザイントークンに
一切触れない。design ステップが生む成果物がない。

## 7. データ要件

requirements_analysis にデータ要件の記載なし。

## 8. 外部連携

requirements_analysis に外部連携の記載なし。

## 9. 制約条件

### 9.1 技術的制約

- Linux + Windows 対応 (macOS 対象外)。パス組み立ては `Path::join` に委ね、区切り文字を直書きしない (NFR2)
- `src-tauri/Cargo.toml` の `[[example]]` 3 件の `required-features = ["gui"]` を変更しない。
  新規依存クレートを追加しない。公開 API・ビルドコマンドを変更しない (NFR3)
- 3 つの probe の診断出力 (coverage dump / placement dump / face 列挙) の書式と意味を変更しない (NFR4)
- `src-tauri/src/render/font/resolver.rs` は無変更 (FR6)

### 9.2 ビジネス上の制約

requirements_analysis にビジネス上の制約の記載なし。

### 9.3 スケジュール制約

requirements_analysis にスケジュール制約の記載なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/example-font-absolute-path/**`
- `test-docs/example-font-absolute-path/**`

`feature-docs/example-font-absolute-path/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/example-font-absolute-path/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/example-font-absolute-path/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/example-font-absolute-path/` ディレクトリを生成しないが、宣言された `test-docs/example-font-absolute-path/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

- `gui` feature が default-on で `resolver.rs` が `include_bytes!` でフォントを埋め込むため、
  `fetch-fonts.sh` 実行前のチェックアウトでは既定 feature の `cargo test` 自体がビルド失敗しうる。
  FR5 のテストをその状態で検証する場合は `--no-default-features` での実行が要る。
  これは設計上の考慮事項であり追加要件ではない (A7)

### 10.2 ビジネスリスク

requirements_analysis にビジネスリスクの記載なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] `src-tauri/examples/` の 3 ファイルいずれにも `"/home/"` で始まる文字列リテラルが存在しない
- [ ] `src-tauri/examples/` の 3 ファイルいずれにも `"native-poc/"` 文字列が存在しない
- [ ] `bold_raster_probe.rs` / `m_placement_probe.rs` が `env!("CARGO_MANIFEST_DIR")` 起点で
      `src-tauri/assets/fonts/` 配下のフォントを実行時に読む
- [ ] `m_placement_probe.rs` の変更が絶対パス箇所のみで、ファイル先頭に新規コメントが追加されていない
- [ ] `src-tauri/tests/` の新規ソース走査テストが緑になる
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml` が
      既存テストを含めて緑のまま
- [ ] `src-tauri/src/render/font/resolver.rs` が無変更
- [ ] `scripts/fetch-fonts.sh` 実行後、3 つの probe が `cargo run --example` で実際に動作する
      (`Inconsolata-Regular.otf` / `Inconsolata-Bold.otf` は同スクリプトが `src-tauri/assets/fonts/` に取得する)

### 11.2 KPI

requirements_analysis に KPI の記載なし。

## 12. テストシナリオ

### 12.1 テスト一覧

| ID | 種別 | 説明 | 対象要件 |
|----|------|------|----------|
| TS1 | integration | `src-tauri/examples/` の全 Rust ソースを読み、`"/home/"` 始まりの絶対パス literal が 0 件であることを assert する | FR1, FR2, FR5, NFR1 |
| TS2 | integration | 同ソース群に `"native-poc/"` 文字列が 0 件であることを assert する | FR3, FR4, FR5 |
| TS3 | regression | 既存 Rust テスト一式が緑のままであること | FR6, NFR3 |
| TS4 | build | example 3 本が `gui` feature 有効でコンパイルできること | FR1, FR2, NFR2, NFR3 |
| TS5 | manual | `scripts/fetch-fonts.sh` 実行後に `bold_raster_probe` / `m_placement_probe` を実行し、フォント読み込みが成功して従来と同等の dump が出ること | FR1, FR2, NFR4 |

### 12.2 実行コマンド

| ID | コマンド |
|----|----------|
| TS1 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test <new-test-name>` |
| TS2 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test <new-test-name>` |
| TS3 | `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml` |
| TS4 | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --examples` |
| TS5 | `bash scripts/fetch-fonts.sh && CARGO_TARGET_DIR=src-tauri/target cargo run --manifest-path src-tauri/Cargo.toml --example bold_raster_probe` |

## 13. 用語定義

requirements_analysis に用語定義の記載なし。

## 14. 確認事項

### 14.1 確認済み事項

- [x] A1: `scripts/fetch-fonts.sh` が `Inconsolata-Regular.otf` と `Inconsolata-Bold.otf` を
      `DEST_DIR=src-tauri/assets/fonts` に配置する。probe が参照するファイル名はこの取得先と一致する。
- [x] A2: `env!("CARGO_MANIFEST_DIR")` は example のビルド時に `src-tauri/` を指すため、
      `join("assets/fonts/...")` が `src-tauri/assets/fonts/` に解決される。
- [x] A3: `src-tauri/src/render/font/resolver.rs` と `scripts/fetch-fonts.sh` のいずれにも
      `"/home/"` 絶対パスおよび `"native-poc"` 文字列は存在しない (走査で確認済み)。
      両ファイルは本フィーチャーで無変更。
- [x] A4: `font_select_probe.rs` はフォントファイルを直接読まず fontdb の `load_system_fonts()` のみを
      使うため、FR1/FR2 のパス変更対象外である。
- [x] A5: `src-tauri/tests/` は既に `cli_subcommands.rs` / `mux_throughput.rs` / `mux_hot_upgrade.rs` を
      持つ統合テストディレクトリであり、新規ファイル追加は既存規約
      (test/README.md "Test File Organization") に沿う。
- [x] A6: `src-tauri/Cargo.toml` の `[[example]]` 3 件はいずれも `required-features = ["gui"]` を持つため、
      `--no-default-features` ではビルド対象外になる。ソース走査テストはこれに依存しない。
- [x] A7: `gui` feature が default-on で `resolver.rs` が `include_bytes!` でフォントを埋め込むため、
      `fetch-fonts.sh` 実行前のチェックアウトでは既定 feature の `cargo test` 自体がビルド失敗しうる。
      FR5 のテストをその状態で検証する場合は `--no-default-features` での実行が要る。
      これは設計上の考慮事項であり追加要件ではない。

### 14.2 未確認・保留事項

なし (FR1-FR6 / NFR1-NFR4 はすべて confirmed)。

## 15. 参考資料

- `.claude/rules/core-build-location.md`: 現行の `src-tauri/` レイアウトと `CARGO_TARGET_DIR` の指定
- `scripts/fetch-fonts.sh`: `Inconsolata-Regular.otf` / `Inconsolata-Bold.otf` の取得元
- test/README.md "Test File Organization": `src-tauri/tests/` の既存規約
