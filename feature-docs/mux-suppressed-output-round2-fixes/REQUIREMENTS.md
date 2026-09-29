---
title: "mux-suppressed-output-round2-fixes"
created_date: 2026-09-29
status: draft
---

# mux-suppressed-output-round2-fixes - 要件定義書

## 1. 概要

### 1.1 背景

mux-suppressed-output-fixes（PR #109）の review round 2 で、medium 指摘 8 件が `unresolved` のまま残っている。

| stable_id | 指摘 |
|-----------|------|
| ecc48041b65a5380 | 保持窓の再開位置に、文字集合指定の指定文字として消費される ESC を選び、存在しない問い合わせを作る（TM-1） |
| dd56f3984c74cde1 | 主画面の色問い合わせを置換出力から外すが、snapshot 再生では応答が捨てられるので一度も応答されない |
| ae48e7cd98084c19 | 書き込みフィルタが、読み取りをまたぐ文字集合指定の待ち状態を持ち越さない（TM-1） |
| b600645f1fa94686 | 画面切り替えで打ち切られた OSC を pending から再送し、元のストリームに無い問い合わせを作る（TM-1） |
| f8b600bcc0ed55da | pending による除外を元チャンクの区間ではなく 1 つの切り位置で行うため、間にある代替画面区間の問い合わせを落とす |
| 03ccd5c7702db8db | 保持窓（256 バイト）より長い主画面のビューア起動が抑止チャンクで完結すると、snapshot にも置換出力にも入らない |
| a93dffe30438a693 | ビューア起動の判定（OSC 番号を数値として解釈）と scrollback の除去判定（文字列一致）が食い違い、先頭ゼロのビューア起動が二重に届く |
| 3eccc254dd278b33 | リングが一周していない主画面の snapshot に残る未完了の尾部を、置換出力でもう一度送る |

発生条件は、snapshot 取得（タブ切替 / reattach）と 1 チャンクが重なり、そのチャンクが抑止されたときに限られる。

### 1.2 目的

- 抑止したチャンクの副作用処理（置換出力の組み立てと書き込みフィルタの判定）を、抑止しなかった場合のクライアントの解析結果と同じ解析状態・同じ順序にする。
- mux-suppressed-output-fixes（PR #109）の review round 2 で unresolved の medium 指摘 8 件それぞれについて、対応済みか対応不要かを判断し、根拠と回帰テストとの対応を記録する。

### 1.3 スコープ

**対象**:
- 機能要件 FR1〜FR9（4 章）と非機能要件 NFR1〜NFR6（5 章）

**対象外**:
- daemon が付け足すバイトと尾部の干渉（as-03）
- term_core の OSC 番号累積で起きる u16 のあふれ（as-06）
- round2.yaml の dropped_at_gate の low 2 件（as-10）
- 抑止したチャンクのインライン画像（Kitty APC・SIXEL DCS）と OSC 777 emterm の image の配送（既知の欠落のまま）
- 起動引数の追加指示（integration ブランチの push と PR 作成、不明点の Codex 相談と Notion への記録）。ワークフローの運用として扱う（as-11）

## 2. ビジネス要件

### 2.1 ビジネス目標

- 抑止したチャンクの副作用処理を、抑止しなかった場合と同じ解析状態・同じ順序で行う。
- review round 2 の medium 指摘 8 件の判断を記録する。

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| mux の利用者 | mux の pane に出力を流しながら、タブ切替や reattach を行う |

### 2.3 期待される効果

- 抑止したチャンクに含まれる問い合わせへの応答、ビューア起動、表示が、抑止しなかった場合と一致する。
- 8 件の指摘それぞれについて、判定・根拠・回帰テストとの対応を追える。

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター |
|----|----------------|----------|
| UC01 | 出力中の pane で snapshot 取得が走る | mux の利用者 |

### 3.2 ユースケース詳細

#### UC01: 出力中の pane で snapshot 取得が走る

**アクター**: mux の利用者

**事前条件**:
- mux の pane に出力が流れ続けている

**基本フロー**:
1. 1 章の条件を含むチャンクの途中で、snapshot 取得（タブ切替 / reattach）が走る
2. そのチャンクが抑止される
3. snapshot の後に置換出力が届き、その後に後続チャンクが届く

