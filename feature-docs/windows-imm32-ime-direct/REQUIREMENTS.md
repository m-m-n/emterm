---
title: "windows-imm32-ime-direct"
created_date: 2026-08-06
status: draft
---

# windows-imm32-ime-direct - 要件定義書

## 1. 概要

### 1.1 背景

Windows で CorvusSKK を使って文字を入力すると、アプリが「応答なし」状態でフリーズする。

原因はフルメモリのハングダンプ解析で確定している。

- winit-win32 0.31.0-beta.2 の `window.rs:1025` が `WindowState` の mutex を保持したまま `Imm*` 呼び出しを行う。
- TSF / CorvusSKK が同一スレッドで wndproc に再入する。
- 再入先の `event_loop.rs:121` が同じ mutex を再ロックし、永久にブロックする。

この診断は確定済みで、再診断は本タスクの対象外とする。

### 1.2 目的

- winit 自体を変更せずに、Windows + CorvusSKK での「応答なし」フリーズを解消する。
- X11 / Wayland および CLI 専用ビルドの現在の IME 挙動をそのまま維持する。

### 1.3 スコープ

対象は `src-tauri/src/ime/winit_bridge.rs` の `BridgeWindow` 実装。Windows 向けにカーソル領域通知経路を winit 経由から IMM32 直呼びへ切り替え、IME デタッチ (`set_ime_allowed(false)`) を変換終了まで遅延させる。

**スコープ外**（タスクで明示的に除外）:

- winit の fork / patch（`[patch.crates-io]` を含む）
- winit 本体への issue / PR 起票
- `Drop` 経路の `set_ime_allowed(false)` の遅延化。設定変更等でブリッジが変換中に差し替えられた場合に残る既知の穴であり、本タスクでは修正しない。
- 無関係な Wayland DnD の問題

## 2. ビジネス要件

### 2.1 ビジネス目標

- winit を変更することなく、Windows で CorvusSKK による文字入力時に発生する「応答なし」フリーズを解消する。
- X11 / Wayland および CLI 専用ビルドにおける現行の IME 挙動を維持する。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| Windows + CorvusSKK 利用者 | eMterm 上で CorvusSKK により日本語入力を行うユーザー。フリーズの影響を直接受ける |
| X11 / Wayland 利用者 | Linux 上で eMterm の IME 入力を行うユーザー。挙動が変わらないことが要件 |
| CLI 専用ビルド利用者 | `--no-default-features` ビルド（winit 非依存）の利用者 |

### 2.3 期待される効果

- Windows + CorvusSKK での変換確定の繰り返しでフリーズしなくなる。
- 変換中の Alt+Tab によるフォーカスアウトでフリーズしなくなる。
- 候補ウィンドウがカーソル位置に追従する状態が維持される。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 変換確定を繰り返す | Windows + CorvusSKK 利用者 | 高 |
| UC02 | 変換中に Alt+Tab でフォーカスアウトする | Windows + CorvusSKK 利用者 | 高 |
| UC03 | 候補ウィンドウがカーソルに追従する | Windows + CorvusSKK 利用者 | 中 |
| UC04 | X11 / Wayland で IME 入力を行う | X11 / Wayland 利用者 | 中 |

### 3.2 ユースケース詳細

#### UC01: 変換確定を繰り返す

**アクター**: Windows + CorvusSKK 利用者

**事前条件**:
- Windows 実機で eMterm が起動している
- CorvusSKK が IME として有効になっている

**基本フロー**:
1. ユーザーが文字を入力し変換を開始する
2. アプリはカーソル領域を IMM32 へ直接通知する
3. ユーザーが変換を確定する
4. 1〜3 を繰り返す

**事後条件**:
- アプリがフリーズしない

#### UC02: 変換中に Alt+Tab でフォーカスアウトする

**アクター**: Windows + CorvusSKK 利用者

**事前条件**:
- 変換（composition）が進行中である

**基本フロー**:
1. ユーザーが変換中に Alt+Tab を押してフォーカスを移す
2. アプリは変換が生きている間 `set_ime_allowed(false)` を送らない
3. `Ime::Disabled` を受信した後の `flush` でデタッチが送られる

**代替フロー**:
- デタッチ保留中にフォーカスインが発生した場合、`pending_allow` が last-writer-wins で上書きされ、デタッチはキャンセルされる

