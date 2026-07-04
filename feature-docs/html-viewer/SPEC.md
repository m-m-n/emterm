# Feature: HTML Viewer

## Overview

Add an `html` CLI subcommand and a child-WebView HTML viewer to eMterm. `emterm html PATH` reads an HTML file, emits an OSC 777 display sequence (kind: `html`), and the GUI terminal opens a child WebView window that renders the HTML as-is — no eMterm styling. Intended for reviewing AI-generated HTML documents; not a full browser.

## Objectives

- Display a local HTML file in a child WebView window via `emterm html PATH`
- Execute the document's JavaScript while blocking all network access
- Resolve relative local resources (images / CSS / JS) against the file's directory (basedir)

## User Stories

### US1: View an AI-generated HTML file
As an eMterm user, I want to run `emterm html report.html` and see the rendered page in a window, so that I can review AI-generated HTML without leaving the terminal.

**Acceptance Criteria:**
- [ ] Running `emterm html file.html` inside the eMterm GUI opens a child WebView window rendering the file
- [ ] The document renders with only its own styles (no eMterm CSS / MD3 theme applied)
- [ ] Inline and locally referenced JavaScript executes
- [ ] Validation failures (missing file, directory, > 10MB, wrong extension) print an error to stderr and exit non-zero

## Technical Requirements

