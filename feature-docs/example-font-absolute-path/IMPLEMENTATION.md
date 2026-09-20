# Implementation Plan: example-font-absolute-path

## Overview

`src-tauri/examples/` の 3 つの font probe から開発者マシン固有の絶対パスと
`native-poc/` 時代の実行手順コメントを取り除き、同じ混入を機械的に検出する
ソース走査テストを `src-tauri/tests/` に追加する。

## Technology Stack

- **Language**: Rust — 既存の `src-tauri` クレート内で完結する。
- **Test runner**: cargo の統合テスト (`src-tauri/tests/` 配下の別コンパイル単位)。
  既存の `cli_subcommands.rs` / `mux_throughput.rs` / `mux_hot_upgrade.rs` と同じ扱い (A5)。
- **New dependencies**: なし (NFR3)。新規依存が無いため、`project.license: MIT` に対して
  ライセンス互換性を照合すべき依存は本フィーチャーに存在しない。

## Layer Structure

本フィーチャーはアーキテクチャを変更しない。変更は次の 2 層に閉じる。

| 層 | 対象 | 許される変更 |
|----|------|--------------|
| 診断 probe 層 (`src-tauri/examples/`) | 既存 3 ファイル | フォント取得元の指定とファイル先頭コメントのみ |
| 統合テスト層 (`src-tauri/tests/`) | 新規 1 ファイル | probe 層のソースをテキストとして読む検証のみ |

依存方向は 統合テスト層 → 診断 probe 層 の一方向で、かつ「ソースをテキストとして読む」
関係に限る。コンパイル時依存・リンク依存・実行依存を新たに作らない。
本体 (`src-tauri/src/`) へはどちらの層からも変更を加えない (FR6)。

## Shared Components

本フィーチャーはタスクを 1 本 (task0001) に閉じたため、タスクをまたいで共有する
コンポーネントは存在しない。

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| (なし) | — | — | — |

## Conventions

- **フォント取得元の解決規約**: probe がフォントファイルを読むときは、コンパイル時に
  crate のマニフェストディレクトリを指す環境変数 `CARGO_MANIFEST_DIR` を起点にし、
  そこへ `assets/fonts/<ファイル名>` を**パス結合 API** で連結して実行時に解決する。
  パス区切り文字を文字列連結や書式化で直書きしない (NFR2)。絶対パスのリテラルを新たに書かない。
  ファイル名は `scripts/fetch-fonts.sh` が配置する名前と一致させる (A1)。
- **実行手順コメントの表記規約**: 実行手順はプロジェクトルートから実行する現行形で書く。
  `CARGO_TARGET_DIR=src-tauri/target` と `--manifest-path src-tauri/Cargo.toml` を用い、
  `native-poc/` の語を残さない (`.claude/rules/core-build-location.md`)。
  コメントの更新は「既にコメントを持つファイル」に限り、コメントを持たないファイルへ新設しない (FR2)。
- **新規テストのファイル名**: `src-tauri/tests/examples_source_hygiene.rs`。
  テスト選択子は `--test examples_source_hygiene`。VERIFICATION.md の TS1 / TS2 の
  実行コマンドはこの名前に依存する。
- **走査スコープ**: 走査対象は `src-tauri/examples/` 配下の Rust ソースのみ。判定に使う
  文字列そのものを保持する走査テスト自身 (`src-tauri/tests/` 配下) は対象に含めない。
- **エラー方針**: probe はフォントを取得できなければ即座に落ちる現在の挙動を維持する。
  失敗時の扱いと診断出力の書式を変えない (NFR4)。

## Cross-task Design Decisions

### D1: タスクを分割せず 1 本にする

走査テスト (FR5) は examples の内容が是正されて初めて緑になる。タスクは互いに順序を
持たず独立した worktree で並列に実装されるため、走査テストを別タスクへ切り出すと、
そのタスクの worktree では examples が未修正のままテストが赤になり、
「テスト通過 = タスク完了」が成立しない。したがって examples の是正 (FR1-FR4) と
走査テストの追加 (FR5) は同一タスク task0001 に置く。

### D2: 走査テストのフォント非依存性は「既定 feature を外した実行」で確認する

`gui` feature が default-on で本体がフォントをコンパイル時に埋め込むため、フォント未取得の
チェックアウトでは既定 feature でのテストビルド自体が失敗しうる (A7)。走査テストの
アサーション自体はフォント資産に依存しない (NFR1) が、その非依存性を実際に確認する場面では
既定 feature を外した実行が必要になる。VERIFICATION.md の NFR1 検証はこの形を採る。

### D3: 本体側のフォント参照方式には寄せない

本体 `src-tauri/src/render/font/resolver.rs` のコンパイル時埋め込みは正しい参照方法であり
変更対象外 (FR6)。probe を同じ埋め込み方式へ寄せる案は採らない。probe の目的は外部フォントを
差し替えて観察することにあり、埋め込みはその診断用途を狭めるため。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 走査テストが自身の判定文字列を検出して常に赤になる | Medium | High | Conventions の走査スコープで対象を `src-tauri/examples/` 配下に限定する |
| コメント書き換えのついでに probe の診断出力が変わる | Low | Medium | 変更をファイル先頭コメントとフォント取得元に限定し、出力不変を受け入れ基準に置く (NFR4) |
| フォント未取得環境で走査テストの緑を確認できない | Medium | Low | D2 の既定 feature を外した実行を VERIFICATION.md に明記する |
| examples に新ファイルが増えたとき規約が守られず再混入する | Low | Medium | 走査テストはディレクトリを列挙する形にし、新規ファイルも自動的に対象化する |
| 走査対象の解決に失敗して 0 件のまま緑になる | Low | High | 走査件数が 0 のときは失敗させる（受け入れ基準に含める） |

## Open Questions

- [ ] なし (FR1-FR6 / NFR1-NFR4 はすべて confirmed、TBD 要件なし)
