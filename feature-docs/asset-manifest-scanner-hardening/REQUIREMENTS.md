---
title: "asset-manifest-scanner-hardening"
created_date: 2026-09-21
status: draft
---

# asset-manifest-scanner-hardening - 要件定義書

## 1. 概要

### 1.1 背景

`src-tauri/tests/asset_manifest.rs` のフォント埋め込みリグレッションガードに、走査の取りこぼしが 2 件ある。

- ギャップ 1: `is_excluded_path` が `BUILD_OUTPUT_PREFIX`（`"target"`）の前方一致規則を相対パスの全 `Component::Normal` に適用しており、`target` で始まる名前の通常ファイル（例: `src/target_resolver.rs`）まで走査対象から外れる（`src-tauri/tests/asset_manifest.rs:175-183`、`:194-219`）。
- ギャップ 2: `embedded_font_names` が `include_bytes!(` に一致した後、マクロ呼び出しの境界を見ずに次の `"` を探索し、`rest` を引用符の直後へ進める（`src-tauri/tests/asset_manifest.rs:86-113`）。

### 1.2 目的

走査の正確さだけを上げ、現行ツリーでの判定結果は一切変えない。

### 1.3 スコープ

変更は `src-tauri/tests/asset_manifest.rs` に閉じる。プロダクションコード（`src-tauri/build.rs`、`src-tauri/src/render/font/resolver.rs`）は変更しない。

## 2. ビジネス要件

### 2.1 ビジネス目標

- `src-tauri/tests/asset_manifest.rs` のフォント埋め込みリグレッションガードが、将来のパス追加・ファイル追加で静かに素通りしないようにする
- 現行ツリーでのガードの判定結果（埋め込み 7 フォント、宣言との突き合わせ）を一切変えずに、走査の正確さだけを上げる

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm 開発者 | `src-tauri/` にフォント埋め込みを追加・変更し、`--test asset_manifest` のガードで宣言との整合を検査する |

### 2.3 期待される効果

- `target` で始まる名前の通常ファイルに置かれた埋め込みがガードに捕捉される
- マクロ呼び出しの外側にある無関係な文字列リテラルを引数として拾わなくなる

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 埋め込みフォントの宣言整合をテストで検査する | eMterm 開発者 | 高 |

### 3.2 ユースケース詳細

#### UC01: 埋め込みフォントの宣言整合をテストで検査する

**アクター**: eMterm 開発者

**事前条件**:
- `src-tauri/` 配下の Rust ソースに `include_bytes!` によるフォント埋め込みがある

**基本フロー**:
1. 開発者が `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test asset_manifest` を実行する
2. `collect_rust_source` がビルド成果物ディレクトリを除いた Rust ソースを収集する
3. `embedded_font_names` が各ソースから `include_bytes!` の引数パスを抽出する
4. 抽出された集合が宣言（fetch script / inventory、`documented_embedded_fonts()`）と突き合わされる

**代替フロー**:
- 抽出集合が宣言と一致しない場合、`embedded_fonts_are_declared_in_fetch_script_and_inventory` が失敗する

**事後条件**:
- 埋め込みと宣言の差分がテスト結果として可視化される

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | target プレフィックス除外をディレクトリ構成要素だけに適用する | 前方一致規則の適用範囲を最終構成要素以外へ限定する | resolved |
| FR2 | include_bytes! の引数抽出をマクロ呼び出しの括弧内に限定する | 対応する閉じ括弧までを引数領域として確定する | resolved |
| FR3 | 現行ツリーでの走査結果を不変に保つ | 7 件の抽出と build.rs:160 の空集合を維持する | resolved |
| FR4 | 走査対象のマクロ区切り文字は丸括弧形式のみ | 角括弧・波括弧形式への対応は追加しない | resolved |
| FR5 | 括弧内に文字列リテラルが無い呼び出しは抽出せず次へ進む | 走査を打ち切らず閉じ括弧の直後から継続する | resolved |
| FR6 | 2 件のギャップそれぞれに再発検出テストを持つ | FR1 / FR2 の再発を検出するテストを追加する | resolved |

### 4.2 機能詳細

#### FR1: target プレフィックス除外をディレクトリ構成要素だけに適用する

