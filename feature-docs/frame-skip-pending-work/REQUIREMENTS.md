---
title: "frame-skip-pending-work"
created_date: 2026-08-12
status: draft
---

# frame-skip-pending-work - 要件定義書

## 1. 概要

### 1.1 背景

em-review (2026-08-11, finding `b5f2cce1822ab271`, resolution: deferred) で自己ロックが報告された。
`App::toast_pending()` は既に生成済みのトーストしか見ないが、`overlay_work`・イベントループの
redraw ペーシング・`next_toast_deadline()` のすべてが `toast_pending()` に依存している。
このため、未処理の SFTP progress イベントや restart フラグは、アイドルフレーム上では
最初のトーストを生成できない。

### 1.2 目的

トースト生成前の pending work（未 drain の SFTP チャネルイベント、restart-required フラグ）を
frame-skip ゲートの `overlay_work` 述語に含め、アイドルタブで最初のトースト表示が遅延しないようにする。

### 1.3 スコープ

対象は `src-tauri/src/` 配下の frame-skip ゲート述語まわりのバックエンド／ランタイムロジック。

対象外（タスク記述による）:

- トースト UI のデザイン・表示時間の変更
- SFTP アップロード機構、チャネルレイアウトの変更
- `should_skip_frame` の他の項（dirty / status bar / egui input）

## 2. ビジネス要件

### 2.1 ビジネス目標

- トースト生成前の pending work（未 drain の SFTP チャネルイベント、restart-required フラグ）を
  frame-skip ゲートの `overlay_work` 述語に含め、アイドルタブでの初回トースト表示を遅延させない。
- em-review (2026-08-11, finding `b5f2cce1822ab271`, resolution: deferred) で報告された自己ロックを解消する。
  `App::toast_pending()` は既に生成済みのトーストしか見ないにもかかわらず、`overlay_work`・
  イベントループの redraw ペーシング・`next_toast_deadline()` がすべてこれに依存しているため、
  pending の SFTP progress イベントや restart フラグはアイドルフレーム上で初回トーストを生成できない。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm 利用者 | アイドル状態のタブで SFTP アップロードを開始し、進捗トーストを見るユーザー |
| eMterm 利用者 | バイナリ入れ替え後の self-spawn 失敗により restart トーストを受け取るユーザー |

### 2.3 期待される効果

- アイドルタブ（カーソル点滅無効、またはウィンドウ非フォーカス）でも SFTP 進捗トーストが速やかに表示される。
- アイドル中でも restart トーストが速やかに arm・表示される。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | アイドルタブでの SFTP アップロード進捗トースト表示 | eMterm 利用者 | 高 |
| UC02 | アイドル中の restart トースト arm・表示 | eMterm 利用者 | 高 |

### 3.2 ユースケース詳細

#### UC01: アイドルタブでの SFTP アップロード進捗トースト表示

**アクター**: eMterm 利用者

**事前条件**:

- タブがアイドル状態（カーソル点滅が無効、またはウィンドウが非フォーカス）
- 表示中のトーストが存在しない

**基本フロー**:

1. ユーザーが SFTP アップロードを開始する
2. `send_progress` が progress イベントを送出し `wake()` を呼ぶ
3. frame-skip ゲートが pending work を検知しフレームを破棄しない
4. 進捗トーストが速やかに表示される

**事後条件**:

- 最初の progress イベント到着後、速やかに進捗トーストが表示されている

#### UC02: アイドル中の restart トースト arm・表示

**アクター**: eMterm 利用者

**事前条件**:

- アプリがアイドル状態
- バイナリ入れ替えにより self-spawn 失敗が発生している

**基本フロー**:

1. `note_spawn_failure` が `RESTART_REQUIRED` を立て `wake()` を呼ぶ
2. frame-skip ゲートが restart フラグの立ち上がりを非消費で検知しフレームを破棄しない
3. restart トーストが arm され表示される

**事後条件**:

- アイドル中でも restart トーストが速やかに表示されている
- `restart_required()` の消費セマンティクス（swap-reset、トーストを 1 回だけ arm）は変わらない

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 状態 |
|----|--------|------|------|
| FR1 | self_exec への非消費 restart peek 追加 | `restart_pending()` を追加する | resolved |
| FR2 | `App::frame_work_pending` 述語の追加 | pending work を非消費で判定する | resolved |
| FR3 | `overlay_work` の新述語利用 | `toast_pending()` の項を差し替える | resolved |
| FR4 | event_loop の redraw ペーシングの新述語利用 | `toast_redraw_due` の読み取りを差し替える | resolved |
| FR5 | `next_toast_deadline` の述語選択 | 現状維持か新述語かを設計時に決める | tbd |
| FR6 | 既知の制約ドキュメントの更新 | `pump_toasts` の doc comment を更新する | resolved |
| FR7 | 新述語のユニットテスト追加 | チャネル非空・restart フラグ設定時の true を検証する | resolved |