**事後条件**:
- 問い合わせ応答・ビューア起動・表示が、抑止しなかった場合と一致する

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 対応する stable_id |
|----|--------|--------------------|
| FR1 | 保持窓の再開位置から、指定文字の枠に入りうる ESC を除く（TM-1） | ecc48041b65a5380 |
| FR2 | 主画面の色問い合わせも置換出力に含める | dd56f3984c74cde1 |
| FR3 | 書き込みフィルタが、読み取りをまたぐ指定文字待ちの状態を持ち越す（TM-1） | ae48e7cd98084c19 |
| FR4 | 書き込みフィルタが、除いた画面切り替え列による打ち切りを反映する（TM-1） | b600645f1fa94686 |
| FR5 | pending による走査の除外を元チャンク上の区間に限る | f8b600bcc0ed55da |
| FR6 | 書き込みフィルタで完結した持ち越し列を置換出力に渡す | 03ccd5c7702db8db |
| FR7 | OSC 番号の復元とビューア起動の識別を共通の層で行う | a93dffe30438a693 |
| FR8 | snapshot がすでに運んだ未完了の尾部を置換出力で再送しない | 3eccc254dd278b33 |
| FR9 | 判断の記録 | — |

すべての要件の状態は resolved（未確定の要件は無い）。

### 4.2 機能詳細

#### FR1: 保持窓の再開位置から、指定文字の枠に入りうる ESC を除く（TM-1）

**説明**: 保持窓（256 バイト）が満杯のときの再開位置の選択（derive_prefix_start）は、指定文字として消費されうる位置の ESC を選ばない。

**ビジネスルール**:
- offset i の ESC は、i == 0 のとき、または window[i-1] が `(` / `)` で、かつ i == 1 か window[i-2] が ESC のときに判定不能とする。
- `ESC ( ESC ( …` の連鎖も同じ規則で順に除く。
- 判定可能な ESC が無いときは既存のフォールバック（as-05）に従う。
- 除外による結果は見逃しにとどまり、問い合わせやビューア起動を作り出さない。

**テストシナリオ**: TS-1

#### FR2: 主画面の色問い合わせも置換出力に含める

**説明**: 抑止したチャンクで完結した OSC 色問い合わせは、CSI の問い合わせと同じく、リングに書く区間（主画面）かどうかを問わず置換出力に含め、snapshot の後に 1 回届ける。

**ビジネスルール**:
- リングに書く区間との重なりによる除外（assemble_items の overlaps_ranges 判定）は無くす。

**テストシナリオ**: TS-2

#### FR3: 書き込みフィルタが、読み取りをまたぐ指定文字待ちの状態を持ち越す（TM-1）

**説明**: ScrollbackWriteFilter は、feed に渡したバイト列が `ESC (` / `ESC )` で終わったとき、指定文字を待っている状態を pending とは別に保持する。

**ビジネスルール**:
- 次の feed の先頭 1 バイトは、ESC であっても無条件に指定文字として消費する。
- 任意の位置で分割した入力の排出内容と pending は、同じ入力を一括で渡したときと一致する。
- pending（OSC・DCS・APC と末尾の単独 ESC）の保留規則と上限 512 KiB は変えない。

**テストシナリオ**: TS-3

#### FR4: 書き込みフィルタが、除いた画面切り替え列による打ち切りを反映する（TM-1）

**説明**: 書き込みフィルタ（または pending を決める処理）は、主画面区間の抽出で除いた画面切り替え列（47/1047/1049 の h/l）の位置で、進行中の OSC・DCS・APC と末尾の単独 ESC を、クライアントと同じく閉じたものとして扱う。

**ビジネスルール**:
- 切り替え列をはさむ入力でも、pending の事後条件（閉じた列と、その後ろのバイトを保持しない）が成り立つ。
- 打ち切られた列は置換出力の尾部として再送しない。

**テストシナリオ**: TS-4

#### FR5: pending による走査の除外を元チャンク上の区間に限る

**説明**: pending を理由に置換出力の走査から外す範囲は、pending に対応する元チャンク上の区間だけにする。

**ビジネスルール**:
- 1 つの切り位置から先をまとめて外す方式（tail_exclusion_start と scan の limit_in_chunk）はやめる。
- 途中にある代替画面区間の問い合わせ・ビューア起動は走査して置換出力に含める。
- FR4 と合わせて、除外範囲は最後の主画面区間に収まる。

**テストシナリオ**: TS-5

#### FR6: 書き込みフィルタで完結した持ち越し列を置換出力に渡す

