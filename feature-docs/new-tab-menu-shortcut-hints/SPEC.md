# Feature: new-tab-menu-shortcut-hints

## Overview

タブバーの + ボタンで開く新規タブのチューザー（`ProfileSelectorState.include_global = true`）で、行の横にその行の対象を直接開くキーボードショートカットのラベルを表示する。対象は「Global Settings」行（`keybinds.new_tab_global`）と、`new_tab` キーバインドが開くデフォルトプロファイルの行（`keybinds.new_tab`）である。要件の詳細は `feature-docs/new-tab-menu-shortcut-hints/REQUIREMENTS.md` を参照する。

## Objectives

- チューザーの行の横に、その行の対象を直接開くキーボードショートカットを表示する
- ユーザーが設定を開かずにショートカットを知り、使えるようにする

## User Stories

### US1: 直接開くショートカットを知る
ユーザーとして、チューザーで「Global Settings」行とデフォルトプロファイル行のショートカットを見たい。設定を開かずにショートカットを知り、使えるようにするため。

**Acceptance Criteria:**
- [ ] AC-1: 既定のキーバインドで、チューザーの「Global Settings」行に `Ctrl+Shift+G` が表示される（FR1, FR5）
- [ ] AC-2: 既定のキーバインドで、デフォルトに設定したプロファイルの行に `Ctrl+Shift+T` が表示される（FR2, FR5）
- [ ] AC-4: デフォルトでないプロファイル行、2 つ目以降の `is_default` プロファイル行、tmux 行はショートカットのラベルを表示しない。デフォルトのプロファイルが無い場合、「Global Settings」行は `new_tab_global` のラベルだけを表示し、どのプロファイル行もラベルを表示しない（FR1, FR2, FR3）
- [ ] AC-5: 小文字で書いた指定（例: `ctrl+shift+g`）は `Ctrl+Shift+G` と表示される。解析できない指定は、フォールバック先の既定チョードとして表示される。すべてのラベルは `parse_chord` で解析すると描画元のチョードに戻る（FR5）

### US2: 変更したキーバインドがラベルに反映される
ユーザーとして、キーバインドを変更した後、チューザーに変更後のショートカットが表示されてほしい。

**Acceptance Criteria:**
- [ ] AC-3: `keybinds.new_tab_global` と `keybinds.new_tab` を変更した設定を適用すると、次のチューザーで「Global Settings」行とデフォルトプロファイル行に新しいチョードが表示される（FR4）

### US3: 押しても開けないショートカットは表示されない
ユーザーとして、押してもその行を開けないショートカットは表示されないでほしい。

**Acceptance Criteria:**
- [ ] AC-6: `new_tab_global` のチョードが copy, paste, profile_selector のいずれかと等しいとき、ラベルは表示されない。`new_tab` のチョードが copy, paste, profile_selector, new_tab_global のいずれかと等しいとき、ラベルは表示されない。`new_tab` = `new_tab_global` の場合も「Global Settings」行はラベルを表示する（FR6）

### US4: ラベルが行の他の表示と重ならない
ユーザーとして、ラベルが行の名前やシェルパスと重ならずに表示されてほしい。

**Acceptance Criteria:**
- [ ] AC-8: ラベルは行の右端に右寄せされる。長いシェルパスを持つラベル付きの行では、シェルパスがラベルの手前で切り詰められ、ラベルと重ならない（目視による手動確認）（FR7, NFR1）

### US5: プロファイルセレクターは変わらない
ユーザーとして、Ctrl+Shift+P のプロファイルセレクターは現在の表示のままであってほしい。

**Acceptance Criteria:**
- [ ] AC-7: Ctrl+Shift+P のプロファイルセレクターはショートカットのラベルを表示しない（FR8）

## Technical Requirements

