# Feature: sftp-send-scan-crlf

## Overview

SFTP send-site 構造検査テストの検出ルーチン `scan_for_bare_send_sites` を、入力テキストの改行コード（LF / CRLF / 混在）に依存しないようにする。要件の詳細は [REQUIREMENTS.md](REQUIREMENTS.md) を参照。

## Objectives

- SFTP send-site 構造検査テストが、作業コピーの改行コード（LF / CRLF）に関係なく同じ結果を出すようにする
- Windows（`core.autocrlf=true`）で `cargo test --lib` を実行したとき、無関係な変更でも赤になる状態をなくす

## User Stories

該当なし

## Technical Requirements

### Functional Requirements
- **FR1:** 改行コードに依存しない境界検出 — `scan_for_bare_send_sites` は、入力テキストの改行が LF でも CRLF でも（混在していても）`#[cfg(test)]` の次の行に `mod tests` が続く境界を検出する。境界より前の領域での bare send 検出結果は、同じ内容を LF で与えたときと同じになる。
- **FR2:** 正規化後の文字列でオフセットを取る — `scan_for_bare_send_sites` は、先頭で `\r\n` を `\n` に置き換えて改行を正規化し、境界検出と走査範囲の切り出し（`&src[..boundary]`）の両方を正規化後の文字列に対して行う。
- **FR3:** CRLF の回帰テスト — `send_site_scan_detection_routine_red_cases` に CRLF 改行の合成テキストのケースを追加する。このケースは、境界が見つかること（`MissingBoundary` にならないこと）と、境界より前にある bare send を検出することを固定する。
- **FR4:** 既存の検出挙動を保つ — 既存の red ケース (i)〜(iv)（境界より前の progress / result の bare send を検出する、`mod tests` 本体の中の send は報告しない、`mod tests` が無ければ `MissingBoundary`）は、変更後もそのまま通る。

### Non-Functional Requirements
- **NFR1 - 変更範囲:** 変更は `src-tauri/src/sftp/service.rs` の `#[cfg(test)] mod tests` 内だけに限る。本番コード、`#[cfg(test)] impl SftpService` の seam ブロック、公開 API は変えない。
- **NFR2 - 禁止形の組み立て:** 禁止される送信の形（`progress_tx` + `.send(` / `result_tx` + `.send(`）は、今と同じように部品を連結して組み立てる。検出ルーチンと追加するテストのソースに、その形をそのまま書かない。

## Implementation Approach

### Architecture

`scan_for_bare_send_sites` の先頭で入力の `\r\n` を `\n` に正規化し、以降の境界検出（`find_mod_tests_boundary`）と走査範囲の切り出し（`&src[..boundary]`）はすべて正規化後の文字列に対して行う（FR2, A-2）。

- `find_mod_tests_boundary` のシグネチャは変えない（A-2）
- アンカー条件（`#[cfg(test)]` の次の行に `mod tests`）は変えない（A-2）
- 対象の改行コードは LF と CRLF（混在を含む）。CR 単独の改行は対象にしない（A-1）

### Data Flow

```
入力テキスト → \r\n を \n に正規化 → find_mod_tests_boundary（正規化後）
            → &normalized[..boundary] を走査 → bare send の検出結果
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/sftp/service.rs` の `#[cfg(test)] mod tests`: `scan_for_bare_send_sites` / `find_mod_tests_boundary` / `send_site_scan_detection_routine_red_cases` / `every_progress_and_result_send_site_routes_through_the_wake_helpers`

**External Dependencies:**
- なし

### File Structure

```
src-tauri/src/sftp/
└── service.rs   # #[cfg(test)] mod tests 内のみ変更（NFR1）
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/sftp-send-scan-crlf/**`
- `test-docs/sftp-send-scan-crlf/**`

`feature-docs/sftp-send-scan-crlf/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/sftp-send-scan-crlf/**` covers `test-docs/sftp-send-scan-crlf/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/sftp-send-scan-crlf/` directory at all; the declared
`test-docs/sftp-send-scan-crlf/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1: CRLF の合成テキスト（`#[cfg(test)]\r\nimpl SftpService {...bare send...}\r\n\r\n#[cfg(test)]\r\nmod tests {}\r\n` の形）を `scan_for_bare_send_sites` に渡す - `Found` になり、bare send が検出される（AC-2 / FR3）
- [ ] TS-2: 既存の red ケース (i)〜(iv) と、実ソースを読む構造検査テストを LF の作業コピーで実行する - すべて通る（AC-3 / FR4, NFR1）

### Integration Tests
該当なし

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected

### Manual Tests
- [ ] TS-3: Linux 上で `core.autocrlf=true` を指定して別の作業コピーを作り（Linux でも checkout 時に CRLF へ変換される）、`service.rs` が CRLF であることを確かめてから `--lib` テストを実行する - 構造検査テストが通る（AC-1 / FR1, FR2）

### Edge Cases
- [ ] LF と CRLF が混在した入力 - 境界を検出し、LF で与えたときと同じ検出結果になる（FR1）

### Performance Tests
該当なし

## Security Considerations

該当なし

## Error Handling

- `mod tests` の境界が無い入力は `MissingBoundary` になる（FR4）
- CRLF の入力で境界がある場合は `MissingBoundary` にならない（FR3）

## Performance Optimization

該当なし

## Success Criteria

- [ ] AC-1: `core.autocrlf=true` でチェックアウトした作業コピー（`service.rs` が CRLF）で `cargo test --lib` を実行すると、`every_progress_and_result_send_site_routes_through_the_wake_helpers` が通る。（FR1, FR2）
- [ ] AC-2: `send_site_scan_detection_routine_red_cases` に CRLF の合成テキストのケースがあり、境界が見つかることと、境界より前の bare send を検出することを確認している。（FR3）
- [ ] AC-3: LF の作業コピーで `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` がすべて通り、既存の red ケース (i)〜(iv) も通る。（FR4, NFR1）

## Assumptions

- A-1: 対象にする改行コードは LF と CRLF（混在を含む）だけにする。CR 単独の改行は対象にしない。
- A-2: 修正方法には、`scan_for_bare_send_sites` の先頭で `\r\n` を `\n` に正規化し、正規化後の文字列でオフセットを取る方法を採用する。`find_mod_tests_boundary` のシグネチャと、アンカー条件（`#[cfg(test)]` の次の行に `mod tests`）は変えない。
- A-3: `.gitattributes` の追加など、リポジトリ側で改行コードを固定する対応はこの feature の範囲外とする。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし

## References

- 要件定義書: [REQUIREMENTS.md](REQUIREMENTS.md)
- 対象ソース: `src-tauri/src/sftp/service.rs`
