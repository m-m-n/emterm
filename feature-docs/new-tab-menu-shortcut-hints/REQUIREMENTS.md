---
title: "new-tab-menu-shortcut-hints"
created_date: 2026-10-03
status: draft
---

# new-tab-menu-shortcut-hints - 要件定義書

## 1. 概要

### 1.1 背景
タブバーの + ボタンで開く新規タブ選択メニュー（以下「チューザー」）の行には、その行の対象を直接開くキーボードショートカットが表示されていない。ショートカットを知るには設定を開く必要がある。

### 1.2 目的
チューザーの行の横に、その行の対象を直接開くキーボードショートカットを表示する。ユーザーは設定を開かずにショートカットを知り、使えるようになる。

### 1.3 スコープ
対象:
- + ボタンで開くチューザー（`ProfileSelectorState.include_global = true`）の「Global Settings」行への `keybinds.new_tab_global` のラベル表示
- 同チューザーで `new_tab` キーバインドが開くプロファイルの行への `keybinds.new_tab` のラベル表示

対象外:
- Ctrl+Shift+P のプロファイルセレクター（`include_global = false`）へのラベル表示
- 上記以外のプロファイル行・tmux 行へのラベル表示
- デザイントークンの追加

## 2. ビジネス要件

### 2.1 ビジネス目標
タブバーの + ボタンで開くチューザーで、行の横にその行の対象を直接開くキーボードショートカットを表示する。ユーザーは設定を開かずにショートカットを知り、使えるようになる。

### 2.2 対象ユーザー
| ユーザータイプ | 説明 |
|----------------|------|
| eMterm のユーザー | + ボタンのチューザーから新規タブを開くユーザー |

### 2.3 期待される効果
- 設定を開かずに、Global Settings とデフォルトプロファイルを直接開くショートカットを知ることができる
- 表示されたショートカットで、その行の対象を直接開くことができる

## 3. ユースケース

### 3.1 ユースケース一覧
| ID | ユースケース名 | アクター |
|----|----------------|----------|
| UC01 | チューザーでショートカットを確認する | ユーザー |
| UC02 | キーバインド変更後にショートカットを確認する | ユーザー |

### 3.2 ユースケース詳細

#### UC01: チューザーでショートカットを確認する

**アクター**: ユーザー

**事前条件**:
- eMterm の GUI が起動している

**基本フロー**:
1. ユーザーがタブバーの + ボタンを押す
2. チューザーが開く
3. 「Global Settings」行に `keybinds.new_tab_global` のチョード（既定 Ctrl+Shift+G）のラベルが表示される
4. `settings.profiles` で最初に `is_default = true` のプロファイルの行に `keybinds.new_tab` のチョード（既定 Ctrl+Shift+T）のラベルが表示される

**代替フロー**:
- デフォルトのプロファイルが無い場合、「Global Settings」行だけが `new_tab_global` のラベルを表示し、どのプロファイル行もラベルを表示しない
- チョードが上位の優先度のアクションに奪われている場合、そのラベルは表示しない（FR6）

**事後条件**:
- ラベルのある行は、ラベルのショートカットで直接開ける行である

#### UC02: キーバインド変更後にショートカットを確認する

**アクター**: ユーザー

**事前条件**:
- `keybinds.new_tab_global` / `keybinds.new_tab` を変更した設定が適用されている

**基本フロー**:
1. ユーザーがタブバーの + ボタンを押す
2. チューザーが開き、「Global Settings」行とデフォルトプロファイル行に変更後のチョードのラベルが表示される

**事後条件**:
- 表示されたラベルは現在のキーバインド設定と一致する

## 4. 機能要件

