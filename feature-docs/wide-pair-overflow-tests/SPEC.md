# Feature: wide-pair-overflow-tests

## Overview

wide ペア相方掃除の overflow 分岐（16 バイト超グラフェム、`char_len == 0xFF`）に
ユニットテストを追加する。対象は print 経路の `blank_wide_pair_partner`
（`crates/term_core/src/print_handler.rs:74`）と、DCH / ECH 経路が共有する
`blank_wide_pair_split`（`crates/term_core/src/csi_edit.rs:161`）の 2 本。
テスト追加のみで `crates/term_core` の非テストコードは変更しない。

要件定義書: `feature-docs/wide-pair-overflow-tests/REQUIREMENTS.md`

## Objectives

- overflow テーブルと逆引き index（`overflow_ridx`）の同期が壊れるリグレッションを
  ユニットテストで検知可能にする。
- feature wide-pair-overwrite-cleanup（PR #30）レビュー round1 の deferred finding
  `06ef78e20e9b9f0b`（medium / comprehensive）のカバレッジギャップを解消する。

## User Stories

### US1: overflow 分岐のリグレッション検知

`crates/term_core` の開発者として、wide ペア相方掃除の overflow 分岐にテストが
あってほしい。overflow テーブルと `overflow_ridx` の同期が壊れたときに CI で
気づけるようにするため。

**Acceptance Criteria:**
- [ ] print 経路のテストが存在し、ZWJ 家族絵文字を col0 に書いて base が overflow
      行きになることを確認したうえで、col0 の ASCII 上書き後に col1 の spacer が
      `" "` / width 1 であることを assert している。（AC1）
- [ ] `csi_edit` / `csi_screen` 経路のテストが存在し、同じ ZWJ ペアの spacer 位置で
      DCH / ECH を実行後、col-1 の `get_cell_char` が `" "`（空文字ではない）を返す
      ことを assert している。（AC2）
- [ ] 追加テストが overflow 分岐（`char_len == 0xFF`）を実際に通ることが assert で
      確認されている（掃除前の overflow 状態の事前 assert を含む）。（AC3）
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      および workspace のテストが全件通る。（AC4）

## Technical Requirements

### Functional Requirements

- **FR1 - print 経路の overflow 相方掃除テスト:** 16 バイト超・幅 2 の ZWJ グラフェム
  クラスタ（例: 家族絵文字 👨‍👩‍👧‍👦、25 バイト）を col0 に書き、base セルが overflow
  テーブル行き（`char_len == 0xFF`）になることを確認したうえで col0 を ASCII で
  上書きし、`blank_wide_pair_partner`（`crates/term_core/src/print_handler.rs:74`）
  経由で col1 の spacer が `get_cell_char == " "` かつ `width == 1` になることを
  assert するテストを追加する。
- **FR2 - DCH / ECH 経路の overflow 相方掃除テスト:** 同じ ZWJ ペアの spacer 位置に
  カーソルを置いて DCH（`handle_delete_characters`、`csi_edit.rs`）および ECH
  （`handle_erase_characters`、`csi_screen.rs`）を実行し、`blank_wide_pair_split`
  （`crates/term_core/src/csi_edit.rs:161`）経由で col-1 が `get_cell_char == " "`
  （空文字ではない）を返すことを assert するテストを追加する。
- **FR3 - overflow 分岐の実行証明:** 追加テストは掃除前の対象セルが実際に overflow
  状態であること（`cell.is_overflow()` / overflow テーブルへのエントリ存在）を事前
  assert し、掃除後に overflow と `overflow_ridx` の両方からエントリが除去されている
  ことを assert することで、`char_len == 0xFF` 分岐を通ったことを証明する。
- **FR4 - 既存テストの維持:** 既存の `term_core --lib` テストおよび workspace テストが
  引き続き全件通る。

### Non-Functional Requirements

- **NFR1 - Maintainability（既存テスト規約への準拠）:** テストは既存の inline
  `#[cfg(test)]` モジュール（`crates/term_core/src/print_handler/tests.rs`、
  `csi_edit.rs` / `csi_screen.rs` の `mod tests`）に置き、命名は既存の
  `<subject>_<scenario>_<expected>` パターンに合わせる。新規テストフレームワーク・
  dev-dependency（proptest 等）は導入しない。
- **NFR2 - Maintainability（プロダクションコード非変更）:** 本 feature はテスト追加
  のみで、`crates/term_core` の非テストコードは変更しない。

## Implementation Approach

### Architecture

対象は `crates/term_core` の 3 モジュールと、それらが呼ぶ 2 つの相方掃除
プリミティブ。

```
print 経路                       CSI 経路
─────────────────────────────    ─────────────────────────────
print_handler.rs                 csi_edit.rs        csi_screen.rs
  blank_wide_pair_partner:74       handle_delete_     handle_erase_
                                   characters          characters:155
                                        │                   │
                                        └─── blank_wide_pair_split
                                             (csi_edit.rs:161)
                                                    │
                                  grid: cells / overflow / overflow_ridx
```

**Component Diagram:**

```
テスト対象プリミティブ:
  blank_wide_pair_partner  … print 上書き時に相方（spacer / base）を空白化
  blank_wide_pair_split    … DCH / ECH で wide ペアが分断されたときに片側を空白化

検証対象状態:
  cell.char_len == 0xFF    … overflow 行きの印
  overflow                 … 16 バイト超グラフェム実体テーブル
  overflow_ridx            … overflow への逆引き index
```

### Data Flow

```
ZWJ 家族絵文字を print → base(col0) が overflow 行き（char_len = 0xFF）+ spacer(col1)
   → 上書き / DCH / ECH → 相方掃除プリミティブ → 対象セルが " " / width 1
   → overflow・overflow_ridx から該当エントリ除去
```

