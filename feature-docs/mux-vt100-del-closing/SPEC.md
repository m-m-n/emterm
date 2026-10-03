# Feature: mux-vt100-del-closing

## Overview

scrollback リングのバイトを vt100 に渡す 2 つの経路（ReadPane の `render_scrollback_rows` と復元時の `MuxPane::from_restored`）で、vt100 に渡すコピーだけ、CSI 内と lone ESC 直後の DEL（`0x7F`）を CAN（`0x18`）に置き換える。scrollback リング、strip / write-filter の出力、term_core の再生経路のバイトは変更しない。

要件定義書: `feature-docs/mux-vt100-del-closing/REQUIREMENTS.md`

## Objectives

- 閉じた CSI の後ろの内容を、scrollback リングを読む 2 つの vt100 コンシューマで正しく描画する: mux read（ReadPane）の描画（`agent_api.rs` の `render_scrollback_rows`）と、復元時の shadow parser 再生（`MuxPane::from_restored`）。
- リングのバイトと term_core（クライアント）の再生経路はバイト単位で同一に保つ。CSI_CLOSING は 1 バイトの DEL（`0x7F`）のまま、D2 の 1 対 1 置換もそのまま。

## User Stories

### US1: ReadPane の描画で閉じた CSI の後ろが正しく表示される
ReadPane 呼び出し元として、scrollback 末尾の描画結果で閉じた CSI の後ろの内容を正しく受け取りたい。

**Acceptance Criteria:**
- [ ] AC-1（FR1, FR5）: `render_pane_tail(b"\x1b[6\x7fHello\r\n", "", 5, 80)` が `"Hello"` を返す。同じ入力は修正前に失敗する（Hello が 6 行目に `"ello"` として現れる）。
- [ ] AC-3（FR4, FR5）: `b"\x1b\x7fHello\r\n"` の `render_pane_tail` が `"Hello"` を返す。修正前に失敗する。
- [ ] AC-5（FR5）: 再現経路: `strip_pty_output_for_scrollback_write_with_written_state` に `b"\x1b[6"`、続けて `b"nHello\r\n"`（状態を引き継ぐ）を渡した出力が D2 の DEL を含む。その出力の `render_pane_tail` が `"Hello"` を返す。

### US2: 復元時の shadow screen で閉じた CSI の後ろが正しく表示される
mux セッション復元として、復元した scrollback を shadow parser に再生したとき、閉じた CSI の後ろの内容を正しく表示し、リングは復元したバイトのまま保持したい。

**Acceptance Criteria:**
- [ ] AC-2（FR2, FR5, NFR1）: `b"\x1b[6\x7fHello"` を持つリングで `MuxPane::from_restored`（`alt_screen=false`）を呼ぶと、shadow screen の 0 行目が `"Hello"` になる。`pane.scrollback` の `read_all` は入力バイト（`0x7F` を含む）と一致する。テストは既存の `from_restored` テストと同じく unix 限定。

### US3: vt100 再生用コピーの変換
FR3 の共有関数として、書き込まれたストリームの状態に従って DEL を置き換えたい。

**Acceptance Criteria:**
- [ ] AC-4（FR3）: 出力長は入力長と等しい。DEL は、entry 状態の CSI 内（`ESC [ DEL`）、parameter 状態の CSI 内（`ESC [ 6 DEL`）、lone ESC の直後（`ESC DEL`）で CAN に置き換わる。DEL は、ground、OSC 本体内（`ESC ] 0 ; a DEL b BEL`）、DCS 本体内と APC 本体内、designator 待ち（`ESC ( DEL`）で保持される。CSI 内で DEL が 2 つ続くとき、置き換わるのは 1 つ目だけ（走査は元のバイトに従い、1 つ目の DEL で CSI が終わる）。DEL を含まない入力はそのまま返る。
- [ ] AC-6（NFR1, NFR3, NFR4）: 既存の scrollback_filter / write_filter / pane / handlers のテストが変更なしで通る。`CSI_CLOSING_BYTE` は `0x7F` のまま。`--no-default-features` の check がコンパイルでき、`--lib` スイート全体が通る。

## Technical Requirements

