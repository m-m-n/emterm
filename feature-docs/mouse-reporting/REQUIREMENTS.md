---
title: "mouse-reporting"
created_date: 2026-09-18
status: draft
---

# mouse-reporting - 要件定義書

> Note: this document follows the em-workflow requirements template (its
> section headings are template-fixed); the content is rendered in English to
> match the language of the resolved requirement statements it renders. Every
> requirement, acceptance criterion, test scenario and assumption below comes
> from requirements-analyst's resolved requirements; nothing is originated here.

## 1. 概要

### 1.1 背景

DEC private modes 1000/1002/1003/1006 currently fall through
`MODE_ACTION_TS_FALLBACK`, a dead path left over from the pre-native WebView
build, so a mouse-tracking request is accepted and then dropped. As a result,
full-screen and TUI programs (vim, less, tmux, Claude Code, htop) that request
DECSET 1000/1002/1003 silently ignore the pointer, and this is the last
unimplemented arm of eMterm's "Full ANSI control sequence support" product claim
on the pointer side.

### 1.2 目的

- Let terminal applications running under eMterm receive mouse events, so
  full-screen and TUI programs that request DECSET 1000/1002/1003 become
  mouse-usable instead of silently ignoring the pointer.
- Close the last unimplemented arm of the "Full ANSI control sequence support"
  claim on the pointer side.
- Preserve every existing local pointer behaviour behind a single predictable
  escape hatch, so enabling application mouse support never costs the user an
  existing capability.

### 1.3 スコープ

**In scope**

- Core-side tracking/encoding mode state for DECSET 1000 / 1002 / 1003 / 1006
  (FR1).
- Press, release, motion and wheel reporting to the active tab's PTY
  (FR2, FR3, FR4, FR11).
- The X10 and SGR (1006) report encodings, including modifier bits and the X10
  coordinate limit (FR5, FR6, FR9).
- The Shift local override and the resulting Ctrl+click / middle-click routing
  (FR7, FR8).
- Precedence of the existing egui chrome guards over reporting (FR10).

**Out of scope**

- DECSET 1005 (UTF-8 extended reporting) and DECSET 1015 (urxvt extended
  reporting): 1005 keeps its existing `MODE_ACTION_TS_FALLBACK` arm, and 1015 is
  deliberately given no match arm and remains a silent no-op (assumption A7).
- Any new user setting: `crates/app_settings`, the TypeScript `AppSettings`
  mirror and the settings panel are untouched (NFR2, assumption A2).
- macOS (NFR4).
- A design step: it is skipped for this feature (see section 6).

## 2. ビジネス要件

### 2.1 ビジネス目標

1. Let terminal applications running under eMterm receive mouse events, so
   full-screen and TUI programs (vim, less, tmux, Claude Code, htop) that request
   DECSET 1000/1002/1003 become mouse-usable instead of silently ignoring the
   pointer.
2. Close the last unimplemented arm of eMterm's "Full ANSI control sequence
   support" product claim on the pointer side: DEC private modes
   1000/1002/1003/1006 currently fall through `MODE_ACTION_TS_FALLBACK`, a dead
   path left over from the pre-native WebView build, so the request is accepted
   and then dropped.
3. Preserve every existing local pointer behaviour (text selection, Ctrl+click
   link open, middle-click PRIMARY paste, scrollback wheel scroll, DECSET 1007
   arrow translation, egui chrome interaction) behind a single predictable escape
   hatch, so enabling application mouse support never costs the user an existing
   capability.

### 2.2 対象ユーザー

| ユーザータイプ | 説明 |
|----------------|------|
| Users of mouse-aware full-screen / TUI programs | Users running vim, less, tmux, Claude Code or htop inside eMterm, whose pointer input is currently ignored by those programs. |
| Terminal applications requesting mouse tracking | Programs that issue DECSET 1000 / 1002 / 1003 / 1006 and expect mouse reports on the PTY. |
| Users relying on eMterm's local pointer behaviour | Users of text selection, Ctrl+click link open, middle-click PRIMARY paste, scrollback wheel scroll and DECSET 1007 arrow translation, which must stay reachable. |

### 2.3 期待される効果

- Full-screen and TUI programs become mouse-usable inside eMterm instead of
  silently ignoring the pointer.
- The "Full ANSI control sequence support" claim is complete on the pointer
  side; DECSET 1000/1002/1003/1006 requests stop being accepted-then-dropped.
- Every existing local pointer capability remains reachable behind a single
  predictable escape hatch (Shift).

## 3. ユースケース

### 3.1 ユースケース一覧

| ID | ユースケース名 | アクター | 優先度 |
|----|----------------|----------|--------|
| UC01 | Operate a mouse-aware application with the pointer | User of a mouse-aware TUI program | — |
| UC02 | Reach eMterm's local pointer behaviour while tracking is active | User of eMterm's local pointer features | — |
| UC03 | Operate a mouse-aware application on a mux-attached remote pane | User of a mux-attached remote pane | — |