**説明**: `is_excluded_path` は `BUILD_OUTPUT_PREFIX`（`"target"`）の前方一致規則を、相対パスの最終構成要素を除く構成要素（＝ディレクトリ部分）にのみ適用する。これにより `target` で始まる名前の通常ファイル（例: `src/target_resolver.rs`、`src/target.rs`）は走査対象に残る。ディレクトリ自体の除外は、この規則と `collect_rust_source` 内の `is_build_output_dir(name_str)` 判定（現行 asset_manifest.rs:209）で従来どおり維持される。

**根拠**: `src-tauri/tests/asset_manifest.rs:175-183`（`is_excluded_path` が `Component::Normal` 全てに規則を適用）、`:194-219`（`collect_rust_source` が dir 判定前に `is_excluded_path` を呼ぶ）

#### FR2: include_bytes! の引数抽出をマクロ呼び出しの括弧内に限定する

**説明**: `embedded_font_names` は固定部分文字列 `include_bytes!(` に一致した後、その呼び出しの対応する閉じ括弧までを引数領域として確定し、文字列リテラルの探索をその領域内に限定する。次の走査再開位置は、当該呼び出しの閉じ括弧の直後とし、呼び出し外で見つかった引用符から再開しない。

**根拠**: `src-tauri/tests/asset_manifest.rs:86-113`（needle 一致後、境界を見ずに次の `"` を探索し、`rest` を引用符の直後へ進める）

#### FR3: 現行ツリーでの走査結果を不変に保つ

**説明**: 変更後も `src-tauri/src/render/font/resolver.rs` の 7 件の埋め込み（NotoSansCJKjp-Regular.otf / NotoSansCJKjp-Bold.otf / Noto-COLRv1.ttf / NotoEmoji-Regular.ttf / Inconsolata-Regular.otf / Inconsolata-Bold.otf / NotoSansSymbols2-Regular.ttf）が抽出され、`src-tauri/build.rs:160` の `include_bytes!({abs_str:?})` は空集合を返す。現在この箇所は呼び出し外まで走って `println!("cargo:rerun-if-changed=...")` 側の引用符までを拾っており（フォント拡張子でないため誤検出には至っていない）、FR2 の適用後はその横断自体が起きない。

**根拠**: `src-tauri/src/render/font/resolver.rs:21,27,34,39,50,58,64`、`src-tauri/build.rs:160`。build.rs の他の `include_bytes` 出現（8,42,44,68 行）は `(` を伴わないコメントで needle に一致しない。

#### FR4: 走査対象のマクロ区切り文字は丸括弧形式のみ

**説明**: `include_bytes!` の走査対象は丸括弧形式 `include_bytes!( ... )` のみに保つ。角括弧形式 `include_bytes![...]` / 波括弧形式 `include_bytes!{...}` への対応は追加しない。今回追加するのは境界検出のみ。

**根拠**: 回答 `requirement.include-bytes-delimiter-forms = parens_only`（Codex が src-tauri 配下 287 Rust ソースを走査し、角括弧/波括弧形式ゼロ、`concat!` 経由の埋め込みゼロを確認）

#### FR5: 括弧内に文字列リテラルが無い呼び出しは抽出せず次へ進む

**説明**: 確定した括弧内領域の最初の文字列リテラルだけを引数パスとみなす。領域内に文字列リテラルが存在しない場合、その呼び出しからは何も抽出せず、閉じ括弧の直後から走査を継続する（途中で `break` して以降の走査を打ち切らない）。ネストしたマクロ（`concat!` 等）の内部へは降りない。

**根拠**: 回答 `requirement.non-literal-macro-argument = skip_non_literal`（`build.rs:160` がツリー唯一の非リテラル呼び出し形で、空集合を返さねばならない）

#### FR6: 2 件のギャップそれぞれに再発検出テストを持つ

**説明**: `src-tauri/tests/asset_manifest.rs` に、FR1 と FR2 それぞれの再発を検出するテストを追加する（TS1〜TS5）。既存テスト（`walk_excludes_build_output_directories`、`walk_excludes_its_own_source_file`、`exactly_the_documented_fonts_are_embedded` ほか）は変更せずに通ること。

**根拠**: task_description の完了の定義「再発を検出するテストがある」

## 5. 非機能要件

