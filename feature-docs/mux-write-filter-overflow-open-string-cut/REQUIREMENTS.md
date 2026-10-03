---
title: "mux-write-filter-overflow-open-string-cut"
created_date: 2026-10-04
status: draft
---

# mux-write-filter-overflow-open-string-cut - 要件定義書

## 1. 概要

### 1.1 背景
scrollback の write filter のオーバーフロー flush が、OSC / DCS / APC の文字列本体を開いたままリングに書き込むことがある。その後に cut（削除される 47 / 1047 / 1049 の画面切替、または reader の fallback closing）が来ても、リング上の文字列本体は閉じられない。

前身フィーチャー mux-write-filter-overflow-lone-esc のレビュー指摘 ed366a655164f747（medium）が未解決のまま残っている。オーバーフローの run が素の本体バイトで終わり、その後に cut が来る既存のケースも同じ原因を持つ。

### 1.2 目的
- オーバーフロー flush が開いた文字列本体をリングに残したとき、後続の cut がリング上でその文字列を閉じ、snapshot replay がクライアントの見た内容と一致すること。
- cut の後に書かれたメインバッファのテキストが replay で表示され、開いた文字列本体に吸収されないこと。後続の BEL がそのテキストで文字列を完了させないこと（そのテキストからタイトルが設定されないこと）。

### 1.3 スコープ
**対象**:
- write filter が保持する「書き込み済みストリームの終端状態」に、開いた OSC 本体・開いた ST 終端本体（DCS / APC）の状態を加える（FR1）
- 開いた文字列本体の状態で cut が来たときに `ESC` + CAN を書き込む（FR2）
- 開いた本体状態の呼び出し間の持ち越し（FR3）
- 関連するテストと doc コメントの更新（FR6 / FR7）

**対象外**:
- cap の値、オーバーフロー警告ログ、クライアントへのライブ転送、snapshot 時の strip、reader ループの cut 導出（NFR5）
- strip が除去する構造（cut なし）の `ESC` がクライアント側で開いた本体を中断したケース（前提 A4）

## 2. ビジネス要件

### 2.1 ビジネス目標
- オーバーフロー flush 後の cut で、リングの snapshot replay がクライアントの見た表示と一致すること。
- レビュー指摘 ed366a655164f747（medium）と、同じ原因を持つ「素の本体バイトで終わるオーバーフロー run の後の cut」のケースを解消すること。

### 2.2 対象ユーザー
| ユーザータイプ | 説明 |
|----------------|------|
| mux クライアント | ペインのリングの snapshot replay を表示する側 |

### 2.3 期待される効果
- cut の後に書かれたメインバッファのテキストが replay で表示される。
- cut の後のテキストが開いた文字列本体に吸収されず、後続の BEL でタイトルとして設定されない。

## 3. ユースケース

### 3.1 ユースケース一覧
| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | オーバーフローした OSC の後に画面切替を挟んだペインの snapshot replay | mux クライアント | — |

### 3.2 ユースケース詳細

#### UC01: オーバーフローした OSC の後に画面切替を挟んだペインの snapshot replay

**アクター**: mux クライアント

**事前条件**:
- ペインの出力に `ESC]0;` で始まる OSC があり、cap を超えて伸びている
- cap を越える read が `ESC` で終わる

**基本フロー**:
1. 次の read が画面切替（`ESC[?1049h` ... `ESC[?1049l`）で始まり、その後にメインバッファの素のテキストが続く
2. write filter が cut の位置で開いた文字列本体のクロージャ `ESC` + CAN を書き込む
3. クライアントがペインのリングを replay する

**代替フロー**:
- cut が reader の fallback closing である場合も、同じクロージャを書き込む

