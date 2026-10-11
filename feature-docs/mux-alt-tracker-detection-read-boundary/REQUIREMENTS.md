---
title: "mux-alt-tracker-detection-read-boundary"
created_date: 2026-10-11
status: draft
---

# mux-alt-tracker-detection-read-boundary - 要件定義書

## 1. 概要

### 1.1 背景

mux-alt-tracker-ris-compound-decset の review round 1 で、次の 2 つの指摘が medium として残った。

- 2139988314e6d6ac: 同じ読み取りの 2 つ目以降の複合形の取りこぼし
- 89b935844bd8bc3b: 検出時の引き継ぎ状態の消去

### 1.2 目的

- mux ペインの AltSpanTracker が、食い違い状態で複合 DECSET を検出した読み取りでも、読み込み境界の位置によらず、同じバイト列を一括で処理した通常タブと同じ OSC 7501 記録表と OSC 777 状態を返す（前身 SPEC FR6）。
- 指摘 2139988314e6d6ac と 89b935844bd8bc3b を解消する。

### 1.3 スコープ

**対象:**
- AltSpanTracker の走査で、複合形を検出した読み取りの扱い（FR1〜FR3）
- 通常タブとの一致（FR4）
- 回帰テストの追加（FR5）
- 既存テスト 1 本の名前と期待値の更新、および前身フィーチャーのテスト記録の更新（FR6）
- 前身 SPEC FR4/FR5 の上書きの宣言と、write_filter.rs の doc コメントの更新（FR7）

**対象外:**
- FR8 に挙げる、変わらない挙動
- CSI 解析の C0（NUL・CAN・SUB）と `:` の扱い（同じレビューの low の指摘）（A6）

## 2. ビジネス要件

### 2.1 ビジネス目標

- mux ペインの AltSpanTracker が、食い違い状態で複合 DECSET を検出した読み取りでも、読み込み境界の位置によらず、同じバイト列を一括で処理した通常タブと同じ OSC 7501 記録表と OSC 777 状態を返す（前身 SPEC FR6）。
- mux-alt-tracker-ris-compound-decset の review round 1 で medium として残った指摘 2139988314e6d6ac（同じ読み取りの 2 つ目以降の複合形の取りこぼし）と 89b935844bd8bc3b（検出時の引き継ぎ状態の消去）を解消する。

### 2.2 対象ユーザー

該当なし。変更対象は mux の読み込みループ内部の状態機械（AltSpanTracker）だけで、UI と画面は変わらない。

### 2.3 期待される効果

- 複合形を検出した読み取りでも、mux ペインの OSC 7501 記録表と OSC 777 状態が、読み込み境界の位置によらず通常タブと一致する。

## 3. ユースケース

該当なし。UI と画面の変更が無い。

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 |
|----|--------|------|
| FR1 | 検出した読み取りの、後続の複合形 | 検出した後は、同じ読み取りの以降の複合形を、食い違いの有無によらず検出する |
| FR2 | 検出した読み取りの引き継ぎ状態 | 最後に検出した複合形より後で記録した引き継ぎ状態を、次の読み取りに引き継ぐ |
| FR3 | 検出した読み取りの、ほかの扱い | フォールバック範囲、shadow_alt、adoption の回数、照合を行わないことは変えない |
| FR4 | 通常タブとの一致 | mux ペインの記録表と OSC 777 状態が、すべての配送で通常タブと一致する |
| FR5 | 回帰テスト | トラッカーの単体テスト、デーモン側の行、通常タブ側の行を追加する |
| FR6 | 既存テストの名前と期待値の更新 | 既存テスト 1 本の名前と期待値を更新し、前身のテスト記録を更新する |
| FR7 | 前身 SPEC FR4/FR5 の上書き | このフィーチャーの SPEC で上書きを宣言し、doc コメントを揃える |
| FR8 | 変わらない挙動 | scrollback の書き込み経路などを変えない |

### 4.2 機能詳細

#### FR1: 検出した読み取りの、後続の複合形

**説明**: 1 回の読み取りの中で複合形（ライブな ESC で始まる DEC private set / reset のうち、引数列に 47 / 1047 / 1049 を含み、ALT_SCREEN_TOGGLES のどの形にも当たらないもの）を検出した後は、同じ読み取りの以降の複合形を、client_alt と shadow_alt の食い違いの有無によらず検出し、client 側の画面を終端の向き（h は代替画面、l はメイン画面）にする。

**ビジネスルール**:
- 読み取りの中でまだ検出していないとき、一致状態で受けた複合形の扱い（前身 A2）は変えない。

#### FR2: 検出した読み取りの引き継ぎ状態

**説明**: 検出した読み取りでは、走査が最後に検出した複合形より後で記録した引き継ぎ状態を消さず、次の読み取りに引き継ぐ。

