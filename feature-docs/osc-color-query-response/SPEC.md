# Feature: osc-color-query-response

要件の一次情報は `feature-docs/osc-color-query-response/REQUIREMENTS.md`。本書はその実装面の記述であり、要件 ID（FR1-FR10 / NFR1-NFR6）は同文書と一致する。

## Overview

eMterm は OSC 4 / 10 / 11 / 12 の色照会（ペイロードが照会トークン `?`）を破棄しており、端末の色を問い合わせるプログラムは応答を得られない。本機能はこれらの照会に対し、ライブテーマの現在値を `ESC ] <n> ; rgb:rrrr/gggg/bbbb <terminator>` の形式で PTY に応答する。あわせて、照会が返す値と描画が実際に使う値を一致させるため、アクティブなカラースキームが指定していない色をテーマに残す既存の 2 つのリセット経路（OSC 110 / 111 と OSC 104）を訂正する。

## Objectives

- OSC 4 / 10 / 11 / 12 の色照会に PTY 上で応答し、端末の前景色・背景色・カーソル色・パレット色を調べるプログラム（Claude Code、tmux、vim/neovim の背景色判定、`$COLORFGBG` 相当のヒューリスティクス）が沈黙ではなく実際の応答を受け取り、推測した明暗テーマにフォールバックしないようにする。
- 照会が報告する値を描画が実際に使う値と同一にする。そのために、アクティブなカラースキームが指定していない色をテーマに残す既存の 2 つのリセット経路（OSC 110 / 111 と OSC 104）を訂正する。
- すべての応答を既存の単一のデバイス応答経路で配送し、tmux-startup-query-response-leak と同種の不具合（応答が稼働中シェルの標準入力に届き、プロンプトに打ち込まれる）が再発しないようにする。

## User Stories

### US1: 端末の色を問い合わせて配色を決める

端末内で動作するプログラムとして、OSC 10 / 11 / 12 / 4 の照会に対する応答を受け取りたい。推測した明暗テーマではなく、端末が実際に描画している色に基づいて自身の配色を決めるため。

**Acceptance Criteria:**

- [ ] eMterm 内で動作するシェルで `printf '\033]11;?\007'` を実行すると `\033]11;rgb:rrrr/gggg/bbbb\007` の形式の応答が表示され、報告された色が端末が描画している背景色と一致する（FR1）。
- [ ] `printf '\033]10;?\033\\'` は ST 終端の応答を受け取り、同じ照会を BEL 終端で送ると BEL 終端の応答を受け取る（FR5）。
- [ ] `printf '\033]4;5;?\007'` はインデックス 5 の現在描画されている色を応答し、`\033]4;5;rgb:11/22/33\007` の後は同じ照会が `rgb:1111/2222/3333` を応答する（FR2、FR3、FR4）。
- [ ] 一度も設定されていない 16..255 のインデックスに対する OSC 4 照会は、そのインデックスの xterm キューブ / グレースケール色を応答する（FR3）。
- [ ] 不正な照会（`\033]4;999;?\007`、`\033]4;abc;?\007`）は PTY にバイトを出さず、テーマを変更しない（FR10）。

### US2: リセット後も設定したカラースキームで表示される

非デフォルトのカラースキームを設定した eMterm 利用者として、OSC 110 / 111 / 104 の後も画面がスキームの色で描画され、照会が同じ値を返してほしい。リセットのたびにフォールバック色や OSC 4 の残留色が表示されるのを避けるため。

**Acceptance Criteria:**

- [ ] 非デフォルトのカラースキーム下で、OSC 110 の後に OSC 10 の照会を行うとスキームの前景色が報告され、画面上の可視テキスト色が `0x40ff40` ではなくスキームの前景色になる（FR7、FR8）。
- [ ] 非デフォルトのカラースキーム下で、OSC 111 の後に OSC 11 の照会を行うとスキームの背景色が報告され、可視背景が黒ではなくスキームの背景色になる（FR7、FR8）。
- [ ] `\033]4;5;rgb:11/22/33\007` の後に `\033]104\007` を送ると、インデックス 5 が再びアクティブなスキームの色 5 で描画され、インデックス 5 の OSC 4 照会も同じスキーム色を報告する（FR7、FR9）。
- [ ] `\033]104;5\007`（明示インデックス）はインデックス 5 のみを復元し、OSC 4 で設定済みの他のインデックスは描画・照会応答の両方で値を保つ（FR9）。
- [ ] OSC 112 の後の OSC 12 照会は `scheme_cursor_fg` を報告する（既存挙動が本機能で変わらない）（FR7）。