**事後条件**:
- replay でメインバッファのテキストが表示される
- replay の表示（行・カーソル・応答）が、生のストリームを term_core に流した参照と一致する

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 書き込み済み状態で開いた文字列本体を区別する | 書き込み済みストリームの終端状態が、開いた OSC 本体・開いた ST 終端本体を ground と区別する | — |
| FR2 | cut での開いた文字列本体のクロージャ | 開いた文字列本体で終わるとき、cut が `ESC` + CAN を書き込む | — |
| FR3 | 開いた本体状態の呼び出し間の持ち越し | オーバーフロー flush が設定した開いた本体状態を後続の呼び出しに持ち越し、書き込むバイトで進める | — |
| FR4 | オーバーフローの保持判定は変更しない | run 末尾の lone `ESC` を保持する判定は変えず、保存する書き込み済み状態だけ開いた本体状態にする | — |
| FR5 | cut 1 回につきクロージャ 1 つ、strip 対象にしない | クロージャは終端状態だけで選び、strip 対象を開始せず ST も作らない | — |
| FR6 | 回帰テストと前身テストの期待値更新 | 回帰を検出するテストを追加し、前身の開いた本体の期待値を更新する | — |
| FR7 | doc コメントを新しいモデルに合わせる | 旧モデルを述べる doc コメントを更新する | — |

### 4.2 機能詳細

#### FR1: 書き込み済み状態で開いた文字列本体を区別する

**説明**: write filter が持ち回る書き込み済みストリームの終端状態（および状態を報告する strip が報告する状態）は、ground と区別して「開いた OSC 文字列本体の中」と「開いた ST 終端文字列本体（DCS `ESC P`、APC `ESC _`）の中」を表す。遷移は term_core の遷移に従う。strip が除去するバイトは状態を進めない（従来どおり）。表現（新しい WrittenState のバリアントか、別の O(1) フラグか）は plan で決める。