**事後条件**:
- アプリがフリーズしない

#### UC03: 候補ウィンドウがカーソルに追従する

**アクター**: Windows + CorvusSKK 利用者

**基本フロー**:
1. ユーザーがカーソル位置を移動させて変換を開始する
2. アプリが IMM32 へ composition window / candidate window の位置を通知する

**事後条件**:
- 候補ウィンドウがカーソル位置に追従して表示される

#### UC04: X11 / Wayland で IME 入力を行う

**アクター**: X11 / Wayland 利用者

**基本フロー**:
1. ユーザーが Linux 上で IME による入力・変換を行う

**事後条件**:
- 観測される IME 挙動が本変更の前後で変わらない

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | Windows のカーソル領域通知を winit 経由から外す | `#[cfg(windows)]` の `BridgeWindow` 実装が IMM32 を直接呼ぶ | 高 |
| FR2 | IMM32 呼び出し順序とウィンドウ形状を winit に一致させる | `ImmGetContext` から `ImmReleaseContext` までの呼び出しレシピ | 高 |
| FR3 | IME デタッチの遅延 | 変換中は `set_ime_allowed(false)` を送らない | 高 |
| FR4 | Enable は winit 経由のまま維持 | `set_ime_allowed(true)` は winit を通す | 高 |
| FR5 | 非 Windows 経路は不変 | X11 / Wayland は現行の winit 経由実装を維持 | 高 |
| FR6 | HWND 取得と依存関係 | `rwh_06::HasWindowHandle` からの HWND 取得と `windows-sys` の feature 追加 | 高 |

### 4.2 機能詳細

#### FR1: Windows のカーソル領域通知を winit 経由から外す

**説明**: Windows では `set_ime_cursor_area` が winit の `request_ime_update` を経由しない。`src-tauri/src/ime/winit_bridge.rs` の `#[cfg(windows)]` `BridgeWindow` 実装が IMM32 を直接呼ぶ。これにより呼び出しが winit の `Mutex<WindowState>` を通らなくなり、TSF による wndproc への再入がその mutex を正常に取得できる。

**入力**:
- カーソル領域: 物理ピクセルの `x`, `y`, `width`, `height`

**出力**:
- IMM32 への composition window / candidate window 位置設定

**ビジネスルール**:
- Windows 経路では winit の `request_ime_update` を使用しない

#### FR2: IMM32 呼び出し順序とウィンドウ形状を winit に一致させる

**説明**: 直接経路は次の順序で処理する。

1. `ImmGetContext`
2. 戻り値が null なら何もしない（no-op）
3. `ImmSetCompositionWindow`（`CFS_POINT`、`ptCurrentPos` = `(x, y + height)`）
4. `ImmSetCandidateWindow`（`CFS_EXCLUDE`、`ptCurrentPos` = `(x, y)`）
5. `ImmReleaseContext`

**入力**:
- `x`, `y`, `width`, `height`: 物理ピクセル（変換せずそのまま渡す）

**ビジネスルール**:
- `rcArea` は composition / candidate の両方で `(x, y, x + width, y + height)`
- 座標は物理ピクセルで、変換を行わない

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| IME コンテキストを取得できない | `ImmGetContext` が null を返す | 何もせずに戻る（no-op） |

#### FR3: IME デタッチの遅延

**説明**: 変換（composition）が生きている間は `set_ime_allowed(false)` を送らない。`Ime::Disabled` を受信した後の `flush` で送信する。`pending_allow` は last-writer-wins のままとし、デタッチ保留中にフォーカスインが発生した場合は上書きによりデタッチがキャンセルされる。

**ビジネスルール**:
- 変換中に `set_ime_allowed(false)` を送らない
- `Ime::Disabled` 受信後の `flush` で 1 回だけ送る
- `pending_allow` は last-writer-wins

#### FR4: Enable は winit 経由のまま維持

**説明**: `set_ime_allowed(true)`（`ImeRequest::Enable`）は winit を通し続ける。winit は `ime_capabilities.is_some()` を条件に WM_IME_* の処理をゲートしている（winit-win32 `event_loop.rs:1415/1428/1479`）ため、Enable をバイパスすると `WindowEvent::Ime` の配送がすべて止まる。

**ビジネスルール**:
- Enable は IMM32 直呼びの対象外

