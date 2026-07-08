---
title: "block-cursor-glyph-font"
created_date: 2026-07-08
status: draft
---

# block cursor 上のグリフフォント不一致修正 - 要件定義書

## 1. 概要

### 1.1 背景

focused block cursor が既に文字が入力されているセルに重なったとき、
そのセル内の文字のフォントが grid 本体の描画と一致していない。
実測では grid 本体は Inconsolata で描画されているが、
block cursor 下の同じ「0」文字は egui 内蔵の monospace (Ubuntu Mono 系)
で描画されており、字形が明確に異なる（Inconsolata の 0 はスラッシュ入り、
egui monospace の 0 は縦棒入り）。

原因は `src-tauri/src/render/cursor.rs` の `draw_block_cursor` が
`FontId::monospace(font_px)` で glyph を再描画していること。
grid 本体の swash 描画パイプラインを経由していない。

### 1.2 目的

block cursor が既存文字の上に重なったときも、そのセルの文字が
grid 本体と同一のフォント・字形で描画されるようにする。

### 1.3 スコープ

- 対象: focused block cursor が既存文字（ASCII / CJK / その他）の上に来たときの glyph 再描画
- 対象外: IME preedit 表示中のカーソル字形（別 issue として分離）
- 対象外: underline / bar / unfocused hollow-block などのブロック以外のカーソル形状（そもそも glyph 再描画をしていない）
- 対象外: 絵文字などの色付きグリフの扱い変更（既存の cursor_glyph_paintable ルールは維持）

## 2. ビジネス要件

### 2.1 ビジネス目標

「AI 時代に使えるターミナル」として、視覚品質・タイポグラフィの一貫性を保つ。
ユーザーが設定したフォントは、カーソルの下に来ても常に同じ字形で見える必要がある。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| eMterm のエンドユーザー | ターミナルフォントを Inconsolata などカスタム字形フォントに設定しているユーザー |

### 2.3 期待される効果

- ブロックカーソル移動時にフォントが切り替わって見える現象の解消
- タイポグラフィの一貫性向上（0 / O / 1 / l / I など字形識別に効く文字での視認性維持）

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | ブロックカーソルを既存文字上に移動する | ユーザー | 高 |

### 3.2 ユースケース詳細

#### UC01: ブロックカーソルを既存文字上に移動する

**アクター**: eMterm ユーザー

**事前条件**:
- カーソル形状が block（DECSCUSR 1/2 相当、または設定で block 指定）
- ターミナルウィンドウが focused
- カーソルがまばたきの ON フェーズ
- カーソル位置のセルに文字が既に描画されている

**基本フロー**:
1. ユーザーがカーソルを既存文字を含むセルに移動させる
2. eMterm は block cursor の rect を cursor color で塗る
3. eMterm は同じセルにあった文字を再描画する
4. 再描画された文字は grid 本体の描画と同一フォント・字形になる

**事後条件**:
- カーソル下の文字が周囲の grid と同じ字形で表示される

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| F01 | block cursor 下グリフの swash 描画 | block cursor が既存文字に重なったとき、grid 本体と同じ swash 経由のフォント描画を通す | 高 |

### 4.2 機能詳細

#### F01: block cursor 下グリフの swash 描画

**説明**: block cursor overlay で文字を再描画するとき、
egui の built-in monospace ではなく、grid 本体と同じ font resolver /
swash rasterizer チェーンを経由させる。字形（Inconsolata の 0 の
スラッシュ等）が grid 本体と完全一致する必要がある。

**ビジネスルール**:
- カーソル下グリフの色は「そのセルの完全解決済み BACKGROUND 色」（既存の
  `resolve_cell_style_from_packed` 経路）を維持する
- wide (2-cell) グリフの扱いは既存のまま（rect が 2 セル分になる、
  leading column に glyph が描画される）
- 絵文字など `cursor_glyph_paintable` が false を返す glyph は今まで通り
  再描画しない