### US3: 応答がシェルに漏れない

eMterm 利用者として、照会応答が画面上のテキストやシェルプロンプトに現れないことを保証したい。過去の tmux-startup-query-response-leak と同種の不具合を再発させないため。

**Acceptance Criteria:**

- [ ] 照会が画面上にテキストとして現れることはなく、ウィンドウ切替・detach/attach サイクル・スクロールバック復元の後にシェルプロンプトに現れることもない（FR6、NFR3）。
- [ ] リプレイ経路とオフスレッドスワップ経路の応答破棄を固定している既存の回帰テストが、変更なしで通り続ける（NFR3）。

## Technical Requirements

### Functional Requirements

- **FR1 — OSC 10 / 11 / 12 色照会応答:** OSC 10、11 または 12 のペイロード要素が照会トークン `?` である場合、端末は PTY に `ESC ] <n> ; rgb:rrrr/gggg/bbbb <terminator>` を送出する。`<n>` はその要素が解決するコード（10 = デフォルト前景色、11 = デフォルト背景色、12 = カーソル前景色）であり、色はライブテーマの現在の `fg` / `bg` / `cursor_fg`。連鎖ペイロードは `Theme::apply_default_color_set` が既に行っているとおり要素ごとにコードを進める（10 → 11 → 12、対象が 12 を超えた時点で停止）ため、連鎖内の各 `?` 要素は自分のコードに対する応答を得る。照会への応答はテーマ状態を変化させず、グリッドを dirty にしない。
- **FR2 — OSC 4 パレット照会応答:** `index;?` 形式の OSC 4 ペイロードペアは `ESC ] 4 ; index ; rgb:rrrr/gggg/bbbb <terminator>` を送出する。ペイロードは複数ペアを連鎖でき（`1;?;200;rgb:00/aa/00;5;?`）、各 `?` ペアはペイロード順にそれぞれの応答要素を生成する。インデックスが 10 進整数として解釈できない、または 256 以上のペアは応答も状態変化も生じない — 既存の `apply_palette_set` のスキップ挙動（`src-tauri/src/render/theme.rs:330-341`）に倣う。
- **FR3 — 照会するパレット値の解決:** パレットインデックス `i` について報告する色は、その時点で描画が `i` に対して使う色と厳密に一致する。すなわち、疎なオーバーレイが `Some` を保持していれば `palette256[i]`、そうでなく `i < 16` なら `palette16[i]`、そうでなく `16..=255` なら xterm の 256 色キューブ / グレースケール式。`Theme::palette256` は疎なオーバーレイであり `None` は「デフォルトを使う」を意味する（`src-tauri/src/render/theme.rs:62-66`）ため、照会が `None` を欠損として報告してはならない。
- **FR4 — set と照会が混在するペイロード:** set と照会が混在するペイロードは、ペイロード順に各 set を適用し各照会に応答する。既存の set 意味論は不変。OSC 4 の `i < 16` は `palette256[i]` と `palette16[i]` の両方に書き込み（`src-tauri/src/render/theme.rs:342-348`）、OSC 12 は `cursor_fg_override_active` を立て（`:376-380`）、空または解釈不能な spec 要素は状態を変えずスキップする（`:362-365`）。連鎖の途中の照会要素は次の要素を消費せず、連鎖のコード進行をずらさない。
- **FR5 — 応答の文字列終端子:** 応答の文字列終端子は要求の終端子に一致する。BEL 終端の要求には BEL（`0x07`）、ST 終端の要求には ST（`ESC \`）。現状 `handle_osc_internal(param, data)`（`crates/term_core/src/osc_handler.rs:50`）に到達する OSC ディスパッチは param とペイロードしか運んでいないため、いずれか一方をハードコードするのではなく終端子を応答生成箇所まで引き回す。
- **FR6 — 応答の単一 PTY 配送経路:** すべての色照会応答は `TerminalCore` の既存の単一スロット応答バッファに置かれ、`TerminalCore::take_response` を通じてのみ PTY に到達する。ドレインは `Tab::write_device_response` へ転送する既存の 3 箇所 — `process_outer_via_core`（`src-tauri/src/tabs/output_pipeline.rs:117-128`）、`apply_active_pane_output`（`:186-204`）、`apply_queued_live_output`（`src-tauri/src/tabs/replay.rs:822-835`）で行う。新たな PTY 書き込み経路を導入せず、`take_response` のライブ消費者を追加しない。mux 越しでは、応答は現在の CPR 応答と同様に発信元のリモートペインへフレーミングして返される。
- **FR7 — リセット後に照会が返す値:** リセットシーケンスの後、照会はすべての場合においてアクティブなカラースキームの値を報告する。OSC 110 の後は OSC 10 の照会がスキームの前景色を、OSC 111 の後は OSC 11 の照会がスキームの背景色を、OSC 112 の後は OSC 12 の照会が `scheme_cursor_fg` を（`src-tauri/src/render/theme.rs:313-321` で既に実現されている挙動）、OSC 104 の後は OSC 4 の照会がインデックス 0-15 についてスキームのパレット値と 16-255 について xterm キューブ / グレースケール値を報告する。リセット後の照会が、描画が同時に使っていない値を報告することはない。アクティブなカラースキームが無い場合（`settings.terminal_color_scheme` が空または未知で `apply_color_scheme` がテーマを変更しない — `:585-610`）、スキーム値は `Theme::default()` のシードである `DEFAULT_TERMINAL_FG` / `DEFAULT_TERMINAL_BG` / `DEFAULT_PALETTE16`。
- **FR8 — OSC 110 / 111 をアクティブなスキームの前景色 / 背景色へリセット:** `Theme` に `scheme_fg` と `scheme_bg` を追加する。既存の `scheme_cursor_fg`（`src-tauri/src/render/theme.rs:45-51`）に倣い、`Theme::default()` で `DEFAULT_TERMINAL_FG` / `DEFAULT_TERMINAL_BG` からシードし（`:166-167`）、`apply_color_scheme` のプリセット分岐で `theme.fg` / `theme.bg` と並んで更新し（`:600-607`）、`apply_user_scheme` では `theme.fg` / `theme.bg` を更新するのと同じパースガード条件下で更新する（`:612-622`）。OSC 10 / 11 自体はこれらを変更しない。`Theme::apply_osc(110)` と `(111)` は `DEFAULT_TERMINAL_FG` / `DEFAULT_TERMINAL_BG` 定数ではなくこれらのフィールドから `fg` / `bg` を復元する（`:305-312`）。これは照会だけの問題ではなく描画に現れる。非デフォルトのスキーム下では、OSC 110 は現在フォールバックの明るい緑 `0x40ff40` で画面を再描画するが、本変更後はスキームの前景色で再描画する。定数は `Theme::default()` のシードとして残す。
- **FR9 — OSC 104 がアクティブなスキームから palette16 を復元する:** `Theme` に `palette16` のミラーである `scheme_palette16` を追加し、`DEFAULT_PALETTE16` からシードして、カラースキームが適用されるすべての箇所（`apply_color_scheme` のプリセット分岐、および `apply_user_scheme` の既存パースガード下でインデックスごと）で更新する。`apply_palette_reset` はペイロードが空の場合に `palette256` オーバーレイ全体をクリアし、加えて `scheme_palette16` から `palette16` を復元する。明示インデックス列がある場合は、列挙された各インデックスについてオーバーレイのスロットをクリアし、`index < 16` であれば `scheme_palette16[index]` から `palette16[index]` を復元する。`changed` の戻り値はオーバーレイまたは `palette16` のいずれかが実際に変化したときに true となり、新たにカバーされるケースでも描画が dirty にマークされる。これは照会だけの問題ではなく描画に現れる。OSC 4 は `i < 16` について `palette256[i]` だけでなく `palette16[i]` にも書き込むため、「インデックス 5 を設定してから OSC 104」を行うと現状インデックス 5 は OSC 4 の色で恒久的に描画され続ける。これを呼び出し側に委ねる旨の古いコメント（`src-tauri/src/render/theme.rs:409-411`）は削除する。
- **FR10 — 不正・非対応・非照会入力の無害化:** テーマが所有しない OSC コード中の `?`、範囲外または解釈不能なパレットインデックス、不正な連鎖ペア、`?` でもパース可能な色指定でもない spec 要素は、応答バイトも状態変化も生じさせない。`Theme::apply_osc` の既存の「目に見える変化が無ければ false を返す」契約を維持し、照会への応答が不要な `mark_all_dirty` を引き起こさないようにする。

### Non-Functional Requirements

- **NFR1 — Maintainability（既存の応答フォーマッタの再利用）:** 応答は既存の `term_core::color_spec::format_color_response`（`crates/term_core/src/color_spec.rs:89-96`）でフォーマットする。この関数は各 8 ビット成分を 16 ビットに展開し（`0xAB -> 0xABAB`）、`rgb:rrrr/gggg/bbbb` を返す。2 つ目のフォーマッタは導入しない。現在この関数の唯一の呼び出し元は自身のインラインテストであり、本機能が初めて製品コードの呼び出し元を与える。
- **NFR2 — Performance / Correctness（コアレスが応答を消失・上書きしない）:** `TerminalCore` の応答バッファは単一スロットであるため、OSC 色照会を運ぶ mux の `PtyOutput` フレームは、後続の照会が未配送の先行応答を上書きしうる形でバッチ処理されてはならない。既存のゲートは `Tab::pty_output_batch_eligible` → `payload_has_device_query`（`src-tauri/src/tabs/output_pipeline.rs:167-175`、`super::input` からインポート）であり、「照会を含むフレームは、後続の照会が term_core の単一スロット応答バッファを上書きする前に応答を確保できるよう、単独でパースされなければならない」と文書化されている。この分類器が OSC 色照会をデバイス照会として扱うか、さもなくばコアレス経路がそれらに対して安全であることを示す必要がある。
- **NFR3 — Security（リプレイ隔離の維持）:** リプレイ経路およびオフスレッドスワップ経路は保留中のデバイス応答を破棄し続ける（`src-tauri/src/tabs/replay.rs:96-105`、および `:569` の同等箇所）。これにより、とうに終了したプログラムがスナップショットのバイト列に焼き込んだ色照会が稼働中シェルの標準入力に届くことはない。この破棄を固定している既存の回帰テストは変更なしで通り続けなければならない。
- **NFR4 — Architecture（term_core は GUI テーマから独立を保つ）:** `crates/term_core` は応答バッファを所有するが `src-tauri/src/render/theme.rs` にアクセスできない。照会は GUI 層のライブな `Theme` から応答する必要があり、その際 term_core が GUI クレートや `Theme` への依存を獲得してはならない。
- **NFR5 — Compatibility（CLI 専用ビルドへの非影響）:** term_core 側の変更は `--no-default-features` でコンパイルでき、GUI 専用の依存を導入しない。`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` で検証する。
- **NFR6 — Portability（クロスプラットフォーム同等性）:** 挙動は Linux と Windows で同一。応答経路のいかなる部分も Unix 専用 API に依存してはならない（プロジェクトは `libc` を `cfg(unix)` でゲートしている）。macOS は対象外。

## Implementation Approach

### Architecture

**System Architecture:**

```
┌─────────────────────────────────────────────────┐
│  端末内のプログラム（Codex TUI / Claude Code 等）│
├─────────────────────────────────────────────────┤
│  PTY                                            │
├─────────────────────────────────────────────────┤
│  src-tauri/src/tabs/ 出力パイプライン           │
│    process_outer_via_core                       │
│    apply_active_pane_output                     │
│    apply_queued_live_output                     │
│    → Tab::write_device_response                 │
├─────────────────────────────────────────────────┤
│  crates/term_core                               │
│    OSC パーサー / osc_handler                   │
│    単一スロット応答バッファ + take_response     │
│    color_spec::format_color_response            │
├─────────────────────────────────────────────────┤
│  src-tauri/src/render/theme.rs（GUI 層）        │
│    Theme: fg / bg / cursor_fg / palette16 /     │
│           palette256 / scheme_* ミラー          │
│    apply_osc / apply_palette_set /              │
│    apply_palette_reset / apply_color_scheme     │
└─────────────────────────────────────────────────┘
```

**Component Diagram:**

- `crates/term_core` — OSC のパースとディスパッチ、単一スロット応答バッファ、`format_color_response`（NFR1）。GUI 層の `Theme` に依存しない（NFR4）。
- `src-tauri/src/render/theme.rs` — 照会値の解決元となるライブな `Theme` と、FR8 / FR9 が追加するスキームミラーのフィールド。
- `src-tauri/src/tabs/` — 応答のドレインと PTY への転送（FR6）、コアレス判定（NFR2）、リプレイ時の応答破棄（NFR3）。

term_core は応答バッファを所有するが `Theme` を見られないため、照会の応答値は GUI 層で解決する必要がある（NFR4）。`Theme` は `Settings` のハンドルを持たないため、リセット時に復元できるのはミラーフィールドからのみである（A3）。

### Data Flow

```
プログラム → PTY → tabs 出力パイプライン → term_core OSC パース
                                              ↓ (? 照会)
                               GUI 層 Theme で色を解決（FR1/FR2/FR3）
                                              ↓
                          format_color_response でフォーマット（NFR1）
                                              ↓
                             単一スロット応答バッファ（FR6）
                                              ↓ take_response
             write_device_response → PTY → プログラム（mux では発信元ペインへ）
