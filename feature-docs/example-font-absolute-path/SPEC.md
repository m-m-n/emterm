# Feature: example-font-absolute-path

## Overview

`src-tauri/examples/` の font probe に残る開発者マシン固有の絶対パス
(`/home/sakura/workspace/Inconsolata/...`) を `env!("CARGO_MANIFEST_DIR")` 起点の実行時解決に置き換え、
`native-poc/` 時代のまま残っている実行手順コメントを現行の `src-tauri/` レイアウトに更新する。
あわせて絶対パスと `native-poc/` の再混入を検出するソース走査テストを `src-tauri/tests/` に追加する。
要件の詳細は REQUIREMENTS.md を参照。

## Objectives

- `src-tauri/examples/` の font probe から開発者マシン固有の絶対パス
  (`/home/sakura/workspace/Inconsolata/...`) を除去し、任意のチェックアウトで probe が動く状態にする
- `native-poc/` 時代のまま残っている実行手順コメントを、現行の `src-tauri/` レイアウト
  (`.claude/rules/core-build-location.md`) に合わせて更新する
- 絶対パスと `native-poc/` の再混入をテストで機械的に検出できるようにする

## Technical Requirements

### Functional Requirements

- **FR1 - bold_raster_probe のフォント読み込みを実行時解決にする:**
  `src-tauri/examples/bold_raster_probe.rs` の Regular / Bold 2 本のフォントパス
  (現行 22行目 `"/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Regular.otf"` /
  26行目 `"...-Bold.otf"`) を、
  `Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/Inconsolata-Regular.otf")` /
  `.join("assets/fonts/Inconsolata-Bold.otf")` による実行時解決に置き換える。
  区切り文字を文字列連結・`format!` で直書きせず `Path::join` を用いる。
- **FR2 - m_placement_probe のフォント読み込みを実行時解決にする:**
  `src-tauri/examples/m_placement_probe.rs` の 8 行目
  `std::fs::read("/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Regular.otf")` を、
  FR1 と同じ `Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/Inconsolata-Regular.otf")`
  に置き換える。このファイルへの変更は絶対パス修正のみとし、実行手順コメントは新設しない。
- **FR3 - bold_raster_probe の実行手順コメントを現行レイアウトに更新する:**
  `src-tauri/examples/bold_raster_probe.rs` の 5-6 行目
  (`"Run: CARGO_TARGET_DIR=native-poc/target cargo run --manifest-path native-poc/Cargo.toml --example bold_raster_probe"`)
  を、プロジェクトルートから実行する現行形
  (`CARGO_TARGET_DIR=src-tauri/target` / `--manifest-path src-tauri/Cargo.toml`) に書き換え、
  `native-poc/` 文字列を残さない。
- **FR4 - font_select_probe の実行手順コメントを現行レイアウトに更新する:**
  `src-tauri/examples/font_select_probe.rs` の 6-7 行目の同様の `native-poc/` 実行手順コメントを、
  `CARGO_TARGET_DIR=src-tauri/target` /
  `--manifest-path src-tauri/Cargo.toml --example font_select_probe -- Inconsolata` の現行形に書き換える。
  このファイルは絶対パスを持たない (fontdb の system font 列挙のみ) ため、フォントパス変更は行わない。
- **FR5 - ソース走査による回帰テストを追加する:**
  `src-tauri/tests/` に統合テスト (別コンパイル単位) を追加し、`src-tauri/examples/` 配下の
  各 Rust ソースについて (a) `"/home/"` で始まる絶対パス literal を含まないこと、
  (b) 文字列 `"native-poc/"` を含まないこと、を assert する。
  テストはソースをテキストとして読むだけで、フォントバイナリの取得有無や example の実行に依存しない。
- **FR6 - 本体側のフォント埋め込みは変更しない:**
  `src-tauri/src/render/font/resolver.rs` の `include_bytes!("../../../assets/fonts/...")` による埋め込み
  (`BUNDLED_BASE_FONT` / `BUNDLED_BASE_BOLD_FONT` ほか計 7 本) はコンパイル時埋め込みのまま変更しない。
  本フィーチャーの変更対象は `src-tauri/examples/` と新規テストに限る。

