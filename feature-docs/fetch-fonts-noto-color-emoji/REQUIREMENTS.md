---
title: "fetch-fonts-noto-color-emoji"
created_date: 2026-09-20
status: draft
---

# fetch-fonts-noto-color-emoji - 要件定義書

## 1. 概要

### 1.1 背景

`src-tauri/examples/swash_emoji.rs` が `include_bytes!` で
`assets/fonts/NotoColorEmoji.ttf` を埋め込んでいる一方、`scripts/fetch-fonts.sh`
はこのファイルを取得しない。`src-tauri/assets/fonts/.gitignore` が `*.ttf` を無視
しているため git 管理下にもない。結果として、クリーンなチェックアウトで
`bash scripts/fetch-fonts.sh` を実行した直後でも、`.claude/rules/core-commands.md`
が案内する `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
が `couldn't read .../NotoColorEmoji.ttf` でコンパイル段階から失敗する。

この破綻は、`include_bytes!` で埋め込むアセットと、それを供給するはずの取得
スクリプトとの間のドリフトが、下流のビルド失敗という形でしか現れないことに起因
する。

### 1.2 目的

- `bash scripts/fetch-fonts.sh` の後、`.claude/rules/core-commands.md` が案内する
  テストコマンドが `--lib` 回避策なしでクリーンなチェックアウト上でコンパイル・
  実行されること。
- `include_bytes!` で埋め込まれたアセットと取得スクリプトとのドリフトを、下流の
  ビルド失敗ではなく自動チェックで検出すること。
- 修正は、COLRv1 移行が意図的に切り離した約 5 MiB のフォントアセット
  （doc/SPECIFICATION.md:131）を復活させるのではなく、退役したコードを削除する
  形で行うこと。

### 1.3 スコープ

**対象**（F01〜F07 = FR1〜FR7）

- 退役した `swash_emoji` example の削除と、それに付随する `src-tauri/Cargo.toml`
  の 2 箇所の編集
- `include_bytes!` フォントアセットの宣言整合性テストの追加（`src-tauri/tests/`
  配下の新規結合テスト）

**対象外**

- `src-tauri/examples/bold_raster_probe.rs` / `src-tauri/examples/m_placement_probe.rs`
  のハードコードされた絶対パスと陳腐化した実行手順（FR8 として記録のみ。本
  フィーチャーでは修正しない）
- `.github/workflows/ci.yml` への Rust ジョブ追加（FR7）
- プロダクション挙動の変更（FR9）
- デザインステップ（`create-spec.design-step` により skip と解決済み）

## 2. ビジネス要件

### 2.1 ビジネス目標

1. `bash scripts/fetch-fonts.sh` の後、`.claude/rules/core-commands.md` に記載された
   テストコマンド（`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`）が、
   クリーンなチェックアウト上で `--lib` 回避策なしにコンパイルされ実行される。
2. `include_bytes!` で埋め込まれたアセットと、それを供給するはずの取得スクリプトとの
   ドリフトが、下流のビルド失敗ではなく自動チェックで検出される。
3. 修正は、COLRv1 移行が意図的に切り離した約 5 MiB のフォントアセットを再追加する
   のではなく、退役したコードを削除することで達成される（doc/SPECIFICATION.md:131）。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 | クリーンなチェックアウトから `fetch-fonts.sh` → `cargo test` の手順をそのまま踏む |
| 自動実行（無人ワークフロー） | ドキュメント記載のテストコマンドをそのまま実行する |

### 2.3 期待される効果

- ドキュメント記載のテストコマンドが入口でつまずかなくなる。
- アセット宣言のドリフトが、コンパイル失敗ではなくテスト失敗として、原因となった
  フォント名付きで報告される。
- バンドルサイズが増えない（退役コードの削除で解決するため）。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | クリーンなチェックアウトでテストを実行する | 開発者 / 自動実行 | 高 |
| UC02 | アセット宣言のドリフトを検出する | 開発者 / 自動実行 | 高 |

### 3.2 ユースケース詳細

#### UC01: クリーンなチェックアウトでテストを実行する

**アクター**: 開発者 / 自動実行（無人ワークフロー）

**事前条件**:

- `src-tauri/assets/fonts/` に `*.ttf` / `*.otf` が 1 つも存在しない

**基本フロー**:

