---
title: "tmux-sockets-discover-flake"
created_date: 2026-08-12
status: draft
---

# tmux-sockets-discover-flake - 要件定義書

## 1. 概要

### 1.1 背景

`src-tauri/src/tmux_sockets.rs` のテスト `discover_returns_only_the_live_socket`
(`src-tauri/src/tmux_sockets.rs:546`) が、既定の並列テストスレッドで
`cargo test --lib` スイート全体を実行したときに非決定的に失敗する。

失敗の機序は fork 窓レースである。並行して走る別テストの `Command::spawn` が
clone から exec までの窓の間、親プロセスの fd テーブルのコピーを一時的に保持する。
そのため in-process で drop 済みの stale listener のソケットが生存し続け、
`probe_unix_socket` (`src-tauri/src/tmux_sockets.rs:105`) の connect が
ECONNREFUSED にならず成功してしまう。

### 1.2 目的

上記テストを既定の並列実行下で決定的に成功させ、無関係な変更に対して CI および
ローカルのフルスイート実行が失敗しないようにする。

### 1.3 スコープ

**対象**:

- `discover_returns_only_the_live_socket` の並列実行下での安定化
- 既存の discover / enumerate テストが持つ「stale ソケットを除外する」検証意図の維持

**対象外**:

- 本番の `discover` / `probe_unix_socket` の挙動、およびその「決して失敗しない」契約の変更
- 本番の chooser が理論上見せうる一時的な stale 表示の是正
  （テスト側の安定化のみでは不十分と判明した場合を除く）

## 2. ビジネス要件

### 2.1 ビジネス目標

- 既定の並列テストスレッドで `cargo test --lib` を実行した際の
  `tmux_sockets::tests::discover_returns_only_the_live_socket` の非決定的失敗を
  なくし、CI およびローカルのフルスイート実行が無関係な変更で失敗しない状態にする。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| 開発者 | ローカルで `cargo test --lib` のフルスイートを実行する |
| CI | 変更ごとにフルスイートを実行する |

### 2.3 期待される効果

- 無関係な変更に対してフルスイートが失敗しなくなる。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 並列でフルスイートを実行する | 開発者 / CI | 高 |

### 3.2 ユースケース詳細

#### UC01: 並列でフルスイートを実行する

**アクター**: 開発者 / CI

**事前条件**:

- Unix 環境である（本モジュールは raw libc ソケットを使う `#[cfg(unix)]` 限定）。

**基本フロー**:

1. 既定の並列テストスレッドで
   `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
   を実行する。
2. `tmux_sockets` のテスト群が、fork 窓レースの有無にかかわらず成功する。

**代替フロー**:

- 並行テストの `Command::spawn` が clone から exec までの窓に入っている状態で
  stale ソケットが probe される。この場合も判定結果は変わらない。

**事後条件**:

- スイートが成功する。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 並列実行下での `discover_returns_only_the_live_socket` の安定化 | fork 窓レースがあってもテストが決定的に成功する | 高 |
| FR2 | stale 除外の検証意図の維持 | 既存テストの stale 除外アサーションを弱めない | 高 |

### 4.2 機能詳細

#### FR1: 並列実行下での `discover_returns_only_the_live_socket` の安定化

**説明**: テスト `discover_returns_only_the_live_socket`
(`src-tauri/src/tmux_sockets.rs:546`) が、既定の並列テストスレッドで
`--lib` スイート全体を実行したときに決定的に成功すること。防御対象として特定されて
いる fork 窓レースは次のとおり: 並行するテストの `Command::spawn`
（clone から exec までの窓）が親の fd テーブルのコピーを一時的に保持し、drop 済みの
stale listener のソケットを生存させるため、`probe_unix_socket`
(`src-tauri/src/tmux_sockets.rs:105`) が ECONNREFUSED を得られず connect に成功
してしまう。

**ステータス**: resolved

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| stale ソケットへの connect が成功する | 並行テストの `Command::spawn` が clone-to-exec 窓にあり、drop 済み listener の fd コピーを保持している | この状況でもテストが失敗しないようにする |

#### FR2: stale 除外の検証意図の維持

**説明**: 既存の discover / enumerate テストが、stale（listen していない）ソケット
ファイルを除外し live なソケットのみを返すこと（本モジュールの AC-1）を引き続き検証
すること。本修正はそのアサーションを弱めたり削除したりしない。

**ステータス**: resolved

## 5. 非機能要件

### 5.1 パフォーマンス要件

該当なし。

### 5.2 セキュリティ要件

該当なし。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

#### NFR1: 並列実行下で成立する修正

安定化は既定の並列テストスレッド構成のもとで達成すること。スイート全体を
`--test-threads=1` に強制することに依存しない。

**ステータス**: resolved

### 5.5 互換性要件

#### NFR2: 本番挙動の不変

本番の `discover` / `probe_unix_socket` の挙動およびその「決して失敗しない」契約は
変更を要しない。本番 chooser の理論上の一時的な stale 表示は、テスト側の安定化のみ
では不十分と判明しない限り明示的にスコープ外とする。

**ステータス**: resolved

## 6. UI/UX要件

該当なし。ヘッドレスな `#[cfg(unix)]` Rust モジュール（tmux ソケット探索）の
テスト安定化であり、UI 面・視覚成果物・デザインシステムへの関与はない。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 本モジュールは Unix 限定（raw libc ソケット probe）であり、Windows のテスト経路は
  影響を受けない。
