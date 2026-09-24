# Feature: agent-notify-visible-pane-docs

## Overview

エージェント状態通知について、`doc/SPECIFICATION.md` と `doc/AGENT-STATUS.md` の記述を main に入っている実際の挙動に合わせる。
あわせて、設定キー `agent_notify_visible_pane`（既定 on）を両ドキュメントに記載する。
変更対象はドキュメントだけとする。

## Objectives

- `doc/SPECIFICATION.md` と `doc/AGENT-STATUS.md` に、main に入っているエージェント状態通知の挙動を記述する。あわせて設定キー `agent_notify_visible_pane`（既定 on）を記載する。

## Acceptance Criteria

- [ ] AC-1: `doc/SPECIFICATION.md` に、可視ペインの通知は `agent_notify_visible_pane`（既定 on）で制御され、この設定が off のときだけ抑止されると記載されている（現在 1231 行目）。
- [ ] AC-2: `doc/SPECIFICATION.md` の設定キー一覧に `agent_notify_visible_pane` が含まれている（現在 1232 行目）。
- [ ] AC-3: `doc/AGENT-STATUS.md` の「## Notifications」節から、可視でないペインにだけ通知するという記述がなくなっている。代わりに、可視ペインにも既定で通知し、`agent_notify_visible_pane` が off のときだけ抑止すると記載されている。
- [ ] AC-4: `doc/AGENT-STATUS.md` の「## Notifications」節に、通知の条件として `agent_status_notifications`、全体の通知設定、`agent_notify_on_done`、`agent_notify_on_blocked`、`agent_notify_visible_pane` が挙げられ、すべて既定 on と記載されている。ペインごとのレート制限への言及も残っている。
- [ ] AC-5: この feature の差分が触るのは `doc/` 配下（と `feature-docs/` 配下）のファイルだけで、ソースファイルとテストファイルは変更されていない。

## Technical Requirements

### Functional Requirements

- **FR1:** SPECIFICATION.md の可視ペインの挙動の記述。`doc/SPECIFICATION.md` の Mux Agent Status and Agent API 節に次のことが書かれている。可視ペイン（フォーカスされたウィンドウのアクティブタブ）への通知は既定で発火し、`agent_notify_visible_pane`（既定 on）で制御され、この設定が off のときだけ抑止される。可視でないペインはこの設定の影響を受けない。これは `doc/SPECIFICATION.md:1231` で既に満たされている。タスクに書かれた行番号（1195）は古い。コードとの照合で不一致が見つからない限り、編集は不要。
- **FR2:** SPECIFICATION.md の設定キー一覧に `agent_notify_visible_pane` を含める。同じ節の設定キー一覧には、`agent_notify_on_done` と `agent_notify_on_blocked` に並んで `agent_notify_visible_pane` が挙げられ、いずれも既定 on と書かれている。これは `doc/SPECIFICATION.md:1232` で既に満たされているため、編集は不要。
- **FR3:** AGENT-STATUS.md の Notifications 節で可視ペインの挙動を記述する。`doc/AGENT-STATUS.md` の「## Notifications」節の冒頭の文（190 行目、"An OS notification fires when a pane not visible in the foreground window has a real transition to `blocked` or `done`"）を置き換える。新しい文には次のことを書く。`blocked` または `done` への実遷移では、どのペインでも OS 通知を発火する。可視ペイン（フォーカスされたウィンドウのアクティブタブ）の通知は `agent_notify_visible_pane`（既定 on）で制御され、この設定が off のときだけ抑止される。可視でないペインはこの設定の影響を受けない。通知しないケースの既存の一覧（同じ状態の再報告、名前だけの変更、snapshot/replay）は変更しない。
- **FR4:** AGENT-STATUS.md の Notifications 節の設定キー一覧を更新する。`doc/AGENT-STATUS.md` の条件の段落（199〜203 行目）を、コードと SPECIFICATION.md の一覧に合わせる。この段落には次をすべて挙げ、いずれも既定 on と書く。`agent_status_notifications`、全体の通知設定、イベントごとの切り替え `agent_notify_on_done`（done への遷移）と `agent_notify_on_blocked`（blocked への遷移）、`agent_notify_visible_pane`（可視ペイン）。既存のペインごとのレート制限の記述も残す。根拠は `src-tauri/src/notifications.rs:271-287` の `should_fire_agent_notification` で、これらの条件をすべて満たすことを要求している。サニタイズされた表示名についての既存の文は残す。

### Non-Functional Requirements

- **NFR1 - 変更範囲:** ドキュメントだけを変更する。Rust、TypeScript、設定スキーマ、テストは変更しない。
- **NFR2 - コードとの一致:** 記述する挙動は main のコードと一致させる。PR #29（active-window-agent-notification）はマージ済みで、挙動は `src-tauri/src/notifications.rs`、`src-tauri/src/app/agent_status.rs`、`src-tauri/src/settings/mod.rs`（321 行目で既定値 true）にある。
- **NFR3 - 記述内容:** ドキュメントには挙動だけを書く。理由、PR や指摘への参照、経緯は書かない。