### Functional Requirements
- **FR1:** Global Settings 行に new_tab_global のショートカットを表示する。新規タブのチューザー（+ ボタン、`ProfileSelectorState.include_global = true`）の「Global Settings」行は、`keybinds.new_tab_global` に対して現在解決されているチョード（既定 Ctrl+Shift+G）のラベルを表示する。この行が表示するショートカットは、デフォルトのプロファイルが無い場合も含め、これだけとする。
- **FR2:** デフォルトプロファイル行に new_tab のショートカットを表示する。新規タブのチューザーで、`new_tab` キーバインドが開くプロファイルの行は、`keybinds.new_tab` に対して現在解決されているチョード（既定 Ctrl+Shift+T）のラベルを表示する。そのプロファイルは `settings.profiles` で最初に `is_default = true` のプロファイルとする。デフォルトのプロファイルが無い場合、どのプロファイル行もこのラベルを表示しない。
- **FR3:** 直接のショートカットが無い行はラベルを表示しない。FR2 のプロファイル以外のプロファイル行はショートカットのラベルを表示しない。2 つ目以降の `is_default` プロファイルもこれに含む。tmux 行もショートカットのラベルを表示しない。
- **FR4:** ラベルは現在のキーバインド設定に追従する。ラベルはチューザーを描画するたびに、現在の解決済みキーバインド表（`App.keybinds`）から導出する。`keybinds.new_tab_global` / `keybinds.new_tab` を変更した設定を適用した後、次にチューザーを開いたときは新しいチョードを表示する。
- **FR5:** ラベル文字列は解決済みチョードの正規表記とする。ラベル文字列は設定の生の文字列ではなく、解決済みの `Chord` から組み立てる。書式は、有効な修飾キーを Ctrl, Shift, Alt の順に並べ、続けて主キー名を置き、`+` で連結する（例: `Ctrl+Shift+G`）。英字は大文字とする。主キー名は `parse_chord` が受け付けるトークンとし、ラベルを解析すると同じ `Chord` になる。指定が解析できず、組み込みの既定チョードにフォールバックして解決された場合、ラベルはその既定チョードを表示する。
- **FR6:** 奪われたチョードは表示しない。あるアクションのチョードがより優先度の高いアクションに奪われていて、押してもその行を開けない場合、そのラベルを表示しない。実行時の照合優先度は copy, paste, profile_selector, new_tab_global, new_tab の順（以下続く）で、`KeybindTable::collisions` が (winner, loser) として報告する順序である。`new_tab_global` のラベルは、そのチョードが copy, paste, profile_selector のいずれかと等しいとき表示しない。`new_tab` のラベルは、そのチョードが copy, paste, profile_selector, new_tab_global のいずれかと等しいとき表示しない。ラベルを表示しない行は、ショートカットが無い行と同じように描画する。
- **FR7:** ラベルの配置。ラベルは行の右パディング（`ROW_PAD_X`）に右寄せする。ラベルのある行では、シェルパスをラベルの手前で終わるように切り詰め、両者の間に `ROW_INNER_GAP` を空ける。ラベルは名前、Default バッジ、シェルパスのいずれとも重ならない。
- **FR8:** チューザーのみ。ラベルは新規タブのチューザーモードだけに表示する。Ctrl+Shift+P のプロファイルセレクター（`include_global = false`）は現在の行のままとし、ショートカットのラベルを表示しない。

### Non-Functional Requirements
- **NFR1 - 既存デザイントークン:** ラベルは既存のトークンを使う。フォントはシェルパスと同じ label-small 12px とする。色は通常行で `md3::on_surface_variant()`、ハイライト行ではシェルパスが使う `on_secondary_container` 由来の色とする。`doc/UI-DESIGN-GUIDELINES.yaml`、`md3.rs`、`dialog/tokens.rs`、`styles.css` にトークンを追加しない。
- **NFR2 - プラットフォーム間の一致:** Linux と Windows で同じラベル文字列を表示する。ラベル文字列は UI ロケールに依存しない。
- **NFR3 - フィーチャーゲートの保全:** 変更は GUI ゲート内のモジュールに収める。`--no-default-features`（CLI のみ）ビルドは引き続きコンパイルできる。
- **NFR4 - テスト可能な判定の中核:** 次の 2 つのロジックを、egui コンテキスト無しで単体テストできる純粋関数とする。(a) どの行にどのラベルを付けるかの選択（FR6 の規則を含む）。(b) `Chord` のラベル文字列への書式化。

## Implementation Approach

### Architecture

**Component Diagram:**
```
App.keybinds（解決済みキーバインド表 KeybindTable）
   │
   ├─> Chord の書式化（純粋関数, NFR4 (b), FR5）
   │
   └─> 行ごとのラベル割り当て（純粋関数, NFR4 (a), FR1/FR2/FR3/FR6/FR8）
          入力: チューザーのモード（include_global）、settings.profiles、tmux 行、解決済みキーバインド表
          出力: 各行のラベル（ラベル無しを含む）
   │
   ▼
チューザーの描画（egui、GUI ゲート内, NFR3）
   ラベルを ROW_PAD_X に右寄せし、シェルパスを ROW_INNER_GAP を空けて切り詰める（FR7, NFR1）
```

### Data Flow

```
チューザーの描画 → App.keybinds を読む → 行ごとのラベル割り当て → Chord の書式化 → ラベルを描画
```

- ラベルはチューザーの描画時に `App.keybinds` から算出し、キャッシュしない（FR4、A3）
- 設定の適用は開いているチューザーを閉じる（`font_settings.rs:533-551`、A3）

### API Design

該当なし。

### Database Schema

該当なし。

### Dependencies