1. `bash scripts/fetch-fonts.sh` を実行する
2. `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
   を実行する
3. コンパイルが通り、テストが実行される

**代替フロー**:

- `--no-default-features` 付きで実行した場合も、同じテストがビルドされ通る（NFR2）

**事後条件**:

- `couldn't read .../NotoColorEmoji.ttf` エラーが発生しない

#### UC02: アセット宣言のドリフトを検出する

**アクター**: 開発者 / 自動実行（無人ワークフロー）

**事前条件**:

- F04 のアセットマニフェスト整合性テストが存在する

**基本フロー**:

1. `src-tauri/` 配下で `include_bytes!` によりフォントを埋め込むコードが変更される
2. テストが `scripts/fetch-fonts.sh` の `fetch_one` エントリと
   `src-tauri/assets/fonts/README.md` のインベントリ表を走査する
3. 両方に宣言があれば pass する

**代替フロー**:

- 片方または両方に宣言が無い場合、テストが失敗し、該当フォント名と、2 つの宣言の
  どちらが欠けているかを示す

**事後条件**:

- ドリフトがテスト失敗として報告される

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 | 状態 |
|----|--------|------|--------|------|
| FR1 | 退役した swash_emoji example の削除 | `src-tauri/examples/swash_emoji.rs` を削除する | 高 | confirmed |
| FR2 | swash_emoji の `[[example]]` ブロック削除 | `src-tauri/Cargo.toml` から該当ブロックを削除する | 高 | confirmed |
| FR3 | png dev-dependency の削除 | `[dev-dependencies]` から `png = "0.17"` を削除する | 高 | confirmed |
| FR4 | アセットマニフェスト整合性テスト | `include_bytes!` フォントの宣言整合性を検査する | 高 | confirmed |
| FR5 | テスト配置: `src-tauri/tests/` | 新規結合テストファイルとして配置する | 高 | confirmed |
| FR6 | 走査方向とフォントパス述語 | 一方向（埋め込み → 宣言）のみを表明する | 高 | confirmed |
| FR7 | 新規 CI ジョブを追加しない | `.github/workflows/ci.yml` を変更しない | 高 | confirmed |
| FR8 | 対象外のプローブ欠陥を記録する | 2 つのプローブの既知の追随課題を spec に書く | 高 | confirmed |
| FR9 | プロダクション挙動を変更しない | 出荷バイナリと 7 フォントに影響を与えない | 高 | confirmed |

未確定（`status: tbd`）の機能要件は無い。

### 4.2 機能詳細

#### FR1: 退役した swash_emoji example の削除

**説明**: `src-tauri/examples/swash_emoji.rs` を削除する。これは COMPLETED 済みの
font-swash-migration の FR1 ゲートであり、CBDT ビットマップストライク
（`Source::ColorBitmap(StrikeWith::BestFit)`、swash_emoji.rs:63-67）を検証する
ものである。doc/SPECIFICATION.md:127 は `Noto-COLRv1.ttf` が `NotoColorEmoji.ttf`
を完全に置き換えたと記録しており、この example の検証対象はプロダクトに既に存在
しない。

#### FR2: swash_emoji の `[[example]]` ブロック削除

**説明**: `src-tauri/Cargo.toml` から
`[[example]] name = "swash_emoji" / required-features = ["gui"]` ブロック
（現在 269-271 行）を削除する。他の 3 つの `[[example]]` ブロック
（`bold_raster_probe`、`font_select_probe`、`m_placement_probe`、273-284 行）は
変更しない。

#### FR3: png dev-dependency の削除

**説明**: `src-tauri/Cargo.toml` の `[dev-dependencies]` から `png = "0.17"` と
その説明コメント（現在 202-203 行）を削除する。コメント自体がこのクレートは
`examples/swash_emoji.rs` のためだけに存在すると述べており、
`swash_emoji.rs:111-116`（`png::Encoder`、`png::ColorType::Rgba`、
`png::BitDepth::Eight`）が走査対象範囲における唯一の利用箇所である。`image`
クレートの `png` **フィーチャー**（`src-tauri/Cargo.toml:91`）は無関係であり、
残さなければならない。

#### FR4: アセットマニフェスト整合性テスト