**エラーケース**:
- 対応フォントが解決できない場合: 既存のフォールバックチェーン
  （swash → ab_glyph → 最終的な .notdef）に従う。cursor overlay のためだけの
  例外パスは作らない

## 5. 非機能要件

### 5.1 パフォーマンス要件

- カーソル位置に来るのは1フレームあたり最大1セル（wide なら2セル）分のみ
  なので、既存 grid 描画に対する追加コストは無視できる範囲であること
- render-cpu-optimization feature の CPU 削減効果を後退させないこと
  （idle 時にカーソル移動だけで再描画コストが増えないこと）

### 5.2 セキュリティ要件

該当なし（レンダリング表層のみの修正）

### 5.3 保守性要件

- egui immediate-mode overlay と wgpu terminal_grid_pass の
  2 経路が glyph 描画に対して同じ font resolution を使うことを、
  コードコメントで明示する

### 5.5 互換性要件

- Linux / Windows 両プラットフォームで同じ挙動になること
- 既存のカーソル形状（underline / bar / unfocused hollow-block）の挙動は
  変わらないこと

## 6. UI/UX要件

### 6.1 画面設計要件

- block cursor が既存文字上に重なったとき、カーソルの矩形は cursor color、
  文字は grid 本体と同じフォントで背景色（= セルの本来の bg）で描画される
- 文字の位置・サイズ（ベースライン等）は grid 本体と一致する
  （ユーザーが「同じ 0 に見える」条件を満たす）

## 9. 制約条件

### 9.1 技術的制約

- block cursor overlay は現状 egui の `Painter` で描画されており、
  wgpu terminal_grid_pass のインスタンス列には介入していない
  （render-cpu-optimization task0001 の設計原則: grid instance は cursor
  state から独立させる）。この設計原則は維持する
- 「grid 側で該当セルの色を反転させて上塗り不要にする」案は
  この設計原則に反するので採用しない（block cursor rect を wgpu
  グリッドに戻すことになる）
- 代わりに egui overlay 側から swash 経路のグリフラスタライズを呼び、
  ラスタライズ結果を egui texture として持ち込むか、
  terminal_grid_pass に「cursor 上塗り glyph」用の描画パスを追加する
  形になる（具体案は create-plan で決定）

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] Inconsolata フォント設定下で、block cursor が既存の `0` 上に来ても
      スラッシュ入りの 0 として描画される（グリフが grid 本体と同一）
- [ ] ASCII / CJK / 記号のいずれの文字上でも grid 本体と字形が一致する
- [ ] wide (2-cell) glyph 上の block cursor で glyph が正しい 1 セルに
      描画され、はみ出さない
- [ ] IME preedit 描画・他のカーソル形状に regression がない
- [ ] `cargo test` / `bun test` / typecheck が通る

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: block cursor + 既存 ASCII 文字上（0, O, 1, l, I 等の字形差が
      顕著な文字を含む）でグリフが grid と一致する
- [ ] 正常系: block cursor + 既存 CJK 文字上でグリフが grid と一致する
- [ ] 境界値: wide グリフ trailing half に cursor が来た場合、leading
      column の glyph が正しく再描画される（既存の
      `resolve_cursor_glyph_col` 経路の regression が無いこと）
- [ ] 境界値: 空セル上ではフォント関連の挙動変化が無い（rect のみ描画）
- [ ] 正常系: unfocused hollow-block / underline / bar のカーソル形状に
      影響が無い（従来通り glyph 再描画をしない）

## 14. 確認事項

### 14.1 確認済み事項

- [x] スコープ: block cursor が既存文字上に来たときにグリフをターミナル
      フォント（swash 描画）と同一にする、で合意
- [x] IME preedit は対象外、別 issue として分離することで合意

## 15. 参考資料

- `src-tauri/src/render/cursor.rs:300-360` — `draw_block_cursor` 現行実装
- `src-tauri/src/render/terminal_grid_pass.rs` — grid 本体の swash 描画パス
- `src-tauri/src/render/font/` — font resolver / swash adapter / fallback チェーン
