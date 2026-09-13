---
title: "mux-link-pending-switch-single-borrow"
created_date: 2026-09-13
status: draft
---

# mux-link-pending-switch-single-borrow - 要件定義書

## 1. 概要

### 1.1 背景

発端は em-review の指摘（2026-08-11、ブランチ `fix/em-review-phase7-batch3`、第3ラウンドレビュー、finding `184365ab368b4acd`）。

そのブランチの第2ラウンドのリファクタリング以前、この箇所は `if let Some(pending) = self.pending_switch.as_mut()` という単一の借用を保持しており、スイッチが進行中であることが構造的に保証されていた。`PendingSwitch::queue_live_output` を切り出した結果、呼び出し側が `pending_switch` を2回引き直す形になり、両者が同じ `Option` を見るという不変条件はコメントだけで支えられている。

現在は2つの参照の間にログ文が1つあるだけなので `None` アームは到達不能である。懸念は、将来この2つの間にコードが挿入された場合の故障モード、すなわち panic もログも伴わない PtyOutput の暗黙の欠落である。

### 1.2 目的

`handle_pty_output` のライブキュー match から到達不能な `None` アームを取り除く。このアームは、キューイング対象の PTY ペイロードがログもフォールバックも無しに暗黙に破棄され得る唯一のコードパスである。

### 1.3 スコープ

対象は `src-tauri/src/tabs/mux_link.rs` の `Tab::handle_pty_output`（現状 291-384 行）内の pending-switch ブロックのみ。mux の pending-switch ライブキューおよびそのオーバーフロー時フォールバックの観測可能な挙動は、現状のまま一切変更しない。

## 2. ビジネス要件

### 2.1 ビジネス目標

- `handle_pty_output` のライブキュー match から到達不能な `None` アームを取り除く。このアームは、キューイングされた PTY ペイロードがログもフォールバックも無しに暗黙に破棄され得る唯一のコードパスである。
- `LiveQueueOutcome` に対する網羅的な match を維持し、将来バリアントが追加された場合にもこの呼び出し箇所が明示的な判断を強制されるようにする。
- mux の pending-switch ライブキューとそのオーバーフロー時フォールバックの観測可能な挙動を、現在のまま維持する。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm の開発者 | `src-tauri/src/tabs/` の mux ライブキュー周辺を今後変更する担当者 |

### 2.3 期待される効果

- 2つの参照の間にコードが挿入された場合でも、PtyOutput が暗黙に欠落する経路が存在しなくなる
- `LiveQueueOutcome` にバリアントが追加された際、この呼び出し箇所がコンパイルエラーで判断を要求される状態が保たれる

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | pending switch 中の対象ペイン出力をキューに積む | mux ライブキュー | 高 |
| UC02 | キュー上限超過時に同期リパースへフォールバックする | mux ライブキュー | 高 |
| UC03 | pending switch 中の非対象ペイン出力を破棄する | mux ライブキュー | 高 |

### 3.2 ユースケース詳細

#### UC01: pending switch 中の対象ペイン出力をキューに積む

**アクター**: mux ライブキュー（`Tab::handle_pty_output`）

**事前条件**:
- `self.pending_switch` が `Some` である
- 到着した出力のペインが `pending_target` と一致する

**基本フロー**:
1. `self.pending_switch.as_mut()` で借用を1回だけ取得する
2. `pending.target_pane` を `pending_target` として `u32` のコピーで控える
3. `pending.queue_live_output(payload)` を呼ぶ
4. 戻り値が `LiveQueueOutcome::Queued` なので `false` を返す

**代替フロー**:
- 戻り値が `LiveQueueOutcome::Overflowed` の場合は UC02 へ

**事後条件**:
- ペイロードがキューに積まれ、再描画は発生しない（`false`）

#### UC02: キュー上限超過時に同期リパースへフォールバックする

**アクター**: mux ライブキュー（`Tab::handle_pty_output`）

**事前条件**:
- `queue_live_output` が `LiveQueueOutcome::Overflowed` を返した

**基本フロー**:
1. match の `Overflowed` アームからオーバーフロー時フォールバックへ落ちる
2. `self.pending_redispatch.take()` を行う
3. `self.supersede_pending_replay(...)` で pending replay を破棄する
4. `self.reset_frame_for_replay(...)` を行う
5. `self.apply_queued_live_output(...)` でキュー済みライブ出力を適用する
6. `true` を返す

**事後条件**:
- supersede と同期リパースが行われ、`true`（再描画あり）が返る

#### UC03: pending switch 中の非対象ペイン出力を破棄する

**アクター**: mux ライブキュー（`Tab::handle_pty_output`）

