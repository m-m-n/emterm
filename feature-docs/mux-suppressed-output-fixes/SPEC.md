# Feature: mux-suppressed-output-fixes

## 概要

抑止したチャンクの置換出力の組み立てと ScrollbackWriteFilter の判定を、クライアント（term_core のパーサーと GUI の theme の応答生成）の解析結果に揃える。あわせて、mux-snapshot-output-boundary（PR #107）の review round 1 で unresolved の medium 指摘 18 件（19 エントリ）について、判定・根拠・回帰テストとの対応を記録する。要件の詳細は [REQUIREMENTS.md](REQUIREMENTS.md) を参照。

## 目的

- 抑止したチャンクの副作用処理（置換出力の組み立て）を、抑止しなかった場合のクライアントの解析結果と同じにする。クライアントは term_core で解析し、色問い合わせへの応答は GUI の theme が作る。
- mux-snapshot-output-boundary（PR #107）の review round 1 で unresolved の medium 指摘 18 件（19 エントリ）それぞれについて、対応済みか対応不要かを判断し、根拠と回帰テストとの対応を記録する。

## ユーザーストーリー

該当なし。UI の変更は無い。変わるのは mux デーモン内部のバイト列処理とテストだけ。

## 技術要件

### 機能要件

- **FR1:** チャンク末尾の ESC で文字列を中断扱いにしない。OSC・DCS・APC の本体の途中にある ESC がチャンクの最後のバイトのときは、中断ではなく未完了として扱う。その文字列を開始位置から尾部として保持・再送する。置換出力の走査と ScrollbackWriteFilter の両方を同じ判定にする。
- **FR2:** 走査の遷移をクライアント（term_core）と一致させる。置換出力の走査は、クライアント側の term_core パーサーと同じ遷移で判定する。規則は下の「走査の遷移規則（FR2）」の表に示す。
- **FR3:** 未完了 CSI の尾部から C0 を除く。尾部として再送する未完了の CSI からは、C0 制御文字を除く。前のチャンクから持ち越した部分も同じ扱いにする。C0 の効果は snapshot（リングの再生または shadow parser の画面）にすでに入っている。
- **FR4:** pending による除外を元チャンクの区間に限る。pending を理由に走査から除外する範囲は、pending に対応する元チャンクの区間だけにする。リングに書かない区間（代替画面側）にある問い合わせとビューア起動、およびチャンク自身の未完了尾部は、pending を理由に落とさない。
- **FR5:** 書き込みフィルタの保留規則を term_core に揃え、本当に未完了の末尾だけを再送する。ScrollbackWriteFilter の完結・中断の判定を FR2 (a)〜(e) に揃える。打ち切られた OSC・DCS・APC は閉じたものとして扱い、pending に保留しない。pending に残るのは、本当に未完了の末尾だけになる。置換出力で pending を再送するときも、すでにクライアントに届いた問い合わせ・文字・完結したシーケンスは再送しない。pending の上限（512 KiB）と、上限を超えたときの吐き出しは変えない。
- **FR6:** チャンクをまたぐ解析状態の持ち越し（末尾の有界なコピー）。reader の通常経路では、読み取りごとに直前チャンクの末尾を上限 N バイトまでコピーして保持するだけにする。通常経路では走査しない。抑止したチャンクを走査するときだけ、保持した末尾を FR2 の規則で解析して走査の開始状態を決める。開始状態は、通常・ESC・ESC＋中間バイト・CSI・文字列・UTF-8 の途中のいずれかになる。
  - 前のチャンクで始まって抑止したチャンクで完結した問い合わせは、開始位置からの全バイト（C0 を除く）を 1 回届ける。
  - 前のチャンクで始まり、抑止したチャンクの末尾でも未完了のシーケンスは、開始位置からの全バイトを尾部として再送する。
  - N は create-plan で決め、CSI・ESC＋中間バイト・UTF-8 の未完了部分の長さを覆う値にする。
  - 保持した末尾に未完了シーケンスの開始が無いときは、通常状態から走査する。上限を超える長い文字列の途中がこれに当たる。リングに書く区間の文字列は pending で扱い、それ以外で上限を超える場合は既知の欠落として記録する。
  - 抑止したチャンクが連続するときは、先の置換出力で送った尾部の続きが次の抑止チャンクにあっても、クライアントのパーサーが未完了状態のまま次の転送チャンクを受け取ることはない。続きが問い合わせなら 1 回だけ応答され、そうでなければ効果が二重に適用されない。
