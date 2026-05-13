# Implementation Plan: Native IME Integration (Phase 4-G)

## Overview

native-poc 側で X11 (XIM) / Wayland (zwp_text_input_v3) / Windows (IMM32) の IME クライアントを自前実装し、Phase 4-E で配線済みの `App::on_ime_{preedit,commit,focus_lost}` をそのまま受け側として再利用する。Phase 4 で deferred になった `NFR3 (Linux fcitx5 IME parity)` および `FR11 / FR12` の manual gate を達成する。

## Objectives

- Linux X11 + fcitx5 / IBus で preedit / commit / IME on-off が動作する状態にする
- Linux Wayland + fcitx5-wayland で同等の動作を実現する
- Windows + MS-IME / Google IME で preedit / commit が動作する状態にする
- `EMTERM_NATIVE_IME` 環境変数 / `settings.ime.native_integration` で明示的にフォールバックできる経路を残し、IM サーバ未起動でもターミナルが落ちないことを保証する
- Phase 4-E の auto-scope (`ime::preedit::State`, `ime::commit::write_commit`, `render::cursor::draw_cursor_with_preedit`, `App::on_ime_*`) を変更しない
- 旧 `src-tauri` の build / test を一切触らない

## Prerequisites

### Development Environment

- 既存 Rust workspace + Docker E2E イメージ (`docker compose -f docker-compose.e2e.yml`)
- 実機 manual gate 用:
  - Linux X11 + fcitx5 / IBus
  - Linux Wayland + fcitx5-wayland (KDE Plasma 6 または Sway)
  - Windows 10/11 + MS-IME / Google 日本語入力

### Dependencies

- Phase 4 (`doc/tasks/mux-tabs-windows-ime/`) 完了。とくに以下は変更しない:
  - `native-poc/src/ime/{preedit.rs, commit.rs, mod.rs}` の既存内容 (Phase 4-E 範囲は再エクスポートの追加のみ可)
  - `App::on_ime_preedit / on_ime_commit / on_ime_focus_lost`
  - `render::cursor::draw_cursor_with_preedit`
- `raw-window-handle` 0.6 が workspace 依存にあること (tao 0.34 経由で取得可)
- `crossbeam-channel` が workspace 依存にあること (Wayland thread → main thread)

## Architecture Overview

### Technology Stack

- **Language**: Rust (workspace pinned)
- **Window / Event loop**: tao 0.34 (継続使用)
- **IME プロトコル実装**:
  - Linux X11: `x11-dl` (dynamic loading)
  - Linux Wayland: `wayland-client` + `wayland-protocols` (`unstable` feature)
  - Windows: `windows` crate (IMM32 + `SetWindowSubclass`)
- **Key libraries (既存)**: `raw-window-handle`, `crossbeam-channel`, `log`

### Design Approach

ボトムアップ + 段階リリース。最初に `ImeBackend` trait + `NullBackend` + App-side pump + opt-out / fallback 配線をすべて入れ、IME 動作を一切変えない (regression なし) ことを保証する。そのあと OS ごとに backend 実装を追加する。各 OS の go/no-go は manual gate で判断する。

Phase 4-E の sanitize / write_commit / focus_lost ルーティングはそのままで、各 backend は `ImeEvent` を queue に積み、`App::pump_ime` が drain して既存の `on_ime_*` に流すだけ。これにより SPEC.md「Phase 4-E の auto-scope を変更しない」契約を担保する。

### Component Interaction

```
[tao::EventLoop]
  WindowEvent::KeyboardInput ──▶ ImeBackend::dispatch_key_event
                                    ├─ Consumed   → (skip tao_key_to_bytes)
                                    └─ Passthrough → 既存 tao_key_to_bytes パス
  WindowEvent::Focused(b)    ──▶ ImeBackend::notify_focus(b)
                                    + (b == false) App::on_ime_focus_lost
  WindowEvent::ReceivedImeText ──▶ App::on_ime_commit (NullBackend fallback パスのみ)

[event-loop tick]
  ImeBackend::pump(&mut events)
    └─▶ App drains:
          ImeEvent::Preedit(s)   → App::on_ime_preedit(&s)
          ImeEvent::Commit(s)    → App::on_ime_commit(&s)
          ImeEvent::FocusOut     → App::on_ime_focus_lost()

[App per-frame cursor diff]
  cursor cell (row, col) 変化時 → ImeBackend::notify_cursor_rect(x, y, w, h)
```

## Implementation Phases

### Sub-Phase 4-G-A: 共通基盤 (ImeBackend trait + NullBackend + App pump + opt-out / fallback)

**Goal**: backend 抽象と App-side pump を入れ、`EMTERM_NATIVE_IME` env / `settings.ime.native_integration` / 初期化失敗 → `NullBackend` フォールバックを成立させる。OS backend はまだ未実装 (どの環境でも実質 Phase 4 と同じ動作)。

**Files to Create**:
- `native-poc/src/ime/backend.rs` — `ImeBackend` trait, `ImeEvent` enum, `KeyDispatchResult` enum, `ImeInitError` enum, `RawKeyEvent` adapter, `ImeBackendFactory` (`Box<dyn ImeBackend>` を返す startup-side コンストラクタ)
- `native-poc/src/ime/null.rs` — `NullBackend` (passthrough only)
- `native-poc/src/ime/settings.rs` (もしくは既存 `settings.rs` への extension で吸収) — `ImeSettings { native_integration: bool }` 構造体 + デフォルト + validation

