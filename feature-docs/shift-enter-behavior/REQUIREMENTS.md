---
title: "Shift+Enter 挙動の3値設定化"
created_date: 2026-07-19
status: draft
---

# Shift+Enter 挙動の3値設定化 - 要件定義書

## 1. 概要

### 1.1 背景

現状、Shift+Enter の扱いは bool 設定 `shift_enter_as_alt_enter`
(デフォルト true) のみで、「Alt+Enter に置換する / しない」の2択である。

### 1.2 目的

Shift+Enter 押下時に PTY へ送るバイト列を、3つの挙動から選択できるようにする。

### 1.3 スコープ

- Rust 側の設定スキーマ・キー書き換えロジック
- 設定パネル (子 WebView) の UI と i18n
- 既存 bool 設定からの移行
- 対象外: kitty キーボードプロトコルの本実装 (ネゴシエーション・モードスタック・
  全キーエンコード)

## 2. 機能要件

### 2.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | 3値設定 `shift_enter_behavior` | `none` / `alt_enter` / `kitty_csi_u` の enum 設定。デフォルト `alt_enter` | 高 |
| F02 | 挙動の実装 | 選択値に応じて Shift+Enter のバイト列を切り替える | 高 |
| F03 | 旧設定からの移行 | `shift_enter_as_alt_enter` を enum に読み替える | 高 |
| F04 | 設定 UI | 既存トグルをセレクトに置き換える | 中 |

### 2.2 機能詳細

#### F01: 3値設定 `shift_enter_behavior`

- 設定キー: `shift_enter_behavior`
- 値: `none` / `alt_enter` / `kitty_csi_u`
- デフォルト: `alt_enter`
- 既存の bool 設定 `shift_enter_as_alt_enter` は廃止する

#### F02: 挙動の実装

対象は修飾キーが Shift のみの Enter 押下 (Ctrl / Alt 併用時は対象外)。

| 値 | 挙動 |
|----|------|
| `none` | 書き換えなし。素の Enter と同じバイト列 (`\r`) を送る |
| `alt_enter` | Alt+Enter として送る (現行の true と同じ) |
| `kitty_csi_u` | `ESC [ 1 3 ; 2 u` (7 バイト) を送る。ネゴシエーションは行わず無条件送信 |

- `kitty_csi_u` のバイト列は mux 接続時 (PosixPty) / ホスト PTY
  (HostPty) のどちらでも同一とする

#### F03: 旧設定からの移行

- settings.json に `shift_enter_behavior` が無く `shift_enter_as_alt_enter`
  がある場合: `true` → `alt_enter`、`false` → `none` として読み込む
- `shift_enter_behavior` がある場合はそちらを優先する

#### F04: 設定 UI

- 設定パネルの Terminal Behavior セクションにある既存トグルを
  3択セレクトに置き換える
- ラベル・説明文は ja / en の locale に追加する

## 3. 非機能要件

- Shift 以外の修飾キーを含む Enter (Ctrl+Enter、Alt+Enter 等) の挙動は
  変更しない
- 検索バーの Shift+Enter (前方マッチ移動) など、PTY へ転送されない
  UI レイヤーのキー処理は影響を受けない

## 4. 成功基準

### 4.1 受け入れ基準

- [ ] 3値それぞれで規定のバイト列が PTY に送られる
- [ ] デフォルトは `alt_enter` で現行挙動と一致する
- [ ] 旧設定 `shift_enter_as_alt_enter: false` の settings.json が
      `none` として読み込まれる
- [ ] 設定パネルで3値を切り替えられ、保存・反映される

## 5. テストシナリオ

- [ ] 正常系: 各 enum 値でのバイト列生成 (ユニットテスト)
- [ ] 正常系: 設定デシリアライズ (新キー / 旧キー true / 旧キー false /
      両方あり / どちらも無し / null)
- [ ] 境界値: Ctrl+Enter / Alt+Enter / Ctrl+Shift+Enter は書き換え対象外
- [ ] UI: セレクトの表示・保存 (typecheck / bun test)

## 6. 確認事項

### 6.1 確認済み事項

- [x] 「Kitty 拡張」の実装方式: ネゴシエーション無しの無条件送信で確定
- [x] デフォルト値: `alt_enter` (現行互換)
- [x] 旧 bool 設定: enum に置換し、移行処理を入れる
- [x] 命名: キー `shift_enter_behavior`、値 `none` / `alt_enter` / `kitty_csi_u`

### 6.2 未確認・保留事項

- なし

## 7. 参考資料

- 議論レポート: tmp/discussion-shift-enter-kitty-protocol.md
