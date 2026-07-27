---
title: "emterm --version フラグ"
created_date: 2026-07-27
status: draft
---

# emterm --version フラグ - 要件定義書

## 1. 概要

### 1.1 背景

バイナリが自分のバージョンを名乗る経路が無い。`env!("CARGO_PKG_VERSION")` は
`src-tauri/src/settings_window/commands.rs:50` で設定パネルの IPC 応答にのみ
使われている。

### 1.2 目的

`emterm --version` で自身のバージョンを確認できるようにする。あわせて、
GitHub でタグが設定された際に、リリースを作る前にリポジトリの version を
タグに合わせて更新・プッシュする Action を用意する。

### 1.3 スコープ

- `emterm --version` フラグの実装（GUI ビルド / CLI-only ビルドの両方）
- `.github/workflows/release.yml` へのバージョン同期ジョブの追加

スコープ外:

- 「未知のフラグが CLI エラーにならず GUI 起動に落ちる」問題の修正
  （同じ dispatch 箇所を触るが、本タスクでは扱わない）
- `-V` 短縮エイリアス（要求に含まれない）

## 2. 承知のうえで受け入れる制約

タスク起票時に明記された、許容済みの制約:

- Cargo.toml の version は手書き文字列
- タグを打つ際に main のコードを修正してバージョンを合わせることは可能だが、
  その後修正してもタグを打つまで旧バージョンで固定される
- したがって表示されるバージョンはタグ間で実際のビルドを指さない

## 3. 機能要件

### 3.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | --version フラグ | `CARGO_PKG_VERSION` を stdout に出力して 0 で終了する | 高 |
| F02 | バージョン同期 Action | タグ push 時、リリース作成前に version を更新してプッシュする | 高 |

### 3.2 機能詳細

#### F01: --version フラグ

**説明**: `emterm --version` 実行時、`CARGO_PKG_VERSION` の値を stdout に
改行付きで出力し、終了コード 0 で終了する。

**入力**: コマンドライン引数 `--version`（第 1 引数）

**出力**: stdout に `CARGO_PKG_VERSION` の値 + 改行

**処理フロー**:

1. `main()` の引数 dispatch（サブコマンド判定と同じ箇所）で `--version` を判定する
2. `println!` でバージョンを出力する
3. `std::process::exit(0)` で終了する

**ビジネスルール**:

- GUI ビルド / CLI-only ビルド（`--no-default-features`）の両方で動作する
- GUI 起動・ロガー初期化・ウィンドウ生成を一切行わずに終了する

#### F02: バージョン同期 Action

**説明**: GitHub でタグ（`v*`）が push された際、リリースを作成する前に
`src-tauri/Cargo.toml` の `[package]` version をタグのバージョンに更新し、
デフォルトブランチにコミット・プッシュする。

**処理フロー**:

1. タグ push で release ワークフローが発火する
2. バージョン同期ジョブがデフォルトブランチを checkout する
3. `src-tauri/Cargo.toml` の version をタグの値（`v` prefix を除去）に書き換える
4. `src-tauri/Cargo.lock` の emterm パッケージエントリも同じ値に更新する
5. 差分があればコミットしてデフォルトブランチへプッシュする（差分が無ければ何もしない）
6. その後にリリース作成ジョブが実行される

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| バージョン一致 | Cargo.toml が既にタグと同じ version | コミット・プッシュせず正常終了する |
| プッシュ失敗 | ブランチ保護等で push が拒否される | ジョブを失敗させ、後続のリリース作成を止める |

## 4. 非機能要件

- `--version` は即座に終了する（イベントループ・PTY・設定読み込みを伴わない）
- ワークフローの他ジョブ（build-linux / build-cli / build-windows）の
  ビルド時 sed によるバージョン埋め込みは現状のまま維持する

## 5. 成功基準

- [ ] `emterm --version` が version 文字列を stdout に出力し 0 で終了する
- [ ] CLI-only ビルドでも同様に動作する
- [ ] タグ push 時、リリース作成前にバージョン更新コミットがデフォルトブランチに積まれる

## 6. 確認事項

### 6.1 決定事項（batch 実行のため自己決定 — SPEC.md の Assumptions 参照）

- [x] 出力形式: version 文字列のみ（バイナリ名 prefix は付けない）
- [x] `-V` エイリアス: 追加しない
- [x] Action の形態: 新規ワークフローではなく `release.yml` に `sync-version`
      ジョブを追加し、`create-release` をこれに依存させる
- [x] プッシュ先: デフォルトブランチ（main）。タグ自体は動かさない

### 6.2 未確認・保留事項

なし