**Files to Modify**:
- `native-poc/src/ime/mod.rs` — `pub mod backend; pub mod null;` の追加と OS 別 cfg backend の `pub mod` 追加 (中身は次 Phase で実装)。既存の `preedit` / `commit` 再エクスポートは保持
- `native-poc/src/settings.rs` — `ImeSettings { native_integration: bool }` 構造体追加 + `Settings` への field 追加 + `Default::default()` で `native_integration: true`。`settings.json` からの実 load は Phase 7 のローダ実装に委ねる (現 `Settings` は他フィールド同様 `#[allow(dead_code)] // Phase 7` 扱い)。Phase 4-G では `Settings::default()` 経路でのみアクセスされる
- `native-poc/src/app.rs` — `App` に `ime_backend: Box<dyn ImeBackend>` フィールド追加、`App::pump_ime` (event drain) 追加、`App::notify_cursor_rect_if_changed`(cell-diff 駆動) 追加、`App::dispatch_key_event_via_ime(...)` ヘルパ追加。既存の `on_ime_*` メソッドは変更しない
- `native-poc/src/window_host.rs` — startup で `ImeBackendFactory::build()` を呼び `App` に注入、`KeyboardInput` ハンドラを `ImeBackend::dispatch_key_event` 優先に変更 (Passthrough のときのみ既存 `tao_key_to_bytes` へ)、`Focused(b)` ハンドラから `ImeBackend::notify_focus(b)` を呼ぶ、event-loop tick の終端で `App::pump_ime` を呼ぶ。`ReceivedImeText` は **NullBackend が active な時のみ** 既存パス (`on_ime_commit`) に流す (実 backend active 時は `Commit` イベントが Backend から来るので二重コミットを避ける)
- `native-poc/Cargo.toml` — IME backend 関連 dep 追加 (詳細は各 OS phase)。`raw-window-handle` は既に direct dep (`Cargo.toml:63`、Phase 3 系で導入済) なので追加変更不要

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `ImeBackend` (trait) | OS 別 IME 実装の唯一の seam | n/a | App から backend 種別を意識せず使える |
| `NullBackend` | passthrough のみ (`dispatch_key_event` → Passthrough、`pump` → 空 vec) | n/a | Phase 4 と完全同等の挙動 |
| `ImeBackendFactory::build(window, display, settings, env)` | env > settings > 初期化失敗判定で適切な backend を返す | startup で window handle 取得済み | `Box<dyn ImeBackend>`; 失敗時は `NullBackend` + warn 一発 |
| `App::pump_ime()` | tick ごとに `ime_backend.pump(&mut events)` → 各 event を `on_ime_*` にルーティング | backend 注入済み | events queue 空 (最大 1024 件/tick で打ち切り、超過分は warn IME_E901) |
| `App::notify_cursor_rect_if_changed()` | cursor cell が変わった時のみ `notify_cursor_rect` を呼ぶ | tab が存在 | rate-limited、毎フレーム呼ばない |
| `ImeSettings { native_integration: bool }` | settings.json の `ime` セクション | settings 読込済み | デフォルト true; 不正値で warn + true |

**Processing Flow** (opt-out / fallback decision tree):

1. startup で `ImeBackendFactory::build` を呼ぶ
   - 分岐1: `EMTERM_NATIVE_IME=0` (env) が set されている
      → `NullBackend` を返し warn 一発 (`"ime: native integration disabled (env)"`)
   - 分岐2: `settings.ime.native_integration == false`
      → `NullBackend` を返し warn 一発 (`"ime: native integration disabled (settings)"`)
   - 分岐3: 上記いずれでもない
      → 実 backend のコンストラクタを試行
        - 成功 → 実 backend (info ログ一発: `"ime: <protocol> initialized"`)
        - 失敗 (`ImeInitError`) → `NullBackend` + warn 一発 (`"ime: native integration disabled (<reason>)"`)
2. event-loop tick 終端で `App::pump_ime()` を呼ぶ
   - backend からの transport error 検出 (`pump` が err を内部 log + queue 空で返した複数 tick 連続など) は次フェーズ (4-G-B 以降) で具体化。基盤フェーズでは hook だけ用意

**Implementation Steps**:

1. **trait + 列挙型を定義** — `ImeBackend`, `ImeEvent`, `KeyDispatchResult`, `ImeInitError`, `RawKeyEvent` を `ime::backend` に追加。SPEC.md §API Design に従う
2. **NullBackend 実装** — すべて no-op / 空 vec を返す。`ImeBackend::init` は常に `Ok(NullBackend)`
3. **settings に `ime.native_integration` を追加** — `ImeSettings` 構造体 + `Settings` フィールド + `Default` で `true`。JSON parse は Phase 7 で実装するので Phase 4-G では `Default::default()` のみ exercise する (他フィールド同様 `#[allow(dead_code)] // Phase 7`)。`ImeBackendFactory::build` から `Settings::ime.native_integration` を参照する
4. **App に IME backend スロット + pump + cursor-rect diff を追加** — `App::pump_ime`, `App::notify_cursor_rect_if_changed`, `App::dispatch_key_event_via_ime`
5. **window_host の event loop に hook** — startup factory 呼び出し、`KeyboardInput` を backend dispatch 優先に、`Focused` を backend に通知、`ReceivedImeText` を NullBackend 時のみ既存パスに、tick 終端で `pump_ime`
6. **regression unit tests** — TS-backend-1..5, TS-cursor-1, TS-focus-1, TS-fallback-1..3, TS-settings-1, TS-route-1..2

