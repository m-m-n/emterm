# Feature: fetch-fonts-noto-color-emoji

要件定義は `feature-docs/fetch-fonts-noto-color-emoji/REQUIREMENTS.md` を参照。
本書はその実装向けの記述であり、要件 ID は REQUIREMENTS.md と同一である。

## Overview

`src-tauri/examples/swash_emoji.rs` が `include_bytes!` で
`assets/fonts/NotoColorEmoji.ttf` を埋め込む一方、`scripts/fetch-fonts.sh` は
そのファイルを取得しないため、クリーンなチェックアウトでは
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
がコンパイル段階で失敗する。本フィーチャーは、退役した `swash_emoji` example と
それ専用の Cargo マニフェスト記述を削除してこの破綻を解消し、`include_bytes!`
フォントと取得スクリプト・インベントリ表の宣言ドリフトを検出する整合性テストを
追加する。プロダクションの挙動と出荷されるフォント一式は変更しない。

## Objectives

- `bash scripts/fetch-fonts.sh` の後、`.claude/rules/core-commands.md` が案内する
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
  が、クリーンなチェックアウト上で `--lib` 回避策なしにコンパイルされ実行される。
- `include_bytes!` で埋め込まれたアセットと、それを供給するはずの取得スクリプトの
  ドリフトが、下流のビルド失敗ではなく自動チェックで検出される。
- COLRv1 移行が意図的に切り離した約 5 MiB のフォントアセット
  （doc/SPECIFICATION.md:131）を再追加するのではなく、退役したコードの削除で修正
  する。

## Assumptions

要件分析で確定した前提のみを記載する。いずれも可逆。

- **A1**: `src-tauri/examples/swash_emoji.rs` は、検証対象がプロダクトに既に存在
  しない退役 PoC である。CBDT ビットマップストライクをラスタライズするものだが、
  doc/SPECIFICATION.md:127/131 が `Noto-COLRv1.ttf` による `NotoColorEmoji.ttf`
  の置き換えと、カラー絵文字が skrifa+tiny-skia の COLRv1 paint graph で
  ラスタライズされることを記録している。削除しても生きた検証は失われない。
- **A2**: `png = "0.17"` dev-dependency は `examples/swash_emoji.rs` のためだけに
  存在する。根拠は src-tauri/Cargo.toml:202-203 のコメント自身と、解決済みの参照
  走査対象 14 件に他の `png` 参照が無いこと。実装者は削除後にコンパイルして確認
  する。
- **A3**: リグレッションチェックはテキスト上の不変条件を検証するものであり、実際の
  コンパイルを検証しない。GTK3/WebKitGTK に加えて cargo とフォントのキャッシュを
  要する新規 Rust CI ジョブの代わりとして意図的に受容した。
- **A4**: `src-tauri/src/render/font/resolver.rs` の 7 つの `include_bytes!`
  フォントパスは、`scripts/fetch-fonts.sh` の 7 つの `fetch_one` エントリ
  （fetch-fonts.sh:138-171）および `src-tauri/assets/fonts/README.md` の
  インベントリ表 7 行（README.md:23-29）と 1:1 に対応する。これは現在のファイル
  によって固定された事実であり、新しいテストが固定する対象そのものである。
- **A5**: `bold_raster_probe.rs` と `m_placement_probe.rs` は当面ハードコードされた
  絶対フォントパスのままとする。報告されたバグ、その再現手順、完了の定義がいずれも
  厳密に `cargo test` のコンパイル失敗に関するものであるため、欠陥は修正せず追随
  課題として記録する。
- **A6**: `doc/tasks/font-swash-migration/` 配下の `swash_emoji` example への歴史的
  参照は、ダングリングポインタのまま残してよい。これらの文書は完了した作業の記録
  であり、更新しない。

## User Stories

### US1: クリーンなチェックアウトで、案内どおりのテストコマンドが通る

As a 開発者 / 自動実行（無人ワークフロー）, I want to `bash scripts/fetch-fonts.sh`
の後にドキュメント記載のテストコマンドをそのまま実行できる, so that 入口で
コンパイル失敗につまずかない。

**Acceptance Criteria:**