Priority is not specified in the resolved requirements, so it is left unset
(`—`) rather than assigned here.

### 3.2 ユースケース詳細

#### UC01: Operate a mouse-aware application with the pointer

**アクター**: A user running a mouse-aware application (e.g. `vim` with
`:set mouse=a`, or `less -X`) inside eMterm.

**事前条件**:
- The application has enabled a tracking mode via DECSET 1000 / 1002 / 1003
  (optionally with 1006), and the mode bit is set core-side (FR1).
- Shift is not held (FR7).

**基本フロー**:
1. The user presses a pointer button over the terminal grid area.
2. eMterm's existing egui chrome guards decline the event (FR10).
3. eMterm encodes the event per the active encoding (FR5) with the modifier bits
   of FR6, and writes it to the active tab's PTY (FR11).
4. The user drags; motion is reported according to the active tracking mode
   (FR3), capped to cell changes (NFR1).
5. The user turns the wheel; the notch is reported as button code 64 / 65 on
   both the main and the alternate screen (FR4).
6. The user releases the button, which is reported per FR2.

**代替フロー**:
- Under the X10 encoding an event whose 1-based column or row exceeds 223
  produces no report at all (FR9).
- Under DECSET 1000 alone, pointer motion produces no report (FR3).

**事後条件**:
- The application has received the reports and updated its own view; eMterm's
  scrollback offset is unchanged for wheel events (AC6).

#### UC02: Reach eMterm's local pointer behaviour while tracking is active

**アクター**: A user of eMterm's local pointer features.

**事前条件**:
- A tracking mode is active (FR1).

**基本フロー**:
1. The user holds Shift and performs the pointer action.
2. Reporting is suppressed entirely for that event (FR7).
3. The event is routed to eMterm's existing local handling: Shift+drag selects
   text, Shift+Ctrl+click opens a hovered link, Shift+middle-click pastes
   PRIMARY, Shift+wheel scrolls eMterm's scrollback.

**代替フロー**:
- Without Shift, a Ctrl+left-click is reported with modifier bit 16 set and a
  middle-click is reported with button code 1, instead of opening the link or
  pasting PRIMARY (FR8).
- An event consumed by one of the existing egui chrome guards keeps its current
  local behaviour and is never reported (FR10).

**事後条件**:
- The local behaviour occurred and no report was written to the PTY (AC9, AC12).

#### UC03: Operate a mouse-aware application on a mux-attached remote pane

**アクター**: A user attached to a mux pane on a remote host.

**事前条件**:
- A mux pane is attached and a mouse-aware application runs in it with a
  tracking mode enabled.

**基本フロー**:
1. The user performs a pointer action over the terminal grid area.
2. The report is written through the same `Tab::write_input` path the DECSET
   1007 arrow translation already uses (FR11).
3. The report reaches the remote application through the existing input
   transport.

**事後条件**:
- Reports reach a local PTY and a mux-attached remote pane identically (FR11).

## 4. 機能要件

### 4.1 機能一覧

| ID | 機能名 | 説明 | 優先度 |
|----|--------|------|--------|
| FR1 | Core-side mouse-tracking mode state | Track DECSET 1000/1002/1003/1006 as core-side mode bits and stop returning `MODE_ACTION_TS_FALLBACK`. | — |
| FR2 | Button press and release reporting | Report press and matching release to the active tab's PTY instead of local selection / link / paste handling. | — |
| FR3 | Motion reporting for 1002 and 1003 | Report motion per the active tracking mode, with the motion bit 32. | — |
| FR4 | Wheel reporting takes precedence over both local wheel consumers | Report wheel notches as 64 / 65 on both screens; scrollback scroll and DECSET 1007 apply only when no tracking mode is active. | — |
| FR5 | X10 and SGR (1006) report encodings | Encode reports as `CSI M Cb Cx Cy` or, with 1006, as `CSI < Cb ; Cx ; Cy M`/`m`. | — |
| FR6 | Modifier bits in the button code | Ctrl sets bit value 16, Alt/Meta sets 8; Shift's bit 4 is never set. | — |
| FR7 | Shift is the sole local override | Holding Shift suppresses reporting and routes the event to local handling. | — |
| FR8 | Ctrl+click and middle-click are reported while tracking is active | Bare Ctrl+click and bare middle-click reach the application instead of opening a link or pasting PRIMARY. | — |
| FR9 | X10 coordinate overflow suppresses the report | Beyond column/row 223 the X10 encoding emits nothing and never clamps. | — |
| FR10 | Existing egui chrome guards run before reporting | Every guard already present in `pointer_routing.rs` keeps precedence over reporting. | — |
| FR11 | Reports are written to the active tab's PTY | Reports go through the existing `Tab::write_input` path. | — |

Priority is not specified in the resolved requirements, so it is left unset
(`—`) rather than assigned here.

### 4.2 機能詳細

#### FR1: Core-side mouse-tracking mode state