**状態遷移**:
| 現在の状態 | 入力 | 次の状態 |
|------------|------|----------|
| — | `ESC ]` | OSC 本体 |
| — | `ESC P` / `ESC _` | ST 終端本体 |
| OSC 本体 | BEL | ground |
| OSC 本体 / ST 終端本体 | `ESC` | 既存の Escape 状態 |
| Escape（本体の `ESC` の後） | `\` | ground（ST 完了） |
| Escape（本体の `ESC` の後） | その他のバイト | 文字列は中断し、そのバイトを Escape 状態から処理する |
| OSC 本体 / ST 終端本体 | 上記以外のバイト | 本体の状態を維持 |

ST 終端本体では BEL は本体を閉じない（AC-6）。

#### FR2: cut での開いた文字列本体のクロージャ

**説明**: 書き込み済みストリームが開いた文字列本体（FR1）で終わるとき、cut はクロージャ `ESC` + CAN（0x1b 0x18）を書き込む。term_core はこれを中断された（Unterminated）文字列と、それに続く完了したエスケープとして扱い、ground に戻る。

**適用する cut の経路**:
| 経路 | 参照する状態 |
|------|--------------|
| 呼び出し内の cut | cut より前の run の状態（strip 後） |
| 空のセグメントでの cut、および reader の fallback closing（空の fed 範囲、fed 0 の cut） | 持ち越した状態 |
| 同じ呼び出しでオーバーフロー flush に続く cut | flush した run の状態 |
| オーバーフロー flush が開いた本体の後に残した保持中のライブ lone `ESC` を捨てる cut / fallback closing | その `ESC` より前に書き込んだバイトの状態 |

**事後状態**: クロージャの後、書き込み済み状態は ground で、designator は待たない。

#### FR3: 開いた本体状態の呼び出し間の持ち越し

**説明**: オーバーフロー flush が設定した開いた本体状態は後続の呼び出しに持ち越され、その呼び出しが書き込むバイトで進む。

**ビジネスルール**:
- 書き込まれた BEL（OSC 本体のみ）、書き込まれた `ESC \`、または書き込まれた `ESC` に続く別のバイトは、FR1 に従って本体を閉じる。
- 書き込まれた素のバイトは本体の状態を維持する。
- 閉じた本体の後の cut は、そのときのストリームの状態に応じたクロージャを書き込む（ground なら何も書き込まない）。

#### FR4: オーバーフローの保持判定は変更しない

**説明**: オーバーフロー flush が run の最後のライブ lone `ESC` を保持する判定は変えない。開いた文字列本体の中で run を終える `ESC` は、ストリームを Escape に置いたまま、1 構造の連鎖として保持する。これと一緒に保存する書き込み済み状態は開いた本体状態とする（ground ではない）。cut がない場合に次の呼び出しが保持中の `ESC` をどう扱うか（続きと一緒に strip、`ESC \` は ST として書き込み、strip 対象でない続きは全体を書き込む）は変えない。

#### FR5: cut 1 回につきクロージャ 1 つ、strip 対象にしない

**説明**: cut は書き込み済みの終端状態だけで選んだクロージャを 1 つだけ書き込む。クロージャは strip の後に書き込み、outcome の dims に帰属させる。write 経路・snapshot 時の strip のどちらでも strip 対象を開始せず、ST も作らない。最初の fallback closing の直後の 2 回目の fallback closing は何も書き込まない。

**クロージャの選択**:
| 書き込み済みの終端状態 | クロージャ |
|------------------------|------------|
| CSI の中 | DEL |
| `ESC` の後 | CAN |
| designator 待ち | designator の `ESC` |
| 開いた文字列本体 | `ESC` + CAN |
| ground | なし |

#### FR6: 回帰テストと前身テストの期待値更新

**説明**: 回帰を検出するテストを置く。
- 「開いた文字列本体の中」で終わるケースの view_after_a_cut 比較（保持中の `ESC` を fallback closing で捨てる場合と、fed 0 の cut で捨てる場合）
- 素の本体バイトで終わるオーバーフロー run の後に cut が来るケースの view_after_a_cut 比較
- 本番の reader での再現

前身のテストのうち、開いた本体で終わるケースの旧挙動を固定しているもの（`endings()` の 'inside an open string body' エントリ: `state_before_esc` が Ground、`closing` が空）は、新しい状態と `ESC` + CAN に更新する。

#### FR7: doc コメントを新しいモデルに合わせる

**説明**: 旧モデルを述べている次の doc コメントを更新する。
- scrollback_filter.rs の WrittenState の doc（'an OSC / DCS / APC body needs no state of its own'）
- WrittenState::advance のエスケープ状態のコメント（'a string introducer enters a body that, for this question, is ground'）
- closure_for と ESCAPE_CLOSING の doc（'a cut writes at most one of the two'）
- write_filter.rs の feed_with_cuts の 'Overflow' / 'Closure at a cut' の段落と、オーバーフロー分岐のコメント（'a string body counts as ground'）

## 5. 非機能要件

### 5.1 パフォーマンス要件
- NFR1: 追加する状態は O(1) で、strip は 1 パスのまま。オーバーフロー分岐は flush する各 run を 1 回だけ strip する。

### 5.2 セキュリティ要件
該当なし

### 5.3 可用性要件
該当なし

### 5.4 保守性要件
- NFR4: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が成功する（mux は CLI ビルドに含まれる）。
- ドキュメント: FR7 の doc コメント更新

### 5.5 互換性要件
- NFR2: 同じ入力に対する strip の除去判定と strip 後のバイトは、持ち込まれた状態によらず変わらない。変わるのは報告する状態と cut でのクロージャだけ。
- NFR3: 開いた文字列本体を書き込んだまま残すことのない経路（オーバーフロー以外のすべての経路）が書き込むバイトは変わらない。`--lib` の全テストが、前身の開いた本体の期待値（FR6）の変更だけで通る。
- NFR5: cap の値、オーバーフロー警告ログ、クライアントへのライブ転送、snapshot 時の strip、reader ループの cut 導出は変えない。

## 6. UI/UX要件

該当なし（mux scrollback write filter のバイトストリーム状態追跡だけを変更する）

## 7. データ要件

該当なし

## 8. 外部連携

該当なし

## 9. 制約条件

### 9.1 技術的制約
- NFR1 / NFR2 / NFR5 の範囲内で変更する
- 書き込み済み状態の表現（WrittenState のバリアントか別の O(1) フラグか）は plan で決める

### 9.2 ビジネス上の制約
なし

### 9.3 スケジュール制約
なし

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-write-filter-overflow-open-string-cut/**`
- `test-docs/mux-write-filter-overflow-open-string-cut/**`

`feature-docs/mux-write-filter-overflow-open-string-cut/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-write-filter-overflow-open-string-cut/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-write-filter-overflow-open-string-cut/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、宣言された `test-docs/{feature}/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題
| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| DCS / APC 本体が `ESC` でだけ終わり BEL を無視すること、SOS / PM が文字列でないことは、term_core の dcs.rs / apc.rs で未確認（前提 A2） | — | plan で確認する |
| strip が除去する構造の `ESC` がクライアント側で開いた本体を中断したケースは、リングで本体が開いたまま残る（前提 A4） | — | 対象外 |

