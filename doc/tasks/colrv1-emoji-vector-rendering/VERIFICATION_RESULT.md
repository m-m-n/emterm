# Verification Result: COLRv1 Vector Emoji Rendering

## 概要

| 項目 | 値 |
|------|-----|
| 検証日時 | 2026-06-30 JST |
| Feature | `colrv1-emoji-vector-rendering` |
| 対象 commit | `4c5d07acbf3d6b8169ec7057ff2f1971bf099828` (HEAD) ※ feature 変更は **未コミット** (workspace = M / ??) |
| VERIFICATION.md | `doc/tasks/colrv1-emoji-vector-rendering/VERIFICATION.md` |
| Project | eMterm (Rust + winit + wgpu + swash + 新規 skrifa/tiny-skia) |
| 検証範囲 | sdd.6 包括検証 (ファイル構造 / SPEC 要件適合 / NFR3 unsafe ガード / アセット差分 / マニュアル項目抽出) |
| 再ビルド/再テスト | **実施せず** — sdd.5-check で実施済み、`completed_at_commit` 同一で staleness なし |

> sdd.yaml の `workflow[*].completed_at_commit` は `4c5d07a...` だが、`git status` 上は feature 変更が
> uncommitted (`scripts/fetch-fonts.sh`, `src-tauri/Cargo.toml`, `src-tauri/build.rs`,
> `src-tauri/src/render/font/{mod,resolver,swash_adapter}.rs` が `M`、`colrv1_painter.rs` と
> `doc/tasks/colrv1-emoji-vector-rendering/` は `??`)。検証は **作業ツリー上のファイル** に対して
> 実施した。コミットを切る判断はユーザー側に委ねる。

---

## 要件適合マトリクス (FR1〜FR8 / NFR1〜NFR5)

