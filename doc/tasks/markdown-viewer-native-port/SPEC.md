# Feature: Markdown Viewer Port to native-poc (Wry Window)

## Overview

Port the Markdown viewer into `native-poc`. Rendering uses a **Wry (WebView) separate window** that hosts the existing WebView Markdown renderer (`src/markdown`), preserving fidelity for mermaid diagrams, syntax highlighting, themes, and fonts. A new `ViewerSpawner` drains the existing OSC queue, accumulates the `emterm;markdown` session, and spawns one window per completed Markdown. The seven Markdown viewer settings are captured into the native-poc settings loader and injected into each viewer window.

## Objectives

- Implement a `ViewerSpawner` foundation that drains `osc_queue` and routes by viewer type (Markdown now; image/json/yaml as future extension points).
- Render Markdown in a Wry window by reusing the WebView renderer via a dedicated, embedded viewer bundle.
- Capture and apply the seven `markdown_*` settings in native-poc.
- Keep `src/` unchanged (import-only reuse), per branch policy.

## User Stories

### US1: Display Markdown
As a terminal user (or AI tool), I want OSC `emterm;markdown` sequences to render in a window, so that I can read formatted documents from the terminal.

**Acceptance Criteria:**
- [ ] `begin` → `chunk`×N → `end` produces a window showing the rendered Markdown.
- [ ] Base syntax + tables + syntax highlighting + mermaid + inline images + outline/TOC render.

### US2: Open links safely
As a terminal user, I want to click links in rendered Markdown, so that they open in my browser without navigating the viewer window.

**Acceptance Criteria:**
- [ ] External links are validated by `is_safe_uri` then opened via the OS handler.
- [ ] In-window navigation to external URLs is suppressed.

### US3: Customize Markdown appearance
As a terminal user, I want Markdown viewer settings to take effect, so that theme and fonts match my preferences.

**Acceptance Criteria:**
- [ ] All seven `markdown_*` settings are loaded and injected into the viewer window.
- [ ] `markdown_theme_follow_ui=true` follows the UI theme/preset; `false` uses `markdown_theme`/`markdown_theme_preset`.

## Technical Requirements

### Functional Requirements