**事前条件**:
- `self.pending_switch` が `Some` である
- 到着した出力のペインが `pending_target` と一致しない

**基本フロー**:
1. 控えておいた `pending_target` のコピーのみを用いて osc-probe の `log::warn!` と `log::debug!`（現状 370-383 行）を出力する
2. `false` を返す

**事後条件**:
- ペイロードは破棄され、ログ2行は現状と同一の文言・レベル・順序で出力される

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | `pending_switch` の `as_mut()` 単一借用 | 2回の引き直しを1回の `as_mut()` に統合する | 高 |
| FR2 | `None` アームの排除と `LiveQueueOutcome` の網羅 | match を2アームにし、ワイルドカード無しで網羅する | 高 |
| FR3 | コピーした `pending_target` による DROP パス | DROP パスは `u32` コピーのみを参照する | 高 |
| FR4 | `pending` 名前衝突の解消 | 内外どちらかの束縛名を改名する | 高 |
| FR5 | 挙動不変 | 観測可能な挙動とログを一切変えない | 高 |

### 4.2 機能詳細

#### FR1: `pending_switch` の `as_mut()` 単一借用

**説明**: `Tab::handle_pty_output`（`src-tauri/src/tabs/mux_link.rs`、現状 291-384 行）の pending-switch ブロックは、`self.pending_switch` をちょうど1回、`as_mut()` によって取得しなければならない。現状の2回取得のペア（291 行の `self.pending_switch.as_ref().map(|p| p.target_pane)` と、それに続く 308-311 行の `self.pending_switch.as_mut().map(|p| p.queue_live_output(payload))`）は廃する。

実装形は提案 (a) とする。

```rust
if let Some(pending) = self.pending_switch.as_mut() {
    let pending_target = pending.target_pane;
    ...
}
```

**ステータス**: resolved

#### FR2: `None` アームの排除と `LiveQueueOutcome` の網羅

**説明**: `queue_live_output` の戻り値に対する match は、`LiveQueueOutcome::Queued`（`false` を返す）と `LiveQueueOutcome::Overflowed`（オーバーフロー時フォールバックへ落ちる）のちょうど2アームでなければならない。

現状の `Some(LiveQueueOutcome::Queued) | None => return false` アーム — 到達不能な `None` が `payload` をログもフォールバックも無しに破棄するアーム — は無くなっていなければならない。

match は enum に対して網羅的なまま（`_` ワイルドカードを使わない）でなければならない。これにより `LiveQueueOutcome`（`src-tauri/src/tabs/replay.rs` 22-30 行）に将来バリアントが追加された場合も、この箇所が判断を下すまでコンパイルが通らない。

**ステータス**: resolved

#### FR3: コピーした `pending_target` による DROP パス

**説明**: `pending_target` は `as_mut()` の借用内で素の `u32` コピーとして控えなければならない。ペイン不一致の DROP パス（osc-probe の `log::warn!` と 370-383 行の `log::debug!`、および `return false`）は、そのコピーのみを使わなければならない。

`queue_live_output` の match より後で `pending` 束縛そのものに触れてはならない。これが、オーバーフロー時フォールバックの `&mut self` 呼び出し（`self.pending_redispatch.take()`、`self.supersede_pending_replay(...)`、`self.reset_frame_for_replay(...)`、`self.apply_queued_live_output(...)`）より前に NLL が `&mut self.pending_switch` の借用を終わらせる条件である。

**ステータス**: resolved

#### FR4: `pending` 名前衝突の解消

**説明**: オーバーフロー時フォールバックは現在 `let pending = self.supersede_pending_replay("live-queue overflow sync reparse").expect(...)`（`mux_link.rs` 327-329 行）を束縛し、その後 `pending.payload` / `pending.segments` / `pending.live_queue`（362 行、365 行）を読んでいる。FR1 の下ではこの名前が外側の `as_mut()` 束縛をシャドウする。

どちらか一方を改名しなければならない。改名は表面的なものであり、後続の各使用箇所がどの値を読むかを変えてはならない。

**ステータス**: resolved

#### FR5: 挙動不変

**説明**: 上限未満のキューイングは `false`（再描画なし）を返し、オーバーフローは supersede と同期リパースを行って `true` を返し、pending switch 中の非対象ペイン出力は同じログ2行とともに破棄される。`payload` の `queue_live_output` への move は、引き続きペイン一致パスでのみ発生する（したがって DROP パスのログで `payload.len()` が読める状態が保たれる）。

ログのメッセージ文言・レベル・順序はいずれも変更しない。