**説明**: 前の読み取りで始まって pending に持ち越され、抑止したチャンクで完結したビューア起動と色問い合わせは、保持窓の長さに関係なく、置換出力に含めて snapshot の後に 1 回届ける。

**ビジネスルール**:
1. 書き込みフィルタから完結列を渡すときは、元チャンク上の位置も渡す。書き込みフィルタは主画面区間を連結して処理するため、位置は元チャンクの座標に戻して渡す。
2. 重複排除は、保持窓による同じ列の再検出（置換出力の走査が同じ列を見つけた場合）だけを除く。同じ内容の別々の起動は消さない。
3. 代替画面区間の問い合わせ・起動列との順序は、元のストリームの順を保つ。
4. 配送する種類の選択は FR7 の共通の識別を使う。

**テストシナリオ**: TS-6

#### FR7: OSC 番号の復元とビューア起動の識別を共通の層で行う

**説明**: OSC 番号の復元（最初の `;` までの数字を 10 進で累積し、最初の `;` より前の数字以外はデータに残す）と、ビューア起動の識別を、mux::ipc に依存しない共通の層で行う。

**ビジネスルール**:
- scrollback の除去（リングへの書き込みと snapshot の組み立て）と置換出力の抽出は、この層の同じ復元・識別を共有する。
- 除去側の判定は、文字列一致から数値による識別に揃える。
- 識別結果と、利用側ごとの除去対象・配送対象の選択は分ける。
    - 除去側: REPLAYABLE_VIEWER_KINDS の種類・agent-status・OSC 9999 emterm-md を除き、fold などその他の種類と emterm-mux は残す。
    - 配送側: ビューア起動のうち image を除く。
- 番号の累積が u16 を超える OSC は識別しない（as-06）。

**テストシナリオ**: TS-7

#### FR8: snapshot がすでに運んだ未完了の尾部を置換出力で再送しない

**説明**: snapshot のバイト列は変えずに満たす。

**ビジネスルール**:
1. 対象は、実際に生成された snapshot がクライアントのパーサーを未完了の尾部の状態（UTF-8 の途中、`ESC (` / `ESC )` の指定文字待ち、未完了 CSI）のまま残す場合とする。リングが一周していても、画面ダンプが空、またはダンプブロックの生成に失敗して後置ブロックが付かない場合を含む（as-02）。
2. snapshot の形と末尾の解析状態は、現在のペイン状態から推測しない。送信先と抑止境界に対応する snapshot の情報として境界の記録に保持し、置換出力の組み立てに渡す。境界を記録するすべての snapshot 経路（可視化復帰・reattach・オンデマンド）が、生成した snapshot の情報を記録する。
3. 置換出力に問い合わせ・起動列が無いときは、クライアントがすでに置かれている尾部を再送しない。
4. 置換出力に問い合わせ・起動列があるとき（併存）は、先頭の問い合わせ・起動列の ESC が指定文字として消費される、CSI を打ち切る、UTF-8 の途中を捨てることを考慮し、問い合わせの配送と、後続チャンクの解析状態の両方を生のストリームと一致させる。
5. daemon が付け足すバイトと尾部の干渉は対象外（as-03）。

**テストシナリオ**: TS-8

#### FR9: 判断の記録

**説明**: feature-docs/mux-suppressed-output-round2-fixes/ に stable_id ごとの判断表を置く。

**ビジネスルール**:
- 各行には判定（対応済み／対応不要）・根拠・対応する回帰テストを書く。
- 8 件をすべて 1 行ずつ載せる。

| stable_id | 対応する要件 |
|-----------|--------------|
| ecc48041b65a5380 | FR1 |
| dd56f3984c74cde1 | FR2 |
| ae48e7cd98084c19 | FR3 |
| b600645f1fa94686 | FR4 |
| f8b600bcc0ed55da | FR5 |
| 03ccd5c7702db8db | FR6 |
| a93dffe30438a693 | FR7 |
| 3eccc254dd278b33 | FR8 |

- feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml は変更しない。

## 5. 非機能要件

### 5.1 パフォーマンス要件

- **NFR3（reader 通常経路の負荷）**: reader の通常経路に新たな走査を足さない。FR3 の指定文字待ちの状態と、FR6 の持ち越し列の完結位置は、書き込みフィルタが既に行う走査の中で得る。識別、問い合わせとビューア起動の抜き出し、尾部の組み立ては、抑止したチャンクのときだけ行う。

### 5.2 セキュリティ要件