- **FR1 — ViewerSpawner foundation:** Drain `NativeCallbacks` `osc_queue` (`EmtermOscRequest{payload}`), parse `<viewer>;<verb>;<k=v>…`, and route by viewer type. Markdown is handled in this feature; image/json/yaml routing branches are reserved (no-op + debug log) for future features.
- **FR2 — Markdown OSC session accumulation:** Port `MarkdownSessionManager` lifecycle to Rust. Handle `begin` (`id`, `format`∈{commonmark,gfm} default commonmark, `version` default 1, `basedir`), `chunk` (`id`, `seq`, `data` base64), `end` (`id`). Enforce max concurrent sessions (10), 30s idle timeout, and cumulative size cap. On `end`, concatenate chunks in `seq` order and base64-decode to UTF-8 Markdown.
- **FR3 — Viewer process spawn:** For each completed session, spawn a separate child viewer process (the same binary, `--viewer` mode) that owns a GTK/Wry window. Multiple child viewers may exist concurrently; closing exits the child; viewers are never reused. The terminal process keeps its single winit window unchanged. (Rationale: on Linux, Wry renders via WebKitGTK and requires GTK init + a GTK main loop, which does not compose with the terminal's winit event loop.)
- **FR4 — Embedded viewer bundle:** A dedicated standalone viewer entry (HTML + TS) reuses `src/markdown` modules, built by a new Bun target, embedded into the native-poc binary and served to the Wry window. `src/` is not modified.
- **FR5 — Rendering parity:** Reuse the WebView renderer for headings, paragraphs, emphasis (bold/italic/strikethrough), ordered/unordered lists, inline/fenced code, blockquotes, horizontal rules, links, GFM tables, syntax highlighting, mermaid diagrams, inline images, and the outline/TOC panel.
- **FR6 — Settings wiring:** Capture `markdown_theme_follow_ui`, `markdown_theme`, `markdown_theme_preset`, `markdown_body_font_family`, `markdown_code_font_family`, `markdown_emoji_font_family`, `markdown_font_size` into native-poc's `RawSettings`/`Settings`, and inject the resolved theme/preset/fonts/size into each viewer window.
- **FR7 — Link handling:** Intercept viewer-window navigation; validate external URIs with `is_safe_uri` (http/https/mailto/ssh); open allowed URIs via the OS (`xdg-open` / `cmd /c start`); suppress in-window navigation; warn on disallowed URIs.
- **FR8 — Inline image resolution:** Render base64 data URIs directly after MIME allowlist validation (png/jpeg/gif/webp/bmp/x-icon; SVG excluded). Resolve `basedir`-relative local file references through native (custom protocol) restricted to `basedir`.
- **FR9 — Window controls:** Close via window close button, `Esc`, or `q`; keyboard and mouse-wheel scrolling within the page.

### Non-Functional Requirements

- **NFR1 - Performance:** Window spawn occurs on `end` and must not block the terminal render/PTY path. Large Markdown is bounded by the cumulative size cap.
- **NFR2 - Security:** Preserve DOMPurify sanitization in the ported bundle; exclude SVG data URIs; enforce the image MIME allowlist; validate links with `is_safe_uri`; restrict local image resolution to `basedir`; suppress arbitrary in-window navigation.
- **NFR3 - Branch policy:** No modification to `src/` ([[project_native_poc_branch_policy]]); reuse by import. Record future relocation of shared viewer modules once the WebView terminal is removed.
- **NFR4 - Platform:** Linux primary (Wry on WebKitGTK); Windows secondary. Gate OS-specific link/image open paths.
- **NFR5 - Maintainability:** Reuse `src/markdown` to minimize divergence from the WebView renderer; `log::warn`/`log::error` for failures and invalid input.

## Implementation Approach

### Architecture

**Component layering (native-poc):**
```
┌──────────────────────────────────────────────────────────┐
│ term_core OSC parser → NativeCallbacks::on_osc (existing)  │
│   OSC 100 (wire 777) "markdown;…" → osc_queue (existing)   │
├──────────────────────────────────────────────────────────┤
│ ViewerSpawner (NEW)                                        │
│   • drain osc_queue                                        │
│   • parse "<viewer>;<verb>;<k=v>…"                         │
│   • route: markdown → MarkdownViewerSessions               │
│            image/json/yaml → reserved (future)             │
├──────────────────────────────────────────────────────────┤
│ MarkdownViewerSessions (NEW, port of MarkdownSessionMgr)   │
│   • begin/chunk/end accumulation, limits, timeout          │
│   • on end → RenderRequest → spawn child viewer process    │
└──────────────────────────────────────────────────────────┘
        │ spawn: self binary `--viewer <payload-temp-file>`
        ▼
┌──────────────────────────────────────────────────────────┐
│ Child viewer process (NEW, `--viewer` mode, GTK/Wry)       │
│   • GTK init + Wry WebView window + child event loop        │
│   • reads payload temp file (markdown/format/basedir/cfg)   │
│   • loads embedded viewer bundle via custom scheme          │
│   • navigation handler (is_safe_uri → OS open)              │
│   • custom-scheme image resolver (basedir, term_images)     │
│   • close/Esc/q → process exit                              │
└──────────────────────────────────────────────────────────┘
```

**Viewer bundle (NEW, TypeScript):**
```
viewer entry (HTML + TS)
  └─ imports src/markdown (MarkdownRenderer, mermaid, outline, …)
  └─ reads injected { markdown, format, basedir, settings }
  └─ renders into the page
built by Bun → embedded into native-poc binary
```

### Data Flow

```
PTY → term_core OSC parser → NativeCallbacks::on_osc(100, "markdown;…")
    → osc_queue.push(EmtermOscRequest{payload})
ViewerSpawner.drain():                              [terminal process]
    parse payload → (verb, params)
    MarkdownViewerSessions.handle(verb, params)
      begin → create session
      chunk → store by seq
      end   → join+decode → RenderRequest
              → serialize {markdown, format, basedir, appearance} to temp file
              → spawn: self binary `--viewer <temp-file>`
Child viewer process (`--viewer`):                  [separate process]
    read payload temp file
    custom scheme serves bundle; page renders Markdown (reused src/markdown renderer)
    link click → navigation handler → is_safe_uri → OS open
    img(basedir-relative) → custom scheme → native resolves → bytes
    close/Esc/q → exit
```

### OSC Protocol (received)

Wire form (already decoded by `callbacks.rs`; payload is the part after `777;`):

```
markdown;begin;id=<uuid>;format=<commonmark|gfm>;version=<n>;basedir=<dir>
markdown;chunk;id=<uuid>;seq=<n>;data=<base64>
markdown;end;id=<uuid>[;interactive=1]
```

- Tokens are `;`-separated; the first two are `<viewer>` and `<verb>`; the rest are `key=value`.
- `data` is base64-encoded Markdown text (chunked to keep individual OSC sequences small).
- `interactive=1` is present on the `end` marker only when the CLI's stdin is a TTY (see "Interactive CLI release"). Absent otherwise.
- tmux DCS passthrough is handled by the CLI emitter; native-poc receives normal OSC.

### Interactive CLI release

`emterm markdown` parks in an interactive stdin loop (navigate/image/quit) when its stdin is a TTY, waiting for a `quit` line before returning the shell prompt. The native viewer is a separate child process that resolves images and links itself, so nothing drives that loop. To return the prompt immediately, the terminal writes `quit\n` to the emitting tab's PTY when it observes a `markdown;end` marker carrying `interactive=1` (the CLI sets that flag only when its stdin is a TTY, so a non-interactive piped/redirected invocation — which has already returned — is not released).

**Accepted residual risk:** `interactive=1` is plaintext in the terminal output stream, which is attacker-controllable. Untrusted output (a `cat`'d file, an SSH peer, a log line) can forge `markdown;end;…;interactive=1` and cause the terminal to write `quit\n` into the emitting tab's foreground program; the `id` is not correlated against a live session. The blast radius is bounded to a single `quit\n` line (not arbitrary input), so this is accepted rather than gated. A stronger gate (terminal-owned foreground-process check via `process_group_leader`, or a begin-correlated session id) is a future option if the residual becomes unacceptable.

### Parent → Child Data Passing

- The terminal process serializes the render payload `{ markdown, format, basedir, appearance }` to a temp file (`/tmp`, per project temp-file conventions) and spawns the child with `--viewer <temp-file-path>`.
- Inside the child, a Wry **custom scheme** (e.g. `emterm-viewer://`) serves: (a) the embedded viewer bundle assets (same binary → shared embed), and (b) `basedir`-relative image bytes on demand.
- Exact scheme/host layout and the payload serialization format are finalized during Phase 3/4 implementation.

### Settings

| Key | Type | Default | Effect |
|-----|------|---------|--------|
| `markdown_theme_follow_ui` | bool | `true` | `true` → use `ui_theme`/`ui_theme_preset`; `false` → use `markdown_theme`/`markdown_theme_preset` |
| `markdown_theme` | UiTheme {Light,Dark,System} | `System` | light/dark variant |
| `markdown_theme_preset` | UiThemePreset {Purple,Blue,Green,Orange,Pink} | `Purple` | accent preset |
| `markdown_body_font_family` | string | `""` | body font (empty → CSS fallback chain) |
| `markdown_code_font_family` | string | `""` | code font (empty → CSS fallback chain) |
| `markdown_emoji_font_family` | string | `""` | emoji font (empty → CSS fallback chain) |
| `markdown_font_size` | u32 | `14` | base font size (pt) |

native-poc already defines `UiTheme`/`UiThemePreset` and resolves md3 palettes (`palette_for`/`set_preset`), reused for `follow_ui=true`.

### Dependencies

**Internal:**
- `native-poc/src/callbacks.rs` — OSC queue producer (`OSC_EMTERM_EXTENSION` = 100; `EmtermOscRequest`).
- `native-poc/src/links.rs` — `is_safe_uri`, OS open paths.
- `native-poc/src/settings.rs` — `RawSettings`/`Settings` loader.
- `native-poc/src/ui/md3.rs` — theme/preset resolution.
- `crates/term_images` — local image decode for `basedir`-relative images.
- `src/markdown/*` — WebView renderer reused by the viewer bundle (import only).

**External:**
- `wry = "0.53"` — already a native-poc dependency.
- Bun — builds the viewer bundle (existing toolchain).
- `marked`, `highlight.js`, `mermaid`, `dompurify` — transitively via reused `src/markdown` bundle.

### File Structure (anticipated; finalized in sdd.2)

```
native-poc/src/
├── viewer/
│   ├── mod.rs            # ViewerSpawner: drain queue, parse, route, real sink
│   ├── markdown.rs       # MarkdownViewerSessions: begin/chunk/end, limits
│   ├── launch.rs         # parent: serialize payload, spawn `--viewer` child, reap
│   ├── window.rs         # child: GTK/Wry window, custom scheme, nav handler, images
│   └── assets.rs         # embedded viewer bundle access
├── settings.rs           # + 7 markdown_* fields + appearance resolver
├── main.rs               # `--viewer <path>` dispatch to child entry
native-poc/viewer/web/    # NEW Bun target
├── index.html
└── entry.ts              # imports src/markdown, renders injected payload
package.json              # + "build:viewer" script (no src/ change)
# window_host.rs unchanged — terminal stays single-window
```

## Test Scenarios

### Unit Tests (Rust, native-poc)
- [ ] Payload parse: `markdown;begin;id=…;format=gfm` → (verb=begin, params).
- [ ] Session: begin→chunk(seq 0,1,2)→end joins in order and decodes base64.
- [ ] Session: out-of-order seq is reordered by seq.
- [ ] Limits: 11th concurrent `begin` rejected; cumulative size cap triggers error end; 30s timeout drops session.
- [ ] Invalid: missing `id`, unknown verb, malformed base64 → warned, no panic.
- [ ] Settings loader: all 7 `markdown_*` keys parsed with correct defaults; `follow_ui` resolution selects the right theme source.
- [ ] `is_safe_uri` gating for link open (reuse existing tests in `links.rs`).

### Integration Tests
- [ ] End-to-end dispatch: feeding `on_osc(100, "markdown;…")` populates a session and yields a spawn request on `end` (window creation mocked/abstracted behind a trait for testability).

### E2E Tests
**Existing E2E tests**: `e2e-tests/` (WebView app via tauri-driver) — not applicable to native-poc, which is a standalone binary the user runs manually.
**Run command**: native-poc has no automated GUI E2E; verification is manual (run `native-poc/target-host/release/emterm-native-poc`, emit a Markdown OSC, observe the window).
- [ ] Manual: emit `emterm markdown` output and confirm a window renders the document with tables/highlight/mermaid/image/outline.
- [ ] Manual: click a link → opens in browser, window does not navigate.
- [ ] Manual: change `markdown_*` settings → next window reflects them.

### Edge Cases
- [ ] Empty Markdown (`begin`→`end`, no chunks) → empty/whitespace window or graceful no-op.
- [ ] Concurrent sessions with interleaved chunks for different `id`s.
- [ ] SVG data URI is excluded; disallowed MIME is not rendered.
- [ ] `basedir`-relative image escaping `basedir` (`../`) is rejected.

### Performance Tests
- [ ] Large Markdown near the size cap does not stall the terminal render/PTY loop.

## Security Considerations

- **XSS Prevention:** DOMPurify sanitization retained in the ported bundle; SVG data URIs excluded; image MIME allowlist enforced.
- **Link safety:** `is_safe_uri` allows only http/https/mailto/ssh; everything else is not opened (warn).
- **Filesystem:** local image resolution restricted to `basedir`; reject path traversal.
- **Navigation isolation:** the viewer window cannot navigate to arbitrary URLs; the navigation handler routes external links to the OS instead.

## Error Handling

| Code | Description | Handling |
|------|-------------|----------|
| ERR_BAD_VERB | Unknown markdown verb | warn + ignore the command |
| ERR_NO_ID | Missing required `id` | warn + ignore the command |
| ERR_MAX_SESSIONS | >10 concurrent sessions | reject `begin` + warn |
| ERR_SIZE | Cumulative data over cap | end session with error + warn |
| ERR_TIMEOUT | No `end` within 30s | drop session |
| ERR_B64 | Malformed base64 chunk | end session with error + warn |
| ERR_SPAWN | Wry window creation failed | warn; terminal unaffected |

## Success Criteria

- [ ] All functional requirements implemented and tested.
- [ ] Unit/integration tests pass; native-poc existing tests do not regress.
- [ ] Security requirements satisfied (XSS, MIME, links, basedir).
- [ ] `src/` unchanged.
- [ ] Manual verification: a Markdown OSC renders a faithful window.

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

- [ ] FR4/FR8 — Native↔window data transport (custom protocol scheme/host layout, payload injection mechanism) is finalized in sdd.2 (design-level, not blocking the requirement).
- [ ] NFR3 — Future relocation of shared `src/markdown` modules to a native-poc-owned location once the WebView terminal is removed (tracked as a follow-up task, out of scope here).

## Implementation Phases

### Phase 1: Foundation + session
**Goals:** ViewerSpawner + Markdown session accumulation (FR1, FR2) with unit tests.
**Deliverables:**
- `viewer/mod.rs` (drain + parse + route), `viewer/markdown.rs` (sessions + limits)
- Settings loader fields + tests (FR6, loader half)

### Phase 2: Window + bundle
**Goals:** Wry window + embedded viewer bundle (FR3, FR4, FR5).
**Deliverables:**
- `viewer/window.rs`, `viewer/assets.rs`
- New Bun viewer target + embedding

### Phase 3: Links, images, settings injection, controls
**Goals:** FR7, FR8, FR6 (injection half), FR9.
**Deliverables:**
- Navigation handler + OS open
- Custom-protocol image resolver
- Settings injection + window controls

## References

- Requirements: `doc/tasks/markdown-viewer-native-port/要件定義書.md`
- OSC receiver: `native-poc/src/callbacks.rs`
- Viewer stubs: `native-poc/src/viewer/mod.rs`, `native-poc/src/viewer/markdown.rs`
- Settings: `src-tauri/src/commands/config/settings.rs`, `src-tauri/src/commands/config/types.rs`, `native-poc/src/settings.rs`
- WebView source: `src/markdown/` (renderer/session/fullscreen/outline/mermaid-renderer/link-dialog/types/security)
- Link safety: `native-poc/src/links.rs`
- Theme: `native-poc/src/ui/md3.rs`
- Build location rule: `.claude/rules/native-poc-build-location.md`
- Settings gap survey: `tmp/native-poc-settings-gap-2026-05-26.md`