**説明**: `src-tauri/` 配下のどこかで `include_bytes!` により埋め込まれている
すべてのフォントアセットが、(a) `scripts/fetch-fonts.sh` の `fetch_one` エントリ
として宣言されており、**かつ** (b) `src-tauri/assets/fonts/README.md` の
Inventory 表に記載されていることを表明するテストを追加する。

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| 宣言欠落 | 埋め込まれたフォントが `fetch_one` または README のどちらかに無い | 違反したファイル名を挙げ、2 つの宣言のどちらが欠けているかを示して失敗する |

**現時点の対象**: NotoSansCJKjp-Regular.otf、NotoSansCJKjp-Bold.otf、
Noto-COLRv1.ttf、NotoEmoji-Regular.ttf、Inconsolata-Regular.otf、
Inconsolata-Bold.otf、NotoSansSymbols2-Regular.ttf の 7 件
（src-tauri/src/render/font/resolver.rs:21,27,34,39,50,58,64）。いずれも現在の
ツリーで不変条件を満たしている。

#### FR5: テスト配置: `src-tauri/tests/`

**説明**: FR4 のテストは `src-tauri/tests/` 配下の新規結合テストファイルに置く。
配置制約を満たす根拠: `src-tauri/Cargo.toml` は `[[test]]` セクションを宣言して
いないため、このディレクトリのファイルは自動検出され、ドキュメント記載の
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
でビルドされる。またマニフェスト自身の 213-220 行のコメントが
`tests/mux_throughput.rs` を「feature-agnostic test binary」と説明しており、
ここの結合テストが `gui` フィーチャーに依らずビルドされることを裏付けている。
`src-tauri/src/render/font/resolver.rs` 内のユニットテストは明示的に却下する:
同モジュールは `#[cfg(feature = "gui")]` の背後にあり、`--no-default-features`
ではビルドされない。

#### FR6: 走査方向とフォントパス述語

**説明**: 表明する方向は一方向である。すべての `include_bytes!` フォントパスが
スクリプトと README に宣言されていること。逆方向（すべての `fetch_one` エントリ
に生きた `include_bytes!` があること）は解決済みの回答では要求されておらず、
たとえ現時点で対応が 1:1 であっても対象外とする。走査は、パスが `.ttf` または
`.otf` で終わる（等価的に `src-tauri/assets/fonts/` 配下に解決される）
`include_bytes!` リテラルにマッチする。そのため、クレート内の他のフォント以外の
`include_bytes!` 利用は巻き込まれない。

#### FR7: 新規 CI ジョブを追加しない

**説明**: `.github/workflows/ci.yml` は変更しない。同ファイルは現在 bun のみ
（checkout / setup-bun / `bun install --frozen-lockfile` / `bun test`）であり、
そのまま維持する。常設の Rust CI は別タスクとする。リグレッションチェックは既存の
cargo test の面の中だけで動作し、ネットワーク、GTK3/WebKitGTK、フォントバイナリの
いずれも必要としない。

#### FR8: 対象外のプローブ欠陥を記録する

**説明**: SPEC に、本フィーチャーでは修正**しない**既知の追随課題として次を記録
する。`src-tauri/examples/bold_raster_probe.rs`（22 行、26 行）と
`src-tauri/examples/m_placement_probe.rs`（8 行）は、両フォントが既に
`src-tauri/assets/fonts/` へ取得されているにもかかわらず
`/home/sakura/workspace/Inconsolata/fonts/otf/Inconsolata-{Regular,Bold}.otf`
を絶対パスで読んでいること、および両者の実行手順が削除済みの `native-poc/`
レイアウトを参照し続けていること（`bold_raster_probe.rs:5-6`、同様に
`font_select_probe.rs:6-7`）。いずれのファイルも本フィーチャーでは変更しない。
これはコンパイルは通るアドホックなローカルプローブの実行時のみの欠陥であり、
報告されたコンパイル失敗には影響しない。

#### FR9: プロダクション挙動を変更しない

**説明**: `src-tauri/src/render/font/resolver.rs`、`scripts/fetch-fonts.sh`、
`src-tauri/assets/fonts/README.md`、`src-tauri/assets/fonts/.gitignore`、
`Makefile` の `fetch-fonts` ターゲットとその `setup`/`dev`/`build`/`win-build`/`dpkg`
依存、および `doc/SPECIFICATION.md` はいずれも変更しない。出荷バイナリと
バンドルされる 7 フォントは影響を受けない。`doc/SPECIFICATION.md` は削除する
example への参照を含まないため、編集不要である。

