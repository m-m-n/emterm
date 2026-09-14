# Feature: notification-worker-thread

## Overview

デスクトップ通知の送出を winit/egui のイベントループスレッドから専用ワーカースレッドへ
移す。現在 `NotifyRustSink::send` がインラインで実行している同期的な Unix ラウンドトリップ
2 件 — `notify_rust::get_capabilities()` (src-tauri/src/callbacks.rs:231) と
`Notification::new()....show()` (:237-241) — をワーカースレッド上に移し、`send` は容量 8 の
境界付きキューへノンブロッキングに投入するだけにする。`NotificationSink` トレイト境界と
全プロデューサ呼び出し箇所は変更しない。

要件の出典: feature-docs/notification-worker-thread/REQUIREMENTS.md

## Objectives

- D-Bus 通知デーモンが遅い・ハングしている・存在しない場合でも、通知送出がイベントループを
  塞がず、タイプ入力のレイテンシとフレームペーシングに影響を与えないこと。
- 既存の `NotificationSink` トレイト境界と全プロデューサ呼び出し箇所（OSC 9、タブ活動、
  エージェント状態、リンク処理）を変更せず、修正を本番シンクの内部に閉じること。

## User Stories

### US1: 通知デーモンの状態に左右されない入力応答性

eMterm 利用者として、通知デーモンが遅い・ハングしている・存在しない状況でも、
ターミナルのタイプ入力とフレームペーシングが影響を受けないことを望む。

**Acceptance Criteria:**

- [ ] AC1: `NotifyRustSink::send` は、呼び出しスレッド上で `notify_rust::get_capabilities()` も
      `Notification::show()` も呼ばない。両者はワーカースレッドの本体にのみ現れる。
- [ ] AC4: ワーカーがブロックしている間に投入された通知について `send` が速やかに戻る。
      ブロック中のワーカーに対して 9 件以上が保留になると超過分はドロップされ、どの
      プロデューサもブロックしない。
- [ ] AC8: ワーカーが D-Bus 呼び出しでブロックしていても、最後の `Arc<NotifyRustSink>` の
      ドロップが境界付き join の期限内に戻る。

### US2: 既存の境界とログ記録を壊さない変更

eMterm 開発者として、トレイト境界・プロデューサ呼び出し箇所・既存テストダブル・ログ記録が
変わらないまま、この修正が本番シンクの内部に閉じることを望む。

**Acceptance Criteria:**

- [ ] AC2: src-tauri/src/callbacks.rs:191-193 の `NotificationSink` トレイト宣言が変更前と
      バイト単位で同一であり、既存の 3 つのテストシンクが無変更でコンパイルできる。
- [ ] AC3: 既存の通知プロデューサ呼び出し箇所（`App::notify` at src-tauri/src/app/mod.rs:836、
      `NativeCallbacks` の OSC 9、タブ活動、`maybe_notify_agent_transition` at
      src-tauri/src/app/agent_status.rs:187、リンク処理）がすべて無変更であり、本番側の唯一の
      構築箇所の変更は src-tauri/src/app/mod.rs:574 の `Arc::new(NotifyRustSink)` が
      コンストラクタ呼び出しになる点のみである。
- [ ] AC6: `log::debug!("notify-rust dispatched: {redacted}")` と
      `log::warn!("notify-rust failed: {e}")` がリテラル接頭辞を維持し、redact 後の表現は
      エスケープ後の値ではなく受領したままの値から導出される。
- [ ] AC7: 送出ごとの capability 照会が引き続き通知ごとに 1 回行われ、その単一の
      `escape_for_send` 評価がタイトルと本文の双方を駆動する。