**Dependencies**: Phase 4-E の auto-scope 完了が前提。`tao_key_to_bytes` の Ctrl/Alt-only Character ガード (`3fcc7ef`) は保持。`raw-window-handle` 0.6 は既に native-poc の direct dep (今回新規追加なし)

**Testing Approach**:
- Unit: TS-backend-1..5 (NullBackend / MockBackend 経由), TS-cursor-1, TS-focus-1, TS-fallback-1..3, TS-settings-1, TS-route-1..2 (Phase 4-E の regression guard)
- Integration: 該当なし (実 IM サーバ未投入)
- E2E: 該当なし (legacy E2E は Tauri のみ)
- Manual: 該当なし

**Acceptance Criteria**:
- [ ] `cargo test --workspace` exit 0、新規 +12 件以上
- [ ] Phase 4-E の `ime::{preedit, commit}` ファイルが diff で content 変更ゼロ (`git diff` で確認)
- [ ] `EMTERM_NATIVE_IME=0` で起動 → NullBackend + warn 一発
- [ ] `Settings::default()` を改変して `ime.native_integration = false` にした状態で起動 → NullBackend + warn 一発 (Phase 7 で実 JSON ローダから設定される経路の test surrogate)
- [ ] env / settings 設定なし → NullBackend (まだ実 backend が存在しないため) + warn 一発 (`Unavailable("no platform backend compiled in")` 相当)、ターミナル動作は Phase 4 と完全同等

**Estimated Effort**: medium

---

### Sub-Phase 4-G-B: Linux X11 (XIM) backend

**Goal**: Phase 4-G の Go / No-Go 判定対象。fcitx5 / IBus と XIM で対話する `X11Backend` を実装し、`TS-manual-ime-x11` / `TS-manual-ime-x11-ibus` を達成する。

**Files to Create**:
- `native-poc/src/ime/x11.rs` — `X11Backend` 本体 (XIM open / IC create / XFilterEvent / XmbLookupString / IM callbacks / XSetICFocus / XUnsetICFocus / XICAttribute XNSpotLocation / `Drop` で XDestroyIC + XCloseIM)

**Files to Modify**:
- `native-poc/src/ime/mod.rs` — `#[cfg(all(unix, not(target_os = "macos")))] pub mod x11;`
- `native-poc/src/ime/backend.rs` — `ImeBackendFactory::build` 内で `RawDisplayHandle::Xlib(_)` を runtime probe し `X11Backend::init` を呼ぶ
- `native-poc/Cargo.toml` — `[target.'cfg(all(unix, not(target_os = "macos")))'.dependencies]` に `x11-dl = "2"` を追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `X11Backend::init(window, display)` | tao が握る X11 Display を borrow し、`XOpenIM` + `XCreateIC` で IC をセットアップ | `RawDisplayHandle::Xlib`, `RawWindowHandle::Xlib`, X11 IM サーバ起動済み | 成功時 IC ready、失敗時 `ImeInitError::{Unavailable, HandleType, PlatformError}` |
| `X11Backend::dispatch_key_event(raw)` | tao の KeyEvent から synthetic `XKeyPressedEvent` を組み立て `XFilterEvent` に渡す | `init` 成功済み | filtered → `Consumed` + (callback 経由で後で `ImeEvent::Commit/Preedit` が pump で取得される) / not filtered → `Passthrough` |
| `X11Backend::pump(events)` | IM callbacks から積まれた `ImeEvent` を drain | dispatch_key_event 呼出後 | events vec に push (最大 1024 件/tick、超過は drop + warn IME_E901) |
| `X11Backend::notify_cursor_rect` | `XICAttribute` の `XNSpotLocation` 更新 | IC 生成済 | 候補ウィンドウがカーソル近傍に出る (best effort) |
| `X11Backend::notify_focus(true/false)` | `XSetICFocus` / `XUnsetICFocus` | IC 生成済 | IM サーバが focus state を把握する |
| `X11Backend::drop` | `XDestroyIC` + `XCloseIM` | IC / IM 保持 | リソース解放 |

**Processing Flow** (preedit + commit):

1. `WindowEvent::KeyboardInput` 到達
2. `App` が `ImeBackend::dispatch_key_event` を呼ぶ (window_host で 4-G-A で追加した hook)
3. `X11Backend::dispatch_key_event`:
   - 分岐1: tao key event から `XKeyPressedEvent` を組み立てる (keycode は tao の physical_key / scancode から逆算、modifier は `XKeyEvent::state` に詰める)
   - 分岐2: `XFilterEvent(&mut synthetic, window)` を呼ぶ
     - 戻り値 true → `XmbLookupString` で direct commit (status == `XLookupChars`) かどうかを確認、direct commit があれば `ImeEvent::Commit(text)` を queue へ; なければ後続 IM callback (`Preedit*Callback` / `StatusCallback`) で `ImeEvent::Preedit` / `ImeEvent::Commit` が積まれる。`KeyDispatchResult::Consumed` を返す
     - 戻り値 false → `KeyDispatchResult::Passthrough` を返す (App は既存 `tao_key_to_bytes` に流す)