### Functional Requirements
- **FR1:** ReadPane rendering cancels the closing for vt100 — `render_scrollback_rows`（`src-tauri/src/mux/ipc/handlers/agent_api.rs`）は、scratch vt100 parser に生の scrollback 末尾ではなく FR3 の vt100 再生用コピーを渡す。例: `ESC [ 6 DEL H e l l o CR LF` は 1 行の `"Hello"` として描画する（6 行目の `"ello"` ではない）。変換は `render_scrollback_rows` の中で `scratch.process` の前に行うため、`render_pane_tail` にも効果が現れる。
- **FR2:** Restored shadow replay cancels the closing for vt100, and the ring is stored verbatim — `MuxPane::from_restored`（`src-tauri/src/mux/session/pane/mod.rs`）は、再生ステップ（再生バイトの `parser.process`）で、復元した scrollback バイトの FR3 vt100 再生用コピーを shadow parser に渡す。ペインの scrollback リングは復元したバイトを DEL も含めて変更せずに保持する。panic ガードと alt-screen ダンプの再生は変更せず、alt-screen ダンプのバイトは変換しない。
- **FR3:** State-aware DEL-to-CAN conversion for the vt100 replay copy — `src-tauri/src/mux/scrollback_filter.rs` の共有関数。バイト列を受け取り、同じ長さのコピーを返す。コピーは入力と同一で、書き込まれたストリームが CSI 内（entry または parameter のサブ状態）にある位置、または lone ESC の直後（`WrittenState::Csi` または `WrittenState::Escape`）にある位置の DEL（`0x7F`）だけを CAN（`0x18`）に置き換える。走査は `WrittenState::Ground` から始め、置換前の元のバイトで term_core の遷移に従って進める。既存の `WrittenState` の遷移（`WrittenState::advance` / `csi_step`）を使い、遷移表の 2 つ目のコピーは追加しない。ground の DEL、OSC / DCS / APC 本体内の DEL（`WrittenState` では Ground）、charset designator 待ち（`WrittenState::Designator`）の DEL は保持する。それ以外のバイトは、すでに存在する CAN も含めて変更せずにコピーする。
- **FR4:** The lone-ESC closing (D1) is covered by the same conversion — 書き込まれた lone ESC の直後に D1 が書く DEL（Escape 状態での `Written::close_before_removal`）は FR3 で CAN になり、vt100 はそこでエスケープを終え、次のバイトをエスケープの final として扱わない。例: `ESC DEL H e l l o` は `"Hello"` として描画する。Escape 状態で書かれる D3 の切断時の閉じはすでに CAN（`write_filter.rs` の `ESCAPE_CLOSING`）で、変更しない。CSI 内で書かれる D3 の切断時の閉じは DEL で、FR3 の対象になる。
- **FR5:** Regression tests — 再発を検出するテスト: (a) ReadPane の描画（`render_pane_tail`）で、`ESC [ 6 DEL Hello` が 1 行目に `"Hello"` を描画し、`ESC DEL Hello` が `"Hello"` を描画する。(b) `ESC [ 6 DEL Hello` を持つリングで `from_restored` を呼ぶと、shadow screen の 0 行目が `"Hello"` になり、リングの `read_all` には DEL バイトが残っている。(c) FR3 の関数の単体テストが、置換する位置と保持する位置を網羅する。(d) 再現経路: 状態を報告する strip に `ESC [ 6` と `n Hello` を 2 回の呼び出しで渡した D2 出力が、`render_pane_tail` で 1 行目に `"Hello"` を描画する。(a)、(b)、(d) は修正前に失敗する。

### Non-Functional Requirements
- **NFR1 - Compatibility:** scrollback リングの内容、strip / write-filter の出力（`CSI_CLOSING_BYTE = 0x7F`、D1 / D2 / D3 の挙動）、クライアントの term_core に届くバイトは、変更前とバイト単位で同一。違うのは vt100 に渡すコピーだけ。
- **NFR2 - Performance:** 変換は O(n) の 1 パスで、出力コピー以外の追加状態は O(1)。ReadPane と復元の経路でのみ実行し、PTY reader / リング書き込みのホットパスでは実行しない。
- **NFR3 - Build:** CLI-only ビルドがコンパイルできる: `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features`。vt100 と mux は常にビルドされるため、新しい関数は `gui` feature の後ろに置かない。
- **NFR4 - Test suite:** ライブラリのテストスイート全体が通る: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`。

## Implementation Approach

### Architecture

**System Architecture:**
```
┌──────────────────────────────────────────────────────────┐
│ scrollback リング（バイトは変更しない。DEL を含む）         │
├──────────────────────────────────────────────────────────┤
│ FR3: vt100 再生用コピー（DEL → CAN、長さ同一）              │
│      src-tauri/src/mux/scrollback_filter.rs               │
├────────────────────────────┬─────────────────────────────┤
│ FR1: render_scrollback_rows │ FR2: MuxPane::from_restored │
│      → scratch vt100       │      → shadow vt100 parser  │
└────────────────────────────┴─────────────────────────────┘
```

**Component Diagram:**
```
scrollback_filter.rs
  ├─ WrittenState（既存: advance / csi_step）
  └─ FR3 の共有関数 ──┬─> agent_api.rs::render_scrollback_rows（FR1）
                     └─> session/pane/mod.rs::MuxPane::from_restored（FR2）
