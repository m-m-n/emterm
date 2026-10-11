# Feature: agent-latch-alt-screen-prompt-mark

## Overview

プレーンタブの agent-status 推定クリア latch で、alt screen 上の OSC 133 候補を pump の live マークとの照合から外し、main screen 上の OSC 133 マークを OSC 777 Set / Clear との真の順序で latch に渡す。要件は `feature-docs/agent-latch-alt-screen-prompt-mark/REQUIREMENTS.md` を参照する。

## Objectives

- 1 つの処理単位に alt screen 上の OSC 133 マーク、alt screen からの復帰、OSC 777 agent-status 報告（Set / Clear）、main screen 上の OSC 133 マークが含まれるとき、プレーンタブの推定クリア latch が main screen 上のマークを報告に対する真の順序で受け取り、alt screen 上のマークを照合に使わない。

## User Stories

### US1: alt screen のマークを含む処理単位で latch が main screen のマークを真の順序で受け取る

**Acceptance Criteria:**
- [ ] AC-1（FR1, FR2）: プレーンタブに `\x1b[?1049h\x1b]133;D;0\x07\x1b]133;A\x07\x1b[?1049l\x1b]777;emterm;agent-status;v=1;state=working\x07\x1b]133;D;0\x07\x1b]133;A\x07` を 1 回の `process_combined` 呼び出しで渡すと、latch 入力は [Set, Mark(CommandEnd), Mark(PromptStart)] になり、対応する Set イベントとともに適用するとタブのステータスはクリアされる。
- [ ] AC-2（FR1）: alt screen の切り替えに `?1047` または `?47` を使っても AC-1 と同じ結果になる。
- [ ] AC-3（FR1, FR2）: 間に Clear がある場合。Set、main screen の D、alt screen の A、復帰、Clear、main screen の A を含む 1 つの処理単位の latch 入力は [Set, Mark(CommandEnd), Clear, Mark(PromptStart)] になる。
- [ ] AC-4（FR1, FR2）: 間に Set がある場合。Set、main screen の D、alt screen の A、復帰、Set、main screen の A を含む 1 つの処理単位の latch 入力は [Set, Mark(CommandEnd), Set, Mark(PromptStart)] になる。
- [ ] AC-5（FR3, NFR2）: 既存の agent-exit-after-icon の latch テスト（ac1..ac7、`reconcile_latch_feed_*`）、osc7501 / program_status_feed のテスト、画面情報付きコールバックのテストが pass する。FR1 によって期待が反転するテストは名前を変え、その test-docs レコードを `.claude/rules/test-docs-records.md` に従って更新する。
- [ ] AC-6（FR1, FR2, FR3）: `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib` がすべて pass し、`CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml` と `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` が pass する。

## Technical Requirements

### Functional Requirements

- **FR1:** alt screen の候補を latch の照合から除外する。alt screen（`?47` / `?1047` / `?1049` で有効になる代替画面。term_core の `MODE_ALT_SCREEN` 判定）上で受け取った OSC 133 候補は、kind や順序に関係なく、agent-status latch のための pump の live マークとの照合に使わない。これらの候補は main screen の live マークを消費しない。
- **FR2:** main screen のマークを真の順序で latch に渡す。pump 内で live と確定した main screen の OSC 133 マークを、同じ pump の OSC 777 Set / Clear 報告に対する相対順序どおりに latch に渡す。同じ pump 内で同じ kind の alt screen マークが先行していても同様とする。
- **FR3:** 既存の振る舞いを維持する。次の振る舞いを変えない。
  - latch の解除条件: Set の後、live な main screen の D に続いて live な main screen の A が来ると、既存の Clear 経路で推定クリアを適用する。Set の後に D を伴わない A が来ても解除しない。
  - 明示的な Clear は latch を解除（disarm）する。
  - OSC 777 Set / Clear は元の位置のまま通過する。
  - 画面情報なしの `on_osc` 通知で届いた OSC 133 は main screen として扱う。
  - 認識できない OSC 133 の kind は何も追加しない。
  - mux 接続タブの latch feed は破棄する。
  - osc7501-alt-screen-prompt-mark による OSC 7501 feed の振る舞い。

### Non-Functional Requirements

- **NFR1 - 処理コスト:** 1 pump 分の latch feed の照合は、feed の長さに比例する時間で終わり、ブロックせず、I/O を行わない。
- **NFR2 - 他経路の維持:** mux 接続経路、OSC 7501 経路、OSC 133 以外のコールバックの振る舞いは、どちらの画面でも変わらない。

## Implementation Approach

### Architecture

**System Architecture:**