- [ ] AC9: `cargo test --manifest-path src-tauri/Cargo.toml` が通り、
      `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が引き続き
      コンパイルできる。

### US3: 通知の取りこぼしが観測できること

eMterm 開発者として、通知デーモンの停滞でドロップが起きたことを emterm.log から把握でき、
かつログが埋め尽くされないことを望む。

**Acceptance Criteria:**

- [ ] AC5: 飽和エピソードの最初のドロップがちょうど 1 件の `log::warn!` 記録を生み、同一
      エピソード内の以降のドロップは記録を生まない。キューが捌けた後、新たなドロップは
      再び警告記録を生む。

## Technical Requirements

### Functional Requirements

- **FR1 — ノンブロッキングな `send`:** `NotifyRustSink::send` は D-Bus I/O を一切行わずに
  呼び出し元へ戻る。`(title, body)` をワーカースレッドへ受け渡すのみを行う。現在インラインで
  実行されている同期的な Unix ラウンドトリップ 2 件 — `notify_rust::get_capabilities()`
  (src-tauri/src/callbacks.rs:231) と `Notification::new()....show()` (:237-241) —
  はワーカースレッド上で実行される。
- **FR2 — トレイト境界の不変性:**
  `pub trait NotificationSink: Send + Sync { fn send(&self, title: &str, body: &str); }`
  (src-tauri/src/callbacks.rs:191-193) はバイト単位で不変に保つ。`send` は引き続き `&self` と
  `&str` 引数を取り `()` を返し、既存のテストダブル（`TestSink` at
  src-tauri/src/callbacks/tests.rs:22、`TestNotifySink` at
  src-tauri/src/app/tests/agent_status.rs:22、`NoopSink` at src-tauri/src/tabs/tests.rs:12）は
  無変更でコンパイルできる。
- **FR3 — 送出ごとの capability 意味論の維持:** fail-closed な body-markup エスケープゲートは
  現在の意味論を維持する。`get_capabilities()` は通知ごとに 1 回だけ再照会され（キャッシュ
  しない）、その単一の結果が `escape_for_send` (src-tauri/src/callbacks.rs:267-277) を通じて
  タイトルと本文双方のエスケープ判断を駆動する。照会をワーカースレッドへ移しても送出ごとの
  鮮度は保たれる。
- **FR4 — ログ記録の維持:** `redact_notification(title, body)` (src-tauri/src/callbacks.rs:214)
  は、エスケープゲートが値をシャドウする前の受領値に対して実行され続け、
  `log::debug!("notify-rust dispatched: {redacted}")` (:246) の成功記録と
  `log::warn!("notify-rust failed: {e}")` (:249) のエラー記録に供給され続ける。両記録は
  ワーカースレッドから、リテラル接頭辞を変えずに出力される。
- **FR5 — 境界付きキューと満杯時ドロップ:** シンクは容量 8 の境界付きチャネルを所有する。
  投入はノンブロッキングであり、キューが満杯のときはブロックもキューの無制限成長もせず、
  その通知をドロップする。
- **FR6 — スロットル付きドロップ観測:** 飽和エピソードの最初のドロップで `log::warn!` 記録を
  出す。同一エピソード内の以降のドロップは記録しない。キューが捌けた時点で警告が再武装
  されるため、持続的な停滞ではドロップごとではなくエピソードごとに 1 件の記録となる。
- **FR7 — 即時起動・プロセス寿命のワーカー:** ワーカースレッドはシンク構築時に起動され、
  プロセスの寿命まで生存する。初回送出時の遅延起動も、通知ごとのスレッド生成も行わない。
- **FR8 — 境界付き join によるシャットダウン:** シンクのドロップ時に送信側をドロップして
  ワーカーの受信ループを終了させ、短い境界付き待機でワーカーを join する。その期限を
  過ぎたら join を諦めてスレッドをデタッチし、固まった D-Bus 呼び出しでプロセス終了が
  ハングしないようにする。シャットダウンは `Drop for NotifyRustSink` に置かれ、`App` の
  ティアダウンで最後の `Arc` クローンが解放された時点で発火する。
- **FR9 — プラットフォーム挙動の統一:** ワーカースレッド経由のディスパッチ経路はサポート
  対象の全プラットフォーム（Linux および Windows）に適用され、ディスパッチ経路上に `cfg`
  分岐を持たない。既存の `#[cfg(unix)]` ゲートは現在と同様に capability 照会とエスケープ
  ゲートに限定される。
- **FR10 — シンクがワーカー状態を所有する:** `NotifyRustSink` はユニット構造体
  (src-tauri/src/callbacks.rs:199) から、境界付きチャネルの送信側とワーカーの `JoinHandle` を
  所有する構造体に変わる。プロセスグローバルな `OnceLock` / `static` は導入しない。唯一の
  本番構築箇所 `Arc::new(NotifyRustSink)` (src-tauri/src/app/mod.rs:574) はコンストラクタ
  呼び出しになる。

### Non-Functional Requirements

