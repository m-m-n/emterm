# Feature: inherited-pty-drop-fd-tiocgptn

## Overview

`mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor` の閉鎖判定を、`st_rdev` の比較から `ioctl(TIOCGPTN)` による pts 番号の比較に置き換える。あわせて、閉鎖判定を決定的に検証する再発検出テストを追加し、誤った前提を書いたコメントと参照を直す。要件の詳細は `REQUIREMENTS.md` を参照する。

## Objectives

- `--lib` 全体の実行で `mux::inherited_pty::tests::dropping_the_adapter_closes_the_owned_descriptor` が間欠的に失敗しない状態にし、無関係な変更の検証結果を読める状態に戻す
- drop 後の fd 番号が別の PTY master（またはその複製）に再利用されても、元の記述子が閉じたことを正しく判定する

## User Stories

### US1: `--lib` 全体のテスト結果を読める状態に戻す
開発者として、`--lib` 全体のテストを実行したときに対象テストが間欠的に失敗しない状態にしたい。無関係な変更の検証結果を読めるようにするため。

**Acceptance Criteria:**
- [ ] AC-1（FR1, FR5）: 対象テストが TIOCGPTN による判定で成功し、`st_rdev` の比較を含まない。
- [ ] AC-3（FR2）: 対象テストの失敗メッセージに fd 番号、drop 前の pts 番号、drop 後の取得結果（pts 番号または errno）が含まれる。
- [ ] AC-5: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` で inherited_pty の全テストが成功し、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が成功する。

### US2: fd 番号の再利用に左右されない閉鎖判定
開発者として、drop 後の fd 番号が別の PTY master（またはその複製）に再利用されても、元の記述子が閉じたことを正しく判定したい。

**Acceptance Criteria:**
- [ ] AC-2（FR3）: 再発検出テストが、別の PTY pair の master とその複製を元の記述子と区別し、元の master の複製は同一と判定し、slave・通常ファイル・閉じた番号を元の記述子ではないと判定する。`st_rdev` ベースの判定に差し替えると失敗する（red を確認できる）。
- [ ] AC-4（FR4）: `inherited_pty.rs` に「再利用された記述子は別のデバイスを指す」という前提のコメントが残らず、`upgrade/tests.rs` に存在しない `stat_rdev` への参照が残らない。

## Technical Requirements

### Functional Requirements
- **FR1:** 閉鎖判定の識別子を TIOCGPTN に置き換える。対象テストは drop 前に `dup_fd` の `ioctl(TIOCGPTN)` で pts 番号を取得する（取得できなければテストを失敗させる）。drop 後に `dup_fd` へ `ioctl(TIOCGPTN)` を呼び、失敗する、または drop 前と異なる番号を返す場合は閉じたと判定する。drop 前と同じ番号を返す場合はテストを失敗させる。`st_rdev` の比較（`assert_ne!(rdev, original_rdev)`）は使わない。
- **FR2:** 失敗メッセージに診断情報を含める。対象テストの失敗メッセージに、fd 番号、drop 前の pts 番号、drop 後に `TIOCGPTN` で取得した値（成功時は pts 番号、失敗時は errno）を含める。
- **FR3:** 再発を検出するテスト。閉鎖判定（FR1 の判定ロジック）が、スレッド間の競合に頼らず決定的に次を満たすことを検証するテストを追加する: 別の PTY pair の master とその複製を「元の記述子ではない」と判定する、元の master の複製を「元の記述子である」と判定する、PTY slave・通常ファイル・閉じた番号を「元の記述子ではない」と判定する。`st_rdev` による判定では別 pair の master を区別できず、このテストが失敗する。
- **FR4:** 誤った前提を書いたコメントと参照を直す。`src-tauri/src/mux/inherited_pty.rs` のコメント `:260-265` と `:466-471`（再利用された記述子は別のデバイスを指すという前提）を、TIOCGPTN による判定の根拠に合わせて書き換える。`stat_rdev` が使われなくなる場合は削除し、`src-tauri/src/mux/upgrade/tests.rs:43` の `inherited_pty.rs::tests::stat_rdev` への参照を実態に合わせて更新する。
- **FR5:** 元の pair を判定完了まで開いたままにする。対象テストは `_pair`（元の PTY pair）を drop 後の判定が終わるまでスコープ内に保持する。これにより元の pts 番号が他の pair に割り当てられない。

### Non-Functional Requirements
- **NFR1 - 変更範囲:** 変更はテストコード（`#[cfg(test)]` 内）に限る。`InheritedMasterPty` 本体と `Drop` の挙動は変えない。
- **NFR2 - 決定性:** 追加・変更するテストは CPU 負荷・反復実行・`sleep` によるタイミング調整に依存しない。
- **NFR3 - テスト名の維持:** 既存テスト名 `dropping_the_adapter_closes_the_owned_descriptor` を変更しない。
- **NFR4 - ビルドとテスト:** `--lib` 全体のテストと `--no-default-features` のビルドチェックが通る。モジュールの unix 限定の前提を維持する。

## Implementation Approach

### Architecture

**System Architecture:**
```
src-tauri/src/mux/inherited_pty.rs
└── #[cfg(test)] mod tests
    ├── dropping_the_adapter_closes_the_owned_descriptor   (FR1, FR2, FR5)
    ├── 再発検出テスト                                       (FR3)
    └── コメント :260-265 / :466-471                          (FR4)

src-tauri/src/mux/upgrade/tests.rs
└── :43 の stat_rdev 参照コメント                            (FR4)
```

