# Implementation Plan: SGR Reverse Default-Color Swap

## Overview

`resolve_cell_style_from_packed` における reverse 処理のうち、両 `PackedColor::DEFAULT` セルでスワップが NOP になるバグを修正する。bold-brighten が perceived foreground を見るための packed-level スワップは現状維持し、`packed_to_egui` の **フォールバック引数（および `unwrap_or_else` の RGB 値）を reverse 時に swap** することで、`None` 返却ケースでも `theme.fg` / `theme.bg` が正しく入れ替わるようにする。indexed / truecolor セルは `packed_to_egui` が `Some(...)` を返してフォールバックを使わないので、追加スワップは不要で挙動は変わらない。

## Objectives

- `\e[7m` 単独適用時（fg/bg ともに DEFAULT）に、cell 描画が `theme.fg` と `theme.bg` を入れ替えた表示になる。
- indexed / truecolor 指定セルの reverse 表示挙動は変更しない。
- selection × reverse の XOR 合成挙動を維持する。
- bold-brighten の作用対象（reverse 後の perceived foreground）と適用順序を維持する。
- dim / hidden の適用順序を維持する。

## Prerequisites

### Development Environment

- Rust toolchain（プロジェクト固定。`rust-toolchain.toml` 準拠）
- `bun`（GUI ビルドの prerequisite だが、本タスクのテストには不要）

### Dependencies

- 内部:
  - `crate::render::theme::Theme`（read-only 参照）
  - `crate::render::CellStyle`（戻り値型、変更なし）
  - `crate::render::bold_brighten_packed`（変更なし、既存利用）
  - `crate::render::packed_to_egui`（変更なし、既存利用）
  - `crates/term_core` の `STYLE_REVERSE` / `STYLE_BOLD` などフラグ定数（変更なし）
- 外部: なし

## Architecture Overview

### Technology Stack

- **Language**: Rust
- **Framework**: なし（render パイプライン内のピュア関数修正）
- **Key Libraries**:
  - `egui::Color32` — 解決後 RGBA 表現
  - `std::mem::swap` — 既存 selection スワップで使用しているものを再利用

### Design Approach

- スコープを `src-tauri/src/render/mod.rs` の `resolve_cell_style_from_packed` 関数（および同モジュール内 `#[cfg(test)] mod tests` のユニットテスト）に閉じる。
- 既存の "packed-level reverse スワップ → bold-brighten → `packed_to_egui` 解決" の流れを保持し、`packed_to_egui` の **フォールバック引数と `unwrap_or_else` の RGB 値を reverse に応じて選ぶ** ように変更する。これにより `packed_to_egui` が `None` を返す両 DEFAULT セルで `theme.fg` / `theme.bg` のスワップが成立する。
- indexed / truecolor セルでは `packed_to_egui` が `Some(...)` を返しフォールバックは消費されないため、packed-level スワップのみで反転が完結する。挙動は現状維持。
- bold-brighten は indexed `0..8` のみを対象とする既存挙動を保ったまま、reverse 後の perceived foreground に対して作用する設計をそのまま使う。

### Component Interaction

```
collect_cell_inputs / draw cell pipeline
        │
        ▼
resolve_cell_style_from_packed(theme, packed_fg, packed_bg, flags, selected)
        │
        ├─ packed-level reverse swap (unchanged)
        ├─ bold-brighten on effective_fg_packed (unchanged)
        ├─ select (fg_fallback, bg_fallback) by reverse (NEW)
        ├─ packed_to_egui resolution with swapped fallbacks (UPDATED)
        ├─ selection swap (unchanged)
        ├─ dim blend (unchanged)
        └─ hidden clamp (unchanged)
        ▼
CellStyle { fg, bg, bold, italic, underline, strikethrough }
```

## Implementation Phases

### Phase 1: packed_to_egui の fallback を reverse 時に swap + 単体テスト追加

**Goal**: `resolve_cell_style_from_packed` が両 DEFAULT の reverse セルに対して `theme.fg` / `theme.bg` のスワップを行い、かつ indexed / truecolor の反転挙動は現状維持となるようにし、変更前後の不変条件を単体テストで固定する。

**Files to Create**: なし

**Files to Modify**:

- `src-tauri/src/render/mod.rs`
  - 対象関数 `resolve_cell_style_from_packed`（約 L1185-1258）の `packed_to_egui` 呼び出し直前に `(fg_fallback, bg_fallback)` を reverse に応じて選ぶブロックを追加し、`packed_to_egui` の第2引数と `unwrap_or_else` の RGB 値を両方ともそのフォールバックに置き換える。
  - 既存の "packed swap before bold-brighten" コメントを、役割分担（packed swap = bold-brighten 可視化のため / フォールバック swap = `packed_to_egui` が `None` を返す両 DEFAULT セルの救済のため）を説明する内容に更新。
  - 同ファイル内 `#[cfg(test)] mod tests`（既存の `packed_to_egui_*` テスト群と同居する場所）に新規ユニットテストを追加。

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `resolve_cell_style_from_packed` | cell の packed colors と flags / selection 状態を最終 `CellStyle` に解決する | `theme` 参照可。`packed_fg` / `packed_bg` は `term_core` の packed 表現。`flags` は `STYLE_*` のビット OR | reverse 設定時は (a) packed swap → bold-brighten → reverse 用フォールバック (theme.bg, theme.fg) で `packed_to_egui` 解決、で `(fg, bg)` が決まる。`packed_to_egui` が `Some(...)` を返す indexed/truecolor 側ではフォールバックは消費されず packed swap だけで反転する。selection 設定時は解決済み RGBA をもう一度スワップする |
| `mod tests`（新規ケース） | 上記関数の reverse 動作を bias なく検証する | `Theme::default()` が `theme.fg`、`theme.bg`、palette16 を提供する | 全テストが `cargo test --lib` で成功する |

**Processing Flow** (diagram-convertible, after the fix):

1. `flags` から各属性ブール（bold / dim / italic / underline / reverse / hidden / strikethrough）を抽出
2. packed-level reverse スワップ（既存挙動を維持）
   - reverse == true -> `(effective_fg_packed, effective_bg_packed) = (packed_bg, packed_fg)`
   - reverse == false -> 元の packed 値をそのまま採用
3. bold-brighten（既存挙動を維持）
   - bold && `theme.bold_brightens_ansi_colors` -> `effective_fg_packed = bold_brighten_packed(effective_fg_packed)`
   - それ以外 -> 変更なし
4. **フォールバック RGB の選択（NEW）**
   - reverse == true -> `(fg_fallback, bg_fallback) = (theme.bg, theme.fg)`
   - reverse == false -> `(fg_fallback, bg_fallback) = (theme.fg, theme.bg)`
5. palette 解決（UPDATED: フォールバック値が変数経由）
   - `fg = packed_to_egui(effective_fg_packed, fg_fallback, theme).unwrap_or_else(|| rgb_to_egui(fg_fallback))`
   - `bg = packed_to_egui(effective_bg_packed, bg_fallback, theme).unwrap_or_else(|| rgb_to_egui(bg_fallback))`
   - indexed / truecolor 入力は `Some(...)` を返してフォールバックを消費しないため、packed-level swap がそのまま反転として働く
   - DEFAULT 入力は `None` を返し、reverse 時に swap された theme 色が `unwrap_or_else` から採用される
6. selection スワップ（既存挙動を維持）
   - selected == true -> `(fg, bg)` を入れ替える
7. dim 処理（既存挙動を維持） -> dim == true なら `fg` を `bg` 方向に 50% ブレンド
8. hidden 処理（既存挙動を維持） -> hidden == true なら `fg = bg`
9. `CellStyle { fg, bg, bold, italic, underline, strikethrough }` を返す

**Implementation Steps** (TDD bias):

1. **失敗テストの追加** — `mod tests` に新規ユニットテスト 6 件を先に追加し、未修正コードで `reverse_with_both_default_*` / `reverse_with_indexed_fg_default_bg_*` 系が失敗することをローカルで確認する。
2. **フォールバックスワップの差し込み** — `packed_to_egui` 呼び出し直前に `(fg_fallback, bg_fallback) = if reverse { (theme.bg, theme.fg) } else { (theme.fg, theme.bg) };` を追加し、`packed_to_egui` の第2引数および `unwrap_or_else` の RGB 引数を `fg_fallback` / `bg_fallback` に差し替える。
3. **コメント更新** — 既存の packed-swap 直上コメントと bold-brighten 直上コメントを、役割分担（packed 層は bold-brighten 可視化用、フォールバック swap は `packed_to_egui` が `None` を返す両 DEFAULT セルの救済用、indexed/truecolor 入力では `Some(...)` が返るためフォールバックは消費されない）に書き換える。
4. **テスト緑化確認** — 新規テストおよび既存 `bold_brighten_packed_*` / `packed_to_egui_*` テストが全て成功することを `cargo test --lib` で確認する。
5. **CLI-only ビルド確認** — `cargo check --no-default-features` で feature gate 越しのビルドが破綻していないことを確認する。

**Dependencies**: Requires 既存 `bold_brighten_packed` / `packed_to_egui` / `Theme`. Blocks なし（修正は局所完結）。

**Testing Approach**:

