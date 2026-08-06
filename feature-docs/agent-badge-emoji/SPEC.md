# Feature: agent-badge-emoji

## Overview

エージェント状態バッジの表現を 4 状態すべて絵文字に統一する。現状は working / idle が絵文字、blocked / done が円で不統一になっている。blocked を疑問符絵文字にし、done を seen 時に 💤 へ落とす（内部状態は done のまま）。

要件の一次情報は `feature-docs/agent-badge-emoji/REQUIREMENTS.md`。

## Objectives

- エージェント状態バッジの表現を 4 状態すべて絵文字に統一する（現状 working/idle=絵文字、blocked/done=円で不統一）。
- blocked を疑問符絵文字にして「人間の入力待ち」という実態と表示を一致させる。
- done を seen 時に 💤 へ落とし、確認済みペインによるタブバーのノイズを減らす（内部状態は done のまま）。

## User Stories

requirements_analysis にユーザーストーリーの定義はない。受け入れ条件は Success Criteria に記載する。

## Technical Requirements

### Badge presentation table

| State | unseen | seen |
|-------|--------|------|
| working | ⚡ (U+26A1) | ⚡ (U+26A1) |
| idle | 💤 (U+1F4A4) | 💤 (U+1F4A4) |
| blocked | ❓ (U+2753) | ❔ (U+2754) |
| done | ✅ (U+2705) | 💤 (U+1F4A4、`IDLE_BADGE_EMOJI` と同一クラスタ) |

### Functional Requirements

- **FR1 - blocked の絵文字表示:** `badge_presentation()` は blocked を unseen で ❓ (U+2753)、seen で ❔ (U+2754) の Emoji presentation に解決する。
- **FR2 - done の絵文字表示:** `badge_presentation()` は done を unseen で ✅ (U+2705)、seen で 💤 (U+1F4A4、`IDLE_BADGE_EMOJI` と同一クラスタ) の Emoji presentation に解決する。
- **FR3 - working / idle は不変:** working (⚡ U+26A1) / idle (💤 U+1F4A4) の表示は変更しない（`WORKING_BADGE_EMOJI` / `IDLE_BADGE_EMOJI` と unseen/seen 非依存の挙動を維持）。
- **FR4 - テクスチャ不在時の円フォールバック維持:** 絵文字テクスチャが取得できないとき `resolve_badge_render_mode()` は既存の円フォールバックに解決し、空白バッジ・toolkit 既定テキスト経路には決してならない（既存 FR3 の維持）。blocked/done のフォールバック円は既存の unseen=塗り / seen=リング（`agent_badge_filled` 相当）の形を保つ。
- **FR5 - 判定関数の単一共有の維持:** 表示判定は `badge_presentation()`（`src-tauri/src/ui/tab_bar.rs:273`）1 箇所に保ち、タブバーと mux サイドバー（`mux_sidebar.rs:571` の `paint_agent_badge` 経由）が同じ判定を共有する状態を維持する。引数は既に Aggregated（unseen 込み）なのでシグネチャ変更は不要。
- **FR6 - 絵文字クラスタ定数の形式:** 新規絵文字クラスタは `WORKING_BADGE_EMOJI` / `IDLE_BADGE_EMOJI` と同形式（`tab_bar.rs` 内の `pub const &'static str`、単一コードポイント・VS-16 なし）で定義する。done+seen は `IDLE_BADGE_EMOJI` と同一クラスタとする。

### Non-Functional Requirements

- **NFR1 - 集約・unseen セマンティクス不変:** 集約優先度 blocked(4) > done+unseen(3) > working(2) > done+seen(1) > idle(0)（`agent_status_model.rs:346`）、unseen フラグの set/clear 挙動（`agent_status_model.rs:220` / `app.rs:3956`）は変更しない。
- **NFR2 - 内部状態の互換性:** done の内部状態は維持され、`emterm mux wait --state done` の挙動は変わらない。
- **NFR3 - テスト規約準拠:** ユニットテストは既存規約どおり対象モジュール内の `#[cfg(test)] mod tests` にインラインで置き、`--lib` で走る形にする（`test/README.md`）。

## Implementation Approach

### Components

| Component | Path | Role |
|---|---|---|
| `badge_presentation()` | `src-tauri/src/ui/tab_bar.rs:273` | 状態 × unseen/seen から表示クラスタを解決する唯一の判定点（FR1 / FR2 / FR3 / FR5） |
| 絵文字クラスタ定数 | `src-tauri/src/ui/tab_bar.rs` | `WORKING_BADGE_EMOJI` / `IDLE_BADGE_EMOJI` と新規クラスタ（FR6） |
| `resolve_badge_render_mode()` | `src-tauri/src/ui/tab_bar.rs` | 絵文字テクスチャ不在時の円フォールバック解決（FR4） |
| `paint_agent_badge` | `src-tauri/src/ui/mux_sidebar.rs:571` | mux サイドバー側の描画。`badge_presentation()` を共有（FR5） |
| 集約モデル | `src-tauri/src/agent_status_model.rs:220`, `:346` | 集約優先度と unseen フラグ。変更しない（NFR1） |