**説明**: `TerminalCore::handle_set_mode` tracks DEC private modes 1000 (normal /
button tracking), 1002 (button-event / drag tracking), 1003 (any-event tracking)
and 1006 (SGR extended encoding) as core-side mode bits, set on `CSI ? Pm h` and
cleared on `CSI ? Pm l`, returning `MODE_ACTION_NONE` instead of the current
`MODE_ACTION_TS_FALLBACK`. The host reads the active tracking mode and the active
encoding through `get_mode`, matching the pattern DECSET 1007 already uses via
`MODE_ALTERNATE_SCROLL`.

**入力**: `CSI ? Pm h` / `CSI ? Pm l` with Pm in {1000, 1002, 1003, 1006}.

**出力**: `MODE_ACTION_NONE`; the corresponding core-side mode bit, readable via
`get_mode`.

**関連受け入れ基準**: AC1, AC15.

**関連テストシナリオ**: TS1, TS2.

#### FR2: Button press and release reporting

**説明**: While any of DECSET 1000 / 1002 / 1003 is active, a pointer button
press and its matching release inside the terminal grid area are reported to the
active tab's PTY instead of driving eMterm's local selection / link / paste
handling. Left, middle and right buttons map to button codes 0, 1 and 2; in the
X10 encoding a release is reported as button code 3, while in the SGR encoding
the press button code is retained and the final byte distinguishes press (`M`)
from release (`m`).

**関連受け入れ基準**: AC2, AC3.

**関連テストシナリオ**: TS3, TS4, TS10.

#### FR3: Motion reporting for 1002 and 1003

**説明**: Under DECSET 1002 a motion report is emitted only while at least one
button is held, carrying that button's code plus the motion bit (value 32). Under
DECSET 1003 motion is reported whether or not a button is held; with no button
held the button code is 3 plus the motion bit. Under DECSET 1000 alone no motion
is reported. Report volume is capped by NFR1.

**関連受け入れ基準**: AC4.

**関連テストシナリオ**: TS3, TS9, TS10.

#### FR4: Wheel reporting takes precedence over both local wheel consumers

**説明**: While any of DECSET 1000 / 1002 / 1003 is active, wheel notches are
reported to the application as button code 64 (up) / 65 (down) on BOTH the main
screen and the alternate screen. eMterm's scrollback scroll
(`App::scroll_up_by` / `scroll_down_by`) and the DECSET 1007 alternate-scroll
arrow translation (`alternate_scroll_wheel_bytes`) apply only when no tracking
mode is active, making the three wheel consumers mutually exclusive behind one
check placed ahead of both existing paths in `handle_mouse_wheel`.

**ビジネスルール**:
- The three wheel consumers (report / DECSET 1007 arrow translation / scrollback
  scroll) are mutually exclusive.

**関連受け入れ基準**: AC6, AC7.

**関連テストシナリオ**: TS3, TS8, TS10.

#### FR5: X10 and SGR (1006) report encodings

**説明**: With DECSET 1006 inactive, reports use the X10 encoding
`CSI M Cb Cx Cy`, where each of the button byte, the 1-based column and the
1-based row is biased by 32. With DECSET 1006 active, reports use the SGR
encoding `CSI < Cb ; Cx ; Cy M` for a press and `... m` for a release, with
unbiased decimal button code and 1-based coordinates. Coordinates are always
1-based and expressed in terminal cells derived from the pointer pixel position.

**関連受け入れ基準**: AC2, AC3, AC11.

**関連テストシナリオ**: TS3, TS4.

#### FR6: Modifier bits in the button code

**説明**: The button code carries the Ctrl modifier as bit value 16 and the
Alt/Meta modifier as bit value 8, taken from the host's current modifier state
(`WindowHost::current_mods`). The Shift modifier bit (value 4) is never set in an
emitted report, because Shift is consumed locally by FR7 and therefore can never
be present at emission time.

**関連受け入れ基準**: AC8, AC10.

**関連テストシナリオ**: TS5.

#### FR7: Shift is the sole local override

**説明**: Holding Shift suppresses mouse reporting entirely for that event and
routes it to eMterm's existing local handling, even while a tracking mode is
active. Consequently Shift+drag selects text, Shift+Ctrl+click opens a hovered
link, Shift+middle-click pastes PRIMARY, and Shift+wheel scrolls eMterm's
scrollback. No other modifier, key, setting or menu suppresses reporting.

**関連受け入れ基準**: AC9, AC10.

**関連テストシナリオ**: TS5, TS11.

#### FR8: Ctrl+click and middle-click are reported while tracking is active

**説明**: While a tracking mode is active and Shift is not held, a Ctrl+left-click
is reported to the application with modifier bit 16 set rather than invoking
`WindowHost::try_open_link_at_pointer`, and a middle-click is reported with button
code 1 rather than performing the `settings.middle_click_paste` PRIMARY paste.
Both local behaviours remain reachable via FR7's Shift override, and both are
unchanged when no tracking mode is active.

**関連受け入れ基準**: AC8, AC9.

**関連テストシナリオ**: TS10, TS11.

#### FR9: X10 coordinate overflow suppresses the report