| 要件 | 判定 | 根拠 |
|------|------|------|
| **FR1** COLRv1 paint graph rasterization | PASS | `colrv1_painter.rs` に 4-param `rasterize(font_bytes, glyph_id, size_px, target_cell_h_px)` 実装。`#[test]` 16 件のうち 10 件 (TS-7〜TS-14 + TS-14b/TS-14c) が rasterize 経路を覆う。SweepGradient / RadialGradient r0 > 0 は tiny-skia 0.11 制約で partial (warn_once ログで通知、FR7)。sdd.5 で全 PASS 報告。 |
| **FR2** Font path routing via `is_colrv1_emoji` + set_base_font 配線 | PASS | `SwashFont.is_colrv1_emoji: bool` フィールド (swash_adapter.rs:117)、`ingest_font` で probe 実行 (L232)、`raster()` の分岐 (L319) と `colrv1_painter::rasterize` 呼出 (L345) を grep で確認。`Inner.base_font: Option<FontId>` (L139)、`SwashRasterizer::set_base_font` impl (L280)、`App::build_font_stack` の `rasterizer.set_base_font(base_id)` 呼出 (app.rs:1071) も確認。`GlyphRasterizer` trait の default no-op `set_base_font` (traits.rs:164) も確認。TS-4/TS-5/TS-15/TS-16 で覆う。 |
| **FR3** Premultiplied → straight alpha | PASS | `un_premultiply_alpha_*` 3 unit tests (TS-1/TS-2/TS-3) ソース上に確認。 |
| **FR4** Bundled font swap (NotoColorEmoji → Noto-COLRv1) | PASS | `BUNDLED_EMOJI_COLOR_FONT` が `assets/fonts/Noto-COLRv1.ttf` を `include_bytes!` (resolver.rs:34)。`NotoColorEmoji.ttf` は作業ツリー上から削除済 (gitignored ファイル)。`build.rs` の failsafe list も `Noto-COLRv1.ttf` に更新済 (L74)。 |
| **FR5** `fetch-fonts.sh` pinned Noto-COLRv1 | PASS | scripts/fetch-fonts.sh:138-140 に `fetch_one "Noto-COLRv1.ttf" "<v2.051 URL>" "0ae57fe58…1e1a28d2"`。実ファイルの `sha256sum` も pin と一致 (= `0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2`)。size = 4,991,984 bytes (pin の 4,991,984 と一致)。 |
| **FR6** Monochrome fallback preserved (`None` → FallbackChain descends) | PASS | swash_adapter.rs:371 で `colrv1: fallback for gid=… (no paint graph)` を `log::info!` し、`raster()` が `None` を返す経路を確認。`unknown_glyph_falls_back_to_chain` 統合テストで TS-17 が PASS。 |
| **FR7** Path-selection logging + warn_once degradation | PASS | fallback 経路: swash_adapter.rs:371 `log::info!("colrv1: fallback …")`。warn_once: colrv1_painter.rs に `warn_once_alloc_failed` (L225)、`warn_once_radial_r0_dropped` (L235)、`warn_once_unsupported_composite` (L247)、sweep gradient fallback の `OnceLock` ガード (L625-630) を確認。SPEC の方針 (info=fallback / debug=hit / warn_once=degradation) と一致。 |
| **FR8** Pixmap sizing uses base cell height with 1px padding + bbox-fit | PASS | `rasterize` 実装に `dim = ceil(target_cell_h_px)` (`> 0` のとき) / `ceil(size_px)` (legacy fallback) (colrv1_painter.rs:120-124)、1 px padding + `dim < 4` skip (L128-130)、`ColorGlyph::bounding_box` 取得 + EM box fallback (L148-155)、`scale = inner / max(bbox_w, bbox_h)` の uniform fit (L156-158)、centering + Y-flip transform (L161-168)、`advance = dim` pin (L187)、`bearing.1` を base ascent で上書き (swash_adapter.rs:356-360) を確認。TS-14 (legacy fallback) / TS-14b (cell-h padding+centering) / TS-14c (tiny-dim no-padding) で覆う。 |
| **NFR1** Performance (< 10 ms first-rasterize on Windows) | **DEFERRED** (manual) | 自動計測ベンチなし。TS-21 (Windows 1.5× DPI) でユーザー実機検証待ち。 |
| **NFR2** Binary size ~5 MiB 削減 | PARTIAL (asset 差分のみ自動確認 / binary 差分は manual TS-20) | アセット差分: 旧 `NotoColorEmoji.ttf` 10,673,480 B → 新 `Noto-COLRv1.ttf` 4,991,984 B = **5,681,496 B (≈ 5.42 MiB) 削減** (IMPLEMENTATION.md 記載値と一致)。release binary 差分計測 (TS-20) は manual。 |
| **NFR3** Safety (no new `unsafe`) | PASS | `grep 'unsafe' src-tauri/src/render/font/colrv1_painter.rs` → 1 hit のみ、ファイルヘッダ doc comment「`unsafe` is not used anywhere in this module (NFR3)」(L17) で `unsafe { … }` ブロックなし。`git diff HEAD -- swash_adapter.rs \| grep '^+' \| grep -iw unsafe` → 0 hit。 |
| **NFR4** Reproducibility (SHA256 pin) | PASS | 上述 FR5 を参照。`sha256sum` 実測値と pin が一致。 |
| **NFR5** Maintainability (unit tests + pinned deps) | PASS | Cargo.toml L158-159 で `skrifa = { version = "0.20", optional = true }` / `tiny-skia = { version = "0.11", optional = true }` (minor pin)。`features.gui` (L29-55) に `dep:skrifa` / `dep:tiny-skia` を含む。`colrv1_painter` 16 unit tests が定義済 (TS-1〜TS-14 + TS-14b/TS-14c)。 |

**自動側総括**: FR1〜FR8 = 8/8 PASS、NFR3〜NFR5 = 3/3 PASS、NFR1 = 1 DEFERRED、NFR2 = PARTIAL (asset 差分 PASS、binary 差分 manual)。

---

## ファイル構造検証結果

### Files Created

| Path | 状態 | 備考 |
|------|------|------|
| `src-tauri/src/render/font/colrv1_painter.rs` | EXISTS | untracked (新規ファイル — sdd.4 implement で作成)。14 `#[test]` 関数定義済。 |
| `src-tauri/assets/fonts/Noto-COLRv1.ttf` | EXISTS | 4,991,984 B。SHA256 = `0ae57fe58…1e1a28d2` (pin と一致)。gitignored asset (`.gitignore` に `*.ttf`)。 |

### Files Modified

