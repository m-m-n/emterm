---
title: "Windows CorvusSKK IME ハング修正"
created_date: 2026-07-30
status: draft
---

# Windows CorvusSKK IME ハング修正 - 要件定義書

## 1. 概要

### 1.1 背景

Windows で CorvusSKK を利用していると、変換モード中に `l` を押した時点で
eMterm 自体が応答なし（Not Responding）になる。ウィンドウを閉じるしかなく、
閉じる操作も OS による kill になる。変換中の誤入力が即アプリ死亡になり UX が
最悪である。Linux の SKK 対応（IME キー抑制ゲートの 2 状態化・Windows は
`ime_enabled` でゲート）後に症状が現れるようになったように見える。

### 1.2 目的

Windows + CorvusSKK の変換モード中の `l` 入力でアプリケーションが応答なしに
ならないようにする。

### 1.3 スコープ

- 対象: `src-tauri/src/ime/`（特に `winit_bridge.rs`）、`src-tauri/src/window_host.rs`、
  `src-tauri/src/app.rs` の IME 呼び出し経路
- 対象外: winit 本体の fork / vendoring、IME 以外の入力経路、UI 変更

## 2. 制約・前提

- ビルド環境（Linux）では Windows + CorvusSKK の症状を再現・検証できない。
  **理論上正しいと考えられる実装であれば受け入れる**（タスク受け入れ条件）。
- 少なくとも Linux（X11 / Wayland）は退行しないこと（タスク受け入れ条件）。
- winit は `=0.31.0-beta.2` に固定されている。winit のバージョン変更・fork は
  行わない。

## 3. 調査で確認した事実（create-spec フェーズ）

winit-win32 0.31.0-beta.2 のソースと eMterm の IME 経路を突き合わせて確認した:

1. eMterm は winit のイベント配送（wndproc 内）から同期的に
   `Window::request_ime_update` を呼んでいる。呼び出し箇所は
   `WinitImeBridge::notify_focus`（`WindowEvent::Focused` ハンドラ内）と
   `notify_cursor_rect`（`RedrawRequested`＝WM_PAINT 配送中）。
2. winit-win32 の `request_ime_update` はイベントループスレッドから呼ばれると
   クロージャを**インライン実行**し、`ImmAssociateContextEx` /
   `ImmSetCompositionWindow` / `ImmSetCandidateWindow` を直接呼ぶ
   （`winit-win32/src/window.rs`）。
3. これらの Imm* API は IME（CorvusSKK は TSF テキストサービス。CUAS 経由で
   IMM32 メッセージに変換される）との**同期メッセージ交換**を伴う。IME 側が
   eMterm のウィンドウへ同期送信中（SendMessage でブロック中）に eMterm 側が
   Imm* を同期発行すると、相互待ち（AB-BA デッドロック）になり得る。wndproc が
   返らなくなるため、Windows はウィンドウを「応答なし」と判定する — 報告症状と
   一致する。
4. SKK 系 IME はモード切替キー `l`（変換モード終了 → ASCII モード）で
   合成の終了・候補ウィンドウ破棄・コンテキスト再関連付けが連鎖するため、
   3. の競合が起きる窓が最も広い。
5. winit-win32 側は `window_state` ロックをイベント配送前に解放しており、
   winit 内部だけで閉じるデッドロックは確認できなかった。eMterm 側から
   wndproc 配送中に Imm* を発行する経路が、eMterm 側で修正可能な唯一の
   同期ブロッキング経路である。

根因を実機で確定できないため、上記 3.-4. を最有力仮説とし、これを塞ぐ
「理論上正しい」防御実装を行う（SPEC.md の Assumptions に記録）。

## 4. 機能要件

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | IME 要求の遅延発行 | winit イベント配送中に `request_ime_update` を同期発行せず、キューに積んで `about_to_wait`（イベント配送外）でフラッシュする | 高 |
| F02 | IME ライフサイクル異常の診断ログ | 合成ライフサイクルの異常遷移を warn レベルで（レート制限付き）記録し、実機からのフィールドレポートで修正の効果を確認可能にする | 中 |
| F03 | Linux 非退行 | 非 Windows のゲート述語・イベント変換・呼び出し順序・dedup の観測可能な振る舞いを変えない | 高 |

## 5. 非機能要件

- 新規依存クレーマを追加しない。
- 入力レイテンシ: フラッシュは同一イベントループターン内（`about_to_wait`）で
  行い、IME 候補ウィンドウ位置の更新に知覚可能な遅延を生じさせない。
- すべての新規テストは Linux ホスト上で実行可能であること（モック /
  プラットフォーム引数化）。

## 6. 成功基準

### 6.1 受け入れ基準

- [ ] winit イベント配送中（`window_event` ハンドラ内）から Imm* に到達する
  同期呼び出し経路が存在しない（コード上で証明可能）
- [ ] 遅延発行後も呼び出し順序・dedup・Drop 時のデタッチが保たれることを
  ユニットテストで検証済み
- [ ] 既存 IME ユニットテストが（呼び出し口の機械的な追随を除き）無変更で
  パスする
- [ ] `cargo test --lib` / `cargo check` が通る

## 7. 確認事項

### 7.1 決定事項（batch モード: Codex 利用不可のため Claude 単独で決定）

- [x] 修正方針: eMterm 側での遅延発行（winit fork はしない）
- [x] design ステップ: スキップ（UI 変更なし）
- [x] feature 名: `windows-skk-ime-hang`

### 7.2 未確認・保留事項

- [ ] 実機（Windows + CorvusSKK）での修正効果の確認 — ビルド環境で不可能な
  ため、リリース後のフィールド確認に委ねる（F02 のログが確認手段）

## 8. 参考資料

- タスク: https://www.notion.so/3ad3509ec8ee80a5b31fcdd8f9a87bb4
- winit-win32 0.31.0-beta.2 ソース（`~/.cargo/registry` 内 `window.rs` /
  `event_loop.rs` / `ime.rs` / `keyboard.rs`）
- `src-tauri/src/ime/winit_bridge.rs` モジュールドキュメント（2 状態ゲートの
  経緯）