### 4.1 機能一覧
| ID | 機能名 | 説明 |
|----|--------|------|
| FR1 | Global Settings 行に new_tab_global のショートカットを表示 | チューザーの「Global Settings」行に `keybinds.new_tab_global` の解決済みチョードのラベルを表示する |
| FR2 | デフォルトプロファイル行に new_tab のショートカットを表示 | `new_tab` キーバインドが開くプロファイルの行に `keybinds.new_tab` の解決済みチョードのラベルを表示する |
| FR3 | 直接のショートカットが無い行はラベルを表示しない | FR2 以外のプロファイル行と tmux 行はラベルを表示しない |
| FR4 | ラベルは現在のキーバインド設定に追従する | チューザーの描画ごとに現在の解決済みキーバインド表からラベルを導出する |
| FR5 | ラベル文字列は解決済みチョードの正規表記 | 解決済みの `Chord` から Ctrl, Shift, Alt の順と主キー名を `+` で連結する |
| FR6 | 奪われたチョードは表示しない | 上位の優先度のアクションに奪われたチョードのラベルは表示しない |
| FR7 | ラベルの配置 | ラベルは行の右パディングに右寄せし、シェルパスはラベルの手前で切り詰める |
| FR8 | チューザーのみ | ラベルは新規タブのチューザーモードだけに表示する |

### 4.2 機能詳細

#### FR1: Global Settings 行に new_tab_global のショートカットを表示

**説明**: 新規タブのチューザー（+ ボタン、`ProfileSelectorState.include_global = true`）の「Global Settings」行に、`keybinds.new_tab_global` に対して現在解決されているチョード（既定 Ctrl+Shift+G）のラベルを表示する。

**ビジネスルール**:
- この行が表示するショートカットは `new_tab_global` のものだけとする。デフォルトのプロファイルが無い場合も同じとする

#### FR2: デフォルトプロファイル行に new_tab のショートカットを表示

**説明**: 新規タブのチューザーで、`new_tab` キーバインドが開くプロファイルの行に、`keybinds.new_tab` に対して現在解決されているチョード（既定 Ctrl+Shift+T）のラベルを表示する。

**ビジネスルール**:
- 対象のプロファイルは、`settings.profiles` で最初に `is_default = true` のプロファイルとする
- デフォルトのプロファイルが無い場合、どのプロファイル行もこのラベルを表示しない

#### FR3: 直接のショートカットが無い行はラベルを表示しない

**説明**: FR2 のプロファイル以外のプロファイル行は、ショートカットのラベルを表示しない。tmux 行もショートカットのラベルを表示しない。

**ビジネスルール**:
- 2 つ目以降の `is_default` プロファイルの行もラベルを表示しない

#### FR4: ラベルは現在のキーバインド設定に追従する

**説明**: ラベルは、チューザーを描画するたびに、現在の解決済みキーバインド表（`App.keybinds`）から導出する。

**ビジネスルール**:
- `keybinds.new_tab_global` / `keybinds.new_tab` を変更した設定を適用した後、次にチューザーを開いたときは新しいチョードを表示する

#### FR5: ラベル文字列は解決済みチョードの正規表記

**説明**: ラベル文字列は、設定の生の文字列ではなく、解決済みの `Chord` から組み立てる。

**ビジネスルール**:
- 書式は、有効な修飾キーを Ctrl, Shift, Alt の順に並べ、続けて主キー名を置き、`+` で連結する（例: `Ctrl+Shift+G`）
- 英字は大文字とする
- 主キー名は `parse_chord` が受け付けるトークンとし、ラベルを `parse_chord` で解析すると元と同じ `Chord` になる

**エラーケース**:
| エラー | 条件 | 対応 |
|--------|------|------|
| 解析できないキーバインド指定 | 指定が解析できず、組み込みの既定チョードにフォールバックして解決された | ラベルはその既定チョードを表示する |

#### FR6: 奪われたチョードは表示しない

**説明**: あるアクションのチョードが、より優先度の高いアクションに奪われていて、押してもその行を開けない場合、そのラベルを表示しない。

**ビジネスルール**:
- 実行時の照合優先度は copy, paste, profile_selector, new_tab_global, new_tab の順（以下続く）とする。これは `KeybindTable::collisions` が (winner, loser) として報告する順序である
- `new_tab_global` のラベルは、そのチョードが copy, paste, profile_selector のいずれかと等しいとき表示しない
- `new_tab` のラベルは、そのチョードが copy, paste, profile_selector, new_tab_global のいずれかと等しいとき表示しない
- ラベルを表示しない行は、ショートカットが無い行と同じように描画する

