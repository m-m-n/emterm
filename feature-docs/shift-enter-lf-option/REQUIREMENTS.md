---
title: "Shift+Enter LF 送信オプションの追加と kitty_csi_u の非表示化"
created_date: 2026-07-19
status: draft
---

# Shift+Enter LF 送信オプションの追加と kitty_csi_u の非表示化 - 要件定義書

## 1. 概要

### 1.1 背景

`shift_enter_behavior` (shift-enter-behavior feature で導入) の
`kitty_csi_u` は、ネゴシエーション無しの無条件送信であるため、CSI u を
解釈しないアプリケーションでは余分な文字が入力される。kitty プロトコル
実装端末 (alacritty / wezterm) の実測では、Shift+Enter を LF (0x0a) に
マップすることで「シェルで余分な文字なし・Claude Code で改行」が両立
されている。

### 1.2 目的

`shift_enter_behavior` に LF 送信の選択肢を追加し、`kitty_csi_u` を
設定 UI から選択できないようにする (実装は温存する)。

### 1.3 スコープ

- `lf` 値の追加 (Rust enum / app_settings schema / TS union / 挙動)
- 設定 UI の選択肢入れ替えと隠し値の表示ルール
- 対象外: `kitty_csi_u` の削除 (variant・パース・送信ロジックは残す)、
  kitty プロトコルの本実装

## 2. 機能要件

### 2.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | `lf` 値の追加 | Shift+Enter で `0x0a` (1バイト) を送信する | 高 |
| F02 | UI 選択肢の入れ替え | セレクトは `alt_enter` / `none` / `lf` を提示。`kitty_csi_u` は提示しない | 高 |
| F03 | 隠し値の維持 | settings.json の `kitty_csi_u` はパース・挙動とも従来通り。現在値が `kitty_csi_u` のときだけセレクトに表示する | 高 |
| F04 | ロケール追加 | `lf` のラベル・説明文 (ja/en) | 中 |

### 2.2 機能詳細

#### F01: `lf` 値の追加

- wire 値: `lf`
- 対象は修飾キーが Shift のみの Enter 押下 (従来と同じ判定)
- 送信バイト列は `0x0a` の1バイトで、mux 接続時 (PosixPty) / ホスト PTY
  (HostPty) のどちらでも同一とする

#### F02: UI 選択肢の入れ替え

- セレクトの提示順: `alt_enter` (デフォルト) / `none` / `lf`

#### F03: 隠し値の維持

- `kitty_csi_u` の enum variant・デシリアライズ・送信ロジックは変更しない
- 現在値が `kitty_csi_u` の場合のみ、セレクトに既存ラベルで表示する
  (別の値に変更したら以後は提示されない)

#### F04: ロケール追加

- ja: ラベル「改行 (LF) として送信」、説明「Claude Code では改行、
  シェルでは Enter と同じ扱いになります」の方向で既存文体に合わせる
- en: ラベル「Send as newline (LF)」、説明は ja と対応する内容

## 3. 非機能要件

- デフォルト値は `alt_enter` のまま変更しない
- 既存の移行規則 (旧 bool キー・null 優先度) は変更しない
- `none` / `alt_enter` / `kitty_csi_u` の送信挙動は変更しない

## 4. 成功基準

### 4.1 受け入れ基準

- [ ] `lf` 選択時、Shift+Enter で `0x0a` が PTY に送られる
- [ ] セレクトに `kitty_csi_u` が現れない (現在値であるときを除く)
- [ ] settings.json の `kitty_csi_u` が従来通り動作する
- [ ] `lf` が設定パネルから保存・再表示できる

## 5. テストシナリオ

- [ ] 正常系: `lf` のバイト列生成 (両 EncodeTarget)
- [ ] 正常系: `lf` のデシリアライズ・保存境界の round-trip
- [ ] 正常系: 現在値による選択肢の出し分け (UI テスト)
- [ ] 回帰: 既存3値の挙動・移行規則が不変

## 6. 確認事項

### 6.1 確認済み事項

- [x] `kitty_csi_u` は削除せず隠し値として維持 (挙動・パースそのまま)
- [x] UI は現在値が `kitty_csi_u` のときだけ選択肢に表示
- [x] `lf` の UI 文言 (ja/en) は上記の方向で確定
- [x] kitty プロトコル本実装時に `kitty_csi_u` を UI に復帰させる想定

### 6.2 未確認・保留事項

- なし

## 7. 参考資料

- 議論レポート: tmp/discussion-shift-enter-kitty-protocol.md (追加調査・追加決定)
- 先行 feature: feature-docs/shift-enter-behavior/
