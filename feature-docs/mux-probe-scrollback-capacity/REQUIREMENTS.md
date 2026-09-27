---
title: "mux-probe-scrollback-capacity"
created_date: 2026-09-27
status: draft
---

# mux-probe-scrollback-capacity - 要件定義書

## 1. 概要

### 1.1 背景
デーモンはリング一周後の snapshot 組み立てで、復元状態を求めるプローブ端末を 10,000 行固定の scrollback 容量で作る（`dump_block.rs:109`）。一方、クライアント（GUI タブ）は自分の scrollback 容量で再生用の端末を作る（`src-tauri/src/tabs/replay.rs:303-325`）。`scrollback_lines` が既定値以外（小さい値や 0）のとき、payload の途中に画面を広げるリサイズがあると、プローブとクライアントでカーソル位置がずれ、復元後の継続出力が別の行に書かれる。

### 1.2 目的
- リング一周済み mux pane の snapshot 復元（タブ切替 / reattach / 可視化による再開 / オンデマンド snapshot）後、クライアントの `scrollback_lines` 設定（小さい値や 0 を含む）に関係なく、継続出力が正しい行に書かれる。
- デーモンの復元状態プローブが、snapshot を再生する GUI タブと同じ scrollback 容量を使い、payload 内の広げるリサイズで両側のカーソルが同じように動く。
- snapshot のバイト配置、既存メッセージのエンコーディング、PROTOCOL_VERSION（3）を変えない。新旧の GUI・bridge・デーモンの混在組み合わせが引き続き動作する。

### 1.3 スコープ
**対象**:
- クライアント容量を報告する追加的な制御メッセージ（GUI → bridge → デーモン）
- bridge での転送と再接続時の再送
- デーモンでの接続単位の容量保持と、4 つの snapshot 組み立て箇所へのプローブ容量の受け渡し
- 容量上限（10,000）と既知の制限の文書化
- 小さいクライアント容量と広げるリサイズを組み合わせた回帰テスト

**対象外**:
- プローブの作業用端末は `scroll_region_scrollback_enabled = true` の既定値を常に使い、クライアントはこのフラグを設定から初期化する（`settings/mod.rs:148`）。この別種のプローブ／クライアント不一致は、本件（容量のみの修正）では扱わない。

## 2. ビジネス要件

### 2.1 ビジネス目標
- 復元後の継続出力が、クライアントの scrollback 設定に関係なく本来の行に書かれる。
- プロトコル互換性を保ったまま修正する。

### 2.2 対象ユーザー
| ユーザータイプ | 説明 |
|----------------|------|
| mux 利用者 | `scrollback_lines` を既定値以外（小さい値や 0 を含む）に設定し、mux の pane で snapshot 復元を行う利用者 |

### 2.3 期待される効果
- `scrollback_lines` が 0 や小さい値でも、リング一周後の復元で継続出力の行がずれない。
- 新旧混在の組み合わせでも接続が止まらない。

## 3. ユースケース

### 3.1 ユースケース一覧
| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | 小さい scrollback 設定での snapshot 復元後の継続出力 | mux 利用者 | 高 |

### 3.2 ユースケース詳細

#### UC01: 小さい scrollback 設定での snapshot 復元後の継続出力

**アクター**: mux 利用者

**事前条件**:
- `scrollback_lines` が 0 または小さい値に設定されている。

**基本フロー**:
1. mux の pane でリングが一周するまで出力し、その途中で画面を広げるリサイズを行う。
2. その pane の snapshot 復元（タブ切替 / reattach / 可視化による再開）を行う。
3. 続けて出力させる。

**代替フロー**:
- 新旧の GUI・bridge・デーモンが混在する場合、接続は動作し続けるが容量の修正は保証されず、従来の 10,000 行が使われる。

