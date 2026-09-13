# Feature: mux-link-pending-switch-single-borrow

要件の出典は `feature-docs/mux-link-pending-switch-single-borrow/REQUIREMENTS.md`。本書はその実装向けの記述であり、要件そのものを新たに起こさない。

## Overview

`src-tauri/src/tabs/mux_link.rs` の `Tab::handle_pty_output`（現状 291-384 行）にある pending-switch ブロックは、`self.pending_switch` を2回引き直している。この2回の間に到達不能な `None` アームが存在し、そこに落ちた場合は PTY ペイロードがログもフォールバックも無く暗黙に破棄される。本フィーチャーは、この箇所を `as_mut()` による単一借用へ統合し、`None` アームを取り除く。観測可能な挙動は一切変えない。

## Objectives

- `handle_pty_output` のライブキュー match から到達不能な `None` アームを取り除く。このアームは、キューイングされた PTY ペイロードがログもフォールバックも無しに暗黙に破棄され得る唯一のコードパスである。
- `LiveQueueOutcome` に対する網羅的な match を維持し、将来バリアントが追加された場合にもこの呼び出し箇所が明示的な判断を強制されるようにする。
- mux の pending-switch ライブキューとそのオーバーフロー時フォールバックの観測可能な挙動を、現在のまま維持する。

## User Stories

### US1: 暗黙の PtyOutput 欠落経路を無くす

eMterm の開発者として、pending-switch ブロックから到達不能な `None` アームを取り除きたい。そうすれば、2つの参照の間に将来コードが挿入されても、panic もログも無く PtyOutput が欠落する故障モードが存在しなくなる。

**Acceptance Criteria:**
- [ ] AC1: `handle_pty_output` の pending-switch ブロック内の `self.pending_switch` アクセスはちょうど1つで、それが `as_mut()` である。`pending_target` はコピーされた `u32` である。
- [ ] AC2: `LiveQueueOutcome` の match は2アームで、`None` アームも `_` ワイルドカードも無い。

### US2: 将来のバリアント追加を強制的に検知する

eMterm の開発者として、`LiveQueueOutcome` に対する match を網羅的なまま保ちたい。そうすれば、このバリアントが増えたときに、この呼び出し箇所が判断を下すまでコンパイルが通らない。