#### FR7: ラベルの配置

**説明**: ラベルは行の右パディング（`ROW_PAD_X`）に右寄せする。

**ビジネスルール**:
- ラベルのある行では、シェルパスをラベルの手前で終わるように切り詰め、両者の間に `ROW_INNER_GAP` を空ける
- ラベルは名前、Default バッジ、シェルパスのいずれとも重ならない

#### FR8: チューザーのみ

**説明**: ラベルは新規タブのチューザーモードだけに表示する。

**ビジネスルール**:
- Ctrl+Shift+P のプロファイルセレクター（`include_global = false`）は現在の行のままとし、ショートカットのラベルを表示しない

## 5. 非機能要件

### 5.1 非機能要件一覧
| ID | 要件名 | 内容 |
|----|--------|------|
| NFR1 | 既存デザイントークン | ラベルは既存のトークンを使う。フォントはシェルパスと同じ label-small 12px とする。色は通常行で `md3::on_surface_variant()`、ハイライト行ではシェルパスが使う `on_secondary_container` 由来の色とする。`doc/UI-DESIGN-GUIDELINES.yaml`、`md3.rs`、`dialog/tokens.rs`、`styles.css` にトークンを追加しない |
| NFR2 | プラットフォーム間の一致 | Linux と Windows で同じラベル文字列を表示する。ラベル文字列は UI ロケールに依存しない |
| NFR3 | フィーチャーゲートの保全 | 変更は GUI ゲート内のモジュールに収める。`--no-default-features`（CLI のみ）ビルドは引き続きコンパイルできる |
| NFR4 | テスト可能な判定の中核 | 次の 2 つのロジックを、egui コンテキスト無しで単体テストできる純粋関数とする。(a) どの行にどのラベルを付けるかの選択（奪われたチョードの規則を含む）。(b) `Chord` のラベル文字列への書式化 |

### 5.2 互換性要件
- 対応プラットフォーム: Linux、Windows（NFR2）
- ビルド構成: `--no-default-features` ビルドがコンパイルできる（NFR3）

## 6. UI/UX要件

### 6.1 画面設計要件
- ラベルは行の右パディング（`ROW_PAD_X`）に右寄せする（FR7）
- ラベルのある行では、シェルパスをラベルの手前で切り詰め、間に `ROW_INNER_GAP` を空ける（FR7）
- ラベルは名前、Default バッジ、シェルパスと重ならない（FR7）
- フォントは label-small 12px、色は通常行で `md3::on_surface_variant()`、ハイライト行でシェルパスと同じ `on_secondary_container` 由来の色とする（NFR1）
- デザインステップはスキップした。既存の egui ダイアログの行に、既存トークン（label-small 12px、on-surface-variant）を使う補助テキストのラベルを 1 つ追加する変更で、新しいコンポーネントやトークンは無い

## 7. データ要件

該当なし。

## 8. 外部連携

該当なし。

## 9. 制約条件

### 9.1 技術的制約
- 変更は GUI ゲート内のモジュールに収める（NFR3）
- デザイントークンを追加しない（NFR1）
- 行へのラベル割り当てと `Chord` の書式化は、egui コンテキスト無しで単体テストできる純粋関数とする（NFR4）

### 9.2 ビジネス上の制約
該当なし。