- **NFR1 — Performance (イベントループのレイテンシ):** `send` は呼び出しスレッド上で、
  境界の定まったアロケーションの軽い時間で完了する。外部デーモンでブロックしうる
  システムコールは winit/egui スレッド上に残さない。
- **NFR2 — Reliability (障害の封じ込め):** 通知デーモンの不在やハングは、通知のドロップと
  ログ記録への劣化に留まる。パニックせず、デッドロックせず、境界付き join の期限を超えて
  プロセス終了を遅延させない。
- **NFR3 — Resource (メモリ境界):** 保留中の通知が占めるメモリは、プロデューサの発行レートに
  よらず容量 8 のキューに収まる。
- **NFR4 — Testability (ヘッドレスなテスト容易性):** キュー挙動（ノンブロッキング投入、
  満杯時ドロップ、警告の再武装）は D-Bus 接続なしでユニットテスト可能である。これは、実
  D-Bus 接続なしでテストできるよう `escape_for_send` を切り出した既存の前例
  (src-tauri/src/callbacks.rs:263-265) に合わせる。
- **NFR5 — Compatibility (フィーチャーゲート互換性):** 変更は既存のモジュール構成の内側に
  留め、`--no-default-features`（CLI 専用）ビルドを壊さない。

## Implementation Approach

### Architecture

**System Architecture:**

```
┌──────────────────────────────────────────────────────────┐
│ 通知プロデューサ（変更なし）                              │
│  App::notify / NativeCallbacks OSC 9 / タブ活動 /         │
│  maybe_notify_agent_transition / リンク処理               │
├──────────────────────────────────────────────────────────┤
│ trait NotificationSink（宣言はバイト単位で不変・FR2）     │
├──────────────────────────────────────────────────────────┤
│ NotifyRustSink（本番実装・本タスクの変更範囲）            │
│  send(): キューへノンブロッキング投入のみ（FR1）          │
│  所有フィールド: 境界付きチャネル送信側 + JoinHandle      │
│                  （FR10）                                 │
│  Drop: 送信側ドロップ + 境界付き join（FR8）              │
├──────────────────────────────────────────────────────────┤
│ 境界付きチャネル（容量 8・満杯時ドロップ・FR5 / FR6）     │
├──────────────────────────────────────────────────────────┤
│ ワーカースレッド（構築時に起動・プロセス寿命・FR7）       │
│  redact_notification → get_capabilities →                │
│  escape_for_send → Notification::show → ログ記録          │
├──────────────────────────────────────────────────────────┤
│ notify_rust（Linux: D-Bus / Windows: トースト）           │
└──────────────────────────────────────────────────────────┘
```

**Component Diagram:**

- **プロデューサ層** — 本タスクでは一切変更しない（AC3）。
- **`NotificationSink` トレイト** — 境界として不変（FR2 / AC2）。テストダブル 3 種は
  従来どおりこのトレイトを実装する。
- **`NotifyRustSink`** — 送信側と `JoinHandle` を所有する構造体。`send` は投入のみ。`Drop` で
  境界付きシャットダウンを実行する。
- **キュー抽象** — 容量 8、ノンブロッキング投入、満杯時ドロップ、ドロップ警告の武装状態を
  持つ。notify-rust トランスポートから切り離され、ヘッドレスにテストできる（NFR4）。
- **ワーカースレッド** — 受信ループ。送信側が全てドロップされるとループを終了する。

### Data Flow

```
プロデューサ → send() → [境界付きキュー 容量8] → ワーカースレッド → notify_rust → デーモン
              ← 即座に return                    ↑ 満杯時はドロップ + エピソード先頭のみ warn
```

送出経路（ワーカースレッド上）:

```
受領した (title, body)
  → redact_notification(title, body)         ← 受領値に対して実行（FR4）
  → get_capabilities()                        ← 通知ごとに 1 回、キャッシュなし（FR3）
  → escape_for_send(...)                      ← 単一の結果で title / body 双方を駆動（FR3）
  → Notification::new()....show()
  → 成功: log::debug!("notify-rust dispatched: {redacted}")
  → 失敗: log::warn!("notify-rust failed: {e}")
```

シャットダウン経路:

```
最後の Arc<NotifyRustSink> が解放
  → Drop for NotifyRustSink
  → 送信側をドロップ → ワーカーの受信ループが終了
  → 短い境界付き待機で join
  → 期限超過なら join を諦めてデタッチ（プロセス終了はハングしない）
```

