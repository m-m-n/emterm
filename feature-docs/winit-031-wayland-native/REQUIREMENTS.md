---
title: "winit 0.31 移行と Wayland native 起動"
created_date: 2026-07-22
status: draft
---

# winit 0.31 移行と Wayland native 起動 - 要件定義書

## 1. 概要

### 1.1 背景

- eMterm は Linux で `WAYLAND_DISPLAY` と `DISPLAY` が両方存在するとき X11 バックエンド（Xwayland）を強制している（`src-tauri/src/main.rs` の `build_event_loop()`）
- X11 強制の理由は winit 0.30 の Wayland バックエンドがファイル D&D（`DroppedFile` / `HoveredFile`）未実装で、SFTP アップロードの入口が死ぬため
- この X11 強制により、Xwayland クライアント（VLC / Avidemux / Catia 等）を Ctrl+Q で終了した際、X11 のフォーカス revert で eMterm に synthetic key press が届き、素の `q` が PTY に書かれるバグが発生する（`tmp/discussion-vlc-ctrl-q-stray-input.md` で経路確定済み）
- `EMTERM_BACKEND=wayland` で起動すると発生しないことを実機確認済み
- winit 0.31.0-beta.2 は Wayland のファイル D&D を実装済み（`winit-wayland/src/dnd.rs`）で、X11 強制の理由が解消する

### 1.2 目的

winit を 0.31.0-beta.2 へアップデートし、Linux でのデフォルト起動を Wayland native にすることで stray-`q` バグを根本解決する。ファイル D&D（SFTP アップロード入口）は Wayland native でも動作させる。

### 1.3 スコープ

- winit 依存を 0.30.9 から 0.31.0-beta.2 へ更新（クレート分割・API 大改編への追従を含む）
- `build_event_loop()` の X11 強制ロジックの撤去（デフォルトは winit の自動選択 = Wayland 優先）
- `EMTERM_BACKEND=wayland` / `EMTERM_BACKEND=x11` オーバーライドの維持
- ファイル D&D の新 API（`DragEntered` / `DragMoved` / `DragDropped` / `DragLeft`）への移行
- X11 バックエンド起動時の防御として synthetic key press を PTY に流さない処理の追加
- Windows ビルド・CLI-only ビルドの維持

## 2. ビジネス要件

### 2.1 目標

- Xwayland クライアント終了時に Claude Code の入力エリアへ `q` が漏れ入力されるバグの根絶
- Wayland native 化による X11/Xwayland 依存経路の解消

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| Linux (Wayland) ユーザー | GNOME 等の Wayland セッションで eMterm を使用する。今回の主対象 |
| Linux (X11) ユーザー | X11 セッション、または `EMTERM_BACKEND=x11` を指定するユーザー |
| Windows ユーザー | 動作維持のみ（挙動変更なし） |

### 2.3 期待される効果

- stray-`q` バグの根本解決（X11 世界のフォーカス revert 経路自体が消える）
- ファイル D&D が Wayland native で動作する

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | Wayland セッションでの通常起動 | Linux ユーザー | 高 |
| UC02 | ファイル D&D による SFTP アップロード | Linux ユーザー | 高 |
| UC03 | `EMTERM_BACKEND=x11` での起動 | Linux ユーザー | 中 |

### 3.2 ユースケース詳細

#### UC01: Wayland セッションでの通常起動

**アクター**: Linux (Wayland) ユーザー

**事前条件**:
- `WAYLAND_DISPLAY`（または `WAYLAND_SOCKET`）が設定されている

**基本フロー**:
1. ユーザーが `emterm` を起動する
2. winit が Wayland バックエンドを選択する
3. ターミナルが Wayland native で動作する
4. Xwayland クライアント（VLC 等）を Ctrl+Q で終了しても eMterm に `q` が入力されない

**代替フロー**:
- `EMTERM_BACKEND=x11` かつ `DISPLAY` あり → X11 バックエンドで起動する

**事後条件**:
- キー入力・IME・レンダリング・子 WebView が従来どおり動作する

#### UC02: ファイル D&D による SFTP アップロード

**アクター**: Linux ユーザー

**事前条件**:
- eMterm が Wayland native で起動している

**基本フロー**:
1. ユーザーがファイルマネージャ等からファイルを eMterm ウィンドウにドラッグする
2. ドロップされたファイルパスが従来の D&D 処理（SFTP アップロード入口）へ渡る

**事後条件**:
- 0.30 の X11 バックエンドでの D&D と同等に機能する

#### UC03: `EMTERM_BACKEND=x11` での起動

**アクター**: Linux ユーザー

**基本フロー**:
1. `EMTERM_BACKEND=x11 emterm` で起動する
2. X11 バックエンドで動作する
3. FocusIn 時の synthetic key press は PTY に書き込まれない

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | winit 0.31.0-beta.2 移行 | クレート分割・API 変更へ追従し全機能をビルド可能にする | 高 |
| F02 | デフォルト Wayland 起動 | X11 強制を撤去し winit の自動選択に任せる。`EMTERM_BACKEND` は維持 | 高 |
| F03 | ファイル D&D 新 API 移行 | `DragEntered` / `DragMoved` / `DragDropped` / `DragLeft` へ移行 | 高 |
| F04 | synthetic key press 防御 | X11 バックエンド時、synthetic な key press を PTY に流さない | 中 |