```

### Data Flow

```
ReadPane:
  scrollback 末尾 → FR3 のコピー → scratch.process → 描画行（render_pane_tail）

復元:
  復元した scrollback バイト ─┬─> scrollback リング（そのまま保持）
                             └─> FR3 のコピー → parser.process → shadow screen
  alt-screen ダンプ → 変換せずに再生（変更なし）

term_core（クライアント）:
  リングのバイト → 変更なし
```

### API Design

該当なし（外部 API の変更はない）。

### Database Schema

該当なし

### Dependencies

**Internal Dependencies:**
- `WrittenState`（`src-tauri/src/mux/scrollback_filter.rs`）: FR3 の走査は既存の `WrittenState::advance` / `csi_step` を使う。
- `render_scrollback_rows` / `render_pane_tail`（`src-tauri/src/mux/ipc/handlers/agent_api.rs`）: FR1 の適用先。
- `MuxPane::from_restored`（`src-tauri/src/mux/session/pane/mod.rs`）: FR2 の適用先。
- `strip_pty_output_for_scrollback_write_with_written_state`: FR5 (d) / AC-5 の再現経路で使う。

**External Dependencies:**
- vt100: scratch parser と shadow parser。常にビルドされる。

### File Structure

```
src-tauri/src/mux/
├── scrollback_filter.rs        # FR3 の共有関数
├── scrollback_filter/
│   └── tests.rs                # TS-4、TS-5
├── ipc/handlers/
│   ├── agent_api.rs            # FR1: render_scrollback_rows
│   └── tests.rs                # TS-1、TS-3、TS-5
└── session/pane/
    ├── mod.rs                  # FR2: MuxPane::from_restored
    └── tests.rs                # TS-2（cfg(unix)）