- [ ] `src-tauri/assets/fonts/*.ttf|*.otf` が無いチェックアウトで、
      `bash scripts/fetch-fonts.sh` に続く
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
      がコンパイルされテストが実行され、`couldn't read .../NotoColorEmoji.ttf`
      エラーが発生しない。
- [ ] `src-tauri/examples/swash_emoji.rs` が存在しない。
- [ ] 残る 3 つの example が引き続きビルドされる:
      `CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml --examples`
      が成功する。

### US2: アセット宣言のドリフトがテストで検出される

As a 開発者 / 自動実行（無人ワークフロー）, I want to `include_bytes!` で埋め込んだ
フォントが取得スクリプトとインベントリ表に宣言されているかを自動で検査したい,
so that 今回と同じ破綻が下流のビルド失敗ではなくテスト失敗として、原因のフォント名
付きで分かる。

**Acceptance Criteria:**

- [ ] 新しいアセットマニフェストテストが、現在のツリーで無修正のまま通る
      （7 フォントすべてが不変条件を満たす）。
- [ ] `scripts/fetch-fonts.sh` から `fetch_one` の 1 行を一時的に削除した場合と、
      README のインベントリ表から 1 行を削除した場合のそれぞれで、新しいテストが
      失敗し、該当フォント名を示す（検証専用。どちらの編集も元に戻す）。
- [ ] `.github/workflows/ci.yml` が変更されていない。

## Technical Requirements

### Functional Requirements

- **FR1 - Delete the retired swash_emoji example:** `src-tauri/examples/swash_emoji.rs`
  を削除する。これは COMPLETED 済みの font-swash-migration の FR1 ゲートであり、
  CBDT ビットマップストライク（`Source::ColorBitmap(StrikeWith::BestFit)`、
  swash_emoji.rs:63-67）を検証するものである。doc/SPECIFICATION.md:127 は
  `Noto-COLRv1.ttf` が `NotoColorEmoji.ttf` を完全に置き換えたと記録しており、
  この example の検証対象はプロダクトに存在しない。
- **FR2 - Remove the swash_emoji `[[example]]` block:** `src-tauri/Cargo.toml` から
  `[[example]] name = "swash_emoji" / required-features = ["gui"]` ブロック
  （現在 269-271 行）を削除する。他の 3 つの `[[example]]` ブロック
  （`bold_raster_probe`、`font_select_probe`、`m_placement_probe`、273-284 行）は
  変更しない。
- **FR3 - Remove the png dev-dependency:** `src-tauri/Cargo.toml` の
  `[dev-dependencies]` から `png = "0.17"` とその説明コメント（現在 202-203 行）を
  削除する。コメント自身がこのクレートは `examples/swash_emoji.rs` のためだけに
  存在すると述べており、`swash_emoji.rs:111-116`（`png::Encoder`、
  `png::ColorType::Rgba`、`png::BitDepth::Eight`）が走査対象範囲における唯一の
  利用箇所である。`image` クレートの `png` **フィーチャー**
  （`src-tauri/Cargo.toml:91`）は無関係であり、残さなければならない。
- **FR4 - Asset-manifest consistency test:** `src-tauri/` 配下のどこかで
  `include_bytes!` により埋め込まれているすべてのフォントアセットが、(a)
  `scripts/fetch-fonts.sh` の `fetch_one` エントリとして宣言されており、**かつ**
  (b) `src-tauri/assets/fonts/README.md` の Inventory 表に記載されていることを
  表明するテストを追加する。違反時は、該当ファイル名を挙げ、2 つの宣言のどちらが
  欠けているかを示して失敗する。現時点の対象は NotoSansCJKjp-Regular.otf、
  NotoSansCJKjp-Bold.otf、Noto-COLRv1.ttf、NotoEmoji-Regular.ttf、
  Inconsolata-Regular.otf、Inconsolata-Bold.otf、NotoSansSymbols2-Regular.ttf の
  7 件（src-tauri/src/render/font/resolver.rs:21,27,34,39,50,58,64）で、いずれも
  現在のツリーで不変条件を満たしている。
