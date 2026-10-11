# Feature: mux-osc-double-esc-abort

## 概要

mux デーモンの `AgentStatusFeedScanner` で、OSC 本文の途中の ESC の後（`InsideOscPendingSt`）に `\` 以外のバイトが来たとき、組み立て中の OSC 文字列を Unterminated として終え、そのバイトを ESC の直後のバイトとして処理する。打ち切られた OSC 7501 は破棄し、OSC 777 agent-status と OSC 133 は term_core と同じく適用する。要件は `feature-docs/mux-osc-double-esc-abort/REQUIREMENTS.md` を参照。

## 目的

- 同じバイト列に対し、`\` が続かない ESC で OSC 文字列が打ち切られたとき、通常タブと mux ペインが同じ状態で終わる。比べる状態は OSC 7501 の記録表、OSC 777 のエージェント状態、OSC 133 D→A の推定クリア（osc7501-leading-zero-length FR4）。
- 再現手順の修正: `ESC]7501;state=working BEL` の後に `ESC]07501;state=clear ESC ESC \` を入力しても、通常タブと同じく mux ペインでも working の記録が残る。

## ユーザーストーリー

### US1: 打ち切られた OSC 7501 報告で記録が変わらない

mux ペインの利用者として、連続 ESC で打ち切られた OSC 7501 報告が mux ペインでも記録を変えないようにしたい。通常タブと同じ記録表を得るため。

**受け入れ基準:**
- [ ] AC1（FR1, FR2, FR7, 再現手順）: `ESC]7501;state=working BEL` の後、`ESC]07501;state=clear ESC ESC \` を入力しても、通常タブと mux ペイン（実際の `pty_reader_loop` とデーモンタスク）の両方で root=working が残る。mux 側では、打ち切られた報告に対する broadcast も revision の変化も無い。
- [ ] AC2（FR2, FR7）: 正規の綴り `ESC]7501;state=clear ESC ESC \` でも AC1 が成り立つ。
- [ ] AC3（FR6, FR7）: ストリームを 2 つの ESC の間で分割しても、AC1 と AC2 で両経路の記録表が同じになる。スキャナ単体では、任意の 1 か所での分割と 1 バイトずつの feed が同じ項目を出す。
- [ ] AC4（FR1, FR5）: `ESC]7501;state=error ESC ESC ]7501;state=clear BEL` は、両経路で clear を適用する。

### US2: 打ち切られた OSC 777 / OSC 133 で通常タブと同じ状態になる

mux ペインの利用者として、打ち切られた OSC 777 agent-status と OSC 133 を通常タブと同じく扱ってほしい。通常タブと同じエージェント状態と推定クリアの結果を得るため。

**受け入れ基準:**
- [ ] AC5（FR3, FR7）: `ESC]777;emterm;agent-status;v=1;state=working` を `ESC ESC \` で打ち切った場合と `ESC X`（X は `\` 以外）で打ち切った場合、スキャナは Report を 1 つ出す。mux ペインの OSC 777 エージェント状態は通常タブと同じ（working）になる。
- [ ] AC6（FR4, FR7）: `ESC]133;D` に続く `ESC]133;A` を、それぞれ `ESC ESC \` または `ESC X` で打ち切ると、メイン画面で `Osc133Mark` 項目を出す。OSC 777 の D→A 推定クリアと 7501 のプロンプト開始（working・blocked・idle は削除、done・error は残す）は両経路で同じ結果になる。同じように打ち切った `ESC]0133;A` は `NonCanonicalPromptStart` だけを出す。
- [ ] AC7（FR4）: 代替画面で打ち切られた OSC 133 は項目を出さない。`ESC]133;A ESC [?1049h` はメイン画面のマークを出す。どちらの結果も、受信をどこで分割しても成り立つ。
- [ ] AC8（FR5）: `ESC]777;emterm;agent-status;v=1;state=working ESC c` は Report、Reset の順に出す。`ESC]133;A ESC ]7501;state=error BEL` はマーク、7501 の項目の順に出す。
- [ ] AC9（FR8）: 既存の program_status_feed、デーモン leading_zero_length、通常タブ一致、RIS 一致のテストがすべて変更なしで通る。
- [ ] AC10（NFR2）: `cargo check --no-default-features` が成功する。

## 技術要件

### 機能要件

- **FR1:** mux 取り込みの打ち切り規則 — mux デーモンの `AgentStatusFeedScanner` で、OSC 本文の途中の ESC の後（`InsideOscPendingSt`）、`\` は従来どおり ST として文字列を完了する。それ以外のバイトは、OSC の種類によらず、組み立て中の OSC 文字列を Unterminated として終える。そのバイトは続けて ESC の直後のバイトとして処理する。ESC は次のエスケープバイトを待ち（`SeenEsc`）、`]` は空の本文で新しい OSC を開始し、`c` は従来どおり RIS の項目を出し、その他のバイトは idle に戻る。
- **FR2:** 打ち切られた OSC 7501 文字列の破棄 — 共通の認識規則で OSC 7501 にあたる文字列（正規の `7501;`、先頭ゼロ付きの `07501;`、その他共通規則が認める綴り）が FR1 で終えられた場合、項目を出さない。記録を変更せず、broadcast も revision の変化も起こさず、照会への応答も返さない。通常タブ（`callbacks.rs:1459-1463`）と一致する。
- **FR3:** 打ち切られた OSC 777 agent-status 文字列の適用 — 本文が正規の接頭辞 `777;emterm;agent-status;` で始まる OSC 文字列が FR1 で終えられた場合、BEL または ST で終わったときと同じ Report 項目を出す。agent-status の取り込みは正規の綴りのみのまま（osc7501-program-status FR18）。それ以外の OSC 777 本文は従来どおり項目を出さない。
- **FR4:** 打ち切られた OSC 133 文字列の適用（ライブ画面判定付き） — FR1 で終えられた OSC 133 文字列は、BEL または ST と同じく `osc133_item` で分類する。正規のマークは `Osc133Mark` を出し、OSC 777 の D→A ラッチと OSC 7501 のプロンプト開始に渡る。共通規則で認めるプロンプト開始は `NonCanonicalPromptStart` を出し、7501 のプロンプト開始だけを適用する。その他の種類は何も出さない。項目は、文字列がライブのメイン画面上で終わったときだけ出す。ライブかどうかは、文字列自身のバイトが終わった時点で載っていた画面で決め、打ち切ったバイトの位置では決めない。例として `ESC]133;A ESC [?1049h` では、文字列を打ち切る ESC が代替画面への切り替えも始めるが、マークはメイン画面のものとして扱う。
- **FR5:** 項目の順序 — 打ち切られた文字列の項目は、打ち切ったバイトとそれ以降のバイトが出すすべての項目より前に出る。`ESC c` による Reset と、`ESC ]` で開いた OSC の項目もこれに含まれる。項目は term_core がディスパッチする順序に従う。
- **FR6:** 受信境界からの独立 — ストリームを任意の 1 か所で分割した場合（2 つの ESC の間、ESC と打ち切るバイトの間を含む）、および 1 バイトずつ受信した場合も、出る項目は同じになる。打ち切りに必要な状態（FR4 のライブ判定など）は feed 呼び出しをまたいで引き継ぐ。
- **FR7:** 通常タブと mux の一致 — 同じ入力に対し、通常タブと mux ペインは同じ OSC 7501 の記録表と同じ OSC 777 のエージェント状態で終わる。正規の OSC 133 D→A の組については、推定クリアの結果も同じになる。対象は次の入力。`ESC ESC \` で打ち切られた OSC 7501 報告（`7501;` と `07501;` の両綴り、一括受信と 2 つの ESC の間での分割受信）。`ESC ESC`、および ESC の後に別のバイトが来て打ち切られた OSC 777 agent-status 報告。`ESC ESC`、および ESC の後に別のバイトが来て打ち切られた OSC 133 マーク（メイン画面と代替画面）。
- **FR8:** 変わらない挙動 — 次は従来どおり。BEL と `ESC \` による確定、受信長 / 4096 バイトの規則、8 KiB の carry 上限、終端された OSC 133 マークのライブ区間判定、RIS の追跡（`step_ris`）、OSC 777 agent-status の正規綴りのみの取り込み、既存のすべてのテスト。term_core は変更しない。
- **FR9:** 回帰テスト — 回帰を検出するテストを追加する。FR1〜FR6 のスキャナ単体テストと、実際の `pty_reader_loop` とデーモンタスクを通す FR7 の通常タブ / mux ペイン一致テスト。一致テストには、`ESC ESC \` で打ち切られた `7501;` と `07501;` の報告、および 2 つの ESC の間で分割された受信を含める。

### 非機能要件

- **NFR1 - 堅牢性:** どの入力でも panic しない。carry は `AGENT_STATUS_FEED_SCANNER_CARRY_OVER_CAP`（8 KiB）以内に収まる。破棄した本文はクリアする（`retained_len` 0）。追加する状態は O(1)。
- **NFR2 - フィーチャーゲート:** 変更は `--no-default-features` でビルドされる mux のコードにとどまり、CLI のみのビルドもコンパイルできる。
- **NFR3 - ログ:** 打ち切られた入力や却下された入力は、従来どおりログに出さない。
- **NFR4 - プラットフォーム:** Linux と Windows。
- **NFR5 - term_core 不変:** term_core は変更せず、7501 を埋め込まない。

## 実装方針

### アーキテクチャ

**構成:**
```
通常タブ（基準の挙動。変更しない）
  PTY 出力 → term_core パーサー（osc_escape, crates/term_core/src/parser/osc.rs）
           → OSC ハンドラ（osc_handler.rs）→ callbacks.rs
              ・OSC 777 / OSC 133: Unterminated でもコールバックを呼ぶ
              ・OSC 7501: Unterminated は捨てる（callbacks.rs:1459）