**事後条件**:
- 継続出力が、クライアント自身の容量で再生した場合と同じ行に書かれる。

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | 追加的なクライアント容量制御メッセージ | タブの scrollback 容量を u32 で運ぶ新しい MessageType を追加する | 高 |
| FR2 | GUI がタブ自身の容量を報告する | タブの `core.scrollback_capacity()` を最初の Welcome 受理時に送る | 高 |
| FR3 | bridge が容量を転送・再送する | 転送し、最後の値を保持し、再接続時に Attach の前に再送する | 高 |
| FR4 | デーモンでの接続単位の容量 | 接続ごとに容量を保持し、検証してから使う | 高 |
| FR5 | プローブ容量を 4 つの組み立て箇所へ渡す | `min(報告値, 10_000)` を 4 箇所すべてのプローブに渡す | 高 |
| FR6 | 10,000 超では上限で打ち切りダンプを保持する | 上限超過でもダンプブロックを付ける | 高 |
| FR7 | snapshot のバイト形状を変えない | 容量 10,000 のとき出力が現行とバイト一致する | 高 |
| FR8 | 既知の制限を文書化する | 上限超過と新旧混在の制限をコードと SPEC に書く | 中 |
| FR9 | 小さい容量と広げるリサイズの回帰テスト | ビルダー単位のテストを追加する | 高 |

### 4.2 機能詳細

#### FR1: 追加的なクライアント容量制御メッセージ

**説明**: クライアントからデーモンへの新しい MessageType を 1 つ追加する（`Upgrading` = 0x26 の次の未使用の値）。報告するタブの scrollback 容量を固定長 u32 の payload で運び、pane_id は 0 とする。既存の MessageType の値と payload エンコーディングは変えず、PROTOCOL_VERSION は 3 のままとする。デーモンは応答を返さない。旧デーモンは既存の未知フレーム経路（`MessageType::from_u8` → `None`）でこの型を捨て、接続を維持する。

**入力**:
- scrollback 容量: u32 - 報告するタブの scrollback 容量

**出力**:
- なし（デーモンは応答を返さない）

**ビジネスルール**:
- 既存の MessageType の値と payload エンコーディングを変えない。
- PROTOCOL_VERSION は 3 のまま。

#### FR2: GUI がタブ自身の容量を報告する

**説明**: GUI タブは、最新のグローバル設定 `scrollback_lines` ではなく、そのタブ自身の `core.scrollback_capacity()` を新しいメッセージで送る。送信経路は既存の `send_control`（タブ PTY → 必要に応じて SSH → bridge → デーモン。Linux は APC/OSC、Windows は `EMUX;<base64>` の平文）。タブは attach の最初に受理した Welcome で、その Welcome への応答として送る Attach・CreateWindow・RequestPaneSnapshot のいずれよりも前に送る。

**ビジネスルール**:
- 値はタブの `core.scrollback_capacity()` とする。
- 送信順は「容量 → Attach → （該当時）CreateWindow・RequestPaneSnapshot」とする。

#### FR3: bridge が容量を転送・再送する

**説明**: bridge は新しいメッセージをデコードしてデーモンへ転送する。受け取った最後の値を `last_attach` と同じ方法で保持する。Unix でのアップグレード告知後の自動再接続では、再送する Attach の前に保持した容量を送る。Windows は再接続しない（この挙動は変えない）。

#### FR4: デーモンでの接続単位の容量

**説明**: デーモンは報告された容量を、既存の接続単位の `visible_state` と同様に接続ごとに 1 つ保持する。セッション単位では保持せず、pane にも永続化せず、ホットアップグレードの引き継ぎでも持ち越さない。デーモンは何かを確保する前に、payload を固定長 u32 として検証する。

**バリデーション**:
| 項目 | ルール | エラーメッセージ |
|------|--------|------------------|
| 容量 payload | 固定長 u32 であること。確保より前に検証する | なし（不正な payload は無視する） |

**エラーケース**:
| エラー | 条件 | 対応 |
|--------|------|------|
| 不正な payload | payload が固定長 u32 でない | 無視する。接続の現在値は変えず、接続も閉じない |
| 未報告 | 接続から容量の報告がない（旧 GUI・旧 bridge・システム起点の経路） | 従来の 10,000 行を使う |

**ビジネスルール**:
- 明示的な 0 の報告は 0 を意味し、「未報告」とは区別する。