### 9.3 スケジュール制約
該当なし。

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/new-tab-menu-shortcut-hints/**`
- `test-docs/new-tab-menu-shortcut-hints/**`

`feature-docs/new-tab-menu-shortcut-hints/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/new-tab-menu-shortcut-hints/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/new-tab-menu-shortcut-hints/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/new-tab-menu-shortcut-hints/` ディレクトリを生成しないが、宣言された `test-docs/new-tab-menu-shortcut-hints/**` は依然として正しい。

## 10. 想定される課題とリスク

該当なし。

## 11. 成功基準

### 11.1 受け入れ基準
- [ ] AC-1: 既定のキーバインドで、チューザーの「Global Settings」行に `Ctrl+Shift+G` が表示される（FR1, FR5）
- [ ] AC-2: 既定のキーバインドで、デフォルトに設定したプロファイルの行に `Ctrl+Shift+T` が表示される（FR2, FR5）
- [ ] AC-3: `keybinds.new_tab_global` と `keybinds.new_tab` を変更した設定を適用すると、次のチューザーで「Global Settings」行とデフォルトプロファイル行に新しいチョードが表示される（FR4）
- [ ] AC-4: デフォルトでないプロファイル行、2 つ目以降の `is_default` プロファイル行、tmux 行はショートカットのラベルを表示しない。デフォルトのプロファイルが無い場合、「Global Settings」行は `new_tab_global` のラベルだけを表示し、どのプロファイル行もラベルを表示しない（FR1, FR2, FR3）
- [ ] AC-5: 小文字で書いた指定（例: `ctrl+shift+g`）は `Ctrl+Shift+G` と表示される。解析できない指定は、フォールバック先の既定チョードとして表示される。すべてのラベルは `parse_chord` で解析すると描画元のチョードに戻る（FR5）
- [ ] AC-6: `new_tab_global` のチョードが copy, paste, profile_selector のいずれかと等しいとき、ラベルは表示されない。`new_tab` のチョードが copy, paste, profile_selector, new_tab_global のいずれかと等しいとき、ラベルは表示されない。`new_tab` = `new_tab_global` の場合も「Global Settings」行はラベルを表示する（FR6）
- [ ] AC-7: Ctrl+Shift+P のプロファイルセレクターはショートカットのラベルを表示しない（FR8）
- [ ] AC-8: ラベルは行の右端に右寄せされる。長いシェルパスを持つラベル付きの行では、シェルパスがラベルの手前で切り詰められ、ラベルと重ならない（目視による手動確認）（FR7, NFR1）

### 11.2 KPI
該当なし。

## 12. テストシナリオ

### 12.1 テスト観点
| ID | 種別 | 配置 | 内容 | 対応 AC |
|----|------|------|------|---------|
| TS-1 | 単体 | `src-tauri/src/ui/keybinds/tests.rs` | `KeybindTable::default().new_tab_global` を書式化すると `Ctrl+Shift+G`、`new_tab` は `Ctrl+Shift+T` になる | AC-1, AC-2 |
| TS-2 | 単体 | `src-tauri/src/ui/keybinds/tests.rs` | 往復変換: 既定の表のすべてのチョードと、代表的な名前付きキー（PageDown, ArrowUp, Plus, F11, 数字, Comma）について `parse_chord(format(chord)) == Some(chord)` が成り立つ | AC-5 |
| TS-3 | 単体 | `src-tauri/src/ui/keybinds/tests.rs` | 小文字の指定 `ctrl+shift+g` から作った表は `Ctrl+Shift+G` と書式化される。解析できない `new_tab` 指定から作った表は `new_tab` を `Ctrl+Shift+T` と書式化する。Alt は Shift の後に置かれる（例: `Ctrl+Shift+Alt+T`） | AC-5 |
| TS-4 | 単体 | `src-tauri/src/app/tests/chooser.rs` または `src-tauri/src/ui/profile_selector.rs` のテスト | プロファイル [A(default), B] のチューザーモードでのラベル割り当て: Global 行は `new_tab_global` のラベル、A は `new_tab` のラベル、B はラベル無し。tmux エントリがある場合、tmux 行はラベル無し | AC-1, AC-2, AC-4 |
| TS-5 | 単体 | TS-4 と同じ | デフォルトのプロファイルが無い場合、ラベルを持つのは Global 行だけで、それは `new_tab_global` のラベルである。プロファイル [A(default), B(default)] では、A だけが `new_tab` のラベルを持つ | AC-4 |
| TS-6 | 単体 | TS-4 と同じ | 奪われたチョード: `new_tab_global` = profile_selector のチョードのとき、Global 行はラベル無し。`new_tab` = copy のチョードのとき、デフォルト行はラベル無し。`new_tab` = `new_tab_global` のとき、デフォルト行はラベル無しで、Global 行はラベルを保つ | AC-6 |
| TS-7 | 単体 | TS-4 と同じ | セレクターモード（`open_profile_selector`、`include_global = false`）では、デフォルトのプロファイルがあってもラベルを割り当てない | AC-7 |
| TS-8 | 単体 | `src-tauri/src/app/tests/chooser.rs` | `keybinds.new_tab_global = 'Ctrl+Alt+G'`、`keybinds.new_tab = 'Ctrl+Alt+N'` の設定を適用し、チューザーを開き直すと、Global 行に `Ctrl+Alt+G`、デフォルト行に `Ctrl+Alt+N` が割り当てられる | AC-3 |
| TS-9 | 手動 | リリースビルド、ユーザー | + のチューザーを開き、次を目視確認する: ラベルが右寄せされている、長いシェルパスがラベルの手前で切り詰められる、通常行とハイライト行でラベルの色が正しい、日本語と英語のロケールで同じラベル文字列が表示される | AC-8 |

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| チューザー | タブバーの + ボタンで開く新規タブ選択メニュー（`ProfileSelectorState.include_global = true`） |
| プロファイルセレクター | Ctrl+Shift+P で開くプロファイル選択（`include_global = false`） |
| 解決済みチョード | 設定のキーバインド指定を解決した結果の `Chord`。解析できない指定は組み込みの既定チョードに解決される |
| デフォルトプロファイル | `settings.profiles` で最初に `is_default = true` のプロファイル。`new_tab` キーバインドが開く |
| 奪われたチョード | より優先度の高いアクションと同じチョードで、押してもそのアクションが実行されないもの |

## 14. 確認事項

### 14.1 確認済み事項

- [x] A1 `new_tab` ラベルを付けるプロファイル: 最初の `is_default` プロファイル（Ctrl+Shift+T が開くもの）だけに付ける。Default バッジはすべての `is_default` プロファイルに残す。ラベルはショートカットが実際に開くプロファイルを示す（`tab_lifecycle.rs:74-82`）
- [x] A2 デフォルトでないプロファイル行と tmux 行: ラベルを付けない。これらを直接開くキーバインドは無い。タスクが名指ししているのは「Global Settings」行とデフォルト行だけである
- [x] A3 ラベルの算出時点: チューザーの描画時に `App.keybinds` から算出し、キャッシュしない。設定の適用は開いているチューザーも閉じる（`font_settings.rs:533-551`）。これによりラベルは現在の設定に追従する
- [x] A4 デフォルトのプロファイルが無い場合の「Global Settings」行: `new_tab_global` のショートカットだけを表示する。この場合 `new_tab` も Global Settings を開くが、表示しない（batch 回答 global_only、質問 `requirement.no-default-global-row`）
- [x] A5 ラベルを表示する画面: + の新規タブのチューザーだけに表示し、Ctrl+Shift+P のプロファイルセレクターには表示しない（batch 回答 chooser_only、質問 `requirement.profile-selector-mode-scope`）
- [x] A6 ラベルの配置: 行の右端に右寄せし、シェルパスはその手前で切り詰める（batch 回答 row_right_edge、質問 `requirement.label-placement`）
- [x] A7 ラベルの書式: 解決済みチョードの正規表記（Ctrl, Shift, Alt の順、続けて主キー、`+` 連結）。フォールバックしたチョードは解決結果のとおり表示する（batch 回答 canonical_from_resolved、質問 `requirement.label-format`）
- [x] A8 奪われたチョード: 上位の優先度のアクション（copy, paste, profile_selector、`new_tab` については new_tab_global も）に奪われたチョードは表示しない（batch 回答 hide_when_shadowed、質問 `requirement.shadowed-chord`）

### 14.2 未確認・保留事項
なし。

## 15. 参考資料

- SPEC.md: `feature-docs/new-tab-menu-shortcut-hints/SPEC.md`
