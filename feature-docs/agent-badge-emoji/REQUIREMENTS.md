---
title: "agent-badge-emoji"
created_date: 2026-08-06
status: draft
---

# agent-badge-emoji - 要件定義書

## 1. 概要

### 1.1 背景

エージェント状態バッジの表現が 4 状態で不統一になっている。working / idle は絵文字、blocked / done は円で描画されている。

### 1.2 目的

- エージェント状態バッジの表現を 4 状態すべて絵文字に統一する。
- blocked を疑問符絵文字にして「人間の入力待ち」という実態と表示を一致させる。
- done を seen 時に 💤 へ落とし、確認済みペインによるタブバーのノイズを減らす（内部状態は done のまま）。

### 1.3 スコープ

**対象**

- `badge_presentation()`（`src-tauri/src/ui/tab_bar.rs:273`）が blocked / done を解決する表示内容。
- 新規の絵文字クラスタ定数（`tab_bar.rs` 内）。
- 絵文字テクスチャ不在時のフォールバック円の形（`resolve_badge_render_mode()`）。

**対象外**

- 集約優先度および unseen フラグのセマンティクス（NFR1）。
- done の内部状態および `emterm mux wait --state done` の挙動（NFR2）。
- `agent_state_color()` そのもの（`ui::status_bar` と共有）。
- working / idle の表示（FR3）。

## 2. ビジネス要件

### 2.1 ビジネス目標

1. エージェント状態バッジの表現を 4 状態すべて絵文字に統一する（現状 working/idle=絵文字、blocked/done=円で不統一）。
2. blocked を疑問符絵文字にして「人間の入力待ち」という実態と表示を一致させる。
3. done を seen 時に 💤 へ落とし、確認済みペインによるタブバーのノイズを減らす（内部状態は done のまま）。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| — | requirements_analysis に対象ユーザーの定義なし |

### 2.3 期待される効果

- 4 状態のバッジ表現が絵文字に統一される。
- blocked の表示が「人間の入力待ち」という実態と一致する。
- 確認済み（seen）の done ペインによるタブバーのノイズが減る。

## 3. ユースケース

requirements_analysis にユースケースの定義はない。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | ステータス |
|----|--------|------|------------|
| FR1 | blocked の絵文字表示 | blocked を unseen=❓ / seen=❔ に解決する | resolved |
| FR2 | done の絵文字表示 | done を unseen=✅ / seen=💤 に解決する | resolved |
| FR3 | working / idle は不変 | working=⚡ / idle=💤 の表示を変更しない | resolved |
| FR4 | テクスチャ不在時の円フォールバック維持 | 絵文字テクスチャ不在時は既存の円フォールバックに解決する | resolved |
| FR5 | 判定関数の単一共有の維持 | 表示判定を `badge_presentation()` 1 箇所に保つ | resolved |
| FR6 | 絵文字クラスタ定数の形式 | 既存定数と同形式で新規クラスタを定義する | resolved |

### 4.2 表示クラスタ一覧

| 状態 | unseen | seen |
|------|--------|------|
| working | ⚡ (U+26A1) | ⚡ (U+26A1) |
| idle | 💤 (U+1F4A4) | 💤 (U+1F4A4) |
| blocked | ❓ (U+2753) | ❔ (U+2754) |
| done | ✅ (U+2705) | 💤 (U+1F4A4、`IDLE_BADGE_EMOJI` と同一クラスタ) |

### 4.3 機能詳細

#### FR1: blocked の絵文字表示

`badge_presentation()` は blocked を unseen で ❓ (U+2753)、seen で ❔ (U+2754) の Emoji presentation に解決する。

#### FR2: done の絵文字表示

`badge_presentation()` は done を unseen で ✅ (U+2705)、seen で 💤 (U+1F4A4、`IDLE_BADGE_EMOJI` と同一クラスタ) の Emoji presentation に解決する。

#### FR3: working / idle は不変