#### FR5: プローブ容量を 4 つの組み立て箇所へ渡す

**説明**: プローブ容量は `min(報告値, 10_000)`、未報告なら 10,000 とする。この値を、プローブの作業用 TerminalCore（`dump_block.rs` の `probe_replay_state`）まで、リング一周を考慮する 4 つの組み立て箇所すべてから渡す。
1. 可視の reattach: `collect_reattach_data` → `build_snapshot_bytes_for_ring`（`reattach.rs:273`）
2. オンデマンドの RequestPaneSnapshot: `build_shadow_parser_snapshot_for_ring`（`reattach.rs:103`、`handlers/mod.rs:529` から呼ばれる）
3. 可視化による再開: `resume_pane_with_permit` → `build_resume_snapshot_bytes_for_ring`（`output_target.rs:432`）。接続の `DeferredOutputQueue` を通って遅延した再開も含む
4. `evaluate_output_target` の再開分岐（`output_target.rs:270`）

**ビジネスルール**:
- どの箇所でも、使う値は snapshot の送信先の接続のものとする。

#### FR6: 10,000 超では上限で打ち切りダンプを保持する

**説明**: 報告された容量が 10,000 を超えるとき、プローブは 10,000 で動き、リング一周復元のダンプブロックは現在と同様に付ける。上限を超えたことだけを理由にダンプを落とさず、GUI タブ自身の容量も変えない。

#### FR7: snapshot のバイト形状を変えない

**説明**: snapshot payload の配置（EMSNAP2 エンベロープ、セグメント、修正前の委譲 payload、ダンプブロックの合成順）はそのままとする。snapshot にマーカーや新しいフィールドを追加しない。プローブ容量が 10,000（未報告の場合を含む）のとき、同じ入力に対する出力は現行の出力とバイト一致する。

#### FR8: 既知の制限を文書化する

**説明**: 次の 2 つの制限を、コード中の上限定数の横と、このフィーチャーの SPEC に記載する。
- (a) クライアント容量が 10,000 を超える場合、広げるリサイズ後のカーソル行の不一致が残る。
- (b) 新旧の GUI・bridge・デーモンが混在する組み合わせでは、接続は動作し続けるが容量の修正は保証されず、従来の 10,000 が使われる。

上限は負荷の増幅だけを抑える。ロック保持時間は変えない（後続課題 `3b5bbd839c74d67b`）。

#### FR9: 小さい容量と広げるリサイズの回帰テスト

**説明**: ビルダー単位のテストを追加する。リング一周、payload 内の広げるリサイズのセグメント（行のみ、行と列、最後のセグメントより大きい `current_dims`）、クライアント容量 C ∈ {0, 小さい非ゼロ値} を組み合わせる。snapshot はプローブ容量 C で組み立て、容量 C で作ったクライアント TerminalCore で再生する。カーソルの行・列と継続出力が書かれる行が、ダンプ前の委譲 payload と同じ継続出力を再生した容量 C のオラクル core と一致することを確認する。プローブを 10,000 行固定にすると少なくとも 1 ケースが失敗（red）し、修正後はすべて成功する。

## 5. 非機能要件

### 5.1 パフォーマンス要件
- NFR2: クライアントが報告した容量は信頼しない。プローブの作業用メモリと時間は 10,000 行の上限で抑え、検証は確保より前に行う。

### 5.2 セキュリティ要件
- 入力検証: NFR2 のとおり、報告された容量は信頼しない値として扱い、確保より前に固定長 u32 として検証する。

### 5.3 可用性要件
- NFR1: 応答を必要としないため、新旧のピアが停止することはない。

### 5.4 保守性要件
- NFR4 ログ出力: リリースログに出す必要がある新しい診断は warn 以上のレベルを使う（リリースビルドは debug と info を捨てる）。
- NFR5 フォーマット: クレート全体に `cargo fmt` を走らせない。このフィーチャーが触るファイルだけをフォーマットする。
- ドキュメント: FR8 の既知の制限をコードと SPEC に記載する。

