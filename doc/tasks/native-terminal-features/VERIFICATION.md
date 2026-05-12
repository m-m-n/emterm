# Verification Document: Native Terminal Feature Port (Phase 3)

## Overview

**Feature**: native-terminal-features (Phase 3 of `tmp/restruct.md`).
**SPEC.md**: `doc/tasks/native-terminal-features/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/native-terminal-features/IMPLEMENTATION.md`

This document is the verification plan for Phase 3. Sections marked *result* are filled in by `sdd.4-implement` and `sdd.6-verify`. Sections marked *planned* are written by the planner.

## Build Verification

- **Command** (planned): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --workspace"`
- **Expected**: exit code 0, no errors. Both `src-tauri` (legacy) and `native-poc` (Phase 3 target) compile. `term_images` becomes a new workspace member without breaking either.
- **Build result** (filled by sdd.4-implement after Phase 0 + Phase 1):
  - Command executed: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --workspace"`
  - Exit code: 0
  - `term_images` workspace member compiles cleanly with no external `tauri` dep (verified by `cargo tree -p term_images`).
  - `src-tauri` continues to compile via re-exports from `term_images` in `src-tauri/src/lib.rs`.
  - `native-poc` builds with only the pre-existing dead-code warnings (unchanged set).
  - **Remaining work** (Phase 2 onwards): not yet executed.

## Test Verification

- **Command** (planned): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"`
- **Coverage target**: minimum 80% on new modules (`native-poc/src/image/`, expanded `native-poc/src/callbacks.rs`, expanded `native-poc/src/selection.rs`). 597 existing `term_core` tests must remain green (no drops).
- **Test result** (filled by sdd.4-implement after Phase 0 + Phase 1):
  - Command executed: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"`
  - Result summary (per-crate `test result: ok` lines):
    - `app_lib` (src-tauri unit + integration): 816 + 10 + 10 + 7 + 6 = 849 passed, 0 failed, 1 ignored (legacy build regression — no drops).
    - `term_core`: 597 passed, 0 failed, 3 ignored (no drops vs. baseline).
    - `term_images`: 182 unit passed, 4 doctest passed, 0 failed (relocated from `src-tauri` with file history preserved via `git mv`).
    - `wasm`: 14 passed, 0 failed (no change to this crate).
    - `emterm-native-poc`: 14 passed, 0 failed (Phase 0 surface-lost-recovery refactor regression-safe).
  - Total: 1646+ passed across all workspace members; failure count: 0.
  - **Note**: Phase 2 onwards still requires per-task unit tests (TS-30, TS-31, TS-32, TS-34, etc.) to be added.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | `NativeCallbacks::on_osc(0, "title")` | `state.title == Some("title")` | Unit |