| ID | 内容 | 状態 |
|----|------|------|
| NFR1 | IMPLEMENTATION D5 の構造を保つ：比較・テキスト解析の各関数は純粋関数のままとし、ファイルシステムに触れるのはファイル末尾のリポジトリ結合部（`collect_rust_source` / `read_from_crate_root` / `crate_root`）に限る。 | resolved |
| NFR2 | 新規依存を追加しない。std と既存の dev-dependency（`tempfile = "3"`）のみを使う。proptest / criterion 等のテストフレームワークは導入しない。 | resolved |
| NFR3 | ビルド成果物ディレクトリ（`target` / `target-host` / `target-win` およびその前方一致ディレクトリ）は引き続き走査しない。 | resolved |
| NFR4 | 走査コストはソーステキスト長に対して概ね線形のまま保つ（1 パスのテキスト走査）。 | resolved |
| NFR5 | 変更は `src-tauri/tests/asset_manifest.rs` に閉じる。プロダクションコード（`src-tauri/build.rs`、`src-tauri/src/render/font/resolver.rs`）は変更しない。`build.rs:160` はギャップ 2 の同型実例としての参照対象であり、修正対象ではない。 | resolved |
| NFR6 | 既存のテスト命名規約 `fn <subject>_<scenario>_<expected>()` と、同ファイル内の既存テストの構築スタイル（合成テキスト／`tempfile::tempdir` による一時ツリー）に揃える。 | resolved |

## 6. UI/UX要件

該当なし。ユーザー可視の UI / UX 表面を持たない変更であり、デザインステップは skip と決定された（`create-spec.design-step` の回答 `decide_autonomously`）。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 走査対象のマクロ区切り文字は丸括弧形式のみ（FR4）
- 新規依存を追加しない（NFR2）
- 変更は `src-tauri/tests/asset_manifest.rs` に閉じる（NFR5）

### 9.2 ビジネス上の制約