**説明**: In the X10 (non-SGR) encoding, an event whose 1-based column or row
would exceed 223 (the largest value the 32-biased single byte can represent)
produces no report at all. The coordinate is never clamped to 223 and no partial
or truncated report is emitted. The SGR encoding has no such limit and reports the
true coordinate.

**エラーケース**:

| エラー | 条件 | 対応 |
|--------|------|------|
| Unencodable coordinate | X10 encoding active and the 1-based column or row exceeds 223 | Emit no report at all; never clamp and never emit a partial report |

**関連受け入れ基準**: AC11.

**関連テストシナリオ**: TS6.

#### FR10: Existing egui chrome guards run before reporting

**説明**: Mouse reporting applies only to events over the terminal grid. Every
guard already present in `pointer_routing.rs` keeps precedence over reporting:
the profile-selector modal, the CSD title bar + tab bar top strip, the status-bar
bottom strip, the right-edge scrollbar overlay, the mux sidebar (persistent panel
and overlay card, via `ui::mux_sidebar::point_in_sidebar`), and the CSD
edge-resize hot zone. A press, release, motion or wheel event that one of those
guards consumes is never reported to the application.

**関連受け入れ基準**: AC12.

**関連テストシナリオ**: TS12.

#### FR11: Reports are written to the active tab's PTY

**説明**: Every emitted report is written to the active tab's PTY input channel
through the same `Tab::write_input` path the DECSET 1007 arrow translation already
uses, so reports reach a local PTY and a mux-attached remote pane identically and
inherit the existing input transport without a new channel.

**関連受け入れ基準**: AC6 (wheel reports reach the PTY without touching
`App::scroll_offset`).

**関連テストシナリオ**: TS10, TS13.

## 5. 非機能要件

### 5.0 非機能要件一覧

| ID | 機能名 | 要約 |
|----|--------|------|
| NFR1 | Motion reports are emitted only on cell change | Motion reports are capped at grid resolution via a cached last-reported cell. |
| NFR2 | No new user setting | No settings field, no TypeScript mirror change, no settings-panel row. |
| NFR3 | CLI-only build keeps compiling | `term_core` changes stay GUI-free; pointer code stays behind `#[cfg(feature = "gui")]`. |
| NFR4 | Linux and Windows parity, no platform-specific branches | Identical behaviour, no `#[cfg(unix)]` / `#[cfg(windows)]` branch. |
| NFR5 | Tests follow the existing inline convention with no new dependency | Inline `#[cfg(test)] mod tests {}`, `fn <subject>_<scenario>_<expected>()` naming, no new test crate. |
| NFR6 | Encoding logic is unit-testable without a window | Pure functions over plain values. |
| NFR7 | Report byte-sequence generation ships with unit tests | Byte-exact tests for both encodings, every event kind, modifier bits and X10 overflow. |

### 5.1 パフォーマンス要件

- **NFR1**: For DECSET 1002 and 1003, a motion report is emitted only when the
  (column, row) cell actually differs from the last reported cell; the last
  reported cell is cached in the host. `handle_pointer_moved` runs once per raw
  winit motion event on a path the project already optimised for CPU, so report
  volume must be capped at grid resolution rather than pointer-pixel resolution.
  (Acceptance criterion AC5; test scenario TS7.)

### 5.2 セキュリティ要件

The resolved requirements contain no security requirement for this feature; none
is stated here.

### 5.3 可用性要件

The resolved requirements contain no availability requirement for this feature;
none is stated here.

### 5.4 保守性要件

- **NFR5**: Verification uses inline `#[cfg(test)] mod tests {}` blocks next to
  the code under test, in the `fn <subject>_<scenario>_<expected>()` naming style
  dominant in `crates/term_core/`, with each unit under test constructed
  explicitly per test and no shared global fixture. No test framework crate is
  added (the project uses neither `proptest` nor `criterion`), and no E2E harness
  is introduced. (AC15.)
- **NFR6**: Button-code composition, coordinate biasing, the X10 overflow
  decision and the cell-change filter are expressed as pure functions taking
  plain values, so they are exercisable by unit tests without a winit window, a
  GPU surface or a live PTY — mirroring how `accumulate_alt_scroll_lines` and
  `alternate_scroll_wheel_bytes` are factored in
  `window_host::input_translate`. (AC16.)
- **NFR7**: Unit tests covering the generated report byte sequences are part of
  the deliverable, not optional follow-up: both encodings (X10 per FR5 and SGR
  per FR5), every reported event kind (press and release per FR2, motion per FR3,
  wheel per FR4), the modifier bits per FR6, and the X10 overflow suppression per
  FR9 each have at least one test asserting the exact emitted bytes. Realised by
  test scenarios TS3, TS4, TS5 and TS6, which run under NFR5's inline convention
  and NFR6's window-free factoring. (AC16.)

### 5.5 互換性要件

- **NFR2**: The feature introduces no setting: `crates/app_settings` gains no
  field, the TypeScript `AppSettings` mirror in
  `src-tauri/web-shared/settings/types.ts` is unchanged, and the settings panel
  gains no row. Emission is governed solely by the application's DECSET request
  plus FR7's Shift override. (AC13.)