mux ペイン（本タスクの変更対象）
  PTY 出力 → pty_reader_loop → AgentStatusFeedScanner::step（デーモンタスク）
           → 項目（OSC 7501 項目 / Report / Osc133Mark / NonCanonicalPromptStart / Reset）
           → OSC 7501 記録表・OSC 777 エージェント状態・broadcast
```

**`InsideOscPendingSt` の遷移（FR1）:**
```
InsideOscPendingSt
  ├─ '\'        → ST で文字列を完了（従来どおり）
  └─ それ以外   → 文字列を Unterminated で終える（FR2 / FR3 / FR4 で種類ごとに処理）
                   続けてそのバイトを ESC 直後のバイトとして処理:
                     ESC   → SeenEsc
                     ']'   → 空の本文で新しい OSC を開始
                     'c'   → RIS の項目（従来どおり）
                     その他 → idle
```

**Unterminated で終えた文字列の扱い:**

| OSC の種類 | 出す項目 | 要件 |
|---|---|---|
| OSC 7501（`7501;` / `07501;` / 共通規則の綴り） | なし（記録・broadcast・revision・照会応答に影響しない） | FR2 |
| OSC 777、本文が `777;emterm;agent-status;` で始まる | BEL / ST と同じ Report | FR3 |
| OSC 777、上記以外 | なし | FR3 |
| OSC 133 の正規マーク | `Osc133Mark`（ライブのメイン画面で終わったときのみ） | FR4 |
| OSC 133 の共通規則のプロンプト開始 | `NonCanonicalPromptStart`（ライブのメイン画面で終わったときのみ） | FR4 |
| OSC 133 のその他 | なし | FR4 |

### データフロー

```
バイト列 → [AgentStatusFeedScanner] → 打ち切られた文字列の項目 → 打ち切ったバイト以降の項目（FR5）
                 ↑
   feed 呼び出しをまたいで状態（ライブ判定を含む）を引き継ぐ（FR6）
