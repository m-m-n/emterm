# Verification Document: Markdown Viewer Port to native-poc (Wry Window)

## Overview

**Feature**: markdown-viewer-native-port
**SPEC.md**: `doc/tasks/markdown-viewer-native-port/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/markdown-viewer-native-port/IMPLEMENTATION.md`

## Build Verification

- Command (native-poc): `CARGO_TARGET_DIR=native-poc/target cargo check --manifest-path native-poc/Cargo.toml`
- Command (release for manual run): `CARGO_TARGET_DIR=native-poc/target-host cargo build --release --manifest-path native-poc/Cargo.toml`
- Command (viewer bundle): `bun run build:viewer`
- Expected: exit code 0, no errors.

### Actual Results (sdd.4 implement)

| Build | Command | Exit | Notes |
|-------|---------|------|-------|
| native-poc check | `CARGO_TARGET_DIR=native-poc/target cargo check --manifest-path native-poc/Cargo.toml` | 0 | wry 0.53 + gtk 0.18 compiled |
| release binary | `CARGO_TARGET_DIR=native-poc/target-host cargo build --release …` | 0 | `Finished release in 2m14s`; binary at `native-poc/target-host/release/emterm-native-poc` (50 MB) |
| viewer bundle | `bun run build:viewer` | 0 | 2331 modules → `native-poc/viewer/dist/` (index.html + hashed JS 3.8 MB + CSS) |

- wry version note: declared `wry = "0.53"`; the workspace shares the root `Cargo.lock` with `tauri 2.10` so wry resolves to `0.53.5` / `webkit2gtk 2.0.1`. An earlier pin to `=0.45.0` (from a stale `native-poc/Cargo.lock`) caused a `webkit2gtk` workspace conflict and was reverted. SPEC risk "wry 0.53 vs locked 0.45" is resolved: the build uses 0.53.

## Test Verification

- Command (native-poc): `CARGO_TARGET_DIR=native-poc/target cargo test --manifest-path native-poc/Cargo.toml --bin emterm-native-poc`
- Command (TS, Docker): `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test src/markdown && bun run typecheck"`
- Coverage target: core viewer logic (parser, session, settings, guards) minimum 80%.

### Actual Results (sdd.4 implement)

| Suite | Command | Result |
|-------|---------|--------|
| native-poc Rust | `cargo test --manifest-path native-poc/Cargo.toml --bin emterm-native-poc` | **1049 passed, 0 failed, 1 ignored** (baseline before this feature: 1002 → +47 new viewer/settings tests) |
| viewer module only | `… cargo test … viewer::` | 46 passed (mod 11, markdown 12, launch 7, image_resolver 12, window 3, assets 3 minus overlap) |
| TS viewer entry | `bun test native-poc/viewer/web/entry.test.ts` | 4 passed |
| TS src/markdown regression | `bun test src/markdown/` | 208 passed, 0 failed (no regression from reuse) |
| TS project typecheck | `bunx tsc --noEmit -p tsconfig.json` | 0 errors (entry.ts clean) |