- **NFR3**: The `crates/term_core` changes (FR1) stay free of GUI-only crates so
  `cargo check --no-default-features` still succeeds; all winit / egui /
  pointer-side code (FR2-FR11) lives under the existing
  `#[cfg(feature = "gui")]` module tree in `src-tauri/src/window_host/`. (AC14.)
- **NFR4**: Reporting behaviour is identical on Linux and Windows. Encoding and
  routing derive from winit's platform-independent `ButtonSource` /
  `MouseScrollDelta` / modifier state, so the feature adds no `#[cfg(unix)]` /
  `#[cfg(windows)]` branch. macOS remains out of scope.

## 6. UI/UX要件

### 6.1 画面設計要件

No UI/UX requirement. The design step is skipped for this feature: it adds no
user-visible UI surface — it changes how existing pointer events are routed and
introduces a byte encoding on the PTY input path. It touches none of the three
Material Design 3 mirror layers (`doc/UI-DESIGN-GUIDELINES.yaml`,
`src-tauri/src/ui/md3.rs`, `src-tauri/web-shared/styles.css`), adds no widget,
dialog or panel, and renders nothing new. With the settings-gate decision
resolved to "no setting", the one branch that could have introduced a
settings-panel row is closed.

### 6.2 画面遷移

Not applicable — no screen is added or changed.

### 6.3 レスポンシブ対応

Not applicable.

## 7. データ要件

No persistent data model is introduced. The only state the feature adds is
runtime state: the core-side mode bits of FR1 and the cached last-reported cell
of NFR1. No settings field is added (NFR2).

## 8. 外部連携

### 8.1 連携システム

| システム名 | 連携方法 | データ |
|------------|----------|--------|
| The active tab's PTY (local or mux-attached remote pane) | The existing `Tab::write_input` input path, the same one the DECSET 1007 arrow translation uses (FR11) | Mouse report byte sequences in the X10 or SGR encoding (FR5) |

### 8.2 API仕様要件

The interface is the terminal control-sequence protocol itself: DECSET
1000/1002/1003/1006 requests in (FR1) and the report byte sequences of FR5 out.
No programmatic API is added.

## 9. 制約条件

### 9.1 技術的制約

- `crates/term_core` changes must stay free of GUI-only crates; pointer-side code
  stays under `#[cfg(feature = "gui")]` in `src-tauri/src/window_host/` (NFR3).
- No `#[cfg(unix)]` / `#[cfg(windows)]` branch may be added; encoding and routing
  derive from winit's platform-independent `ButtonSource` / `MouseScrollDelta` /
  modifier state (NFR4).
- No new test framework crate and no E2E harness (NFR5); the project has no E2E
  harness, so end-to-end confirmation is user-driven (TS10).
- Reports must use the existing `Tab::write_input` path rather than a new channel
  (FR11, assumption A8).
- Coordinates come from the existing `WindowHost::pixel_to_cell` grid mapping and
  are 1-based screen coordinates (assumption A9).
- DECSET 1005 keeps its existing `MODE_ACTION_TS_FALLBACK` arm and DECSET 1015
  keeps falling through the unknown-mode arm (assumption A7).

### 9.2 ビジネス上の制約

- No new user setting may be introduced (NFR2, assumption A2).
- macOS is out of scope (NFR4).

### 9.3 スケジュール制約

None recorded in the resolved requirements.

### 9.4 宣言された変更集合

このフィーチャー固有のパスは手動で列挙せず、create-plan で `workflow.yaml` の各タスクの `files` から導出する（`references/phases/create-plan-phase.md`）。

**デフォルトメンバー**（SPEC作成者が明示的に除外しない限り、常に宣言に含まれる）:
- `feature-docs/mouse-reporting/**`
- `test-docs/mouse-reporting/**`

`feature-docs/mouse-reporting/**` に含まれるもの: `REQUIREMENTS.md`、`SPEC.md`、`IMPLEMENTATION.md`、`workflow.yaml`、`phase-state/`、`tasks/`、`reviews/roundN.yaml`、`VERIFICATION.md`、`retrospect.yaml`、およびデザインステップが生成するデザイン成果物。生成主体は各フェーズドキュメントおよび `references/phase-state.md` を参照（引用のみ、ルールは再掲しない）。

`test-docs/mouse-reporting/**` に含まれるもの: `{T}.tests.yaml`（パス形式: `test-docs/mouse-reporting/{T}.tests.yaml`）。生成主体は `implement-phase.md` を参照（引用のみ、ルールは再掲しない）。

**意味論**:
- デフォルトのメンバーは、SPEC作成者が明示的に除外しない限り宣言に含まれる。除外は意図的な絞り込みであり、記載漏れによる省略ではない。
- この宣言はスーパーセット（superset）の主張であり、実際の変更集合は宣言に含まれる（CONTAINED IN）必要がある。実際には生成されないパスが宣言されていても違反にはならない。implementタスクを1つも生成しないフィーチャーは `test-docs/mouse-reporting/` ディレクトリを生成しないが、宣言された `test-docs/mouse-reporting/**` は依然として正しい。