### Non-Functional Requirements

- **NFR1 - テストのフォント非依存:**
  FR5 のテストは `scripts/fetch-fonts.sh` によるフォント取得前のチェックアウトでも成立する。
  フォントファイルの存在確認を assert に含めない。
- **NFR2 - プラットフォーム非依存のパス組み立て:**
  本プロジェクトは Linux + Windows 対応 (macOS 対象外)。
  パス組み立ては `Path::join` に委ね、区切り文字を直書きしない。
- **NFR3 - 既存のビルド構成を変えない:**
  `src-tauri/Cargo.toml` の `[[example]]` 3 件の `required-features = ["gui"]` を変更しない。
  新規依存クレートを追加しない。公開 API・ビルドコマンドを変更しない。
- **NFR4 - probe の出力内容を変えない:**
  3 つの probe が出力する診断内容 (coverage dump / placement dump / face 列挙) の書式と意味を変更しない。
  変更はフォント取得元とコメントに限る。

## Implementation Approach

### Architecture

本フィーチャーはアーキテクチャを変更しない。変更は `src-tauri/examples/` のソース内文字列
(フォントパスと実行手順コメント) と、`src-tauri/tests/` への新規ソース走査テスト追加に限る。

### Data Flow

probe のフォント取得元だけが変わる。

```
変更前: bold_raster_probe / m_placement_probe → "/home/sakura/workspace/Inconsolata/fonts/otf/*.otf" (絶対パス literal)
変更後: bold_raster_probe / m_placement_probe → Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/*.otf")
```

`env!("CARGO_MANIFEST_DIR")` は example のビルド時に `src-tauri/` を指すため、
`join("assets/fonts/...")` が `src-tauri/assets/fonts/` に解決される (A2)。
このフォントは `scripts/fetch-fonts.sh` が `DEST_DIR=src-tauri/assets/fonts` に配置する (A1)。

### API Design

該当なし。公開 API を変更しない (NFR3)。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**

- `scripts/fetch-fonts.sh`: `Inconsolata-Regular.otf` / `Inconsolata-Bold.otf` を
  `src-tauri/assets/fonts/` に取得する (A1)。本フィーチャーで無変更 (A3)。
- `src-tauri/src/render/font/resolver.rs`: `include_bytes!` によるコンパイル時埋め込み。無変更 (FR6, A3)。

**External Dependencies:**

新規依存クレートを追加しない (NFR3)。

### File Structure

```
src-tauri/
├── examples/
│   ├── bold_raster_probe.rs     # FR1 (フォントパス), FR3 (実行手順コメント)
│   ├── m_placement_probe.rs     # FR2 (フォントパスのみ / コメント新設なし)
│   └── font_select_probe.rs     # FR4 (実行手順コメントのみ)
├── tests/
│   └── <new-test-name>.rs       # FR5 (ソース走査テスト、別コンパイル単位)
└── src/render/font/resolver.rs  # FR6 (無変更)
```

