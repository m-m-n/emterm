# Verification Document: Mux Output Throughput Optimization

## Overview
**Feature**: Mux Output Throughput Optimization
**SPEC.md**: `doc/tasks/mux-output-throughput/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/mux-output-throughput/IMPLEMENTATION.md`

## Build Verification
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml --no-run"`
- Expected: exit code 0, no compilation errors

## Test Verification
- Command: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"`
- Coverage target: 全既存テスト通過 + merge_consecutive_chunks の新規テスト通過

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | merge_consecutive_chunks: 同一pane_idの連続チャンク | 1つのチャンクに結合される | Unit |
| TS-2 | merge_consecutive_chunks: 異なるpane_id | 結合されず別々のチャンクとして残る | Unit |
| TS-3 | merge_consecutive_chunks: 空データ（exit signal） | 結合されず独立したチャンクとして残る | Unit |
| TS-4 | merge_consecutive_chunks: 単一チャンク | そのまま1要素のVecとして返る | Unit |
| TS-5a | merge_consecutive_chunks: バッチ途中にexit signal | exit前後のデータが正しく分離される | Unit |
| TS-5b | merge_consecutive_chunks: 複数pane混在の順序保証 | pane A→B→Aの順序が維持される | Unit |
| TS-6 | 複数pane同時出力でデータ欠損なし | 全データがフロントエンドに到達 | Manual |
| TS-7 | 出力フラッド中のキー入力応答性 | 入力が遅延なく処理される | Manual |
| TS-8 | DRAIN_BATCH_LIMIT到達 | select!に戻り入力処理の機会が得られる | Unit (implicit in design) |

## Code Quality Verification
- Format: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --manifest-path src-tauri/Cargo.toml -- --check"`
- Static analysis: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"`

## File Structure Verification

### Files to Create
- なし

### Files to Modify
- `src-tauri/src/mux/ipc/connection.rs` — select!ループのバッチ処理化、biased追加、merge_consecutive_chunks関数追加、DRAIN_BATCH_LIMIT定数追加

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | glances 2タブ同時実行でフリーズしない | 手動: glancesを2タブで起動し30秒以上動作確認 |
| SC-2 | yes バックグラウンド実行中にアクティブタブで入力が途切れない | 手動: Tab1でyes実行、Tab2で入力操作 |
| SC-3 | 既存Unit/E2Eテスト全通過 | 自動: cargo test + E2Eテスト |
| SC-4 | detach/reattachが正常動作 | 手動: mux detach後にreattachし状態が復元されること |
| SC-5 | cat large_fileのスループットが劣化しない | 手動: 大きなファイルをcatし体感速度が同等であること |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: select!ループ内でドレイン+バッチ処理 | Phase 1 | TS-1〜4のUnit test + 手動確認 |
| FR2: PtyInput優先（biased select!） | Phase 1 | TS-6 手動確認 |
| FR3: 全pane出力データを欠損なく転送 | Phase 1 | TS-5 手動確認 |
| FR4: detach/reattach/ring buffer/shadow parser維持 | Phase 1 | SC-4 手動確認 |

## E2E Testing (Docker)
- [ ] 既存E2Eテストが全て通過: `./scripts/run-e2e-docker.sh test`

## Manual Testing (E2E Not Possible)
- [ ] glances 2タブ同時実行: 両タブが描画を継続し、タブ切替が応答する
- [ ] yes バックグラウンド: Tab1で`yes`実行中、Tab2でコマンド入力が途切れない
- [ ] Ctrl+C応答性: バックグラウンドタブのプロセスをCtrl+Cで停止できる
- [ ] detach/reattach: mux detach → reattach後に画面状態が復元される
- [ ] 大ファイルcat: `cat`による大量出力のスループットが体感上劣化しない

## Performance Verification
- NFR1 (Latency): 出力フラッド中のキー入力遅延 — 体感50ms以下（手動確認）
- NFR2 (Throughput): cat large_file — 現行と同等以上（手動確認）
- NFR3 (Memory): バッチ処理による追加メモリ — 最大4MB/バッチ（DRAIN_BATCH_LIMIT 64 × 65KB）

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit Tests | 6 | 6 | 0 | 0 |
| Code Quality | 2 | 2 | 0 | 0 |
| Functional | 4 | 0 | 0 | 4 |
| Performance | 3 | 0 | 0 | 3 |
| E2E | 1 | 0 | 1 | 0 |
| **Total** | **17** | **9** | **1** | **7** |