### 4.2 機能詳細

#### FR1: self_exec への非消費 restart peek 追加

**説明**: `src-tauri/src/self_exec.rs` に、`RESTART_REQUIRED` を `load()` で読み取りリセットしない
非消費 peek `restart_pending()` を追加する。既存の swap 消費型 `restart_required()` は消費
セマンティクスを変更しない（タスク記述の制約。Phase 7 batch 4 E-1 option a に一致）。

**ビジネスルール**:

- `restart_required()` の消費セマンティクスは不変。

#### FR2: `App::frame_work_pending` 述語の追加

**説明**: `src-tauri/src/app/mod.rs` に
`App::frame_work_pending(&self) -> bool` を追加し、
`toast_pending() || !sftp_progress_rx.is_empty() || !sftp_result_rx.is_empty() || crate::self_exec::restart_pending()`
と等価にする。SFTP レシーバは `crossbeam_channel::Receiver`
（`src-tauri/src/sftp/service.rs:58-60` の `ProgressReceiver` / `ResultReceiver`）であり、
`is_empty()` は非破壊チェックとなる。

**ビジネスルール**:

- 述語自体は何も消費しない（drain しない、swap しない）。

#### FR3: `overlay_work` の新述語利用

**説明**: `src-tauri/src/window_host/render_surface.rs` の `overlay_work` 式（現状 291 行付近）にある
`app.toast_pending()` の項を新述語 `frame_work_pending()` に置き換え、トースト生成前の pending work が
存在する間は `should_skip_frame` が早期 return しないようにする。

#### FR4: event_loop の redraw ペーシングの新述語利用

**説明**: `src-tauri/src/window_host/event_loop.rs` の `toast_redraw_due` に供給される
`self.app.toast_pending()` の読み取り（現状 645 行付近）を新述語に置き換え、トースト生成前の
pending work が存在する間は redraw ペーシングがフレームを流し続けるようにする。

#### FR5: `next_toast_deadline` の述語選択

**説明**: `App::next_toast_deadline()`（`src-tauri/src/app/mod.rs:932`）は、`toast_pending()` を
使い続けても、新述語に移行してもよい。

**状態**: tbd

**TBD 理由**: タスク記述がこの選択を明示的に設計時へ委ねている。ユーザー向けの質問ではなく、
ワークフローの plan ステップで決める。

#### FR6: 既知の制約ドキュメントの更新

**説明**: `App::pump_toasts` の doc comment にある既知の制約の段落
（`src-tauri/src/app/mod.rs:910-916`、"a pending SFTP event with no toast up yet ... relies on another
redraw trigger" の記述）を、新しい実装に合わせて更新する。

#### FR7: 新述語のユニットテスト追加

**説明**: 新述語が (a) SFTP チャネルが非空のとき、(b) restart フラグが設定されているときに true を返すことを
検証するユニットテストを追加する。プロジェクトの inline `#[cfg(test)] mod tests` 慣習と
`<subject>_<scenario>_<expected>` 命名に従う。

## 5. 非機能要件

| ID | 内容 |
|----|------|
| NFR1 | 既存の `restart_required()` の消費セマンティクス（swap-reset、トーストを 1 回だけ arm）を変更しない |
| NFR2 | 3 つのチェック構成すべてで警告ゼロ: Linux GUI `cargo check`、`cargo check --no-default-features`、Windows ターゲット向け `cargo xwin check --tests` |
| NFR3 | 述語チェックは、イベントループの 1 ターンごと／レンダー判定ごとに実行しても十分安価であること（atomic load + チャネル `is_empty()`、App が既に保持している以上のロックを取らない） |

## 6. UI/UX要件

トースト UI のデザイン・表示時間の変更はタスク記述により対象外。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 既存の `restart_required()` は swap 消費型のまま維持する（タスク記述の制約、Phase 7 batch 4 E-1 option a）。
- 述語は非消費であること（drain・swap を行わない）。
- Linux + Windows の GUI 機能のコードパスである。`self_exec` の restart 検知はモジュール doc により Linux 限定で、
  新しい peek はモジュール既存の cfg 構造に従う。

### 9.2 スコープ上の制約

- トースト UI のデザイン・表示時間の変更、SFTP アップロード機構・チャネルレイアウトの変更、
  および `should_skip_frame` の他の項（dirty / status bar / egui input）は対象外。

## 10. 想定される課題とリスク