working (⚡ U+26A1) / idle (💤 U+1F4A4) の表示は変更しない（`WORKING_BADGE_EMOJI` / `IDLE_BADGE_EMOJI` と unseen/seen 非依存の挙動を維持）。

#### FR4: テクスチャ不在時の円フォールバック維持

絵文字テクスチャが取得できないとき `resolve_badge_render_mode()` は既存の円フォールバックに解決し、空白バッジ・toolkit 既定テキスト経路には決してならない（既存 FR3 の維持）。blocked / done のフォールバック円は既存の unseen=塗り / seen=リング（`agent_badge_filled` 相当）の形を保つ。

#### FR5: 判定関数の単一共有の維持

表示判定は `badge_presentation()`（`src-tauri/src/ui/tab_bar.rs:273`）1 箇所に保ち、タブバーと mux サイドバー（`mux_sidebar.rs:571` の `paint_agent_badge` 経由）が同じ判定を共有する状態を維持する。引数は既に Aggregated（unseen 込み）なのでシグネチャ変更は不要。

#### FR6: 絵文字クラスタ定数の形式

新規絵文字クラスタは `WORKING_BADGE_EMOJI` / `IDLE_BADGE_EMOJI` と同形式（`tab_bar.rs` 内の `pub const &'static str`、単一コードポイント・VS-16 なし）で定義する。done+seen は `IDLE_BADGE_EMOJI` と同一クラスタとする。

## 5. 非機能要件

パフォーマンス・セキュリティ・可用性の各カテゴリについて requirements_analysis に記載はない。確定している非機能要件は次の 3 件。

### 5.1 NFR1: 集約・unseen セマンティクス不変

集約優先度 blocked(4) > done+unseen(3) > working(2) > done+seen(1) > idle(0)（`agent_status_model.rs:346`）、unseen フラグの set/clear 挙動（`agent_status_model.rs:220` / `app.rs:3956`）は変更しない。

### 5.2 NFR2: 内部状態の互換性

done の内部状態は維持され、`emterm mux wait --state done` の挙動は変わらない。

### 5.3 NFR3: テスト規約準拠

ユニットテストは既存規約どおり対象モジュール内の `#[cfg(test)] mod tests` にインラインで置き、`--lib` で走る形にする（`test/README.md`）。

## 6. UI/UX要件

### 6.1 画面表示要件

- 表示クラスタは 4.2 の表のとおり。
- レイアウトは既存の統一バッジスロット（`AGENT_BADGE_SLOT_WIDTH=12px`）をそのまま使う。新規のレイアウト・トークン・コンポーネント設計判断は存在しないため design ステップは skip とする。
- 表示箇所はタブバーと mux サイドバーで、両者は同じ判定関数を共有する（FR5）。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 新規絵文字クラスタ定数は既存定数と同形式（単一コードポイント・VS-16 なし）とする（FR6）。
- 表示判定は `badge_presentation()` の 1 箇所に保つ（FR5）。
- 集約優先度・unseen セマンティクスは変更しない（NFR1）。
- done の内部状態と `emterm mux wait --state done` の挙動は変えない（NFR2）。
- ユニットテストは対象モジュール内インライン `#[cfg(test)] mod tests`（NFR3）。

### 9.2 ビジネス上の制約

