# Implementation Plan: Mux Output Throughput Optimization

## Overview

muxデーモンのselect!ループにおけるPTY出力のバッチ処理と入力優先制御を実装し、高頻度出力時のフリーズを解消する。

## Objectives

- select!ループ内でチャネルをドレインし、溜まったチャンクをバッチ送信する
- biased select!でクライアント入力を出力より優先する
- 同一paneの連続チャンクをマージしてIPCフレーム数を削減する

## Prerequisites

### Development Environment
- Rust toolchain (existing)
- Docker (for test execution)

### Dependencies
- `tokio::sync::mpsc` (existing)
- `futures::SinkExt` — `feed()` / `flush()` (existing)

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Framework**: Tokio async runtime
- **Key Libraries**: tokio (async), futures (SinkExt), tokio-util (Framed codec)

### Design Approach

現行のselect!ループは1チャンクずつ`send()`（= feed + flush）している。これを「ドレイン → マージ → feed × N → flush × 1」に変更する。biased select!で入力メッセージを常に優先し、出力フラッド中も応答性を維持する。

### Component Interaction

```
pane_output_rx (shared mpsc channel)
       │
       ▼
  drain_and_batch (new logic in select! branch)
       │ recv() + try_recv() loop
       │ merge consecutive same-pane chunks
       ▼
  framed (Framed<UnixStream, MuxCodec>)
       │ feed() × N + flush() × 1
       ▼
  Unix socket → bridge → frontend
```

## Implementation Phases

### Phase 1: Output Drain & Batch + Input Priority

**Goal**: select!ループのPTY出力処理をバッチ化し、入力を優先する

**Files to Modify**:
- `src-tauri/src/mux/ipc/connection.rs` — select!ループのPTY出力ブランチをドレイン+バッチ方式に変更、biased追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| drain logic | チャネルから溜まったチャンクを一括取得 | recv()で最初の1チャンクが到着 | 最大DRAIN_BATCH_LIMITチャンクがVecに収集される |
| merge_consecutive_chunks | 同一pane_idの連続チャンクを結合 | チャンクのVec（順序保証済み） | pane境界またはexit signalで分割された結合チャンクのVec |
| batch send | feed() × N + flush() × 1で一括送信 | マージ済みチャンクのVec | 全チャンクがIPCソケットに書き出される |
| biased select! | 入力メッセージを出力より優先 | select!ループ開始時 | framed.next()ブランチが常に先にポーリングされる |

**Processing Flow**:
1. select!でpane_output_rx.recv()が発火
2. 最初のチャンクを取得
3. try_recv()ループで残りをドレイン（上限: DRAIN_BATCH_LIMIT = 64）
4. merge_consecutive_chunks で同一pane連続チャンクを結合
   - 同一pane_idが連続 → dataをconcat
   - pane_idが変わる or data が空（exit signal） → 新しいチャンクとして分離
5. 各チャンクをframed.feed()でバッファに積む
   - 空データ → PtyExitedメッセージとしてfeed
   - 非空データ → PtyOutputメッセージとしてfeed
6. framed.flush()で一括書き出し
7. select!に戻る（biasedにより、入力があれば先に処理）

**Implementation Steps**:
1. **定数追加** — DRAIN_BATCH_LIMIT定数を定義（64: チャネル容量256の1/4。1バッチの最大メモリ使用量を約4MBに抑えつつ、select!の入力チェック頻度を確保するバランス値）
2. **merge_consecutive_chunks関数** — 同一pane連続チャンクの結合ロジックを実装
3. **select!ブランチ修正** — recv() + try_recv()ドレイン、マージ、feed/flushバッチ送信に変更
4. **biased追加** — select!マクロにbiasedキーワードを追加し、入力ブランチを最上位に配置
5. **Unit tests** — merge_consecutive_chunksのテスト

**Dependencies**: なし（Phase 1のみ）

**Testing Approach**:
- Unit: merge_consecutive_chunksの各パターン（同一pane結合、異pane分離、exit signal分離、単一チャンク、バッチ途中のexit signal、複数pane混在時の順序保証）
- Integration: 既存Rustテスト（cargo test）でリグレッションなし確認
- E2E (Docker): 既存E2Eテストの通過
- Manual: glances 2タブ同時実行、yes バックグラウンド実行中の入力応答性

**Acceptance Criteria**:
- [ ] glances 2タブ同時実行でフリーズしない
- [ ] yes バックグラウンド実行中にアクティブタブで入力が途切れない
- [ ] 既存テストが全て通過
- [ ] detach/reattachが正常動作

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/mux/ipc/
├── connection.rs    # Modified: select! loop batch + biased + merge helper
```

## Testing Strategy

- Unit: merge_consecutive_chunks の6パターン
- Integration: cargo test（既存テスト群）
- E2E (Docker): ./scripts/run-e2e-docker.sh test
- Manual: 高頻度出力アプリでのフリーズ確認、入力応答性確認

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| tokio | existing | mpsc channel, select! macro |
| futures | existing | SinkExt (feed/flush) |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| feed()のバッファが大きくなりすぎてメモリ圧迫 | Low | Medium | DRAIN_BATCH_LIMIT(64)で上限を設け、各チャンクは最大65KBなのでバッチ全体で最大4MB |
| biased select!による出力starvation | Low | Low | DRAIN_BATCH_LIMITで1イテレーションの出力量を制限。入力は通常低頻度で、出力処理の機会は十分にある |
| merge時のデータ順序逆転 | Very Low | High | mergeは入力Vecの順序を維持し、異なるpane間はマージしない |

## Open Questions

なし。

## Success Metrics

- [ ] 高頻度出力（glances等）を複数タブで実行してもフリーズしない
- [ ] 出力フラッド中のキー入力遅延が体感上問題ない
- [ ] cat large_fileのスループットが劣化しない
- [ ] 全既存テスト通過