## 5. 非機能要件

### 5.1 非機能要件一覧

| ID | 名称 | 優先度 | 状態 |
|----|------|--------|------|
| NFR1 | 既存のテスト面で動作する | 高 | confirmed |
| NFR2 | フィーチャー非依存 | 高 | confirmed |
| NFR3 | フォントバイナリの無いクリーンチェックアウトで通る | 高 | confirmed |
| NFR4 | 受容した制限を spec に明示する | 高 | confirmed |
| NFR5 | 決定的 | 高 | confirmed |

未確定（`status: tbd`）の非機能要件は無い。

### 5.2 非機能要件詳細

#### NFR1: 既存のテスト面で動作する

新しいテストは
`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
の下で動作し、新しいツール、ネットワークアクセス、GTK3/WebKitGTK、新しい CI
インフラのいずれも必要としない。

#### NFR2: フィーチャー非依存

テストは `gui` ゲート下の要素に一切依存しないため、
`cargo test --no-default-features` でもビルドされ実行される。なお、ドキュメント
記載の CLI-only 検証は `cargo check --no-default-features` であり、これはテスト
ターゲットをビルドしない点に注意する。

#### NFR3: フォントバイナリの無いクリーンチェックアウトで通る

テストは `resolver.rs`、`scripts/fetch-fonts.sh`、
`src-tauri/assets/fonts/README.md` をテキストとして読む（`CARGO_MANIFEST_DIR`
基準で解決）。`src-tauri/assets/fonts/.gitignore` がフォントを git から除外して
いるため、また `fetch-fonts.sh` が一度も実行されていない状態でも通る必要がある
ため、`.ttf` / `.otf` の存在を要求してはならない。

#### NFR4: 受容した制限を spec に明示する

このチェックは**テキスト上の不変条件**を検証するものであり、実際のコンパイルを
検証するものではない。マクロで組み立てられたパスやコンパイル時に計算されたパスの
埋め込みは検出できず、`cargo test` がリンクすることを証明もしない。この制限は
解決済みのリグレッションチェックの決定により受容されたものであり、暗黙に留めず
SPEC に明記しなければならない。

#### NFR5: 決定的

テスト結果は追跡対象のソースファイルのみに依存する。`src-tauri/target*/` の
ビルドディレクトリを除外することで、target ディレクトリが埋まっていても結果が
変わらないようにする。

### 5.3 本フィーチャーで要件の無いカテゴリ

パフォーマンス、セキュリティ、可用性、互換性（ブラウザ / API バージョン）に関する
要件は、確定した要件セットに含まれていない。

## 6. UI/UX要件

該当なし。デザインステップは `create-spec.design-step` で skip と解決されている。
変更面は削除する example ソース 1 件、Cargo マニフェストの 2 箇所、新規結合テスト
1 件であり、ユーザーに見える UI、描画挙動、デザイントークンのいずれにも影響しない
（FR9）。

## 7. データ要件

該当なし。データモデル、データ項目、データ保持期間のいずれにも変更は無い。

## 8. 外部連携

該当なし。新しいテストはネットワークアクセスを行わない（NFR1）。

## 9. 制約条件

### 9.1 技術的制約

- テストは既存の cargo test 面の中だけで動作し、新しいツール・ネットワーク・
  GTK3/WebKitGTK・新規 CI インフラを使えない（NFR1、FR7）。
- テストは `gui` ゲート下の要素に依存できない。`src-tauri/src/render/font/resolver.rs`
  内のユニットテストは `--no-default-features` でビルドされないため不可（FR5、NFR2）。
- テストは `.ttf` / `.otf` の存在を前提にできない（NFR3）。
- テスト自身のソースに含まれる `include_bytes!` リテラルが自己マッチしないように
  走査から自身のファイルを除外する（テストシナリオ参照）。
- `src-tauri/target*/` を走査から除外する（NFR5）。

### 9.2 ビジネス上の制約

- COLRv1 移行が意図的に切り離した約 5 MiB のフォントアセットを再追加しない
  （doc/SPECIFICATION.md:131）。

### 9.3 スケジュール制約

無し。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各
タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:

- `feature-docs/fetch-fonts-noto-color-emoji/**`
- `test-docs/fetch-fonts-noto-color-emoji/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、
`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、
`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザイン
ステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび
`references/phase-state.md` を参照。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式:
`test-docs/fetch-fonts-noto-color-emoji/{T}.tests.yaml`）。生成主体は
`implement-phase.md` を参照。

**意味論**:

- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。
  除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に
  含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても
  違反にはならない。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| チェックはテキスト上の不変条件のみを検証し、実際のコンパイルは検証しない。マクロ生成・コンパイル時計算されたパスは検出できない | 中 | 受容済みの制限として SPEC に明記する（NFR4、A3） |
| `png = "0.17"` が `examples/swash_emoji.rs` 専用であるという前提が誤っている可能性 | 低 | 削除後にコンパイルして確認する（A2） |
| 既存スイートの非決定性: `tabs.rs` の replay テストは並列実行で不安定、`tmux_sockets` の discover テストは稀に失敗 | 低 | 本変更とは無関係の既存事象として扱う（必要なら `--test-threads=1`） |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| 常設の Rust CI が無いままとなり、テキスト不変条件を超える破綻は検出されない | 中 | 中 | 常設 Rust CI は別タスクとする（FR7） |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] `src-tauri/assets/fonts/*.ttf|*.otf` が無いチェックアウトで、`bash scripts/fetch-fonts.sh`
      に続けて `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml`
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
- [ ] ネガティブチェック: `scripts/fetch-fonts.sh` から `fetch_one` の 1 行を一時的に
      削除した場合と、README のインベントリ表から 1 行を削除した場合のそれぞれで、
      新しいテストが失敗し、該当フォント名を示す（検証専用。どちらの編集も元に戻す）。
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
- [ ] SPEC の本文に、NFR4 のテキスト不変条件の制限と、FR8 の 2 つのプローブの
      ハードコード絶対パスおよび陳腐化した `native-poc/` 実行手順に関する既知の
      追随課題の両方が含まれている。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: 新しいテストが `src-tauri/src/render/font/resolver.rs` の 7 つの
      `include_bytes!` フォントパスを列挙し、それぞれを `scripts/fetch-fonts.sh`
      と README のインベントリ表の両方で見つける → pass。
- [ ] 異常系: フォントが埋め込まれているが `fetch_one` エントリが無い → fail。
      メッセージがフォント名とスクリプト側宣言の欠落を示す（報告されたバグと
      同じ形）。
- [ ] 異常系: フォントが埋め込まれ取得もされているが、インベントリ表に無い →
      fail。メッセージがフォント名と README 行の欠落を示す。
- [ ] 境界値: クリーンチェックアウト。`src-tauri/assets/fonts/` に `.gitignore`、
      `LICENSE`、`README.md` しか無い状態でテストが通る。
- [ ] 境界値: 自己マッチガード。テスト自身のソースにスキャナのリテラル
      `include_bytes!` が含まれるため、走査から自身のファイルを除外する
      （またはトークンがマッチしないよう構成する）。そのうえで通る。
- [ ] 境界値: ビルドディレクトリ除外。過去のビルドで `src-tauri/target/` が
      埋まっていても結果が変わらない。
- [ ] 境界値: クレート内のフォント以外の `include_bytes!` がフォントパス述語で
      無視され、誤検出を起こさない。
- [ ] リグレッション: デフォルトフィーチャーでの `cargo test` に新たな失敗が
      生じない。プロジェクトの記録どおり `tabs.rs` の replay テストは並列で
      非決定的（`--test-threads=1` で安定）、`tmux_sockets` の discover テストは
      稀に不安定で、いずれも既存かつ本変更とは無関係。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| `include_bytes!` | コンパイル時にファイル内容をバイナリへ埋め込む Rust マクロ |
| `fetch_one` | `scripts/fetch-fonts.sh` における 1 フォント取得エントリ |
| Inventory 表 | `src-tauri/assets/fonts/README.md` のフォント一覧表 |
| CBDT ビットマップストライク | ビットマップ形式のカラー絵文字グリフ。`swash_emoji.rs` の検証対象だった |
| COLRv1 | ベクターのカラーフォント形式。`Noto-COLRv1.ttf` が採用し、skrifa+tiny-skia の paint graph でラスタライズされる |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 修正方針（`fix-approach`）: `src-tauri/examples/swash_emoji.rs` を、
      `src-tauri/Cargo.toml` の `[[example]]` ブロックおよびそれ専用に存在する
      `png` dev-dependency とともに削除する。
- [x] リグレッション検出手段（`regression-check`）: `src-tauri/` 配下で
      `include_bytes!` により埋め込まれるすべてのフォントパスが、
      `scripts/fetch-fonts.sh` の `fetch_one` エントリとして宣言され、かつ
      `src-tauri/assets/fonts/README.md` のインベントリ表に記載されていることを
      表明する整合性テストを追加する。新規 CI ジョブは作らない。受容した制限:
      これはテキスト上の不変条件を検証するものであり、実際のコンパイルを検証する
      ものではない。
- [x] 他の example の扱い（`other-examples-scope`）: 本フィーチャーでは対象外。
      `bold_raster_probe.rs` と `m_placement_probe.rs` のハードコード絶対パスの
      欠陥は既知の追随課題として spec に記録し、これらのファイルはここでは変更
      しない。
- [x] デザインステップ（`design-step`）: skip する。

**前提（assumptions）**

| ID | 内容 | 可逆 |
|----|------|------|
| A1 | `src-tauri/examples/swash_emoji.rs` は、検証対象がプロダクトに既に存在しない退役 PoC である。CBDT ビットマップストライクをラスタライズするものだが、doc/SPECIFICATION.md:127/131 が `Noto-COLRv1.ttf` による `NotoColorEmoji.ttf` の置き換えと、カラー絵文字が skrifa+tiny-skia の COLRv1 paint graph でラスタライズされることを記録している。削除しても生きた検証は失われない | 可 |
| A2 | `png = "0.17"` dev-dependency は `examples/swash_emoji.rs` のためだけに存在する。根拠は src-tauri/Cargo.toml:203 のコメント自身と、解決済みの 14 の参照走査対象すべてに他の `png` 参照が無いこと。実装者は削除後にコンパイルして確認する | 可 |
| A3 | リグレッションチェックはテキスト上の不変条件を検証するものであり、実際のコンパイルを検証しない。GTK3/WebKitGTK に加えて cargo とフォントのキャッシュを要する新規 Rust CI ジョブの代わりとして、意図的に受容した | 可 |
| A4 | `src-tauri/src/render/font/resolver.rs` の 7 つの `include_bytes!` フォントパスは、`scripts/fetch-fonts.sh` の 7 つの `fetch_one` エントリおよび README インベントリ表の 7 行と 1:1 に対応する。これは現在のファイルによって固定された事実であり、新しいテストが固定する対象そのものである | 可 |
| A5 | `bold_raster_probe.rs` と `m_placement_probe.rs` は当面ハードコードされた絶対フォントパスのままとする。報告されたバグ、その再現手順、完了の定義がいずれも厳密に `cargo test` のコンパイル失敗に関するものであるため、欠陥は修正せず追随課題として記録する | 可 |
| A6 | `doc/tasks/font-swash-migration/` 配下の `swash_emoji` example への歴史的参照は、ダングリングポインタのまま残してよい。これらの文書は完了した作業の記録であり、更新しない | 可 |

### 14.2 未確認・保留事項

- [ ] `src-tauri/examples/bold_raster_probe.rs` および
      `src-tauri/examples/m_placement_probe.rs` のハードコード絶対パスと、陳腐化
      した `native-poc/` 実行手順（FR8 として記録済み。本フィーチャーでは修正
      しない）
- [ ] 常設の Rust CI の導入（FR7 により本フィーチャーの対象外）

未確定（`status: tbd`）の要件は無い。

## 15. 参考資料

- SPEC: `feature-docs/fetch-fonts-noto-color-emoji/SPEC.md`
- テストコマンドの案内: `.claude/rules/core-commands.md`
- COLRv1 移行の記録: `doc/SPECIFICATION.md:124-131`
- フォント取得スクリプト: `scripts/fetch-fonts.sh`
- フォントインベントリ: `src-tauri/assets/fonts/README.md`
- 埋め込み箇所: `src-tauri/src/render/font/resolver.rs:21,27,34,39,50,58,64`
- 起票: [https://www.notion.so/3db3509ec8ee81859c85ca94adb01dbc](https://www.notion.so/3db3509ec8ee81859c85ca94adb01dbc)
