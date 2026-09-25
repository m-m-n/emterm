---
title: "sftp-send-scan-crlf"
created_date: 2026-09-25
status: draft
---

# sftp-send-scan-crlf - 要件定義書

## 1. 概要

### 1.1 背景
Windows（`core.autocrlf=true`）で `cargo test --lib` を実行すると、SFTP send-site 構造検査テストが無関係な変更でも赤になる。

### 1.2 目的
- SFTP send-site 構造検査テストが、作業コピーの改行コード（LF / CRLF）に関係なく同じ結果を出すようにする
- Windows（`core.autocrlf=true`）で `cargo test --lib` を実行したとき、無関係な変更でも赤になる状態をなくす

### 1.3 スコープ
- 対象: `src-tauri/src/sftp/service.rs` の `#[cfg(test)] mod tests` 内の `scan_for_bare_send_sites` と、そのテスト `send_site_scan_detection_routine_red_cases`
- 対象外: 本番コード、`#[cfg(test)] impl SftpService` の seam ブロック、公開 API
- 対象外: `.gitattributes` の追加など、リポジトリ側で改行コードを固定する対応

## 2. ビジネス要件

### 2.1 ビジネス目標
- SFTP send-site 構造検査テストが、作業コピーの改行コード（LF / CRLF）に関係なく同じ結果を出すようにする
- Windows（`core.autocrlf=true`）で `cargo test --lib` を実行したとき、無関係な変更でも赤になる状態をなくす

### 2.2 対象ユーザー
該当なし

### 2.3 期待される効果
- 作業コピーの改行コードが LF でも CRLF でも、構造検査テストの結果が同じになる

## 3. ユースケース

該当なし

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 改行コードに依存しない境界検出 | LF / CRLF / 混在のいずれでも `mod tests` の境界を検出する | 高 |
| FR2 | 正規化後の文字列でオフセットを取る | 先頭で `\r\n` を `\n` に正規化し、境界検出と走査範囲の切り出しを正規化後の文字列で行う | 高 |
| FR3 | CRLF の回帰テスト | `send_site_scan_detection_routine_red_cases` に CRLF のケースを追加する | 高 |
| FR4 | 既存の検出挙動を保つ | 既存の red ケース (i)〜(iv) が変更後もそのまま通る | 高 |

### 4.2 機能詳細

#### FR1: 改行コードに依存しない境界検出

**説明**: `scan_for_bare_send_sites` は、入力テキストの改行が LF でも CRLF でも（混在していても）`#[cfg(test)]` の次の行に `mod tests` が続く境界を検出する。境界より前の領域での bare send 検出結果は、同じ内容を LF で与えたときと同じになる。

#### FR2: 正規化後の文字列でオフセットを取る

**説明**: `scan_for_bare_send_sites` は、先頭で `\r\n` を `\n` に置き換えて改行を正規化し、境界検出と走査範囲の切り出し（`&src[..boundary]`）の両方を正規化後の文字列に対して行う。

#### FR3: CRLF の回帰テスト

**説明**: `send_site_scan_detection_routine_red_cases` に CRLF 改行の合成テキストのケースを追加する。このケースは、境界が見つかること（`MissingBoundary` にならないこと）と、境界より前にある bare send を検出することを固定する。

#### FR4: 既存の検出挙動を保つ

**説明**: 既存の red ケース (i)〜(iv)（境界より前の progress / result の bare send を検出する、`mod tests` 本体の中の send は報告しない、`mod tests` が無ければ `MissingBoundary`）は、変更後もそのまま通る。

## 5. 非機能要件

### 5.1 パフォーマンス要件
該当なし

### 5.2 セキュリティ要件
該当なし

### 5.3 可用性要件
該当なし

### 5.4 保守性要件
- NFR1: 変更は `src-tauri/src/sftp/service.rs` の `#[cfg(test)] mod tests` 内だけに限る。本番コード、`#[cfg(test)] impl SftpService` の seam ブロック、公開 API は変えない。
- NFR2: 禁止される送信の形（`progress_tx` + `.send(` / `result_tx` + `.send(`）は、今と同じように部品を連結して組み立てる。検出ルーチンと追加するテストのソースに、その形をそのまま書かない。

### 5.5 互換性要件
該当なし

## 6. UI/UX要件

該当なし

## 7. データ要件

該当なし

## 8. 外部連携

該当なし

## 9. 制約条件

### 9.1 技術的制約
- 変更は `src-tauri/src/sftp/service.rs` の `#[cfg(test)] mod tests` 内だけに限る（NFR1）
- 禁止される送信の形は部品を連結して組み立て、ソースにそのまま書かない（NFR2）
- `find_mod_tests_boundary` のシグネチャと、アンカー条件（`#[cfg(test)]` の次の行に `mod tests`）は変えない（A-2）

### 9.2 ビジネス上の制約
該当なし

### 9.3 スケジュール制約
該当なし

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/sftp-send-scan-crlf/**`
- `test-docs/sftp-send-scan-crlf/**`

`feature-docs/sftp-send-scan-crlf/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/sftp-send-scan-crlf/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/sftp-send-scan-crlf/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/sftp-send-scan-crlf/` ディレクトリを生成しないが、宣言された `test-docs/sftp-send-scan-crlf/**` は依然として正しい。

## 10. 想定される課題とリスク

該当なし

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC-1: `core.autocrlf=true` でチェックアウトした作業コピー（`service.rs` が CRLF）で `cargo test --lib` を実行すると、`every_progress_and_result_send_site_routes_through_the_wake_helpers` が通る。（FR1, FR2）
- [ ] AC-2: `send_site_scan_detection_routine_red_cases` に CRLF の合成テキストのケースがあり、境界が見つかることと、境界より前の bare send を検出することを確認している。（FR3）
- [ ] AC-3: LF の作業コピーで `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` がすべて通り、既存の red ケース (i)〜(iv) も通る。（FR4, NFR1）

### 11.2 KPI
該当なし

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] TS-1（単体・AC-2）: CRLF の合成テキスト（`#[cfg(test)]\r\nimpl SftpService {...bare send...}\r\n\r\n#[cfg(test)]\r\nmod tests {}\r\n` の形）を `scan_for_bare_send_sites` に渡すと、`Found` になり、bare send が検出される。
- [ ] TS-2（単体・AC-3）: 既存の red ケース (i)〜(iv) と、実ソースを読む構造検査テストが LF の作業コピーで通る。
- [ ] TS-3（手動・AC-1）: Linux 上で `core.autocrlf=true` を指定して別の作業コピーを作り（Linux でも checkout 時に CRLF へ変換される）、`service.rs` が CRLF であることを確かめてから `--lib` テストを実行し、構造検査テストが通ることを確認する。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| bare send | 境界より前の領域にある、禁止される送信の形（`progress_tx` + `.send(` / `result_tx` + `.send(`） |
| 境界 | `#[cfg(test)]` の次の行に `mod tests` が続く位置 |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A-1: 対象にする改行コードは LF と CRLF（混在を含む）だけにする。CR 単独の改行は対象にしない。
- [x] A-2: 修正方法には、`scan_for_bare_send_sites` の先頭で `\r\n` を `\n` に正規化し、正規化後の文字列でオフセットを取る方法を採用する。`find_mod_tests_boundary` のシグネチャと、アンカー条件（`#[cfg(test)]` の次の行に `mod tests`）は変えない。
- [x] A-3: `.gitattributes` の追加など、リポジトリ側で改行コードを固定する対応はこの feature の範囲外とする。

### 14.2 未確認・保留事項
なし

## 15. 参考資料

- `src-tauri/src/sftp/service.rs`