- **NFR4（TM-1・TM-3）**: クライアントのパーサーが制御シーケンスを始めない位置から、問い合わせやビューア起動を作り出さない。置換出力は、その snapshot を受け取った送信先にだけ送る。FR8 の snapshot の情報は、その送信先と抑止境界に対応する記録から取り、現在の output_target を読み直して決めない。
- **NFR5（TM-2）**: 置換出力の走査・保持窓の再開位置の選択・書き込みフィルタの境界走査・snapshot 末尾の解析状態の判定は、有界な 1 パスで行い、パニックしない。空の置換出力を空の PtyOutput チャンクにしない。pending の上限 512 KiB と、上限を超えたときの strip を通した吐き出しは変えない。

### 5.3 可用性要件

- **NFR2（ロック規律）**: ロックの順序は output_target → 捕捉の排他 → リング・shadow parser とする。output_target を保持したまま blocking_send しない。境界の記録に足す snapshot の情報（FR8）も同じ規律で扱い、snapshot の組み立てと末尾の解析状態の判定は捕捉の排他の外で行う。

### 5.4 保守性要件

- **NFR6（ビルドとプラットフォーム）**: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。FR7 の共通の層は gui feature と mux::ipc に依存しない。

### 5.5 互換性要件

- **NFR1（ワイヤ形式と snapshot のバイト列）**: mux_ipc のワイヤ形式と、Snapshot / SnapshotRestore のフレームの形は変えない。可視化復帰・reattach・オンデマンドの snapshot のバイト列の組み立て（snapshot_bytes.rs の各 layout）は変えない。置換出力は既存の PtyOutput チャンクとして送る。
    - 例外は FR7 による除去対象の変化だけとし、これを明記する。対象は、先頭ゼロ付きの番号（例: `0777;emterm;markdown;…`、`09999;emterm-md`）と、共通の復元が最初の `;` より前の数字以外をデータに残すことで識別されるようになる列（例: `777emterm;;markdown;…`）。これらはリングと snapshot から除かれるようになる。
- **NFR6（プラットフォーム）**: Linux と Windows で同じように動く。

## 6. UI/UX要件

該当なし。UI の変更が無い。変わるのは mux デーモン内部のバイト列処理とテストだけ（デザインステップは skipped）。

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約

- 『クライアントと同じ解析規則』の基準は、term_core のパーサーの遷移と、GUI の theme（src-tauri/src/render/theme.rs）の応答生成とする。前フィーチャー SPEC の FR2 の遷移規則表 (a)〜(h) を引き継ぐ（as-01）。
- 前フィーチャーの不変条件を守る（as-04）。対象は、ロック順序、output_target を保持中に blocking_send しないこと、mux_ipc のワイヤ形式、pending の上限 512 KiB、保持窓の上限 N = 256 バイト、置換出力の配送順序（snapshot → 問い合わせとビューア起動を元の順序で → 尾部 → 後続チャンク）、未完了 CSI の尾部から C0 を除くこと。
- `ESC (` / `ESC )` の直後に切り替え列が来る形（クライアントは切り替え列の ESC を指定文字として消費する）での、主画面区間の抽出（extract_main_buffer_bytes）と shadow parser の扱いは変えない（as-08）。
- AgentStatusFeedScanner と PassthroughScanner の OSC の検出は変えない。FR7 の共通の層を使うのは、scrollback の除去と置換出力の抽出だけ（as-09）。

### 9.2 ビジネス上の制約

- feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml は変更しない。

### 9.3 スケジュール制約