### Functional Requirements
- **FR1:** `emterm html <file>` CLI subcommand — validates the input (extension `.html`/`.htm` case-insensitive, regular file, ≤ 10MB), base64-encodes the content in 128KB chunks, and emits an OSC 777 sequence with kind `html`, a session UUID, and `basedir` (the canonical file's parent directory). Wraps in tmux DCS passthrough when inside tmux. Follows the existing `markdown` subcommand pipeline (`src-tauri/src/cli/`). Works in the CLI-only build (`--no-default-features`).
- **FR2:** GUI-side HTML viewer — the terminal's OSC 777 handler routes kind `html` to a child WebView viewer process that renders the received HTML document directly (no Markdown renderer, no eMterm stylesheet). `html` is added to `REPLAYABLE_VIEWER_KINDS` (`src-tauri/src/viewer_kinds.rs`) so mux snapshot replay strips the launch sequence and does not relaunch the viewer.
- **FR3:** JavaScript execution — scripts inside the document (inline and basedir-local) execute normally.
- **FR4:** Network isolation — all network resource loading (scripts, stylesheets, images, fonts, fetch/XHR, WebSocket) is blocked via CSP and/or WebView request interception on both platforms.
- **FR5:** Basedir-relative local resources — relative URLs in the document resolve against the basedir and load from disk, using the same basedir mechanism as the markdown viewer. Resolution outside the basedir subtree is denied.
- **FR6:** Link handling — clicking an `http(s)` link opens it in the system default browser; the WebView never navigates away from the document. In-page anchors (`#fragment`) work inside the WebView.

### Non-Functional Requirements
- **NFR1 - Security:** No network access from the viewer window; no WebView navigation to any URL other than the loaded document; local file access limited to basedir resolution.
- **NFR2 - Compatibility:** Works on Linux (GTK + WebKitGTK) and Windows (WebView2) through the shared `webview_host` layer. The `html` subcommand compiles and runs in the `--no-default-features` CLI build.

## Implementation Approach

### Architecture

Same pipeline as the existing markdown viewer:

```
emterm html file.html (CLI)
  → validate (.html/.htm, regular file, ≤10MB)
  → base64 chunks (128KB) + OSC 777 (kind=html, basedir, session UUID)
  → stdout (tmux DCS passthrough when applicable)
      ↓ (terminal PTY)
eMterm GUI: OSC 777 handler → ViewerRouter (kind=html)
  → spawn child process (wry WebView, via webview_host)
  → load HTML document as-is (raw, no wrapper styling)
```

### Data Flow

- CLI side: new `src-tauri/src/cli/html.rs` mirroring `markdown.rs` (read → validate → encode → `generate_html_osc`), dispatched from `cli/mod.rs`.
- OSC encoding: new generator in `src-tauri/src/cli/encoding/osc.rs` alongside the markdown/json/yaml generators.
- GUI side: `ViewerRouter::route` gains an `html` arm; the viewer child process receives the payload (HTML bytes + basedir) and loads it into the WebView without passing it through the Markdown renderer or applying `web-shared` styles.
- Security controls (CSP injection / request interception, external-browser link delegation, navigation blocking) are implemented in the viewer host layer for both WebKitGTK and WebView2.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/cli/` — subcommand pipeline (encoding, validation, tmux passthrough, error types)
- `src-tauri/src/viewer_kinds.rs` — `REPLAYABLE_VIEWER_KINDS` SSOT (add `html`; drift test in `viewer/mod.rs` must stay green)
- `src-tauri/src/viewer/` + webview_host — child WebView window plumbing (window management identical to the markdown viewer)
- `src-tauri/src/mux/scrollback_filter.rs` — snapshot rich-content stripping picks up the new kind via the SSOT

**External Dependencies:**
- None new (wry / WebKitGTK / WebView2 already in use)

### File Structure

```
src-tauri/src/cli/html.rs          # new: html subcommand handler
src-tauri/src/cli/mod.rs           # dispatch entry for "html"
src-tauri/src/cli/encoding/osc.rs  # generate_html_osc
src-tauri/src/cli/validation/      # extension check helper (if not inline)
src-tauri/src/viewer_kinds.rs      # + "html"
src-tauri/src/viewer/...           # html routing + viewer window
src-tauri/main.rs                  # child-process entry if a dedicated flag is needed
```

(Exact GUI-side file split is decided in the implementation plan.)

## Test Scenarios

### Unit Tests
- [ ] `generate_html_osc` produces well-formed OSC 777 frames (single/multi chunk, basedir present/absent, sanitized basedir)
- [ ] Extension validation accepts `.html` / `.htm` (case-insensitive), rejects others
- [ ] Size validation rejects > 10MB, accepts exactly 10MB
- [ ] Missing file / directory input produce the correct `CommandError` variants
- [ ] `REPLAYABLE_VIEWER_KINDS` contains `html`; existing drift test covers the router arm

### Integration Tests
- [ ] `emterm html <file>` end-to-end via `tests/cli_subcommands.rs`: valid file emits sequence to stdout, invalid inputs exit non-zero with stderr message
- [ ] CLI-only build check: `cargo check --no-default-features` passes with the new subcommand

### E2E Tests
**Existing E2E tests**: None
**Run command**: Not detected
- [ ] Manual scenario: run `emterm html sample.html` in the GUI — window opens, JS runs, external resources blocked, links open externally

### Edge Cases
- [ ] Empty HTML file — viewer opens with a blank document (no error)
- [ ] HTML referencing `http(s)://` resources — resources do not load; page still renders
- [ ] HTML referencing `../outside.png` (parent traversal above basedir) — resource load denied
- [ ] Executed inside tmux — sequence is DCS-passthrough wrapped

## Security Considerations

- **Network isolation:** CSP (and platform request interception where needed) blocks all remote loads including fetch/XHR/WebSocket.
- **Navigation control:** top-level navigation away from the document is cancelled; `http(s)` targets are delegated to the system browser; other schemes are dropped.
- **Local file access:** only basedir-relative resolution is served; paths escaping the basedir subtree are denied.
- **Input Validation:** extension / file-type / size checks at the CLI; OSC parameter sanitization (semicolons, control chars) as in existing generators.
- **XSS:** not applicable as a boundary — the document's own JS is intentionally executed; isolation is enforced at the network/navigation/filesystem layers instead.

## Error Handling

| Case | Behavior |
|------|----------|
| File not found | stderr message, non-zero exit |
| Not a regular file | stderr message, non-zero exit |
| File > 10MB | stderr message (size + limit), non-zero exit |
| Extension not `.html`/`.htm` | stderr message, non-zero exit |

Error message wording follows the existing `CommandError` / i18n (`t(ja,en)`) conventions in `src-tauri/src/cli/`.

## Success Criteria

- [ ] All functional requirements (FR1–FR6) implemented and tested
- [ ] Unit + integration test scenarios pass
- [ ] `cargo check --no-default-features` passes
- [ ] Linux GUI manual scenario confirmed

## Open Questions

- None

## References

- Requirements: `feature-docs/html-viewer/REQUIREMENTS.md`
- Existing markdown subcommand: `src-tauri/src/cli/markdown.rs`
- Viewer kind SSOT: `src-tauri/src/viewer_kinds.rs`