### 10.2 ビジネスリスク
該当なし

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC-1（FR2, FR6）: 本番の reader（`run_reader_without_owner`）で、read が `ESC]0;` の OSC を cap を超えて伸ばし、cap を越える read が `ESC` で終わり、次の read が画面切替（`ESC[?1049h` ... `ESC[?1049l`）とその後のメインバッファの素のテキストで始まる。ペインのリングを replay した term_core がそのテキストを表示し、その表示（行・カーソル・応答）が参照（生のストリームを流した term_core）と一致する。
- [ ] AC-2（FR2, FR4, FR5）: 保持中の `ESC` で開いた本体が終わるケース（cap で保持した OSC、呼び出し 1 = 本体バイト + `ESC`）で、reader の fallback closing がちょうど `ESC` + CAN を書き込み、`pending` は空、書き込み済み状態は ground、designator は待たない。2 回目の fallback closing は何も書き込まない。47 / 1047 / 1049 の各ペアについて、書き込んだバイトと後続の素のテキストの呼び出しの replay が、生のストリームの view_after_a_cut と一致する。
- [ ] AC-3（FR2）: 同じ終わり方の後、fed 0 の cut と素のテキストを運ぶ呼び出しが `ESC` + CAN とテキストを書き込む。`pending` は空、状態は ground。各切替ペアで replay が view_after_a_cut と一致する。
- [ ] AC-4（FR2, FR3）: 素の OSC 本体バイトで終わる（末尾に `ESC` がない）オーバーフロー run の後の cut（次の呼び出しの fed 0、および fallback closing）で、クロージャが `ESC` + CAN であり、後続の素のテキストを含む replay が view_after_a_cut と一致する。
- [ ] AC-5（FR2, FR5）: 同じ呼び出しでオーバーフロー flush に cut が続き、run が開いた本体で終わるとき、書き込むバイトは run の strip 結果に `ESC` + CAN が続いたもので、何も保持せず、状態は ground。
- [ ] AC-6（FR1, FR2）: DCS / APC の本体（strip 対象でない DCS / APC と、Kitty APC / SIXEL DCS を cap を超えて開いたままにしたもの）で、cut が `ESC` + CAN を書き込み、replay が view_after_a_cut と一致する。DCS / APC 本体の中の BEL は本体を閉じない（後続の cut はなお `ESC` + CAN を書き込む）。
- [ ] AC-7（FR3）: オーバーフローが開いた OSC 本体を残した後、BEL（または `ESC \`）を書き込む呼び出しが書き込み済み状態を ground に戻し、続く cut は何も書き込まない。素のバイトの呼び出しは開いた本体状態を維持する。
- [ ] AC-8（FR4, FR6）: 前身の開いた本体の終わり方を更新する。呼び出し 1 の後に `ESC` がちょうど 1 つ保持され、書き込み済み状態は開いた本体状態で、fallback closing は `ESC` + CAN を書き込む。`\` の続きはなお `ESC \` として書き込まれ ground に戻る。overflow_lone_esc のその他のテストは変更なしで通る。
- [ ] AC-9（NFR1, NFR2, NFR3, NFR4）: `--lib` の全テストが通り（その他の既存の期待値は変更しない）、`--no-default-features` の cargo check が成功する。
- [ ] AC-10（FR7）: FR7 に挙げた doc コメントが開いた本体状態と `ESC` + CAN のクロージャを述べ、文字列本体を ground とみなすと述べるものがない。

### 11.2 KPI
該当なし

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] TS-1（AC-1、Integration）: `run_reader_without_owner` を通した本番の reader での再現。リングの replay を生のストリームの参照と比較する。
- [ ] TS-2（AC-2, AC-3、Unit）: 保持中の `ESC` で開いた本体が終わるケースを、fallback closing と fed 0 の cut で、47 / 1047 / 1049 の各ペアについて view_after_a_cut を oracle として検証する。
- [ ] TS-3（AC-4, AC-5、Unit）: 素の本体バイトで終わるオーバーフローの後に、次の呼び出しの cut、fallback closing、同じ呼び出し内の cut が来るケース。
- [ ] TS-4（AC-6、Unit）: DCS / APC 本体。DCS / APC 本体の中の BEL を含む。
- [ ] TS-5（AC-7、Unit）: 呼び出し間の状態の持ち越し（BEL / `ESC \` で閉じる、素のバイトで維持する）。
- [ ] TS-6（AC-8、Unit）: overflow_lone_esc.rs の前身の期待値の更新。
- [ ] TS-7（AC-9）: `--lib` の全テストと `--no-default-features` の cargo check。
- [ ] TS-8（AC-10）: doc コメントの通読。

**oracle に関する注記**: term_core の終端状態 oracle `client_written_state`（scrollback_filter/tests.rs）は、現状では開いた OSC 本体を Escape に分類する（`m` と `[m` がどちらも吸収される）。新しい状態をこれで検証する場合は、それらを区別するプローブが要る（例: `\m` は Escape からは表示されるが本体からは表示されない。BEL `m` は OSC 本体からは表示されるが DCS / APC 本体からは表示されない）。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| cut | 削除される 47 / 1047 / 1049 の画面切替、または reader の fallback closing |
| fallback closing | reader の fallback closing（空の fed 範囲で fed 0 に cut がある呼び出し） |
| クロージャ | cut で書き込むバイト列（FR5 の表） |
| 開いた OSC 本体 | `ESC ]` の後で、BEL または ST でまだ閉じていない状態 |
| 開いた ST 終端本体 | DCS（`ESC P`）/ APC（`ESC _`）の後で、ST でまだ閉じていない状態 |
| ST | `ESC \` |
| 保持中の lone `ESC` | オーバーフロー flush が run の末尾で書き込まずに保持したライブの `ESC` |
| view_after_a_cut | テストで replay と比較する oracle |

## 14. 確認事項

### 14.1 確認済み事項
なし（batch 実行。ユーザーへの質問と回答はない）

### 14.2 未確認・保留事項（前提）
- [ ] A1（可逆）: 開いた文字列本体のクロージャは `ESC` + CAN とする（タスクとレビュー指摘 ed366a655164f747 の提案）。term_core の osc_escape は `\` 以外のバイトで OSC を Unterminated としてディスパッチし、そのバイトをエスケープとして再処理する。CAN はそのエスケープを効果なしで完了させる（ESCAPE_CLOSING の文書化された挙動）。クライアントは画面切替の `ESC` で同じ Unterminated ディスパッチを見ている。
- [ ] A2（可逆）: term_core の DCS / APC 本体は `ESC`（ST または中断）でだけ終わり BEL を無視する。SOS / PM（`ESC X`、`ESC ^`）は文字列ではない。これは term_core の dcs / apc ハンドラを写すと文書化された scan_boundary / find_st に基づく。dcs.rs / apc.rs は入力に含まれておらず、plan で確認する。
- [ ] A3（可逆）: 開いた文字列本体を書き込んだまま残せるのはオーバーフロー flush だけである。オーバーフロー以外の経路はすべて、不完全な文字列を書き込まずに `pending` に保持する。したがって新しい状態はオーバーフロー以外の出力を変えない（NFR3）。
- [ ] A4（可逆）: 対象外: strip が除去する構造（cut なし）の `ESC` がクライアント側で開いた書き込み済み本体を中断したケース（例: 開いた本体の後、後続の呼び出しで `abc ESC[6n def`）。リングでは本体が開いたまま残り、replay で `def` が吸収される。これは受け入れ済みの「開いた CSI と除去される構造」のケースと同じ種類で、直すと strip の出力が変わる（NFR2）。
- [ ] A5（可逆）: 前身の AC-5 / TS-4 の期待値「開いた文字列本体で終わるケースの後には何もない」と、前身 SPEC の FR4 / NFR2 の記述は、このフィーチャーで置き換える。test-docs/mux-write-filter-overflow-lone-esc/task0001.tests.yaml に載っている既存テストを改名する場合は、その記録を更新し、この SPEC の FR2 / FR6 を挙げる supersede 注記を加える（`.claude/rules/test-docs-records.md`）。名前を維持する場合、記録は変更しない。前身の feature-docs の記述はそのまま残す。
- [ ] A6（可逆）: タスク本文が原因説明で参照する「written-state model」は文脈であり、SPEC に根拠として写さない。

## 15. 参考資料

- 前身フィーチャー: `feature-docs/mux-write-filter-overflow-lone-esc/`
- test-docs 記録のルール: `.claude/rules/test-docs-records.md`
