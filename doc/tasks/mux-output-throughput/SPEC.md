# Feature: Mux Output Throughput Optimization

## Overview

muxデーモンの高頻度PTY出力時のフリーズを解消する。glancesなど画面更新の多いアプリを複数タブで実行した際、デーモンのイベントループがPTY出力の逐次送信でブロックされ、入力を含む全処理が停止する問題を修正する。

## Objectives

- 高頻度出力アプリ（glances, htop, yes等）を複数タブで実行してもフリーズしない
- キー入力の応答性を出力フラッド中も維持する
- 既存のdetach/reattach動作を壊さない

## User Stories

### US1: 高頻度出力アプリの同時利用
As a terminal user, I want to run glances in multiple tabs without the terminal freezing, so that I can monitor system resources while working.

**Acceptance Criteria:**
- [ ] Tab1でglances、Tab2でglancesを同時実行し、両方のタブが描画を継続する
- [ ] タブ切替がフリーズしない
- [ ] キー入力（Ctrl+C等）が遅延なく処理される

### US2: 大量出力中の入力応答性
As a terminal user, I want to type commands while a background tab produces heavy output, so that my workflow is not interrupted.

**Acceptance Criteria:**
- [ ] `yes` をバックグラウンドタブで実行中、アクティブタブでの入力が途切れない
- [ ] Ctrl+Cでバックグラウンドプロセスを停止できる

## Technical Requirements

### Functional Requirements

- **FR1:** `select!`ループ内でPTY出力チャネルをドレインし、溜まったチャンクをバッチ処理する
- **FR2:** PtyInput（キー入力）を出力処理より優先し、出力バッチ処理中もクライアントメッセージをチェックする
- **FR3:** 全paneの出力データを欠損なくIPC転送する（間引きは行わない）
- **FR4:** 既存のdetach/reattach、ring buffer、shadow parserの動作を維持する

> **将来検討:** 非アクティブpane（ユーザーに見えていないタブ）のIPC転送スキップは、デーモン側shadow parserからのタブ切替時復元と組み合わせることで実現可能。tmuxの間引き（表示中ペインの中間フレームドロップ）とは異なり、ユーザー視認性に影響しない最適化。タブ切替時の復元コストはパフォーマンス次第だが、基本的に許容可能なトレードオフ。本タスクのスコープ外とし、別途検討する。

### Non-Functional Requirements

- **NFR1 - Latency:** 出力フラッド中のキー入力遅延が50ms以下
- **NFR2 - Throughput:** `cat large_file`のスループットが現行と同等以上
- **NFR3 - Memory:** 追加メモリ使用量がpaneあたり1MB以下
- **NFR4 - Correctness:** バッチ処理によるデータ欠損・順序逆転がないこと

## Implementation Approach

### Architecture

**現状のデータフロー:**
```
PTY reader thread (per pane)
  │ blocking read (65536 bytes)
  │ try_send / blocking_send
  ▼
Shared mpsc channel (capacity: 256, 全pane共有)
  │
  ▼
select! loop (single async task)
  │ recv() → 1チャンク取得
  │ framed.send().await → IPC書き込み待ち
  ▼
Unix socket → Bridge process → APC sequences → Frontend
```

**問題:** `select!`が1チャンクずつ逐次`framed.send().await`するため、IPC書き込み待ちの間に次のチャンクを処理できない。チャネルが詰まるとreader threadの`blocking_send()`がブロックし、全paneのPTY読み取りが停止する。

**改善後のデータフロー:**
```
PTY reader thread (per pane)
  │ blocking read (65536 bytes)
  │ try_send / blocking_send
  ▼
Shared mpsc channel (capacity: 256, 全pane共有)
  │
  ▼
select! loop (single async task)
  │ recv() → 最初の1チャンク
  │ try_recv() loop → 残りを全てドレイン (上限: DRAIN_BATCH_LIMIT)
  │ pane_id別にグルーピング
  │ 各paneの連続チャンクを結合 (concat)
  │ framed.feed() × N → バッファに積む
  │ framed.flush().await → 一括書き込み
  ▼
Unix socket → Bridge process → APC sequences → Frontend
```