### Dependencies

**Internal Dependencies:**
- `crates/term_core` — テスト対象。grapheme merge / 幅計算 / overflow テーブル /
  相方掃除プリミティブ。
- feature wide-pair-overwrite-cleanup（PR #30） — 対象コード（`blank_wide_pair_partner`
  / `blank_wide_pair_split`）の提供元。マージ済み。

**External Dependencies:**
- 追加なし（新規テストフレームワーク・dev-dependency を導入しない: NFR1）。

### File Structure

```
crates/term_core/src/
├── print_handler.rs          # blank_wide_pair_partner:74 (変更しない)
├── print_handler/
│   └── tests.rs              # FR1 のテストを追加
├── csi_edit.rs               # handle_delete_characters / blank_wide_pair_split:161
│                             #   → mod tests に FR2(DCH) のテストを追加
└── csi_screen.rs             # handle_erase_characters:155
                              #   → mod tests に FR2(ECH) のテストを追加
```

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1, FR3): 家族絵文字を (0,row) に書く → overflow に (0,abs) が存在・
      base の `is_overflow()` が真であることを事前 assert → col0 に ASCII を print →
      col1 が `" "` / width 1、overflow・`overflow_ridx` から該当エントリが消えている。
- [ ] **TS2** (FR1, FR3): 家族絵文字ペアの spacer（col1）を ASCII で上書き →
      col0（overflow base）が `" "` / width 1 になり overflow・`overflow_ridx` 同期が
      保たれる。
- [ ] **TS3** (FR2, FR3): spacer 位置（col1）にカーソルを置き
      `handle_delete_characters(1)` → col0 の `get_cell_char == " "`
      （`unwrap_or_default` の空文字ではない）、overflow エントリ除去済み。
- [ ] **TS4** (FR2): spacer 位置にカーソルを置き `handle_erase_characters(1)` →
      start-1 = col0 が `" "` を返す（`csi_screen.rs:155` の `blank_wide_pair_split`
      呼び出しを通る）。

### Integration Tests

追加なし。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

### Regression

- [ ] **TS5** (FR4): 既存の `term_core --lib` スイート全件と workspace テストが通過。

実行コマンド:

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib
```

### Edge Cases

- [ ] 16 バイト超グラフェム（`char_len == 0xFF`、overflow テーブル行き）かつ幅 2 の
      base — 本 feature が検証する分岐そのもの。
- [ ] `term_core` の grapheme merge / 幅計算が ZWJ 家族絵文字を幅 2 の 1 クラスタと
      して扱わない場合 — 幅 2 かつ 16 バイト超の別グラフェムクラスタを選定する
      （要件の本質は「overflow 行きの幅 2 base」であり特定の絵文字ではない）。

### Performance Tests

該当なし。

## Security Considerations

該当なし（テスト追加のみ、プロダクションコード非変更）。

## Error Handling

該当なし（テスト追加のみ、プロダクションコード非変更）。

## Performance Optimization

該当なし。

## Success Criteria

- [ ] AC1: print 経路のテストが存在し、ZWJ 家族絵文字を col0 に書いて base が overflow
      行きになることを確認したうえで、col0 の ASCII 上書き後に col1 の spacer が
      `" "` / width 1 であることを assert している。
- [ ] AC2: `csi_edit` / `csi_screen` 経路のテストが存在し、同じ ZWJ ペアの spacer 位置で
      DCH / ECH を実行後、col-1 の `get_cell_char` が `" "`（空文字ではない）を返す
      ことを assert している。
- [ ] AC3: 追加テストが overflow 分岐（`char_len == 0xFF`）を実際に通ることが assert で
      確認されている（掃除前の overflow 状態の事前 assert を含む）。
- [ ] AC4: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
      および workspace のテストが全件通る。
- [ ] NFR1: テストが既存の inline `#[cfg(test)]` モジュールに置かれ、命名が
      `<subject>_<scenario>_<expected>` パターンに従い、新規 dev-dependency が無い。
- [ ] NFR2: `crates/term_core` の非テストコードに差分が無い。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（FR1–FR4、NFR1–NFR2 はすべて `status: resolved`）。

## Assumptions

- `term_core` の grapheme merge / 幅計算が ZWJ 家族絵文字を幅 2 の 1 クラスタとして
  扱う。万一扱わない場合は、幅 2 かつ 16 バイト超の別グラフェムクラスタを選定する
  （要件の本質は「overflow 行きの幅 2 base」であり特定の絵文字ではない）。
- 相方掃除プリミティブ共通化タスク（finding `b8a62feaf016ef08` / `931abe859e23fa5d`）は
  未着手であり、テスト対象は `blank_wide_pair_partner` と `blank_wide_pair_split` の 2 本。
- タスク記述の「csi_edit 経路」は DCH（`csi_edit.rs`）と ECH（`csi_screen.rs`）の
  両方を指す。
- PR #30 はマージ済みで、対象コードは integration worktree に存在する（実在確認済み）。

## References

- 要件定義書: `feature-docs/wide-pair-overflow-tests/REQUIREMENTS.md`
- `crates/term_core/src/print_handler.rs:74` — `blank_wide_pair_partner`
- `crates/term_core/src/csi_edit.rs:161` — `blank_wide_pair_split`
- `crates/term_core/src/csi_screen.rs:155` — ECH からの `blank_wide_pair_split` 呼び出し
- feature wide-pair-overwrite-cleanup（PR #30）レビュー round1 deferred finding
  `06ef78e20e9b9f0b`（medium / comprehensive）