## Implementation Approach

### 変更対象ファイル

```
doc/
├── SPECIFICATION.md   # FR1 / FR2: 既に満たされている。コードとの照合で不一致が見つかった場合だけ編集する
└── AGENT-STATUS.md    # FR3: 「## Notifications」節の冒頭の文（190 行目）を置き換える
                       # FR4: 条件の段落（199〜203 行目）を更新する
```

### 照合するコード

- `src-tauri/src/notifications.rs:271-287` — `should_fire_agent_notification`
- `src-tauri/src/app/agent_status.rs:45-57` — `agent_status_pane_visible`
- `src-tauri/src/settings/mod.rs:321` — `agent_notify_visible_pane` の既定値 true

### API Design / Database Schema / Dependencies

該当なし。

## Declared Change Set

この節には手で書いた一覧を置かず、create-plan で導出する。feature 固有のパスは、create-plan の時点で `workflow.yaml` にある全タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

feature 固有のパスに加えて、すべての SPEC は既定で次の 2 つのワークフロー生成物を宣言する。

- `feature-docs/agent-notify-visible-pane-docs/**`
- `test-docs/agent-notify-visible-pane-docs/**`

`feature-docs/agent-notify-visible-pane-docs/**` には `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、design ステップが出力するデザイン成果物が含まれる。これらは各フェーズのドキュメントと `references/phase-state.md` が生成して管理する。この節ではそれらを参照するだけで、規則は再掲しない。

`test-docs/agent-notify-visible-pane-docs/**` には、タスクごとのテスト記録 `test-docs/agent-notify-visible-pane-docs/{T}.tests.yaml` が含まれる。これは `implement-phase.md` が生成して管理する。この節ではそれを参照するだけで、規則は再掲しない。

この 2 つの既定エントリは、SPEC の作成者が明示的に外さない限り宣言に含まれる。書いていないことを理由に、外したとは見なさない。外すのは意図的で明示的な絞り込みに限る。

この宣言は上位集合の宣言である。検証時に観測した実際の変更集合は、宣言した集合に含まれていればよく、一致している必要はない。implement タスクが 1 つもない feature では `test-docs/agent-notify-visible-pane-docs/` ディレクトリが生成されない。その場合も、宣言した `test-docs/agent-notify-visible-pane-docs/**` は正しいままである。宣言したパスが実際に作られなくても違反にはならない。

## Test Scenarios

### 検証シナリオ

- [ ] TS-1（AC-1, AC-2）: `doc/SPECIFICATION.md` を `agent_notify_visible_pane` で grep する。Mux Agent Status 節の可視ペインの記述と、設定キー一覧の両方に出現すること。
- [ ] TS-2（AC-3）: `doc/AGENT-STATUS.md` を "not visible in the foreground window" で grep すると一致しないこと。Notifications 節を `agent_notify_visible_pane` で grep すると一致すること。
- [ ] TS-3（AC-4）: `doc/AGENT-STATUS.md` の Notifications 節を `agent_status_notifications`、`agent_notify_on_done`、`agent_notify_on_blocked`、`agent_notify_visible_pane` のそれぞれで grep する。すべて見つかること。
- [ ] TS-4（AC-1, AC-3, AC-4）: 記述した通知の条件を、`should_fire_agent_notification`（`src-tauri/src/notifications.rs:271-287`）と `agent_status_pane_visible`（`src-tauri/src/app/agent_status.rs:45-57`）に対して目視で照合する。可視とは、OS ウィンドウにフォーカスがあり、かつペインがアクティブタブに属していることを指す。
- [ ] TS-5（AC-5）: ベースリビジョンに対する `git diff --name-only` に、`doc/` と `feature-docs/` 配下のパスだけが出ること。

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

## Assumptions

- `doc/SPECIFICATION.md` の「## Configuration > Key Configuration Fields」にある JSON の例（1617〜1650 行目）は設定キーの一部を示す例で、全キーの一覧ではない。エージェント通知のキーはここに 1 つも出てこないため、タスクが指す設定キーの一覧には当たらない。この例は変更しない。（可逆）
- `doc/AGENT-STATUS.md` の「可視ペイン」は、コードと SPECIFICATION.md と同じ定義で使う。OS ウィンドウにフォーカスがあり、かつペインがアクティブタブに属していることを指す。mux に接続したタブでは、そのタブのウィンドウグループ全体がこれに当たる。（可逆）

## Success Criteria

- [ ] FR1〜FR4 を満たしている
- [ ] TS-1〜TS-5 をすべて通過する
- [ ] NFR1〜NFR3 を満たしている

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

なし。

## References

- doc/SPECIFICATION.md: Mux Agent Status and Agent API 節
- doc/AGENT-STATUS.md: 「## Notifications」節
- src-tauri/src/notifications.rs
- src-tauri/src/app/agent_status.rs
- src-tauri/src/settings/mod.rs