対象の引き継ぎ状態:
- 切り替えの途中状態
- 指示子待ち
- CSI の解析状態

**例**:
- `ESC [?1;1047h ESC (` で終わる読み取りの後、次の読み取りの `ESC c` は指示子スロットの ESC c として扱う。
- `ESC [?1;1049l ESC [?10` | `47h` では ?1047h を適用する。

#### FR3: 検出した読み取りの、ほかの扱い

**説明**: 検出した読み取りでは、次の扱いを変えない。

- 読み取りの終わりに client 側の画面で判定する A3 のフォールバック範囲（メイン画面なら読み取り全体、代替画面なら空）を返すこと。
- shadow_alt に読み取り後のパーサー状態を採ること。
- adoption を 1 回数えること。
- 読み取り後の照合を行わないこと。

#### FR4: 通常タブとの一致

**説明**: 再現手順 1 と 2 のストリーム、および FR1 と FR2 の形を含むストリームについて、mux ペイン（実際の pty_reader_loop とデーモンの agent-status タスク）の OSC 7501 記録表と OSC 777 状態が、同じストリームを一括で処理した通常タブと一致する。この一致は、各行が指定するすべての配送（1 か所で分割するすべての位置と、1 バイトずつの読み込みを含む）で成り立つ。

#### FR5: 回帰テスト

**説明**: 次の 3 つを追加する。

1. トラッカーの単体テスト（altscreen_split_read.rs）: M1 / M2 の後に複合形を 2 つ続ける行と、複合形の後で読み取りを `ESC (`、`ESC`、`ESC [?10` で分割する行を、すべての分割位置と 1 バイトずつの読み込みで流し、期待するライブ区間になることを確かめる。
2. デーモン側の行（daemon/tests.rs、drive_reads、seed S-W）を、K58 以降の別の一覧として追加する。
3. 2 に対応する通常タブ側の行（tabs/tests/output_pipeline.rs、process_combined）を、K58 以降の別の一覧として追加する。

**ビジネスルール**:
- 2 と 3 には、それぞれの一覧のすべての行を流すテストを付ける。
- K38〜K57 の一覧と、それを固定するテストは変えない。

#### FR6: 既存テストの名前と期待値の更新

**説明**: altmismatch_ac4_a_detection_read_applies_later_switches_and_clears_the_carry を、新しい挙動を表す名前（例: altmismatch_ac4_a_detection_read_applies_later_switches_and_keeps_the_later_carry）に変える。期待値は FR2 に合わせて更新する。

**期待値の更新**:
- 3 つ目のケースでは、2 回目の読み取りの後の partial_len を 5、3 回目の読み取り `49l`+mark の範囲を [3..11] にする。
- 4 つ目のケースでは partial_len を 1 にする。

**テスト記録の更新**:
- .claude/rules/test-docs-records.md に従い、test-docs/mux-alt-tracker-ris-compound-decset/task0001.tests.yaml の AC-4 の tests にあるこのテスト名を、新しい名前に更新する。
- AC-4 のブロックに、このフィーチャーの SPEC.md と FR2 / FR6 を挙げる Superseded の YAML コメントを付ける。
- red_reason は変えない。

#### FR7: 前身 SPEC FR4/FR5 の上書き

**説明**: このフィーチャーの SPEC で、次の上書きを宣言する。検出した読み取りでは、前身 SPEC（feature-docs/mux-alt-tracker-ris-compound-decset/SPEC.md）の FR4/FR5 と引き継ぎ状態表にある「adopt のときに引き継ぎ状態を消す」を、複合形より前の状態に限る。

**ビジネスルール**:
- 前身の SPEC.md と REQUIREMENTS.md は書かれたまま残す。
- write_filter.rs の AltSpanTracker の doc コメント（1615-1616 行、1674-1677 行）と scan() の doc コメントは、FR1 と FR2 の挙動に揃える。

#### FR8: 変わらない挙動

**説明**: 次は変えない。

- scrollback の書き込み経路（extract_main_buffer、to_write、cuts、ScrollbackWriteFilter）
- AgentStatusFeedScanner（step_ris、cut_live / tail_live を含む）
- term_core
- ShadowParser
- AltSpanTracker::advance のシグネチャと mod.rs の呼び出し側
- 一致状態で受けた未対応の形のフォールバック（AC-6a）
- 検出しなかった読み取りの扱い

FR6 の 1 本を除く既存テストは、すべて変更なしで通る。

## 5. 非機能要件