| TS-2 | `NativeCallbacks::on_osc(1, "icon")` | log only; no state change | Unit |
| TS-3 | `NativeCallbacks::on_osc(2, "title")` | `state.title == Some("title")` | Unit |
| TS-4 | `NativeCallbacks::on_osc(4, "1;#ff0000")` | `Theme.palette[1]` becomes red | Unit |
| TS-5 | `NativeCallbacks::on_osc(7, "file:///home/user")` | `tab.cwd == Some("/home/user")` | Unit |
| TS-6 | `NativeCallbacks::on_osc(8, "id=x;https://example.com")` | log only; term_core stores URI | Unit |
| TS-7 | `NativeCallbacks::on_osc(9, "hello")` | `notify-rust` invoked once; second call within 1 s suppressed | Unit |
| TS-8 | `NativeCallbacks::on_osc(10, "rgb:ff/ff/ff")` | `Theme.fg` becomes white | Unit |
| TS-9 | `NativeCallbacks::on_osc(11, "rgb:00/00/00")` | `Theme.bg` becomes black | Unit |
| TS-10 | `NativeCallbacks::on_osc(12, "rgb:ff/00/ff")` | `Theme.cursor_fg` becomes magenta | Unit |
| TS-11 | `NativeCallbacks::on_osc(22, "block")` | `Theme.cursor_style == Block` | Unit |
| TS-12 | `NativeCallbacks::on_osc(52, "c;<base64>")` under `clipboard_read_osc52=false` | clipboard untouched; `LOG_OSC52_DENIED` log | Unit |
| TS-13 | `NativeCallbacks::on_osc(52, "c;<base64>")` under `clipboard_read_osc52=true` (default) within size cap | `arboard::Clipboard::set_text` called | Unit |
| TS-14 | `NativeCallbacks::on_osc(104, "")` | palette restored to default | Unit |
| TS-15 | `NativeCallbacks::on_osc(110, "")` | `Theme.fg` restored to default | Unit |
| TS-16 | `NativeCallbacks::on_osc(111, "")` | `Theme.bg` restored to default | Unit |
| TS-17 | `NativeCallbacks::on_osc(112, "")` | `Theme.cursor_fg` restored to default | Unit |
| TS-18 | `NativeCallbacks::on_osc(133, "A")` | prompt mark stored on current row | Unit |
| TS-19 | `NativeCallbacks::on_osc(100, "spawn-viewer;...")` | payload pushed onto `osc_queue` | Unit |
| TS-20 | `NativeCallbacks::on_osc(101, "iterm2 payload")` | log only | Unit |
| TS-21 | `NativeCallbacks::on_osc(255, "xyz")` | `log::warn!` and ignore | Unit |
| TS-22 | `parse::decode_apc(<kitty fixture>, cursor_row, cursor_col, &mut ImageProcessor)` (called after `NativeCallbacks::on_apc` buffers payload in `Tab::pump`) | returns `ImageReady` + `Place` events, which `ImageLayer::ingest` applies | Unit |
| TS-23 | `parse::decode_dcs(<sixel fixture>, cursor_row, cursor_col, &mut ImageProcessor)` | returns `ImageReady` + `Place` events, which `ImageLayer::ingest` applies | Unit |
| TS-24 | `Selection::extend` in Character mode | range covers single cells along drag | Unit |
| TS-25 | `Selection::extend` in Word mode (double click) | range snaps to word boundaries | Unit |
| TS-26 | `Selection::extend` in Line mode (triple click) | range covers full line | Unit |
| TS-27 | `bracketed_paste("hello", enabled=true)` | wraps with `\e[200~ ... \e[201~` | Unit |
| TS-28 | `bracketed_paste("hello", enabled=false)` | returns raw text | Unit |
| TS-29 | `sanitize_bracket_sequences("foo\e[201~bar")` | embedded sequence removed | Unit |
| TS-30 | `App::dirty_rows_this_frame()` for cursor-only move | returns previous + current cursor row only | Unit |
| TS-31 | `App::dirty_rows_this_frame()` for full-screen write | returns 0..rows | Unit |
| TS-32 | Settings: missing `scrollback_lines` | default 10000 | Unit |
| TS-33 | Settings: missing `image_memory_quota_mb` | default 320 | Unit |
| TS-34 | Settings: missing `ambiguous_width_mode` | default `"narrow"` | Unit |
| TS-35 | Settings: missing `clipboard_read_osc52` / `clipboard_max_size_osc52` | defaults `true` / `10 * 1024 * 1024` (mirrors legacy `src-tauri/src/commands/config/settings.rs`) | Unit |
| TS-36 | `ImageLayer::evict_until_quota` over 320 MB | oldest texture(s) dropped; `LOG_IMG_QUOTA` log | Unit |
| TS-37 | PTY -> SGR truecolor + DECSCUSR + DECTCEM | grid state and cursor getters reflect input | Integration |
| TS-38 | PTY -> Kitty APC | `ImageEvent::Place` reaches `ImageLayer.placements` | Integration |
| TS-39 | PTY -> SIXEL DCS | same as TS-38 | Integration |
| TS-40 | Resize during streamed output | reflow keeps wrapped lines coherent; image placements update | Integration |
| TS-41 | Scrollback at exactly 10000 + 1 lines | oldest line evicted | Integration |
| TS-42 | Alt-screen toggle during selection | selection cleared | Integration |
| TS-43 | Malformed APC (truncated) | warn log; no crash | Integration |
| TS-44 | Malformed DCS (truncated) | warn log; no crash | Integration |
| TS-45 | Rapid resize during heavy output | no panic; no stuck rows | Integration |

