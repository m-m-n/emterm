# Feature: mux-osc-abort-altscreen-split-read

## 概要

mux の読み取りループ（`pty_reader_loop`）で、代替画面の切り替えシーケンスの途中状態を PTY の受信をまたいで引き継ぎ、`AgentStatusFeedScanner::feed` に正確なライブ区間を渡す。scrollback 書き込み経路は変更しない。要件は `feature-docs/mux-osc-abort-altscreen-split-read/REQUIREMENTS.md` を参照。

## 目的

- 代替画面の切り替えシーケンスが PTY の受信をまたいで分割されても、mux ペインは、同じバイト列を一括で処理した通常タブと同じ OSC 7501 の記録表で終わる。これは受信をどの位置で分割しても成り立つ。
- 再現手順の修正: root の working の記録を持つ mux ペインに、`drive_reads` で `\x1b[?1049h`、`\x1b[?1049`、`l\x1b]133;A`、`\x1b\x1b\\` の受信を与える。通常タブと同じく、記録表は空で終わる。

## ユーザーストーリー

### US1: 分割受信された画面切り替えの後でも通常タブと同じ記録表になる

mux ペインの利用者として、代替画面の切り替えシーケンスが受信をまたいで分割されても、mux ペインが通常タブと同じ OSC 7501 記録表と OSC 777 状態で終わってほしい。