4. tick 終端の `pump` で queue を drain
5. App が `on_ime_preedit` / `on_ime_commit` を呼ぶ → 既存 `sanitize` 経由

**Processing Flow** (IM サーバ突然死):

1. `XFilterEvent` 中に X 接続が落ちる、または `XCloseIM` が呼ばれた状態を検出
2. `X11Backend::pump` が backend health flag を false にする
3. `App` 側で health == false を検出したら `App::on_ime_focus_lost()` を呼んで preedit クリア
4. window_host が次の event-loop tick で `ImeBackendFactory::build` を再呼出 (focus-in トリガで再接続) → 失敗ならそのまま `NullBackend` 継続、成功なら再接続

**Implementation Steps**:

1. **X11 ハンドル取得** — `raw-window-handle::RawDisplayHandle::Xlib`, `RawWindowHandle::Xlib` を probe。それ以外は `ImeInitError::HandleType` を返す
2. **XIM open + IC create** — `XOpenIM` + `XCreateIC` (style: `XIMPreeditCallbacks | XIMStatusCallbacks` を最優先、フォールバック `XIMPreeditNothing | XIMStatusNothing`)
3. **IM callbacks 配線** — preedit start / draw / done / caret、status start / draw / done を callback で受けて internal queue に積む
4. **dispatch_key_event + XFilterEvent + XmbLookupString** — synthetic XKeyPressedEvent 経由のフィルタ + direct commit 取り出し
5. **notify_cursor_rect / notify_focus + Drop** — IC attribute 更新 + リソース解放
6. **Integration test TS-backend-int-1** — `#[ignore]` 付きで xvfb-run + minimal stub IM responder で commit が pump に届くまで E2E (`cargo test --ignored ime_x11_*` で起動)

**Dependencies**: 4-G-A 完了が前提。`raw-window-handle` direct dep が必要

**Testing Approach**:
- Unit: 内部の XKeyPressedEvent 組み立て (tao key → keycode 変換) の純粋関数化を行い、その関数を unit test (TS-x11-1: ASCII letter のマッピング、TS-x11-2: modifier mask)
- Integration: TS-backend-int-1 (`#[ignore]`, Docker host + xvfb)
- Manual: TS-manual-ime-x11 (fcitx5), TS-manual-ime-x11-ibus (IBus), TS-manual-ime-imserver-restart, TS-manual-ime-mux

**Acceptance Criteria**:
- [ ] `cargo build --workspace` (Linux) exit 0
- [ ] `cargo test --workspace` exit 0
- [ ] TS-manual-ime-x11 で fcitx5 経由の "nihongo" → "日本語" 入力が成立
- [ ] TS-manual-ime-x11-ibus で IBus 経由でも同等
- [ ] TS-manual-ime-imserver-restart: fcitx5 kill 後に fallback、再起動 + focus 再取得で復帰
- [ ] TS-manual-ime-mux: mux session 内でも commit が PTY (mux 経由) に届く
- [ ] Phase 1 fcitx5 acceptance criteria (`doc/tasks/ime-input-support/SPEC.md` US1-US5) パス

**Estimated Effort**: large

---

### Sub-Phase 4-G-C: Linux Wayland (zwp_text_input_v3) backend

**Goal**: Wayland セッションで `WaylandBackend` を実装し、`TS-manual-ime-wayland` を達成する。

**Files to Create**:
- `native-poc/src/ime/wayland.rs` — `WaylandBackend` 本体 (`zwp_text_input_manager_v3` bind / `zwp_text_input_v3` listener / 専用 event pump スレッド / `crossbeam_channel` 経由で main thread と通信 / `set_cursor_rectangle` / `enable` / `disable` / `Drop` で `destroy`)

**Files to Modify**:
- `native-poc/src/ime/mod.rs` — `#[cfg(all(unix, not(target_os = "macos")))] pub mod wayland;`
- `native-poc/src/ime/backend.rs` — `ImeBackendFactory::build` 内で `RawDisplayHandle::Wayland(_)` を runtime probe し `WaylandBackend::init` を呼ぶ (X11 probe より先または後の優先順位は env display backend に従う; runtime で来た handle variant をそのまま使う)
- `native-poc/Cargo.toml` — `[target.'cfg(all(unix, not(target_os = "macos")))'.dependencies]` に `wayland-client = "0.31"` + `wayland-protocols = { version = "0.31", features = ["unstable"] }` を追加

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `WaylandBackend::init(window, display)` | tao の Wayland display proxy を取得、`zwp_text_input_manager_v3` global を bind | `RawDisplayHandle::Wayland`, compositor に `zwp_text_input_manager_v3` 存在 | text-input 起動 + pump スレッド起動 / 失敗時 `ImeInitError::Unavailable` |
| `WaylandPumpThread` | Wayland 専用 event loop (`Connection::dispatch`) を回し、`commit_string` / `preedit_string` / `done` を crossbeam channel に push | スレッド起動 + Wayland connection 取得済 | channel に `ImeEvent` 流入 |
| `WaylandBackend::pump(events)` | crossbeam_channel から `ImeEvent` を drain | スレッド稼働中 | events vec に push (最大 1024 件/tick、超過は drop + warn IME_E901) |
| `WaylandBackend::notify_cursor_rect` | `set_cursor_rectangle(x, y, w, h)` (main thread から発行可能。これは proxy method なので thread-safe wrapper を経由) | text-input enabled | 候補ウィンドウが追従 |
| `WaylandBackend::notify_focus(true/false)` | `enable` / `disable` イベント送信 | text-input bind 済 | IM が focus を把握 |
| `WaylandBackend::dispatch_key_event` | 常に `Passthrough` を返す (Wayland では keyboard listener が真の入力ソース。tao の KeyboardInput はそのまま PTY に流す) | n/a | tao_key_to_bytes が走る。preedit 中の文字キーは Phase 4 同様 `ReceivedImeText` 経由でなく `Commit` 経由で来るので二重コミット回避は `App::pump_ime` 側のロジックで担保 |
| `WaylandBackend::drop` | text-input destroy + pump スレッド join | リソース保持 | クリーンアップ |