```

## Declared Change Set

このフィーチャー固有のパスは手書きで列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、フィーチャー固有のパスに加えて、次の 2 つのワークフロー生成エントリをデフォルトで宣言する。

- `feature-docs/mux-vt100-del-closing/**`
- `test-docs/mux-vt100-del-closing/**`

`feature-docs/mux-vt100-del-closing/**` は `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物を含む。生成と所有は各フェーズドキュメントと `references/phase-state.md` にあり、この節は引用のみでルールは再掲しない。

`test-docs/mux-vt100-del-closing/**` はタスクごとのテスト記録 `test-docs/mux-vt100-del-closing/{T}.tests.yaml` を含む。生成と所有は `implement-phase.md` にあり、この節は引用のみでルールは再掲しない。

この 2 つのデフォルトエントリは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載がないことを除外とはみなさない。除外は意図的で明示的な絞り込みである。

この宣言はスーパーセットの主張である。検証時に観測される実際の変更集合は、宣言集合と等しい必要はなく、宣言集合に含まれる（CONTAINED IN）必要がある。implement タスクを生成しないフィーチャーは `test-docs/{feature}/` ディレクトリを生成しないが、その場合も宣言された `test-docs/{feature}/**` は正しい。実際に生成されないパスが宣言されていても違反ではない。

## Test Scenarios

### Unit Tests
- [ ] TS-1（AC-1）`src-tauri/src/mux/ipc/handlers/tests.rs`: D2 形の末尾 `ESC [ 6 DEL Hello CR LF` で `render_pane_tail` を呼ぶ - 結果が `"Hello"`
- [ ] TS-2（AC-2）`src-tauri/src/mux/session/pane/tests.rs`、`cfg(unix)`: `ESC [ 6 DEL Hello` を持つ `ScrollbackRingBuffer` を作り、`from_restored`（`alt_screen=false`）を呼ぶ - shadow screen の 0 行目が `"Hello"`、scrollback の `read_all` が入力バイトと一致
- [ ] TS-3（AC-3）`handlers/tests.rs`: `ESC DEL Hello CR LF` で `render_pane_tail` を呼ぶ - 結果が `"Hello"`
- [ ] TS-4（AC-4）`src-tauri/src/mux/scrollback_filter/tests.rs`: 入力と期待出力の表 - 置換するケース（CSI entry、CSI param、lone ESC、開いた CSI の末尾にある D3 形の DEL）と保持するケース（ground、OSC / DCS / APC 本体、designator 待ち、取り消しの DEL に続く 2 つ目の DEL、DEL を含まない入力）が期待どおりで、長さが保たれる
- [ ] TS-5（AC-5）`scrollback_filter/tests.rs` または `handlers/tests.rs`: 状態を報告する strip を `ESC [ 6` に、続けて引き継いだ状態で `n Hello CR LF` に実行し、出力を連結する - `0x7F` を含み、`render_pane_tail` が `"Hello"` を返す

### Integration Tests
- [ ] TS-6（AC-6）スイート: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` と `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` を実行する - 既存の scrollback_filter / write_filter / pane / handlers のテストを含めてすべて通り、check がコンパイルできる

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Existing E2E tests pass without regression

### Edge Cases
- [ ] CSI 内の 2 つの DEL: 走査では 1 つ目で CSI が終わるため、2 つ目は ground にあり保持される。
- [ ] プログラムが CSI 内に書いた DEL（閉じではない）も置き換わる。term_core もその DEL で CSI を取り消すため、vt100 が term_core と一致する。
- [ ] CSI 内の生の CAN: 走査は CSI を開いたままにし（term_core は CSI 内で C0 を実行する）、CAN は変更せずにコピーする。term_core と vt100 の差異があるとしても既存のもので、対象外。
- [ ] designator 待ちの DEL（`ESC ( DEL`）: 保持する。この状態では閉じは書かれない。
- [ ] ReadPane の末尾切り出し（`agent_api.rs` の `SCROLLBACK_READ_TAIL_BYTES`）やリング先頭の追い出しで状態が失われる場合: 走査は ground から始まるため、切り出し範囲の最初の ESC より前の DEL は保持される。vt100 も ground から始まり、ground では DEL を無視する。既知の制約。
- [ ] 空の入力は空の出力を返す。DEL を含まない入力はそのまま返る。

### Performance Tests
- 該当なし（NFR2 の計算量と実行経路は実装で満たす）

## Security Considerations

- **Authentication:** 該当なし
- **Authorization:** 該当なし
- **Input Validation:** 該当なし
- **Data Protection:** 該当なし
- **XSS Prevention:** 該当なし
- **SQL Injection Prevention:** 該当なし
- **CSRF Protection:** 該当なし

## Error Handling

### Error Codes

該当なし（FR3 の関数は任意のバイト列に対して同じ長さのコピーを返す）。

### Error Flow

- `MuxPane::from_restored` の panic ガードは変更しない（FR2）。

## Performance Optimization

### Performance Goals
- 変換は O(n) の 1 パス、出力コピー以外の追加状態は O(1)（NFR2）。

### Optimization Strategies
- 実行経路の限定: 変換は ReadPane と復元の経路でのみ実行し、PTY reader / リング書き込みのホットパスでは実行しない（NFR2）。

### Caching Strategy
- 該当なし

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All test scenarios pass
- [ ] Performance meets specified goals
- [ ] Security requirements are satisfied
- [ ] Documentation is complete
- [ ] Code review is completed
- [ ] AC-1 から AC-6 がすべて満たされる

## Assumptions

- a1: リングのバイト、strip / write-filter の出力、term_core の再生経路は変更しない。CSI_CLOSING は 1 バイトの DEL のままで、D2 の置換は 1 対 1 のまま。
- a2: 提供されたスキャン対象の範囲では、リングのバイトを受け取る vt100 コンシューマは `render_scrollback_rows` と `MuxPane::from_restored` だけ。ライブの PTY reader（`pty_spawn/mod.rs`）は、リングのバイトではなく生の読み取りデータを shadow parser に渡す。`build_shadow_parser_snapshot` はリングのバイトをクライアントの term_core に送り、vt100 には渡さない。`reference_scan_targets` 外のパスは調べていない。
- a3: lone ESC 直後の DEL（D1）は対象に含み、同じ変換で扱う（requirement.fix-approach の回答で確認済み）。
- a4: 変換するすべてのコピーで、走査は ground から始める。ReadPane の末尾切り出しより前やリングの追い出された先頭より前の状態は再構築しない。

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- なし

## References

- 要件定義書: `feature-docs/mux-vt100-del-closing/REQUIREMENTS.md`
- `src-tauri/src/mux/scrollback_filter.rs`
- `src-tauri/src/mux/ipc/handlers/agent_api.rs`
- `src-tauri/src/mux/session/pane/mod.rs`
- `src-tauri/src/mux/ipc/pty_spawn/write_filter.rs`