**ステータス**: resolved

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| ライブキューが上限を超える | `queue_live_output` が `LiveQueueOutcome::Overflowed` を返す | pending switch を破棄し、同期リパース＋ライブ出力適用を行って `true` を返す |
| pending switch 中に非対象ペインの出力が届く | 到着ペイン != `pending_target` | ログ2行を出力して破棄し、`false` を返す |

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし（観測可能な挙動を変更しない内部リファクタリングのため）。

### 5.2 セキュリティ要件

- NFR3: 新たな `unsafe` を導入しない。`payload` の新たなクローンを作らない。`payload` の所有権の流れを組み替えない。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

- NFR1: スコープは `src-tauri/src/tabs/mux_link.rs` の `Tab::handle_pty_output` に限定する。`PendingSwitch::queue_live_output` のシグネチャ、意味論、`#[must_use]` 属性、および `src-tauri/src/tabs/replay.rs` での配置はスコープ外。
- NFR2: オーバーフロー時フォールバックのロジック（`supersede_pending_replay` の前に `pending_redispatch` を take し、coalesce された再ディスパッチ用ペイロードを優先し、続いて `reset_frame_for_replay` と `apply_queued_live_output` を行う）はスコープ外であり、FR4 の改名を除いてそのまま持ち越す。
- NFR4: 呼び出し箇所の既存 doc コメントは、削除される `None` アームを説明している箇所のみ更新する。周辺の FR3 / FR7 / FR8 に関する記述はそのまま残す。

### 5.5 互換性要件

- NFR5: いずれの feature 構成でも新たなコンパイラ警告を出さない。

## 6. UI/UX要件

該当なし。ユーザーから見える表面（UI、CSS、デザイントークン）を持たない内部制御フローの変更である。

## 7. データ要件

該当なし。データモデル・保持期間に変更はない。

## 8. 外部連携

該当なし。ワイヤプロトコルや公開 API の変更はない。

## 9. 制約条件

### 9.1 技術的制約

- `&mut self.pending_switch` の借用は、オーバーフロー時フォールバックの `&mut self` 呼び出しより前に NLL によって終了している必要がある。そのために match 以降で `pending` 束縛そのものに触れないこと（FR3）。
- `payload` の所有権は組み替えない。DROP パスのログ（`mux_link.rs:375`）が `payload` をその場で所有していることに依存している。

### 9.2 ビジネス上の制約