対象はプレーンタブの latch 経路。`callbacks.rs` の OSC 133 分岐（`handle_semantic_prompt`）が `LatchFeedEvent` 候補を供給し、`reconcile_latch_feed` が pump の live マークと照合する。mux daemon 側は対象外。

**Component Diagram:**
```
TerminalCallbacks::on_osc_with_screen (alt_screen_active)
TerminalCallbacks::on_osc              (画面情報なし = main screen)
        │
        ▼
callbacks.rs handle_semantic_prompt ── LatchFeedEvent 候補
        │
        ▼
agent_status_model.rs reconcile_latch_feed ── pump の live マークと照合
        │
        ▼
latch 入力列 [Set / Clear / Mark(...)]
```

### Data Flow

```
process_combined (1 処理単位)
  → OSC 133 / OSC 777 通知
  → handle_semantic_prompt: alt screen 上の候補は latch の照合対象から除外 (FR1)
  → reconcile_latch_feed: main screen の live マークを Set / Clear との真の順序で並べる (FR2)
  → latch 入力列を適用
```

alt screen の判定は term_core の `MODE_ALT_SCREEN` 判定（`?47` / `?1047` / `?1049` 共通）に従い、`TerminalCallbacks::on_osc_with_screen` が渡す。osc7501-alt-screen-prompt-mark と同じ判定とする。

### API Design

該当なし

### Database Schema

該当なし

### Dependencies

**Internal Dependencies:**
- term_core: `MODE_ALT_SCREEN` 判定（`?47` / `?1047` / `?1049` 共通）
- `TerminalCallbacks::on_osc_with_screen`: alt screen 判定付きの OSC 通知
- osc7501-alt-screen-prompt-mark: 同じ alt screen 判定を使う OSC 7501 feed（振る舞いは FR3 / NFR2 で維持）

**External Dependencies:**
- 該当なし

### File Structure

影響を受けるファイル:

```
callbacks.rs                    # handle_semantic_prompt の latch push、LatchFeedEvent / ProgramStatusFeedItem の doc
callbacks/tests.rs              # osc_133_a_on_the_alternate_screen_skips_the_7501_feed_and_keeps_the_latch_candidate (名前変更候補)
                                # osc_133_every_recognized_kind_follows_the_screen_rule_and_an_unrecognized_one_pushes_nothing
                                # screen_aware_notifications_keep_true_order_in_both_feeds
agent_status_model.rs           # reconcile_latch_feed の doc
agent_status_model/tests.rs     # ac5_* の alt 候補テスト
tabs/output_pipeline.rs         # コメント
tabs/tests/mux_link.rs          # latch_feed_end_to_end_* テスト
test-docs/osc7501-alt-screen-prompt-mark/*.tests.yaml   # テスト名変更に伴う更新義務
```

## Declared Change Set

フィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

すべての SPEC は、フィーチャー固有のパスに加えて、次の 2 つのワークフロー生成エントリをデフォルトで宣言する。

- `feature-docs/agent-latch-alt-screen-prompt-mark/**`
- `test-docs/agent-latch-alt-screen-prompt-mark/**`

`feature-docs/agent-latch-alt-screen-prompt-mark/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` で、この節は引用のみでルールは再掲しない。

`test-docs/agent-latch-alt-screen-prompt-mark/**` に含まれるもの: タスクごとのテスト記録 `test-docs/agent-latch-alt-screen-prompt-mark/{T}.tests.yaml`。生成主体は `implement-phase.md` で、この節は引用のみでルールは再掲しない。

この 2 つのデフォルトエントリは、SPEC 作成者が明示的に除外しない限り宣言に含まれる。記載が無いことを除外とはみなさない。除外は意図的で明示的な絞り込みとする。

この宣言はスーパーセットの主張であり、verify 時に観測される実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある（一致は求めない）。implement タスクを 1 つも生成しないフィーチャーは `test-docs/agent-latch-alt-screen-prompt-mark/` ディレクトリを生成しないが、宣言された `test-docs/agent-latch-alt-screen-prompt-mark/**` はその場合も正しい。宣言したパスが生成されなくても違反ではない。

## Test Scenarios

### Unit Tests