## 10. 想定される課題とリスク

### 10.1 技術的課題

The resolved requirements record no separate risk register; what follows renders
the assumptions that carry a decision risk, with the impact and reversibility
recorded for each.

| 前提 | 内容 | 影響度 | 可逆性 |
|------|------|--------|--------|
| A1 | Wheel notches are reported on both screens while tracking is active; scrollback scroll and DECSET 1007 apply only when no tracking mode is active. | 高 | 可逆 |
| A2 | No new setting is introduced; emission follows the application's DECSET request alone. | 中 | 可逆 |
| A3 | Shift is the sole local override; while a tracking mode is active, bare drag reports instead of selecting text. | 高 | 可逆 |
| A4 | In the X10 encoding, a column or row above 223 produces no report; coordinates are never clamped. | 中 | 可逆 |
| A5 | Motion reports for 1002/1003 are emitted only when the reported cell changes. | 中 | 可逆 |
| A6 | `MODE_ACTION_TS_FALLBACK` is a dead path in the current native build, so moving 1000/1002/1003/1006 off it changes no live consumer other than the test that pins it. | 中 | 可逆 |
| A7 | DECSET 1005 and 1015 are out of scope; 1005 stays on the fallback arm and 1015 stays a silent no-op. | 低 | 可逆 |
| A8 | Reports are written via the existing `Tab::write_input` path rather than a new channel. | 低 | 可逆 |
| A9 | Coordinates are 1-based and derived from the existing `WindowHost::pixel_to_cell` grid mapping (screen row, not absolute buffer row). | 中 | 可逆 |

### 10.2 ビジネスリスク

None recorded in the resolved requirements beyond the assumption impacts above.

## 11. 成功基準

### 11.1 受け入れ基準

- [ ] AC1 — `CSI ? 1000 h` sets the normal-tracking mode bit and returns `MODE_ACTION_NONE`; `CSI ? 1000 l` clears it. The same holds for 1002, 1003 and 1006. (FR1)
- [ ] AC2 — With 1000 active, a left press at grid cell (col 1, row 1) emits `CSI M` followed by bytes 32, 33, 33, and its release emits `CSI M` followed by bytes 35, 33, 33. (FR2, FR5)
- [ ] AC3 — With 1000 and 1006 both active, the same press emits `CSI < 0 ; 1 ; 1 M` and the release emits `CSI < 0 ; 1 ; 1 m`. (FR2, FR5)
- [ ] AC4 — With 1000 active (no 1002/1003), pointer motion emits nothing. With 1002 active, motion with no button held emits nothing, while motion with the left button held emits a report whose button code includes the motion bit 32. With 1003 active, motion with no button held emits a report with button code 3 plus the motion bit. (FR3)
- [ ] AC5 — With 1002 or 1003 active and the pointer moving within one cell, exactly one motion report is emitted; crossing into the next cell emits a second. (NFR1)
- [ ] AC6 — With any tracking mode active, a wheel-up notch on the MAIN screen emits button code 64 and does not change `App::scroll_offset`; a wheel-down notch emits 65. The same holds on the ALTERNATE screen, and no DECSET 1007 arrow bytes are written. (FR4)
- [ ] AC7 — With no tracking mode active, wheel behaviour is byte-for-byte what it is today: DECSET 1007 arrow translation on the alternate screen when the mode bit and `settings.alternate_scroll_enabled` are both on, scrollback scroll otherwise. (FR4)
- [ ] AC8 — With a tracking mode active, a Ctrl+left press emits a report whose button code has bit 16 set and does not open the hovered link; a middle press emits button code 1 and does not paste PRIMARY. (FR6, FR8)
- [ ] AC9 — With a tracking mode active, Shift+drag produces no report and produces a text selection; Shift+Ctrl+click produces no report and opens the hovered link; Shift+middle-click produces no report and pastes PRIMARY; Shift+wheel produces no report and scrolls eMterm's scrollback. (FR7)
- [ ] AC10 — No emitted report ever has the Shift bit (value 4) set in its button code. (FR6, FR7)
- [ ] AC11 — With 1000 active and 1006 inactive, an event at column 224 or row 224 emits no bytes at all; the same event with 1006 active emits `CSI < 0 ; 224 ; ... M` carrying the true coordinate. (FR9, FR5)
- [ ] AC12 — With a tracking mode active, a press on the tab bar, on the status bar, on the scrollbar overlay, on the mux sidebar (persistent or overlay), on the CSD edge-resize hot zone, or while the profile selector is visible, emits no report and retains its current local behaviour. (FR10)
- [ ] AC13 — `crates/app_settings`, `src-tauri/web-shared/settings/types.ts` and the settings panel are untouched by the change. (NFR2)
- [ ] AC14 — `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml --no-default-features` succeeds. (NFR3)
- [ ] AC15 — `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path crates/term_core/Cargo.toml --lib` and `... --manifest-path src-tauri/Cargo.toml --lib` both pass, including the updated `test_mode_ts_fallback`. (NFR5, reference_impact RI1)
- [ ] AC16 — The merged change contains unit tests asserting the exact emitted byte sequence for: an X10 press, an X10 release, an SGR press, an SGR release, a motion report, a wheel-up and a wheel-down report, a Ctrl-modified report, an Alt-modified report, and an X10 event beyond column/row 223 (asserting no bytes). Each runs without a winit window or a live PTY. (NFR7, NFR6)