記載なし。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 出典 | 対応 |
|------|------|------|
| 現行の `resolve_badge_render_mode` は Emoji フォールバックを常に `filled:true` とする実装であり、blocked/done の Emoji 化後に seen=リングを保つにはフォールバック形の伝搬が必要 | A1 | FR4 で unseen=塗り / seen=リングの維持を要件化 |
| `agent_state_color()` の MD3 色役割は絵文字表示では効かなくなる（絵文字テクスチャは untinted blit）。色役割はフォールバック円の描画にのみ残る | A2 | `agent_state_color` 自体は変更しない |
| done+seen が idle と同一クラスタになるため両者は視覚的に区別不能になる | A4 | タスクの意図どおり（確認済み done ペインは実質待機中） |

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] blocked が unseen で ❓ (U+2753)、seen で ❔ (U+2754) を表示する
- [ ] done が unseen で ✅ (U+2705)、seen で 💤 (U+1F4A4、idle と同一クラスタ) を表示する
- [ ] working / idle の表示は変更されていない
- [ ] 絵文字テクスチャが取得できないときは既存の円フォールバックが働き、空白バッジにならない（`resolve_badge_render_mode` の FR3 を維持）
- [ ] タブバーと mux サイドバーで同じ表示になる（両者が同じ判定関数を共有している状態を維持）
- [ ] 4 状態 × unseen/seen の全組み合わせを網羅するユニットテストがある

## 12. テストシナリオ

- [ ] TS1: `badge_presentation()` のテーブルテスト — 4 状態 × unseen/seen の 8 組み合わせを網羅し、期待クラスタ（working=⚡/⚡、idle=💤/💤、blocked=❓/❔、done=✅/💤）を検証する
- [ ] TS2: `resolve_badge_render_mode()` — blocked/done の Emoji presentation で `texture_available=false` のとき円フォールバックになり空白にならないこと、フォールバック円が unseen=塗り / seen=リングを保つことを検証する
- [ ] TS3: done+seen の Emoji クラスタが `IDLE_BADGE_EMOJI` と同一参照/同一文字列であることを検証する
- [ ] TS4: 既存の tab_bar / mux_sidebar テストを含む `--lib` スイート全体がグリーン（`CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`）

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| unseen / seen | 集約状態に含まれる確認状態フラグ。set/clear は `agent_status_model.rs:220` / `app.rs:3956` |
| 集約優先度 | blocked(4) > done+unseen(3) > working(2) > done+seen(1) > idle(0)（`agent_status_model.rs:346`） |
| Emoji presentation | `badge_presentation()` が返す、絵文字クラスタで描画する表示解決 |
| 円フォールバック | 絵文字テクスチャ不在時に `resolve_badge_render_mode()` が解決する描画形（unseen=塗り / seen=リング） |

## 14. 確認事項

### 14.1 前提事項

- [x] A1: 受け入れ条件の「既存の円フォールバック」は blocked/done について既存の unseen=塗り / seen=リング形（`agent_badge_filled` のセマンティクス）を指す。現行の `resolve_badge_render_mode` は Emoji フォールバックを常に `filled:true` とするが、これは working/idle が常に塗りであることを前提とした実装であり、blocked/done が Emoji 化された後は seen=リングを保てるようフォールバック形の伝搬が必要になる
- [x] A2: `agent_state_color()` の MD3 色役割は絵文字表示では効かなくなる（絵文字テクスチャは untinted blit）。色役割はフォールバック円の描画にのみ残る。`ui::status_bar` が共有する `agent_state_color` 自体は変更しない
- [x] A3: 新規 3 コードポイント（U+2753 / U+2754 / U+2705）はいずれも単一コードポイント・default emoji presentation で、既存定数（U+26A1 / U+1F4A4）と同じくバンドルの Noto Color Emoji で描画可能
- [x] A4: done+seen が idle と同一クラスタになるため両者は視覚的に区別不能になるが、これはタスクの意図どおり（確認済み done ペインは実質待機中）

### 14.2 未確認・保留事項

なし（FR1–FR6 / NFR1–NFR3 はすべて resolved）。

## 15. 参考資料

- `src-tauri/src/ui/tab_bar.rs:273`: `badge_presentation()`
- `src-tauri/src/ui/mux_sidebar.rs:571`: `paint_agent_badge`
- `src-tauri/src/agent_status_model.rs:346`: 集約優先度
- `src-tauri/src/agent_status_model.rs:220` / `src-tauri/src/app.rs:3956`: unseen フラグの set/clear
- `test/README.md`: テスト配置規約