- **FR7:** 抑止したチャンクのビューア起動を 1 回届ける。抑止したチャンクで完結したビューア起動は、置換出力に含めて snapshot の後に 1 回届ける。ビューア起動とは、子 WebView ウィンドウを開く OSC 777 emterm の markdown・json・yaml と、OSC 9999 emterm-md を指す。
  - 問い合わせとビューア起動は元の順序で並べ、尾部はその後に置く。
  - インライン画像（Kitty APC・SIXEL DCS）は届けず、既知の欠落として残す。
  - OSC 777 emterm の image がビューア起動かインライン画像かは create-plan で確認する。
  - 同じビューア起動を二重に届けない。pending の再送で届くもの、クライアントにすでに届いたものは、抽出の対象から外す。
  - 判断表では、9e6a468b3a45ceeb の中で対象ごとの判定を記録する（ビューア起動は対応済み、インライン画像は対応不要）。c8aa5052b1a02acd のビューア起動の部分は、この『重複させない』要件として扱う。
- **FR8:** 可視化復帰の snapshot で画面モードを復元する。可視化復帰の snapshot（resume_pane_with_permit）を適用した後、クライアントが表示している画面（主画面か代替画面か）は、snapshot を読んだ時点の shadow parser の状態と一致する。
  - 代替画面なら、shadow parser の画面内容が代替画面に表示される。主画面なら、主画面の履歴と画面が表示される。
  - hidden の間に画面が切り替わった場合と、切り替えを含むチャンクが抑止された場合の両方でこれが成り立つ。
  - 主画面のペインを主画面のままのクライアントへ復帰するときは、カーソル位置とスクロール領域を含めて今と同じ表示になる（apt の進捗バー）。
  - バイト列の形を変えるのは可視化復帰の snapshot だけで、reattach とオンデマンドの snapshot は変えない。
  - term_core は 47/1047/1049 で処理を止め、画面の切り替えは呼び出し側が行う（terminal_dispatch.rs:16-19）。ChunkKind::Snapshot の中の切り替えをクライアントがどう適用するかは、create-plan で確認する。
- **FR9:** カーソル位置の応答（対応不要として記録）。判断表に対応不要と記録し、理由も残す。理由は 2 つ。snapshot 適用後の位置で応答する差は前 SPEC のエッジケースで許容済みであること。修正前も、チャンクを二重に適用した途中の位置で応答していたこと。そのうえで、抜き出した問い合わせが snapshot の後・次のチャンクより前に元の順序で 1 回ずつ届くこと、応答の数が問い合わせの数と一致することをテストする。
- **FR10:** ResumeWithSnapshot 分岐を削除する。evaluate_output_target は Detached から Connected への遷移を行わない。見える状態への遷移は resume_pane_with_permit だけが行う。EvalResult::ResumeWithSnapshot と、それを照合するテストを削除する。
  - 本番の唯一の呼び出し元は handle_set_visibility の hidden 側（handlers/attach.rs:189、visible=false）で、影響を受けない。
  - 削除するテストが確かめていた性質は、resume_pane_with_permit のテストで確かめる。対象は、ラップしたリングの見出し行・上限超過・shadow parser の poison・同じ接続での hide/show の往復。前 3 つのうち 2 つは既存テスト（pane/tests.rs:1848, 1984）で確かめられており、poison と往復のテストは移植する。
  - snapshot が置換出力より先に届く順序は、実際の復帰経路でテストする。