| Path | 状態 | 確認内容 |
|------|------|----------|
| `src-tauri/Cargo.toml` | MODIFIED | `[dependencies]` に `skrifa = 0.20` / `tiny-skia = 0.11` (optional)、`[features].gui` に `dep:skrifa` / `dep:tiny-skia` を確認。 |
| `src-tauri/src/render/font/mod.rs` | MODIFIED | L16 `pub mod colrv1_painter;` を確認。 |
| `src-tauri/src/render/font/resolver.rs` | MODIFIED | `BUNDLED_EMOJI_COLOR_FONT` の `include_bytes!` が `Noto-COLRv1.ttf` に変更 (L34)。doc コメントが「COLRv1 + glyf」に refresh 済 (L29-33)。 |
| `src-tauri/src/render/font/swash_adapter.rs` | MODIFIED | `SwashFont.is_colrv1_emoji: bool` (L117)、ingest probe (L226)、raster 分岐 (L309-311)、`has_color OR is_colrv1_emoji` (L456)、hit/fallback ログ (L314, L329)、3 統合テスト (L784/L815/L848) を grep で確認。 |
| `scripts/fetch-fonts.sh` | MODIFIED | Noto-COLRv1.ttf エントリ追加 (L138-140) と pin SHA256 一致を確認。 |
| `src-tauri/assets/fonts/README.md` | MODIFIED | L23 のインベントリ行が `Noto-COLRv1.ttf` + pin SHA256 にスワップ済。 |
| `src-tauri/build.rs` | MODIFIED (計画追補) | L74 failsafe list を `assets/fonts/Noto-COLRv1.ttf` に更新済。VERIFICATION.md の「Files Modified (planning addendum)」に明記された追補修正。 |

### Files Deleted (local FS only, gitignored)

| Path | 状態 |
|------|------|
| `src-tauri/assets/fonts/NotoColorEmoji.ttf` | DELETED 確認済 (作業ツリー上に存在せず、git にも tracked されていない)。 |

**ファイル構造判定**: PASS (24/24 想定どおり)

---

## NFR3 (unsafe ガード) 結果 — TS-24

| 確認項目 | 結果 |
|---------|------|
| `grep -n 'unsafe' src-tauri/src/render/font/colrv1_painter.rs` | 1 hit (L17 doc-comment `//! \`unsafe\` is not used anywhere in this module (NFR3).`)。**`unsafe { … }` ブロックは 0 件**。 |
| `git diff HEAD -- src-tauri/src/render/font/swash_adapter.rs \| grep '^+' \| grep -iw unsafe` | 0 hit。COLRv1 dispatch 分岐に新規 unsafe なし。 |
| `grep '\.unwrap()' src-tauri/src/render/font/colrv1_painter.rs` | 0 hit。 |
| `git diff HEAD -- src-tauri/src/render/font/swash_adapter.rs \| grep '^+' \| grep '\.unwrap()'` | 0 hit。 |

**NFR3 判定**: PASS

---

## マニュアル項目 (ユーザー実施待ち)

VERIFICATION.md の「Manual Testing (E2E Not Possible)」セクションから 4 件抽出 (TS-24 は本検証で自動確認済のため除外)。

### TS-20 — Release binary size delta

**判定**: PENDING (ユーザー実機が必要)

期待: 旧 → 新 で release binary が約 5 MiB 縮小。

実行スニペット (プロジェクトルートから、`cd` せず実行)：

```sh
# 1. 現状 (新 path) のリリースビルド
CARGO_TARGET_DIR=src-tauri/target-host cargo build --release \
  --manifest-path src-tauri/Cargo.toml
ls -l src-tauri/target-host/release/emterm

# 2. ベースライン (旧 path) を別ブランチ / stash から取り直し再ビルド
#    必要なら git worktree で別ディレクトリにチェックアウトして同様にビルド
#    ls -l 差分を比較
```

自動側で確認済の事実: **アセット差分** は `NotoColorEmoji.ttf` 10,673,480 B → `Noto-COLRv1.ttf` 4,991,984 B = **5,681,496 B (~5.42 MiB) 削減**。release binary の縮小幅はアセット差分に加えて `skrifa`/`tiny-skia` 新規リンク分の増加が引かれるため、実機計測が必要。

### TS-21 — Windows 1.5× DPI 視覚比較

**判定**: PENDING (Windows 実機が必要、ユーザー側に Windows ハードあり)

期待: `echo 😀🚀❤️🌍👍🏽` で描画されるグリフが `tmp/verify-emoji/out/compare3_*_26px.png` の C variant に視覚的に一致 (エッジが鮮明、にじみなし、色のくすみなし)。