- **FR5 - Test placement: `src-tauri/tests/`:** FR4 のテストは `src-tauri/tests/`
  配下の新規結合テストファイルに置く。根拠: `src-tauri/Cargo.toml` は `[[test]]`
  セクションを宣言していないため、このディレクトリのファイルは自動検出され、
  ドキュメント記載の
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
  でビルドされる。マニフェスト自身の 213-220 行のコメントが
  `tests/mux_throughput.rs` を「feature-agnostic test binary」と説明しており、
  ここの結合テストが `gui` フィーチャーに依らずビルドされることを裏付ける。
  `src-tauri/src/render/font/resolver.rs` 内のユニットテストは明示的に却下する:
  同モジュールは `#[cfg(feature = "gui")]` の背後にあり、`--no-default-features`
  ではビルドされない。
- **FR6 - Scan direction and font-path predicate:** 表明する方向は一方向である。
  すべての `include_bytes!` フォントパスがスクリプトと README に宣言されている
  こと。逆方向（すべての `fetch_one` エントリに生きた `include_bytes!` がある
  こと）は解決済みの回答では要求されておらず、現時点で対応が 1:1 であっても対象外
  とする。走査は、パスが `.ttf` または `.otf` で終わる（等価的に
  `src-tauri/assets/fonts/` 配下に解決される）`include_bytes!` リテラルに
  マッチする。そのため、クレート内のフォント以外の `include_bytes!` 利用は
  巻き込まれない。
- **FR7 - No new CI job:** `.github/workflows/ci.yml` は変更しない。同ファイルは
  現在 bun のみ（checkout / setup-bun / `bun install --frozen-lockfile` /
  `bun test`）であり、そのまま維持する。常設の Rust CI は別タスクとする。
  リグレッションチェックは既存の cargo test の面の中だけで動作し、ネットワーク、
  GTK3/WebKitGTK、フォントバイナリのいずれも必要としない。
- **FR8 - Record the out-of-scope probe defect:** 本 SPEC に、ここでは修正**しない**
  既知の追随課題として次を記録する。`src-tauri/examples/bold_raster_probe.rs`
  （22 行、26 行）と `src-tauri/examples/m_placement_probe.rs`（8 行）は、両フォント
  が既に `src-tauri/assets/fonts/` へ取得されているにもかかわらず
  `/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-{Regular,Bold}.otf`
  を絶対パスで読んでいること、および両者の実行手順が削除済みの `native-poc/`
  レイアウトを参照し続けていること（`bold_raster_probe.rs:5-6`、同様に
  `font_select_probe.rs:6-7`）。いずれのファイルも本フィーチャーでは変更しない。
  これはコンパイルは通るアドホックなローカルプローブの実行時のみの欠陥であり、
  報告されたコンパイル失敗には影響しない。詳細は下の
  "Known Follow-ups (Out of Scope)" 節に再掲する。
- **FR9 - No production behavior change:** `src-tauri/src/render/font/resolver.rs`、
  `scripts/fetch-fonts.sh`、`src-tauri/assets/fonts/README.md`、
  `src-tauri/assets/fonts/.gitignore`、`Makefile` の `fetch-fonts` ターゲットと
  その `setup`/`dev`/`build`/`win-build`/`dpkg` 依存、および `doc/SPECIFICATION.md`
  はいずれも変更しない。出荷バイナリとバンドルされる 7 フォントは影響を受けない。
  `doc/SPECIFICATION.md` は削除する example への参照を含まないため編集不要である。

### Non-Functional Requirements

- **NFR1 - Runs in the existing test surface:** 新しいテストは
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
  の下で動作し、新しいツール、ネットワークアクセス、GTK3/WebKitGTK、新しい CI
  インフラのいずれも必要としない。
- **NFR2 - Feature-agnostic:** テストは `gui` ゲート下の要素に一切依存しないため、
  `cargo test --no-default-features` でもビルドされ実行される。なお、ドキュメント
  記載の CLI-only 検証は `cargo check --no-default-features` であり、これはテスト
  ターゲットをビルドしない。
- **NFR3 - Passes in a clean checkout without font binaries:** テストは
  `resolver.rs`、`scripts/fetch-fonts.sh`、`src-tauri/assets/fonts/README.md` を
  テキストとして読む（`CARGO_MANIFEST_DIR` 基準で解決）。
  `src-tauri/assets/fonts/.gitignore` がフォントを git から除外しており、
  `fetch-fonts.sh` が一度も実行されていない状態でも通る必要があるため、
  `.ttf` / `.otf` の存在を要求してはならない。