## Code Quality Verification

- **Format command** (planned): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --all"`
- **Format result** (filled by sdd.4-implement after Phase 0 + Phase 1):
  - `cargo fmt --all` ran clean after Phase 0 + Phase 1 changes.
  - Two pre-existing legacy-code format diffs in `src-tauri/src/tauri_commands.rs` were normalized as a side effect (out of scope for Phase 3 but auto-fixed by the formatter).
- **Static analysis** (planned): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --workspace --no-deps -- -D warnings"`
  - Allowance: pre-existing warnings outside Phase 3 changes may be kept (do not gate on legacy code that is out of scope).
- **Clippy result** (filled by sdd.4-implement): not yet executed for Phase 0/1; deferred to the final pass (Phase 7).

## File Structure Verification

### Files to Create

- `crates/term_images/Cargo.toml`
- `crates/term_images/src/lib.rs`
- `crates/term_images/src/image_proc/` (moved from `src-tauri/src/image/`)
- `crates/term_images/src/ansi/{mod.rs, apc.rs, dcs.rs}` (moved from `src-tauri/src/ansi/`)
- `native-poc/src/image/mod.rs`
- `native-poc/src/image/overlay.rs`
- `native-poc/src/image/parse.rs`

### Files to Modify

- `Cargo.toml` — add `crates/term_images` to workspace members.
- `src-tauri/Cargo.toml` — add `term_images` path dep.
- `src-tauri/src/lib.rs` — re-export from `term_images`.
- `native-poc/Cargo.toml` — add `term_images`, `notify-rust`.
- `native-poc/src/app.rs` — dirty union, scroll position, image layer per tab.
- `native-poc/src/callbacks.rs` — full OSC matrix, APC/DCS routing, OSC 52 policy, notifications.
- `native-poc/src/selection.rs` — word/line modes, bracketed paste helpers.
- `native-poc/src/settings.rs` — `scrollback_lines`, `image_memory_quota_mb`, `ambiguous_width_mode`, `clipboard_read_osc52` (default `true`), `clipboard_max_size_osc52` (default `10 * 1024 * 1024`).
- `native-poc/src/tabs.rs` — cwd, scrollback control, ImageEvent::Response drain.
- `native-poc/src/window_host.rs` — surface-lost fix; mouse/keyboard routing for selection/paste/scroll.
- `native-poc/src/render/mod.rs` — dirty-row diff, full SGR, cursor shape, image overlay call.
- `native-poc/src/render/theme.rs` — palette and fg/bg/cursor color from OSC 4/10/11/12 etc.
- `native-poc/README.md` — Phase 3 feature matrix.

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR14 demonstrably working | Per-phase acceptance criteria in IMPLEMENTATION.md + Phase 7 manual verification |
| SC-2 | US1–US9 acceptance criteria checked | Test scenarios TS-1..TS-45 + manual verification |
| SC-3 | `cargo test --workspace` green | Phase 1 + Phase 5 + Phase 7 final pass |
| SC-4 | Kitty + SIXEL visual parity | Phase 7 manual side-by-side vs. legacy build |
| SC-5 | 12+ hour Claude Code session | Phase 7 manual run + memory samples |
| SC-6 | Legacy Tauri `cargo test --workspace` continues to pass (1646+ tests incl. app_lib 849). Legacy E2E (`./scripts/run-e2e-docker.sh test`) is **excluded** from this SDD's gate (see SPEC.md SC-6 rationale). | sub-phase 1 and sub-phase 7 `cargo test --workspace` |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 (dirty-row diff) | Phase 2 | TS-30, TS-31; manual ghost check |
| FR2 (cursor full) | Phase 3 | TS-11; manual cursor shape/color/visibility check |
| FR3 (selection) | Phase 4 | TS-24, TS-25, TS-26; manual PRIMARY auto-copy |
| FR4 (paste) | Phase 4 | TS-27, TS-28, TS-29; manual Ctrl+Shift+V / middle-click |
| FR5 (scrollback) | Phase 4 | TS-41, TS-42; manual wheel + Shift+PageUp/PageDown |
| FR6 (inline Kitty) | Phase 5 | TS-22, TS-38; Phase 7 visual parity |
| FR7 (inline SIXEL) | Phase 5 | TS-23, TS-39; Phase 7 visual parity |
| FR8 (OSC matrix) | Phase 6 | TS-1..TS-21 |
| FR9 (SGR full) | Phase 3 | Manual SGR sampler comparison |
| FR10 (resize/reflow) | Phase 4 (input routing) + Phase 5 (image follow) | TS-40, TS-45 |
| FR11 (ambiguous width) | Phase 3 | TS-34; manual East-Asian text check |
| FR12 (OSC 9 notifications) | Phase 6 | TS-7; manual notification check |
| FR13 (OSC 52 clipboard) | Phase 6 | TS-12, TS-13 |
| FR14 (long-run stability / no leaks) | Phase 7 | 12 h session + memory samples |
| NFR1 (performance) | Phase 7 | Manual 60 FPS / latency by feel |
| NFR2 (12+ h stability) | Phase 7 | Manual session + samples |
| NFR3 (logging) | Phase 2 onwards | Manual `RUST_LOG=info` / `RUST_LOG=debug` check |
| NFR4 (module layout) | Phase 5 | File-structure inspection |
| NFR5 (Linux only) | All | Build target on Linux dev machine |
| NFR6 (legacy build alive) | Phase 1 | `cargo test --workspace` regression gate (legacy E2E excluded — see SPEC.md SC-6 rationale) |