- [ ] TS-1（FR1, FR2）: プレーンタブで AC-1 のバイト列を 1 回の `process_combined` 呼び出しで渡す - タブの保留中の latch 入力が [Set, Mark(CommandEnd), Mark(PromptStart)] で、それを適用した後のモデル状態がクリアされている。
- [ ] TS-2（FR1）: alt screen の切り替えを `?1047`、`?47` に替えて TS-1 を繰り返す - TS-1 と同じ結果になる。
- [ ] TS-3（FR1, FR2）: Set、main の D、`?1049h`、alt の A、`?1049l`、Clear、main の A を 1 回の呼び出しで渡す - 保留中の latch 入力が [Set, Mark(CommandEnd), Clear, Mark(PromptStart)] になる。
- [ ] TS-4（FR1, FR2）: Set、main の D、`?1049h`、alt の A、`?1049l`、Set、main の A を 1 回の呼び出しで渡す - 保留中の latch 入力が [Set, Mark(CommandEnd), Set, Mark(PromptStart)] になる。
- [ ] TS-5（FR1）: コールバック単位で、`on_osc_with_screen` に `alt_screen_active=true` で渡した OSC 133 マーク - 認識できるすべての kind（A / B / C / D）で latch の照合に使われない。`alt_screen_active=false` と `on_osc` 経由では使われる。
- [ ] TS-6（FR1, FR2）: 同じ処理単位で main screen の A が alt screen のマークより前にある場合と、複数の alt screen マークが main screen のマークより前にある場合 - main screen のマークのみが順序どおり latch に届く。

### Integration Tests

- [ ] TS-7（FR3, NFR1, NFR2）: 既存の latch / osc7501 / program_status_feed / コールバックのテストを含む `--lib` スイート全体を実行する - すべて pass する。

### E2E Tests

**Existing E2E tests**: なし
**Run command**: 未検出
- [ ] 該当なし

### Edge Cases

- [ ] alt screen のマークの kind が後続の main screen のマークと一致する形（両画面で D → A）: alt screen のマークは照合に使わず、main screen のマークのみを真の順序で latch に渡す（FR1, FR2 / AC-1, TS-1）。
- [ ] 同じ処理単位で main screen の A が alt screen のマークより前にある: main screen のマークのみが順序どおり latch に届く（TS-6）。
- [ ] 複数の alt screen マークが main screen のマークより前にある: main screen のマークのみが順序どおり latch に届く（TS-6）。
- [ ] alt screen のマークと main screen の A の間に Clear または 2 回目の Set がある: latch 入力は [Set, Mark(CommandEnd), Clear, Mark(PromptStart)] / [Set, Mark(CommandEnd), Set, Mark(PromptStart)]（AC-3, AC-4）。
- [ ] 画面情報なしの `on_osc` 通知で届いた OSC 133: main screen として扱う（FR3, TS-5）。

### Performance Tests

- [ ] NFR1: 1 pump 分の latch feed の照合が feed の長さに比例する時間で終わり、ブロックせず、I/O を行わない（TS-7）。

## Security Considerations

該当なし

## Error Handling

該当なし

## Performance Optimization

### Performance Goals

- NFR1: 1 pump 分の latch feed の照合は、feed の長さに比例する時間で終わり、ブロックせず、I/O を行わない。

### Optimization Strategies

- 該当なし

### Caching Strategy

- 該当なし

## Success Criteria

- [ ] FR1〜FR3 を実装し、テストしている
- [ ] TS-1〜TS-7 が pass する
- [ ] NFR1 / NFR2 を満たす
- [ ] AC-1〜AC-6 を満たす

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

- なし

### Assumptions

- A1（可逆）: latch の解除条件は変わらない。Set の後、live な main screen の D に続いて live な main screen の A で latch が解除され、Set の後の A 単独では解除されない（`agent_status_model/tests.rs` の `ac1_set_then_live_d_then_live_a_clears_via_existing_clear_path` と `ac2_set_then_live_a_without_d_leaves_state_unchanged` で固定されている）。このため、タスクの再現手順（alt screen の A、復帰、Set、main screen の A）は、Set と main screen の A の間に main screen の D があり、alt screen のマークの kind が main screen のマークと揃う形として読む。受け入れ基準はこの手順を使う。
- A2（可逆）: alt screen の検出には、`TerminalCallbacks::on_osc_with_screen` が渡す term_core の `MODE_ALT_SCREEN` 判定（`?47` / `?1047` / `?1049` 共通）を使う。osc7501-alt-screen-prompt-mark と同じ判定とする。
- A3（可逆）: 同じ処理単位内で RIS より前に捕捉された候補（latch feed には、OSC 7501 feed と異なり reset 項目が無い）は対象外とする。
- A4（可逆）: 対象はプレーンタブの latch 経路（`callbacks.rs` の OSC 133 分岐 `handle_semantic_prompt` が `LatchFeedEvent` 候補を供給する経路と `reconcile_latch_feed`）。mux daemon 側は対象外とする。

## Implementation Phases (if applicable)

該当なし

## References

- 要件定義書: `feature-docs/agent-latch-alt-screen-prompt-mark/REQUIREMENTS.md`
- テスト名変更時の test-docs レコード更新規則: `.claude/rules/test-docs-records.md`
- 先行フィーチャー: osc7501-alt-screen-prompt-mark