### Strategy 1: Output Drain & Batch (Primary)

`select!`の`pane_output_rx.recv()`ブランチ内で、チャネルに溜まったチャンクを`try_recv()`で一括取得し、バッチ送信する。

```rust
chunk = pane_output_rx.recv() => {
    if let Some(first) = chunk {
        let mut chunks = vec![first];
        // ドレイン: チャネルに残っているチャンクを非ブロッキングで全取得
        // 上限を設けて入力処理の機会を確保
        while chunks.len() < DRAIN_BATCH_LIMIT {
            match pane_output_rx.try_recv() {
                Ok(c) => chunks.push(c),
                Err(_) => break,
            }
        }

        // 同一pane_idの連続チャンクをマージ（オプション最適化）
        let merged = merge_consecutive_chunks(chunks);

        // feed()でバッファに積み、最後にflush()で一括送信
        for chunk in merged {
            if chunk.data.is_empty() {
                // PTY exit signal
                let exit_msg = ...;
                framed.feed(exit_msg).await?;
            } else {
                let msg = MuxMessage::pty_output(chunk.pane_id, chunk.data);
                framed.feed(msg).await?;
            }
        }
        framed.flush().await?;
    }
}
```

**`DRAIN_BATCH_LIMIT`**: 64チャンク。これを超えたらループを抜けて`select!`に戻り、入力メッセージを処理する機会を与える。

**`merge_consecutive_chunks`**: 同一pane_idの連続チャンクを1つの`Vec<u8>`に結合する。フレーム数を削減し、IPC/base64エンコードのオーバーヘッドを軽減する。

### Strategy 2: Input Priority (Complementary)

`select!`のbiased modeを使い、クライアントメッセージ（PtyInput含む）を出力より優先する。

```rust
tokio::select! {
    biased;  // 上から順に優先

    // 最優先: クライアントからのメッセージ（PtyInput等）
    msg = framed.next() => { ... }

    // 次: PTY出力のバッチ処理
    chunk = pane_output_rx.recv() => { ... }

    // 低優先: ステータスバー更新
    _ = render_tick => { ... }
    Some(cmd_name) = cmd_tick_rx.recv() => { ... }
    Some((name, output)) = cmd_result_rx.recv() => { ... }
}
```

`biased`により、クライアントメッセージとPTY出力の両方が準備完了の場合、常にクライアントメッセージが先に処理される。

### Data Flow

```
PTY Process (per pane)
  │
  ▼ (blocking read, 65KB)
PTY Reader Thread
  │
  ▼ (try_send / blocking_send)
Shared Channel (capacity: 256)
  │
  ▼ (recv + try_recv drain, batch up to 64)
select! loop ──biased──► Client messages (PtyInput priority)
  │
  ▼ (merge consecutive same-pane chunks)
  │
  ▼ (feed × N + flush)
Framed<UnixStream, MuxCodec>
  │
  ▼ (length-delimited frames)
Unix Domain Socket
  │
  ▼ (bridge process reads, wraps in APC)
PTY stdout → Frontend WASM parser → Canvas render
```

### Key Design Decisions

**Q: チャネル容量(256)を増やすべきか？**
A: 現時点では変更しない。ドレイン方式でチャネルの消費速度が大幅に向上するため、容量256でも十分なバックプレッシャーが機能する。必要に応じて後から調整可能。

**Q: per-paneチャネルにすべきか？**
A: 現時点では共有チャネルを維持する。per-paneチャネルは`select!`のビルド時固定制約（動的にブランチを追加できない）と相性が悪く、`FuturesUnordered`等の導入が必要になり複雑度が上がる。ドレイン方式は共有チャネルのまま問題を解決できる。

**Q: 非アクティブpaneの出力をドロップすべきか？**
A: 本タスクでは行わない。現在フロントエンドが非アクティブpaneのgrid状態をWASMで保持しており、全データの転送が必要。将来的にはデーモン側shadow parserからのタブ切替時復元と組み合わせてIPC転送をスキップする最適化が可能（FR3の将来検討を参照）。