- 観測可能な挙動を変えないこと。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-link-pending-switch-single-borrow/**`
- `test-docs/mux-link-pending-switch-single-borrow/**`

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| `as_mut()` 借用がオーバーフロー時フォールバックの `&mut self` 呼び出しと衝突する | 高 | match 以降で `pending` 束縛に触れず、`pending_target` を `u32` コピーで控える（FR3）。成立は AC4 の `cargo check` で機械的に確認する |
| 外側の `pending` 束縛が既存のフォールバック内 `pending` をシャドウする | 中 | どちらか一方を改名する（FR4）。各使用箇所が読む値は変えない |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| リファクタリングによりライブキューの挙動が変わる | 低 | 高 | 既存回帰テスト2件（`ts3_live_output_queued_during_pending_switch`、`offthread_live_queue_cap_falls_back_to_sync`）で担保する（FR5） |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1（FR1 / FR3）: `handle_pty_output` の pending-switch ブロック内の `self.pending_switch` アクセスはちょうど1つで、それが `as_mut()` である。`pending_target` はコピーされた `u32` である。
- [ ] AC2（FR2）: `LiveQueueOutcome` の match は2アームで、`None` アームも `_` ワイルドカードも無い。
- [ ] AC3（FR5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。`src-tauri/src/tabs/tests/replay.rs::ts3_live_output_queued_during_pending_switch`（234 行）および `::offthread_live_queue_cap_falls_back_to_sync`（258 行）を含む。
- [ ] AC4（FR4 / NFR）: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` と、同じものに `--no-default-features` を付けたものの両方が、新たな警告なしで成功する。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] TS1（正常系、既存回帰）: `ts3_live_output_queued_during_pending_switch`（`src-tauri/src/tabs/tests/replay.rs:234`）— FR5 の上限未満キューパス。対象ペインの2チャンクが到着順にキューされ、非対象ペインのチャンクが破棄される。対応要件: FR3、FR5。
- [ ] TS2（境界値、既存回帰）: `offthread_live_queue_cap_falls_back_to_sync`（`src-tauri/src/tabs/tests/replay.rs:258`）— FR5 のオーバーフローパス。1 MiB のチャンク4つは pending のままで、5つ目が上限を超え、pending switch が破棄され、スナップショットが同期リパースされ、その上にライブ出力が適用される。対応要件: FR4、FR5。
- [ ] TS3（コンパイル時）: `LiveQueueOutcome` match の網羅性 — FR2。網羅性は match のコンパイル時の性質であり、ランタイムテストではなく AC4 の `cargo check` で検証する。対応要件: FR1、FR2。

新規テストは追加しない（確認済み事項 A2 を参照）。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| pending switch | `Tab` が保持する進行中のペイン切り替え状態（`self.pending_switch`） |
| ライブキュー | pending switch 進行中に到着した対象ペインの PTY 出力を貯める領域 |
| DROP パス | pending switch 中に非対象ペインの出力が届いた際に、ログ2行を出して破棄する経路 |
| NLL | Non-Lexical Lifetimes。借用の有効範囲を最後の使用箇所で終わらせる Rust の借用チェック方式 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A1 受け入れゲートの範囲: 受け入れはプロジェクトが文書化している Rust のゲートに限定する — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`、および同じ check に `--no-default-features` を付けたもの。Windows クロスチェックは明示的にブロッキングな受け入れゲートとしない。これはタスク記述に書かれた4番目の受け入れ基準を狭めるものである。
  - 根拠: 触れるコードはプラットフォーム非依存の制御フローであり、`src-tauri/src/tabs/` 配下のどこにも `cfg(windows)` / `cfg(unix)` / `cfg(target_os)` の分岐は無い。`cargo xwin check --tests` という形はこのプロジェクトでは文書化されていない。
  - 出典: answers[requirement.windows-cross-check-command]（batch-codex-consultation、option project_documented_only）／変更可能
- [x] A2 新規テストの有無: この変更に対して新規テストは追加しない。
  - 根拠: 候補となる2シナリオはいずれも `--lib` スイートに既存のまま存在し（TS1、TS2）、FR2 の網羅性はランタイムで観測できる性質ではなくコンパイル時の性質である。
  - 出典: answers[requirement.regression-test-expectation]（batch-codex-consultation、option no_new_test）／変更可能
- [x] A3 実装形: 実装形は提案 (a)、すなわち単一の `as_mut()` 借用の中で `let pending_target = pending.target_pane;` を控える形とし、提案 (b) は採らない。
  - 根拠: Codex 相談と Opus エスカレーションの双方が選択した。
  - 出典: orchestrator が保持する確定済み設計判断／変更可能
- [x] A4 借用の終了: NLL は `queue_live_output` の呼び出し時点で `&mut self.pending_switch` の借用を終わらせるため、オーバーフロー時フォールバックの後続の `&mut self` 呼び出しはコンパイルが通る。ただし match より後で `pending` 束縛そのものを使わないこと（FR3）が条件である。
  - 根拠: 確定済み設計判断とともに提示された借用領域の推論。AC4 で機械的に証明される。
  - 出典: orchestrator が保持する確定済み設計判断／変更可能
- [x] A5 `payload` の所有権: `payload` の所有権は組み替えない。引き続きペイン一致パスでのみ `queue_live_output` に move される。
  - 根拠: `mux_link.rs:375` の DROP パスのログが、その地点で `payload` を所有していることに依存している。
  - 出典: orchestrator が保持する確定済み設計判断／変更可能
- [x] A6 `queue_live_output` の契約: `PendingSwitch::queue_live_output` は `#[must_use] -> LiveQueueOutcome` という契約と、`src-tauri/src/tabs/replay.rs` という現在の置き場所を維持する。
  - 根拠: タスク記述がスコープ外と宣言している。この属性は、上限チェックと呼び出し側のオーバーフロー対応とを結ぶ唯一のリンクである。
  - 出典: タスク記述のスコープ記述（ソース上で確認済み）／変更可能

### 14.2 未確認・保留事項

なし。全要件が resolved である。

デザインステップはスキップされている。理由: ユーザーから見える表面を持たない Rust 関数1つの内部制御フローのリファクタリングであり、UI も CSS もデザイントークンもワイヤプロトコル／公開 API の変更も無い。挙動は不変であることが要求されている。

## 15. 参考資料

- `src-tauri/src/tabs/mux_link.rs`: `Tab::handle_pty_output`（現状 291-384 行）— 変更対象
- `src-tauri/src/tabs/replay.rs`: `LiveQueueOutcome`（22-30 行）、`PendingSwitch::queue_live_output`
- `src-tauri/src/tabs/tests/replay.rs`: `ts3_live_output_queued_during_pending_switch`（234 行）、`offthread_live_queue_cap_falls_back_to_sync`（258 行）
- em-review finding `184365ab368b4acd`（2026-08-11、ブランチ `fix/em-review-phase7-batch3`、第3ラウンドレビュー）