- 既定の並列テストスレッドのもとで安定させる（スイート全体の逐次実行に依存しない）。

### 9.2 ビジネス上の制約

- 本番 chooser の一時的な stale 表示の是正は、テスト側の安定化で足りる場合はスコープ外。

### 9.3 スケジュール制約

該当なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| 並行テストの `Command::spawn` が clone-to-exec 窓で fd テーブルのコピーを保持し、drop 済み stale listener が生存する | 高 | この窓の存在を前提に、テストが決定的に成功するよう安定化する |

### 10.2 ビジネスリスク

| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| テスト側の安定化のみでは不十分だった場合 | 低 | 中 | その場合に限り本番 chooser の一時的 stale 表示の是正をスコープに戻す |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] `discover_returns_only_the_live_socket` が、並列でのストレス実行 60 回以上で
      失敗 0 件であること。
- [ ] 既存の discover / enumerate テストの意図（stale ソケットが結果から除外される）が
      弱められていないこと。
- [ ] `src-tauri` の `cargo test --lib` フルスイートが引き続き成功すること。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] ストレス: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib tmux_sockets`
      を（fork の多い兄弟テストと並列で）60 回以上繰り返し、失敗 0 件であること。
- [ ] リグレッション: stale なソケットファイル（listener を bind 後に drop し、ファイルは
      ディスク上に残した状態）が `discover_in` の結果から引き続き除外され、live な
      ソケットが返ること。
- [ ] フルスイート: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
      が成功すること。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| fork 窓 / clone-to-exec 窓 | `Command::spawn` において子プロセスが clone されてから exec するまでの間。この間、子は親の fd テーブルのコピーを保持する |
| stale ソケット | ソケットファイルはディスク上に残っているが、listener が既に存在しない状態 |
| live ソケット | listener が生存しており connect できる状態 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] テスト側の安定化を成果物とすること: ユーザーはテスト側の安定化で足りる場合、
      本番 chooser の一時的 stale 表示の是正を明示的にスコープ外とした。
- [x] 対象プラットフォーム: 本モジュールは Unix 限定（raw libc ソケット probe）であり、
      Windows のテスト経路は影響を受けない。
- [x] flake の機序: タスク記述にある機序（SOCK_CLOEXEC な fd のコピーが、並行する
      `Command::spawn` の clone-to-exec 窓を生き延びる）は確認済みで、これが防御対象。
      ソースの内容とも整合する — stale listener は
      `src-tauri/src/tmux_sockets.rs:555` で in-process に drop され、直後に connect で
      probe される。

### 14.2 未確認・保留事項

なし。

## 15. 参考資料

- `src-tauri/src/tmux_sockets.rs`: 対象モジュール（`probe_unix_socket` は 105 行目、
  `discover_returns_only_the_live_socket` は 546 行目、stale listener の drop は 555 行目）