#### FR5: 非 Windows 経路は不変

**説明**: X11 / Wayland は現行の winit 経由 `BridgeWindow` 実装を維持する。これらのターゲットで観測される IME 挙動は変わらない。

#### FR6: HWND 取得と依存関係

**説明**: HWND は winit の `Window` から `rwh_06::HasWindowHandle` 経由で取得する。既存の `windows-sys` 直接依存（0.59、`src-tauri/Cargo.toml` で既に `Win32_Foundation` / `Win32_System_Console` を有効化済み）に `Win32_UI_Input_Ime` feature を追加する。

## 5. 非機能要件

### 5.1 パフォーマンス要件

本要件で定義された目標値はない。

### 5.2 セキュリティ要件

本要件で定義された事項はない。

### 5.3 可用性要件

| ID | 要件 |
|----|------|
| NFR3 | IMM32 呼び出しはイベントループスレッド上で実行される。`flush` は `about_to_wait` の内側で実行されるため、新たなスレッド機構を追加せずに IMM32 の呼び出しスレッド要件を満たす |

### 5.4 保守性要件

| ID | 要件 |
|----|------|
| NFR1 | winit を fork / patch しない（`[patch.crates-io]` はスコープ外）。ピン留めされた `winit = "=0.31.0-beta.2"` 依存はそのまま維持する |

### 5.5 互換性要件

| ID | 要件 |
|----|------|
| NFR2 | `cargo check --no-default-features`（CLI 専用、winit なし）が通り続ける。新規コードは `gui` feature と `#[cfg(windows)]` の内側に置く |

## 6. UI/UX要件

デザインステップはスキップされている。IME 配線（winit ブリッジ内部の IMM32 呼び出し経路）のバグ修正であり、視覚要素や UI サーフェスの追加・変更はない。ユーザーから見える結果はフリーズが起きないことと、既存の候補ウィンドウがカーソルに追従し続けることのみであるため、デザインステップで規定する対象がない。

## 7. データ要件

本要件で扱うデータモデルの追加・変更はない。

## 8. 外部連携

| システム名 | 連携方法 | データ |
|------------|----------|--------|
| Windows IMM32 | `ImmGetContext` / `ImmSetCompositionWindow` / `ImmSetCandidateWindow` / `ImmReleaseContext` の直接呼び出し | カーソル領域（物理ピクセル座標） |

## 9. 制約条件

### 9.1 技術的制約

- winit を fork / patch しない。`winit = "=0.31.0-beta.2"` のピン留めを維持する（NFR1）。
- `set_ime_allowed(true)` は winit 経由を維持する必要がある。バイパスすると `WindowEvent::Ime` の配送が止まる（FR4）。
- IMM32 の呼び出しはイベントループスレッド上で行う必要がある（NFR3）。
- 新規コードは `gui` feature と `#[cfg(windows)]` の内側に置く（NFR2）。

### 9.2 ビジネス上の制約

- Windows 実機での受け入れ基準（AC3〜AC5）は Windows + CorvusSKK の物理マシンでのみ検証可能であり、Linux 開発ホストや CI では実行できない。

### 9.3 スケジュール制約

本要件で定義された事項はない。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| `Drop` 経路の `set_ime_allowed(false)` が変換中に走り得る（設定変更等でブリッジが差し替えられた場合の既知の残存穴） | 中 | 本タスクではスコープ外として修正しない |
| AC3〜AC5 が自動検証できない | 中 | Windows + CorvusSKK 実機での手動検証ゲートを verify フェーズで計画する |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1: Windows で `set_ime_cursor_area` が IMM32 を直接呼び、winit の `request_ime_update` を経由しない（検証: コードレビュー + ユニットテスト（mock `BridgeWindow`）。任意のホストで自動実行可能）
- [ ] AC2: 変換が生きている間は `set_ime_allowed(false)` を送らず、`Ime::Disabled` 受信後の flush で送る（検証: ブリッジの pending 状態ロジックに対するユニットテスト。任意のホストで自動実行可能）
- [ ] AC3: Windows + CorvusSKK 実機で、変換確定を繰り返してもアプリがフリーズしない（検証: **手動・実機のみ**（Windows + CorvusSKK）。本 Linux ホストおよび CI では実行不可）
- [ ] AC4: 実機で、変換中の Alt+Tab によるフォーカスアウトでアプリがフリーズしない（検証: **手動・実機のみ**（Windows + CorvusSKK））
- [ ] AC5: 実機で、候補ウィンドウがカーソル位置に追従する（検証: **手動・実機のみ**（Windows + CorvusSKK））
- [ ] AC6: X11 / Wayland の IME 挙動が変わらない（検証: 既存 `winit_bridge` ユニットテストスイートがグリーンのまま + Linux ホストでの手動スポットチェック）
- [ ] AC7: `cargo check --no-default-features`（CLI 専用）が通る（検証: 自動 `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`）