Test-to-scenario mapping (all implemented and passing):
- TS-1 `parse_payload_splits_kind_verb_and_params`; TS-2 `begin_chunk_end_joins_in_seq_order_and_decodes`; TS-3 `out_of_order_chunks_are_reordered_by_seq`; TS-4 `eleventh_concurrent_begin_is_rejected`; TS-5 `size_cap_drops_session_no_panic`; TS-6 `idle_session_evicted_after_timeout`; TS-7 `missing_id_*` / `malformed_base64_*` / `unknown_verb_*`; TS-8 `reserved_kinds_are_ignored_without_request`; TS-9 `markdown_settings_defaults_match_spec`; TS-10 `appearance_follow_ui_{true,false}_*`; TS-11 `each_completed_session_yields_exactly_one_request`; TS-12 `payload_round_trips_through_json`; TS-13 `launch_with_writes_payload_and_invokes_spawn_once`; TS-14 `safe_uri_*` + `open_safe_uri_refuses_disallowed_schemes`; TS-15 `rejects_*traversal*` / `resolves_simple_relative_png`; TS-16 `rejects_svg_mime` (Rust resolver) + reused src/markdown SVG-exclusion tests; TS-17 entry.test.ts `renders an injected sample …`.

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | Parse `markdown;begin;id=…;format=gfm` | (kind=markdown, verb=begin, params with format=gfm) | Unit |
| TS-2 | begin→chunk(seq 0,1,2)→end | One document, chunks joined in order, base64 decoded | Unit |
| TS-3 | Out-of-order seq chunks | Document assembled in `seq` order | Unit |
| TS-4 | 11th concurrent `begin` | Rejected; warning logged; existing sessions intact | Unit |
| TS-5 | Cumulative data over size cap | Session ends with error; warning logged | Unit |
| TS-6 | No `end` within 30s | Session dropped on next drain | Unit |
| TS-7 | Missing `id` / unknown verb / malformed base64 | Warned, ignored; no panic | Unit |
| TS-8 | Reserved kinds (image/json/yaml) | Debug-logged and ignored | Unit |
| TS-9 | Settings: all 7 keys parse with defaults (fonts empty, size 14, follow_ui true, theme System, preset Purple) | Correct resolved values | Unit |
| TS-10 | Appearance resolver `follow_ui` true/false | Selects ui_theme/preset vs markdown_theme/preset | Unit |
| TS-11 | End-to-end dispatch via capturing sink | Exactly one RenderRequest per completed session | Integration |
| TS-12 | Payload serialize→deserialize round-trip (parent→child temp file) | Child reconstructs {markdown, format, basedir, appearance} | Unit |
| TS-13 | Real sink spawns one child per RenderRequest (spawn boundary abstracted) | Exactly one spawn intent per completed session | Unit |
| TS-14 | `is_safe_uri` gating | http/https/mailto/ssh allowed; others denied | Unit |
| TS-15 | basedir-relative image resolution | Bytes within basedir for allowed MIME; traversal rejected | Unit |
| TS-16 | SVG data URI / disallowed MIME | Excluded / not rendered | Unit (TS, reused src/markdown) |
| TS-17 | Viewer bundle renders injected sample | Parity structure (headings/tables/code/mermaid/outline) | Unit (TS) |

## Code Quality Verification

- Format: `cargo fmt --manifest-path native-poc/Cargo.toml` (+ `bun run format` for TS)
- Static analysis: `CARGO_TARGET_DIR=native-poc/target cargo clippy --manifest-path native-poc/Cargo.toml` (if used by the repo)

### Actual Results (sdd.4 implement)

- `cargo fmt --manifest-path native-poc/Cargo.toml --check` → exit 0 (clean).
- `cargo clippy … --bin emterm-native-poc` → no warnings on any `viewer/*` / `links.rs` / `settings.rs` viewer addition (test-only helpers gated with `#[cfg(test)]` or annotated `#[allow(dead_code)]` with rationale). Remaining warnings are pre-existing baseline (unused font-stack imports, `app.rs::build_font_stack` complex-type) unrelated to this feature.
- TS: the repo has no JS/TS formatter configured (no biome/prettier/format script); new TS uses tabs matching `src/markdown` style.

## File Structure Verification

### Files to Create
- [x] `native-poc/src/viewer/mod.rs` - ViewerSpawner (drain/parse/route/sink) + ProcessViewerSink
- [x] `native-poc/src/viewer/markdown.rs` - MarkdownViewerSessions
- [x] `native-poc/src/viewer/launch.rs` - parent launcher (serialize payload, spawn child, reap)
- [x] `native-poc/src/viewer/window.rs` - child GTK/Wry viewer window (scheme/nav/images/controls)
- [x] `native-poc/src/viewer/assets.rs` - embedded bundle accessor
- [x] `native-poc/src/viewer/image_resolver.rs` - **added** (not in original plan): cross-platform, testable basedir-confined image resolver extracted out of `window.rs` so FR8 security logic is unit-tested without GTK
- [x] `native-poc/build.rs` - **added** (not in original plan): walks `viewer/dist` and emits the embedded-asset manifest, since bun emits content-hashed filenames that `include_bytes!` can't target by fixed path (avoids adding `include_dir`/`rust-embed`)
- [x] `native-poc/viewer/web/index.html` - viewer page entry
- [x] `native-poc/viewer/web/entry.ts` - reuse src/markdown; render injected payload
- [x] `native-poc/viewer/web/entry.test.ts` - **added**: entry render/appearance tests

