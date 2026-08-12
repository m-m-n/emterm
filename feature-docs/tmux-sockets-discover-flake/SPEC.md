# Feature: tmux-sockets-discover-flake

## Overview

`src-tauri/src/tmux_sockets.rs` のテスト `discover_returns_only_the_live_socket`
(`src-tauri/src/tmux_sockets.rs:546`) が、既定の並列テストスレッドでの
`cargo test --lib` において非決定的に失敗する。原因は fork 窓レースであり、並行する
テストの `Command::spawn`（clone から exec までの窓）が親の fd テーブルのコピーを
一時的に保持することで、drop 済み stale listener のソケットが生存し、
`probe_unix_socket` (`src-tauri/src/tmux_sockets.rs:105`) の connect が
ECONNREFUSED にならず成功してしまう。本仕様はこの flake をテスト側で解消し、既存の
stale 除外検証と本番挙動をそのまま保つことを定める。

要件の出所は `feature-docs/tmux-sockets-discover-flake/REQUIREMENTS.md`。

## Objectives

- 既定の並列テストスレッドでの `cargo test --lib` において、
  `tmux_sockets::tests::discover_returns_only_the_live_socket` の非決定的失敗を
  なくし、CI およびローカルのフルスイート実行が無関係な変更で失敗しないようにする。

## User Stories

### US1: 並列フルスイートが決定的に成功する

開発者 / CI として、既定の並列テストスレッドでフルスイートを実行したときに
`tmux_sockets` の discover テストが決定的に成功してほしい。無関係な変更で
ビルドが赤くならないようにするため。

**Acceptance Criteria:**

- [ ] `discover_returns_only_the_live_socket` が並列でのストレス実行 60 回以上で
      失敗 0 件である。
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      のフルスイートが成功する。

### US2: stale 除外の検証が保たれる

開発者として、flake 解消後も「stale ソケットは discover の結果から除外される」ことを
テストが引き続き検証してほしい。モジュール本来の契約（AC-1）を守るため。

**Acceptance Criteria:**

- [ ] 既存の discover / enumerate テストの意図（stale ソケットが結果から除外される）が
      弱められていない。

## Technical Requirements

### Functional Requirements

- **FR1:** 並列実行下での `discover_returns_only_the_live_socket` の安定化 —
  テスト `discover_returns_only_the_live_socket`
  (`src-tauri/src/tmux_sockets.rs:546`) は、既定の並列テストスレッドで `--lib`
  スイート全体を実行したときに決定的に成功すること。特定済みの fork 窓レース、すなわち
  並行するテストの `Command::spawn`（clone から exec までの窓）が親の fd テーブルの
  コピーを一時的に保持し、drop 済み stale listener のソケットを生存させることで
  `probe_unix_socket` (`src-tauri/src/tmux_sockets.rs:105`) が ECONNREFUSED では
  なく connect 成功を得てしまう状況においても、これを満たすこと。
  (status: resolved)

- **FR2:** stale 除外の検証意図の維持 — 既存の discover / enumerate テストは、stale
  （listen していない）ソケットファイルが除外され live なソケットのみが返ることを
  引き続き検証すること（本モジュールの AC-1）。本修正はそのアサーションを弱めたり
  削除したりしないこと。
  (status: resolved)

### Non-Functional Requirements

- **NFR1 - 並列実行下で成立する修正:** 安定化は既定の並列テストスレッド構成のもとで
  達成すること。スイート全体を `--test-threads=1` に強制することに依存しないこと。
  (status: resolved)

- **NFR2 - 本番挙動の不変:** 本番の `discover` / `probe_unix_socket` の挙動および
  その「決して失敗しない」契約は変更を要しない。本番 chooser の理論上の一時的な
  stale 表示は、テスト側の安定化のみでは不十分と判明しない限り明示的にスコープ外と
  する。
  (status: resolved)

## Implementation Approach

### Architecture

対象は `src-tauri/src/tmux_sockets.rs` 単一モジュールのテスト経路。アプリケーション
層・UI 層は関与しない。