| ID | 項目 | 内容 |
|----|------|------|
| NFR1 | 堅牢性 | どの入力でも panic しない。引き継ぐ状態は O(1) のまま（切り替えの途中状態は最大 7 バイトで、それに CSI の解析状態を加える）とする。 |
| NFR2 | フィーチャーゲート | 変更は、--no-default-features でビルドされる mux のコードの中に収める。CLI のみのビルドもコンパイルできる。 |
| NFR3 | ログ | 新しいログ出力を追加しない。 |
| NFR4 | プラットフォーム | Linux と Windows。 |
| NFR5 | term_core 不変 | term_core（crates/term_core）は基準となる挙動として扱い、変更しない。 |

### 5.1 パフォーマンス要件

- 引き継ぐ状態は O(1) のまま（切り替えの途中状態は最大 7 バイトで、それに CSI の解析状態を加える）とする（NFR1）。

### 5.2 セキュリティ要件

- 入力検証: どの入力でも panic しない（NFR1）。

### 5.3 可用性要件

該当なし。

### 5.4 保守性要件

- ログ出力: 新しいログ出力を追加しない（NFR3）。

### 5.5 互換性要件

- フィーチャーゲート: --no-default-features でビルドされる mux のコードの中に変更を収め、CLI のみのビルドもコンパイルできる（NFR2）。
- プラットフォーム: Linux と Windows（NFR4）。
- term_core: 基準となる挙動として扱い、変更しない（NFR5）。

## 6. UI/UX要件

該当なし。UI と画面の変更が無い（デザインステップはスキップする。A7）。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- term_core（crates/term_core）は変更しない（NFR5）。
- 変更は、--no-default-features でビルドされる mux のコードの中に収める（NFR2）。
- AltSpanTracker::advance のシグネチャと mod.rs の呼び出し側は変えない（FR8）。

### 9.2 ビジネス上の制約