- **FR11:** 停止点の準備順序を直す。split_osc9_across_two_suppressed_chunks_never_fires_more_than_once では、chunk_a を解除する前に次の停止点（p2.arm）を準備する（現状は pty_spawn/tests.rs:1579 で解除、1583 で準備）。
- **FR12:** 対応済みの指摘を確認し、残りの 1 件に対応する。
  - 692921cdd030612d と 9a7dc7697c6af992 は対応済みとして記録する。collect_reattach_data はサイズ判定を output_target ロックの外で、encoded_snapshot_segments_len による算術で行い、エンコードはしない（reattach.rs:249-265）。エンコードは send_reattach_data の 1 回だけ（reattach.rs:421）。
  - 5988c2406aa06a7b も対応済みとして記録する。visible reattach の停止点付きテスト（pty_spawn/tests.rs:3921）と、resume_pane_with_permit の停止点付きテスト（pane/tests.rs:2114）が既にある。
  - aca2b1d612ab97e0 には対応する。reader・resize と並行して実際の snapshot 経路を回すテストを加える。経路は collect_reattach_data、handle_request_pane_snapshot、resume_pane_with_permit の 3 つで、FR10 の後は 3 経路になる。既存の stress テスト（pty_spawn/tests.rs:1972）は captured_read と record_boundary の代替操作を使っている。
- **FR13:** 判断の記録。feature-docs/mux-suppressed-output-fixes/ に、stable_id ごとの判断表を置く。各行には、判定（対応済み／対応不要）・根拠・対応する回帰テストを書く。18 件をすべて載せ、9e6a468b3a45ceeb は対象ごとの判定を分けて書く。mux-snapshot-output-boundary/reviews/round1.yaml は変更しない。

### 非機能要件

- **NFR1 - 互換性（ワイヤ形式と snapshot の形）:** mux_ipc のワイヤ形式と、Snapshot / SnapshotRestore のフレームの形は変えない。reattach とオンデマンドの snapshot のバイト列の形も変えない。可視化復帰の snapshot は、画面モードの復元（FR8）に必要な分だけ形を変えてよい。これは前フィーチャー NFR1 の限定例外とする。置換出力は、既存の PtyOutput チャンクとして送る。
- **NFR2 - ロック規律:** output_target を保持したまま blocking_send しない。ロックの順序は、output_target → 捕捉の排他 → リング・shadow parser とする。
- **NFR3 - reader 通常経路の負荷:** reader の通常経路に足す処理は、連番の比較と、直前チャンク末尾の上限 N バイトのコピーだけにする。これは前フィーチャー NFR4 の限定例外とする。解析、問い合わせとビューア起動の抜き出し、尾部の組み立ては、抑止したチャンクのときだけ行う。
- **NFR4 - セキュリティ（TM-1）:** クライアントのパーサーが制御シーケンスを始めない位置から、問い合わせやビューア起動を作り出さない。置換出力は、その snapshot を受け取った送信先にだけ送る。
- **NFR5 - セキュリティ（TM-2）:** 置換出力の走査は、保持した末尾を含めて有界な 1 パスで行う。空の置換出力を、空の PtyOutput チャンクにしない。ScrollbackWriteFilter のメモリ上限（pending 512 KiB）は変えない。
- **NFR6 - ビルドとプラットフォーム:** --no-default-features のビルドが通る。Linux と Windows で同じように動く。

## 実装方針

### 走査の遷移規則（FR2）

基準は term_core のパーサーの遷移（crates/term_core/src/parser/）と、GUI の theme の応答生成（src-tauri/src/render/theme.rs）とする（as-01）。