### 11.2 KPI

本要件で定義された指標はない。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] ユニット（mock `BridgeWindow`、`winit_bridge.rs` の既存テストパターン）: 変換が開いている状態（`Ime::Enabled` 観測済み・`Ime::Disabled` 未受信）で `notify_focus(false)` + `flush` が `set_ime_allowed(false)` を配送しない。`Ime::Disabled` 到達後の次の `flush` でちょうど 1 回配送する。
- [ ] ユニット: デタッチ保留中に到着したフォーカスインが `pending_allow` を上書き（last-writer-wins）し、デタッチが一切送られない。
- [ ] ユニット: 既存の遅延 flush / 重複排除 / 順序のテストがグリーンのまま。記録・flush の契約は変更せず、変わるのは `BridgeWindow` の背後にある Windows シンクのみ。
- [ ] 自動ビルドゲート: `cargo test --lib`（src-tauri、`CARGO_TARGET_DIR=src-tauri/target`）、`cargo check --no-default-features`、および Windows コード経路がコンパイルされることの確認として `make win-build` / cargo xwin クロスチェック。
- [ ] 手動（Windows + CorvusSKK 実機）: 変換確定の繰り返しでフリーズしない。変換中の Alt+Tab でフリーズしない。候補ウィンドウがキャレットに追従する。
- [ ] 手動（Linux ホスト）: X11 / Wayland の変換ラウンドトリップが変わらない。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| IMM32 | Windows の Input Method Manager API |
| TSF | Text Services Framework |
| CorvusSKK | Windows 向けの SKK 系 IME |
| `BridgeWindow` | `set_ime_allowed` / `set_ime_cursor_area` を持つ、本機能の統合シーム（trait） |
| 遅延 flush アーキテクチャ | `pending_allow` / `pending_cursor_area` を保持し、`about_to_wait` から `flush` する既存構造 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 根本原因: フルメモリのハングダンプ解析で確定済み。winit-win32 0.31.0-beta.2 の `window.rs:1025` が `WindowState` mutex を `Imm*` 呼び出しをまたいで保持し、TSF / CorvusSKK が同一スレッドで wndproc に再入、`event_loop.rs:121` が再ロックして永久にブロックする。再診断はスコープ外。
- [x] 統合シーム: `BridgeWindow` trait（`set_ime_allowed` / `set_ime_cursor_area`）を統合シームとする。遅延 flush アーキテクチャ（`pending_allow` / `pending_cursor_area`、`about_to_wait` からの flush）は既存であり、そのまま維持する。
- [x] スコープ外（タスクで明示）: winit の fork / patch、winit 上流への issue / PR、`Drop` 経路の `set_ime_allowed(false)` の遅延化（ブリッジが変換中に差し替えられた場合の既知の残存穴）、無関係な Wayland DnD の問題。
- [x] Windows 実機基準（AC3〜AC5）: ユーザーが Windows + CorvusSKK 実機で手動検証する。Linux 開発ホストでは実行できない。これらは手動検証を伴う解決済み要件であり、TBD ではない。

### 14.2 未確認・保留事項

未確認・保留事項はない（全要件が `resolved`）。

## 15. 参考資料

- 根本原因のハングダンプ解析記録: プロジェクトローカルの `tmp/` 配下メモ（gitignored のため恒久参照先ではない）。解析結果の実体は本書 1.1 および SPEC.md 内に転記済み。
- winit-win32 0.31.0-beta.2 `window.rs:1025`（mutex 保持箇所）、`event_loop.rs:121`（再ロック箇所）、`event_loop.rs:1415/1428/1479`（`ime_capabilities.is_some()` による WM_IME_* ゲート）