**Processing Flow**:

1. `WaylandPumpThread` が `Connection::dispatch` で blocking。`preedit_string` / `commit_string` / `done` を listener で受ける
2. listener: `preedit_string` → `ImeEvent::Preedit(text)` を channel に push、`commit_string` → `ImeEvent::Commit(text)` を push、`done` で flush
3. main thread の tick 終端で `WaylandBackend::pump` が channel から drain → `App` が `on_ime_*` を呼ぶ
4. cursor 移動時に main thread から `set_cursor_rectangle` を発行

**Implementation Steps**:

1. **Wayland display proxy 取得** — `raw-window-handle::RawDisplayHandle::Wayland` から `wl_display` ポインタを取得、`Connection::from_external_display` で `wayland-client` の Connection に変換
2. **text-input-manager bind + text-input 生成** — registry をスキャン、`zwp_text_input_manager_v3` 不在は `Unavailable`
3. **pump スレッド起動** — `crossbeam_channel::unbounded::<ImeEvent>()` で送受、listener コードを別スレッドで blocking dispatch
4. **focus / cursor rect / drain** — `notify_focus` で `enable`/`disable`、`notify_cursor_rect` で `set_cursor_rectangle`、`pump` で channel drain
5. **Drop でクリーンアップ** — text-input destroy + pump スレッド join
6. **regression / smoke tests** — pump スレッドの起動と channel 通信を mock compositor なしで検証する unit (`WaylandBackend::pump` を direct channel push でテスト): TS-wayland-1 (`#[cfg(test)]` で内部 channel に push したものが pump で drain される)

**Dependencies**: 4-G-A 完了が前提。`wayland-client` / `wayland-protocols` 新規 dep

**Testing Approach**:
- Unit: TS-wayland-1 (channel drain), TS-wayland-2 (`Unavailable` 経路: registry に manager がない時 `init` が `Unavailable` を返す — manager presence flag を `#[cfg(test)]` で注入可能にする)
- Integration: 該当なし (実 compositor 必須、host gate に降りる)
- Manual: TS-manual-ime-wayland (KDE Plasma 6 / Sway + fcitx5-wayland)

**Acceptance Criteria**:
- [ ] `cargo build --workspace` (Linux) exit 0、Wayland feature 有効
- [ ] `cargo test --workspace` exit 0
- [ ] TS-manual-ime-wayland on KDE Plasma 6 (KWin) で "nihongo" → "日本語" 成立
- [ ] TS-manual-ime-wayland on Sway で同等
- [ ] compositor が `zwp_text_input_manager_v3` を持たない時は warn + NullBackend にフォールバック (ターミナルは継続)

**Estimated Effort**: large

---

### Sub-Phase 4-G-D: Windows IMM32 backend

**Goal**: Windows で `WindowsBackend` を実装し、`TS-manual-ime-windows` を達成する。

**Files to Create**:
- `native-poc/src/ime/windows.rs` — `WindowsBackend` 本体 (`SetWindowSubclass` で wndproc 差し替え / `WM_IME_STARTCOMPOSITION` / `WM_IME_COMPOSITION` (`GCS_COMPSTR` / `GCS_RESULTSTR`) / `WM_IME_ENDCOMPOSITION` の購読 / `ImmGetCompositionStringW` で UTF-16 → UTF-8 / `ImmSetCompositionWindow` で `CFS_POINT` 報告 / `RemoveWindowSubclass` で `Drop`)

**Files to Modify**:
- `native-poc/src/ime/mod.rs` — `#[cfg(windows)] pub mod windows;`
- `native-poc/src/ime/backend.rs` — `ImeBackendFactory::build` 内で `cfg(windows)` の時 `WindowsBackend::init` を呼ぶ
- `native-poc/Cargo.toml` — `[target.'cfg(windows)'.dependencies]` に `windows = { version = "0.58", features = [...] }` を追加 (`Win32_UI_Input_Ime`, `Win32_UI_WindowsAndMessaging`, `Win32_UI_Shell` 系)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `WindowsBackend::init(window, display)` | `RawWindowHandle::Win32` から HWND を取得、`SetWindowSubclass` を installation | tao が Win32 window を生成済み | subclass installed / 失敗時 `ImeInitError::PlatformError` |
| `subclass wndproc` | `WM_IME_*` を識別、`ImmGetCompositionStringW` で文字列取得、UTF-16 → UTF-8、`ImeEvent` を thread-local queue に push、他メッセージは `DefSubclassProc` | subclass installed | tao は変更なく動作 |
| `WindowsBackend::pump(events)` | thread-local queue を drain | wndproc が events を積んだ後 | events vec に push (最大 1024 件/tick、超過は drop + warn IME_E901) |
| `WindowsBackend::notify_cursor_rect` | `ImmSetCompositionWindow(CFS_POINT, x, y)` | composition open または無条件 | 候補ウィンドウがカーソル近傍に出る (best effort) |
| `WindowsBackend::notify_focus(true/false)` | n/a (subclass が `WM_SETFOCUS` / `WM_KILLFOCUS` を `DefSubclassProc` に流すだけで OS が処理する) | n/a | focus は OS 任せ |
| `WindowsBackend::dispatch_key_event` | 常に `Passthrough` を返す (IMM32 は `WM_KEYDOWN` を independently に解釈して `WM_IME_*` を発火するため、tao の KeyboardInput とは独立) | n/a | tao_key_to_bytes が走る。preedit 中の文字キーは `WM_IME_COMPOSITION` で `Commit` 経由になるので、`tao_key_to_bytes` 側で同じキーが encode されないこと (要確認 — 実装中に Win32 で `WM_KEYDOWN` の tao 透過挙動を検証、二重入力が出るなら `App::pump_ime` 側で commit 直後の同等 ASCII を suppress するロジックを入れる) |
| `WindowsBackend::drop` | `RemoveWindowSubclass` | subclass installed | tao wndproc が pristine 状態に戻る |