- **NFR4 - Accepted limitation, stated explicitly in the spec:** このチェックは
  **テキスト上の不変条件（TEXTUAL invariant）を検証するものであり、実際の
  コンパイルを検証するものではない。** マクロで組み立てられた埋め込みパスや
  コンパイル時に計算されたパスは検出できず、`cargo test` がリンクすることを証明も
  しない。この制限は解決済みのリグレッションチェックの決定により受容されたもので
  あり、暗黙に留めず本 SPEC に明記する（下の "Accepted Limitation (NFR4)" 節）。
- **NFR5 - Deterministic:** テスト結果は追跡対象のソースファイルのみに依存する。
  `src-tauri/target*/` のビルドディレクトリを除外し、target ディレクトリが埋まって
  いても結果が変わらないようにする。

## Accepted Limitation (NFR4)

**The regression check verifies a TEXTUAL invariant, not an actual compile.**

新しいアセットマニフェストテストは、`include_bytes!` のリテラルパス、
`scripts/fetch-fonts.sh` の `fetch_one` エントリ、
`src-tauri/assets/fonts/README.md` のインベントリ表を、いずれもテキストとして
突き合わせる。したがって:

- マクロで組み立てられたパス、あるいはコンパイル時に計算されたパスの埋め込みは
  検出できない。
- `cargo test` が実際にコンパイル・リンクできることは証明しない。

この制限は、GTK3/WebKitGTK に加えて cargo とフォントのキャッシュを必要とする新規
Rust CI ジョブを避けるトレードオフとして意図的に受容されたものである（A3）。
常設の Rust CI は別タスクとする（FR7）。

## Known Follow-ups (Out of Scope)

本フィーチャーでは**修正しない**既知の欠陥として、次を記録する（FR8、A5）。

- `src-tauri/examples/bold_raster_probe.rs`（22 行、26 行）と
  `src-tauri/examples/m_placement_probe.rs`（8 行）は
  `/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Regular.otf` および
  `/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-Bold.otf` を絶対パスで
  読む。両フォントは既に `src-tauri/assets/fonts/` へ取得されている。
- 両者の実行手順は、削除済みの `native-poc/` レイアウトを参照し続けている
  （`bold_raster_probe.rs:5-6`、同様に `font_select_probe.rs:6-7`）。

これらはコンパイルは通るアドホックなローカルプローブの実行時のみの欠陥であり、
報告されたコンパイル失敗には影響しない。本フィーチャーではこれらのファイルを
一切変更しない。

## Implementation Approach

### Architecture

本フィーチャーはアプリケーションのアーキテクチャを変更しない。変更面は次の 3 つの
層に閉じる。

```
┌─────────────────────────────────────────────┐
│ examples/  : swash_emoji.rs を削除 (FR1)     │
├─────────────────────────────────────────────┤
│ Cargo.toml : [[example]] 削除 (FR2)          │
│              png dev-dependency 削除 (FR3)   │
├─────────────────────────────────────────────┤
│ tests/     : アセットマニフェスト整合性テスト │
│              を新規追加 (FR4, FR5)            │
└─────────────────────────────────────────────┘
      （src/ 配下のプロダクションコードは不変 — FR9）
```

**Component Diagram:**

```
新規結合テスト (src-tauri/tests/<new>.rs)
  ├── 読む: src-tauri/ 配下のソース  → include_bytes! フォントパスを収集 (FR6)
  ├── 読む: scripts/fetch-fonts.sh   → fetch_one エントリ集合
  └── 読む: src-tauri/assets/fonts/README.md → Inventory 表の行集合
        すべてテキストとして読み、CARGO_MANIFEST_DIR 基準で解決 (NFR3)
        src-tauri/target*/ は走査から除外 (NFR5)
```

### Data Flow

```
collect: include_bytes!("...{.ttf|.otf}") のリテラル群
   ↓
for each font name:
   ├── scripts/fetch-fonts.sh に fetch_one 宣言があるか
   └── assets/fonts/README.md の Inventory 表に行があるか
   ↓
両方あり → pass
いずれか欠落 → fail（フォント名 + 欠けている宣言の種別を報告）
```