### 11.2 KPI

None recorded in the resolved requirements.

## 12. テストシナリオ

### 12.1 テスト観点

- [ ] 正常系: press / release / motion / wheel reporting under each tracking mode
      and both encodings (TS1, TS3, TS4, TS8, TS9, TS10).
- [ ] 異常系: Shift suppresses reporting and routes to local handling (TS5,
      TS11); egui chrome guards consume the event and nothing is reported (TS12).
- [ ] 境界値: X10 coordinate 223 encodes, 224 emits nothing (TS6); motion within
      one cell emits exactly one report (TS7).
- [ ] セキュリティ: no security scenario is recorded in the resolved
      requirements.
- [ ] パフォーマンス: motion report volume is capped at cell granularity (TS7,
      NFR1).

### 12.2 テストシナリオ一覧

| ID | 種別 | タイトル | 対象要件 | 実施場所 |
|----|------|----------|----------|----------|
| TS1 | unit | DECSET 1000/1002/1003/1006 set and clear their mode bits | FR1 | `crates/term_core/src/csi_modes.rs` — inline `#[cfg(test)] mod tests` |
| TS2 | unit | `test_mode_ts_fallback` updated for the narrowed fallback set | FR1 | `crates/term_core/src/csi_modes.rs` — existing test, must be edited |
| TS3 | unit | X10 encoding of press, release and motion | FR2, FR3, FR5, NFR7 | `src-tauri/src/window_host/` (encoding helper module) — inline `#[cfg(test)] mod tests` |
| TS4 | unit | SGR (1006) encoding of press and release | FR2, FR5, NFR7 | `src-tauri/src/window_host/` (encoding helper module) |
| TS5 | unit | Modifier bits: Ctrl sets 16, Alt sets 8, Shift never appears | FR6, FR7, NFR7 | `src-tauri/src/window_host/` (encoding helper module) |
| TS6 | unit | X10 coordinate overflow yields no report | FR9, NFR7 | `src-tauri/src/window_host/` (encoding helper module) |
| TS7 | unit | Motion is filtered to cell changes | NFR1 | `src-tauri/src/window_host/` (cell-change filter helper) |
| TS8 | unit | Wheel routing decision is a pure three-way choice | FR4 | `src-tauri/src/window_host/input_translate.rs` — inline `#[cfg(test)] mod tests` |
| TS9 | unit | Motion reporting gate per tracking mode | FR3 | `src-tauri/src/window_host/` (routing helper) |
| TS10 | manual | Real application mouse interaction | FR2, FR3, FR4, FR8, FR11 | Manual verification against a release build (`make build`; `src-tauri/target-host/release/emterm`) |
| TS11 | manual | Shift override end to end | FR7, FR8 | Manual verification against a release build |
| TS12 | manual | Chrome guards unaffected | FR10 | Manual verification against a release build |
| TS13 | manual | Mouse reporting over a mux-attached remote pane | FR11 | Manual verification against a release build with `emterm mux` |

### 12.3 テストシナリオ詳細