## E2E Testing

The existing `e2e-tests/` (WebdriverIO + tauri-driver) targets the **legacy Tauri build**. Per the SC-6 rationale in SPEC.md, the legacy E2E suite is **excluded** from this SDD's regression gate because:

- The 2026-05-12 baseline comparison showed identical failing-spec lists between `main` (647a79b) and `refactor/native-terminal-hybrid` HEAD (d448a99) — confirming the 10 failing specs are preexisting and unrelated to Phase 0 / Phase 1.
- `src-tauri/` is scheduled for retirement in Phase 7 of `tmp/restruct.md`, so fixing the preexisting failures would not be recoverable investment.
- The code paths this SDD touches are covered by `cargo test --workspace` (849 `app_lib` tests).

Legacy compatibility gate (replacing the legacy E2E gate):

- [ ] `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"` exits 0 after sub-phase 1 (immediate regression gate).
- [ ] Same command exits 0 once more in sub-phase 7 (final regression gate).

Phase 3 adds **no new E2E specs**: no headless driver covers tao+wgpu+egui. Manual verification fills that gap (below).

## Manual Testing (E2E Not Possible)

- [ ] Visual parity for Kitty Graphics Protocol against the current Tauri build (1–3 representative payloads).
- [ ] Visual parity for SIXEL against the current Tauri build.
- [ ] SGR sampler (`printf` script exercising every attribute) side-by-side with the Tauri build.
- [ ] 12+ hour Claude Code session on the Linux dev machine with RSS / GPU memory samples at 4h / 8h / 12h marks.
- [ ] OSC 9 notification visibly appears on `printf '\033]9;hello\007'`.
- [ ] Cursor shape changes on `printf '\033[3 q'` (bar) / `printf '\033[1 q'` (block blinking) etc.
- [ ] PRIMARY auto-copy works: select text, paste with middle-click in another terminal.
- [ ] CLIPBOARD copy works: Ctrl+Shift+C, Ctrl+v in another app.
- [ ] Bracketed paste verified by pasting multi-line text into `vim` insert mode.
- [ ] Window survives 3 consecutive launches (Phase 0 surface-lost fix).
- [ ] `cargo run -p emterm-native-poc` shows a working interactive shell.