**Q: `feed()` + `flush()` vs 逐次 `send()`?**
A: `feed()` + `flush()`を使う。`send()` = `feed()` + `flush()`であり、チャンクごとにflushすると各回でsyscall(write)が発生する。バッチ全体を`feed()`で内部バッファに積み、最後に1回の`flush()`で書き出すことでsyscall回数を削減する。

### Dependencies

**Internal Dependencies:**
- `connection.rs`: select!ループの修正（主要変更箇所）
- `pane.rs`: `PTY_CHANNEL_CAPACITY`定数（変更なし）
- `pty_spawn.rs`: reader thread（変更なし）
- `protocol.rs`: MuxMessage（変更なし）

**External Dependencies:**
- `tokio`: mpsc channel, select! macro（既存）
- `futures`: SinkExt::feed/flush（既存）

### File Structure

```
src-tauri/src/mux/ipc/
├── connection.rs    # select!ループの修正（Strategy 1 & 2）
```

変更対象は`connection.rs`の`handle_gui_streaming`関数内、約30行の修正。

## Test Scenarios

### Unit Tests
- [ ] `merge_consecutive_chunks`: 同一pane_idの連続チャンクが結合される
- [ ] `merge_consecutive_chunks`: 異なるpane_idは結合されない
- [ ] `merge_consecutive_chunks`: 空データ（exit signal）は結合されない
- [ ] `merge_consecutive_chunks`: 単一チャンクはそのまま返る

### Integration Tests
- [ ] 複数paneからの同時出力がデータ欠損なく転送される
- [ ] バッチ処理中にPtyInputメッセージが遅延なく処理される
- [ ] DRAIN_BATCH_LIMITに達した場合、残りのチャンクが次のイテレーションで処理される

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/` 配下の既存テスト
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] 既存E2Eテストがリグレッションなしで通過
- [ ] (手動) glancesを2タブで同時起動し、フリーズしないことを確認

### Edge Cases
- [ ] 単一paneのみの場合: ドレイン後もチャンク1個の場合が多く、オーバーヘッドが最小
- [ ] 全チャンクが同一pane: マージにより1フレームに結合
- [ ] PTY exit signalがバッチの途中に含まれる場合: exit signalは個別送信
- [ ] DRAIN_BATCH_LIMIT到達: select!に戻り入力を処理してから残りを処理

### Performance Tests
- [ ] `cat /dev/urandom | head -c 100M > /dev/null` のスループット: 現行と同等以上
- [ ] glances 2タブ同時実行時のキー入力遅延: 50ms以下

## Error Handling

### Error Flow

```
framed.feed() error → break select! loop → detach session panes → cleanup
framed.flush() error → break select! loop → detach session panes → cleanup
```

既存の`framed.send().await.is_err()`のエラーハンドリングパターンを`feed()`/`flush()`に適用する。接続断時は既存のdetachフローが発動する。

## Success Criteria

- [ ] glances 2タブ同時実行でフリーズしない
- [ ] `yes` バックグラウンド実行中にアクティブタブで入力が途切れない
- [ ] 既存のUnit/E2Eテストが全て通過
- [ ] detach/reattachが正常動作
- [ ] `cat large_file`のスループットが劣化しない

## Open Questions

なし。全要件が確定済み。

## Implementation Phases

### Phase 1: Output Drain & Batch + Input Priority
**Goals:** フリーズ解消と入力応答性の確保
**Deliverables:**
- `connection.rs`のselect!ループ修正（drain + biased）
- `merge_consecutive_chunks`ヘルパー関数
- Unit tests

## References

- tmux: `bufferevent_disable(EV_READ)`によるバックプレッシャー + 1msレンダリング遅延
- Zellij: bounded channel(容量50) + 30msレンダリングインターバル
- GNU Screen: `obuflimit`(256B)による出力バッファ制限
- WezTerm: `mux_output_parser_coalesce_delay_ms`(3ms)による出力結合