該当なし。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/asset-manifest-scanner-hardening/**`
- `test-docs/asset-manifest-scanner-hardening/**`

`feature-docs/{feature}/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照。

`test-docs/{feature}/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/asset-manifest-scanner-hardening/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 括弧境界の検出を誤ると現行ツリーの抽出結果（7 件）が変わる | 高 | FR3 / AC5 / TS6 で 7 件の集合の不変を固定する |
| 除外規則の緩和がビルド成果物ディレクトリの走査を招く | 高 | NFR3 / AC2 / TS2 で既存の除外 4 パスを固定する |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1: `src-tauri/` 配下に `target` で始まる名前の `.rs` ファイルを置き、その中で未宣言フォントを `include_bytes!` した場合、`embedded_fonts_are_declared_in_fetch_script_and_inventory` が失敗する（task_description の再現手順が再現しなくなる）。
- [ ] AC2: `is_excluded_path` は `target` 始まりのディレクトリ構成要素を含むパスを除外し続け、`target` 始まりの通常ファイル名（最終構成要素）は除外しない。
- [ ] AC3: `embedded_font_names` は `build.rs:160` と同形のテキスト（`include_bytes!({abs_str:?})` を含む format 文字列）に対して空集合を返し、後続の無関係な文字列リテラルを拾わない。
- [ ] AC4: 非リテラル引数の呼び出しの後ろに本物のフォント埋め込みがある場合、本物の 1 件だけが抽出される（走査が打ち切られない）。
- [ ] AC5: 無変更のツリーに対し `scan_embedded_fonts(crate_root())` が `documented_embedded_fonts()` の 7 件と厳密一致し続ける。
- [ ] AC6: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test asset_manifest` が緑。既存 12 テストは無改変で通る。
- [ ] AC7: `Cargo.toml` の依存関係に差分が無い。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

| ID | 対象 | シナリオ |
|----|------|----------|
| TS1 | FR1, AC2 | `is_excluded_path(Path::new("src/target_resolver.rs"))` が false。`src/target.rs`（最終構成要素が完全一致 `target`）も false。 |
| TS2 | FR1, NFR3, AC2 | 既存 `walk_excludes_build_output_directories` の 4 パス（`target/debug/build/x/out.rs`、`target-host/...`、`target-win/...`、`target-anything-else/out.rs`）が引き続き true。`src/render/font/resolver.rs` は false のまま。 |
| TS3 | FR1, AC1 | `tempfile::tempdir` 上に `src/target_resolver.rs` を作り `include_bytes!("../assets/fonts/Fake-Regular.ttf")` を書いたとき、`scan_embedded_fonts(root)` が `Fake-Regular.ttf` を返す（現行実装では空集合になる）。併せて `target/x.rs` に同様の埋め込みを置いたケースでは空集合のままであることを確認する。 |
| TS4 | FR2, FR5, AC3 | `build.rs:160` と同形の合成テキスト（`include_bytes!({abs_str:?})` を含む format 文字列と、その後に続く `println!("cargo:rerun-if-changed=...")` 相当の文字列リテラル）に対し `embedded_font_names` が空集合を返す。 |
| TS5 | FR2, FR5, AC4 | 非リテラル引数の呼び出し（例: `include_bytes!(SOME_PATH_CONST)`）の直後に `include_bytes!("../assets/fonts/Real-Regular.ttf")` が続くテキストで、抽出結果が `Real-Regular.ttf` 1 件のみ。 |
| TS6 | FR3, AC5 | 既存 `exactly_the_documented_fonts_are_embedded` が無改変で通り、7 件の集合が変わらない。 |
| TS7 | FR6, AC6 | `--test asset_manifest` のスイート全体が緑（既存テスト + 追加テスト）。 |

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| ギャップ 1 | `is_excluded_path` が `target` 前方一致規則を最終構成要素にも適用している問題 |
| ギャップ 2 | `embedded_font_names` がマクロ呼び出しの境界を見ずに文字列リテラルを探索している問題 |
| `BUILD_OUTPUT_PREFIX` | `asset_manifest.rs` が持つビルド成果物ディレクトリ判定用の前方一致文字列 `"target"` |

## 14. 確認事項

### 14.1 確認済み事項

- [x] `include_bytes!` の走査対象とする区切り文字の形式: 丸括弧形式のみに保ち、境界検出だけを追加する（`requirement.include-bytes-delimiter-forms` = `parens_only`）
- [x] 括弧内に文字列リテラルが無い呼び出しの扱い: 括弧内の最初の文字列リテラルだけを見て、無ければその呼び出しは抽出せず次へ進む（`requirement.non-literal-macro-argument` = `skip_non_literal`）
- [x] デザインステップの要否: skip（`design-step.recommendation` = `decide_autonomously`）

### 14.2 未確認・保留事項

なし。全要件（FR1〜FR6、NFR1〜NFR6）が resolved。

### 14.3 前提事項

| ID | 前提 | 可逆 | 出所 |
|----|------|------|------|
| a1 | 比較・解析関数の純粋性（IMPLEMENTATION D5）と一方向判定（D6）は維持される前提の変更である。 | 可 | `src-tauri/tests/asset_manifest.rs` 冒頭ドキュメンテーションコメントおよび既存テスト |
| a2 | ビルド成果物ディレクトリの除外は既存テスト `walk_excludes_build_output_directories` に固定された不変条件として維持する。 | 可 | 既存テストによる固定 |
| a3 | 埋め込みフォント 7 件の集合は現行ツリーの事実であり、本変更で変えない。 | 可 | 既存テスト `exactly_the_documented_fonts_are_embedded` / `resolver.rs` |
| a4 | `tests/asset_manifest.rs` 自身の走査除外（`SELF_RELATIVE_PATH` 規則）は維持する。追加テストの合成テキストが実埋め込みと誤認されないために必要。 | 可 | 既存テスト `walk_excludes_its_own_source_file` |
| a5 | `include_bytes!` の走査対象は丸括弧形式のみに保ち、境界検出だけを追加する（角括弧・波括弧形式は対象外）。 | 可 | 回答 `requirement.include-bytes-delimiter-forms`（`parens_only`, batch-codex-consultation） |
| a6 | 括弧内の最初の文字列リテラルだけを見て、無ければその呼び出しは抽出せず次へ進む（ネストしたマクロの内部へは降りない）。 | 可 | 回答 `requirement.non-literal-macro-argument`（`skip_non_literal`, batch-codex-consultation） |
| a7 | 新規 dev-dependency は追加せず、既存の `tempfile = "3"` と std のみで一時ツリーのテストを書ける。 | 可 | `src-tauri/Cargo.toml:202-204` |
| a8 | `src-tauri/build.rs:160` は同型実例としての参照であり修正対象ではない（生成コード側の意図的な形）。 | 可 | task_description「該当箇所」欄および build.rs の生成コード構造 |

## 15. 参考資料

- `src-tauri/tests/asset_manifest.rs`: 対象のリグレッションガード（`is_excluded_path` :175-183、`embedded_font_names` :86-113、`collect_rust_source` :194-219）
- `src-tauri/src/render/font/resolver.rs`: 埋め込み 7 件（:21,27,34,39,50,58,64）
- `src-tauri/build.rs`: :160 の `include_bytes!({abs_str:?})`（ギャップ 2 の同型実例）
- `src-tauri/Cargo.toml`: :202-204 の dev-dependency `tempfile = "3"`