### Files to Modify
- [x] `native-poc/src/settings.rs` - 7 markdown_* fields + `MarkdownAppearance` + `markdown_appearance()` resolver
- [x] `native-poc/src/callbacks.rs` - removed the stale `#[allow(dead_code)]` on `EmtermOscRequest.payload` (now read). **Note**: the OSC queue *read side* was already provided by the pre-existing `Tab::drain_osc()` (`tabs.rs:1032`), so no new callbacks API was needed; the plan's "expose OSC queue read/drain side" is satisfied by reusing it.
- [x] `native-poc/src/main.rs` - `--viewer <path>` dispatch to child entry
- [x] `native-poc/src/app.rs` - **modified** (not listed in plan): owns the `ViewerSpawner` + `ProcessViewerSink` and drains every tab's OSC in `pump_all` (the integration point the plan describes as "the parent drains the viewer queue on the event-loop wakeup")
- [x] `native-poc/src/links.rs` - added `open_safe_uri` (is_safe_uri gate + OS open) for the viewer nav handler
- [x] `package.json` - added `build:viewer` script
- [x] `native-poc/Cargo.toml` - wry kept at `0.53`; added Linux-gated `gtk = "0.18"`

Note: `native-poc/src/window_host.rs` is intentionally **not** modified (terminal stays single-window; viewers are separate processes). `src/` is **unchanged** — verified `git status --porcelain src/` empty after all phases (NFR3 / SC-7).

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | begin/chunk/end renders Markdown in a window | Manual run + TS-2/TS-11 |
| SC-2 | Parity: tables, highlight, mermaid, images, outline | Manual run + TS-17 |
| SC-3 | 7 settings captured and reflected | TS-9/TS-10 + manual |
| SC-4 | Links open via OS; no in-window navigation | TS-14 + manual |
| SC-5 | Esc/q/close works | Manual |
| SC-6 | Limits/timeout/size function | TS-4/TS-5/TS-6 |
| SC-7 | `src/` unchanged | `git diff --stat src/` empty |
| SC-8 | No regression in native-poc tests | Test command green |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1 ViewerSpawner | Phase 2 | TS-1, TS-8, TS-11 |
| FR2 Session accumulation | Phase 2 | TS-2, TS-3, TS-4, TS-5, TS-6, TS-7 |
| FR3 Viewer process spawn | Phase 4 | TS-12, TS-13 + manual |
| FR4 Embedded bundle | Phase 3 | TS-17 + bundle build |
| FR5 Rendering parity | Phase 3 | TS-17 + manual |
| FR6 Settings wiring | Phase 1, 5 | TS-9, TS-10 + manual |
| FR7 Link handling | Phase 5 | TS-14 + manual |
| FR8 Image resolution | Phase 5 | TS-15, TS-16 + manual |
| FR9 Window controls | Phase 4, 5 | Manual |
| NFR1 Performance | Phase 4 | Manual (terminal not stalled by large MD) |
| NFR2 Security | Phase 5 | TS-14, TS-15, TS-16 |
| NFR3 Branch policy | All | `git diff --stat src/` empty |
| NFR4 Platform | Phase 4 | Manual on Linux/WebKitGTK |
| NFR5 Maintainability | Phase 3 | Reuses src/markdown; warn/error logging present |

## E2E Testing

native-poc has no automated GUI E2E framework (the existing `e2e-tests/` target the WebView app via tauri-driver, not native-poc). E2E-level checks are performed manually below.

### Existing E2E Regression (sdd.4 Phase 3.8)

- The repo's automated E2E (`e2e-tests/`, WebView app via tauri-driver) does **not** exercise native-poc — this feature is native-poc-only and `src/` is unchanged, so the WebView E2E suite is not applicable and was **not** run as part of this implementation (it would not cover the change). No regression risk to the WebView app: `git status --porcelain src/` is empty.