方向は片方向のみ（FR6）。`fetch_one` 側から `include_bytes!` 側への検査は行わない。

### API Design

該当なし。公開 API・エンドポイントの追加や変更は無い。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/render/font/resolver.rs`: `include_bytes!` の 7 つの生きた対象を
  持つ（21,27,34,39,50,58,64 行）。テストが読むが、変更しない（FR9）。
- `scripts/fetch-fonts.sh`: `fetch_one` エントリ（138-171 行）。テストが読むが、
  変更しない（FR9）。
- `src-tauri/assets/fonts/README.md`: Inventory 表（23-29 行）。テストが読むが、
  変更しない（FR9）。

**External Dependencies:**

- 削除: `png = "0.17"`（`[dev-dependencies]`、`examples/swash_emoji.rs` 専用、FR3）
- 維持: `image` クレートの `png` フィーチャー（`src-tauri/Cargo.toml:91`）。上記
  とは無関係（FR3）
- 追加: 無し。新しいテストは新規クレートを必要としない（NFR1）

### File Structure

```
src-tauri/
├── Cargo.toml                 # [[example]] swash_emoji 削除 (FR2)
│                              # [dev-dependencies] png 削除 (FR3)
├── examples/
│   ├── swash_emoji.rs         # 削除 (FR1)
│   ├── bold_raster_probe.rs   # 変更しない (FR8)
│   ├── font_select_probe.rs   # 変更しない (FR8)
│   └── m_placement_probe.rs   # 変更しない (FR8)
└── tests/
    └── <new test file>.rs     # 新規追加 (FR4, FR5)
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/fetch-fonts-noto-color-emoji/**`
- `test-docs/fetch-fonts-noto-color-emoji/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/{feature}/**` covers
`test-docs/fetch-fonts-noto-color-emoji/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites
it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it.

## Test Scenarios

### Unit Tests

該当なし。FR5 により、検査はユニットテストではなく `src-tauri/tests/` 配下の結合
テストとして実装する。

### Integration Tests

- [ ] **TS1** (FR4, FR5, FR6, NFR1, NFR2) Happy path: 新しいテストが
      `src-tauri/src/render/font/resolver.rs` の 7 つの `include_bytes!` フォント
      パスを列挙し、それぞれを `scripts/fetch-fonts.sh` と README のインベントリ表
      の両方で見つける → pass。
- [ ] **TS2** (FR4, FR6) Missing from fetch script: フォントが埋め込まれているが
      `fetch_one` エントリが無い → fail。メッセージがフォント名と、スクリプト側の
      宣言が欠けていることを示す（報告されたバグとまったく同じ形）。
- [ ] **TS3** (FR4, FR6) Missing from README: フォントが埋め込まれ取得もされて
      いるが、インベントリ表に無い → fail。メッセージがフォント名と、README の行が
      欠けていることを示す。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

E2E テストの入力は解決されていない（`resolved_input_paths.e2e` は空）。

### Edge Cases

- [ ] **TS4** (NFR3) Clean checkout: `src-tauri/assets/fonts/` に `.gitignore`、
      `LICENSE`、`README.md` しか無い状態でテストが通る。
- [ ] **TS5** (FR4, FR6) Self-match guard: テスト自身のソースにスキャナのリテラル
      トークン `include_bytes!` が含まれる。走査は自身のファイルを除外する（または
      マッチしないようトークンを構成する）必要があり、そのうえで通ること。
- [ ] **TS6** (NFR5) Build-dir exclusion: 過去のビルドで `src-tauri/target/` が
      埋まっていても、テストは同一の結果で通る。
- [ ] **TS7** (FR6) クレート内のフォント以外の `include_bytes!` がフォントパス
      述語に無視され、誤検出を起こさない。
- [ ] **TS8** (FR1, FR2, FR3, FR9) Regression guard: デフォルトフィーチャーでの
      `cargo test` に新たな失敗が生じない。プロジェクトの記録どおり `tabs.rs` の
      replay テストは並列で非決定的（`--test-threads=1` で安定）、`tmux_sockets`
      の discover テストは稀に不安定で、いずれも既存かつ本変更とは無関係。

### Performance Tests

該当なし。性能要件は定義されていない。

## Security Considerations

該当なし。認証・認可・機微データの取り扱いはいずれも変更しない。新しいテストは
ネットワークアクセスを行わず（NFR1）、読むのは追跡対象のリポジトリ内テキスト
ファイルのみである（NFR3、NFR5）。

## Error Handling

エラー報告はテストの失敗メッセージとして行う（FR4）。

| 条件 | 失敗メッセージが含むもの |
|------|--------------------------|
| 埋め込まれたフォントに `fetch_one` 宣言が無い | 該当フォント名 + `scripts/fetch-fonts.sh` の宣言が欠けている旨 |
| 埋め込まれたフォントが README の Inventory 表に無い | 該当フォント名 + README の行が欠けている旨 |
| 両方欠けている | 該当フォント名 + 欠けている 2 つの宣言 |

## Performance Optimization

該当なし。

## Success Criteria

- [ ] `src-tauri/assets/fonts/*.ttf|*.otf` が無いチェックアウトで、
      `bash scripts/fetch-fonts.sh` に続く
      `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
      がコンパイルされテストが実行される。`couldn't read .../NotoColorEmoji.ttf`
      エラーが発生しない。
- [ ] `src-tauri/examples/swash_emoji.rs` が存在しない。
- [ ] `src-tauri/Cargo.toml` に `swash_emoji` の `[[example]]` ブロックが無く、
      `[dev-dependencies]` に `png` エントリが無い。91 行の `image` クレートの
      `png` フィーチャーは変更されていない。
- [ ] 残る 3 つの example が引き続きビルドされる:
      `CARGO_TARGET_DIR=src-tauri/target cargo build --manifest-path src-tauri/Cargo.toml --examples`
      が成功する。
- [ ] 新しいアセットマニフェストテストが、現在のツリーで無修正のまま通る
      （7 フォントすべてが不変条件を満たす）。
- [ ] ネガティブチェック: `scripts/fetch-fonts.sh` から `fetch_one` の 1 行を
      一時的に削除した場合と、README のインベントリ表から 1 行を削除した場合の
      それぞれで、新しいテストが失敗し、該当フォント名を示す（検証専用。どちらの
      編集も元に戻す）。
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`
      が引き続き通る。
- [ ] `cargo test --no-default-features --manifest-path src-tauri/Cargo.toml` が
      ビルドされ、新しいテストがそこでも通る（NFR2 の確認。ドキュメント記載の
      `cargo check` 版ではテストターゲットが動かないため）。
- [ ] ビルドに関係するファイルが `swash_emoji` を参照しない:
      `src-tauri/Cargo.toml`、`Makefile`、`scripts/`、`.github/workflows/`、
      `.claude/rules/`、`doc/SPECIFICATION.md`。`doc/tasks/font-swash-migration/`
      配下の歴史的参照はダングリングのまま残してよい。
- [ ] `.github/workflows/ci.yml` が変更されていない。
- [ ] 本 SPEC の本文に、NFR4 のテキスト不変条件の制限と、FR8 の 2 つのプローブの
      ハードコード絶対パスおよび陳腐化した `native-poc/` 実行手順に関する既知の
      追随課題の両方が含まれている。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

無し。FR1〜FR9、NFR1〜NFR5 はすべて `status: confirmed` である。

## Implementation Phases (if applicable)

該当なし。変更面は削除する example ソース 1 件、Cargo マニフェストの 2 箇所、
新規結合テスト 1 件であり、段階分割しない。

## References

- 要件定義書: `feature-docs/fetch-fonts-noto-color-emoji/REQUIREMENTS.md`
- テストコマンドの案内: `.claude/rules/core-commands.md`
- COLRv1 移行の記録: `doc/SPECIFICATION.md:124-131`
- 埋め込み箇所: `src-tauri/src/render/font/resolver.rs:21,27,34,39,50,58,64`
- フォント取得スクリプト: `scripts/fetch-fonts.sh:138-171`
- フォントインベントリ: `src-tauri/assets/fonts/README.md:23-29`
- 起票: [https://www.notion.so/3db3509ec8ee81859c85ca94adb01dbc](https://www.notion.so/3db3509ec8ee81859c85ca94adb01dbc)