**受け入れ基準:**
- [ ] AC1（FR1, FR2, FR6）: root の working で初期化した mux ペインに、`drive_reads` で `\x1b[?1049h`、`\x1b[?1049`、`l\x1b]133;A`、`\x1b\x1b\\` を与える。記録表は空で終わり、通常タブの一括処理の結果と一致する。
- [ ] AC2（FR1, FR2, FR6）: ストリーム `\x1b[?1049h` + `\x1b[?1049l` + `\x1b]133;A` + 終端 / 打ち切りの形のいずれか（BEL、`ESC \`、`ESC ESC \`、`ESC [0m`）。これを `\x1b[?1049l` の内部のすべての位置と、文字列の終わりと ESC の間で分割する。どの場合も、mux の記録表は通常タブの一括処理の記録表と一致する（root の working で初期化した場合は空）。1047 と 47 の形でも同じ。
- [ ] AC3（FR3, FR6）: ストリーム `\x1b[?1049h\x1b[?1049l\x1b]133;A\x07\x1b[?1049h` を、メイン画面への切り替えの内部で分割する。mux の記録表は通常タブと一致する（空）。
- [ ] AC4（FR3, FR6）: ストリーム `\x1b[?1049h\x1b]133;A\x07\x1b[?1049l` を、代替画面への切り替えの内部で分割する。root の working の初期状態は、mux と通常タブの両方で working のまま残る。
- [ ] AC5（FR4, FR6）: 指示子スロットの場合。メイン画面で始まる `\x1b(\x1b[?1049h\x1b]133;A\x07` と、代替画面で始まる `\x1b[?1049h\x1b(\x1b[?1049l\x1b]133;A\x07`。切り替えシーケンスの内部と、`\x1b(` と ESC の間で分割する。どちらの場合も、mux の記録表は通常タブと一致する。

### US2: 既存の挙動を保つ

mux ペインの利用者として、scrollback の書き込みとスキャナの既存の挙動は変わらないでほしい。

**受け入れ基準:**
- [ ] AC6（FR5）: 既存の書き込み経路テストが変更なしで通る（`fallback_path_keeps_the_whole_chunk_gate`、`round3_*` / `round4_*` / `escape_state_carry` テストを含む）。AC1 のストリームで、フォールバックの受信は従来どおり scrollback リングに何も書かない。
- [ ] AC7（FR8, NFR2）: src-tauri と term_core の `--lib` スイートがすべて通り、`cargo check --no-default-features` が成功する。

## 技術要件

### 機能要件

- **FR1:** 切り替えシーケンスの途中状態を受信をまたいで引き継ぐ（スキャナのライブ区間） — 代替画面の切り替えシーケンス（`ESC [?1049h/l`、`ESC [?1047h/l`、`ESC [?47h/l`。ALT_SCREEN_TOGGLES の形）は、受信の境界で分割されることがある。その場合、mux の読み取りループ（`pty_reader_loop`）は、前の受信の終わりの時点での切り替えシーケンスの途中状態を次の受信に引き継ぐ。これにより、受信のすべてのバイトについて、`AgentStatusFeedScanner::feed` に渡す live_spans が、同じバイト列を一括で抽出したライブ区間（その受信の範囲に限ったもの）と一致する。
- **FR2:** 現状の代替画面フォールバック受信のライブ区間 — 現状、先頭のバイトでメイン画面への切り替えを完了する受信は、代替画面フォールバック（`mod.rs:544`）に入り、空の live_spans を受け取る。FR1 により、切り替えの完了後のメイン画面のバイトを、代わりにライブとして渡す。その受信の中で終わる OSC 133 マーク（BEL 形式または ST 形式）は、通常タブと同じく項目を出す。その受信で始まり、次の受信で本文中の ESC（`ESC ESC \` / `ESC X`）により打ち切られるマークも同じく項目を出す。スキャナの cut_live / tail_live は、正確なライブ区間に従う。
- **FR3:** フォールバックでない受信でライブ区間を誤る場合 — FR1 は、フォールバックではないが、受信ごとの抽出でライブ区間を誤る受信も対象にする。(1) 例: `l\x1b]133;A\x07\x1b[?1049h`。分割された切り替えの完了後にメイン画面の範囲があり、同じ受信の中で代替画面に戻る。走査とシャドウパーサーが同じ状態で終わるため、この受信はフォールバックしない経路を通るが、ライブ区間が空になる。このメイン画面の範囲をライブとして渡す。(2) 例: `\x1b[?10` | `49h<OSC 133 を含む代替画面のバイト>\x1b[?1049l`。代替画面への切り替えが境界をまたいだ直後の、代替画面のバイトはライブとして渡さない。
- **FR4:** 文字集合指示子スロットとの整合 — 引き継ぐ途中状態は term_core の解釈に従う。切り替えシーケンスの ESC が `ESC (` / `ESC )` の直後の指示子スロットにある場合、受信をまたいで分割されていても切り替えとして扱わない（既存の DesignatorSlot の規則、mux-suppressed-output-round4 FR5 と整合）。途中状態の後に、切り替えを完了しないバイトが来た場合、そこまでのバイトは従来どおりに扱う。
- **FR5:** scrollback 書き込み経路は変えない — 次は変わらない。to_write / cuts の決定、フォールバック経路で何も書かないこと（`(&[], Vec::new(), vec![0])`）、ScrollbackWriteFilter、既存の書き込み経路テストの期待値。FR1 の正確なライブ区間は、スキャナの入力にだけ使う。
- **FR6:** 通常タブとの一致 — 対象の入力は、OSC 133 マークを切り替えシーケンスの前または後に置いたストリームで、切り替えシーケンスの内部の任意の位置、または文字列の終わりと本文中の ESC の間で分割したもの。この入力について、mux ペインの OSC 7501 記録表（実際の `pty_reader_loop` とデーモンの agent-status タスク）は、同じストリームを一括で処理した通常タブの記録表と一致する。同じ行で、OSC 777 agent-status の状態が一致することも確かめる。
- **FR7:** 回帰テスト — 次のテストを追加する。(a) すべての分割位置で、スキャナに渡す受信ごとの live_spans が、一括抽出の live_spans をその受信に限ったものと等しいことを示す単体テスト。(b) 実際の `pty_reader_loop` とデーモンタスクを通す mux / 通常タブ一致テスト。再現手順、BEL / ST / 打ち切りの形、FR3 の形、指示子スロットの場合を含める。
- **FR8:** 変わらない挙動 — 次は従来どおり。AgentStatusFeedScanner の打ち切り規則（mux-osc-double-esc-abort FR1〜FR6）、cut_live / tail_live の判定規則そのもの、BEL / ST による確定、受信長 / 4096 バイトの規則、8 KiB の carry 上限、`step_ris`、OSC 777 agent-status の正規綴りのみの取り込み。既存のすべてのテスト（SC-1 K1〜K15 と `dblesc_*` テストを含む）が変更なしで通る。term_core は変更しない。

### 非機能要件

- **NFR1 - 堅牢性:** どの入力でも panic しない。引き継ぐ状態は O(1) で、最大 8 バイトの切り替えシーケンスの途中状態のみ。
- **NFR2 - フィーチャーゲート:** 変更は `--no-default-features` でビルドされる mux のコードにとどまり、CLI のみのビルドもコンパイルできる。
- **NFR3 - ログ:** 新しいログ出力を追加しない。
- **NFR4 - プラットフォーム:** Linux と Windows。
- **NFR5 - term_core 不変:** term_core（`crates/term_core`）は基準の挙動であり、変更しない。

## 実装方針

### アーキテクチャ

**構成:**
```
通常タブ（基準の挙動。変更しない）
  PTY 出力 → term_core で一括処理 → OSC 7501 記録表・OSC 777 状態（基準）

mux ペイン（本タスクの変更対象）
  PTY の受信 → pty_reader_loop
       ├─ 受信ごとの切り替え検出（toggle_at / next_live_toggle / extract_main_buffer）
       │     ＋ 前の受信から引き継いだ切り替えシーケンスの途中状態（FR1、追加）
       │       → live_spans（一括抽出をその受信に限ったものと一致）
       │       → AgentStatusFeedScanner::feed → デーモンの agent-status タスク
       │       → OSC 7501 記録表・OSC 777 状態
       └─ to_write / cuts → ScrollbackWriteFilter → scrollback リング（変更しない、FR5）
```

**受信ごとの live_spans（FR1〜FR4）:**

| 受信の形 | 現状 | 変更後 |
|---|---|---|
| 先頭のバイトでメイン画面への切り替えを完了する（代替画面フォールバック、`mod.rs:544`） | 空 | 切り替え完了後のメイン画面のバイトをライブとして渡す（FR2） |
| 分割された切り替えの完了後にメイン画面の範囲があり、同じ受信で代替画面に戻る（例: `l\x1b]133;A\x07\x1b[?1049h`） | 空 | そのメイン画面の範囲をライブとして渡す（FR3(1)） |
| 代替画面への切り替えが境界をまたいだ直後（例: `\x1b[?10` \| `49h...`） | 代替画面のバイトの扱いを誤る | 代替画面のバイトはライブとして渡さない（FR3(2)） |
| 切り替えシーケンスの ESC が指示子スロット（`ESC (` / `ESC )` の直後）にある | — | 受信をまたいでも切り替えとして扱わない（FR4） |
| 途中状態の後に切り替えを完了しないバイトが来る | — | そこまでのバイトは従来どおり（FR4） |

### データフロー

```
受信 N の末尾: 切り替えシーケンスの途中状態（最大 8 バイト分、O(1)、NFR1）
        ↓ 引き継ぎ（FR1）
受信 N+1: 正確な live_spans → AgentStatusFeedScanner::feed（cut_live / tail_live の規則は不変、FR8）
```

### API 設計

該当なし。

### データベーススキーマ

該当なし。

### 依存関係

**内部依存:**
- term_core: 基準の挙動。変更しない（A2、NFR5）。代替画面上の OSC 133 を抑止し、打ち切られた文字列を、打ち切ったバイトが効く前に Unterminated としてディスパッチする（`osc.rs:67-70`）
- `write_filter.rs`: `:1197` toggle_at、`:1214` next_live_toggle、`:1332` extract_main_buffer（受信ごとの切り替え検出、途中状態の引き継ぎ無し）、`:1579` cut_live、`:1538-1543` tail_live（A7）
- `mod.rs:544`: 空の live_spans を渡す代替画面フォールバック（A7）
- DesignatorSlot の規則（mux-suppressed-output-round4 FR5）（FR4）
- AgentStatusFeedScanner の打ち切り規則（mux-osc-double-esc-abort FR1〜FR6）（FR8）

**外部依存:**
- 該当なし

### ファイル構成

```
write_filter.rs                    # 受信ごとの切り替え検出と live_spans の計算（A7）
mod.rs                             # pty_reader_loop の代替画面フォールバック（:544）
pty_spawn のテストモジュール       # TS1
daemon/tests.rs                    # SC-1 コーパス（K16 以降を追加）、TS2
tabs/tests/output_pipeline.rs      # seeded_plain_tab / process_combined、TS3
```

## 宣言された変更集合

このセクションは手書きの一覧を置かず、create-plan での導出方法を示す。フィーチャー固有のパスは、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、フィーチャー固有のパスに加えて、ワークフローが生成する次の 2 つをデフォルトで宣言する。

- `feature-docs/mux-osc-abort-altscreen-split-read/**`
- `test-docs/mux-osc-abort-altscreen-split-read/**`

`feature-docs/mux-osc-abort-altscreen-split-read/**` には `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物が含まれる。生成と管理は各フェーズドキュメントと `references/phase-state.md` が担う。このセクションは引用のみで、それらのルールは再掲しない。

`test-docs/mux-osc-abort-altscreen-split-read/**` には、タスクごとのテスト記録 `test-docs/mux-osc-abort-altscreen-split-read/{T}.tests.yaml` が含まれる。生成と管理は `implement-phase.md` が担う。このセクションは引用のみで、そのルールは再掲しない。

この 2 つのデフォルトは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載が無いことを除外とはみなさない。除外は意図的な絞り込みとして明示する。

この宣言はスーパーセットの主張であり、検証時に観測される実際の変更集合は、宣言された集合と等しい必要はなく、宣言に含まれる（CONTAINED IN）必要がある。implement タスクを 1 つも生成しないフィーチャーは `test-docs/mux-osc-abort-altscreen-split-read/` ディレクトリを生成しないが、その場合も宣言された `test-docs/mux-osc-abort-altscreen-split-read/**` は正しい。宣言されたパスが生成されなくても違反にはならない。

## テストシナリオ

### 単体テスト

- [ ] TS1（FR1, FR3, FR4）pty_spawn のテストモジュール: ストリームのコーパス（AC2〜AC5 のストリームと CUT_FORMS のすべての形）を、すべての位置で分割して順に受信させる。各受信が出すライブ区間が、`spans_within(一括抽出の区間, 受信の範囲)` と等しいことを確かめる。1047 / 47 の形と、途中状態が始まったが切り替えにならない分割も確かめる。

### 結合テスト

- [ ] TS2（FR6, FR7, AC1〜AC5）mux 側: `daemon/tests.rs` の SC-1 コーパスに行を追加し（K16 以降）、各行を、その行が指定するすべての配送で `drive_reads`（ChunkedReader）に流し、記録表と OSC 777 状態を比べる。
- [ ] TS3（FR6, AC1〜AC5）通常タブ側: 同じストリームを `seeded_plain_tab` / `process_combined`（`tabs/tests/output_pipeline.rs`）に一括で流し、基準の記録表と OSC 777 状態を得る。
- [ ] TS4（FR5, AC6）: AC1 のストリームで、scrollback リングの内容が従来の挙動と同じ（フォールバックは何も書かない）。
- [ ] TS5（FR8, AC7）: src-tauri と term_core の `--lib` スイート全体と、`--no-default-features` のチェックを実行する。

### E2E テスト

**既存の E2E テスト**: なし
**実行コマンド**: 検出されず
- [ ] 既存の E2E テストが退行なく通る（既存の E2E テストは無い）

### エッジケース

- [ ] 切り替えシーケンスの内部のすべての位置での分割（受信の末尾が単独の ESC になる場合を含む）
- [ ] `?1047` と `?1049` の接頭辞の曖昧さ（`\x1b[?104` | `9l` / `7l`）
- [ ] 途中状態の後に、切り替えを完了しないバイトが来る場合（`\x1b[?10` | `4x`）
- [ ] ESC が指示子スロットにある切り替えシーケンス（受信をまたいで引き継がれるスロットを含む）
- [ ] 1 回の受信で、メイン画面への切り替えを完了してから再び代替画面に入る場合
- [ ] マークの BEL / ST / `ESC ESC \` / `ESC X` の形で、同じ受信または次の受信で終わる場合

### パフォーマンステスト

該当なし（引き継ぐ状態は O(1)、NFR1）。

## セキュリティ上の考慮

- **認証:** 該当なし
- **認可:** 該当なし
- **入力検証:** どの入力でも panic しない。引き継ぐ状態は最大 8 バイトの切り替えシーケンスの途中状態のみ（NFR1）
- **データ保護:** 該当なし
- **XSS 対策:** 該当なし
- **SQL インジェクション対策:** 該当なし
- **CSRF 対策:** 該当なし

## エラー処理

### エラーコード

該当なし。

### エラーフロー

新しいログ出力を追加しない（NFR3）。

## パフォーマンス

### パフォーマンス目標

- 引き継ぐ状態は O(1)（NFR1）

### 最適化方針

該当なし。

### キャッシュ方針

該当なし。

## 成功基準

- [ ] すべての機能要件が実装され、テストされている
- [ ] すべてのテストシナリオが通る
- [ ] AC1〜AC7 を満たす
- [ ] ドキュメントがそろっている
- [ ] コードレビューが完了している

## 未解決事項

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## 前提

- A1: 範囲は exact_live_spans_feed_only（create-spec-q0001 scope.fallback-coverage、batch-codex-consultation）。切り替えシーケンスの途中状態を受信をまたいで引き継ぎ、スキャナに正確なライブ区間を渡す。scrollback 書き込み経路とそのテストの期待値は変えない。
- A2: 基準は通常タブの一括処理（term_core、変更しない）。term_core は代替画面上の OSC 133 を抑止し、打ち切られた文字列を、打ち切ったバイトが効く前に Unterminated としてディスパッチする（`osc.rs:67-70`）。
- A3: 対象外: ALT_SCREEN_TOGGLES に無い代替画面の切り替えの形（`ESC [?1;1049h` のような複合パラメータなど）。従来のフォールバック（空のライブ区間）のまま。
- A4: 対象外: 分割された切り替えに対する scrollback 書き込み経路の不正確さ（例: FR3(2) の代替画面への切り替えの分割で、代替画面のバイトがリングに書かれる）。書き込み経路は変えない（FR5）。
- A5: スキャナの cut_live / tail_live の判定規則は変えない。修正はその上流のライブ区間の計算で行い、不明なライブ状態を先へ引き継ぐ方式（タスクが示した最初の修正案）は取らない。
- A6: 同じ Osc133Mark 項目が OSC 777 の D→A ラッチも動かすため、一致テストの行で OSC 777 の状態も確かめる。
- A7: タスクのコード位置はコードで確認済み。`write_filter.rs:1579`（cut_live）、`:1538-1543`（tail_live）、`:1197` toggle_at、`:1214` next_live_toggle / `:1332` extract_main_buffer（受信ごとの切り替え検出、途中状態の引き継ぎ無し）、`mod.rs:544`（空の live_spans を渡す代替画面フォールバック）。

## 参考資料

- 要件定義書: `feature-docs/mux-osc-abort-altscreen-split-read/REQUIREMENTS.md`
- mux-osc-double-esc-abort SPEC: `feature-docs/mux-osc-double-esc-abort/SPEC.md`（FR1〜FR6）
- mux-suppressed-output-round4: FR5（DesignatorSlot の規則）