`src-tauri/tests/` は既に `cli_subcommands.rs` / `mux_throughput.rs` / `mux_hot_upgrade.rs` を持つ
統合テストディレクトリであり、新規ファイル追加は既存規約
(test/README.md "Test File Organization") に沿う (A5)。

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/example-font-absolute-path/**`
- `test-docs/example-font-absolute-path/**`

`feature-docs/example-font-absolute-path/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/example-font-absolute-path/**` covers
`test-docs/example-font-absolute-path/{T}.tests.yaml`, the per-task test record.
It is generated and owned by `implement-phase.md`; this section cites it and
restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/example-font-absolute-path/` directory at all; the declared
`test-docs/example-font-absolute-path/**` entry is still correct in that case — a
declared path that never materializes is not a violation.

## Test Scenarios

### Integration Tests

- [ ] **TS1** (FR1, FR2, FR5, NFR1): `src-tauri/examples/` の全 Rust ソースを読み、
      `"/home/"` 始まりの絶対パス literal が 0 件であることを assert する
      — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test <new-test-name>`
- [ ] **TS2** (FR3, FR4, FR5): 同ソース群に `"native-poc/"` 文字列が 0 件であることを assert する
      — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test <new-test-name>`

### Regression Tests

- [ ] **TS3** (FR6, NFR3): 既存 Rust テスト一式が緑のままであること
      — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`

### Build Checks

- [ ] **TS4** (FR1, FR2, NFR2, NFR3): example 3 本が `gui` feature 有効でコンパイルできること
      — `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --examples`

### Manual Tests

- [ ] **TS5** (FR1, FR2, NFR4): `scripts/fetch-fonts.sh` 実行後に `bold_raster_probe` /
      `m_placement_probe` を実行し、フォント読み込みが成功して従来と同等の dump が出ること
      — `bash scripts/fetch-fonts.sh && CARGO_TARGET_DIR=src-tauri/target cargo run --manifest-path src-tauri/Cargo.toml --example bold_raster_probe`

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

## Assumptions

REQUIREMENTS.md 14.1 と同一。

- **A1**: `scripts/fetch-fonts.sh` が `Inconsolata-Regular.otf` と `Inconsolata-Bold.otf` を
  `DEST_DIR=src-tauri/assets/fonts` に配置する。probe が参照するファイル名はこの取得先と一致する。
- **A2**: `env!("CARGO_MANIFEST_DIR")` は example のビルド時に `src-tauri/` を指すため、
  `join("assets/fonts/...")` が `src-tauri/assets/fonts/` に解決される。
- **A3**: `src-tauri/src/render/font/resolver.rs` と `scripts/fetch-fonts.sh` のいずれにも
  `"/home/"` 絶対パスおよび `"native-poc"` 文字列は存在しない (走査で確認済み)。
  両ファイルは本フィーチャーで無変更。
- **A4**: `font_select_probe.rs` はフォントファイルを直接読まず fontdb の `load_system_fonts()` のみを
  使うため、FR1/FR2 のパス変更対象外である。
- **A5**: `src-tauri/tests/` は既に `cli_subcommands.rs` / `mux_throughput.rs` / `mux_hot_upgrade.rs` を
  持つ統合テストディレクトリであり、新規ファイル追加は既存規約
  (test/README.md "Test File Organization") に沿う。
- **A6**: `src-tauri/Cargo.toml` の `[[example]]` 3 件はいずれも `required-features = ["gui"]` を持つため、
  `--no-default-features` ではビルド対象外になる。ソース走査テストはこれに依存しない。
- **A7**: `gui` feature が default-on で `resolver.rs` が `include_bytes!` でフォントを埋め込むため、
  `fetch-fonts.sh` 実行前のチェックアウトでは既定 feature の `cargo test` 自体がビルド失敗しうる。
  FR5 のテストをその状態で検証する場合は `--no-default-features` での実行が要る。
  これは設計上の考慮事項であり追加要件ではない。

## Security Considerations

該当なし。requirements_analysis にセキュリティ要件の記載なし。

## Error Handling

該当なし。requirements_analysis にエラーハンドリング要件の記載なし。

## Performance Optimization

該当なし。requirements_analysis に性能目標の記載なし。

## Success Criteria

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

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。FR1-FR6 / NFR1-NFR4 はすべて confirmed。

## Design Step

Skipped: 変更対象は `src-tauri/examples/` の診断用 probe のソース内文字列 (フォントパスと
実行手順コメント) とソース走査テストの追加のみで、UI 表示面・画面遷移・デザイントークンに
一切触れない。design ステップが生む成果物がない。

## References

- REQUIREMENTS.md: `feature-docs/example-font-absolute-path/REQUIREMENTS.md`
- `.claude/rules/core-build-location.md`: 現行の `src-tauri/` レイアウトと `CARGO_TARGET_DIR` の指定
- `scripts/fetch-fonts.sh`: `Inconsolata-Regular.otf` / `Inconsolata-Bold.otf` の取得元
- test/README.md "Test File Organization": `src-tauri/tests/` の既存規約