該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-suppressed-output-round2-fixes/**`
- `test-docs/mux-suppressed-output-round2-fixes/**`

`feature-docs/mux-suppressed-output-round2-fixes/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-suppressed-output-round2-fixes/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-suppressed-output-round2-fixes/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

| 課題 | 影響度 | 対応 |
|------|--------|------|
| 保持窓に判定可能な ESC が無いときは通常状態から走査し、リングに書かない区間で N を超える文字列の途中から続く場合は既知の欠落とする（前フィーチャー as-05）。FR1 で除外する位置が増えても、結果は見逃し側に倒れる | 中 | リングに書く区間の長い列は FR6 で扱う（as-05） |
| daemon が付け足すバイトと尾部の干渉。対象は、reattach・オンデマンドの末尾の切り替え列、可視化復帰の代替画面の ESC[?1049h、ダンプブロック等 | 中 | このフィーチャーでは直さない。これらの形では、置換出力は従来どおり尾部を再送する（as-03） |
| term_core の OSC 番号累積で起きる u16 のあふれ（osc.rs:22） | 低 | このフィーチャーでは直さない。共通の識別は、あふれた番号を識別しない（as-06） |
| term_core は、UTF-8 の途中で ESC を受けると置換文字を出さずに途中のバイトを捨てる（根拠は既存テスト visible_reattach_redelivers_a_cut_utf8_tail_with_no_replacement_character_in_any_row。snapshot 末尾の ESC[?1049l がこの形になる） | 中 | create-plan で term_core の実装を確認する（as-07） |

### 10.2 ビジネスリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC-1: 8 件の指摘それぞれに、修正前のコードで失敗し、修正後に通る再発検出テストがある。
- [ ] AC-2: snapshot（実際の組み立て関数の出力を reset_and_replay_segments で適用）・置換出力・後続チャンクを term_core に流した結果が、生のストリームを 1 回流した参照と一致する。比べるのは、画面・カーソル・応答バイト列・表示された文字。対象は FR3・FR4・FR5・FR6・FR8 の各ケースで、分割位置を変えて確かめる。続きのバイトや置換文字は表示されない。
- [ ] AC-3: 抑止したチャンクの問い合わせ（CSI・色問い合わせ。主画面の色問い合わせを含む）は、snapshot の後・次のチャンクより前に、元の順序でちょうど 1 回ずつ届く。次は届かない。すでにクライアントに届いた問い合わせ、クライアントのパーサーが始めない位置にある問い合わせ形のバイト（保持窓の指定文字の枠の ESC から始まるもの、読み取りをまたぐ指定文字待ちの後のもの）、打ち切られた OSC（除いた切り替え列による打ち切りを含む）。
- [ ] AC-4: 抑止したチャンクで完結したビューア起動は、snapshot と置換出力を合わせてちょうど 1 回届く。保持窓より前に始まった起動、先頭ゼロ付きの番号の起動を含む。同じ内容の別々の起動はそれぞれ 1 回届く。インライン画像と OSC 777 emterm の image は届かない。
- [ ] AC-5: 書き込みフィルタの排出内容と pending は、指定文字の直前で分割した終端の無い入力を含め、分割位置によらず一括入力のときと一致する。切り替え列をはさむ入力でも pending の事後条件が成り立つ。
- [ ] AC-6: 可視化復帰・reattach・オンデマンドの snapshot のバイト列は、NFR1 の例外（FR7 の除去対象の変化）を除き、変更前と同じになる。mux_ipc のワイヤ形式は変わらない。
- [ ] AC-7: 8 件の stable_id それぞれについて、判定・根拠・回帰テストとの対応が feature-docs/mux-suppressed-output-round2-fixes/ の判断表に記録されている。feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml は変更されていない。
- [ ] AC-8: 意図して挙動を変えるテスト以外の既存テストが、変更なしで通る。挙動を変えるテストは一覧化されている（見込み: suppressed_output.rs の osc_color_query_inside_ring_written_ranges_is_not_redelivered、pty_spawn/tests.rs の visible_reattach_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring と on_demand_snapshot_redelivers_an_alt_screen_color_query_but_not_one_that_reached_the_ring）。関数の引数・戻り値の変更に伴う呼び出しの書き換えは、挙動の変更に含めない。
- [ ] AC-9: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。

### 11.2 KPI

該当なし。

## 12. テストシナリオ

### 12.1 テスト観点

| ID | 要件 | シナリオ |
|----|------|----------|
| TS-1 | FR1 | 満杯の保持窓 `ESC ( ESC ]10;` と空白 249 個の後にチャンク `?` BEL、`ESC ( ESC [` と NUL 252 個の後にチャンク `c`、`ESC ( ESC ( …` の連鎖の各組を抑止する。置換出力と応答が空になる。判定可能な ESC（指定文字の枠に無いもの）が窓にあるときは、従来どおりそこから走査して問い合わせを 1 回届ける。 |
| TS-2 | FR2 | 主画面で `ESC ]11;?` BEL を含むチャンクを抑止する。snapshot を reset_and_replay_segments と take_response の破棄で適用し、その後に置換出力を流す。応答の数が 1 になる。reader 経由（visible reattach・オンデマンド・可視化復帰）でも主画面の色問い合わせが 1 回届く。 |
| TS-3 | FR3 | 終端文字の無い入力（例: `ESC ( ESC ]11;?tail`）を `ESC (` の直後で分割して 2 回の feed に渡す。排出内容と pending が一括入力のときと一致する。分割入力 → 抑止 → 再送 → BEL の流れで、色問い合わせへの応答が生じない。 |
| TS-4 | FR4 | 主画面で `ESC ]11;?`、`ESC [?47h`、`ESC [?47l`、空白 1 個を出力するチャンクを抑止し、置換出力の後に BEL を流す。応答が生じない。feed 後の pending に打ち切られた OSC が残らない。1047・1049 の形でも同じ。 |
| TS-5 | FR5 | 主画面で `ESC ]2;x`、`ESC [?1049h`、`ESC [6n`、`ESC [?1049l`、`y` の順に受けたチャンクを抑止する。代替画面区間の CSI 6n が 1 回届く。最後の主画面区間に本当に未完了の列が残る形では、その列だけが尾部になり、前の代替画面区間の問い合わせ・ビューア起動も届く。 |
| TS-6 | FR6 | 256 バイトを超える OSC 777 emterm markdown を読み取りの間で分割し、完結する側のチャンクを抑止する。snapshot の後に 1 回届く。開始が保持窓の中にある短い起動を分割した場合も 1 回（重複排除）。同じ内容の起動が 2 つある場合は 2 回。代替画面区間の問い合わせ・起動列と元の順序で並ぶ。持ち越した色問い合わせが抑止チャンクで完結する場合も 1 回応答される。 |
| TS-7 | FR7 | `0777;emterm;markdown;…` と `09999;emterm-md;…` を主画面の抑止チャンクに入れる。snapshot には入らず、置換出力で合計 1 回届く。代替画面区間でも 1 回。`777emterm;;markdown;…` はリングから除かれ、ビューア起動として識別される。`0777;emterm;image;…` はリングから除かれ、届かない。agent-status は除かれ、fold と emterm-mux は残る。番号が u16 を超える OSC は識別されない。 |
| TS-8 | FR8 | 主画面ペインの可視化復帰（リング未周回、およびリング周回済みで画面ダンプが空の場合）で、UTF-8 の途中・`ESC (` の直後・未完了 CSI で分割し、前半のチャンクを抑止する。クライアントの表示・カーソル・後続チャンクの解析が参照と一致し、置換文字や指定文字の `(` が表示されない。併存: 問い合わせの後に同じ尾部で終わるチャンクを抑止する。問い合わせが 1 回応答され、後続チャンクの解析が参照と一致する。reattach 経路の既存テスト（visible_reattach_redelivers_a_cut_csi_tail_and_the_client_view_matches_the_reference、visible_reattach_redelivers_a_cut_utf8_tail_with_no_replacement_character_in_any_row）は変更なしで通る。 |
| TS-9 | NFR5 | 敵対的な入力（指定文字の連鎖、切り替え列と未完了の導入子の繰り返し、長い持ち越し列）で、置換出力の組み立てと書き込みフィルタが時間予算内に終わり、パニックしない。 |
| TS-10 | NFR6 | `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が通る。 |

### 12.2 エッジケース

- pending が上限 512 KiB を超えて吐き出された列は、完結として扱わない（FR6 の受け渡し対象にならない）
- FR8 の併存で尾部が `ESC (` / `ESC )` のとき、先頭の問い合わせ・起動列の ESC が指定文字として消費されないようにしたうえで、後続チャンクの先頭バイトが指定文字として消費される状態に戻す
- FR6 で持ち越した起動が、代替画面に切り替わって終わる抑止チャンクの主画面区間で完結する
- 連続する抑止チャンクで、先の置換出力が再送した pending の続きが次の抑止チャンクで完結する（それぞれの snapshot の後に 1 回だけ届く）
- 同じ送信先に snapshot が続けて送られたときは、チャンクを覆う境界の記録の snapshot 情報を使う
- 抑止したチャンクのインライン画像（Kitty APC・SIXEL DCS）と OSC 777 emterm の image は届けず、既知の欠落のまま（前フィーチャー FR7）

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| 抑止チャンク | snapshot 取得と重なり、通常の配送を抑止した読み取りチャンク |
| 保持窓 | 置換出力の走査の再開位置を選ぶために保持する直前のバイト列。上限 N = 256 バイト |
| 置換出力 | 抑止したチャンクについて、snapshot の後に既存の PtyOutput チャンクとして送る出力。配送順序は、問い合わせとビューア起動を元の順序で、その後に尾部 |
| pending | 書き込みフィルタが保留する、OSC・DCS・APC と末尾の単独 ESC。上限 512 KiB |
| 指定文字 | 文字集合指定 `ESC (` / `ESC )` の直後に続き、指定文字として消費される 1 バイト |
| 境界の記録 | 送信先と抑止境界ごとの記録。FR8 で、生成した snapshot の情報を保持する |

## 14. 確認事項

### 14.1 確認済み事項

- [x] as-01: 『クライアントと同じ解析規則』の基準は、term_core のパーサーの遷移と、GUI の theme（src-tauri/src/render/theme.rs）の応答生成とする。前フィーチャー SPEC の FR2 の遷移規則表 (a)〜(h) を引き継ぐ。
- [x] as-02: FR8 の対象判定は、生成された snapshot のバイト列がクライアントのパーサーを未完了の尾部の状態で終えるかで行う。現行の組み立てでこれに当たるのは、主画面の可視化復帰の snapshot のうちダンプブロックが付かないもの（リング未周回、またはリング周回済みで画面ダンプが空・生成失敗）。reattach・オンデマンドの snapshot（末尾に ESC[?1049l）、代替画面の snapshot、ダンプブロック付きの snapshot は、daemon が付け足すバイトで終わる。
- [x] as-03: daemon が付け足すバイトと尾部の干渉は、このフィーチャーでは直さない。対象は、reattach・オンデマンドの末尾の切り替え列、可視化復帰の代替画面の ESC[?1049h、ダンプブロック等。これらの形では、置換出力は従来どおり尾部を再送する。
- [x] as-04: 前フィーチャーの不変条件を守る。対象は、ロック順序、output_target を保持中に blocking_send しないこと、mux_ipc のワイヤ形式、pending の上限 512 KiB、保持窓の上限 N = 256 バイト、置換出力の配送順序（snapshot → 問い合わせとビューア起動を元の順序で → 尾部 → 後続チャンク）、未完了 CSI の尾部から C0 を除くこと。
- [x] as-05: 保持窓に判定可能な ESC が無いときは通常状態から走査し、リングに書かない区間で N を超える文字列の途中から続く場合は既知の欠落とする（前フィーチャー as-05）。FR1 で除外する位置が増えても、結果は見逃し側に倒れる。リングに書く区間の長い列は FR6 で扱う。
- [x] as-06: term_core の OSC 番号累積で起きる u16 のあふれ（osc.rs:22）は、このフィーチャーでは直さない。共通の識別は、あふれた番号を識別しない。
- [x] as-07: term_core は、UTF-8 の途中で ESC を受けると置換文字を出さずに途中のバイトを捨てる。根拠は既存テスト visible_reattach_redelivers_a_cut_utf8_tail_with_no_replacement_character_in_any_row（snapshot 末尾の ESC[?1049l がこの形になる）。create-plan で term_core の実装を確認する。
- [x] as-08: `ESC (` / `ESC )` の直後に切り替え列が来る形（クライアントは切り替え列の ESC を指定文字として消費する）での、主画面区間の抽出（extract_main_buffer_bytes）と shadow parser の扱いは変えない。
- [x] as-09: AgentStatusFeedScanner と PassthroughScanner の OSC の検出は変えない。FR7 の共通の層を使うのは、scrollback の除去と置換出力の抽出だけ。
- [x] as-10: round2.yaml の dropped_at_gate の low 2 件は対象外。うち色問い合わせの二重ループは、FR2 で overlaps_ranges を無くすことで無くなる。
- [x] as-11: 起動引数の追加指示（integration ブランチの push と PR 作成、不明点の Codex 相談と Notion への記録）はワークフローの運用として扱い、機能要件には含めない。

### 14.2 未確認・保留事項

なし（status: tbd の要件は無い）。

## 15. 参考資料

- review round 2 の指摘: ブランチ em-workflow/mux-suppressed-output-fixes/integration の feature-docs/mux-suppressed-output-fixes/reviews/round2.yaml
- 該当箇所:
    - src-tauri/src/mux/ipc/pty_spawn/client_parity_scan.rs
    - src-tauri/src/mux/ipc/pty_spawn/suppressed_output.rs
    - src-tauri/src/mux/ipc/pty_spawn/write_filter.rs
    - src-tauri/src/mux/ipc/pty_spawn/mod.rs