## Manual Testing (E2E Not Possible)

### Implementation-time bring-up (sdd.4, this host: Wayland + WebKitGTK)

Smoke-tested directly against the release binary by hand-crafting payload JSON files and running `emterm-native-poc --viewer <payload>`:

- [x] **`--viewer` dispatch**: no path → logs usage error, exit 2; missing payload → logs read error, exit 1; valid payload → window opens. Confirmed.
- [x] **GTK/WebKitGTK window opens** on Linux/Wayland and stays alive (ran to the kill-timeout, not an early exit). `Gdk-CRITICAL gdk_wayland_window_set_dbus_properties_libgtk_only` lines are emitted — these are cosmetic GTK3-on-Wayland D-Bus warnings, **non-fatal**; the window renders. (FR3, NFR1, NFR4)
- [x] **Multiple concurrent viewers** (two payloads launched together) coexist independently for the full lifetime; closing/killing one does not affect the other or the parent. (FR3 window-lifecycle)

The following require human visual confirmation (a live browser, settings file edits, real `emterm markdown` CLI output) and remain for sdd.6 / manual QA:

- [ ] Emit real `emterm markdown` output from the CLI; a window renders the document (the implementation-time test injected payloads directly).
- [ ] Document shows headings, tables, syntax-highlighted code, a mermaid diagram, an inline image, and an outline (structure asserted by entry.test.ts; visual fidelity is human-only).
- [ ] Click a link → opens in the system browser; the viewer window does not navigate (logic verified by `navigation_allowed` tests; the actual browser launch is an OS side effect).
- [ ] An image referenced relative to `basedir` displays (resolver logic fully unit-tested; on-screen display is human-only).
- [ ] Change `markdown_*` settings → the next window reflects theme/preset/fonts/size; `follow_ui` toggles source (resolver + applier tested; visual reflection is human-only).
- [ ] `Esc` / `q` / close button closes the window (wired in `window.rs::run`; key-event delivery is human-only).

## Performance Verification

- Large Markdown near the size cap: terminal rendering/input remains responsive while the window is created and rendered.

## Security Verification

- [ ] `is_safe_uri` blocks non-allowed schemes (TS-14).
- [ ] basedir traversal rejected (TS-15).
- [ ] SVG data URIs excluded; disallowed MIME not rendered (TS-16).
- [ ] Viewer window cannot navigate to arbitrary URLs (manual + navigation interceptor).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Unit / Integration | 17 | 17 | 0 | 0 |
| Success criteria | 8 | 4 | 0 | 4 |
| Manual scenarios | 7 | 0 | 0 | 7 |
| Security | 4 | 3 | 0 | 1 |

### Implementation Status (sdd.4)

All 5 phases **completed**. Automated verification green:
- native-poc Rust: **1049 passed / 0 failed / 1 ignored** (47 new vs the 1002 baseline).
- TS: 4 viewer-entry + 208 src/markdown (no regression); project typecheck clean.
- Build: `cargo check`, release `cargo build`, and `bun run build:viewer` all exit 0.
- `cargo fmt --check` clean; clippy clean on all viewer additions.
- `src/` unchanged (NFR3 / SC-7) — `git status --porcelain src/` empty.

Implementation-time manual bring-up on Linux/Wayland/WebKitGTK confirmed the `--viewer` dispatch, a live viewer window, and two concurrent independent viewers (FR3/NFR1/NFR4). Visual-fidelity, real-CLI, live-browser, settings-edit, and key-event checks (the remaining `[ ]` items) require human QA and are deferred to sdd.6.

### Known Limitations

- GTK3-on-Wayland emits cosmetic `Gdk-CRITICAL gdk_wayland_window_set_dbus_properties_libgtk_only` warnings at viewer startup. Non-fatal — the window renders normally. (Inherent to gtk 0.18 / WebKitGTK on Wayland; not a code defect.)
- The OS-open side of `links::open_safe_uri` (xdg-open/ShellExecuteW) and the actual on-screen rendering of images/themes are OS/visual side effects not covered by unit tests; their decision logic (safe-scheme gate, basedir confinement, appearance resolution) is fully unit-tested.