**Internal Dependencies:**
- `KeybindTable`（`App.keybinds`）: `new_tab_global` / `new_tab` / copy / paste / profile_selector の解決済みチョードを提供する
- `KeybindTable::collisions`: 実行時の照合優先度の順序（FR6）
- `parse_chord`: ラベル文字列の主キー名が受け付けられるトークンであること、ラベルの往復変換（FR5）
- `ProfileSelectorState.include_global`: チューザーモードとプロファイルセレクターモードの区別（FR8）
- `settings.profiles` の `is_default`: `new_tab` が開くプロファイルの特定（FR2、`tab_lifecycle.rs:74-82`）
- `md3::on_surface_variant()` とシェルパスのハイライト行の色: ラベルの色（NFR1）

**External Dependencies:**
- 該当なし

### File Structure

フィーチャー固有の変更ファイルは create-plan で導出する。テストシナリオが指定するテストの配置は次のとおり。

```
src-tauri/src/
├── ui/
│   ├── keybinds/
│   │   └── tests.rs           # TS-1, TS-2, TS-3
│   └── profile_selector.rs    # TS-4〜TS-7 の配置候補
└── app/
    └── tests/
        └── chooser.rs         # TS-4〜TS-7 の配置候補, TS-8
```

## Declared Change Set

このセクションは手書きの一覧の代わりに create-plan での導出を述べる。上記のフィーチャー固有のパスは、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

この SPEC は、フィーチャー固有のパスに加えて、ワークフローが生成する次の 2 つのエントリを既定で宣言する。

- `feature-docs/new-tab-menu-shortcut-hints/**`
- `test-docs/new-tab-menu-shortcut-hints/**`