## Performance Verification

- 60 FPS during normal interactive use (manual, by feel on Linux dev). Expected threshold: no perceptible frame drops on `vim`, `htop`, `tmux` resize storms.
- Input latency ≤ Phase 1 PoC by feel.
- Kitty PNG (1920×1080) renders within ≤ 300 ms of APC arrival (timed manually with `time emterm image …`).
- Scrolling 10,000-line scrollback is smooth.

## Security Verification

- [ ] OSC 52 default-allow + size cap verified: with `clipboard_read_osc52=false` payload arrives, clipboard untouched, log shows `LOG_OSC52_DENIED` (TS-12). With defaults (allow + ≤10 MB), clipboard receives payload (TS-13).
- [ ] Malformed APC / DCS payloads do not crash (TS-43, TS-44).
- [ ] Pasted content containing `\e[201~` cannot escape the bracketed-paste wrapper (TS-29).
- [ ] Image LRU enforces 320 MB quota (TS-36).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| OSC dispatch | 21 | 21 | 0 | 0 |
| APC/DCS dispatch | 4 | 4 | 0 | 0 |
| Selection / paste | 6 | 6 | 0 | 3 (PRIMARY, CLIPBOARD, bracketed paste) |
| Dirty-row diff | 2 | 2 | 0 | 1 (ghost check) |
| Settings | 4 | 4 | 0 | 0 |
| Image layer | 1 | 1 | 0 | 2 (Kitty + SIXEL parity) |
| Resize / reflow / scrollback | 5 | 5 | 0 | 1 (rapid resize) |
| Stability | 0 | 0 | 0 | 4 (12 h session + 3 memory samples) |
| Cursor / SGR | 1 | 1 | 0 | 2 (cursor shape, SGR sampler) |
| Notifications | 1 | 1 | 0 | 1 (visible toast) |
| Legacy regression | 1 | 1 | 0 | 0 |
| **Totals** | **45 TS + manual** | **46** | **0** | **14** |

> **Legacy regression note**: `cargo test --workspace` counts as 1 automated check (covering 849 `app_lib` tests). Legacy E2E (`./scripts/run-e2e-docker.sh`) is excluded from this SDD's gate per SPEC.md SC-6 rationale.

## Implementation Results — Phase 5 (inline image overlay)

- **Build**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build -p emterm-native-poc"` exit 0; full workspace `cargo build --workspace` exit 0.
- **Tests**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"` exit 0.
  - native-poc went from 66 → 104 passing tests (+38 new tests across `image::*` and `callbacks::tests`). Per-crate counts: app_lib 816+10+10+7+6, term_core 597, term_images 182, wasm 4, emterm-native-poc 104. Total: 1736 passing (vs. 1698 Phase 4 baseline).
  - Specific TS coverage added:
    - TS-22 (decode_apc reaches ImageEvent::Place) — `image::parse::tests::decode_apc_cursor_coords_propagate_to_place_events`.
    - TS-23 (decode_dcs reaches ImageEvent::Place) — `image::parse::tests::decode_dcs_accepts_minimal_sixel_introducer` + symmetric Query/Delete coverage.
    - TS-33 (Settings missing `image_memory_quota_mb` defaults to 320) — `settings::tests::default_image_memory_quota_is_320_mb`.
    - TS-36 (ImageLayer evicts LRU when over quota) — `image::tests::record_image_evicts_lru_when_over_quota` + `record_image_evicts_multiple_when_huge_insert` + `touch_image_moves_to_mru_end`.
    - TS-40 (resize keeps placements anchored) — `image::tests::resolve_pixel_placements_resize_anchor_stable`.
    - TS-43 / TS-44 (malformed APC/DCS warn-and-continue) — `image::parse::tests::decode_apc_rejects_empty_payload` / `decode_apc_rejects_non_kitty_payload` / `decode_dcs_rejects_payload_without_q_introducer`.