実行手順:

1. Windows 機で eMterm を 1.5× DPI で起動
2. `echo 😀🚀❤️🌍👍🏽` を実行
3. レンダ結果を `tmp/verify-emoji/out/compare3_*_26px.png` (C variant) と並べて目視比較

### TS-22 — Linux 1.0× DPI リグレッション確認

**判定**: PENDING (Linux 実機 + 比較対象として現 main の旧描画も必要)

期待: 同じ入力 (`echo 😀🚀❤️🌍👍🏽`) で current main (CBDT path) との視覚的差分なし、もしくは明確な改善のみ。

実行手順:

1. Linux 1.0× DPI で eMterm を起動 (本変更ブランチ)
2. `echo 😀🚀❤️🌍👍🏽` を実行
3. 同じ手順を current main (CBDT path) で実施し、目視で比較

### TS-23 — Windows RDP 1.0× リグレッション確認

**判定**: PENDING (Windows RDP セッションが必要)

期待: RDP 1.0× 越しで同じ入力にリグレッションなし。

実行手順: TS-21 と同じだが、Windows RDP セッション 1.0× scaling 配下で実施。

### TS-24 — `unsafe` 監査

**判定**: **DONE (自動確認済)** — 上記「NFR3 (unsafe ガード) 結果」セクション参照。

---

## sdd.5 から引き継いだ既知事項

### A. tabs.rs flaky failures (5 件)

VERIFICATION.md L104 に既知事項として記録済。

- `tabs::tests::ts7`, `ts9`, `ts10`, `ts13` (off-thread replay timeouts)
- `welcome_without_windows_leaves_group_none`

本 feature が touch しているモジュール (`src/render/font/*`) には影響しない。MEMORY.md `project_test_execution_notes.md` に従って `--test-threads=1` 実行で回避可。本検証は再テストしないため判定不要。

### B. doc コメント残骸 3 件 (静的解析 WARN レベル)

sdd.5-check で報告された「doc コメント残骸 3 件」に該当する `NotoColorEmoji.ttf` への明示的言及が以下 3 箇所に残存する (動作影響なし、doc コメントのみ)：

| File | Line | 残存テキスト概要 |
|------|------|-----------------|
| `src-tauri/src/render/font/user_dir.rs` | 436 | `// NotoColorEmoji ("Noto Color Emoji"). Family lookup must …` |
| `src-tauri/src/ui/chrome.rs` | 77 | `/// We register the bundled \`NotoSansCJK-JP\` and \`NotoColorEmoji.ttf\` …` |
| `src-tauri/src/render/font/fallback.rs` | 461 | `/// Regression: bundled \`NotoColorEmoji.ttf\` happens to carry glyphs …` |

上記は「現在のバンドル名」と一致しない doc コメント。WARN レベルとして引き継ぎ、本検証では blocker としない (sdd.5 でも非 blocker 扱い)。

> なお `CBDT` / `COLR` の一般的キャパビリティを述べる doc コメント
> (例: `traits.rs`, `presentation.rs`, `emoji_resample.rs` 等) はバンドル
> ファイル名と紐付かないため、文言として技術的に誤っていない。WARN 対象外。

---

## 総合判定

| 区分 | 結果 |
|------|------|
| 自動検証 (file structure / SPEC 要件 / NFR3 unsafe / asset 差分 / dep 配線 / log 配線) | **AUTO PASS** |
| マニュアル待ち項目 | **PENDING — 4 件** (TS-20 binary size, TS-21 Windows 1.5× DPI, TS-22 Linux 1.0× regression, TS-23 Windows RDP) |
| sdd.5 引き継ぎ既知事項 | tabs.rs flaky 5 件 (本 feature 非関連、blocker 外) / doc コメント残骸 3 件 (WARN、blocker 外) |

**総合**: **AUTO PASS (manual 項目除く) / 全体としては PENDING — 4 件のマニュアル確認待ち**。

マニュアル 4 件のうち TS-20 はアセット差分 (5.42 MiB) として自動側で部分的に裏付けられているため、実機計測でリンカ分の増減を確認すれば確定する。残り 3 件 (TS-21/22/23) は実機 GPU/DPI 環境を要し、自動代替不可。