### 5.5 互換性要件
- NFR1 プロトコル互換性: PROTOCOL_VERSION を上げず、既存のエンコーディングを変えない。新しい GUI と旧デーモン、旧 GUI と新しいデーモンの組み合わせは従来の 10,000 で動作し続ける。
- NFR3 プラットフォームと feature gate: Linux と Windows。デーモン・bridge・プロトコルの変更は CLI 専用ビルド（`--no-default-features`）でコンパイルできる。GUI 専用のコードは `gui` feature の下に置く。

## 6. UI/UX要件

### 6.1 画面設計要件
該当なし（UI や見た目の変更はない）。

### 6.2 画面遷移
該当なし。

### 6.3 レスポンシブ対応
該当なし。

## 7. データ要件

### 7.1 データモデル概要
永続化するデータはない。デーモンは容量を接続単位のメモリ上の値として保持する（FR4）。

### 7.2 データ項目
| エンティティ | 項目名 | 型 | 必須 | 説明 |
|--------------|--------|-----|------|------|
| 容量制御メッセージ | scrollback 容量 | u32 | ○ | 報告するタブの scrollback 容量。pane_id は 0 |
| デーモンの接続状態 | 報告された容量 | 未報告 / u32 | × | 未報告と明示的な 0 を区別する |
| bridge の状態 | 最後に受け取った容量 | u32 | × | 再接続時に Attach の前に再送する |

### 7.3 データ保持期間
| データ種別 | 保持期間 |
|------------|----------|
| デーモンの接続単位の容量 | 接続が続く間。pane に永続化せず、ホットアップグレードでも持ち越さない |
| bridge の最後の容量 | bridge プロセスが動いている間 |

## 8. 外部連携

### 8.1 連携システム
| システム名 | 連携方法 | データ |
|------------|----------|--------|
| GUI タブ → bridge → デーモン | 既存の `send_control` 経路（タブ PTY → 必要に応じて SSH → bridge → デーモン。Linux は APC/OSC、Windows は `EMUX;<base64>`） | 新しい容量制御メッセージ |

### 8.2 API仕様要件
FR1 のとおり。既存メッセージのエンコーディングと PROTOCOL_VERSION（3）は変えない。

## 9. 制約条件

### 9.1 技術的制約
- apt 進捗バー残骸の既知課題に関わるため、snapshot のバイト形状を変えない。
- PROTOCOL_VERSION は 3 のまま。追加するのは新しいメッセージ型だけとする。
- Linux と Windows を対象とし、デーモン・bridge・プロトコルの変更は CLI 専用ビルドでコンパイルできること。

### 9.2 ビジネス上の制約
- 該当なし。