- **TS1**: For each of 1000, 1002, 1003, 1006 assert `handle_set_mode(mode, true) == 0` (MODE_ACTION_NONE) and `get_mode(...)` is then true, and that the `false` call clears it. Mirrors the existing `decset_1007_toggles_alternate_scroll_bit` construction style.
- **TS2**: The existing test asserts 0xFF for `[1, 1000, 1002, 1003, 1005, 1006]`. It must be narrowed to the modes still on the fallback arm and gain positive assertions for the modes moved onto FR1's bits. See reference_impact RI1.
- **TS3**: Table-driven: (button, press/release, col, row, mods, motion flag) to expected byte sequence. Includes the (1,1) origin case from AC2 and the release-becomes-3 rule. Also covers the wheel button codes 64/65 in X10 form (FR4).
- **TS4**: Same table as TS3 with 1006 active; asserts the `M` / `m` final-byte split and that the press button code is retained on release. Includes the SGR wheel form.
- **TS5**: Asserts bit 16 for Ctrl, bit 8 for Alt, both together, and that no input state produces bit 4.
- **TS6**: col=223 and row=223 encode; col=224 or row=224 returns None under X10 and encodes normally under SGR. Boundary-exact per AC11.
- **TS7**: Feeds a sequence of (col,row) values with repeats and asserts the filter yields exactly the distinct consecutive cells, with the cached last-cell state carried across calls.
- **TS8**: Given (tracking_active, shift_held, alt_screen, mode_1007_bit, alternate_scroll_enabled) assert the chosen consumer is exactly one of report / arrow-translate / scrollback-scroll, covering both screens for the tracking_active case and preserving today's matrix for the tracking-inactive case. Sits alongside the existing `accumulate_alt_scroll_lines` / `alternate_scroll_wheel_bytes` tests.
- **TS9**: Given (active tracking mode, buttons-held count) assert whether motion reports at all: 1000 never, 1002 only when held, 1003 always.
- **TS10**: In eMterm run a mouse-aware application (e.g. `vim` with `:set mouse=a`, or `less -X`). Confirm click positions the cursor, drag selects inside the application, and the wheel scrolls the application's own view on BOTH the main and alternate screen. The project has no E2E harness, so this scenario is user-driven. This is the scenario that directly demonstrates the task's goal of operating a mouse-aware TUI application inside eMterm.
- **TS11**: With the same mouse-aware application running: Shift+drag selects eMterm text (PRIMARY updated), Shift+Ctrl+click over a URL opens it, Shift+middle-click pastes PRIMARY, Shift+wheel moves eMterm's scrollback. Then confirm bare Ctrl+click and bare middle-click reach the application instead.
- **TS12**: With a tracking mode active, click a tab's close button, drag the tab strip with the wheel, click the status bar, drag the scrollbar, click and wheel the mux sidebar in both persistent and overlay placement, and grab a CSD window edge. Each must behave exactly as before, with nothing reaching the application.
- **TS13**: Attach to a mux pane on a remote host, run a mouse-aware application there, and confirm reports reach it through the existing `write_input` transport.

## 13. 用語定義

| 用語 | 定義 |
|------|------|
| DECSET 1000 | Normal / button tracking: press and release are reported (FR1, FR2). |
| DECSET 1002 | Button-event / drag tracking: motion is reported while at least one button is held (FR1, FR3). |
| DECSET 1003 | Any-event tracking: motion is reported whether or not a button is held (FR1, FR3). |
| DECSET 1006 | SGR extended encoding of reports (FR1, FR5). |
| DECSET 1007 | Alternate-scroll mode, whose wheel-to-arrow translation is the existing behaviour preserved by FR4. |
| X10 encoding | `CSI M Cb Cx Cy`, with the button byte and the 1-based column and row each biased by 32 (FR5). |
| SGR encoding | `CSI < Cb ; Cx ; Cy M` for a press and `... m` for a release, with unbiased decimal button code and 1-based coordinates (FR5). |
| Motion bit | The value 32 added to the button code of a motion report (FR3). |
| Tracking mode active | Any of DECSET 1000 / 1002 / 1003 is set (FR2, FR4). |
| `MODE_ACTION_TS_FALLBACK` | The return value 1000/1002/1003/1006 currently take, a dead path left over from the pre-native WebView build (business objective 2, assumption A6). |

## 14. 確認事項

### 14.1 確認済み事項

- [x] Wheel routing while tracking is active (`requirement.wheel-routing`): wheel
      notches are reported as buttons 64/65 on both the main and the alternate
      screen; eMterm's scrollback scroll and the DECSET 1007 arrow translation
      apply only when no tracking mode is active. Recorded as assumption A1.
- [x] Whether a setting gates the feature (`requirement.settings-gate`): no new
      setting is introduced; emission follows the application's DECSET request
      alone. Recorded as assumption A2.
- [x] Modifier / click override policy (`requirement.modifier-click-override`):
      Shift is the sole local override, and Ctrl+click and middle-click are
      reported while tracking is active; bare drag reports instead of selecting,
      with selection reachable via Shift. Recorded as assumption A3.
- [x] X10 coordinate overflow (`edge.x10-coordinate-overflow`): a column or row
      above 223 produces no report; coordinates are never clamped. Recorded as
      assumption A4.
- [x] Motion granularity (`nfr.motion-granularity`): motion reports for
      1002/1003 are emitted only when the reported cell changes. Recorded as
      assumption A5.
- [x] Design step: skipped — the feature adds no user-visible UI surface and
      touches none of the three Material Design 3 mirror layers (see section 6).

Additional decisions recorded as assumptions without a question: A6
(`MODE_ACTION_TS_FALLBACK` is a dead path), A7 (DECSET 1005 and 1015 out of
scope), A8 (reports use the existing `Tab::write_input` path), A9 (1-based
screen coordinates from `WindowHost::pixel_to_cell`).

### 14.2 未確認・保留事項

None. Every functional and non-functional requirement in this document has
`status: resolved`; no requirement is `tbd`.

## 15. 参考資料

- SPEC.md: `feature-docs/mouse-reporting/SPEC.md` — the implementation-focused
  rendering of these requirements.
- `.claude/rules/core-architecture.md` — describes the TS side as the removed
  pre-native WebView build (cited by assumption A6).
- `.claude/rules/core-build-location.md` / `.claude/rules/core-commands.md` — the
  `CARGO_TARGET_DIR` build and test commands named in AC14, AC15 and TS10.