`feature-docs/new-tab-menu-shortcut-hints/**` は `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物を含む。これらは各フェーズドキュメントと `references/phase-state.md` が生成・所有する。このセクションはそれらを引用するだけで、ルールは再掲しない。

`test-docs/new-tab-menu-shortcut-hints/**` はタスクごとのテスト記録 `test-docs/new-tab-menu-shortcut-hints/{T}.tests.yaml` を含む。これは `implement-phase.md` が生成・所有する。このセクションはそれを引用するだけで、ルールは再掲しない。

この 2 つの既定エントリは、SPEC の作成者が明示的に除外しない限り宣言に含まれる。記載が無いことを除外とはみなさない。除外は意図的で明示的な絞り込みである。

この宣言はスーパーセットの主張である。検証時に観測される実際の変更集合は、宣言された集合と等しい必要はなく、宣言された集合に含まれる（CONTAINED IN）必要がある。implement タスクを 1 つも生成しないフィーチャーは `test-docs/new-tab-menu-shortcut-hints/` ディレクトリを生成しないが、その場合も宣言された `test-docs/new-tab-menu-shortcut-hints/**` は正しい。宣言されたパスが生成されないことは違反ではない。

## Test Scenarios

### Unit Tests
- [ ] TS-1（`src-tauri/src/ui/keybinds/tests.rs`、AC-1, AC-2）: `KeybindTable::default().new_tab_global` の書式化 - `Ctrl+Shift+G` になる。`new_tab` は `Ctrl+Shift+T` になる
- [ ] TS-2（`src-tauri/src/ui/keybinds/tests.rs`、AC-5）: 往復変換 - 既定の表のすべてのチョードと、代表的な名前付きキー（PageDown, ArrowUp, Plus, F11, 数字, Comma）について `parse_chord(format(chord)) == Some(chord)` が成り立つ
- [ ] TS-3（`src-tauri/src/ui/keybinds/tests.rs`、AC-5）: 指定の正規化とフォールバック - 小文字の指定 `ctrl+shift+g` から作った表は `Ctrl+Shift+G` と書式化される。解析できない `new_tab` 指定から作った表は `new_tab` を `Ctrl+Shift+T` と書式化する。Alt は Shift の後に置かれる（例: `Ctrl+Shift+Alt+T`）
- [ ] TS-4（`src-tauri/src/app/tests/chooser.rs` または `src-tauri/src/ui/profile_selector.rs` のテスト、AC-1, AC-2, AC-4）: プロファイル [A(default), B] のチューザーモードでのラベル割り当て - Global 行は `new_tab_global` のラベル、A は `new_tab` のラベル、B はラベル無し。tmux エントリがある場合、tmux 行はラベル無し
- [ ] TS-5（TS-4 と同じ配置、AC-4）: デフォルトのプロファイルが無い場合と複数ある場合 - デフォルトのプロファイルが無い場合、ラベルを持つのは Global 行だけで、それは `new_tab_global` のラベルである。プロファイル [A(default), B(default)] では、A だけが `new_tab` のラベルを持つ
- [ ] TS-6（TS-4 と同じ配置、AC-6）: 奪われたチョード - `new_tab_global` = profile_selector のチョードのとき、Global 行はラベル無し。`new_tab` = copy のチョードのとき、デフォルト行はラベル無し。`new_tab` = `new_tab_global` のとき、デフォルト行はラベル無しで、Global 行はラベルを保つ
- [ ] TS-7（TS-4 と同じ配置、AC-7）: セレクターモード - `open_profile_selector`（`include_global = false`）では、デフォルトのプロファイルがあってもラベルを割り当てない
- [ ] TS-8（`src-tauri/src/app/tests/chooser.rs`、AC-3）: 設定適用後の追従 - `keybinds.new_tab_global = 'Ctrl+Alt+G'`、`keybinds.new_tab = 'Ctrl+Alt+N'` の設定を適用し、チューザーを開き直すと、Global 行に `Ctrl+Alt+G`、デフォルト行に `Ctrl+Alt+N` が割り当てられる

### Integration Tests
該当なし。

### Manual Tests
- [ ] TS-9（リリースビルド、ユーザー、AC-8）: + のチューザーを開き、次を目視確認する - ラベルが右寄せされている。長いシェルパスがラベルの手前で切り詰められる。通常行とハイライト行でラベルの色が正しい。日本語と英語のロケールで同じラベル文字列が表示される

### E2E Tests
**Existing E2E tests**: なし
**Run command**: 未検出

### Edge Cases
- [ ] デフォルトのプロファイルが無い: 「Global Settings」行だけが `new_tab_global` のラベルを表示し、どのプロファイル行もラベルを表示しない（FR1, FR2, TS-5）
- [ ] `is_default` のプロファイルが複数ある: 最初のプロファイルだけが `new_tab` のラベルを表示する。Default バッジはすべての `is_default` プロファイルに残す（FR2, FR3, TS-5）
- [ ] 解析できないキーバインド指定: フォールバック先の既定チョードを表示する（FR5, TS-3）
- [ ] 小文字のキーバインド指定: 正規表記（大文字）で表示する（FR5, TS-3）
- [ ] チョードが上位の優先度のアクションに奪われている: そのラベルを表示しない（FR6, TS-6）
- [ ] `new_tab` = `new_tab_global`: デフォルト行はラベル無し、「Global Settings」行はラベルを表示する（FR6, TS-6）

### Performance Tests
該当なし。

## Security Considerations

該当なし。

## Error Handling

- 解析できないキーバインド指定は、組み込みの既定チョードにフォールバックして解決される。ラベルはその既定チョードを表示する（FR5）
- 奪われたチョードのラベルは表示せず、その行はショートカットが無い行と同じように描画する（FR6）

## Performance Optimization

### Caching Strategy
- ラベル: キャッシュしない。チューザーの描画時に `App.keybinds` から算出する（FR4、A3）

## Success Criteria

- [ ] FR1〜FR8 と NFR1〜NFR4 を実装し、テストする
- [ ] TS-1〜TS-8 の単体テストが通る
- [ ] TS-9 の手動確認が完了する
- [ ] `--no-default-features` ビルドがコンパイルできる（NFR3）
- [ ] AC-1〜AC-8 を満たす

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## Assumptions

- A1: `new_tab` のラベルは、最初の `is_default` プロファイル（Ctrl+Shift+T が開くもの）だけに付ける。Default バッジはすべての `is_default` プロファイルに残す
- A2: デフォルトでないプロファイル行と tmux 行にはラベルを付けない。これらを直接開くキーバインドは無い
- A3: ラベルはチューザーの描画時に `App.keybinds` から算出し、キャッシュしない。設定の適用は開いているチューザーも閉じる（`font_settings.rs:533-551`）
- A4: デフォルトのプロファイルが無い場合、「Global Settings」行は `new_tab_global` のショートカットだけを表示する。この場合 `new_tab` も Global Settings を開くが、表示しない
- A5: ラベルは + の新規タブのチューザーだけに表示し、Ctrl+Shift+P のプロファイルセレクターには表示しない
- A6: ラベルは行の右端に右寄せし、シェルパスはその手前で切り詰める
- A7: ラベルは解決済みチョードの正規表記（Ctrl, Shift, Alt の順、続けて主キー、`+` 連結）とする。フォールバックしたチョードは解決結果のとおり表示する
- A8: 上位の優先度のアクション（copy, paste, profile_selector、`new_tab` については new_tab_global も）に奪われたチョードは表示しない

## References

- 要件定義書: `feature-docs/new-tab-menu-shortcut-hints/REQUIREMENTS.md`
- UI デザインガイドライン: `doc/UI-DESIGN-GUIDELINES.yaml`