**Processing Flow**:

1. OS が `WM_IME_STARTCOMPOSITION` を送信 → subclass: `ImeEvent::Preedit("")` を push、`DefSubclassProc` で tao にも流す
2. OS が `WM_IME_COMPOSITION` (`GCS_COMPSTR`) を送信 → subclass: `ImmGetCompositionStringW(GCS_COMPSTR)` で UTF-16 取得 → UTF-8 変換 → `ImeEvent::Preedit(text)` を push、`DefSubclassProc`
3. OS が `WM_IME_COMPOSITION` (`GCS_RESULTSTR`) を送信 → subclass: 同様に `GCS_RESULTSTR` を取得し `ImeEvent::Commit(text)` を push
4. OS が `WM_IME_ENDCOMPOSITION` を送信 → subclass はそのまま `DefSubclassProc`
5. main thread の tick 終端で `pump` が thread-local queue を drain → App routes to `on_ime_*`

**Implementation Steps**:

1. **HWND 取得 + subclass install** — `raw-window-handle::RawWindowHandle::Win32` 経由で HWND、`SetWindowSubclass` 呼出
2. **wndproc 実装** — `WM_IME_*` 分岐、`ImmGetCompositionStringW` 呼出 (バッファサイズ取得 → UTF-16 取得 → UTF-8 変換)、`ImeEvent` push、それ以外は `DefSubclassProc`
3. **UTF-16 → UTF-8 変換** — `String::from_utf16` を使い、失敗時は drop + warn (IME_E401)
4. **notify_cursor_rect** — `ImmGetContext` → `ImmSetCompositionWindow(CFS_POINT)` → `ImmReleaseContext`
5. **Drop** — `RemoveWindowSubclass`
6. **Integration test TS-backend-int-2** — `#[cfg(windows)]` + hidden HWND + `SendMessageW` で `WM_IME_COMPOSITION` を疑似送信し、pump 経由で `ImeEvent::Commit` が届くまで E2E

**Dependencies**: 4-G-A 完了が前提。`windows` crate 新規 dep

**Testing Approach**:
- Unit: UTF-16 → UTF-8 変換ヘルパ (TS-windows-1: BMP), TS-windows-2 (Surrogate pair), TS-windows-3 (invalid surrogate → drop + warn)
- Integration: TS-backend-int-2 (`#[cfg(windows)]` + hidden window)
- Manual: TS-manual-ime-windows (MS-IME + Google IME)

**Acceptance Criteria**:
- [ ] `cargo build --target x86_64-pc-windows-msvc --workspace` exit 0 (host または cross)
- [ ] `cargo test --workspace` exit 0
- [ ] TS-manual-ime-windows with MS-IME: "nihongo" → "日本語" 成立、preedit overlay と commit がそれぞれ 1 回ずつ
- [ ] TS-manual-ime-windows with Google IME: 同等
- [ ] 候補ウィンドウがカーソル近傍に出る (best effort、gating ではない)

**Estimated Effort**: large

---

### Sub-Phase 4-G-E: 最終ゲート + パフォーマンス計測 + ドキュメント

**Goal**: パフォーマンスゲート、clippy / fmt、Phase 4 manual gate のクロスタスク更新、README 更新。

**Files to Create**:
- (なし; ドキュメント更新のみ)

**Files to Modify**:
- `native-poc/README.md` — Phase 4-G feature matrix (Linux X11 / Wayland / Windows + fallback) と env / settings 説明を追記
- `doc/tasks/ime-native-integration/VERIFICATION_RESULT.md` — sdd.6-verify が作成 (この phase では作らない)
- 必要に応じて `doc/tasks/mux-tabs-windows-ime/sdd.yaml` の NFR3 / FR11 / FR12 manual gate を notes だけ追記 (本 SDD の責務外なので「ゲート flip は別 PR で」と memo)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| performance harness (instrumented binary) | `App::on_ime_preedit` 入口と `WindowHost::request_redraw` 間の `Instant::now()` delta を log | release build | TS-perf-3 / TS-perf-4 / TS-perf-regression の数値を記録 |
| final gate | `cargo fmt --all --check`, `cargo clippy -p emterm-native-poc -- -D warnings`, `cargo build --workspace`, `cargo test --workspace` | 全 sub-phase merged | exit 0 |
| README 更新 | Phase 4-G の feature matrix と env / settings | implementation 完了 | matrix 更新 |