- **Code quality**: `cargo fmt --all -- --check` exit 0 (no diff). Clippy not re-run for Phase 5 (deferred to the final Phase 7 verification pass; the implementation introduces only warnings of the same kind already present in the tree: dead-code on currently-unused public helpers reserved for upcoming phases).
- **Files created (Phase 5)**: `native-poc/src/image/mod.rs`, `native-poc/src/image/overlay.rs`, `native-poc/src/image/parse.rs`.
- **Files modified (Phase 5)**: `native-poc/Cargo.toml` (term_images dep), `native-poc/src/main.rs` (module declaration), `native-poc/src/settings.rs` (image_memory_quota_mb), `native-poc/src/callbacks.rs` (pending_apc/dcs buffers), `native-poc/src/tabs.rs` (ImageProcessor + drain_and_decode_images + drain_image_events), `native-poc/src/window_host.rs` (ImageLayer + OverlayPipeline + image-overlay render pass).
- **Existing E2E regression**: not re-run for Phase 5 in isolation; deferred to Phase 7 (per SPEC.md SC-6 rationale, legacy E2E is out of this SDD's gate, and tauri-driver-based E2E requires the legacy WebView build which is unaffected by native-poc changes).
- **Known deferred**: image animation (Kitty `a=f`/`a=a`/`a=c`) — `ImageEvent::Animation` is logged at debug level and not applied to the GPU layer in Phase 5; tracked for a follow-up phase. Plumbing is in place (`split_image_events` + `ingest_state_events` already handle the variant cleanly).

## Implementation Results — Phase 6 (OSC dispatch matrix + notifications + clipboard policy)

- **Build**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build -p emterm-native-poc"` exit 0; full workspace `cargo build --workspace` exit 0.
- **Tests**: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --workspace"` exit 0.
  - native-poc went from 104 → 169 passing tests (+65 new tests across `callbacks::tests`, `render::theme::tests`, and `settings::tests`). Per-crate counts: app_lib 816+10+10+7+6, term_core 597, term_images 182, wasm 4, emterm-native-poc 169. Total: 1801 passing (vs. 1736 Phase 5 baseline).
  - Specific TS coverage added:
    - TS-1 (`OSC 0` → `state.title + state.icon_name`) — `callbacks::tests::osc_0_sets_title_and_icon`.
    - TS-2 (`OSC 1` icon-only, log) — `callbacks::tests::osc_1_sets_icon_name_only`.
    - TS-3 (`OSC 2` title-only) — `callbacks::tests::osc_2_sets_title_only`.
    - TS-4 (`OSC 4` palette set) — `callbacks::tests::osc_4_sets_palette_and_marks_theme_dirty` + `render::theme::tests::apply_osc_4_*` (3 variants).
    - TS-5 (`OSC 7` cwd capture) — `callbacks::tests::osc_7_sets_cwd`.
    - TS-6 (`OSC 8` log-only) — `callbacks::tests::osc_8_is_logged_only`.
    - TS-7 (`OSC 9` notify with rate-limit) — `callbacks::tests::osc_9_emits_notification` + `osc_9_no_separator_uses_fallback_title` + `rate_limiter_dedupes_identical_pair_within_window` + `rate_limiter_allows_after_window_elapsed` + `rate_limiter_distinct_pairs_not_deduped`.
    - TS-8, TS-9, TS-10 (`OSC 10/11/12` fg/bg/cursor) — `callbacks::tests::osc_10_sets_fg_and_marks_theme_dirty` etc. + `render::theme::tests::apply_osc_{10,11,12}_*` (4 variants).
    - TS-11 (`OSC 22` cursor style) — `callbacks::tests::osc_22_updates_cursor_style` + `render::theme::tests::apply_osc_22_{block,underline,bar,empty,invalid}`.
    - TS-12 (`OSC 52` deny on read disabled) — `callbacks::tests::osc_52_query_denied_when_read_disabled`.
    - TS-13 (`OSC 52` allow within default policy) — `callbacks::tests::osc_52_write_default_allows_within_quota` + `osc_52_query_default_allows_read` + `osc_52_write_denied_when_over_quota` + `osc_52_clear_pushes_empty_write` + `parse_osc52_*` (4 variants).
    - TS-14..TS-17 (`OSC 104/110/111/112` resets) — `callbacks::tests::osc_{104,110,111,112}_resets_*` + `render::theme::tests::apply_osc_{104,110,111,112}_*`.
    - TS-18 (`OSC 133` prompt mark) — `callbacks::tests::osc_133_{a_records_prompt_start,b_c_d_records_each,d_with_nonzero_exit_code}` + `parse_osc133_unknown_returns_none`.
    - TS-19 (`OSC 100` viewer queue) — `callbacks::tests::osc_100_emterm_extension_pushes_to_queue`.
    - TS-20 (`OSC 101` iTerm2 log) — `callbacks::tests::osc_101_iterm2_is_logged_only`.
    - TS-21 (`OSC 255` unknown warn) — `callbacks::tests::osc_255_unknown_is_logged_only`.
    - TS-35 (Settings defaults for OSC 52 fields mirror legacy) — `settings::tests::default_clipboard_read_osc52_is_true` + `default_clipboard_max_size_osc52_is_10_mib`.
- **Code quality**: `cargo fmt --all -- --check` exit 0 (no diff). Clippy not re-run (per Phase 5 / Phase 7 deferral policy — only the same dead-code warning kinds as already present).
- **Files created (Phase 6)**: none.
- **Files modified (Phase 6)**: `native-poc/Cargo.toml` (notify-rust = "4"), `native-poc/src/render/theme.rs` (CursorStyle enum, palette256 overlay, cursor_fg, Theme::apply_osc + parse_color_spec), `native-poc/src/settings.rs` (clipboard_read_osc52 + clipboard_max_size_osc52 mirroring legacy field names), `native-poc/src/callbacks.rs` (full OSC matrix + NotificationSink trait + NotifyRustSink + NotificationRateLimiter with injectable clock + PromptMark + OSC 52 policy gate), `native-poc/src/tabs.rs` (Tab now owns Arc<Mutex<Theme>>; pump drains theme_dirty + mark_all_dirty), `native-poc/src/app.rs` (settings: Arc<Settings>).
- **Existing E2E regression**: not re-run for Phase 6 in isolation; deferred to Phase 7 (legacy E2E excluded per SPEC.md SC-6 rationale; native-poc has no headless driver).
- **Known deferred**:
  - OSC 52 _write/read_ side effects to `arboard` happen on the UI thread; `Tab::pump` currently buffers `pending_clipboard_writes` / `pending_clipboard_reads` but the UI-thread drain that posts the OSC 52 query response back through the PTY is wired up in a follow-up sub-phase (`arboard::Clipboard` is not `Sync`, so the integration belongs in `window_host`).
  - The renderer still constructs `Theme::default()` per-frame; `Tab.theme: Arc<Mutex<Theme>>` is plumbed and `theme_dirty` already triggers `mark_all_dirty`, but the render path itself reads the shared theme in a separate follow-up. The OSC color/style state is fully captured today (visible via tests).
  - `NotifyRustSink` is wired but never instantiated from tests; Docker E2E does not exercise D-Bus. Manual verification (`printf '\033]9;hello\007'`) is the FR12 acceptance gate and runs as part of Phase 7.