```

### API Design

本機能の外部インターフェースは PTY 上の制御シーケンスである。HTTP API は存在しない。

#### OSC 10 / 11 / 12（FR1、FR5）

**Request:**

```
ESC ] 10 ; ? <terminator>          # デフォルト前景色
ESC ] 11 ; ? <terminator>          # デフォルト背景色
ESC ] 12 ; ? <terminator>          # カーソル前景色
ESC ] 10 ; ? ; ? ; ? <terminator>  # 連鎖: コードが 10 → 11 → 12 と進む
```

**Response:**

```
ESC ] 10 ; rgb:rrrr/gggg/bbbb <terminator>
ESC ] 11 ; rgb:rrrr/gggg/bbbb <terminator>
ESC ] 12 ; rgb:rrrr/gggg/bbbb <terminator>
```

`<terminator>` は要求の終端子に一致する（BEL = `0x07`、ST = `ESC \`）。

#### OSC 4（FR2、FR3、FR4、FR5）

**Request:**

```
ESC ] 4 ; index ; ? <terminator>
ESC ] 4 ; 1 ; ? ; 200 ; rgb:00/aa/00 ; 5 ; ? <terminator>   # set と照会の混在
```

**Response:**

```
ESC ] 4 ; index ; rgb:rrrr/gggg/bbbb <terminator>
```

各 `?` ペアがペイロード順にそれぞれの応答要素を生成する。

**No-Response（FR2、FR10）:**

```
ESC ] 4 ; 999 ; ? <terminator>   # index >= 256   → 応答なし・状態変化なし
ESC ] 4 ; abc ; ? <terminator>   # index が解釈不能 → 応答なし・状態変化なし
```

### Database Schema

該当なし。本機能は永続データを追加しない。追加するのは `Theme` 上のインメモリなスキームミラーのフィールドのみである。

| フィールド | 所有者 | シード | 更新箇所 | 用途 |
|---|---|---|---|---|
| `scheme_fg` | `Theme` | `DEFAULT_TERMINAL_FG` | `apply_color_scheme` プリセット分岐、`apply_user_scheme`（パースガード下） | OSC 110 の復元元（FR8） |
| `scheme_bg` | `Theme` | `DEFAULT_TERMINAL_BG` | 同上 | OSC 111 の復元元（FR8） |
| `scheme_palette16` | `Theme` | `DEFAULT_PALETTE16` | `apply_color_scheme` プリセット分岐、`apply_user_scheme`（インデックスごと、パースガード下） | OSC 104 の復元元（FR9） |
| `scheme_cursor_fg` | `Theme` | 既存 | 既存 | OSC 112 の復元元（既存、FR7） |

### Dependencies

**Internal Dependencies:**

- `crates/term_core`（`osc_handler.rs`、`color_spec.rs`、単一スロット応答バッファ）: OSC パースと応答フォーマット、応答の保持。
- `src-tauri/src/render/theme.rs`: 照会値の解決元、および FR8 / FR9 のリセット訂正の実装先。
- `src-tauri/src/tabs/`（`output_pipeline.rs`、`replay.rs`、`input.rs`）: 応答のドレインと PTY 転送、コアレス判定、リプレイ時の破棄。

**External Dependencies:**

- 追加なし。

### File Structure

```
crates/term_core/src/
├── osc_handler.rs        # handle_osc_internal(:50) — 終端子の引き回し（FR5）
└── color_spec.rs         # format_color_response(:89-96) — 応答フォーマット（NFR1）