| 課題 | 対応策 |
|------|--------|
| 非消費 peek の追加により、既存の消費型 `restart_required()` のセマンティクスが崩れる | `restart_pending()` は `load()` のみで、`restart_required()` は変更しない（FR1 / NFR1）。ユニットテストで消費セマンティクス不変を検証する |
| 述語をイベントループ毎ターン評価するコスト | atomic load + チャネル `is_empty()` に限定する（NFR3） |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1: アイドルタブ（カーソル点滅無効、またはウィンドウ非フォーカス）で SFTP アップロードを開始すると、
      最初の progress イベント到着後すみやかに進捗トーストが表示される
- [ ] AC2: バイナリ入れ替えによる self-spawn 失敗の後、アプリがアイドル状態でも restart トーストが
      すみやかに arm され表示される（Phase 7 E-1）
- [ ] AC3: `App::pump_toasts` の doc の既知の制約テキストが新しい実装を反映して更新されている
- [ ] AC4: 新述語のユニットテストが通る — SFTP チャネル非空で true、restart フラグ設定で true、それぞれ独立に成立する
- [ ] AC5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が完走し、
      3 つのチェック構成（Linux GUI、`--no-default-features`、`cargo xwin check --tests`）すべてが警告ゼロで完了する

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: 新述語は、トーストなし・チャネル空・restart フラグクリアのとき false を返す
- [ ] 正常系: 新述語は、SFTP progress チャネルにイベントが入っていてトースト未生成のとき true を返す
- [ ] 正常系: 新述語は、SFTP result チャネルにイベントが入っているとき true を返す
- [ ] 境界値: 新述語は、`RESTART_REQUIRED` が設定されているとき true を返し、かつその確認でフラグをクリアしない
      （非消費 peek）ため、後続の `restart_required()` が引き続き true を返す
- [ ] 境界値: `restart_required()` の消費セマンティクスは不変 — 1 回目 true、2 回目 false
- [ ] 手動（E2E 基盤は存在しない）: AC1 のアイドルタブ SFTP アップロードと AC2 の restart トースト arm は
      ユーザーによる手動確認。`test/README.md` の「エンドツーエンドの挙動は手動で検証する」という記述と整合する

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| frame-skip ゲート | `should_skip_frame` によるフレーム破棄判定 |
| pending work | トースト生成前の未処理作業（未 drain の SFTP チャネルイベント、restart-required フラグ） |
| 非消費 peek | 状態を読み取るがリセット・drain しない参照 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] wake パスは既に存在する（`send_progress` が `wake()` を呼び、`note_spawn_failure` が `wake()` を呼ぶ）。
      よって本修正は `should_skip_frame` / redraw ペーシングがフレームを破棄することを止めればよく、
      新しい wake 機構は不要
- [x] 主たる修正方針はタスク記述で調査済みの方針（新述語の追加）。代替案（`pump_toasts` を skip ゲートより前に移す）は、
      `pump_sftp` が egui のフレーム時間クロックを必要とするため重いとして、そこで却下されている
- [x] スコープ外（タスク記述による）: トースト UI のデザイン・表示時間の変更、SFTP アップロード機構・
      チャネルレイアウトの変更、`should_skip_frame` の他の項（dirty / status bar / egui input）
- [x] 本件は Linux + Windows の GUI 機能のコードパス。`self_exec` の restart 検知はモジュール doc により
      Linux 限定であり、新しい peek はモジュール既存の cfg 構造に従う
- [x] デザインステップは skip: `src-tauri/src/` 内の純粋なバックエンド／ランタイムロジックのバグ修正
      （frame-skip ゲート述語）であり、新規 UI サーフェスはない。トースト UI のデザイン変更はタスク記述で
      明示的にスコープ外のため、ビジュアル／UI 設計ステップが決めるべきことがない

### 14.2 未確認・保留事項

- [ ] FR5: `next_toast_deadline` の述語選択 — タスク記述がこの選択を明示的に設計時へ委ねている。
      ユーザー向けの質問ではなく、ワークフローの plan ステップで決める

## 15. 参考資料

- em-review finding `b5f2cce1822ab271` (2026-08-11, resolution: deferred)
- `src-tauri/src/self_exec.rs`
- `src-tauri/src/app/mod.rs`（`pump_toasts` 910-916、`next_toast_deadline` 932）
- `src-tauri/src/window_host/render_surface.rs`（`overlay_work` 291 行付近）
- `src-tauri/src/window_host/event_loop.rs`（`toast_redraw_due` 645 行付近）
- `src-tauri/src/sftp/service.rs:58-60`（`ProgressReceiver` / `ResultReceiver`）
- `test/README.md`