- Unit (`src-tauri/src/render/mod.rs` 内 `mod tests`):
  - **TS-1** reverse + 両 DEFAULT で `fg == rgb_to_egui(theme.bg)`、`bg == rgb_to_egui(theme.fg)`
  - **TS-2** reverse + indexed(1) fg + DEFAULT bg で `fg == rgb_to_egui(theme.bg)`、`bg == indexed(1) 解決色`
  - **TS-3** reverse + truecolor 両指定で `fg`／`bg` の RGB が完全に入れ替わる
  - **TS-4** reverse + selected + 両 DEFAULT で結果が non-reverse / non-selected と一致（XOR 成立）
  - **TS-5** reverse なし / selection なし / 両 DEFAULT で `fg == rgb_to_egui(theme.fg)`、`bg == rgb_to_egui(theme.bg)`（コントロール）
  - **TS-6** reverse + bold + `bold_brightens_ansi_colors=true`、packed_fg=DEFAULT、packed_bg=indexed(1) で、最終 `fg`（= 描画される文字色 = perceived foreground）が bright 化された indexed(9) の解決色、最終 `bg` が `rgb_to_egui(theme.fg)` になる
- Integration: なし（関数はピュア・ユニットで網羅可能）
- E2E: なし（既存 E2E ハーネスに本パスの covering case 無し。新規追加もしない）
- Manual: SPEC.md の Manual Tests を踏襲（後述）

**Acceptance Criteria**:

- [ ] `resolve_cell_style_from_packed` が「packed swap → bold-brighten → reverse 応じたフォールバックで解決」の構成になっている（RGBA swap 自体は追加していない）
- [ ] 新規ユニットテスト TS-1〜TS-6 がすべて成功する
- [ ] 既存 `bold_brighten_packed_*` / `packed_to_egui_*` テストが成功し続ける
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` が exit 0
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が exit 0

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/render/
└── mod.rs                  # resolve_cell_style_from_packed を編集
                            # #[cfg(test)] mod tests に TS-1〜TS-6 を追加
```

その他の変更ファイルはなし。

## Testing Strategy

- Unit: 本タスクの中核。新規 6 ケース + 既存 `bold_brighten_packed_*` / `packed_to_egui_*` のリグレッション確認。
- Integration: 不要（関数はピュア・依存は read-only な `Theme` のみ）。
- E2E: 既存ハーネスに該当 covering case 無し。本タスクでは新規追加しない（SPEC §"E2E Tests" の方針に準拠）。
- Manual: SGR 7 の表示確認は人間判断（テーマ依存の色味）。`printf` で 2 ケースを目視確認する。

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (なし) | — | 外部依存・新規追加なし |

`crates/term_core` および `egui` 依存はバージョン据え置き。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| reverse × selection の XOR 合成を崩す | Low | Medium | TS-4 で `reverse + selected` が `non-reverse / non-selected` と一致することを固定 |
| bold-brighten の作用対象が変わる | Low | Medium | packed-level swap を残し、TS-6 で perceived fg に bright 化が効くことを固定 |
| dim / hidden の適用順序が壊れる | Low | Low | 既存コードの順序（selection → dim → hidden）を保ち、既存テストが covering している分はリグレッション扱いで担保 |
| `_fallback` 引数の意味変容と混同される | Low | Low | 本タスクでは `_fallback` の整理を行わない。caller 側の `unwrap_or_else` で吸収するので `packed_to_egui` の signature は据え置き（SPEC §"Affected Code" 明記） |

## Open Questions

- [ ] dim / hidden の自動テストはスコープ外。`run-regression-suite` で既存挙動を担保する方針で良いかは sdd.3 / sdd.6 で確認する。
- [ ] NFR1 の "no performance regression" は実測ではなく "constant time edit" の合議で確定（SPEC NFR1 と一致）。

## Success Metrics

- [ ] Functional: FR1〜FR4 を満たす実装になっている
- [ ] Quality: 新規ユニットテスト 6 件追加、既存テストの失敗 0
- [ ] Performance: 関数本体に O(1) の swap が 1 つ増えるのみ。ベンチ測定は不要（NFR1 合議）

## References

- 仕様書: `doc/tasks/sgr-reverse-default-color-swap/SPEC.md`
- 要件定義書: `doc/tasks/sgr-reverse-default-color-swap/要件定義書.md`
- 既存コード: `src-tauri/src/render/mod.rs` `resolve_cell_style_from_packed` (~L1185-1258)
- 既存コード: `src-tauri/src/render/mod.rs` `packed_to_egui` (~L1329) — `_fallback` は据え置き
- 既存テスト: `src-tauri/src/render/mod.rs` `mod tests` の `packed_to_egui_default_returns_none` / `_indexed_uses_theme_palette` / `_truecolor_returns_exact_rgb` (~L1532-1554)
- SGR パース: `crates/term_core/src/sgr.rs:31` (SGR 7 set), `:38` (SGR 27 clear)