| 項 | 状態・入力 | 遷移 | 参照 |
|----|-----------|------|------|
| (a) | OSC 中の ESC | 次が `\` なら ST で完結する。それ以外なら OSC を打ち切り、そのバイトを新しいエスケープシーケンスとして処理する | osc.rs:37-57 |
| (b) | DCS・APC 中の ESC | (a) と同じ規則で打ち切る | dcs.rs:19-34, apc.rs:19-34 |
| (c) | ESC X（SOS）・ESC ^（PM） | 文字列を始めない。2 バイトのエスケープシーケンスとして終わり、続くバイトは通常の状態で処理する | escape.rs:53-60 |
| (d) | ESC ( ・ESC ) | 直後の 1 バイトを ESC も含めて指定文字として消費する。ほかの中間バイト（0x20-0x2F）はそのバイト自体で終わる | escape.rs:34-39, 64-73 |
| (e) | ESC ESC | Escape 状態に留まる | escape.rs:49-52 |
| (f) | CSI 内の C0・ESC | C0 はその場で実行して CSI を続ける。ESC は CSI を打ち切って新しいエスケープを始める | csi.rs:36-44, 76-84 |
| (g) | OSC 番号 | 最初の `;` までの数字を 10 進で累積する。先頭ゼロは値に影響しない | osc.rs:20-27 |
| (h) | 色問い合わせ | 打ち切られた OSC には応答しない。OSC 10/11/12 は `;` 区切りの項目が順に 10・11・12 に対応し、12 を超えたら終わる。前後の空白を除いて `?` の項目ごとに応答する。OSC 4 は `index;spec` の組ごとに、spec が `?` なら応答する。応答が 1 つ以上生じる OSC を問い合わせとして扱い、その OSC 全体を 1 回届ける | theme.rs:316-326, 493-536, 538-564 |

ScrollbackWriteFilter の完結・中断の判定は (a)〜(e) に揃える（FR5）。揃える範囲には、OSC の中断に加えて、DCS・APC の ESC による中断、ESC X／ESC ^ を文字列として扱わないこと、文字集合指定、ESC ESC を含める（as-03）。

### 置換出力の構成と配送順序

```
snapshot → 置換出力（問い合わせとビューア起動を元の順序で並べ、尾部はその後） → 後続チャンク
```

- 置換出力は既存の PtyOutput チャンクとして送る（NFR1）。空の置換出力は、空の PtyOutput チャンクにしない（NFR5）。
- 置換出力は、その snapshot を受け取った送信先にだけ送る（NFR4）。
- 設定と問い合わせが混ざった OSC（例: 10;#fff;?）が代替画面区間にあるときは、OSC 全体を 1 回届ける（as-04）。

### 処理の分担（FR6・NFR3）

| 経路 | 行う処理 |
|------|----------|
| reader の通常経路 | 連番の比較と、直前チャンク末尾の上限 N バイトのコピーだけ |
| 抑止したチャンクの処理 | 保持した末尾から開始状態を決める解析、問い合わせとビューア起動の抜き出し、尾部の組み立て |

### ロック規律（NFR2）

```
output_target → 捕捉の排他 → リング・shadow parser
```

output_target を保持したまま blocking_send しない。

### stable_id と要件の対応（FR13）

| stable_id | 対応する要件 | 記録する判定（要件に明記されたもの） |
|-----------|--------------|--------------------------------------|
| 19209420de72b144 | FR1 | — |
| 61d33252f22b6fd2 | FR1 | — |
| 8d069c589dc21784 | FR2 | — |
| 66d05376ff960d53 | FR2 | — |
| 001161ab9fa3c20b | FR3 | — |
| c8aa5052b1a02acd | FR4, FR5, FR7 | — |
| 66170217dc5057ce | FR5 | — |
| 3bc1e21fdd8702ef | FR6 | — |
| 30ee5a7036a7fc6e | FR6 | — |
| 9e6a468b3a45ceeb | FR7 | ビューア起動は対応済み、インライン画像は対応不要 |
| 830f950f39e499fa | FR8 | — |
| 9a548939524b405a | FR9 | 対応不要 |
| 29ff65b6c01032dc | FR10 | — |
| 39267160fcf1bb2a | FR11 | — |
| 692921cdd030612d | FR12 | 対応済み |
| 9a7dc7697c6af992 | FR12 | 対応済み |
| 5988c2406aa06a7b | FR12 | 対応済み |
| aca2b1d612ab97e0 | FR12 | — |

### 依存

**内部依存:**
- term_core のパーサー（crates/term_core/src/parser/）: 走査の遷移規則の基準（as-01）
- GUI の theme（src-tauri/src/render/theme.rs）: 色問い合わせの応答有無の基準（as-01）

**外部依存:** なし

## 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

デフォルトで、次の 2 つをフィーチャー固有のパスに加えて宣言する。

- `feature-docs/mux-suppressed-output-fixes/**`
- `test-docs/mux-suppressed-output-fixes/**`

`feature-docs/mux-suppressed-output-fixes/**` は `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物を含む。生成主体は各フェーズドキュメントおよび `references/phase-state.md` で、この節はそれを引用するだけでルールは再掲しない。

`test-docs/mux-suppressed-output-fixes/**` はタスクごとのテスト記録 `test-docs/mux-suppressed-output-fixes/{T}.tests.yaml` を含む。生成主体は `implement-phase.md` で、この節はそれを引用するだけでルールは再掲しない。

この 2 つのデフォルトは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載が無いことで除外とはみなさない。除外は意図的な絞り込みとして明示する。

この宣言はスーパーセットの主張であり、検証時に観測された実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。implement タスクを 1 つも生成しないフィーチャーは `test-docs/mux-suppressed-output-fixes/` を生成しないが、その場合も宣言された `test-docs/mux-suppressed-output-fixes/**` は正しい。宣言されたパスが生成されないことは違反ではない。

## テストシナリオ

### 単体・統合テスト
- [ ] TS-1（FR1）: 代替画面で、OSC 11;? の ST が ESC と `\` の間でチャンクに分割され、前半のチャンクが抑止される。問い合わせが 1 回届き、後続チャンクの `\` が文字として表示されない。書き込みフィルタ側でも、末尾 ESC で OSC を中断しない
- [ ] TS-2（FR2）: 抑止したチャンクに次のバイト列を入れ、それぞれ term_core を参照として比べる。ESC ( ESC [ 6 n／ESC X ... ESC [ 6 n／ESC P q ESC [ 6 n ESC `\`／ESC ESC [ c／OSC 010;?／OSC 10;?;?／OSC 10;#fff;?／OSC 4;1;?;2;#000000／打ち切られた OSC 11;?。抜き出す問い合わせの有無と応答の数が、参照と一致する
- [ ] TS-3（FR3）: LF を途中に含む未完了 CSI でチャンクが切れる（前のチャンクから持ち越した場合も含む）。LF が 2 回実行されない
- [ ] TS-4（FR4）: pending が空でないとき、抑止したチャンクの代替画面区間に問い合わせ・ビューア起動・未完了尾部がある。いずれも落ちない
- [ ] TS-5（FR5）: 前の読み取りで打ち切られた OSC・DCS の後に、CSI 6n と文字が続く。書き込みフィルタは pending に保留せず、抑止時に CSI 6n と文字が再送されない。リングの中身は、strip 後に参照と一致する
- [ ] TS-6（FR6）: ESC [ 6 が届いた後に、n で始まるチャンクが抑止される。応答は 1 回。UTF-8 の先頭バイトが届いた後に、続きのバイトを含むチャンクが抑止される。置換文字は出ない。分割位置を 1 バイトずつ変えて確かめる。上限 N を超える未完了シーケンスの挙動も確かめる
- [ ] TS-7（FR6）: 連続する 2 つのチャンクが抑止され、1 つ目の末尾の未完了 CSI（問い合わせではない SGR）が 2 つ目で完結する。次の転送チャンクの先頭バイトが CSI の終端として消費されない
- [ ] TS-8（FR7）: 抑止したチャンクの中で完結した OSC 777 emterm markdown が、snapshot の後に 1 回届く。pending の再送と重なる場合も 1 回。Kitty APC は届かない
- [ ] TS-9（FR8）: 可視化復帰の直前に ESC[?1049h を含むチャンクが抑止される。hidden 中に ESC[?1049l を経て主画面に戻る。代替画面のまま復帰する。どの場合も、画面モードと内容が shadow parser と一致する
- [ ] TS-10（FR8）: apt 形式の進捗バー（DECSTBM と DECSC/DECRC）を表示中の主画面ペインを、hide した後に show する。カーソル位置・スクロール領域・表示が変更前と同じになる
- [ ] TS-11（FR9）: A ESC[6n B ESC[c を含むチャンクが抑止される。問い合わせが snapshot の後に元の順序で 1 回ずつ届き、応答の数が 2 になる
- [ ] TS-12（FR10）: handle_set_visibility の visible 側（resume_pane_with_permit）で、snapshot → 置換出力 → 後続チャンクの順に届く。poison された shadow parser と、hide/show の往復を resume_pane_with_permit で確かめる
- [ ] TS-13（FR11）: split_osc9_across_two_suppressed_chunks_never_fires_more_than_once で、p2.arm を解除より前に行う

### 並行性テスト
- [ ] TS-14（FR12）: reader と resize を回しながら、collect_reattach_data・handle_request_pane_snapshot・resume_pane_with_permit を並行して繰り返す。時間予算内に終わり、デッドロックしない

### E2E テスト
**既存の E2E テスト**: なし
**実行コマンド**:
- `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`

### 挙動を変える既存テスト（AC-8）
意図して挙動を変えるテストは一覧化する。一覧に入る見込みのテストは次のとおり。
- suppressed_output.rs の bare_trailing_esc_is_treated_as_a_tail
- suppressed_output.rs の embedded_esc_aborts_the_osc_and_the_following_csi_query_is_extracted
- suppressed_output.rs の pending_* の各テスト
- pty_spawn/tests.rs:4017, 4153（DCS 内に見える CSI 6n を届ける形に変わる。as-03）
- snapshot_bytes.rs:688-717
- ResumeWithSnapshot を照合する pane/tests.rs のテスト

### エッジケース・既知の欠落
- [ ] 抑止したチャンクのインライン画像（Kitty APC・SIXEL DCS）は届けず、既知の欠落として残す（FR7）
- [ ] 保持した末尾（上限 N）の中に未完了シーケンスの開始が無いときは、通常状態から走査する。リングに書かない区間で N を超える文字列の途中から続く場合は、既知の欠落として記録する（FR6、as-05）
- [ ] term_core の OSC 番号累積で起きる u16 のあふれ（osc.rs:22）は、このフィーチャーでは直さない（as-06）

## セキュリティ

- **入力検証（TM-1、NFR4）:** クライアントのパーサーが制御シーケンスを始めない位置から、問い合わせやビューア起動を作り出さない。置換出力は、その snapshot を受け取った送信先にだけ送る。
- **資源の上限（TM-2、NFR5）:** 置換出力の走査は、保持した末尾を含めて有界な 1 パスで行う。空の置換出力を、空の PtyOutput チャンクにしない。ScrollbackWriteFilter のメモリ上限（pending 512 KiB）は変えない。

## エラー処理

- pending の上限（512 KiB）と、上限を超えたときの吐き出しは変えない（FR5）。

## 性能

- reader の通常経路に足す処理は、連番の比較と、直前チャンク末尾の上限 N バイトのコピーだけにする（NFR3）。
- 置換出力の走査は、保持した末尾を含めて有界な 1 パスで行う（NFR5）。

## 前提

| ID | 内容 | 影響度 |
|----|------|--------|
| as-01 | 『クライアントと同じ解析規則』の基準は、term_core のパーサーの遷移（crates/term_core/src/parser/）とする。色問い合わせの応答の有無は、GUI の theme（src-tauri/src/render/theme.rs）の応答生成を基準にする | high |
| as-02 | 前フィーチャーの不変条件は、このフィーチャーでも守る。対象は、ロック順序、output_target を保持中に blocking_send しないこと、mux_ipc のワイヤ形式を変えないこと | high |
| as-03 | FR2・FR5 の揃える範囲には、OSC の中断に加えて、DCS・APC の ESC による中断、ESC X／ESC ^ を文字列として扱わないこと、文字集合指定、ESC ESC を含める。この結果、pty_spawn/tests.rs:4017, 4153 の期待値は、DCS 内に見える CSI 6n を届ける形に変わる | medium |
| as-04 | 設定と問い合わせが混ざった OSC（例: 10;#fff;?）が代替画面区間にあるときは、OSC 全体を 1 回届ける | low |
| as-05 | 保持した末尾（上限 N）の中に未完了シーケンスの開始が無いときは、通常状態から走査する。リングに書かない区間で N を超える文字列の途中から続く場合は、既知の欠落として記録する | medium |
| as-06 | term_core の OSC 番号累積で起きる u16 のあふれ（osc.rs:22）は、このフィーチャーでは直さない | low |

## 成功基準

- [ ] AC-1: 対応した各指摘に、修正前のコードで失敗し、修正後に通る再発検出テストがある
- [ ] AC-2: snapshot・置換出力・後続チャンクを term_core に流した結果が、生のストリームを 1 回流した参照と一致する。比べるのは、画面・カーソル・応答バイト列・表示された文字。対象は、直前のチャンク、抑止したチャンク（1 つまたは連続）、直後のチャンクの組で、分割位置を変えて確かめる。続きのバイトや置換文字は表示されない。対象のケースは、チャンクをまたぐ CSI・UTF-8・ESC＋中間バイト、末尾 ESC、C0 を含む CSI 尾部、打ち切られた OSC・DCS・APC を含む pending、pending の後ろにある代替画面区間
- [ ] AC-3: 抑止したチャンクの問い合わせは、snapshot の後・次のチャンクより前に、元の順序でちょうど 1 回ずつ届く。次の 3 つは届かない。すでにクライアントに届いた問い合わせ、クライアントのパーサーが始めない位置にある問い合わせ形のバイト（ESC ( の後の ESC [ 6 n など）、打ち切られた OSC
- [ ] AC-4: 抑止したチャンクで完結したビューア起動は、snapshot の後にちょうど 1 回届き、二重には届かない。インライン画像は届かない
- [ ] AC-5: 可視化復帰の後、クライアントの画面モードと画面内容が shadow parser と一致する。対象は、代替画面への切り替え・代替画面からの復帰・hidden 中の切り替え・切り替えを含む抑止チャンク。apt の進捗バーを表示中の主画面ペインを復帰した表示は、変更前と同じになる
- [ ] AC-6: EvalResult::ResumeWithSnapshot が存在しない。実際の復帰経路で、snapshot が置換出力より先に届く
- [ ] AC-7: 18 件の stable_id それぞれについて、判定・根拠・回帰テストとの対応が feature-docs/mux-suppressed-output-fixes/ の判断表に記録されている。round1.yaml は変更されていない
- [ ] AC-8: 意図して挙動を変えるテスト以外の既存テストが、変更なしで通る。挙動を変えるテストは一覧化されている
- [ ] AC-9: CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib と、CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features が通る

## 未解決事項

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

`status: tbd` の要件は無い。

create-plan で確認する事項:
- [ ] FR6: 保持する末尾の上限 N の値（CSI・ESC＋中間バイト・UTF-8 の未完了部分の長さを覆う値）
- [ ] FR7: OSC 777 emterm の image がビューア起動かインライン画像か
- [ ] FR8: ChunkKind::Snapshot の中の画面切り替えをクライアントがどう適用するか

## 参考資料

- 要件定義書: [REQUIREMENTS.md](REQUIREMENTS.md)
- mux-snapshot-output-boundary（PR #107）の review round 1: mux-snapshot-output-boundary/reviews/round1.yaml
- term_core のパーサー: crates/term_core/src/parser/
- GUI の theme の応答生成: src-tauri/src/render/theme.rs