**Processing Flow**:

1. release build で TS-perf-3 / TS-perf-4 を計測 (Linux X11 host)、結果を `tmp/phase-4g-perf.txt` 等に保存
2. Phase 4 の `TS-perf-1` / `TS-perf-2` を re-run、Phase 4 baseline と比較 (+10% 以内)
3. fmt / clippy / build / test 最終ゲート
4. README に Phase 4-G feature matrix と env / settings の使い方を追記
5. VERIFICATION_RESULT.md は sdd.6-verify で作成 (本 phase の責務ではない)

**Implementation Steps**:

1. **Performance instrumentation** — `App::on_ime_preedit` / `App::on_ime_commit` 入口で `Instant::now()` を log::debug (release では出ないので、TS-perf-3/4 計測時のみ `log::warn` 一時化 or feature gate)
2. **fmt + clippy sweep** — mechanical fixes
3. **README 更新** — Phase 4-G の feature matrix
4. **Phase 4 cross-task notes** — `doc/tasks/mux-tabs-windows-ime/sdd.yaml` に「NFR3 / FR11 / FR12 は Phase 4-G で完遂、別 PR で flip 予定」のメモ (本 SDD では flip しない)

**Dependencies**: 4-G-A〜4-G-D が完了済み

**Testing Approach**:
- Unit / Integration: 再実行
- E2E: 該当なし
- Manual: TS-perf-3 / TS-perf-4 / TS-perf-regression を release host で計測

**Acceptance Criteria**:
- [ ] `cargo fmt --all -- --check` exit 0
- [ ] `cargo clippy -p emterm-native-poc -- -D warnings` exit 0 (forward-staged な warning は notes に記録した上で許容)
- [ ] `cargo build --workspace` exit 0
- [ ] `cargo test --workspace` exit 0
- [ ] TS-perf-3: preedit redraw < 30 ms (Linux X11 release host)
- [ ] TS-perf-4: commit → PtySession::write < 5 ms
- [ ] TS-perf-regression: IME-OFF key 入力 latency が Phase 4 baseline + 10% 以内
- [ ] README Phase 4-G matrix 更新済み

**Estimated Effort**: small (実装はほぼなく、計測 + ドキュメント中心)

---

## Complete File Structure

```
native-poc/
├── Cargo.toml                           # MODIFY: [target.'cfg(all(unix, not(target_os="macos")))'.dependencies] +x11-dl,
│                                        #                                                              +wayland-client,
│                                        #                                                              +wayland-protocols,
│                                        #         [target.'cfg(windows)'.dependencies] +windows
│                                        #         (raw-window-handle 0.6 は既に direct dep)
├── README.md                            # MODIFY (4-G-E): Phase 4-G feature matrix
└── src/
    ├── app.rs                           # MODIFY (4-G-A): ime_backend field + pump_ime + cursor diff
    ├── window_host.rs                   # MODIFY (4-G-A): startup factory, KeyboardInput dispatch, Focused notify, pump_ime tick
    ├── settings.rs                      # MODIFY (4-G-A): ime.native_integration: bool (default true)
    └── ime/
        ├── mod.rs                       # MODIFY: pub mod backend; pub mod null; + cfg backends
        ├── preedit.rs                   # UNCHANGED (Phase 4-E auto-scope)
        ├── commit.rs                    # UNCHANGED (Phase 4-E auto-scope)
        ├── backend.rs                   # NEW (4-G-A): trait + enums + factory
        ├── null.rs                      # NEW (4-G-A): NullBackend
        ├── x11.rs                       # NEW (4-G-B): X11Backend (cfg unix, not macos)
        ├── wayland.rs                   # NEW (4-G-C): WaylandBackend (cfg unix, not macos)
        └── windows.rs                   # NEW (4-G-D): WindowsBackend (cfg windows)
```

## Testing Strategy

- **Unit**: backend 抽象 / NullBackend / settings / fallback / route guard / 各 OS の純粋関数 (X11 keycode 変換、UTF-16 → UTF-8 変換、Wayland channel drain)
  - TS-backend-1..5, TS-cursor-1, TS-focus-1, TS-fallback-1..3, TS-settings-1, TS-route-1..2 (4-G-A)
  - TS-x11-1..2 (4-G-B)
  - TS-wayland-1..2 (4-G-C)
  - TS-windows-1..3 (4-G-D)
- **Integration**: 実 OS / IM サーバ依存があるものは `#[ignore]` 付きで harness を入れる
  - TS-backend-int-1 (X11, xvfb + stub IM responder) — 4-G-B
  - TS-backend-int-2 (Windows, hidden HWND + `SendMessageW`) — 4-G-D
- **E2E**: 既存 `./scripts/run-e2e-docker.sh` は legacy Tauri 向けで native-poc には適用外。Phase 4-G では new E2E は追加しない (regression 検査として既存 fail list と差分なしを確認)
- **Manual**: host 実機 gate
  - TS-manual-ime-x11, TS-manual-ime-x11-ibus, TS-manual-ime-imserver-restart, TS-manual-ime-mux (4-G-B)
  - TS-manual-ime-wayland (4-G-C)
  - TS-manual-ime-windows (4-G-D)
  - TS-manual-ime-fallback (4-G-A 以降どこでも)
  - TS-perf-3, TS-perf-4, TS-perf-regression (4-G-E)

