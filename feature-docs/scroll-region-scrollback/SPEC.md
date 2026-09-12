# Feature: scroll-region-scrollback

> 要件定義書: `feature-docs/scroll-region-scrollback/REQUIREMENTS.md`
> 本書は同じ要件を実装視点で記述したものであり、要件の出所は上記ドキュメントと同一である。

## Overview

`TerminalCore::scroll_up_internal` の非全画面 (スクロール領域) 分岐に、Ghostty 1.3.0 と同一の転写条件を導入する。スクロール領域の上マージンが画面最上行であり、左右マージンが無く、代替画面が非アクティブで、core が scrollback を保持しており、設定が有効である場合に限り、領域上端から出る行を破棄せず scrollback へ転写する。条件が 1 つでも崩れれば挙動は現状とバイト単位で同一である。あわせて bool 設定 `scroll_region_scrollback_enabled` (既定 `true`) を settings.json と設定パネルに追加し、タブ生成時のシードと設定保存時の既存タブへの即時反映を行う。

## Objectives

- Codex TUI (v0.153.4) が MAIN スクリーン上で DECSTBM + 絶対カーソル移動により描画する会話履歴に、Shift+PageUp / マウスホイール / スクロールバーから到達できるようにする。
- 同一入力列に対して Ghostty 1.3.0 (PR #9907 / issue #9905) とバイト単位で一致させ、同じ Codex TUI セッションが時系列の会話として読めるようにする。
- 既存 TUI の挙動を変えない。Claude Code (代替画面 + alternate scroll)、vim、less で行の重複・順序入れ替わり・scrollback 汚染を発生させない。
- 意図的に xterm 非準拠となる挙動であるため、GUI からユーザーが元に戻せる状態を保つ。

## User Stories

### US1: 領域スクロールする TUI の履歴を遡る
Codex TUI 利用者として、会話 UI が MAIN スクリーンのスクロール領域で更新されていても過去のターンを scrollback で遡りたい。そうすれば会話全体を時系列で読み返せる。

**Acceptance Criteria:**
- [ ] AC-1: `scrollback_lines > 0` の全幅 core に `ESC[1;5r` (上マージン = 画面最上行、下マージンは最終行より上) を与え、領域が N 回スクロールするだけの `\n` を領域下端で流したとき、新しい scrollback 行がちょうど N 行あり、内容は連続する旧行 0 の内容と時系列順で一致する (自動、--lib バイト列)。
- [ ] AC-2: AC-1 のシナリオで、下マージンより下の画面行がスクロール前後で同じ位置に同じ文字を保持し、下マージン行が空である (自動、G7 / FR5)。
- [ ] AC-6: 上マージン 0 の領域内の `CSI S` (SU) は転写し、領域高さより大きい SU カウントはちょうど領域高さ分だけ転写する (自動、FR3 / FR4)。
- [ ] AC-13: Codex TUI v0.153.4 で複数ターンの会話の後、Shift+PageUp・ホイール・スクロールバーのそれぞれが過去のターンに到達し、履歴が時系列順に読め、ターンの重複や混在がない (手動、実機)。

### US2: 既存 TUI の挙動が変わらないこと
既存 TUI 利用者として、Claude Code・vim・less の見え方と scrollback が今までどおりであってほしい。そうすれば本変更を意識せずに済む。

**Acceptance Criteria:**
- [ ] AC-3: 上マージンが画面最上行でない領域 (例 `ESC[3;10r`) では、同じ LF 列で scrollback 行が 0 件増加し、変更前の in-place シフト結果と完全に一致する (自動、FR2)。
- [ ] AC-4: `ESC[?1049h` の後は上マージン 0 の領域 + LF 列で scrollback 行が 0 件増加し、`ESC[?1049l` の後は同じ列で再び転写される (自動、FR8)。
- [ ] AC-5: 上マージン 0 の領域内の `CSI L` (IL) と `CSI M` (DL) は scrollback 行を 0 件しか増やさない (自動、FR7)。
- [ ] AC-7: 領域上端での `CSI T` (SD) と `ESC M` (RI) は scrollback 行を 0 件しか増やさない (自動、FR7)。
- [ ] AC-9: 全画面スクロール経路が不変 — scrollback 内容、`scrollback_evicted_total`、`count == 1` の ScrollEvent / dirty 行最適化が従来どおり (自動、FR13)。
- [ ] AC-10: `scrollback_lines == 0` (`scrollback_capacity == 0`) かつ上マージン 0 の領域では、変更前の画面結果となり scrollback は増えない (自動、FR1 条項 d)。
- [ ] AC-14: Claude Code (代替画面 + alternate scroll、DECSET 1007) が従来どおり — ホイールは引き続きアプリ自身のスクロールを駆動し、代替画面の内容は scrollback に入らず、代替画面を抜けると従来の MAIN スクリーン表示が復元される (手動、実機)。
- [ ] AC-15: vim と less でスクロール中および後に行の重複・順序入れ替わり・可視画面の破損がなく、scrollback の内容が変更前ビルドと一致する (手動、実機)。

### US3: 転写を設定から無効化する
設定で無効化する利用者として、xterm 非準拠の転写で問題が出たときに設定パネルから切り、開いているタブに即座に反映させたい。そうすればタブを再起動せずに元の挙動へ戻せる。

**Acceptance Criteria:**
- [ ] AC-8: 設定が無効のとき、上記すべてのシナリオが変更前の scrollback 内容と画面内容を完全に再現する (自動、FR2 / NFR4)。
- [ ] AC-11: キーが既定 `true` で settings.json をラウンドトリップし、明示的な `null` は null 許容デシリアライザで `true` にフォールバックし、ネイティブ側ミラーと raw オーバーレイのマージがこれを運び、TypeScript `AppSettings` ミラーが `bun run typecheck` を通り、パネルのトグルが描画され保存される (自動、設定)。
- [ ] AC-12: `apply_settings_updates_cursor_style_and_blink_on_every_tab` (src-tauri/src/app/tests/font_settings.rs:231) と同じスタイルの `App::apply_settings` テストが、新しい値が新規タブだけでなく既存の全タブの core に届くことを検証する (自動、ライブ反映)。

## Technical Requirements

### Functional Requirements

- **FR1: Ghostty transcription condition on the region-scroll branch** — `TerminalCore::scroll_up_internal` (crates/term_core/src/ring_buffer.rs:330-368) の非全画面分岐は、次のすべてが成立する場合に限り、領域上端から出る `count` 行を scrollback へ転写する: (a) `scroll_region_top == 0`; (b) 左マージンも右マージンも無い (`left == 0`、`right == cols - 1`) — FR6 参照; (c) 代替画面が非アクティブ (`get_mode(MODE_ALT_SCREEN)` が false、terminal_core/types.rs:29); (d) core が scrollback を保持 (`scrollback_capacity > 0`); (e) 設定 `scroll_region_scrollback_enabled` が有効。下マージンは明示的に無制約であり、`bottom < rows - 1` でも転写する。条件 (a) は「領域から出る行がビューポート最上行そのものである」ことを意味し、既存の `ring_push_blank` 追い出し経路 (ring_buffer.rs:121-225) がその圧縮形式を変えずに転写を担う。
- **FR2: Fallback to the current in-place region shift** — FR1 の条項がいずれか成立しない場合、挙動は現状とバイト単位で同一であり、`shift_rows_up(top, bottom, count)` が領域内で delete-line 相当の in-place シフトを行い scrollback 行は作られない。これは Ghostty 自身のフォールバック (G2) であり、設定が無効のとき・代替画面がアクティブなとき・`scrollback_capacity == 0` のときに求められる挙動でもある。
- **FR3: Every `scroll_up_internal` entry point is covered** — FR1 は `scroll_up_internal` のすべての呼び出し元に一律適用する: LF 経路 (`TerminalCore::line_feed`、terminal_core.rs:874-887。C0 LF ハンドラ、print-wrap 経路、terminal_dispatch.rs の ASCII 高速経路から到達)、`ESC D` (IND) / `ESC E` (NEL) の `esc_index` / `esc_next_line` (esc_handler.rs:37-49)、`CSI S` (SU) の `handle_scroll_up` (csi_scroll.rs:8-11)。Ghostty も `index()` と `scrollUp()` に同一条件を適用しており (G3)、条件は個々の呼び出し元ではなく `scroll_up_internal` の内部に置く。
- **FR4: Count clamping** — 転写行数は ring_buffer.rs:334 で既にクランプ済みの `count.min(bottom - top + 1)` に一致する (Ghostty G5)。領域高さより大きいカウントの SU は最大でも領域高さ分しか転写せず、行は古い順に push されるため scrollback の順序は画面上の並びと一致する。
- **FR5: Rows below the bottom margin do not move on screen** — `top == 0` かつ `bottom < rows - 1` のとき (Ghostty G7)、操作後は: 出て行った行 0 が scrollback にあり、画面行 1..bottom が 1 行分上にシフトし、下マージン行 `bottom` が空 (`cursor.bg` による BCE 埋め。現行の `ring_push_blank` と `shift_rows_up` の双方に一致) であり、行 `bottom+1 ..= rows-1` は操作前とまったく同じ内容・属性をまったく同じ画面位置に保持している。`ring_push_blank` は `ring_head` を回転させて新しいビューポート最下行を空にするため、領域外の末尾行は操作の一部として元の画面位置へ復元する必要がある。
- **FR6: Left/right margin clause stated even though DECSLRM is unimplemented** — eMterm は DECSLRM / DECLRMM を実装していないため、FR1 の条項 (b) は今日の時点では自明に真である (G8)。それでも仕様上は規範として記述し、将来の DECSLRM 実装が「水平マージンが設定されている間は転写を抑止する」制約に最初から縛られるようにする。
- **FR7: Exclusions: downward scroll and line insert/delete never transcribe** — 次では scrollback 行を一切作らない: SD / `CSI T` (`handle_scroll_down` -> `scroll_down_internal` -> `shift_rows_down`、csi_scroll.rs:14-16、ring_buffer.rs:371-376)、RI / `ESC M` (`esc_reverse_index` -> `scroll_down_internal`、esc_handler.rs:52-58)、IL / `CSI L` (`handle_insert_lines` -> `shift_rows_down`、csi_edit.rs:8-17)、DL / `CSI M` (`handle_delete_lines` -> `shift_rows_up`、csi_edit.rs:20-29)。Ghostty の `reverseIndex()` / `scrollDown()` および `insertLines()` / `deleteLines()` も同一の挙動である (G4)。csi_edit.rs の直接の `shift_rows_up` / `shift_rows_down` 呼び出し箇所は変更しない。
- **FR8: Alternate screen is excluded by an explicit mode gate** — `MODE_ALT_SCREEN` が設定されている間 (DECSET 47 / 1047 / 1049、csi_modes.rs:50-84)、領域分岐は現状どおりに振る舞う (FR2)。このゲートは省略できない: eMterm には代替画面専用の core もバッファも無く、代替画面は同じ ring buffer と同じ scrollback deque を共有しており、`process_pty_data_fully` が返すバッファ切替のモードアクションはライブポンプで破棄されている (src-tauri/src/tabs/mod.rs:887)。
- **FR9: Setting: settings.json key** — bool の `scroll_region_scrollback_enabled` を `app_settings::Settings` (crates/app_settings/src/settings.rs) に追加する。配線は既存の `alternate_scroll_enabled` を厳密に踏襲し、既存の `deserialize_null_with!` マクロ一覧 (settings.rs:29-37, 104, 394-397) 経由で `#[serde(default = "default_scroll_region_scrollback_enabled", deserialize_with = "deserialize_null_scroll_region_scrollback_enabled")]` を付与し、構造体の `Default` 実装に対応するエントリを追加する。**既定値は `true`** (Ghostty はこの挙動に設定を持たず無条件に適用しており (G6)、本機能の目標は設定なしでの Ghostty パリティであるため)。
- **FR10: Setting: settings-panel toggle and its four mirrors** — このキーを設定パネルのターミナル動作 (Behavior) サブセクションのトグルとして露出し、`alternate_scroll_enabled` を端から端まで踏襲する: (1) ネイティブ側ミラーのフィールドと既定値 — src-tauri/src/settings/mod.rs (:140、:294 参照); (2) オプショナルなオーバーレイのマージ — src-tauri/src/settings/raw.rs (:89、:498-500 参照); (3) TypeScript `AppSettings` ミラー — src-tauri/web-shared/settings/types.ts (:53 参照); (4) alternate scroll トグルに隣接する `renderToggle(...)` エントリ — src-tauri/web-shared/settings/sections/terminal-behavior-section.ts (:136-147 参照) で `ctx.saveSetting("scroll_region_scrollback_enabled", v)` を呼ぶ; (5) `settings.terminal.*` 配下のラベルキーと説明キーを src-tauri/web-shared/i18n/locales/en.json と ja.json の**両方**に追加 (:100-101 参照)。
- **FR11: Seeding at tab spawn** — `Tab::build` (src-tauri/src/tabs/mod.rs:671-681) が `TerminalCore::new` の直後に、`TerminalCore` の新しい公開セッター経由で設定値を新規 core にシードする。位置とスタイルは既存の `core.set_cursor_blink(settings.cursor_blink)` / `core.set_cursor_style(...)` と同じ。core 側フィールドの既定値は `true` であり、シードされていない core も Ghostty 互換に振る舞う。
- **FR12: Live apply to every existing tab on settings save** — `App::apply_settings` (src-tauri/src/app/font_settings.rs:529、per-tab ループは :603-627) が、既に `set_cursor_blink` / `set_cursor_style` を呼んでいる同じ `tab.core.lock()` ブロック内 (:621-625) で新しい値を既存の全タブの core に適用する。タブ再起動・PTY 再生成・scrollback 破棄は伴わない。これは `cursor_blink` / `fold_enabled` のライブ反映の前例に従うものであり、`scrollback_lines` の生成時のみ反映の前例 (既存 scrollback を破棄するがゆえに生成時のみ) には従わない。
- **FR13: Render bookkeeping stays correct on the transcribing region path** — 転写を行う領域経路では、画面内容が変化した行 (`top..=bottom`) のみを dirty とし、`count == 1` の全画面分岐が使う全画面 `ScrollEvent` / `shift_dirty_down_by_one` のキャンバスシフト最適化 (ring_buffer.rs:341-360) は出さない。この最適化はキャンバス全体をずらすため画面全体がスクロールした場合にのみ妥当である (消費者は src-tauri/src/render/terminal_grid_pass/builder.rs:131)。全画面分岐自体は変更しない。
- **FR14: Scrollback-length accounting stays consistent for the parked view** — 転写された行は既存のすべての消費者から scrollback 1 行として数えられる: `scrollback_count()` / `get_scrollback_length()`、容量到達時の `scrollback_evicted_total`、`App::pump_all` が `ScrollPosition::OffsetFromLive` の追従補正へ渡す per-pump の scrollback デルタ (src-tauri/src/app/mod.rs:1422-1434、:1620-1628)。領域スクロールはこれまでデルタ 0 だった箇所で非 0 のデルタを生むようになるが、追従ルール自体は変更しない。

### Non-Functional Requirements

- **NFR1 - Maintainability (既存 scrollback 機構を変更しない):** `ring_push_blank` の圧縮形式 (`cell_to_slim` による SlimCell intern、`styles` / `chars` の参照カウント、`scrollback_wrapped`、ring_buffer.rs:145-199 のオーバーフロー副表クリア)、`scrollback_bypass` のスナップショット replay 契約、`OffsetFromLive` の追従ルールは、いずれもそのまま使用し再設計しない。
- **NFR2 - Performance (出力ホットパスを劣化させない):** 追加される条件は、既に通っている `scroll_up_internal` の領域分岐上での少数の整数/bool 比較である。全画面分岐、ASCII 高速経路、`process_pty_data` のバルクスクロール skip-ahead (terminal_dispatch.rs:35-89。`scroll_region_top == 0 && scroll_region_bottom == rows-1` を要求するため本機能の影響を受けない) にバイトあたりの処理を増やしてはならない。非転写経路でのアロケーションを導入しない。
- **NFR3 - Compatibility (プラットフォーム間の等価性):** Linux と Windows で同一に振る舞い、`#[cfg(unix)]` / `#[cfg(windows)]` を新たに導入しない。`crates/app_settings` は GUI ビルドと `--no-default-features` の CLI ビルドの双方でビルドされるため、新しいキーが `cargo check --no-default-features` を壊してはならない。
- **NFR4 - Usability (既定 ON かつユーザーが元に戻せる):** 出荷時の既定値は設定なしで Ghostty を再現し (G6)、意図的に xterm 非準拠な転写で問題に当たったユーザーは設定パネルから無効化して、その効果を既に開いているタブで即座に確認できる (FR12)。
- **NFR5 - Style (フォーマットとスタイルの規約):** Rust は `make fmt` 経由の rustfmt (style_edition 2024)、TypeScript は Biome。クレート全体のフォーマットは走らせず、触れたファイルのみをフォーマットする。

## Implementation Approach

### Architecture

**System Architecture:**
```
┌──────────────────────────────────────────────────────────┐
│ 設定パネル (WebView / TypeScript)                        │
│   terminal-behavior-section.ts  renderToggle             │
│   web-shared/settings/types.ts  AppSettings              │
│   web-shared/i18n/locales/{en,ja}.json                   │
├──────────────────────────────────────────────────────────┤
│ ネイティブ設定層 (Rust)                                  │
│   src-tauri/src/settings/{mod.rs, raw.rs}  ミラー/マージ │
│   crates/app_settings/src/settings.rs      serde スキーマ│
├──────────────────────────────────────────────────────────┤
│ アプリ層 (Rust)                                          │
│   App::apply_settings (font_settings.rs) 既存全タブへ反映│
│   Tab::build (tabs/mod.rs)               生成時シード    │
├──────────────────────────────────────────────────────────┤
│ ターミナルコア (crates/term_core)                        │
│   scroll_up_internal (ring_buffer.rs:330-368)            │
│     ├─ 全画面分岐        : 変更なし                      │
│     └─ 領域分岐 + FR1 条件                               │
│          ├─ 成立 → ring_push_blank で転写 + FR5 復元     │
│          └─ 不成立 → shift_rows_up (現状どおり)          │
├──────────────────────────────────────────────────────────┤
│ scrollback リング + レンダリング記帳                     │
│   scrollback_count / scrollback_evicted_total            │
│   dirty 行 / ScrollEvent (builder.rs:131)                │
└──────────────────────────────────────────────────────────┘
```

**Component Diagram:**
```
settings.json ──┬─> app_settings::Settings (FR9)
                ├─> ネイティブ設定ミラー / raw オーバーレイ (FR10-1,2)
                └─> AppSettings (TS) ─> 設定パネルのトグル (FR10-3,4,5)
                                           │ saveSetting
                                           v
                                   App::apply_settings (FR12)
                                           │        Tab::build (FR11)
                                           v            v
                                   TerminalCore の新フィールド
                                           │
                                           v
                                 scroll_up_internal (FR1-FR5, FR13)
                                           │
                                           v
                                 scrollback リング (FR14)
```

### Data Flow

```
PTY バイト列 → process_pty_data → LF / IND / NEL / SU (FR3)
                                        │
                                        v
                              scroll_up_internal
                                        │
              ┌─────────────────────────┴──────────────────────────┐
        FR1 条件 a-e 成立                                  いずれか不成立
              │                                                    │
              v                                                    v
   ring_push_blank で行を scrollback へ (FR4 の件数)      shift_rows_up (FR2)
              │                                                    │
              v                                                    v
   領域外末尾行を元位置へ復元 (FR5)                        scrollback 増加なし
              │
              v
   dirty = top..=bottom、ScrollEvent は出さない (FR13)
              │
              v
   scrollback_count / evicted_total / per-pump デルタ (FR14)
              │
              v
   ScrollPosition::OffsetFromLive の追従補正 (EC-4)
```

### API Design

該当なし — 本機能はネットワーク API を追加しない (ターミナルコア内部の分岐と設定キー 1 件の追加のみ)。

### Database Schema

該当なし — 永続ストアを持たない。追加される永続データは settings.json の bool キー 1 件のみである (FR9)。

### Dependencies

**Internal Dependencies:**
- `crates/term_core`: `scroll_up_internal` / `ring_push_blank` / `shift_rows_up` / `shift_rows_down` / `get_mode(MODE_ALT_SCREEN)` — 転写条件とフォールバックの実装先 (FR1-FR8、FR13)。
- `crates/app_settings`: `Settings` の serde スキーマと `deserialize_null_with!` マクロ一覧 — 新しいキーの定義先 (FR9)。GUI と `--no-default-features` の双方でビルドされる (NFR3)。
- `src-tauri/src/settings`: ネイティブ側ミラーと raw オーバーレイのマージ (FR10)。
- `src-tauri/src/tabs`: `Tab::build` の生成時シード (FR11)、ライブポンプによるモードアクション破棄 (FR8 の根拠)。
- `src-tauri/src/app`: `App::apply_settings` のライブ反映 (FR12)、per-pump の scrollback デルタと `OffsetFromLive` 補正 (FR14)。
- `src-tauri/src/render/terminal_grid_pass/builder.rs`: `ScrollEvent` の消費者 (FR13)。
- `src-tauri/web-shared/settings` および `web-shared/i18n`: TypeScript ミラー、トグル、ラベル (FR10)。

**External Dependencies:**
- 新規の外部依存なし。参照事実としての Ghostty 1.3.0 (ghostty-org/ghostty、PR #9907 / issue #9905) は挙動の基準であり、コード上の依存ではない。

### File Structure

```
crates/
├── term_core/src/
│   ├── ring_buffer.rs                # scroll_up_internal の領域分岐 (FR1-FR5, FR13)
│   ├── terminal_core.rs              # 新フィールド + 公開セッター (FR11, FR12)
│   └── csi_scroll.rs                 # 新規テストを既存の領域スクロールテストの隣に置く場合のみ (前提 a8)
└── app_settings/src/
    └── settings.rs                   # scroll_region_scrollback_enabled (FR9)
src-tauri/
├── src/
│   ├── settings/mod.rs               # ネイティブ側ミラー + 既定値 (FR10-1)
│   ├── settings/raw.rs               # オーバーレイのマージ (FR10-2)
│   ├── tabs/mod.rs                   # Tab::build のシード (FR11)
│   └── app/font_settings.rs          # apply_settings のライブ反映 (FR12)
└── web-shared/
    ├── settings/types.ts             # AppSettings ミラー (FR10-3)
    ├── settings/sections/terminal-behavior-section.ts   # renderToggle (FR10-4)
    └── i18n/locales/{en,ja}.json     # ラベル + 説明 (FR10-5)
```

**Out of scope (前提 a5 / a6):**
- `src-tauri/src/mux/**` — `MuxPane` は `TerminalCore` を保持せず、設定の伝搬は不要。
- `src-tauri/src/tabs/replay.rs` の off-thread スナップショット replay と swap 経路 — 既知の制限として EC-7 に記録。

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/scroll-region-scrollback/**`
- `test-docs/scroll-region-scrollback/**`

`feature-docs/scroll-region-scrollback/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/scroll-region-scrollback/**` covers
`test-docs/scroll-region-scrollback/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it.

## Test Scenarios

### Unit Tests

- [ ] TS-1 (crates/term_core --lib、`TerminalCore::process_pty_data` / `process_pty_data_fully`): DECSTBM (上マージン = 画面最上行、下マージンは最終行より上) + 領域下端での LF。scrollback の内容と順序、下マージンより下の行が位置的に不変であること、下マージン行が空であることを検証。AC-1、AC-2 をカバー。
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- [ ] TS-2 (crates/term_core --lib、`TerminalCore::process_pty_data`): バイト列で駆動する否定ケース — 非 0 上マージン、代替画面切替 (`ESC[?1049h` / `l`)、IL/DL、SD/RI、設定無効、`scrollback_lines == 0`。いずれも scrollback 増加 0 と変更前の画面状態を検証。AC-3〜AC-5、AC-7、AC-8、AC-10 をカバー。
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- [ ] TS-3 (crates/term_core --lib、`CSI S` 経由の `TerminalCore::handle_scroll_up`): 上マージン 0 の領域内での SU が転写すること、過大なカウントが領域高さにクランプされること、全画面 SU の挙動と ScrollEvent 最適化が不変であること。AC-6、AC-9 をカバー。
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib`
- [ ] TS-4 (crates/app_settings --lib、Settings の serde ラウンドトリップ): 既定 `true`、明示 `false` のラウンドトリップ、`null` → 既定 (crates/app_settings/src/settings/tests.rs のスタイル)。AC-11 の Rust 側をカバー。
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/app_settings/Cargo.toml --lib`
- [ ] TS-5 (src-tauri --lib、`App::apply_settings`): 既存の全タブ core へのライブ反映と、`Tab::build` の生成時シード。AC-12、FR11 をカバー。
  `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- [ ] TS-6 (bun test、src-tauri/web-shared/settings/sections/terminal-behavior-section.test.ts): 新トグルが Behavior サブセクションに描画され、正しいキーで `ctx.saveSetting` を呼ぶこと。各設定セクションテストの `AppSettings` フィクスチャにフィールドを追加する。AC-11 の TypeScript 側をカバー。
  `bun test`

### Integration Tests

- [ ] TS-7 (static、typecheck / feature-gate check): TypeScript ミラーが `tsc --noEmit` を通ること、新しい `app_settings` フィールドを含む CLI のみビルドがコンパイルできること (`cargo check --no-default-features`)。NFR3 をカバー。
  `bun run typecheck`

### E2E Tests

**Existing E2E tests**: None — 本プロジェクトには E2E 基盤が存在しない (test/README.md、前提 a7)。
**Run command**: Not detected

退行の証跡は上記のバイト列 --lib ユニットテストと、下記 3 件の実機確認で構成する (前提 a7、回答済み決定 `manual_plus_unit_byte_stream`)。

- [ ] TS-8 (手動、実機リリースビルド、Codex TUI v0.153.4): Shift+PageUp / ホイール / スクロールバーのそれぞれが過去の会話ターンに時系列順で到達する。AC-13 をカバー。
- [ ] TS-9 (手動、実機リリースビルド、Claude Code): 代替画面 + alternate scroll の挙動が不変で、代替画面の内容が scrollback に入らない。AC-14 をカバー。
- [ ] TS-10 (手動、実機リリースビルド、vim / less): 領域スクロール中および後に行の重複・順序入れ替わり・可視破損がない。AC-15 をカバー。

### Edge Cases

- [ ] EC-1: `top == 0 && bottom == rows - 1` は既存の全画面分岐であり領域分岐ではない。挙動は不変 (既に転写している)。
- [ ] EC-2: `scrollback_capacity == 0` では scrollback 行を作らず、表示結果は変更前の in-place シフトと一致する (FR1 条項 d)。
- [ ] EC-3: 容量到達時の転写では、既存の `ring_push_blank` 経路で最古の scrollback 行が追い出され `scrollback_evicted_total` が増える。新しい追い出し記帳は追加しない。
- [ ] EC-4: 固定表示ビュー (`ScrollPosition::OffsetFromLive`) では、領域スクロールも per-pump の scrollback デルタに寄与し、全画面スクロールと同様に補正される。オフセット上限で固定ビューがずれるのは既に受容済みの挙動 (src-tauri/src/app/mod.rs:1620-1628)。
- [ ] EC-5: 絶対カーソル位置で再描画する TUI (Codex TUI の形) は、後で画面上で書き換える行を転写しうる。転写されたコピーはその時点のスナップショットであり、意図された Ghostty パリティの結果であって欠陥ではない。
- [ ] EC-6: `ring_wrapped` フラグは、転写される行の既存 wrap フラグをそのまま `scrollback_wrapped` へ持ち込む。領域行は通常 wrap の継続ではなく、リサイズ時の reflow は他の scrollback 行と同様に扱う。
- [ ] EC-7: off-thread スナップショット replay core (src-tauri/src/tabs/replay.rs) は設定シードなしでワーカーが構築し、`apply_offthread_swap` はコールバックを移植するがこのフィールドは移植しない。core 側既定が `true` のため既定ユーザーには不可視だが、設定を無効化したユーザーは次の `apply_settings` まで swap 後の core で転写が再開しうる。本機能のスコープ外 (前提 a6) とし、既知の制限として記録する。
- [ ] EC-8: `process_pty_data` のバルクスクロール skip-ahead (terminal_dispatch.rs:35-89) は全画面領域を条件とするため、新しい分岐と相互作用しない。
- [ ] EC-9: 領域がアクティブな状態で届くリサイズは既存の resize/reflow 経路をそのまま通り、転写された行はそこでは通常の scrollback 行である。

### Performance Tests

該当なし — 数値目標を伴う負荷試験は分析に含まれていない。パフォーマンスの要求は NFR2 (ホットパスにバイトあたりの処理とアロケーションを追加しない) であり、AC-9 の全画面経路不変チェックと、変更箇所が既に通っている領域分岐上の整数/bool 比較に限られることで担保する。

## Security Considerations

- **Authentication / Authorization:** 該当なし — ローカルのターミナルエミュレータ内部処理であり、認証・認可の主体が存在しない。
- **Input Validation:** 新しい設定キーは既存の null 許容 serde ヘルパーを通す素の bool であり、不正・悪意ある settings.json でも `{true,false}` 以外の値にはならない。
- **Parsing Surface:** 新しいパース面を追加しない。新しいエスケープシーケンスも新しい外部入力も増えず、既にパース済みの SU / IND / NEL / LF が「これまで破棄していた行」をどう扱うかだけが変わる。
- **Data Protection:** これまで消えていたターミナル内容がメモリ内の scrollback リングに残るようになる。保持は既存の `scrollback_lines` 容量と同じインプロセス寿命で引き続き制限され、ディスクへの新規永続化も IPC への新規露出もない。
- **XSS / SQL Injection / CSRF:** 該当なし — 追加する UI は既存設定パネルの bool トグル 1 件であり、新しい動的 HTML も、データベースも、HTTP エンドポイントも導入しない。

## Error Handling

### Error Codes

該当なし — 本機能はエラーコードを定義しない。条件が成立しない場合はエラーではなく FR2 の従来挙動へのフォールバックとなる。

### Error Flow

```
FR1 の条件 a-e のいずれか不成立 → shift_rows_up による領域内シフト (FR2)
                                 → scrollback 行は作らない (エラー通知なし)
```

## Performance Optimization

### Performance Goals

- 出力ホットパス (全画面分岐、ASCII 高速経路、`process_pty_data` のバルクスクロール skip-ahead) にバイトあたりの処理を追加しない (NFR2)。
- 非転写経路でアロケーションを追加しない (NFR2)。

### Optimization Strategies

- 条件判定は既に通っている領域分岐の内部に置き、少数の整数/bool 比較で済ませる (NFR2)。
- 転写は既存の `ring_push_blank` 経路をそのまま使い、圧縮形式や副表の扱いを再設計しない (NFR1)。
- 全画面用のキャンバスシフト最適化 (`ScrollEvent` / `shift_dirty_down_by_one`) は領域経路では出さず、dirty 行を `top..=bottom` に限定する (FR13)。

### Caching Strategy

該当なし — 新しいキャッシュ層を導入しない。

## Success Criteria

- [ ] FR1〜FR14 のすべてが実装され、テストされている
- [ ] AC-1〜AC-12 の自動テスト (TS-1〜TS-7) が通る
- [ ] AC-13〜AC-15 の実機確認 (TS-8〜TS-10) が完了している
- [ ] NFR2 のとおりホットパスにバイトあたりの処理とアロケーションが追加されていない
- [ ] NFR3 のとおり `cargo check --no-default-features` と `bun run typecheck` が通る
- [ ] NFR5 のとおり触れたファイルのみ rustfmt / Biome でフォーマットされている
- [ ] REQUIREMENTS.md と SPEC.md の要件 ID が一致している

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- なし — 要件分析時点で `status: tbd` の要件は存在しない。EC-7 の off-thread swap 経路は未解決の質問ではなく、スコープ外の既知の制限である (前提 a6)。

## Assumptions

要件分析が確定した前提。いずれも可逆である。

| ID | 前提 | 根拠 |
|----|------|------|
| a1 | 設定キー名は `scroll_region_scrollback_enabled` (Rust / settings.json は snake_case、TypeScript `AppSettings` ミラーも同一キー文字列。`alternate_scroll_enabled` の前例に一致) | 名前を固定する入力が供給されていないが、1 つ定めることで下流ドキュメントの一貫性が保たれる。未出荷キーの改名は 5 つのミラーにまたがる機械的編集で済む |
| a2 | 既定値は `true` (有効) | Ghostty 事実 G6: Ghostty は無条件に適用しており、本機能の目標は設定なしの Ghostty パリティ |
| a3 | eMterm は DECSLRM / DECLRMM を実装していないため、FR1 の左右マージン条項は今日は自明に真であり、前方制約としてのみ仕様に載る | 事実 G8。crates/term_core/src/csi_*.rs に DECSLRM ハンドラが存在しない |
| a4 | 代替画面の除外は明示的な `get_mode(MODE_ALT_SCREEN)` チェックで実装する。eMterm は両画面で 1 つの core と 1 つの scrollback deque を保持しているため | csi_modes.rs:50-84 が `MODE_ALT_SCREEN` を設定しバッファ切替アクションを返すが、ライブポンプが破棄している (tabs/mod.rs:887)。唯一の消費者は app/mod.rs:1414-1421 |
| a5 | mux デーモンのペインは本機能の影響を受けず設定伝搬も不要。`MuxPane` (src-tauri/src/mux/session/pane/mod.rs:66) は `TerminalCore` を保持せず、デーモンは生バイトの scrollback を保ちクライアントがスナップショット replay で core を再構築する | 回答済みスコープ決定 `local_tabs_only`。src-tauri/src/mux/ 配下に本番の `TerminalCore::new` が存在しない |
| a6 | off-thread スナップショット replay core と swap 経路はスコープ外。core 側の既定 ON により、シードなしでも Ghostty 互換に保たれる (EC-7) | 回答済みスコープ決定 `local_tabs_only` と a2 |
| a7 | 退行の証跡は `process_pty_data` を通すバイト列 --lib ユニットテストと 3 件の実機確認であり、E2E 基盤は新設しない | 回答済み決定 `manual_plus_unit_byte_stream`。test/README.md が E2E 基盤の不在を記録している |
| a8 | 本番コードの変更は crates/term_core/src/ring_buffer.rs (`scroll_up_internal` の領域分岐と `ring_push_blank` の利用)、`TerminalCore` のフィールド/セッター、FR9-FR12 の設定配線に限定する。csi_scroll.rs は新規テストを既存の領域スクロールテストの隣に置く場合にのみ触る | ring_buffer.rs と csi_scroll.rs を対象ファイルとして挙げたタスク記述上の制約を、`scroll_up_internal` の実位置 (ring_buffer.rs:330) および回答済み決定が要求する設定面の所在と突き合わせた結果 |

## Implementation Phases (if applicable)

該当なし — 単一フィーチャーとして一括で実装する。作業分割は create-plan が `workflow.yaml` のタスクとして決定する。

## References

- 要件定義書: `feature-docs/scroll-region-scrollback/REQUIREMENTS.md`
- Ghostty 参照事実 (オーケストレータ供給、G1-G8):
  - G1: Ghostty は `scrolling_region.top == 0` かつ `scrolling_region.left == 0` かつ `scrolling_region.right == cols - 1` かつアクティブスクリーンが scrollback を保持する (primary screen) 場合に限り、出て行く先頭行を scrollback へ転写する。下マージンは無制約。
  - G2: それ以外では delete-line セマンティクスの in-place 領域シフトにフォールバックし scrollback を作らない。
  - G3: 条件は `index()` (IND/LF) と `scrollUp()` (SU / `CSI S`) に同一に適用される。
  - G4: `reverseIndex()` は scrollDown へ行き scrollback を作らない。`deleteLines()` / `insertLines()` も scrollback を作らない。
  - G5: SU は転写カウントをスクロール領域高さにクランプする。
  - G6: Ghostty はこの挙動に設定オプションを持たず無条件である。出典: ghostty-org/ghostty `src/terminal/Terminal.zig` の `index()` / `scrollUp()`、PR #9907、issue #9905、Ghostty 1.3.0 で出荷。
  - G7: `top == 0` かつ `bottom < rows - 1` のとき、下マージンより下の画面行は動いてはならない。
  - G8: eMterm は DECSLRM / DECLRMM を実装していないため、左右マージン項は自明に真だが規範として明記する。
- 実装対象コード: `crates/term_core/src/ring_buffer.rs` (:121-225, :330-368, :334, :341-360, :371-376)、`terminal_core.rs` (:874-887)、`esc_handler.rs` (:37-49, :52-58)、`csi_scroll.rs` (:8-11, :14-16)、`csi_edit.rs` (:8-17, :20-29)、`csi_modes.rs` (:50-84)、`terminal_dispatch.rs` (:35-89)
- 設定まわり: `crates/app_settings/src/settings.rs` (:29-37, 104, 394-397)、`src-tauri/src/settings/mod.rs` (:140, :294)、`src-tauri/src/settings/raw.rs` (:89, :498-500)、`src-tauri/web-shared/settings/types.ts` (:53)、`src-tauri/web-shared/settings/sections/terminal-behavior-section.ts` (:136-147)、`src-tauri/web-shared/i18n/locales/{en,ja}.json` (:100-101)
- 反映経路: `src-tauri/src/tabs/mod.rs` (:671-681, :887)、`src-tauri/src/app/font_settings.rs` (:529, :603-627, :621-625)、`src-tauri/src/app/mod.rs` (:1414-1421, :1422-1434, :1620-1628)、`src-tauri/src/render/terminal_grid_pass/builder.rs` (:131)
- テスト前例: `src-tauri/src/app/tests/font_settings.rs` (:231 `apply_settings_updates_cursor_style_and_blink_on_every_tab`)、`crates/app_settings/src/settings/tests.rs`