- 前身の SPEC.md と REQUIREMENTS.md は書かれたまま残す（FR7）。
- K38〜K57 の一覧と、それを固定するテストは変えない（FR5）。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-alt-tracker-detection-read-boundary/**`
- `test-docs/mux-alt-tracker-detection-read-boundary/**`

`feature-docs/mux-alt-tracker-detection-read-boundary/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-alt-tracker-detection-read-boundary/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-alt-tracker-detection-read-boundary/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

該当なし。

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1（FR1、FR4）: tracker の Rig で read(`ESC[?1047h`) の後に read(`ESC[?1;1049l ESC[?1;1049h ESC]133;A BEL`) を呼ぶと、空の範囲が返る。続く read(mark()) も空になる。逆向き（M2 の後に h 形、l 形の順の複合形と OSC 133;A）では、読み取り全体の範囲が返り、続く mark() はライブになる。
- [ ] AC2（FR2、FR4）: read(`ESC[?1047h ESC[?1;1047h ESC(`) の後の read(`ESC c ESC]133;A BEL`) は空の範囲を返す。分割せずに送った場合と同じ結果になる。M1 の後の `ESC[?1;1049l ESC[?10` | `47h` では ?1047h を適用し、client 側は代替画面になる。
- [ ] AC3（FR4、FR5）: K58 以降の行をデーモン側の drive_reads（seed S-W）に流す。対象は、M1 + l 形 + h 形、M2 + h 形 + l 形、複合形の後に ESC ( / ESC / ESC [?10 が続いて読み取りが切れるもの。各行が指定するすべての配送（1 か所で分割するすべての位置と、1 バイトずつの読み込み）で、OSC 7501 記録表と OSC 777 状態が、同じ行を通常タブで一括処理した結果と一致する。
- [ ] AC4（FR6）: 名前を変えた既存テストが新しい期待値で通る。task0001.tests.yaml の AC-4 は新しい名前を挙げていて、その名前は `cargo test --lib -- --list` の出力にテストとして現れる。Superseded のコメントが付き、red_reason は変わっていない。
- [ ] AC5（FR8、NFR2）: FR6 の 1 本を除く既存テスト（altsplit_*、SC-1 K1〜K57、altmismatch_*、dblesc_* など）が変更なしで通る。src-tauri と term_core の --lib スイートが通り、cargo check --no-default-features が成功する。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系（TS1、FR1〜FR3、AC1、AC2）: altscreen_split_read.rs で、M1 / M2 の後に複合形を 2 つ続けるストリームと、複合形の後で読み取りが ESC ( / ESC / ESC [?10 / ESC [?1;10 / ESC ( ESC [?10 で切れるストリームを、すべての分割位置と 1 バイトずつの読み込みで流す。各読み取りのライブ区間が期待値と一致することを確かめる。オラクルには画面の上書き点を 2 か所以上渡せるようにする（client_walk を拡張するか、別の関数を足す）。adoption の回数は配送ごとに期待値を定める。
- [ ] 正常系（TS2、FR6、AC4）: 名前を変えた既存テストの 4 つのケース（C1l E(1049)、C1h L(1049)、C1l ESC[?10 | 49l、C1l ESC）が、新しい期待値で通る。
- [ ] 正常系（TS3、FR4、FR5、AC3）: daemon/tests.rs に K58 以降の行を別の一覧として追加し、各行をその行が指定するすべての配送で drive_reads（seed S-W）に流す。記録表と OSC 777 状態が期待値と一致する。
- [ ] 正常系（TS4、FR4、FR5）: tabs/tests/output_pipeline.rs で、同じ行を別の一覧として seeded plain tab / process_combined で一括処理し、基準となる記録表と OSC 777 状態が得られることを確かめる。
- [ ] 回帰（TS5、FR8、NFR2、AC5）: src-tauri と term_core の --lib スイート全体と、--no-default-features でのチェックを実行する。
- [ ] 境界値: 12.2 のエッジケースを参照。

### 12.2 エッジケース

- [ ] 最初の複合形で 2 つの画面が一致する並び: M1 + l 形（C1l、C7l）+ h 形（C1h、Crh、C7h、C0h、Ceh）と、M2 + h 形 + l 形。
- [ ] 1 回の読み取りに複合形が 3 つ以上交互に続く並び。
- [ ] 複合形の後に、完全な切り替えパターン（E(1049) / L(1049)）が同じ読み取りで続く並び（挙動は変わらない）。
- [ ] 検出した読み取りが、指示子待ち、単独の ESC、切り替えの途中状態、CSI の解析途中、指示子スロットの途中状態のどれかで終わる場合。
- [ ] 検出した読み取りが単独の ESC で終わり、次の読み取りが c で始まる場合は RIS として扱う（前身 FR2）。
- [ ] 複合形が 2 つ続くストリームでは、adoption の回数が配送によって変わる（同じ読み取りなら 1 回、2 つの複合形の間で分割すれば 2 回）。
- [ ] 読み取りの最初の複合形を一致状態で受けた場合は、従来どおり検出しない（前身 A2）。
- [ ] 同じ読み取りで、複合形より前にあるマークは正確に扱えない（前身 A3、許容済み）。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 複合形 | ライブな ESC で始まる DEC private set / reset のうち、引数列に 47 / 1047 / 1049 を含み、ALT_SCREEN_TOGGLES のどの形にも当たらないもの |
| 検出した読み取り | 1 回の読み取りの中で複合形を検出した読み取り |
| 引き継ぎ状態 | 次の読み取りに引き継ぐ状態。切り替えの途中状態、指示子待ち、CSI の解析状態 |
| A3 のフォールバック範囲 | 読み取りの終わりに client 側の画面で判定する範囲。メイン画面なら読み取り全体、代替画面なら空 |
| 前身 SPEC | feature-docs/mux-alt-tracker-ris-compound-decset/SPEC.md |

## 14. 確認事項

### 14.1 確認済み事項

- [x] 検出した読み取りの終わりに残る carry: 必ず最後に検出した複合形より後で記録されたものである。scan() は持ち越した carry を冒頭で消費し、carry を書き込むのはループを抜ける break の箇所だけである（write_filter.rs:1738, 1763, 1778, 1846, 1872 のコード構造）（A1）。
- [x] 一致状態で受けた最初の複合形: 検出しない（前身 A2 を維持する）。既存テスト altmismatch_ac4_a_compound_in_a_matched_state_keeps_todays_behavior が固定している（A2）。
- [x] requirement.predecessor-spec-carry-wording: supersede_in_new_spec。前身 SPEC の FR4/FR5 と引き継ぎ状態表は、このフィーチャーの SPEC で上書きを宣言する。前身の SPEC.md と REQUIREMENTS.md は書かれたまま残す（A3）。
- [x] testing.regression-row-list: separate_list。回帰行は K58 以降の別の一覧にする。K38〜K57 の一覧と、それを固定するテストは変えない（A4）。
- [x] testing.existing-test-rename: rename_and_update_records。既存テスト altmismatch_ac4_a_detection_read_applies_later_switches_and_clears_the_carry は名前を変える。task0001.tests.yaml AC-4 の記録は新しい名前に更新し、Superseded のコメントを付ける。red_reason は変えない（A5）。
- [x] CSI 解析の C0（NUL・CAN・SUB）と `:` の扱い（同じレビューの low の指摘）: 対象外とする（A6）。
- [x] design-step.recommendation: skip。デザインステップはスキップする（A7）。

### 14.2 未確認・保留事項

なし。

## 15. 参考資料

- 前身 SPEC: feature-docs/mux-alt-tracker-ris-compound-decset/SPEC.md
- テスト記録のルール: .claude/rules/test-docs-records.md
- review round 1 の指摘: 2139988314e6d6ac、89b935844bd8bc3b