**Acceptance Criteria:**
- [ ] AC2: `LiveQueueOutcome` の match は2アームで、`None` アームも `_` ワイルドカードも無い。
- [ ] AC4: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` と、同じものに `--no-default-features` を付けたものの両方が、新たな警告なしで成功する。
- [ ] AC5: `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests` が新たな警告なしで成功する。

### US3: ライブキューの挙動を現状のまま保つ

eMterm の利用者として、mux の pending-switch ライブキューとそのオーバーフロー時フォールバックの挙動が現状のままであってほしい。そうすれば、このリファクタリングによって表に出る変化が何も起きない。

**Acceptance Criteria:**
- [ ] AC3: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。`src-tauri/src/tabs/tests/replay.rs::ts3_live_output_queued_during_pending_switch`（234 行）および `::offthread_live_queue_cap_falls_back_to_sync`（258 行）を含む。

## Technical Requirements

### Functional Requirements

- **FR1 — Single `as_mut()` borrow of `pending_switch`:** `Tab::handle_pty_output`（`src-tauri/src/tabs/mux_link.rs`、現状 291-384 行）の pending-switch ブロックは、`self.pending_switch` をちょうど1回、`as_mut()` によって取得しなければならない。現状の2回取得のペア（291 行の `self.pending_switch.as_ref().map(|p| p.target_pane)` と、続く 308-311 行の `self.pending_switch.as_mut().map(|p| p.queue_live_output(payload))`）は使わない。実装形は提案 (a)、すなわち `if let Some(pending) = self.pending_switch.as_mut() { let pending_target = pending.target_pane; ... }` とする。
- **FR2 — No `None` arm; exhaustive over `LiveQueueOutcome`:** `queue_live_output` の戻り値に対する match は `LiveQueueOutcome::Queued`（`false` を返す）と `LiveQueueOutcome::Overflowed`（オーバーフロー時フォールバックへ落ちる）のちょうど2アームでなければならない。現状の `Some(LiveQueueOutcome::Queued) | None => return false` アーム — 到達不能な `None` が `payload` をログもフォールバックも無しに破棄するアーム — は無くなっていなければならない。match は enum に対して網羅的なまま（`_` ワイルドカードを使わない）でなければならず、`LiveQueueOutcome`（`src-tauri/src/tabs/replay.rs` 22-30 行）に将来バリアントが追加された場合はこの箇所が判断を下すまでコンパイルが通らない。
- **FR3 — Copied `pending_target` latch serves the DROP path:** `pending_target` は `as_mut()` の借用内で素の `u32` コピーとして控えなければならない。ペイン不一致の DROP パス（osc-probe の `log::warn!` と 370-383 行の `log::debug!`、および `return false`）はそのコピーのみを使わなければならない。`queue_live_output` の match より後で `pending` 束縛そのものに触れてはならない。これが、オーバーフロー時フォールバックの `&mut self` 呼び出し（`self.pending_redispatch.take()`、`self.supersede_pending_replay(...)`、`self.reset_frame_for_replay(...)`、`self.apply_queued_live_output(...)`）より前に NLL が `&mut self.pending_switch` の借用を終わらせる条件である。
- **FR4 — Resolve the `pending` name collision:** オーバーフロー時フォールバックは現在 `let pending = self.supersede_pending_replay("live-queue overflow sync reparse").expect(...)`（`mux_link.rs` 327-329 行）を束縛し、その後 `pending.payload` / `pending.segments` / `pending.live_queue`（362 行、365 行）を読む。FR1 の下ではこの名前が外側の `as_mut()` 束縛をシャドウする。どちらか一方を改名しなければならない。改名は表面的なものであり、後続の各使用箇所がどの値を読むかを変えてはならない。
- **FR5 — Behaviour unchanged:** 上限未満のキューイングは `false`（再描画なし）を返し、オーバーフローは supersede と同期リパースを行って `true` を返し、pending switch 中の非対象ペイン出力は同じログ2行とともに破棄される。`payload` の `queue_live_output` への move は引き続きペイン一致パスでのみ発生する（したがって DROP パスのログで `payload.len()` が読める）。ログのメッセージ文言・レベル・順序はいずれも変更しない。

### Non-Functional Requirements

- **NFR1 - Maintainability (scope):** スコープは `src-tauri/src/tabs/mux_link.rs` の `Tab::handle_pty_output` に限定する。`PendingSwitch::queue_live_output` のシグネチャ、意味論、`#[must_use]` 属性、および `src-tauri/src/tabs/replay.rs` での配置はスコープ外。
- **NFR2 - Maintainability (fallback carried verbatim):** オーバーフロー時フォールバックのロジック（`supersede_pending_replay` の前に `pending_redispatch` を take し、coalesce された再ディスパッチ用ペイロードを優先し、続いて `reset_frame_for_replay` と `apply_queued_live_output` を行う）はスコープ外であり、FR4 の改名を除いてそのまま持ち越す。
- **NFR3 - Safety:** 新たな `unsafe` を導入しない。`payload` の新たなクローンを作らない。`payload` の所有権の流れを組み替えない。
- **NFR4 - Documentation:** 呼び出し箇所の既存 doc コメントは、削除される `None` アームを説明している箇所のみ更新する。周辺の FR3 / FR7 / FR8 に関する記述はそのまま残す。
- **NFR5 - Compatibility:** いずれの feature 構成でも新たなコンパイラ警告を出さない。

## Implementation Approach

### Architecture

本フィーチャーはアーキテクチャを変えない。関係するのは既存の2モジュールのみ。

**Component Diagram:**

```
src-tauri/src/tabs/mux_link.rs
  Tab::handle_pty_output        <- 唯一の変更対象（pending-switch ブロック）
        |
        | pending.queue_live_output(payload) -> LiveQueueOutcome
        v
src-tauri/src/tabs/replay.rs
  PendingSwitch::queue_live_output   <- 変更しない（NFR1）
  enum LiveQueueOutcome { Queued, Overflowed }  <- 変更しない
```

### Data Flow

```
PtyOutput 到着
  -> if let Some(pending) = self.pending_switch.as_mut()     [FR1: 借用は1回だけ]
       let pending_target = pending.target_pane;             [FR3: u32 コピー]
       if 到着ペイン == pending_target
         match pending.queue_live_output(payload)            [FR2: 2アーム、網羅]
           Queued     -> return false                        [再描画なし]
           Overflowed -> オーバーフロー時フォールバックへ落ちる
       else
         log::warn! (osc-probe) + log::debug!                [FR3/FR5: コピーのみ参照]
         return false

  オーバーフロー時フォールバック（NFR2: FR4 の改名以外そのまま）
    -> self.pending_redispatch.take()
    -> self.supersede_pending_replay("live-queue overflow sync reparse")  [FR4: 改名対象]
    -> self.reset_frame_for_replay(...)
    -> self.apply_queued_live_output(...)
    -> return true
```