src-tauri/src/render/
└── theme.rs              # Theme のスキームミラー追加（FR8/FR9）、照会値解決（FR1-FR4, FR7, FR10）

src-tauri/src/tabs/
├── output_pipeline.rs    # ドレイン 2 箇所(:117-128, :186-204)、コアレス判定(:167-175)
├── replay.rs             # ドレイン(:822-835)、応答破棄(:96-105, :569)
└── input.rs              # payload_has_device_query（NFR2）
```

具体的な変更ファイル集合は create-plan で各タスクの `files` として確定する。

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored
list: the feature-specific paths above are derived at create-plan from
every task's `files` entries in `workflow.yaml`
(`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated
entries in addition to the feature-specific paths above:

- `feature-docs/osc-color-query-response/**`
- `test-docs/osc-color-query-response/**`

`feature-docs/osc-color-query-response/**` covers `REQUIREMENTS.md`,
`SPEC.md`, `IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the
phase documents and by `references/phase-state.md`; this section cites them
and restates none of their rules.

`test-docs/osc-color-query-response/**` covers
`test-docs/osc-color-query-response/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section
cites it and restates none of its rules.

These two default entries are part of the declaration unless the SPEC
author explicitly removes them; their absence is never assumed by
silence — removal is a deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed
at verification time must be CONTAINED IN the declared set, not equal to
it. A feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared
`test-docs/osc-color-query-response/**` entry is still correct in that
case — a declared path that never materializes is not a violation.

## Test Scenarios

### Unit Tests

- [ ] **TS1** (NFR1): `format_color_response` の出力形状を境界成分（0x00、0x7f、0xff）で確認する — `crates/term_core/src/color_spec.rs` の既存インラインテストを拡張する。
- [ ] **TS2** (FR1): OSC 10 / 11 / 12 の単一照会がライブの `fg` / `bg` / `cursor_fg` を返し、ペイロードが `?` ではなく set の場合は応答を返さない。
- [ ] **TS3** (FR1): 連鎖 OSC 10 ペイロード `?;?;?` がコード 10、11、12 の順に 3 つの応答を生成し、4 要素の連鎖はコード 12 の後で停止する。
- [ ] **TS4** (FR2、FR3): OSC 4 の照会値解決 — オーバーレイ設定済み（`palette256[i] = Some`）、オーバーレイ未設定で `i < 16`（`palette16[i]` にフォールバック）、オーバーレイ未設定で `i >= 16`（xterm キューブ / グレースケール式）。
- [ ] **TS5** (FR4): 混在ペイロード `1;rgb:10/00/00;5;?` がインデックス 1 の set を適用し、インデックス 5 の照会に順序どおり応答する。
- [ ] **TS6** (FR7、FR8): プリセットスキームが有効なとき OSC 110 が `fg` を `scheme_fg` から復元する（`DEFAULT_TERMINAL_FG` ではない） — `src-tauri/src/render/theme.rs:891-898` の既存テスト `apply_osc_112_resets_cursor_fg_to_active_scheme_not_hardcoded_default` の直接的な類似。OSC 111 / `scheme_bg` も同様。
- [ ] **TS7** (FR7、FR8): `foreground` / `background` の指定がパースに失敗するユーザー定義スキーム下での OSC 110 / 111 は、`apply_user_scheme` のパースガード付き更新に一致して `Theme::default()` のシードにフォールバックする。
- [ ] **TS8** (FR7、FR9): `apply_osc(4, "5;rgb:11/22/33")` の後の `apply_osc(104, "")` がオーバーレイをクリアし、かつ `palette16[5]` をアクティブなスキームの色 5 に復元する。`apply_osc(104, "5")` はインデックス 5 の `palette16` エントリのみを復元する。
- [ ] **TS9** (FR9、FR10): `apply_osc(104, ...)` は `palette16` のみが変化した場合（オーバーレイが既にクリア）に `true` を返し、新たにカバーされるケースで描画が dirty にマークされる。何も変化しない場合は引き続き `false` を返す。
- [ ] **TS10** (FR8、FR9): 既存テスト `apply_osc_110_resets_fg`、`apply_osc_111_resets_bg`、`apply_osc_104_empty_resets_all`、`apply_osc_104_indexed_resets_only_listed`、`apply_osc_104_no_change_returns_false` を削除ではなく新しいリセット意味論に更新する。

### Integration Tests

- [ ] **TS11** (FR6): チャンク中に生成された応答が `take_response` でドレインされ、`write_device_response` 経由で 1 回転送される。別々にパースされた 2 つのフレームの照会はそれぞれ自分の応答を得る。
- [ ] **TS12** (NFR3): `reset_frame_for_replay` がリプレイするスナップショットバイトに埋め込まれた色照会は PTY 書き込みを生じない — 既存の破棄回帰テストを DA1 / DSR だけでなく OSC 色照会ペイロードも対象にするよう拡張する。
- [ ] **TS13** (NFR2): OSC 色照会を運ぶ `PtyOutput` フレームが後続フレームとコアレスされない。

### E2E Tests

**Existing E2E tests**: None（E2E ハーネスが存在しない — `test/README.md:97-101`）

**Run command**: Not detected

- [ ] **TS14** (FR1、FR2、FR8、FR9): 手動検証。`.claude/rules/debugging-constraints.md` に従い `emterm.log` と画面観察で検証する。非デフォルトの `terminal_color_scheme` を設定した実際の eMterm セッションで上記の `printf` プローブを実行し、OSC 110 / 104 の描画訂正を目視確認する。

### Edge Cases

- [ ] 連鎖の途中の照会要素が次の要素を消費せず、連鎖のコード進行をずらさない（FR4）。
- [ ] 連鎖 OSC 10 のコードが 12 を超えた時点で停止する（FR1）。
- [ ] パレットインデックスが 10 進整数として解釈できない、または 256 以上の場合に応答も状態変化も生じない（FR2、FR10）。
- [ ] テーマが所有しない OSC コード中の `?` が応答バイトも状態変化も生じさせない（FR10）。
- [ ] アクティブなカラースキームが無い場合、リセットと照会が `Theme::default()` のシード値を対象とする（FR7）。
- [ ] BEL 終端の要求に BEL 終端の応答、ST 終端の要求に ST 終端の応答が返る（FR5）。

### Performance Tests

- [ ] 照会への応答がグリッドを dirty にせず、不要な `mark_all_dirty` を引き起こさない（FR1、FR10）。
- [ ] 単一スロット応答バッファが、連続する照会によって未配送の応答を失わない（NFR2、TS13）。

## Security Considerations

- **Authentication:** 該当なし。
- **Authorization:** 該当なし。
- **Input Validation:** パレットインデックスは 10 進整数として解釈でき、かつ 256 未満である必要がある。満たさないペアは応答も状態変化も生じない。`?` でもパース可能な色指定でもない spec 要素はスキップする（FR2、FR10）。
- **Data Protection:** 応答は `TerminalCore` の単一スロット応答バッファと `take_response` のみを経由し、新たな PTY 書き込み経路を導入しない（FR6）。
- **応答のリーク防止:** リプレイ経路とオフスレッドスワップ経路は保留中のデバイス応答を破棄し続ける（`src-tauri/src/tabs/replay.rs:96-105`、`:569`）。これにより、とうに終了したプログラムがスナップショットのバイト列に焼き込んだ色照会が稼働中シェルの標準入力に届くことはない（NFR3）。既存の回帰テストを変更なしで通す。
- **XSS Prevention:** 該当なし（子 WebView を経由しない）。
- **SQL Injection Prevention:** 該当なし（データベースを使用しない）。
- **CSRF Protection:** 該当なし。

## Error Handling

### Error Codes

エラーコード体系は存在しない。不正入力は無害化（no-op）で扱う。

| 条件 | 応答 | 状態変化 |
|------|------|----------|
| パレットインデックスが `>= 256` | なし | なし |
| パレットインデックスが解釈不能 | なし | なし |
| テーマが所有しない OSC コード中の `?` | なし | なし |
| 不正な連鎖ペア | なし | なし |
| `?` でもパース可能な色指定でもない spec 要素 | なし | スキップ（既存挙動） |
| オーバーレイも `palette16` も変化しない OSC 104 | なし | `changed` は false |

### Error Flow

```
不正入力 → 既存のスキップ判定 → 応答を生成しない → 状態を変えない → apply_osc は false を返す
```

## Performance Optimization

### Performance Goals

- 照会への応答がグリッドを dirty にしない（FR1）。
- 照会への応答が不要な `mark_all_dirty` を引き起こさない（FR10）。
- 単一スロット応答バッファにおいて、未配送の応答が後続の照会に上書きされない（NFR2）。

### Optimization Strategies

- 既存のコアレスゲート `Tab::pty_output_batch_eligible` → `payload_has_device_query` を使い、照会を含むフレームを単独でパースする。または、コアレス経路が OSC 色照会に対して安全であることを示す（NFR2）。
- 既存の `format_color_response` を再利用し、フォーマッタを増やさない（NFR1）。

### Caching Strategy

該当なし。照会はその時点のライブな `Theme` から解決する（FR3）。

## Success Criteria

- [ ] FR1-FR10 のすべての機能要件が実装され、テストされている。
- [ ] TS1-TS14 のすべてのテストシナリオが通る（TS14 は手動）。
- [ ] NFR1-NFR6 のすべての非機能要件が満たされている。
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と `crates/term_core` の同等コマンドが通る。
- [ ] `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る（NFR5）。
- [ ] 既存のリプレイ破棄回帰テストが変更なしで通り続ける（NFR3）。
- [ ] REQUIREMENTS.md 11.1 の受け入れ基準がすべて満たされている。
- [ ] コードレビューが完了している。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

`status: tbd` の要件はない。FR1-FR10 および NFR1-NFR6 はすべて `resolved`。

plan フェーズで確認する未検証の前提（要件としては解決済み）:

- A6: `crates/term_core` の OSC パーサーが到着した文字列終端子（BEL か ST か）を現在保持しているかは未検証。`handle_osc_internal` は `(param, data)` しか受け取らず、パーサーのソースは本ディスパッチの読み取り可能な入力集合の外にある。FR5 は要件を述べ、引き回しは planner に委ねる。
- A7: `payload_has_device_query`（`src-tauri/src/tabs/input.rs`）が OSC 色照会を既に認識しているかは未検証。当該ファイルは本ディスパッチの読み取り可能な入力集合の外にある。NFR2 は現状の可否を断定しない。

## Implementation Phases (if applicable)

具体的なタスク分割は create-plan で確定する。本書は要件の依存関係のみを示す。

- FR8 / FR9 のスキームミラー導入は FR7 の前提であり、FR7 は FR1 / FR2 / FR3 の照会値と一致する必要がある。
- FR5 の終端子引き回しは FR1 / FR2 の応答生成の前提である（A6 の確認を含む）。
- NFR2 の確認（A7）は FR6 の配送が正しく機能するための前提である。

## References

- 要件定義書: `feature-docs/osc-color-query-response/REQUIREMENTS.md`
- タスクトラッカー: [https://www.notion.so/3d93509ec8ee81dc8fa3c31cc698e212](https://www.notion.so/3d93509ec8ee81dc8fa3c31cc698e212)
- `src-tauri/src/render/theme.rs`: `Theme`、`apply_osc`、`apply_default_color_set`、`apply_palette_set`、`apply_palette_reset`、`apply_color_scheme`、`apply_user_scheme`
- `crates/term_core/src/color_spec.rs`: `format_color_response`（`:89-96`）
- `crates/term_core/src/osc_handler.rs`: `handle_osc_internal`（`:50`）
- `src-tauri/src/tabs/output_pipeline.rs`: `process_outer_via_core`（`:117-128`）、`apply_active_pane_output`（`:186-204`）、`pty_output_batch_eligible`（`:167-175`）
- `src-tauri/src/tabs/replay.rs`: 応答破棄（`:96-105`、`:569`）、`apply_queued_live_output`（`:822-835`）
- `.claude/rules/debugging-constraints.md`: 調査は `emterm.log` 経由で行う
- `.claude/rules/core-commands.md`: ビルド・テストコマンド
- `test/README.md`（`:97-101`）: E2E ハーネスが存在しないこと