```
cargo test --lib (並列テストスレッド)
        │
        ├── tmux_sockets::tests::discover_returns_only_the_live_socket
        │        │  live socket を bind、stale listener を bind 後 drop
        │        └── discover_in → probe_unix_socket (connect)
        │
        └── 並行テスト: Command::spawn
                 └── clone-to-exec 窓で親 fd テーブルのコピーを保持
                     → drop 済み stale listener の fd が一時的に生存
```

**Component Diagram:**

- `probe_unix_socket` (`src-tauri/src/tmux_sockets.rs:105`) — connect による
  live 判定。本番挙動は NFR2 により不変。
- `discover_in` — probe 結果に基づき live なソケットのみを返す。
- `discover_returns_only_the_live_socket` (`src-tauri/src/tmux_sockets.rs:546`) —
  安定化の対象。stale listener は同 555 行目で in-process に drop され、直後に
  probe される。

### Data Flow

```
テスト: listener を bind → stale 側を drop → discover_in
        → probe_unix_socket が connect
        → ECONNREFUSED なら stale として除外 / 成功なら live として採用
```

fork 窓レース下では、drop 済み listener の fd コピーが並行 `Command::spawn` の
clone-to-exec 窓に残るため、connect が成功して stale が live と誤判定される。

### API Design

該当なし（外部 API 面の変更はない）。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**

- `src-tauri/src/tmux_sockets.rs`: 対象モジュール（テスト経路）。

**External Dependencies:**

- libc（raw Unix socket probe）: 本モジュールは Unix 限定であり、Windows のテスト
  経路は影響を受けない。

### File Structure

```
src-tauri/src/
└── tmux_sockets.rs      # probe_unix_socket:105 / discover_in /
                         # discover_returns_only_the_live_socket:546
                         # (stale listener drop: 555)
```

## Test Scenarios

### Unit Tests

- [ ] **TS2** (FR2): stale なソケットファイル（listener を bind 後に drop し、ファイルは
      ディスク上に残した状態）が `discover_in` の結果から引き続き除外され、live な
      ソケットが返ること。

### Integration Tests

- [ ] **TS3** (FR1, FR2, NFR2): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      のフルスイートが成功すること。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Edge Cases

- [ ] 並行テストの `Command::spawn` が clone-to-exec 窓にあり、drop 済み stale
      listener の fd コピーを保持している状態で probe が走る — この状況でも判定結果が
      変わらないこと（FR1）。

### Performance Tests

- [ ] **TS1** (FR1, NFR1) ストレス: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib tmux_sockets`
      を（fork の多い兄弟テストと並列で）60 回以上繰り返し、失敗 0 件であること。

## Security Considerations

該当なし。

## Error Handling

`probe_unix_socket` の ECONNREFUSED は stale 判定のシグナルであり、エラー処理面の
仕様変更はない（NFR2）。

## Performance Optimization

該当なし。

## Success Criteria

- [ ] `discover_returns_only_the_live_socket` が、並列でのストレス実行 60 回以上で
      失敗 0 件である。
- [ ] 既存の discover / enumerate テストの意図（stale ソケットが結果から除外される）が
      弱められていない。
- [ ] `src-tauri` の `cargo test --lib` フルスイートが引き続き成功する。
- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（FR1 / FR2 / NFR1 / NFR2 はすべて `status: resolved`）。

## Assumptions

- テスト側の安定化を成果物とすること。ユーザーはテスト側の安定化で足りる場合、本番
  chooser の一時的な stale 表示の是正を明示的にスコープ外とした。
- 本モジュールは Unix 限定（raw libc ソケット probe）であり、Windows のテスト経路は
  影響を受けない。
- タスク記述にある flake の機序（SOCK_CLOEXEC な fd のコピーが、並行する
  `Command::spawn` の clone-to-exec 窓を生き延びる）は確認済みで、これが防御対象。
  ソースが示す内容とも整合する — stale listener は
  `src-tauri/src/tmux_sockets.rs:555` で in-process に drop され、直後に connect で
  probe される。

## References

- 要件定義書: `feature-docs/tmux-sockets-discover-flake/REQUIREMENTS.md`
- 対象モジュール: `src-tauri/src/tmux_sockets.rs`