**Component Diagram:**
```
閉鎖判定（FR1）
  入力: 判定対象の fd、drop 前の pts 番号
  ioctl(TIOCGPTN) 失敗          → 元の記述子ではない（閉じた）
  drop 前と異なる pts 番号      → 元の記述子ではない（閉じた）
  drop 前と同じ pts 番号        → 元の記述子である
```

### Data Flow

```
_pair を開く（判定完了まで保持: FR5）
  → dup_fd に ioctl(TIOCGPTN) → drop 前の pts 番号（取得失敗ならテスト失敗）
  → adapter を drop
  → dup_fd に ioctl(TIOCGPTN)
      失敗 / 異なる番号 → 閉じたと判定（成功）
      同じ番号          → テスト失敗（fd 番号・drop 前の pts 番号・取得結果を出力: FR2）
```

### API Design

該当なし

### Database Schema

該当なし

### Dependencies

**Internal Dependencies:**
- `slave_device_path`（`inherited_pty.rs:218-230`）: 同じテストモジュールですでに `libc::TIOCGPTN` を使っている

**External Dependencies:**
- `libc`: `libc::TIOCGPTN` を使う。新しい依存は追加しない

### File Structure

```
src-tauri/src/mux/
├── inherited_pty.rs        # 対象テスト・再発検出テスト・コメント（#[cfg(test)] 内）
└── upgrade/
    └── tests.rs            # :43 の stat_rdev 参照コメント
```

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/inherited-pty-drop-fd-tiocgptn/**`
- `test-docs/inherited-pty-drop-fd-tiocgptn/**`

`feature-docs/inherited-pty-drop-fd-tiocgptn/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/inherited-pty-drop-fd-tiocgptn/**` covers `test-docs/inherited-pty-drop-fd-tiocgptn/{T}.tests.yaml`, the
per-task test record. It is generated and owned by `implement-phase.md`;
this section cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/{feature}/**` entry is still correct in that case — a declared
path that never materializes is not a violation.

## Test Scenarios

### Unit Tests
- [ ] TS-1（AC-1 / FR1, FR5）: 対象テストを単体実行し成功する（`--exact` 指定）。
- [ ] TS-2（AC-2 / FR3）: PTY pair A と B を開き、A の master の pts 番号を基準に、B の master・B の複製・A の複製・A の slave・tempfile・`-1` に対する判定結果を検証する。
- [ ] TS-3（AC-2 / FR3, red check）: 判定を `st_rdev` 比較に戻した状態で TS-2 が失敗することを確認する。
- [ ] TS-6（AC-5 / NFR4）: `--lib` 全体を 1 回実行し、`--no-default-features` の check を 1 回実行する。

### Integration Tests
- 該当なし

### Review
- [ ] TS-4（AC-3 / FR2）: 失敗メッセージの書式に fd 番号と識別子が含まれることをコードで確認する。
- [ ] TS-5（AC-4 / FR4）: `inherited_pty.rs` のコメントと `upgrade/tests.rs:43` の参照をコードで確認する。

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- 該当なし

### Edge Cases
- [ ] 閉じた番号（`-1`）を「元の記述子ではない」と判定する（TS-2）
- [ ] 元の master の複製を「元の記述子である」と判定する（TS-2）

### Performance Tests
- 該当なし（NFR2: CPU 負荷・反復実行・`sleep` によるタイミング調整に依存しない）

## Security Considerations

- 該当なし

## Error Handling

### Error Codes

該当なし

### Error Flow

```
drop 前の ioctl(TIOCGPTN) 失敗          → テスト失敗
drop 後の ioctl(TIOCGPTN) が同じ番号を返す → テスト失敗（fd 番号・drop 前の pts 番号・取得結果を出力）
```

## Performance Optimization

### Performance Goals
- 該当なし

### Optimization Strategies
- 該当なし

### Caching Strategy
- 該当なし

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] AC-1 〜 AC-5 を満たす

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- なし

## Assumptions

- A1: 完了の定義「再現手順で現象が起きない」は、TS-2 が「解放された番号を別の PTY master が取った」状態をプロセス内で決定的に作って検証することで満たす。調査で使った `LD_PRELOAD` 注入ライブラリはリポジトリ外で追跡されていないため、注入による再確認は必須にしない。
- A2: テスト名 `dropping_the_adapter_closes_the_owned_descriptor` は変えない。そのため `.claude/rules/test-docs-records.md` の名前追従による先行 feature の test-docs 更新は発生しない。
- A3: `libc::TIOCGPTN` は同じテストモジュールの `slave_device_path`（`inherited_pty.rs:218-230`）ですでに使われており、現行のビルド対象でコンパイルできる。新しい依存は追加しない。
- A4: 先行 feature `inherited-pty-drop-fd-flake-investigation` の `INVESTIGATION.md` にある `stat_rdev` などの記述は、その時点の記録として変更しない。
- A5: `src-tauri/src/mux/upgrade/tests.rs` の `stat_dev_ino`（通常ファイルの `(st_dev, st_ino)` 比較）は PTY master の問題が当てはまらないため、判定ロジックは変えず、`stat_rdev` への参照コメントだけを直す。
- A6: `InheritedMasterPty` の製品コードは正しく close している（INVESTIGATION.md 6 の (b) 却下）ため、変更しない。

## References

- 要件定義書: `feature-docs/inherited-pty-drop-fd-tiocgptn/REQUIREMENTS.md`
- 先行 feature の調査記録: `feature-docs/inherited-pty-drop-fd-flake-investigation/INVESTIGATION.md`
