# Implementation Plan: HTML Viewer

## Overview

Add an `html` CLI subcommand and a wry-based child HTML viewer window, mirroring the existing Markdown viewer pipeline (CLI → OSC 777 → accumulate → temp payload → child process) while adding raw-document serving, network-blocking CSP, and popup delegation.

## Technology Stack

- **Rust** — CLI subcommand, OSC routing, child window (existing stack; no new crates)
- **wry / webview_host** — the child WebView window (same shared host layer as the Markdown viewer)

## Layer Structure

Follows the existing viewer pipeline layers; dependency direction is strictly downward:

1. **CLI layer** (`src-tauri/src/cli/`) — reads the file, validates, emits OSC 777. Depends only on always-built crates (must compile with `--no-default-features`).
2. **GUI ingest layer** (`src-tauri/src/viewer/`) — parses OSC 777 params, accumulates chunks, writes the payload file, spawns the child.
3. **Child viewer layer** (`src-tauri/src/main.rs` dispatch + `src-tauri/src/viewer/html_window.rs`) — reads the payload, serves the document over a custom protocol, enforces the security policy.
4. **Shared host layer** (`src-tauri/src/webview_host/`) — platform abstraction (GTK+WebKitGTK / winit+WebView2); gains a new-window (popup) handler hook.

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| OSC 777 `html` frame grammar | Wire format between CLI and GUI | Same verb grammar as the markdown kind: `begin` (carries sanitized basedir), `chunk` (seq-numbered base64), `end`. Kind string is `html`. | task0001 (producer), task0002 (consumer) |
| HTML viewer payload file | Hand-off between spawner and child process | JSON temp file containing the decoded HTML text and optional basedir; written mode 0600 / create-new (same discipline as the markdown `ViewerPayload`); child reads then deletes it. No appearance fields — the document renders with its own styles only. | task0002 (writer), task0004 (reader) |
| Basedir resource resolver | Safe basedir-relative file serving | Precondition: a basedir and a percent-decoded relative URL path. Postcondition: returns file bytes + MIME only when the path resolves inside the basedir subtree (lexical normalization + symlink canonicalization re-check, mirroring the existing image resolver); absolute paths, `..` escapes, and disallowed types are denied. Allowlist covers HTML-document needs: raster images, CSS, JS, fonts. SVG stays excluded. | task0003 (owner), task0004 (caller) |
| `viewer_kinds::REPLAYABLE_VIEWER_KINDS` | Viewer-kind SSOT | Gains `html`; the mux scrollback stripper and the route drift test read it. | task0002 |
| webview_host new-window handler | Popup interception hook | New optional handler on the shared host config, implemented on both platforms: receives the requested URL, returns deny-in-WebView; http(s) targets are delegated to the safe external-open helper. | task0004 |

## Conventions

- Kind string / module naming: `html` everywhere — `cli/html.rs`, `viewer/html.rs`, `viewer/html_resolver.rs`, `viewer/html_window.rs`.
- CLI errors reuse the existing `CommandError` variants; user-facing CLI strings are bilingual (En/Ja) in `cli/messages.rs`, following the markdown subcommand's entries (not the English-only image ones).
- OSC parameter values are sanitized with the existing helper (semicolons / control chars stripped) before embedding.
- All cargo commands run from the project root with `--manifest-path` and an explicit `CARGO_TARGET_DIR` (project rule).

## Cross-task Design Decisions

### D1: Dedicated child flag and window module (not the markdown `--viewer`)

The markdown `--viewer` loads the embedded TS bundle (marked + DOMPurify) and injects the document as a JS global — unsuitable for raw HTML. The HTML viewer gets its own child flag (`--html-viewer`) dispatched in `main.rs` and its own window module that serves the raw document directly as the root response of a custom protocol. Affected: task0002 (spawns the flag), task0004 (implements it).

### D2: Custom-protocol serving with relative-URL resolution

The document is served as the root resource of a dedicated custom scheme; relative URLs in the document then resolve against that scheme naturally, and the protocol handler answers them through the basedir resource resolver. On Windows, WebView2 rewrites custom schemes to an `http(s)://{scheme}.localhost/` form — every origin check and the CSP source list must accept both forms (same workaround as the existing viewer's navigation gate). Affected: task0003, task0004.

### D3: Security policy (spec FR4/FR5/FR6/NFR1)

- **Network isolation**: a CSP is attached to the document response permitting only the viewer's own scheme (both platform forms), inline script/style, and `data:` URIs — no remote sources, no connect targets. JavaScript execution itself stays enabled.
- **Navigation**: the navigation gate allows only the viewer scheme; any other target cancels in-WebView navigation and, when http(s), is delegated to the existing safe external-open helper (`links::open_safe_uri`).
- **Popups**: `window.open` / `target=_blank` go through the new webview_host new-window handler with the same delegation rule (deny in-WebView; http(s) → external browser).
- **Filesystem**: only basedir-subtree resolution is served (resolver contract above).

Affected: task0003, task0004.

### D4: Ingest reuses the markdown accumulator pattern

The GUI-side `html` session accumulator mirrors the markdown one (begin/chunk/end, seq-ordered join, single base64 decode) as a separate module rather than generalizing the markdown module — keeps the markdown path untouched. Affected: task0002.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| WebView2 scheme rewrite breaks CSP source matching on Windows | Medium | High (network block silently ineffective or page blank) | List both scheme forms in the CSP; unit-test the header builder; manual Windows check listed in VERIFICATION.md |
| WebKitGTK vs WebView2 differences in new-window handler semantics | Medium | Medium | Keep the handler contract minimal (URL in, deny + optional external open); platform-specific wiring stays inside webview_host |
| CSP alone insufficient for some load paths on a given engine | Low | High | Defense in depth: custom protocol serves only resolver-approved local files; navigation gate blocks document-level escapes |

## Open Questions

- なし