### 4.2 機能詳細

#### F01: winit 0.31.0-beta.2 移行

**説明**: `src-tauri/Cargo.toml` の winit 依存を 0.31.0-beta.2 に更新する。0.31 はクレートが `winit-core` / `winit-wayland` / `winit-x11` 等に分割されており、イベントループ構築・ウィンドウ生成・キーボード / IME / D&D イベント処理の API 変更に追従する。

**ビジネスルール**:
- 既存機能（キー入力、IME、レンダリング、子 WebView 連携、mux）の動作を維持する
- GUI feature gate（`gui` on/off）の構造を維持する

#### F02: デフォルト Wayland 起動

**説明**: `build_event_loop()` の「`WAYLAND_DISPLAY` + `DISPLAY` 両方あるとき X11 強制」ロジックを撤去する。

**ビジネスルール**:
- `EMTERM_BACKEND` 未設定（auto）: winit の自動選択（Wayland 優先）
- `EMTERM_BACKEND=wayland`: Wayland を使用
- `EMTERM_BACKEND=x11`: `DISPLAY` があれば X11 を使用

#### F03: ファイル D&D 新 API 移行

**説明**: 0.30 の `DroppedFile` / `HoveredFile` / `HoveredFileCancelled`（ファイル 1 個ずつ）を 0.31 の `DragEntered` / `DragMoved` / `DragDropped` / `DragLeft`（ファイルセット単位のリスト渡し）に置き換え、既存の SFTP アップロード入口へ接続する。

**エラーケース**:
| エラー | 条件 | 対応 |
|--------|------|------|
| 空のファイルリスト | ドロップ内容にファイルパスが含まれない | 従来の「ドロップ対象なし」と同じ扱い（無視） |

#### F04: synthetic key press 防御

**説明**: `WindowEvent::KeyboardInput` の `is_synthetic` フィールドを参照し、synthetic な press をキー入力処理（PTY 書き込み・キーバインド発火）に流さない。

**ビジネスルール**:
- X11 バックエンドで FocusIn 時に届く synthetic key press が対象
- Wayland では winit が synthetic press を生成しないため挙動は変わらない

## 5. 非機能要件

### 5.1 互換性要件

- Windows ビルド（`cargo xwin` による cross-check）がコンパイルできる
- CLI-only ビルド（`--no-default-features`）がコンパイルできる
- Linux X11 セッション（`DISPLAY` のみの環境）でも起動できる

### 5.2 保守性要件

- winit 0.31 が beta である旨と、安定版リリース時にバージョン更新する旨を Cargo.toml のコメントに記録する

## 9. 制約条件

### 9.1 技術的制約

- winit 0.31.0-beta.2 は beta 版（crates.io 安定版は 0.30.13）
- winit 0.31 はクレート分割（`winit-core` / `winit-wayland` / `winit-x11` 等）と API 大改編を含む
- wry（子 WebView）・egui・wgpu との連携が winit のバージョンに依存するため、関連クレートのバージョン整合を確認する必要がある

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| winit 0.31 API 大改編への追従範囲が広い | 高 | イベントループ・ウィンドウ生成・入力処理を段階的に移行し、ビルドとテストで確認する |
| egui / wgpu / wry との winit バージョン整合 | 高 | 依存クレートの 0.31 対応状況を計画フェーズで調査し、必要なら統合レイヤーを自前で追従する |
| beta 版の API が安定版で変わる可能性 | 中 | Cargo.toml でバージョンを固定し、安定版リリース時に追従する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] Wayland セッションで eMterm がデフォルトで Wayland native 起動する
- [ ] `QT_QPA_PLATFORM=xcb` の Qt アプリを Ctrl+Q で終了しても eMterm に `q` が入力されない
- [ ] Wayland native でファイル D&D → SFTP アップロード入口が動作する
- [ ] `EMTERM_BACKEND=x11` / `EMTERM_BACKEND=wayland` オーバーライドが機能する
- [ ] Rust テスト・CLI-only チェック・Windows cross-check が通る

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: Wayland セッションでの起動・キー入力・IME・レンダリング
- [ ] 正常系: ファイル D&D（単一 / 複数ファイル）
- [ ] 異常系: ファイルパスを含まないドロップ
- [ ] 正常系: `EMTERM_BACKEND=x11` での起動と synthetic key press の無視
- [ ] 回帰: Windows cross-check / CLI-only ビルド

## 14. 確認事項

### 14.1 確認済み事項

- [x] 実装アプローチ: winit 0.31.0-beta.2 へ移行して Wayland native 起動（beta 依存を main に入れることを了承済み）
- [x] X11 の扱い: X11 対応は残し、デフォルトのみ Wayland 化。`EMTERM_BACKEND=x11` オプトインは維持
- [x] synthetic 防御: X11 バックエンド起動時の防御として `is_synthetic` な press を PTY に流さない処理を入れる
- [x] D&D 検証: 自動テストは困難なため、verify フェーズの手動確認項目として Wayland 上での D&D → SFTP アップロードを検証する

### 14.2 未確認・保留事項

- なし

## 15. 参考資料

- 調査レポート: `tmp/discussion-vlc-ctrl-q-stray-input.md`
- 現行実装: `src-tauri/src/main.rs` `build_event_loop()`