### API Design

外部 API は追加しない。プロセス内のインタフェースは以下のとおり。

**変更しない（FR2 / AC2）:**

```rust
pub trait NotificationSink: Send + Sync {
    fn send(&self, title: &str, body: &str);
}
```

**変更する（FR10 / AC3）:**

- `NotifyRustSink` — ユニット構造体から、境界付きチャネルの送信側とワーカーの `JoinHandle` を
  所有する構造体へ。
- 構築 — src-tauri/src/app/mod.rs:574 の `Arc::new(NotifyRustSink)` がコンストラクタ呼び出しへ。
- `Drop for NotifyRustSink` — 境界付き join によるシャットダウンを実装する。

### Database Schema

該当なし。永続データの変更はない。

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/callbacks.rs`: `NotificationSink` / `NotifyRustSink` / `redact_notification` /
  `escape_for_send` の所在。本タスクの主たる変更先。
- `src-tauri/src/app/mod.rs`: シンクの唯一の本番構築箇所 (:574) と `App::notify` (:836)。
- `src-tauri/src/app/agent_status.rs`: `maybe_notify_agent_transition` (:187)。呼び出し側として
  無変更。

**External Dependencies:**

- `notify_rust`: 通知送出。Linux は D-Bus、Windows はトースト。既存依存であり追加・更新はしない。

### File Structure

```
src-tauri/src/
├── callbacks.rs                  # NotificationSink（不変） / NotifyRustSink（変更）
├── callbacks/tests.rs            # 既存テスト（無変更で通る） + キュー挙動のユニットテスト追加先
├── app/
│   ├── mod.rs                    # :574 構築箇所のみ変更 / :836 App::notify は無変更
│   ├── agent_status.rs           # 無変更
│   └── tests/agent_status.rs     # 無変更で通る
└── tabs/tests.rs                 # 無変更で通る
```

## Declared Change Set

このセクションは手書きの一覧ではなく create-plan での導出を宣言する。フィーチャー固有の
パスは create-plan で `workflow.yaml` の各タスクの `files` から導出される
(`references/phases/create-plan-phase.md`)。

上記のフィーチャー固有パスに加え、本 SPEC は既定で次の 2 つのワークフロー生成エントリを
宣言する。

- `feature-docs/notification-worker-thread/**`
- `test-docs/notification-worker-thread/**`

`feature-docs/{feature}/**` は `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、
`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、
`retrospect.yaml`、およびデザインステップが生成するデザイン成果物を覆う。これらは各
フェーズドキュメントと `references/phase-state.md` が生成・所有する。本セクションはそれらを
引用するのみで、規則を再掲しない。

`test-docs/{feature}/**` はタスクごとのテスト記録 `test-docs/{feature}/{T}.tests.yaml` を覆う。
これは `implement-phase.md` が生成・所有する。本セクションはそれを引用するのみで、規則を
再掲しない。

この 2 つの既定エントリは、SPEC 作成者が明示的に取り除かない限り宣言の一部である。沈黙に
よる省略は想定されない。取り除くことは意図的かつ明示的な絞り込みである。

この宣言はスーパーセットの主張である。検証時に観測される実際の変更集合は、宣言集合に
含まれる（CONTAINED IN）必要があり、等しい必要はない。implement タスクを 1 つも生成しない
フィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、その場合でも宣言された
`test-docs/{feature}/**` は正しい。宣言されたパスが実体化しないことは違反ではない。

## Test Scenarios

### Unit Tests

- [ ] TS1: ワーカーが占有されている状態で投入がブロックせずに戻る（ヘッドレス）。キュー側の
      抽象を notify-rust トランスポートから切り離して直接検証する。— 対応要件: FR1, FR5, NFR1, NFR4
- [ ] TS2: 容量 8 のキューを満たしてもう 1 件投入すると、超過分がドロップされ、ドロップが
      報告され、プロデューサはブロックしない（ヘッドレス）。— 対応要件: FR5, FR6, NFR3, NFR4
- [ ] TS3: ドロップ警告の武装 — エピソード最初のドロップは報告し、同一エピソード内の以降の
      ドロップは報告しない。キューが捌けると再武装され、次のドロップは再び報告する
      （ヘッドレス）。— 対応要件: FR6, NFR4
- [ ] TS4: シャットダウンで送信側がドロップされ、ワーカーループが終了し、境界付き join が
      期限内に戻る（ヘッドレス）。— 対応要件: FR8, NFR2, NFR4

### Integration Tests

- [ ] TS5（回帰）: 既存の `escape_for_send` / fail-closed capability テスト
      (src-tauri/src/callbacks/tests.rs:691-860) と、既存のレートリミット / redaction テストが
      無変更で通り続ける。— 対応要件: FR2, FR3, FR4
- [ ] TS6（回帰）: 既存のエージェント状態通知テスト
      (src-tauri/src/app/tests/agent_status.rs) が、キャプチャ用シンクを変更しないまま通り
      続ける。— 対応要件: FR2

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

本フィーチャーはユーザーに見える面を持たないため、E2E シナリオは追加しない。

### Edge Cases

- [ ] 通知デーモンが存在しない: 通知はドロップとログ記録へ劣化し、パニックもデッドロックも
      起きない（NFR2）。
- [ ] 通知デーモンがハングしている: 容量 8 を超えた通知はドロップされ、エピソード先頭のみ
      `log::warn!` を出す（FR5 / FR6）。
- [ ] ワーカーが D-Bus 呼び出しで固まったままプロセス終了: 境界付き join の期限でデタッチし、
      終了はハングしない（FR8 / AC8）。
- [ ] 実 `App` を構築するテスト: src-tauri/src/app/tests/agent_status.rs:49 が
      `app.notification_sink` をテストシンクで上書きするのは `App` 構築の後であるため、
      本番ワーカースレッドが起動され即座にドロップされる。境界付き join により挙動上は
      無害だが、ドロップ経路は `cargo test` 下で健全であり続ける必要がある（A9）。

### Performance Tests

専用の負荷試験は設けない。性能要件 NFR1 / NFR3 は AC1（呼び出しスレッド上に D-Bus 同期
呼び出しが存在しないこと）と AC4（ブロック中のワーカーに対して `send` が速やかに戻り、
超過分がドロップされること）で確認する。

## Security Considerations

- **Input Validation:** FR3 の fail-closed な body-markup エスケープゲートを現行の意味論のまま
  維持する。capability 照会は通知ごとに 1 回行われ、その単一の結果が `escape_for_send` を
  通じてタイトルと本文双方のエスケープ判断を駆動する。
- **Data Protection:** FR4 の `redact_notification` を、エスケープゲートが値をシャドウする前の
  受領値に対して実行し続ける。ログに出るのは redact 後の表現のみである。
- **Out of scope:** 通知本文のサニタイズは別タスクが扱う。通知の発火条件とレートリミットも
  変更しない。

## Error Handling

### Error Cases

| ケース | 条件 | 挙動 |
|--------|------|------|
| 通知送出の失敗 | `Notification::show()` がエラーを返す | ワーカースレッドから `log::warn!("notify-rust failed: {e}")` を出力（FR4） |
| キュー満杯 | 容量 8 のキューが満杯 | 通知をドロップ。ブロックしない（FR5） |
| 飽和の可観測化 | エピソード最初のドロップ | `log::warn!` を 1 件出力。以降は再武装まで無音（FR6） |
| 終了時のワーカー停滞 | join が期限内に戻らない | join を諦めてデタッチ。終了はハングしない（FR8） |

### Error Flow

```
エラー発生 → ワーカースレッドでログ記録 → 呼び出し元へは伝播しない（send は () を返す）
```

`send` の戻り値型は `()` のままであり（FR2）、送出失敗はログ記録のみで表現される。

## Performance Optimization

### Performance Goals

- 呼び出しスレッド（winit/egui）上に残る、外部デーモンでブロックしうるシステムコール: 0 件。
- `send` は境界の定まったアロケーションの軽い時間で完了する（NFR1）。
- 保留通知のメモリ上限: 容量 8 のキュー分（NFR3）。

### Optimization Strategies

- 同期 D-Bus ラウンドトリップ 2 件のワーカースレッドへの移動（FR1）。
- ノンブロッキング投入と満杯時ドロップによるプロデューサ側の待ちの排除（FR5）。
- 構築時の即時起動により、送出経路から遅延初期化の同期を排除（FR7）。

### Caching Strategy

capability 照会結果はキャッシュしない。通知ごとに 1 回照会する（FR3）。

## Success Criteria

- [ ] AC1 から AC9 のすべてを満たす（REQUIREMENTS.md 11.1 と本書 User Stories 参照）。
- [ ] FR1 から FR10、NFR1 から NFR5 のすべてが実装・検証される。
- [ ] TS1 から TS7 のすべてのテストシナリオが通る。
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` が通る。
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る（TS7）。
- [ ] レビューが完了している。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。FR1 から FR10 および NFR1 から NFR5 のすべてが `status: resolved`。

## Assumptions

以下はバッチ協議で確定し、`record_as_assumption: true` 方針により想定として記録されたもの。
いずれも可逆。

- **A2:** シャットダウンは送信側をドロップし、短い境界付き待機でワーカーを join する。期限を
  過ぎたらデタッチする。（`requirement.worker-lifecycle` / `join_bounded`）
- **A3:** 境界付きチャネルの容量は 8。調整可能な定数であり、構造上の確定事項ではない。
  （`requirement.channel-capacity` / `cap_8`）
- **A4:** ドロップした通知は飽和エピソードの最初のドロップで `log::warn` を出し、キューが
  捌いた時点で再武装される。`warn` はリリースビルドで残る最下位レベル
  (.claude/rules/debugging-constraints.md)。（`requirement.drop-observability` /
  `log_warn_throttled`）
- **A5:** ワーカースレッド経路は Linux と Windows の双方に同様に適用し、ディスパッチ経路に
  cfg 分岐を持たない。既存の `#[cfg(unix)]` ゲートは影響を受けない。
  （`requirement.platform-scope` / `all_platforms`）
- **A6:** ワーカースレッドはシンク構築時に即時起動し、プロセスの寿命まで生存する。本番シンクは
  src-tauri/src/app/mod.rs:574 でちょうど 1 回だけ構築される。（`requirement.spawn-timing` /
  `eager_on_construction`）
- **A7:** ノンブロッキング投入と満杯時ドロップを覆うユニットテストを追加し、ヘッドレスに
  走るよう notify-rust トランスポートから切り離す。`escape_for_send` を切り出した既存の前例
  (src-tauri/src/callbacks.rs:263-265) に沿う。（`requirement.drop-test-coverage` /
  `add_queue_tests`）
- **A8:** `NotifyRustSink` はプロセスグローバルな `OnceLock` ではなく所有フィールド（境界付き
  チャネル送信側とワーカーの `JoinHandle`）を持ち、シャットダウンは `Drop for NotifyRustSink`
  に置く。所有状態がワーカーの寿命をシンクの寿命に結び付け、`Drop` ベースの境界付き join
  （A2）を定義可能にする。トレイト宣言は不変のままなので変更は構築箇所のみであり、AC2 /
  AC3 と両立する。
- **A9:** 実 `App` を構築するテストは本番ワーカースレッドを起動して即座にドロップする。
  src-tauri/src/app/tests/agent_status.rs:49 が `app.notification_sink` をテストシンクで
  上書きするのは `App` 構築の後だからである。境界付き join（A2）により挙動上は無害だが、
  ドロップ経路は `cargo test` 下で健全であり続ける必要がある。

## Scope Exclusions

- 通知本文のサニタイズ（別タスクで扱う）。
- 通知の発火条件の変更。
- レートリミットの変更。

## Design Step

スキップ。本フィーチャーはユーザーに見える面を持たない。変更されない `NotificationSink`
トレイト境界の背後で、同期 D-Bus ラウンドトリップ 2 件をイベントループスレッドから移す
だけであり、UI・レイアウト・デザイントークン・新規のユーザー向け文字列のいずれも関与しない。
本プロジェクトで検出されたデザインシステム候補 2 件（doc/UI-DESIGN-GUIDELINES.yaml、
src-tauri/web-shared/styles.css）はいずれも影響を受けない。
（`design-step.decision` / `decide_autonomously` による確定。）

## References

- 要件定義書: feature-docs/notification-worker-thread/REQUIREMENTS.md
- ログレベルの制約: .claude/rules/debugging-constraints.md
- ビルド・テストコマンド: .claude/rules/core-commands.md
- アーキテクチャとフィーチャーゲート: .claude/rules/core-architecture.md