`pending` 束縛は match より後では一切参照されないため、NLL は `queue_live_output` の呼び出し時点で `&mut self.pending_switch` の借用を終える。これによりフォールバック側の `&mut self` 呼び出し群がコンパイルを通る（FR3）。

### API Design

該当なし。公開 API・ワイヤプロトコルの変更は無い。`PendingSwitch::queue_live_output` は `#[must_use] -> LiveQueueOutcome` の契約を維持する。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/tabs/replay.rs`: `LiveQueueOutcome`（22-30 行）と `PendingSwitch::queue_live_output`。参照するのみで変更しない（NFR1）。
- `src-tauri/src/tabs/tests/replay.rs`: 既存回帰テスト2件が本変更の挙動不変性を担保する。

**External Dependencies:**
- 追加なし。

### File Structure

```
src-tauri/src/tabs/
├── mux_link.rs        # Tab::handle_pty_output（291-384 行付近）— 唯一の変更対象
├── replay.rs          # LiveQueueOutcome / PendingSwitch::queue_live_output — 変更しない
└── tests/
    └── replay.rs      # 既存回帰テスト2件 — 変更しない
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/mux-link-pending-switch-single-borrow/**`
- `test-docs/mux-link-pending-switch-single-borrow/**`

これら2つのデフォルトエントリは、SPEC 作成者が明示的に取り除かない限り宣言の一部である。この宣言はスーパーセット（superset）の主張であり、検証時に観測される実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。

## Test Scenarios

### Unit Tests

- [ ] TS1 — `ts3_live_output_queued_during_pending_switch`（`src-tauri/src/tabs/tests/replay.rs:234`、既存回帰、新規作成なし）: FR5 の上限未満キューパス。対象ペインの2チャンクが到着順にキューされ、非対象ペインのチャンクが破棄される。対応要件: FR3、FR5。
- [ ] TS2 — `offthread_live_queue_cap_falls_back_to_sync`（`src-tauri/src/tabs/tests/replay.rs:258`、既存回帰、新規作成なし）: FR5 のオーバーフローパス。1 MiB のチャンク4つは pending のままで、5つ目が上限を超え、pending switch が破棄され、スナップショットが同期リパースされ、その上にライブ出力が適用される。対応要件: FR4、FR5。

### Integration Tests

追加なし。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

本フィーチャーに E2E テストは関与しない。

### Edge Cases

- [ ] TS3 — `LiveQueueOutcome` match の網羅性（コンパイル時、新規作成なし）: FR2。網羅性は match のコンパイル時の性質であり、ランタイムテストではなく AC4 の `cargo check` によって検証される。対応要件: FR1、FR2。
- [ ] pending switch 中の非対象ペイン出力: ログ2行（osc-probe の `log::warn!` と `log::debug!`）を現状と同一の文言・レベル・順序で出力し、`false` を返して破棄する（FR5）。

### Performance Tests

該当なし。

## Security Considerations

- **Memory safety:** 新たな `unsafe` を導入しない（NFR3）。
- **Data handling:** `payload` の新たなクローンを作らず、所有権の流れも組み替えない。`queue_live_output` への move はペイン一致パスでのみ発生する（NFR3、FR5）。
- 認証・認可・入力検証・XSS・SQL インジェクション・CSRF はいずれも該当なし。

## Error Handling

### Error Codes

エラーコード体系は導入しない。この箇所の異常系は2つの既存経路のみで扱う。

| 条件 | 挙動 |
|------|------|
| `queue_live_output` が `LiveQueueOutcome::Overflowed` を返す | pending switch を破棄し、supersede と同期リパースを行って `true` を返す |
| 到着ペインが `pending_target` と一致しない | ログ2行を出力して破棄し、`false` を返す |

### Error Flow

```
Overflowed -> pending_redispatch.take() -> supersede_pending_replay
           -> reset_frame_for_replay -> apply_queued_live_output -> return true
```

## Performance Optimization

該当なし。観測可能な挙動を変更しないため、性能目標も最適化方針も設定しない。

## Success Criteria

- [ ] AC1（FR1 / FR3）: `handle_pty_output` の pending-switch ブロック内の `self.pending_switch` アクセスはちょうど1つで、それが `as_mut()` である。`pending_target` はコピーされた `u32` である。
- [ ] AC2（FR2）: `LiveQueueOutcome` の match は2アームで、`None` アームも `_` ワイルドカードも無い。
- [ ] AC3（FR5）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が通る。`src-tauri/src/tabs/tests/replay.rs::ts3_live_output_queued_during_pending_switch`（234 行）および `::offthread_live_queue_cap_falls_back_to_sync`（258 行）を含む。
- [ ] AC4（FR4 / NFR）: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` と、同じものに `--no-default-features` を付けたものの両方が、新たな警告なしで成功する。
- [ ] AC5（NFR5）: `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests` が新たな警告なしで成功する。

Windows クロスチェック（AC5）も受け入れゲートに含める（Assumption A1）。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

未解決の要件は無い。FR1〜FR5、NFR1〜NFR5 はすべて resolved である。

## Assumptions

requirements-analyst が確定した前提。いずれも本書が新たに起こしたものではない。

- **A1**（出典: answers[requirement.windows-cross-check-command]、batch-codex-consultation → orchestrator による再判定／変更可能）: 受け入れゲートは4つとする — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`、同じ check に `--no-default-features` を付けたもの、および Windows クロスチェック `CARGO_TARGET_DIR=src-tauri/target-win cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --lib --tests`。経緯: Codex 相談と Opus エスカレーションはいずれも Windows 分を外す (`project_documented_only`) と判定した。根拠は (i) 触れるコードがプラットフォーム非依存で `src-tauri/src/tabs/` 配下に `cfg(windows)` / `cfg(unix)` / `cfg(target_os)` の分岐が無いこと、(ii) `cargo xwin check --tests` という形がプロジェクトのルールファイルに文書化されておらず、未文書かつ Windows SDK をダウンロードするコマンドを必須ゲートにすると無人実行がツールチェーン起因の偽陰性で止まりうること、の2点。その後 orchestrator が承認ストア（`bash_guard.py --list`）を確認したところ、上記のクロスチェック文字列は**すでに承認済み**であり、根拠 (ii) は成り立たないと判明した。残る根拠 (i) は「追加のカバレッジが無い」という弱い主張に過ぎず、タスク本文が明記した4番目の受け入れ基準（`cargo check` 3構成）を狭める理由としては不十分と判断し、Windows クロスチェックを受け入れゲートに戻した（`require_xwin_check` 相当）。
- **A2**（出典: answers[requirement.regression-test-expectation]、batch-codex-consultation、option no_new_test／変更可能）: この変更に対して新規テストは追加しない。根拠: 候補となる2シナリオはいずれも `--lib` スイートに既存のまま存在し（TS1、TS2）、FR2 の網羅性はランタイムで観測できる性質ではなくコンパイル時の性質である。
- **A3**（出典: orchestrator が保持する確定済み設計判断／変更可能）: 実装形は提案 (a)、すなわち単一の `as_mut()` 借用の中で `let pending_target = pending.target_pane;` を控える形であり、提案 (b) ではない。根拠: Codex 相談と Opus エスカレーションの双方が選択した。
- **A4**（出典: orchestrator が保持する確定済み設計判断／変更可能）: NLL は `queue_live_output` の呼び出し時点で `&mut self.pending_switch` の借用を終わらせるため、オーバーフロー時フォールバックの後続の `&mut self` 呼び出しはコンパイルが通る。ただし match より後で `pending` 束縛そのものを使わないこと（FR3）が条件である。根拠: 確定済み設計判断とともに提示された借用領域の推論であり、AC4 で機械的に証明される。
- **A5**（出典: orchestrator が保持する確定済み設計判断／変更可能）: `payload` の所有権は組み替えず、引き続きペイン一致パスでのみ `queue_live_output` に move される。根拠: `mux_link.rs:375` の DROP パスのログが、その地点で `payload` を所有していることに依存している。
- **A6**（出典: タスク記述のスコープ記述、ソース上で確認済み／変更可能）: `PendingSwitch::queue_live_output` は `#[must_use] -> LiveQueueOutcome` という契約と、`src-tauri/src/tabs/replay.rs` という現在の置き場所を維持する。根拠: タスク記述がスコープ外と宣言しており、この属性は上限チェックと呼び出し側のオーバーフロー対応とを結ぶ唯一のリンクである。

## Implementation Phases (if applicable)

単一フェーズ。分割しない。

## References

- 要件定義書: `feature-docs/mux-link-pending-switch-single-borrow/REQUIREMENTS.md`
- 変更対象: `src-tauri/src/tabs/mux_link.rs` — `Tab::handle_pty_output`（現状 291-384 行）
- 参照のみ: `src-tauri/src/tabs/replay.rs` — `LiveQueueOutcome`（22-30 行）、`PendingSwitch::queue_live_output`
- 既存回帰テスト: `src-tauri/src/tabs/tests/replay.rs:234`、`src-tauri/src/tabs/tests/replay.rs:258`
- 発端: em-review finding `184365ab368b4acd`（2026-08-11、ブランチ `fix/em-review-phase7-batch3`、第3ラウンドレビュー）