## Dependencies

| Package | Version | Purpose | Target |
|---------|---------|---------|--------|
| `x11-dl` | 2 | XIM bindings (dynamic loading) | `cfg(all(unix, not(target_os = "macos")))` |
| `wayland-client` | 0.31 | Wayland IPC | 同上 |
| `wayland-protocols` | 0.31 (`unstable` feature) | `zwp_text_input_v3` | 同上 |
| `windows` | 0.58 | IMM32 + `SetWindowSubclass` | `cfg(windows)` |
| `raw-window-handle` | 0.6 (既存 direct dep、追加変更なし) | window / display handle | all |
| `crossbeam-channel` | (既存) | Wayland thread → main thread の event 受け渡し | `cfg(all(unix, not(target_os = "macos")))` |

新規 third-party crate: `x11-dl`, `wayland-client`, `wayland-protocols`, `windows`。

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| tao 0.34 が X11 内部で別 `XOpenDisplay` を握っていて競合 | 中 | 高 | tao の Display を `raw-window-handle::RawDisplayHandle::Xlib` 経由で borrow。second `XOpenDisplay` は呼ばない。それでも競合する場合は X11 backend 全体を `XSync` で fence する fallback を準備 |
| Wayland compositor によって `zwp_text_input_v3` の挙動差 | 中 | 中 | KDE Plasma 6 と Sway を最初の Go ターゲットにし、GNOME は best effort |
| `SetWindowSubclass` が tao の event dispatch と相互作用して挙動が壊れる | 低 | 中 | 全 message を `DefSubclassProc` に forward (`WM_IME_*` だけ peek)。tao の wndproc には影響しない |
| Wayland pump スレッドの crossbeam_channel 不整合 / Drop 順 | 中 | 中 | Drop で channel sender を先に close → pump スレッド `recv` が `RecvError` で抜ける → join。テストで lifecycle pin |
| 4-G-A の `KeyboardInput` パス変更で IME-OFF regression | 中 | 高 | TS-backend-5 + TS-perf-regression で gating。NullBackend active 時は Phase 4 と完全同一の経路を維持 (`dispatch_key_event` が常に `Passthrough` → 既存 `tao_key_to_bytes`) |
| Windows で `WM_KEYDOWN` の tao 経路と `WM_IME_COMPOSITION` の Commit 経路が二重入力 | 中 | 中 | manual gate で確認、必要なら `App::pump_ime` 側で commit 直後の同 ASCII keydown を suppress するロジック追加 (4-G-D の Implementation Steps 段階で再評価) |
| TS-perf-3 が 30 ms を満たさない | 低 | 中 | rate-limit (`notify_cursor_rect` の cell-diff)、pump 件数上限 1024、event-loop の `request_redraw` 頻度を見直す。それでも超える場合は Phase 4 baseline との差分原因を切り分けて報告 |

## Open Questions

- [ ] OQ1: X11 crate 選定は `x11-dl` を採択 (SPEC.md §Open Questions OQ1 と整合)。`x11rb` は XIM サポートが薄いため不採用
- [ ] OQ2: Wayland binding は `wayland-client` 直叩きを採択 (SPEC.md OQ2 と整合)。`smithay-client-toolkit` は overkill
- [ ] OQ3: Wayland compositor 最低保証は KDE Plasma 6 + Sway。GNOME は best effort (SPEC.md OQ3)
- [ ] OQ4: Windows TSF は Phase 4-G スコープ外。IMM32 で不足が判明したら別 SDD (SPEC.md OQ4)
- [ ] OQ5: `notify_cursor_rect` のサブセル精度は不要 (cell-aligned で十分。SPEC.md OQ5)
- [ ] **Implementation-specific**: 4-G-D 時点で確認すべき項目 — Windows で `WM_IME_COMPOSITION` の commit 文字に対応する `WM_KEYDOWN` を tao が同時に `KeyboardInput` として App に届けるかどうか。届く場合は二重入力になるので、`pump_ime` か `dispatch_key_event` 側で suppress する
- [ ] **Implementation-specific**: 4-G-B で X11 IM サーバ突然死後の reconnect トリガ — focus-in イベント時に再 init を試みるか、event-loop tick 毎にバックオフ付きで再試行するかは 4-G-B 実装時に決定

## Success Metrics

- [ ] FR1-FR10 / NFR1-NFR8 すべて実装 + tests / manual gate でカバー
- [ ] `cargo build --workspace` (Linux + Windows) exit 0
- [ ] `cargo test --workspace` exit 0
- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy -p emterm-native-poc -- -D warnings` clean (forward-staged warning は notes に明記すれば許容)
- [ ] TS-manual-ime-x11 / TS-manual-ime-x11-ibus / TS-manual-ime-wayland / TS-manual-ime-windows / TS-manual-ime-fallback / TS-manual-ime-imserver-restart / TS-manual-ime-mux すべて pass
- [ ] TS-perf-3 (< 30 ms), TS-perf-4 (< 5 ms), TS-perf-regression (+10% 以内) 達成
- [ ] Phase 4-E `ime::{preedit, commit}` のファイル content が unchanged (`git diff` で確認)
- [ ] 旧 `src-tauri` build / test affected なし (NFR5)