### 9.3 スケジュール制約
- 該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mux-probe-scrollback-capacity/**`
- `test-docs/mux-probe-scrollback-capacity/**`

`feature-docs/mux-probe-scrollback-capacity/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mux-probe-scrollback-capacity/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mux-probe-scrollback-capacity/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/mux-probe-scrollback-capacity/` ディレクトリを生成しないが、宣言された `test-docs/mux-probe-scrollback-capacity/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題
| 課題 | 影響度 | 対応策 |
|------|--------|--------|
| クライアント容量が 10,000 を超える場合、広げるリサイズ後のカーソル行の不一致が残る | 中 | 既知の制限として文書化する（FR8） |
| 新旧混在の組み合わせでは容量の修正が保証されない | 中 | 従来の 10,000 で動作を継続し、既知の制限として文書化する（FR8、NFR1） |
| 上限はロック保持時間を変えない | 低 | 後続課題 `3b5bbd839c74d67b` に委ねる |

### 10.2 ビジネスリスク
| リスク | 発生確率 | 影響度 | 対応策 |
|--------|----------|--------|--------|
| 該当なし | - | - | - |

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC-1: `scrollback_lines` が 0 または別の小さい値で、payload 内に広げるリサイズを含むリング一周済み pane を、タブ切替・reattach・可視化による再開で復元したとき、継続出力がクライアント自身の容量で再生した場合と同じ行に書かれる（FR2、FR5、FR9）。
- [ ] AC-2: 新しい MessageType が MuxMessage フレーム、APC/OSC、EMUX 平文の各エンコーディングで往復する。既存のすべての MessageType の値と PROTOCOL_VERSION == 3 は変わらない（FR1、FR7）。
- [ ] AC-3: 最初に受理した Welcome で、GUI は容量メッセージを Attach・CreateWindow・RequestPaneSnapshot より前に送り、その値はタブ core の `scrollback_capacity()` と等しい（FR2）。
- [ ] AC-4: bridge は容量メッセージを転送する。Unix のアップグレード告知後の再接続では、再送する Attach の前に保持した容量を送る（FR3）。
- [ ] AC-5: デーモンのプローブ容量は次のとおり決まる。未報告 → 10,000、報告 0 → 0、報告 50 → 50、報告 10,000 → 10,000、報告 10,001 または u32::MAX → 10,000 でダンプブロックは残る、不正な payload → 値は変わらず接続は開いたまま。値は接続単位で、異なる報告をした 2 つの接続はそれぞれ自分の容量を持つ（FR4、FR5、FR6）。
- [ ] AC-6: 4 つの組み立て箇所すべてが要求元の接続のプローブ容量を使い、遅延出力キューを通る可視化による再開も同様である（FR5）。
- [ ] AC-7: プローブ容量 10,000 のとき、`build_snapshot_bytes_for_ring` と `build_resume_snapshot_bytes_for_ring` は同じ入力に対して現行実装とバイト一致する出力を生成する。既存の `wrap_restore_tests`、`dump_block` のテスト、apt 進捗バー関連の snapshot テストは、容量引数の追加以外は変更なしで成功する（FR7）。
- [ ] AC-8: 旧デーモンは新しいメッセージを受け取ると捨て、接続は動作し続ける。既存の codec の未知型テストのパターンでこれを確認する（NFR1）。
- [ ] AC-9: 既知の制限（10,000 超と新旧混在）が、コード中の上限の横と SPEC に記載されている（FR8）。
- [ ] AC-10: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`、mux_ipc クレートのテスト、`--no-default-features` の `cargo check` がすべて成功する（NFR3）。

### 11.2 KPI
| 指標 | 目標値 | 測定方法 |
|------|--------|----------|
| 該当なし | - | - |

## 12. テストシナリオ

### 12.1 テスト観点
- [ ] 正常系: TS-1（FR9、AC-1）小さいクライアント容量 C ∈ {0, 小さい非ゼロ値} と広げるリサイズ（行のみ、行と列、最後のセグメントより大きい `current_dims`）を組み合わせ、`reset_and_replay_segments` と `build_from_snapshot` の両方で再生し、継続出力の位置が容量 C のオラクル core と一致する。プローブ 10,000 固定で red になることを確認する。
- [ ] 正常系: TS-5（FR2、AC-3）最初に受理した Welcome での送信順が「容量 → Attach →（該当時）CreateWindow・RequestPaneSnapshot」で、値が 0 のような既定値以外のタブ core の `scrollback_capacity()` と等しい。
- [ ] 正常系: TS-6（FR3、AC-4）bridge が容量メッセージを `capture_if_attach` と同じ方法で保持し、再接続経路で保持した Attach の前に容量を書く。
- [ ] 正常系: TS-7（FR4、FR5、AC-5、AC-6）接続単位の値が `collect_reattach_data`、`handle_request_pane_snapshot`、`resume_pane_with_permit`（直接と遅延）、`evaluate_output_target` の再開分岐に届く。2 つの接続が独立した値を持つ。
- [ ] 異常系: TS-7 不正な payload は無視される。TS-4（FR1、AC-2、AC-8）未知型のフレームはストリームを閉じずに捨てられる。
- [ ] 境界値: TS-2（FR5、FR6、AC-5）未報告・0・小さい値・10,000・10,001・u32::MAX がそれぞれ 10,000・0・小さい値・10,000・10,000・10,000 になり、上限超過でもダンプブロックが付く。
- [ ] 互換性: TS-3（FR7、AC-7）プローブ容量 10,000 で既存フィクスチャに対する出力が現行実装と一致し、既存の `dump_block` のプローブ一致テストが容量を明示的に渡して成功する。TS-4 新しい型が `from_u8`、`from_frame_body`、`from_apc`、平文経路でデコードされ、u32 payload が往復し、値と PROTOCOL_VERSION が変わらない。
- [ ] 手動: TS-8（AC-1）リリースビルドで `scrollback_lines = 0` と小さい値のそれぞれについて、pane をリング一周まで埋め、出力中にウィンドウを広げ、タブ切替または reattach してから出力を続け、出力が期待する行に書かれる。

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| プローブ | デーモンが snapshot 組み立て時に作業用 TerminalCore で復元状態を求める処理（`dump_block.rs` の `probe_replay_state`） |
| プローブ容量 | プローブの作業用 TerminalCore の scrollback 容量。`min(報告値, 10_000)`、未報告なら 10,000 |
| リング一周 | pane の出力リングバッファが一周した状態 |
| 広げるリサイズ | payload の途中で画面の行数（または行数と列数）が増えるリサイズ |
| ダンプブロック | リング一周復元のために snapshot に付けるブロック |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A-1 修正方針（fix-approach）: 再生するタブ自身の容量を新しい追加的な制御メッセージでデーモンへ送り、プローブをその容量で動かす（pass_client_capacity）。クライアント側での復元はチケットが禁じる snapshot 形状の変更が必要になり、制限の文書化だけでは完了の定義の 1 項目目を満たせないため、採用しない。
- [x] A-2 プロトコル互換性（protocol-compat）: 新しいメッセージ型だけを追加し、PROTOCOL_VERSION は 3 のまま。未報告は従来の 10,000 とし、明示的な 0 と区別する。新旧混在の組み合わせは動作し続けるが修正は保証されない。ピアからの応答は不要（avoid_bump_preferred）。
- [x] A-3 大きい容量（large-capacity-cost）: u32 を確保より前に検証し、`probe_capacity = min(報告値, 10,000)` とする。上限を超えるとプローブは 10,000 で動き、ダンプは保持する。修正は 0..=10,000 で保証する。10,000 超の残る不一致は文書化した制限とし、ロック保持時間は後続課題 `3b5bbd839c74d67b` に委ねる（cap_with_fallback）。
- [x] A-4 デザインステップ（design-step）: UI や見た目の変更がないため省略する。

### 14.2 未確認・保留事項
- 未確認・保留事項はない。

以下は確認済みの回答以外の前提として記録する。
- A-5: タブの scrollback 容量は TerminalCore の構築時に決まる。TerminalCore には `scrollback_capacity()` があり setter はなく、別スレッドの再生も置き換え用の core を同じ容量で作る。そのため attach ごとに 1 回の報告と bridge 再接続時の再送で足り、後から設定を変えても影響するのは新しいタブだけである（出典: `crates/term_core/src/terminal_core.rs:402`、`src-tauri/src/tabs/replay.rs:303-325`）。
- A-6: 不正な容量 payload（ちょうど 1 つの u32 でないもの）は、接続の値を変えず、接続も閉じずに無視する。回答「確保より前に検証する」から導かれる（出典: 回答 large-capacity-cost から導出）。
- A-7: チケットの制約として、apt 進捗バー残骸の既知課題のため snapshot のバイト形状を変えない（出典: task_description 制約・前提）。
- A-8: 対象外: プローブの作業用 core は常に `scroll_region_scrollback_enabled = true` の既定値を使い、クライアントはこのフラグを設定から初期化する（`settings/mod.rs:148`）。これは別種のプローブ／クライアント不一致の可能性で、容量のみの本修正では扱わない（出典: `dump_block.rs:109`、`settings/mod.rs:142-148`）。

## 15. 参考資料

- `src-tauri/src/mux/snapshot_bytes/dump_block.rs:109`: プローブの 10,000 行固定容量
- `src-tauri/src/tabs/replay.rs:303-325`: クライアントの再生用端末の構築
- `crates/term_core/src/terminal_core.rs:402`: `scrollback_capacity()`
- `settings/mod.rs:142-148`: `scroll_region_scrollback_enabled` の初期化