### Data Flow

```
Aggregated (state + unseen)
  → badge_presentation()            # FR1 / FR2 / FR3 / FR5
    → Emoji presentation (cluster)  # FR6 の定数
      → resolve_badge_render_mode() # texture available?
        → emoji texture blit        # untinted
        → circle fallback           # FR4: unseen=塗り / seen=リング
```

タブバーと mux サイドバーは同一の `badge_presentation()` を通るため、両者の表示は一致する（FR5）。

### Layout

既存の統一バッジスロット（`AGENT_BADGE_SLOT_WIDTH=12px`）をそのまま使う。新規のレイアウト・トークン・コンポーネント設計判断はないため design ステップは skip。

### Out of scope

- 集約優先度および unseen フラグの set/clear 挙動（NFR1）
- done の内部状態と `emterm mux wait --state done`（NFR2）
- `agent_state_color()` 自体（`ui::status_bar` と共有）

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1, FR2, FR3): `badge_presentation()` のテーブルテスト — 4 状態 × unseen/seen の 8 組み合わせを網羅し、期待クラスタ（working=⚡/⚡、idle=💤/💤、blocked=❓/❔、done=✅/💤）を検証する。
- [ ] **TS2** (FR4): `resolve_badge_render_mode()` — blocked/done の Emoji presentation で `texture_available=false` のとき円フォールバックになり空白にならないこと、フォールバック円が unseen=塗り / seen=リングを保つことを検証する。
- [ ] **TS3** (FR2, FR6): done+seen の Emoji クラスタが `IDLE_BADGE_EMOJI` と同一参照/同一文字列であることを検証する。

### Regression

- [ ] **TS4** (FR5, NFR1, NFR2, NFR3): 既存の tab_bar / mux_sidebar テストを含む `--lib` スイート全体がグリーン。

```
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib
```

テストは NFR3 のとおり対象モジュール内の `#[cfg(test)] mod tests` にインラインで置く。

### Edge Cases

- [ ] 絵文字テクスチャが取得できない状態で blocked/done を描画しても、空白バッジや toolkit 既定テキスト経路にならない（FR4 / TS2）。
- [ ] done+seen と idle は同一クラスタになり視覚的に区別できない（A4: 意図どおり）。

## Assumptions

- **A1:** 受け入れ条件の「既存の円フォールバック」は blocked/done について既存の unseen=塗り / seen=リング形（`agent_badge_filled` のセマンティクス）を指す。現行の `resolve_badge_render_mode` は Emoji フォールバックを常に `filled:true` とするが、これは working/idle が常に塗りであることを前提とした実装であり、blocked/done が Emoji 化された後は seen=リングを保てるようフォールバック形の伝搬が必要になる。
- **A2:** `agent_state_color()` の MD3 色役割は絵文字表示では効かなくなる（絵文字テクスチャは untinted blit）。色役割はフォールバック円の描画にのみ残る。`ui::status_bar` が共有する `agent_state_color` 自体は変更しない。
- **A3:** 新規 3 コードポイント（U+2753 / U+2754 / U+2705）はいずれも単一コードポイント・default emoji presentation で、既存定数（U+26A1 / U+1F4A4）と同じくバンドルの Noto Color Emoji で描画可能。
- **A4:** done+seen が idle と同一クラスタになるため両者は視覚的に区別不能になるが、これはタスクの意図どおり（確認済み done ペインは実質待機中）。

## Success Criteria

- [ ] blocked が unseen で ❓ (U+2753)、seen で ❔ (U+2754) を表示する
- [ ] done が unseen で ✅ (U+2705)、seen で 💤 (U+1F4A4、idle と同一クラスタ) を表示する
- [ ] working / idle の表示は変更されていない
- [ ] 絵文字テクスチャが取得できないときは既存の円フォールバックが働き、空白バッジにならない（`resolve_badge_render_mode` の FR3 を維持）
- [ ] タブバーと mux サイドバーで同じ表示になる（両者が同じ判定関数を共有している状態を維持）
- [ ] 4 状態 × unseen/seen の全組み合わせを網羅するユニットテストがある

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし（FR1–FR6 / NFR1–NFR3 はすべて `resolved`）。

## References

- 要件定義書: `feature-docs/agent-badge-emoji/REQUIREMENTS.md`
- `src-tauri/src/ui/tab_bar.rs:273`: `badge_presentation()`
- `src-tauri/src/ui/mux_sidebar.rs:571`: `paint_agent_badge`
- `src-tauri/src/agent_status_model.rs:346`: 集約優先度
- `src-tauri/src/agent_status_model.rs:220` / `src-tauri/src/app.rs:3956`: unseen フラグの set/clear
- `test/README.md`: テスト配置規約