```

### API 設計

該当なし。

### データベーススキーマ

該当なし。

### 依存関係

**内部依存:**
- term_core（`osc_escape`、`crates/term_core/src/parser/osc.rs:63`）: 基準の挙動。変更しない（A2、NFR5）
- `extract_main_buffer`（`write_filter.rs:1347-1369`）: OSC 133 のライブ区間の構築（A6）
- `osc133_item`: OSC 133 の分類（FR4）
- `step_ris`: RIS の追跡（FR8）

**外部依存:**
- 該当なし

### ファイル構成

```
src-tauri/src/mux/ipc/pty_spawn/
├── write_filter.rs   # AgentStatusFeedScanner（InsideOscPendingSt の分岐: :1556、:1610）
└── tests.rs          # program_status_feed、デーモン leading_zero_length、ScriptedReader（:1152）
crates/term_core/src/parser/osc.rs   # 参照のみ（変更しない）
output_pipeline.rs                   # 通常タブ側テストの seeded_plain_tab / process_combined
```

## 宣言された変更集合

このセクションは手書きの一覧を置かず、create-plan での導出方法を示す。フィーチャー固有のパスは、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、フィーチャー固有のパスに加えて、ワークフローが生成する次の 2 つをデフォルトで宣言する。

- `feature-docs/mux-osc-double-esc-abort/**`
- `test-docs/mux-osc-double-esc-abort/**`

`feature-docs/mux-osc-double-esc-abort/**` には `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物が含まれる。生成と管理は各フェーズドキュメントと `references/phase-state.md` が担う。このセクションは引用のみで、それらのルールは再掲しない。

`test-docs/mux-osc-double-esc-abort/**` には、タスクごとのテスト記録 `test-docs/mux-osc-double-esc-abort/{T}.tests.yaml` が含まれる。生成と管理は `implement-phase.md` が担う。このセクションは引用のみで、そのルールは再掲しない。

この 2 つのデフォルトは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載が無いことを除外とはみなさない。除外は意図的な絞り込みとして明示する。

この宣言はスーパーセットの主張であり、検証時に観測される実際の変更集合は、宣言された集合と等しい必要はなく、宣言に含まれる（CONTAINED IN）必要がある。implement タスクを 1 つも生成しないフィーチャーは `test-docs/mux-osc-double-esc-abort/` ディレクトリを生成しないが、その場合も宣言された `test-docs/mux-osc-double-esc-abort/**` は正しい。宣言されたパスが生成されなくても違反にはならない。

## テストシナリオ

### 単体テスト

- [ ] TS1（FR1〜FR6）`src-tauri/src/mux/ipc/pty_spawn/tests.rs` の program_status_feed: 7501・777・133 について `ESC ESC`、`ESC X`、`ESC ]`、`ESC c` による打ち切りの遷移、項目の順序、`ESC [?1049h` による打ち切りを含むライブ区間判定、すべての位置での分割と 1 バイトずつの feed、打ち切り後の `retained_len` を確かめる。

### 結合テスト

- [ ] TS2（FR7, AC1〜AC3）mux: デーモン leading_zero_length の `drive()` と sc4 コーパスに、`ESC ESC \` で打ち切られた `7501;` と `07501;` の行を追加する。2 つの ESC の間での分割には、分割して返す reader を使う（`pty_spawn/tests.rs:1152` に `ScriptedReader` がある）。
- [ ] TS3（FR7, AC1〜AC3）通常タブ: 同じストリームを `seeded_plain_tab` / `process_combined`（`output_pipeline.rs`）に、一括で、および 2 つの ESC の間で分けた 2 回の `process_combined` 呼び出しで流し、記録表を TS2 の結果と比べる。
- [ ] TS4（FR3, FR4, FR7, AC5〜AC7）OSC 777 agent-status と OSC 133 の打ち切られた文字列の一致: mux ペインの osc777 状態と記録を、通常タブの agent-status イベント、ラッチ結果、7501 の記録表と比べる。
- [ ] TS5（FR8, AC9, AC10）: src-tauri と term_core の `--lib` スイート全体と、`--no-default-features` のチェックを実行する。

### E2E テスト

**既存の E2E テスト**: なし
**実行コマンド**: 検出されず
- [ ] 既存の E2E テストが退行なく通る（既存の E2E テストは無い）

### エッジケース

- [ ] 2 つの ESC の間、ESC と打ち切るバイトの間での受信分割、1 バイトずつの受信（FR6, AC3, AC7）
- [ ] 打ち切る ESC が代替画面への切り替え `ESC [?1049h` を始める場合、マークはメイン画面のものとして扱う（FR4, AC7）
- [ ] 打ち切る ESC が `ESC c`（Reset）や `ESC ]`（新しい OSC）を始める場合、打ち切られた文字列の項目が先に出る（FR5, AC4, AC8）
- [ ] 先頭ゼロ付きの綴り `07501;` / `0133;`（FR2, FR4, AC1, AC6）

### パフォーマンステスト

該当なし（追加する状態は O(1)、NFR1）。

## セキュリティ上の考慮

- **認証:** 該当なし
- **認可:** 該当なし
- **入力検証:** どの入力でも panic しない。carry は `AGENT_STATUS_FEED_SCANNER_CARRY_OVER_CAP`（8 KiB）以内。破棄した本文はクリアする（NFR1）
- **データ保護:** 該当なし
- **XSS 対策:** 該当なし
- **SQL インジェクション対策:** 該当なし
- **CSRF 対策:** 該当なし

## エラー処理

### エラーコード

該当なし。

### エラーフロー

打ち切られた入力や却下された入力は、従来どおりログに出さない（NFR3）。

## パフォーマンス

### パフォーマンス目標

- 追加する状態は O(1)（NFR1）

### 最適化方針

該当なし。

### キャッシュ方針

該当なし。

## 成功基準

- [ ] すべての機能要件が実装され、テストされている
- [ ] すべてのテストシナリオが通る
- [ ] AC1〜AC10 を満たす
- [ ] ドキュメントがそろっている
- [ ] コードレビューが完了している

## 未解決事項

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## 前提

- A1: タスク記述の末尾（integration ブランチを残したままの push と gh pr create、Codex への相談と Notion への議論の記録）は実行指示であり、要件ではない。
- A2: term_core（通常タブ）を基準の挙動とし、変更しない。
- A3: スキャナと term_core の既存の 2 つの差異は対象外とする。スキャナは文字集合指示子スロットを追跡しない（`ac1_existing_osc_recognition_is_unchanged_after_a_designator_escape` で固定済み）。また DCS/APC 本文も追跡しない（tmux パススルー形式 `ESC P tmux; ESC ESC ] ... ESC ESC \ ESC \` を含む）。
- A4: 履歴除去（`osc_body_identity` / `client_parity_scan`）と scrollback 書き込みフィルタは変更しない。`write_filter.rs` の `scan_string` は、ESC の後に `\` 以外のバイトが来たとき既に Aborted を返す（`write_filter.rs:1103-1109`）。
- A5: 打ち切りの範囲は all_and_align_unterminated。term_core は ESC の後に `\` 以外のバイトが来ると Unterminated の文字列をディスパッチし（`osc.rs:68`）、その文字列でも OSC 777 と OSC 133 のコールバックを呼ぶ（`osc_handler.rs:202`、`:275`、`callbacks.rs:1375`）。Unterminated を捨てるのは OSC 7501 だけ（`callbacks.rs:1459`）。mux も同じく、打ち切られた 7501 は捨て、777 / 133 は適用する。
- A6: 打ち切られた OSC 133 のライブ判定は、文字列のバイトが終わった時点で載っていた画面で行う。term_core は打ち切ったバイトが効く前にディスパッチする。打ち切る ESC が代替画面への切り替えを始める場合、`extract_main_buffer` はその ESC をライブ区間から外す（`write_filter.rs:1347-1369`）。

## 参考資料

- 要件定義書: `feature-docs/mux-osc-double-esc-abort/REQUIREMENTS.md`
- osc7501-leading-zero-length SPEC: `feature-docs/osc7501-leading-zero-length/SPEC.md`（FR4）
- osc7501-program-status SPEC: FR18
